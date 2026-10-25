//! Porte de `src/vs/workbench/contrib/timeline/test/browser/timelinePane.test.ts` (suíte `TimelinePane`), caso a caso e na mesma
//! ordem (o laço `for (const hasStaleItems of [false, true])` vira um teste por parâmetro).
//!
//! Notas de tradução:
//! - **`Object.assign(Object.create(TimelinePane.prototype), { ... })`.** O original monta o painel sem o construtor e troca os
//!   serviços por objetos parciais. Aqui o painel nasce pelo construtor, com os serviços que o original troca injetados pelos
//!   mesmos pontos ([`TimelinePaneServices`]): o `tree` é o [`FakeBody`] (o [`ITimelineBody`] que grava as chamadas, entregue por
//!   `set_body`), o `isBodyVisible: () => true` é `ViewPane::set_visible(true)`, o `uri` é `set_uri`, e os serviços de que o
//!   construtor precisa e o teste não usa são os dublês que lançam `X is not a function`, como o objeto parcial faz ao ser
//!   chamado. O `_maxItemCount: 20` é o `pageSize` de uma árvore sem altura (20).
//! - **Membros `private`.** `clear`, `loadTimelineForSource`, `onProvidersChanged`, `onEditableChanged`, `onTimelineChanged`,
//!   `onSelectionChanged`, `loadMore`, `refresh`, `message`, `pendingRequests`, `timelinesBySource` e `excludedSources` são os
//!   métodos e acessores `pub` do painel (ver as divergências de `timeline_pane.rs`).
//! - **`ensureNoDisposablesAreLeakedInTestSuite` e `store.add({ dispose: () => pane.clear(true) })`.** É o guarda de
//!   vazamentos criado no começo do caso; o painel também entra no `store` (o original nunca o constrói).
//! - **`await timeout(0)` e `DeferredPromise`.** Os casos que esperam promessas rodam em `run_with_faked_timers`; `await
//!   deferred.complete(x)` é o `await` da promessa que `complete` devolve.
//! - **`assert.deepStrictEqual` sobre os overlays e as chamadas.** São `assert_eq!` sobre listas de pares; os valores dinâmicos
//!   comparam por `SameValueZero` ([`Dynamic`]) e os argumentos do comando por `assert_deep_strict_equal`.
//! - **`upcastPartial<TimelinePane>({ id: 'timeline' })`.** É o [`PartialPane`], um [`ITimelinePaneCommandsHost`] que só tem o `id`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;
#[path = "support/notification_doubles.rs"]
mod notification_doubles;

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use common::{assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::actions::{IActionRunner, ToActionProps, to_action};
use je_base::cancellation::CancellationTokenSource;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::marshalling_ids::MarshalledId;
use je_base::map::OrderedMap;
use je_base::nls::LocalizedString;
use je_base::promise::{DeferredPromise, MaybePromise, Promise};
use je_base::uri::URI;
use je_editor::services::theme_service::{IColorTheme, IThemeService};
use je_platform::actions::actions::{IMenu, IMenuService, MenuActionGroup, MenuActionOptions, MenuChangeEvent, MenuCreateOptions, MenuId};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::{ContextKeyExpression, IContext, IContextKey, IContextKeyChangeEvent, IContextKeyService, IContextKeyServiceTarget, IScopedContextKeyService};
use je_platform::contextview::context_view::{ContextMenuRequest, IContextMenuService};
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::label::label::{FormatterChangeEvent, ILabelService, LabelFormatter, LabelSeparator, ResourceLabelFormatter, UriLabelOptions, WorkspaceLabelOptions, WorkspaceLabelTarget};
use je_platform::list::list_service::ListService;
use je_platform::progress::progress::{IProgress, IProgressService, Progress, ProgressOptions, ProgressStep, ProgressTask};
use je_platform::resources::{ext_uri, uri_file};
use je_platform::storage::storage::InMemoryStorageService;
use je_platform::storage::StorageService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_render::Rgba;
use je_theme::color::Color;
use je_ui::Ui;
use je_widgets::action_view_items::{IActionViewItem, IActionViewItemOptions};
use je_widgets::highlighted_label::HighlightedLabelStyles;
use je_widgets::hover_api::{IHoverDelegate, get_base_layer_hover_delegate, get_default_hover_delegate};
use je_widgets::icon_label::IconLabelStyles;
use je_widgets::input_box::InputBoxStyles;
use je_widgets::list::list_styles::IListStyles;
use je_widgets::progress_bar::IProgressBarOptions;
use je_workbench::browser::parts::views::view_pane::{
    IViewPaneOptions, IViewPaneToolbar, IViewWelcomeView, ListOverrideStyleColors, ViewPaneServices, ViewPaneToolbarArgs, ViewWelcomeViewArgs,
};
use je_workbench::browser::workbench_tree::WorkbenchTreeServices;
use je_workbench::common::editor::{EditorIdentifier, EditorInputOrUntyped, EditorInputRef, EditorInputWithOptions, EditorsOrder, IEditorPane, UntypedEditorInput};
use je_workbench::common::views::{
    IViewContainerModel, IViewDescriptor, IViewDescriptorService, ViewContainerLocation, ViewContainerLocationChange, ViewContainerRc, ViewContainersChange, ViewDescriptorRc,
    ViewVisibilityState, ViewsContainerChange, ViewsLocationChange,
};
use je_workbench::contrib::files::browser_files::EditableData;
use je_workbench::contrib::timeline::browser::timeline_pane::{
    ITimelineBody, ITimelinePaneCommandsHost, LOAD_MORE_HANDLE, LoadMoreCommand, TimelineActionContext, TimelineActionRunner, TimelineBodyServices, TimelineHoverDelegateArgs,
    TimelineItemRef, TimelinePane, TimelinePaneCommands, TimelinePaneServices, TreeElement, timeline_action_context_to_dynamic,
};
use je_workbench::contrib::timeline::common::timeline::{
    ITimelineService, TIMELINE_PANE_ID, Timeline, TimelineChangeEvent, TimelineEditable, TimelineItem, TimelineItemIdentifier, TimelineOptions, TimelineProvider,
    TimelineProvidersChangeEvent, TimelineRequest, TimelineSource, TimelineTriggerKind, WorkspaceTimelineProvider,
};
use je_workbench::services::editor::editor_service::{
    ActiveTextEditorControl, FindEditorOptions, IEditorService, SaveAllEditorsOptions, SaveEditorsOptions, SaveEditorsResult,
};
use je_workbench::test::browser::workbench_test_services::TestExtensionService;
use je_workbench::test::common::workbench_test_services::TestLifecycleService;
use notification_doubles::{NoKeybindingService, RecordingOpener};
use support::mock_command_service::MockCommandService;

/// O membro que o objeto parcial do original não define: chamá-lo é um `TypeError`.
fn not_a_function(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

// ---- os dublês do `setup` ---------------------------------------------------------------------------

/// `children`, `selection` e `treeCalls` do `createPane`: o que a árvore de mentira grava e devolve.
#[derive(Default)]
struct TreeState {
    tree_calls: RefCell<Vec<String>>,
    children: RefCell<Vec<TreeElement>>,
    selection: RefCell<Vec<Option<TreeElement>>>,
}

/// O `tree: new class extends mock<TimelinePane['tree']>() { ... }` e o `$message` do `createPane`: grava `setChildren`,
/// `rerender`, `hasElement` (sempre verdadeiro), `getSelection` e `domFocus`; o `$container` está sem foco, então o foco volta à
/// árvore; o resto é um membro ausente do objeto parcial.
struct FakeBody {
    state: Rc<TreeState>,
}

impl ITimelineBody for FakeBody {
    fn render_height(&self) -> f64 {
        // `this.tree?.renderHeight ?? 0`: o objeto parcial não define `renderHeight`
        0.0
    }

    fn set_children(&self, elements: Vec<TreeElement>) {
        self.state.tree_calls.borrow_mut().push("setChildren".to_owned());
        *self.state.children.borrow_mut() = elements;
    }

    fn clear_children(&self) {
        // `tree.setChildren(null, undefined)`
        self.set_children(Vec::new());
    }

    fn rerender(&self) {
        self.state.tree_calls.borrow_mut().push("rerender".to_owned());
    }

    fn rerender_element(&self, element: &TreeElement) {
        self.state.tree_calls.borrow_mut().push(format!("rerender {}", element.handle()));
    }

    fn has_element(&self, _element: &TreeElement) -> bool {
        true
    }

    fn get_selection(&self) -> Vec<Option<TreeElement>> {
        self.state.selection.borrow().clone()
    }

    fn dom_focus(&self) {
        self.state.tree_calls.borrow_mut().push("domFocus".to_owned());
    }

    fn set_focus(&self, _element: &TreeElement) {
        not_a_function("setFocus")
    }

    fn layout(&self, _height: f64, _width: f64) {
        not_a_function("layout")
    }

    fn show_message(&self, _message: &str) {}

    fn hide_message(&self) {}

    fn set_uri(&self, _uri: Option<&URI>) {}

    fn is_focus_free_or_inside(&self) -> bool {
        true
    }
}

/// `timelineService: upcastPartial<ITimelineService>({ get editable, getSources, getTimeline, onDidChangeProviders })`.
struct StubTimelineService {
    results: RefCell<OrderedMap<String, Promise<Option<Timeline>>>>,
    requests: RefCell<Vec<TimelineRequest>>,
    editable: RefCell<Option<TimelineEditable>>,
}

impl StubTimelineService {
    fn new() -> Rc<Self> {
        Rc::new(Self { results: RefCell::new(OrderedMap::new()), requests: RefCell::new(Vec::new()), editable: RefCell::new(None) })
    }

    /// `results.set(source, result)`.
    fn set_result(&self, source: &str, result: Promise<Option<Timeline>>) {
        self.results.borrow_mut().insert(source.to_owned(), result);
    }

    /// `results.delete(source)`.
    fn delete_result(&self, source: &str) {
        self.results.borrow_mut().remove(&source.to_owned());
    }

    /// `results.clear()`.
    fn clear_results(&self) {
        self.results.borrow_mut().clear();
    }

    /// `requests`.
    fn requests(&self) -> Vec<TimelineRequest> {
        self.requests.borrow().clone()
    }

    /// `requests[requests.length - 1]`.
    fn last_request(&self) -> TimelineRequest {
        self.requests.borrow().last().cloned().expect("há uma requisição")
    }

    /// `setEditable(value)`.
    fn set_editable_state(&self, value: Option<TimelineEditable>) {
        *self.editable.borrow_mut() = value;
    }
}

impl ITimelineService for StubTimelineService {
    fn on_did_change_providers(&self) -> Event<TimelineProvidersChangeEvent> {
        Event::none()
    }

    fn on_did_change_timeline(&self) -> Event<TimelineChangeEvent> {
        Event::none()
    }

    fn on_did_change_uri(&self) -> Event<Option<URI>> {
        Event::none()
    }

    fn on_did_change_editable(&self) -> Event<()> {
        Event::none()
    }

    fn editable(&self) -> Option<TimelineEditable> {
        self.editable.borrow().clone()
    }

    fn register_timeline_provider(&self, _provider: Rc<dyn TimelineProvider>) -> DisposableRef {
        not_a_function("registerTimelineProvider")
    }

    fn unregister_timeline_provider(&self, _id: &str) {
        not_a_function("unregisterTimelineProvider")
    }

    fn register_workspace_timeline_provider(&self, _provider: Rc<dyn WorkspaceTimelineProvider>) -> DisposableRef {
        not_a_function("registerWorkspaceTimelineProvider")
    }

    fn unregister_workspace_timeline_provider(&self, _id: &str) {
        not_a_function("unregisterWorkspaceTimelineProvider")
    }

    fn get_sources(&self) -> Vec<TimelineSource> {
        self.results.borrow().keys().map(|id| TimelineSource { id: id.clone(), label: id.clone() }).collect()
    }

    fn has_workspace_providers(&self) -> bool {
        not_a_function("hasWorkspaceProviders")
    }

    fn get_timeline(&self, id: &str, uri: Option<&URI>, options: &TimelineOptions, token_source: CancellationTokenSource) -> Option<TimelineRequest> {
        let result = self.results.borrow().get(&id.to_owned()).cloned().expect("results.get(source)!");
        let request = TimelineRequest { result, options: options.clone(), source: id.to_owned(), token_source, uri: uri.cloned() };
        self.requests.borrow_mut().push(request.clone());
        Some(request)
    }

    fn set_uri(&self, _uri: Option<URI>) -> Promise<()> {
        not_a_function("setUri")
    }

    fn set_editable(&self, _item: TimelineItemIdentifier, _data: Option<EditableData>) {
        not_a_function("setEditable")
    }
}

/// `progressService: upcastPartial<IProgressService>({ withProgress: (_options, task) => task(Progress.None) })`.
struct TestProgressService;

impl IProgressService for TestProgressService {
    fn with_progress(&self, _options: ProgressOptions, task: ProgressTask, _on_did_cancel: Option<Rc<dyn Fn(Option<i32>)>>) -> Promise<()> {
        let progress: Rc<dyn IProgress<ProgressStep>> = Progress::<ProgressStep>::none();
        task(progress)
    }
}

/// `labelService: upcastPartial<ILabelService>({ getUriBasenameLabel: () => 'timeline.txt' })`.
struct StubLabelService;

impl ILabelService for StubLabelService {
    fn get_uri_label(&self, _resource: &URI, _options: &UriLabelOptions) -> String {
        not_a_function("getUriLabel")
    }

    fn get_uri_basename_label(&self, _resource: &URI) -> String {
        "timeline.txt".to_owned()
    }

    fn get_workspace_label(&self, _workspace: &WorkspaceLabelTarget, _options: Option<&WorkspaceLabelOptions>) -> String {
        not_a_function("getWorkspaceLabel")
    }

    fn get_host_label(&self, _scheme: &str, _authority: Option<&str>) -> String {
        not_a_function("getHostLabel")
    }

    fn get_host_tooltip(&self, _scheme: &str, _authority: Option<&str>) -> Option<String> {
        not_a_function("getHostTooltip")
    }

    fn get_separator(&self, _scheme: &str, _authority: Option<&str>) -> LabelSeparator {
        not_a_function("getSeparator")
    }

    fn get_uri_home(&self, _resource: &URI) -> Option<URI> {
        not_a_function("getUriHome")
    }

    fn register_formatter(&self, _formatter: LabelFormatter) -> DisposableRef {
        not_a_function("registerFormatter")
    }

    fn on_did_change_formatters(&self) -> Event<FormatterChangeEvent> {
        Event::none()
    }

    fn register_cached_formatter(&self, _formatter: ResourceLabelFormatter) -> DisposableRef {
        not_a_function("registerCachedFormatter")
    }
}

/// `uriIdentityService: upcastPartial<IUriIdentityService>({ extUri })`.
struct StubUriIdentityService;

impl IUriIdentityService for StubUriIdentityService {
    fn ext_uri(&self) -> je_base::resources::ExtUri {
        ext_uri()
    }

    fn as_canonical_uri(&self, _uri: &URI) -> URI {
        not_a_function("asCanonicalUri")
    }
}

/// O `IEditorService` do construtor: o painel só o usa ao abrir um editor ou ao seguir o editor ativo, o que nenhum caso faz.
struct UnusedEditorService;

impl IEditorService for UnusedEditorService {
    fn visible_editor_panes(&self) -> Vec<Rc<dyn IEditorPane>> {
        not_a_function("visibleEditorPanes")
    }

    fn active_editor(&self) -> Option<EditorInputOrUntyped> {
        not_a_function("activeEditor")
    }

    fn active_text_editor_control(&self) -> Option<ActiveTextEditorControl> {
        not_a_function("activeTextEditorControl")
    }

    fn active_text_editor_language_id(&self) -> Option<String> {
        not_a_function("activeTextEditorLanguageId")
    }

    fn open_editor(&self, _editor: UntypedEditorInput) -> Promise<Option<Rc<dyn IEditorPane>>> {
        not_a_function("openEditor")
    }

    fn find_editors(&self, _resource: &URI, _options: Option<FindEditorOptions>) -> Vec<EditorIdentifier> {
        not_a_function("findEditors")
    }

    fn save(&self, _editors: Vec<EditorIdentifier>, _options: Option<SaveEditorsOptions>) -> Promise<SaveEditorsResult> {
        not_a_function("save")
    }

    fn save_all(&self, _options: Option<SaveAllEditorsOptions>) -> Promise<SaveEditorsResult> {
        not_a_function("saveAll")
    }

    fn get_editors(&self, _order: EditorsOrder) -> Vec<EditorIdentifier> {
        not_a_function("getEditors")
    }

    fn open_editors(&self, _editors: Vec<EditorInputWithOptions>) -> Promise<Vec<Rc<dyn IEditorPane>>> {
        not_a_function("openEditors")
    }

    fn is_visible(&self, _editor: &EditorInputRef) -> bool {
        not_a_function("isVisible")
    }
}

/// O `IContextMenuService` do construtor: nenhum caso abre um menu de contexto.
struct UnusedContextMenuService;

impl IContextMenuService for UnusedContextMenuService {
    fn on_did_show_context_menu(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_hide_context_menu(&self) -> Event<()> {
        Event::none()
    }

    fn show_context_menu(&self, _request: ContextMenuRequest) {
        not_a_function("showContextMenu")
    }
}

/// O menu do título da vista (`ViewMenuActions`): sem ações.
struct EmptyMenu;

impl IDisposable for EmptyMenu {
    fn dispose(&self) {}
}

impl IMenu for EmptyMenu {
    fn on_did_change(&self) -> Event<MenuChangeEvent> {
        Event::none()
    }

    fn get_actions(&self, _options: Option<&MenuActionOptions>) -> Vec<MenuActionGroup> {
        Vec::new()
    }
}

/// `menuService: upcastPartial<IMenuService>({ getMenuActions: () => [] })`, com o `createMenu` do título da vista.
struct StubMenuService;

impl IMenuService for StubMenuService {
    fn create_menu(&self, _id: &MenuId, _context_key_service: Rc<dyn IContextKeyService>, _options: Option<MenuCreateOptions>) -> Rc<dyn IMenu> {
        Rc::new(EmptyMenu)
    }

    fn get_menu_actions(&self, _id: &MenuId, _context_key_service: &dyn IContextKeyService, _options: Option<&MenuActionOptions>) -> Vec<MenuActionGroup> {
        Vec::new()
    }

    fn get_menu_contexts(&self, _id: &MenuId) -> HashSet<String> {
        not_a_function("getMenuContexts")
    }

    fn reset_hidden_states(&self, _menu_ids: Option<&[MenuId]>) {
        not_a_function("resetHiddenStates")
    }
}

/// `viewDescriptorService` do construtor da `ViewPane`: só a localização e o descritor da vista.
struct StubViewDescriptorService;

impl IViewDescriptorService for StubViewDescriptorService {
    fn view_containers(&self) -> Vec<ViewContainerRc> {
        not_a_function("viewContainers")
    }

    fn on_did_change_view_containers(&self) -> Event<ViewContainersChange> {
        Event::none()
    }

    fn get_default_view_container(&self, _location: ViewContainerLocation) -> Option<ViewContainerRc> {
        not_a_function("getDefaultViewContainer")
    }

    fn get_view_container_by_id(&self, _id: &str) -> Option<ViewContainerRc> {
        not_a_function("getViewContainerById")
    }

    fn is_view_container_removed_permanently(&self, _id: &str) -> bool {
        not_a_function("isViewContainerRemovedPermanently")
    }

    fn get_default_view_container_location(&self, _view_container: &ViewContainerRc) -> Option<ViewContainerLocation> {
        not_a_function("getDefaultViewContainerLocation")
    }

    fn get_view_container_location(&self, _view_container: &ViewContainerRc) -> Option<ViewContainerLocation> {
        not_a_function("getViewContainerLocation")
    }

    fn get_view_containers_by_location(&self, _location: ViewContainerLocation) -> Vec<ViewContainerRc> {
        not_a_function("getViewContainersByLocation")
    }

    fn get_view_container_model(&self, _view_container: &ViewContainerRc) -> Rc<dyn IViewContainerModel> {
        not_a_function("getViewContainerModel")
    }

    fn on_did_change_container_location(&self) -> Event<ViewContainerLocationChange> {
        Event::none()
    }

    fn move_view_container_to_location(&self, _view_container: &ViewContainerRc, _location: ViewContainerLocation, _requested_index: Option<usize>, _reason: Option<&str>) {
        not_a_function("moveViewContainerToLocation")
    }

    fn get_view_container_badge_enablement_state(&self, _id: &str) -> bool {
        not_a_function("getViewContainerBadgeEnablementState")
    }

    fn set_view_container_badge_enablement_state(&self, _id: &str, _badges_enabled: bool) {
        not_a_function("setViewContainerBadgeEnablementState")
    }

    fn get_view_descriptor_by_id(&self, id: &str) -> Option<ViewDescriptorRc> {
        let name = LocalizedString { original: "Timeline".to_owned(), value: "Timeline".to_owned() };
        Some(Rc::new(IViewDescriptor::new(id, name)))
    }

    fn get_view_container_by_view_id(&self, _id: &str) -> Option<ViewContainerRc> {
        not_a_function("getViewContainerByViewId")
    }

    fn get_default_container_by_id(&self, _id: &str) -> Option<ViewContainerRc> {
        not_a_function("getDefaultContainerById")
    }

    fn get_view_location_by_id(&self, _id: &str) -> Option<ViewContainerLocation> {
        Some(ViewContainerLocation::Sidebar)
    }

    fn can_move_views(&self) -> bool {
        not_a_function("canMoveViews")
    }

    fn on_did_change_container(&self) -> Event<ViewsContainerChange> {
        Event::none()
    }

    fn move_views_to_container(&self, _views: &[ViewDescriptorRc], _view_container: &ViewContainerRc, _visibility_state: Option<ViewVisibilityState>, _reason: Option<&str>) {
        not_a_function("moveViewsToContainer")
    }

    fn on_did_change_location(&self) -> Event<ViewsLocationChange> {
        Event::none()
    }

    fn move_view_to_location(&self, _view: &ViewDescriptorRc, _location: ViewContainerLocation, _reason: Option<&str>) {
        not_a_function("moveViewToLocation")
    }

    fn reset(&self) {
        not_a_function("reset")
    }
}

/// O tema do `IThemeService` do corpo: vazio, o `renderBody` não roda nestes casos.
struct EmptyColorTheme;

impl IColorTheme for EmptyColorTheme {
    fn get_color(&self, _color: &str, _use_default: Option<bool>) -> Option<Color> {
        None
    }
}

struct EmptyThemeService;

impl IThemeService for EmptyThemeService {
    fn get_color_theme(&self) -> Rc<dyn IColorTheme> {
        Rc::new(EmptyColorTheme)
    }

    fn on_did_color_theme_change(&self) -> Event<Rc<dyn IColorTheme>> {
        Event::none()
    }
}

/// `contextKeyService: upcastPartial<IContextKeyService>({ createOverlay: overlay => { overlays.push(Object.fromEntries(overlay)); ... } })`.
struct OverlayRecordingContextKeyService {
    overlays: Rc<RefCell<Vec<Vec<(String, Dynamic)>>>>,
}

impl IContextKeyService for OverlayRecordingContextKeyService {
    fn on_did_change_context(&self) -> Event<Rc<dyn IContextKeyChangeEvent>> {
        not_a_function("onDidChangeContext")
    }

    fn buffer_change_events(&self, _callback: &dyn Fn()) {
        not_a_function("bufferChangeEvents")
    }

    fn create_key(&self, _key: &str, _default_value: Dynamic) -> Rc<dyn IContextKey> {
        not_a_function("createKey")
    }

    fn context_matches_rules(&self, _rules: Option<&ContextKeyExpression>) -> bool {
        not_a_function("contextMatchesRules")
    }

    fn get_context_key_value(&self, _key: &str) -> Dynamic {
        not_a_function("getContextKeyValue")
    }

    fn create_scoped(&self, _target: Rc<dyn IContextKeyServiceTarget>) -> Rc<dyn IScopedContextKeyService> {
        not_a_function("createScoped")
    }

    fn create_overlay(&self, overlay: Vec<(String, Dynamic)>) -> Rc<dyn IContextKeyService> {
        self.overlays.borrow_mut().push(overlay);
        MockContextKeyService::new()
    }

    fn get_context(&self, _target: Option<Rc<dyn IContextKeyServiceTarget>>) -> Rc<dyn IContext> {
        not_a_function("getContext")
    }

    fn update_parent(&self, _parent_context_key_service: Rc<dyn IContextKeyService>) {
        not_a_function("updateParent")
    }

    fn context_id(&self) -> i64 {
        not_a_function("contextId")
    }

    fn get_context_values_container(&self, _context_id: i64) -> Rc<dyn IContext> {
        not_a_function("getContextValuesContainer")
    }
}

/// `upcastPartial<TimelinePane>({ id: 'timeline' })`.
struct PartialPane;

impl ITimelinePaneCommandsHost for PartialPane {
    fn id(&self) -> String {
        TIMELINE_PANE_ID.to_owned()
    }

    fn reset(&self) {
        not_a_function("reset")
    }

    fn show_workspace_timeline(&self) {
        not_a_function("showWorkspaceTimeline")
    }

    fn toggle_follow_active_editor(&self) -> bool {
        not_a_function("followActiveEditor")
    }

    fn compare_selected(&self) -> Promise<()> {
        not_a_function("compareSelected")
    }
}

/// `const uri = URI.file('/timeline.txt')`.
fn timeline_uri() -> URI {
    uri_file("/timeline.txt")
}

/// O estilo do texto das linhas e da mensagem (o `renderBody` não roda, então ninguém o lê).
fn icon_label_styles() -> IconLabelStyles {
    IconLabelStyles {
        text: HighlightedLabelStyles {
            foreground: Rgba::opaque(0xcc, 0xcc, 0xcc),
            highlight_foreground: None,
            highlight_weight: 400,
            font_size: 13.0,
            font_weight: 400,
            line_height: None,
            italic: false,
            strikethrough: false,
        },
        disabled_foreground: None,
    }
}

/// Os serviços do corpo (`renderBody`), que nenhum caso chama.
fn body_services(context_key_service: Rc<dyn IContextKeyService>, configuration_service: Rc<dyn IConfigurationService>) -> TimelineBodyServices {
    TimelineBodyServices {
        tree: WorkbenchTreeServices { context_key_service, list_service: ListService::new(), configuration_service, default_list_styles: Rc::new(IListStyles::default) },
        theme_service: Rc::new(EmptyThemeService),
        create_hover_delegate: Rc::new(|args: &TimelineHoverDelegateArgs| -> Rc<dyn IHoverDelegate> { get_default_hover_delegate(args.placement) }),
        icon_label_styles: icon_label_styles(),
        input_box_styles: Rc::new(InputBoxStyles::default),
        context_view_provider: None,
        list_override_styles: Rc::new(|_colors: &ListOverrideStyleColors| IListStyles::default()),
    }
}

/// O que o `createPane` devolve (`pane`, `results`, `requests`, `treeCalls`, `setEditable`, `setSelection`, `getLoadMore`).
struct Harness {
    pane: Rc<TimelinePane>,
    uri: URI,
    service: Rc<StubTimelineService>,
    state: Rc<TreeState>,
}

impl Harness {
    /// `treeCalls`, copiado.
    fn tree_calls(&self) -> Vec<String> {
        self.state.tree_calls.borrow().clone()
    }

    /// `treeCalls.length = 0`.
    fn clear_tree_calls(&self) {
        self.state.tree_calls.borrow_mut().clear();
    }

    /// `setSelection(value)`.
    fn set_selection(&self, value: &[TimelineItemRef]) {
        *self.state.selection.borrow_mut() = value.iter().map(|item| Some(TreeElement::Item(item.clone()))).collect();
    }

    /// `getLoadMore()`.
    fn get_load_more(&self) -> Rc<LoadMoreCommand> {
        let children = self.state.children.borrow();
        let element = children.iter().find(|child| child.handle() == LOAD_MORE_HANDLE).expect("o item Load more está na árvore");
        element.load_more_command().cloned().expect("o item Load more é um LoadMoreCommand")
    }

    /// `pane.loadTimelineForSource(source, uri, true)`.
    fn load_timeline_for_source(&self, source: &str) {
        self.pane.load_timeline_for_source(source, Some(&self.uri), true, None, TimelineTriggerKind::Automatic);
    }
}

/// `createPane()`.
fn create_pane(store: &DisposableStore) -> Harness {
    let uri = timeline_uri();
    let service = StubTimelineService::new();
    let state = Rc::new(TreeState::default());

    let configuration_service: Rc<dyn IConfigurationService> = TestConfigurationService::new(None);
    let context_key_service: Rc<dyn IContextKeyService> = MockContextKeyService::new();
    let storage = store.add_rc(InMemoryStorageService::new());
    let storage_service: Rc<dyn StorageService> = Rc::new(storage);
    let lifecycle_service = store.add_rc(TestLifecycleService::new());

    let view_pane_services = ViewPaneServices {
        ui: Ui::new(),
        keybinding_service: Rc::new(NoKeybindingService),
        context_menu_service: Rc::new(UnusedContextMenuService),
        configuration_service: configuration_service.clone(),
        context_key_service: context_key_service.clone(),
        view_descriptor_service: Rc::new(StubViewDescriptorService),
        menu_service: Rc::new(StubMenuService),
        opener_service: Rc::new(RecordingOpener::default()),
        hover_service: get_base_layer_hover_delegate(),
        lifecycle_service,
        accessible_view_information_service: None,
        header_styles: icon_label_styles().text,
        description_foreground: Rgba::default(),
        progress_bar_options: IProgressBarOptions::default(),
        create_toolbar: Rc::new(|_args: ViewPaneToolbarArgs| -> Rc<dyn IViewPaneToolbar> { not_a_function("createToolbar") }),
        create_action_view_item: Rc::new(|_action: &je_base::actions::ActionRef, _options: &IActionViewItemOptions, _menu_as_child: bool| -> Option<Rc<dyn IActionViewItem>> { None }),
        create_welcome_view: Rc::new(|_args: ViewWelcomeViewArgs| -> Rc<dyn IViewWelcomeView> { not_a_function("createWelcomeView") }),
    };

    let pane = TimelinePane::new(
        IViewPaneOptions { id: TIMELINE_PANE_ID.to_owned(), ..IViewPaneOptions::default() },
        TimelinePaneServices {
            view_pane: view_pane_services,
            storage_service,
            editor_service: Rc::new(UnusedEditorService),
            command_service: Rc::new(MockCommandService::default()),
            progress_service: Rc::new(TestProgressService),
            timeline_service: service.clone(),
            label_service: Rc::new(StubLabelService),
            uri_identity_service: Rc::new(StubUriIdentityService),
            extension_service: TestExtensionService::new(),
            body: body_services(context_key_service, configuration_service),
        },
    );

    // `tree`, `isBodyVisible: () => true` e `uri`
    pane.set_body(Rc::new(FakeBody { state: state.clone() }));
    pane.view_pane().set_visible(true);
    pane.set_uri(Some(uri.clone()));

    let cleared = pane.clone();
    store.add(to_disposable(move || cleared.clear(true)));
    store.add_rc(pane.clone());
    Harness { pane, uri, service, state }
}

/// `addPageableProvider(pane, results)`.
async fn add_pageable_provider(harness: &Harness) {
    harness.service.set_result(
        "B",
        Promise::resolved(Some(Timeline { source: "B".to_owned(), items: vec![timeline_item("B", "B|first", "First item", 1.0)], cursor: Some("next".to_owned()) })),
    );
    harness.load_timeline_for_source("B");
    timeout(0).await.unwrap();
}

/// `{ handle, source, label, timestamp }`.
fn timeline_item(source: &str, handle: &str, label: &str, timestamp: f64) -> TimelineItem {
    TimelineItem { handle: handle.to_owned(), source: source.to_owned(), label: label.to_owned(), timestamp, ..TimelineItem::default() }
}

/// `requests.map(request => [request.source, request.options.cursor])`.
fn source_and_cursor(requests: &[TimelineRequest]) -> Vec<(String, Option<String>)> {
    requests.iter().map(|request| (request.source.clone(), request.options.cursor.clone())).collect()
}

/// `new DeferredPromise<Timeline | undefined>()`.
fn deferred() -> DeferredPromise<Option<Timeline>> {
    DeferredPromise::new()
}

// ---- os casos ---------------------------------------------------------------------------------------

#[test]
fn removing_a_pending_provider_releases_pagination_for_a_surviving_provider() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let harness = create_pane(&store);
        add_pageable_provider(&harness).await;
        let pending = deferred();
        harness.service.set_result("A", pending.promise.clone());
        harness.load_timeline_for_source("A");
        let removed_request = harness.service.last_request();

        harness.service.delete_result("A");
        harness.pane.on_providers_changed(&TimelineProvidersChangeEvent { removed: Some(vec!["A".to_owned()]), added: None });
        assert!(!harness.get_load_more().loading());
        // The extension host discards results from a disposed provider.
        pending.complete(None).await.unwrap();
        timeout(0).await.unwrap();

        let load_more = harness.get_load_more();
        assert!(!load_more.loading());
        assert!(removed_request.token_source.token().is_cancellation_requested());
        harness.service.set_result("B", deferred().promise);
        harness.pane.load_more(&load_more);
        assert_eq!(
            source_and_cursor(&harness.service.requests()),
            vec![("B".to_owned(), None), ("A".to_owned(), None), ("B".to_owned(), Some("next".to_owned()))]
        );
    });
}

/// O corpo do `for (const hasStaleItems of [false, true])`.
fn late_result_cannot_affect_a_replacement_provider_request(has_stale_items: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let harness = create_pane(&store);
        add_pageable_provider(&harness).await;
        let old_result = deferred();
        harness.service.set_result("A", old_result.promise.clone());
        harness.load_timeline_for_source("A");
        harness.pane.on_providers_changed(&TimelineProvidersChangeEvent { removed: Some(vec!["A".to_owned()]), added: None });

        let replacement_result = deferred();
        harness.service.set_result("A", replacement_result.promise.clone());
        harness.pane.on_providers_changed(&TimelineProvidersChangeEvent { removed: None, added: Some(vec!["A".to_owned()]) });
        let replacement_request = harness.service.last_request();
        harness.pane.refresh();
        old_result
            .complete(has_stale_items.then(|| Timeline { source: "A".to_owned(), items: vec![timeline_item("A", "A|stale", "Stale item", 3.0)], cursor: None }))
            .await
            .unwrap();
        timeout(0).await.unwrap();

        assert_eq!(harness.pane.pending_request("A").map(|request| request.result.ptr_eq(&replacement_request.result)), Some(true));
        assert!(harness.pane.timeline_for_source("A").is_none());
        assert!(harness.get_load_more().loading());

        replacement_result
            .complete(Some(Timeline { source: "A".to_owned(), items: vec![timeline_item("A", "A|replacement", "Replacement item", 2.0)], cursor: None }))
            .await
            .unwrap();
        timeout(0).await.unwrap();
        let handles = harness.pane.timeline_for_source("A").map(|timeline| timeline.items.borrow().iter().map(|item| item.borrow().handle.clone()).collect::<Vec<_>>());
        assert_eq!(handles, Some(vec!["A|replacement".to_owned()]));
        assert!(!harness.get_load_more().loading());
    });
}

#[test]
fn late_undefined_result_cannot_affect_a_replacement_provider_request() {
    late_result_cannot_affect_a_replacement_provider_request(false);
}

#[test]
fn late_nonempty_result_cannot_affect_a_replacement_provider_request() {
    late_result_cannot_affect_a_replacement_provider_request(true);
}

#[test]
fn re_renders_the_edited_item_and_defers_refreshing_until_editing_ends() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let harness = create_pane(&store);
        add_pageable_provider(&harness).await;
        let editable = TimelineEditable {
            item: TimelineItemIdentifier { source: "B".to_owned(), handle: "B|first".to_owned() },
            data: EditableData {
                validation_message: Rc::new(|_: &str| None),
                placeholder: None,
                starting_value: None,
                on_finish: Rc::new(|_: String, _: bool| Promise::resolved(())),
            },
        };
        let record = |action: &dyn Fn()| {
            harness.clear_tree_calls();
            action();
            harness.tree_calls()
        };

        let start = record(&|| {
            harness.service.set_editable_state(Some(editable.clone()));
            harness.pane.on_editable_changed();
        });
        let cancel = record(&|| {
            harness.service.set_editable_state(None);
            harness.pane.on_editable_changed();
        });
        let refresh_while_editing = record(&|| {
            harness.service.set_editable_state(Some(editable.clone()));
            harness.pane.on_editable_changed();
            harness.pane.refresh();
        });
        let finish = record(&|| {
            harness.service.set_editable_state(None);
            harness.pane.on_editable_changed();
        });

        assert_eq!(start, vec!["rerender B|first"]);
        assert_eq!(cancel, vec!["rerender B|first", "domFocus"]);
        assert_eq!(refresh_while_editing, vec!["rerender B|first"]);
        assert_eq!(finish, vec!["setChildren", "domFocus"]);
    });
}

#[test]
fn explains_why_the_timeline_is_empty() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let harness = create_pane(&guard.store());
    let message_for = |sources: &[&str], excluded_sources: &[&str]| {
        harness.service.clear_results();
        for source in sources {
            harness.service.set_result(source, Promise::resolved(None));
        }
        harness.pane.set_excluded_sources(excluded_sources.iter().map(|source| (*source).to_owned()).collect());
        harness.pane.refresh();
        harness.pane.message()
    };

    let no_providers = message_for(&[], &[]);
    let no_providers_with_stale_filter = message_for(&[], &["git"]);
    let stale_filter_with_other_provider = message_for(&["other"], &["git"]);
    let all_providers_filtered = message_for(&["git"], &["git"]);
    let some_providers_filtered = message_for(&["git", "other"], &["git"]);

    assert_eq!(no_providers.as_deref(), Some("No timeline information was provided. Source Control has not been configured."));
    assert_eq!(no_providers_with_stale_filter.as_deref(), Some("No timeline information was provided. Source Control has not been configured."));
    assert_eq!(stale_filter_with_other_provider.as_deref(), Some("No timeline information was provided. Source Control has not been configured."));
    assert_eq!(all_providers_filtered.as_deref(), Some("All timeline sources have been filtered out. Source Control has not been configured."));
    assert_eq!(some_providers_filtered.as_deref(), Some("No filtered timeline information was provided. Source Control has not been configured."));
}

#[test]
fn reloads_a_changed_timeline_with_a_limit_covering_the_loaded_items() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let harness = create_pane(&store);
        add_pageable_provider(&harness).await;
        harness.service.set_result("B", deferred().promise);

        harness.pane.on_timeline_changed(&TimelineChangeEvent { id: "B".to_owned(), workspace: false, uris: Some(vec![uri_file("/other.txt")]) });
        harness.pane.on_timeline_changed(&TimelineChangeEvent { id: "B".to_owned(), workspace: true, uris: None });
        harness.pane.on_timeline_changed(&TimelineChangeEvent { id: "B".to_owned(), workspace: false, uris: Some(vec![harness.uri.clone()]) });

        let requests: Vec<(String, Option<String>, Option<f64>, TimelineTriggerKind)> = harness
            .service
            .requests()
            .iter()
            .map(|request| (request.source.clone(), request.options.cursor.clone(), request.options.limit, request.options.trigger_kind))
            .collect();
        assert_eq!(
            requests,
            vec![("B".to_owned(), None, Some(20.0), TimelineTriggerKind::Automatic), ("B".to_owned(), None, Some(20.0), TimelineTriggerKind::Automatic)]
        );
    });
}

#[test]
fn reloads_a_source_whose_providers_changed() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let harness = create_pane(&store);
        add_pageable_provider(&harness).await;
        harness.service.set_result("B", deferred().promise);

        harness.pane.on_providers_changed(&TimelineProvidersChangeEvent { removed: Some(vec!["B".to_owned()]), added: Some(vec!["B".to_owned()]) });

        assert_eq!(source_and_cursor(&harness.service.requests()), vec![("B".to_owned(), None), ("B".to_owned(), None)]);
    });
}

#[test]
fn re_renders_the_inline_actions_of_items_entering_or_leaving_a_multi_selection() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let harness = create_pane(&store);
        add_pageable_provider(&harness).await;
        let [first, second, third] = ["B|1", "B|2", "B|3"].map(|handle| -> TimelineItemRef { Rc::new(RefCell::new(timeline_item("B", handle, handle, 1.0))) });
        let record = |selection: &[TimelineItemRef]| {
            harness.clear_tree_calls();
            harness.set_selection(selection);
            harness.pane.on_selection_changed();
            harness.tree_calls()
        };

        let single = record(&[first.clone()]);
        let multiple = record(&[first.clone(), second.clone()]);
        let extended = record(&[first.clone(), second.clone(), third.clone()]);
        let single2 = record(&[third.clone()]);

        assert_eq!(single, Vec::<String>::new());
        assert_eq!(multiple, vec!["rerender B|1", "rerender B|2"]);
        assert_eq!(extended, vec!["rerender B|1", "rerender B|2", "rerender B|3"]);
        assert_eq!(single2, vec!["rerender B|1", "rerender B|2", "rerender B|3"]);
    });
}

#[test]
fn inline_and_context_menus_of_an_item_get_the_same_selection_context() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let uri = timeline_uri();
    let overlays: Rc<RefCell<Vec<Vec<(String, Dynamic)>>>> = Rc::new(RefCell::new(Vec::new()));
    let pane: Rc<dyn ITimelinePaneCommandsHost> = Rc::new(PartialPane);
    let storage = guard.add_rc(InMemoryStorageService::new());
    let storage_service: Rc<dyn StorageService> = Rc::new(storage);
    let commands = guard.add_rc(TimelinePaneCommands::new(
        Rc::downgrade(&pane),
        StubTimelineService::new(),
        storage_service,
        Rc::new(OverlayRecordingContextKeyService { overlays: overlays.clone() }),
        Rc::new(StubMenuService),
    ));
    let item = |handle: &str, resource_uri: Option<&URI>| -> TimelineItemRef {
        Rc::new(RefCell::new(TimelineItem {
            handle: handle.to_owned(),
            source: "B".to_owned(),
            label: handle.to_owned(),
            timestamp: 1.0,
            context_value: Some("commit".to_owned()),
            resource_uri: resource_uri.cloned(),
            ..TimelineItem::default()
        }))
    };
    let selection = vec![item("B|1", Some(&uri)), item("B|2", Some(&uri))];

    commands.get_item_actions(&TreeElement::Item(selection[0].clone()), Some(&selection));
    commands.get_item_context_actions(&TreeElement::Item(selection[0].clone()), Some(&selection));
    commands.get_item_actions(&TreeElement::Item(item("B|3", None)), None);

    let overlay = |multi_selected: bool| -> Vec<(String, Dynamic)> {
        vec![
            ("view".to_owned(), Dynamic::string("timeline")),
            ("timelineItem".to_owned(), Dynamic::string("commit")),
            ("listMultiSelection".to_owned(), Dynamic::Bool(multi_selected)),
            ("listDoubleSelection".to_owned(), Dynamic::Bool(multi_selected)),
            ("timelineCanCompareSelection".to_owned(), Dynamic::Bool(multi_selected)),
        ]
    };
    assert_eq!(*overlays.borrow(), vec![overlay(true), overlay(true), overlay(false)]);
}

#[test]
fn runs_commands_with_the_selected_items_of_the_provider_of_the_item() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let uri = timeline_uri();
        let item = |source: &str, handle: &str| -> TimelineItemRef { Rc::new(RefCell::new(timeline_item(source, handle, handle, 1.0))) };
        let (first, second, other, unselected) = (item("A", "first"), item("A", "second"), item("B", "other"), item("A", "unselected"));
        let calls: Rc<RefCell<Vec<Vec<Dynamic>>>> = Rc::new(RefCell::new(Vec::new()));
        let recorded = calls.clone();
        let action = to_action(ToActionProps {
            id: "test".to_owned(),
            label: "Test".to_owned(),
            tooltip: None,
            enabled: None,
            checked: None,
            class: None,
            alt: None,
            run: Rc::new(move |args: &[Dynamic]| {
                recorded.borrow_mut().push(args.to_vec());
                MaybePromise::Value(Dynamic::Undefined)
            }),
        });
        let to_context = |item: &TimelineItemRef| -> Dynamic {
            let item = item.borrow();
            Dynamic::object([
                ("$mid", Dynamic::from(MarshalledId::TimelineActionContext as i32)),
                ("handle", Dynamic::string(&item.handle)),
                ("source", Dynamic::string(&item.source)),
                ("uri", je_workbench::api::common::ext_host_values::ApiValue::to_dynamic(&uri)),
            ])
        };

        let selection: Rc<RefCell<Vec<TimelineItemRef>>> = Rc::new(RefCell::new(vec![first.clone(), second.clone(), other.clone()]));
        let selected = selection.clone();
        let runner = store.add_rc(TimelineActionRunner::new(Rc::new(move || selected.borrow().iter().map(|item| Some(TreeElement::Item(item.clone()))).collect::<Vec<Option<TreeElement>>>())));
        let context = |item: &TimelineItemRef| Some(timeline_action_context_to_dynamic(&TimelineActionContext { uri: Some(uri.clone()), item: TreeElement::Item(item.clone()) }));
        runner.run(&action, context(&first)).await.unwrap();
        runner.run(&action, context(&unselected)).await.unwrap();
        *selection.borrow_mut() = vec![first.clone()];
        runner.run(&action, context(&first)).await.unwrap();

        let uri_argument = je_workbench::api::common::ext_host_values::ApiValue::to_dynamic(&uri);
        let expected: Vec<Vec<Dynamic>> = vec![
            vec![to_context(&first), uri_argument.clone(), Dynamic::string("A"), Dynamic::array([to_context(&first), to_context(&second)])],
            vec![to_context(&unselected), uri_argument.clone(), Dynamic::string("A"), Dynamic::Undefined],
            vec![to_context(&first), uri_argument, Dynamic::string("A"), Dynamic::Undefined],
        ];
        let calls = calls.borrow();
        assert_eq!(calls.len(), expected.len());
        for (actual, expected) in calls.iter().zip(&expected) {
            assert_eq!(actual.len(), expected.len());
            for (actual, expected) in actual.iter().zip(expected) {
                assert_deep_strict_equal(actual, expected);
            }
        }
    });
}
