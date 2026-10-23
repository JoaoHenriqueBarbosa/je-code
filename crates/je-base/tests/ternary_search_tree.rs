//! Porte de `src/vs/base/test/common/ternarySearchtree.test.ts` (suíte `Ternary Search Tree`), caso a caso e na mesma ordem.
//!
//! Pendente: a suíte `TST, perf` do mesmo arquivo, que no original é `suite.skip` (cronometra com
//! `StopWatch` e só imprime tempos).
//!
//! O `any` do caso "set w/ undefined" é `Option<i32>`: o `undefined` do original é `None` e, por ser um
//! valor gravado, o `set` seguinte devolve `Some(None)` onde o JavaScript devolve `undefined`.

use std::fmt::Debug;
use std::rc::Rc;

use je_base::arrays;
use je_base::extpath::random_path;
use je_base::path::{FixedEnvironment, Platform};
use je_base::ternary_search_tree::{
    ConfigKeysIterator, KeyIterator, PathIterator, StringIterator, TernarySearchTree, UriIterator, UriPredicate,
};
use je_base::uri::{UriComponents, URI};

fn s(text: &str) -> String {
    text.to_string()
}

fn parse(value: &str) -> URI {
    URI::parse(value, false)
}

fn file(path: &str) -> URI {
    URI::file(path, Platform::Posix)
}

fn never() -> UriPredicate {
    Rc::new(|_| false)
}

fn always() -> UriPredicate {
    Rc::new(|_| true)
}

fn uri_tree<V>(ignore_path_casing: UriPredicate, ignore_query_and_fragment: UriPredicate) -> TernarySearchTree<URI, V> {
    TernarySearchTree::new(Box::new(UriIterator::new(ignore_path_casing, ignore_query_and_fragment)))
}

fn tree<V>(iter: impl KeyIterator<String> + 'static) -> TernarySearchTree<String, V> {
    TernarySearchTree::new(Box::new(iter))
}

#[test]
fn path_iterator() {
    let mut iter = PathIterator::default();
    iter.reset(&s("file:///usr/bin/file.txt"));

    assert_eq!(iter.value(), "file:");
    assert!(iter.has_next());
    assert_eq!(iter.cmp(&"file:".encode_utf16().collect::<Vec<_>>()), 0);
    let cmp = |iter: &PathIterator, a: &str| iter.cmp(&a.encode_utf16().collect::<Vec<_>>());
    assert!(cmp(&iter, "a") < 0);
    assert!(cmp(&iter, "aile:") < 0);
    assert!(cmp(&iter, "z") > 0);
    assert!(cmp(&iter, "zile:") > 0);

    iter.next();
    assert_eq!(iter.value(), "usr");
    assert!(iter.has_next());

    iter.next();
    assert_eq!(iter.value(), "bin");
    assert!(iter.has_next());

    iter.next();
    assert_eq!(iter.value(), "file.txt");
    assert!(!iter.has_next());

    iter.next();
    assert_eq!(iter.value(), "");
    assert!(!iter.has_next());
    iter.next();
    assert_eq!(iter.value(), "");
    assert!(!iter.has_next());

    //
    iter.reset(&s("/foo/bar/"));
    assert_eq!(iter.value(), "foo");
    assert!(iter.has_next());

    iter.next();
    assert_eq!(iter.value(), "bar");
    assert!(!iter.has_next());
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

#[test]
fn uri_iterator() {
    let mut iter = UriIterator::new(never(), never());
    iter.reset(&parse("file:///usr/bin/file.txt"));

    assert_eq!(iter.value(), "file");
    // assert.strictEqual(iter.cmp('FILE'), 0);
    assert_eq!(iter.cmp(&units("file")), 0);
    assert!(iter.has_next());
    iter.next();

    assert_eq!(iter.value(), "usr");
    assert!(iter.has_next());
    iter.next();

    assert_eq!(iter.value(), "bin");
    assert!(iter.has_next());
    iter.next();

    assert_eq!(iter.value(), "file.txt");
    assert!(!iter.has_next());

    iter.reset(&parse("file://share/usr/bin/file.txt?foo"));

    // scheme
    assert_eq!(iter.value(), "file");
    // assert.strictEqual(iter.cmp('FILE'), 0);
    assert_eq!(iter.cmp(&units("file")), 0);
    assert!(iter.has_next());
    iter.next();

    // authority
    assert_eq!(iter.value(), "share");
    assert_eq!(iter.cmp(&units("SHARe")), 0);
    assert!(iter.has_next());
    iter.next();

    // path
    assert_eq!(iter.value(), "usr");
    assert!(iter.has_next());
    iter.next();

    // path
    assert_eq!(iter.value(), "bin");
    assert!(iter.has_next());
    iter.next();

    // path
    assert_eq!(iter.value(), "file.txt");
    assert!(iter.has_next());
    iter.next();

    // query
    assert_eq!(iter.value(), "foo");
    assert!(iter.cmp(&units("z")) > 0);
    assert!(iter.cmp(&units("a")) < 0);
    assert!(!iter.has_next());
}

#[test]
fn uri_iterator_ignore_query_fragment() {
    let mut iter = UriIterator::new(never(), always());
    iter.reset(&parse("file:///usr/bin/file.txt"));

    assert_eq!(iter.value(), "file");
    // assert.strictEqual(iter.cmp('FILE'), 0);
    assert_eq!(iter.cmp(&units("file")), 0);
    assert!(iter.has_next());
    iter.next();

    assert_eq!(iter.value(), "usr");
    assert!(iter.has_next());
    iter.next();

    assert_eq!(iter.value(), "bin");
    assert!(iter.has_next());
    iter.next();

    assert_eq!(iter.value(), "file.txt");
    assert!(!iter.has_next());

    iter.reset(&parse("file://share/usr/bin/file.txt?foo"));

    // scheme
    assert_eq!(iter.value(), "file");
    // assert.strictEqual(iter.cmp('FILE'), 0);
    assert_eq!(iter.cmp(&units("file")), 0);
    assert!(iter.has_next());
    iter.next();

    // authority
    assert_eq!(iter.value(), "share");
    assert_eq!(iter.cmp(&units("SHARe")), 0);
    assert!(iter.has_next());
    iter.next();

    // path
    assert_eq!(iter.value(), "usr");
    assert!(iter.has_next());
    iter.next();

    // path
    assert_eq!(iter.value(), "bin");
    assert!(iter.has_next());
    iter.next();

    // path
    assert_eq!(iter.value(), "file.txt");
    assert!(!iter.has_next());
}

fn assert_tst_dfs<E: Clone + PartialEq + Debug>(trie: &TernarySearchTree<String, E>, elements: &[(&str, E)]) {
    assert!(trie.is_balanced(), "TST is not balanced");

    let mut i = 0;
    for (key, value) in trie.iter_entries() {
        let expected = &elements[i];
        i += 1;
        assert_eq!(key, expected.0);
        assert_eq!(value, expected.1);
    }

    assert_eq!(i, elements.len());

    // `new Map()` do original: a última gravação de cada chave vence.
    let mut map: Vec<(String, E)> = Vec::new();
    for (key, value) in elements {
        match map.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = value.clone(),
            None => map.push((key.to_string(), value.clone())),
        }
    }
    for (key, value) in &map {
        assert_eq!(trie.get(key), Some(value));
    }

    // forEach
    let mut for_each_count = 0;
    trie.for_each(|element, key| {
        assert_eq!(Some(element), map.iter().find(|(k, _)| k == key).map(|(_, v)| v));
        for_each_count += 1;
    });
    assert_eq!(map.len(), for_each_count);

    // iterator
    let mut iter_count = 0;
    for (key, value) in trie.iter_entries() {
        assert_eq!(Some(&value), map.iter().find(|(k, _)| *k == key).map(|(_, v)| v));
        iter_count += 1;
    }
    assert_eq!(map.len(), iter_count);
}

#[test]
fn ternary_search_tree_set() {
    let mut trie = TernarySearchTree::<String, i32>::for_strings();
    trie.set(s("foobar"), 1);
    trie.set(s("foobaz"), 2);

    assert_tst_dfs(&trie, &[("foobar", 1), ("foobaz", 2)]); // longer

    trie = TernarySearchTree::for_strings();
    trie.set(s("foobar"), 1);
    trie.set(s("fooba"), 2);
    assert_tst_dfs(&trie, &[("fooba", 2), ("foobar", 1)]); // shorter

    trie = TernarySearchTree::for_strings();
    trie.set(s("foo"), 1);
    trie.set(s("foo"), 2);
    assert_tst_dfs(&trie, &[("foo", 2)]);

    trie = TernarySearchTree::for_strings();
    trie.set(s("foo"), 1);
    trie.set(s("foobar"), 2);
    trie.set(s("bar"), 3);
    trie.set(s("foob"), 4);
    trie.set(s("bazz"), 5);

    assert_tst_dfs(&trie, &[("bar", 3), ("bazz", 5), ("foo", 1), ("foob", 4), ("foobar", 2)]);
}

#[test]
fn ternary_search_tree_set_with_undefined() {
    let mut trie = TernarySearchTree::<String, Option<i32>>::for_strings();
    trie.set(s("foobar"), None);
    trie.set(s("foobaz"), Some(2));

    assert_eq!(trie.get(&s("foobar")).copied().flatten(), None);
    assert_eq!(trie.get(&s("foobaz")).copied().flatten(), Some(2));
    assert_eq!(trie.get(&s("NOT HERE")).copied().flatten(), None);

    assert!(trie.has(&s("foobaz")));
    assert!(trie.has(&s("foobar")));
    assert!(!trie.has(&s("NOT HERE")));

    assert_tst_dfs(&trie, &[("foobar", None), ("foobaz", Some(2))]); // should check for undefined value

    let old_value = trie.set(s("foobar"), Some(3));
    assert_eq!(old_value.flatten(), None);
    assert_eq!(trie.get(&s("foobar")).copied().flatten(), Some(3));
}

#[test]
fn ternary_search_tree_find_longest_match() {
    let mut trie = TernarySearchTree::<String, i32>::for_strings();
    trie.set(s("foo"), 1);
    trie.set(s("foobar"), 2);
    trie.set(s("foobaz"), 3);
    assert_tst_dfs(&trie, &[("foo", 1), ("foobar", 2), ("foobaz", 3)]);

    assert_eq!(trie.find_substr(&s("f")), None);
    assert_eq!(trie.find_substr(&s("z")), None);
    assert_eq!(trie.find_substr(&s("foo")), Some(&1));
    assert_eq!(trie.find_substr(&s("fooö")), Some(&1));
    assert_eq!(trie.find_substr(&s("fooba")), Some(&1));
    assert_eq!(trie.find_substr(&s("foobarr")), Some(&2));
    assert_eq!(trie.find_substr(&s("foobazrr")), Some(&3));
}

#[test]
fn ternary_search_tree_basics() {
    let mut trie = tree::<i32>(StringIterator::new());

    trie.set(s("foo"), 1);
    trie.set(s("bar"), 2);
    trie.set(s("foobar"), 3);
    assert_tst_dfs(&trie, &[("bar", 2), ("foo", 1), ("foobar", 3)]);

    assert_eq!(trie.get(&s("foo")), Some(&1));
    assert_eq!(trie.get(&s("bar")), Some(&2));
    assert_eq!(trie.get(&s("foobar")), Some(&3));
    assert_eq!(trie.get(&s("foobaz")), None);
    assert_eq!(trie.get(&s("foobarr")), None);

    assert_eq!(trie.find_substr(&s("fo")), None);
    assert_eq!(trie.find_substr(&s("foo")), Some(&1));
    assert_eq!(trie.find_substr(&s("foooo")), Some(&1));

    trie.delete(&s("foobar"));
    trie.delete(&s("bar"));
    assert_eq!(trie.get(&s("foobar")), None);
    assert_eq!(trie.get(&s("bar")), None);

    trie.set(s("foobar"), 17);
    trie.set(s("barr"), 18);
    assert_eq!(trie.get(&s("foobar")), Some(&17));
    assert_eq!(trie.get(&s("barr")), Some(&18));
    assert_eq!(trie.get(&s("bar")), None);
}

#[test]
fn ternary_search_tree_delete_and_cleanup() {
    // normal delete
    let mut trie = tree::<i32>(StringIterator::new());
    trie.set(s("foo"), 1);
    trie.set(s("foobar"), 2);
    trie.set(s("bar"), 3);
    assert_tst_dfs(&trie, &[("bar", 3), ("foo", 1), ("foobar", 2)]);
    trie.delete(&s("foo"));
    assert_tst_dfs(&trie, &[("bar", 3), ("foobar", 2)]);
    trie.delete(&s("foobar"));
    assert_tst_dfs(&trie, &[("bar", 3)]);

    // superstr-delete
    trie = tree::<i32>(StringIterator::new());
    trie.set(s("foo"), 1);
    trie.set(s("foobar"), 2);
    trie.set(s("bar"), 3);
    trie.set(s("foobarbaz"), 4);
    trie.delete_superstr(&s("foo"));
    assert_tst_dfs(&trie, &[("bar", 3), ("foo", 1)]);

    trie = tree::<i32>(StringIterator::new());
    trie.set(s("foo"), 1);
    trie.set(s("foobar"), 2);
    trie.set(s("bar"), 3);
    trie.set(s("foobarbaz"), 4);
    trie.delete_superstr(&s("fo"));
    assert_tst_dfs(&trie, &[("bar", 3)]);

    // trie = new TernarySearchTree<string, number>(new StringIterator());
    // trie.set('foo', 1);
    // trie.set('foobar', 2);
    // trie.set('bar', 3);
    // trie.deleteSuperStr('f');
    // assertTernarySearchTree(trie, ['bar', 3]);
}

#[test]
fn ternary_search_tree_path_segments_basics() {
    let mut trie = tree::<i32>(PathIterator::default());

    trie.set(s("/user/foo/bar"), 1);
    trie.set(s("/user/foo"), 2);
    trie.set(s("/user/foo/flip/flop"), 3);

    assert_eq!(trie.get(&s("/user/foo/bar")), Some(&1));
    assert_eq!(trie.get(&s("/user/foo")), Some(&2));
    assert_eq!(trie.get(&s("/user//foo")), Some(&2));
    assert_eq!(trie.get(&s("/user\\foo")), Some(&2));
    assert_eq!(trie.get(&s("/user/foo/flip/flop")), Some(&3));

    assert_eq!(trie.find_substr(&s("/user/bar")), None);
    assert_eq!(trie.find_substr(&s("/user/foo")), Some(&2));
    assert_eq!(trie.find_substr(&s("\\user\\foo")), Some(&2));
    assert_eq!(trie.find_substr(&s("/user//foo")), Some(&2));
    assert_eq!(trie.find_substr(&s("/user/foo/ba")), Some(&2));
    assert_eq!(trie.find_substr(&s("/user/foo/far/boo")), Some(&2));
    assert_eq!(trie.find_substr(&s("/user/foo/bar")), Some(&1));
    assert_eq!(trie.find_substr(&s("/user/foo/bar/far/boo")), Some(&1));
}

#[test]
fn ternary_search_tree_avl_set() {
    {
        // rotate left
        let mut trie = tree::<i32>(PathIterator::default());
        trie.set(s("/fileA"), 1);
        trie.set(s("/fileB"), 2);
        trie.set(s("/fileC"), 3);
        assert_tst_dfs(&trie, &[("/fileA", 1), ("/fileB", 2), ("/fileC", 3)]);
    }

    {
        // rotate left (inside middle)
        let mut trie = tree::<i32>(PathIterator::default());
        trie.set(s("/foo/fileA"), 1);
        trie.set(s("/foo/fileB"), 2);
        trie.set(s("/foo/fileC"), 3);
        assert_tst_dfs(&trie, &[("/foo/fileA", 1), ("/foo/fileB", 2), ("/foo/fileC", 3)]);
    }

    {
        // rotate right
        let mut trie = tree::<i32>(PathIterator::default());
        trie.set(s("/fileC"), 3);
        trie.set(s("/fileB"), 2);
        trie.set(s("/fileA"), 1);
        assert_tst_dfs(&trie, &[("/fileA", 1), ("/fileB", 2), ("/fileC", 3)]);
    }

    {
        // rotate right (inside middle)
        let mut trie = tree::<i32>(PathIterator::default());
        trie.set(s("/mid/fileC"), 3);
        trie.set(s("/mid/fileB"), 2);
        trie.set(s("/mid/fileA"), 1);
        assert_tst_dfs(&trie, &[("/mid/fileA", 1), ("/mid/fileB", 2), ("/mid/fileC", 3)]);
    }

    {
        // rotate right, left
        let mut trie = tree::<i32>(PathIterator::default());
        trie.set(s("/fileD"), 7);
        trie.set(s("/fileB"), 2);
        trie.set(s("/fileG"), 42);
        trie.set(s("/fileF"), 24);
        trie.set(s("/fileZ"), 73);
        trie.set(s("/fileE"), 15);
        assert_tst_dfs(&trie, &[("/fileB", 2), ("/fileD", 7), ("/fileE", 15), ("/fileF", 24), ("/fileG", 42), ("/fileZ", 73)]);
    }

    {
        // rotate left, right
        let mut trie = tree::<i32>(PathIterator::default());
        trie.set(s("/fileJ"), 42);
        trie.set(s("/fileZ"), 73);
        trie.set(s("/fileE"), 15);
        trie.set(s("/fileB"), 2);
        trie.set(s("/fileF"), 7);
        trie.set(s("/fileG"), 1);
        assert_tst_dfs(&trie, &[("/fileB", 2), ("/fileE", 15), ("/fileF", 7), ("/fileG", 1), ("/fileJ", 42), ("/fileZ", 73)]);
    }
}

#[test]
fn ternary_search_tree_bst_delete() {
    let mut trie = tree::<i32>(StringIterator::new());

    // delete root
    trie.set(s("d"), 1);
    assert_tst_dfs(&trie, &[("d", 1)]);
    trie.delete(&s("d"));
    assert_tst_dfs(&trie, &[]);

    // delete node with two element
    trie.clear();
    trie.set(s("d"), 1);
    trie.set(s("b"), 1);
    trie.set(s("f"), 1);
    assert_tst_dfs(&trie, &[("b", 1), ("d", 1), ("f", 1)]);
    trie.delete(&s("d"));
    assert_tst_dfs(&trie, &[("b", 1), ("f", 1)]);

    // single child node
    trie.clear();
    trie.set(s("d"), 1);
    trie.set(s("b"), 1);
    trie.set(s("f"), 1);
    trie.set(s("e"), 1);
    assert_tst_dfs(&trie, &[("b", 1), ("d", 1), ("e", 1), ("f", 1)]);
    trie.delete(&s("f"));
    assert_tst_dfs(&trie, &[("b", 1), ("d", 1), ("e", 1)]);
}

#[test]
fn ternary_search_tree_avl_delete() {
    let mut trie = tree::<i32>(StringIterator::new());

    trie.clear();
    trie.set(s("d"), 1);
    trie.set(s("b"), 1);
    trie.set(s("f"), 1);
    trie.set(s("e"), 1);
    trie.set(s("z"), 1);
    assert_tst_dfs(&trie, &[("b", 1), ("d", 1), ("e", 1), ("f", 1), ("z", 1)]);

    // right, right
    trie.delete(&s("b"));
    assert_tst_dfs(&trie, &[("d", 1), ("e", 1), ("f", 1), ("z", 1)]);

    trie.clear();
    trie.set(s("d"), 1);
    trie.set(s("c"), 1);
    trie.set(s("f"), 1);
    trie.set(s("a"), 1);
    trie.set(s("b"), 1);
    assert_tst_dfs(&trie, &[("a", 1), ("b", 1), ("c", 1), ("d", 1), ("f", 1)]);

    // left, left
    trie.delete(&s("f"));
    assert_tst_dfs(&trie, &[("a", 1), ("b", 1), ("c", 1), ("d", 1)]);

    // mid
    trie.clear();
    trie.set(s("a"), 1);
    trie.set(s("ad"), 1);
    trie.set(s("ab"), 1);
    trie.set(s("af"), 1);
    trie.set(s("ae"), 1);
    trie.set(s("az"), 1);
    assert_tst_dfs(&trie, &[("a", 1), ("ab", 1), ("ad", 1), ("ae", 1), ("af", 1), ("az", 1)]);

    trie.delete(&s("ab"));
    assert_tst_dfs(&trie, &[("a", 1), ("ad", 1), ("ae", 1), ("af", 1), ("az", 1)]);

    trie.delete(&s("a"));
    assert_tst_dfs(&trie, &[("ad", 1), ("ae", 1), ("af", 1), ("az", 1)]);
}

#[test]
fn ternary_search_tree_cannot_read_property_1_of_undefined_138284() {
    let keys = [parse("fake-fs:/C"), parse("fake-fs:/A"), parse("fake-fs:/D"), parse("fake-fs:/B")];

    let mut tst = TernarySearchTree::<URI, bool>::for_uris(never(), never());
    for item in &keys {
        tst.set(item.clone(), true);
    }

    assert!(tst.is_balanced());
    tst.delete(&keys[0]);
    assert!(tst.is_balanced());
}

#[test]
fn ternary_search_tree_cannot_read_property_1_of_undefined_138284_simple() {
    let keys = ["C", "A", "D", "B"];
    let mut tst = TernarySearchTree::<String, bool>::for_strings();
    for item in keys {
        tst.set(s(item), true);
    }
    assert_tst_dfs(&tst, &[("A", true), ("B", true), ("C", true), ("D", true)]);

    tst.delete(&s(keys[0]));
    assert_tst_dfs(&tst, &[("A", true), ("B", true), ("D", true)]);

    {
        let mut tst = TernarySearchTree::<String, bool>::for_strings();
        tst.set(s("C"), true);
        tst.set(s("A"), true);
        tst.set(s("B"), true);
        assert_tst_dfs(&tst, &[("A", true), ("B", true), ("C", true)]);
    }
}

fn join_uris(keys: &[URI]) -> String {
    keys.iter().map(|k| k.to_string(false).to_string_lossy()).collect::<Vec<_>>().join(",")
}

#[test]
fn ternary_search_tree_cannot_read_property_1_of_undefined_138284_random() {
    let env = FixedEnvironment::new(Platform::Posix, "/workspace/project");
    for _round in (0..=10).rev() {
        let mut keys: Vec<URI> = Vec::new();
        for _ in 0..100 {
            keys.push(URI::from(
                &UriComponents {
                    scheme: Some("fake-fs".into()),
                    path: Some(random_path(None, None, 10, &env).into()),
                    ..UriComponents::default()
                },
                false,
            ));
        }
        let mut tst = TernarySearchTree::<URI, bool>::for_uris(never(), never());

        for item in &keys {
            tst.set(item.clone(), true);
            assert!(tst.is_balanced(), "SET{:?}|{}", item, join_uris(&keys));
        }

        for item in &keys {
            tst.delete(item);
            assert!(tst.is_balanced(), "DEL{:?}|{}", item, join_uris(&keys));
        }
    }
}

#[test]
fn https_github_com_microsoft_vscode_issues_227147() {
    let raw = "fake-fs:CAOnRvUuxO,fake-fs:1qcbfq54rg,fake-fs:UtDstYUQ56,fake-fs:d5ktqDysll,fake-fs:w5NSAKA4Ch,fake-fs:QcIIIY6WHX,fake-fs:WCedQu9Ogd,fake-fs:cKUC5LunBr,fake-fs:XrIIYjI3HB,fake-fs:xgTkoneFzF,fake-fs:QYkCVx2nYC,fake-fs:ePrIDEKEpJ,fake-fs:nrOPYCW81a,fake-fs:MQbkFLcDsA,fake-fs:wXG8YiOrBI,fake-fs:4tHTWi240D,fake-fs:5uQWjgZGGJ,fake-fs:famP6pZXyx,fake-fs:aB9sUhwP1J,fake-fs:DlS0CssyhG,fake-fs:9vK2k3rL2V,fake-fs:iqWeu7zF6t,fake-fs:8vC6bQX2WH,fake-fs:nFILXMQTRg,fake-fs:miiV72aajE,fake-fs:9VRbqvaw0q,fake-fs:WnEHS1arfZ,fake-fs:Fco75PJ5pM,fake-fs:6CsEpoZ7VW,fake-fs:B2PrCtDpWu,fake-fs:y8Hi94Oekg,fake-fs:wyEjPNa5lo,fake-fs:zw1Ljv0erc,fake-fs:y4KWPUOMx0,fake-fs:1basrPTlTp,fake-fs:5iErr4YM34,fake-fs:Q2TQaujh8Q,fake-fs:QxcYzNNxZw,fake-fs:3QUDHjU55a,fake-fs:23ymf9ggMV,fake-fs:qQhuKFdy29,fake-fs:JuwmxA33oJ,fake-fs:NQeUyfMNUo,fake-fs:2Vo3eR1jxM,fake-fs:NzUXQidwel,fake-fs:aESYKGPxIx,fake-fs:mxLdeJartN,fake-fs:PhSd2xLwVe,fake-fs:9nmWjUUMRz,fake-fs:Wc6a4RsGhn,fake-fs:5a0AlFHALQ,fake-fs:Q93jnNZBxJ,fake-fs:4CuVkbfPSG,fake-fs:mdFlJ7WQva,fake-fs:fgVsaRm1KG,fake-fs:P7UXWiRJYj,fake-fs:q6nz5Q9BEW,fake-fs:1UZmGkvNTn,fake-fs:AKY8cnUQFl,fake-fs:RezYuPU7FD,fake-fs:5zaYc72Bit,fake-fs:yh8FTxFfQq,fake-fs:ayNPgEuc2q,fake-fs:EdOb27cRhF,fake-fs:h4c2uNyI4l,fake-fs:BhzOLNL4JO,fake-fs:HVPTdAMWpS,fake-fs:7K7IlacaZe,fake-fs:iUKJonC5eq,fake-fs:Y9E3NX3eJD,fake-fs:66h80uK32I,fake-fs:gFXpry1Y09,fake-fs:qOqvvXPcu4,fake-fs:UbbLn2NFSJ,fake-fs:TzJ07HsAGz,fake-fs:nQngmvgx4m,fake-fs:6bZQCR8epb,fake-fs:xb3SJKX1bi,fake-fs:GF3DPK4zDj,fake-fs:HmxgAqEegt,fake-fs:yT2OAMQYal,fake-fs:MiVX4VYXHk,fake-fs:QMbsUbjJTI,fake-fs:KzAbDNsmPc,fake-fs:m6CGOwOcdT,fake-fs:0cyHx9zsA3,fake-fs:SIwjWfFLSY,fake-fs:uZSDXCEqLY,fake-fs:HuoTL3nK7k,fake-fs:oyoejYE0CI,fake-fs:56WLhiCxbz,fake-fs:SqYOi0z5sM,fake-fs:LZq3ei28Ez,fake-fs:pTc4pCtwk8,fake-fs:AAJSFf0RHS,fake-fs:up6EHkEbO9,fake-fs:GB1Pesdnxd,fake-fs:Oyvq4Z96S4,fake-fs:rYXrhklgf6,fake-fs:g1HdUkQziH";
    let keys: Vec<URI> = raw.split(',').map(|value| URI::parse(value, true)).collect();

    let mut tst = TernarySearchTree::<URI, bool>::for_uris(never(), never());
    for item in &keys {
        tst.set(item.clone(), true);
        assert!(tst.is_balanced(), "SET{:?}|{}", item, join_uris(&keys));
    }

    let length_now = tst.iter_entries().count();
    assert_eq!(length_now, keys.len());

    let mut keys2 = keys.clone();

    for (index, item) in keys.iter().enumerate() {
        tst.delete(item);
        assert!(tst.is_balanced(), "DEL{:?}|{}", item, join_uris(&keys));

        let idx = keys2.iter().position(|k| k == item);
        assert!(idx.is_some());
        keys2.remove(idx.unwrap());

        let actual_keys: Vec<URI> = tst.iter_entries().map(|(key, _)| key).collect();

        assert_eq!(
            actual_keys.len(),
            keys2.len(),
            "FAILED with {} -> {}\nWANTED:{}\nACTUAL:{}",
            index,
            item.to_string(false),
            join_uris(&keys2),
            join_uris(&actual_keys)
        );
    }

    assert_eq!(tst.iter_entries().count(), 0);
}

const CONFIG_RAW: &str = "config.debug.toolBarLocation,floating,config.editor.renderControlCharacters,true,config.editor.renderWhitespace,selection,config.files.autoSave,off,config.git.enabled,true,config.notebook.globalToolbar,true,config.terminal.integrated.tabs.enabled,true,config.terminal.integrated.tabs.showActions,singleTerminalOrNarrow,config.terminal.integrated.tabs.showActiveTerminal,singleTerminalOrNarrow,config.workbench.activityBar.visible,true,config.workbench.experimental.settingsProfiles.enabled,true,config.workbench.layoutControl.type,both,config.workbench.sideBar.location,left,config.workbench.statusBar.visible,true";

fn config_tuples() -> Vec<(String, String)> {
    let array: Vec<&str> = CONFIG_RAW.split(',').collect();
    array.chunks(2).map(|pair| (pair[0].to_string(), pair[1].to_string())).collect()
}

fn join_entries(map: &TernarySearchTree<String, String>) -> String {
    map.iter_entries().map(|(key, value)| format!("{key},{value}")).collect::<Vec<_>>().join(",")
}

#[test]
fn ternary_search_tree_cannot_read_properties_of_undefined_reading_length_161618_simple() {
    let tuples = config_tuples();

    let mut map = TernarySearchTree::<String, String>::for_config_keys();
    map.fill(&tuples);

    assert_eq!(join_entries(&map), CONFIG_RAW);
    assert!(map.has(&s("config.editor.renderWhitespace")));

    let len = map.iter_entries().count();
    map.delete(&s("config.editor.renderWhitespace"));
    assert!(map.is_balanced());
    assert_eq!(map.iter_entries().count(), len - 1);
}

#[test]
fn ternary_search_tree_cannot_read_properties_of_undefined_reading_length_161618_random() {
    let mut tuples = config_tuples();

    for _round in (0..=100).rev() {
        arrays::shuffle(&mut tuples, None);
        let mut map = TernarySearchTree::<String, String>::for_config_keys();
        map.fill(&tuples);

        assert_eq!(join_entries(&map), CONFIG_RAW);
        assert!(map.has(&s("config.editor.renderWhitespace")));

        let len = map.iter_entries().count();
        map.delete(&s("config.editor.renderWhitespace"));
        assert!(map.is_balanced());
        assert_eq!(map.iter_entries().count(), len - 1);
    }
}

#[test]
fn ternary_search_tree_path_segments_lookup() {
    let mut map = tree::<i32>(PathIterator::default());
    map.set(s("/user/foo/bar"), 1);
    map.set(s("/user/foo"), 2);
    map.set(s("/user/foo/flip/flop"), 3);

    assert_eq!(map.get(&s("/foo")), None);
    assert_eq!(map.get(&s("/user")), None);
    assert_eq!(map.get(&s("/user/foo")), Some(&2));
    assert_eq!(map.get(&s("/user/foo/bar")), Some(&1));
    assert_eq!(map.get(&s("/user/foo/bar/boo")), None);
}

fn assert_superstr_values<K: Clone, V: Clone + PartialEq + Debug>(
    map: &TernarySearchTree<K, V>,
    key: &K,
    expected: &[V],
) {
    let mut iter = map.find_superstr(key).expect("findSuperstr devolve um iterador");
    for value in expected {
        let item = iter.next().expect("done: false");
        assert_eq!(&item.1, value);
    }
    assert!(iter.next().is_none()); // value: undefined, done: true
}

#[test]
fn ternary_search_tree_path_segments_superstr() {
    let mut map = tree::<i32>(PathIterator::default());
    map.set(s("/user/foo/bar"), 1);
    map.set(s("/user/foo"), 2);
    map.set(s("/user/foo/flip/flop"), 3);
    map.set(s("/usr/foo"), 4);

    assert_superstr_values(&map, &s("/user"), &[2, 1, 3]);
    assert_superstr_values(&map, &s("/usr"), &[4]);

    assert!(map.find_superstr(&s("/not")).is_none());
    assert!(map.find_superstr(&s("/us")).is_none());
    assert!(map.find_superstr(&s("/usrr")).is_none());
    assert!(map.find_superstr(&s("/userr")).is_none());
}

#[test]
fn ternary_search_tree_path_segments_delete_superstr() {
    let mut map = tree::<i32>(PathIterator::default());
    map.set(s("/user/foo/bar"), 1);
    map.set(s("/user/foo"), 2);
    map.set(s("/user/foo/flip/flop"), 3);
    map.set(s("/usr/foo"), 4);

    assert_tst_dfs(&map, &[("/user/foo", 2), ("/user/foo/bar", 1), ("/user/foo/flip/flop", 3), ("/usr/foo", 4)]);

    // not a segment
    map.delete_superstr(&s("/user/fo"));
    assert_tst_dfs(&map, &[("/user/foo", 2), ("/user/foo/bar", 1), ("/user/foo/flip/flop", 3), ("/usr/foo", 4)]);

    // delete a segment
    map.set(s("/user/foo/bar"), 1);
    map.set(s("/user/foo"), 2);
    map.set(s("/user/foo/flip/flop"), 3);
    map.set(s("/usr/foo"), 4);
    map.delete_superstr(&s("/user/foo"));
    assert_tst_dfs(&map, &[("/user/foo", 2), ("/usr/foo", 4)]);
}

#[test]
fn ternary_search_tree_uri_basics() {
    let mut trie = uri_tree::<i32>(never(), never());

    trie.set(file("/user/foo/bar"), 1);
    trie.set(file("/user/foo"), 2);
    trie.set(file("/user/foo/flip/flop"), 3);

    assert_eq!(trie.get(&file("/user/foo/bar")), Some(&1));
    assert_eq!(trie.get(&file("/user/foo")), Some(&2));
    assert_eq!(trie.get(&file("/user/foo/flip/flop")), Some(&3));

    assert_eq!(trie.find_substr(&file("/user/bar")), None);
    assert_eq!(trie.find_substr(&file("/user/foo")), Some(&2));
    assert_eq!(trie.find_substr(&file("/user/foo/ba")), Some(&2));
    assert_eq!(trie.find_substr(&file("/user/foo/far/boo")), Some(&2));
    assert_eq!(trie.find_substr(&file("/user/foo/bar")), Some(&1));
    assert_eq!(trie.find_substr(&file("/user/foo/bar/far/boo")), Some(&1));
}

#[test]
fn ternary_search_tree_uri_query_parameters() {
    let mut trie = uri_tree::<i32>(never(), always());
    let root = parse("memfs:/?param=1");
    trie.set(root, 1);

    assert_eq!(trie.get(&parse("memfs:/?param=1")), Some(&1));

    assert_eq!(trie.find_substr(&parse("memfs:/?param=1")), Some(&1));
    assert_eq!(trie.find_substr(&parse("memfs:/aaa?param=1")), Some(&1));
}

#[test]
fn ternary_search_tree_uri_lookup() {
    let mut map = uri_tree::<i32>(never(), never());
    map.set(parse("http://foo.bar/user/foo/bar"), 1);
    map.set(parse("http://foo.bar/user/foo?query"), 2);
    map.set(parse("http://foo.bar/user/foo?QUERY"), 3);
    map.set(parse("http://foo.bar/user/foo/flip/flop"), 3);

    assert_eq!(map.get(&parse("http://foo.bar/foo")), None);
    assert_eq!(map.get(&parse("http://foo.bar/user")), None);
    assert_eq!(map.get(&parse("http://foo.bar/user/foo/bar")), Some(&1));
    assert_eq!(map.get(&parse("http://foo.bar/user/foo?query")), Some(&2));
    assert_eq!(map.get(&parse("http://foo.bar/user/foo?Query")), None);
    assert_eq!(map.get(&parse("http://foo.bar/user/foo?QUERY")), Some(&3));
    assert_eq!(map.get(&parse("http://foo.bar/user/foo/bar/boo")), None);
}

#[test]
fn ternary_search_tree_uri_lookup_casing() {
    // `/^https?$/.test(uri.scheme)`
    let mut map = uri_tree::<i32>(Rc::new(|uri| uri.scheme() == "http" || uri.scheme() == "https"), never());
    map.set(parse("http://foo.bar/user/foo/bar"), 1);
    assert_eq!(map.get(&parse("http://foo.bar/USER/foo/bar")), Some(&1));

    map.set(parse("foo://foo.bar/user/foo/bar"), 1);
    assert_eq!(map.get(&parse("foo://foo.bar/USER/foo/bar")), None);
}

#[test]
fn ternary_search_tree_uri_superstr() {
    let mut map = uri_tree::<i32>(never(), never());
    map.set(file("/user/foo/bar"), 1);
    map.set(file("/user/foo"), 2);
    map.set(file("/user/foo/flip/flop"), 3);
    map.set(file("/usr/foo"), 4);

    assert_superstr_values(&map, &file("/user"), &[2, 1, 3]);
    assert_superstr_values(&map, &file("/usr"), &[4]);
    assert_superstr_values(&map, &file("/"), &[2, 1, 3, 4]);

    assert!(map.find_superstr(&file("/not")).is_none());
    assert!(map.find_superstr(&file("/us")).is_none());
    assert!(map.find_superstr(&file("/usrr")).is_none());
    assert!(map.find_superstr(&file("/userr")).is_none());
}

#[test]
fn ternary_search_tree_config_key_segments_basics() {
    let mut trie = tree::<i32>(ConfigKeysIterator::default());

    trie.set(s("config.foo.bar"), 1);
    trie.set(s("config.foo"), 2);
    trie.set(s("config.foo.flip.flop"), 3);

    assert_eq!(trie.get(&s("config.foo.bar")), Some(&1));
    assert_eq!(trie.get(&s("config.foo")), Some(&2));
    assert_eq!(trie.get(&s("config.foo.flip.flop")), Some(&3));

    assert_eq!(trie.find_substr(&s("config.bar")), None);
    assert_eq!(trie.find_substr(&s("config.foo")), Some(&2));
    assert_eq!(trie.find_substr(&s("config.foo.ba")), Some(&2));
    assert_eq!(trie.find_substr(&s("config.foo.far.boo")), Some(&2));
    assert_eq!(trie.find_substr(&s("config.foo.bar")), Some(&1));
    assert_eq!(trie.find_substr(&s("config.foo.bar.far.boo")), Some(&1));
}

#[test]
fn ternary_search_tree_config_key_segments_lookup() {
    let mut map = tree::<i32>(ConfigKeysIterator::default());
    map.set(s("config.foo.bar"), 1);
    map.set(s("config.foo"), 2);
    map.set(s("config.foo.flip.flop"), 3);

    assert_eq!(map.get(&s("foo")), None);
    assert_eq!(map.get(&s("config")), None);
    assert_eq!(map.get(&s("config.foo")), Some(&2));
    assert_eq!(map.get(&s("config.foo.bar")), Some(&1));
    assert_eq!(map.get(&s("config.foo.bar.boo")), None);
}

#[test]
fn ternary_search_tree_config_key_segments_superstr() {
    let mut map = tree::<i32>(ConfigKeysIterator::default());
    map.set(s("config.foo.bar"), 1);
    map.set(s("config.foo"), 2);
    map.set(s("config.foo.flip.flop"), 3);
    map.set(s("boo"), 4);

    assert_superstr_values(&map, &s("config"), &[2, 1, 3]);

    assert!(map.find_superstr(&s("foo")).is_none());
    assert!(map.find_superstr(&s("config.foo.no")).is_none());
    assert!(map.find_superstr(&s("config.foop")).is_none());
}

#[test]
fn ternary_search_tree_config_key_segments_delete_superstr() {
    let mut map = tree::<i32>(ConfigKeysIterator::default());
    map.set(s("config.foo.bar"), 1);
    map.set(s("config.foo"), 2);
    map.set(s("config.foo.flip.flop"), 3);
    map.set(s("boo"), 4);

    assert_tst_dfs(&map, &[("boo", 4), ("config.foo", 2), ("config.foo.bar", 1), ("config.foo.flip.flop", 3)]);

    // not a segment
    map.delete_superstr(&s("config.fo"));
    assert_tst_dfs(&map, &[("boo", 4), ("config.foo", 2), ("config.foo.bar", 1), ("config.foo.flip.flop", 3)]);

    // delete a segment
    map.set(s("config.foo.bar"), 1);
    map.set(s("config.foo"), 2);
    map.set(s("config.foo.flip.flop"), 3);
    map.set(s("config.boo"), 4);
    map.delete_superstr(&s("config.foo"));
    assert_tst_dfs(&map, &[("boo", 4), ("config.foo", 2)]);
}

#[test]
fn tst_fill() {
    let mut tst = TernarySearchTree::<String, bool>::for_strings();

    let keys = [s("foo"), s("bar"), s("bang"), s("bazz")];
    tst.fill_keys(true, &keys);

    for key in &keys {
        assert_eq!(tst.get(key), Some(&true), "{key}");
    }
}
