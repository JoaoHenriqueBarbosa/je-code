//! Porte de `src/vs/workbench/services/notification/common/notificationService.ts`: o `NotificationService`, que
//! liga o modelo de notificações do workbench ao contrato `INotificationService`.
//!
//! Notas de tradução:
//! - **`extends Disposable`**: a struct guarda um [`Disposable`] por dentro, com o modelo e o emitter registrados.
//!   O que o original escreve com `this` num fecho (o ouvinte do modelo) segura um `Weak` do serviço, que nasce em
//!   `Rc::new_cyclic`.
//! - **`registerSingleton(INotificationService, NotificationService, InstantiationType.Delayed)`** não é portado: o
//!   registro de singletons é feito por quem monta os serviços.
//! - **`NotificationMessage | NotificationMessage[]`** de `info`, `warn` e `error` é o `Vec<NotificationMessage>` do
//!   contrato (uma mensagem só é um vetor de um item); a recursão do original vira o laço.
//! - **`mapSourceToFilter`** (`Map<string, INotificationSourceFilter>`) é um `Vec` que mantém a ordem de inserção e
//!   troca o valor no lugar, como o `Map` do JavaScript. O JSON de `perSourceDoNotDisturbMode` é a lista
//!   `{ id, label, filter }`, com `filter` numérico (`NotificationsFilter.OFF` é 0 e `ERROR` é 1).
//! - **O `handle` que os fechos de `notify` e `prompt` usam** antes de ele existir (`const handle` mais abaixo) é um
//!   espaço compartilhado preenchido logo depois que o modelo devolve o handle.
//! - `notification.actions = actions` (a mutação do argumento) é a atribuição ao `Notification` recebido por valor.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::actions::{Action, ActionCallback, ActionRef};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::json::{Object, Value};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::map::OrderedMap;
use je_base::nls::localize;
use je_base::promise::{MaybePromise, Promise};
use je_platform::notification::notification_telemetry::{NotificationActionTelemetry, NotificationActionTelemetryId, with_notification_action_telemetry};
use je_platform::notification::{
    BasePromptChoice, INotificationService, NeverShowAgainOptions, NeverShowAgainScope, NoOpNotification, Notification, NotificationActions, NotificationFilterSetting,
    NotificationHandle, NotificationMessage, NotificationProperties, NotificationSource, NotificationSourceFilter, NotificationsFilter,
    PromptChoice, PromptChoiceKind, PromptOptions, Severity, StatusHandle, StatusMessageOptions,
};
use je_platform::storage::storage::{StorageScope, StorageService, StorageTarget};
use je_storage::StorageValue;

use crate::common::notifications::{ChoiceAction, INotificationsModel, NotificationChangeType, NotificationsFilterUpdate, NotificationsModel};

const GLOBAL_FILTER_SETTINGS_KEY: &str = "notifications.doNotDisturbMode";
const PER_SOURCE_FILTER_SETTINGS_KEY: &str = "notifications.perSourceDoNotDisturbMode";

/// O espaço do `handle` que os fechos usam antes de o modelo devolvê-lo.
type HandleSlot = Rc<RefCell<Option<Rc<dyn NotificationHandle>>>>;

fn close_slot(slot: &HandleSlot) {
    let handle = slot.borrow().clone();
    if let Some(handle) = handle {
        handle.close();
    }
}

/// `NotificationService`.
pub struct NotificationService {
    base: Rc<Disposable>,
    me: Weak<NotificationService>,

    pub model: Rc<NotificationsModel>,
    storage_service: Rc<dyn StorageService>,

    on_did_change_filter: Rc<Emitter<()>>,
    global_filter_enabled: Cell<bool>,
    map_source_to_filter: RefCell<Vec<NotificationSourceFilter>>,
}

impl NotificationService {
    pub fn new(storage_service: Rc<dyn StorageService>) -> Rc<Self> {
        let base = Disposable::new();
        let model = NotificationsModel::new();
        base.register(model.clone());
        let on_did_change_filter = Emitter::new();
        base.register(on_did_change_filter.clone());

        let mut map_source_to_filter: Vec<NotificationSourceFilter> = Vec::new();
        let stored = storage_service.get_object(PER_SOURCE_FILTER_SETTINGS_KEY, StorageScope::Application, Some(Value::Array(Vec::new())));
        if let Some(Value::Array(entries)) = stored {
            for entry in &entries {
                if let Some(source_filter) = source_filter_from_json(entry) {
                    Self::set_in(&mut map_source_to_filter, source_filter);
                }
            }
        }

        let global_filter_enabled = storage_service.get_boolean(GLOBAL_FILTER_SETTINGS_KEY, StorageScope::Application, Some(false)).unwrap_or(false);

        let service = Rc::new_cyclic(|me| Self {
            base,
            me: me.clone(),
            model,
            storage_service,
            on_did_change_filter,
            global_filter_enabled: Cell::new(global_filter_enabled),
            map_source_to_filter: RefCell::new(map_source_to_filter),
        });

        service.update_filters();
        service.register_listeners();
        service
    }

    fn register_listeners(&self) {
        let me = self.me.clone();
        self.base.register(self.model.on_did_change_notification().listen(move |e| {
            let Some(this) = me.upgrade() else { return };
            if e.kind != NotificationChangeType::Add {
                return;
            }
            // Make sure to track sources for notifications by registering
            // them with our do not disturb system which is backed by storage
            let source = match (e.item.source_id(), e.item.source()) {
                (Some(id), Some(label)) => Some(NotificationSource { id, label }),
                _ => None,
            };
            if let Some(source) = source {
                let known = this.map_source_to_filter.borrow().iter().any(|existing| existing.source.id == source.id);
                if !known {
                    this.set_filter(NotificationFilterSetting::Source(NotificationSourceFilter { source, filter: NotificationsFilter::Off }));
                } else {
                    this.update_source_filter(&source);
                }
            }
        }));
    }

    /// `Map#set`: troca no lugar se a chave existe, senão acrescenta no fim.
    fn set_in(map: &mut Vec<NotificationSourceFilter>, value: NotificationSourceFilter) {
        match map.iter_mut().find(|existing| existing.source.id == value.source.id) {
            Some(existing) => *existing = value,
            None => map.push(value),
        }
    }

    // #region Filters

    fn update_source_filter(&self, source: &NotificationSource) {
        let existing = self.map_source_to_filter.borrow().iter().find(|existing| existing.source.id == source.id).cloned();
        let Some(existing) = existing else {
            return; // nothing to do
        };

        // Store into model and persist
        if existing.source.label != source.label {
            Self::set_in(&mut self.map_source_to_filter.borrow_mut(), NotificationSourceFilter { source: source.clone(), filter: existing.filter });
            self.save_source_filters();
        }
    }

    fn save_source_filters(&self) {
        let entries: Vec<Value> = self.map_source_to_filter.borrow().iter().map(source_filter_to_json).collect();
        self.storage_service.store(PER_SOURCE_FILTER_SETTINGS_KEY, StorageValue::Object(Value::Array(entries)), StorageScope::Application, StorageTarget::Machine);
    }

    fn update_filters(&self) {
        let mut sources = OrderedMap::new();
        for source_filter in self.map_source_to_filter.borrow().iter() {
            sources.insert(source_filter.source.id.clone(), source_filter.filter);
        }
        self.model.set_filter(NotificationsFilterUpdate {
            global: Some(if self.global_filter_enabled.get() { NotificationsFilter::Error } else { NotificationsFilter::Off }),
            sources: Some(sources),
        });
    }

    // #endregion

    fn add_messages(&self, severity: Severity, messages: Vec<NotificationMessage>) {
        for message in messages {
            self.model.add_notification(Notification::new(severity, message));
        }
    }

    fn to_storage_scope(options: &NeverShowAgainOptions) -> StorageScope {
        match options.scope {
            Some(NeverShowAgainScope::Application) => StorageScope::Application,
            Some(NeverShowAgainScope::Profile) => StorageScope::Profile,
            Some(NeverShowAgainScope::Workspace) => StorageScope::Workspace,
            None => StorageScope::Application,
        }
    }
}

fn source_filter_to_json(source_filter: &NotificationSourceFilter) -> Value {
    Value::Object(Object::from_iter([
        ("id", Value::from(source_filter.source.id.as_str())),
        ("label", Value::from(source_filter.source.label.as_str())),
        ("filter", Value::from(if source_filter.filter == NotificationsFilter::Error { 1.0 } else { 0.0 })),
    ]))
}

fn source_filter_from_json(value: &Value) -> Option<NotificationSourceFilter> {
    let Value::Object(object) = value else { return None };
    let text = |key: &str| match object.get(key) {
        Some(Value::String(text)) => Some(text.to_string_lossy()),
        _ => None,
    };
    let filter = match object.get("filter") {
        Some(Value::Number(number)) if *number == 1.0 => NotificationsFilter::Error,
        _ => NotificationsFilter::Off,
    };
    Some(NotificationSourceFilter { source: NotificationSource { id: text("id")?, label: text("label")? }, filter })
}

impl IDisposable for NotificationService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl INotificationService for NotificationService {
    fn on_did_change_filter(&self) -> Event<()> {
        self.on_did_change_filter.event()
    }

    fn set_filter(&self, filter: NotificationFilterSetting) {
        match filter {
            NotificationFilterSetting::Global(filter) => {
                if self.global_filter_enabled.get() == (filter == NotificationsFilter::Error) {
                    return; // no change
                }

                // Store into model and persist
                self.global_filter_enabled.set(filter == NotificationsFilter::Error);
                self.storage_service.store(GLOBAL_FILTER_SETTINGS_KEY, StorageValue::Bool(self.global_filter_enabled.get()), StorageScope::Application, StorageTarget::Machine);

                // Update model
                self.update_filters();

                // Events
                self.on_did_change_filter.fire(());
            }
            NotificationFilterSetting::Source(filter) => {
                let existing = self.map_source_to_filter.borrow().iter().find(|existing| existing.source.id == filter.source.id).cloned();
                if existing.is_some_and(|existing| existing.filter == filter.filter && existing.source.label == filter.source.label) {
                    return; // no change
                }

                // Store into model and persist
                Self::set_in(&mut self.map_source_to_filter.borrow_mut(), filter);
                self.save_source_filters();

                // Update model
                self.update_filters();
            }
        }
    }

    fn get_filter(&self, source: Option<&NotificationSource>) -> NotificationsFilter {
        if let Some(source) = source {
            return self.map_source_to_filter.borrow().iter().find(|existing| existing.source.id == source.id).map_or(NotificationsFilter::Off, |existing| existing.filter);
        }

        if self.global_filter_enabled.get() { NotificationsFilter::Error } else { NotificationsFilter::Off }
    }

    fn get_filters(&self) -> Vec<NotificationSourceFilter> {
        self.map_source_to_filter.borrow().clone()
    }

    fn remove_filter(&self, source_id: &str) {
        let removed = {
            let mut map = self.map_source_to_filter.borrow_mut();
            let before = map.len();
            map.retain(|existing| existing.source.id != source_id);
            map.len() != before
        };
        if removed {
            // Persist
            self.save_source_filters();

            // Update model
            self.update_filters();
        }
    }

    fn info(&self, message: Vec<NotificationMessage>) {
        self.add_messages(Severity::Info, message);
    }

    fn warn(&self, message: Vec<NotificationMessage>) {
        self.add_messages(Severity::Warning, message);
    }

    fn error(&self, message: Vec<NotificationMessage>) {
        self.add_messages(Severity::Error, message);
    }

    fn notify(&self, mut notification: Notification) -> Rc<dyn NotificationHandle> {
        let to_dispose = DisposableStore::new();
        let slot: HandleSlot = Rc::default();

        // Handle neverShowAgain option accordingly
        if let Some(never_show_again) = notification.properties.never_show_again.clone() {
            let scope = Self::to_storage_scope(&never_show_again);
            let id = never_show_again.id.clone();

            // If the user already picked to not show the notification
            // again, we return with a no-op notification here
            if self.storage_service.get_boolean(&id, scope, None).unwrap_or(false) {
                to_dispose.dispose();
                return Rc::new(NoOpNotification::new());
            }

            let (storage_service, handle_slot) = (self.storage_service.clone(), slot.clone());
            let callback: ActionCallback = Rc::new(move |_: Dynamic| -> MaybePromise<Dynamic> {
                // Close notification
                close_slot(&handle_slot);

                // Remember choice
                storage_service.store(&id, StorageValue::Bool(true), scope, StorageTarget::User);

                MaybePromise::Promise(Promise::resolved(Dynamic::Undefined))
            });
            let never_show_again_action = to_dispose.add_rc(Action::new("workbench.notification.neverShowAgain", &localize("neverShowAgain", "Don't Show Again", &[]), "", true, Some(callback)));
            let never_show_again_action: ActionRef =
                with_notification_action_telemetry(never_show_again_action, Some(NotificationActionTelemetry::Id(NotificationActionTelemetryId::NeverShowAgain)));

            // Insert as primary or secondary action
            let mut actions = notification.actions.take().unwrap_or_default();
            if !never_show_again.is_secondary {
                actions.primary.insert(0, never_show_again_action); // action comes first
            } else {
                actions.secondary.push(never_show_again_action); // actions comes last
            }

            notification.actions = Some(actions);
        }

        // Show notification
        let handle = self.model.add_notification(notification);
        *slot.borrow_mut() = Some(handle.clone());

        // Cleanup when notification gets disposed
        handle.on_did_close().once().listen(move |_| to_dispose.dispose());

        handle
    }

    fn prompt(&self, severity: Severity, message: NotificationMessage, choices: Vec<PromptChoiceKind>, options: Option<PromptOptions>) -> Rc<dyn NotificationHandle> {
        let mut choices = choices;

        // Handle neverShowAgain option accordingly
        if let Some(never_show_again) = options.as_ref().and_then(|options| options.properties.never_show_again.clone()) {
            let scope = Self::to_storage_scope(&never_show_again);
            let id = never_show_again.id.clone();

            // If the user already picked to not show the notification
            // again, we return with a no-op notification here
            if self.storage_service.get_boolean(&id, scope, None).unwrap_or(false) {
                return Rc::new(NoOpNotification::new());
            }

            let storage_service = self.storage_service.clone();
            let never_show_again_choice = PromptChoiceKind::Plain(PromptChoice {
                base: BasePromptChoice {
                    telemetry_id: Some(NotificationActionTelemetryId::NeverShowAgain),
                    label: localize("neverShowAgain", "Don't Show Again", &[]),
                    keep_open: false,
                    run: Rc::new(move || storage_service.store(&id, StorageValue::Bool(true), scope, StorageTarget::User)),
                },
                is_secondary: never_show_again.is_secondary,
            });

            // Insert as primary or secondary action
            if !never_show_again.is_secondary {
                choices.insert(0, never_show_again_choice); // action comes first
            } else {
                choices.push(never_show_again_choice); // actions comes last
            }
        }

        let choice_clicked = Rc::new(Cell::new(false));
        let to_dispose = DisposableStore::new();
        let slot: HandleSlot = Rc::default();

        // Convert choices into primary/secondary actions
        let mut primary_actions: Vec<ActionRef> = Vec::new();
        let mut secondary_actions: Vec<ActionRef> = Vec::new();
        for (index, choice) in choices.iter().enumerate() {
            let action = ChoiceAction::new(&format!("workbench.dialog.choice.{index}"), choice);
            let is_secondary = match choice {
                PromptChoiceKind::Plain(choice) => choice.is_secondary,
                PromptChoiceKind::WithMenu(_) => false,
            };
            if !is_secondary {
                primary_actions.push(action.clone());
            } else {
                secondary_actions.push(action.clone());
            }

            // React to action being clicked
            let (choice_clicked, handle_slot, keep_open) = (choice_clicked.clone(), slot.clone(), action.keep_open());
            to_dispose.add(action.on_did_run().listen(move |_| {
                choice_clicked.set(true);

                // Close notification unless we are told to keep open
                if !keep_open {
                    close_slot(&handle_slot);
                }
            }));

            to_dispose.add_rc(action);
        }

        // Show notification with actions
        let actions = NotificationActions { primary: primary_actions, secondary: secondary_actions };
        let properties = options.as_ref().map_or_else(NotificationProperties::default, |options| NotificationProperties {
            telemetry: options.properties.telemetry.clone(),
            legacy_extension_link_parsing: options.properties.legacy_extension_link_parsing,
            sticky: options.properties.sticky,
            priority: options.properties.priority,
            never_show_again: None,
        });
        let handle = self.notify(Notification { properties, id: None, severity, message, source: None,actions: Some(actions), progress: None });
        *slot.borrow_mut() = Some(handle.clone());

        let on_cancel = options.and_then(|options| options.on_cancel);
        handle.on_did_close().once().listen(move |_| {
            // Cleanup when notification gets disposed
            to_dispose.dispose();

            // Indicate cancellation to the outside if no action was executed
            if let Some(on_cancel) = &on_cancel
                && !choice_clicked.get()
            {
                on_cancel();
            }
        });

        handle
    }

    fn status(&self, message: NotificationMessage, options: Option<StatusMessageOptions>) -> Rc<dyn StatusHandle> {
        self.model.show_status_message(message, options)
    }
}
