//! Porte de `src/vs/workbench/contrib/terminalContrib/notification/browser/terminalNotificationHandler.ts`: o tratador das
//! notificações do protocolo OSC 99 do terminal (o protocolo de notificação do kitty). Junta as partes de uma notificação
//! (título, corpo e botões, que chegam em sequências separadas), mostra a notificação pelo hospedeiro, e devolve ao processo os
//! relatórios de ativação, fechamento, consulta e vivos.
//!
//! Decisões de tradução:
//! - **`IOsc99NotificationHost`** é o trait [`IOsc99NotificationHost`]; `notify(INotification)` recebe a [`Notification`] do porte
//!   (a `priority` do `INotification` está em `properties.priority`), e `updateEnableNotifications` devolve uma [`Promise`].
//! - **`Map` com ordem de inserção** (`_osc99ActiveNotifications`, cujas chaves o `alive` lista) é um [`IndexMap`].
//! - **Estados e notificações ativas** (`IOsc99NotificationState`, `IOsc99ActiveNotification`) são structs com campos mutáveis em
//!   `Cell`/`RefCell`, compartilhadas por `Rc`; os fechos que o hospedeiro guarda seguram o tratador por `Weak`.
//! - **`decodeBase64(payload).toString()`** não falha no porte (`decode_base64` devolve o buffer), então o `catch` que escreve
//!   `Failed to decode OSC 99 payload` não existe; `logWarn` fica no contrato do hospedeiro.
//! - **`Number.parseInt(value, 10)`** é [`parse_int`]: o prefixo inteiro com sinal, e `None` (o `NaN`) sem dígitos.
//! - **`state.focusOnActivate`/`reportOnActivate`/`id`** lidos dentro dos botões valem o que o estado tinha ao mostrar a
//!   notificação (o estado sai da lista de pendentes logo depois de mostrada e ninguém mais o altera).
//! - **`_register(handle.onDidClose(...))`**, o `_register(new DisposableStore())` e o `disposableTimeout(..., this._store)` usam o
//!   `Disposable` do tratador e o seu `store`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::actions::{Action, ActionCallback, ActionRef};
use je_base::async_::disposable_timeout;
use je_base::buffer::decode_base64;
use je_base::dynamic::Dynamic;
use je_base::js_regexp::JsRegExp;
use je_base::js_string;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::promise::{MaybePromise, Promise};
use je_platform::notification::notification::{
    Notification, NotificationActions, NotificationHandle, NotificationMessage, NotificationPriority, NotificationProperties, NotificationSource, NotificationSourceOrText, Severity,
};

/// `const enum Osc99PayloadType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Osc99PayloadType {
    Title,
    Body,
    Buttons,
    Close,
    Query,
    Alive,
}

impl Osc99PayloadType {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "title" => Some(Self::Title),
            "body" => Some(Self::Body),
            "buttons" => Some(Self::Buttons),
            "close" => Some(Self::Close),
            "?" => Some(Self::Query),
            "alive" => Some(Self::Alive),
            _ => None,
        }
    }
}

/// `Osc99Occasion`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Osc99Occasion {
    Always,
    Unfocused,
    Invisible,
}

/// `Osc99CloseReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Osc99CloseReason {
    Button,
    Secondary,
    Auto,
    Protocol,
}

/// `IOsc99NotificationState`.
struct Osc99NotificationState {
    id: Option<String>,
    title: String,
    body: String,
    buttons_payload: String,
    focus_on_activate: bool,
    report_on_activate: bool,
    report_on_close: bool,
    urgency: Option<i64>,
    auto_close_ms: Option<i64>,
    occasion: Option<Osc99Occasion>,
}

/// `IOsc99ActiveNotification`.
struct Osc99ActiveNotification {
    id: Option<String>,
    handle: Rc<dyn NotificationHandle>,
    action_store: RefCell<Rc<DisposableStore>>,
    auto_close_disposable: RefCell<Option<DisposableRef>>,
    report_on_activate: Cell<bool>,
    report_on_close: Cell<bool>,
    focus_on_activate: Cell<bool>,
    close_reason: Cell<Option<Osc99CloseReason>>,
}

/// `IOsc99NotificationHost`.
pub trait IOsc99NotificationHost {
    fn is_enabled(&self) -> bool;
    fn is_window_focused(&self) -> bool;
    fn is_terminal_visible(&self) -> bool;
    fn focus_terminal(&self);
    fn notify(&self, notification: Notification) -> Rc<dyn NotificationHandle>;
    fn update_enable_notifications(&self, value: bool) -> Promise<()>;
    fn log_warn(&self, message: &str);
    fn write_to_process(&self, data: &str);
}

/// `Number.parseInt(value, 10)`: o prefixo inteiro (com sinal opcional, depois dos espaços), ou `None` quando não há dígito.
fn parse_int(value: &str) -> Option<i64> {
    let units: Vec<u16> = value.encode_utf16().collect();
    let trimmed = String::from_utf16_lossy(js_string::trim_start(&units));
    let (negative, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(&trimmed)),
    };
    let digits: String = digits.chars().take_while(char::is_ascii_digit).collect();
    let parsed = digits.parse::<i64>().ok()?;
    Some(if negative { -parsed } else { parsed })
}

/// `text.trim()` do JavaScript.
fn js_trim(text: &str) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16_lossy(js_string::trim(&units))
}

/// `values?.[values.length - 1]` de um `Map<string, string[]>` de metadados.
fn last_value<'a>(metadata: &'a HashMap<String, Vec<String>>, key: &str) -> Option<&'a str> {
    metadata.get(key).and_then(|values| values.last()).map(String::as_str)
}

struct Inner {
    host: Rc<dyn IOsc99NotificationHost>,
    base: Rc<Disposable>,
    store: Rc<DisposableStore>,
    osc99_pending_notifications: RefCell<HashMap<String, Rc<RefCell<Osc99NotificationState>>>>,
    osc99_pending_anonymous: RefCell<Option<Rc<RefCell<Osc99NotificationState>>>>,
    osc99_active_notifications: RefCell<IndexMap<String, Rc<Osc99ActiveNotification>>>,
}

/// `TerminalNotificationHandler`.
pub struct TerminalNotificationHandler {
    inner: Rc<Inner>,
}

impl TerminalNotificationHandler {
    pub fn new(host: Rc<dyn IOsc99NotificationHost>) -> Self {
        let base = Disposable::new();
        let store = DisposableStore::new();
        base.register(store.clone());
        Self {
            inner: Rc::new(Inner {
                host,
                base,
                store,
                osc99_pending_notifications: RefCell::new(HashMap::new()),
                osc99_pending_anonymous: RefCell::new(None),
                osc99_active_notifications: RefCell::new(IndexMap::new()),
            }),
        }
    }

    pub fn handle_sequence(&self, data: &str) -> bool {
        Inner::handle_sequence(&self.inner, data)
    }
}

impl IDisposable for TerminalNotificationHandler {
    fn dispose(&self) {
        self.inner.base.dispose();
    }
}

impl Inner {
    fn handle_sequence(this: &Rc<Self>, data: &str) -> bool {
        let (metadata, payload) = Self::split_osc99_data(data);
        let metadata_entries = Self::parse_osc99_metadata(metadata);
        let raw_payload_type = last_value(&metadata_entries, "p");
        let payload_type_text = raw_payload_type.filter(|raw| !raw.is_empty()).unwrap_or("title");
        let id = Self::sanitize_osc99_id(metadata_entries.get("i").and_then(|values| values.first()).map(String::as_str));

        if !this.host.is_enabled() {
            return true;
        }

        match Osc99PayloadType::parse(payload_type_text) {
            Some(Osc99PayloadType::Query) => {
                this.send_osc99_query_response(id.as_deref());
                return true;
            }
            Some(Osc99PayloadType::Alive) => {
                this.send_osc99_alive_response(id.as_deref());
                return true;
            }
            Some(Osc99PayloadType::Close) => {
                this.close_osc99_notification(id.as_deref());
                return true;
            }
            _ => {}
        }

        let state = this.get_or_create_osc99_state(id.as_deref());
        Self::update_osc99_state_from_metadata(&mut state.borrow_mut(), &metadata_entries);

        let is_encoded = metadata_entries.get("e").and_then(|values| values.first()).map(String::as_str) == Some("1");
        let payload_text = this.decode_osc99_payload(payload, is_encoded);
        let is_done = metadata_entries.get("d").and_then(|values| values.first()).map(String::as_str) != Some("0");

        match Osc99PayloadType::parse(payload_type_text) {
            Some(Osc99PayloadType::Title) => state.borrow_mut().title.push_str(&payload_text),
            Some(Osc99PayloadType::Body) => state.borrow_mut().body.push_str(&payload_text),
            Some(Osc99PayloadType::Buttons) => state.borrow_mut().buttons_payload.push_str(&payload_text),
            _ => return true,
        }

        if !is_done {
            return true;
        }
        let occasion = state.borrow().occasion;
        if !this.should_honor_osc99_occasion(occasion) {
            this.clear_osc99_pending_state(id.as_deref());
            return true;
        }

        if Self::show_osc99_notification(this, &state.borrow()) {
            this.clear_osc99_pending_state(id.as_deref());
        }
        true
    }

    fn split_osc99_data(data: &str) -> (&str, &str) {
        match data.find(';') {
            None => (data, ""),
            Some(separator_index) => (&data[..separator_index], &data[separator_index + 1..]),
        }
    }

    fn parse_osc99_metadata(metadata: &str) -> HashMap<String, Vec<String>> {
        let mut result: HashMap<String, Vec<String>> = HashMap::new();
        if metadata.is_empty() {
            return result;
        }
        for entry in metadata.split(':') {
            if entry.is_empty() {
                continue;
            }
            let Some(separator_index) = entry.find('=') else {
                continue;
            };
            let key = &entry[..separator_index];
            let value = &entry[separator_index + 1..];
            if key.is_empty() {
                continue;
            }
            result.entry(key.to_owned()).or_default().push(value.to_owned());
        }
        result
    }

    fn decode_osc99_payload(&self, payload: &str, is_encoded: bool) -> String {
        if !is_encoded {
            return payload.to_owned();
        }
        decode_base64(payload).to_string()
    }

    /// `rawId.replace(/[^a-zA-Z0-9_\-+.]/g, '')`, e `undefined` quando sobra nada.
    fn sanitize_osc99_id(raw_id: Option<&str>) -> Option<String> {
        let raw_id = raw_id.filter(|raw| !raw.is_empty())?;
        let sanitized: String = raw_id.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '.')).collect();
        (!sanitized.is_empty()).then_some(sanitized)
    }

    fn sanitize_osc99_message_text(text: &str) -> String {
        // Markdown link syntax is nestable, so a single pass can splice surviving fragments into a
        // new, valid link (e.g. `[[ ](http://z)label](command:...)` becomes `[ label](command:...)`).
        // Replace to a fixed point so no parsable link can remain in the sanitized output.
        let link = JsRegExp::from_str(r"\[([^\]]+)\]\(([^)]+)\)", "g").expect("padrão de link markdown válido");
        let mut sanitized: Vec<u16> = text.encode_utf16().collect();
        loop {
            let previous = sanitized.clone();
            link.set_last_index(0);
            let mut out: Vec<u16> = Vec::with_capacity(previous.len());
            let mut position = 0;
            while let Some(found) = link.exec(&previous) {
                out.extend_from_slice(&previous[position..found.start()]);
                if let Some(Some(label)) = found.captures.get(1) {
                    out.extend_from_slice(&previous[label.clone()]);
                }
                position = found.end();
            }
            out.extend_from_slice(&previous[position..]);
            sanitized = out;
            if sanitized == previous {
                break;
            }
        }
        String::from_utf16_lossy(&sanitized)
    }

    fn get_or_create_osc99_state(&self, id: Option<&str>) -> Rc<RefCell<Osc99NotificationState>> {
        let Some(id) = id else {
            return self.osc99_pending_anonymous.borrow_mut().get_or_insert_with(|| Rc::new(RefCell::new(Self::create_osc99_state(None)))).clone();
        };
        self.osc99_pending_notifications.borrow_mut().entry(id.to_owned()).or_insert_with(|| Rc::new(RefCell::new(Self::create_osc99_state(Some(id))))).clone()
    }

    fn create_osc99_state(id: Option<&str>) -> Osc99NotificationState {
        Osc99NotificationState {
            id: id.map(str::to_owned),
            title: String::new(),
            body: String::new(),
            buttons_payload: String::new(),
            focus_on_activate: true,
            report_on_activate: false,
            report_on_close: false,
            urgency: None,
            auto_close_ms: None,
            occasion: None,
        }
    }

    fn clear_osc99_pending_state(&self, id: Option<&str>) {
        match id {
            None => *self.osc99_pending_anonymous.borrow_mut() = None,
            Some(id) => {
                self.osc99_pending_notifications.borrow_mut().remove(id);
            }
        }
    }

    fn update_osc99_state_from_metadata(state: &mut Osc99NotificationState, metadata_entries: &HashMap<String, Vec<String>>) {
        if let Some(action_value) = last_value(metadata_entries, "a") {
            let (focus_on_activate, report_on_activate) = Self::parse_osc99_actions(action_value);
            state.focus_on_activate = focus_on_activate;
            state.report_on_activate = report_on_activate;
        }
        if let Some(close_value) = last_value(metadata_entries, "c") {
            state.report_on_close = close_value == "1";
        }
        if let Some(urgency_value) = last_value(metadata_entries, "u")
            && let Some(urgency) = parse_int(urgency_value)
        {
            state.urgency = Some(urgency);
        }
        if let Some(auto_close_value) = last_value(metadata_entries, "w")
            && let Some(auto_close) = parse_int(auto_close_value)
        {
            state.auto_close_ms = Some(auto_close);
        }
        match last_value(metadata_entries, "o") {
            Some("always") => state.occasion = Some(Osc99Occasion::Always),
            Some("unfocused") => state.occasion = Some(Osc99Occasion::Unfocused),
            Some("invisible") => state.occasion = Some(Osc99Occasion::Invisible),
            _ => {}
        }
    }

    /// `_parseOsc99Actions(value)`: `(focusOnActivate, reportOnActivate)`.
    fn parse_osc99_actions(value: &str) -> (bool, bool) {
        let mut focus_on_activate = true;
        let mut report_on_activate = false;
        for token in value.split(',') {
            match token {
                "focus" => focus_on_activate = true,
                "-focus" => focus_on_activate = false,
                "report" => report_on_activate = true,
                "-report" => report_on_activate = false,
                _ => {}
            }
        }
        (focus_on_activate, report_on_activate)
    }

    fn should_honor_osc99_occasion(&self, occasion: Option<Osc99Occasion>) -> bool {
        let occasion = match occasion {
            None | Some(Osc99Occasion::Always) => return true,
            Some(occasion) => occasion,
        };
        let window_focused = self.host.is_window_focused();
        match occasion {
            Osc99Occasion::Unfocused => !window_focused,
            Osc99Occasion::Invisible => !window_focused && !self.host.is_terminal_visible(),
            Osc99Occasion::Always => true,
        }
    }

    fn show_osc99_notification(this: &Rc<Self>, state: &Osc99NotificationState) -> bool {
        let Some(message) = Self::get_osc99_notification_message(state) else {
            return false;
        };

        let severity = if state.urgency == Some(2) { Severity::Warning } else { Severity::Info };
        let priority = Self::get_osc99_notification_priority(state.urgency);
        let source = NotificationSource { id: "terminal".to_owned(), label: localize("terminalNotificationSource", "Terminal", &[]) };
        let buttons: Vec<&str> = if state.buttons_payload.is_empty() { Vec::new() } else { state.buttons_payload.split('\u{2028}').collect() };
        let action_store = DisposableStore::new();
        this.base.register(action_store.clone());

        // `handleRef` e `activeRef` do original: preenchidos depois de os botões serem criados.
        let handle_ref: Rc<RefCell<Option<Rc<dyn NotificationHandle>>>> = Rc::default();
        let active_ref: Rc<RefCell<Option<Rc<Osc99ActiveNotification>>>> = Rc::default();
        let weak = Rc::downgrade(this);
        let focus_on_activate = state.focus_on_activate;
        let report_on_activate = state.report_on_activate;
        let state_id = state.id.clone();
        let report_activation: Rc<dyn Fn(Option<usize>, bool)> = {
            let weak = weak.clone();
            Rc::new(move |button_index, force_focus| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                if force_focus || focus_on_activate {
                    this.host.focus_terminal();
                }
                if report_on_activate {
                    this.send_osc99_activation_report(state_id.as_deref(), button_index);
                }
            })
        };

        let mut primary_actions: Vec<ActionRef> = Vec::new();
        for (i, label) in buttons.iter().enumerate() {
            if label.is_empty() {
                continue;
            }
            let callback: ActionCallback = {
                let (active_ref, handle_ref, report_activation) = (active_ref.clone(), handle_ref.clone(), report_activation.clone());
                Rc::new(move |_| {
                    if let Some(active) = active_ref.borrow().as_ref() {
                        active.close_reason.set(Some(Osc99CloseReason::Button));
                    }
                    report_activation(Some(i + 1), false);
                    let handle = handle_ref.borrow().clone();
                    if let Some(handle) = handle {
                        handle.close();
                    }
                    MaybePromise::Value(Dynamic::Undefined)
                })
            };
            let action = action_store.add_rc(Action::new(&format!("terminal.osc99.button.{i}"), label, "", true, Some(callback)));
            primary_actions.push(action);
        }

        let mut secondary_actions: Vec<ActionRef> = Vec::new();
        {
            let (active_ref, handle_ref) = (active_ref.clone(), handle_ref.clone());
            let callback: ActionCallback = Rc::new(move |_| {
                if let Some(active) = active_ref.borrow().as_ref() {
                    active.close_reason.set(Some(Osc99CloseReason::Secondary));
                }
                let handle = handle_ref.borrow().clone();
                if let Some(handle) = handle {
                    handle.close();
                }
                MaybePromise::Value(Dynamic::Undefined)
            });
            secondary_actions.push(action_store.add_rc(Action::new("terminal.osc99.dismiss", &localize("terminalNotificationDismiss", "Dismiss", &[]), "", true, Some(callback))));
        }
        {
            let (active_ref, handle_ref, weak) = (active_ref.clone(), handle_ref.clone(), weak.clone());
            let callback: ActionCallback = Rc::new(move |_| {
                let Some(this) = weak.upgrade() else {
                    return MaybePromise::Value(Dynamic::Undefined);
                };
                let (active_ref, handle_ref) = (active_ref.clone(), handle_ref.clone());
                // `await this._host.updateEnableNotifications(false)` e depois fecha.
                MaybePromise::Promise(this.host.update_enable_notifications(false).then(move |()| {
                    if let Some(active) = active_ref.borrow().as_ref() {
                        active.close_reason.set(Some(Osc99CloseReason::Secondary));
                    }
                    let handle = handle_ref.borrow().clone();
                    if let Some(handle) = handle {
                        handle.close();
                    }
                    Dynamic::Undefined
                }))
            });
            secondary_actions.push(action_store.add_rc(Action::new(
                "terminal.osc99.disable",
                &localize("terminalNotificationDisable", "Disable Terminal Notifications", &[]),
                "",
                true,
                Some(callback),
            )));
        }

        let actions = NotificationActions { primary: primary_actions, secondary: secondary_actions };

        if let Some(state_id) = &state.id {
            let existing = this.osc99_active_notifications.borrow().get(state_id).cloned();
            if let Some(existing) = existing {
                *active_ref.borrow_mut() = Some(existing.clone());
                *handle_ref.borrow_mut() = Some(existing.handle.clone());
                existing.handle.update_message(NotificationMessage::Text(message));
                existing.handle.update_severity(severity);
                existing.handle.update_actions(Some(actions));
                existing.action_store.borrow().dispose();
                *existing.action_store.borrow_mut() = action_store;
                existing.focus_on_activate.set(state.focus_on_activate);
                existing.report_on_activate.set(state.report_on_activate);
                existing.report_on_close.set(state.report_on_close);
                let previous = existing.auto_close_disposable.borrow_mut().take();
                if let Some(previous) = previous {
                    previous.dispose();
                }
                *existing.auto_close_disposable.borrow_mut() = Self::schedule_osc99_auto_close(this, &existing, state.auto_close_ms);
                return true;
            }
        }

        let handle = this.host.notify(Notification {
            properties: NotificationProperties { priority, ..NotificationProperties::default() },
            id: state.id.as_ref().map(|id| format!("terminal.osc99.{id}")),
            severity,
            message: NotificationMessage::Text(message),
            source: Some(NotificationSourceOrText::Source(source)),
            actions: Some(actions),
            progress: None,
        });
        *handle_ref.borrow_mut() = Some(handle.clone());

        let active = Rc::new(Osc99ActiveNotification {
            id: state.id.clone(),
            handle: handle.clone(),
            action_store: RefCell::new(action_store),
            auto_close_disposable: RefCell::new(None),
            report_on_activate: Cell::new(state.report_on_activate),
            report_on_close: Cell::new(state.report_on_close),
            focus_on_activate: Cell::new(state.focus_on_activate),
            close_reason: Cell::new(None),
        });
        *active_ref.borrow_mut() = Some(active.clone());
        *active.auto_close_disposable.borrow_mut() = Self::schedule_osc99_auto_close(this, &active, state.auto_close_ms);
        {
            let weak = weak.clone();
            let active = active.clone();
            this.base.register(handle.on_did_close().listen(move |()| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                if active.report_on_activate.get() && active.close_reason.get().is_none() {
                    if active.focus_on_activate.get() {
                        this.host.focus_terminal();
                    }
                    this.send_osc99_activation_report(active.id.as_deref(), None);
                }
                if active.report_on_close.get() {
                    this.send_osc99_close_report(active.id.as_deref());
                }
                active.action_store.borrow().dispose();
                let auto_close = active.auto_close_disposable.borrow_mut().take();
                if let Some(auto_close) = auto_close {
                    auto_close.dispose();
                }
                if let Some(id) = &active.id {
                    this.osc99_active_notifications.borrow_mut().shift_remove(id);
                }
            }));
        }

        if let Some(id) = &active.id {
            this.osc99_active_notifications.borrow_mut().insert(id.clone(), active.clone());
        }
        true
    }

    fn get_osc99_notification_message(state: &Osc99NotificationState) -> Option<String> {
        let title = Self::sanitize_osc99_message_text(&state.title);
        let body = Self::sanitize_osc99_message_text(&state.body);
        let has_title = !js_trim(&title).is_empty();
        let has_body = !js_trim(&body).is_empty();
        if has_title && has_body {
            return Some(format!("{title}: {body}"));
        }
        if has_title {
            return Some(title);
        }
        if has_body {
            return Some(body);
        }
        None
    }

    fn get_osc99_notification_priority(urgency: Option<i64>) -> Option<NotificationPriority> {
        match urgency {
            Some(0) => Some(NotificationPriority::Silent),
            Some(1) => Some(NotificationPriority::Default),
            Some(2) => Some(NotificationPriority::Urgent),
            _ => None,
        }
    }

    fn schedule_osc99_auto_close(this: &Rc<Self>, active: &Rc<Osc99ActiveNotification>, auto_close_ms: Option<i64>) -> Option<DisposableRef> {
        let auto_close_ms = auto_close_ms.filter(|ms| *ms > 0)?;
        let active = active.clone();
        Some(disposable_timeout(
            move || {
                active.close_reason.set(Some(Osc99CloseReason::Auto));
                active.handle.close();
            },
            auto_close_ms as u64,
            Some(this.store.clone()),
        ))
    }

    fn close_osc99_notification(&self, id: Option<&str>) {
        let Some(id) = id else {
            return;
        };
        let active = self.osc99_active_notifications.borrow().get(id).cloned();
        if let Some(active) = active {
            active.close_reason.set(Some(Osc99CloseReason::Protocol));
            active.handle.close();
        }
        self.osc99_pending_notifications.borrow_mut().remove(id);
    }

    fn send_osc99_query_response(&self, id: Option<&str>) {
        let request_id = id.unwrap_or("0");
        self.send_osc99_response(
            &[&format!("i={request_id}"), "p=?", "a=report,focus", "c=1", "o=always,unfocused,invisible", "p=title,body,buttons,close,alive,?", "u=0,1,2", "w=1"],
            "",
        );
    }

    fn send_osc99_alive_response(&self, id: Option<&str>) {
        let request_id = id.unwrap_or("0");
        let alive_ids = self.osc99_active_notifications.borrow().keys().cloned().collect::<Vec<_>>().join(",");
        self.send_osc99_response(&[&format!("i={request_id}"), "p=alive"], &alive_ids);
    }

    fn send_osc99_activation_report(&self, id: Option<&str>, button_index: Option<usize>) {
        let report_id = id.unwrap_or("0");
        self.send_osc99_response(&[&format!("i={report_id}")], &button_index.map_or_else(String::new, |index| index.to_string()));
    }

    fn send_osc99_close_report(&self, id: Option<&str>) {
        let report_id = id.unwrap_or("0");
        self.send_osc99_response(&[&format!("i={report_id}"), "p=close"], "");
    }

    fn send_osc99_response(&self, metadata_parts: &[&str], payload: &str) {
        let metadata = metadata_parts.join(":");
        self.host.write_to_process(&format!("\x1b]99;{metadata};{payload}\x1b\\"));
    }
}
