//! Porte parcial de `src/vs/workbench/contrib/notebook/common/notebookEditorModel.ts`: o modelo de uma working copy de
//! notebook (`NotebookFileWorkingCopyModel`), a fábrica dele (`NotebookFileWorkingCopyModelFactory`) e o erro de salvamento
//! (`NotebookSaveError`, `getSaveErrorMessage`). O `SimpleNotebookEditorModel` (o `EditorModel` que amarra o modelo ao
//! `IFileWorkingCopyManager`) não é portado: depende do gerenciador de working copies de notebook e do editor de notebook.
//!
//! Divergências:
//! - `Disposable` é o campo `base`; o modelo nasce como `Rc<Self>` (`Rc::new_cyclic`), e os fechos (eventos, o delegate de
//!   salvamento) guardam um `Weak` dele.
//! - `IStoredFileWorkingCopyModel` e `IUntitledFileWorkingCopyModel` têm eventos de conteúdo com tipos diferentes
//!   (`on_did_change_stored_content` e `on_did_change_untitled_content`) e o `IFileWorkingCopyModel` o sinal sem dados; o
//!   original dispara um objeto só que serve aos três, e o porte dispara os três juntos ([`NotebookFileWorkingCopyModel::fire_content_changed`]).
//! - `save: ((options, token) => Promise<IFileStatWithMetadata>) | undefined` é o `save` do trait `IStoredFileWorkingCopyModel`,
//!   que devolve `Option<Promise<..>>`: `None` é o `save` indefinido (o `typeof model.save === 'function'` do original).
//! - `onWillDispose = _notebookModel.onWillDispose.bind(_notebookModel)` é `notebook_model.on_will_dispose()`.
//! - `versionId` devolve o `alternativeVersionId` (texto) como [`Dynamic`], o tipo do `versionId` do trait.
//! - `info instanceof SimpleNotebookProviderInfo` não é checado: `withNotebookDataProvider` devolve sempre o
//!   `SimpleNotebookProviderInfo` no porte.
//! - `NotebookSaveError` (`class extends Error` com `name = 'NotebookSaveError'`) é o [`Error`] com esse nome; `error.name`
//!   é o campo `name`. `error instanceof FileOperationError` é `FileOperationError::from_error`.
//! - `async` é `async_fn`, e o `catch` encadeado em `setSaveDelegate()` é o `Promise::catch`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::buffer::{VSBufferReadableStream, stream_to_buffer};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::configuration::service::IConfigurationService;
use je_platform::files::files::{FileOperationError, FileOperationResult, FileStatWithMetadata, WriteFileOptions};
use je_platform::telemetry::ITelemetryService;

use super::model::notebook_text_model::NotebookTextModel;
use super::notebook_common::{NotebookCellsChangeType, NotebookSetting};
use super::notebook_logging_service::INotebookLoggingService;
use super::notebook_service::{INotebookSerializer, INotebookService};
use crate::services::working_copy::file_working_copy::{FileWorkingCopyModelConfiguration, IFileWorkingCopyModel, IFileWorkingCopyModelFactory, SnapshotContext};
use crate::services::working_copy::stored_file_working_copy::{IStoredFileWorkingCopyModel, StoredFileWorkingCopyModelContentChangedEvent};
use crate::services::working_copy::untitled_file_working_copy::{IUntitledFileWorkingCopyModel, UntitledFileWorkingCopyModelContentChangedEvent};

/// O delegate de `save`: `(options, token) => Promise<IFileStatWithMetadata>`.
type SaveDelegate = Rc<dyn Fn(WriteFileOptions, CancellationToken) -> Promise<FileStatWithMetadata>>;

/// `new NotebookSaveError(message)`.
fn notebook_save_error(message: &str) -> Error {
    Error::named("NotebookSaveError", message)
}

/// `NotebookFileWorkingCopyModel`.
pub struct NotebookFileWorkingCopyModel {
    base: Rc<Disposable>,
    this: Weak<NotebookFileWorkingCopyModel>,
    notebook_model: Rc<NotebookTextModel>,
    notebook_service: Rc<dyn INotebookService>,
    configuration_service: Rc<dyn IConfigurationService>,
    telemetry_service: Rc<dyn ITelemetryService>,
    notebook_log_service: Rc<dyn INotebookLoggingService>,
    on_did_change_content: Rc<Emitter<()>>,
    on_did_change_stored_content: Rc<Emitter<StoredFileWorkingCopyModelContentChangedEvent>>,
    on_did_change_untitled_content: Rc<Emitter<UntitledFileWorkingCopyModelContentChangedEvent>>,
    configuration: Cell<Option<FileWorkingCopyModelConfiguration>>,
    save: RefCell<Option<SaveDelegate>>,
}

impl NotebookFileWorkingCopyModel {
    pub fn new(
        notebook_model: Rc<NotebookTextModel>,
        notebook_service: Rc<dyn INotebookService>,
        configuration_service: Rc<dyn IConfigurationService>,
        telemetry_service: Rc<dyn ITelemetryService>,
        notebook_log_service: Rc<dyn INotebookLoggingService>,
    ) -> Rc<Self> {
        let this = Rc::new_cyclic(|this| {
            let base = Disposable::new();
            let on_did_change_content = Emitter::<()>::new();
            let on_did_change_stored_content = Emitter::<StoredFileWorkingCopyModelContentChangedEvent>::new();
            let on_did_change_untitled_content = Emitter::<UntitledFileWorkingCopyModelContentChangedEvent>::new();
            base.register(on_did_change_content.clone());
            base.register(on_did_change_stored_content.clone());
            base.register(on_did_change_untitled_content.clone());
            Self {
                base,
                this: this.clone(),
                notebook_model: notebook_model.clone(),
                notebook_service,
                configuration_service,
                telemetry_service,
                notebook_log_service,
                on_did_change_content,
                on_did_change_stored_content,
                on_did_change_untitled_content,
                configuration: Cell::new(None),
                save: RefCell::new(None),
            }
        });

        let weak = Rc::downgrade(&this);
        this.base.register(notebook_model.on_did_change_content().listen(move |e| {
            let Some(this) = weak.upgrade() else { return };
            for raw_event in &e.raw_events {
                if raw_event.event.kind() == NotebookCellsChangeType::Initialize {
                    continue;
                }
                if raw_event.transient {
                    continue;
                }
                this.fire_content_changed();
                break;
            }
        }));

        let save_with_reduced_communication = matches!(this.configuration_service.get_value(Some(NotebookSetting::REMOTE_SAVING), None), Dynamic::Bool(true));

        if save_with_reduced_communication || this.notebook_model.uri().scheme().to_string_lossy() == schemas::VSCODE_REMOTE {
            // Intentionally pick a larger delay for triggering backups to allow auto-save
            // to complete first on the optimized save path
            this.configuration.set(Some(FileWorkingCopyModelConfiguration { backup_delay: Some(10000) }));
        }

        // Override save behavior to avoid transferring the buffer across the wire 3 times
        if save_with_reduced_communication {
            let log_service = this.notebook_log_service.clone();
            drop(this.set_save_delegate().catch(move |error| log_service.error("WorkingCopyModel", &format!("Failed to set save delegate: {error}"))));
        }

        this
    }

    /// `_onDidChangeContent.fire({ isRedoing: false, isUndoing: false, isInitial: false })`.
    fn fire_content_changed(&self) {
        // A informação de desfazer e refazer ainda não vem do modelo do notebook (`isRedoing` e `isUndoing` ficam falsos).
        self.on_did_change_stored_content.fire(StoredFileWorkingCopyModelContentChangedEvent { is_redoing: false, is_undoing: false });
        self.on_did_change_untitled_content.fire(UntitledFileWorkingCopyModelContentChangedEvent { is_initial: false });
        self.on_did_change_content.fire(());
    }

    fn set_save_delegate(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            // make sure we wait for a serializer to resolve before we try to handle saves in the EH
            this.get_notebook_serializer().await?;

            let weak = Rc::downgrade(&this);
            let delegate: SaveDelegate = Rc::new(move |options: WriteFileOptions, token: CancellationToken| match weak.upgrade() {
                Some(this) => this.save_with_serializer(options, token),
                None => Promise::rejected(Error::new("NotebookFileWorkingCopyModel is disposed")),
            });
            *this.save.borrow_mut() = Some(delegate);
            Ok(())
        })
    }

    /// O corpo do `this.save = async (options, token) => { ... }`.
    fn save_with_serializer(self: &Rc<Self>, options: WriteFileOptions, token: CancellationToken) -> Promise<FileStatWithMetadata> {
        let this = self.clone();
        async_fn(async move {
            let result = this.save_with_serializer_inner(options, &token).await;
            if let Err(error) = &result
                && !token.is_cancellation_requested()
                && error.name != "Canceled"
            {
                let is_ipynb = this.notebook_model.view_type() == "jupyter-notebook" || this.notebook_model.view_type() == "interactive";
                let error_message = get_save_error_message(error);
                this.telemetry_service.public_log_error2(
                    "notebook/SaveError",
                    Some(Dynamic::object([
                        ("isRemote", Dynamic::Bool(this.notebook_model.uri().scheme().to_string_lossy() == schemas::VSCODE_REMOTE)),
                        (
                            "isIPyNbWorkerSerializer",
                            Dynamic::Bool(is_ipynb && matches!(this.configuration_service.get_value(Some("ipynb.experimental.serialization"), None), Dynamic::Bool(true))),
                        ),
                        ("error", Dynamic::string(&error_message)),
                    ])),
                );
            }
            result
        })
    }

    /// O `try { ... }` do `save` (o `catch` reporta o erro e o relança).
    fn save_with_serializer_inner(self: &Rc<Self>, options: WriteFileOptions, token: &CancellationToken) -> Promise<FileStatWithMetadata> {
        let this = self.clone();
        let token = token.clone();
        async_fn(async move {
            let mut serializer = this.notebook_service.try_get_data_provider_sync(&this.notebook_model.view_type()).map(|info| info.serializer.clone());

            if serializer.is_none() {
                this.notebook_log_service.info("WorkingCopyModel", "No serializer found for notebook model, checking if provider still needs to be resolved");
                serializer = Some(match this.get_notebook_serializer().await {
                    Ok(serializer) => serializer,
                    Err(error) => {
                        this.notebook_log_service.error("WorkingCopyModel", &format!("Failed to get notebook serializer: {error}"));
                        // The serializer was set initially but somehow is no longer available
                        *this.save.borrow_mut() = None;
                        return Err(notebook_save_error("Failed to get notebook serializer"));
                    }
                });
            }

            if token.is_cancellation_requested() {
                return Err(Error::cancellation());
            }

            let serializer = serializer.expect("serializer");
            serializer.save(&this.notebook_model.uri(), this.notebook_model.version_id(), options, token).await
        })
    }

    pub fn notebook_model(&self) -> Rc<NotebookTextModel> {
        self.notebook_model.clone()
    }

    pub fn get_notebook_serializer(&self) -> Promise<Rc<dyn INotebookSerializer>> {
        self.notebook_service.with_notebook_data_provider(&self.notebook_model.view_type()).then(|info| info.serializer.clone())
    }
}

impl IFileWorkingCopyModel for NotebookFileWorkingCopyModel {
    fn on_did_change_content(&self) -> Event<()> {
        self.on_did_change_content.event()
    }

    fn on_will_dispose(&self) -> Event<()> {
        self.notebook_model.on_will_dispose()
    }

    fn configuration(&self) -> Option<FileWorkingCopyModelConfiguration> {
        self.configuration.get()
    }

    fn snapshot(&self, context: SnapshotContext, token: CancellationToken) -> Promise<VSBufferReadableStream> {
        self.notebook_service.create_notebook_text_document_snapshot(&self.notebook_model.uri(), context, token)
    }

    fn update(&self, stream: VSBufferReadableStream, token: CancellationToken) -> Promise<()> {
        let this = self.this.upgrade().expect("NotebookFileWorkingCopyModel");
        async_fn(async move {
            let serializer = this.get_notebook_serializer().await?;

            let bytes = stream_to_buffer(&*stream).await?;
            let data = serializer.data_to_notebook(bytes).await?;

            if token.is_cancellation_requested() {
                return Err(Error::cancellation());
            }

            this.notebook_log_service.info("WorkingCopyModel", &format!("Notebook content updated from file system - {}", this.notebook_model.uri().to_string(false)));
            this.notebook_model.reset(data.cells, data.metadata, serializer.options());
            Ok(())
        })
    }
}

impl IStoredFileWorkingCopyModel for NotebookFileWorkingCopyModel {
    fn on_did_change_stored_content(&self) -> Event<StoredFileWorkingCopyModelContentChangedEvent> {
        self.on_did_change_stored_content.event()
    }

    fn version_id(&self) -> Dynamic {
        Dynamic::string(&self.notebook_model.alternative_version_id())
    }

    fn push_stack_element(&self) {
        self.notebook_model.push_stack_element();
    }

    fn save(&self, options: WriteFileOptions, token: CancellationToken) -> Option<Promise<FileStatWithMetadata>> {
        let delegate = self.save.borrow().clone();
        delegate.map(|delegate| delegate(options, token))
    }
}

impl IUntitledFileWorkingCopyModel for NotebookFileWorkingCopyModel {
    fn on_did_change_untitled_content(&self) -> Event<UntitledFileWorkingCopyModelContentChangedEvent> {
        self.on_did_change_untitled_content.event()
    }
}

impl IDisposable for NotebookFileWorkingCopyModel {
    fn dispose(&self) {
        self.notebook_model.dispose();
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "NotebookFileWorkingCopyModel"
    }
}

/// `NotebookFileWorkingCopyModelFactory`.
pub struct NotebookFileWorkingCopyModelFactory {
    view_type: String,
    notebook_service: Rc<dyn INotebookService>,
    configuration_service: Rc<dyn IConfigurationService>,
    telemetry_service: Rc<dyn ITelemetryService>,
    notebook_log_service: Rc<dyn INotebookLoggingService>,
}

impl NotebookFileWorkingCopyModelFactory {
    pub fn new(
        view_type: &str,
        notebook_service: Rc<dyn INotebookService>,
        configuration_service: Rc<dyn IConfigurationService>,
        telemetry_service: Rc<dyn ITelemetryService>,
        notebook_log_service: Rc<dyn INotebookLoggingService>,
    ) -> Self {
        Self { view_type: view_type.to_string(), notebook_service, configuration_service, telemetry_service, notebook_log_service }
    }
}

impl IFileWorkingCopyModelFactory<NotebookFileWorkingCopyModel> for NotebookFileWorkingCopyModelFactory {
    fn create_model(&self, resource: &URI, stream: VSBufferReadableStream, _token: CancellationToken) -> Promise<Rc<NotebookFileWorkingCopyModel>> {
        let resource = resource.clone();
        let view_type = self.view_type.clone();
        let notebook_service = self.notebook_service.clone();
        let configuration_service = self.configuration_service.clone();
        let telemetry_service = self.telemetry_service.clone();
        let notebook_log_service = self.notebook_log_service.clone();
        async_fn(async move {
            let notebook_model = match notebook_service.get_notebook_text_model(&resource) {
                Some(notebook_model) => notebook_model,
                None => notebook_service.create_notebook_text_model(&view_type, &resource, Some(stream)).await?,
            };

            Ok(NotebookFileWorkingCopyModel::new(notebook_model, notebook_service, configuration_service, telemetry_service, notebook_log_service))
        })
    }
}

/// `getSaveErrorMessage`.
fn get_save_error_message(error: &Error) -> String {
    if error.name == "NotebookSaveError" {
        return error.message.clone();
    }
    if let Some(file_operation_error) = FileOperationError::from_error(error) {
        return match file_operation_error.file_operation_result {
            FileOperationResult::FileIsDirectory => "File is a directory",
            FileOperationResult::FileNotFound => "File not found",
            FileOperationResult::FileNotModifiedSince => "File not modified since",
            FileOperationResult::FileModifiedSince => "File modified since",
            FileOperationResult::FileMoveConflict => "File move conflict",
            FileOperationResult::FileWriteLocked => "File write locked",
            FileOperationResult::FilePermissionDenied => "File permission denied",
            FileOperationResult::FileTooLarge => "File too large",
            FileOperationResult::FileInvalidPath => "File invalid path",
            FileOperationResult::FileNotDirectory => "File not directory",
            FileOperationResult::FileOtherError => "File other error",
        }
        .to_string();
    }
    "Unknown error".to_string()
}
