//! Porte de `src/vs/workbench/contrib/notebook/browser/viewModel/notebookOutlineDataSource.ts`: a fonte de dados do outline de um
//! notebook, que monta a árvore de [`OutlineEntry`] das células, acompanha os marcadores (erros e avisos) e a célula ativa.
//!
//! Divergências:
//! - `_editor: INotebookEditor` é o `Rc<dyn IActiveNotebookEditor>` (o contrato lógico do editor ativo): `hasModel()` é sempre
//!   verdadeiro, e `isDisposed` é falso (o widget lógico não é descartado antes da fonte de dados).
//! - Os serviços injetados (`IMarkerService`, `IConfigurationService`, `INotebookOutlineEntryFactory`) são os `Rc<dyn ...>` do
//!   construtor; o construtor devolve `Rc<Self>` porque os ouvintes de eventos precisam de uma referência fraca.
//! - `MutableDisposable` do ouvinte de marcadores é `RefCell<Option<DisposableRef>>`; `_disposables.clear()` descarta os dois
//!   ouvintes (o de marcadores e o de configuração).
//! - `console.error(...)` de [`NotebookCellOutlineDataSource::compute_full_symbols`] é `eprintln!` (o original só o registra).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::Promise;
use je_base::resources::ext_uri;
use je_base::uri::URI;
use je_platform::configuration::service::IConfigurationService;
use je_platform::markers::markers::IMarkerService;

use super::notebook_outline_entry_factory::INotebookOutlineEntryFactory;
use super::outline_entry::OutlineEntry;
use crate::contrib::notebook::browser::controller::IActiveNotebookEditor;
use crate::contrib::notebook::common::notebook_common::CellKind;
use crate::services::outline::outline::{OutlineChangeEvent, OutlineConfigKeys};

/// `INotebookCellOutlineDataSource`.
pub trait INotebookCellOutlineDataSource {
    fn active_element(&self) -> Option<Rc<OutlineEntry>>;
    fn entries(&self) -> Vec<Rc<OutlineEntry>>;
}

/// `NotebookCellOutlineDataSource`.
pub struct NotebookCellOutlineDataSource {
    this: Weak<NotebookCellOutlineDataSource>,
    editor: Rc<dyn IActiveNotebookEditor>,
    marker_service: Rc<dyn IMarkerService>,
    configuration_service: Rc<dyn IConfigurationService>,
    outline_entry_factory: Rc<dyn INotebookOutlineEntryFactory>,
    disposables: RefCell<Vec<DisposableRef>>,
    marker_service_listener: RefCell<Option<DisposableRef>>,
    on_did_change: Rc<Emitter<OutlineChangeEvent>>,
    uri: RefCell<Option<URI>>,
    entries: RefCell<Vec<Rc<OutlineEntry>>>,
    active_entry: RefCell<Option<Rc<OutlineEntry>>>,
    disposed: Cell<bool>,
}

impl INotebookCellOutlineDataSource for NotebookCellOutlineDataSource {
    fn active_element(&self) -> Option<Rc<OutlineEntry>> {
        self.active_entry.borrow().clone()
    }

    fn entries(&self) -> Vec<Rc<OutlineEntry>> {
        self.entries.borrow().clone()
    }
}

fn same_entry(a: &Option<Rc<OutlineEntry>>, b: &Option<Rc<OutlineEntry>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}

impl NotebookCellOutlineDataSource {
    pub fn new(
        editor: Rc<dyn IActiveNotebookEditor>,
        marker_service: Rc<dyn IMarkerService>,
        configuration_service: Rc<dyn IConfigurationService>,
        outline_entry_factory: Rc<dyn INotebookOutlineEntryFactory>,
    ) -> Rc<Self> {
        let data_source = Rc::new_cyclic(|this| NotebookCellOutlineDataSource {
            this: this.clone(),
            editor,
            marker_service,
            configuration_service,
            outline_entry_factory,
            disposables: RefCell::new(Vec::new()),
            marker_service_listener: RefCell::new(None),
            on_did_change: Emitter::new(),
            uri: RefCell::new(None),
            entries: RefCell::new(Vec::new()),
            active_entry: RefCell::new(None),
            disposed: Cell::new(false),
        });
        data_source.recompute_state();
        data_source
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<OutlineChangeEvent> {
        self.on_did_change.event()
    }

    /// `get isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.entries.borrow().is_empty()
    }

    /// `get uri()`.
    pub fn uri(&self) -> Option<URI> {
        self.uri.borrow().clone()
    }

    /// `computeFullSymbols(cancelToken)`.
    pub fn compute_full_symbols(&self, cancel_token: &CancellationToken) -> Promise<()> {
        let notebook_cells: Vec<_> = self.editor.get_view_model().view_cells().into_iter().filter(|cell| cell.cell_kind() == CellKind::Code).collect();

        // limit the number of cells so that we don't resolve an excessive amount of text models
        // gather all symbols asynchronously
        let promises: Vec<Promise<()>> = notebook_cells.iter().take(50).map(|cell| self.outline_entry_factory.cache_symbols(cell, cancel_token)).collect();
        let this = self.this.clone();
        Promise::all_settled(promises).then(move |results| {
            for result in results {
                if let Err(err) = result {
                    eprintln!("Failed to compute notebook outline symbols: {err:?}");
                }
            }
            if let Some(this) = this.upgrade() {
                this.recompute_state();
            }
        })
    }

    fn clear_disposables(&self) {
        for disposable in self.disposables.borrow_mut().drain(..) {
            disposable.dispose();
        }
        if let Some(listener) = self.marker_service_listener.borrow_mut().take() {
            listener.dispose();
        }
    }

    /// `recomputeState()`.
    pub fn recompute_state(&self) {
        self.clear_disposables();
        *self.active_entry.borrow_mut() = None;
        *self.uri.borrow_mut() = None;

        *self.uri.borrow_mut() = Some(self.editor.text_model().uri());

        if self.editor.get_length() == 0 {
            return;
        }

        let notebook_cells = self.editor.get_view_model().view_cells();

        let mut entries: Vec<Rc<OutlineEntry>> = Vec::new();
        for cell in &notebook_cells {
            let next = self.outline_entry_factory.get_outline_entries(cell, entries.len());
            entries.extend(next);
        }

        // build a tree from the list of entries
        if !entries.is_empty() {
            let mut result: Vec<Rc<OutlineEntry>> = vec![entries[0].clone()];
            let mut parent_stack: Vec<Rc<OutlineEntry>> = vec![entries[0].clone()];

            for entry in entries.iter().skip(1) {
                loop {
                    match parent_stack.last().cloned() {
                        None => {
                            // root node
                            result.push(entry.clone());
                            parent_stack.push(entry.clone());
                            break;
                        }
                        Some(parent_candidate) => {
                            if parent_candidate.level < entry.level {
                                parent_candidate.add_child(entry.clone());
                                parent_stack.push(entry.clone());
                                break;
                            }
                            parent_stack.pop();
                        }
                    }
                }
            }
            *self.entries.borrow_mut() = result;
        }

        // feature: show markers with each cell
        self.update_marker_updater();
        let this = self.this.clone();
        let listener = self.configuration_service.on_did_change_configuration().listen(move |e| {
            if (e.affects_configuration("problems.visibility", None) || e.affects_configuration(OutlineConfigKeys::PROBLEMS_ENABLED, None))
                && let Some(this) = this.upgrade()
            {
                this.update_marker_updater();
                this.on_did_change.fire(OutlineChangeEvent::default());
            }
        });
        self.disposables.borrow_mut().push(listener);

        let changed = self.recompute_active();
        if !changed {
            self.on_did_change.fire(OutlineChangeEvent::default());
        }
    }

    fn do_update_marker(&self, clear: bool) {
        for entry in self.entries.borrow().iter() {
            if clear {
                entry.clear_markers();
            } else {
                entry.update_markers(self.marker_service.as_ref());
            }
        }
    }

    fn update_marker_updater(&self) {
        let problem = self.configuration_service.get_value(Some("problems.visibility"), None);
        if matches!(problem, Dynamic::Undefined) {
            return;
        }

        let config = self.configuration_service.get_value(Some(OutlineConfigKeys::PROBLEMS_ENABLED), None);

        if matches!(problem, Dynamic::Bool(true)) && matches!(config, Dynamic::Bool(true)) {
            let this = self.this.clone();
            let listener = self.marker_service.on_marker_changed().listen(move |uris| {
                let Some(this) = this.upgrade() else {
                    return;
                };
                let cells = this.editor.get_cells_in_range(None);
                if uris.iter().any(|uri| cells.iter().any(|cell| ext_uri().is_equal(Some(&cell.uri()), Some(uri), false))) {
                    this.do_update_marker(false);
                    this.on_did_change.fire(OutlineChangeEvent::default());
                }
            });
            if let Some(previous) = self.marker_service_listener.borrow_mut().replace(listener) {
                previous.dispose();
            }
            self.do_update_marker(false);
        } else {
            if let Some(previous) = self.marker_service_listener.borrow_mut().take() {
                previous.dispose();
            }
            self.do_update_marker(true);
        }
    }

    /// `recomputeActive()`: devolve `changeEventTriggered`.
    pub fn recompute_active(&self) -> bool {
        let mut new_active: Option<Rc<OutlineEntry>> = None;
        if self.editor.get_length() > 0
            && let Some(cell) = self.editor.cell_at(self.editor.get_focus().start)
        {
            for entry in self.entries.borrow().iter() {
                new_active = entry.find(&cell, &mut Vec::new());
                if new_active.is_some() {
                    break;
                }
            }
        }

        if !same_entry(&new_active, &self.active_entry.borrow()) {
            *self.active_entry.borrow_mut() = new_active;
            self.on_did_change.fire(OutlineChangeEvent { affect_only_active_element: true });
            return true;
        }
        false
    }
}

impl IDisposable for NotebookCellOutlineDataSource {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        self.entries.borrow_mut().clear();
        *self.active_entry.borrow_mut() = None;
        self.clear_disposables();
        self.on_did_change.dispose();
    }
}
