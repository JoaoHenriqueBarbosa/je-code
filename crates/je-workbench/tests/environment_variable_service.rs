//! Porte de `src/vs/workbench/contrib/terminal/test/common/environmentVariableService.test.ts` (suíte `EnvironmentVariable -
//! EnvironmentVariableService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()` e `setup`.** O [`Fixture`] cria o guarda de vazamentos antes de tudo; o
//!   emissor `changeExtensionsEvent`, o `TestInstantiationService` e os serviços criados passam pelo store do guarda.
//! - **`instantiationService.stub(...)`.** O `InstantiationService` do caso tem só os dois serviços que o construtor lê: o
//!   `IExtensionService` (o `TestExtensionService` com `extensions` e `onDidChangeExtensions` trocados: é o
//!   [`StubExtensionService`], que também devolve `whenInstalledExtensionsRegistered` resolvido) e o `IStorageService` (o
//!   `TestStorageService` é o `InMemoryStorageService`). O `IHistoryService` do `setup` não é lido pelo serviço e não entra.
//! - **`class TestEnvironmentVariableService extends EnvironmentVariableService`.** Os métodos `protected` que a subclasse
//!   expõe (`persistCollections` e `notifyCollectionUpdates`) são públicos no [`EnvironmentVariableService`] do porte.
//! - **`{ identifier: { value: 'ext1' } }`.** É a `ExtensionDescription` nula com o identificador trocado.
//! - **`new Map<string, IEnvironmentVariableMutator>()` e `{ map: collection, persistent: true }`.** São o `OrderedMap` e a
//!   `IEnvironmentVariableCollectionWithPersistence`; os mutadores sem `options` têm `options: None`.
//! - **`[...mergedCollection.getVariableMap(...).entries()]`.** É a lista de pares do `OrderedMap`, na ordem de inserção.
//! - **`await mergedCollection.applyToProcessEnvironment(env, scope)`.** O caso assíncrono roda sob `run_with_faked_timers`:
//!   o `debounce` de 1 s do serviço (que o original deixa para o descarte da suíte) é drenado no relógio virtual.
//! - **`IProcessEnvironment`.** É o `ProcessEnvironment` (mapa ordenado de texto).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::event::Emitter;
use je_base::map::OrderedMap;
use je_base::process::ProcessEnvironment;
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::storage::{I_STORAGE_SERVICE, InMemoryStorageService, StorageService};
use je_platform::terminal::environment_variable::{
    EnvironmentVariableMutatorType, EnvironmentVariableScope, IEnvironmentVariableMutator, IEnvironmentVariableMutatorOptions, IExtensionOwnedEnvironmentVariableMutator,
};
use je_platform::workspace::WorkspaceFolder;
use je_workbench::contrib::terminal::common::environment_variable::{IEnvironmentVariableCollectionWithPersistence, IEnvironmentVariableService};
use je_workbench::contrib::terminal::common::environment_variable_service::EnvironmentVariableService;
use je_workbench::services::extensions::extensions::{I_EXTENSION_SERVICE, IExtensionService, IExtensionsChangeEvent, null_extension_description};
use support::extensions_workbench_service_support::StubExtensionService;

struct Fixture {
    instantiation_service: Rc<InstantiationService>,
    environment_variable_service: Rc<EnvironmentVariableService>,
    // Declarado por último: o `Drop` dele descarta o que o `Fixture` registrou e confere os vazamentos.
    store: LeakGuard,
}

impl Fixture {
    /// O `setup` da suíte.
    fn new() -> Self {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let change_extensions_event = store.add_rc(Emitter::<IExtensionsChangeEvent>::new());

        let extensions = ["ext1", "ext2", "ext3"]
            .into_iter()
            .map(|identifier| ExtensionDescription { identifier: ExtensionIdentifier::new(identifier), ..null_extension_description() })
            .collect();
        let extension_service: Rc<dyn IExtensionService> = StubExtensionService::configured(extensions, false, false, change_extensions_event.event());
        let storage = store.add_rc(InMemoryStorageService::new());
        let storage_service: Rc<dyn StorageService> = storage;
        let instantiation_service =
            store.add_rc(InstantiationService::new(ServiceCollection::new().with(I_EXTENSION_SERVICE, extension_service).with(I_STORAGE_SERVICE, storage_service)));

        let environment_variable_service = store.add_rc(instantiation_service.create_instance::<EnvironmentVariableService>(()));
        Self { instantiation_service, environment_variable_service, store }
    }

    /// `store.add(instantiationService.createInstance(TestEnvironmentVariableService))`.
    fn create_service(&self) -> Rc<EnvironmentVariableService> {
        self.store.add_rc(self.instantiation_service.create_instance::<EnvironmentVariableService>(()))
    }
}

/// `{ value, type, variable }`, com `options` e `scope` opcionais.
fn mutator(variable: &str, value: &str, mutator_type: EnvironmentVariableMutatorType) -> IEnvironmentVariableMutator {
    IEnvironmentVariableMutator { variable: variable.to_owned(), value: value.to_owned(), mutator_type, scope: None, options: None }
}

/// `new Map([[key, mutator], ...])`.
fn collection_of<const N: usize>(entries: [(&str, IEnvironmentVariableMutator); N]) -> OrderedMap<String, IEnvironmentVariableMutator> {
    let mut map = OrderedMap::new();
    for (key, entry) in entries {
        map.insert(key.to_owned(), entry);
    }
    map
}

/// `{ map: collection, persistent: true }`.
fn persistent(map: OrderedMap<String, IEnvironmentVariableMutator>) -> IEnvironmentVariableCollectionWithPersistence {
    IEnvironmentVariableCollectionWithPersistence { map, description_map: None, persistent: true }
}

/// `{ extensionIdentifier, type, value, variable, options, scope }`.
fn owned(
    extension_identifier: &str,
    mutator_type: EnvironmentVariableMutatorType,
    value: &str,
    variable: &str,
    options: Option<IEnvironmentVariableMutatorOptions>,
    scope: Option<EnvironmentVariableScope>,
) -> IExtensionOwnedEnvironmentVariableMutator {
    IExtensionOwnedEnvironmentVariableMutator {
        variable: variable.to_owned(),
        value: value.to_owned(),
        mutator_type,
        scope,
        options,
        extension_identifier: extension_identifier.to_owned(),
    }
}

/// `[...service.mergedCollection.getVariableMap(scope).entries()]`.
fn variable_map_entries(service: &EnvironmentVariableService, scope: Option<&EnvironmentVariableScope>) -> Vec<(String, Vec<IExtensionOwnedEnvironmentVariableMutator>)> {
    service.merged_collection().get_variable_map(scope).iter().map(|(variable, mutators)| (variable.clone(), mutators.clone())).collect()
}

fn workspace_scope(name: &str, index: usize) -> EnvironmentVariableScope {
    EnvironmentVariableScope { workspace_folder: Some(WorkspaceFolder::new(URI::file(name, je_platform::resources::host_platform()), name, index, None)) }
}

fn entry(variable: &str, mutators: Vec<IExtensionOwnedEnvironmentVariableMutator>) -> (String, Vec<IExtensionOwnedEnvironmentVariableMutator>) {
    (variable.to_owned(), mutators)
}

#[test]
fn should_persist_collections_to_the_storage_service_and_be_able_to_restore_from_them() {
    let fixture = Fixture::new();
    let service = fixture.environment_variable_service.clone();
    let both = Some(IEnvironmentVariableMutatorOptions { apply_at_process_creation: Some(true), apply_at_shell_integration: Some(true) });
    let collection = collection_of([
        ("A-key", mutator("A", "a", EnvironmentVariableMutatorType::Replace)),
        ("B-key", mutator("B", "b", EnvironmentVariableMutatorType::Append)),
        ("C-key", IEnvironmentVariableMutator { options: both, ..mutator("C", "c", EnvironmentVariableMutatorType::Prepend) }),
    ]);
    service.set("ext1", persistent(collection));
    let expected = vec![
        entry("A", vec![owned("ext1", EnvironmentVariableMutatorType::Replace, "a", "A", None, None)]),
        entry("B", vec![owned("ext1", EnvironmentVariableMutatorType::Append, "b", "B", None, None)]),
        entry("C", vec![owned("ext1", EnvironmentVariableMutatorType::Prepend, "c", "C", both, None)]),
    ];
    assert_eq!(variable_map_entries(&service, None), expected);

    // Persist with old service, create a new service with the same storage service to verify restore
    service.persist_collections();
    let service2 = fixture.create_service();
    assert_eq!(variable_map_entries(&service2, None), expected);
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('mergedCollection')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn merged_collection_should_overwrite_any_other_variable_with_the_first_extension_that_replaces() {
    let fixture = Fixture::new();
    let service = fixture.environment_variable_service.clone();
    let collection1 = collection_of([
        ("A-key", mutator("A", "a1", EnvironmentVariableMutatorType::Append)),
        ("B-key", mutator("B", "b1", EnvironmentVariableMutatorType::Replace)),
    ]);
    let collection2 = collection_of([
        ("A-key", mutator("A", "a2", EnvironmentVariableMutatorType::Replace)),
        ("B-key", mutator("B", "b2", EnvironmentVariableMutatorType::Append)),
    ]);
    let collection3 = collection_of([
        ("A-key", mutator("A", "a3", EnvironmentVariableMutatorType::Prepend)),
        ("B-key", mutator("B", "b3", EnvironmentVariableMutatorType::Replace)),
    ]);
    service.set("ext1", persistent(collection1));
    service.set("ext2", persistent(collection2));
    service.set("ext3", persistent(collection3));
    assert_eq!(
        variable_map_entries(&service, None),
        vec![
            entry(
                "A",
                vec![
                    owned("ext2", EnvironmentVariableMutatorType::Replace, "a2", "A", None, None),
                    owned("ext1", EnvironmentVariableMutatorType::Append, "a1", "A", None, None),
                ]
            ),
            entry("B", vec![owned("ext1", EnvironmentVariableMutatorType::Replace, "b1", "B", None, None)]),
        ]
    );
}

#[test]
fn merged_collection_should_correctly_apply_the_environment_values_from_multiple_extension_contributions_in_the_correct_order() {
    let fixture = Fixture::new();
    run_with_faked_timers(async move {
        let service = fixture.environment_variable_service.clone();
        let collection1 = collection_of([("A-key", mutator("A", ":a1", EnvironmentVariableMutatorType::Append))]);
        let collection2 = collection_of([("A-key", mutator("A", "a2:", EnvironmentVariableMutatorType::Prepend))]);
        let collection3 = collection_of([("A-key", mutator("A", "a3", EnvironmentVariableMutatorType::Replace))]);
        service.set("ext1", persistent(collection1));
        service.set("ext2", persistent(collection2));
        service.set("ext3", persistent(collection3));

        // The entries should be ordered in the order they are applied
        assert_eq!(
            variable_map_entries(&service, None),
            vec![entry(
                "A",
                vec![
                    owned("ext3", EnvironmentVariableMutatorType::Replace, "a3", "A", None, None),
                    owned("ext2", EnvironmentVariableMutatorType::Prepend, "a2:", "A", None, None),
                    owned("ext1", EnvironmentVariableMutatorType::Append, ":a1", "A", None, None),
                ]
            )]
        );

        // Verify the entries get applied to the environment as expected
        let mut env: ProcessEnvironment = ProcessEnvironment::from([("A".to_owned(), "foo".to_owned())]);
        let merged = service.merged_collection();
        merged.apply_to_process_environment(&mut env, None, None).await.unwrap();
        assert_eq!(env, ProcessEnvironment::from([("A".to_owned(), "a2:a3:a1".to_owned())]));
    });
}

#[test]
fn merged_collection_should_correctly_apply_the_workspace_specific_environment_values_from_multiple_extension_contributions_in_the_correct_order() {
    let fixture = Fixture::new();
    run_with_faked_timers(async move {
        let service = fixture.environment_variable_service.clone();
        let scope1 = workspace_scope("workspace1", 0);
        let scope2 = workspace_scope("workspace2", 3);
        let collection1 = collection_of([("A-key", IEnvironmentVariableMutator { scope: Some(scope1.clone()), ..mutator("A", ":a1", EnvironmentVariableMutatorType::Append) })]);
        let collection2 = collection_of([("A-key", mutator("A", "a2:", EnvironmentVariableMutatorType::Prepend))]);
        let collection3 = collection_of([("A-key", IEnvironmentVariableMutator { scope: Some(scope2), ..mutator("A", "a3", EnvironmentVariableMutatorType::Replace) })]);
        service.set("ext1", persistent(collection1));
        service.set("ext2", persistent(collection2));
        service.set("ext3", persistent(collection3));

        // The entries should be ordered in the order they are applied
        assert_eq!(
            variable_map_entries(&service, Some(&scope1)),
            vec![entry(
                "A",
                vec![
                    owned("ext2", EnvironmentVariableMutatorType::Prepend, "a2:", "A", None, None),
                    owned("ext1", EnvironmentVariableMutatorType::Append, ":a1", "A", None, Some(scope1.clone())),
                ]
            )]
        );

        // Verify the entries get applied to the environment as expected
        let mut env: ProcessEnvironment = ProcessEnvironment::from([("A".to_owned(), "foo".to_owned())]);
        let merged = service.merged_collection();
        merged.apply_to_process_environment(&mut env, Some(&scope1), None).await.unwrap();
        assert_eq!(env, ProcessEnvironment::from([("A".to_owned(), "a2:foo:a1".to_owned())]));
    });
}
