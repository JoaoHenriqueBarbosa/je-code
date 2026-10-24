//! Porte de `src/vs/workbench/common/notificationTelemetry.ts`: a telemetria de exposição e de interação das
//! notificações do workbench.
//!
//! Notas de tradução:
//! - O `WeakMap<INotificationViewItem, INotificationTelemetryState>` é uma tabela por thread chaveada pela
//!   identidade do `Rc` do item, com um `Weak` para saber se a entrada ainda vale (o endereço de um item
//!   descartado pode ser reaproveitado). O estado é compartilhado (`Rc<RefCell<..>>`): a deduplicação põe o
//!   mesmo objeto de estado no item substituto.
//! - Os tipos `NotificationShownEvent` e `NotificationInteractionEvent` viram structs; os eventos seguem para
//!   `publicLog2` como [`Dynamic`] com as chaves e a ordem do original. As anotações de classificação
//!   (`NotificationShownClassification`, `NotificationInteractionClassification`) são só tipos do compilador
//!   de telemetria, sem forma em tempo de execução, e não são portadas.
//! - `surface: NotificationTelemetrySurface | 'unknown'` é `Option<NotificationTelemetrySurface>` (`None` é
//!   `'unknown'`); `actionId: NotificationActionTelemetryId | 'unknown'` é `&'static str`.
//! - O `candidate.menu` de `logNotificationAction` é o [`ChoiceAction::menu`] (a única ação com submenu de
//!   escolhas), alcançado por `as_any`.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, IAction};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::id_of;
use je_base::severity::Severity;
use je_base::stopwatch::StopWatch;
use je_platform::notification::notification_telemetry::{
    NotificationActionTelemetry, NotificationActionTelemetryId, NotificationTelemetryOrigin, NotificationTelemetrySource, get_notification_action_telemetry,
};
use je_platform::telemetry::ITelemetryService;

use super::notifications::{ChoiceAction, NotificationViewItem};

/// `NotificationTelemetrySurface`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NotificationTelemetrySurface {
    Toast,
    Center,
    AccessibleView,
}

impl NotificationTelemetrySurface {
    /// O valor de texto do original.
    pub fn as_str(self) -> &'static str {
        match self {
            NotificationTelemetrySurface::Toast => "toast",
            NotificationTelemetrySurface::Center => "center",
            NotificationTelemetrySurface::AccessibleView => "accessibleView",
        }
    }
}

/// `NotificationInteraction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationInteraction {
    PrimaryAction,
    SecondaryAction,
    ProgressCancel,
    Dismiss,
    ClearAll,
    Expand,
    Collapse,
    Copy,
    Configure,
    Link,
}

impl NotificationInteraction {
    /// O valor de texto do original.
    pub fn as_str(self) -> &'static str {
        match self {
            NotificationInteraction::PrimaryAction => "primaryAction",
            NotificationInteraction::SecondaryAction => "secondaryAction",
            NotificationInteraction::ProgressCancel => "progressCancel",
            NotificationInteraction::Dismiss => "dismiss",
            NotificationInteraction::ClearAll => "clearAll",
            NotificationInteraction::Expand => "expand",
            NotificationInteraction::Collapse => "collapse",
            NotificationInteraction::Copy => "copy",
            NotificationInteraction::Configure => "configure",
            NotificationInteraction::Link => "link",
        }
    }
}

/// `actionRole: 'primary' | 'secondary' | 'none'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NotificationActionRole {
    Primary,
    Secondary,
    #[default]
    None,
}

impl NotificationActionRole {
    /// O valor de texto do original.
    pub fn as_str(self) -> &'static str {
        match self {
            NotificationActionRole::Primary => "primary",
            NotificationActionRole::Secondary => "secondary",
            NotificationActionRole::None => "none",
        }
    }
}

/// `NotificationShownEvent`: a origem (`INotificationTelemetrySource`) mais o resto do evento.
#[derive(Clone, Debug, PartialEq)]
pub struct NotificationShownEvent {
    pub source: NotificationTelemetrySource,
    pub instance_id: u64,
    /// `None` é o `'unknown'` do original.
    pub surface: Option<NotificationTelemetrySurface>,
    /// `'info' | 'warning' | 'error' | 'unknown'`.
    pub severity: &'static str,
    pub has_progress: bool,
    pub cancellable: bool,
}

impl NotificationShownEvent {
    /// As chaves do evento, na ordem do original.
    fn entries(&self) -> Vec<(&'static str, Dynamic)> {
        vec![
            ("origin", Dynamic::from(self.source.origin.as_str())),
            ("notificationId", Dynamic::from(self.source.notification_id.as_str())),
            ("extensionId", Dynamic::from(self.source.extension_id.as_str())),
            ("instanceId", Dynamic::from(self.instance_id as f64)),
            ("surface", Dynamic::from(self.surface.map_or("unknown", NotificationTelemetrySurface::as_str))),
            ("severity", Dynamic::from(self.severity)),
            ("hasProgress", Dynamic::from(self.has_progress)),
            ("cancellable", Dynamic::from(self.cancellable)),
        ]
    }

    pub fn to_dynamic(&self) -> Dynamic {
        Dynamic::object(self.entries())
    }
}

/// `NotificationInteractionEvent`: o evento de exposição mais os campos da interação.
#[derive(Clone, Debug, PartialEq)]
pub struct NotificationInteractionEvent {
    pub shown: NotificationShownEvent,
    pub interaction: NotificationInteraction,
    /// `NotificationActionTelemetryId | 'unknown'`.
    pub action_id: &'static str,
    pub action_role: NotificationActionRole,
    pub extension_button_index: i64,
    pub time_since_shown_ms: f64,
}

impl NotificationInteractionEvent {
    pub fn to_dynamic(&self) -> Dynamic {
        let mut entries = self.shown.entries();
        entries.push(("interaction", Dynamic::from(self.interaction.as_str())));
        entries.push(("actionId", Dynamic::from(self.action_id)));
        entries.push(("actionRole", Dynamic::from(self.action_role.as_str())));
        entries.push(("extensionButtonIndex", Dynamic::from(self.extension_button_index as f64)));
        entries.push(("timeSinceShownMs", Dynamic::from(self.time_since_shown_ms)));
        Dynamic::object(entries)
    }
}

/// `INotificationTelemetryState`.
struct NotificationTelemetryState {
    instance_id: u64,
    shown_surfaces: HashSet<NotificationTelemetrySurface>,
    first_shown: Option<StopWatch>,
    last_surface: Option<NotificationTelemetrySurface>,
}

type SharedState = Rc<RefCell<NotificationTelemetryState>>;

thread_local! {
    static INSTANCE_ID: Cell<u64> = const { Cell::new(0) };
    static STATES: RefCell<HashMap<usize, (Weak<NotificationViewItem>, SharedState)>> = RefCell::new(HashMap::new());
}

fn state_key(item: &Rc<NotificationViewItem>) -> usize {
    id_of(item)
}

fn get_state(item: &Rc<NotificationViewItem>) -> SharedState {
    STATES.with(|states| {
        let mut states = states.borrow_mut();
        let key = state_key(item);
        let live = states.get(&key).filter(|(owner, _)| owner.upgrade().is_some_and(|owner| Rc::ptr_eq(&owner, item))).map(|(_, state)| state.clone());
        live.unwrap_or_else(|| {
            let instance_id = INSTANCE_ID.with(|id| {
                id.set(id.get() + 1);
                id.get()
            });
            let state = Rc::new(RefCell::new(NotificationTelemetryState { instance_id, shown_surfaces: HashSet::new(), first_shown: None, last_surface: None }));
            states.retain(|_, (owner, _)| owner.strong_count() > 0);
            states.insert(key, (Rc::downgrade(item), state.clone()));
            state
        })
    })
}

/// Deduplication can replace a view item without presenting a new logical notification.
pub fn inherit_notification_telemetry(duplicate: &Rc<NotificationViewItem>, replacement: &Rc<NotificationViewItem>) {
    if duplicate.telemetry == replacement.telemetry {
        let state = get_state(duplicate);
        STATES.with(|states| {
            let mut states = states.borrow_mut();
            states.retain(|_, (owner, _)| owner.strong_count() > 0);
            states.insert(state_key(replacement), (Rc::downgrade(replacement), state));
        });
    }
}

fn is_progress_cancel(action: &ActionRef) -> bool {
    get_notification_action_telemetry(action) == Some(NotificationActionTelemetry::Id(NotificationActionTelemetryId::ProgressCancel))
}

fn get_event(item: &Rc<NotificationViewItem>, surface: Option<NotificationTelemetrySurface>) -> NotificationShownEvent {
    let state = get_state(item);
    let state = state.borrow();
    let has_active_progress = item.has_active_progress();
    NotificationShownEvent {
        source: item.telemetry.clone(),
        instance_id: state.instance_id,
        surface: surface.or(state.last_surface),
        severity: match item.severity() {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
            Severity::Ignore => "unknown",
        },
        has_progress: has_active_progress,
        cancellable: has_active_progress && item.actions().primary.iter().any(|action| action.enabled() && is_progress_cancel(action)),
    }
}

pub fn log_notification_shown(telemetry_service: &dyn ITelemetryService, item: &Rc<NotificationViewItem>, surface: NotificationTelemetrySurface) {
    let state = get_state(item);
    {
        let mut state = state.borrow_mut();
        state.last_surface = Some(surface);
        if !state.shown_surfaces.insert(surface) {
            return;
        }
        state.first_shown.get_or_insert_with(|| StopWatch::create(None));
    }
    telemetry_service.public_log2("notificationShown", Some(get_event(item, Some(surface)).to_dynamic()));
}

pub fn log_notification_interaction(
    telemetry_service: &dyn ITelemetryService,
    item: &Rc<NotificationViewItem>,
    interaction: NotificationInteraction,
    surface: Option<NotificationTelemetrySurface>,
    action: Option<&ActionRef>,
    action_role: NotificationActionRole,
) {
    if let Some(surface) = surface {
        get_state(item).borrow_mut().last_surface = Some(surface);
    }
    let metadata = action.and_then(get_notification_action_telemetry);
    let shown = get_event(item, surface);
    let event = NotificationInteractionEvent {
        shown,
        interaction,
        action_id: match metadata {
            Some(NotificationActionTelemetry::Id(id)) => id.as_str(),
            _ => "unknown",
        },
        action_role,
        extension_button_index: match metadata {
            Some(NotificationActionTelemetry::ExtensionButtonIndex(index)) if item.telemetry.origin == NotificationTelemetryOrigin::Extension => index,
            _ => -1,
        },
        time_since_shown_ms: get_state(item).borrow().first_shown.map_or(-1.0, |first_shown| first_shown.elapsed()),
    };
    telemetry_service.public_log2("notificationInteraction", Some(event.to_dynamic()));
}

pub fn log_notification_action(telemetry_service: &dyn ITelemetryService, item: &Rc<NotificationViewItem>, action: &ActionRef, surface: Option<NotificationTelemetrySurface>) {
    if !action.enabled() {
        return;
    }
    if let Some(surface) = surface {
        // Toolbar actions can delegate to commands, which receive only the notification.
        get_state(item).borrow_mut().last_surface = Some(surface);
    }
    let actions = item.actions();
    let primary = actions.primary.iter().any(|candidate| {
        id_of(candidate) == id_of(action)
            || candidate
                .as_any()
                .downcast_ref::<ChoiceAction>()
                .and_then(ChoiceAction::menu)
                .is_some_and(|menu| menu.iter().any(|choice| id_of(choice) == id_of(action)))
    });
    let secondary = actions.secondary.iter().any(|candidate| id_of(candidate) == id_of(action));
    let metadata = get_notification_action_telemetry(action);
    if primary || secondary {
        let interaction = if is_progress_cancel(action) {
            NotificationInteraction::ProgressCancel
        } else if primary {
            NotificationInteraction::PrimaryAction
        } else {
            NotificationInteraction::SecondaryAction
        };
        let role = if primary { NotificationActionRole::Primary } else { NotificationActionRole::Secondary };
        log_notification_interaction(telemetry_service, item, interaction, surface, Some(action), role);
    } else if metadata == Some(NotificationActionTelemetry::Id(NotificationActionTelemetryId::Copy)) {
        log_notification_interaction(telemetry_service, item, NotificationInteraction::Copy, surface, Some(action), NotificationActionRole::None);
    } else if metadata == Some(NotificationActionTelemetry::Id(NotificationActionTelemetryId::Configure)) {
        log_notification_interaction(telemetry_service, item, NotificationInteraction::Configure, surface, Some(action), NotificationActionRole::None);
    }
}

pub fn log_notification_expansion(telemetry_service: &dyn ITelemetryService, item: &Rc<NotificationViewItem>, expanded: bool, surface: Option<NotificationTelemetrySurface>) {
    if item.can_collapse() && item.expanded() != expanded {
        let interaction = if expanded { NotificationInteraction::Expand } else { NotificationInteraction::Collapse };
        log_notification_interaction(telemetry_service, item, interaction, surface, None, NotificationActionRole::None);
    }
}
