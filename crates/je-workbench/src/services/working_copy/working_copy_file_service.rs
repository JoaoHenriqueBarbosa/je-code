//! Porte de `src/vs/workbench/services/workingCopy/common/workingCopyFileService.ts`: o `WorkingCopyFileService`, que
//! executa as operações de arquivo (`create`, `createFolder`, `move`, `copy`, `delete`) com suporte a working copies
//! (reverte as sujas que ficariam órfãs) e dispara os eventos `onWillRun...`, `onDidFail...` e `onDidRun...` em cada
//! estado da operação, mais os participantes de operação de arquivo e de salvar de working copies de arquivo armazenado.
//!
//! Os contratos (`IWorkingCopyFileService`, `WorkingCopyFileEvent`, `ICreateFileOperation`, `IMoveOperation`,
//! `IDeleteOperation`, `IWorkingCopyFileOperationParticipant`, `WorkingCopyProvider`...) ficam em
//! `services/textfile/collaborators.rs`, onde os consumidores já os leem, e o `IStoredFileWorkingCopySaveParticipant` em
//! `stored_file_working_copy_save_participant.rs`.
//!
//! Divergências:
//! - **Injeção.** O construtor recebe os serviços (`IFileService`, `IWorkingCopyService`, `IUriIdentityService`,
//!   `ILogService`, `IConfigurationService` e `IProgressService`) no lugar do `IInstantiationService`, que só criava os dois
//!   participantes ([`WorkingCopyFileOperationParticipant`] e [`StoredFileWorkingCopySaveParticipant`]); o
//!   `registerSingleton(..., InstantiationType.Delayed)` é [`register`], `Eager` porque o identificador do serviço não tem
//!   proxy de instanciação atrasada. O serviço guarda um `Weak` de si (`Rc::new_cyclic`), que as operações `async`
//!   promovem.
//! - **`AsyncEmitter.fireAsync`.** Os eventos são `Emitter<WorkingCopyFileEvent>` e o `waitUntil` do original é o
//!   [`WorkingCopyFileEvent::wait_until`], que guarda a promessa no evento: `fire_async` dispara o evento para todos os
//!   ouvintes e depois espera as promessas que eles entregaram (o original espera as de cada ouvinte antes de chamar o
//!   próximo). Uma promessa rejeitada é reportada como erro inesperado e não interrompe a operação, como no `AsyncEmitter`.
//!   O `token` do `fireAsync` é sempre `CancellationToken.None` no original ("intentional"), então não é repassado.
//! - **`Promises.settled`** é [`promises::settled`]. As validações (`canCreateFile`, `canMove`, `canCopy`, `canDelete`)
//!   devolvem `Promise<Result<(), Error>>` (o `Error | true` do original), e o primeiro `Err` é o erro lançado.
//! - **`ICreateFileOperation` e `ICreateOperation`.** `createFolder` converte o [`CreateOperation`] em
//!   [`CreateFileOperation`] sem `contents` (o `(operation as ICreateFileOperation).contents` do original é `undefined`
//!   nele). O `contents` é o [`EncodedContent`] (`VSBuffer | VSBufferReadable`) que o `TextFileService.create` produz.
//! - **`IFileStatWithMetadata[]`.** `create` e `createFolder` devolvem os stats, `move` e `copy` também, `delete` nada.
//! - **`Set<IWorkingCopy>`** do `getDirty` compara por identidade do objeto: aqui é a identidade do `Rc` ([`id_of`]).
//! - **`insert(array, element)`** do `registerWorkingCopyProvider` é o `push` no `Vec` mais o `toDisposable` que remove o
//!   provedor por identidade (`Rc::ptr_eq`).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::{async_fn, promises};
use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableId, DisposableRef, IDisposable, id_of, to_disposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::{Injectable, InstantiationType, register_singleton, service_impl};
use je_platform::configuration::service::IConfigurationService;
use je_platform::files::file_service::FileContentSource;
use je_platform::files::files::{CreateFileOptions, FileOperation, FileStatWithMetadata, PartialFileDeleteOptions};
use je_platform::files::i_file_service::IFileService;
use je_platform::log::LogServiceRef;
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use crate::common::editor::RevertOptions;
use crate::services::textfile::collaborators::{
    CopyOperation, CreateFileOperation, CreateOperation, DeleteOperation, FileOperationUndoRedoInfo, I_WORKING_COPY_FILE_SERVICE, IProgress, IProgressService,
    IWorkingCopyFileOperationParticipant, IWorkingCopyFileService, MoveOperation, ProgressStep, StoredFileWorkingCopySaveParticipantContext, WorkingCopyFileEvent,
    WorkingCopyFileEventFile, WorkingCopyProvider,
};
use crate::services::working_copy::stored_file_working_copy::IStoredFileWorkingCopy;
use crate::services::working_copy::stored_file_working_copy_save_participant::{IStoredFileWorkingCopySaveParticipant, StoredFileWorkingCopySaveParticipant};
use crate::services::working_copy::working_copy::IWorkingCopy;
use crate::services::working_copy::working_copy_file_operation_participant::WorkingCopyFileOperationParticipant;
use crate::services::working_copy::working_copy_service::IWorkingCopyService;

/// `WorkingCopyFileService`.
pub struct WorkingCopyFileService {
    this: Weak<WorkingCopyFileService>,
    base: Rc<Disposable>,

    //#region Events
    on_will_run_working_copy_file_operation: Rc<Emitter<WorkingCopyFileEvent>>,
    on_did_fail_working_copy_file_operation: Rc<Emitter<WorkingCopyFileEvent>>,
    on_did_run_working_copy_file_operation: Rc<Emitter<WorkingCopyFileEvent>>,
    //#endregion
    correlation_ids: Cell<usize>,

    file_service: Rc<dyn IFileService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,

    file_operation_participants: Rc<WorkingCopyFileOperationParticipant>,
    save_participants: Rc<StoredFileWorkingCopySaveParticipant>,
    working_copy_providers: Rc<RefCell<Vec<WorkingCopyProvider>>>,
}

impl WorkingCopyFileService {
    /// `constructor(@IFileService fileService, @IWorkingCopyService workingCopyService, @IInstantiationService
    /// instantiationService, @IUriIdentityService uriIdentityService)`, mais os serviços que os dois participantes leem.
    pub fn new(
        file_service: Rc<dyn IFileService>,
        working_copy_service: Rc<dyn IWorkingCopyService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
        log_service: LogServiceRef,
        configuration_service: Rc<dyn IConfigurationService>,
        progress_service: Rc<dyn IProgressService>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|this| {
            let base = Disposable::new();
            let on_will_run_working_copy_file_operation = Emitter::new();
            let on_did_fail_working_copy_file_operation = Emitter::new();
            let on_did_run_working_copy_file_operation = Emitter::new();
            base.register(on_will_run_working_copy_file_operation.clone());
            base.register(on_did_fail_working_copy_file_operation.clone());
            base.register(on_did_run_working_copy_file_operation.clone());

            let file_operation_participants = WorkingCopyFileOperationParticipant::new(log_service.clone(), configuration_service);
            base.register(file_operation_participants.clone());
            let save_participants = StoredFileWorkingCopySaveParticipant::new(log_service, progress_service);
            base.register(save_participants.clone());

            let working_copy_providers: Rc<RefCell<Vec<WorkingCopyProvider>>> = Rc::default();

            let service = Self {
                this: this.clone(),
                base,
                on_will_run_working_copy_file_operation,
                on_did_fail_working_copy_file_operation,
                on_did_run_working_copy_file_operation,
                correlation_ids: Cell::new(0),
                file_service: file_service.clone(),
                uri_identity_service: uri_identity_service.clone(),
                file_operation_participants,
                save_participants,
                working_copy_providers,
            };

            // register a default working copy provider that uses the working copy service
            let default_provider: WorkingCopyProvider = Rc::new(move |resource: &URI| {
                let ext_uri = uri_identity_service.ext_uri();
                let has_provider = file_service.has_provider(resource);

                working_copy_service
                    .working_copies()
                    .into_iter()
                    .filter(|working_copy| {
                        if has_provider {
                            // only check for parents if the resource can be handled
                            // by the file system where we then assume a folder like
                            // path structure
                            return ext_uri.is_equal_or_parent(&working_copy.resource(), resource, false, false);
                        }

                        ext_uri.is_equal(Some(&working_copy.resource()), Some(resource), false)
                    })
                    .collect()
            });
            service.base.register(service.register_provider(default_provider));

            service
        })
    }

    /// O `this` das operações `async`.
    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço de arquivos de working copy está vivo enquanto um método dele roda")
    }

    //#region File operations

    fn do_create_file_or_folder(&self, operations: Vec<CreateFileOperation>, is_file: bool, token: CancellationToken, undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>> {
        let this = self.rc();

        async_fn(async move {
            if operations.is_empty() {
                return Ok(Vec::new());
            }

            // validate create operation before starting
            if is_file {
                let validate_creates = promises::settled(
                    operations
                        .iter()
                        .map(|operation| this.file_service.can_create_file(&operation.resource, Some(CreateFileOptions { overwrite: operation.overwrite.unwrap_or(false) })))
                        .collect(),
                )
                .await?;
                if let Some(error) = validate_creates.into_iter().find_map(Result::err) {
                    return Err(error);
                }
            }

            // file operation participant
            let files: Vec<WorkingCopyFileEventFile> = operations.iter().map(|operation| WorkingCopyFileEventFile { source: None, target: operation.resource.clone() }).collect();
            this.run_file_operation_participants(files.clone(), FileOperation::Create, undo_info, token).await?;

            // before events
            let event = this.new_event(FileOperation::Create, files);
            Self::fire_async(&this.on_will_run_working_copy_file_operation, &event).await?;

            // now actually create on disk
            let stats = if is_file {
                promises::settled(
                    operations
                        .iter()
                        .map(|operation| {
                            let contents = operation.contents.clone().map(FileContentSource::from);
                            this.file_service.create_file(&operation.resource, contents, Some(CreateFileOptions { overwrite: operation.overwrite.unwrap_or(false) }))
                        })
                        .collect(),
                )
                .await
            } else {
                promises::settled(operations.iter().map(|operation| this.file_service.create_folder(&operation.resource)).collect()).await
            };
            let stats = match stats {
                Ok(stats) => stats,
                Err(error) => {
                    // error event
                    Self::fire_async(&this.on_did_fail_working_copy_file_operation, &event).await?;

                    return Err(error);
                }
            };

            // after event
            Self::fire_async(&this.on_did_run_working_copy_file_operation, &event).await?;

            Ok(stats)
        })
    }

    fn do_move_or_copy(&self, operations: Vec<MoveOperation>, is_move: bool, token: CancellationToken, undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>> {
        let this = self.rc();

        async_fn(async move {
            let mut stats = Vec::new();

            // validate move/copy operation before starting
            for operation in &operations {
                let (source, target, overwrite) = (&operation.file.source, &operation.file.target, operation.overwrite);
                let validate_move_or_copy = if is_move { this.file_service.can_move(source, target, overwrite).await? } else { this.file_service.can_copy(source, target, overwrite).await? };
                if let Err(error) = validate_move_or_copy {
                    return Err(error);
                }
            }

            // file operation participant
            let files: Vec<WorkingCopyFileEventFile> =
                operations.iter().map(|operation| WorkingCopyFileEventFile { source: Some(operation.file.source.clone()), target: operation.file.target.clone() }).collect();
            let file_operation = if is_move { FileOperation::Move } else { FileOperation::Copy };
            this.run_file_operation_participants(files.clone(), file_operation, undo_info, token).await?;

            // before event
            let event = this.new_event(file_operation, files);
            Self::fire_async(&this.on_will_run_working_copy_file_operation, &event).await?;

            let result: Result<(), Error> = async {
                for operation in &operations {
                    let (source, target, overwrite) = (&operation.file.source, &operation.file.target, operation.overwrite);

                    // if source and target are not equal, handle dirty working copies
                    // depending on the operation:
                    // - move: revert both source and target (if any)
                    // - copy: revert target (if any)
                    if !this.uri_identity_service.ext_uri().is_equal(Some(source), Some(target), false) {
                        let dirty_working_copies = if is_move {
                            let mut dirty = this.get_dirty(source);
                            dirty.extend(this.get_dirty(target));
                            dirty
                        } else {
                            this.get_dirty(target)
                        };
                        promises::settled(dirty_working_copies.iter().map(|dirty_working_copy| dirty_working_copy.revert(Some(soft_revert()))).collect()).await?;
                    }

                    // now we can rename the source to target via file operation
                    if is_move {
                        stats.push(this.file_service.move_(source, target, overwrite).await?);
                    } else {
                        stats.push(this.file_service.copy(source, target, overwrite).await?);
                    }
                }

                Ok(())
            }
            .await;
            if let Err(error) = result {
                // error event
                Self::fire_async(&this.on_did_fail_working_copy_file_operation, &event).await?;

                return Err(error);
            }

            // after event
            Self::fire_async(&this.on_did_run_working_copy_file_operation, &event).await?;

            Ok(stats)
        })
    }

    fn do_delete(&self, operations: Vec<DeleteOperation>, token: CancellationToken, undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<()> {
        let this = self.rc();

        async_fn(async move {
            // validate delete operation before starting
            for operation in &operations {
                let validate_delete = this.file_service.can_delete(&operation.resource, Some(delete_options(operation))).await?;
                if let Err(error) = validate_delete {
                    return Err(error);
                }
            }

            // file operation participant
            let files: Vec<WorkingCopyFileEventFile> = operations.iter().map(|operation| WorkingCopyFileEventFile { source: None, target: operation.resource.clone() }).collect();
            this.run_file_operation_participants(files.clone(), FileOperation::Delete, undo_info, token).await?;

            // before events
            let event = this.new_event(FileOperation::Delete, files);
            Self::fire_async(&this.on_will_run_working_copy_file_operation, &event).await?;

            // check for any existing dirty working copies for the resource
            // and do a soft revert before deleting to be able to close
            // any opened editor with these working copies
            for operation in &operations {
                let dirty_working_copies = this.get_dirty(&operation.resource);
                promises::settled(dirty_working_copies.iter().map(|dirty_working_copy| dirty_working_copy.revert(Some(soft_revert()))).collect()).await?;
            }

            // now actually delete from disk
            let result: Result<(), Error> = async {
                for operation in &operations {
                    this.file_service.del(&operation.resource, Some(delete_options(operation))).await?;
                }

                Ok(())
            }
            .await;
            if let Err(error) = result {
                // error event
                Self::fire_async(&this.on_did_fail_working_copy_file_operation, &event).await?;

                return Err(error);
            }

            // after event
            Self::fire_async(&this.on_did_run_working_copy_file_operation, &event).await
        })
    }

    /// `{ correlationId: this.correlationIds++, operation, files }`.
    fn new_event(&self, operation: FileOperation, files: Vec<WorkingCopyFileEventFile>) -> WorkingCopyFileEvent {
        let correlation_id = self.correlation_ids.get();
        self.correlation_ids.set(correlation_id + 1);

        WorkingCopyFileEvent::new(correlation_id, operation, files)
    }

    /// `emitter.fireAsync(event, CancellationToken.None /* intentional: we currently only forward cancellation to
    /// participants */)`.
    fn fire_async(emitter: &Emitter<WorkingCopyFileEvent>, event: &WorkingCopyFileEvent) -> Promise<()> {
        emitter.fire(event.clone());

        Promise::all_settled(event.take_joiners()).then(|results| {
            for result in results {
                if let Err(error) = result {
                    errors::on_unexpected_error(&error);
                }
            }
        })
    }

    //#endregion

    //#region File operation participants

    fn run_file_operation_participants(&self, files: Vec<WorkingCopyFileEventFile>, operation: FileOperation, undo_info: Option<FileOperationUndoRedoInfo>, token: CancellationToken) -> Promise<()> {
        self.file_operation_participants.participate(files, operation, undo_info, token)
    }

    //#endregion
}

/// `{ soft: true }` das opções de reverter.
fn soft_revert() -> RevertOptions {
    RevertOptions { soft: Some(true), ..RevertOptions::default() }
}

/// `{ recursive: operation.recursive, useTrash: operation.useTrash }`.
fn delete_options(operation: &DeleteOperation) -> PartialFileDeleteOptions {
    PartialFileDeleteOptions { recursive: operation.recursive, use_trash: operation.use_trash, ..PartialFileDeleteOptions::default() }
}

impl IDisposable for WorkingCopyFileService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IWorkingCopyFileService for WorkingCopyFileService {
    fn on_will_run_working_copy_file_operation(&self) -> Event<WorkingCopyFileEvent> {
        self.on_will_run_working_copy_file_operation.event()
    }

    fn on_did_fail_working_copy_file_operation(&self) -> Event<WorkingCopyFileEvent> {
        self.on_did_fail_working_copy_file_operation.event()
    }

    fn on_did_run_working_copy_file_operation(&self) -> Event<WorkingCopyFileEvent> {
        self.on_did_run_working_copy_file_operation.event()
    }

    //#region File operations

    fn create(&self, operations: Vec<CreateFileOperation>, token: CancellationToken, undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>> {
        self.do_create_file_or_folder(operations, true, token, undo_info)
    }

    fn create_folder(&self, operations: Vec<CreateOperation>, token: CancellationToken, undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>> {
        let operations = operations.into_iter().map(|CreateOperation { resource, overwrite }| CreateFileOperation { resource, contents: None, overwrite }).collect();

        self.do_create_file_or_folder(operations, false, token, undo_info)
    }

    fn move_(&self, operations: Vec<MoveOperation>, token: CancellationToken, undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>> {
        self.do_move_or_copy(operations, true, token, undo_info)
    }

    fn copy(&self, operations: Vec<CopyOperation>, token: CancellationToken, undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>> {
        self.do_move_or_copy(operations, false, token, undo_info)
    }

    fn delete(&self, operations: Vec<DeleteOperation>, token: CancellationToken, undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<()> {
        self.do_delete(operations, token, undo_info)
    }

    //#endregion

    //#region File operation participants

    fn add_file_operation_participant(&self, participant: Rc<dyn IWorkingCopyFileOperationParticipant>) -> DisposableRef {
        self.file_operation_participants.add_file_operation_participant(participant)
    }

    //#endregion

    //#region Save participants (stored file working copies only)

    fn has_save_participants(&self) -> bool {
        !self.save_participants.is_empty()
    }

    fn add_save_participant(&self, participant: Rc<dyn IStoredFileWorkingCopySaveParticipant>) -> DisposableRef {
        self.save_participants.add_save_participant(participant)
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

    //#endregion

    //#region Path related

    fn register_working_copy_provider(&self, provider: WorkingCopyProvider) -> DisposableRef {
        self.register_provider(provider)
    }

    fn get_dirty(&self, resource: &URI) -> Vec<Rc<dyn IWorkingCopy>> {
        let mut seen: Vec<DisposableId> = Vec::new();
        let mut dirty_working_copies = Vec::new();

        let providers: Vec<WorkingCopyProvider> = self.working_copy_providers.borrow().clone();
        for provider in providers {
            for working_copy in provider(resource) {
                if working_copy.is_dirty() && !seen.contains(&id_of(&working_copy)) {
                    seen.push(id_of(&working_copy));
                    dirty_working_copies.push(working_copy);
                }
            }
        }

        dirty_working_copies
    }

    //#endregion
}

impl WorkingCopyFileService {
    /// `registerWorkingCopyProvider(provider)`: `insert(this.workingCopyProviders, provider)` e o `toDisposable(remove)`.
    fn register_provider(&self, provider: WorkingCopyProvider) -> DisposableRef {
        self.working_copy_providers.borrow_mut().push(provider.clone());

        let providers = self.working_copy_providers.clone();
        to_disposable(move || {
            let mut providers = providers.borrow_mut();
            if let Some(index) = providers.iter().position(|candidate| Rc::ptr_eq(candidate, &provider)) {
                providers.remove(index);
            }
        })
    }
}

impl Injectable for WorkingCopyFileService {
    type Args = ();
    type Deps = (Rc<dyn IFileService>, Rc<dyn IWorkingCopyService>, Rc<dyn IUriIdentityService>, LogServiceRef, Rc<dyn IConfigurationService>, Rc<dyn IProgressService>);

    fn create(_: (), (file_service, working_copy_service, uri_identity_service, log_service, configuration_service, progress_service): Self::Deps) -> Rc<Self> {
        Self::new(file_service, working_copy_service, uri_identity_service, log_service, configuration_service, progress_service)
    }
}

service_impl!(WorkingCopyFileService => dyn IWorkingCopyFileService, disposable);

/// `registerSingleton(IWorkingCopyFileService, WorkingCopyFileService, InstantiationType.Delayed)`.
pub fn register() {
    register_singleton::<WorkingCopyFileService>(I_WORKING_COPY_FILE_SERVICE, InstantiationType::Eager);
}
