//! Porte de `src/vs/platform/userDataSync/common/userDataSyncEnablementService.ts`: quais recursos o Settings
//! Sync sincroniza e se a sincronização está ligada (chaves `sync.enable` e `sync.enable.<recurso>` do
//! armazenamento de aplicação).
//!
//! Divergências: `isWeb` é falso (o alvo é nativo), então o alvo de armazenamento é sempre `Machine`;
//! `environmentService.sync` é o [`SyncMode`] do trait de ambiente.

use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_storage::StorageValue;

use super::user_data_sync::{ALL_SYNC_RESOURCES, IUserDataSyncEnablementService, IUserDataSyncStoreManagementService, SyncResource, get_enablement_key};
use crate::environment::environment::{EnvironmentService, SyncMode};
use crate::storage::{StorageScope, StorageService, StorageTarget, StorageValueChangeEvent};

const ENABLEMENT_KEY: &str = "sync.enable";

/// `UserDataSyncEnablementService`.
pub struct UserDataSyncEnablementService {
    base: Rc<Disposable>,
    storage_service: Rc<dyn StorageService>,
    environment_service: Rc<dyn EnvironmentService>,
    user_data_sync_store_management_service: Rc<dyn IUserDataSyncStoreManagementService>,
    on_did_change_enablement: Rc<Emitter<bool>>,
    on_did_change_resource_enablement: Rc<Emitter<(SyncResource, bool)>>,
}

impl UserDataSyncEnablementService {
    pub fn new(
        storage_service: Rc<dyn StorageService>,
        environment_service: Rc<dyn EnvironmentService>,
        user_data_sync_store_management_service: Rc<dyn IUserDataSyncStoreManagementService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_enablement = Emitter::new();
        base.register(on_did_change_enablement.clone());
        let on_did_change_resource_enablement = Emitter::new();
        base.register(on_did_change_resource_enablement.clone());
        let store = DisposableStore::new();
        base.register(store.clone());

        let this = Rc::new(Self { base, storage_service, environment_service, user_data_sync_store_management_service, on_did_change_enablement, on_did_change_resource_enablement });
        let weak = Rc::downgrade(&this);
        let listener = this.storage_service.on_did_change_value(StorageScope::Application, None, &store).listen(move |event| {
            if let Some(this) = weak.upgrade() {
                this.on_did_storage_change(event);
            }
        });
        store.add(listener);
        this
    }

    fn on_did_storage_change(&self, storage_change_event: &StorageValueChangeEvent) {
        if ENABLEMENT_KEY == storage_change_event.key {
            self.on_did_change_enablement.fire(self.is_enabled());
            return;
        }

        if let Some(resource_key) = ALL_SYNC_RESOURCES.into_iter().find(|resource_key| get_enablement_key(*resource_key) == storage_change_event.key) {
            self.on_did_change_resource_enablement.fire((resource_key, self.is_resource_enabled(resource_key, None)));
        }
    }

    fn stored_resource_enablement(&self, resource: SyncResource) -> Option<bool> {
        self.storage_service.get_boolean(&get_enablement_key(resource), StorageScope::Application, None)
    }
}

impl IDisposable for UserDataSyncEnablementService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IUserDataSyncEnablementService for UserDataSyncEnablementService {
    fn on_did_change_enablement(&self) -> Event<bool> {
        self.on_did_change_enablement.event()
    }

    fn is_enabled(&self) -> bool {
        match self.environment_service.sync() {
            Some(SyncMode::On) => true,
            Some(SyncMode::Off) => false,
            None => self.storage_service.get_boolean(ENABLEMENT_KEY, StorageScope::Application, Some(false)).unwrap_or(false),
        }
    }

    fn can_toggle_enablement(&self) -> bool {
        self.user_data_sync_store_management_service.user_data_sync_store().is_some() && self.environment_service.sync().is_none()
    }

    fn set_enablement(&self, enabled: bool) {
        if enabled && !self.can_toggle_enablement() {
            return;
        }
        self.storage_service.store(ENABLEMENT_KEY, StorageValue::from(enabled), StorageScope::Application, StorageTarget::Machine);
    }

    fn on_did_change_resource_enablement(&self) -> Event<(SyncResource, bool)> {
        self.on_did_change_resource_enablement.event()
    }

    fn is_resource_enabled(&self, resource: SyncResource, default_value: Option<bool>) -> bool {
        let default_value = default_value.unwrap_or(resource != SyncResource::Prompts);
        self.stored_resource_enablement(resource).unwrap_or(default_value)
    }

    fn set_resource_enablement(&self, resource: SyncResource, enabled: bool) {
        if self.is_resource_enabled(resource, None) != enabled {
            self.storage_service.store(&get_enablement_key(resource), StorageValue::from(enabled), StorageScope::Application, StorageTarget::Machine);
        }
    }

    fn get_resource_sync_state_version(&self, _resource: SyncResource) -> Option<String> {
        None
    }

    fn is_resource_enablement_configured(&self, resource: SyncResource) -> bool {
        self.stored_resource_enablement(resource).is_some()
    }
}
