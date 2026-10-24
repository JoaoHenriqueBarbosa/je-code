//! Porte de `src/vs/platform/notification/test/common/testNotificationService.ts`: o `TestNotificationService`,
//! que não mostra nada. `notify` e `prompt` devolvem o `NoOpNotification` e `status` devolve um handle que não faz
//! nada.
//!
//! Divergências: no original `info`, `warn` e `error` chamam `notify` e descartam o resultado; aqui o
//! [`INotificationService`] do porte os declara sem retorno, e como `notify` não tem efeito eles não fazem nada.

use std::rc::Rc;

use je_base::event::Event;
use je_platform::notification::{
    INotificationService, NoOpNotification, Notification, NotificationFilterSetting, NotificationHandle,
    NotificationMessage, NotificationSource, NotificationSourceFilter, NotificationsFilter, PromptChoiceKind,
    PromptOptions, Severity, StatusHandle, StatusMessageOptions,
};

/// `Disposable.None` devolvido por `status`.
struct NoOpStatus;

impl StatusHandle for NoOpStatus {
    fn close(&self) {}
}

#[derive(Default)]
pub struct TestNotificationService;

impl INotificationService for TestNotificationService {
    fn on_did_change_filter(&self) -> Event<()> {
        Event::none()
    }

    fn set_filter(&self, _filter: NotificationFilterSetting) {}

    fn get_filter(&self, _source: Option<&NotificationSource>) -> NotificationsFilter {
        NotificationsFilter::Off
    }

    fn get_filters(&self) -> Vec<NotificationSourceFilter> {
        Vec::new()
    }

    fn remove_filter(&self, _source_id: &str) {}

    fn notify(&self, _notification: Notification) -> Rc<dyn NotificationHandle> {
        Rc::new(NoOpNotification::new())
    }

    fn info(&self, _message: Vec<NotificationMessage>) {}

    fn warn(&self, _message: Vec<NotificationMessage>) {}

    fn error(&self, _message: Vec<NotificationMessage>) {}

    fn prompt(
        &self,
        _severity: Severity,
        _message: NotificationMessage,
        _choices: Vec<PromptChoiceKind>,
        _options: Option<PromptOptions>,
    ) -> Rc<dyn NotificationHandle> {
        Rc::new(NoOpNotification::new())
    }

    fn status(&self, _message: NotificationMessage, _options: Option<StatusMessageOptions>) -> Rc<dyn StatusHandle> {
        Rc::new(NoOpStatus)
    }
}
