//! Porte de `src/vs/workbench/common/editor/textEditorModel.ts`: o `BaseTextEditorModel`, que usa o modelo de texto do
//! editor (`TextModel`) e é a base do modelo de arquivo de texto e do modelo sem título.
//!
//! Divergências:
//! - **Herança.** `BaseTextEditorModel extends EditorModel` é a struct [`BaseTextEditorModel`] (em `Rc`) que guarda o
//!   [`EditorModel`] e cujo dono (a subclasse) a embute. As chamadas virtuais da base para a subclasse (`this.dispose()`
//!   do descarte do modelo de texto) passam pelo `owner`, um `Weak<dyn IDisposable>` da subclasse; as chamadas da subclasse
//!   para a base (`super.installModelListeners`, `super.autoDetectLanguage`, `super.dispose`) são chamadas diretas.
//! - **`@IModelService`.** O `IModelService` do porte é o recorte com `getModel`; o contrato inteiro (`createModel`,
//!   `updateModel`, `destroyModel`) é o do [`ModelService`], que a base recebe concreto.
//! - **`ITextModel`** é o [`TextModel`] concreto; `ITextBufferFactory` é o [`TextBufferFactoryRef`] (consumido uma vez);
//!   `ILanguageSelection` entra por `language_id()` (o `TextModel` do porte guarda só o identificador).
//! - **`this.register`.** O `Disposable` do original é o `DisposableStore` `store`, descartado em [`BaseTextEditorModel::dispose`].
//! - **`isReadonly(): boolean | IMarkdownString`** é [`Readonly`].
//! - **Estado compartilhado com os fechos.** O que os ouvintes e a detecção de linguagem leem depois (`textEditorModelHandle`,
//!   `_languageChangeSource`, `_blockLanguageChangeListener`) fica em `Rc<..>`/`Cell` clonados para dentro dos fechos.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::ThrottledDelayer;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, MutableDisposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_base::{JsString, async_::async_fn};
use je_editor::language_service::ILanguageService;
use je_editor::model::TextSnapshot;
use je_editor::model_service::ModelService;
use je_editor::modes_registry::PLAINTEXT_LANGUAGE_ID;
use je_editor::services_model::IModelService;
use je_editor::text_model::TextModel;
use je_editor::text_model_edit_source::TextModelEditSource;

use crate::common::editor::editor_model::EditorModel;
use crate::services::files_configuration::files_configuration_service::Readonly;
use crate::services::textfile::collaborators::{ILanguageDetectionService, IAccessibilityService, LANGUAGE_DETECTION_LANGUAGE_EVENT_SOURCE};
use crate::services::textfile::textfiles::TextBufferFactoryRef;

/// `ModelConstants.FIRST_LINE_DETECTION_LENGTH_LIMIT`.
pub const FIRST_LINE_DETECTION_LENGTH_LIMIT: usize = 1000;

/// `BaseTextEditorModel.AUTO_DETECT_LANGUAGE_THROTTLE_DELAY`.
const AUTO_DETECT_LANGUAGE_THROTTLE_DELAY: u64 = 600;

/// `_languageChangeSource: 'user' | 'api' | undefined` (sem o `undefined`, que é `None`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LanguageChangeSource {
    User,
    Api,
}

/// `BaseTextEditorModel`.
pub struct BaseTextEditorModel {
    this: Weak<BaseTextEditorModel>,
    /// A subclasse, para o `this.dispose()` virtual.
    owner: Weak<dyn IDisposable>,
    editor_model: EditorModel,
    store: Rc<DisposableStore>,

    model_service: Rc<ModelService>,
    language_service: Rc<dyn ILanguageService>,
    language_detection_service: Rc<dyn ILanguageDetectionService>,
    accessibility_service: Rc<dyn IAccessibilityService>,

    /// `textEditorModelHandle`.
    text_editor_model_handle: Rc<RefCell<Option<URI>>>,
    created_editor_model: Cell<bool>,
    model_dispose_listener: Rc<MutableDisposable<dyn IDisposable>>,
    auto_detect_language_throttler: ThrottledDelayer<()>,

    block_language_change_listener: Rc<Cell<bool>>,
    language_change_source: Rc<Cell<Option<LanguageChangeSource>>>,
}

impl BaseTextEditorModel {
    /// `constructor(modelService, languageService, languageDetectionService, accessibilityService, textEditorModelHandle?)`.
    pub fn new(
        owner: Weak<dyn IDisposable>,
        model_service: Rc<ModelService>,
        language_service: Rc<dyn ILanguageService>,
        language_detection_service: Rc<dyn ILanguageDetectionService>,
        accessibility_service: Rc<dyn IAccessibilityService>,
        text_editor_model_handle: Option<URI>,
    ) -> Rc<Self> {
        let store = DisposableStore::new();
        let model_dispose_listener = MutableDisposable::new();
        store.add(model_dispose_listener.clone());

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            owner,
            editor_model: EditorModel::new_inner(),
            store,
            model_service,
            language_service,
            language_detection_service,
            accessibility_service,
            text_editor_model_handle: Rc::new(RefCell::new(None)),
            created_editor_model: Cell::new(false),
            model_dispose_listener,
            auto_detect_language_throttler: ThrottledDelayer::new(AUTO_DETECT_LANGUAGE_THROTTLE_DELAY),
            block_language_change_listener: Rc::new(Cell::new(false)),
            language_change_source: Rc::new(Cell::new(None)),
        });

        if let Some(handle) = text_editor_model_handle {
            this.handle_existing_model(&handle);
        }

        this
    }

    /// O `DisposableStore` de `this._register(...)`, descartado com o modelo.
    pub fn store(&self) -> &Rc<DisposableStore> {
        &self.store
    }

    /// `modelService`.
    pub fn model_service(&self) -> &Rc<ModelService> {
        &self.model_service
    }

    /// `languageService`.
    pub fn language_service(&self) -> &Rc<dyn ILanguageService> {
        &self.language_service
    }

    /// `onWillDispose` do `EditorModel`.
    pub fn on_will_dispose(&self) -> Event<()> {
        use crate::common::editor::editor_model::IEditorModel;
        self.editor_model.on_will_dispose()
    }

    /// `isDisposed()` do `EditorModel`.
    pub fn is_disposed(&self) -> bool {
        use crate::common::editor::editor_model::IEditorModel;
        self.editor_model.is_disposed()
    }

    fn handle_existing_model(&self, text_editor_model_handle: &URI) {
        // We need the resource to point to an existing model
        let Some(model) = self.model_service.get_model(text_editor_model_handle) else {
            je_base::errors::throw(je_base::errors::Error::new(format!(
                "Document with resource {} does not exist",
                text_editor_model_handle.to_string(true).to_string_lossy()
            )));
        };

        *self.text_editor_model_handle.borrow_mut() = Some(text_editor_model_handle.clone());

        // Make sure we clean up when this model gets disposed
        self.register_model_dispose_listener(&model);
    }

    fn register_model_dispose_listener(&self, model: &TextModel) {
        let handle = self.text_editor_model_handle.clone();
        let owner = self.owner.clone();
        self.model_dispose_listener.set(Some(model.on_will_dispose().listen(move |_| {
            *handle.borrow_mut() = None; // make sure we do not dispose code editor model again
            if let Some(owner) = owner.upgrade() {
                owner.dispose();
            }
        })));
    }

    /// `textEditorModel` (`ITextModel | null`).
    pub fn text_editor_model(&self) -> Option<Rc<TextModel>> {
        let handle = self.text_editor_model_handle.borrow().clone();
        handle.and_then(|handle| self.model_service.get_model(&handle))
    }

    /// `textEditorModelHandle`.
    pub fn text_editor_model_handle(&self) -> Option<URI> {
        self.text_editor_model_handle.borrow().clone()
    }

    pub fn is_readonly(&self) -> Readonly {
        Readonly::Bool(true)
    }

    /// `languageChangeSource`.
    pub fn language_change_source(&self) -> Option<LanguageChangeSource> {
        self.language_change_source.get()
    }

    /// `hasLanguageSetExplicitly`: `typeof _languageChangeSource === 'string'`.
    pub fn has_language_set_explicitly(&self) -> bool {
        self.language_change_source.get().is_some()
    }

    /// `setLanguageId(languageId, source?)`.
    pub fn set_language_id(&self, language_id: &str, source: Option<&str>) {
        // Remember that an explicit language was set
        self.language_change_source.set(Some(LanguageChangeSource::User));

        self.set_language_id_internal(language_id, source);
    }

    fn set_language_id_internal(&self, language_id: &str, source: Option<&str>) {
        let Some(model) = self.text_editor_model() else {
            return;
        };

        if language_id.is_empty() || language_id == model.get_language_id() {
            return;
        }

        self.block_language_change_listener.set(true);
        let language = self.language_service.create_by_id(Some(language_id));
        model.set_language(&language.language_id(), source);
        self.block_language_change_listener.set(false);
    }

    /// `installModelListeners(model)`.
    pub fn install_model_listeners(&self, model: &TextModel) {
        // Setup listener for lower level language changes
        let slot: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));
        let (block, source, listener_slot) = (self.block_language_change_listener.clone(), self.language_change_source.clone(), slot.clone());
        let disposable = model.on_did_change_language().listen(move |e| {
            if e.source == LANGUAGE_DETECTION_LANGUAGE_EVENT_SOURCE || block.get() {
                return;
            }

            source.set(Some(LanguageChangeSource::Api));
            let listener = listener_slot.borrow_mut().take();
            if let Some(listener) = listener {
                listener.dispose();
            }
        });
        *slot.borrow_mut() = Some(disposable.clone());
        self.store.add(disposable);
    }

    /// `getLanguageId()`.
    pub fn get_language_id(&self) -> Option<String> {
        self.text_editor_model().map(|model| model.get_language_id())
    }

    /// `autoDetectLanguage()`.
    pub fn auto_detect_language(&self) -> Promise<()> {
        let this = self.this.upgrade().expect("o modelo está vivo enquanto é usado");
        self.auto_detect_language_throttler.trigger(move |_| this.do_auto_detect_language(), None)
    }

    fn do_auto_detect_language(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let handle = this.text_editor_model_handle();
            if this.has_language_set_explicitly() // skip detection when the user has made an explicit choice on the language
                || handle.is_none() // require a URI to run the detection for
                || !this.language_detection_service.is_enabled_for_language(&this.get_language_id().unwrap_or_else(|| PLAINTEXT_LANGUAGE_ID.to_string()))
            // require a valid language that is enlisted for detection
            {
                return Ok(());
            }

            let lang = this.language_detection_service.detect_language(&handle.expect("o recurso foi conferido")).await?;
            let prev_lang = this.get_language_id();
            if let Some(lang) = lang {
                if Some(&lang) != prev_lang.as_ref() && !this.is_disposed() {
                    this.set_language_id_internal(&lang, Some(LANGUAGE_DETECTION_LANGUAGE_EVENT_SOURCE));
                    let language_name = this.language_service.get_language_name(&lang);
                    this.accessibility_service.alert(&localize(
                        "languageAutoDetected",
                        "Language {0} was automatically detected and set as the language mode.",
                        &[language_name.as_deref().unwrap_or(&lang)],
                    ));
                }
            }
            Ok(())
        })
    }

    /// `createTextEditorModel(value, resource, preferredLanguageId?)`: cria o modelo de texto com o valor, a linguagem
    /// preferida (pode ser separada por vírgulas) e o recurso.
    pub fn create_text_editor_model(&self, value: &TextBufferFactoryRef, first_line_text: &str, resource: Option<&URI>, preferred_language_id: Option<&str>) -> Rc<TextModel> {
        let language_id = self.get_or_create_language(resource, preferred_language_id, Some(first_line_text));

        self.do_create_text_editor_model(value, &language_id, resource)
    }

    fn do_create_text_editor_model(&self, value: &TextBufferFactoryRef, language_id: &str, resource: Option<&URI>) -> Rc<TextModel> {
        let existing = resource.and_then(|resource| self.model_service.get_model(resource));
        let model = match existing {
            None => {
                let language = self.language_service.create_by_id(Some(language_id));
                let factory = value.take().expect("a fábrica de texto ainda não foi consumida");
                let model = self.model_service.create_model(je_editor::text_model::TextSource::Factory(factory), Some(language.as_ref()), resource, false);
                self.created_editor_model.set(true);

                // Make sure we clean up when this model gets disposed
                self.register_model_dispose_listener(&model);
                model
            }
            Some(model) => {
                self.update_text_editor_model(Some(value), Some(language_id), None);
                model
            }
        };

        *self.text_editor_model_handle.borrow_mut() = Some(model.uri());

        model
    }

    /// `getFirstLineText(value)` para um `ITextBufferFactory`.
    pub fn get_first_line_text(value: &je_editor::piece_tree_text_buffer_builder::PieceTreeTextBufferFactory) -> String {
        value.get_first_line_text(FIRST_LINE_DETECTION_LENGTH_LIMIT).to_string_lossy()
    }

    /// `getFirstLineText(value)` para um `ITextModel`.
    pub fn get_first_line_text_of_model(value: &TextModel) -> String {
        let line: JsString = value.get_line_content(1);
        line.slice(0, line.len().min(FIRST_LINE_DETECTION_LENGTH_LIMIT)).to_string_lossy()
    }

    /// `getOrCreateLanguage(resource, languageService, preferredLanguage, firstLineText?)`: o identificador da linguagem
    /// (o `ILanguageSelection.languageId`).
    pub fn get_or_create_language(&self, resource: Option<&URI>, preferred_language: Option<&str>, first_line_text: Option<&str>) -> String {
        // lookup language via resource path if the provided language is unspecific
        match preferred_language {
            None | Some("") | Some(PLAINTEXT_LANGUAGE_ID) => {
                let first_line = first_line_text.map(JsString::from);
                self.language_service.create_by_filepath_or_first_line(resource, first_line.as_ref()).language_id()
            }
            // otherwise take the preferred language for granted
            Some(preferred_language) => self.language_service.create_by_id(Some(preferred_language)).language_id(),
        }
    }

    /// `updateTextEditorModel(newValue?, preferredLanguageId?, reason?)`.
    pub fn update_text_editor_model(&self, new_value: Option<&TextBufferFactoryRef>, preferred_language_id: Option<&str>, reason: Option<TextModelEditSource>) {
        let Some(model) = self.text_editor_model() else {
            return;
        };

        // contents
        if let Some(new_value) = new_value {
            if let Some(factory) = new_value.take() {
                self.model_service.update_model(&model, je_editor::text_model::TextSource::Factory(factory), reason);
            }
        }

        // language (only if specific and changed)
        if let Some(preferred_language_id) = preferred_language_id {
            if !preferred_language_id.is_empty() && preferred_language_id != PLAINTEXT_LANGUAGE_ID && model.get_language_id() != preferred_language_id {
                let language = self.language_service.create_by_id(Some(preferred_language_id));
                model.set_language(&language.language_id(), None);
            }
        }
    }

    /// `createSnapshot()` (`ITextSnapshot | null`, com o BOM preservado).
    pub fn create_snapshot(&self) -> Option<Box<dyn TextSnapshot>> {
        self.text_editor_model().map(|model| model.create_snapshot(true /* preserve BOM */))
    }

    /// `isResolved()`: `!!textEditorModelHandle`.
    pub fn is_resolved(&self) -> bool {
        self.text_editor_model_handle.borrow().is_some()
    }

    /// `dispose()`.
    pub fn dispose(&self) {
        self.model_dispose_listener.dispose(); // dispose this first because it will trigger another dispose() otherwise

        let handle = self.text_editor_model_handle.borrow_mut().take();
        if let Some(handle) = handle {
            if self.created_editor_model.get() {
                self.model_service.destroy_model(&handle);
            }
        }

        self.created_editor_model.set(false);

        self.editor_model.dispose();
        self.store.dispose();
    }
}
