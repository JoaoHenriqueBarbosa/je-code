//! Porte parcial de `src/vs/workbench/contrib/notebook/browser/contrib/outline/notebookOutline.ts`, o resto do arquivo: o
//! [`NotebookCellOutline`] (o estado do outline de um notebook, que liga as fontes de dados aos eventos do editor, da configuração,
//! da execução e dos provedores de símbolos) e o [`NotebookOutlineCreator`] (que cria o outline e, no seletor rápido, carrega os
//! símbolos antes de entregá-lo). Os provedores de visão estão em `notebook_outline.rs`.
//!
//! Divergências:
//! - O `NotebookCellOutline` não implementa o [`IOutline`](crate::services::outline::outline::IOutline), e o `NotebookOutlineCreator` não
//!   se registra no `IOutlineService`: `reveal` (abre o editor com `cellRevealType`/`selection`), `preview` (decorações de célula e de
//!   modelo, `revealRangeInCenterIfOutsideViewportAsync`) e `captureViewState` (`getEditorViewState`/`restoreListViewState`) dependem do
//!   widget do notebook e do `IEditorService`, e não estão portados. `outlineKind` é [`NotebookCellOutline::OUTLINE_KIND`].
//! - `config` guarda só as três fontes de dados (`treeDataSource`, `quickPickDataSource`, `breadcrumbsDataSource`): `delegate`, `renderers`,
//!   `comparator` (`NotebookComparator`, que usa `Intl.Collator` numérico) e `options` (`IWorkbenchDataTreeOptions`) são widgets de árvore.
//!   Como no original, o `config` é montado no fim do construtor com as fontes de dados daquele momento, e um `onDidChangeModel`
//!   posterior troca as fontes privadas, não as do `config`.
//! - `INotebookEditorPane` é o trait [`INotebookEditorPane`], com o que o outline lê do painel: `getControl()` (só quando o editor tem
//!   modelo, o `!notebookEditor?.hasModel()` do original), `onDidChangeModel`, `onDidChangeSelection` e `textModel`.
//! - `@IThemeService` entra como o evento `onDidFileIconThemeChange` (o `IThemeService` do porte não tem tema de ícones de arquivo) e o
//!   `Fn() -> IFileIconTheme` do seletor rápido; `@ILanguageFeaturesService` é o [`LanguageFeaturesService`].
//! - `NotebookCellOutlineDataSource` vem da fábrica por referência contada; as fontes de dados dos provedores recebem a referência como
//!   `Reference<Rc<dyn INotebookCellOutlineDataSource>>` que divide o descarte com a original.
//! - `createInstance(NotebookCellOutline, editor, target)` é [`NotebookCellOutline::new`] com o [`NotebookOutlineServices`].
//! - `console.error(...)` de `activeElement` fora do painel é `eprintln!`.
//! - `Delayer.trigger` devolve uma promessa que rejeita com cancelamento quando o `cancel()` chega antes da hora; o original a ignora e
//!   aqui ela é ignorada do mesmo jeito.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::Delayer;
use je_base::cancellation::CancellationToken;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable, Reference, plain_disposable, to_disposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::symbols::Delay;
use je_base::uri::URI;
use je_editor::language_features_service::LanguageFeaturesService;
use je_platform::configuration::service::IConfigurationService;

use super::notebook_outline::{IFileIconTheme, NotebookBreadcrumbsProvider, NotebookOutlinePaneProvider, NotebookQuickPickProvider, filter_entry, read_bool};
use crate::common::editor::EditorPaneSelectionChangeEvent;
use crate::contrib::notebook::browser::controller::IActiveNotebookEditor;
use crate::contrib::notebook::browser::view_model::notebook_outline_data_source::{INotebookCellOutlineDataSource, NotebookCellOutlineDataSource};
use crate::contrib::notebook::browser::view_model::notebook_outline_data_source_factory::INotebookCellOutlineDataSourceFactory;
use crate::contrib::notebook::browser::view_model::outline_entry::OutlineEntry;
use crate::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use crate::contrib::notebook::common::notebook_common::{NotebookCellsChangeType, NotebookSetting};
use crate::contrib::notebook::common::notebook_execution_state_service::{ExecutionStateChange, INotebookExecutionStateService};
use crate::services::outline::outline::{OutlineChangeEvent, OutlineTarget};

/// `INotebookEditorPane`, só com o que o outline lê (ver as divergências do módulo).
pub trait INotebookEditorPane {
    /// `getControl()`, quando o editor tem modelo.
    fn get_control(&self) -> Option<Rc<dyn IActiveNotebookEditor>>;
    /// `onDidChangeModel`.
    fn on_did_change_model(&self) -> Event<()>;
    /// `onDidChangeSelection`.
    fn on_did_change_selection(&self) -> Event<EditorPaneSelectionChangeEvent>;
    /// `textModel`.
    fn text_model(&self) -> Option<Rc<NotebookTextModel>>;
}

/// Os serviços que o `createInstance` injetava no `NotebookCellOutline`.
#[derive(Clone)]
pub struct NotebookOutlineServices {
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub language_features_service: Rc<LanguageFeaturesService>,
    pub notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
    pub data_source_factory: Rc<dyn INotebookCellOutlineDataSourceFactory>,
    /// `IThemeService.onDidFileIconThemeChange`.
    pub on_did_file_icon_theme_change: Event<()>,
    /// `IThemeService.getFileIconTheme()`.
    pub file_icon_theme: Rc<dyn Fn() -> IFileIconTheme>,
}

/// `config`: as fontes de dados das três visões.
#[derive(Clone)]
pub struct NotebookOutlineConfig {
    pub tree_data_source: Rc<NotebookOutlinePaneProvider>,
    pub quick_pick_data_source: Rc<NotebookQuickPickProvider>,
    pub breadcrumbs_data_source: Rc<NotebookBreadcrumbsProvider>,
}

/// A referência da fonte de dados concreta como a que os provedores recebem (`IReference<NotebookCellOutlineDataSource>`).
fn as_provider_reference(reference: &Reference<Rc<NotebookCellOutlineDataSource>>) -> Reference<Rc<dyn INotebookCellOutlineDataSource>> {
    let original = reference.clone();
    let object: Rc<dyn INotebookCellOutlineDataSource> = reference.object.clone();
    // Só uma fachada de tipo sobre a mesma referência (o original passa o mesmo `IReference` aos três provedores): não é um
    // descartável novo, por isso não é rastreado.
    Reference::new(object, plain_disposable(move || original.dispose()))
}

/// `NotebookCellOutline`.
pub struct NotebookCellOutline {
    this: Weak<NotebookCellOutline>,
    editor: Rc<dyn INotebookEditorPane>,
    target: OutlineTarget,
    services: NotebookOutlineServices,

    disposables: Rc<DisposableStore>,
    model_disposables: Rc<DisposableStore>,
    data_source_disposables: Rc<DisposableStore>,

    on_did_change: Rc<Emitter<OutlineChangeEvent>>,

    delayer_recompute_state: Rc<Delayer<()>>,
    delayer_recompute_active: Rc<Delayer<()>>,
    // this can be long, because it will force a recompute at the end, so ideally we only do this once all nb language features are registered
    delayer_recompute_symbols: Rc<Delayer<()>>,

    config: OnceCell<NotebookOutlineConfig>,
    outline_data_source_reference: RefCell<Option<Reference<Rc<NotebookCellOutlineDataSource>>>>,
    // These three fields will always be set via setDataSources()
    tree_data_source: RefCell<Option<Rc<NotebookOutlinePaneProvider>>>,
    quick_pick_data_source: RefCell<Option<Rc<NotebookQuickPickProvider>>>,
    breadcrumbs_data_source: RefCell<Option<Rc<NotebookBreadcrumbsProvider>>>,

    // view settings
    outline_show_code_cells: Cell<bool>,
    outline_show_code_cell_symbols: Cell<bool>,
    outline_show_markdown_headers_only: Cell<bool>,
}

impl NotebookCellOutline {
    /// `outlineKind`.
    pub const OUTLINE_KIND: &'static str = "notebookCells";

    pub fn new(editor: Rc<dyn INotebookEditorPane>, target: OutlineTarget, services: NotebookOutlineServices) -> Rc<Self> {
        let disposables = DisposableStore::new();
        let model_disposables = DisposableStore::new();
        let data_source_disposables = DisposableStore::new();
        let on_did_change = Emitter::<OutlineChangeEvent>::new();
        let delayer_recompute_state = Rc::new(Delayer::<()>::new(Delay::Millis(300)));
        let delayer_recompute_active = Rc::new(Delayer::<()>::new(Delay::Millis(200)));
        let delayer_recompute_symbols = Rc::new(Delayer::<()>::new(Delay::Millis(2000)));
        disposables.add_rc(delayer_recompute_state.clone());
        disposables.add_rc(delayer_recompute_active.clone());
        disposables.add_rc(delayer_recompute_symbols.clone());

        let configuration_service = services.configuration_service.clone();
        let this = Rc::new_cyclic(|weak| Self {
            this: weak.clone(),
            editor,
            target,
            services,
            disposables,
            model_disposables,
            data_source_disposables,
            on_did_change,
            delayer_recompute_state,
            delayer_recompute_active,
            delayer_recompute_symbols,
            config: OnceCell::new(),
            outline_data_source_reference: RefCell::new(None),
            tree_data_source: RefCell::new(None),
            quick_pick_data_source: RefCell::new(None),
            breadcrumbs_data_source: RefCell::new(None),
            outline_show_code_cells: Cell::new(read_bool(&*configuration_service, NotebookSetting::OUTLINE_SHOW_CODE_CELLS)),
            outline_show_code_cell_symbols: Cell::new(read_bool(&*configuration_service, NotebookSetting::OUTLINE_SHOW_CODE_CELL_SYMBOLS)),
            outline_show_markdown_headers_only: Cell::new(read_bool(&*configuration_service, NotebookSetting::OUTLINE_SHOW_MARKDOWN_HEADERS_ONLY)),
        });

        this.initialize_outline();

        let config = NotebookOutlineConfig {
            tree_data_source: this.tree_data_source.borrow().clone().expect("setDataSources define a fonte da árvore"),
            quick_pick_data_source: this.quick_pick_data_source.borrow().clone().expect("setDataSources define a fonte do seletor rápido"),
            breadcrumbs_data_source: this.breadcrumbs_data_source.borrow().clone().expect("setDataSources define a fonte das migalhas"),
        };
        assert!(this.config.set(config).is_ok(), "o config é definido uma vez");
        this
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<OutlineChangeEvent> {
        self.on_did_change.event()
    }

    /// `config`.
    pub fn config(&self) -> &NotebookOutlineConfig {
        self.config.get().expect("o construtor define o config")
    }

    /// `get activeElement()`.
    pub fn active_element(&self) -> Option<Rc<OutlineEntry>> {
        self.check_delayer();
        if self.target == OutlineTarget::OUTLINE_PANE {
            self.config().tree_data_source.get_active_entry()
        } else {
            eprintln!("activeElement should not be called outside of the OutlinePane");
            None
        }
    }

    /// `get entries()`.
    pub fn entries(&self) -> Vec<Rc<OutlineEntry>> {
        self.check_delayer();
        self.outline_data_source_reference.borrow().as_ref().map(|reference| reference.object.entries()).unwrap_or_default()
    }

    /// `get uri()`.
    pub fn uri(&self) -> Option<URI> {
        self.outline_data_source_reference.borrow().as_ref().and_then(|reference| reference.object.uri())
    }

    /// `get isEmpty()`.
    pub fn is_empty(&self) -> bool {
        let reference = self.outline_data_source_reference.borrow();
        let Some(reference) = reference.as_ref() else {
            return true;
        };

        !reference.object.entries().iter().any(|entry| {
            !filter_entry(
                entry,
                self.outline_show_markdown_headers_only.get(),
                self.outline_show_code_cells.get(),
                self.outline_show_code_cell_symbols.get(),
            )
        })
    }

    fn check_delayer(&self) {
        if self.delayer_recompute_state.is_triggered() {
            self.delayer_recompute_state.cancel();
            self.recompute_state();
        }
    }

    fn initialize_outline(&self) {
        // initial setup
        self.set_data_sources();
        self.set_model_listeners();

        // reset the data sources + model listeners when we get a new notebook model
        let weak = self.this.clone();
        self.disposables.add(self.editor.on_did_change_model().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.set_data_sources();
                this.set_model_listeners();
                this.compute_symbols(&CancellationToken::None);
            }
        }));

        // recompute symbols as document symbol providers are updated in the language features registry
        let weak = self.this.clone();
        self.disposables.add(self.services.language_features_service.document_symbol_provider.on_did_change().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.delayed_compute_symbols();
            }
        }));

        // recompute active when the selection changes
        let weak = self.this.clone();
        self.disposables.add(self.editor.on_did_change_selection().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.delayed_recompute_active();
            }
        }));

        // recompute state when filter config changes
        let weak = self.this.clone();
        self.disposables.add(self.services.configuration_service.on_did_change_configuration().listen(move |e| {
            let Some(this) = weak.upgrade() else { return };
            if e.affects_configuration(NotebookSetting::OUTLINE_SHOW_MARKDOWN_HEADERS_ONLY, None)
                || e.affects_configuration(NotebookSetting::OUTLINE_SHOW_CODE_CELLS, None)
                || e.affects_configuration(NotebookSetting::OUTLINE_SHOW_CODE_CELL_SYMBOLS, None)
                || e.affects_configuration(NotebookSetting::BREADCRUMBS_SHOW_CODE_CELLS, None)
            {
                this.read_view_settings();
                this.delayed_recompute_state();
            }
        }));

        // recompute state when execution states change
        let weak = self.this.clone();
        self.disposables.add(self.services.notebook_execution_state_service.on_did_change_execution().listen(move |e| {
            let Some(this) = weak.upgrade() else { return };
            if let ExecutionStateChange::Cell(cell_event) = e
                && this.editor.text_model().is_some_and(|text_model| cell_event.affects_notebook(&text_model.uri()))
            {
                this.delayed_recompute_state();
            }
        }));

        // recompute symbols when the configuration changes (recompute state - and therefore recompute active - is also called within compute symbols)
        let weak = self.this.clone();
        self.disposables.add(self.services.configuration_service.on_did_change_configuration().listen(move |e| {
            let Some(this) = weak.upgrade() else { return };
            if e.affects_configuration(NotebookSetting::OUTLINE_SHOW_CODE_CELL_SYMBOLS, None) {
                this.outline_show_code_cell_symbols.set(read_bool(&*this.services.configuration_service, NotebookSetting::OUTLINE_SHOW_CODE_CELL_SYMBOLS));
                this.compute_symbols(&CancellationToken::None);
            }
        }));

        // fire a change event when the theme changes
        let on_did_change = self.on_did_change.clone();
        self.disposables.add(self.services.on_did_file_icon_theme_change.listen(move |_| {
            on_did_change.fire(OutlineChangeEvent::default());
        }));

        // finish with a recompute state
        self.recompute_state();
    }

    fn read_view_settings(&self) {
        let configuration_service = &*self.services.configuration_service;
        self.outline_show_code_cells.set(read_bool(configuration_service, NotebookSetting::OUTLINE_SHOW_CODE_CELLS));
        self.outline_show_code_cell_symbols.set(read_bool(configuration_service, NotebookSetting::OUTLINE_SHOW_CODE_CELL_SYMBOLS));
        self.outline_show_markdown_headers_only.set(read_bool(configuration_service, NotebookSetting::OUTLINE_SHOW_MARKDOWN_HEADERS_ONLY));
    }

    /// set up the primary data source + three viewing sources for the various outline views
    fn set_data_sources(&self) {
        let notebook_editor = self.editor.get_control();
        let previous = self.outline_data_source_reference.borrow_mut().take();
        if let Some(previous) = previous {
            previous.dispose();
        }
        self.data_source_disposables.clear();

        match notebook_editor {
            None => *self.outline_data_source_reference.borrow_mut() = None,
            Some(notebook_editor) => {
                let reference = self.services.data_source_factory.get_or_create(notebook_editor);
                let to_release = reference.clone();
                self.data_source_disposables.add(to_disposable(move || to_release.dispose()));
                // escalate outline data source change events
                let on_did_change = self.on_did_change.clone();
                self.data_source_disposables.add(reference.object.on_did_change().listen(move |_| {
                    on_did_change.fire(OutlineChangeEvent::default());
                }));
                *self.outline_data_source_reference.borrow_mut() = Some(reference);
            }
        }

        // these fields can be passed undefined outlineDataSources. View Providers all handle it accordingly
        let provider_reference = || self.outline_data_source_reference.borrow().as_ref().map(as_provider_reference);
        let configuration_service = &self.services.configuration_service;
        let tree = self.data_source_disposables.add_rc(Rc::new(NotebookOutlinePaneProvider::new(provider_reference(), configuration_service)));
        let quick_pick =
            self.data_source_disposables.add_rc(Rc::new(NotebookQuickPickProvider::new(provider_reference(), configuration_service, self.services.file_icon_theme.clone())));
        let breadcrumbs = self.data_source_disposables.add_rc(Rc::new(NotebookBreadcrumbsProvider::new(provider_reference(), configuration_service)));
        *self.tree_data_source.borrow_mut() = Some(tree);
        *self.quick_pick_data_source.borrow_mut() = Some(quick_pick);
        *self.breadcrumbs_data_source.borrow_mut() = Some(breadcrumbs);
    }

    /// set up the listeners for the outline content, these respond to model changes in the notebook
    fn set_model_listeners(&self) {
        self.model_disposables.clear();
        let Some(text_model) = self.editor.text_model() else {
            return;
        };

        // Perhaps this is the first time we're building the outline
        if self.entries().is_empty() {
            self.compute_symbols(&CancellationToken::None);
        }

        // recompute state when there are notebook content changes
        let weak = self.this.clone();
        self.model_disposables.add(text_model.on_did_change_content().listen(move |content_changes| {
            let Some(this) = weak.upgrade() else { return };
            if content_changes.raw_events.iter().any(|change| {
                matches!(
                    change.event.kind(),
                    NotebookCellsChangeType::ChangeCellContent
                        | NotebookCellsChangeType::ChangeCellInternalMetadata
                        | NotebookCellsChangeType::Move
                        | NotebookCellsChangeType::ModelChange
                )
            }) {
                this.delayed_recompute_state();
            }
        }));
    }

    /// `computeSymbols(cancelToken)`.
    fn compute_symbols(&self, cancel_token: &CancellationToken) {
        if self.target == OutlineTarget::OUTLINE_PANE && self.outline_show_code_cell_symbols.get() {
            // No need to wait for this, we want the outline to show up quickly.
            let _ = self.do_compute_symbols(cancel_token);
        }
    }

    /// `doComputeSymbols(cancelToken)`.
    pub fn do_compute_symbols(&self, cancel_token: &CancellationToken) -> Promise<()> {
        match &*self.outline_data_source_reference.borrow() {
            Some(reference) => reference.object.compute_full_symbols(cancel_token),
            None => Promise::resolved(()),
        }
    }

    fn delayed_compute_symbols(&self) {
        self.delayer_recompute_state.cancel();
        self.delayer_recompute_active.cancel();
        let weak = self.this.clone();
        let _ = self.delayer_recompute_symbols.trigger(
            move || {
                if let Some(this) = weak.upgrade() {
                    this.compute_symbols(&CancellationToken::None);
                }
                MaybePromise::Value(())
            },
            None,
        );
    }

    fn recompute_state(&self) {
        if let Some(reference) = &*self.outline_data_source_reference.borrow() {
            reference.object.recompute_state();
        }
    }

    fn delayed_recompute_state(&self) {
        self.delayer_recompute_active.cancel(); // Active is always recomputed after a recomputing the State.
        let weak = self.this.clone();
        let _ = self.delayer_recompute_state.trigger(
            move || {
                if let Some(this) = weak.upgrade() {
                    this.recompute_state();
                }
                MaybePromise::Value(())
            },
            None,
        );
    }

    fn recompute_active(&self) {
        if let Some(reference) = &*self.outline_data_source_reference.borrow() {
            reference.object.recompute_active();
        }
    }

    fn delayed_recompute_active(&self) {
        let weak = self.this.clone();
        let _ = self.delayer_recompute_active.trigger(
            move || {
                if let Some(this) = weak.upgrade() {
                    this.recompute_active();
                }
                MaybePromise::Value(())
            },
            None,
        );
    }
}

impl IDisposable for NotebookCellOutline {
    fn dispose(&self) {
        self.on_did_change.dispose();
        self.disposables.dispose();
        self.model_disposables.dispose();
        self.data_source_disposables.dispose();
        let reference = self.outline_data_source_reference.borrow_mut().take();
        if let Some(reference) = reference {
            reference.dispose();
        }
    }

    fn constructor_name(&self) -> &'static str {
        "NotebookCellOutline"
    }
}

/// `NotebookOutlineCreator` (sem o registro no `IOutlineService`, ver as divergências do módulo).
pub struct NotebookOutlineCreator {
    services: NotebookOutlineServices,
}

impl NotebookOutlineCreator {
    pub fn new(services: NotebookOutlineServices) -> Self {
        Self { services }
    }

    /// `createOutline(editor, target, cancelToken)`.
    pub fn create_outline(&self, editor: Rc<dyn INotebookEditorPane>, target: OutlineTarget, cancel_token: &CancellationToken) -> Promise<Rc<NotebookCellOutline>> {
        let outline = NotebookCellOutline::new(editor, target, self.services.clone());
        if target == OutlineTarget::QUICK_PICK {
            // The quickpick creates the outline on demand
            // so we need to ensure the symbols are pre-cached before the entries are syncronously requested
            let created = outline.clone();
            return outline.do_compute_symbols(cancel_token).then(move |()| created);
        }
        Promise::resolved(outline)
    }
}
