//! Auxiliares dos testes de `vs/platform/files`: o `NullFileSystemProvider` de
//! `test/common/nullFileSystemProvider.ts` e o [`OverridableProvider`], que faz o papel das subclasses
//! anônimas que os testes do original criam (`new class extends InMemoryFileSystemProvider { override
//! async write(...) { ... super.write(...) } }`): cada método sobrescrito é um closure que recebe o
//! provider de baixo (o `super`) e os mesmos argumentos.
#![allow(dead_code, clippy::type_complexity)]

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use je_base::buffer::{VSBuffer, VSBufferReadableStream};
use je_base::cancellation::CancellationToken;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, DisposableStore, none};
use je_base::promise::Promise;
use je_base::path::ProcessEnvironment;
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_platform::files::files::*;

/// `NullFileSystemProvider`.
pub struct NullFileSystemProvider {
    capabilities: Cell<FileSystemProviderCapabilities>,
    on_did_change_capabilities: Rc<Emitter<()>>,
    on_did_change_file: Rc<Emitter<Vec<FileChange>>>,
    disposable_factory: Box<dyn Fn() -> DisposableRef>,
}

impl NullFileSystemProvider {
    pub fn new() -> Rc<Self> {
        Self::with_disposable_factory(Box::new(none))
    }

    pub fn with_disposable_factory(disposable_factory: Box<dyn Fn() -> DisposableRef>) -> Rc<Self> {
        Rc::new(Self {
            capabilities: Cell::new(FileSystemProviderCapabilities::READONLY),
            on_did_change_capabilities: Emitter::new(),
            on_did_change_file: Emitter::new(),
            disposable_factory,
        })
    }

    pub fn emit_file_change_events(&self, changes: Vec<FileChange>) {
        self.on_did_change_file.fire(changes);
    }

    pub fn set_capabilities(&self, capabilities: FileSystemProviderCapabilities) {
        self.capabilities.set(capabilities);
        self.on_did_change_capabilities.fire(());
    }
}

impl FileSystemProvider for NullFileSystemProvider {
    fn capabilities(&self) -> FileSystemProviderCapabilities {
        self.capabilities.get()
    }
    fn on_did_change_capabilities(&self) -> Event<()> {
        self.on_did_change_capabilities.event()
    }
    fn on_did_change_file(&self) -> Event<Vec<FileChange>> {
        self.on_did_change_file.event()
    }
    fn watch(&self, _resource: &URI, _opts: &WatchOptions) -> DisposableRef {
        (self.disposable_factory)()
    }
    /// `async stat() { return undefined!; }`: o stat sem nada, na forma mais próxima.
    fn stat(&self, _resource: &URI) -> Promise<Stat> {
        Promise::resolved(Stat::default())
    }
    fn mkdir(&self, _resource: &URI) -> Promise<()> {
        Promise::resolved(())
    }
    fn readdir(&self, _resource: &URI) -> Promise<Vec<(String, FileType)>> {
        Promise::resolved(Vec::new())
    }
    fn delete(&self, _resource: &URI, _opts: &FileDeleteOptions) -> Promise<()> {
        Promise::resolved(())
    }
    fn rename(&self, _from: &URI, _to: &URI, _opts: &FileOverwriteOptions) -> Promise<()> {
        Promise::resolved(())
    }
    fn copy(&self, _from: &URI, _to: &URI, _opts: &FileOverwriteOptions) -> Promise<()> {
        Promise::resolved(())
    }
    fn read_file(&self, _resource: &URI, _atomic: bool) -> Promise<VSBuffer> {
        Promise::resolved(VSBuffer::alloc(0))
    }
    fn write_file(&self, _resource: &URI, _content: VSBuffer, _opts: &FileWriteOptions) -> Promise<()> {
        Promise::resolved(())
    }
    fn open(&self, _resource: &URI, _opts: &FileOpenOptions) -> Promise<i64> {
        Promise::resolved(0)
    }
    fn close(&self, _fd: i64) -> Promise<()> {
        Promise::resolved(())
    }
    fn read(&self, _fd: i64, _pos: u64, _data: &VSBuffer, _offset: usize, _length: usize) -> Promise<usize> {
        Promise::resolved(0)
    }
    fn write(&self, _fd: i64, _pos: u64, _data: &VSBuffer, _offset: usize, _length: usize) -> Promise<usize> {
        Promise::resolved(0)
    }
}

type Sup<'a> = &'a Rc<dyn FileSystemProvider>;

/// Os closures de sobrescrita; cada um recebe o provider de baixo (`super`) e os argumentos do método.
#[derive(Default)]
pub struct Overrides {
    pub capabilities: Option<Rc<dyn Fn(Sup) -> FileSystemProviderCapabilities>>,
    pub watch: Option<Rc<dyn Fn(Sup, &URI, &WatchOptions) -> DisposableRef>>,
    pub stat: Option<Rc<dyn Fn(Sup, &URI) -> Promise<Stat>>>,
    pub mkdir: Option<Rc<dyn Fn(Sup, &URI) -> Promise<()>>>,
    pub readdir: Option<Rc<dyn Fn(Sup, &URI) -> Promise<Vec<(String, FileType)>>>>,
    pub delete: Option<Rc<dyn Fn(Sup, &URI, &FileDeleteOptions) -> Promise<()>>>,
    pub rename: Option<Rc<dyn Fn(Sup, &URI, &URI, &FileOverwriteOptions) -> Promise<()>>>,
    pub copy: Option<Rc<dyn Fn(Sup, &URI, &URI, &FileOverwriteOptions) -> Promise<()>>>,
    pub read_file: Option<Rc<dyn Fn(Sup, &URI, bool) -> Promise<VSBuffer>>>,
    pub write_file: Option<Rc<dyn Fn(Sup, &URI, VSBuffer, &FileWriteOptions) -> Promise<()>>>,
    pub read_file_stream: Option<Rc<dyn Fn(Sup, &URI, &FileReadStreamOptions, &CancellationToken) -> VSBufferReadableStream>>,
    pub open: Option<Rc<dyn Fn(Sup, &URI, &FileOpenOptions) -> Promise<i64>>>,
    pub close: Option<Rc<dyn Fn(Sup, i64) -> Promise<()>>>,
    pub read: Option<Rc<dyn Fn(Sup, i64, u64, &VSBuffer, usize, usize) -> Promise<usize>>>,
    pub write: Option<Rc<dyn Fn(Sup, i64, u64, &VSBuffer, usize, usize) -> Promise<usize>>>,
    pub clone_file: Option<Rc<dyn Fn(Sup, &URI, &URI) -> Promise<()>>>,
    pub realpath: Option<Rc<dyn Fn(Sup, &URI) -> Promise<String>>>,
    pub enforce_atomic_read_file: Option<Rc<dyn Fn(Sup, &URI) -> bool>>,
    pub enforce_atomic_write_file: Option<Rc<dyn Fn(Sup, &URI) -> Option<FileAtomicOptions>>>,
    pub enforce_atomic_delete: Option<Rc<dyn Fn(Sup, &URI) -> Option<FileAtomicOptions>>>,
}

/// Uma subclasse anônima: delega ao `inner` tudo que não tem closure em [`Overrides`].
pub struct OverridableProvider {
    pub inner: Rc<dyn FileSystemProvider>,
    pub overrides: RefCell<Overrides>,
}

impl OverridableProvider {
    pub fn new(inner: Rc<dyn FileSystemProvider>, overrides: Overrides) -> Rc<Self> {
        Rc::new(Self { inner, overrides: RefCell::new(overrides) })
    }
}

macro_rules! dispatch {
    ($self:ident, $field:ident, ($($arg:expr),*), $default:expr) => {{
        let hook = $self.overrides.borrow().$field.clone();
        match hook {
            Some(hook) => hook(&$self.inner, $($arg),*),
            None => $default,
        }
    }};
}

impl FileSystemProvider for OverridableProvider {
    fn capabilities(&self) -> FileSystemProviderCapabilities {
        dispatch!(self, capabilities, (), self.inner.capabilities())
    }
    fn on_did_change_capabilities(&self) -> Event<()> {
        self.inner.on_did_change_capabilities()
    }
    fn on_did_change_file(&self) -> Event<Vec<FileChange>> {
        self.inner.on_did_change_file()
    }
    fn on_did_watch_error(&self) -> Option<Event<String>> {
        self.inner.on_did_watch_error()
    }
    fn watch(&self, resource: &URI, opts: &WatchOptions) -> DisposableRef {
        dispatch!(self, watch, (resource, opts), self.inner.watch(resource, opts))
    }
    fn stat(&self, resource: &URI) -> Promise<Stat> {
        dispatch!(self, stat, (resource), self.inner.stat(resource))
    }
    fn mkdir(&self, resource: &URI) -> Promise<()> {
        dispatch!(self, mkdir, (resource), self.inner.mkdir(resource))
    }
    fn readdir(&self, resource: &URI) -> Promise<Vec<(String, FileType)>> {
        dispatch!(self, readdir, (resource), self.inner.readdir(resource))
    }
    fn delete(&self, resource: &URI, opts: &FileDeleteOptions) -> Promise<()> {
        dispatch!(self, delete, (resource, opts), self.inner.delete(resource, opts))
    }
    fn rename(&self, from: &URI, to: &URI, opts: &FileOverwriteOptions) -> Promise<()> {
        dispatch!(self, rename, (from, to, opts), self.inner.rename(from, to, opts))
    }
    fn copy(&self, from: &URI, to: &URI, opts: &FileOverwriteOptions) -> Promise<()> {
        dispatch!(self, copy, (from, to, opts), self.inner.copy(from, to, opts))
    }
    fn read_file(&self, resource: &URI, atomic: bool) -> Promise<VSBuffer> {
        dispatch!(self, read_file, (resource, atomic), self.inner.read_file(resource, atomic))
    }
    fn write_file(&self, resource: &URI, content: VSBuffer, opts: &FileWriteOptions) -> Promise<()> {
        dispatch!(self, write_file, (resource, content.clone(), opts), self.inner.write_file(resource, content, opts))
    }
    fn read_file_stream(&self, resource: &URI, opts: &FileReadStreamOptions, token: &CancellationToken) -> VSBufferReadableStream {
        dispatch!(self, read_file_stream, (resource, opts, token), self.inner.read_file_stream(resource, opts, token))
    }
    fn open(&self, resource: &URI, opts: &FileOpenOptions) -> Promise<i64> {
        dispatch!(self, open, (resource, opts), self.inner.open(resource, opts))
    }
    fn close(&self, fd: i64) -> Promise<()> {
        dispatch!(self, close, (fd), self.inner.close(fd))
    }
    fn read(&self, fd: i64, pos: u64, data: &VSBuffer, offset: usize, length: usize) -> Promise<usize> {
        dispatch!(self, read, (fd, pos, data, offset, length), self.inner.read(fd, pos, data, offset, length))
    }
    fn write(&self, fd: i64, pos: u64, data: &VSBuffer, offset: usize, length: usize) -> Promise<usize> {
        dispatch!(self, write, (fd, pos, data, offset, length), self.inner.write(fd, pos, data, offset, length))
    }
    fn clone_file(&self, from: &URI, to: &URI) -> Promise<()> {
        dispatch!(self, clone_file, (from, to), self.inner.clone_file(from, to))
    }
    fn realpath(&self, resource: &URI) -> Promise<String> {
        dispatch!(self, realpath, (resource), self.inner.realpath(resource))
    }
    fn enforce_atomic_read_file(&self, resource: &URI) -> bool {
        dispatch!(self, enforce_atomic_read_file, (resource), self.inner.enforce_atomic_read_file(resource))
    }
    fn enforce_atomic_write_file(&self, resource: &URI) -> Option<FileAtomicOptions> {
        dispatch!(self, enforce_atomic_write_file, (resource), self.inner.enforce_atomic_write_file(resource))
    }
    fn enforce_atomic_delete(&self, resource: &URI) -> Option<FileAtomicOptions> {
        dispatch!(self, enforce_atomic_delete, (resource), self.inner.enforce_atomic_delete(resource))
    }
    fn read_only_message(&self) -> Option<String> {
        self.inner.read_only_message()
    }
}

/// `btoa(text)` para texto ASCII (o título do teste).
fn btoa(text: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in text.as_bytes().chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `toResource.call(this, path)` do `utils.ts`: `URI.file(join('/', btoa(this.test.fullTitle()), path))`.
pub fn to_resource(full_title: &str, path: &str) -> URI {
    super::uri_file(&super::join(&["/", &btoa(full_title), path]))
}

/// `joinPath(resource, ...pathFragments)` de `resources.ts` (o `extUri` sem viés).
pub fn join_path(resource: &URI, path_fragments: &[&str]) -> URI {
    ExtUri::unbiased(Rc::new(ProcessEnvironment)).join_path(resource, path_fragments)
}

/// Um teste `async` da suíte: o `ensureNoDisposablesAreLeakedInTestSuite` e o `runWithFakedTimers`, com o
/// store do `disposables` entregue ao corpo.
pub fn async_test<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<DisposableStore>) -> F) {
    let guard = super::ensure_no_disposables_are_leaked_in_test_suite();
    super::run_with_faked_timers(body(guard.store()));
}
