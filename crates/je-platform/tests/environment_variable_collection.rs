//! Porte de `src/vs/workbench/contrib/terminal/test/common/environmentVariableCollection.test.ts`
//! (18 casos, na ordem do original).
//!
//! O `ensureNoDisposablesAreLeakedInTestSuite` vira o `LeakGuard` criado no começo de cada teste e conferido
//! por último. Os mapas do original (`Map`, que itera na ordem de inserção) são o `OrderedMap`, e o
//! `deepStrictEqual` sobre `[...map.entries()]` compara o vetor de pares na mesma ordem. O `isWindows` do
//! teste que depende do sistema é o `Platform::detect().is_windows`, como no módulo portado.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::collections::BTreeMap;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::map::OrderedMap;
use je_base::path::Platform as PathPlatform;
use je_base::platform::Platform;
use je_base::process::ProcessEnvironment;
use je_base::uri::URI;
use je_platform::terminal::environment_variable::{
    EnvironmentVariableMutatorType, EnvironmentVariableScope, IEnvironmentVariableCollection, IEnvironmentVariableCollectionDescription, IEnvironmentVariableMutator,
    IEnvironmentVariableMutatorOptions, IExtensionOwnedEnvironmentVariableMutator, IMergedEnvironmentVariableCollection,
};
use je_platform::terminal::environment_variable_collection::MergedEnvironmentVariableCollection;
use je_platform::terminal::environment_variable_shared::{deserialize_environment_description_map, deserialize_environment_variable_collection};
use je_platform::workspace::WorkspaceFolder;

use EnvironmentVariableMutatorType::{Append, Prepend, Replace};

type Mutator = IEnvironmentVariableMutator;
type Owned = IExtensionOwnedEnvironmentVariableMutator;
type Scope = EnvironmentVariableScope;

fn mutator(variable: &str, value: &str, mutator_type: EnvironmentVariableMutatorType, scope: Option<&Scope>) -> Mutator {
    Mutator { variable: variable.to_string(), value: value.to_string(), mutator_type, scope: scope.cloned(), options: None }
}

fn owned(extension: &str, variable: &str, value: &str, mutator_type: EnvironmentVariableMutatorType, scope: Option<&Scope>) -> Owned {
    Owned {
        extension_identifier: extension.to_string(),
        variable: variable.to_string(),
        value: value.to_string(),
        mutator_type,
        scope: scope.cloned(),
        options: None,
    }
}

fn workspace_scope(name: &str, index: usize) -> Scope {
    Scope { workspace_folder: Some(WorkspaceFolder::new(URI::file(name, PathPlatform::Posix), name, index, None)) }
}

fn collection(entries: Vec<(&str, Mutator)>) -> IEnvironmentVariableCollection {
    IEnvironmentVariableCollection {
        map: deserialize_environment_variable_collection(entries.into_iter().map(|(key, mutator)| (key.to_string(), mutator)).collect()),
        description_map: None,
    }
}

fn merged(extensions: Vec<(&str, IEnvironmentVariableCollection)>) -> MergedEnvironmentVariableCollection {
    let mut collections = OrderedMap::new();
    for (extension, collection) in extensions {
        collections.insert(extension.to_string(), collection);
    }
    MergedEnvironmentVariableCollection::new(collections)
}

/// `[...map.entries()]`.
fn entries(map: &OrderedMap<String, Vec<Owned>>) -> Vec<(String, Vec<Owned>)> {
    map.iter().map(|(key, mutators)| (key.clone(), mutators.clone())).collect()
}

fn pair(variable: &str, mutators: Vec<Owned>) -> (String, Vec<Owned>) {
    (variable.to_string(), mutators)
}

fn env_of(pairs: &[(&str, &str)]) -> ProcessEnvironment {
    pairs.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect()
}

// ---- ctor ------------------------------------------------------------------------------------------

#[test]
fn ctor_should_keep_entries_that_come_after_a_prepend_or_append_type_mutators() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext4_options = IEnvironmentVariableMutatorOptions { apply_at_process_creation: Some(true), apply_at_shell_integration: Some(true) };
    let merged = merged(vec![
        ("ext1", collection(vec![("A-key", mutator("A", "a1", Prepend, None))])),
        ("ext2", collection(vec![("A-key", mutator("A", "a2", Append, None))])),
        ("ext3", collection(vec![("A-key", mutator("A", "a3", Prepend, None))])),
        ("ext4", collection(vec![("A-key", Mutator { options: Some(ext4_options), ..mutator("A", "a4", Append, None) })])),
    ]);
    assert_eq!(
        entries(&merged.get_variable_map(None)),
        vec![pair(
            "A",
            vec![
                Owned { options: Some(ext4_options), ..owned("ext4", "A", "a4", Append, None) },
                owned("ext3", "A", "a3", Prepend, None),
                owned("ext2", "A", "a2", Append, None),
                owned("ext1", "A", "a1", Prepend, None),
            ]
        )]
    );
}

#[test]
fn ctor_should_remove_entries_that_come_after_a_replace_type_mutator() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let merged = merged(vec![
        ("ext1", collection(vec![("A-key", mutator("A", "a1", Prepend, None))])),
        ("ext2", collection(vec![("A-key", mutator("A", "a2", Append, None))])),
        ("ext3", collection(vec![("A-key", mutator("A", "a3", Replace, None))])),
        ("ext4", collection(vec![("A-key", mutator("A", "a4", Append, None))])),
    ]);
    assert_eq!(
        entries(&merged.get_variable_map(None)),
        vec![pair(
            "A",
            vec![owned("ext3", "A", "a3", Replace, None), owned("ext2", "A", "a2", Append, None), owned("ext1", "A", "a1", Prepend, None)]
        )],
        "The ext4 entry should be removed as it comes after a Replace"
    );
}

#[test]
fn ctor_appropriate_workspace_scoped_entries_are_returned_when_querying_for_a_particular_workspace_folder() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let scope1 = workspace_scope("workspace1", 0);
    let scope2 = workspace_scope("workspace2", 3);
    let merged = merged(vec![
        ("ext1", collection(vec![("A-key", mutator("A", "a1", Prepend, Some(&scope1)))])),
        ("ext2", collection(vec![("A-key", mutator("A", "a2", Append, None))])),
        ("ext3", collection(vec![("A-key", mutator("A", "a3", Prepend, Some(&scope2)))])),
        ("ext4", collection(vec![("A-key", mutator("A", "a4", Append, None))])),
    ]);
    assert_eq!(
        entries(&merged.get_variable_map(Some(&scope2))),
        vec![pair(
            "A",
            vec![owned("ext4", "A", "a4", Append, None), owned("ext3", "A", "a3", Prepend, Some(&scope2)), owned("ext2", "A", "a2", Append, None)]
        )]
    );
}

#[test]
fn ctor_workspace_scoped_entries_are_not_included_when_looking_for_global_entries() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let scope1 = workspace_scope("workspace1", 0);
    let scope2 = workspace_scope("workspace2", 3);
    let merged = merged(vec![
        ("ext1", collection(vec![("A-key", mutator("A", "a1", Prepend, Some(&scope1)))])),
        ("ext2", collection(vec![("A-key", mutator("A", "a2", Append, None))])),
        ("ext3", collection(vec![("A-key", mutator("A", "a3", Prepend, Some(&scope2)))])),
        ("ext4", collection(vec![("A-key", mutator("A", "a4", Append, None))])),
    ]);
    assert_eq!(
        entries(&merged.get_variable_map(None)),
        vec![pair("A", vec![owned("ext4", "A", "a4", Append, None), owned("ext2", "A", "a2", Append, None)])]
    );
}

#[test]
fn ctor_workspace_scoped_description_entries_are_properly_filtered_for_each_extension() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let scope1 = workspace_scope("workspace1", 0);
    let scope2 = workspace_scope("workspace2", 3);
    let description = |text: &str, scope: Option<&Scope>| IEnvironmentVariableCollectionDescription { description: Some(text.to_string()), scope: scope.cloned() };
    let with_descriptions = |mut base: IEnvironmentVariableCollection, descriptions: Vec<(&str, IEnvironmentVariableCollectionDescription)>| {
        base.description_map = Some(deserialize_environment_description_map(Some(descriptions.into_iter().map(|(key, value)| (key.to_string(), value)).collect())));
        base
    };
    let merged = merged(vec![
        (
            "ext1",
            with_descriptions(
                collection(vec![("A-key", mutator("A", "a1", Prepend, Some(&scope1)))]),
                vec![("A-key-scope1", description("ext1 scope1 description", Some(&scope1))), ("A-key-scope2", description("ext1 scope2 description", Some(&scope2)))],
            ),
        ),
        (
            "ext2",
            with_descriptions(collection(vec![("A-key", mutator("A", "a2", Append, None))]), vec![("A-key", description("ext2 global description", None))]),
        ),
        (
            "ext3",
            with_descriptions(
                collection(vec![("A-key", mutator("A", "a3", Prepend, Some(&scope2)))]),
                vec![("A-key", description("ext3 scope2 description", Some(&scope2)))],
            ),
        ),
        ("ext4", collection(vec![("A-key", mutator("A", "a4", Append, None))])),
    ]);
    let description_entries = |scope: Option<&Scope>| -> Vec<(String, Option<String>)> {
        merged.get_description_map(scope).iter().map(|(key, value)| (key.clone(), value.clone())).collect()
    };
    assert_eq!(description_entries(Some(&scope1)), vec![("ext1".to_string(), Some("ext1 scope1 description".to_string()))]);
    assert_eq!(description_entries(None), vec![("ext2".to_string(), Some("ext2 global description".to_string()))]);
}

// ---- applyToProcessEnvironment ---------------------------------------------------------------------

#[test]
fn apply_should_apply_the_collection_to_an_environment() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let merged = merged(vec![(
            "ext",
            collection(vec![
                ("A-key", mutator("A", "a", Replace, None)),
                ("B", mutator("B", "b", Append, None)),
                ("C", mutator("C", "c", Prepend, None)),
            ]),
        )]);
        let mut env = env_of(&[("A", "foo"), ("B", "bar"), ("C", "baz")]);
        merged.apply_to_process_environment(&mut env, None, None).await.unwrap();
        assert_eq!(env, env_of(&[("A", "a"), ("B", "barb"), ("C", "cbaz")]));
    });
}

#[test]
fn apply_should_apply_the_appropriate_workspace_scoped_entries_to_an_environment() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let scope1 = workspace_scope("workspace1", 0);
        let scope2 = workspace_scope("workspace2", 3);
        let merged = merged(vec![(
            "ext",
            collection(vec![
                ("A-key", mutator("A", "a", Replace, Some(&scope1))),
                ("B", mutator("B", "b", Append, Some(&scope2))),
                ("C", mutator("C", "c", Prepend, None)),
            ]),
        )]);
        let mut env = env_of(&[("A", "foo"), ("B", "bar"), ("C", "baz")]);
        merged.apply_to_process_environment(&mut env, Some(&scope1), None).await.unwrap();
        assert_eq!(
            env,
            env_of(&[
                ("A", "a"),
                ("B", "bar"), // This is not changed because the scope does not match
                ("C", "cbaz"),
            ])
        );
    });
}

#[test]
fn apply_should_apply_the_collection_to_environment_entries_with_no_values() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let merged = merged(vec![(
            "ext",
            collection(vec![
                ("A-key", mutator("A", "a", Replace, None)),
                ("B", mutator("B", "b", Append, None)),
                ("C", mutator("C", "c", Prepend, None)),
            ]),
        )]);
        let mut env = ProcessEnvironment::new();
        merged.apply_to_process_environment(&mut env, None, None).await.unwrap();
        assert_eq!(env, env_of(&[("A", "a"), ("B", "b"), ("C", "c")]));
    });
}

#[test]
fn apply_should_apply_to_variable_case_insensitively_on_windows_only() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let merged = merged(vec![(
            "ext",
            collection(vec![
                ("A-key", mutator("a", "a", Replace, None)),
                ("b", mutator("b", "b", Append, None)),
                ("c", mutator("c", "c", Prepend, None)),
            ]),
        )]);
        let mut env = env_of(&[("A", "A"), ("B", "B"), ("C", "C")]);
        merged.apply_to_process_environment(&mut env, None, None).await.unwrap();
        let expected: BTreeMap<String, String> = if Platform::detect().is_windows {
            env_of(&[("A", "a"), ("B", "Bb"), ("C", "cC")])
        } else {
            env_of(&[("a", "a"), ("A", "A"), ("b", "b"), ("B", "B"), ("c", "c"), ("C", "C")])
        };
        assert_eq!(env, expected);
    });
}

// ---- diff ------------------------------------------------------------------------------------------

#[test]
fn diff_should_return_undefined_when_collectinos_are_the_same() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let merged1 = merged(vec![("ext1", collection(vec![("A-key", mutator("A", "a", Replace, None))]))]);
    let merged2 = merged(vec![("ext1", collection(vec![("A-key", mutator("A", "a", Replace, None))]))]);
    assert!(merged1.diff(&merged2, None).is_none());
}

#[test]
fn diff_should_generate_added_diffs_from_when_the_first_entry_is_added() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let merged1 = merged(vec![]);
    let merged2 = merged(vec![("ext1", collection(vec![("A-key", mutator("A", "a", Replace, None))]))]);
    let diff = merged1.diff(&merged2, None).unwrap();
    assert_eq!(diff.changed.len(), 0);
    assert_eq!(diff.removed.len(), 0);
    assert_eq!(entries(&diff.added), vec![pair("A", vec![owned("ext1", "A", "a", Replace, None)])]);
}

#[test]
fn diff_should_generate_added_diffs_from_the_same_extension() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let merged1 = merged(vec![("ext1", collection(vec![("A-key", mutator("A", "a", Replace, None))]))]);
    let merged2 = merged(vec![(
        "ext1",
        collection(vec![("A-key", mutator("A", "a", Replace, None)), ("B", mutator("B", "b", Append, None))]),
    )]);
    let diff = merged1.diff(&merged2, None).unwrap();
    assert_eq!(diff.changed.len(), 0);
    assert_eq!(diff.removed.len(), 0);
    assert_eq!(entries(&diff.added), vec![pair("B", vec![owned("ext1", "B", "b", Append, None)])]);
}

#[test]
fn diff_should_generate_added_diffs_from_a_different_extension() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let merged1 = merged(vec![("ext1", collection(vec![("A-key", mutator("A", "a1", Prepend, None))]))]);

    let merged2 = merged(vec![
        ("ext2", collection(vec![("A-key", mutator("A", "a2", Append, None))])),
        ("ext1", collection(vec![("A-key", mutator("A", "a1", Prepend, None))])),
    ]);
    let diff = merged1.diff(&merged2, None).unwrap();
    assert_eq!(diff.changed.len(), 0);
    assert_eq!(diff.removed.len(), 0);
    assert_eq!(entries(&diff.added), vec![pair("A", vec![owned("ext2", "A", "a2", Append, None)])]);

    let merged3 = merged(vec![
        ("ext1", collection(vec![("A-key", mutator("A", "a1", Prepend, None))])),
        // This entry should get removed
        ("ext2", collection(vec![("A-key", mutator("A", "a2", Append, None))])),
    ]);
    let diff2 = merged1.diff(&merged3, None).unwrap();
    assert_eq!(diff2.changed.len(), 0);
    assert_eq!(diff2.removed.len(), 0);
    assert_eq!(entries(&diff.added), entries(&diff2.added), "Swapping the order of the entries in the other collection should yield the same result");
}

#[test]
fn diff_should_remove_entries_in_the_diff_that_come_after_a_replace() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let merged1 = merged(vec![("ext1", collection(vec![("A-key", mutator("A", "a1", Replace, None))]))]);
    let merged4 = merged(vec![
        ("ext1", collection(vec![("A-key", mutator("A", "a1", Replace, None))])),
        // This entry should get removed as it comes after a replace
        ("ext2", collection(vec![("A-key", mutator("A", "a2", Append, None))])),
    ]);
    let diff = merged1.diff(&merged4, None);
    assert!(diff.is_none(), "Replace should ignore any entries after it");
}

#[test]
fn diff_should_generate_removed_diffs() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let merged1 = merged(vec![(
        "ext1",
        collection(vec![("A-key", mutator("A", "a", Replace, None)), ("B", mutator("B", "b", Replace, None))]),
    )]);
    let merged2 = merged(vec![("ext1", collection(vec![("A-key", mutator("A", "a", Replace, None))]))]);
    let diff = merged1.diff(&merged2, None).unwrap();
    assert_eq!(diff.changed.len(), 0);
    assert_eq!(diff.added.len(), 0);
    assert_eq!(entries(&diff.removed), vec![pair("B", vec![owned("ext1", "B", "b", Replace, None)])]);
}

#[test]
fn diff_should_generate_changed_diffs() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let merged1 = merged(vec![(
        "ext1",
        collection(vec![("A-key", mutator("A", "a1", Replace, None)), ("B", mutator("B", "b", Replace, None))]),
    )]);
    let merged2 = merged(vec![(
        "ext1",
        collection(vec![("A-key", mutator("A", "a2", Replace, None)), ("B", mutator("B", "b", Append, None))]),
    )]);
    let diff = merged1.diff(&merged2, None).unwrap();
    assert_eq!(diff.added.len(), 0);
    assert_eq!(diff.removed.len(), 0);
    assert_eq!(
        entries(&diff.changed),
        vec![pair("A", vec![owned("ext1", "A", "a2", Replace, None)]), pair("B", vec![owned("ext1", "B", "b", Append, None)])]
    );
}

#[test]
fn diff_should_generate_diffs_with_added_changed_and_removed() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let merged1 = merged(vec![(
        "ext1",
        collection(vec![("A-key", mutator("A", "a1", Replace, None)), ("B", mutator("B", "b", Prepend, None))]),
    )]);
    let merged2 = merged(vec![(
        "ext1",
        collection(vec![("A-key", mutator("A", "a2", Replace, None)), ("C", mutator("C", "c", Append, None))]),
    )]);
    let diff = merged1.diff(&merged2, None).unwrap();
    assert_eq!(entries(&diff.added), vec![pair("C", vec![owned("ext1", "C", "c", Append, None)])]);
    assert_eq!(entries(&diff.removed), vec![pair("B", vec![owned("ext1", "B", "b", Prepend, None)])]);
    assert_eq!(entries(&diff.changed), vec![pair("A", vec![owned("ext1", "A", "a2", Replace, None)])]);
}

#[test]
fn diff_should_only_generate_workspace_specific_diffs() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let scope1 = workspace_scope("workspace1", 0);
    let scope2 = workspace_scope("workspace2", 3);
    let merged1 = merged(vec![(
        "ext1",
        collection(vec![("A-key", mutator("A", "a1", Replace, Some(&scope1))), ("B", mutator("B", "b", Prepend, None))]),
    )]);
    let merged2 = merged(vec![(
        "ext1",
        collection(vec![("A-key", mutator("A", "a2", Replace, Some(&scope1))), ("C", mutator("C", "c", Append, Some(&scope2)))]),
    )]);
    let diff = merged1.diff(&merged2, Some(&scope1)).unwrap();
    assert_eq!(diff.added.len(), 0);
    assert_eq!(entries(&diff.removed), vec![pair("B", vec![owned("ext1", "B", "b", Prepend, None)])]);
    assert_eq!(entries(&diff.changed), vec![pair("A", vec![owned("ext1", "A", "a2", Replace, Some(&scope1))])]);
}
