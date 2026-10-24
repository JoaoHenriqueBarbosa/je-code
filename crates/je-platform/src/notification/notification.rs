//! Porte de `src/vs/platform/notification/common/notification.ts`: os tipos e o contrato do serviço.
//!
//! Divergências: `Error` em `NotificationMessage` é [`je_base::errors::Error`]; as listas de ações são
//! `Vec<ActionRef>`; o `run` de uma escolha de prompt é `Rc<dyn Fn()>`.

use std::rc::Rc;

use je_base::actions::ActionRef;
use je_base::error_message::ErrorWithActions;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::nls::localize;
pub use je_base::severity::Severity;
use je_instantiation::service_id;

use super::notification_legacy::LegacyExtensionLinkParsing;
use super::notification_message::NotificationText;
use super::notification_telemetry::{NotificationActionTelemetryId, NotificationTelemetry};

/// `NotificationMessage`: `string | Error | NotificationText`.
///
/// O `Error` que carrega ações (`IErrorWithActions`, `isErrorWithActions`) é a variante `ErrorWithActions`: o
/// [`Error`] do porte guarda propriedades como texto e não cabe uma lista de ações nele.
#[derive(Clone, Debug)]
pub enum NotificationMessage {
    Text(String),
    Error(Error),
    ErrorWithActions(ErrorWithActions),
    Rich(NotificationText),
}

impl From<ErrorWithActions> for NotificationMessage {
    fn from(error: ErrorWithActions) -> Self {
        NotificationMessage::ErrorWithActions(error)
    }
}

impl From<&str> for NotificationMessage {
    fn from(text: &str) -> Self {
        NotificationMessage::Text(text.to_string())
    }
}

impl From<String> for NotificationMessage {
    fn from(text: String) -> Self {
        NotificationMessage::Text(text)
    }
}

impl From<Error> for NotificationMessage {
    fn from(error: Error) -> Self {
        NotificationMessage::Error(error)
    }
}

impl From<NotificationText> for NotificationMessage {
    fn from(text: NotificationText) -> Self {
        NotificationMessage::Rich(text)
    }
}

/// `NotificationPriority`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NotificationPriority {
    /// Prioridade padrão: o centro de notificações mostra o toast.
    #[default]
    Default,
    /// Mostrada só quando o usuário não tiver pedido para não ser incomodado.
    Optional,
    /// Nunca vira toast: só aparece no centro.
    Silent,
    /// Mostrada mesmo com o modo "não perturbe" ligado.
    Urgent,
}

/// `INotificationProperties`.
#[derive(Clone, Debug, Default)]
pub struct NotificationProperties {
    pub telemetry: Option<NotificationTelemetry>,
    /// Só as pontes de notificação e progresso das extensões usam.
    pub legacy_extension_link_parsing: Option<LegacyExtensionLinkParsing>,
    /// Notificação que fica visível até o usuário fechar.
    pub sticky: bool,
    pub priority: Option<NotificationPriority>,
    pub never_show_again: Option<NeverShowAgainOptions>,
}

/// `NeverShowAgainScope`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NeverShowAgainScope {
    Workspace,
    Profile,
    Application,
}

/// `INeverShowAgainOptions`.
#[derive(Clone, Debug)]
pub struct NeverShowAgainOptions {
    pub id: String,
    pub is_secondary: bool,
    pub scope: Option<NeverShowAgainScope>,
}

/// `INotificationSource`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationSource {
    pub id: String,
    pub label: String,
}

/// `string | INotificationSource`.
#[derive(Clone, Debug)]
pub enum NotificationSourceOrText {
    Text(String),
    Source(NotificationSource),
}

/// `INotificationActions`.
#[derive(Clone, Default)]
pub struct NotificationActions {
    pub primary: Vec<ActionRef>,
    pub secondary: Vec<ActionRef>,
}

/// `INotificationProgressProperties`.
#[derive(Clone, Copy, Debug, Default)]
pub struct NotificationProgressProperties {
    pub infinite: bool,
    pub total: Option<f64>,
    pub worked: Option<f64>,
}

/// `INotification`.
#[derive(Clone)]
pub struct Notification {
    pub properties: NotificationProperties,
    pub id: Option<String>,
    pub severity: Severity,
    pub message: NotificationMessage,
    pub source: Option<NotificationSourceOrText>,
    pub actions: Option<NotificationActions>,
    pub progress: Option<NotificationProgressProperties>,
}

impl Notification {
    pub fn new(severity: Severity, message: impl Into<NotificationMessage>) -> Self {
        Self {
            properties: NotificationProperties::default(),
            id: None,
            severity,
            message: message.into(),
            source: None,
            actions: None,
            progress: None,
        }
    }
}

/// `INotificationProgress`.
pub trait NotificationProgress {
    fn infinite(&self);
    fn total(&self, value: f64);
    fn worked(&self, value: f64);
    fn done(&self);
}

/// `INotificationHandle`.
pub trait NotificationHandle {
    fn visible(&self) -> Option<bool> {
        None
    }
    fn on_did_close(&self) -> Event<()>;
    fn on_did_change_visibility(&self) -> Event<bool>;
    fn progress(&self) -> Rc<dyn NotificationProgress>;
    fn update_severity(&self, severity: Severity);
    fn update_message(&self, message: NotificationMessage);
    fn update_actions(&self, actions: Option<NotificationActions>);
    fn close(&self);
}

/// `IStatusHandle`.
pub trait StatusHandle {
    fn close(&self);
}

/// `IBasePromptChoice`.
#[derive(Clone)]
pub struct BasePromptChoice {
    pub telemetry_id: Option<NotificationActionTelemetryId>,
    pub label: String,
    pub keep_open: bool,
    pub run: Rc<dyn Fn()>,
}

/// `IPromptChoice`.
#[derive(Clone)]
pub struct PromptChoice {
    pub base: BasePromptChoice,
    pub is_secondary: bool,
}

/// `IPromptChoiceWithMenu`: o `isSecondary` é sempre `false`/`undefined`.
#[derive(Clone)]
pub struct PromptChoiceWithMenu {
    pub base: BasePromptChoice,
    pub menu: Vec<BasePromptChoice>,
}

/// `IPromptChoice | IPromptChoiceWithMenu`.
#[derive(Clone)]
pub enum PromptChoiceKind {
    Plain(PromptChoice),
    WithMenu(PromptChoiceWithMenu),
}

/// `IPromptOptions`.
#[derive(Clone, Default)]
pub struct PromptOptions {
    pub properties: NotificationProperties,
    pub on_cancel: Option<Rc<dyn Fn()>>,
}

/// `IStatusMessageOptions`.
#[derive(Clone, Copy, Debug, Default)]
pub struct StatusMessageOptions {
    /// Quantos milissegundos esperar antes de mostrar a mensagem de status.
    pub show_after: Option<u64>,
    /// Quantos milissegundos esperar antes de escondê-la.
    pub hide_after: Option<u64>,
}

/// `NotificationsFilter`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationsFilter {
    /// Todas as notificações são mostradas.
    Off,
    /// Só as de erro viram toast.
    Error,
}

/// `INotificationSourceFilter`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationSourceFilter {
    pub source: NotificationSource,
    pub filter: NotificationsFilter,
}

/// `NotificationsFilter | INotificationSourceFilter`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotificationFilterSetting {
    Global(NotificationsFilter),
    Source(NotificationSourceFilter),
}

/// `INotificationService`.
pub trait INotificationService {
    fn on_did_change_filter(&self) -> Event<()>;
    fn set_filter(&self, filter: NotificationFilterSetting);
    fn get_filter(&self, source: Option<&NotificationSource>) -> NotificationsFilter;
    fn get_filters(&self) -> Vec<NotificationSourceFilter>;
    fn remove_filter(&self, source_id: &str);
    fn notify(&self, notification: Notification) -> Rc<dyn NotificationHandle>;
    fn info(&self, message: Vec<NotificationMessage>);
    fn warn(&self, message: Vec<NotificationMessage>);
    fn error(&self, message: Vec<NotificationMessage>);
    fn prompt(&self, severity: Severity, message: NotificationMessage, choices: Vec<PromptChoiceKind>, options: Option<PromptOptions>) -> Rc<dyn NotificationHandle>;
    fn status(&self, message: NotificationMessage, options: Option<StatusMessageOptions>) -> Rc<dyn StatusHandle>;
}

service_id!(pub I_NOTIFICATION_SERVICE: dyn INotificationService = "notificationService");

/// `NoOpNotification`.
#[derive(Default)]
pub struct NoOpNotification {
    progress: Rc<NoOpProgress>,
}

impl NoOpNotification {
    pub fn new() -> Self {
        Self::default()
    }
}

impl NotificationHandle for NoOpNotification {
    fn on_did_close(&self) -> Event<()> {
        Event::none()
    }
    fn on_did_change_visibility(&self) -> Event<bool> {
        Event::none()
    }
    fn progress(&self) -> Rc<dyn NotificationProgress> {
        self.progress.clone()
    }
    fn update_severity(&self, _severity: Severity) {}
    fn update_message(&self, _message: NotificationMessage) {}
    fn update_actions(&self, _actions: Option<NotificationActions>) {}
    fn close(&self) {}
}

/// `NoOpProgress`.
#[derive(Default)]
pub struct NoOpProgress;

impl NotificationProgress for NoOpProgress {
    fn infinite(&self) {}
    fn done(&self) {}
    fn total(&self, _value: f64) {}
    fn worked(&self, _value: f64) {}
}

/// `withSeverityPrefix`.
pub fn with_severity_prefix(label: &str, severity: Severity) -> String {
    if severity == Severity::Error {
        return localize("severityPrefix.error", "Error: {0}", &[label]);
    }
    if severity == Severity::Warning {
        return localize("severityPrefix.warning", "Warning: {0}", &[label]);
    }
    localize("severityPrefix.info", "Info: {0}", &[label])
}

/// `isNotificationSource`: `id` e `label` de texto. O tipo já garante; o guarda existe para valores dinâmicos.
pub fn is_notification_source(thing: &je_base::dynamic::Dynamic) -> bool {
    matches!(thing, je_base::dynamic::Dynamic::Object(_))
        && matches!(thing.get_str("id"), je_base::dynamic::Dynamic::String(_))
        && matches!(thing.get_str("label"), je_base::dynamic::Dynamic::String(_))
}
