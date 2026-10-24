//! Porte de `src/vs/server/test/node/serverMain.test.ts` (suíte `server.main directory creation`), caso a caso e na mesma
//! ordem.
//!
//! Notas de tradução:
//! - O original não chama código de produção: simula o que o `server.main.ts` faz (criar diretórios com `recursive` e modo
//!   `0o700`) direto com o `fs`. O porte faz o mesmo com `std::fs::DirBuilder` (`recursive(true)` e, no Unix, `mode(0o700)`).
//! - **`getRandomTestPath(os.tmpdir(), 'vsctests', ...)` e o `rmSync` do `finally`** são um diretório do `tempfile`, apagado
//!   ao sair do escopo (também quando o caso falha).
//! - **A verificação de permissões** (só em sistemas tipo Unix) é `#[cfg(unix)]`.

use std::fs;
use std::path::Path;

fn make_dir_recursive_with_mode(path: &Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

#[test]
fn should_create_nested_directories_with_recursive_option() {
    let test_dir = tempfile::tempdir().unwrap().keep();
    let test_dir = test_dir.join("server-main-dirs");
    let nested_path = test_dir.join("parent").join("child").join("extensions");
    let _cleanup = Cleanup(test_dir.clone());

    // Ensure the test directory doesn't exist
    if test_dir.exists() {
        fs::remove_dir_all(&test_dir).unwrap();
    }

    // This simulates what server.main.ts does - create directories with recursive option
    if !nested_path.exists() {
        make_dir_recursive_with_mode(&nested_path).unwrap();
    }

    // Verify all directories were created
    assert!(nested_path.exists(), "Nested directory should exist");
    assert!(test_dir.join("parent").exists(), "Parent directory should exist");
    assert!(test_dir.join("parent").join("child").exists(), "Child directory should exist");

    // Verify the permissions (only on Unix-like systems)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&nested_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "Directory should have 0o700 permissions");
    }
}

#[test]
fn should_not_fail_when_parent_directories_do_not_exist() {
    let test_dir = tempfile::tempdir().unwrap().keep();
    let test_dir = test_dir.join("server-main-nonexistent");
    let deeply_nested_path = test_dir.join("level1").join("level2").join("level3").join("extensions");
    let _cleanup = Cleanup(test_dir.clone());

    // Ensure the test directory doesn't exist
    if test_dir.exists() {
        fs::remove_dir_all(&test_dir).unwrap();
    }

    // This should not throw an error even though parent directories don't exist
    if !deeply_nested_path.exists() {
        make_dir_recursive_with_mode(&deeply_nested_path).expect("Should not throw when creating deeply nested directories");
    }

    // Verify the directory was created
    assert!(deeply_nested_path.exists(), "Deeply nested directory should exist");
}

#[test]
fn should_handle_existing_directories_gracefully() {
    let test_dir = tempfile::tempdir().unwrap().keep();
    let test_dir = test_dir.join("server-main-existing");
    let extensions_path = test_dir.join("extensions");
    let _cleanup = Cleanup(test_dir.clone());

    // Create the directory first
    make_dir_recursive_with_mode(&extensions_path).unwrap();
    assert!(extensions_path.exists());

    // Try to create it again - this simulates the if (!fs.existsSync(f)) check in server.main.ts
    if !extensions_path.exists() {
        make_dir_recursive_with_mode(&extensions_path).expect("Should not throw when directory already exists");
    }

    // The directory should still exist
    assert!(extensions_path.exists());
}

/// O `finally` do original: apaga o diretório de teste (e o pai temporário) ao sair, também quando o caso falha.
struct Cleanup(std::path::PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        if self.0.exists() {
            let _ = fs::remove_dir_all(&self.0);
        }
        if let Some(parent) = self.0.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }
}
