//! Teste de ponta a ponta de `vscode.scm.createSourceControl` (não é porte do oráculo): uma extensão de exemplo em JavaScript, ativada
//! pelo host local (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), cria uma fonte de controle de código-fonte com
//! caixa de entrada, contador, comandos de barra de status, comando de aceite, provedor de quick diff e grupos com estados de
//! recurso, e o efeito aparece no cliente `MainThreadSCM` do workbench.
//!
//! O lado do ExtHost é o `ExtHostSCM` REAL (portado de `extHostSCM.ts`), ligado pelo host em processo. Não existe ainda um
//! `MainThreadSCM` em `crates/je-workbench/src/api/browser/` (ele depende da camada de SCM da interface), então o lado do workbench é
//! um dublê do `MainThreadSCMShape` ([`Panel`]) que faz o papel da thread principal: anota cada chamada (`$registerSourceControl`,
//! `$updateSourceControl`, `$registerGroups`, `$spliceResourceStates`, `$setInputBox*`, `$unregister*`) numa lista de textos e, quando o
//! teste manda, pede ao ExtHost o que a visão real pediria (`$onInputBoxValueChange`, `$provideOriginalResource`,
//! `$executeResourceCommand`).

#[path = "support/extension_host_api_harness.rs"]
mod harness;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use harness::{run_extension, settled_ok, Workbench};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::marshalling_ids::MarshalledId;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::languages::StringOrMarkdown;
use je_exthost::in_process::IMainThreadBinder;
use je_workbench::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostSCMShape, IMainContext, IconPathDto, InputValidationType, MainContext, MainThreadSCMShape, SCMGroupFeatures, SCMGroupRegistration,
    SCMHistoryItemRefDto, SCMHistoryItemRefsChangeEventDto, SCMProviderFeatures, SCMRawResourceSplices,
};

/// O JavaScript da extensão de exemplo.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__activated = true;
  globalThis.__events = [];
  const log = (text) => globalThis.__events.push(text);
  const command = (id, handler) => context.subscriptions.push(vscode.commands.registerCommand(id, handler));

  command('t.scmCreate', () => {
    const control = vscode.scm.createSourceControl('je-git', 'Je Git', vscode.Uri.file('/repo'));
    globalThis.__control = control;
    control.count = 3;
    control.commitTemplate = 'feat: ';
    control.inputBox.placeholder = 'Mensagem';
    control.inputBox.value = 'inicial';
    control.inputBox.enabled = false;
    control.inputBox.visible = false;
    control.acceptInputCommand = { command: 't.accept', title: 'Commit' };
    control.statusBarCommands = [{ command: 't.branch', title: 'main' }, { command: 't.sync', title: 'sync' }];
    control.quickDiffProvider = { provideOriginalResource: (uri) => vscode.Uri.file('/orig' + uri.path) };
    const staged = control.createResourceGroup('staged', 'Staged');
    staged.hideWhenEmpty = true;
    globalThis.__staged = staged;
    const changes = control.createResourceGroup('changes', 'Changes');
    globalThis.__changes = changes;
    changes.resourceStates = [
      {
        resourceUri: vscode.Uri.file('/repo/b.txt'),
        command: { command: 't.openState', title: 'Abrir', arguments: ['b'] },
        contextValue: 'modified',
        decorations: { strikeThrough: true, tooltip: 'Modificado' },
      },
      { resourceUri: vscode.Uri.file('/repo/a.txt') },
    ];
    return JSON.stringify({ id: control.id, label: control.label, root: control.rootUri.path, count: control.count, groups: [staged.id, changes.id] });
  });
  command('t.scmInput', () => globalThis.__control.inputBox.value);
  command('t.scmProps', () => {
    const control = globalThis.__control;
    return JSON.stringify({
      template: control.commitTemplate,
      accept: control.acceptInputCommand.command,
      status: control.statusBarCommands.map((item) => item.title),
      placeholder: control.inputBox.placeholder,
      enabled: control.inputBox.enabled,
      visible: control.inputBox.visible,
      hide: [globalThis.__staged.hideWhenEmpty, globalThis.__changes.hideWhenEmpty],
      states: globalThis.__changes.resourceStates.map((state) => state.resourceUri.path),
    });
  });
  command('t.openState', (name, preserveFocus) => { log('state:' + name + ',' + preserveFocus); return 'ok'; });
  command('t.scmShrink', () => {
    globalThis.__changes.label = 'Alterações';
    globalThis.__changes.resourceStates = [{ resourceUri: vscode.Uri.file('/repo/a.txt') }];
    return 'ok';
  });
  command('t.scmDispose', () => { globalThis.__control.dispose(); return 'ok'; });
  command('t.scmIdentity', (control, group) => JSON.stringify({ control: control === globalThis.__control, group: group === globalThis.__changes }));
  command('t.scmState', (state) => JSON.stringify({ path: state.resourceUri.path, context: state.contextValue }));
};
"#;

/// Um resumo (em texto) de um `SCMProviderFeatures`: só os campos que o `$updateSourceControl` carrega.
fn describe_features(features: &SCMProviderFeatures) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(count) = &features.count {
        parts.push(format!("count={}", count.map_or_else(|| "none".to_owned(), |count| count.to_string())));
    }
    if let Some(template) = &features.commit_template {
        parts.push(format!("commitTemplate={}", template.clone().unwrap_or_default()));
    }
    if features.accept_input_command.is_some() {
        parts.push("acceptInputCommand".to_owned());
    }
    if let Some(commands) = &features.status_bar_commands {
        parts.push(format!("statusBarCommands={}", commands.len()));
    }
    if let Some(has_provider) = features.has_quick_diff_provider {
        parts.push(format!("quickDiff={has_provider}"));
    }
    parts.join(",")
}

/// O papel da thread principal: o `MainThreadSCMShape` que anota as chamadas.
struct Panel {
    ext_host: RefCell<Option<Rc<dyn ExtHostSCMShape>>>,
    log: RefCell<Vec<String>>,
    /// Os `handle` das fontes registradas.
    handles: RefCell<Vec<u32>>,
    /// O `handle` e o `id` de cada grupo registrado.
    groups: RefCell<Vec<(u32, String)>>,
    /// O texto da URI, o `handle` do grupo e o `handle` de cada recurso que entrou numa lista.
    resources: RefCell<Vec<(String, u32, u32)>>,
}

impl Panel {
    fn new() -> Rc<Self> {
        Rc::new(Self { ext_host: RefCell::new(None), log: RefCell::new(Vec::new()), handles: RefCell::new(Vec::new()), groups: RefCell::new(Vec::new()), resources: RefCell::new(Vec::new()) })
    }

    fn ext_host(&self) -> Rc<dyn ExtHostSCMShape> {
        self.ext_host.borrow().clone().expect("o host já subiu e ligou o dublê")
    }

    fn note(&self, line: String) {
        self.log.borrow_mut().push(line);
    }

    fn lines(&self) -> Vec<String> {
        self.log.borrow().clone()
    }

    fn control_handle(&self) -> u32 {
        *self.handles.borrow().first().expect("a extensão criou uma fonte")
    }

    fn group_id(&self, handle: u32) -> String {
        self.groups.borrow().iter().find(|(known, _)| *known == handle).map(|(_, id)| id.clone()).unwrap_or_default()
    }

    /// Anota as listas de recursos de cada grupo (`prefix:grupo:início+remoções:uris`) e guarda os `handle` deles.
    fn note_splices(&self, prefix: &str, splices: &[SCMRawResourceSplices]) {
        for group in splices {
            let id = self.group_id(group.handle);
            for splice in &group.splices {
                let uris: Vec<String> = splice
                    .to_insert
                    .iter()
                    .map(|resource| {
                        let uri = resource.resource_uri.to_string(false).to_string_lossy();
                        self.resources.borrow_mut().push((uri.clone(), group.handle, resource.handle));
                        uri
                    })
                    .collect();
                self.note(format!("{prefix}:{id}:{}+{}:{}", splice.start, splice.delete_count, uris.join(",")));
            }
        }
    }
}

impl IDisposable for Panel {
    fn dispose(&self) {}
}

impl MainThreadSCMShape for Panel {
    fn rpc_register_source_control(
        &self,
        handle: u32,
        _parent_handle: Option<u32>,
        id: &str,
        label: &str,
        root_uri: Option<URI>,
        _icon_path: Option<IconPathDto>,
        _is_hidden: Option<bool>,
        _input_box_document_uri: URI,
    ) -> Promise<()> {
        self.handles.borrow_mut().push(handle);
        let root = root_uri.map_or_else(|| "none".to_owned(), |root| root.to_string(false).to_string_lossy());
        self.note(format!("register:{id}:{label}:{root}"));
        Promise::resolved(())
    }

    fn rpc_update_source_control(&self, _handle: u32, features: SCMProviderFeatures) -> Promise<()> {
        self.note(format!("update:{}", describe_features(&features)));
        Promise::resolved(())
    }

    fn rpc_unregister_source_control(&self, _handle: u32) -> Promise<()> {
        self.note("unregister".to_owned());
        Promise::resolved(())
    }

    fn rpc_register_groups(&self, _source_control_handle: u32, groups: Vec<SCMGroupRegistration>, splices: Vec<SCMRawResourceSplices>) -> Promise<()> {
        let described: Vec<String> = groups
            .iter()
            .map(|(handle, id, label, features, _)| {
                self.groups.borrow_mut().push((*handle, id.clone()));
                format!("{id}:{label}:hideWhenEmpty={:?}", features.hide_when_empty)
            })
            .collect();
        self.note(format!("groups:{}", described.join(",")));
        self.note_splices("registerSplice", &splices);
        Promise::resolved(())
    }

    fn rpc_update_group(&self, _source_control_handle: u32, _handle: u32, features: SCMGroupFeatures) -> Promise<()> {
        self.note(format!("updateGroup:hideWhenEmpty={:?}", features.hide_when_empty));
        Promise::resolved(())
    }

    fn rpc_update_group_label(&self, _source_control_handle: u32, handle: u32, label: &str) -> Promise<()> {
        self.note(format!("groupLabel:{}={label}", self.group_id(handle)));
        Promise::resolved(())
    }

    fn rpc_unregister_group(&self, _source_control_handle: u32, handle: u32) -> Promise<()> {
        self.note(format!("unregisterGroup:{}", self.group_id(handle)));
        Promise::resolved(())
    }

    fn rpc_splice_resource_states(&self, _source_control_handle: u32, splices: Vec<SCMRawResourceSplices>) -> Promise<()> {
        self.note_splices("spliceStates", &splices);
        Promise::resolved(())
    }

    fn rpc_set_input_box_value(&self, _source_control_handle: u32, value: &str) -> Promise<()> {
        self.note(format!("inputBox.value={value}"));
        Promise::resolved(())
    }

    fn rpc_set_input_box_placeholder(&self, _source_control_handle: u32, placeholder: &str) -> Promise<()> {
        self.note(format!("inputBox.placeholder={placeholder}"));
        Promise::resolved(())
    }

    fn rpc_set_input_box_enablement(&self, _source_control_handle: u32, enabled: bool) -> Promise<()> {
        self.note(format!("inputBox.enabled={enabled}"));
        Promise::resolved(())
    }

    fn rpc_set_input_box_visibility(&self, _source_control_handle: u32, visible: bool) -> Promise<()> {
        self.note(format!("inputBox.visible={visible}"));
        Promise::resolved(())
    }

    fn rpc_show_validation_message(&self, _source_control_handle: u32, _message: StringOrMarkdown, _validation_type: InputValidationType) -> Promise<()> {
        Promise::resolved(())
    }

    fn rpc_set_validation_provider_is_enabled(&self, _source_control_handle: u32, _enabled: bool) -> Promise<()> {
        Promise::resolved(())
    }

    fn rpc_on_did_change_history_provider_current_history_item_refs(
        &self,
        _source_control_handle: u32,
        _history_item_ref: Option<SCMHistoryItemRefDto>,
        _history_item_remote_ref: Option<SCMHistoryItemRefDto>,
        _history_item_base_ref: Option<SCMHistoryItemRefDto>,
    ) -> Promise<()> {
        Promise::resolved(())
    }

    fn rpc_on_did_change_history_provider_history_item_refs(&self, _source_control_handle: u32, _history_item_refs: SCMHistoryItemRefsChangeEventDto) -> Promise<()> {
        Promise::resolved(())
    }

    fn rpc_on_did_change_artifacts(&self, _source_control_handle: u32, _groups: Vec<String>) -> Promise<()> {
        Promise::resolved(())
    }
}

/// Liga o dublê da thread principal ao host: registra o cliente e entrega a ele o proxy do ExtHost.
struct Binder {
    panel: Rc<Panel>,
}

impl IMainThreadBinder for Binder {
    fn bind(&self, context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>> {
        *self.panel.ext_host.borrow_mut() = Some(context.get_proxy(&ExtHostContext::EXT_HOST_SCM));
        let panel: Rc<dyn MainThreadSCMShape> = self.panel.clone();
        context.set(&MainContext::MAIN_THREAD_SCM, panel);
        Vec::new()
    }
}

fn setup(_workbench: &Workbench) -> (Rc<dyn IMainThreadBinder>, Rc<Panel>) {
    let panel = Panel::new();
    (Rc::new(Binder { panel: panel.clone() }), panel)
}

fn run(workbench: &Workbench, command: &str) -> Dynamic {
    workbench.execute(command, Vec::new())
}

fn events(workbench: &Workbench) -> String {
    workbench.probe("JSON.stringify(globalThis.__events)")
}

/// Deixa vencer os `debounce` de 100 ms do `ExtHostSourceControl` (grupos e estados de recurso) e esvazia o laço.
fn settle(workbench: &Workbench) {
    std::thread::sleep(Duration::from_millis(200));
    workbench.pump();
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn create_source_control_registers_it_and_forwards_every_property_to_the_main_thread() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        assert_eq!(
            run(workbench, "t.scmCreate"),
            Dynamic::string(r#"{"id":"je-git","label":"Je Git","root":"/repo","count":3,"groups":["staged","changes"]}"#)
        );
        settle(workbench);

        assert_eq!(
            panel.lines(),
            strings(&[
                "register:je-git:Je Git:file:///repo",
                "update:count=3",
                "update:commitTemplate=feat: ",
                "inputBox.placeholder=Mensagem",
                "inputBox.value=inicial",
                "inputBox.enabled=false",
                "inputBox.visible=false",
                "update:acceptInputCommand",
                "update:statusBarCommands=2",
                "update:quickDiff=true",
                "updateGroup:hideWhenEmpty=Some(true)",
                "groups:staged:Staged:hideWhenEmpty=Some(true),changes:Changes:hideWhenEmpty=None",
                "registerSplice:changes:0+0:file:///repo/a.txt,file:///repo/b.txt",
            ]),
            "a fonte se registra na hora; os grupos e os estados de recurso vão juntos depois do debounce, ordenados por caminho"
        );
        assert_eq!(
            run(workbench, "t.scmProps"),
            Dynamic::string(
                r#"{"template":"feat: ","accept":"t.accept","status":["main","sync"],"placeholder":"Mensagem","enabled":false,"visible":false,"hide":[true,null],"states":["/repo/b.txt","/repo/a.txt"]}"#
            ),
            "a leitura devolve o que a extensão atribuiu (os estados na ordem atribuída)"
        );
    });
}

#[test]
fn input_box_value_and_quick_diff_provider_answer_the_main_thread() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        run(workbench, "t.scmCreate");
        settle(workbench);

        // `$onInputBoxValueChange`: o usuário digitou na caixa de entrada.
        panel.ext_host().rpc_on_input_box_value_change(panel.control_handle(), "digitado");
        workbench.pump();
        assert_eq!(run(workbench, "t.scmInput"), Dynamic::string("digitado"));

        // `$provideOriginalResource`: o `provideOriginalResource` da extensão responde com a URI do original.
        let uri = URI::file("/repo/a.txt", je_platform::resources::host_platform());
        let original = panel.ext_host().rpc_provide_original_resource(panel.control_handle(), &uri, CancellationToken::None);
        workbench.pump();
        let original = settled_ok(&original, "$provideOriginalResource").expect("o provedor devolveu uma URI");
        assert_eq!(original.to_string(false).to_string_lossy(), "file:///orig/repo/a.txt");
    });
}

#[test]
fn resource_command_runs_and_state_changes_reach_the_main_thread_as_splices() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        run(workbench, "t.scmCreate");
        settle(workbench);

        // `$executeResourceCommand`: o usuário clicou no recurso `b.txt`; o comando (que não é `vscode.open`) roda com o `preserveFocus`.
        let (group, resource) = {
            let resources = panel.resources.borrow();
            let (_, group, resource) = resources.iter().find(|(uri, _, _)| uri == "file:///repo/b.txt").expect("b.txt entrou na lista");
            (*group, *resource)
        };
        let executed = panel.ext_host().rpc_execute_resource_command(panel.control_handle(), group, resource, true);
        workbench.pump();
        settled_ok(&executed, "$executeResourceCommand");
        assert_eq!(events(workbench), r#"["state:b,true"]"#, "o comando recebe os argumentos do estado e o preserveFocus");

        // Tirar `b.txt` da lista vira um splice (do índice 1, uma remoção); mudar o rótulo vai na hora.
        assert_eq!(run(workbench, "t.scmShrink"), Dynamic::string("ok"));
        settle(workbench);
        let lines = panel.lines();
        assert_eq!(lines[lines.len() - 2..], strings(&["groupLabel:changes=Alterações", "spliceStates:changes:1+1:"])[..]);
    });
}

#[test]
fn command_arguments_are_the_same_objects_the_extension_created() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        run(workbench, "t.scmCreate");
        settle(workbench);

        let number = |value: u32| Dynamic::from(f64::from(value));
        let control = panel.control_handle();
        let changes = panel.groups.borrow().iter().find(|(_, id)| id == "changes").map(|(handle, _)| *handle).expect("o grupo changes foi registrado");
        let resource = panel.resources.borrow().iter().find(|(uri, _, _)| uri == "file:///repo/b.txt").map(|(_, _, handle)| *handle).expect("b.txt entrou na lista");
        let marshalled = |id: MarshalledId, fields: Vec<(&str, Dynamic)>| {
            let mut entries = vec![("$mid", Dynamic::from(id as i32))];
            entries.extend(fields);
            Dynamic::object(entries)
        };

        // `ScmProvider` e `ScmResourceGroup`: a extensão recebe a mesma fonte e o mesmo grupo que `createSourceControl` e
        // `createResourceGroup` devolveram (`commentThread.value` e equivalentes no original).
        let provider = marshalled(MarshalledId::ScmProvider, vec![("handle", number(control))]);
        let group = marshalled(MarshalledId::ScmResourceGroup, vec![("sourceControlHandle", number(control)), ("groupHandle", number(changes))]);
        assert_eq!(workbench.execute("t.scmIdentity", vec![provider, group]), Dynamic::string(r#"{"control":true,"group":true}"#));

        // `ScmResource`: o estado que a extensão atribuiu ao grupo, com as propriedades dela.
        let state = marshalled(
            MarshalledId::ScmResource,
            vec![("sourceControlHandle", number(control)), ("groupHandle", number(changes)), ("handle", number(resource))],
        );
        assert_eq!(workbench.execute("t.scmState", vec![state]), Dynamic::string(r#"{"path":"/repo/b.txt","context":"modified"}"#));
    });
}

#[test]
fn dispose_unregisters_the_groups_and_the_source_control() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        run(workbench, "t.scmCreate");
        settle(workbench);

        assert_eq!(run(workbench, "t.scmDispose"), Dynamic::string("ok"));
        let lines = panel.lines();
        assert_eq!(lines[lines.len() - 3..], strings(&["unregisterGroup:staged", "unregisterGroup:changes", "unregister"])[..]);
    });
}
