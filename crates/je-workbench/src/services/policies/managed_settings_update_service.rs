//! Porte de `src/vs/workbench/services/policies/browser/managedSettingsUpdateService.ts`: o `updateInfo` observável do
//! bloqueio por versão mínima do cliente, recomputado quando o erro de compatibilidade ou o estado do atualizador mudam.
//!
//! Divergências:
//! - `registerSingleton(IManagedSettingsUpdateService, ..., Delayed)` não tem equivalente (o registro de singletons não
//!   está portado): quem monta o workbench cria o serviço com [`ManagedSettingsUpdateService::new`] e o registra com
//!   [`I_MANAGED_SETTINGS_UPDATE_SERVICE`].
//! - O `equals` de `base/common/objects` (igualdade profunda) é o `PartialEq` de [`IManagedSettingsUpdateInfo`]. O estado do
//!   atualizador é um `Rc<State>` novo a cada leitura (um objeto novo do JS), então cada mudança de estado recomputa.
//! - O `this` de `observableFromEvent(this, ...)` e `derivedOpts({ owner: this })` só dá nome ao observável e não entra.

use std::rc::Rc;

use je_base::lifecycle::{Disposable, IDisposable};
use je_base::observable::StrictEquals;
use je_base::observable::base::{IObservable, Observable};
use je_base::observable::deps::EqualityComparer;
use je_base::observable::{DerivedOptions, derived_opts, observable_from_event};
use je_platform::product::IProductService;
use je_platform::update::update::{State, UpdateService};

use super::managed_settings_update::{IManagedSettingsUpdateInfo, IManagedSettingsUpdateService, get_managed_settings_update_info};
use crate::services::accounts::default_account::IDefaultAccountService;
use crate::services::accounts::managed_settings::IManagedSettingsCompatibilityError;

impl StrictEquals for IManagedSettingsCompatibilityError {
    fn strict_equals(&self, other: &Self) -> bool {
        self == other
    }
}

/// `ManagedSettingsUpdateService`.
pub struct ManagedSettingsUpdateService {
    base: Rc<Disposable>,
    update_info: IObservable<Option<IManagedSettingsUpdateInfo>>,
}

impl ManagedSettingsUpdateService {
    pub fn new(default_account_service: Rc<dyn IDefaultAccountService>, product_service: Rc<dyn IProductService>, update_service: Rc<dyn UpdateService>) -> Rc<Self> {
        let compatibility_error = {
            let default_account_service = default_account_service.clone();
            observable_from_event(default_account_service.on_did_change_managed_settings_compatibility_error(), move |_| {
                default_account_service.managed_settings_compatibility_error()
            })
        };
        let update_state = {
            let update_service = update_service.clone();
            observable_from_event(update_service.on_state_change(), move |_| Rc::new(update_service.state()))
        };

        let equals_fn: EqualityComparer<Option<IManagedSettingsUpdateInfo>> = Rc::new(|first, second| first == second);
        let update_info = derived_opts(DerivedOptions { equals_fn: Some(equals_fn), ..DerivedOptions::default() }, move |reader| {
            let error = compatibility_error.read(reader);
            error.map(|error| {
                let state: Rc<State> = update_state.read(reader);
                get_managed_settings_update_info(&error, &*product_service, &state)
            })
        });

        Rc::new(Self { base: Disposable::new(), update_info })
    }
}

impl IManagedSettingsUpdateService for ManagedSettingsUpdateService {
    fn update_info(&self) -> IObservable<Option<IManagedSettingsUpdateInfo>> {
        self.update_info.clone()
    }
}

impl IDisposable for ManagedSettingsUpdateService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
