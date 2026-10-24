//! Porte de `src/vs/workbench/common/editor/textEditorModel.ts`: o `BaseTextEditorModel`, o modelo de editor que se apoia
//! no modelo de texto do editor de código. A classe só serve para ser herdada, não para ser instanciada.
//!
//! Divergências:
//! - **Herança.** `BaseTextEditorModel extends EditorModel implements ITextEditorModel, ILanguageSupport` é a struct
//!   [`BaseTextEditorModel`] (que guarda o [`EditorModel`], o estado e o comportamento da classe, como métodos próprios) e
//!   uma subclasse a guarda num campo e implementa os traits [`IEditorModel`], [`ITextEditorModel`] e [`ILanguageSupport`]
//!   com a macro [`impl_base_text_editor_model`](crate::impl_base_text_editor_model), que só repassa à base. O `this` das
//!   chamadas virtuais (o `this.dispose()` do `registerModelDisposeListener`) é o [`Weak`] do objeto de fora (o campo
//!   `outer`), que a subclasse entrega ao criar a base dentro do `Rc::new_cyclic` dela. A base é um `Rc` para as
//!   continuações assíncronas (`doAutoDetectLanguage`) terem acesso a ela.
//! - **Construtor que lança.** `new Error('Document with resource ... does not exist')` do `handleExistingModel` é o
//!   [`throw`] (pânico que `errors::catch` captura), como no `ModelService`.
//! - **`ITextModel`** é o [`TextModel`] concreto do porte, e `ITextBufferFactory` é a [`PieceTreeTextBufferFactory`], que
//!   é consumida por `createModel` e `updateModel` (daí o valor ser passado por posse). O `ITextBufferFactory | ITextModel`
//!   de `getFirstLineText` é o [`FirstLineSource`].
//! - **`ModelConstants.FIRST_LINE_DETECTION_LENGTH_LIMIT`** (`model.ts`, que o porte do editor não tem como tipo) é a
//!   constante [`FIRST_LINE_DETECTION_LENGTH_LIMIT`].
//! - **`setLanguage(languageSelection, source)`** do `ITextModel` recebe o `ILanguageSelection` e o porte do modelo recebe
//!   o identificador (`languageSelection.languageId`), que é o que `createById` resolveu.
//! - **`getOrCreateLanguage`** (que uma subclasse pode sobrescrever) é [`BaseTextEditorModel::get_or_create_language`] e
//!   o `createTextEditorModel` o chama; a subclasse que a sobrescrever (o modelo de texto sem título, ainda não portado)
//!   usa [`BaseTextEditorModel::create_text_editor_model_with`], que recebe a função de resolução da linguagem.
//! - **`languageChangeSource`** (`'user' | 'api' | undefined`) é `Option<LanguageChangeSource>`.
//! - **`try/finally`** do `setLanguageIdInternal` é o guarda [`LanguageChangeBlock`], que libera o bloqueio no `Drop`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::ThrottledDelayer;
use je_base::errors::{Error, throw};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable, MutableDisposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::language_service::{ILanguageSelection, ILanguageService};
use je_editor::model::TextSnapshot;
use je_editor::modes_registry::PLAINTEXT_LANGUAGE_ID;
use je_editor::piece_tree_text_buffer_builder::PieceTreeTextBufferFactory;
use je_editor::services_model::IModelService;
use je_editor::text_model::{TextModel, TextSource};
use je_editor::text_model_edit_source::TextModelEditSource;
use je_platform::accessibility::IAccessibilityService;

use crate::common::editor::editor_model::{EditorModel, IEditorModel as _};
use crate::services::files_configuration::files_configuration_service::Readonly;
use crate::services::language_detection::language_detection_worker_service::{ILanguageDetectionService, LANGUAGE_DETECTION_LANGUAGE_EVENT_SOURCE};

/// `ModelConstants.FIRST_LINE_DETECTION_LENGTH_LIMIT`.
pub const FIRST_LINE_DETECTION_LENGTH_LIMIT: usize = 1000;

/// `BaseTextEditorModel.AUTO_DETECT_LANGUAGE_THROTTLE_DELAY`.
const AUTO_DETECT_LANGUAGE_THROTTLE_DELAY: u64 = 600;

/// `'user' | 'api'`: quem mudou a linguagem do modelo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LanguageChangeSource {
    User,
    Api,
}

/// `ITextBufferFactory | ITextModel`: de onde `getFirstLineText` lê a primeira linha.
pub enum FirstLineSource<'a> {
    /// Text buffer factory.
    Factory(&'a PieceTreeTextBufferFactory),
    /// Text model.
    Model(&'a TextModel),
}

/// `getOrCreateLanguage(resource, languageService, preferredLanguage, firstLineText?)`.
pub type GetOrCreateLanguage<'a> = &'a dyn Fn(Option<&URI>, &dyn ILanguageService, Option<&str>, Option<&JsString>) -> Rc<dyn ILanguageSelection>;

/// O `try/finally` de `_blockLanguageChangeListener`: bloqueia a escuta durante a mudança de linguagem e libera ao sair.
struct LanguageChangeBlock {
    flag: Rc<Cell<bool>>,
}

impl LanguageChangeBlock {
    fn new(flag: &Rc<Cell<bool>>) -> Self {
        flag.set(true);
        Self { flag: flag.clone() }
    }
}

impl Drop for LanguageChangeBlock {
    fn drop(&mut self) {
        self.flag.set(false);
    }
}

/// `BaseTextEditorModel`: the base text editor model leverages the code editor model. This class is only intended to be
/// subclassed and not instantiated.
pub struct BaseTextEditorModel {
    this: Weak<BaseTextEditorModel>,
    outer: Weak<dyn IDisposable>,
    editor_model: EditorModel,

    text_editor_model_handle: RefCell<Option<URI>>,

    created_editor_model: Cell<bool>,

    model_dispose_listener: Rc<MutableDisposable<dyn IDisposable>>,
    auto_detect_language_throttler: Rc<ThrottledDelayer<()>>,

    block_language_change_listener: Rc<Cell<bool>>,
    language_change_source: Rc<Cell<Option<LanguageChangeSource>>>,

    /// `modelService` (protected).
    pub model_service: Rc<dyn IModelService>,
    /// `languageService` (protected).
    pub language_service: Rc<dyn ILanguageService>,
    language_detection_service: Rc<dyn ILanguageDetectionService>,
    accessibility_service: Rc<dyn IAccessibilityService>,
}

impl BaseTextEditorModel {
    /// `constructor(modelService, languageService, languageDetectionService, accessibilityService, textEditorModelHandle?)`.
    /// `outer` é o objeto da subclasse (o `this` dos métodos virtuais).
    pub fn new(
        outer: Weak<dyn IDisposable>,
        model_service: Rc<dyn IModelService>,
        language_service: Rc<dyn ILanguageService>,
        language_detection_service: Rc<dyn ILanguageDetectionService>,
        accessibility_service: Rc<dyn IAccessibilityService>,
        text_editor_model_handle: Option<URI>,
    ) -> Rc<Self> {
        let editor_model = EditorModel::new_inner();

        let model_dispose_listener = MutableDisposable::<dyn IDisposable>::new();
        editor_model.register(model_dispose_listener.clone());
        let auto_detect_language_throttler = Rc::new(ThrottledDelayer::<()>::new(AUTO_DETECT_LANGUAGE_THROTTLE_DELAY));
        editor_model.register(auto_detect_language_throttler.clone());

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            outer,
            editor_model,
            text_editor_model_handle: RefCell::new(None),
            created_editor_model: Cell::new(false),
            model_dispose_listener,
            auto_detect_language_throttler,
            block_language_change_listener: Rc::new(Cell::new(false)),
            language_change_source: Rc::new(Cell::new(None)),
            model_service,
            language_service,
            language_detection_service,
            accessibility_service,
        });

        if let Some(text_editor_model_handle) = text_editor_model_handle {
            this.handle_existing_model(text_editor_model_handle);
        }

        this
    }

    fn handle_existing_model(&self, text_editor_model_handle: URI) {
        // We need the resource to point to an existing model
        let Some(model) = self.model_service.get_model(&text_editor_model_handle) else {
            throw(Error::new(format!("Document with resource {} does not exist", text_editor_model_handle.to_string(true).to_string_lossy())));
        };

        *self.text_editor_model_handle.borrow_mut() = Some(text_editor_model_handle);

        // Make sure we clean up when this model gets disposed
        self.register_model_dispose_listener(&model);
    }

    fn register_model_dispose_listener(&self, model: &TextModel) {
        let this = self.this.clone();
        let outer = self.outer.clone();
        self.model_dispose_listener.set(Some(model.on_will_dispose().listen(move |_| {
            if let Some(this) = this.upgrade() {
                *this.text_editor_model_handle.borrow_mut() = None; // make sure we do not dispose code editor model again
            }
            if let Some(outer) = outer.upgrade() {
                outer.dispose();
            }
        })));
    }

    /// `textEditorModelHandle`: o recurso do modelo de texto que este modelo guarda (protected).
    pub fn text_editor_model_handle(&self) -> Option<URI> {
        self.text_editor_model_handle.borrow().clone()
    }

    /// `textEditorModel`.
    pub fn text_editor_model(&self) -> Option<Rc<TextModel>> {
        let handle = self.text_editor_model_handle();
        handle.and_then(|handle| self.model_service.get_model(&handle))
    }

    /// `isReadonly()`.
    pub fn is_readonly(&self) -> Readonly {
        Readonly::Bool(true)
    }

    /// `languageChangeSource`.
    pub fn language_change_source(&self) -> Option<LanguageChangeSource> {
        self.language_change_source.get()
    }

    /// `hasLanguageSetExplicitly`: this is technically not 100% correct, because 'api' can also be set as source if a
    /// model is resolved as text first and then transitions into the resolved language. But to preserve the current
    /// behaviour, we do not change this property. Rather, `languageChangeSource` can be used to get more fine grained
    /// information.
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
        if !self.is_resolved() {
            return;
        }

        let Some(text_editor_model) = self.text_editor_model() else {
            return;
        };

        if language_id.is_empty() || language_id == text_editor_model.get_language_id() {
            return;
        }

        let _block = LanguageChangeBlock::new(&self.block_language_change_listener);
        text_editor_model.set_language(&self.language_service.create_by_id(Some(language_id)).language_id(), source);
    }

    /// `installModelListeners(model)` (protected).
    pub fn install_model_listeners(&self, model: &TextModel) {
        // Setup listener for lower level language changes
        let slot: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));

        let (block_language_change_listener, language_change_source, own_slot) =
            (self.block_language_change_listener.clone(), self.language_change_source.clone(), slot.clone());
        let disposable = self.editor_model.register(model.on_did_change_language().listen(move |e| {
            if e.source == LANGUAGE_DETECTION_LANGUAGE_EVENT_SOURCE || block_language_change_listener.get() {
                return;
            }

            language_change_source.set(Some(LanguageChangeSource::Api));
            let disposable = own_slot.borrow_mut().take();
            if let Some(disposable) = disposable {
                disposable.dispose();
            }
        }));
        *slot.borrow_mut() = Some(disposable);
    }

    /// `getLanguageId()`.
    pub fn get_language_id(&self) -> Option<String> {
        self.text_editor_model().map(|model| model.get_language_id())
    }

    /// `autoDetectLanguage()` (protected).
    pub fn auto_detect_language(&self) -> Promise<()> {
        let this = self.this.clone();
        self.auto_detect_language_throttler.trigger(
            move |_| match this.upgrade() {
                Some(this) => this.do_auto_detect_language(),
                None => Promise::resolved(()),
            },
            None,
        )
    }

    fn do_auto_detect_language(&self) -> Promise<()> {
        let handle = self.text_editor_model_handle();
        let language_id = self.get_language_id().unwrap_or_else(|| PLAINTEXT_LANGUAGE_ID.to_string());
        let Some(handle) = handle.filter(|_| {
            // skip detection when the user has made an explicit choice on the language
            !self.has_language_set_explicitly()
                // require a valid language that is enlisted for detection
                && self.language_detection_service.is_enabled_for_language(&language_id)
        }) else {
            // require a URI to run the detection for
            return Promise::resolved(());
        };

        let this = self.this.clone();
        self.language_detection_service.detect_language(&handle, None).then(move |lang| {
            let Some(this) = this.upgrade() else {
                return;
            };

            let prev_lang = this.get_language_id();
            if let Some(lang) = lang.filter(|lang| !lang.is_empty() && Some(lang) != prev_lang.as_ref()) {
                if this.is_disposed() {
                    return;
                }

                this.set_language_id_internal(&lang, Some(LANGUAGE_DETECTION_LANGUAGE_EVENT_SOURCE));
                let language_name = this.language_service.get_language_name(&lang);
                this.accessibility_service.alert(&localize(
                    "languageAutoDetected",
                    "Language {0} was automatically detected and set as the language mode.",
                    &[language_name.as_deref().unwrap_or(&lang)],
                ));
            }
        })
    }

    /// `createTextEditorModel(value, resource, preferredLanguageId?)`: creates the text editor model with the provided
    /// value, optional preferred language (can be comma separated for multiple values) and optional resource URL.
    pub fn create_text_editor_model(&self, value: PieceTreeTextBufferFactory, resource: Option<&URI>, preferred_language_id: Option<&str>) -> Rc<TextModel> {
        self.create_text_editor_model_with(value, resource, preferred_language_id, &Self::get_or_create_language)
    }

    /// `createTextEditorModel` de uma subclasse que sobrescreve o `getOrCreateLanguage`.
    pub fn create_text_editor_model_with(
        &self,
        value: PieceTreeTextBufferFactory,
        resource: Option<&URI>,
        preferred_language_id: Option<&str>,
        get_or_create_language: GetOrCreateLanguage<'_>,
    ) -> Rc<TextModel> {
        let first_line_text = Self::get_first_line_text(FirstLineSource::Factory(&value));
        let language_selection = get_or_create_language(resource, self.language_service.as_ref(), preferred_language_id, Some(&first_line_text));

        self.do_create_text_editor_model(value, language_selection.as_ref(), resource)
    }

    fn do_create_text_editor_model(&self, value: PieceTreeTextBufferFactory, language_selection: &dyn ILanguageSelection, resource: Option<&URI>) -> Rc<TextModel> {
        let existing_model = resource.and_then(|resource| self.model_service.get_model(resource));
        let model = match existing_model {
            None => {
                let model = self.model_service.create_model(TextSource::Factory(value), Some(language_selection), resource, false);
                self.created_editor_model.set(true);

                // Make sure we clean up when this model gets disposed
                self.register_model_dispose_listener(&model);
                model
            }
            Some(model) => {
                self.update_text_editor_model(Some(value), Some(&language_selection.language_id()), None);
                model
            }
        };

        *self.text_editor_model_handle.borrow_mut() = Some(model.uri());

        model
    }

    /// `getFirstLineText(value)` (protected).
    pub fn get_first_line_text(value: FirstLineSource<'_>) -> JsString {
        match value {
            // text buffer factory
            FirstLineSource::Factory(text_buffer_factory) => text_buffer_factory.get_first_line_text(FIRST_LINE_DETECTION_LENGTH_LIMIT),

            // text model
            FirstLineSource::Model(text_snapshot) => {
                let first_line = text_snapshot.get_line_content(1);
                let units = first_line.units();
                JsString::from_units(units[..units.len().min(FIRST_LINE_DETECTION_LENGTH_LIMIT)].to_vec())
            }
        }
    }

    /// `getOrCreateLanguage(resource, languageService, preferredLanguage, firstLineText?)`: gets the language for the
    /// given identifier.
    ///
    /// `first_line_text`: optional first line of the text buffer to set the language on. This can be used to guess a
    /// language from content.
    pub fn get_or_create_language(
        resource: Option<&URI>,
        language_service: &dyn ILanguageService,
        preferred_language: Option<&str>,
        first_line_text: Option<&JsString>,
    ) -> Rc<dyn ILanguageSelection> {
        // lookup language via resource path if the provided language is unspecific
        match preferred_language {
            None | Some("") | Some(PLAINTEXT_LANGUAGE_ID) => language_service.create_by_filepath_or_first_line(resource, first_line_text),

            // otherwise take the preferred language for granted
            Some(preferred_language) => language_service.create_by_id(Some(preferred_language)),
        }
    }

    /// `updateTextEditorModel(newValue?, preferredLanguageId?, reason?)`: updates the text editor model with the provided
    /// value. If the value is the same as the model has, this is a no-op.
    pub fn update_text_editor_model(&self, new_value: Option<PieceTreeTextBufferFactory>, preferred_language_id: Option<&str>, reason: Option<TextModelEditSource>) {
        if !self.is_resolved() {
            return;
        }

        let Some(text_editor_model) = self.text_editor_model() else {
            return;
        };

        // contents
        if let Some(new_value) = new_value {
            self.model_service.update_model(&text_editor_model, TextSource::Factory(new_value), reason);
        }

        // language (only if specific and changed)
        if let Some(preferred_language_id) = preferred_language_id
            && !preferred_language_id.is_empty()
            && preferred_language_id != PLAINTEXT_LANGUAGE_ID
            && text_editor_model.get_language_id() != preferred_language_id
        {
            text_editor_model.set_language(&self.language_service.create_by_id(Some(preferred_language_id)).language_id(), None);
        }
    }

    /// `createSnapshot()`.
    pub fn create_snapshot(&self) -> Option<Box<dyn TextSnapshot>> {
        let text_editor_model = self.text_editor_model()?;

        Some(text_editor_model.create_snapshot(true /* preserve BOM */))
    }

    /// `isResolved()`.
    pub fn is_resolved(&self) -> bool {
        self.text_editor_model_handle.borrow().is_some()
    }

    /// `resolve()` de `EditorModel`.
    pub fn resolve(&self) -> Promise<()> {
        self.editor_model.resolve()
    }

    /// `onWillDispose` de `EditorModel`.
    pub fn on_will_dispose(&self) -> Event<()> {
        self.editor_model.on_will_dispose()
    }

    /// `isDisposed()` de `EditorModel`.
    pub fn is_disposed(&self) -> bool {
        self.editor_model.is_disposed()
    }

    /// `dispose()`.
    pub fn dispose(&self) {
        self.model_dispose_listener.dispose(); // dispose this first because it will trigger another dispose() otherwise

        let handle = self.text_editor_model_handle();
        if let Some(handle) = handle
            && self.created_editor_model.get()
        {
            self.model_service.destroy_model(&handle);
        }

        *self.text_editor_model_handle.borrow_mut() = None;
        self.created_editor_model.set(false);

        self.editor_model.dispose();
    }
}

/// Gera, para uma subclasse do `BaseTextEditorModel` que guarda a base no campo dado, os contratos que a classe herda
/// sem sobrescrever: [`IEditorModel`], [`ITextEditorModel`](crate::services::textmodel_resolver::resolver_service::ITextEditorModel)
/// e [`ILanguageSupport`](crate::services::textfile::textfiles::ILanguageSupport). O `dispose` (que as subclasses
/// sobrescrevem) fica com quem usa a macro.
#[macro_export]
macro_rules! impl_base_text_editor_model {
    ($ty:ty, $base:ident) => {
        impl $crate::common::editor::editor_model::IEditorModel for $ty {
            fn as_any(&self) -> &dyn ::std::any::Any {
                self
            }

            fn on_will_dispose(&self) -> ::je_base::event::Event<()> {
                self.$base.on_will_dispose()
            }

            fn resolve(&self) -> ::je_base::promise::Promise<()> {
                self.$base.resolve()
            }

            fn is_resolved(&self) -> bool {
                self.$base.is_resolved()
            }

            fn is_disposed(&self) -> bool {
                self.$base.is_disposed()
            }
        }

        impl $crate::services::textmodel_resolver::resolver_service::ITextEditorModel for $ty {
            fn text_editor_model(&self) -> Option<::std::rc::Rc<::je_editor::text_model::TextModel>> {
                self.$base.text_editor_model()
            }

            fn is_readonly(&self) -> $crate::services::files_configuration::files_configuration_service::Readonly {
                self.$base.is_readonly()
            }

            fn create_snapshot(&self) -> Option<Box<dyn ::je_editor::model::TextSnapshot>> {
                self.$base.create_snapshot()
            }
        }

        impl $crate::services::textfile::textfiles::ILanguageSupport for $ty {
            fn set_language_id(&self, language_id: &str, source: Option<&str>) {
                self.$base.set_language_id(language_id, source);
            }
        }
    };
}
