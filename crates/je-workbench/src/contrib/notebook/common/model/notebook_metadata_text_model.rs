//! Porte de `src/vs/workbench/contrib/notebook/common/model/notebookMetadataTextModel.ts`: o texto JSON dos metadados do
//! notebook (`getFormattedNotebookMetadataJSON`) e o modelo que o mantém (`NotebookDocumentMetadataTextModel`).
//!
//! Divergências:
//! - `Disposable` é o campo `base`; o modelo nasce como `Rc<Self>` (`Rc::new_cyclic`) e os fechos dos eventos guardam um
//!   `Weak` dele (mesmo padrão de [`NotebookCellTextModel`](super::notebook_cell_text_model::NotebookCellTextModel)).
//! - `INotebookTextModel` é o `Rc<NotebookTextModel>` concreto (como no resto do porte).
//! - `ITextBuffer` é o [`TextBufferRef`] compartilhado, e `createTextBuffer(source, LF).textBuffer` é o
//!   `create_text_buffer` do porte (que devolve o buffer; o `dispose` do original é o `disposable()` registrado na base).
//! - `toFormattedString(filteredMetadata, {})` é `to_json` seguido de `to_formatted_string` com as opções padrão (o mesmo
//!   caminho de `getFormattedMetadataJSON`).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::hash::StringSha1;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::uri::URI;
use je_editor::model::{DefaultEndOfLine, EndOfLinePreference, Eol};
use je_editor::range::Range;
use je_editor::text_model::{TextSource, create_text_buffer};
use je_platform::js::to_json;

use super::notebook_text_model::NotebookTextModel;
use crate::contrib::notebook::common::notebook_common::{
    INotebookDocumentMetadataTextModel, NotebookCellsChangeType, NotebookDocumentMetadata, NotebookMetadataUri, TextBufferRef, TransientDocumentMetadata,
};

/// `getFormattedNotebookMetadataJSON`.
pub fn get_formatted_notebook_metadata_json(transient_metadata: Option<&TransientDocumentMetadata>, metadata: &NotebookDocumentMetadata) -> String {
    let filtered_metadata = match transient_metadata {
        Some(transient) => {
            let entries: Vec<(JsString, Dynamic)> =
                metadata.own_entries().into_iter().filter(|(key, _)| !transient.get(&key.to_string_lossy()).copied().unwrap_or(false)).collect();
            Dynamic::object(entries)
        }
        None => metadata.clone(),
    };

    let value = to_json(&filtered_metadata).unwrap_or(je_base::json::Value::Null);
    je_base::json_formatter::to_formatted_string(&value, &je_base::json_formatter::FormattingOptions::default()).to_string_lossy()
}

/// `NotebookDocumentMetadataTextModel`.
pub struct NotebookDocumentMetadataTextModel {
    base: Rc<Disposable>,
    this: Weak<NotebookDocumentMetadataTextModel>,
    uri: URI,
    pub notebook_model: Rc<NotebookTextModel>,
    on_did_change: Rc<Emitter<()>>,
    text_buffer_hash: RefCell<Option<String>>,
    text_buffer: RefCell<Option<TextBufferRef>>,
}

impl NotebookDocumentMetadataTextModel {
    pub fn new(notebook_model: Rc<NotebookTextModel>) -> Rc<Self> {
        let this = Rc::new_cyclic(|this| {
            let base = Disposable::new();
            let on_did_change = Emitter::<()>::new();
            base.register(on_did_change.clone());
            Self {
                base,
                this: this.clone(),
                uri: NotebookMetadataUri::generate(&notebook_model.uri()),
                notebook_model: notebook_model.clone(),
                on_did_change,
                text_buffer_hash: RefCell::new(None),
                text_buffer: RefCell::new(None),
            }
        });

        let weak = Rc::downgrade(&this);
        this.base.register(notebook_model.on_did_change_content().listen(move |e| {
            let Some(this) = weak.upgrade() else { return };
            if e.raw_events.iter().any(|event| matches!(event.event.kind(), NotebookCellsChangeType::ChangeDocumentMetadata | NotebookCellsChangeType::ModelChange)) {
                let text_buffer = this.text_buffer.borrow_mut().take();
                if let Some(text_buffer) = text_buffer {
                    text_buffer.borrow().dispose();
                }
                *this.text_buffer_hash.borrow_mut() = None;
                this.on_did_change.fire(());
            }
        }));

        this
    }

    fn get_full_model_range(&self) -> Range {
        let text_buffer = INotebookDocumentMetadataTextModel::text_buffer(self);
        let text_buffer = text_buffer.borrow();
        let line_count = text_buffer.get_line_count() as i32;
        Range::new(1, 1, line_count, text_buffer.get_line_length(line_count) as i32 + 1)
    }
}

impl INotebookDocumentMetadataTextModel for NotebookDocumentMetadataTextModel {
    fn uri(&self) -> URI {
        self.uri.clone()
    }

    fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    fn metadata(&self) -> NotebookDocumentMetadata {
        self.notebook_model.metadata()
    }

    fn text_buffer(&self) -> TextBufferRef {
        if let Some(text_buffer) = self.text_buffer.borrow().clone() {
            return text_buffer;
        }

        let source = get_formatted_notebook_metadata_json(Some(&self.notebook_model.transient_options().transient_document_metadata), &self.metadata());
        let buffer = create_text_buffer(TextSource::Text(JsString::from(source)), DefaultEndOfLine::Lf);
        self.base.register(buffer.disposable());
        let on_did_change_content = buffer.on_did_change_content();
        let text_buffer: TextBufferRef = Rc::new(RefCell::new(buffer));
        *self.text_buffer.borrow_mut() = Some(text_buffer.clone());

        let weak = self.this.clone();
        self.base.register(on_did_change_content.listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.on_did_change.fire(());
            }
        }));

        text_buffer
    }

    fn get_hash(&self) -> String {
        if let Some(text_buffer_hash) = self.text_buffer_hash.borrow().clone() {
            return text_buffer_hash;
        }

        let mut sha_computer = StringSha1::new();
        let text_buffer = INotebookDocumentMetadataTextModel::text_buffer(self);
        let mut snapshot = text_buffer.borrow().create_snapshot(false);
        while let Some(text) = snapshot.read().filter(|text| !text.is_empty()) {
            sha_computer.update(text.units());
        }
        let digest = sha_computer.digest();
        *self.text_buffer_hash.borrow_mut() = Some(digest.clone());
        digest
    }

    fn get_value(&self) -> String {
        let full_range = self.get_full_model_range();
        let text_buffer = INotebookDocumentMetadataTextModel::text_buffer(self);
        let text_buffer = text_buffer.borrow();
        let eol = if text_buffer.get_eol() == Eol::Lf { EndOfLinePreference::Lf } else { EndOfLinePreference::CrLf };
        text_buffer.get_value_in_range(&full_range, eol).to_string_lossy()
    }
}

impl IDisposable for NotebookDocumentMetadataTextModel {
    fn dispose(&self) {
        self.base.dispose();
    }
}
