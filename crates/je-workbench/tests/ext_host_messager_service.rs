//! Porte de `src/vs/workbench/api/test/browser/extHostMessagerService.test.ts` (suíte `ExtHostMessageService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso; o `store` do original é o
//!   `add` do guarda.
//! - O `for (const identifier of [...])` que gera três `test(...)` é um só `#[test]` que roda o corpo para cada
//!   identificador (`Some("Publisher.Extension")`, `None` e `Some("/private/extension")`), cada rodada com o seu guarda
//!   de vazamentos, como cada `test` do mocha tem o seu.
//! - `new MainThreadMessageService(null!, ...)`: o construtor do porte não recebe o contexto do host de extensões (o
//!   original não o usa).
//! - `mock<IDialogService>()` com `prompt` sobrescrito é o [`PromptDialogService`]: só o `prompt` do caso responde, e todo
//!   outro membro falha como falharia chamar um membro ausente do `mock`.
//! - `emptyNotificationService` (todo método lança `not implemented`, `status` devolve um handle) e a classe
//!   `EmptyNotificationService` (com o `withNotify`) são o mesmo dublê, [`EmptyNotificationService`], com ou sem o fecho
//!   `with_notify`.
//! - `await` sobre as promessas roda dentro de `run_with_faked_timers`; `queueMicrotask` é `runtime::queue_microtask`.
//! - `assert.deepStrictEqual` de objetos vira a comparação das partes com tipos que têm `PartialEq`; os eventos de
//!   telemetria (`Record<string, unknown>`) são lidos como [`Dynamic`].
//! - `TestDialogService` e o `emptyCommandService` são reimplementados como no original (o diálogo roda o primeiro botão).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::actions::IActionRunner;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::{IDisposable, to_disposable};
use je_base::linked_text::{Link, LinkedTextNode};
use je_base::map::OrderedMap;
use je_base::promise::{MaybePromise, Promise};
use je_base::runtime;
use je_base::severity::Severity;
use je_base::JsString;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::commands::commands::{CommandEvent, ICommandService};
use je_platform::dialogs::dialogs::{
    Confirmation, ConfirmationResult, DialogTypeOrSeverity, IDialogService, Input, InputResult, Prompt, PromptBaseButton, PromptCancel, PromptResult, CheckboxResult,
};
use je_platform::notification::notification_telemetry::{NotificationTelemetryOrigin, NotificationTelemetrySource, NotificationTelemetrySourceId};
use je_platform::notification::{
    INotificationService, NoOpNotification, Notification, NotificationFilterSetting, NotificationHandle, NotificationMessage, NotificationSource, NotificationSourceFilter,
    NotificationsFilter, PromptChoiceKind, PromptOptions, StatusHandle, StatusMessageOptions,
};
use je_platform::storage::storage::{InMemoryStorageService, StorageService};
use je_workbench::api::common::ext_host_protocol::{MainThreadMessageOptions, MainThreadMessageServiceShape, MainThreadMessageSource, MessageCommandDto};
use je_workbench::api::browser::main_thread_message_service::MainThreadMessageService;
use je_workbench::browser::parts::notifications::notifications_commands::{NotificationActionRunner, notification_view_item_context};
use je_workbench::common::notification_telemetry::{NotificationTelemetrySurface, log_notification_shown};
use je_workbench::common::notifications::{INotificationsModel, NotificationViewItem, NotificationsFilterState};
use je_workbench::services::notification::common::notification_service::NotificationService;
use je_workbench::test::browser::workbench_test_services::TestExtensionService;
use je_workbench::test::common::test_notification_telemetry::TestNotificationTelemetryService;

/// `emptyCommandService`.
struct EmptyCommandService;

impl ICommandService for EmptyCommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }

    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }

    fn execute_command(&self, _command_id: &str, _args: Vec<Dynamic>) -> Promise<Dynamic> {
        Promise::resolved(Dynamic::Undefined)
    }
}

struct NoopStatusHandle;

impl StatusHandle for NoopStatusHandle {
    fn close(&self) {}
}

/// `emptyNotificationService` e `EmptyNotificationService`.
struct EmptyNotificationService {
    with_notify: Option<Box<dyn Fn(Notification)>>,
}

impl EmptyNotificationService {
    /// `emptyNotificationService`: `notify` lança `not implemented`.
    fn throwing() -> Rc<Self> {
        Rc::new(Self { with_notify: None })
    }

    /// `new EmptyNotificationService(withNotify)`.
    fn with_notify(with_notify: impl Fn(Notification) + 'static) -> Rc<Self> {
        Rc::new(Self { with_notify: Some(Box::new(with_notify)) })
    }
}

impl INotificationService for EmptyNotificationService {
    fn on_did_change_filter(&self) -> Event<()> {
        Event::none()
    }

    fn set_filter(&self, _filter: NotificationFilterSetting) {
        panic!("not implemented");
    }

    fn get_filter(&self, _source: Option<&NotificationSource>) -> NotificationsFilter {
        panic!("not implemented");
    }

    fn get_filters(&self) -> Vec<NotificationSourceFilter> {
        panic!("not implemented");
    }

    fn remove_filter(&self, _source_id: &str) {
        panic!("not implemented");
    }

    fn notify(&self, notification: Notification) -> Rc<dyn NotificationHandle> {
        match &self.with_notify {
            Some(with_notify) => {
                with_notify(notification);
                Rc::new(NoOpNotification::new())
            }
            None => panic!("not implemented"),
        }
    }

    fn info(&self, _message: Vec<NotificationMessage>) {
        panic!("not implemented");
    }

    fn warn(&self, _message: Vec<NotificationMessage>) {
        panic!("not implemented");
    }

    fn error(&self, _message: Vec<NotificationMessage>) {
        panic!("not implemented");
    }

    fn prompt(&self, _severity: Severity, _message: NotificationMessage, _choices: Vec<PromptChoiceKind>, _options: Option<PromptOptions>) -> Rc<dyn NotificationHandle> {
        panic!("not implemented");
    }

    fn status(&self, _message: NotificationMessage, _options: Option<StatusMessageOptions>) -> Rc<dyn StatusHandle> {
        Rc::new(NoopStatusHandle)
    }
}

/// Roda o botão com `{ checkboxChecked: false }` (as funções `run` dos casos devolvem o valor direto).
fn run_button(button: &PromptBaseButton) -> Dynamic {
    match (button.run)(CheckboxResult { checkbox_checked: Some(false) }) {
        MaybePromise::Value(value) => value,
        MaybePromise::Promise(_) => panic!("o botão devolveu uma promessa"),
    }
}

/// `TestDialogService` (`platform/dialogs/test/common/testDialogService.ts`), sem resultados padrão.
struct TestDialogService;

impl IDialogService for TestDialogService {
    fn on_will_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn confirm(&self, _confirmation: Confirmation) -> Promise<ConfirmationResult> {
        Promise::resolved(ConfirmationResult { checkbox_checked: None, confirmed: false })
    }

    fn prompt(&self, prompt: Prompt) -> Promise<PromptResult> {
        let mut buttons: Vec<PromptBaseButton> = prompt.buttons.unwrap_or_default().into_iter().map(|button| button.base).collect();
        if let Some(PromptCancel::Button(cancel)) = prompt.cancel_button {
            buttons.push(cancel.base);
        }
        Promise::resolved(PromptResult { checkbox_checked: None, result: buttons.first().map(run_button) })
    }

    fn input(&self, _input: Input) -> Promise<InputResult> {
        Promise::resolved(InputResult { confirmation: ConfirmationResult { checkbox_checked: None, confirmed: true }, values: Some(Vec::new()) })
    }

    fn info(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn warn(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn error(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn about(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

/// `new class extends mock<IDialogService>() { override prompt(...) }`.
struct PromptDialogService {
    prompt: Box<dyn Fn(Prompt) -> Promise<PromptResult>>,
}

impl PromptDialogService {
    fn new(prompt: impl Fn(Prompt) -> Promise<PromptResult> + 'static) -> Rc<Self> {
        Rc::new(Self { prompt: Box::new(prompt) })
    }
}

impl IDialogService for PromptDialogService {
    fn on_will_show_dialog(&self) -> Event<()> {
        panic!("onWillShowDialog is not defined")
    }

    fn on_did_show_dialog(&self) -> Event<()> {
        panic!("onDidShowDialog is not defined")
    }

    fn confirm(&self, _confirmation: Confirmation) -> Promise<ConfirmationResult> {
        panic!("confirm is not a function")
    }

    fn prompt(&self, prompt: Prompt) -> Promise<PromptResult> {
        (self.prompt)(prompt)
    }

    fn input(&self, _input: Input) -> Promise<InputResult> {
        panic!("input is not a function")
    }

    fn info(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        panic!("info is not a function")
    }

    fn warn(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        panic!("warn is not a function")
    }

    fn error(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        panic!("error is not a function")
    }

    fn about(&self) -> Promise<()> {
        panic!("about is not a function")
    }
}

fn command(handle: f64, title: &str, is_close_affordance: bool) -> MessageCommandDto {
    MessageCommandDto { title: title.to_owned(), is_close_affordance, handle }
}

fn text_of(value: &Dynamic) -> String {
    match value {
        Dynamic::String(text) => text.to_string_lossy(),
        other => panic!("esperava texto, veio {other:?}"),
    }
}

fn number_of(value: &Dynamic) -> f64 {
    match value {
        Dynamic::Number(number) => *number,
        other => panic!("esperava número, veio {other:?}"),
    }
}

fn message_service(guard: &LeakGuard, notification_service: Rc<dyn INotificationService>, dialog_service: Rc<dyn IDialogService>) -> Rc<MainThreadMessageService> {
    guard.add_rc(MainThreadMessageService::new(notification_service, Rc::new(EmptyCommandService), dialog_service, TestExtensionService::new()))
}

// ---- suite `notification telemetry` -----------------------------------------------------------------

fn attributes_extension_messages_and_button_positions(identifier: Option<&'static str>) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let telemetry = Rc::new(TestNotificationTelemetryService::new());
    let storage: Rc<dyn StorageService> = Rc::new(guard.add_rc(InMemoryStorageService::new()));
    let notifications = guard.add_rc(NotificationService::new(storage));
    {
        let notifications = notifications.clone();
        guard.add(to_disposable(move || {
            for item in notifications.model.notifications() {
                item.close();
            }
        }));
    }
    let service = message_service(&guard, notifications.clone(), Rc::new(TestDialogService));
    let runner = guard.add_rc(NotificationActionRunner::new(None, telemetry.clone(), notifications.clone()));
    let promise = service.rpc_show_message(
        Severity::Warning,
        "private message",
        MainThreadMessageOptions {
            source: identifier.map(|identifier| MainThreadMessageSource { identifier: ExtensionIdentifier::new(identifier), label: "private source label".to_owned() }),
            ..MainThreadMessageOptions::default()
        },
        vec![command(2.0, "Sign in to private server", true)],
    );
    let item = notifications.model.notifications()[0].clone();
    log_notification_shown(&*telemetry, &item, NotificationTelemetrySurface::Toast);

    let (run_item, run_telemetry) = (item.clone(), telemetry.clone());
    run_with_faked_timers(async move {
        if identifier.is_some() {
            let secondary = run_item.actions().secondary[0].clone();
            runner.run(&secondary, Some(notification_view_item_context(&run_item))).await.expect("a ação secundária rejeitou");
        }
        let primary = run_item.actions().primary[0].clone();
        runner.run(&primary, Some(notification_view_item_context(&run_item))).await.expect("a ação primária rejeitou");
        run_item.close();
        let selected = promise.await.expect("a mensagem rejeitou");

        let interactions = run_telemetry.interactions();
        let actions: Vec<(String, String, f64)> =
            interactions.iter().map(|event| (text_of(&event.get_str("interaction")), text_of(&event.get_str("actionId")), number_of(&event.get_str("extensionButtonIndex")))).collect();
        let mut payload = run_telemetry.shown();
        payload.extend(interactions);
        let private_payload = je_base::dynamic::json_stringify(&Dynamic::array(payload)).expect("o payload serializa").contains("private");

        let extension_id = if identifier == Some("Publisher.Extension") { "publisher.extension" } else { "unknown" };
        let mut expected_actions: Vec<(String, String, f64)> = Vec::new();
        if identifier.is_some() {
            expected_actions.push(("secondaryAction".to_owned(), "manageExtension".to_owned(), -1.0));
        }
        expected_actions.push(("primaryAction".to_owned(), "unknown".to_owned(), 2.0));

        assert_eq!(selected, Some(2.0));
        assert_eq!(
            run_item.telemetry,
            NotificationTelemetrySource {
                origin: NotificationTelemetryOrigin::Extension,
                notification_id: NotificationTelemetrySourceId::ExtensionMessage,
                extension_id: extension_id.to_owned()
            }
        );
        assert_eq!(actions, expected_actions);
        assert!(!private_payload);
    });
}

#[test]
fn attributes_extension_messages_and_button_positions_without_inferring_semantics() {
    for identifier in [Some("Publisher.Extension"), None, Some("/private/extension")] {
        attributes_extension_messages_and_button_positions(identifier);
    }
}

#[test]
fn preserves_command_and_web_links_in_extension_notifications() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let message_nodes: Rc<RefCell<Option<Vec<LinkedTextNode>>>> = Rc::default();
    let nodes_slot = message_nodes.clone();
    let store = guard.store();
    let service = message_service(
        &guard,
        EmptyNotificationService::with_notify(move |notification| {
            let item = NotificationViewItem::create(&notification, &NotificationsFilterState { global: NotificationsFilter::Off, sources: OrderedMap::new() }).expect("a notificação tem mensagem");
            let closing = item.clone();
            store.add(to_disposable(move || closing.close()));
            *nodes_slot.borrow_mut() = Some(item.message().linked_text.nodes.clone());
            let primary = notification.actions.expect("as ações").primary[0].clone();
            runtime::queue_microtask(move || {
                let _ = primary.run(&[]);
            });
        }),
        Rc::new(TestDialogService),
    );

    run_with_faked_timers(async move {
        let selected = service
            .rpc_show_message(
                Severity::Warning,
                "See [logs](command:python.viewOutput) or [documentation](https://example.com).",
                MainThreadMessageOptions::default(),
                vec![command(42.0, "Dismiss", true)],
            )
            .await
            .expect("a mensagem rejeitou");

        let link = |label: &str, href: &str| LinkedTextNode::Link(Link { label: JsString::from(label), href: JsString::from(href), title: None });
        let text = |text: &str| LinkedTextNode::Text(JsString::from(text));
        assert_eq!(selected, Some(42.0));
        assert_eq!(
            message_nodes.borrow().clone(),
            Some(vec![text("See "), link("logs", "command:python.viewOutput"), text(" or "), link("documentation", "https://example.com"), text(".")])
        );
    });
}

#[test]
fn propagte_handle_on_select() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = message_service(
        &guard,
        EmptyNotificationService::with_notify(|notification| {
            let primary = notification.actions.expect("as ações").primary;
            assert_eq!(primary.len(), 1);
            let action = primary[0].clone();
            runtime::queue_microtask(move || {
                let _ = action.run(&[]);
            });
        }),
        Rc::new(TestDialogService),
    );

    let running = service.clone();
    run_with_faked_timers(async move {
        let handle = running.rpc_show_message(Severity::Info, "h", MainThreadMessageOptions::default(), vec![command(42.0, "a thing", true)]).await.expect("a mensagem rejeitou");
        assert_eq!(handle, Some(42.0));
    });

    service.dispose();
}

// ---- suite `modal` ----------------------------------------------------------------------------------

fn modal_options() -> MainThreadMessageOptions {
    MainThreadMessageOptions { modal: Some(true), ..MainThreadMessageOptions::default() }
}

#[test]
fn calls_dialog_service() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = message_service(
        &guard,
        EmptyNotificationService::throwing(),
        PromptDialogService::new(|prompt| {
            assert_eq!(prompt.base.dialog_type, Some(DialogTypeOrSeverity::Severity(Severity::Info)));
            assert_eq!(prompt.base.message, "h");
            let buttons = prompt.buttons.expect("os botões");
            assert_eq!(buttons.len(), 1);
            let Some(PromptCancel::Button(cancel_button)) = &prompt.cancel_button else { panic!("o botão de cancelar") };
            assert_eq!(cancel_button.label.as_deref(), Some("Cancel"));
            Promise::resolved(PromptResult { checkbox_checked: None, result: Some(run_button(&buttons[0].base)) })
        }),
    );

    let running = service.clone();
    run_with_faked_timers(async move {
        let handle = running.rpc_show_message(Severity::Info, "h", modal_options(), vec![command(42.0, "a thing", false)]).await.expect("a mensagem rejeitou");
        assert_eq!(handle, Some(42.0));
    });

    service.dispose();
}

#[test]
fn returns_undefined_when_cancelled() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = message_service(
        &guard,
        EmptyNotificationService::throwing(),
        PromptDialogService::new(|prompt| {
            let Some(PromptCancel::Button(cancel_button)) = &prompt.cancel_button else { panic!("o botão de cancelar") };
            Promise::resolved(PromptResult { checkbox_checked: None, result: Some(run_button(&cancel_button.base)) })
        }),
    );

    let running = service.clone();
    run_with_faked_timers(async move {
        let handle = running.rpc_show_message(Severity::Info, "h", modal_options(), vec![command(42.0, "a thing", false)]).await.expect("a mensagem rejeitou");
        assert_eq!(handle, None);
    });

    service.dispose();
}

#[test]
fn hides_cancel_button_when_not_needed() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let service = message_service(
        &guard,
        EmptyNotificationService::throwing(),
        PromptDialogService::new(|prompt| {
            assert_eq!(prompt.buttons.expect("os botões").len(), 0);
            let Some(PromptCancel::Button(cancel_button)) = &prompt.cancel_button else { panic!("o botão de cancelar") };
            Promise::resolved(PromptResult { checkbox_checked: None, result: Some(run_button(&cancel_button.base)) })
        }),
    );

    let running = service.clone();
    run_with_faked_timers(async move {
        let handle = running.rpc_show_message(Severity::Info, "h", modal_options(), vec![command(42.0, "a thing", true)]).await.expect("a mensagem rejeitou");
        assert_eq!(handle, Some(42.0));
    });

    service.dispose();
}
