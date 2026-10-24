//! Porte de `src/vs/workbench/contrib/notebook/common/model/notebookCellTextModel.ts`: o modelo de uma célula de
//! notebook (`NotebookCellTextModel`), com o buffer de texto preguiçoso, o modelo de texto do editor ligado a ela, as
//! saídas, os metadados, a detecção automática de linguagem e as comparações usadas pelo diff de notebooks.
//!
//! Divergências:
//! - `Disposable` é o campo `base`; o modelo nasce como `Rc<Self>` (`Rc::new_cyclic`), e os fechos dos eventos guardam
//!   um `Weak` dele.
//! - `ITextModel` é o `Rc<TextModel>` e `ITextBuffer` é o [`PieceTreeTextBuffer`] compartilhado como [`TextBufferRef`].
//!   `_textModel.setLanguage(languageId)`, `_overwriteVersionId` e `_overwriteAlternativeVersionId` são
//!   `set_language`, `overwrite_version_id` e `overwrite_alternative_version_id` do `TextModel`.
//! - As propriedades com `get`/`set` (`language`, `metadata`, `internalMetadata`, `textModel`) são os métodos do trait
//!   [`ICell`] (`language()` e `set_language(...)`); `mime` tem [`NotebookCellTextModel::mime`] e
//!   [`NotebookCellTextModel::set_mime`].
//! - `onDidChangeContent` carrega o enum [`CellContentChange`] (`'content' | 'language' | 'mime' | { type: 'model', event }`).
//! - `autoDetectLanguage` e `_doAutoDetectLanguage` (`async`) devolvem `Promise<()>`; o `ThrottledDelayer<void>` é o
//!   `ThrottledDelayer<()>` do je-base, e o resultado de `trigger` não é observado, como no original.
//! - `_languageDetectionService: ILanguageDetectionService | undefined` é `Option<Rc<dyn ILanguageDetectionService>>`.
//! - `Date.now()` de `computeRunStartTimeAdjustment` é `runtime::date_now()` (relógio virtual dos testes).
//! - `getHashValue` monta o mesmo valor que o original passa a `hash(...)` como um [`Dynamic`], e devolve `i32`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::ThrottledDelayer;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::event::{Emitter, Event};
use je_base::hash::{StringSha1, hash};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, dispose_all};
use je_base::promise::Promise;
use je_base::runtime;
use je_base::strings::split_lines;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_editor::language_service::ILanguageService;
use je_editor::model::{DefaultEndOfLine, EndOfLinePreference, Eol, TextSnapshot};
use je_editor::modes_registry::PLAINTEXT_LANGUAGE_ID;
use je_editor::piece_tree_text_buffer::PieceTreeTextBuffer;
use je_editor::range::Range;
use je_editor::text_model::{TextModel, TextSource, create_text_buffer};
use je_editor::text_model_events::ModelContentChangedEvent;
use je_platform::js::to_json;

use super::notebook_cell_output_text_model::NotebookCellOutputTextModel;
use crate::contrib::notebook::common::notebook_common::{
    CellInternalMetadataChangedEvent, CellKind, ICell, ICellDto2, ICellOutput, IOutputDto, IOutputItemDto, NotebookCellCollapseState,
    NotebookCellDefaultCollapseConfig, NotebookCellInternalMetadata, NotebookCellMetadata, NotebookCellOutputsSplice,
    TextBufferRef, TransientCellMetadata, TransientOptions,
};
use crate::contrib::notebook::common::notebook_logging_service::INotebookLoggingService;
use crate::services::language_detection::language_detection_worker_service::ILanguageDetectionService;

/// `'content' | 'language' | 'mime' | { type: 'model'; event: IModelContentChangedEvent }`.
#[derive(Clone, Debug)]
pub enum CellContentChange {
    Content,
    Language,
    Mime,
    Model(ModelContentChangedEvent),
}

const AUTO_DETECT_LANGUAGE_THROTTLE_DELAY: u64 = 600;

pub struct NotebookCellTextModel {
    base: Rc<Disposable>,
    this: Weak<NotebookCellTextModel>,
    uri: URI,
    handle: i64,
    pub transient_options: TransientOptions,
    language_service: Rc<dyn ILanguageService>,
    default_eol: DefaultEndOfLine,
    language_detection_service: Option<Rc<dyn ILanguageDetectionService>>,
    notebook_logging_service: Rc<dyn INotebookLoggingService>,

    on_did_change_text_model: Rc<Emitter<()>>,
    on_did_change_outputs: Rc<Emitter<NotebookCellOutputsSplice>>,
    on_did_change_output_items: Rc<Emitter<()>>,
    on_did_change_content: Rc<Emitter<CellContentChange>>,
    on_did_change_metadata: Rc<Emitter<()>>,
    on_did_change_internal_metadata: Rc<Emitter<CellInternalMetadataChangedEvent>>,
    on_did_change_language: Rc<Emitter<String>>,

    outputs: RefCell<Vec<Rc<dyn ICellOutput>>>,
    metadata: RefCell<NotebookCellMetadata>,
    internal_metadata: RefCell<NotebookCellInternalMetadata>,
    text_buffer: RefCell<Option<TextBufferRef>>,
    text_buffer_hash: RefCell<Option<String>>,
    hash: Cell<Option<i32>>,
    version_id: Cell<i64>,
    alternative_id: Cell<i64>,
    text_model_disposables: Rc<DisposableStore>,
    text_model: RefCell<Option<Rc<TextModel>>>,
    auto_detect_language_throttler: Rc<ThrottledDelayer<()>>,
    auto_language_detection_enabled: Cell<bool>,
    has_language_set_explicitly: Cell<bool>,
    source: JsString,
    language: RefCell<String>,
    mime: RefCell<Option<String>>,
    cell_kind: CellKind,
    pub collapse_state: Option<NotebookCellCollapseState>,
}

impl NotebookCellTextModel {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        uri: URI,
        handle: i64,
        cell: ICellDto2,
        transient_options: TransientOptions,
        language_service: Rc<dyn ILanguageService>,
        default_eol: DefaultEndOfLine,
        default_collapse_config: Option<NotebookCellDefaultCollapseConfig>,
        language_detection_service: Option<Rc<dyn ILanguageDetectionService>>,
        notebook_logging_service: Rc<dyn INotebookLoggingService>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|this| {
            let base = Disposable::new();
            let on_did_change_text_model = Emitter::<()>::new();
            let on_did_change_outputs = Emitter::<NotebookCellOutputsSplice>::new();
            let on_did_change_output_items = Emitter::<()>::new();
            let on_did_change_content = Emitter::<CellContentChange>::new();
            let on_did_change_metadata = Emitter::<()>::new();
            let on_did_change_internal_metadata = Emitter::<CellInternalMetadataChangedEvent>::new();
            let on_did_change_language = Emitter::<String>::new();
            base.register(on_did_change_text_model.clone());
            base.register(on_did_change_outputs.clone());
            base.register(on_did_change_output_items.clone());
            base.register(on_did_change_content.clone());
            base.register(on_did_change_metadata.clone());
            base.register(on_did_change_internal_metadata.clone());
            base.register(on_did_change_language.clone());

            let text_model_disposables = DisposableStore::new();
            base.register(text_model_disposables.clone());
            let auto_detect_language_throttler = Rc::new(ThrottledDelayer::<()>::new(AUTO_DETECT_LANGUAGE_THROTTLE_DELAY));
            base.register(auto_detect_language_throttler.clone());

            // Compute collapse state: use cell's state if provided, otherwise use default config for this cell kind
            let default_config = default_collapse_config.and_then(|config| match cell.cell_kind {
                CellKind::Code => config.code_cell,
                CellKind::Markup => config.markup_cell,
            });
            let collapse_state = cell.collapse_state.or(default_config);
            let outputs: Vec<Rc<dyn ICellOutput>> =
                cell.outputs.into_iter().map(|op| -> Rc<dyn ICellOutput> { NotebookCellOutputTextModel::new(op) }).collect();

            Self {
                base,
                this: this.clone(),
                uri,
                handle,
                transient_options,
                language_service,
                default_eol,
                language_detection_service,
                notebook_logging_service,
                on_did_change_text_model,
                on_did_change_outputs,
                on_did_change_output_items,
                on_did_change_content,
                on_did_change_metadata,
                on_did_change_internal_metadata,
                on_did_change_language,
                outputs: RefCell::new(outputs),
                metadata: RefCell::new(cell.metadata.unwrap_or_else(empty_object)),
                internal_metadata: RefCell::new(cell.internal_metadata.unwrap_or_default()),
                text_buffer: RefCell::new(None),
                text_buffer_hash: RefCell::new(None),
                hash: Cell::new(None),
                version_id: Cell::new(1),
                alternative_id: Cell::new(1),
                text_model_disposables,
                text_model: RefCell::new(None),
                auto_detect_language_throttler,
                auto_language_detection_enabled: Cell::new(false),
                has_language_set_explicitly: Cell::new(false),
                source: cell.source,
                language: RefCell::new(cell.language),
                mime: RefCell::new(cell.mime),
                cell_kind: cell.cell_kind,
                collapse_state,
            }
        })
    }

    pub fn on_did_change_content(&self) -> Event<CellContentChange> {
        self.on_did_change_content.event()
    }

    pub fn alternative_id(&self) -> i64 {
        self.alternative_id.get()
    }

    pub fn has_language_set_explicitly(&self) -> bool {
        self.has_language_set_explicitly.get()
    }

    pub fn mime(&self) -> Option<String> {
        self.mime.borrow().clone()
    }

    pub fn set_mime(&self, new_mime: Option<String>) {
        if *self.mime.borrow() == new_mime {
            return;
        }
        *self.mime.borrow_mut() = new_mime;
        self.hash.set(None);
        self.on_did_change_content.fire(CellContentChange::Mime);
    }

    fn set_registered_language(&self, new_language: &str, current_language: &str) {
        // The language defined in the cell might not be supported in the editor so the text model might be using the
        // default fallback. If so let's not modify the language
        let is_fall_back_language = new_language == PLAINTEXT_LANGUAGE_ID || new_language == "jupyter";
        if !self.language_service.is_registered_language_id(Some(current_language)) && is_fall_back_language {
            // notify to display warning, but don't change the language
            self.on_did_change_language.fire(current_language.to_string());
        } else {
            self.set_language(new_language);
        }
    }

    pub fn enable_auto_language_detection(&self) {
        self.auto_language_detection_enabled.set(true);
        let _ = self.auto_detect_language();
    }

    pub fn auto_detect_language(&self) -> Promise<()> {
        if self.auto_language_detection_enabled.get() {
            let this = self.this.clone();
            let _ = self.auto_detect_language_throttler.trigger(
                move |_token| match this.upgrade() {
                    Some(this) => this.do_auto_detect_language(),
                    None => Promise::resolved(()),
                },
                None,
            );
        }
        Promise::resolved(())
    }

    fn do_auto_detect_language(&self) -> Promise<()> {
        if self.has_language_set_explicitly() {
            return Promise::resolved(());
        }

        let detected = match &self.language_detection_service {
            Some(service) => service.detect_language(&self.uri, None),
            None => Promise::resolved(None),
        };
        let this = self.this.clone();
        detected.then(move |new_language| {
            let Some(this) = this.upgrade() else {
                return;
            };
            let Some(new_language) = new_language.filter(|language| !language.is_empty()) else {
                return;
            };

            if let Some(text_model) = this.text_model.borrow().clone() {
                let model_language = Some(text_model.get_language_id());
                if model_language == this.language_service.get_language_id_by_language_name(&new_language)
                    && model_language == this.language_service.get_language_id_by_language_name(&this.language())
                {
                    return;
                }
            }

            this.set_language_internal(&new_language);
        })
    }

    fn set_language_internal(&self, new_language: &str) {
        let Some(new_language_id) = self.language_service.get_language_id_by_language_name(new_language) else {
            return;
        };

        if let Some(text_model) = self.text_model.borrow().clone() {
            let language_id = self.language_service.create_by_id(Some(&new_language_id));
            text_model.set_language(&language_id.language_id(), None);
        }

        if self.language.borrow().as_str() == new_language {
            return;
        }

        *self.language.borrow_mut() = new_language.to_string();
        self.hash.set(None);
        self.on_did_change_language.fire(new_language.to_string());
        self.on_did_change_content.fire(CellContentChange::Language);
    }

    pub fn reset_text_buffer(&self, text_buffer: TextBufferRef) {
        *self.text_buffer.borrow_mut() = Some(text_buffer);
    }

    pub fn get_text_buffer_hash(&self) -> String {
        if let Some(text_buffer_hash) = self.text_buffer_hash.borrow().clone() {
            return text_buffer_hash;
        }

        let mut sha_computer = StringSha1::new();
        let text_buffer = self.text_buffer();
        let mut snapshot = text_buffer.borrow().create_snapshot(false);
        while let Some(text) = snapshot.read().filter(|text| !text.is_empty()) {
            sha_computer.update(text.units());
        }
        let digest = sha_computer.digest();
        *self.text_buffer_hash.borrow_mut() = Some(digest.clone());
        digest
    }

    fn get_persistent_metadata(&self) -> String {
        get_formatted_metadata_json(
            Some(&self.transient_options.transient_cell_metadata),
            &self.metadata(),
            Some(&self.language()),
            None,
        )
    }

    pub fn get_text_length(&self) -> usize {
        self.text_buffer().borrow().get_length()
    }

    pub fn get_full_model_range(&self) -> Range {
        let text_buffer = self.text_buffer();
        let text_buffer = text_buffer.borrow();
        let line_count = text_buffer.get_line_count() as i32;
        Range::new(1, 1, line_count, text_buffer.get_line_length(line_count) as i32 + 1)
    }

    pub fn splice_notebook_cell_outputs(&self, splice: NotebookCellOutputsSplice) {
        self.notebook_logging_service.trace(
            "textModelEdits",
            &format!("splicing outputs at {} length: {} with {} new outputs", splice.start, splice.delete_count, splice.new_outputs.len()),
        );
        if splice.delete_count > 0 && !splice.new_outputs.is_empty() {
            let common_len = splice.delete_count.min(splice.new_outputs.len());
            // update
            for i in 0..common_len {
                let current_output = self.outputs.borrow()[splice.start + i].clone();
                let new_output = splice.new_outputs[i].clone();

                self.replace_output(&current_output.output_id(), new_output);
            }

            let remaining_outputs = splice.new_outputs[common_len..].to_vec();
            let removed = self.splice_outputs(splice.start + common_len, splice.delete_count - common_len, remaining_outputs.clone());
            dispose_all(removed.iter());
            self.on_did_change_outputs.fire(NotebookCellOutputsSplice {
                start: splice.start + common_len,
                delete_count: splice.delete_count - common_len,
                new_outputs: remaining_outputs,
            });
        } else {
            let removed = self.splice_outputs(splice.start, splice.delete_count, splice.new_outputs.clone());
            dispose_all(removed.iter());
            self.on_did_change_outputs.fire(splice);
        }
    }

    /// `this.outputs.splice(start, deleteCount, ...items)`: `start` e `deleteCount` limitados ao tamanho da lista.
    fn splice_outputs(&self, start: usize, delete_count: usize, items: Vec<Rc<dyn ICellOutput>>) -> Vec<Rc<dyn ICellOutput>> {
        let mut outputs = self.outputs.borrow_mut();
        let start = start.min(outputs.len());
        let end = start.saturating_add(delete_count).min(outputs.len());
        outputs.splice(start..end, items).collect()
    }

    pub fn replace_output(&self, output_id: &str, new_output_item: Rc<dyn ICellOutput>) -> bool {
        let output_index = self.outputs.borrow().iter().position(|output| output.output_id() == output_id);
        let Some(output_index) = output_index else {
            return false;
        };

        self.notebook_logging_service.trace("textModelEdits", &format!("replacing an output item at index {output_index}"));
        let output = self.outputs.borrow()[output_index].clone();
        // convert to dto and dispose the cell output model
        output.replace_data(IOutputDto {
            outputs: new_output_item.outputs(),
            output_id: new_output_item.output_id(),
            metadata: new_output_item.metadata(),
        });
        new_output_item.dispose();
        self.on_did_change_output_items.fire(());
        true
    }

    pub fn change_output_items(&self, output_id: &str, append: bool, items: Vec<IOutputItemDto>) -> bool {
        let output_index = self.outputs.borrow().iter().position(|output| output.output_id() == output_id);
        let Some(output_index) = output_index else {
            return false;
        };

        let output = self.outputs.borrow()[output_index].clone();
        self.notebook_logging_service.trace(
            "textModelEdits",
            &format!("{} {} output items to for output index {}", if append { "appending" } else { "replacing" }, items.len(), output_index),
        );
        if append {
            output.append_data(items);
        } else {
            output.replace_data(IOutputDto { output_id: output_id.to_string(), outputs: items, metadata: output.metadata() });
        }
        self.on_did_change_output_items.fire(());
        true
    }

    fn output_not_equal_fast_check(&self, left: &[Rc<dyn ICellOutput>], right: &[Rc<dyn ICellOutput>]) -> bool {
        if left.len() != right.len() {
            return false;
        }

        for i in 0..self.outputs.borrow().len() {
            let l = left[i].outputs();
            let r = right[i].outputs();

            if l.len() != r.len() {
                return false;
            }

            for k in 0..l.len() {
                if l[k].mime != r[k].mime {
                    return false;
                }

                if l[k].data.byte_length() != r[k].data.byte_length() {
                    return false;
                }
            }
        }

        true
    }

    pub fn equal(&self, b: &NotebookCellTextModel) -> bool {
        if self.language() != b.language() {
            return false;
        }

        if self.outputs.borrow().len() != b.outputs.borrow().len() {
            return false;
        }

        if self.get_text_length() != b.get_text_length() {
            return false;
        }

        if !self.transient_options.transient_outputs {
            // compare outputs
            if !self.output_not_equal_fast_check(&self.outputs(), &b.outputs()) {
                return false;
            }
        }

        self.get_hash_value() == b.get_hash_value()
    }

    /// Only compares
    /// - language
    /// - mime
    /// - cellKind
    /// - internal metadata (conditionally)
    /// - source
    pub fn fast_equal(&self, b: &ICellDto2, ignore_metadata: bool) -> bool {
        if self.language() != b.language {
            return false;
        }

        if self.mime() != b.mime {
            return false;
        }

        if self.cell_kind != b.cell_kind {
            return false;
        }

        if !ignore_metadata {
            let internal_metadata = self.internal_metadata.borrow();
            let other = b.internal_metadata.as_ref();
            if internal_metadata.execution_order != other.and_then(|m| m.execution_order)
                || internal_metadata.last_run_success != other.and_then(|m| m.last_run_success)
                || internal_metadata.run_start_time != other.and_then(|m| m.run_start_time)
                || internal_metadata.run_start_time_adjustment != other.and_then(|m| m.run_start_time_adjustment)
                || internal_metadata.run_end_time != other.and_then(|m| m.run_end_time)
            {
                return false;
            }
        }

        // Once we attach the cell text buffer to an editor, the source of truth is the text buffer instead of the original source
        let attached_buffer = self.text_buffer.borrow().clone();
        match attached_buffer {
            Some(text_buffer) => {
                if !Self::lines_are_equal(&text_buffer.borrow().get_lines_content(), &b.source) {
                    return false;
                }
            }
            None => {
                if self.source != b.source {
                    return false;
                }
            }
        }

        true
    }

    fn lines_are_equal(a_lines: &[JsString], b: &JsString) -> bool {
        let b_lines = split_lines(b.units());
        if a_lines.len() != b_lines.len() {
            return false;
        }
        a_lines.iter().zip(b_lines.iter()).all(|(a, b)| a.units() == *b)
    }
}

impl IDisposable for NotebookCellTextModel {
    fn dispose(&self) {
        let outputs = self.outputs.borrow().clone();
        dispose_all(outputs.iter());
        // Manually release reference to previous text buffer to avoid large leaks in case someone leaks a CellTextModel reference
        let empty_disposed_text_buffer = PieceTreeTextBuffer::new(Vec::new(), JsString::new(), Eol::Lf, false, false, true, true);
        empty_disposed_text_buffer.dispose();
        *self.text_buffer.borrow_mut() = Some(Rc::new(RefCell::new(empty_disposed_text_buffer)));
        self.base.dispose();
    }
}

impl ICell for NotebookCellTextModel {
    fn uri(&self) -> URI {
        self.uri.clone()
    }

    fn handle(&self) -> i64 {
        self.handle
    }

    fn language(&self) -> String {
        self.language.borrow().clone()
    }

    fn set_language(&self, new_language: &str) {
        if let Some(text_model) = self.text_model.borrow().clone() {
            let model_language = Some(text_model.get_language_id());
            // 1. the language update is from workspace edit, checking if it's the same as text model's mode
            if model_language == self.language_service.get_language_id_by_language_name(new_language)
                // 2. the text model's mode might be the same as the `this.language`, even if the language friendly name is not the same, we should not trigger an update
                && model_language == self.language_service.get_language_id_by_language_name(&self.language())
            {
                return;
            }
        }

        self.has_language_set_explicitly.set(true);
        self.set_language_internal(new_language);
    }

    fn cell_kind(&self) -> CellKind {
        self.cell_kind
    }

    fn outputs(&self) -> Vec<Rc<dyn ICellOutput>> {
        self.outputs.borrow().clone()
    }

    fn metadata(&self) -> NotebookCellMetadata {
        self.metadata.borrow().clone()
    }

    fn set_metadata(&self, new_metadata: NotebookCellMetadata) {
        *self.metadata.borrow_mut() = new_metadata;
        self.hash.set(None);
        self.on_did_change_metadata.fire(());
    }

    fn internal_metadata(&self) -> NotebookCellInternalMetadata {
        self.internal_metadata.borrow().clone()
    }

    fn set_internal_metadata(&self, new_internal_metadata: NotebookCellInternalMetadata) {
        let last_run_success_changed = self.internal_metadata.borrow().last_run_success != new_internal_metadata.last_run_success;
        let run_start_time_adjustment = compute_run_start_time_adjustment(&self.internal_metadata.borrow(), &new_internal_metadata);
        let new_internal_metadata = NotebookCellInternalMetadata { run_start_time_adjustment, ..new_internal_metadata };
        *self.internal_metadata.borrow_mut() = new_internal_metadata;
        self.hash.set(None);
        self.on_did_change_internal_metadata.fire(CellInternalMetadataChangedEvent { last_run_success_changed: Some(last_run_success_changed) });
    }

    fn get_hash_value(&self) -> i32 {
        if let Some(hash_value) = self.hash.get() {
            return hash_value;
        }

        let outputs = if self.transient_options.transient_outputs {
            Vec::new()
        } else {
            self.outputs
                .borrow()
                .iter()
                .map(|op| {
                    let items = op.outputs().into_iter().map(|output| {
                        let data = output.data.with_bytes(|bytes| bytes.iter().map(|&byte| Dynamic::Number(f64::from(byte))).collect::<Vec<_>>());
                        Dynamic::object([("mime", Dynamic::string(&output.mime)), ("data", Dynamic::array(data))])
                    });
                    Dynamic::object([("outputs", Dynamic::array(items)), ("metadata", op.metadata().unwrap_or(Dynamic::Undefined))])
                })
                .collect()
        };
        let hash_value = hash(&Dynamic::array([
            Dynamic::Number(f64::from(hash(&Dynamic::string(&self.language())))),
            Dynamic::string(&self.get_text_buffer_hash()),
            Dynamic::string(&self.get_persistent_metadata()),
            Dynamic::array(outputs),
        ]));
        self.hash.set(Some(hash_value));
        hash_value
    }

    fn text_buffer(&self) -> TextBufferRef {
        if let Some(text_buffer) = self.text_buffer.borrow().clone() {
            return text_buffer;
        }

        let buffer = create_text_buffer(TextSource::Text(self.source.clone()), self.default_eol);
        self.base.register(buffer.disposable());
        let on_did_change_content = buffer.on_did_change_content();
        let text_buffer: TextBufferRef = Rc::new(RefCell::new(buffer));
        *self.text_buffer.borrow_mut() = Some(text_buffer.clone());

        let this = self.this.clone();
        self.base.register(on_did_change_content.listen(move |_| {
            let Some(this) = this.upgrade() else {
                return;
            };
            this.hash.set(None);
            if this.text_model.borrow().is_none() {
                this.on_did_change_content.fire(CellContentChange::Content);
            }
            let _ = this.auto_detect_language();
        }));

        text_buffer
    }

    fn text_model(&self) -> Option<Rc<TextModel>> {
        self.text_model.borrow().clone()
    }

    fn set_text_model(&self, m: Option<Rc<TextModel>>) {
        let unchanged = match (&*self.text_model.borrow(), &m) {
            (None, None) => true,
            (Some(current), Some(next)) => Rc::ptr_eq(current, next),
            _ => false,
        };
        if unchanged {
            return;
        }

        self.text_model_disposables.clear();
        *self.text_model.borrow_mut() = m.clone();
        let Some(text_model) = m else {
            return;
        };

        self.set_registered_language(&text_model.get_language_id(), &self.language());

        // Listen to language changes on the model
        let this = self.this.clone();
        self.text_model_disposables.add(text_model.on_did_change_language().listen(move |e| {
            if let Some(this) = this.upgrade() {
                this.set_registered_language(&e.new_language, &this.language());
            }
        }));
        let this = self.this.clone();
        self.text_model_disposables.add(text_model.on_will_dispose().listen(move |_| {
            if let Some(this) = this.upgrade() {
                this.set_text_model(None);
            }
        }));
        let this = self.this.clone();
        self.text_model_disposables.add(text_model.on_did_change_content().listen(move |e| {
            let Some(this) = this.upgrade() else {
                return;
            };
            if let Some(text_model) = this.text_model.borrow().clone() {
                this.version_id.set(text_model.get_version_id());
                this.alternative_id.set(text_model.get_alternative_version_id());
            }
            *this.text_buffer_hash.borrow_mut() = None;
            this.on_did_change_content.fire(CellContentChange::Content);
            this.on_did_change_content.fire(CellContentChange::Model(e.clone()));
        }));

        text_model.overwrite_version_id(self.version_id.get());
        text_model.overwrite_alternative_version_id(self.version_id.get());
        self.on_did_change_text_model.fire(());
    }

    fn on_did_change_text_model(&self) -> Event<()> {
        self.on_did_change_text_model.event()
    }

    fn get_value(&self) -> JsString {
        let full_range = self.get_full_model_range();
        let text_buffer = self.text_buffer();
        let text_buffer = text_buffer.borrow();
        let eol = if text_buffer.get_eol() == Eol::Lf { EndOfLinePreference::Lf } else { EndOfLinePreference::CrLf };
        text_buffer.get_value_in_range(&full_range, eol)
    }

    fn on_did_change_outputs(&self) -> Event<NotebookCellOutputsSplice> {
        self.on_did_change_outputs.event()
    }

    fn on_did_change_output_items(&self) -> Event<()> {
        self.on_did_change_output_items.event()
    }

    fn on_did_change_language(&self) -> Event<String> {
        self.on_did_change_language.event()
    }

    fn on_did_change_metadata(&self) -> Event<()> {
        self.on_did_change_metadata.event()
    }

    fn on_did_change_internal_metadata(&self) -> Event<CellInternalMetadataChangedEvent> {
        self.on_did_change_internal_metadata.event()
    }
}

/// `{}`.
fn empty_object() -> Dynamic {
    Dynamic::Object(ObjectRef::new())
}

/// `cloneNotebookCellTextModel`.
pub fn clone_notebook_cell_text_model(cell: &NotebookCellTextModel) -> ICellDto2 {
    ICellDto2 {
        source: cell.get_value(),
        language: cell.language(),
        mime: cell.mime(),
        cell_kind: cell.cell_kind(),
        outputs: cell
            .outputs()
            .iter()
            .map(|output| IOutputDto {
                outputs: output.outputs(),
                /* paste should generate new outputId */ output_id: generate_uuid(),
                metadata: None,
            })
            .collect(),
        metadata: Some(empty_object()),
        internal_metadata: None,
        collapse_state: None,
    }
}

fn compute_run_start_time_adjustment(old_metadata: &NotebookCellInternalMetadata, new_metadata: &NotebookCellInternalMetadata) -> Option<f64> {
    match new_metadata.run_start_time {
        Some(run_start_time) if old_metadata.run_start_time != new_metadata.run_start_time => {
            let offset = runtime::date_now() as f64 - run_start_time;
            Some(if offset < 0.0 { offset.abs() } else { 0.0 })
        }
        _ => new_metadata.run_start_time_adjustment,
    }
}

/// `getFormattedMetadataJSON`.
pub fn get_formatted_metadata_json(
    transient_cell_metadata: Option<&TransientCellMetadata>,
    metadata: &NotebookCellMetadata,
    language: Option<&str>,
    sort_keys: Option<bool>,
) -> String {
    let own_entries = metadata.own_entries();
    let filtered_metadata: Vec<(JsString, Dynamic)> = match transient_cell_metadata {
        Some(transient) => own_entries.into_iter().filter(|(key, _)| !transient.get(&key.to_string_lossy()).copied().unwrap_or(false)).collect(),
        None => own_entries,
    };

    // `{ language, ...filteredMetadata }`: `language` abre o objeto e um `language` dos metadados só troca o valor.
    let language_key = JsString::from("language");
    let mut entries: Vec<(JsString, Dynamic)> = vec![(language_key.clone(), language.map_or(Dynamic::Undefined, Dynamic::string))];
    for (key, value) in filtered_metadata {
        match entries.iter_mut().find(|(existing, _)| *existing == key) {
            Some(existing) => existing.1 = value,
            None => entries.push((key, value)),
        }
    }
    // Give preference to the language we have been given. Metadata can contain `language` due to round-tripping of
    // cell metadata. I.e. we add it here, and then from SCM when we revert the cell, we get this same metadata back
    // with the `language` property.
    if let Some(language) = language.filter(|language| !language.is_empty()) {
        entries[0].1 = Dynamic::string(language);
    }
    let obj = Dynamic::object(entries);
    let obj = if sort_keys.unwrap_or(false) { sort_object_properties_recursively(&obj) } else { obj };
    let value = to_json(&obj).unwrap_or(je_base::json::Value::Null);
    je_base::json_formatter::to_formatted_string(&value, &je_base::json_formatter::FormattingOptions::default()).to_string_lossy()
}

/// Sort the JSON to ensure when diffing, the JSON keys are sorted & matched correctly in diff view.
pub fn sort_object_properties_recursively(obj: &Dynamic) -> Dynamic {
    match obj {
        Dynamic::Array(array) => Dynamic::array(array.to_vec().iter().map(sort_object_properties_recursively)),
        Dynamic::Object(_) if !obj.own_keys().is_empty() => {
            let mut entries = obj.own_entries();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Dynamic::object(entries.into_iter().map(|(key, value)| (key, sort_object_properties_recursively(&value))))
        }
        _ => obj.clone(),
    }
}
