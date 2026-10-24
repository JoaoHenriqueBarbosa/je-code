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

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::buffer::{VSBuffer, VSBufferReadableStream};
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::{DisposableId, DisposableRef, DisposableStore, IDisposable, id_of, id_of_ref};
use je_base::promise::{MaybePromise, Promise};
use je_base::stream::{ReadableStream, ReadableStreamEvents, Transformer, transform};
use je_base::uri::URI;
use je_editor::language_service::{ILanguageService, LanguageService};
use je_editor::model_service::ModelService;
use je_editor::services_model::IModelService;
use je_editor::text_model::TextModel;
use je_editor::text_resource_configuration_service::ITextResourceConfigurationService;
use je_editor::undo_redo_service::UndoRedoService;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::accessibility::{AccessibilitySupport, IAccessibilityService};
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::dialogs::dialogs::{
    CheckboxResult, ConfirmResult, Confirmation, ConfirmationResult, FileNameOrResource, IDialogService, IFileDialogService, Input, InputResult, OpenDialogOptions, PickAndOpenOptions,
    Prompt, PromptCancel, PromptResult, SaveDialogOptions,
};
use je_platform::files::files::{FileOperationError, FileStat, FileStatWithMetadata};
use je_platform::files::i_file_service::IFileService;
use je_platform::path::path_service::IPathService;
use je_platform::workspace::IWorkspaceContextService;
use je_workbench::common::editor::SaveReason;
use je_workbench::services::files_configuration::files_configuration_service::{
    IAutoSaveConfiguration, IAutoSaveMode, IFilesConfigurationService, Readonly, ReadonlyUpdate, ResourceOrEditor, SingleReadonlyUpdate,
};
use je_workbench::services::working_copy::working_copy::{IWorkingCopy, WorkingCopyIdentifier};
use je_workbench::services::working_copy::working_copy_service::{IWorkingCopyService, WorkingCopySaveEvent, WorkingCopyService};
use je_workbench::services::decorations::decorations::{IDecoration, IDecorationsProvider, IDecorationsService, IResourceDecorationChangeEvent};
use je_workbench::services::environment::environment_service::IWorkbenchEnvironmentService;
use je_workbench::services::language_detection::language_detection_worker_service::ILanguageDetectionService;
use je_workbench::services::lifecycle::lifecycle::ILifecycleService;
use je_workbench::services::textfile::abstract_text_file_service::{AbstractTextFileService, TextFileServiceDependencies};
use je_workbench::services::textfile::browser_text_file_service::BrowserTextFileService;
use je_workbench::services::textfile::collaborators::{FileOperationUndoRedoInfo, ICodeEditorTransientProperties, IWorkingCopyFileService};
use je_workbench::services::textfile::text_buffer_factory::create_text_buffer_factory_from_stream;
use je_workbench::services::textfile::text_file_editor_model::{TextFileEditorModel, TextFileEditorModelServices};
use je_workbench::services::textfile::text_file_editor_model_manager::TextFileEditorModelManager;
use je_workbench::services::textfile::text_file_save_participant::TextFileSaveParticipant;
use je_workbench::services::textfile::textfiles::{
    BaseTextFileContent, EncodedContent, I_TEXT_FILE_SERVICE, IResourceEncodings, ITextFileEditorModelManager, ITextFileService, ReadTextFileEncodingOptions, ReadTextFileOptions,
    ResolvedDecoding, ResolvedEncoding, TextFileContent, TextFileCreateOperation, TextFileSaveAsOptions, TextFileSaveOptions, TextFileStreamContent, TextOrSnapshot,
    WriteTextFileOptions,
};
use je_workbench::services::untitled::untitled_text_editor_service::{IUntitledTextEditorModelManager, UntitledTextEditorService};
use je_workbench::test::browser::workbench_test_services::{TestEnvironmentService, TestExtensionService, TestPathService};
use je_workbench::test::common::workbench_test_services::{TestContextService, TestTextResourceConfigurationService, TestTextResourcePropertiesService};

use super::stored_file_working_copy_support::TestServiceAccessor;

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

/// `TestTextFileService`.
pub struct TestTextFileService {
    inner: Rc<AbstractTextFileService>,
    manager: Rc<TextFileEditorModelManager>,
    file_service: Rc<dyn IFileService>,
    read_stream_error: RefCell<Option<FileOperationError>>,
    write_error: RefCell<Option<FileOperationError>>,
}

impl TestTextFileService {
    /// `constructor(...)`: o `super(...)` é o [`BrowserTextFileService::new`].
    pub fn new(dependencies: TextFileServiceDependencies) -> Rc<Self> {
        let file_service = dependencies.file_service.clone();
        let manager = dependencies.files.clone();
        let inner = BrowserTextFileService::new(dependencies);

        Rc::new(Self { inner, manager, file_service, read_stream_error: RefCell::new(None), write_error: RefCell::new(None) })
    }

    /// `files as ITestTextFileEditorModelManager`.
    pub fn files_manager(&self) -> Rc<TextFileEditorModelManager> {
        self.manager.clone()
    }

    /// `setReadStreamErrorOnce(error)`.
    pub fn set_read_stream_error_once(&self, error: FileOperationError) {
        *self.read_stream_error.borrow_mut() = Some(error);
    }

    /// `setWriteErrorOnce(error)`.
    pub fn set_write_error_once(&self, error: FileOperationError) {
        *self.write_error.borrow_mut() = Some(error);
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
        let error = self.read_stream_error.borrow_mut().take();
        if let Some(error) = error {
            return Promise::rejected(error.into_error());
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
        let error = self.write_error.borrow_mut().take();
        if let Some(error) = error {
            return Promise::rejected(error.into_error());
        }

        ITextFileService::write(&*self.inner, resource, value, options)
    }
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
        Self::with_working_copy_file_service(disposables, |base| -> Rc<dyn IWorkingCopyFileService> { base.working_copy_file_service.clone() })
    }

    /// Como [`TextFileTestAccessor::new`], com o `IWorkingCopyFileService` que `make_working_copy_file_service` cria sobre os
    /// serviços do [`TestServiceAccessor`] no lugar do dublê dele (o `WorkingCopyFileService` real, nas suítes que o testam).
    pub fn with_working_copy_file_service(
        disposables: &Rc<DisposableStore>,
        make_working_copy_file_service: impl FnOnce(&TestServiceAccessor) -> Rc<dyn IWorkingCopyFileService>,
    ) -> Self {
        let base = TestServiceAccessor::new(disposables);
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
        let text_resource_configuration_service = TestTextResourceConfigurationService::new(configuration_service.clone());
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
        let untitled_text_editor_service = disposables.add_rc(UntitledTextEditorService::new(instantiation_service, configuration_service.clone()));

        // Os modelos precisam do próprio `ITextFileService`, que só existe depois do gerenciador que os cria.
        let text_file_service_slot: Rc<RefCell<Weak<TestTextFileService>>> = Rc::new(RefCell::new(Weak::new()));
        let working_copy_service = TestWorkingCopyService::new(base.working_copy_service.clone());
        let services_factory: ServicesFactory = {
            let slot = text_file_service_slot.clone();
            let (language_service, model_service) = (language_service.clone(), model_service.clone());
            let (file_service, log_service) = (base.file_service.clone(), base.log_service.clone());
            let (working_copy_backup_service, working_copy_service) = (base.working_copy_backup_service.clone(), working_copy_service.clone());
            let (files_configuration_service, label_service) = (base.files_configuration_service.clone(), base.label_service.clone());
            let (path_service, progress_service) = (path_service.clone(), base.progress_service.clone());
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
        let model_factory: ModelFactory = {
            let services_factory = services_factory.clone();
            Rc::new(move |resource, preferred_encoding, preferred_language_id| {
                TextFileEditorModel::new(resource, preferred_encoding, preferred_language_id, services_factory())
            })
        };

        let manager = TextFileEditorModelManager::new(
            base.file_service.clone(),
            base.notification_service.clone(),
            working_copy_file_service.clone(),
            base.uri_identity_service.clone(),
            TextFileSaveParticipant::new(base.log_service.clone(), base.progress_service.clone()),
            model_factory.clone(),
        );

        let text_file_service = disposables.add_rc(TestTextFileService::new(TextFileServiceDependencies {
            file_service: base.file_service.clone(),
            untitled_text_editor_service: untitled_text_editor_service.clone(),
            lifecycle_service: base.lifecycle_service.clone() as Rc<dyn ILifecycleService>,
            model_service: model_service.clone(),
            environment_service,
            dialog_service: dialog_service.clone(),
            file_dialog_service: file_dialog_service.clone(),
            text_resource_configuration_service: text_resource_configuration_service as Rc<dyn ITextResourceConfigurationService>,
            files_configuration_service: base.files_configuration_service.clone(),
            code_editor_service: Rc::new(TestCodeEditorService::default()),
            path_service,
            working_copy_file_service: working_copy_file_service.clone(),
            uri_identity_service: base.uri_identity_service.clone(),
            workspace_context_service: context_service.clone() as Rc<dyn IWorkspaceContextService>,
            language_service: language_service.clone(),
            log_service: base.log_service.clone(),
            elevated_file_service: base.elevated_file_service.clone(),
            decorations_service: Rc::new(TestDecorationsService),
            files: manager,
        }));
        *text_file_service_slot.borrow_mut() = Rc::downgrade(&text_file_service);

        collection.set(I_CONFIGURATION_SERVICE, configuration_service);
        collection.set(I_TEXT_FILE_SERVICE, text_file_service.clone() as Rc<dyn ITextFileService>);

        Self { base, text_file_service, model_service, language_service, dialog_service, file_dialog_service, untitled_text_editor_service, context_service, test_configuration_service, working_copy_service, model_factory, services_factory, working_copy_file_service_contract: working_copy_file_service }
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
