//! Porte parcial de `src/vs/workbench/contrib/preferences/browser/preferencesWidgets.ts`: só o tipo `SettingsTarget`, o
//! escopo em que o editor de configurações mostra e grava as configurações. Os widgets (`SettingsTargetsWidget`,
//! `SettingsHeaderWidget`, `SearchWidget`...) dependem da árvore de widgets nativa e entram com o porte das telas de
//! preferências.
//!
//! Divergências: `SettingsTarget` (`ConfigurationTarget.APPLICATION | USER_LOCAL | USER_REMOTE | WORKSPACE | URI`) é o
//! enum [`SettingsTarget`]; a pasta do workspace (o `URI`) é [`SettingsTarget::Resource`]. Um `ConfigurationTarget`
//! volta com [`SettingsTarget::configuration_target`], em que o `URI` é `WORKSPACE_FOLDER`, como no original
//! (`URI.isUri(scope) ? ConfigurationTarget.WORKSPACE_FOLDER : scope`).

use je_base::uri::URI;
use je_platform::configuration::ConfigurationTarget;

/// `SettingsTarget`.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingsTarget {
    Application,
    UserLocal,
    UserRemote,
    Workspace,
    Resource(URI),
}

impl SettingsTarget {
    /// `URI.isUri(target) ? ConfigurationTarget.WORKSPACE_FOLDER : target`.
    pub fn configuration_target(&self) -> ConfigurationTarget {
        match self {
            Self::Application => ConfigurationTarget::Application,
            Self::UserLocal => ConfigurationTarget::UserLocal,
            Self::UserRemote => ConfigurationTarget::UserRemote,
            Self::Workspace => ConfigurationTarget::Workspace,
            Self::Resource(_) => ConfigurationTarget::WorkspaceFolder,
        }
    }

    /// O `URI` do alvo, se ele é uma pasta (`URI.isUri(target)`).
    pub fn resource(&self) -> Option<&URI> {
        match self {
            Self::Resource(resource) => Some(resource),
            _ => None,
        }
    }
}
