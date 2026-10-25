//! Porte de `src/vs/platform/browserView/common/browserView.ts`: o contrato do navegador integrado entre o
//! workbench e o processo principal (estado da visão, eventos, criação, escopos de armazenamento, zoom e o
//! serviço de IPC [`BrowserViewService`]), além de quem controla uma visão ([`BrowserViewOwner`]) e a quem
//! ela é concedida ([`BrowserViewAudience`]).
//!
//! Divergências:
//! - `IBrowserViewOwner` (`{ type: 'user' } | { type: 'agent'; sessionId }`) é o `enum` [`BrowserViewOwner`], e
//!   `IBrowserViewAgentAudience`/`IBrowserViewAudience` (único tipo hoje: `'agent'`, com `sessionId` opcional que
//!   vale como curinga) é o `enum` [`BrowserViewAudience`]. O campo `type` das duas uniões é a escolha da
//!   variante, então `first.type === second.type` é o casamento das variantes.
//! - `UriComponents` (`associatedResource`) é [`URI`]; `Record<string, string>` é `IndexMap<String, String>`;
//!   `IntegratedBrowserOpenSource` (união de textos de `browserViewTelemetry.ts`, ainda não portado) é o texto
//!   `String`; e `IBrowserViewState.lastScreenshot` é `Option<VSBuffer>`.
//! - O canal de IPC não existe aqui: `ipcBrowserViewChannelName`, `BrowserViewEventMap`, `BrowserViewChangeEvent`,
//!   `BrowserViewEvent`, `serializeBrowserViewInfo`, `reviveBrowserViewInfo` e o método
//!   `onDynamicBrowserViewEvent` do serviço descrevem a serialização do IPC e ficam fora de [`BrowserViewService`].
//! - `IBrowserViewWindowConfiguration` perde `proxyInfo` (`ITunnelProxyInfo`, de `tunnelProxy.ts`, não portado).
//! - `isBrowserViewAssociatedResourceNavigation` usa o `extUriBiasedIgnorePathCase` do ambiente hospedeiro.
//! - `IBrowserViewState`, `IBrowserViewInfo` e o resto de dados puros derivam `Clone` (e `PartialEq` quando não
//!   carregam `VSBuffer`), porque o original os copia livremente por referência.

use indexmap::IndexMap;
use je_base::buffer::VSBuffer;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::uri::{Field, URI, UriChange};

use super::browser_permissions::{
    BrowserDeviceCandidate, BrowserDeviceType, PermissionCategory, PermissionCategoryState, SerializedBrowserPermissionsSnapshot,
};
use crate::resources::ext_uri_biased_ignore_path_case;
use crate::sandbox::sandbox_settings_resolution_helper::SandboxNetworkRestrictions;

/// `BrowserViewCommandId`: os ids de comando do navegador integrado (`workbench.action.browser.*`).
pub mod browser_view_command_id {
    // Tab management
    pub const OPEN: &str = "workbench.action.browser.open";
    pub const OPEN_FILE: &str = "workbench.action.browser.openFile";
    pub const NEW_TAB: &str = "workbench.action.browser.newTab";
    pub const QUICK_OPEN: &str = "workbench.action.browser.quickOpen";
    pub const OPEN_OR_LIST: &str = "workbench.action.browser.openOrList";
    pub const CLOSE_ALL: &str = "workbench.action.browser.closeAll";
    pub const CLOSE_ALL_IN_GROUP: &str = "workbench.action.browser.closeAllInGroup";

    // Navigation
    pub const GO_BACK: &str = "workbench.action.browser.goBack";
    pub const GO_FORWARD: &str = "workbench.action.browser.goForward";
    pub const RELOAD: &str = "workbench.action.browser.reload";
    pub const HARD_RELOAD: &str = "workbench.action.browser.hardReload";

    // Editor actions
    pub const FOCUS_URL_INPUT: &str = "workbench.action.browser.focusUrlInput";
    pub const OPEN_EXTERNAL: &str = "workbench.action.browser.openExternal";
    pub const OPEN_SETTINGS: &str = "workbench.action.browser.openSettings";

    // Favorites
    pub const TOGGLE_FAVORITE: &str = "workbench.action.browser.toggleFavorite";

    // History
    pub const SHOW_HISTORY: &str = "workbench.action.browser.showHistory";

    // Permissions
    pub const MANAGE_PERMISSIONS: &str = "workbench.action.browser.managePermissions";

    // Chat actions
    pub const ADD_ELEMENT_TO_CHAT: &str = "workbench.action.browser.addElementToChat";
    pub const ADD_ELEMENT_COMMENT_TO_CHAT: &str = "workbench.action.browser.addElementCommentToChat";
    pub const ADD_CONSOLE_LOGS_TO_CHAT: &str = "workbench.action.browser.addConsoleLogsToChat";
    pub const ADD_SCREENSHOT_TO_CHAT: &str = "workbench.action.browser.addScreenshotToChat";
    pub const ADD_AREA_SCREENSHOT_TO_CHAT: &str = "workbench.action.browser.addAreaScreenshotToChat";
    pub const ADD_FULL_PAGE_SCREENSHOT_TO_CHAT: &str = "workbench.action.browser.addFullPageScreenshotToChat";

    // Dev Tools
    pub const TOGGLE_DEV_TOOLS: &str = "workbench.action.browser.toggleDevTools";

    // Storage
    pub const CLEAR_GLOBAL_STORAGE: &str = "workbench.action.browser.clearGlobalStorage";
    pub const CLEAR_WORKSPACE_STORAGE: &str = "workbench.action.browser.clearWorkspaceStorage";
    pub const CLEAR_EPHEMERAL_STORAGE: &str = "workbench.action.browser.clearEphemeralStorage";
    pub const CLEAR_AGENT_STORAGE: &str = "workbench.action.browser.clearAgentStorage";

    // Find in page
    pub const SHOW_FIND: &str = "workbench.action.browser.showFind";
    pub const HIDE_FIND: &str = "workbench.action.browser.hideFind";
    pub const FIND_NEXT: &str = "workbench.action.browser.findNext";
    pub const FIND_PREVIOUS: &str = "workbench.action.browser.findPrevious";
}

/// `IElementAncestor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementAncestor {
    pub tag_name: String,
    pub id: Option<String>,
    pub class_names: Option<Vec<String>>,
}

/// `BrowserElementSelectionMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserElementSelectionMode {
    Select,
    Comment,
}

impl BrowserElementSelectionMode {
    /// O valor de texto (`'select'` ou `'comment'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Select => "select",
            Self::Comment => "comment",
        }
    }
}

/// `IBrowserElementSelectionOptions`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BrowserElementSelectionOptions {
    pub highlight_focused_element: Option<bool>,
    pub continuous: Option<bool>,
    pub mode: Option<BrowserElementSelectionMode>,
}

/// `IBrowserElementSelectionState`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BrowserElementSelectionState {
    pub active: bool,
    pub options: BrowserElementSelectionOptions,
}

/// `{ x, y, width, height }` (`IElementData.bounds`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ElementBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// `{ top, left, width, height }` (`IElementData.dimensions`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ElementDimensions {
    pub top: f64,
    pub left: f64,
    pub width: f64,
    pub height: f64,
}

/// `IElementData`.
#[derive(Debug, Clone, PartialEq)]
pub struct ElementData {
    pub url: Option<String>,
    pub element_id: Option<String>,
    pub comment: Option<String>,
    pub outer_html: String,
    pub computed_style: String,
    pub bounds: ElementBounds,
    pub ancestors: Option<Vec<ElementAncestor>>,
    pub attributes: Option<IndexMap<String, String>>,
    pub computed_styles: Option<IndexMap<String, String>>,
    pub dimensions: Option<ElementDimensions>,
    pub inner_text: Option<String>,
}

/// `IBrowserElementComment`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserElementComment {
    pub element_id: String,
    pub body: String,
}

/// `IBrowserElementCommentsUpdate`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrowserElementCommentsUpdate {
    pub comments: Option<Vec<BrowserElementComment>>,
    pub pending_comment_ids_to_discard: Option<Vec<String>>,
}

/// `IBrowserViewRect`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrowserViewRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// `IBrowserViewPreloadLocalizedStrings`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserViewPreloadLocalizedStrings {
    pub add_comment: String,
    pub add_comment_placeholder: String,
    pub comment_on_selected_element: String,
    pub element_comment: String,
    pub element_comment_with_body: String,
    pub empty_element_comment: String,
    pub remove_comment: String,
    pub remove_element_comment: String,
}

/// `IBrowserViewTheme`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrowserViewTheme {
    pub focus_border: Option<String>,
    pub button_background: Option<String>,
    pub button_foreground: Option<String>,
    pub widget_background: Option<String>,
    pub widget_foreground: Option<String>,
    pub widget_border: Option<String>,
    pub widget_shadow: Option<String>,
    pub contrast_border: Option<String>,
    pub description_foreground: Option<String>,
    pub input_placeholder_foreground: Option<String>,
    pub toolbar_hover_background: Option<String>,
    pub font: Option<String>,
    pub reduced_motion: Option<bool>,
}

/// `IBrowserViewWindowConfiguration`: tudo o que uma janela contribui para as visões que ela possui,
/// enviado como uma unidade só pela janela dona.
#[derive(Debug, Clone, PartialEq)]
pub struct BrowserViewWindowConfiguration {
    /// Theme variables for injected UI.
    pub theme: BrowserViewTheme,
    /// Map of command ID to accelerator label for context menus.
    pub keybindings: IndexMap<String, String>,
    /// Whether AI features are disabled for this window.
    pub ai_features_disabled: Option<bool>,
    /// Maximum number of entries to retain per browser session history.
    pub max_history_entries: Option<f64>,
    /// The window's contribution to the `file://` allowlist used by integrated browser sessions. Main unions
    /// every window's contribution into a process-wide allowlist; entries are dropped when the window is
    /// destroyed. Usually `getTrustedUris()` plus, when the workspace itself is trusted, its workspace folder
    /// paths.
    pub trusted_file_roots: Vec<String>,
    /// Whether Workspace Trust is disabled entirely (`security.workspace.trust.enabled: false`) for this
    /// window. When `true`, every `file://` request is allowed regardless of `trusted_file_roots` since there
    /// is no meaningful notion of an untrusted folder to enforce.
    pub trust_all_files: bool,
}

/// `{ scale }` (`IBrowserViewBounds.emulation`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrowserViewBoundsEmulation {
    pub scale: f64,
}

/// `IBrowserViewBounds`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrowserViewBounds {
    pub window_id: i64,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub zoom_factor: f64,
    pub corner_radius: f64,
    pub emulation: Option<BrowserViewBoundsEmulation>,
}

/// `'jpeg' | 'png'` (`IBrowserViewCaptureScreenshotOptions.format`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserViewScreenshotFormat {
    Jpeg,
    Png,
}

/// `IBrowserViewCaptureScreenshotOptions`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BrowserViewCaptureScreenshotOptions {
    /// JPEG quality from 0-100. Only applies when `format` is `Jpeg`.
    pub quality: Option<f64>,
    /// Encoding for the captured image. Defaults to `Jpeg`. `Png` is lossless (no compression artifacts) at
    /// the cost of a larger buffer.
    pub format: Option<BrowserViewScreenshotFormat>,
    pub screen_rect: Option<BrowserViewRect>,
    pub page_rect: Option<BrowserViewRect>,
    /// When true, capture the full scrollable document, not just the visible viewport. Ignored when
    /// `screen_rect` or `page_rect` is set.
    pub full_page: Option<bool>,
    /// When true, wait for the next compositor frame to be presented before capturing. Use this when the
    /// caller has just torn down an in-page overlay (e.g. the area picker's dashed selection rectangle) that
    /// would otherwise still be present in the GPU surface that `capturePage` reads.
    ///
    /// Adds ~1 frame of latency (bounded by a short timeout fallback), so leave off for captures that don't
    /// follow a DOM teardown.
    pub await_next_paint: Option<bool>,
}

/// `IBrowserViewAccessibilitySnapshot`: texto semântico limitado do Chromium para a acessibilidade do
/// usuário. O `scope` é sempre `'main-frame'`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserViewAccessibilitySnapshot {
    pub text: String,
    pub truncated: bool,
}

/// `IBrowserViewOwner`: identifica quem controla uma visão do navegador.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserViewOwner {
    User,
    Agent { session_id: String },
}

/// `IBrowserViewAudience` (`IBrowserViewAgentAudience`): concede acesso aos agentes que casam. O
/// identificador omitido (`None`) casa com qualquer valor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserViewAudience {
    Agent { session_id: Option<String> },
}

/// `equalsBrowserViewAudience`.
pub fn equals_browser_view_audience(first: &BrowserViewAudience, second: &BrowserViewAudience) -> bool {
    first == second
}

/// `matchesBrowserViewAudience`: se uma audiência satisfaz um padrão cujos identificadores omitidos
/// são curingas.
pub fn matches_browser_view_audience(candidate: &BrowserViewAudience, pattern: &BrowserViewAudience) -> bool {
    let (BrowserViewAudience::Agent { session_id: candidate_session }, BrowserViewAudience::Agent { session_id: pattern_session }) = (candidate, pattern);
    pattern_session.is_none() || pattern_session == candidate_session
}

/// `isBrowserViewAssociatedResourceNavigation`: se o alvo é o recurso associado, ignorando consulta e fragmento.
pub fn is_browser_view_associated_resource_navigation(associated_resource: &URI, target: &str) -> bool {
    let target_resource = URI::parse(target, false);
    let without_query_and_fragment = UriChange { query: Field::Clear, fragment: Field::Clear, ..UriChange::default() };
    ext_uri_biased_ignore_path_case().is_equal(
        Some(&associated_resource.with(&without_query_and_fragment)),
        Some(&target_resource.with(&without_query_and_fragment)),
        false,
    )
}

/// `IBrowserViewHost`: identifica a janela do workbench e a sessão opcional da Agents Window que hospedam
/// uma visão.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserViewHost {
    pub window_id: i64,
    pub session_id: Option<String>,
}

/// `BrowserViewPresentation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserViewPresentation {
    Listed,
    Unlisted,
}

impl BrowserViewPresentation {
    /// O valor de texto (`'listed'` ou `'unlisted'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Listed => "listed",
            Self::Unlisted => "unlisted",
        }
    }
}

/// `IBrowserViewInfo`: resumo de uma visão (estado atual e posse), devolvido pelo serviço principal ao listar
/// ou criar visões.
#[derive(Clone)]
pub struct BrowserViewInfo {
    pub id: String,
    pub host: BrowserViewHost,
    pub owner: BrowserViewOwner,
    pub presentation: BrowserViewPresentation,
    pub associated_resource: Option<URI>,
    pub state: BrowserViewState,
}

/// `{ x, y, width, height }` opcionais (`IBrowserViewEditorOpenOptions.auxiliaryWindow`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AuxiliaryWindowBounds {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub width: Option<f64>,
    pub height: Option<f64>,
}

/// `IBrowserViewEditorOpenOptions`: como o workbench apresenta uma visão nova como editor.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BrowserViewEditorOpenOptions {
    pub preserve_focus: Option<bool>,
    pub background: Option<bool>,
    pub pinned: Option<bool>,
    /// The parent view ID. Used by the workbench to place the new tab in the same editor group.
    pub parent_view_id: Option<String>,
    /// When set, open in an auxiliary (new) window with these bounds.
    pub auxiliary_window: Option<AuxiliaryWindowBounds>,
}

/// `IBrowserViewCreatedEvent`.
#[derive(Clone)]
pub struct BrowserViewCreatedEvent {
    pub info: BrowserViewInfo,
    pub initial_url: Option<String>,
    /// Omitted when the creator does not request an editor.
    pub editor_open_request: Option<BrowserViewEditorOpenOptions>,
}

/// `IBrowserViewCreationContext`: host, posse, armazenamento e acesso inicial de uma visão nova.
#[derive(Debug, Clone)]
pub struct BrowserViewCreationContext {
    pub sandbox_network_restrictions: Option<SandboxNetworkRestrictions>,
    pub host: BrowserViewHost,
    pub owner: BrowserViewOwner,
    pub session: BrowserViewSessionSelector,
    /// Controls whether the workbench registers this view as a normal integrated-browser page.
    pub presentation: Option<BrowserViewPresentation>,
    /// Grants automation clients access before the view is announced to other processes.
    pub initial_audiences: Option<Vec<BrowserViewAudience>>,
}

/// `IBrowserViewCreateOptions`: o contrato completo de criação de uma visão no processo principal.
#[derive(Debug, Clone)]
pub struct BrowserViewCreateOptions {
    pub context: BrowserViewCreationContext,
    pub associated_resource: Option<URI>,
    pub initial_url: Option<String>,
    pub open_source: Option<String>,
}

/// `IBrowserViewStorageKeys`: chaves de `applicationSharedStorage` em que a sessão escreve. Vazio nas sessões
/// efêmeras.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrowserViewStorageKeys {
    pub history: Option<String>,
    pub favicons: Option<String>,
    pub permissions: Option<String>,
}

/// `IBrowserViewState`.
#[derive(Clone)]
pub struct BrowserViewState {
    pub sandbox_session_id: Option<String>,
    pub url: String,
    pub title: String,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub loading: bool,
    pub focused: bool,
    pub visible: bool,
    pub is_dev_tools_open: bool,
    pub last_screenshot: Option<VSBuffer>,
    pub last_favicon: Option<String>,
    pub last_error: Option<BrowserViewLoadError>,
    pub certificate_error: Option<BrowserViewCertificateError>,
    pub storage_scope: BrowserViewStorageScope,
    pub storage_keys: BrowserViewStorageKeys,
    pub permissions: SerializedBrowserPermissionsSnapshot,
    pub browser_zoom_index: usize,
    pub element_selection_state: BrowserElementSelectionState,
    pub is_remote_session: bool,
    pub is_area_selection_active: bool,
    pub device: Option<BrowserDeviceProfile>,
    pub audiences: Vec<BrowserViewAudience>,
}

/// `IBrowserViewNavigationEvent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserViewNavigationEvent {
    pub url: String,
    pub title: String,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub certificate_error: Option<BrowserViewCertificateError>,
}

/// `IBrowserViewLoadingEvent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserViewLoadingEvent {
    pub loading: bool,
    pub error: Option<BrowserViewLoadError>,
}

/// `IBrowserViewLoadError`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserViewLoadError {
    pub url: String,
    pub error_code: i32,
    pub error_description: String,
    pub certificate_error: Option<BrowserViewCertificateError>,
}

/// `IBrowserViewCertificateError`.
#[derive(Debug, Clone, PartialEq)]
pub struct BrowserViewCertificateError {
    pub host: String,
    pub fingerprint: String,
    pub error: String,
    pub url: String,
    pub has_trusted_exception: bool,
    pub issuer_name: String,
    pub subject_name: String,
    pub valid_start: f64,
    pub valid_expiry: f64,
}

impl Eq for BrowserViewCertificateError {}

/// `IBrowserViewFocusEvent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrowserViewFocusEvent {
    pub focused: bool,
}

/// `IBrowserViewVisibilityEvent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrowserViewVisibilityEvent {
    pub visible: bool,
}

/// `IBrowserViewDevToolsStateEvent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrowserViewDevToolsStateEvent {
    pub is_dev_tools_open: bool,
}

/// `IBrowserViewKeyDownEvent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserViewKeyDownEvent {
    pub key: String,
    pub key_code: i32,
    pub code: String,
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub meta_key: bool,
    pub repeat: bool,
}

/// `IBrowserViewTitleChangeEvent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserViewTitleChangeEvent {
    pub title: String,
}

/// `IBrowserViewFaviconChangeEvent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserViewFaviconChangeEvent {
    pub favicon: Option<String>,
}

/// `IBrowserViewPermissionRequestEvent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserViewPermissionRequestEvent {
    pub origin: String,
    pub category: PermissionCategory,
    pub device: Option<BrowserViewDeviceRequest>,
}

/// `IBrowserViewDeviceRequest`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserViewDeviceRequest {
    pub request_id: String,
    pub device_type: BrowserDeviceType,
    pub devices: Vec<BrowserDeviceCandidate>,
}

/// `IBrowserViewFindInPageOptions`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BrowserViewFindInPageOptions {
    pub recompute: Option<bool>,
    pub forward: Option<bool>,
    pub match_case: Option<bool>,
}

/// `IBrowserViewFindInPageResult`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrowserViewFindInPageResult {
    pub active_match_ordinal: f64,
    pub matches: f64,
    pub selection_area: Option<BrowserViewRect>,
    pub final_update: bool,
}

/// `BrowserViewStorageScope`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserViewStorageScope {
    Global,
    Workspace,
    Ephemeral,
    Agent,
}

impl BrowserViewStorageScope {
    /// O valor de texto (`'global'`, `'workspace'`, `'ephemeral'` ou `'agent'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Workspace => "workspace",
            Self::Ephemeral => "ephemeral",
            Self::Agent => "agent",
        }
    }
}

/// `IBrowserViewSessionOptions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserViewSessionOptions {
    Global,
    Workspace,
    Ephemeral,
    /// Views with the same affinity share one in-memory browser session.
    Agent { affinity: Option<String> },
}

/// `isInMemoryStorageScope`.
pub fn is_in_memory_storage_scope(scope: BrowserViewStorageScope) -> bool {
    scope == BrowserViewStorageScope::Ephemeral || scope == BrowserViewStorageScope::Agent
}

/// `isBrowserViewStorageScopeShareableWithAgent`.
pub fn is_browser_view_storage_scope_shareable_with_agent(scope: BrowserViewStorageScope, network_filtering_enabled: bool) -> bool {
    !network_filtering_enabled || scope == BrowserViewStorageScope::Agent
}

/// `BrowserViewSessionSelector` (`string | IBrowserViewSessionOptions`): seleciona um contexto de navegador
/// existente pelo id ou o resolve a partir das opções de armazenamento.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserViewSessionSelector {
    Id(String),
    Options(BrowserViewSessionOptions),
}

/// O retorno de `getAgentBrowserViewCreationDefaults`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentBrowserViewCreationDefaults {
    pub owner: BrowserViewOwner,
    pub initial_audiences: Vec<BrowserViewAudience>,
    pub session: BrowserViewSessionSelector,
}

/// `getAgentBrowserViewCreationDefaults`.
pub fn get_agent_browser_view_creation_defaults(session_id: &str, storage_affinity: Option<&str>) -> AgentBrowserViewCreationDefaults {
    AgentBrowserViewCreationDefaults {
        owner: BrowserViewOwner::Agent { session_id: session_id.to_owned() },
        initial_audiences: vec![BrowserViewAudience::Agent { session_id: None }],
        session: BrowserViewSessionSelector::Options(BrowserViewSessionOptions::Agent { affinity: storage_affinity.map(str::to_owned) }),
    }
}

/// `browserZoomFactors`: níveis de zoom discretos como os do Edge/Chrome. Quando esses navegadores dizem "33%" e
/// "67%" de zoom, querem dizer 33,33...% e 66,66...%.
pub const BROWSER_ZOOM_FACTORS: [f64; 17] = [0.25, 1.0 / 3.0, 0.5, 2.0 / 3.0, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0];

/// `browserZoomDefaultIndex`: o índice do fator `1` em [`BROWSER_ZOOM_FACTORS`].
pub const BROWSER_ZOOM_DEFAULT_INDEX: usize = 7;

/// `Math.round(zoomFactor * 100)` do JavaScript (arredonda a metade para cima).
fn zoom_percent(zoom_factor: f64) -> String {
    format!("{}", (zoom_factor * 100.0 + 0.5).floor())
}

/// `browserZoomLabel`.
pub fn browser_zoom_label(zoom_factor: f64) -> String {
    localize("browserZoomPercent", "{0}%", &[&zoom_percent(zoom_factor)])
}

/// `browserZoomAccessibilityLabel`.
pub fn browser_zoom_accessibility_label(zoom_factor: f64) -> String {
    localize("browserZoomAccessibilityLabel", "Page Zoom: {0}%", &[&zoom_percent(zoom_factor)])
}

/// `IBrowserDeviceProfile`: o perfil de emulação de dispositivo ativo. Campo `None` quer dizer "usar o padrão do
/// hospedeiro" para aquela propriedade.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BrowserDeviceProfile {
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub mobile: Option<bool>,
    pub user_agent: Option<String>,
    pub device_scale_factor: Option<f64>,
}

/// `browserViewIsolatedWorldId`: deve casar com o id do mundo isolado definido em `preload-browserView.ts`.
pub const BROWSER_VIEW_ISOLATED_WORLD_ID: i32 = 999;

/// `IBrowserViewService`.
pub trait BrowserViewService {
    fn set_session_network_restrictions(&self, session_id: &str, restrictions: SandboxNetworkRestrictions) -> Promise<()>;

    /// Read-only accessibility snapshot that does not grant agent access.
    fn get_accessibility_snapshot(&self, id: &str, expected_host_window_id: i64) -> Promise<BrowserViewAccessibilitySnapshot>;

    /// Get all known browser views with their ownership and state information.
    fn get_browser_views(&self, window_id: Option<i64>) -> Promise<Vec<BrowserViewInfo>>;

    /// Get or create a browser view instance. If a view with the given ID already exists, `options` are
    /// ignored.
    fn get_or_create_browser_view(&self, id: &str, options: BrowserViewCreateOptions) -> Promise<BrowserViewInfo>;

    /// Destroy a browser view instance.
    fn destroy_browser_view(&self, id: &str) -> Promise<()>;

    /// Update the owner of an existing browser view.
    fn set_owner(&self, id: &str, owner: BrowserViewOwner) -> Promise<()>;

    /// Get the state of an existing browser view by ID, or reject if it doesn't exist.
    fn get_state(&self, id: &str) -> Promise<BrowserViewState>;

    /// Adds an audience or, when disabled, removes every audience matching it.
    fn set_audience(&self, id: &str, audience: BrowserViewAudience, enabled: bool) -> Promise<()>;

    /// Update the bounds of a browser view.
    fn layout(&self, id: &str, bounds: BrowserViewBounds) -> Promise<()>;

    /// Set the visibility of a browser view.
    fn set_visible(&self, id: &str, visible: bool) -> Promise<()>;

    /// Navigate the browser view to a URL.
    fn load_url(&self, id: &str, url: &str) -> Promise<()>;

    /// Get the current URL of a browser view.
    fn get_url(&self, id: &str) -> Promise<String>;

    /// Go back in navigation history.
    fn go_back(&self, id: &str) -> Promise<()>;

    /// Go forward in navigation history.
    fn go_forward(&self, id: &str) -> Promise<()>;

    /// Reload the current page; `hard` bypasses the cache.
    fn reload(&self, id: &str, hard: Option<bool>) -> Promise<()>;

    /// Toggle developer tools for the browser view.
    fn toggle_dev_tools(&self, id: &str) -> Promise<()>;

    /// Check if the view can go back.
    fn can_go_back(&self, id: &str) -> Promise<bool>;

    /// Check if the view can go forward.
    fn can_go_forward(&self, id: &str) -> Promise<bool>;

    /// Capture a screenshot of the browser view as a buffer.
    fn capture_screenshot(&self, id: &str, options: Option<BrowserViewCaptureScreenshotOptions>) -> Promise<VSBuffer>;

    /// Focus the browser view; `force` focuses even if the view's window is not focused.
    fn focus(&self, id: &str, force: Option<bool>) -> Promise<()>;

    /// Find text in the browser view's page.
    fn find_in_page(&self, id: &str, text: &str, options: Option<BrowserViewFindInPageOptions>) -> Promise<()>;

    /// Stop the find in page session; `keep_selection` keeps the current selection.
    fn stop_find_in_page(&self, id: &str, keep_selection: Option<bool>) -> Promise<()>;

    /// Get the currently selected text in the browser view. Returns immediately with an empty string if the
    /// page is still loading.
    fn get_selected_text(&self, id: &str) -> Promise<String>;

    /// Clear all storage data for the global browser session.
    fn clear_global_storage(&self) -> Promise<()>;

    /// Clear all storage data for a specific workspace browser session.
    fn clear_workspace_storage(&self, workspace_id: &str) -> Promise<()>;

    /// Clear storage data for a specific browser view.
    fn clear_storage(&self, id: &str) -> Promise<()>;

    /// Set the browser zoom index (independent from VS Code zoom).
    fn set_browser_zoom_index(&self, id: &str, zoom_index: usize) -> Promise<()>;

    /// Set or clear the active device profile for a browser view.
    fn set_device_emulation(&self, id: &str, device: Option<BrowserDeviceProfile>) -> Promise<()>;

    /// Trust a certificate for a given host in the browser view's session. The page is reloaded after
    /// trusting.
    fn trust_certificate(&self, id: &str, host: &str, fingerprint: &str) -> Promise<()>;

    /// Revoke trust for a previously trusted certificate. The browser view is closed after revoking.
    fn untrust_certificate(&self, id: &str, host: &str, fingerprint: &str) -> Promise<()>;

    /// Delete entries from this view's session history. Without `entry_ids`, deletes all history.
    fn delete_browser_history(&self, id: &str, entry_ids: Option<&[f64]>) -> Promise<()>;

    /// Record permission decisions for an origin in this view's session. This is the single write API for
    /// permissions: it is used both by the management UI and to answer an outstanding permission prompt.
    /// Recording a decision for a category auto-resolves any pending request for that (origin, category). The
    /// only values ever stored are `allow` and `deny`; a `None` decision clears the saved choice, falling back
    /// to the category default. Changes are persisted immediately.
    fn set_permissions(&self, id: &str, origin: &str, grants: &[PermissionCategoryState]) -> Promise<()>;

    /// Answer an in-progress hardware-device chooser. Pass the chosen `device_id`, or `None` to cancel it.
    fn select_device(&self, id: &str, request_id: &str, device_id: Option<&str>) -> Promise<()>;

    /// Get captured console logs for a browser view as a single string. Console messages are captured from
    /// the moment the view is created.
    fn get_console_logs(&self, id: &str) -> Promise<String>;

    /// Toggle element selection mode in a browser view; `enabled` omitted toggles.
    fn toggle_element_selection(&self, id: &str, enabled: Option<bool>, options: Option<BrowserElementSelectionOptions>) -> Promise<()>;

    /// Synchronize the element comments displayed in a browser view.
    fn set_element_comments(&self, id: &str, update: BrowserElementCommentsUpdate) -> Promise<()>;

    /// Toggle drag-to-select area picking on the top frame of a browser view; `enabled` omitted toggles.
    fn toggle_area_selection(&self, id: &str, enabled: Option<bool>) -> Promise<()>;

    /// Replace the calling window's configuration for the browser views it owns.
    fn update_window_configuration(&self, window_id: i64, config: BrowserViewWindowConfiguration) -> Promise<()>;
}
