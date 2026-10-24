//! Teste de ponta a ponta de `vscode.workspace.createFileSystemWatcher` e dos eventos `onDid/WillCreate/Delete/RenameFiles` (não é porte
//! do oráculo): uma extensão de exemplo em JavaScript, ativada pelo host local (`AbstractExtensionService` + `LocalExtensionHost` +
//! `RPCProtocol`), cria observadores e escuta as operações de arquivo, e o efeito aparece nos dois lados.
//!
//! O lado do ExtHost é o `ExtHostFileSystemEventService` REAL (portado de `extHostFileSystemEventService.ts`), ligado pelo host em
//! processo. O lado do workbench é um dublê do `MainThreadFileSystemEventServiceShape` ([`Panel`]) que anota cada `$watch` e
//! `$unwatch` e guarda o proxy do ExtHost, por onde o teste entrega `$onFileEvent`, `$onDidRunFileOperation` e
//! `$onWillRunFileOperation`. As pastas e a configuração que o `createFileSystemWatcher` lê vêm dos dublês de workspace em
//! `support/extension_host_workspace_doubles.rs`.

#[path = "support/extension_host_api_harness.rs"]
mod harness;
#[path = "support/extension_host_workspace_doubles.rs"]
mod workspace_doubles;

use std::cell::RefCell;
use std::rc::Rc;

use harness::{run_extension, settled_ok, Workbench};
use je_base::cancellation::CancellationToken;
use je_base::lifecycle::IDisposable;
use je_base::uri::URI;
use je_exthost::in_process::IMainThreadBinder;
use je_platform::files::files::{FileOperation, WatchOptions};
use je_workbench::api::browser::main_thread_message_service::MainThreadMessageService;
use je_workbench::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostFileSystemEventServiceShape, FileSystemEvents, IMainContext, MainContext, MainThreadFileSystemEventServiceShape, SourceTargetPair,
    WorkspaceEditEntryDto,
};

/// O JavaScript da extensão de exemplo.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__activated = true;
  globalThis.__watch = [];
  globalThis.__files = [];
  globalThis.__will = [];
  const command = (id, handler) => context.subscriptions.push(vscode.commands.registerCommand(id, handler));
  const pattern = () => new vscode.RelativePattern(vscode.Uri.file('/w'), '**/*.txt');
  const flags = (watcher) => JSON.stringify([watcher.ignoreCreateEvents, watcher.ignoreChangeEvents, watcher.ignoreDeleteEvents]);
  const observe = (watcher, prefix) => {
    const log = (kind) => (uri) => globalThis.__watch.push(prefix + kind + ':' + uri.path);
    context.subscriptions.push(watcher.onDidCreate(log('create')), watcher.onDidChange(log('change')), watcher.onDidDelete(log('delete')));
  };
  const paths = (files) => files.map((uri) => uri.path).join(',');
  const renames = (files) => files.map((file) => file.oldUri.path + '>' + file.newUri.path).join(',');

  command('t.watch', () => {
    const watcher = vscode.workspace.createFileSystemWatcher(pattern());
    globalThis.__watcher = watcher;
    observe(watcher, '');
    return flags(watcher);
  });
  command('t.watchNoChange', () => {
    const watcher = vscode.workspace.createFileSystemWatcher(pattern(), 0, 1, '');
    observe(watcher, 'b:');
    return flags(watcher);
  });
  command('t.watchNothing', () => flags(vscode.workspace.createFileSystemWatcher(pattern(), true, true, true)));
  command('t.disposeWatcher', () => {
    globalThis.__watcher.dispose();
    return 'ok';
  });

  command('t.listen', () => {
    const { workspace } = vscode;
    context.subscriptions.push(workspace.onDidCreateFiles((event) => globalThis.__files.push('create:' + paths(event.files))));
    context.subscriptions.push(workspace.onDidDeleteFiles((event) => globalThis.__files.push('delete:' + paths(event.files))));
    context.subscriptions.push(workspace.onDidRenameFiles((event) => globalThis.__files.push('rename:' + renames(event.files))));
    return 'ok';
  });

  command('t.will', () => {
    const { workspace } = vscode;
    const insertion = (path, line, text) => {
      const edit = new vscode.WorkspaceEdit();
      edit.insert(vscode.Uri.file(path), new vscode.Position(line, 0), text);
      return edit;
    };
    context.subscriptions.push(workspace.onWillCreateFiles((event) => {
      globalThis.__event = event;
      globalThis.__will.push('create:' + paths(event.files) + ':' + event.token.isCancellationRequested);
      event.waitUntil(Promise.resolve(insertion('/w/a.txt', 0, 'hdr')));
    }));
    context.subscriptions.push(workspace.onWillCreateFiles((event) => event.waitUntil(Promise.resolve(42))));
    context.subscriptions.push(workspace.onWillRenameFiles((event) => {
      globalThis.__will.push('rename:' + renames(event.files));
      event.waitUntil(Promise.resolve(insertion('/w/r.txt', 1, 'ren')));
    }));
    context.subscriptions.push(workspace.onWillDeleteFiles((event) => globalThis.__will.push('delete:' + paths(event.files))));
    return 'ok';
  });
  command('t.lateWaitUntil', () => {
    try {
      globalThis.__event.waitUntil(Promise.resolve());
      return 'sem erro';
    } catch (error) {
      return 'erro:' + error.message;
    }
  });
};
"#;

/// O papel da thread principal: o `MainThreadFileSystemEventServiceShape` que anota o que recebe e guarda o proxy do ExtHost.
struct Panel {
    log: RefCell<Vec<String>>,
    /// As sessões de `$watch`, na ordem; o `$unwatch` é anotado pelo índice (a sessão é sorteada).
    sessions: RefCell<Vec<f64>>,
    ext_host: RefCell<Option<Rc<dyn ExtHostFileSystemEventServiceShape>>>,
}

impl IDisposable for Panel {
    fn dispose(&self) {}
}

impl MainThreadFileSystemEventServiceShape for Panel {
    fn rpc_watch(&self, extension_id: &str, session: f64, resource: &URI, opts: WatchOptions, correlate: bool) {
        self.sessions.borrow_mut().push(session);
        self.log.borrow_mut().push(format!(
            "watch:{extension_id}:{}:recursive={}:excludes={}:correlate={correlate}",
            resource.to_string(false).to_string_lossy(),
            opts.recursive,
            opts.excludes.len(),
        ));
    }

    fn rpc_unwatch(&self, session: f64) {
        let index = self.sessions.borrow().iter().position(|known| *known == session);
        self.log.borrow_mut().push(format!("unwatch:{index:?}"));
    }
}

impl Panel {
    fn ext_host(&self) -> Rc<dyn ExtHostFileSystemEventServiceShape> {
        self.ext_host.borrow().clone().expect("o binder já guardou o proxy do ExtHost")
    }

    fn send_file_event(&self, created: &[&str], changed: &[&str], deleted: &[&str]) {
        let uris = |paths: &[&str]| -> Vec<URI> { paths.iter().map(|path| file_uri(path)).collect() };
        self.ext_host().rpc_on_file_event(FileSystemEvents { session: None, created: uris(created), changed: uris(changed), deleted: uris(deleted) });
    }
}

fn file_uri(path: &str) -> URI {
    URI::file(path, je_platform::resources::host_platform())
}

/// Um item de operação de arquivo: `source` só existe em renomeação e cópia.
fn pair(source: Option<&str>, target: &str) -> SourceTargetPair {
    SourceTargetPair { source: source.map(file_uri), target: file_uri(target) }
}

/// Liga o dublê da thread principal e os clientes de workspace ao host.
struct Binder {
    panel: Rc<Panel>,
    message_service: Rc<MainThreadMessageService>,
}

impl IMainThreadBinder for Binder {
    fn bind(&self, context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>> {
        *self.panel.ext_host.borrow_mut() = Some(context.get_proxy(&ExtHostContext::EXT_HOST_FILE_SYSTEM_EVENT_SERVICE));
        let panel: Rc<dyn MainThreadFileSystemEventServiceShape> = self.panel.clone();
        context.set(&MainContext::MAIN_THREAD_FILE_SYSTEM_EVENT_SERVICE, panel);
        let mut clients = workspace_doubles::bind_workspace_clients(&context, self.message_service.clone(), workspace_doubles::workspace_data(&["/w"]));
        clients.push(self.panel.clone());
        clients
    }
}

fn setup(workbench: &Workbench) -> (Rc<dyn IMainThreadBinder>, Rc<Panel>) {
    let panel = Rc::new(Panel { log: RefCell::new(Vec::new()), sessions: RefCell::new(Vec::new()), ext_host: RefCell::new(None) });
    (Rc::new(Binder { panel: panel.clone(), message_service: workspace_doubles::message_service(workbench) }), panel)
}

fn run(workbench: &Workbench, command: &str) -> String {
    match workbench.execute(command, Vec::new()) {
        je_base::dynamic::Dynamic::String(text) => text.to_string_lossy(),
        other => panic!("o comando {command} devolve texto, veio {other:?}"),
    }
}

fn global(workbench: &Workbench, name: &str) -> String {
    workbench.pump();
    workbench.probe(&format!("JSON.stringify(globalThis.{name})"))
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

/// O que um `$onWillRunFileOperation` devolveu: os nomes das extensões e uma linha por edição de texto (`recurso:linha:texto`).
fn participation(workbench: &Workbench, panel: &Panel, operation: FileOperation, files: Vec<SourceTargetPair>) -> Option<(Vec<String>, Vec<String>)> {
    let promise = panel.ext_host().rpc_on_will_run_file_operation(operation, files, 1000.0, CancellationToken::None);
    workbench.pump();
    settled_ok(&promise, "$onWillRunFileOperation").map(|participation| {
        let edits = participation
            .edit
            .edits
            .iter()
            .map(|edit| match edit {
                WorkspaceEditEntryDto::Text(text) => format!("{}:{}:{}", text.resource.to_string(false).to_string_lossy(), text.text_edit.range.start_line_number, text.text_edit.text),
                _ => panic!("o teste só devolve edições de texto"),
            })
            .collect();
        (participation.extension_names, edits)
    })
}

#[test]
fn watcher_asks_the_workbench_to_watch_and_delivers_only_matching_events() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        assert_eq!(run(workbench, "t.watch"), "[false,false,false]");
        assert_eq!(
            *panel.log.borrow(),
            strings(&["watch:je.exthost:file:///w:recursive=true:excludes=0:correlate=false"]),
            "um padrão relativo com `**` vira observação recursiva da pasta base, sem correlação"
        );
        panel.send_file_event(&["/w/a.txt", "/w/c.md", "/other/d.txt"], &["/w/sub/b.txt"], &["/w/e.txt"]);
        assert_eq!(
            global(workbench, "__watch"),
            r#"["create:/w/a.txt","change:/w/sub/b.txt","delete:/w/e.txt"]"#,
            "só os eventos sob a base que casam com o glob chegam aos três eventos"
        );
    });
}

#[test]
fn disposing_the_watcher_unwatches_and_silences_it() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        run(workbench, "t.watch");
        assert_eq!(run(workbench, "t.disposeWatcher"), "ok");
        assert_eq!(panel.log.borrow().last().map(String::as_str), Some("unwatch:Some(0)"), "o dispose desfaz a observação da sessão aberta");
        panel.send_file_event(&["/w/a.txt"], &[], &[]);
        assert_eq!(global(workbench, "__watch"), "[]", "depois do dispose nenhum evento chega");
    });
}

#[test]
fn ignore_flags_follow_boolean_coercion_and_skip_ignored_events() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        assert_eq!(run(workbench, "t.watchNoChange"), "[false,true,false]", "0, 1 e '' passam por Boolean() como no original");
        panel.send_file_event(&["/w/a.txt"], &["/w/a.txt"], &["/w/a.txt"]);
        assert_eq!(global(workbench, "__watch"), r#"["b:create:/w/a.txt","b:delete:/w/a.txt"]"#);

        let watches = panel.log.borrow().len();
        assert_eq!(run(workbench, "t.watchNothing"), "[true,true,true]");
        assert_eq!(panel.log.borrow().len(), watches, "ignorar os três eventos dispensa a observação no workbench");
    });
}

#[test]
fn did_run_file_operation_events_carry_the_files() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        assert_eq!(run(workbench, "t.listen"), "ok");
        let ext_host = panel.ext_host();
        ext_host.rpc_on_did_run_file_operation(FileOperation::Create, vec![pair(None, "/w/new.txt")]);
        ext_host.rpc_on_did_run_file_operation(FileOperation::Copy, vec![pair(Some("/w/orig.txt"), "/w/copy.txt")]);
        ext_host.rpc_on_did_run_file_operation(FileOperation::Delete, vec![pair(None, "/w/gone.txt"), pair(None, "/w/gone2.txt")]);
        ext_host.rpc_on_did_run_file_operation(FileOperation::Move, vec![pair(Some("/w/old.txt"), "/w/renamed.txt")]);
        ext_host.rpc_on_did_run_file_operation(FileOperation::Write, vec![pair(None, "/w/saved.txt")]);
        assert_eq!(
            global(workbench, "__files"),
            r#"["create:/w/new.txt","create:/w/copy.txt","delete:/w/gone.txt,/w/gone2.txt","rename:/w/old.txt>/w/renamed.txt"]"#,
            "copiar conta como criar e gravar não gera evento"
        );
    });
}

#[test]
fn will_run_file_operation_collects_only_workspace_edits_from_wait_until() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        assert_eq!(run(workbench, "t.will"), "ok");

        let created = participation(workbench, &panel, FileOperation::Create, vec![pair(None, "/w/new.txt")]).expect("o participante devolveu uma edição");
        assert_eq!(created.0, strings(&["je.exthost"]), "o nome da extensão que participou");
        assert_eq!(created.1, strings(&["file:///w/a.txt:1:hdr"]), "o valor 42 do segundo ouvinte é ignorado, só a WorkspaceEdit entra");

        let renamed = participation(workbench, &panel, FileOperation::Move, vec![pair(Some("/w/old.txt"), "/w/renamed.txt")]).expect("o participante devolveu uma edição");
        assert_eq!(renamed.1, strings(&["file:///w/r.txt:2:ren"]));

        assert!(participation(workbench, &panel, FileOperation::Delete, vec![pair(None, "/w/gone.txt")]).is_none(), "sem waitUntil não há participação");
        assert!(participation(workbench, &panel, FileOperation::Write, vec![pair(None, "/w/saved.txt")]).is_none(), "gravar não tem participante");

        assert_eq!(global(workbench, "__will"), r#"["create:/w/new.txt:false","rename:/w/old.txt>/w/renamed.txt","delete:/w/gone.txt"]"#);
    });
}

#[test]
fn wait_until_outside_the_dispatch_throws() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        run(workbench, "t.will");
        participation(workbench, &panel, FileOperation::Create, vec![pair(None, "/w/new.txt")]);
        assert_eq!(run(workbench, "t.lateWaitUntil"), "erro:waitUntil can NOT be called asynchronous");
    });
}
