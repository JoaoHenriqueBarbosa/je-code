//! Porte parcial de `src/vs/workbench/browser/parts/notifications/notificationsCommands.ts`: o
//! `NotificationActionRunner`, que executa as ações de uma notificação registrando a telemetria e reportando o erro.
//!
//! O registro dos comandos e das regras de teclado (`registerNotificationCommands`, que depende do `IListService`, do
//! centro e dos toasts) fica fora: só o executor entra aqui, porque é o que as pontes e os testes usam.
//!
//! Notas de tradução:
//! - **`extends ActionRunner`**: a struct guarda o [`ActionRunner`] base, criado com o `runAction` sobrescrito
//!   ([`ActionRunner::with_run_action`]); `super.runAction` é o `run` da própria ação.
//! - **O `context: unknown`** é um [`Dynamic`]; o item de notificação (`NotificationViewItem`) viaja nele como carga
//!   nativa de um objeto ([`notification_view_item_context`]) e `isNotificationViewItem(context)` é
//!   [`notification_view_item_from_context`].
//! - **`this.notificationService.error(error)`** recebe o [`Error`] da rejeição como [`NotificationMessage::Error`].

use std::rc::Rc;

use je_base::actions::{ActionRef, ActionRunner, IActionRunner, IRunEvent};
use je_base::async_::async_fn;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_platform::notification::{INotificationService, NotificationMessage};
use je_platform::telemetry::ITelemetryService;

use crate::common::notification_telemetry::{NotificationTelemetrySurface, log_notification_action};
use crate::common::notifications::NotificationViewItem;

/// O `NotificationViewItem` como contexto de uma ação (o item vivo, não uma cópia).
pub fn notification_view_item_context(item: &Rc<NotificationViewItem>) -> Dynamic {
    Dynamic::Object(ObjectRef::with_host(item.clone()))
}

/// `isNotificationViewItem(context)`: o item que o contexto carrega, se for um.
pub fn notification_view_item_from_context(context: &Dynamic) -> Option<Rc<NotificationViewItem>> {
    match context {
        Dynamic::Object(object) => object.host::<NotificationViewItem>(),
        _ => None,
    }
}

/// `NotificationActionRunner`.
pub struct NotificationActionRunner {
    runner: Rc<ActionRunner>,
    pub telemetry_surface: Option<NotificationTelemetrySurface>,
}

impl NotificationActionRunner {
    pub fn new(telemetry_surface: Option<NotificationTelemetrySurface>, telemetry_service: Rc<dyn ITelemetryService>, notification_service: Rc<dyn INotificationService>) -> Rc<Self> {
        let runner = ActionRunner::with_run_action(Rc::new(move |action: &ActionRef, context: Option<Dynamic>| -> Promise<()> {
            telemetry_service.public_log2("workbenchActionExecuted", Some(Dynamic::object([("id", Dynamic::from(action.id())), ("from", Dynamic::from("message"))])));
            if let Some(item) = context.as_ref().and_then(notification_view_item_from_context) {
                log_notification_action(&*telemetry_service, &item, action, telemetry_surface);
            }

            // Run and make sure to notify on any error again
            let started = errors::catch(|| action.run(&[context.unwrap_or(Dynamic::Undefined)]));
            let notification_service = notification_service.clone();
            async_fn(async move {
                let result: Result<(), Error> = match started {
                    Ok(promise) => promise.await.map(|_| ()),
                    Err(error) => Err(error),
                };
                if let Err(error) = result {
                    notification_service.error(vec![NotificationMessage::Error(error)]);
                }
                Ok(())
            })
        }));
        Rc::new(Self { runner, telemetry_surface })
    }
}

impl IActionRunner for NotificationActionRunner {
    fn on_did_run(&self) -> Event<IRunEvent> {
        self.runner.on_did_run()
    }

    fn on_will_run(&self) -> Event<IRunEvent> {
        self.runner.on_will_run()
    }

    fn run(&self, action: &ActionRef, context: Option<Dynamic>) -> Promise<()> {
        self.runner.run(action, context)
    }
}

impl IDisposable for NotificationActionRunner {
    fn dispose(&self) {
        self.runner.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "NotificationActionRunner"
    }
}
