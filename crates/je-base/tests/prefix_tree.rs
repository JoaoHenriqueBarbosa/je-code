//! Porte de `src/vs/base/test/common/prefixTree.test.ts` (suíte `WellDefinedPrefixTree`), caso a caso e na mesma ordem.
//!
//! O `ensureNoDisposablesAreLeakedInTestSuite` não entra, porque a árvore não cria `IDisposable`.

use je_base::prefix_tree::WellDefinedPrefixTree;

fn tree() -> WellDefinedPrefixTree<i32> {
    WellDefinedPrefixTree::new()
}

#[test]
fn find() {
    let mut tree = tree();
    let key1 = ["foo", "bar"];
    let key2 = ["foo", "baz"];
    tree.insert(key1, 42, None);
    tree.insert(key2, 43, None);
    assert_eq!(tree.find(key1), Some(&42));
    assert_eq!(tree.find(key2), Some(&43));
    assert_eq!(tree.find(["foo", "baz", "bop"]), None);
    assert_eq!(tree.find(["foo"]), None);
}

#[test]
fn has_parent_of_key() {
    let mut tree = tree();
    let key = ["foo", "bar"];
    tree.insert(key, 42, None);

    assert!(tree.has_key_or_parent(["foo", "bar", "baz"]));
    assert!(tree.has_key_or_parent(["foo", "bar"]));
    assert!(!tree.has_key_or_parent(["foo"]));
    assert!(!tree.has_key_or_parent(["baz"]));
}

#[test]
fn has_key_or_children() {
    let mut tree = tree();
    let key = ["foo", "bar"];
    tree.insert(key, 42, None);

    assert!(tree.has_key_or_children([] as [&str; 0]));
    assert!(tree.has_key_or_children(["foo"]));
    assert!(tree.has_key_or_children(["foo", "bar"]));
    assert!(!tree.has_key_or_children(["foo", "bar", "baz"]));
}

#[test]
fn has_key() {
    let mut tree = tree();
    let key = ["foo", "bar"];
    tree.insert(key, 42, None);

    assert!(tree.has_key(key));
    assert!(!tree.has_key(["foo"]));
    assert!(!tree.has_key(["baz"]));
    assert!(!tree.has_key(["foo", "bar", "baz"]));
}

#[test]
fn size() {
    let mut tree = tree();
    let key1 = ["foo", "bar"];
    let key2 = ["foo", "baz"];
    assert_eq!(tree.size(), 0);
    tree.insert(key1, 42, None);
    assert_eq!(tree.size(), 1);
    tree.insert(key2, 43, None);
    assert_eq!(tree.size(), 2);
    tree.insert(key2, 44, None);
    assert_eq!(tree.size(), 2);
}

#[test]
fn mutate() {
    let mut tree = tree();
    let key1 = ["foo", "bar"];
    let key2 = ["foo", "baz"];
    tree.insert(key1, 42, None);
    tree.insert(key2, 43, None);
    tree.mutate(key1, |value| {
        assert_eq!(value, Some(42));
        44
    });
    assert_eq!(tree.find(key1), Some(&44));
    assert_eq!(tree.find(key2), Some(&43));
}

#[test]
fn delete() {
    let mut tree = tree();
    let key1 = ["foo", "bar"];
    let key2 = ["foo", "baz"];
    tree.insert(key1, 42, None);
    tree.insert(key2, 43, None);
    assert_eq!(tree.size(), 2);

    assert_eq!(tree.delete(key1), Some(42));
    assert_eq!(tree.size(), 1);
    assert_eq!(tree.find(key1), None);
    assert_eq!(tree.find(key2), Some(&43));

    assert_eq!(tree.delete(key2), Some(43));
    assert_eq!(tree.size(), 0);
    assert_eq!(tree.find(key1), None);
    assert_eq!(tree.find(key2), None);

    tree.delete(key2);
    assert_eq!(tree.size(), 0);
}

#[test]
fn delete_child() {
    let mut tree = tree();
    let key1 = ["foo", "bar"];
    let key2 = ["foo", "bar", "baz"];
    tree.insert(key1, 42, None);
    tree.insert(key2, 43, None);
    assert_eq!(tree.size(), 2);

    assert_eq!(tree.delete(key2), Some(43));
    assert_eq!(tree.size(), 1);
    assert_eq!(tree.find(key1), Some(&42));
    assert_eq!(tree.find(key2), None);
}

#[test]
fn delete_noops_if_deleting_parent() {
    let mut tree = tree();
    let key1 = ["foo", "bar"];
    let key2 = ["foo", "bar", "baz"];
    tree.insert(key2, 43, None);
    assert_eq!(tree.size(), 1);

    assert_eq!(tree.delete(key1), None);
    assert_eq!(tree.size(), 1);
    assert_eq!(tree.find(key2), Some(&43));
    assert_eq!(tree.find(key1), None);
}

#[test]
fn values() {
    let mut tree = tree();
    let key1 = ["foo", "bar"];
    let key2 = ["foo", "baz"];
    tree.insert(key1, 42, None);
    tree.insert(key2, 43, None);

    assert_eq!(tree.values().copied().collect::<Vec<_>>(), vec![43, 42]);
}

#[test]
fn delete_recursive() {
    let mut tree = tree();
    let key1 = vec!["foo", "bar"];
    let key2 = vec!["foo", "bar", "baz"];
    let key3 = vec!["foo", "bar", "baz2", "baz3"];
    let key4 = vec!["foo", "bar2"];
    tree.insert(key1.clone(), 42, None);
    tree.insert(key2, 43, None);
    tree.insert(key3, 44, None);
    tree.insert(key4.clone(), 45, None);
    assert_eq!(tree.size(), 4);

    assert_eq!(tree.delete_recursive(key1.clone()).collect::<Vec<_>>(), vec![42, 44, 43]);
    assert_eq!(tree.size(), 1);

    assert_eq!(tree.delete_recursive(key1).collect::<Vec<_>>(), Vec::<i32>::new());
    assert_eq!(tree.size(), 1);

    assert_eq!(tree.delete_recursive(key4).collect::<Vec<_>>(), vec![45]);
    assert_eq!(tree.size(), 0);
}

#[test]
fn insert_and_delete_root() {
    let mut tree = tree();
    let root = [] as [&str; 0];
    assert_eq!(tree.size(), 0);
    tree.insert(root, 1234, None);
    assert_eq!(tree.size(), 1);
    assert_eq!(tree.find(root), Some(&1234));
    assert_eq!(tree.delete(root), Some(1234));
    assert_eq!(tree.size(), 0);

    assert_eq!(tree.find(root), None);
    assert_eq!(tree.delete(root), None);
    assert_eq!(tree.size(), 0);
}

#[test]
fn insert_and_delete_recursive_root() {
    let mut tree = tree();
    let root = [] as [&str; 0];
    assert_eq!(tree.size(), 0);
    tree.insert(root, 1234, None);
    tree.insert(["a"], 4567, None);
    assert_eq!(tree.size(), 2);
    assert_eq!(tree.find(root), Some(&1234));
    assert_eq!(tree.delete_recursive(root).collect::<Vec<_>>(), vec![1234, 4567]);
    assert_eq!(tree.size(), 0);

    assert_eq!(tree.find(root), None);
    assert_eq!(tree.delete_recursive(root).collect::<Vec<_>>(), Vec::<i32>::new());
    assert_eq!(tree.size(), 0);
}
