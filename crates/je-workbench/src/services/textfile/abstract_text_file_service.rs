//! Porte de `src/vs/workbench/services/textfile/browser/textFileService.ts`: o `AbstractTextFileService`, a base dos
//! serviços de arquivos de texto (no navegador e no nativo), com a leitura, a escrita e a criação de arquivos de texto
//! na codificação certa, o "Salvar" e o "Salvar como" (de arquivos e de editores sem título), a reversão, o estado de
//! sujo e as decorações de somente leitura e de arquivo apagado; e o `EncodingOracle`, que decide a codificação de
//! leitura e de escrita de cada recurso.
//!
//! Divergências:
//! - **Classe abstrata.** `AbstractTextFileService` é a struct concreta que as subclasses compõem (como o
//!   `AbstractPathService`): o construtor [`AbstractTextFileService::new`] devolve o `Rc` e as subclasses acrescentam o
//!   comportamento por cima dele (ver o [`BrowserTextFileService`](super::browser_text_file_service)). Os membros
//!   `protected` do original são os acessores `file_service`, `lifecycle_service`, `environment_service`,
//!   `text_resource_configuration_service`, `files_configuration_service` e `log_service`, e o `_register` é
//!   [`AbstractTextFileService::register`].
//! - **Injeção.** São 19 serviços, mais do que a tupla do `Injectable` comporta (15): o construtor recebe a struct
//!   [`TextFileServiceDependencies`]. O `IInstantiationService` do original só servia para criar o
//!   `TextFileEditorModelManager` e o `EncodingOracle`: o gerenciador chega pronto em `files` (os modelos dele precisam
//!   do próprio `ITextFileService`, então a montagem do workbench o cria com um proxy atrasado) e o oráculo é criado
//!   aqui com os quatro serviços dele.
//! - **`ICodeEditorService`** é o [`ICodeEditorTransientProperties`] (só `getTransientModelProperties` e
//!   `setTransientModelProperty`, que o "Salvar como" lê).
//! - **Sobrecargas.** `save(resource, ITextFileSaveOptions)` do contrato recebe as opções do nível `ITextFileSaveOptions`,
//!   e o corpo vive em [`AbstractTextFileService::save_with`], que recebe o `ITextFileSaveAsOptions` (o "Salvar como"
//!   repassa as opções inteiras, com `from` e `suggestedTarget`, como o original). `getEncodedReadable` e as sobrecargas
//!   são o método único do contrato, com `Option<EncodedContent>`.
//! - **Modelo de origem do "Salvar como".** `IResolvedTextEditorModel | IResolvedUntitledTextEditorModel | ITextModel`
//!   é o enum interno `SaveAsSource`, e os `instanceof` e o teste de `getEncoding` viram os braços dele.
//! - **`async`.** As funções `async` são `async_fn`; o `await` de uma promessa é `.await?`. `EncodingOracle` é síncrono por
//!   dentro (o `encodingExists` é síncrono no porte) e devolve promessas nos métodos que o contrato `IResourceEncodings`
//!   e o original tornam assíncronos.
//! - **`readonly`.** `isReadonly()` devolve `boolean | IMarkdownString`: a decoração usa o valor como booleano.
//! - **Recursos de `SaveSourceRegistry`.** `textFileCreate.source` e `textFileOverwrite.source` são registrados no uso
//!   (o registro é idempotente).
//! - **Overloads de `EncodingOracle`.** Os métodos de leitura e escrita com `URI | undefined` e as opções são os
//!   inerentes ([`EncodingOracle::get_write_encoding`], [`EncodingOracle::get_preferred_read_encoding_with`] ...), e o
//!   contrato `IResourceEncodings` (com o recurso obrigatório) os chama.
//! - **Ouvintes e ciclos.** Os ouvintes que o serviço instala seguram o serviço por `Weak`.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::buffer::{VSBuffer, VSBufferReadableStream, buffer_to_stream};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::path::PathModule;
use je_base::promise::{MaybePromise, Promise};
use je_base::resources::to_local_resource;
use je_base::stream::{Readable, ReadableStream, ReadableStreamEvents, consume_stream};
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_editor::language_service::ILanguageService;
use je_editor::model::{IndentSize, TextModelUpdateOptions};
use je_editor::modes_registry::PLAINTEXT_LANGUAGE_ID;
use je_editor::services_model::IModelService;
use je_editor::text_model::{TextModel, TextSource, create_text_buffer_factory_from_snapshot};
use je_editor::text_resource_configuration_service::ITextResourceConfigurationService;
use je_platform::dialogs::dialogs::{BaseDialogOptions, Confirmation, DialogDetail, DialogType, DialogTypeOrSeverity, IDialogService, IFileDialogService};
use je_platform::files::file_service::FileContentSource;
use je_platform::files::files::{FileOperationError, FileOperationResult, FileReadStreamOptions, FileStatWithMetadata, ReadFileOptions, WriteFileOptions};
use je_platform::files::i_file_service::IFileService;
use je_platform::log::{LogMessage, LogServiceRef, Logger};
use je_platform::path::path_service::IPathService;
use je_platform::resources::{ext_uri, host_environment, join_path};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::{IWorkspaceContextService, WORKSPACE_EXTENSION};

use crate::common::editor::editor_model::IEditorModel;
use crate::common::editor::{RevertOptions, with_save_source_registry};
use crate::services::decorations::decorations::{DecorationData, DecorationLetter, IDecorationsProvider, IDecorationsService};
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::files::elevated_file_service::IElevatedFileService;
use crate::services::files_configuration::files_configuration_service::{IFilesConfigurationService, ReadonlyUpdate, SingleReadonlyUpdate};
use crate::services::lifecycle::lifecycle::ILifecycleService;
use crate::services::textfile::collaborators::{
    CreateFileOperation, FileOperationUndoRedoInfo, ICodeEditorTransientProperties, IWorkingCopyFileService, MoveOperation, RequiredSourceTargetPair,
};
use crate::services::textfile::encoding::{
    DecodeStreamError, DecodeStreamErrorKind, DecodeStreamOptions, DecodeStreamResult, EncodeOptions, UTF8, UTF8_WITH_BOM, UTF16BE, UTF16LE, encoding_exists, to_decode_stream,
    to_encode_readable,
};
use crate::services::textfile::text_buffer_factory::create_text_buffer_factory_from_stream;
use crate::services::textfile::text_file_editor_model_manager::TextFileEditorModelManager;
use crate::services::textfile::textfiles::{
    BaseTextFileContent, EncodedContent, IResourceEncodings, ITextFileEditorModel, ITextFileEditorModelManager, ITextFileService, ReadTextFileEncodingOptions, ReadTextFileOptions,
    ResolvedDecoding, ResolvedEncoding, ResourceEncoding, TextFileContent, TextFileCreateOperation, TextFileEditorModelResolveOrCreateOptions, TextFileOperationError,
    TextFileOperationResult, TextFileSaveAsOptions, TextFileSaveOptions, TextFileStreamContent, TextOrSnapshot, VSBufferReadable, WriteTextFileOptions, string_to_snapshot,
    text_file_operation_result_of, to_buffer_or_readable,
};
use crate::services::textmodel_resolver::resolver_service::ITextEditorModel;
use crate::services::untitled::untitled_text_editor_model::IUntitledTextEditorModel;
use crate::services::untitled::untitled_text_editor_service::{IUntitledTextEditorModelManager, IUntitledTextEditorService};
use crate::services::working_copy::working_copy::IWorkingCopy;

/// `listErrorForeground`: o id da cor `list.errorForeground` do registro de cores.
const LIST_ERROR_FOREGROUND: &str = "list.errorForeground";

/// O id do codicon `Codicon.lockSmall`.
const CODICON_LOCK_SMALL: &str = "lock-small";

/// `TEXTFILE_SAVE_CREATE_SOURCE`.
fn save_create_source() -> String {
    with_save_source_registry(|registry| registry.register_source("textFileCreate.source", &localize("textFileCreate.source", "File Created", &[])))
}

/// `TEXTFILE_SAVE_REPLACE_SOURCE`.
fn save_replace_source() -> String {
    with_save_source_registry(|registry| registry.register_source("textFileOverwrite.source", &localize("textFileOverwrite.source", "File Replaced", &[])))
}

/// A veracidade de um valor de configuração no JavaScript (o `||` das leituras de configuração).
fn is_truthy(value: &Dynamic) -> bool {
    match value {
        Dynamic::Undefined | Dynamic::Null => false,
        Dynamic::Bool(flag) => *flag,
        Dynamic::Number(number) => *number != 0.0 && !number.is_nan(),
        Dynamic::String(text) => !text.is_empty(),
        _ => true,
    }
}

/// O texto de um valor de configuração (`undefined` quando não é texto).
fn string_of(value: &Dynamic) -> Option<String> {
    match value {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// A lista de textos de um valor de configuração (vazia quando não é uma lista).
fn string_list_of(value: &Dynamic) -> Vec<String> {
    match value {
        Dynamic::Array(items) => items.to_vec().iter().filter_map(string_of).collect(),
        _ => Vec::new(),
    }
}

/// O `VSBufferReadable` compartilhado como o leitor de um dono só que o serviço de arquivos consome.
struct SharedReadable(VSBufferReadable);

impl Readable<VSBuffer> for SharedReadable {
    fn read(&mut self) -> Option<VSBuffer> {
        self.0.borrow_mut().read()
    }
}

impl From<EncodedContent> for FileContentSource {
    fn from(content: EncodedContent) -> Self {
        match content {
            EncodedContent::Buffer(buffer) => FileContentSource::Buffer(buffer),
            EncodedContent::Readable(readable) => FileContentSource::Readable(Box::new(SharedReadable(readable))),
        }
    }
}

/// O `ITextSnapshot` (que já é um `Readable<string>` no original) como o leitor de texto que o `toEncodeReadable` lê.
struct SnapshotTextReadable {
    snapshot: Box<dyn je_editor::model::TextSnapshot>,
}

impl Readable<String> for SnapshotTextReadable {
    fn read(&mut self) -> Option<String> {
        self.snapshot.read().map(|chunk| chunk.to_string_lossy())
    }
}

//#region EncodingOracle

/// `IEncodingOverride`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodingOverride {
    pub parent: Option<URI>,
    pub extension: Option<String>,
    pub encoding: String,
}

/// `EncodingOracle`.
pub struct EncodingOracle {
    base: Rc<Disposable>,
    encoding_overrides: RefCell<Vec<EncodingOverride>>,

    text_resource_configuration_service: Rc<dyn ITextResourceConfigurationService>,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    context_service: Rc<dyn IWorkspaceContextService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
}

impl EncodingOracle {
    /// `constructor(textResourceConfigurationService, environmentService, contextService, uriIdentityService)`.
    pub fn new(
        text_resource_configuration_service: Rc<dyn ITextResourceConfigurationService>,
        environment_service: Rc<dyn IWorkbenchEnvironmentService>,
        context_service: Rc<dyn IWorkspaceContextService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
    ) -> Rc<Self> {
        let oracle = Rc::new(Self {
            base: Disposable::new(),
            encoding_overrides: RefCell::new(Vec::new()),
            text_resource_configuration_service,
            environment_service,
            context_service,
            uri_identity_service,
        });
        oracle.set_encoding_overrides(oracle.get_default_encoding_overrides());

        oracle.register_listeners();

        oracle
    }

    fn register_listeners(self: &Rc<Self>) {
        // Workspace Folder Change
        let this = Rc::downgrade(self);
        self.base.register(self.context_service.on_did_change_workspace_folders().listen(move |_| {
            if let Some(this) = this.upgrade() {
                this.set_encoding_overrides(this.get_default_encoding_overrides());
            }
        }));
    }

    /// `encodingOverrides` (protected getter).
    pub fn encoding_overrides(&self) -> Vec<EncodingOverride> {
        self.encoding_overrides.borrow().clone()
    }

    /// `encodingOverrides = value` (protected setter).
    pub fn set_encoding_overrides(&self, value: Vec<EncodingOverride>) {
        *self.encoding_overrides.borrow_mut() = value;
    }

    fn get_default_encoding_overrides(&self) -> Vec<EncodingOverride> {
        let mut default_encoding_overrides: Vec<EncodingOverride> = Vec::new();

        // Global settings
        default_encoding_overrides.push(EncodingOverride { parent: Some(self.environment_service.user_roaming_data_home()), extension: None, encoding: UTF8.to_string() });

        // Workspace files (via extension and via untitled workspaces location)
        default_encoding_overrides.push(EncodingOverride { parent: None, extension: Some(WORKSPACE_EXTENSION.to_string()), encoding: UTF8.to_string() });
        default_encoding_overrides.push(EncodingOverride { parent: Some(self.environment_service.untitled_workspaces_home()), extension: None, encoding: UTF8.to_string() });

        // Folder Settings
        for folder in self.context_service.get_workspace().folders() {
            default_encoding_overrides.push(EncodingOverride { parent: Some(join_path(&folder.uri, &[".vscode"])), extension: None, encoding: UTF8.to_string() });
        }

        default_encoding_overrides
    }

    /// `getWriteEncoding(resource, options)`.
    pub fn get_write_encoding(&self, resource: Option<&URI>, options: Option<&WriteTextFileOptions>) -> Promise<ResolvedEncoding> {
        let ResourceEncoding { encoding, has_bom } = self.preferred_write_encoding(resource, options.and_then(|options| options.encoding.as_deref()));

        Promise::resolved(ResolvedEncoding { encoding, add_bom: has_bom })
    }

    /// `getPreferredReadEncoding(resource, options, detectedEncoding)` com o recurso opcional.
    pub fn get_preferred_read_encoding_with(&self, resource: Option<&URI>, options: Option<&ReadTextFileEncodingOptions>, detected_encoding: Option<&str>) -> Promise<ResourceEncoding> {
        let mut preferred_encoding: Option<String> = None;

        // Encoding passed in as option
        if let Some(encoding) = options.and_then(|options| options.encoding.as_deref()).filter(|encoding| !encoding.is_empty()) {
            if detected_encoding == Some(UTF8_WITH_BOM) && encoding == UTF8 {
                preferred_encoding = Some(UTF8_WITH_BOM.to_string()); // indicate the file has BOM if we are to resolve with UTF 8
            } else {
                preferred_encoding = Some(encoding.to_string()); // give passed in encoding highest priority
            }
        }
        // Encoding detected
        else if let Some(detected_encoding) = detected_encoding {
            preferred_encoding = Some(detected_encoding.to_string());
        }
        // Encoding configured
        else if string_of(&self.text_resource_configuration_service.get_value(resource, Some("files.encoding"))).as_deref() == Some(UTF8_WITH_BOM) {
            preferred_encoding = Some(UTF8.to_string()); // if we did not detect UTF 8 BOM before, this can only be UTF 8 then
        }

        let encoding = self.validated_encoding_for_resource(resource, preferred_encoding.as_deref());

        Promise::resolved(ResourceEncoding { has_bom: Self::enforces_bom(&encoding), encoding })
    }

    /// `getPreferredWriteEncoding(resource, preferredEncoding)` com o recurso opcional.
    fn preferred_write_encoding(&self, resource: Option<&URI>, preferred_encoding: Option<&str>) -> ResourceEncoding {
        let resource_encoding = self.validated_encoding_for_resource(resource, preferred_encoding);

        ResourceEncoding { has_bom: Self::enforces_bom(&resource_encoding), encoding: resource_encoding }
    }

    /// `encoding === UTF16be || encoding === UTF16le || encoding === UTF8_with_bom`: enforce BOM for certain encodings.
    fn enforces_bom(encoding: &str) -> bool {
        encoding == UTF16BE || encoding == UTF16LE || encoding == UTF8_WITH_BOM
    }

    /// `getUnvalidatedEncodingForResource(resource, preferredEncoding)`.
    pub fn get_unvalidated_encoding_for_resource(&self, resource: Option<&URI>, preferred_encoding: Option<&str>) -> String {
        let file_encoding: Option<String> = if let Some(encoding_override) = self.get_encoding_override(resource) {
            Some(encoding_override) // encoding override always wins
        } else if let Some(preferred_encoding) = preferred_encoding.filter(|encoding| !encoding.is_empty()) {
            Some(preferred_encoding.to_string()) // preferred encoding comes second
        } else {
            string_of(&self.text_resource_configuration_service.get_value(resource, Some("files.encoding"))) // and last we check for settings
        };

        file_encoding.filter(|encoding| !encoding.is_empty()).unwrap_or_else(|| UTF8.to_string())
    }

    fn validated_encoding_for_resource(&self, resource: Option<&URI>, preferred_encoding: Option<&str>) -> String {
        let mut file_encoding = self.get_unvalidated_encoding_for_resource(resource, preferred_encoding);
        if file_encoding != UTF8 && !encoding_exists(&file_encoding) {
            file_encoding = UTF8.to_string();
        }

        file_encoding
    }

    fn get_encoding_override(&self, resource: Option<&URI>) -> Option<String> {
        let resource = resource?;
        let overrides = self.encoding_overrides.borrow();
        for encoding_override in overrides.iter() {
            // check if the resource is child of encoding override path
            if let Some(parent) = &encoding_override.parent
                && self.uri_identity_service.ext_uri().is_equal_or_parent(resource, parent, false, false)
            {
                return Some(encoding_override.encoding.clone());
            }

            // check if the resource extension is equal to encoding override
            if let Some(extension) = &encoding_override.extension
                && ext_uri().extname(resource) == format!(".{extension}")
            {
                return Some(encoding_override.encoding.clone());
            }
        }

        None
    }
}

impl IDisposable for EncodingOracle {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IResourceEncodings for EncodingOracle {
    fn get_preferred_read_encoding(&self, resource: &URI) -> Promise<ResourceEncoding> {
        self.get_preferred_read_encoding_with(Some(resource), None, None)
    }

    fn get_preferred_write_encoding(&self, resource: &URI, preferred_encoding: Option<&str>) -> Promise<ResourceEncoding> {
        Promise::resolved(self.preferred_write_encoding(Some(resource), preferred_encoding))
    }
}

//#endregion

//#region decorations

/// O provedor de decorações dos modelos de arquivo de texto (somente leitura e apagado) que o serviço registra.
struct TextFileModelDecorationsProvider {
    base: Rc<Disposable>,
    on_did_change: Rc<Emitter<Vec<URI>>>,
    files: Rc<dyn ITextFileEditorModelManager>,
}

impl TextFileModelDecorationsProvider {
    fn new(files: Rc<dyn ITextFileEditorModelManager>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());

        let provider = Rc::new(Self { base, on_did_change, files });
        provider.register_listeners();

        provider
    }

    fn register_listeners(&self) {
        // Creates
        let emitter = self.on_did_change.clone();
        self.base.register(self.files.on_did_resolve().listen(move |event| {
            let model = &event.model;
            if model.is_readonly().is_readonly() || model.has_state(crate::services::textfile::textfiles::TextFileEditorModelState::Orphan) {
                emitter.fire(vec![model.resource()]);
            }
        }));

        // Removals: once a text file model is no longer
        // under our control, make sure to signal this as
        // decoration change because from this point on we
        // have no way of updating the decoration anymore.
        let emitter = self.on_did_change.clone();
        self.base.register(self.files.on_did_remove().listen(move |model_uri| emitter.fire(vec![model_uri.clone()])));

        // Changes
        let emitter = self.on_did_change.clone();
        self.base.register(self.files.on_did_change_readonly().listen(move |model| emitter.fire(vec![model.resource()])));
        let emitter = self.on_did_change.clone();
        self.base.register(self.files.on_did_change_orphaned().listen(move |model| emitter.fire(vec![model.resource()])));
    }
}

impl IDisposable for TextFileModelDecorationsProvider {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IDecorationsProvider for TextFileModelDecorationsProvider {
    fn label(&self) -> String {
        localize("textFileModelDecorations", "Text File Model Decorations", &[])
    }

    fn on_did_change(&self) -> Event<Vec<URI>> {
        self.on_did_change.event()
    }

    fn provide_decorations(&self, uri: &URI, _token: CancellationToken) -> MaybePromise<Option<DecorationData>> {
        let Some(model) = self.files.get(uri) else {
            return MaybePromise::Value(None);
        };
        if model.is_disposed() {
            return MaybePromise::Value(None);
        }

        let is_readonly = model.is_readonly().is_readonly();
        let is_orphaned = model.has_state(crate::services::textfile::textfiles::TextFileEditorModelState::Orphan);

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

//#endregion

/// O modelo de origem do "Salvar como": `IResolvedTextEditorModel | IResolvedUntitledTextEditorModel | ITextModel`.
#[derive(Clone)]
enum SaveAsSource {
    TextFile(Rc<dyn ITextFileEditorModel>),
    Untitled(Rc<dyn IUntitledTextEditorModel>),
    TextModel(Rc<TextModel>),
}

impl SaveAsSource {
    /// O `getEncoding()` do modelo, quando ele o tem (o `ITextModel` não tem).
    fn encoding(&self) -> Option<String> {
        match self {
            SaveAsSource::TextFile(model) => model.get_encoding(),
            SaveAsSource::Untitled(model) => model.get_encoding(),
            SaveAsSource::TextModel(_) => None,
        }
    }

    /// O `ITextModel` de onde o conteúdo é lido: o `textEditorModel` do modelo resolvido ou o próprio `ITextModel`.
    fn text_model(&self) -> Option<Rc<TextModel>> {
        match self {
            SaveAsSource::TextFile(model) => {
                if model.is_resolved() {
                    model.text_editor_model()
                } else {
                    None
                }
            }
            SaveAsSource::Untitled(model) => {
                if model.is_resolved() {
                    model.text_editor_model()
                } else {
                    None
                }
            }
            SaveAsSource::TextModel(model) => Some(model.clone()),
        }
    }
}

/// Os serviços que o `AbstractTextFileService` recebe por injeção, na ordem do construtor do original, e o
/// `TextFileEditorModelManager` que ele cria (ver as divergências do módulo).
#[derive(Clone)]
pub struct TextFileServiceDependencies {
    pub file_service: Rc<dyn IFileService>,
    pub untitled_text_editor_service: Rc<dyn IUntitledTextEditorModelManager>,
    pub lifecycle_service: Rc<dyn ILifecycleService>,
    pub model_service: Rc<dyn IModelService>,
    pub environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    pub dialog_service: Rc<dyn IDialogService>,
    pub file_dialog_service: Rc<dyn IFileDialogService>,
    pub text_resource_configuration_service: Rc<dyn ITextResourceConfigurationService>,
    pub files_configuration_service: Rc<dyn IFilesConfigurationService>,
    pub code_editor_service: Rc<dyn ICodeEditorTransientProperties>,
    pub path_service: Rc<dyn IPathService>,
    pub working_copy_file_service: Rc<dyn IWorkingCopyFileService>,
    pub uri_identity_service: Rc<dyn IUriIdentityService>,
    pub workspace_context_service: Rc<dyn IWorkspaceContextService>,
    pub language_service: Rc<dyn ILanguageService>,
    pub log_service: LogServiceRef,
    pub elevated_file_service: Rc<dyn IElevatedFileService>,
    pub decorations_service: Rc<dyn IDecorationsService>,
    /// `this.files = createInstance(TextFileEditorModelManager)`.
    pub files: Rc<TextFileEditorModelManager>,
}

/// `AbstractTextFileService`.
pub struct AbstractTextFileService {
    this: Weak<AbstractTextFileService>,
    base: Rc<Disposable>,

    files: Rc<dyn ITextFileEditorModelManager>,
    untitled: Rc<dyn IUntitledTextEditorModelManager>,

    encoding: RefCell<Option<Rc<EncodingOracle>>>,

    file_service: Rc<dyn IFileService>,
    lifecycle_service: Rc<dyn ILifecycleService>,
    model_service: Rc<dyn IModelService>,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    dialog_service: Rc<dyn IDialogService>,
    file_dialog_service: Rc<dyn IFileDialogService>,
    text_resource_configuration_service: Rc<dyn ITextResourceConfigurationService>,
    files_configuration_service: Rc<dyn IFilesConfigurationService>,
    code_editor_service: Rc<dyn ICodeEditorTransientProperties>,
    path_service: Rc<dyn IPathService>,
    working_copy_file_service: Rc<dyn IWorkingCopyFileService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    workspace_context_service: Rc<dyn IWorkspaceContextService>,
    language_service: Rc<dyn ILanguageService>,
    log_service: LogServiceRef,
    elevated_file_service: Rc<dyn IElevatedFileService>,
    decorations_service: Rc<dyn IDecorationsService>,
}

impl AbstractTextFileService {
    /// `constructor(fileService, untitledTextEditorService, lifecycleService, instantiationService, modelService,
    /// environmentService, dialogService, fileDialogService, textResourceConfigurationService, filesConfigurationService,
    /// codeEditorService, pathService, workingCopyFileService, uriIdentityService, languageService, logService,
    /// elevatedFileService, decorationsService)`.
    pub fn new(dependencies: TextFileServiceDependencies) -> Rc<Self> {
        let base = Disposable::new();

        base.register(dependencies.files.clone());
        let files: Rc<dyn ITextFileEditorModelManager> = dependencies.files;

        let service = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            files,
            untitled: dependencies.untitled_text_editor_service,
            encoding: RefCell::new(None),
            file_service: dependencies.file_service,
            lifecycle_service: dependencies.lifecycle_service,
            model_service: dependencies.model_service,
            environment_service: dependencies.environment_service,
            dialog_service: dependencies.dialog_service,
            file_dialog_service: dependencies.file_dialog_service,
            text_resource_configuration_service: dependencies.text_resource_configuration_service,
            files_configuration_service: dependencies.files_configuration_service,
            code_editor_service: dependencies.code_editor_service,
            path_service: dependencies.path_service,
            working_copy_file_service: dependencies.working_copy_file_service,
            uri_identity_service: dependencies.uri_identity_service,
            workspace_context_service: dependencies.workspace_context_service,
            language_service: dependencies.language_service,
            log_service: dependencies.log_service,
            elevated_file_service: dependencies.elevated_file_service,
            decorations_service: dependencies.decorations_service,
        });

        service.provide_decorations();

        service
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço está vivo enquanto é usado")
    }

    /// `_register(disposable)`: o que a subclasse registra é descartado junto com o serviço.
    pub fn register(&self, disposable: DisposableRef) -> DisposableRef {
        self.base.register(disposable)
    }

    /// `fileService` (protected).
    pub fn file_service(&self) -> Rc<dyn IFileService> {
        self.file_service.clone()
    }

    /// `lifecycleService` (protected).
    pub fn lifecycle_service(&self) -> Rc<dyn ILifecycleService> {
        self.lifecycle_service.clone()
    }

    /// `environmentService` (protected).
    pub fn environment_service(&self) -> Rc<dyn IWorkbenchEnvironmentService> {
        self.environment_service.clone()
    }

    /// `textResourceConfigurationService` (protected).
    pub fn text_resource_configuration_service(&self) -> Rc<dyn ITextResourceConfigurationService> {
        self.text_resource_configuration_service.clone()
    }

    /// `filesConfigurationService` (protected).
    pub fn files_configuration_service(&self) -> Rc<dyn IFilesConfigurationService> {
        self.files_configuration_service.clone()
    }

    /// `logService` (protected).
    pub fn log_service(&self) -> LogServiceRef {
        self.log_service.clone()
    }

    //#region decorations

    fn provide_decorations(&self) {
        // Text file model decorations
        let provider = TextFileModelDecorationsProvider::new(self.files.clone());
        self.base.register(provider.clone());

        self.base.register(self.decorations_service.register_decorations_provider(provider));
    }

    //#endregion

    //#region text file read / write / create

    /// `get encoding(): EncodingOracle`: o oráculo de codificação, criado no primeiro uso.
    pub fn encoding_oracle(&self) -> Rc<EncodingOracle> {
        if let Some(encoding) = self.encoding.borrow().as_ref() {
            return encoding.clone();
        }

        let oracle = EncodingOracle::new(
            self.text_resource_configuration_service.clone(),
            self.environment_service.clone(),
            self.workspace_context_service.clone(),
            self.uri_identity_service.clone(),
        );
        self.base.register(oracle.clone());
        *self.encoding.borrow_mut() = Some(oracle.clone());

        oracle
    }

    /// `ITextFileSaveAsOptions.availableFileSystems` (`options?.availableFileSystems`).
    fn available_file_systems(options: Option<&TextFileSaveAsOptions>) -> Option<Vec<String>> {
        options.and_then(|options| options.options.base.available_file_systems.clone())
    }

    /// As opções de leitura do serviço de arquivos (`IReadTextFileOptions` repassadas como `IReadFileStreamOptions`).
    fn file_read_options(options: Option<&ReadTextFileOptions>) -> Option<ReadFileOptions> {
        options.map(|options| ReadFileOptions {
            stream: FileReadStreamOptions { limit_size: options.limits.and_then(|limits| limits.size).or(options.read.stream.limit_size), ..options.read.stream },
            ..options.read.clone()
        })
    }

    fn do_read(&self, resource: &URI, options: Option<ReadTextFileOptions>, prefer_unbuffered: bool) -> Promise<(je_platform::files::files::FileStreamContent, DecodeStreamResult)> {
        let (this, resource) = (self.rc(), resource.clone());
        async_fn(async move {
            let cts = CancellationTokenSource::new(None);

            // read stream raw (either buffered or unbuffered)
            let read_options = Self::file_read_options(options.as_ref());
            let buffer_stream = if prefer_unbuffered {
                let content = this.file_service.read_file(&resource, read_options, Some(cts.token())).await?;
                je_platform::files::files::FileStreamContent { stat: content.stat, value: buffer_to_stream(content.value) }
            } else {
                this.file_service.read_file_stream(&resource, read_options, Some(cts.token())).await?
            };

            // read through encoding library
            match this.do_get_decoded_stream(Some(&resource), buffer_stream.value.clone(), options.as_ref().map(|options| &options.encoding)).await {
                Ok(decoder) => Ok((buffer_stream, decoder)),
                Err(error) => {
                    // Make sure to cancel reading on error to
                    // stop file service activity as soon as
                    // possible. When for example a large binary
                    // file is read we want to cancel the read
                    // instantly.
                    // Refs:
                    // - https://github.com/microsoft/vscode/issues/138805
                    // - https://github.com/microsoft/vscode/issues/132771
                    cts.dispose_with(true);

                    // special treatment for streams that are binary
                    if DecodeStreamError::from_error(&error).is_some_and(|error| error.decode_stream_error_kind == DecodeStreamErrorKind::StreamIsBinary) {
                        Err(TextFileOperationError::new(localize("fileBinaryError", "File seems to be binary and cannot be opened as text", &[]), TextFileOperationResult::FileIsBinary))
                    }
                    // re-throw any other error as it is
                    else {
                        Err(error)
                    }
                }
            }
        })
    }

    fn do_get_decoded_stream(&self, resource: Option<&URI>, stream: VSBufferReadableStream, options: Option<&ReadTextFileEncodingOptions>) -> Promise<DecodeStreamResult> {
        // read through encoding library
        let this = self.rc();
        let resource = resource.cloned();
        let options = options.cloned();

        let guess_encoding = options.as_ref().and_then(|options| options.auto_guess_encoding) == Some(true)
            || is_truthy(&self.text_resource_configuration_service.get_value(resource.as_ref(), Some("files.autoGuessEncoding")));
        let candidate_guess_encodings = match options.as_ref().and_then(|options| options.candidate_guess_encodings.clone()) {
            Some(candidate_guess_encodings) => candidate_guess_encodings,
            None => string_list_of(&self.text_resource_configuration_service.get_value(resource.as_ref(), Some("files.candidateGuessEncodings"))),
        };

        let (overwrite_this, overwrite_resource, overwrite_options) = (this, resource, options.clone());
        to_decode_stream(
            stream,
            DecodeStreamOptions {
                accept_text_only: options.as_ref().and_then(|options| options.accept_text_only).unwrap_or(false),
                guess_encoding,
                candidate_guess_encodings,
                min_bytes_required_for_detection: None,
                overwrite_encoding: Rc::new(move |detected_encoding: Option<String>| {
                    overwrite_this.validate_detected_encoding_of(overwrite_resource.as_ref(), detected_encoding, overwrite_options.clone())
                }),
            },
        )
    }

    /// `validateDetectedEncoding(resource, detectedEncoding | undefined, options)`.
    fn validate_detected_encoding_of(&self, resource: Option<&URI>, detected_encoding: Option<String>, options: Option<ReadTextFileEncodingOptions>) -> Promise<String> {
        let oracle = self.encoding_oracle();
        let resource = resource.cloned();
        async_fn(async move {
            let ResourceEncoding { encoding, .. } = oracle.get_preferred_read_encoding_with(resource.as_ref(), options.as_ref(), detected_encoding.as_deref()).await?;

            Ok(encoding)
        })
    }

    //#endregion

    //#region save

    /// `save(resource, options)` com o `ITextFileSaveAsOptions`, que o "Salvar como" repassa inteiro.
    pub fn save_with(&self, resource: &URI, options: Option<TextFileSaveAsOptions>) -> Promise<Option<URI>> {
        let (this, resource) = (self.rc(), resource.clone());
        async_fn(async move {
            // Untitled
            if *resource.scheme() == schemas::UNTITLED {
                if let Some(model) = this.untitled.get(&resource) {
                    // Untitled with associated file path don't need to prompt
                    let target_uri: Option<URI> = if model.has_associated_file_path() {
                        Some(this.suggest_save_path(&resource).await?)
                    }
                    // Otherwise ask user
                    else {
                        let suggested = this.suggest_save_path(&resource).await?;
                        this.file_dialog_service.pick_file_to_save(&suggested, Self::available_file_systems(options.as_ref())).await?
                    };

                    // Save as if target provided
                    if let Some(target_uri) = target_uri {
                        return ITextFileService::save_as(&*this, &resource, Some(&target_uri), options).await;
                    }
                }
            }
            // File
            else if let Some(model) = this.files.get(&resource) {
                return Ok(if model.save_text_file(options.unwrap_or_default()).await? { Some(resource) } else { None });
            }

            Ok(None)
        })
    }

    fn do_save_as(&self, source: &URI, target: &URI, options: Option<TextFileSaveAsOptions>) -> Promise<Option<URI>> {
        let (this, source, target) = (self.rc(), source.clone(), target.clone());
        async_fn(async move {
            let resolved_text_model: Option<SaveAsSource> = if *source.scheme() != schemas::UNTITLED {
                this.files.get(&source).filter(|model| model.is_resolved()).map(SaveAsSource::TextFile)
            } else {
                this.untitled.get(&source).filter(|model| model.is_resolved()).map(SaveAsSource::Untitled)
            };

            let success: bool;

            // If the source is an existing resolved file or untitled text model, we can
            // directly use that model to copy the contents to the target destination
            if let Some(resolved_text_model) = resolved_text_model {
                success = this.do_save_as_text_file(resolved_text_model, &source, &target, options).await?;
            }
            // Otherwise if the source can be handled by the file service
            // we can simply invoke the copy() function to save as
            else if this.file_service.has_provider(&source) {
                this.file_service.copy(&source, &target, Some(true)).await?;

                success = true;
            }
            // Finally we simply check if we can find a editor model that
            // would give us access to the contents.
            else if let Some(text_model) = this.model_service.get_model(&source) {
                success = this.do_save_as_text_file(SaveAsSource::TextModel(text_model), &source, &target, options).await?;
            } else {
                success = false;
            }

            if !success {
                return Ok(None);
            }

            // Revert the source
            if let Err(error) = ITextFileService::revert(&*this, &source, None).await {
                // It is possible that reverting the source fails, for example
                // when a remote is disconnected and we cannot read it anymore.
                // However, this should not interrupt the "Save As" flow, so
                // we gracefully catch the error and just log it.

                this.log_service.error_args(&LogMessage::from(&error), &[]);
            }

            // Events
            if *source.scheme() == schemas::UNTITLED {
                this.untitled.notify_did_save(&source, &target);
            }

            Ok(Some(target))
        })
    }

    fn do_save_as_text_file(&self, source_model: SaveAsSource, source: &URI, target: &URI, options: Option<TextFileSaveAsOptions>) -> Promise<bool> {
        let (this, source, target) = (self.rc(), source.clone(), target.clone());
        async_fn(async move {
            // Find source encoding if any
            let source_model_encoding = source_model.encoding();

            // Prefer an existing model if it is already resolved for the given target resource
            let target_exists: bool;
            let target_model: Rc<dyn ITextFileEditorModel>;
            if let Some(existing) = this.files.get(&target).filter(|model| model.is_resolved()) {
                target_exists = true;
                target_model = existing;
            }
            // Otherwise create the target file empty if it does not exist already and resolve it from there
            else {
                target_exists = this.file_service.exists(&target).await?;

                // create target file adhoc if it does not exist yet
                if !target_exists {
                    ITextFileService::create(&*this, vec![TextFileCreateOperation { resource: target.clone(), value: Some(TextOrSnapshot::Text(String::new())), overwrite: None }], None).await?;
                }

                let resolve_options = TextFileEditorModelResolveOrCreateOptions { encoding: source_model_encoding.clone(), ..Default::default() };
                match this.files.resolve(&target, Some(resolve_options)).await {
                    Ok(model) => target_model = model,
                    Err(error) => {
                        // if the target already exists and was not created by us, it is possible
                        // that we cannot resolve the target as text model if it is binary or too
                        // large. in that case we have to delete the target file first and then
                        // re-run the operation.
                        if target_exists
                            && (text_file_operation_result_of(&error) == Some(TextFileOperationResult::FileIsBinary)
                                || FileOperationError::from_error(&error).is_some_and(|error| error.file_operation_result == FileOperationResult::FileTooLarge))
                        {
                            this.file_service.del(&target, None).await?;

                            return this.do_save_as_text_file(source_model, &source, &target, options).await;
                        }

                        return Err(error);
                    }
                }
            }

            // Confirm to overwrite if we have an untitled file with associated file where
            // the file actually exists on disk and we are instructed to save to that file
            // path. This can happen if the file was created after the untitled file was opened.
            // See https://github.com/microsoft/vscode/issues/67946
            let write = match &source_model {
                SaveAsSource::Untitled(model)
                    if model.has_associated_file_path()
                        && target_exists
                        && this.uri_identity_service.ext_uri().is_equal(
                            Some(&target),
                            Some(&to_local_resource(&IWorkingCopy::resource(&**model), this.environment_service.remote_authority().as_deref(), &this.path_service.default_uri_scheme())),
                            false,
                        ) =>
                {
                    this.confirm_overwrite(&target).await?
                }
                _ => true,
            };

            if !write {
                return Ok(false);
            }

            let source_text_model = source_model.text_model();

            let target_text_model = if target_model.is_resolved() { target_model.text_editor_model() } else { None };

            // take over model value, encoding and language (only if more specific) from source model
            if let (Some(source_text_model), Some(target_text_model)) = (&source_text_model, &target_text_model) {
                // encoding
                target_model.update_preferred_encoding(source_model_encoding.as_deref());

                // content
                let factory = create_text_buffer_factory_from_snapshot(source_text_model.create_snapshot(false).as_mut());
                this.model_service.update_model(target_text_model, TextSource::Factory(factory), None);

                // language
                let source_language_id = source_text_model.get_language_id();
                let target_language_id = target_text_model.get_language_id();
                if source_language_id != PLAINTEXT_LANGUAGE_ID && target_language_id == PLAINTEXT_LANGUAGE_ID {
                    target_text_model.set_language(&source_language_id, None); // only use if more specific than plain/text
                }

                // indentation options (preserve tabs vs spaces, tab size, indent size)
                let source_options = source_text_model.get_options();
                target_text_model.update_options(TextModelUpdateOptions {
                    tab_size: Some(source_options.tab_size),
                    indent_size: Some(IndentSize::Size(source_options.indent_size)),
                    insert_spaces: Some(source_options.insert_spaces),
                    ..TextModelUpdateOptions::default()
                });

                // end of line sequence (preserve LF vs CRLF)
                target_text_model.set_eol(source_text_model.get_end_of_line_sequence());

                // transient properties
                if let Some(source_transient_properties) = this.code_editor_service.get_transient_model_properties(source_text_model) {
                    for (key, value) in source_transient_properties {
                        this.code_editor_service.set_transient_model_property(target_text_model, &key, value);
                    }
                }
            }

            // set source options depending on target exists or not
            let mut options = options.unwrap_or_default();
            if options.options.base.source.as_deref().is_none_or(str::is_empty) {
                options.options.base.source = Some(if target_exists { save_replace_source() } else { save_create_source() });
            }

            // save model
            options.from = Some(source);
            target_model.save_text_file(options).await
        })
    }

    fn confirm_overwrite(&self, resource: &URI) -> Promise<bool> {
        let dialog_service = self.dialog_service.clone();
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
        let dialog_service = self.dialog_service.clone();
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
        let (this, resource) = (self.rc(), resource.clone());
        async_fn(async move {
            // Just take the resource as is if the file service can handle it
            if this.file_service.has_provider(&resource) {
                return Ok(resource);
            }

            let remote_authority = this.environment_service.remote_authority();
            let default_file_path = this.file_dialog_service.default_file_path(None).await?;

            // Otherwise try to suggest a path that can be saved
            let mut suggested_filename: Option<String> = None;
            if *resource.scheme() == schemas::UNTITLED
                && let Some(model) = this.untitled.get(&resource)
            {
                // Untitled with associated file path
                if model.has_associated_file_path() {
                    return Ok(to_local_resource(&resource, remote_authority.as_deref(), &this.path_service.default_uri_scheme()));
                }

                // Untitled without associated file path: use name
                // of untitled model if it is a valid path name and
                // figure out the file extension from the mode if any.

                let model_name = IWorkingCopy::name(&*model);
                let name_candidate: String = if this.path_service.has_valid_basename(&join_path(&default_file_path, &[&model_name]), Some(&model_name)).await? {
                    model_name
                } else {
                    ext_uri().basename(&resource, None)
                };

                suggested_filename = match model.get_language_id() {
                    Some(language_id) if !language_id.is_empty() && language_id != PLAINTEXT_LANGUAGE_ID => Some(this.suggest_filename(&language_id, &name_candidate)),
                    _ => Some(name_candidate),
                };
            }

            // Fallback to basename of resource
            let suggested_filename = suggested_filename.filter(|filename| !filename.is_empty()).unwrap_or_else(|| ext_uri().basename(&resource, None));

            // Try to place where last active file was if any
            // Otherwise fallback to user home
            Ok(join_path(&default_file_path, &[&suggested_filename]))
        })
    }

    /// `suggestFilename(languageId, untitledName)`.
    pub fn suggest_filename(&self, language_id: &str, untitled_name: &str) -> String {
        if self.language_service.get_language_name(language_id).is_none() {
            return untitled_name.to_string(); // unknown language, so we cannot suggest a better name
        }

        let environment = host_environment();
        let untitled_extension = PathModule::native(environment.as_ref()).extname(untitled_name);

        let extensions = self.language_service.get_extensions(language_id);
        if extensions.contains(&untitled_extension) {
            return untitled_name.to_string(); // preserve extension if it is compatible with the mode
        }

        if let Some(primary_extension) = extensions.first().filter(|extension| !extension.is_empty()) {
            if !untitled_extension.is_empty() {
                let end = untitled_name.find(&untitled_extension).unwrap_or(0);
                return format!("{}{}", &untitled_name[..end], primary_extension);
            }

            return format!("{untitled_name}{primary_extension}");
        }

        let filenames = self.language_service.get_filenames(language_id);
        if filenames.iter().any(|filename| filename == untitled_name) {
            return untitled_name.to_string(); // preserve name if it is compatible with the mode
        }

        filenames.first().cloned().unwrap_or_else(|| untitled_name.to_string())
    }

    //#endregion
}

impl IDisposable for AbstractTextFileService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl ITextFileService for AbstractTextFileService {
    fn files(&self) -> Rc<dyn ITextFileEditorModelManager> {
        self.files.clone()
    }

    fn untitled(&self) -> Rc<dyn IUntitledTextEditorModelManager> {
        self.untitled.clone()
    }

    fn encoding(&self) -> Rc<dyn IResourceEncodings> {
        self.encoding_oracle()
    }

    //#region text file read / write / create

    fn read(&self, resource: &URI, options: Option<ReadTextFileOptions>) -> Promise<TextFileContent> {
        let (this, resource) = (self.rc(), resource.clone());
        async_fn(async move {
            // optimization: since we know that the caller does not
            // care about buffering, we indicate this to the reader.
            // this reduces all the overhead the buffered reading
            // has (open, read, close) if the provider supports
            // unbuffered reading.
            let (buffer_stream, decoder) = this.do_read(&resource, options, true).await?;

            let events: &dyn ReadableStreamEvents<String> = &*decoder.stream;
            let value = consume_stream(events, Rc::new(|strings: Vec<String>| strings.join(""))).await?;

            Ok(TextFileContent { base: BaseTextFileContent::from_stat(&buffer_stream.stat, Self::detected_encoding_of(&decoder)), value })
        })
    }

    fn read_stream(&self, resource: &URI, options: Option<ReadTextFileOptions>) -> Promise<TextFileStreamContent> {
        let (this, resource) = (self.rc(), resource.clone());
        async_fn(async move {
            let (buffer_stream, decoder) = this.do_read(&resource, options, false).await?;

            let value = create_text_buffer_factory_from_stream(&*decoder.stream).await?;

            Ok(TextFileStreamContent { base: BaseTextFileContent::from_stat(&buffer_stream.stat, Self::detected_encoding_of(&decoder)), value })
        })
    }

    fn create(&self, operations: Vec<TextFileCreateOperation>, undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>> {
        let this = self.rc();
        async_fn(async move {
            let encoded: Vec<Promise<CreateFileOperation>> = operations
                .into_iter()
                .map(|operation| {
                    let TextFileCreateOperation { resource, value, overwrite } = operation;
                    let this = this.clone();
                    async_fn(async move {
                        let contents = ITextFileService::get_encoded_readable(&*this, Some(&resource), value, None).await?;
                        Ok(CreateFileOperation { resource, contents, overwrite })
                    })
                })
                .collect();
            let operations_with_contents = Promise::all(encoded).await?;

            this.working_copy_file_service.create(operations_with_contents, CancellationToken::None, undo_info).await
        })
    }

    fn write(&self, resource: &URI, value: TextOrSnapshot, options: Option<WriteTextFileOptions>) -> Promise<FileStatWithMetadata> {
        let (this, resource) = (self.rc(), resource.clone());
        async_fn(async move {
            let readable = ITextFileService::get_encoded_readable(&*this, Some(&resource), Some(value), options.clone()).await?;
            let content: FileContentSource = readable.map(FileContentSource::from).unwrap_or_else(|| FileContentSource::Buffer(VSBuffer::from_string("")));
            let write_options: Option<WriteFileOptions> = options.as_ref().map(|options| options.write.clone());

            if options.as_ref().and_then(|options| options.write_elevated) == Some(true) && this.elevated_file_service.is_supported(&resource) {
                return this.elevated_file_service.write_file_elevated(&resource, content, write_options).await;
            }

            this.file_service.write_file(&resource, content, write_options).await
        })
    }

    fn get_encoded_readable(&self, resource: Option<&URI>, value: Option<TextOrSnapshot>, options: Option<WriteTextFileOptions>) -> Promise<Option<EncodedContent>> {
        let this = self.rc();
        let resource = resource.cloned();
        async_fn(async move {
            // check for encoding
            let ResolvedEncoding { encoding, add_bom } = this.encoding_oracle().get_write_encoding(resource.as_ref(), options.as_ref()).await?;

            // when encoding is standard skip encoding step
            if encoding == UTF8 && !add_bom {
                return Ok(to_buffer_or_readable(value));
            }

            // otherwise create encoded readable
            let snapshot = match value.unwrap_or_else(|| TextOrSnapshot::Text(String::new())) {
                TextOrSnapshot::Text(text) => string_to_snapshot(&text),
                TextOrSnapshot::Snapshot(snapshot) => snapshot,
            };
            let readable: Box<dyn Readable<VSBuffer>> = Box::new(to_encode_readable(SnapshotTextReadable { snapshot }, &encoding, Some(EncodeOptions { add_bom }))?);

            Ok(Some(EncodedContent::Readable(Rc::new(RefCell::new(readable)))))
        })
    }

    fn get_decoded_stream(&self, resource: Option<&URI>, value: VSBufferReadableStream, options: Option<ReadTextFileEncodingOptions>) -> Promise<Rc<dyn ReadableStream<String>>> {
        let (this, resource) = (self.rc(), resource.cloned());
        async_fn(async move { Ok(this.do_get_decoded_stream(resource.as_ref(), value, options.as_ref()).await?.stream) })
    }

    fn get_encoding(&self, resource: &URI) -> String {
        let model_encoding = if *resource.scheme() == schemas::UNTITLED {
            self.untitled.get(resource).and_then(|model| model.get_encoding())
        } else {
            self.files.get(resource).and_then(|model| model.get_encoding())
        };

        model_encoding.unwrap_or_else(|| self.encoding_oracle().get_unvalidated_encoding_for_resource(Some(resource), None))
    }

    fn resolve_decoding(&self, resource: Option<&URI>, options: Option<ReadTextFileEncodingOptions>) -> Promise<ResolvedDecoding> {
        let (this, resource) = (self.rc(), resource.cloned());
        async_fn(async move {
            let preferred_encoding = this.encoding_oracle().get_preferred_read_encoding_with(resource.as_ref(), options.as_ref(), None).await?.encoding;

            let guess_encoding = options.as_ref().and_then(|options| options.auto_guess_encoding) == Some(true)
                || is_truthy(&this.text_resource_configuration_service.get_value(resource.as_ref(), Some("files.autoGuessEncoding")));
            let candidate_guess_encodings = match options.as_ref().and_then(|options| options.candidate_guess_encodings.clone()) {
                Some(candidate_guess_encodings) => candidate_guess_encodings,
                None => string_list_of(&this.text_resource_configuration_service.get_value(resource.as_ref(), Some("files.candidateGuessEncodings"))),
            };

            Ok(ResolvedDecoding { preferred_encoding, guess_encoding, candidate_guess_encodings })
        })
    }

    fn validate_detected_encoding(&self, resource: Option<&URI>, detected_encoding: &str, options: Option<ReadTextFileEncodingOptions>) -> Promise<String> {
        self.validate_detected_encoding_of(resource, Some(detected_encoding.to_string()), options)
    }

    fn resolve_encoding(&self, resource: Option<&URI>, options: Option<WriteTextFileOptions>) -> Promise<ResolvedEncoding> {
        self.encoding_oracle().get_write_encoding(resource, options.as_ref())
    }

    //#endregion

    //#region save

    fn save(&self, resource: &URI, options: Option<TextFileSaveOptions>) -> Promise<Option<URI>> {
        self.save_with(resource, options.map(|options| TextFileSaveAsOptions { options, ..TextFileSaveAsOptions::default() }))
    }

    fn save_as(&self, source: &URI, target: Option<&URI>, options: Option<TextFileSaveAsOptions>) -> Promise<Option<URI>> {
        let (this, source, target) = (self.rc(), source.clone(), target.cloned());
        async_fn(async move {
            // Get to target resource
            let target = match target {
                Some(target) => target,
                None => {
                    let suggested_target = options.as_ref().and_then(|options| options.suggested_target.clone()).unwrap_or_else(|| source.clone());
                    let suggested = this.suggest_save_path(&suggested_target).await?;
                    match this.file_dialog_service.pick_file_to_save(&suggested, Self::available_file_systems(options.as_ref())).await? {
                        Some(target) => target,
                        None => return Ok(None), // user canceled
                    }
                }
            };

            // Ensure target is not marked as readonly and prompt otherwise
            if this.files_configuration_service.is_readonly(&target, None).is_readonly() {
                let confirmed = this.confirm_make_writeable(&target).await?;
                if !confirmed {
                    return Ok(None);
                } else {
                    let _ = this.files_configuration_service.update_readonly(&target, SingleReadonlyUpdate::Update(ReadonlyUpdate::False));
                }
            }

            // Just save if target is same as models own resource
            if ext_uri().is_equal(Some(&source), Some(&target), false) {
                // force to save, even if not dirty (https://github.com/microsoft/vscode/issues/99619)
                let mut forced = options.unwrap_or_default();
                forced.options.base.force = Some(true);

                return this.save_with(&source, Some(forced)).await;
            }

            // If the target is different but of same identity, we
            // move the source to the target, knowing that the
            // underlying file system cannot have both and then save.
            // However, this will only work if the source exists
            // and is not orphaned, so we need to check that too.
            if this.file_service.has_provider(&source)
                && this.uri_identity_service.ext_uri().is_equal(Some(&source), Some(&target), false)
                && this.file_service.exists(&source).await?
            {
                let operation = MoveOperation { file: RequiredSourceTargetPair { source: source.clone(), target: target.clone() }, overwrite: None };
                this.working_copy_file_service.move_(vec![operation], CancellationToken::None, None).await?;

                // At this point we don't know whether we have a
                // model for the source or the target URI so we
                // simply try to save with both resources.
                let success = this.save_with(&source, options.clone()).await?;
                if success.is_none() {
                    this.save_with(&target, options).await?;
                }

                return Ok(Some(target));
            }

            // Do it
            this.do_save_as(&source, &target, options).await
        })
    }

    //#endregion

    //#region revert

    fn revert(&self, resource: &URI, options: Option<RevertOptions>) -> Promise<()> {
        let (this, resource) = (self.rc(), resource.clone());
        async_fn(async move {
            // Untitled
            if *resource.scheme() == schemas::UNTITLED {
                if let Some(model) = this.untitled.get(&resource) {
                    return IWorkingCopy::revert(&*model, options).await;
                }
            }
            // File
            else if let Some(model) = this.files.get(&resource)
                && (model.is_dirty() || options.is_some_and(|options| options.force == Some(true)))
            {
                return model.revert(options).await;
            }

            Ok(())
        })
    }

    //#endregion

    //#region dirty

    fn is_dirty(&self, resource: &URI) -> bool {
        if *resource.scheme() == schemas::UNTITLED {
            self.untitled.get(resource).is_some_and(|model| IWorkingCopy::is_dirty(&*model))
        } else {
            self.files.get(resource).is_some_and(|model| model.is_dirty())
        }
    }

    //#endregion
}

impl AbstractTextFileService {
    /// `decoder.detected.encoding || UTF8`.
    fn detected_encoding_of(decoder: &DecodeStreamResult) -> String {
        decoder.detected.encoding.clone().filter(|encoding| !encoding.is_empty()).unwrap_or_else(|| UTF8.to_string())
    }
}
