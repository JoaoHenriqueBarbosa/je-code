//! Porte de `src/vs/base/test/common/resourceTree.test.ts` (suíte `ResourceTree`), caso a caso e na mesma ordem.

use je_base::path::Platform;
use je_base::resource_tree::ResourceTree;
use je_base::uri::URI;

fn file(path: &str) -> URI {
    URI::file(path, Platform::Posix)
}

fn tree() -> ResourceTree<String, ()> {
    ResourceTree::with_defaults(())
}

#[test]
fn ctor() {
    let tree = tree();
    assert_eq!(tree.root.children_count(), 0);
}

#[test]
fn simple() {
    let tree = tree();

    tree.add(&file("/foo/bar.txt"), "bar contents".to_string());
    assert_eq!(tree.root.children_count(), 1);

    let foo = tree.root.get("foo").unwrap();
    assert_eq!(foo.children_count(), 1);

    let bar = foo.get("bar.txt").unwrap();
    assert_eq!(bar.element().as_deref(), Some("bar contents"));

    tree.add(&file("/hello.txt"), "hello contents".to_string());
    assert_eq!(tree.root.children_count(), 2);

    let mut hello = tree.root.get("hello.txt").unwrap();
    assert_eq!(hello.element().as_deref(), Some("hello contents"));

    tree.delete(&file("/foo/bar.txt"));
    assert_eq!(tree.root.children_count(), 1);
    hello = tree.root.get("hello.txt").unwrap();
    assert_eq!(hello.element().as_deref(), Some("hello contents"));
}

#[test]
fn folders_with_data() {
    let tree = tree();

    assert_eq!(tree.root.children_count(), 0);

    tree.add(&file("/foo"), "foo".to_string());
    assert_eq!(tree.root.children_count(), 1);
    assert_eq!(tree.root.get("foo").unwrap().element().as_deref(), Some("foo"));

    tree.add(&file("/bar"), "bar".to_string());
    assert_eq!(tree.root.children_count(), 2);
    assert_eq!(tree.root.get("bar").unwrap().element().as_deref(), Some("bar"));

    tree.add(&file("/foo/file.txt"), "file".to_string());
    assert_eq!(tree.root.children_count(), 2);
    assert_eq!(tree.root.get("foo").unwrap().element().as_deref(), Some("foo"));
    assert_eq!(tree.root.get("bar").unwrap().element().as_deref(), Some("bar"));
    assert_eq!(tree.root.get("foo").unwrap().get("file.txt").unwrap().element().as_deref(), Some("file"));

    tree.delete(&file("/foo"));
    assert_eq!(tree.root.children_count(), 1);
    assert!(tree.root.get("foo").is_none());
    assert_eq!(tree.root.get("bar").unwrap().element().as_deref(), Some("bar"));

    tree.delete(&file("/bar"));
    assert_eq!(tree.root.children_count(), 0);
    assert!(tree.root.get("foo").is_none());
    assert!(tree.root.get("bar").is_none());
}
