//! Teste de ponta a ponta de `vscode.notebooks` e dos membros de notebook de `vscode.workspace` (não é porte do oráculo): uma extensão de
//! exemplo em JavaScript, ativada pelo host local (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), abre e cria
//! notebooks, lê o notebook e as células, cria um controlador de notebook que executa células (saída, ordem de execução, término), muda a
//! afinidade e descarta o controlador, e registra um provedor de itens da barra de status das células; o efeito aparece nos clientes
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
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_exthost::in_process::IMainThreadBinder;
use je_workbench::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostNotebookKernelsShape, ExtHostNotebookShape, ICellExecuteUpdateDto, ICellExecutionCompleteDto, IMainContext, INotebookDocumentShowOptions,
    INotebookDocumentsAndEditorsDelta, INotebookKernelDto2, INotebookModelAddedData, MainContext, MainThreadNotebookDocumentsShape, MainThreadNotebookEditorsShape,
    MainThreadNotebookKernelsShape, MainThreadNotebookShape, NotebookCreateOptions, NotebookOutputItemDto, SerializableObjectWithBuffers, VariablesResult,
};
use je_workbench::api::common::ext_host_type_converters::CellRange;
use je_workbench::api::common::ext_host_types::{NotebookControllerAffinity2, NotebookEditorRevealType};
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

/// O papel da thread principal: os quatro `MainThreadNotebook*Shape` que anotam as chamadas.
struct Panel {
    ext_host: RefCell<Option<Rc<dyn ExtHostNotebookShape>>>,
    ext_kernels: RefCell<Option<Rc<dyn ExtHostNotebookKernelsShape>>>,
    log: RefCell<Vec<String>>,
    /// O `handle` do controlador registrado.
    kernel_handle: RefCell<Option<u32>>,
    /// O `handle` e o `eventHandle` do provedor da barra de status registrado.
    status_bar: RefCell<Option<(u32, Option<u32>)>>,
}

impl Panel {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            ext_host: RefCell::new(None),
            ext_kernels: RefCell::new(None),
            log: RefCell::new(Vec::new()),
            kernel_handle: RefCell::new(None),
            status_bar: RefCell::new(None),
        })
    }

    fn ext_host(&self) -> Rc<dyn ExtHostNotebookShape> {
        self.ext_host.borrow().clone().expect("o host já subiu e ligou o dublê")
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
        _handle: u32,
        _extension: NotebookExtensionDescription,
        _view_type: &str,
        _options: TransientOptions,
        _registration: Option<INotebookContributionData>,
    ) {
        panic!("o teste não registra serializadores")
    }

    fn rpc_unregister_notebook_serializer(&self, _handle: u32) {
        panic!("o teste não registra serializadores")
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
    fn rpc_try_show_notebook_document(&self, _uri: &URI, _view_type: &str, _options: INotebookDocumentShowOptions) -> Promise<String> {
        panic!("o teste não mostra notebooks")
    }

    fn rpc_try_reveal_range(&self, _id: &str, _range: CellRange, _reveal_type: NotebookEditorRevealType) -> Promise<()> {
        panic!("o teste não revela intervalos")
    }

    fn rpc_try_set_selections(&self, _id: &str, _range: Vec<CellRange>) -> Promise<()> {
        panic!("o teste não muda seleções")
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
                r#""listed":["/proj/a.ipynb"],"ranged":1}"#
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
