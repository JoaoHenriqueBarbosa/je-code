//! Porte de `src/vs/editor/contrib/suggest/browser/suggest.ts`: o item de sugestão (`CompletionItem`), a ordem dos
//! snippets, as opções e o resultado de um pedido (`CompletionOptions`, `CompletionItemModel`), a coleta das
//! sugestões dos provedores (`provideSuggestionItems`), os comparadores de ordenação, as chaves de contexto do
//! widget de sugestões e o corpo do comando `_executeCompletionItemProvider`.
//!
//! Representação:
//! - `CompletionItem` é um objeto compartilhado no original: o mesmo item entra em vários `CompletionModel` (o
//!   `providerItemsToReuse` do pedido seguinte devolve os itens do anterior) e o modelo escreve nele a nota, a
//!   distância, o índice e a palavra com que o filtrou. Aqui [`CompletionItem`] é o manipulador `Rc` de um
//!   [`CompletionItemData`]: copiar o manipulador é referenciar o mesmo item ([`CompletionItem::is_same`] é o
//!   `===`), e os campos mutáveis são `Cell` e `RefCell`.
//! - `completion` é um `RefCell`: o `resolve` do original muda o objeto do provedor com `Object.assign`, e os
//!   testes mudam `preselect` depois de criar o item. O item guarda uma cópia do `languages::CompletionItem` do
//!   provedor, então o `sortText` que `provideSuggestionItems` preenche (quando falta) só aparece na cópia.
//! - `Object.assign(this.completion, value)` copia os campos presentes do valor resolvido: os obrigatórios
//!   (`label`, `kind`, `range`, `insertText`) sempre, os opcionais só quando vêm preenchidos.
//! - `extensionId` não existe (o `languages::CompletionItem` do porte não o tem).
//! - O `range` de um `languages::CompletionItem` é obrigatório no porte (como no tipo do original), então o `range`
//!   padrão (a palavra sob o cursor) que `provideSuggestionItems` preenche para os provedores do host de extensões
//!   não tem onde ser usado, e a palavra sob a posição não é calculada.
//! - `Set<CompletionItemProvider>` e `Map<CompletionItemProvider, CompletionItem[]>` são o [`ProviderSet`] e o
//!   [`ProviderItems`]: listas na ordem de inserção (a mesma que `Set` e `Map` enumeram), com a identidade do
//!   provedor como chave ([`provider_id`]).
//! - `Set<CompletionItemKind>` é um `HashSet`; os argumentos com valor padrão do construtor de `CompletionOptions`
//!   são os campos públicos de [`CompletionOptions`], com o `Default` do original.
//! - `provideSuggestionItems` é uma função `async`: o corpo roda já, de forma síncrona, até o primeiro `await`
//!   ([`async_fn`]). Os pedidos aos provedores de um grupo e ao provedor de snippets são promessas que rodam como as
//!   `Promise`s do original.
//! - `Context` é o [`SuggestContext`] (as chaves de `RawContextKey` num `thread_local`, lidas por
//!   [`suggest_context`]); `Context.Visible` (o `historyNavigationVisible` de `contextScopedHistoryWidget.ts`) é a
//!   chave `suggestWidgetVisible` dele. O `suggestWidgetStatusbarMenu` (um `MenuId`) e o `showSimpleSuggestions`
//!   (que chama o `SuggestController`) não têm porte: dependem do sistema de menus e do controlador, ainda sem porte.
//! - `CommandsRegistry.registerCommand('_executeCompletionItemProvider', ...)` registra o comando na carga do módulo
//!   e lê `ILanguageFeaturesService` e `ITextModelService`, que o porte não tem; o corpo do comando é
//!   [`execute_completion_item_provider`], que recebe o registro e o modelo já resolvidos (os `assertType` dos
//!   argumentos são o tipo dos parâmetros). O `completion.disposable.dispose()` do `setTimeout` é o descarte do
//!   [`DisposableStore`] do resultado.

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::ops::Deref;
use std::rc::Rc;

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error, is_cancellation_error, on_unexpected_external_error};
use je_base::filters::{FUZZY_SCORE_DEFAULT, FuzzyScore};
use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::runtime;
use je_base::arrays::sort_by_compare;
use je_base::stopwatch::StopWatch;
use je_platform::contextkey::{ContextKeyMeta, RawContextKey};

use crate::config::editor_options::quick_suggestions::InternalQuickSuggestionsOptions;
use crate::config::editor_options::quick_suggestions::QuickSuggestionsValue;
use crate::encoded_token_attributes::standard_token_type;
use crate::language_feature_registry::LanguageFeatureRegistry;
use crate::languages::{
    self, CompletionContext, CompletionItemKind, CompletionItemProvider, CompletionItemRange, CompletionItemTag,
    CompletionLabel, CompletionList, CompletionTriggerKind, completion_item_insert_text_rule,
};
use crate::position::Position;
use crate::snippet::snippet_parser::SnippetParser;
use crate::text_model::TextModel;

/// `Context.*`: as chaves de contexto do widget de sugestões.
#[derive(Clone)]
pub struct SuggestContext {
    /// `Context.Visible` (`historyNavigationVisible`).
    pub visible: RawContextKey,
    pub has_focused_suggestion: RawContextKey,
    pub details_visible: RawContextKey,
    pub details_focused: RawContextKey,
    pub multiple_suggestions: RawContextKey,
    pub makes_text_edit: RawContextKey,
    pub accept_suggestions_on_enter: RawContextKey,
    pub has_insert_and_replace_range: RawContextKey,
    /// `'insert' | 'replace'`, sem valor padrão.
    pub insert_mode: RawContextKey,
    pub can_resolve: RawContextKey,
}

thread_local! {
    static CONTEXT: SuggestContext = SuggestContext {
        visible: RawContextKey::new(
            "suggestWidgetVisible",
            false,
            localize("suggestWidgetVisible", "Whether suggestion are visible", &[]),
        ),
        has_focused_suggestion: RawContextKey::new(
            "suggestWidgetHasFocusedSuggestion",
            false,
            localize("suggestWidgetHasSelection", "Whether any suggestion is focused", &[]),
        ),
        details_visible: RawContextKey::new(
            "suggestWidgetDetailsVisible",
            false,
            localize("suggestWidgetDetailsVisible", "Whether suggestion details are visible", &[]),
        ),
        details_focused: RawContextKey::new(
            "suggestWidgetDetailsFocused",
            false,
            localize("suggestWidgetDetailsFocused", "Whether the details pane of the suggest widget has focus", &[]),
        ),
        multiple_suggestions: RawContextKey::new(
            "suggestWidgetMultipleSuggestions",
            false,
            localize("suggestWidgetMultipleSuggestions", "Whether there are multiple suggestions to pick from", &[]),
        ),
        makes_text_edit: RawContextKey::new(
            "suggestionMakesTextEdit",
            true,
            localize(
                "suggestionMakesTextEdit",
                "Whether inserting the current suggestion yields in a change or has everything already been typed",
                &[],
            ),
        ),
        accept_suggestions_on_enter: RawContextKey::new(
            "acceptSuggestionOnEnter",
            true,
            localize("acceptSuggestionOnEnter", "Whether suggestions are inserted when pressing Enter", &[]),
        ),
        has_insert_and_replace_range: RawContextKey::new(
            "suggestionHasInsertAndReplaceRange",
            false,
            localize(
                "suggestionHasInsertAndReplaceRange",
                "Whether the current suggestion has insert and replace behaviour",
                &[],
            ),
        ),
        insert_mode: RawContextKey::new(
            "suggestionInsertMode",
            Dynamic::Undefined,
            ContextKeyMeta::Info {
                ty: "string".to_string(),
                description: localize("suggestionInsertMode", "Whether the default behaviour is to insert or replace", &[]),
            },
        ),
        can_resolve: RawContextKey::new(
            "suggestionCanResolve",
            false,
            localize("suggestionCanResolve", "Whether the current suggestion supports to resolve further details", &[]),
        ),
    };
}

/// `Context`: as chaves de contexto do widget de sugestões (as mesmas a cada chamada, no thread).
pub fn suggest_context() -> SuggestContext {
    CONTEXT.with(Clone::clone)
}

/// `SnippetSortOrder`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnippetSortOrder {
    Top,
    Inline,
    Bottom,
}

/// A identidade de um provedor (o `===` do JavaScript): o endereço do objeto, sem a tabela de métodos.
pub fn provider_id(provider: &Rc<dyn CompletionItemProvider>) -> usize {
    Rc::as_ptr(provider).cast::<()>() as usize
}

/// `Set<CompletionItemProvider>`: os provedores na ordem de inserção, sem repetição.
#[derive(Clone, Default)]
pub struct ProviderSet(Vec<Rc<dyn CompletionItemProvider>>);

impl ProviderSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// `add(provider)`.
    pub fn add(&mut self, provider: Rc<dyn CompletionItemProvider>) -> &mut Self {
        if !self.has(&provider) {
            self.0.push(provider);
        }
        self
    }

    /// `has(provider)`.
    pub fn has(&self, provider: &Rc<dyn CompletionItemProvider>) -> bool {
        self.0.iter().any(|candidate| provider_id(candidate) == provider_id(provider))
    }

    /// `size`.
    pub fn size(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Rc<dyn CompletionItemProvider>> {
        self.0.iter()
    }
}

impl FromIterator<Rc<dyn CompletionItemProvider>> for ProviderSet {
    fn from_iter<T: IntoIterator<Item = Rc<dyn CompletionItemProvider>>>(iter: T) -> Self {
        let mut set = ProviderSet::new();
        for provider in iter {
            set.add(provider);
        }
        set
    }
}

/// `Map<CompletionItemProvider, CompletionItem[]>`: os itens de cada provedor, na ordem em que o provedor entrou.
#[derive(Clone, Default)]
pub struct ProviderItems(Vec<(Rc<dyn CompletionItemProvider>, Vec<CompletionItem>)>);

impl ProviderItems {
    pub fn new() -> Self {
        Self::default()
    }

    /// `get(provider)`.
    pub fn get(&self, provider: &Rc<dyn CompletionItemProvider>) -> Option<&Vec<CompletionItem>> {
        self.0.iter().find(|(candidate, _)| provider_id(candidate) == provider_id(provider)).map(|(_, items)| items)
    }

    /// `get(provider)` para mudar a lista.
    pub fn get_mut(&mut self, provider: &Rc<dyn CompletionItemProvider>) -> Option<&mut Vec<CompletionItem>> {
        self.0.iter_mut().find(|(candidate, _)| provider_id(candidate) == provider_id(provider)).map(|(_, items)| items)
    }

    /// `has(provider)`.
    pub fn has(&self, provider: &Rc<dyn CompletionItemProvider>) -> bool {
        self.get(provider).is_some()
    }

    /// `set(provider, items)`: troca a lista de um provedor que já está, ou o põe no fim.
    pub fn set(&mut self, provider: Rc<dyn CompletionItemProvider>, items: Vec<CompletionItem>) {
        match self.get_mut(&provider) {
            Some(existing) => *existing = items,
            None => self.0.push((provider, items)),
        }
    }

    /// `size`.
    pub fn size(&self) -> usize {
        self.0.len()
    }

    /// `[provider, items]` de cada entrada, na ordem de inserção.
    pub fn iter(&self) -> impl Iterator<Item = &(Rc<dyn CompletionItemProvider>, Vec<CompletionItem>)> {
        self.0.iter()
    }
}

/// O conteúdo de um [`CompletionItem`]: tudo o que a ordenação e a filtragem precisam.
pub struct CompletionItemData {
    // ---- quem pediu e de onde veio
    pub position: Position,
    pub completion: RefCell<languages::CompletionItem>,
    pub container: Rc<CompletionList>,
    pub provider: Rc<dyn CompletionItemProvider>,

    // ---- a edição
    pub edit_start: Position,
    pub edit_insert_end: Position,
    pub edit_replace_end: Position,

    // ---- o rótulo
    pub text_label: String,

    // ---- perf: as variantes em minúsculas
    pub label_low: String,
    pub sort_text_low: Option<String>,
    pub filter_text_low: Option<String>,

    // ---- validação
    pub is_invalid: bool,

    // ---- ordenação e filtragem
    pub score: RefCell<FuzzyScore>,
    pub distance: Cell<i32>,
    pub idx: Cell<Option<usize>>,
    pub word: RefCell<Option<Rc<str>>>,

    // ---- resolução
    resolve_duration: Cell<Option<f64>>,
    resolve_cache: RefCell<Option<Promise<()>>>,
}

/// `CompletionItem`: o manipulador compartilhado de um item de sugestão (ver o topo do módulo).
#[derive(Clone)]
pub struct CompletionItem(Rc<CompletionItemData>);

impl Deref for CompletionItem {
    type Target = CompletionItemData;

    fn deref(&self) -> &CompletionItemData {
        &self.0
    }
}

/// `toLowerCase` do JavaScript sobre texto UTF-8.
fn to_lower_case(text: &str) -> String {
    JsString::from(text).to_lower_case().to_string_lossy()
}

/// O texto de um rótulo (`typeof label === 'string' ? label : label.label`).
pub fn label_text(label: &CompletionLabel) -> &str {
    match label {
        CompletionLabel::Text(text) => text,
        CompletionLabel::Item(label) => &label.label,
    }
}

/// `Object.assign(target, value)`: os campos obrigatórios do valor sempre entram, os opcionais só quando
/// preenchidos.
fn assign_completion(target: &mut languages::CompletionItem, value: languages::CompletionItem) {
    target.label = value.label;
    target.kind = value.kind;
    target.range = value.range;
    target.insert_text = value.insert_text;
    if value.tags.is_some() {
        target.tags = value.tags;
    }
    if value.detail.is_some() {
        target.detail = value.detail;
    }
    if value.documentation.is_some() {
        target.documentation = value.documentation;
    }
    if value.sort_text.is_some() {
        target.sort_text = value.sort_text;
    }
    if value.filter_text.is_some() {
        target.filter_text = value.filter_text;
    }
    if value.preselect.is_some() {
        target.preselect = value.preselect;
    }
    if value.insert_text_rules.is_some() {
        target.insert_text_rules = value.insert_text_rules;
    }
    if value.commit_characters.is_some() {
        target.commit_characters = value.commit_characters;
    }
}

impl CompletionItem {
    pub fn new(
        position: Position,
        completion: languages::CompletionItem,
        container: Rc<CompletionList>,
        provider: Rc<dyn CompletionItemProvider>,
    ) -> Self {
        let text_label = label_text(&completion.label).to_string();

        // ensure lower-variants (perf)
        let label_low = to_lower_case(&text_label);

        // validate label
        let mut is_invalid = text_label.is_empty();

        // `completion.sortText && completion.sortText.toLowerCase()`: o texto vazio é falso e fica como está
        let sort_text_low = completion.sort_text.as_deref().filter(|text| !text.is_empty()).map(to_lower_case);
        let filter_text_low = completion.filter_text.as_deref().filter(|text| !text.is_empty()).map(to_lower_case);

        // normalize ranges
        let (edit_start, edit_insert_end, edit_replace_end) = match completion.range {
            CompletionItemRange::Single(range) => {
                // validate range
                is_invalid = is_invalid || range.spans_multiple_lines() || range.start_line_number != position.line_number;
                (
                    Position::new(range.start_line_number, range.start_column),
                    Position::new(range.end_line_number, range.end_column),
                    Position::new(range.end_line_number, range.end_column),
                )
            }
            CompletionItemRange::InsertReplace { insert, replace } => {
                // validate ranges
                is_invalid = is_invalid
                    || insert.spans_multiple_lines()
                    || replace.spans_multiple_lines()
                    || insert.start_line_number != position.line_number
                    || replace.start_line_number != position.line_number
                    || insert.start_column != replace.start_column;
                (
                    Position::new(insert.start_line_number, insert.start_column),
                    Position::new(insert.end_line_number, insert.end_column),
                    Position::new(replace.end_line_number, replace.end_column),
                )
            }
        };

        // create the suggestion resolver
        let (resolve_cache, resolve_duration) = if provider.has_resolve_completion_item() {
            (None, None)
        } else {
            (Some(Promise::resolved(())), Some(0.0))
        };

        Self(Rc::new(CompletionItemData {
            position,
            completion: RefCell::new(completion),
            container,
            provider,
            edit_start,
            edit_insert_end,
            edit_replace_end,
            text_label,
            label_low,
            sort_text_low,
            filter_text_low,
            is_invalid,
            score: RefCell::new(FUZZY_SCORE_DEFAULT.to_vec()),
            distance: Cell::new(0),
            idx: Cell::new(None),
            word: RefCell::new(None),
            resolve_duration: Cell::new(resolve_duration),
            resolve_cache: RefCell::new(resolve_cache),
        }))
    }

    /// `a === b`: os dois manipuladores são o mesmo item.
    pub fn is_same(&self, other: &CompletionItem) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    // ---- resolving

    pub fn is_resolved(&self) -> bool {
        self.resolve_duration.get().is_some()
    }

    pub fn resolve_duration(&self) -> f64 {
        self.resolve_duration.get().unwrap_or(-1.0)
    }

    /// `resolve(token)`: completa o item com o provedor uma vez só (a promessa fica em cache); um cancelamento
    /// desfaz o cache.
    pub fn resolve(&self, token: &CancellationToken) -> Promise<()> {
        if self.resolve_cache.borrow().is_none() {
            let on_cancel = self.clone();
            let sub = token.on_cancellation_requested().listen(move |_| {
                *on_cancel.resolve_cache.borrow_mut() = None;
                on_cancel.resolve_duration.set(None);
            });
            let sw = StopWatch::new(Some(true));
            let completion = self.completion.borrow().clone();
            let this = self.clone();
            let promise = self
                .provider
                .resolve_completion_item(&completion, token)
                .then_result(move |result| {
                    match result {
                        Ok(value) => {
                            if let Some(value) = value {
                                assign_completion(&mut this.completion.borrow_mut(), value);
                            }
                            this.resolve_duration.set(Some(sw.elapsed()));
                        }
                        Err(error) => {
                            if is_cancellation_error(&error) {
                                // the IPC queue will reject the request with the
                                // cancellation error -> reset cached
                                *this.resolve_cache.borrow_mut() = None;
                                this.resolve_duration.set(None);
                            }
                        }
                    }
                    Ok(())
                })
                .finally(move || sub.dispose());
            *self.resolve_cache.borrow_mut() = Some(promise);
        }
        self.resolve_cache.borrow().clone().expect("cache de resolução")
    }
}

/// `CompletionOptions`: o `Default` é o `CompletionOptions.default`.
#[derive(Clone)]
pub struct CompletionOptions {
    pub snippet_sort_order: SnippetSortOrder,
    pub kind_filter: HashSet<CompletionItemKind>,
    pub provider_filter: ProviderSet,
    pub provider_items_to_reuse: ProviderItems,
    pub show_deprecated: bool,
}

impl Default for CompletionOptions {
    fn default() -> Self {
        Self {
            snippet_sort_order: SnippetSortOrder::Bottom,
            kind_filter: HashSet::new(),
            provider_filter: ProviderSet::new(),
            provider_items_to_reuse: ProviderItems::new(),
            show_deprecated: true,
        }
    }
}

thread_local! {
    /// `_snippetSuggestSupport`.
    static SNIPPET_SUGGEST_SUPPORT: RefCell<Option<Rc<dyn CompletionItemProvider>>> = const { RefCell::new(None) };
}

/// `getSnippetSuggestSupport()`.
pub fn get_snippet_suggest_support() -> Option<Rc<dyn CompletionItemProvider>> {
    SNIPPET_SUGGEST_SUPPORT.with(|support| support.borrow().clone())
}

/// `setSnippetSuggestSupport(support)`: devolve o provedor que estava antes.
pub fn set_snippet_suggest_support(support: Option<Rc<dyn CompletionItemProvider>>) -> Option<Rc<dyn CompletionItemProvider>> {
    SNIPPET_SUGGEST_SUPPORT.with(|current| current.replace(support))
}

/// `CompletionDurationEntry`.
#[derive(Clone, Debug, PartialEq)]
pub struct CompletionDurationEntry {
    pub provider_name: String,
    pub elapsed_provider: f64,
    pub elapsed_overall: f64,
}

/// `CompletionDurations`.
#[derive(Clone, Debug, PartialEq)]
pub struct CompletionDurations {
    pub entries: Vec<CompletionDurationEntry>,
    pub elapsed: f64,
}

/// `CompletionItemModel`.
#[derive(Clone)]
pub struct CompletionItemModel {
    pub items: Vec<CompletionItem>,
    pub needs_clipboard: bool,
    pub durations: CompletionDurations,
    /// O que descarta as listas dos provedores (o `disposable` do original).
    pub disposable: Rc<DisposableStore>,
}

/// O estado que o corpo de `provideSuggestionItems` e os pedidos aos provedores dividem (as variáveis que o
/// `onCompletionList` do original captura).
struct SuggestionCollector {
    options: CompletionOptions,
    position: Position,
    result: RefCell<Vec<CompletionItem>>,
    disposables: Rc<DisposableStore>,
    durations: RefCell<Vec<CompletionDurationEntry>>,
    needs_clipboard: Cell<bool>,
}

impl SuggestionCollector {
    /// `onCompletionList(provider, container, sw)`: acrescenta os itens da lista; diz se acrescentou algum.
    fn on_completion_list(&self, provider: &Rc<dyn CompletionItemProvider>, container: Option<Rc<CompletionList>>, sw: &StopWatch) -> bool {
        let mut did_add_result = false;
        let Some(container) = container else {
            return did_add_result;
        };
        for suggestion in &container.suggestions {
            if self.options.kind_filter.contains(&suggestion.kind) {
                continue;
            }
            // skip if not showing deprecated suggestions
            if !self.options.show_deprecated
                && suggestion.tags.as_ref().is_some_and(|tags| tags.contains(&CompletionItemTag::Deprecated))
            {
                continue;
            }
            let mut suggestion = suggestion.clone();
            // fill in default sortText when missing
            if suggestion.sort_text.as_deref().is_none_or(str::is_empty) {
                suggestion.sort_text = Some(label_text(&suggestion.label).to_string());
            }
            if !self.needs_clipboard.get()
                && suggestion.insert_text_rules.is_some_and(|rules| rules & completion_item_insert_text_rule::INSERT_AS_SNIPPET != 0)
            {
                let template: Vec<u16> = suggestion.insert_text.encode_utf16().collect();
                self.needs_clipboard.set(SnippetParser::guess_needs_clipboard(&template));
            }
            self.result.borrow_mut().push(CompletionItem::new(self.position, suggestion, Rc::clone(&container), Rc::clone(provider)));
            did_add_result = true;
        }
        if let Some(dispose) = &container.dispose {
            let dispose = Rc::clone(dispose);
            self.disposables.add(to_disposable(move || dispose()));
        }
        self.durations.borrow_mut().push(CompletionDurationEntry {
            provider_name: provider.debug_display_name().to_string(),
            elapsed_provider: container.duration.unwrap_or(-1.0),
            elapsed_overall: sw.elapsed(),
        });
        did_add_result
    }
}

/// O que cada pedido a um provedor lê do `provideSuggestionItems` que o chamou.
#[derive(Clone)]
struct SuggestionRequest {
    collector: Rc<SuggestionCollector>,
    model: Rc<TextModel>,
    context: CompletionContext,
    token: CancellationToken,
}

impl SuggestionRequest {
    /// O `async () => { ... }` que pede as sugestões ao provedor de snippets, em paralelo com os "de verdade".
    fn snippet_completions(&self) -> Promise<()> {
        let request = self.clone();
        async_fn(async move {
            let options = &request.collector.options;
            let Some(support) = get_snippet_suggest_support() else {
                return Ok(());
            };
            if options.kind_filter.contains(&CompletionItemKind::Snippet) {
                return Ok(());
            }
            // we have items from a previous session that we can reuse
            if let Some(reuse_items) = options.provider_items_to_reuse.get(&support) {
                request.collector.result.borrow_mut().extend(reuse_items.iter().cloned());
                return Ok(());
            }
            if options.provider_filter.size() > 0 && !options.provider_filter.has(&support) {
                return Ok(());
            }
            let sw = StopWatch::new(None);
            let list = support
                .provide_completion_items(&request.model, request.collector.position, &request.context, &request.token)
                .await?;
            request.collector.on_completion_list(&support, list, &sw);
            Ok(())
        })
    }

    /// O `async provider => { ... }` de um grupo: pede as sugestões a um provedor; `did_add_result` é o
    /// `didAddResult` do grupo.
    fn provider_completions(&self, provider: Rc<dyn CompletionItemProvider>, did_add_result: Rc<Cell<bool>>) -> Promise<()> {
        let request = self.clone();
        async_fn(async move {
            let options = &request.collector.options;
            // we have items from a previous session that we can reuse
            if let Some(items) = options.provider_items_to_reuse.get(&provider) {
                request.collector.result.borrow_mut().extend(items.iter().cloned());
                did_add_result.set(did_add_result.get() || !items.is_empty());
                return Ok(());
            }
            // check if this provider is filtered out
            if options.provider_filter.size() > 0 && !options.provider_filter.has(&provider) {
                return Ok(());
            }
            let sw = StopWatch::new(None);
            let provided = match errors::catch(|| {
                provider.provide_completion_items(&request.model, request.collector.position, &request.context, &request.token)
            }) {
                Ok(provided) => provided.await,
                Err(error) => Err(error),
            };
            match provided {
                Ok(list) => {
                    did_add_result.set(request.collector.on_completion_list(&provider, list, &sw) || did_add_result.get());
                }
                Err(error) => on_unexpected_external_error(&error),
            }
            Ok(())
        })
    }
}

/// `provideSuggestionItems(registry, model, position, options, context, token)`: pede as sugestões aos provedores
/// (em grupos de mesma pontuação, parando no primeiro grupo que devolve algo) e ao provedor de snippets, e devolve os
/// itens ordenados. Se o `token` foi cancelado, a promessa é rejeitada com o `CancellationError`.
pub fn provide_suggestion_items(
    registry: &Rc<LanguageFeatureRegistry<dyn CompletionItemProvider>>,
    model: &Rc<TextModel>,
    position: Position,
    options: CompletionOptions,
    context: CompletionContext,
    token: &CancellationToken,
) -> Promise<CompletionItemModel> {
    let registry = Rc::clone(registry);
    let model = Rc::clone(model);
    let token = token.clone();
    async_fn(async move {
        let sw = StopWatch::new(None);
        let snippet_sort_order = options.snippet_sort_order;
        let disposables = DisposableStore::new();
        let collector = Rc::new(SuggestionCollector {
            options,
            position,
            result: RefCell::new(Vec::new()),
            disposables: Rc::clone(&disposables),
            durations: RefCell::new(Vec::new()),
            needs_clipboard: Cell::new(false),
        });
        let request = SuggestionRequest { collector: Rc::clone(&collector), model: Rc::clone(&model), context, token: token.clone() };

        // ask for snippets in parallel to asking "real" providers. Only do something if configured to
        // do so - no snippet filter, no special-providers-only request
        let snippet_completions = request.snippet_completions();

        // add suggestions from contributed providers - providers are ordered in groups of
        // equal score and once a group produces a result the process stops
        // get provider groups, always add snippet suggestion provider
        for provider_group in registry.ordered_groups(&model) {
            // for each support in the group ask for suggestions
            let did_add_result = Rc::new(Cell::new(false));
            let requests: Vec<Promise<()>> = provider_group
                .into_iter()
                .map(|provider| request.provider_completions(provider, Rc::clone(&did_add_result)))
                .collect();
            Promise::all(requests).await?;

            if did_add_result.get() || token.is_cancellation_requested() {
                break;
            }
        }

        snippet_completions.await?;

        if token.is_cancellation_requested() {
            disposables.dispose();
            return Err(Error::cancellation());
        }

        let mut items = collector.result.take();
        let comparator = get_suggestion_comparator(snippet_sort_order);
        sort_by_compare(&mut items, |a, b| f64::from(comparator(a, b)));
        Ok(CompletionItemModel {
            items,
            needs_clipboard: collector.needs_clipboard.get(),
            durations: CompletionDurations { entries: collector.durations.take(), elapsed: sw.elapsed() },
            disposable: disposables,
        })
    })
}

/// A comparação de textos do JavaScript (`<` e `>` sobre strings): unidade UTF-16 a unidade.
fn compare_js_strings(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

fn default_comparator(a: &CompletionItem, b: &CompletionItem) -> i32 {
    // check with 'sortText'
    if let (Some(a_sort_text), Some(b_sort_text)) = (&a.sort_text_low, &b.sort_text_low) {
        match compare_js_strings(a_sort_text, b_sort_text) {
            Ordering::Less => return -1,
            Ordering::Greater => return 1,
            Ordering::Equal => {}
        }
    }
    // check with 'label'
    match compare_js_strings(&a.text_label, &b.text_label) {
        Ordering::Less => return -1,
        Ordering::Greater => return 1,
        Ordering::Equal => {}
    }
    // check with 'type'
    a.completion.borrow().kind.to_index() as i32 - b.completion.borrow().kind.to_index() as i32
}

/// `snippetUpComparator` e `snippetDownComparator`, e o `_compareCompletionItemsSnippetsUp` e `...Down` do
/// `CompletionModel`: o snippet vai para o começo (`snippets_first`) ou para o fim, e os demais pares seguem
/// `otherwise`.
pub fn compare_with_snippets(
    a: &CompletionItem,
    b: &CompletionItem,
    snippets_first: bool,
    otherwise: fn(&CompletionItem, &CompletionItem) -> i32,
) -> i32 {
    let (a_kind, b_kind) = (a.completion.borrow().kind, b.completion.borrow().kind);
    if a_kind != b_kind {
        if a_kind == CompletionItemKind::Snippet {
            return if snippets_first { -1 } else { 1 };
        } else if b_kind == CompletionItemKind::Snippet {
            return if snippets_first { 1 } else { -1 };
        }
    }
    otherwise(a, b)
}

fn snippet_up_comparator(a: &CompletionItem, b: &CompletionItem) -> i32 {
    compare_with_snippets(a, b, true, default_comparator)
}

fn snippet_down_comparator(a: &CompletionItem, b: &CompletionItem) -> i32 {
    compare_with_snippets(a, b, false, default_comparator)
}

/// `getSuggestionComparator(snippetConfig)`: o comparador (negativo, zero ou positivo) da ordem dos snippets.
pub fn get_suggestion_comparator(snippet_config: SnippetSortOrder) -> fn(&CompletionItem, &CompletionItem) -> i32 {
    match snippet_config {
        SnippetSortOrder::Top => snippet_up_comparator,
        SnippetSortOrder::Bottom => snippet_down_comparator,
        SnippetSortOrder::Inline => default_comparator,
    }
}

/// O corpo do comando `_executeCompletionItemProvider`: as sugestões da posição (validada contra o modelo) e, dos
/// primeiros `max_items_to_resolve` itens, os detalhes resolvidos. O descarte das listas dos provedores acontece 100
/// milissegundos depois, como no original.
pub fn execute_completion_item_provider(
    registry: &Rc<LanguageFeatureRegistry<dyn CompletionItemProvider>>,
    model: &Rc<TextModel>,
    position: Position,
    trigger_character: Option<String>,
    max_items_to_resolve: Option<usize>,
) -> Promise<CompletionList> {
    let registry = Rc::clone(registry);
    let model = Rc::clone(model);
    async_fn(async move {
        let trigger_character = trigger_character.filter(|character| !character.is_empty());
        let mut result = CompletionList { incomplete: Some(false), suggestions: Vec::new(), dispose: None, duration: None };

        let actual_position = model.validate_position(position);
        let context = CompletionContext {
            trigger_kind: if trigger_character.is_some() { CompletionTriggerKind::TriggerCharacter } else { CompletionTriggerKind::Invoke },
            trigger_character,
        };
        let completions =
            provide_suggestion_items(&registry, &model, actual_position, CompletionOptions::default(), context, &CancellationToken::None).await?;

        let mut resolving: Vec<Promise<()>> = Vec::new();
        for item in &completions.items {
            if resolving.len() < max_items_to_resolve.unwrap_or(0) {
                resolving.push(item.resolve(&CancellationToken::None));
            }
            // `result.incomplete = result.incomplete || item.container.incomplete`
            if result.incomplete != Some(true) {
                result.incomplete = item.container.incomplete;
            }
            result.suggestions.push(item.completion.borrow().clone());
        }

        let resolved = Promise::all(resolving).await;
        let disposable = Rc::clone(&completions.disposable);
        runtime::set_timeout(move || disposable.dispose(), 100);
        resolved?;
        Ok(result)
    })
}

/// `ISuggestItemPreselector`.
pub trait ISuggestItemPreselector {
    /// `priority`: o pré-seletor de maior prioridade é perguntado primeiro.
    fn priority(&self) -> i32;

    /// `select(model, pos, items)`: o índice do item a pré-selecionar, ou `None` (o `-1`), caso em que os
    /// pré-seletores de menor prioridade são perguntados.
    fn select(&self, model: &TextModel, pos: Position, items: &[CompletionItem]) -> Option<usize>;
}

/// `QuickSuggestionsOptions`.
pub struct QuickSuggestionsOptions;

impl QuickSuggestionsOptions {
    /// `isAllOff(config)`.
    pub fn is_all_off(config: &InternalQuickSuggestionsOptions) -> bool {
        config.other == QuickSuggestionsValue::Off && config.comments == QuickSuggestionsValue::Off && config.strings == QuickSuggestionsValue::Off
    }

    /// `isAllOn(config)`.
    pub fn is_all_on(config: &InternalQuickSuggestionsOptions) -> bool {
        config.other == QuickSuggestionsValue::On && config.comments == QuickSuggestionsValue::On && config.strings == QuickSuggestionsValue::On
    }

    /// `valueFor(config, tokenType)`: o valor do tipo de token (os comentários e as strings têm o seu, o resto usa
    /// `other`). `token_type` é um dos [`standard_token_type`].
    pub fn value_for(config: &InternalQuickSuggestionsOptions, token_type: u32) -> QuickSuggestionsValue {
        match token_type {
            standard_token_type::COMMENT => config.comments,
            standard_token_type::STRING => config.strings,
            _ => config.other,
        }
    }
}
