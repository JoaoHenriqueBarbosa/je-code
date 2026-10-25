//! Porte de `src/vs/editor/browser/widget/multiDiffEditor/multiDiffEditorViewModel.ts`: o view model do editor de diff de vários
//! arquivos (`MultiDiffEditorViewModel`: a lista de itens, o foco, o colapso) e o de cada documento
//! (`DocumentDiffItemViewModel`, que guarda o `DiffEditorViewModel` do par original/modificado).
//!
//! O original mora em `editor/browser`, mas usa o `ITextModelService.createSyntheticDocument` (`resolverService.ts`), que o porte
//! guarda no `je-workbench` (o `ITextEditorModel` depende do `IEditorModel` do workbench); por isso este arquivo vive aqui, junto
//! do restante do editor de diff de vários arquivos do workbench, e o modelo (`model.ts`) fica em
//! [`je_editor::multi_diff_editor::model`].
//!
//! Divergências:
//! - **`this` durante a construção.** O `MultiDiffEditorViewModel` é criado por `Rc::new_cyclic`: os itens (`DocumentDiffItemViewModel`)
//!   guardam um `Weak` do editor (o `_editorViewModel`) e o promovem só ao ler `isActive`, `isFirst` ou chamar `setActive`, depois
//!   da construção, como o `this` do original (que já existe, ainda incompleto, quando `_createItem` roda no construtor).
//! - **`RefCounted`.** É o de `je_base::lifecycle` (ver lá): `createNewRef(this)` é `create_new_ref()`, sem o dono de depuração.
//!   `RefCounted<DocumentDiffItemViewModel>` é um [`ItemRef`] (`Rc<RefCounted<Rc<DocumentDiffItemViewModel>>>`).
//! - **`_createItem` (`async`, com `try/finally`).** É um `async_fn` sobre [`create_item_in`], e o `finally` roda depois dela, solta o
//!   `resources` se o item não foi entregue ao `store`. O `ITextModelService` e o `IInstantiationService` entram por
//!   [`Injectable`] (`Deps`), e o `model` é o argumento estático.
//! - **`mapObservableArrayCached`** usa a identidade do `Rc` de cada documento como chave (a chave padrão do original é o próprio
//!   item), e o valor mapeado é a promessa de `_createItem` num `Rc` (a igualdade estrita é a identidade).
//! - **`activeDiffItem`.** É o `DerivedWithWritableCache` (`derivedObservableWithWritableCache`), guardado num `Rc` para o item chamar
//!   `setCache`; o valor é `Option<Rc<DocumentDiffItemViewModel>>` (o `... | undefined` do original).
//! - **`waitForInitialDiffOr1s`.** O `Promise.race` de duas promessas `void` é a corrida de `Promise<Option<()>>` (o `catch` do
//!   original resolve `undefined` no cancelamento), seguida de `then(|_| ())` para a promessa `void` do `ObservablePromise`.
//! - **`getKey()`.** `JSON.stringify([a?.toString(), b?.toString()])` escreve `null` para um lado ausente, como o
//!   `MultiDiffEditorItem::get_key`.
//! - `lastTemplateData` guarda `{ expandedContentHeight, selections }` num [`LastTemplateData`] dentro de um `Rc` (igualdade por
//!   identidade, como o objeto literal novo do original).

use std::cell::Cell;
use std::rc::{Rc, Weak};

use je_base::async_::{async_fn, catch_if_not_canceled, timeout_with_token};
use je_base::cancellation::cancel_on_dispose;
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::Error;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, RefCounted, id_of, to_disposable};
use je_base::observable::observables::observable_value::{ObservableValue, observable_value_owned};
use je_base::observable::utils::utils::DerivedWithWritableCache;
use je_base::observable::{
    DebugOwner, IObservable, ISettableObservable, ITransaction, Observable, ObservableExt, ObservablePromise, ObservableResolvedPromise, Settable, SettableExt, const_observable,
    derived, derived_observable_with_writable_cache, map_observable_array_cached_by, observable_from_value_with_change_event, transaction, wait_for_state,
};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::diff_editor_options::{DiffEditorOptions, spread_options};
use je_editor::diff_editor_view_model::{DiffEditorModel, DiffEditorViewModel, IDiffEditorViewModel};
use je_editor::multi_diff_editor::model::{ContextKeys, DocumentDiffItemRef, DocumentsValue, IDocumentDiffItem, IMultiDiffEditorModel};
use je_editor::multi_diff_editor::view_model::IDocumentDiffItemViewModel;
use je_editor::selection::Selection;
use je_editor::text_model::{TextModel, TextSource};
use je_instantiation::{Injectable, InstantiationService};

use crate::services::textmodel_resolver::resolver_service::{ITextModelService, TextModelReference};

/// `RefCounted<DocumentDiffItemViewModel>`.
pub type ItemRef = Rc<RefCounted<Rc<DocumentDiffItemViewModel>>>;

/// O valor de `_createItem` (a promessa) como o objeto que o `mapObservableArrayCached` guarda.
type PendingItem = Rc<Promise<Option<ItemRef>>>;

/// O que `createInstance(DocumentDiffItemViewModel, ...)` precisa e `_createItem` tem à mão.
struct ItemFactory {
    instantiation_service: Rc<InstantiationService>,
    text_model_service: Rc<dyn ITextModelService>,
    editor_view_model: Weak<MultiDiffEditorViewModel>,
}

pub struct MultiDiffEditorViewModel {
    base: Rc<Disposable>,
    pub model: Rc<dyn IMultiDiffEditorModel>,
    documents: IObservable<DocumentsValue>,
    pub is_loading: IObservable<bool>,
    wait_for_new_diffs: IObservable<Rc<ObservablePromise<Rc<Vec<ItemRef>>>>>,
    pub items: IObservable<Rc<Vec<Rc<DocumentDiffItemViewModel>>>>,
    pub focused_diff_item: IObservable<Option<Rc<DocumentDiffItemViewModel>>>,
    pub active_diff_item: Rc<DerivedWithWritableCache<Option<Rc<DocumentDiffItemViewModel>>>>,
}

impl Injectable for MultiDiffEditorViewModel {
    type Args = Rc<dyn IMultiDiffEditorModel>;
    type Deps = (Rc<InstantiationService>, Rc<dyn ITextModelService>);

    fn create(model: Self::Args, (instantiation_service, text_model_service): Self::Deps) -> Rc<Self> {
        Rc::new_cyclic(|this: &Weak<Self>| {
            let base = Disposable::new();
            let owner = DebugOwner::new("MultiDiffEditorViewModel");
            let documents = observable_from_value_with_change_event(owner.clone(), model.documents());

            let documents_arr = {
                let documents = documents.clone();
                derived(move |reader| match documents.read(reader) {
                    DocumentsValue::Loading => Rc::new(Vec::new()),
                    DocumentsValue::Items(items) => items,
                })
            };

            let factory = Rc::new(ItemFactory { instantiation_service, text_model_service, editor_view_model: this.clone() });
            let all_items: IObservable<Rc<Vec<PendingItem>>> = map_observable_array_cached_by(
                Some(owner.clone()),
                documents_arr,
                move |document: &DocumentDiffItemRef, store| Rc::new(create_item(factory.clone(), document.clone(), store.clone())),
                id_of,
            )
            .recompute_initially_and_on_change(base.store(), None);

            let wait_for_new_diffs: IObservable<Rc<ObservablePromise<Rc<Vec<ItemRef>>>>> = derived(move |reader| {
                let pending = all_items.read(reader);
                ObservablePromise::from_fn(move || {
                    async_fn(async move {
                        let promises: Vec<Promise<Option<ItemRef>>> = pending.iter().map(|p| (**p).clone()).collect();
                        let next: Vec<ItemRef> = Promise::all(promises).await?.into_iter().flatten().collect();
                        let waits: Vec<Promise<()>> = next.iter().map(|i| i.object().wait_for_initial_diff_or_1s.promise.clone()).collect();
                        Promise::all(waits).await?;
                        Ok(Rc::new(next))
                    })
                })
            });

            let resolved = ObservableResolvedPromise::new(wait_for_new_diffs.clone(), Rc::new(Vec::new()), base.store());

            let items: IObservable<Rc<Vec<Rc<DocumentDiffItemViewModel>>>> = {
                let last_resolved = resolved.last_resolved.clone();
                derived(move |reader| {
                    let resolved_items = last_resolved.read(reader);
                    Rc::new(
                        resolved_items
                            .iter()
                            .map(|i| {
                                let reference = i.create_new_ref();
                                reader.store().add(reference.clone());
                                reference.object().clone()
                            })
                            .collect::<Vec<_>>(),
                    )
                })
            };

            let is_loading = {
                let (documents, is_resolving) = (documents.clone(), resolved.is_resolving.clone());
                derived(move |reader| matches!(documents.read(reader), DocumentsValue::Loading) || is_resolving.read(reader))
            };

            let focused_diff_item = {
                let items = items.clone();
                derived(move |reader| items.read(reader).iter().find(|i| i.is_focused.read(reader)).cloned())
            };
            let active_diff_item = {
                let (focused_diff_item, items) = (focused_diff_item.clone(), items.clone());
                Rc::new(derived_observable_with_writable_cache::<Option<Rc<DocumentDiffItemViewModel>>>(move |reader, last_value| {
                    focused_diff_item.read(reader).or_else(|| {
                        let last_value = last_value.and_then(|last| last.clone())?;
                        items.read(reader).iter().any(|i| Rc::ptr_eq(i, &last_value)).then_some(last_value)
                    })
                }))
            };

            Self { base, model, documents, is_loading, wait_for_new_diffs, items, focused_diff_item, active_diff_item }
        })
    }
}

/// `_createItem(document, store)`: a promessa do item (ou `undefined` se o `store` foi descartado no meio do caminho).
fn create_item(factory: Rc<ItemFactory>, document: DocumentDiffItemRef, store: Rc<DisposableStore>) -> Promise<Option<ItemRef>> {
    async_fn(async move {
        let resources = DisposableStore::new();
        let transferred = Cell::new(false);
        let result = create_item_in(&factory, &document, &store, &resources, &transferred).await;
        if !transferred.get() {
            resources.dispose();
        }
        result
    })
}

/// O corpo do `try` de `_createItem`; `transferred` indica que o `resources` passou a pertencer ao item.
async fn create_item_in(
    factory: &ItemFactory,
    document: &DocumentDiffItemRef,
    store: &Rc<DisposableStore>,
    resources: &Rc<DisposableStore>,
    transferred: &Cell<bool>,
) -> Result<Option<ItemRef>, Error> {
    let document_reference = document.create_new_ref();
    resources.add(document_reference.clone());

    let original = match document.object().original().and_then(|source| source.text_model.clone()) {
        Some(text_model) => text_model,
        None => synthetic_text_model(factory, resources).await?,
    };
    if store.is_disposed() {
        return Ok(None);
    }
    let modified = match document.object().modified().and_then(|source| source.text_model.clone()) {
        Some(text_model) => text_model,
        None => synthetic_text_model(factory, resources).await?,
    };
    if store.is_disposed() {
        return Ok(None);
    }
    let item = factory.instantiation_service.create_instance::<DocumentDiffItemViewModel>(DocumentDiffItemViewModelArgs {
        document_diff_item_ref: document_reference,
        editor_view_model: factory.editor_view_model.clone(),
        original_text_model: original,
        modified_text_model: modified,
        diff_editor_view_model_store: resources.clone(),
    });
    let reference = RefCounted::create(item);
    store.add(reference.clone());
    transferred.set(true);
    Ok(Some(reference))
}

/// `resources.add(await this._textModelService.createSyntheticDocument('', null)).object.textEditorModel`.
async fn synthetic_text_model(factory: &ItemFactory, resources: &Rc<DisposableStore>) -> Result<Rc<TextModel>, Error> {
    let reference: TextModelReference = factory.text_model_service.create_synthetic_document(TextSource::from(""), None).await?;
    let text_model = reference.object.text_editor_model();
    resources.add(Rc::new(reference));
    text_model.ok_or_else(|| Error::new("o documento sintético não tem modelo de texto"))
}

impl MultiDiffEditorViewModel {
    /// `waitForDiffOr1s()`.
    pub fn wait_for_diff_or_1s(&self) -> Promise<()> {
        let documents = self.documents.clone();
        let wait_for_new_diffs = self.wait_for_new_diffs.clone();
        async_fn(async move {
            if matches!(documents.get(), DocumentsValue::Loading) {
                wait_for_state(documents.clone(), |documents| !matches!(documents, DocumentsValue::Loading), None, None).await?;
            }

            wait_for_new_diffs.get().promise.clone().await?;
            Ok(())
        })
    }

    /// Dá o mesmo valor de colapso a todos os itens, numa transação só.
    fn set_collapsed_for_all(&self, collapsed: bool) {
        transaction(|tx| {
            for d in self.items.get().iter() {
                d.collapsed.set(collapsed, Some(tx));
            }
        });
    }

    pub fn collapse_all(&self) {
        self.set_collapsed_for_all(true);
    }

    pub fn expand_all(&self) {
        self.set_collapsed_for_all(false);
    }

    pub fn collapse(&self, item: &DocumentDiffItemViewModel) {
        transaction(|tx| item.collapsed.set(true, Some(tx)));
    }

    pub fn expand(&self, item: &DocumentDiffItemViewModel) {
        transaction(|tx| item.collapsed.set(false, Some(tx)));
    }

    /// `get contextKeys()`.
    pub fn context_keys(&self) -> Option<ContextKeys> {
        self.model.context_keys()
    }
}

impl IDisposable for MultiDiffEditorViewModel {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `{ expandedContentHeight: number; selections: Selection[] | undefined }`.
#[derive(Clone)]
pub struct LastTemplateData {
    pub expanded_content_height: f64,
    pub selections: Option<Vec<Selection>>,
}

/// Os argumentos estáticos do construtor de `DocumentDiffItemViewModel` (os que antecedem o `@IInstantiationService`).
pub struct DocumentDiffItemViewModelArgs {
    pub document_diff_item_ref: DocumentDiffItemRef,
    pub editor_view_model: Weak<MultiDiffEditorViewModel>,
    pub original_text_model: Rc<TextModel>,
    pub modified_text_model: Rc<TextModel>,
    pub diff_editor_view_model_store: Rc<DisposableStore>,
}

pub struct DocumentDiffItemViewModel {
    base: Rc<Disposable>,
    /// The diff editor view model keeps its inner objects alive.
    pub diff_editor_view_model_ref: Rc<RefCounted<Rc<DiffEditorViewModel>>>,
    pub wait_for_initial_diff_or_1s: Rc<ObservablePromise<()>>,
    pub collapsed: Rc<ObservableValue<bool, ()>>,
    pub last_template_data: Rc<ObservableValue<Rc<LastTemplateData>, ()>>,
    pub is_active: IObservable<bool>,
    pub is_first: IObservable<bool>,
    is_focused_source: Rc<ObservableValue<IObservable<bool>, ()>>,
    pub is_focused: IObservable<bool>,
    document_diff_item_ref: DocumentDiffItemRef,
    editor_view_model: Weak<MultiDiffEditorViewModel>,
    pub is_alive: Rc<ObservableValue<bool, ()>>,
    me: Weak<Self>,
}

/// `updateOptions(options)`: as opções do item, com as regiões sem mudanças sempre escondidas.
fn update_options(options: &Dynamic) -> Dynamic {
    spread_options(options, &Dynamic::object([("hideUnchangedRegions", Dynamic::object([("enabled", Dynamic::Bool(true))]))]))
}

/// `this.documentDiffItem.options || {}`.
fn item_options(item: &dyn IDocumentDiffItem) -> Dynamic {
    item.options().unwrap_or_else(|| Dynamic::object(Vec::<(&str, Dynamic)>::new()))
}

impl Injectable for DocumentDiffItemViewModel {
    type Args = DocumentDiffItemViewModelArgs;
    type Deps = (Rc<InstantiationService>,);

    fn create(args: Self::Args, (instantiation_service,): Self::Deps) -> Rc<Self> {
        let DocumentDiffItemViewModelArgs { document_diff_item_ref, editor_view_model, original_text_model, modified_text_model, diff_editor_view_model_store } = args;
        Rc::new_cyclic(|me: &Weak<Self>| {
            let base = Disposable::new();
            let owner = DebugOwner::new("DocumentDiffItemViewModel");

            let is_alive = observable_value_owned::<bool, ()>(owner.clone(), true);
            {
                let is_alive = is_alive.clone();
                base.register(to_disposable(move || is_alive.set(false, None)));
            }

            let document_diff_item: Rc<dyn IDocumentDiffItem> = document_diff_item_ref.object().clone();
            let options = instantiation_service.create_instance::<DiffEditorOptions>(update_options(&item_options(&*document_diff_item)));
            if let Some(on_options_did_change) = document_diff_item.on_options_did_change() {
                let (options, document_diff_item) = (options.clone(), document_diff_item.clone());
                base.register(on_options_did_change.listen(move |_| options.update_options(&update_options(&item_options(&*document_diff_item)))));
            }

            let diff_editor_view_model = instantiation_service
                .create_instance::<DiffEditorViewModel>((DiffEditorModel { original: original_text_model, modified: modified_text_model }, options));
            let diff_editor_view_model_ref = RefCounted::create_with_disposable(diff_editor_view_model, diff_editor_view_model_store);
            base.register(diff_editor_view_model_ref.clone());

            let wait_for_diff = diff_editor_view_model_ref.object().wait_for_diff();
            let timeout_1s = timeout_with_token(1000, &cancel_on_dispose(base.store()));
            let wait_for_initial_diff_or_1s = ObservablePromise::new(
                Promise::race(vec![catch_if_not_canceled(&wait_for_diff), catch_if_not_canceled(&timeout_1s)]).then(|_| ()),
            );

            let is_active = {
                let (editor_view_model, me) = (editor_view_model.clone(), me.clone());
                derived(move |reader| {
                    let Some(editor) = editor_view_model.upgrade() else { return false };
                    editor.active_diff_item.observable.read(reader).is_some_and(|active| std::ptr::eq(Rc::as_ptr(&active), me.as_ptr()))
                })
            };
            let is_first = {
                let (editor_view_model, me) = (editor_view_model.clone(), me.clone());
                derived(move |reader| {
                    let Some(editor) = editor_view_model.upgrade() else { return false };
                    editor.items.read(reader).first().is_some_and(|first| std::ptr::eq(Rc::as_ptr(first), me.as_ptr()))
                })
            };

            let is_focused_source = observable_value_owned::<IObservable<bool>, ()>(owner.clone(), const_observable(false));
            let is_focused = {
                let is_focused_source = is_focused_source.clone();
                derived(move |reader| is_focused_source.read(reader).read(reader))
            };

            Self {
                base,
                diff_editor_view_model_ref,
                wait_for_initial_diff_or_1s,
                collapsed: observable_value_owned::<bool, ()>(owner.clone(), false),
                last_template_data: observable_value_owned::<Rc<LastTemplateData>, ()>(
                    owner,
                    Rc::new(LastTemplateData { expanded_content_height: 500.0, selections: None }),
                ),
                is_active,
                is_first,
                is_focused_source,
                is_focused,
                document_diff_item_ref,
                editor_view_model,
                is_alive,
                me: me.clone(),
            }
        })
    }
}

impl DocumentDiffItemViewModel {
    pub fn diff_editor_view_model(&self) -> &Rc<DiffEditorViewModel> {
        self.diff_editor_view_model_ref.object()
    }

    pub fn original_uri(&self) -> Option<URI> {
        self.document_diff_item().original().map(|source| source.uri.clone())
    }

    pub fn modified_uri(&self) -> Option<URI> {
        self.document_diff_item().modified().map(|source| source.uri.clone())
    }

    pub fn is_binary(&self) -> bool {
        let item = self.document_diff_item();
        item.original().is_some_and(|original| original.text_model.is_none()) || item.modified().is_some_and(|modified| modified.text_model.is_none())
    }

    pub fn set_active(&self, tx: Option<&dyn ITransaction>) {
        if let (Some(editor), Some(this)) = (self.editor_view_model.upgrade(), self.me.upgrade()) {
            editor.active_diff_item.set_cache(Some(Some(this)), tx);
        }
    }

    pub fn set_is_focused(&self, source: IObservable<bool>, tx: Option<&dyn ITransaction>) {
        self.is_focused_source.set(source, tx);
    }

    pub fn document_diff_item(&self) -> &Rc<dyn IDocumentDiffItem> {
        self.document_diff_item_ref.object()
    }

    pub fn get_key(&self) -> String {
        let to_dynamic = |uri: Option<URI>| uri.map_or(Dynamic::Null, |uri| Dynamic::string(&uri.to_string(false).to_string_lossy()));
        json_stringify(&Dynamic::array([to_dynamic(self.original_uri()), to_dynamic(self.modified_uri())])).unwrap_or_default()
    }
}

impl IDisposable for DocumentDiffItemViewModel {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// O recorte que o `DiffEditorItemTemplate` do `je-editor` lê do item (ver `je_editor::multi_diff_editor::view_model`). Os
/// métodos inherentes acima continuam sendo a fonte, porque são chamados por todo o workbench sem o trait em escopo; este
/// bloco só os entrega ao trait do `je-editor`, que não enxerga esta struct.
impl IDocumentDiffItemViewModel for DocumentDiffItemViewModel {
    fn document_diff_item(&self) -> &Rc<dyn IDocumentDiffItem> {
        DocumentDiffItemViewModel::document_diff_item(self)
    }

    fn diff_editor_view_model(&self) -> &Rc<DiffEditorViewModel> {
        DocumentDiffItemViewModel::diff_editor_view_model(self)
    }

    fn diff_editor_view_model_ref(&self) -> &Rc<RefCounted<Rc<DiffEditorViewModel>>> {
        &self.diff_editor_view_model_ref
    }

    fn original_uri(&self) -> Option<URI> {
        DocumentDiffItemViewModel::original_uri(self)
    }

    fn modified_uri(&self) -> Option<URI> {
        DocumentDiffItemViewModel::modified_uri(self)
    }

    fn is_binary(&self) -> bool {
        DocumentDiffItemViewModel::is_binary(self)
    }

    fn collapsed(&self) -> ISettableObservable<bool> {
        self.collapsed.to_settable()
    }

    fn is_active(&self) -> IObservable<bool> {
        self.is_active.clone()
    }

    fn is_first(&self) -> IObservable<bool> {
        self.is_first.clone()
    }

    fn expanded_content_height(&self) -> f64 {
        self.last_template_data.get().expanded_content_height
    }

    fn set_active(&self, tx: Option<&dyn ITransaction>) {
        DocumentDiffItemViewModel::set_active(self, tx);
    }
}
