//! Teste de ponta a ponta de `vscode.notebooks` e dos membros de notebook de `vscode.workspace` (não é porte do oráculo): uma extensão de
//! exemplo em JavaScript, ativada pelo host local (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), abre e cria
//! notebooks, lê o notebook e as células, cria um controlador de notebook que executa células (saída, ordem de execução, término), muda a
//! afinidade e descarta o controlador, registra um provedor de itens da barra de status das células e um serializador de notebook (por
//! referência, com `Uint8Array`), escuta os eventos de mudança, de gravação e de pré-gravação (`waitUntil`) dos documentos e os editores de
//! notebook (`showNotebookDocument`, `activeNotebookEditor`, os eventos); o efeito aparece nos clientes
//! `MainThreadNotebook*` do workbench.
//!
//! O lado do ExtHost é o `ExtHostNotebookController`, o `ExtHostNotebookDocuments` e o `ExtHostNotebookKernels` REAIS (portados de
//! `extHostNotebook.ts`, `extHostNotebookDocuments.ts` e `extHostNotebookKernels.ts`), ligados pelo host em processo. Não existe ainda um
//! `MainThreadNotebook*` em `crates/je-workbench/src/api/browser/` (eles dependem da camada de notebooks da interface), então o lado do
//! workbench é um dublê dos quatro `MainThreadNotebook*Shape` ([`Panel`]) que anota cada chamada e faz o papel da thread principal quando o
//! teste manda: entrega ao ExtHost o delta de documentos (`$acceptDocumentAndEditorsDelta`), a associação do notebook ao controlador
//! (`$acceptNotebookAssociation`), a execução das células (`$executeCells`) e o pedido dos itens da barra de status
//! (`$provideNotebookCellStatusBarItems`) pelos proxies `ExtHostNotebookShape` e `ExtHostNotebookKernelsShape` que o contexto em processo
//! entrega. Os documentos de texto (cada célula é um) vêm do mesmo suporte dos testes de `languages`, e a célula do notebook vem do suporte
//! de testes de `ExtHostNotebook` (`cell_dto`).

#[allow(dead_code)]
#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/extension_host_api_harness.rs"]
mod harness;
#[allow(dead_code, unused_imports)]
#[path = "support/main_thread_support.rs"]
mod main_thread_support;

/// O suporte de editor do `je-editor/tests/support` que o `TestCodeEditorService` precisa, no mesmo módulo pai (os arquivos se referem uns
/// aos outros por `super::`).
#[allow(dead_code, unused_imports)]
#[path = "../../je-editor/tests/support"]
mod editor_support {
    pub mod editor_test_services;
    pub mod test_clipboard_service;
    pub mod test_code_editor;
    pub mod test_code_editor_service;
    pub mod test_color_theme_service;
    pub mod test_configuration;
    pub mod test_instantiation_service;
    pub mod test_language_configuration_service;
    pub mod test_notification_service;
    pub mod test_text_model;
    pub mod test_theme_service;
}
#[path = "support/extension_host_api_languages_support.rs"]
mod languages_support;
#[allow(dead_code, unused_imports)]
#[path = "support/ext_host_workspace_support.rs"]
mod ext_host_workspace_support;
#[allow(dead_code, unused_imports)]
#[path = "support/ext_host_notebook_support.rs"]
mod ext_host_notebook_support;

use std::cell::RefCell;
use std::rc::Rc;

use ext_host_notebook_support::cell_dto;
use harness::{run_extension, settled_ok, Workbench};
use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_exthost::in_process::IMainThreadBinder;
use je_workbench::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostNotebookDocumentSaveParticipantShape, ExtHostNotebookDocumentsShape, ExtHostNotebookEditorsShape, ExtHostNotebookKernelsShape, ExtHostNotebookShape, ICellExecuteUpdateDto,
    ICellExecutionCompleteDto, IMainContext, INotebookDocumentShowOptions, INotebookDocumentsAndEditorsDelta, INotebookEditorAddData, INotebookEditorPropertiesChangeData,
    INotebookEditorViewColumnInfo, INotebookKernelDto2, INotebookModelAddedData, INotebookSelectionChangeEvent, INotebookVisibleRangesEvent, IWorkspaceEditDto, MainContext, MainThreadBulkEditsShape,
    MainThreadNotebookDocumentsShape, MainThreadNotebookEditorsShape,
    MainThreadNotebookKernelsShape, MainThreadNotebookShape, NotebookCellDataDto, NotebookCellsChangedEventDto, NotebookCreateOptions, NotebookDataDto, NotebookOutputItemDto,
    NotebookRawContentEventDto, SerializableObjectWithBuffers, VariablesResult,
};
use je_workbench::api::common::ext_host_type_converters::CellRange;
use je_workbench::api::common::ext_host_types::{NotebookControllerAffinity2, NotebookEditorRevealType};
use je_workbench::common::editor::SaveReason;
use je_workbench::contrib::notebook::common::notebook_common::{CellKind, INotebookContributionData, NotebookExtensionDescription, TransientOptions};
use languages_support::{languages_setup, Observed};

/// O JavaScript da extensão de exemplo.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__activated = true;
  globalThis.__events = [];
  const log = (text) => globalThis.__events.push(text);
  const command = (id, handler) => context.subscriptions.push(vscode.commands.registerCommand(id, handler));

  command('t.listen', () => {
    context.subscriptions.push(vscode.workspace.onDidOpenNotebookDocument((document) => log('opened:' + document.uri.path + ':' + document.cellCount)));
    context.subscriptions.push(vscode.workspace.onDidCloseNotebookDocument((document) => log('closed:' + document.uri.path + ':' + document.isClosed)));
    return 'ok';
  });

  command('t.open', async () => {
    const document = await vscode.workspace.openNotebookDocument(vscode.Uri.parse('file:///proj/a.ipynb'));
    globalThis.__doc = document;
    const second = document.cellAt(1);
    return JSON.stringify({
      uri: document.uri.toString(),
      type: document.notebookType,
      version: document.version,
      count: document.cellCount,
      dirty: document.isDirty,
      untitled: document.isUntitled,
      closed: document.isClosed,
      kinds: document.getCells().map((cell) => cell.kind),
      languages: document.getCells().map((cell) => cell.document.languageId),
      text: second.document.getText(),
      index: second.index,
      owner: second.notebook.uri.toString(),
      listed: vscode.workspace.notebookDocuments.map((item) => item.uri.path),
      ranged: document.getCells(new vscode.NotebookRange(0, 1)).length,
      sameCell: document.cellAt(1) === second && document.getCells()[1] === second,
      sameNotebook: second.notebook === document && vscode.workspace.notebookDocuments[0] === document,
    });
  });

  command('t.save', async () => String(await globalThis.__doc.save()));

  command('t.documentCount', () => String(vscode.workspace.notebookDocuments.length));

  command('t.create', async () => {
    const document = await vscode.workspace.openNotebookDocument('test-notebook', new vscode.NotebookData([new vscode.NotebookCellData(vscode.NotebookCellKind.Code, '1 + 1', 'javascript')]));
    let invalid;
    try { await vscode.workspace.openNotebookDocument(42); } catch (error) { invalid = error.message; }
    return JSON.stringify({ uri: document.uri.toString(), untitled: document.isUntitled, type: document.notebookType, invalid });
  });

  command('t.values', () => {
    const output = new vscode.NotebookCellOutput([vscode.NotebookCellOutputItem.json({ a: 1 }), vscode.NotebookCellOutputItem.stdout('x')], { k: 'v' });
    const range = new vscode.NotebookRange(3, 1);
    return JSON.stringify({
      mimes: output.items.map((item) => item.mime),
      jsonLength: output.items[0].data.length,
      stdout: output.items[1].data,
      metadata: output.metadata.k,
      hasId: output.id.length > 0,
      range: range.start + '-' + range.end + ':' + range.isEmpty,
      kinds: [vscode.NotebookCellKind.Markup, vscode.NotebookCellKind.Code],
      alignment: [vscode.NotebookCellStatusBarAlignment.Left, vscode.NotebookCellStatusBarAlignment.Right],
    });
  });

  command('t.controller', () => {
    const controller = vscode.notebooks.createNotebookController('je-kernel', 'test-notebook', 'Je Kernel', async (cells, notebook, ctrl) => {
      log('execute:' + cells.map((cell) => cell.index).join(',') + ':' + notebook.uri.path + ':' + ctrl.id);
      const execution = ctrl.createNotebookCellExecution(cells[0]);
      log('execution:' + execution.cell.index + ':' + typeof execution.token.isCancellationRequested);
      execution.executionOrder = 1;
      execution.start(1000);
      execution.replaceOutput([new vscode.NotebookCellOutput([vscode.NotebookCellOutputItem.text('hola')])]);
      execution.end(true, 2000);
    });
    globalThis.__controller = controller;
    controller.supportedLanguages = ['javascript'];
    controller.detail = 'detalhe';
    controller.description = 'descrição';
    controller.supportsExecutionOrder = true;
    context.subscriptions.push(controller.onDidChangeSelectedNotebooks((event) => log('selected:' + event.selected + ':' + event.notebook.uri.path)));
    return JSON.stringify({
      id: controller.id,
      type: controller.notebookType,
      label: controller.label,
      languages: controller.supportedLanguages,
      detail: controller.detail,
      handler: typeof controller.executeHandler,
    });
  });

  command('t.affinity', () => {
    globalThis.__controller.updateNotebookAffinity(globalThis.__doc, vscode.NotebookControllerAffinity.Preferred);
    return 'ok';
  });

  command('t.controllerDispose', () => {
    globalThis.__controller.dispose();
    let message;
    try { globalThis.__controller.createNotebookCellExecution(globalThis.__doc.cellAt(1)); } catch (error) { message = error.message; }
    return message;
  });

  command('t.statusBar', () => {
    globalThis.__emitter = new vscode.EventEmitter();
    globalThis.__registration = vscode.notebooks.registerNotebookCellStatusBarItemProvider('test-notebook', {
      onDidChangeCellStatusBarItems: globalThis.__emitter.event,
      provideCellStatusBarItems: (cell, token) => {
        log('provide:' + cell.index + ':' + typeof token.isCancellationRequested);
        const item = new vscode.NotebookCellStatusBarItem('Executar ' + cell.index, vscode.NotebookCellStatusBarAlignment.Right);
        item.tooltip = 'dica';
        item.priority = 5;
        item.command = 'je.run';
        return [item];
      },
    });
    return 'ok';
  });

  command('t.statusBarFire', () => {
    globalThis.__emitter.fire();
    return 'ok';
  });

  command('t.statusBarDispose', () => {
    globalThis.__registration.dispose();
    return 'ok';
  });

  command('t.serializer', () => {
    class Serializer {
      constructor() { this.prefix = 'cell:'; }
      deserializeNotebook(bytes, token) {
        log('deserialize:' + (bytes instanceof Uint8Array) + ':' + bytes.length + ':' + typeof token.isCancellationRequested);
        const text = String.fromCharCode.apply(null, Array.from(bytes));
        return new vscode.NotebookData(text.split('\n').map((line) => new vscode.NotebookCellData(vscode.NotebookCellKind.Code, this.prefix + line, 'javascript')));
      }
      async serializeNotebook(data, token) {
        const text = data.cells.map((cell) => cell.value).join('\n');
        log('serialize:' + data.cells.map((cell) => cell.value).join('|') + ':' + typeof token.isCancellationRequested);
        return Uint8Array.from(text.split('').map((char) => char.charCodeAt(0)));
      }
    }
    globalThis.__serializer = vscode.workspace.registerNotebookSerializer('test-notebook', new Serializer(), { transientOutputs: true });
    let empty;
    try { vscode.workspace.registerNotebookSerializer('  ', new Serializer()); } catch (error) { empty = error.message; }
    let incomplete;
    try { vscode.workspace.registerNotebookSerializer('other', { deserializeNotebook() {} }); } catch (error) { incomplete = error.message; }
    return JSON.stringify({ empty, incomplete });
  });

  command('t.serializerDispose', () => {
    globalThis.__serializer.dispose();
    return 'ok';
  });

  command('t.documentEvents', () => {
    context.subscriptions.push(vscode.workspace.onDidChangeNotebookDocument((event) => {
      log('change:' + event.notebook.uri.path + ':cells=' + event.cellChanges.length + ':content=' + event.contentChanges.length + ':' + event.cellChanges.map((change) => change.cell.index + '=' + change.metadata.k).join(','));
    }));
    context.subscriptions.push(vscode.workspace.onDidSaveNotebookDocument((document) => log('saved:' + document.uri.path)));
    context.subscriptions.push(vscode.workspace.onWillSaveNotebookDocument((event) => {
      log('willSave:' + event.notebook.uri.path + ':' + event.reason + ':' + typeof event.token.isCancellationRequested);
      const edit = new vscode.WorkspaceEdit();
      edit.replace(event.notebook.cellAt(1).document.uri, new vscode.Range(0, 0, 0, 3), 'var');
      event.waitUntil(Promise.resolve(edit));
    }));
    return 'ok';
  });

  command('t.editors', async () => {
    context.subscriptions.push(vscode.window.onDidChangeActiveNotebookEditor((editor) => log('active:' + (editor ? editor.notebook.uri.path : 'none'))));
    context.subscriptions.push(vscode.window.onDidChangeVisibleNotebookEditors((editors) => log('visible:' + editors.length)));
    const before = vscode.window.activeNotebookEditor === undefined && vscode.window.visibleNotebookEditors.length === 0;
    const editor = await vscode.window.showNotebookDocument(globalThis.__doc, { viewColumn: 2, preserveFocus: true, preview: false, selections: [new vscode.NotebookRange(0, 1)] });
    const initial = editor.selection.start + '-' + editor.selection.end;
    editor.selection = new vscode.NotebookRange(1, 2);
    editor.revealRange(new vscode.NotebookRange(1, 2), vscode.NotebookEditorRevealType.InCenter);
    return JSON.stringify({
      before,
      uri: editor.notebook.uri.path,
      initial,
      selection: editor.selection.start + '-' + editor.selection.end,
      selections: editor.selections.length,
      viewColumn: editor.viewColumn,
      active: vscode.window.activeNotebookEditor.notebook.uri.path,
      visible: vscode.window.visibleNotebookEditors.length,
    });
  });

  command('t.editorProperties', () => {
    const describe = (ranges) => ranges.map((range) => range.start + '-' + range.end).join(',');
    context.subscriptions.push(vscode.window.onDidChangeNotebookEditorSelection((event) => log('selection:' + event.notebookEditor.notebook.uri.path + ':' + describe(event.selections))));
    context.subscriptions.push(vscode.window.onDidChangeNotebookEditorVisibleRanges((event) => log('visibleRanges:' + event.notebookEditor.notebook.uri.path + ':' + describe(event.visibleRanges))));
    return 'ok';
  });

  command('t.editorState', () => {
    const editor = vscode.window.activeNotebookEditor;
    const describe = (ranges) => ranges.map((range) => range.start + '-' + range.end).join(',');
    return JSON.stringify({ selections: describe(editor.selections), visibleRanges: describe(editor.visibleRanges), viewColumn: editor.viewColumn });
  });

  command('t.notebookEdit', async () => {
    const insert = vscode.NotebookEdit.insertCells(1, [new vscode.NotebookCellData(vscode.NotebookCellKind.Code, 'x', 'javascript')]);
    const edit = new vscode.WorkspaceEdit();
    edit.set(globalThis.__doc.uri, [
      insert,
      [vscode.NotebookEdit.updateNotebookMetadata({ a: 1 }), { label: 'meta', needsConfirmation: false }],
      vscode.NotebookEdit.deleteCells(new vscode.NotebookRange(0, 1)),
    ]);
    const applied = await vscode.workspace.applyEdit(edit);
    const created = new vscode.NotebookEdit(new vscode.NotebookRange(2, 3), []);
    return JSON.stringify({
      applied,
      insertRange: insert.range.start + '-' + insert.range.end,
      cells: insert.newCells.length,
      isEdit: vscode.NotebookEdit.isNotebookCellEdit(insert),
      plain: vscode.NotebookEdit.isNotebookCellEdit({ range: new vscode.NotebookRange(0, 0), newCells: [] }),
      notEdit: vscode.NotebookEdit.isNotebookCellEdit({ range: new vscode.NotebookRange(0, 0) }),
      cellMetadata: vscode.NotebookEdit.updateCellMetadata(2, { k: 'v' }).newCellMetadata.k,
      notebookMetadata: vscode.NotebookEdit.updateNotebookMetadata({ a: 1 }).newNotebookMetadata.a,
      created: created.range.start + '-' + created.range.end,
    });
  });
};
"#;

/// Os itens de saída como `mime=texto`, separados por `+`.
fn describe_items(items: &[NotebookOutputItemDto]) -> String {
    items.iter().map(|item| format!("{}={}", item.mime, String::from_utf8_lossy(&item.value_bytes.to_vec()))).collect::<Vec<_>>().join("+")
}

/// Um resumo (em texto) de uma atualização de execução que chegou à thread principal.
fn describe_update(update: &ICellExecuteUpdateDto) -> String {
    match update {
        ICellExecuteUpdateDto::Output(edit) => format!(
            "output:cell={}:append={}:{}",
            edit.cell_handle,
            edit.append.unwrap_or(false),
            edit.outputs.iter().map(|output| describe_items(&output.items)).collect::<Vec<_>>().join("|")
        ),
        ICellExecuteUpdateDto::OutputItems(edit) => format!("outputItems:append={}:{}", edit.append.unwrap_or(false), describe_items(&edit.items)),
        ICellExecuteUpdateDto::ExecutionState(state) => format!("state:order={:?}:start={:?}", state.execution_order, state.run_start_time),
    }
}

/// O papel da thread principal: os quatro `MainThreadNotebook*Shape` (e o `MainThreadBulkEdits`) que anotam as chamadas.
struct Panel {
    ext_host: RefCell<Option<Rc<dyn ExtHostNotebookShape>>>,
    ext_kernels: RefCell<Option<Rc<dyn ExtHostNotebookKernelsShape>>>,
    ext_documents: RefCell<Option<Rc<dyn ExtHostNotebookDocumentsShape>>>,
    ext_editors: RefCell<Option<Rc<dyn ExtHostNotebookEditorsShape>>>,
    ext_save: RefCell<Option<Rc<dyn ExtHostNotebookDocumentSaveParticipantShape>>>,
    log: RefCell<Vec<String>>,
    /// O `handle` do controlador registrado.
    kernel_handle: RefCell<Option<u32>>,
    /// O `handle` do serializador registrado.
    serializer_handle: RefCell<Option<u32>>,
    /// O `handle` e o `eventHandle` do provedor da barra de status registrado.
    status_bar: RefCell<Option<(u32, Option<u32>)>>,
}

impl Panel {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            ext_host: RefCell::new(None),
            ext_kernels: RefCell::new(None),
            ext_documents: RefCell::new(None),
            ext_editors: RefCell::new(None),
            ext_save: RefCell::new(None),
            log: RefCell::new(Vec::new()),
            kernel_handle: RefCell::new(None),
            serializer_handle: RefCell::new(None),
            status_bar: RefCell::new(None),
        })
    }

    fn ext_host(&self) -> Rc<dyn ExtHostNotebookShape> {
        self.ext_host.borrow().clone().expect("o host já subiu e ligou o dublê")
    }

    fn ext_documents(&self) -> Rc<dyn ExtHostNotebookDocumentsShape> {
        self.ext_documents.borrow().clone().expect("o host já subiu e ligou o dublê")
    }

    fn ext_editors(&self) -> Rc<dyn ExtHostNotebookEditorsShape> {
        self.ext_editors.borrow().clone().expect("o host já subiu e ligou o dublê")
    }

    fn ext_save(&self) -> Rc<dyn ExtHostNotebookDocumentSaveParticipantShape> {
        self.ext_save.borrow().clone().expect("o host já subiu e ligou o dublê")
    }

    fn serializer_handle(&self) -> u32 {
        self.serializer_handle.borrow().expect("a extensão registrou um serializador")
    }

    fn ext_kernels(&self) -> Rc<dyn ExtHostNotebookKernelsShape> {
        self.ext_kernels.borrow().clone().expect("o host já subiu e ligou o dublê")
    }

    fn note(&self, line: String) {
        self.log.borrow_mut().push(line);
    }

    fn lines(&self) -> Vec<String> {
        self.log.borrow().clone()
    }

    fn kernel_handle(&self) -> u32 {
        self.kernel_handle.borrow().expect("a extensão criou um controlador")
    }

    /// O delta de documentos que a thread principal real mandaria ao abrir um notebook: um cabeçalho markdown e um código com duas linhas.
    fn add_notebook(&self, uri: &URI) {
        let delta = INotebookDocumentsAndEditorsDelta {
            added_documents: Some(vec![INotebookModelAddedData {
                uri: uri.clone(),
                version_id: 0,
                cells: vec![
                    cell_dto(uri, 0, &["# Título"], "markdown", CellKind::Markup),
                    cell_dto(uri, 1, &["let a = 1;", "let b = 2;"], "javascript", CellKind::Code),
                ],
                view_type: "test-notebook".to_owned(),
                metadata: None,
            }]),
            ..INotebookDocumentsAndEditorsDelta::default()
        };
        self.ext_host().rpc_accept_document_and_editors_delta(SerializableObjectWithBuffers::new(delta));
    }

    fn remove_notebook(&self, uri: &URI) {
        let delta = INotebookDocumentsAndEditorsDelta { removed_documents: Some(vec![uri.clone()]), ..INotebookDocumentsAndEditorsDelta::default() };
        self.ext_host().rpc_accept_document_and_editors_delta(SerializableObjectWithBuffers::new(delta));
    }
}

impl IDisposable for Panel {
    fn dispose(&self) {}
}

impl MainThreadNotebookShape for Panel {
    fn rpc_register_notebook_serializer(
        &self,
        handle: u32,
        extension: NotebookExtensionDescription,
        view_type: &str,
        options: TransientOptions,
        registration: Option<INotebookContributionData>,
    ) {
        *self.serializer_handle.borrow_mut() = Some(handle);
        self.note(format!("registerSerializer:{view_type}:{}:outputs={}:registration={}", extension.id.value, options.transient_outputs, registration.is_some()));
    }

    fn rpc_unregister_notebook_serializer(&self, handle: u32) {
        self.note(format!("unregisterSerializer:same={}", *self.serializer_handle.borrow() == Some(handle)));
    }

    fn rpc_register_notebook_cell_status_bar_item_provider(&self, handle: u32, event_handle: Option<u32>, view_type: &str) -> Promise<()> {
        *self.status_bar.borrow_mut() = Some((handle, event_handle));
        self.note(format!("registerStatusBar:{view_type}:event={}", event_handle.is_some()));
        Promise::resolved(())
    }

    fn rpc_unregister_notebook_cell_status_bar_item_provider(&self, handle: u32, event_handle: Option<u32>) -> Promise<()> {
        let registered = *self.status_bar.borrow();
        self.note(format!("unregisterStatusBar:same={}", registered == Some((handle, event_handle))));
        Promise::resolved(())
    }

    fn rpc_emit_cell_status_bar_event(&self, event_handle: u32) {
        let registered = *self.status_bar.borrow();
        self.note(format!("statusBarEvent:same={}", registered.and_then(|(_, event)| event) == Some(event_handle)));
    }
}

impl MainThreadNotebookDocumentsShape for Panel {
    fn rpc_try_create_notebook(&self, options: NotebookCreateOptions) -> Promise<URI> {
        self.note(format!("create:{}:content={}", options.view_type, options.content.is_some()));
        let uri = URI::parse("untitled:Untitled-1", false);
        self.add_notebook(&uri);
        Promise::resolved(uri)
    }

    fn rpc_try_open_notebook(&self, uri: &URI) -> Promise<URI> {
        self.note(format!("open:{}", uri.to_string(false).to_string_lossy()));
        self.add_notebook(uri);
        Promise::resolved(uri.clone())
    }

    fn rpc_try_save_notebook(&self, uri: &URI) -> Promise<bool> {
        self.note(format!("save:{}", uri.to_string(false).to_string_lossy()));
        Promise::resolved(true)
    }
}

impl MainThreadNotebookEditorsShape for Panel {
    fn rpc_try_show_notebook_document(&self, uri: &URI, view_type: &str, options: INotebookDocumentShowOptions) -> Promise<String> {
        self.note(format!(
            "show:{}:{view_type}:preserve={:?}:pinned={:?}:selections={:?}",
            uri.to_string(false).to_string_lossy(),
            options.preserve_focus,
            options.pinned,
            options.selections.as_ref().map(Vec::len)
        ));
        let id = "editor-1".to_owned();
        let delta = INotebookDocumentsAndEditorsDelta {
            added_editors: Some(vec![INotebookEditorAddData {
                id: id.clone(),
                document_uri: uri.clone(),
                selections: vec![CellRange { start: 0.0, end: 1.0 }],
                visible_ranges: Vec::new(),
                view_column: Some(1.0), // o main thread manda o EditorGroupColumn de base 0; ViewColumn.to soma 1
                view_type: view_type.to_owned(),
            }]),
            ..INotebookDocumentsAndEditorsDelta::default()
        };
        self.ext_host().rpc_accept_document_and_editors_delta(SerializableObjectWithBuffers::new(delta));
        let delta = INotebookDocumentsAndEditorsDelta {
            new_active_editor: Some(Some(id.clone())),
            visible_editors: Some(vec![id.clone()]),
            ..INotebookDocumentsAndEditorsDelta::default()
        };
        self.ext_host().rpc_accept_document_and_editors_delta(SerializableObjectWithBuffers::new(delta));
        Promise::resolved(id)
    }

    fn rpc_try_reveal_range(&self, id: &str, range: CellRange, reveal_type: NotebookEditorRevealType) -> Promise<()> {
        self.note(format!("reveal:{id}:{}-{}:{reveal_type:?}", range.start, range.end));
        Promise::resolved(())
    }

    fn rpc_try_set_selections(&self, id: &str, range: Vec<CellRange>) -> Promise<()> {
        self.note(format!("selections:{id}:{}", range.iter().map(|range| format!("{}-{}", range.start, range.end)).collect::<Vec<_>>().join(",")));
        Promise::resolved(())
    }
}

impl MainThreadBulkEditsShape for Panel {
    fn rpc_try_apply_workspace_edit(&self, workspace_edit_dto: SerializableObjectWithBuffers<IWorkspaceEditDto>, _undo_redo_group_id: Option<f64>, _is_refactoring: Option<bool>) -> Promise<bool> {
        self.note(format!("bulkEdits:{}", workspace_edit_dto.value.edits.len()));
        Promise::resolved(true)
    }
}

impl MainThreadNotebookKernelsShape for Panel {
    fn rpc_post_message(&self, _handle: u32, _editor_id: Option<&str>, _message: Dynamic) -> Promise<bool> {
        panic!("o teste não manda mensagens ao renderizador")
    }

    fn rpc_add_kernel(&self, handle: u32, data: INotebookKernelDto2) -> Promise<()> {
        *self.kernel_handle.borrow_mut() = Some(handle);
        self.note(format!("kernelAdd:{}:{}:{}", data.id, data.notebook_type, data.label));
        Promise::resolved(())
    }

    fn rpc_update_kernel(&self, _handle: u32, data: INotebookKernelDto2) {
        self.note(format!(
            "kernelUpdate:label={}:languages={}:detail={}:description={}:order={:?}",
            data.label,
            data.supported_languages.clone().unwrap_or_default().join(","),
            data.detail.clone().unwrap_or_default(),
            data.description.clone().unwrap_or_default(),
            data.supports_execution_order
        ));
    }

    fn rpc_remove_kernel(&self, _handle: u32) {
        self.note("kernelRemove".to_owned());
    }

    fn rpc_update_notebook_priority(&self, _handle: u32, uri: &URI, value: Option<NotebookControllerAffinity2>) {
        self.note(format!("affinity:{}:{:?}", uri.path(), value.map(|affinity| affinity.value())));
    }

    fn rpc_create_execution(&self, _handle: u32, controller_id: &str, _uri: &URI, cell_handle: i64) {
        self.note(format!("createExecution:{controller_id}:cell={cell_handle}"));
    }

    fn rpc_update_execution(&self, _handle: u32, data: SerializableObjectWithBuffers<Vec<ICellExecuteUpdateDto>>) {
        for update in &data.value {
            self.note(format!("update:{}", describe_update(update)));
        }
    }

    fn rpc_complete_execution(&self, _handle: u32, data: SerializableObjectWithBuffers<ICellExecutionCompleteDto>) {
        self.note(format!("complete:success={:?}:end={:?}", data.value.last_run_success, data.value.run_end_time));
    }

    fn rpc_create_notebook_execution(&self, _handle: u32, _controller_id: &str, _uri: &URI) {
        panic!("o teste não cria execuções de notebook")
    }

    fn rpc_begin_notebook_execution(&self, _handle: u32) {
        panic!("o teste não cria execuções de notebook")
    }

    fn rpc_complete_notebook_execution(&self, _handle: u32) {
        panic!("o teste não cria execuções de notebook")
    }

    fn rpc_add_kernel_detection_task(&self, _handle: u32, _notebook_type: &str) -> Promise<()> {
        panic!("o teste não cria tarefas de detecção")
    }

    fn rpc_remove_kernel_detection_task(&self, _handle: u32) {
        panic!("o teste não cria tarefas de detecção")
    }

    fn rpc_add_kernel_source_action_provider(&self, _handle: u32, _event_handle: u32, _notebook_type: &str) -> Promise<()> {
        panic!("o teste não registra provedores de ação de origem")
    }

    fn rpc_remove_kernel_source_action_provider(&self, _handle: u32, _event_handle: u32) {
        panic!("o teste não registra provedores de ação de origem")
    }

    fn rpc_emit_notebook_kernel_source_actions_change_event(&self, _event_handle: u32) {
        panic!("o teste não registra provedores de ação de origem")
    }

    fn rpc_receive_variable(&self, _request_id: &str, _variable: VariablesResult) {
        panic!("o teste não usa o provedor de variáveis")
    }

    fn rpc_variables_updated(&self, _notebook_uri: &URI) {
        panic!("o teste não usa o provedor de variáveis")
    }
}

/// Liga o dublê da thread principal ao host, junto dos clientes de documentos e linguagens do suporte de `languages` (cada célula é um
/// documento de texto).
struct Binder {
    documents: Rc<dyn IMainThreadBinder>,
    panel: Rc<Panel>,
}

impl IMainThreadBinder for Binder {
    fn bind(&self, context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>> {
        let disposables = self.documents.bind(context.clone());
        *self.panel.ext_host.borrow_mut() = Some(context.get_proxy(&ExtHostContext::EXT_HOST_NOTEBOOK));
        *self.panel.ext_kernels.borrow_mut() = Some(context.get_proxy(&ExtHostContext::EXT_HOST_NOTEBOOK_KERNELS));
        *self.panel.ext_documents.borrow_mut() = Some(context.get_proxy(&ExtHostContext::EXT_HOST_NOTEBOOK_DOCUMENTS));
        *self.panel.ext_editors.borrow_mut() = Some(context.get_proxy(&ExtHostContext::EXT_HOST_NOTEBOOK_EDITORS));
        *self.panel.ext_save.borrow_mut() = Some(context.get_proxy(&ExtHostContext::EXT_HOST_NOTEBOOK_DOCUMENT_SAVE_PARTICIPANT));
        let bulk_edits: Rc<dyn MainThreadBulkEditsShape> = self.panel.clone();
        context.set(&MainContext::MAIN_THREAD_BULK_EDITS, bulk_edits);
        let notebook: Rc<dyn MainThreadNotebookShape> = self.panel.clone();
        context.set(&MainContext::MAIN_THREAD_NOTEBOOK, notebook);
        let documents: Rc<dyn MainThreadNotebookDocumentsShape> = self.panel.clone();
        context.set(&MainContext::MAIN_THREAD_NOTEBOOK_DOCUMENTS, documents);
        let editors: Rc<dyn MainThreadNotebookEditorsShape> = self.panel.clone();
        context.set(&MainContext::MAIN_THREAD_NOTEBOOK_EDITORS, editors);
        let kernels: Rc<dyn MainThreadNotebookKernelsShape> = self.panel.clone();
        context.set(&MainContext::MAIN_THREAD_NOTEBOOK_KERNELS, kernels);
        disposables
    }
}

fn setup(_workbench: &Workbench) -> (Rc<dyn IMainThreadBinder>, (Observed, Rc<Panel>)) {
    let (documents, observed) = languages_setup();
    let panel = Panel::new();
    (Rc::new(Binder { documents, panel: panel.clone() }), (observed, panel))
}

fn run(workbench: &Workbench, command: &str) -> Dynamic {
    workbench.execute(command, Vec::new())
}

fn events(workbench: &Workbench) -> String {
    workbench.probe("JSON.stringify(globalThis.__events)")
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn notebook_uri() -> URI {
    URI::parse("file:///proj/a.ipynb", false)
}

#[test]
fn open_notebook_document_exposes_the_notebook_its_cells_and_the_open_and_close_events() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, (_observed, panel)| {
        run(workbench, "t.listen");
        assert_eq!(
            run(workbench, "t.open"),
            Dynamic::string(concat!(
                r#"{"uri":"file:///proj/a.ipynb","type":"test-notebook","version":0,"count":2,"dirty":false,"untitled":false,"closed":false,"#,
                r#""kinds":[1,2],"languages":["markdown","javascript"],"text":"let a = 1;\nlet b = 2;","index":1,"owner":"file:///proj/a.ipynb","#,
                r#""listed":["/proj/a.ipynb"],"ranged":1,"sameCell":true,"sameNotebook":true}"#
            ))
        );
        assert_eq!(panel.lines(), strings(&["open:file:///proj/a.ipynb"]));
        assert_eq!(events(workbench), r#"["opened:/proj/a.ipynb:2"]"#, "o evento de abertura chega com o notebook já montado");

        assert_eq!(run(workbench, "t.save"), Dynamic::string("true"));
        assert_eq!(panel.lines()[1], "save:file:///proj/a.ipynb");

        panel.remove_notebook(&notebook_uri());
        workbench.pump();
        assert_eq!(events(workbench), r#"["opened:/proj/a.ipynb:2","closed:/proj/a.ipynb:true"]"#, "o notebook já está fechado quando o evento chega");
        assert_eq!(run(workbench, "t.documentCount"), Dynamic::string("0"));
    });
}

#[test]
fn create_notebook_document_forwards_the_content_and_rejects_invalid_arguments() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, (_observed, panel)| {
        assert_eq!(
            run(workbench, "t.create"),
            Dynamic::string(r#"{"uri":"untitled:Untitled-1","untitled":true,"type":"test-notebook","invalid":"Invalid arguments"}"#)
        );
        assert_eq!(panel.lines(), strings(&["create:test-notebook:content=true"]));
    });
}

#[test]
fn value_classes_and_enums_behave_like_the_originals() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, (_observed, _panel)| {
        assert_eq!(
            run(workbench, "t.values"),
            Dynamic::string(concat!(
                r#"{"mimes":["text/x-json","application/vnd.code.notebook.stdout"],"jsonLength":11,"stdout":[120],"metadata":"v","hasId":true,"#,
                r#""range":"1-3:false","kinds":[1,2],"alignment":[1,2]}"#
            ))
        );
    });
}

#[test]
fn notebook_controller_registers_updates_executes_cells_and_disposes() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, (_observed, panel)| {
        run(workbench, "t.open");
        assert_eq!(
            run(workbench, "t.controller"),
            Dynamic::string(r#"{"id":"je-kernel","type":"test-notebook","label":"Je Kernel","languages":["javascript"],"detail":"detalhe","handler":"function"}"#)
        );
        workbench.pump();
        assert_eq!(
            panel.lines()[1..].to_vec(),
            strings(&[
                "kernelAdd:je.exthost/je-kernel:test-notebook:Je Kernel",
                "kernelUpdate:label=Je Kernel:languages=javascript:detail=detalhe:description=descrição:order=Some(true)",
            ]),
            "as atribuições da mesma rodada viram um só $updateKernel"
        );

        let handle = panel.kernel_handle();
        let kernels = panel.ext_kernels();
        kernels.rpc_accept_notebook_association(handle, &notebook_uri(), true);
        let executed = kernels.rpc_execute_cells(handle, &notebook_uri(), vec![1]);
        std::thread::sleep(std::time::Duration::from_millis(50));
        workbench.pump();
        settled_ok(&executed, "executeCells");

        assert_eq!(
            events(workbench),
            r#"["selected:true:/proj/a.ipynb","execute:1:/proj/a.ipynb:je-kernel","execution:1:boolean"]"#,
            "o tratador recebe as células, o notebook e o controlador"
        );
        assert_eq!(
            panel.lines()[3..].to_vec(),
            strings(&[
                "createExecution:je.exthost/je-kernel:cell=1",
                "update:state:order=Some(1.0):start=None",
                "update:state:order=None:start=Some(1000.0)",
                "update:output:cell=1:append=false:text/plain=hola",
                "complete:success=Some(true):end=Some(2000.0)",
            ])
        );

        run(workbench, "t.affinity");
        assert_eq!(panel.lines().last().map(String::as_str), Some("affinity:/proj/a.ipynb:Some(2)"));

        assert_eq!(run(workbench, "t.controllerDispose"), Dynamic::string("notebook controller is DISPOSED"));
        assert_eq!(panel.lines().last().map(String::as_str), Some("kernelRemove"));
    });
}

#[test]
fn cell_status_bar_item_provider_answers_the_main_thread_and_forwards_its_change_event() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, (_observed, panel)| {
        run(workbench, "t.open");
        run(workbench, "t.statusBar");
        assert_eq!(panel.lines()[1], "registerStatusBar:test-notebook:event=true");
        let (handle, _) = (*panel.status_bar.borrow()).expect("o provedor foi registrado");

        let provided = panel.ext_host().rpc_provide_notebook_cell_status_bar_items(handle, &notebook_uri(), 1, CancellationToken::None);
        workbench.pump();
        let list = settled_ok(&provided, "provideNotebookCellStatusBarItems").expect("o provedor devolveu itens");
        assert_eq!(list.items.len(), 1);
        let item = &list.items[0];
        assert_eq!((item.text.as_str(), item.tooltip.as_deref(), item.priority), ("Executar 1", Some("dica"), Some(5.0)));
        assert_eq!(format!("{:?}", item.alignment), "Right");
        assert_eq!(item.command.as_ref().map(|command| command.id.as_str()), Some("je.run"));
        assert_eq!(events(workbench), r#"["provide:1:boolean"]"#, "a célula e o token chegam ao provedor");

        run(workbench, "t.statusBarFire");
        assert_eq!(panel.lines().last().map(String::as_str), Some("statusBarEvent:same=true"));

        run(workbench, "t.statusBarDispose");
        assert_eq!(panel.lines().last().map(String::as_str), Some("unregisterStatusBar:same=true"));
    });
}

fn sorted_events(workbench: &Workbench) -> String {
    workbench.probe("JSON.stringify(globalThis.__events.slice().sort())")
}

#[test]
fn notebook_serializer_registers_by_reference_and_converts_between_bytes_and_notebook_data() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, (_observed, panel)| {
        assert_eq!(
            run(workbench, "t.serializer"),
            Dynamic::string(
                r#"{"empty":"viewType cannot be empty or just whitespace","incomplete":"registerNotebookSerializer: o serializador precisa de serializeNotebook"}"#
            )
        );
        assert_eq!(panel.lines(), strings(&["registerSerializer:test-notebook:je.exthost:outputs=true:registration=false"]));

        let handle = panel.serializer_handle();
        let deserialized = panel.ext_host().rpc_data_to_notebook(handle, VSBuffer::from_string("a\nb"), CancellationToken::None);
        workbench.pump();
        let dto = settled_ok(&deserialized, "dataToNotebook").value;
        let cells: Vec<(String, String)> = dto.cells.iter().map(|cell| (cell.source.clone(), cell.language.clone())).collect();
        assert_eq!(cells, vec![("cell:a".to_owned(), "javascript".to_owned()), ("cell:b".to_owned(), "javascript".to_owned())], "a extensão recebe um Uint8Array e devolve um NotebookData");

        let cell = |source: &str| NotebookCellDataDto {
            source: source.to_owned(),
            language: "javascript".to_owned(),
            mime: None,
            cell_kind: CellKind::Code,
            outputs: Vec::new(),
            metadata: None,
            internal_metadata: None,
        };
        let data = NotebookDataDto { cells: vec![cell("x"), cell("y")], metadata: Dynamic::object(Vec::<(&str, Dynamic)>::new()) };
        let serialized = panel.ext_host().rpc_notebook_to_data(handle, SerializableObjectWithBuffers::new(data), CancellationToken::None);
        workbench.pump();
        assert_eq!(settled_ok(&serialized, "notebookToData").to_vec(), b"x\ny".to_vec(), "o Uint8Array da extensão volta como os bytes");
        assert_eq!(events(workbench), r#"["deserialize:true:3:boolean","serialize:x|y:boolean"]"#);

        run(workbench, "t.serializerDispose");
        assert_eq!(panel.lines().last().map(String::as_str), Some("unregisterSerializer:same=true"));
    });
}

#[test]
fn notebook_document_change_save_and_will_save_events_reach_the_extension() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, (_observed, panel)| {
        run(workbench, "t.open");
        run(workbench, "t.documentEvents");

        let metadata = Dynamic::object(vec![("k", Dynamic::string("v"))]);
        let event = NotebookCellsChangedEventDto { raw_events: vec![NotebookRawContentEventDto::ChangeCellMetadata { index: 1, metadata }], version_id: 1 };
        panel.ext_documents().rpc_accept_model_changed(&notebook_uri(), SerializableObjectWithBuffers::new(event), true, None);
        panel.ext_documents().rpc_accept_model_saved(&notebook_uri());
        workbench.pump();
        assert_eq!(events(workbench), r#"["change:/proj/a.ipynb:cells=1:content=0:1=v","saved:/proj/a.ipynb"]"#);

        let participated = panel.ext_save().rpc_participate_in_save(&notebook_uri(), SaveReason::Explicit, CancellationToken::None);
        workbench.pump();
        assert!(settled_ok(&participated, "participateInSave"), "as edições dos ouvintes foram aplicadas");
        assert_eq!(panel.lines().last().map(String::as_str), Some("bulkEdits:1"), "a edição do waitUntil chega ao MainThreadBulkEdits");
        assert_eq!(
            events(workbench),
            r#"["change:/proj/a.ipynb:cells=1:content=0:1=v","saved:/proj/a.ipynb","willSave:/proj/a.ipynb:1:boolean"]"#,
            "o ouvinte recebe o notebook, o motivo e o token"
        );
    });
}

#[test]
fn notebook_editors_follow_the_main_thread_and_show_notebook_document_opens_one() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, (_observed, panel)| {
        run(workbench, "t.open");
        assert_eq!(
            run(workbench, "t.editors"),
            Dynamic::string(
                r#"{"before":true,"uri":"/proj/a.ipynb","initial":"0-1","selection":"1-2","selections":1,"viewColumn":2,"active":"/proj/a.ipynb","visible":1}"#
            )
        );
        workbench.pump();
        assert_eq!(
            panel.lines()[1..].to_vec(),
            strings(&[
                "show:file:///proj/a.ipynb:test-notebook:preserve=Some(true):pinned=Some(true):selections=Some(1)",
                "selections:editor-1:1-2",
                "reveal:editor-1:1-2:InCenter",
            ])
        );
        assert_eq!(sorted_events(workbench), r#"["active:/proj/a.ipynb","visible:1"]"#, "os eventos de editor ativo e visível disparam com o editor novo");
    });
}

#[test]
fn notebook_editor_selection_visible_ranges_and_view_column_follow_the_main_thread() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, (_observed, panel)| {
        run(workbench, "t.open");
        run(workbench, "t.editors");
        run(workbench, "t.editorProperties");
        workbench.pump();

        let data = INotebookEditorPropertiesChangeData {
            visible_ranges: Some(INotebookVisibleRangesEvent { ranges: vec![CellRange { start: 0.0, end: 2.0 }] }),
            selections: Some(INotebookSelectionChangeEvent { selections: vec![CellRange { start: 1.0, end: 2.0 }, CellRange { start: 3.0, end: 4.0 }] }),
        };
        panel.ext_editors().rpc_accept_editor_properties_changed("editor-1", data);
        let mut columns = INotebookEditorViewColumnInfo::new();
        columns.insert("editor-1".to_owned(), 2.0);
        panel.ext_editors().rpc_accept_editor_view_columns(columns);
        workbench.pump();

        let logged = events(workbench);
        assert!(logged.contains(r#""visibleRanges:/proj/a.ipynb:0-2","selection:/proj/a.ipynb:1-2,3-4""#), "visibleRanges dispara antes de selection, com o editor e os intervalos novos: {logged}");
        assert_eq!(
            run(workbench, "t.editorState"),
            Dynamic::string(r#"{"selections":"1-2,3-4","visibleRanges":"0-2","viewColumn":3}"#),
            "a coluna de base 0 do main thread vira a de base 1 da API uma única vez"
        );
    });
}

#[test]
fn notebook_edit_in_workspace_edit_set_reaches_the_bulk_edits_client() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, (_observed, panel)| {
        run(workbench, "t.open");
        assert_eq!(
            run(workbench, "t.notebookEdit"),
            Dynamic::string(concat!(
                r#"{"applied":true,"insertRange":"1-1","cells":1,"isEdit":true,"plain":true,"notEdit":false,"#,
                r#""cellMetadata":"v","notebookMetadata":1,"created":"2-3"}"#
            ))
        );
        workbench.pump();
        assert_eq!(panel.lines().last().map(String::as_str), Some("bulkEdits:3"), "inserir, metadados do notebook e apagar viram três entradas do DTO");
    });
}
