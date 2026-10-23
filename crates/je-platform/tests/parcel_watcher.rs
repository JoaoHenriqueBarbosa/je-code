//! Porte de `src/vs/platform/files/test/node/parcelWatcher.test.ts` (suíte `File Watcher (parcel)`), caso a caso e na mesma ordem.
//!
//! O arquivo original tem duas suítes: `File Watcher (parcel) request deduplication` (6 casos, que o
//! original executa) e `File Watcher (parcel)` (36 casos, `suite.skip` no original: "flaky em pipelines do
//! Azure, roda sob demanda quando a biblioteca do watcher muda"). Aqui as duas rodam, na ordem do arquivo
//! (a tarefa falava em 29 casos; são 42, todos portados), com esperas por condição do loop (nunca `sleep`
//! fixo) e o prazo de 10 s do `this.timeout(10000)`.
//!
//! Os casos que o original pula por plataforma viram `#[ignore]` condicional no mesmo critério, mantendo
//! a contagem. Pendente neste host (Linux): `unc support`, `wrong casing`, os dois `suspend/resume (...
//! not reusing watcher)` (só macOS, no original porque "Linux/Windows: times out for some reason") e os
//! quatro `... case insensitive on Windows/Mac`, que não puderam ser executados aqui.
//!
//! Divergências: as fixtures (`fixtures/service`) são geradas por `common::watcher::TestDir` em vez de
//! copiadas de `ref/`; `FileAccess.asFileUri('').fsPath` (um segundo diretório válido fora do de teste, em
//! `invalid path does not crash watcher`) é um segundo diretório temporário; os `await Promises.x()` de
//! disco rodam uma volta do loop antes de operar (como o pool de threads do Node) e o `setImmediate` do
//! `awaitEvent` é uma volta do loop; o `correlationId = Math.random()` é um inteiro fixo.

mod common;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use common::watcher::*;
use je_base::glob::{PatternItem, RelativePattern};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_platform::files::files::{FileChange, FileChangeFilter, FileChangeType};
use je_platform::files::node_watcher::base_watcher::{Suspended, WatcherBackend};
use je_platform::files::node_watcher::parcel::ParcelWatcher;
use je_platform::files::watcher::{LogMessageType, WatchRequest, Watcher};

use FileChangeType::{Added, Deleted, Updated};

const NO_CID: Cid = Cid::Exactly(None);

/// `TestParcelWatcher.testRemoveDuplicateRequests(paths, excludes)`.
fn test_remove_duplicate_requests(watcher: &ParcelWatcher, paths: &[&str], excludes: &[&str]) -> Vec<String> {
    // Work with strings as paths to simplify testing
    let requests = paths.iter().map(|path| WatchRequest { excludes: excludes.iter().map(|e| e.to_string()).collect(), ..request(path, true) }).collect();
    watcher.remove_duplicate_requests(requests, false /* validate paths skipped for tests */).into_iter().map(|r| r.path).collect()
}

/// `TestParcelWatcher.deduplicateRequests(requests)`.
fn deduplicate_requests(watcher: &ParcelWatcher, requests: Vec<WatchRequest>) -> Vec<WatchRequest> {
    watcher.remove_duplicate_requests(requests, false /* validate paths skipped for tests */)
}

// region: suíte `File Watcher (parcel) request deduplication`

#[test]
fn uncorrelated_same_root_requests_preserve_a_broad_watcher_in_either_order() {
    let watcher = test_parcel_watcher();
    let broad = request("/workspace", true);
    let narrow = WatchRequest { includes: Some(vec!["mcp/*.mjs".into()]), ..request("/workspace", true) };

    assert_eq!(deduplicate_requests(&watcher, vec![broad.clone(), narrow.clone()]), vec![broad.clone()]);
    assert_eq!(deduplicate_requests(&watcher, vec![narrow, broad.clone()]), vec![broad]);
    watcher.dispose();
}

#[test]
fn uncorrelated_same_root_requests_combine_narrow_includes_and_preserve_shared_excludes() {
    let watcher = test_parcel_watcher();
    let first = WatchRequest { excludes: vec!["**/node_modules/**".into()], includes: Some(vec!["mcp/*.mjs".into()]), ..request("/workspace", true) };
    let second = WatchRequest { excludes: vec!["**/node_modules/**".into()], includes: Some(vec![".custom/agents/*.md".into()]), ..request("/workspace", true) };

    assert_eq!(
        deduplicate_requests(&watcher, vec![first.clone(), second]),
        vec![WatchRequest { includes: Some(vec!["mcp/*.mjs".into(), ".custom/agents/*.md".into()]), ..first }]
    );
    watcher.dispose();
}

#[test]
fn uncorrelated_same_root_requests_combine_overlapping_narrow_includes() {
    let watcher = test_parcel_watcher();
    let first = WatchRequest { includes: Some(vec!["mcp/*.mjs".into(), "shared/**".into()]), ..request("/workspace", true) };
    let second = WatchRequest { includes: Some(vec!["shared/*.json".into(), ".custom/agents/*.md".into()]), ..request("/workspace", true) };

    assert_eq!(
        deduplicate_requests(&watcher, vec![first.clone(), second]),
        vec![WatchRequest { includes: Some(vec!["mcp/*.mjs".into(), "shared/**".into(), "shared/*.json".into(), ".custom/agents/*.md".into()]), ..first }]
    );
    watcher.dispose();
}

#[test]
fn uncorrelated_same_root_requests_retain_overlapping_excludes() {
    let watcher = test_parcel_watcher();
    let first = WatchRequest { excludes: vec!["**/node_modules/**".into(), "**/dist/**".into()], ..request("/workspace", true) };
    let second = WatchRequest { excludes: vec!["**/node_modules/**".into()], ..request("/workspace", true) };

    assert_eq!(deduplicate_requests(&watcher, vec![first.clone(), second]), vec![WatchRequest { excludes: vec!["**/node_modules/**".into()], ..first }]);
    watcher.dispose();
}

#[test]
fn uncorrelated_same_root_requests_do_not_retain_exclusions_absent_from_another_request() {
    let watcher = test_parcel_watcher();
    let first = WatchRequest { excludes: vec!["mcp/**".into()], ..request("/workspace", true) };
    let second = WatchRequest { includes: Some(vec!["mcp/*.mjs".into()]), ..request("/workspace", true) };

    assert_eq!(deduplicate_requests(&watcher, vec![first.clone(), second]), vec![WatchRequest { excludes: vec![], ..first }]);
    watcher.dispose();
}

#[test]
fn requests_with_different_correlation_ids_remain_separate() {
    let watcher = test_parcel_watcher();
    let first = WatchRequest { correlation_id: Some(1), ..request("/workspace", true) };
    let second = WatchRequest { includes: Some(vec!["mcp/*.mjs".into()]), correlation_id: Some(2), ..request("/workspace", true) };

    assert_eq!(deduplicate_requests(&watcher, vec![first.clone(), second.clone()]), vec![first, second]);
    watcher.dispose();
}

// endregion

// region: suíte `File Watcher (parcel)`

/// O estado que o `setup`/`teardown` do original mantêm.
struct Test {
    dir: TestDir,
    watcher: Rc<ParcelWatcher>,
}

impl Test {
    fn await_event(&self, path: &str, change_type: FileChangeType) -> Promise<Vec<FileChange>> {
        self.await_event_with(path, change_type, None, NO_CID, None)
    }

    /// `awaitEvent(watcher, path, type, failOnEventReason, correlationId, expectedCount)`, sem o `await
    /// timeout(1)` final (o `unsubscribe` fora da pilha do evento não importa para o `notify`).
    fn await_event_with(&self, path: &str, change_type: FileChangeType, fail_reason: Option<&str>, cid: Cid, expected_count: Option<usize>) -> Promise<Vec<FileChange>> {
        await_event_or_fail(self.watcher.on_did_change_file(), path, change_type, fail_reason, cid, expected_count)
    }

    /// `awaitMessage(watcher, type)`.
    fn await_message(&self, message_type: LogMessageType) -> Promise<()> {
        to_promise(&self.watcher.on_did_log_message().filter(move |m| m.message_type == message_type, None)).then(|_| ())
    }

    fn on_watch_fail(&self) -> Promise<WatchRequest> {
        to_promise(&self.watcher.base().on_did_watch_fail.event())
    }

    fn on_did_watch(&self) -> Promise<()> {
        to_promise(&self.watcher.base().on_did_watch.event())
    }

    fn is_suspended(&self, request: &WatchRequest) -> Suspended {
        self.watcher.base().is_suspended(request)
    }

    async fn watch(&self, requests: Vec<WatchRequest>) {
        must(self.watcher.watch(requests)).await;
    }

    async fn basic_crud_test(&self, file_path: &str, cid: Cid, expected_count: Option<usize>) {
        // New file
        let change_future = self.await_event_with(file_path, Added, None, cid, expected_count);
        write_file(file_path, "Hello World").await;
        must(change_future).await;

        // Change file
        let change_future = self.await_event_with(file_path, Updated, None, cid, expected_count);
        write_file(file_path, "Hello Change").await;
        must(change_future).await;

        // Delete file
        let change_future = self.await_event_with(file_path, Deleted, None, cid, expected_count);
        unlink(file_path).await;
        must(change_future).await;
    }

    async fn crud(&self, file_path: &str) {
        self.basic_crud_test(file_path, NO_CID, None).await;
    }

    async fn test_excludes(&self, excludes: Vec<String>) {
        self.watch(vec![WatchRequest { excludes, ..request(&self.dir.path, true) }]).await;

        // New file (*.txt)
        let new_text_file_path = self.dir.join(&["deep", "newFile.txt"]);
        let change_future = self.await_event(&new_text_file_path, Added);
        write_file(&new_text_file_path, "Hello World").await;

        let res = must(Promise::race(vec![timeout(500).then(|()| true), change_future.then(|_| false)])).await;

        if !res {
            panic!("Unexpected change event");
        }
    }
}

/// `setup` + corpo + `teardown`.
fn run<F, Fut>(body: F)
where
    F: FnOnce(Rc<Test>) -> Fut,
    Fut: Future<Output = ()> + 'static,
{
    let test = Rc::new(Test { dir: TestDir::new(), watcher: test_parcel_watcher() });
    let teardown = test.clone();
    let future = body(test);
    run_async_test(async move {
        future.await;

        let watchers = teardown.watcher.watchers();
        let stopped_instances = Rc::new(Cell::new(0usize));
        for instance in &watchers {
            let (counter, watched) = (stopped_instances.clone(), instance.clone());
            std::mem::forget(instance.on_did_stop.event().once().listen(move |_| {
                if watched.stopped() {
                    counter.set(counter.get() + 1);
                }
            }));
        }

        must(teardown.watcher.stop()).await;
        assert_eq!(stopped_instances.get(), watchers.len(), "All watchers must be stopped before the test ends");
        teardown.watcher.dispose();
    });
}

fn req(path: &str) -> WatchRequest {
    request(path, true)
}

#[test]
fn basics() {
    run(|t| async move {
        let request = req(&t.dir.path);
        t.watch(vec![request.clone()]).await;

        let instance = t.watcher.watchers()[0].clone();
        assert_eq!(request, instance.request);
        assert!(!instance.failed());
        assert!(!instance.stopped());

        let disposables = DisposableStore::new();

        let subscriptions1: Rc<RefCell<Vec<(String, FileChangeType)>>> = Rc::new(RefCell::new(Vec::new()));
        let subscriptions2: Rc<RefCell<Vec<(String, FileChangeType)>>> = Rc::new(RefCell::new(Vec::new()));
        let record = |store: &Rc<RefCell<Vec<(String, FileChangeType)>>>| {
            let store = store.clone();
            Rc::new(move |change: &FileChange| {
                let path = je_platform_fs_path(change);
                let mut entries = store.borrow_mut();
                match entries.iter_mut().find(|(p, _)| *p == path) {
                    Some(entry) => entry.1 = change.change_type,
                    None => entries.push((path, change.change_type)),
                }
            }) as Rc<dyn Fn(&FileChange)>
        };
        let get = |store: &Rc<RefCell<Vec<(String, FileChangeType)>>>, path: &str| store.borrow().iter().find(|(p, _)| p == path).map(|(_, t)| *t);

        // New file
        let new_file_path = t.dir.join(&["deep", "newFile.txt"]);
        disposables.add(instance.subscribe(&new_file_path, record(&subscriptions1)));
        disposables.add(instance.subscribe(&new_file_path, record(&subscriptions2))); // can subscribe multiple times
        assert!(instance.include(&new_file_path));
        assert!(!instance.exclude(&new_file_path));
        let change_future = t.await_event(&new_file_path, Added);
        write_file(&new_file_path, "Hello World").await;
        must(change_future).await;
        assert_eq!(get(&subscriptions1, &new_file_path), Some(Added));
        assert_eq!(get(&subscriptions2, &new_file_path), Some(Added));

        // New folder
        let new_folder_path = t.dir.join(&["deep", "New Folder"]);
        disposables.add(instance.subscribe(&new_folder_path, record(&subscriptions1)));
        let disposable = instance.subscribe(&new_folder_path, record(&subscriptions2));
        disposable.dispose();
        assert!(instance.include(&new_folder_path));
        assert!(!instance.exclude(&new_folder_path));
        let change_future = t.await_event(&new_folder_path, Added);
        mkdir(&new_folder_path).await;
        must(change_future).await;
        assert_eq!(get(&subscriptions1, &new_folder_path), Some(Added));
        assert_eq!(get(&subscriptions2, &new_folder_path), None /* subscription was disposed before the event */);

        // Rename file
        let mut renamed_file_path = t.dir.join(&["deep", "renamedFile.txt"]);
        disposables.add(instance.subscribe(&renamed_file_path, record(&subscriptions1)));
        let change_future = vec![t.await_event(&new_file_path, Deleted), t.await_event(&renamed_file_path, Added)];
        rename(&new_file_path, &renamed_file_path).await;
        must_all(change_future.into_iter().map(|f| f.then(|_| ())).collect()).await;
        assert_eq!(get(&subscriptions1, &new_file_path), Some(Deleted));
        assert_eq!(get(&subscriptions1, &renamed_file_path), Some(Added));

        // Rename folder
        let mut renamed_folder_path = t.dir.join(&["deep", "Renamed Folder"]);
        disposables.add(instance.subscribe(&renamed_folder_path, record(&subscriptions1)));
        let change_future = vec![t.await_event(&new_folder_path, Deleted), t.await_event(&renamed_folder_path, Added)];
        rename(&new_folder_path, &renamed_folder_path).await;
        must_all(change_future.into_iter().map(|f| f.then(|_| ())).collect()).await;
        assert_eq!(get(&subscriptions1, &new_folder_path), Some(Deleted));
        assert_eq!(get(&subscriptions1, &renamed_folder_path), Some(Added));

        // Rename file (same name, different case)
        let case_renamed_file_path = t.dir.join(&["deep", "RenamedFile.txt"]);
        let change_future = vec![t.await_event(&renamed_file_path, Deleted), t.await_event(&case_renamed_file_path, Added)];
        rename(&renamed_file_path, &case_renamed_file_path).await;
        must_all(change_future.into_iter().map(|f| f.then(|_| ())).collect()).await;
        renamed_file_path = case_renamed_file_path;

        // Rename folder (same name, different case)
        let case_renamed_folder_path = t.dir.join(&["deep", "REnamed Folder"]);
        let change_future = vec![t.await_event(&renamed_folder_path, Deleted), t.await_event(&case_renamed_folder_path, Added)];
        rename(&renamed_folder_path, &case_renamed_folder_path).await;
        must_all(change_future.into_iter().map(|f| f.then(|_| ())).collect()).await;
        renamed_folder_path = case_renamed_folder_path;

        // Move file
        let moved_filepath = t.dir.join(&["movedFile.txt"]);
        let change_future = vec![t.await_event(&renamed_file_path, Deleted), t.await_event(&moved_filepath, Added)];
        rename(&renamed_file_path, &moved_filepath).await;
        must_all(change_future.into_iter().map(|f| f.then(|_| ())).collect()).await;

        // Move folder
        let moved_folderpath = t.dir.join(&["Moved Folder"]);
        let change_future = vec![t.await_event(&renamed_folder_path, Deleted), t.await_event(&moved_folderpath, Added)];
        rename(&renamed_folder_path, &moved_folderpath).await;
        must_all(change_future.into_iter().map(|f| f.then(|_| ())).collect()).await;

        // Copy file
        let copied_filepath = t.dir.join(&["deep", "copiedFile.txt"]);
        let change_future = t.await_event(&copied_filepath, Added);
        copy_file(&moved_filepath, &copied_filepath).await;
        must(change_future).await;

        // Copy folder
        let copied_folderpath = t.dir.join(&["deep", "Copied Folder"]);
        let change_future = t.await_event(&copied_folderpath, Added);
        copy(&moved_folderpath, &copied_folderpath).await;
        must(change_future).await;

        // Change file
        let change_future = t.await_event(&copied_filepath, Updated);
        write_file(&copied_filepath, "Hello Change").await;
        must(change_future).await;

        // Create new file
        let another_new_file_path = t.dir.join(&["deep", "anotherNewFile.txt"]);
        let change_future = t.await_event(&another_new_file_path, Added);
        write_file(&another_new_file_path, "Hello Another World").await;
        must(change_future).await;

        // Read file does not emit event
        let change_future = t.await_event_with(&another_new_file_path, Updated, Some("unexpected-event-from-read-file"), NO_CID, None);
        io().await;
        std::fs::read(&another_new_file_path).expect("readFile");
        must(Promise::race(vec![timeout(100).then(|()| Vec::new()), change_future])).await;

        // Stat file does not emit event
        let change_future = t.await_event_with(&another_new_file_path, Updated, Some("unexpected-event-from-stat"), NO_CID, None);
        io().await;
        std::fs::metadata(&another_new_file_path).expect("stat");
        must(Promise::race(vec![timeout(100).then(|()| Vec::new()), change_future])).await;

        // Stat folder does not emit event
        let change_future = t.await_event_with(&copied_folderpath, Updated, Some("unexpected-event-from-stat"), NO_CID, None);
        io().await;
        std::fs::metadata(&copied_folderpath).expect("stat");
        must(Promise::race(vec![timeout(100).then(|()| Vec::new()), change_future])).await;

        // Delete file
        let change_future = t.await_event(&copied_filepath, Deleted);
        disposables.add(instance.subscribe(&copied_filepath, record(&subscriptions1)));
        unlink(&copied_filepath).await;
        must(change_future).await;
        assert_eq!(get(&subscriptions1, &copied_filepath), Some(Deleted));

        // Delete folder
        let change_future = t.await_event(&copied_folderpath, Deleted);
        disposables.add(instance.subscribe(&copied_folderpath, record(&subscriptions1)));
        rmdir(&copied_folderpath).await;
        must(change_future).await;
        assert_eq!(get(&subscriptions1, &copied_folderpath), Some(Deleted));

        disposables.dispose();
    });
}

/// `change.resource.fsPath`.
fn je_platform_fs_path(change: &FileChange) -> String {
    use je_base::path::{Environment, ProcessEnvironment};
    change.resource.fs_path(ProcessEnvironment.platform()).to_string_lossy()
}

#[test]
#[cfg_attr(target_os = "macos", ignore = "test.skip no macOS: não é possível com o backend fsevents, como no original")]
fn basics_atomic_writes() {
    run(|t| async move {
        t.watch(vec![req(&t.dir.path)]).await;

        // Delete + Recreate file
        let new_file_path = t.dir.join(&["deep", "conway.js"]);
        let change_future = t.await_event(&new_file_path, Updated);
        unlink(&new_file_path).await;
        let path = new_file_path.clone();
        detach(async move { write_file(&path, "Hello Atomic World").await });
        must(change_future).await;
    });
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore = "test.skip fora do Linux: o polling só é usado em ambientes Linux (WSL), como no original")]
fn basics_polling() {
    run(|t| async move {
        t.watch(vec![WatchRequest { polling_interval: Some(100), ..req(&t.dir.path) }]).await;

        t.crud(&t.dir.join(&["deep", "newFile.txt"])).await;
    });
}

#[test]
fn multiple_events() {
    run(|t| async move {
        t.watch(vec![req(&t.dir.path)]).await;
        mkdir(&t.dir.join(&["deep-multiple"])).await;

        // multiple add

        let new_file_path1 = t.dir.join(&["newFile-1.txt"]);
        let new_file_path2 = t.dir.join(&["newFile-2.txt"]);
        let new_file_path3 = t.dir.join(&["newFile-3.txt"]);
        let new_file_path4 = t.dir.join(&["deep-multiple", "newFile-1.txt"]);
        let new_file_path5 = t.dir.join(&["deep-multiple", "newFile-2.txt"]);
        let new_file_path6 = t.dir.join(&["deep-multiple", "newFile-3.txt"]);
        let paths = [&new_file_path1, &new_file_path2, &new_file_path3, &new_file_path4, &new_file_path5, &new_file_path6];

        let futures: Vec<_> = paths.iter().map(|p| t.await_event(p, Added).then(|_| ())).collect();

        for (i, path) in paths.iter().enumerate() {
            write_file(path, &format!("Hello World {}", i + 1)).await;
        }

        must_all(futures).await;

        // multiple change

        let futures: Vec<_> = paths.iter().map(|p| t.await_event(p, Updated).then(|_| ())).collect();

        for (i, path) in paths.iter().enumerate() {
            write_file(path, &format!("Hello Update {}", i + 1)).await;
        }

        must_all(futures).await;

        // copy with multiple files

        let copy_futures: Vec<_> = [
            t.dir.join(&["deep-multiple-copy", "newFile-1.txt"]),
            t.dir.join(&["deep-multiple-copy", "newFile-2.txt"]),
            t.dir.join(&["deep-multiple-copy", "newFile-3.txt"]),
            t.dir.join(&["deep-multiple-copy"]),
        ]
        .iter()
        .map(|p| t.await_event(p, Added).then(|_| ()))
        .collect();

        copy(&t.dir.join(&["deep-multiple"]), &t.dir.join(&["deep-multiple-copy"])).await;

        must_all(copy_futures).await;

        // multiple delete (single files)

        let futures: Vec<_> = paths.iter().map(|p| t.await_event(p, Deleted).then(|_| ())).collect();

        for path in paths {
            unlink(path).await;
        }

        must_all(futures).await;

        // multiple delete (folder)

        let delete_folder_futures: Vec<_> =
            [t.dir.join(&["deep-multiple"]), t.dir.join(&["deep-multiple-copy"])].iter().map(|p| t.await_event(p, Deleted).then(|_| ())).collect();

        rm(&t.dir.join(&["deep-multiple"])).await;
        rm(&t.dir.join(&["deep-multiple-copy"])).await;

        must_all(delete_folder_futures).await;
    });
}

#[test]
fn subsequent_watch_updates_watchers_path() {
    run(|t| async move {
        let unrelated = join(&[&realpath(&t.dir.path), "unrelated"]);
        t.watch(vec![WatchRequest { excludes: vec![unrelated.clone()], ..req(&t.dir.path) }]).await;

        // New file (*.txt)
        let mut new_text_file_path = t.dir.join(&["deep", "newFile.txt"]);
        let mut change_future = t.await_event(&new_text_file_path, Added);
        write_file(&new_text_file_path, "Hello World").await;
        must(change_future).await;

        t.watch(vec![WatchRequest { excludes: vec![unrelated.clone()], ..req(&t.dir.join(&["deep"])) }]).await;
        new_text_file_path = t.dir.join(&["deep", "newFile2.txt"]);
        change_future = t.await_event(&new_text_file_path, Added);
        write_file(&new_text_file_path, "Hello World").await;
        must(change_future).await;

        t.watch(vec![WatchRequest { excludes: vec![realpath(&t.dir.path)], ..req(&t.dir.join(&["deep"])) }]).await;
        t.watch(vec![req(&t.dir.join(&["deep"]))]).await;
        new_text_file_path = t.dir.join(&["deep", "newFile3.txt"]);
        change_future = t.await_event(&new_text_file_path, Added);
        write_file(&new_text_file_path, "Hello World").await;
        must(change_future).await;
    });
}

#[test]
fn invalid_path_does_not_crash_watcher() {
    run(|t| async move {
        // `FileAccess.asFileUri('').fsPath`: um segundo diretório válido, fora do de teste.
        let other = TestDir::new();
        t.watch(vec![req(&t.dir.path), req(&t.dir.join(&["invalid-folder"])), req(&other.path)]).await;

        t.crud(&t.dir.join(&["deep", "newFile.txt"])).await;
    });
}

#[test]
fn subsequent_watch_updates_watchers_excludes() {
    run(|t| async move {
        t.watch(vec![WatchRequest { excludes: vec![realpath(&t.dir.path)], ..req(&t.dir.path) }]).await;
        t.watch(vec![req(&t.dir.path)]).await;

        t.crud(&t.dir.join(&["deep", "newFile.txt"])).await;
    });
}

#[test]
fn subsequent_watch_updates_watchers_includes() {
    run(|t| async move {
        t.watch(vec![WatchRequest { includes: Some(vec!["nothing".into()]), ..req(&t.dir.path) }]).await;
        t.watch(vec![req(&t.dir.path)]).await;

        t.crud(&t.dir.join(&["deep", "newFile.txt"])).await;
    });
}

#[test]
fn includes_are_supported() {
    run(|t| async move {
        t.watch(vec![WatchRequest { includes: Some(vec!["**/deep/**".into()]), ..req(&t.dir.path) }]).await;

        t.crud(&t.dir.join(&["deep", "newFile.txt"])).await;
    });
}

#[test]
fn includes_are_supported_relative_pattern_explicit() {
    run(|t| async move {
        let includes = vec![PatternItem::Relative(RelativePattern { base: t.dir.path.clone(), pattern: "deep/newFile.txt".to_string() })];
        t.watch(vec![WatchRequest { includes: Some(includes), ..req(&t.dir.path) }]).await;

        t.crud(&t.dir.join(&["deep", "newFile.txt"])).await;
    });
}

#[test]
fn includes_are_supported_relative_pattern_implicit() {
    run(|t| async move {
        t.watch(vec![WatchRequest { includes: Some(vec!["deep/newFile.txt".into()]), ..req(&t.dir.path) }]).await;

        t.crud(&t.dir.join(&["deep", "newFile.txt"])).await;
    });
}

#[test]
fn excludes_are_supported_path() {
    run(|t| async move { t.test_excludes(vec![join(&[&realpath(&t.dir.path), "deep"])]).await });
}

#[test]
fn excludes_are_supported_glob() {
    run(|t| async move { t.test_excludes(vec!["deep/**".to_string()]).await });
}

#[test]
#[cfg_attr(windows, ignore = "test.skip no Windows: não cria link simbólico de arquivo sem contexto elevado, como no original")]
fn symlink_support_root() {
    run(|t| async move {
        let link = t.dir.join(&["deep-linked"]);
        let link_target = t.dir.join(&["deep"]);
        symlink(&link_target, &link);

        t.watch(vec![req(&link)]).await;

        t.crud(&join(&[&link, "newFile.txt"])).await;
    });
}

#[test]
#[cfg_attr(windows, ignore = "test.skip no Windows: não cria link simbólico de arquivo sem contexto elevado, como no original")]
fn symlink_support_via_extra_watch() {
    run(|t| async move {
        let link = t.dir.join(&["deep-linked"]);
        let link_target = t.dir.join(&["deep"]);
        symlink(&link_target, &link);

        t.watch(vec![req(&t.dir.path), req(&link)]).await;

        t.crud(&join(&[&link, "newFile.txt"])).await;
    });
}

#[test]
#[cfg_attr(not(windows), ignore = "test.skip fora do Windows: UNC só existe lá, como no original")]
fn unc_support() {
    run(|t| async move {
        // `addUNCHostToAllowlist('localhost')`: a lista de hosts UNC não existe no porte.
        let drive = je_base::extpath::get_drive_letter(&t.dir.path, true).map(|c| c.to_ascii_lowercase()).unwrap_or('c');
        let rest = t.dir.path.split_once(':').map(|(_, rest)| rest).unwrap_or(&t.dir.path).trim_start_matches('\\').to_string();
        let unc_path = format!("\\\\localhost\\{drive}$\\{rest}");

        t.watch(vec![req(&unc_path)]).await;

        t.crud(&join(&[&unc_path, "deep", "newFile.txt"])).await;
    });
}

#[test]
#[cfg_attr(target_os = "linux", ignore = "test.skip no Linux: o sistema de arquivos distingue maiúsculas e minúsculas, como no original")]
fn wrong_casing() {
    run(|t| async move {
        let deep_wrong_cased_path = t.dir.join(&["DEEP"]);

        t.watch(vec![req(&deep_wrong_cased_path)]).await;

        t.crud(&join(&[&deep_wrong_cased_path, "newFile.txt"])).await;
    });
}

#[test]
fn invalid_folder_does_not_explode() {
    run(|t| async move {
        let invalid_path = t.dir.join(&["invalid"]);

        t.watch(vec![req(&invalid_path)]).await;
    });
}

#[test]
#[cfg_attr(windows, ignore = "test.skip no Windows: instável lá, como no original")]
fn deleting_watched_path_without_correlation_restarts_watching() {
    run(|t| async move {
        let watched_path = t.dir.join(&["deep"]);

        t.watch(vec![req(&watched_path)]).await;

        // Delete watched path and await
        let warn_future = t.await_message(LogMessageType::Warn);
        rm(&watched_path).await;
        must(warn_future).await;

        // Restore watched path
        must(timeout(1500)).await; // node.js watcher used for monitoring folder restore is async
        mkdir(&watched_path).await;
        must(timeout(1500)).await; // restart is delayed
        must(WatcherBackend::when_ready(&*t.watcher)).await;

        // Verify events come in again
        let new_file_path = join(&[&watched_path, "newFile.txt"]);
        let change_future = t.await_event(&new_file_path, Added);
        write_file(&new_file_path, "Hello World").await;
        must(change_future).await;
    });
}

#[test]
fn correlation_id_is_supported() {
    run(|t| async move {
        let correlation_id = 7_654_321;
        t.watch(vec![WatchRequest { correlation_id: Some(correlation_id), ..req(&t.dir.path) }]).await;

        t.basic_crud_test(&t.dir.join(&["newFile.txt"]), Cid::Exactly(Some(correlation_id)), None).await;
    });
}

#[test]
fn should_not_exclude_roots_that_do_not_overlap() {
    run(|t| async move {
        if cfg!(windows) {
            assert_eq!(test_remove_duplicate_requests(&t.watcher, &["C:\\a"], &[]), vec!["C:\\a"]);
            assert_eq!(test_remove_duplicate_requests(&t.watcher, &["C:\\a", "C:\\b"], &[]), vec!["C:\\a", "C:\\b"]);
            assert_eq!(test_remove_duplicate_requests(&t.watcher, &["C:\\a", "C:\\b", "C:\\c\\d\\e"], &[]), vec!["C:\\a", "C:\\b", "C:\\c\\d\\e"]);
        } else {
            assert_eq!(test_remove_duplicate_requests(&t.watcher, &["/a"], &[]), vec!["/a"]);
            assert_eq!(test_remove_duplicate_requests(&t.watcher, &["/a", "/b"], &[]), vec!["/a", "/b"]);
            assert_eq!(test_remove_duplicate_requests(&t.watcher, &["/a", "/b", "/c/d/e"], &[]), vec!["/a", "/b", "/c/d/e"]);
        }
    });
}

#[test]
fn should_remove_sub_folders_of_other_paths() {
    run(|t| async move {
        if cfg!(windows) {
            assert_eq!(test_remove_duplicate_requests(&t.watcher, &["C:\\a", "C:\\a\\b"], &[]), vec!["C:\\a"]);
            assert_eq!(test_remove_duplicate_requests(&t.watcher, &["C:\\a", "C:\\b", "C:\\a\\b"], &[]), vec!["C:\\a", "C:\\b"]);
            assert_eq!(test_remove_duplicate_requests(&t.watcher, &["C:\\b\\a", "C:\\a", "C:\\b", "C:\\a\\b"], &[]), vec!["C:\\a", "C:\\b"]);
            assert_eq!(test_remove_duplicate_requests(&t.watcher, &["C:\\a", "C:\\a\\b", "C:\\a\\c\\d"], &[]), vec!["C:\\a"]);
        } else {
            assert_eq!(test_remove_duplicate_requests(&t.watcher, &["/a", "/a/b"], &[]), vec!["/a"]);
            assert_eq!(test_remove_duplicate_requests(&t.watcher, &["/a", "/b", "/a/b"], &[]), vec!["/a", "/b"]);
            assert_eq!(test_remove_duplicate_requests(&t.watcher, &["/b/a", "/a", "/b", "/a/b"], &[]), vec!["/a", "/b"]);
            assert_eq!(test_remove_duplicate_requests(&t.watcher, &["/a", "/a/b", "/a/c/d"], &[]), vec!["/a"]);
        }
    });
}

#[test]
fn should_ignore_when_everything_excluded() {
    run(|t| async move {
        assert_eq!(test_remove_duplicate_requests(&t.watcher, &["/foo/bar", "/bar"], &["**", "something"]), Vec::<String>::new());
    });
}

#[test]
fn watching_same_or_overlapping_paths_supported_when_correlation_is_applied() {
    run(|t| async move {
        let correlated = |path: &str, id: Option<i64>, excludes: Vec<String>| WatchRequest { correlation_id: id, excludes, ..req(path) };
        t.watch(vec![correlated(&t.dir.path, Some(1), vec![])]).await;

        t.basic_crud_test(&t.dir.join(&["newFile.txt"]), Cid::Any, Some(1)).await;

        // same path, same options
        t.watch(vec![correlated(&t.dir.path, Some(1), vec![]), correlated(&t.dir.path, Some(2), vec![]), correlated(&t.dir.path, None, vec![])]).await;

        t.basic_crud_test(&t.dir.join(&["newFile.txt"]), Cid::Any, Some(3)).await;
        t.basic_crud_test(&t.dir.join(&["otherNewFile.txt"]), Cid::Any, Some(3)).await;

        // same path, different options
        let real = realpath(&t.dir.path);
        t.watch(vec![
            correlated(&t.dir.path, Some(1), vec![]),
            correlated(&t.dir.path, Some(2), vec![]),
            correlated(&t.dir.path, None, vec![]),
            correlated(&t.dir.path, Some(3), vec![join(&[&real, "deep"])]),
            correlated(&t.dir.path, Some(4), vec![join(&[&real, "other"])]),
        ])
        .await;

        t.basic_crud_test(&t.dir.join(&["newFile.txt"]), Cid::Any, Some(5)).await;
        t.basic_crud_test(&t.dir.join(&["otherNewFile.txt"]), Cid::Any, Some(5)).await;

        // overlapping paths (same options)
        t.watch(vec![
            correlated(&dirname(&t.dir.path), Some(1), vec![]),
            correlated(&t.dir.path, Some(2), vec![]),
            correlated(&t.dir.join(&["deep"]), Some(3), vec![]),
        ])
        .await;

        t.basic_crud_test(&t.dir.join(&["deep", "newFile.txt"]), Cid::Any, Some(3)).await;
        t.basic_crud_test(&t.dir.join(&["deep", "otherNewFile.txt"]), Cid::Any, Some(3)).await;

        // overlapping paths (different options)
        t.watch(vec![
            correlated(&dirname(&t.dir.path), Some(1), vec![]),
            correlated(&t.dir.path, Some(2), vec![join(&[&real, "some"])]),
            correlated(&t.dir.join(&["deep"]), Some(3), vec![join(&[&real, "other"])]),
        ])
        .await;

        t.basic_crud_test(&t.dir.join(&["deep", "newFile.txt"]), Cid::Any, Some(3)).await;
        t.basic_crud_test(&t.dir.join(&["deep", "otherNewFile.txt"]), Cid::Any, Some(3)).await;
    });
}

#[test]
fn watching_missing_path_emits_watcher_fail_event() {
    run(|t| async move {
        let on_did_watch_fail = t.on_watch_fail();

        let folder_path = t.dir.join(&["missing"]);
        let _ = t.watcher.watch(vec![req(&folder_path)]);

        must(on_did_watch_fail).await;
    });
}

#[test]
fn deleting_watched_path_emits_watcher_fail_and_delete_event_if_correlated() {
    run(|t| async move {
        let folder_path = t.dir.join(&["deep"]);

        t.watch(vec![WatchRequest { correlation_id: Some(1), ..req(&folder_path) }]).await;

        let failed = Rc::new(Cell::new(false));
        let instance = t.watcher.watchers()[0].clone();
        assert!(instance.include(&folder_path));
        let flag = failed.clone();
        let subscription = instance.on_did_fail.event().listen(move |()| flag.set(true));

        let on_did_watch_fail = t.on_watch_fail();
        let change_future = t.await_event_with(&folder_path, Deleted, None, Cid::Exactly(Some(1)), None);
        let path = folder_path.clone();
        detach(async move { rm(&path).await });
        must(on_did_watch_fail).await;
        must(change_future).await;
        assert!(failed.get());
        assert!(instance.failed());
        subscription.dispose();
    });
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "test.skip fora do macOS: \"Linux/Windows: times out for some reason\", como no original")]
fn watch_requests_support_suspend_resume_folder_does_not_exist_in_beginning_not_reusing_watcher() {
    run(|t| async move { test_watch_folder_does_not_exist(&t, false).await });
}

#[test]
fn watch_requests_support_suspend_resume_folder_does_not_exist_in_beginning_reusing_watcher() {
    run(|t| async move { test_watch_folder_does_not_exist(&t, true).await });
}

async fn test_watch_folder_does_not_exist(t: &Test, reuse_existing_watcher: bool) {
    let mut on_did_watch_fail = t.on_watch_fail();

    let folder_path = t.dir.join(&["not-found"]);

    let mut requests = Vec::new();
    if reuse_existing_watcher {
        requests.push(req(&t.dir.path));
        t.watch(requests.clone()).await;
    }

    let request = req(&folder_path);
    requests.push(request.clone());

    t.watch(requests).await;
    must(on_did_watch_fail).await;

    if reuse_existing_watcher {
        assert_eq!(t.is_suspended(&request), Suspended::Yes);
    } else {
        assert_eq!(t.is_suspended(&request), Suspended::Polling);
    }

    let mut change_future = t.await_event(&folder_path, Added);
    let mut on_did_watch = t.on_did_watch();
    mkdir(&folder_path).await;
    must(change_future).await;
    must(on_did_watch).await;

    assert_eq!(t.is_suspended(&request), Suspended::No);

    let file_path = join(&[&folder_path, "newFile.txt"]);
    t.crud(&file_path).await;

    if !reuse_existing_watcher {
        on_did_watch_fail = t.on_watch_fail();
        rm(&folder_path).await;
        must(on_did_watch_fail).await;

        change_future = t.await_event(&folder_path, Added);
        on_did_watch = t.on_did_watch();
        mkdir(&folder_path).await;
        must(change_future).await;
        must(on_did_watch).await;

        t.crud(&file_path).await;
    }
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "test.skip fora do macOS: \"Linux/Windows: times out for some reason\", como no original")]
fn watch_requests_support_suspend_resume_folder_exist_in_beginning_not_reusing_watcher() {
    run(|t| async move { test_watch_folder_exists(&t, false).await });
}

#[test]
fn watch_requests_support_suspend_resume_folder_exist_in_beginning_reusing_watcher() {
    run(|t| async move { test_watch_folder_exists(&t, true).await });
}

async fn test_watch_folder_exists(t: &Test, reuse_existing_watcher: bool) {
    let folder_path = t.dir.join(&["deep"]);

    let mut requests = vec![req(&folder_path)];
    if reuse_existing_watcher {
        requests.push(req(&t.dir.path));
    }

    t.watch(requests).await;

    let file_path = join(&[&folder_path, "newFile.txt"]);
    t.crud(&file_path).await;

    if !reuse_existing_watcher {
        let on_did_watch_fail = t.on_watch_fail();
        rm(&folder_path).await;
        must(on_did_watch_fail).await;

        let change_future = t.await_event(&folder_path, Added);
        let on_did_watch = t.on_did_watch();
        mkdir(&folder_path).await;
        must(change_future).await;
        must(on_did_watch).await;

        t.crud(&file_path).await;
    }
}

#[test]
fn watch_request_reuses_another_recursive_watcher_even_when_requests_are_coming_in_at_the_same_time() {
    run(|t| async move {
        let folder_path1 = t.dir.join(&["deep", "not-existing1"]);
        let folder_path2 = t.dir.join(&["deep", "not-existing2"]);
        let folder_path3 = t.dir.join(&["not-existing3"]);

        let requests = vec![
            WatchRequest { correlation_id: Some(1), ..req(&folder_path1) },
            WatchRequest { correlation_id: Some(2), ..req(&folder_path2) },
            WatchRequest { correlation_id: Some(3), ..req(&folder_path3) },
            req(&t.dir.join(&["deep"])),
        ];

        t.watch(requests.clone()).await;

        assert_eq!(t.is_suspended(&requests[0]), Suspended::Yes);
        assert_eq!(t.is_suspended(&requests[1]), Suspended::Yes);
        assert_eq!(t.is_suspended(&requests[2]), Suspended::Polling);
        assert_eq!(t.is_suspended(&requests[3]), Suspended::No);
    });
}

#[test]
fn event_type_filter() {
    run(|t| async move {
        let request = WatchRequest { filter: Some(FileChangeFilter::ADDED | FileChangeFilter::DELETED), correlation_id: Some(1), ..req(&t.dir.path) };
        t.watch(vec![request]).await;

        // Change file
        let file_path = t.dir.join(&["lorem-newfile.txt"]);
        let change_future = t.await_event_with(&file_path, Added, None, Cid::Exactly(Some(1)), None);
        write_file(&file_path, "Hello Change").await;
        must(change_future).await;

        // Delete file
        let change_future = t.await_event_with(&file_path, Deleted, None, Cid::Exactly(Some(1)), None);
        unlink(&file_path).await;
        must(change_future).await;
    });
}

/// O corpo dos dois casos `includes are case insensitive on Windows/Mac` (idênticos no original).
async fn includes_are_case_insensitive(t: &Test) {
    t.watch(vec![WatchRequest { includes: Some(vec!["**/*.TXT".into()]), ..req(&t.dir.path) }]).await;

    // New file (matches *.TXT case-insensitively)
    let new_file_path = t.dir.join(&["deep", "newFile.txt"]);
    let mut change_future = t.await_event(&new_file_path, Added);
    write_file(&new_file_path, "Hello World").await;
    must(change_future).await;

    // Change file
    change_future = t.await_event(&new_file_path, Updated);
    write_file(&new_file_path, "Hello Change").await;
    must(change_future).await;

    // Delete file
    change_future = t.await_event(&new_file_path, Deleted);
    unlink(&new_file_path).await;
    must(change_future).await;
}

/// O corpo dos dois casos `excludes are case insensitive on Windows/Mac` (idênticos no original).
async fn excludes_are_case_insensitive(t: &Test) {
    t.watch(vec![WatchRequest { excludes: vec!["**/DEEP/**".into()], ..req(&t.dir.path) }]).await;

    // New file in excluded folder (should not trigger event)
    let new_text_file_path = t.dir.join(&["deep", "newFile.txt"]);
    let change_future = t.await_event(&new_text_file_path, Added);
    write_file(&new_text_file_path, "Hello World").await;

    let res = must(Promise::race(vec![timeout(500).then(|()| true), change_future.then(|_| false)])).await;

    if !res {
        panic!("Unexpected change event");
    }
}

#[test]
#[cfg_attr(target_os = "linux", ignore = "test.skip no Linux: includes só ignoram a caixa no Windows e no macOS, como no original")]
fn includes_are_case_insensitive_on_windows_mac() {
    run(|t| async move { includes_are_case_insensitive(&t).await });
}

#[test]
#[cfg_attr(target_os = "linux", ignore = "test.skip no Linux: includes só ignoram a caixa no Windows e no macOS, como no original (o arquivo repete este caso)")]
fn includes_are_case_insensitive_on_windows_mac_repeated() {
    run(|t| async move { includes_are_case_insensitive(&t).await });
}

#[test]
#[cfg_attr(target_os = "linux", ignore = "test.skip no Linux: excludes só ignoram a caixa no Windows e no macOS, como no original")]
fn excludes_are_case_insensitive_on_windows_mac() {
    run(|t| async move { excludes_are_case_insensitive(&t).await });
}

#[test]
#[cfg_attr(target_os = "linux", ignore = "test.skip no Linux: excludes só ignoram a caixa no Windows e no macOS, como no original (o arquivo repete este caso)")]
fn excludes_are_case_insensitive_on_windows_mac_repeated() {
    run(|t| async move { excludes_are_case_insensitive(&t).await });
}

// endregion
