//! Porte de `src/vs/workbench/browser/parts/notifications/notificationAccessibleView.ts`: o provedor de conteúdo acessível
//! da notificação em foco (`NotificationAccessibleView`) e as ações que a visão acessível oferece (`getActionsFromNotification`).
//!
//! Notas de tradução:
//! - **`implements IAccessibleViewImplementation`** é o trait de `accessible_view_registry`; o `getProvider` que devolve
//!   `AccessibleContentProvider | undefined` é [`NotificationAccessibleView::get_content_provider`] (o tipo concreto, que
//!   quem testa lê) e o do trait o envolve em [`AccessibleViewContentProvider::Content`]. `when` é a chave
//!   `NotificationFocusedContext`.
//! - **`list instanceof WorkbenchList`** é [`IListWidget::as_workbench_list`]; `indexOf`, `domFocus`, `setFocus`,
//!   `focusNext` e `focusPrevious` são os métodos de [`IWorkbenchList`]. O `try { list.setFocus(...) } catch { }` ignora o
//!   `Err`.
//! - **Variáveis capturadas pelos fechos** (`notification`, `notificationIndex`, `content`, `notificationListeners`,
//!   `resources`, `onDidChangeContent` e o próprio `provider`) são os campos de [`ProviderState`], compartilhado pelos fechos
//!   do provedor; os fechos que o estado alcança (os ouvintes da notificação) seguram só um `Weak` dele.
//! - **`notification.message.original.toString()`** é o texto da mensagem original: o texto literal, o `toString` do `Error`
//!   (`Nome: mensagem`) ou o do `NotificationText` (só os rótulos).
//! - **Objeto literal da ação "Clear Notification"** (`{ id, label, tooltip, run, enabled, class }`) é um `to_action` com as
//!   mesmas propriedades (a ação do `toAction` e o literal não se distinguem pelo contrato `IAction`).
//! - **Ícones.** `Codicon.bell` e `Codicon.link` são os ids `bell` e `link`; `gear` e `clear-all` vêm de
//!   `notifications_actions::codicon`.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, ToActionProps, to_action};
use je_base::dynamic::Dynamic;
use je_base::event::Emitter;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::linked_text::LinkedTextNode;
use je_base::nls::localize;
use je_base::promise::MaybePromise;
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_editor::code_action::platform::{AccessibilitySignal, I_ACCESSIBILITY_SIGNAL_SERVICE, IAccessibilitySignalService};
use je_instantiation::ServicesAccessor;
use je_platform::accessibility::accessible_view::{
    AccessibleContentProvider, AccessibleContentProviderArgs, AccessibleViewContentProvider, AccessibleViewProviderId, AccessibleViewType, I_ACCESSIBLE_VIEW_SERVICE,
    IAccessibleViewOptions, IAccessibleViewService,
};
use je_platform::accessibility::accessible_view_registry::IAccessibleViewImplementation;
use je_platform::commands::commands::{ICommandService, I_COMMAND_SERVICE};
use je_platform::contextkey::ContextKeyExpression;
use je_platform::list::list_service::{I_LIST_SERVICE, IListService, IWorkbenchList};
use je_platform::notification::{NotificationMessage, with_severity_prefix};
use je_platform::opener::opener::{AllowCommands, I_OPENER_SERVICE, IOpenerService, OpenOptions, OpenResource};
use je_platform::telemetry::{I_TELEMETRY_SERVICE, ITelemetryService};

use crate::browser::parts::notifications::notifications_actions::codicon;
use crate::browser::parts::notifications::notifications_commands::{SHOW_NOTIFICATIONS_CENTER, get_notification_from_context};
use crate::common::contextkeys::notification_focused_context;
use crate::common::notification_telemetry::{
    NotificationActionRole, NotificationInteraction, NotificationTelemetrySurface, log_notification_action, log_notification_interaction, log_notification_shown,
};
use crate::common::notifications::{NotificationViewItem, NotificationViewItemContentChangeKind};

/// `Codicon.bell.id` e `Codicon.link.id`.
const CODICON_BELL: &str = "bell";
const CODICON_LINK: &str = "link";

/// `NotificationAccessibleView`.
pub struct NotificationAccessibleView;

impl NotificationAccessibleView {
    pub const PRIORITY: i32 = 90;
    pub const NAME: &'static str = "notifications";

    /// `getProvider(accessor)`.
    pub fn get_content_provider(&self, accessor: &ServicesAccessor) -> Option<Rc<AccessibleContentProvider>> {
        let services = ProviderServices {
            accessible_view_service: accessor.get(I_ACCESSIBLE_VIEW_SERVICE),
            list_service: accessor.get(I_LIST_SERVICE),
            command_service: accessor.get(I_COMMAND_SERVICE),
            accessibility_signal_service: accessor.get(I_ACCESSIBILITY_SIGNAL_SERVICE),
            opener_service: accessor.get(I_OPENER_SERVICE),
            telemetry_service: accessor.get(I_TELEMETRY_SERVICE),
        };
        get_provider(services)
    }
}

impl IAccessibleViewImplementation for NotificationAccessibleView {
    fn view_type(&self) -> AccessibleViewType {
        AccessibleViewType::View
    }

    fn priority(&self) -> i32 {
        Self::PRIORITY
    }

    fn name(&self) -> &str {
        Self::NAME
    }

    fn get_provider(&self, accessor: &ServicesAccessor) -> Option<AccessibleViewContentProvider> {
        self.get_content_provider(accessor).map(AccessibleViewContentProvider::Content)
    }

    fn when(&self) -> Option<ContextKeyExpression> {
        Some(notification_focused_context().expr())
    }
}

/// Os serviços que `getProvider` pede ao `accessor`.
#[derive(Clone)]
struct ProviderServices {
    accessible_view_service: Rc<dyn IAccessibleViewService>,
    list_service: Rc<dyn IListService>,
    command_service: Rc<dyn ICommandService>,
    accessibility_signal_service: Rc<dyn IAccessibilitySignalService>,
    opener_service: Rc<dyn IOpenerService>,
    telemetry_service: Rc<dyn ITelemetryService>,
}

/// O que os fechos de `getProvider` compartilham (ver as notas do módulo).
struct ProviderState {
    services: ProviderServices,
    list: Rc<dyn IWorkbenchList>,
    notification: RefCell<Rc<NotificationViewItem>>,
    notification_index: Cell<isize>,
    content: RefCell<String>,
    resources: Rc<DisposableStore>,
    on_did_change_content: Rc<Emitter<()>>,
    notification_listeners: RefCell<Option<Rc<DisposableStore>>>,
    provider: RefCell<Weak<AccessibleContentProvider>>,
}

/// `notification.message.original.toString()`.
fn original_message_text(message: &NotificationMessage) -> String {
    match message {
        NotificationMessage::Text(text) => text.clone(),
        NotificationMessage::Error(error) => error.to_string(),
        NotificationMessage::ErrorWithActions(error) => error.error.to_string(),
        NotificationMessage::Rich(text) => text.to_string(),
    }
}

/// `getContentForNotification`.
fn get_content_for_notification(notification: &NotificationViewItem) -> String {
    let message = original_message_text(&notification.message().original);
    match notification.source().filter(|source| !source.is_empty()) {
        Some(source) => with_severity_prefix(&localize("notification.accessibleViewSrc", "{0} Source: {1}", &[&message, &source]), notification.severity()),
        None => with_severity_prefix(&message, notification.severity()),
    }
}

fn get_provider(services: ProviderServices) -> Option<Rc<AccessibleContentProvider>> {
    let initial_notification = get_notification_from_context(&*services.list_service, None)?;
    let notification = initial_notification;
    let _ = services.command_service.execute_command(SHOW_NOTIFICATIONS_CENTER, Vec::new());
    let list = services.list_service.last_focused_list().and_then(|list| list.as_workbench_list())?;
    let notification_index = list.index_of(&(notification.clone() as Rc<dyn Any>));

    let content = get_content_for_notification(&notification);
    if content.is_empty() {
        return None;
    }
    let resources = DisposableStore::new();
    let on_did_change_content = resources.add_rc(Emitter::new());

    let state = Rc::new(ProviderState {
        services: services.clone(),
        list,
        notification: RefCell::new(notification.clone()),
        notification_index: Cell::new(notification_index),
        content: RefCell::new(content),
        resources,
        on_did_change_content: on_did_change_content.clone(),
        notification_listeners: RefCell::new(None),
        provider: RefCell::new(Weak::new()),
    });

    let mut args = AccessibleContentProviderArgs::new(
        AccessibleViewProviderId::Notification,
        IAccessibleViewOptions::new(AccessibleViewType::View),
        {
            let state = state.clone();
            Rc::new(move || state.content.borrow().clone())
        },
        {
            let state = state.clone();
            Rc::new(move || state.focus_list())
        },
        "accessibility.verbosity.notification",
    );
    args.on_open = Some({
        let state = state.clone();
        Rc::new(move || {
            let previous = state.notification_listeners.borrow().clone();
            if let Some(previous) = previous {
                previous.dispose();
            }
            let listeners = DisposableStore::new();
            *state.notification_listeners.borrow_mut() = Some(listeners.clone());
            state.update_content();
            state.listen_to_notification();
            let notification = state.notification.borrow().clone();
            log_notification_shown(&*state.services.telemetry_service, &notification, NotificationTelemetrySurface::AccessibleView);
            let listeners: DisposableRef = listeners;
            Some(listeners)
        })
    });
    args.actions = Some(get_actions_from_notification(&notification, &services));
    args.provide_next_content = Some({
        let state = state.clone();
        Rc::new(move || {
            state.focus_list();
            state.list.focus_next();
            state.update_notification()
        })
    });
    args.provide_previous_content = Some({
        let state = state.clone();
        Rc::new(move || {
            state.focus_list();
            state.list.focus_previous();
            state.update_notification()
        })
    });
    args.on_did_change_content = Some(on_did_change_content.event());

    let provider = AccessibleContentProvider::new(args);
    *state.provider.borrow_mut() = Rc::downgrade(&provider);
    {
        let state = state.clone();
        provider.set_on_dispose(move || {
            let listeners = state.notification_listeners.borrow().clone();
            if let Some(listeners) = listeners {
                listeners.dispose();
            }
            state.resources.dispose();
        });
    }
    Some(provider)
}

impl ProviderState {
    /// `focusList`.
    fn focus_list(&self) {
        let _ = self.services.command_service.execute_command(SHOW_NOTIFICATIONS_CENTER, Vec::new());
        self.list.dom_focus();
        let _ = self.list.set_focus(&[self.notification_index.get()]);
    }

    /// `updateContent`.
    fn update_content(&self) {
        let notification = self.notification.borrow().clone();
        *self.content.borrow_mut() = get_content_for_notification(&notification);
        let provider = self.provider.borrow().upgrade();
        if let Some(provider) = provider {
            provider.set_actions(Some(get_actions_from_notification(&notification, &self.services)));
        }
    }

    /// `listenToNotification`.
    fn listen_to_notification(self: &Rc<Self>) {
        let listeners = self.notification_listeners.borrow().clone();
        let Some(listeners) = listeners else {
            return;
        };
        if listeners.is_disposed() {
            return;
        }
        listeners.clear();
        let notification = self.notification.borrow().clone();
        let accessible_view_service = self.services.accessible_view_service.clone();
        listeners.add(notification.on_did_close().once().listen(move |_| accessible_view_service.next()));
        let state = Rc::downgrade(self);
        listeners.add(notification.on_did_change_content().listen(move |event| {
            if matches!(
                event.kind,
                NotificationViewItemContentChangeKind::Message | NotificationViewItemContentChangeKind::Actions | NotificationViewItemContentChangeKind::Severity
            ) && let Some(state) = state.upgrade()
            {
                state.update_content();
                state.on_did_change_content.fire(());
            }
        }));
    }

    /// `updateNotification`.
    fn update_notification(self: &Rc<Self>) -> Option<String> {
        let focused_notification = get_notification_from_context(&*self.services.list_service, None)?;
        *self.notification.borrow_mut() = focused_notification.clone();
        self.notification_index.set(self.list.index_of(&(focused_notification.clone() as Rc<dyn Any>)));
        self.update_content();
        self.listen_to_notification();
        log_notification_shown(&*self.services.telemetry_service, &focused_notification, NotificationTelemetrySurface::AccessibleView);
        Some(self.content.borrow().clone())
    }
}

/// `getActionsFromNotification`.
fn get_actions_from_notification(notification: &Rc<NotificationViewItem>, services: &ProviderServices) -> Vec<ActionRef> {
    let notification_actions = notification.actions();
    let mut actions: Vec<ActionRef> = notification_actions
        .primary
        .iter()
        .chain(notification_actions.secondary.iter())
        .map(|action| {
            let (original, notification, telemetry_service) = (action.clone(), notification.clone(), services.telemetry_service.clone());
            to_action(ToActionProps {
                id: action.id(),
                label: action.label(),
                tooltip: Some(action.tooltip()),
                enabled: Some(action.enabled()),
                checked: action.checked(),
                class: Some(ThemeIcon::from_id(CODICON_BELL).as_class_name()),
                alt: None,
                run: Rc::new(move |_| {
                    log_notification_action(&*telemetry_service, &notification, &original, Some(NotificationTelemetrySurface::AccessibleView));
                    let result = original.run(&[]);
                    notification.close();
                    MaybePromise::Promise(result)
                }),
            })
        })
        .collect();
    if let Some(manage_extension) = actions.iter().find(|action| action.label().contains("Manage Extension")) {
        manage_extension.set_class(Some(ThemeIcon::from_id(codicon::GEAR).as_class_name()));
    }
    let message = notification.message();
    let nodes: Vec<LinkedTextNode> = match &message.original {
        NotificationMessage::Rich(text) => text.nodes().to_vec(),
        _ => message.linked_text.nodes.clone(),
    };
    for (index, node) in nodes.iter().enumerate() {
        let LinkedTextNode::Link(link) = node else {
            continue;
        };
        let href = link.href.clone();
        let tooltip = link.title.as_ref().filter(|title| !title.is_empty()).unwrap_or(&link.href).to_string_lossy();
        let (notification, telemetry_service, opener_service) = (notification.clone(), services.telemetry_service.clone(), services.opener_service.clone());
        actions.push(to_action(ToActionProps {
            id: format!("notification.link.{index}"),
            label: link.label.to_string_lossy(),
            tooltip: Some(tooltip),
            enabled: None,
            checked: None,
            class: Some(ThemeIcon::from_id(CODICON_LINK).as_class_name()),
            alt: None,
            run: Rc::new(move |_| {
                log_notification_interaction(
                    &*telemetry_service,
                    &notification,
                    NotificationInteraction::Link,
                    Some(NotificationTelemetrySurface::AccessibleView),
                    None,
                    NotificationActionRole::None,
                );
                let options = OpenOptions { allow_commands: Some(AllowCommands::Bool(true)), ..OpenOptions::default() };
                MaybePromise::Promise(opener_service.open(&OpenResource::Uri(URI::parse(href.clone(), false)), Some(&options)).then(Dynamic::Bool))
            }),
        }));
    }
    let clear_label = localize("clearNotification", "Clear Notification", &[]);
    let (notification, telemetry_service, accessibility_signal_service) = (notification.clone(), services.telemetry_service.clone(), services.accessibility_signal_service.clone());
    actions.push(to_action(ToActionProps {
        id: "clearNotification".to_owned(),
        label: clear_label.clone(),
        tooltip: Some(clear_label),
        enabled: Some(true),
        checked: None,
        class: Some(ThemeIcon::from_id(codicon::CLEAR_ALL).as_class_name()),
        alt: None,
        run: Rc::new(move |_| {
            log_notification_interaction(
                &*telemetry_service,
                &notification,
                NotificationInteraction::Dismiss,
                Some(NotificationTelemetrySurface::AccessibleView),
                None,
                NotificationActionRole::None,
            );
            notification.close();
            accessibility_signal_service.play_signal(AccessibilitySignal::Clear);
            MaybePromise::Value(Dynamic::Undefined)
        }),
    }));
    actions
}
