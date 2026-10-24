//! Porte de `src/vs/workbench/services/workingCopy/common/workingCopyBackupService.ts`: o modelo dos backups conhecidos
//! (`WorkingCopyBackupsModel`), o serviço que escreve e lê os backups no sistema de arquivos
//! (`WorkingCopyBackupServiceImpl`), a versão em memória (`InMemoryWorkingCopyBackupService`) e a classe
//! `WorkingCopyBackupService`, que escolhe uma das duas conforme exista uma pasta de backup do workspace.
//!
//! Divergências:
//! - **Injeção.** `@IFileService` é o `Rc<dyn IFileService>` e `@ILogService` é o [`LogServiceRef`].
//! - **Classe abstrata e implementações.** `WorkingCopyBackupService` é uma struct concreta (a subclasse
//!   `NativeWorkingCopyBackupService` a guarda por composição, ver `native_working_copy_backup_service.rs`) que
//!   delega a um [`BackupServiceImpl`], o que o campo `impl` (união de `WorkingCopyBackupServiceImpl` e
//!   `InMemoryWorkingCopyBackupService`) é no original. `impl instanceof WorkingCopyBackupServiceImpl` é o `match` no
//!   [`ImplHandle`]. O trait [`BackupServiceImpl`] acrescenta `toBackupResource` e `joinBackups`, que as duas
//!   implementações têm além do `IWorkingCopyBackupService`.
//! - **Propriedades e `this`.** `this` dentro das funções `async` é um `Rc` do próprio objeto, guardado como `Weak` por
//!   `Rc::new_cyclic` (`rc()`). `private ready!: Promise<...>` e `private model` são campos em `RefCell`.
//! - **Metadados.** `IWorkingCopyBackupMeta` é um [`Dynamic`] objeto; `deepClone` e `equals` são os de `je_base::objects`.
//!   O `typeId` lido do preâmbulo só vale quando é texto (no original o valor de qualquer tipo passaria adiante, mas o
//!   `IWorkingCopyIdentifier.typeId` é `string`); um `typeId` que não seja texto vira o `NO_TYPE_ID`.
//! - **`versionId`.** `number` é `u64` (o valor padrão do `add` é `0`).
//! - **Erros.** `(<FileOperationError>error).fileOperationResult !== FILE_NOT_FOUND` é o
//!   [`FileOperationError::from_error`], que lê o resultado de volta do `Error` da promessa.
//! - **`ResourceMap`.** O `update` do modelo regrava a entrada (`ResourceMap` do porte não devolve referência mutável).
//! - **`catch` do `resolve` do modelo.** O `try/catch` que ignora qualquer erro é um `Result` descartado.
//! - **Cancelamento.** `token?.isCancellationRequested` é [`is_cancelled`] sobre o `Option<CancellationToken>`.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::{ResourceQueue, async_fn, promises};
use je_base::buffer::{VSBuffer, VSBufferReadableStream, buffer_to_stream, prefixed_buffer_readable, prefixed_buffer_stream, readable_to_buffer, stream_to_buffer};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::hash::hash;
use je_base::jsonc::json_parse;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::map::ResourceMap;
use je_base::network::schemas;
use je_base::objects::{deep_clone, equals};
use je_base::promise::Promise;
use je_base::stream::{Readable, peek_stream};
use je_base::types::is_empty_object;
use je_base::uri::{Field, URI, UriChange, UriComponents};
use je_platform::files::file_service::FileContentSource;
use je_platform::files::i_file_service::IFileService;
use je_platform::files::files::{FileOperationError, FileOperationResult, FileReadStreamOptions, PartialFileDeleteOptions, ReadFileOptions, to_radix};
use je_platform::log::{LogServiceRef, Logger};
use je_platform::resources::{ext_uri, host_platform, join_path};

use super::working_copy::{NO_TYPE_ID, WorkingCopyBackupContent, WorkingCopyIdentifier};
use super::working_copy_backup::{DiscardBackupsFilter, IWorkingCopyBackupService, ResolvedWorkingCopyBackup};

/// `token?.isCancellationRequested`.
fn is_cancelled(token: &Option<CancellationToken>) -> bool {
    token.as_ref().is_some_and(CancellationToken::is_cancellation_requested)
}

/// O valor de uma entrada do cache do modelo: `{ versionId?: number; meta?: IWorkingCopyBackupMeta }`.
struct BackupEntry {
    version_id: u64,
    meta: Option<Dynamic>,
}

/// `WorkingCopyBackupsModel`.
pub struct WorkingCopyBackupsModel {
    cache: RefCell<ResourceMap<BackupEntry>>,
    backup_root: URI,
    file_service: Rc<dyn IFileService>,
}

impl WorkingCopyBackupsModel {
    /// `static async create(backupRoot, fileService)`.
    pub fn create(backup_root: &URI, file_service: &Rc<dyn IFileService>) -> Promise<Rc<WorkingCopyBackupsModel>> {
        let model = Rc::new(Self::new(backup_root.clone(), file_service.clone()));

        async_fn(async move {
            model.resolve().await?;

            Ok(model)
        })
    }

    fn new(backup_root: URI, file_service: Rc<dyn IFileService>) -> Self {
        Self { cache: RefCell::new(ResourceMap::new()), backup_root, file_service }
    }

    fn resolve(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();

        async_fn(async move {
            // Ignore any errors
            let _ = this.read_known_backups().await;

            Ok(())
        })
    }

    fn read_known_backups(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();

        async_fn(async move {
            let backup_root_stat = this.file_service.resolve(&this.backup_root, None).await?;
            if let Some(children) = backup_root_stat.children {
                let tasks: Vec<Promise<()>> = children
                    .iter()
                    .filter(|child| child.is_directory)
                    .map(|backup_schema_folder| {
                        let this = this.clone();
                        let folder = backup_schema_folder.resource.clone();

                        async_fn(async move {
                            // Read backup directory for backups
                            let backup_schema_folder_stat = this.file_service.resolve(&folder, None).await?;

                            // Remember known backups in our caches
                            //
                            // Note: this does NOT account for resolving
                            // associated meta data because that requires
                            // opening the backup and reading the meta
                            // preamble. Instead, when backups are actually
                            // resolved, the meta data will be added via
                            // additional `update` calls.
                            if let Some(backups_for_schema) = backup_schema_folder_stat.children {
                                for backup_for_schema in backups_for_schema {
                                    if !backup_for_schema.is_directory {
                                        this.add(&backup_for_schema.resource, None, None);
                                    }
                                }
                            }

                            Ok(())
                        })
                    })
                    .collect();

                promises::settled(tasks).await?;
            }

            Ok(())
        })
    }

    /// `add(resource, versionId = 0, meta?)`.
    pub fn add(&self, resource: &URI, version_id: Option<u64>, meta: Option<&Dynamic>) {
        self.cache.borrow_mut().set(resource.clone(), BackupEntry { version_id: version_id.unwrap_or(0), meta: meta.map(deep_clone) });
    }

    /// `update(resource, meta?)`.
    pub fn update(&self, resource: &URI, meta: Option<&Dynamic>) {
        let mut cache = self.cache.borrow_mut();
        let version_id = cache.get(resource).map(|entry| entry.version_id);
        if let Some(version_id) = version_id {
            cache.set(resource.clone(), BackupEntry { version_id, meta: meta.map(deep_clone) });
        }
    }

    pub fn count(&self) -> usize {
        self.cache.borrow().size()
    }

    /// `has(resource, versionId?, meta?)`.
    pub fn has(&self, resource: &URI, version_id: Option<u64>, meta: Option<&Dynamic>) -> bool {
        let cache = self.cache.borrow();
        let Some(entry) = cache.get(resource) else {
            return false; // unknown resource
        };

        if version_id.is_some_and(|version_id| version_id != entry.version_id) {
            return false; // different versionId
        }

        if let Some(meta) = meta
            && !equals(meta, entry.meta.as_ref().unwrap_or(&Dynamic::Undefined))
        {
            return false; // different metadata
        }

        true
    }

    pub fn get(&self) -> Vec<URI> {
        self.cache.borrow().keys().cloned().collect()
    }

    pub fn remove(&self, resource: &URI) {
        self.cache.borrow_mut().delete(resource);
    }

    pub fn clear(&self) {
        self.cache.borrow_mut().clear();
    }
}

/// O que as duas implementações têm além do `IWorkingCopyBackupService`: `toBackupResource` e `joinBackups`.
pub trait BackupServiceImpl: IWorkingCopyBackupService {
    fn to_backup_resource(&self, identifier: &WorkingCopyIdentifier) -> URI;

    fn join_backups(&self) -> Promise<()>;
}

/// O `impl` de `WorkingCopyBackupService`: `WorkingCopyBackupServiceImpl | InMemoryWorkingCopyBackupService`.
#[derive(Clone)]
enum ImplHandle {
    Disk(Rc<WorkingCopyBackupServiceImpl>),
    InMemory(Rc<InMemoryWorkingCopyBackupService>),
}

/// `WorkingCopyBackupService` (abstrata no original): delega a uma implementação em disco ou em memória.
pub struct WorkingCopyBackupService {
    base: Rc<Disposable>,
    file_service: Rc<dyn IFileService>,
    log_service: LogServiceRef,
    impl_handle: RefCell<ImplHandle>,
}

impl WorkingCopyBackupService {
    pub fn new(backup_workspace_home: Option<URI>, file_service: Rc<dyn IFileService>, log_service: LogServiceRef) -> Rc<Self> {
        let base = Disposable::new();
        let impl_handle = match backup_workspace_home {
            Some(backup_workspace_home) => {
                let disk = WorkingCopyBackupServiceImpl::new(backup_workspace_home, file_service.clone(), log_service.clone());
                base.register(disk.clone());
                ImplHandle::Disk(disk)
            }
            None => {
                let in_memory = InMemoryWorkingCopyBackupService::new();
                base.register(in_memory.clone());
                ImplHandle::InMemory(in_memory)
            }
        };

        Rc::new(Self { base, file_service, log_service, impl_handle: RefCell::new(impl_handle) })
    }

    /// `fileService` (protegido no original).
    pub fn file_service(&self) -> &Rc<dyn IFileService> {
        &self.file_service
    }

    /// `logService` (privado no original).
    pub fn log_service(&self) -> &LogServiceRef {
        &self.log_service
    }

    fn current(&self) -> Rc<dyn BackupServiceImpl> {
        match &*self.impl_handle.borrow() {
            ImplHandle::Disk(disk) => disk.clone(),
            ImplHandle::InMemory(in_memory) => in_memory.clone(),
        }
    }

    pub fn reinitialize(&self, backup_workspace_home: Option<&URI>) {
        // Re-init implementation (unless we are running in-memory)
        let handle = self.impl_handle.borrow().clone();
        if let ImplHandle::Disk(disk) = handle {
            match backup_workspace_home {
                Some(backup_workspace_home) => disk.initialize(backup_workspace_home),
                None => *self.impl_handle.borrow_mut() = ImplHandle::InMemory(InMemoryWorkingCopyBackupService::new()),
            }
        }
    }

    pub fn to_backup_resource(&self, identifier: &WorkingCopyIdentifier) -> URI {
        self.current().to_backup_resource(identifier)
    }

    pub fn join_backups(&self) -> Promise<()> {
        self.current().join_backups()
    }
}

impl IDisposable for WorkingCopyBackupService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IWorkingCopyBackupService for WorkingCopyBackupService {
    fn has_backup_sync(&self, identifier: &WorkingCopyIdentifier, version_id: Option<u64>, meta: Option<&Dynamic>) -> bool {
        self.current().has_backup_sync(identifier, version_id, meta)
    }

    fn get_backups(&self) -> Promise<Vec<WorkingCopyIdentifier>> {
        self.current().get_backups()
    }

    fn resolve(&self, identifier: &WorkingCopyIdentifier) -> Promise<Option<ResolvedWorkingCopyBackup>> {
        self.current().resolve(identifier)
    }

    fn backup(
        &self,
        identifier: &WorkingCopyIdentifier,
        content: Option<WorkingCopyBackupContent>,
        version_id: Option<u64>,
        meta: Option<Dynamic>,
        token: Option<CancellationToken>,
    ) -> Promise<()> {
        self.current().backup(identifier, content, version_id, meta, token)
    }

    fn discard_backup(&self, identifier: &WorkingCopyIdentifier, token: Option<CancellationToken>) -> Promise<()> {
        self.current().discard_backup(identifier, token)
    }

    fn discard_backups(&self, filter: Option<DiscardBackupsFilter>) -> Promise<()> {
        self.current().discard_backups(filter)
    }
}

const PREAMBLE_END_MARKER: &str = "\n";
const PREAMBLE_END_MARKER_CHARCODE: u8 = b'\n';
/// Using a character that is know to be escaped in a URI as separator.
const PREAMBLE_META_SEPARATOR: char = ' ';
const PREAMBLE_MAX_LENGTH: usize = 10000;

/// `WorkingCopyBackupServiceImpl`: escreve os backups no sistema de arquivos, um arquivo por working copy, com um
/// preâmbulo `<recurso> <json do meta>\n` antes do conteúdo.
struct WorkingCopyBackupServiceImpl {
    this: Weak<WorkingCopyBackupServiceImpl>,
    base: Rc<Disposable>,
    /// Queue IO operations to ensure write/delete file order.
    io_operation_queues: Rc<ResourceQueue>,
    backup_workspace_home: RefCell<URI>,
    file_service: Rc<dyn IFileService>,
    log_service: LogServiceRef,
    ready: RefCell<Option<Promise<Rc<WorkingCopyBackupsModel>>>>,
    model: RefCell<Option<Rc<WorkingCopyBackupsModel>>>,
}

impl WorkingCopyBackupServiceImpl {
    fn new(backup_workspace_home: URI, file_service: Rc<dyn IFileService>, log_service: LogServiceRef) -> Rc<Self> {
        let base = Disposable::new();
        let io_operation_queues = Rc::new(ResourceQueue::new(ext_uri()));
        base.register(io_operation_queues.clone());

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            io_operation_queues,
            backup_workspace_home: RefCell::new(backup_workspace_home.clone()),
            file_service,
            log_service,
            ready: RefCell::new(None),
            model: RefCell::new(None),
        });

        this.initialize(&backup_workspace_home);

        this
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço de backup está vivo enquanto é usado")
    }

    fn initialize(&self, backup_workspace_resource: &URI) {
        *self.backup_workspace_home.borrow_mut() = backup_workspace_resource.clone();

        let ready = self.do_initialize();
        *self.ready.borrow_mut() = Some(ready);
    }

    fn do_initialize(&self) -> Promise<Rc<WorkingCopyBackupsModel>> {
        let this = self.rc();
        let backup_workspace_home = self.backup_workspace_home.borrow().clone();

        async_fn(async move {
            // Create backup model
            let model = WorkingCopyBackupsModel::create(&backup_workspace_home, &this.file_service).await?;
            *this.model.borrow_mut() = Some(model.clone());

            Ok(model)
        })
    }

    fn ready(&self) -> Promise<Rc<WorkingCopyBackupsModel>> {
        self.ready.borrow().clone().expect("o serviço de backup é inicializado no construtor")
    }

    fn create_preamble(&self, identifier: &WorkingCopyIdentifier, meta: Option<&Dynamic>) -> String {
        // `{ ...meta, typeId: identifier.typeId }`
        let mut entries = meta.map(Dynamic::own_entries).unwrap_or_default();
        let type_id = Dynamic::string(&identifier.type_id);
        match entries.iter_mut().find(|(key, _)| *key == "typeId") {
            Some(slot) => slot.1 = type_id,
            None => entries.push((JsString::from("typeId"), type_id)),
        }
        let meta_json = json_stringify(&Dynamic::object(entries)).unwrap_or_default();

        format!("{}{}{}{}", identifier.resource.to_string(false).to_string_lossy(), PREAMBLE_META_SEPARATOR, meta_json, PREAMBLE_END_MARKER)
    }

    fn do_discard_backup(&self, backup_resource: &URI, token: Option<CancellationToken>) -> Promise<()> {
        let this = self.rc();
        let backup_resource = backup_resource.clone();

        async_fn(async move {
            let model = this.ready().await?;
            if is_cancelled(&token) {
                return Ok(());
            }

            let queued = this.io_operation_queues.queue_for(&backup_resource.clone(), {
                let this = this.clone();

                move || {
                    async_fn(async move {
                        if is_cancelled(&token) {
                            return Ok(());
                        }

                        // Delete backup file ignoring any file not found errors
                        this.delete_ignore_file_not_found(&backup_resource).await?;

                        //
                        // Update model
                        //
                        // Note: not checking for cancellation here because a successful
                        // delete of the backup file should be noted in the model to
                        // prevent the model being out of sync with the backup file
                        model.remove(&backup_resource);

                        Ok(())
                    })
                }
            });

            queued.await
        })
    }

    fn delete_ignore_file_not_found(&self, backup_resource: &URI) -> Promise<()> {
        let file_service = self.file_service.clone();
        let backup_resource = backup_resource.clone();

        async_fn(async move {
            match file_service.del(&backup_resource, Some(PartialFileDeleteOptions { recursive: Some(true), ..PartialFileDeleteOptions::default() })).await {
                Ok(()) => Ok(()),
                Err(error) => {
                    let not_found = FileOperationError::from_error(&error).is_some_and(|error| error.file_operation_result == FileOperationResult::FileNotFound);
                    if not_found {
                        Ok(())
                    } else {
                        Err(error) // re-throw any other error than file not found which is OK
                    }
                }
            }
        })
    }

    fn resolve_identifier(&self, backup_resource: &URI, model: &Rc<WorkingCopyBackupsModel>) -> Promise<Option<WorkingCopyIdentifier>> {
        let this = self.rc();
        let backup_resource = backup_resource.clone();
        let model = model.clone();
        let res: Rc<RefCell<Option<WorkingCopyIdentifier>>> = Rc::new(RefCell::new(None));

        async_fn(async move {
            let slot = res.clone();
            let queued = this.io_operation_queues.queue_for(&backup_resource.clone(), {
                let this = this.clone();

                move || {
                    async_fn(async move {
                        if !model.has(&backup_resource, None, None) {
                            return Ok(()); // require backup to be present
                        }

                        // Read the entire backup preamble by reading up to
                        // `PREAMBLE_MAX_LENGTH` in the backup file until
                        // the `PREAMBLE_END_MARKER` is found
                        let backup_preamble = this.read_to_matching_string(&backup_resource, PREAMBLE_END_MARKER, PREAMBLE_MAX_LENGTH as u64).await?;
                        let Some(backup_preamble) = backup_preamble.filter(|preamble| !preamble.is_empty()) else {
                            return Ok(());
                        };

                        // Figure out the offset in the preamble where meta
                        // information possibly starts. This can be `-1` for
                        // older backups without meta.
                        let meta_start_index = backup_preamble.find(PREAMBLE_META_SEPARATOR);

                        // Extract the preamble content for resource and meta
                        let (resource_preamble, meta_preamble) = match meta_start_index {
                            Some(meta_start_index) if meta_start_index > 0 => {
                                (&backup_preamble[..meta_start_index], Some(&backup_preamble[meta_start_index + PREAMBLE_META_SEPARATOR.len_utf8()..]))
                            }
                            _ => (&backup_preamble[..], None),
                        };

                        // Try to parse the meta preamble for figuring out
                        // `typeId` and `meta` if defined.
                        let (type_id, meta) = parse_preamble_meta(meta_preamble);

                        // Update model entry with now resolved meta
                        model.update(&backup_resource, meta.as_ref());

                        *slot.borrow_mut() = Some(WorkingCopyIdentifier {
                            type_id: type_id.unwrap_or_else(|| NO_TYPE_ID.to_string()),
                            resource: URI::parse(resource_preamble, false),
                        });

                        Ok(())
                    })
                }
            });
            queued.await?;

            let resolved = res.borrow().clone();
            Ok(resolved)
        })
    }

    fn read_to_matching_string(&self, backup_resource: &URI, matching_string: &str, maximum_bytes_to_read: u64) -> Promise<Option<String>> {
        let file_service = self.file_service.clone();
        let backup_resource = backup_resource.clone();
        let matching_string = matching_string.to_string();

        async_fn(async move {
            let options = ReadFileOptions { stream: FileReadStreamOptions { length: Some(maximum_bytes_to_read), ..FileReadStreamOptions::default() }, ..ReadFileOptions::default() };
            let contents = file_service.read_file(&backup_resource, Some(options), None).await?.value.to_string();

            // Unable to find matching string in file
            Ok(contents.find(&matching_string).map(|matching_string_index| contents[..matching_string_index].to_string()))
        })
    }
}

/// `parsePreambleMeta(preambleMetaRaw)`: o `typeId` e o `meta` do trecho JSON do preâmbulo.
fn parse_preamble_meta(preamble_meta_raw: Option<&str>) -> (Option<String>, Option<Dynamic>) {
    let mut type_id: Option<String> = None;
    let mut meta: Option<Dynamic> = None;

    if let Some(preamble_meta_raw) = preamble_meta_raw.filter(|raw| !raw.is_empty()) {
        // Ignore JSON parse errors
        if let Ok(value) = json_parse(JsString::from(preamble_meta_raw).units()) {
            let parsed = Dynamic::from_json(&value);

            // `meta?.typeId`
            let parsed_type_id = if parsed.is_nullish() { Dynamic::Undefined } else { parsed.get_str("typeId") };
            if let Dynamic::String(text) = &parsed_type_id {
                type_id = Some(text.to_string_lossy());
            }

            // `typeId` is a property that we add so we
            // remove it when returning to clients.
            let mut keep_meta = true;
            if let (Dynamic::String(_), Dynamic::Object(object)) = (&parsed_type_id, &parsed) {
                object.delete(&JsString::from("typeId"));

                if is_empty_object(&parsed) {
                    keep_meta = false;
                }
            }

            if keep_meta {
                meta = Some(parsed);
            }
        }
    }

    (type_id, meta)
}

impl IDisposable for WorkingCopyBackupServiceImpl {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl BackupServiceImpl for WorkingCopyBackupServiceImpl {
    fn to_backup_resource(&self, identifier: &WorkingCopyIdentifier) -> URI {
        let backup_workspace_home = self.backup_workspace_home.borrow().clone();

        join_path(&backup_workspace_home, &[&identifier.resource.scheme().to_string_lossy(), &hash_identifier(identifier)])
    }

    fn join_backups(&self) -> Promise<()> {
        self.io_operation_queues.when_drained()
    }
}

impl IWorkingCopyBackupService for WorkingCopyBackupServiceImpl {
    fn has_backup_sync(&self, identifier: &WorkingCopyIdentifier, version_id: Option<u64>, meta: Option<&Dynamic>) -> bool {
        let Some(model) = self.model.borrow().clone() else {
            return false;
        };

        let backup_resource = self.to_backup_resource(identifier);

        model.has(&backup_resource, version_id, meta)
    }

    fn backup(
        &self,
        identifier: &WorkingCopyIdentifier,
        content: Option<WorkingCopyBackupContent>,
        version_id: Option<u64>,
        meta: Option<Dynamic>,
        token: Option<CancellationToken>,
    ) -> Promise<()> {
        let this = self.rc();
        let identifier = identifier.clone();

        async_fn(async move {
            let model = this.ready().await?;
            if is_cancelled(&token) {
                return Ok(());
            }

            let backup_resource = this.to_backup_resource(&identifier);
            if model.has(&backup_resource, version_id, meta.as_ref()) {
                // return early if backup version id matches requested one
                return Ok(());
            }

            let queued = this.io_operation_queues.queue_for(&backup_resource.clone(), {
                let this = this.clone();

                move || {
                    async_fn(async move {
                        if is_cancelled(&token) {
                            return Ok(());
                        }

                        if model.has(&backup_resource, version_id, meta.as_ref()) {
                            // return early if backup version id matches requested one
                            // this can happen when multiple backup IO operations got
                            // scheduled, racing against each other.
                            return Ok(());
                        }

                        // Encode as: Resource + META-START + Meta + END
                        // and respect max length restrictions in case
                        // meta is too large.
                        let mut preamble = this.create_preamble(&identifier, meta.as_ref());
                        if preamble.encode_utf16().count() >= PREAMBLE_MAX_LENGTH {
                            preamble = this.create_preamble(&identifier, None);
                        }

                        // Update backup with value
                        let preamble_buffer = VSBuffer::from_string(&preamble);
                        let backup_buffer = match content {
                            Some(WorkingCopyBackupContent::Stream(stream)) => FileContentSource::Stream(prefixed_buffer_stream(preamble_buffer, &*stream)),
                            Some(WorkingCopyBackupContent::Readable(readable)) => {
                                let source: Box<dyn Readable<VSBuffer>> = Box::new(move || readable.borrow_mut().read());

                                FileContentSource::Readable(Box::new(prefixed_buffer_readable(preamble_buffer, source)))
                            }
                            None => FileContentSource::Buffer(VSBuffer::concat(&[preamble_buffer, VSBuffer::from_string("")], None)),
                        };

                        // Write backup via file service
                        this.file_service.write_file(&backup_resource, backup_buffer, None).await?;

                        //
                        // Update model
                        //
                        // Note: not checking for cancellation here because a successful
                        // write into the backup file should be noted in the model to
                        // prevent the model being out of sync with the backup file
                        model.add(&backup_resource, version_id, meta.as_ref());

                        Ok(())
                    })
                }
            });

            queued.await
        })
    }

    fn discard_backups(&self, filter: Option<DiscardBackupsFilter>) -> Promise<()> {
        let this = self.rc();

        async_fn(async move {
            let model = this.ready().await?;

            // Discard all but some backups
            let except = filter.map(|filter| filter.except).unwrap_or_default();
            if !except.is_empty() {
                let mut except_map = ResourceMap::<bool>::new();
                for except_working_copy in &except {
                    except_map.set(this.to_backup_resource(except_working_copy), true);
                }

                let discards: Vec<Promise<()>> =
                    model.get().into_iter().filter(|backup_resource| !except_map.has(backup_resource)).map(|backup_resource| this.do_discard_backup(&backup_resource, None)).collect();

                promises::settled(discards).await?;
            }
            // Discard all backups
            else {
                let backup_workspace_home = this.backup_workspace_home.borrow().clone();
                this.delete_ignore_file_not_found(&backup_workspace_home).await?;

                model.clear();
            }

            Ok(())
        })
    }

    fn discard_backup(&self, identifier: &WorkingCopyIdentifier, token: Option<CancellationToken>) -> Promise<()> {
        let backup_resource = self.to_backup_resource(identifier);

        self.do_discard_backup(&backup_resource, token)
    }

    fn get_backups(&self) -> Promise<Vec<WorkingCopyIdentifier>> {
        let this = self.rc();

        async_fn(async move {
            let model = this.ready().await?;

            // Ensure to await any pending backup operations
            this.join_backups().await?;

            let resolving: Vec<Promise<Option<WorkingCopyIdentifier>>> = model.get().iter().map(|backup_resource| this.resolve_identifier(backup_resource, &model)).collect();
            let backups = Promise::all(resolving).await?;

            Ok(backups.into_iter().flatten().collect())
        })
    }

    fn resolve(&self, identifier: &WorkingCopyIdentifier) -> Promise<Option<ResolvedWorkingCopyBackup>> {
        let this = self.rc();
        let backup_resource = self.to_backup_resource(identifier);

        async_fn(async move {
            let model = this.ready().await?;

            let res: Rc<RefCell<Option<ResolvedWorkingCopyBackup>>> = Rc::new(RefCell::new(None));
            let slot = res.clone();

            let queued = this.io_operation_queues.queue_for(&backup_resource.clone(), {
                let this = this.clone();

                move || {
                    async_fn(async move {
                        if !model.has(&backup_resource, None, None) {
                            return Ok(()); // require backup to be present
                        }

                        // Load the backup content and peek into the first chunk
                        // to be able to resolve the meta data
                        let backup_stream = this.file_service.read_file_stream(&backup_resource, None, None).await?;
                        let peeked_backup_stream = peek_stream(backup_stream.value.clone(), 1).await?;
                        let first_backup_chunk = VSBuffer::concat(&peeked_backup_stream.buffer, None);

                        // We have seen reports (e.g. https://github.com/microsoft/vscode/issues/78500) where
                        // if VSCode goes down while writing the backup file, the file can turn empty because
                        // it always first gets truncated and then written to. In this case, we will not find
                        // the meta-end marker ('\n') and as such the backup can only be invalid. We bail out
                        // here if that is the case.
                        let preamble_end_index = first_backup_chunk.with_bytes(|bytes| bytes.iter().position(|byte| *byte == PREAMBLE_END_MARKER_CHARCODE));
                        let Some(preamble_end_index) = preamble_end_index else {
                            this.log_service.trace(&format!(
                                "Backup: Could not find meta end marker in {}. The file is probably corrupt (filesize: {}).",
                                backup_resource.to_string(false).to_string_lossy(),
                                backup_stream.size()
                            ));

                            return Ok(());
                        };

                        let preambel_raw = first_backup_chunk.slice(Some(0), Some(preamble_end_index as i64)).to_string();

                        // Extract meta data (if any)
                        let mut meta: Option<Dynamic> = None;
                        if let Some(meta_start_index) = preambel_raw.find(PREAMBLE_META_SEPARATOR) {
                            meta = parse_preamble_meta(Some(&preambel_raw[meta_start_index + PREAMBLE_META_SEPARATOR.len_utf8()..])).1;
                        }

                        // Update model entry with now resolved meta
                        model.update(&backup_resource, meta.as_ref());

                        // Build a new stream without the preamble
                        let first_backup_chunk_without_preamble = first_backup_chunk.slice(Some(preamble_end_index as i64 + 1), None);
                        let value: VSBufferReadableStream = if peeked_backup_stream.ended {
                            buffer_to_stream(first_backup_chunk_without_preamble)
                        } else {
                            prefixed_buffer_stream(first_backup_chunk_without_preamble, &*peeked_backup_stream.stream)
                        };

                        *slot.borrow_mut() = Some(ResolvedWorkingCopyBackup { meta, value });

                        Ok(())
                    })
                }
            });
            queued.await?;

            let resolved = res.borrow().clone();
            Ok(resolved)
        })
    }
}

/// `InMemoryWorkingCopyBackupService`: guarda os backups em memória, para quando não há pasta de backup.
pub struct InMemoryWorkingCopyBackupService {
    this: Weak<InMemoryWorkingCopyBackupService>,
    base: Rc<Disposable>,
    backups: RefCell<ResourceMap<InMemoryBackup>>,
}

/// O valor do mapa de `InMemoryWorkingCopyBackupService`: `{ typeId: string; content: VSBuffer; meta?: ... }`.
struct InMemoryBackup {
    type_id: String,
    content: VSBuffer,
    meta: Option<Dynamic>,
}

impl InMemoryWorkingCopyBackupService {
    pub fn new() -> Rc<Self> {
        Rc::new_cyclic(|this| Self { this: this.clone(), base: Disposable::new(), backups: RefCell::new(ResourceMap::new()) })
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço de backup está vivo enquanto é usado")
    }
}

impl IDisposable for InMemoryWorkingCopyBackupService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl BackupServiceImpl for InMemoryWorkingCopyBackupService {
    fn to_backup_resource(&self, identifier: &WorkingCopyIdentifier) -> URI {
        URI::from(
            &UriComponents { scheme: Some(JsString::from(schemas::IN_MEMORY)), path: Some(JsString::from(hash_identifier(identifier))), ..UriComponents::default() },
            false,
        )
    }

    fn join_backups(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

impl IWorkingCopyBackupService for InMemoryWorkingCopyBackupService {
    fn has_backup_sync(&self, identifier: &WorkingCopyIdentifier, _version_id: Option<u64>, _meta: Option<&Dynamic>) -> bool {
        let backup_resource = self.to_backup_resource(identifier);

        self.backups.borrow().has(&backup_resource)
    }

    fn backup(
        &self,
        identifier: &WorkingCopyIdentifier,
        content: Option<WorkingCopyBackupContent>,
        _version_id: Option<u64>,
        meta: Option<Dynamic>,
        _token: Option<CancellationToken>,
    ) -> Promise<()> {
        let this = self.rc();
        let identifier = identifier.clone();

        async_fn(async move {
            let backup_resource = this.to_backup_resource(&identifier);
            let content = match content {
                Some(WorkingCopyBackupContent::Stream(stream)) => stream_to_buffer(&*stream).await?,
                Some(WorkingCopyBackupContent::Readable(readable)) => readable_to_buffer(&mut **readable.borrow_mut()),
                None => VSBuffer::from_string(""),
            };
            this.backups.borrow_mut().set(backup_resource, InMemoryBackup { type_id: identifier.type_id.clone(), content, meta });

            Ok(())
        })
    }

    fn resolve(&self, identifier: &WorkingCopyIdentifier) -> Promise<Option<ResolvedWorkingCopyBackup>> {
        let this = self.rc();
        let backup_resource = self.to_backup_resource(identifier);

        async_fn(async move {
            let resolved = this.backups.borrow().get(&backup_resource).map(|backup| {
                let value: VSBufferReadableStream = buffer_to_stream(backup.content.clone());

                ResolvedWorkingCopyBackup { value, meta: backup.meta.clone() }
            });

            Ok(resolved)
        })
    }

    fn get_backups(&self) -> Promise<Vec<WorkingCopyIdentifier>> {
        let this = self.rc();

        async_fn(async move {
            let backups: Vec<WorkingCopyIdentifier> = this.backups.borrow().entries().map(|(resource, backup)| WorkingCopyIdentifier { type_id: backup.type_id.clone(), resource: resource.clone() }).collect();

            Ok(backups)
        })
    }

    fn discard_backup(&self, identifier: &WorkingCopyIdentifier, _token: Option<CancellationToken>) -> Promise<()> {
        let this = self.rc();
        let backup_resource = self.to_backup_resource(identifier);

        async_fn(async move {
            this.backups.borrow_mut().delete(&backup_resource);

            Ok(())
        })
    }

    fn discard_backups(&self, filter: Option<DiscardBackupsFilter>) -> Promise<()> {
        let this = self.rc();

        async_fn(async move {
            let except = filter.map(|filter| filter.except).unwrap_or_default();
            if !except.is_empty() {
                let mut except_map = ResourceMap::<bool>::new();
                for except_working_copy in &except {
                    except_map.set(this.to_backup_resource(except_working_copy), true);
                }

                for backup in this.get_backups().await? {
                    if !except_map.has(&this.to_backup_resource(&backup)) {
                        this.discard_backup(&backup, None).await?;
                    }
                }
            } else {
                this.backups.borrow_mut().clear();
            }

            Ok(())
        })
    }
}

/// `hashIdentifier(identifier)` (exportada só para teste no original).
pub fn hash_identifier(identifier: &WorkingCopyIdentifier) -> String {
    // IMPORTANT: for backwards compatibility, ensure that
    // we ignore the `typeId` unless a value is provided.
    // To preserve previous backups without type id, we
    // need to just hash the resource. Otherwise we use
    // the type id as a seed to the resource path.
    let resource = if !identifier.type_id.is_empty() {
        let type_id_hash = hash_string(&JsString::from(identifier.type_id.as_str()));
        if !identifier.resource.path().is_empty() {
            join_path(&identifier.resource, &[&type_id_hash])
        } else {
            identifier.resource.with(&UriChange { path: Field::from(type_id_hash.as_str()), ..UriChange::default() })
        }
    } else {
        identifier.resource.clone()
    };

    hash_path(&resource)
}

fn hash_path(resource: &URI) -> String {
    let scheme = resource.scheme();
    let str = if scheme == schemas::FILE || scheme == schemas::UNTITLED { resource.fs_path(host_platform()) } else { resource.to_string(false) };

    hash_string(&str)
}

/// `hash(str).toString(16)`.
fn hash_string(str: &JsString) -> String {
    to_radix(i64::from(hash(&Dynamic::String(str.clone()))), 16)
}
