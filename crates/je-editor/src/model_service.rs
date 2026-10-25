//! Porte de `src/vs/editor/common/services/modelService.ts`: o `ModelService`, que cria, guarda e destrói os modelos
//! de texto, lê as opções de criação da configuração (`editor.*`, `files.eol`), as aplica quando a configuração
//! muda, calcula as edições mínimas de `updateModel` e guarda a pilha de desfazer de um arquivo fechado para o
//! caso de ele ser aberto de novo com o mesmo conteúdo.
//!
//! Diferenças do original, nenhuma de comportamento:
//! - O [`IModelService`] de `services_model.rs` é o recorte do contrato (`getModel`, `getCreationOptions`, `createModel`,
//!   `updateModel` e `destroyModel`, o que os consumidores portados leem); o [`ModelService`] o implementa (o `createModel`
//!   e o `updateModel` do trait recebem o [`TextSource`], e os métodos próprios aceitam qualquer `Into<TextSource>`) e
//!   traz o resto do contrato (`getModels` e os eventos) como métodos próprios.
//! - Não há injeção de dependências: [`ModelService::new`] recebe `IConfigurationService`,
//!   `ITextResourcePropertiesService` e o `UndoRedoService`. O `IInstantiationService` do original só criava o
//!   `TextModel`, que aqui se constrói direto com o `UndoRedoService`.
//! - O modelo recebe o identificador da linguagem (o `TextModel` do porte não guarda o `ILanguageSelection`): um
//!   `ILanguageSelection` entra por `language_id()`, e uma mudança da seleção depois não é seguida.
//! - O `ITextModelSHA1Computer` (o `_getSHA1Computer()` que as subclasses trocam) é o parâmetro
//!   [`ModelService::with_sha1_computer`]; `_schemaShouldMaintainUndoRedoElements` não é substituível.
//! - `ITextBufferFactory` e `string` do `createModel` e do `updateModel` são o [`TextSource`]; o `createTextBuffer`
//!   devolve só o buffer, que leva o `disposable` dentro (`PieceTreeTextBuffer::disposable`).
//! - `Object.keys(this._models)` é a ordem de inserção (`OrderedMap`), como a do JavaScript para chaves de texto.
//! - O `_models[modelId]` repetido (erro de programação) lança `Error` (pânico que `errors::catch` captura).
//! - Valores de `editor.*` da configuração são [`Dynamic`]; `clampedInt` e `boolean` são os de `editor_options`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::errors::{Error, throw};
use je_base::event::{Emitter, Event};
use je_base::hash::StringSha1;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::map::OrderedMap;
use je_base::platform::Platform;
use je_base::uri::URI;
use je_platform::configuration::ConfigurationOverrides;
use je_platform::configuration::models::ConfigurationChangeEvent;
use je_platform::configuration::service::IConfigurationService;

use crate::config::editor_options::{boolean, clamped_int, clamped_int_or};
use crate::cursor_common::CursorSimpleModel;
use crate::edit_operation::{EditOperation, SingleEditOperation};
use crate::edit_stack::as_edit_stack_element;
use crate::language_service::ILanguageSelection;
use crate::model::{
    BracketPairColorizationOptions, DefaultEndOfLine, EndOfLinePreference, EndOfLineSequence, IndentSize, TextModelCreationOptions,
    TextModelUpdateOptions, TextSnapshot, ValidEditOperation,
};
use crate::modes_registry::PLAINTEXT_LANGUAGE_ID;
use crate::piece_tree_text_buffer::PieceTreeTextBuffer;
use crate::range::Range;
use crate::selection::Selection;
use crate::services_model::IModelService;
use crate::text_model::{TextModel, TextSource, create_text_buffer};
use crate::text_model_defaults::EDITOR_MODEL_DEFAULTS;
use crate::text_model_edit_source::{EditSources, TextModelEditSource};
use crate::text_model_events::ModelLanguageChangedEvent;
use crate::text_resource_configuration_service::ITextResourcePropertiesService;
use crate::undo_redo::{IUndoRedoService, ResourceEditStackSnapshot, UndoRedoElementRef};
use crate::undo_redo_service::UndoRedoService;

/// `MODEL_ID(resource)`.
fn model_id(resource: &URI) -> String {
    resource.to_string(false).to_string_lossy()
}

/// `language + resource`: a concatenação de texto do original, com `undefined` quando não há recurso.
fn language_and_resource_key(language: &str, resource: Option<&URI>) -> String {
    format!("{language}{}", resource.map_or_else(|| "undefined".to_string(), model_id))
}

/// `isEditStackElement(element) && element.matchesResource(resource)`.
fn is_edit_stack_element_of(element: &UndoRedoElementRef, resource: &URI) -> bool {
    as_edit_stack_element(element).is_some_and(|element| element.matches_resource(resource))
}

/// `{ model, oldLanguageId }` de `onModelLanguageChanged`.
#[derive(Clone)]
pub struct ModelLanguageChanged {
    pub model: Rc<TextModel>,
    pub old_language_id: String,
}

/// `ModelData`: o modelo e os ouvintes que o serviço mantém nos eventos dele.
struct ModelData {
    model: Rc<TextModel>,
    model_event_listeners: Rc<DisposableStore>,
}

impl ModelData {
    fn new(model: Rc<TextModel>, service: Weak<ModelService>) -> Rc<Self> {
        let model_event_listeners = DisposableStore::new();
        let will_dispose_model = model.downgrade();
        let will_dispose_service = service.clone();
        model_event_listeners.add(model.on_will_dispose().listen(move |_| {
            if let (Some(service), Some(model)) = (will_dispose_service.upgrade(), will_dispose_model.upgrade()) {
                service.on_will_dispose(&model);
            }
        }));
        let language_model = model.downgrade();
        model_event_listeners.add(model.on_did_change_language().listen(move |event| {
            if let (Some(service), Some(model)) = (service.upgrade(), language_model.upgrade()) {
                service.on_did_change_language(&model, event);
            }
        }));
        Rc::new(Self { model, model_event_listeners })
    }
}

impl IDisposable for ModelData {
    fn dispose(&self) {
        self.model_event_listeners.dispose();
    }
}

/// `DisposedModelInfo`.
#[derive(Clone)]
struct DisposedModelInfo {
    uri: URI,
    initial_undo_redo_snapshot: Option<ResourceEditStackSnapshot>,
    time: f64,
    shares_undo_redo_stack: bool,
    heap_size: i64,
    sha1: String,
    version_id: i64,
    alternative_version_id: i64,
}

/// `ITextModelSHA1Computer`.
pub trait ITextModelSha1Computer {
    fn can_compute_sha1(&self, model: &TextModel) -> bool;
    fn compute_sha1(&self, model: &TextModel) -> String;
}

/// `DefaultModelSHA1Computer`.
pub struct DefaultModelSha1Computer;

impl DefaultModelSha1Computer {
    /// `MAX_MODEL_SIZE`: calcular o SHA-1 de um modelo de 10 MB leva 200 ms numa máquina nova.
    pub const MAX_MODEL_SIZE: usize = 10 * 1024 * 1024;
}

impl ITextModelSha1Computer for DefaultModelSha1Computer {
    fn can_compute_sha1(&self, model: &TextModel) -> bool {
        model.get_value_length() <= Self::MAX_MODEL_SIZE
    }

    fn compute_sha1(&self, model: &TextModel) -> String {
        // compute the sha1
        let mut sha_computer = StringSha1::new();
        let mut snapshot = model.create_snapshot(false);
        while let Some(text) = snapshot.read() {
            if text.is_empty() {
                break;
            }
            sha_computer.update(text.units());
        }
        sha_computer.digest()
    }
}

/// `{ resource, overrideIdentifier }`.
fn overrides_of(resource: Option<&URI>, override_identifier: &str) -> ConfigurationOverrides {
    ConfigurationOverrides { resource: resource.cloned(), override_identifier: Some(JsString::from(override_identifier)) }
}

/// `DEFAULT_EOL`.
fn default_eol() -> DefaultEndOfLine {
    let platform = Platform::detect();
    if platform.is_linux || platform.is_macintosh { DefaultEndOfLine::Lf } else { DefaultEndOfLine::CrLf }
}

/// `ModelService`.
pub struct ModelService {
    me: Weak<ModelService>,
    base: Rc<Disposable>,
    configuration_service: Rc<dyn IConfigurationService>,
    resource_properties_service: Rc<dyn ITextResourcePropertiesService>,
    undo_redo_service: Rc<UndoRedoService>,
    sha1_computer: Rc<dyn ITextModelSha1Computer>,

    on_model_added: Rc<Emitter<Rc<TextModel>>>,
    on_model_removed: Rc<Emitter<Rc<TextModel>>>,
    on_model_mode_changed: Rc<Emitter<ModelLanguageChanged>>,

    model_creation_options_by_language_and_resource: RefCell<HashMap<String, TextModelCreationOptions>>,

    /// All the models known in the system.
    models: RefCell<OrderedMap<String, Rc<ModelData>>>,
    disposed_models: RefCell<OrderedMap<String, DisposedModelInfo>>,
    disposed_models_heap_size: Cell<i64>,
}

impl ModelService {
    pub const MAX_MEMORY_FOR_CLOSED_FILES_UNDO_STACK: i64 = 20 * 1024 * 1024;

    pub fn new(
        configuration_service: Rc<dyn IConfigurationService>,
        resource_properties_service: Rc<dyn ITextResourcePropertiesService>,
        undo_redo_service: Rc<UndoRedoService>,
    ) -> Rc<Self> {
        Self::with_sha1_computer(configuration_service, resource_properties_service, undo_redo_service, Rc::new(DefaultModelSha1Computer))
    }

    /// O construtor com o `ITextModelSHA1Computer` trocado (o `_getSHA1Computer()` das subclasses).
    pub fn with_sha1_computer(
        configuration_service: Rc<dyn IConfigurationService>,
        resource_properties_service: Rc<dyn ITextResourcePropertiesService>,
        undo_redo_service: Rc<UndoRedoService>,
        sha1_computer: Rc<dyn ITextModelSha1Computer>,
    ) -> Rc<Self> {
        let service = Rc::new_cyclic(|me: &Weak<ModelService>| {
            let base = Disposable::new();
            let on_model_added: Rc<Emitter<Rc<TextModel>>> = Emitter::new();
            base.register(on_model_added.clone());
            let on_model_removed: Rc<Emitter<Rc<TextModel>>> = Emitter::new();
            base.register(on_model_removed.clone());
            let on_model_mode_changed: Rc<Emitter<ModelLanguageChanged>> = Emitter::new();
            base.register(on_model_mode_changed.clone());

            let configuration_listener = me.clone();
            base.register(configuration_service.on_did_change_configuration().listen(move |event| {
                if let Some(service) = configuration_listener.upgrade() {
                    service.update_model_options(Some(event));
                }
            }));

            Self {
                me: me.clone(),
                base,
                configuration_service,
                resource_properties_service,
                undo_redo_service,
                sha1_computer,
                on_model_added,
                on_model_removed,
                on_model_mode_changed,
                model_creation_options_by_language_and_resource: RefCell::new(HashMap::new()),
                models: RefCell::new(OrderedMap::new()),
                disposed_models: RefCell::new(OrderedMap::new()),
                disposed_models_heap_size: Cell::new(0),
            }
        });
        service.update_model_options(None);
        service
    }

    /// `onModelAdded`.
    pub fn on_model_added(&self) -> Event<Rc<TextModel>> {
        self.on_model_added.event()
    }

    /// `onModelRemoved`.
    pub fn on_model_removed(&self) -> Event<Rc<TextModel>> {
        self.on_model_removed.event()
    }

    /// `onModelLanguageChanged`.
    pub fn on_model_language_changed(&self) -> Event<ModelLanguageChanged> {
        self.on_model_mode_changed.event()
    }

    /// `_readModelOptions(config, isForSimpleWidget)`: `editor` é o valor de `editor` da configuração (um objeto,
    /// ou `undefined`) e `eol` o de `config.eol`.
    fn read_model_options(editor: &Dynamic, eol: &str, is_for_simple_widget: bool) -> TextModelCreationOptions {
        let editor_config = matches!(editor, Dynamic::Object(_)).then_some(editor);
        let option = |name: &str| editor_config.map(|editor| editor.get_str(name)).filter(|value| !value.is_undefined());

        let mut tab_size = EDITOR_MODEL_DEFAULTS.tab_size;
        if let Some(value) = option("tabSize") {
            tab_size = clamped_int(&value, f64::from(EDITOR_MODEL_DEFAULTS.tab_size), 1.0, 100.0) as i32;
        }

        let mut indent_size = IndentSize::TabSize;
        if let Some(value) = option("indentSize").filter(|value| !matches!(value, Dynamic::String(text) if *text == "tabSize")) {
            indent_size = match clamped_int_or(&value, IndentSize::TabSize, 1.0, 100.0) {
                Ok(size) => IndentSize::Size(size as i32),
                Err(default) => default,
            };
        }

        let mut insert_spaces = EDITOR_MODEL_DEFAULTS.insert_spaces;
        if let Some(value) = option("insertSpaces") {
            insert_spaces = boolean(&value, EDITOR_MODEL_DEFAULTS.insert_spaces);
        }

        let mut new_default_eol = default_eol();
        if eol == "\r\n" {
            new_default_eol = DefaultEndOfLine::CrLf;
        } else if eol == "\n" {
            new_default_eol = DefaultEndOfLine::Lf;
        }

        let mut trim_auto_whitespace = EDITOR_MODEL_DEFAULTS.trim_auto_whitespace;
        if let Some(value) = option("trimAutoWhitespace") {
            trim_auto_whitespace = boolean(&value, EDITOR_MODEL_DEFAULTS.trim_auto_whitespace);
        }

        let mut detect_indentation = EDITOR_MODEL_DEFAULTS.detect_indentation;
        if let Some(value) = option("detectIndentation") {
            detect_indentation = boolean(&value, EDITOR_MODEL_DEFAULTS.detect_indentation);
        }

        let mut large_file_optimizations = EDITOR_MODEL_DEFAULTS.large_file_optimizations;
        if let Some(value) = option("largeFileOptimizations") {
            large_file_optimizations = boolean(&value, EDITOR_MODEL_DEFAULTS.large_file_optimizations);
        }

        let mut bracket_pair_colorization_options = EDITOR_MODEL_DEFAULTS.bracket_pair_colorization_options;
        if let Some(bracket_pair_colorization @ Dynamic::Object(_)) = option("bracketPairColorization") {
            use je_base::dynamic::Truthy;
            bracket_pair_colorization_options = BracketPairColorizationOptions {
                enabled: bracket_pair_colorization.get_str("enabled").is_truthy(),
                independent_color_pool_per_bracket_type: bracket_pair_colorization.get_str("independentColorPoolPerBracketType").is_truthy(),
            };
        }

        TextModelCreationOptions {
            is_for_simple_widget,
            tab_size,
            indent_size,
            insert_spaces,
            detect_indentation,
            default_eol: new_default_eol,
            trim_auto_whitespace,
            large_file_optimizations,
            bracket_pair_colorization_options,
        }
    }

    /// `_getEOL(resource, language)`.
    fn get_eol(&self, resource: Option<&URI>, language: &str) -> String {
        if let Some(resource) = resource {
            return self.resource_properties_service.get_eol(resource, Some(language));
        }
        let overrides = ConfigurationOverrides { resource: None, override_identifier: Some(JsString::from(language)) };
        if let Dynamic::String(eol) = self.configuration_service.get_value(Some("files.eol"), Some(&overrides)) {
            let eol = eol.to_string_lossy();
            if !eol.is_empty() && eol != "auto" {
                return eol;
            }
        }
        let platform = Platform::detect();
        (if platform.is_linux || platform.is_macintosh { "\n" } else { "\r\n" }).to_string()
    }

    /// `_shouldRestoreUndoStack()`.
    fn should_restore_undo_stack(&self) -> bool {
        match self.configuration_service.get_value(Some("files.restoreUndoStack"), None) {
            Dynamic::Bool(result) => result,
            _ => true,
        }
    }


    /// `_updateModelOptions(e)`.
    fn update_model_options(&self, e: Option<&Rc<ConfigurationChangeEvent>>) {
        let old_options_by_language_and_resource = self.model_creation_options_by_language_and_resource.take();

        // Update options on all models
        let model_datas: Vec<Rc<ModelData>> = self.models.borrow().values().cloned().collect();
        for model_data in model_datas {
            let language = model_data.model.get_language_id();
            let uri = model_data.model.uri();

            if let Some(e) = e {
                let overrides = overrides_of(Some(&uri), &language);
                if !e.affects_configuration("editor", Some(&overrides)) && !e.affects_configuration("files.eol", Some(&overrides)) {
                    continue; // perf: skip if this model is not affected by configuration change
                }
            }

            let old_options = old_options_by_language_and_resource.get(&language_and_resource_key(&language, Some(&uri))).copied();
            let new_options = self.get_creation_options(&language, Some(&uri), model_data.model.is_for_simple_widget());
            Self::set_model_options_for_model(&model_data.model, &new_options, old_options.as_ref());
        }
    }

    /// `_setModelOptionsForModel(model, newOptions, currentOptions)`.
    fn set_model_options_for_model(model: &TextModel, new_options: &TextModelCreationOptions, current_options: Option<&TextModelCreationOptions>) {
        if let Some(current_options) = current_options
            && current_options.default_eol != new_options.default_eol
            && CursorSimpleModel::get_line_count(model) == 1
        {
            model.set_eol(if new_options.default_eol == DefaultEndOfLine::Lf { EndOfLineSequence::Lf } else { EndOfLineSequence::CrLf });
        }

        if let Some(current_options) = current_options
            && current_options.detect_indentation == new_options.detect_indentation
            && current_options.insert_spaces == new_options.insert_spaces
            && current_options.tab_size == new_options.tab_size
            && current_options.indent_size == new_options.indent_size
            && current_options.trim_auto_whitespace == new_options.trim_auto_whitespace
            && current_options.bracket_pair_colorization_options == new_options.bracket_pair_colorization_options
        {
            // Same indent opts, no need to touch the model
            return;
        }

        if new_options.detect_indentation {
            model.detect_indentation(new_options.insert_spaces, new_options.tab_size);
            model.update_options(TextModelUpdateOptions {
                trim_auto_whitespace: Some(new_options.trim_auto_whitespace),
                bracket_colorization_options: Some(new_options.bracket_pair_colorization_options),
                ..TextModelUpdateOptions::default()
            });
        } else {
            model.update_options(TextModelUpdateOptions {
                insert_spaces: Some(new_options.insert_spaces),
                tab_size: Some(new_options.tab_size),
                indent_size: Some(new_options.indent_size),
                trim_auto_whitespace: Some(new_options.trim_auto_whitespace),
                bracket_colorization_options: Some(new_options.bracket_pair_colorization_options),
            });
        }
    }

    // --- begin IModelService

    /// `_insertDisposedModel(disposedModelData)`.
    fn insert_disposed_model(&self, disposed_model_data: DisposedModelInfo) {
        self.disposed_models_heap_size.set(self.disposed_models_heap_size.get() + disposed_model_data.heap_size);
        self.disposed_models.borrow_mut().insert(model_id(&disposed_model_data.uri), disposed_model_data);
    }

    /// `_removeDisposedModel(resource)`.
    fn remove_disposed_model(&self, resource: &URI) -> Option<DisposedModelInfo> {
        let disposed_model_data = self.disposed_models.borrow_mut().remove(&model_id(resource));
        if let Some(disposed_model_data) = &disposed_model_data {
            self.disposed_models_heap_size.set(self.disposed_models_heap_size.get() - disposed_model_data.heap_size);
        }
        disposed_model_data
    }

    /// `_ensureDisposedModelsHeapSize(maxModelsHeapSize)`.
    fn ensure_disposed_models_heap_size(&self, max_models_heap_size: i64) {
        if self.disposed_models_heap_size.get() > max_models_heap_size {
            // we must remove some old undo stack elements to free up some memory
            let mut disposed_models: Vec<DisposedModelInfo> =
                self.disposed_models.borrow().values().filter(|entry| !entry.shares_undo_redo_stack).cloned().collect();
            disposed_models.sort_by(|a, b| a.time.total_cmp(&b.time));
            let mut disposed_models = disposed_models.into_iter();
            while self.disposed_models_heap_size.get() > max_models_heap_size {
                let Some(disposed_model) = disposed_models.next() else { break };
                self.remove_disposed_model(&disposed_model.uri);
                if let Some(snapshot) = &disposed_model.initial_undo_redo_snapshot {
                    self.undo_redo_service.restore_snapshot(snapshot);
                }
            }
        }
    }

    /// `_createModelData(value, languageIdOrSelection, resource, isForSimpleWidget)`.
    fn create_model_data(&self, value: TextSource, language: &str, resource: Option<&URI>, is_for_simple_widget: bool) -> Rc<ModelData> {
        // create & save the model
        let options = self.get_creation_options(language, resource, is_for_simple_widget);
        let model = TextModel::new(value, language, options, resource.cloned(), self.undo_redo_service.clone());
        if let Some(resource) = resource
            && self.disposed_models.borrow().contains_key(&model_id(resource))
        {
            let disposed_model_data = self.remove_disposed_model(resource).expect("modelo descartado conhecido");
            let elements = self.undo_redo_service.get_elements(resource);
            let sha1_is_equal = if self.sha1_computer.can_compute_sha1(&model) {
                self.sha1_computer.compute_sha1(&model) == disposed_model_data.sha1
            } else {
                false
            };
            if sha1_is_equal || disposed_model_data.shares_undo_redo_stack {
                for element in elements.past.iter().chain(elements.future.iter()) {
                    if let Some(element) = as_edit_stack_element(element)
                        && element.matches_resource(resource)
                    {
                        element.set_model(resource, Some(&*model));
                    }
                }
                self.undo_redo_service.set_elements_valid_flag(resource, true, &|element| is_edit_stack_element_of(element, resource));
                if sha1_is_equal {
                    model.overwrite_version_id(disposed_model_data.version_id);
                    model.overwrite_alternative_version_id(disposed_model_data.alternative_version_id);
                    model.overwrite_initial_undo_redo_snapshot(disposed_model_data.initial_undo_redo_snapshot.clone());
                }
            } else if let Some(snapshot) = &disposed_model_data.initial_undo_redo_snapshot {
                self.undo_redo_service.restore_snapshot(snapshot);
            }
        }
        let id = model_id(&model.uri());

        if self.models.borrow().contains_key(&id) {
            // There already exists a model with this id => this is a programmer error
            throw(Error::new("ModelService: Cannot add model because it already exists!"));
        }

        let model_data = ModelData::new(model, self.me.clone());
        self.models.borrow_mut().insert(id, model_data.clone());

        model_data
    }

    /// `updateModel(model, value, reason)`: leva o modelo ao texto dado com as edições mínimas.
    pub fn update_model(&self, model: &TextModel, value: impl Into<TextSource>, reason: Option<TextModelEditSource>) {
        let reason = reason.unwrap_or_else(|| EditSources::unknown(Some("updateModel")));
        let options = self.get_creation_options(&model.get_language_id(), Some(&model.uri()), model.is_for_simple_widget());
        let text_buffer = create_text_buffer(value.into(), options.default_eol);

        // Return early if the text is already set in that form
        if model.equals_text_buffer(&text_buffer) {
            text_buffer.disposable().dispose();
            return;
        }

        // Otherwise find a diff between the values and update model
        model.push_stack_element();
        model.push_eol(EndOfLineSequence::from(text_buffer.get_eol()));
        model.push_edit_operations(
            Some(Vec::new()),
            &Self::compute_edits(model, &text_buffer),
            Some(&mut |_: &[ValidEditOperation]| -> Option<Vec<Selection>> { Some(Vec::new()) }),
            None,
            Some(reason),
        );
        model.push_stack_element();
        text_buffer.disposable().dispose();
    }

    /// `_commonPrefix(a, aLen, aDelta, b, bLen, bDelta)`.
    fn common_prefix(a: &TextModel, a_len: i32, a_delta: i32, b: &PieceTreeTextBuffer, b_len: i32, b_delta: i32) -> i32 {
        let max_result = a_len.min(b_len);

        let mut result = 0;
        let mut i = 0;
        while i < max_result && a.get_line_content(a_delta + i) == b.get_line_content(b_delta + i) {
            result += 1;
            i += 1;
        }
        result
    }

    /// `_commonSuffix(a, aLen, aDelta, b, bLen, bDelta)`.
    fn common_suffix(a: &TextModel, a_len: i32, a_delta: i32, b: &PieceTreeTextBuffer, b_len: i32, b_delta: i32) -> i32 {
        let max_result = a_len.min(b_len);

        let mut result = 0;
        let mut i = 0;
        while i < max_result && a.get_line_content(a_delta + a_len - i) == b.get_line_content(b_delta + b_len - i) {
            result += 1;
            i += 1;
        }
        result
    }

    /// `_computeEdits(model, textBuffer)`: as edições que levam `model` ao estado de `text_buffer`.
    pub fn compute_edits(model: &TextModel, text_buffer: &PieceTreeTextBuffer) -> Vec<SingleEditOperation> {
        let model_line_count = CursorSimpleModel::get_line_count(model);
        let text_buffer_line_count = text_buffer.get_line_count() as i32;
        let common_prefix = Self::common_prefix(model, model_line_count, 1, text_buffer, text_buffer_line_count, 1);

        if model_line_count == text_buffer_line_count && common_prefix == model_line_count {
            // equality case
            return Vec::new();
        }

        let common_suffix = Self::common_suffix(
            model,
            model_line_count - common_prefix,
            common_prefix,
            text_buffer,
            text_buffer_line_count - common_prefix,
            common_prefix,
        );

        let (old_range, new_range) = if common_suffix > 0 {
            (
                Range::new(common_prefix + 1, 1, model_line_count - common_suffix + 1, 1),
                Range::new(common_prefix + 1, 1, text_buffer_line_count - common_suffix + 1, 1),
            )
        } else if common_prefix > 0 {
            (
                Range::new(
                    common_prefix,
                    CursorSimpleModel::get_line_max_column(model, common_prefix),
                    model_line_count,
                    CursorSimpleModel::get_line_max_column(model, model_line_count),
                ),
                Range::new(
                    common_prefix,
                    1 + text_buffer.get_line_length(common_prefix) as i32,
                    text_buffer_line_count,
                    1 + text_buffer.get_line_length(text_buffer_line_count) as i32,
                ),
            )
        } else {
            (
                Range::new(1, 1, model_line_count, CursorSimpleModel::get_line_max_column(model, model_line_count)),
                Range::new(1, 1, text_buffer_line_count, 1 + text_buffer.get_line_length(text_buffer_line_count) as i32),
            )
        };

        vec![EditOperation::replace_move(old_range, Some(text_buffer.get_value_in_range(&new_range, EndOfLinePreference::TextDefined)))]
    }

    /// `createModel(value, languageSelection, resource?, isForSimpleWidget = false)`.
    pub fn create_model(
        &self,
        value: impl Into<TextSource>,
        language_selection: Option<&dyn ILanguageSelection>,
        resource: Option<&URI>,
        is_for_simple_widget: bool,
    ) -> Rc<TextModel> {
        let language = language_selection.map_or_else(|| PLAINTEXT_LANGUAGE_ID.to_string(), |selection| selection.language_id());
        let model_data = self.create_model_data(value.into(), &language, resource, is_for_simple_widget);

        self.on_model_added.fire(model_data.model.clone());

        model_data.model.clone()
    }

    /// `destroyModel(resource)`.
    pub fn destroy_model(&self, resource: &URI) {
        // We need to support that not all models get disposed through this service (i.e. model.dispose() should work!)
        let model_data = self.models.borrow().get(&model_id(resource)).cloned();
        let Some(model_data) = model_data else {
            return;
        };
        model_data.model.dispose();
    }

    /// `getModels()`.
    pub fn get_models(&self) -> Vec<Rc<TextModel>> {
        self.models.borrow().values().map(|model_data| model_data.model.clone()).collect()
    }

    // --- end IModelService

    /// `_schemaShouldMaintainUndoRedoElements(resource)`.
    fn schema_should_maintain_undo_redo_elements(resource: &URI) -> bool {
        let scheme = resource.scheme().to_string_lossy();
        matches!(scheme.as_str(), "file" | "vscode-remote" | "vscode-userdata" | "vscode-notebook-cell")
            || scheme == "fake-fs" // for tests
    }

    /// `_onWillDispose(model)`.
    fn on_will_dispose(&self, model: &Rc<TextModel>) {
        let uri = model.uri();
        let id = model_id(&uri);
        let model_data = self.models.borrow().get(&id).cloned().expect("o modelo a descartar está no serviço");

        let shares_undo_redo_stack = self.undo_redo_service.get_uri_comparison_key(&uri) != uri.to_string(false).to_string_lossy();
        let mut maintain_undo_redo_stack = false;
        let mut heap_size: i64 = 0;
        if shares_undo_redo_stack || (self.should_restore_undo_stack() && Self::schema_should_maintain_undo_redo_elements(&uri)) {
            let elements = self.undo_redo_service.get_elements(&uri);
            if !elements.past.is_empty() || !elements.future.is_empty() {
                for element in elements.past.iter().chain(elements.future.iter()) {
                    if let Some(element) = as_edit_stack_element(element)
                        && element.matches_resource(&uri)
                    {
                        maintain_undo_redo_stack = true;
                        heap_size += element.heap_size(&uri) as i64;
                        element.set_model(&uri, None); // remove reference from text buffer instance
                    }
                }
            }
        }

        let max_memory = Self::MAX_MEMORY_FOR_CLOSED_FILES_UNDO_STACK;
        if !maintain_undo_redo_stack {
            if !shares_undo_redo_stack
                && let Some(initial_undo_redo_snapshot) = model_data.model.get_initial_undo_redo_snapshot()
            {
                self.undo_redo_service.restore_snapshot(&initial_undo_redo_snapshot);
            }
        } else if !shares_undo_redo_stack && (heap_size > max_memory || !self.sha1_computer.can_compute_sha1(model)) {
            // the undo stack for this file would never fit in the configured memory or the file is very large, so don't bother with it.
            if let Some(initial_undo_redo_snapshot) = model_data.model.get_initial_undo_redo_snapshot() {
                self.undo_redo_service.restore_snapshot(&initial_undo_redo_snapshot);
            }
        } else {
            self.ensure_disposed_models_heap_size(max_memory - heap_size);
            // We only invalidate the elements, but they remain in the undo-redo service.
            self.undo_redo_service.set_elements_valid_flag(&uri, false, &|element| is_edit_stack_element_of(element, &uri));
            let time = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |elapsed| elapsed.as_millis() as f64);
            self.insert_disposed_model(DisposedModelInfo {
                uri: uri.clone(),
                initial_undo_redo_snapshot: model_data.model.get_initial_undo_redo_snapshot(),
                time,
                shares_undo_redo_stack,
                heap_size,
                sha1: self.sha1_computer.compute_sha1(model),
                version_id: model.get_version_id(),
                alternative_version_id: model.get_alternative_version_id(),
            });
        }

        self.models.borrow_mut().remove(&id);
        model_data.dispose();

        // clean up cache
        self.model_creation_options_by_language_and_resource.borrow_mut().remove(&language_and_resource_key(&model.get_language_id(), Some(&uri)));

        self.on_model_removed.fire(model.clone());
    }

    /// `_onDidChangeLanguage(model, e)`.
    fn on_did_change_language(&self, model: &Rc<TextModel>, e: &ModelLanguageChangedEvent) {
        let old_language_id = e.old_language.clone();
        let new_language_id = model.get_language_id();
        let uri = model.uri();
        let old_options = self.get_creation_options(&old_language_id, Some(&uri), model.is_for_simple_widget());
        let new_options = self.get_creation_options(&new_language_id, Some(&uri), model.is_for_simple_widget());
        Self::set_model_options_for_model(model, &new_options, Some(&old_options));
        self.on_model_mode_changed.fire(ModelLanguageChanged { model: model.clone(), old_language_id });
    }
}

impl IModelService for ModelService {
    /// `getCreationOptions(languageIdOrSelection, resource, isForSimpleWidget)`.
    fn get_creation_options(&self, language: &str, resource: Option<&URI>, is_for_simple_widget: bool) -> TextModelCreationOptions {
        let key = language_and_resource_key(language, resource);
        let cached = self.model_creation_options_by_language_and_resource.borrow().get(&key).copied();
        if let Some(creation_options) = cached {
            return creation_options;
        }
        let editor = self.configuration_service.get_value(Some("editor"), Some(&overrides_of(resource, language)));
        let eol = self.get_eol(resource, language);
        let creation_options = Self::read_model_options(&editor, &eol, is_for_simple_widget);
        self.model_creation_options_by_language_and_resource.borrow_mut().insert(key, creation_options);
        creation_options
    }

    /// `getModel(resource)`.
    fn get_model(&self, resource: &URI) -> Option<Rc<TextModel>> {
        self.models.borrow().get(&model_id(resource)).map(|model_data| model_data.model.clone())
    }

    /// `getModels()`.
    fn get_models(&self) -> Vec<Rc<TextModel>> {
        ModelService::get_models(self)
    }

    /// `onModelAdded`.
    fn on_model_added(&self) -> Event<Rc<TextModel>> {
        ModelService::on_model_added(self)
    }

    /// `onModelRemoved`.
    fn on_model_removed(&self) -> Event<Rc<TextModel>> {
        ModelService::on_model_removed(self)
    }

    /// `onModelLanguageChanged`.
    fn on_model_language_changed(&self) -> Event<ModelLanguageChanged> {
        ModelService::on_model_language_changed(self)
    }

    /// `createModel(value, languageSelection, resource?, isForSimpleWidget)`: o método genérico do `ModelService`, com o
    /// tipo de conteúdo que o contrato `dyn` aceita.
    fn create_model(&self, value: TextSource, language_selection: Option<&dyn ILanguageSelection>, resource: Option<&URI>, is_for_simple_widget: bool) -> Rc<TextModel> {
        ModelService::create_model(self, value, language_selection, resource, is_for_simple_widget)
    }

    /// `updateModel(model, value, reason)`.
    fn update_model(&self, model: &TextModel, value: TextSource, reason: Option<TextModelEditSource>) {
        ModelService::update_model(self, model, value, reason);
    }

    /// `destroyModel(resource)`.
    fn destroy_model(&self, resource: &URI) {
        ModelService::destroy_model(self, resource);
    }
}

impl IDisposable for ModelService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
