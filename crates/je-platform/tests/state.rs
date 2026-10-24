//! Porte de `src/vs/platform/state/test/node/state.test.ts` (suíte `StateService`), caso a caso e na mesma ordem.
//!
//! Divergências: roda sobre o provider de disco e o laço de eventos real (a E/S e o `ThrottledDelayer` de 100 ms
//! levam tempo de verdade), por isso o `flakySuite` do original. `Basics (delayed strategy)` e `Basics
//! (immediate strategy)`, e `Multiple ops...`, repetem o mesmo corpo no original; aqui o corpo é uma função
//! ([`basics`], [`multiple_ops`]) chamada com cada estratégia, para não duplicar código. O `setup` e o
//! `teardown` são o [`TestDir`], que cria e apaga o diretório de teste.

mod common;

use je_platform::files::i_file_service::IFileService;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use common::disk::{get_random_test_path, run_on_real_loop};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::DisposableStore;
use je_base::uri::URI;
use je_platform::files::disk::DiskFileSystemProvider;
use je_platform::files::file_service::FileService;
use je_platform::log::NullLogService;
use je_platform::state::{FileStorage, SaveStrategy};

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new() -> Self {
        let path = get_random_test_path(&["vsctests", "statemainservice"]);
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn storage_file(&self) -> PathBuf {
        let file = self.path.join("storage.json");
        std::fs::write(&file, "").unwrap();
        file
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn file_service(disposables: &Rc<DisposableStore>) -> Rc<dyn IFileService> {
    let log_service = NullLogService::shared();
    let file_service = disposables.add_rc(FileService::new(log_service.clone()));
    let disk_file_system_provider = disposables.add_rc(DiskFileSystemProvider::new(log_service, None));
    disposables.add(file_service.register_provider("file", disk_file_system_provider).unwrap());
    file_service
}

fn storage(disposables: &Rc<DisposableStore>, file_service: &Rc<dyn IFileService>, storage_file: &Path, strategy: SaveStrategy) -> Rc<FileStorage> {
    disposables.add_rc(FileStorage::new(URI::file(&*storage_file.to_string_lossy(), je_base::path::Platform::Posix), strategy, NullLogService::shared(), file_service.clone()))
}

fn s(text: &str) -> Dynamic {
    Dynamic::string(text)
}

fn item(service: &FileStorage, key: &str) -> Dynamic {
    service.get_item(key, Dynamic::Undefined)
}

fn item_or(service: &FileStorage, key: &str, default: &str) -> Dynamic {
    service.get_item(key, s(default))
}

fn items(entries: Vec<(&str, Dynamic)>) -> Vec<(String, Dynamic)> {
    entries.into_iter().map(|(key, data)| (key.to_owned(), data)).collect()
}

fn basics(strategy: SaveStrategy) {
    let disposables = DisposableStore::new();
    let test_dir = TestDir::new();
    run_on_real_loop({
        let disposables = disposables.clone();
        async move {
            let file_service = file_service(&disposables);
            let storage_file = test_dir.storage_file();

            let mut service = storage(&disposables, &file_service, &storage_file, strategy);
            service.init().await.unwrap();

            service.set_item("some.key", s("some.value"));
            assert_eq!(item(&service, "some.key"), s("some.value"));

            service.remove_item("some.key");
            assert_eq!(item_or(&service, "some.key", "some.default"), s("some.default"));

            assert!(!matches!(item(&service, "some.unknonw.key"), Dynamic::String(_)));

            service.set_item("some.other.key", s("some.other.value"));

            service.close().await.unwrap();

            service = storage(&disposables, &file_service, &storage_file, strategy);
            service.init().await.unwrap();

            assert_eq!(item(&service, "some.other.key"), s("some.other.value"));

            service.set_item("some.other.key", s("some.other.value"));
            assert_eq!(item(&service, "some.other.key"), s("some.other.value"));

            service.set_item("some.undefined.key", Dynamic::Undefined);
            assert_eq!(item_or(&service, "some.undefined.key", "some.default"), s("some.default"));

            service.set_item("some.null.key", Dynamic::Null);
            assert_eq!(item_or(&service, "some.null.key", "some.default"), s("some.default"));

            service.set_items(&items(vec![
                ("some.setItems.key1", s("some.value")),
                ("some.setItems.key2", Dynamic::Number(0.0)),
                ("some.setItems.key3", Dynamic::Bool(true)),
                ("some.setItems.key4", Dynamic::Null),
                ("some.setItems.key5", Dynamic::Undefined),
            ]));

            assert_eq!(item(&service, "some.setItems.key1"), s("some.value"));
            assert_eq!(item(&service, "some.setItems.key2"), Dynamic::Number(0.0));
            assert_eq!(item(&service, "some.setItems.key3"), Dynamic::Bool(true));
            assert_eq!(item(&service, "some.setItems.key4"), Dynamic::Undefined);
            assert_eq!(item(&service, "some.setItems.key5"), Dynamic::Undefined);

            service.set_items(&items(vec![
                ("some.setItems.key1", Dynamic::Undefined),
                ("some.setItems.key2", Dynamic::Undefined),
                ("some.setItems.key3", Dynamic::Undefined),
                ("some.setItems.key4", Dynamic::Null),
                ("some.setItems.key5", Dynamic::Undefined),
            ]));

            for key in ["some.setItems.key1", "some.setItems.key2", "some.setItems.key3", "some.setItems.key4", "some.setItems.key5"] {
                assert_eq!(item(&service, key), Dynamic::Undefined);
            }

            service.close().await.unwrap();
            disposables.clear();
        }
    });
}

#[test]
fn basics_delayed_strategy() {
    basics(SaveStrategy::Delayed);
}

#[test]
fn basics_immediate_strategy() {
    basics(SaveStrategy::Immediate);
}

fn multiple_ops(strategy: SaveStrategy) {
    let disposables = DisposableStore::new();
    let test_dir = TestDir::new();
    run_on_real_loop({
        let disposables = disposables.clone();
        async move {
            let file_service = file_service(&disposables);
            let storage_file = test_dir.storage_file();

            let mut service = storage(&disposables, &file_service, &storage_file, strategy);
            service.init().await.unwrap();

            service.set_item("some.key1", s("some.value1"));
            service.set_item("some.key2", s("some.value2"));
            service.set_item("some.key3", s("some.value3"));
            service.set_item("some.key4", s("some.value4"));
            service.remove_item("some.key4");

            assert_eq!(item(&service, "some.key1"), s("some.value1"));
            assert_eq!(item(&service, "some.key2"), s("some.value2"));
            assert_eq!(item(&service, "some.key3"), s("some.value3"));
            assert_eq!(item(&service, "some.key4"), Dynamic::Undefined);

            service.close().await.unwrap();

            service = storage(&disposables, &file_service, &storage_file, strategy);
            service.init().await.unwrap();

            assert_eq!(item(&service, "some.key1"), s("some.value1"));
            assert_eq!(item(&service, "some.key2"), s("some.value2"));
            assert_eq!(item(&service, "some.key3"), s("some.value3"));
            assert_eq!(item(&service, "some.key4"), Dynamic::Undefined);

            service.close().await.unwrap();
            disposables.clear();
        }
    });
}

#[test]
fn multiple_ops_are_buffered_and_applied() {
    multiple_ops(SaveStrategy::Delayed);
}

#[test]
fn multiple_ops_immediate_strategy() {
    multiple_ops(SaveStrategy::Immediate);
}

#[test]
fn used_before_init() {
    let disposables = DisposableStore::new();
    let test_dir = TestDir::new();
    run_on_real_loop({
        let disposables = disposables.clone();
        async move {
            let file_service = file_service(&disposables);
            let storage_file = test_dir.storage_file();

            let service = storage(&disposables, &file_service, &storage_file, SaveStrategy::Delayed);

            service.set_item("some.key1", s("some.value1"));
            service.set_item("some.key2", s("some.value2"));
            service.set_item("some.key3", s("some.value3"));
            service.set_item("some.key4", s("some.value4"));
            service.remove_item("some.key4");

            assert_eq!(item(&service, "some.key1"), s("some.value1"));
            assert_eq!(item(&service, "some.key2"), s("some.value2"));
            assert_eq!(item(&service, "some.key3"), s("some.value3"));
            assert_eq!(item(&service, "some.key4"), Dynamic::Undefined);

            service.init().await.unwrap();

            assert_eq!(item(&service, "some.key1"), s("some.value1"));
            assert_eq!(item(&service, "some.key2"), s("some.value2"));
            assert_eq!(item(&service, "some.key3"), s("some.value3"));
            assert_eq!(item(&service, "some.key4"), Dynamic::Undefined);

            service.close().await.unwrap();
            disposables.clear();
        }
    });
}

#[test]
fn used_after_close() {
    let disposables = DisposableStore::new();
    let test_dir = TestDir::new();
    run_on_real_loop({
        let disposables = disposables.clone();
        async move {
            let file_service = file_service(&disposables);
            let storage_file = test_dir.storage_file();

            let service = storage(&disposables, &file_service, &storage_file, SaveStrategy::Delayed);

            service.init().await.unwrap();

            service.set_item("some.key1", s("some.value1"));
            service.set_item("some.key2", s("some.value2"));
            service.set_item("some.key3", s("some.value3"));
            service.set_item("some.key4", s("some.value4"));

            service.close().await.unwrap();

            service.set_item("some.key5", s("some.marker"));

            let contents = std::fs::read_to_string(&storage_file).unwrap();
            assert!(contents.contains("some.value1"));
            assert!(!contents.contains("some.marker"));

            service.close().await.unwrap();
            disposables.clear();
        }
    });
}

#[test]
fn closed_before_init() {
    let disposables = DisposableStore::new();
    let test_dir = TestDir::new();
    run_on_real_loop({
        let disposables = disposables.clone();
        async move {
            let file_service = file_service(&disposables);
            let storage_file = test_dir.storage_file();

            let service = storage(&disposables, &file_service, &storage_file, SaveStrategy::Delayed);

            service.set_item("some.key1", s("some.value1"));
            service.set_item("some.key2", s("some.value2"));
            service.set_item("some.key3", s("some.value3"));
            service.set_item("some.key4", s("some.value4"));

            service.close().await.unwrap();

            let contents = std::fs::read_to_string(&storage_file).unwrap();
            assert_eq!(contents.len(), 0);
            disposables.clear();
        }
    });
}
