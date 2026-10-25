//! Porte de `src/vs/workbench/contrib/multiDiffEditor/browser/multiDiffEditorInput.ts`: a entrada do editor de diff de vários
//! arquivos (`MultiDiffEditorInput`), o modelo que ela resolve para o view model, o item de documento com o `MultiDiffEditorItem`
//! (`IDocumentDiffItemWithMultiDiffEditorItem`), o serializador (`MultiDiffEditorSerializer`) e a contribuição que registra o editor
//! no resolvedor de editores (`MultiDiffEditorResolverContribution`).
//!
//! Divergências:
//! - **Herança.** `MultiDiffEditorInput extends EditorInput` guarda o [`EditorInputBase`] e implementa o trait [`EditorInput`]; o que o
//!   original declara como campo `override readonly` (`onDidChangeDirty`, `closeHandler`) é o método de mesmo nome do trait.
//!   `implements ILanguageSupport` é o [`ILanguageSupport`].
//! - **Injeção.** Os argumentos estáticos do construtor (`multiDiffSource`, `label`, `initialResources`, `isTransient`) são
//!   [`Injectable::Args`], e os serviços decorados são a tupla `Deps`, na ordem do original. `initialResources` fica guardado num
//!   `Rc` (a lista é imutável e a identidade do `Rc` faz o papel da do array), mas entra como `Vec`.
//! - **`this` no construtor.** O `Rc::new_cyclic` só dá o `Weak` da entrada depois que o `Rc` existe, e o `autorun` que atualiza o
//!   nome roda na hora (no construtor, `this` já existe, incompleto). Por isso o `autorun` é registrado logo depois do
//!   `new_cyclic`, com o `Weak`, e roda uma vez em seguida, como no original. O `LazyStatefulPromise` do view model guarda o `Weak`
//!   e promove só ao ler `_store.isDisposed` e ao registrar o `store` (uma entrada já liberada vale como descartada).
//! - **`multiDiffSource` sem fonte.** `new Date().getMilliseconds().toString() + Math.random().toString()` (um identificador
//!   qualquer, único) é o `generate_uuid` de `je_base::uuid`, que o porte já usa para identificadores únicos.
//! - **`mapObservableArrayCached` assíncrono.** O mapeamento do original devolve `Promise<RefCounted | undefined>`; aqui o valor
//!   mapeado é a promessa num `Rc` (a igualdade estrita é a identidade), como no `MultiDiffEditorViewModel`. A chave padrão é o
//!   `getKey()` do item (o `JSON.stringify([modifiedUri?.toString(), originalUri?.toString()])`).
//! - **`derived(async reader => ...)`.** O valor derivado é a promessa num `Rc`; a parte do corpo antes do primeiro `await` (as
//!   leituras do `reader`) roda na hora, e o resto roda numa função `async` que nunca usa o leitor, como no original.
//! - **`Promise.allSettled`, `console.error`.** São `Promise::all_settled` e `eprintln!` seguido do `onUnexpectedError`.
//! - **`observableFromValueWithChangeEvent` de `ValueWithChangeEventFromObservable`.** O atalho do original (usar o observável de
//!   dentro) não existe sobre um trait object (ver `je_base::observable::utils::value_with_change_event`); o observável passa pelo
//!   evento, o que dá a mesma leitura.
//! - **`_createModel` com erro.** O original não solta a inscrição do `recomputeInitiallyAndOnChange` se o `await` do
//!   `updateDocuments` falha; aqui ela é solta (o observador não fica preso).
//! - **`FastEventDispatcher`.** O `Set` de ouvintes de cada chave é uma lista em ordem de inserção com um identificador por ouvinte;
//!   a entrega percorre uma cópia e pula quem saiu durante a entrega. O ouvinte inscrito durante a entrega não a recebe (o `Set` do
//!   JavaScript o visitaria; o único ouvinte deste arquivo, o do `observableFromEvent`, nunca se inscreve em outra chave no meio).
//! - **`IEditorConfiguration`, `IDiffEditorOptions`, `IMarkdownString`.** As opções do diff são um objeto [`Dynamic`] (ver
//!   `diff_editor_options`); `readOnlyMessage` é o DTO do `IMarkdownString` (`MarkdownString::to_dto`). O `{ ...a, ...b }` é o
//!   [`spread_options`], e `Object.assign(editorConfiguration, diffEditorConfiguration)` sobre um `editor` ausente lança o `TypeError`
//!   do JavaScript.
//! - **`ISerializedMultiDiffEditorInput`.** É a struct [`SerializedMultiDiffEditorInput`], escrita por `JSON.stringify` (os campos
//!   `undefined` somem) e lida por `parse` de `marshalling`; o `JSON.parse` com a URI como texto devolve só texto, e o erro do `try`
//!   do original (JSON inválido, `URI.parse` de campo ausente, item sem os dois lados) é o `errors::catch`.
//! - **Registro.** `registerEditorSerializer` e `registerWorkbenchContribution2`, que o original faz em
//!   `multiDiffEditor.contribution.ts`, ficam com quem monta o workbench: [`MultiDiffEditorResolverContribution`] é [`Injectable`] e
//!   o serializador é [`MultiDiffEditorSerializer`].

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::{LazyStatefulPromise, async_fn, race_timeout};
use je_base::cancellation::cancel_on_dispose;
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::{self, Error, on_unexpected_error};
use je_base::event::{ConstValueWithChangeEvent, Event, Listener, ValueWithChangeEventLike};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, RefCounted, plain_disposable};
use je_base::marshalling::{Marshalled, parse};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::objects::deep_clone;
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::{
    DebugOwner, IObservable, Observable, ObservableExt, ObservableLazyPromise, SettableExt, ValueWithChangeEventFromObservable, autorun, const_observable,
    derived, keep_observed, map_observable_array_cached_by, observable_from_event, observable_from_value_with_change_event, observable_value,
    recompute_initially_and_on_change,
};
use je_base::promise::{MaybePromise, Promise};
use je_base::types::is_object;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_editor::diff_editor_options::spread_options;
use je_editor::multi_diff_editor::model::{ContextKeys, DiffItemSource, DocumentDiffItemRef, DocumentsValue, IDocumentDiffItem, IMultiDiffEditorModel};
use je_editor::text_resource_configuration_service::ITextResourceConfigurationService;
use je_instantiation::{Injectable, InstantiationService};
use je_platform::dialogs::ConfirmResult;
use je_platform::resources::host_platform;

use super::icons_contribution::multi_diff_editor_icon;
use super::multi_diff_editor_view_model::MultiDiffEditorViewModel;
use super::multi_diff_source_resolver_service::{
    IMultiDiffSourceResolverService, IResolvedMultiDiffSource, MultiDiffEditorItem, MultiDiffResources,
};
use crate::api::common::ext_host_type_converters::markup::MarkdownString;
use crate::common::editor::editor_factory_registry::IEditorSerializer;
use crate::common::editor::editor_input::{EditorInput, EditorInputBase, EditorInputIcon, IEditorCloseHandler, matches_by_default};
use crate::common::editor::{
    DEFAULT_EDITOR_ASSOCIATION_ID, EditorIdentifier, EditorInputCapabilities, EditorInputOrUntyped, EditorInputRef, EditorInputWithOptions, GroupIdentifier,
    RevertOptions, SaveOptions, UntypedEditorInput, default_editor_association,
};
use crate::services::editor::editor_groups_service::IEditorGroup;
use crate::services::editor::editor_resolver_service::{
    EditorInputFactoryObject, GlobPattern, IEditorResolverService, MultiDiffEditorInputFactoryFunction, RegisteredEditorOptions, RegisteredEditorPriority,
    RegisteredEditorRegistrationInfo,
};
use crate::services::files_configuration::files_configuration_service::Readonly;
use crate::services::textfile::textfiles::{
    ILanguageSupport, ITextFileEditorModel, ITextFileService, TextFileOperationResult, TextFileSaveOptions, is_text_file_operation_error,
    text_file_operation_result_of,
};
use crate::services::textmodel_resolver::resolver_service::{ITextEditorModel, ITextModelService, TextModelReference};
use crate::services::working_copy::working_copy::IWorkingCopy;

/// `MultiDiffEditorInput.ID`.
pub const MULTI_DIFF_EDITOR_INPUT_ID: &str = "workbench.input.multiDiffEditor";

/// `isBinaryTextFileOperationError(error)`.
fn is_binary_text_file_operation_error(error: &Error) -> bool {
    is_text_file_operation_error(error) && text_file_operation_result_of(error) == Some(TextFileOperationResult::FileIsBinary)
}

/// `{ source, resources, label }` do `_resolvedSource`: a fonte resolvida e os observáveis dela.
struct ResolvedSource {
    source: Option<Rc<dyn IResolvedMultiDiffSource>>,
    resources: IObservable<Rc<Vec<MultiDiffEditorItem>>>,
    label: Option<IObservable<String>>,
}

/// O `{ resources: ValueWithChangeEvent.const(this.initialResources) }` de `_resolvedSource`.
struct InitialResourcesSource {
    resources: Rc<Vec<MultiDiffEditorItem>>,
}

impl IResolvedMultiDiffSource for InitialResourcesSource {
    fn resources(&self) -> MultiDiffResources {
        Rc::new(ConstValueWithChangeEvent::new(self.resources.clone()))
    }
}

/// O corpo da `ObservableLazyPromise` de `_resolvedSource`.
fn resolve_source(
    initial_resources: Option<Rc<Vec<MultiDiffEditorItem>>>,
    multi_diff_source_resolver_service: Rc<dyn IMultiDiffSourceResolverService>,
    multi_diff_source: URI,
) -> Promise<Rc<ResolvedSource>> {
    async_fn(async move {
        let source: Option<Rc<dyn IResolvedMultiDiffSource>> = match initial_resources {
            Some(resources) => Some(Rc::new(InitialResourcesSource { resources })),
            None => multi_diff_source_resolver_service.resolve(&multi_diff_source).await?,
        };
        let owner = DebugOwner::new("MultiDiffEditorInput");
        Ok(Rc::new(ResolvedSource {
            resources: match &source {
                Some(source) => observable_from_value_with_change_event(owner.clone(), source.resources()),
                None => const_observable(Rc::new(Vec::new())),
            },
            label: source.as_ref().and_then(|source| source.label()).map(|label| observable_from_value_with_change_event(owner.clone(), label)),
            source,
        }))
    })
}

/// Os serviços e a fonte de que `_createModel` precisa.
struct ModelContext {
    text_model_service: Rc<dyn ITextModelService>,
    text_resource_configuration_service: Rc<dyn ITextResourceConfigurationService>,
    resolved_source: Rc<ObservableLazyPromise<Rc<ResolvedSource>>>,
}

/// `IDocumentDiffItemWithMultiDiffEditorItem`: o item de documento do multi diff, com o `MultiDiffEditorItem` de que veio.
pub struct DocumentDiffItemWithMultiDiffEditorItem {
    /// `multiDiffEditorItem`.
    pub multi_diff_editor_item: MultiDiffEditorItem,
    original: Option<DiffItemSource>,
    modified: Option<DiffItemSource>,
    context_keys: Option<ContextKeys>,
    /// O `modified?.object` do original, que o getter `options` lê (`isReadonly()`) a cada leitura.
    modified_model: Option<Rc<dyn ITextEditorModel>>,
    uri: URI,
    text_resource_configuration_service: Rc<dyn ITextResourceConfigurationService>,
}

impl IDocumentDiffItem for DocumentDiffItemWithMultiDiffEditorItem {
    fn original(&self) -> Option<&DiffItemSource> {
        self.original.as_ref()
    }

    fn modified(&self) -> Option<&DiffItemSource> {
        self.modified.as_ref()
    }

    /// O getter `options`: `{ ...getReadonlyConfiguration(modified?.object.isReadonly() ?? true), ...computeOptions(getValue(uri)) }`.
    fn options(&self) -> Option<Dynamic> {
        let is_readonly = self.modified_model.as_ref().map_or(Readonly::Bool(true), |model| model.is_readonly());
        Some(spread_options(
            &get_readonly_configuration(&is_readonly),
            &compute_options(&self.text_resource_configuration_service.get_value(Some(&self.uri), None)),
        ))
    }

    /// `onOptionsDidChange`: a mudança de configuração que afeta o `editor` ou o `diffEditor` do recurso.
    fn on_options_did_change(&self) -> Option<Event<()>> {
        let uri = self.uri.clone();
        Some(
            self.text_resource_configuration_service
                .on_did_change_configuration()
                .filter(move |e| e.affects_configuration(Some(&uri), "editor") || e.affects_configuration(Some(&uri), "diffEditor"), None)
                .map(|_| (), None),
        )
    }

    fn context_keys(&self) -> Option<ContextKeys> {
        self.context_keys.clone()
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

/// O `item as IDocumentDiffItemWithMultiDiffEditorItem` do original: o `MultiDiffEditorItem` de um item de documento do multi diff
/// (`None` para um item de outra origem).
pub fn multi_diff_editor_item_of(item: &dyn IDocumentDiffItem) -> Option<&MultiDiffEditorItem> {
    item.as_any()?.downcast_ref::<DocumentDiffItemWithMultiDiffEditorItem>().map(|item| &item.multi_diff_editor_item)
}

/// `getReadonlyConfiguration(isReadonly)`: `{ readOnly, readOnlyMessage }`.
fn get_readonly_configuration(is_readonly: &Readonly) -> Dynamic {
    Dynamic::object([
        ("readOnly", Dynamic::Bool(is_readonly.is_readonly())),
        (
            "readOnlyMessage",
            match is_readonly {
                Readonly::Markdown(markdown) => MarkdownString::to_dto(markdown),
                Readonly::Bool(_) => Dynamic::Undefined,
            },
        ),
    ])
}

/// `computeOptions(configuration)`.
fn compute_options(configuration: &Dynamic) -> Dynamic {
    let editor_configuration = deep_clone(&configuration.get_str("editor"));

    // Handle diff editor specially by merging in diffEditor configuration
    let diff_editor = configuration.get_str("diffEditor");
    if is_object(&diff_editor) {
        let diff_editor_configuration = deep_clone(&diff_editor);
        let Dynamic::Object(diff_editor_configuration) = diff_editor_configuration else {
            return editor_configuration;
        };

        if matches!(diff_editor_configuration.get(&JsString::from("hideOriginalLineNumbers")), Some(Dynamic::Null)) {
            diff_editor_configuration.delete(&JsString::from("hideOriginalLineNumbers"));
        }

        // User settings defines `diffEditor.codeLens`, but here we rename that to `diffEditor.diffCodeLens` to avoid collisions with `editor.codeLens`.
        let code_lens = diff_editor_configuration.get(&JsString::from("codeLens")).unwrap_or(Dynamic::Undefined);
        diff_editor_configuration.set(&JsString::from("diffCodeLens"), code_lens);
        diff_editor_configuration.delete(&JsString::from("codeLens"));

        // User settings defines `diffEditor.wordWrap`, but here we rename that to `diffEditor.diffWordWrap` to avoid collisions with `editor.wordWrap`.
        let word_wrap = diff_editor_configuration.get(&JsString::from("wordWrap")).unwrap_or(Dynamic::Undefined);
        diff_editor_configuration.set(&JsString::from("diffWordWrap"), word_wrap);
        diff_editor_configuration.delete(&JsString::from("wordWrap"));

        // `Object.assign(editorConfiguration, diffEditorConfiguration)`
        match &editor_configuration {
            Dynamic::Object(target) => {
                for (key, value) in diff_editor_configuration.entries() {
                    target.set(&key, value);
                }
            }
            Dynamic::Undefined | Dynamic::Null => errors::throw(Error::type_error("Cannot convert undefined or null to object")),
            _ => {}
        }
    }
    editor_configuration
}

/// Uses a map to efficiently dispatch events to listeners that are interested in a specific key.
struct FastEventDispatcher<T: 'static, K: 'static> {
    this: Weak<FastEventDispatcher<T, K>>,
    event: Event<T>,
    get_event_args_key: Box<dyn Fn(&T) -> String>,
    key_to_string: Box<dyn Fn(&K) -> String>,
    count: Cell<usize>,
    next_listener_id: Cell<usize>,
    buckets: RefCell<HashMap<String, Vec<(usize, Listener<T>)>>>,
    event_subscription: RefCell<Option<DisposableRef>>,
}

impl<T: 'static, K: Clone + 'static> FastEventDispatcher<T, K> {
    fn new(event: Event<T>, get_event_args_key: impl Fn(&T) -> String + 'static, key_to_string: impl Fn(&K) -> String + 'static) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            event,
            get_event_args_key: Box::new(get_event_args_key),
            key_to_string: Box::new(key_to_string),
            count: Cell::new(0),
            next_listener_id: Cell::new(0),
            buckets: RefCell::new(HashMap::new()),
            event_subscription: RefCell::new(None),
        })
    }

    /// `filteredEvent(filter)`: o evento só para os valores da chave dada.
    fn filtered_event(&self, filter: &K) -> Event<T> {
        let dispatcher = self.this.upgrade().expect("o despachante está vivo enquanto é usado");
        let filter = filter.clone();
        Event::new(move |listener: Listener<T>| {
            let key = (dispatcher.key_to_string)(&filter);
            let listener_id = dispatcher.next_listener_id.get();
            dispatcher.next_listener_id.set(listener_id + 1);
            dispatcher.buckets.borrow_mut().entry(key.clone()).or_default().push((listener_id, listener));

            dispatcher.count.set(dispatcher.count.get() + 1);
            if dispatcher.count.get() == 1 {
                let weak = dispatcher.this.clone();
                let subscription = dispatcher.event.listen(move |e| {
                    if let Some(dispatcher) = weak.upgrade() {
                        dispatcher.handle_event_change(e);
                    }
                });
                *dispatcher.event_subscription.borrow_mut() = Some(subscription);
            }

            let dispatcher = dispatcher.clone();
            plain_disposable(move || {
                {
                    let mut buckets = dispatcher.buckets.borrow_mut();
                    if let Some(bucket) = buckets.get_mut(&key) {
                        bucket.retain(|(id, _)| *id != listener_id);
                        if bucket.is_empty() {
                            buckets.remove(&key);
                        }
                    }
                }
                dispatcher.count.set(dispatcher.count.get() - 1);

                if dispatcher.count.get() == 0 {
                    let subscription = dispatcher.event_subscription.borrow_mut().take();
                    if let Some(subscription) = subscription {
                        subscription.dispose();
                    }
                }
            })
        })
    }

    /// `_handleEventChange`.
    fn handle_event_change(&self, e: &T) {
        let key = (self.get_event_args_key)(e);
        let listeners: Vec<(usize, Listener<T>)> = self.buckets.borrow().get(&key).cloned().unwrap_or_default();
        for (listener_id, listener) in listeners {
            let still_listening = self.buckets.borrow().get(&key).is_some_and(|bucket| bucket.iter().any(|(id, _)| *id == listener_id));
            if still_listening {
                listener(e);
            }
        }
    }
}

/// `isUriDirty(onDidChangeDirty, textFileService, uri)`.
fn is_uri_dirty(
    on_did_change_dirty: &FastEventDispatcher<Rc<dyn ITextFileEditorModel>, URI>,
    text_file_service: &Rc<dyn ITextFileService>,
    uri: &URI,
) -> IObservable<bool> {
    let (text_file_service, dirty_uri) = (text_file_service.clone(), uri.clone());
    observable_from_event(on_did_change_dirty.filtered_event(uri), move |_| text_file_service.is_dirty(&dirty_uri))
}

/// O modelo que `_createModel` entrega ao view model (`IMultiDiffEditorModel & IDisposable`).
struct MultiDiffEditorModel {
    documents: Rc<ValueWithChangeEventFromObservable<DocumentsValue>>,
    context_keys: Option<ContextKeys>,
    /// O `recomputeInitiallyAndOnChange(updateDocuments)`.
    subscription: DisposableRef,
}

impl IMultiDiffEditorModel for MultiDiffEditorModel {
    fn documents(&self) -> Rc<dyn ValueWithChangeEventLike<DocumentsValue>> {
        self.documents.clone()
    }

    fn context_keys(&self) -> Option<ContextKeys> {
        self.context_keys.clone()
    }
}

impl IDisposable for MultiDiffEditorModel {
    fn dispose(&self) {
        self.subscription.dispose();
    }
}

/// O corpo do mapeamento de `documentsWithPromises` em `_createModel`: o item de documento de um recurso, ou `None` se o `store` foi
/// descartado no meio do caminho ou a resolução falhou.
fn create_document(ctx: Rc<ModelContext>, item: MultiDiffEditorItem, store: Rc<DisposableStore>) -> Promise<Option<DocumentDiffItemRef>> {
    async_fn(async move {
        /* @description documentsWithPromises */
        let multi_diff_item_store = DisposableStore::new();
        let create_model_reference = |resource: Option<URI>| -> Promise<Option<TextModelReference>> {
            let text_model_service = ctx.text_model_service.clone();
            async_fn(async move {
                match resource {
                    Some(resource) => Ok(Some(text_model_service.create_model_reference(&resource).await?)),
                    None => Ok(None),
                }
            })
        };

        let mut results = Promise::all_settled(vec![create_model_reference(item.original_uri.clone()), create_model_reference(item.modified_uri.clone())])
            .await?
            .into_iter();
        let original_result = results.next().expect("o resultado do lado original");
        let modified_result = results.next().expect("o resultado do lado modificado");

        let mut original: Option<TextModelReference> = None;
        let mut modified: Option<TextModelReference> = None;
        if let Ok(reference) = &original_result {
            original = reference.clone();
            if let Some(reference) = &original {
                multi_diff_item_store.add(Rc::new(reference.clone()));
            }
        }
        if let Ok(reference) = &modified_result {
            modified = reference.clone();
            if let Some(reference) = &modified {
                multi_diff_item_store.add(Rc::new(reference.clone()));
            }
        }

        if store.is_disposed() {
            multi_diff_item_store.dispose();
            return Ok(None);
        }

        let error_results: Vec<&Error> = [&original_result, &modified_result].into_iter().filter_map(|result| result.as_ref().err()).collect();
        if let Some(error) = error_results.iter().find(|error| !is_binary_text_file_operation_error(error)) {
            multi_diff_item_store.dispose();
            eprintln!("{error}");
            on_unexpected_error(error);
            return Ok(None);
        }

        let is_binary = !error_results.is_empty();
        if is_binary {
            multi_diff_item_store.clear();
            original = None;
            modified = None;
        }

        let uri = item.modified_uri.clone().or_else(|| item.original_uri.clone()).expect("o item tem ao menos um dos lados");
        let source_of = |uri: &Option<URI>, reference: &Option<TextModelReference>| {
            uri.clone().map(|uri| DiffItemSource::new(uri, reference.as_ref().and_then(|reference| reference.object.text_editor_model())))
        };
        let result: Rc<dyn IDocumentDiffItem> = Rc::new(DocumentDiffItemWithMultiDiffEditorItem {
            original: source_of(&item.original_uri, &original),
            modified: source_of(&item.modified_uri, &modified),
            context_keys: item.context_keys.clone(),
            modified_model: modified.as_ref().map(|reference| reference.object.clone()),
            multi_diff_editor_item: item,
            uri,
            text_resource_configuration_service: ctx.text_resource_configuration_service.clone(),
        });
        let reference = RefCounted::create_of_non_disposable(result, multi_diff_item_store);
        store.add(reference.clone());
        Ok(Some(reference))
    })
}

/// `_createModel()`.
async fn create_model(ctx: &Rc<ModelContext>) -> Result<Rc<MultiDiffEditorModel>, Error> {
    let source = ctx.resolved_source.get_promise().await?;

    let documents_with_promises = {
        let ctx = ctx.clone();
        map_observable_array_cached_by(
            Some(DebugOwner::new("MultiDiffEditorInput")),
            source.resources.clone(),
            move |item: &MultiDiffEditorItem, store: &Rc<DisposableStore>| Rc::new(create_document(ctx.clone(), item.clone(), store.clone())),
            |item: &MultiDiffEditorItem| item.get_key(),
        )
    };

    let documents: Rc<ObservableValue<DocumentsValue, ()>> = observable_value("documents", DocumentsValue::Loading);

    let update_documents: IObservable<Rc<Promise<()>>> = {
        let documents = documents.clone();
        derived(move |reader| {
            /* @description Update documents */
            let cancellation_token = cancel_on_dispose(&reader.store());
            let docs_promises = documents_with_promises.read(reader);
            let documents = documents.clone();
            Rc::new(async_fn(async move {
                let docs = Promise::all(docs_promises.iter().map(|promise| (**promise).clone()).collect()).await?;
                if cancellation_token.is_cancellation_requested() {
                    return Ok(());
                }
                let new_documents: Vec<DocumentDiffItemRef> = docs.into_iter().flatten().collect();
                documents.set(DocumentsValue::Items(Rc::new(new_documents)), None);
                Ok(())
            }))
        })
    };

    let subscription = recompute_initially_and_on_change(&update_documents, None);
    let pending = update_documents.get();
    if let Err(error) = (*pending).clone().await {
        subscription.dispose();
        return Err(error);
    }

    let observable: IObservable<DocumentsValue> = documents;
    Ok(Rc::new(MultiDiffEditorModel {
        subscription,
        documents: Rc::new(ValueWithChangeEventFromObservable { observable }),
        context_keys: source.source.as_ref().and_then(|source| source.context_keys()),
    }))
}

/// `{ label, multiDiffSourceUri, resources }` de `ISerializedMultiDiffEditorInput`, um item: `{ originalUri, modifiedUri, goToFileUri }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SerializedMultiDiffEditorItem {
    pub original_uri: Option<String>,
    pub modified_uri: Option<String>,
    pub go_to_file_uri: Option<String>,
}

/// `ISerializedMultiDiffEditorInput`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SerializedMultiDiffEditorInput {
    pub multi_diff_source_uri: String,
    pub label: Option<String>,
    pub resources: Option<Vec<SerializedMultiDiffEditorItem>>,
}

/// O texto de um campo de um objeto lido por `parse` (`None` para um campo ausente, nulo ou que não é texto).
fn string_field(value: &Marshalled, name: &str) -> Option<String> {
    let Marshalled::Object(entries) = value else { return None };
    match entries.iter().find(|(key, _)| key == name) {
        Some((_, Marshalled::String(text))) => Some(text.to_string_lossy()),
        _ => None,
    }
}

impl SerializedMultiDiffEditorItem {
    /// O item como o `JSON.stringify` o escreve: os campos `undefined` somem.
    fn to_dynamic(&self) -> Dynamic {
        let entries = [("originalUri", &self.original_uri), ("modifiedUri", &self.modified_uri), ("goToFileUri", &self.go_to_file_uri)];
        Dynamic::object(entries.into_iter().filter_map(|(key, value)| value.as_ref().map(|value| (key, Dynamic::string(value)))))
    }
}

impl SerializedMultiDiffEditorInput {
    /// A forma que o `JSON.stringify(editor.serialize())` escreve.
    fn to_dynamic(&self) -> Dynamic {
        let mut entries: Vec<(&str, Dynamic)> = Vec::new();
        if let Some(label) = &self.label {
            entries.push(("label", Dynamic::string(label)));
        }
        entries.push(("multiDiffSourceUri", Dynamic::string(&self.multi_diff_source_uri)));
        if let Some(resources) = &self.resources {
            entries.push(("resources", Dynamic::array(resources.iter().map(SerializedMultiDiffEditorItem::to_dynamic))));
        }
        Dynamic::object(entries)
    }

    /// O `parse(serializedEditor) as ISerializedMultiDiffEditorInput`; o campo `multiDiffSourceUri` ausente é o `TypeError` que o
    /// `URI.parse(undefined)` do original lançaria.
    fn from_marshalled(data: &Marshalled) -> Result<Self, Error> {
        let multi_diff_source_uri = string_field(data, "multiDiffSourceUri").ok_or_else(|| Error::type_error("multiDiffSourceUri must be a string"))?;
        let resources = match data {
            Marshalled::Object(entries) => match entries.iter().find(|(key, _)| key == "resources") {
                Some((_, Marshalled::Array(items))) => Some(
                    items
                        .iter()
                        .map(|item| SerializedMultiDiffEditorItem {
                            original_uri: string_field(item, "originalUri"),
                            modified_uri: string_field(item, "modifiedUri"),
                            go_to_file_uri: string_field(item, "goToFileUri"),
                        })
                        .collect(),
                ),
                _ => None,
            },
            _ => None,
        };
        Ok(Self { multi_diff_source_uri, label: string_field(data, "label"), resources })
    }
}

/// O `closeHandler` de `MultiDiffEditorInput`: o `confirm` nunca é chamado e responde `DONT_SAVE`.
struct MultiDiffEditorCloseHandler;

impl IEditorCloseHandler for MultiDiffEditorCloseHandler {
    // This is a workaround for not having a better way
    // to figure out if the editors this input wraps
    // around are opened or not
    fn show_confirm(&self) -> bool {
        false
    }

    fn confirm(&self, _editors: &[EditorIdentifier]) -> Promise<ConfirmResult> {
        Promise::resolved(ConfirmResult::DontSave)
    }
}

/// O `mode` de `doSaveOrRevert`, com as opções que cada um leva.
enum SaveOrRevert {
    Save(Option<SaveOptions>),
    Revert(Option<RevertOptions>),
}

impl SaveOrRevert {
    /// `mode === 'save' ? textFileService.save(uri, options) : textFileService.revert(uri, options)`.
    fn run(&self, text_file_service: &dyn ITextFileService, resource: &URI) -> Promise<()> {
        match self {
            SaveOrRevert::Save(options) => text_file_service
                .save(resource, options.clone().map(|base| TextFileSaveOptions { base, ..TextFileSaveOptions::default() }))
                .then(|_| ()),
            SaveOrRevert::Revert(options) => text_file_service.revert(resource, *options),
        }
    }
}

/// `MultiDiffEditorInput`.
pub struct MultiDiffEditorInput {
    base: EditorInputBase,
    /// `multiDiffSource`.
    pub multi_diff_source: URI,
    /// `label`.
    pub label: Option<String>,
    /// `initialResources`.
    pub initial_resources: Option<Rc<Vec<MultiDiffEditorItem>>>,
    /// `isTransient`.
    pub is_transient: bool,

    name: Rc<RefCell<String>>,
    text_file_service: Rc<dyn ITextFileService>,
    view_model: LazyStatefulPromise<Rc<MultiDiffEditorViewModel>>,
    /// `resources`.
    pub resources: IObservable<Option<Rc<Vec<MultiDiffEditorItem>>>>,
    is_dirty_observable: IObservable<bool>,
    close_handler: Rc<dyn IEditorCloseHandler>,
}

/// Os argumentos estáticos do construtor: `multiDiffSource`, `label`, `initialResources` e `isTransient`.
pub type MultiDiffEditorInputArgs = (URI, Option<String>, Option<Vec<MultiDiffEditorItem>>, bool);

impl MultiDiffEditorInput {
    /// `MultiDiffEditorInput.ID`.
    pub const ID: &'static str = MULTI_DIFF_EDITOR_INPUT_ID;

    /// `fromResourceMultiDiffEditorInput(input, instantiationService)`.
    pub fn from_resource_multi_diff_editor_input(input: &UntypedEditorInput, instantiation_service: &InstantiationService) -> Rc<MultiDiffEditorInput> {
        if input.multi_diff_source.is_none() && input.resources.is_none() {
            errors::throw(Error::bug_indicating(Some("MultiDiffEditorInput requires either multiDiffSource or resources")));
        }
        let multi_diff_source =
            input.multi_diff_source.clone().unwrap_or_else(|| URI::parse(format!("multi-diff-editor:{}", generate_uuid()).as_str(), false));
        instantiation_service.create_instance::<MultiDiffEditorInput>((
            multi_diff_source,
            input.label.clone(),
            input.resources.as_ref().map(|resources| {
                resources
                    .iter()
                    .map(|resource| {
                        MultiDiffEditorItem::new(
                            resource.diff.original.as_ref().and_then(|original| original.resource.clone()),
                            resource.diff.modified.as_ref().and_then(|modified| modified.resource.clone()),
                            resource.go_to_file_resource.clone(),
                            None,
                            None,
                        )
                    })
                    .collect()
            }),
            input.is_transient.unwrap_or(false),
        ))
    }

    /// `fromSerialized(data, instantiationService)`.
    pub fn from_serialized(data: &SerializedMultiDiffEditorInput, instantiation_service: &InstantiationService) -> Rc<MultiDiffEditorInput> {
        let parse_uri = |uri: &Option<String>| uri.as_deref().filter(|uri| !uri.is_empty()).map(|uri| URI::parse(uri, false));
        instantiation_service.create_instance::<MultiDiffEditorInput>((
            URI::parse(data.multi_diff_source_uri.as_str(), false),
            data.label.clone(),
            data.resources.as_ref().map(|resources| {
                resources
                    .iter()
                    .map(|resource| {
                        MultiDiffEditorItem::new(parse_uri(&resource.original_uri), parse_uri(&resource.modified_uri), parse_uri(&resource.go_to_file_uri), None, None)
                    })
                    .collect()
            }),
            false,
        ))
    }

    /// `serialize()`.
    pub fn serialize(&self) -> SerializedMultiDiffEditorInput {
        let to_text = |uri: &Option<URI>| uri.as_ref().map(|uri| uri.to_string(false).to_string_lossy());
        SerializedMultiDiffEditorInput {
            label: self.label.clone(),
            multi_diff_source_uri: self.multi_diff_source.to_string(false).to_string_lossy(),
            resources: self.initial_resources.as_ref().map(|resources| {
                resources
                    .iter()
                    .map(|resource| SerializedMultiDiffEditorItem {
                        original_uri: to_text(&resource.original_uri),
                        modified_uri: to_text(&resource.modified_uri),
                        go_to_file_uri: to_text(&resource.go_to_file_uri),
                    })
                    .collect()
            }),
        }
    }

    /// `getViewModel()`.
    pub fn get_view_model(&self) -> Promise<Rc<MultiDiffEditorViewModel>> {
        self.view_model.get_promise()
    }

    /// `doSaveOrRevert(mode, group, options)`.
    fn do_save_or_revert(&self, mode: SaveOrRevert) -> Promise<()> {
        let Some(view_model) = self.view_model.current_value() else {
            return Promise::resolved(());
        };

        let text_file_service = self.text_file_service.clone();
        let items = view_model.items.get();
        let promises: Vec<Promise<()>> = items
            .iter()
            .filter(|item| !item.is_binary())
            .map(|item| {
                let model = &item.diff_editor_view_model().model;
                let (original, modified) = (model.original.uri(), model.modified.uri());
                let handle_original = *original.scheme() != schemas::UNTITLED && text_file_service.is_dirty(&original); // match diff editor behaviour

                Promise::all(vec![
                    if handle_original { mode.run(&*text_file_service, &original) } else { Promise::resolved(()) },
                    mode.run(&*text_file_service, &modified),
                ])
                .then(|_| ())
            })
            .collect();
        Promise::all(promises).then(|_| ())
    }
}

impl Injectable for MultiDiffEditorInput {
    type Args = MultiDiffEditorInputArgs;
    type Deps = (
        Rc<dyn ITextModelService>,
        Rc<dyn ITextResourceConfigurationService>,
        Rc<InstantiationService>,
        Rc<dyn IMultiDiffSourceResolverService>,
        Rc<dyn ITextFileService>,
    );

    fn create(
        (multi_diff_source, label, initial_resources, is_transient): Self::Args,
        (text_model_service, text_resource_configuration_service, instantiation_service, multi_diff_source_resolver_service, text_file_service): Self::Deps,
    ) -> Rc<Self> {
        let initial_resources = initial_resources.map(Rc::new);
        let owner = DebugOwner::new("MultiDiffEditorInput");

        let resolved_source = {
            let (initial_resources, multi_diff_source) = (initial_resources.clone(), multi_diff_source.clone());
            Rc::new(ObservableLazyPromise::new(move || {
                resolve_source(initial_resources.clone(), multi_diff_source_resolver_service.clone(), multi_diff_source.clone())
            }))
        };

        let resources: IObservable<Option<Rc<Vec<MultiDiffEditorItem>>>> = {
            let resolved_source = resolved_source.clone();
            derived(move |reader| {
                resolved_source.cached_promise_result.read(reader).and_then(|result| result.data.clone()).map(|source| source.resources.read(reader))
            })
        };

        let text_file_service_on_did_change = FastEventDispatcher::new(
            text_file_service.files().on_did_change_dirty(),
            |item: &Rc<dyn ITextFileEditorModel>| item.resource().to_string(false).to_string_lossy(),
            |uri: &URI| uri.to_string(false).to_string_lossy(),
        );
        let is_dirty_observables = {
            let text_file_service = text_file_service.clone();
            map_observable_array_cached_by(
                Some(owner),
                resources.map(|resources, _| resources.unwrap_or_else(|| Rc::new(Vec::new()))),
                move |res: &MultiDiffEditorItem, _store: &Rc<DisposableStore>| {
                    let dirty_of = |uri: &Option<URI>| {
                        uri.as_ref().map_or_else(|| const_observable(false), |uri| is_uri_dirty(&text_file_service_on_did_change, &text_file_service, uri))
                    };
                    let (is_modified_dirty, is_original_dirty) = (dirty_of(&res.modified_uri), dirty_of(&res.original_uri));
                    derived(move |reader| /* @description modifiedDirty||originalDirty */ is_modified_dirty.read(reader) || is_original_dirty.read(reader))
                },
                |item: &MultiDiffEditorItem| item.get_key(),
            )
        };
        let is_dirty_observable: IObservable<bool> = derived(move |reader| is_dirty_observables.read(reader).iter().any(|is_dirty| is_dirty.read(reader)));

        let ctx = Rc::new(ModelContext {
            text_model_service,
            text_resource_configuration_service,
            resolved_source: resolved_source.clone(),
        });
        let name = Rc::new(RefCell::new(String::new()));

        let this = Rc::new_cyclic(|this: &Weak<Self>| {
            let view_model = LazyStatefulPromise::new({
                let (this, ctx) = (this.clone(), ctx.clone());
                move || {
                    async_fn(async move {
                        let store = DisposableStore::new();
                        let result = create_view_model(&this, &ctx, &instantiation_service, &store).await;
                        if result.is_err() {
                            store.dispose();
                        }
                        result
                    })
                }
            });

            Self {
                base: EditorInputBase::new(this.clone()),
                multi_diff_source,
                label: label.clone(),
                initial_resources,
                is_transient,
                name: name.clone(),
                text_file_service,
                view_model,
                resources: resources.clone(),
                is_dirty_observable,
                close_handler: Rc::new(MultiDiffEditorCloseHandler),
            }
        });

        this.base.register(keep_observed(&this.is_dirty_observable));

        let weak = Rc::downgrade(&this);
        this.base.register(autorun(move |reader| {
            /* @description Updates name */
            let resources = resources.read(reader);
            let resolved_source = resolved_source.cached_promise_result.read(reader).and_then(|result| result.data.clone());
            let label = resolved_source
                .and_then(|source| source.label.as_ref().map(|label| label.read(reader)))
                .or_else(|| label.clone())
                .unwrap_or_else(|| localize("name", "Multi Diff Editor", &[]));
            *name.borrow_mut() = match &resources {
                Some(resources) if resources.len() == 1 => localize("nameWithOneFile", "{0} (1 file)", &[label.as_str()]),
                Some(resources) => localize("nameWithFiles", "{0} ({1} files)", &[label.as_str(), resources.len().to_string().as_str()]),
                None => label,
            };
            if let Some(this) = weak.upgrade() {
                this.base.fire_did_change_label();
            }
        }));

        this
    }
}

/// O corpo do `LazyStatefulPromise` de `_viewModel`, depois do `new DisposableStore()` e antes do `catch`.
async fn create_view_model(
    this: &Weak<MultiDiffEditorInput>,
    ctx: &Rc<ModelContext>,
    instantiation_service: &Rc<InstantiationService>,
    store: &Rc<DisposableStore>,
) -> Result<Rc<MultiDiffEditorViewModel>, Error> {
    // Uma entrada já liberada vale como descartada (`this._store.isDisposed`).
    let is_disposed = || this.upgrade().is_none_or(|this| this.base.is_disposed());

    let model = store.add_rc(create_model(ctx).await?);
    if is_disposed() {
        return Err(Error::cancellation());
    }

    let model: Rc<dyn IMultiDiffEditorModel> = model;
    let vm = store.add_rc(instantiation_service.create_instance::<MultiDiffEditorViewModel>(model));
    race_timeout(&vm.wait_for_diff_or_1s(), 1000, None).await?;
    if is_disposed() {
        return Err(Error::cancellation());
    }

    if let Some(this) = this.upgrade() {
        this.base.register(store.clone());
    }
    Ok(vm)
}

impl ILanguageSupport for MultiDiffEditorInput {
    fn set_language_id(&self, language_id: &str, source: Option<&str>) {
        let Some(active_diff_item) = self.view_model.require_value().active_diff_item.observable.get() else {
            return;
        };
        let value = active_diff_item.document_diff_item();
        let Some(target) = value.modified().or_else(|| value.original()).and_then(|side| side.text_model.clone()) else {
            return;
        };
        target.set_language(language_id, source);
    }
}

impl IDisposable for MultiDiffEditorInput {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl EditorInput for MultiDiffEditorInput {
    fn base(&self) -> &EditorInputBase {
        &self.base
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn editor_type_id(&self) -> String {
        Self::ID.to_string()
    }

    fn resource(&self) -> Option<URI> {
        Some(self.multi_diff_source.clone())
    }

    fn capabilities(&self) -> EditorInputCapabilities {
        EditorInputCapabilities::READONLY
    }

    fn get_name(&self) -> String {
        self.name.borrow().clone()
    }

    fn editor_id(&self) -> Option<String> {
        Some(DEFAULT_EDITOR_ASSOCIATION_ID.to_string())
    }

    fn get_icon(&self) -> Option<EditorInputIcon> {
        Some(EditorInputIcon::Theme(multi_diff_editor_icon()))
    }

    fn matches(&self, other_input: &EditorInputOrUntyped) -> bool {
        if matches_by_default(self, other_input) {
            return true;
        }

        match other_input {
            EditorInputOrUntyped::Typed(other_input) => other_input
                .as_any()
                .downcast_ref::<MultiDiffEditorInput>()
                .is_some_and(|other_input| self.multi_diff_source.to_string(false) == other_input.multi_diff_source.to_string(false)),
            EditorInputOrUntyped::Untyped(_) => false,
        }
    }

    fn on_did_change_dirty(&self) -> Event<()> {
        Event::<()>::from_observable_light(self.is_dirty_observable.clone())
    }

    fn is_dirty(&self) -> bool {
        self.is_dirty_observable.get()
    }

    fn save(&self, _group: GroupIdentifier, options: Option<SaveOptions>) -> Promise<Option<EditorInputOrUntyped>> {
        let this = self.this();
        self.do_save_or_revert(SaveOrRevert::Save(options)).then(move |()| Some(EditorInputOrUntyped::Typed(this)))
    }

    fn revert(&self, _group: GroupIdentifier, options: Option<RevertOptions>) -> Promise<()> {
        self.do_save_or_revert(SaveOrRevert::Revert(options))
    }

    fn close_handler(&self) -> Option<Rc<dyn IEditorCloseHandler>> {
        Some(self.close_handler.clone())
    }
}

/// `MultiDiffEditorResolverContribution`.
pub struct MultiDiffEditorResolverContribution {
    base: Rc<Disposable>,
}

impl MultiDiffEditorResolverContribution {
    pub const ID: &'static str = "workbench.contrib.multiDiffEditorResolver";
}

impl Injectable for MultiDiffEditorResolverContribution {
    type Args = ();
    type Deps = (Rc<dyn IEditorResolverService>, Rc<InstantiationService>);

    fn create(_: Self::Args, (editor_resolver_service, instantiation_service): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();

        let default_editor = default_editor_association();
        let create_multi_diff_editor_input: MultiDiffEditorInputFactoryFunction =
            Rc::new(move |multi_diff_editor: &UntypedEditorInput, _group: &Rc<dyn IEditorGroup>| {
                let editor: EditorInputRef = MultiDiffEditorInput::from_resource_multi_diff_editor_input(multi_diff_editor, &instantiation_service);
                MaybePromise::Value(EditorInputWithOptions { editor, options: None })
            });
        base.register(editor_resolver_service.register_editor(
            GlobPattern::from("*"),
            RegisteredEditorRegistrationInfo {
                id: default_editor.id.to_string(),
                label: default_editor.display_name,
                detail: Some(default_editor.provider_display_name),
                priority: RegisteredEditorPriority::Builtin.into(),
            },
            RegisteredEditorOptions::default(),
            EditorInputFactoryObject { create_multi_diff_editor_input: Some(create_multi_diff_editor_input), ..EditorInputFactoryObject::default() },
        ));

        Rc::new(Self { base })
    }
}

impl IDisposable for MultiDiffEditorResolverContribution {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `MultiDiffEditorSerializer`.
pub struct MultiDiffEditorSerializer;

impl Injectable for MultiDiffEditorSerializer {
    type Args = ();
    type Deps = ();

    fn create(_: Self::Args, _: Self::Deps) -> Rc<Self> {
        Rc::new(Self)
    }
}

impl IEditorSerializer for MultiDiffEditorSerializer {
    fn can_serialize(&self, editor: &EditorInputRef) -> bool {
        editor.as_any().downcast_ref::<MultiDiffEditorInput>().is_some_and(|editor| !editor.is_transient)
    }

    fn serialize(&self, editor: &EditorInputRef) -> Option<String> {
        if !self.can_serialize(editor) {
            return None;
        }

        let editor = editor.as_any().downcast_ref::<MultiDiffEditorInput>()?;
        json_stringify(&editor.serialize().to_dynamic())
    }

    fn deserialize(&self, instantiation_service: &InstantiationService, serialized_editor: &str) -> Option<EditorInputRef> {
        let deserialized = errors::catch(|| {
            let data = parse(JsString::from(serialized_editor).units(), host_platform());
            let data = SerializedMultiDiffEditorInput::from_marshalled(&data)?;
            // `MultiDiffEditorItem` com os dois lados ausentes e `URI.parse` de texto inválido lançam, como no original.
            Ok::<_, Error>(MultiDiffEditorInput::from_serialized(&data, instantiation_service))
        })
        .and_then(|result| result);

        match deserialized {
            Ok(editor) => Some(editor),
            Err(error) => {
                on_unexpected_error(&error);
                None
            }
        }
    }
}
