//! Infraestrutura de teste de `src/vs/workbench/services/workingCopy/test/browser/storedFileWorkingCopy.test.ts`: o
//! modelo e as fábricas do teste (`TestStoredFileWorkingCopyModel`, `TestStoredFileWorkingCopyModelWithCustomSave` e as duas
//! fábricas), o `TestServiceAccessor` com o `workbenchInstantiationService` que o alimenta, e os dublês de serviço que
//! faltavam (`IWorkingCopyFileService` com os participantes de salvar, `IWorkingCopyEditorService`, `IElevatedFileService` do
//! navegador e `IProgressService`).
//!
//! Divergências:
//! - **Herança.** `TestStoredFileWorkingCopyModelWithCustomSave extends TestStoredFileWorkingCopyModel` é o mesmo tipo
//!   ([`TestStoredFileWorkingCopyModel`]) com o estado do `save` customizado presente (`saveCounter`, `throwOnSave`,
//!   `saveOperation`). Sem esse estado o `save` do contrato devolve `None` (o `model.save` indefinido da classe base), então a
//!   working copy cai no `snapshot` mais a escrita do serviço de arquivos. As duas fábricas são o mesmo tipo
//!   ([`TestStoredFileWorkingCopyModelFactory::new`] e [`TestStoredFileWorkingCopyModelFactory::with_custom_save`]): a working
//!   copy genérica no modelo fica com um tipo só, e um modelo por classe obrigaria a repassar cada membro da base.
//! - **`workbenchInstantiationService` e `TestServiceAccessor`.** O contêiner de injeção não entra: o
//!   [`TestServiceAccessor`] cria só os serviços que a working copy de arquivo armazenado lê (os mesmos dublês do original:
//!   `TestFileService`, `FilesConfigurationService`, `WorkingCopyService`, `InMemoryWorkingCopyBackupService`,
//!   `TestNotificationService`, `TestEditorService`, `NullLogService`) e os registra no `DisposableStore` dado, no lugar do
//!   `disposables.add(...)`. O `UriIdentityService` é o real, como no original.
//! - **`TestWorkingCopyBackupService`.** É o [`TestWorkingCopyBackupService`]: o `InMemoryWorkingCopyBackupService` com o
//!   `resolved` (o `parseBackupContent` não é lido por estas suítes).
//! - **`WorkingCopyFileService`.** O serviço real ainda não foi portado: o [`TestWorkingCopyFileService`] traz os três eventos
//!   de operação com `waitUntil`, as operações `create`, `move`, `copy` e `delete` (que o teste do gerenciador chama) e os
//!   participantes de salvar do `StoredFileWorkingCopySaveParticipant` de produção. As divergências dele estão na doc do tipo.
//! - **`WorkingCopyEditorService`.** O real lê `editorService.getEditors(...)`, que no `TestEditorService` é sempre `[]`: o
//!   [`TestWorkingCopyEditorService`] guarda os handlers e o `findEditor` devolve `None`.
//! - **`BrowserElevatedFileService`.** Não existe no porte de produção: [`BrowserElevatedFileService`] tem o comportamento da
//!   classe do navegador (`isSupported` falso, `writeFileElevated` rejeita com `Unsupported`).
//! - **Teardown.** O laço `for (const workingCopy of accessor.workingCopyService.workingCopies) workingCopy.dispose()` é
//!   [`TestServiceAccessor::dispose_registered_working_copies`]: o `IWorkingCopy` registrado não expõe `dispose`, então ele
//!   cruza o registro do serviço com as working copies que o acessor criou.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::{async_fn, promises};
use je_base::buffer::{VSBuffer, VSBufferReadableStream, new_writeable_buffer_stream, stream_to_buffer};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::promise::Promise;
use je_base::stream::WriteableStream;
use je_base::uri::URI;
use je_instantiation::Injectable;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::file_service::FileContentSource;
use je_platform::files::files::{FileOperation, FileStat, FileStatWithMetadata, WriteFileOptions};
use je_platform::files::i_file_service::IFileService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::log::{LogServiceRef, NullLogService};
use je_platform::resources::ext_uri;
use je_platform::storage::StorageService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_workbench::common::editor::{EditorIdentifier, RevertOptions};
use je_workbench::services::files::elevated_file_service::IElevatedFileService;
use je_workbench::services::label::label_service::LabelService;
use je_workbench::services::textfile::collaborators::{
    IProgress, IProgressService, IWorkingCopyFileService, ProgressOptions, ProgressStep, ProgressTask, StoredFileWorkingCopySaveParticipantContext, WorkingCopyFileEvent,
    WorkingCopyFileEventFile,
};
use je_workbench::services::working_copy::file_working_copy::{IFileWorkingCopyModel, IFileWorkingCopyModelFactory, SnapshotContext};
use je_workbench::services::working_copy::stored_file_working_copy::{
    IStoredFileWorkingCopy, IStoredFileWorkingCopyModel, IStoredFileWorkingCopyModelFactory, StoredFileWorkingCopy, StoredFileWorkingCopyModelContentChangedEvent,
    StoredFileWorkingCopyResolveOptions, StoredFileWorkingCopyResolver, StoredFileWorkingCopyServices,
};
use je_workbench::services::working_copy::stored_file_working_copy_save_participant::{IStoredFileWorkingCopySaveParticipant, StoredFileWorkingCopySaveParticipant};
use je_workbench::services::working_copy::working_copy::{IWorkingCopy, WorkingCopyBackupContent, WorkingCopyIdentifier};
use je_workbench::services::working_copy::working_copy_backup::{DiscardBackupsFilter, IWorkingCopyBackupService, ResolvedWorkingCopyBackup};
use je_workbench::services::working_copy::working_copy_backup_service::InMemoryWorkingCopyBackupService;
use je_workbench::services::working_copy::working_copy_editor_service::{IWorkingCopyEditorHandler, IWorkingCopyEditorService};
use je_workbench::services::working_copy::working_copy_service::{IWorkingCopyService, WorkingCopyService};
use je_workbench::test::browser::workbench_test_services::{TestEditorService, TestEnvironmentService, TestPathService, TestRemoteAgentService};
use je_workbench::test::common::workbench_test_services::{
    TestContextService, TestFileService, TestFilesConfigurationService, TestLifecycleService, TestMarkerService, TestNotificationService, TestStorageService,
    TestTextResourceConfigurationService,
};

//#region TestStoredFileWorkingCopyModel

/// O estado do `save` customizado de `TestStoredFileWorkingCopyModelWithCustomSave`.
#[derive(Default)]
struct CustomSave {
    save_counter: Cell<usize>,
    throw_on_save: Cell<bool>,
    save_operation: RefCell<Option<Promise<()>>>,
}

/// `TestStoredFileWorkingCopyModel` (e, com o estado do `save` presente, `TestStoredFileWorkingCopyModelWithCustomSave`).
pub struct TestStoredFileWorkingCopyModel {
    this: Weak<TestStoredFileWorkingCopyModel>,
    base: Rc<Disposable>,
    on_did_change_stored_content: Rc<Emitter<StoredFileWorkingCopyModelContentChangedEvent>>,
    on_will_dispose: Rc<Emitter<()>>,
    resource: URI,
    contents: RefCell<String>,
    throw_on_snapshot: Cell<bool>,
    version_id: Cell<usize>,
    pushed_stack_element: Cell<bool>,
    custom_save: Option<CustomSave>,
}

impl TestStoredFileWorkingCopyModel {
    /// `constructor(resource, contents)`; `custom_save` liga o estado da subclasse `...WithCustomSave`.
    pub fn new(resource: URI, contents: String, custom_save: bool) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_stored_content: Rc<Emitter<StoredFileWorkingCopyModelContentChangedEvent>> = Emitter::new();
        let on_will_dispose: Rc<Emitter<()>> = Emitter::new();
        base.register(on_did_change_stored_content.clone());
        base.register(on_will_dispose.clone());

        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            on_did_change_stored_content,
            on_will_dispose,
            resource,
            contents: RefCell::new(contents),
            throw_on_snapshot: Cell::new(false),
            version_id: Cell::new(0),
            pushed_stack_element: Cell::new(false),
            custom_save: custom_save.then(CustomSave::default),
        })
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o modelo está vivo enquanto é usado")
    }

    /// `resource`.
    pub fn resource(&self) -> &URI {
        &self.resource
    }

    /// `contents`.
    pub fn contents(&self) -> String {
        self.contents.borrow().clone()
    }

    /// `fireContentChangeEvent(event)`.
    pub fn fire_content_change_event(&self, event: StoredFileWorkingCopyModelContentChangedEvent) {
        self.on_did_change_stored_content.fire(event);
    }

    /// `updateContents(newContents)`.
    pub fn update_contents(&self, new_contents: &str) {
        self.do_update(new_contents);
    }

    /// `setThrowOnSnapshot()`.
    pub fn set_throw_on_snapshot(&self) {
        self.throw_on_snapshot.set(true);
    }

    /// `versionId--`.
    pub fn decrement_version_id(&self) {
        self.version_id.set(self.version_id.get() - 1);
    }

    /// `pushedStackElement`.
    pub fn pushed_stack_element(&self) -> bool {
        self.pushed_stack_element.get()
    }

    fn custom(&self) -> &CustomSave {
        self.custom_save.as_ref().expect("o modelo não é o de salvar customizado")
    }

    /// `saveCounter`.
    pub fn save_counter(&self) -> usize {
        self.custom().save_counter.get()
    }

    /// `throwOnSave = value`.
    pub fn set_throw_on_save(&self, value: bool) {
        self.custom().throw_on_save.set(value);
    }

    /// `saveOperation = promise`.
    pub fn set_save_operation(&self, operation: Promise<()>) {
        *self.custom().save_operation.borrow_mut() = Some(operation);
    }

    fn do_update(&self, new_contents: &str) {
        *self.contents.borrow_mut() = new_contents.to_string();

        self.version_id.set(self.version_id.get() + 1);

        self.on_did_change_stored_content.fire(StoredFileWorkingCopyModelContentChangedEvent { is_redoing: false, is_undoing: false });
    }
}

impl IDisposable for TestStoredFileWorkingCopyModel {
    fn dispose(&self) {
        self.on_will_dispose.fire(());

        self.base.dispose();
    }
}

impl IFileWorkingCopyModel for TestStoredFileWorkingCopyModel {
    fn on_did_change_content(&self) -> Event<()> {
        self.on_did_change_stored_content.event().map(|_| (), None)
    }

    fn on_will_dispose(&self) -> Event<()> {
        self.on_will_dispose.event()
    }

    fn snapshot(&self, _context: SnapshotContext, _token: CancellationToken) -> Promise<VSBufferReadableStream> {
        if self.throw_on_snapshot.get() {
            return Promise::rejected(Error::new("Fail"));
        }

        let stream = new_writeable_buffer_stream(None);
        stream.end(Some(VSBuffer::from_string(&self.contents.borrow())));

        let stream: VSBufferReadableStream = stream;
        Promise::resolved(stream)
    }

    fn update(&self, contents: VSBufferReadableStream, _token: CancellationToken) -> Promise<()> {
        let this = self.rc();

        async_fn(async move {
            let buffer = stream_to_buffer(&*contents).await?;
            this.do_update(&buffer.to_string());

            Ok(())
        })
    }
}

impl IStoredFileWorkingCopyModel for TestStoredFileWorkingCopyModel {
    fn on_did_change_stored_content(&self) -> Event<StoredFileWorkingCopyModelContentChangedEvent> {
        self.on_did_change_stored_content.event()
    }

    fn version_id(&self) -> Dynamic {
        Dynamic::Number(self.version_id.get() as f64)
    }

    fn push_stack_element(&self) {
        self.pushed_stack_element.set(true);
    }

    /// `save(options, token)` da subclasse; sem o estado customizado é o `model.save` indefinido da base (`None`).
    fn save(&self, _options: WriteFileOptions, token: CancellationToken) -> Option<Promise<FileStatWithMetadata>> {
        self.custom_save.as_ref()?;

        let this = self.rc();
        Some(async_fn(async move {
            let custom = this.custom();
            if custom.throw_on_save.get() {
                return Err(Error::new("Fail"));
            }

            let save_operation = custom.save_operation.borrow().clone();
            if let Some(save_operation) = save_operation {
                save_operation.await?;
            }

            if token.is_cancellation_requested() {
                return Err(Error::new("Canceled"));
            }

            custom.save_counter.set(custom.save_counter.get() + 1);

            Ok(FileStatWithMetadata::new(FileStat {
                resource: this.resource.clone(),
                name: "resource2".to_string(),
                size: Some(0),
                mtime: Some(0),
                ctime: Some(0),
                etag: Some(String::new()),
                readonly: Some(false),
                locked: Some(false),
                executable: Some(false),
                is_file: true,
                is_directory: false,
                is_symbolic_link: false,
                children: None,
            }))
        }))
    }
}

/// `TestStoredFileWorkingCopyModelFactory` e `TestStoredFileWorkingCopyModelWithCustomSaveFactory`.
pub struct TestStoredFileWorkingCopyModelFactory {
    custom_save: bool,
}

impl TestStoredFileWorkingCopyModelFactory {
    /// `new TestStoredFileWorkingCopyModelFactory()`.
    pub fn new() -> Rc<Self> {
        Rc::new(Self { custom_save: false })
    }

    /// `new TestStoredFileWorkingCopyModelWithCustomSaveFactory()`.
    pub fn with_custom_save() -> Rc<Self> {
        Rc::new(Self { custom_save: true })
    }
}

impl IFileWorkingCopyModelFactory<TestStoredFileWorkingCopyModel> for TestStoredFileWorkingCopyModelFactory {
    fn create_model(&self, resource: &URI, contents: VSBufferReadableStream, _token: CancellationToken) -> Promise<Rc<TestStoredFileWorkingCopyModel>> {
        let (resource, custom_save) = (resource.clone(), self.custom_save);

        async_fn(async move {
            let contents = stream_to_buffer(&*contents).await?.to_string();

            Ok(TestStoredFileWorkingCopyModel::new(resource, contents, custom_save))
        })
    }
}

//#endregion

//#region Serviços que faltavam

/// O participante do teste: `{ participate: async (wc, context) => { ... } }`.
struct FnSaveParticipant<F>(F);

impl<F> IStoredFileWorkingCopySaveParticipant for FnSaveParticipant<F>
where
    F: Fn(Rc<dyn IStoredFileWorkingCopy>, StoredFileWorkingCopySaveParticipantContext) -> Promise<()>,
{
    fn participate(
        &self,
        working_copy: Rc<dyn IStoredFileWorkingCopy>,
        context: StoredFileWorkingCopySaveParticipantContext,
        _progress: Rc<dyn IProgress<ProgressStep>>,
        _token: CancellationToken,
    ) -> Promise<()> {
        (self.0)(working_copy, context)
    }
}

/// O objeto literal `{ participate: async (wc, context) => { ... } }` que os casos passam ao `addSaveParticipant`.
pub fn save_participant(
    participate: impl Fn(Rc<dyn IStoredFileWorkingCopy>, StoredFileWorkingCopySaveParticipantContext) -> Promise<()> + 'static,
) -> Rc<dyn IStoredFileWorkingCopySaveParticipant> {
    Rc::new(FnSaveParticipant(participate))
}

/// O `IWorkingCopyFileService` do teste. Os participantes de salvar são o `StoredFileWorkingCopySaveParticipant` de
/// produção; as operações de arquivo (`create`, `move`, `copy` e `delete`) e os três eventos com `waitUntil` seguem o
/// `WorkingCopyFileService` do original (o porte de produção dele ainda não existe: o `IFileService` reduzido não tem
/// `canCreateFile`, `canMove`, `canCopy`, `canDelete`, `move` e `copy`).
///
/// Divergências do `WorkingCopyFileService`:
/// - Sem os participantes de operação de arquivo (`addFileOperationParticipant`: nenhum registrado, então não fazem nada)
///   e sem o `CancellationToken` das operações (só era repassado a eles).
/// - A validação (`canXxx`) é sempre `true` no `TestFileService`; `move` e `copy` do `TestFileService` devolvem `null` e
///   `createFile` o stat que ninguém lê, então as operações devolvem `()`. `move` e `copy` não tocam o serviço de arquivos.
/// - `fireAsync` é disparar o evento e esperar as promessas que os ouvintes entregaram a `waitUntil`.
/// - `getDirty` e o provider de working copies padrão (o único) leem o `IWorkingCopyService`.
pub struct TestWorkingCopyFileService {
    base: Rc<Disposable>,
    save_participants: Rc<StoredFileWorkingCopySaveParticipant>,
    on_will_run_working_copy_file_operation: Rc<Emitter<WorkingCopyFileEvent>>,
    on_did_fail_working_copy_file_operation: Rc<Emitter<WorkingCopyFileEvent>>,
    on_did_run_working_copy_file_operation: Rc<Emitter<WorkingCopyFileEvent>>,
    correlation_ids: Cell<usize>,
    file_service: Rc<dyn IFileService>,
    working_copy_service: Rc<dyn IWorkingCopyService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
}

impl TestWorkingCopyFileService {
    pub fn new(
        log_service: LogServiceRef,
        progress_service: Rc<dyn IProgressService>,
        file_service: Rc<dyn IFileService>,
        working_copy_service: Rc<dyn IWorkingCopyService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_will_run_working_copy_file_operation = Emitter::new();
        let on_did_fail_working_copy_file_operation = Emitter::new();
        let on_did_run_working_copy_file_operation = Emitter::new();
        base.register(on_will_run_working_copy_file_operation.clone());
        base.register(on_did_fail_working_copy_file_operation.clone());
        base.register(on_did_run_working_copy_file_operation.clone());
        let save_participants = StoredFileWorkingCopySaveParticipant::new(log_service, progress_service);
        base.register(save_participants.clone());

        Rc::new(Self {
            base,
            save_participants,
            on_will_run_working_copy_file_operation,
            on_did_fail_working_copy_file_operation,
            on_did_run_working_copy_file_operation,
            correlation_ids: Cell::new(0),
            file_service,
            working_copy_service,
            uri_identity_service,
        })
    }

    /// `addSaveParticipant(participant)`.
    pub fn add_save_participant(&self, participant: Rc<dyn IStoredFileWorkingCopySaveParticipant>) -> DisposableRef {
        self.save_participants.add_save_participant(participant)
    }

    //#region File operations

    /// `create([{ resource }], CancellationToken.None)`.
    pub fn create(self: &Rc<Self>, resources: &[URI]) -> Promise<()> {
        let (this, resources) = (self.clone(), resources.to_vec());

        async_fn(async move {
            if resources.is_empty() {
                return Ok(());
            }

            // before events
            let files = resources.iter().map(|resource| WorkingCopyFileEventFile { source: None, target: resource.clone() }).collect();
            let event = this.new_event(FileOperation::Create, files);
            this.fire_async(&this.on_will_run_working_copy_file_operation, &event).await?;

            // now actually create on disk
            let created = promises::settled(resources.iter().map(|resource| this.file_service.create_file(resource, None, None).then(|_| ())).collect()).await;
            if let Err(error) = created {
                // error event
                this.fire_async(&this.on_did_fail_working_copy_file_operation, &event).await?;

                return Err(error);
            }

            // after event
            this.fire_async(&this.on_did_run_working_copy_file_operation, &event).await
        })
    }

    /// `move([{ file: { source, target } }], CancellationToken.None)`.
    pub fn move_files(self: &Rc<Self>, files: &[WorkingCopyFileEventFile]) -> Promise<()> {
        self.do_move_or_copy(files, true)
    }

    /// `copy([{ file: { source, target } }], CancellationToken.None)`.
    pub fn copy(self: &Rc<Self>, files: &[WorkingCopyFileEventFile]) -> Promise<()> {
        self.do_move_or_copy(files, false)
    }

    fn do_move_or_copy(self: &Rc<Self>, files: &[WorkingCopyFileEventFile], is_move: bool) -> Promise<()> {
        let (this, files) = (self.clone(), files.to_vec());

        async_fn(async move {
            // before event
            let operation = if is_move { FileOperation::Move } else { FileOperation::Copy };
            let event = this.new_event(operation, files.clone());
            this.fire_async(&this.on_will_run_working_copy_file_operation, &event).await?;

            for file in &files {
                let (Some(source), target) = (&file.source, &file.target) else {
                    continue;
                };

                // if source and target are not equal, handle dirty working copies
                // depending on the operation:
                // - move: revert both source and target (if any)
                // - copy: revert target (if any)
                if !this.uri_identity_service.ext_uri().is_equal(Some(source), Some(target), false) {
                    let mut dirty_working_copies = Vec::new();
                    if is_move {
                        dirty_working_copies.extend(this.get_dirty(source));
                    }
                    dirty_working_copies.extend(this.get_dirty(target));
                    promises::settled(dirty_working_copies.iter().map(|dirty_working_copy| dirty_working_copy.revert(Some(RevertOptions { soft: Some(true), ..RevertOptions::default() }))).collect())
                        .await?;
                }
            }

            // after event
            this.fire_async(&this.on_did_run_working_copy_file_operation, &event).await
        })
    }

    /// `delete([{ resource }], CancellationToken.None)`.
    pub fn delete(self: &Rc<Self>, resources: &[URI]) -> Promise<()> {
        let (this, resources) = (self.clone(), resources.to_vec());

        async_fn(async move {
            // before events
            let files = resources.iter().map(|resource| WorkingCopyFileEventFile { source: None, target: resource.clone() }).collect();
            let event = this.new_event(FileOperation::Delete, files);
            this.fire_async(&this.on_will_run_working_copy_file_operation, &event).await?;

            // check for any existing dirty working copies for the resource
            // and do a soft revert before deleting to be able to close
            // any opened editor with these working copies
            for resource in &resources {
                let dirty_working_copies = this.get_dirty(resource);
                promises::settled(dirty_working_copies.iter().map(|dirty_working_copy| dirty_working_copy.revert(Some(RevertOptions { soft: Some(true), ..RevertOptions::default() }))).collect())
                    .await?;
            }

            // now actually delete from disk
            for resource in &resources {
                if let Err(error) = this.file_service.del(resource, None).await {
                    // error event
                    this.fire_async(&this.on_did_fail_working_copy_file_operation, &event).await?;

                    return Err(error);
                }
            }

            // after event
            this.fire_async(&this.on_did_run_working_copy_file_operation, &event).await
        })
    }

    //#endregion

    /// `{ correlationId: this.correlationIds++, operation, files }`.
    fn new_event(&self, operation: FileOperation, files: Vec<WorkingCopyFileEventFile>) -> WorkingCopyFileEvent {
        let correlation_id = self.correlation_ids.get();
        self.correlation_ids.set(correlation_id + 1);

        WorkingCopyFileEvent::new(correlation_id, operation, files)
    }

    /// `emitter.fireAsync(event, CancellationToken.None)`.
    fn fire_async(&self, emitter: &Emitter<WorkingCopyFileEvent>, event: &WorkingCopyFileEvent) -> Promise<()> {
        emitter.fire(event.clone());

        promises::settled(event.take_joiners()).then(|_| ())
    }

    /// `getDirty(resource)` com o provider de working copies padrão.
    fn get_dirty(&self, resource: &URI) -> Vec<Rc<dyn IWorkingCopy>> {
        let ext_uri = self.uri_identity_service.ext_uri();
        let has_provider = self.file_service.has_provider(resource);

        self.working_copy_service
            .working_copies()
            .into_iter()
            .filter(|working_copy| {
                if has_provider {
                    // only check for parents if the resource can be handled
                    // by the file system where we then assume a folder like
                    // path structure
                    ext_uri.is_equal_or_parent(&working_copy.resource(), resource, false, false)
                } else {
                    ext_uri.is_equal(Some(&working_copy.resource()), Some(resource), false)
                }
            })
            .filter(|working_copy| working_copy.is_dirty())
            .collect()
    }
}

impl IDisposable for TestWorkingCopyFileService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IWorkingCopyFileService for TestWorkingCopyFileService {
    fn on_will_run_working_copy_file_operation(&self) -> Event<WorkingCopyFileEvent> {
        self.on_will_run_working_copy_file_operation.event()
    }

    fn on_did_fail_working_copy_file_operation(&self) -> Event<WorkingCopyFileEvent> {
        self.on_did_fail_working_copy_file_operation.event()
    }

    fn on_did_run_working_copy_file_operation(&self) -> Event<WorkingCopyFileEvent> {
        self.on_did_run_working_copy_file_operation.event()
    }

    fn has_save_participants(&self) -> bool {
        !self.save_participants.is_empty()
    }

    fn run_save_participants(
        &self,
        working_copy: Rc<dyn IStoredFileWorkingCopy>,
        context: StoredFileWorkingCopySaveParticipantContext,
        progress: Rc<dyn IProgress<ProgressStep>>,
        token: CancellationToken,
    ) -> Promise<()> {
        self.save_participants.participate(working_copy, context, progress, token)
    }
}

/// `Progress.None` do contrato de progresso do workbench.
struct NoProgress;

impl IProgress<ProgressStep> for NoProgress {
    fn report(&self, _item: ProgressStep) {}
}

/// `TestProgressService`: roda a tarefa com `Progress.None`.
pub struct TestProgressService;

impl IProgressService for TestProgressService {
    fn with_progress(&self, _options: ProgressOptions, task: ProgressTask, _on_did_cancel: Option<Rc<dyn Fn()>>) -> Promise<()> {
        task(Rc::new(NoProgress))
    }
}

/// `TestWorkingCopyBackupService`: o `InMemoryWorkingCopyBackupService` que lembra os identificadores que resolveu
/// (`resolved`). `extends` é a composição com o serviço em memória, de que os métodos do contrato são repassados.
///
/// O `resolved` do original é um `Set<IWorkingCopyIdentifier>` por identidade do objeto; o identificador do porte é um
/// valor (`typeId` e recurso), então [`TestWorkingCopyBackupService::has_resolved`] compara por valor. O
/// `parseBackupContent` não é lido por esta suíte.
pub struct TestWorkingCopyBackupService {
    inner: Rc<InMemoryWorkingCopyBackupService>,
    resolved: RefCell<Vec<WorkingCopyIdentifier>>,
}

impl TestWorkingCopyBackupService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { inner: InMemoryWorkingCopyBackupService::new(), resolved: RefCell::new(Vec::new()) })
    }

    /// `resolved.has(identifier)`.
    pub fn has_resolved(&self, identifier: &WorkingCopyIdentifier) -> bool {
        self.resolved.borrow().contains(identifier)
    }
}

impl IDisposable for TestWorkingCopyBackupService {
    fn dispose(&self) {
        self.inner.dispose();
    }
}

impl IWorkingCopyBackupService for TestWorkingCopyBackupService {
    fn has_backup_sync(&self, identifier: &WorkingCopyIdentifier, version_id: Option<u64>, meta: Option<&Dynamic>) -> bool {
        self.inner.has_backup_sync(identifier, version_id, meta)
    }

    fn get_backups(&self) -> Promise<Vec<WorkingCopyIdentifier>> {
        self.inner.get_backups()
    }

    fn resolve(&self, identifier: &WorkingCopyIdentifier) -> Promise<Option<ResolvedWorkingCopyBackup>> {
        self.resolved.borrow_mut().push(identifier.clone());

        self.inner.resolve(identifier)
    }

    fn backup(
        &self,
        identifier: &WorkingCopyIdentifier,
        content: Option<WorkingCopyBackupContent>,
        version_id: Option<u64>,
        meta: Option<Dynamic>,
        token: Option<CancellationToken>,
    ) -> Promise<()> {
        self.inner.backup(identifier, content, version_id, meta, token)
    }

    fn discard_backup(&self, identifier: &WorkingCopyIdentifier, token: Option<CancellationToken>) -> Promise<()> {
        self.inner.discard_backup(identifier, token)
    }

    fn discard_backups(&self, filter: Option<DiscardBackupsFilter>) -> Promise<()> {
        self.inner.discard_backups(filter)
    }
}

/// `BrowserElevatedFileService`.
pub struct BrowserElevatedFileService;

impl IElevatedFileService for BrowserElevatedFileService {
    fn is_supported(&self, _resource: &URI) -> bool {
        // Saving elevated is currently not supported in web for as
        // long as we have no generic support from the file service
        // (https://github.com/microsoft/vscode/issues/48659)
        false
    }

    fn write_file_elevated(&self, _resource: &URI, _value: FileContentSource, _options: Option<WriteFileOptions>) -> Promise<FileStatWithMetadata> {
        Promise::rejected(Error::new("Unsupported"))
    }
}

/// `WorkingCopyEditorService` sobre o `TestEditorService`: guarda os handlers e não acha editor (o `getEditors()` do dublê é `[]`).
pub struct TestWorkingCopyEditorService {
    on_did_register_handler: Rc<Emitter<Rc<dyn IWorkingCopyEditorHandler>>>,
    handlers: Rc<RefCell<Vec<Rc<dyn IWorkingCopyEditorHandler>>>>,
}

impl TestWorkingCopyEditorService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { on_did_register_handler: Emitter::new(), handlers: Rc::default() })
    }
}

impl IDisposable for TestWorkingCopyEditorService {
    fn dispose(&self) {
        self.on_did_register_handler.dispose();
        self.handlers.borrow_mut().clear();
    }
}

impl IWorkingCopyEditorService for TestWorkingCopyEditorService {
    fn on_did_register_handler(&self) -> Event<Rc<dyn IWorkingCopyEditorHandler>> {
        self.on_did_register_handler.event()
    }

    fn register_handler(&self, handler: Rc<dyn IWorkingCopyEditorHandler>) -> DisposableRef {
        self.handlers.borrow_mut().push(handler.clone());
        self.on_did_register_handler.fire(handler.clone());

        let handlers = self.handlers.clone();
        to_disposable(move || handlers.borrow_mut().retain(|candidate| !std::ptr::addr_eq(Rc::as_ptr(candidate), Rc::as_ptr(&handler))))
    }

    fn find_editor(&self, _working_copy: &dyn IWorkingCopy) -> Option<EditorIdentifier> {
        None
    }
}

//#endregion

//#region TestServiceAccessor

/// O `typeId` que o teste dá às working copies.
pub const STORED_FILE_WORKING_COPY_TYPE_ID: &str = "testStoredFileWorkingCopyType";

/// A working copy do teste.
pub type TestStoredFileWorkingCopy = StoredFileWorkingCopy<TestStoredFileWorkingCopyModel>;

/// `workbenchInstantiationService(undefined, disposables)` mais o `TestServiceAccessor`, com os serviços que a working copy de
/// arquivo armazenado lê.
pub struct TestServiceAccessor {
    pub file_service: Rc<TestFileService>,
    pub log_service: LogServiceRef,
    pub working_copy_file_service: Rc<TestWorkingCopyFileService>,
    pub files_configuration_service: Rc<TestFilesConfigurationService>,
    pub working_copy_backup_service: Rc<TestWorkingCopyBackupService>,
    pub working_copy_service: Rc<WorkingCopyService>,
    pub notification_service: Rc<TestNotificationService>,
    pub working_copy_editor_service: Rc<TestWorkingCopyEditorService>,
    pub editor_service: Rc<TestEditorService>,
    pub elevated_file_service: Rc<BrowserElevatedFileService>,
    pub progress_service: Rc<TestProgressService>,
    pub lifecycle_service: Rc<TestLifecycleService>,
    pub label_service: Rc<LabelService>,
    pub uri_identity_service: Rc<UriIdentityService>,
    created: RefCell<Vec<Rc<TestStoredFileWorkingCopy>>>,
}

impl TestServiceAccessor {
    /// `workbenchInstantiationService(undefined, disposables)` e `instantiationService.createInstance(TestServiceAccessor)`.
    pub fn new(disposables: &Rc<DisposableStore>) -> Self {
        let test_configuration_service = TestConfigurationService::new(Some(Dynamic::object([(
            "files",
            Dynamic::object([("participants", Dynamic::object([("timeout", Dynamic::from(60000))]))]),
        )])));
        disposables.add_rc(test_configuration_service.on_did_change_configuration_emitter.clone());
        let configuration_service: Rc<dyn IConfigurationService> = test_configuration_service;

        let log_service = NullLogService::shared();
        let progress_service = Rc::new(TestProgressService);
        let context_key_service = disposables.add_rc(MockContextKeyService::new());
        let file_service = disposables.add_rc(TestFileService::new());
        let file_service_contract: Rc<dyn IFileService> = file_service.clone();
        let uri_identity_service = disposables.add_rc(UriIdentityService::create((), (file_service_contract.clone(),)));
        let context_service = TestContextService::new(None, None);
        let files_configuration_service = disposables.add_rc(TestFilesConfigurationService::create(
            (),
            (
                context_key_service,
                configuration_service.clone(),
                context_service.clone(),
                TestEnvironmentService::new(),
                uri_identity_service.clone(),
                file_service_contract.clone(),
                Rc::new(TestMarkerService),
                TestTextResourceConfigurationService::new(configuration_service),
            ),
        ));
        let working_copy_service = disposables.add_rc(WorkingCopyService::new());
        let working_copy_file_service = disposables.add_rc(TestWorkingCopyFileService::new(
            log_service.clone(),
            progress_service.clone(),
            file_service_contract,
            working_copy_service.clone(),
            uri_identity_service.clone(),
        ));
        let storage = disposables.add_rc(TestStorageService::new());
        let storage_service: Rc<dyn StorageService> = Rc::new(storage);
        let lifecycle_service = disposables.add_rc(TestLifecycleService::new());
        let label_service = disposables.add_rc(LabelService::create(
            (),
            (
                TestEnvironmentService::new(),
                context_service,
                TestPathService::new(None, None),
                Rc::new(TestRemoteAgentService),
                storage_service,
                lifecycle_service.clone(),
                uri_identity_service.clone(),
            ),
        ));

        Self {
            file_service,
            log_service,
            working_copy_file_service,
            files_configuration_service,
            working_copy_backup_service: disposables.add_rc(TestWorkingCopyBackupService::new()),
            working_copy_service,
            notification_service: Rc::new(TestNotificationService),
            working_copy_editor_service: disposables.add_rc(TestWorkingCopyEditorService::new()),
            editor_service: Rc::new(TestEditorService),
            elevated_file_service: Rc::new(BrowserElevatedFileService),
            progress_service,
            lifecycle_service,
            label_service,
            uri_identity_service,
            created: RefCell::new(Vec::new()),
        }
    }

    /// Os 11 serviços injetados do construtor da `StoredFileWorkingCopy`.
    pub fn services(&self) -> StoredFileWorkingCopyServices {
        StoredFileWorkingCopyServices {
            file_service: self.file_service.clone(),
            log_service: self.log_service.clone(),
            working_copy_file_service: self.working_copy_file_service.clone(),
            files_configuration_service: self.files_configuration_service.clone(),
            working_copy_backup_service: self.working_copy_backup_service.clone(),
            working_copy_service: self.working_copy_service.clone(),
            notification_service: self.notification_service.clone(),
            working_copy_editor_service: self.working_copy_editor_service.clone(),
            editor_service: self.editor_service.clone(),
            elevated_file_service: self.elevated_file_service.clone(),
            progress_service: self.progress_service.clone(),
        }
    }

    /// `new StoredFileWorkingCopy('testStoredFileWorkingCopyType', resource, basename(resource), factory, options =>
    /// workingCopy.resolve(options), accessor.fileService, ...)`. O resolvedor externo segura a working copy por `Weak`
    /// (no original o coletor de lixo resolve o ciclo).
    pub fn create_stored_file_working_copy(&self, resource: &URI, factory: Rc<TestStoredFileWorkingCopyModelFactory>) -> Rc<TestStoredFileWorkingCopy> {
        let slot: Rc<RefCell<Weak<TestStoredFileWorkingCopy>>> = Rc::default();

        let resolver_slot = slot.clone();
        let external_resolver: StoredFileWorkingCopyResolver = Rc::new(move |options: Option<StoredFileWorkingCopyResolveOptions>| {
            let working_copy = resolver_slot.borrow().upgrade();
            match working_copy {
                Some(working_copy) => working_copy.resolve_stored(options),
                None => Promise::resolved(()),
            }
        });

        let working_copy = StoredFileWorkingCopy::new(
            STORED_FILE_WORKING_COPY_TYPE_ID.to_string(),
            resource.clone(),
            ext_uri().basename(resource, None),
            factory as Rc<dyn IStoredFileWorkingCopyModelFactory<TestStoredFileWorkingCopyModel>>,
            external_resolver,
            self.services(),
        );
        *slot.borrow_mut() = Rc::downgrade(&working_copy);
        self.created.borrow_mut().push(working_copy.clone());

        working_copy
    }

    /// `for (const workingCopy of accessor.workingCopyService.workingCopies) workingCopy.dispose()`.
    pub fn dispose_registered_working_copies(&self) {
        for registered in self.working_copy_service.working_copies() {
            let created = self.created.borrow().iter().find(|candidate| std::ptr::addr_eq(Rc::as_ptr(candidate), Rc::as_ptr(&registered))).cloned();
            if let Some(working_copy) = created {
                working_copy.dispose();
            }
        }
    }
}

//#endregion
