//! Porte da interface de `src/vs/workbench/contrib/browserView/common/browserZoomService.ts`: o
//! `IBrowserZoomService` que o `BrowserViewModel` consulta para o zoom de cada visão do navegador integrado.
//!
//! Divergências:
//! - Só a interface, o evento de mudança e a constante `MATCH_WINDOW_ZOOM_LABEL` estão aqui. A classe
//!   `BrowserZoomService` (o mapa persistente por host sobre `IStorageService` e o `IConfigurationService`)
//!   é portada junto do seu próprio teste, `browserZoomService.test.ts`.
//! - `_serviceBrand` não existe: o identificador do serviço é [`I_BROWSER_ZOOM_SERVICE`].
//! - Os índices de zoom são `usize`, índices de `BROWSER_ZOOM_FACTORS`.

use je_base::event::Event;
use je_instantiation::service_id;

service_id!(pub I_BROWSER_ZOOM_SERVICE: dyn IBrowserZoomService = "browserZoomService");

/// Valor especial da configuração de zoom padrão que manda a visão casar dinamicamente o nível de zoom mais
/// próximo do zoom atual da interface do aplicativo.
pub const MATCH_WINDOW_ZOOM_LABEL: &str = "Match Window";

/// `IBrowserZoomChangeEvent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserZoomChangeEvent {
    /// The host (e.g. `"example.com"`) whose zoom changed, or `None` when the global default zoom level
    /// changed.
    pub host: Option<String>,

    /// Whether the change came from an in-memory session.
    /// - `true`: only in-memory views need to react.
    /// - `false`: all views for the host may be affected.
    pub is_in_memory_change: bool,
}

/// Gerencia duas hierarquias de zoom em cascata, independentes, para as visões do navegador integrado:
///
///  Persistent views: `persistent per-host override` ?? `configured default`
///  In-memory views:  `in-memory per-host override`  ?? `configured default`
///
/// As visões em memória nunca veem as sobrescritas persistentes diretamente. Quando um valor persistente muda,
/// ele é copiado para o mapa em memória, e as visões em memória refletem o novo nível na hora. Em sentido
/// contrário, mudanças em memória nunca afetam as visões persistentes.
///
/// Valores por host iguais ao padrão atual são sempre removidos (persistentes e em memória), para a visão
/// acompanhar o padrão dali em diante.
pub trait IBrowserZoomService {
    /// Fired whenever the effective zoom for a host may have changed.
    fn on_did_change_zoom(&self) -> Event<BrowserZoomChangeEvent>;

    /// Returns the effective zoom index for the given host and session type. Pass `host = None` to obtain
    /// only the configured default zoom index.
    fn get_effective_zoom_index(&self, host: Option<&str>, is_in_memory: bool) -> usize;

    /// Set the zoom for a host.
    ///
    /// Persistent: persisted to storage. Also propagated into the in-memory map so in-memory views
    /// immediately reflect the change.
    ///
    /// In-memory: stored in memory only, dropped on restart.
    ///
    /// In both cases, if the value equals the current default, the entry is removed so the view tracks the
    /// default going forward.
    fn set_host_zoom_index(&self, host: &str, zoom_index: usize, is_in_memory: bool);

    /// Notifies the service of the application's current UI zoom factor. Must be called once on startup and
    /// again whenever the window zoom changes. Only relevant when the default zoom level is set to
    /// [`MATCH_WINDOW_ZOOM_LABEL`].
    fn notify_window_zoom_changed(&self, window_zoom_factor: f64);
}
