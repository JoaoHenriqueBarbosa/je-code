//! Porte de `src/vs/platform/update/common/update.ts`: `IUpdate`, a máquina de estados do serviço de
//! atualização (`StateType`, `State` e suas fábricas), `UpdateType`, `DisablementReason` e os contratos
//! `IAutoUpdater` e `IUpdateService`.
//!
//! Decisões de tradução:
//! - Os números do original (`number`) são `f64`, como no JavaScript (bytes, progresso e `timestamp`).
//! - `State` é um enum cujas variantes carregam as structs do original (`Idle`, `Downloading`...); as
//!   fábricas do namespace `State` são funções associadas em minúsculas (`State::idle(...)`), e
//!   `State.Uninitialized` e `State.Cancelling` são as variantes sem dados.
//! - `IUpdateService` é o trait [`UpdateService`] (como `LogService`, o `createDecorator` e o
//!   `_serviceBrand` não têm equivalente: o serviço circula como `Rc<dyn UpdateService>`).
//! - `IAutoUpdater extends Event.NodeEventEmitter`: a parte de `EventEmitter` do Node (`on`,
//!   `removeListener`) fica com quem implementa o trait; aqui só o contrato próprio do auto updater.

use je_base::event::Event;
use je_base::promise::Promise;
use std::rc::Rc;

/// `IUpdate`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Update {
    /// Build commit ID.
    pub version: String,
    /// Product version like 1.2.3.
    pub product_version: Option<String>,
    pub timestamp: Option<f64>,
    pub url: Option<String>,
    pub sha256hash: Option<String>,
}

/// `StateType`. Updates are run as a state machine:
///
/// ```text
///      Uninitialized
///           ↓
///          Idle
///          ↓  ↑
///   Checking for Updates  →  Available for Download
///         ↓                    ↓
///                      ←   Overwriting
///     Downloading              ↑
///                      →      Ready
///         ↓                    ↑
///     Downloaded      →     Updating
/// ```
///
/// - Available: there is an update available for download (linux, darwin on metered connection).
/// - Ready: code will be updated as soon as it restarts (win32, darwin).
/// - Downloaded: there is an update ready to be installed in the background (win32).
/// - Overwriting: a newer update is being downloaded to replace the pending update (darwin).
/// - Cancelling: updates are being disabled at runtime; in-flight/pending work is being torn down before
///   Disabled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StateType {
    Uninitialized,
    Idle,
    Disabled,
    CheckingForUpdates,
    AvailableForDownload,
    Downloading,
    Downloaded,
    Updating,
    Ready,
    Overwriting,
    Cancelling,
    Restarting,
}

impl StateType {
    /// O valor de texto do `const enum` do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Uninitialized => "uninitialized",
            Self::Idle => "idle",
            Self::Disabled => "disabled",
            Self::CheckingForUpdates => "checking for updates",
            Self::AvailableForDownload => "available for download",
            Self::Downloading => "downloading",
            Self::Downloaded => "downloaded",
            Self::Updating => "updating",
            Self::Ready => "ready",
            Self::Overwriting => "overwriting",
            Self::Cancelling => "cancelling",
            Self::Restarting => "restarting",
        }
    }
}

/// `UpdateType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UpdateType {
    Setup,
    Archive,
    Snap,
}

/// `DisablementReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DisablementReason {
    NotBuilt,
    DisabledByEnvironment,
    ManuallyDisabled,
    Policy,
    MissingConfiguration,
    InvalidConfiguration,
    RunningAsAdmin,
}

/// `Disabled`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Disabled {
    pub reason: DisablementReason,
}

/// `Idle`.
#[derive(Clone, Debug, PartialEq)]
pub struct Idle {
    pub update_type: UpdateType,
    pub error: Option<String>,
    pub not_available: Option<bool>,
}

/// `CheckingForUpdates`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckingForUpdates {
    pub explicit: bool,
}

/// `AvailableForDownload`.
#[derive(Clone, Debug, PartialEq)]
pub struct AvailableForDownload {
    pub update: Update,
    pub can_install: Option<bool>,
}

/// `Downloading`.
#[derive(Clone, Debug, PartialEq)]
pub struct Downloading {
    pub update: Option<Update>,
    pub explicit: bool,
    pub overwrite: bool,
    pub downloaded_bytes: Option<f64>,
    pub total_bytes: Option<f64>,
    pub start_time: Option<f64>,
}

/// `Downloaded`.
#[derive(Clone, Debug, PartialEq)]
pub struct Downloaded {
    pub update: Update,
    pub explicit: bool,
    pub overwrite: bool,
}

/// `Updating`.
#[derive(Clone, Debug, PartialEq)]
pub struct Updating {
    pub update: Update,
    pub current_progress: Option<f64>,
    pub max_progress: Option<f64>,
    pub explicit: bool,
}

/// `Ready`.
#[derive(Clone, Debug, PartialEq)]
pub struct Ready {
    pub update: Update,
    pub explicit: bool,
    pub overwrite: bool,
}

/// `Overwriting`.
#[derive(Clone, Debug, PartialEq)]
pub struct Overwriting {
    pub update: Update,
    pub explicit: bool,
}

/// `Restarting`.
#[derive(Clone, Debug, PartialEq)]
pub struct Restarting {
    pub update: Update,
}

/// `State`: a união dos estados do serviço de atualização.
#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Uninitialized,
    Disabled(Disabled),
    Idle(Idle),
    CheckingForUpdates(CheckingForUpdates),
    AvailableForDownload(AvailableForDownload),
    Downloading(Downloading),
    Downloaded(Downloaded),
    Updating(Updating),
    Ready(Ready),
    Overwriting(Overwriting),
    Cancelling,
    Restarting(Restarting),
}

impl State {
    /// O `type` do estado.
    pub fn state_type(&self) -> StateType {
        match self {
            Self::Uninitialized => StateType::Uninitialized,
            Self::Disabled(_) => StateType::Disabled,
            Self::Idle(_) => StateType::Idle,
            Self::CheckingForUpdates(_) => StateType::CheckingForUpdates,
            Self::AvailableForDownload(_) => StateType::AvailableForDownload,
            Self::Downloading(_) => StateType::Downloading,
            Self::Downloaded(_) => StateType::Downloaded,
            Self::Updating(_) => StateType::Updating,
            Self::Ready(_) => StateType::Ready,
            Self::Overwriting(_) => StateType::Overwriting,
            Self::Cancelling => StateType::Cancelling,
            Self::Restarting(_) => StateType::Restarting,
        }
    }

    /// `State.Disabled(reason)`.
    pub fn disabled(reason: DisablementReason) -> Self {
        Self::Disabled(Disabled { reason })
    }

    /// `State.Idle(updateType, error?, notAvailable?)`.
    pub fn idle(update_type: UpdateType, error: Option<String>, not_available: Option<bool>) -> Self {
        Self::Idle(Idle { update_type, error, not_available })
    }

    /// `State.CheckingForUpdates(explicit)`.
    pub fn checking_for_updates(explicit: bool) -> Self {
        Self::CheckingForUpdates(CheckingForUpdates { explicit })
    }

    /// `State.AvailableForDownload(update, canInstall?)`.
    pub fn available_for_download(update: Update, can_install: Option<bool>) -> Self {
        Self::AvailableForDownload(AvailableForDownload { update, can_install })
    }

    /// `State.Downloading(update, explicit, overwrite, downloadedBytes?, totalBytes?, startTime?)`.
    pub fn downloading(
        update: Option<Update>,
        explicit: bool,
        overwrite: bool,
        downloaded_bytes: Option<f64>,
        total_bytes: Option<f64>,
        start_time: Option<f64>,
    ) -> Self {
        Self::Downloading(Downloading { update, explicit, overwrite, downloaded_bytes, total_bytes, start_time })
    }

    /// `State.Downloaded(update, explicit, overwrite)`.
    pub fn downloaded(update: Update, explicit: bool, overwrite: bool) -> Self {
        Self::Downloaded(Downloaded { update, explicit, overwrite })
    }

    /// `State.Updating(update, explicit, currentProgress?, maxProgress?)`.
    pub fn updating(update: Update, explicit: bool, current_progress: Option<f64>, max_progress: Option<f64>) -> Self {
        Self::Updating(Updating { update, explicit, current_progress, max_progress })
    }

    /// `State.Ready(update, explicit, overwrite)`.
    pub fn ready(update: Update, explicit: bool, overwrite: bool) -> Self {
        Self::Ready(Ready { update, explicit, overwrite })
    }

    /// `State.Overwriting(update, explicit)`.
    pub fn overwriting(update: Update, explicit: bool) -> Self {
        Self::Overwriting(Overwriting { update, explicit })
    }

    /// `State.Restarting(update)`.
    pub fn restarting(update: Update) -> Self {
        Self::Restarting(Restarting { update })
    }
}

/// `IAutoUpdater` (sem a parte `NodeEventEmitter`, ver o topo do arquivo).
pub trait AutoUpdater {
    fn set_feed_url(&self, url: &str);
    fn check_for_updates(&self);
    /// `applyUpdate?()`: `None` quando o auto updater não o implementa.
    fn apply_update(&self) -> Option<Promise<()>> {
        None
    }
    fn quit_and_install(&self);
}

/// `IUpdateService`.
pub trait UpdateService {
    fn on_state_change(&self) -> Event<State>;
    fn state(&self) -> State;

    fn check_for_updates(&self, explicit: bool) -> Promise<()>;
    fn download_update(&self, explicit: bool) -> Promise<()>;
    fn apply_update(&self) -> Promise<()>;
    fn quit_and_install(&self) -> Promise<()>;

    /// `@deprecated This method should not be used any more. It will be removed in a future release.`
    fn is_latest_version(&self) -> Promise<Option<bool>>;
    /// `_applySpecificUpdate(packagePath)`.
    fn apply_specific_update(&self, package_path: &str) -> Promise<()>;
    fn set_internal_org(&self, internal_org: Option<&str>) -> Promise<()>;
}

/// O handle compartilhado do serviço de atualização.
pub type UpdateServiceRef = Rc<dyn UpdateService>;
