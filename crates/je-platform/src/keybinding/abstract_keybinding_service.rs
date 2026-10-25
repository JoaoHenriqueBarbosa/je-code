//! Porte de `src/vs/platform/keybinding/common/abstractKeybindingService.ts`: a lógica de acordes, o despacho
//! de teclado e o de modificador único.
//!
//! A classe abstrata vira o trait [`KeybindingServiceHost`]: o que a subclasse implementa (`_getResolver`,
//! `resolveKeybinding`, `resolveKeyboardEvent`...) e o estado ([`KeybindingServiceState`], que guarda os campos
//! da classe base). O resto do `IKeybindingService` é o que a classe base implementa, num `impl` em bloco para todo
//! `KeybindingServiceHost`. O `_dispatch` e o `_singleModifierDispatch` (protegidos no original) são
//! [`dispatch_internal`] e [`single_modifier_dispatch`].
//!
//! Divergências: o `_documentHasFocus` abstrato é uma função entregue ao estado (o temporizador que confere o foco
//! precisa dela fora do `&self` do serviço); `console.warn` vira `eprintln!`; o `Promise` do `executeCommand` que
//! falha vira um aviso no serviço de notificações quando a promessa rejeita.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::async_::{IntervalTimer, TimeoutTimer};
use je_base::ime::ime;
use je_base::key_codes::KeyCode;
use je_base::keybindings::{Keybinding, ResolvedChord, ResolvedKeybinding, SingleModifierChord};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::runtime;
use zyarr::RegExp;

use super::keybinding::{IKeybindingService, KeyboardEvent, KeybindingsSchemaContribution};
use super::keybinding_resolver::{KeybindingResolver, ResolutionResult};
use super::resolved_keybinding_item::{ResolvedKeybindingItem, single_modifier_name};
use crate::commands::ICommandService;
use crate::contextkey::{IContextKeyService, IContextKeyServiceTarget};
use crate::log::{LogArg, LogServiceRef};
use crate::notification::{INotificationService, NotificationMessage, Severity, StatusHandle, StatusMessageOptions};
use crate::telemetry::ITelemetryService;

/// `CurrentChord`.
#[derive(Clone)]
struct CurrentChord {
    keypress: String,
    label: Option<String>,
}

/// `HIGH_FREQ_COMMANDS`.
const HIGH_FREQ_COMMANDS: &str = "^(cursor|delete|undo|redo|tab|editor\\.action\\.clipboard)";

/// Se a tecla pertence a uma composição de IME em andamento. O `StandardKeyboardEvent` normaliza toda tecla
/// de composição para `KeyCode::KeyInComposition`, inclusive as combinações que reportariam o código real.
fn is_key_in_composition(e: &KeyboardEvent) -> bool {
    e.key_code == KeyCode::KeyInComposition
}

/// `KeybindingModifierSet`.
#[derive(Clone, Copy, Default)]
struct KeybindingModifierSet {
    ctrl_key: bool,
    shift_key: bool,
    alt_key: bool,
    meta_key: bool,
}

impl KeybindingModifierSet {
    fn new(source: Option<&ResolvedChord>) -> Self {
        match source {
            Some(s) => Self { ctrl_key: s.ctrl_key, shift_key: s.shift_key, alt_key: s.alt_key, meta_key: s.meta_key },
            None => Self::default(),
        }
    }

    fn has(&self, modifier: SingleModifierChord) -> bool {
        match modifier {
            SingleModifierChord::Ctrl => self.ctrl_key,
            SingleModifierChord::Shift => self.shift_key,
            SingleModifierChord::Alt => self.alt_key,
            SingleModifierChord::Meta => self.meta_key,
        }
    }
}

/// O estado que a classe base do original guarda em campos.
struct StateInner {
    context_key_service: Rc<dyn IContextKeyService>,
    command_service: Rc<dyn ICommandService>,
    telemetry_service: Rc<dyn ITelemetryService>,
    notification_service: Rc<dyn INotificationService>,
    log_service: LogServiceRef,
    document_has_focus: Rc<dyn Fn() -> bool>,
    /// Teclas recentes que podem disparar um atalho. Se há `cmd+k cmd+i` e o usuário pressionou `cmd+k`, ele fica
    /// aqui até o `cmd+i` chegar e o serviço invocar o comando.
    current_chords: RefCell<Vec<CurrentChord>>,
    current_chord_checker: Rc<IntervalTimer>,
    current_chord_status_message: RefCell<Option<Rc<dyn StatusHandle>>>,
    ignore_single_modifiers: Cell<KeybindingModifierSet>,
    current_single_modifier: Cell<Option<SingleModifierChord>>,
    current_single_modifier_clear_timeout: Rc<TimeoutTimer>,
    currently_dispatching_command_id: RefCell<Option<String>>,
    logging: Cell<bool>,
}

/// O estado da classe base `AbstractKeybindingService`.
pub struct KeybindingServiceState {
    inner: Rc<StateInner>,
    base: Rc<Disposable>,
    on_did_update_keybindings: Rc<Emitter<()>>,
}

impl KeybindingServiceState {
    pub fn new(
        context_key_service: Rc<dyn IContextKeyService>,
        command_service: Rc<dyn ICommandService>,
        telemetry_service: Rc<dyn ITelemetryService>,
        notification_service: Rc<dyn INotificationService>,
        log_service: LogServiceRef,
        document_has_focus: Rc<dyn Fn() -> bool>,
    ) -> Self {
        let base = Disposable::new();
        let on_did_update_keybindings = Emitter::new();
        base.register(on_did_update_keybindings.clone());
        Self {
            inner: Rc::new(StateInner {
                context_key_service,
                command_service,
                telemetry_service,
                notification_service,
                log_service,
                document_has_focus,
                current_chords: RefCell::new(Vec::new()),
                current_chord_checker: IntervalTimer::new(),
                current_chord_status_message: RefCell::new(None),
                ignore_single_modifiers: Cell::new(KeybindingModifierSet::default()),
                current_single_modifier: Cell::new(None),
                current_single_modifier_clear_timeout: TimeoutTimer::new(),
                currently_dispatching_command_id: RefCell::new(None),
                logging: Cell::new(false),
            }),
            base,
            on_did_update_keybindings,
        }
    }

    /// `_onDidUpdateKeybindings`: a subclasse dispara quando os atalhos mudam.
    pub fn on_did_update_keybindings_emitter(&self) -> &Rc<Emitter<()>> {
        &self.on_did_update_keybindings
    }

    /// `_register(o)`.
    pub fn register(&self, o: DisposableRef) -> DisposableRef {
        self.base.register(o)
    }

    /// `_currentlyDispatchingCommandId`.
    pub fn currently_dispatching_command_id(&self) -> Option<String> {
        self.inner.currently_dispatching_command_id.borrow().clone()
    }

    /// `_commandService`.
    pub fn command_service(&self) -> &Rc<dyn ICommandService> {
        &self.inner.command_service
    }

    /// `_telemetryService`.
    pub fn telemetry_service(&self) -> &Rc<dyn ITelemetryService> {
        &self.inner.telemetry_service
    }

    /// `_logService`.
    pub fn log_service(&self) -> &LogServiceRef {
        &self.inner.log_service
    }

    /// `_log`.
    pub fn log(&self, message: &str) {
        self.inner.log(message);
    }

    /// `(str) => this._log(str)`: o registro do serviço como função independente do `&self`, que o resolvedor guarda.
    pub fn logger(&self) -> Rc<dyn Fn(&str)> {
        let inner = self.inner.clone();
        Rc::new(move |message| inner.log(message))
    }
}

impl IDisposable for KeybindingServiceState {
    fn dispose(&self) {
        self.inner.current_chord_checker.cancel();
        self.inner.current_single_modifier_clear_timeout.cancel();
        self.base.dispose();
    }
}

impl StateInner {
    fn log(&self, message: &str) {
        if self.logging.get() {
            self.log_service.info(&format!("[KeybindingService]: {message}"));
        }
    }

    fn in_chord_mode(&self) -> bool {
        !self.current_chords.borrow().is_empty()
    }

    fn current_chord_keypresses(&self) -> Vec<String> {
        self.current_chords.borrow().iter().map(|c| c.keypress.clone()).collect()
    }

    fn current_chord_labels(&self) -> String {
        self.current_chords.borrow().iter().map(|c| c.label.clone().unwrap_or_else(|| "null".to_string())).collect::<Vec<_>>().join(", ")
    }

    fn schedule_leave_chord_mode(self: &Rc<Self>) {
        let chord_last_interacted_time = runtime::date_now();
        let this = Rc::downgrade(self);
        self.current_chord_checker.cancel_and_set(
            move || {
                let Some(this) = this.upgrade() else { return };
                if !(this.document_has_focus)() {
                    // Focus has been lost => leave chord mode
                    this.leave_chord_mode();
                    return;
                }
                if runtime::date_now() - chord_last_interacted_time > 5000 {
                    // 5 seconds elapsed => leave chord mode
                    this.leave_chord_mode();
                }
            },
            500,
        );
    }

    fn expect_another_chord(self: &Rc<Self>, first_chord: &str, keypress_label: Option<String>) {
        self.current_chords.borrow_mut().push(CurrentChord { keypress: first_chord.to_string(), label: keypress_label.clone() });
        let length = self.current_chords.borrow().len();
        let status = match length {
            0 => errors::throw(Error::new("Illegal state: impossible")),
            1 => {
                let label = keypress_label.unwrap_or_else(|| "null".to_string());
                self.notification_service.status(
                    NotificationMessage::Text(localize("first.chord", "({0}) was pressed. Waiting for second key of chord...", &[&label])),
                    None,
                )
            }
            _ => {
                let full_keypress_label = self.current_chord_labels();
                self.notification_service.status(
                    NotificationMessage::Text(localize("next.chord", "({0}) was pressed. Waiting for next key of chord...", &[&full_keypress_label])),
                    None,
                )
            }
        };
        *self.current_chord_status_message.borrow_mut() = Some(status);
        self.schedule_leave_chord_mode();
        if ime().enabled() {
            ime().disable();
        }
    }

    fn leave_chord_mode(&self) {
        let status = self.current_chord_status_message.borrow_mut().take();
        if let Some(status) = status {
            status.close();
        }
        self.current_chord_checker.cancel();
        self.current_chords.borrow_mut().clear();
        ime().enable();
    }
}

/// O que a subclasse de `AbstractKeybindingService` implementa.
pub trait KeybindingServiceHost {
    /// O estado da classe base.
    fn state(&self) -> &KeybindingServiceState;

    /// `_getResolver`.
    fn get_resolver(&self) -> Rc<KeybindingResolver>;

    fn resolve_keybinding(&self, keybinding: &Keybinding) -> Vec<Rc<dyn ResolvedKeybinding>>;
    fn resolve_keyboard_event(&self, keyboard_event: &KeyboardEvent) -> Rc<dyn ResolvedKeybinding>;
    fn resolve_user_binding(&self, user_binding: &str) -> Vec<Rc<dyn ResolvedKeybinding>>;
    fn register_schema_contribution(&self, contribution: Rc<dyn KeybindingsSchemaContribution>) -> DisposableRef;
    fn dump_debug_info(&self) -> String;
    fn dump_debug_info_json(&self) -> String;
    fn enable_keybinding_hold_mode(&self, command_id: &str) -> Option<Promise<()>>;

    /// `getDefaultKeybindingsContent`: a classe base devolve vazio.
    fn get_default_keybindings_content(&self) -> String {
        String::new()
    }

    /// `customKeybindingsCount`: a classe base devolve zero.
    fn custom_keybindings_count(&self) -> usize {
        0
    }
}

/// `_dispatch`.
pub fn dispatch_internal<H: KeybindingServiceHost + ?Sized>(host: &H, e: &KeyboardEvent, target: Option<Rc<dyn IContextKeyServiceTarget>>) -> bool {
    if is_key_in_composition(e) {
        // The keystroke belongs to the IME, which owns Enter (commit), Space and the arrows (candidate selection)
        // and Escape (cancel) for the duration of the composition. Dispatching would run commands the user never
        // invoked.
        host.state().inner.log("+ Ignoring keybinding dispatch because an IME composition is in progress.");
        return false;
    }
    do_dispatch(host, &*host.resolve_keyboard_event(e), target, false)
}

/// `_singleModifierDispatch`.
pub fn single_modifier_dispatch<H: KeybindingServiceHost + ?Sized>(host: &H, e: &KeyboardEvent, target: Option<Rc<dyn IContextKeyServiceTarget>>) -> bool {
    if is_key_in_composition(e) {
        return false;
    }
    let state = &host.state().inner;
    let keybinding = host.resolve_keyboard_event(e);
    let single_modifier = keybinding.get_single_modifier_dispatch_chords().into_iter().next().flatten();

    if let Some(single_modifier) = single_modifier {
        let name = single_modifier_name(single_modifier);
        if state.ignore_single_modifiers.get().has(single_modifier) {
            state.log(&format!("+ Ignoring single modifier {name} due to it being pressed together with other keys."));
            state.ignore_single_modifiers.set(KeybindingModifierSet::default());
            state.current_single_modifier_clear_timeout.cancel();
            state.current_single_modifier.set(None);
            return false;
        }
        state.ignore_single_modifiers.set(KeybindingModifierSet::default());

        let Some(current) = state.current_single_modifier.get() else {
            // we have a valid `singleModifier`, store it for the next keyup, but clear it in 300ms
            state.log(&format!("+ Storing single modifier for possible chord {name}."));
            state.current_single_modifier.set(Some(single_modifier));
            let weak = Rc::downgrade(state);
            state.current_single_modifier_clear_timeout.cancel_and_set(
                move || {
                    if let Some(state) = weak.upgrade() {
                        state.log("+ Clearing single modifier due to 300ms elapsed.");
                        state.current_single_modifier.set(None);
                    }
                },
                300,
            );
            return false;
        };

        if single_modifier == current {
            // bingo!
            state.log(&format!("/ Dispatching single modifier chord {name} {name}"));
            state.current_single_modifier_clear_timeout.cancel();
            state.current_single_modifier.set(None);
            return do_dispatch(host, &*keybinding, target, true);
        }

        state.log(&format!("+ Clearing single modifier due to modifier mismatch: {} {name}", single_modifier_name(current)));
        state.current_single_modifier_clear_timeout.cancel();
        state.current_single_modifier.set(None);
        return false;
    }

    // When pressing a modifier and holding it pressed with any other modifier or key combination, the pressed
    // modifiers should no longer be considered for single modifier dispatch.
    let first_chord = keybinding.get_chords().into_iter().next();
    state.ignore_single_modifiers.set(KeybindingModifierSet::new(first_chord.as_ref()));

    if state.current_single_modifier.get().is_some() {
        state.log("+ Clearing single modifier due to other key up.");
    }
    state.current_single_modifier_clear_timeout.cancel();
    state.current_single_modifier.set(None);
    false
}

fn do_dispatch<H: KeybindingServiceHost + ?Sized>(
    host: &H,
    user_keypress: &dyn ResolvedKeybinding,
    target: Option<Rc<dyn IContextKeyServiceTarget>>,
    is_single_modifier_chord: bool,
) -> bool {
    let state = &host.state().inner;
    let mut should_prevent_default = false;

    if user_keypress.has_multiple_chords() {
        // warn - because user can press a single chord at a time
        eprintln!("Unexpected keyboard event mapped to multiple chords");
        return false;
    }

    let (user_pressed_chord, current_chords): (Option<String>, Vec<String>) = if is_single_modifier_chord {
        // The keybinding is the second keypress of a single modifier chord, e.g. "shift shift". A single modifier can
        // only occur when the same modifier is pressed in short sequence, hence we disregard `_currentChord` and use
        // the same modifier instead.
        let dispatch_keyname = user_keypress.get_single_modifier_dispatch_chords().into_iter().next().flatten().map(|m| single_modifier_name(m).to_string());
        let current = dispatch_keyname.clone().map(|k| vec![k]).unwrap_or_default();
        (dispatch_keyname, current)
    } else {
        (user_keypress.get_dispatch_chords().into_iter().next().flatten(), state.current_chord_keypresses())
    };

    let Some(user_pressed_chord) = user_pressed_chord else {
        state.log("\\ Keyboard event cannot be dispatched in keydown phase.");
        // cannot be dispatched, probably only modifier keys
        return should_prevent_default;
    };

    let context_value = state.context_key_service.get_context(target);
    let keypress_label = user_keypress.get_label();
    let label_arg: LogArg = keypress_label.as_deref().map_or(LogArg::Value(Dynamic::Null), LogArg::from);
    let label_text = keypress_label.clone().unwrap_or_else(|| "null".to_string());
    let resolve_result = host.get_resolver().resolve(&*context_value, &current_chords, &user_pressed_chord);

    match resolve_result {
        ResolutionResult::NoMatchingKb => {
            state.log_service.trace_args("KeybindingService#dispatch", &[label_arg, "[ No matching keybinding ]".into()]);
            if state.in_chord_mode() {
                let current_chords_label = state.current_chord_labels();
                state.log(&format!("+ Leaving multi-chord mode: Nothing bound to \"{current_chords_label}, {label_text}\"."));
                state.notification_service.status(
                    NotificationMessage::Text(localize("missing.chord", "The key combination ({0}, {1}) is not a command.", &[&current_chords_label, &label_text])),
                    Some(StatusMessageOptions { show_after: None, hide_after: Some(10 * 1000) }),
                );
                state.leave_chord_mode();
                should_prevent_default = true;
            }
            should_prevent_default
        }
        ResolutionResult::MoreChordsNeeded => {
            state.log_service.trace_args("KeybindingService#dispatch", &[label_arg, "[ Several keybindings match - more chords needed ]".into()]);
            should_prevent_default = true;
            state.expect_another_chord(&user_pressed_chord, keypress_label);
            state.log(if state.current_chords.borrow().len() == 1 { "+ Entering multi-chord mode..." } else { "+ Continuing multi-chord mode..." });
            should_prevent_default
        }
        ResolutionResult::KbFound { command_id, command_args, is_bubble } => {
            let command_text = command_id.clone().unwrap_or_else(|| "null".to_string());
            state.log_service.trace_args("KeybindingService#dispatch", &[label_arg, format!("[ Will dispatch command {command_text} ]").as_str().into()]);

            match command_id.filter(|c| !c.is_empty()) {
                None => {
                    if state.in_chord_mode() {
                        let current_chords_label = state.current_chord_labels();
                        state.log(&format!("+ Leaving chord mode: Nothing bound to \"{current_chords_label}, {label_text}\"."));
                        state.notification_service.status(
                            NotificationMessage::Text(localize("missing.chord", "The key combination ({0}, {1}) is not a command.", &[&current_chords_label, &label_text])),
                            Some(StatusMessageOptions { show_after: None, hide_after: Some(10 * 1000) }),
                        );
                        state.leave_chord_mode();
                        should_prevent_default = true;
                    }
                }
                Some(command_id) => {
                    if state.in_chord_mode() {
                        state.leave_chord_mode();
                    }
                    if !is_bubble {
                        should_prevent_default = true;
                    }
                    state.log(&format!("+ Invoking command {command_id}."));
                    *state.currently_dispatching_command_id.borrow_mut() = Some(command_id.clone());
                    let args = if command_args.is_undefined() { Vec::new() } else { vec![command_args] };
                    let promise = state.command_service.execute_command(&command_id, args);
                    *state.currently_dispatching_command_id.borrow_mut() = None;
                    let notification_service = state.notification_service.clone();
                    let _ = promise.then_result(move |result| {
                        if let Err(err) = result {
                            notification_service.warn(vec![NotificationMessage::Error(err)]);
                        }
                        Ok(Dynamic::Undefined)
                    });

                    let high_frequency = RegExp::from_str(HIGH_FREQ_COMMANDS, "").is_ok_and(|re| re.exec(&command_id.encode_utf16().collect::<Vec<_>>(), 0).is_some());
                    if !high_frequency {
                        let detail = user_keypress.get_user_settings_label().map_or(Dynamic::Undefined, |d| Dynamic::string(&d));
                        state.telemetry_service.public_log2(
                            "workbenchActionExecuted",
                            Some(Dynamic::object([("id", Dynamic::string(&command_id)), ("from", Dynamic::string("keybinding")), ("detail", detail)])),
                        );
                    }
                }
            }
            should_prevent_default
        }
    }
}

impl<H: KeybindingServiceHost + ?Sized> IKeybindingService for H {
    fn in_chord_mode(&self) -> bool {
        self.state().inner.in_chord_mode()
    }

    fn on_did_update_keybindings(&self) -> Event<()> {
        self.state().on_did_update_keybindings.event()
    }

    fn resolve_keybinding(&self, keybinding: &Keybinding) -> Vec<Rc<dyn ResolvedKeybinding>> {
        KeybindingServiceHost::resolve_keybinding(self, keybinding)
    }

    fn resolve_keyboard_event(&self, keyboard_event: &KeyboardEvent) -> Rc<dyn ResolvedKeybinding> {
        KeybindingServiceHost::resolve_keyboard_event(self, keyboard_event)
    }

    fn resolve_user_binding(&self, user_binding: &str) -> Vec<Rc<dyn ResolvedKeybinding>> {
        KeybindingServiceHost::resolve_user_binding(self, user_binding)
    }

    fn dispatch_event(&self, e: &KeyboardEvent, target: Option<Rc<dyn IContextKeyServiceTarget>>) -> bool {
        dispatch_internal(self, e, target)
    }

    fn soft_dispatch(&self, e: &KeyboardEvent, target: Option<Rc<dyn IContextKeyServiceTarget>>) -> ResolutionResult {
        let state = &self.state().inner;
        state.log("/ Soft dispatching keyboard event");
        if is_key_in_composition(e) {
            // Must agree with `_dispatch`: callers use this to decide whether the workbench will claim the key, and a
            // "yes" here followed by a "no" there would drop the keystroke on the floor.
            state.log("\\ Keyboard event is part of an IME composition");
            return ResolutionResult::NoMatchingKb;
        }
        let keybinding = KeybindingServiceHost::resolve_keyboard_event(self, e);
        if keybinding.has_multiple_chords() {
            eprintln!("keyboard event should not be mapped to multiple chords");
            return ResolutionResult::NoMatchingKb;
        }
        let Some(first_chord) = keybinding.get_dispatch_chords().into_iter().next().flatten() else {
            // cannot be dispatched, probably only modifier keys
            state.log("\\ Keyboard event cannot be dispatched");
            return ResolutionResult::NoMatchingKb;
        };
        let context_value = state.context_key_service.get_context(target);
        let current_chords = state.current_chord_keypresses();
        self.get_resolver().resolve(&*context_value, &current_chords, &first_chord)
    }

    fn enable_keybinding_hold_mode(&self, command_id: &str) -> Option<Promise<()>> {
        KeybindingServiceHost::enable_keybinding_hold_mode(self, command_id)
    }

    fn dispatch_by_user_settings_label(&self, user_settings_label: &str, target: Option<Rc<dyn IContextKeyServiceTarget>>) {
        let state = &self.state().inner;
        state.log(&format!("/ Dispatching keybinding triggered via menu entry accelerator - {user_settings_label}"));
        let keybindings = KeybindingServiceHost::resolve_user_binding(self, user_settings_label);
        match keybindings.first() {
            None => state.log(&format!("\\ Could not resolve - {user_settings_label}")),
            Some(first) => {
                do_dispatch(self, &**first, target, false);
            }
        }
    }

    fn lookup_keybindings(&self, command_id: &str) -> Vec<Rc<dyn ResolvedKeybinding>> {
        self.get_resolver().lookup_keybindings(command_id).into_iter().filter_map(|item| item.resolved_keybinding.clone()).collect()
    }

    fn lookup_keybinding(&self, command_id: &str, context: Option<&dyn IContextKeyService>, enforce_context_check: bool) -> Option<Rc<dyn ResolvedKeybinding>> {
        let state = &self.state().inner;
        let context = context.unwrap_or(&*state.context_key_service);
        let result = self.get_resolver().lookup_primary_keybinding(command_id, context, enforce_context_check)?;
        result.resolved_keybinding.clone()
    }

    fn get_default_keybindings_content(&self) -> String {
        KeybindingServiceHost::get_default_keybindings_content(self)
    }

    fn get_default_keybindings(&self) -> Vec<Rc<ResolvedKeybindingItem>> {
        self.get_resolver().get_default_keybindings().to_vec()
    }

    fn get_keybindings(&self) -> Vec<Rc<ResolvedKeybindingItem>> {
        self.get_resolver().get_keybindings().to_vec()
    }

    fn custom_keybindings_count(&self) -> usize {
        KeybindingServiceHost::custom_keybindings_count(self)
    }

    fn might_produce_printable_character(&self, event: &KeyboardEvent) -> bool {
        if event.ctrl_key || event.meta_key {
            // ignore ctrl/cmd-combination but not shift/alt-combinatios
            return false;
        }
        // weak check for certain ranges. this is properly implemented in a subclass with access to the
        // KeyboardMapperFactory.
        (event.key_code >= KeyCode::KeyA && event.key_code <= KeyCode::KeyZ) || (event.key_code >= KeyCode::Digit0 && event.key_code <= KeyCode::Digit9)
    }

    fn register_schema_contribution(&self, contribution: Rc<dyn KeybindingsSchemaContribution>) -> DisposableRef {
        KeybindingServiceHost::register_schema_contribution(self, contribution)
    }

    fn toggle_logging(&self) -> bool {
        let state = &self.state().inner;
        state.logging.set(!state.logging.get());
        state.logging.get()
    }

    fn append_keybinding(&self, label: &str, command_id: Option<&str>, context: Option<&dyn IContextKeyService>, enforce_context_check: bool) -> String {
        if let Some(command_id) = command_id.filter(|c| !c.is_empty()) {
            let keybinding_label = self.lookup_keybinding(command_id, context, enforce_context_check).and_then(|kb| kb.get_label());
            if let Some(keybinding_label) = keybinding_label.filter(|l| !l.is_empty()) {
                return localize("keybindingLabel", "{0} ({1})", &[label, &keybinding_label]);
            }
        }
        label.to_string()
    }

    fn dump_debug_info(&self) -> String {
        KeybindingServiceHost::dump_debug_info(self)
    }

    fn dump_debug_info_json(&self) -> String {
        KeybindingServiceHost::dump_debug_info_json(self)
    }
}

/// `Severity` reexportado para quem monta notificações a partir do serviço.
pub type NotificationSeverity = Severity;
