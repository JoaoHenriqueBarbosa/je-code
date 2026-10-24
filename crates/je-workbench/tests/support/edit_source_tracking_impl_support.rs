//! `MutableObservableWorkspace` e `MutableObservableDocument` de
//! `src/vs/workbench/contrib/editTelemetry/test/browser/editTelemetry.test.ts`, o espaço de trabalho observável
//! mutável que os testes do `editTelemetry` usam.
//!
//! Notas de tradução:
//! - `extends ObservableWorkspace` é o `impl ObservableWorkspace` com o [`ObservableWorkspaceState`] guardado (ver
//!   `observable_workspace`); `getDocument` do original devolve o `MutableObservableDocument`, então o método do trait
//!   (que devolve o documento como `dyn`) é sobrescrito e [`MutableObservableWorkspace::get_mutable_document`] devolve o
//!   tipo concreto;
//! - `{ added, removed }` do delta de `_openDocuments` é o [`DocumentsChange`];
//! - `applyEdit(edit: StringEdit | StringEditWithReason, ...)` recebe o [`AppliedEdit`] (`From` das duas formas);
//! - os fechos de descarte dos documentos guardam `Rc` do mapa e do observável (não do espaço de trabalho), e removem o
//!   documento do mapa no descarte, o que desfaz o ciclo;
//! - o `SearchString` de `findRange` (com `≪` e `≫` para dar contexto) é procurado em unidades UTF-16, como o `indexOf`
//!   do original.
#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;
use je_base::errors::{self, Error};
use je_base::lifecycle::{Disposable, IDisposable, to_disposable};
use je_base::observable::{IObservable, ITransaction, Observable, SettableExt, observable_value, subtransaction};
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::base::Settable;
use je_base::uri::URI;
use je_editor::edits::edit::BaseEdit;
use je_editor::edits::string_edit::{BaseStringEdit, StringEdit, StringReplacement};
use je_editor::offset_range::OffsetRange;
use je_editor::text::abstract_text::StringText;
use je_editor::text_model_edit_source::EditSources;
use je_workbench::contrib::edit_telemetry::browser::helpers::observable_workspace::{
    DocumentList, IObservableDocument, ObservableWorkspace, ObservableWorkspaceState, StringEditWithReason,
};

/// O delta `{ added, removed }` de `_openDocuments`.
#[derive(Clone)]
pub struct DocumentsChange {
    pub added: Vec<Rc<dyn IObservableDocument>>,
    pub removed: Vec<Rc<dyn IObservableDocument>>,
}

type OpenDocuments = Rc<ObservableValue<DocumentList, DocumentsChange>>;
type DocumentMap = Rc<RefCell<HashMap<String, Rc<MutableObservableDocument>>>>;

fn uri_key(uri: &URI) -> String {
    uri.to_string(false).to_string()
}

/// As opções de `createDocument`.
#[derive(Default)]
pub struct CreateDocumentOptions {
    pub uri: Option<URI>,
    pub workspace_root: Option<URI>,
    pub initial_value: Option<String>,
    pub initial_version_id: Option<i32>,
    pub language_id: Option<String>,
}

/// `MutableObservableWorkspace`.
pub struct MutableObservableWorkspace {
    state: ObservableWorkspaceState,
    open_documents: OpenDocuments,
    documents: DocumentMap,
}

impl MutableObservableWorkspace {
    pub fn new() -> Rc<Self> {
        let list: DocumentList = Rc::from(Vec::<Rc<dyn IObservableDocument>>::new());
        let open_documents: OpenDocuments = observable_value::<DocumentList, DocumentsChange>("openDocuments", list);
        let state = ObservableWorkspaceState::new(open_documents.to_observable());
        Rc::new(Self { state, open_documents, documents: Rc::new(RefCell::new(HashMap::new())) })
    }

    /// Dispose to remove.
    pub fn create_document(&self, options: CreateDocumentOptions, tx: Option<&dyn ITransaction>) -> Rc<MutableObservableDocument> {
        let uri = options.uri.expect("createDocument exige o uri");
        assert!(!self.documents.borrow().contains_key(&uri_key(&uri)));

        let open_documents = self.open_documents.clone();
        let documents = self.documents.clone();
        let removed_uri = uri.clone();
        // O fecho de descarte do original lê o `tx` do `createDocument`; o `ITransaction` não é `'static`, então o descarte
        // usa uma transação própria (o `tx` do original só adia a notificação até o fim da transação do chamador).
        let document = MutableObservableDocument::new(
            uri.clone(),
            StringText::new(options.initial_value.unwrap_or_default()),
            Vec::new(),
            options.language_id.unwrap_or_else(|| "plaintext".to_owned()),
            Box::new(move || {
                documents.borrow_mut().remove(&uri_key(&removed_uri));
                let docs = open_documents.get();
                let filtered: Vec<Rc<dyn IObservableDocument>> = docs.iter().filter(|d| uri_key(d.uri()) != uri_key(&removed_uri)).cloned().collect();
                if filtered.len() != docs.len() {
                    let removed: Vec<Rc<dyn IObservableDocument>> = docs.iter().filter(|d| uri_key(d.uri()) == uri_key(&removed_uri)).cloned().collect();
                    open_documents.set_with_change(Rc::from(filtered), None, DocumentsChange { added: Vec::new(), removed });
                }
            }),
            options.initial_version_id.unwrap_or(0),
            options.workspace_root,
        );

        self.documents.borrow_mut().insert(uri_key(&uri), document.clone());
        let mut open: Vec<Rc<dyn IObservableDocument>> = self.open_documents.get().iter().cloned().collect();
        let as_dyn: Rc<dyn IObservableDocument> = document.clone();
        open.push(as_dyn.clone());
        self.open_documents.set_with_change(Rc::from(open), tx, DocumentsChange { added: vec![as_dyn], removed: Vec::new() });

        document
    }

    /// `getDocument(id)`, com o tipo concreto.
    pub fn get_mutable_document(&self, id: &URI) -> Option<Rc<MutableObservableDocument>> {
        self.documents.borrow().get(&uri_key(id)).cloned()
    }

    pub fn clear(&self) {
        let removed: Vec<Rc<dyn IObservableDocument>> = self.open_documents.get().iter().cloned().collect();
        self.open_documents.set_with_change(Rc::from(Vec::<Rc<dyn IObservableDocument>>::new()), None, DocumentsChange { added: Vec::new(), removed });
        let docs: Vec<Rc<MutableObservableDocument>> = self.documents.borrow().values().cloned().collect();
        for doc in docs {
            doc.dispose();
        }
        self.documents.borrow_mut().clear();
    }
}

impl ObservableWorkspace for MutableObservableWorkspace {
    fn documents(&self) -> IObservable<DocumentList> {
        self.open_documents.to_observable()
    }

    fn state(&self) -> &ObservableWorkspaceState {
        &self.state
    }

    fn get_document(&self, document_id: &URI) -> Option<Rc<dyn IObservableDocument>> {
        self.get_mutable_document(document_id).map(|document| document as Rc<dyn IObservableDocument>)
    }
}

/// `StringEdit | StringEditWithReason`.
pub enum AppliedEdit {
    Plain(StringEdit),
    WithReason(StringEditWithReason),
}

impl From<StringEdit> for AppliedEdit {
    fn from(edit: StringEdit) -> Self {
        Self::Plain(edit)
    }
}

impl From<StringEditWithReason> for AppliedEdit {
    fn from(edit: StringEditWithReason) -> Self {
        Self::WithReason(edit)
    }
}

/// `MutableObservableDocument`.
pub struct MutableObservableDocument {
    base: Rc<Disposable>,
    uri: URI,
    value: Rc<ObservableValue<Rc<StringText>, StringEditWithReason>>,
    selection: Rc<ObservableValue<Rc<Vec<OffsetRange>>, ()>>,
    visible_ranges: Rc<ObservableValue<Rc<Vec<OffsetRange>>, ()>>,
    language_id: Rc<ObservableValue<String, ()>>,
    version: Rc<ObservableValue<i32, ()>>,
    pub workspace_root: Option<URI>,
}

impl MutableObservableDocument {
    pub fn new(
        uri: URI,
        value: StringText,
        selection: Vec<OffsetRange>,
        language_id: String,
        on_dispose: Box<dyn FnOnce()>,
        version_id: i32,
        workspace_root: Option<URI>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        base.register(to_disposable(on_dispose));

        Rc::new(Self {
            base,
            uri,
            value: observable_value::<Rc<StringText>, StringEditWithReason>("value", Rc::new(value)),
            selection: observable_value::<Rc<Vec<OffsetRange>>, ()>("selection", Rc::new(selection)),
            visible_ranges: observable_value::<Rc<Vec<OffsetRange>>, ()>("visibleRanges", Rc::new(Vec::new())),
            language_id: observable_value::<String, ()>("languageId", language_id),
            version: observable_value::<i32, ()>("version", version_id),
            workspace_root,
        })
    }

    pub fn selection(&self) -> IObservable<Rc<Vec<OffsetRange>>> {
        self.selection.to_observable()
    }

    pub fn visible_ranges(&self) -> IObservable<Rc<Vec<OffsetRange>>> {
        self.visible_ranges.to_observable()
    }

    pub fn set_selection(&self, selection: Vec<OffsetRange>, tx: Option<&dyn ITransaction>) {
        self.selection.set(Rc::new(selection), tx);
    }

    pub fn set_visible_range(&self, visible_ranges: Vec<OffsetRange>, tx: Option<&dyn ITransaction>) {
        self.visible_ranges.set(Rc::new(visible_ranges), tx);
    }

    pub fn apply_edit(&self, edit: impl Into<AppliedEdit>, tx: Option<&dyn ITransaction>, new_version: Option<i32>) {
        let edit_with_reason = match edit.into() {
            AppliedEdit::WithReason(edit) => edit,
            AppliedEdit::Plain(edit) => StringEditWithReason::new(edit.replacements().to_vec(), Rc::new(EditSources::unknown(None))),
        };
        let new_value = Rc::new(edit_with_reason.apply_on_text(&self.value.get()));
        subtransaction(tx, |tx| {
            self.value.set_with_change(new_value, Some(tx), edit_with_reason);
            self.version.set(new_version.unwrap_or_else(|| self.version.get() + 1), Some(tx));
        });
    }

    pub fn update_selection(&self, selection: Vec<OffsetRange>, tx: Option<&dyn ITransaction>) {
        self.selection.set(Rc::new(selection), tx);
    }

    pub fn set_value(&self, value: StringText, tx: Option<&dyn ITransaction>, new_version: Option<i32>) {
        let reason = Rc::new(EditSources::unknown(None));
        let length = self.value.get().value.len() as i32;
        let e = StringEditWithReason::new(vec![StringReplacement::replace(OffsetRange::new(0, length), value.value.clone())], reason);
        let value = Rc::new(value);
        subtransaction(tx, |tx| {
            self.value.set_with_change(value, Some(tx), e);
            self.version.set(new_version.unwrap_or_else(|| self.version.get() + 1), Some(tx));
        });
    }

    /// `findRange(search)`.
    pub fn find_range(&self, search: &str) -> OffsetRange {
        find_offset_range(&self.value.get().value, search)
    }
}

impl IObservableDocument for MutableObservableDocument {
    fn uri(&self) -> &URI {
        &self.uri
    }

    fn value(&self) -> IObservable<Rc<StringText>> {
        self.value.to_observable()
    }

    fn version(&self) -> IObservable<i32> {
        self.version.to_observable()
    }

    fn language_id(&self) -> IObservable<String> {
        self.language_id.to_observable()
    }
}

impl IDisposable for MutableObservableDocument {
    fn dispose(&self) {
        self.base.dispose();
    }
}

fn find_units(haystack: &[u16], needle: &[u16]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// `findOffsetRange(str, search)`: `search` pode ter `≪` e `≫` para dar contexto, por exemplo `e≪l≫` só casa o primeiro
/// `l` de `hello`.
pub fn find_offset_range(text: &JsString, search: &str) -> OffsetRange {
    let search_units: Vec<u16> = search.encode_utf16().collect();
    let start_context_index = search_units.iter().position(|unit| *unit == '≪' as u16);
    let end_context_index = search_units.iter().position(|unit| *unit == '≫' as u16);

    let (before_context, after_context, search_str): (Vec<u16>, Vec<u16>, Vec<u16>) = match (start_context_index, end_context_index) {
        (Some(start), Some(end)) if end > start => (search_units[..start].to_vec(), search_units[end + 1..].to_vec(), search_units[start + 1..end].to_vec()),
        _ => (Vec::new(), Vec::new(), search_units.clone()),
    };

    let needle: Vec<u16> = [before_context.as_slice(), search_str.as_slice(), after_context.as_slice()].concat();
    let Some(start_index) = find_units(text.units(), &needle) else {
        errors::throw(Error::new(format!(
            "Could not find context \"{}\" + \"{}\" + \"{}\" in string \"{}\"",
            String::from_utf16_lossy(&before_context),
            String::from_utf16_lossy(&search_str),
            String::from_utf16_lossy(&after_context),
            text
        )))
    };

    let match_start = (start_index + before_context.len()) as i32;
    OffsetRange::new(match_start, match_start + search_str.len() as i32)
}
