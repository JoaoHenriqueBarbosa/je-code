//! Porte de `src/vs/platform/notification/common/notificationTelemetry.ts`.
//!
//! `IExtensionNotificationTelemetry` (um objeto marcado, com a origem num `WeakMap`) é um
//! [`ExtensionNotificationTelemetry`] que carrega a origem dentro de si. A associação de telemetria a uma
//! ação (`WeakMap<IAction, ...>`) é uma tabela por thread chaveada pela identidade do `Rc` da ação.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, IAction};
use zyarr::RegExp;

/// `NotificationTelemetryId`: tipos auditados, sem relação com o conteúdo. Não servem para deduplicar notificações.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NotificationTelemetryId {
    AgentHostProgress,
    AuthenticationSignIn,
    AuthenticationContinue,
    ExtensionsActivation,
    ExtensionInstall,
    ExtensionDownload,
    ExtensionDownloadComplete,
    ExtensionDownloadError,
    ExtensionsDisabled,
    ExtensionDependencyLoop,
    ExtensionsAutoRestart,
    ExtensionHostUnresponsive,
    ExtensionHostVersionMismatch,
    ExtensionHostCrashRepeated,
    RemoteExtensionHostCrashRepeated,
    RemoteReconnect,
    RemoteAgentHostSshConnect,
    RemoteAgentHostSshConnectError,
    RemoteAgentHostTunnelConnect,
    RemoteAgentHostTunnelConnectError,
    RemoteAgentHostTunnelAuthenticationError,
    RemoteAgentHostWslConnect,
    RemoteAgentHostWslConnectError,
    RemoteAgentHostUpdate,
    RemoteAgentHostReconnect,
    PluginPackageOperation,
    PluginUpdate,
    PluginRepositoryClone,
    PluginRepositoryUpdate,
    PluginRepositoryRepair,
    DictationModelPrepare,
    DictationModelImport,
    FileSaveParticipants,
    WorkingCopySaveParticipants,
    FileOperationParticipants,
}

impl NotificationTelemetryId {
    /// O valor de texto do original.
    pub fn as_str(self) -> &'static str {
        use NotificationTelemetryId::*;
        match self {
            AgentHostProgress => "agentHost.progress",
            AuthenticationSignIn => "authentication.signIn",
            AuthenticationContinue => "authentication.continue",
            ExtensionsActivation => "extensions.activation",
            ExtensionInstall => "extensions.install",
            ExtensionDownload => "extensions.downloadVsix",
            ExtensionDownloadComplete => "extensions.downloadVsix.complete",
            ExtensionDownloadError => "extensions.downloadVsix.error",
            ExtensionsDisabled => "extensions.disabled",
            ExtensionDependencyLoop => "extensions.dependencyLoop",
            ExtensionsAutoRestart => "extensions.autoRestart",
            ExtensionHostUnresponsive => "extensions.host.unresponsive",
            ExtensionHostVersionMismatch => "extensions.host.versionMismatch",
            ExtensionHostCrashRepeated => "extensions.host.crashRepeated",
            RemoteExtensionHostCrashRepeated => "extensions.remoteHost.crashRepeated",
            RemoteReconnect => "remote.reconnect",
            RemoteAgentHostSshConnect => "remoteAgentHost.ssh.connect",
            RemoteAgentHostSshConnectError => "remoteAgentHost.ssh.connectError",
            RemoteAgentHostTunnelConnect => "remoteAgentHost.tunnel.connect",
            RemoteAgentHostTunnelConnectError => "remoteAgentHost.tunnel.connectError",
            RemoteAgentHostTunnelAuthenticationError => "remoteAgentHost.tunnel.authenticationError",
            RemoteAgentHostWslConnect => "remoteAgentHost.wsl.connect",
            RemoteAgentHostWslConnectError => "remoteAgentHost.wsl.connectError",
            RemoteAgentHostUpdate => "remoteAgentHost.update",
            RemoteAgentHostReconnect => "remoteAgentHost.reconnect",
            PluginPackageOperation => "plugins.packageOperation",
            PluginUpdate => "plugins.update",
            PluginRepositoryClone => "plugins.repository.clone",
            PluginRepositoryUpdate => "plugins.repository.update",
            PluginRepositoryRepair => "plugins.repository.repair",
            DictationModelPrepare => "dictation.model.prepare",
            DictationModelImport => "dictation.model.import",
            FileSaveParticipants => "files.saveParticipants",
            WorkingCopySaveParticipants => "files.workingCopySaveParticipants",
            FileOperationParticipants => "files.operationParticipants",
        }
    }
}

/// `NotificationActionTelemetryId`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NotificationActionTelemetryId {
    ProgressCancel,
    ManageExtension,
    NeverShowAgain,
    Reload,
    Relaunch,
    RestartExtensionHost,
    Continue,
    Decline,
    Copy,
    Configure,
}

impl NotificationActionTelemetryId {
    /// O valor de texto do original.
    pub fn as_str(self) -> &'static str {
        match self {
            NotificationActionTelemetryId::ProgressCancel => "progress.cancel",
            NotificationActionTelemetryId::ManageExtension => "manageExtension",
            NotificationActionTelemetryId::NeverShowAgain => "neverShowAgain",
            NotificationActionTelemetryId::Reload => "reload",
            NotificationActionTelemetryId::Relaunch => "relaunch",
            NotificationActionTelemetryId::RestartExtensionHost => "restartExtensionHost",
            NotificationActionTelemetryId::Continue => "continue",
            NotificationActionTelemetryId::Decline => "decline",
            NotificationActionTelemetryId::Copy => "copy",
            NotificationActionTelemetryId::Configure => "configure",
        }
    }
}

/// `INotificationTelemetrySource.origin`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationTelemetryOrigin {
    Core,
    Extension,
    Unknown,
}

/// `INotificationTelemetrySource.notificationId`: `NotificationTelemetryId | 'extension.message' | 'extension.progress' | 'unknown'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationTelemetrySourceId {
    Core(NotificationTelemetryId),
    ExtensionMessage,
    ExtensionProgress,
    Unknown,
}

/// `INotificationTelemetrySource`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationTelemetrySource {
    pub origin: NotificationTelemetryOrigin,
    pub notification_id: NotificationTelemetrySourceId,
    pub extension_id: String,
}

fn unknown_source() -> NotificationTelemetrySource {
    NotificationTelemetrySource {
        origin: NotificationTelemetryOrigin::Unknown,
        notification_id: NotificationTelemetrySourceId::Unknown,
        extension_id: "unknown".to_string(),
    }
}

/// `IExtensionNotificationTelemetry`: só se cria por [`extension_notification_telemetry`].
#[derive(Clone, Debug)]
pub struct ExtensionNotificationTelemetry(Rc<NotificationTelemetrySource>);

/// `NotificationTelemetry`.
#[derive(Clone, Debug)]
pub enum NotificationTelemetry {
    Id(NotificationTelemetryId),
    Extension(ExtensionNotificationTelemetry),
}

/// `EXTENSION_IDENTIFIER_REGEX` do `extensionManagement.ts`.
/// TODO(integração): trocar pelo porte de `platform/extensionManagement`.
const EXTENSION_IDENTIFIER_REGEX: &str = "^([a-z0-9A-Z][a-z0-9-A-Z]*)\\.([a-z0-9A-Z][a-z0-9-A-Z]*)$";

/// `extensionNotificationTelemetry`: só para pontes de extensão, com o identificador da descrição da extensão.
pub fn extension_notification_telemetry(extension_id: Option<&str>, kind: &str) -> ExtensionNotificationTelemetry {
    let valid = extension_id.filter(|id| {
        id.encode_utf16().count() <= 255 && RegExp::from_str(EXTENSION_IDENTIFIER_REGEX, "").is_ok_and(|re| re.exec(&id.encode_utf16().collect::<Vec<_>>(), 0).is_some())
    });
    ExtensionNotificationTelemetry(Rc::new(NotificationTelemetrySource {
        origin: NotificationTelemetryOrigin::Extension,
        notification_id: if kind == "message" { NotificationTelemetrySourceId::ExtensionMessage } else { NotificationTelemetrySourceId::ExtensionProgress },
        extension_id: valid.map_or_else(|| "unknown".to_string(), str::to_lowercase),
    }))
}

/// `getNotificationTelemetrySource`.
pub fn get_notification_telemetry_source(telemetry: Option<&NotificationTelemetry>) -> NotificationTelemetrySource {
    match telemetry {
        Some(NotificationTelemetry::Id(id)) => NotificationTelemetrySource {
            origin: NotificationTelemetryOrigin::Core,
            notification_id: NotificationTelemetrySourceId::Core(*id),
            extension_id: "none".to_string(),
        },
        Some(NotificationTelemetry::Extension(extension)) => (*extension.0).clone(),
        None => unknown_source(),
    }
}

/// `NotificationActionTelemetry`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationActionTelemetry {
    Id(NotificationActionTelemetryId),
    ExtensionButtonIndex(i64),
}

type ActionTelemetryTable = HashMap<usize, (Weak<dyn IAction>, NotificationActionTelemetry)>;

thread_local! {
    static ACTION_TELEMETRY: RefCell<ActionTelemetryTable> = RefCell::new(HashMap::new());
}

fn action_key(action: &ActionRef) -> usize {
    Rc::as_ptr(action) as *const () as usize
}

/// `withNotificationActionTelemetry`: associa metadados auditados a uma ação sem mudar a execução nem o id do comando.
pub fn with_notification_action_telemetry(action: ActionRef, telemetry: Option<NotificationActionTelemetry>) -> ActionRef {
    let accepted = match telemetry {
        Some(NotificationActionTelemetry::Id(id)) => Some(NotificationActionTelemetry::Id(id)),
        Some(NotificationActionTelemetry::ExtensionButtonIndex(index)) if (0..=1000).contains(&index) => {
            Some(NotificationActionTelemetry::ExtensionButtonIndex(index))
        }
        _ => None,
    };
    if let Some(accepted) = accepted {
        ACTION_TELEMETRY.with(|map| {
            let mut map = map.borrow_mut();
            // Solta as entradas de ações que já foram descartadas.
            map.retain(|_, (weak, _)| weak.strong_count() > 0);
            map.insert(action_key(&action), (Rc::downgrade(&action), accepted));
        });
    }
    action
}

/// `getNotificationActionTelemetry`.
pub fn get_notification_action_telemetry(action: &ActionRef) -> Option<NotificationActionTelemetry> {
    ACTION_TELEMETRY.with(|map| map.borrow().get(&action_key(action)).filter(|(weak, _)| weak.strong_count() > 0).map(|(_, t)| *t))
}
