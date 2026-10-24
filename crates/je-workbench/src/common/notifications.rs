//! Porte de `src/vs/workbench/common/notifications.ts`: o modelo de dados das notificações do workbench
//! (toasts, centro de notificações e mensagens de status), sem DOM.
//!
//! Notas de tradução:
//! - **`INotificationViewItem`** tem uma única implementação, `NotificationViewItem`, e a interface não vira
//!   trait: o item é [`NotificationViewItem`] dentro de um `Rc` (um item é compartilhado entre o modelo, os
//!   handles e as views, e a identidade `===` é `Rc::ptr_eq`). [`is_notification_view_item`] é o
//!   `instanceof`, por `as_any`. `INotificationViewItemProgress` é [`NotificationViewItemProgress`].
//! - **Herança de `Disposable`**: cada tipo guarda um [`Disposable`] por dentro, com os emitters registrados,
//!   e implementa [`IDisposable`] por ele. Os tipos que o original cria com `new` e que se auto-referenciam
//!   (o item, o modelo) nascem em `Rc` por `Rc::new_cyclic`, com um `Weak` de si; os fechos que o original
//!   escreve com `this` seguram esse `Weak`, para o item e o modelo não se prenderem uns aos outros.
//! - **Eventos**: `Event<T>` do `je-base`; um evento que carrega o item leva o `Rc` dele. `Partial<INotificationsFilter>`
//!   é [`NotificationsFilterUpdate`] (cada campo opcional) e `Map<string, NotificationsFilter>` é um [`OrderedMap`].
//! - **`NotificationMessage`** é o enum do `je-platform`, com a variante `ErrorWithActions` acrescentada lá
//!   (`isErrorWithActions(notification.message)`). Os valores que o TypeScript aceita em tempo de execução e o
//!   tipo já recusa (`message: null`, `severity` que não é número, `legacyExtensionLinkParsing` que não é a
//!   capacidade) não são representáveis; o padrão `Severity.Info` do original fica sem uso porque a severidade
//!   é obrigatória.
//! - **`NotificationViewItem.actions`** devolve [`NotificationActions`] (o original devolve `undefined` só se
//!   `setActions` nunca rodou, o que o construtor garante).
//! - **`NotificationsModel.NO_OP_NOTIFICATION`** (estático compartilhado) é um campo do modelo; como a
//!   identidade do handle não é observável pelo contrato, o efeito é o mesmo.
//! - **`ChoiceAction extends Action`** é composição: guarda uma [`Action`] e repassa o contrato de [`IAction`]
//!   a ela (o `as_any` devolve o próprio `ChoiceAction`, que é o que o `instanceof` precisa). A telemetria da ação
//!   é registrada no `Rc<ChoiceAction>` final, a identidade que quem consome vê.
//! - **`StatusMessageViewItem`** (a classe privada com `create`) é a função [`create_status_message_view_item`];
//!   `IStatusMessageViewItem` é [`StatusMessageViewItem`].
//! - **`NotificationsSettings`** e **`NotificationsPosition`** (`const enum` de texto) são enums com `as_str`.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::actions::{Action, ActionCallback, IAction, IActionChangeEvent};
use je_base::arrays;
use je_base::dynamic::Dynamic;
use je_base::error_message::to_error_message_of;
use je_base::errors::is_cancellation_error;
use je_base::event::{Emitter, Event};
use je_base::js_string::{substring, trim_end, trim_start};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::linked_text::{Link, LinkedText, LinkedTextNode, parse_linked_text};
use je_base::map::{OrderedMap, maps_strict_equal_ignore_order};
use je_base::promise::{MaybePromise, Promise};
use je_platform::configuration::service::IConfigurationService;
use je_platform::notification::notification_legacy::is_legacy_extension_link_parsing;
use je_platform::notification::notification_telemetry::{
    NotificationActionTelemetry, NotificationTelemetrySource, get_notification_telemetry_source, with_notification_action_telemetry,
};
use je_platform::notification::{
    BasePromptChoice, NoOpNotification, Notification, NotificationActions, NotificationHandle as INotificationHandle, NotificationMessage, NotificationPriority,
    NotificationProgress, NotificationProgressProperties, NotificationSourceOrText, NotificationsFilter, PromptChoiceKind, Severity, StatusHandle,
    StatusMessageOptions,
};

use super::notification_telemetry::inherit_notification_telemetry;

/// Onde um item de mensagem (`MAX_MESSAGE_LENGTH`) é cortado, em unidades UTF-16.
const MAX_MESSAGE_LENGTH: usize = 1000;

/// `INotificationsModel`.
pub trait INotificationsModel {
    // #region Notifications as Toasts/Center

    fn notifications(&self) -> Vec<Rc<NotificationViewItem>>;

    fn on_did_change_notification(&self) -> Event<NotificationChangeEvent>;
    fn on_did_change_filter(&self) -> Event<NotificationsFilterUpdate>;

    fn add_notification(&self, notification: Notification) -> Rc<dyn INotificationHandle>;

    fn set_filter(&self, filter: NotificationsFilterUpdate);

    // #endregion

    // #region Notifications as Status

    fn status_message(&self) -> Option<Rc<StatusMessageViewItem>>;

    fn on_did_change_status_message(&self) -> Event<StatusMessageChangeEvent>;

    fn show_status_message(&self, message: NotificationMessage, options: Option<StatusMessageOptions>) -> Rc<dyn StatusHandle>;

    // #endregion
}

/// `NotificationChangeType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationChangeType {
    /// A notification was added.
    Add,

    /// A notification changed. Check `detail` property on the event for additional information.
    Change,

    /// A notification expanded or collapsed.
    ExpandCollapse,

    /// A notification was removed.
    Remove,
}

/// `INotificationChangeEvent`.
#[derive(Clone)]
pub struct NotificationChangeEvent {
    /// The index this notification has in the list of notifications.
    pub index: usize,

    /// The notification this change is about.
    pub item: Rc<NotificationViewItem>,

    /// The kind of notification change.
    pub kind: NotificationChangeType,

    /// Additional detail about the item change. Only applies to `NotificationChangeType::Change`.
    pub detail: Option<NotificationViewItemContentChangeKind>,

    /// Whether progress changed between active and inactive.
    pub active_progress_changed: Option<bool>,
}

/// `StatusMessageChangeType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusMessageChangeType {
    Add,
    Remove,
}

/// `IStatusMessageViewItem`.
#[derive(Clone, Debug)]
pub struct StatusMessageViewItem {
    pub message: String,
    pub options: Option<StatusMessageOptions>,
}

/// `IStatusMessageChangeEvent`.
#[derive(Clone)]
pub struct StatusMessageChangeEvent {
    /// The status message item this change is about.
    pub item: Rc<StatusMessageViewItem>,

    /// The kind of status message change.
    pub kind: StatusMessageChangeType,
}

/// `NotificationHandle`: o handle que o serviço devolve a quem notifica.
pub struct NotificationHandle {
    base: Rc<Disposable>,
    item: Rc<NotificationViewItem>,
    on_close: Rc<dyn Fn(&Rc<NotificationViewItem>)>,
    on_did_close: Rc<Emitter<()>>,
    on_did_change_visibility: Rc<Emitter<bool>>,
}

impl NotificationHandle {
    pub fn new(item: Rc<NotificationViewItem>, on_close: Rc<dyn Fn(&Rc<NotificationViewItem>)>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_close = Emitter::new();
        base.register(on_did_close.clone());
        let on_did_change_visibility = Emitter::new();
        base.register(on_did_change_visibility.clone());

        // Visibility
        let visibility = on_did_change_visibility.clone();
        base.register(item.on_did_change_visibility().listen(move |visible| visibility.fire(*visible)));

        // Closing
        let (closed, owner) = (on_did_close.clone(), base.clone());
        item.on_did_close().once().listen(move |_| {
            closed.fire(());

            owner.dispose();
        });

        Rc::new(Self { base, item, on_close, on_did_close, on_did_change_visibility })
    }
}

impl IDisposable for NotificationHandle {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl INotificationHandle for NotificationHandle {
    fn visible(&self) -> Option<bool> {
        Some(self.item.visible())
    }

    fn on_did_close(&self) -> Event<()> {
        self.on_did_close.event()
    }

    fn on_did_change_visibility(&self) -> Event<bool> {
        self.on_did_change_visibility.event()
    }

    fn progress(&self) -> Rc<dyn NotificationProgress> {
        self.item.progress()
    }

    fn update_severity(&self, severity: Severity) {
        self.item.update_severity(severity);
    }

    fn update_message(&self, message: NotificationMessage) {
        self.item.update_message(message);
    }

    fn update_actions(&self, actions: Option<NotificationActions>) {
        self.item.update_actions(actions);
    }

    fn close(&self) {
        (self.on_close)(&self.item);

        self.base.dispose();
    }
}

/// `INotificationsFilter`.
#[derive(Clone, Debug)]
pub struct NotificationsFilterState {
    pub global: NotificationsFilter,
    pub sources: OrderedMap<String, NotificationsFilter>,
}

impl Default for NotificationsFilterState {
    /// O filtro de partida do modelo: nada filtrado.
    fn default() -> Self {
        Self { global: NotificationsFilter::Off, sources: OrderedMap::new() }
    }
}

/// `Partial<INotificationsFilter>`.
#[derive(Clone, Debug, Default)]
pub struct NotificationsFilterUpdate {
    pub global: Option<NotificationsFilter>,
    pub sources: Option<OrderedMap<String, NotificationsFilter>>,
}

/// `StatusHandle` de [`NotificationsModel::show_status_message`]: o fecho que fecha a mensagem.
struct StatusMessageHandle(Box<dyn Fn()>);

impl StatusHandle for StatusMessageHandle {
    fn close(&self) {
        (self.0)();
    }
}

/// `NotificationsModel`.
pub struct NotificationsModel {
    base: Rc<Disposable>,
    me: Weak<NotificationsModel>,

    on_did_change_notification: Rc<Emitter<NotificationChangeEvent>>,
    on_did_change_status_message: Rc<Emitter<StatusMessageChangeEvent>>,
    on_did_change_filter: Rc<Emitter<NotificationsFilterUpdate>>,

    notifications: RefCell<Vec<Rc<NotificationViewItem>>>,
    status_message: Rc<RefCell<Option<Rc<StatusMessageViewItem>>>>,

    filter: RefCell<NotificationsFilterState>,

    no_op_notification: Rc<NoOpNotification>,
}

impl NotificationsModel {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_notification = Emitter::new();
        base.register(on_did_change_notification.clone());
        let on_did_change_status_message = Emitter::new();
        base.register(on_did_change_status_message.clone());
        let on_did_change_filter = Emitter::new();
        base.register(on_did_change_filter.clone());

        Rc::new_cyclic(|me| Self {
            base,
            me: me.clone(),
            on_did_change_notification,
            on_did_change_status_message,
            on_did_change_filter,
            notifications: RefCell::new(Vec::new()),
            status_message: Rc::new(RefCell::new(None)),
            filter: RefCell::new(NotificationsFilterState::default()),
            no_op_notification: Rc::new(NoOpNotification::new()),
        })
    }

    fn on_close(&self, item: &Rc<NotificationViewItem>) {
        match self.find_notification(item) {
            Some(live_item) if !Rc::ptr_eq(&live_item, item) => {
                live_item.close(); // item could have been replaced with another one, make sure to close the live item
            }
            _ => {
                item.close(); // otherwise just close the item that was passed in
            }
        }
    }

    fn find_notification(&self, item: &NotificationViewItem) -> Option<Rc<NotificationViewItem>> {
        let notifications = self.notifications.borrow().clone();
        notifications.into_iter().find(|notification| notification.equals(item))
    }

    fn index_of(&self, item: &Rc<NotificationViewItem>) -> Option<usize> {
        self.notifications.borrow().iter().position(|notification| Rc::ptr_eq(notification, item))
    }

    fn fire_notification_change_event(
        &self,
        item: &Rc<NotificationViewItem>,
        kind: NotificationChangeType,
        detail: Option<NotificationViewItemContentChangeKind>,
        active_progress_changed: Option<bool>,
    ) {
        if let Some(index) = self.index_of(item) {
            self.on_did_change_notification.fire(NotificationChangeEvent { item: item.clone(), index, kind, detail, active_progress_changed });
        }
    }

    fn create_view_item(&self, notification: &Notification) -> Option<Rc<NotificationViewItem>> {
        let item = NotificationViewItem::create(notification, &self.filter.borrow())?;

        // Item Events
        let (model, weak_item) = (self.me.clone(), Rc::downgrade(&item));
        let fire_notification_change_event = Rc::new(move |kind, detail, active_progress_changed| {
            if let (Some(model), Some(item)) = (model.upgrade(), weak_item.upgrade()) {
                model.fire_notification_change_event(&item, kind, detail, active_progress_changed);
            }
        });

        let fire = fire_notification_change_event.clone();
        let item_expansion_change_listener = item.on_did_change_expansion().listen(move |_| fire(NotificationChangeType::ExpandCollapse, None, None));
        let item_content_change_listener =
            item.on_did_change_content().listen(move |e| fire_notification_change_event(NotificationChangeType::Change, Some(e.kind), e.active_progress_changed));

        let (model, weak_item) = (self.me.clone(), Rc::downgrade(&item));
        item.on_did_close().once().listen(move |_| {
            item_expansion_change_listener.dispose();
            item_content_change_listener.dispose();

            if let (Some(model), Some(item)) = (model.upgrade(), weak_item.upgrade())
                && let Some(index) = model.index_of(&item)
            {
                model.notifications.borrow_mut().remove(index);
                model.on_did_change_notification.fire(NotificationChangeEvent {
                    item,
                    index,
                    kind: NotificationChangeType::Remove,
                    detail: None,
                    active_progress_changed: None,
                });
            }
        });

        Some(item)
    }
}

impl IDisposable for NotificationsModel {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl INotificationsModel for NotificationsModel {
    fn notifications(&self) -> Vec<Rc<NotificationViewItem>> {
        self.notifications.borrow().clone()
    }

    fn status_message(&self) -> Option<Rc<StatusMessageViewItem>> {
        self.status_message.borrow().clone()
    }

    fn on_did_change_notification(&self) -> Event<NotificationChangeEvent> {
        self.on_did_change_notification.event()
    }

    fn on_did_change_status_message(&self) -> Event<StatusMessageChangeEvent> {
        self.on_did_change_status_message.event()
    }

    fn on_did_change_filter(&self) -> Event<NotificationsFilterUpdate> {
        self.on_did_change_filter.event()
    }

    fn set_filter(&self, filter: NotificationsFilterUpdate) {
        let mut global_changed = false;
        if let Some(global) = filter.global {
            let mut current = self.filter.borrow_mut();
            global_changed = current.global != global;
            current.global = global;
        }

        let mut sources_changed = false;
        if let Some(sources) = &filter.sources {
            let mut current = self.filter.borrow_mut();
            sources_changed = !maps_strict_equal_ignore_order(&current.sources, sources);
            current.sources = sources.clone();
        }

        if global_changed || sources_changed {
            self.on_did_change_filter.fire(NotificationsFilterUpdate {
                global: if global_changed { filter.global } else { None },
                sources: if sources_changed { filter.sources } else { None },
            });
        }
    }

    fn add_notification(&self, notification: Notification) -> Rc<dyn INotificationHandle> {
        let Some(item) = self.create_view_item(&notification) else {
            return self.no_op_notification.clone(); // return early if this is a no-op
        };

        // Deduplicate
        if let Some(duplicate) = self.find_notification(&item) {
            inherit_notification_telemetry(&duplicate, &item);
            duplicate.close();
        }

        // Add to list as first entry
        self.notifications.borrow_mut().insert(0, item.clone());

        // Events
        self.on_did_change_notification.fire(NotificationChangeEvent {
            item: item.clone(),
            index: 0,
            kind: NotificationChangeType::Add,
            detail: None,
            active_progress_changed: None,
        });

        // Wrap into handle
        let model = self.me.clone();
        NotificationHandle::new(
            item,
            Rc::new(move |item| {
                if let Some(model) = model.upgrade() {
                    model.on_close(item);
                }
            }),
        )
    }

    fn show_status_message(&self, message: NotificationMessage, options: Option<StatusMessageOptions>) -> Rc<dyn StatusHandle> {
        let Some(item) = create_status_message_view_item(&message, options).map(Rc::new) else {
            return Rc::new(StatusMessageHandle(Box::new(|| {})));
        };

        *self.status_message.borrow_mut() = Some(item.clone());
        self.on_did_change_status_message.fire(StatusMessageChangeEvent { kind: StatusMessageChangeType::Add, item: item.clone() });

        let (status_message, on_did_change_status_message) = (self.status_message.clone(), self.on_did_change_status_message.clone());
        Rc::new(StatusMessageHandle(Box::new(move || {
            let is_current = status_message.borrow().as_ref().is_some_and(|current| Rc::ptr_eq(current, &item));
            if is_current {
                *status_message.borrow_mut() = None;
                on_did_change_status_message.fire(StatusMessageChangeEvent { kind: StatusMessageChangeType::Remove, item: item.clone() });
            }
        })))
    }
}

/// `isNotificationViewItem`: `obj instanceof NotificationViewItem`.
pub fn is_notification_view_item(obj: &dyn Any) -> bool {
    obj.is::<NotificationViewItem>() || obj.is::<Rc<NotificationViewItem>>()
}

/// `NotificationViewItemContentChangeKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationViewItemContentChangeKind {
    Severity,
    Message,
    Actions,
    Progress,
}

/// `INotificationViewItemContentChangeEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotificationViewItemContentChangeEvent {
    pub kind: NotificationViewItemContentChangeKind,
    pub active_progress_changed: Option<bool>,
}

/// `INotificationViewItemProgressState`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NotificationViewItemProgressState {
    pub infinite: Option<bool>,
    pub total: Option<f64>,
    pub worked: Option<f64>,
    pub done: Option<bool>,
}

/// `NotificationViewItemProgress` (e `INotificationViewItemProgress`).
pub struct NotificationViewItemProgress {
    base: Rc<Disposable>,
    state: Cell<NotificationViewItemProgressState>,
    on_did_change: Rc<Emitter<()>>,
}

impl NotificationViewItemProgress {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());

        Rc::new(Self { base, state: Cell::new(NotificationViewItemProgressState::default()), on_did_change })
    }

    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    pub fn state(&self) -> NotificationViewItemProgressState {
        self.state.get()
    }

    /// Aplica a mudança ao estado e avisa. Cada método de progresso decide antes se há mudança.
    fn change(&self, apply: impl FnOnce(&mut NotificationViewItemProgressState)) {
        let mut state = self.state.get();
        apply(&mut state);
        self.state.set(state);

        self.on_did_change.fire(());
    }
}

impl IDisposable for NotificationViewItemProgress {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl NotificationProgress for NotificationViewItemProgress {
    fn infinite(&self) {
        if self.state.get().infinite == Some(true) {
            return;
        }

        self.change(|state| {
            state.infinite = Some(true);

            state.total = None;
            state.worked = None;
            state.done = None;
        });
    }

    fn done(&self) {
        if self.state.get().done == Some(true) {
            return;
        }

        self.change(|state| {
            state.done = Some(true);

            state.infinite = None;
            state.total = None;
            state.worked = None;
        });
    }

    fn total(&self, value: f64) {
        if self.state.get().total == Some(value) {
            return;
        }

        self.change(|state| {
            state.total = Some(value);

            state.infinite = None;
            state.done = None;
        });
    }

    fn worked(&self, value: f64) {
        self.change(|state| {
            state.worked = Some(state.worked.map_or(value, |worked| worked + value));

            state.infinite = None;
            state.done = None;
        });
    }
}

/// `IMessageLink`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageLink {
    pub href: String,
    pub name: String,
    pub title: String,
    pub offset: usize,
    pub length: usize,
}

/// `INotificationMessage`.
pub struct NotificationItemMessage {
    pub raw: String,
    pub original: NotificationMessage,
    pub linked_text: LinkedText,
}

/// O texto de uma mensagem de notificação: o `toErrorMessage` de um erro e o `toString` do resto. Vazio é
/// "sem mensagem para mostrar".
fn notification_message_text(input: &NotificationMessage) -> Option<String> {
    let text = match input {
        NotificationMessage::Text(text) => text.clone(),
        NotificationMessage::Error(error) => to_error_message_of(error, false),
        NotificationMessage::ErrorWithActions(error) => to_error_message_of(&error.error, false),
        NotificationMessage::Rich(text) => text.to_string(),
    };

    (!text.is_empty()).then_some(text)
}

/// `isCancellationError(message)`.
fn is_cancellation(message: &NotificationMessage) -> bool {
    match message {
        NotificationMessage::Error(error) => is_cancellation_error(error),
        NotificationMessage::ErrorWithActions(error) => is_cancellation_error(&error.error),
        NotificationMessage::Text(_) | NotificationMessage::Rich(_) => false,
    }
}

/// `text.replace(/\r\n|\n|\r/g, ' ')`.
fn replace_line_breaks(units: &[u16]) -> Vec<u16> {
    const CR: u16 = b'\r' as u16;
    const LF: u16 = b'\n' as u16;
    const SPACE: u16 = b' ' as u16;

    let mut out = Vec::with_capacity(units.len());
    let mut i = 0;
    while i < units.len() {
        match units[i] {
            CR if units.get(i + 1) == Some(&LF) => {
                out.push(SPACE);
                i += 2;
            }
            CR | LF => {
                out.push(SPACE);
                i += 1;
            }
            unit => {
                out.push(unit);
                i += 1;
            }
        }
    }
    out
}

fn parse_notification_message(input: &NotificationMessage, parse_legacy_links: bool) -> Option<NotificationItemMessage> {
    let message = notification_message_text(input)?; // we need a message to show

    let rich = match input {
        NotificationMessage::Rich(text) => Some(text),
        _ => None,
    };
    let mut linked_text = match rich {
        Some(text) => normalize_linked_text(text.nodes()),
        None => normalize_linked_text(&[LinkedTextNode::Text(JsString::from(message.as_str()))]),
    };
    if parse_legacy_links && rich.is_none() {
        let parsed = parse_linked_text(linked_text.to_js_string().units());
        linked_text = parsed;
    }

    Some(NotificationItemMessage { raw: message, linked_text, original: input.clone() })
}

fn normalize_linked_text(input: &[LinkedTextNode]) -> LinkedText {
    let mut nodes: Vec<LinkedTextNode> = Vec::new();
    let mut length = 0;
    for node in input {
        let text = match node {
            LinkedTextNode::Text(text) => text,
            LinkedTextNode::Link(link) => &link.label,
        };
        let remaining = MAX_MESSAGE_LENGTH.saturating_sub(length) as i64;
        let value = JsString::from_units(replace_line_breaks(substring(text.units(), 0, remaining)));
        if !value.is_empty() {
            match node {
                LinkedTextNode::Text(_) => match nodes.last_mut() {
                    Some(LinkedTextNode::Text(last)) => last.push_units(value.units()),
                    _ => nodes.push(LinkedTextNode::Text(value)),
                },
                LinkedTextNode::Link(link) => nodes.push(LinkedTextNode::Link(Link { label: value, ..link.clone() })),
            }
        }
        length += text.len();
        if length > MAX_MESSAGE_LENGTH {
            nodes.push(LinkedTextNode::Text(JsString::from("...")));
            break;
        }
    }

    if let Some(LinkedTextNode::Text(first)) = nodes.first_mut() {
        let trimmed = JsString::from(trim_start(first.units()));
        *first = trimmed;
    }
    if let Some(LinkedTextNode::Text(last)) = nodes.last_mut() {
        let trimmed = JsString::from(trim_end(last.units()));
        *last = trimmed;
    }
    LinkedText::new(nodes.into_iter().filter(|node| !matches!(node, LinkedTextNode::Text(text) if text.is_empty())).collect())
}

fn messages_equal(message: &NotificationItemMessage, other: &NotificationItemMessage) -> bool {
    message.raw == other.raw && message.linked_text.nodes == other.linked_text.nodes
}

/// `NotificationViewItem`.
pub struct NotificationViewItem {
    me: Weak<NotificationViewItem>,
    base: Rc<Disposable>,

    pub id: Option<String>,
    pub telemetry: NotificationTelemetrySource,
    severity: Cell<Severity>,
    sticky: bool,
    priority: NotificationPriority,
    message: RefCell<Rc<NotificationItemMessage>>,
    parse_legacy_links: bool,
    source: Option<NotificationSourceOrText>,

    expanded: Cell<bool>,
    visible: Cell<bool>,

    actions: RefCell<NotificationActions>,
    progress: RefCell<Option<Rc<NotificationViewItemProgress>>>,

    on_did_change_expansion: Rc<Emitter<()>>,
    on_did_close: Rc<Emitter<()>>,
    on_did_change_content: Rc<Emitter<NotificationViewItemContentChangeEvent>>,
    on_did_change_visibility: Rc<Emitter<bool>>,
}

impl NotificationViewItem {
    pub fn create(notification: &Notification, filter: &NotificationsFilterState) -> Option<Rc<Self>> {
        if is_cancellation(&notification.message) {
            return None; // we need a message to show
        }

        let severity = notification.severity;

        let parse_legacy_links = is_legacy_extension_link_parsing(notification.properties.legacy_extension_link_parsing.as_ref());
        let message = parse_notification_message(&notification.message, parse_legacy_links)?; // we need a message to show

        let actions = match (&notification.actions, &notification.message) {
            (Some(actions), _) => Some(actions.clone()),
            (None, NotificationMessage::ErrorWithActions(error)) => Some(NotificationActions { primary: error.actions.clone(), secondary: Vec::new() }),
            (None, _) => None,
        };

        let mut priority = notification.properties.priority.unwrap_or(NotificationPriority::Default);
        if matches!(priority, NotificationPriority::Default | NotificationPriority::Optional) && severity != Severity::Error {
            if filter.global == NotificationsFilter::Error {
                priority = NotificationPriority::Silent; // filtered globally
            } else if let Some(NotificationSourceOrText::Source(source)) = &notification.source
                && filter.sources.get(&source.id) == Some(&NotificationsFilter::Error)
            {
                priority = NotificationPriority::Silent; // filtered by source
            }
        }

        Some(Self::new(
            notification.id.clone(),
            get_notification_telemetry_source(notification.properties.telemetry.as_ref()),
            severity,
            notification.properties.sticky,
            priority,
            message,
            parse_legacy_links,
            notification.source.clone(),
            notification.progress,
            actions,
        ))
    }

    #[allow(clippy::too_many_arguments)] // os campos do construtor privado do original
    fn new(
        id: Option<String>,
        telemetry: NotificationTelemetrySource,
        severity: Severity,
        sticky: bool,
        priority: NotificationPriority,
        message: NotificationItemMessage,
        parse_legacy_links: bool,
        source: Option<NotificationSourceOrText>,
        progress: Option<NotificationProgressProperties>,
        actions: Option<NotificationActions>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_expansion = Emitter::new();
        base.register(on_did_change_expansion.clone());
        let on_did_close = Emitter::new();
        base.register(on_did_close.clone());
        let on_did_change_content = Emitter::new();
        base.register(on_did_change_content.clone());
        let on_did_change_visibility = Emitter::new();
        base.register(on_did_change_visibility.clone());

        let item = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            id,
            telemetry,
            severity: Cell::new(severity),
            sticky,
            priority,
            message: RefCell::new(Rc::new(message)),
            parse_legacy_links,
            source,
            expanded: Cell::new(false),
            visible: Cell::new(false),
            actions: RefCell::new(NotificationActions::default()),
            progress: RefCell::new(None),
            on_did_change_expansion,
            on_did_close,
            on_did_change_content,
            on_did_change_visibility,
        });

        if let Some(progress) = progress {
            item.set_progress(&progress);
        }

        item.set_actions(actions);

        item
    }

    fn set_progress(&self, progress: &NotificationProgressProperties) {
        // `if (progress.infinite)` e `else if (progress.total)`: zero e NaN são falsos como em JavaScript.
        let truthy = |value: Option<f64>| value.filter(|value| *value != 0.0 && !value.is_nan());
        if progress.infinite {
            self.progress().infinite();
        } else if let Some(total) = truthy(progress.total) {
            self.progress().total(total);

            if let Some(worked) = truthy(progress.worked) {
                self.progress().worked(worked);
            }
        }
    }

    fn set_actions(&self, actions: Option<NotificationActions>) {
        let actions = actions.unwrap_or_default();

        self.expanded.set(!actions.primary.is_empty());
        *self.actions.borrow_mut() = actions;
    }

    pub fn on_did_change_expansion(&self) -> Event<()> {
        self.on_did_change_expansion.event()
    }

    pub fn on_did_close(&self) -> Event<()> {
        self.on_did_close.event()
    }

    pub fn on_did_change_content(&self) -> Event<NotificationViewItemContentChangeEvent> {
        self.on_did_change_content.event()
    }

    pub fn on_did_change_visibility(&self) -> Event<bool> {
        self.on_did_change_visibility.event()
    }

    pub fn can_collapse(&self) -> bool {
        !self.has_actions()
    }

    pub fn expanded(&self) -> bool {
        self.expanded.get()
    }

    pub fn severity(&self) -> Severity {
        self.severity.get()
    }

    pub fn sticky(&self) -> bool {
        if self.sticky {
            return true; // explicitly sticky
        }

        let has_actions = self.has_actions();
        // notification errors with actions are sticky, notifications that got expanded are sticky and
        // notifications with running progress are sticky
        (has_actions && self.severity.get() == Severity::Error) || (!has_actions && self.expanded.get()) || self.has_active_progress()
    }

    pub fn priority(&self) -> NotificationPriority {
        self.priority
    }

    fn has_actions(&self) -> bool {
        !self.actions.borrow().primary.is_empty()
    }

    pub fn has_progress(&self) -> bool {
        self.progress.borrow().is_some()
    }

    pub fn has_active_progress(&self) -> bool {
        let Some(progress) = self.progress.borrow().clone() else {
            return false;
        };
        let state = progress.state();
        state.done != Some(true) && (state.infinite == Some(true) || state.total.is_some() || state.worked.is_some())
    }

    /// `get progress()`: cria o progresso na primeira vez que alguém pede.
    pub fn progress(&self) -> Rc<NotificationViewItemProgress> {
        if let Some(progress) = self.progress.borrow().clone() {
            return progress;
        }

        let progress = NotificationViewItemProgress::new();
        self.base.register(progress.clone());
        *self.progress.borrow_mut() = Some(progress.clone());

        let was_active = Cell::new(self.has_active_progress());
        let item = self.me.clone();
        self.base.register(progress.on_did_change().listen(move |_| {
            let Some(item) = item.upgrade() else {
                return;
            };
            let is_active = item.has_active_progress();
            let active_progress_changed = was_active.get() != is_active;
            was_active.set(is_active);
            item.on_did_change_content.fire(NotificationViewItemContentChangeEvent {
                kind: NotificationViewItemContentChangeKind::Progress,
                active_progress_changed: Some(active_progress_changed),
            });
        }));

        progress
    }

    pub fn message(&self) -> Rc<NotificationItemMessage> {
        self.message.borrow().clone()
    }

    pub fn source(&self) -> Option<String> {
        match &self.source {
            Some(NotificationSourceOrText::Text(text)) => Some(text.clone()),
            Some(NotificationSourceOrText::Source(source)) => Some(source.label.clone()),
            None => None,
        }
    }

    pub fn source_id(&self) -> Option<String> {
        match &self.source {
            Some(NotificationSourceOrText::Source(source)) => Some(source.id.clone()),
            _ => None,
        }
    }

    pub fn actions(&self) -> NotificationActions {
        self.actions.borrow().clone()
    }

    pub fn visible(&self) -> bool {
        self.visible.get()
    }

    pub fn update_severity(&self, severity: Severity) {
        if severity == self.severity.get() {
            return;
        }

        self.severity.set(severity);
        self.on_did_change_content.fire(NotificationViewItemContentChangeEvent { kind: NotificationViewItemContentChangeKind::Severity, active_progress_changed: None });
    }

    pub fn update_message(&self, input: NotificationMessage) {
        let Some(message) = parse_notification_message(&input, self.parse_legacy_links) else {
            return;
        };
        if messages_equal(&message, &self.message()) {
            return;
        }

        *self.message.borrow_mut() = Rc::new(message);
        self.on_did_change_content.fire(NotificationViewItemContentChangeEvent { kind: NotificationViewItemContentChangeKind::Message, active_progress_changed: None });
    }

    pub fn update_actions(&self, actions: Option<NotificationActions>) {
        self.set_actions(actions);
        self.on_did_change_content.fire(NotificationViewItemContentChangeEvent { kind: NotificationViewItemContentChangeKind::Actions, active_progress_changed: None });
    }

    pub fn update_visibility(&self, visible: bool) {
        if self.visible.get() != visible {
            self.visible.set(visible);

            self.on_did_change_visibility.fire(visible);
        }
    }

    pub fn expand(&self) {
        if self.expanded.get() || !self.can_collapse() {
            return;
        }

        self.expanded.set(true);
        self.on_did_change_expansion.fire(());
    }

    pub fn collapse(&self, skip_events: bool) {
        if !self.expanded.get() || !self.can_collapse() {
            return;
        }

        self.expanded.set(false);

        if !skip_events {
            self.on_did_change_expansion.fire(());
        }
    }

    pub fn toggle(&self) {
        if self.expanded.get() {
            self.collapse(false);
        } else {
            self.expand();
        }
    }

    pub fn close(&self) {
        self.on_did_close.fire(());

        self.base.dispose();
    }

    pub fn equals(&self, other: &NotificationViewItem) -> bool {
        if self.has_progress() || other.has_progress() {
            return false;
        }

        if self.id.is_some() || other.id.is_some() {
            return self.id == other.id;
        }

        match &self.source {
            Some(NotificationSourceOrText::Source(source)) => {
                if Some(&source.label) != other.source().as_ref() || Some(&source.id) != other.source_id().as_ref() {
                    return false;
                }
            }
            _ => {
                if self.source() != other.source() {
                    return false;
                }
            }
        }

        if !messages_equal(&self.message(), &other.message()) {
            return false;
        }

        let (primary_actions, other_primary_actions) = (self.actions().primary, other.actions().primary);
        arrays::equals(Some(&primary_actions[..]), Some(&other_primary_actions[..]), |action, other_action| {
            format!("{}{}", action.id(), action.label()) == format!("{}{}", other_action.id(), other_action.label())
        })
    }
}

impl IDisposable for NotificationViewItem {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// Repassa a [`ChoiceAction`] os métodos de [`IAction`] que o `extends Action` do original herda.
macro_rules! forward_to_action {
    ($($method:ident($($arg:ident: $ty:ty),*) $(-> $ret:ty)?;)*) => {
        $(
            fn $method(&self, $($arg: $ty),*) $(-> $ret)? {
                self.action.$method($($arg),*)
            }
        )*
    };
}

/// `ChoiceAction`.
pub struct ChoiceAction {
    base: Rc<Disposable>,
    action: Rc<Action>,
    on_did_run: Rc<Emitter<()>>,
    keep_open: bool,
    menu: Option<Vec<Rc<ChoiceAction>>>,
}

impl ChoiceAction {
    pub fn new(id: &str, choice: &PromptChoiceKind) -> Rc<Self> {
        match choice {
            PromptChoiceKind::Plain(choice) => Self::create(id, &choice.base, None),
            PromptChoiceKind::WithMenu(choice) => Self::create(id, &choice.base, Some(&choice.menu)),
        }
    }

    /// O construtor do original, com o submenu (`IPromptChoiceWithMenu.menu`) à parte: uma escolha comum não tem.
    fn create(id: &str, choice: &BasePromptChoice, menu: Option<&[BasePromptChoice]>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_run = Emitter::new();
        base.register(on_did_run.clone());

        let callback: ActionCallback = {
            let (run, on_did_run) = (choice.run.clone(), on_did_run.clone());
            Rc::new(move |_: Dynamic| -> MaybePromise<Dynamic> {
                // Pass to runner
                run();

                // Emit Event
                on_did_run.fire(());

                Dynamic::Undefined.into()
            })
        };
        let action = Action::new(id, &choice.label, "", true, Some(callback));
        base.register(action.clone());

        let menu = menu.map(|menu| menu.iter().enumerate().map(|(index, choice)| Self::create(&format!("{id}.{index}"), choice, None)).collect());
        let this = Rc::new(Self { base, action, on_did_run, keep_open: choice.keep_open, menu });
        with_notification_action_telemetry(this.clone(), choice.telemetry_id.map(NotificationActionTelemetry::Id));
        this
    }

    pub fn on_did_run(&self) -> Event<()> {
        self.on_did_run.event()
    }

    /// O `onDidChange` que a classe herda de `Action`.
    pub fn on_did_change(&self) -> Event<IActionChangeEvent> {
        self.action.on_did_change()
    }

    pub fn menu(&self) -> Option<&[Rc<ChoiceAction>]> {
        self.menu.as_deref()
    }

    pub fn keep_open(&self) -> bool {
        self.keep_open
    }
}

impl IDisposable for ChoiceAction {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "ChoiceAction"
    }
}

impl IAction for ChoiceAction {
    forward_to_action! {
        id() -> String;
        label() -> String;
        set_label(value: String);
        tooltip() -> String;
        set_tooltip(value: String);
        class() -> Option<String>;
        set_class(value: Option<String>);
        enabled() -> bool;
        set_enabled(value: bool);
        checked() -> Option<bool>;
        set_checked(value: Option<bool>);
        run(args: &[Dynamic]) -> Promise<Dynamic>;
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `StatusMessageViewItem.create`.
fn create_status_message_view_item(notification: &NotificationMessage, options: Option<StatusMessageOptions>) -> Option<StatusMessageViewItem> {
    if is_cancellation(notification) {
        return None; // we need a message to show
    }

    let message = notification_message_text(notification)?; // we need a message to show

    Some(StatusMessageViewItem { message, options })
}

/// `NotificationsSettings`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationsSettings {
    NotificationsPosition,
    NotificationsButton,
}

impl NotificationsSettings {
    /// A chave da configuração.
    pub fn as_str(self) -> &'static str {
        match self {
            NotificationsSettings::NotificationsPosition => "workbench.notifications.position",
            NotificationsSettings::NotificationsButton => "workbench.notifications.showInTitleBar",
        }
    }
}

/// `NotificationsPosition`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationsPosition {
    BottomRight,
    BottomLeft,
    TopRight,
}

impl NotificationsPosition {
    /// O valor de texto do original.
    pub fn as_str(self) -> &'static str {
        match self {
            NotificationsPosition::BottomRight => "bottom-right",
            NotificationsPosition::BottomLeft => "bottom-left",
            NotificationsPosition::TopRight => "top-right",
        }
    }
}

pub fn get_notifications_position(configuration_service: &dyn IConfigurationService) -> NotificationsPosition {
    let position = configuration_service.get_value(Some(NotificationsSettings::NotificationsPosition.as_str()), None);

    [NotificationsPosition::BottomLeft, NotificationsPosition::TopRight]
        .into_iter()
        .find(|candidate| position == Dynamic::from(candidate.as_str()))
        .unwrap_or(NotificationsPosition::BottomRight)
}
