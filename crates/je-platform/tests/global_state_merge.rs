//! Porte de `src/vs/platform/userDataSync/test/common/globalStateMerge.test.ts` (25 casos, na ordem do original).

use je_base::JsString;
use je_platform::log::NullLogService;
use je_platform::user_data_sync::global_state_merge::{MergeResult, StorageDictionary, StorageKeys, merge};
use je_platform::user_data_sync::user_data_sync::IStorageValue;

/// Um objeto literal `{ 'chave': { version: 1, value: 'valor' }, ... }`, na ordem dada.
fn dict(entries: &[(&str, &str)]) -> StorageDictionary {
    let mut result = StorageDictionary::new();
    for (key, value) in entries {
        result.insert(JsString::from(*key), IStorageValue { version: 1.0, value: JsString::from(*value), scope: None });
    }
    result
}

fn keys(names: &[&str]) -> Vec<JsString> {
    names.iter().map(|name| JsString::from(*name)).collect()
}

fn sorted_entries(actual: &StorageDictionary) -> Vec<(String, f64, String)> {
    let mut entries: Vec<(String, f64, String)> =
        actual.iter().map(|(k, v)| (k.to_string_lossy(), v.version, v.value.to_string_lossy())).collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

/// O `assert.deepStrictEqual` entre objetos não olha a ordem das chaves.
fn assert_dict(actual: &StorageDictionary, expected: &[(&str, &str)]) {
    assert_eq!(sorted_entries(actual), sorted_entries(&dict(expected)));
}

fn assert_removed(actual: &[JsString], expected: &[&str]) {
    let actual: Vec<String> = actual.iter().map(JsString::to_string_lossy).collect();
    assert_eq!(actual, expected);
}

fn assert_all(actual: &Option<StorageDictionary>, expected: Option<&[(&str, &str)]>) {
    match (actual, expected) {
        (None, None) => {}
        (Some(actual), Some(expected)) => assert_dict(actual, expected),
        (actual, expected) => panic!("remote.all diverge: obtido {:?}, esperado {:?}", actual.is_some(), expected.is_some()),
    }
}

/// As quatro asserções de cada caso, na ordem do original.
fn check(
    actual: &MergeResult,
    local_added: &[(&str, &str)],
    local_updated: &[(&str, &str)],
    local_removed: &[&str],
    remote_all: Option<&[(&str, &str)]>,
) {
    assert_dict(&actual.local.added, local_added);
    assert_dict(&actual.local.updated, local_updated);
    assert_removed(&actual.local.removed, local_removed);
    assert_all(&actual.remote.all, remote_all);
}

fn run(
    local: &StorageDictionary,
    remote: &StorageDictionary,
    base: Option<&StorageDictionary>,
    machine: &[&str],
    unregistered: &[&str],
) -> MergeResult {
    let machine = keys(machine);
    let unregistered = keys(unregistered);
    merge(local, Some(remote), base, StorageKeys { machine: &machine, unregistered: &unregistered }, &NullLogService)
}

#[test]
fn merge_when_local_and_remote_are_same_with_one_value_and_local_is_not_synced_yet() {
    let local = dict(&[("a", "a")]);
    let remote = dict(&[("a", "a")]);

    let actual = run(&local, &remote, None, &[], &[]);

    check(&actual, &[], &[], &[], None);
}

#[test]
fn merge_when_local_and_remote_are_same_with_multiple_entries_and_local_is_not_synced_yet() {
    let local = dict(&[("a", "a"), ("b", "b")]);
    let remote = dict(&[("a", "a"), ("b", "b")]);

    let actual = run(&local, &remote, None, &[], &[]);

    check(&actual, &[], &[], &[], None);
}

#[test]
fn merge_when_local_and_remote_are_same_with_multiple_entries_in_different_order_and_local_is_not_synced_yet() {
    let local = dict(&[("a", "a"), ("b", "b")]);
    let remote = dict(&[("b", "b"), ("a", "a")]);

    let actual = run(&local, &remote, None, &[], &[]);

    check(&actual, &[], &[], &[], None);
}

#[test]
fn merge_when_local_and_remote_are_same_with_different_base_content() {
    let local = dict(&[("a", "a"), ("b", "b")]);
    let remote = dict(&[("b", "b"), ("a", "a")]);
    let base = dict(&[("b", "a")]);

    let actual = run(&local, &remote, Some(&base), &[], &[]);

    check(&actual, &[], &[], &[], None);
}

#[test]
fn merge_when_a_new_entry_is_added_to_remote_and_local_has_not_synced_yet() {
    let local = dict(&[("a", "a")]);
    let remote = dict(&[("b", "b"), ("a", "a")]);

    let actual = run(&local, &remote, None, &[], &[]);

    check(&actual, &[("b", "b")], &[], &[], None);
}

#[test]
fn merge_when_multiple_new_entries_are_added_to_remote_and_local_is_not_synced_yet() {
    let local = dict(&[]);
    let remote = dict(&[("b", "b"), ("a", "a")]);

    let actual = run(&local, &remote, None, &[], &[]);

    check(&actual, &[("b", "b"), ("a", "a")], &[], &[], None);
}

#[test]
fn merge_when_new_entry_is_added_to_remote_from_base_and_local_has_not_changed() {
    let local = dict(&[("a", "a")]);
    let remote = dict(&[("b", "b"), ("a", "a")]);

    let actual = run(&local, &remote, Some(&local), &[], &[]);

    check(&actual, &[("b", "b")], &[], &[], None);
}

#[test]
fn merge_when_an_entry_is_removed_from_remote_from_base_and_local_has_not_changed() {
    let local = dict(&[("b", "b"), ("a", "a")]);
    let remote = dict(&[("a", "a")]);

    let actual = run(&local, &remote, Some(&local), &[], &[]);

    check(&actual, &[], &[], &["b"], None);
}

#[test]
fn merge_when_all_entries_are_removed_from_base_and_local_has_not_changed() {
    let local = dict(&[("b", "b"), ("a", "a")]);
    let remote = dict(&[]);

    let actual = run(&local, &remote, Some(&local), &[], &[]);

    check(&actual, &[], &[], &["b", "a"], None);
}

#[test]
fn merge_when_an_entry_is_updated_in_remote_from_base_and_local_has_not_changed() {
    let local = dict(&[("a", "a")]);
    let remote = dict(&[("a", "b")]);

    let actual = run(&local, &remote, Some(&local), &[], &[]);

    check(&actual, &[], &[("a", "b")], &[], None);
}

#[test]
fn merge_when_remote_has_moved_forwarded_with_multiple_changes_and_local_stays_with_base() {
    let local = dict(&[("a", "a"), ("b", "b")]);
    let remote = dict(&[("a", "d"), ("c", "c")]);

    let actual = run(&local, &remote, Some(&local), &[], &[]);

    check(&actual, &[("c", "c")], &[("a", "d")], &["b"], None);
}

#[test]
fn merge_when_new_entries_are_added_to_local_and_local_is_not_synced_yet() {
    let local = dict(&[("a", "a"), ("b", "b")]);
    let remote = dict(&[("a", "a")]);

    let actual = run(&local, &remote, None, &[], &[]);

    check(&actual, &[], &[], &[], Some(&[("a", "a"), ("b", "b")]));
}

#[test]
fn merge_when_multiple_new_entries_are_added_to_local_from_base_and_remote_is_not_changed() {
    let local = dict(&[("a", "a"), ("b", "b"), ("c", "c")]);
    let remote = dict(&[("a", "a")]);

    let actual = run(&local, &remote, Some(&remote), &[], &[]);

    check(&actual, &[], &[], &[], Some(&[("a", "a"), ("b", "b"), ("c", "c")]));
}

#[test]
fn merge_when_an_entry_is_removed_from_local_from_base_and_remote_has_not_changed() {
    let local = dict(&[("a", "a")]);
    let remote = dict(&[("a", "a"), ("b", "b")]);

    let actual = run(&local, &remote, Some(&remote), &[], &[]);

    check(&actual, &[], &[], &[], Some(&[("a", "a")]));
}

#[test]
fn merge_when_an_entry_is_updated_in_local_from_base_and_remote_has_not_changed() {
    let local = dict(&[("a", "b")]);
    let remote = dict(&[("a", "a")]);

    let actual = run(&local, &remote, Some(&remote), &[], &[]);

    check(&actual, &[], &[], &[], Some(&[("a", "b")]));
}

#[test]
fn merge_when_local_has_moved_forwarded_with_multiple_changes_and_remote_stays_with_base() {
    let local = dict(&[("a", "d"), ("b", "b")]);
    let remote = dict(&[("a", "a"), ("c", "c")]);

    let actual = run(&local, &remote, Some(&remote), &[], &[]);

    check(&actual, &[], &[], &[], Some(&[("a", "d"), ("b", "b")]));
}

#[test]
fn merge_when_local_and_remote_with_one_entry_but_different_value_and_local_is_not_synced_yet() {
    let local = dict(&[("a", "a")]);
    let remote = dict(&[("a", "b")]);

    let actual = run(&local, &remote, None, &[], &[]);

    check(&actual, &[], &[("a", "b")], &[], None);
}

#[test]
fn merge_when_the_entry_is_removed_in_remote_but_updated_in_local_and_a_new_entry_is_added_in_remote() {
    let base = dict(&[("a", "a"), ("b", "b")]);
    let local = dict(&[("a", "a"), ("b", "d")]);
    let remote = dict(&[("a", "a"), ("c", "c")]);

    let actual = run(&local, &remote, Some(&base), &[], &[]);

    check(&actual, &[("c", "c")], &[], &[], Some(&[("a", "a"), ("c", "c"), ("b", "d")]));
}

#[test]
fn merge_with_single_entry_and_local_is_empty() {
    let base = dict(&[("a", "a")]);
    let local = dict(&[]);
    let remote = dict(&[("a", "b")]);

    let actual = run(&local, &remote, Some(&base), &[], &[]);

    check(&actual, &[], &[], &[], Some(&[]));
}

#[test]
fn merge_when_local_and_remote_has_moved_forward_with_conflicts() {
    let base = dict(&[("a", "a")]);
    let local = dict(&[("a", "d")]);
    let remote = dict(&[("a", "b")]);

    let actual = run(&local, &remote, Some(&base), &[], &[]);

    check(&actual, &[], &[], &[], Some(&[("a", "d")]));
}

#[test]
fn merge_when_a_new_entry_is_added_to_remote_but_scoped_to_machine_locally_and_local_is_not_synced_yet() {
    let local = dict(&[("a", "a")]);
    let remote = dict(&[("b", "b"), ("a", "a")]);

    let actual = run(&local, &remote, None, &["b"], &[]);

    check(&actual, &[], &[], &[], None);
}

#[test]
fn merge_when_an_entry_is_updated_to_remote_but_scoped_to_machine_locally() {
    let local = dict(&[("a", "a")]);
    let remote = dict(&[("a", "b")]);

    let actual = run(&local, &remote, Some(&local), &["a"], &[]);

    check(&actual, &[], &[], &[], None);
}

#[test]
fn merge_when_a_local_value_is_removed_and_scoped_to_machine_locally() {
    let base = dict(&[("a", "a"), ("b", "b")]);
    let local = dict(&[("a", "a")]);
    let remote = dict(&[("b", "b"), ("a", "a")]);

    let actual = run(&local, &remote, Some(&base), &["b"], &[]);

    check(&actual, &[], &[], &[], Some(&[("a", "a")]));
}

#[test]
fn merge_when_local_moved_forwared_by_changing_a_key_to_machine_scope() {
    let base = dict(&[("a", "a"), ("b", "b")]);
    let remote = dict(&[("a", "a"), ("b", "b")]);
    let local = dict(&[("a", "a")]);

    let actual = run(&local, &remote, Some(&base), &["b"], &[]);

    check(&actual, &[], &[], &[], Some(&[("a", "a")]));
}

#[test]
fn merge_should_not_remove_remote_keys_if_not_registered() {
    let local = dict(&[("a", "a"), ("b", "b")]);
    let base = dict(&[("a", "a"), ("c", "c")]);
    let remote = dict(&[("a", "a"), ("c", "c")]);

    let actual = run(&local, &remote, Some(&base), &[], &["c"]);

    check(&actual, &[], &[], &[], Some(&[("a", "a"), ("b", "b"), ("c", "c")]));
}
