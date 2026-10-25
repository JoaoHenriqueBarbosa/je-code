//! Porte de `src/vs/workbench/contrib/codeEditor/browser/outline/documentSymbolsOutline.ts`: o outline de símbolos do
//! documento (`DocumentSymbolsOutline`) e a fonte das migalhas (`DocumentSymbolBreadcrumbsSource`).
//!
//! Notas de tradução:
//! - **Injeção.** Os decoradores `@ILanguageFeaturesService`, `@IOutlineModelService`, `@IConfigurationService`,
//!   `@IMarkerDecorationsService`, `@ITextResourceConfigurationService` e `@IInstantiationService` são os campos de
//!   [`DocumentSymbolsOutlineServices`] (o `documentSymbolProvider` do `ILanguageFeaturesService` é o registro de provedores; o
//!   `IInstantiationService` só criava o renderer e o filtro, que recebem o que usam).
//! - **`DocumentSymbolItem`.** É o [`DocumentSymbolItem`]; o elemento do outline que a `IOutline` expõe é o `Rc` do grupo ou do
//!   elemento do modelo (`Rc<dyn Any>`), de modo que quem consome o recupera por `downcast`.
//! - **`async _createOutline`.** É um `async fn` sobre `Rc<Inner>` entregue por `async_fn`; o `try/catch` envolve só os `await`
//!   que podem rejeitar (o resto não lança).
//! - **`treeDataSource` com `parent === this`.** A entrada da árvore ([`InputOrElement::Input`]) é o próprio outline, e os filhos
//!   dela são os do `_outlineModel` atual, que a fonte de dados lê do estado compartilhado.
//! - **`dnd`.** Sem porte (ver `documentSymbolsTree`), então as opções da árvore não o definem.
//! - **`DocumentSymbolsOutlineCreator`.** O registro no `IOutlineService` e a escolha do adaptador pelo `IEditorPane`
//!   (`isCodeEditor`, `isDiffEditor`, `CustomEditorInput.navigation`) dependem do painel de editor concreto e ficam para quem
//!   liga o serviço; os adaptadores e o outline estão prontos para ele.

use std::any::Any;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::{Barrier, TimeoutTimer, async_fn, race_cancellation_or, timeout};
use je_base::cancellation::CancellationTokenSource;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error, on_unexpected_error};
use je_base::event::{Emitter, Event};
use je_base::filters::FuzzyScore;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::language_feature_registry::LanguageFeatureRegistry;
use je_editor::language_providers::DocumentSymbolProvider;
use je_editor::outline_model::{IOutlineModelService, OutlineMarker, OutlineModel, TreeElement};
use je_editor::position::Position;
use je_editor::services::marker_decorations::IMarkerDecorationsService;
use je_editor::text_model_events::ModelContentChangedEvent;
use je_editor::text_resource_configuration_service::ITextResourceConfigurationService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::markers::MarkerSeverity;
use je_platform::opener::editor_types::EditorOptions;
use je_ui::Ui;
use je_widgets::tree::abstract_tree::ExpandOnlyOnTwistieClick;
use je_widgets::tree::data_tree::DataTreeOptions;
use je_widgets::tree::object_tree_model::ObjectTreeModel;
use je_widgets::tree::tree_types::{IDataSource, ITreeFilter, ITreeRenderer, InputOrElement};

use super::document_symbol_editor::IDocumentSymbolEditor;
use super::document_symbols_tree::{
    DocumentSymbolAccessibilityProvider, DocumentSymbolComparator, DocumentSymbolFilter, DocumentSymbolGroupRenderer, DocumentSymbolIdentityProvider, DocumentSymbolItem,
    DocumentSymbolNavigationLabelProvider, DocumentSymbolRenderer, DocumentSymbolVirtualDelegate, document_symbol_kind_to_config_name,
};
use crate::services::outline::outline::{
    BreadcrumbsOutlineElement, IBreadcrumbsDataSource, IOutline, IOutlineListConfig, IQuickPickDataSource, OutlineChangeEvent, OutlineConfigCollapseItemsValues, OutlineConfigKeys,
    OutlineElement as OutlineItem, OutlineTarget, OutlineTreeElement, QuickPickOutlineElement,
};

/// `Dynamic` como `boolean` do `getValue<boolean>`: só o `true` conta (as chaves de `breadcrumbs.*` e `outline.*` são booleanas).
fn is_true(value: &Dynamic) -> bool {
    matches!(value, Dynamic::Bool(true))
}

/// `DocumentSymbolBreadcrumbsSource`.
struct DocumentSymbolBreadcrumbsSource {
    editor: Rc<dyn IDocumentSymbolEditor>,
    text_resource_configuration_service: Rc<dyn ITextResourceConfigurationService>,
    breadcrumbs: RefCell<Vec<BreadcrumbsOutlineElement>>,
}

impl DocumentSymbolBreadcrumbsSource {
    fn new(editor: Rc<dyn IDocumentSymbolEditor>, text_resource_configuration_service: Rc<dyn ITextResourceConfigurationService>) -> Rc<Self> {
        Rc::new(Self { editor, text_resource_configuration_service, breadcrumbs: RefCell::new(Vec::new()) })
    }

    /// `clear()`.
    fn clear(&self) {
        self.breadcrumbs.borrow_mut().clear();
    }

    /// `update(model, position)`.
    fn update(&self, model: &Rc<OutlineModel>, position: Position) {
        let new_elements = self.compute_breadcrumbs(model, position);
        *self.breadcrumbs.borrow_mut() = new_elements
            .into_iter()
            .map(|element| {
                let label = match &element {
                    DocumentSymbolItem::Element(element) => element.symbol.name.clone(),
                    DocumentSymbolItem::Group(_) => String::new(),
                };
                BreadcrumbsOutlineElement { element: element.to_outline_item(), label }
            })
            .collect();
    }

    /// `_computeBreadcrumbs(model, position)`.
    fn compute_breadcrumbs(&self, model: &Rc<OutlineModel>, position: Position) -> Vec<DocumentSymbolItem> {
        let Some(enclosing) = model.get_item_enclosing_position(position, None) else {
            return Vec::new();
        };
        let mut chain: Vec<TreeElement> = Vec::new();
        let mut item = Some(TreeElement::Element(enclosing));
        while let Some(current) = item {
            chain.push(current.clone());
            let parent = current.parent();
            match &parent {
                Some(TreeElement::Model(_)) => break,
                Some(TreeElement::Group(group)) => {
                    let grandparent = TreeElement::Group(Rc::clone(group)).parent();
                    if grandparent.is_some_and(|grandparent| grandparent.children().borrow().len() == 1) {
                        break;
                    }
                }
                _ => {}
            }
            item = parent;
        }
        let mut result: Vec<DocumentSymbolItem> = Vec::new();
        for element in chain.iter().rev() {
            let Some(item) = DocumentSymbolItem::from_tree_element(element) else { continue };
            if self.is_filtered(&item) {
                break;
            }
            result.push(item);
        }
        result
    }

    /// `_isFiltered(element)`.
    fn is_filtered(&self, element: &DocumentSymbolItem) -> bool {
        let DocumentSymbolItem::Element(element) = element else {
            return false;
        };
        let key = format!("breadcrumbs.{}", document_symbol_kind_to_config_name(element.symbol.kind));
        let uri: Option<URI> = self.editor.get_model().map(|model| model.uri());
        !is_true(&self.text_resource_configuration_service.get_value(uri.as_ref(), Some(&key)))
    }
}

impl IBreadcrumbsDataSource for DocumentSymbolBreadcrumbsSource {
    fn get_breadcrumb_elements(&self) -> Vec<BreadcrumbsOutlineElement> {
        self.breadcrumbs.borrow().clone()
    }
}

/// Os serviços que o `DocumentSymbolsOutline` recebe por injeção.
#[derive(Clone)]
pub struct DocumentSymbolsOutlineServices {
    pub ui: Ui,
    pub language_features: LanguageFeatureRegistry<dyn DocumentSymbolProvider>,
    pub outline_model_service: Rc<dyn IOutlineModelService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub marker_decorations_service: Rc<dyn IMarkerDecorationsService>,
    pub text_resource_configuration_service: Rc<dyn ITextResourceConfigurationService>,
}

/// O estado compartilhado do outline: o que os ouvintes, a fonte de dados da árvore e os métodos leem.
struct Inner {
    editor: Rc<dyn IDocumentSymbolEditor>,
    services: DocumentSymbolsOutlineServices,
    disposables: Rc<DisposableStore>,
    outline_disposables: Rc<DisposableStore>,
    on_did_change: Rc<Emitter<OutlineChangeEvent>>,
    outline_model: RefCell<Option<Rc<OutlineModel>>>,
    breadcrumbs_data_source: Rc<DocumentSymbolBreadcrumbsSource>,
}

impl Inner {
    fn dispose_all(&self) {
        self.disposables.dispose();
        self.outline_disposables.dispose();
    }

    /// `_createOutline(contentChangeEvent?)`.
    fn create_outline(self: &Rc<Self>, content_change: Option<ModelContentChangedEvent>) -> Promise<()> {
        let inner = Rc::clone(self);
        async_fn(async move {
            inner.run_create_outline(content_change).await;
            Ok(())
        })
    }

    async fn run_create_outline(self: Rc<Self>, content_change: Option<ModelContentChangedEvent>) {
        self.outline_disposables.clear();
        if content_change.is_none() {
            self.set_outline_model(None);
        }

        let Some(buffer) = self.editor.get_model() else {
            return;
        };
        if !self.services.language_features.has(&buffer) {
            return;
        }

        let cts = CancellationTokenSource::new(None);
        let version_id_then = buffer.get_version_id();
        let timeout_timer = TimeoutTimer::new();

        self.outline_disposables.add_rc(Rc::clone(&timeout_timer));
        let cts_to_dispose = cts.clone();
        self.outline_disposables.add(to_disposable(move || cts_to_dispose.dispose_with(true)));

        let model = match self.services.outline_model_service.get_or_create(&buffer, &cts.token()).await {
            Ok(model) => model,
            Err(error) => {
                self.set_outline_model(None);
                on_unexpected_error(&error);
                return;
            }
        };
        if cts.token().is_cancellation_requested() {
            // cancelled -> do nothing
            return;
        }

        let model_element = TreeElement::Model(Rc::clone(&model));
        if TreeElement::empty(&model_element) || self.editor.get_model().is_none() {
            // empty -> no outline elements
            self.set_outline_model(Some(model));
            return;
        }

        // heuristic: when the symbols-to-lines ratio changes by 50% between edits
        // wait a little (and hope that the next change isn't as drastic).
        let previous_model = self.outline_model.borrow().clone();
        if let (Some(change), Some(previous_model)) = (&content_change, previous_model) {
            if buffer.get_line_count() >= 25 {
                let new_size = TreeElement::size(&model_element) as f64;
                let new_length = buffer.get_value_length() as f64;
                let new_ratio = new_size / new_length;
                let old_size = TreeElement::size(&TreeElement::Model(previous_model)) as f64;
                let old_length = new_length - change.changes.iter().map(|change| change.range_length as f64).sum::<f64>();
                let old_ratio = old_size / old_length;
                if new_ratio <= old_ratio * 0.5 || new_ratio >= old_ratio * 1.5 {
                    // wait for a better state and ignore current model when more
                    // typing has happened
                    let waited = timeout(2000).promise.then(|_| true);
                    match race_cancellation_or(&waited, &cts.token(), false).await {
                        Ok(true) => {}
                        Ok(false) => return,
                        Err(error) => {
                            self.set_outline_model(None);
                            on_unexpected_error(&error);
                            return;
                        }
                    }
                }
            }
        }

        // feature: show markers with outline element
        self.apply_markers_to_outline(Some(&model));
        let weak = Rc::downgrade(&self);
        let marker_model = Rc::clone(&model);
        self.outline_disposables.add(self.services.marker_decorations_service.on_did_change_marker().listen({
            let weak = weak.clone();
            move |text_model| {
                let Some(inner) = weak.upgrade() else { return };
                if marker_model.uri.to_string(false) == text_model.uri().to_string(false) {
                    inner.apply_markers_to_outline(Some(&marker_model));
                    inner.on_did_change.fire(OutlineChangeEvent::default());
                }
            }
        }));
        let config_model = Rc::clone(&model);
        self.outline_disposables.add(self.services.configuration_service.on_did_change_configuration().listen({
            let weak = weak.clone();
            move |event| {
                let Some(inner) = weak.upgrade() else { return };
                if event.affects_configuration(OutlineConfigKeys::PROBLEMS_ENABLED, None) || event.affects_configuration("problems.visibility", None) {
                    let problem = inner.services.configuration_service.get_value(Some("problems.visibility"), None);
                    let config = inner.services.configuration_service.get_value(Some(OutlineConfigKeys::PROBLEMS_ENABLED), None);

                    if !problem.is_truthy() || !config.is_truthy() {
                        config_model.update_marker(&mut []);
                    } else {
                        inner.apply_markers_to_outline(Some(&config_model));
                    }
                    inner.on_did_change.fire(OutlineChangeEvent::default());
                }
                if event.affects_configuration("outline", None) {
                    // outline filtering, problems on/off
                    inner.on_did_change.fire(OutlineChangeEvent::default());
                }
                if event.affects_configuration("breadcrumbs", None) {
                    // breadcrumbs filtering
                    inner.update_breadcrumbs(&config_model);
                    inner.on_did_change.fire(OutlineChangeEvent::default());
                }
            }
        }));

        // feature: toggle icons
        self.outline_disposables.add(self.services.configuration_service.on_did_change_configuration().listen({
            let weak = weak.clone();
            move |event| {
                let Some(inner) = weak.upgrade() else { return };
                if event.affects_configuration(OutlineConfigKeys::ICONS, None) {
                    inner.on_did_change.fire(OutlineChangeEvent::default());
                }
                if event.affects_configuration("outline", None) {
                    inner.on_did_change.fire(OutlineChangeEvent::default());
                }
            }
        }));

        // feature: update active when cursor changes
        let cursor_model = Rc::clone(&model);
        let cursor_buffer = Rc::clone(&buffer);
        let cursor_timer = Rc::clone(&timeout_timer);
        self.outline_disposables.add(self.editor.on_did_change_cursor_position().listen(move |_| {
            let weak = weak.clone();
            let (model, buffer) = (Rc::clone(&cursor_model), Rc::clone(&cursor_buffer));
            cursor_timer.cancel_and_set(
                move || {
                    let Some(inner) = weak.upgrade() else { return };
                    if !buffer.is_disposed() && version_id_then == buffer.get_version_id() && inner.editor.get_model().is_some() {
                        inner.update_breadcrumbs(&model);
                        inner.on_did_change.fire(OutlineChangeEvent { affect_only_active_element: true });
                    }
                },
                150,
            );
        }));

        // update properties, send event
        self.set_outline_model(Some(model));
    }

    /// `_applyMarkersToOutline(model)`.
    fn apply_markers_to_outline(&self, model: Option<&Rc<OutlineModel>>) {
        let problem = self.services.configuration_service.get_value(Some("problems.visibility"), None);
        let config = self.services.configuration_service.get_value(Some(OutlineConfigKeys::PROBLEMS_ENABLED), None);
        let Some(model) = model else { return };
        if !problem.is_truthy() || !config.is_truthy() {
            return;
        }
        let mut markers: Vec<OutlineMarker> = Vec::new();
        for (range, marker) in self.services.marker_decorations_service.get_live_markers(&model.uri) {
            if marker.severity == MarkerSeverity::ERROR || marker.severity == MarkerSeverity::WARNING {
                markers.push(OutlineMarker { range, severity: marker.severity });
            }
        }
        model.update_marker(&mut markers);
    }

    /// `_setOutlineModel(model)`.
    fn set_outline_model(&self, model: Option<Rc<OutlineModel>>) {
        match model {
            None => {
                *self.outline_model.borrow_mut() = None;
                self.breadcrumbs_data_source.clear();
            }
            Some(model) => {
                let merged = self.outline_model.borrow().as_ref().is_some_and(|current| current.merge(&model));
                if !merged {
                    *self.outline_model.borrow_mut() = Some(Rc::clone(&model));
                }
                self.update_breadcrumbs(&model);
            }
        }
        self.on_did_change.fire(OutlineChangeEvent::default());
    }

    /// `_updateBreadcrumbs(model)`.
    fn update_breadcrumbs(&self, model: &Rc<OutlineModel>) {
        match self.editor.get_position() {
            Some(position) => self.breadcrumbs_data_source.update(model, position),
            None => self.breadcrumbs_data_source.clear(),
        }
    }
}

/// `treeDataSource`: os filhos do outline são os do modelo atual, os de um grupo ou de um elemento são os dele.
struct DocumentSymbolDataSource {
    inner: Rc<Inner>,
}

/// Os filhos de um mapa do modelo como elementos da árvore.
fn child_items(children: &je_editor::outline_model::Children) -> Vec<OutlineTreeElement> {
    children
        .borrow()
        .values()
        .filter_map(DocumentSymbolItem::from_tree_element)
        .map(|item| OutlineTreeElement(item.to_outline_item()))
        .collect()
}

impl IDataSource<Rc<dyn IOutline>, OutlineTreeElement> for DocumentSymbolDataSource {
    fn get_children(&self, element: &InputOrElement<Rc<dyn IOutline>, OutlineTreeElement>) -> Vec<OutlineTreeElement> {
        match element {
            InputOrElement::Element(element) => match DocumentSymbolItem::from_outline_item(&element.0) {
                Some(item) => child_items(&item.to_tree_element().children()),
                None => Vec::new(),
            },
            InputOrElement::Input(_) => match self.inner.outline_model.borrow().as_ref() {
                Some(model) => child_items(&model.children()),
                None => Vec::new(),
            },
        }
    }
}

/// `quickPickDataSource: { getQuickPickElements: () => { throw new Error('not implemented'); } }`.
struct NotImplementedQuickPick;

impl IQuickPickDataSource for NotImplementedQuickPick {
    fn get_quick_pick_elements(&self) -> Vec<QuickPickOutlineElement> {
        errors::throw(Error::named("Error", "not implemented"))
    }
}

/// `DocumentSymbolsOutline`.
pub struct DocumentSymbolsOutline {
    inner: Rc<Inner>,
    config: IOutlineListConfig,
}

impl DocumentSymbolsOutline {
    /// `new DocumentSymbolsOutline(editor, target, firstLoadBarrier, ...services)`.
    pub fn new(editor: Rc<dyn IDocumentSymbolEditor>, target: OutlineTarget, first_load_barrier: Barrier, services: DocumentSymbolsOutlineServices) -> Rc<Self> {
        let breadcrumbs_data_source = DocumentSymbolBreadcrumbsSource::new(Rc::clone(&editor), Rc::clone(&services.text_resource_configuration_service));
        let disposables = DisposableStore::new();
        let on_did_change = disposables.add_rc(Emitter::new());
        let inner = Rc::new(Inner {
            editor: Rc::clone(&editor),
            services: services.clone(),
            disposables,
            outline_disposables: DisposableStore::new(),
            on_did_change,
            outline_model: RefCell::new(None),
            breadcrumbs_data_source: Rc::clone(&breadcrumbs_data_source),
        });

        let initial_state = services.text_resource_configuration_service.get_value(editor.get_model().map(|model| model.uri()).as_ref(), Some(OutlineConfigKeys::COLLAPSE_ITEMS));
        let collapsed_initially = matches!(&initial_state, Dynamic::String(value) if value.to_string_lossy() == OutlineConfigCollapseItemsValues::COLLAPSED);
        let collapse_by_default = target == OutlineTarget::BREADCRUMBS || (target == OutlineTarget::OUTLINE_PANE && collapsed_initially);
        let filter_prefix: Option<&'static str> = if target == OutlineTarget::OUTLINE_PANE {
            Some("outline")
        } else if target == OutlineTarget::BREADCRUMBS {
            Some("breadcrumbs")
        } else {
            None
        };
        let text_resource_configuration_service = Rc::clone(&services.text_resource_configuration_service);
        let options = move || {
            let mut options = DataTreeOptions::<OutlineTreeElement, FuzzyScore>::default();
            options.tree.collapse_by_default = collapse_by_default;
            options.tree.update.expand_only_on_twistie_click = Some(ExpandOnlyOnTwistieClick::Always(true));
            options.tree.update.multiple_selection_support = Some(false);
            options.tree.identity_provider = Some(Rc::new(DocumentSymbolIdentityProvider));
            options.tree.keyboard_navigation_label_provider = Some(Rc::new(DocumentSymbolNavigationLabelProvider));
            options.tree.accessibility_provider = Some(Rc::new(DocumentSymbolAccessibilityProvider::new("Document Symbols")));
            if let Some(prefix) = filter_prefix {
                let filter: Rc<dyn ITreeFilter<Option<OutlineTreeElement>, FuzzyScore>> = Rc::new(DocumentSymbolFilter::new(prefix, Rc::clone(&text_resource_configuration_service)));
                options.tree.filter = Some(filter);
            }
            options
        };

        let group_renderer: Rc<dyn ITreeRenderer<ObjectTreeModel<OutlineTreeElement, FuzzyScore>>> = Rc::new(DocumentSymbolGroupRenderer::new(services.ui.clone()));
        let symbol_renderer: Rc<dyn ITreeRenderer<ObjectTreeModel<OutlineTreeElement, FuzzyScore>>> = Rc::new(DocumentSymbolRenderer::new(services.ui.clone()));
        let config = IOutlineListConfig {
            breadcrumbs_data_source: breadcrumbs_data_source as Rc<dyn IBreadcrumbsDataSource>,
            delegate: Rc::new(DocumentSymbolVirtualDelegate),
            renderers: vec![group_renderer, symbol_renderer],
            tree_data_source: Rc::new(DocumentSymbolDataSource { inner: Rc::clone(&inner) }),
            comparator: Rc::new(DocumentSymbolComparator::new()),
            options: Rc::new(options),
            quick_pick_data_source: Rc::new(NotImplementedQuickPick),
        };

        let weak: Weak<Inner> = Rc::downgrade(&inner);
        let recreate = |event: Event<()>, weak: Weak<Inner>| {
            event.listen(move |_| {
                if let Some(inner) = weak.upgrade() {
                    let _ = inner.create_outline(None);
                }
            })
        };

        // update as language, model, providers changes
        inner.disposables.add(services.language_features.on_did_change().signal().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(inner) = weak.upgrade() {
                    let _ = inner.create_outline(None);
                }
            }
        }));
        inner.disposables.add(recreate(editor.on_did_change_model(), weak.clone()));
        inner.disposables.add(recreate(editor.on_did_change_model_language(), weak.clone()));

        // update soon'ish as model content change
        let update_soon = TimeoutTimer::new();
        inner.disposables.add_rc(Rc::clone(&update_soon));
        inner.disposables.add(editor.on_did_change_model_content().listen({
            let weak = weak.clone();
            move |event| {
                let Some(inner) = weak.upgrade() else { return };
                let Some(model) = inner.editor.get_model() else { return };
                let delay = inner.services.outline_model_service.get_debounce_value(&model);
                let (event, weak) = (event.clone(), weak.clone());
                update_soon.cancel_and_set(
                    move || {
                        if let Some(inner) = weak.upgrade() {
                            let _ = inner.create_outline(Some(event));
                        }
                    },
                    delay as u64,
                );
            }
        }));

        // stop when editor dies
        inner.disposables.add(editor.on_did_dispose().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(inner) = weak.upgrade() {
                    inner.dispose_all();
                }
            }
        }));

        // initial load
        let _ = inner.create_outline(None).finally(move || first_load_barrier.open());

        Rc::new(Self { inner, config })
    }

    /// O `OutlineElement` (`Rc<dyn Any>`) do item do modelo, para quem consome o outline.
    fn item_of(entry: &OutlineItem) -> Option<DocumentSymbolItem> {
        DocumentSymbolItem::from_outline_item(entry)
    }
}

impl IDisposable for DocumentSymbolsOutline {
    fn dispose(&self) {
        self.inner.dispose_all();
    }
}

impl IOutline for DocumentSymbolsOutline {
    fn uri(&self) -> Option<URI> {
        self.inner.outline_model.borrow().as_ref().map(|model| model.uri.clone())
    }

    fn config(&self) -> &IOutlineListConfig {
        &self.config
    }

    fn outline_kind(&self) -> String {
        "documentSymbols".to_owned()
    }

    fn is_empty(&self) -> bool {
        match self.inner.outline_model.borrow().as_ref() {
            Some(model) => TreeElement::empty(&TreeElement::Model(Rc::clone(model))),
            None => true,
        }
    }

    fn active_element(&self) -> Option<OutlineItem> {
        let position = self.inner.editor.get_position()?;
        let model = self.inner.outline_model.borrow().clone()?;
        let element = model.get_item_enclosing_position(position, None)?;
        Some(element as Rc<dyn Any>)
    }

    fn on_did_change(&self) -> Event<OutlineChangeEvent> {
        self.inner.on_did_change.event()
    }

    fn reveal(&self, entry: &OutlineItem, options: &EditorOptions, side_by_side: bool, select: bool) -> MaybePromise<()> {
        let Some(DocumentSymbolItem::Element(element)) = Self::item_of(entry) else {
            return MaybePromise::Value(());
        };
        let Some(model) = OutlineModel::get(Some(&TreeElement::Element(Rc::clone(&element)))) else {
            return MaybePromise::Value(());
        };
        let selection = if select { element.symbol.range } else { element.symbol.selection_range.collapse_to_start() };
        MaybePromise::Promise(self.inner.editor.reveal(&model.uri, element.symbol.range, selection, options, side_by_side))
    }

    fn preview(&self, entry: &OutlineItem) -> DisposableRef {
        match Self::item_of(entry) {
            Some(DocumentSymbolItem::Element(element)) => self.inner.editor.preview(element.symbol.range),
            _ => je_base::lifecycle::none(),
        }
    }

    fn capture_view_state(&self) -> DisposableRef {
        self.inner.editor.capture_view_state()
    }
}
