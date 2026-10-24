//! Porte de `src/vs/workbench/contrib/terminalContrib/notification/test/browser/terminalNotification.test.ts` (suíte `Terminal OSC 99 notifications`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`TestNotificationHandle`, `TestNotificationProgress` e `TestOsc99Host`.** São as structs de mesmo nome, com os campos em `Cell`/`RefCell` (os do original são
//!   públicos e mutáveis). O `_disposeActions`, que chama `dispose` nas ações que o têm, é o `downcast_ref::<Action>()` do `IAction`. Os emissores dos
//!   handles (que o original não descarta) entram no guarda de vazamentos.
//! - **`setup`/`teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O [`Fixture`] cria o guarda, o hospedeiro e o tratador (`store.add(...)`), e o `Drop`
//!   fecha as notificações do hospedeiro (o `teardown`) antes de o guarda descartar o tratador e conferir os vazamentos.
//! - **`strictEqual(host.notifications[0].message, 'Hello: World')`.** A mensagem é um `NotificationMessage::Text`, comparada pelo texto ([`message_text`]).
//! - **`await actions.primary[0].run()`.** É `.run(&[]).await` dentro de `run_with_faked_timers`.
//! - **`if (!actions?.primary || actions.primary.length === 0) throw ...`** é um `assert!` com a mesma mensagem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::actions::{Action, ActionRef};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_platform::notification::notification::{
    Notification, NotificationActions, NotificationHandle, NotificationMessage, NotificationPriority, NotificationProgress, NotificationSourceOrText, Severity,
};
use je_workbench::contrib::terminal_contrib::notification::browser::terminal_notification_handler::{IOsc99NotificationHost, TerminalNotificationHandler};

struct TestNotificationProgress;

impl NotificationProgress for TestNotificationProgress {
    fn infinite(&self) {}

    fn total(&self, _value: f64) {}

    fn worked(&self, _value: f64) {}

    fn done(&self) {}
}

struct TestNotificationHandle {
    on_did_close: Rc<Emitter<()>>,
    closed: Cell<bool>,
    message: RefCell<NotificationMessage>,
    severity: Cell<Severity>,
    actions: RefCell<Option<NotificationActions>>,
    priority: Option<NotificationPriority>,
    #[allow(dead_code)]
    source: Option<NotificationSourceOrText>,
}

impl TestNotificationHandle {
    fn new(notification: Notification, store: &DisposableStore) -> Rc<Self> {
        let on_did_close = Emitter::new();
        store.add_rc(on_did_close.clone());
        Rc::new(Self {
            on_did_close,
            closed: Cell::new(false),
            message: RefCell::new(notification.message),
            severity: Cell::new(notification.severity),
            actions: RefCell::new(notification.actions),
            priority: notification.properties.priority,
            source: notification.source,
        })
    }

    fn message_text(&self) -> String {
        message_text(&self.message.borrow())
    }

    fn dispose_actions(actions: Option<&NotificationActions>) {
        for action in actions.iter().flat_map(|actions| actions.primary.iter().chain(actions.secondary.iter())) {
            if let Some(action) = action.as_any().downcast_ref::<Action>() {
                action.dispose();
            }
        }
    }
}

/// O texto de uma `NotificationMessage::Text`.
fn message_text(message: &NotificationMessage) -> String {
    match message {
        NotificationMessage::Text(text) => text.clone(),
        other => panic!("mensagem inesperada: {other:?}"),
    }
}

impl NotificationHandle for TestNotificationHandle {
    fn on_did_close(&self) -> Event<()> {
        self.on_did_close.event()
    }

    fn on_did_change_visibility(&self) -> Event<bool> {
        Event::none()
    }

    fn progress(&self) -> Rc<dyn NotificationProgress> {
        Rc::new(TestNotificationProgress)
    }

    fn update_severity(&self, severity: Severity) {
        self.severity.set(severity);
    }

    fn update_message(&self, message: NotificationMessage) {
        *self.message.borrow_mut() = message;
    }

    fn update_actions(&self, actions: Option<NotificationActions>) {
        Self::dispose_actions(self.actions.borrow().as_ref());
        *self.actions.borrow_mut() = actions;
    }

    fn close(&self) {
        if self.closed.get() {
            return;
        }
        self.closed.set(true);
        Self::dispose_actions(self.actions.borrow().as_ref());
        self.on_did_close.fire(());
    }
}

struct TestOsc99Host {
    enabled: Cell<bool>,
    window_focused: Cell<bool>,
    terminal_visible: Cell<bool>,
    writes: RefCell<Vec<String>>,
    notifications: RefCell<Vec<Rc<TestNotificationHandle>>>,
    focus_calls: Cell<u32>,
    updated_enable_notifications: RefCell<Vec<bool>>,
    log_messages: RefCell<Vec<String>>,
    store: Rc<DisposableStore>,
}

impl TestOsc99Host {
    fn new(store: Rc<DisposableStore>) -> Rc<Self> {
        Rc::new(Self {
            enabled: Cell::new(true),
            window_focused: Cell::new(false),
            terminal_visible: Cell::new(false),
            writes: RefCell::new(Vec::new()),
            notifications: RefCell::new(Vec::new()),
            focus_calls: Cell::new(0),
            updated_enable_notifications: RefCell::new(Vec::new()),
            log_messages: RefCell::new(Vec::new()),
            store,
        })
    }

    fn notification(&self, index: usize) -> Rc<TestNotificationHandle> {
        self.notifications.borrow()[index].clone()
    }

    fn notification_count(&self) -> usize {
        self.notifications.borrow().len()
    }

    fn write(&self, index: usize) -> String {
        self.writes.borrow()[index].clone()
    }
}

impl IOsc99NotificationHost for TestOsc99Host {
    fn is_enabled(&self) -> bool {
        self.enabled.get()
    }

    fn is_window_focused(&self) -> bool {
        self.window_focused.get()
    }

    fn is_terminal_visible(&self) -> bool {
        self.terminal_visible.get()
    }

    fn focus_terminal(&self) {
        self.focus_calls.set(self.focus_calls.get() + 1);
    }

    fn notify(&self, notification: Notification) -> Rc<dyn NotificationHandle> {
        let handle = TestNotificationHandle::new(notification, &self.store);
        self.notifications.borrow_mut().push(handle.clone());
        handle
    }

    fn update_enable_notifications(&self, value: bool) -> Promise<()> {
        self.enabled.set(value);
        self.updated_enable_notifications.borrow_mut().push(value);
        Promise::resolved(())
    }

    fn log_warn(&self, message: &str) {
        self.log_messages.borrow_mut().push(message.to_owned());
    }

    fn write_to_process(&self, data: &str) {
        self.writes.borrow_mut().push(data.to_owned());
    }
}

/// O `setup` e o `teardown` da suíte.
struct Fixture {
    host: Rc<TestOsc99Host>,
    handler: Rc<TerminalNotificationHandler>,
    // Declarado por último: o `Drop` dele descarta o tratador e confere os vazamentos depois do `Drop` do `Fixture`.
    _guard: LeakGuard,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let notifications = self.host.notifications.borrow().clone();
        for notification in notifications {
            notification.close();
        }
    }
}

impl Fixture {
    fn new() -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let host = TestOsc99Host::new(guard.store());
        let handler = guard.add_rc(Rc::new(TerminalNotificationHandler::new(host.clone())));
        Self { host, handler, _guard: guard }
    }

    fn handle(&self, data: &str) {
        self.handler.handle_sequence(data);
    }

    /// `actions.primary` da notificação `index`, que o original exige não vazia.
    fn primary_actions(&self, index: usize) -> Vec<ActionRef> {
        let actions = self.host.notification(index).actions.borrow().clone();
        let primary = actions.map(|actions| actions.primary).unwrap_or_default();
        assert!(!primary.is_empty(), "Expected primary actions");
        primary
    }
}

#[test]
fn ignores_notifications_when_disabled() {
    let f = Fixture::new();
    f.host.enabled.set(false);

    f.handle(";Hello");
    assert_eq!(f.host.notification_count(), 0);
    assert_eq!(f.host.writes.borrow().len(), 0);
}

#[test]
fn creates_notification_for_title_and_body_and_updates() {
    let f = Fixture::new();
    f.handle("i=1:d=0:p=title;Hello");
    assert_eq!(f.host.notification_count(), 0);

    f.handle("i=1:p=body;World");
    assert_eq!(f.host.notification_count(), 1);
    assert_eq!(f.host.notification(0).message_text(), "Hello: World");
}

#[test]
fn decodes_base64_payloads() {
    let f = Fixture::new();
    f.handle("e=1:p=title;SGVsbG8=");
    assert_eq!(f.host.notification_count(), 1);
    assert_eq!(f.host.notification(0).message_text(), "Hello");
}

#[test]
fn sanitizes_markdown_links_in_payloads() {
    let f = Fixture::new();
    f.handle("i=link:d=0:p=title;Click [run](command:workbench.action.reloadWindow)");
    f.handle("i=link:p=body;See [docs](https://example.com)");
    assert_eq!(f.host.notification_count(), 1);
    assert_eq!(f.host.notification(0).message_text(), "Click run: See docs");
}

#[test]
fn sanitizes_nested_markdown_links_that_would_otherwise_reassemble_into_a_command_link() {
    let f = Fixture::new();
    f.handle("i=nested:p=title;[[ ](http://z)Update available - click to install](command:workbench.action.terminal.sendSequence?%7B%22text%22%3A%22calc%5Cr%22%7D)");
    assert_eq!(f.host.notification_count(), 1);
    assert_eq!(f.host.notification(0).message_text(), " Update available - click to install");
}

#[test]
fn defers_display_until_done() {
    let f = Fixture::new();
    f.handle("i=chunk:d=0:p=title;Hello ");
    assert_eq!(f.host.notification_count(), 0);

    f.handle("i=chunk:d=1:p=title;World");
    assert_eq!(f.host.notification_count(), 1);
    assert_eq!(f.host.notification(0).message_text(), "Hello World");
}

#[test]
fn reports_activation_on_button_click() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        f.handle("i=btn:d=0:a=report:p=title;Hi");
        f.handle("i=btn:p=buttons;Yes");

        let primary = f.primary_actions(0);
        primary[0].run(&[]).await.unwrap();
        assert_eq!(f.host.write(0), "\x1b]99;i=btn;1\x1b\\");
    });
}

#[test]
fn supports_buttons_before_title_and_reports_body_activation() {
    run_with_faked_timers(async {
        let f = Fixture::new();
        f.handle("i=btn:p=buttons;One\u{2028}Two");
        f.handle("i=btn:a=report;Buttons test");

        assert_eq!(f.host.notification_count(), 1);
        let actions = f.host.notification(0).actions.borrow().clone();
        let primary = actions.map(|actions| actions.primary).unwrap_or_default();
        assert!(primary.len() == 2, "Expected two primary actions");
        assert_eq!(primary[0].label(), "One");
        assert_eq!(primary[1].label(), "Two");

        primary[1].run(&[]).await.unwrap();
        assert_eq!(f.host.write(0), "\x1b]99;i=btn;2\x1b\\");
    });
}

#[test]
fn reports_activation_when_notification_closes_without_button_action() {
    let f = Fixture::new();
    f.handle("i=btn:p=buttons;One\u{2028}Two");
    f.handle("i=btn:a=report;Buttons test");

    f.host.notification(0).close();
    assert_eq!(f.host.write(0), "\x1b]99;i=btn;\x1b\\");
}

#[test]
fn sends_close_report_when_requested() {
    let f = Fixture::new();
    f.handle("i=close:c=1:p=title;Bye");
    assert_eq!(f.host.notification_count(), 1);
    f.host.notification(0).close();
    assert_eq!(f.host.write(0), "\x1b]99;i=close:p=close;\x1b\\");
}

#[test]
fn responds_to_query_and_alive() {
    let f = Fixture::new();
    f.handle("i=a:p=title;A");
    f.handle("i=b:p=title;B");
    f.handle("i=q:p=?;");
    f.handle("i=q:p=alive;");

    assert_eq!(f.host.write(0), "\x1b]99;i=q:p=?:a=report,focus:c=1:o=always,unfocused,invisible:p=title,body,buttons,close,alive,?:u=0,1,2:w=1;\x1b\\");
    assert_eq!(f.host.write(1), "\x1b]99;i=q:p=alive;a,b\x1b\\");
}

#[test]
fn honors_occasion_for_visibility_and_focus() {
    let f = Fixture::new();
    f.host.window_focused.set(true);
    f.host.terminal_visible.set(true);
    f.handle("o=unfocused:p=title;Hidden");
    assert_eq!(f.host.notification_count(), 0);

    f.host.window_focused.set(false);
    f.host.terminal_visible.set(true);
    f.handle("o=invisible:p=title;Hidden");
    assert_eq!(f.host.notification_count(), 0);

    f.host.terminal_visible.set(false);
    f.handle("o=invisible:p=title;Shown");
    assert_eq!(f.host.notification_count(), 1);
}

#[test]
fn closes_notifications_via_close_payload() {
    let f = Fixture::new();
    f.handle("i=closeme:p=title;Close");
    assert_eq!(f.host.notification_count(), 1);
    assert!(!f.host.notification(0).closed.get());

    f.handle("i=closeme:p=close;");
    assert!(f.host.notification(0).closed.get());
}

#[test]
fn maps_urgency_to_severity_and_priority() {
    let f = Fixture::new();
    f.handle("u=2:p=title;Urgent");
    assert_eq!(f.host.notification(0).severity.get(), Severity::Warning);
    assert_eq!(f.host.notification(0).priority, Some(NotificationPriority::Urgent));
}
