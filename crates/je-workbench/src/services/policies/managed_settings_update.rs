//! Porte de `src/vs/workbench/services/policies/common/managedSettingsUpdate.ts`: a apresentação do bloqueio de cliente
//! desatualizado ("sua organização exige uma atualização para usar os recursos de IA"). Não avalia nem aplica política.
//!
//! Divergências:
//! - `ManagedSettingsUpdateRequiredContext` é [`managed_settings_update_required_context`], criada (e registrada) na primeira
//!   chamada, como as chaves de `common::contextkeys`.
//! - `IManagedSettingsUpdateService.updateInfo` é um [`IObservable`] de `Option<IManagedSettingsUpdateInfo>`.
//! - `localize(...)` é o texto padrão (sem tradução ativa os dois coincidem).

use je_base::nls::localize;
use je_base::observable::StrictEquals;
use je_base::observable::base::IObservable;
use je_instantiation::service_id;
use je_platform::contextkey::contextkey::{ContextKeyMeta, RawContextKey};
use je_platform::product::IProductService;
use je_platform::update::update::{DisablementReason, State};

use crate::services::accounts::managed_settings::IManagedSettingsCompatibilityError;

/// `ManagedSettingsUpdateRequiredContext`.
pub fn managed_settings_update_required_context() -> RawContextKey {
    thread_local! { static KEY: RawContextKey = RawContextKey::new("managedSettingsUpdateRequired", false, ContextKeyMeta::None); }
    KEY.with(Clone::clone)
}

/// `MANAGED_SETTINGS_UPDATE_VIEW_ID`.
pub const MANAGED_SETTINGS_UPDATE_VIEW_ID: &str = "workbench.panel.chat.updateRequired";

/// `{ label, href }`, o `action` de [`IManagedSettingsUpdateInfo`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IManagedSettingsUpdateAction {
    pub label: String,
    pub href: String,
}

/// `IManagedSettingsUpdateInfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IManagedSettingsUpdateInfo {
    pub title: String,
    pub message: String,
    pub detail: Option<String>,
    pub action: Option<IManagedSettingsUpdateAction>,
    pub update_status: Option<String>,
}

impl StrictEquals for IManagedSettingsUpdateInfo {
    fn strict_equals(&self, other: &Self) -> bool {
        self == other
    }
}

/// `IManagedSettingsUpdateService`: presentation of the existing client compatibility block; does not evaluate or enforce policy.
pub trait IManagedSettingsUpdateService {
    /// `updateInfo`.
    fn update_info(&self) -> IObservable<Option<IManagedSettingsUpdateInfo>>;
}

service_id!(pub I_MANAGED_SETTINGS_UPDATE_SERVICE: dyn IManagedSettingsUpdateService = "managedSettingsUpdateService");

/// `getManagedSettingsUpdateMessage(error, product)`: o `title`, o `message` e o `detail`.
fn get_managed_settings_update_message(error: &IManagedSettingsCompatibilityError, product: &dyn IProductService) -> (String, String, Option<String>) {
    // The managed-settings request identifies this client as vscode; runtime targets are not in the response contract.
    let component = product.name_short();
    // `error.clientVersion || product.version`: a string vazia é falsa.
    let current_version = error.client_version.as_ref().map(|version| version.to_string_lossy()).filter(|version| !version.is_empty()).or_else(|| Some(product.version().to_owned()).filter(|version| !version.is_empty()));
    let required_version = error.minimum_client_version.as_ref().map(|version| version.to_string_lossy()).filter(|version| !version.is_empty());

    let message = match required_version {
        Some(required_version) => localize("managedSettingsUpdate.minimumVersion", "Your organization requires {0} {1} or later to use AI features.", &[component, required_version.as_str()]),
        None => localize("managedSettingsUpdate.message", "Your organization requires an update to {0} to use AI features.", &[component]),
    };

    (
        localize("managedSettingsUpdate.title", "Update required by your organization", &[]),
        message,
        current_version.map(|version| localize("managedSettingsUpdate.installed", "Installed: {0}", &[version.as_str()])),
    )
}

/// `getManagedSettingsUpdateInfo(error, product, state)`.
pub fn get_managed_settings_update_info(error: &IManagedSettingsCompatibilityError, product: &dyn IProductService, state: &State) -> IManagedSettingsUpdateInfo {
    let mut action = None;
    let mut update_status = None;
    let link = |label: String, href: &str| Some(IManagedSettingsUpdateAction { label, href: href.to_owned() });
    match state {
        State::Uninitialized => update_status = Some(localize("managedSettingsUpdate.initializing", "Update availability is not yet known.", &[])),
        State::Idle(_) => action = link(localize("managedSettingsUpdate.check", "Check for Updates", &[]), "command:update.checkForUpdate"),
        State::AvailableForDownload(_) => action = link(localize("managedSettingsUpdate.download", "Download Update", &[]), "command:update.downloadUpdate"),
        State::Downloaded(_) => action = link(localize("managedSettingsUpdate.install", "Install Update", &[]), "command:update.installUpdate"),
        State::Ready(_) => action = link(localize("managedSettingsUpdate.restart", "Restart to Update", &[]), "command:update.restartToUpdate"),
        State::Disabled(disabled) => {
            if disabled.reason == DisablementReason::Policy {
                update_status = Some(localize(
                    "managedSettingsUpdate.disabled",
                    "Built-in updates are disabled by your organization. Contact your administrator for an approved update.",
                    &[],
                ));
            }
        }
        _ => update_status = Some(localize("managedSettingsUpdate.progress", "An update operation is in progress.", &[])),
    }

    let (title, message, detail) = get_managed_settings_update_message(error, product);
    IManagedSettingsUpdateInfo { title, message, detail, action, update_status }
}
