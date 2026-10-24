//! Infraestrutura de teste de `src/vs/workbench/services/keybinding/test/browser/keybindingEditing.test.ts`: o
//! `TextModelResolverService` e o `TestTextFileService` que o `workbenchInstantiationService` entrega ao serviço, sobre o
//! `FileService` real e o `ModelService` real.
//!
//! Divergências:
//! - **Um objeto só para os dois serviços.** O `TextModelResolverService` e o `TextFileService` do original se falam pelo
//!   `TextFileEditorModelManager` (o resolvedor pede o modelo de arquivo ao serviço de texto, e o `save` do serviço grava o
//!   modelo que o resolvedor entregou). O [`TestTextEditingServices`] guarda, por recurso, o modelo vivo, e implementa
//!   `ITextModelService` e `ITextFileService` sobre esse mesmo estado. Nenhum dos dois serviços completos foi portado.
//! - **Modelo de texto.** `createModelReference` lê o arquivo pelo `FileService` e cria o `TextModel` no `ModelService`
//!   (o que o `TextFileEditorModel.resolve` faz); referências seguintes ao mesmo recurso compartilham o modelo.
//! - **Descarte.** Como o `canDispose` do `TextFileEditorModelManager`, o modelo só é descartado quando a última referência
//!   é liberada e ele não está sujo.
//! - **Sujo e salvar.** O modelo está sujo quando o `alternativeVersionId` difere do que foi salvo (o que o
//!   `TextFileEditorModel` calcula pelo mesmo contador); `save` grava o conteúdo do modelo só quando ele está sujo, como o
//!   `TextFileEditorModel.save`, e `write` grava o texto direto no `FileService`.
//! - **`instantiationService.stub(ITextFileService, 'isDirty', true)`** é [`TestTextEditingServices::stub_is_dirty`].
//! - **Operações não exercitadas.** Os demais métodos do `ITextFileService` (arquivos, sem título, codificação, leitura,
//!   criação, `saveAs` e `revert`) não são chamados por esta suíte; uma chamada reprova o caso com o nome da operação.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::buffer::{VSBuffer, VSBufferReadableStream};
use je_base::event::Event;
use je_base::lifecycle::{IDisposable, to_disposable};
use je_base::promise::Promise;
use je_base::stream::ReadableStream;
use je_base::uri::URI;
use je_editor::model::TextSnapshot;
use je_editor::model_service::ModelService;
use je_editor::text_model::TextModel;
use je_platform::files::files::FileStatWithMetadata;
use je_platform::files::i_file_service::IFileService;
use je_workbench::common::editor::RevertOptions;
use je_workbench::common::editor::editor_model::{EditorModel, IEditorModel};
use je_workbench::services::files_configuration::files_configuration_service::Readonly;
use je_workbench::services::textfile::collaborators::FileOperationUndoRedoInfo;
use je_workbench::services::textfile::textfiles::{
    EncodedContent, IResourceEncodings, ITextFileEditorModelManager, ITextFileService, ReadTextFileEncodingOptions, ReadTextFileOptions, ResolvedDecoding,
    ResolvedEncoding, TextFileContent, TextFileCreateOperation, TextFileSaveAsOptions, TextFileSaveOptions, TextFileStreamContent, TextOrSnapshot,
    WriteTextFileOptions, snapshot_to_string,
};
use je_workbench::services::textmodel_resolver::resolver_service::{ITextEditorModel, ITextModelService, TextModelReference};
use je_workbench::services::untitled::untitled_text_editor_service::IUntitledTextEditorModelManager;

/// A chave de um recurso no mapa de modelos vivos.
fn key_of(resource: &URI) -> String {
    resource.to_string(false).to_string_lossy()
}

/// O `IResolvedTextEditorModel` de um arquivo: o modelo de editor que guarda o [`TextModel`].
pub struct TestTextEditorModel {
    base: Rc<EditorModel>,
    model: Rc<TextModel>,
}

impl TestTextEditorModel {
    fn new(model: Rc<TextModel>) -> Rc<Self> {
        let base = EditorModel::new();
        // O modelo nasce resolvido: o texto já está no `TextModel`.
        let _ = base.resolve();
        Rc::new(Self { base, model })
    }
}

impl IDisposable for TestTextEditorModel {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IEditorModel for TestTextEditorModel {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn on_will_dispose(&self) -> Event<()> {
        self.base.on_will_dispose()
    }

    fn resolve(&self) -> Promise<()> {
        self.base.resolve()
    }

    fn is_resolved(&self) -> bool {
        true
    }

    fn is_disposed(&self) -> bool {
        self.base.is_disposed()
    }
}

impl ITextEditorModel for TestTextEditorModel {
    fn text_editor_model(&self) -> Option<Rc<TextModel>> {
        Some(self.model.clone())
    }

    fn is_readonly(&self) -> Readonly {
        Readonly::Bool(false)
    }

    fn create_snapshot(&self) -> Option<Box<dyn TextSnapshot>> {
        Some(self.model.create_snapshot(false))
    }
}

/// O modelo vivo de um recurso, com as referências abertas e a versão salva.
struct Entry {
    editor_model: Rc<TestTextEditorModel>,
    references: Cell<usize>,
    saved_version: Cell<i64>,
}

impl Entry {
    fn is_dirty(&self) -> bool {
        self.editor_model.model.get_alternative_version_id() != self.saved_version.get()
    }
}

/// O `TextModelResolverService` mais o `TestTextFileService` (ver a nota do módulo).
pub struct TestTextEditingServices {
    this: Weak<Self>,
    file_service: Rc<dyn IFileService>,
    model_service: Rc<ModelService>,
    entries: RefCell<HashMap<String, Rc<Entry>>>,
    is_dirty_stub: Cell<Option<bool>>,
}

impl TestTextEditingServices {
    pub fn new(file_service: Rc<dyn IFileService>, model_service: Rc<ModelService>) -> Rc<Self> {
        Rc::new_cyclic(|this| Self { this: this.clone(), file_service, model_service, entries: RefCell::new(Default::default()), is_dirty_stub: Cell::new(None) })
    }

    /// `instantiationService.stub(ITextFileService, 'isDirty', value)`.
    pub fn stub_is_dirty(&self, value: bool) {
        self.is_dirty_stub.set(Some(value));
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço está vivo enquanto é usado")
    }

    /// Libera uma referência; o modelo cai quando é a última e ele não está sujo.
    fn release(&self, key: &str, entry: &Rc<Entry>) {
        let remaining = entry.references.get().saturating_sub(1);
        entry.references.set(remaining);
        if remaining == 0 && !entry.is_dirty() {
            self.entries.borrow_mut().remove(key);
            entry.editor_model.dispose();
            entry.editor_model.model.dispose();
        }
    }
}

impl ITextModelService for TestTextEditingServices {
    fn create_model_reference(&self, resource: &URI) -> Promise<TextModelReference> {
        let (this, resource) = (self.rc(), resource.clone());
        async_fn(async move {
            let key = key_of(&resource);
            let existing = this.entries.borrow().get(&key).cloned();
            let entry = match existing {
                Some(entry) => entry,
                None => {
                    let content = this.file_service.read_file(&resource, None, None).await?.value.to_string();
                    let model = this.model_service.create_model(content, None, Some(&resource), false);
                    let saved_version = model.get_alternative_version_id();
                    let entry = Rc::new(Entry { editor_model: TestTextEditorModel::new(model), references: Cell::new(0), saved_version: Cell::new(saved_version) });
                    this.entries.borrow_mut().insert(key.clone(), entry.clone());
                    entry
                }
            };
            entry.references.set(entry.references.get() + 1);

            let object: Rc<dyn ITextEditorModel> = entry.editor_model.clone();
            let weak = Rc::downgrade(&this);
            Ok(TextModelReference::new(
                object,
                to_disposable(move || {
                    if let Some(this) = weak.upgrade() {
                        this.release(&key, &entry);
                    }
                }),
            ))
        })
    }
}

impl ITextFileService for TestTextEditingServices {
    fn files(&self) -> Rc<dyn ITextFileEditorModelManager> {
        unreachable!("TestTextEditingServices.files não é exercitado por esta suíte")
    }

    fn untitled(&self) -> Rc<dyn IUntitledTextEditorModelManager> {
        unreachable!("TestTextEditingServices.untitled não é exercitado por esta suíte")
    }

    fn encoding(&self) -> Rc<dyn IResourceEncodings> {
        unreachable!("TestTextEditingServices.encoding não é exercitado por esta suíte")
    }

    fn is_dirty(&self, resource: &URI) -> bool {
        if let Some(stubbed) = self.is_dirty_stub.get() {
            return stubbed;
        }
        self.entries.borrow().get(&key_of(resource)).is_some_and(|entry| entry.is_dirty())
    }

    fn save(&self, resource: &URI, _options: Option<TextFileSaveOptions>) -> Promise<Option<URI>> {
        let (this, resource) = (self.rc(), resource.clone());
        async_fn(async move {
            let entry = this.entries.borrow().get(&key_of(&resource)).cloned();
            if let Some(entry) = entry.filter(|entry| entry.is_dirty()) {
                let model = &entry.editor_model.model;
                let content = model.get_value().to_string_lossy();
                this.file_service.write_file(&resource, VSBuffer::from_string(&content).into(), None).await?;
                entry.saved_version.set(model.get_alternative_version_id());
            }
            Ok(Some(resource))
        })
    }

    fn save_as(&self, _resource: &URI, _target_resource: Option<&URI>, _options: Option<TextFileSaveAsOptions>) -> Promise<Option<URI>> {
        unreachable!("TestTextEditingServices.saveAs não é exercitado por esta suíte")
    }

    fn revert(&self, _resource: &URI, _options: Option<RevertOptions>) -> Promise<()> {
        unreachable!("TestTextEditingServices.revert não é exercitado por esta suíte")
    }

    fn read(&self, _resource: &URI, _options: Option<ReadTextFileOptions>) -> Promise<TextFileContent> {
        unreachable!("TestTextEditingServices.read não é exercitado por esta suíte")
    }

    fn read_stream(&self, _resource: &URI, _options: Option<ReadTextFileOptions>) -> Promise<TextFileStreamContent> {
        unreachable!("TestTextEditingServices.readStream não é exercitado por esta suíte")
    }

    fn write(&self, resource: &URI, value: TextOrSnapshot, _options: Option<WriteTextFileOptions>) -> Promise<FileStatWithMetadata> {
        let content = match value {
            TextOrSnapshot::Text(text) => text,
            TextOrSnapshot::Snapshot(mut snapshot) => snapshot_to_string(snapshot.as_mut()),
        };
        self.file_service.write_file(resource, VSBuffer::from_string(&content).into(), None)
    }

    fn create(&self, _operations: Vec<TextFileCreateOperation>, _undo_info: Option<FileOperationUndoRedoInfo>) -> Promise<Vec<FileStatWithMetadata>> {
        unreachable!("TestTextEditingServices.create não é exercitado por esta suíte")
    }

    fn get_encoded_readable(&self, _resource: Option<&URI>, _value: Option<TextOrSnapshot>, _options: Option<WriteTextFileOptions>) -> Promise<Option<EncodedContent>> {
        unreachable!("TestTextEditingServices.getEncodedReadable não é exercitado por esta suíte")
    }

    fn get_decoded_stream(&self, _resource: Option<&URI>, _value: VSBufferReadableStream, _options: Option<ReadTextFileEncodingOptions>) -> Promise<Rc<dyn ReadableStream<String>>> {
        unreachable!("TestTextEditingServices.getDecodedStream não é exercitado por esta suíte")
    }

    fn get_encoding(&self, _resource: &URI) -> String {
        unreachable!("TestTextEditingServices.getEncoding não é exercitado por esta suíte")
    }

    fn resolve_decoding(&self, _resource: Option<&URI>, _options: Option<ReadTextFileEncodingOptions>) -> Promise<ResolvedDecoding> {
        unreachable!("TestTextEditingServices.resolveDecoding não é exercitado por esta suíte")
    }

    fn resolve_encoding(&self, _resource: Option<&URI>, _options: Option<WriteTextFileOptions>) -> Promise<ResolvedEncoding> {
        unreachable!("TestTextEditingServices.resolveEncoding não é exercitado por esta suíte")
    }

    fn validate_detected_encoding(&self, _resource: Option<&URI>, _detected_encoding: &str, _options: Option<ReadTextFileEncodingOptions>) -> Promise<String> {
        unreachable!("TestTextEditingServices.validateDetectedEncoding não é exercitado por esta suíte")
    }
}
