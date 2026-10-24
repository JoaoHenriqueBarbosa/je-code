//! Porte de `src/vs/workbench/services/untitled/test/browser/untitledTextEditor.test.ts` (suíte `Untitled text editors`), caso a caso e na mesma ordem.
//!
//! O arquivo traz também a segunda suíte do original (`UntitledTextEditorWorkingCopyEditorHandler`), logo depois da
//! primeira.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de
//!   cada caso (`LeakGuard`), e o `setup` ([`setup`]) roda logo depois; o `disposables` do original é o store do guarda.
//! - **`workbenchInstantiationService(undefined, disposables)` e `TestServiceAccessor`.** O [`Fixture`] monta o
//!   `InstantiationService` com os serviços que os casos usam: os do [`WorkbenchServices`], o `LanguageService` e o
//!   `ModelService` reais, o `WorkingCopyService` real, o `InMemoryWorkingCopyBackupService`, o
//!   `UntitledTextEditorService` real e os dublês locais. `instantiationService.createInstance(X, ...)` é o
//!   `create_instance::<X>(...)`, e `accessor.untitledTextEditorService`, `accessor.workingCopyService`,
//!   `accessor.testConfigurationService`, `accessor.editorService` e `accessor.languageService` são campos do [`Fixture`].
//! - **`TestTextFileService`.** O `TextFileService` abstrato ainda não foi portado: o [`TestTextFileService`] traz o que o
//!   `UntitledTextEditorModel` e a entrada leem dele (`untitled`, `revert` de recurso sem título, `getDecodedStream` e
//!   `getEncodedReadable` com o encoding `utf8` que o `EncodingOracle` devolve quando `files.encoding` não está
//!   configurado), e as demais operações reprovam o caso com o nome da operação, porque os casos não as exercitam.
//! - **`ITextModelService`.** O `TextModelResolverService` real ainda não foi portado: o [`TestTextModelService`] faz o que
//!   ele faz para o esquema `untitled` (`textFileService.untitled.resolve({ untitledResource })`), com a contagem de
//!   referências do `ReferenceCollection` e o descarte do modelo depois do `canDispose` (`destroyReferencedObject`).
//! - **`IWorkingCopyEditorService` e `ITextEditorService`.** O [`TestWorkingCopyEditorService`] guarda os handlers
//!   registrados (`registerHandler` e `onDidRegisterHandler`); o `findEditor` e o `ITextEditorService` não são exercitados
//!   pelos casos.
//! - **`accessor.editorService.activeTextEditorLanguageId = ...`.** O [`TestEditorServiceWithActiveLanguage`] é o
//!   `TestEditorService` com a propriedade atribuível.
//! - **Casos `async`** rodam em `run_with_faked_timers`; `await` é `.await.unwrap()` (uma rejeição reprova o caso). Os que
//!   não têm `await` rodam direto.
//! - **`disposables.add(x)`** devolve `x` tipado no original; aqui o [`LeakGuard::add`] devolve a referência genérica, então
//!   [`track_model`] e [`resolve_and_track`] registram e devolvem o modelo.
//! - **`TestUntitledTextEditorInput.getModel()`** é o `UntitledTextEditorInput::model`, que já é público.
//! - **`input.resource`** é `EditorInput::resource`, e `model.resource` é `IWorkingCopy::resource` (ver [`input_resource`]).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod editor_input_services;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use editor_input_services::{TestAccessibilityService, TestLanguageDetectionService, WorkbenchServices};
use je_base::JsString;
use je_base::buffer::{VSBufferReadableStream, readable_to_buffer, stream_to_buffer};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::network::schemas;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::promise::{MaybePromise, Promise};
use je_base::runtime;
use je_base::stream::ReadableStream;
use je_base::uri::{URI, UriComponents};
use je_editor::edit_operation::SingleEditOperation;
use je_editor::language_service::{ILanguageService, I_LANGUAGE_SERVICE, LanguageService};
use je_editor::model_service::ModelService;
use je_editor::modes_registry::{LanguageExtensionPoint, PLAINTEXT_LANGUAGE_ID};
use je_editor::range::Range;
use je_editor::services_model::{IModelService, I_MODEL_SERVICE};
use je_editor::text_model::{TextModel, TextSource};
use je_editor::text_resource_configuration_service::{ITextResourceConfigurationService, I_TEXT_RESOURCE_CONFIGURATION_SERVICE};
use je_editor::undo_redo_service::UndoRedoService;
use je_instantiation::{Injectable, InstantiationService, ServiceCollection};
use je_platform::accessibility::{IAccessibilityService, I_ACCESSIBILITY_SERVICE};
use je_platform::configuration::service::{IConfigurationService, I_CONFIGURATION_SERVICE};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::files::files::FileStatWithMetadata;
use je_platform::files::i_file_service::{IFileService, I_FILE_SERVICE};
use je_platform::label::label::{ILabelService, I_LABEL_SERVICE};
use je_platform::resources::uri_file;
use je_workbench::common::editor::editor_input::{EditorInput, UntypedEditorOptions};
use je_workbench::common::editor::{
    EditorInputCapabilities, EditorInputOrUntyped, EditorInputRef, IEditorPane, RevertOptions, UntypedEditorInput,
};
use je_workbench::services::editor::custom_editor_label_service::{CustomEditorLabelService, ICustomEditorLabelService, I_CUSTOM_EDITOR_LABEL_SERVICE};
use je_workbench::services::editor::editor_service::{ActiveTextEditorControl, IEditorService, I_EDITOR_SERVICE};
use je_workbench::services::environment::environment_service::{IWorkbenchEnvironmentService, I_WORKBENCH_ENVIRONMENT_SERVICE};
use je_workbench::services::files_configuration::files_configuration_service::{IFilesConfigurationService, I_FILES_CONFIGURATION_SERVICE};
use je_workbench::services::language_detection::language_detection_worker_service::{
    ILanguageDetectionService, I_LANGUAGE_DETECTION_SERVICE, LANGUAGE_DETECTION_LANGUAGE_EVENT_SOURCE,
};
use je_workbench::services::path::path_service::{IPathService, I_PATH_SERVICE};
use je_workbench::services::textfile::collaborators::FileOperationUndoRedoInfo;
use je_workbench::services::textfile::encoding::{DecodeStreamOptions, UTF8, UTF8_WITH_BOM, encoding_exists, to_decode_stream};
use je_workbench::services::textfile::text_editor_service::{ITextEditorService, I_TEXT_EDITOR_SERVICE};
use je_workbench::services::textfile::textfiles::{
    EncodedContent, ILanguageSupport, IResourceEncodings, ITextFileEditorModelManager, ITextFileService, ReadTextFileEncodingOptions, ReadTextFileOptions,
    ResolvedDecoding, ResolvedEncoding, TextFileContent, TextFileCreateOperation, TextFileSaveAsOptions, TextFileSaveOptions, TextFileStreamContent,
    TextOrSnapshot, WriteTextFileOptions, I_TEXT_FILE_SERVICE, snapshot_to_string, to_buffer_or_readable,
};
use je_workbench::services::textmodel_resolver::resolver_service::{ITextEditorModel, ITextModelService, TextModelReference, I_TEXT_MODEL_SERVICE};
use je_workbench::services::untitled::untitled_text_editor_handler::UntitledTextEditorWorkingCopyEditorHandler;
use je_workbench::services::untitled::untitled_text_editor_input::UntitledTextEditorInput;
use je_workbench::services::untitled::untitled_text_editor_model::IUntitledTextEditorModel;
use je_workbench::services::untitled::untitled_text_editor_service::{
    AssociatedResource, IUntitledTextEditorModelManager, IUntitledTextEditorService, UntitledTextEditorOptions, UntitledTextEditorService,
    I_UNTITLED_TEXT_EDITOR_SERVICE,
};
use je_workbench::services::working_copy::working_copy::{IWorkingCopy, WorkingCopyBackupContent, WorkingCopyIdentifier, NO_TYPE_ID};
use je_workbench::services::working_copy::working_copy_backup::{IWorkingCopyBackupService, I_WORKING_COPY_BACKUP_SERVICE};
use je_workbench::services::working_copy::working_copy_backup_service::InMemoryWorkingCopyBackupService;
use je_workbench::services::working_copy::working_copy_editor_service::{IWorkingCopyEditorHandler, IWorkingCopyEditorService, I_WORKING_COPY_EDITOR_SERVICE};
use je_workbench::services::working_copy::working_copy_service::{IWorkingCopyService, WorkingCopyService, I_WORKING_COPY_SERVICE};
use je_workbench::test::browser::workbench_test_services::{TestEditorInput, TestEnvironmentService, TestPathService};
use je_workbench::test::common::workbench_test_services::TestTextResourcePropertiesService;

/// `IUntitledTextEditorModel`.
type ModelRef = Rc<dyn IUntitledTextEditorModel>;

/// O encoding que o `EncodingOracle` devolve para um recurso sem override quando `files.encoding` não está configurado:
/// `getUnvalidatedEncodingForResource` (o preferido, senão o configurado, senão `utf8`) e a validação (`utf8` sempre vale, os
/// demais precisam existir).
fn validated_encoding(preferred_encoding: Option<&str>) -> String {
    let file_encoding = preferred_encoding.filter(|encoding| !encoding.is_empty()).unwrap_or(UTF8);
    if file_encoding != UTF8 && !encoding_exists(file_encoding) {
        return UTF8.to_string();
    }

    file_encoding.to_string()
}

/// `EncodingOracle.getPreferredReadEncoding(resource, options, detectedEncoding)` para um recurso sem override.
fn preferred_read_encoding(options_encoding: Option<&str>, detected_encoding: Option<&str>) -> String {
    let preferred_encoding = match (options_encoding.filter(|encoding| !encoding.is_empty()), detected_encoding) {
        // Encoding passed in as option
        (Some(options_encoding), detected) => {
            if detected == Some(UTF8_WITH_BOM) && options_encoding == UTF8 {
                Some(UTF8_WITH_BOM) // indicate the file has BOM if we are to resolve with UTF 8
            } else {
                Some(options_encoding) // give passed in encoding highest priority
            }
        }
        // Encoding detected
        (None, Some(detected)) => Some(detected),
        // Encoding configured: `files.encoding` não está configurado
        (None, None) => None,
    };

    validated_encoding(preferred_encoding)
}

/// `TestTextFileService`: o `ITextFileService` com o que o modelo e a entrada sem título leem dele. Os casos não chamam as
/// demais operações (uma chamada reprova o caso com o nome da operação).
struct TestTextFileService {
    untitled: Rc<UntitledTextEditorService>,
}

impl ITextFileService for TestTextFileService {
    fn files(&self) -> Rc<dyn ITextFileEditorModelManager> {
        unreachable!("TestTextFileService.files não é exercitado por esta suíte")
    }

    fn untitled(&self) -> Rc<dyn IUntitledTextEditorModelManager> {
        self.untitled.clone()
    }

    fn encoding(&self) -> Rc<dyn IResourceEncodings> {
        unreachable!("TestTextFileService.encoding não é exercitado por esta suíte")
    }

    fn is_dirty(&self, _resource: &URI) -> bool {
        unreachable!("TestTextFileService.isDirty não é exercitado por esta suíte")
    }

    fn save(&self, _resource: &URI, _options: Option<TextFileSaveOptions>) -> Promise<Option<URI>> {
        unreachable!("TestTextFileService.save não é exercitado por esta suíte")
    }

    fn save_as(&self, _resource: &URI, _target_resource: Option<&URI>, _options: Option<TextFileSaveAsOptions>) -> Promise<Option<URI>> {
        unreachable!("TestTextFileService.saveAs não é exercitado por esta suíte")
    }

    /// `revert(resource, options)`: o ramo do esquema `untitled` (o ramo de arquivo lê o `files`, que os casos não usam).
    fn revert(&self, resource: &URI, options: Option<RevertOptions>) -> Promise<()> {
        assert!(*resource.scheme() == schemas::UNTITLED, "TestTextFileService.revert de arquivo não é exercitado por esta suíte");

        let model = IUntitledTextEditorService::get(self.untitled.as_ref(), resource);
        je_base::async_::async_fn(async move {
            match model {
                Some(model) => model.revert(options).await,
                None => Ok(()),
            }
        })
    }

    fn read(&self, _resource: &URI, _options: Option<ReadTextFileOptions>) -> Promise<TextFileContent> {
        unreachable!("TestTextFileService.read não é exercitado por esta suíte")
    }

    fn read_stream(&self, _resource: &URI, _options: Option<ReadTextFileOptions>) -> Promise<TextFileStreamContent> {
        unreachable!("TestTextFileService.readStream não é exercitado por esta suíte")
    }

    fn write(&self, _resource: &URI, _value: TextOrSnapshot, _options: Option<WriteTextFileOptions>) -> Promise<FileStatWithMetadata> {
        unreachable!("TestTextFileService.write não é exercitado por esta suíte")
    }

    fn create(&self, _operations: Vec<TextFileCreateOperation>, _undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>> {
        unreachable!("TestTextFileService.create não é exercitado por esta suíte")
    }

    /// `getEncodedReadable(resource, value, options)`: com o encoding `utf8` sem BOM o original pula a codificação e devolve
    /// `toBufferOrReadable(value)`; os demais encodings não são exercitados.
    fn get_encoded_readable(&self, _resource: Option<&URI>, value: Option<TextOrSnapshot>, options: Option<WriteTextFileOptions>) -> Promise<Option<EncodedContent>> {
        let encoding = validated_encoding(options.and_then(|options| options.encoding).as_deref());
        assert!(encoding == UTF8, "TestTextFileService.getEncodedReadable com o encoding {encoding} não é exercitado por esta suíte");

        Promise::resolved(to_buffer_or_readable(value))
    }

    /// `getDecodedStream(resource, value, options)`: `toDecodeStream` com `guessEncoding` desligado (`files.autoGuessEncoding`
    /// não está configurado) e o encoding validado pelo `getPreferredReadEncoding`.
    fn get_decoded_stream(&self, _resource: Option<&URI>, value: VSBufferReadableStream, options: Option<ReadTextFileEncodingOptions>) -> Promise<Rc<dyn ReadableStream<String>>> {
        let options = options.unwrap_or_default();
        let options_encoding = options.encoding.clone();

        to_decode_stream(
            value,
            DecodeStreamOptions {
                accept_text_only: options.accept_text_only.unwrap_or(false),
                guess_encoding: options.auto_guess_encoding.unwrap_or(false),
                candidate_guess_encodings: options.candidate_guess_encodings.unwrap_or_default(),
                min_bytes_required_for_detection: None,
                overwrite_encoding: Rc::new(move |detected_encoding| Promise::resolved(preferred_read_encoding(options_encoding.as_deref(), detected_encoding.as_deref()))),
            },
        )
        .then(|decoded| decoded.stream)
    }

    fn get_encoding(&self, _resource: &URI) -> String {
        unreachable!("TestTextFileService.getEncoding não é exercitado por esta suíte")
    }

    fn resolve_decoding(&self, _resource: Option<&URI>, _options: Option<ReadTextFileEncodingOptions>) -> Promise<ResolvedDecoding> {
        unreachable!("TestTextFileService.resolveDecoding não é exercitado por esta suíte")
    }

    fn resolve_encoding(&self, _resource: Option<&URI>, _options: Option<WriteTextFileOptions>) -> Promise<ResolvedEncoding> {
        unreachable!("TestTextFileService.resolveEncoding não é exercitado por esta suíte")
    }

    fn validate_detected_encoding(&self, _resource: Option<&URI>, _detected_encoding: &str, _options: Option<ReadTextFileEncodingOptions>) -> Promise<String> {
        unreachable!("TestTextFileService.validateDetectedEncoding não é exercitado por esta suíte")
    }
}

/// O `TextModelResolverService` para o esquema `untitled`: resolve o modelo pelo serviço de sem título e o descarta quando a
/// última referência é liberada, depois que o `canDispose` do serviço deixa (`destroyReferencedObject`).
struct TestTextModelService {
    untitled: Rc<UntitledTextEditorService>,
    references: Rc<RefCell<HashMap<String, usize>>>,
}

impl ITextModelService for TestTextModelService {
    fn create_model_reference(&self, resource: &URI) -> Promise<TextModelReference> {
        let untitled = self.untitled.clone();
        let references = self.references.clone();
        let key = uri_string(resource);
        *references.borrow_mut().entry(key.clone()).or_insert(0) += 1;

        let resource = resource.clone();
        je_base::async_::async_fn(async move {
            let model = IUntitledTextEditorService::resolve(
                untitled.as_ref(),
                Some(UntitledTextEditorOptions { untitled_resource: Some(resource), ..UntitledTextEditorOptions::default() }),
            )
            .await?;

            let model_to_release = model.clone();
            let release = to_disposable(move || {
                let remaining = {
                    let mut references = references.borrow_mut();
                    let count = references.get_mut(&key).expect("a referência foi adquirida");
                    *count -= 1;
                    *count
                };
                if remaining > 0 {
                    return;
                }
                references.borrow_mut().remove(&key);

                // `destroyReferencedObject` é `async`: espera o `modelPromise` antes de consultar o `canDispose`, então a
                // decisão acontece numa microtarefa (o `await modelPromise` do original).
                let untitled = untitled.clone();
                runtime::queue_microtask(move || {
                    runtime::spawn_local(async move {
                        // untitled file models have conditions that prevent them from dispose, so we have to wait until we can dispose
                        let can_dispose = match IUntitledTextEditorService::can_dispose(untitled.as_ref(), &model_to_release) {
                            None => true,
                            Some(can_dispose) => can_dispose.await.is_ok(),
                        };
                        if can_dispose {
                            model_to_release.dispose();
                        }
                    });
                });
            });

            Ok(TextModelReference::new(model, release))
        })
    }
}

/// `TestEditorService` com `activeTextEditorLanguageId` atribuível.
#[derive(Default)]
struct TestEditorServiceWithActiveLanguage {
    active_text_editor_language_id: RefCell<Option<String>>,
}

impl TestEditorServiceWithActiveLanguage {
    /// `accessor.editorService.activeTextEditorLanguageId = languageId`.
    fn set_active_text_editor_language_id(&self, language_id: Option<&str>) {
        *self.active_text_editor_language_id.borrow_mut() = language_id.map(str::to_string);
    }
}

impl IEditorService for TestEditorServiceWithActiveLanguage {
    fn visible_editor_panes(&self) -> Vec<Rc<dyn IEditorPane>> {
        Vec::new()
    }

    fn active_editor(&self) -> Option<EditorInputOrUntyped> {
        None
    }

    fn active_text_editor_control(&self) -> Option<ActiveTextEditorControl> {
        None
    }

    fn active_text_editor_language_id(&self) -> Option<String> {
        self.active_text_editor_language_id.borrow().clone()
    }

    fn open_editor(&self, _editor: UntypedEditorInput) -> Promise<Option<Rc<dyn IEditorPane>>> {
        Promise::resolved(None)
    }

    /// Não é exercitado por esta suíte.
    fn find_editors(&self, _resource: &je_base::uri::URI, _options: Option<je_workbench::services::editor::editor_service::FindEditorOptions>) -> Vec<je_workbench::common::editor::EditorIdentifier> {
        unreachable!("TestEditorServiceWithActiveLanguage.findEditors não é exercitado por esta suíte")
    }

    /// Não é exercitado por esta suíte.
    fn save(
        &self,
        _editors: Vec<je_workbench::common::editor::EditorIdentifier>,
        _options: Option<je_workbench::services::editor::editor_service::SaveEditorsOptions>,
    ) -> Promise<je_workbench::services::editor::editor_service::SaveEditorsResult> {
        unreachable!("TestEditorServiceWithActiveLanguage.save não é exercitado por esta suíte")
    }

    /// Não é exercitado por esta suíte.
    fn save_all(&self, _options: Option<je_workbench::services::editor::editor_service::SaveAllEditorsOptions>) -> Promise<je_workbench::services::editor::editor_service::SaveEditorsResult> {
        unreachable!("TestEditorServiceWithActiveLanguage.saveAll não é exercitado por esta suíte")
    }
}

/// `ITextEditorService`: os casos não criam editores por ele.
struct TestTextEditorService;

impl ITextEditorService for TestTextEditorService {
    fn create_text_editor(&self, _input: UntypedEditorInput) -> EditorInputRef {
        unreachable!("TestTextEditorService.createTextEditor não é exercitado por esta suíte")
    }

    fn resolve_text_editor(&self, _input: UntypedEditorInput) -> Promise<EditorInputRef> {
        unreachable!("TestTextEditorService.resolveTextEditor não é exercitado por esta suíte")
    }
}

/// `IWorkingCopyEditorService`: guarda os handlers registrados (`registerHandler`, `onDidRegisterHandler`).
struct TestWorkingCopyEditorService {
    on_did_register_handler: Rc<Emitter<Rc<dyn IWorkingCopyEditorHandler>>>,
    handlers: Rc<RefCell<Vec<Rc<dyn IWorkingCopyEditorHandler>>>>,
}

impl TestWorkingCopyEditorService {
    fn new() -> Rc<Self> {
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

    fn find_editor(&self, _working_copy: &dyn IWorkingCopy) -> Option<je_workbench::common::editor::EditorIdentifier> {
        unreachable!("TestWorkingCopyEditorService.findEditor não é exercitado por esta suíte")
    }
}

/// O que o `setup` do original monta: o `instantiationService` e o `accessor`.
struct Fixture {
    instantiation_service: Rc<InstantiationService>,
    untitled_text_editor_service: Rc<UntitledTextEditorService>,
    working_copy_service: Rc<WorkingCopyService>,
    test_configuration_service: Rc<TestConfigurationService>,
    editor_service: Rc<TestEditorServiceWithActiveLanguage>,
    language_service: Rc<LanguageService>,
}

fn setup(disposables: &LeakGuard) -> Fixture {
    let test_configuration_service = TestConfigurationService::new(None);
    disposables.add_rc(test_configuration_service.on_did_change_configuration_emitter.clone());
    let services = WorkbenchServices::new(disposables, &test_configuration_service);

    let custom_editor_label_service =
        disposables.add_rc(CustomEditorLabelService::create((), (test_configuration_service.clone(), services.context_service.clone())));

    let configuration_service: Rc<dyn IConfigurationService> = test_configuration_service.clone();
    let language_service = disposables.add_rc(LanguageService::new(false));
    let model_service = disposables.add_rc(ModelService::new(
        configuration_service.clone(),
        TestTextResourcePropertiesService::new(configuration_service.clone()),
        UndoRedoService::for_tests(),
    ));

    let collection = ServiceCollection::new();
    let instantiation_service = disposables.add_rc(InstantiationService::new(collection.clone()));
    let untitled_text_editor_service = disposables.add_rc(UntitledTextEditorService::new(instantiation_service.clone(), configuration_service.clone()));
    let working_copy_service = disposables.add_rc(WorkingCopyService::new());
    let working_copy_backup_service = disposables.add_rc(InMemoryWorkingCopyBackupService::new());
    let working_copy_editor_service = disposables.add_rc(TestWorkingCopyEditorService::new());
    let editor_service = Rc::new(TestEditorServiceWithActiveLanguage::default());
    let environment_service: Rc<dyn IWorkbenchEnvironmentService> = TestEnvironmentService::new();
    let path_service: Rc<dyn IPathService> = TestPathService::new(None, None);

    collection.set(I_CONFIGURATION_SERVICE, configuration_service);
    collection.set(I_LANGUAGE_SERVICE, language_service.clone() as Rc<dyn ILanguageService>);
    collection.set(I_MODEL_SERVICE, model_service as Rc<dyn IModelService>);
    collection.set(I_WORKING_COPY_SERVICE, working_copy_service.clone() as Rc<dyn IWorkingCopyService>);
    collection.set(I_WORKING_COPY_BACKUP_SERVICE, working_copy_backup_service as Rc<dyn IWorkingCopyBackupService>);
    collection.set(I_WORKING_COPY_EDITOR_SERVICE, working_copy_editor_service as Rc<dyn IWorkingCopyEditorService>);
    collection.set(I_TEXT_RESOURCE_CONFIGURATION_SERVICE, services.text_resource_configuration_service.clone() as Rc<dyn ITextResourceConfigurationService>);
    collection.set(I_TEXT_FILE_SERVICE, Rc::new(TestTextFileService { untitled: untitled_text_editor_service.clone() }) as Rc<dyn ITextFileService>);
    collection.set(
        I_TEXT_MODEL_SERVICE,
        Rc::new(TestTextModelService { untitled: untitled_text_editor_service.clone(), references: Rc::default() }) as Rc<dyn ITextModelService>,
    );
    collection.set(I_TEXT_EDITOR_SERVICE, Rc::new(TestTextEditorService) as Rc<dyn ITextEditorService>);
    collection.set(I_UNTITLED_TEXT_EDITOR_SERVICE, untitled_text_editor_service.clone() as Rc<dyn IUntitledTextEditorService>);
    collection.set(I_LABEL_SERVICE, services.label_service.clone() as Rc<dyn ILabelService>);
    collection.set(I_EDITOR_SERVICE, editor_service.clone() as Rc<dyn IEditorService>);
    collection.set(I_FILE_SERVICE, services.file_service.clone() as Rc<dyn IFileService>);
    collection.set(I_FILES_CONFIGURATION_SERVICE, services.files_configuration_service.clone() as Rc<dyn IFilesConfigurationService>);
    collection.set(I_CUSTOM_EDITOR_LABEL_SERVICE, custom_editor_label_service as Rc<dyn ICustomEditorLabelService>);
    collection.set(I_WORKBENCH_ENVIRONMENT_SERVICE, environment_service);
    collection.set(I_PATH_SERVICE, path_service);
    collection.set(I_LANGUAGE_DETECTION_SERVICE, Rc::new(TestLanguageDetectionService) as Rc<dyn ILanguageDetectionService>);
    collection.set(I_ACCESSIBILITY_SERVICE, Rc::new(TestAccessibilityService) as Rc<dyn IAccessibilityService>);

    Fixture { instantiation_service, untitled_text_editor_service, working_copy_service, test_configuration_service, editor_service, language_service }
}

impl Fixture {
    /// `accessor.untitledTextEditorService`.
    fn service(&self) -> Rc<dyn IUntitledTextEditorService> {
        self.untitled_text_editor_service.clone()
    }

    /// `instantiationService.createInstance(UntitledTextEditorInput, model)`.
    fn create_input(&self, model: ModelRef) -> Rc<UntitledTextEditorInput> {
        self.instantiation_service.create_instance::<UntitledTextEditorInput>((model,))
    }
}

/// `URI.from({ scheme: Schemas.untitled, path })`.
fn untitled_resource_of(path: &str) -> URI {
    URI::from(&UriComponents { scheme: Some(JsString::from(schemas::UNTITLED)), path: Some(JsString::from(path)), ..UriComponents::default() }, false)
}

/// `URI.file(join('C:\\', '/foo/<name>'))`.
fn associated_file(name: &str) -> URI {
    uri_file(&PathModule::native(&ProcessEnvironment).join(&["C:\\", &format!("/foo/{name}")]))
}

/// `resource.toString()`.
fn uri_string(resource: &URI) -> String {
    resource.to_string(false).to_string_lossy()
}

/// `input.resource`.
fn input_resource(input: &UntitledTextEditorInput) -> URI {
    EditorInput::resource(input).expect("a entrada sem título tem recurso")
}

/// `input.getName()`.
fn input_name(input: &UntitledTextEditorInput) -> String {
    EditorInput::get_name(input)
}

/// `model.textEditorModel`.
fn text_model_of(model: &ModelRef) -> Rc<TextModel> {
    model.text_editor_model().expect("o modelo de texto existe")
}

/// `model.textEditorModel?.setValue(value)`.
fn set_value(model: &ModelRef, value: &str) {
    text_model_of(model).set_value(TextSource::from(value));
}

/// `disposables.add(model)`.
fn track_model(disposables: &LeakGuard, model: ModelRef) -> ModelRef {
    disposables.add(model.clone());
    model
}

/// `disposables.add(await input.resolve())`.
async fn resolve_and_track(disposables: &LeakGuard, input: &UntitledTextEditorInput) -> ModelRef {
    let model = input.resolve_untitled().await.unwrap();
    track_model(disposables, model)
}

/// `assert.strictEqual(actual, expected)` para dois modelos: a mesma instância.
fn assert_same_model(actual: Option<ModelRef>, expected: &ModelRef) {
    let actual = actual.expect("o modelo existe");
    assert!(std::ptr::addr_eq(Rc::as_ptr(&actual), Rc::as_ptr(expected)), "os modelos são instâncias diferentes");
}

/// `service.create({ ... })` com as opções dadas.
fn create_with(service: &Rc<dyn IUntitledTextEditorService>, options: UntitledTextEditorOptions) -> ModelRef {
    service.create(Some(options))
}

/// `{ preserveViewState: 0 }` (e `preserveResource`) das opções de `toUntyped`.
fn untyped_options(preserve_resource: Option<bool>) -> UntypedEditorOptions {
    UntypedEditorOptions { preserve_view_state: Some(0), preserve_resource }
}

/// `config.setUserConfiguration('files', { 'defaultLanguage': defaultLanguage })`.
fn set_default_language(configuration: &TestConfigurationService, default_language: Option<&str>) {
    let value = default_language.map_or(Dynamic::Undefined, Dynamic::from);
    let _ = configuration.set_user_configuration("files", Dynamic::object([("defaultLanguage", value)]), None);
}

/// `awaitDidChangeDirty(service)`.
fn await_did_change_dirty(service: &Rc<dyn IUntitledTextEditorService>) -> Promise<URI> {
    service.on_did_change_dirty().to_promise(None).promise.then(|model| model.resource())
}

#[test]
fn basics() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let service = fixture.service();
        let working_copy_service = fixture.working_copy_service.clone();

        let events: Rc<RefCell<Vec<ModelRef>>> = Rc::default();
        {
            let events = events.clone();
            disposables.add(service.on_did_create().listen(move |model| {
                events.borrow_mut().push(model.clone());
            }));
        }

        let input1 = fixture.create_input(service.create(None));
        input1.resolve_untitled().await.unwrap();
        assert_same_model(service.get(&input_resource(&input1)), &input1.model());
        assert!(!service.is_untitled_with_associated_resource(&input_resource(&input1)));

        assert_eq!(events.borrow().len(), 1);
        assert_eq!(uri_string(&events.borrow()[0].resource()), uri_string(&input1.model().resource()));

        assert!(service.get(&input_resource(&input1)).is_some());
        assert!(service.get(&uri_file("testing")).is_none());

        assert!(input1.has_capability(EditorInputCapabilities::UNTITLED));
        assert!(!input1.has_capability(EditorInputCapabilities::READONLY));
        assert!(!input1.is_readonly().is_readonly());
        assert!(!input1.has_capability(EditorInputCapabilities::SINGLETON));
        assert!(!input1.has_capability(EditorInputCapabilities::REQUIRES_TRUST));
        assert!(!input1.has_capability(EditorInputCapabilities::SCRATCHPAD));

        let input2 = fixture.create_input(service.create(None));
        assert_same_model(service.get(&input_resource(&input2)), &input2.model());

        // toUntyped()
        let untyped_input = input1.to_untyped(Some(&untyped_options(None))).expect("a entrada sem título tem representação sem tipo");
        assert_eq!(untyped_input.force_untitled, Some(true));

        // get()
        assert_same_model(service.get(&input_resource(&input1)), &input1.model());
        assert_same_model(service.get(&input_resource(&input2)), &input2.model());

        // revert()
        input1.revert(0, None).await.unwrap();
        assert!(input1.is_disposed());
        assert!(service.get(&input_resource(&input1)).is_none());

        // dirty
        let model = input2.resolve_untitled().await.unwrap();
        assert_same_model(
            Some(service.resolve(Some(UntitledTextEditorOptions { untitled_resource: Some(input_resource(&input2)), ..UntitledTextEditorOptions::default() })).await.unwrap()),
            &model,
        );
        assert!(service.get(&model.resource()).is_some());

        assert_eq!(events.borrow().len(), 2);
        assert_eq!(uri_string(&events.borrow()[1].resource()), uri_string(&input_resource(&input2)));

        assert!(!input2.is_dirty());

        let resource_promise = await_did_change_dirty(&service);

        set_value(&model, "foo bar");

        let resource = resource_promise.await.unwrap();

        assert_eq!(uri_string(&resource), uri_string(&input_resource(&input2)));

        assert!(input2.is_dirty());

        let dirty_untyped_input = input2.to_untyped(Some(&untyped_options(None))).expect("a entrada sem título tem representação sem tipo");
        assert_eq!(dirty_untyped_input.contents.as_deref(), Some("foo bar"));
        assert!(dirty_untyped_input.resource.is_none());

        let dirty_untyped_input_with_resource = input2.to_untyped(Some(&untyped_options(Some(true)))).expect("a entrada sem título tem representação sem tipo");
        assert_eq!(dirty_untyped_input_with_resource.contents.as_deref(), Some("foo bar"));
        assert_eq!(dirty_untyped_input_with_resource.resource.as_ref().map(uri_string), Some(uri_string(&input_resource(&input2))));

        let dirty_untyped_input_without_content = input2.to_untyped(None).expect("a entrada sem título tem representação sem tipo");
        assert_eq!(dirty_untyped_input_without_content.resource.as_ref().map(uri_string), Some(uri_string(&input_resource(&input2))));
        assert_eq!(dirty_untyped_input_without_content.contents, None);

        assert!(working_copy_service.is_dirty(&input_resource(&input2), None));
        assert_eq!(working_copy_service.dirty_count(), 1);

        input1.revert(0, None).await.unwrap();
        input2.revert(0, None).await.unwrap();
        assert!(service.get(&input_resource(&input1)).is_none());
        assert!(service.get(&input_resource(&input2)).is_none());
        assert!(!input2.is_dirty());
        assert!(!model.is_dirty());

        assert!(!working_copy_service.is_dirty(&input_resource(&input2), None));
        assert_eq!(working_copy_service.dirty_count(), 0);

        input1.revert(0, None).await.unwrap();
        assert!(input1.is_disposed());
        assert!(service.get(&input_resource(&input1)).is_none());

        input2.dispose();
        assert!(service.get(&input_resource(&input2)).is_none());
    });
}

#[test]
fn associated_resource_is_dirty() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let service = fixture.service();
        let file = associated_file("file.txt");

        let on_did_change_dirty_model: Rc<RefCell<Option<ModelRef>>> = Rc::default();
        {
            let on_did_change_dirty_model = on_did_change_dirty_model.clone();
            disposables.add(service.on_did_change_dirty().listen(move |model| {
                *on_did_change_dirty_model.borrow_mut() = Some(model.clone());
            }));
        }

        let model = track_model(&disposables, create_with(&service, UntitledTextEditorOptions { associated_resource: Some(AssociatedResource::from(&file)), ..UntitledTextEditorOptions::default() }));
        assert!(service.is_untitled_with_associated_resource(&model.resource()));
        let untitled = disposables.add_rc(fixture.create_input(model.clone()));
        assert!(untitled.is_dirty());
        assert_same_model(on_did_change_dirty_model.borrow().clone(), &model);

        let resolved_model = untitled.resolve_untitled().await.unwrap();

        assert!(resolved_model.has_associated_file_path());
        assert!(untitled.is_dirty());
    });
}

#[test]
fn no_longer_dirty_when_content_gets_empty_not_with_associated_resource() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let service = fixture.service();
        let working_copy_service = fixture.working_copy_service.clone();
        let input = disposables.add_rc(fixture.create_input(service.create(None)));

        // dirty
        let model = resolve_and_track(&disposables, &input).await;
        set_value(&model, "foo bar");
        assert!(model.is_dirty());
        assert!(working_copy_service.is_dirty(&model.resource(), Some(model.working_copy_type_id().as_str())));
        set_value(&model, "");
        assert!(!model.is_dirty());
        assert!(!working_copy_service.is_dirty(&model.resource(), Some(model.working_copy_type_id().as_str())));
    });
}

#[test]
fn via_create_options() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let service = fixture.service();

        let input1 = disposables.add_rc(fixture.create_input(service.create(None)));
        let model1 = resolve_and_track(&disposables, &input1).await;

        set_value(&model1, "foo bar");
        assert!(model1.is_dirty());

        set_value(&model1, "");
        assert!(!model1.is_dirty());

        let input2 = disposables.add_rc(fixture.create_input(create_with(&service, UntitledTextEditorOptions { initial_value: Some("Hello World".to_string()), ..UntitledTextEditorOptions::default() })));
        let model2 = resolve_and_track(&disposables, &input2).await;
        let mut snapshot = model2.create_snapshot().expect("o snapshot");
        assert_eq!(snapshot_to_string(snapshot.as_mut()), "Hello World");

        let input = disposables.add_rc(fixture.create_input(track_model(&disposables, service.create(None))));

        let input3 = disposables.add_rc(fixture.create_input(create_with(&service, UntitledTextEditorOptions { untitled_resource: Some(input_resource(&input)), ..UntitledTextEditorOptions::default() })));
        let model3 = resolve_and_track(&disposables, &input3).await;

        assert_eq!(uri_string(&model3.resource()), uri_string(&input_resource(&input)));

        let file = associated_file("file44.txt");
        let input4 = disposables.add_rc(fixture.create_input(create_with(&service, UntitledTextEditorOptions { associated_resource: Some(AssociatedResource::from(&file)), ..UntitledTextEditorOptions::default() })));
        let model4 = resolve_and_track(&disposables, &input4).await;
        assert!(model4.has_associated_file_path());
        assert!(model4.is_dirty());
    });
}

#[test]
fn associated_path_remains_dirty_when_content_gets_empty() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let service = fixture.service();
        let file = associated_file("file.txt");
        let input = disposables.add_rc(fixture.create_input(create_with(&service, UntitledTextEditorOptions { associated_resource: Some(AssociatedResource::from(&file)), ..UntitledTextEditorOptions::default() })));

        // dirty
        let model = resolve_and_track(&disposables, &input).await;
        set_value(&model, "foo bar");
        assert!(model.is_dirty());
        set_value(&model, "");
        assert!(model.is_dirty());
    });
}

#[test]
fn initial_content_is_dirty() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let service = fixture.service();
        let working_copy_service = fixture.working_copy_service.clone();

        let untitled = disposables.add_rc(fixture.create_input(create_with(&service, UntitledTextEditorOptions { initial_value: Some("Hello World".to_string()), ..UntitledTextEditorOptions::default() })));
        assert!(untitled.is_dirty());

        let backup = untitled.model().backup(&CancellationToken::None).await.unwrap().content;
        match backup {
            Some(WorkingCopyBackupContent::Stream(stream)) => {
                let value = stream_to_buffer(stream.as_ref()).await.unwrap();
                assert_eq!(value.to_string(), "Hello World");
            }
            Some(WorkingCopyBackupContent::Readable(readable)) => {
                let mut guard = readable.borrow_mut();
                let value = readable_to_buffer(&mut **guard);
                assert_eq!(value.to_string(), "Hello World");
            }
            None => panic!("Missing untitled backup"),
        }

        // dirty
        let model = resolve_and_track(&disposables, &untitled).await;
        assert!(model.is_dirty());
        assert_eq!(working_copy_service.dirty_count(), 1);
    });
}

#[test]
fn created_with_files_default_language_setting() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = setup(&disposables);

    let default_language = "javascript";
    let config = fixture.test_configuration_service.clone();
    set_default_language(&config, Some(default_language));

    let service = fixture.service();
    let input = track_model(&disposables, service.create(None));

    assert_eq!(input.get_language_id().as_deref(), Some(default_language));

    set_default_language(&config, None);
}

#[test]
fn created_with_files_default_language_setting_active_editor_language() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = setup(&disposables);

    let config = fixture.test_configuration_service.clone();
    set_default_language(&config, Some("${activeEditorLanguage}"));

    fixture.editor_service.set_active_text_editor_language_id(Some("typescript"));

    let service = fixture.service();
    let model = track_model(&disposables, service.create(None));

    assert_eq!(model.get_language_id().as_deref(), Some("typescript"));

    set_default_language(&config, None);
    fixture.editor_service.set_active_text_editor_language_id(None);
}

#[test]
fn created_with_language_overrides_files_default_language_setting() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = setup(&disposables);

    let language = "typescript";
    let default_language = "javascript";
    let config = fixture.test_configuration_service.clone();
    set_default_language(&config, Some(default_language));

    let service = fixture.service();
    let input = track_model(&disposables, create_with(&service, UntitledTextEditorOptions { language_id: Some(language.to_string()), ..UntitledTextEditorOptions::default() }));

    assert_eq!(input.get_language_id().as_deref(), Some(language));

    set_default_language(&config, None);
}

#[test]
fn can_change_language_afterwards() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let language_id = "untitled-input-test";

        disposables.add(fixture.language_service.register_language(LanguageExtensionPoint::new(language_id)));

        let service = fixture.service();
        let input = disposables.add_rc(fixture.create_input(create_with(&service, UntitledTextEditorOptions { language_id: Some(language_id.to_string()), ..UntitledTextEditorOptions::default() })));

        assert_eq!(input.get_language_id().as_deref(), Some(language_id));

        let model = resolve_and_track(&disposables, &input).await;
        assert_eq!(model.get_language_id().as_deref(), Some(language_id));

        input.set_language_id(PLAINTEXT_LANGUAGE_ID, None);

        assert_eq!(input.get_language_id().as_deref(), Some(PLAINTEXT_LANGUAGE_ID));
    });
}

#[test]
fn remembers_that_language_was_set_explicitly() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = setup(&disposables);

    let language = "untitled-input-test";

    disposables.add(fixture.language_service.register_language(LanguageExtensionPoint::new(language)));

    let service = fixture.service();
    let model = track_model(&disposables, service.create(None));
    let input = disposables.add_rc(fixture.create_input(model));

    assert!(!input.has_language_set_explicitly());
    input.set_language_id(PLAINTEXT_LANGUAGE_ID, None);
    assert!(input.has_language_set_explicitly());

    assert_eq!(input.get_language_id().as_deref(), Some(PLAINTEXT_LANGUAGE_ID));
}

// Issue #159202
#[test]
fn remembers_that_language_was_set_explicitly_if_set_by_another_source() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let language = "untitled-input-test";

        disposables.add(fixture.language_service.register_language(LanguageExtensionPoint::new(language)));

        let service = fixture.service();
        let model = track_model(&disposables, service.create(None));
        let input = disposables.add_rc(fixture.create_input(model.clone()));
        resolve_and_track(&disposables, &input).await;

        assert!(!input.has_language_set_explicitly());
        text_model_of(&model).set_language(language, None);
        assert!(input.has_language_set_explicitly());

        assert_eq!(model.get_language_id().as_deref(), Some(language));
    });
}

#[test]
fn language_is_not_set_explicitly_if_set_by_language_detection_source() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);

        let language = "untitled-input-test";

        disposables.add(fixture.language_service.register_language(LanguageExtensionPoint::new(language)));

        let service = fixture.service();
        let model = track_model(&disposables, service.create(None));
        let input = disposables.add_rc(fixture.create_input(model.clone()));
        input.resolve_untitled().await.unwrap();

        assert!(!input.has_language_set_explicitly());
        text_model_of(&model).set_language(
            language,
            // This is really what this is testing
            Some(LANGUAGE_DETECTION_LANGUAGE_EVENT_SOURCE),
        );
        assert!(!input.has_language_set_explicitly());

        assert_eq!(model.get_language_id().as_deref(), Some(language));
    });
}

#[test]
fn service_on_did_change_encoding() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let service = fixture.service();
        let input = disposables.add_rc(fixture.create_input(service.create(None)));

        let counter = Rc::new(Cell::new(0));
        let input_resource_string = uri_string(&input_resource(&input));

        {
            let counter = counter.clone();
            disposables.add(service.on_did_change_encoding().listen(move |model| {
                counter.set(counter.get() + 1);
                assert_eq!(uri_string(&model.resource()), input_resource_string);
            }));
        }

        // encoding
        let model = resolve_and_track(&disposables, &input).await;
        model.set_encoding("utf16").await.unwrap();
        assert_eq!(counter.get(), 1);
    });
}

#[test]
fn service_on_did_change_label() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let service = fixture.service();
        let input = disposables.add_rc(fixture.create_input(service.create(None)));

        let counter = Rc::new(Cell::new(0));
        let input_resource_string = uri_string(&input_resource(&input));

        {
            let counter = counter.clone();
            disposables.add(service.on_did_change_label().listen(move |model| {
                counter.set(counter.get() + 1);
                assert_eq!(uri_string(&model.resource()), input_resource_string);
            }));
        }

        // label
        let model = resolve_and_track(&disposables, &input).await;
        set_value(&model, "Foo Bar");
        assert_eq!(counter.get(), 1);
    });
}

#[test]
fn service_on_will_dispose() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let service = fixture.service();
        let input = disposables.add_rc(fixture.create_input(service.create(None)));

        let counter = Rc::new(Cell::new(0));
        let input_resource_string = uri_string(&input_resource(&input));

        {
            let counter = counter.clone();
            disposables.add(service.on_will_dispose().listen(move |model| {
                counter.set(counter.get() + 1);
                assert_eq!(uri_string(&model.resource()), input_resource_string);
            }));
        }

        let model = resolve_and_track(&disposables, &input).await;
        assert_eq!(counter.get(), 0);
        model.dispose();
        assert_eq!(counter.get(), 1);
    });
}

#[test]
fn service_get_value() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let service = fixture.service();
        let input1 = disposables.add_rc(fixture.create_input(service.create(None)));
        let model1 = resolve_and_track(&disposables, &input1).await;

        set_value(&model1, "foo bar");
        assert_eq!(service.get_value(&model1.resource()).as_deref(), Some("foo bar"));
        model1.dispose();

        // When a model doesn't exist, it should return undefined
        assert_eq!(service.get_value(&URI::parse("https://www.microsoft.com", false)), None);
    });
}

#[test]
fn model_on_did_change_content() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let service = fixture.service();
        let input = disposables.add_rc(fixture.create_input(service.create(None)));

        let counter = Rc::new(Cell::new(0));

        let model = resolve_and_track(&disposables, &input).await;
        {
            let counter = counter.clone();
            disposables.add(model.on_did_change_content().listen(move |_| counter.set(counter.get() + 1)));
        }

        set_value(&model, "foo");

        assert_eq!(counter.get(), 1, "Dirty model should trigger event");
        set_value(&model, "bar");

        assert_eq!(counter.get(), 2, "Content change when dirty should trigger event");
        set_value(&model, "");

        assert_eq!(counter.get(), 3, "Manual revert should trigger event");
        set_value(&model, "foo");

        assert_eq!(counter.get(), 4, "Dirty model should trigger event");
    });
}

#[test]
fn model_on_did_revert_and_input_disposed_when_reverted() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let service = fixture.service();
        let input = disposables.add_rc(fixture.create_input(service.create(None)));

        let counter = Rc::new(Cell::new(0));

        let model = resolve_and_track(&disposables, &input).await;
        {
            let counter = counter.clone();
            disposables.add(model.on_did_revert().listen(move |_| counter.set(counter.get() + 1)));
        }

        set_value(&model, "foo");

        model.revert(None).await.unwrap();

        assert!(input.is_disposed());
        assert!(counter.get() == 1);
    });
}

#[test]
fn model_on_did_change_name_and_input_name() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let service = fixture.service();
        let input = disposables.add_rc(fixture.create_input(service.create(None)));

        let counter = Rc::new(Cell::new(0));

        let model = resolve_and_track(&disposables, &input).await;
        {
            let counter = counter.clone();
            disposables.add(model.on_did_change_name().listen(move |_| counter.set(counter.get() + 1)));
        }

        set_value(&model, "foo");
        assert_eq!(input_name(&input), "foo");
        assert_eq!(model.name(), "foo");

        assert_eq!(counter.get(), 1);
        set_value(&model, "bar");
        assert_eq!(input_name(&input), "bar");
        assert_eq!(model.name(), "bar");

        assert_eq!(counter.get(), 2);
        set_value(&model, "");
        assert_eq!(input_name(&input), "Untitled-1");
        assert_eq!(model.name(), "Untitled-1");

        set_value(&model, "        ");
        assert_eq!(input_name(&input), "Untitled-1");
        assert_eq!(model.name(), "Untitled-1");

        set_value(&model, "([]}"); // require actual words
        assert_eq!(input_name(&input), "Untitled-1");
        assert_eq!(model.name(), "Untitled-1");

        set_value(&model, "([]}hello   "); // require actual words
        assert_eq!(input_name(&input), "([]}hello");
        assert_eq!(model.name(), "([]}hello");

        set_value(&model, "12345678901234567890123456789012345678901234567890"); // trimmed at 40chars max
        assert_eq!(input_name(&input), "1234567890123456789012345678901234567890");
        assert_eq!(model.name(), "1234567890123456789012345678901234567890");

        set_value(&model, "123456789012345678901234567890123456789🌞"); // do not break grapehems (#111235)
        assert_eq!(input_name(&input), "123456789012345678901234567890123456789");
        assert_eq!(model.name(), "123456789012345678901234567890123456789");

        set_value(&model, "hello\u{202E}world"); // do not allow RTL in names (#190133)
        assert_eq!(input_name(&input), "helloworld");
        assert_eq!(model.name(), "helloworld");

        assert_eq!(counter.get(), 7);

        set_value(&model, "Hello\nWorld");
        assert_eq!(counter.get(), 8);

        /// `createSingleEditOp(text, positionLineNumber, positionColumn, selectionLineNumber = positionLineNumber, selectionColumn = positionColumn)`.
        fn create_single_edit_op(text: &str, position_line_number: i32, position_column: i32, selection_line_number: Option<i32>, selection_column: Option<i32>) -> SingleEditOperation {
            let range = Range::new(
                selection_line_number.unwrap_or(position_line_number),
                selection_column.unwrap_or(position_column),
                position_line_number,
                position_column,
            );

            SingleEditOperation::new(range, Some(JsString::from(text)), false)
        }

        text_model_of(&model).apply_edits(&[create_single_edit_op("hello", 2, 2, None, None)]);
        assert_eq!(counter.get(), 8); // change was not on first line

        input.dispose();
        model.dispose();

        let input_with_contents =
            disposables.add_rc(fixture.create_input(create_with(&service, UntitledTextEditorOptions { initial_value: Some("Foo".to_string()), ..UntitledTextEditorOptions::default() })));
        resolve_and_track(&disposables, &input_with_contents).await;

        assert_eq!(input_name(&input_with_contents), "Foo");
    });
}

#[test]
fn model_on_did_change_dirty() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let service = fixture.service();
        let input = disposables.add_rc(fixture.create_input(service.create(None)));

        let counter = Rc::new(Cell::new(0));

        let model = resolve_and_track(&disposables, &input).await;
        {
            let counter = counter.clone();
            disposables.add(model.on_did_change_dirty().listen(move |_| counter.set(counter.get() + 1)));
        }

        set_value(&model, "foo");

        assert_eq!(counter.get(), 1, "Dirty model should trigger event");
        set_value(&model, "bar");

        assert_eq!(counter.get(), 1, "Another change does not fire event");
    });
}

#[test]
fn model_on_did_change_encoding() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let service = fixture.service();
        let input = disposables.add_rc(fixture.create_input(service.create(None)));

        let counter = Rc::new(Cell::new(0));

        let model = resolve_and_track(&disposables, &input).await;
        {
            let counter = counter.clone();
            disposables.add(model.on_did_change_encoding().listen(move |_| counter.set(counter.get() + 1)));
        }

        model.set_encoding("utf16").await.unwrap();

        assert_eq!(counter.get(), 1, "Dirty model should trigger event");
        model.set_encoding("utf16").await.unwrap();

        assert_eq!(counter.get(), 1, "Another change to same encoding does not fire event");
    });
}

#[test]
fn can_dispose_with_dirty_model() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&disposables);
        let service = fixture.service();
        let input = disposables.add_rc(fixture.create_input(service.create(None)));

        let model = resolve_and_track(&disposables, &input).await;

        set_value(&model, "foo");

        let can_dispose_promise = service.can_dispose(&model);
        assert!(can_dispose_promise.is_some()); // `canDisposePromise instanceof Promise`

        let can_dispose = Rc::new(Cell::new(false));
        {
            let can_dispose = can_dispose.clone();
            let can_dispose_promise = can_dispose_promise.expect("a promessa de canDispose");
            runtime::spawn_local(async move {
                if can_dispose_promise.await.is_ok() {
                    can_dispose.set(true);
                }
            });
        }

        assert!(!can_dispose.get());
        drop(model.revert(Some(RevertOptions { soft: Some(true), ..RevertOptions::default() })));

        timeout(0).await.unwrap();

        assert!(can_dispose.get());

        let can_dispose2 = service.can_dispose(&model);
        assert!(can_dispose2.is_none()); // `canDispose2 === true`
    });
}

// Suíte `UntitledTextEditorWorkingCopyEditorHandler`.

/// `handler.handles(workingCopy)`: o valor direto (o original compara com `true` e `false`, sem `await`).
fn handles(handler: &UntitledTextEditorWorkingCopyEditorHandler, working_copy: &WorkingCopyIdentifier) -> bool {
    match handler.handles(working_copy) {
        MaybePromise::Value(handled) => handled,
        MaybePromise::Promise(_) => panic!("handles devolveu uma promessa, o original devolve o booleano"),
    }
}

/// O `setup` da segunda suíte: o `instantiationService` e o `handler`.
fn setup_handler(disposables: &LeakGuard) -> (Fixture, Rc<UntitledTextEditorWorkingCopyEditorHandler>) {
    let fixture = setup(disposables);
    let handler = disposables.add_rc(fixture.instantiation_service.create_instance::<UntitledTextEditorWorkingCopyEditorHandler>(()));

    (fixture, handler)
}

#[test]
fn handles_only_untitled_working_copies_with_no_type_id() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (_fixture, handler) = setup_handler(&disposables);

    let untitled_resource = untitled_resource_of("Untitled-1");

    assert!(handles(&handler, &WorkingCopyIdentifier { resource: untitled_resource.clone(), type_id: NO_TYPE_ID.to_string() }));
    assert!(!handles(&handler, &WorkingCopyIdentifier { resource: untitled_resource, type_id: "someTypeId".to_string() }));
    assert!(!handles(&handler, &WorkingCopyIdentifier { resource: uri_file("/test.txt"), type_id: NO_TYPE_ID.to_string() }));
}

#[test]
fn is_open_matches_untitled_text_editor_input_with_same_resource() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (fixture, handler) = setup_handler(&disposables);

    let untitled_resource = untitled_resource_of("Untitled-1");
    let working_copy = WorkingCopyIdentifier { resource: untitled_resource.clone(), type_id: NO_TYPE_ID.to_string() };

    let untitled_input = disposables.add_rc(fixture.create_input(create_with(
        &fixture.service(),
        UntitledTextEditorOptions { untitled_resource: Some(untitled_resource), ..UntitledTextEditorOptions::default() },
    )));
    let editor: EditorInputRef = untitled_input;
    assert!(handler.is_open(&working_copy, &editor));
}

#[test]
fn is_open_matches_non_untitled_text_editor_input_editors_with_same_untitled_resource() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (_fixture, handler) = setup_handler(&disposables);

    let untitled_resource = untitled_resource_of("Untitled-1");
    let working_copy = WorkingCopyIdentifier { resource: untitled_resource.clone(), type_id: NO_TYPE_ID.to_string() };

    // A custom editor (or any other editor type) sharing the same untitled resource
    // should be recognized as representing this working copy to prevent duplicate
    // tabs on backup restoration.
    let custom_editor_input_concrete = disposables.add_rc(TestEditorInput::new(untitled_resource, "customEditorType"));
    let custom_editor_input: EditorInputRef = custom_editor_input_concrete;
    assert!(handler.is_open(&working_copy, &custom_editor_input));
}

#[test]
fn is_open_does_not_match_editors_with_different_resource() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (_fixture, handler) = setup_handler(&disposables);

    let untitled_resource1 = untitled_resource_of("Untitled-1");
    let untitled_resource2 = untitled_resource_of("Untitled-2");
    let working_copy = WorkingCopyIdentifier { resource: untitled_resource1, type_id: NO_TYPE_ID.to_string() };

    let other_input_concrete = disposables.add_rc(TestEditorInput::new(untitled_resource2, "customEditorType"));
    let other_input: EditorInputRef = other_input_concrete;
    assert!(!handler.is_open(&working_copy, &other_input));
}

#[test]
fn is_open_returns_false_for_non_untitled_working_copies() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let (_fixture, handler) = setup_handler(&disposables);

    let file_resource = uri_file("/test.txt");
    let working_copy = WorkingCopyIdentifier { resource: file_resource.clone(), type_id: NO_TYPE_ID.to_string() };

    let editor_concrete = disposables.add_rc(TestEditorInput::new(file_resource, "testType"));
    let editor: EditorInputRef = editor_concrete;
    assert!(!handler.is_open(&working_copy, &editor));
}
