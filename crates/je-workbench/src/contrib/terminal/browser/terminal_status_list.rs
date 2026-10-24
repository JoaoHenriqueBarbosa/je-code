//! Porte de `src/vs/workbench/contrib/terminal/browser/terminalStatusList.ts`: a lista de status de um terminal (sino,
//! desconectado, relançamento necessário, informação de integração com o shell...), o status principal e as animações.
//!
//! Divergências:
//! - **Identidade do status.** O original compara os status por referência de objeto (`add` não faz nada se o mesmo objeto já
//!   está lá). Aqui o status é um `Rc<ITerminalStatus>` e a identidade é `Rc::ptr_eq`.
//! - **`remove(status | id)`** são [`TerminalStatusList::remove`] (o status) e [`TerminalStatusList::remove_id`] (o id).
//! - **`mainWindow.setTimeout`** é o `runtime::set_timeout`.
//! - **`TerminalStatus`** (o `const enum` de string) é um enum com `as_str()`.
//! - O `Disposable` do original é o campo `base`; descartá-lo descarta os três emissores.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::dynamic::Truthy;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::runtime::{self, TimerId};
use je_base::severity::Severity;
use je_base::themables::ThemeIcon;
use je_instantiation::Injectable;
use je_platform::configuration::service::IConfigurationService;
use je_platform::terminal::terminal_settings::TerminalSettingId;

use crate::contrib::terminal::common::terminal::ITerminalStatus;

/// `TerminalStatus`: the set of _internal_ terminal statuses, other components building on the terminal should put their
/// statuses within their component.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TerminalStatus {
    Bell,
    Disconnected,
    RelaunchNeeded,
    EnvironmentVariableInfoChangesActive,
    ShellIntegrationInfo,
    ShellIntegrationAttentionNeeded,
}

impl TerminalStatus {
    /// O valor do `const enum` do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bell => "bell",
            Self::Disconnected => "disconnected",
            Self::RelaunchNeeded => "relaunch-needed",
            Self::EnvironmentVariableInfoChangesActive => "env-var-info-changes-active",
            Self::ShellIntegrationInfo => "shell-integration-info",
            Self::ShellIntegrationAttentionNeeded => "shell-integration-attention-needed",
        }
    }
}

/// `ITerminalStatusList`.
pub trait ITerminalStatusList {
    /// Gets the most recent, highest severity status.
    fn primary(&self) -> Option<Rc<ITerminalStatus>>;
    /// Gets all active statues.
    fn statuses(&self) -> Vec<Rc<ITerminalStatus>>;

    fn on_did_add_status(&self) -> Event<Rc<ITerminalStatus>>;
    fn on_did_remove_status(&self) -> Event<Rc<ITerminalStatus>>;
    fn on_did_change_primary_status(&self) -> Event<Option<Rc<ITerminalStatus>>>;

    /// Adds a status to the list. Ideally a single status object that does not change will be shared as this call will
    /// no-op if the status is already set (checked by object reference). When `duration` (in milliseconds) is specified
    /// the status will remove itself when the duration elapses unless the status gets re-added.
    fn add(&self, status: Rc<ITerminalStatus>, duration: Option<u64>);
    fn remove(&self, status: &Rc<ITerminalStatus>);
    fn remove_id(&self, status_id: &str);
    fn toggle(&self, status: Rc<ITerminalStatus>, value: bool);
}

/// Se os dois são o mesmo objeto (`===`), ou os dois são ausentes.
fn is_same_status(left: &Option<Rc<ITerminalStatus>>, right: &Option<Rc<ITerminalStatus>>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => Rc::ptr_eq(left, right),
        _ => false,
    }
}

/// `TerminalStatusList`.
pub struct TerminalStatusList {
    base: Rc<Disposable>,
    this: Weak<Self>,
    statuses: RefCell<IndexMap<String, Rc<ITerminalStatus>>>,
    status_timeouts: RefCell<HashMap<String, TimerId>>,
    on_did_add_status: Rc<Emitter<Rc<ITerminalStatus>>>,
    on_did_remove_status: Rc<Emitter<Rc<ITerminalStatus>>>,
    on_did_change_primary_status: Rc<Emitter<Option<Rc<ITerminalStatus>>>>,

    configuration_service: Rc<dyn IConfigurationService>,
}

impl TerminalStatusList {
    fn apply_animation_setting(&self, status: Rc<ITerminalStatus>) -> Rc<ITerminalStatus> {
        let Some(icon) = status.icon.as_ref() else {
            return status;
        };
        if ThemeIcon::get_modifier(icon) != Some("spin")
            || self.configuration_service.get_value(Some(TerminalSettingId::TabsEnableAnimation.as_str()), None).is_truthy()
        {
            return status;
        }
        // Loading without animation is just a curved line that doesn't mean anything (`spinningLoading.id`)
        let new_icon = if icon.id == "loading~spin" { ThemeIcon::from_id("play") } else { ThemeIcon::modify(icon, None) };
        // Clone the status when changing the icon so that setting changes are applied without a reload being needed
        Rc::new(ITerminalStatus { icon: Some(new_icon), ..(*status).clone() })
    }

    fn remove_status(&self, status: Option<Rc<ITerminalStatus>>) {
        // Verify the status is the same as the one passed in
        let Some(status) = status else { return };
        if !self.statuses.borrow().contains_key(&status.id) {
            return;
        }
        let was_primary = self.primary().is_some_and(|primary| primary.id == status.id);
        self.statuses.borrow_mut().shift_remove(&status.id);
        self.on_did_remove_status.fire(status);
        if was_primary {
            self.on_did_change_primary_status.fire(self.primary());
        }
    }
}

impl ITerminalStatusList for TerminalStatusList {
    fn primary(&self) -> Option<Rc<ITerminalStatus>> {
        let mut result: Option<Rc<ITerminalStatus>> = None;
        for status in self.statuses.borrow().values() {
            if result.as_ref().is_none_or(|result| (status.severity as i32) >= (result.severity as i32))
                && (status.icon.is_some() || !result.as_ref().is_some_and(|result| result.icon.is_some()))
            {
                result = Some(status.clone());
            }
        }
        result
    }

    fn statuses(&self) -> Vec<Rc<ITerminalStatus>> {
        self.statuses.borrow().values().cloned().collect()
    }

    fn on_did_add_status(&self) -> Event<Rc<ITerminalStatus>> {
        self.on_did_add_status.event()
    }

    fn on_did_remove_status(&self) -> Event<Rc<ITerminalStatus>> {
        self.on_did_remove_status.event()
    }

    fn on_did_change_primary_status(&self) -> Event<Option<Rc<ITerminalStatus>>> {
        self.on_did_change_primary_status.event()
    }

    fn add(&self, status: Rc<ITerminalStatus>, duration: Option<u64>) {
        let status = self.apply_animation_setting(status);
        let out_timeout = self.status_timeouts.borrow_mut().remove(&status.id);
        if let Some(out_timeout) = out_timeout {
            runtime::clear_timeout(out_timeout);
        }
        if let Some(duration) = duration.filter(|duration| *duration > 0) {
            let weak = self.this.clone();
            let timed_status = status.clone();
            let timeout = runtime::set_timeout(
                move || {
                    if let Some(this) = weak.upgrade() {
                        this.remove(&timed_status);
                    }
                },
                duration,
            );
            self.status_timeouts.borrow_mut().insert(status.id.clone(), timeout);
        }
        let existing_status = self.statuses.borrow().get(&status.id).cloned();
        if let Some(existing_status) = existing_status
            && !Rc::ptr_eq(&existing_status, &status)
        {
            self.on_did_remove_status.fire(existing_status.clone());
            self.statuses.borrow_mut().shift_remove(&existing_status.id);
        }
        if !self.statuses.borrow().contains_key(&status.id) {
            let old_primary = self.primary();
            self.statuses.borrow_mut().insert(status.id.clone(), status.clone());
            self.on_did_add_status.fire(status);
            let new_primary = self.primary();
            if !is_same_status(&old_primary, &new_primary) {
                self.on_did_change_primary_status.fire(new_primary);
            }
        }
    }

    fn remove(&self, status: &Rc<ITerminalStatus>) {
        self.remove_status(Some(status.clone()));
    }

    fn remove_id(&self, status_id: &str) {
        let status = self.statuses.borrow().get(status_id).cloned();
        self.remove_status(status);
    }

    fn toggle(&self, status: Rc<ITerminalStatus>, value: bool) {
        if value {
            self.add(status, None);
        } else {
            self.remove(&status);
        }
    }
}

impl Injectable for TerminalStatusList {
    type Args = ();
    type Deps = (Rc<dyn IConfigurationService>,);

    fn create(_: (), (configuration_service,): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_add_status = Emitter::new();
        base.register(on_did_add_status.clone());
        let on_did_remove_status = Emitter::new();
        base.register(on_did_remove_status.clone());
        let on_did_change_primary_status = Emitter::new();
        base.register(on_did_change_primary_status.clone());
        Rc::new_cyclic(|this| Self {
            base,
            this: this.clone(),
            statuses: RefCell::new(IndexMap::new()),
            status_timeouts: RefCell::new(HashMap::new()),
            on_did_add_status,
            on_did_remove_status,
            on_did_change_primary_status,
            configuration_service,
        })
    }
}

impl IDisposable for TerminalStatusList {
    fn dispose(&self) {
        // Os temporizadores pendentes do original continuam armados (o `Disposable` não os limpa); `remove` ignora o que já saiu.
        self.base.dispose();
    }
}

/// `getColorForSeverity`: o id da cor do texto de um status.
pub fn get_color_for_severity(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "list.errorForeground",
        Severity::Warning => "list.warningForeground",
        _ => "",
    }
}
