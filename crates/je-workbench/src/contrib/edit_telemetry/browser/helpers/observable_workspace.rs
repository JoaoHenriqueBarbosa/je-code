//! Porte de `src/vs/workbench/contrib/editTelemetry/browser/helpers/observableWorkspace.ts`.
//!
//! Representação:
//! - `IObservableWithChange<StringText, StringEditWithReason>` é um `IObservable<Rc<StringText>>` cujo produtor
//!   avisa as mudanças com `set_with_change(.., StringEditWithReason)`; quem lê o delta usa `run_on_change` com
//!   `C = StringEditWithReason` (o tipo da mudança não faz parte do tipo do observável neste porte). O texto vai em
//!   `Rc` porque o `StringText` é um objeto do JavaScript, comparado por identidade.
//! - `readonly IObservableDocument[]` é o [`DocumentList`] (`Rc<[..]>`, comparado por identidade como o array).
//! - `abstract class ObservableWorkspace` é o trait [`ObservableWorkspace`]: o getter abstrato `documents` é o método
//!   exigido e os campos da classe base (`_version`, `onDidOpenDocumentChange`, `lastActiveDocument`) ficam no
//!   [`ObservableWorkspaceState`], que o implementador guarda e devolve em `state()`.
//! - `StringEditWithReason extends StringEdit` é a composição com `Deref` para o `StringEdit`.

use std::cell::Cell;
use std::ops::Deref;
use std::rc::Rc;

use je_base::JsString;
use je_base::observable::{
    DebugNameOptions, FlattenExt, IObservable, IChangeContext, Observable, SettableExt, autorun, derived, observable_value, run_on_change,
};
use je_base::observable::change_tracker::FnChangeTracker;
use je_base::observable::observables::derived::{DerivedHandleChangesOptions, derived_handle_changes};
use je_base::uri::URI;
use je_editor::edits::string_edit::{StringEdit, StringReplacement};
use je_editor::offset_range::OffsetRange;
use je_editor::text::abstract_text::StringText;
use je_editor::text_model_edit_source::{EditSources, TextModelEditSource};

/// `readonly IObservableDocument[]`.
pub type DocumentList = Rc<[Rc<dyn IObservableDocument>]>;

/// `ObservableWorkspace`.
pub trait ObservableWorkspace {
    /// `documents`.
    fn documents(&self) -> IObservable<DocumentList>;

    /// Os campos da classe base.
    fn state(&self) -> &ObservableWorkspaceState;

    fn get_first_open_document(&self) -> Option<Rc<dyn IObservableDocument>> {
        self.documents().get().first().cloned()
    }

    fn get_document(&self, document_id: &URI) -> Option<Rc<dyn IObservableDocument>> {
        let id = document_id.to_string(false);
        self.documents().get().iter().find(|d| d.uri().to_string(false) == id).cloned()
    }

    /// Is fired when any open document changes.
    fn on_did_open_document_change(&self) -> IObservable<i32> {
        self.state().on_did_open_document_change.clone()
    }

    /// `lastActiveDocument`.
    fn last_active_document(&self) -> IObservable<Option<Rc<dyn IObservableDocument>>> {
        self.state().last_active_document.clone()
    }
}

/// Os campos da classe base `ObservableWorkspace`, derivados do observável `documents`.
pub struct ObservableWorkspaceState {
    on_did_open_document_change: IObservable<i32>,
    last_active_document: IObservable<Option<Rc<dyn IObservableDocument>>>,
}

/// O `changeSummary` de `onDidOpenDocumentChange`.
struct OpenDocumentChangeSummary {
    did_change: Cell<bool>,
}

impl ObservableWorkspaceState {
    pub fn new(documents: IObservable<DocumentList>) -> Self {
        let version = Rc::new(Cell::new(0_i32));

        let documents_any = documents.to_any_observable();
        let tracker = FnChangeTracker::<OpenDocumentChangeSummary>::new(
            |_| OpenDocumentChangeSummary { did_change: Cell::new(false) },
            move |ctx: &dyn IChangeContext, change_summary: &OpenDocumentChangeSummary| {
                if !ctx.did_change(&*documents_any) {
                    change_summary.did_change.set(true); // A document changed
                }
                true
            },
        );
        let docs = documents.clone();
        let on_did_open_document_change = derived_handle_changes::<i32, (), OpenDocumentChangeSummary>(
            DerivedHandleChangesOptions { debug: DebugNameOptions::named("onDidOpenDocumentChange"), change_tracker: tracker, equality_comparer: None },
            move |reader, change_summary| {
                let list = docs.read(reader);
                for d in list.iter() {
                    d.value().read(reader); // add dependency
                }
                if change_summary.did_change.get() {
                    version.set(version.get() + 1); // to force a change
                }
                version.get()
            },
        );

        let last_active_document = derived(move |reader| {
            let obs = observable_value::<Option<Rc<dyn IObservableDocument>>, ()>("lastActiveDocument", None);
            let documents = documents.clone();
            let obs_for_run = obs.clone();
            reader.store().add(autorun(move |reader| {
                let list = documents.read(reader);
                for d in list.iter() {
                    let obs = obs_for_run.clone();
                    let doc = d.clone();
                    reader.store().add(run_on_change::<Rc<StringText>, ()>(d.value(), move |_, _, _| {
                        obs.set(Some(doc.clone()), None);
                    }));
                }
            }));
            obs.to_observable()
        })
        .flatten();

        Self { on_did_open_document_change, last_active_document }
    }
}

/// `IObservableDocument`.
pub trait IObservableDocument {
    fn uri(&self) -> &URI;

    /// O valor do documento; o delta é um [`StringEditWithReason`].
    fn value(&self) -> IObservable<Rc<StringText>>;

    /// Increases whenever the value changes. Is also used to reference document states from the past.
    fn version(&self) -> IObservable<i32>;

    fn language_id(&self) -> IObservable<String>;
}

/// `StringEditWithReason`.
#[derive(Clone, Debug)]
pub struct StringEditWithReason {
    edit: StringEdit,
    pub reason: Rc<TextModelEditSource>,
}

impl StringEditWithReason {
    pub fn new(replacements: Vec<StringReplacement>, reason: Rc<TextModelEditSource>) -> Self {
        Self { edit: StringEdit::new(replacements), reason }
    }

    /// `StringEditWithReason.replace(range, newText, source = EditSources.unknown({}))`.
    pub fn replace(range: OffsetRange, new_text: impl Into<JsString>, source: Option<Rc<TextModelEditSource>>) -> Self {
        Self::new(vec![StringReplacement::new(range, new_text)], source.unwrap_or_else(|| Rc::new(EditSources::unknown(None))))
    }
}

impl Deref for StringEditWithReason {
    type Target = StringEdit;

    fn deref(&self) -> &StringEdit {
        &self.edit
    }
}
