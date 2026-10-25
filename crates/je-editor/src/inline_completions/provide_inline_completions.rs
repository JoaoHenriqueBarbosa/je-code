//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/model/provideInlineCompletions.ts`: a consulta aos
//! provedores de completação em linha (`provideInlineCompletions`), o dado de cada sugestão (`InlineSuggestData`) e a
//! lista com contagem de referências (`InlineSuggestionList`).
//!
//! Notas de tradução:
//! - `async`/`await` é [`async_fn`] sobre [`Promise`]; o `finally { runningCount-- }` é um guarda com `Drop`;
//! - o `CachedFunction` do `queryProvider` chama a si mesmo (para os `yieldsToGroupIds`), então o estado do pedido
//!   é um `Rc` criado com `new_cyclic` e a função do cache guarda um `Weak` dele;
//! - provedores são comparados por identidade (o `Map` do JavaScript): [`ProviderKey`] usa o endereço do `Rc`;
//! - `InlineCompletionContextWithoutUuid` (`Omit<InlineCompletionContext, 'requestUuid'>`) é o próprio contexto, cujo
//!   `request_uuid` é sobrescrito pelo uuid do pedido;
//! - o `InlineSuggestData` é compartilhado pelos itens (`withIdentity` reaproveita o mesmo dado), então vive em `Rc`
//!   com o estado mutável num `RefCell`; `withAction` devolve o próprio `Rc` (o `this` do original);
//! - a lista guarda os dados que ela mesma criou, e cada dado aponta de volta para a lista: ao chegar a zero
//!   referências a lista solta os dados (depois de avisar o fim de vida de cada um), o que desfaz o ciclo de `Rc`;
//! - o original troca o `snippet` do `InlineCompletion` do provedor por uma versão com os colchetes fechados. O item é
//!   um `Rc` imutável e a identidade dele é o que o `handleEndOfLifetime` do provedor espera, então o fechamento só
//!   aparece no `SnippetInfo` e no `insert_text` da ação, e o item de origem segue como o provedor o devolveu;
//! - os textos da ação são [`JsString`] (UTF-16), como o `string` do original; tempos são milissegundos em `f64`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::{AsyncIterableProducer, async_fn};
use je_base::cache::CachedFunction;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::{Error, on_unexpected_error, on_unexpected_external_error};
use je_base::lifecycle::{DisposableRef, none, plain_disposable};
use je_base::promise::Promise;
use je_base::runtime::date_now;
use je_base::uri::{URI, UriState};
use je_base::uuid::prefixed_uuid;
use je_platform::commands::commands::ICommandService;
use je_platform::resources::host_platform;

use crate::code_action::languages::ProviderId;
use crate::cursor_common::CursorSimpleModel;
use crate::edit_operation::SingleEditOperation;
use crate::edits::string_edit::{BaseStringReplacement, StringReplacement};
use crate::edits::text_edit::TextReplacement;
use crate::inline_completions::edit_kind::InlineSuggestionEditKind;
use crate::inline_completions::graph::DirectedGraph;
use crate::inline_completions::inline_completion_is_visible::inline_completion_is_visible;
use crate::inline_completions::inline_completion_view_data::{InlineCompletionViewData, InlineCompletionViewKind};
use crate::inline_completions::inline_suggest_alternative_action::InlineSuggestAlternativeAction;
use crate::inline_completions::utils::ErrorResult;
use crate::bracket_pairs_text_model_part::fix_brackets::fix_brackets_in_line;
use crate::language_configuration_registry::ILanguageConfigurationService;
use crate::languages::{
    IInlineCompletionHint, InlineCompletion, InlineCompletionContext, InlineCompletionEndOfLifeReason,
    InlineCompletionEndOfLifeReasonKind, InlineCompletionInsertText, InlineCompletionTriggerKind, InlineCompletions,
    InlineCompletionsDisposeReason, InlineCompletionsDisposeReasonKind, InlineCompletionsProvider, LifetimeSummary,
    PartialAcceptInfo,
};
use crate::line_tokens::{IViewLineTokens, LineTokens};
use crate::offset_range::OffsetRange;
use crate::position::Position;
use crate::range::Range;
use crate::snippet::snippet_parser::SnippetParser;
use crate::text_model::TextModel;
use crate::text_model_edit_source::EditDeltaInfo;

/// `InlineCompletionContextWithoutUuid`: o contexto cujo `request_uuid` o pedido sobrescreve.
pub type InlineCompletionContextWithoutUuid = InlineCompletionContext;

/// O que um provedor devolve já convertido: a lista de sugestões, ou `None`.
type SuggestionListPromise = Promise<Option<Rc<InlineSuggestionList>>>;

/// O provedor como chave de mapa: igualdade e hash pela identidade do objeto.
#[derive(Clone)]
struct ProviderKey(Rc<dyn InlineCompletionsProvider>);

impl ProviderKey {
    fn address(&self) -> usize {
        Rc::as_ptr(&self.0).cast::<()>() as usize
    }
}

impl PartialEq for ProviderKey {
    fn eq(&self, other: &Self) -> bool {
        self.address() == other.address()
    }
}

impl Eq for ProviderKey {}

impl Hash for ProviderKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.address().hash(state);
    }
}

/// `Date.now()`.
fn now_ms() -> f64 {
    date_now() as f64
}

/// O estado de um pedido a todos os provedores.
struct RequestState {
    cancellation_token_source: CancellationTokenSource,
    cancel_reason: Rc<Cell<Option<InlineCompletionsDisposeReason>>>,
    running_count: Cell<i32>,
    yields_to_graph: DirectedGraph<ProviderKey>,
    default_replace_range: Range,
    model: Rc<TextModel>,
    position: Position,
    context_with_uuid: InlineCompletionContext,
    request_info: InlineSuggestRequestInfo,
    language_configuration_service: Option<Rc<dyn ILanguageConfigurationService>>,
    query_provider: CachedFunction<ProviderKey, SuggestionListPromise>,
}

/// `runningCount--` no `finally`.
struct RunningGuard(Rc<RequestState>);

impl Drop for RunningGuard {
    fn drop(&mut self) {
        self.0.running_count.set(self.0.running_count.get() - 1);
    }
}

impl RequestState {
    /// O corpo de `queryProvider`.
    fn query(state: Rc<RequestState>, provider: ProviderKey) -> SuggestionListPromise {
        async_fn(async move {
            state.running_count.set(state.running_count.get() + 1);
            let _guard = RunningGuard(Rc::clone(&state));
            let token = state.cancellation_token_source.token();
            if token.is_cancellation_requested() {
                return Ok(None);
            }

            let yields_to = state.yields_to_graph.get_outgoing(&provider);
            for other in yields_to {
                // We know there is no cycle, so no recursion here
                let result = state.query_provider.get(other).await?;
                if let Some(result) = result {
                    for item in &result.inline_suggestions.items {
                        if item.is_inline_edit.unwrap_or(false) || matches!(item.insert_text, Some(InlineCompletionInsertText::Snippet(_))) {
                            return Ok(None);
                        }
                        if let Some(InlineCompletionInsertText::Text(text)) = &item.insert_text {
                            let replacement = TextReplacement::new(item.range.unwrap_or(state.default_replace_range), text.as_str());
                            if inline_completion_is_visible(&replacement, None, &state.model, state.position) {
                                return Ok(None);
                            }
                        }

                        // else: inline completion is not visible, so lets not block
                    }
                }
            }

            let provider_start_time = now_ms();
            let result = match provider.0.provide_inline_completions(&state.model, state.position, &state.context_with_uuid, &token).await {
                Ok(result) => result,
                Err(error) => {
                    on_unexpected_external_error(&error);
                    return Ok(None);
                }
            };
            let provider_end_time = now_ms();

            let Some(result) = result else {
                return Ok(None);
            };

            let list = Rc::new(InlineSuggestionList::new(Rc::clone(&result), Rc::clone(&provider.0)));
            list.add_ref();
            let cancelled_list = Rc::clone(&list);
            let cancel_reason = Rc::clone(&state.cancel_reason);
            run_when_cancelled(&token, move || cancelled_list.remove_ref(cancel_reason.get()));
            if token.is_cancellation_requested() {
                return Ok(None); // The list is disposed now, so we cannot return the items!
            }

            for item in &result.items {
                let data = to_inline_suggest_data(
                    item,
                    &list,
                    state.default_replace_range,
                    &state.model,
                    state.language_configuration_service.as_ref(),
                    &state.context_with_uuid,
                    &state.request_info,
                    InlineSuggestProviderRequestInfo { start_time: provider_start_time, end_time: provider_end_time },
                );
                match data {
                    Ok(data) => list.add_data(data),
                    Err(error) => error.log_error(),
                }
            }

            Ok(Some(list))
        })
    }
}

/// `provideInlineCompletions(providers, position, model, context, requestInfo, languageConfigurationService?)`.
pub fn provide_inline_completions(
    providers: &[Rc<dyn InlineCompletionsProvider>],
    position: Position,
    model: &Rc<TextModel>,
    context: InlineCompletionContextWithoutUuid,
    request_info: InlineSuggestRequestInfo,
    language_configuration_service: Option<&Rc<dyn ILanguageConfigurationService>>,
) -> IInlineCompletionProviderResult {
    let request_uuid = prefixed_uuid("icr");

    let mut context_with_uuid = context;
    context_with_uuid.request_uuid = request_uuid;

    let default_replace_range = get_default_range(position, model);

    let keys: Vec<ProviderKey> = providers.iter().map(|p| ProviderKey(Rc::clone(p))).collect();
    let mut providers_by_group_id: HashMap<String, Vec<ProviderKey>> = HashMap::new();
    for key in &keys {
        if let Some(group_id) = key.0.group_id() {
            providers_by_group_id.entry(group_id.to_string()).or_default().push(key.clone());
        }
    }
    let mut yields_to_graph = DirectedGraph::from(&keys, |p| {
        p.0.yields_to_group_ids()
            .map(|group_ids| group_ids.iter().flat_map(|group_id| providers_by_group_id.get(group_id).cloned().unwrap_or_default()).collect())
            .unwrap_or_default()
    });
    let found_cycles = yields_to_graph.remove_cycles();
    if !found_cycles.is_empty() {
        // O `toString` do objeto no original: o `[object Object]` quando o provedor não define o dele.
        let path: Vec<String> =
            found_cycles.iter().map(|p| p.0.to_display_string().unwrap_or_else(|| "[object Object]".to_string())).collect();
        on_unexpected_external_error(&Error::new(format!(
            "Inline completions: cyclic yield-to dependency detected. Path: {}",
            path.join(" -> ")
        )));
    }

    let model = Rc::clone(model);
    let language_configuration_service = language_configuration_service.cloned();
    let state = Rc::new_cyclic(|weak: &Weak<RequestState>| {
        let weak = weak.clone();
        RequestState {
            cancellation_token_source: CancellationTokenSource::new(None),
            cancel_reason: Rc::new(Cell::new(None)),
            running_count: Cell::new(0),
            yields_to_graph,
            default_replace_range,
            model,
            position,
            context_with_uuid: context_with_uuid.clone(),
            request_info,
            language_configuration_service,
            query_provider: CachedFunction::new(move |provider| {
                let state = weak.upgrade().expect("o estado do pedido vive enquanto o cache é consultado");
                RequestState::query(state, provider)
            }),
        }
    });

    let promises: Vec<SuggestionListPromise> = keys.into_iter().map(|key| state.query_provider.get(key)).collect();
    let lists = AsyncIterableProducer::from_promises_resolve_order(promises).coalesce();

    IInlineCompletionProviderResult { context_with_uuid, state, lists }
}

/// If the token is eventually cancelled, this will not leak either.
pub fn run_when_cancelled(token: &CancellationToken, callback: impl FnOnce() + 'static) -> DisposableRef {
    if token.is_cancellation_requested() {
        callback();
        none()
    } else {
        let callback = RefCell::new(Some(callback));
        let listener: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
        let own = Rc::clone(&listener);
        let subscription = token.on_cancellation_requested().listen(move |_| {
            let current = own.borrow().clone();
            if let Some(current) = current {
                current.dispose();
            }
            if let Some(callback) = callback.borrow_mut().take() {
                callback();
            }
        });
        *listener.borrow_mut() = Some(subscription);
        plain_disposable(move || {
            let current = listener.borrow().clone();
            if let Some(current) = current {
                current.dispose();
            }
        })
    }
}

/// `IInlineCompletionProviderResult`.
pub struct IInlineCompletionProviderResult {
    pub context_with_uuid: InlineCompletionContext,
    state: Rc<RequestState>,
    /// As listas, na ordem em que os provedores respondem.
    pub lists: Rc<AsyncIterableProducer<Rc<InlineSuggestionList>>>,
}

impl IInlineCompletionProviderResult {
    /// `didAllProvidersReturn`.
    pub fn did_all_providers_return(&self) -> bool {
        self.state.running_count.get() == 0
    }

    /// `cancelAndDispose(reason)`: só o primeiro motivo vale.
    pub fn cancel_and_dispose(&self, reason: InlineCompletionsDisposeReason) {
        if self.state.cancel_reason.get().is_some() {
            return;
        }
        self.state.cancel_reason.set(Some(reason));
        self.state.cancellation_token_source.dispose_with(true);
    }
}

/// `closeBrackets(text, position, model, languageConfigurationService)`: fecha os colchetes que `text` deixa abertos
/// quando inserido em `position`, tokenizando a linha como ela ficaria depois da inserção.
fn close_brackets(
    text: &JsString,
    position: Position,
    model: &Rc<TextModel>,
    language_configuration_service: &Rc<dyn ILanguageConfigurationService>,
) -> JsString {
    let current_line = model.get_line_content(position.line_number);
    let edit = StringReplacement::replace(OffsetRange::new(position.column - 1, current_line.len() as i32), text.clone());

    let proposed_line = edit.replace(current_line.units());
    let proposed_line_tokens = model.tokenization().tokenize_lines_at(position.line_number, &[proposed_line]);
    let range_after_replace = OffsetRange::new(edit.replace_range.start, edit.replace_range.start + edit.new_text.len() as i32);
    let Some(text_tokens) = proposed_line_tokens.and_then(|tokens| tokens.into_iter().next()).map(|tokens| tokens.slice_zero_copy(range_after_replace))
    else {
        return text.clone();
    };

    // `fixBracketsInLine` lê o `LineTokens` concreto: a fatia vira um `LineTokens` com o mesmo texto e os mesmos fins.
    let mut ends_and_metadata = Vec::with_capacity(text_tokens.get_count() * 2);
    for index in 0..text_tokens.get_count() {
        ends_and_metadata.push(text_tokens.get_end_offset(index) as u32);
        ends_and_metadata.push(text_tokens.get_metadata(index));
    }
    let sliced = LineTokens::new(ends_and_metadata, text_tokens.get_line_content(), text_tokens.language_id_codec().clone());
    fix_brackets_in_line(&sliced, language_configuration_service)
}

/// Fecha os colchetes de `text` e, se o tamanho mudou, ajusta o fim de `range` pela diferença.
fn close_brackets_and_adjust_range(
    text: &JsString,
    range: Range,
    model: &Rc<TextModel>,
    language_configuration_service: &Rc<dyn ILanguageConfigurationService>,
) -> (JsString, Range) {
    let closed = close_brackets(text, range.get_start_position(), model, language_configuration_service);

    // Modify range depending on if brackets are added or removed
    let diff = closed.len() as i32 - text.len() as i32;
    let range = if diff != 0 { Range::new(range.start_line_number, range.start_column, range.end_line_number, range.end_column + diff) } else { range };
    (closed, range)
}

#[allow(clippy::too_many_arguments)]
fn to_inline_suggest_data(
    inline_completion: &Rc<InlineCompletion>,
    source: &Rc<InlineSuggestionList>,
    default_replace_range: Range,
    text_model: &Rc<TextModel>,
    language_configuration_service: Option<&Rc<dyn ILanguageConfigurationService>>,
    context: &InlineCompletionContext,
    request_info: &InlineSuggestRequestInfo,
    provider_request_info: InlineSuggestProviderRequestInfo,
) -> Result<Rc<InlineSuggestData>, ErrorResult> {
    let uri = inline_completion.uri.as_ref().map(|components| URI::revive(&UriState::from(components.clone()), host_platform()));

    let action = if let Some(position) = inline_completion.jump_to_position {
        Some(IInlineSuggestDataAction::JumpTo(IInlineSuggestDataActionJumpTo { position, uri }))
    } else if let Some(insert_text) = &inline_completion.insert_text {
        let mut range = inline_completion.range.unwrap_or(default_replace_range);
        let complete_bracket_pairs = inline_completion.complete_bracket_pairs.unwrap_or(false);
        let brackets_service = language_configuration_service.filter(|_| complete_bracket_pairs);

        let (insert_text, snippet_info) = match insert_text {
            InlineCompletionInsertText::Text(text) => {
                let mut insert_text = JsString::from(text.as_str());
                if let Some(service) = brackets_service {
                    (insert_text, range) = close_brackets_and_adjust_range(&insert_text, range, text_model, service);
                }
                (insert_text, None)
            }
            InlineCompletionInsertText::Snippet(snippet_text) => {
                let mut snippet_text = JsString::from(snippet_text.as_str());
                if let Some(service) = brackets_service {
                    (snippet_text, range) = close_brackets_and_adjust_range(&snippet_text, range, text_model, service);
                }

                let snippet = SnippetParser::new().parse(snippet_text.units(), false, false);
                let children = snippet.children();
                let only_text = match children.as_slice() {
                    [child] => child.as_text().map(|text| text.value.borrow().clone()),
                    _ => None,
                };
                match only_text {
                    Some(text) => (text, None),
                    None => (snippet.to_js_string(), Some(SnippetInfo { snippet: snippet_text, range })),
                }
            }
        };
        Some(IInlineSuggestDataAction::Edit(IInlineSuggestDataActionEdit { range, insert_text, snippet_info, uri, alternative_action: None }))
    } else {
        if inline_completion.hint.is_none() {
            return Err(ErrorResult::message("Inline completion has no insertText, jumpToPosition nor hint."));
        }
        None
    };

    Ok(Rc::new(InlineSuggestData::new(
        action,
        inline_completion.hint.clone(),
        inline_completion.additional_text_edits.clone().unwrap_or_default(),
        Rc::clone(inline_completion),
        Rc::clone(source),
        context.clone(),
        inline_completion.is_inline_edit.unwrap_or(false),
        inline_completion.supports_rename.unwrap_or(false),
        request_info.clone(),
        provider_request_info,
        inline_completion.correlation_id.clone(),
    )))
}

/// `InlineSuggestSku`: `type` e `plan`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineSuggestSku {
    /// O `type` do original.
    pub kind: String,
    pub plan: String,
}

/// `InlineSuggestRequestInfo`.
#[derive(Clone, Debug)]
pub struct InlineSuggestRequestInfo {
    pub start_time: f64,
    pub editor_type: InlineCompletionEditorType,
    pub language_id: String,
    pub reason: String,
    pub typing_interval: f64,
    pub typing_interval_character_count: i32,
    pub available_providers: Vec<ProviderId>,
    pub sku: Option<InlineSuggestSku>,
}

/// `InlineSuggestProviderRequestInfo`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InlineSuggestProviderRequestInfo {
    pub start_time: f64,
    pub end_time: f64,
}

/// `PartialAcceptance`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PartialAcceptance {
    pub characters: i32,
    pub count: i32,
    pub ratio: f64,
}

/// `RenameInfo`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenameInfo {
    pub created_rename: bool,
    pub duration: f64,
    pub timed_out: Option<bool>,
    pub dropped_other_edits: Option<i32>,
    pub dropped_rename_edits: Option<i32>,
}

/// `InlineSuggestViewData`.
#[derive(Clone, Debug)]
pub struct InlineSuggestViewData {
    pub editor_type: InlineCompletionEditorType,
    pub render_data: Option<InlineCompletionViewData>,
    pub view_kind: Option<InlineCompletionViewKind>,
}

/// `IInlineSuggestDataAction`.
#[derive(Clone)]
pub enum IInlineSuggestDataAction {
    Edit(IInlineSuggestDataActionEdit),
    JumpTo(IInlineSuggestDataActionJumpTo),
}

/// `IInlineSuggestDataActionEdit`.
#[derive(Clone)]
pub struct IInlineSuggestDataActionEdit {
    pub range: Range,
    pub insert_text: JsString,
    pub snippet_info: Option<SnippetInfo>,
    pub uri: Option<URI>,
    pub alternative_action: Option<InlineSuggestAlternativeAction>,
}

/// `IInlineSuggestDataActionJumpTo`.
#[derive(Clone)]
pub struct IInlineSuggestDataActionJumpTo {
    pub position: Position,
    pub uri: Option<URI>,
}

/// O estado mutável de um [`InlineSuggestData`].
struct DataState {
    action: Option<IInlineSuggestDataAction>,
    did_show: bool,
    time_until_shown: Option<f64>,
    time_until_actually_shown: Option<f64>,
    show_start_time: Option<f64>,
    shown_duration: f64,
    show_uncollapsed_start_time: Option<f64>,
    show_uncollapsed_duration: f64,
    not_shown_reason: Option<String>,
    view_data: InlineSuggestViewData,
    did_report_end_of_life: bool,
    last_set_end_of_life_reason: Option<InlineCompletionEndOfLifeReason>,
    is_preceeded: bool,
    partially_accepted_count: i32,
    partially_accepted_since_original: PartialAcceptance,
    rename_info: Option<RenameInfo>,
    edit_kind: Option<InlineSuggestionEditKind>,
    performance: InlineSuggestionsPerformance,
}

impl DataState {
    fn update_shown_duration(&mut self, view_kind: InlineCompletionViewKind) {
        let time_now = now_ms();
        if self.show_start_time.is_none_or(|time| time == 0.0) {
            self.show_start_time = Some(time_now);
        }

        let is_collapsed = view_kind == InlineCompletionViewKind::Collapsed;
        if !is_collapsed && self.show_uncollapsed_start_time.is_none() {
            self.show_uncollapsed_start_time = Some(time_now);
        }

        if is_collapsed && let Some(start) = self.show_uncollapsed_start_time {
            self.show_uncollapsed_duration += time_now - start;
        }
    }

    fn report_inline_edit_hidden(&mut self) {
        let Some(show_start_time) = self.show_start_time else {
            return;
        };
        let time_now = now_ms();
        self.shown_duration += time_now - show_start_time;
        self.show_start_time = None;

        let Some(uncollapsed_start) = self.show_uncollapsed_start_time else {
            return;
        };
        self.show_uncollapsed_duration += time_now - uncollapsed_start;
        self.show_uncollapsed_start_time = None;
    }
}

/// `InlineSuggestData`.
pub struct InlineSuggestData {
    pub hint: Option<IInlineCompletionHint>,
    pub additional_text_edits: Vec<SingleEditOperation>,
    pub source_inline_completion: Rc<InlineCompletion>,
    pub source: Rc<InlineSuggestionList>,
    pub context: InlineCompletionContext,
    pub is_inline_edit: bool,
    pub supports_rename: bool,
    request_info: InlineSuggestRequestInfo,
    provider_request_info: InlineSuggestProviderRequestInfo,
    correlation_id: Option<String>,
    state: RefCell<DataState>,
}

/// O provedor de mentira do [`InlineSuggestData::create_for_test`].
struct MockInlineCompletionsProvider;

impl InlineCompletionsProvider for MockInlineCompletionsProvider {
    fn provide_inline_completions(
        &self,
        _model: &Rc<TextModel>,
        _position: Position,
        _context: &InlineCompletionContext,
        _token: &CancellationToken,
    ) -> Promise<Option<Rc<InlineCompletions>>> {
        Promise::resolved(Some(Rc::new(InlineCompletions::default())))
    }

    fn dispose_inline_completions(&self, _completions: &Rc<InlineCompletions>, _reason: InlineCompletionsDisposeReason) {}
}

impl InlineSuggestData {
    /// `InlineSuggestData.createForTest(action, targetUri)`.
    pub fn create_for_test(action: Option<IInlineSuggestDataAction>, _target_uri: &URI) -> Rc<InlineSuggestData> {
        let edit = match &action {
            Some(IInlineSuggestDataAction::Edit(edit)) => Some(edit),
            _ => None,
        };
        let mock_inline_completion = Rc::new(InlineCompletion {
            insert_text: Some(InlineCompletionInsertText::Text(edit.map(|edit| edit.insert_text.to_string_lossy()).unwrap_or_default())),
            range: edit.map(|edit| edit.range),
            is_inline_edit: Some(true),
            ..InlineCompletion::default()
        });
        let mock_provider: Rc<dyn InlineCompletionsProvider> = Rc::new(MockInlineCompletionsProvider);
        let mock_source = Rc::new(InlineSuggestionList::new(
            Rc::new(InlineCompletions { items: vec![Rc::clone(&mock_inline_completion)], ..InlineCompletions::default() }),
            mock_provider,
        ));
        let mock_context = InlineCompletionContext {
            trigger_kind: InlineCompletionTriggerKind::Explicit,
            selected_suggestion_info: None,
            user_prompt: None,
            request_uuid: format!("test-{}", date_now()),
            include_inline_edits: false,
            include_inline_completions: true,
            request_issued_date_time: now_ms(),
            earliest_shown_date_time: 0.0,
            change_hint: None,
        };
        let mock_request_info = InlineSuggestRequestInfo {
            start_time: now_ms(),
            sku: None,
            editor_type: InlineCompletionEditorType::TextEditor,
            language_id: "plaintext".to_string(),
            available_providers: Vec::new(),
            reason: String::new(),
            typing_interval: 0.0,
            typing_interval_character_count: 0,
        };
        let mock_provider_request_info = InlineSuggestProviderRequestInfo { start_time: now_ms(), end_time: now_ms() };

        Rc::new(InlineSuggestData::new(
            action,
            None,
            Vec::new(),
            mock_inline_completion,
            mock_source,
            mock_context,
            true,
            false,
            mock_request_info,
            mock_provider_request_info,
            None,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        action: Option<IInlineSuggestDataAction>,
        hint: Option<IInlineCompletionHint>,
        additional_text_edits: Vec<SingleEditOperation>,
        source_inline_completion: Rc<InlineCompletion>,
        source: Rc<InlineSuggestionList>,
        context: InlineCompletionContext,
        is_inline_edit: bool,
        supports_rename: bool,
        request_info: InlineSuggestRequestInfo,
        provider_request_info: InlineSuggestProviderRequestInfo,
        correlation_id: Option<String>,
    ) -> Self {
        let view_data = InlineSuggestViewData { editor_type: request_info.editor_type, render_data: None, view_kind: None };
        Self {
            hint,
            additional_text_edits,
            source_inline_completion,
            source,
            context,
            is_inline_edit,
            supports_rename,
            request_info,
            provider_request_info,
            correlation_id,
            state: RefCell::new(DataState {
                action,
                did_show: false,
                time_until_shown: None,
                time_until_actually_shown: None,
                show_start_time: None,
                shown_duration: 0.0,
                show_uncollapsed_start_time: None,
                show_uncollapsed_duration: 0.0,
                not_shown_reason: None,
                view_data,
                did_report_end_of_life: false,
                last_set_end_of_life_reason: None,
                is_preceeded: false,
                partially_accepted_count: 0,
                partially_accepted_since_original: PartialAcceptance::default(),
                rename_info: None,
                edit_kind: None,
                performance: InlineSuggestionsPerformance::new(),
            }),
        }
    }

    /// `action`.
    pub fn action(&self) -> Option<IInlineSuggestDataAction> {
        self.state.borrow().action.clone()
    }

    /// `showInlineEditMenu`.
    pub fn show_inline_edit_menu(&self) -> bool {
        self.source_inline_completion.show_inline_edit_menu.unwrap_or(false)
    }

    /// `partialAccepts`.
    pub fn partial_accepts(&self) -> PartialAcceptance {
        self.state.borrow().partially_accepted_since_original
    }

    /// `reportInlineEditShown(...)`: avisa o provedor e executa o `shownCommand`; a promessa assenta com o comando.
    pub fn report_inline_edit_shown(
        &self,
        command_service: &dyn ICommandService,
        updated_insert_text: &JsString,
        view_kind: InlineCompletionViewKind,
        view_data: InlineCompletionViewData,
        edit_kind: Option<InlineSuggestionEditKind>,
        time_when_shown: f64,
    ) -> Promise<()> {
        {
            let mut state = self.state.borrow_mut();
            state.update_shown_duration(view_kind);

            if state.did_show || state.did_report_end_of_life {
                return Promise::resolved(());
            }
            state.performance.mark("shown");
            state.did_show = true;
            state.edit_kind = edit_kind;
            state.view_data.view_kind = Some(view_kind);
            state.view_data.render_data = Some(view_data.clone());
            state.time_until_shown = Some(time_when_shown - self.request_info.start_time);
            state.time_until_actually_shown = Some(now_ms() - self.request_info.start_time);
        }

        let edit_delta_info = EditDeltaInfo::new(
            view_data.line_count_modified,
            view_data.line_count_original,
            view_data.character_count_modified,
            view_data.character_count_original,
        );
        self.source.provider.handle_item_did_show(
            &self.source.inline_suggestions,
            &self.source_inline_completion,
            &updated_insert_text.to_string_lossy(),
            edit_delta_info,
        );

        match &self.source_inline_completion.shown_command {
            Some(command) => command_service.execute_command(&command.id, command.arguments.clone().unwrap_or_default()).then(|_| ()),
            None => Promise::resolved(()),
        }
    }

    /// `reportPartialAccept(acceptedCharacters, info, partialAcceptance)`.
    pub fn report_partial_accept(&self, accepted_characters: i32, info: PartialAcceptInfo, partial_acceptance: PartialAcceptance) {
        {
            let mut state = self.state.borrow_mut();
            state.partially_accepted_count += 1;
            let since_original = &mut state.partially_accepted_since_original;
            since_original.characters += partial_acceptance.characters;
            since_original.ratio = (since_original.ratio + (1.0 - since_original.ratio) * partial_acceptance.ratio).min(1.0);
            since_original.count += partial_acceptance.count;
        }

        self.source.provider.handle_partial_accept(&self.source.inline_suggestions, &self.source_inline_completion, accepted_characters, info);
    }

    /// Sends the end of life event to the provider.
    /// If no reason is provided, the last set reason is used.
    /// If no reason was set, the default reason is used.
    pub fn report_end_of_life(&self, reason: Option<InlineCompletionEndOfLifeReason>) {
        let (reason, summary) = {
            let mut state = self.state.borrow_mut();
            if state.did_report_end_of_life {
                return;
            }
            state.did_report_end_of_life = true;
            state.report_inline_edit_hidden();

            let ignored = || InlineCompletionEndOfLifeReason::Ignored { superseded_by: None, user_typing_disagreed: false };
            let mut reason = reason.or_else(|| state.last_set_end_of_life_reason.clone()).unwrap_or_else(ignored);

            // A suggestion can only be "rejected" if it was actually shown to the user.
            // If the suggestion was never shown, downgrade to "ignored".
            if reason.kind() == InlineCompletionEndOfLifeReasonKind::Rejected && !state.did_show {
                reason = ignored();
            }

            let summary = self.source.provider.has_handle_end_of_lifetime().then(|| self.lifetime_summary(&state));
            (reason, summary)
        };

        if reason.kind() == InlineCompletionEndOfLifeReasonKind::Rejected && self.source.provider.has_handle_rejection() {
            self.source.provider.handle_rejection(&self.source.inline_suggestions, &self.source_inline_completion);
        }

        if let Some(summary) = summary {
            self.source.provider.handle_end_of_lifetime(&self.source.inline_suggestions, &self.source_inline_completion, &reason, &summary);
        }
    }

    fn lifetime_summary(&self, state: &DataState) -> LifetimeSummary {
        let render_data = state.view_data.render_data.as_ref().map(InlineCompletionViewData::get_data);
        LifetimeSummary {
            request_uuid: self.context.request_uuid.clone(),
            correlation_id: self.correlation_id.clone(),
            selected_suggestion_info: self.context.selected_suggestion_info.is_some(),
            partially_accepted: state.partially_accepted_count,
            partially_accepted_count_since_original: state.partially_accepted_since_original.count,
            partially_accepted_ratio_since_original: state.partially_accepted_since_original.ratio,
            partially_accepted_characters_since_original: state.partially_accepted_since_original.characters,
            shown: state.did_show,
            shown_duration: state.shown_duration,
            shown_duration_uncollapsed: state.show_uncollapsed_duration,
            edit_kind: state.edit_kind.as_ref().map(InlineSuggestionEditKind::to_json_string),
            preceeded: state.is_preceeded,
            time_until_shown: state.time_until_shown,
            time_until_actually_shown: state.time_until_actually_shown,
            time_until_provider_request: self.provider_request_info.start_time - self.request_info.start_time,
            time_until_provider_response: self.provider_request_info.end_time - self.request_info.start_time,
            editor_type: state.view_data.editor_type.as_str().to_string(),
            language_id: self.request_info.language_id.clone(),
            request_reason: self.request_info.reason.clone(),
            view_kind: state.view_data.view_kind.map(|kind| kind.as_str().to_string()),
            not_shown_reason: state.not_shown_reason.clone(),
            performance_markers: Some(state.performance.to_json_string()),
            rename_created: state.rename_info.map(|info| info.created_rename),
            rename_duration: state.rename_info.map(|info| info.duration),
            rename_timed_out: state.rename_info.and_then(|info| info.timed_out),
            rename_dropped_other_edits: state.rename_info.and_then(|info| info.dropped_other_edits),
            rename_dropped_rename_edits: state.rename_info.and_then(|info| info.dropped_rename_edits),
            typing_interval: self.request_info.typing_interval,
            typing_interval_character_count: self.request_info.typing_interval_character_count,
            sku_plan: self.request_info.sku.as_ref().map(|sku| sku.plan.clone()),
            sku_type: self.request_info.sku.as_ref().map(|sku| sku.kind.clone()),
            available_providers: self.request_info.available_providers.iter().map(ProviderId::to_string).collect::<Vec<_>>().join(","),
            cursor_column_distance: render_data.as_ref().map(|data| data.cursor_column_distance),
            cursor_line_distance: render_data.as_ref().map(|data| data.cursor_line_distance),
            line_count_original: render_data.as_ref().map(|data| data.line_count_original),
            line_count_modified: render_data.as_ref().map(|data| data.line_count_modified),
            character_count_original: render_data.as_ref().map(|data| data.character_count_original),
            character_count_modified: render_data.as_ref().map(|data| data.character_count_modified),
            disjoint_replacements: render_data.as_ref().map(|data| data.disjoint_replacements),
            same_shape_replacements: render_data.as_ref().and_then(|data| data.same_shape_replacements),
            long_distance_hint_visible: render_data.as_ref().and_then(|data| data.long_distance_hint_visible),
            long_distance_hint_distance: render_data.as_ref().and_then(|data| data.long_distance_hint_distance),
            is_for_another_document: render_data.as_ref().and_then(|data| data.is_for_another_document),
        }
    }

    /// `setIsPreceeded(partialAccepts)`.
    pub fn set_is_preceeded(&self, partial_accepts: PartialAcceptance) {
        let mut state = self.state.borrow_mut();
        state.is_preceeded = true;

        if state.partially_accepted_since_original != PartialAcceptance::default() {
            eprintln!("Expected partiallyAcceptedCountSinceOriginal to be {{ characters: 0, rate: 0, partialAcceptances: 0 }} before setIsPreceeded.");
        }
        state.partially_accepted_since_original = partial_accepts;
    }

    /// `setNotShownReason(reason)`: só o primeiro motivo fica.
    pub fn set_not_shown_reason(&self, reason: &str) {
        let mut state = self.state.borrow_mut();
        if state.not_shown_reason.is_none() {
            state.not_shown_reason = Some(reason.to_string());
        }
    }

    /// Sets the end of life reason, but does not send the event to the provider yet.
    pub fn set_end_of_life_reason(&self, reason: InlineCompletionEndOfLifeReason) {
        let mut state = self.state.borrow_mut();
        state.report_inline_edit_hidden();
        state.last_set_end_of_life_reason = Some(reason);
    }

    /// `setRenameProcessingInfo(info)`: só uma vez.
    pub fn set_rename_processing_info(&self, info: RenameInfo) {
        let mut state = self.state.borrow_mut();
        if state.rename_info.is_some() {
            je_base::errors::throw(Error::bug_indicating(Some("Rename info has already been set.")));
        }
        state.rename_info = Some(info);
    }

    /// `withAction(action)`: troca a ação e devolve o próprio dado.
    pub fn with_action(self: &Rc<Self>, action: IInlineSuggestDataAction) -> Rc<InlineSuggestData> {
        self.state.borrow_mut().action = Some(action);
        Rc::clone(self)
    }

    /// `addPerformanceMarker(marker)`.
    pub fn add_performance_marker(&self, marker: &str) {
        self.state.borrow_mut().performance.mark(marker);
    }
}

/// `InlineSuggestionsPerformance`.
struct InlineSuggestionsPerformance {
    markers: Vec<(String, f64)>,
}

impl InlineSuggestionsPerformance {
    fn new() -> Self {
        Self { markers: vec![("start".to_string(), now_ms())] }
    }

    fn mark(&mut self, marker: &str) {
        self.markers.push((marker.to_string(), now_ms()));
    }

    /// `JSON.stringify(deltas)`: um objeto `{ [marcador]: tempo desde o anterior }` por marcador.
    fn to_json_string(&self) -> String {
        let deltas: Vec<String> = self
            .markers
            .windows(2)
            .map(|pair| format!("{{{}:{}}}", json_string(&pair[1].0), json_number(pair[1].1 - pair[0].1)))
            .collect();
        format!("[{}]", deltas.join(","))
    }
}

/// `JSON.stringify(text)` de um texto.
fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `JSON.stringify(number)`: inteiro sem a parte decimal.
fn json_number(value: f64) -> String {
    if value.fract() == 0.0 { format!("{}", value as i64) } else { value.to_string() }
}

/// `SnippetInfo`.
#[derive(Clone, Debug)]
pub struct SnippetInfo {
    pub snippet: JsString,
    /// Could be different than the main range.
    pub range: Range,
}

/// `InlineCompletionEditorType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineCompletionEditorType {
    TextEditor,
    DiffEditor,
    Notebook,
}

impl InlineCompletionEditorType {
    /// O valor textual do membro (`'textEditor'`, `'diffEditor'`, `'notebook'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TextEditor => "textEditor",
            Self::DiffEditor => "diffEditor",
            Self::Notebook => "notebook",
        }
    }
}

/// A ref counted pointer to the computed `InlineCompletions` and the `InlineCompletionsProvider` that
/// computed them.
pub struct InlineSuggestionList {
    ref_count: Cell<i32>,
    pub inline_suggestions: Rc<InlineCompletions>,
    inline_suggestions_data: RefCell<Vec<Rc<InlineSuggestData>>>,
    pub provider: Rc<dyn InlineCompletionsProvider>,
}

impl InlineSuggestionList {
    pub fn new(inline_suggestions: Rc<InlineCompletions>, provider: Rc<dyn InlineCompletionsProvider>) -> Self {
        Self { ref_count: Cell::new(0), inline_suggestions, inline_suggestions_data: RefCell::new(Vec::new()), provider }
    }

    /// `inlineSuggestionsData`: os dados criados para os itens da lista (vazio depois que a lista foi descartada).
    pub fn inline_suggestions_data(&self) -> Vec<Rc<InlineSuggestData>> {
        self.inline_suggestions_data.borrow().clone()
    }

    /// O `data.push(...)` do original, que compartilha o vetor com a lista.
    fn add_data(&self, data: Rc<InlineSuggestData>) {
        self.inline_suggestions_data.borrow_mut().push(data);
    }

    pub fn add_ref(&self) {
        self.ref_count.set(self.ref_count.get() + 1);
    }

    pub fn remove_ref(&self, reason: Option<InlineCompletionsDisposeReason>) {
        let reason = reason.unwrap_or(InlineCompletionsDisposeReason { kind: InlineCompletionsDisposeReasonKind::Other });
        self.ref_count.set(self.ref_count.get() - 1);
        let ref_count = self.ref_count.get();
        if ref_count == 0 {
            let items = std::mem::take(&mut *self.inline_suggestions_data.borrow_mut());
            for item in items {
                // Fallback if it has not been called before
                item.report_end_of_life(None);
            }
            self.provider.dispose_inline_completions(&self.inline_suggestions, reason);
        } else if ref_count < 0 {
            // Invariant: every addRef must be paired with exactly one removeRef.
            // Going negative means a removeRef without a matching addRef somewhere.
            let provider_id = self.provider.provider_id().map_or_else(|| "undefined".to_string(), ProviderId::to_string);
            on_unexpected_error(&Error::bug_indicating(Some(&format!(
                "InlineSuggestionList (provider={provider_id}) refCount went negative ({ref_count}): more removeRef than addRef calls."
            ))));
        }
    }
}

fn get_default_range(position: Position, model: &Rc<TextModel>) -> Range {
    let word = model.get_word_at_position(position);
    let max_column = model.get_line_max_column(position.line_number);
    // By default, always replace up until the end of the current line.
    // This default might be subject to change!
    match word {
        Some(word) => Range::new(position.line_number, word.start_column, position.line_number, max_column),
        None => Range::from_positions(position, position.with(position.line_number, max_column)),
    }
}
