//! Teste de ponta a ponta de `vscode.workspace` (não é porte do oráculo): uma extensão de exemplo em JavaScript, ativada pelo host
//! local (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), exerce a API de verdade, e o efeito aparece no lado do
//! workbench.
//!
//! - **Pastas, nome, `rootPath`, confiança e `asRelativePath`/`getWorkspaceFolder`** vêm do `IWorkspaceData` que o lado do
//!   workbench entrega por `$initializeWorkspace` ao `ExtHostWorkspace` portado; `onDidChangeWorkspaceFolders` e
//!   `onDidGrantWorkspaceTrust` disparam quando o workbench manda `$acceptWorkspaceData` e `$onDidGrantWorkspaceTrust`.
//! - **Configuração** passa pelo `MainThreadConfiguration` REAL: os dados iniciais (`$initializeConfiguration`) alimentam
//!   `getConfiguration().get/has/inspect`, `update` chega ao `IConfigurationService` do workbench, e uma mudança de configuração do
//!   workbench dispara `onDidChangeConfiguration` na extensão com o `affectsConfiguration` certo.
//!
//! Os dublês (`WorkspaceClient` no lugar do `MainThreadWorkspace`, `StaticConfigurationService` semeado e o
//! `MainThreadMessageService` real que o `ExtHostWorkspace` exige ao nascer) vêm de `support/extension_host_workspace_doubles.rs`.

#[path = "support/extension_host_api_harness.rs"]
mod harness;
#[path = "support/extension_host_workspace_doubles.rs"]
mod workspace_doubles;

use std::rc::Rc;

use harness::{run_extension, Workbench};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::JsString;
use je_exthost::in_process::IMainThreadBinder;
use je_platform::configuration::models::{Configuration, ConfigurationChangeEvent};
use je_platform::configuration::{ConfigurationChange, ConfigurationData, ConfigurationTarget};
use je_platform::log::NullLogService;
use je_workbench::api::browser::main_thread_message_service::MainThreadMessageService;
use je_workbench::api::common::ext_host_protocol::{IMainContext, IWorkspaceData};
use workspace_doubles::{bind_workspace_clients, message_service, workspace_data, StaticConfigurationService, WorkspaceClient};

/// O JavaScript da extensão de exemplo: cada comando devolve um valor simples que o teste confere.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__activated = true;
  const command = (id, handler) => context.subscriptions.push(vscode.commands.registerCommand(id, handler));
  command('t.folders', () => (vscode.workspace.workspaceFolders || []).map((folder) => ({
    name: folder.name, path: folder.uri.fsPath, scheme: folder.uri.scheme, index: folder.index, isUri: folder.uri instanceof vscode.Uri,
  })));
  command('t.info', () => ({
    name: vscode.workspace.name, rootPath: vscode.workspace.rootPath, trusted: vscode.workspace.isTrusted,
  }));
  command('t.relative', (path) => vscode.workspace.asRelativePath(path));
  command('t.folderOf', (path) => {
    const folder = vscode.workspace.getWorkspaceFolder(vscode.Uri.file(path));
    return folder ? folder.name : null;
  });
  command('t.config', () => {
    const config = vscode.workspace.getConfiguration('exthost');
    const inspected = config.inspect('greeting');
    return {
      has: config.has('greeting'), missing: config.has('nope'), greeting: config.get('greeting'), fallback: config.get('nope', 'padrão'),
      nested: config.get('nested.level'), viaProperty: config.greeting, inspectedKey: inspected ? inspected.key : null,
    };
  });
  command('t.update', async () => {
    await vscode.workspace.getConfiguration('exthost').update('greeting', 'oi', vscode.ConfigurationTarget.Global);
    return 'atualizado';
  });
  command('t.watch', () => {
    globalThis.__changes = [];
    globalThis.__folderEvents = [];
    globalThis.__trust = [];
    context.subscriptions.push(vscode.workspace.onDidChangeConfiguration((event) => {
      globalThis.__changes.push(event.affectsConfiguration('exthost.greeting') + ':' + event.affectsConfiguration('outra'));
    }));
    context.subscriptions.push(vscode.workspace.onDidChangeWorkspaceFolders((event) => {
      globalThis.__folderEvents.push(event.added.map((f) => f.name).join(',') + '|' + event.removed.map((f) => f.name).join(','));
    }));
    context.subscriptions.push(vscode.workspace.onDidGrantWorkspaceTrust(() => globalThis.__trust.push(vscode.workspace.isTrusted)));
    return 'observando';
  });
};
"#;

/// Os dados em que `exthost.greeting` vale `greeting` e `exthost.nested.level` vale 3, na camada do usuário.
fn configuration_data(greeting: &str) -> ConfigurationData {
    let contents = Dynamic::object([(
        "exthost",
        Dynamic::object([("greeting", Dynamic::string(greeting)), ("nested", Dynamic::object([("level", Dynamic::Number(3.0))]))]),
    )]);
    workspace_doubles::configuration_data(workspace_doubles::model(contents, &["exthost.greeting", "exthost.nested.level"]))
}

/// O que o teste observa do lado do workbench depois do `bind`.
struct Observed {
    configuration_service: Rc<StaticConfigurationService>,
    workspace_client: Rc<std::cell::RefCell<Option<Rc<WorkspaceClient>>>>,
}

/// Liga `MainThreadWorkspace` (dublê), `MainThreadConfiguration` e `MainThreadMessageService` (reais) ao host.
struct Binder {
    configuration_service: Rc<StaticConfigurationService>,
    workspace_client: Rc<std::cell::RefCell<Option<Rc<WorkspaceClient>>>>,
    message_service: Rc<MainThreadMessageService>,
    initial_data: IWorkspaceData,
}

impl IMainThreadBinder for Binder {
    fn bind(&self, context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>> {
        let clients = bind_workspace_clients(&context, self.message_service.clone(), self.initial_data.clone(), self.configuration_service.clone());
        *self.workspace_client.borrow_mut() = Some(clients.workspace);
        clients.disposables
    }
}

fn text(value: &Dynamic, key: &str) -> String {
    match value.get_str(key) {
        Dynamic::String(text) => text.to_string_lossy(),
        other => panic!("esperava texto em '{key}', veio {other:?}"),
    }
}

fn setup(workbench: &Workbench) -> (Rc<dyn IMainThreadBinder>, Observed) {
    let configuration_service = StaticConfigurationService::seeded(configuration_data("olá"));
    let workspace_client = Rc::new(std::cell::RefCell::new(None));
    let binder = Binder {
        configuration_service: configuration_service.clone(),
        workspace_client: workspace_client.clone(),
        message_service: message_service(workbench),
        initial_data: workspace_data(&["/work/app", "/work/lib"]),
    };
    (Rc::new(binder), Observed { configuration_service, workspace_client })
}

#[test]
fn workspace_folders_name_root_path_and_relative_paths_come_from_the_workbench() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _observed| {
        let folders = workbench.execute("t.folders", Vec::new());
        let Dynamic::Array(folders) = folders else { panic!("workspaceFolders deveria ser uma lista") };
        let folders = folders.to_vec();
        assert_eq!(folders.len(), 2);
        assert_eq!(text(&folders[0], "name"), "app");
        assert_eq!(text(&folders[0], "path"), "/work/app");
        assert_eq!(text(&folders[0], "scheme"), "file");
        assert_eq!(folders[0].get_str("index"), Dynamic::Number(0.0));
        assert_eq!(folders[0].get_str("isUri"), Dynamic::Bool(true), "uri deveria ser uma instância de vscode.Uri");
        assert_eq!(text(&folders[1], "name"), "lib");

        let info = workbench.execute("t.info", Vec::new());
        assert_eq!(text(&info, "name"), "meu-projeto");
        assert_eq!(text(&info, "rootPath"), "/work/app", "rootPath é a primeira pasta");
        assert_eq!(info.get_str("trusted"), Dynamic::Bool(false));

        // Com duas pastas, `asRelativePath` inclui o nome da pasta (`includeWorkspace` padrão), como o original.
        assert_eq!(workbench.execute("t.relative", vec![Dynamic::string("/work/lib/src/main.rs")]), Dynamic::string("lib/src/main.rs"));
        assert_eq!(workbench.execute("t.relative", vec![Dynamic::string("/fora/arquivo.txt")]), Dynamic::string("/fora/arquivo.txt"));
        assert_eq!(workbench.execute("t.folderOf", vec![Dynamic::string("/work/lib/x.rs")]), Dynamic::string("lib"));
        assert_eq!(workbench.execute("t.folderOf", vec![Dynamic::string("/fora/x.rs")]), Dynamic::Null);
    });
}

#[test]
fn configuration_reads_the_workbench_data_and_update_reaches_the_configuration_service() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        let config = workbench.execute("t.config", Vec::new());
        assert_eq!(config.get_str("has"), Dynamic::Bool(true));
        assert_eq!(config.get_str("missing"), Dynamic::Bool(false));
        assert_eq!(text(&config, "greeting"), "olá");
        assert_eq!(text(&config, "fallback"), "padrão");
        assert_eq!(config.get_str("nested"), Dynamic::Number(3.0));
        assert_eq!(text(&config, "viaProperty"), "olá", "o valor também é propriedade do objeto");
        assert_eq!(text(&config, "inspectedKey"), "exthost.greeting");

        assert_eq!(workbench.execute("t.update", Vec::new()), Dynamic::string("atualizado"));
        let calls = observed.configuration_service.inner.update_value_calls();
        assert_eq!(calls.len(), 1, "o update da extensão deveria chegar ao IConfigurationService do workbench");
        assert_eq!(calls[0].key, "exthost.greeting");
        assert_eq!(calls[0].value, Dynamic::string("oi"));
        assert_eq!(calls[0].target, Some(ConfigurationTarget::User), "ConfigurationTarget.Global é a camada do usuário");
    });
}

#[test]
fn change_events_fire_for_configuration_workspace_folders_and_trust() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, observed| {
        assert_eq!(workbench.execute("t.watch", Vec::new()), Dynamic::string("observando"));

        // O workbench muda `exthost.greeting`: a extensão recebe o evento com o `affectsConfiguration` certo.
        let log = NullLogService::shared();
        let data = configuration_data("novo");
        observed.configuration_service.set_data(data.clone());
        let change = ConfigurationChange { keys: vec![JsString::from("exthost.greeting")], overrides: Vec::new() };
        let event = ConfigurationChangeEvent::new(change, None, Rc::new(Configuration::parse(&data, log.clone())), None, log);
        observed.configuration_service.inner.on_did_change_configuration_emitter.fire(Rc::new(event));
        workbench.pump();
        assert_eq!(workbench.probe("JSON.stringify(globalThis.__changes)"), r#"["true:false"]"#);
        let after = workbench.execute("t.config", Vec::new());
        assert_eq!(text(&after, "greeting"), "novo", "a leitura seguinte vê o valor novo");

        // O workbench troca as pastas: `onDidChangeWorkspaceFolders` entrega o que entrou e o que saiu.
        let client = observed.workspace_client.borrow().clone().expect("o cliente de workspace foi ligado");
        client.publish(workspace_data(&["/work/app", "/work/docs"]));
        workbench.pump();
        assert_eq!(workbench.probe("JSON.stringify(globalThis.__folderEvents)"), r#"["docs|lib"]"#);
        let folders = workbench.execute("t.folders", Vec::new());
        let Dynamic::Array(folders) = folders else { panic!("workspaceFolders deveria ser uma lista") };
        assert_eq!(folders.len(), 2);

        // A confiança concedida chega, e `isTrusted` passa a valer `true` já dentro do listener.
        client.grant_trust();
        workbench.pump();
        assert_eq!(workbench.probe("JSON.stringify(globalThis.__trust)"), "[true]");
    });
}
