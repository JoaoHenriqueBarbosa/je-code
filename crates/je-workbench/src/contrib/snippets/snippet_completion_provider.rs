//! Porte de `src/vs/workbench/contrib/snippets/browser/snippetCompletionProvider.ts`: o item de sugestão de um snippet
//! (`SnippetCompletion`) e o provedor que casa os snippets com o texto antes do cursor
//! (`SnippetCompletionProvider`).
//!
//! Notas de tradução:
//! - `SnippetCompletion implements CompletionItem` é uma struct própria; [`SnippetCompletion::into_completion_item`] a
//!   converte no [`CompletionItem`] de `languages.rs` que a lista de sugestões carrega. O `label` é um objeto
//!   compartilhado e mutável no original (o `_disambiguateSnippets` o altera e compara sua identidade), então é
//!   `Rc<RefCell<CompletionItemLabel>>` e a comparação é [`Rc::ptr_eq`]: dois itens têm sempre rótulos distintos, o
//!   que torna o laço de desambiguação inerte, exatamente como no original.
//! - `provideCompletionItems` é `async`: [`async_fn`] roda o corpo até o primeiro `await` (o `getSnippets`). O texto da
//!   linha, as colunas e os comprimentos são unidades UTF-16 (`&[u16]`), como nas `string` do JavaScript.
//! - `new Set(await this._snippets.getSnippets(...))` com `snippets.delete(snippet)` durante a iteração é a lista
//!   única (por identidade, na ordem de inserção) mais um vetor que marca os já casados.
//! - `compareSubstring(lineContentLow, snippet.prefixLow, columnOffset, columnOffset + prefixRestLen, columnOffset - pos)`
//!   só é comparado com `0`: com `prefixRestLen <= 0` os dois trechos são vazios (igual), e além do fim da linha o
//!   `charCodeAt` do original dá `NaN` (diferente); o `compare_substring` do porte não admite o fim além da linha.
//! - `/\s/.test(ch)` é [`is_js_whitespace`]; `lineContentLow[i]` fora da linha é `undefined`, que não é espaço.
//! - `StopWatch#elapsed` é o `duration` da lista.
//!
//! Divergências:
//! - `CompletionItem.command` e `CompletionItem.extensionId` não existem no [`CompletionItem`] do porte (ver o cabeçalho
//!   de `languages.rs`). O comando `_snippet.markAsUsed` (com o snippet como argumento) é a constante
//!   [`MARK_SNIPPET_AS_USED`] mais o campo `snippet` do [`SnippetCompletion`]; o `extensionId` fica em
//!   [`SnippetCompletion::extension_id`]. O registro do comando em `CommandsRegistry` entra com o porte dos comandos
//!   da plataforma, e o corpo dele é `ISnippetsService#updateUsageTimestamp`.
//! - `resolveCompletionItem` só recebe itens deste provedor, então o `item instanceof SnippetCompletion` é sempre
//!   verdadeiro; o item já traz o `codeSnippet` como `insert_text`, que é o que `resolve()` usa.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::filters::is_pattern_in_word;
use je_base::html_content::{IMarkdownString, MarkdownString, TrustedOrOptions};
use je_base::js_string::{index_of, is_js_whitespace, substring};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::stopwatch::StopWatch;
use je_base::strings::compare_substring;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::language_configuration_registry::ILanguageConfigurationService;
use je_editor::language_providers::ProviderResult;
use je_editor::language_service::ILanguageService;
use je_editor::languages::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionItemLabel, CompletionItemProvider, CompletionItemRange, CompletionLabel,
    CompletionList, CompletionTriggerKind, StringOrMarkdown, completion_item_insert_text_rule,
};
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::snippet::snippet_parser::SnippetParser;
use je_editor::text_model::TextModel;
use je_editor::word_helper::WordAtPosition;
use je_extensions::manifest::ExtensionIdentifier;

use super::snippets::ISnippetsService;
use super::snippets_file::{Snippet, SnippetSource};

/// `markSnippetAsUsed`: o id do comando que o item de sugestão dispara depois de aceito.
pub const MARK_SNIPPET_AS_USED: &str = "_snippet.markAsUsed";

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// O `documentation` de `resolve()`: `new MarkdownString().appendCodeblock('', SnippetParser.asInsertText(codeSnippet))`.
fn snippet_documentation(code_snippet: &str) -> IMarkdownString {
    let mut markdown = MarkdownString::new(None, TrustedOrOptions::default());
    markdown.append_codeblock(&[], SnippetParser::as_insert_text(&units(code_snippet)).units());
    IMarkdownString {
        value: markdown.value,
        is_trusted: markdown.is_trusted,
        support_theme_icons: markdown.support_theme_icons,
        support_html: markdown.support_html,
        support_alert_syntax: markdown.support_alert_syntax,
        base_uri: markdown.base_uri,
        uris: markdown.uris,
    }
}

pub struct SnippetCompletion {
    pub snippet: Rc<Snippet>,
    pub label: Rc<RefCell<CompletionItemLabel>>,
    pub detail: String,
    pub insert_text: String,
    pub documentation: RefCell<Option<IMarkdownString>>,
    pub range: CompletionItemRange,
    pub sort_text: String,
    pub kind: CompletionItemKind,
    pub insert_text_rules: u32,
    pub extension_id: Option<ExtensionIdentifier>,
}

impl SnippetCompletion {
    pub fn new(snippet: Rc<Snippet>, range: CompletionItemRange) -> Self {
        let label = CompletionItemLabel { label: snippet.prefix.clone(), detail: None, description: Some(snippet.name.clone()) };
        let described = if snippet.description.is_empty() { &snippet.name } else { &snippet.description };
        let detail = localize("detail.snippet", "{0} ({1})", &[described.as_str(), snippet.source.as_str()]);
        let sort_text = format!("{}-{}", if snippet.snippet_source == SnippetSource::Extension { 'z' } else { 'a' }, snippet.prefix);
        Self {
            label: Rc::new(RefCell::new(label)),
            detail,
            insert_text: snippet.code_snippet(),
            documentation: RefCell::new(None),
            range,
            sort_text,
            kind: CompletionItemKind::Snippet,
            insert_text_rules: completion_item_insert_text_rule::INSERT_AS_SNIPPET,
            extension_id: snippet.extension_id.clone(),
            snippet,
        }
    }

    pub fn resolve(&self) -> &Self {
        *self.documentation.borrow_mut() = Some(snippet_documentation(&self.insert_text));
        self
    }

    pub fn compare_by_label(a: &SnippetCompletion, b: &SnippetCompletion) -> Ordering {
        a.label.borrow().label.encode_utf16().cmp(b.label.borrow().label.encode_utf16())
    }

    /// O item de sugestão do porte (ver as divergências do módulo).
    pub fn into_completion_item(&self) -> CompletionItem {
        CompletionItem {
            label: CompletionLabel::Item(self.label.borrow().clone()),
            kind: self.kind,
            range: self.range,
            tags: None,
            detail: Some(self.detail.clone()),
            documentation: self.documentation.borrow().clone().map(StringOrMarkdown::Markdown),
            sort_text: Some(self.sort_text.clone()),
            filter_text: None,
            preselect: None,
            insert_text: self.insert_text.clone(),
            insert_text_rules: Some(self.insert_text_rules),
            commit_characters: None,
            additional_text_edits: None,
        }
    }
}

struct SnippetPosition {
    start_column: i32,
    prefix_low: Vec<u16>,
    #[allow(dead_code)] // o original grava e nunca lê
    is_word: bool,
}

pub struct SnippetCompletionProvider {
    language_service: Rc<dyn ILanguageService>,
    snippets: Rc<dyn ISnippetsService>,
    language_configuration_service: Rc<dyn ILanguageConfigurationService>,
}

impl SnippetCompletionProvider {
    pub fn new(
        language_service: Rc<dyn ILanguageService>,
        snippets: Rc<dyn ISnippetsService>,
        language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    ) -> Self {
        Self { language_service, snippets, language_configuration_service }
    }
}

/// `/^\s/.test(text)`.
fn starts_with_whitespace(text: &[u16]) -> bool {
    text.first().is_some_and(|unit| is_js_whitespace(*unit))
}

/// `new Set(snippets)`: sem repetição de identidade, na ordem de inserção.
fn unique_snippets(snippets: Vec<Rc<Snippet>>) -> Vec<Rc<Snippet>> {
    let mut result: Vec<Rc<Snippet>> = Vec::with_capacity(snippets.len());
    for snippet in snippets {
        if !result.iter().any(|candidate| Rc::ptr_eq(candidate, &snippet)) {
            result.push(snippet);
        }
    }
    result
}

/// `_computeSnippetPositions`.
fn compute_snippet_positions(model: &TextModel, line: i32, word: &WordAtPosition, line_content_with_word_low: &[u16]) -> Vec<SnippetPosition> {
    let rest_from = |column: i32| substring(line_content_with_word_low, i64::from(column) - 1, line_content_with_word_low.len() as i64).to_vec();
    let mut result: Vec<SnippetPosition> = Vec::new();

    let mut column = 1;
    while column < word.start_column {
        let word_info = model.get_word_at_position(Position::new(line, column));
        result.push(SnippetPosition { start_column: column, prefix_low: rest_from(column), is_word: word_info.is_some() });
        if let Some(word_info) = word_info {
            column = word_info.end_column;

            // the character right after a word is an anchor, always
            result.push(SnippetPosition { start_column: word_info.end_column, prefix_low: rest_from(word_info.end_column), is_word: false });
        }
        column += 1;
    }

    if !word.word.is_empty() || result.is_empty() {
        result.push(SnippetPosition { start_column: word.start_column, prefix_low: rest_from(word.start_column), is_word: true });
    }

    result
}

/// `_getLanguageIdAtPosition`.
fn get_language_id_at_position(language_service: &dyn ILanguageService, model: &TextModel, position: Position) -> String {
    // validate the `languageId` to ensure this is a user
    // facing language with a name and the chance to have
    // snippets, else fall back to the outer language
    model.tokenization().tokenize_if_cheap(position.line_number);
    let mut language_id = model.get_language_id_at_position(position.line_number, position.column);
    if language_service.get_language_name(&language_id).is_none() {
        language_id = model.get_language_id();
    }
    language_id
}

/// `_disambiguateSnippets`.
fn disambiguate_snippets(suggestions: &mut [SnippetCompletion]) {
    suggestions.sort_by(SnippetCompletion::compare_by_label);
    let mut i = 0;
    while i < suggestions.len() {
        let mut to = i + 1;
        while to < suggestions.len() && Rc::ptr_eq(&suggestions[i].label, &suggestions[to].label) {
            let label = suggestions[to].label.borrow().label.clone();
            suggestions[to].label.borrow_mut().label = localize("snippetSuggest.longLabel", "{0}, {1}", &[label.as_str(), suggestions[to].snippet.name.as_str()]);
            to += 1;
        }
        if to > i + 1 {
            let label = suggestions[i].label.borrow().label.clone();
            suggestions[i].label.borrow_mut().label = localize("snippetSuggest.longLabel", "{0}, {1}", &[label.as_str(), suggestions[i].snippet.name.as_str()]);
            i = to;
        }
        i += 1;
    }
}

/// O corpo de `provideCompletionItems`.
async fn provide_completion_items(
    provider_language_service: Rc<dyn ILanguageService>,
    provider_snippets: Rc<dyn ISnippetsService>,
    provider_language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    model: Rc<TextModel>,
    position: Position,
    context: CompletionContext,
) -> Result<Option<Rc<CompletionList>>, Error> {
    let sw = StopWatch::new(None);

    // compute all snippet anchors: word starts and every non word character
    let line = position.line_number;
    let word = model.get_word_at_position(position).unwrap_or(WordAtPosition {
        word: JsString::new(),
        start_column: position.column,
        end_column: position.column,
    });

    let line_content_low: Vec<u16> = model.get_line_content(position.line_number).to_lower_case().units().to_vec();
    let line_content_with_word_low = substring(&line_content_low, 0, i64::from(word.start_column) + word.word.len() as i64 - 1).to_vec();
    let anchors = compute_snippet_positions(&model, line, &word, &line_content_with_word_low);

    // loop over possible snippets and match them against the anchors
    let column_offset = i64::from(position.column) - 1;
    let trigger_character_low = context.trigger_character.as_deref().map(|character| JsString::from(character).to_lower_case().to_string_lossy()).unwrap_or_default();
    let language_id = get_language_id_at_position(provider_language_service.as_ref(), &model, position);
    let language_config = provider_language_configuration_service.get_language_configuration(&language_id);
    let resource_uri = model.uri();
    let snippets = unique_snippets(provider_snippets.get_snippets(Some(language_id.as_str()), Some(&resource_uri), None).await?);
    let mut matched = vec![false; snippets.len()];
    let mut suggestions: Vec<SnippetCompletion> = Vec::new();

    for (index, snippet) in snippets.iter().enumerate() {
        if context.trigger_kind == CompletionTriggerKind::TriggerCharacter && !snippet.prefix_low.starts_with(&trigger_character_low) {
            // strict -> when having trigger characters they must prefix-match
            continue;
        }

        let snippet_prefix_low = units(&snippet.prefix_low);
        let mut candidate: Option<&SnippetPosition> = None;
        for anchor in &anchors {
            if starts_with_whitespace(&anchor.prefix_low) && !starts_with_whitespace(&snippet_prefix_low) {
                // only allow whitespace anchor when snippet prefix starts with whitespace too
                continue;
            }

            if is_pattern_in_word(&anchor.prefix_low, 0, anchor.prefix_low.len(), &snippet_prefix_low, 0, snippet_prefix_low.len(), false) {
                candidate = Some(anchor);
                break;
            }
        }

        let Some(candidate) = candidate else {
            continue;
        };

        let pos = i64::from(candidate.start_column) - 1;

        let prefix_rest_len = snippet_prefix_low.len() as i64 - (column_offset - pos);
        let ends_with_prefix_rest = prefix_rest_len <= 0
            || (column_offset + prefix_rest_len <= line_content_low.len() as i64
                && compare_substring(
                    &line_content_low,
                    &snippet_prefix_low,
                    column_offset as usize,
                    (column_offset + prefix_rest_len) as usize,
                    (column_offset - pos) as usize,
                    snippet_prefix_low.len(),
                ) == 0);
        let start_position = position.with(position.line_number, (pos + 1) as i32);

        let mut end_column = if ends_with_prefix_rest { position.column + prefix_rest_len as i32 } else { position.column };

        // First check if there is anything to the right of the cursor
        if column_offset < line_content_low.len() as i64 {
            let auto_closing_pairs = language_config.get_auto_closing_pairs();
            let right_of_cursor = JsString::from_units(vec![line_content_low[column_offset as usize]]);
            let standard_auto_closing_pair_conditionals = auto_closing_pairs.auto_closing_pairs_close_single_char.get(&right_of_cursor);
            // If the character to the right of the cursor is a closing character of an autoclosing pair
            let start_unit = usize::try_from(start_position.column - 1).ok().and_then(|at| line_content_low.get(at)).copied();
            let last_prefix_unit = snippet.prefix.encode_utf16().last();
            if standard_auto_closing_pair_conditionals.is_some_and(|pairs| {
                pairs.iter().any(|pair| {
                    // and the start position is the opening character of an autoclosing pair
                    start_unit.is_some_and(|unit| units(&pair.open) == [unit])
                        // and the snippet prefix contains the opening and closing pair at its edges
                        && snippet.prefix.starts_with(&pair.open)
                        && last_prefix_unit.is_some_and(|unit| units(&pair.close) == [unit])
                })
            }) {
                // Eat the character that was likely inserted because of auto-closing pairs
                end_column += 1;
            }
        }

        let replace = Range::from_positions(Position::new(line, candidate.start_column), Position::new(line, end_column));
        let insert = replace.set_end_position(line, position.column);

        suggestions.push(SnippetCompletion::new(Rc::clone(snippet), CompletionItemRange::InsertReplace { insert, replace }));
        matched[index] = true;
    }

    // add remaing snippets when the current prefix ends in whitespace or when line is empty
    // and when not having a trigger character
    let ends_in_whitespace = usize::try_from(i64::from(position.column) - 2)
        .ok()
        .and_then(|at| line_content_low.get(at))
        .is_some_and(|unit| is_js_whitespace(*unit));
    if trigger_character_low.is_empty() && (ends_in_whitespace || line_content_low.is_empty()) {
        for (index, snippet) in snippets.iter().enumerate() {
            if matched[index] {
                continue;
            }
            let snippet_prefix_low = units(&snippet.prefix_low);
            let insert = Range::from_positions(position, position);
            let replace = if index_of(&line_content_low, &snippet_prefix_low, column_offset as usize) == Some(column_offset as usize) {
                insert.set_end_position(position.line_number, position.column + snippet_prefix_low.len() as i32)
            } else {
                insert
            };
            suggestions.push(SnippetCompletion::new(Rc::clone(snippet), CompletionItemRange::InsertReplace { insert, replace }));
        }
    }

    // dismbiguate suggestions with same labels
    disambiguate_snippets(&mut suggestions);

    Ok(Some(Rc::new(CompletionList {
        suggestions: suggestions.iter().map(SnippetCompletion::into_completion_item).collect(),
        incomplete: None,
        dispose: None,
        duration: Some(sw.elapsed()),
    })))
}

impl CompletionItemProvider for SnippetCompletionProvider {
    fn debug_display_name(&self) -> &str {
        "snippetCompletions"
    }

    fn provide_completion_items(
        &self,
        model: &Rc<TextModel>,
        position: Position,
        context: &CompletionContext,
        _token: &CancellationToken,
    ) -> ProviderResult<Rc<CompletionList>> {
        async_fn(provide_completion_items(
            Rc::clone(&self.language_service),
            Rc::clone(&self.snippets),
            Rc::clone(&self.language_configuration_service),
            Rc::clone(model),
            position,
            context.clone(),
        ))
    }

    fn has_resolve_completion_item(&self) -> bool {
        true
    }

    fn resolve_completion_item(&self, item: &CompletionItem, _token: &CancellationToken) -> Promise<Option<CompletionItem>> {
        let mut resolved = item.clone();
        resolved.documentation = Some(StringOrMarkdown::Markdown(snippet_documentation(&item.insert_text)));
        Promise::resolved(Some(resolved))
    }
}
