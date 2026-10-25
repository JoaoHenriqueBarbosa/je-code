//! Porte de `src/vs/base/test/node/snapshot.test.ts`.
//!
//! Notas de tradução:
//! - O `assertSnapshot` e o `SnapshotContext` são os de `je_base::snapshot`. O `setup` e o `teardown` globais do mocha são o
//!   [`begin_snapshot_test`], cujo guarda apaga os snapshots antigos ao fim de um teste que passou.
//! - Os `.snap` de referência são lidos (e, se faltarem, criados) em `ref/vscode/src/vs/base/test/node/__snapshots__`,
//!   a pasta do arquivo de teste original, sem nenhum caminho da máquina no repositório.
//! - `makeContext` é `SnapshotContext::with_snapshots_dir` sobre o diretório temporário do caso; `assertThrowsAsync` é
//!   `is_err()`. `Promises.readdir` não ordena, mas os snapshots registram a ordem alfabética, então a árvore é impressa
//!   ordenada.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::snapshot::{SnapshotContext, SnapshotOptions, SnapshotValue, assert_snapshot, begin_snapshot_test, new_object};
use je_base::uuid::generate_uuid;

const NODE_TEST_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../ref/vscode/src/vs/base/test/node");

/// O `testDir` do `setup` e o `Promises.rm` do `teardown`.
struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join("vsctests").join("snapshot").join(generate_uuid());
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn make_context(&self) -> SnapshotContext {
        SnapshotContext::with_snapshots_dir(self.0.clone(), "hello world!")
    }

    fn snapshot_file_tree(&self) {
        let mut tree = String::new();
        print_dir(&self.0, 0, &mut tree);
        assert_snapshot(&SnapshotValue::str(tree), None).unwrap();
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn print_dir(dir: &Path, indent: usize, out: &mut String) {
    let mut children: Vec<String> = fs::read_dir(dir).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect();
    children.sort();
    for child in children {
        let path = dir.join(&child);
        if path.is_file() {
            let content = fs::read_to_string(&path).unwrap();
            out.push_str(&format!("{}{child}:\n", " ".repeat(indent)));
            for line in content.split('\n') {
                out.push_str(&format!("{}{line}\n", " ".repeat(indent + 2)));
            }
        } else {
            out.push_str(&format!("{}{child}/\n", " ".repeat(indent)));
            print_dir(&path, indent + 2, out);
        }
    }
}

fn bool_object(key: &str, value: bool) -> SnapshotValue {
    SnapshotValue::object(vec![(key, SnapshotValue::Bool(value))])
}

fn number_object(key: &str, value: f64) -> SnapshotValue {
    SnapshotValue::object(vec![(key, SnapshotValue::Number(value))])
}

#[test]
fn creates_a_snapshot() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_dir = TestDir::new();
    let _snapshot = begin_snapshot_test(Path::new(NODE_TEST_DIR), "snapshot creates a snapshot");
    let ctx = test_dir.make_context();

    ctx.assert(&bool_object("cool", true), None).unwrap();
    test_dir.snapshot_file_tree();
}

#[test]
fn validates_a_snapshot() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_dir = TestDir::new();
    let _snapshot = begin_snapshot_test(Path::new(NODE_TEST_DIR), "snapshot validates a snapshot");

    let ctx1 = test_dir.make_context();
    ctx1.assert(&bool_object("cool", true), None).unwrap();

    let ctx2 = test_dir.make_context();
    // should pass:
    ctx2.assert(&bool_object("cool", true), None).unwrap();

    let ctx3 = test_dir.make_context();
    // should fail:
    assert!(ctx3.assert(&bool_object("cool", false), None).is_err());
}

#[test]
fn cleans_up_old_snapshots() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_dir = TestDir::new();
    let _snapshot = begin_snapshot_test(Path::new(NODE_TEST_DIR), "snapshot cleans up old snapshots");

    let ctx1 = test_dir.make_context();
    ctx1.assert(&bool_object("cool", true), None).unwrap();
    ctx1.assert(&bool_object("nifty", true), None).unwrap();
    ctx1.assert(&number_object("customName", 1.0), Some(&SnapshotOptions::named("thirdTest").with_extension("txt"))).unwrap();
    ctx1.assert(&number_object("customName", 2.0), Some(&SnapshotOptions::named("fourthTest"))).unwrap();

    test_dir.snapshot_file_tree();

    let ctx2 = test_dir.make_context();
    ctx2.assert(&bool_object("cool", true), None).unwrap();
    ctx2.assert(&number_object("customName", 1.0), Some(&SnapshotOptions::named("thirdTest"))).unwrap();
    ctx2.remove_old_snapshots().unwrap();

    test_dir.snapshot_file_tree();
}

#[test]
fn formats_object_nicely() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let _snapshot = begin_snapshot_test(Path::new(NODE_TEST_DIR), "snapshot formats object nicely");

    let circular = new_object(vec![]);
    circular.borrow_mut().push(("a".to_owned(), SnapshotValue::Object(circular.clone())));

    let value = SnapshotValue::array(vec![
        SnapshotValue::Number(1.0),
        SnapshotValue::Bool(true),
        SnapshotValue::Undefined,
        SnapshotValue::Null,
        SnapshotValue::BigInt(123),
        SnapshotValue::Symbol("heyo".to_owned()),
        SnapshotValue::str("hello"),
        SnapshotValue::object(vec![("hello", SnapshotValue::str("world"))]),
        SnapshotValue::Object(circular),
        SnapshotValue::map(vec![(SnapshotValue::str("hello"), SnapshotValue::Number(1.0)), (SnapshotValue::str("goodbye"), SnapshotValue::Number(2.0))]),
        SnapshotValue::set(vec![SnapshotValue::Number(1.0), SnapshotValue::Number(2.0), SnapshotValue::Number(3.0)]),
        SnapshotValue::Function("helloWorld".to_owned()),
        SnapshotValue::RegExp("/hello/g".to_owned()),
        SnapshotValue::array(vec![SnapshotValue::str("long string".repeat(10)); 10]),
        SnapshotValue::debug_description(|| "Range [1 -> 5]".to_owned()),
    ]);
    assert_snapshot(&value, None).unwrap();
}

#[test]
fn cleans_up_mismatch_artifacts_after_a_passing_assertion() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_dir = TestDir::new();
    let ctx = test_dir.make_context();
    let snapshot_name = "hello_world_.traffic.snap";
    fs::write(test_dir.0.join(snapshot_name), "{ cool: true }").unwrap();
    assert!(ctx.assert(&bool_object("cool", false), Some(&SnapshotOptions::named("traffic"))).is_err());
    ctx.assert(&bool_object("cool", true), Some(&SnapshotOptions::named("traffic"))).unwrap();
    ctx.remove_old_snapshots().unwrap();

    let names: Vec<String> = fs::read_dir(&test_dir.0).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, [snapshot_name]);
}
