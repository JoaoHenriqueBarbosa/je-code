//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/model/inlineCompletionsSource.ts`: a fonte das sugestões
//! em linha (fila de requisições, debounce, estado `InlineCompletionsState` mantido por um redutor observável e
//! telemetria de resposta vazia) sobre o `provide_inline_completions`.
//!
//! Notas de tradução:
//! - `Disposable` (classe base) é o `Rc<Disposable>` guardado em `base`; o `_store.isDisposed` é o `disposed`, ligado
//!   antes de a base descartar os filhos (como o `DisposableStore.dispose` faz);
//! - `async`/`await` é [`async_fn`] sobre [`Promise`]; o `try { ... } finally { ... }` do `fetch` é o `run` seguido do
//!   bloco de limpeza, que roda no sucesso e no erro; o `for await` sobre as listas é um laço sobre `next()` que fecha o
//!   iterador com `return_` ao sair mais cedo (um erro do corpo não fecha o iterador);
//! - `observableReducerSettable` com `recordChangesLazy({ versionId })` é o redutor de `je_base` com o rastreador
//!   preguiçoso; o delta de `versionId` é o [`ModelContentChangedEvent`] (ausente quando indefinido). O estado é o
//!   [`SourceState`], cujos campos são `Rc<InlineCompletionsState>` e cuja igualdade é a identidade dos dois;
//! - `new Set(providers)` compara provedores por identidade, então o conjunto é de endereços dos `Rc`;
//! - colaboradores cujo porte ainda não existe entram como traits injetados no [`InlineCompletionsSourceServices`]:
//!   o `RenameSymbolProcessor` ([`InlineCompletionsRenameProcessor`]), o `StructuredLogger` do `logFetch`
//!   ([`InlineCompletionsFetchLogger`]) e o `DataChannelForwardingTelemetryService` com o `isCopilotLikeExtension`
//!   ([`InlineCompletionsRequestTelemetry`]). O `formatRecordableLogEntry` é [`format_recordable_log_entry`] e o
//!   `isCompletionsEnabledFromObject` (`completionsEnablement.ts`) é [`is_completions_enabled_from_object`];
//! - o `offsetEditFromContentChanges` (`textModelStringEdit.ts`) é a função privada de mesmo nome;
//! - o objeto de log de cada sugestão é um [`Dynamic`] montado campo a campo no lugar do `cloneAndChange`: `IRange` e
//!   `IPosition` viram texto, `Command` vira `{ $commandId }`; o `uri` sai com os componentes e o `warning` e o `hint`
//!   saem com a representação de depuração (o `JSON.stringify` do original lê os campos do objeto);
//! - `Date.now()` é [`date_now`] (milissegundos).

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use je_base::async_::{AsyncIterable, RunOnceScheduler, async_fn};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::Error;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, MutableDisposable, plain_disposable, to_disposable};
use je_base::observable::base::{IObservable, ISettableObservable, ITransaction, Observable};
use je_base::observable::change_tracker::{RecordedChanges, record_changes, record_changes_lazy};
use je_base::observable::experimental::reducer::{ReducerOptions, observable_reducer_settable};
use je_base::observable::observables::derived::{IDerivedReader, derived};
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::utils::utils::recompute_initially_and_on_change;
use je_base::observable::{DebugOwner, ObservableExt, SettableExt, StrictEquals, observable_value, run_on_change, transaction};
use je_base::promise::Promise;
use je_base::runtime::date_now;
use je_base::uri::{URI, UriComponents};
use je_base::JsString;
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::contextkey::IContextKeyService;
use je_platform::log::{LogServiceRef, Logger};
use je_platform::observable::platform_observable_utils::observable_config_value;

use crate::code_action::languages::Command;
use crate::edits::edit::BaseEdit;
use crate::edits::string_edit::{StringEdit, StringReplacement};
use crate::editor_extensions::{ITextModelService, ModelReference};
use crate::inline_completions::inline_suggestion_item::{InlineSuggestionIdentity, InlineSuggestionItem, InlineSuggestionItemBase};
use crate::inline_completions::provide_inline_completions::{
    IInlineSuggestDataAction, InlineCompletionContextWithoutUuid, InlineSuggestRequestInfo, provide_inline_completions,
    run_when_cancelled,
};
use crate::inline_completions::text_model_value_reference::TextModelValueReference;
use crate::inline_completions::utils::wait;
use crate::language_configuration_registry::ILanguageConfigurationService;
use crate::language_feature_debounce::IFeatureDebounceInformation;
use crate::languages::{
    InlineCompletion, InlineCompletionEndOfLifeReason, InlineCompletionInsertText, InlineCompletionTriggerKind,
    InlineCompletionsDisposeReason, InlineCompletionsDisposeReasonKind, InlineCompletionsProvider,
};
use crate::offset_range::OffsetRange;
use crate::position::Position;
use crate::range::Range;
use crate::text_model::TextModel;
use crate::text_model_events::{ModelContentChange, ModelContentChangedEvent};

/// O `sourceId` das entradas de log do `fetch`.
const FETCH_SOURCE_ID: &str = "InlineCompletions.fetch";

thread_local! {
    /// `InlineCompletionsSource._requestId`.
    static REQUEST_ID: Cell<u32> = const { Cell::new(0) };
}

/// `isCompletionsEnabledFromObject(completionsEnablementObject, modeId)` (`completionsEnablement.ts`).
pub fn is_completions_enabled_from_object(completions_enablement_object: Option<&Dynamic>, mode_id: &str) -> bool {
    let Some(Dynamic::Object(object)) = completions_enablement_object else {
        return false; // default to disabled if setting is not available
    };
    let object = Dynamic::Object(object.clone());

    let explicit = object.get_str(mode_id);
    if !explicit.is_undefined() {
        return is_truthy(&explicit); // go with setting if explicitly defined
    }

    is_truthy(&object.get_str("*")) // fallback to global setting otherwise
}

/// O `Boolean(value)` do JavaScript.
fn is_truthy(value: &Dynamic) -> bool {
    match value {
        Dynamic::Undefined | Dynamic::Null => false,
        Dynamic::Bool(b) => *b,
        Dynamic::Number(n) => *n != 0.0 && !n.is_nan(),
        Dynamic::BigInt(n) => *n != 0,
        Dynamic::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// O `offsetEditFromContentChanges(contentChanges)` de `textModelStringEdit.ts`.
fn offset_edit_from_content_changes(content_changes: &[ModelContentChange]) -> StringEdit {
    let mut replacements: Vec<StringReplacement> = content_changes
        .iter()
        .map(|c| StringReplacement::new(OffsetRange::of_start_and_length(c.range_offset as i32, c.range_length as i32), c.text.clone()))
        .collect();
    replacements.reverse();
    StringEdit::new(replacements)
}

// #region Colaboradores injetados

/// O `RenameSymbolProcessor`: refina uma sugestão com a refatoração de renomear, quando o provedor a suporta.
pub trait InlineCompletionsRenameProcessor: IDisposable {
    /// `proposeRenameRefactoring(textModel, suggestion, context)`.
    fn propose_rename_refactoring(
        &self,
        text_model: &Rc<TextModel>,
        suggestion: InlineSuggestionItem,
        context: &InlineCompletionContextWithoutUuid,
    ) -> Promise<InlineSuggestionItem>;
}

/// As entradas que o `fetch` registra: `IDocumentEventFetchStart` (com `IRecordableEditorLogEntry`) e `IEventFetchEnd`.
#[derive(Clone)]
pub enum FetchLogEntry {
    Start {
        request_id: u32,
        model_uri: URI,
        model_version: i64,
        context: Dynamic,
        time: f64,
        provider: Option<String>,
    },
    End {
        request_id: u32,
        duration_ms: f64,
        error: Option<String>,
        result: Vec<Dynamic>,
        time: f64,
        did_all_providers_return: bool,
    },
}

impl FetchLogEntry {
    /// O `sourceId`.
    pub fn source_id(&self) -> &'static str {
        FETCH_SOURCE_ID
    }

    /// O objeto da entrada, sem o `sourceId` e com o `modelUri` em texto (o que o `formatRecordableLogEntry` serializa).
    pub fn to_dynamic(&self) -> Dynamic {
        match self {
            FetchLogEntry::Start { request_id, model_uri, model_version, context, time, provider } => Dynamic::object([
                ("kind", Dynamic::string("start")),
                ("requestId", Dynamic::Number(f64::from(*request_id))),
                ("modelUri", Dynamic::String(model_uri.to_string(false))),
                ("modelVersion", Dynamic::Number(*model_version as f64)),
                ("context", context.clone()),
                ("time", Dynamic::Number(*time)),
                ("provider", provider.as_deref().map_or(Dynamic::Undefined, Dynamic::string)),
            ]),
            FetchLogEntry::End { request_id, duration_ms, error, result, time, did_all_providers_return } => Dynamic::object([
                ("kind", Dynamic::string("end")),
                ("requestId", Dynamic::Number(f64::from(*request_id))),
                ("durationMs", Dynamic::Number(*duration_ms)),
                ("error", error.as_deref().map_or(Dynamic::Undefined, Dynamic::string)),
                ("result", Dynamic::array(result.iter().cloned())),
                ("time", Dynamic::Number(*time)),
                ("didAllProvidersReturn", Dynamic::Bool(*did_all_providers_return)),
            ]),
        }
    }
}

/// `formatRecordableLogEntry(entry)`.
pub fn format_recordable_log_entry(entry: &FetchLogEntry) -> String {
    format!("{} @@ {}", entry.source_id(), json_stringify(&entry.to_dynamic()).unwrap_or_default())
}

/// O `StructuredLogger` de `editor.inlineSuggest.logFetch.commandId`.
pub trait InlineCompletionsFetchLogger: IDisposable {
    /// `isEnabled`.
    fn is_enabled(&self) -> IObservable<bool>;
    /// `log(entry)`.
    fn log(&self, entry: &FetchLogEntry);
}

/// O evento de fim de vida que a fonte envia para um pedido sem sugestão (`InlineCompletionEndOfLifeEvent` com
/// `groupId: 'empty'`, `shown: false`, `extensionId: 'vscode-core'` e `extensionVersion: '0.0.0'`); os demais campos
/// do original são indefinidos nesse evento.
#[derive(Clone, Debug)]
pub struct EmptyResponseTelemetryEvent {
    pub opportunity_id: String,
    pub no_suggestion_reason: String,
    pub sku_plan: Option<String>,
    pub sku_type: Option<String>,
    pub editor_type: String,
    pub request_reason: String,
    pub typing_interval: f64,
    pub typing_interval_character_count: i32,
    pub language_id: String,
    pub selected_suggestion_info: bool,
    pub available_providers: String,
    /// `forwardToChannelIf(...)`.
    pub forward_to_channel: bool,
}

/// O `DataChannelForwardingTelemetryService` e o `isCopilotLikeExtension` de `forwardingTelemetryService.ts`.
pub trait InlineCompletionsRequestTelemetry {
    /// `isCopilotLikeExtension(extensionId)`.
    fn is_copilot_like_extension(&self, extension_id: Option<&str>) -> bool;
    /// `sendInlineCompletionsEndOfLifeTelemetry(dataChannelTelemetryService, event)`.
    fn send_empty_response(&self, event: EmptyResponseTelemetryEvent);
}

/// Os serviços que o construtor recebe por injeção.
pub struct InlineCompletionsSourceServices {
    pub language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    pub log_service: LogServiceRef,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub text_model_service: Rc<dyn ITextModelService>,
    pub rename_processor: Rc<dyn InlineCompletionsRenameProcessor>,
    pub fetch_logger: Rc<dyn InlineCompletionsFetchLogger>,
    pub telemetry: Rc<dyn InlineCompletionsRequestTelemetry>,
}

// #endregion

/// O valor do redutor: `{ inlineCompletions, suggestWidgetInlineCompletions }`.
#[derive(Clone)]
struct SourceState {
    inline_completions: Rc<InlineCompletionsState>,
    suggest_widget_inline_completions: Rc<InlineCompletionsState>,
}

impl SourceState {
    fn empty() -> Self {
        Self {
            inline_completions: Rc::new(InlineCompletionsState::create_empty()),
            suggest_widget_inline_completions: Rc::new(InlineCompletionsState::create_empty()),
        }
    }

    fn dispose(&self) {
        self.inline_completions.dispose();
        self.suggest_widget_inline_completions.dispose();
    }
}

impl StrictEquals for SourceState {
    fn strict_equals(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inline_completions, &other.inline_completions)
            && Rc::ptr_eq(&self.suggest_widget_inline_completions, &other.suggest_widget_inline_completions)
    }
}

/// `InlineCompletionsSource`.
pub struct InlineCompletionsSource {
    base: Rc<Disposable>,
    disposed: Cell<bool>,
    text_model: Rc<TextModel>,
    cursor_position: IObservable<Position>,
    debounce_value: Rc<dyn IFeatureDebounceInformation>,
    language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    log_service: LogServiceRef,
    context_key_service: Rc<dyn IContextKeyService>,
    text_model_service: Rc<dyn ITextModelService>,
    update_operation: Rc<MutableDisposable<UpdateOperation>>,
    logging_enabled: IObservable<bool>,
    send_request_data: IObservable<bool>,
    structured_fetch_logger: Rc<dyn InlineCompletionsFetchLogger>,
    rename_processor: Rc<dyn InlineCompletionsRenameProcessor>,
    telemetry: Rc<dyn InlineCompletionsRequestTelemetry>,
    state: ISettableObservable<SourceState, ()>,
    completions_enabled: Rc<RefCell<Option<Dynamic>>>,
    loading_count: Rc<ObservableValue<i32, ()>>,

    pub inline_completions: IObservable<Rc<InlineCompletionsState>>,
    pub suggest_widget_inline_completions: IObservable<Rc<InlineCompletionsState>>,
    pub clear_operation_on_text_model_change: IObservable<()>,
    pub loading: IObservable<bool>,
}

impl InlineCompletionsSource {
    /// O construtor: `versionId` é a versão do modelo com o evento de mudança como delta (`null` sem modelo).
    pub fn new(
        text_model: Rc<TextModel>,
        version_id: IObservable<Option<i64>>,
        debounce_value: Rc<dyn IFeatureDebounceInformation>,
        cursor_position: IObservable<Position>,
        completions_enablement_setting: Option<String>,
        services: InlineCompletionsSourceServices,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let InlineCompletionsSourceServices {
            language_configuration_service,
            log_service,
            configuration_service,
            context_key_service,
            text_model_service,
            rename_processor,
            fetch_logger,
            telemetry,
        } = services;

        let logging_enabled = observable_config_value("editor.inlineSuggest.logFetch", false, Rc::clone(&configuration_service), as_bool);
        base.register(recompute_initially_and_on_change(&logging_enabled, None));
        let send_request_data =
            observable_config_value("editor.inlineSuggest.emptyResponseInformation", true, Rc::clone(&configuration_service), as_bool);
        base.register(recompute_initially_and_on_change(&send_request_data, None));

        base.register(dispose_handle(&fetch_logger));
        base.register(dispose_handle(&rename_processor));

        let update_operation = MutableDisposable::<UpdateOperation>::new();
        base.register(Rc::clone(&update_operation) as DisposableRef);

        let state = Self::create_state(&text_model, &version_id);

        let state_observable = state.to_observable();
        let inline_completions = state_observable.map(|v, _| v.inline_completions);
        let suggest_widget_inline_completions = state_observable.map(|v, _| v.suggest_widget_inline_completions);

        let clear_operation_on_text_model_change = {
            let (version_id, update_operation) = (Rc::clone(&version_id), Rc::clone(&update_operation));
            derived(move |reader| {
                version_id.read(reader);
                update_operation.clear();
            })
        };
        base.register(recompute_initially_and_on_change(&clear_operation_on_text_model_change, None));

        let completions_enabled: Rc<RefCell<Option<Dynamic>>> = Rc::default();
        if let Some(setting) = completions_enablement_setting {
            update_completions_enablement(&configuration_service, &setting, &completions_enabled);
            let (configuration_service_for_listener, completions_enabled_for_listener) =
                (Rc::clone(&configuration_service), Rc::clone(&completions_enabled));
            base.register(configuration_service.on_did_change_configuration().listen(move |e| {
                if e.affects_configuration(setting.as_str(), None) {
                    update_completions_enablement(&configuration_service_for_listener, &setting, &completions_enabled_for_listener);
                }
            }));
        }

        base.register(recompute_initially_and_on_change(&state_observable, None));

        let loading_count = observable_value::<i32, ()>("loadingCount", 0);
        let loading = loading_count.to_observable().map(|v, _| v > 0);

        Rc::new(Self {
            base,
            disposed: Cell::new(false),
            text_model,
            cursor_position,
            debounce_value,
            language_configuration_service,
            log_service,
            context_key_service,
            text_model_service,
            update_operation,
            logging_enabled,
            send_request_data,
            structured_fetch_logger: fetch_logger,
            rename_processor,
            telemetry,
            state,
            completions_enabled,
            loading_count,
            inline_completions,
            suggest_widget_inline_completions,
            clear_operation_on_text_model_change,
            loading,
        })
    }

    /// O `observableReducerSettable(this, { initial, disposeFinal, changeTracker, update })`.
    fn create_state(text_model: &Rc<TextModel>, version_id: &IObservable<Option<i64>>) -> ISettableObservable<SourceState, ()> {
        let tracked_version_id = Rc::clone(version_id);
        let text_model = Rc::clone(text_model);
        observable_reducer_settable::<SourceState, RecordedChanges, ()>(
            DebugOwner::new("InlineCompletionsSource"),
            ReducerOptions {
                initial: Rc::new(SourceState::empty),
                dispose_final: Some(Rc::new(|values: &SourceState| values.dispose())),
                change_tracker: record_changes_lazy(move || {
                    record_changes().add::<Option<i64>, ModelContentChangedEvent>("versionId", &tracked_version_id)
                }),
                equality_comparer: None,
                update: Rc::new(move |_reader: &dyn IDerivedReader<()>, previous_value: &SourceState, changes: &RecordedChanges| {
                    let edits: Vec<StringEdit> = changes
                        .changes
                        .borrow()
                        .iter()
                        .map(|c| c.change::<ModelContentChangedEvent>().map_or_else(StringEdit::empty, |change| offset_edit_from_content_changes(&change.changes)))
                        .collect();
                    let edit = StringEdit::compose_all(&edits);

                    if edit.is_empty() {
                        return previous_value.clone();
                    }
                    let result = SourceState {
                        inline_completions: Rc::new(previous_value.inline_completions.create_state_with_applied_edit(&edit, &text_model)),
                        suggest_widget_inline_completions: Rc::new(
                            previous_value.suggest_widget_inline_completions.create_state_with_applied_edit(&edit, &text_model),
                        ),
                    };
                    previous_value.dispose();
                    result
                }),
            },
        )
    }

    fn log(&self, entry: &FetchLogEntry) {
        if self.logging_enabled.get() {
            self.log_service.info(&format_recordable_log_entry(entry));
        }
        self.structured_fetch_logger.log(entry);
    }

    /// `fetch(...)`: pede as sugestões aos provedores, reaproveitando a requisição em andamento ou o estado atual
    /// quando eles já satisfazem o pedido. Resolve em `true` quando o estado foi atualizado.
    pub fn fetch(
        self: &Rc<Self>,
        providers: Vec<Rc<dyn InlineCompletionsProvider>>,
        providers_label: Option<String>,
        context: InlineCompletionContextWithoutUuid,
        active_inline_completion: Option<Rc<InlineSuggestionIdentity>>,
        with_debounce: bool,
        user_jumped_to_active_completion: IObservable<bool>,
        request_info: InlineSuggestRequestInfo,
    ) -> Promise<bool> {
        let position = self.cursor_position.get();
        let request = Rc::new(UpdateRequest::new(position, context.clone(), self.text_model.get_version_id(), provider_addresses(&providers)));

        let target = if context.selected_suggestion_info.is_some() {
            self.suggest_widget_inline_completions.get()
        } else {
            self.inline_completions.get()
        };

        if let Some(operation) = self.update_operation.value()
            && operation.request.satisfies(&request)
        {
            return operation.promise.clone();
        } else if target.request.as_ref().is_some_and(|r| r.satisfies(&request)) {
            return Promise::resolved(true);
        }

        let update_ongoing = self.update_operation.value().is_some();
        self.update_operation.clear();

        let source = CancellationTokenSource::new(None);

        let run = FetchRun {
            providers,
            providers_label,
            context,
            active_inline_completion,
            with_debounce,
            user_jumped_to_active_completion,
            request_info,
            request: Rc::clone(&request),
            token: source.token(),
            update_ongoing,
        };
        let this = Rc::clone(self);
        let promise = async_fn(async move {
            let store = DisposableStore::new();

            this.loading_count.set(this.loading_count.get() + 1, None);
            let did_decrease = Rc::new(Cell::new(false));
            let decrease_loading_count: Rc<dyn Fn()> = {
                let (this, did_decrease) = (Rc::clone(&this), Rc::clone(&did_decrease));
                Rc::new(move || {
                    if !did_decrease.get() {
                        did_decrease.set(true);
                        this.loading_count.set(this.loading_count.get() - 1, None);
                    }
                })
            };
            let loading_reset = {
                let decrease_loading_count = Rc::clone(&decrease_loading_count);
                RunOnceScheduler::new(move || decrease_loading_count(), 10 * 1000)
            };
            store.add(Rc::clone(&loading_reset) as DisposableRef);
            loading_reset.schedule(None);

            let inline_suggestions_providers: Vec<Rc<dyn InlineCompletionsProvider>> =
                run.providers.iter().filter(|p| p.provider_id().is_some()).cloned().collect();
            let request_response_info = RequestResponseData::new(run.context.clone(), run.request_info.clone(), inline_suggestions_providers);

            let outcome = this.run(&run, &store, &request_response_info).await;

            store.dispose();
            decrease_loading_count();
            this.send_inline_completions_request_telemetry(&request_response_info);

            outcome
        });

        self.update_operation.set(Some(Rc::new(UpdateOperation { request, cancellation_token_source: source, promise: promise.clone() })));

        promise
    }

    /// O corpo do `try` do `fetch`: `Ok(false)` é o `return false` e `Ok(true)` é o fim do `try`.
    async fn run(self: &Rc<Self>, run: &FetchRun, store: &DisposableStore, info: &RequestResponseData) -> Result<bool, Error> {
        let context = &run.context;
        let token = &run.token;
        let request = &run.request;

        let recommended_debounce_value = self.debounce_value.get(&self.text_model);
        let debounce_value = find_last_max_defined(run.providers.iter().map(|p| p.debounce_delay_ms())).unwrap_or(recommended_debounce_value);

        // Debounce in any case if update is ongoing
        let should_debounce = run.update_ongoing || (run.with_debounce && context.trigger_kind == InlineCompletionTriggerKind::Automatic);
        if should_debounce {
            // This debounces the operation
            wait(debounce_value.max(0.0) as u64, Some(token)).await?;
        }

        if self.is_stale(token, request) {
            info.set_no_suggestion_reason_if_not_set("canceled:beforeFetch");
            return Ok(false);
        }

        let request_id = REQUEST_ID.with(|counter| {
            let current = counter.get();
            counter.set(current + 1);
            current
        });
        let logging = self.logging_enabled.get() || self.structured_fetch_logger.is_enabled().get();
        if logging {
            self.log(&FetchLogEntry::Start {
                request_id,
                model_uri: self.text_model.uri(),
                model_version: self.text_model.get_version_id(),
                context: Dynamic::object([
                    ("triggerKind", Dynamic::Number(f64::from(context.trigger_kind as u8))),
                    ("suggestInfo", if context.selected_suggestion_info.is_some() { Dynamic::Bool(true) } else { Dynamic::Undefined }),
                ]),
                time: date_now() as f64,
                provider: run.providers_label.clone(),
            });
        }

        let start_time = date_now();
        let provider_result = Rc::new(provide_inline_completions(
            &run.providers,
            self.cursor_position.get(),
            &self.text_model,
            context.clone(),
            run.request_info.clone(),
            Some(&self.language_configuration_service),
        ));

        let cancelled_result = Rc::clone(&provider_result);
        let _ = run_when_cancelled(token, move || {
            cancelled_result.cancel_and_dispose(InlineCompletionsDisposeReason { kind: InlineCompletionsDisposeReasonKind::TokenCancellation });
        });

        let mut should_stop_early = false;
        let mut produced_suggestion = false;

        let mut provider_suggestions: Vec<InlineSuggestionItem> = Vec::new();
        let iterator = provider_result.lists.async_iterator();
        while let Some(list) = iterator.next().await? {
            list.add_ref();
            {
                let list = Rc::clone(&list);
                store.add(to_disposable(move || {
                    list.remove_ref(Some(InlineCompletionsDisposeReason {
                        kind: if list.inline_suggestions_data().is_empty() {
                            InlineCompletionsDisposeReasonKind::Empty
                        } else {
                            InlineCompletionsDisposeReasonKind::NotTaken
                        },
                    }));
                }));
            }

            for item in list.inline_suggestions_data() {
                produced_suggestion = true;
                let is_edit_like = item.is_inline_edit || item.show_inline_edit_menu();
                if !context.include_inline_edits && is_edit_like {
                    item.set_not_shown_reason("notInlineEditRequested");
                    continue;
                }
                if !context.include_inline_completions && !is_edit_like {
                    item.set_not_shown_reason("notInlineCompletionRequested");
                    continue;
                }

                item.add_performance_marker("providerReturned");

                let target_uri = match item.action() {
                    Some(IInlineSuggestDataAction::Edit(action)) => action.uri,
                    Some(IInlineSuggestDataAction::JumpTo(action)) => action.uri,
                    None => None,
                };
                let model_reference: Option<ModelReference>;
                let target_model: Rc<TextModel>;

                match target_uri {
                    Some(target_uri) if target_uri.to_string(false) != self.text_model.uri().to_string(false) => {
                        let model_ref = self.text_model_service.create_model_reference(&target_uri).await?;
                        target_model = Rc::clone(&model_ref.object);
                        model_reference = Some(model_ref);
                    }
                    _ => {
                        target_model = Rc::clone(&self.text_model);
                        model_reference = None;
                    }
                }

                let text_ref = TextModelValueReference::snapshot(&target_model);

                let i = InlineSuggestionItem::create(&item, &text_ref, true);
                if let Some(model_reference) = model_reference {
                    let own: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
                    let own_in_callback = Rc::clone(&own);
                    let subscription = run_on_change::<(), ()>(i.base().identity.on_dispose(), move |_, _, _| {
                        model_reference.dispose();
                        let current = own_in_callback.borrow_mut().take();
                        if let Some(current) = current {
                            current.dispose();
                        }
                    });
                    *own.borrow_mut() = Some(subscription);
                }

                item.add_performance_marker("itemCreated");
                // Stop after first visible inline completion
                if let InlineSuggestionItem::Completion(completion) = &i
                    && !i.show_inline_edit_menu()
                    && context.trigger_kind == InlineCompletionTriggerKind::Automatic
                    && completion.is_visible(&self.text_model, self.cursor_position.get())
                {
                    should_stop_early = true;
                }
                provider_suggestions.push(i);
            }

            if should_stop_early {
                if let Some(returned) = iterator.return_() {
                    returned.await?;
                }
                break;
            }
        }

        for s in &provider_suggestions {
            s.add_performance_marker("providersResolved");
        }

        let renamed = Promise::all(
            provider_suggestions
                .iter()
                .map(|s| self.rename_processor.propose_rename_refactoring(&self.text_model, s.clone(), context))
                .collect(),
        )
        .await?;
        let suggestions: Vec<InlineSuggestionItem> = renamed;

        for s in &suggestions {
            s.add_performance_marker("renameProcessed");
        }

        provider_result.cancel_and_dispose(InlineCompletionsDisposeReason { kind: InlineCompletionsDisposeReasonKind::LostRace });

        if logging {
            let error = self.is_stale(token, request).then(|| "canceled".to_string());
            let result: Vec<Dynamic> = suggestions
                .iter()
                .filter_map(|c| {
                    let provider_id = c.source().provider.provider_id().map(|p| p.to_string());
                    completion_log_object(&c.get_source_completion(), provider_id)
                })
                .collect();
            self.log(&FetchLogEntry::End {
                request_id,
                duration_ms: date_now().saturating_sub(start_time) as f64,
                error,
                result,
                time: date_now() as f64,
                did_all_providers_return: provider_result.did_all_providers_return(),
            });
        }

        info.set_request_uuid(&provider_result.context_with_uuid.request_uuid);
        if produced_suggestion {
            info.set_has_produced_suggestion();
            if !suggestions.is_empty() && token.is_cancellation_requested() {
                for s in &suggestions {
                    s.set_not_shown_reason_if_not_set("canceled:whileAwaitingOtherProviders");
                }
            }
        } else if token.is_cancellation_requested() {
            info.set_no_suggestion_reason_if_not_set("canceled:whileFetching");
        } else {
            let completions_quota_exceeded = is_truthy(&self.context_key_service.get_context_key_value("completionsQuotaExceeded"));
            info.set_no_suggestion_reason_if_not_set(if completions_quota_exceeded { "completionsQuotaExceeded" } else { "noSuggestion" });
        }

        let remaining_time_to_wait = context.earliest_shown_date_time - date_now() as f64;
        if remaining_time_to_wait > 0.0 {
            wait(remaining_time_to_wait as u64, Some(token)).await?;
        }

        for s in &suggestions {
            s.add_performance_marker("minShowDelayPassed");
        }

        if self.is_stale(token, request) || run.user_jumped_to_active_completion.get() {
            /* In the meantime the user showed interest for the active completion so dont hide it */
            let not_shown_reason = if token.is_cancellation_requested() {
                "canceled:afterMinShowDelay"
            } else if self.disposed.get() {
                "canceled:disposed"
            } else if self.text_model.get_version_id() != request.version_id {
                "canceled:documentChanged"
            } else if run.user_jumped_to_active_completion.get() {
                "canceled:userJumped"
            } else {
                "unknown"
            };
            for s in &suggestions {
                s.set_not_shown_reason_if_not_set(not_shown_reason);
            }
            return Ok(false);
        }

        let end_time = date_now();
        self.debounce_value.update(&self.text_model, end_time.saturating_sub(start_time) as f64);

        let cursor_position = self.cursor_position.get();
        self.update_operation.clear();
        transaction(|tx: &dyn ITransaction| {
            /* @description Update completions with provider result */
            let v = self.state.get();

            if context.selected_suggestion_info.is_some() {
                self.state.set(
                    SourceState {
                        inline_completions: Rc::new(InlineCompletionsState::create_empty()),
                        suggest_widget_inline_completions: Rc::new(v.suggest_widget_inline_completions.create_state_with_applied_results(
                            suggestions.clone(),
                            Rc::clone(request),
                            &self.text_model,
                            cursor_position,
                            run.active_inline_completion.as_ref(),
                        )),
                    },
                    Some(tx),
                );
            } else {
                self.state.set(
                    SourceState {
                        inline_completions: Rc::new(v.inline_completions.create_state_with_applied_results(
                            suggestions.clone(),
                            Rc::clone(request),
                            &self.text_model,
                            cursor_position,
                            run.active_inline_completion.as_ref(),
                        )),
                        suggest_widget_inline_completions: Rc::new(InlineCompletionsState::create_empty()),
                    },
                    Some(tx),
                );
            }

            v.dispose();
        });

        Ok(true)
    }

    /// `source.token.isCancellationRequested || this._store.isDisposed || this._textModel.getVersionId() !== request.versionId`.
    fn is_stale(&self, token: &CancellationToken, request: &UpdateRequest) -> bool {
        token.is_cancellation_requested() || self.disposed.get() || self.text_model.get_version_id() != request.version_id
    }

    /// `clear(tx)`.
    pub fn clear(&self, tx: &dyn ITransaction) {
        if self.disposed.get() {
            return;
        }
        self.update_operation.clear();
        let v = self.state.get();
        self.state.set(SourceState::empty(), Some(tx));
        v.dispose();
    }

    /// `seedInlineCompletionsWithSuggestWidget()`.
    pub fn seed_inline_completions_with_suggest_widget(&self) {
        let inline_completions = self.inline_completions.get();
        let suggest_widget_inline_completions = self.suggest_widget_inline_completions.get();
        transaction(|tx: &dyn ITransaction| {
            /* @description Seed inline completions with (newer) suggest widget inline completions */
            let suggest_version = suggest_widget_inline_completions.request.as_ref().map_or(-1, |r| r.version_id);
            let inline_version = inline_completions.request.as_ref().map_or(-1, |r| r.version_id);
            if suggest_version > inline_version {
                inline_completions.dispose();
                let s = self.state.get();
                self.state.set(
                    SourceState {
                        inline_completions: Rc::new(suggest_widget_inline_completions.clone_state()),
                        suggest_widget_inline_completions: Rc::new(InlineCompletionsState::create_empty()),
                    },
                    Some(tx),
                );
                s.dispose();
            }
            self.clear_suggest_widget_inline_completions(tx);
        });
    }

    /// Seeds the inline completions with an external inline completion item.
    /// Used when transplanting a completion from one model to another (cross-file edits).
    pub fn seed_with_completion(&self, item: InlineSuggestionItem, tx: &dyn ITransaction) {
        let s = self.state.get();
        self.state.set(
            SourceState {
                inline_completions: Rc::new(InlineCompletionsState::new(vec![item], None)),
                suggest_widget_inline_completions: Rc::new(InlineCompletionsState::create_empty()),
            },
            Some(tx),
        );
        s.dispose();
    }

    /// `_sendInlineCompletionsRequestTelemetry(requestResponseInfo)`.
    fn send_inline_completions_request_telemetry(&self, info: &RequestResponseData) {
        if !self.send_request_data.get() && !is_truthy(&self.context_key_service.get_context_key_value("isRunningUnificationExperiment")) {
            return;
        }

        let Some(request_uuid) = info.request_uuid.borrow().clone() else {
            return;
        };
        if info.has_produced_suggestion.get() {
            return;
        }

        if !is_completions_enabled_from_object(self.completions_enabled.borrow().as_ref(), &self.text_model.get_language_id()) {
            return;
        }

        let is_copilot_like = |p: &Rc<dyn InlineCompletionsProvider>| {
            self.telemetry.is_copilot_like_extension(p.provider_id().and_then(|id| id.extension_id.as_deref()))
        };
        if !info.providers.iter().any(is_copilot_like) {
            return;
        }

        let request_info = &info.request_info;
        let empty_end_of_life_event = EmptyResponseTelemetryEvent {
            opportunity_id: request_uuid,
            no_suggestion_reason: info.no_suggestion_reason.borrow().clone().unwrap_or_else(|| "unknown".to_string()),
            sku_plan: request_info.sku.as_ref().map(|sku| sku.plan.clone()),
            sku_type: request_info.sku.as_ref().map(|sku| sku.kind.clone()),
            editor_type: request_info.editor_type.as_str().to_string(),
            request_reason: request_info.reason.clone(),
            typing_interval: request_info.typing_interval,
            typing_interval_character_count: request_info.typing_interval_character_count,
            language_id: request_info.language_id.clone(),
            selected_suggestion_info: info.context.selected_suggestion_info.is_some(),
            available_providers: info
                .providers
                .iter()
                .filter_map(|p| p.provider_id().map(|id| id.to_string()))
                .collect::<Vec<_>>()
                .join(","),
            forward_to_channel: info.providers.iter().any(is_copilot_like),
        };

        self.telemetry.send_empty_response(empty_end_of_life_event);
    }

    /// `clearSuggestWidgetInlineCompletions(tx)`.
    pub fn clear_suggest_widget_inline_completions(&self, _tx: &dyn ITransaction) {
        if self.update_operation.value().is_some_and(|operation| operation.request.context.selected_suggestion_info.is_some()) {
            self.update_operation.clear();
        }
    }

    /// `cancelUpdate()`.
    pub fn cancel_update(&self) {
        self.update_operation.clear();
    }
}

impl IDisposable for InlineCompletionsSource {
    fn dispose(&self) {
        self.disposed.set(true);
        self.base.dispose();
    }
}

/// `value ?? defaultValue` para os booleanos de configuração.
fn as_bool(value: &Dynamic) -> Option<bool> {
    match value {
        Dynamic::Bool(b) => Some(*b),
        _ => None,
    }
}

/// O descarte de um colaborador compartilhado como `DisposableRef` (o `Rc<dyn Trait>` não vira `Rc<dyn IDisposable>`
/// sem o laço de conversão).
fn dispose_handle<T: IDisposable + ?Sized + 'static>(target: &Rc<T>) -> DisposableRef {
    let target = Rc::clone(target);
    plain_disposable(move || target.dispose())
}

/// `_updateCompletionsEnablement(enalementSetting)`.
fn update_completions_enablement(
    configuration_service: &Rc<dyn IConfigurationService>,
    setting: &str,
    completions_enabled: &Rc<RefCell<Option<Dynamic>>>,
) {
    let result = configuration_service.get_value(Some(setting), None);
    *completions_enabled.borrow_mut() = if matches!(result, Dynamic::Object(_)) { Some(result) } else { None };
}

/// `new Set(providers)`: provedores por identidade.
fn provider_addresses(providers: &[Rc<dyn InlineCompletionsProvider>]) -> HashSet<usize> {
    providers.iter().map(|p| Rc::as_ptr(p).cast::<()>() as usize).collect()
}

/// `findLastMax(delays, compareUndefinedSmallest(numberComparator))`: o maior atraso definido (`undefined` é o menor,
/// então só sobra `undefined` quando nenhum provedor define atraso).
fn find_last_max_defined(delays: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    let mut best: Option<f64> = None;
    for delay in delays {
        match (best, delay) {
            (Some(current), Some(candidate)) if candidate >= current => best = Some(candidate),
            (None, Some(candidate)) => best = Some(candidate),
            _ => {}
        }
    }
    best
}

/// O objeto de log de uma sugestão, ou `None` quando o provedor marcou `doNotLog`.
fn completion_log_object(comp: &InlineCompletion, provider_id: Option<String>) -> Option<Dynamic> {
    if comp.do_not_log.unwrap_or(false) {
        return None;
    }
    let range_text = |range: &Range| Dynamic::string(&range.to_string());
    let command = |command: &Command| Dynamic::object([("$commandId", Dynamic::string(&command.id))]);
    let optional = |value: Option<Dynamic>| value.unwrap_or(Dynamic::Undefined);

    Some(Dynamic::object([
        (
            "insertText",
            optional(comp.insert_text.as_ref().map(|text| match text {
                InlineCompletionInsertText::Text(text) => Dynamic::string(text),
                InlineCompletionInsertText::Snippet(snippet) => Dynamic::object([("snippet", Dynamic::string(snippet))]),
            })),
        ),
        ("range", optional(comp.range.as_ref().map(range_text))),
        (
            "additionalTextEdits",
            optional(comp.additional_text_edits.as_ref().map(|edits| {
                Dynamic::array(edits.iter().map(|edit| {
                    Dynamic::object([
                        ("range", range_text(&edit.range)),
                        ("text", optional(edit.text.clone().map(Dynamic::String))),
                    ])
                }))
            })),
        ),
        ("uri", optional(comp.uri.as_ref().map(uri_components_object))),
        ("command", optional(comp.command.as_ref().map(command))),
        ("gutterMenuLinkAction", optional(comp.gutter_menu_link_action.as_ref().map(command))),
        ("shownCommand", optional(comp.shown_command.as_ref().map(command))),
        ("completeBracketPairs", optional(comp.complete_bracket_pairs.map(Dynamic::Bool))),
        ("isInlineEdit", optional(comp.is_inline_edit.map(Dynamic::Bool))),
        ("showInlineEditMenu", optional(comp.show_inline_edit_menu.map(Dynamic::Bool))),
        ("showRange", optional(comp.show_range.as_ref().map(range_text))),
        ("warning", optional(comp.warning.as_ref().map(|warning| Dynamic::string(&format!("{warning:?}"))))),
        ("hint", optional(comp.hint.as_ref().map(|hint| Dynamic::string(&format!("{hint:?}"))))),
        ("supportsRename", optional(comp.supports_rename.map(Dynamic::Bool))),
        ("correlationId", optional(comp.correlation_id.as_deref().map(Dynamic::string))),
        ("jumpToPosition", optional(comp.jump_to_position.map(|position| Dynamic::string(&position.to_string())))),
        ("$providerId", optional(provider_id.as_deref().map(Dynamic::string))),
    ]))
}

/// Os componentes definidos de uma `URI`, na ordem do `toJSON`.
fn uri_components_object(components: &UriComponents) -> Dynamic {
    let fields = [
        ("scheme", &components.scheme),
        ("authority", &components.authority),
        ("path", &components.path),
        ("query", &components.query),
        ("fragment", &components.fragment),
    ];
    Dynamic::object(fields.into_iter().filter_map(|(key, value)| value.as_ref().map(|value: &JsString| (key, Dynamic::String(value.clone())))))
}

/// Os argumentos de uma chamada de `fetch`, vistos pelo corpo assíncrono.
struct FetchRun {
    providers: Vec<Rc<dyn InlineCompletionsProvider>>,
    providers_label: Option<String>,
    context: InlineCompletionContextWithoutUuid,
    active_inline_completion: Option<Rc<InlineSuggestionIdentity>>,
    with_debounce: bool,
    user_jumped_to_active_completion: IObservable<bool>,
    request_info: InlineSuggestRequestInfo,
    request: Rc<UpdateRequest>,
    token: CancellationToken,
    update_ongoing: bool,
}

/// `UpdateRequest`.
pub struct UpdateRequest {
    pub position: Position,
    pub context: InlineCompletionContextWithoutUuid,
    pub version_id: i64,
    providers: HashSet<usize>,
}

impl UpdateRequest {
    fn new(position: Position, context: InlineCompletionContextWithoutUuid, version_id: i64, providers: HashSet<usize>) -> Self {
        Self { position, context, version_id, providers }
    }

    /// `satisfies(other)`.
    pub fn satisfies(&self, other: &UpdateRequest) -> bool {
        self.position == other.position
            && self.context.selected_suggestion_info == other.context.selected_suggestion_info
            && (other.context.trigger_kind == InlineCompletionTriggerKind::Automatic
                || self.context.trigger_kind == InlineCompletionTriggerKind::Explicit)
            && self.version_id == other.version_id
            && other.providers.is_subset(&self.providers)
    }

    /// `isExplicitRequest`.
    pub fn is_explicit_request(&self) -> bool {
        self.context.trigger_kind == InlineCompletionTriggerKind::Explicit
    }
}

/// `RequestResponseData`.
struct RequestResponseData {
    request_uuid: RefCell<Option<String>>,
    no_suggestion_reason: RefCell<Option<String>>,
    has_produced_suggestion: Cell<bool>,
    context: InlineCompletionContextWithoutUuid,
    request_info: InlineSuggestRequestInfo,
    providers: Vec<Rc<dyn InlineCompletionsProvider>>,
}

impl RequestResponseData {
    fn new(context: InlineCompletionContextWithoutUuid, request_info: InlineSuggestRequestInfo, providers: Vec<Rc<dyn InlineCompletionsProvider>>) -> Self {
        Self {
            request_uuid: RefCell::new(None),
            no_suggestion_reason: RefCell::new(None),
            has_produced_suggestion: Cell::new(false),
            context,
            request_info,
            providers,
        }
    }

    fn set_request_uuid(&self, uuid: &str) {
        *self.request_uuid.borrow_mut() = Some(uuid.to_string());
    }

    fn set_no_suggestion_reason_if_not_set(&self, reason: &str) {
        let mut current = self.no_suggestion_reason.borrow_mut();
        if current.is_none() {
            *current = Some(reason.to_string());
        }
    }

    fn set_has_produced_suggestion(&self) {
        self.has_produced_suggestion.set(true);
    }
}

/// `UpdateOperation`.
struct UpdateOperation {
    request: Rc<UpdateRequest>,
    cancellation_token_source: CancellationTokenSource,
    promise: Promise<bool>,
}

impl IDisposable for UpdateOperation {
    fn dispose(&self) {
        self.cancellation_token_source.cancel();
    }
}

/// `InlineCompletionsState`.
pub struct InlineCompletionsState {
    pub inline_completions: Vec<InlineSuggestionItem>,
    pub request: Option<Rc<UpdateRequest>>,
    disposed: Cell<bool>,
}

impl InlineCompletionsState {
    pub fn create_empty() -> InlineCompletionsState {
        InlineCompletionsState::new(Vec::new(), None)
    }

    pub fn new(inline_completions: Vec<InlineSuggestionItem>, request: Option<Rc<UpdateRequest>>) -> Self {
        for inline_completion in &inline_completions {
            inline_completion.add_ref();
        }
        Self { inline_completions, request, disposed: Cell::new(false) }
    }

    fn find_by_id(&self, id: &Rc<InlineSuggestionIdentity>) -> Option<&InlineSuggestionItem> {
        self.inline_completions.iter().find(|i| Rc::ptr_eq(&i.base().identity, id))
    }

    fn find_by_hash(&self, hash: &str) -> Option<&InlineSuggestionItem> {
        self.inline_completions.iter().find(|i| i.hash() == hash)
    }

    /// Applies the edit on the state.
    pub fn create_state_with_applied_edit(&self, edit: &StringEdit, text_model: &Rc<TextModel>) -> InlineCompletionsState {
        let new_inline_completions = self.inline_completions.iter().filter_map(|i| i.with_edit(edit, text_model)).collect();
        InlineCompletionsState::new(new_inline_completions, self.request.clone())
    }

    pub fn create_state_with_applied_results(
        &self,
        mut updated_suggestions: Vec<InlineSuggestionItem>,
        request: Rc<UpdateRequest>,
        text_model: &Rc<TextModel>,
        cursor_position: Position,
        item_id_to_preserve_at_top: Option<&Rc<InlineSuggestionIdentity>>,
    ) -> InlineCompletionsState {
        let mut item_to_preserve: Option<&InlineSuggestionItem> = None;
        if let Some(item_id_to_preserve_at_top) = item_id_to_preserve_at_top
            && let Some(item_to_preserve_candidate) = self.find_by_id(item_id_to_preserve_at_top)
            && item_to_preserve_candidate.can_be_reused(text_model, request.position)
        {
            item_to_preserve = Some(item_to_preserve_candidate);

            let candidate_hash = item_to_preserve_candidate.hash();
            match updated_suggestions.iter().position(|i| i.hash() == candidate_hash) {
                Some(index) => {
                    let updated_item_to_preserve = updated_suggestions.remove(index);
                    updated_suggestions.insert(0, updated_item_to_preserve);
                }
                None => updated_suggestions.insert(0, item_to_preserve_candidate.clone()),
            }
        }

        let prefer_inline_completions = match item_to_preserve {
            // itemToPreserve has precedence
            Some(item) => !item.is_inline_edit(),
            // Otherwise: prefer inline completion if there is a visible one
            None => updated_suggestions.iter().any(|i| match i {
                InlineSuggestionItem::Completion(completion) => completion.is_visible(text_model, cursor_position),
                InlineSuggestionItem::Edit(_) => false,
            }),
        };

        let mut updated_items: Vec<InlineSuggestionItem> = Vec::new();
        for i in updated_suggestions {
            let old_item = self.find_by_hash(&i.hash());
            let item = match old_item {
                Some(old_item) if !Rc::ptr_eq(&old_item.base().identity, &i.base().identity) => {
                    let item = i.with_identity(Rc::clone(&old_item.base().identity));
                    i.set_is_preceeded(old_item);
                    old_item.set_end_of_life_reason(InlineCompletionEndOfLifeReason::Ignored {
                        superseded_by: Some(i.get_source_completion()),
                        user_typing_disagreed: false,
                    });
                    item
                }
                _ => i,
            };
            if prefer_inline_completions != item.is_inline_edit() {
                updated_items.push(item);
            }
        }

        updated_items.sort_by_key(|i| i.show_inline_edit_menu());
        let updated_items = distinct_by_key(updated_items, |i| i.semantic_id());

        InlineCompletionsState::new(updated_items, Some(request))
    }

    /// `clone()`.
    pub fn clone_state(&self) -> InlineCompletionsState {
        InlineCompletionsState::new(self.inline_completions.clone(), self.request.clone())
    }
}

impl IDisposable for InlineCompletionsState {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        for inline_completion in &self.inline_completions {
            inline_completion.remove_ref();
        }
    }
}

/// Keeps the first item in case of duplicates.
fn distinct_by_key<T, K: std::hash::Hash + Eq>(items: Vec<T>, key: impl Fn(&T) -> K) -> Vec<T> {
    let mut seen = HashSet::new();
    items.into_iter().filter(|item| seen.insert(key(item))).collect()
}
