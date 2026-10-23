//! Porte de `src/vs/base/test/node/crypto.test.ts` (suíte `Crypto`), caso a caso e na mesma ordem.
//!
//! Notas de tradução: o `setup` cria um diretório aleatório no `tmpdir` e o `teardown` o remove (o `Drop` do
//! `TestDir`); `assert.rejects(..., /Hash mismatch/)` é o `Err` com essa mensagem.

mod common;

use std::path::PathBuf;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::node::crypto::checksum;
use je_base::uuid::generate_uuid;

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join("vsctests").join("crypto").join(generate_uuid());
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn checksum_test() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let test_dir = TestDir::new();
    let test_file = test_dir.0.join("checksum.txt");
    std::fs::write(&test_file, "Hello World").unwrap();

    checksum(&test_file, Some("a591a6d40bf420404a011733cfb7b190d62c65bf0bcda32b57b277d9ad9f146e")).unwrap();
}

#[test]
fn checksum_mismatch_rejects() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let test_dir = TestDir::new();
    let test_file = test_dir.0.join("checksum-mismatch.txt");
    std::fs::write(&test_file, "Hello World").unwrap();

    let error = checksum(&test_file, Some("wrong-hash")).expect_err("devia rejeitar");
    assert!(error.message.contains("Hash mismatch"), "{}", error.message);
}
