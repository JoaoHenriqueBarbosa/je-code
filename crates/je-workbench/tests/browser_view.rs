//! Porte de `src/vs/workbench/contrib/browserView/test/common/browserView.test.ts` (suíte `BrowserViewModel`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso, e `store.add(...)`
//!   vira `guard.add(...)`.
//! - `upcastPartial<T>({ ... })` monta um objeto parcial que quebra ao usar um membro não informado. Aqui cada
//!   serviço é um tipo de teste que implementa o `trait` inteiro: os membros informados no original têm o mesmo
//!   comportamento, e os demais chamam `unreachable!`, o equivalente do acesso a um membro ausente.
//! - `upcastPartial<ITelemetryService>({})`, `<IStorageService>({})` e `<ILogService>({})` viram os serviços nulos
//!   do projeto (`NullTelemetryServiceShape`, `InMemoryStorageService` e `NullLogService`): o caso não os usa.
//! - `Event.None` é `Event::none()`, e `async () => { }` é `Promise::resolved(())`.
//! - `new BrowserViewModel(...)` com parâmetros decorados é `BrowserViewModel::new(...)` com os serviços
//!   explícitos, na mesma ordem.
//! - O nome do modelo do primeiro caso, `` `browser-${storageScope}-${audiences.length}` ``, usa o texto do escopo
//!   (`BrowserViewStorageScope::as_str`).
//! - `assert.deepStrictEqual` de objetos vira `assert_eq!` de tuplas com os mesmos campos, na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::buffer::VSBuffer;
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::browser_view::browser_permissions::{PermissionCategoryState, SerializedBrowserPermissionsSnapshot};
use je_platform::browser_view::browser_view::{
    BROWSER_ZOOM_DEFAULT_INDEX, BrowserDeviceProfile, BrowserElementCommentsUpdate, BrowserElementSelectionOptions,
    BrowserElementSelectionState, BrowserViewAccessibilitySnapshot, BrowserViewAudience, BrowserViewBounds,
    BrowserViewCaptureScreenshotOptions, BrowserViewCreateOptions, BrowserViewFindInPageOptions, BrowserViewHost, BrowserViewInfo,
    BrowserViewNavigationEvent, BrowserViewOwner, BrowserViewLoadingEvent, BrowserViewService, BrowserViewState, BrowserViewStorageKeys,
    BrowserViewStorageScope, BrowserViewTitleChangeEvent, BrowserViewWindowConfiguration,
};
use je_platform::dialogs::dialogs::{
    Confirmation, ConfirmationResultCell, IDialogService, Input, InputResultCell, Prompt, PromptResultCell,
};
use je_platform::log::NullLogService;
use je_platform::network_filter::network_filter_service::IAgentNetworkFilterService;
use je_platform::sandbox::sandbox_settings_resolution_helper::SandboxNetworkRestrictions;
use je_platform::storage::storage::InMemoryStorageService;
use je_platform::telemetry::telemetry_utils::NullTelemetryServiceShape;
use je_workbench::contrib::browser_view::browser_view::{
    BrowserViewEventEmitters, BrowserViewModel, BrowserViewSharingState, BrowserViewWorkbenchCreateOptions, IBrowserViewWorkbenchService,
    create_browser_view_event_emitters,
};
use je_workbench::contrib::browser_view::browser_zoom_service::{BrowserZoomChangeEvent, IBrowserZoomService};

/// O `upcastPartial<IBrowserViewService>` dos casos: só `destroyBrowserView` e `setBrowserZoomIndex` existem.
struct PartialBrowserViewService;

impl BrowserViewService for PartialBrowserViewService {
    fn set_session_network_restrictions(&self, _: &str, _: SandboxNetworkRestrictions) -> Promise<()> { unreachable!() }
    fn get_accessibility_snapshot(&self, _: &str, _: i64) -> Promise<BrowserViewAccessibilitySnapshot> { unreachable!() }
    fn get_browser_views(&self, _: Option<i64>) -> Promise<Vec<BrowserViewInfo>> { unreachable!() }
    fn get_or_create_browser_view(&self, _: &str, _: BrowserViewCreateOptions) -> Promise<BrowserViewInfo> { unreachable!() }
    fn destroy_browser_view(&self, _: &str) -> Promise<()> { Promise::resolved(()) }
    fn set_owner(&self, _: &str, _: BrowserViewOwner) -> Promise<()> { unreachable!() }
    fn get_state(&self, _: &str) -> Promise<BrowserViewState> { unreachable!() }
    fn set_audience(&self, _: &str, _: BrowserViewAudience, _: bool) -> Promise<()> { unreachable!() }
    fn layout(&self, _: &str, _: BrowserViewBounds) -> Promise<()> { unreachable!() }
    fn set_visible(&self, _: &str, _: bool) -> Promise<()> { unreachable!() }
    fn load_url(&self, _: &str, _: &str) -> Promise<()> { unreachable!() }
    fn get_url(&self, _: &str) -> Promise<String> { unreachable!() }
    fn go_back(&self, _: &str) -> Promise<()> { unreachable!() }
    fn go_forward(&self, _: &str) -> Promise<()> { unreachable!() }
    fn reload(&self, _: &str, _: Option<bool>) -> Promise<()> { unreachable!() }
    fn toggle_dev_tools(&self, _: &str) -> Promise<()> { unreachable!() }
    fn can_go_back(&self, _: &str) -> Promise<bool> { unreachable!() }
    fn can_go_forward(&self, _: &str) -> Promise<bool> { unreachable!() }
    fn capture_screenshot(&self, _: &str, _: Option<BrowserViewCaptureScreenshotOptions>) -> Promise<VSBuffer> { unreachable!() }
    fn focus(&self, _: &str, _: Option<bool>) -> Promise<()> { unreachable!() }
    fn find_in_page(&self, _: &str, _: &str, _: Option<BrowserViewFindInPageOptions>) -> Promise<()> { unreachable!() }
    fn stop_find_in_page(&self, _: &str, _: Option<bool>) -> Promise<()> { unreachable!() }
    fn get_selected_text(&self, _: &str) -> Promise<String> { unreachable!() }
    fn clear_global_storage(&self) -> Promise<()> { unreachable!() }
    fn clear_workspace_storage(&self, _: &str) -> Promise<()> { unreachable!() }
    fn clear_storage(&self, _: &str) -> Promise<()> { unreachable!() }
    fn set_browser_zoom_index(&self, _: &str, _: usize) -> Promise<()> { Promise::resolved(()) }
    fn set_device_emulation(&self, _: &str, _: Option<BrowserDeviceProfile>) -> Promise<()> { unreachable!() }
    fn trust_certificate(&self, _: &str, _: &str, _: &str) -> Promise<()> { unreachable!() }
    fn untrust_certificate(&self, _: &str, _: &str, _: &str) -> Promise<()> { unreachable!() }
    fn delete_browser_history(&self, _: &str, _: Option<&[f64]>) -> Promise<()> { unreachable!() }
    fn set_permissions(&self, _: &str, _: &str, _: &[PermissionCategoryState]) -> Promise<()> { unreachable!() }
    fn select_device(&self, _: &str, _: &str, _: Option<&str>) -> Promise<()> { unreachable!() }
    fn get_console_logs(&self, _: &str) -> Promise<String> { unreachable!() }
    fn toggle_element_selection(&self, _: &str, _: Option<bool>, _: Option<BrowserElementSelectionOptions>) -> Promise<()> { unreachable!() }
    fn set_element_comments(&self, _: &str, _: BrowserElementCommentsUpdate) -> Promise<()> { unreachable!() }
    fn toggle_area_selection(&self, _: &str, _: Option<bool>) -> Promise<()> { unreachable!() }
    fn update_window_configuration(&self, _: i64, _: BrowserViewWindowConfiguration) -> Promise<()> { unreachable!() }
}

/// O `upcastPartial<IBrowserViewWorkbenchService>`: só `isSharingAvailable` e `onDidChangeSharingAvailable`.
struct PartialBrowserViewWorkbenchService {
    is_sharing_available: bool,
}

impl IBrowserViewWorkbenchService for PartialBrowserViewWorkbenchService {
    fn will_use_remote_proxy(&self) -> bool { unreachable!() }
    fn on_did_change_browser_views(&self) -> Event<()> { unreachable!() }
    fn is_sharing_available(&self) -> bool { self.is_sharing_available }
    fn on_did_change_sharing_available(&self) -> Event<bool> { Event::none() }
    fn create_browser_view(&self, _: BrowserViewWorkbenchCreateOptions, _: Option<je_platform::browser_view::browser_view::BrowserViewEditorOpenOptions>) -> Promise<Rc<BrowserViewModel>> { unreachable!() }
    fn create_external_browser_view(&self, _: &str, _: Option<&str>) -> Promise<Rc<BrowserViewModel>> { unreachable!() }
    fn clear_global_storage(&self) -> Promise<()> { unreachable!() }
    fn clear_workspace_storage(&self) -> Promise<()> { unreachable!() }
}

/// O `upcastPartial<IAgentNetworkFilterService>`: `isEnabled`, `isUriAllowed` (quando informado) e `onDidChange`.
struct PartialAgentNetworkFilterService {
    is_enabled: bool,
    is_uri_allowed: Option<bool>,
}

impl IAgentNetworkFilterService for PartialAgentNetworkFilterService {
    fn is_uri_allowed(&self, _: &URI, _: Option<&SandboxNetworkRestrictions>) -> bool {
        self.is_uri_allowed.expect("isUriAllowed não foi informado")
    }
    fn is_enabled(&self, _: Option<&SandboxNetworkRestrictions>) -> bool { self.is_enabled }
    fn format_error(&self, _: &URI) -> String { unreachable!() }
    fn on_did_change(&self) -> Event<()> { Event::none() }
}

/// O `upcastPartial<IBrowserZoomService>`: `getEffectiveZoomIndex` e `onDidChangeZoom`.
struct PartialBrowserZoomService;

impl IBrowserZoomService for PartialBrowserZoomService {
    fn on_did_change_zoom(&self) -> Event<BrowserZoomChangeEvent> { Event::none() }
    fn get_effective_zoom_index(&self, _: Option<&str>, _: bool) -> usize { BROWSER_ZOOM_DEFAULT_INDEX }
    fn set_host_zoom_index(&self, _: &str, _: usize, _: bool) { unreachable!() }
    fn notify_window_zoom_changed(&self, _: f64) { unreachable!() }
}

/// O `upcastPartial<IDialogService>({})`: nenhum membro existe.
struct PartialDialogService;

impl IDialogService for PartialDialogService {
    fn on_will_show_dialog(&self) -> Event<()> { unreachable!() }
    fn on_did_show_dialog(&self) -> Event<()> { unreachable!() }
    fn confirm(&self, _: Confirmation) -> Promise<ConfirmationResultCell> { unreachable!() }
    fn prompt(&self, _: Prompt) -> Promise<PromptResultCell> { unreachable!() }
    fn input(&self, _: Input) -> Promise<InputResultCell> { unreachable!() }
    fn info(&self, _: &str, _: Option<&str>) -> Promise<()> { unreachable!() }
    fn warn(&self, _: &str, _: Option<&str>) -> Promise<()> { unreachable!() }
    fn error(&self, _: &str, _: Option<&str>) -> Promise<()> { unreachable!() }
    fn about(&self) -> Promise<()> { unreachable!() }
}

fn create_initial_state(storage_scope: BrowserViewStorageScope, audiences: Vec<BrowserViewAudience>) -> BrowserViewState {
    BrowserViewState {
        sandbox_session_id: None,
        url: "https://blocked.example.com/".to_owned(),
        title: String::new(),
        can_go_back: false,
        can_go_forward: false,
        loading: false,
        focused: false,
        visible: false,
        is_dev_tools_open: false,
        last_screenshot: None,
        last_favicon: None,
        last_error: None,
        certificate_error: None,
        storage_scope,
        storage_keys: BrowserViewStorageKeys::default(),
        permissions: SerializedBrowserPermissionsSnapshot::default(),
        browser_zoom_index: BROWSER_ZOOM_DEFAULT_INDEX,
        element_selection_state: BrowserElementSelectionState { active: false, options: BrowserElementSelectionOptions::default() },
        is_remote_session: false,
        is_area_selection_active: false,
        device: None,
        audiences,
    }
}

/// `new BrowserViewModel(id, { windowId: 1 }, { type: 'user' }, undefined, initialState, ...)`.
fn new_model(
    id: &str,
    initial_state: BrowserViewState,
    emitters: BrowserViewEventEmitters,
    workbench_service: PartialBrowserViewWorkbenchService,
    agent_network_filter_service: PartialAgentNetworkFilterService,
) -> Rc<BrowserViewModel> {
    BrowserViewModel::new(
        id,
        BrowserViewHost { window_id: 1, session_id: None },
        BrowserViewOwner::User,
        None,
        initial_state,
        Rc::new(PartialBrowserViewService),
        emitters,
        Rc::new(workbench_service),
        Rc::new(NullTelemetryServiceShape),
        Rc::new(PartialDialogService),
        Rc::new(InMemoryStorageService::new()),
        Rc::new(PartialBrowserZoomService),
        Rc::new(agent_network_filter_service),
        NullLogService::shared(),
    )
}

#[test]
fn only_blocks_disallowed_pages_that_cannot_be_shared_directly() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();

    let create_model = |storage_scope: BrowserViewStorageScope, audiences: Vec<BrowserViewAudience>| {
        let id = format!("browser-{}-{}", storage_scope.as_str(), audiences.len());
        let emitters_store = DisposableStore::new();
        guard.add(emitters_store.clone());
        let model = new_model(
            &id,
            create_initial_state(storage_scope, audiences),
            create_browser_view_event_emitters(&emitters_store),
            PartialBrowserViewWorkbenchService { is_sharing_available: true },
            PartialAgentNetworkFilterService { is_enabled: true, is_uri_allowed: Some(false) },
        );
        guard.add(model.clone());
        model
    };

    assert_eq!(
        (
            create_model(BrowserViewStorageScope::Workspace, vec![BrowserViewAudience::Agent { session_id: None }]).sharing_state(),
            create_model(BrowserViewStorageScope::Workspace, vec![]).sharing_state(),
            create_model(BrowserViewStorageScope::Agent, vec![]).sharing_state(),
        ),
        (BrowserViewSharingState::Shared, BrowserViewSharingState::BlockedByNetworkPolicy, BrowserViewSharingState::Available)
    );
}

#[test]
fn mirrors_workbench_owned_events_before_notifying_local_consumers_without_subscribing_to_ipc() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();

    let event_store = DisposableStore::new();
    guard.add(event_store.clone());
    let emitters = create_browser_view_event_emitters(&event_store);
    let model = new_model(
        "page",
        create_initial_state(BrowserViewStorageScope::Global, vec![]),
        emitters.clone(),
        PartialBrowserViewWorkbenchService { is_sharing_available: true },
        PartialAgentNetworkFilterService { is_enabled: false, is_uri_allowed: None },
    );
    guard.add(model.clone());
    let disposing_store = event_store.clone();
    event_store.add(model.on_will_dispose().once().listen(move |_| disposing_store.dispose()));
    let received: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let log = received.clone();
    let removed = guard.add(model.on_did_change_title().listen(move |_| log.borrow_mut().push("removed".to_owned())));
    removed.dispose();
    let (log, observed) = (received.clone(), model.clone());
    guard.add(model.on_did_navigate().listen(move |_| log.borrow_mut().push(observed.url())));
    let (log, observed) = (received.clone(), model.clone());
    guard.add(model.on_did_change_title().listen(move |_| log.borrow_mut().push(observed.title())));
    let (log, observed) = (received.clone(), model.clone());
    guard.add(model.on_did_change_loading_state().listen(move |_| log.borrow_mut().push(format!("loading:{}", observed.loading()))));
    let log = received.clone();
    guard.add(model.on_did_close().listen(move |_| log.borrow_mut().push("closed".to_owned())));
    let log = received.clone();
    guard.add(model.on_did_change_focus().listen(move |_| log.borrow_mut().push("unexpected focus".to_owned())));
    let log = received.clone();
    guard.add(model.on_did_pick_area().listen(move |_| log.borrow_mut().push("unexpected area".to_owned())));
    emitters.on_did_navigate.fire(BrowserViewNavigationEvent {
        url: "https://example.com/".to_owned(),
        title: "initial".to_owned(),
        can_go_back: true,
        can_go_forward: false,
        certificate_error: None,
    });
    emitters.on_did_change_title.fire(BrowserViewTitleChangeEvent { title: "Example".to_owned() });
    emitters.on_did_change_loading_state.fire(BrowserViewLoadingEvent { loading: false, error: None });
    emitters.on_did_close.fire(());
    model.dispose();
    emitters.on_did_change_title.fire(BrowserViewTitleChangeEvent { title: "after model disposal".to_owned() });
    assert_eq!(
        (received.borrow().clone(), model.can_go_back(), model.title()),
        (vec!["https://example.com/".to_owned(), "Example".to_owned(), "loading:false".to_owned(), "closed".to_owned()], true, "Example".to_owned())
    );
}
