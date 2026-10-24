//! Porte de `src/vs/workbench/contrib/terminal/test/common/environmentVariableShared.test.ts` (suítes
//! `EnvironmentVariable - deserializeEnvironmentVariableCollection` e
//! `EnvironmentVariable - serializeEnvironmentVariableCollection`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `deepStrictEqual` vira `assert_eq!`; o campo `type` do mutador é `mutator_type`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::map::OrderedMap;
use je_platform::terminal::environment_variable::{EnvironmentVariableMutatorType, IEnvironmentVariableMutator};
use je_platform::terminal::environment_variable_shared::{
    deserialize_environment_variable_collection, serialize_environment_variable_collection,
};

/// `{ value, type, variable }`.
fn mutator(variable: &str, value: &str, mutator_type: EnvironmentVariableMutatorType) -> IEnvironmentVariableMutator {
    IEnvironmentVariableMutator { variable: variable.to_string(), value: value.to_string(), mutator_type, scope: None, options: None }
}

// suite('EnvironmentVariable - deserializeEnvironmentVariableCollection')

#[test]
fn should_construct_correctly_with_3_arguments() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let c = deserialize_environment_variable_collection(vec![
        ("A".to_string(), mutator("A", "a", EnvironmentVariableMutatorType::Replace)),
        ("B".to_string(), mutator("B", "b", EnvironmentVariableMutatorType::Append)),
        ("C".to_string(), mutator("C", "c", EnvironmentVariableMutatorType::Prepend)),
    ]);
    let keys: Vec<&String> = c.keys().collect();
    assert_eq!(keys, ["A", "B", "C"]);
    assert_eq!(c.get(&"A".to_string()), Some(&mutator("A", "a", EnvironmentVariableMutatorType::Replace)));
    assert_eq!(c.get(&"B".to_string()), Some(&mutator("B", "b", EnvironmentVariableMutatorType::Append)));
    assert_eq!(c.get(&"C".to_string()), Some(&mutator("C", "c", EnvironmentVariableMutatorType::Prepend)));
}

// suite('EnvironmentVariable - serializeEnvironmentVariableCollection')

#[test]
fn should_correctly_serialize_the_object() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut collection: OrderedMap<String, IEnvironmentVariableMutator> = OrderedMap::new();
    assert_eq!(serialize_environment_variable_collection(&collection), vec![]);
    collection.insert("A".to_string(), mutator("A", "a", EnvironmentVariableMutatorType::Replace));
    collection.insert("B".to_string(), mutator("B", "b", EnvironmentVariableMutatorType::Append));
    collection.insert("C".to_string(), mutator("C", "c", EnvironmentVariableMutatorType::Prepend));
    assert_eq!(
        serialize_environment_variable_collection(&collection),
        vec![
            ("A".to_string(), mutator("A", "a", EnvironmentVariableMutatorType::Replace)),
            ("B".to_string(), mutator("B", "b", EnvironmentVariableMutatorType::Append)),
            ("C".to_string(), mutator("C", "c", EnvironmentVariableMutatorType::Prepend)),
        ]
    );
}
