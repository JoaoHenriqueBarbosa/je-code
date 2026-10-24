//! Porte de `src/vs/workbench/contrib/browserView/common/browserView.ts`: o modelo de uma visão do navegador
//! integrado no workbench ([`BrowserViewModel`]), que espelha o estado do processo principal, e o contrato do
//! serviço do workbench ([`IBrowserViewWorkbenchService`]).
//!
//! Divergências:
//! - Os parâmetros decorados do construtor (`@IBrowserViewWorkbenchService`, `@ITelemetryService`, ...) são
//!   argumentos explícitos de [`BrowserViewModel::new`], na ordem do original.
//! - O `IBrowserViewModel` (interface que só o `BrowserViewModel` implementa) não vira `trait`: o tipo concreto
//!   cumpre o papel e evita uma segunda assinatura por método.
//! - `BrowserEditorInput` (`browserEditorInput.ts`) ainda não foi portado. Por isso ficam fora de
//!   [`IBrowserViewWorkbenchService`] os membros que o citam (`getKnownBrowserViews`, `registerContextualFilter`,
//!   `getContextualBrowserViews`, `registerOpenHandler`, `getOrCreateLazy`, `getPreferredGroup` e
//!   `setRemoteProxyInfo`, este por depender de `ITunnelProxyInfo`), assim como `IBrowserViewContextualFilter`,
//!   `IBrowserViewOpenHandler` e `getAgentBrowserViewsNewestFirst`. `createBrowserView` devolve o
//!   [`BrowserViewModel`] já resolvido, o que `setSharedWithAgent` faz com `input.resolve()`.
//! - `IBrowserViewCDPService` depende dos tipos CDP de `platform/browserView/common/cdp/types.ts`, ainda não
//!   portados, e fica para esse porte.
//! - `IntegratedBrowserOpenSource` é o texto `String` (a união vive em `browserViewTelemetry.ts`).
//! - `parseHistorySnapshot` usa o analisador JSON tolerante de `je_base::json`, que aceita comentários e
//!   vírgula sobrando; o `JSON.parse` do original recusaria esses textos. O valor lido do armazenamento do
//!   próprio navegador é sempre JSON estrito, então a diferença não aparece.
//! - A classificação de telemetria (`IntegratedBrowser*Classification`) não existe: `publicLog2` recebe o
//!   evento como [`Dynamic`].
//! - `_store` do `Disposable` não é exposto pelo porte; o modelo registra um `DisposableStore` próprio para as
//!   assinaturas de armazenamento.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::buffer::VSBuffer;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::json::parse_str;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;
use je_platform::browser_view::browser_history::BrowserHistoryStore;
use je_platform::browser_view::browser_permissions::{BrowserPermissionStore, PermissionCategoryState, SerializedBrowserPermissionsSnapshot};
use je_platform::browser_view::browser_view::{
    BROWSER_ZOOM_FACTORS,BrowserDeviceProfile, BrowserElementCommentsUpdate, BrowserElementSelectionOptions,
    BrowserElementSelectionState, BrowserViewAccessibilitySnapshot, BrowserViewAudience, BrowserViewBounds,
    BrowserViewCaptureScreenshotOptions, BrowserViewCertificateError, BrowserViewDevToolsStateEvent, BrowserViewEditorOpenOptions,
    BrowserViewFaviconChangeEvent, BrowserViewFindInPageOptions, BrowserViewFindInPageResult, BrowserViewFocusEvent, BrowserViewHost,
    BrowserViewKeyDownEvent, BrowserViewLoadError, BrowserViewLoadingEvent, BrowserViewNavigationEvent, BrowserViewOwner,
    BrowserViewPermissionRequestEvent, BrowserViewRect, BrowserViewService, BrowserViewSessionOptions, BrowserViewSessionSelector,
    BrowserViewState, BrowserViewStorageScope, BrowserViewTitleChangeEvent, BrowserViewVisibilityEvent, ElementData,
    is_browser_view_storage_scope_shareable_with_agent, is_in_memory_storage_scope,
};
use je_platform::dialogs::dialogs::{
    BaseDialogOptions, Checkbox, Confirmation, DialogDetail, DialogType, DialogTypeOrSeverity, IDialogService,
};
use je_platform::log::{LogArg, LogMessage, LogService, Logger};
use je_platform::network_filter::network_filter_service::IAgentNetworkFilterService;
use je_platform::sandbox::sandbox_settings_resolution_helper::SandboxNetworkRestrictions;
use je_platform::storage::storage::{StorageScope, StorageService, StorageTarget, StorageValueChangeEvent};
use je_platform::telemetry::telemetry::ITelemetryService;
use je_platform::url::is_localhost_authority;
use je_platform::url::whatwg::{host, try_new_url};
use je_storage::StorageValue;

use super::browser_zoom_service::{BrowserZoomChangeEvent, IBrowserZoomService};

/// `BrowserViewSharingState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserViewSharingState {
    /// Tools are available and the page is shared with the agent.
    Shared,
    /// Tools are available but the page is not shared.
    Available,
    /// The current page cannot be shared because its URL is blocked by network policy.
    BlockedByNetworkPolicy,
    /// Browser tools are disabled: sharing is not possible.
    Unavailable,
}

impl BrowserViewSharingState {
    /// O valor de texto (`'shared'`, `'available'`, `'blockedByNetworkPolicy'` ou `'unavailable'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Available => "available",
            Self::BlockedByNetworkPolicy => "blockedByNetworkPolicy",
            Self::Unavailable => "unavailable",
        }
    }
}

/// `URL.parse(text)`: `None` quando o texto não é uma URL.
fn parse_url(text: &str) -> Option<::url::Url> {
    try_new_url(&JsString::from(text))
}

/// `URL.parse(text)?.host`.
fn url_host(text: &str) -> Option<String> {
    parse_url(text).map(|url| host(&url).to_string_lossy())
}

/// Whether a browser URL belongs to the same destination host as the target URL.
pub fn browser_view_url_matches(candidate_url: Option<&str>, target_url: &str, include_blank: bool) -> bool {
    let Some(target) = parse_url(target_url) else {
        return false;
    };
    let target_host = host(&target).to_string_lossy();
    if target.scheme() != "file" && target_host.is_empty() {
        return false;
    }
    if include_blank && candidate_url.is_none_or(|url| url.is_empty() || url == "about:blank") {
        return true;
    }

    let Some(candidate) = candidate_url.and_then(parse_url) else {
        return false;
    };
    let candidate_host = host(&candidate).to_string_lossy();
    candidate_host == target_host
        || (target.scheme() == "file" && candidate.scheme() == "file")
        || (!candidate_host.is_empty()
            && !target_host.is_empty()
            && (candidate_host.ends_with(&format!(".{target_host}")) || target_host.ends_with(&format!(".{candidate_host}"))))
}

/// Extracts the host from a URL string for zoom tracking purposes.
fn parse_zoom_host(url: &str) -> Option<String> {
    let parsed = parse_url(url)?;
    let parsed_host = host(&parsed).to_string_lossy();
    if parsed_host.is_empty() || (parsed.scheme() != "http" && parsed.scheme() != "https") {
        return None;
    }
    Some(parsed_host)
}

/// `parseHistorySnapshot`: o objeto JSON do texto, ou `None` quando vazio, inválido ou não objeto.
fn parse_history_snapshot(raw: Option<&str>) -> Option<Dynamic> {
    let raw = raw.filter(|text| !text.is_empty())?;
    let mut errors = Vec::new();
    let parsed = parse_str(raw, &mut errors).filter(|_| errors.is_empty())?;
    let value = Dynamic::from_json(&parsed);
    matches!(value, Dynamic::Object(_) | Dynamic::Array(_)).then_some(value)
}

/// `IntegratedBrowserNavigationEvent.navigationType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NavigationType {
    UrlInput,
    SearchInput,
    GoBack,
    GoForward,
    Reload,
}

impl NavigationType {
    fn as_str(self) -> &'static str {
        match self {
            Self::UrlInput => "urlInput",
            Self::SearchInput => "searchInput",
            Self::GoBack => "goBack",
            Self::GoForward => "goForward",
            Self::Reload => "reload",
        }
    }
}

/// To be used in telemetry. This is the source for an address-bar-initiated navigation: whether the user
/// typed a URL or ran a web search. Defaults to `UrlInput` when omitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserNavigationSource {
    UrlInput,
    SearchInput,
}

impl From<BrowserNavigationSource> for NavigationType {
    fn from(source: BrowserNavigationSource) -> Self {
        match source {
            BrowserNavigationSource::UrlInput => Self::UrlInput,
            BrowserNavigationSource::SearchInput => Self::SearchInput,
        }
    }
}

/// `INavigateOptions`: opções de uma navegação iniciada por [`BrowserViewModel::load_url`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NavigateOptions {
    /// Source of the navigation, for telemetry purposes. Defaults to `UrlInput` when omitted.
    pub source: Option<BrowserNavigationSource>,
}

/// `IBrowserEditorViewState`: o estado de visão guardado nas opções do editor ao abrir uma visão.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrowserEditorViewState {
    pub url: Option<String>,
    pub title: Option<String>,
    pub favicon: Option<String>,
}

/// `BrowserViewEditorId`: o editor que renderiza uma página no navegador integrado.
pub const BROWSER_VIEW_EDITOR_ID: &str = "workbench.editor.browser";

/// `IBrowserViewFilterContext`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrowserViewFilterContext {
    /// Restrict visibility to browser storage isolated for this agent session.
    pub sandbox_session_id: Option<String>,
    /// The session *resource* URI string (`session.resource.toString()`) of the relevant session, if any. This
    /// is the same value stored in an agent [`BrowserViewOwner`], not the composite `ISession.sessionId`
    /// (`providerId:resource`).
    pub active_session_id: Option<String>,
}

/// `IBrowserViewWorkbenchCreateOptions`.
#[derive(Debug, Clone)]
pub struct BrowserViewWorkbenchCreateOptions {
    pub sandbox_network_restrictions: Option<SandboxNetworkRestrictions>,
    pub owner: BrowserViewOwner,
    pub session: BrowserViewSessionSelector,
    pub initial_audiences: Option<Vec<BrowserViewAudience>>,
    pub initial_url: Option<String>,
    pub associated_resource: Option<URI>,
    pub open_source: Option<String>,
}

/// Workbench-level service for browser views that provides model-based access to browser views. This service
/// manages browser view models that proxy to the main process browser view service.
pub trait IBrowserViewWorkbenchService {
    /// Returns true if the remote proxy is enabled; i.e. we are in a remote workspace and the setting is
    /// enabled.
    fn will_use_remote_proxy(&self) -> bool;

    /// Fires when the set of known browser views changes, or a model is created for an existing input.
    fn on_did_change_browser_views(&self) -> Event<()>;

    /// Whether sharing browser pages with the agent is currently available (chat enabled, agent mode
    /// enabled, browser tools setting enabled, etc.).
    fn is_sharing_available(&self) -> bool;

    /// Fires when [`Self::is_sharing_available`] changes.
    fn on_did_change_sharing_available(&self) -> Event<bool>;

    /// Creates and resolves a browser view, optionally requesting editor presentation.
    fn create_browser_view(&self, options: BrowserViewWorkbenchCreateOptions, editor_open_options: Option<BrowserViewEditorOpenOptions>) -> Promise<Rc<BrowserViewModel>>;

    /// Creates an unlisted user-owned ephemeral view for embedding outside the browser editor.
    fn create_external_browser_view(&self, initial_url: &str, open_source: Option<&str>) -> Promise<Rc<BrowserViewModel>>;

    /// Clear all storage data for the global browser session.
    fn clear_global_storage(&self) -> Promise<()>;

    /// Clear all storage data for the current workspace browser session.
    fn clear_workspace_storage(&self) -> Promise<()>;
}

service_id!(pub I_BROWSER_VIEW_WORKBENCH_SERVICE: dyn IBrowserViewWorkbenchService = "browserViewWorkbenchService");

/// `BrowserViewEventEmitters` (`{ [K in keyof BrowserViewEventData]: Emitter<BrowserViewEventData[K]> }`): os
/// emissores dos eventos que o processo principal envia a uma visão. O workbench é dono deles, e o modelo
/// apenas os assina, sem passar pelo canal de IPC.
#[derive(Clone)]
pub struct BrowserViewEventEmitters {
    pub on_did_change_area_selection_active: Rc<Emitter<bool>>,
    pub on_did_change_audiences: Rc<Emitter<Vec<BrowserViewAudience>>>,
    pub on_did_change_device_emulation: Rc<Emitter<Option<BrowserDeviceProfile>>>,
    pub on_did_change_dev_tools_state: Rc<Emitter<BrowserViewDevToolsStateEvent>>,
    pub on_did_change_element_selection_state: Rc<Emitter<BrowserElementSelectionState>>,
    pub on_did_change_favicon: Rc<Emitter<BrowserViewFaviconChangeEvent>>,
    pub on_did_change_focus: Rc<Emitter<BrowserViewFocusEvent>>,
    pub on_did_change_loading_state: Rc<Emitter<BrowserViewLoadingEvent>>,
    pub on_did_change_owner: Rc<Emitter<BrowserViewOwner>>,
    pub on_did_change_permissions: Rc<Emitter<SerializedBrowserPermissionsSnapshot>>,
    pub on_did_change_remote_status: Rc<Emitter<bool>>,
    pub on_did_change_title: Rc<Emitter<BrowserViewTitleChangeEvent>>,
    pub on_did_change_visibility: Rc<Emitter<BrowserViewVisibilityEvent>>,
    pub on_did_close: Rc<Emitter<()>>,
    pub on_did_find_in_page: Rc<Emitter<BrowserViewFindInPageResult>>,
    pub on_did_key_command: Rc<Emitter<BrowserViewKeyDownEvent>>,
    pub on_did_navigate: Rc<Emitter<BrowserViewNavigationEvent>>,
    pub on_did_pick_area: Rc<Emitter<Option<BrowserViewRect>>>,
    pub on_did_remove_element_comment: Rc<Emitter<String>>,
    pub on_did_request_permission: Rc<Emitter<BrowserViewPermissionRequestEvent>>,
    pub on_did_select_element: Rc<Emitter<ElementData>>,
}

/// `createBrowserViewEventEmitters`: cria um emissor por evento, todos descartados com o `store`.
pub fn create_browser_view_event_emitters(store: &DisposableStore) -> BrowserViewEventEmitters {
    fn emitter<T: 'static>(store: &DisposableStore) -> Rc<Emitter<T>> {
        store.add_rc(Emitter::new())
    }
    BrowserViewEventEmitters {
        on_did_change_area_selection_active: emitter(store),
        on_did_change_audiences: emitter(store),
        on_did_change_device_emulation: emitter(store),
        on_did_change_dev_tools_state: emitter(store),
        on_did_change_element_selection_state: emitter(store),
        on_did_change_favicon: emitter(store),
        on_did_change_focus: emitter(store),
        on_did_change_loading_state: emitter(store),
        on_did_change_owner: emitter(store),
        on_did_change_permissions: emitter(store),
        on_did_change_remote_status: emitter(store),
        on_did_change_title: emitter(store),
        on_did_change_visibility: emitter(store),
        on_did_close: emitter(store),
        on_did_find_in_page: emitter(store),
        on_did_key_command: emitter(store),
        on_did_navigate: emitter(store),
        on_did_pick_area: emitter(store),
        on_did_remove_element_comment: emitter(store),
        on_did_request_permission: emitter(store),
        on_did_select_element: emitter(store),
    }
}

/// A chave de armazenamento de "não perguntar de novo" do compartilhamento com o agente.
const SHARE_DONT_ASK_KEY: &str = "browserView.shareWithAgent.dontAskAgain";

/// Os campos mutáveis do modelo, espelho do estado do processo principal.
struct ModelState {
    url: String,
    owner: BrowserViewOwner,
    title: String,
    favicon: Option<String>,
    screenshot: Option<VSBuffer>,
    loading: bool,
    focused: bool,
    visible: bool,
    is_dev_tools_open: bool,
    can_go_back: bool,
    can_go_forward: bool,
    error: Option<BrowserViewLoadError>,
    certificate_error: Option<BrowserViewCertificateError>,
    storage_scope: BrowserViewStorageScope,
    is_remote_session: bool,
    is_in_memory: bool,
    zoom_host: Option<String>,
    audiences: Vec<BrowserViewAudience>,
    last_sharing_state: BrowserViewSharingState,
    browser_zoom_index: usize,
    element_selection_state: BrowserElementSelectionState,
    is_area_selection_active: bool,
    device: Option<BrowserDeviceProfile>,
}

/// A browser view model that represents a single browser view instance in the workbench. This model proxies
/// calls to the main process browser view service using its unique ID.
pub struct BrowserViewModel {
    base: Rc<Disposable>,
    store: Rc<DisposableStore>,
    pub sandbox_session_id: Option<String>,
    pub id: String,
    pub host: BrowserViewHost,
    pub associated_resource: Option<URI>,
    pub history: Rc<BrowserHistoryStore>,
    pub permissions: Rc<BrowserPermissionStore>,
    state: RefCell<ModelState>,
    on_did_change_device: Rc<Emitter<Option<BrowserDeviceProfile>>>,
    on_did_change_sharing_state: Rc<Emitter<BrowserViewSharingState>>,
    on_did_change_zoom: Rc<Emitter<()>>,
    on_will_dispose: Rc<Emitter<()>>,
    on_will_navigate: Rc<Emitter<String>>,
    browser_view_service: Rc<dyn BrowserViewService>,
    remote_events: BrowserViewEventEmitters,
    browser_view_workbench_service: Rc<dyn IBrowserViewWorkbenchService>,
    telemetry_service: Rc<dyn ITelemetryService>,
    dialog_service: Rc<dyn IDialogService>,
    storage_service: Rc<dyn StorageService>,
    zoom_service: Rc<dyn IBrowserZoomService>,
    agent_network_filter_service: Rc<dyn IAgentNetworkFilterService>,
    log_service: Rc<dyn LogService>,
}

/// Um `Emitter` registrado no `Disposable` do modelo (`this._register(new Emitter<T>())`).
fn registered_emitter<T: 'static>(base: &Disposable) -> Rc<Emitter<T>> {
    let emitter = Emitter::new();
    base.register(emitter.clone());
    emitter
}

impl BrowserViewModel {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: &str,
        host: BrowserViewHost,
        owner: BrowserViewOwner,
        associated_resource: Option<URI>,
        initial_state: BrowserViewState,
        browser_view_service: Rc<dyn BrowserViewService>,
        remote_events: BrowserViewEventEmitters,
        browser_view_workbench_service: Rc<dyn IBrowserViewWorkbenchService>,
        telemetry_service: Rc<dyn ITelemetryService>,
        dialog_service: Rc<dyn IDialogService>,
        storage_service: Rc<dyn StorageService>,
        zoom_service: Rc<dyn IBrowserZoomService>,
        agent_network_filter_service: Rc<dyn IAgentNetworkFilterService>,
        log_service: Rc<dyn LogService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let store = DisposableStore::new();
        base.register(store.clone());
        let history = BrowserHistoryStore::new(None);
        base.register(history.clone());
        let permissions = BrowserPermissionStore::new();
        base.register(permissions.clone());
        let on_did_change_device = registered_emitter(&base);
        let on_did_change_sharing_state = registered_emitter(&base);
        let on_did_change_zoom = registered_emitter(&base);
        let on_will_dispose = registered_emitter(&base);
        let on_will_navigate = registered_emitter(&base);

        let storage_keys = initial_state.storage_keys.clone();
        let initial_permissions = initial_state.permissions.to_dynamic();
        let state = ModelState {
            url: initial_state.url.clone(),
            owner,
            title: initial_state.title,
            favicon: initial_state.last_favicon,
            screenshot: initial_state.last_screenshot,
            loading: initial_state.loading,
            focused: initial_state.focused,
            visible: initial_state.visible,
            is_dev_tools_open: initial_state.is_dev_tools_open,
            can_go_back: initial_state.can_go_back,
            can_go_forward: initial_state.can_go_forward,
            error: initial_state.last_error,
            certificate_error: initial_state.certificate_error,
            storage_scope: initial_state.storage_scope,
            is_remote_session: initial_state.is_remote_session,
            is_in_memory: is_in_memory_storage_scope(initial_state.storage_scope),
            zoom_host: parse_zoom_host(&initial_state.url),
            audiences: initial_state.audiences,
            last_sharing_state: BrowserViewSharingState::Unavailable,
            browser_zoom_index: initial_state.browser_zoom_index,
            element_selection_state: initial_state.element_selection_state,
            is_area_selection_active: initial_state.is_area_selection_active,
            device: initial_state.device,
        };

        let this = Rc::new(Self {
            base,
            store,
            sandbox_session_id: initial_state.sandbox_session_id,
            id: id.to_owned(),
            host,
            associated_resource,
            history,
            permissions,
            state: RefCell::new(state),
            on_did_change_device,
            on_did_change_sharing_state,
            on_did_change_zoom,
            on_will_dispose,
            on_will_navigate,
            browser_view_service,
            remote_events,
            browser_view_workbench_service,
            telemetry_service,
            dialog_service,
            storage_service,
            zoom_service,
            agent_network_filter_service,
            log_service,
        });
        let sharing_state = this.sharing_state();
        this.state.borrow_mut().last_sharing_state = sharing_state;

        if let Some(entries_key) = &storage_keys.history {
            this.reload_history_entries(entries_key);
            this.watch_storage(entries_key, Self::reload_history_entries);
        }
        if let Some(favicons_key) = &storage_keys.favicons {
            this.reload_history_favicons(favicons_key);
            this.watch_storage(favicons_key, Self::reload_history_favicons);
        }

        // Permissions are synced via browser-view state + a dynamic event rather than storage, so they work
        // for ephemeral sessions (which never persist).
        this.permissions.hydrate(&initial_permissions);
        this.subscribe(this.remote_events.on_did_change_permissions.event(), |this, snapshot| {
            this.permissions.hydrate(&snapshot.to_dynamic());
        });

        // Sync initial zoom
        let (zoom_host, is_in_memory, zoom_index) = {
            let state = this.state.borrow();
            (state.zoom_host.clone(), state.is_in_memory, state.browser_zoom_index)
        };
        let effective_zoom_index = this.zoom_service.get_effective_zoom_index(zoom_host.as_deref(), is_in_memory);
        if effective_zoom_index != zoom_index {
            let log_service = this.log_service.clone();
            let _ = this.set_browser_zoom_index(effective_zoom_index, false).catch(move |error| {
                log_service.warn_args(&LogMessage::Text("[BrowserViewModel] Failed to set initial zoom:".to_owned()), &[LogArg::Error(error)]);
            });
        }

        // Set up state synchronization
        this.subscribe(this.zoom_service.on_did_change_zoom(), |this, event: &BrowserZoomChangeEvent| {
            let (zoom_host, is_in_memory) = {
                let state = this.state.borrow();
                (state.zoom_host.clone(), state.is_in_memory)
            };
            if event.is_in_memory_change && !is_in_memory {
                return;
            }
            if event.host.is_none() || event.host == zoom_host {
                let zoom_index = this.zoom_service.get_effective_zoom_index(zoom_host.as_deref(), is_in_memory);
                let _ = this.set_browser_zoom_index(zoom_index, false).catch(|_| ());
            }
        });

        this.subscribe(this.on_did_navigate(), |this, event| {
            let (zoom_host, is_in_memory) = {
                let mut state = this.state.borrow_mut();
                // Clear favicon on navigation to a different host
                if url_host(&event.url) != url_host(&state.url) {
                    state.favicon = None;
                }
                state.zoom_host = parse_zoom_host(&event.url);
                state.url = event.url.clone();
                state.title = event.title.clone();
                state.can_go_back = event.can_go_back;
                state.can_go_forward = event.can_go_forward;
                state.certificate_error = event.certificate_error.clone();
                (state.zoom_host.clone(), state.is_in_memory)
            };
            this.update_sharing_state();

            // Always forceApply because Chromium resets zoom on cross-origin navigation, and an origin change
            // may not correspond to a host change (e.g. http to https).
            let zoom_index = this.zoom_service.get_effective_zoom_index(zoom_host.as_deref(), is_in_memory);
            let _ = this.set_browser_zoom_index(zoom_index, true);
        });

        this.subscribe(this.on_did_change_loading_state(), |this, event| {
            let mut state = this.state.borrow_mut();
            state.loading = event.loading;
            state.error = event.error.clone();
        });

        this.subscribe(this.on_did_change_dev_tools_state(), |this, event| {
            this.state.borrow_mut().is_dev_tools_open = event.is_dev_tools_open;
        });

        this.subscribe(this.on_did_change_title(), |this, event| {
            this.state.borrow_mut().title = event.title.clone();
        });

        this.subscribe(this.on_did_change_favicon(), |this, event| {
            this.state.borrow_mut().favicon = event.favicon.clone();
        });

        this.subscribe(this.on_did_change_owner(), |this, owner| {
            this.state.borrow_mut().owner = owner.clone();
        });

        this.subscribe(this.on_did_change_focus(), |this, event| {
            this.state.borrow_mut().focused = event.focused;
        });

        this.subscribe(this.on_did_change_visibility(), |this, event| {
            this.state.borrow_mut().visible = event.visible;
        });

        this.subscribe(this.remote_events.on_did_change_device_emulation.event(), |this, device| {
            let changed = {
                let mut state = this.state.borrow_mut();
                let changed = state.device != *device;
                if changed {
                    state.device = device.clone();
                }
                changed
            };
            if changed {
                this.on_did_change_device.fire(device.clone());
            }
        });

        this.subscribe(this.on_did_change_element_selection_state(), |this, new_state| {
            let was_active = this.state.borrow().element_selection_state.active;
            if new_state.active && !was_active {
                this.log_event("integratedBrowser.addElementToChat.start", &[]);
            }
            this.state.borrow_mut().element_selection_state = *new_state;
        });

        this.subscribe(this.on_did_change_area_selection_active(), |this, active| {
            this.state.borrow_mut().is_area_selection_active = *active;
        });

        this.subscribe(this.remote_events.on_did_change_audiences.event(), |this, audiences| {
            this.state.borrow_mut().audiences = audiences.clone();
            this.update_sharing_state();
        });

        this.subscribe(this.browser_view_workbench_service.on_did_change_sharing_available(), |this, _| {
            this.update_sharing_state();
        });

        this.subscribe(this.agent_network_filter_service.on_did_change(), |this, _| this.update_sharing_state());

        this.subscribe(this.on_did_change_remote_status(), |this, is_remote_session| {
            this.state.borrow_mut().is_remote_session = *is_remote_session;
        });

        this
    }

    /// Assina `event` pelo tempo de vida do modelo: o `handler` roda enquanto o modelo não foi descartado.
    fn subscribe<T: 'static>(self: &Rc<Self>, event: Event<T>, handler: impl Fn(&Rc<Self>, &T) + 'static) {
        let weak: Weak<Self> = Rc::downgrade(self);
        self.base.register(event.listen(move |value| {
            if let Some(this) = weak.upgrade() {
                handler(&this, value);
            }
        }));
    }

    /// `this._register(this.storageService.onDidChangeValue(APPLICATION, key, this._store)(() => reload(key)))`.
    fn watch_storage(self: &Rc<Self>, key: &str, reload: fn(&Self, &str)) {
        let weak: Weak<Self> = Rc::downgrade(self);
        let key = key.to_owned();
        let event: Event<StorageValueChangeEvent> = self.storage_service.on_did_change_value(StorageScope::Application, Some(&key), &self.store);
        self.base.register(event.listen(move |_| {
            if let Some(this) = weak.upgrade() {
                reload(&this, &key);
            }
        }));
    }

    pub fn on_did_navigate(&self) -> Event<BrowserViewNavigationEvent> {
        self.remote_events.on_did_navigate.event()
    }
    pub fn on_did_change_loading_state(&self) -> Event<BrowserViewLoadingEvent> {
        self.remote_events.on_did_change_loading_state.event()
    }
    pub fn on_did_change_focus(&self) -> Event<BrowserViewFocusEvent> {
        self.remote_events.on_did_change_focus.event()
    }
    pub fn on_did_change_dev_tools_state(&self) -> Event<BrowserViewDevToolsStateEvent> {
        self.remote_events.on_did_change_dev_tools_state.event()
    }
    pub fn on_did_key_command(&self) -> Event<BrowserViewKeyDownEvent> {
        self.remote_events.on_did_key_command.event()
    }
    pub fn on_did_change_title(&self) -> Event<BrowserViewTitleChangeEvent> {
        self.remote_events.on_did_change_title.event()
    }
    pub fn on_did_change_favicon(&self) -> Event<BrowserViewFaviconChangeEvent> {
        self.remote_events.on_did_change_favicon.event()
    }
    pub fn on_did_change_owner(&self) -> Event<BrowserViewOwner> {
        self.remote_events.on_did_change_owner.event()
    }
    pub fn on_did_find_in_page(&self) -> Event<BrowserViewFindInPageResult> {
        self.remote_events.on_did_find_in_page.event()
    }
    pub fn on_did_change_visibility(&self) -> Event<BrowserViewVisibilityEvent> {
        self.remote_events.on_did_change_visibility.event()
    }
    pub fn on_did_close(&self) -> Event<()> {
        self.remote_events.on_did_close.event()
    }
    pub fn on_did_select_element(&self) -> Event<ElementData> {
        self.remote_events.on_did_select_element.event()
    }
    pub fn on_did_remove_element_comment(&self) -> Event<String> {
        self.remote_events.on_did_remove_element_comment.event()
    }
    pub fn on_did_change_element_selection_state(&self) -> Event<BrowserElementSelectionState> {
        self.remote_events.on_did_change_element_selection_state.event()
    }
    pub fn on_did_pick_area(&self) -> Event<Option<BrowserViewRect>> {
        self.remote_events.on_did_pick_area.event()
    }
    pub fn on_did_change_area_selection_active(&self) -> Event<bool> {
        self.remote_events.on_did_change_area_selection_active.event()
    }
    pub fn on_did_change_remote_status(&self) -> Event<bool> {
        self.remote_events.on_did_change_remote_status.event()
    }
    pub fn on_did_request_permission(&self) -> Event<BrowserViewPermissionRequestEvent> {
        self.remote_events.on_did_request_permission.event()
    }
    pub fn on_did_change_device(&self) -> Event<Option<BrowserDeviceProfile>> {
        self.on_did_change_device.event()
    }
    pub fn on_did_change_sharing_state(&self) -> Event<BrowserViewSharingState> {
        self.on_did_change_sharing_state.event()
    }
    pub fn on_did_change_zoom(&self) -> Event<()> {
        self.on_did_change_zoom.event()
    }
    pub fn on_will_dispose(&self) -> Event<()> {
        self.on_will_dispose.event()
    }
    pub fn on_will_navigate(&self) -> Event<String> {
        self.on_will_navigate.event()
    }

    pub fn url(&self) -> String {
        self.state.borrow().url.clone()
    }
    pub fn owner(&self) -> BrowserViewOwner {
        self.state.borrow().owner.clone()
    }
    pub fn title(&self) -> String {
        self.state.borrow().title.clone()
    }
    pub fn favicon(&self) -> Option<String> {
        self.state.borrow().favicon.clone()
    }
    pub fn loading(&self) -> bool {
        self.state.borrow().loading
    }
    pub fn focused(&self) -> bool {
        self.state.borrow().focused
    }
    pub fn visible(&self) -> bool {
        self.state.borrow().visible
    }
    pub fn is_dev_tools_open(&self) -> bool {
        self.state.borrow().is_dev_tools_open
    }
    pub fn can_go_back(&self) -> bool {
        self.state.borrow().can_go_back
    }
    pub fn can_go_forward(&self) -> bool {
        self.state.borrow().can_go_forward
    }
    pub fn screenshot(&self) -> Option<VSBuffer> {
        self.state.borrow().screenshot.clone()
    }
    pub fn error(&self) -> Option<BrowserViewLoadError> {
        self.state.borrow().error.clone()
    }
    pub fn certificate_error(&self) -> Option<BrowserViewCertificateError> {
        self.state.borrow().certificate_error.clone()
    }
    pub fn storage_scope(&self) -> BrowserViewStorageScope {
        self.state.borrow().storage_scope
    }
    pub fn is_remote_session(&self) -> bool {
        self.state.borrow().is_remote_session
    }
    pub fn sharing_state(&self) -> BrowserViewSharingState {
        if !self.browser_view_workbench_service.is_sharing_available() {
            return BrowserViewSharingState::Unavailable;
        }
        let url = {
            let state = self.state.borrow();
            if state.audiences.iter().any(|audience| matches!(audience, BrowserViewAudience::Agent { .. })) {
                return BrowserViewSharingState::Shared;
            }
            state.url.clone()
        };
        if !self.is_directly_shareable() && !url.is_empty() && !self.agent_network_filter_service.is_uri_allowed(&URI::parse(url.as_str(), false), None) {
            return BrowserViewSharingState::BlockedByNetworkPolicy;
        }
        BrowserViewSharingState::Available
    }
    pub fn is_directly_shareable(&self) -> bool {
        is_browser_view_storage_scope_shareable_with_agent(self.storage_scope(), self.agent_network_filter_service.is_enabled(None))
    }
    pub fn zoom_factor(&self) -> f64 {
        BROWSER_ZOOM_FACTORS[self.state.borrow().browser_zoom_index]
    }
    pub fn can_zoom_in(&self) -> bool {
        self.state.borrow().browser_zoom_index < BROWSER_ZOOM_FACTORS.len() - 1
    }
    pub fn can_zoom_out(&self) -> bool {
        self.state.borrow().browser_zoom_index > 0
    }
    pub fn element_selection_state(&self) -> BrowserElementSelectionState {
        self.state.borrow().element_selection_state
    }
    pub fn is_area_selection_active(&self) -> bool {
        self.state.borrow().is_area_selection_active
    }
    pub fn device(&self) -> Option<BrowserDeviceProfile> {
        self.state.borrow().device.clone()
    }

    pub fn layout(&self, bounds: BrowserViewBounds) -> Promise<()> {
        self.browser_view_service.layout(&self.id, bounds)
    }

    pub fn set_visible(&self, visible: bool) -> Promise<()> {
        self.state.borrow_mut().visible = visible; // Set optimistically so model is in sync immediately
        self.browser_view_service.set_visible(&self.id, visible)
    }

    pub fn load_url(&self, url: &str, options: Option<NavigateOptions>) -> Promise<()> {
        let source = options.and_then(|options| options.source).map_or(NavigationType::UrlInput, NavigationType::from);
        self.log_navigation_telemetry(source, url);
        self.on_will_navigate.fire(url.to_owned());

        // Prepend http:// for bare localhost authorities (e.g. "localhost:3000").
        let is_bare_localhost = url.get(..9).is_some_and(|prefix| prefix.eq_ignore_ascii_case("localhost"))
            && url[9..].chars().next().is_none_or(|next| next == ':' || next == '/');
        let url = if is_bare_localhost || parse_url(url).is_none() {
            // No scheme: default to http://; sites typically upgrade to https://.
            format!("http://{url}")
        } else {
            url.to_owned()
        };

        self.browser_view_service.load_url(&self.id, &url)
    }

    pub fn go_back(&self) -> Promise<()> {
        self.log_navigation_telemetry(NavigationType::GoBack, &self.url());
        self.browser_view_service.go_back(&self.id)
    }

    pub fn go_forward(&self) -> Promise<()> {
        self.log_navigation_telemetry(NavigationType::GoForward, &self.url());
        self.browser_view_service.go_forward(&self.id)
    }

    pub fn reload(&self, hard: Option<bool>) -> Promise<()> {
        self.log_navigation_telemetry(NavigationType::Reload, &self.url());
        self.browser_view_service.reload(&self.id, hard)
    }

    pub fn toggle_dev_tools(&self) -> Promise<()> {
        self.browser_view_service.toggle_dev_tools(&self.id)
    }

    pub fn capture_screenshot(self: &Rc<Self>, options: Option<BrowserViewCaptureScreenshotOptions>) -> Promise<VSBuffer> {
        let weak: Weak<Self> = Rc::downgrade(self);
        self.browser_view_service.capture_screenshot(&self.id, options).then(move |result| {
            // Store full-page screenshots for display in UI as placeholders
            let partial = options.is_some_and(|options| options.screen_rect.is_some() || options.page_rect.is_some() || options.full_page == Some(true));
            if !partial && let Some(this) = weak.upgrade() {
                this.state.borrow_mut().screenshot = Some(result.clone());
            }
            result
        })
    }

    pub fn get_accessibility_snapshot(&self) -> Promise<BrowserViewAccessibilitySnapshot> {
        self.browser_view_service.get_accessibility_snapshot(&self.id, self.host.window_id)
    }

    pub fn focus(&self, force: Option<bool>) -> Promise<()> {
        self.browser_view_service.focus(&self.id, force)
    }

    pub fn find_in_page(&self, text: &str, options: Option<BrowserViewFindInPageOptions>) -> Promise<()> {
        self.browser_view_service.find_in_page(&self.id, text, options)
    }

    pub fn stop_find_in_page(&self, keep_selection: Option<bool>) -> Promise<()> {
        self.browser_view_service.stop_find_in_page(&self.id, keep_selection)
    }

    pub fn get_selected_text(&self) -> Promise<String> {
        self.browser_view_service.get_selected_text(&self.id)
    }

    pub fn clear_storage(&self) -> Promise<()> {
        self.browser_view_service.clear_storage(&self.id)
    }

    pub fn set_owner(&self, owner: BrowserViewOwner) -> Promise<()> {
        self.browser_view_service.set_owner(&self.id, owner)
    }

    pub fn trust_certificate(&self, host: &str, fingerprint: &str) -> Promise<()> {
        self.browser_view_service.trust_certificate(&self.id, host, fingerprint)
    }

    pub fn untrust_certificate(&self, host: &str, fingerprint: &str) -> Promise<()> {
        self.browser_view_service.untrust_certificate(&self.id, host, fingerprint)
    }

    pub fn delete_history(&self, entry_ids: Option<&[f64]>) -> Promise<()> {
        // Mirror locally so the workbench updates immediately; the eventual storage change event from the
        // main-process flush will re-hydrate to the same content.
        match entry_ids {
            None => self.history.clear(),
            Some(ids) => {
                for id in ids {
                    self.history.entries.delete(*id);
                }
            }
        }
        self.browser_view_service.delete_browser_history(&self.id, entry_ids)
    }

    pub fn set_permissions(&self, origin: &str, grants: &[PermissionCategoryState]) -> Promise<()> {
        // Mirror locally so the workbench reflects the decision immediately
        self.permissions.set_many(origin, grants.iter().copied());
        self.browser_view_service.set_permissions(&self.id, origin, grants)
    }

    pub fn select_device(&self, request_id: &str, device_id: Option<&str>) -> Promise<()> {
        self.browser_view_service.select_device(&self.id, request_id, device_id)
    }

    /// `forceApply`: when true, the IPC call is made even if the local cached zoom index already matches the
    /// requested value. Pass true after cross-document navigation because Chromium resets the zoom to its
    /// per-origin default, making the cache stale.
    fn set_browser_zoom_index(self: &Rc<Self>, zoom_index: usize, force_apply: bool) -> Promise<()> {
        let clamped = zoom_index.min(BROWSER_ZOOM_FACTORS.len() - 1);
        {
            let mut state = self.state.borrow_mut();
            if !force_apply && clamped == state.browser_zoom_index {
                return Promise::resolved(());
            }
            state.browser_zoom_index = clamped;
        }
        let weak: Weak<Self> = Rc::downgrade(self);
        self.browser_view_service.set_browser_zoom_index(&self.id, clamped).then(move |_| {
            if let Some(this) = weak.upgrade() {
                this.on_did_change_zoom.fire(());
            }
        })
    }

    /// Grava no serviço de zoom o índice atual da visão para o host da página.
    fn remember_host_zoom(&self, zoom_index: usize) {
        let (zoom_host, is_in_memory) = {
            let state = self.state.borrow();
            (state.zoom_host.clone(), state.is_in_memory)
        };
        if let Some(zoom_host) = zoom_host {
            self.zoom_service.set_host_zoom_index(&zoom_host, zoom_index, is_in_memory);
        }
    }

    pub fn zoom_in(self: &Rc<Self>) -> Promise<()> {
        if !self.can_zoom_in() {
            return Promise::resolved(());
        }
        let next = self.state.borrow().browser_zoom_index + 1;
        let this = self.clone();
        self.set_browser_zoom_index(next, false).then(move |_| {
            let current = this.state.borrow().browser_zoom_index;
            this.remember_host_zoom(current);
        })
    }

    pub fn zoom_out(self: &Rc<Self>) -> Promise<()> {
        if !self.can_zoom_out() {
            return Promise::resolved(());
        }
        let previous = self.state.borrow().browser_zoom_index - 1;
        let this = self.clone();
        self.set_browser_zoom_index(previous, false).then(move |_| {
            let current = this.state.borrow().browser_zoom_index;
            this.remember_host_zoom(current);
        })
    }

    pub fn reset_zoom(self: &Rc<Self>) -> Promise<()> {
        let default_index = self.zoom_service.get_effective_zoom_index(None, false);
        let this = self.clone();
        self.set_browser_zoom_index(default_index, false).then(move |_| this.remember_host_zoom(default_index))
    }

    pub fn get_console_logs(&self) -> Promise<String> {
        self.browser_view_service.get_console_logs(&self.id)
    }

    pub fn toggle_element_selection(&self, enabled: Option<bool>, options: Option<BrowserElementSelectionOptions>) -> Promise<()> {
        self.browser_view_service.toggle_element_selection(&self.id, enabled, options)
    }

    pub fn set_element_comments(&self, update: BrowserElementCommentsUpdate) -> Promise<()> {
        self.browser_view_service.set_element_comments(&self.id, update)
    }

    pub fn toggle_area_selection(&self, enabled: Option<bool>) -> Promise<()> {
        self.browser_view_service.toggle_area_selection(&self.id, enabled)
    }

    pub fn set_device(&self, device: Option<BrowserDeviceProfile>) -> Promise<()> {
        // Update model state optimistically so dependent UI reacts immediately; the echo from the main
        // process is filtered by deep comparison.
        let changed = {
            let mut state = self.state.borrow_mut();
            let changed = state.device != device;
            if changed {
                state.device = device.clone();
            }
            changed
        };
        if changed {
            self.on_did_change_device.fire(device.clone());
        }
        self.browser_view_service.set_device_emulation(&self.id, device)
    }

    /// Resolve para o modelo compartilhado, ou `None` quando o usuário recusou ou o compartilhamento é
    /// impossível. Quando a visão não pode ser compartilhada diretamente, resolve para o modelo da cópia
    /// aberta numa aba nova.
    pub fn set_shared_with_agent(self: &Rc<Self>, shared: bool) -> Promise<Option<Rc<BrowserViewModel>>> {
        let agent = BrowserViewAudience::Agent { session_id: None };
        if !shared {
            let this = self.clone();
            return self.browser_view_service.set_audience(&self.id, agent, false).then(move |_| Some(this));
        }
        let sharing_state = self.sharing_state();
        if sharing_state == BrowserViewSharingState::Shared {
            return Promise::resolved(Some(self.clone()));
        }
        if sharing_state == BrowserViewSharingState::BlockedByNetworkPolicy {
            let uri = URI::parse(self.url().as_str(), false);
            return self
                .dialog_service
                .info(&localize("browserView.shareBlocked.title", "Cannot Share with Agent", &[]), Some(&self.agent_network_filter_service.format_error(&uri)))
                .then(|_| None);
        }
        if !self.is_directly_shareable() {
            return self.share_through_new_tab();
        }

        let stored_choice = self.storage_service.get_boolean(SHARE_DONT_ASK_KEY, StorageScope::Profile, None);

        if stored_choice != Some(true) {
            // First time (or no stored preference): ask.
            let confirmation = Confirmation {
                base: BaseDialogOptions {
                    dialog_type: Some(DialogTypeOrSeverity::Type(DialogType::Question)),
                    title: Some(localize("browserView.shareWithAgent.title", "Share with Agent?", &[])),
                    detail: Some(DialogDetail::Text(localize(
                        "browserView.shareWithAgent.detail",
                        "The agent will be able to read and modify browser content and saved data, including cookies.",
                        &[],
                    ))),
                    checkbox: Some(Checkbox { label: localize("browserView.shareWithAgent.dontAskAgain", "Don't ask again", &[]), checked: Some(false) }),
                    ..BaseDialogOptions::new(localize("browserView.shareWithAgent.message", "Share this browser page with the agent?", &[]))
                },
                primary_button: Some(localize("browserView.shareWithAgent.allow", "&&Allow", &[])),
                cancel_button: Some(localize("browserView.shareWithAgent.deny", "Deny", &[])),
            };
            let this = self.clone();
            return self.dialog_service.confirm(confirmation).then_promise(move |result| {
                // Only persist "don't ask again" if user accepted sharing, so the button doesn't just do nothing.
                if result.confirmed && result.checkbox_checked == Some(true) {
                    this.storage_service.store(SHARE_DONT_ASK_KEY, StorageValue::Bool(result.confirmed), StorageScope::Profile, StorageTarget::User);
                }

                this.log_event(
                    "integratedBrowser.shareWithAgent",
                    &[("shared", Dynamic::Bool(result.confirmed)), ("dontAskAgain", Dynamic::Bool(result.checkbox_checked.unwrap_or(false)))],
                );

                if !result.confirmed {
                    return Promise::resolved(None);
                }
                this.add_agent_audience()
            });
        }
        self.log_event("integratedBrowser.shareWithAgent", &[("shared", Dynamic::Bool(true)), ("dontAskAgain", Dynamic::Bool(true))]);
        self.add_agent_audience()
    }

    /// O ramo de `setSharedWithAgent` para uma visão que não aplica as restrições exigidas: pergunta e abre a
    /// página numa aba nova com armazenamento do agente.
    fn share_through_new_tab(self: &Rc<Self>) -> Promise<Option<Rc<BrowserViewModel>>> {
        let confirmation = Confirmation {
            base: BaseDialogOptions {
                dialog_type: Some(DialogTypeOrSeverity::Type(DialogType::Question)),
                detail: Some(DialogDetail::Text(localize(
                    "browserView.openShareableCopy.detail",
                    "This tab does not enforce the configured policy restrictions required for agent sharing.\n\nOpen the page in a new tab to share it with agents.",
                    &[],
                ))),
                ..BaseDialogOptions::new(localize("browserView.openShareableCopy.message", "Open a New Tab for Sharing?", &[]))
            },
            primary_button: Some(localize("browserView.openShareableCopy.confirm", "&&Open and Share", &[])),
            cancel_button: Some(localize("browserView.openShareableCopy.cancel", "Cancel", &[])),
        };
        let this = self.clone();
        self.dialog_service.confirm(confirmation).then_promise(move |result| {
            this.log_event("integratedBrowser.shareWithAgent", &[("shared", Dynamic::Bool(result.confirmed)), ("dontAskAgain", Dynamic::Bool(false))]);
            if !result.confirmed {
                return Promise::resolved(None);
            }

            this.browser_view_workbench_service
                .create_browser_view(
                    BrowserViewWorkbenchCreateOptions {
                        sandbox_network_restrictions: None,
                        owner: BrowserViewOwner::User,
                        session: BrowserViewSessionSelector::Options(BrowserViewSessionOptions::Agent { affinity: None }),
                        initial_audiences: Some(vec![BrowserViewAudience::Agent { session_id: None }]),
                        initial_url: Some(this.url()),
                        associated_resource: None,
                        open_source: None,
                    },
                    Some(BrowserViewEditorOpenOptions { preserve_focus: Some(false), parent_view_id: Some(this.id.clone()), ..Default::default() }),
                )
                .then(Some)
        })
    }

    /// `setAudience(id, { type: 'agent' }, true)` e o modelo como resultado.
    fn add_agent_audience(self: &Rc<Self>) -> Promise<Option<Rc<BrowserViewModel>>> {
        let this = self.clone();
        self.browser_view_service.set_audience(&self.id, BrowserViewAudience::Agent { session_id: None }, true).then(move |_| Some(this))
    }

    fn update_sharing_state(&self) {
        let sharing_state = self.sharing_state();
        let changed = {
            let mut state = self.state.borrow_mut();
            let changed = sharing_state != state.last_sharing_state;
            if changed {
                state.last_sharing_state = sharing_state;
            }
            changed
        };
        if changed {
            self.on_did_change_sharing_state.fire(sharing_state);
        }
    }

    fn reload_history_entries(&self, key: &str) {
        let raw = self.storage_service.get(key, StorageScope::Application, None);
        self.history.entries.hydrate(parse_history_snapshot(raw.as_deref()).as_ref());
    }

    fn reload_history_favicons(&self, key: &str) {
        let raw = self.storage_service.get(key, StorageScope::Application, None);
        self.history.favicons.hydrate(parse_history_snapshot(raw.as_deref()).as_ref());
    }

    /// `telemetryService.publicLog2(name, data)`.
    fn log_event(&self, name: &str, data: &[(&str, Dynamic)]) {
        self.telemetry_service.public_log2(name, Some(Dynamic::object(data.iter().map(|(key, value)| (*key, value.clone())))));
    }

    /// Log navigation telemetry event.
    fn log_navigation_telemetry(&self, navigation_type: NavigationType, url: &str) {
        let localhost = parse_url(url).is_some_and(|parsed| is_localhost_authority(host(&parsed).as_ref()));
        self.log_event(
            "integratedBrowser.navigation",
            &[("navigationType", Dynamic::string(navigation_type.as_str())), ("isLocalhost", Dynamic::Bool(localhost))],
        );
    }
}

impl IDisposable for BrowserViewModel {
    fn dispose(&self) {
        self.on_will_dispose.fire(());

        // Clean up the browser view when the model is disposed
        let _ = self.browser_view_service.destroy_browser_view(&self.id);

        self.base.dispose();
    }
}
