//! Porte de `src/vs/platform/keybinding/test/common/abstractKeybindingService.test.ts` (suíte `AbstractKeybindingService`), caso a caso e na mesma ordem.
//!
//! Divergências: o `TestKeybindingService extends AbstractKeybindingService` é um struct que implementa
//! `KeybindingServiceHost` (o trait que carrega o que a subclasse implementa); os serviços de contexto, de comandos e de
//! notificação são implementações de teste que gravam as chamadas, como os literais de objeto do original; o contexto
//! corrente (`currentContextValue`) é um `Rc<RefCell<..>>` compartilhado. O `StandardKeyboardEvent` composto vira o
//! `KeyboardEvent` com `KeyCode::KeyInComposition`.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::keybinding::{create_us_layout_resolved_keybinding, os};
use common::*;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::key_codes::{KeyCode, KeyMod, key_chord};
use je_base::keybindings::{BinaryKeybinding, KeyCodeChord, Keybinding, ResolvedKeybinding, create_simple_keybinding};
use je_base::lifecycle::{DisposableRef, IDisposable, none};
use je_base::promise::Promise;
use je_platform::commands::ICommandService;
use je_platform::contextkey::{
    ContextKeyExpr, ContextKeyExpression, IContext, IContextKey, IContextKeyService, IContextKeyServiceTarget, IScopedContextKeyService,
};
use je_platform::contextkey::IContextKeyChangeEvent;
use je_platform::keybinding::abstract_keybinding_service::{KeybindingServiceHost, KeybindingServiceState, dispatch_internal};
use je_platform::keybinding::keybinding_resolver::{KeybindingResolver, ResolutionResult, ResultKind};
use je_platform::keybinding::resolved_keybinding_item::ResolvedKeybindingItem;
use je_platform::keybinding::us_layout_resolved_keybinding::USLayoutResolvedKeybinding;
use je_platform::keybinding::{IKeybindingService, KeyboardEvent, KeybindingsSchemaContribution};
use je_platform::log::NullLogService;
use je_platform::notification::{
    INotificationService, Notification, NotificationFilterSetting, NotificationHandle, NotificationMessage, NotificationSource, NotificationSourceFilter,
    NotificationsFilter, NoOpNotification, PromptChoiceKind, PromptOptions, Severity, StatusHandle, StatusMessageOptions,
};
use je_platform::telemetry::null_telemetry_service;

struct TestContext(Dynamic);

impl IContext for TestContext {
    fn get_value(&self, key: &str) -> Dynamic {
        self.0.get_str(key)
    }
}

type CurrentContext = Rc<RefCell<Option<Rc<dyn IContext>>>>;

fn create_context(ctx: &str) -> Option<Rc<dyn IContext>> {
    Some(Rc::new(TestContext(j(ctx))))
}

/// O `contextKeyService` literal do original: só `contextMatchesRules`, `getContext` e `bufferChangeEvents` funcionam.
struct TestContextKeyService {
    current: CurrentContext,
}

impl IContextKeyService for TestContextKeyService {
    fn on_did_change_context(&self) -> Event<Rc<dyn IContextKeyChangeEvent>> {
        Event::none()
    }
    fn buffer_change_events(&self, _callback: &dyn Fn()) {}
    fn create_key(&self, _key: &str, _default_value: Dynamic) -> Rc<dyn IContextKey> {
        unimplemented!()
    }
    fn context_matches_rules(&self, rules: Option<&ContextKeyExpression>) -> bool {
        let Some(rules) = rules else { return true };
        match &*self.current.borrow() {
            None => false,
            Some(current) => rules.evaluate(&**current),
        }
    }
    fn get_context_key_value(&self, _key: &str) -> Dynamic {
        unimplemented!()
    }
    fn create_scoped(&self, _target: Rc<dyn IContextKeyServiceTarget>) -> Rc<dyn IScopedContextKeyService> {
        unimplemented!()
    }
    fn create_overlay(&self, _overlay: Vec<(String, Dynamic)>) -> Rc<dyn IContextKeyService> {
        unimplemented!()
    }
    fn get_context(&self, _target: Option<Rc<dyn IContextKeyServiceTarget>>) -> Rc<dyn IContext> {
        match &*self.current.borrow() {
            Some(current) => current.clone(),
            None => Rc::new(TestContext(Dynamic::Undefined)),
        }
    }
    fn update_parent(&self, _parent: Rc<dyn IContextKeyService>) {}
    fn context_id(&self) -> i64 {
        0
    }
    fn get_context_values_container(&self, _context_id: i64) -> Rc<dyn IContext> {
        self.get_context(None)
    }
}

#[derive(Clone, Debug, PartialEq)]
struct ExecuteCall {
    command_id: String,
    args: Vec<Dynamic>,
}

#[derive(Default)]
struct Recorded {
    execute_command_calls: Vec<ExecuteCall>,
    show_message_calls: Vec<(Severity, String)>,
    status_message_calls: Vec<String>,
    status_message_calls_disposed: Vec<String>,
}

type Calls = Rc<RefCell<Recorded>>;

struct TestCommandService {
    calls: Calls,
}

impl ICommandService for TestCommandService {
    fn on_will_execute_command(&self) -> Event<je_platform::commands::CommandEvent> {
        Event::none()
    }
    fn on_did_execute_command(&self) -> Event<je_platform::commands::CommandEvent> {
        Event::none()
    }
    fn execute_command(&self, command_id: &str, args: Vec<Dynamic>) -> Promise<Dynamic> {
        self.calls.borrow_mut().execute_command_calls.push(ExecuteCall { command_id: command_id.to_string(), args });
        Promise::resolved(Dynamic::Undefined)
    }
}

fn message_text(message: &NotificationMessage) -> String {
    match message {
        NotificationMessage::Text(text) => text.clone(),
        other => format!("{other:?}"),
    }
}

struct TestNotificationService {
    calls: Calls,
}

struct RecordingStatus {
    message: String,
    calls: Calls,
}

impl StatusHandle for RecordingStatus {
    fn close(&self) {
        self.calls.borrow_mut().status_message_calls_disposed.push(self.message.clone());
    }
}

impl INotificationService for TestNotificationService {
    fn on_did_change_filter(&self) -> Event<()> {
        Event::none()
    }
    fn set_filter(&self, _filter: NotificationFilterSetting) {
        unimplemented!()
    }
    fn get_filter(&self, _source: Option<&NotificationSource>) -> NotificationsFilter {
        unimplemented!()
    }
    fn get_filters(&self) -> Vec<NotificationSourceFilter> {
        unimplemented!()
    }
    fn remove_filter(&self, _source_id: &str) {
        unimplemented!()
    }
    fn notify(&self, notification: Notification) -> Rc<dyn NotificationHandle> {
        self.calls.borrow_mut().show_message_calls.push((notification.severity, message_text(&notification.message)));
        Rc::new(NoOpNotification::new())
    }
    fn info(&self, message: Vec<NotificationMessage>) {
        self.calls.borrow_mut().show_message_calls.push((Severity::Info, message_text(&message[0])));
    }
    fn warn(&self, message: Vec<NotificationMessage>) {
        self.calls.borrow_mut().show_message_calls.push((Severity::Warning, message_text(&message[0])));
    }
    fn error(&self, message: Vec<NotificationMessage>) {
        self.calls.borrow_mut().show_message_calls.push((Severity::Error, message_text(&message[0])));
    }
    fn prompt(&self, _severity: Severity, _message: NotificationMessage, _choices: Vec<PromptChoiceKind>, _options: Option<PromptOptions>) -> Rc<dyn NotificationHandle> {
        panic!("not implemented")
    }
    fn status(&self, message: NotificationMessage, _options: Option<StatusMessageOptions>) -> Rc<dyn StatusHandle> {
        let message = message_text(&message);
        self.calls.borrow_mut().status_message_calls.push(message.clone());
        Rc::new(RecordingStatus { message, calls: self.calls.clone() })
    }
}

struct TestKeybindingService {
    state: KeybindingServiceState,
    resolver: Rc<KeybindingResolver>,
}

impl KeybindingServiceHost for TestKeybindingService {
    fn state(&self) -> &KeybindingServiceState {
        &self.state
    }

    fn get_resolver(&self) -> Rc<KeybindingResolver> {
        self.resolver.clone()
    }

    fn resolve_keybinding(&self, kb: &Keybinding) -> Vec<Rc<dyn ResolvedKeybinding>> {
        USLayoutResolvedKeybinding::resolve_keybinding(kb, os())
    }

    fn resolve_keyboard_event(&self, keyboard_event: &KeyboardEvent) -> Rc<dyn ResolvedKeybinding> {
        let chord = KeyCodeChord::new(
            keyboard_event.ctrl_key,
            keyboard_event.shift_key,
            keyboard_event.alt_key,
            keyboard_event.meta_key,
            keyboard_event.key_code,
        )
        .to_keybinding();
        KeybindingServiceHost::resolve_keybinding(self, &chord).remove(0)
    }

    fn resolve_user_binding(&self, _user_binding: &str) -> Vec<Rc<dyn ResolvedKeybinding>> {
        Vec::new()
    }

    fn register_schema_contribution(&self, _contribution: Rc<dyn KeybindingsSchemaContribution>) -> DisposableRef {
        none()
    }

    fn dump_debug_info(&self) -> String {
        String::new()
    }

    fn dump_debug_info_json(&self) -> String {
        String::new()
    }

    fn enable_keybinding_hold_mode(&self, _command_id: &str) -> Option<Promise<()>> {
        None
    }
}

impl IDisposable for TestKeybindingService {
    fn dispose(&self) {
        self.state.dispose();
    }
}

impl TestKeybindingService {
    fn test_dispatch(&self, kb: u32, is_composing: bool) -> bool {
        dispatch_internal(self, &self.to_keyboard_event(kb, is_composing), None)
    }

    fn test_soft_dispatch(&self, kb: u32, is_composing: bool) -> ResolutionResult {
        IKeybindingService::soft_dispatch(self, &self.to_keyboard_event(kb, is_composing), None)
    }

    fn to_keyboard_event(&self, kb: u32, is_composing: bool) -> KeyboardEvent {
        let keybinding = create_simple_keybinding(kb, os());
        KeyboardEvent {
            ctrl_key: keybinding.ctrl_key,
            shift_key: keybinding.shift_key,
            alt_key: keybinding.alt_key,
            meta_key: keybinding.meta_key,
            alt_graph_key: false,
            // `StandardKeyboardEvent` normalizes composing keystrokes to KEY_IN_COMPOSITION.
            key_code: if is_composing { KeyCode::KeyInComposition } else { keybinding.key_code },
            code: None,
        }
    }
}

struct Fixture {
    _guard: LeakGuard,
    current_context_value: CurrentContext,
    calls: Calls,
}

impl Fixture {
    fn new() -> Self {
        Self { _guard: ensure_no_disposables_are_leaked_in_test_suite(), current_context_value: Rc::new(RefCell::new(None)), calls: Rc::new(RefCell::new(Recorded::default())) }
    }

    fn create_test_keybinding_service(&self, items: Vec<Rc<ResolvedKeybindingItem>>) -> TestKeybindingService {
        let context_key_service: Rc<dyn IContextKeyService> = Rc::new(TestContextKeyService { current: self.current_context_value.clone() });
        let command_service: Rc<dyn ICommandService> = Rc::new(TestCommandService { calls: self.calls.clone() });
        let notification_service: Rc<dyn INotificationService> = Rc::new(TestNotificationService { calls: self.calls.clone() });
        let resolver = Rc::new(KeybindingResolver::new(items, vec![], Rc::new(|_| {})));
        TestKeybindingService {
            state: KeybindingServiceState::new(
                context_key_service,
                command_service,
                null_telemetry_service(),
                notification_service,
                NullLogService::shared(),
                Rc::new(|| true),
            ),
            resolver,
        }
    }

    fn set_context(&self, ctx: &str) {
        *self.current_context_value.borrow_mut() = create_context(ctx);
    }

    fn reset_calls(&self) {
        *self.calls.borrow_mut() = Recorded::default();
    }

    fn execute_command_calls(&self) -> Vec<ExecuteCall> {
        self.calls.borrow().execute_command_calls.clone()
    }

    fn show_message_calls(&self) -> Vec<(Severity, String)> {
        self.calls.borrow().show_message_calls.clone()
    }

    fn status_message_calls(&self) -> Vec<String> {
        self.calls.borrow().status_message_calls.clone()
    }

    fn status_message_calls_disposed(&self) -> Vec<String> {
        self.calls.borrow().status_message_calls_disposed.clone()
    }

    fn assert_no_side_effects(&self) {
        assert_eq!(self.execute_command_calls(), vec![]);
        assert_eq!(self.show_message_calls(), vec![]);
        assert_eq!(self.status_message_calls(), Vec::<String>::new());
        assert_eq!(self.status_message_calls_disposed(), Vec::<String>::new());
    }
}

fn kb_item(keybinding: impl Into<BinaryKeybinding>, command: Option<&str>, when: Option<ContextKeyExpression>) -> Rc<ResolvedKeybindingItem> {
    ResolvedKeybindingItem::new(create_us_layout_resolved_keybinding(keybinding, os()), command, Dynamic::Null, when, true, None, false, false)
}

fn to_us_label(keybinding: u32) -> String {
    create_us_layout_resolved_keybinding(keybinding, os()).unwrap().get_label().unwrap()
}

fn code(key_code: KeyCode) -> u32 {
    key_code.0 as u32
}

const CTRL_CMD: u32 = KeyMod::CTRL_CMD;

fn call(command_id: &str) -> ExecuteCall {
    ExecuteCall { command_id: command_id.to_string(), args: vec![Dynamic::Null] }
}

fn waiting_second(chord: u32) -> String {
    format!("({}) was pressed. Waiting for second key of chord...", to_us_label(chord))
}

mod simple_tests_single_and_multi_chord_keybindings_are_dispatched {
    use super::*;

    #[test]
    fn a_single_chord_keybinding_is_dispatched_correctly_this_test_makes_sure_the_dispatch_in_general_works_before_we_test_empty_string_null_command_id() {
        let f = Fixture::new();
        let key = CTRL_CMD | code(KeyCode::KeyK);
        let kb_service = f.create_test_keybinding_service(vec![kb_item(key, Some("myCommand"), None)]);

        f.set_context("{}");
        let should_prevent_default = kb_service.test_dispatch(key, false);
        assert!(should_prevent_default);
        assert_eq!(f.execute_command_calls(), vec![call("myCommand")]);
        assert_eq!(f.show_message_calls(), vec![]);
        assert_eq!(f.status_message_calls(), Vec::<String>::new());
        assert_eq!(f.status_message_calls_disposed(), Vec::<String>::new());

        kb_service.dispose();
    }

    #[test]
    fn a_multi_chord_keybinding_is_dispatched_correctly() {
        let f = Fixture::new();
        let chord0 = CTRL_CMD | code(KeyCode::KeyK);
        let chord1 = CTRL_CMD | code(KeyCode::KeyI);
        let key = vec![chord0, chord1];
        let kb_service = f.create_test_keybinding_service(vec![kb_item(key, Some("myCommand"), None)]);

        f.set_context("{}");
        let mut should_prevent_default = kb_service.test_dispatch(chord0, false);
        assert!(should_prevent_default);
        assert_eq!(f.execute_command_calls(), vec![]);
        assert_eq!(f.show_message_calls(), vec![]);
        assert_eq!(f.status_message_calls(), vec![waiting_second(chord0)]);
        assert_eq!(f.status_message_calls_disposed(), Vec::<String>::new());

        should_prevent_default = kb_service.test_dispatch(chord1, false);
        assert!(should_prevent_default);
        assert_eq!(f.execute_command_calls(), vec![call("myCommand")]);
        assert_eq!(f.show_message_calls(), vec![]);
        assert_eq!(f.status_message_calls(), vec![waiting_second(chord0)]);
        assert_eq!(f.status_message_calls_disposed(), vec![waiting_second(chord0)]);

        kb_service.dispose();
    }
}

mod keybindings_with_empty_string_null_command_id {
    use super::*;

    fn single_chord_unbinds(command: Option<&str>) {
        let f = Fixture::new();
        let kb_service = f.create_test_keybinding_service(vec![
            kb_item(CTRL_CMD | code(KeyCode::KeyK), Some("myCommand"), None),
            kb_item(CTRL_CMD | code(KeyCode::KeyK), command, None),
        ]);

        // send Ctrl/Cmd + K
        f.set_context("{}");
        let should_prevent_default = kb_service.test_dispatch(CTRL_CMD | code(KeyCode::KeyK), false);
        assert!(!should_prevent_default);
        f.assert_no_side_effects();

        kb_service.dispose();
    }

    fn multi_chord_keeps(command: Option<&str>) {
        let f = Fixture::new();
        let chord0 = CTRL_CMD | code(KeyCode::KeyK);
        let chord1 = CTRL_CMD | code(KeyCode::KeyI);
        let key = vec![chord0, chord1];
        let kb_service = f.create_test_keybinding_service(vec![kb_item(key.clone(), Some("myCommand"), None), kb_item(key, command, None)]);

        f.set_context("{}");
        let mut should_prevent_default = kb_service.test_dispatch(CTRL_CMD | code(KeyCode::KeyK), false);
        assert!(should_prevent_default);
        assert_eq!(f.execute_command_calls(), vec![]);
        assert_eq!(f.show_message_calls(), vec![]);
        assert_eq!(f.status_message_calls(), vec![waiting_second(chord0)]);
        assert_eq!(f.status_message_calls_disposed(), Vec::<String>::new());

        should_prevent_default = kb_service.test_dispatch(CTRL_CMD | code(KeyCode::KeyI), false);
        assert!(should_prevent_default);
        assert_eq!(f.execute_command_calls(), vec![]);
        assert_eq!(f.show_message_calls(), vec![]);
        assert_eq!(
            f.status_message_calls(),
            vec![waiting_second(chord0), format!("The key combination ({}, {}) is not a command.", to_us_label(chord0), to_us_label(chord1))]
        );
        assert_eq!(f.status_message_calls_disposed(), vec![waiting_second(chord0)]);

        kb_service.dispose();
    }

    #[test]
    fn a_single_chord_keybinding_with_an_empty_string_command_id_unbinds_the_keybinding_should_prevent_default_false() {
        single_chord_unbinds(Some(""));
    }

    #[test]
    fn a_single_chord_keybinding_with_a_null_command_id_unbinds_the_keybinding_should_prevent_default_false() {
        single_chord_unbinds(None);
    }

    #[test]
    fn a_multi_chord_keybinding_with_an_empty_string_command_id_keeps_the_keybinding_should_prevent_default_true() {
        multi_chord_keeps(Some(""));
    }

    #[test]
    fn a_multi_chord_keybinding_with_a_null_command_id_keeps_the_keybinding_should_prevent_default_true() {
        multi_chord_keeps(None);
    }
}

#[test]
fn issue_16498_chord_mode_is_quit_for_invalid_chords() {
    let f = Fixture::new();
    let kb_service = f.create_test_keybinding_service(vec![
        kb_item(key_chord(CTRL_CMD | code(KeyCode::KeyK), CTRL_CMD | code(KeyCode::KeyX)), Some("chordCommand"), None),
        kb_item(code(KeyCode::Backspace), Some("simpleCommand"), None),
    ]);

    // send Ctrl/Cmd + K
    let mut should_prevent_default = kb_service.test_dispatch(CTRL_CMD | code(KeyCode::KeyK), false);
    assert!(should_prevent_default);
    assert_eq!(f.execute_command_calls(), vec![]);
    assert_eq!(f.show_message_calls(), vec![]);
    assert_eq!(f.status_message_calls(), vec![waiting_second(CTRL_CMD | code(KeyCode::KeyK))]);
    assert_eq!(f.status_message_calls_disposed(), Vec::<String>::new());
    f.reset_calls();

    // send backspace
    should_prevent_default = kb_service.test_dispatch(code(KeyCode::Backspace), false);
    assert!(should_prevent_default);
    assert_eq!(f.execute_command_calls(), vec![]);
    assert_eq!(f.show_message_calls(), vec![]);
    assert_eq!(
        f.status_message_calls(),
        vec![format!("The key combination ({}, {}) is not a command.", to_us_label(CTRL_CMD | code(KeyCode::KeyK)), to_us_label(code(KeyCode::Backspace)))]
    );
    assert_eq!(f.status_message_calls_disposed(), vec![waiting_second(CTRL_CMD | code(KeyCode::KeyK))]);
    f.reset_calls();

    // send backspace
    should_prevent_default = kb_service.test_dispatch(code(KeyCode::Backspace), false);
    assert!(should_prevent_default);
    assert_eq!(f.execute_command_calls(), vec![call("simpleCommand")]);
    assert_eq!(f.show_message_calls(), vec![]);
    assert_eq!(f.status_message_calls(), Vec::<String>::new());
    assert_eq!(f.status_message_calls_disposed(), Vec::<String>::new());
    f.reset_calls();

    kb_service.dispose();
}

#[test]
fn issue_16833_keybinding_service_should_not_test_dispatch_on_modifier_keys() {
    let f = Fixture::new();
    let kb_service = f.create_test_keybinding_service(vec![
        kb_item(code(KeyCode::Ctrl), Some("nope"), None),
        kb_item(code(KeyCode::Meta), Some("nope"), None),
        kb_item(code(KeyCode::Alt), Some("nope"), None),
        kb_item(code(KeyCode::Shift), Some("nope"), None),
        kb_item(CTRL_CMD, Some("nope"), None),
        kb_item(KeyMod::WIN_CTRL, Some("nope"), None),
        kb_item(KeyMod::ALT, Some("nope"), None),
        kb_item(KeyMod::SHIFT, Some("nope"), None),
    ]);

    let assert_is_ignored = |keybinding: u32| {
        let should_prevent_default = kb_service.test_dispatch(keybinding, false);
        assert!(!should_prevent_default);
        f.assert_no_side_effects();
        f.reset_calls();
    };

    assert_is_ignored(code(KeyCode::Ctrl));
    assert_is_ignored(code(KeyCode::Meta));
    assert_is_ignored(code(KeyCode::Alt));
    assert_is_ignored(code(KeyCode::Shift));
    assert_is_ignored(CTRL_CMD);
    assert_is_ignored(KeyMod::WIN_CTRL);
    assert_is_ignored(KeyMod::ALT);
    assert_is_ignored(KeyMod::SHIFT);

    kb_service.dispose();
}

#[test]
fn keybindings_are_not_dispatched_while_an_ime_composition_is_in_progress() {
    let f = Fixture::new();
    let kb_service = f.create_test_keybinding_service(vec![kb_item(code(KeyCode::Enter), Some("enterCommand"), None)]);

    // Enter commits the IME composition and belongs to the input method, not to the workbench.
    let should_prevent_default_while_composing = kb_service.test_dispatch(code(KeyCode::Enter), true);
    assert_eq!((should_prevent_default_while_composing, f.execute_command_calls()), (false, vec![]));

    // `softDispatch` must agree, otherwise callers that ask "will the workbench claim this key?" prevent the
    // default and then nobody handles the keystroke.
    assert_eq!(kb_service.test_soft_dispatch(code(KeyCode::Enter), true).kind(), ResultKind::NoMatchingKb);

    // Once the composition has committed, the very same key runs the command as usual.
    let should_prevent_default = kb_service.test_dispatch(code(KeyCode::Enter), false);
    assert_eq!((should_prevent_default, f.execute_command_calls()), (true, vec![call("enterCommand")]));
    assert_eq!(kb_service.test_soft_dispatch(code(KeyCode::Enter), false).kind(), ResultKind::KbFound);

    kb_service.dispose();
}

#[test]
fn can_trigger_command_that_is_sharing_keybinding_with_chord() {
    let f = Fixture::new();
    let kb_service = f.create_test_keybinding_service(vec![
        kb_item(key_chord(CTRL_CMD | code(KeyCode::KeyK), CTRL_CMD | code(KeyCode::KeyX)), Some("chordCommand"), None),
        kb_item(CTRL_CMD | code(KeyCode::KeyK), Some("simpleCommand"), Some(ContextKeyExpr::has("key1"))),
    ]);

    // send Ctrl/Cmd + K
    f.set_context("{'key1': true}");
    let mut should_prevent_default = kb_service.test_dispatch(CTRL_CMD | code(KeyCode::KeyK), false);
    assert!(should_prevent_default);
    assert_eq!(f.execute_command_calls(), vec![call("simpleCommand")]);
    assert_eq!(f.show_message_calls(), vec![]);
    assert_eq!(f.status_message_calls(), Vec::<String>::new());
    assert_eq!(f.status_message_calls_disposed(), Vec::<String>::new());
    f.reset_calls();

    // send Ctrl/Cmd + K
    f.set_context("{}");
    should_prevent_default = kb_service.test_dispatch(CTRL_CMD | code(KeyCode::KeyK), false);
    assert!(should_prevent_default);
    assert_eq!(f.execute_command_calls(), vec![]);
    assert_eq!(f.show_message_calls(), vec![]);
    assert_eq!(f.status_message_calls(), vec![waiting_second(CTRL_CMD | code(KeyCode::KeyK))]);
    assert_eq!(f.status_message_calls_disposed(), Vec::<String>::new());
    f.reset_calls();

    // send Ctrl/Cmd + X
    f.set_context("{}");
    should_prevent_default = kb_service.test_dispatch(CTRL_CMD | code(KeyCode::KeyX), false);
    assert!(should_prevent_default);
    assert_eq!(f.execute_command_calls(), vec![call("chordCommand")]);
    assert_eq!(f.show_message_calls(), vec![]);
    assert_eq!(f.status_message_calls(), Vec::<String>::new());
    assert_eq!(f.status_message_calls_disposed(), vec![waiting_second(CTRL_CMD | code(KeyCode::KeyK))]);
    f.reset_calls();

    kb_service.dispose();
}

#[test]
fn cannot_trigger_chord_if_command_is_overwriting() {
    let f = Fixture::new();
    let kb_service = f.create_test_keybinding_service(vec![
        kb_item(key_chord(CTRL_CMD | code(KeyCode::KeyK), CTRL_CMD | code(KeyCode::KeyX)), Some("chordCommand"), Some(ContextKeyExpr::has("key1"))),
        kb_item(CTRL_CMD | code(KeyCode::KeyK), Some("simpleCommand"), None),
    ]);

    // send Ctrl/Cmd + K
    f.set_context("{}");
    let mut should_prevent_default = kb_service.test_dispatch(CTRL_CMD | code(KeyCode::KeyK), false);
    assert!(should_prevent_default);
    assert_eq!(f.execute_command_calls(), vec![call("simpleCommand")]);
    assert_eq!(f.show_message_calls(), vec![]);
    assert_eq!(f.status_message_calls(), Vec::<String>::new());
    assert_eq!(f.status_message_calls_disposed(), Vec::<String>::new());
    f.reset_calls();

    // send Ctrl/Cmd + K
    f.set_context("{'key1': true}");
    should_prevent_default = kb_service.test_dispatch(CTRL_CMD | code(KeyCode::KeyK), false);
    assert!(should_prevent_default);
    assert_eq!(f.execute_command_calls(), vec![call("simpleCommand")]);
    assert_eq!(f.show_message_calls(), vec![]);
    assert_eq!(f.status_message_calls(), Vec::<String>::new());
    assert_eq!(f.status_message_calls_disposed(), Vec::<String>::new());
    f.reset_calls();

    // send Ctrl/Cmd + X
    f.set_context("{'key1': true}");
    should_prevent_default = kb_service.test_dispatch(CTRL_CMD | code(KeyCode::KeyX), false);
    assert!(!should_prevent_default);
    f.assert_no_side_effects();
    f.reset_calls();

    kb_service.dispose();
}

#[test]
fn can_have_spying_command() {
    let f = Fixture::new();
    let kb_service = f.create_test_keybinding_service(vec![kb_item(CTRL_CMD | code(KeyCode::KeyK), Some("^simpleCommand"), None)]);

    // send Ctrl/Cmd + K
    f.set_context("{}");
    let should_prevent_default = kb_service.test_dispatch(CTRL_CMD | code(KeyCode::KeyK), false);
    assert!(!should_prevent_default);
    assert_eq!(f.execute_command_calls(), vec![call("simpleCommand")]);
    assert_eq!(f.show_message_calls(), vec![]);
    assert_eq!(f.status_message_calls(), Vec::<String>::new());
    assert_eq!(f.status_message_calls_disposed(), Vec::<String>::new());
    f.reset_calls();

    kb_service.dispose();
}

mod append_keybinding {
    use super::*;

    fn ctrl_k() -> u32 {
        CTRL_CMD | code(KeyCode::KeyK)
    }

    #[test]
    fn appends_keybinding_label_when_command_has_a_keybinding() {
        let f = Fixture::new();
        let kb_service = f.create_test_keybinding_service(vec![kb_item(ctrl_k(), Some("myCommand"), None)]);
        let result = kb_service.append_keybinding("My Label", Some("myCommand"), None, false);
        assert_eq!(result, format!("My Label ({})", to_us_label(ctrl_k())));
        kb_service.dispose();
    }

    #[test]
    fn returns_only_label_when_command_has_no_keybinding() {
        let f = Fixture::new();
        let kb_service = f.create_test_keybinding_service(vec![]);
        assert_eq!(kb_service.append_keybinding("My Label", Some("myCommand"), None, false), "My Label");
        kb_service.dispose();
    }

    #[test]
    fn returns_only_label_when_command_id_is_null() {
        let f = Fixture::new();
        let kb_service = f.create_test_keybinding_service(vec![kb_item(ctrl_k(), Some("myCommand"), None)]);
        assert_eq!(kb_service.append_keybinding("My Label", None, None, false), "My Label");
        kb_service.dispose();
    }

    #[test]
    fn returns_only_label_when_command_id_is_undefined() {
        let f = Fixture::new();
        let kb_service = f.create_test_keybinding_service(vec![kb_item(ctrl_k(), Some("myCommand"), None)]);
        assert_eq!(kb_service.append_keybinding("My Label", None, None, false), "My Label");
        kb_service.dispose();
    }

    #[test]
    fn returns_only_label_when_command_id_is_empty_string() {
        let f = Fixture::new();
        let kb_service = f.create_test_keybinding_service(vec![kb_item(ctrl_k(), Some("myCommand"), None)]);
        assert_eq!(kb_service.append_keybinding("My Label", Some(""), None, false), "My Label");
        kb_service.dispose();
    }

    #[test]
    fn appends_keybinding_for_command_with_context_when_context_matches() {
        let f = Fixture::new();
        let kb_service = f.create_test_keybinding_service(vec![kb_item(ctrl_k(), Some("myCommand"), Some(ContextKeyExpr::has("key1")))]);
        f.set_context("{'key1': true}");
        assert_eq!(kb_service.append_keybinding("My Label", Some("myCommand"), None, false), format!("My Label ({})", to_us_label(ctrl_k())));
        kb_service.dispose();
    }

    #[test]
    fn returns_only_label_when_context_does_not_match_and_enforce_context_check_is_true() {
        let f = Fixture::new();
        let kb_service = f.create_test_keybinding_service(vec![kb_item(ctrl_k(), Some("myCommand"), Some(ContextKeyExpr::has("key1")))]);
        f.set_context("{}");
        assert_eq!(kb_service.append_keybinding("My Label", Some("myCommand"), None, true), "My Label");
        kb_service.dispose();
    }

    #[test]
    fn appends_keybinding_when_context_does_not_match_but_enforce_context_check_is_false() {
        let f = Fixture::new();
        let kb_service = f.create_test_keybinding_service(vec![kb_item(ctrl_k(), Some("myCommand"), Some(ContextKeyExpr::has("key1")))]);
        f.set_context("{}");
        assert_eq!(kb_service.append_keybinding("My Label", Some("myCommand"), None, false), format!("My Label ({})", to_us_label(ctrl_k())));
        kb_service.dispose();
    }

    #[test]
    fn appends_keybinding_even_when_label_is_empty_string() {
        let f = Fixture::new();
        let kb_service = f.create_test_keybinding_service(vec![kb_item(ctrl_k(), Some("myCommand"), None)]);
        assert_eq!(kb_service.append_keybinding("", Some("myCommand"), None, false), format!(" ({})", to_us_label(ctrl_k())));
        kb_service.dispose();
    }
}
