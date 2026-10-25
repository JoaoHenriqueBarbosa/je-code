//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellParts/cellContextKeys.ts`: `CellContextKeyPart` e
//! `CellContextKeyManager`, que mantêm as chaves de contexto da célula (`notebookCellType`, `notebookCellFocused`,
//! `notebookCellExecutionState`...) em sincronia com o view model e com o estado de execução.
//!
//! Divergências:
//! - As chaves de `notebookContextKeys.ts` que esta parte usa (ainda sem porte) são declaradas aqui, como funções que devolvem a mesma
//!   chave a cada chamada e a registram na primeira (o modelo de `common/contextkeys.rs`). `NotebookCellExecutionStateContext` é o
//!   enum [`NotebookCellExecutionStateContext`].
//! - `INotebookEditorDelegate` é o contrato estreito [`ICellContextKeyEditor`] (`getActiveCell`, `onDidChangeActiveCell`,
//!   `isReadOnly`); `@IContextKeyService` e `@INotebookExecutionStateService` são argumentos do construtor, e o
//!   `createInstance(CellContextKeyManager, notebookEditor, undefined)` do `CellContextKeyPart` é [`CellContextKeyManager::new`].
//! - `autorun(reader => ...readObservable(element.executionErrorDiagnostic))` lê o observável com o leitor da `autorun`.
//! - `this.cellType.set('markup')` e demais valores de texto são `Dynamic::String`; `cellRunState.reset()` é `IContextKey::reset`.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::observable::{Observable, autorun};
use je_platform::contextkey::contextkey::{ContextKeyMeta, IContextKey, IContextKeyService, RawContextKey};

use crate::contrib::notebook::browser::notebook_browser::CellEditState;
use crate::contrib::notebook::browser::notebook_view_events::CellViewModelStateChangeEvent;
use crate::contrib::notebook::browser::view::cell_part::{CellPart, CellPartState};
use crate::contrib::notebook::browser::view_model::cell_view_model_types::CellFocusMode;
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use crate::contrib::notebook::common::notebook_common::NotebookCellExecutionState;
use crate::contrib::notebook::common::notebook_execution_state_service::{ExecutionStateChange, INotebookExecutionStateService};

macro_rules! raw_keys {
    ($($(#[$doc:meta])* $name:ident: $key:literal, $default:expr, $meta:expr;)*) => {$(
        $(#[$doc])*
        pub fn $name() -> RawContextKey {
            thread_local! { static KEY: RawContextKey = RawContextKey::new($key, $default, $meta); }
            KEY.with(Clone::clone)
        }
    )*};
}

raw_keys! {
    /// `NOTEBOOK_EDITOR_FOCUSED`.
    notebook_editor_focused: "notebookEditorFocused", false, ContextKeyMeta::None;
    /// `NOTEBOOK_CELL_TYPE` (`'code' | 'markup'`).
    notebook_cell_type: "notebookCellType", Dynamic::Undefined, ContextKeyMeta::None;
    /// `NOTEBOOK_CELL_EDITABLE`.
    notebook_cell_editable: "notebookCellEditable", false, ContextKeyMeta::None;
    /// `NOTEBOOK_CELL_FOCUSED`.
    notebook_cell_focused: "notebookCellFocused", false, ContextKeyMeta::None;
    /// `NOTEBOOK_CELL_EDITOR_FOCUSED`.
    notebook_cell_editor_focused: "notebookCellEditorFocused", false, ContextKeyMeta::None;
    /// `NOTEBOOK_CELL_MARKDOWN_EDIT_MODE`.
    notebook_cell_markdown_edit_mode: "notebookCellMarkdownEditMode", false, ContextKeyMeta::None;
    /// `NOTEBOOK_CELL_LINE_NUMBERS` (`'on' | 'off' | 'inherit'`).
    notebook_cell_line_numbers: "notebookCellLineNumbers", "inherit", ContextKeyMeta::None;
    /// `NOTEBOOK_CELL_EXECUTION_STATE`.
    notebook_cell_execution_state: "notebookCellExecutionState", Dynamic::Undefined, ContextKeyMeta::None;
    /// `NOTEBOOK_CELL_EXECUTING` (existe só para simplificar uma expressão de contexto, ver o issue 129625 do VS Code).
    notebook_cell_executing: "notebookCellExecuting", false, ContextKeyMeta::None;
    /// `NOTEBOOK_CELL_HAS_OUTPUTS`.
    notebook_cell_has_outputs: "notebookCellHasOutputs", false, ContextKeyMeta::None;
    /// `NOTEBOOK_CELL_INPUT_COLLAPSED`.
    notebook_cell_input_collapsed: "notebookCellInputIsCollapsed", false, ContextKeyMeta::None;
    /// `NOTEBOOK_CELL_OUTPUT_COLLAPSED`.
    notebook_cell_output_collapsed: "notebookCellOutputIsCollapsed", false, ContextKeyMeta::None;
    /// `NOTEBOOK_CELL_RESOURCE`.
    notebook_cell_resource: "notebookCellResource", "", ContextKeyMeta::None;
    /// `NOTEBOOK_CELL_HAS_ERROR_DIAGNOSTICS`.
    notebook_cell_has_error_diagnostics: "notebookCellHasErrorDiagnostics", false, ContextKeyMeta::None;
}

/// `NotebookCellExecutionStateContext`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotebookCellExecutionStateContext {
    Idle,
    Pending,
    Executing,
    Succeeded,
    Failed,
}

impl NotebookCellExecutionStateContext {
    /// O literal do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Pending => "pending",
            Self::Executing => "executing",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }
}

/// O que as partes de contexto chamam do `INotebookEditorDelegate`.
pub trait ICellContextKeyEditor {
    /// `getActiveCell()`.
    fn get_active_cell(&self) -> Option<CellViewModel>;
    /// `onDidChangeActiveCell`.
    fn on_did_change_active_cell(&self) -> Event<()>;
    /// `isReadOnly`.
    fn is_read_only(&self) -> bool;
}

/// `CellContextKeyPart`.
pub struct CellContextKeyPart {
    state: CellPartState,
    cell_context_key_manager: Rc<CellContextKeyManager>,
}

impl CellContextKeyPart {
    pub fn new(
        notebook_editor: Rc<dyn ICellContextKeyEditor>,
        context_key_service: Rc<dyn IContextKeyService>,
        notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
    ) -> Rc<Self> {
        let state = CellPartState::new();
        let cell_context_key_manager = CellContextKeyManager::new(notebook_editor, None, context_key_service, notebook_execution_state_service);
        state.register(cell_context_key_manager.clone());
        Rc::new(Self { state, cell_context_key_manager })
    }
}

impl CellPart for CellContextKeyPart {
    fn state(&self) -> &CellPartState {
        &self.state
    }

    fn did_render_cell(&self, element: &CellViewModel) {
        self.cell_context_key_manager.update_for_element(Some(element.clone()));
    }
}

impl_cell_content_part!(CellContextKeyPart, "CellContextKeyPart");

/// As chaves que o `CellContextKeyManager` liga ao serviço de chaves de contexto.
struct CellContextKeys {
    cell_type: Rc<dyn IContextKey>,
    cell_editable: Rc<dyn IContextKey>,
    cell_focused: Rc<dyn IContextKey>,
    cell_editor_focused: Rc<dyn IContextKey>,
    cell_run_state: Rc<dyn IContextKey>,
    cell_executing: Rc<dyn IContextKey>,
    cell_has_outputs: Rc<dyn IContextKey>,
    cell_content_collapsed: Rc<dyn IContextKey>,
    cell_output_collapsed: Rc<dyn IContextKey>,
    cell_line_numbers: Rc<dyn IContextKey>,
    cell_resource: Rc<dyn IContextKey>,
    cell_has_error_diagnostics: Rc<dyn IContextKey>,
    markdown_edit_mode: Rc<dyn IContextKey>,
}

impl CellContextKeys {
    fn bind(context_key_service: &dyn IContextKeyService) -> Self {
        Self {
            cell_type: notebook_cell_type().bind_to(context_key_service),
            cell_editable: notebook_cell_editable().bind_to(context_key_service),
            cell_focused: notebook_cell_focused().bind_to(context_key_service),
            cell_editor_focused: notebook_cell_editor_focused().bind_to(context_key_service),
            markdown_edit_mode: notebook_cell_markdown_edit_mode().bind_to(context_key_service),
            cell_run_state: notebook_cell_execution_state().bind_to(context_key_service),
            cell_executing: notebook_cell_executing().bind_to(context_key_service),
            cell_has_outputs: notebook_cell_has_outputs().bind_to(context_key_service),
            cell_content_collapsed: notebook_cell_input_collapsed().bind_to(context_key_service),
            cell_output_collapsed: notebook_cell_output_collapsed().bind_to(context_key_service),
            cell_line_numbers: notebook_cell_line_numbers().bind_to(context_key_service),
            cell_resource: notebook_cell_resource().bind_to(context_key_service),
            cell_has_error_diagnostics: notebook_cell_has_error_diagnostics().bind_to(context_key_service),
        }
    }
}

/// `CellContextKeyManager`.
pub struct CellContextKeyManager {
    base: Rc<Disposable>,
    notebook_editor: Rc<dyn ICellContextKeyEditor>,
    element: RefCell<Option<CellViewModel>>,
    context_key_service: Rc<dyn IContextKeyService>,
    notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
    keys: CellContextKeys,
    element_disposables: Rc<DisposableStore>,
    me: Weak<CellContextKeyManager>,
}

impl CellContextKeyManager {
    pub fn new(
        notebook_editor: Rc<dyn ICellContextKeyEditor>,
        element: Option<CellViewModel>,
        context_key_service: Rc<dyn IContextKeyService>,
        notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
    ) -> Rc<Self> {
        let created: RefCell<Option<Rc<Self>>> = RefCell::new(None);
        context_key_service.buffer_change_events(&|| {
            let base = Disposable::new();
            let element_disposables = DisposableStore::new();
            base.register(element_disposables.clone());
            let this = Rc::new_cyclic(|me: &Weak<Self>| Self {
                base,
                notebook_editor: notebook_editor.clone(),
                element: RefCell::new(None),
                context_key_service: context_key_service.clone(),
                notebook_execution_state_service: notebook_execution_state_service.clone(),
                keys: CellContextKeys::bind(&*context_key_service),
                element_disposables,
                me: me.clone(),
            });

            if let Some(element) = element.clone() {
                this.update_for_element(Some(element));
            }

            *created.borrow_mut() = Some(this);
        });
        let this = created.into_inner().expect("o callback de bufferChangeEvents roda uma vez");

        let weak = Rc::downgrade(&this);
        this.base.register(this.notebook_execution_state_service.on_did_change_execution().listen(move |e| {
            let Some(this) = weak.upgrade() else { return };
            if let ExecutionStateChange::Cell(change) = e {
                let affected = this.element.borrow().as_ref().is_some_and(|element| change.affects_cell(&element.uri()));
                if affected {
                    this.update_for_execution_state();
                }
            }
        }));

        this
    }

    pub fn update_for_element(&self, element: Option<CellViewModel>) {
        self.element_disposables.clear();
        *self.element.borrow_mut() = element.clone();

        let Some(element) = element else {
            return;
        };

        {
            let me = self.me.clone();
            self.element_disposables.add(element.base().on_did_change_state().listen(move |e| {
                if let Some(this) = me.upgrade() {
                    this.on_did_change_state(e);
                }
            }));
        }

        if let CellViewModel::Code(cell) = &element {
            {
                let me = self.me.clone();
                self.element_disposables.add(cell.on_did_change_outputs().listen(move |_| {
                    if let Some(this) = me.upgrade() {
                        this.update_for_outputs();
                    }
                }));
            }

            let cell_has_error_diagnostics = self.keys.cell_has_error_diagnostics.clone();
            let execution_error_diagnostic = cell.execution_error_diagnostic();
            self.element_disposables.add(autorun(move |reader| {
                cell_has_error_diagnostics.set(Dynamic::Bool(execution_error_diagnostic.read(reader).is_some()));
            }));
        }

        {
            let me = self.me.clone();
            self.element_disposables.add(self.notebook_editor.on_did_change_active_cell().listen(move |_| {
                if let Some(this) = me.upgrade() {
                    this.update_for_focus_state();
                }
            }));
        }

        match &element {
            CellViewModel::Markup(_) => self.keys.cell_type.set(Dynamic::string("markup")),
            CellViewModel::Code(_) => self.keys.cell_type.set(Dynamic::string("code")),
        }

        self.context_key_service.buffer_change_events(&|| {
            self.update_for_focus_state();
            self.update_for_execution_state();
            self.update_for_edit_state();
            self.update_for_collapse_state();
            self.update_for_outputs();

            self.keys.cell_line_numbers.set(Dynamic::string(element.base().line_numbers().as_str()));
            self.keys.cell_resource.set(Dynamic::string(&element.uri().to_string(false).to_string_lossy()));
        });
    }

    fn on_did_change_state(&self, e: &CellViewModelStateChangeEvent) {
        let Some(element) = self.element.borrow().clone() else {
            return;
        };

        self.context_key_service.buffer_change_events(&|| {
            if e.internal_metadata_changed {
                self.update_for_execution_state();
            }

            if e.edit_state_changed {
                self.update_for_edit_state();
            }

            if e.focus_mode_changed {
                self.update_for_focus_state();
            }

            if e.cell_line_number_changed {
                self.keys.cell_line_numbers.set(Dynamic::string(element.base().line_numbers().as_str()));
            }

            if e.input_collapsed_changed || e.output_collapsed_changed {
                self.update_for_collapse_state();
            }
        });
    }

    fn update_for_focus_state(&self) {
        let Some(element) = self.element.borrow().clone() else {
            return;
        };

        let active_cell = self.notebook_editor.get_active_cell();
        let is_active = active_cell.as_ref() == Some(&element);
        self.keys.cell_focused.set(Dynamic::Bool(is_active));

        if is_active {
            self.keys.cell_editor_focused.set(Dynamic::Bool(element.base().focus_mode() == CellFocusMode::Editor));
        } else {
            self.keys.cell_editor_focused.set(Dynamic::Bool(false));
        }
    }

    fn update_for_execution_state(&self) {
        let Some(element) = self.element.borrow().clone() else {
            return;
        };

        let internal_metadata = element.base().internal_metadata();
        self.keys.cell_editable.set(Dynamic::Bool(!self.notebook_editor.is_read_only()));

        let exe_state = self.notebook_execution_state_service.get_cell_execution(&element.uri()).map(|execution| execution.state());
        if matches!(element, CellViewModel::Markup(_)) {
            self.keys.cell_run_state.reset();
            self.keys.cell_executing.reset();
        } else if exe_state == Some(NotebookCellExecutionState::Executing) {
            self.keys.cell_run_state.set(Dynamic::string(NotebookCellExecutionStateContext::Executing.as_str()));
            self.keys.cell_executing.set(Dynamic::Bool(true));
        } else if matches!(exe_state, Some(NotebookCellExecutionState::Pending | NotebookCellExecutionState::Unconfirmed)) {
            self.keys.cell_run_state.set(Dynamic::string(NotebookCellExecutionStateContext::Pending.as_str()));
            self.keys.cell_executing.set(Dynamic::Bool(true));
        } else if internal_metadata.last_run_success == Some(true) {
            self.keys.cell_run_state.set(Dynamic::string(NotebookCellExecutionStateContext::Succeeded.as_str()));
            self.keys.cell_executing.set(Dynamic::Bool(false));
        } else if internal_metadata.last_run_success == Some(false) {
            self.keys.cell_run_state.set(Dynamic::string(NotebookCellExecutionStateContext::Failed.as_str()));
            self.keys.cell_executing.set(Dynamic::Bool(false));
        } else {
            self.keys.cell_run_state.set(Dynamic::string(NotebookCellExecutionStateContext::Idle.as_str()));
            self.keys.cell_executing.set(Dynamic::Bool(false));
        }
    }

    fn update_for_edit_state(&self) {
        let Some(element) = self.element.borrow().clone() else {
            return;
        };

        match &element {
            CellViewModel::Markup(cell) => self.keys.markdown_edit_mode.set(Dynamic::Bool(cell.get_edit_state() == CellEditState::Editing)),
            CellViewModel::Code(_) => self.keys.markdown_edit_mode.set(Dynamic::Bool(false)),
        }
    }

    fn update_for_collapse_state(&self) {
        let Some(element) = self.element.borrow().clone() else {
            return;
        };

        self.keys.cell_content_collapsed.set(Dynamic::Bool(element.base().is_input_collapsed()));
        self.keys.cell_output_collapsed.set(Dynamic::Bool(element.base().is_output_collapsed()));
    }

    fn update_for_outputs(&self) {
        let element = self.element.borrow().clone();
        match element {
            Some(CellViewModel::Code(cell)) => self.keys.cell_has_outputs.set(Dynamic::Bool(!cell.outputs_view_models().is_empty())),
            _ => self.keys.cell_has_outputs.set(Dynamic::Bool(false)),
        }
    }
}

impl IDisposable for CellContextKeyManager {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "CellContextKeyManager"
    }
}
