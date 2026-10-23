//! Auxiliares dos testes dos watchers de disco (`nodejsWatcher.test.ts` e `parcelWatcher.test.ts`):
//! o diretório de teste com as fixtures, os `awaitEvent` e `Event.toPromise` dos originais, a execução de
//! um corpo `async` no loop real até a condição ou o prazo, e as subclasses de teste dos watchers.
//!
//! Os testes de watcher de disco dependem de tempo real (eventos do sistema de arquivos chegam num thread
//! à parte). Eles não dormem um tempo fixo: o loop de `je_base::runtime` gira até a promessa esperada
//! resolver, e só o prazo (que reprova o teste) é um limite de tempo.
#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::path::ProcessEnvironment;
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::runtime;
use je_platform::files::files::{FileChange, FileChangeType};
use je_platform::files::node_watcher::base_watcher::BaseWatcherOptions;
use je_platform::files::node_watcher::nodejs::NodeJsWatcher;
use je_platform::files::node_watcher::parcel::ParcelWatcher;
use je_platform::files::watcher::{RecursiveWatcherWithSubscribe, WatchRequest, Watcher};

use super::base::{fail_on_unexpected_errors, record_unexpected_errors};

/// O prazo de cada teste (`this.timeout(10000)` dos originais).
pub const TEST_TIMEOUT_MS: u64 = 10_000;

/// O que a subclasse de teste dos originais sobrescreve: `suspendedWatchRequestPollingInterval = 100`,
/// `getUpdateWatchersDelay() => 0` e o `doWatch` que espera o `ready` e dispara `onDidWatch`.
pub fn test_options() -> BaseWatcherOptions {
    BaseWatcherOptions { update_watchers_delay: 0, suspended_poll_interval: 100, join_ready: true }
}

/// O `loggingEnabled` dos originais: com `JE_WATCHER_TEST_LOG` definida, o watcher liga o log detalhado e
/// as mensagens vão para o stderr.
fn enable_logging(watcher: &dyn Watcher, label: &str) {
    if std::env::var_os("JE_WATCHER_TEST_LOG").is_none() {
        return;
    }
    let _ = watcher.set_verbose_logging(true);
    let label = label.to_string();
    std::mem::forget(watcher.on_did_log_message().listen(move |e| eprintln!("[{label} test message] {}", e.message)));
}

/// `new TestParcelWatcher()`.
pub fn test_parcel_watcher() -> Rc<ParcelWatcher> {
    let watcher = ParcelWatcher::with_options(test_options());
    enable_logging(&*watcher, "recursive watcher");
    watcher
}

/// `new TestNodeJSWatcher(accessor)`.
pub fn test_nodejs_watcher(accessor: Option<Rc<dyn RecursiveWatcherWithSubscribe>>) -> Rc<NodeJsWatcher> {
    let watcher = NodeJsWatcher::with_options(accessor, test_options());
    enable_logging(&*watcher, "non-recursive watcher");
    watcher
}

/// Gira o loop até `done` ou o prazo; devolve se `done` valeu. Erros inesperados (um `assert` que falhou
/// dentro do corpo `async`) reprovam na hora.
pub fn run_until(timeout_ms: u64, mut done: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    loop {
        runtime::turn();
        fail_on_unexpected_errors();
        if done() {
            return true;
        }
        if start.elapsed() >= Duration::from_millis(timeout_ms) {
            return false;
        }
        let wait = runtime::time_to_next_timer().unwrap_or(Duration::from_millis(2)).min(Duration::from_millis(2));
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
    }
}

/// Roda o corpo `async` de um teste no loop real e reprova se ele não termina no prazo.
pub fn run_async_test(body: impl Future<Output = ()> + 'static) {
    record_unexpected_errors();
    let done = Rc::new(Cell::new(false));
    let finished = done.clone();
    runtime::spawn_local(async move {
        body.await;
        finished.set(true);
    });
    assert!(run_until(TEST_TIMEOUT_MS, || done.get()), "o teste não terminou em {TEST_TIMEOUT_MS} ms");
}

/// `timeout(ms)` do `async.ts`.
pub fn timeout(ms: u64) -> Promise<()> {
    let cancelable = je_base::async_::timeout(ms);
    (*cancelable).clone()
}

/// `Event.toPromise(event)`.
pub fn to_promise<T: Clone + 'static>(event: &Event<T>) -> Promise<T> {
    (*event.to_promise(None)).clone()
}

/// `await promise` com o erro virando pânico (o `await` de um teste que rejeita reprova).
pub async fn must<T: Clone + 'static>(promise: Promise<T>) -> T {
    promise.await.unwrap_or_else(|error| panic!("a promessa foi rejeitada: {error}"))
}

pub async fn must_all(promises: Vec<Promise<()>>) {
    must(Promise::all(promises)).await;
}

/// O `correlationId` esperado por `awaitEvent`: `null` aceita qualquer um, o resto compara.
#[derive(Clone, Copy, Debug)]
pub enum Cid {
    Any,
    Exactly(Option<i64>),
}

impl From<Option<i64>> for Cid {
    fn from(value: Option<i64>) -> Self {
        Cid::Exactly(value)
    }
}

fn ext_uri() -> ExtUri {
    ExtUri::biased_ignore_path_case(Rc::new(ProcessEnvironment), cfg!(target_os = "linux"))
}

fn same_resource(change: &FileChange, path: &str) -> bool {
    use je_base::path::Environment;
    ext_uri().is_equal(Some(&change.resource), Some(&je_base::uri::URI::file(path, ProcessEnvironment.platform())), false)
}

/// `awaitEvent`: resolve quando chega um evento do `tipo` para `path` (e `cid`), a `expected_count`-ésima vez.
pub fn await_event_on(event: Event<Vec<FileChange>>, path: &str, change_type: FileChangeType, cid: Cid, expected_count: Option<usize>) -> Promise<Vec<FileChange>> {
    await_event_or_fail(event, path, change_type, None, cid, expected_count)
}

/// O `awaitEvent` do `parcelWatcher.test.ts`, com o `failOnEventReason`: se ele vem, o evento rejeita a
/// promessa (`Unexpected file event: <motivo>`) em vez de resolvê-la.
pub fn await_event_or_fail(
    event: Event<Vec<FileChange>>,
    path: &str,
    change_type: FileChangeType,
    fail_on_event_reason: Option<&str>,
    cid: Cid,
    expected_count: Option<usize>,
) -> Promise<Vec<FileChange>> {
    let path = path.to_string();
    let fail_on_event_reason = fail_on_event_reason.map(str::to_string);
    Promise::with_executor(move |resolver| {
        let counter = Cell::new(0usize);
        let subscription: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));
        let slot = subscription.clone();
        let disposable = event.listen(move |events: &Vec<FileChange>| {
            for change in events {
                let cid_matches = match cid {
                    Cid::Any => true,
                    Cid::Exactly(expected) => change.c_id == expected,
                };
                if same_resource(change, &path) && change.change_type == change_type && cid_matches {
                    counter.set(counter.get() + 1);
                    if expected_count.is_some_and(|expected| counter.get() < expected) {
                        continue; // not yet
                    }
                    if let Some(subscription) = slot.borrow_mut().take() {
                        subscription.dispose();
                    }
                    match &fail_on_event_reason {
                        Some(reason) => resolver.reject(Error::new(format!("Unexpected file event: {reason}"))),
                        None => resolver.resolve(events.clone()),
                    }
                    break;
                }
            }
        });
        *subscription.borrow_mut() = Some(disposable);
    })
}

/// A volta do loop que o I/O assíncrono do Node (`fs.promises.*`, que roda no pool de threads) dá antes de
/// completar: as microtarefas pendentes, como o monitor de um pedido suspenso, rodam antes da operação.
pub async fn io() {
    must(timeout(0)).await;
}

/// `Promises.x(...)` sem `await`: a operação roda no loop e ninguém espera por ela.
pub fn detach(operation: impl Future<Output = ()> + 'static) {
    runtime::spawn_local(operation);
}

/// O diretório de teste: `getRandomTestPath(realpathSync(tmpdir()), ...)` com as fixtures de `service`.
pub struct TestDir {
    _tmp: tempfile::TempDir,
    pub path: String,
}

impl TestDir {
    pub fn new() -> Self {
        let tmp = tempfile::Builder::new().prefix("jetests-filewatcher-").tempdir().expect("tempdir");
        // Rule out strange testing conditions by using the realpath here. for example, on macOS the tmp
        // dir is potentially a symlink in some of the root folders, which is a rather unrealisic case for
        // the file watcher.
        let real = std::fs::canonicalize(tmp.path()).expect("realpath do tmp");
        let path = real.to_string_lossy().into_owned();
        create_fixtures(&real);
        Self { _tmp: tmp, path }
    }

    /// `join(testDir, ...)`.
    pub fn join(&self, segments: &[&str]) -> String {
        let mut parts = vec![self.path.as_str()];
        parts.extend_from_slice(segments);
        join(&parts)
    }
}

/// O `join` do módulo `path` da plataforma.
pub fn join(segments: &[&str]) -> String {
    je_base::path::PathModule::native(&ProcessEnvironment).join(segments)
}

/// `dirname`.
pub fn dirname(path: &str) -> String {
    je_base::path::PathModule::native(&ProcessEnvironment).dirname(path)
}

/// Um subconjunto de `fixtures/service` (o que os testes de watcher usam), gerado em vez de copiado:
/// `ref/` fica fora do git e o conteúdo dos arquivos não importa para a observação.
fn create_fixtures(dir: &Path) {
    std::fs::write(dir.join("lorem.txt"), "Lorem ipsum dolor sit amet, consectetur adipiscing elit.\n".repeat(64)).expect("lorem.txt");
    std::fs::write(dir.join("small.txt"), "Small file").expect("small.txt");
    std::fs::write(dir.join("index.html"), "<html><body>fixture</body></html>").expect("index.html");
    std::fs::create_dir(dir.join("deep")).expect("deep");
    for name in ["conway.js", "company.js", "employee.js", "small.js"] {
        std::fs::write(dir.join("deep").join(name), format!("// {name}\n")).expect("fixture js");
    }
}

/// `Promises.copy(source, target, { preserveSymlinks: false })`.
pub async fn copy(source: &str, target: &str) {
    io().await;
    copy_sync(source, target);
}

fn copy_sync(source: &str, target: &str) {
    let source = Path::new(source);
    if source.is_dir() {
        std::fs::create_dir_all(target).expect("copy: mkdir");
        for entry in std::fs::read_dir(source).expect("copy: readdir") {
            let entry = entry.expect("copy: entry");
            copy_sync(&entry.path().to_string_lossy(), &join(&[target, &entry.file_name().to_string_lossy()]));
        }
    } else {
        std::fs::copy(source, target).expect("copy: file");
    }
}

/// `Promises.writeFile(path, data)`.
pub async fn write_file(path: &str, content: &str) {
    io().await;
    std::fs::write(path, content).unwrap_or_else(|e| panic!("writeFile {path}: {e}"));
}

/// `Promises.rm(path, RimRafMode.UNLINK)` (e `Promises.rm(path)`).
pub async fn rm(path: &str) {
    io().await;
    let result = if Path::new(path).is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
    result.unwrap_or_else(|e| panic!("rm {path}: {e}"));
}

/// `fs.promises.mkdir(path)`.
pub async fn mkdir(path: &str) {
    io().await;
    std::fs::create_dir(path).unwrap_or_else(|e| panic!("mkdir {path}: {e}"));
}

/// `fs.promises.unlink(path)`.
pub async fn unlink(path: &str) {
    io().await;
    std::fs::remove_file(path).unwrap_or_else(|e| panic!("unlink {path}: {e}"));
}

/// `fs.promises.rmdir(path)`.
pub async fn rmdir(path: &str) {
    io().await;
    std::fs::remove_dir(path).unwrap_or_else(|e| panic!("rmdir {path}: {e}"));
}

/// `Promises.rename(source, target)`.
pub async fn rename(source: &str, target: &str) {
    io().await;
    std::fs::rename(source, target).unwrap_or_else(|e| panic!("rename {source} -> {target}: {e}"));
}

/// `fs.promises.copyFile(source, target)`.
pub async fn copy_file(source: &str, target: &str) {
    io().await;
    std::fs::copy(source, target).unwrap_or_else(|e| panic!("copyFile {source} -> {target}: {e}"));
}

/// `realpathSync(path)`.
pub fn realpath(path: &str) -> String {
    std::fs::canonicalize(path).unwrap_or_else(|e| panic!("realpath {path}: {e}")).to_string_lossy().into_owned()
}

/// Um pedido `{ path, excludes: [], recursive }`.
pub fn request(path: &str, recursive: bool) -> WatchRequest {
    WatchRequest { path: path.to_string(), recursive, excludes: Vec::new(), includes: None, correlation_id: None, filter: None, polling_interval: None }
}

/// O `Result` de uma promessa que pode ter falhado, sem pânico.
pub fn settled_error(promise: &Promise<()>) -> Option<Error> {
    promise.result().and_then(Result::err)
}

/// Garante que o tipo implementa `Watcher` (o teste chama `stop` pelo trait).
pub fn stop_watcher(watcher: &dyn Watcher) -> Promise<()> {
    watcher.stop()
}

/// `fs.promises.symlink(target, link)`.
pub fn symlink(target: &str, link: &str) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap_or_else(|e| panic!("symlink {link} -> {target}: {e}"));
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(target, link).unwrap_or_else(|e| panic!("symlink {link} -> {target}: {e}"));
}
