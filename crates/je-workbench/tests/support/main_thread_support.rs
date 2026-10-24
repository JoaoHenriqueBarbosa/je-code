//! Dublês que `mainThreadDocumentsAndEditors.test.ts` e `mainThreadEditors.test.ts` montam com `mock<T>()`
//! (`base/test/common/mock.ts`: um objeto vazio que o TypeScript trata como `T`, e que falha com `TypeError` ao chamar um
//! membro que a suíte não definiu), mais o `SingleProxyRPCProtocol` dessas suítes.
//!
//! Divergências:
//! - **`mock<T>()`** não existe em Rust: cada dublê implementa o contrato inteiro, e o membro que o original não
//!   sobrescreve é `unreachable!` (o `TypeError` do original).
//! - **`SingleProxyRPCProtocol(thing)`** devolve o mesmo objeto para qualquer identificador. O `IMainContext` do porte
//!   guarda o proxy com a forma do identificador (`Rc<dyn ExtHostXxxShape>`), então o [`FixedProxyContext`] devolve o
//!   mesmo [`RecordingProxy`], visto pela forma de cada um dos três identificadores que as suítes usam
//!   (`ExtHostDocumentsAndEditors`, `ExtHostDocuments` e `ExtHostEditors`). O `thing` do original só define três métodos
//!   (`$acceptDocumentsAndEditorsDelta`, `$acceptEditorDiffInformation` e `$acceptEditorPropertiesChanged`); os métodos
//!   de `ExtHostDocumentsShape` e `ExtHostEditorsShape` que o original deixaria sem definição não fazem nada aqui.
//! - **`ITextFileService`** das duas suítes: `isDirty() { return false; }`, `files` com `onDidSave`, `onDidRevert`,
//!   `onDidChangeDirty` e `onDidChangeEncoding` (`Event.None`), `untitled` com `onDidChangeEncoding` e
//!   `getEncoding() { return 'utf8'; }`. Os outros eventos dos gerenciadores também são `Event.None` (o original os deixa
//!   `undefined`, e ninguém os lê).
//! - **`IWorkingCopyFileService`** com `onDidRunWorkingCopyFileOperation = Event.None`; os outros contratos de evento do
//!   `TestWorkingCopyFileService` do original (que estende o serviço real) também não disparam.
//! - **`IPaneCompositePartService`** com os dois eventos `Event.None` e `getActivePaneComposite() { return undefined; }`.
//! - **`IQuickDiffModelService`** com `createQuickDiffModelReference() { return undefined; }`.
//! - **`null!` do `ITextModelService`**: [`UnusedTextModelService`] falha ao ser usado, como chamar um método de `null`.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::buffer::VSBufferReadableStream;
use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::files::files::FileStatWithMetadata;
use je_workbench::api::common::ext_host_protocol::{
    DocumentsAndEditorsDelta, EditorPropertiesChangeData, ExtHostDocumentsAndEditorsShape, ExtHostDocumentsShape, ExtHostEditorsShape, IMainContext, ITextEditorDiffInformation,
    SerializedModelContentChangedEvent, TextEditorPositionData,
};
use je_workbench::common::editor::RevertOptions;
use je_workbench::common::views::ViewContainerLocation;
use je_workbench::contrib::scm::browser::quick_diff_model::{IQuickDiffModelService, QuickDiffModelOptions, QuickDiffModelReference};
use je_workbench::services::panecomposite::panecomposite::{IPaneComposite, IPaneCompositePartService, PaneCompositeEvent};
use je_workbench::services::textfile::collaborators::{FileOperationUndoRedoInfo, IProgress, IWorkingCopyFileService, ProgressStep};
use je_workbench::services::textfile::textfiles::{
    EncodedContent, IResourceEncodings, ISaveErrorHandler, ITextFileEditorModel, ITextFileEditorModelManager, ITextFileSaveParticipant, ITextFileService, ReadTextFileEncodingOptions,
    ReadTextFileOptions, ResolvedDecoding, ResolvedEncoding, TextFileContent, TextFileCreateOperation, TextFileEditorModelResolveOrCreateOptions, TextFileResolveEvent,
    TextFileSaveAsOptions, TextFileSaveEvent, TextFileSaveOptions, TextFileSaveParticipantContext, TextFileStreamContent, TextOrSnapshot, WriteTextFileOptions,
};
use je_workbench::services::textmodel_resolver::resolver_service::{ITextModelService, TextModelReference};
use je_workbench::services::untitled::untitled_text_editor_model::IUntitledTextEditorModel;
use je_workbench::services::untitled::untitled_text_editor_service::{
    IUntitledTextEditorModelManager, IUntitledTextEditorService, UntitledTextEditorModelSaveEvent, UntitledTextEditorOptions,
};

// ---- SingleProxyRPCProtocol

/// O objeto de `SingleProxyRPCProtocol({ $acceptDocumentsAndEditorsDelta, $acceptEditorDiffInformation,
/// $acceptEditorPropertiesChanged })`: guarda os deltas e conta as mudanças de propriedades.
#[derive(Default)]
pub struct RecordingProxy {
    /// `deltas`.
    pub deltas: RefCell<Vec<DocumentsAndEditorsDelta>>,
    /// `propertyChanges`.
    pub property_changes: Cell<usize>,
}

impl ExtHostDocumentsAndEditorsShape for RecordingProxy {
    fn rpc_accept_documents_and_editors_delta(&self, delta: DocumentsAndEditorsDelta) {
        self.deltas.borrow_mut().push(delta);
    }
}

impl ExtHostEditorsShape for RecordingProxy {
    fn rpc_accept_editor_properties_changed(&self, _id: &str, _props: EditorPropertiesChangeData) {
        self.property_changes.set(self.property_changes.get() + 1);
    }

    fn rpc_accept_editor_position_data(&self, _data: TextEditorPositionData) {}

    fn rpc_accept_editor_diff_information(&self, _id: &str, _diff_information: Option<Vec<ITextEditorDiffInformation>>) {}
}

impl ExtHostDocumentsShape for RecordingProxy {
    fn rpc_accept_model_language_changed(&self, _uri: &URI, _new_language_id: &str) {}

    fn rpc_accept_model_saved(&self, _uri: &URI) {}

    fn rpc_accept_dirty_state_changed(&self, _uri: &URI, _is_dirty: bool) {}

    fn rpc_accept_encoding_changed(&self, _uri: &URI, _encoding: &str) {}

    fn rpc_accept_model_changed(&self, _uri: &URI, _e: SerializedModelContentChangedEvent, _is_dirty: bool) {}
}

/// O contexto de `SingleProxyRPCProtocol(thing)`: qualquer identificador devolve o mesmo objeto.
pub struct FixedProxyContext {
    proxy: Rc<RecordingProxy>,
}

impl FixedProxyContext {
    pub fn new(proxy: Rc<RecordingProxy>) -> Rc<dyn IMainContext> {
        Rc::new(Self { proxy })
    }
}

impl IMainContext for FixedProxyContext {
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any> {
        match sid {
            "ExtHostDocumentsAndEditors" => {
                let shape: Rc<dyn ExtHostDocumentsAndEditorsShape> = self.proxy.clone();
                Rc::new(shape)
            }
            "ExtHostDocuments" => {
                let shape: Rc<dyn ExtHostDocumentsShape> = self.proxy.clone();
                Rc::new(shape)
            }
            "ExtHostEditors" => {
                let shape: Rc<dyn ExtHostEditorsShape> = self.proxy.clone();
                Rc::new(shape)
            }
            other => unreachable!("o objeto do SingleProxyRPCProtocol não tem forma para '{other}'"),
        }
    }

    fn set_erased(&self, _sid: &'static str, _value: Rc<dyn Any>) {}
}

// ---- ITextFileService

/// `files` do `ITextFileService` das suítes.
struct NoEventFilesManager;

impl ITextFileEditorModelManager for NoEventFilesManager {
    fn on_did_create(&self) -> Event<Rc<dyn ITextFileEditorModel>> {
        Event::none()
    }

    fn on_did_resolve(&self) -> Event<TextFileResolveEvent> {
        Event::none()
    }

    fn on_did_change_dirty(&self) -> Event<Rc<dyn ITextFileEditorModel>> {
        Event::none()
    }

    fn on_did_change_readonly(&self) -> Event<Rc<dyn ITextFileEditorModel>> {
        Event::none()
    }

    fn on_did_remove(&self) -> Event<URI> {
        Event::none()
    }

    fn on_did_change_orphaned(&self) -> Event<Rc<dyn ITextFileEditorModel>> {
        Event::none()
    }

    fn on_did_change_encoding(&self) -> Event<Rc<dyn ITextFileEditorModel>> {
        Event::none()
    }

    fn on_did_save_error(&self) -> Event<Rc<dyn ITextFileEditorModel>> {
        Event::none()
    }

    fn on_did_save(&self) -> Event<TextFileSaveEvent> {
        Event::none()
    }

    fn on_did_revert(&self) -> Event<Rc<dyn ITextFileEditorModel>> {
        Event::none()
    }

    fn models(&self) -> Vec<Rc<dyn ITextFileEditorModel>> {
        unreachable!("TestTextFileService.files.models não é definido pela suíte")
    }

    fn set_save_error_handler(&self, _handler: Rc<dyn ISaveErrorHandler>) {
        unreachable!("TestTextFileService.files.saveErrorHandler não é definido pela suíte")
    }

    fn save_error_handler(&self) -> Rc<dyn ISaveErrorHandler> {
        unreachable!("TestTextFileService.files.saveErrorHandler não é definido pela suíte")
    }

    fn get(&self, _resource: &URI) -> Option<Rc<dyn ITextFileEditorModel>> {
        unreachable!("TestTextFileService.files.get não é definido pela suíte")
    }

    fn resolve(&self, _resource: &URI, _options: Option<TextFileEditorModelResolveOrCreateOptions>) -> Promise<Rc<dyn ITextFileEditorModel>> {
        unreachable!("TestTextFileService.files.resolve não é definido pela suíte")
    }

    fn add_save_participant(&self, _participant: Rc<dyn ITextFileSaveParticipant>) -> DisposableRef {
        unreachable!("TestTextFileService.files.addSaveParticipant não é definido pela suíte")
    }

    fn run_save_participants(
        &self,
        _model: Rc<dyn ITextFileEditorModel>,
        _context: TextFileSaveParticipantContext,
        _progress: Rc<dyn IProgress<ProgressStep>>,
        _token: CancellationToken,
    ) -> Promise<()> {
        unreachable!("TestTextFileService.files.runSaveParticipants não é definido pela suíte")
    }

    fn can_dispose(&self, _model: Rc<dyn ITextFileEditorModel>) -> Option<Promise<()>> {
        unreachable!("TestTextFileService.files.canDispose não é definido pela suíte")
    }
}

/// `untitled` do `ITextFileService` das suítes.
struct NoEventUntitledManager;

impl IUntitledTextEditorService for NoEventUntitledManager {
    fn on_did_save(&self) -> Event<UntitledTextEditorModelSaveEvent> {
        Event::none()
    }

    fn on_did_change_dirty(&self) -> Event<Rc<dyn IUntitledTextEditorModel>> {
        Event::none()
    }

    fn on_did_change_encoding(&self) -> Event<Rc<dyn IUntitledTextEditorModel>> {
        Event::none()
    }

    fn on_did_change_label(&self) -> Event<Rc<dyn IUntitledTextEditorModel>> {
        Event::none()
    }

    fn on_did_create(&self) -> Event<Rc<dyn IUntitledTextEditorModel>> {
        Event::none()
    }

    fn on_will_dispose(&self) -> Event<Rc<dyn IUntitledTextEditorModel>> {
        Event::none()
    }

    fn create(&self, _options: Option<UntitledTextEditorOptions>) -> Rc<dyn IUntitledTextEditorModel> {
        unreachable!("TestTextFileService.untitled.create não é definido pela suíte")
    }

    fn get(&self, _resource: &URI) -> Option<Rc<dyn IUntitledTextEditorModel>> {
        unreachable!("TestTextFileService.untitled.get não é definido pela suíte")
    }

    fn get_value(&self, _resource: &URI) -> Option<String> {
        unreachable!("TestTextFileService.untitled.getValue não é definido pela suíte")
    }

    fn resolve(&self, _options: Option<UntitledTextEditorOptions>) -> Promise<Rc<dyn IUntitledTextEditorModel>> {
        unreachable!("TestTextFileService.untitled.resolve não é definido pela suíte")
    }

    fn is_untitled_with_associated_resource(&self, _resource: &URI) -> bool {
        unreachable!("TestTextFileService.untitled.isUntitledWithAssociatedResource não é definido pela suíte")
    }

    fn can_dispose(&self, _model: &Rc<dyn IUntitledTextEditorModel>) -> Option<Promise<()>> {
        unreachable!("TestTextFileService.untitled.canDispose não é definido pela suíte")
    }
}

impl IUntitledTextEditorModelManager for NoEventUntitledManager {
    fn notify_did_save(&self, _source: &URI, _target: &URI) {
        unreachable!("TestTextFileService.untitled.notifyDidSave não é definido pela suíte")
    }
}

/// O `ITextFileService` das suítes: `isDirty` falso, `getEncoding` `utf8` e os eventos de `files` e `untitled` que
/// nunca disparam.
pub struct StubTextFileService;

impl ITextFileService for StubTextFileService {
    fn files(&self) -> Rc<dyn ITextFileEditorModelManager> {
        Rc::new(NoEventFilesManager)
    }

    fn untitled(&self) -> Rc<dyn IUntitledTextEditorModelManager> {
        Rc::new(NoEventUntitledManager)
    }

    fn encoding(&self) -> Rc<dyn IResourceEncodings> {
        unreachable!("TestTextFileService.encoding não é definido pela suíte")
    }

    fn is_dirty(&self, _resource: &URI) -> bool {
        false
    }

    fn save(&self, _resource: &URI, _options: Option<TextFileSaveOptions>) -> Promise<Option<URI>> {
        unreachable!("TestTextFileService.save não é definido pela suíte")
    }

    fn save_as(&self, _resource: &URI, _target_resource: Option<&URI>, _options: Option<TextFileSaveAsOptions>) -> Promise<Option<URI>> {
        unreachable!("TestTextFileService.saveAs não é definido pela suíte")
    }

    fn revert(&self, _resource: &URI, _options: Option<RevertOptions>) -> Promise<()> {
        unreachable!("TestTextFileService.revert não é definido pela suíte")
    }

    fn read(&self, _resource: &URI, _options: Option<ReadTextFileOptions>) -> Promise<TextFileContent> {
        unreachable!("TestTextFileService.read não é definido pela suíte")
    }

    fn read_stream(&self, _resource: &URI, _options: Option<ReadTextFileOptions>) -> Promise<TextFileStreamContent> {
        unreachable!("TestTextFileService.readStream não é definido pela suíte")
    }

    fn write(&self, _resource: &URI, _value: TextOrSnapshot, _options: Option<WriteTextFileOptions>) -> Promise<FileStatWithMetadata> {
        unreachable!("TestTextFileService.write não é definido pela suíte")
    }

    fn create(&self, _operations: Vec<TextFileCreateOperation>, _undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>> {
        unreachable!("TestTextFileService.create não é definido pela suíte")
    }

    fn get_encoded_readable(&self, _resource: Option<&URI>, _value: Option<TextOrSnapshot>, _options: Option<WriteTextFileOptions>) -> Promise<Option<EncodedContent>> {
        unreachable!("TestTextFileService.getEncodedReadable não é definido pela suíte")
    }

    fn get_decoded_stream(
        &self,
        _resource: Option<&URI>,
        _value: VSBufferReadableStream,
        _options: Option<ReadTextFileEncodingOptions>,
    ) -> Promise<Rc<dyn je_base::stream::ReadableStream<String>>> {
        unreachable!("TestTextFileService.getDecodedStream não é definido pela suíte")
    }

    fn get_encoding(&self, _resource: &URI) -> String {
        "utf8".to_string()
    }

    fn resolve_decoding(&self, _resource: Option<&URI>, _options: Option<ReadTextFileEncodingOptions>) -> Promise<ResolvedDecoding> {
        unreachable!("TestTextFileService.resolveDecoding não é definido pela suíte")
    }

    fn resolve_encoding(&self, _resource: Option<&URI>, _options: Option<WriteTextFileOptions>) -> Promise<ResolvedEncoding> {
        unreachable!("TestTextFileService.resolveEncoding não é definido pela suíte")
    }

    fn validate_detected_encoding(&self, _resource: Option<&URI>, _detected_encoding: &str, _options: Option<ReadTextFileEncodingOptions>) -> Promise<String> {
        unreachable!("TestTextFileService.validateDetectedEncoding não é definido pela suíte")
    }
}

// ---- demais serviços

/// `IWorkingCopyFileService` cujos eventos nunca disparam.
pub struct NoEventWorkingCopyFileService;

impl IWorkingCopyFileService for NoEventWorkingCopyFileService {
    fn on_will_run_working_copy_file_operation(&self) -> Event<je_workbench::services::textfile::collaborators::WorkingCopyFileEvent> {
        Event::none()
    }

    fn on_did_fail_working_copy_file_operation(&self) -> Event<je_workbench::services::textfile::collaborators::WorkingCopyFileEvent> {
        Event::none()
    }

    fn on_did_run_working_copy_file_operation(&self) -> Event<je_workbench::services::textfile::collaborators::WorkingCopyFileEvent> {
        Event::none()
    }
}

/// O `null!` que a suíte passa como `ITextModelService`: qualquer uso falha.
pub struct UnusedTextModelService;

impl ITextModelService for UnusedTextModelService {
    fn create_model_reference(&self, _resource: &URI) -> Promise<TextModelReference> {
        unreachable!("Cannot read properties of null (reading 'createModelReference')")
    }
}

/// `IPaneCompositePartService` sem painel ativo e sem eventos.
pub struct StubPaneCompositePartService;

impl IPaneCompositePartService for StubPaneCompositePartService {
    fn on_did_pane_composite_open(&self) -> Event<PaneCompositeEvent> {
        Event::none()
    }

    fn on_did_pane_composite_close(&self) -> Event<PaneCompositeEvent> {
        Event::none()
    }

    fn get_active_pane_composite(&self, _view_container_location: ViewContainerLocation) -> Option<Rc<dyn IPaneComposite>> {
        None
    }
}

/// `IQuickDiffModelService` que nunca devolve referência.
pub struct StubQuickDiffModelService;

impl IQuickDiffModelService for StubQuickDiffModelService {
    fn create_quick_diff_model_reference(&self, _resource: &URI, _options: Option<QuickDiffModelOptions>) -> Option<QuickDiffModelReference> {
        None
    }
}

/// O `ITextFileService` de `mainThreadEditors.test.ts`: o [`StubTextFileService`] mais `create` (que registra os recursos em
/// `createdResources` e devolve uma lista vazia, o `Object.create(null)` do original) e `getEncodedReadable` (que devolve
/// `undefined`). O que o original não define segue como o [`StubTextFileService`].
pub struct RecordingTextFileService {
    inner: StubTextFileService,
    /// `createdResources`.
    pub created_resources: Rc<RefCell<Vec<URI>>>,
}

impl RecordingTextFileService {
    pub fn new(created_resources: Rc<RefCell<Vec<URI>>>) -> Self {
        Self { inner: StubTextFileService, created_resources }
    }
}

impl ITextFileService for RecordingTextFileService {
    fn files(&self) -> Rc<dyn ITextFileEditorModelManager> {
        self.inner.files()
    }

    fn untitled(&self) -> Rc<dyn IUntitledTextEditorModelManager> {
        self.inner.untitled()
    }

    fn encoding(&self) -> Rc<dyn IResourceEncodings> {
        self.inner.encoding()
    }

    fn is_dirty(&self, resource: &URI) -> bool {
        self.inner.is_dirty(resource)
    }

    fn save(&self, resource: &URI, options: Option<TextFileSaveOptions>) -> Promise<Option<URI>> {
        self.inner.save(resource, options)
    }

    fn save_as(&self, resource: &URI, target_resource: Option<&URI>, options: Option<TextFileSaveAsOptions>) -> Promise<Option<URI>> {
        self.inner.save_as(resource, target_resource, options)
    }

    fn revert(&self, resource: &URI, options: Option<RevertOptions>) -> Promise<()> {
        self.inner.revert(resource, options)
    }

    fn read(&self, resource: &URI, options: Option<ReadTextFileOptions>) -> Promise<TextFileContent> {
        self.inner.read(resource, options)
    }

    fn read_stream(&self, resource: &URI, options: Option<ReadTextFileOptions>) -> Promise<TextFileStreamContent> {
        self.inner.read_stream(resource, options)
    }

    fn write(&self, resource: &URI, value: TextOrSnapshot, options: Option<WriteTextFileOptions>) -> Promise<FileStatWithMetadata> {
        self.inner.write(resource, value, options)
    }

    fn create(&self, operations: Vec<TextFileCreateOperation>, _undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>> {
        for operation in operations {
            self.created_resources.borrow_mut().push(operation.resource);
        }
        Promise::resolved(Vec::new())
    }

    fn get_encoded_readable(&self, _resource: Option<&URI>, _value: Option<TextOrSnapshot>, _options: Option<WriteTextFileOptions>) -> Promise<Option<EncodedContent>> {
        Promise::resolved(None)
    }

    fn get_decoded_stream(
        &self,
        resource: Option<&URI>,
        value: VSBufferReadableStream,
        options: Option<ReadTextFileEncodingOptions>,
    ) -> Promise<Rc<dyn je_base::stream::ReadableStream<String>>> {
        self.inner.get_decoded_stream(resource, value, options)
    }

    fn get_encoding(&self, resource: &URI) -> String {
        self.inner.get_encoding(resource)
    }

    fn resolve_decoding(&self, resource: Option<&URI>, options: Option<ReadTextFileEncodingOptions>) -> Promise<ResolvedDecoding> {
        self.inner.resolve_decoding(resource, options)
    }

    fn resolve_encoding(&self, resource: Option<&URI>, options: Option<WriteTextFileOptions>) -> Promise<ResolvedEncoding> {
        self.inner.resolve_encoding(resource, options)
    }

    fn validate_detected_encoding(&self, resource: Option<&URI>, detected_encoding: &str, options: Option<ReadTextFileEncodingOptions>) -> Promise<String> {
        self.inner.validate_detected_encoding(resource, detected_encoding, options)
    }
}
