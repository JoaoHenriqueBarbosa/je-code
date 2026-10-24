//! Porte de `src/vs/workbench/contrib/testing/common/testingContinuousRunService.ts`: o serviço que mantém as execuções
//! contínuas de testes (globais, por perfil ou por teste) e as para em ordem de profundidade.
//!
//! Divergências:
//! - **`profiles: ITestRunProfile[] | TestRunProfileBitset`** é o enum [`ContinuousRunProfiles`].
//! - **Identidade de perfil.** O original usa o próprio objeto do perfil como chave (`DisposableMap<ITestRunProfile, ...>`,
//!   `Set` de perfis, `arrays.equals` com `===`). O `ITestRunProfile` do porte é um valor, então a chave do mapa é o par
//!   `(controllerId, profileId)`, que identifica o perfil no serviço de perfis, e a união e a comparação de listas usam a
//!   igualdade por valor ([`PartialEq`]), que coincide com a identidade enquanto o serviço de perfis devolve o mesmo perfil.
//! - **`ISettableObservable<ITestRunProfile[]>`** guarda um `Rc<Vec<_>>`: cada `set` de uma lista nova muda o valor (um
//!   array novo é sempre diferente do anterior no JavaScript), como o `StrictEquals` do `Rc` (ponteiro) faz.
//! - **`Set<number>` da última execução** é um `IndexSet`, que mantém a ordem de inserção como o `Set`; a serialização é a
//!   do original (`JSON.stringify([...v])` e `new Set(JSON.parse(v))`).
//! - **`@ITestService`, `@IStorageService`, `@IContextKeyService` e `@ITestProfileService`** são `Rc<dyn ...>`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use indexmap::IndexSet;
use je_base::cancellation::CancellationTokenSource;
use je_base::dynamic::Dynamic;
use je_base::errors::{Error, throw};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableMap, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::observable::reactions::autorun::autorun_iterable_delta_by;
use je_base::observable::{IReader, ISettableObservable, Observable, Settable, SettableExt, observable_value};
use je_base::prefix_tree::WellDefinedPrefixTree;
use je_instantiation::service_id;
use je_platform::contextkey::IContextKeyService;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};

use super::stored_value::{IStoredValueSerialization, StoredValue, StoredValueOptions};
use super::test_id::TestId;
use super::test_profile_service::ITestProfileService;
use super::test_service::ITestService;
use super::test_types::{ITestRunProfile, ResolvedTestRunRequest, TestRunProfileBitset, TestRunTarget};
use super::testing_context_keys::is_continuous_mode_on;

/// `ITestRunProfile[] | TestRunProfileBitset`.
#[derive(Clone, Debug)]
pub enum ContinuousRunProfiles {
    /// Uma lista específica de perfis.
    Profiles(Vec<ITestRunProfile>),
    /// Todos os perfis padrão de um grupo.
    Group(TestRunProfileBitset),
}

/// `ITestingContinuousRunService`.
pub trait ITestingContinuousRunService {
    /// `lastRunProfileIds`: gets a list of the last test profiles that were continuously run in the workspace.
    fn last_run_profile_ids(&self) -> IndexSet<u32>;

    /// `onDidChange`: fired when a test is added or removed from continuous run, or when enablement is changed globally.
    fn on_did_change(&self) -> Event<Option<String>>;

    /// `isSpecificallyEnabledFor(testId)`: gets whether continuous run is specifically enabled for the given test ID.
    fn is_specifically_enabled_for(&self, test_id: &str) -> bool;

    /// `isEnabledForAParentOf(testId)`: gets whether continuous run is specifically enabled for the given test ID, or any
    /// of its parents.
    fn is_enabled_for_a_parent_of(&self, test_id: &str) -> bool;

    /// `isEnabledForAChildOf(testId)`: gets whether continuous run is specifically enabled for the given test ID, or any
    /// of its children.
    fn is_enabled_for_a_child_of(&self, test_id: &str) -> bool;

    /// `isEnabledForProfile(profile)`: gets whether continuous run is turned on for the given profile.
    fn is_enabled_for_profile(&self, profile: &ITestRunProfile) -> bool;

    /// `isEnabled()`: gets whether it's enabled at all.
    fn is_enabled(&self) -> bool;

    /// `start(profile, testId?)`: starts a continuous auto run with a specific set of profiles, or all default profiles in
    /// a group. Globally if no test is given, for a specific test otherwise.
    fn start(&self, profiles: ContinuousRunProfiles, test_id: Option<&str>);

    /// `stopProfile(profile)`: stops a continuous run for the given test profile.
    fn stop_profile(&self, profile: &ITestRunProfile);

    /// `stop(testId?)`: stops any continuous run. Globally if no test is given, for a specific test otherwise.
    fn stop(&self, test_id: Option<&str>);
}

service_id!(pub I_TESTING_CONTINUOUS_RUN_SERVICE: dyn ITestingContinuousRunService = "testingContinuousRunService");

/// `RunningRef`.
struct RunningRef {
    path: Vec<String>,
    profiles: ISettableObservable<Rc<Vec<ITestRunProfile>>>,
    auto_set_default: Cell<bool>,
    handle: Rc<DisposableStore>,
}

/// A união sem repetição, na ordem em que aparecem (`[...new Set([...a, ...b])]`).
fn union(a: &[ITestRunProfile], b: &[ITestRunProfile]) -> Vec<ITestRunProfile> {
    let mut out: Vec<ITestRunProfile> = Vec::new();
    for profile in a.iter().chain(b) {
        if !out.contains(profile) {
            out.push(profile.clone());
        }
    }
    out
}

fn profile_key(profile: &ITestRunProfile) -> (String, u32) {
    (profile.controller_id.clone(), profile.profile_id)
}

/// `TestingContinuousRunService`.
pub struct TestingContinuousRunService {
    base: Rc<Disposable>,
    change_emitter: Rc<Emitter<Option<String>>>,
    running: Rc<RefCell<WellDefinedPrefixTree<Rc<RunningRef>>>>,
    last_run: Rc<StoredValue<IndexSet<u32>>>,
    test_service: Rc<dyn ITestService>,
    test_profile_service: Rc<dyn ITestProfileService>,
}

impl TestingContinuousRunService {
    /// `new TestingContinuousRunService(testService, storageService, contextKeyService, testProfileService)`.
    pub fn new(
        test_service: Rc<dyn ITestService>,
        storage_service: Rc<dyn StorageService>,
        context_key_service: Rc<dyn IContextKeyService>,
        test_profile_service: Rc<dyn ITestProfileService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let change_emitter = Emitter::new();
        base.register(change_emitter.clone());
        let running: Rc<RefCell<WellDefinedPrefixTree<Rc<RunningRef>>>> = Rc::default();

        let is_globally_on = is_continuous_mode_on().bind_to(context_key_service.as_ref());
        let running_for_key = running.clone();
        base.register(change_emitter.event().listen(move |_| {
            is_globally_on.set(Dynamic::Bool(running_for_key.borrow().root.value.is_some()));
        }));

        let last_run = StoredValue::with_serialization(
            StoredValueOptions { key: "lastContinuousRunProfileIds".to_owned(), scope: StorageScope::Workspace, target: StorageTarget::Machine },
            IStoredValueSerialization {
                deserialize: Rc::new(|value: &str| -> IndexSet<u32> {
                    match serde_json::from_str::<Vec<u32>>(value) {
                        Ok(ids) => ids.into_iter().collect(),
                        Err(error) => throw(Error::new(error.to_string())),
                    }
                }),
                serialize: Rc::new(|value: &IndexSet<u32>| serde_json::to_string(&value.iter().collect::<Vec<_>>()).expect("uma lista de números serializa")),
            },
            storage_service,
        );
        base.register(last_run.clone());

        let running_for_dispose = running.clone();
        base.register(to_disposable(move || {
            let handles: Vec<Rc<DisposableStore>> = running_for_dispose.borrow().values().map(|node| node.handle.clone()).collect();
            for handle in handles {
                handle.dispose();
            }
        }));

        Rc::new(Self { base, change_emitter, running, last_run, test_service, test_profile_service })
    }
}

impl IDisposable for TestingContinuousRunService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl ITestingContinuousRunService for TestingContinuousRunService {
    fn last_run_profile_ids(&self) -> IndexSet<u32> {
        self.last_run.get_or(IndexSet::new())
    }

    fn on_did_change(&self) -> Event<Option<String>> {
        self.change_emitter.event()
    }

    fn is_specifically_enabled_for(&self, test_id: &str) -> bool {
        let running = self.running.borrow();
        running.size() > 0 && running.has_key(TestId::from_string(test_id).path())
    }

    fn is_enabled_for_a_parent_of(&self, test_id: &str) -> bool {
        let running = self.running.borrow();
        running.root.value.is_some() || (running.size() > 0 && running.has_key_or_parent(TestId::from_string(test_id).path()))
    }

    fn is_enabled_for_profile(&self, profile: &ITestRunProfile) -> bool {
        self.running
            .borrow()
            .values()
            .any(|node| node.profiles.get().iter().any(|p| p.profile_id == profile.profile_id && p.controller_id == profile.controller_id))
    }

    fn is_enabled_for_a_child_of(&self, test_id: &str) -> bool {
        let running = self.running.borrow();
        running.root.value.is_some() || (running.size() > 0 && running.has_key_or_children(TestId::from_string(test_id).path()))
    }

    fn is_enabled(&self) -> bool {
        let running = self.running.borrow();
        running.root.value.is_some() || running.size() > 0
    }

    fn start(&self, profiles: ContinuousRunProfiles, test_id: Option<&str>) {
        let store = DisposableStore::new();
        let test_id: Option<String> = test_id.filter(|id| !id.is_empty()).map(str::to_owned);
        let path: Vec<String> = test_id.as_deref().map(|id| TestId::from_string(id).path().to_vec()).unwrap_or_default();

        let auto_set_default = matches!(profiles, ContinuousRunProfiles::Group(_));
        let actual_profiles: ISettableObservable<Rc<Vec<ITestRunProfile>>> = match &profiles {
            ContinuousRunProfiles::Profiles(list) => observable_value("crProfiles", Rc::new(list.clone())).to_settable(),
            ContinuousRunProfiles::Group(group) => {
                let group = *group;
                // restart the continuous run when default profiles change, if we were
                // asked to run for a group
                let (profile_service, filter_test_id) = (self.test_profile_service.clone(), test_id.clone());
                let get_relevant = Rc::new(move || -> Vec<ITestRunProfile> {
                    profile_service
                        .get_group_default_profiles(group, None)
                        .into_iter()
                        .filter(|p| p.supports_continuous_run && filter_test_id.as_deref().is_none_or(|id| TestId::root_string(id) == p.controller_id))
                        .collect()
                });
                observable_value("crProfiles", Rc::new(get_relevant())).to_settable()
            }
        };
        let ref_: Rc<RunningRef> = Rc::new(RunningRef { profiles: actual_profiles.clone(), handle: store.clone(), path: path.clone(), auto_set_default: Cell::new(auto_set_default) });

        if let ContinuousRunProfiles::Group(group) = &profiles {
            let group = *group;
            let (profile_service, filter_test_id) = (self.test_profile_service.clone(), test_id.clone());
            let get_relevant = move || -> Vec<ITestRunProfile> {
                profile_service
                    .get_group_default_profiles(group, None)
                    .into_iter()
                    .filter(|p| p.supports_continuous_run && filter_test_id.as_deref().is_none_or(|id| TestId::root_string(id) == p.controller_id))
                    .collect()
            };
            let (ref_for_change, profiles_for_change) = (ref_.clone(), actual_profiles.clone());
            store.add(self.test_profile_service.on_did_change().listen(move |_| {
                if ref_for_change.auto_set_default.get() {
                    let new_relevant = get_relevant();
                    if new_relevant != *profiles_for_change.get() {
                        profiles_for_change.set(Rc::new(get_relevant()), None);
                    }
                }
            }));
        }

        // If we're already running this specific test, then add the profile and turn
        // off the auto-addition of bitset-based profiles.
        let existing = self.running.borrow().find(path.iter()).cloned();
        if let Some(existing) = existing {
            store.dispose();
            ref_.auto_set_default.set(false);
            existing.auto_set_default.set(false);
            existing.profiles.set(Rc::new(union(&actual_profiles.get(), &existing.profiles.get())), None);
            self.change_emitter.fire(test_id);
            return;
        }

        self.running.borrow_mut().insert(path, ref_, None);

        let cancellation_stores: Rc<DisposableMap<(String, u32), CancellationTokenSource>> = DisposableMap::new();
        let stores_for_dispose = cancellation_stores.clone();
        store.add(to_disposable(move || {
            for cts in stores_for_dispose.values() {
                cts.cancel();
            }
            stores_for_dispose.dispose();
        }));

        let (test_service, last_run, stores_for_delta, test_id_for_run) = (self.test_service.clone(), self.last_run.clone(), cancellation_stores, test_id.clone());
        let observable = actual_profiles.clone();
        store.add(autorun_iterable_delta_by(
            move |reader: &dyn IReader| -> Vec<ITestRunProfile> { observable.read(reader).as_ref().clone() },
            move |delta| {
                for profile in delta.added_values {
                    let cts = Rc::new(CancellationTokenSource::new(None));
                    drop(test_service.start_continuous_run(
                        ResolvedTestRunRequest {
                            continuous: Some(true),
                            group: profile.group,
                            targets: vec![TestRunTarget {
                                test_ids: vec![test_id_for_run.clone().unwrap_or_else(|| profile.controller_id.clone())],
                                controller_id: profile.controller_id.clone(),
                                profile_id: profile.profile_id,
                            }],
                            exclude: None,
                            preserve_focus: None,
                        },
                        cts.token(),
                    ));
                    stores_for_delta.set(profile_key(&profile), cts, false);
                }

                for profile in delta.removed_values {
                    if let Some(cts) = stores_for_delta.get(&profile_key(&profile)) {
                        cts.cancel();
                    }
                    stores_for_delta.delete_and_dispose(&profile_key(&profile));
                }

                last_run.store(stores_for_delta.keys().into_iter().map(|(_, profile_id)| profile_id).collect());
            },
            profile_key,
        ));

        self.change_emitter.fire(test_id);
    }

    /// Stops a continuous run for the profile across all test items that are running it.
    fn stop_profile(&self, profile: &ITestRunProfile) {
        let mut to_delete: Vec<Rc<RunningRef>> = Vec::new();
        let nodes: Vec<Rc<RunningRef>> = self.running.borrow().values().cloned().collect();
        for node in nodes {
            let profs = node.profiles.get();
            let filtered: Vec<ITestRunProfile> =
                profs.iter().filter(|p| p.profile_id != profile.profile_id || p.controller_id != profile.controller_id).cloned().collect();
            if filtered.len() == profs.len() {
                continue;
            } else if filtered.is_empty() {
                to_delete.push(node);
            } else {
                node.profiles.set(Rc::new(filtered), None);
            }
        }

        for node in to_delete.iter().rev() {
            node.handle.dispose();
            self.running.borrow_mut().delete(node.path.iter());
        }

        self.change_emitter.fire(None);
    }

    fn stop(&self, test_id: Option<&str>) {
        let key: Vec<String> = test_id.filter(|id| !id.is_empty()).map(|id| TestId::from_string(id).path().to_vec()).unwrap_or_default();
        let cancellations: Vec<Rc<RunningRef>> = self.running.borrow_mut().delete_recursive(key.iter()).collect();
        // deleteRecursive returns a BFS order, reverse it so children are cancelled before parents
        for cancellation in cancellations.iter().rev() {
            cancellation.handle.dispose();
        }

        self.change_emitter.fire(test_id.filter(|id| !id.is_empty()).map(str::to_owned));
    }
}
