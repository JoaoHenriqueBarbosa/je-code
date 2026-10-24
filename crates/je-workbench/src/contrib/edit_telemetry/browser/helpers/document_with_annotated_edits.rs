//! Porte de `src/vs/workbench/contrib/editTelemetry/browser/helpers/documentWithAnnotatedEdits.ts`.
//!
//! Representação:
//! - `IObservableWithChange<StringText, { edit: AnnotatedStringEdit<T> }>` é um `IObservable<Rc<StringText>>` cujo
//!   delta é um [`AnnotatedEdit<T>`] (ver `observable_workspace` para a razão);
//! - `EditSourceBase._cache` (um `CachedFunction` por `toString()`, que devolve sempre o mesmo objeto) existe só para
//!   a identidade (`this.source !== data.source`); como o `toString()` de cada origem cobre todos os seus campos,
//!   a igualdade por valor de [`EditSource`] é a mesma relação e o cache não é necessário;
//! - a identidade de `editSource` em `EditSourceData.join` é a do `Rc<TextModelEditSource>`;
//! - `TEditData extends (EditKeySourceData | EditSourceData)` é o trait [`EditDataWithSource`];
//! - o `@IInstantiationService` do `CombineStreamedChanges` e o `@IEditorWorkerService` do `DiffService` são
//!   dependências do [`Injectable`].

use std::cell::RefCell;
use std::fmt;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::{AsyncIterable, AsyncIterator, AsyncRead, AsyncReader, async_fn};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::{IObservable, Observable, Settable, observable_value, run_on_change};
use je_base::promise::Promise;
use je_editor::edits::edit::BaseEdit;
use je_editor::edits::string_edit::{AnnotatedStringEdit, BaseStringEdit, IEditData, StringEdit};
use je_editor::editor_worker::{DiffAlgorithmName, IEditorWorkerService};
use je_editor::text::abstract_text::StringText;
use je_editor::text_model_edit_source::TextModelEditSource;
use je_instantiation::{Injectable, InstantiationService};

use super::observable_workspace::{IObservableDocument, StringEditWithReason};
use super::utils::{ObservableChange, iterate_observable_changes, map_observable_delta};

/// O delta `{ edit: AnnotatedStringEdit<TEditData> }` dos documentos com edições anotadas.
#[derive(Clone, Debug)]
pub struct AnnotatedEdit<D: IEditData> {
    pub edit: AnnotatedStringEdit<D>,
}

impl<D: IEditData> AnnotatedEdit<D> {
    /// `AnnotatedStringEdit.compose(changes.map(c => c.edit))`.
    pub fn compose_all(changes: &[AnnotatedEdit<D>]) -> AnnotatedStringEdit<D> {
        AnnotatedStringEdit::compose_all(&changes.iter().map(|c| c.edit.clone()).collect::<Vec<_>>())
    }
}

/// `IDocumentWithAnnotatedEdits<TEditData>`: `value` tem como delta um [`AnnotatedEdit<TEditData>`].
pub trait IDocumentWithAnnotatedEdits<D: IEditData = EditKeySourceData> {
    fn value(&self) -> IObservable<Rc<StringText>>;

    fn wait_for_queue(&self) -> Promise<()>;
}

/// `TEditData extends (EditKeySourceData | EditSourceData)`: o dado de edição que carrega um [`EditSource`].
pub trait EditDataWithSource: IEditData + 'static {
    fn source(&self) -> &EditSource;
}

/// Creates a document that is a delayed copy of the original document,
/// but with edits annotated with the source of the edit.
pub struct DocumentWithSourceAnnotatedEdits {
    base: Rc<Disposable>,
    value: Rc<ObservableValue<Rc<StringText>, AnnotatedEdit<EditSourceData>>>,
}

impl DocumentWithSourceAnnotatedEdits {
    pub fn new(original_doc: &Rc<dyn IObservableDocument>) -> Rc<Self> {
        let base = Disposable::new();

        let v = observable_value::<Rc<StringText>, AnnotatedEdit<EditSourceData>>("value", original_doc.value().get());
        let target = v.clone();

        base.register(run_on_change::<Rc<StringText>, StringEditWithReason>(original_doc.value(), move |val, _prev_val, edits| {
            let composed = AnnotatedStringEdit::compose_all(
                &edits
                    .iter()
                    .map(|e| {
                        let edit_source_data = EditSourceData::new(e.reason.clone());
                        e.map_data(|_| edit_source_data.clone())
                    })
                    .collect::<Vec<_>>(),
            );

            target.set_with_change(val.clone(), None, AnnotatedEdit { edit: composed });
        }));

        Rc::new(Self { base, value: v })
    }
}

impl IDocumentWithAnnotatedEdits<EditSourceData> for DocumentWithSourceAnnotatedEdits {
    fn value(&self) -> IObservable<Rc<StringText>> {
        self.value.to_observable()
    }

    fn wait_for_queue(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

impl IDisposable for DocumentWithSourceAnnotatedEdits {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// Only joins touching edits if the source and the metadata is the same (e.g. requestUuids must be equal).
#[derive(Clone, Debug, PartialEq)]
pub struct EditSourceData {
    pub edit_source: Rc<TextModelEditSource>,
    pub source: EditSource,
    pub key: String,
}

impl EditSourceData {
    pub fn new(edit_source: Rc<TextModelEditSource>) -> Self {
        let key = edit_source.to_key(1, &[]);
        let source = EditSourceBase::create(&edit_source);
        Self { edit_source, source, key }
    }

    pub fn to_edit_source_data(&self) -> EditKeySourceData {
        EditKeySourceData::new(self.key.clone(), self.source.clone(), self.edit_source.clone())
    }
}

impl IEditData for EditSourceData {
    fn join(&self, data: &EditSourceData) -> Option<EditSourceData> {
        if !Rc::ptr_eq(&self.edit_source, &data.edit_source) {
            return None;
        }
        Some(self.clone())
    }
}

impl EditDataWithSource for EditSourceData {
    fn source(&self) -> &EditSource {
        &self.source
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EditKeySourceData {
    pub key: String,
    pub source: EditSource,
    pub representative: Rc<TextModelEditSource>,
}

impl EditKeySourceData {
    pub fn new(key: String, source: EditSource, representative: Rc<TextModelEditSource>) -> Self {
        Self { key, source, representative }
    }
}

impl IEditData for EditKeySourceData {
    fn join(&self, data: &EditKeySourceData) -> Option<EditKeySourceData> {
        if self.key != data.key {
            return None;
        }
        if self.source != data.source {
            return None;
        }
        // The representatives could be different! (But equal modulo key)
        Some(self.clone())
    }
}

impl EditDataWithSource for EditKeySourceData {
    fn source(&self) -> &EditSource {
        &self.source
    }
}

/// `EditSourceBase`: a fábrica de [`EditSource`] a partir da origem da edição do modelo.
pub struct EditSourceBase;

impl EditSourceBase {
    pub fn create(reason: &TextModelEditSource) -> EditSource {
        match reason.source.as_str() {
            "reloadFromDisk" => EditSource::External,
            "inlineCompletionPartialAccept" | "inlineCompletionAccept" => {
                let edit_type = reason.metadata_value("type").map(str::to_owned);
                let kind = if is_truthy(reason.metadata_value("$nes")) { InlineSuggestKind::Nes } else { InlineSuggestKind::Completion };
                EditSource::InlineSuggest(InlineSuggestEditSource {
                    kind,
                    extension_id: reason.metadata_value("$extensionId").unwrap_or_default().to_owned(),
                    provider_id: reason.metadata_value("$providerId").unwrap_or_default().to_owned(),
                    edit_type,
                })
            }
            "snippet" => EditSource::Ide { feature: "suggest".to_owned() },
            "unknown" => match reason.metadata_value("name") {
                Some("formatEditsCommand") => EditSource::Ide { feature: "format".to_owned() },
                _ => EditSource::Unknown,
            },
            "Chat.applyEdits" => {
                if reason.metadata_value("$origin") == Some("agentHost") {
                    EditSource::AgentHost
                } else {
                    EditSource::Chat { kind: ChatEditKind::Sidebar }
                }
            }
            "inlineChat.applyEdits" => EditSource::Chat { kind: ChatEditKind::Inline },
            "cursor" => EditSource::User,
            _ => EditSource::Unknown,
        }
    }
}

/// A veracidade (truthiness) de um valor de `metadata`: `undefined`, `''` e `false` são falsos.
fn is_truthy(value: Option<&str>) -> bool {
    !matches!(value, None | Some("") | Some("false"))
}

/// `'completion' | 'nes'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineSuggestKind {
    Completion,
    Nes,
}

impl InlineSuggestKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Completion => "completion",
            Self::Nes => "nes",
        }
    }
}

/// `InlineSuggestEditSource`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineSuggestEditSource {
    pub kind: InlineSuggestKind,
    pub extension_id: String,
    pub provider_id: String,
    /// `type: 'word' | 'line' | undefined`.
    pub edit_type: Option<String>,
}

/// `'sidebar' | 'inline'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatEditKind {
    Sidebar,
    Inline,
}

/// `EditSource`: `InlineSuggestEditSource | ChatEditSource | AgentHostEditSource | IdeEditSource | UserEditSource |
/// UnknownEditSource | ExternalEditSource`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditSource {
    InlineSuggest(InlineSuggestEditSource),
    Chat { kind: ChatEditKind },
    AgentHost,
    /// `feature: 'suggest' | 'format' | string`.
    Ide { feature: String },
    User,
    Unknown,
    /// Caused by external tools that trigger a reload from disk.
    External,
}

impl EditSource {
    /// `category`.
    pub fn category(&self) -> &'static str {
        match self {
            Self::InlineSuggest(_) | Self::Chat { .. } => "ai",
            Self::AgentHost => "agentHost",
            Self::Ide { .. } => "ide",
            Self::User => "user",
            Self::Unknown => "unknown",
            Self::External => "external",
        }
    }

    /// `feature`: só as origens de IA, do agente e da IDE têm.
    pub fn feature(&self) -> Option<&str> {
        match self {
            Self::InlineSuggest(_) => Some("inlineSuggest"),
            Self::Chat { .. } | Self::AgentHost => Some("chat"),
            Self::Ide { feature } => Some(feature.as_str()),
            Self::User | Self::Unknown | Self::External => None,
        }
    }

    /// `getColor()`.
    pub fn get_color(&self) -> &'static str {
        match self {
            Self::InlineSuggest(_) => "#00ff0033",
            Self::Chat { .. } | Self::AgentHost => "#00ff0066",
            Self::Ide { feature } => {
                if feature == "format" {
                    "#0000ff33"
                } else {
                    "#80808033"
                }
            }
            Self::User => "#d3d3d333",
            Self::External => "#009ab254",
            Self::Unknown => "#ff000033",
        }
    }
}

/// `toString()`.
impl fmt::Display for EditSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InlineSuggest(s) => write!(
                f,
                "{}/inlineSuggest/{}/{}/{}/{}",
                self.category(),
                s.kind.as_str(),
                s.extension_id,
                s.provider_id,
                s.edit_type.as_deref().unwrap_or("undefined")
            ),
            Self::Chat { kind } => write!(f, "{}/chat/{}", self.category(), if *kind == ChatEditKind::Sidebar { "sidebar" } else { "inline" }),
            Self::AgentHost => write!(f, "{}/chat", self.category()),
            Self::Ide { feature } => write!(f, "{}/{}", self.category(), feature),
            Self::User | Self::Unknown | Self::External => f.write_str(self.category()),
        }
    }
}

type StreamedChange<D> = ObservableChange<Rc<StringText>, AnnotatedEdit<D>>;

pub struct CombineStreamedChanges<D: EditDataWithSource> {
    me: Weak<Self>,
    base: Rc<Disposable>,
    value: Rc<ObservableValue<Rc<StringText>, AnnotatedEdit<D>>>,
    run_store: Rc<DisposableStore>,
    run_queue: RefCell<Promise<()>>,
    diff_service: Rc<DiffService>,
    original_doc: Rc<dyn IDocumentWithAnnotatedEdits<D>>,
}

impl<D: EditDataWithSource> Injectable for CombineStreamedChanges<D> {
    type Args = Rc<dyn IDocumentWithAnnotatedEdits<D>>;
    type Deps = (Rc<InstantiationService>,);

    fn create(original_doc: Self::Args, (instantiation_service,): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();

        let diff_service = instantiation_service.create_instance::<DiffService>(());
        let value = observable_value::<Rc<StringText>, AnnotatedEdit<D>>("value", original_doc.value().get());
        let run_store = DisposableStore::new();
        base.register(run_store.clone());

        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            value,
            run_store,
            run_queue: RefCell::new(Promise::resolved(())),
            diff_service,
            original_doc,
        });
        this.restart();
        this
    }
}

impl<D: EditDataWithSource> CombineStreamedChanges<D> {
    /// `_restart()`: o resultado só importa a quem espera a fila ([`IDocumentWithAnnotatedEdits::wait_for_queue`]).
    pub fn restart(&self) -> Promise<()> {
        let this = self.me.upgrade().expect("o objeto está vivo");
        async_fn(async move {
            this.run_store.clear();
            let iterator = iterate_observable_changes::<Rc<StringText>, AnnotatedEdit<D>>(&this.original_doc.value(), &this.run_store).async_iterator();
            let p = this.run_queue.borrow().clone();
            let runner = this.clone();
            let queued = p.then_promise(move |_| runner.run(iterator));
            *this.run_queue.borrow_mut() = queued;
            p.await?;
            Ok(())
        })
    }

    fn run(&self, iterator: Rc<dyn AsyncIterator<StreamedChange<D>>>) -> Promise<()> {
        let this = self.me.upgrade().expect("o objeto está vivo");
        async_fn(async move {
            let reader = AsyncReader::new(iterator);
            loop {
                let mut peeked = match reader.peek().await? {
                    AsyncRead::EndOfStream => return Ok(()),
                    AsyncRead::Item(item) => item,
                };
                if is_chat_edit(&peeked) {
                    let first = peeked.clone();

                    let last;
                    let mut chat_edit = AnnotatedStringEdit::<D>::empty();

                    loop {
                        reader.read_buffered_or_throw();
                        let current = peeked.clone();
                        chat_edit = chat_edit.compose(&AnnotatedEdit::compose_all(&current.change));
                        let next = match reader.peek_timeout(1000).await? {
                            Some(AsyncRead::Item(next)) if is_chat_edit(&next) => next,
                            _ => {
                                last = current;
                                break;
                            }
                        };
                        peeked = next;
                    }

                    if !chat_edit.is_empty() {
                        let data = chat_edit.replacements()[0].data.clone();
                        let diff_edit = this.diff_service.compute_diff(&first.prev_value.value, &last.value.value).await?;
                        let edit = diff_edit.map_data(|_e| data.clone());
                        this.value.set_with_change(last.value.clone(), None, AnnotatedEdit { edit });
                    }
                } else {
                    reader.read_buffered_or_throw();
                    let e = AnnotatedEdit::compose_all(&peeked.change);
                    this.value.set_with_change(peeked.value.clone(), None, AnnotatedEdit { edit: e });
                }
            }
        })
    }
}

impl<D: EditDataWithSource> IDocumentWithAnnotatedEdits<D> for CombineStreamedChanges<D> {
    fn value(&self) -> IObservable<Rc<StringText>> {
        self.value.to_observable()
    }

    fn wait_for_queue(&self) -> Promise<()> {
        let this = self.me.upgrade().expect("o objeto está vivo");
        async_fn(async move {
            this.original_doc.wait_for_queue().await?;
            this.restart().await?;
            Ok(())
        })
    }
}

impl<D: EditDataWithSource> IDisposable for CombineStreamedChanges<D> {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `DiffService`.
pub struct DiffService {
    editor_worker_service: Rc<dyn IEditorWorkerService>,
}

impl Injectable for DiffService {
    type Args = ();
    type Deps = (Rc<dyn IEditorWorkerService>,);

    fn create(_: (), (editor_worker_service,): Self::Deps) -> Rc<Self> {
        Rc::new(Self { editor_worker_service })
    }
}

impl DiffService {
    pub fn compute_diff(&self, original: &JsString, modified: &JsString) -> Promise<StringEdit> {
        self.editor_worker_service.compute_string_edit_from_diff(original, modified, 500.0, DiffAlgorithmName::Advanced)
    }
}

fn is_chat_edit<D: EditDataWithSource>(next: &StreamedChange<D>) -> bool {
    next.change.iter().all(|c| {
        c.edit.replacements().iter().all(|e| {
            let source = e.data.source();
            source.category() == "ai" && source.feature() == Some("chat")
        })
    })
}

pub struct MinimizeEditsProcessor<D: IEditData + 'static> {
    base: Rc<Disposable>,
    original_doc: Rc<dyn IDocumentWithAnnotatedEdits<D>>,
    value: Rc<ObservableValue<Rc<StringText>, AnnotatedEdit<D>>>,
}

impl<D: IEditData + 'static> MinimizeEditsProcessor<D> {
    pub fn new(original_doc: Rc<dyn IDocumentWithAnnotatedEdits<D>>) -> Rc<Self> {
        let base = Disposable::new();

        let v = observable_value::<Rc<StringText>, AnnotatedEdit<D>>("value", original_doc.value().get());
        let target = v.clone();

        let prev_value = RefCell::new(original_doc.value().get().value.clone());
        base.register(run_on_change::<Rc<StringText>, AnnotatedEdit<D>>(original_doc.value(), move |val, _prev_val, edits| {
            let composed = AnnotatedEdit::compose_all(edits);

            let e = composed.remove_common_suffix_and_prefix(prev_value.borrow().units());
            *prev_value.borrow_mut() = val.value.clone();

            target.set_with_change(val.clone(), None, AnnotatedEdit { edit: e });
        }));

        Rc::new(Self { base, original_doc, value: v })
    }
}

impl<D: IEditData + 'static> IDocumentWithAnnotatedEdits<D> for MinimizeEditsProcessor<D> {
    fn value(&self) -> IObservable<Rc<StringText>> {
        self.value.to_observable()
    }

    fn wait_for_queue(&self) -> Promise<()> {
        self.original_doc.wait_for_queue()
    }
}

impl<D: IEditData + 'static> IDisposable for MinimizeEditsProcessor<D> {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// Removing the metadata allows touching edits from the same source to merged, even if they were caused by different actions (e.g. two user edits).
pub fn create_doc_with_just_reason(
    doc_with_annotated_edits: &Rc<dyn IDocumentWithAnnotatedEdits<EditSourceData>>,
    store: &DisposableStore,
) -> Rc<dyn IDocumentWithAnnotatedEdits<EditKeySourceData>> {
    Rc::new(DocWithJustReason {
        value: map_observable_delta(
            &doc_with_annotated_edits.value(),
            |edit: &AnnotatedEdit<EditSourceData>| AnnotatedEdit { edit: edit.edit.map_data(|d| d.data.to_edit_source_data()) },
            store,
        ),
        original: doc_with_annotated_edits.clone(),
    })
}

/// O objeto literal `docWithJustReason` de `createDocWithJustReason`.
struct DocWithJustReason {
    value: IObservable<Rc<StringText>>,
    original: Rc<dyn IDocumentWithAnnotatedEdits<EditSourceData>>,
}

impl IDocumentWithAnnotatedEdits<EditKeySourceData> for DocWithJustReason {
    fn value(&self) -> IObservable<Rc<StringText>> {
        self.value.clone()
    }

    fn wait_for_queue(&self) -> Promise<()> {
        self.original.wait_for_queue()
    }
}
