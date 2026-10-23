//! Porte de `src/vs/platform/files/test/browser/inmemoryFileService.test.ts` (suíte `InMemory File Service`), caso a caso e na mesma ordem.
//!
//! Divergências: o `setup` (serviço, provider e fixture) roda no início do corpo de cada teste, dentro do
//! mesmo `runWithFakedTimers` (o original usa o relógio real; o `timeout(20)` do caso dos eventos do
//! provider avança no relógio virtual). `await service.createFolder(...)` etc. viram `.await.unwrap()`.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::files::{async_test, join_path};
use je_base::async_::timeout;
use je_base::buffer::{VSBuffer, stream_to_buffer};
use je_base::lifecycle::DisposableStore;
use je_base::network::schemas;
use je_base::uri::{URI, UriComponents};
use je_platform::files::file_service::{FileContentSource, FileService};
use je_platform::files::files::*;
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::log::NullLogService;

fn get_by_name<'a>(root: &'a FileStat, name: &str) -> Option<&'a FileStat> {
    root.children.as_ref()?.iter().find(|child| child.name == name)
}

fn create_large_buffer(size: usize, seed: usize) -> VSBuffer {
    VSBuffer::wrap((0..size).map(|i| ((seed + i) % 256) as u8).collect())
}

struct Fixture {
    root: URI,
    index_html: URI,
    small_txt: URI,
    small_umlaut_txt: URI,
    deep_dir: URI,
}

struct Setup {
    service: Rc<FileService>,
    provider: Rc<InMemoryFileSystemProvider>,
    fixture: Fixture,
}

/// O `setup` da suíte.
async fn setup(disposables: &Rc<DisposableStore>) -> Setup {
    let service = disposables.add_rc(FileService::new(NullLogService::shared()));
    let provider = disposables.add_rc(InMemoryFileSystemProvider::new());
    disposables.add(service.register_provider(schemas::IN_MEMORY, provider.clone()).unwrap());

    let fixture = create_fixture(&service).await;
    Setup { service, provider, fixture }
}

fn text(content: &FileContent) -> String {
    content.value.to_string()
}

fn buf(content: &str) -> FileContentSource {
    FileContentSource::from(VSBuffer::from_string(content))
}

/// Os 64 bytes do início, depois do prefixo e do fim do conteúdo, como o original confere.
fn assert_append_result(content: &VSBuffer, prefix: &VSBuffer, suffix: &VSBuffer) {
    assert_eq!(content.byte_length(), prefix.byte_length() + suffix.byte_length());

    let (p, s, c) = (prefix.byte_length() as i64, suffix.byte_length() as i64, content.byte_length() as i64);
    assert_eq!(content.slice(Some(0), Some(64)).to_vec(), prefix.slice(Some(0), Some(64)).to_vec());
    assert_eq!(content.slice(Some(p), Some(p + 64)).to_vec(), suffix.slice(Some(0), Some(64)).to_vec());
    assert_eq!(content.slice(Some(c - 64), Some(c)).to_vec(), suffix.slice(Some(s - 64), Some(s)).to_vec());
}

fn listen_run_operation(service: &Rc<FileService>, disposables: &Rc<DisposableStore>) -> Rc<RefCell<Option<FileOperationEvent>>> {
    let event = Rc::new(RefCell::new(None));
    let slot = event.clone();
    disposables.add(service.on_did_run_operation().listen(move |e| *slot.borrow_mut() = Some(e.clone())));
    event
}

#[test]
fn create_folder() {
    async_test(|disposables| async move {
        let Setup { service, fixture, .. } = setup(&disposables).await;
        let event = listen_run_operation(&service, &disposables);

        let new_folder_resource = join_path(&fixture.root, &["newFolder"]);
        let new_folder = service.create_folder(&new_folder_resource).await.unwrap();

        assert_eq!(new_folder.name, "newFolder");
        assert!(service.exists(&new_folder_resource).await.unwrap());

        let event = event.borrow();
        let event = event.as_ref().expect("evento esperado");
        assert_eq!(event.resource.to_string(false), new_folder_resource.to_string(false));
        assert_eq!(event.operation, FileOperation::Create);
        let target = event.target.as_ref().unwrap();
        assert_eq!(target.resource.to_string(false), new_folder_resource.to_string(false));
        assert!(target.is_directory);
    });
}

#[test]
fn create_folder_creating_multiple_folders_at_once() {
    async_test(|disposables| async move {
        let Setup { service, fixture, .. } = setup(&disposables).await;
        let event = listen_run_operation(&service, &disposables);

        let multi_folder_paths = ["a", "couple", "of", "folders"];
        let new_folder_resource = join_path(&fixture.root, &multi_folder_paths);

        let new_folder = service.create_folder(&new_folder_resource).await.unwrap();
        assert_eq!(new_folder.name, multi_folder_paths[multi_folder_paths.len() - 1]);
        assert!(service.exists(&new_folder_resource).await.unwrap());

        let event = event.borrow();
        let event = event.as_ref().expect("evento esperado");
        assert_eq!(event.resource.to_string(false), new_folder_resource.to_string(false));
        assert_eq!(event.operation, FileOperation::Create);
        let target = event.target.as_ref().unwrap();
        assert_eq!(target.resource.to_string(false), new_folder_resource.to_string(false));
        assert!(target.is_directory);
    });
}

#[test]
fn exists() {
    async_test(|disposables| async move {
        let Setup { service, fixture, .. } = setup(&disposables).await;

        assert!(service.exists(&fixture.root).await.unwrap());

        assert!(!service.exists(&join_path(&fixture.root, &["does-not-exist"])).await.unwrap());
    });
}

#[test]
fn resolve_file() {
    async_test(|disposables| async move {
        let Setup { service, fixture, .. } = setup(&disposables).await;
        let resolved = service.resolve(&fixture.index_html, None).await.unwrap();

        assert_eq!(resolved.name, "index.html");
        assert!(resolved.is_file);
        assert!(!resolved.is_directory);
    });
}

#[test]
fn resolve_directory() {
    async_test(|disposables| async move {
        let Setup { service, fixture, .. } = setup(&disposables).await;
        let resolved = service.resolve(&fixture.root, None).await.unwrap();
        assert!(resolved.is_directory);
        let children = resolved.children.expect("filhos esperados");

        let mut names: Vec<_> = children.iter().map(|c| c.name.clone()).collect();
        names.sort();
        let mut expected = vec!["examples", "index.html", "other", "site.css", "deep", "small.txt", "small_umlaut.txt"];
        expected.sort();
        assert_eq!(names, expected);
    });
}

#[test]
fn resolve_directory_with_resolve_to() {
    async_test(|disposables| async move {
        let Setup { service, fixture, .. } = setup(&disposables).await;
        let options = ResolveFileOptions { resolve_to: vec![fixture.deep_dir.clone()], ..Default::default() };
        let resolved = service.resolve(&fixture.root, Some(options)).await.unwrap();

        let deep = get_by_name(&resolved, "deep").expect("deep esperado");
        assert_eq!(deep.children.as_ref().expect("filhos esperados").len(), 4);
    });
}

#[test]
fn read_file() {
    async_test(|disposables| async move {
        let Setup { service, fixture, .. } = setup(&disposables).await;
        let content = service.read_file(&fixture.small_txt, None, None).await.unwrap();
        assert_eq!(text(&content), "Small File");
    });
}

fn from_position(position: u64) -> Option<ReadFileOptions> {
    Some(ReadFileOptions { stream: FileReadStreamOptions { position: Some(position), ..Default::default() }, ..Default::default() })
}

#[test]
fn read_file_from_position_ascii() {
    async_test(|disposables| async move {
        let Setup { service, fixture, .. } = setup(&disposables).await;
        let content = service.read_file(&fixture.small_txt, from_position(6), None).await.unwrap();
        assert_eq!(text(&content), "File");
    });
}

#[test]
fn read_file_from_position_with_umlaut() {
    async_test(|disposables| async move {
        let Setup { service, fixture, .. } = setup(&disposables).await;
        let pos = VSBuffer::from_string("Small File with Ü").byte_length() as u64;
        let content = service.read_file(&fixture.small_umlaut_txt, from_position(pos), None).await.unwrap();
        assert_eq!(text(&content), "mlaut");
    });
}

#[test]
fn read_file_stream() {
    async_test(|disposables| async move {
        let Setup { service, fixture, .. } = setup(&disposables).await;
        let content = service.read_file_stream(&fixture.small_txt, None, None).await.unwrap();
        assert_eq!(stream_to_buffer(&*content.value).await.unwrap().to_string(), "Small File");
    });
}

#[test]
fn write_file() {
    async_test(|disposables| async move {
        let Setup { service, fixture, .. } = setup(&disposables).await;
        service.write_file(&fixture.small_txt, buf("Updated"), None).await.unwrap();

        let content = service.read_file(&fixture.small_txt, None, None).await.unwrap();
        assert_eq!(text(&content), "Updated");
    });
}

#[test]
fn provider_open_write_append() {
    async_test(|disposables| async move {
        let Setup { service, provider, fixture } = setup(&disposables).await;
        let resource = join_path(&fixture.root, &["append.txt"]);
        service.write_file(&resource, buf("Hello"), None).await.unwrap();

        let fd = provider.open(&resource, &FileOpenOptions::for_write(false, true)).await.unwrap();
        let data = VSBuffer::from_string(" World");
        let written = provider.write(fd, 0, &data, 0, data.byte_length()).await;
        provider.close(fd).await.unwrap();
        written.unwrap();

        let content = service.read_file(&resource, None, None).await.unwrap();
        assert_eq!(text(&content), "Hello World");
    });
}

#[test]
fn provider_open_write_append_large() {
    async_test(|disposables| async move {
        let Setup { service, provider, fixture } = setup(&disposables).await;
        let resource = join_path(&fixture.root, &["append-large-open.txt"]);
        let prefix = create_large_buffer(256 * 1024, 1);
        let suffix = create_large_buffer(256 * 1024, 2);

        service.write_file(&resource, prefix.clone().into(), None).await.unwrap();

        let fd = provider.open(&resource, &FileOpenOptions::for_write(false, true)).await.unwrap();
        // 123: ignorado no modo append
        let written = provider.write(fd, 123, &suffix, 0, suffix.byte_length()).await;
        provider.close(fd).await.unwrap();
        written.unwrap();

        let content = service.read_file(&resource, None, None).await.unwrap();
        assert_append_result(&content.value, &prefix, &suffix);
    });
}

fn append() -> Option<WriteFileOptions> {
    Some(WriteFileOptions { append: true, ..Default::default() })
}

#[test]
fn write_file_append() {
    async_test(|disposables| async move {
        let Setup { service, fixture, .. } = setup(&disposables).await;
        let resource = join_path(&fixture.root, &["append-via-writeFile.txt"]);
        service.write_file(&resource, buf("Hello"), None).await.unwrap();

        service.write_file(&resource, buf(" World"), append()).await.unwrap();

        let content = service.read_file(&resource, None, None).await.unwrap();
        assert_eq!(text(&content), "Hello World");
    });
}

#[test]
fn write_file_append_large() {
    async_test(|disposables| async move {
        let Setup { service, fixture, .. } = setup(&disposables).await;
        let resource = join_path(&fixture.root, &["append-large-writeFile.txt"]);
        let prefix = create_large_buffer(256 * 1024, 3);
        let suffix = create_large_buffer(256 * 1024, 4);

        service.write_file(&resource, prefix.clone().into(), None).await.unwrap();
        service.write_file(&resource, suffix.clone().into(), append()).await.unwrap();

        let content = service.read_file(&resource, None, None).await.unwrap();
        assert_append_result(&content.value, &prefix, &suffix);
    });
}

#[test]
fn rename() {
    async_test(|disposables| async move {
        let Setup { service, fixture, .. } = setup(&disposables).await;
        let source = join_path(&fixture.root, &["site.css"]);
        let target = join_path(&fixture.root, &["SITE.css"]);

        service.move_(&source, &target, Some(true)).await.unwrap();

        assert!(!service.exists(&source).await.unwrap());
        assert!(service.exists(&target).await.unwrap());
    });
}

#[test]
fn copy() {
    async_test(|disposables| async move {
        let Setup { service, fixture, .. } = setup(&disposables).await;
        let source = fixture.index_html.clone();
        let target = join_path(&fixture.root, &["index-copy.html"]);

        service.copy(&source, &target, Some(true)).await.unwrap();

        let copied = service.read_file(&target, None, None).await.unwrap();
        assert_eq!(text(&copied), "Index");
    });
}

#[test]
fn delete_file() {
    async_test(|disposables| async move {
        let Setup { service, fixture, .. } = setup(&disposables).await;
        let resource = join_path(&fixture.root, &["to-delete.txt"]);
        service.write_file(&resource, buf("delete me"), None).await.unwrap();
        assert!(service.exists(&resource).await.unwrap());

        service.del(&resource, None).await.unwrap();
        assert!(!service.exists(&resource).await.unwrap());
    });
}

#[test]
fn provider_events_bubble_through_file_service() {
    async_test(|disposables| async move {
        let Setup { service, fixture, .. } = setup(&disposables).await;
        let change_count = Rc::new(Cell::new(0));
        let resource = join_path(&fixture.root, &["events.txt"]);
        let (counter, watched) = (change_count.clone(), resource.clone());
        disposables.add(service.on_did_files_change().listen(move |e| {
            if e.contains(&watched, &[FileChangeType::Updated]) || e.contains(&watched, &[FileChangeType::Added]) || e.contains(&watched, &[FileChangeType::Deleted]) {
                counter.set(counter.get() + 1);
            }
        }));

        service.write_file(&resource, buf("1"), None).await.unwrap();
        service.write_file(&resource, buf("2"), None).await.unwrap();
        service.del(&resource, None).await.unwrap();

        timeout(20).await.unwrap(); // provider fires changes async
        assert!(change_count.get() > 0);
    });
}

#[test]
fn set_read_only_toggles_provider_capabilities() {
    async_test(|disposables| async move {
        let Setup { service, provider, fixture } = setup(&disposables).await;
        provider.set_read_only(true);
        assert!(provider.capabilities().intersects(FileSystemProviderCapabilities::READONLY));

        let result = service.write_file(&join_path(&fixture.root, &["readonly.txt"]), buf("fail"), None).await;

        assert!(result.is_err());

        provider.set_read_only(false);
        service.write_file(&join_path(&fixture.root, &["readonly.txt"]), buf("ok"), None).await.unwrap();
    });
}

async fn create_fixture(service: &Rc<FileService>) -> Fixture {
    let root = URI::from(&UriComponents { scheme: Some(schemas::IN_MEMORY.into()), path: Some("/".into()), ..Default::default() }, false);
    let at = |path: &[&str]| join_path(&root, path);

    service.create_folder(&at(&["examples"])).await.unwrap();
    service.create_folder(&at(&["other"])).await.unwrap();
    service.write_file(&at(&["index.html"]), buf("Index"), None).await.unwrap();
    service.write_file(&at(&["site.css"]), buf("body { }"), None).await.unwrap();

    service.write_file(&at(&["small.txt"]), buf("Small File"), None).await.unwrap();
    service.write_file(&at(&["small_umlaut.txt"]), buf("Small File with Ümlaut"), None).await.unwrap();

    service.create_folder(&at(&["deep"])).await.unwrap();
    service.write_file(&at(&["deep", "conway.js"]), buf("console.log(\"conway\");"), None).await.unwrap();
    service.write_file(&at(&["deep", "a.txt"]), buf("A"), None).await.unwrap();
    service.write_file(&at(&["deep", "b.txt"]), buf("B"), None).await.unwrap();
    service.write_file(&at(&["deep", "c.txt"]), buf("C"), None).await.unwrap();

    service.create_folder(&at(&["other", "deep"])).await.unwrap();
    for name in ["1", "2", "3", "4"] {
        service.write_file(&at(&["other", "deep", &format!("{name}.txt")]), buf(name), None).await.unwrap();
    }

    Fixture {
        index_html: at(&["index.html"]),
        small_txt: at(&["small.txt"]),
        small_umlaut_txt: at(&["small_umlaut.txt"]),
        deep_dir: at(&["deep"]),
        root,
    }
}
