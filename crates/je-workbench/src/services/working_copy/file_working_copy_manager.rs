//! Porte de `src/vs/workbench/services/workingCopy/common/fileWorkingCopyManager.ts`: o `FileWorkingCopyManager`, que junta
//! o gerenciador de working copies armazenadas (`stored`) e o de working copies sem título (`untitled`), resolve cada
//! recurso no gerenciador certo, implementa o "Salvar como" de arquivos (de armazenados e de sem título, com as
//! confirmações de somente leitura e de sobrescrita) e fornece as decorações de somente leitura e de arquivo apagado.
//!
//! Divergências:
//! - **Injeção.** Os 21 parâmetros do construtor são o `workingCopyTypeId`, as duas fábricas de modelo e o
//!   [`FileWorkingCopyManagerServices`]: os 11 serviços da working copy armazenada (os mesmos que o gerenciador lê, no
//!   [`StoredFileWorkingCopyServices`]) e os que só este gerenciador lê (`ILifecycleService`, `ILabelService`,
//!   `IUriIdentityService`, `IFileDialogService`, `IPathService`, `IWorkbenchEnvironmentService`, `IDialogService` e
//!   `IDecorationsService`).
//! - **Genéricos e união.** O gerenciador é genérico nos dois modelos (`S` e `U`), como no original. A união
//!   `IUntitledFileWorkingCopy<U> | IStoredFileWorkingCopy<S>` é o enum [`FileWorkingCopy`] (`instanceof` é o `match` dele).
//! - **Sobrecargas de `resolve`.** As quatro sobrecargas viram o argumento [`FileWorkingCopyResolveArg`] (um recurso com as
//!   opções do gerenciador armazenado, ou as opções de um sem título) e o resultado é a união [`FileWorkingCopy`]: no
//!   original o tipo estático de cada sobrecarga escolhe o ramo, aqui quem chama o escolhe com
//!   [`FileWorkingCopy::into_stored`] e [`FileWorkingCopy::into_untitled`].
//! - **`saveAs`.** As duas sobrecargas são [`FileWorkingCopyManager::save_as`], com o alvo ausente como `None` e as opções
//!   como [`FileWorkingCopySaveAsOptions`] (`IFileWorkingCopySaveAsOptions extends ISaveOptions`: o nível base em `base`).
//! - **Interface base.** `IFileWorkingCopyManager extends IBaseFileWorkingCopyManager`, mas a classe não herda de
//!   `BaseFileWorkingCopyManager` (guarda os dois gerenciadores): o trait [`IBaseFileWorkingCopyManager`] exige a parte
//!   base, que não existe aqui. Os membros dele (`onDidCreate`, `workingCopies`, `get`, `destroy`) são métodos próprios,
//!   com a união como tipo da working copy.
//! - **Herança.** `extends Disposable` é um [`Disposable`] próprio (campo `store`).
//! - **Fonte de salvar.** `FILE_WORKING_COPY_SAVE_CREATE_SOURCE` e `FILE_WORKING_COPY_SAVE_REPLACE_SOURCE` são registrados no
//!   `SaveSourceRegistry` na hora do uso (como os do `AbstractTextFileService`).
//! - **Decorações.** O provedor guarda o gerenciador armazenado por `Weak` (o gerenciador guarda o provedor por meio do
//!   serviço de decorações, e no JavaScript o coletor de lixo desfaz o ciclo). `listErrorForeground` é o id da cor e
//!   `Codicon.lockSmall` é o id do codicon, como no `AbstractTextFileService`.
//! - **Delegado de salvar.** O fecho que o gerenciador sem título recebe segura o gerenciador por `Weak`, para o ciclo `Rc`
//!   não vazar.

use std::rc::{Rc, Weak};

use je_base::async_::{async_fn, promises};
use je_base::buffer::VSBufferReadableStream;
use je_base::cancellation::CancellationToken;
use je_base::event::{self, Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::promise::{MaybePromise, Promise};
use je_base::resources::to_local_resource;
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_platform::dialogs::dialogs::{BaseDialogOptions, Confirmation, DialogDetail, DialogType, DialogTypeOrSeverity, IDialogService, IFileDialogService};
use je_platform::label::label::ILabelService;
use je_platform::log::LogMessage;
use je_platform::path::path_service::IPathService;
use je_platform::resources::{ext_uri, join_path};
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use crate::common::editor::{SaveOptions, with_save_source_registry};
use crate::services::decorations::decorations::{DecorationData, DecorationLetter, IDecorationsProvider, IDecorationsService};
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::files_configuration::files_configuration_service::{ReadonlyUpdate, SingleReadonlyUpdate};
use crate::services::lifecycle::lifecycle::ILifecycleService;
use crate::services::textfile::collaborators::{CreateFileOperation, MoveOperation, RequiredSourceTargetPair};
use crate::services::working_copy::abstract_file_working_copy_manager::IBaseFileWorkingCopyManager;
use crate::services::working_copy::file_working_copy::{IFileWorkingCopy, IFileWorkingCopyModel, SnapshotContext};
use crate::services::working_copy::resource_working_copy::IResourceWorkingCopy;
use crate::services::working_copy::stored_file_working_copy::{
    IStoredFileWorkingCopy, IStoredFileWorkingCopyModel, IStoredFileWorkingCopyModelFactory, StoredFileWorkingCopy, StoredFileWorkingCopySaveAsOptions, StoredFileWorkingCopyServices,
    StoredFileWorkingCopyState,
};
use crate::services::working_copy::stored_file_working_copy_manager::{IStoredFileWorkingCopyManager, StoredFileWorkingCopyManager, StoredFileWorkingCopyManagerResolveOptions};
use crate::services::working_copy::untitled_file_working_copy::{
    IUntitledFileWorkingCopy, IUntitledFileWorkingCopyModel, IUntitledFileWorkingCopyModelFactory, UntitledFileWorkingCopy, UntitledFileWorkingCopySaveDelegate,
};
use crate::services::working_copy::untitled_file_working_copy_manager::{
    IUntitledFileWorkingCopyManager, NewOrExistingUntitledFileWorkingCopyOptions, NewUntitledFileWorkingCopyOptions, UntitledFileWorkingCopyManager,
    UntitledFileWorkingCopyResolveOptions,
};
use crate::services::working_copy::working_copy::IWorkingCopy;

/// `listErrorForeground`: o id da cor `list.errorForeground` do registro de cores.
const LIST_ERROR_FOREGROUND: &str = "list.errorForeground";

/// O id do codicon `Codicon.lockSmall`.
const CODICON_LOCK_SMALL: &str = "lock-small";

/// `FileWorkingCopyManager.FILE_WORKING_COPY_SAVE_CREATE_SOURCE`.
fn save_create_source() -> String {
    with_save_source_registry(|registry| registry.register_source("fileWorkingCopyCreate.source", &localize("fileWorkingCopyCreate.source", "File Created", &[])))
}

/// `FileWorkingCopyManager.FILE_WORKING_COPY_SAVE_REPLACE_SOURCE`.
fn save_replace_source() -> String {
    with_save_source_registry(|registry| registry.register_source("fileWorkingCopyReplace.source", &localize("fileWorkingCopyReplace.source", "File Replaced", &[])))
}

/// `IUntitledFileWorkingCopy<U> | IStoredFileWorkingCopy<S>`.
pub enum FileWorkingCopy<S: IStoredFileWorkingCopyModel + 'static, U: IUntitledFileWorkingCopyModel + 'static> {
    Stored(Rc<StoredFileWorkingCopy<S>>),
    Untitled(Rc<UntitledFileWorkingCopy<U>>),
}

impl<S: IStoredFileWorkingCopyModel + 'static, U: IUntitledFileWorkingCopyModel + 'static> Clone for FileWorkingCopy<S, U> {
    fn clone(&self) -> Self {
        match self {
            FileWorkingCopy::Stored(working_copy) => FileWorkingCopy::Stored(working_copy.clone()),
            FileWorkingCopy::Untitled(working_copy) => FileWorkingCopy::Untitled(working_copy.clone()),
        }
    }
}

impl<S: IStoredFileWorkingCopyModel + 'static, U: IUntitledFileWorkingCopyModel + 'static> FileWorkingCopy<S, U> {
    /// O `IFileWorkingCopy` (não genérico) por trás da união.
    fn as_file_working_copy(&self) -> &dyn IFileWorkingCopy {
        match self {
            FileWorkingCopy::Stored(working_copy) => &**working_copy,
            FileWorkingCopy::Untitled(working_copy) => &**working_copy,
        }
    }

    /// `workingCopy instanceof StoredFileWorkingCopy`.
    pub fn as_stored(&self) -> Option<&Rc<StoredFileWorkingCopy<S>>> {
        match self {
            FileWorkingCopy::Stored(working_copy) => Some(working_copy),
            FileWorkingCopy::Untitled(_) => None,
        }
    }

    /// `workingCopy instanceof UntitledFileWorkingCopy`.
    pub fn as_untitled(&self) -> Option<&Rc<UntitledFileWorkingCopy<U>>> {
        match self {
            FileWorkingCopy::Stored(_) => None,
            FileWorkingCopy::Untitled(working_copy) => Some(working_copy),
        }
    }

    /// A working copy armazenada, para quem sabe que o recurso não é `untitled`.
    pub fn into_stored(self) -> Option<Rc<StoredFileWorkingCopy<S>>> {
        match self {
            FileWorkingCopy::Stored(working_copy) => Some(working_copy),
            FileWorkingCopy::Untitled(_) => None,
        }
    }

    /// A working copy sem título, para quem sabe que o recurso é `untitled`.
    pub fn into_untitled(self) -> Option<Rc<UntitledFileWorkingCopy<U>>> {
        match self {
            FileWorkingCopy::Stored(_) => None,
            FileWorkingCopy::Untitled(working_copy) => Some(working_copy),
        }
    }

    /// `a === b`: a identidade do objeto.
    pub fn is_same(&self, other: &Self) -> bool {
        match (self, other) {
            (FileWorkingCopy::Stored(a), FileWorkingCopy::Stored(b)) => Rc::ptr_eq(a, b),
            (FileWorkingCopy::Untitled(a), FileWorkingCopy::Untitled(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }

    pub fn resource(&self) -> URI {
        self.as_file_working_copy().resource()
    }

    pub fn name(&self) -> String {
        self.as_file_working_copy().name()
    }

    pub fn is_resolved(&self) -> bool {
        self.as_file_working_copy().is_resolved()
    }

    pub fn is_dirty(&self) -> bool {
        self.as_file_working_copy().is_dirty()
    }

    pub fn revert(&self) -> Promise<()> {
        self.as_file_working_copy().revert(None)
    }

    pub fn dispose(&self) {
        self.as_file_working_copy().dispose();
    }

    /// O `model` (não tipado) da working copy.
    pub fn model(&self) -> Option<Rc<dyn IFileWorkingCopyModel>> {
        self.as_file_working_copy().model()
    }
}

/// O argumento de `resolve`: `URI` com opções (armazenada, ou sem título quando o esquema é `untitled`), ou as opções de uma
/// working copy sem título nova.
pub enum FileWorkingCopyResolveArg {
    Resource { resource: URI, options: Option<StoredFileWorkingCopyManagerResolveOptions> },
    Untitled(Option<UntitledFileWorkingCopyResolveOptions>),
}

impl FileWorkingCopyResolveArg {
    /// `resolve(resource, options?)`.
    pub fn resource(resource: &URI, options: Option<StoredFileWorkingCopyManagerResolveOptions>) -> Self {
        FileWorkingCopyResolveArg::Resource { resource: resource.clone(), options }
    }

    /// `resolve(options?)` com qualquer uma das três opções de uma working copy sem título.
    pub fn untitled(options: Option<impl Into<UntitledFileWorkingCopyResolveOptions>>) -> Self {
        FileWorkingCopyResolveArg::Untitled(options.map(Into::into))
    }
}

/// `IFileWorkingCopySaveAsOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileWorkingCopySaveAsOptions {
    /// O `ISaveOptions` de que herda.
    pub base: SaveOptions,
    /// Optional target resource to suggest to the user in case no target resource is provided to save to.
    pub suggested_target: Option<URI>,
}

/// Os serviços que o `FileWorkingCopyManager` recebe por injeção (o `workingCopyTypeId` e as duas fábricas são argumentos).
#[derive(Clone)]
pub struct FileWorkingCopyManagerServices {
    /// Os 11 serviços da working copy armazenada, que o gerenciador também lê (`IFileService`, `ILogService`,
    /// `IWorkingCopyFileService`, `IFilesConfigurationService`, `IWorkingCopyBackupService`...).
    pub services: StoredFileWorkingCopyServices,
    pub lifecycle_service: Rc<dyn ILifecycleService>,
    pub label_service: Rc<dyn ILabelService>,
    pub uri_identity_service: Rc<dyn IUriIdentityService>,
    pub file_dialog_service: Rc<dyn IFileDialogService>,
    pub path_service: Rc<dyn IPathService>,
    pub environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    pub dialog_service: Rc<dyn IDialogService>,
    pub decorations_service: Rc<dyn IDecorationsService>,
}

/// `FileWorkingCopyManager<S, U>`.
pub struct FileWorkingCopyManager<S: IStoredFileWorkingCopyModel + 'static, U: IUntitledFileWorkingCopyModel + 'static> {
    this: Weak<FileWorkingCopyManager<S, U>>,
    /// O `_register(...)` da classe.
    store: Rc<Disposable>,

    /// Provides access to the manager for stored file working copies.
    pub stored: Rc<StoredFileWorkingCopyManager<S>>,
    /// Provides access to the manager for untitled file working copies.
    pub untitled: Rc<UntitledFileWorkingCopyManager<U>>,

    on_did_create: Event<FileWorkingCopy<S, U>>,

    services: FileWorkingCopyManagerServices,
}

impl<S: IStoredFileWorkingCopyModel + 'static, U: IUntitledFileWorkingCopyModel + 'static> FileWorkingCopyManager<S, U> {
    /// `constructor(workingCopyTypeId, storedWorkingCopyModelFactory, untitledWorkingCopyModelFactory, ...services)`.
    pub fn new(
        working_copy_type_id: String,
        stored_working_copy_model_factory: Rc<dyn IStoredFileWorkingCopyModelFactory<S>>,
        untitled_working_copy_model_factory: Rc<dyn IUntitledFileWorkingCopyModelFactory<U>>,
        services: FileWorkingCopyManagerServices,
    ) -> Rc<Self> {
        let this = Rc::new_cyclic(|this: &Weak<FileWorkingCopyManager<S, U>>| {
            let store = Disposable::new();

            // Stored file working copies manager
            let stored = StoredFileWorkingCopyManager::new(
                working_copy_type_id.clone(),
                stored_working_copy_model_factory,
                services.services.clone(),
                services.lifecycle_service.clone(),
                services.label_service.clone(),
                services.uri_identity_service.clone(),
            );
            store.register(stored.clone());

            // Untitled file working copies manager
            let weak_manager = this.clone();
            let save_delegate: UntitledFileWorkingCopySaveDelegate<U> = Rc::new(move |working_copy: Rc<UntitledFileWorkingCopy<U>>, options: Option<SaveOptions>| {
                let Some(manager) = weak_manager.upgrade() else {
                    return Promise::resolved(false);
                };

                async_fn(async move {
                    let result = manager.save_as(&working_copy.resource(), None, options.map(|base| FileWorkingCopySaveAsOptions { base, suggested_target: None })).await?;

                    Ok(result.is_some())
                })
            });
            let untitled = UntitledFileWorkingCopyManager::new(
                working_copy_type_id,
                untitled_working_copy_model_factory,
                save_delegate,
                services.services.file_service.clone(),
                services.label_service.clone(),
                services.services.log_service.clone(),
                services.services.working_copy_backup_service.clone(),
                services.services.working_copy_service.clone(),
            );
            store.register(untitled.clone());

            // Events
            let on_did_create = event::any(vec![
                stored.on_did_create().map(|working_copy| FileWorkingCopy::Stored(working_copy.clone()), None),
                untitled.on_did_create().map(|working_copy| FileWorkingCopy::Untitled(working_copy.clone()), None),
            ]);

            Self { this: this.clone(), store, stored, untitled, on_did_create, services }
        });

        // Decorations
        this.provide_decorations();

        this
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o gerenciador está vivo enquanto é usado")
    }

    /// An event for when a file working copy was created.
    pub fn on_did_create(&self) -> Event<FileWorkingCopy<S, U>> {
        self.on_did_create.clone()
    }

    //#region decorations

    fn provide_decorations(&self) {
        // File working copy decorations
        let provider = FileWorkingCopyDecorationsProvider::new(&self.stored);
        self.store.register(provider.clone());

        self.store.register(self.services.decorations_service.register_decorations_provider(provider));
    }

    //#endregion

    //#region get / get all

    /// `workingCopies`.
    pub fn working_copies(&self) -> Vec<FileWorkingCopy<S, U>> {
        self.stored
            .working_copies()
            .into_iter()
            .map(FileWorkingCopy::Stored)
            .chain(self.untitled.working_copies().into_iter().map(FileWorkingCopy::Untitled))
            .collect()
    }

    /// `get(resource)`.
    pub fn get(&self, resource: &URI) -> Option<FileWorkingCopy<S, U>> {
        self.stored.get(resource).map(FileWorkingCopy::Stored).or_else(|| self.untitled.get(resource).map(FileWorkingCopy::Untitled))
    }

    //#endregion

    //#region resolve

    /// `resolve(...)`: um `URI` de esquema `untitled` resolve pelo gerenciador sem título, qualquer outro pelo armazenado; as
    /// opções de uma working copy sem título nova resolvem pelo gerenciador sem título.
    pub fn resolve(&self, arg: FileWorkingCopyResolveArg) -> Promise<FileWorkingCopy<S, U>> {
        match arg {
            FileWorkingCopyResolveArg::Resource { resource, options } => {
                // Untitled: via untitled manager
                if *resource.scheme() == schemas::UNTITLED {
                    let options = NewOrExistingUntitledFileWorkingCopyOptions {
                        options: NewUntitledFileWorkingCopyOptions::default(),
                        untitled_resource: resource,
                        is_scratchpad: None,
                    };

                    self.untitled.resolve(Some(options.into())).then(FileWorkingCopy::Untitled)
                }
                // else: via stored file manager
                else {
                    self.stored.resolve(&resource, options).then(FileWorkingCopy::Stored)
                }
            }
            FileWorkingCopyResolveArg::Untitled(options) => self.untitled.resolve(options).then(FileWorkingCopy::Untitled),
        }
    }

    //#endregion

    //#region Save

    /// Implements "Save As" for file based working copies. The API is `URI` based because it works even without resolved
    /// file working copies. If a file working copy exists for any given `URI`, the implementation will deal with them
    /// properly (e.g. dirty contents of the source will be written to the target and the source will be reverted).
    ///
    /// Note: it is possible that the returned file working copy has a different `URI` than the `target` that was passed in.
    /// Based on URI identity, the file working copy may chose to return an existing file working copy with different casing
    /// to respect file systems that are case insensitive.
    ///
    /// Note: Callers must `dispose` the working copy when no longer needed.
    ///
    /// Note: Untitled file working copies are being disposed when saved.
    ///
    /// - `target`: the optional target resource to save to. if not defined, the user will be asked for input.
    ///
    /// Resolves to the target stored working copy that was saved to or `None` in case of cancellation.
    pub fn save_as(&self, source: &URI, target: Option<&URI>, options: Option<FileWorkingCopySaveAsOptions>) -> Promise<Option<Rc<StoredFileWorkingCopy<S>>>> {
        let (this, source, target) = (self.rc(), source.clone(), target.cloned());

        async_fn(async move {
            // Get to target resource
            let target = match target {
                Some(target) => target,
                None => {
                    let working_copy = this.get(&source);
                    let picked = if working_copy.as_ref().and_then(FileWorkingCopy::as_untitled).is_some_and(|working_copy| working_copy.has_associated_file_path()) {
                        Some(this.suggest_save_path(&source).await?)
                    } else {
                        let suggested_target = options.as_ref().and_then(|options| options.suggested_target.clone()).unwrap_or_else(|| source.clone());
                        let suggested = this.suggest_save_path(&suggested_target).await?;

                        this.services
                            .file_dialog_service
                            .pick_file_to_save(&suggested, options.as_ref().and_then(|options| options.base.available_file_systems.clone()))
                            .await?
                    };

                    match picked {
                        Some(target) => target,
                        None => return Ok(None), // user canceled
                    }
                }
            };

            // Ensure target is not marked as readonly and prompt otherwise
            if this.services.services.files_configuration_service.is_readonly(&target, None).is_readonly() {
                let confirmed = this.confirm_make_writeable(&target).await?;
                if !confirmed {
                    return Ok(None);
                } else {
                    let _ = this.services.services.files_configuration_service.update_readonly(&target, SingleReadonlyUpdate::Update(ReadonlyUpdate::False));
                }
            }

            // Just save if target is same as working copies own resource
            // and we are not saving an untitled file working copy
            if this.services.services.file_service.has_provider(&source) && ext_uri().is_equal(Some(&source), Some(&target), false) {
                // force to save, even if not dirty (https://github.com/microsoft/vscode/issues/99619)
                let mut forced = options.map(|options| options.base).unwrap_or_default();
                forced.force = Some(true);

                return this.do_save(&source, Some(forced)).await;
            }

            // If the target is different but of same identity, we
            // move the source to the target, knowing that the
            // underlying file system cannot have both and then save.
            // However, this will only work if the source exists
            // and is not orphaned, so we need to check that too.
            if this.services.services.file_service.has_provider(&source)
                && this.services.uri_identity_service.ext_uri().is_equal(Some(&source), Some(&target), false)
                && this.services.services.file_service.exists(&source).await?
            {
                // Move via working copy file service to enable participants
                let operation = MoveOperation { file: RequiredSourceTargetPair { source: source.clone(), target: target.clone() }, overwrite: None };
                this.services.services.working_copy_file_service.move_(vec![operation], CancellationToken::None, None).await?;

                // At this point we don't know whether we have a
                // working copy for the source or the target URI so we
                // simply try to save with both resources.
                let save_options = options.map(|options| options.base);
                return match this.do_save(&source, save_options.clone()).await? {
                    Some(saved) => Ok(Some(saved)),
                    None => this.do_save(&target, save_options).await,
                };
            }

            // Perform normal "Save As"
            this.do_save_as(&source, &target, options).await
        })
    }

    fn do_save(&self, resource: &URI, options: Option<SaveOptions>) -> Promise<Option<Rc<StoredFileWorkingCopy<S>>>> {
        // Save is only possible with stored file working copies,
        // any other have to go via `saveAs` flow.
        let stored_file_working_copy = self.stored.get(resource);

        async_fn(async move {
            if let Some(stored_file_working_copy) = stored_file_working_copy {
                let success = stored_file_working_copy.save_stored(Some(StoredFileWorkingCopySaveAsOptions::from_save_options(options.unwrap_or_default()))).await?;
                if success {
                    return Ok(Some(stored_file_working_copy));
                }
            }

            Ok(None)
        })
    }

    fn do_save_as(&self, source: &URI, target: &URI, options: Option<FileWorkingCopySaveAsOptions>) -> Promise<Option<Rc<StoredFileWorkingCopy<S>>>> {
        let (this, source, target) = (self.rc(), source.clone(), target.clone());

        async_fn(async move {
            // If the source is an existing file working copy, we can directly
            // use that to copy the contents to the target destination
            let source_working_copy = this.get(&source);
            let resolved_source_model = source_working_copy.as_ref().filter(|working_copy| working_copy.is_resolved()).and_then(FileWorkingCopy::model);
            let source_contents: VSBufferReadableStream = match resolved_source_model {
                Some(model) => model.snapshot(SnapshotContext::Save, CancellationToken::None).await?,
                // Otherwise we resolve the contents from the underlying file
                None => this.services.services.file_service.read_file_stream(&source, None, None).await?.value,
            };

            // Resolve target
            let (target_file_exists, target_stored_file_working_copy) = this.do_resolve_save_target(&source, &target).await?;

            // Confirm to overwrite if we have an untitled file working copy with associated path where
            // the file actually exists on disk and we are instructed to save to that file path.
            // This can happen if the file was created after the untitled file was opened.
            // See https://github.com/microsoft/vscode/issues/67946
            if let Some(source_untitled_working_copy) = source_working_copy.as_ref().and_then(FileWorkingCopy::as_untitled)
                && source_untitled_working_copy.has_associated_file_path()
                && target_file_exists
                && this.services.uri_identity_service.ext_uri().is_equal(
                    Some(&target),
                    Some(&to_local_resource(
                        &source_untitled_working_copy.resource(),
                        this.services.environment_service.remote_authority().as_deref(),
                        &this.services.path_service.default_uri_scheme(),
                    )),
                    false,
                )
            {
                let overwrite = this.confirm_overwrite(&target).await?;
                if !overwrite {
                    return Ok(None);
                }
            }

            // Take over content from source to target
            if let Some(model) = target_stored_file_working_copy.model() {
                model.update(source_contents, CancellationToken::None).await?;
            }

            // Set source options depending on target exists or not
            let mut base = options.map(|options| options.base).unwrap_or_default();
            if base.source.as_deref().is_none_or(str::is_empty) {
                base.source = Some(if target_file_exists { save_replace_source() } else { save_create_source() });
            }

            // Save target (force to save, even if not dirty: https://github.com/microsoft/vscode/issues/99619)
            let mut save_options = StoredFileWorkingCopySaveAsOptions::from_save_options(base);
            save_options.from = Some(source.clone());
            save_options.options.base.force = Some(true);
            let success = target_stored_file_working_copy.save_stored(Some(save_options)).await?;
            if !success {
                return Ok(None);
            }

            // Revert the source
            if let Some(source_working_copy) = &source_working_copy
                && let Err(error) = source_working_copy.revert().await
            {
                // It is possible that reverting the source fails, for example
                // when a remote is disconnected and we cannot read it anymore.
                // However, this should not interrupt the "Save As" flow, so
                // we gracefully catch the error and just log it.

                this.services.services.log_service.error_args(&LogMessage::from(&error), &[]);
            }

            // Events
            if *source.scheme() == schemas::UNTITLED {
                this.untitled.notify_did_save(&source, &target);
            }

            Ok(Some(target_stored_file_working_copy))
        })
    }

    /// `doResolveSaveTarget(source, target)`: devolve `{ targetFileExists, targetStoredFileWorkingCopy }`.
    fn do_resolve_save_target(&self, source: &URI, target: &URI) -> Promise<(bool, Rc<StoredFileWorkingCopy<S>>)> {
        let (this, source, target) = (self.rc(), source.clone(), target.clone());

        async_fn(async move {
            // Prefer an existing stored file working copy if it is already resolved
            // for the given target resource
            if let Some(target_stored_file_working_copy) = this.stored.get(&target).filter(|working_copy| working_copy.is_resolved()) {
                return Ok((true, target_stored_file_working_copy));
            }

            // Otherwise create the target working copy empty if
            // it does not exist already and resolve it from there
            let target_file_exists = this.services.services.file_service.exists(&target).await?;

            // Create target file adhoc if it does not exist yet
            if !target_file_exists {
                let operation = CreateFileOperation { resource: target.clone(), contents: None, overwrite: None };
                this.services.services.working_copy_file_service.create(vec![operation], CancellationToken::None, None).await?;
            }

            // At this point we need to resolve the target working copy
            // and we have to do an explicit check if the source URI
            // equals the target via URI identity. If they match and we
            // have had an existing working copy with the source, we
            // prefer that one over resolving the target. Otherwise we
            // would potentially introduce a
            let target_stored_file_working_copy = if this.services.uri_identity_service.ext_uri().is_equal(Some(&source), Some(&target), false) && this.get(&source).is_some() {
                this.stored.resolve(&source, None).await?
            } else {
                this.stored.resolve(&target, None).await?
            };

            Ok((target_file_exists, target_stored_file_working_copy))
        })
    }

    fn confirm_overwrite(&self, resource: &URI) -> Promise<bool> {
        let dialog_service = self.services.dialog_service.clone();
        let name = ext_uri().basename(resource, None);
        let parent_name = ext_uri().basename(&ext_uri().dirname(resource), None);

        async_fn(async move {
            let mut base = BaseDialogOptions::new(localize("confirmOverwrite", "'{0}' already exists. Do you want to replace it?", &[&name]));
            base.dialog_type = Some(DialogTypeOrSeverity::Type(DialogType::Warning));
            base.detail = Some(DialogDetail::Text(localize(
                "overwriteIrreversible",
                "A file or folder with the name '{0}' already exists in the folder '{1}'. Replacing it will overwrite its current contents.",
                &[&name, &parent_name],
            )));
            let confirmation = Confirmation { base, primary_button: Some(localize("replaceButtonLabel", "&&Replace", &[])), cancel_button: None };

            Ok(dialog_service.confirm(confirmation).await?.confirmed)
        })
    }

    fn confirm_make_writeable(&self, resource: &URI) -> Promise<bool> {
        let dialog_service = self.services.dialog_service.clone();
        let name = ext_uri().basename(resource, None);

        async_fn(async move {
            let mut base = BaseDialogOptions::new(localize("confirmMakeWriteable", "'{0}' is marked as read-only. Do you want to save anyway?", &[&name]));
            base.dialog_type = Some(DialogTypeOrSeverity::Type(DialogType::Warning));
            base.detail = Some(DialogDetail::Text(localize("confirmMakeWriteableDetail", "Paths can be configured as read-only via settings.", &[])));
            let confirmation = Confirmation { base, primary_button: Some(localize("makeWriteableButtonLabel", "&&Save Anyway", &[])), cancel_button: None };

            Ok(dialog_service.confirm(confirmation).await?.confirmed)
        })
    }

    fn suggest_save_path(&self, resource: &URI) -> Promise<URI> {
        // 1.) Just take the resource as is if the file service can handle it
        if self.services.services.file_service.has_provider(resource) {
            return Promise::resolved(resource.clone());
        }

        // 2.) Pick the associated file path for untitled working copies if any
        let working_copy = self.get(resource);
        if working_copy.as_ref().and_then(FileWorkingCopy::as_untitled).is_some_and(|working_copy| working_copy.has_associated_file_path()) {
            return Promise::resolved(to_local_resource(
                resource,
                self.services.environment_service.remote_authority().as_deref(),
                &self.services.path_service.default_uri_scheme(),
            ));
        }

        let (this, resource) = (self.rc(), resource.clone());
        async_fn(async move {
            let default_file_path = this.services.file_dialog_service.default_file_path(None).await?;

            // 3.) Pick the working copy name if valid joined with default path
            if let Some(working_copy) = working_copy {
                let name = working_copy.name();
                let candidate_path = join_path(&default_file_path, &[&name]);
                if this.services.path_service.has_valid_basename(&candidate_path, Some(&name)).await? {
                    return Ok(candidate_path);
                }
            }

            // 4.) Finally fallback to the name of the resource joined with default path
            Ok(join_path(&default_file_path, &[&ext_uri().basename(&resource, None)]))
        })
    }

    //#endregion

    //#region Lifecycle

    /// Disposes all working copies of the manager and disposes the manager. This method is different from `dispose` in that
    /// it will unregister any working copy from the `IWorkingCopyService`. Since this impact things like backups, the method
    /// is `async` because it needs to trigger `save` for any dirty working copy to preserve the data.
    ///
    /// Callers should make sure to e.g. close any editors associated with the working copy.
    pub fn destroy(&self) -> Promise<()> {
        let (stored, untitled) = (self.stored.destroy(), self.untitled.destroy());

        async_fn(async move {
            promises::settled(vec![stored, untitled]).await?;

            Ok(())
        })
    }

    //#endregion
}

impl<S: IStoredFileWorkingCopyModel + 'static, U: IUntitledFileWorkingCopyModel + 'static> IDisposable for FileWorkingCopyManager<S, U> {
    fn dispose(&self) {
        self.store.dispose();
    }
}

/// O provedor de decorações das working copies armazenadas (somente leitura e arquivo apagado).
struct FileWorkingCopyDecorationsProvider<S: IStoredFileWorkingCopyModel + 'static> {
    base: Rc<Disposable>,
    on_did_change: Rc<Emitter<Vec<URI>>>,
    stored: Weak<StoredFileWorkingCopyManager<S>>,
}

impl<S: IStoredFileWorkingCopyModel + 'static> FileWorkingCopyDecorationsProvider<S> {
    fn new(stored: &Rc<StoredFileWorkingCopyManager<S>>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());

        let provider = Rc::new(Self { base, on_did_change, stored: Rc::downgrade(stored) });
        provider.register_listeners(stored);

        provider
    }

    fn register_listeners(&self, stored: &StoredFileWorkingCopyManager<S>) {
        // Creates
        let emitter = self.on_did_change.clone();
        self.base.register(stored.on_did_resolve().listen(move |working_copy| {
            if working_copy.is_readonly().is_readonly() || working_copy.has_state(StoredFileWorkingCopyState::Orphan) {
                emitter.fire(vec![working_copy.resource()]);
            }
        }));

        // Removals: once a stored working copy is no longer
        // under our control, make sure to signal this as
        // decoration change because from this point on we
        // have no way of updating the decoration anymore.
        let emitter = self.on_did_change.clone();
        self.base.register(stored.on_did_remove().listen(move |working_copy_uri| emitter.fire(vec![working_copy_uri.clone()])));

        // Changes
        let emitter = self.on_did_change.clone();
        self.base.register(stored.on_did_change_readonly().listen(move |working_copy| emitter.fire(vec![working_copy.resource()])));
        let emitter = self.on_did_change.clone();
        self.base.register(stored.on_did_change_orphaned().listen(move |working_copy| emitter.fire(vec![working_copy.resource()])));
    }
}

impl<S: IStoredFileWorkingCopyModel + 'static> IDisposable for FileWorkingCopyDecorationsProvider<S> {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl<S: IStoredFileWorkingCopyModel + 'static> IDecorationsProvider for FileWorkingCopyDecorationsProvider<S> {
    fn label(&self) -> String {
        localize("fileWorkingCopyDecorations", "File Working Copy Decorations", &[])
    }

    fn on_did_change(&self) -> Event<Vec<URI>> {
        self.on_did_change.event()
    }

    fn provide_decorations(&self, uri: &URI, _token: CancellationToken) -> MaybePromise<Option<DecorationData>> {
        let Some(working_copy) = self.stored.upgrade().and_then(|stored| stored.get(uri)) else {
            return MaybePromise::Value(None);
        };
        if working_copy.is_disposed() {
            return MaybePromise::Value(None);
        }

        let is_readonly = working_copy.is_readonly().is_readonly();
        let is_orphaned = working_copy.has_state(StoredFileWorkingCopyState::Orphan);

        let lock_small = || DecorationLetter::Icon(ThemeIcon::from_id(CODICON_LOCK_SMALL));

        // Readonly + Orphaned
        let decoration = if is_readonly && is_orphaned {
            Some(DecorationData {
                color: Some(LIST_ERROR_FOREGROUND.to_string()),
                letter: Some(lock_small()),
                strikethrough: Some(true),
                tooltip: Some(localize("readonlyAndDeleted", "Deleted, Read-only", &[])),
                ..DecorationData::default()
            })
        }
        // Readonly
        else if is_readonly {
            Some(DecorationData { letter: Some(lock_small()), tooltip: Some(localize("readonly", "Read-only", &[])), ..DecorationData::default() })
        }
        // Orphaned
        else if is_orphaned {
            Some(DecorationData {
                color: Some(LIST_ERROR_FOREGROUND.to_string()),
                strikethrough: Some(true),
                tooltip: Some(localize("deleted", "Deleted", &[])),
                ..DecorationData::default()
            })
        } else {
            None
        };

        MaybePromise::Value(decoration)
    }
}
