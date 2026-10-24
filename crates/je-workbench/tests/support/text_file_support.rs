//! Infraestrutura de teste de `src/vs/workbench/services/textfile/test/browser/textFileEditorModelManager.test.ts`: o
//! `TestTextFileService` de `src/vs/workbench/test/browser/workbenchTestServices.ts` e o acessor que o `setup` da suíte
//! usa (`workbenchInstantiationService` mais `TestServiceAccessor`, com o `ITextFileService`, o `IModelService` e o
//! `ILanguageService`).
//!
//! Divergências:
//! - **Herança.** `TestTextFileService extends BrowserTextFileService` é a composição sobre o
//!   [`AbstractTextFileService`] real (montado por [`BrowserTextFileService::new`], que já registra o veto do desligamento):
//!   o [`TestTextFileService`] repassa ao serviço real todo membro do contrato `ITextFileService` e só sobrescreve
//!   `readStream` e `write`, como o original. O repasse é gerado por uma macro (`delegate_to_inner!`), porque o Rust não
//!   tem herança e escrever cada corpo repetiria o mesmo código 15 vezes.
//! - **`readStream`.** O original cria o `ITextFileStreamContent` direto do `fileService.readFileStream` (sem a detecção de
//!   codificação), com `encoding: 'utf8'` e `size: 10`. O `createTextBufferFactoryFromStream` do original aceita
//!   `string | VSBuffer`; o do porte aceita só texto, então os buffers do fluxo passam por um `transform` que os decodifica
//!   como UTF-8 (o `VSBuffer.toString()` que o original aplica a cada trecho).
//! - **`getLastResolvedFileStat`.** Não existe no `TestTextFileService` do oráculo (o `lastResolvedFileStat` do modelo é
//!   lido por `TextFileEditorModel::last_resolved_file_stat`), então não há equivalente aqui.
//! - **`ITestTextFileEditorModelManager`.** É o [`TextFileEditorModelManager`] concreto ([`TestTextFileService::files_manager`]),
//!   que tem `add` e `remove` públicos (o contrato `ITextFileEditorModelManager` não os tem).
//! - **`createInstance(TextFileEditorModel, resource, encoding, languageId)`.** É [`TextFileTestAccessor::create_model`], que
//!   usa a mesma fábrica que o gerenciador usa para criar os modelos, com os 14 serviços que o construtor injeta. Os modelos
//!   precisam do próprio `ITextFileService`, que só existe depois do gerenciador: a fábrica o lê de um `Weak` preenchido
//!   quando o serviço nasce.
//! - **`workbenchInstantiationService`.** O contêiner de injeção não entra: o [`TextFileTestAccessor`] compõe o
//!   [`TestServiceAccessor`] de `stored_file_working_copy_support` (que ele expõe por `Deref`) e acrescenta os serviços que o
//!   serviço de arquivos de texto lê, com os mesmos dublês do original. O `IInstantiationService` do
//!   `UntitledTextEditorService` leva só os serviços que o modelo sem título usaria; esta suíte não cria modelos sem título.
//! - **`TestFileDialogService.confirmResult`.** Sem `setConfirmResult`, o original devolve `undefined`; o porte devolve
//!   `ConfirmResult::Cancel`, o valor que os chamadores tratam como "sem escolha".
//! - **`TestDialogService.prompt`.** Roda o primeiro botão (mais o botão de cancelar personalizado), como o original; o
//!   resultado fica sem `checkboxChecked`.
//! - **`TestCodeEditorService`.** Guarda as propriedades transitórias por recurso do modelo, como o
//!   `AbstractCodeEditorService`.
//!
//! Acrescentado para `textFileEditorModel.test.ts`:
//! - **`TestWorkingCopyService.testUnregisterWorkingCopy`.** O `unregisterWorkingCopy` do `WorkingCopyService` é privado, e
//!   o único caminho público é o descarte do registro devolvido por `registerWorkingCopy`. O [`TestWorkingCopyService`]
//!   embrulha o serviço real, guarda esse registro de cada working copy e o descarta no `testUnregisterWorkingCopy`. A
//!   diferença para o original é que o descarte também dispara `onDidUnregister` (o `unregisterWorkingCopy` direto não
//!   dispara); nenhum caso da suíte escuta esse evento. O descarte é de uma vez só, então o descarte do modelo depois dele
//!   não faz mais nada, como no original.
//! - **`TestReadonlyTextFileEditorModel`.** `isReadonly()` sobrescrito para `true`: como o `TextFileEditorModel` do porte é
//!   uma struct sem herança, [`TextFileTestAccessor::create_readonly_model`] cria o modelo com um
//!   `IFilesConfigurationService` que devolve somente leitura (todas as consultas de somente leitura do modelo passam por
//!   `isReadonly()`, que é o que o serviço responde), e repassa o resto ao serviço real.
//!
//! Acrescentado para `fileEditorInput.test.ts` (e o caso `save() and isDirty() - proper with check for mtimes` de
//! `textFileEditorModel.test.ts`):
//! - **`createFileEditorInput`.** [`TextFileTestAccessor::create_file_editor_input`] é o `createFileEditorInput` de
//!   `workbenchTestServices.ts`, sobre o [`TextFileTestAccessor::instantiation_service`] (o `instantiationService` do
//!   `workbenchInstantiationService`), que passa a ter os serviços que a `FileEditorInput` injeta (`ITextFileService`,
//!   `ITextModelService`, `ILabelService`, `IFileService`, `IFilesConfigurationService`, `IEditorService`, `IPathService`,
//!   `ITextResourceConfigurationService` e `ICustomEditorLabelService`). O `ITextEditorService` entra pela
//!   [`TextFileTestAccessor::collection`], como a sobrescrita `textEditorService` do `workbenchInstantiationService`.
//! - **`ITextModelService`.** O [`TestTextModelService`] faz o que o `TextModelResolverService` faz para recursos de arquivo.
//! - **`setReadStreamErrorOnce` e `setWriteErrorOnce`** aceitam também o erro já montado (o `TextFileOperationError`).
//!
//! Acrescentado para `nativeTextFileService.test.ts` e `nativeTextFileService.io.test.ts`:
//! - **`TestNativeTextFileServiceWithEncodingOverrides`.** [`TextFileTestAccessor::with_native_child_services`] e
//!   [`TestTextFileService::with_native_encoding_overrides`] são as versões nativas das do navegador (o serviço é o
//!   `NativeTextFileService`, com o mesmo `TestEncodingOracle`).
//! - **Modelos criados pelo caso.** No original, `instantiationService.createInstance(TextFileEditorModel, ...)` usa o
//!   `IFileService` do pai (o `TestFileService`) e o gerenciador do filho usa o do `createChild(collection)`. O
//!   [`TextFileTestAccessor::create_model`] do acessor nativo repete isso (o `IFileService` do pai); o `ITextFileService` que o
//!   modelo lê continua sendo o do filho, porque o acessor só monta um serviço de arquivos de texto por vez (a suíte só
//!   resolve e salva um modelo sem alteração).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::buffer::{VSBuffer, VSBufferReadableStream, new_writeable_buffer_stream};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::{DisposableId, DisposableRef, DisposableStore, IDisposable, id_of, id_of_ref, to_disposable};
use je_base::network::schemas;
use je_base::promise::{MaybePromise, Promise};
use je_base::runtime;
use je_base::stream::{ReadableStream, ReadableStreamEvents, Transformer, WriteableStream, WriteableStreamImpl, transform};
use je_base::uri::URI;
use je_editor::language_service::{ILanguageService, I_LANGUAGE_SERVICE, LanguageService};
use je_editor::model_service::ModelService;
use je_editor::services_model::{IModelService, I_MODEL_SERVICE};
use je_editor::text_model::TextModel;
use je_editor::text_resource_configuration_service::{ITextResourceConfigurationService, I_TEXT_RESOURCE_CONFIGURATION_SERVICE};
use je_editor::undo_redo_service::UndoRedoService;
use je_instantiation::{Injectable, InstantiationService, ServiceCollection};
use je_platform::accessibility::{AccessibilitySupport, IAccessibilityService, I_ACCESSIBILITY_SERVICE};
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::dialogs::dialogs::{
    CheckboxResult, ConfirmResult, Confirmation, ConfirmationResult, FileNameOrResource, IDialogService, IFileDialogService, Input, InputResult, OpenDialogOptions, PickAndOpenOptions,
    Prompt, PromptCancel, PromptResult, SaveDialogOptions,
};
use je_platform::files::file_service::FileService;
use je_platform::files::files::{
    FileChange, FileDeleteOptions, FileOpenOptions, FileOperationError, FileOverwriteOptions, FileReadStreamOptions, FileStat, FileStatWithMetadata, FileSystemProvider,
    FileSystemProviderCapabilities, FileType, FileWriteOptions, Stat, WatchOptions,
};
use je_platform::files::i_file_service::{IFileService, I_FILE_SERVICE};
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::label::label::{ILabelService, I_LABEL_SERVICE};
use je_platform::log::NullLogService;
use je_platform::path::path_service::{IPathService, I_PATH_SERVICE};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_platform::workspace::IWorkspaceContextService;
use je_workbench::common::editor::SaveReason;
use je_workbench::contrib::files::browser::editors::file_editor_input::FileEditorInput;
use je_workbench::services::editor::custom_editor_label_service::{CustomEditorLabelService, ICustomEditorLabelService, I_CUSTOM_EDITOR_LABEL_SERVICE};
use je_workbench::services::editor::editor_service::{IEditorService, I_EDITOR_SERVICE};
use je_workbench::services::files_configuration::files_configuration_service::{
    IAutoSaveConfiguration, IAutoSaveMode, IFilesConfigurationService, Readonly, ReadonlyUpdate, ResourceOrEditor, SingleReadonlyUpdate, I_FILES_CONFIGURATION_SERVICE,
};
use je_workbench::services::textmodel_resolver::resolver_service::{ITextEditorModel, ITextModelService, TextModelReference, I_TEXT_MODEL_SERVICE};
use je_workbench::services::working_copy::working_copy::{IWorkingCopy, WorkingCopyIdentifier};
use je_workbench::services::working_copy::working_copy_backup::{IWorkingCopyBackupService, I_WORKING_COPY_BACKUP_SERVICE};
use je_workbench::services::working_copy::working_copy_service::{IWorkingCopyService, WorkingCopySaveEvent, WorkingCopyService, I_WORKING_COPY_SERVICE};
use je_workbench::services::decorations::decorations::{IDecoration, IDecorationsProvider, IDecorationsService, IResourceDecorationChangeEvent};
use je_workbench::services::environment::environment_service::IWorkbenchEnvironmentService;
use je_workbench::services::language_detection::language_detection_worker_service::{ILanguageDetectionService, I_LANGUAGE_DETECTION_SERVICE};
use je_workbench::services::lifecycle::lifecycle::ILifecycleService;
use je_workbench::services::textfile::abstract_text_file_service::{AbstractTextFileService, EncodingOverride, TextFileServiceDependencies};
use je_workbench::services::textfile::browser_text_file_service::BrowserTextFileService;
use je_workbench::services::textfile::collaborators::{FileOperationUndoRedoInfo, ICodeEditorTransientProperties, IWorkingCopyFileService};
use je_workbench::services::textfile::encoding::{UTF16BE, UTF16LE, UTF8_WITH_BOM};
use je_workbench::services::textfile::native_text_file_service::NativeTextFileService;
use je_workbench::services::textfile::text_buffer_factory::create_text_buffer_factory_from_stream;
use je_workbench::services::textfile::text_file_editor_model::{TextFileEditorModel, TextFileEditorModelServices};
use je_workbench::services::textfile::text_file_editor_model_manager::TextFileEditorModelManager;
use je_workbench::services::textfile::text_file_save_participant::TextFileSaveParticipant;
use je_workbench::services::textfile::textfiles::{
    BaseTextFileContent, EncodedContent, I_TEXT_FILE_SERVICE, IResourceEncodings, ITextFileEditorModel, ITextFileEditorModelManager, ITextFileService, ReadTextFileEncodingOptions,
    ReadTextFileOptions, ResolvedDecoding, ResolvedEncoding, TextFileContent, TextFileCreateOperation, TextFileEditorModelResolveOrCreateOptions, TextFileResolveOptions,
    TextFileResolveReason, TextFileSaveAsOptions, TextFileSaveOptions, TextFileStreamContent, TextOrSnapshot, WriteTextFileOptions,
};
use je_workbench::services::untitled::untitled_text_editor_service::{IUntitledTextEditorModelManager, UntitledTextEditorService};
use je_workbench::test::browser::workbench_test_services::{TestEnvironmentService, TestExtensionService, TestPathService};
use je_workbench::test::common::workbench_test_services::{TestContextService, TestTextResourceConfigurationService, TestTextResourcePropertiesService};

use super::stored_file_working_copy_support::{TestProgressService, TestServiceAccessor, TestWorkingCopyFileService};

//#region TestTextFileService

/// Gera os membros do contrato `ITextFileService` que o `TestTextFileService` herda sem sobrescrever: cada um chama o
/// membro de mesmo nome do serviço real.
macro_rules! delegate_to_inner {
    ($(fn $name:ident(&self $(, $arg:ident: $ty:ty)*) -> $ret:ty;)*) => {
        $(
            fn $name(&self $(, $arg: $ty)*) -> $ret {
                ITextFileService::$name(&*self.inner $(, $arg)*)
            }
        )*
    };
}

/// Qual classe de teste o [`TestTextFileService`] representa.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TestServiceKind {
    /// `TestTextFileService`: sobrescreve `readStream` e `write`.
    Test,
    /// `TestBrowserTextFileServiceWithEncodingOverrides`: o serviço do navegador com o `TestEncodingOracle` (que fixa as
    /// extensões `utf16le`, `utf16be` e `utf8bom`).
    BrowserWithEncodingOverrides,
    /// `TestNativeTextFileServiceWithEncodingOverrides`: o serviço do desktop (`NativeTextFileService`) com o mesmo
    /// `TestEncodingOracle`.
    NativeWithEncodingOverrides,
}

/// `TestTextFileService` (e, com [`TestTextFileService::with_encoding_overrides`],
/// `TestBrowserTextFileServiceWithEncodingOverrides`: as duas classes repassam o mesmo serviço real, então dividem o
/// repasse gerado pela macro e diferem só no que sobrescrevem).
pub struct TestTextFileService {
    kind: TestServiceKind,
    inner: Rc<AbstractTextFileService>,
    manager: Rc<TextFileEditorModelManager>,
    file_service: Rc<dyn IFileService>,
    read_stream_error: RefCell<Option<Error>>,
    write_error: RefCell<Option<Error>>,
}

impl TestTextFileService {
    /// `constructor(...)`: o `super(...)` é o [`BrowserTextFileService::new`].
    pub fn new(dependencies: TextFileServiceDependencies) -> Rc<Self> {
        Self::build(dependencies, TestServiceKind::Test)
    }

    /// `new TestBrowserTextFileServiceWithEncodingOverrides(...)`: o serviço do navegador sem as sobrescritas de
    /// `readStream` e `write`, com o `TestEncodingOracle` no lugar do oráculo de codificação.
    ///
    /// Divergência: o `TestEncodingOracle` sobrescreve o getter e o setter de `encodingOverrides` (o setter não faz
    /// nada, então a mudança de pastas do workspace não restaura os padrões). O [`EncodingOracle`] do porte é o real,
    /// com a lista trocada logo depois de criado; o escutador de mudança de pastas do workspace restauraria os padrões,
    /// mas o workspace de teste nunca muda.
    pub fn with_encoding_overrides(dependencies: TextFileServiceDependencies) -> Rc<Self> {
        Self::build(dependencies, TestServiceKind::BrowserWithEncodingOverrides)
    }

    /// `new TestNativeTextFileServiceWithEncodingOverrides(...)`: como [`TestTextFileService::with_encoding_overrides`], sobre o
    /// [`NativeTextFileService`] (o `super(...)` é o [`NativeTextFileService::new`]).
    pub fn with_native_encoding_overrides(dependencies: TextFileServiceDependencies) -> Rc<Self> {
        Self::build(dependencies, TestServiceKind::NativeWithEncodingOverrides)
    }

    fn build(dependencies: TextFileServiceDependencies, kind: TestServiceKind) -> Rc<Self> {
        let file_service = dependencies.file_service.clone();
        let manager = dependencies.files.clone();
        let inner = match kind {
            TestServiceKind::Test | TestServiceKind::BrowserWithEncodingOverrides => BrowserTextFileService::new(dependencies),
            TestServiceKind::NativeWithEncodingOverrides => NativeTextFileService::new(dependencies),
        };

        if kind != TestServiceKind::Test {
            inner.encoding_oracle().set_encoding_overrides(
                [("utf16le", UTF16LE), ("utf16be", UTF16BE), ("utf8bom", UTF8_WITH_BOM)]
                    .into_iter()
                    .map(|(extension, encoding)| EncodingOverride { parent: None, extension: Some(extension.to_string()), encoding: encoding.to_string() })
                    .collect(),
            );
        }

        Rc::new(Self { kind, inner, manager, file_service, read_stream_error: RefCell::new(None), write_error: RefCell::new(None) })
    }

    /// `files as ITestTextFileEditorModelManager`.
    pub fn files_manager(&self) -> Rc<TextFileEditorModelManager> {
        self.manager.clone()
    }

    /// `suggestFilename(languageId, untitledName)`.
    pub fn suggest_filename(&self, language_id: &str, untitled_name: &str) -> String {
        self.inner.suggest_filename(language_id, untitled_name)
    }

    /// `setReadStreamErrorOnce(error)`: aceita o `FileOperationError` e o erro já montado (o `TextFileOperationError`, que é um
    /// [`Error`] com a propriedade `textFileOperationResult`).
    pub fn set_read_stream_error_once(&self, error: impl Into<Error>) {
        *self.read_stream_error.borrow_mut() = Some(error.into());
    }

    /// `setWriteErrorOnce(error)`.
    pub fn set_write_error_once(&self, error: impl Into<Error>) {
        *self.write_error.borrow_mut() = Some(error.into());
    }
}

impl IDisposable for TestTextFileService {
    fn dispose(&self) {
        self.inner.dispose();
    }
}

impl ITextFileService for TestTextFileService {
    delegate_to_inner! {
        fn files(&self) -> Rc<dyn ITextFileEditorModelManager>;
        fn untitled(&self) -> Rc<dyn IUntitledTextEditorModelManager>;
        fn encoding(&self) -> Rc<dyn IResourceEncodings>;
        fn is_dirty(&self, resource: &URI) -> bool;
        fn save(&self, resource: &URI, options: Option<TextFileSaveOptions>) -> Promise<Option<URI>>;
        fn save_as(&self, resource: &URI, target_resource: Option<&URI>, options: Option<TextFileSaveAsOptions>) -> Promise<Option<URI>>;
        fn revert(&self, resource: &URI, options: Option<je_workbench::common::editor::RevertOptions>) -> Promise<()>;
        fn read(&self, resource: &URI, options: Option<ReadTextFileOptions>) -> Promise<TextFileContent>;
        fn create(&self, operations: Vec<TextFileCreateOperation>, undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>>;
        fn get_encoded_readable(&self, resource: Option<&URI>, value: Option<TextOrSnapshot>, options: Option<WriteTextFileOptions>) -> Promise<Option<EncodedContent>>;
        fn get_decoded_stream(&self, resource: Option<&URI>, value: VSBufferReadableStream, options: Option<ReadTextFileEncodingOptions>) -> Promise<Rc<dyn ReadableStream<String>>>;
        fn get_encoding(&self, resource: &URI) -> String;
        fn resolve_decoding(&self, resource: Option<&URI>, options: Option<ReadTextFileEncodingOptions>) -> Promise<ResolvedDecoding>;
        fn resolve_encoding(&self, resource: Option<&URI>, options: Option<WriteTextFileOptions>) -> Promise<ResolvedEncoding>;
        fn validate_detected_encoding(&self, resource: Option<&URI>, detected_encoding: &str, options: Option<ReadTextFileEncodingOptions>) -> Promise<String>;
    }

    fn read_stream(&self, resource: &URI, options: Option<ReadTextFileOptions>) -> Promise<TextFileStreamContent> {
        if self.kind != TestServiceKind::Test {
            return ITextFileService::read_stream(&*self.inner, resource, options);
        }

        let error = self.read_stream_error.borrow_mut().take();
        if let Some(error) = error {
            return Promise::rejected(error);
        }

        let (file_service, resource) = (self.file_service.clone(), resource.clone());
        async_fn(async move {
            let content = file_service.read_file_stream(&resource, options.map(|options| options.read), None).await?;

            let events: &dyn ReadableStreamEvents<VSBuffer> = &*content.value;
            let strings = transform(
                events,
                Transformer { data: Rc::new(|chunk: &VSBuffer| chunk.to_string()), error: None },
                Rc::new(|chunks: Vec<String>| chunks.join("")),
            );
            let value = create_text_buffer_factory_from_stream(&*strings).await?;

            let stat = &content.stat;
            Ok(TextFileStreamContent {
                base: BaseTextFileContent {
                    resource: stat.resource.clone(),
                    name: stat.name.clone(),
                    mtime: stat.mtime(),
                    ctime: stat.ctime(),
                    size: 10,
                    etag: stat.etag().to_string(),
                    readonly: false,
                    locked: false,
                    executable: false,
                    encoding: "utf8".to_string(),
                },
                value,
            })
        })
    }

    fn write(&self, resource: &URI, value: TextOrSnapshot, options: Option<WriteTextFileOptions>) -> Promise<FileStatWithMetadata> {
        if self.kind != TestServiceKind::Test {
            return ITextFileService::write(&*self.inner, resource, value, options);
        }

        let error = self.write_error.borrow_mut().take();
        if let Some(error) = error {
            return Promise::rejected(error);
        }

        ITextFileService::write(&*self.inner, resource, value, options)
    }
}

//#endregion

//#region TestInMemoryFileSystemProvider e os serviços do setup de `browserTextFileService.io.test.ts`

/// O `setTimeout(0)` do `timeout(0)` do `async.ts`.
fn timeout_zero() -> Promise<()> {
    Promise::with_executor(|resolver| {
        runtime::set_timeout(move || resolver.resolve(()), 0);
    })
}

/// `TestInMemoryFileSystemProvider extends InMemoryFileSystemProvider`: o provider em memória real com as capacidades
/// `FileReadWrite`, `PathCaseSensitive` e `FileReadStream`, e o `readFileStream` em trechos de 64 KiB. Sem herança, é a
/// composição sobre o [`InMemoryFileSystemProvider`], que repassa o restante do contrato.
pub struct TestInMemoryFileSystemProvider {
    inner: Rc<InMemoryFileSystemProvider>,
}

impl TestInMemoryFileSystemProvider {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { inner: InMemoryFileSystemProvider::new() })
    }

    /// Escreve o arquivo em trechos de 64 KiB, com um `timeout(0)` antes de cada um e antes do fim.
    async fn write_chunks(inner: Rc<InMemoryFileSystemProvider>, resource: URI, stream: Rc<WriteableStreamImpl<VSBuffer>>) -> Result<(), Error> {
        const BUFFER_SIZE: usize = 64 * 1024;

        let data = inner.read_file(&resource, false).await?.to_vec();

        let mut offset = 0;
        while offset < data.len() {
            timeout_zero().await?;
            let end = (offset + BUFFER_SIZE).min(data.len());
            if let Some(pending) = stream.write(VSBuffer::wrap(data[offset..end].to_vec())) {
                pending.await?;
            }
            offset += BUFFER_SIZE;
        }

        timeout_zero().await?;
        stream.end(None);

        Ok(())
    }
}

impl IDisposable for TestInMemoryFileSystemProvider {
    fn dispose(&self) {
        self.inner.dispose();
    }
}

impl FileSystemProvider for TestInMemoryFileSystemProvider {
    fn capabilities(&self) -> FileSystemProviderCapabilities {
        FileSystemProviderCapabilities::FILE_READ_WRITE | FileSystemProviderCapabilities::PATH_CASE_SENSITIVE | FileSystemProviderCapabilities::FILE_READ_STREAM
    }

    fn on_did_change_capabilities(&self) -> Event<()> {
        self.inner.on_did_change_capabilities()
    }

    fn on_did_change_file(&self) -> Event<Vec<FileChange>> {
        self.inner.on_did_change_file()
    }

    fn watch(&self, resource: &URI, opts: &WatchOptions) -> DisposableRef {
        self.inner.watch(resource, opts)
    }

    fn stat(&self, resource: &URI) -> Promise<Stat> {
        self.inner.stat(resource)
    }

    fn mkdir(&self, resource: &URI) -> Promise<()> {
        self.inner.mkdir(resource)
    }

    fn readdir(&self, resource: &URI) -> Promise<Vec<(String, FileType)>> {
        self.inner.readdir(resource)
    }

    fn delete(&self, resource: &URI, opts: &FileDeleteOptions) -> Promise<()> {
        self.inner.delete(resource, opts)
    }

    fn rename(&self, from: &URI, to: &URI, opts: &FileOverwriteOptions) -> Promise<()> {
        self.inner.rename(from, to, opts)
    }

    fn read_file(&self, resource: &URI, atomic: bool) -> Promise<VSBuffer> {
        self.inner.read_file(resource, atomic)
    }

    fn write_file(&self, resource: &URI, content: VSBuffer, opts: &FileWriteOptions) -> Promise<()> {
        self.inner.write_file(resource, content, opts)
    }

    fn read_file_stream(&self, resource: &URI, _opts: &FileReadStreamOptions, _token: &CancellationToken) -> VSBufferReadableStream {
        let stream = new_writeable_buffer_stream(None);

        let (inner, resource, writer) = (self.inner.clone(), resource.clone(), stream.clone());
        drop(async_fn(async move {
            if let Err(error) = Self::write_chunks(inner, resource, writer.clone()).await {
                writer.error(error);
                writer.end(None);
            }

            Ok(())
        }));

        stream
    }

    fn open(&self, resource: &URI, opts: &FileOpenOptions) -> Promise<i64> {
        self.inner.open(resource, opts)
    }

    fn close(&self, fd: i64) -> Promise<()> {
        self.inner.close(fd)
    }

    fn read(&self, fd: i64, pos: u64, data: &VSBuffer, offset: usize, length: usize) -> Promise<usize> {
        self.inner.read(fd, pos, data, offset, length)
    }

    fn write(&self, fd: i64, pos: u64, data: &VSBuffer, offset: usize, length: usize) -> Promise<usize> {
        self.inner.write(fd, pos, data, offset, length)
    }
}

/// Os serviços que `instantiationService.createChild(collection)` troca no `setup` de `browserTextFileService.io.test.ts`:
/// o `IFileService` e o `IWorkingCopyFileService`. Os demais vêm do `workbenchInstantiationService` pai.
pub struct ChildServices {
    pub file_service: Rc<dyn IFileService>,
    pub working_copy_file_service: Rc<dyn IWorkingCopyFileService>,
}

/// O `FileService` com o [`TestInMemoryFileSystemProvider`] registrado para o esquema `file`, e o
/// `WorkingCopyFileService` sobre ele (com um `WorkingCopyService` e um `UriIdentityService` próprios, como no original).
pub fn browser_io_child_services(disposables: &Rc<DisposableStore>) -> (ChildServices, Rc<TestInMemoryFileSystemProvider>) {
    let log_service = NullLogService::shared();
    let file_service = disposables.add_rc(FileService::new(log_service.clone()));

    let file_provider = disposables.add_rc(TestInMemoryFileSystemProvider::new());
    disposables.add(file_service.register_provider(schemas::FILE, file_provider.clone()).expect("o esquema file ainda não tem provider"));

    let file_service: Rc<dyn IFileService> = file_service;
    let working_copy_service = disposables.add_rc(WorkingCopyService::new());
    let uri_identity_service = disposables.add_rc(UriIdentityService::create((), (file_service.clone(),)));
    let working_copy_file_service =
        disposables.add_rc(TestWorkingCopyFileService::new(log_service, Rc::new(TestProgressService), file_service.clone(), working_copy_service, uri_identity_service));

    (ChildServices { file_service, working_copy_file_service }, file_provider)
}

//#endregion

//#region Dublês de serviço do workbenchInstantiationService

/// `TestDialogService` (sem resultado padrão).
pub struct TestDialogService;

impl IDialogService for TestDialogService {
    fn on_will_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn confirm(&self, _confirmation: Confirmation) -> Promise<ConfirmationResult> {
        Promise::resolved(ConfirmationResult { checkbox_checked: None, confirmed: false })
    }

    fn prompt(&self, prompt: Prompt) -> Promise<PromptResult> {
        let mut buttons: Vec<_> = prompt.buttons.unwrap_or_default().into_iter().map(|button| button.base).collect();
        if let Some(PromptCancel::Button(cancel_button)) = prompt.cancel_button {
            buttons.push(cancel_button.base);
        }

        match buttons.first().map(|button| (button.run)(CheckboxResult { checkbox_checked: Some(false) })) {
            None => Promise::resolved(PromptResult { checkbox_checked: None, result: None }),
            Some(MaybePromise::Value(result)) => Promise::resolved(PromptResult { checkbox_checked: None, result: Some(result) }),
            Some(MaybePromise::Promise(promise)) => promise.then(|result| PromptResult { checkbox_checked: None, result: Some(result) }),
        }
    }

    fn input(&self, _input: Input) -> Promise<InputResult> {
        Promise::resolved(InputResult { confirmation: ConfirmationResult { checkbox_checked: None, confirmed: true }, values: Some(Vec::new()) })
    }

    fn info(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn warn(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn error(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::resolved(())
    }

    fn about(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

/// `TestFileDialogService`.
pub struct TestFileDialogService {
    path_service: Rc<dyn IPathService>,
    confirm_result: Cell<ConfirmResult>,
    file_to_save: RefCell<Option<URI>>,
}

impl TestFileDialogService {
    pub fn new(path_service: Rc<dyn IPathService>) -> Rc<Self> {
        Rc::new(Self { path_service, confirm_result: Cell::new(ConfirmResult::Cancel), file_to_save: RefCell::new(None) })
    }

    /// `setPickFileToSave(path)`.
    pub fn set_pick_file_to_save(&self, path: URI) {
        *self.file_to_save.borrow_mut() = Some(path);
    }

    /// `setConfirmResult(result)`.
    pub fn set_confirm_result(&self, result: ConfirmResult) {
        self.confirm_result.set(result);
    }
}

impl IFileDialogService for TestFileDialogService {
    fn default_file_path(&self, _scheme_filter: Option<&str>) -> Promise<URI> {
        self.path_service.user_home()
    }

    fn default_folder_path(&self, _scheme_filter: Option<&str>) -> Promise<URI> {
        self.path_service.user_home()
    }

    fn default_workspace_path(&self, _scheme_filter: Option<&str>) -> Promise<URI> {
        self.path_service.user_home()
    }

    fn pick_file_folder_and_open(&self, _options: PickAndOpenOptions) -> Promise<()> {
        Promise::resolved(())
    }

    fn pick_file_and_open(&self, _options: PickAndOpenOptions) -> Promise<()> {
        Promise::resolved(())
    }

    fn pick_folder_and_open(&self, _options: PickAndOpenOptions) -> Promise<()> {
        Promise::resolved(())
    }

    fn pick_workspace_and_open(&self, _options: PickAndOpenOptions) -> Promise<()> {
        Promise::resolved(())
    }

    fn pick_file_to_save(&self, _default_uri: &URI, _available_file_systems: Option<Vec<String>>) -> Promise<Option<URI>> {
        Promise::resolved(self.file_to_save.borrow().clone())
    }

    fn preferred_home(&self, _scheme_filter: Option<&str>) -> Promise<URI> {
        self.path_service.user_home()
    }

    fn show_save_dialog(&self, _options: SaveDialogOptions) -> Promise<Option<URI>> {
        Promise::resolved(None)
    }

    fn show_save_confirm(&self, _file_names_or_resources: &[FileNameOrResource]) -> Promise<ConfirmResult> {
        Promise::resolved(self.confirm_result.get())
    }

    fn show_open_dialog(&self, _options: OpenDialogOptions) -> Promise<Option<Vec<URI>>> {
        Promise::resolved(None)
    }
}

/// `TestDecorationsService`.
pub struct TestDecorationsService;

impl IDecorationsService for TestDecorationsService {
    fn on_did_change_decorations(&self) -> Event<Rc<dyn IResourceDecorationChangeEvent>> {
        Event::none()
    }

    fn register_decorations_provider(&self, _provider: Rc<dyn IDecorationsProvider>) -> DisposableRef {
        je_base::lifecycle::none()
    }

    fn get_decoration(&self, _uri: &URI, _include_children: bool) -> Option<Rc<dyn IDecoration>> {
        None
    }
}

/// `TestCodeEditorService`: as propriedades transitórias dos modelos, por recurso.
#[derive(Default)]
pub struct TestCodeEditorService {
    properties: RefCell<HashMap<String, Vec<(String, Dynamic)>>>,
}

impl ICodeEditorTransientProperties for TestCodeEditorService {
    fn get_transient_model_properties(&self, model: &TextModel) -> Option<Vec<(String, Dynamic)>> {
        self.properties.borrow().get(&model.uri().to_string(false).to_string_lossy()).cloned()
    }

    fn set_transient_model_property(&self, model: &TextModel, key: &str, value: Dynamic) {
        let mut properties = self.properties.borrow_mut();
        let entries = properties.entry(model.uri().to_string(false).to_string_lossy()).or_default();
        match entries.iter_mut().find(|(candidate, _)| candidate == key) {
            Some(entry) => entry.1 = value,
            None => entries.push((key.to_string(), value)),
        }
    }
}

/// `TestLanguageDetectionService`: a detecção desligada.
struct TestLanguageDetectionService;

impl ILanguageDetectionService for TestLanguageDetectionService {
    fn is_enabled_for_language(&self, _language_id: &str) -> bool {
        false
    }

    fn detect_language(&self, _resource: &URI, _supported_langs: Option<&[String]>) -> Promise<Option<String>> {
        Promise::resolved(None)
    }
}

/// `TestAccessibilityService`.
struct TestAccessibilityService;

impl IAccessibilityService for TestAccessibilityService {
    fn on_did_change_screen_reader_optimized(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_change_reduced_motion(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_change_reduced_transparency(&self) -> Event<()> {
        Event::none()
    }

    fn always_underline_access_keys(&self) -> Promise<bool> {
        Promise::resolved(false)
    }

    fn is_screen_reader_optimized(&self) -> bool {
        false
    }

    fn is_motion_reduced(&self) -> bool {
        false
    }

    fn is_transparency_reduced(&self) -> bool {
        false
    }

    fn get_accessibility_support(&self) -> AccessibilitySupport {
        AccessibilitySupport::Unknown
    }

    fn set_accessibility_support(&self, _accessibility_support: AccessibilitySupport) {}

    fn alert(&self, _message: &str) {}

    fn status(&self, _message: &str) {}
}

//#endregion

//#region TestWorkingCopyService

/// `TestWorkingCopyService extends WorkingCopyService`: o serviço real mais o `testUnregisterWorkingCopy`.
pub struct TestWorkingCopyService {
    inner: Rc<WorkingCopyService>,
    /// O registro que `registerWorkingCopy` devolveu, por identidade da working copy.
    registrations: RefCell<Vec<(DisposableId, DisposableRef)>>,
}

impl TestWorkingCopyService {
    pub fn new(inner: Rc<WorkingCopyService>) -> Rc<Self> {
        Rc::new(Self { inner, registrations: RefCell::new(Vec::new()) })
    }

    /// `testUnregisterWorkingCopy(workingCopy)`.
    pub fn test_unregister_working_copy(&self, working_copy: &dyn IWorkingCopy) {
        let id = id_of_ref(working_copy);
        let registration = {
            let mut registrations = self.registrations.borrow_mut();
            registrations.iter().position(|(candidate, _)| *candidate == id).map(|index| registrations.remove(index).1)
        };

        if let Some(registration) = registration {
            registration.dispose();
        }
    }
}

impl IWorkingCopyService for TestWorkingCopyService {
    fn on_did_register(&self) -> Event<Rc<dyn IWorkingCopy>> {
        self.inner.on_did_register()
    }

    fn on_did_unregister(&self) -> Event<Rc<dyn IWorkingCopy>> {
        self.inner.on_did_unregister()
    }

    fn on_did_change_dirty(&self) -> Event<Rc<dyn IWorkingCopy>> {
        self.inner.on_did_change_dirty()
    }

    fn on_did_change_content(&self) -> Event<Rc<dyn IWorkingCopy>> {
        self.inner.on_did_change_content()
    }

    fn on_did_save(&self) -> Event<WorkingCopySaveEvent> {
        self.inner.on_did_save()
    }

    fn dirty_count(&self) -> usize {
        self.inner.dirty_count()
    }

    fn dirty_working_copies(&self) -> Vec<Rc<dyn IWorkingCopy>> {
        self.inner.dirty_working_copies()
    }

    fn modified_count(&self) -> usize {
        self.inner.modified_count()
    }

    fn modified_working_copies(&self) -> Vec<Rc<dyn IWorkingCopy>> {
        self.inner.modified_working_copies()
    }

    fn has_dirty(&self) -> bool {
        self.inner.has_dirty()
    }

    fn is_dirty(&self, resource: &URI, type_id: Option<&str>) -> bool {
        self.inner.is_dirty(resource, type_id)
    }

    fn working_copies(&self) -> Vec<Rc<dyn IWorkingCopy>> {
        self.inner.working_copies()
    }

    fn register_working_copy(&self, working_copy: Rc<dyn IWorkingCopy>) -> DisposableRef {
        let id = id_of(&working_copy);
        let registration = self.inner.register_working_copy(working_copy);
        self.registrations.borrow_mut().push((id, registration.clone()));

        registration
    }

    fn has(&self, identifier: &WorkingCopyIdentifier) -> bool {
        self.inner.has(identifier)
    }

    fn has_resource(&self, resource: &URI) -> bool {
        self.inner.has_resource(resource)
    }

    fn get(&self, identifier: &WorkingCopyIdentifier) -> Option<Rc<dyn IWorkingCopy>> {
        self.inner.get(identifier)
    }

    fn get_all(&self, resource: &URI) -> Option<Vec<Rc<dyn IWorkingCopy>>> {
        self.inner.get_all(resource)
    }
}

//#endregion

//#region TestReadonlyTextFileEditorModel

/// O `IFilesConfigurationService` do `TestReadonlyTextFileEditorModel`: tudo do serviço real, menos `isReadonly`, que é
/// sempre `true`.
struct ReadonlyFilesConfigurationService {
    inner: Rc<dyn IFilesConfigurationService>,
}

impl IFilesConfigurationService for ReadonlyFilesConfigurationService {
    fn on_did_change_auto_save_configuration(&self) -> Event<()> {
        self.inner.on_did_change_auto_save_configuration()
    }

    fn on_did_change_auto_save_disabled(&self) -> Event<URI> {
        self.inner.on_did_change_auto_save_disabled()
    }

    fn get_auto_save_configuration(&self, resource_or_editor: Option<&ResourceOrEditor>) -> IAutoSaveConfiguration {
        self.inner.get_auto_save_configuration(resource_or_editor)
    }

    fn has_short_auto_save_delay(&self, resource_or_editor: Option<&ResourceOrEditor>) -> bool {
        self.inner.has_short_auto_save_delay(resource_or_editor)
    }

    fn get_auto_save_mode(&self, resource_or_editor: Option<&ResourceOrEditor>, save_reason: Option<SaveReason>) -> IAutoSaveMode {
        self.inner.get_auto_save_mode(resource_or_editor, save_reason)
    }

    fn toggle_auto_save(&self) -> Promise<()> {
        self.inner.toggle_auto_save()
    }

    fn enable_auto_save_after_short_delay(&self, resource_or_editor: &ResourceOrEditor) -> DisposableRef {
        self.inner.enable_auto_save_after_short_delay(resource_or_editor)
    }

    fn disable_auto_save(&self, resource_or_editor: &ResourceOrEditor) -> DisposableRef {
        self.inner.disable_auto_save(resource_or_editor)
    }

    fn on_did_change_readonly(&self) -> Event<()> {
        self.inner.on_did_change_readonly()
    }

    fn is_readonly(&self, _resource: &URI, _stat: Option<&FileStat>) -> Readonly {
        Readonly::Bool(true)
    }

    fn update_readonly(&self, resource: &URI, readonly: SingleReadonlyUpdate) -> Promise<()> {
        self.inner.update_readonly(resource, readonly)
    }

    fn update_readonly_all(&self, resources: &[URI], readonly: ReadonlyUpdate) -> Promise<()> {
        self.inner.update_readonly_all(resources, readonly)
    }

    fn on_did_change_files_association(&self) -> Event<()> {
        self.inner.on_did_change_files_association()
    }

    fn is_hot_exit_enabled(&self) -> bool {
        self.inner.is_hot_exit_enabled()
    }

    fn hot_exit_configuration(&self) -> Option<String> {
        self.inner.hot_exit_configuration()
    }

    fn prevent_save_conflicts(&self, resource: &URI, language: Option<&str>) -> bool {
        self.inner.prevent_save_conflicts(resource, language)
    }
}

//#endregion

//#region TestTextModelService

/// `ReferenceCollection`: o contador e o objeto de uma referência (`{ counter, object }`).
struct ModelReference {
    counter: Rc<Cell<usize>>,
    object: Promise<Rc<TextFileEditorModel>>,
}

/// O `ITextModelService` do `workbenchInstantiationService` (o `TextModelResolverService` de
/// `textModelResolverService.ts`) para recursos de arquivo: a `ReferenceCollection` com a contagem de referências por
/// recurso canônico, o `files.resolve(resource, { reason: REFERENCE })` que cria o modelo (`doCreateReferencedObject`) e o
/// descarte do modelo depois do `canDispose` quando a última referência é solta (`destroyReferencedObject`, com o
/// `modelsToDispose` que cancela o descarte quando o recurso é adquirido de novo).
///
/// Divergências: o `TextModelResolverService` real ainda não foi portado. Os recursos `untitled`, em memória e de
/// provedores de conteúdo, a ativação tardia de provedores e a sincronização do `ModelUndoRedoParticipant` não entram: as
/// suítes de `FileEditorInput` só resolvem recursos de arquivo, e um recurso sem provedor de arquivo rejeita com o
/// `Unable to resolve resource` do original.
pub struct TestTextModelService {
    files: Rc<TextFileEditorModelManager>,
    file_service: Rc<dyn IFileService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    references: Rc<RefCell<HashMap<String, ModelReference>>>,
    models_to_dispose: Rc<RefCell<HashMap<String, Promise<Rc<TextFileEditorModel>>>>>,
}

impl TestTextModelService {
    pub fn new(files: Rc<TextFileEditorModelManager>, file_service: Rc<dyn IFileService>, uri_identity_service: Rc<dyn IUriIdentityService>) -> Self {
        Self { files, file_service, uri_identity_service, references: Rc::default(), models_to_dispose: Rc::default() }
    }

    /// `createReferencedObject(key)`: o `doCreateReferencedObject` para recursos de arquivo.
    fn create_referenced_object(&self, key: &str, resource: &URI) -> Promise<Rc<TextFileEditorModel>> {
        // Untrack as being disposed
        self.models_to_dispose.borrow_mut().remove(key);

        // File or remote file: go through text file service
        if self.file_service.has_provider(resource) {
            let resolve = self.files.resolve_model(
                resource,
                Some(TextFileEditorModelResolveOrCreateOptions {
                    resolve: TextFileResolveOptions { reason: Some(TextFileResolveReason::Reference), ..TextFileResolveOptions::default() },
                    ..TextFileEditorModelResolveOrCreateOptions::default()
                }),
            );

            let key = key.to_string();
            return async_fn(async move {
                let model = resolve.await?;
                if ITextFileEditorModel::text_editor_model(&*model).is_none() {
                    return Err(Error::new(format!("Unable to resolve resource {key}")));
                }

                Ok(model)
            });
        }

        Promise::rejected(Error::new(format!("Unable to resolve resource {key}")))
    }

    /// `destroyReferencedObject(key, modelPromise)`.
    fn destroy_referenced_object(
        files: Rc<TextFileEditorModelManager>,
        models_to_dispose: Rc<RefCell<HashMap<String, Promise<Rc<TextFileEditorModel>>>>>,
        key: String,
        model_promise: Promise<Rc<TextFileEditorModel>>,
    ) {
        // Track as being disposed before waiting for model to load
        // to handle the case that the reference is acquired again
        models_to_dispose.borrow_mut().insert(key.clone(), model_promise.clone());

        // O IIFE `async` do original espera o `modelPromise` antes de consultar o `modelsToDispose`: a decisão acontece numa
        // microtarefa (o `await modelPromise`).
        runtime::queue_microtask(move || {
            runtime::spawn_local(async move {
                // `catch` ignora o erro e o `finally` solta o recurso do `modelsToDispose`
                let _ = Self::dispose_when_possible(&files, &models_to_dispose, &key, model_promise).await;
                models_to_dispose.borrow_mut().remove(&key); // Untrack as being disposed
            });
        });
    }

    async fn dispose_when_possible(
        files: &Rc<TextFileEditorModelManager>,
        models_to_dispose: &Rc<RefCell<HashMap<String, Promise<Rc<TextFileEditorModel>>>>>,
        key: &str,
        model_promise: Promise<Rc<TextFileEditorModel>>,
    ) -> Result<(), Error> {
        let model = model_promise.await?;

        if !models_to_dispose.borrow().contains_key(key) {
            // return if model has been acquired again meanwhile
            return Ok(());
        }

        // text file models have conditions that prevent them
        // from dispose, so we have to wait until we can dispose
        let model_contract: Rc<dyn ITextFileEditorModel> = model.clone();
        if let Some(can_dispose) = ITextFileEditorModelManager::can_dispose(&**files, model_contract) {
            can_dispose.await?;
        }

        if !models_to_dispose.borrow().contains_key(key) {
            // return if model has been acquired again meanwhile
            return Ok(());
        }

        // Finally we can dispose the model
        model.dispose();

        Ok(())
    }
}

impl ITextModelService for TestTextModelService {
    fn create_model_reference(&self, resource: &URI) -> Promise<TextModelReference> {
        // From this moment on, only operate on the canonical resource
        // to ensure we reduce the chance of resolving the same resource
        // with different resource forms (e.g. path casing on Windows)
        let resource = self.uri_identity_service.as_canonical_uri(resource);
        let key = resource.to_string(false).to_string_lossy();

        // `ReferenceCollection.acquire(key)`
        let existing = self.references.borrow().get(&key).map(|reference| (reference.counter.clone(), reference.object.clone()));
        let (counter, object) = match existing {
            Some(existing) => existing,
            None => {
                let object = self.create_referenced_object(&key, &resource);
                let counter = Rc::new(Cell::new(0));
                self.references.borrow_mut().insert(key.clone(), ModelReference { counter: counter.clone(), object: object.clone() });

                (counter, object)
            }
        };
        counter.set(counter.get() + 1);

        let release = {
            let (files, models_to_dispose, references) = (self.files.clone(), self.models_to_dispose.clone(), self.references.clone());
            let (key, counter, object) = (key.clone(), counter.clone(), object.clone());
            to_disposable(move || {
                counter.set(counter.get() - 1);
                if counter.get() == 0 {
                    Self::destroy_referenced_object(files, models_to_dispose, key.clone(), object);
                    references.borrow_mut().remove(&key);
                }
            })
        };

        // `AsyncReferenceCollection.acquire`
        async_fn(async move {
            match object.await {
                Ok(model) => {
                    let object: Rc<dyn ITextEditorModel> = model;
                    Ok(TextModelReference::new(object, release))
                }
                Err(error) => {
                    release.dispose();

                    Err(error)
                }
            }
        })
    }
}

//#endregion

//#region TestServiceAccessor

/// A fábrica de modelos: `instantiationService.createInstance(TextFileEditorModel, resource, encoding, languageId)`.
type ModelFactory = Rc<dyn Fn(URI, Option<String>, Option<String>) -> Rc<TextFileEditorModel>>;

/// Os 14 serviços que o construtor do `TextFileEditorModel` injeta.
type ServicesFactory = Rc<dyn Fn() -> TextFileEditorModelServices>;

/// `workbenchInstantiationService(undefined, disposables)` mais o `TestServiceAccessor` do `workbenchTestServices.ts`, com os
/// serviços que o serviço de arquivos de texto e os modelos leem. Os demais serviços (`fileService`, `workingCopyService`,
/// `lifecycleService`, ...) vêm do [`TestServiceAccessor`] por `Deref`.
pub struct TextFileTestAccessor {
    base: TestServiceAccessor,
    /// `textFileService`.
    pub text_file_service: Rc<TestTextFileService>,
    /// `modelService`.
    pub model_service: Rc<ModelService>,
    /// `languageService`.
    pub language_service: Rc<LanguageService>,
    /// `dialogService`.
    pub dialog_service: Rc<TestDialogService>,
    /// `fileDialogService`.
    pub file_dialog_service: Rc<TestFileDialogService>,
    /// `untitledTextEditorService`.
    pub untitled_text_editor_service: Rc<UntitledTextEditorService>,
    /// `contextService`.
    pub context_service: Rc<TestContextService>,
    /// `testConfigurationService`.
    pub test_configuration_service: Rc<TestConfigurationService>,
    /// `workingCopyService` (o `TestWorkingCopyService`, que tem o `testUnregisterWorkingCopy`).
    pub working_copy_service: Rc<TestWorkingCopyService>,
    model_factory: ModelFactory,
    services_factory: ServicesFactory,
    /// O `IWorkingCopyFileService` que o gerenciador e o serviço de arquivos de texto leem: o dublê do
    /// [`TestServiceAccessor`] em [`TextFileTestAccessor::new`], ou o que [`TextFileTestAccessor::with_working_copy_file_service`]
    /// criou (o `WorkingCopyFileService` real, que é o `accessor.workingCopyFileService` do original).
    pub working_copy_file_service_contract: Rc<dyn IWorkingCopyFileService>,
    /// O `instantiationService` do `workbenchInstantiationService`: tem os serviços que a `FileEditorInput` injeta.
    pub instantiation_service: Rc<InstantiationService>,
    /// A coleção de serviços do [`TextFileTestAccessor::instantiation_service`] (o que `workbenchInstantiationService({ ... })`
    /// recebe como sobrescritas, como o `textEditorService`).
    pub collection: ServiceCollection,
}

impl Deref for TextFileTestAccessor {
    type Target = TestServiceAccessor;

    fn deref(&self) -> &TestServiceAccessor {
        &self.base
    }
}

impl TextFileTestAccessor {
    /// `workbenchInstantiationService(undefined, disposables)` e `instantiationService.createInstance(TestServiceAccessor)`.
    pub fn new(disposables: &Rc<DisposableStore>) -> Self {
        Self::build(disposables, None, TestServiceKind::Test, |base| -> Rc<dyn IWorkingCopyFileService> { base.working_copy_file_service.clone() })
    }

    /// Como [`TextFileTestAccessor::new`], com o `IWorkingCopyFileService` que `make_working_copy_file_service` cria sobre os
    /// serviços do [`TestServiceAccessor`] no lugar do dublê dele (o `WorkingCopyFileService` real, nas suítes que o testam).
    pub fn with_working_copy_file_service(
        disposables: &Rc<DisposableStore>,
        make_working_copy_file_service: impl FnOnce(&TestServiceAccessor) -> Rc<dyn IWorkingCopyFileService>,
    ) -> Self {
        Self::build(disposables, None, TestServiceKind::Test, make_working_copy_file_service)
    }

    /// O mesmo acessor, mas com o serviço de arquivos de texto criado numa `createChild(collection)` que troca o
    /// `IFileService` e o `IWorkingCopyFileService` (o `setup` de `browserTextFileService.io.test.ts`), e com a classe
    /// `TestBrowserTextFileServiceWithEncodingOverrides` no lugar da `TestTextFileService`.
    pub fn with_child_services(disposables: &Rc<DisposableStore>, child: ChildServices) -> Self {
        let child_working_copy_file_service = child.working_copy_file_service.clone();
        Self::build(disposables, Some(child), TestServiceKind::BrowserWithEncodingOverrides, move |_| child_working_copy_file_service)
    }

    /// Como [`TextFileTestAccessor::with_child_services`], com a classe `TestNativeTextFileServiceWithEncodingOverrides` (o
    /// `setup` de `nativeTextFileService.test.ts` e de `nativeTextFileService.io.test.ts`).
    pub fn with_native_child_services(disposables: &Rc<DisposableStore>, child: ChildServices) -> Self {
        let child_working_copy_file_service = child.working_copy_file_service.clone();
        Self::build(disposables, Some(child), TestServiceKind::NativeWithEncodingOverrides, move |_| child_working_copy_file_service)
    }

    fn build(
        disposables: &Rc<DisposableStore>,
        child: Option<ChildServices>,
        service_kind: TestServiceKind,
        make_working_copy_file_service: impl FnOnce(&TestServiceAccessor) -> Rc<dyn IWorkingCopyFileService>,
    ) -> Self {
        let base = TestServiceAccessor::new(disposables);
        let file_service: Rc<dyn IFileService> = match &child {
            Some(child) => child.file_service.clone(),
            None => base.file_service.clone(),
        };
        let working_copy_file_service = make_working_copy_file_service(&base);

        let test_configuration_service = TestConfigurationService::new(None);
        disposables.add_rc(test_configuration_service.on_did_change_configuration_emitter.clone());
        let configuration_service: Rc<dyn IConfigurationService> = test_configuration_service.clone();

        let language_service = disposables.add_rc(LanguageService::new(false));
        let model_service = disposables.add_rc(ModelService::new(
            configuration_service.clone(),
            TestTextResourcePropertiesService::new(configuration_service.clone()),
            UndoRedoService::for_tests(),
        ));
        let text_resource_configuration_service: Rc<dyn ITextResourceConfigurationService> = TestTextResourceConfigurationService::new(configuration_service.clone());
        let context_service = TestContextService::new(None, None);
        let environment_service: Rc<dyn IWorkbenchEnvironmentService> = TestEnvironmentService::new();
        let path_service: Rc<dyn IPathService> = TestPathService::new(None, None);
        let dialog_service = Rc::new(TestDialogService);
        let file_dialog_service = TestFileDialogService::new(path_service.clone());
        let language_detection_service: Rc<dyn ILanguageDetectionService> = Rc::new(TestLanguageDetectionService);
        let accessibility_service: Rc<dyn IAccessibilityService> = Rc::new(TestAccessibilityService);
        let extension_service = TestExtensionService::new();

        let collection = ServiceCollection::new();
        let instantiation_service = disposables.add_rc(InstantiationService::new(collection.clone()));
        let untitled_text_editor_service = disposables.add_rc(UntitledTextEditorService::new(instantiation_service.clone(), configuration_service.clone()));

        // Os modelos precisam do próprio `ITextFileService`, que só existe depois do gerenciador que os cria.
        let text_file_service_slot: Rc<RefCell<Weak<TestTextFileService>>> = Rc::new(RefCell::new(Weak::new()));
        let working_copy_service = TestWorkingCopyService::new(base.working_copy_service.clone());

        // Os serviços que o `UntitledTextEditorModel` injeta (o `UntitledTextEditorService` o cria por `createInstance`).
        collection.set(I_LANGUAGE_SERVICE, language_service.clone() as Rc<dyn ILanguageService>);
        collection.set(I_MODEL_SERVICE, model_service.clone() as Rc<dyn IModelService>);
        collection.set(I_WORKING_COPY_BACKUP_SERVICE, base.working_copy_backup_service.clone() as Rc<dyn IWorkingCopyBackupService>);
        collection.set(I_TEXT_RESOURCE_CONFIGURATION_SERVICE, text_resource_configuration_service.clone());
        collection.set(I_WORKING_COPY_SERVICE, working_copy_service.clone() as Rc<dyn IWorkingCopyService>);
        collection.set(I_LABEL_SERVICE, base.label_service.clone() as Rc<dyn ILabelService>);
        collection.set(I_EDITOR_SERVICE, base.editor_service.clone() as Rc<dyn IEditorService>);
        collection.set(I_LANGUAGE_DETECTION_SERVICE, language_detection_service.clone());
        collection.set(I_ACCESSIBILITY_SERVICE, accessibility_service.clone());

        let make_services_factory = |file_service: Rc<dyn IFileService>| -> ServicesFactory {
            let slot = text_file_service_slot.clone();
            let (language_service, model_service) = (language_service.clone(), model_service.clone());
            let log_service = base.log_service.clone();
            let (working_copy_backup_service, working_copy_service) = (base.working_copy_backup_service.clone(), working_copy_service.clone());
            let (files_configuration_service, label_service) = (base.files_configuration_service.clone(), base.label_service.clone());
            let (path_service, progress_service) = (path_service.clone(), base.progress_service.clone());
            let (language_detection_service, accessibility_service, extension_service) = (language_detection_service.clone(), accessibility_service.clone(), extension_service.clone());
            Rc::new(move || {
                let text_file_service = slot.borrow().upgrade().expect("o serviço de arquivos de texto está vivo enquanto cria modelos");
                TextFileEditorModelServices {
                    language_service: language_service.clone(),
                    model_service: model_service.clone(),
                    file_service: file_service.clone(),
                    text_file_service,
                    working_copy_backup_service: working_copy_backup_service.clone(),
                    log_service: log_service.clone(),
                    working_copy_service: working_copy_service.clone(),
                    files_configuration_service: files_configuration_service.clone(),
                    label_service: label_service.clone(),
                    language_detection_service: language_detection_service.clone(),
                    accessibility_service: accessibility_service.clone(),
                    path_service: path_service.clone(),
                    extension_service: extension_service.clone(),
                    progress_service: progress_service.clone(),
                }
            })
        };
        let make_model_factory = |services_factory: ServicesFactory| -> ModelFactory {
            Rc::new(move |resource, preferred_encoding, preferred_language_id| {
                TextFileEditorModel::new(resource, preferred_encoding, preferred_language_id, services_factory())
            })
        };

        // O gerenciador cria os modelos na `createChild(collection)` (o `IFileService` dela, quando há).
        let manager_model_factory = make_model_factory(make_services_factory(file_service.clone()));

        // Os modelos que o caso cria por `instantiationService.createInstance(TextFileEditorModel, ...)` usam os serviços do pai
        // (o `TestFileService`) quando o serviço de arquivos de texto é o do filho nativo (ver as divergências do módulo).
        let services_factory = match service_kind {
            TestServiceKind::NativeWithEncodingOverrides => make_services_factory(base.file_service.clone() as Rc<dyn IFileService>),
            TestServiceKind::Test | TestServiceKind::BrowserWithEncodingOverrides => make_services_factory(file_service.clone()),
        };
        let model_factory = make_model_factory(services_factory.clone());

        let manager = TextFileEditorModelManager::new(
            file_service.clone(),
            base.notification_service.clone(),
            working_copy_file_service.clone(),
            base.uri_identity_service.clone(),
            TextFileSaveParticipant::new(base.log_service.clone(), base.progress_service.clone()),
            manager_model_factory,
        );


        let dependencies = TextFileServiceDependencies {
            file_service,
            untitled_text_editor_service: untitled_text_editor_service.clone(),
            lifecycle_service: base.lifecycle_service.clone() as Rc<dyn ILifecycleService>,
            model_service: model_service.clone(),
            environment_service,
            dialog_service: dialog_service.clone(),
            file_dialog_service: file_dialog_service.clone(),
            text_resource_configuration_service: text_resource_configuration_service.clone(),
            files_configuration_service: base.files_configuration_service.clone(),
            code_editor_service: Rc::new(TestCodeEditorService::default()),
            path_service: path_service.clone(),
            working_copy_file_service: working_copy_file_service.clone(),
            uri_identity_service: base.uri_identity_service.clone(),
            workspace_context_service: context_service.clone() as Rc<dyn IWorkspaceContextService>,
            language_service: language_service.clone(),
            log_service: base.log_service.clone(),
            elevated_file_service: base.elevated_file_service.clone(),
            decorations_service: Rc::new(TestDecorationsService),
            files: manager.clone(),
        };
        let text_file_service = disposables.add_rc(match service_kind {
            TestServiceKind::Test => TestTextFileService::new(dependencies),
            TestServiceKind::BrowserWithEncodingOverrides => TestTextFileService::with_encoding_overrides(dependencies),
            TestServiceKind::NativeWithEncodingOverrides => TestTextFileService::with_native_encoding_overrides(dependencies),
        });
        *text_file_service_slot.borrow_mut() = Rc::downgrade(&text_file_service);

        let custom_editor_label_service = disposables.add_rc(CustomEditorLabelService::create((), (test_configuration_service.clone(), context_service.clone())));
        let text_model_service = Rc::new(TestTextModelService::new(manager, base.file_service.clone(), base.uri_identity_service.clone()));

        collection.set(I_CONFIGURATION_SERVICE, configuration_service);
        collection.set(I_TEXT_FILE_SERVICE, text_file_service.clone() as Rc<dyn ITextFileService>);
        collection.set(I_TEXT_MODEL_SERVICE, text_model_service as Rc<dyn ITextModelService>);
        collection.set(I_TEXT_RESOURCE_CONFIGURATION_SERVICE, text_resource_configuration_service);
        collection.set(I_LABEL_SERVICE, base.label_service.clone() as Rc<dyn ILabelService>);
        collection.set(I_FILE_SERVICE, base.file_service.clone() as Rc<dyn IFileService>);
        collection.set(I_FILES_CONFIGURATION_SERVICE, base.files_configuration_service.clone() as Rc<dyn IFilesConfigurationService>);
        collection.set(I_EDITOR_SERVICE, base.editor_service.clone() as Rc<dyn IEditorService>);
        collection.set(I_PATH_SERVICE, path_service);
        collection.set(I_CUSTOM_EDITOR_LABEL_SERVICE, custom_editor_label_service as Rc<dyn ICustomEditorLabelService>);

        Self {
            base,
            text_file_service,
            model_service,
            language_service,
            dialog_service,
            file_dialog_service,
            untitled_text_editor_service,
            context_service,
            test_configuration_service,
            working_copy_service,
            model_factory,
            services_factory,
            working_copy_file_service_contract: working_copy_file_service,
            instantiation_service,
            collection,
        }
    }

    /// `createFileEditorInput(instantiationService, resource)` de `workbenchTestServices.ts`:
    /// `instantiationService.createInstance(FileEditorInput, resource, undefined, undefined, undefined, undefined, undefined,
    /// undefined)`.
    pub fn create_file_editor_input(&self, resource: &URI) -> Rc<FileEditorInput> {
        self.instantiation_service.create_instance::<FileEditorInput>((resource.clone(), None, None, None, None, None, None))
    }

    /// `instantiationService.createInstance(TestReadonlyTextFileEditorModel, resource, preferredEncoding, preferredLanguageId)`.
    pub fn create_readonly_model(&self, resource: &URI, preferred_encoding: Option<&str>, preferred_language_id: Option<&str>) -> Rc<TextFileEditorModel> {
        let mut services = (self.services_factory)();
        services.files_configuration_service = Rc::new(ReadonlyFilesConfigurationService { inner: services.files_configuration_service.clone() });

        TextFileEditorModel::new(resource.clone(), preferred_encoding.map(str::to_string), preferred_language_id.map(str::to_string), services)
    }

    /// `instantiationService.createInstance(TextFileEditorModel, resource, preferredEncoding, preferredLanguageId)`.
    pub fn create_model(&self, resource: &URI, preferred_encoding: Option<&str>, preferred_language_id: Option<&str>) -> Rc<TextFileEditorModel> {
        (self.model_factory)(resource.clone(), preferred_encoding.map(str::to_string), preferred_language_id.map(str::to_string))
    }

    /// `accessor.textFileService.files as ITestTextFileEditorModelManager`.
    pub fn files(&self) -> Rc<TextFileEditorModelManager> {
        self.text_file_service.files_manager()
    }
}

//#endregion
