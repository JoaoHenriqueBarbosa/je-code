//! Porte de `src/vs/workbench/services/configuration/common/experimentalSettings.ts`: o serviço que acompanha as
//! atribuições reais de experimento (ExP) por configuração, inclusive as que valem o mesmo que o padrão e por isso
//! não têm substituição de valor padrão.
//!
//! Divergências:
//! - **Injeção.** `IExperimentalSettingsService` é o trait [`IExperimentalSettingsService`] com o identificador
//!   [`I_EXPERIMENTAL_SETTINGS_SERVICE`]. O `registerSingleton(..., InstantiationType.Delayed)` do fim do arquivo não
//!   é portado aqui (Rust não tem efeito colateral ao carregar o módulo): o serviço é criado por
//!   [`ExperimentalSettingsService::new`], e o registro no contêiner entra com a montagem do workbench.
//! - **`extends Disposable`** é um [`Disposable`] guardado no campo `base`, com o emissor e o agendador registrados.
//! - **`ReadonlySet<string>`** do evento é um `Rc<HashSet<String>>` (o conjunto é entregue a vários ouvintes sem cópia).
//! - **O agendador** do original (`RunOnceScheduler(..., 0)`) captura o conjunto pendente e o emissor, não o serviço,
//!   para não formar um ciclo de referências.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use je_base::async_::RunOnceScheduler;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_instantiation::service_id;

/// `IExperimentalSettingsService`: tracks real assignments, including default-equal values that have no experiment
/// default override.
pub trait IExperimentalSettingsService {
    /// Coalesces changes until the next event-loop turn, including treatments resolved across microtasks.
    fn on_did_change_assignments(&self) -> Event<Rc<HashSet<String>>>;

    /// Whether this window resolved a real ExP assignment for the setting, independent of its effective value.
    fn has_assignment(&self, setting: &str) -> bool;

    /// Published by the configuration experiment contribution, including its startup latching semantics.
    fn set_assignment(&self, setting: &str, assigned: bool);
}

service_id!(pub I_EXPERIMENTAL_SETTINGS_SERVICE: dyn IExperimentalSettingsService = "experimentalSettingsService");

/// `ExperimentalSettingsService`.
pub struct ExperimentalSettingsService {
    base: Rc<Disposable>,
    assignments: RefCell<HashSet<String>>,
    pending_changes: Rc<RefCell<HashSet<String>>>,
    on_did_change_assignments: Rc<Emitter<Rc<HashSet<String>>>>,
    change_scheduler: Rc<RunOnceScheduler>,
}

impl ExperimentalSettingsService {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let pending_changes: Rc<RefCell<HashSet<String>>> = Rc::new(RefCell::new(HashSet::new()));
        let on_did_change_assignments = Emitter::new();
        base.register(on_did_change_assignments.clone());
        let change_scheduler = RunOnceScheduler::new(
            {
                let pending_changes = pending_changes.clone();
                let on_did_change_assignments = on_did_change_assignments.clone();
                move || {
                    let changes = std::mem::take(&mut *pending_changes.borrow_mut());
                    on_did_change_assignments.fire(Rc::new(changes));
                }
            },
            0,
        );
        base.register(change_scheduler.clone());
        Rc::new(Self { base, assignments: RefCell::new(HashSet::new()), pending_changes, on_did_change_assignments, change_scheduler })
    }
}

impl IExperimentalSettingsService for ExperimentalSettingsService {
    fn on_did_change_assignments(&self) -> Event<Rc<HashSet<String>>> {
        self.on_did_change_assignments.event()
    }

    fn has_assignment(&self, setting: &str) -> bool {
        self.assignments.borrow().contains(setting)
    }

    fn set_assignment(&self, setting: &str, assigned: bool) {
        if self.assignments.borrow().contains(setting) == assigned {
            return;
        }
        if assigned {
            self.assignments.borrow_mut().insert(setting.to_owned());
        } else {
            self.assignments.borrow_mut().remove(setting);
        }
        self.pending_changes.borrow_mut().insert(setting.to_owned());
        if !self.change_scheduler.is_scheduled() {
            self.change_scheduler.schedule(None);
        }
    }
}

impl IDisposable for ExperimentalSettingsService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
