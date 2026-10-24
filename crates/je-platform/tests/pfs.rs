//! Porte de `src/vs/base/test/node/pfs/pfs.test.ts` (suíte `PFS`), caso a caso e na mesma ordem.
//!
//! Divergências:
//!
//! - Cada teste `async` roda no laço de eventos real do thread ([`run_on_real_loop`]), porque a E/S de disco
//!   leva tempo de verdade; o `setup` e o `teardown` (o diretório de teste aleatório e a guarda de vazamentos)
//!   são o [`Fixture`], descartado no fim do teste. O `configureFlushOnWrite(false)` do topo do original roda
//!   em cada [`Fixture::new`].
//! - Os casos `copy, rename and delete` e `rename without retry` apagam o `parentDir` inteiro. No original
//!   ele é `tmpdir()/vsctests/pfs`, o pai de todos os `testDir` (o mocha roda um teste por vez); aqui os testes
//!   rodam em paralelo, então cada um usa um `parentDir` aleatório próprio, sob `tmpdir()/vsctests`.
//! - `Promises.realpath` e `realpathSync` são a mesma função [`pfs::realpath`] no porte.
//! - Os três dados de `testWriteFile` (`string`, `Buffer` e `Uint8Array`) são `String` e `Vec<u8>`: o Rust só
//!   tem uma representação de bytes.
//! - Os links simbólicos de pasta usam `symlink` no Unix e `symlink_dir` no Windows (o original usa
//!   `junction`, que o Rust não cria).
//! - `Promises.readdir(path, { withFileTypes: true })` é [`pfs::readdir`] e a versão só de nomes é
//!   [`pfs::readdir_names`].

mod common;

use std::path::Path;

use common::disk::{TestDir, run_on_real_loop, vscode_source_path};
use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, join};
use je_base::async_::{async_fn, timeout};
use je_base::buffer::VSBuffer;
use je_base::extpath::random_path;
use je_base::path::ProcessEnvironment;
use je_base::promise::Promise;
use je_platform::files::pfs::{self, RenameRetry, RimRafMode};

/// `setup` e `teardown` da suíte: o `testDir` aleatório (criado, e apagado ao sair do escopo) e a guarda
/// do `ensureNoDisposablesAreLeakedInTestSuite`.
struct Fixture {
    _leaks: LeakGuard,
    dir: TestDir,
}

impl Fixture {
    fn new() -> Self {
        pfs::configure_flush_on_write(false); // speed up all unit tests by disabling flush on write
        Self { _leaks: ensure_no_disposables_are_leaked_in_test_suite(), dir: TestDir::new(&["vsctests", "pfs"]) }
    }

    /// `testDir`.
    fn path(&self) -> String {
        self.dir.path().to_string_lossy().into_owned()
    }

    /// `join(testDir, name)`.
    fn join(&self, name: &str) -> String {
        join(&[&self.path(), name])
    }
}

/// `randomPath(parent)`.
fn random(parent: &str) -> String {
    random_path(Some(parent), None, 8, &ProcessEnvironment)
}

fn exists(path: &str) -> bool {
    Path::new(path).exists()
}

/// `fs.writeFileSync(path, 'Contents')`.
fn write_contents(path: &str) {
    std::fs::write(path, "Contents").unwrap();
}

/// `fs.symlinkSync(target, path, 'junction')`.
fn create_symlink(target: &str, path: &str) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, path).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(target, path).unwrap();
}

/// `fs.writeFileSync` de dois arquivos na raiz do `testDir`, o começo dos casos de `rimraf`.
fn write_two_files(fixture: &Fixture) {
    write_contents(&fixture.join("somefile.txt"));
    write_contents(&fixture.join("someOtherFile.txt"));
}

/// `(new Array(100 * 1024)).join('Large String\n')`.
fn big_data() -> String {
    "Large String\n".repeat(100 * 1024 - 1)
}

#[test]
fn write_file() {
    let fixture = Fixture::new();
    let test_file = fixture.join("writefile.txt");

    run_on_real_loop(async move {
        assert!(!pfs::exists(&test_file));

        pfs::write_file(&test_file, "Hello World", None).await.unwrap();

        assert_eq!(String::from_utf8(std::fs::read(&test_file).unwrap()).unwrap(), "Hello World");
    });
}

#[test]
fn write_file_parallel_write_on_different_files_works() {
    let fixture = Fixture::new();
    let test_files: Vec<String> = (1..=5).map(|n| fixture.join(&format!("writefile{n}.txt"))).collect();

    run_on_real_loop(async move {
        Promise::all(test_files.iter().enumerate().map(|(index, file)| pfs::write_file(file, format!("Hello World {}", index + 1), None)).collect())
            .await
            .unwrap();
        for (index, file) in test_files.iter().enumerate() {
            assert_eq!(String::from_utf8(std::fs::read(file).unwrap()).unwrap(), format!("Hello World {}", index + 1));
        }
    });
}

#[test]
fn write_file_parallel_write_on_same_files_works_and_is_sequentalized() {
    let fixture = Fixture::new();
    let test_file = fixture.join("writefile.txt");

    run_on_real_loop(async move {
        let delayed = |value: u32| {
            let test_file = test_file.clone();
            async_fn(async move {
                timeout(10).await?;
                pfs::write_file(&test_file, format!("Hello World {value}"), None).await
            })
        };

        Promise::all(vec![
            pfs::write_file(&test_file, "Hello World 1", None),
            pfs::write_file(&test_file, "Hello World 2", None),
            delayed(3),
            pfs::write_file(&test_file, "Hello World 4", None),
            delayed(5),
        ])
        .await
        .unwrap();
        assert_eq!(String::from_utf8(std::fs::read(&test_file).unwrap()).unwrap(), "Hello World 5");
    });
}

#[test]
fn rimraf_simple_unlink() {
    let fixture = Fixture::new();
    write_two_files(&fixture);
    let test_dir = fixture.path();

    run_on_real_loop(async move {
        pfs::rm(&test_dir, RimRafMode::Unlink, None).await.unwrap();
        assert!(!exists(&test_dir));
    });
}

#[test]
fn rimraf_simple_move() {
    let fixture = Fixture::new();
    write_two_files(&fixture);
    let test_dir = fixture.path();

    run_on_real_loop(async move {
        pfs::rm(&test_dir, RimRafMode::Move, None).await.unwrap();
        assert!(!exists(&test_dir));
    });
}

#[test]
fn rimraf_simple_move_with_move_to_path() {
    let fixture = Fixture::new();
    write_two_files(&fixture);
    let test_dir = fixture.path();

    run_on_real_loop(async move {
        // join(dirname(testDir), `${basename(testDir)}.vsctmp`)
        let move_to_path = format!("{test_dir}.vsctmp");
        pfs::rm(&test_dir, RimRafMode::Move, Some(move_to_path.as_str())).await.unwrap();
        assert!(!exists(&test_dir));
    });
}

#[test]
fn rimraf_path_does_not_exist_move() {
    let fixture = Fixture::new();
    let non_existing_dir = fixture.join("unknown-move");

    run_on_real_loop(async move {
        pfs::rm(&non_existing_dir, RimRafMode::Move, None).await.unwrap();
    });
}

#[test]
fn rimraf_path_does_not_exist_unlink() {
    let fixture = Fixture::new();
    let non_existing_dir = fixture.join("unknown-unlink");

    run_on_real_loop(async move {
        pfs::rm(&non_existing_dir, RimRafMode::Unlink, None).await.unwrap();
    });
}

/// O começo dos casos de estrutura recursiva: dois arquivos e uma pasta com um arquivo.
fn write_recursive_structure(fixture: &Fixture) {
    write_two_files(fixture);
    std::fs::create_dir(fixture.join("somefolder")).unwrap();
    write_contents(&join(&[&fixture.path(), "somefolder", "somefile.txt"]));
}

#[test]
fn rimraf_recursive_folder_structure_unlink() {
    let fixture = Fixture::new();
    write_recursive_structure(&fixture);
    let test_dir = fixture.path();

    run_on_real_loop(async move {
        pfs::rm(&test_dir, RimRafMode::Unlink, None).await.unwrap();
        assert!(!exists(&test_dir));
    });
}

#[test]
fn rimraf_recursive_folder_structure_move() {
    let fixture = Fixture::new();
    write_recursive_structure(&fixture);
    let test_dir = fixture.path();

    run_on_real_loop(async move {
        pfs::rm(&test_dir, RimRafMode::Move, None).await.unwrap();
        assert!(!exists(&test_dir));
    });
}

#[test]
fn rimraf_simple_ends_with_dot_move() {
    let fixture = Fixture::new();
    write_two_files(&fixture);
    let test_dir = fixture.path();

    run_on_real_loop(async move {
        pfs::rm(&test_dir, RimRafMode::Move, None).await.unwrap();
        assert!(!exists(&test_dir));
    });
}

#[test]
fn rimraf_simple_ends_with_dot_slash_backslash_move() {
    let fixture = Fixture::new();
    write_two_files(&fixture);
    let test_dir = fixture.path();

    run_on_real_loop(async move {
        pfs::rm(&format!("{test_dir}{}", std::path::MAIN_SEPARATOR), RimRafMode::Move, None).await.unwrap();
        assert!(!exists(&test_dir));
    });
}

/// As asserções do que `copy` deixa em `dir`: `index.html`, `site.css` e `examples/small.jxs`.
fn assert_fixtures_in(dir: &str) {
    assert!(exists(dir));
    assert!(exists(&join(&[dir, "index.html"])));
    assert!(exists(&join(&[dir, "site.css"])));
    assert!(exists(&join(&[dir, "examples"])));
    assert!(std::fs::metadata(join(&[dir, "examples"])).unwrap().is_dir());
    assert!(exists(&join(&[dir, "examples", "small.jxs"])));
}

fn pfs_fixtures_dir() -> String {
    vscode_source_path("vs/base/test/node/pfs/fixtures").to_string_lossy().into_owned()
}

#[test]
fn copy_rename_and_delete() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    pfs::configure_flush_on_write(false);
    let source_dir = pfs_fixtures_dir();
    let parent = TestDir::uncreated(&["vsctests"]);
    let parent_dir = parent.path().to_string_lossy().into_owned();
    let target_dir = random(&parent_dir);
    let target_dir2 = random(&parent_dir);

    run_on_real_loop(async move {
        pfs::copy(&source_dir, &target_dir, true).unwrap();

        assert_fixtures_in(&target_dir);

        pfs::rename(&target_dir, &target_dir2, RenameRetry::Default).await.unwrap();

        assert!(!exists(&target_dir));
        assert_fixtures_in(&target_dir2);

        pfs::rename(&join(&[&target_dir2, "index.html"]), &join(&[&target_dir2, "index_moved.html"]), RenameRetry::Default).await.unwrap();

        assert!(!exists(&join(&[&target_dir2, "index.html"])));
        assert!(exists(&join(&[&target_dir2, "index_moved.html"])));

        pfs::rm(&parent_dir, RimRafMode::Unlink, None).await.unwrap();

        assert!(!exists(&parent_dir));
    });
}

#[test]
fn rename_without_retry() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    pfs::configure_flush_on_write(false);
    let source_dir = pfs_fixtures_dir();
    let parent = TestDir::uncreated(&["vsctests"]);
    let parent_dir = parent.path().to_string_lossy().into_owned();
    let target_dir = random(&parent_dir);
    let target_dir2 = random(&parent_dir);

    run_on_real_loop(async move {
        pfs::copy(&source_dir, &target_dir, true).unwrap();
        pfs::rename(&target_dir, &target_dir2, RenameRetry::Disabled).await.unwrap();

        assert!(!exists(&target_dir));
        assert_fixtures_in(&target_dir2);

        pfs::rename(&join(&[&target_dir2, "index.html"]), &join(&[&target_dir2, "index_moved.html"]), RenameRetry::Disabled).await.unwrap();

        assert!(!exists(&join(&[&target_dir2, "index.html"])));
        assert!(exists(&join(&[&target_dir2, "index_moved.html"])));

        pfs::rm(&parent_dir, RimRafMode::Unlink, None).await.unwrap();

        assert!(!exists(&parent_dir));
    });
}

#[test]
fn copy_handles_symbolic_links() {
    let fixture = Fixture::new();
    let test_dir = fixture.path();
    let symbolic_link_target = random(&test_dir);
    let sym_link = random(&test_dir);
    let copy_target = random(&test_dir);

    run_on_real_loop(async move {
        std::fs::create_dir_all(&symbolic_link_target).unwrap();

        create_symlink(&symbolic_link_target, &sym_link);

        // Copy preserves symlinks if configured as such
        //
        // Windows: this test does not work because creating symlinks
        // requires priviledged permissions (admin).
        if !cfg!(windows) {
            pfs::copy(&sym_link, &copy_target, true).unwrap();

            assert!(exists(&copy_target));

            let symbolic_link = pfs::symlink_stat(&copy_target).unwrap().symbolic_link.expect("o destino deve ser um link simbólico");
            assert!(!symbolic_link.dangling);

            let target = std::fs::read_link(&copy_target).unwrap();
            assert_eq!(target.to_string_lossy(), symbolic_link_target);

            // Copy does not preserve symlinks if configured as such

            pfs::rm(&copy_target, RimRafMode::Unlink, None).await.unwrap();
            pfs::copy(&sym_link, &copy_target, false).unwrap();

            assert!(exists(&copy_target));

            assert!(pfs::symlink_stat(&copy_target).unwrap().symbolic_link.is_none());
        }

        // Copy does not fail over dangling symlinks

        pfs::rm(&copy_target, RimRafMode::Unlink, None).await.unwrap();
        pfs::rm(&symbolic_link_target, RimRafMode::Unlink, None).await.unwrap();

        pfs::copy(&sym_link, &copy_target, true).unwrap(); // this should not throw

        if !cfg!(windows) {
            assert!(pfs::symlink_stat(&copy_target).unwrap().symbolic_link.is_some_and(|link| link.dangling));
        } else {
            assert!(!exists(&copy_target));
        }
    });
}

#[test]
fn copy_handles_symbolic_links_when_the_reference_is_inside_source() {
    let fixture = Fixture::new();
    let test_dir = fixture.path();

    run_on_real_loop(async move {
        // Source Folder
        let source_folder = join(&[&random(&test_dir), "copy-test"]); // copy-test
        let source_link_test_folder = join(&[&source_folder, "link-test"]); // copy-test/link-test
        let source_link_md5_js_folder = join(&[&source_link_test_folder, "md5"]); // copy-test/link-test/md5
        let source_link_md5_js_file = join(&[&source_link_md5_js_folder, "md5.js"]); // copy-test/link-test/md5/md5.js
        std::fs::create_dir_all(&source_link_md5_js_folder).unwrap();
        pfs::write_file(&source_link_md5_js_file, "Hello from MD5", None).await.unwrap();

        let source_link_md5_js_folder_linked = join(&[&source_link_test_folder, "md5-linked"]); // copy-test/link-test/md5-linked
        create_symlink(&source_link_md5_js_folder, &source_link_md5_js_folder_linked);

        // Target Folder
        let target_link_test_folder = join(&[&source_folder, "link-test copy"]); // copy-test/link-test copy
        let target_link_md5_js_folder = join(&[&target_link_test_folder, "md5"]); // copy-test/link-test copy/md5
        let target_link_md5_js_file = join(&[&target_link_md5_js_folder, "md5.js"]); // copy-test/link-test copy/md5/md5.js
        let target_link_md5_js_folder_linked = join(&[&target_link_test_folder, "md5-linked"]); // copy-test/link-test copy/md5-linked

        // Copy with `preserveSymlinks: true` and verify result
        //
        // Windows: this test does not work because creating symlinks
        // requires priviledged permissions (admin).
        if !cfg!(windows) {
            pfs::copy(&source_link_test_folder, &target_link_test_folder, true).unwrap();

            assert!(exists(&target_link_test_folder));
            assert!(exists(&target_link_md5_js_folder));
            assert!(exists(&target_link_md5_js_file));
            assert!(exists(&target_link_md5_js_folder_linked));
            assert!(std::fs::symlink_metadata(&target_link_md5_js_folder_linked).unwrap().is_symlink());

            let link_target = std::fs::read_link(&target_link_md5_js_folder_linked).unwrap();
            assert_eq!(link_target.to_string_lossy(), target_link_md5_js_folder);

            pfs::rm(&target_link_test_folder, RimRafMode::Unlink, None).await.unwrap();
        }

        // Copy with `preserveSymlinks: false` and verify result
        pfs::copy(&source_link_test_folder, &target_link_test_folder, false).unwrap();

        assert!(exists(&target_link_test_folder));
        assert!(exists(&target_link_md5_js_folder));
        assert!(exists(&target_link_md5_js_file));
        assert!(exists(&target_link_md5_js_folder_linked));
        assert!(std::fs::symlink_metadata(&target_link_md5_js_folder_linked).unwrap().is_dir());
    });
}

#[test]
fn read_dirs_in_dir() {
    let fixture = Fixture::new();
    for name in ["somefolder1", "somefolder2", "somefolder3"] {
        std::fs::create_dir(fixture.join(name)).unwrap();
    }
    write_two_files(&fixture);
    let test_dir = fixture.path();

    run_on_real_loop(async move {
        let result = pfs::read_dirs_in_dir(&test_dir).unwrap();
        assert_eq!(result.len(), 3);
        assert!(result.iter().any(|name| name == "somefolder1"));
        assert!(result.iter().any(|name| name == "somefolder2"));
        assert!(result.iter().any(|name| name == "somefolder3"));
    });
}

#[test]
fn stat_link() {
    let fixture = Fixture::new();
    let test_dir = fixture.path();
    let directory = random(&test_dir);
    let symbolic_link = random(&test_dir);

    run_on_real_loop(async move {
        std::fs::create_dir_all(&directory).unwrap();

        create_symlink(&directory, &symbolic_link);

        let stat_and_is_link = pfs::symlink_stat(&directory).unwrap();
        assert!(stat_and_is_link.symbolic_link.is_none());

        let stat_and_is_link = pfs::symlink_stat(&symbolic_link).unwrap();
        assert!(stat_and_is_link.symbolic_link.is_some_and(|link| !link.dangling));
    });
}

#[test]
fn stat_link_non_existing_target() {
    let fixture = Fixture::new();
    let test_dir = fixture.path();
    let directory = random(&test_dir);
    let symbolic_link = random(&test_dir);

    run_on_real_loop(async move {
        std::fs::create_dir_all(&directory).unwrap();

        create_symlink(&directory, &symbolic_link);

        pfs::rm(&directory, RimRafMode::Unlink, None).await.unwrap();

        let stat_and_is_link = pfs::symlink_stat(&symbolic_link).unwrap();
        assert!(stat_and_is_link.symbolic_link.is_some_and(|link| link.dangling));
    });
}

#[test]
fn readdir() {
    let fixture = Fixture::new();
    let parent = random(&join(&[&fixture.path(), "pfs"]));
    let new_dir = join(&[&parent, "öäü"]);

    run_on_real_loop(async move {
        std::fs::create_dir_all(&new_dir).unwrap();

        assert!(exists(&new_dir));

        let children = pfs::readdir_names(&parent).unwrap();
        assert!(children.iter().any(|name| name == "öäü")); // Mac always converts to NFD, so
    });
}

#[test]
fn readdir_with_file_types() {
    let fixture = Fixture::new();
    let new_dir = fixture.join("öäü");
    let some_file = fixture.join("somefile.txt");
    let test_dir = fixture.path();

    run_on_real_loop(async move {
        std::fs::create_dir_all(&new_dir).unwrap();

        pfs::write_file(&some_file, "contents", None).await.unwrap();

        assert!(exists(&new_dir));

        let children = pfs::readdir(&test_dir).unwrap();

        assert!(children.iter().any(|n| n.name == "öäü")); // Mac always converts to NFD, so
        assert!(children.iter().any(|n| n.kind.is_directory));

        assert!(children.iter().any(|n| n.name == "somefile.txt"));
        assert!(children.iter().any(|n| n.kind.is_file));
    });
}

/// `testWriteFile`.
async fn test_write_file<D: Into<Vec<u8>>>(test_dir: String, small_data: D, small_data_value: &str, big_data: D, big_data_value: &str) {
    let test_file = join(&[&test_dir, "flushed.txt"]);

    assert!(exists(&test_dir));

    pfs::write_file(&test_file, small_data, None).await.unwrap();
    assert_eq!(String::from_utf8(std::fs::read(&test_file).unwrap()).unwrap(), small_data_value);

    pfs::write_file(&test_file, big_data, None).await.unwrap();
    assert_eq!(String::from_utf8(std::fs::read(&test_file).unwrap()).unwrap(), big_data_value);
}

#[test]
fn write_file_string() {
    let fixture = Fixture::new();
    let test_dir = fixture.path();

    run_on_real_loop(async move {
        let small_data = "Hello World";
        let big = big_data();

        test_write_file(test_dir, small_data.to_string(), small_data, big.clone(), &big).await;
    });
}

#[test]
fn write_file_string_flush_on_write() {
    let fixture = Fixture::new();
    let test_dir = fixture.path();
    pfs::configure_flush_on_write(true);

    run_on_real_loop(async move {
        let small_data = "Hello World";
        let big = big_data();

        test_write_file(test_dir, small_data.to_string(), small_data, big.clone(), &big).await;
    });
    pfs::configure_flush_on_write(false);
}

#[test]
fn write_file_buffer() {
    let fixture = Fixture::new();
    let test_dir = fixture.path();

    run_on_real_loop(async move {
        let small_data = "Hello World";
        let big = big_data();

        test_write_file(test_dir, small_data.as_bytes().to_vec(), small_data, big.clone().into_bytes(), &big).await;
    });
}

#[test]
fn write_file_uint8_array() {
    let fixture = Fixture::new();
    let test_dir = fixture.path();

    run_on_real_loop(async move {
        let small_data = "Hello World";
        let big = big_data();

        test_write_file(test_dir, VSBuffer::from_string(small_data).to_vec(), small_data, VSBuffer::from_string(&big).to_vec(), &big).await;
    });
}

#[test]
fn write_file_string_error_handling() {
    let fixture = Fixture::new();
    let test_file = fixture.join("flushed.txt");

    std::fs::create_dir(&test_file).unwrap(); // this will trigger an error later because testFile is now a directory!

    run_on_real_loop(async move {
        let expected_error = pfs::write_file(&test_file, "Hello World", None).await.err();

        assert!(expected_error.is_some());
    });
}

#[test]
fn write_file_sync() {
    let fixture = Fixture::new();
    let test_file = fixture.join("flushed.txt");

    pfs::write_file_sync(&test_file, "Hello World", None).unwrap();
    assert_eq!(String::from_utf8(std::fs::read(&test_file).unwrap()).unwrap(), "Hello World");

    let large_string = big_data();

    pfs::write_file_sync(&test_file, &large_string, None).unwrap();
    assert_eq!(String::from_utf8(std::fs::read(&test_file).unwrap()).unwrap(), large_string);
}

#[test]
fn realcase() {
    let fixture = Fixture::new();
    let test_dir = fixture.path();

    run_on_real_loop(async move {
        // assume case insensitive file system
        if cfg!(any(windows, target_os = "macos")) {
            let upper = test_dir.to_uppercase();
            let real = pfs::realcase(&upper, None);

            if let Some(real) = real {
                // can be null in case of permission errors
                assert_ne!(real, upper);
                assert_eq!(real.to_uppercase(), upper);
                assert_eq!(real, test_dir);
            }
        }
        // linux, unix, etc. -> assume case sensitive file system
        else {
            let real = pfs::realcase(&test_dir, None);
            assert_eq!(real.as_deref(), Some(test_dir.as_str()));

            let real = pfs::realcase(&test_dir.to_uppercase(), None);
            assert_eq!(real.as_deref(), Some(test_dir.to_uppercase().as_str()));
        }
    });
}

#[test]
fn realpath() {
    let fixture = Fixture::new();
    let test_dir = fixture.path();

    run_on_real_loop(async move {
        let realpath_val = pfs::realpath(&test_dir).unwrap();
        assert!(!realpath_val.is_empty());
    });
}

#[test]
fn realpath_sync() {
    let fixture = Fixture::new();

    let realpath = pfs::realpath(&fixture.path()).unwrap();
    assert!(!realpath.is_empty());
}
