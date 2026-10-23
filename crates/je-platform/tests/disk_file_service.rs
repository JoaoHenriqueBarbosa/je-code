//! Porte de `src/vs/platform/files/test/node/diskFileService.integrationTest.ts` (suíte `Disk File Service`), caso a caso e na mesma ordem.
//!
//! Divergências:
//!
//! - Cada teste `async` roda no laço de eventos real do thread ([`run_on_real_loop`]), porque a E/S de disco
//!   leva tempo de verdade; o `setup` e o `teardown` (serviço, providers e diretório de teste) são o
//!   [`Fixture`], descartado no fim do teste. `DiskFileSystemProvider.configureFlushOnWrite` é global do
//!   processo: um `RwLock` faz os testes comuns (flush desligado, como o topo do original) e os dois de
//!   "flush on write" (ligado) nunca se misturarem nos threads do `cargo test`.
//! - Os `test(...)` gerados por funções auxiliares (`testMoveAcrossProviders`, `testReadFile`...) viram um
//!   `#[test]` por combinação, na ordem do original.
//! - `readdirSync` do original compara a ordem crua do sistema de arquivos; aqui os dois lados são ordenados,
//!   porque a ordem do `read_dir` não é garantida entre dois diretórios (o que o caso prova é o conteúdo).
//! - `new TestDiskFileSystemProvider#setInvalidStatSize` não consegue falsear o tamanho como texto (o `Stat`
//!   de Rust só tem número): o caso do #72909 liga o setter e confere que a leitura sem etag funciona.
//! - Os `assert.ok(service.exists(...))` do original afirmam sobre a promessa (sempre verdadeira); aqui o
//!   resultado é esperado, o que é mais estrito.
//! - Os links simbólicos de pasta usam `symlink` no Unix e `symlink_dir` no Windows (o original usa `junction`,
//!   que o Rust não cria); no Windows podem precisar de contexto elevado. Os testes de lixeira
//!   (`useTrash`) não rodam: o provider de disco do porte ainda não implementa a lixeira do sistema.
//! - O `chmod` do caso dos arquivos travados tira o bit de escrita no Unix e usa `set_readonly` fora dele.
//! - `FileReadWrite & FileAtomicRead` do caso "atomic (natively supported)" é zero no original, e zero no
//!   `TestDiskFileSystemProvider` volta às capacidades padrão (que incluem `FileAtomicRead`); o auxiliar
//!   `set_capabilities_keeping_case_sensitivity` reproduz isso.

mod common;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use common::disk::{TestDir, TestDiskFileSystemProvider, copy_fixtures, file_uri, fixture_path, run_on_real_loop};
use je_base::async_::{async_fn, timeout};
use je_base::buffer::{
    BytesOrString, VSBuffer, VSBufferReadableStream, buffer_to_readable, buffer_to_stream, stream_to_buffer, stream_to_buffer_readable_stream,
};
use je_base::errors::Error;
use je_base::js_string::JsString;
use je_base::lifecycle::DisposableStore;
use je_base::network::schemas;
use je_base::path::{Environment, ProcessEnvironment};
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::stream::{Readable, WriteableStream, new_writeable_stream};
use je_base::uri::{Field, URI, UriChange};
use je_platform::files::disk::DiskFileSystemProvider;
use je_platform::files::file_service::{FileContentSource, FileService};
use je_platform::files::files::*;
use je_platform::log::NullLogService;

use FileSystemProviderCapabilities as Cap;

const TEST_SCHEMA: &str = "test";

// region: infraestrutura do teste

/// `DiskFileSystemProvider.configureFlushOnWrite` é estado global do processo: os testes comuns seguram o lado
/// de leitura (flush desligado) e os de "flush on write" o de escrita (flush ligado).
static FLUSH_LOCK: RwLock<()> = RwLock::new(());

#[allow(dead_code)]
enum FlushGuard {
    Shared(RwLockReadGuard<'static, ()>),
    Exclusive(RwLockWriteGuard<'static, ()>),
}

/// O `setup` da suíte: o serviço, os dois providers (`file` e `test`) e o diretório de teste com a cópia das
/// fixtures `service`. O `teardown` é o `Drop`.
struct Fixture {
    service: Rc<FileService>,
    file_provider: Rc<TestDiskFileSystemProvider>,
    test_provider: Rc<TestDiskFileSystemProvider>,
    test_dir: TestDir,
    disposables: Rc<DisposableStore>,
    flush: Option<FlushGuard>,
}

impl Fixture {
    fn new() -> Self {
        let flush = FlushGuard::Shared(FLUSH_LOCK.read().unwrap_or_else(PoisonError::into_inner));
        DiskFileSystemProvider::configure_flush_on_write(false); // speed up all unit tests by disabling flush on write
        Self::with_flush(flush)
    }

    /// O mesmo para o teste que liga o flush (`configureFlushOnWrite(true)`), com o processo só para ele.
    fn new_with_flush_on_write() -> Self {
        let flush = FlushGuard::Exclusive(FLUSH_LOCK.write().unwrap_or_else(PoisonError::into_inner));
        DiskFileSystemProvider::configure_flush_on_write(true);
        Self::with_flush(flush)
    }

    fn with_flush(flush: FlushGuard) -> Self {
        let disposables = DisposableStore::new();
        let service = disposables.add_rc(FileService::new(NullLogService::shared()));

        let file_provider = disposables.add_rc(TestDiskFileSystemProvider::new());
        disposables.add(service.register_provider(schemas::FILE, file_provider.as_provider()).unwrap());

        let test_provider = disposables.add_rc(TestDiskFileSystemProvider::new());
        disposables.add(service.register_provider(TEST_SCHEMA, test_provider.as_provider()).unwrap());

        let test_dir = TestDir::uncreated(&["vsctests", "diskfileservice"]);
        copy_fixtures("service", test_dir.path());

        Self { service, file_provider, test_provider, test_dir, disposables, flush: Some(flush) }
    }

    /// `URI.file(join(testDir, ...segments))`.
    fn uri(&self, relative: &str) -> URI {
        self.test_dir.uri(relative)
    }

    /// `setCapabilities(fileProvider, capabilities)`.
    fn set_capabilities(&self, capabilities: FileSystemProviderCapabilities) {
        self.file_provider.set_capabilities_keeping_case_sensitivity(capabilities);
    }

    /// `disposables.add(service.onDidRunOperation(e => event = e))`: todos os eventos, na ordem.
    fn record_events(&self) -> Rc<RefCell<Vec<FileOperationEvent>>> {
        let events = Rc::new(RefCell::new(Vec::new()));
        let sink = events.clone();
        self.disposables.add(self.service.on_did_run_operation().listen(move |e| sink.borrow_mut().push(e.clone())));
        events
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.disposables.clear();
        // O diretório (`TestDir`) sai antes de o lock de flush ser solto.
        drop(self.flush.take());
    }
}

/// Um teste `async` da suíte, com o `setup` e o `teardown`.
fn disk_test<F: Future<Output = ()> + 'static>(body: impl FnOnce(Fixture) -> F) {
    run_on_real_loop(body(Fixture::new()));
}

fn disk_test_with_flush_on_write<F: Future<Output = ()> + 'static>(body: impl FnOnce(Fixture) -> F) {
    run_on_real_loop(body(Fixture::new_with_flush_on_write()));
}

/// `URI#fsPath`.
fn fs_path(resource: &URI) -> String {
    resource.fs_path(ProcessEnvironment.platform()).to_string_lossy()
}

/// `basename(path)`.
fn basename(path: &str) -> String {
    Path::new(path).file_name().unwrap().to_string_lossy().into_owned()
}

/// `readFileSync(path).toString()`.
fn read_to_string(path: impl AsRef<Path>) -> String {
    String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned()
}

/// `readdirSync`, ordenado (ver a divergência no cabeçalho).
fn read_dir_sorted(path: impl AsRef<Path>) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(path).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

/// `FileAccess.asFileUri('vs/platform/files/test/node/fixtures/<relative>')`.
fn fixture_uri(relative: &str) -> URI {
    file_uri(fixture_path(relative))
}

fn get_by_name<'a>(root: &'a FileStat, name: &str) -> Option<&'a FileStat> {
    root.children.as_ref()?.iter().find(|child| child.name == name)
}

fn buffer_source(contents: &str) -> FileContentSource {
    FileContentSource::from(VSBuffer::from_string(contents))
}

/// `error.fileOperationResult`.
fn operation_result(error: &Error) -> FileOperationResult {
    FileOperationError::from_error(error).expect("era esperado um FileOperationError").file_operation_result
}

/// `posix.join(parent.path, name)` aplicado com `URI#with({ path })`.
fn with_joined_path(parent: &URI, name: &str) -> URI {
    let path = format!("{}/{}", parent.path().to_string_lossy().trim_end_matches('/'), name);
    parent.with(&UriChange { path: Field::Set(JsString::from(path)), ..UriChange::default() })
}

fn with_query(resource: URI, query: Option<&str>) -> URI {
    match query {
        Some(query) => resource.with(&UriChange { query: Field::from(query), ..UriChange::default() }),
        None => resource,
    }
}

fn with_scheme(resource: &URI, scheme: &str) -> URI {
    resource.with(&UriChange { scheme: Field::from(scheme), ..UriChange::default() })
}

fn join_uri(resource: &URI, segments: &[&str]) -> URI {
    ExtUri::unbiased(Rc::new(ProcessEnvironment)).join_path(resource, segments)
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(windows)]
fn symlink(target: &Path, link: &Path) {
    if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, link).unwrap();
    } else {
        std::os::windows::fs::symlink_file(target, link).unwrap();
    }
}

/// `chmod(path, stats.mode & ~0o200)`.
fn remove_write_permission(path: &Path) {
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(permissions.mode() & !0o200);
    }
    #[cfg(not(unix))]
    permissions.set_readonly(true);
    std::fs::set_permissions(path, permissions).unwrap();
}

/// `toLineByLineReadable(content)`.
fn to_line_by_line_readable(content: &str) -> FileContentSource {
    let mut chunks: VecDeque<String> = content.split('\n').enumerate().map(|(index, chunk)| if index == 0 { chunk.to_string() } else { format!("\n{chunk}") }).collect();
    let readable: Box<dyn Readable<VSBuffer>> = Box::new(move || chunks.pop_front().map(|chunk| VSBuffer::from_string(&chunk)));
    FileContentSource::from(readable)
}

/// `streamToBufferReadableStream(createReadStream(path))`: o `fs.createReadStream` entrega o arquivo em
/// pedaços de 64 KiB, cada um num turno do laço de eventos.
fn create_read_stream_as_buffer_stream(path: PathBuf) -> FileContentSource {
    const HIGH_WATER_MARK: usize = 64 * 1024;
    let node_stream = new_writeable_stream::<BytesOrString>(None, None);
    let writer = node_stream.clone();
    je_base::runtime::spawn_local(async move {
        let bytes = std::fs::read(path).unwrap();
        for chunk in bytes.chunks(HIGH_WATER_MARK) {
            timeout(0).await.unwrap();
            writer.write(BytesOrString::Bytes(chunk.to_vec()));
        }
        writer.end(None);
    });
    let stream: VSBufferReadableStream = stream_to_buffer_readable_stream(&*node_stream);
    FileContentSource::from(stream)
}

fn provider_of(f: &Fixture, resource: &URI) -> FileSystemProviderRef {
    f.service.get_provider(&resource.scheme().to_string_lossy()).expect("provider registrado")
}

// endregion

#[test]
fn create_folder() {
    disk_test(|f| async move {
        let event = f.record_events();

        let parent = f.service.resolve(&f.test_dir.root_uri(), None).await.unwrap();

        let new_folder_resource = file_uri(Path::new(&fs_path(&parent.resource)).join("newFolder"));

        let new_folder = f.service.create_folder(&new_folder_resource).await.unwrap();

        assert_eq!(new_folder.name, "newFolder");
        assert!(Path::new(&fs_path(&new_folder.resource)).exists());

        let event = event.borrow();
        let event = event.last().expect("evento esperado");
        assert_eq!(fs_path(&event.resource), fs_path(&new_folder_resource));
        assert_eq!(event.operation, FileOperation::Create);
        let target = event.target.as_ref().unwrap();
        assert_eq!(fs_path(&target.resource), fs_path(&new_folder_resource));
        assert!(target.is_directory);
    });
}

#[test]
fn create_folder_creating_multiple_folders_at_once() {
    disk_test(|f| async move {
        let event = f.record_events();

        let multi_folder_paths = ["a", "couple", "of", "folders"];
        let parent = f.service.resolve(&f.test_dir.root_uri(), None).await.unwrap();

        let new_folder_resource = file_uri(multi_folder_paths.iter().fold(PathBuf::from(fs_path(&parent.resource)), |path, segment| path.join(segment)));

        let new_folder = f.service.create_folder(&new_folder_resource).await.unwrap();

        let last_folder_name = multi_folder_paths[multi_folder_paths.len() - 1];
        assert_eq!(new_folder.name, last_folder_name);
        assert!(Path::new(&fs_path(&new_folder.resource)).exists());

        let event = event.borrow();
        let event = event.last().expect("evento esperado");
        assert_eq!(fs_path(&event.resource), fs_path(&new_folder_resource));
        assert_eq!(event.operation, FileOperation::Create);
        let target = event.target.as_ref().unwrap();
        assert_eq!(fs_path(&target.resource), fs_path(&new_folder_resource));
        assert!(target.is_directory);
    });
}

#[test]
fn exists() {
    disk_test(|f| async move {
        let mut exists = f.service.exists(&f.test_dir.root_uri()).await.unwrap();
        assert!(exists);

        exists = f.service.exists(&file_uri(format!("{}something", f.test_dir.path().to_string_lossy()))).await.unwrap();
        assert!(!exists);
    });
}

#[test]
fn resolve_file() {
    disk_test(|f| async move {
        let resource = fixture_uri("resolver/index.html");
        let resolved = f.service.resolve(&resource, None).await.unwrap();

        assert_eq!(resolved.name, "index.html");
        assert!(resolved.is_file);
        assert!(!resolved.is_directory);
        assert_eq!(resolved.readonly, Some(false));
        assert!(!resolved.is_symbolic_link);
        assert_eq!(resolved.resource.to_string(false), resource.to_string(false));
        assert!(resolved.children.is_none());
        assert!(resolved.mtime.unwrap() > 0);
        assert!(resolved.ctime.unwrap() > 0);
        assert!(resolved.size.unwrap() > 0);
    });
}

#[test]
fn resolve_directory() {
    disk_test(|f| async move {
        let tests_elements = ["examples", "other", "index.html", "site.css"];

        let resource = fixture_uri("resolver");
        let result = f.service.resolve(&resource, None).await.unwrap();

        assert_eq!(result.resource.to_string(false), resource.to_string(false));
        assert_eq!(result.name, "resolver");
        let children = result.children.as_ref().expect("filhos esperados");
        assert!(!children.is_empty());
        assert!(result.is_directory);
        assert_eq!(result.readonly, Some(false));
        assert!(result.mtime.unwrap() > 0);
        assert!(result.ctime.unwrap() > 0);
        assert_eq!(children.len(), tests_elements.len());

        assert!(children.iter().all(|entry| tests_elements.iter().any(|name| basename(&fs_path(&entry.resource)) == *name)));

        for value in children {
            let name = basename(&fs_path(&value.resource));
            assert!(!name.is_empty());
            if ["examples", "other"].contains(&name.as_str()) {
                assert!(value.is_directory);
                assert_eq!(value.mtime, None);
                assert_eq!(value.ctime, None);
            } else if name == "index.html" || name == "site.css" {
                assert!(!value.is_directory);
                assert!(value.children.is_none());
                assert_eq!(value.mtime, None);
                assert_eq!(value.ctime, None);
            } else {
                panic!("Unexpected value {name}");
            }
        }
    });
}

#[test]
fn resolve_directory_with_metadata() {
    disk_test(|f| async move {
        let tests_elements = ["examples", "other", "index.html", "site.css"];

        let options = ResolveFileOptions { resolve_metadata: true, ..Default::default() };
        let result = f.service.resolve_with_metadata(&fixture_uri("resolver"), Some(options)).await.unwrap();

        assert_eq!(result.name, "resolver");
        let children = result.children.as_ref().expect("filhos esperados");
        assert!(!children.is_empty());
        assert!(result.is_directory);
        assert!(result.mtime() > 0);
        assert!(result.ctime() > 0);
        assert_eq!(children.len(), tests_elements.len());

        assert!(children.iter().all(|entry| tests_elements.iter().any(|name| basename(&fs_path(&entry.resource)) == *name)));

        assert!(children.iter().all(|entry| !entry.etag.as_deref().unwrap().is_empty()));

        for value in children {
            let name = basename(&fs_path(&value.resource));
            assert!(!name.is_empty());
            if ["examples", "other"].contains(&name.as_str()) {
                assert!(value.is_directory);
                assert!(value.mtime.unwrap() > 0);
                assert!(value.ctime.unwrap() > 0);
            } else if name == "index.html" || name == "site.css" {
                assert!(!value.is_directory);
                assert!(value.children.is_none());
                assert!(value.mtime.unwrap() > 0);
                assert!(value.ctime.unwrap() > 0);
            } else {
                panic!("Unexpected value {name}");
            }
        }
    });
}

#[test]
fn resolve_directory_with_resolve_to() {
    disk_test(|f| async move {
        let options = ResolveFileOptions { resolve_to: vec![f.uri("deep")], ..Default::default() };
        let resolved = f.service.resolve(&f.test_dir.root_uri(), Some(options)).await.unwrap();
        assert_eq!(resolved.children.as_ref().unwrap().len(), 8);

        let deep = get_by_name(&resolved, "deep").unwrap();
        assert_eq!(deep.children.as_ref().unwrap().len(), 4);
    });
}

#[test]
fn resolve_directory_resolve_to_single_directory() {
    disk_test(|f| async move {
        let resolver_fixtures_path = fixture_path("resolver");
        let options = ResolveFileOptions { resolve_to: vec![file_uri(resolver_fixtures_path.join("other/deep"))], ..Default::default() };
        let result = f.service.resolve(&file_uri(&resolver_fixtures_path), Some(options)).await.unwrap();

        let children = result.children.as_ref().expect("filhos esperados");
        assert!(!children.is_empty());
        assert!(result.is_directory);

        assert_eq!(children.len(), 4);

        let other = get_by_name(&result, "other").unwrap();
        assert!(!other.children.as_ref().unwrap().is_empty());

        let deep = get_by_name(other, "deep").unwrap();
        assert!(!deep.children.as_ref().unwrap().is_empty());
        assert_eq!(deep.children.as_ref().unwrap().len(), 4);
    });
}

#[test]
fn resolve_directory_resolve_to_multiple_directories() {
    disk_test(|f| test_resolve_directory_with_target(f, false));
}

#[test]
fn resolve_directory_resolve_to_with_a_uri_that_has_query_parameter() {
    disk_test(|f| test_resolve_directory_with_target(f, true));
}

async fn test_resolve_directory_with_target(f: Fixture, with_query_param: bool) {
    let resolver_fixtures_path = fixture_path("resolver");
    let query = with_query_param.then_some("test");
    let options = ResolveFileOptions {
        resolve_to: vec![
            with_query(file_uri(resolver_fixtures_path.join("other/deep")), query),
            with_query(file_uri(resolver_fixtures_path.join("examples")), query),
        ],
        ..Default::default()
    };
    let result = f.service.resolve(&with_query(file_uri(&resolver_fixtures_path), query), Some(options)).await.unwrap();

    let children = result.children.as_ref().expect("filhos esperados");
    assert!(!children.is_empty());
    assert!(result.is_directory);

    assert_eq!(children.len(), 4);

    let other = get_by_name(&result, "other").unwrap();
    assert!(!other.children.as_ref().unwrap().is_empty());

    let deep = get_by_name(other, "deep").unwrap();
    assert!(!deep.children.as_ref().unwrap().is_empty());
    assert_eq!(deep.children.as_ref().unwrap().len(), 4);

    let examples = get_by_name(&result, "examples").unwrap();
    assert!(!examples.children.as_ref().unwrap().is_empty());
    assert_eq!(examples.children.as_ref().unwrap().len(), 4);
}

#[test]
fn resolve_directory_resolve_single_child_folders() {
    disk_test(|f| async move {
        let resolver_fixtures_path = fixture_path("resolver/other");
        let options = ResolveFileOptions { resolve_single_child_descendants: true, ..Default::default() };
        let result = f.service.resolve(&file_uri(&resolver_fixtures_path), Some(options)).await.unwrap();

        let children = result.children.as_ref().expect("filhos esperados");
        assert!(!children.is_empty());
        assert!(result.is_directory);

        assert_eq!(children.len(), 1);

        let deep = get_by_name(&result, "deep").unwrap();
        assert!(!deep.children.as_ref().unwrap().is_empty());
        assert_eq!(deep.children.as_ref().unwrap().len(), 4);
    });
}

#[test]
fn resolves() {
    disk_test(|f| async move {
        let options = ResolveFileOptions { resolve_to: vec![f.uri("deep")], ..Default::default() };
        let res = f.service.resolve_all(vec![(f.test_dir.root_uri(), Some(options)), (f.uri("deep"), None)]).await.unwrap();

        let r1 = res[0].stat.as_ref().unwrap();
        assert_eq!(r1.children.as_ref().unwrap().len(), 8);

        let deep = get_by_name(r1, "deep").unwrap();
        assert_eq!(deep.children.as_ref().unwrap().len(), 4);

        let r2 = res[1].stat.as_ref().unwrap();
        assert_eq!(r2.children.as_ref().unwrap().len(), 4);
        assert_eq!(r2.name, "deep");
    });
}

#[test]
fn resolve_realpath_folder_symbolic_link() {
    disk_test(|f| async move {
        let link = f.uri("deep-link");
        symlink(&f.test_dir.join("deep"), Path::new(&fs_path(&link)));

        let resolved = f.service.resolve(&link, None).await.unwrap();
        assert_eq!(resolved.children.as_ref().unwrap().len(), 4);
        assert!(resolved.is_directory);
        assert!(resolved.is_symbolic_link);

        let realpath = f.service.realpath(&link).await.unwrap().expect("realpath esperado");
        assert_eq!(basename(&fs_path(&realpath)), "deep");
    });
}

#[test]
#[cfg_attr(windows, ignore = "windows: cannot create file symbolic link without elevated context")]
fn resolve_file_symbolic_link() {
    disk_test(|f| async move {
        let link = f.uri("lorem.txt-linked");
        symlink(&f.test_dir.join("lorem.txt"), Path::new(&fs_path(&link)));

        let resolved = f.service.resolve(&link, None).await.unwrap();
        assert!(!resolved.is_directory);
        assert!(resolved.is_symbolic_link);
    });
}

#[test]
fn resolve_symbolic_link_pointing_to_nonexistent_file_does_not_break() {
    disk_test(|f| async move {
        symlink(&f.test_dir.join("foo"), &f.test_dir.join("bar"));

        let resolved = f.service.resolve(&f.test_dir.root_uri(), None).await.unwrap();
        assert!(resolved.is_directory);
        assert_eq!(resolved.children.as_ref().unwrap().len(), 9);

        let resolved_link = resolved.children.as_ref().unwrap().iter().find(|child| child.name == "bar" && child.is_symbolic_link);
        let resolved_link = resolved_link.expect("o link esperado");

        assert!(!resolved_link.is_directory);
        assert!(!resolved_link.is_file);
    });
}

#[test]
fn stat_file() {
    disk_test(|f| async move {
        let resource = fixture_uri("resolver/index.html");
        let resolved = f.service.stat(&resource).await.unwrap();

        assert_eq!(resolved.name, "index.html");
        assert!(resolved.is_file);
        assert!(!resolved.is_directory);
        assert!(!resolved.readonly());
        assert!(!resolved.is_symbolic_link);
        assert_eq!(resolved.resource.to_string(false), resource.to_string(false));
        assert!(resolved.mtime() > 0);
        assert!(resolved.ctime() > 0);
        assert!(resolved.size() > 0);
    });
}

#[test]
fn stat_directory() {
    disk_test(|f| async move {
        let resource = fixture_uri("resolver");
        let result = f.service.stat(&resource).await.unwrap();

        assert_eq!(result.resource.to_string(false), resource.to_string(false));
        assert_eq!(result.name, "resolver");
        assert!(result.is_directory);
        assert!(!result.readonly());
        assert!(result.mtime() > 0);
        assert!(result.ctime() > 0);
    });
}

// The executable bit does not exist on Windows so use a condition not skip
#[cfg(not(windows))]
#[test]
fn stat_executable() {
    disk_test(|f| async move {
        let non_executable = fixture_uri("executable/non_executable");
        let mut resolved = f.service.stat(&non_executable).await.unwrap();
        assert!(resolved.is_file);
        assert!(!resolved.executable());

        let executable = fixture_uri("executable/executable");
        resolved = f.service.stat(&executable).await.unwrap();
        assert!(resolved.is_file);
        assert!(resolved.executable());
    });
}

#[test]
fn delete_file_non_recursive() {
    disk_test(|f| test_delete_file(f, false, false));
}

#[test]
fn delete_file_recursive() {
    disk_test(|f| test_delete_file(f, false, true));
}

// Pendente: o `DiskFileSystemProvider` do porte não implementa a lixeira do sistema (`shell.trashItem`), então o
// provider nunca anuncia `Trash` de verdade. No original o caso também é pulado no Linux ("trash is unreliable").
#[test]
#[ignore = "pendente: lixeira do sistema ainda não portada no provider de disco"]
fn delete_file_use_trash() {
    disk_test(|f| test_delete_file(f, true, false));
}

async fn test_delete_file(f: Fixture, use_trash: bool, recursive: bool) {
    let event = f.record_events();

    let resource = f.uri("deep/conway.js");
    let source = f.service.resolve(&resource, None).await.unwrap();

    let options = || Some(PartialFileDeleteOptions { use_trash: Some(use_trash), recursive: Some(recursive), atomic: None });
    assert!(f.service.can_delete(&source.resource, options()).await.unwrap().is_ok());
    f.service.del(&source.resource, options()).await.unwrap();

    assert!(!Path::new(&fs_path(&source.resource)).exists());

    {
        let event = event.borrow();
        let event = event.last().expect("evento esperado");
        assert_eq!(fs_path(&event.resource), fs_path(&resource));
        assert_eq!(event.operation, FileOperation::Delete);
    }

    let error = f.service.del(&source.resource, options()).await.expect_err("era esperado um erro");
    assert_eq!(operation_result(&error), FileOperationResult::FileNotFound);
}

#[test]
#[cfg_attr(windows, ignore = "windows: cannot create file symbolic link without elevated context")]
fn delete_file_symbolic_link_exists() {
    disk_test(|f| async move {
        let target = f.uri("lorem.txt");
        let link = f.uri("lorem.txt-linked");
        symlink(Path::new(&fs_path(&target)), Path::new(&fs_path(&link)));

        let source = f.service.resolve(&link, None).await.unwrap();

        let event = f.record_events();

        assert!(f.service.can_delete(&source.resource, None).await.unwrap().is_ok());
        f.service.del(&source.resource, None).await.unwrap();

        assert!(!Path::new(&fs_path(&source.resource)).exists());

        {
            let event = event.borrow();
            let event = event.last().expect("evento esperado");
            assert_eq!(fs_path(&event.resource), fs_path(&link));
            assert_eq!(event.operation, FileOperation::Delete);
        }

        assert!(Path::new(&fs_path(&target)).exists()); // target the link pointed to is never deleted
    });
}

#[test]
#[cfg_attr(windows, ignore = "windows: cannot create file symbolic link without elevated context")]
fn delete_file_symbolic_link_pointing_to_nonexistent_file() {
    disk_test(|f| async move {
        let target = f.uri("foo");
        let link = f.uri("bar");
        symlink(Path::new(&fs_path(&target)), Path::new(&fs_path(&link)));

        let event = f.record_events();

        assert!(f.service.can_delete(&link, None).await.unwrap().is_ok());
        f.service.del(&link, None).await.unwrap();

        assert!(!Path::new(&fs_path(&link)).exists());

        let event = event.borrow();
        let event = event.last().expect("evento esperado");
        assert_eq!(fs_path(&event.resource), fs_path(&link));
        assert_eq!(event.operation, FileOperation::Delete);
    });
}

#[test]
fn delete_folder_recursive() {
    disk_test(|f| test_delete_folder_recursive(f, false, None));
}

#[test]
fn delete_folder_recursive_atomic() {
    disk_test(|f| test_delete_folder_recursive(f, false, Some(FileAtomicOptions { postfix: ".vsctmp".to_string() })));
}

// Pendente: lixeira do sistema não portada (ver `delete_file_use_trash`).
#[test]
#[ignore = "pendente: lixeira do sistema ainda não portada no provider de disco"]
fn delete_folder_recursive_use_trash() {
    disk_test(|f| test_delete_folder_recursive(f, true, None));
}

async fn test_delete_folder_recursive(f: Fixture, use_trash: bool, atomic: Option<FileAtomicOptions>) {
    let event = f.record_events();

    let resource = f.uri("deep");
    let source = f.service.resolve(&resource, None).await.unwrap();

    let options = || Some(PartialFileDeleteOptions { recursive: Some(true), use_trash: Some(use_trash), atomic: atomic.clone() });
    assert!(f.service.can_delete(&source.resource, options()).await.unwrap().is_ok());
    f.service.del(&source.resource, options()).await.unwrap();

    assert!(!Path::new(&fs_path(&source.resource)).exists());
    let event = event.borrow();
    let event = event.last().expect("evento esperado");
    assert_eq!(fs_path(&event.resource), fs_path(&resource));
    assert_eq!(event.operation, FileOperation::Delete);
}

#[test]
fn delete_folder_non_recursive() {
    disk_test(|f| async move {
        let resource = f.uri("deep");
        let source = f.service.resolve(&resource, None).await.unwrap();

        assert!(f.service.can_delete(&source.resource, None).await.unwrap().is_err());

        assert!(f.service.del(&source.resource, None).await.is_err());
    });
}

#[test]
fn delete_folder_empty_folder_recursive() {
    disk_test(|f| test_delete_empty_folder(f, true));
}

#[test]
fn delete_folder_empty_folder_non_recursive() {
    disk_test(|f| test_delete_empty_folder(f, false));
}

async fn test_delete_empty_folder(f: Fixture, recursive: bool) {
    let resource = f.service.create_folder(&f.uri("deep/empty")).await.unwrap().resource.clone();

    let options = PartialFileDeleteOptions { recursive: Some(recursive), ..Default::default() };
    f.service.del(&resource, Some(options)).await.unwrap();

    assert!(!f.service.exists(&resource).await.unwrap());
}

#[test]
fn move_() {
    disk_test(|f| async move {
        let event = f.record_events();

        let source = f.uri("index.html");
        let source_contents = std::fs::read(fs_path(&source)).unwrap();

        let target = file_uri(Path::new(&fs_path(&source)).parent().unwrap().join("other.html"));

        assert!(f.service.can_move(&source, &target, None).await.unwrap().is_ok());
        let renamed = f.service.move_(&source, &target, None).await.unwrap();

        assert!(Path::new(&fs_path(&renamed.resource)).exists());
        assert!(!Path::new(&fs_path(&source)).exists());
        {
            let event = event.borrow();
            let event = event.last().expect("evento esperado");
            assert_eq!(fs_path(&event.resource), fs_path(&source));
            assert_eq!(event.operation, FileOperation::Move);
            assert_eq!(fs_path(&event.target.as_ref().unwrap().resource), fs_path(&renamed.resource));
        }

        let target_contents = std::fs::read(fs_path(&target)).unwrap();

        assert_eq!(source_contents.len(), target_contents.len());
        assert_eq!(String::from_utf8_lossy(&source_contents), String::from_utf8_lossy(&target_contents));
    });
}

#[test]
fn move_across_providers_buffered_to_buffered() {
    disk_test(|f| async move {
        f.file_provider.set_capabilities_keeping_case_sensitivity(Cap::FILE_OPEN_READ_WRITE_CLOSE);
        f.test_provider.set_capabilities_keeping_case_sensitivity(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        test_move_across_providers(&f, "index.html").await;
    });
}

#[test]
fn move_across_providers_unbuffered_to_unbuffered() {
    disk_test(|f| async move {
        f.file_provider.set_capabilities_keeping_case_sensitivity(Cap::FILE_READ_WRITE);
        f.test_provider.set_capabilities_keeping_case_sensitivity(Cap::FILE_READ_WRITE);

        test_move_across_providers(&f, "index.html").await;
    });
}

#[test]
fn move_across_providers_buffered_to_unbuffered() {
    disk_test(|f| async move {
        f.file_provider.set_capabilities_keeping_case_sensitivity(Cap::FILE_OPEN_READ_WRITE_CLOSE);
        f.test_provider.set_capabilities_keeping_case_sensitivity(Cap::FILE_READ_WRITE);

        test_move_across_providers(&f, "index.html").await;
    });
}

#[test]
fn move_across_providers_unbuffered_to_buffered() {
    disk_test(|f| async move {
        f.file_provider.set_capabilities_keeping_case_sensitivity(Cap::FILE_READ_WRITE);
        f.test_provider.set_capabilities_keeping_case_sensitivity(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        test_move_across_providers(&f, "index.html").await;
    });
}

#[test]
fn move_across_providers_large_buffered_to_buffered() {
    disk_test(|f| async move {
        f.file_provider.set_capabilities_keeping_case_sensitivity(Cap::FILE_OPEN_READ_WRITE_CLOSE);
        f.test_provider.set_capabilities_keeping_case_sensitivity(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        test_move_across_providers(&f, "lorem.txt").await;
    });
}

#[test]
fn move_across_providers_large_unbuffered_to_unbuffered() {
    disk_test(|f| async move {
        f.file_provider.set_capabilities_keeping_case_sensitivity(Cap::FILE_READ_WRITE);
        f.test_provider.set_capabilities_keeping_case_sensitivity(Cap::FILE_READ_WRITE);

        test_move_across_providers(&f, "lorem.txt").await;
    });
}

#[test]
fn move_across_providers_large_buffered_to_unbuffered() {
    disk_test(|f| async move {
        f.file_provider.set_capabilities_keeping_case_sensitivity(Cap::FILE_OPEN_READ_WRITE_CLOSE);
        f.test_provider.set_capabilities_keeping_case_sensitivity(Cap::FILE_READ_WRITE);

        test_move_across_providers(&f, "lorem.txt").await;
    });
}

#[test]
fn move_across_providers_large_unbuffered_to_buffered() {
    disk_test(|f| async move {
        f.file_provider.set_capabilities_keeping_case_sensitivity(Cap::FILE_READ_WRITE);
        f.test_provider.set_capabilities_keeping_case_sensitivity(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        test_move_across_providers(&f, "lorem.txt").await;
    });
}

async fn test_move_across_providers(f: &Fixture, source_file: &str) {
    let event = f.record_events();

    let source = f.uri(source_file);
    let source_contents = std::fs::read(fs_path(&source)).unwrap();

    let target = with_scheme(&file_uri(Path::new(&fs_path(&source)).parent().unwrap().join("other.html")), TEST_SCHEMA);

    assert!(f.service.can_move(&source, &target, None).await.unwrap().is_ok());
    let renamed = f.service.move_(&source, &target, None).await.unwrap();

    assert!(Path::new(&fs_path(&renamed.resource)).exists());
    assert!(!Path::new(&fs_path(&source)).exists());
    {
        let event = event.borrow();
        let event = event.last().expect("evento esperado");
        assert_eq!(fs_path(&event.resource), fs_path(&source));
        assert_eq!(event.operation, FileOperation::Copy);
        assert_eq!(fs_path(&event.target.as_ref().unwrap().resource), fs_path(&renamed.resource));
    }

    let target_contents = std::fs::read(fs_path(&target)).unwrap();

    assert_eq!(source_contents.len(), target_contents.len());
    assert_eq!(String::from_utf8_lossy(&source_contents), String::from_utf8_lossy(&target_contents));
}

#[test]
fn move_multi_folder() {
    disk_test(|f| async move {
        let event = f.record_events();

        let multi_folder_paths = ["a", "couple", "of", "folders"];
        let rename_to_path = multi_folder_paths.iter().fold(PathBuf::new(), |path, segment| path.join(segment)).join("other.html");

        let source = f.uri("index.html");
        let target = file_uri(Path::new(&fs_path(&source)).parent().unwrap().join(&rename_to_path));

        assert!(f.service.can_move(&source, &target, None).await.unwrap().is_ok());
        let renamed = f.service.move_(&source, &target, None).await.unwrap();

        assert!(Path::new(&fs_path(&renamed.resource)).exists());
        assert!(!Path::new(&fs_path(&source)).exists());
        let event = event.borrow();
        let event = event.last().expect("evento esperado");
        assert_eq!(fs_path(&event.resource), fs_path(&source));
        assert_eq!(event.operation, FileOperation::Move);
        assert_eq!(fs_path(&event.target.as_ref().unwrap().resource), fs_path(&renamed.resource));
    });
}

#[test]
fn move_directory() {
    disk_test(|f| async move {
        let event = f.record_events();

        let source = f.uri("deep");
        let target = file_uri(Path::new(&fs_path(&source)).parent().unwrap().join("deeper"));

        assert!(f.service.can_move(&source, &target, None).await.unwrap().is_ok());
        let renamed = f.service.move_(&source, &target, None).await.unwrap();

        assert!(Path::new(&fs_path(&renamed.resource)).exists());
        assert!(!Path::new(&fs_path(&source)).exists());
        let event = event.borrow();
        let event = event.last().expect("evento esperado");
        assert_eq!(fs_path(&event.resource), fs_path(&source));
        assert_eq!(event.operation, FileOperation::Move);
        assert_eq!(fs_path(&event.target.as_ref().unwrap().resource), fs_path(&renamed.resource));
    });
}

#[test]
fn move_directory_across_providers_buffered_to_buffered() {
    disk_test(|f| async move {
        f.file_provider.set_capabilities_keeping_case_sensitivity(Cap::FILE_OPEN_READ_WRITE_CLOSE);
        f.test_provider.set_capabilities_keeping_case_sensitivity(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        test_move_folder_across_providers(&f).await;
    });
}

#[test]
fn move_directory_across_providers_unbuffered_to_unbuffered() {
    disk_test(|f| async move {
        f.file_provider.set_capabilities_keeping_case_sensitivity(Cap::FILE_READ_WRITE);
        f.test_provider.set_capabilities_keeping_case_sensitivity(Cap::FILE_READ_WRITE);

        test_move_folder_across_providers(&f).await;
    });
}

#[test]
fn move_directory_across_providers_buffered_to_unbuffered() {
    disk_test(|f| async move {
        f.file_provider.set_capabilities_keeping_case_sensitivity(Cap::FILE_OPEN_READ_WRITE_CLOSE);
        f.test_provider.set_capabilities_keeping_case_sensitivity(Cap::FILE_READ_WRITE);

        test_move_folder_across_providers(&f).await;
    });
}

#[test]
fn move_directory_across_providers_unbuffered_to_buffered() {
    disk_test(|f| async move {
        f.file_provider.set_capabilities_keeping_case_sensitivity(Cap::FILE_READ_WRITE);
        f.test_provider.set_capabilities_keeping_case_sensitivity(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        test_move_folder_across_providers(&f).await;
    });
}

async fn test_move_folder_across_providers(f: &Fixture) {
    let event = f.record_events();

    let source = f.uri("deep");
    let source_children = read_dir_sorted(fs_path(&source));

    let target = with_scheme(&file_uri(Path::new(&fs_path(&source)).parent().unwrap().join("deeper")), TEST_SCHEMA);

    assert!(f.service.can_move(&source, &target, None).await.unwrap().is_ok());
    let renamed = f.service.move_(&source, &target, None).await.unwrap();

    assert!(Path::new(&fs_path(&renamed.resource)).exists());
    assert!(!Path::new(&fs_path(&source)).exists());
    {
        let event = event.borrow();
        let event = event.last().expect("evento esperado");
        assert_eq!(fs_path(&event.resource), fs_path(&source));
        assert_eq!(event.operation, FileOperation::Copy);
        assert_eq!(fs_path(&event.target.as_ref().unwrap().resource), fs_path(&renamed.resource));
    }

    let target_children = read_dir_sorted(fs_path(&target));
    assert_eq!(source_children, target_children);
}

#[test]
fn move_mix_case() {
    disk_test(|f| async move {
        let event = f.record_events();

        let options = ResolveFileOptions { resolve_metadata: true, ..Default::default() };
        let source = f.service.resolve_with_metadata(&f.uri("index.html"), Some(options.clone())).await.unwrap();
        assert!(source.size() > 0);

        let renamed_resource = file_uri(Path::new(&fs_path(&source.resource)).parent().unwrap().join("INDEX.html"));
        assert!(f.service.can_move(&source.resource, &renamed_resource, None).await.unwrap().is_ok());
        let _renamed = f.service.move_(&source.resource, &renamed_resource, None).await.unwrap();

        assert!(Path::new(&fs_path(&renamed_resource)).exists());
        assert_eq!(basename(&fs_path(&renamed_resource)), "INDEX.html");
        {
            let event = event.borrow();
            let event = event.last().expect("evento esperado");
            assert_eq!(fs_path(&event.resource), fs_path(&source.resource));
            assert_eq!(event.operation, FileOperation::Move);
            assert_eq!(fs_path(&event.target.as_ref().unwrap().resource), fs_path(&renamed_resource));
        }

        let renamed = f.service.resolve_with_metadata(&renamed_resource, Some(options)).await.unwrap();
        assert_eq!(source.size(), renamed.size());
    });
}

#[test]
fn move_same_file() {
    disk_test(|f| async move {
        let event = f.record_events();

        let options = ResolveFileOptions { resolve_metadata: true, ..Default::default() };
        let source = f.service.resolve_with_metadata(&f.uri("index.html"), Some(options.clone())).await.unwrap();
        assert!(source.size() > 0);

        let same = file_uri(fs_path(&source.resource));
        assert!(f.service.can_move(&source.resource, &same, None).await.unwrap().is_ok());
        let renamed = f.service.move_(&source.resource, &same, None).await.unwrap();

        assert!(Path::new(&fs_path(&renamed.resource)).exists());
        assert_eq!(basename(&fs_path(&renamed.resource)), "index.html");
        {
            let event = event.borrow();
            let event = event.last().expect("evento esperado");
            assert_eq!(fs_path(&event.resource), fs_path(&source.resource));
            assert_eq!(event.operation, FileOperation::Move);
            assert_eq!(fs_path(&event.target.as_ref().unwrap().resource), fs_path(&renamed.resource));
        }

        let renamed = f.service.resolve_with_metadata(&renamed.resource, Some(options)).await.unwrap();
        assert_eq!(source.size(), renamed.size());
    });
}

#[test]
fn move_same_file_2() {
    disk_test(|f| async move {
        let event = f.record_events();

        let options = ResolveFileOptions { resolve_metadata: true, ..Default::default() };
        let source = f.service.resolve_with_metadata(&f.uri("index.html"), Some(options.clone())).await.unwrap();
        assert!(source.size() > 0);

        let target = with_joined_path(&f.test_dir.root_uri(), &basename(&source.resource.path().to_string_lossy()));

        assert!(f.service.can_move(&source.resource, &target, None).await.unwrap().is_ok());
        let renamed = f.service.move_(&source.resource, &target, None).await.unwrap();

        assert!(Path::new(&fs_path(&renamed.resource)).exists());
        assert_eq!(basename(&fs_path(&renamed.resource)), "index.html");
        {
            let event = event.borrow();
            let event = event.last().expect("evento esperado");
            assert_eq!(fs_path(&event.resource), fs_path(&source.resource));
            assert_eq!(event.operation, FileOperation::Move);
            assert_eq!(fs_path(&event.target.as_ref().unwrap().resource), fs_path(&renamed.resource));
        }

        let renamed = f.service.resolve_with_metadata(&renamed.resource, Some(options)).await.unwrap();
        assert_eq!(source.size(), renamed.size());
    });
}

#[test]
fn move_source_parent_of_target() {
    disk_test(|f| async move {
        let event = f.record_events();

        let options = ResolveFileOptions { resolve_metadata: true, ..Default::default() };
        let mut source = f.service.resolve_with_metadata(&f.uri("index.html"), Some(options.clone())).await.unwrap();
        let original_size = source.size();
        assert!(original_size > 0);

        assert!(f.service.can_move(&f.test_dir.root_uri(), &f.uri("binary.txt"), None).await.unwrap().is_err());

        let result = f.service.move_(&f.test_dir.root_uri(), &f.uri("binary.txt"), None).await;

        assert!(result.is_err());
        assert!(event.borrow().is_empty());

        source = f.service.resolve_with_metadata(&source.resource, Some(options)).await.unwrap();
        assert_eq!(original_size, source.size());
    });
}

#[test]
fn move_file_move_conflict() {
    disk_test(|f| async move {
        let event = f.record_events();

        let options = ResolveFileOptions { resolve_metadata: true, ..Default::default() };
        let mut source = f.service.resolve_with_metadata(&f.uri("index.html"), Some(options.clone())).await.unwrap();
        let original_size = source.size();
        assert!(original_size > 0);

        assert!(f.service.can_move(&source.resource, &f.uri("binary.txt"), None).await.unwrap().is_err());

        let error = f.service.move_(&source.resource, &f.uri("binary.txt"), None).await.expect_err("era esperado um erro");

        assert_eq!(operation_result(&error), FileOperationResult::FileMoveConflict);
        assert!(event.borrow().is_empty());

        source = f.service.resolve_with_metadata(&source.resource, Some(options)).await.unwrap();
        assert_eq!(original_size, source.size());
    });
}

#[test]
fn move_overwrite_folder_with_file() {
    disk_test(|f| async move {
        let events = f.record_events();

        let parent = f.service.resolve(&f.test_dir.root_uri(), None).await.unwrap();
        let folder_resource = file_uri(Path::new(&fs_path(&parent.resource)).join("conway.js"));
        let folder = f.service.create_folder(&folder_resource).await.unwrap();
        let source = f.uri("deep/conway.js");

        assert!(f.service.can_move(&source, &folder.resource, Some(true)).await.unwrap().is_ok());
        let moved = f.service.move_(&source, &folder.resource, Some(true)).await.unwrap();

        assert!(Path::new(&fs_path(&moved.resource)).exists());
        assert!(std::fs::metadata(fs_path(&moved.resource)).unwrap().is_file());
        let events = events.borrow();
        let last_of = |operation: FileOperation| events.iter().rev().find(|e| e.operation == operation).expect("evento esperado");
        let _create_event = last_of(FileOperation::Create);
        let delete_event = last_of(FileOperation::Delete);
        let move_event = last_of(FileOperation::Move);
        assert_eq!(fs_path(&move_event.resource), fs_path(&source));
        assert_eq!(fs_path(&move_event.target.as_ref().unwrap().resource), fs_path(&moved.resource));
        assert_eq!(fs_path(&delete_event.resource), fs_path(&folder_resource));
    });
}

#[test]
fn copy() {
    disk_test(|f| async move {
        do_test_copy(&f, "index.html").await;
    });
}

#[test]
fn copy_unbuffered_file_system_provider_capabilities_file_read_write() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE);

        do_test_copy(&f, "index.html").await;
    });
}

#[test]
fn copy_unbuffered_large_file_system_provider_capabilities_file_read_write() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE);

        do_test_copy(&f, "lorem.txt").await;
    });
}

#[test]
fn copy_buffered_file_system_provider_capabilities_file_open_read_write_close() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        do_test_copy(&f, "index.html").await;
    });
}

#[test]
fn copy_buffered_large_file_system_provider_capabilities_file_open_read_write_close() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        do_test_copy(&f, "lorem.txt").await;
    });
}

async fn do_test_copy(f: &Fixture, source_name: &str) {
    let event = f.record_events();

    let source = f.service.resolve(&f.uri(source_name), None).await.unwrap();
    let target = f.uri("other.html");

    assert!(f.service.can_copy(&source.resource, &target, None).await.unwrap().is_ok());
    let copied = f.service.copy(&source.resource, &target, None).await.unwrap();

    assert!(Path::new(&fs_path(&copied.resource)).exists());
    assert!(Path::new(&fs_path(&source.resource)).exists());
    {
        let event = event.borrow();
        let event = event.last().expect("evento esperado");
        assert_eq!(fs_path(&event.resource), fs_path(&source.resource));
        assert_eq!(event.operation, FileOperation::Copy);
        assert_eq!(fs_path(&event.target.as_ref().unwrap().resource), fs_path(&copied.resource));
    }

    let source_contents = std::fs::read(fs_path(&source.resource)).unwrap();
    let target_contents = std::fs::read(fs_path(&target)).unwrap();

    assert_eq!(source_contents.len(), target_contents.len());
    assert_eq!(String::from_utf8_lossy(&source_contents), String::from_utf8_lossy(&target_contents));
}

#[test]
fn copy_overwrite_folder_with_file() {
    disk_test(|f| async move {
        let events = f.record_events();

        let parent = f.service.resolve(&f.test_dir.root_uri(), None).await.unwrap();
        let folder_resource = file_uri(Path::new(&fs_path(&parent.resource)).join("conway.js"));
        let folder = f.service.create_folder(&folder_resource).await.unwrap();
        let source = f.uri("deep/conway.js");

        assert!(f.service.can_copy(&source, &folder.resource, Some(true)).await.unwrap().is_ok());
        let copied = f.service.copy(&source, &folder.resource, Some(true)).await.unwrap();

        assert!(Path::new(&fs_path(&copied.resource)).exists());
        assert!(std::fs::metadata(fs_path(&copied.resource)).unwrap().is_file());
        let events = events.borrow();
        let last_of = |operation: FileOperation| events.iter().rev().find(|e| e.operation == operation).expect("evento esperado");
        let _create_event = last_of(FileOperation::Create);
        let delete_event = last_of(FileOperation::Delete);
        let copy_event = last_of(FileOperation::Copy);
        assert_eq!(fs_path(&copy_event.resource), fs_path(&source));
        assert_eq!(fs_path(&copy_event.target.as_ref().unwrap().resource), fs_path(&copied.resource));
        assert_eq!(fs_path(&delete_event.resource), fs_path(&folder_resource));
    });
}

#[test]
fn copy_mix_case_same_target_no_overwrite() {
    disk_test(|f| async move {
        let options = ResolveFileOptions { resolve_metadata: true, ..Default::default() };
        let mut source = f.service.resolve_with_metadata(&f.uri("index.html"), Some(options.clone())).await.unwrap();
        let original_size = source.size();
        assert!(original_size > 0);

        let target = file_uri(Path::new(&fs_path(&source.resource)).parent().unwrap().join("INDEX.html"));

        let can_copy = f.service.can_copy(&source.resource, &target, None).await.unwrap();

        let copied = f.service.copy(&source.resource, &target, None).await;

        if cfg!(target_os = "linux") {
            let copied = copied.expect("não era esperado um erro");
            assert!(can_copy.is_ok());

            assert!(Path::new(&fs_path(&copied.resource)).exists());
            assert!(read_dir_sorted(f.test_dir.path()).iter().any(|name| name == "INDEX.html"));
            assert_eq!(source.size(), copied.size());
        } else {
            assert!(copied.is_err());
            assert!(can_copy.is_err());

            source = f.service.resolve_with_metadata(&source.resource, Some(options)).await.unwrap();
            assert_eq!(original_size, source.size());
        }
    });
}

#[test]
fn copy_mix_case_same_target_overwrite() {
    disk_test(|f| async move {
        let options = ResolveFileOptions { resolve_metadata: true, ..Default::default() };
        let mut source = f.service.resolve_with_metadata(&f.uri("index.html"), Some(options.clone())).await.unwrap();
        let original_size = source.size();
        assert!(original_size > 0);

        let target = file_uri(Path::new(&fs_path(&source.resource)).parent().unwrap().join("INDEX.html"));

        let can_copy = f.service.can_copy(&source.resource, &target, Some(true)).await.unwrap();

        let copied = f.service.copy(&source.resource, &target, Some(true)).await;

        if cfg!(target_os = "linux") {
            let copied = copied.expect("não era esperado um erro");
            assert!(can_copy.is_ok());

            assert!(Path::new(&fs_path(&copied.resource)).exists());
            assert!(read_dir_sorted(f.test_dir.path()).iter().any(|name| name == "INDEX.html"));
            assert_eq!(source.size(), copied.size());
        } else {
            assert!(copied.is_err());
            assert!(can_copy.is_err());

            source = f.service.resolve_with_metadata(&source.resource, Some(options)).await.unwrap();
            assert_eq!(original_size, source.size());
        }
    });
}

#[test]
fn copy_mix_case_different_target_overwrite() {
    disk_test(|f| async move {
        let options = ResolveFileOptions { resolve_metadata: true, ..Default::default() };
        let source1 = f.service.resolve_with_metadata(&f.uri("index.html"), Some(options.clone())).await.unwrap();
        assert!(source1.size() > 0);

        let renamed = f.service.move_(&source1.resource, &file_uri(Path::new(&fs_path(&source1.resource)).parent().unwrap().join("CONWAY.js")), None).await.unwrap();
        assert!(Path::new(&fs_path(&renamed.resource)).exists());
        assert!(read_dir_sorted(f.test_dir.path()).iter().any(|name| name == "CONWAY.js"));
        assert_eq!(source1.size(), renamed.size());

        let source2 = f.service.resolve_with_metadata(&f.uri("deep/conway.js"), Some(options)).await.unwrap();
        let target = f.uri(&basename(&source2.resource.path().to_string_lossy()));

        assert!(f.service.can_copy(&source2.resource, &target, Some(true)).await.unwrap().is_ok());
        let res = f.service.copy(&source2.resource, &target, Some(true)).await.unwrap();
        assert!(Path::new(&fs_path(&res.resource)).exists());
        assert!(read_dir_sorted(f.test_dir.path()).iter().any(|name| name == "conway.js"));
        assert_eq!(source2.size(), res.size());
    });
}

#[test]
fn copy_same_file() {
    disk_test(|f| async move {
        let event = f.record_events();

        let options = ResolveFileOptions { resolve_metadata: true, ..Default::default() };
        let source = f.service.resolve_with_metadata(&f.uri("index.html"), Some(options.clone())).await.unwrap();
        assert!(source.size() > 0);

        let same = file_uri(fs_path(&source.resource));
        assert!(f.service.can_copy(&source.resource, &same, None).await.unwrap().is_ok());
        let copied = f.service.copy(&source.resource, &same, None).await.unwrap();

        assert!(Path::new(&fs_path(&copied.resource)).exists());
        assert_eq!(basename(&fs_path(&copied.resource)), "index.html");
        {
            let event = event.borrow();
            let event = event.last().expect("evento esperado");
            assert_eq!(fs_path(&event.resource), fs_path(&source.resource));
            assert_eq!(event.operation, FileOperation::Copy);
            assert_eq!(fs_path(&event.target.as_ref().unwrap().resource), fs_path(&copied.resource));
        }

        let copied = f.service.resolve_with_metadata(&source.resource, Some(options)).await.unwrap();
        assert_eq!(source.size(), copied.size());
    });
}

#[test]
fn copy_same_file_2() {
    disk_test(|f| async move {
        let event = f.record_events();

        let options = ResolveFileOptions { resolve_metadata: true, ..Default::default() };
        let source = f.service.resolve_with_metadata(&f.uri("index.html"), Some(options.clone())).await.unwrap();
        assert!(source.size() > 0);

        let target = with_joined_path(&f.test_dir.root_uri(), &basename(&source.resource.path().to_string_lossy()));
        let target_file = file_uri(fs_path(&target));

        assert!(f.service.can_copy(&source.resource, &target_file, None).await.unwrap().is_ok());
        let copied = f.service.copy(&source.resource, &target_file, None).await.unwrap();

        assert!(Path::new(&fs_path(&copied.resource)).exists());
        assert_eq!(basename(&fs_path(&copied.resource)), "index.html");
        {
            let event = event.borrow();
            let event = event.last().expect("evento esperado");
            assert_eq!(fs_path(&event.resource), fs_path(&source.resource));
            assert_eq!(event.operation, FileOperation::Copy);
            assert_eq!(fs_path(&event.target.as_ref().unwrap().resource), fs_path(&copied.resource));
        }

        let copied = f.service.resolve_with_metadata(&source.resource, Some(options)).await.unwrap();
        assert_eq!(source.size(), copied.size());
    });
}

#[test]
fn clone_file_basics() {
    disk_test(|f| async move {
        test_clone_file(&f).await;
    });
}

#[test]
fn clone_file_via_copy_capability() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE | Cap::FILE_FOLDER_COPY);

        test_clone_file(&f).await;
    });
}

#[test]
fn clone_file_via_pipe() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        test_clone_file(&f).await;
    });
}

async fn test_clone_file(f: &Fixture) {
    let size_of = |resource: URI| {
        let service = f.service.clone();
        async move {
            let options = ResolveFileOptions { resolve_metadata: true, ..Default::default() };
            service.resolve_with_metadata(&resource, Some(options)).await.unwrap().size()
        }
    };

    let source1 = f.uri("index.html");
    let source1_size = size_of(source1.clone()).await;

    let source2 = f.uri("lorem.txt");
    let source2_size = size_of(source2.clone()).await;

    let target_parent = f.test_dir.root_uri();

    // same path is a no-op
    f.service.clone_file(&source1, &source1).await.unwrap();

    // simple clone to existing parent folder path
    let target1 = with_joined_path(&target_parent, &format!("{}-clone", basename(&source1.path().to_string_lossy())));

    f.service.clone_file(&source1, &file_uri(fs_path(&target1))).await.unwrap();

    assert!(Path::new(&fs_path(&target1)).exists());
    assert_eq!(basename(&fs_path(&target1)), "index.html-clone");

    let mut target1_size = size_of(target1.clone()).await;

    assert_eq!(source1_size, target1_size);

    // clone to same path overwrites
    f.service.clone_file(&source2, &file_uri(fs_path(&target1))).await.unwrap();

    target1_size = size_of(target1.clone()).await;

    assert_eq!(source2_size, target1_size);
    assert_ne!(source1_size, target1_size);

    // clone creates missing folders ad-hoc
    let target2 = with_joined_path(&target_parent, &format!("foo/bar/{}-clone", basename(&source1.path().to_string_lossy())));

    f.service.clone_file(&source1, &file_uri(fs_path(&target2))).await.unwrap();

    assert!(Path::new(&fs_path(&target2)).exists());
    assert_eq!(basename(&fs_path(&target2)), "index.html-clone");

    let target2_size = size_of(target2.clone()).await;

    assert_eq!(source1_size, target2_size);
}

#[test]
fn read_file_small_file_default() {
    disk_test(|f| async move {
        test_read_file(&f, f.uri("small.txt"), None).await;
    });
}

#[test]
fn read_file_small_file_buffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        test_read_file(&f, f.uri("small.txt"), None).await;
    });
}

#[test]
fn read_file_small_file_buffered_readonly() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE | Cap::READONLY);

        test_read_file(&f, f.uri("small.txt"), None).await;
    });
}

#[test]
fn read_file_small_file_unbuffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE);

        test_read_file(&f, f.uri("small.txt"), None).await;
    });
}

#[test]
fn read_file_small_file_unbuffered_readonly() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE | Cap::READONLY);

        test_read_file(&f, f.uri("small.txt"), None).await;
    });
}

#[test]
fn read_file_small_file_streamed() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_STREAM);

        test_read_file(&f, f.uri("small.txt"), None).await;
    });
}

#[test]
fn read_file_small_file_streamed_readonly() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_STREAM | Cap::READONLY);

        test_read_file(&f, f.uri("small.txt"), None).await;
    });
}

#[test]
fn read_file_large_file_default() {
    disk_test(|f| async move {
        test_read_file(&f, f.uri("lorem.txt"), None).await;
    });
}

#[test]
fn read_file_large_file_buffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        test_read_file(&f, f.uri("lorem.txt"), None).await;
    });
}

#[test]
fn read_file_large_file_unbuffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE);

        test_read_file(&f, f.uri("lorem.txt"), None).await;
    });
}

#[test]
fn read_file_large_file_streamed() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_STREAM);

        test_read_file(&f, f.uri("lorem.txt"), None).await;
    });
}

#[test]
fn read_file_atomic_emulated_on_service_level() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_STREAM);

        test_read_file(&f, f.uri("lorem.txt"), Some(ReadFileOptions { atomic: true, ..Default::default() })).await;
    });
}

#[test]
fn read_file_atomic_natively_supported() {
    disk_test(|f| async move {
        // `FileReadWrite & FileAtomicRead` é zero (as máscaras não se sobrepõem) e zero volta ao padrão do provider.
        f.set_capabilities(Cap::FILE_READ_WRITE & Cap::FILE_ATOMIC_READ);

        test_read_file(&f, f.uri("lorem.txt"), Some(ReadFileOptions { atomic: true, ..Default::default() })).await;
    });
}

async fn test_read_file(f: &Fixture, resource: URI, options: Option<ReadFileOptions>) {
    let content = f.service.read_file(&resource, options, None).await.unwrap();

    assert_eq!(content.value.to_string(), read_to_string(fs_path(&resource)));
}

#[test]
fn read_file_stream_small_file_default() {
    disk_test(|f| async move {
        test_read_file_stream(&f, f.uri("small.txt")).await;
    });
}

#[test]
fn read_file_stream_small_file_buffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        test_read_file_stream(&f, f.uri("small.txt")).await;
    });
}

#[test]
fn read_file_stream_small_file_unbuffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE);

        test_read_file_stream(&f, f.uri("small.txt")).await;
    });
}

#[test]
fn read_file_stream_small_file_streamed() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_STREAM);

        test_read_file_stream(&f, f.uri("small.txt")).await;
    });
}

async fn test_read_file_stream(f: &Fixture, resource: URI) {
    let content = f.service.read_file_stream(&resource, None, None).await.unwrap();

    assert_eq!(stream_to_buffer(&*content.value).await.unwrap().to_string(), read_to_string(fs_path(&resource)));
}

#[test]
fn read_file_files_are_intermingled_38331_default() {
    disk_test(|f| async move {
        test_files_not_intermingled(&f).await;
    });
}

#[test]
fn read_file_files_are_intermingled_38331_buffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        test_files_not_intermingled(&f).await;
    });
}

#[test]
fn read_file_files_are_intermingled_38331_unbuffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE);

        test_files_not_intermingled(&f).await;
    });
}

#[test]
fn read_file_files_are_intermingled_38331_streamed() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_STREAM);

        test_files_not_intermingled(&f).await;
    });
}

async fn test_files_not_intermingled(f: &Fixture) {
    let resource1 = f.uri("lorem.txt");
    let resource2 = f.uri("some_utf16le.css");

    // load in sequence and keep data
    let value1 = f.service.read_file(&resource1, None, None).await.unwrap();
    let value2 = f.service.read_file(&resource2, None, None).await.unwrap();

    // load in parallel in expect the same result
    let result = Promise::all(vec![f.service.read_file(&resource1, None, None), f.service.read_file(&resource2, None, None)]).await.unwrap();

    assert_eq!(result[0].value.to_string(), value1.value.to_string());
    assert_eq!(result[1].value.to_string(), value2.value.to_string());
}

fn read_options(position: Option<u64>, length: Option<u64>) -> Option<ReadFileOptions> {
    Some(ReadFileOptions { stream: FileReadStreamOptions { position, length, limit_size: None }, ..Default::default() })
}

#[test]
fn read_file_from_position_ascii_default() {
    disk_test(|f| async move {
        test_read_file_from_position_ascii(&f).await;
    });
}

#[test]
fn read_file_from_position_ascii_buffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        test_read_file_from_position_ascii(&f).await;
    });
}

#[test]
fn read_file_from_position_ascii_unbuffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE);

        test_read_file_from_position_ascii(&f).await;
    });
}

#[test]
fn read_file_from_position_ascii_streamed() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_STREAM);

        test_read_file_from_position_ascii(&f).await;
    });
}

async fn test_read_file_from_position_ascii(f: &Fixture) {
    let resource = f.uri("small.txt");

    let contents = f.service.read_file(&resource, read_options(Some(6), None), None).await.unwrap();

    assert_eq!(contents.value.to_string(), "File");
}

#[test]
fn read_file_from_position_with_umlaut_default() {
    disk_test(|f| async move {
        test_read_file_from_position_umlaut(&f).await;
    });
}

#[test]
fn read_file_from_position_with_umlaut_buffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        test_read_file_from_position_umlaut(&f).await;
    });
}

#[test]
fn read_file_from_position_with_umlaut_unbuffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE);

        test_read_file_from_position_umlaut(&f).await;
    });
}

#[test]
fn read_file_from_position_with_umlaut_streamed() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_STREAM);

        test_read_file_from_position_umlaut(&f).await;
    });
}

async fn test_read_file_from_position_umlaut(f: &Fixture) {
    let resource = f.uri("small_umlaut.txt");

    let contents = f.service.read_file(&resource, read_options(Some("Small File with Ü".len() as u64), None), None).await.unwrap();

    assert_eq!(contents.value.to_string(), "mlaut");
}

#[test]
fn read_file_3_bytes_ascii_default() {
    disk_test(|f| async move {
        test_read_three_bytes_from_file(&f).await;
    });
}

#[test]
fn read_file_3_bytes_ascii_buffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        test_read_three_bytes_from_file(&f).await;
    });
}

#[test]
fn read_file_3_bytes_ascii_unbuffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE);

        test_read_three_bytes_from_file(&f).await;
    });
}

#[test]
fn read_file_3_bytes_ascii_streamed() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_STREAM);

        test_read_three_bytes_from_file(&f).await;
    });
}

async fn test_read_three_bytes_from_file(f: &Fixture) {
    let resource = f.uri("small.txt");

    let contents = f.service.read_file(&resource, read_options(None, Some(3)), None).await.unwrap();

    assert_eq!(contents.value.to_string(), "Sma");
}

#[test]
fn read_file_20000_bytes_large_default() {
    disk_test(|f| async move {
        read_large_file_with_length(&f, 20000).await;
    });
}

#[test]
fn read_file_20000_bytes_large_buffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        read_large_file_with_length(&f, 20000).await;
    });
}

#[test]
fn read_file_20000_bytes_large_unbuffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE);

        read_large_file_with_length(&f, 20000).await;
    });
}

#[test]
fn read_file_20000_bytes_large_streamed() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_STREAM);

        read_large_file_with_length(&f, 20000).await;
    });
}

#[test]
fn read_file_80000_bytes_large_default() {
    disk_test(|f| async move {
        read_large_file_with_length(&f, 80000).await;
    });
}

#[test]
fn read_file_80000_bytes_large_buffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        read_large_file_with_length(&f, 80000).await;
    });
}

#[test]
fn read_file_80000_bytes_large_unbuffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE);

        read_large_file_with_length(&f, 80000).await;
    });
}

#[test]
fn read_file_80000_bytes_large_streamed() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_STREAM);

        read_large_file_with_length(&f, 80000).await;
    });
}

async fn read_large_file_with_length(f: &Fixture, length: u64) {
    let resource = f.uri("lorem.txt");

    let contents = f.service.read_file(&resource, read_options(None, Some(length)), None).await.unwrap();

    assert_eq!(contents.value.byte_length() as u64, length);
}

#[test]
fn read_file_file_is_directory() {
    disk_test(|f| async move {
        let error = f.service.read_file(&f.uri("deep"), None, None).await.expect_err("era esperado um erro");

        assert_eq!(operation_result(&error), FileOperationResult::FileIsDirectory);
    });
}

#[test]
#[cfg_attr(windows, ignore = "error code does not seem to be supported on windows")]
fn read_file_file_not_directory() {
    disk_test(|f| async move {
        let error = f.service.read_file(&f.uri("lorem.txt/file.txt"), None, None).await.expect_err("era esperado um erro");

        assert_eq!(operation_result(&error), FileOperationResult::FileNotDirectory);
    });
}

#[test]
fn read_file_file_not_found() {
    disk_test(|f| async move {
        let error = f.service.read_file(&f.uri("404.html"), None, None).await.expect_err("era esperado um erro");

        assert_eq!(operation_result(&error), FileOperationResult::FileNotFound);
    });
}

#[test]
fn read_file_file_not_modified_since_default() {
    disk_test(|f| async move {
        test_not_modified_since(&f).await;
    });
}

#[test]
fn read_file_file_not_modified_since_buffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        test_not_modified_since(&f).await;
    });
}

#[test]
fn read_file_file_not_modified_since_unbuffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE);

        test_not_modified_since(&f).await;
    });
}

#[test]
fn read_file_file_not_modified_since_streamed() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_STREAM);

        test_not_modified_since(&f).await;
    });
}

async fn test_not_modified_since(f: &Fixture) {
    let resource = f.uri("index.html");

    let contents = f.service.read_file(&resource, None, None).await.unwrap();
    f.file_provider.set_total_bytes_read(0);

    let options = ReadFileOptions { etag: Some(contents.etag().to_string()), ..Default::default() };
    let error = f.service.read_file(&resource, Some(options), None).await.expect_err("era esperado um erro");

    let operation_error = FileOperationError::from_error(&error).expect("era esperado um FileOperationError");
    assert_eq!(operation_error.file_operation_result, FileOperationResult::FileNotModifiedSince);
    assert!(operation_error.stat.is_some()); // NotModifiedSinceFileOperationError
    assert_eq!(f.file_provider.total_bytes_read(), 0);
}

#[test]
fn read_file_file_not_modified_since_does_not_fire_wrongly_72909() {
    disk_test(|f| async move {
        f.file_provider.set_invalid_stat_size(true);

        let resource = f.uri("index.html");

        f.service.read_file(&resource, None, None).await.unwrap();

        f.service.read_file(&resource, Some(ReadFileOptions { etag: None, ..Default::default() }), None).await.expect("não era esperado um erro");
    });
}

#[test]
fn read_file_file_too_large_default() {
    disk_test(|f| async move {
        test_file_too_large(&f).await;
    });
}

#[test]
fn read_file_file_too_large_buffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        test_file_too_large(&f).await;
    });
}

#[test]
fn read_file_file_too_large_unbuffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE);

        test_file_too_large(&f).await;
    });
}

#[test]
fn read_file_file_too_large_streamed() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_STREAM);

        test_file_too_large(&f).await;
    });
}

async fn test_file_too_large(f: &Fixture) {
    do_test_file_too_large(f, false).await;

    // Also test when the stat size is wrong
    f.file_provider.set_small_stat_size(true);
    do_test_file_too_large(f, true).await;
}

async fn do_test_file_too_large(f: &Fixture, stat_size_wrong: bool) {
    let resource = f.uri("index.html");

    let options = ReadFileOptions { stream: FileReadStreamOptions { limit_size: Some(10), ..Default::default() }, ..Default::default() };
    let error = f.service.read_file(&resource, Some(options), None).await.expect_err("era esperado um erro");

    let operation_error = FileOperationError::from_error(&error).expect("era esperado um FileOperationError");
    if !stat_size_wrong {
        assert!(operation_error.size.is_some()); // TooLargeFileOperationError
    }
    assert_eq!(operation_error.file_operation_result, FileOperationResult::FileTooLarge);
}

#[test]
#[cfg_attr(windows, ignore = "windows: cannot create file symbolic link without elevated context")]
fn read_file_dangling_symbolic_link_116049() {
    disk_test(|f| async move {
        let link = f.uri("small.js-link");
        symlink(&f.test_dir.join("small.js"), Path::new(&fs_path(&link)));

        assert!(f.service.read_file(&link, None, None).await.is_err());
    });
}

#[test]
fn create_file() {
    disk_test(|f| async move {
        assert_create_file(&f, buffer_source).await;
    });
}

#[test]
fn create_file_readable() {
    disk_test(|f| async move {
        assert_create_file(&f, |contents| {
            let readable: Box<dyn Readable<VSBuffer>> = Box::new(buffer_to_readable(VSBuffer::from_string(contents)));
            FileContentSource::from(readable)
        })
        .await;
    });
}

#[test]
fn create_file_stream() {
    disk_test(|f| async move {
        assert_create_file(&f, |contents| {
            let stream: VSBufferReadableStream = buffer_to_stream(VSBuffer::from_string(contents));
            FileContentSource::from(stream)
        })
        .await;
    });
}

async fn assert_create_file(f: &Fixture, converter: impl FnOnce(&str) -> FileContentSource) {
    let event = f.record_events();

    let contents = "Hello World";
    let resource = f.uri("test.txt");

    assert!(f.service.can_create_file(&resource, None).await.unwrap().is_ok());
    let file_stat = f.service.create_file(&resource, Some(converter(contents)), None).await.unwrap();
    assert_eq!(file_stat.name, "test.txt");
    assert!(Path::new(&fs_path(&file_stat.resource)).exists());
    assert_eq!(read_to_string(fs_path(&file_stat.resource)), contents);

    let event = event.borrow();
    let event = event.last().expect("evento esperado");
    assert_eq!(fs_path(&event.resource), fs_path(&resource));
    assert_eq!(event.operation, FileOperation::Create);
    assert_eq!(fs_path(&event.target.as_ref().unwrap().resource), fs_path(&resource));
}

#[test]
fn create_file_does_not_overwrite_by_default() {
    disk_test(|f| async move {
        let contents = "Hello World";
        let resource = f.uri("test.txt");

        std::fs::write(fs_path(&resource), "").unwrap(); // create file

        assert!(f.service.can_create_file(&resource, None).await.unwrap().is_err());

        assert!(f.service.create_file(&resource, Some(buffer_source(contents)), None).await.is_err());
    });
}

#[test]
fn create_file_allows_to_overwrite_existing() {
    disk_test(|f| async move {
        let event = f.record_events();

        let contents = "Hello World";
        let resource = f.uri("test.txt");

        std::fs::write(fs_path(&resource), "").unwrap(); // create file

        let overwrite = Some(CreateFileOptions { overwrite: true });
        assert!(f.service.can_create_file(&resource, overwrite).await.unwrap().is_ok());
        let file_stat = f.service.create_file(&resource, Some(buffer_source(contents)), overwrite).await.unwrap();
        assert_eq!(file_stat.name, "test.txt");
        assert!(Path::new(&fs_path(&file_stat.resource)).exists());
        assert_eq!(read_to_string(fs_path(&file_stat.resource)), contents);

        let event = event.borrow();
        let event = event.last().expect("evento esperado");
        assert_eq!(fs_path(&event.resource), fs_path(&resource));
        assert_eq!(event.operation, FileOperation::Create);
        assert_eq!(fs_path(&event.target.as_ref().unwrap().resource), fs_path(&resource));
    });
}

fn atomic_options(atomic: bool) -> Option<FileAtomicOptions> {
    atomic.then(|| FileAtomicOptions { postfix: ".vsctmp".to_string() })
}

#[test]
fn write_file_default() {
    disk_test(|f| async move {
        test_write_file(&f, false).await.unwrap();
    });
}

#[test]
fn write_file_flush_on_write() {
    disk_test_with_flush_on_write(|f| async move {
        test_write_file(&f, false).await.unwrap();
    });
}

#[test]
fn write_file_buffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        test_write_file(&f, false).await.unwrap();
    });
}

#[test]
fn write_file_unbuffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE);

        test_write_file(&f, false).await.unwrap();
    });
}

#[test]
fn write_file_default_atomic() {
    disk_test(|f| async move {
        test_write_file(&f, true).await.unwrap();
    });
}

#[test]
fn write_file_flush_on_write_atomic() {
    disk_test_with_flush_on_write(|f| async move {
        test_write_file(&f, true).await.unwrap();
    });
}

#[test]
fn write_file_buffered_atomic() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE | Cap::FILE_ATOMIC_WRITE);

        assert!(test_write_file(&f, true).await.is_err());
    });
}

#[test]
fn write_file_unbuffered_atomic() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE | Cap::FILE_ATOMIC_WRITE);

        test_write_file(&f, true).await.unwrap();
    });
}

#[test]
#[cfg_attr(windows, ignore = "windows: cannot create file symbolic link without elevated context")]
fn write_file_atomic_writing_does_not_break_symlinks() {
    disk_test(|f| async move {
        let link = f.uri("lorem.txt-linked");
        symlink(&f.test_dir.join("lorem.txt"), Path::new(&fs_path(&link)));

        let content = "Updates to the lorem file";
        let options = WriteFileOptions { atomic: atomic_options(true), ..Default::default() };
        f.service.write_file(&link, buffer_source(content), Some(options)).await.unwrap();
        assert_eq!(read_to_string(fs_path(&link)), content);

        let resolved = f.service.resolve(&link, None).await.unwrap();
        assert!(resolved.is_symbolic_link);
    });
}

async fn test_write_file(f: &Fixture, atomic: bool) -> Result<(), Error> {
    let event = f.record_events();

    let resource = f.uri("small.txt");

    let content = read_to_string(fs_path(&resource));
    assert_eq!(content, "Small File");

    let new_content = "Updates to the small file";
    let options = WriteFileOptions { atomic: atomic_options(atomic), ..Default::default() };
    f.service.write_file(&resource, buffer_source(new_content), Some(options)).await?;

    {
        let event = event.borrow();
        let event = event.last().expect("evento esperado");
        assert_eq!(fs_path(&event.resource), fs_path(&resource));
        assert_eq!(event.operation, FileOperation::Write);
    }

    assert_eq!(read_to_string(fs_path(&resource)), new_content);
    Ok(())
}

#[test]
fn write_file_large_file_default() {
    disk_test(|f| async move {
        test_write_file_large(&f, false).await.unwrap();
    });
}

#[test]
fn write_file_large_file_buffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        test_write_file_large(&f, false).await.unwrap();
    });
}

#[test]
fn write_file_large_file_unbuffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE);

        test_write_file_large(&f, false).await.unwrap();
    });
}

#[test]
fn write_file_large_file_default_atomic() {
    disk_test(|f| async move {
        test_write_file_large(&f, true).await.unwrap();
    });
}

#[test]
fn write_file_large_file_buffered_atomic() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE | Cap::FILE_ATOMIC_WRITE);

        assert!(test_write_file_large(&f, true).await.is_err());
    });
}

#[test]
fn write_file_large_file_unbuffered_atomic() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE | Cap::FILE_ATOMIC_WRITE);

        test_write_file_large(&f, true).await.unwrap();
    });
}

async fn test_write_file_large(f: &Fixture, atomic: bool) -> Result<(), Error> {
    let resource = f.uri("lorem.txt");

    let content = read_to_string(fs_path(&resource));
    let new_content = format!("{content}{content}");

    let options = WriteFileOptions { atomic: atomic_options(atomic), ..Default::default() };
    let file_stat = f.service.write_file(&resource, buffer_source(&new_content), Some(options)).await?;
    assert_eq!(file_stat.name, "lorem.txt");

    assert_eq!(read_to_string(fs_path(&resource)), new_content);
    Ok(())
}

#[test]
fn write_file_large_file_unbuffered_atomic_concurrent_writes_with_multiple_services() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE | Cap::FILE_ATOMIC_WRITE);

        let resource = f.uri("lorem.txt");

        let content = read_to_string(fs_path(&resource));
        let new_content = format!("{content}{content}");

        let mut promises = Vec::new();
        let mut suffix = 0;
        for _ in 0..10 {
            let service = f.disposables.add_rc(FileService::new(NullLogService::shared()));
            f.disposables.add(service.register_provider(schemas::FILE, f.file_provider.as_provider()).unwrap());

            suffix += 1;
            let options = WriteFileOptions { atomic: atomic_options(true), ..Default::default() };
            promises.push(service.write_file(&resource, buffer_source(&format!("{new_content}{suffix}")), Some(options)));
            timeout(0).await.unwrap();
        }

        let _ = Promise::all_settled(promises).await;

        assert_eq!(read_to_string(fs_path(&resource)), format!("{new_content}{suffix}"));
    });
}

#[test]
fn write_file_buffered_readonly_throws() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE | Cap::READONLY);

        test_write_file_readonly_throws(&f).await;
    });
}

#[test]
fn write_file_unbuffered_readonly_throws() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE | Cap::READONLY);

        test_write_file_readonly_throws(&f).await;
    });
}

async fn test_write_file_readonly_throws(f: &Fixture) {
    let resource = f.uri("small.txt");

    let content = read_to_string(fs_path(&resource));
    assert_eq!(content, "Small File");

    let new_content = "Updates to the small file";

    assert!(f.service.write_file(&resource, buffer_source(new_content), None).await.is_err());
}

#[test]
fn write_file_large_file_multiple_parallel_writes_queue_up_and_atomic_read_support_via_file_service() {
    disk_test(|f| async move {
        let resource = f.uri("lorem.txt");

        let content = read_to_string(fs_path(&resource));
        let new_content = format!("{content}{content}");

        let write_promises = Promise::all(
            ["0", "00", "000", "0000", "00000"]
                .into_iter()
                .map(|offset| {
                    let service = f.service.clone();
                    let resource = resource.clone();
                    let new_content = new_content.clone();
                    async_fn(async move {
                        let file_stat = service.write_file(&resource, buffer_source(&format!("{offset}{new_content}")), None).await?;
                        assert_eq!(file_stat.name, "lorem.txt");
                        Ok(())
                    })
                })
                .collect(),
        );

        let read_promises = Promise::all(
            (0..5)
                .map(|_| {
                    let service = f.service.clone();
                    let resource = resource.clone();
                    async_fn(async move {
                        let file_content = service.read_file(&resource, Some(ReadFileOptions { atomic: true, ..Default::default() }), None).await?;
                        assert!(file_content.value.byte_length() > 0); // `atomic: true` ensures we never read a truncated file
                        Ok(())
                    })
                })
                .collect(),
        );

        write_promises.await.unwrap();
        read_promises.await.unwrap();
    });
}

#[test]
fn provider_write_barrier_prevents_dirty_writes() {
    disk_test(|f| async move {
        let resource = f.uri("lorem.txt");

        let content = read_to_string(fs_path(&resource));
        let new_content = format!("{content}{content}");

        let provider = provider_of(&f, &resource);
        assert!(has_open_read_write_close_capability(&*provider));

        let write_promises = Promise::all(
            ["0", "00", "000", "0000", "00000"]
                .into_iter()
                .map(|offset| {
                    let provider = provider.clone();
                    let resource = resource.clone();
                    let content = format!("{offset}{new_content}");
                    async_fn(async move {
                        let content_buffer = VSBuffer::from_string(&content);

                        let fd = provider.open(&resource, &FileOpenOptions::for_write(false, false)).await?;
                        let result = async {
                            provider.write(fd, 0, &content_buffer, 0, content_buffer.byte_length()).await?;

                            // Here since `close` is not called, all other writes are
                            // waiting on the barrier to release, so doing a readFile
                            // should give us a consistent view of the file contents
                            assert_eq!(read_to_string(fs_path(&resource)), content);
                            Ok(())
                        }
                        .await;
                        provider.close(fd).await?;
                        result
                    })
                })
                .collect(),
        );

        write_promises.await.unwrap();
    });
}

#[test]
fn provider_write_barrier_is_partitioned_per_resource() {
    disk_test(|f| async move {
        let resource1 = f.uri("lorem.txt");
        let resource2 = f.uri("test.txt");

        let provider = provider_of(&f, &resource1);
        assert!(has_open_read_write_close_capability(&*provider));

        let fd1 = provider.open(&resource1, &FileOpenOptions::for_write(false, false)).await.unwrap();
        let fd2 = provider.open(&resource2, &FileOpenOptions::for_write(false, false)).await.unwrap();

        let new_content = VSBuffer::from_string("Hello World");

        let result: Result<(), Error> = async {
            provider.write(fd1, 0, &new_content, 0, new_content.byte_length()).await?;
            assert_eq!(read_to_string(fs_path(&resource1)), "Hello World");

            provider.write(fd2, 0, &new_content, 0, new_content.byte_length()).await?;
            assert_eq!(read_to_string(fs_path(&resource2)), "Hello World");
            Ok(())
        }
        .await;
        let closed1 = provider.close(fd1).await;
        let closed2 = provider.close(fd2).await;
        result.unwrap();
        closed1.unwrap();
        closed2.unwrap();
    });
}

#[test]
fn provider_write_barrier_not_becoming_stale() {
    disk_test(|f| async move {
        let new_folder = f.test_dir.join("new-folder");
        let new_resource = file_uri(new_folder.join("lorem.txt"));

        let provider = provider_of(&f, &new_resource);
        assert!(has_open_read_write_close_capability(&*provider));

        let error = provider.open(&new_resource, &FileOpenOptions::for_write(false, false)).await;

        assert!(error.is_err()); // expected because `new-folder` does not exist

        std::fs::create_dir(&new_folder).unwrap();

        let content = read_to_string(f.test_dir.join("lorem.txt"));
        let new_content = format!("{content}{content}");
        let new_content_buffer = VSBuffer::from_string(&new_content);

        let fd = provider.open(&new_resource, &FileOpenOptions::for_write(false, false)).await.unwrap();
        let result: Result<(), Error> = async {
            provider.write(fd, 0, &new_content_buffer, 0, new_content_buffer.byte_length()).await?;

            assert_eq!(read_to_string(fs_path(&new_resource)), new_content);
            Ok(())
        }
        .await;
        provider.close(fd).await.unwrap();
        result.unwrap();
    });
}

#[test]
fn provider_atomic_reads_write_pending_when_read_starts() {
    disk_test(|f| async move {
        let resource = f.uri("lorem.txt");

        let content = read_to_string(fs_path(&resource));
        let new_content = format!("{content}{content}");
        let new_content_buffer = VSBuffer::from_string(&new_content);

        let provider = provider_of(&f, &resource);
        assert!(has_open_read_write_close_capability(&*provider));
        assert!(has_file_atomic_read_capability(&*provider));

        let fd = provider.open(&resource, &FileOpenOptions::for_write(false, false)).await.unwrap();

        // Start reading while write is pending
        let atomic_read_promise = provider.read_file(&resource, true);

        let result: Result<usize, Error> = async {
            // Simulate a slow write, giving the read
            // a chance to succeed if it were not atomic
            timeout(20).await?;

            provider.write(fd, 0, &new_content_buffer, 0, new_content_buffer.byte_length()).await
        }
        .await;
        provider.close(fd).await.unwrap();
        result.unwrap();

        let atomic_read_result = atomic_read_promise.await.unwrap();
        assert_eq!(atomic_read_result.byte_length(), new_content_buffer.byte_length());
    });
}

#[test]
fn provider_atomic_reads_read_pending_when_write_starts() {
    disk_test(|f| async move {
        let resource = f.uri("lorem.txt");

        let content = read_to_string(fs_path(&resource));
        let new_content = format!("{content}{content}");
        let new_content_buffer = VSBuffer::from_string(&new_content);

        let provider = provider_of(&f, &resource);
        assert!(has_open_read_write_close_capability(&*provider));
        assert!(has_file_atomic_read_capability(&*provider));

        let atomic_read_promise = provider.read_file(&resource, true);

        let fd_promise = {
            let provider = provider.clone();
            let resource = resource.clone();
            let new_content_buffer = new_content_buffer.clone();
            async_fn(async move {
                let fd = provider.open(&resource, &FileOpenOptions::for_write(false, false)).await?;
                let result = provider.write(fd, 0, &new_content_buffer, 0, new_content_buffer.byte_length()).await;
                provider.close(fd).await?;
                result
            })
        };

        let atomic_read_result = atomic_read_promise.await.unwrap();
        assert_eq!(atomic_read_result.byte_length(), content.len());

        fd_promise.await.unwrap();

        let atomic_read_promise = provider.read_file(&resource, true);
        let atomic_read_result = atomic_read_promise.await.unwrap();
        assert_eq!(atomic_read_result.byte_length(), new_content_buffer.byte_length());
    });
}

#[test]
fn write_file_readable_default() {
    disk_test(|f| async move {
        test_write_file_readable(&f).await;
    });
}

#[test]
fn write_file_readable_buffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        test_write_file_readable(&f).await;
    });
}

#[test]
fn write_file_readable_unbuffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE);

        test_write_file_readable(&f).await;
    });
}

async fn test_write_file_readable(f: &Fixture) {
    let resource = f.uri("small.txt");

    let content = read_to_string(fs_path(&resource));
    assert_eq!(content, "Small File");

    let new_content = "Updates to the small file";
    f.service.write_file(&resource, to_line_by_line_readable(new_content), None).await.unwrap();

    assert_eq!(read_to_string(fs_path(&resource)), new_content);
}

#[test]
fn write_file_large_file_readable_default() {
    disk_test(|f| async move {
        test_write_file_large_readable(&f).await;
    });
}

#[test]
fn write_file_large_file_readable_buffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        test_write_file_large_readable(&f).await;
    });
}

#[test]
fn write_file_large_file_readable_unbuffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE);

        test_write_file_large_readable(&f).await;
    });
}

async fn test_write_file_large_readable(f: &Fixture) {
    let resource = f.uri("lorem.txt");

    let content = read_to_string(fs_path(&resource));
    let new_content = format!("{content}{content}");

    let file_stat = f.service.write_file(&resource, to_line_by_line_readable(&new_content), None).await.unwrap();
    assert_eq!(file_stat.name, "lorem.txt");

    assert_eq!(read_to_string(fs_path(&resource)), new_content);
}

#[test]
fn write_file_stream_default() {
    disk_test(|f| async move {
        test_write_file_stream(&f).await;
    });
}

#[test]
fn write_file_stream_buffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        test_write_file_stream(&f).await;
    });
}

#[test]
fn write_file_stream_unbuffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE);

        test_write_file_stream(&f).await;
    });
}

async fn test_write_file_stream(f: &Fixture) {
    let source = f.uri("small.txt");
    let target = f.uri("small-copy.txt");

    let file_stat = f.service.write_file(&target, create_read_stream_as_buffer_stream(PathBuf::from(fs_path(&source))), None).await.unwrap();
    assert_eq!(file_stat.name, "small-copy.txt");

    let target_contents = read_to_string(fs_path(&target));
    assert_eq!(read_to_string(fs_path(&source)), target_contents);
}

#[test]
fn write_file_large_file_stream_default() {
    disk_test(|f| async move {
        test_write_file_large_stream(&f).await;
    });
}

#[test]
fn write_file_large_file_stream_buffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        test_write_file_large_stream(&f).await;
    });
}

#[test]
fn write_file_large_file_stream_unbuffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE);

        test_write_file_large_stream(&f).await;
    });
}

async fn test_write_file_large_stream(f: &Fixture) {
    let source = f.uri("lorem.txt");
    let target = f.uri("lorem-copy.txt");

    let file_stat = f.service.write_file(&target, create_read_stream_as_buffer_stream(PathBuf::from(fs_path(&source))), None).await.unwrap();
    assert_eq!(file_stat.name, "lorem-copy.txt");

    let target_contents = read_to_string(fs_path(&target));
    assert_eq!(read_to_string(fs_path(&source)), target_contents);
}

#[test]
fn write_file_file_is_created_including_parents() {
    disk_test(|f| async move {
        let resource = f.uri("other/newfile.txt");

        let content = "File is created including parent";
        let file_stat = f.service.write_file(&resource, buffer_source(content), None).await.unwrap();
        assert_eq!(file_stat.name, "newfile.txt");

        assert_eq!(read_to_string(fs_path(&resource)), content);
    });
}

#[test]
fn write_file_locked_files_and_unlocking() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE | Cap::FILE_WRITE_UNLOCK);

        test_locked_files(&f, false).await;
    });
}

#[test]
fn write_file_stream_locked_files_and_unlocking() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE | Cap::FILE_WRITE_UNLOCK);

        test_locked_files(&f, false).await;
    });
}

#[test]
fn write_file_locked_files_and_unlocking_throws_error_when_missing_capability() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE);

        test_locked_files(&f, true).await;
    });
}

#[test]
fn write_file_stream_locked_files_and_unlocking_throws_error_when_missing_capability() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        test_locked_files(&f, true).await;
    });
}

async fn test_locked_files(f: &Fixture, expect_error: bool) {
    let locked_file = f.uri("my-locked-file");

    let content = f.service.write_file(&locked_file, buffer_source("Locked File"), None).await.unwrap();
    assert!(!content.locked());

    remove_write_permission(Path::new(&fs_path(&locked_file)));

    let mut stat = f.service.stat(&locked_file).await.unwrap();
    assert!(stat.locked());

    let new_content = "Updates to locked file";
    assert!(f.service.write_file(&locked_file, buffer_source(new_content), None).await.is_err());

    let unlock = Some(WriteFileOptions { unlock: true, ..Default::default() });
    if expect_error {
        assert!(f.service.write_file(&locked_file, buffer_source(new_content), unlock).await.is_err());
    } else {
        f.service.write_file(&locked_file, buffer_source(new_content), unlock).await.unwrap();
        assert_eq!(read_to_string(fs_path(&locked_file)), new_content);

        stat = f.service.stat(&locked_file).await.unwrap();
        assert!(!stat.locked());
    }
}

#[test]
fn write_file_error_when_folder_is_encountered() {
    disk_test(|f| async move {
        let resource = f.test_dir.root_uri();

        assert!(f.service.write_file(&resource, buffer_source("File is created including parent"), None).await.is_err());
    });
}

#[test]
fn write_file_no_error_when_providing_up_to_date_etag() {
    disk_test(|f| async move {
        let resource = f.uri("small.txt");

        let stat = f.service.resolve_with_metadata(&resource, Some(ResolveFileOptions { resolve_metadata: true, ..Default::default() })).await.unwrap();

        let content = read_to_string(fs_path(&resource));
        assert_eq!(content, "Small File");

        let new_content = "Updates to the small file";
        let options = WriteFileOptions { etag: Some(stat.etag().to_string()), mtime: Some(stat.mtime()), ..Default::default() };
        f.service.write_file(&resource, buffer_source(new_content), Some(options)).await.unwrap();

        assert_eq!(read_to_string(fs_path(&resource)), new_content);
    });
}

#[test]
fn write_file_error_when_writing_to_file_that_has_been_updated_meanwhile() {
    disk_test(|f| async move {
        let resource = f.uri("small.txt");

        let stat = f.service.resolve_with_metadata(&resource, Some(ResolveFileOptions { resolve_metadata: true, ..Default::default() })).await.unwrap();

        let content = read_to_string(fs_path(&resource));
        assert_eq!(content, "Small File");

        let new_content = "Updates to the small file";
        let options = WriteFileOptions { etag: Some(stat.etag().to_string()), mtime: Some(stat.mtime()), ..Default::default() };
        f.service.write_file(&resource, buffer_source(new_content), Some(options)).await.unwrap();

        let new_content_leading_to_error = format!("{new_content}{new_content}");

        let fake_mtime = 1000;
        let fake_size = 1000;

        let options = WriteFileOptions { etag: Some(etag(fake_mtime, fake_size)), mtime: Some(fake_mtime), ..Default::default() };
        let error = f.service.write_file(&resource, buffer_source(&new_content_leading_to_error), Some(options)).await.expect_err("era esperado um erro");

        assert_eq!(operation_result(&error), FileOperationResult::FileModifiedSince);
    });
}

#[test]
fn write_file_no_error_when_writing_to_file_where_size_is_the_same() {
    disk_test(|f| async move {
        let resource = f.uri("small.txt");

        let stat = f.service.resolve_with_metadata(&resource, Some(ResolveFileOptions { resolve_metadata: true, ..Default::default() })).await.unwrap();

        let content = read_to_string(fs_path(&resource));
        assert_eq!(content, "Small File");

        let new_content = content.clone(); // same content
        let options = WriteFileOptions { etag: Some(stat.etag().to_string()), mtime: Some(stat.mtime()), ..Default::default() };
        f.service.write_file(&resource, buffer_source(&new_content), Some(options)).await.unwrap();

        let new_content_leading_to_no_error = new_content.clone(); // writing the same content should be OK

        let fake_mtime = 1000;
        let actual_size = new_content.chars().count() as u64;

        let options = WriteFileOptions { etag: Some(etag(fake_mtime, actual_size)), mtime: Some(fake_mtime), ..Default::default() };
        f.service.write_file(&resource, buffer_source(&new_content_leading_to_no_error), Some(options)).await.expect("não era esperado um erro");
    });
}

#[test]
fn write_file_no_error_when_writing_to_file_where_content_is_the_same() {
    disk_test(|f| async move {
        let resource = f.uri("small.txt");

        f.service.resolve(&resource, None).await.unwrap();

        let content = read_to_string(fs_path(&resource));
        assert_eq!(content, "Small File");

        let new_content = content; // same content
        let options = WriteFileOptions { etag: Some("anything".to_string()), mtime: Some(0), ..Default::default() }; /* fake it */
        f.service.write_file(&resource, buffer_source(&new_content), Some(options)).await.expect("não era esperado um erro");
    });
}

#[test]
fn write_file_error_when_writing_to_file_where_content_is_the_same_length_but_different() {
    disk_test(|f| async move {
        let resource = f.uri("small.txt");

        f.service.resolve(&resource, None).await.unwrap();

        let content = read_to_string(fs_path(&resource));
        assert_eq!(content, "Small File");

        let new_content: String = content.chars().rev().collect(); // reverse content
        let options = WriteFileOptions { etag: Some("anything".to_string()), mtime: Some(0), ..Default::default() }; /* fake it */
        let error = f.service.write_file(&resource, buffer_source(&new_content), Some(options)).await.expect_err("era esperado um erro");

        assert_eq!(operation_result(&error), FileOperationResult::FileModifiedSince);
    });
}

#[test]
fn write_file_no_error_when_writing_to_same_nonexistent_folder_multiple_times_different_new_files() {
    disk_test(|f| async move {
        let new_folder = f.uri("some/new/folder");

        let file1 = join_uri(&new_folder, &["file-1"]);
        let file2 = join_uri(&new_folder, &["file-2"]);
        let file3 = join_uri(&new_folder, &["file-3"]);

        // this essentially verifies that the mkdirp logic implemented
        // in the file service is able to receive multiple requests for
        // the same folder and will not throw errors if another racing
        // call succeeded first.
        let new_content = "Updates to the small file";
        Promise::all(vec![
            f.service.write_file(&file1, buffer_source(new_content), None),
            f.service.write_file(&file2, buffer_source(new_content), None),
            f.service.write_file(&file3, buffer_source(new_content), None),
        ])
        .await
        .unwrap();

        assert!(f.service.exists(&file1).await.unwrap());
        assert!(f.service.exists(&file2).await.unwrap());
        assert!(f.service.exists(&file3).await.unwrap());
    });
}

#[test]
fn write_file_error_when_writing_to_folder_that_is_a_file() {
    disk_test(|f| async move {
        let existing_file = f.uri("my-file");

        f.service.create_file(&existing_file, None, None).await.unwrap();

        let new_file = join_uri(&existing_file, &["file-1"]);

        let new_content = "Updates to the small file";
        assert!(f.service.write_file(&new_file, buffer_source(new_content), None).await.is_err());
    });
}

fn append_options() -> Option<WriteFileOptions> {
    Some(WriteFileOptions { append: true, ..Default::default() })
}

#[test]
fn append_file() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE | Cap::FILE_APPEND);

        test_append_file(&f).await;
    });
}

#[test]
fn append_file_buffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE | Cap::FILE_APPEND);

        test_append_file(&f).await;
    });
}

async fn test_append_file(f: &Fixture) {
    let event = f.record_events();

    let resource = f.uri("small.txt");

    let content = read_to_string(fs_path(&resource));
    assert_eq!(content, "Small File");

    let append_content = " - Appended!";
    f.service.write_file(&resource, buffer_source(append_content), append_options()).await.unwrap();

    {
        let event = event.borrow();
        let event = event.last().expect("evento esperado");
        assert_eq!(fs_path(&event.resource), fs_path(&resource));
        assert_eq!(event.operation, FileOperation::Write);
    }

    assert_eq!(read_to_string(fs_path(&resource)), "Small File - Appended!");
}

#[test]
fn append_file_readable() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE | Cap::FILE_APPEND);

        test_append_file_readable(&f).await;
    });
}

#[test]
fn append_file_readable_buffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE | Cap::FILE_APPEND);

        test_append_file_readable(&f).await;
    });
}

async fn test_append_file_readable(f: &Fixture) {
    let resource = f.uri("small.txt");

    let content = read_to_string(fs_path(&resource));
    assert_eq!(content, "Small File");

    let append_content = " - Appended via readable!";
    let readable: Box<dyn Readable<VSBuffer>> = Box::new(buffer_to_readable(VSBuffer::from_string(append_content)));
    f.service.write_file(&resource, FileContentSource::from(readable), append_options()).await.unwrap();

    assert_eq!(read_to_string(fs_path(&resource)), "Small File - Appended via readable!");
}

#[test]
fn append_file_stream() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE | Cap::FILE_APPEND);

        test_append_file_stream(&f).await;
    });
}

#[test]
fn append_file_stream_buffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE | Cap::FILE_APPEND);

        test_append_file_stream(&f).await;
    });
}

async fn test_append_file_stream(f: &Fixture) {
    let resource = f.uri("small.txt");

    let content = read_to_string(fs_path(&resource));
    assert_eq!(content, "Small File");

    let append_content = " - Appended via stream!";
    let stream: VSBufferReadableStream = buffer_to_stream(VSBuffer::from_string(append_content));
    f.service.write_file(&resource, FileContentSource::from(stream), append_options()).await.unwrap();

    assert_eq!(read_to_string(fs_path(&resource)), "Small File - Appended via stream!");
}

#[test]
fn append_file_creates_file_if_not_exists() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE | Cap::FILE_APPEND);

        test_append_file_creates_file(&f).await;
    });
}

#[test]
fn append_file_creates_file_if_not_exists_buffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE | Cap::FILE_APPEND);

        test_append_file_creates_file(&f).await;
    });
}

async fn test_append_file_creates_file(f: &Fixture) {
    let resource = f.uri("appendfile-new.txt");

    assert!(!Path::new(&fs_path(&resource)).exists());

    let content = "Initial content via append";
    f.service.write_file(&resource, buffer_source(content), append_options()).await.unwrap();

    assert!(Path::new(&fs_path(&resource)).exists());
    assert_eq!(read_to_string(fs_path(&resource)), content);
}

#[test]
fn append_file_multiple_appends() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_READ_WRITE | Cap::FILE_APPEND);

        test_append_file_multiple(&f).await;
    });
}

#[test]
fn append_file_multiple_appends_buffered() {
    disk_test(|f| async move {
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE | Cap::FILE_APPEND);

        test_append_file_multiple(&f).await;
    });
}

async fn test_append_file_multiple(f: &Fixture) {
    let resource = f.uri("appendfile-multiple.txt");

    f.service.write_file(&resource, buffer_source("Line 1\n"), append_options()).await.unwrap();
    f.service.write_file(&resource, buffer_source("Line 2\n"), append_options()).await.unwrap();
    f.service.write_file(&resource, buffer_source("Line 3\n"), append_options()).await.unwrap();

    assert_eq!(read_to_string(fs_path(&resource)), "Line 1\nLine 2\nLine 3\n");
}

#[test]
fn append_file_throws_when_provider_does_not_support_append() {
    disk_test(|f| async move {
        // Remove FileAppend capability - should throw error
        f.set_capabilities(Cap::FILE_OPEN_READ_WRITE_CLOSE);

        let resource = f.uri("small.txt");
        let append_content = " - Appended via fallback!";

        let error = f.service.write_file(&resource, buffer_source(append_content), append_options()).await.expect_err("era esperado um erro");

        assert!(error.message.contains("does not support append"), "mensagem: {}", error.message);
    });
}

#[test]
fn read_mixed_positions() {
    disk_test(|f| async move {
        let resource = f.uri("lorem.txt");
        let provider = f.file_provider.as_provider();

        // read multiple times from position 0
        let mut buffer = VSBuffer::alloc(1024);
        let mut fd = provider.open(&resource, &FileOpenOptions::for_read()).await.unwrap();
        for _ in 0..3 {
            provider.read(fd, 0, &buffer, 0, 26).await.unwrap();
            assert_eq!(buffer.slice(Some(0), Some(26)).to_string(), "Lorem ipsum dolor sit amet");
        }
        provider.close(fd).await.unwrap();

        // read multiple times at various locations
        buffer = VSBuffer::alloc(1024);
        fd = provider.open(&resource, &FileOpenOptions::for_read()).await.unwrap();

        let mut pos_in_file = 0;

        provider.read(fd, pos_in_file, &buffer, 0, 26).await.unwrap();
        assert_eq!(buffer.slice(Some(0), Some(26)).to_string(), "Lorem ipsum dolor sit amet");
        pos_in_file += 26;

        provider.read(fd, pos_in_file, &buffer, 0, 1).await.unwrap();
        assert_eq!(buffer.slice(Some(0), Some(1)).to_string(), ",");
        pos_in_file += 1;

        provider.read(fd, pos_in_file, &buffer, 0, 12).await.unwrap();
        assert_eq!(buffer.slice(Some(0), Some(12)).to_string(), " consectetur");
        pos_in_file += 12;

        provider.read(fd, 98 /* no longer in sequence of posInFile */, &buffer, 0, 9).await.unwrap();
        assert_eq!(buffer.slice(Some(0), Some(9)).to_string(), "fermentum");

        provider.read(fd, 27, &buffer, 0, 12).await.unwrap();
        assert_eq!(buffer.slice(Some(0), Some(12)).to_string(), " consectetur");

        provider.read(fd, 26, &buffer, 0, 1).await.unwrap();
        assert_eq!(buffer.slice(Some(0), Some(1)).to_string(), ",");

        provider.read(fd, 0, &buffer, 0, 26).await.unwrap();
        assert_eq!(buffer.slice(Some(0), Some(26)).to_string(), "Lorem ipsum dolor sit amet");

        provider.read(fd, pos_in_file /* back in sequence */, &buffer, 0, 11).await.unwrap();
        assert_eq!(buffer.slice(Some(0), Some(11)).to_string(), " adipiscing");

        provider.close(fd).await.unwrap();
    });
}

#[test]
fn write_mixed_positions() {
    disk_test(|f| async move {
        let resource = f.uri("lorem.txt");
        let provider = f.file_provider.as_provider();

        let buffer = VSBuffer::alloc(1024);
        let fd_write = provider.open(&resource, &FileOpenOptions::for_write(false, false)).await.unwrap();
        let fd_read = provider.open(&resource, &FileOpenOptions::for_read()).await.unwrap();

        let mut pos_in_file_write: u64 = 0;
        let mut pos_in_file_read: u64 = 0;

        let initial_contents = VSBuffer::from_string("Lorem ipsum dolor sit amet");
        provider.write(fd_write, pos_in_file_write, &initial_contents, 0, initial_contents.byte_length()).await.unwrap();
        pos_in_file_write += initial_contents.byte_length() as u64;

        provider.read(fd_read, pos_in_file_read, &buffer, 0, 26).await.unwrap();
        assert_eq!(buffer.slice(Some(0), Some(26)).to_string(), "Lorem ipsum dolor sit amet");
        pos_in_file_read += 26;

        let contents = VSBuffer::from_string("Hello World");
        let contents_length = contents.byte_length();

        provider.write(fd_write, pos_in_file_write, &contents, 0, contents_length).await.unwrap();
        pos_in_file_write += contents_length as u64;

        provider.read(fd_read, pos_in_file_read, &buffer, 0, contents_length).await.unwrap();
        assert_eq!(buffer.slice(Some(0), Some(contents_length as i64)).to_string(), "Hello World");
        let _ = pos_in_file_read + contents_length as u64;

        provider.write(fd_write, 6, &contents, 0, contents_length).await.unwrap();

        provider.read(fd_read, 0, &buffer, 0, 11).await.unwrap();
        assert_eq!(buffer.slice(Some(0), Some(11)).to_string(), "Lorem Hello");

        provider.write(fd_write, pos_in_file_write, &contents, 0, contents_length).await.unwrap();
        pos_in_file_write += contents_length as u64;

        provider.read(fd_read, pos_in_file_write - contents_length as u64, &buffer, 0, contents_length).await.unwrap();
        assert_eq!(buffer.slice(Some(0), Some(contents_length as i64)).to_string(), "Hello World");

        provider.close(fd_write).await.unwrap();
        provider.close(fd_read).await.unwrap();
    });
}

#[test]
fn readonly_is_handled_properly_for_a_single_resource() {
    disk_test(|f| async move {
        f.file_provider.set_readonly(true);

        let resource = f.uri("index.html");

        let resolve_result = f.service.resolve(&resource, None).await.unwrap();
        assert_eq!(resolve_result.readonly, Some(true));

        let read_result = f.service.read_file(&resource, None, None).await.unwrap();
        assert!(read_result.readonly());

        assert!(f.service.write_file(&resource, buffer_source("Hello Test"), None).await.is_err());

        assert!(f.service.del(&resource, None).await.is_err());
    });
}
