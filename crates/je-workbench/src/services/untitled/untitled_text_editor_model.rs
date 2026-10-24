//! Porte de `src/vs/workbench/services/untitled/common/untitledTextEditorModel.ts`: o `UntitledTextEditorModel`, o modelo
//! de editor de texto que ainda não tem arquivo. Guarda o texto num modelo do editor de código, nasce sujo quando tem
//! valor inicial ou caminho associado, tira o nome da primeira linha, resolve de um backup ou do valor inicial e se
//! registra como working copy.
//!
//! Divergências:
//! - **Herança.** `UntitledTextEditorModel extends BaseTextEditorModel` guarda a [`BaseTextEditorModel`] (campo `base`). A
//!   macro [`impl_base_text_editor_model`](crate::impl_base_text_editor_model) não é usada porque a classe sobrescreve
//!   `setLanguageId`, `getLanguageId`, `resolve`, `isResolved` e `isReadonly`: os contratos [`IEditorModel`],
//!   [`ITextEditorModel`] e [`ILanguageSupport`] saem escritos à mão, repassando à base o que a classe não sobrescreve.
//! - **Injeção.** O recurso, `hasAssociatedFilePath`, o valor inicial, a linguagem e o encoding preferidos são os
//!   argumentos estáticos ([`Injectable::Args`]) e os serviços decorados são a tupla `Deps`, na ordem do construtor. O
//!   `IEditorService` entra só para o `activeTextEditorLanguageId`.
//! - **`IUntitledTextEditorModel`.** É o trait [`IUntitledTextEditorModel`] (com `ITextEditorModel`, `ILanguageSupport` e
//!   `IWorkingCopy` de supertraits). `getEncoding` e `setEncoding` (de `IEncodingSupport`) e `getLanguageId` (de
//!   `ITextEditorModel`) entram nele porque os traits do porte não os trazem para este caso. `resolve` e `isResolved` são
//!   os do [`IEditorModel`]; `IResolvedUntitledTextEditorModel` (`textEditorModel` nunca nulo) é o mesmo trait, como o
//!   `IResolvedTextEditorModel` do `resolver_service`.
//! - **Propriedades** viram métodos, e os eventos devolvem o [`Event`]. `typeId` é [`IWorkingCopy::working_copy_type_id`].
//! - **Configuração.** `getValue(resource, 'files.encoding')` e `getValue(resource, 'workbench.editor.untitled.labelFormat')`
//!   leem um [`Dynamic`]; só o texto conta (`typeof configuredEncoding === 'string'`), e o formato de rótulo só vale com
//!   `content` ou `name`.
//! - **Nome da primeira linha.** `trim().replace(/\s+/g, ' ')` e `replace(/‮/g, '')` trabalham sobre as unidades
//!   UTF-16 do texto, com o `\s` do JavaScript ([`is_js_whitespace`]); `substr(0, n)` é o corte das unidades.
//! - **`try/finally`** do `revert` (`ignoreDirtyOnModelContentChange`) não precisa de guarda: um pânico no meio do
//!   `updateTextEditorModel` já derruba o modelo.
//! - **Escutas.** As escutas que o modelo instala nele mesmo seguram o modelo por `Weak` e leem o modelo de texto na hora
//!   do evento, para o ciclo `Rc` não vazar (no JavaScript o coletor de lixo cuida disso).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::buffer::{VSBuffer, VSBufferReadableStream, buffer_to_stream};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::js_string::{is_js_whitespace, trim};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::strings::get_char_containing_offset;
use je_base::uri::URI;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::language_service::ILanguageService;
use je_editor::model::TextSnapshot;
use je_editor::range::Range;
use je_editor::services_model::IModelService;
use je_editor::text_model::{TextModel, create_text_buffer_factory};
use je_editor::text_model_events::ModelContentChangedEvent;
use je_editor::text_resource_configuration_service::{ITextResourceConfigurationChangeEvent, ITextResourceConfigurationService};
use je_editor::word_helper::ensure_valid_word_definition;
use je_instantiation::Injectable;
use je_platform::accessibility::IAccessibilityService;
use je_platform::label::label::ILabelService;

use crate::common::editor::editor_model::IEditorModel;
use crate::common::editor::text_editor_model::BaseTextEditorModel;
use crate::common::editor::{RevertOptions, SaveOptions};
use crate::services::editor::editor_service::IEditorService;
use crate::services::files_configuration::files_configuration_service::Readonly;
use crate::services::language_detection::language_detection_worker_service::ILanguageDetectionService;
use crate::services::textfile::encoding::UTF8;
use crate::services::textfile::text_buffer_factory::create_text_buffer_factory_from_stream;
use crate::services::textfile::textfiles::{
    EncodedContent, ILanguageSupport, ITextFileService, ReadTextFileEncodingOptions, TextFileSaveOptions, TextOrSnapshot, WriteTextFileOptions,
};
use crate::services::textmodel_resolver::resolver_service::ITextEditorModel;
use crate::services::working_copy::working_copy::{
    IWorkingCopy, NO_TYPE_ID, WorkingCopyBackup, WorkingCopyBackupContent, WorkingCopyCapabilities, WorkingCopyIdentifier, WorkingCopySaveEvent,
};
use crate::services::working_copy::working_copy_backup::IWorkingCopyBackupService;
use crate::services::working_copy::working_copy_service::IWorkingCopyService;

/// `IUntitledTextEditorModel`.
pub trait IUntitledTextEditorModel: ITextEditorModel + ILanguageSupport + IWorkingCopy {
    /// Emits an event when the encoding of this untitled model changes.
    fn on_did_change_encoding(&self) -> Event<()>;

    /// Emits an event when the name of this untitled model changes.
    fn on_did_change_name(&self) -> Event<()>;

    /// Emits an event when this untitled model is reverted.
    fn on_did_revert(&self) -> Event<()>;

    /// Whether this untitled text model has an associated file path.
    fn has_associated_file_path(&self) -> bool;

    /// Whether this model has an explicit language or not.
    fn has_language_set_explicitly(&self) -> bool;

    /// `IEncodingSupport.getEncoding`: gets the encoding of the object if known.
    fn get_encoding(&self) -> Option<String>;

    /// Sets the encoding to use for this untitled model.
    fn set_encoding(&self, encoding: &str) -> Promise<()>;

    /// `getLanguageId()` (de `ITextEditorModel`): the language id of the text model if known.
    fn get_language_id(&self) -> Option<String>;
}

/// `workbench.editor.untitled.labelFormat` (`'content' | 'name'`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LabelFormat {
    Content,
    Name,
}

impl LabelFormat {
    /// `configuredLabelFormat === 'content' || configuredLabelFormat === 'name'`.
    fn from_configuration(value: &Dynamic) -> Option<Self> {
        match value {
            Dynamic::String(value) => match value.to_string_lossy().as_str() {
                "content" => Some(Self::Content),
                "name" => Some(Self::Name),
                _ => None,
            },
            _ => None,
        }
    }
}

/// `UntitledTextEditorModel.FIRST_LINE_NAME_MAX_LENGTH`.
const FIRST_LINE_NAME_MAX_LENGTH: usize = 40;

/// `UntitledTextEditorModel.FIRST_LINE_NAME_CANDIDATE_MAX_LENGTH`.
const FIRST_LINE_NAME_CANDIDATE_MAX_LENGTH: usize = FIRST_LINE_NAME_MAX_LENGTH * 10;

/// `UntitledTextEditorModel.ACTIVE_EDITOR_LANGUAGE_ID`: support the special '${activeEditorLanguage}' language by looking
/// up the language id from the editor that is active before the untitled editor opens. This special id is only used for
/// the initial language and can be changed after the fact (either manually or through auto-detection).
const ACTIVE_EDITOR_LANGUAGE_ID: &str = "${activeEditorLanguage}";

/// `‮`: the Right-to-Left Override character (#190133).
const RIGHT_TO_LEFT_OVERRIDE: u16 = 0x202E;

/// `UntitledTextEditorModel`.
pub struct UntitledTextEditorModel {
    this: Weak<UntitledTextEditorModel>,
    base: Rc<BaseTextEditorModel>,

    //#region Events
    on_did_change_content: Rc<Emitter<()>>,
    on_did_change_name: Rc<Emitter<()>>,
    on_did_change_dirty: Rc<Emitter<()>>,
    on_did_change_encoding: Rc<Emitter<()>>,
    on_did_save: Rc<Emitter<WorkingCopySaveEvent>>,
    on_did_revert: Rc<Emitter<()>>,
    //#endregion

    /// `resource`.
    resource: URI,
    /// `hasAssociatedFilePath`.
    has_associated_file_path: bool,
    initial_value: Option<String>,
    preferred_language_id: RefCell<Option<String>>,
    preferred_encoding: RefCell<Option<String>>,

    configured_label_format: Cell<LabelFormat>,
    cached_model_first_line_words: RefCell<Option<String>>,
    configured_encoding: RefCell<Option<String>>,

    dirty: Cell<bool>,
    ignore_dirty_on_model_content_change: Cell<bool>,

    working_copy_backup_service: Rc<dyn IWorkingCopyBackupService>,
    text_resource_configuration_service: Rc<dyn ITextResourceConfigurationService>,
    text_file_service: Rc<dyn ITextFileService>,
    label_service: Rc<dyn ILabelService>,
    editor_service: Rc<dyn IEditorService>,
}

impl Injectable for UntitledTextEditorModel {
    /// `(resource, hasAssociatedFilePath, initialValue, preferredLanguageId, preferredEncoding)`.
    type Args = (URI, bool, Option<String>, Option<String>, Option<String>);
    type Deps = (
        Rc<dyn ILanguageService>,
        Rc<dyn IModelService>,
        Rc<dyn IWorkingCopyBackupService>,
        Rc<dyn ITextResourceConfigurationService>,
        Rc<dyn IWorkingCopyService>,
        Rc<dyn ITextFileService>,
        Rc<dyn ILabelService>,
        Rc<dyn IEditorService>,
        Rc<dyn ILanguageDetectionService>,
        Rc<dyn IAccessibilityService>,
    );

    fn create(
        (resource, has_associated_file_path, initial_value, preferred_language_id, preferred_encoding): Self::Args,
        (
            language_service,
            model_service,
            working_copy_backup_service,
            text_resource_configuration_service,
            working_copy_service,
            text_file_service,
            label_service,
            editor_service,
            language_detection_service,
            accessibility_service,
        ): Self::Deps,
    ) -> Rc<Self> {
        let dirty = has_associated_file_path || initial_value.as_deref().is_some_and(|value| !value.is_empty());

        let this = Rc::new_cyclic(|this: &Weak<UntitledTextEditorModel>| {
            let outer: Weak<dyn IDisposable> = this.clone();
            let base = BaseTextEditorModel::new(outer, model_service, language_service, language_detection_service, accessibility_service, None);

            let on_did_change_content = Emitter::new();
            let on_did_change_name = Emitter::new();
            let on_did_change_dirty = Emitter::new();
            let on_did_change_encoding = Emitter::new();
            let on_did_save = Emitter::new();
            let on_did_revert = Emitter::new();
            let store = base.store();
            store.add(on_did_change_content.clone());
            store.add(on_did_change_name.clone());
            store.add(on_did_change_dirty.clone());
            store.add(on_did_change_encoding.clone());
            store.add(on_did_save.clone());
            store.add(on_did_revert.clone());

            Self {
                this: this.clone(),
                base,
                on_did_change_content,
                on_did_change_name,
                on_did_change_dirty,
                on_did_change_encoding,
                on_did_save,
                on_did_revert,
                resource,
                has_associated_file_path,
                initial_value,
                preferred_language_id: RefCell::new(preferred_language_id.clone()),
                preferred_encoding: RefCell::new(preferred_encoding),
                configured_label_format: Cell::new(LabelFormat::Content),
                cached_model_first_line_words: RefCell::new(None),
                configured_encoding: RefCell::new(None),
                dirty: Cell::new(dirty),
                ignore_dirty_on_model_content_change: Cell::new(false),
                working_copy_backup_service,
                text_resource_configuration_service,
                text_file_service,
                label_service,
                editor_service,
            }
        });

        // Make known to working copy service
        let working_copy: Rc<dyn IWorkingCopy> = this.clone();
        this.base.store().add(working_copy_service.register_working_copy(working_copy));

        // This is typically controlled by the setting `files.defaultLanguage`.
        // If that setting is set, we should not detect the language.
        if let Some(preferred_language_id) = preferred_language_id.filter(|language_id| !language_id.is_empty()) {
            this.set_language_id_override(&preferred_language_id, None);
        }

        // Fetch config
        this.on_configuration_change(None, false);

        this.register_listeners();

        this
    }
}

impl UntitledTextEditorModel {
    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o modelo está vivo enquanto é usado")
    }

    fn register_listeners(&self) {
        // Config Changes
        let this = self.this.clone();
        self.base.store().add(self.text_resource_configuration_service.on_did_change_configuration().listen(move |e| {
            if let Some(this) = this.upgrade() {
                this.on_configuration_change(Some(e.as_ref()), true);
            }
        }));
    }

    fn on_configuration_change(&self, e: Option<&dyn ITextResourceConfigurationChangeEvent>, from_event: bool) {
        // Encoding
        if e.is_none_or(|e| e.affects_configuration(Some(&self.resource), "files.encoding")) {
            let configured_encoding = self.text_resource_configuration_service.get_value(Some(&self.resource), Some("files.encoding"));
            if let Dynamic::String(configured_encoding) = configured_encoding {
                let configured_encoding = configured_encoding.to_string_lossy();
                if self.configured_encoding.borrow().as_deref() != Some(configured_encoding.as_str()) {
                    *self.configured_encoding.borrow_mut() = Some(configured_encoding);

                    let has_preferred_encoding = self.preferred_encoding.borrow().as_deref().is_some_and(|encoding| !encoding.is_empty());
                    if from_event && !has_preferred_encoding {
                        self.on_did_change_encoding.fire(()); // do not fire event if we have a preferred encoding set
                    }
                }
            }
        }

        // Label Format
        if e.is_none_or(|e| e.affects_configuration(Some(&self.resource), "workbench.editor.untitled.labelFormat")) {
            let configured_label_format = self.text_resource_configuration_service.get_value(Some(&self.resource), Some("workbench.editor.untitled.labelFormat"));
            if let Some(configured_label_format) = LabelFormat::from_configuration(&configured_label_format)
                && self.configured_label_format.get() != configured_label_format
            {
                self.configured_label_format.set(configured_label_format);

                if from_event {
                    self.on_did_change_name.fire(());
                }
            }
        }
    }

    //#region Language

    /// `setLanguageId(languageId, source?)` (override).
    fn set_language_id_override(&self, language_id: &str, source: Option<&str>) {
        let actual_language: Option<String> =
            if language_id == ACTIVE_EDITOR_LANGUAGE_ID { self.editor_service.active_text_editor_language_id() } else { Some(language_id.to_string()) };
        *self.preferred_language_id.borrow_mut() = actual_language.clone();

        if let Some(actual_language) = actual_language.filter(|language| !language.is_empty()) {
            self.base.set_language_id(&actual_language, source);
        }
    }

    //#endregion

    //#region Dirty

    fn set_dirty(&self, dirty: bool) {
        if self.dirty.get() == dirty {
            return;
        }

        self.dirty.set(dirty);
        self.on_did_change_dirty.fire(());
    }

    //#endregion

    //#region Resolve

    /// `resolve()` (override).
    fn resolve_impl(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            // Create text editor model if not yet done
            let mut created_untitled_model = false;
            let mut has_backup = false;
            if this.base.text_editor_model().is_none() {
                // Check for backups or use initial value or empty
                let working_copy: Rc<dyn IWorkingCopy> = this.clone();
                let backup = this.working_copy_backup_service.resolve(&WorkingCopyIdentifier::of(working_copy.as_ref())).await?;
                let untitled_contents: VSBufferReadableStream = match backup {
                    Some(backup) => {
                        has_backup = true;
                        backup.value
                    }
                    None => {
                        let stream: VSBufferReadableStream = buffer_to_stream(VSBuffer::from_string(this.initial_value.as_deref().unwrap_or("")));
                        stream
                    }
                };

                // Determine untitled contents based on backup
                // or initial value. We must use text file service
                // to create the text factory to respect encodings
                // accordingly.
                let decoded_stream = this
                    .text_file_service
                    .get_decoded_stream(Some(&this.resource), untitled_contents, Some(ReadTextFileEncodingOptions { encoding: Some(UTF8.to_string()), ..Default::default() }))
                    .await?;
                let untitled_contents_factory = create_text_buffer_factory_from_stream(decoded_stream.as_ref()).await?;

                let preferred_language_id = this.preferred_language_id.borrow().clone();
                let factory = untitled_contents_factory.take().expect("a fábrica de buffer é consumida uma única vez");
                this.base.create_text_editor_model(factory, Some(&this.resource), preferred_language_id.as_deref());
                created_untitled_model = true;
            }
            // Otherwise: the untitled model already exists and we must assume
            // that the value of the model was changed by the user. As such we
            // do not update the contents, only the language if configured.
            else {
                let preferred_language_id = this.preferred_language_id.borrow().clone();
                this.base.update_text_editor_model(None, preferred_language_id.as_deref(), None);
            }

            // Listen to text model events
            let text_editor_model = this.base.text_editor_model().expect("o modelo de texto existe depois de criado ou atualizado");
            this.install_model_listeners(&text_editor_model);

            // Only adjust name and dirty state etc. if we
            // actually created the untitled model
            if created_untitled_model {
                let has_initial_value = this.initial_value.as_deref().is_some_and(|value| !value.is_empty());

                // Name
                if has_backup || has_initial_value {
                    this.update_name_from_first_line(&text_editor_model);
                }

                // Untitled associated to file path are dirty right away as well as untitled with content
                this.set_dirty(this.has_associated_file_path || has_backup || has_initial_value);

                // If we have initial contents, make sure to emit this
                // as the appropiate events to the outside.
                if has_backup || has_initial_value {
                    this.on_did_change_content.fire(());
                }
            }

            this.base.resolve().await
        })
    }

    /// `installModelListeners(model)` (override).
    fn install_model_listeners(&self, model: &TextModel) {
        let store = self.base.store();

        let this = self.this.clone();
        store.add(model.on_did_change_content().listen(move |e| {
            if let Some(this) = this.upgrade()
                && let Some(model) = this.base.text_editor_model()
            {
                this.on_model_content_changed(&model, e);
            }
        }));

        let this = self.this.clone();
        store.add(model.on_did_change_language().listen(move |_| {
            // language change can have impact on config
            if let Some(this) = this.upgrade() {
                this.on_configuration_change(None, true);
            }
        }));

        self.base.install_model_listeners(model);
    }

    fn on_model_content_changed(&self, text_editor_model: &TextModel, e: &ModelContentChangedEvent) {
        if !self.ignore_dirty_on_model_content_change.get() {
            // mark the untitled text editor as non-dirty once its content becomes empty and we do
            // not have an associated path set. we never want dirty indicator in that case.
            if !self.has_associated_file_path && text_editor_model.get_line_count() == 1 && text_editor_model.get_line_length(1) == 0 {
                self.set_dirty(false);
            }
            // turn dirty otherwise
            else {
                self.set_dirty(true);
            }
        }

        // Check for name change if first line changed in the range of 0-FIRST_LINE_NAME_CANDIDATE_MAX_LENGTH columns
        if e.changes
            .iter()
            .any(|change| (change.range.start_line_number == 1 || change.range.end_line_number == 1) && change.range.start_column <= FIRST_LINE_NAME_CANDIDATE_MAX_LENGTH as i32)
        {
            self.update_name_from_first_line(text_editor_model);
        }

        // Emit as general content change event
        self.on_did_change_content.fire(());

        // Detect language from content
        drop(self.base.auto_detect_language());
    }

    fn update_name_from_first_line(&self, text_editor_model: &TextModel) {
        if self.has_associated_file_path {
            return; // not in case of an associated file path
        }

        // Determine the first words of the model following these rules:
        // - cannot be only whitespace (so we trim())
        // - cannot be only non-alphanumeric characters (so we run word definition regex over it)
        // - cannot be longer than FIRST_LINE_MAX_TITLE_LENGTH
        // - normalize multiple whitespaces to a single whitespace

        let mut model_first_words_candidate: Option<String> = None;

        // first cap at FIRST_LINE_NAME_CANDIDATE_MAX_LENGTH
        let first_line_value = text_editor_model.get_value_in_range(&Range::new(1, 1, 1, (FIRST_LINE_NAME_CANDIDATE_MAX_LENGTH + 1) as i32));

        // normalize whitespaces and drop Right-to-Left Override character (#190133)
        let mut first_line_text: Vec<u16> = Vec::new();
        let mut in_whitespace = false;
        for &unit in trim(first_line_value.units()) {
            if is_js_whitespace(unit) {
                if !in_whitespace {
                    first_line_text.push(0x20);
                    in_whitespace = true;
                }
            } else {
                first_line_text.push(unit);
                in_whitespace = false;
            }
        }
        first_line_text.retain(|&unit| unit != RIGHT_TO_LEFT_OVERRIDE);

        // finally cap at FIRST_LINE_NAME_MAX_LENGTH (grapheme aware #111235)
        let cut_at = get_char_containing_offset(&first_line_text, FIRST_LINE_NAME_MAX_LENGTH).0;
        first_line_text.truncate(cut_at);

        if !first_line_text.is_empty() && ensure_valid_word_definition(None).exec(&first_line_text).is_some() {
            model_first_words_candidate = Some(JsString::from_units(first_line_text).to_string_lossy());
        }

        if model_first_words_candidate != *self.cached_model_first_line_words.borrow() {
            *self.cached_model_first_line_words.borrow_mut() = model_first_words_candidate;
            self.on_did_change_name.fire(());
        }
    }

    //#endregion
}

impl IDisposable for UntitledTextEditorModel {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IEditorModel for UntitledTextEditorModel {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn on_will_dispose(&self) -> Event<()> {
        self.base.on_will_dispose()
    }

    fn resolve(&self) -> Promise<()> {
        self.resolve_impl()
    }

    /// `isResolved()` (override): `!!this.textEditorModelHandle`.
    fn is_resolved(&self) -> bool {
        self.base.is_resolved()
    }

    fn is_disposed(&self) -> bool {
        self.base.is_disposed()
    }
}

impl ITextEditorModel for UntitledTextEditorModel {
    fn text_editor_model(&self) -> Option<Rc<TextModel>> {
        self.base.text_editor_model()
    }

    /// `isReadonly()` (override).
    fn is_readonly(&self) -> Readonly {
        Readonly::Bool(false)
    }

    fn create_snapshot(&self) -> Option<Box<dyn TextSnapshot>> {
        self.base.create_snapshot()
    }
}

impl ILanguageSupport for UntitledTextEditorModel {
    fn set_language_id(&self, language_id: &str, source: Option<&str>) {
        self.set_language_id_override(language_id, source);
    }
}

impl IWorkingCopy for UntitledTextEditorModel {
    /// `typeId`: IMPORTANT: never change this to not break existing assumptions (e.g. backups).
    fn working_copy_type_id(&self) -> String {
        NO_TYPE_ID.to_string()
    }

    fn resource(&self) -> URI {
        self.resource.clone()
    }

    fn name(&self) -> String {
        // Take name from first line if present and only if
        // we have no associated file path. In that case we
        // prefer the file name as title.
        if self.configured_label_format.get() == LabelFormat::Content && !self.has_associated_file_path {
            let cached = self.cached_model_first_line_words.borrow().clone();
            if let Some(cached) = cached.filter(|words| !words.is_empty()) {
                return cached;
            }
        }

        // Otherwise fallback to resource
        self.label_service.get_uri_basename_label(&self.resource)
    }

    fn capabilities(&self) -> WorkingCopyCapabilities {
        WorkingCopyCapabilities::UNTITLED
    }

    fn on_did_change_dirty(&self) -> Event<()> {
        self.on_did_change_dirty.event()
    }

    fn on_did_change_content(&self) -> Event<()> {
        self.on_did_change_content.event()
    }

    fn on_did_save(&self) -> Event<WorkingCopySaveEvent> {
        self.on_did_save.event()
    }

    fn is_dirty(&self) -> bool {
        self.dirty.get()
    }

    fn is_modified(&self) -> bool {
        self.is_dirty()
    }

    fn backup(&self, _token: &CancellationToken) -> Promise<WorkingCopyBackup> {
        // Make sure to check whether this model has been resolved
        // or not and fallback to the initial value - if any - to
        // prevent backing up an unresolved model and loosing the
        // initial value.
        let encoded = if self.base.is_resolved() {
            // Fill in content the same way we would do when saving the file
            // via the text file service encoding support (hardcode UTF-8)
            let snapshot = self.base.create_snapshot().map(TextOrSnapshot::Snapshot);
            Some(self.text_file_service.get_encoded_readable(
                Some(&self.resource),
                snapshot,
                Some(WriteTextFileOptions { encoding: Some(UTF8.to_string()), ..Default::default() }),
            ))
        } else {
            None
        };
        let initial_value = self.initial_value.clone();

        async_fn(async move {
            let content: Option<WorkingCopyBackupContent> = match encoded {
                Some(encoded) => encoded.await?.map(WorkingCopyBackupContent::from),
                None => initial_value.map(|initial_value| EncodedContent::Buffer(VSBuffer::from_string(&initial_value)).into()),
            };

            Ok(WorkingCopyBackup { meta: None, content })
        })
    }

    fn save(&self, options: Option<SaveOptions>) -> Promise<bool> {
        let (reason, source) = options.as_ref().map(|options| (options.reason, options.source.clone())).unwrap_or_default();
        let target = self.text_file_service.save(&self.resource, options.map(|base| TextFileSaveOptions { base, ..TextFileSaveOptions::default() }));

        let this = self.rc();
        target.then(move |target| {
            // Emit as event
            if target.is_some() {
                this.on_did_save.fire(WorkingCopySaveEvent { reason, source });
            }

            target.is_some()
        })
    }

    fn revert(&self, _options: Option<RevertOptions>) -> Promise<()> {
        // Reset contents to be empty
        self.ignore_dirty_on_model_content_change.set(true);
        self.base.update_text_editor_model(Some(create_text_buffer_factory(&[])), None, None);
        self.ignore_dirty_on_model_content_change.set(false);

        // No longer dirty
        self.set_dirty(false);

        // Emit as event
        self.on_did_revert.fire(());

        Promise::resolved(())
    }
}

impl IUntitledTextEditorModel for UntitledTextEditorModel {
    fn on_did_change_encoding(&self) -> Event<()> {
        self.on_did_change_encoding.event()
    }

    fn on_did_change_name(&self) -> Event<()> {
        self.on_did_change_name.event()
    }

    fn on_did_revert(&self) -> Event<()> {
        self.on_did_revert.event()
    }

    fn has_associated_file_path(&self) -> bool {
        self.has_associated_file_path
    }

    fn has_language_set_explicitly(&self) -> bool {
        self.base.has_language_set_explicitly()
    }

    fn get_encoding(&self) -> Option<String> {
        let preferred_encoding = self.preferred_encoding.borrow().clone().filter(|encoding| !encoding.is_empty());
        preferred_encoding.or_else(|| self.configured_encoding.borrow().clone())
    }

    fn set_encoding(&self, encoding: &str) -> Promise<()> {
        let old_encoding = self.get_encoding();
        *self.preferred_encoding.borrow_mut() = Some(encoding.to_string());

        // Emit if it changed
        if old_encoding.as_deref() != Some(encoding) {
            self.on_did_change_encoding.fire(());
        }

        Promise::resolved(())
    }

    /// `getLanguageId()` (override).
    fn get_language_id(&self) -> Option<String> {
        if let Some(text_editor_model) = self.base.text_editor_model() {
            return Some(text_editor_model.get_language_id());
        }

        self.preferred_language_id.borrow().clone()
    }
}
