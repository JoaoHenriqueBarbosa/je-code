//! Porte de `src/vs/workbench/api/common/extHostNotebookKernels.ts`: os controladores de notebook (`vscode.NotebookController`),
//! as execuções de célula e de notebook, as tarefas de detecção de kernel, os provedores de ações de origem de kernel e o
//! provedor de variáveis, do lado das extensões.
//!
//! Divergências:
//! - O construtor perde o parâmetro `_initData` (`IExtHostInitDataService`): só `asWebviewUri` o usava, e `asWebviewUri`
//!   (`NotebookController#asWebviewUri`, de `notebookMessaging`) NÃO está portado, porque `asWebviewUri` de
//!   `contrib/webview/common/webview.ts` e o `remote` de `IExtHostInitDataService` não existem no porte. O construtor devolve
//!   [`Result`] (o `registerApiCommand` do original lança se o comando já existe).
//! - `vscode.NotebookController`, `vscode.NotebookCellExecution` e `vscode.NotebookExecution` são os envoltórios
//!   [`NotebookController`], [`NotebookCellExecution`] e [`NotebookExecution`] (um `Rc` do estado); os getters e setters são
//!   métodos. As propriedades somente leitura (`id`, `notebookType`) não têm setter: o `assert.throws` de atribuí-las não tem
//!   correspondente. O `Object.freeze` não existe.
//! - O que o original lança ao ser chamado de forma síncrona (`createNotebookCellExecution`, `start`, `end`, `clearOutput`,
//!   `postMessage`, o setter de `variableProvider`...) devolve [`Result`]. Um erro dentro de função `async` do original
//!   (`updateOutputs`) é a rejeição da promessa devolvida.
//! - `executeHandler`, `interruptHandler` e `variableProvider` são [`NotebookExecuteHandler`], [`NotebookInterruptHandler`] e
//!   [`NotebookVariableProvider`]; o `this` do `.call(controller, ...)` não existe (o fecho captura o que precisa) e o
//!   `AsyncIterable<VariableResult>` de `provideVariables` é [`NotebookVariableResults`] (o `next` do iterador). Um
//!   `executeHandler` que lança (pânico com [`Error`]) é capturado como o `try/catch` de `$executeCells`.
//! - `NotebookControllerAffinity | NotebookControllerAffinity2` de `updateNotebookAffinity` é o `NotebookControllerAffinity2`
//!   (os dois têm os mesmos valores nos membros comuns). `Variable` e `VariableResult` são os tipos da proposta
//!   `notebookVariableProvider`; o campo `type` é `r#type`.
//! - `types.NotebookRendererScript` é o [`NotebookRendererScriptDto`] (`{ uri, provides }`): a conversão `from`/`to` do original
//!   é a cópia. `types.NotebookKernelSourceAction` e `CellExecutionError`/`CellErrorStackFrame` (que `ext_host_types` não
//!   tem) são os structs deste arquivo; o `command` do primeiro é `string | Command` (o [`NotebookCellStatusBarCommand`]).
//! - O `CellErrorStackFrame.position` que `createSerializeableError` manda NÃO tem lugar no [`ICellErrorStackFrame`] do porte
//!   (só `location`, que o original deixa de preencher): a posição não é enviada.
//! - `NotebookCellExecutionTaskState` e `NotebookExecutionTaskState` (idênticos) são o [`ExecutionTaskState`]. O `HANDLE`
//!   estático das duas tarefas é [`AtomicU32`].
//! - `NotebookCellExecutionTask` e `NotebookExecutionTask` estendem `Disposable`: guardam o [`Disposable`] com o `Emitter` de
//!   estado e a `CancellationTokenSource` registrados, e implementam [`IDisposable`].
//! - `Promise.resolve().then(...)` do `_update` é `queue_microtask`; `timeout(10).then(flush)` do coletor é o `timeout` de
//!   `async_`. `console.log`/`console.warn`/`console.error` são `eprintln!`.
//! - `NotebookExecutionTask.cancel` (nunca chamado no original) fica portado e sem uso.
//! - `vscode.executeNotebookVariableProvider` e `notebook.selectKernel` trabalham sobre [`Dynamic`]: o `notebookEditor` do
//!   argumento de `notebook.selectKernel` é um objeto com o editor ligado (`ObjectRef::with_host`), como o processador de
//!   argumentos de `ext_host_notebook.rs` faz com a célula e o notebook.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU32, Ordering};

use je_base::JsString;
use je_base::async_::{async_fn, await_maybe, timeout};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::map::ResourceMap;
use je_base::promise::{DeferredPromise, MaybePromise, Promise};
use je_base::runtime;
use je_base::uri::{URI, UriComponents};
use je_editor::code_action::languages::Command;
use je_editor::range::Range;
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::LogServiceRef;

use super::ext_host_commands::{ApiCommand, ApiCommandArgument, ApiCommandResult, CommandsConverter, ExtHostCommands};
use super::ext_host_notebook::ExtHostNotebookController;
use super::ext_host_notebook_document::{ExtHostCell, ExtHostNotebookDocument, NotebookCell, NotebookDocument};
use super::ext_host_notebook_editor::{ExtHostNotebookEditor, NotebookEditor};
use super::ext_host_protocol::{
    ExtHostNotebookKernelsShape, ICellExecuteOutputEditDto, ICellExecuteOutputItemEditDto, ICellExecuteUpdateDto, ICellExecutionCompleteDto, IMainContext,
    INotebookKernelDto2, MainContext, MainThreadNotebookKernelsShape, NotebookOutputDto, NotebookRendererScriptDto, SerializableObjectWithBuffers,
    VariablesRequestKindDto, VariablesResult,
};
use super::ext_host_type_converters::{self as type_converters, ICommandsConverter};
use super::ext_host_types::{self as types, NotebookCellStatusBarCommand};
use crate::contrib::notebook::common::notebook_common::{INotebookKernelSourceAction, StringOrCommand, UriComponentsOrString};
use crate::contrib::notebook::common::notebook_execution_state_service::{
    CellExecutionErrorStack, ICellErrorStackFrame, ICellExecutionError, ICellExecutionStateUpdate,
};
use crate::services::extensions::extensions::check_proposed_api_enabled;

/// `variablePageSize` (`contrib/notebook/common/notebookKernelService.ts`).
const VARIABLE_PAGE_SIZE: usize = 100;

/// `types.NotebookRendererScript`.
pub type NotebookRendererScript = NotebookRendererScriptDto;

/// O argumento de `onDidChangeSelectedNotebooks`: `{ selected: boolean; notebook: vscode.NotebookDocument }`.
#[derive(Clone)]
pub struct NotebookSelectionChange {
    pub selected: bool,
    pub notebook: NotebookDocument,
}

/// O argumento de `onDidReceiveMessage`: `{ editor: vscode.NotebookEditor; message: unknown }`.
#[derive(Clone)]
pub struct NotebookKernelMessage {
    pub editor: NotebookEditor,
    pub message: Dynamic,
}

/// `vscode.Variable`.
#[derive(Clone, Debug)]
pub struct Variable {
    pub name: String,
    pub value: String,
    pub r#type: Option<String>,
    pub interfaces: Option<Vec<String>>,
    pub language: Option<String>,
    pub expression: Option<String>,
}

/// `vscode.VariableResult`.
#[derive(Clone, Debug)]
pub struct VariableResult {
    pub variable: Variable,
    pub has_named_children: bool,
    pub indexed_children_count: usize,
}

/// O iterador assíncrono (`AsyncIterable<VariableResult>`) que `provideVariables` devolve: `None` é o fim.
pub trait NotebookVariableResults {
    fn next(&self) -> MaybePromise<Option<VariableResult>>;
}

/// O argumento de `onDidChangeVariables`: `{ uri: vscode.Uri }`.
#[derive(Clone, Debug)]
pub struct NotebookVariablesChange {
    pub uri: URI,
}

/// `vscode.NotebookVariableProvider`.
pub trait NotebookVariableProvider {
    fn on_did_change_variables(&self) -> Event<NotebookVariablesChange>;

    fn provide_variables(
        &self,
        notebook: &NotebookDocument,
        parent: Option<&Variable>,
        kind: types::NotebookVariablesRequestKind,
        start: usize,
        token: CancellationToken,
    ) -> Rc<dyn NotebookVariableResults>;
}

/// `vscode.NotebookKernelSourceAction`.
#[derive(Clone, Debug)]
pub struct NotebookKernelSourceAction {
    pub label: String,
    pub description: Option<String>,
    pub detail: Option<String>,
    pub command: Option<NotebookCellStatusBarCommand>,
    pub documentation: Option<UriComponentsOrString>,
}

impl NotebookKernelSourceAction {
    pub fn new(label: impl Into<String>) -> NotebookKernelSourceAction {
        NotebookKernelSourceAction { label: label.into(), description: None, detail: None, command: None, documentation: None }
    }
}

/// `vscode.NotebookKernelSourceActionProvider`.
pub trait NotebookKernelSourceActionProvider {
    /// `onDidChangeNotebookKernelSourceActions` (opcional no original).
    fn on_did_change_notebook_kernel_source_actions(&self) -> Option<Event<()>> {
        None
    }

    /// `provideNotebookKernelSourceActions`: `ProviderResult<NotebookKernelSourceAction[]>`.
    fn provide_notebook_kernel_source_actions(&self, token: CancellationToken) -> MaybePromise<Option<Vec<NotebookKernelSourceAction>>>;
}

/// `vscode.CellErrorStackFrame`.
#[derive(Clone)]
pub struct CellErrorStackFrame {
    pub uri: Option<URI>,
    pub position: Option<types::Position>,
    pub label: String,
}

/// `string | CellErrorStackFrame[] | undefined`, o valor definido de `CellExecutionError.stack`.
#[derive(Clone)]
pub enum CellExecutionStack {
    Text(String),
    Frames(Vec<CellErrorStackFrame>),
}

/// `vscode.CellExecutionError`.
#[derive(Clone)]
pub struct CellExecutionError {
    pub name: String,
    pub message: String,
    pub stack: Option<CellExecutionStack>,
    pub uri: URI,
    pub location: Option<types::Range>,
}

/// O `executeHandler` de `vscode.NotebookController`.
pub type NotebookExecuteHandler = Rc<dyn Fn(Vec<NotebookCell>, NotebookDocument, NotebookController) -> MaybePromise<()>>;

/// O `interruptHandler` de `vscode.NotebookController`.
pub type NotebookInterruptHandler = Rc<dyn Fn(NotebookDocument) -> MaybePromise<()>>;

/// `createKernelId`.
pub fn create_kernel_id(extension_identifier: &ExtensionIdentifier, id: &str) -> String {
    format!("{}/{id}", extension_identifier.value)
}

/// `_defaultExecutHandler`.
fn default_execute_handler(kernel_id: String, extension_identifier: String) -> NotebookExecuteHandler {
    Rc::new(move |_cells: Vec<NotebookCell>, _notebook: NotebookDocument, _controller: NotebookController| {
        eprintln!("NO execute handler from notebook controller '{kernel_id}' of extension: '{extension_identifier}'");
        MaybePromise::Value(())
    })
}

/// O `NotebookEditor` que o objeto `Dynamic` carrega (ver as divergências do módulo).
fn editor_of(value: &Dynamic) -> Option<NotebookEditor> {
    match value {
        Dynamic::Object(object) => object.host::<ExtHostNotebookEditor>().map(|editor| editor.api_editor()),
        _ => None,
    }
}

// ---

/// `NotebookCellExecutionTaskState` e `NotebookExecutionTaskState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExecutionTaskState {
    Init,
    Started,
    Resolved,
}

/// `TimeoutBasedCollector<T>`.
struct TimeoutBasedCollector<T: Clone + 'static> {
    this: Weak<TimeoutBasedCollector<T>>,
    delay: u64,
    callback: Rc<dyn Fn(Vec<T>) -> Promise<()>>,
    batch: RefCell<Vec<T>>,
    started_timer: Cell<u64>,
    current_deferred: RefCell<Option<Rc<DeferredPromise<()>>>>,
}

impl<T: Clone + 'static> TimeoutBasedCollector<T> {
    fn new(delay: u64, callback: impl Fn(Vec<T>) -> Promise<()> + 'static) -> Rc<TimeoutBasedCollector<T>> {
        Rc::new_cyclic(|this| TimeoutBasedCollector {
            this: this.clone(),
            delay,
            callback: Rc::new(callback),
            batch: RefCell::new(Vec::new()),
            started_timer: Cell::new(runtime::date_now()),
            current_deferred: RefCell::new(None),
        })
    }

    fn add_item(&self, item: T) -> Promise<()> {
        self.batch.borrow_mut().push(item);
        if self.current_deferred.borrow().is_none() {
            *self.current_deferred.borrow_mut() = Some(Rc::new(DeferredPromise::new()));
            self.started_timer.set(runtime::date_now());
            let this = self.this.upgrade().expect("o coletor está vivo enquanto um método dele roda");
            let _ = timeout(self.delay).then_promise(move |_| this.flush());
        }

        // This can be called by the extension repeatedly for a long time before the timeout is able to run.
        // Force a flush after the delay.
        if runtime::date_now().saturating_sub(self.started_timer.get()) > self.delay {
            return self.flush();
        }

        let deferred = self.current_deferred.borrow().clone().expect("o deferred foi criado acima");
        deferred.promise.clone()
    }

    fn flush(&self) -> Promise<()> {
        let deferred = self.current_deferred.borrow().clone();
        let Some(deferred) = deferred else {
            return Promise::resolved(());
        };
        if self.batch.borrow().is_empty() {
            return Promise::resolved(());
        }

        *self.current_deferred.borrow_mut() = None;
        let batch = std::mem::take(&mut *self.batch.borrow_mut());
        (self.callback)(batch).finally(move || {
            let _ = deferred.complete(());
        })
    }
}

/// `NotebookCellExecutionTask.update`: manda as atualizações à thread principal.
fn send_updates(proxy: &Rc<dyn MainThreadNotebookKernelsShape>, handle: u32, updates: Vec<ICellExecuteUpdateDto>) -> Promise<()> {
    proxy.rpc_update_execution(handle, SerializableObjectWithBuffers::new(updates));
    Promise::resolved(())
}

/// `NotebookCellExecutionTask.HANDLE`.
static CELL_EXECUTION_HANDLE: AtomicU32 = AtomicU32::new(0);

/// `NotebookCellExecutionTask`.
struct NotebookCellExecutionTask {
    base: Rc<Disposable>,
    handle: u32,
    on_did_change_state: Rc<Emitter<()>>,
    state: Cell<ExecutionTaskState>,
    token_source: CancellationTokenSource,
    collector: Rc<TimeoutBasedCollector<ICellExecuteUpdateDto>>,
    execution_order: Cell<Option<f64>>,
    cell: Rc<ExtHostCell>,
    proxy: Rc<dyn MainThreadNotebookKernelsShape>,
}

impl NotebookCellExecutionTask {
    fn new(controller_id: &str, cell: Rc<ExtHostCell>, proxy: Rc<dyn MainThreadNotebookKernelsShape>) -> Rc<NotebookCellExecutionTask> {
        let notebook = cell.notebook.upgrade().unwrap_or_else(|| errors::throw(Error::new("the notebook of the cell is gone")));

        let base = Disposable::new();
        let on_did_change_state: Rc<Emitter<()>> = Emitter::new();
        base.register(on_did_change_state.clone());
        let token_source = CancellationTokenSource::new(None);
        base.register(Rc::new(token_source.clone()));

        let handle = CELL_EXECUTION_HANDLE.fetch_add(1, Ordering::SeqCst);
        let collector = TimeoutBasedCollector::new(10, {
            let proxy = proxy.clone();
            move |updates| send_updates(&proxy, handle, updates)
        });

        let execution_order = cell.internal_metadata().execution_order;
        proxy.rpc_create_execution(handle, controller_id, &notebook.uri, cell.handle);

        Rc::new(NotebookCellExecutionTask {
            base,
            handle,
            on_did_change_state,
            state: Cell::new(ExecutionTaskState::Init),
            token_source,
            collector,
            execution_order: Cell::new(execution_order),
            cell,
            proxy,
        })
    }

    fn cancel(&self) {
        self.token_source.cancel();
    }

    fn update_soon(&self, update: ICellExecuteUpdateDto) -> Promise<()> {
        self.collector.add_item(update)
    }

    fn update(&self, updates: Vec<ICellExecuteUpdateDto>) -> Promise<()> {
        send_updates(&self.proxy, self.handle, updates)
    }

    fn verify_state_for_output(&self) -> Result<(), Error> {
        if self.state.get() == ExecutionTaskState::Init {
            return Err(Error::new("Must call start before modifying cell output"));
        }

        if self.state.get() == ExecutionTaskState::Resolved {
            return Err(Error::new("Cannot modify cell output after calling resolve"));
        }
        Ok(())
    }

    fn cell_index_to_handle(&self, cell_or_cell_index: Option<&NotebookCell>) -> Result<i64, Error> {
        let cell = match cell_or_cell_index {
            Some(api_cell) => self.cell.notebook.upgrade().and_then(|notebook| notebook.get_cell_from_api_cell(api_cell)),
            None => Some(self.cell.clone()),
        };
        cell.map(|cell| cell.handle).ok_or_else(|| Error::new("INVALID cell"))
    }

    fn validate_and_convert_outputs(items: &[types::NotebookCellOutput]) -> Vec<NotebookOutputDto> {
        items
            .iter()
            .map(|output| {
                let new_output = types::NotebookCellOutput::ensure_unique_mime_types(output.items.clone());
                type_converters::notebook::NotebookCellOutput::from(&types::NotebookCellOutput { items: new_output, id: output.id.clone(), metadata: output.metadata.clone() })
            })
            .collect()
    }

    fn update_outputs(self: &Rc<Self>, outputs: Vec<types::NotebookCellOutput>, cell: Option<&NotebookCell>, append: bool) -> Promise<()> {
        let handle = match self.cell_index_to_handle(cell) {
            Ok(handle) => handle,
            Err(error) => return Promise::rejected(error),
        };
        let output_dtos = Self::validate_and_convert_outputs(&outputs);
        self.update_soon(ICellExecuteUpdateDto::Output(ICellExecuteOutputEditDto { cell_handle: handle, append: Some(append), outputs: output_dtos }))
    }

    fn update_output_items(&self, items: Vec<types::NotebookCellOutputItem>, output: &types::NotebookCellOutput, append: bool) -> Promise<()> {
        let items = types::NotebookCellOutput::ensure_unique_mime_types(items);
        self.update_soon(ICellExecuteUpdateDto::OutputItems(ICellExecuteOutputItemEditDto {
            items: items.iter().map(type_converters::notebook::NotebookCellOutputItem::from).collect(),
            output_id: output.id.clone(),
            append: Some(append),
        }))
    }

    fn as_api_object(self: &Rc<Self>) -> NotebookCellExecution {
        NotebookCellExecution(self.clone())
    }
}

impl IDisposable for NotebookCellExecutionTask {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `createSerializeableError`.
fn create_serializeable_error(execution_error: Option<&CellExecutionError>) -> Option<ICellExecutionError> {
    let components_of = |uri: &URI| -> UriComponents { uri.to_json().components };

    let convert_range = |range: &types::Range| Range {
        start_line_number: range.start().line() as i32,
        start_column: range.start().character() as i32,
        end_line_number: range.end().line() as i32,
        end_column: range.end().character() as i32,
    };

    let convert_stack_frame = |frame: &CellErrorStackFrame| ICellErrorStackFrame { uri: frame.uri.as_ref().map(components_of), location: None, label: frame.label.clone() };

    execution_error.map(|execution_error| ICellExecutionError {
        name: execution_error.name.clone(),
        message: execution_error.message.clone(),
        stack: execution_error.stack.as_ref().map(|stack| match stack {
            CellExecutionStack::Frames(frames) => CellExecutionErrorStack::Frames(frames.iter().map(convert_stack_frame).collect()),
            CellExecutionStack::Text(text) => CellExecutionErrorStack::Text(text.clone()),
        }),
        location: execution_error.location.as_ref().map(convert_range),
        uri: components_of(&execution_error.uri),
    })
}

/// `vscode.NotebookCellExecution`.
#[derive(Clone)]
pub struct NotebookCellExecution(Rc<NotebookCellExecutionTask>);

impl NotebookCellExecution {
    /// `token`.
    pub fn token(&self) -> CancellationToken {
        self.0.token_source.token()
    }

    /// `cell`.
    pub fn cell(&self) -> NotebookCell {
        self.0.cell.api_cell()
    }

    /// `executionOrder`.
    pub fn execution_order(&self) -> Option<f64> {
        self.0.execution_order.get()
    }

    pub fn set_execution_order(&self, value: Option<f64>) {
        self.0.execution_order.set(value);
        let _ = self.0.update(vec![ICellExecuteUpdateDto::ExecutionState(ICellExecutionStateUpdate { execution_order: value, ..ICellExecutionStateUpdate::default() })]);
    }

    pub fn start(&self, start_time: Option<f64>) -> Result<(), Error> {
        let task = &self.0;
        if task.state.get() == ExecutionTaskState::Resolved || task.state.get() == ExecutionTaskState::Started {
            return Err(Error::new("Cannot call start again"));
        }

        task.state.set(ExecutionTaskState::Started);
        task.on_did_change_state.fire(());

        let _ = task.update(vec![ICellExecuteUpdateDto::ExecutionState(ICellExecutionStateUpdate { run_start_time: start_time, ..ICellExecutionStateUpdate::default() })]);
        Ok(())
    }

    pub fn end(&self, success: Option<bool>, end_time: Option<f64>, execution_error: Option<&CellExecutionError>) -> Result<(), Error> {
        let task = &self.0;
        if task.state.get() == ExecutionTaskState::Resolved {
            return Err(Error::new("Cannot call resolve twice"));
        }

        task.state.set(ExecutionTaskState::Resolved);
        task.on_did_change_state.fire(());

        // The last update needs to be ordered correctly and applied immediately,
        // so we use updateSoon and immediately flush.
        let _ = task.collector.flush();

        let error = create_serializeable_error(execution_error);

        task.proxy.rpc_complete_execution(
            task.handle,
            SerializableObjectWithBuffers::new(ICellExecutionCompleteDto { run_end_time: end_time, last_run_success: success, error }),
        );
        Ok(())
    }

    pub fn clear_output(&self, cell: Option<&NotebookCell>) -> Result<Promise<()>, Error> {
        self.0.verify_state_for_output()?;
        Ok(self.0.update_outputs(Vec::new(), cell, false))
    }

    pub fn append_output(&self, outputs: Vec<types::NotebookCellOutput>, cell: Option<&NotebookCell>) -> Result<Promise<()>, Error> {
        self.0.verify_state_for_output()?;
        Ok(self.0.update_outputs(outputs, cell, true))
    }

    pub fn replace_output(&self, outputs: Vec<types::NotebookCellOutput>, cell: Option<&NotebookCell>) -> Result<Promise<()>, Error> {
        self.0.verify_state_for_output()?;
        Ok(self.0.update_outputs(outputs, cell, false))
    }

    pub fn append_output_items(&self, items: Vec<types::NotebookCellOutputItem>, output: &types::NotebookCellOutput) -> Result<Promise<()>, Error> {
        self.0.verify_state_for_output()?;
        Ok(self.0.update_output_items(items, output, true))
    }

    pub fn replace_output_items(&self, items: Vec<types::NotebookCellOutputItem>, output: &types::NotebookCellOutput) -> Result<Promise<()>, Error> {
        self.0.verify_state_for_output()?;
        Ok(self.0.update_output_items(items, output, false))
    }
}

/// `NotebookExecutionTask.HANDLE`.
static NOTEBOOK_EXECUTION_HANDLE: AtomicU32 = AtomicU32::new(0);

/// `NotebookExecutionTask`.
struct NotebookExecutionTask {
    base: Rc<Disposable>,
    handle: u32,
    on_did_change_state: Rc<Emitter<()>>,
    state: Cell<ExecutionTaskState>,
    token_source: CancellationTokenSource,
    proxy: Rc<dyn MainThreadNotebookKernelsShape>,
}

impl NotebookExecutionTask {
    fn new(controller_id: &str, notebook: &Rc<ExtHostNotebookDocument>, proxy: Rc<dyn MainThreadNotebookKernelsShape>) -> Rc<NotebookExecutionTask> {
        let base = Disposable::new();
        let on_did_change_state: Rc<Emitter<()>> = Emitter::new();
        base.register(on_did_change_state.clone());
        let token_source = CancellationTokenSource::new(None);
        base.register(Rc::new(token_source.clone()));

        let handle = NOTEBOOK_EXECUTION_HANDLE.fetch_add(1, Ordering::SeqCst);
        proxy.rpc_create_notebook_execution(handle, controller_id, &notebook.uri);

        Rc::new(NotebookExecutionTask { base, handle, on_did_change_state, state: Cell::new(ExecutionTaskState::Init), token_source, proxy })
    }

    #[allow(dead_code)]
    fn cancel(&self) {
        self.token_source.cancel();
    }

    fn as_api_object(self: &Rc<Self>) -> NotebookExecution {
        NotebookExecution(self.clone())
    }
}

impl IDisposable for NotebookExecutionTask {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `vscode.NotebookExecution`.
#[derive(Clone)]
pub struct NotebookExecution(Rc<NotebookExecutionTask>);

impl NotebookExecution {
    pub fn start(&self) -> Result<(), Error> {
        let task = &self.0;
        if task.state.get() == ExecutionTaskState::Resolved || task.state.get() == ExecutionTaskState::Started {
            return Err(Error::new("Cannot call start again"));
        }

        task.state.set(ExecutionTaskState::Started);
        task.on_did_change_state.fire(());

        task.proxy.rpc_begin_notebook_execution(task.handle);
        Ok(())
    }

    pub fn end(&self) -> Result<(), Error> {
        let task = &self.0;
        if task.state.get() == ExecutionTaskState::Resolved {
            return Err(Error::new("Cannot call resolve twice"));
        }

        task.state.set(ExecutionTaskState::Resolved);
        task.on_did_change_state.fire(());

        task.proxy.rpc_complete_notebook_execution(task.handle);
        Ok(())
    }
}

// ---

/// O estado de um `vscode.NotebookController`: o que `createNotebookController` guarda em fechos e em `IKernelData`.
struct ControllerInner {
    extension: ExtensionDescription,
    handle: u32,
    id: String,
    data: RefCell<INotebookKernelDto2>,
    is_disposed: Rc<Cell<bool>>,
    token_pool: Cell<u64>,
    execute_handler: RefCell<NotebookExecuteHandler>,
    interrupt_handler: RefCell<Option<NotebookInterruptHandler>>,
    variable_provider: RefCell<Option<Rc<dyn NotebookVariableProvider>>>,
    variable_provider_disposable: RefCell<Option<DisposableRef>>,
    /// notebook documents that are associated to this controller
    associated_notebooks: Rc<RefCell<ResourceMap<bool>>>,
    on_did_change_selection: Rc<Emitter<NotebookSelectionChange>>,
    on_did_receive_message: Rc<Emitter<NotebookKernelMessage>>,
    proxy: Rc<dyn MainThreadNotebookKernelsShape>,
    ext_host_notebook: Rc<ExtHostNotebookController>,
    kernels: Weak<ExtHostNotebookKernels>,
    log_service: LogServiceRef,
}

impl ControllerInner {
    // update: all setters write directly into the dto object
    // and trigger an update. the actual update will only happen
    // once per event loop execution
    fn update(self: &Rc<Self>) {
        if self.is_disposed.get() {
            return;
        }
        let my_token = self.token_pool.get() + 1;
        self.token_pool.set(my_token);
        let this = self.clone();
        runtime::queue_microtask(move || {
            if my_token == this.token_pool.get() {
                let data = this.data.borrow().clone();
                this.proxy.rpc_update_kernel(this.handle, data);
            }
        });
    }

    fn check_associated(&self, notebook_uri: &URI) -> Result<(), Error> {
        if !self.associated_notebooks.borrow().has(notebook_uri) {
            let associated: Vec<String> = self.associated_notebooks.borrow().keys().map(|uri| uri.to_string(false).to_string_lossy()).collect();
            self.log_service.trace(&format!("NotebookController[{}] NOT associated to notebook, associated to THESE notebooks: {}", self.handle, associated.join(", ")));
            return Err(Error::new(format!("notebook controller is NOT associated to notebook: {}", notebook_uri.to_string(false))));
        }
        Ok(())
    }

    fn kernels(&self) -> Result<Rc<ExtHostNotebookKernels>, Error> {
        self.kernels.upgrade().ok_or_else(|| Error::new("ExtHostNotebookKernels is disposed"))
    }
}

/// `vscode.NotebookController`.
#[derive(Clone)]
pub struct NotebookController(Rc<ControllerInner>);

impl NotebookController {
    /// A identidade (`a === b`).
    pub fn ptr_eq(a: &NotebookController, b: &NotebookController) -> bool {
        Rc::ptr_eq(&a.0, &b.0)
    }

    pub fn id(&self) -> &str {
        &self.0.id
    }

    pub fn notebook_type(&self) -> String {
        self.0.data.borrow().notebook_type.clone()
    }

    pub fn on_did_change_selected_notebooks(&self) -> Event<NotebookSelectionChange> {
        self.0.on_did_change_selection.event()
    }

    pub fn label(&self) -> String {
        self.0.data.borrow().label.clone()
    }

    pub fn set_label(&self, value: Option<&str>) {
        let label = match value {
            Some(value) => value.to_owned(),
            None => self.0.extension.manifest.display_name.clone().unwrap_or_else(|| self.0.extension.manifest.name.clone()),
        };
        self.0.data.borrow_mut().label = label;
        self.0.update();
    }

    pub fn detail(&self) -> String {
        self.0.data.borrow().detail.clone().unwrap_or_default()
    }

    pub fn set_detail(&self, value: Option<String>) {
        self.0.data.borrow_mut().detail = value;
        self.0.update();
    }

    pub fn description(&self) -> String {
        self.0.data.borrow().description.clone().unwrap_or_default()
    }

    pub fn set_description(&self, value: Option<String>) {
        self.0.data.borrow_mut().description = value;
        self.0.update();
    }

    pub fn supported_languages(&self) -> Option<Vec<String>> {
        self.0.data.borrow().supported_languages.clone()
    }

    pub fn set_supported_languages(&self, value: Option<Vec<String>>) {
        self.0.data.borrow_mut().supported_languages = value;
        self.0.update();
    }

    pub fn supports_execution_order(&self) -> bool {
        self.0.data.borrow().supports_execution_order.unwrap_or(false)
    }

    pub fn set_supports_execution_order(&self, value: Option<bool>) {
        self.0.data.borrow_mut().supports_execution_order = value;
        self.0.update();
    }

    pub fn renderer_scripts(&self) -> Vec<NotebookRendererScript> {
        self.0.data.borrow().preloads.clone().unwrap_or_default()
    }

    pub fn execute_handler(&self) -> NotebookExecuteHandler {
        self.0.execute_handler.borrow().clone()
    }

    pub fn set_execute_handler(&self, value: Option<NotebookExecuteHandler>) {
        let handler = value.unwrap_or_else(|| default_execute_handler(self.0.data.borrow().id.clone(), self.0.extension.identifier.value.clone()));
        *self.0.execute_handler.borrow_mut() = handler;
    }

    pub fn interrupt_handler(&self) -> Option<NotebookInterruptHandler> {
        self.0.interrupt_handler.borrow().clone()
    }

    pub fn set_interrupt_handler(&self, value: Option<NotebookInterruptHandler>) {
        let supports_interrupt = value.is_some();
        *self.0.interrupt_handler.borrow_mut() = value;
        self.0.data.borrow_mut().supports_interrupt = Some(supports_interrupt);
        self.0.update();
    }

    pub fn variable_provider(&self) -> Option<Rc<dyn NotebookVariableProvider>> {
        self.0.variable_provider.borrow().clone()
    }

    /// O setter de `variableProvider`: lança se a extensão não declara `notebookVariableProvider`.
    pub fn set_variable_provider(&self, value: Option<Rc<dyn NotebookVariableProvider>>) -> Result<(), Error> {
        check_proposed_api_enabled(&self.0.extension, "notebookVariableProvider")?;
        let previous = self.0.variable_provider_disposable.borrow_mut().take();
        if let Some(previous) = previous {
            previous.dispose();
        }
        self.0.data.borrow_mut().has_variable_provider = Some(value.is_some());
        let proxy = self.0.proxy.clone();
        let subscription = value.as_ref().map(|provider| provider.on_did_change_variables().listen(move |change: &NotebookVariablesChange| proxy.rpc_variables_updated(&change.uri)));
        *self.0.variable_provider_disposable.borrow_mut() = subscription;
        *self.0.variable_provider.borrow_mut() = value;
        self.0.update();
        Ok(())
    }

    pub fn create_notebook_cell_execution(&self, cell: &NotebookCell) -> Result<NotebookCellExecution, Error> {
        if self.0.is_disposed.get() {
            return Err(Error::new("notebook controller is DISPOSED"));
        }
        self.0.check_associated(cell.notebook().uri())?;
        self.0.kernels()?.create_notebook_cell_execution(cell, &create_kernel_id(&self.0.extension.identifier, &self.0.id))
    }

    /// `createNotebookExecution` (proposta `notebookExecution`).
    pub fn create_notebook_execution(&self, notebook: &NotebookDocument) -> Result<NotebookExecution, Error> {
        check_proposed_api_enabled(&self.0.extension, "notebookExecution")?;
        if self.0.is_disposed.get() {
            return Err(Error::new("notebook controller is DISPOSED"));
        }
        self.0.check_associated(notebook.uri())?;
        self.0.kernels()?.create_notebook_execution(notebook, &create_kernel_id(&self.0.extension.identifier, &self.0.id))
    }

    /// `updateNotebookAffinity`: lança se `Hidden` e a extensão não declara `notebookControllerAffinityHidden`.
    pub fn update_notebook_affinity(&self, notebook: &NotebookDocument, priority: types::NotebookControllerAffinity2) -> Result<(), Error> {
        if priority == types::NotebookControllerAffinity2::Hidden {
            // This api only adds an extra enum value, the function is the same, so just gate on the new value being passed
            // for proposedAPI check.
            check_proposed_api_enabled(&self.0.extension, "notebookControllerAffinityHidden")?;
        }
        self.0.proxy.rpc_update_notebook_priority(self.0.handle, notebook.uri(), Some(priority));
        Ok(())
    }

    // --- ipc

    pub fn on_did_receive_message(&self) -> Event<NotebookKernelMessage> {
        self.0.on_did_receive_message.event()
    }

    /// `postMessage` (proposta `notebookMessaging`).
    pub fn post_message(&self, message: Dynamic, editor: Option<&NotebookEditor>) -> Result<Promise<bool>, Error> {
        check_proposed_api_enabled(&self.0.extension, "notebookMessaging")?;
        let editor_id = editor.and_then(|editor| self.0.ext_host_notebook.get_id_by_editor(editor));
        Ok(self.0.proxy.rpc_post_message(self.0.handle, editor_id.as_deref(), message))
    }
}

impl IDisposable for NotebookController {
    fn dispose(&self) {
        let inner = &self.0;
        if !inner.is_disposed.get() {
            inner.log_service.trace(&format!("NotebookController[{}], DISPOSED", inner.handle));
            inner.is_disposed.set(true);
            if let Some(kernels) = inner.kernels.upgrade() {
                kernels.kernel_data.borrow_mut().remove(&inner.handle);
            }
            inner.on_did_change_selection.dispose();
            inner.on_did_receive_message.dispose();
            let variable_provider_disposable = inner.variable_provider_disposable.borrow_mut().take();
            if let Some(variable_provider_disposable) = variable_provider_disposable {
                variable_provider_disposable.dispose();
            }
            inner.proxy.rpc_remove_kernel(inner.handle);
        }
    }
}

// ---

/// `ExtHostNotebookKernels`.
pub struct ExtHostNotebookKernels {
    this: Weak<ExtHostNotebookKernels>,

    proxy: Rc<dyn MainThreadNotebookKernelsShape>,
    active_executions: RefCell<ResourceMap<Rc<NotebookCellExecutionTask>>>,
    active_notebook_executions: RefCell<ResourceMap<(Rc<NotebookExecutionTask>, DisposableRef)>>,

    /// `_kernelDetectionTask`: o valor do `Map` nunca é lido, então só os handles ficam.
    kernel_detection_task: RefCell<HashSet<u32>>,
    kernel_detection_task_handle_pool: Cell<u32>,

    kernel_source_action_providers: RefCell<HashMap<u32, Rc<dyn NotebookKernelSourceActionProvider>>>,
    kernel_source_action_provider_handle_pool: Cell<u32>,

    kernel_data: RefCell<HashMap<u32, NotebookController>>,
    handle_pool: Cell<u32>,

    id: Cell<u32>,
    variable_store: RefCell<HashMap<u32, Variable>>,

    ext_host_notebook: Rc<ExtHostNotebookController>,
    commands: Rc<ExtHostCommands>,
    log_service: LogServiceRef,
}

impl ExtHostNotebookKernels {
    pub fn new(
        main_context: &dyn IMainContext,
        ext_host_notebook: Rc<ExtHostNotebookController>,
        commands: &Rc<ExtHostCommands>,
        log_service: LogServiceRef,
    ) -> Result<Rc<ExtHostNotebookKernels>, Error> {
        let kernels = Rc::new_cyclic(|this| ExtHostNotebookKernels {
            this: this.clone(),
            proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_NOTEBOOK_KERNELS),
            active_executions: RefCell::new(ResourceMap::new()),
            active_notebook_executions: RefCell::new(ResourceMap::new()),
            kernel_detection_task: RefCell::new(HashSet::new()),
            kernel_detection_task_handle_pool: Cell::new(0),
            kernel_source_action_providers: RefCell::new(HashMap::new()),
            kernel_source_action_provider_handle_pool: Cell::new(0),
            kernel_data: RefCell::new(HashMap::new()),
            handle_pool: Cell::new(0),
            id: Cell::new(0),
            variable_store: RefCell::new(HashMap::new()),
            ext_host_notebook: ext_host_notebook.clone(),
            commands: commands.clone(),
            log_service,
        });

        // todo@rebornix @joyceerhl: move to APICommands once stabilized.
        let select_kernel_api_command = ApiCommand::new(
            "notebook.selectKernel",
            "_notebook.selectKernel",
            "Trigger kernel picker for specified notebook editor widget",
            vec![ApiCommandArgument::new("options", "Select kernel options", |_| true, move |v: &Dynamic| {
                if !v.is_truthy() {
                    return Ok(v.clone());
                }
                let has = |name: &str| v.has_property(&JsString::from(name));
                if has("notebookEditor") && has("id") {
                    let notebook_editor_id = editor_of(&v.get_str("notebookEditor")).and_then(|editor| ext_host_notebook.get_id_by_editor(&editor));
                    return Ok(Dynamic::object([
                        ("id", v.get_str("id")),
                        ("extension", v.get_str("extension")),
                        ("notebookEditorId", notebook_editor_id.map_or(Dynamic::Undefined, Dynamic::from)),
                    ]));
                } else if has("notebookEditor") {
                    let editor = editor_of(&v.get_str("notebookEditor"));
                    let Some(notebook_editor_id) = editor.as_ref().and_then(|editor| ext_host_notebook.get_id_by_editor(editor)) else {
                        let uri = editor.map_or_else(|| "undefined".to_owned(), |editor| editor.notebook().uri().to_string(false).to_string_lossy());
                        return Err(Error::new(format!("Cannot invoke 'notebook.selectKernel' for unrecognized notebook editor {uri}")));
                    };
                    if has("skipIfAlreadySelected") {
                        return Ok(Dynamic::object([
                            ("notebookEditorId", Dynamic::from(notebook_editor_id)),
                            ("skipIfAlreadySelected", v.get_str("skipIfAlreadySelected")),
                        ]));
                    }
                    return Ok(Dynamic::object([("notebookEditorId", Dynamic::from(notebook_editor_id))]));
                }
                Ok(v.clone())
            })],
            ApiCommandResult::void(),
        );

        let request_kernel_variables_api_command = ApiCommand::new(
            "vscode.executeNotebookVariableProvider",
            "_executeNotebookVariableProvider",
            "Execute notebook variable provider",
            vec![ApiCommandArgument::uri()],
            ApiCommandResult::new("A promise that resolves to an array of variables", |value: &Dynamic, _api_args: &[Dynamic], _converter: &CommandsConverter| match value {
                Dynamic::Array(variables) => Ok(Dynamic::array(variables.to_vec().iter().map(|variable| {
                    Dynamic::object([
                        (
                            "variable",
                            Dynamic::object([
                                ("name", variable.get_str("name")),
                                ("value", variable.get_str("value")),
                                ("expression", variable.get_str("expression")),
                                ("type", variable.get_str("type")),
                                ("language", variable.get_str("language")),
                            ]),
                        ),
                        ("hasNamedChildren", variable.get_str("hasNamedChildren")),
                        ("indexedChildrenCount", variable.get_str("indexedChildrenCount")),
                    ])
                }))),
                _ => Err(Error::type_error("value.map is not a function")),
            }),
        );
        commands.register_api_command(select_kernel_api_command)?;
        commands.register_api_command(request_kernel_variables_api_command)?;
        Ok(kernels)
    }

    /// O `Rc` deste host, para as operações assíncronas que precisam dele depois do retorno.
    fn rc(&self) -> Rc<ExtHostNotebookKernels> {
        self.this.upgrade().expect("o ExtHostNotebookKernels está vivo enquanto um método dele roda")
    }

    fn controller_of(&self, handle: u32) -> Option<NotebookController> {
        self.kernel_data.borrow().get(&handle).cloned()
    }

    pub fn create_notebook_controller(
        &self,
        extension: &ExtensionDescription,
        id: &str,
        view_type: &str,
        label: &str,
        handler: Option<NotebookExecuteHandler>,
        preloads: Option<Vec<NotebookRendererScript>>,
    ) -> Result<NotebookController, Error> {
        for controller in self.kernel_data.borrow().values() {
            if controller.0.id == id && extension.identifier == controller.0.extension.identifier {
                return Err(Error::new(format!("notebook controller with id '{id}' ALREADY exist")));
            }
        }

        let handle = self.handle_pool.get();
        self.handle_pool.set(handle + 1);

        self.log_service.trace(&format!("NotebookController[{handle}], CREATED by {}, {id}", extension.identifier.value));

        let data = INotebookKernelDto2 {
            id: create_kernel_id(&extension.identifier, id),
            notebook_type: view_type.to_owned(),
            extension_id: extension.identifier.clone(),
            extension_location: URI::parse(extension.extension_location.as_str(), false),
            label: if label.is_empty() { extension.identifier.value.clone() } else { label.to_owned() },
            detail: None,
            description: None,
            supported_languages: None,
            supports_interrupt: None,
            supports_execution_order: None,
            preloads: Some(preloads.unwrap_or_default()),
            has_variable_provider: None,
        };

        let execute_handler = handler.unwrap_or_else(|| default_execute_handler(data.id.clone(), extension.identifier.value.clone()));
        let is_disposed = Rc::new(Cell::new(false));

        let _ = self.proxy.rpc_add_kernel(handle, data.clone()).catch({
            let is_disposed = is_disposed.clone();
            move |err| {
                // this can happen when a kernel with that ID is already registered
                eprintln!("{err}");
                is_disposed.set(true);
            }
        });

        let controller = NotebookController(Rc::new(ControllerInner {
            extension: extension.clone(),
            handle,
            id: id.to_owned(),
            data: RefCell::new(data),
            is_disposed,
            token_pool: Cell::new(0),
            execute_handler: RefCell::new(execute_handler),
            interrupt_handler: RefCell::new(None),
            variable_provider: RefCell::new(None),
            variable_provider_disposable: RefCell::new(None),
            associated_notebooks: Rc::new(RefCell::new(ResourceMap::new())),
            on_did_change_selection: Emitter::new(),
            on_did_receive_message: Emitter::new(),
            proxy: self.proxy.clone(),
            ext_host_notebook: self.ext_host_notebook.clone(),
            kernels: self.this.clone(),
            log_service: self.log_service.clone(),
        }));

        self.kernel_data.borrow_mut().insert(handle, controller.clone());
        Ok(controller)
    }

    pub fn get_id_by_controller(&self, controller: &NotebookController) -> Option<String> {
        self.kernel_data
            .borrow()
            .values()
            .find(|candidate| NotebookController::ptr_eq(candidate, controller))
            .map(|candidate| create_kernel_id(&candidate.0.extension.identifier, controller.id()))
    }

    pub fn create_notebook_controller_detection_task(&self, extension: &ExtensionDescription, view_type: &str) -> types::Disposable {
        let handle = self.kernel_detection_task_handle_pool.get();
        self.kernel_detection_task_handle_pool.set(handle + 1);

        self.log_service.trace(&format!("NotebookControllerDetectionTask[{handle}], CREATED by {}", extension.identifier.value));
        let _ = self.proxy.rpc_add_kernel_detection_task(handle, view_type);

        self.kernel_detection_task.borrow_mut().insert(handle);
        let this = self.this.clone();
        types::Disposable::new(move || {
            let Some(this) = this.upgrade() else {
                return;
            };
            this.kernel_detection_task.borrow_mut().remove(&handle);
            this.proxy.rpc_remove_kernel_detection_task(handle);
        })
    }

    pub fn register_kernel_source_action_provider(
        &self,
        extension: &ExtensionDescription,
        view_type: &str,
        provider: Rc<dyn NotebookKernelSourceActionProvider>,
    ) -> types::Disposable {
        let handle = self.kernel_source_action_provider_handle_pool.get();
        self.kernel_source_action_provider_handle_pool.set(handle + 1);
        let on_did_change = provider.on_did_change_notebook_kernel_source_actions();
        let event_handle = on_did_change.as_ref().map(|_| handle);

        self.kernel_source_action_providers.borrow_mut().insert(handle, provider);
        self.log_service.trace(&format!("NotebookKernelSourceActionProvider[{handle}], CREATED by {}", extension.identifier.value));
        let _ = self.proxy.rpc_add_kernel_source_action_provider(handle, handle, view_type);

        let subscription: Option<DisposableRef> = match (on_did_change, event_handle) {
            (Some(event), Some(event_handle)) => {
                let proxy = self.proxy.clone();
                Some(event.listen(move |_| proxy.rpc_emit_notebook_kernel_source_actions_change_event(event_handle)))
            }
            _ => None,
        };

        let this = self.this.clone();
        types::Disposable::new(move || {
            let Some(this) = this.upgrade() else {
                return;
            };
            this.kernel_source_action_providers.borrow_mut().remove(&handle);
            this.proxy.rpc_remove_kernel_source_action_provider(handle, handle);
            if let Some(subscription) = &subscription {
                subscription.dispose();
            }
        })
    }

    // ---

    /// `_createNotebookCellExecution`.
    pub fn create_notebook_cell_execution(&self, cell: &NotebookCell, controller_id: &str) -> Result<NotebookCellExecution, Error> {
        if cell.index() < 0.0 {
            return Err(Error::new("CANNOT execute cell that has been REMOVED from notebook"));
        }
        let notebook_uri = cell.notebook().uri().clone();
        let notebook = self
            .ext_host_notebook
            .try_get_notebook_document(&notebook_uri)
            .ok_or_else(|| Error::new(format!("NO notebook document for '{}'", notebook_uri.to_string(false))))?;
        let cell_obj = notebook.get_cell_from_api_cell(cell).ok_or_else(|| Error::new("invalid cell"))?;
        if self.active_executions.borrow().has(&cell_obj.uri) {
            return Err(Error::new(format!("duplicate execution for {}", cell_obj.uri.to_string(false))));
        }
        let execution = NotebookCellExecutionTask::new(controller_id, cell_obj.clone(), self.proxy.clone());
        self.active_executions.borrow_mut().set(cell_obj.uri.clone(), execution.clone());

        let listener_slot: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));
        let listener = execution.on_did_change_state.event().listen({
            let this = self.this.clone();
            let execution = Rc::downgrade(&execution);
            let listener_slot = listener_slot.clone();
            let uri = cell_obj.uri.clone();
            move |_| {
                let Some(execution) = execution.upgrade() else {
                    return;
                };
                if execution.state.get() == ExecutionTaskState::Resolved {
                    execution.dispose();
                    let listener = listener_slot.borrow_mut().take();
                    if let Some(listener) = listener {
                        listener.dispose();
                    }
                    if let Some(this) = this.upgrade() {
                        this.active_executions.borrow_mut().delete(&uri);
                    }
                }
            }
        });
        *listener_slot.borrow_mut() = Some(listener);
        Ok(execution.as_api_object())
    }

    // ---

    /// `_createNotebookExecution`.
    pub fn create_notebook_execution(&self, nb: &NotebookDocument, controller_id: &str) -> Result<NotebookExecution, Error> {
        let notebook = self
            .ext_host_notebook
            .try_get_notebook_document(nb.uri())
            .ok_or_else(|| Error::new(format!("NO notebook document for '{}'", nb.uri().to_string(false))))?;
        let running_cell = nb.get_cells(None).into_iter().find(|cell| {
            let api_cell = notebook.get_cell_from_api_cell(cell);
            api_cell.is_some_and(|api_cell| self.active_executions.borrow().has(&api_cell.uri))
        });
        if let Some(running_cell) = running_cell {
            return Err(Error::new(format!("duplicate cell execution for {}", running_cell.document().uri().to_string(false))));
        }
        if self.active_notebook_executions.borrow().has(&notebook.uri) {
            return Err(Error::new(format!("duplicate notebook execution for {}", notebook.uri.to_string(false))));
        }
        let execution = NotebookExecutionTask::new(controller_id, &notebook, self.proxy.clone());
        let listener_slot: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));
        let listener = execution.on_did_change_state.event().listen({
            let this = self.this.clone();
            let execution = Rc::downgrade(&execution);
            let listener_slot = listener_slot.clone();
            let uri = notebook.uri.clone();
            move |_| {
                let Some(execution) = execution.upgrade() else {
                    return;
                };
                if execution.state.get() == ExecutionTaskState::Resolved {
                    execution.dispose();
                    let listener = listener_slot.borrow_mut().take();
                    if let Some(listener) = listener {
                        listener.dispose();
                    }
                    if let Some(this) = this.upgrade() {
                        this.active_notebook_executions.borrow_mut().delete(&uri);
                    }
                }
            }
        });
        *listener_slot.borrow_mut() = Some(listener.clone());
        self.active_notebook_executions.borrow_mut().set(notebook.uri.clone(), (execution.clone(), listener));
        Ok(execution.as_api_object())
    }
}

/// `ICommandsConverter.toInternal` aplicado ao `command` de uma ação de origem de kernel.
fn kernel_source_action_from(
    item: &NotebookKernelSourceAction,
    commands_converter: &dyn ICommandsConverter,
    disposables: &DisposableStore,
) -> Result<INotebookKernelSourceAction, Error> {
    let command = match &item.command {
        Some(NotebookCellStatusBarCommand::Id(id)) => Some(types::Command::new("", id.as_str())),
        Some(NotebookCellStatusBarCommand::Command(command)) => Some(command.clone()),
        None => None,
    };

    Ok(INotebookKernelSourceAction {
        command: commands_converter.to_internal(command.as_ref(), disposables)?.map(|dto| {
            StringOrCommand::Command(Command { id: dto.id, title: dto.title, tooltip: dto.tooltip, arguments: dto.arguments })
        }),
        label: item.label.clone(),
        description: item.description.clone(),
        detail: item.detail.clone(),
        documentation: item.documentation.clone(),
    })
}

impl ExtHostNotebookKernelsShape for ExtHostNotebookKernels {
    fn rpc_provide_kernel_source_actions(&self, handle: u32, token: CancellationToken) -> Promise<Vec<INotebookKernelSourceAction>> {
        let provider = self.kernel_source_action_providers.borrow().get(&handle).cloned();
        let converter = self.commands.converter();
        async_fn(async move {
            let Some(provider) = provider else {
                return Ok(Vec::new());
            };
            let disposables = DisposableStore::new();
            let ret = await_maybe(provider.provide_notebook_kernel_source_actions(token)).await?;
            ret.unwrap_or_default().iter().map(|item| kernel_source_action_from(item, &*converter, &disposables)).collect()
        })
    }

    fn rpc_accept_notebook_association(&self, handle: u32, uri: &URI, value: bool) {
        let Some(controller) = self.controller_of(handle) else {
            return;
        };
        // update data structure
        let notebook = self.ext_host_notebook.get_notebook_document(uri);
        if value {
            controller.0.associated_notebooks.borrow_mut().set(notebook.uri.clone(), true);
        } else {
            controller.0.associated_notebooks.borrow_mut().delete(&notebook.uri);
        }
        self.log_service.trace(&format!("NotebookController[{handle}] ASSOCIATE notebook {} {value}", notebook.uri.to_string(false)));
        // send event
        controller.0.on_did_change_selection.fire(NotebookSelectionChange { selected: value, notebook: notebook.api_notebook() });
    }

    fn rpc_execute_cells(&self, handle: u32, uri: &URI, handles: Vec<i64>) -> Promise<()> {
        let this = self.rc();
        let uri = uri.clone();
        async_fn(async move {
            let Some(controller) = this.controller_of(handle) else {
                // extension can dispose kernels in the meantime
                return Ok(());
            };
            let document = this.ext_host_notebook.get_notebook_document(&uri);
            let mut cells: Vec<NotebookCell> = Vec::new();
            for cell_handle in handles {
                if let Some(cell) = document.get_cell(cell_handle) {
                    cells.push(cell.api_cell());
                }
            }

            this.log_service.trace(&format!("NotebookController[{handle}] EXECUTE cells {} {}", document.uri.to_string(false), cells.len()));
            let execute_handler = controller.execute_handler();
            let api_notebook = document.api_notebook();
            let outcome = match errors::catch(|| execute_handler(cells, api_notebook, controller.clone())) {
                Ok(result) => await_maybe(result).await,
                Err(error) => Err(error),
            };
            if let Err(err) = outcome {
                //
                this.log_service.error(&format!("NotebookController[{handle}] execute cells FAILED {err}"));
                eprintln!("{err}");
            }
            Ok(())
        })
    }

    fn rpc_cancel_cells(&self, handle: u32, uri: &URI, handles: Vec<i64>) -> Promise<()> {
        let this = self.rc();
        let uri = uri.clone();
        async_fn(async move {
            let Some(controller) = this.controller_of(handle) else {
                // extension can dispose kernels in the meantime
                return Ok(());
            };

            // cancel or interrupt depends on the controller. When an interrupt handler is used we
            // don't trigger the cancelation token of executions.
            let document = this.ext_host_notebook.get_notebook_document(&uri);
            if let Some(interrupt_handler) = controller.interrupt_handler() {
                await_maybe(interrupt_handler(document.api_notebook())).await?;
            } else {
                for cell_handle in handles.iter().copied() {
                    if let Some(cell) = document.get_cell(cell_handle) {
                        let execution = this.active_executions.borrow().get(&cell.uri).cloned();
                        if let Some(execution) = execution {
                            execution.cancel();
                        }
                    }
                }
            }

            if controller.interrupt_handler().is_some() {
                // If we're interrupting all cells, we also need to cancel the notebook level execution.
                let items = this.active_notebook_executions.borrow().get(&document.uri).cloned();
                this.active_notebook_executions.borrow_mut().delete(&document.uri);
                if let Some((execution, listener)) = items
                    && !handles.is_empty()
                {
                    execution.dispose();
                    listener.dispose();
                }
            }
            Ok(())
        })
    }

    fn rpc_provide_variables(
        &self,
        handle: u32,
        request_id: &str,
        notebook_uri: &URI,
        parent_id: Option<u32>,
        kind: VariablesRequestKindDto,
        start: usize,
        token: CancellationToken,
    ) -> Promise<()> {
        let this = self.rc();
        let request_id = request_id.to_owned();
        let notebook_uri = notebook_uri.clone();
        async_fn(async move {
            let Some(controller) = this.controller_of(handle) else {
                return Ok(());
            };

            let document = this.ext_host_notebook.get_notebook_document(&notebook_uri);
            let variable_provider = controller.variable_provider();
            let Some(variable_provider) = variable_provider else {
                return Ok(());
            };

            let parent: Option<Variable> = match parent_id {
                Some(parent_id) => {
                    let parent = this.variable_store.borrow().get(&parent_id).cloned();
                    if parent.is_none() {
                        // request for unknown parent
                        return Ok(());
                    }
                    parent
                }
                None => {
                    // root request, clear store
                    this.variable_store.borrow_mut().clear();
                    None
                }
            };

            let request_kind = match kind {
                VariablesRequestKindDto::Named => types::NotebookVariablesRequestKind::Named,
                VariablesRequestKindDto::Indexed => types::NotebookVariablesRequestKind::Indexed,
            };
            let variable_results = variable_provider.provide_variables(&document.api_notebook(), parent.as_ref(), request_kind, start, token.clone());

            let mut result_count: usize = 0;
            loop {
                let next = await_maybe(variable_results.next()).await?;
                let Some(result) = next else {
                    break;
                };
                if token.is_cancellation_requested() {
                    return Ok(());
                }
                let id = this.id.get();
                this.id.set(id + 1);
                let variable = VariablesResult {
                    id,
                    name: result.variable.name.clone(),
                    value: result.variable.value.clone(),
                    r#type: result.variable.r#type.clone(),
                    interfaces: result.variable.interfaces.clone(),
                    language: result.variable.language.clone(),
                    expression: result.variable.expression.clone(),
                    has_named_children: result.has_named_children,
                    indexed_children_count: result.indexed_children_count,
                    extension_id: controller.0.extension.identifier.value.clone(),
                };
                this.variable_store.borrow_mut().insert(id, result.variable);
                this.proxy.rpc_receive_variable(&request_id, variable);

                let previous_count = result_count;
                result_count += 1;
                if previous_count >= VARIABLE_PAGE_SIZE {
                    return Ok(());
                }
            }
            Ok(())
        })
    }

    fn rpc_accept_kernel_message_from_renderer(&self, handle: u32, editor_id: &str, message: Dynamic) {
        let Some(controller) = self.controller_of(handle) else {
            // extension can dispose kernels in the meantime
            return;
        };

        let editor = self.ext_host_notebook.get_editor_by_id(editor_id);
        controller.0.on_did_receive_message.fire(NotebookKernelMessage { editor: editor.api_editor(), message });
    }
}
