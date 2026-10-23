//! Porte de `src/vs/platform/files/common/inMemoryFilesystemProvider.ts`: um provider de arquivos que
//! guarda a árvore na memória, com leitura e escrita inteiras, abertura com handle e append.
//!
//! Avaliado contra o `vfs` do pseudo-linus (`docs/reuse-pseudo-linus.md`): o `vfs` tem a semântica de
//! erro do Linux (errno, permissão, sticky, ELOOP), enquanto este provider tem a do VS Code (`EntryNotFound`,
//! `EntryExists`, o `mtime` por escrita, o evento agrupado em 5 ms). Os testes do oráculo medem a segunda,
//! então este é um porte direto. O `vfs` fica para o alvo web como provider alternativo (tmpfs com
//! snapshot), sem tocar neste contrato.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::buffer::{VSBuffer, new_writeable_buffer_stream};
use je_base::cancellation::CancellationToken;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable, none};
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::runtime::{TimerId, clear_timeout, date_now, set_timeout};
use je_base::stream::WriteableStream;
use je_base::uri::URI;
use je_base::path::ProcessEnvironment;

use super::files::*;

type EntryRef = Rc<RefCell<Entry>>;

struct File {
    ctime: i64,
    mtime: i64,
    size: u64,
    name: String,
    data: Option<Vec<u8>>,
}

struct Directory {
    ctime: i64,
    mtime: i64,
    size: i64,
    name: String,
    /// `Map<string, File | Directory>`: a ordem de inserção é a ordem do `readdir`.
    entries: Vec<(String, EntryRef)>,
}

enum Entry {
    File(File),
    Directory(Directory),
}

impl Entry {
    fn new_file(name: &str) -> EntryRef {
        let now = now_ms();
        Rc::new(RefCell::new(Entry::File(File { ctime: now, mtime: now, size: 0, name: name.to_string(), data: None })))
    }

    fn new_directory(name: &str) -> EntryRef {
        let now = now_ms();
        Rc::new(RefCell::new(Entry::Directory(Directory { ctime: now, mtime: now, size: 0, name: name.to_string(), entries: Vec::new() })))
    }

    fn name(&self) -> String {
        match self {
            Entry::File(f) => f.name.clone(),
            Entry::Directory(d) => d.name.clone(),
        }
    }

    fn file_type(&self) -> FileType {
        match self {
            Entry::File(_) => FileType::FILE,
            Entry::Directory(_) => FileType::DIRECTORY,
        }
    }

    fn stat(&self) -> Stat {
        match self {
            Entry::File(f) => Stat { file_type: FileType::FILE, ctime: f.ctime, mtime: f.mtime, size: f.size, permissions: None },
            Entry::Directory(d) => Stat { file_type: FileType::DIRECTORY, ctime: d.ctime, mtime: d.mtime, size: d.size.max(0) as u64, permissions: None },
        }
    }
}

fn now_ms() -> i64 {
    i64::try_from(date_now()).unwrap_or(i64::MAX)
}

fn provider_error(message: &str, code: FileSystemProviderErrorCode) -> je_base::errors::Error {
    create_file_system_provider_error_from_text(message, code)
}

struct OpenFile {
    file: EntryRef,
    resource: URI,
    append: bool,
    write: bool,
}

/// `Map.prototype.set` em cima de um vetor de pares: mantém a posição se a chave já existe.
fn map_set(entries: &mut Vec<(String, EntryRef)>, name: &str, entry: EntryRef) {
    match entries.iter_mut().find(|(n, _)| n == name) {
        Some(slot) => slot.1 = entry,
        None => entries.push((name.to_string(), entry)),
    }
}

fn map_delete(entries: &mut Vec<(String, EntryRef)>, name: &str) -> bool {
    let before = entries.len();
    entries.retain(|(n, _)| n != name);
    entries.len() != before
}

fn map_get(entries: &[(String, EntryRef)], name: &str) -> Option<EntryRef> {
    entries.iter().find(|(n, _)| n == name).map(|(_, e)| e.clone())
}

/// `InMemoryFileSystemProvider`.
pub struct InMemoryFileSystemProvider {
    this: Weak<InMemoryFileSystemProvider>,
    ext_uri: ExtUri,
    fd_counter: Cell<i64>,
    fd_memory: RefCell<Vec<(i64, OpenFile)>>,
    capabilities: Cell<FileSystemProviderCapabilities>,
    on_did_change_capabilities: Rc<Emitter<()>>,
    on_did_change_file: Rc<Emitter<Vec<FileChange>>>,
    root: EntryRef,
    buffered_changes: RefCell<Vec<FileChange>>,
    fire_soon_handle: Cell<Option<TimerId>>,
}

impl InMemoryFileSystemProvider {
    pub fn new() -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            ext_uri: ExtUri::unbiased(Rc::new(ProcessEnvironment)),
            fd_counter: Cell::new(0),
            fd_memory: RefCell::new(Vec::new()),
            capabilities: Cell::new(
                FileSystemProviderCapabilities::FILE_READ_WRITE
                    | FileSystemProviderCapabilities::FILE_OPEN_READ_WRITE_CLOSE
                    | FileSystemProviderCapabilities::FILE_APPEND
                    | FileSystemProviderCapabilities::PATH_CASE_SENSITIVE,
            ),
            on_did_change_capabilities: Emitter::new(),
            on_did_change_file: Emitter::new(),
            root: Entry::new_directory(""),
            buffered_changes: RefCell::new(Vec::new()),
            fire_soon_handle: Cell::new(None),
        })
    }

    /// `setReadOnly(readonly)`.
    pub fn set_read_only(&self, readonly: bool) {
        let is_readonly = self.capabilities.get().intersects(FileSystemProviderCapabilities::READONLY);
        if readonly != is_readonly {
            self.capabilities.set(
                FileSystemProviderCapabilities::FILE_READ_WRITE
                    | FileSystemProviderCapabilities::FILE_APPEND
                    | FileSystemProviderCapabilities::PATH_CASE_SENSITIVE
                    | if readonly { FileSystemProviderCapabilities::READONLY } else { FileSystemProviderCapabilities::NONE },
            );
            self.on_did_change_capabilities.fire(());
        }
    }

    fn basename(&self, resource: &URI) -> String {
        self.ext_uri.basename(resource, None)
    }

    fn dirname(&self, resource: &URI) -> URI {
        self.ext_uri.dirname(resource)
    }

    // --- lookup

    fn lookup(&self, uri: &URI) -> Result<Option<EntryRef>, je_base::errors::Error> {
        self.lookup_inner(uri, false)
    }

    fn lookup_silent(&self, uri: &URI) -> Option<EntryRef> {
        self.lookup_inner(uri, true).ok().flatten()
    }

    fn lookup_inner(&self, uri: &URI, silent: bool) -> Result<Option<EntryRef>, je_base::errors::Error> {
        let path = uri.path().to_string_lossy();
        let mut entry = self.root.clone();
        for part in path.split('/') {
            if part.is_empty() {
                continue;
            }
            let child = match &*entry.borrow() {
                Entry::Directory(directory) => map_get(&directory.entries, part),
                Entry::File(_) => None,
            };
            match child {
                Some(child) => entry = child,
                None if silent => return Ok(None),
                None => return Err(provider_error("file not found", FileSystemProviderErrorCode::FileNotFound)),
            }
        }
        Ok(Some(entry))
    }

    fn lookup_entry(&self, uri: &URI) -> Result<EntryRef, je_base::errors::Error> {
        self.lookup(uri).map(|entry| entry.expect("lookup não silencioso sempre devolve a entrada"))
    }

    fn lookup_as_directory(&self, uri: &URI) -> Result<EntryRef, je_base::errors::Error> {
        let entry = self.lookup_entry(uri)?;
        if matches!(&*entry.borrow(), Entry::Directory(_)) {
            return Ok(entry);
        }
        Err(provider_error("file not a directory", FileSystemProviderErrorCode::FileNotADirectory))
    }

    fn lookup_as_file(&self, uri: &URI) -> Result<EntryRef, je_base::errors::Error> {
        let entry = self.lookup_entry(uri)?;
        if matches!(&*entry.borrow(), Entry::File(_)) {
            return Ok(entry);
        }
        Err(provider_error("file is a directory", FileSystemProviderErrorCode::FileIsADirectory))
    }

    fn lookup_parent_directory(&self, uri: &URI) -> Result<EntryRef, je_base::errors::Error> {
        self.lookup_as_directory(&self.dirname(uri))
    }

    // --- eventos

    fn fire_soon(&self, changes: Vec<FileChange>) {
        self.buffered_changes.borrow_mut().extend(changes);
        if let Some(handle) = self.fire_soon_handle.take() {
            clear_timeout(handle);
        }
        let this = self.this.clone();
        self.fire_soon_handle.set(Some(set_timeout(
            move || {
                if let Some(this) = this.upgrade() {
                    let changes = std::mem::take(&mut *this.buffered_changes.borrow_mut());
                    this.on_did_change_file.fire(changes);
                }
            },
            5,
        )));
    }

    fn file_data(&self, resource: &URI) -> Result<Option<Vec<u8>>, je_base::errors::Error> {
        let entry = self.lookup_as_file(resource)?;
        let data = match &*entry.borrow() {
            Entry::File(file) => file.data.clone(),
            Entry::Directory(_) => None,
        };
        Ok(data)
    }

    fn with_fd<R>(&self, fd: i64, f: impl FnOnce(&mut OpenFile) -> R) -> Result<R, je_base::errors::Error> {
        let mut memory = self.fd_memory.borrow_mut();
        match memory.iter_mut().find(|(id, _)| *id == fd) {
            Some((_, open)) => Ok(f(open)),
            None => Err(provider_error("No file with that descriptor open", FileSystemProviderErrorCode::Unavailable)),
        }
    }

    fn do_write_file(&self, resource: &URI, content: &VSBuffer, opts: &FileWriteOptions) -> Result<(), je_base::errors::Error> {
        let basename = self.basename(resource);
        let parent = self.lookup_parent_directory(resource)?;
        let mut entry = match &*parent.borrow() {
            Entry::Directory(directory) => map_get(&directory.entries, &basename),
            Entry::File(_) => None,
        };
        if entry.as_ref().is_some_and(|e| matches!(&*e.borrow(), Entry::Directory(_))) {
            return Err(provider_error("file is directory", FileSystemProviderErrorCode::FileIsADirectory));
        }
        if entry.is_none() && !opts.create {
            return Err(provider_error("file not found", FileSystemProviderErrorCode::FileNotFound));
        }
        if entry.is_some() && opts.create && !opts.overwrite {
            return Err(provider_error("file exists already", FileSystemProviderErrorCode::FileExists));
        }
        if entry.is_none() {
            let created = Entry::new_file(&basename);
            if let Entry::Directory(directory) = &mut *parent.borrow_mut() {
                map_set(&mut directory.entries, &basename, created.clone());
            }
            entry = Some(created);
            self.fire_soon(vec![FileChange::new(FileChangeType::Added, resource.clone())]);
        }
        if let Some(entry) = entry
            && let Entry::File(file) = &mut *entry.borrow_mut()
        {
            file.mtime = now_ms();
            let incoming = content.to_vec();
            if opts.append {
                file.size += incoming.len() as u64;
                let mut data = file.data.take().unwrap_or_default();
                data.extend_from_slice(&incoming);
                file.data = Some(data);
            } else {
                file.size = incoming.len() as u64;
                file.data = Some(incoming);
            }
        }
        self.fire_soon(vec![FileChange::new(FileChangeType::Updated, resource.clone())]);
        Ok(())
    }

    fn do_open(&self, resource: &URI, opts: &FileOpenOptions) -> Result<i64, je_base::errors::Error> {
        let mut file = self.lookup_silent(resource);
        let write = opts.is_for_write();
        let append = write && opts.append;
        match &file {
            None => {
                if !write {
                    return Err(provider_error("file not found", FileSystemProviderErrorCode::FileNotFound));
                }
                let basename = self.basename(resource);
                let parent = self.lookup_parent_directory(resource)?;
                let created = Entry::new_file(&basename);
                if let Entry::File(f) = &mut *created.borrow_mut() {
                    f.data = Some(Vec::new());
                }
                if let Entry::Directory(directory) = &mut *parent.borrow_mut() {
                    map_set(&mut directory.entries, &basename, created.clone());
                }
                file = Some(created);
                self.fire_soon(vec![FileChange::new(FileChangeType::Added, resource.clone())]);
            }
            Some(existing) if matches!(&*existing.borrow(), Entry::Directory(_)) => {
                return Err(provider_error("file is directory", FileSystemProviderErrorCode::FileIsADirectory));
            }
            Some(_) => {}
        }
        let file = file.expect("a entrada existe a esta altura");
        if let Entry::File(f) = &mut *file.borrow_mut()
            && f.data.is_none()
        {
            f.data = Some(Vec::new());
        }
        let fd = self.fd_counter.get();
        self.fd_counter.set(fd + 1);
        self.fd_memory.borrow_mut().push((fd, OpenFile { file, resource: resource.clone(), write, append }));
        Ok(fd)
    }
}

impl FileSystemProvider for InMemoryFileSystemProvider {
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

    fn stat(&self, resource: &URI) -> Promise<Stat> {
        match self.lookup_entry(resource) {
            Ok(entry) => Promise::resolved(entry.borrow().stat()),
            Err(error) => Promise::rejected(error),
        }
    }

    fn readdir(&self, resource: &URI) -> Promise<Vec<(String, FileType)>> {
        match self.lookup_as_directory(resource) {
            Ok(entry) => {
                let result = match &*entry.borrow() {
                    Entry::Directory(directory) => directory.entries.iter().map(|(name, child)| (name.clone(), child.borrow().file_type())).collect(),
                    Entry::File(_) => Vec::new(),
                };
                Promise::resolved(result)
            }
            Err(error) => Promise::rejected(error),
        }
    }

    fn read_file(&self, resource: &URI, _atomic: bool) -> Promise<VSBuffer> {
        match self.file_data(resource) {
            Ok(Some(data)) => Promise::resolved(VSBuffer::wrap(data)),
            Ok(None) => Promise::rejected(provider_error("file not found", FileSystemProviderErrorCode::FileNotFound)),
            Err(error) => Promise::rejected(error),
        }
    }

    fn read_file_stream(&self, resource: &URI, _opts: &FileReadStreamOptions, _token: &CancellationToken) -> je_base::buffer::VSBufferReadableStream {
        let stream = new_writeable_buffer_stream(None);
        match self.file_data(resource) {
            Ok(data) => stream.end(data.map(VSBuffer::wrap)),
            Err(error) => {
                stream.error(error);
                stream.end(None);
            }
        }
        stream
    }

    fn write_file(&self, resource: &URI, content: VSBuffer, opts: &FileWriteOptions) -> Promise<()> {
        match self.do_write_file(resource, &content, opts) {
            Ok(()) => Promise::resolved(()),
            Err(error) => Promise::rejected(error),
        }
    }

    fn open(&self, resource: &URI, opts: &FileOpenOptions) -> Promise<i64> {
        match self.do_open(resource, opts) {
            Ok(fd) => Promise::resolved(fd),
            Err(error) => Promise::rejected(error),
        }
    }

    fn close(&self, fd: i64) -> Promise<()> {
        let removed = {
            let mut memory = self.fd_memory.borrow_mut();
            memory.iter().position(|(id, _)| *id == fd).map(|index| memory.remove(index).1)
        };
        if let Some(open) = removed
            && open.write
        {
            if let Entry::File(file) = &mut *open.file.borrow_mut() {
                file.mtime = now_ms();
                file.size = file.data.as_ref().map_or(0, |d| d.len() as u64);
            }
            self.fire_soon(vec![FileChange::new(FileChangeType::Updated, open.resource)]);
        }
        Promise::resolved(())
    }

    fn read(&self, fd: i64, pos: u64, data: &VSBuffer, offset: usize, length: usize) -> Promise<usize> {
        let result = self.with_fd(fd, |open| match &*open.file.borrow() {
            Entry::File(file) => match &file.data {
                None => 0,
                Some(bytes) => {
                    let start = usize::try_from(pos).unwrap_or(usize::MAX).min(bytes.len());
                    let end = start.saturating_add(length).min(bytes.len());
                    data.set_bytes(&bytes[start..end], offset);
                    end - start
                }
            },
            Entry::Directory(_) => 0,
        });
        match result {
            Ok(read) => Promise::resolved(read),
            Err(error) => Promise::rejected(error),
        }
    }

    fn write(&self, fd: i64, pos: u64, data: &VSBuffer, offset: usize, length: usize) -> Promise<usize> {
        let result = self.with_fd(fd, |open| {
            let to_write = data.slice(Some(offset as i64), Some((offset + length) as i64)).to_vec();
            if let Entry::File(file) = &mut *open.file.borrow_mut() {
                let bytes = file.data.get_or_insert_with(Vec::new);
                let write_pos = if open.append { bytes.len() } else { usize::try_from(pos).unwrap_or(usize::MAX) };
                let end_pos = write_pos + to_write.len();
                if end_pos > bytes.len() {
                    bytes.resize(end_pos, 0);
                }
                bytes[write_pos..end_pos].copy_from_slice(&to_write);
            }
            to_write.len()
        });
        match result {
            Ok(written) => Promise::resolved(written),
            Err(error) => Promise::rejected(error),
        }
    }

    fn rename(&self, from: &URI, to: &URI, opts: &FileOverwriteOptions) -> Promise<()> {
        let result = (|| {
            if !opts.overwrite && self.lookup_silent(to).is_some() {
                return Err(provider_error("file exists already", FileSystemProviderErrorCode::FileExists));
            }
            let entry = self.lookup_entry(from)?;
            let old_parent = self.lookup_parent_directory(from)?;
            let new_parent = self.lookup_parent_directory(to)?;
            let new_name = self.basename(to);
            let old_name = entry.borrow().name();
            if let Entry::Directory(directory) = &mut *old_parent.borrow_mut() {
                map_delete(&mut directory.entries, &old_name);
            }
            match &mut *entry.borrow_mut() {
                Entry::File(file) => file.name = new_name.clone(),
                Entry::Directory(directory) => directory.name = new_name.clone(),
            }
            if let Entry::Directory(directory) = &mut *new_parent.borrow_mut() {
                map_set(&mut directory.entries, &new_name, entry.clone());
            }
            self.fire_soon(vec![FileChange::new(FileChangeType::Deleted, from.clone()), FileChange::new(FileChangeType::Added, to.clone())]);
            Ok(())
        })();
        match result {
            Ok(()) => Promise::resolved(()),
            Err(error) => Promise::rejected(error),
        }
    }

    fn delete(&self, resource: &URI, _opts: &FileDeleteOptions) -> Promise<()> {
        let result = (|| {
            let dirname = self.dirname(resource);
            let basename = self.basename(resource);
            let parent = self.lookup_as_directory(&dirname)?;
            let deleted = match &mut *parent.borrow_mut() {
                Entry::Directory(directory) => {
                    let deleted = map_delete(&mut directory.entries, &basename);
                    if deleted {
                        directory.mtime = now_ms();
                        directory.size -= 1;
                    }
                    deleted
                }
                Entry::File(_) => false,
            };
            if deleted {
                self.fire_soon(vec![FileChange::new(FileChangeType::Updated, dirname), FileChange::new(FileChangeType::Deleted, resource.clone())]);
            }
            Ok(())
        })();
        match result {
            Ok(()) => Promise::resolved(()),
            Err(error) => Promise::rejected(error),
        }
    }

    fn mkdir(&self, resource: &URI) -> Promise<()> {
        let result = (|| {
            if self.lookup_silent(resource).is_some() {
                return Err(provider_error("file exists already", FileSystemProviderErrorCode::FileExists));
            }
            let basename = self.basename(resource);
            let dirname = self.dirname(resource);
            let parent = self.lookup_as_directory(&dirname)?;
            let entry = Entry::new_directory(&basename);
            if let Entry::Directory(directory) = &mut *parent.borrow_mut() {
                map_set(&mut directory.entries, &basename, entry);
                directory.mtime = now_ms();
                directory.size += 1;
            }
            self.fire_soon(vec![FileChange::new(FileChangeType::Updated, dirname), FileChange::new(FileChangeType::Added, resource.clone())]);
            Ok(())
        })();
        match result {
            Ok(()) => Promise::resolved(()),
            Err(error) => Promise::rejected(error),
        }
    }
}

impl IDisposable for InMemoryFileSystemProvider {
    fn dispose(&self) {
        self.on_did_change_capabilities.dispose();
        self.on_did_change_file.dispose();
        self.fd_memory.borrow_mut().clear();
    }
}
