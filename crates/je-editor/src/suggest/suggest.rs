//! Porte de `src/vs/editor/contrib/suggest/browser/suggest.ts`, só a parte de dados que o modelo de sugestões
//! e a memória de seleção leem: o [`CompletionItem`] (o item de sugestão, com o item do provedor, o resultado
//! do provedor e o provedor) e o [`SnippetSortOrder`].
//!
//! Diferenças do original, nenhuma de comportamento:
//! - `completion` é um `RefCell`: o `resolve` do original muda o objeto do provedor com `Object.assign`, e os
//!   testes mudam `preselect` depois de criar o item. Os campos de ordenação (`score`, `distance`, `idx`,
//!   `word`) são campos comuns, mudados com `&mut`.
//! - `Object.assign(this.completion, value)` copia os campos presentes do valor resolvido: os obrigatórios
//!   (`label`, `kind`, `range`, `insertText`) sempre, os opcionais só quando vêm preenchidos.
//! - `extensionId` não existe (o `languages::CompletionItem` do porte não o tem).
//! - O contexto (`Context`), o menu, `CompletionOptions`, `provideSuggestionItems`, `getSuggestionComparator` e
//!   o resto do arquivo dependem do `LanguageFeatureRegistry`, do `ICodeEditor` e dos comandos, ainda sem porte.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::JsString;
use je_base::cancellation::CancellationToken;
use je_base::errors::is_cancellation_error;
use je_base::filters::{FUZZY_SCORE_DEFAULT, FuzzyScore};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::stopwatch::StopWatch;

use crate::languages::{self, CompletionItemProvider, CompletionItemRange, CompletionLabel, CompletionList};
use crate::position::Position;

/// `SnippetSortOrder`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnippetSortOrder {
    Top,
    Inline,
    Bottom,
}

/// `CompletionItem`: um item de sugestão, com tudo o que a ordenação e a filtragem precisam.
pub struct CompletionItem {
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
    pub score: FuzzyScore,
    pub distance: i32,
    pub idx: Option<usize>,
    pub word: Option<String>,

    // ---- resolução
    resolve_duration: Cell<Option<f64>>,
    resolve_cache: RefCell<Option<Promise<()>>>,
}

/// `toLowerCase` do JavaScript sobre texto UTF-8.
fn to_lower_case(text: &str) -> String {
    JsString::from(text).to_lower_case().to_string_lossy()
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
        let text_label = match &completion.label {
            CompletionLabel::Text(text) => text.clone(),
            CompletionLabel::Item(label) => label.label.clone(),
        };

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

        Self {
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
            score: FUZZY_SCORE_DEFAULT.to_vec(),
            distance: 0,
            idx: None,
            word: None,
            resolve_duration: Cell::new(resolve_duration),
            resolve_cache: RefCell::new(resolve_cache),
        }
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
    pub fn resolve(self: &Rc<Self>, token: &CancellationToken) -> Promise<()> {
        if self.resolve_cache.borrow().is_none() {
            let on_cancel = Rc::clone(self);
            let sub = token.on_cancellation_requested().listen(move |_| {
                *on_cancel.resolve_cache.borrow_mut() = None;
                on_cancel.resolve_duration.set(None);
            });
            let sw = StopWatch::new(Some(true));
            let completion = self.completion.borrow().clone();
            let this = Rc::clone(self);
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
