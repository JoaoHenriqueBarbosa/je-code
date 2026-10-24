//! Teste de ponta a ponta de `vscode.WorkspaceEdit` e `vscode.workspace.applyEdit` (não é porte do oráculo): uma extensão de exemplo em
//! JavaScript, ativada pelo host local (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), monta uma edição de
//! workspace (texto, arquivos, metadados), lê o estado dela de volta (`size`, `has`, `get`, `entries`, `toJSON`) e a aplica.
//!
//! O lado do ExtHost é o `ExtHostBulkEdits` REAL (portado de `extHostBulkEdits.ts`), ligado pelo host em processo. O lado do workbench é
//! um dublê do `MainThreadBulkEditsShape` ([`Panel`]) que anota cada edição do DTO que chega em `$tryApplyWorkspaceEdit` e responde o
//! que o teste mandar.

#[path = "support/extension_host_api_harness.rs"]
mod harness;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use harness::{run_extension, Workbench};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_workbench::api::common::ext_host_protocol::{
    IMainContext, IWorkspaceEditDto, MainContext, MainThreadBulkEditsShape, SerializableObjectWithBuffers, WorkspaceEditEntryDto,
};
use je_exthost::in_process::IMainThreadBinder;

/// O JavaScript da extensão de exemplo.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__activated = true;
  globalThis.__result = [];
  const command = (id, handler) => context.subscriptions.push(vscode.commands.registerCommand(id, handler));
  const a = vscode.Uri.file('/w/a.txt');
  const b = vscode.Uri.file('/w/b.txt');
  const range = (startLine, startCharacter, endLine, endCharacter) => new vscode.Range(startLine, startCharacter, endLine, endCharacter);

  command('t.build', () => {
    const edit = new vscode.WorkspaceEdit();
    globalThis.__edit = edit;
    edit.replace(a, range(0, 0, 0, 5), 'olá');
    edit.insert(a, new vscode.Position(1, 2), 'x', { needsConfirmation: true, label: 'Inserir x' });
    edit.delete(b, range(2, 0, 2, 3));
    edit.set(b, [vscode.TextEdit.replace(range(3, 0, 3, 1), 'y'), [vscode.TextEdit.insert(new vscode.Position(4, 0), 'z'), { needsConfirmation: false, label: 'Inserir z' }]]);
    edit.createFile(vscode.Uri.file('/w/new.txt'), { overwrite: true });
    edit.renameFile(vscode.Uri.file('/w/old.txt'), vscode.Uri.file('/w/renamed.txt'), { ignoreIfExists: true });
    edit.deleteFile(vscode.Uri.file('/w/gone.txt'), { recursive: true, ignoreIfNotExists: true });
    return 'ok';
  });
  command('t.state', () => {
    const edit = globalThis.__edit;
    return JSON.stringify({
      size: edit.size,
      hasA: edit.has(a),
      hasC: edit.has(vscode.Uri.file('/w/c.txt')),
      getA: edit.get(a).map((item) => item.newText),
      getB: edit.get(b).map((item) => item.newText),
      entries: edit.entries().map(([uri, edits]) => [uri.path, edits.length]),
      json: JSON.parse(JSON.stringify(edit)).map(([uri, edits]) => [uri.path || uri.external || uri, edits.length]),
    });
  });
  command('t.unset', () => {
    const edit = globalThis.__edit;
    edit.set(b, null);
    return JSON.stringify({ size: edit.size, hasB: edit.has(b), hasA: edit.has(a) });
  });
  command('t.apply', () => {
    vscode.workspace.applyEdit(globalThis.__edit).then((applied) => globalThis.__result.push('applied:' + applied));
    return 'ok';
  });
  command('t.applyRefactoring', () => {
    vscode.workspace.applyEdit(globalThis.__edit, { isRefactoring: true }).then((applied) => globalThis.__result.push('applied:' + applied));
    return 'ok';
  });
  command('t.fresh', () => {
    const edit = new vscode.WorkspaceEdit();
    const other = new vscode.WorkspaceEdit();
    edit.replace(a, range(0, 0, 0, 1), 'q');
    return JSON.stringify({ editSize: edit.size, otherSize: other.size });
  });
  command('t.contents', () => {
    try {
      new vscode.WorkspaceEdit().createFile(a, { contents: new Uint8Array([1]) });
      return 'sem erro';
    } catch (error) {
      return 'erro:' + error.message;
    }
  });
};
"#;

/// Um resumo (em texto) de uma edição do DTO, na ordem em que chegou.
fn describe(edit: &WorkspaceEditEntryDto) -> String {
    match edit {
        WorkspaceEditEntryDto::File(file) => {
            let side = |uri: &Option<je_base::uri::URI>| uri.as_ref().map_or_else(|| "none".to_owned(), |uri| uri.to_string(false).to_string_lossy());
            let options = file.options.as_ref().map_or_else(
                String::new,
                |options| format!(":overwrite={:?},ignoreIfExists={:?},ignoreIfNotExists={:?},recursive={:?}", options.overwrite, options.ignore_if_exists, options.ignore_if_not_exists, options.recursive),
            );
            format!("file:{}->{}{options}", side(&file.old_resource), side(&file.new_resource))
        }
        WorkspaceEditEntryDto::Text(text) => {
            let range = text.text_edit.range;
            let metadata = text.metadata.as_ref().map_or_else(String::new, |metadata| format!(":{}/{}", metadata.needs_confirmation, metadata.label));
            format!(
                "text:{}:{}:{}-{}:{}:{}{metadata}",
                text.resource.to_string(false).to_string_lossy(),
                text.version_id.map_or_else(|| "none".to_owned(), |version| version.to_string()),
                range.start_line_number,
                range.start_column,
                range.end_line_number,
                text.text_edit.text,
            )
        }
        WorkspaceEditEntryDto::Cell(_) => "cell".to_owned(),
    }
}

/// O papel da thread principal: o `MainThreadBulkEditsShape` que anota o que recebe.
struct Panel {
    log: RefCell<Vec<String>>,
    /// O que `$tryApplyWorkspaceEdit` responde.
    answer: Cell<bool>,
}

impl IDisposable for Panel {
    fn dispose(&self) {}
}

impl MainThreadBulkEditsShape for Panel {
    fn rpc_try_apply_workspace_edit(
        &self,
        workspace_edit_dto: SerializableObjectWithBuffers<IWorkspaceEditDto>,
        undo_redo_group_id: Option<f64>,
        is_refactoring: Option<bool>,
    ) -> Promise<bool> {
        let mut log = self.log.borrow_mut();
        log.push(format!("apply:undoGroup={undo_redo_group_id:?},isRefactoring={is_refactoring:?}"));
        log.extend(workspace_edit_dto.value.edits.iter().map(describe));
        Promise::resolved(self.answer.get())
    }
}

/// Liga o dublê da thread principal ao host.
struct Binder {
    panel: Rc<Panel>,
}

impl IMainThreadBinder for Binder {
    fn bind(&self, context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>> {
        let panel: Rc<dyn MainThreadBulkEditsShape> = self.panel.clone();
        context.set(&MainContext::MAIN_THREAD_BULK_EDITS, panel);
        Vec::new()
    }
}

fn setup(_workbench: &Workbench) -> (Rc<dyn IMainThreadBinder>, Rc<Panel>) {
    let panel = Rc::new(Panel { log: RefCell::new(Vec::new()), answer: Cell::new(true) });
    (Rc::new(Binder { panel: panel.clone() }), panel)
}

fn run(workbench: &Workbench, command: &str) -> Dynamic {
    workbench.execute(command, Vec::new())
}

fn results(workbench: &Workbench) -> String {
    workbench.pump();
    workbench.probe("JSON.stringify(globalThis.__result)")
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn workspace_edit_keeps_its_state_and_reads_it_back() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _| {
        assert_eq!(run(workbench, "t.build"), Dynamic::string("ok"));
        assert_eq!(
            run(workbench, "t.state"),
            Dynamic::string(
                r#"{"size":2,"hasA":true,"hasC":false,"getA":["olá","x"],"getB":["","y","z"],"entries":[["/w/a.txt",2],["/w/b.txt",3]],"json":[["/w/a.txt",2],["/w/b.txt",3]]}"#
            ),
            "o objeto muda o receptor: as edições ficam no lado Rust, agrupadas por recurso na ordem da primeira ocorrência"
        );
        assert_eq!(
            run(workbench, "t.unset"),
            Dynamic::string(r#"{"size":1,"hasB":false,"hasA":true}"#),
            "set(uri, null) remove só as edições daquele recurso"
        );
    });
}

#[test]
fn each_workspace_edit_has_its_own_state() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _| {
        assert_eq!(run(workbench, "t.fresh"), Dynamic::string(r#"{"editSize":1,"otherSize":0}"#));
    });
}

#[test]
fn create_file_contents_fail_loudly() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, _| {
        let result = run(workbench, "t.contents");
        let Dynamic::String(text) = result else { panic!("o comando devolve texto") };
        assert!(text.to_string_lossy().starts_with("erro:"), "contents não atravessa a fronteira e não pode ser ignorado em silêncio");
    });
}

#[test]
fn apply_edit_sends_every_entry_in_order_to_the_main_thread() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        run(workbench, "t.build");
        assert_eq!(run(workbench, "t.apply"), Dynamic::string("ok"));
        assert_eq!(results(workbench), r#"["applied:true"]"#);
        assert_eq!(
            *panel.log.borrow(),
            strings(&[
                "apply:undoGroup=None,isRefactoring=Some(false)",
                "text:file:///w/a.txt:none:1-1:1-6:olá",
                "text:file:///w/a.txt:none:2-3:2-3:x:true/Inserir x",
                "text:file:///w/b.txt:none:3-1:3-4:",
                "text:file:///w/b.txt:none:4-1:4-2:y",
                "text:file:///w/b.txt:none:5-1:5-1:z:false/Inserir z",
                "file:none->file:///w/new.txt:overwrite=Some(true),ignoreIfExists=None,ignoreIfNotExists=None,recursive=None",
                "file:file:///w/old.txt->file:///w/renamed.txt:overwrite=None,ignoreIfExists=Some(true),ignoreIfNotExists=None,recursive=None",
                "file:file:///w/gone.txt->none:overwrite=None,ignoreIfExists=None,ignoreIfNotExists=Some(true),recursive=Some(true)",
            ]),
            "um documento que a extensão não abriu vai sem versão; a ordem é a das chamadas"
        );
    });
}

#[test]
fn apply_edit_forwards_the_refactoring_flag_and_the_rejection() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, panel| {
        panel.answer.set(false);
        run(workbench, "t.build");
        run(workbench, "t.applyRefactoring");
        assert_eq!(results(workbench), r#"["applied:false"]"#, "o resultado da thread principal volta para a promessa da extensão");
        assert_eq!(panel.log.borrow()[0], "apply:undoGroup=None,isRefactoring=Some(true)");
    });
}
