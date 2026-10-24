//! Porte de `src/vs/workbench/contrib/notebook/browser/contrib/cellDiagnostics/cellDiagnosticEditorContrib.ts`: a contribuição
//! que, quando uma célula de código falha com um erro que tem posição, publica um marcador de erro na célula e o remove quando a
//! célula roda de novo, muda de conteúdo ou perde as saídas (`CellDiagnostics`).
//!
//! Divergências:
//! - `INotebookEditor` é o [`ICellDiagnosticsEditor`], com o que a contribuição lê dele (`textModel?.uri` e `getCellByHandle`).
//! - `registerNotebookContribution(id, ctor)` e `INotebookEditorContribution` (o registro de contribuições do widget) não têm
//!   porte: a contribuição é criada por quem monta o editor, e o `id` é [`CellDiagnostics::ID`].
//! - `@INotebookExecutionStateService`, `@IMarkerService`, `@IChatAgentService` e `@IConfigurationService` são argumentos do
//!   construtor. O `IChatAgentService` é só o contrato (ver `chat_agents`): sem o chat, nenhum agente é registrado e a
//!   contribuição fica desligada, como no original sem agente de notebook.
//! - `cell instanceof CodeCellViewModel` é o `CellViewModel::Code`; `cell.executionErrorDiagnostic` é o observável do
//!   [`CodeCellViewModel`], e o `autorun` é o do `je_base`.
//! - `Event.accumulate(event, 200)` é o `accumulate` com `Delay::Millis(200)`; o `changes.reverse()` do original reverte o
//!   próprio array, aqui a iteração é de trás para frente (o array não é lido depois).
//! - O `Map<number, IDisposable[]>` é um `HashMap` e o `for (const handle of keys) clear(handle)` itera sobre uma cópia das chaves
//!   (o `clear` remove a chave durante o laço). `clear` só solta o empréstimo do mapa antes de descartar, porque o descarte do
//!   observável reentra em `clear` pelo `autorun`, exatamente como no original (as ações descartadas são de chamada única).
//! - O construtor devolve `Rc<Self>` (os fechos guardam um `Weak`).
//! - A severidade `8` do marcador é o [`MarkerSeverity::ERROR`].

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use je_base::dynamic::Truthy;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::observable::{Observable, SettableExt, autorun};
use je_base::symbols::Delay;
use je_base::uri::URI;
use je_editor::range::Range;
use je_platform::configuration::service::IConfigurationService;
use je_platform::markers::markers::{IMarkerService, MarkerData, MarkerSeverity};

use crate::contrib::chat::common::constants::ChatAgentLocation;
use crate::contrib::chat::common::participants::chat_agents::IChatAgentService;
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use crate::contrib::notebook::common::notebook_common::{CellKind, ICell, NotebookSetting};
use crate::contrib::notebook::common::notebook_execution_state_service::{ExecutionStateChange, INotebookExecutionStateService};

/// O que o `CellDiagnostics` lê do `INotebookEditor`.
pub trait ICellDiagnosticsEditor {
    /// `notebookEditor.textModel?.uri`.
    fn text_model_uri(&self) -> Option<URI>;

    /// `notebookEditor.getCellByHandle(handle)`.
    fn get_cell_by_handle(&self, handle: i64) -> Option<CellViewModel>;
}

/// `CellDiagnostics`.
pub struct CellDiagnostics {
    this: Weak<CellDiagnostics>,
    base: Rc<DisposableStore>,
    notebook_editor: Rc<dyn ICellDiagnosticsEditor>,
    notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
    marker_service: Rc<dyn IMarkerService>,
    chat_agent_service: Rc<dyn IChatAgentService>,
    configuration_service: Rc<dyn IConfigurationService>,
    enabled: Cell<bool>,
    listening: Cell<bool>,
    diagnostics_by_handle: RefCell<HashMap<i64, Vec<DisposableRef>>>,
}

impl CellDiagnostics {
    pub const ID: &'static str = "workbench.notebook.cellDiagnostics";

    pub fn new(
        notebook_editor: Rc<dyn ICellDiagnosticsEditor>,
        notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
        marker_service: Rc<dyn IMarkerService>,
        chat_agent_service: Rc<dyn IChatAgentService>,
        configuration_service: Rc<dyn IConfigurationService>,
    ) -> Rc<Self> {
        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base: DisposableStore::new(),
            notebook_editor,
            notebook_execution_state_service,
            marker_service,
            chat_agent_service: chat_agent_service.clone(),
            configuration_service: configuration_service.clone(),
            enabled: Cell::new(false),
            listening: Cell::new(false),
            diagnostics_by_handle: RefCell::new(HashMap::new()),
        });

        this.update_enabled();

        let weak = this.this.clone();
        this.base.add(chat_agent_service.on_did_change_agents().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_enabled();
            }
        }));
        let weak = this.this.clone();
        this.base.add(configuration_service.on_did_change_configuration().listen(move |e| {
            if e.affects_configuration(NotebookSetting::CELL_FAILURE_DIAGNOSTICS, None) {
                if let Some(this) = weak.upgrade() {
                    this.update_enabled();
                }
            }
        }));
        this
    }

    fn has_notebook_agent(&self) -> bool {
        self.chat_agent_service.get_agents().iter().any(|agent| agent.locations.contains(&ChatAgentLocation::Notebook))
    }

    fn update_enabled(&self) {
        let setting_enabled = self.configuration_service.get_value(Some(NotebookSetting::CELL_FAILURE_DIAGNOSTICS), None).is_truthy();
        if self.enabled.get() && (!setting_enabled || !self.has_notebook_agent()) {
            self.enabled.set(false);
            self.clear_all();
        } else if !self.enabled.get() && setting_enabled && self.has_notebook_agent() {
            self.enabled.set(true);
            if !self.listening.get() {
                self.listening.set(true);
                let weak = self.this.clone();
                self.base.add(self.notebook_execution_state_service.on_did_change_execution().accumulate(Delay::Millis(200), None, None).listen(move |changes| {
                    if let Some(this) = weak.upgrade() {
                        this.handle_change_execution_state(changes);
                    }
                }));
            }
        }
    }

    fn handle_change_execution_state(&self, changes: &[ExecutionStateChange]) {
        if !self.enabled.get() {
            return;
        }

        let mut handled: HashSet<i64> = HashSet::new();
        for e in changes.iter().rev() {
            let notebook_uri = self.notebook_editor.text_model_uri();
            let ExecutionStateChange::Cell(e) = e else { continue };
            let Some(notebook_uri) = notebook_uri else { continue };
            if e.affects_notebook(&notebook_uri) && !handled.contains(&e.cell_handle()) {
                handled.insert(e.cell_handle());
                if e.changed().is_some() {
                    // cell is running
                    self.clear(e.cell_handle());
                } else {
                    self.set_diagnostics(e.cell_handle());
                }
            }
        }
    }

    fn clear_all(&self) {
        let handles: Vec<i64> = self.diagnostics_by_handle.borrow().keys().copied().collect();
        for handle in handles {
            self.clear(handle);
        }
    }

    pub fn clear(&self, cell_handle: i64) {
        let disposables = self.diagnostics_by_handle.borrow().get(&cell_handle).cloned();
        if let Some(disposables) = disposables {
            for disposable in &disposables {
                disposable.dispose();
            }
            self.diagnostics_by_handle.borrow_mut().remove(&cell_handle);
        }
    }

    fn set_diagnostics(&self, cell_handle: i64) {
        if self.diagnostics_by_handle.borrow().contains_key(&cell_handle) {
            // multiple diagnostics per cell not supported for now
            return;
        }

        let Some(cell) = self.notebook_editor.get_cell_by_handle(cell_handle) else { return };
        if cell.cell_kind() != CellKind::Code {
            return;
        }

        let metadata = cell.base().internal_metadata();
        let CellViewModel::Code(code_cell) = &cell else { return };
        let Some(error) = metadata.error.as_ref() else { return };
        let Some(location) = error.location.as_ref() else { return };
        if metadata.last_run_success.is_truthy() {
            return;
        }

        let mut disposables: Vec<DisposableRef> = Vec::new();
        let error_label = if error.name.is_empty() { error.message.clone() } else { format!("{}: {}", error.name, error.message) };
        let marker = Self::create_marker_data(&error_label, location);
        let cell_uri = cell.uri();
        self.marker_service.change_one(Self::ID, &cell_uri, vec![marker], None);
        {
            let marker_service = self.marker_service.clone();
            let cell_uri = cell_uri.clone();
            disposables.push(to_disposable(move || marker_service.change_one(Self::ID, &cell_uri, Vec::new(), None)));
        }

        let diagnostic = code_cell.execution_error_diagnostic();
        diagnostic.set(Some(Rc::new(error.clone())), None);
        {
            let diagnostic = diagnostic.clone();
            disposables.push(to_disposable(move || diagnostic.set(None, None)));
        }
        {
            let diagnostic = diagnostic.clone();
            let weak = self.this.clone();
            disposables.push(autorun(move |reader| {
                if diagnostic.read(reader).is_none() {
                    if let Some(this) = weak.upgrade() {
                        this.clear(cell_handle);
                    }
                }
            }));
        }

        let model = cell.model();
        {
            let weak = self.this.clone();
            let model_for_outputs = model.clone();
            disposables.push(model.on_did_change_outputs().listen(move |_| {
                if model_for_outputs.outputs().is_empty() {
                    if let Some(this) = weak.upgrade() {
                        this.clear(cell_handle);
                    }
                }
            }));
        }
        {
            let weak = self.this.clone();
            disposables.push(model.on_did_change_content().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.clear(cell_handle);
                }
            }));
        }
        self.diagnostics_by_handle.borrow_mut().insert(cell_handle, disposables);
    }

    fn create_marker_data(message: &str, location: &Range) -> MarkerData {
        let mut marker = MarkerData::new(
            MarkerSeverity::ERROR,
            message,
            i64::from(location.start_line_number) + 1,
            i64::from(location.start_column) + 1,
            i64::from(location.end_line_number) + 1,
            i64::from(location.end_column) + 1,
        );
        marker.source = Some("Cell Execution Error".to_string());
        marker
    }
}

impl IDisposable for CellDiagnostics {
    fn dispose(&self) {
        self.base.dispose();
        self.clear_all();
    }
}
