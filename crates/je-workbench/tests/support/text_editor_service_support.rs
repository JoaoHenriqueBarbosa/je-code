//! Auxiliares do porte de `textEditorService.test.ts`: o `NullFileSystemProvider` de
//! `src/vs/platform/files/test/common/nullFileSystemProvider.ts` e o `FileServiceProvider` do corpo da suíte, que registra
//! um provider nulo para um esquema e o desfaz ao ser descartado.

use std::cell::Cell;
use std::rc::Rc;

use je_base::buffer::VSBuffer;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, none};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::files::files::{
    FileChange, FileDeleteOptions, FileOpenOptions, FileOverwriteOptions, FileSystemProvider, FileSystemProviderCapabilities, FileType, FileWriteOptions, Stat, WatchOptions,
};
use je_platform::files::i_file_service::IFileService;

/// `NullFileSystemProvider`: todas as operações terminam sem efeito e sem erro.
pub struct NullFileSystemProvider {
    capabilities: Cell<FileSystemProviderCapabilities>,
    on_did_change_capabilities: Rc<Emitter<()>>,
    on_did_change_file: Rc<Emitter<Vec<FileChange>>>,
}

impl NullFileSystemProvider {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { capabilities: Cell::new(FileSystemProviderCapabilities::READONLY), on_did_change_capabilities: Emitter::new(), on_did_change_file: Emitter::new() })
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
        none()
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

/// `FileServiceProvider extends Disposable`: registra um `NullFileSystemProvider` para o esquema e o desfaz ao ser descartado.
pub struct FileServiceProvider {
    base: Rc<Disposable>,
}

impl FileServiceProvider {
    /// `instantiationService.createInstance(FileServiceProvider, scheme)`.
    pub fn new(scheme: &str, file_service: Rc<dyn IFileService>) -> Rc<Self> {
        let base = Disposable::new();
        base.register(file_service.register_provider(scheme, NullFileSystemProvider::new()).expect("o esquema ainda não tem provider"));

        Rc::new(Self { base })
    }
}

impl IDisposable for FileServiceProvider {
    fn dispose(&self) {
        self.base.dispose();
    }
}
