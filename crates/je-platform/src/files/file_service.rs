//! Porte de `src/vs/platform/files/common/fileService.ts`: o `FileService`, que registra providers por
//! scheme e implementa resolver, ler, escrever, mover, copiar, apagar e observar por cima deles.
//!
//! As funções `async` do original são `async_fn` sobre o event loop do `je_base`: o corpo roda já até o
//! primeiro `await` e as continuações são microtarefas, como no JavaScript.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::async_::{ResourceQueue, async_fn, promises};
use je_base::buffer::{VSBuffer, VSBufferReadableStream, buffer_to_readable, buffered_stream_to_buffer, new_writeable_buffer_stream, readable_to_buffer, stream_to_buffer};
use je_base::runtime::set_timeout;
use je_base::stream::{
    IStreamListener, PeekedReadable, Readable, ReadableBufferedStream, StreamListener, Transformer, WriteableStream, consume_stream_ignoring_data,
    listen_stream, new_writeable_stream, peek_readable, peek_stream, transform,
};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, dispose_all, to_disposable};
use je_base::nls::localize;
use je_base::path::{Environment, ProcessEnvironment};
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::ternary_search_tree::TernarySearchTree;
use je_base::uri::{Field, URI, UriChange};

use super::files::*;
use super::io::{CreateReadStreamOptions, read_file_into_stream};

/// O que `writeFile` aceita: `VSBuffer | VSBufferReadable | VSBufferReadableStream`, mais o stream já
/// espiado (`VSBufferReadableBufferedStream`), que só existe por dentro do serviço.
pub enum FileContentSource {
    Buffer(VSBuffer),
    Readable(Box<dyn Readable<VSBuffer>>),
    Stream(VSBufferReadableStream),
    BufferedStream(ReadableBufferedStream<VSBuffer>),
}

impl From<VSBuffer> for FileContentSource {
    fn from(buffer: VSBuffer) -> Self {
        Self::Buffer(buffer)
    }
}

impl From<&str> for FileContentSource {
    fn from(text: &str) -> Self {
        Self::Buffer(VSBuffer::from_string(text))
    }
}

impl From<Box<dyn Readable<VSBuffer>>> for FileContentSource {
    fn from(readable: Box<dyn Readable<VSBuffer>>) -> Self {
        Self::Readable(readable)
    }
}

impl From<VSBufferReadableStream> for FileContentSource {
    fn from(stream: VSBufferReadableStream) -> Self {
        Self::Stream(stream)
    }
}

/// `IFileSystemWatcher`.
pub struct FileSystemWatcher {
    disposables: Rc<DisposableStore>,
    on_did_change: Rc<Emitter<Rc<FileChangesEvent>>>,
}

impl FileSystemWatcher {
    pub fn on_did_change(&self) -> Event<Rc<FileChangesEvent>> {
        self.on_did_change.event()
    }
}

impl IDisposable for FileSystemWatcher {
    fn dispose(&self) {
        self.disposables.dispose();
    }
}

/// Um `IStat | { type } & Partial<IStat>`: o que `toFileStat` aceita.
#[derive(Clone, Copy, Default)]
struct PartialStat {
    file_type: FileType,
    mtime: Option<i64>,
    ctime: Option<i64>,
    size: Option<u64>,
    permissions: Option<FilePermission>,
}

impl From<Stat> for PartialStat {
    fn from(stat: Stat) -> Self {
        Self { file_type: stat.file_type, mtime: Some(stat.mtime), ctime: Some(stat.ctime), size: Some(stat.size), permissions: stat.permissions }
    }
}

type RecurseFn = Rc<dyn Fn(&FileStat, Option<usize>) -> bool>;

struct ActiveWatcher {
    disposable: DisposableRef,
    count: usize,
}

thread_local! {
    static WATCHER_CORRELATION_IDS: Cell<i64> = const { Cell::new(0) };
}

const BUFFER_SIZE: usize = 256 * 1024;

/// `FileService`.
pub struct FileService {
    log_service: super::LogServiceRef,
    disposables: Rc<DisposableStore>,
    env: Rc<dyn Environment>,
    ext_uri: ExtUri,
    ext_uri_ignore_path_case: ExtUri,
    providers: RefCell<Vec<(String, FileSystemProviderRef)>>,
    on_did_change_registrations: Rc<Emitter<FileSystemProviderRegistrationEvent>>,
    on_will_activate: Rc<Emitter<FileSystemProviderActivationEvent>>,
    on_did_change_capabilities: Rc<Emitter<FileSystemProviderCapabilitiesChangeEvent>>,
    on_did_run_operation: Rc<Emitter<FileOperationEvent>>,
    internal_on_did_files_change: Rc<Emitter<Rc<FileChangesEvent>>>,
    on_did_uncorrelated_files_change: Rc<Emitter<Rc<FileChangesEvent>>>,
    on_did_watch_error: Rc<Emitter<Error>>,
    active_watchers: RefCell<HashMap<String, ActiveWatcher>>,
    write_queue: ResourceQueue,
}

impl FileService {
    pub fn new(log_service: super::LogServiceRef) -> Rc<Self> {
        let env: Rc<dyn Environment> = Rc::new(ProcessEnvironment);
        let disposables = DisposableStore::new();
        let ext_uri = ExtUri::unbiased(env.clone());
        Rc::new(Self {
            log_service,
            ext_uri: ext_uri.clone(),
            ext_uri_ignore_path_case: ExtUri::ignore_path_case(env.clone()),
            env,
            providers: RefCell::new(Vec::new()),
            on_did_change_registrations: disposables.add_rc(Emitter::new()),
            on_will_activate: disposables.add_rc(Emitter::new()),
            on_did_change_capabilities: disposables.add_rc(Emitter::new()),
            on_did_run_operation: disposables.add_rc(Emitter::new()),
            internal_on_did_files_change: disposables.add_rc(Emitter::new()),
            on_did_uncorrelated_files_change: disposables.add_rc(Emitter::new()),
            on_did_watch_error: disposables.add_rc(Emitter::new()),
            active_watchers: RefCell::new(HashMap::new()),
            write_queue: ResourceQueue::new(ext_uri),
            disposables,
        })
    }

    // region: eventos

    pub fn on_did_change_file_system_provider_registrations(&self) -> Event<FileSystemProviderRegistrationEvent> {
        self.on_did_change_registrations.event()
    }

    pub fn on_will_activate_file_system_provider(&self) -> Event<FileSystemProviderActivationEvent> {
        self.on_will_activate.event()
    }

    pub fn on_did_change_file_system_provider_capabilities(&self) -> Event<FileSystemProviderCapabilitiesChangeEvent> {
        self.on_did_change_capabilities.event()
    }

    pub fn on_did_run_operation(&self) -> Event<FileOperationEvent> {
        self.on_did_run_operation.event()
    }

    /// O `onDidFilesChange` global: pula os eventos correlacionados.
    pub fn on_did_files_change(&self) -> Event<Rc<FileChangesEvent>> {
        self.on_did_uncorrelated_files_change.event()
    }

    pub fn on_did_watch_error(&self) -> Event<Error> {
        self.on_did_watch_error.event()
    }

    // endregion

    // region: providers

    /// `registerProvider(scheme, provider)`: falha se o scheme já tem provider.
    pub fn register_provider(self: &Rc<Self>, scheme: &str, provider: FileSystemProviderRef) -> Result<DisposableRef, Error> {
        if self.providers.borrow().iter().any(|(s, _)| s == scheme) {
            return Err(Error::new(format!("A filesystem provider for the scheme '{scheme}' is already registered.")));
        }
        let provider_disposables = DisposableStore::new();
        self.providers.borrow_mut().push((scheme.to_string(), provider.clone()));
        self.on_did_change_registrations.fire(FileSystemProviderRegistrationEvent { added: true, scheme: scheme.to_string(), provider: Some(provider.clone()) });

        let this = Rc::downgrade(self);
        let changed_provider = provider.clone();
        provider_disposables.add(provider.on_did_change_file().listen(move |changes| {
            let Some(this) = this.upgrade() else { return };
            let event = Rc::new(FileChangesEvent::new(changes, !this.is_path_case_sensitive(changed_provider.as_ref())));
            this.internal_on_did_files_change.fire(event.clone());
            if !event.has_correlation() {
                this.on_did_uncorrelated_files_change.fire(event);
            }
        }));
        if let Some(on_error) = provider.on_did_watch_error() {
            let this = Rc::downgrade(self);
            provider_disposables.add(on_error.listen(move |error| {
                if let Some(this) = this.upgrade() {
                    this.on_did_watch_error.fire(Error::new(error.clone()));
                }
            }));
        }
        let this = Rc::downgrade(self);
        let (changed_provider, changed_scheme) = (provider.clone(), scheme.to_string());
        provider_disposables.add(provider.on_did_change_capabilities().listen(move |()| {
            if let Some(this) = this.upgrade() {
                this.on_did_change_capabilities.fire(FileSystemProviderCapabilitiesChangeEvent { provider: changed_provider.clone(), scheme: changed_scheme.clone() });
            }
        }));

        let this = self.clone();
        let scheme = scheme.to_string();
        Ok(to_disposable(move || {
            this.on_did_change_registrations.fire(FileSystemProviderRegistrationEvent { added: false, scheme: scheme.clone(), provider: Some(provider.clone()) });
            this.providers.borrow_mut().retain(|(s, _)| *s != scheme);
            provider_disposables.dispose();
        }))
    }

    pub fn get_provider(&self, scheme: &str) -> Option<FileSystemProviderRef> {
        self.providers.borrow().iter().find(|(s, _)| s == scheme).map(|(_, p)| p.clone())
    }

    /// `activateProvider(scheme)`: dá aos ouvintes a chance de registrar o provider e espera por eles.
    pub fn activate_provider(self: &Rc<Self>, scheme: &str) -> Promise<()> {
        let this = self.clone();
        let scheme = scheme.to_string();
        async_fn(async move {
            let event = FileSystemProviderActivationEvent::new(scheme.clone());
            this.on_will_activate.fire(event.clone());
            if this.get_provider(&scheme).is_some() {
                return Ok(()); // o provider já está aqui, então voltamos direto
            }
            promises::settled(event.take_joiners()).await?;
            Ok(())
        })
    }

    pub fn can_handle_resource(self: &Rc<Self>, resource: &URI) -> Promise<bool> {
        let (this, resource) = (self.clone(), resource.clone());
        async_fn(async move {
            this.activate_provider(&resource.scheme().to_string_lossy()).await?;
            Ok(this.has_provider(&resource))
        })
    }

    pub fn has_provider(&self, resource: &URI) -> bool {
        self.get_provider(&resource.scheme().to_string_lossy()).is_some()
    }

    pub fn has_capability(&self, resource: &URI, capability: FileSystemProviderCapabilities) -> bool {
        self.get_provider(&resource.scheme().to_string_lossy()).is_some_and(|p| p.capabilities().intersects(capability))
    }

    pub fn list_capabilities(&self) -> Vec<(String, FileSystemProviderCapabilities)> {
        self.providers.borrow().iter().map(|(scheme, provider)| (scheme.clone(), provider.capabilities())).collect()
    }

    async fn with_provider(self: &Rc<Self>, resource: &URI) -> Result<FileSystemProviderRef, Error> {
        if !self.ext_uri.is_absolute_path(resource) {
            return Err(FileOperationError::new(
                localize("invalidPath", "Unable to resolve filesystem provider with relative file path '{0}'", &[&self.resource_for_error(resource)]),
                FileOperationResult::FileInvalidPath,
            )
            .into());
        }
        self.activate_provider(&resource.scheme().to_string_lossy()).await?;
        match self.get_provider(&resource.scheme().to_string_lossy()) {
            Some(provider) => Ok(provider),
            None => Err(Error::named(
                "CodeExpectedError",
                localize("noProviderFound", "ENOPRO: No file system provider found for resource '{0}'", &[&resource.to_string(false).to_string_lossy()]),
            )),
        }
    }

    async fn with_read_provider(self: &Rc<Self>, resource: &URI) -> Result<FileSystemProviderRef, Error> {
        let provider = self.with_provider(resource).await?;
        if has_open_read_write_close_capability(provider.as_ref()) || has_read_write_capability(provider.as_ref()) || has_file_read_stream_capability(provider.as_ref()) {
            return Ok(provider);
        }
        Err(Error::new(format!(
            "Filesystem provider for scheme '{}' neither has FileReadWrite, FileReadStream nor FileOpenReadWriteClose capability which is needed for the read operation.",
            resource.scheme().to_string_lossy()
        )))
    }

    async fn with_write_provider(self: &Rc<Self>, resource: &URI) -> Result<FileSystemProviderRef, Error> {
        let provider = self.with_provider(resource).await?;
        if has_open_read_write_close_capability(provider.as_ref()) || has_read_write_capability(provider.as_ref()) {
            return Ok(provider);
        }
        Err(Error::new(format!(
            "Filesystem provider for scheme '{}' neither has FileReadWrite nor FileOpenReadWriteClose capability which is needed for the write operation.",
            resource.scheme().to_string_lossy()
        )))
    }

    // endregion

    // region: resolver

    /// `resolve(resource, options)`.
    pub fn resolve(self: &Rc<Self>, resource: &URI, options: Option<ResolveFileOptions>) -> Promise<FileStat> {
        let (this, resource) = (self.clone(), resource.clone());
        async_fn(async move {
            match this.do_resolve_file(&resource, options.as_ref()).await {
                Ok(stat) => Ok(stat),
                Err(error) => {
                    if to_file_system_provider_error_code(Some(&error)) == FileSystemProviderErrorCode::FileNotFound {
                        return Err(FileOperationError::new(
                            localize("fileNotFoundError", "Unable to resolve nonexistent file '{0}'", &[&this.resource_for_error(&resource)]),
                            FileOperationResult::FileNotFound,
                        )
                        .into());
                    }
                    Err(error)
                }
            }
        })
    }

    /// `resolve(resource, { ...options, resolveMetadata: true })`.
    pub fn resolve_with_metadata(self: &Rc<Self>, resource: &URI, options: Option<ResolveFileOptions>) -> Promise<FileStatWithMetadata> {
        let options = ResolveFileOptions { resolve_metadata: true, ..options.unwrap_or_default() };
        self.resolve(resource, Some(options)).then(FileStatWithMetadata::new)
    }

    async fn do_resolve_file(self: &Rc<Self>, resource: &URI, options: Option<&ResolveFileOptions>) -> Result<FileStat, Error> {
        let provider = self.with_provider(resource).await?;
        let is_path_case_sensitive = self.is_path_case_sensitive(provider.as_ref());
        let resolve_to = options.map(|o| o.resolve_to.clone()).unwrap_or_default();
        let resolve_single_child_descendants = options.is_some_and(|o| o.resolve_single_child_descendants);
        let resolve_metadata = options.is_some_and(|o| o.resolve_metadata);

        let stat = provider.stat(resource).await?;

        let trie: Rc<RefCell<Option<TernarySearchTree<URI, bool>>>> = Rc::default();
        let root = resource.clone();
        let recurse: RecurseFn = Rc::new(move |stat: &FileStat, siblings: Option<usize>| {
            let mut trie = trie.borrow_mut();
            let trie = trie.get_or_insert_with(|| {
                // Convert the paths from options.resolveTo to absolute paths
                let mut tree = TernarySearchTree::for_uris(Rc::new(move |_| !is_path_case_sensitive), Rc::new(|_| false));
                tree.set(root.clone(), true);
                tree.fill_keys(true, &resolve_to);
                tree
            });
            if trie.get(&stat.resource).is_some()
                || trie.find_superstr(&stat.resource.with(&UriChange { query: Field::Clear, fragment: Field::Clear, ..UriChange::default() })).is_some()
            {
                return true;
            }
            if stat.is_directory && resolve_single_child_descendants {
                return siblings == Some(1);
            }
            false
        });
        self.to_file_stat(&provider, resource, stat.into(), None, resolve_metadata, recurse).await
    }

    fn to_file_stat(
        self: &Rc<Self>,
        provider: &FileSystemProviderRef,
        resource: &URI,
        stat: PartialStat,
        siblings: Option<usize>,
        resolve_metadata: bool,
        recurse: RecurseFn,
    ) -> Promise<FileStat> {
        let (this, provider, resource) = (self.clone(), provider.clone(), resource.clone());
        async_fn(async move {
            let provider_ext_uri = this.get_ext_uri(provider.as_ref()).0;
            let permissions = stat.permissions.unwrap_or(FilePermission::NONE);
            let mut file_stat = FileStat {
                resource: resource.clone(),
                name: provider_ext_uri.basename(&resource, None),
                is_file: stat.file_type.intersects(FileType::FILE),
                is_directory: stat.file_type.intersects(FileType::DIRECTORY),
                is_symbolic_link: stat.file_type.intersects(FileType::SYMBOLIC_LINK),
                mtime: stat.mtime,
                ctime: stat.ctime,
                size: stat.size,
                readonly: Some(permissions.intersects(FilePermission::READONLY) || provider.capabilities().intersects(FileSystemProviderCapabilities::READONLY)),
                locked: Some(permissions.intersects(FilePermission::LOCKED)),
                executable: Some(permissions.intersects(FilePermission::EXECUTABLE)),
                etag: stat.mtime.zip(stat.size).map(|(mtime, size)| etag(mtime, size)),
                children: None,
            };

            // check if recursing for directories
            if file_stat.is_directory && recurse(&file_stat, siblings) {
                match provider.readdir(&resource).await {
                    Ok(entries) => {
                        let count = entries.len();
                        let children = entries.into_iter().map(|(name, entry_type)| {
                            let (this, provider, recurse) = (this.clone(), provider.clone(), recurse.clone());
                            let (provider_ext_uri, resource) = (provider_ext_uri.clone(), resource.clone());
                            async_fn(async move {
                                let result: Result<FileStat, Error> = async {
                                    let child_resource = provider_ext_uri.join_path(&resource, &[&name]);
                                    let child_stat = if resolve_metadata {
                                        PartialStat::from(provider.stat(&child_resource).await?)
                                    } else {
                                        PartialStat { file_type: entry_type, ..PartialStat::default() }
                                    };
                                    this.to_file_stat(&provider, &child_resource, child_stat, Some(count), resolve_metadata, recurse).await
                                }
                                .await;
                                match result {
                                    Ok(stat) => Ok(Some(stat)),
                                    Err(error) => {
                                        this.log_service.trace(&error.to_string());
                                        Ok(None) // can happen e.g. due to permission errors
                                    }
                                }
                            })
                        });
                        let resolved = promises::settled(children.collect()).await?;
                        file_stat.children = Some(resolved.into_iter().flatten().collect());
                    }
                    Err(error) => {
                        this.log_service.trace(&error.to_string());
                        file_stat.children = Some(Vec::new()); // gracefully handle errors, we may not have permissions to read
                    }
                }
            }
            Ok(file_stat)
        })
    }

    /// `resolveAll(toResolve)`.
    pub fn resolve_all(self: &Rc<Self>, to_resolve: Vec<(URI, Option<ResolveFileOptions>)>) -> Promise<Vec<FileStatResult>> {
        let this = self.clone();
        let entries = to_resolve.into_iter().map(|(resource, options)| {
            let this = this.clone();
            async_fn(async move {
                match this.do_resolve_file(&resource, options.as_ref()).await {
                    Ok(stat) => Ok(FileStatResult { stat: Some(stat), success: true }),
                    Err(error) => {
                        // Do not log when the file is missing: that is expected and not an error.
                        if to_file_operation_result(&error) != FileOperationResult::FileNotFound {
                            this.log_service.trace(&error.to_string());
                        }
                        Ok(FileStatResult { stat: None, success: false })
                    }
                }
            })
        });
        promises::settled(entries.collect())
    }

    /// `stat(resource)`: sem resolver filhos.
    pub fn stat(self: &Rc<Self>, resource: &URI) -> Promise<FileStatWithMetadata> {
        let (this, resource) = (self.clone(), resource.clone());
        async_fn(async move {
            let provider = this.with_provider(&resource).await?;
            let stat = provider.stat(&resource).await?;
            let file_stat = this.to_file_stat(&provider, &resource, stat.into(), None, true, Rc::new(|_, _| false)).await?;
            Ok(FileStatWithMetadata::new(file_stat))
        })
    }

    pub fn realpath(self: &Rc<Self>, resource: &URI) -> Promise<Option<URI>> {
        let (this, resource) = (self.clone(), resource.clone());
        async_fn(async move {
            let provider = this.with_provider(&resource).await?;
            if has_file_realpath_capability(provider.as_ref()) {
                let realpath = provider.realpath(&resource).await?;
                return Ok(Some(with_path(&resource, &realpath)));
            }
            Ok(None)
        })
    }

    pub fn exists(self: &Rc<Self>, resource: &URI) -> Promise<bool> {
        let (this, resource) = (self.clone(), resource.clone());
        async_fn(async move {
            let provider = this.with_provider(&resource).await?;
            Ok(provider.stat(&resource).await.is_ok())
        })
    }

    // endregion

    // region: criar e escrever

    /// `canCreateFile(resource, options)`: `Ok(())` é o `true` do original, `Err` é o erro devolvido.
    pub fn can_create_file(self: &Rc<Self>, resource: &URI, options: Option<CreateFileOptions>) -> Promise<Result<(), Error>> {
        let (this, resource) = (self.clone(), resource.clone());
        async_fn(async move { Ok(this.do_validate_create_file(&resource, options).await) })
    }

    async fn do_validate_create_file(self: &Rc<Self>, resource: &URI, options: Option<CreateFileOptions>) -> Result<(), Error> {
        // validate overwrite
        if !options.is_some_and(|o| o.overwrite) && self.exists(resource).await? {
            return Err(FileOperationError::new(
                localize("fileExists", "Unable to create file '{0}' that already exists when overwrite flag is not set", &[&self.resource_for_error(resource)]),
                FileOperationResult::FileModifiedSince,
            )
            .into());
        }
        Ok(())
    }

    /// `createFile(resource, bufferOrReadableOrStream = '', options)`.
    pub fn create_file(self: &Rc<Self>, resource: &URI, content: Option<FileContentSource>, options: Option<CreateFileOptions>) -> Promise<FileStatWithMetadata> {
        let (this, resource) = (self.clone(), resource.clone());
        async_fn(async move {
            // validate
            this.do_validate_create_file(&resource, options).await?;
            // do write into file (this will create it too)
            let source = content.unwrap_or_else(|| FileContentSource::Buffer(VSBuffer::from_string("")));
            let file_stat = this.write_file(&resource, source, None).await?;
            // events
            this.on_did_run_operation.fire(FileOperationEvent::new(resource, FileOperation::Create, Some(file_stat.clone())));
            Ok(file_stat)
        })
    }

    /// `writeFile(resource, bufferOrReadableOrStream, options)`.
    pub fn write_file(self: &Rc<Self>, resource: &URI, content: FileContentSource, options: Option<WriteFileOptions>) -> Promise<FileStatWithMetadata> {
        let (this, resource) = (self.clone(), resource.clone());
        async_fn(async move {
            let provider = this.throw_if_file_system_is_readonly(this.with_write_provider(&resource).await?, &resource)?;
            let provider_ext_uri = this.get_ext_uri(provider.as_ref()).0;

            let mut write_file_options = options;
            // enforce atomic write if the provider requires it
            if has_file_atomic_write_capability(provider.as_ref()) && !write_file_options.as_ref().is_some_and(|o| o.atomic.is_some())
                && let Some(enforced) = provider.enforce_atomic_write_file(&resource)
            {
                write_file_options = Some(WriteFileOptions { atomic: Some(enforced), ..write_file_options.unwrap_or_default() });
            }

            let result: Result<(), Error> = async {
                // validate write (this may already return a peeked-at buffer)
                let (stat, mut source, peeked) = this.validate_write_file(&provider, &resource, content, write_file_options.as_ref()).await?;

                // mkdir recursively as needed
                if stat.is_none() {
                    this.mkdirp(&provider, &provider_ext_uri.dirname(&resource)).await?;
                }

                // optimization: if the provider has unbuffered write capability and the data to write is not a buffer, we peek
                if !peeked {
                    source = this.peek_buffer_for_writing(&provider, source).await?;
                }

                let atomic = write_file_options.as_ref().is_some_and(|o| o.atomic.is_some());
                if !has_open_read_write_close_capability(provider.as_ref()) // buffered writing is unsupported
                    || (has_read_write_capability(provider.as_ref()) && matches!(source, FileContentSource::Buffer(_))) // data is a full buffer already
                    || (has_read_write_capability(provider.as_ref()) && has_file_atomic_write_capability(provider.as_ref()) && atomic)
                // atomic write forces unbuffered write if the provider supports it
                {
                    this.do_write_unbuffered(&provider, &resource, write_file_options.as_ref(), source).await?;
                } else {
                    let source = match source {
                        FileContentSource::Buffer(buffer) => FileContentSource::Readable(Box::new(buffer_to_readable(buffer))),
                        other => other,
                    };
                    this.do_write_buffered(&provider, &resource, write_file_options.as_ref(), source).await?;
                }

                this.on_did_run_operation.fire(FileOperationEvent::new(resource.clone(), FileOperation::Write, None));
                Ok(())
            }
            .await;
            if let Err(error) = result {
                return Err(FileOperationError::new(
                    localize("err.write", "Unable to write file '{0}' ({1})", &[&this.resource_for_error(&resource), &error.to_string()]),
                    to_file_operation_result(&error),
                )
                .into());
            }

            this.resolve_with_metadata(&resource, None).await
        })
    }

    async fn peek_buffer_for_writing(self: &Rc<Self>, provider: &FileSystemProviderRef, source: FileContentSource) -> Result<FileContentSource, Error> {
        // Using unbuffered writes requires reading everything into memory, so peek a few chunks and
        // fall back to buffered writing if the content is too big.
        if has_read_write_capability(provider.as_ref()) && !matches!(source, FileContentSource::Buffer(_)) {
            return match source {
                FileContentSource::Stream(stream) => {
                    let buffered = peek_stream(stream, 3).await?;
                    if buffered.ended {
                        Ok(FileContentSource::Buffer(VSBuffer::concat(&buffered.buffer, None)))
                    } else {
                        Ok(FileContentSource::BufferedStream(buffered))
                    }
                }
                FileContentSource::Readable(readable) => {
                    match peek_readable(readable, |data| VSBuffer::concat(&data, None), 3) {
                        PeekedReadable::Value(buffer) => Ok(FileContentSource::Buffer(buffer)),
                        PeekedReadable::Readable(readable) => Ok(FileContentSource::Readable(readable)),
                    }
                }
                other => Ok(other),
            };
        }
        Ok(source)
    }

    async fn validate_write_file(
        self: &Rc<Self>,
        provider: &FileSystemProviderRef,
        resource: &URI,
        source: FileContentSource,
        options: Option<&WriteFileOptions>,
    ) -> Result<(Option<Stat>, FileContentSource, bool), Error> {
        // Validate unlock support
        let unlock = options.is_some_and(|o| o.unlock);
        if unlock && !provider.capabilities().intersects(FileSystemProviderCapabilities::FILE_WRITE_UNLOCK) {
            return Err(Error::new(localize("writeFailedUnlockUnsupported", "Unable to unlock file '{0}' because provider does not support it.", &[&self.resource_for_error(resource)])));
        }

        // Validate append support
        if options.is_some_and(|o| o.append) && !has_file_append_capability(provider.as_ref()) {
            return Err(FileOperationError::new(
                localize("err.noAppend", "Filesystem provider for scheme '{0}' does not does not support append", &[&self.resource_for_error(resource)]),
                FileOperationResult::FilePermissionDenied,
            )
            .into());
        }

        // Validate atomic support
        let atomic = options.is_some_and(|o| o.atomic.is_some());
        if atomic {
            if !provider.capabilities().intersects(FileSystemProviderCapabilities::FILE_ATOMIC_WRITE) {
                return Err(Error::new(localize("writeFailedAtomicUnsupported1", "Unable to atomically write file '{0}' because provider does not support it.", &[&self.resource_for_error(resource)])));
            }
            if !provider.capabilities().intersects(FileSystemProviderCapabilities::FILE_READ_WRITE) {
                return Err(Error::new(localize(
                    "writeFailedAtomicUnsupported2",
                    "Unable to atomically write file '{0}' because provider does not support unbuffered writes.",
                    &[&self.resource_for_error(resource)],
                )));
            }
            if unlock {
                return Err(Error::new(localize("writeFailedAtomicUnlock", "Unable to unlock file '{0}' because atomic write is enabled.", &[&self.resource_for_error(resource)])));
            }
        }

        // Validate via file stat meta data
        let Ok(stat) = provider.stat(resource).await else {
            return Ok((None, source, false)); // file might not exist
        };

        // File cannot be directory
        if stat.file_type.intersects(FileType::DIRECTORY) {
            return Err(FileOperationError::new(
                localize("fileIsDirectoryWriteError", "Unable to write file '{0}' that is actually a directory", &[&self.resource_for_error(resource)]),
                FileOperationResult::FileIsDirectory,
            )
            .into());
        }

        // File cannot be readonly
        self.throw_if_file_is_readonly(resource, &stat)?;

        // Dirty write prevention: if the file on disk has been changed and does not match our expected
        // mtime and etag, we bail out to prevent dirty writing.
        if let (Some(mtime), Some(options_etag)) = (options.and_then(|o| o.mtime), options.and_then(|o| o.etag.as_deref()))
            && options_etag != ETAG_DISABLED
            && mtime < stat.mtime
            && options_etag != etag(mtime /* not using stat.mtime for a reason, see above */, stat.size)
        {
            let source = self.peek_buffer_for_writing(provider, source).await?;

            // Allow writing if the contents are identical: the file on disk changed but not its content.
            if let FileContentSource::Buffer(buffer) = &source
                && buffer.byte_length() as u64 == stat.size
                && let Ok(content) = self.read_file(resource, Some(ReadFileOptions { stream: FileReadStreamOptions { limit_size: Some(stat.size), ..Default::default() }, ..Default::default() }), None).await
                && buffer.equals(&content.value)
            {
                return Ok((Some(stat), source, true)); // allow writing since contents are identical
            }

            return Err(FileOperationError::new(localize("fileModifiedError", "File Modified Since", &[]), FileOperationResult::FileModifiedSince).into());
        }

        Ok((Some(stat), source, false))
    }

    // endregion

    // region: ler

    /// `readFile(resource, options, token)`.
    pub fn read_file(self: &Rc<Self>, resource: &URI, options: Option<ReadFileOptions>, token: Option<CancellationToken>) -> Promise<FileContent> {
        let (this, resource) = (self.clone(), resource.clone());
        async_fn(async move {
            let provider = this.with_read_provider(&resource).await?;
            if options.as_ref().is_some_and(|o| o.atomic) {
                return this.do_read_file_atomic(&provider, &resource, options, token).await;
            }
            this.do_read_file(&provider, &resource, options, token).await
        })
    }

    async fn do_read_file_atomic(self: &Rc<Self>, provider: &FileSystemProviderRef, resource: &URI, options: Option<ReadFileOptions>, token: Option<CancellationToken>) -> Result<FileContent, Error> {
        let (promise, resolver) = Promise::<FileContent>::new();
        let (this, provider, queued_resource) = (self.clone(), provider.clone(), resource.clone());
        let provider_ext_uri = self.get_ext_uri(provider.as_ref()).0;
        let _ = self.write_queue.queue_for_with(
            resource,
            move || {
                async_fn(async move {
                    match this.do_read_file(&provider, &queued_resource, options, token).await {
                        Ok(content) => resolver.resolve(content),
                        Err(error) => resolver.reject(error),
                    }
                    Ok(())
                })
            },
            &provider_ext_uri,
        );
        promise.await
    }

    async fn do_read_file(self: &Rc<Self>, provider: &FileSystemProviderRef, resource: &URI, options: Option<ReadFileOptions>, token: Option<CancellationToken>) -> Result<FileContent, Error> {
        let stream = self.do_read_file_stream(provider, resource, options, token, true).await?;
        let value = stream_to_buffer(&*stream.value).await?;
        Ok(FileContent { stat: stream.stat, value })
    }

    /// `readFileStream(resource, options, token)`.
    pub fn read_file_stream(self: &Rc<Self>, resource: &URI, options: Option<ReadFileOptions>, token: Option<CancellationToken>) -> Promise<FileStreamContent> {
        let (this, resource) = (self.clone(), resource.clone());
        async_fn(async move {
            let provider = this.with_read_provider(&resource).await?;
            this.do_read_file_stream(&provider, &resource, options, token, false).await
        })
    }

    async fn do_read_file_stream(
        self: &Rc<Self>,
        provider: &FileSystemProviderRef,
        resource: &URI,
        options: Option<ReadFileOptions>,
        token: Option<CancellationToken>,
        prefer_unbuffered: bool,
    ) -> Result<FileStreamContent, Error> {
        // install a cancellation token that gets cancelled when any error occurs, so that the read is aborted
        let cancellable_source = CancellationTokenSource::new(token.as_ref());

        // enforce atomic read if the provider requires it
        let mut read_file_options = options.unwrap_or_default();
        if has_file_atomic_read_capability(provider.as_ref()) && provider.enforce_atomic_read_file(resource) {
            read_file_options.atomic = true;
        }

        // validate read operation
        let stat_promise: Promise<FileStatWithMetadata> = {
            let (this, resource, read_file_options, source) = (self.clone(), resource.clone(), read_file_options.clone(), cancellable_source.clone());
            async_fn(async move {
                this.validate_read_file(&resource, &read_file_options).await.inspect_err(|_| source.dispose_with(true))
            })
        };

        let mut file_stream: Option<VSBufferReadableStream> = None;
        let outcome: Result<FileStreamContent, Error> = async {
            // if the etag is provided, we await the result of the validation due to the likelihood of hitting a NOT_MODIFIED_SINCE result
            if read_file_options.etag.as_deref().is_some_and(|e| e != ETAG_DISABLED) {
                stat_promise.clone().await?;
            }

            let stream = if (read_file_options.atomic && has_file_atomic_read_capability(provider.as_ref())) // atomic reads are always unbuffered
                || !(has_open_read_write_close_capability(provider.as_ref()) || has_file_read_stream_capability(provider.as_ref())) // provider has no buffered capability
                || (has_read_write_capability(provider.as_ref()) && prefer_unbuffered)
            // unbuffered read is preferred
            {
                self.read_file_unbuffered(provider, resource, &read_file_options)
            } else if has_file_read_stream_capability(provider.as_ref()) {
                self.read_file_streamed(provider, resource, &cancellable_source.token(), &read_file_options)
            } else {
                self.read_file_buffered(provider, resource, &cancellable_source.token(), &read_file_options)
            };
            file_stream = Some(stream.clone());

            // Release the cancellation source when the stream is done.
            let on_end = cancellable_source.clone();
            stream.on(StreamListener::end(move || on_end.dispose_with(false)));
            let on_error = cancellable_source.clone();
            stream.on(StreamListener::error(move |_| on_error.dispose_with(false)));

            let file_stat = stat_promise.await?;
            Ok(FileStreamContent { stat: file_stat, value: stream })
        }
        .await;
        match outcome {
            Ok(content) => Ok(content),
            Err(error) => {
                // Await the stat promise settled: it may already have disposed the source.
                cancellable_source.dispose_with(true);
                // Await the stream so that it does not leak the producer's resources.
                if let Some(stream) = file_stream {
                    consume_stream_ignoring_data(&*stream).await?;
                }
                Err(restore_read_error(&self.resource_for_error(resource), &error))
            }
        }
    }

    fn read_file_streamed(&self, provider: &FileSystemProviderRef, resource: &URI, token: &CancellationToken, options: &ReadFileOptions) -> VSBufferReadableStream {
        let file_stream = provider.read_file_stream(resource, &options.stream, token);
        let restore = self.error_restorer(resource);
        let transformer = Transformer { data: Rc::new(|data: &VSBuffer| data.clone()), error: Some(Rc::new(move |error: Error| restore(&error))) };
        transform(&*file_stream, transformer, Rc::new(|data: Vec<VSBuffer>| VSBuffer::concat(&data, None)))
    }

    fn error_restorer(&self, resource: &URI) -> Rc<dyn Fn(&Error) -> Error> {
        let resource_text = self.resource_for_error(resource);
        Rc::new(move |error| restore_read_error(&resource_text, error))
    }

    fn read_file_buffered(&self, provider: &FileSystemProviderRef, resource: &URI, token: &CancellationToken, options: &ReadFileOptions) -> VSBufferReadableStream {
        let stream = new_writeable_buffer_stream(None);
        let restore = self.error_restorer(resource);
        let _ = read_file_into_stream(
            provider.clone(),
            resource.clone(),
            stream.clone(),
            CreateReadStreamOptions { stream: options.stream, buffer_size: BUFFER_SIZE, error_transformer: Some(Rc::new(move |error| restore(error))) },
            token.clone(),
        );
        stream
    }

    fn read_file_unbuffered(&self, provider: &FileSystemProviderRef, resource: &URI, options: &ReadFileOptions) -> VSBufferReadableStream {
        let stream = new_writeable_stream(Some(Rc::new(|data: Vec<VSBuffer>| VSBuffer::concat(&data, None))), None);
        let (target, provider, resource, options) = (stream.clone(), provider.clone(), resource.clone(), options.clone());
        let this_resource_text = self.resource_for_error(&resource);
        let _ = async_fn(async move {
            let result: Result<VSBuffer, Error> = async {
                let mut buffer = if options.atomic && has_file_atomic_read_capability(provider.as_ref()) {
                    provider.read_file(&resource, true).await?
                } else {
                    provider.read_file(&resource, false).await?
                };

                // respect position option
                if let Some(position) = options.stream.position {
                    buffer = buffer.slice(Some(i64::try_from(position).unwrap_or(i64::MAX)), None);
                }
                // respect length option
                if let Some(length) = options.stream.length {
                    buffer = buffer.slice(Some(0), Some(i64::try_from(length).unwrap_or(i64::MAX)));
                }

                // Throw if file is too large to load
                validate_read_file_limits(&this_resource_text, buffer.byte_length() as u64, &options)?;
                Ok(buffer)
            }
            .await;
            match result {
                Ok(buffer) => target.end(Some(buffer)),
                Err(error) => {
                    target.error(error);
                    target.end(None);
                }
            }
            Ok(())
        });
        stream
    }

    async fn validate_read_file(self: &Rc<Self>, resource: &URI, options: &ReadFileOptions) -> Result<FileStatWithMetadata, Error> {
        let stat = self.resolve_with_metadata(resource, None).await?;

        // Throw if resource is a directory
        if stat.is_directory {
            return Err(FileOperationError::new(
                localize("fileIsDirectoryReadError", "Unable to read file '{0}' that is actually a directory", &[&self.resource_for_error(resource)]),
                FileOperationResult::FileIsDirectory,
            )
            .into());
        }

        // Throw if file not modified since (unless disabled)
        if options.etag.as_deref().is_some_and(|e| e != ETAG_DISABLED && e == stat.etag()) {
            return Err(FileOperationError::not_modified_since(localize("fileNotModifiedError", "File not modified since", &[]), stat).into());
        }

        // Throw if file is too large to load
        validate_read_file_limits(&self.resource_for_error(resource), stat.size(), options)?;
        Ok(stat)
    }

    // endregion

    // region: mover e copiar

    pub fn can_move(self: &Rc<Self>, source: &URI, target: &URI, overwrite: Option<bool>) -> Promise<Result<(), Error>> {
        self.do_can_move_copy(source, target, true, overwrite)
    }

    pub fn can_copy(self: &Rc<Self>, source: &URI, target: &URI, overwrite: Option<bool>) -> Promise<Result<(), Error>> {
        self.do_can_move_copy(source, target, false, overwrite)
    }

    fn do_can_move_copy(self: &Rc<Self>, source: &URI, target: &URI, is_move: bool, overwrite: Option<bool>) -> Promise<Result<(), Error>> {
        let (this, source, target) = (self.clone(), source.clone(), target.clone());
        async_fn(async move {
            if source.to_string(false) != target.to_string(false) {
                let result: Result<(), Error> = async {
                    let source_provider = if is_move {
                        this.throw_if_file_system_is_readonly(this.with_write_provider(&source).await?, &source)?
                    } else {
                        this.with_read_provider(&source).await?
                    };
                    let target_provider = this.throw_if_file_system_is_readonly(this.with_write_provider(&target).await?, &target)?;
                    this.do_validate_move_copy(&source_provider, &source, &target_provider, &target, is_move, overwrite).await?;
                    Ok(())
                }
                .await;
                return Ok(result);
            }
            Ok(Ok(()))
        })
    }

    /// `move(source, target, overwrite)`.
    pub fn move_(self: &Rc<Self>, source: &URI, target: &URI, overwrite: Option<bool>) -> Promise<FileStatWithMetadata> {
        let (this, source, target) = (self.clone(), source.clone(), target.clone());
        async_fn(async move {
            let source_provider = this.throw_if_file_system_is_readonly(this.with_write_provider(&source).await?, &source)?;
            let target_provider = this.throw_if_file_system_is_readonly(this.with_write_provider(&target).await?, &target)?;
            // move
            let mode = this.do_move_copy(&source_provider, &source, &target_provider, &target, MoveCopy::Move, overwrite.unwrap_or(false)).await?;
            // resolve and send events
            let file_stat = this.resolve_with_metadata(&target, None).await?;
            this.on_did_run_operation.fire(FileOperationEvent::new(source, if mode == MoveCopy::Move { FileOperation::Move } else { FileOperation::Copy }, Some(file_stat.clone())));
            Ok(file_stat)
        })
    }

    /// `copy(source, target, overwrite)`.
    pub fn copy(self: &Rc<Self>, source: &URI, target: &URI, overwrite: Option<bool>) -> Promise<FileStatWithMetadata> {
        let (this, source, target) = (self.clone(), source.clone(), target.clone());
        async_fn(async move {
            let source_provider = this.with_read_provider(&source).await?;
            let target_provider = this.throw_if_file_system_is_readonly(this.with_write_provider(&target).await?, &target)?;
            // copy
            let mode = this.do_move_copy(&source_provider, &source, &target_provider, &target, MoveCopy::Copy, overwrite.unwrap_or(false)).await?;
            // resolve and send events
            let file_stat = this.resolve_with_metadata(&target, None).await?;
            this.on_did_run_operation.fire(FileOperationEvent::new(source, if mode == MoveCopy::Copy { FileOperation::Copy } else { FileOperation::Move }, Some(file_stat.clone())));
            Ok(file_stat)
        })
    }

    async fn do_move_copy(
        self: &Rc<Self>,
        source_provider: &FileSystemProviderRef,
        source: &URI,
        target_provider: &FileSystemProviderRef,
        target: &URI,
        mode: MoveCopy,
        overwrite: bool,
    ) -> Result<MoveCopy, Error> {
        if source.to_string(false) == target.to_string(false) {
            return Ok(mode); // simulate node.js behaviour here and do a no-op if paths match
        }

        // validation
        let (exists, is_same_resource_with_different_path_case) =
            self.do_validate_move_copy(source_provider, source, target_provider, target, mode == MoveCopy::Move, Some(overwrite)).await?;

        // delete the target if it exists and we are overwriting (not for case-only renames)
        if exists && !is_same_resource_with_different_path_case && overwrite {
            self.del(target, Some(PartialFileDeleteOptions { recursive: Some(true), ..Default::default() })).await?;
        }

        // create parent folders
        let target_ext_uri = self.get_ext_uri(target_provider.as_ref()).0;
        self.mkdirp(target_provider, &target_ext_uri.dirname(target)).await?;

        // copy source => target
        if mode == MoveCopy::Copy {
            // same provider with fast copy: leverage copy() functionality
            if Rc::ptr_eq(source_provider, target_provider) && has_file_folder_copy_capability(source_provider.as_ref()) {
                source_provider.copy(source, target, &FileOverwriteOptions { overwrite }).await?;
            }
            // when copying via buffer/unbuffered, we have to manually traverse the source if it is a folder
            else {
                let source_file = self.resolve(source, None).await?;
                if source_file.is_directory {
                    self.do_copy_folder(source_provider, source_file, target_provider, target).await?;
                } else {
                    self.do_copy_file(source_provider, source, target_provider, target).await?;
                }
            }
            return Ok(mode);
        }

        // move source => target
        // same provider: leverage rename() functionality
        if Rc::ptr_eq(source_provider, target_provider) {
            source_provider.rename(source, target, &FileOverwriteOptions { overwrite }).await?;
            return Ok(mode);
        }
        // across providers: copy to target & delete at source
        Box::pin(self.do_move_copy(source_provider, source, target_provider, target, MoveCopy::Copy, overwrite)).await?;
        self.del(source, Some(PartialFileDeleteOptions { recursive: Some(true), ..Default::default() })).await?;
        Ok(MoveCopy::Copy)
    }

    async fn do_copy_file(self: &Rc<Self>, source_provider: &FileSystemProviderRef, source: &URI, target_provider: &FileSystemProviderRef, target: &URI) -> Result<(), Error> {
        let (sp, tp) = (source_provider.as_ref(), target_provider.as_ref());
        // copy: source (buffered) => target (buffered)
        if has_open_read_write_close_capability(sp) && has_open_read_write_close_capability(tp) {
            return self.do_pipe_buffered(source_provider, source, target_provider, target).await;
        }
        // copy: source (buffered) => target (unbuffered)
        if has_open_read_write_close_capability(sp) && has_read_write_capability(tp) {
            return self.do_pipe_buffered_to_unbuffered(source_provider, source, target_provider, target).await;
        }
        // copy: source (unbuffered) => target (buffered)
        if has_read_write_capability(sp) && has_open_read_write_close_capability(tp) {
            return self.do_pipe_unbuffered_to_buffered(source_provider, source, target_provider, target).await;
        }
        // copy: source (unbuffered) => target (unbuffered)
        if has_read_write_capability(sp) && has_read_write_capability(tp) {
            return self.do_pipe_unbuffered(source_provider, source, target_provider, target).await;
        }
        Ok(())
    }

    async fn do_copy_folder(self: &Rc<Self>, source_provider: &FileSystemProviderRef, source_folder: FileStat, target_provider: &FileSystemProviderRef, target_folder: &URI) -> Result<(), Error> {
        // create folder in target
        target_provider.mkdir(target_folder).await?;
        // create children in target
        if let Some(children) = source_folder.children {
            let target_ext_uri = self.get_ext_uri(target_provider.as_ref()).0;
            let copies = children.into_iter().map(|source_child| {
                let (this, source_provider, target_provider) = (self.clone(), source_provider.clone(), target_provider.clone());
                let target_child = target_ext_uri.join_path(target_folder, &[&source_child.name]);
                async_fn(async move {
                    if source_child.is_directory {
                        let resolved = this.resolve(&source_child.resource, None).await?;
                        Box::pin(this.do_copy_folder(&source_provider, resolved, &target_provider, &target_child)).await
                    } else {
                        this.do_copy_file(&source_provider, &source_child.resource, &target_provider, &target_child).await
                    }
                })
            });
            promises::settled(copies.collect()).await?;
        }
        Ok(())
    }

    async fn do_validate_move_copy(
        self: &Rc<Self>,
        source_provider: &FileSystemProviderRef,
        source: &URI,
        target_provider: &FileSystemProviderRef,
        target: &URI,
        is_move: bool,
        overwrite: Option<bool>,
    ) -> Result<(bool, bool), Error> {
        let mut is_same_resource_with_different_path_case = false;

        // Check if source is equal or parent to target (requires providers to be the same)
        if Rc::ptr_eq(source_provider, target_provider) {
            let (provider_ext_uri, is_path_case_sensitive) = self.get_ext_uri(source_provider.as_ref());
            if !is_path_case_sensitive {
                is_same_resource_with_different_path_case = provider_ext_uri.is_equal(Some(source), Some(target), false);
            }
            if is_same_resource_with_different_path_case && !is_move {
                return Err(Error::new(localize(
                    "unableToMoveCopyError1",
                    "Unable to copy when source '{0}' is same as target '{1}' with different path case on a case insensitive file system",
                    &[&self.resource_for_error(source), &self.resource_for_error(target)],
                )));
            }
            if !is_same_resource_with_different_path_case && provider_ext_uri.is_equal_or_parent(target, source, false, false) {
                return Err(Error::new(localize(
                    "unableToMoveCopyError2",
                    "Unable to move/copy when source '{0}' is parent of target '{1}'.",
                    &[&self.resource_for_error(source), &self.resource_for_error(target)],
                )));
            }
        }

        // Extra checks if target exists and this is not a rename
        let exists = self.exists(target).await?;
        if exists && !is_same_resource_with_different_path_case {
            // Bail out if target exists and we are not about to overwrite
            if !overwrite.unwrap_or(false) {
                return Err(FileOperationError::new(
                    localize("unableToMoveCopyError3", "Unable to move/copy '{0}' because target '{1}' already exists at destination.", &[&self.resource_for_error(source), &self.resource_for_error(target)]),
                    FileOperationResult::FileMoveConflict,
                )
                .into());
            }
            // Special case: if the target is a parent of the source, we cannot delete it to overwrite
            // because that would delete the source as well.
            if Rc::ptr_eq(source_provider, target_provider) {
                let provider_ext_uri = self.get_ext_uri(source_provider.as_ref()).0;
                if provider_ext_uri.is_equal_or_parent(source, target, false, false) {
                    return Err(Error::new(localize(
                        "unableToMoveCopyError4",
                        "Unable to move/copy '{0}' into '{1}' since a file would replace the folder it is contained in.",
                        &[&self.resource_for_error(source), &self.resource_for_error(target)],
                    )));
                }
            }
        }
        Ok((exists, is_same_resource_with_different_path_case))
    }

    fn get_ext_uri(&self, provider: &dyn FileSystemProvider) -> (ExtUri, bool) {
        let is_path_case_sensitive = self.is_path_case_sensitive(provider);
        (if is_path_case_sensitive { self.ext_uri.clone() } else { self.ext_uri_ignore_path_case.clone() }, is_path_case_sensitive)
    }

    fn is_path_case_sensitive(&self, provider: &dyn FileSystemProvider) -> bool {
        provider.capabilities().intersects(FileSystemProviderCapabilities::PATH_CASE_SENSITIVE)
    }

    // endregion

    // region: pastas e apagar

    pub fn create_folder(self: &Rc<Self>, resource: &URI) -> Promise<FileStatWithMetadata> {
        let (this, resource) = (self.clone(), resource.clone());
        async_fn(async move {
            let provider = this.throw_if_file_system_is_readonly(this.with_provider(&resource).await?, &resource)?;
            // mkdir recursively
            this.mkdirp(&provider, &resource).await?;
            // events
            let file_stat = this.resolve_with_metadata(&resource, None).await?;
            this.on_did_run_operation.fire(FileOperationEvent::new(resource, FileOperation::Create, Some(file_stat.clone())));
            Ok(file_stat)
        })
    }

    async fn mkdirp(self: &Rc<Self>, provider: &FileSystemProviderRef, directory: &URI) -> Result<(), Error> {
        let mut directories_to_create: Vec<String> = Vec::new();
        let provider_ext_uri = self.get_ext_uri(provider.as_ref()).0;
        let mut directory = directory.clone();

        // mkdir until we reach root
        while !provider_ext_uri.is_equal(Some(&directory), Some(&provider_ext_uri.dirname(&directory)), false) {
            match provider.stat(&directory).await {
                Ok(stat) => {
                    if !stat.file_type.intersects(FileType::DIRECTORY) {
                        return Err(Error::new(localize(
                            "mkdirExistsError",
                            "Unable to create folder '{0}' that already exists but is not a directory",
                            &[&self.resource_for_error(&directory)],
                        )));
                    }
                    break; // we have hit a directory that exists -> good
                }
                Err(error) => {
                    // Bubble up any other error that is not file not found
                    if to_file_system_provider_error_code(Some(&error)) != FileSystemProviderErrorCode::FileNotFound {
                        return Err(error);
                    }
                    // Upon error, remember directories that need to be created
                    directories_to_create.push(provider_ext_uri.basename(&directory, None));
                    // Continue up
                    directory = provider_ext_uri.dirname(&directory);
                }
            }
        }

        // Create directories as needed
        for name in directories_to_create.iter().rev() {
            directory = provider_ext_uri.join_path(&directory, &[name]);
            if let Err(error) = provider.mkdir(&directory).await {
                // For mkdirp() we tolerate that the mkdir() call fails in case the folder already exists.
                if to_file_system_provider_error_code(Some(&error)) != FileSystemProviderErrorCode::FileExists {
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    pub fn can_delete(self: &Rc<Self>, resource: &URI, options: Option<PartialFileDeleteOptions>) -> Promise<Result<(), Error>> {
        let (this, resource) = (self.clone(), resource.clone());
        async_fn(async move { Ok(this.do_validate_delete(&resource, options.as_ref()).await.map(|_| ())) })
    }

    async fn do_validate_delete(self: &Rc<Self>, resource: &URI, options: Option<&PartialFileDeleteOptions>) -> Result<FileSystemProviderRef, Error> {
        let provider = self.throw_if_file_system_is_readonly(self.with_provider(resource).await?, resource)?;

        // Validate trash support
        let use_trash = options.is_some_and(|o| o.use_trash == Some(true));
        if use_trash && !provider.capabilities().intersects(FileSystemProviderCapabilities::TRASH) {
            return Err(Error::new(localize("deleteFailedTrashUnsupported", "Unable to delete file '{0}' via trash because provider does not support it.", &[&self.resource_for_error(resource)])));
        }

        // Validate atomic support
        let atomic = options.and_then(|o| o.atomic.as_ref());
        if atomic.is_some() && !provider.capabilities().intersects(FileSystemProviderCapabilities::FILE_ATOMIC_DELETE) {
            return Err(Error::new(localize("deleteFailedAtomicUnsupported", "Unable to delete file '{0}' atomically because provider does not support it.", &[&self.resource_for_error(resource)])));
        }
        if use_trash && atomic.is_some() {
            return Err(Error::new(localize("deleteFailedTrashAndAtomicUnsupported", "Unable to atomically delete file '{0}' because using trash is enabled.", &[&self.resource_for_error(resource)])));
        }

        // Validate delete
        match provider.stat(resource).await {
            Ok(stat) => self.throw_if_file_is_readonly(resource, &stat)?,
            Err(_) => {
                return Err(FileOperationError::new(
                    localize("deleteFailedNotFound", "Unable to delete nonexistent file '{0}'", &[&self.resource_for_error(resource)]),
                    FileOperationResult::FileNotFound,
                )
                .into());
            }
        }

        // Validate recursive
        let recursive = options.is_some_and(|o| o.recursive == Some(true));
        if !recursive {
            let stat = self.resolve(resource, None).await?;
            if stat.is_directory && stat.children.as_ref().is_some_and(|children| !children.is_empty()) {
                return Err(Error::new(localize("deleteFailedNonEmptyFolder", "Unable to delete non-empty folder '{0}'.", &[&self.resource_for_error(resource)])));
            }
        }
        Ok(provider)
    }

    /// `del(resource, options)`.
    pub fn del(self: &Rc<Self>, resource: &URI, options: Option<PartialFileDeleteOptions>) -> Promise<()> {
        let (this, resource) = (self.clone(), resource.clone());
        async_fn(async move {
            let provider = this.do_validate_delete(&resource, options.as_ref()).await?;
            let mut delete_file_options = options.unwrap_or_default();

            // enforce atomic delete if the provider requires it
            if has_file_atomic_delete_capability(provider.as_ref())
                && delete_file_options.atomic.is_none()
                && let Some(enforced) = provider.enforce_atomic_delete(&resource)
            {
                delete_file_options.atomic = Some(enforced);
            }

            provider
                .delete(
                    &resource,
                    &FileDeleteOptions {
                        recursive: delete_file_options.recursive.unwrap_or(false),
                        use_trash: delete_file_options.use_trash.unwrap_or(false),
                        atomic: delete_file_options.atomic,
                    },
                )
                .await?;
            this.on_did_run_operation.fire(FileOperationEvent::new(resource, FileOperation::Delete, None));
            Ok(())
        })
    }

    /// `cloneFile(source, target)`.
    pub fn clone_file(self: &Rc<Self>, source: &URI, target: &URI) -> Promise<()> {
        let (this, source, target) = (self.clone(), source.clone(), target.clone());
        async_fn(async move {
            let source_provider = this.with_provider(&source).await?;
            let target_provider = this.throw_if_file_system_is_readonly(this.with_write_provider(&target).await?, &target)?;

            // Check if source is equal to target (requires providers to be the same)
            if Rc::ptr_eq(&source_provider, &target_provider) && this.get_ext_uri(source_provider.as_ref()).0.is_equal(Some(&source), Some(&target), false) {
                return Ok(()); // return early if paths are equal
            }

            // same provider, use `cloneFile` when native support is provided
            if Rc::ptr_eq(&source_provider, &target_provider) && has_file_clone_capability(source_provider.as_ref()) {
                return source_provider.clone_file(&source, &target).await;
            }

            // otherwise, either copy or pipe
            let target_ext_uri = this.get_ext_uri(target_provider.as_ref()).0;
            this.mkdirp(&target_provider, &target_ext_uri.dirname(&target)).await?;
            let source_ext_uri = this.get_ext_uri(source_provider.as_ref()).0;
            if Rc::ptr_eq(&source_provider, &target_provider) && has_file_folder_copy_capability(source_provider.as_ref()) {
                let (provider, from, to) = (source_provider.clone(), source.clone(), target.clone());
                return this.write_queue.queue_for_with(&source, move || provider.copy(&from, &to, &FileOverwriteOptions { overwrite: true }), &source_ext_uri).await;
            }
            let (this2, sp, tp, from, to) = (this.clone(), source_provider.clone(), target_provider.clone(), source.clone(), target.clone());
            this.write_queue
                .queue_for_with(&source, move || async_fn(async move { this2.do_copy_file(&sp, &from, &tp, &to).await }), &source_ext_uri)
                .await
        })
    }

    // endregion

    // region: watch

    /// `createWatcher(resource, options)`: um watcher não recursivo cujos eventos são só os dele.
    pub fn create_watcher(self: &Rc<Self>, resource: &URI, options: WatchOptions) -> Rc<FileSystemWatcher> {
        let correlation_id = WATCHER_CORRELATION_IDS.with(|c| c.replace(c.get() + 1));
        self.watch_correlated(resource, WatchOptions { correlation_id: Some(correlation_id), ..options })
    }

    /// `watch(resource, options)` sem correlação.
    pub fn watch(self: &Rc<Self>, resource: &URI, options: Option<WatchOptions>) -> DisposableRef {
        let options = options.unwrap_or_default();
        self.watch_internal(resource, options)
    }

    /// `watch(resource, options)` com `correlationId`: devolve o watcher com o evento próprio.
    pub fn watch_correlated(self: &Rc<Self>, resource: &URI, options: WatchOptions) -> Rc<FileSystemWatcher> {
        let correlation_id = options.correlation_id;
        let disposables = self.watch_store(resource, options);
        let on_did_change = Emitter::new();
        disposables.add(on_did_change.clone());
        if let Some(correlation_id) = correlation_id {
            let emitter = on_did_change.clone();
            disposables.add(self.internal_on_did_files_change.event().listen(move |e| {
                if e.correlates(correlation_id) {
                    emitter.fire(e.clone());
                }
            }));
        }
        Rc::new(FileSystemWatcher { disposables, on_did_change })
    }

    fn watch_internal(self: &Rc<Self>, resource: &URI, options: WatchOptions) -> DisposableRef {
        self.watch_store(resource, options)
    }

    fn watch_store(self: &Rc<Self>, resource: &URI, options: WatchOptions) -> Rc<DisposableStore> {
        let disposables = DisposableStore::new();
        let watch_disposed = Rc::new(Cell::new(false));
        let dispose_watch: Rc<RefCell<Box<dyn Fn()>>> = Rc::new(RefCell::new(Box::new(|| {})));
        {
            let (watch_disposed, dispose_watch) = (watch_disposed.clone(), dispose_watch.clone());
            disposables.add(to_disposable(move || {
                watch_disposed.set(true);
                let f = std::mem::replace(&mut *dispose_watch.borrow_mut(), Box::new(|| {}));
                f();
            }));
        }
        let (this, resource) = (self.clone(), resource.clone());
        let _ = async_fn(async move {
            match this.do_watch(&resource, &options).await {
                Ok(disposable) => {
                    if watch_disposed.get() {
                        disposable.dispose();
                    } else {
                        *dispose_watch.borrow_mut() = Box::new(move || disposable.dispose());
                    }
                }
                Err(error) => this.log_service.error(&error.to_string()),
            }
            Ok(())
        });
        disposables
    }

    async fn do_watch(self: &Rc<Self>, resource: &URI, options: &WatchOptions) -> Result<DisposableRef, Error> {
        let provider = self.with_provider(resource).await?;

        // Deduplicate identical watch requests
        let watch_key = format!("{}|{:?}", self.get_ext_uri(provider.as_ref()).0.get_comparison_key(resource, false).to_string_lossy(), options);
        {
            let mut watchers = self.active_watchers.borrow_mut();
            if !watchers.contains_key(&watch_key) {
                watchers.insert(watch_key.clone(), ActiveWatcher { count: 0, disposable: provider.watch(resource, options) });
            }
            if let Some(watcher) = watchers.get_mut(&watch_key) {
                watcher.count += 1;
            }
        }
        let this = self.clone();
        Ok(to_disposable(move || {
            let mut watchers = this.active_watchers.borrow_mut();
            let finished = match watchers.get_mut(&watch_key) {
                Some(watcher) => {
                    watcher.count -= 1;
                    watcher.count == 0
                }
                None => false,
            };
            if finished && let Some(watcher) = watchers.remove(&watch_key) {
                drop(watchers);
                watcher.disposable.dispose();
            }
        }))
    }

    // endregion

    // region: escrita em baixo nível

    async fn do_write_buffered(self: &Rc<Self>, provider: &FileSystemProviderRef, resource: &URI, options: Option<&WriteFileOptions>, source: FileContentSource) -> Result<(), Error> {
        let (provider, queued_resource) = (provider.clone(), resource.clone());
        let (unlock, append) = (options.is_some_and(|o| o.unlock), options.is_some_and(|o| o.append));
        let provider_ext_uri = self.get_ext_uri(provider.as_ref()).0;
        self.write_queue
            .queue_for_with(
                resource,
                move || {
                    async_fn(async move {
                        // open handle
                        let handle = provider.open(&queued_resource, &FileOpenOptions::for_write(unlock, append)).await?;

                        // write into handle until all bytes from buffer have been written
                        let result = match source {
                            FileContentSource::Stream(stream) => do_write_stream_buffered_queued(&provider, handle, FileContentSource::Stream(stream)).await,
                            FileContentSource::BufferedStream(buffered) => do_write_stream_buffered_queued(&provider, handle, FileContentSource::BufferedStream(buffered)).await,
                            FileContentSource::Readable(readable) => do_write_readable_buffered_queued(&provider, handle, readable).await,
                            FileContentSource::Buffer(buffer) => do_write_readable_buffered_queued(&provider, handle, Box::new(buffer_to_readable(buffer))).await,
                        };

                        // always close the handle; an error from close wins
                        let closed = provider.close(handle).await;
                        closed?;
                        result.map_err(|error| ensure_file_system_provider_error(Some(error)))
                    })
                },
                &provider_ext_uri,
            )
            .await
    }

    async fn do_write_unbuffered(self: &Rc<Self>, provider: &FileSystemProviderRef, resource: &URI, options: Option<&WriteFileOptions>, source: FileContentSource) -> Result<(), Error> {
        let (provider, queued_resource, options) = (provider.clone(), resource.clone(), options.cloned());
        let provider_ext_uri = self.get_ext_uri(provider.as_ref()).0;
        self.write_queue
            .queue_for_with(
                resource,
                move || async_fn(async move { do_write_unbuffered_queued(&provider, &queued_resource, options.as_ref(), source).await }),
                &provider_ext_uri,
            )
            .await
    }

    async fn do_pipe_buffered(self: &Rc<Self>, source_provider: &FileSystemProviderRef, source: &URI, target_provider: &FileSystemProviderRef, target: &URI) -> Result<(), Error> {
        let (sp, tp, from, to) = (source_provider.clone(), target_provider.clone(), source.clone(), target.clone());
        let target_ext_uri = self.get_ext_uri(target_provider.as_ref()).0;
        self.write_queue.queue_for_with(target, move || async_fn(async move { do_pipe_buffered_queued(&sp, &from, &tp, &to).await }), &target_ext_uri).await
    }

    async fn do_pipe_unbuffered(self: &Rc<Self>, source_provider: &FileSystemProviderRef, source: &URI, target_provider: &FileSystemProviderRef, target: &URI) -> Result<(), Error> {
        let (sp, tp, from, to) = (source_provider.clone(), target_provider.clone(), source.clone(), target.clone());
        let target_ext_uri = self.get_ext_uri(target_provider.as_ref()).0;
        self.write_queue
            .queue_for_with(
                target,
                move || {
                    async_fn(async move {
                        let content = sp.read_file(&from, false).await?;
                        tp.write_file(&to, content, &FileWriteOptions { create: true, overwrite: true, ..Default::default() }).await
                    })
                },
                &target_ext_uri,
            )
            .await
    }

    async fn do_pipe_unbuffered_to_buffered(self: &Rc<Self>, source_provider: &FileSystemProviderRef, source: &URI, target_provider: &FileSystemProviderRef, target: &URI) -> Result<(), Error> {
        let (sp, tp, from, to) = (source_provider.clone(), target_provider.clone(), source.clone(), target.clone());
        let target_ext_uri = self.get_ext_uri(target_provider.as_ref()).0;
        self.write_queue
            .queue_for_with(
                target,
                move || {
                    async_fn(async move {
                        // Open handle
                        let target_handle = tp.open(&to, &FileOpenOptions::for_write(false, false)).await?;
                        // Read entire buffer from source and write buffered
                        let result: Result<(), Error> = async {
                            let buffer = sp.read_file(&from, false).await?;
                            let length = buffer.byte_length();
                            do_write_buffer(&tp, target_handle, &buffer, length, 0, 0).await
                        }
                        .await;
                        let closed = tp.close(target_handle).await;
                        closed?;
                        result.map_err(|error| ensure_file_system_provider_error(Some(error)))
                    })
                },
                &target_ext_uri,
            )
            .await
    }

    async fn do_pipe_buffered_to_unbuffered(self: &Rc<Self>, source_provider: &FileSystemProviderRef, source: &URI, target_provider: &FileSystemProviderRef, target: &URI) -> Result<(), Error> {
        // Read buffer via stream buffered
        let stream = self.read_file_buffered(source_provider, source, &CancellationToken::None, &ReadFileOptions::default());
        let buffer = stream_to_buffer(&*stream).await?;
        // Write buffer into target at once
        self.do_write_unbuffered(target_provider, target, None, FileContentSource::Buffer(buffer)).await
    }

    // endregion

    fn throw_if_file_system_is_readonly(&self, provider: FileSystemProviderRef, resource: &URI) -> Result<FileSystemProviderRef, Error> {
        if provider.capabilities().intersects(FileSystemProviderCapabilities::READONLY) {
            return Err(FileOperationError::new(
                localize("err.readonly", "Unable to modify read-only file '{0}'", &[&self.resource_for_error(resource)]),
                FileOperationResult::FilePermissionDenied,
            )
            .into());
        }
        Ok(provider)
    }

    fn throw_if_file_is_readonly(&self, resource: &URI, stat: &Stat) -> Result<(), Error> {
        if stat.permissions.is_some_and(|p| p.intersects(FilePermission::READONLY)) {
            return Err(FileOperationError::new(
                localize("err.readonly", "Unable to modify read-only file '{0}'", &[&self.resource_for_error(resource)]),
                FileOperationResult::FilePermissionDenied,
            )
            .into());
        }
        Ok(())
    }

    fn resource_for_error(&self, resource: &URI) -> String {
        if resource.scheme().to_string_lossy() == je_base::network::schemas::FILE {
            return resource.fs_path(self.env.platform()).to_string_lossy();
        }
        resource.to_string(true).to_string_lossy()
    }
}

impl IDisposable for FileService {
    fn dispose(&self) {
        self.disposables.dispose();
        self.write_queue.dispose();
        // Dispose the provider watchers
        let watchers: Vec<_> = self.active_watchers.borrow_mut().drain().map(|(_, w)| w.disposable).collect();
        dispose_all(watchers.iter().map(|d| d.as_ref()));
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MoveCopy {
    Move,
    Copy,
}

/// `restoreReadError`: o erro de leitura com a mensagem do serviço, preservando as subclasses.
fn restore_read_error(resource_text: &str, error: &Error) -> Error {
    let message = localize("err.read", "Unable to read file '{0}' ({1})", &[resource_text, &error.to_string()]);
    if let Some(op) = FileOperationError::from_error(error) {
        if let Some(stat) = op.stat {
            return FileOperationError::not_modified_since(message, stat).into();
        }
        if let Some(size) = op.size {
            return FileOperationError::too_large(message, size).into();
        }
    }
    FileOperationError::new(message, to_file_operation_result(error)).into()
}

fn validate_read_file_limits(resource_text: &str, size: u64, options: &ReadFileOptions) -> Result<(), Error> {
    if options.stream.limit_size.is_some_and(|limit| size > limit) {
        return Err(FileOperationError::too_large(localize("fileTooLargeError", "Unable to read file '{0}' that is too large to open", &[resource_text]), size).into());
    }
    Ok(())
}

async fn do_write_unbuffered_queued(provider: &FileSystemProviderRef, resource: &URI, options: Option<&WriteFileOptions>, source: FileContentSource) -> Result<(), Error> {
    let buffer = match source {
        FileContentSource::Buffer(buffer) => buffer,
        FileContentSource::Stream(stream) => stream_to_buffer(&*stream).await?,
        FileContentSource::BufferedStream(buffered) => buffered_stream_to_buffer(buffered).await?,
        FileContentSource::Readable(mut readable) => readable_to_buffer(&mut *readable),
    };
    provider
        .write_file(
            resource,
            buffer,
            &FileWriteOptions {
                create: true,
                overwrite: true,
                unlock: options.is_some_and(|o| o.unlock),
                atomic: options.and_then(|o| o.atomic.clone()),
                append: options.is_some_and(|o| o.append),
            },
        )
        .await
}

async fn do_write_buffer(provider: &FileSystemProviderRef, handle: i64, buffer: &VSBuffer, length: usize, pos_in_file: usize, pos_in_buffer: usize) -> Result<(), Error> {
    let mut total_bytes_written = 0;
    while total_bytes_written < length {
        // Write through the provider
        let bytes_written = provider.write(handle, (pos_in_file + total_bytes_written) as u64, buffer, pos_in_buffer + total_bytes_written, length - total_bytes_written).await?;
        total_bytes_written += bytes_written;
    }
    Ok(())
}

async fn do_write_readable_buffered_queued(provider: &FileSystemProviderRef, handle: i64, mut readable: Box<dyn Readable<VSBuffer>>) -> Result<(), Error> {
    let mut pos_in_file = 0;
    while let Some(chunk) = readable.read() {
        let length = chunk.byte_length();
        do_write_buffer(provider, handle, &chunk, length, pos_in_file, 0).await?;
        pos_in_file += length;
    }
    Ok(())
}

async fn do_write_stream_buffered_queued(provider: &FileSystemProviderRef, handle: i64, source: FileContentSource) -> Result<(), Error> {
    let mut pos_in_file = 0usize;
    let stream = match source {
        FileContentSource::BufferedStream(buffered) => {
            if !buffered.buffer.is_empty() {
                let chunk = VSBuffer::concat(&buffered.buffer, None);
                let length = chunk.byte_length();
                do_write_buffer(provider, handle, &chunk, length, pos_in_file, 0).await?;
                pos_in_file += length;
            }
            if buffered.ended {
                return Ok(());
            }
            buffered.stream
        }
        FileContentSource::Stream(stream) => stream,
        _ => return Ok(()),
    };

    let (promise, resolver) = Promise::<()>::new();
    let pending_write: Rc<RefCell<Promise<()>>> = Rc::new(RefCell::new(Promise::resolved(())));
    let stream_ended = Rc::new(Cell::new(false));
    let first_error: Rc<RefCell<Option<Error>>> = Rc::default();
    let position = Rc::new(Cell::new(pos_in_file));

    // Wait for the pending writes before settling, so the queue only releases after the last write.
    let finish: Rc<dyn Fn(Option<Error>)> = {
        let (stream_ended, first_error, pending_write) = (stream_ended.clone(), first_error.clone(), pending_write.clone());
        Rc::new(move |error: Option<Error>| {
            stream_ended.set(true);
            if let Some(error) = error {
                first_error.borrow_mut().get_or_insert(error);
            }
            let (resolver, first_error) = (resolver.clone(), first_error.clone());
            let _ = pending_write.borrow().then_result(move |result| {
                match (result, first_error.borrow().clone()) {
                    (_, Some(error)) => resolver.reject(error),
                    (Err(error), None) => resolver.reject(error),
                    (Ok(()), None) => resolver.resolve(()),
                }
                Ok(())
            });
        })
    };

    let on_data = {
        let (stream, pending_write, stream_ended, position, provider, finish) = (stream.clone(), pending_write.clone(), stream_ended.clone(), position.clone(), provider.clone(), finish.clone());
        move |chunk: &VSBuffer| {
            if stream_ended.get() {
                return; // stopped reading after an error or the end
            }
            // pause the stream until the write finished: this serializes the writes and applies backpressure
            stream.pause();
            let (chunk, provider, position, stream, stream_ended) = (chunk.clone(), provider.clone(), position.clone(), stream.clone(), stream_ended.clone());
            let next = pending_write.borrow().then_promise(move |()| {
                async_fn(async move {
                    let length = chunk.byte_length();
                    do_write_buffer(&provider, handle, &chunk, length, position.get(), 0).await?;
                    position.set(position.get() + length);
                    set_timeout(
                        move || {
                            if !stream_ended.get() {
                                stream.resume();
                            }
                        },
                        0,
                    );
                    Ok(())
                })
            });
            *pending_write.borrow_mut() = next.clone();
            let finish = finish.clone();
            let _ = next.then_result(move |result| {
                if let Err(error) = result {
                    finish(Some(error));
                }
                Ok(())
            });
        }
    };
    let (on_error, on_end) = (finish.clone(), finish);
    listen_stream(
        &*stream,
        IStreamListener { on_data: Rc::new(on_data), on_error: Rc::new(move |error| on_error(Some(error.clone()))), on_end: Rc::new(move || on_end(None)) },
        None,
    );
    promise.await
}

async fn do_pipe_buffered_queued(source_provider: &FileSystemProviderRef, source: &URI, target_provider: &FileSystemProviderRef, target: &URI) -> Result<(), Error> {
    let mut source_handle: Option<i64> = None;
    let mut target_handle: Option<i64> = None;
    let result: Result<(), Error> = async {
        // Open handles
        source_handle = Some(source_provider.open(source, &FileOpenOptions::for_read()).await?);
        target_handle = Some(target_provider.open(target, &FileOpenOptions::for_write(false, false)).await?);
        let (source_handle, target_handle) = (source_handle.unwrap_or_default(), target_handle.unwrap_or_default());

        let buffer = VSBuffer::alloc(BUFFER_SIZE);
        let mut pos_in_file = 0usize;
        let mut pos_in_buffer = 0usize;
        loop {
            // read from source (sourceHandle) at current position (posInFile) into buffer (buffer) at
            // buffer position (posInBuffer) up to the size of the buffer (buffer.byteLength).
            let bytes_read = source_provider.read(source_handle, pos_in_file as u64, &buffer, pos_in_buffer, buffer.byte_length() - pos_in_buffer).await?;
            // write into target (targetHandle) at current position (posInFile) from buffer (buffer) at
            // buffer position (posInBuffer) all bytes we read (bytesRead).
            do_write_buffer(target_provider, target_handle, &buffer, bytes_read, pos_in_file, pos_in_buffer).await?;
            pos_in_file += bytes_read;
            pos_in_buffer += bytes_read;
            // when buffer full, fill it again from the beginning
            if pos_in_buffer == buffer.byte_length() {
                pos_in_buffer = 0;
            }
            if bytes_read == 0 {
                break;
            }
        }
        Ok(())
    }
    .await;

    // Close handles
    let closes = vec![
        source_handle.map_or_else(|| Promise::resolved(()), |h| source_provider.close(h)),
        target_handle.map_or_else(|| Promise::resolved(()), |h| target_provider.close(h)),
    ];
    promises::settled(closes).await?;
    result.map_err(|error| ensure_file_system_provider_error(Some(error)))
}


/// `whenProviderRegistered(file, fileService)`: resolve quando já há provider para o esquema do recurso,
/// ou assim que um for registrado.
pub fn when_provider_registered(file: &URI, file_service: &Rc<FileService>) -> Promise<()> {
    let scheme = file.scheme().to_string_lossy();
    if file_service.get_provider(&scheme).is_some() {
        return Promise::resolved(());
    }

    let (promise, resolver) = Promise::new();
    let listener: Rc<std::cell::RefCell<Option<DisposableRef>>> = Rc::new(std::cell::RefCell::new(None));
    let slot = listener.clone();
    let disposable = file_service.on_did_change_file_system_provider_registrations().listen(move |event| {
        if event.scheme == scheme && event.added {
            if let Some(disposable) = slot.borrow_mut().take() {
                disposable.dispose();
            }
            resolver.resolve(());
        }
    });
    *listener.borrow_mut() = Some(disposable);
    promise
}
