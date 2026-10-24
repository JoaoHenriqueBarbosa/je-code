//! Porte de `src/vs/workbench/contrib/notebook/browser/services/notebookExecutionStateServiceImpl.ts`: o
//! `NotebookExecutionStateService`, que guarda as execuções de célula e de notebook (e dispara os eventos de mudança), a
//! última célula que falhou e a última que completou, e os ouvintes que cancelam execuções quando uma célula é removida
//! ou o notebook é descartado.
//!
//! Divergências:
//! - Os serviços do construtor (`@INotebookService`, `@ILogService`, `@IAccessibilitySignalService`) e os que o
//!   `NotebookExecutionListeners` resolve por injeção (`@INotebookKernelService`, `@INotebookExecutionService`, e o próprio
//!   `@INotebookExecutionStateService`) são recebidos por [`NotebookExecutionStateService::new`], que devolve o serviço em um
//!   `Rc` (o ouvinte guarda um `Weak` do serviço). O `IInstantiationService.createInstance` de `CellExecution`,
//!   `NotebookExecution` e `NotebookExecutionListeners` é a chamada direta ao construtor com o `ILogService`.
//! - `ResourceMap<Map<number, CellExecution>>` é `ResourceMap<Rc<RefCell<OrderedMap<i64, Rc<CellExecution>>>>>`: o
//!   `ResourceMap` do porte não dá acesso mutável ao valor, e o `OrderedMap` guarda a ordem de inserção do `Map`.
//! - `[NotebookExecution, IDisposable]` é uma tupla; `combinedDisposable(a, b)` é o `combined_disposable`.
//! - `CellExecution.update` com `didPause` e `isPaused` lê os campos opcionais de `ICellExecutionStateUpdate`.
//! - `CellEditType.Output`, `OutputItems` e `PartialInternalMetadata` com `handle` são `OutputByHandle`, `OutputItems` e
//!   `PartialInternalMetadataByHandle`; as propriedades `undefined` do objeto de metadados são os `None` externos do
//!   `NullablePartialNotebookCellInternalMetadata`, e `null` é `Some(None)`.
//! - `CellExecutionUpdateType[u.editType]` (o nome do enum no log) é [`update_type_name`].
//! - `JSON.stringify([...handlesToCancel])` é a lista de inteiros sem espaços.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, combined_disposable};
use je_base::map::{OrderedMap, ResourceMap};
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_editor::code_action::platform::{AccessibilitySignal, IAccessibilitySignalService};
use je_platform::log::{LogServiceRef, Logger};
use je_platform::resources::ext_uri;

use crate::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use crate::contrib::notebook::common::notebook_common::{
    CellUri, ICell, ICellEditOperation, ICellOutputEditByHandle, ICellOutputItemEdit, ICellPartialInternalMetadataEditByHandle, NotebookCellExecutionState,
    NotebookExecutionState, NotebookTextModelWillAddRemoveEvent, NullablePartialNotebookCellInternalMetadata,
};
use crate::contrib::notebook::common::notebook_execution_service::{CellExecutionUpdateType, INotebookExecutionService};
use crate::contrib::notebook::common::notebook_execution_state_service::{
    ExecutionStateChange, ICellExecuteUpdate, ICellExecutionComplete, ICellExecutionStateChangedEvent, IExecutionStateChangedEvent, IFailedCellInfo, INotebookCellExecution,
    INotebookExecution, INotebookExecutionStateService, INotebookFailStateChangedEvent,
};
use crate::contrib::notebook::common::notebook_kernel_service::INotebookKernelService;
use crate::contrib::notebook::common::notebook_service::INotebookService;

type CellExecutionMap = Rc<RefCell<OrderedMap<i64, Rc<CellExecution>>>>;

/// `JSON.stringify(handles)`.
fn json_handles(handles: &[i64]) -> String {
    format!("[{}]", handles.iter().map(i64::to_string).collect::<Vec<_>>().join(","))
}

/// `isEqual(a, b)`.
fn is_equal(a: &URI, b: &URI) -> bool {
    ext_uri().is_equal(Some(a), Some(b), false)
}

/// `NotebookExecutionStateService`.
pub struct NotebookExecutionStateService {
    this: Weak<NotebookExecutionStateService>,
    base: Rc<Disposable>,

    notebook_service: Rc<dyn INotebookService>,
    notebook_kernel_service: Rc<dyn INotebookKernelService>,
    notebook_execution_service: Rc<dyn INotebookExecutionService>,
    log_service: LogServiceRef,
    accessibility_signal_service: Rc<dyn IAccessibilitySignalService>,

    executions: RefCell<ResourceMap<CellExecutionMap>>,
    notebook_executions: RefCell<ResourceMap<(Rc<NotebookExecution>, DisposableRef)>>,
    notebook_listeners: RefCell<ResourceMap<Rc<NotebookExecutionListeners>>>,
    cell_listeners: RefCell<ResourceMap<DisposableRef>>,
    last_failed_cells: RefCell<ResourceMap<IFailedCellInfo>>,
    last_completed_cell_handles: RefCell<ResourceMap<i64>>,

    on_did_change_execution: Rc<Emitter<ExecutionStateChange>>,
    on_did_change_last_run_fail_state: Rc<Emitter<INotebookFailStateChangedEvent>>,
}

impl NotebookExecutionStateService {
    pub fn new(
        notebook_service: Rc<dyn INotebookService>,
        notebook_kernel_service: Rc<dyn INotebookKernelService>,
        notebook_execution_service: Rc<dyn INotebookExecutionService>,
        log_service: LogServiceRef,
        accessibility_signal_service: Rc<dyn IAccessibilitySignalService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_execution = Emitter::<ExecutionStateChange>::new();
        let on_did_change_last_run_fail_state = Emitter::<INotebookFailStateChangedEvent>::new();
        base.register(on_did_change_execution.clone());
        base.register(on_did_change_last_run_fail_state.clone());
        Rc::new_cyclic(|weak| Self {
            this: weak.clone(),
            base,
            notebook_service,
            notebook_kernel_service,
            notebook_execution_service,
            log_service,
            accessibility_signal_service,
            executions: RefCell::new(ResourceMap::new()),
            notebook_executions: RefCell::new(ResourceMap::new()),
            notebook_listeners: RefCell::new(ResourceMap::new()),
            cell_listeners: RefCell::new(ResourceMap::new()),
            last_failed_cells: RefCell::new(ResourceMap::new()),
            last_completed_cell_handles: RefCell::new(ResourceMap::new()),
            on_did_change_execution,
            on_did_change_last_run_fail_state,
        })
    }

    fn executions_of(&self, notebook: &URI) -> Option<CellExecutionMap> {
        self.executions.borrow().get(notebook).cloned()
    }

    fn on_cell_execution_did_change(&self, notebook_uri: &URI, cell_handle: i64, exe: &Rc<CellExecution>) {
        self.on_did_change_execution.fire(ExecutionStateChange::Cell(Rc::new(NotebookCellExecutionEvent { notebook: notebook_uri.clone(), cell_handle, changed: Some(exe.clone()) })));
    }

    fn on_cell_execution_did_complete(&self, notebook_uri: &URI, cell_handle: i64, exe: &Rc<CellExecution>, last_run_success: Option<bool>) {
        let Some(notebook_executions) = self.executions_of(notebook_uri) else {
            self.log_service.debug(&format!("NotebookExecutionStateService#_onCellExecutionDidComplete - unknown notebook {}", notebook_uri.to_string(false).to_string_lossy()));
            return;
        };

        exe.dispose();
        let cell_uri = CellUri::generate(notebook_uri, cell_handle);
        let cell_listener = self.cell_listeners.borrow().get(&cell_uri).cloned();
        if let Some(cell_listener) = cell_listener {
            cell_listener.dispose();
        }
        self.cell_listeners.borrow_mut().delete(&cell_uri);
        notebook_executions.borrow_mut().remove(&cell_handle);
        if notebook_executions.borrow().len() == 0 {
            self.executions.borrow_mut().delete(notebook_uri);
            let notebook_listener = self.notebook_listeners.borrow().get(notebook_uri).cloned();
            if let Some(notebook_listener) = notebook_listener {
                notebook_listener.dispose();
            }
            self.notebook_listeners.borrow_mut().delete(notebook_uri);
        }

        if let Some(last_run_success) = last_run_success {
            if last_run_success {
                if self.executions.borrow().size() == 0 {
                    self.accessibility_signal_service.play_signal(AccessibilitySignal::NotebookCellCompleted);
                }
                self.clear_last_failed_cell(notebook_uri);
            } else {
                self.accessibility_signal_service.play_signal(AccessibilitySignal::NotebookCellFailed);
                self.set_last_failed_cell(notebook_uri, cell_handle);
            }
            self.last_completed_cell_handles.borrow_mut().set(notebook_uri.clone(), cell_handle);
        }

        self.on_did_change_execution.fire(ExecutionStateChange::Cell(Rc::new(NotebookCellExecutionEvent { notebook: notebook_uri.clone(), cell_handle, changed: None })));
    }

    fn on_execution_did_change(&self, notebook_uri: &URI, exe: &Rc<NotebookExecution>) {
        self.on_did_change_execution.fire(ExecutionStateChange::Notebook(Rc::new(NotebookExecutionEvent { notebook: notebook_uri.clone(), changed: Some(exe.clone()) })));
    }

    fn on_execution_did_complete(&self, notebook_uri: &URI) {
        let disposables = self.notebook_executions.borrow().get(notebook_uri).cloned();
        let Some((exe, disposable)) = disposables else {
            self.log_service.debug(&format!("NotebookExecutionStateService#_onCellExecutionDidComplete - unknown notebook {}", notebook_uri.to_string(false).to_string_lossy()));
            return;
        };

        self.notebook_executions.borrow_mut().delete(notebook_uri);
        self.on_did_change_execution.fire(ExecutionStateChange::Notebook(Rc::new(NotebookExecutionEvent { notebook: notebook_uri.clone(), changed: None })));
        // `disposables.forEach(d => d.dispose())`: a tupla `[NotebookExecution, IDisposable]` descarta a execução e as inscrições.
        exe.dispose();
        disposable.dispose();
    }

    fn create_notebook_cell_execution(&self, notebook: &Rc<NotebookTextModel>, cell_handle: i64) -> Rc<CellExecution> {
        let notebook_uri = notebook.uri();
        let exe = CellExecution::new(cell_handle, notebook.clone(), self.log_service.clone());
        let (weak, uri, handle) = (self.this.clone(), notebook_uri.clone(), cell_handle);
        let exe_for_update = Rc::downgrade(&exe);
        let on_update = exe.on_did_update.event().listen(move |_| {
            if let (Some(this), Some(exe)) = (weak.upgrade(), exe_for_update.upgrade()) {
                this.on_cell_execution_did_change(&uri, handle, &exe);
            }
        });
        let (weak, uri) = (self.this.clone(), notebook_uri.clone());
        let exe_for_complete = Rc::downgrade(&exe);
        let on_complete = exe.on_did_complete.event().listen(move |last_run_success| {
            if let (Some(this), Some(exe)) = (weak.upgrade(), exe_for_complete.upgrade()) {
                this.on_cell_execution_did_complete(&uri, handle, &exe, *last_run_success);
            }
        });
        let disposable = combined_disposable(vec![on_update, on_complete]);
        self.cell_listeners.borrow_mut().set(CellUri::generate(&notebook_uri, cell_handle), disposable);

        exe
    }

    fn create_notebook_execution(&self, notebook: &Rc<NotebookTextModel>) -> (Rc<NotebookExecution>, DisposableRef) {
        let notebook_uri = notebook.uri();
        let exe = NotebookExecution::new(notebook.clone(), self.log_service.clone());
        let (weak, uri) = (self.this.clone(), notebook_uri.clone());
        let exe_for_update = Rc::downgrade(&exe);
        let on_update = exe.on_did_update.event().listen(move |_| {
            if let (Some(this), Some(exe)) = (weak.upgrade(), exe_for_update.upgrade()) {
                this.on_execution_did_change(&uri, &exe);
            }
        });
        let weak = self.this.clone();
        let on_complete = exe.on_did_complete.event().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.on_execution_did_complete(&notebook_uri);
            }
        });
        (exe, combined_disposable(vec![on_update, on_complete]))
    }

    fn create_listeners(&self, notebook: &URI) -> Rc<NotebookExecutionListeners> {
        NotebookExecutionListeners::new(
            notebook,
            self.notebook_service.clone(),
            self.notebook_kernel_service.clone(),
            self.notebook_execution_service.clone(),
            self.this.clone(),
            self.log_service.clone(),
        )
    }

    fn set_last_failed_cell(&self, notebook_uri: &URI, cell_handle: i64) {
        let prev_last_failed_cell_info = self.last_failed_cells.borrow().get(notebook_uri).cloned();
        let Some(notebook) = self.notebook_service.get_notebook_text_model(notebook_uri) else { return };

        let new_last_failed_cell_info = IFailedCellInfo {
            cell_handle,
            disposable: match prev_last_failed_cell_info {
                Some(prev) => prev.disposable,
                None => self.get_failed_cell_listener(&notebook),
            },
            visible: true,
        };

        self.last_failed_cells.borrow_mut().set(notebook_uri.clone(), new_last_failed_cell_info);

        self.on_did_change_last_run_fail_state.fire(INotebookFailStateChangedEvent { visible: true, notebook: notebook_uri.clone() });
    }

    fn set_last_failed_cell_visibility(&self, notebook_uri: &URI, visible: bool) {
        let last_failed_cell_info = self.last_failed_cells.borrow().get(notebook_uri).cloned();

        if let Some(last_failed_cell_info) = last_failed_cell_info {
            self.last_failed_cells.borrow_mut().set(notebook_uri.clone(), IFailedCellInfo { cell_handle: last_failed_cell_info.cell_handle, disposable: last_failed_cell_info.disposable, visible });
        }

        self.on_did_change_last_run_fail_state.fire(INotebookFailStateChangedEvent { visible, notebook: notebook_uri.clone() });
    }

    fn clear_last_failed_cell(&self, notebook_uri: &URI) {
        let last_failed_cell_info = self.last_failed_cells.borrow().get(notebook_uri).cloned();

        if let Some(last_failed_cell_info) = last_failed_cell_info {
            last_failed_cell_info.disposable.dispose();
            self.last_failed_cells.borrow_mut().delete(notebook_uri);
        }

        self.on_did_change_last_run_fail_state.fire(INotebookFailStateChangedEvent { visible: false, notebook: notebook_uri.clone() });
    }

    fn get_failed_cell_listener(&self, notebook: &Rc<NotebookTextModel>) -> DisposableRef {
        let weak = self.this.clone();
        let notebook_ref = Rc::downgrade(notebook);
        notebook.on_will_add_remove_cells().listen(move |e: &NotebookTextModelWillAddRemoveEvent| {
            let (Some(this), Some(notebook)) = (weak.upgrade(), notebook_ref.upgrade()) else { return };
            let uri = notebook.uri();
            let last_failed_cell = this.last_failed_cells.borrow().get(&uri).map(|info| info.cell_handle);
            let Some(last_failed_cell) = last_failed_cell else { return };

            let last_failed_cell_pos = notebook.cells().iter().position(|cell| cell.handle() == last_failed_cell);
            for (start, delete_count, added_cells) in &e.raw_event.changes {
                if *delete_count > 0 && last_failed_cell_pos.is_some_and(|pos| pos >= *start && pos < start + delete_count) {
                    this.set_last_failed_cell_visibility(&uri, false);
                }

                if added_cells.iter().any(|cell| cell.handle() == last_failed_cell) {
                    this.set_last_failed_cell_visibility(&uri, true);
                }
            }
        })
    }
}

impl INotebookExecutionStateService for NotebookExecutionStateService {
    fn on_did_change_execution(&self) -> Event<ExecutionStateChange> {
        self.on_did_change_execution.event()
    }

    fn on_did_change_last_run_fail_state(&self) -> Event<INotebookFailStateChangedEvent> {
        self.on_did_change_last_run_fail_state.event()
    }

    fn get_last_failed_cell_for_notebook(&self, notebook: &URI) -> Option<i64> {
        let failed_cell = self.last_failed_cells.borrow().get(notebook).cloned();
        failed_cell.filter(|failed_cell| failed_cell.visible).map(|failed_cell| failed_cell.cell_handle)
    }

    fn get_last_completed_cell_for_notebook(&self, notebook: &URI) -> Option<i64> {
        self.last_completed_cell_handles.borrow().get(notebook).copied()
    }

    fn force_cancel_notebook_executions(&self, notebook_uri: &URI) {
        if let Some(notebook_cell_executions) = self.executions_of(notebook_uri) {
            let exes: Vec<Rc<CellExecution>> = notebook_cell_executions.borrow().values().cloned().collect();
            for exe in exes {
                self.on_cell_execution_did_complete(notebook_uri, exe.cell_handle, &exe, None);
            }
        }
        if self.notebook_executions.borrow().has(notebook_uri) {
            self.on_execution_did_complete(notebook_uri);
        }
    }

    fn get_cell_execution(&self, cell_uri: &URI) -> Option<Rc<dyn INotebookCellExecution>> {
        let Some(parsed) = CellUri::parse(cell_uri) else {
            errors::throw(Error::new(format!("Not a cell URI: {}", cell_uri.to_string(false).to_string_lossy())));
        };

        let exe_map = self.executions_of(&parsed.notebook)?;
        let exe = exe_map.borrow().get(&parsed.handle).cloned()?;
        Some(exe)
    }

    fn get_execution(&self, notebook: &URI) -> Option<Rc<dyn INotebookExecution>> {
        let info = self.notebook_executions.borrow().get(notebook).map(|(exe, _)| exe.clone())?;
        Some(info)
    }

    fn get_cell_executions_for_notebook(&self, notebook: &URI) -> Vec<Rc<dyn INotebookCellExecution>> {
        match self.executions_of(notebook) {
            Some(exe_map) => exe_map.borrow().values().map(|exe| exe.clone() as Rc<dyn INotebookCellExecution>).collect(),
            None => Vec::new(),
        }
    }

    fn get_cell_executions_by_handle_for_notebook(&self, notebook: &URI) -> Option<HashMap<i64, Rc<dyn INotebookCellExecution>>> {
        let exe_map = self.executions_of(notebook)?;
        let entries = exe_map.borrow().iter().map(|(handle, exe)| (*handle, exe.clone() as Rc<dyn INotebookCellExecution>)).collect();
        Some(entries)
    }

    fn create_cell_execution(&self, notebook_uri: &URI, cell_handle: i64) -> Rc<dyn INotebookCellExecution> {
        let Some(notebook) = self.notebook_service.get_notebook_text_model(notebook_uri) else {
            errors::throw(Error::new(format!("Notebook not found: {}", notebook_uri.to_string(false).to_string_lossy())));
        };

        let notebook_execution_map = match self.executions_of(notebook_uri) {
            Some(map) => map,
            None => {
                let listeners = self.create_listeners(notebook_uri);
                self.notebook_listeners.borrow_mut().set(notebook_uri.clone(), listeners);

                let map: CellExecutionMap = Rc::new(RefCell::new(OrderedMap::new()));
                self.executions.borrow_mut().set(notebook_uri.clone(), map.clone());
                map
            }
        };

        let existing = notebook_execution_map.borrow().get(&cell_handle).cloned();
        let exe = match existing {
            Some(exe) => exe,
            None => {
                let exe = self.create_notebook_cell_execution(&notebook, cell_handle);
                notebook_execution_map.borrow_mut().insert(cell_handle, exe.clone());
                exe.initialize();
                self.on_did_change_execution.fire(ExecutionStateChange::Cell(Rc::new(NotebookCellExecutionEvent { notebook: notebook_uri.clone(), cell_handle, changed: Some(exe.clone()) })));
                exe
            }
        };

        exe
    }

    fn create_execution(&self, notebook_uri: &URI) -> Rc<dyn INotebookExecution> {
        let Some(notebook) = self.notebook_service.get_notebook_text_model(notebook_uri) else {
            errors::throw(Error::new(format!("Notebook not found: {}", notebook_uri.to_string(false).to_string_lossy())));
        };

        if !self.notebook_listeners.borrow().has(notebook_uri) {
            let listeners = self.create_listeners(notebook_uri);
            self.notebook_listeners.borrow_mut().set(notebook_uri.clone(), listeners);
        }

        let info = self.notebook_executions.borrow().get(notebook_uri).cloned();
        let info = match info {
            Some(info) => info,
            None => {
                let info = self.create_notebook_execution(&notebook);
                self.notebook_executions.borrow_mut().set(notebook_uri.clone(), info.clone());
                self.on_did_change_execution.fire(ExecutionStateChange::Notebook(Rc::new(NotebookExecutionEvent { notebook: notebook_uri.clone(), changed: Some(info.0.clone()) })));
                info
            }
        };

        info.0
    }
}

impl IDisposable for NotebookExecutionStateService {
    fn dispose(&self) {
        self.base.dispose();
        let executions: Vec<CellExecutionMap> = self.executions.borrow().values().cloned().collect();
        for execution_map in executions {
            let exes: Vec<Rc<CellExecution>> = execution_map.borrow().values().cloned().collect();
            for execution in exes {
                execution.dispose();
            }
            *execution_map.borrow_mut() = OrderedMap::new();
        }
        self.executions.borrow_mut().clear();
        let notebook_executions: Vec<DisposableRef> = self.notebook_executions.borrow().values().map(|(exe, disposable)| {
            exe.dispose();
            disposable.clone()
        }).collect();
        for disposable in notebook_executions {
            disposable.dispose();
        }
        self.notebook_executions.borrow_mut().clear();

        let cell_listeners: Vec<DisposableRef> = self.cell_listeners.borrow().values().cloned().collect();
        for disposable in cell_listeners {
            disposable.dispose();
        }
        let notebook_listeners: Vec<Rc<NotebookExecutionListeners>> = self.notebook_listeners.borrow().values().cloned().collect();
        for disposable in notebook_listeners {
            disposable.dispose();
        }
        let last_failed_cells: Vec<DisposableRef> = self.last_failed_cells.borrow().values().map(|elem| elem.disposable.clone()).collect();
        for disposable in last_failed_cells {
            disposable.dispose();
        }
    }

    fn constructor_name(&self) -> &'static str {
        "NotebookExecutionStateService"
    }
}

/// `NotebookCellExecutionEvent`.
struct NotebookCellExecutionEvent {
    notebook: URI,
    cell_handle: i64,
    changed: Option<Rc<CellExecution>>,
}

impl ICellExecutionStateChangedEvent for NotebookCellExecutionEvent {
    fn notebook(&self) -> URI {
        self.notebook.clone()
    }

    fn cell_handle(&self) -> i64 {
        self.cell_handle
    }

    fn changed(&self) -> Option<Rc<dyn INotebookCellExecution>> {
        self.changed.clone().map(|exe| exe as Rc<dyn INotebookCellExecution>)
    }

    fn affects_cell(&self, cell: &URI) -> bool {
        CellUri::parse(cell).is_some_and(|parsed| is_equal(&self.notebook, &parsed.notebook) && self.cell_handle == parsed.handle)
    }

    fn affects_notebook(&self, notebook: &URI) -> bool {
        is_equal(&self.notebook, notebook)
    }
}

/// `NotebookExecutionEvent`.
struct NotebookExecutionEvent {
    notebook: URI,
    changed: Option<Rc<NotebookExecution>>,
}

impl IExecutionStateChangedEvent for NotebookExecutionEvent {
    fn notebook(&self) -> URI {
        self.notebook.clone()
    }

    fn changed(&self) -> Option<Rc<dyn INotebookExecution>> {
        self.changed.clone().map(|exe| exe as Rc<dyn INotebookExecution>)
    }

    fn affects_notebook(&self, notebook: &URI) -> bool {
        is_equal(&self.notebook, notebook)
    }
}

/// `NotebookExecutionListeners`.
pub struct NotebookExecutionListeners {
    base: Rc<Disposable>,
    notebook_model: Rc<NotebookTextModel>,
    notebook_kernel_service: Rc<dyn INotebookKernelService>,
    notebook_execution_service: Rc<dyn INotebookExecutionService>,
    notebook_execution_state_service: Weak<NotebookExecutionStateService>,
    log_service: LogServiceRef,
}

impl NotebookExecutionListeners {
    fn new(
        notebook: &URI,
        notebook_service: Rc<dyn INotebookService>,
        notebook_kernel_service: Rc<dyn INotebookKernelService>,
        notebook_execution_service: Rc<dyn INotebookExecutionService>,
        notebook_execution_state_service: Weak<NotebookExecutionStateService>,
        log_service: LogServiceRef,
    ) -> Rc<Self> {
        log_service.debug(&format!("NotebookExecution#ctor {}", notebook.to_string(false).to_string_lossy()));

        let Some(notebook_model) = notebook_service.get_notebook_text_model(notebook) else {
            errors::throw(Error::new(format!("Notebook not found: {}", notebook.to_string(false).to_string_lossy())));
        };

        let this = Rc::new(Self { base: Disposable::new(), notebook_model: notebook_model.clone(), notebook_kernel_service, notebook_execution_service, notebook_execution_state_service, log_service });
        let weak = Rc::downgrade(&this);
        this.base.register(notebook_model.on_will_add_remove_cells().listen(move |e| {
            if let Some(this) = weak.upgrade() {
                this.on_will_add_remove_cells(e);
            }
        }));
        let weak = Rc::downgrade(&this);
        this.base.register(notebook_model.on_will_dispose().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.on_will_dispose_document();
            }
        }));
        this
    }

    fn cancel_all(&self) {
        self.log_service.debug("NotebookExecutionListeners#cancelAll");
        let Some(state_service) = self.notebook_execution_state_service.upgrade() else { return };
        let exes = state_service.get_cell_executions_for_notebook(&self.notebook_model.uri());
        self.notebook_execution_service.cancel_notebook_cell_handles(&*self.notebook_model, exes.iter().map(|exe| exe.cell_handle()).collect());
    }

    fn on_will_dispose_document(&self) {
        self.log_service.debug("NotebookExecution#onWillDisposeDocument");
        self.cancel_all();
    }

    fn on_will_add_remove_cells(&self, e: &NotebookTextModelWillAddRemoveEvent) {
        let Some(state_service) = self.notebook_execution_state_service.upgrade() else { return };
        let notebook_exes = state_service.get_cell_executions_by_handle_for_notebook(&self.notebook_model.uri());

        let mut executing_deleted_handles: Vec<i64> = Vec::new();
        let mut pending_deleted_handles: Vec<i64> = Vec::new();
        if let Some(notebook_exes) = &notebook_exes {
            for (start, delete_count, _) in &e.raw_event.changes {
                if *delete_count > 0 {
                    let cells = self.notebook_model.cells();
                    let end = (start + delete_count).min(cells.len());
                    let deleted_handles: Vec<i64> = cells[(*start).min(end)..end].iter().map(|cell| cell.handle()).collect();
                    for handle in deleted_handles {
                        match notebook_exes.get(&handle) {
                            Some(exe) if exe.state() == NotebookCellExecutionState::Executing => {
                                if !executing_deleted_handles.contains(&handle) {
                                    executing_deleted_handles.push(handle);
                                }
                            }
                            Some(_) => {
                                if !pending_deleted_handles.contains(&handle) {
                                    pending_deleted_handles.push(handle);
                                }
                            }
                            None => {}
                        }
                    }
                }
            }
        }

        if !executing_deleted_handles.is_empty() || !pending_deleted_handles.is_empty() {
            let kernel = self.notebook_kernel_service.get_selected_or_suggested_kernel(&*self.notebook_model);
            if let Some(kernel) = kernel {
                let implements_interrupt = kernel.implements_interrupt().unwrap_or(false);
                let handles_to_cancel: Vec<i64> =
                    if implements_interrupt { executing_deleted_handles } else { executing_deleted_handles.into_iter().chain(pending_deleted_handles).collect() };
                self.log_service.debug(&format!("NotebookExecution#onWillAddRemoveCells, {}", json_handles(&handles_to_cancel)));
                if !handles_to_cancel.is_empty() {
                    kernel.cancel_notebook_cell_execution(&self.notebook_model.uri(), handles_to_cancel);
                }
            }
        }
    }
}

impl IDisposable for NotebookExecutionListeners {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "NotebookExecutionListeners"
    }
}

/// `CellExecutionUpdateType[editType]`.
fn update_type_name(update: &ICellExecuteUpdate) -> &'static str {
    match update {
        ICellExecuteUpdate::Output(_) => "Output",
        ICellExecuteUpdate::OutputItems(_) => "OutputItems",
        ICellExecuteUpdate::ExecutionState(_) => "ExecutionState",
    }
}

/// `updateToEdit(update, cellHandle)`.
fn update_to_edit(update: &ICellExecuteUpdate, cell_handle: i64) -> ICellEditOperation {
    match update {
        ICellExecuteUpdate::Output(update) => ICellEditOperation::OutputByHandle(ICellOutputEditByHandle { handle: update.cell_handle, append: update.append, outputs: update.outputs.clone() }),
        ICellExecuteUpdate::OutputItems(update) => {
            ICellEditOperation::OutputItems(ICellOutputItemEdit { items: update.items.clone(), append: update.append, output_id: update.output_id.clone() })
        }
        ICellExecuteUpdate::ExecutionState(update) => {
            let mut new_internal_metadata = NullablePartialNotebookCellInternalMetadata::default();
            if let Some(execution_order) = update.execution_order {
                new_internal_metadata.execution_order = Some(Some(execution_order));
            }
            if let Some(run_start_time) = update.run_start_time {
                new_internal_metadata.run_start_time = Some(Some(run_start_time));
            }
            ICellEditOperation::PartialInternalMetadataByHandle(ICellPartialInternalMetadataEditByHandle { handle: cell_handle, internal_metadata: new_internal_metadata })
        }
    }
}

/// `CellExecution`.
pub struct CellExecution {
    base: Rc<Disposable>,
    on_did_update: Rc<Emitter<()>>,
    on_did_complete: Rc<Emitter<Option<bool>>>,
    state: Cell<NotebookCellExecutionState>,
    did_pause: Cell<bool>,
    is_paused: Cell<bool>,
    cell_handle: i64,
    notebook_model: Rc<NotebookTextModel>,
    log_service: LogServiceRef,
}

impl CellExecution {
    fn new(cell_handle: i64, notebook_model: Rc<NotebookTextModel>, log_service: LogServiceRef) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_update = Emitter::<()>::new();
        let on_did_complete = Emitter::<Option<bool>>::new();
        base.register(on_did_update.clone());
        base.register(on_did_complete.clone());
        let this = Rc::new(Self {
            base,
            on_did_update,
            on_did_complete,
            state: Cell::new(NotebookCellExecutionState::Unconfirmed),
            did_pause: Cell::new(false),
            is_paused: Cell::new(false),
            cell_handle,
            notebook_model,
            log_service,
        });
        this.log_service.debug(&format!("CellExecution#ctor {}", this.get_cell_log()));
        this
    }

    fn initialize(&self) {
        let start_execute_edit = ICellEditOperation::PartialInternalMetadataByHandle(ICellPartialInternalMetadataEditByHandle {
            handle: self.cell_handle,
            internal_metadata: NullablePartialNotebookCellInternalMetadata {
                execution_id: Some(Some(generate_uuid())),
                run_start_time: Some(None),
                run_end_time: Some(None),
                last_run_success: Some(None),
                execution_order: Some(None),
                render_duration: Some(None),
                ..Default::default()
            },
        });
        self.apply_execution_edits(&[start_execute_edit]);
    }

    fn get_cell_log(&self) -> String {
        format!("{}, {}", self.notebook_model.uri().to_string(false).to_string_lossy(), self.cell_handle)
    }

    fn log_updates(&self, updates: &[ICellExecuteUpdate]) {
        let update_types = updates.iter().map(update_type_name).collect::<Vec<_>>().join(", ");
        self.log_service.debug(&format!("CellExecution#updateExecution {}, [{}]", self.get_cell_log(), update_types));
    }

    fn apply_execution_edits(&self, edits: &[ICellEditOperation]) {
        self.notebook_model.apply_edits(edits, true, None, &|| None, None, false);
    }

    fn find_cell(&self) -> Option<Rc<crate::contrib::notebook::common::model::notebook_cell_text_model::NotebookCellTextModel>> {
        self.notebook_model.cells().into_iter().find(|cell| cell.handle() == self.cell_handle)
    }
}

impl INotebookCellExecution for CellExecution {
    fn notebook(&self) -> URI {
        self.notebook_model.uri()
    }

    fn cell_handle(&self) -> i64 {
        self.cell_handle
    }

    fn state(&self) -> NotebookCellExecutionState {
        self.state.get()
    }

    fn did_pause(&self) -> bool {
        self.did_pause.get()
    }

    fn is_paused(&self) -> bool {
        self.is_paused.get()
    }

    fn confirm(&self) {
        self.log_service.debug(&format!("CellExecution#confirm {}", self.get_cell_log()));
        self.state.set(NotebookCellExecutionState::Pending);
        self.on_did_update.fire(());
    }

    fn update(&self, updates: Vec<ICellExecuteUpdate>) {
        self.log_updates(&updates);
        let execution_state_updates = || updates.iter().filter_map(|update| if let ICellExecuteUpdate::ExecutionState(update) = update { Some(update) } else { None });
        let has_execution_state = execution_state_updates().next().is_some();
        if has_execution_state {
            self.state.set(NotebookCellExecutionState::Executing);
        }

        if !self.did_pause.get() && execution_state_updates().any(|update| update.did_pause == Some(true)) {
            self.did_pause.set(true);
        }

        let last_is_paused_update = execution_state_updates().filter(|update| update.is_paused.is_some()).last();
        if let Some(last_is_paused_update) = last_is_paused_update {
            self.is_paused.set(last_is_paused_update.is_paused.unwrap_or(false));
        }

        if self.find_cell().is_none() {
            self.log_service.debug(&format!("CellExecution#update, updating cell not in notebook: {}", self.get_cell_log()));
        } else {
            let edits: Vec<ICellEditOperation> = updates.iter().map(|update| update_to_edit(update, self.cell_handle)).collect();
            self.apply_execution_edits(&edits);
        }

        if has_execution_state {
            self.on_did_update.fire(());
        }
    }

    fn complete(&self, completion_data: ICellExecutionComplete) {
        match self.find_cell() {
            None => {
                self.log_service.debug(&format!("CellExecution#complete, completing cell not in notebook: {}", self.get_cell_log()));
            }
            Some(cell_model) => {
                let edit = ICellEditOperation::PartialInternalMetadataByHandle(ICellPartialInternalMetadataEditByHandle {
                    handle: self.cell_handle,
                    internal_metadata: NullablePartialNotebookCellInternalMetadata {
                        last_run_success: completion_data.last_run_success.map(Some),
                        run_start_time: if self.did_pause.get() { Some(None) } else { cell_model.internal_metadata().run_start_time.map(Some) },
                        run_end_time: if self.did_pause.get() { Some(None) } else { completion_data.run_end_time.map(Some) },
                        error: completion_data.error.clone().map(Some),
                        ..Default::default()
                    },
                });
                self.apply_execution_edits(&[edit]);
            }
        }

        self.on_did_complete.fire(completion_data.last_run_success);
    }
}

impl IDisposable for CellExecution {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "CellExecution"
    }
}

/// `NotebookExecution`.
pub struct NotebookExecution {
    base: Rc<Disposable>,
    on_did_update: Rc<Emitter<()>>,
    on_did_complete: Rc<Emitter<()>>,
    state: Cell<NotebookExecutionState>,
    notebook_model: Rc<NotebookTextModel>,
    log_service: LogServiceRef,
}

impl NotebookExecution {
    fn new(notebook_model: Rc<NotebookTextModel>, log_service: LogServiceRef) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_update = Emitter::<()>::new();
        let on_did_complete = Emitter::<()>::new();
        base.register(on_did_update.clone());
        base.register(on_did_complete.clone());
        log_service.debug("NotebookExecution#ctor");
        Rc::new(Self { base, on_did_update, on_did_complete, state: Cell::new(NotebookExecutionState::Unconfirmed), notebook_model, log_service })
    }

    fn debug(&self, message: &str) {
        self.log_service.debug(&format!("{} {}", message, self.notebook_model.uri().to_string(false).to_string_lossy()));
    }
}

impl INotebookExecution for NotebookExecution {
    fn notebook(&self) -> URI {
        self.notebook_model.uri()
    }

    fn state(&self) -> NotebookExecutionState {
        self.state.get()
    }

    fn confirm(&self) {
        self.debug("Execution#confirm");
        self.state.set(NotebookExecutionState::Pending);
        self.on_did_update.fire(());
    }

    fn begin(&self) {
        self.debug("Execution#begin");
        self.state.set(NotebookExecutionState::Executing);
        self.on_did_update.fire(());
    }

    fn complete(&self) {
        self.debug("Execution#begin");
        self.state.set(NotebookExecutionState::Unconfirmed);
        self.on_did_complete.fire(());
    }
}

impl IDisposable for NotebookExecution {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "NotebookExecution"
    }
}
