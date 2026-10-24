//! Porte de `src/vs/workbench/contrib/testing/common/testProfileService.ts`: os perfis de execução (executar, depurar,
//! cobertura) que os controladores de teste publicam, com o padrão do usuário guardado no `IStorageService`.
//!
//! Divergências:
//! - **Perfil estendido.** `IExtendedTestRunProfile extends ITestRunProfile` é [`ExtendedTestRunProfile`] (o perfil mais
//!   `was_initially_default`). Os métodos do serviço devolvem [`ITestRunProfile`] (cópias, sem o campo extra); o
//!   original devolve o objeto estendido (e o array interno), que o chamador só lê.
//! - **`Object.assign(profile, update)`** é a aplicação de cada campo presente do [`PartialTestRunProfile`].
//! - **`DefaultsMap`** é `IndexMap<String, BTreeMap<u32, bool>>`: as chaves numéricas de um objeto JavaScript saem em
//!   ordem crescente. Sua forma JSON é o [`StoredJson`] dele.
//! - **`removeProfile(controllerId, profileId?)`:** `!profileId` é verdadeiro para `undefined` e para `0`, como no original.
//! - **`all()`** devolve [`ControllerProfiles`] (a visão `Readonly<{ controller, profiles }>`).
//! - **`controllerProfiles: Map`** guarda a ordem de inserção (`IndexMap`); remover uma chave preserva a ordem das demais.
//! - **`capabilityContextKeys`** devolve `(chave, valor)` com a chave em texto.
//! - **`a.label.localeCompare(b.label)`** é o `locale_compare` do `collator` do porte.
//! - **`TestProfileService extends Disposable`:** o `Disposable` fica no campo `base`. A leitura de
//!   `this.userDefaults.get({})` usa [`StoredValue::get_or`].

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::collator::locale_compare;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_instantiation::service_id;
use je_platform::contextkey::{IContextKey, IContextKeyService};
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use serde_json::Value;

use super::stored_value::{StoredJson, StoredValue, StoredValueOptions};
use super::test_id::TestId;
use super::test_service::IMainThreadTestController;
use super::test_types::{
    ITestItem, ITestRunProfile, InternalTestItem, PartialTestRunProfile, TEST_RUN_PROFILE_BITSET_LIST, TestRunProfileBitset, test_run_profile_bitset,
};
use super::testing_context_keys;

/// O `Readonly<{ controller, profiles }>` de `all()`.
#[derive(Clone)]
pub struct ControllerProfiles {
    pub controller: Rc<dyn IMainThreadTestController>,
    pub profiles: Vec<ITestRunProfile>,
}

pub trait ITestProfileService {
    /// Fired when any profile changes.
    fn on_did_change(&self) -> Event<()>;

    /// Publishes a new test profile.
    fn add_profile(&self, controller: Rc<dyn IMainThreadTestController>, profile: ITestRunProfile);

    /// Updates an existing test run profile
    fn update_profile(&self, controller_id: &str, profile_id: u32, update: PartialTestRunProfile);

    /// Removes a profile. If profileId is not given, all profiles
    /// for the given controller will be removed.
    fn remove_profile(&self, controller_id: &str, profile_id: Option<u32>);

    /// Gets capabilities for the given test, indicating whether
    /// there's any usable profiles available for those groups.
    /// Devolve um bitset para usar com [`test_run_profile_bitset`].
    fn capabilities_for_test(&self, test: &ITestItem) -> u32;

    /// Configures a test profile.
    fn configure(&self, controller_id: &str, profile_id: u32);

    /// Gets all registered controllers, grouping by controller.
    fn all(&self) -> Vec<ControllerProfiles>;

    /// Gets the default profiles to be run for a given run group.
    fn get_group_default_profiles(&self, group: TestRunProfileBitset, controller_id: Option<&str>) -> Vec<ITestRunProfile>;

    /// Sets the default profiles to be run for a given run group.
    fn set_group_default_profiles(&self, group: TestRunProfileBitset, profiles: &[ITestRunProfile]);

    /// Gets the profiles for a controller, in priority order.
    fn get_controller_profiles(&self, controller_id: &str) -> Vec<ITestRunProfile>;

    /// Gets the preferred profile, if any, to run the test.
    fn get_default_profile_for_test(&self, group: TestRunProfileBitset, test: &InternalTestItem) -> Option<ITestRunProfile>;
}

service_id!(pub I_TEST_PROFILE_SERVICE: dyn ITestProfileService = "testProfileService");

/// Gets whether the given profile can be used to run the test.
pub fn can_use_profile_with_test(profile: &ITestRunProfile, test: &InternalTestItem) -> bool {
    profile.controller_id == test.controller_id
        && (TestId::is_root_string(&test.item.ext_id)
            || profile.tag.as_deref().is_none_or(|tag| tag.is_empty() || test.item.tags.iter().any(|t| t == tag)))
}

fn sorter(a: &ExtendedTestRunProfile, b: &ExtendedTestRunProfile) -> Ordering {
    if a.profile.is_default != b.profile.is_default {
        return if a.profile.is_default { Ordering::Less } else { Ordering::Greater };
    }

    locale_compare(&a.profile.label, &b.profile.label)
}

/// `IExtendedTestRunProfile`.
#[derive(Clone, Debug)]
pub struct ExtendedTestRunProfile {
    pub profile: ITestRunProfile,
    pub was_initially_default: bool,
}

/// Given a capabilities bitset, returns a map of context keys representing
/// them.
pub fn capability_context_keys(capabilities: u32) -> Vec<(String, bool)> {
    vec![
        (testing_context_keys::has_runnable_tests().key().to_string(), (capabilities & test_run_profile_bitset::RUN) != 0),
        (testing_context_keys::has_debuggable_tests().key().to_string(), (capabilities & test_run_profile_bitset::DEBUG) != 0),
        (testing_context_keys::has_coverable_tests().key().to_string(), (capabilities & test_run_profile_bitset::COVERAGE) != 0),
    ]
}

/// `DefaultsMap`: `{ [controllerId: string]: { [profileId: number]: boolean } }`.
pub type DefaultsMap = IndexMap<String, BTreeMap<u32, bool>>;

impl StoredJson for DefaultsMap {
    fn to_json(&self) -> Value {
        let mut object = serde_json::Map::new();
        for (controller_id, profiles) in self {
            let mut inner = serde_json::Map::new();
            for (profile_id, is_default) in profiles {
                inner.insert(profile_id.to_string(), Value::Bool(*is_default));
            }
            object.insert(controller_id.clone(), Value::Object(inner));
        }
        Value::Object(object)
    }

    fn from_json(value: &Value) -> Option<Self> {
        let mut map = DefaultsMap::new();
        for (controller_id, profiles) in value.as_object()? {
            let mut inner = BTreeMap::new();
            for (profile_id, is_default) in profiles.as_object()? {
                inner.insert(profile_id.parse::<u32>().ok()?, is_default.as_bool()?);
            }
            map.insert(controller_id.clone(), inner);
        }
        Some(map)
    }
}

struct ControllerRecord {
    profiles: Vec<ExtendedTestRunProfile>,
    controller: Rc<dyn IMainThreadTestController>,
}

pub struct TestProfileService {
    base: Rc<Disposable>,
    user_defaults: Rc<StoredValue<DefaultsMap>>,
    capabilities_contexts: Vec<(TestRunProfileBitset, Rc<dyn IContextKey>)>,
    change_emitter: Rc<Emitter<()>>,
    controller_profiles: RefCell<IndexMap</* controller ID */ String, ControllerRecord>>,
}

impl TestProfileService {
    pub fn new(context_key_service: Rc<dyn IContextKeyService>, storage_service: Rc<dyn StorageService>) -> Rc<Self> {
        let base = Disposable::new();
        let change_emitter = Emitter::new();
        base.register(change_emitter.clone());

        storage_service.remove("testingPreferredProfiles", StorageScope::Workspace); // cleanup old format
        let user_defaults = StoredValue::<DefaultsMap>::new(
            StoredValueOptions { key: "testingPreferredProfiles2".to_string(), scope: StorageScope::Workspace, target: StorageTarget::Machine },
            storage_service,
        );
        base.register(user_defaults.clone());

        let capabilities_contexts = TEST_RUN_PROFILE_BITSET_LIST
            .iter()
            .map(|bitset| {
                let key = testing_context_keys::capability_to_context_key(*bitset).expect("todo bit da lista tem chave de contexto");
                (*bitset, key.bind_to(context_key_service.as_ref()))
            })
            .collect();

        let this = Rc::new(Self { base, user_defaults, capabilities_contexts, change_emitter, controller_profiles: RefCell::new(IndexMap::new()) });
        this.refresh_context_keys();
        this
    }

    fn refresh_context_keys(&self) {
        let mut all_capabilities: u32 = 0;
        for record in self.controller_profiles.borrow().values() {
            for extended in &record.profiles {
                let profile = &extended.profile;
                all_capabilities |= if all_capabilities & profile.group != 0 { test_run_profile_bitset::HAS_NON_DEFAULT_PROFILE } else { profile.group };
                all_capabilities |= if profile.supports_continuous_run { test_run_profile_bitset::SUPPORTS_CONTINUOUS_RUN } else { 0 };
            }
        }

        for (group, context) in &self.capabilities_contexts {
            context.set(Dynamic::Bool((all_capabilities & group) != 0));
        }
    }
}

impl ITestProfileService for TestProfileService {
    /// @inheritdoc
    fn on_did_change(&self) -> Event<()> {
        self.change_emitter.event()
    }

    /// @inheritdoc
    fn add_profile(&self, controller: Rc<dyn IMainThreadTestController>, profile: ITestRunProfile) {
        let previous_explicit_default_value = self.user_defaults.get().and_then(|defaults| defaults.get(&controller.id()).and_then(|profiles| profiles.get(&profile.profile_id).copied()));
        let extended = ExtendedTestRunProfile {
            profile: ITestRunProfile { is_default: previous_explicit_default_value.unwrap_or(profile.is_default), ..profile.clone() },
            was_initially_default: profile.is_default,
        };

        {
            let mut records = self.controller_profiles.borrow_mut();
            match records.get_mut(&profile.controller_id) {
                Some(record) => {
                    record.profiles.push(extended);
                    record.profiles.sort_by(sorter);
                }
                None => {
                    records.insert(profile.controller_id.clone(), ControllerRecord { profiles: vec![extended], controller });
                }
            }
        }

        self.refresh_context_keys();
        self.change_emitter.fire(());
    }

    /// @inheritdoc
    fn update_profile(&self, controller_id: &str, profile_id: u32, update: PartialTestRunProfile) {
        let mut next_defaults: Option<DefaultsMap> = None;
        {
            let mut records = self.controller_profiles.borrow_mut();
            let Some(ctrl) = records.get_mut(controller_id) else {
                return;
            };

            let Some(extended) = ctrl.profiles.iter_mut().find(|c| c.profile.controller_id == controller_id && c.profile.profile_id == profile_id) else {
                return;
            };

            let profile = &mut extended.profile;
            if let Some(controller_id) = &update.controller_id {
                profile.controller_id = controller_id.clone();
            }
            if let Some(profile_id) = update.profile_id {
                profile.profile_id = profile_id;
            }
            if let Some(label) = &update.label {
                profile.label = label.clone();
            }
            if let Some(group) = update.group {
                profile.group = group;
            }
            if let Some(is_default) = update.is_default {
                profile.is_default = is_default;
            }
            if let Some(tag) = &update.tag {
                profile.tag = tag.clone();
            }
            if let Some(has_configuration_handler) = update.has_configuration_handler {
                profile.has_configuration_handler = has_configuration_handler;
            }
            if let Some(supports_continuous_run) = update.supports_continuous_run {
                profile.supports_continuous_run = supports_continuous_run;
            }

            // store updates is isDefault as if the user changed it (which they might
            // have through some extension-contributed UI)
            if let Some(is_default) = update.is_default {
                let mut map = self.user_defaults.get_or(DefaultsMap::new());
                set_is_default(&mut map, extended, is_default);
                next_defaults = Some(map);
            }

            ctrl.profiles.sort_by(sorter);
        }

        if let Some(map) = next_defaults {
            self.user_defaults.store(map);
        }

        self.change_emitter.fire(());
    }

    /// @inheritdoc
    fn configure(&self, controller_id: &str, profile_id: u32) {
        let controller = self.controller_profiles.borrow().get(controller_id).map(|record| record.controller.clone());
        if let Some(controller) = controller {
            controller.configure_run_profile(profile_id);
        }
    }

    /// @inheritdoc
    fn remove_profile(&self, controller_id: &str, profile_id: Option<u32>) {
        {
            let mut records = self.controller_profiles.borrow_mut();
            if !records.contains_key(controller_id) {
                return;
            }

            if profile_id.is_none_or(|id| id == 0) {
                records.shift_remove(controller_id);
                drop(records);
                self.change_emitter.fire(());
                return;
            }

            let ctrl = records.get_mut(controller_id).expect("o controlador foi conferido acima");
            let Some(index) = ctrl.profiles.iter().position(|c| Some(c.profile.profile_id) == profile_id) else {
                return;
            };

            ctrl.profiles.remove(index);
        }

        self.refresh_context_keys();
        self.change_emitter.fire(());
    }

    /// @inheritdoc
    fn capabilities_for_test(&self, test: &ITestItem) -> u32 {
        let records = self.controller_profiles.borrow();
        let Some(ctrl) = records.get(TestId::root_string(&test.ext_id)) else {
            return 0;
        };

        let mut capabilities: u32 = 0;
        for extended in &ctrl.profiles {
            let profile = &extended.profile;
            if profile.tag.as_deref().is_none_or(|tag| tag.is_empty() || test.tags.iter().any(|t| t == tag)) {
                capabilities |= if capabilities & profile.group != 0 { test_run_profile_bitset::HAS_NON_DEFAULT_PROFILE } else { profile.group };
            }
        }

        capabilities
    }

    /// @inheritdoc
    fn all(&self) -> Vec<ControllerProfiles> {
        self.controller_profiles
            .borrow()
            .values()
            .map(|record| ControllerProfiles { controller: record.controller.clone(), profiles: record.profiles.iter().map(|p| p.profile.clone()).collect() })
            .collect()
    }

    /// @inheritdoc
    fn get_controller_profiles(&self, profile_id: &str) -> Vec<ITestRunProfile> {
        self.controller_profiles.borrow().get(profile_id).map(|record| record.profiles.iter().map(|p| p.profile.clone()).collect()).unwrap_or_default()
    }

    /// @inheritdoc
    fn get_group_default_profiles(&self, group: TestRunProfileBitset, controller_id: Option<&str>) -> Vec<ITestRunProfile> {
        let all_profiles: Vec<ITestRunProfile> = match controller_id.filter(|id| !id.is_empty()) {
            Some(controller_id) => self.get_controller_profiles(controller_id),
            None => self.controller_profiles.borrow().values().flat_map(|c| c.profiles.iter().map(|p| p.profile.clone())).collect(),
        };
        let mut defaults: Vec<ITestRunProfile> = all_profiles.iter().filter(|c| c.group == group && c.is_default).cloned().collect();

        // have *some* default profile to run if none are set otherwise
        if defaults.is_empty()
            && let Some(first) = all_profiles.iter().find(|p| p.group == group)
        {
            defaults.push(first.clone());
        }

        defaults
    }

    /// @inheritdoc
    fn set_group_default_profiles(&self, group: TestRunProfileBitset, profiles: &[ITestRunProfile]) {
        let mut next = DefaultsMap::new();
        {
            let mut records = self.controller_profiles.borrow_mut();
            for ctrl in records.values_mut() {
                next.insert(ctrl.controller.id(), BTreeMap::new());
                for profile in ctrl.profiles.iter_mut() {
                    if profile.profile.group != group {
                        continue;
                    }

                    let is_default = profiles.iter().any(|p| p.profile_id == profile.profile.profile_id);
                    set_is_default(&mut next, profile, is_default);
                }

                // When switching a profile, if the controller has a same-named profile in
                // other groups, update those to match the enablement state as well.
                for index in 0..ctrl.profiles.len() {
                    if ctrl.profiles[index].profile.group == group {
                        continue;
                    }
                    let label = ctrl.profiles[index].profile.label.clone();
                    let matching = ctrl.profiles.iter().find(|p| p.profile.group == group && p.profile.label == label).map(|m| m.profile.is_default);
                    if let Some(matching_is_default) = matching {
                        set_is_default(&mut next, &mut ctrl.profiles[index], matching_is_default);
                    }
                }

                ctrl.profiles.sort_by(sorter);
            }
        }

        self.user_defaults.store(next);
        self.change_emitter.fire(());
    }

    fn get_default_profile_for_test(&self, group: TestRunProfileBitset, test: &InternalTestItem) -> Option<ITestRunProfile> {
        self.get_controller_profiles(&test.controller_id).into_iter().find(|p| (p.group & group) != 0 && can_use_profile_with_test(p, test))
    }
}

impl IDisposable for TestProfileService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

fn set_is_default(map: &mut DefaultsMap, profile: &mut ExtendedTestRunProfile, is_default: bool) {
    profile.profile.is_default = is_default;
    let defaults = map.entry(profile.profile.controller_id.clone()).or_default();
    if profile.profile.is_default != profile.was_initially_default {
        defaults.insert(profile.profile.profile_id, profile.profile.is_default);
    } else {
        defaults.remove(&profile.profile.profile_id);
    }
}
