//! Porte de `src/vs/platform/files/test/node/nodejsWatcher.test.ts` (suíte `File Watcher (node.js)`), caso a caso e na mesma ordem.
//!
//! A suíte inteira é `suite.skip` no original ("flaky em pipelines do Azure, roda sob demanda quando a
//! biblioteca do watcher muda"); aqui ela roda, com as esperas por condição do loop (nunca `sleep` fixo) e
//! o prazo de 10 s do `this.timeout(10000)`. O original tem 40 casos (a tarefa falava em 27); todos estão
//! aqui, na ordem do arquivo.
//!
//! Os casos que o original pula por plataforma (`isWindows ? test.skip : test` e afins) viram `#[ignore]`
//! condicional no mesmo critério, mantendo a contagem. Pendente neste host (Linux): `unc support` (2),
//! `wrong casing` (2), `recreating a deleted watched folder...` e os três `... case insensitive on
//! Windows/Mac`, que só existem no Windows e no macOS e não puderam ser executados aqui.
//!
//! Divergências: as fixtures (`fixtures/service`) são geradas por `common::watcher::TestDir` em vez de
//! copiadas de `ref/`; os `await Promises.x()` de disco são chamadas bloqueantes (o loop gira nas esperas
//! por evento); o `correlationId = Math.random()` é um inteiro fixo (o `WatchRequest` guarda `i64`).

mod common;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use common::watcher::*;
use je_base::cancellation::CancellationTokenSource;
use je_base::lifecycle::IDisposable;
use je_base::promise::{DeferredPromise, Promise};
use je_platform::files::files::{FileChange, FileChangeFilter, FileChangeType};
use je_platform::files::node_watcher::base_watcher::Suspended;
use je_platform::files::node_watcher::nodejs::{NodeJsWatcher, watch_file_contents};
use je_platform::files::node_watcher::parcel::ParcelWatcher;
use je_platform::files::watcher::{RecursiveWatcherWithSubscribe, WatchRequest, Watcher};

use FileChangeType::{Added, Deleted, Updated};
use je_base::glob::{PatternItem, RelativePattern};

const NO_CID: Cid = Cid::Exactly(None);

/// O estado que o `setup`/`teardown` do original mantêm.
struct Test {
    dir: TestDir,
    watcher: RefCell<Rc<NodeJsWatcher>>,
}

impl Test {
    fn watcher(&self) -> Rc<NodeJsWatcher> {
        self.watcher.borrow().clone()
    }

    /// `createWatcher(accessor)`.
    async fn create_watcher(&self, accessor: Option<Rc<dyn RecursiveWatcherWithSubscribe>>) {
        let old = self.watcher();
        must(old.stop()).await;
        old.dispose();

        *self.watcher.borrow_mut() = test_nodejs_watcher(accessor);
    }

    fn await_event(&self, path: &str, change_type: FileChangeType) -> Promise<Vec<FileChange>> {
        self.await_event_with(path, change_type, NO_CID, None)
    }

    fn await_event_with(&self, path: &str, change_type: FileChangeType, cid: Cid, expected_count: Option<usize>) -> Promise<Vec<FileChange>> {
        await_event_on(self.watcher().on_did_change_file(), path, change_type, cid, expected_count)
    }

    fn on_watch_fail(&self) -> Promise<WatchRequest> {
        to_promise(&self.watcher().base().on_did_watch_fail.event())
    }

    fn on_did_watch(&self) -> Promise<()> {
        to_promise(&self.watcher().base().on_did_watch.event())
    }

    fn is_suspended(&self, request: &WatchRequest) -> Suspended {
        self.watcher().base().is_suspended(request)
    }

    async fn watch(&self, requests: Vec<WatchRequest>) {
        must(self.watcher().watch(requests)).await;
    }

    async fn basic_crud_test(&self, file_path: &str, skip_add: bool, cid: Cid, expected_count: Option<usize>, await_watch_after_add: bool) {
        // New file
        if !skip_add {
            let change_future = self.await_event_with(file_path, Added, cid, expected_count);
            write_file(file_path, "Hello World").await;
            must(change_future).await;
            if await_watch_after_add {
                must(self.on_did_watch()).await;
            }
        }

        // Change file
        let change_future = self.await_event_with(file_path, Updated, cid, expected_count);
        write_file(file_path, "Hello Change").await;
        must(change_future).await;

        // Delete file
        let change_future = self.await_event_with(file_path, Deleted, cid, expected_count);
        unlink(&realpath(file_path)).await; // support symlinks
        must(change_future).await;
    }

    async fn crud(&self, file_path: &str) {
        self.basic_crud_test(file_path, false, NO_CID, None, false).await;
    }
}

/// `setup` + corpo + `teardown`.
fn run<F, Fut>(body: F)
where
    F: FnOnce(Rc<Test>) -> Fut,
    Fut: Future<Output = ()> + 'static,
{
    let test = Rc::new(Test { dir: TestDir::new(), watcher: RefCell::new(test_nodejs_watcher(None)) });
    let teardown = test.clone();
    let future = body(test);
    run_async_test(async move {
        future.await;
        must(teardown.watcher().stop()).await;
        teardown.watcher().dispose();
    });
}

fn req(path: &str) -> WatchRequest {
    request(path, false)
}

#[test]
fn basics_folder_watch() {
    run(|t| async move {
        let request = req(&t.dir.path);
        t.watch(vec![request.clone()]).await;
        assert_eq!(t.is_suspended(&request), Suspended::No);

        let instance = t.watcher().watchers()[0].instance.clone();
        assert!(!instance.is_reusing_recursive_watcher());
        assert!(!instance.failed());

        // New file
        let new_file_path = t.dir.join(&["newFile.txt"]);
        let change_future = t.await_event(&new_file_path, Added);
        write_file(&new_file_path, "Hello World").await;
        must(change_future).await;

        // New folder
        let new_folder_path = t.dir.join(&["New Folder"]);
        let change_future = t.await_event(&new_folder_path, Added);
        mkdir(&new_folder_path).await;
        must(change_future).await;

        // Rename file
        let mut renamed_file_path = t.dir.join(&["renamedFile.txt"]);
        let change_future = vec![t.await_event(&new_file_path, Deleted), t.await_event(&renamed_file_path, Added)];
        rename(&new_file_path, &renamed_file_path).await;
        must_all(change_future.into_iter().map(|f| f.then(|_| ())).collect()).await;

        // Rename folder
        let mut renamed_folder_path = t.dir.join(&["Renamed Folder"]);
        let change_future = vec![t.await_event(&new_folder_path, Deleted), t.await_event(&renamed_folder_path, Added)];
        rename(&new_folder_path, &renamed_folder_path).await;
        must_all(change_future.into_iter().map(|f| f.then(|_| ())).collect()).await;

        // Rename file (same name, different case)
        let case_renamed_file_path = t.dir.join(&["RenamedFile.txt"]);
        let change_future = vec![t.await_event(&renamed_file_path, Deleted), t.await_event(&case_renamed_file_path, Added)];
        rename(&renamed_file_path, &case_renamed_file_path).await;
        must_all(change_future.into_iter().map(|f| f.then(|_| ())).collect()).await;
        renamed_file_path = case_renamed_file_path;

        // Rename folder (same name, different case)
        let case_renamed_folder_path = t.dir.join(&["REnamed Folder"]);
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
        let copied_filepath = t.dir.join(&["copiedFile.txt"]);
        let change_future = t.await_event(&copied_filepath, Added);
        copy_file(&moved_filepath, &copied_filepath).await;
        must(change_future).await;

        // Copy folder
        let copied_folderpath = t.dir.join(&["Copied Folder"]);
        let change_future = t.await_event(&copied_folderpath, Added);
        copy(&moved_folderpath, &copied_folderpath).await;
        must(change_future).await;

        // Change file
        let change_future = t.await_event(&copied_filepath, Updated);
        write_file(&copied_filepath, "Hello Change").await;
        must(change_future).await;

        // Create new file
        let another_new_file_path = t.dir.join(&["anotherNewFile.txt"]);
        let change_future = t.await_event(&another_new_file_path, Added);
        write_file(&another_new_file_path, "Hello Another World").await;
        must(change_future).await;

        // Delete file
        let change_future = t.await_event(&copied_filepath, Deleted);
        unlink(&copied_filepath).await;
        must(change_future).await;

        // Delete folder
        let change_future = t.await_event(&copied_folderpath, Deleted);
        rmdir(&copied_folderpath).await;
        must(change_future).await;

        t.watcher().dispose();
    });
}

#[test]
fn basics_file_watch() {
    run(|t| async move {
        let file_path = t.dir.join(&["lorem.txt"]);
        let request = req(&file_path);
        t.watch(vec![request.clone()]).await;
        assert_eq!(t.is_suspended(&request), Suspended::No);

        let instance = t.watcher().watchers()[0].instance.clone();
        assert!(!instance.is_reusing_recursive_watcher());
        assert!(!instance.failed());

        // Change file
        let change_future = t.await_event(&file_path, Updated);
        write_file(&file_path, "Hello Change").await;
        must(change_future).await;

        // Delete file
        let change_future = t.await_event(&file_path, Deleted);
        unlink(&file_path).await;
        must(change_future).await;

        // Recreate watcher
        write_file(&file_path, "Hello Change").await;
        t.watch(vec![]).await;
        t.watch(vec![req(&file_path)]).await;

        // Move file
        let change_future = t.await_event(&file_path, Deleted);
        rename(&file_path, &format!("{file_path}-moved")).await;
        must(change_future).await;
    });
}

#[test]
fn folder_child_matching_watched_folder_basename() {
    run(|t| async move {
        let folder_path = t.dir.join(&["deep"]);
        t.watch(vec![req(&folder_path)]).await;

        t.crud(&t.dir.join(&["deep", "deep"])).await;
    });
}

#[test]
fn atomic_writes_folder_watch() {
    run(|t| async move {
        t.watch(vec![req(&t.dir.path)]).await;

        // Delete + Recreate file
        let new_file_path = t.dir.join(&["lorem.txt"]);
        let change_future = t.await_event(&new_file_path, Updated);
        unlink(&new_file_path).await;
        let path = new_file_path.clone();
        detach(async move { write_file(&path, "Hello Atomic World").await });
        must(change_future).await;
    });
}

#[test]
fn atomic_writes_file_watch() {
    run(|t| async move {
        let file_path = t.dir.join(&["lorem.txt"]);
        t.watch(vec![req(&file_path)]).await;

        // Delete + Recreate file
        let change_future = t.await_event(&file_path, Updated);
        unlink(&file_path).await;
        let path = file_path.clone();
        detach(async move { write_file(&path, "Hello Atomic World").await });
        must(change_future).await;
    });
}

#[test]
fn multiple_events_folder_watch() {
    run(|t| async move {
        t.watch(vec![req(&t.dir.path)]).await;

        // multiple add

        let new_file_path1 = t.dir.join(&["newFile-1.txt"]);
        let new_file_path2 = t.dir.join(&["newFile-2.txt"]);
        let new_file_path3 = t.dir.join(&["newFile-3.txt"]);

        let added = [t.await_event(&new_file_path1, Added), t.await_event(&new_file_path2, Added), t.await_event(&new_file_path3, Added)];

        write_file(&new_file_path1, "Hello World 1").await;
        write_file(&new_file_path2, "Hello World 2").await;
        write_file(&new_file_path3, "Hello World 3").await;

        must_all(added.into_iter().map(|f| f.then(|_| ())).collect()).await;

        // multiple change

        let changed = [t.await_event(&new_file_path1, Updated), t.await_event(&new_file_path2, Updated), t.await_event(&new_file_path3, Updated)];

        write_file(&new_file_path1, "Hello Update 1").await;
        write_file(&new_file_path2, "Hello Update 2").await;
        write_file(&new_file_path3, "Hello Update 3").await;

        must_all(changed.into_iter().map(|f| f.then(|_| ())).collect()).await;

        // copy with multiple files

        let copied = [
            t.await_event(&t.dir.join(&["newFile-1-copy.txt"]), Added),
            t.await_event(&t.dir.join(&["newFile-2-copy.txt"]), Added),
            t.await_event(&t.dir.join(&["newFile-3-copy.txt"]), Added),
        ];

        copy(&t.dir.join(&["newFile-1.txt"]), &t.dir.join(&["newFile-1-copy.txt"])).await;
        copy(&t.dir.join(&["newFile-2.txt"]), &t.dir.join(&["newFile-2-copy.txt"])).await;
        copy(&t.dir.join(&["newFile-3.txt"]), &t.dir.join(&["newFile-3-copy.txt"])).await;

        must_all(copied.into_iter().map(|f| f.then(|_| ())).collect()).await;

        // multiple delete

        let deleted = [t.await_event(&new_file_path1, Deleted), t.await_event(&new_file_path2, Deleted), t.await_event(&new_file_path3, Deleted)];

        unlink(&new_file_path1).await;
        unlink(&new_file_path2).await;
        unlink(&new_file_path3).await;

        must_all(deleted.into_iter().map(|f| f.then(|_| ())).collect()).await;
    });
}

#[test]
fn multiple_events_file_watch() {
    run(|t| async move {
        let file_path = t.dir.join(&["lorem.txt"]);
        t.watch(vec![req(&file_path)]).await;

        // multiple change

        let change_future1 = t.await_event(&file_path, Updated);

        write_file(&file_path, "Hello Update 1").await;
        write_file(&file_path, "Hello Update 2").await;
        write_file(&file_path, "Hello Update 3").await;

        must(change_future1).await;
    });
}

#[test]
fn excludes_can_be_updated_folder_watch() {
    run(|t| async move {
        t.watch(vec![WatchRequest { excludes: vec!["**".into()], ..req(&t.dir.path) }]).await;
        t.watch(vec![req(&t.dir.path)]).await;

        t.crud(&t.dir.join(&["files-excludes.txt"])).await;
    });
}

#[test]
fn excludes_are_ignored_file_watch() {
    run(|t| async move {
        let file_path = t.dir.join(&["lorem.txt"]);
        t.watch(vec![WatchRequest { excludes: vec!["**".into()], ..req(&file_path) }]).await;

        t.basic_crud_test(&file_path, true, NO_CID, None, false).await;
    });
}

#[test]
fn includes_can_be_updated_folder_watch() {
    run(|t| async move {
        t.watch(vec![WatchRequest { includes: Some(vec!["nothing".into()]), ..req(&t.dir.path) }]).await;
        t.watch(vec![req(&t.dir.path)]).await;

        t.crud(&t.dir.join(&["files-includes.txt"])).await;
    });
}

#[test]
fn non_includes_are_ignored_file_watch() {
    run(|t| async move {
        let file_path = t.dir.join(&["lorem.txt"]);
        t.watch(vec![WatchRequest { includes: Some(vec!["nothing".into()]), ..req(&file_path) }]).await;

        t.basic_crud_test(&file_path, true, NO_CID, None, false).await;
    });
}

#[test]
fn includes_are_supported_folder_watch() {
    run(|t| async move {
        t.watch(vec![WatchRequest { includes: Some(vec!["**/files-includes.txt".into()]), ..req(&t.dir.path) }]).await;

        t.crud(&t.dir.join(&["files-includes.txt"])).await;
    });
}

#[test]
fn includes_are_supported_folder_watch_relative_pattern_explicit() {
    run(|t| async move {
        let includes = vec![PatternItem::Relative(RelativePattern { base: t.dir.path.clone(), pattern: "files-includes.txt".to_string() })];
        t.watch(vec![WatchRequest { includes: Some(includes), ..req(&t.dir.path) }]).await;

        t.crud(&t.dir.join(&["files-includes.txt"])).await;
    });
}

#[test]
fn includes_are_supported_folder_watch_relative_pattern_implicit() {
    run(|t| async move {
        t.watch(vec![WatchRequest { includes: Some(vec!["files-includes.txt".into()]), ..req(&t.dir.path) }]).await;

        t.crud(&t.dir.join(&["files-includes.txt"])).await;
    });
}

#[test]
fn correlation_id_is_supported() {
    run(|t| async move {
        let correlation_id = 7_654_321;
        t.watch(vec![WatchRequest { correlation_id: Some(correlation_id), ..req(&t.dir.path) }]).await;

        t.basic_crud_test(&t.dir.join(&["newFile.txt"]), false, Cid::Exactly(Some(correlation_id)), None, false).await;
    });
}

#[test]
#[cfg_attr(windows, ignore = "test.skip no Windows: não cria link simbólico de arquivo sem contexto elevado, como no original")]
fn symlink_support_folder_watch() {
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
fn symlink_support_file_watch() {
    run(|t| async move {
        let link = t.dir.join(&["lorem.txt-linked"]);
        let link_target = t.dir.join(&["lorem.txt"]);
        symlink(&link_target, &link);

        t.watch(vec![req(&link)]).await;

        t.basic_crud_test(&link, true, NO_CID, None, false).await;
    });
}

/// `\\localhost\c$\my_dir`: o caminho UNC local do diretório de teste.
fn unc_path(test_dir: &str) -> String {
    let drive = je_base::extpath::get_drive_letter(test_dir, true).map(|c| c.to_ascii_lowercase()).unwrap_or('c');
    let rest = test_dir.split_once(':').map(|(_, rest)| rest).unwrap_or(test_dir).trim_start_matches('\\');
    format!("\\\\localhost\\{drive}$\\{rest}")
}

#[test]
#[cfg_attr(not(windows), ignore = "test.skip fora do Windows: UNC só existe lá, como no original")]
fn unc_support_folder_watch() {
    run(|t| async move {
        // `addUNCHostToAllowlist('localhost')`: a lista de hosts UNC não existe no porte (não há o
        // ambiente do Electron que a exige).
        let unc = unc_path(&t.dir.path);

        t.watch(vec![req(&unc)]).await;

        t.crud(&join(&[&unc, "newFile.txt"])).await;
    });
}

#[test]
#[cfg_attr(not(windows), ignore = "test.skip fora do Windows: UNC só existe lá, como no original")]
fn unc_support_file_watch() {
    run(|t| async move {
        let unc = format!("{}\\lorem.txt", unc_path(&t.dir.path));

        t.watch(vec![req(&unc)]).await;

        t.basic_crud_test(&unc, true, NO_CID, None, false).await;
    });
}

#[test]
#[cfg_attr(target_os = "linux", ignore = "test.skip no Linux: o sistema de arquivos distingue maiúsculas e minúsculas, como no original")]
fn wrong_casing_folder_watch() {
    run(|t| async move {
        let base = t.dir.path.rsplit(['/', '\\']).next().unwrap_or("").to_uppercase();
        let wrong_case = join(&[&dirname(&t.dir.path), &base]);

        t.watch(vec![req(&wrong_case)]).await;

        t.crud(&join(&[&wrong_case, "newFile.txt"])).await;
    });
}

#[test]
#[cfg_attr(target_os = "linux", ignore = "test.skip no Linux: o sistema de arquivos distingue maiúsculas e minúsculas, como no original")]
fn wrong_casing_file_watch() {
    run(|t| async move {
        let file_path = t.dir.join(&["LOREM.txt"]);
        t.watch(vec![req(&file_path)]).await;

        t.basic_crud_test(&file_path, true, NO_CID, None, false).await;
    });
}

#[test]
fn invalid_path_does_not_explode() {
    run(|t| async move {
        let invalid_path = t.dir.join(&["invalid"]);

        t.watch(vec![req(&invalid_path)]).await;
    });
}

#[test]
fn watch_file_contents_test() {
    run(|t| async move {
        let watched_path = t.dir.join(&["lorem.txt"]);

        let cts = CancellationTokenSource::default();

        let ready_promise = Rc::new(DeferredPromise::<()>::new());
        let chunk_promise = Rc::new(DeferredPromise::<()>::new());
        let (chunk, ready) = (chunk_promise.clone(), ready_promise.clone());
        let watch_promise = watch_file_contents(
            &watched_path,
            Rc::new(move |_| {
                chunk.complete(());
            }),
            Rc::new(move || {
                ready.complete(());
            }),
            cts.token(),
            512,
        );

        must(ready_promise.promise.clone()).await;

        let path = watched_path.clone();
        detach(async move { write_file(&path, "Hello World").await });

        must(chunk_promise.promise.clone()).await;

        cts.cancel(); // this will resolve `watchPromise`

        must(watch_promise).await;
    });
}

#[test]
fn watching_same_or_overlapping_paths_supported_when_correlation_is_applied() {
    run(|t| async move {
        t.watch(vec![WatchRequest { correlation_id: Some(1), ..req(&t.dir.path) }]).await;

        t.basic_crud_test(&t.dir.join(&["newFile_1.txt"]), false, Cid::Any, Some(1), false).await;

        t.watch(vec![
            WatchRequest { correlation_id: Some(1), ..req(&t.dir.path) },
            WatchRequest { correlation_id: Some(2), ..req(&t.dir.path) },
            WatchRequest { correlation_id: None, ..req(&t.dir.path) },
        ])
        .await;

        t.basic_crud_test(&t.dir.join(&["newFile_2.txt"]), false, Cid::Any, Some(3), false).await;
        t.basic_crud_test(&t.dir.join(&["otherNewFile.txt"]), false, Cid::Any, Some(3), false).await;
    });
}

#[test]
fn watching_missing_path_emits_watcher_fail_event() {
    run(|t| async move {
        let on_did_watch_fail = t.on_watch_fail();

        let folder_path = t.dir.join(&["missing"]);
        let _ = t.watcher().watch(vec![request(&folder_path, true)]);

        must(on_did_watch_fail).await;
    });
}

#[test]
fn deleting_watched_path_emits_watcher_fail_and_delete_event_when_correlated_file_watch() {
    run(|t| async move {
        let file_path = t.dir.join(&["lorem.txt"]);

        t.watch(vec![WatchRequest { correlation_id: Some(1), ..req(&file_path) }]).await;

        let instance = t.watcher().watchers()[0].instance.clone();

        let on_did_watch_fail = t.on_watch_fail();
        let change_future = t.await_event_with(&file_path, Deleted, Cid::Exactly(Some(1)), None);
        let path = file_path.clone();
        detach(async move { unlink(&path).await });
        must(on_did_watch_fail).await;
        must(change_future).await;
        assert!(instance.failed());
    });
}

#[test]
#[cfg_attr(target_os = "macos", ignore = "test.skip no macOS: não reporta deletes de pasta, como no original")]
fn deleting_watched_path_emits_watcher_fail_and_delete_event_when_correlated_folder_watch() {
    run(|t| async move {
        let folder_path = t.dir.join(&["deep"]);

        t.watch(vec![WatchRequest { correlation_id: Some(1), ..req(&folder_path) }]).await;

        let instance = t.watcher().watchers()[0].instance.clone();
        let on_did_watch_fail = t.on_watch_fail();
        let change_future = t.await_event_with(&folder_path, Deleted, Cid::Exactly(Some(1)), None);
        let path = folder_path.clone();
        detach(async move { rm(&path).await });
        must(on_did_watch_fail).await;
        must(change_future).await;
        assert!(instance.failed());
    });
}

#[test]
#[cfg_attr(not(windows), ignore = "só roda no Windows, como no original (`isWindows ? test : test.skip`): pendente, não há Windows neste host")]
fn recreating_a_deleted_watched_folder_stops_the_stale_watcher_and_reattaches() {
    run(|t| async move {
        let folder_path = t.dir.join(&["recreated"]);
        mkdir(&folder_path).await;

        t.watcher().set_verbose_logging(true);
        let raw_event_count = Rc::new(Cell::new(0usize));
        let counter = raw_event_count.clone();
        let subscription = t.watcher().on_did_log_message().listen(move |event| {
            if event.message.contains("[raw]") {
                counter.set(counter.get() + 1);
            }
        });

        t.watch(vec![WatchRequest { correlation_id: Some(1), ..req(&folder_path) }]).await;

        let instance = t.watcher().watchers()[0].instance.clone();
        let mut raw_event_counts_stable = Vec::new();
        for i in 0..3 {
            let reattached = to_promise(&t.watcher().on_did_log_message().filter(|event| event.message.contains("Started watching"), None));
            rm(&folder_path).await;
            mkdir(&folder_path).await;
            must(reattached).await;
            let raw_event_count_after_reattach = raw_event_count.get();
            must(timeout(200)).await;
            raw_event_counts_stable.push(raw_event_count.get() == raw_event_count_after_reattach);
            t.basic_crud_test(&join(&[&folder_path, &format!("newFile-{i}.txt")]), false, Cid::Exactly(Some(1)), None, false).await;
        }

        assert_eq!((instance.failed(), raw_event_counts_stable), (false, vec![true, true, true]));
        subscription.dispose();
    });
}

#[test]
fn watch_requests_support_suspend_resume_file_does_not_exist_in_beginning() {
    run(|t| async move {
        let file_path = t.dir.join(&["not-found.txt"]);

        let on_did_watch_fail = t.on_watch_fail();
        let request = req(&file_path);
        t.watch(vec![request.clone()]).await;
        must(on_did_watch_fail).await;
        assert_eq!(t.is_suspended(&request), Suspended::Polling);

        t.basic_crud_test(&file_path, false, NO_CID, None, true).await;
        t.basic_crud_test(&file_path, false, NO_CID, None, true).await;
    });
}

#[test]
fn watch_requests_support_suspend_resume_file_exists_in_beginning() {
    run(|t| async move {
        let file_path = t.dir.join(&["lorem.txt"]);
        let request = req(&file_path);
        t.watch(vec![request.clone()]).await;

        let on_did_watch_fail = t.on_watch_fail();
        t.basic_crud_test(&file_path, true, NO_CID, None, false).await;
        must(on_did_watch_fail).await;
        assert_eq!(t.is_suspended(&request), Suspended::Polling);

        t.basic_crud_test(&file_path, false, NO_CID, None, true).await;
    });
}

#[test]
#[cfg_attr(windows, ignore = "test.skip no Windows: não reporta isso, como no original")]
fn watch_requests_support_suspend_resume_folder_does_not_exist_in_beginning() {
    run(|t| async move {
        let on_did_watch_fail = t.on_watch_fail();

        let folder_path = t.dir.join(&["not-found"]);
        let request = req(&folder_path);
        t.watch(vec![request.clone()]).await;
        must(on_did_watch_fail).await;
        assert_eq!(t.is_suspended(&request), Suspended::Polling);

        let change_future = t.await_event(&folder_path, Added);
        let on_did_watch = t.on_did_watch();
        mkdir(&folder_path).await;
        must(change_future).await;
        must(on_did_watch).await;

        assert_eq!(t.is_suspended(&request), Suspended::No);

        // O ramo `if (isWindows)` do original ("somehow failing on macOS/Linux") só roda no Windows.
        if cfg!(windows) {
            let file_path = join(&[&folder_path, "newFile.txt"]);
            t.crud(&file_path).await;

            let on_did_watch_fail = t.on_watch_fail();
            rmdir(&folder_path).await;
            must(on_did_watch_fail).await;

            let change_future = t.await_event(&folder_path, Added);
            let on_did_watch = t.on_did_watch();
            mkdir(&folder_path).await;
            must(change_future).await;
            must(on_did_watch).await;

            must(timeout(500)).await; // somehow needed on Linux

            t.crud(&file_path).await;
        }
    });
}

#[test]
#[cfg_attr(target_os = "macos", ignore = "test.skip no macOS: não reporta isso, como no original")]
fn watch_requests_support_suspend_resume_folder_exists_in_beginning() {
    run(|t| async move {
        let folder_path = t.dir.join(&["deep"]);
        t.watch(vec![req(&folder_path)]).await;

        let file_path = join(&[&folder_path, "newFile.txt"]);
        t.crud(&file_path).await;

        let on_did_watch_fail = t.on_watch_fail();
        rm(&folder_path).await;
        must(on_did_watch_fail).await;

        let change_future = t.await_event(&folder_path, Added);
        let on_did_watch = t.on_did_watch();
        mkdir(&folder_path).await;
        must(change_future).await;
        must(on_did_watch).await;

        must(timeout(500)).await; // somehow needed on Linux

        t.crud(&file_path).await;
    });
}

#[test]
fn parcel_watcher_reused_when_present_for_non_recursive_file_watching_uncorrelated() {
    run(|t| async move { test_parcel_watcher_reused(&t, None).await });
}

#[test]
fn parcel_watcher_reused_when_present_for_non_recursive_file_watching_correlated() {
    run(|t| async move { test_parcel_watcher_reused(&t, Some(2)).await });
}

fn create_parcel_watcher() -> Rc<ParcelWatcher> {
    test_parcel_watcher()
}

async fn test_parcel_watcher_reused(t: &Test, correlation_id: Option<i64>) {
    let recursive_watcher = create_parcel_watcher();
    must(recursive_watcher.watch(vec![WatchRequest { correlation_id: Some(1), ..request(&t.dir.path, true) }])).await;

    let recursive_instance = recursive_watcher.watchers()[0].clone();
    assert_eq!(recursive_instance.subscriptions_count(), 0);

    t.create_watcher(Some(recursive_watcher.clone() as Rc<dyn RecursiveWatcherWithSubscribe>)).await;

    let file_path = t.dir.join(&["deep", "conway.js"]);
    t.watch(vec![WatchRequest { correlation_id, ..req(&file_path) }]).await;

    let instance = t.watcher().watchers()[0].instance.clone();
    assert!(instance.is_reusing_recursive_watcher());
    assert_eq!(recursive_instance.subscriptions_count(), 1);

    // macOS: "somehow fsevents seems to report still on the initial create from test setup"
    let change_type = if cfg!(target_os = "macos") { Added } else { Updated };
    let change_future = t.await_event_with(&file_path, change_type, Cid::Exactly(correlation_id), None);
    write_file(&file_path, "Hello World").await;
    must(change_future).await;

    must(recursive_watcher.stop()).await;
    recursive_watcher.dispose();

    must(timeout(500)).await; // give the watcher some time to restart

    let change_future = t.await_event_with(&file_path, Updated, Cid::Exactly(correlation_id), None);
    write_file(&file_path, "Hello World").await;
    must(change_future).await;

    assert!(!instance.is_reusing_recursive_watcher());
}

#[test]
fn watch_requests_support_suspend_resume_file_does_not_exist_in_beginning_parcel_watcher_reused() {
    run(|t| async move {
        let recursive_watcher = create_parcel_watcher();
        must(recursive_watcher.watch(vec![request(&t.dir.path, true)])).await;

        t.create_watcher(Some(recursive_watcher.clone() as Rc<dyn RecursiveWatcherWithSubscribe>)).await;

        let file_path = t.dir.join(&["not-found-2.txt"]);

        let on_did_watch_fail = t.on_watch_fail();
        let request = req(&file_path);
        t.watch(vec![request.clone()]).await;
        must(on_did_watch_fail).await;
        assert_eq!(t.is_suspended(&request), Suspended::Yes);

        let change_future = t.await_event(&file_path, Added);
        write_file(&file_path, "Hello World").await;
        must(change_future).await;

        assert_eq!(t.is_suspended(&request), Suspended::No);

        must(recursive_watcher.stop()).await;
        recursive_watcher.dispose();
    });
}

#[test]
fn event_type_filter_file_watch() {
    run(|t| async move {
        let file_path = t.dir.join(&["lorem.txt"]);
        let request = WatchRequest { filter: Some(FileChangeFilter::UPDATED | FileChangeFilter::DELETED), correlation_id: Some(1), ..req(&file_path) };
        t.watch(vec![request]).await;

        // Change file
        let change_future = t.await_event_with(&file_path, Updated, Cid::Exactly(Some(1)), None);
        write_file(&file_path, "Hello Change").await;
        must(change_future).await;

        // Delete file
        let change_future = t.await_event_with(&file_path, Deleted, Cid::Exactly(Some(1)), None);
        unlink(&file_path).await;
        must(change_future).await;
    });
}

#[test]
fn event_type_filter_folder_watch() {
    run(|t| async move {
        let request = WatchRequest { filter: Some(FileChangeFilter::UPDATED | FileChangeFilter::DELETED), correlation_id: Some(1), ..req(&t.dir.path) };
        t.watch(vec![request]).await;

        // Change file
        let file_path = t.dir.join(&["lorem.txt"]);
        let change_future = t.await_event_with(&file_path, Updated, Cid::Exactly(Some(1)), None);
        write_file(&file_path, "Hello Change").await;
        must(change_future).await;

        // Delete file
        let change_future = t.await_event_with(&file_path, Deleted, Cid::Exactly(Some(1)), None);
        unlink(&file_path).await;
        must(change_future).await;
    });
}

#[test]
#[cfg_attr(target_os = "linux", ignore = "test.skip no Linux: includes só ignoram a caixa no Windows e no macOS, como no original")]
fn includes_are_case_insensitive_on_windows_mac() {
    run(|t| async move {
        t.watch(vec![WatchRequest { includes: Some(vec!["*.TXT".into()]), ..req(&t.dir.path) }]).await;

        t.crud(&t.dir.join(&["newFile.txt"])).await;
    });
}

/// O corpo dos dois casos `excludes are case insensitive on Windows/Mac` (idênticos no original).
async fn excludes_are_case_insensitive(t: &Test) {
    t.watch(vec![WatchRequest { excludes: vec!["*.TXT".into()], ..req(&t.dir.path) }]).await;

    // New file (should be excluded)
    let new_file_path = t.dir.join(&["newFile.txt"]);
    let change_future = t.await_event(&new_file_path, Added);
    write_file(&new_file_path, "Hello World").await;

    let res = must(Promise::race(vec![timeout(500).then(|()| true), change_future.then(|_| false)])).await;

    if !res {
        panic!("Unexpected change event");
    }
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
