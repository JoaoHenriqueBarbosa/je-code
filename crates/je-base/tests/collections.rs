//! Porte de `src/vs/base/test/common/collections.test.ts` (suíte `Collections`), caso a caso e na mesma ordem.

use je_base::collections::{self, SetWithKey};

#[derive(Clone, Debug, PartialEq)]
struct Item {
    some_prop: String,
}

fn item(s: &str) -> Item {
    Item { some_prop: s.to_string() }
}

#[test]
fn group_by() {
    let (group1, group2) = ("a", "b");
    let (value1, value2, value3) = (1, 2, 3);

    #[derive(Clone)]
    struct Entry {
        key: &'static str,
        value: i32,
    }
    let source = [Entry { key: group1, value: value1 }, Entry { key: group1, value: value2 }, Entry { key: group2, value: value3 }];

    let grouped = collections::group_by(&source, |x| x.key);
    let get = |key: &str| grouped.iter().find(|(k, _)| *k == key).map(|(_, group)| group);

    // Group 1
    assert_eq!(get(group1).map(Vec::len), Some(2));
    assert_eq!(get(group1).unwrap()[0].value, value1);
    assert_eq!(get(group1).unwrap()[1].value, value2);

    // Group 2
    assert_eq!(get(group2).map(Vec::len), Some(1));
    assert_eq!(get(group2).unwrap()[0].value, value3);
}

#[test]
fn equal_sets() {
    assert!(collections::equal_sets::<&str>(&[], &[]));
    assert!(collections::equal_sets(&["a", "b"], &["b", "a"]));

    let same = ["a"];
    assert!(collections::equal_sets(&same, &same));

    assert!(!collections::equal_sets(&["a"], &["a", "b"]));
    assert!(!collections::equal_sets(&["a", "b"], &["a"]));
    assert!(!collections::equal_sets(&["a", "b"], &["a", "c"]));
}

fn set_with_key() -> SetWithKey<Item, String> {
    SetWithKey::new(["a", "b", "c"].map(item).to_vec(), |value: &Item| value.some_prop.clone())
}

fn initial_values() -> Vec<Item> {
    ["a", "b", "c"].map(item).to_vec()
}

#[test]
fn set_with_key_size() {
    let set = set_with_key();
    assert_eq!(set.size(), 3);
}

#[test]
fn set_with_key_add() {
    let mut set = set_with_key();
    set.add(item("d"));
    assert_eq!(set.size(), 4);
    assert!(set.has(&item("d")));
}

#[test]
fn set_with_key_delete() {
    let mut set = set_with_key();
    assert!(set.has(&item("b")));
    set.delete(&item("b"));
    assert_eq!(set.size(), 2);
    assert!(!set.has(&item("b")));
}

#[test]
fn set_with_key_has() {
    let set = set_with_key();
    assert!(set.has(&item("a")));
    assert!(set.has(&item("b")));
}

#[test]
fn set_with_key_entries() {
    let set = set_with_key();
    let entries: Vec<(Item, Item)> = set.entries().map(|(a, b)| (a.clone(), b.clone())).collect();
    let expected: Vec<(Item, Item)> = initial_values().into_iter().map(|value| (value.clone(), value)).collect();
    assert_eq!(entries, expected);
}

#[test]
fn set_with_key_keys_and_values() {
    let set = set_with_key();
    let keys: Vec<Item> = set.keys().cloned().collect();
    let values: Vec<Item> = set.values().cloned().collect();
    assert_eq!(keys, initial_values());
    assert_eq!(values, initial_values());
}

#[test]
fn set_with_key_clear() {
    let mut set = set_with_key();
    set.clear();
    assert_eq!(set.size(), 0);
}

#[test]
fn set_with_key_for_each() {
    let set = set_with_key();
    let mut values = Vec::new();
    set.for_each(|value, _| values.push(value.clone()));
    assert_eq!(values, initial_values());
}

#[test]
fn set_with_key_iterator() {
    let set = set_with_key();
    let mut values = Vec::new();
    for value in set.iter() {
        values.push(value.clone());
    }
    assert_eq!(values, initial_values());
}

#[test]
fn set_with_key_to_string_tag() {
    let set = set_with_key();
    assert_eq!(set.to_string_tag(), "SetWithKey");
}
