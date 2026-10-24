//! Porte de `src/vs/editor/contrib/suggest/browser/completionModel.ts`: a visão filtrada e ordenada das sugestões
//! (`CompletionModel`), que a cada mudança do texto digitado refiltra os itens, pontua cada um contra a palavra sob o
//! cursor e guarda os itens por provedor.
//!
//! Representação:
//! - Os getters do original (`items`, `stats`, `getItemsByProvider`, `getIncompleteProvider`) montam o estado em cache
//!   quando ele está velho; o estado é um `RefCell` e os métodos recebem `&self`, como o modelo compartilhado do
//!   original. O `set lineContext` é [`CompletionModel::set_line_context`].
//! - `items` devolve o `Rc` da lista filtrada: a mesma lista (o `===` do original, [`Rc::ptr_eq`]) enquanto o estado
//!   em cache vale. Os itens são [`CompletionItem`], o manipulador compartilhado: o modelo escreve neles a palavra, a
//!   nota, a distância e o índice, e o mesmo item pode estar em vários modelos.
//! - `ReadonlyMap<CompletionItemProvider, CompletionItem[]>` e `Set<CompletionItemProvider>` são o [`ProviderItems`] e o
//!   [`ProviderSet`] de `suggest.rs`; `getItemsByProvider` devolve o `Rc` do mapa em cache.
//! - O texto da linha (`LineContext.leadingLineContent`) é UTF-8, e o filtro o converte para unidades UTF-16 (os
//!   comprimentos, as colunas e o `slice(-n)` do original contam unidades UTF-16).
//! - `snippetSuggestions: 'top' | 'bottom' | 'inline' | 'none'` é o texto do valor da opção: `top` e `bottom` trocam o
//!   comparador, os outros dois ficam com o padrão.
//! - `quickSelect(labelLengths.length - .85, ...)` pede o elemento de posição fracionária `n - 0.85`; as comparações
//!   `nth < tamanho` do `quickSelect` dão o mesmo resultado com o piso dela, que é `n - 1` para `n >= 1` (o maior
//!   comprimento de rótulo).
//! - O `Refilter` é o enum [`Refilter`] (`Nothing`, `All`, `Incr`).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::JsString;
use je_base::arrays::{quick_select, sort_by_compare};
use je_base::filters::{
    FUZZY_SCORE_DEFAULT, FuzzyScore, FuzzyScoreOptions, any_score, fuzzy_score, fuzzy_score_graceful_aggressive,
};
use je_base::strings::compare_ignore_case;

use crate::config::editor_options::suggest::InternalSuggestOptions;
use crate::suggest::suggest::{CompletionItem, ProviderItems, ProviderSet, compare_with_snippets};
use crate::suggest::word_distance::WordDistance;

/// `ICompletionStats`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompletionStats {
    pub p_label_len: usize,
}

/// `LineContext`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineContext {
    pub leading_line_content: String,
    pub character_count_delta: i32,
}

/// `Refilter`: o que o estado em cache precisa para ficar novo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refilter {
    /// O estado em cache vale.
    Nothing,
    /// Refiltra todos os itens.
    All,
    /// Refiltra só os que passaram no filtro anterior (o texto digitado só cresceu).
    Incr,
}

/// `CharCode.Space` e `CharCode.Tab`.
const SPACE: u16 = 32;
const TAB: u16 = 9;

/// O tipo de `FuzzyScorer`: `fuzzyScore` e `fuzzyScoreGracefulAggressive`.
type FuzzyScorer = fn(&[u16], &[u16], usize, &[u16], &[u16], usize, Option<&FuzzyScoreOptions>) -> Option<FuzzyScore>;

/// O estado que muda: o contexto da linha, o que refiltrar e o que o filtro calculou.
struct CachedState {
    line_context: LineContext,
    refilter_kind: Refilter,
    filtered_items: Option<Rc<Vec<CompletionItem>>>,
    items_by_provider: Option<Rc<ProviderItems>>,
    stats: Option<CompletionStats>,
}

/// `str.slice(-count)` sobre unidades UTF-16: com `count > 0` são as últimas `count` unidades (ou todas), e com
/// `count < 0` o que sobra depois das primeiras `-count`.
fn slice_from_end(units: &[u16], count: i32) -> &[u16] {
    let length = units.len() as i64;
    let start = -i64::from(count);
    let start = if start < 0 { (length + start).max(0) } else { start.min(length) };
    &units[start as usize..]
}

/// `_compareCompletionItems`: a maior nota primeiro, depois a menor distância, depois a ordem original.
fn compare_completion_items(a: &CompletionItem, b: &CompletionItem) -> i32 {
    let (a_score, b_score) = (a.score.borrow()[0], b.score.borrow()[0]);
    b_score.cmp(&a_score).then_with(|| a.distance.get().cmp(&b.distance.get())).then_with(|| a.idx.get().cmp(&b.idx.get())) as i32
}

/// `_compareCompletionItemsSnippetsUp`.
fn compare_completion_items_snippets_up(a: &CompletionItem, b: &CompletionItem) -> i32 {
    compare_with_snippets(a, b, true, compare_completion_items)
}

/// `_compareCompletionItemsSnippetsDown`.
fn compare_completion_items_snippets_down(a: &CompletionItem, b: &CompletionItem) -> i32 {
    compare_with_snippets(a, b, false, compare_completion_items)
}

/// Sorted, filtered completion view model.
pub struct CompletionModel {
    items: Vec<CompletionItem>,
    column: i32,
    word_distance: Rc<dyn WordDistance>,
    options: InternalSuggestOptions,
    snippet_compare_fn: fn(&CompletionItem, &CompletionItem) -> i32,
    fuzzy_score_options: FuzzyScoreOptions,
    state: RefCell<CachedState>,
    pub clipboard_text: Option<String>,
}

impl CompletionModel {
    #[allow(clippy::too_many_arguments)] // os parâmetros do construtor do original
    pub fn new(
        items: Vec<CompletionItem>,
        column: i32,
        line_context: LineContext,
        word_distance: Rc<dyn WordDistance>,
        options: InternalSuggestOptions,
        snippet_suggestions: &str,
        fuzzy_score_options: Option<FuzzyScoreOptions>,
        clipboard_text: Option<String>,
    ) -> Self {
        let snippet_compare_fn: fn(&CompletionItem, &CompletionItem) -> i32 = match snippet_suggestions {
            "top" => compare_completion_items_snippets_up,
            "bottom" => compare_completion_items_snippets_down,
            _ => compare_completion_items,
        };
        Self {
            items,
            column,
            word_distance,
            options,
            snippet_compare_fn,
            fuzzy_score_options: fuzzy_score_options.unwrap_or(FuzzyScoreOptions::DEFAULT),
            state: RefCell::new(CachedState {
                line_context,
                refilter_kind: Refilter::All,
                filtered_items: None,
                items_by_provider: None,
                stats: None,
            }),
            clipboard_text,
        }
    }

    /// `get lineContext`.
    pub fn line_context(&self) -> LineContext {
        self.state.borrow().line_context.clone()
    }

    /// `set lineContext(value)`: um contexto diferente invalida o estado em cache; se o texto só cresceu, o filtro
    /// parte dos itens que já passaram.
    pub fn set_line_context(&self, value: LineContext) {
        let mut state = self.state.borrow_mut();
        if state.line_context.leading_line_content != value.leading_line_content
            || state.line_context.character_count_delta != value.character_count_delta
        {
            state.refilter_kind = if state.line_context.character_count_delta < value.character_count_delta && state.filtered_items.is_some() {
                Refilter::Incr
            } else {
                Refilter::All
            };
            state.line_context = value;
        }
    }

    /// `get items`: os itens que passam no filtro, na ordem de exibição.
    pub fn items(&self) -> Rc<Vec<CompletionItem>> {
        self.ensure_cached_state();
        self.state.borrow().filtered_items.clone().expect("o estado em cache tem a lista filtrada")
    }

    /// `getItemsByProvider()`: os itens (que não são inválidos) de cada provedor, na ordem do modelo.
    pub fn get_items_by_provider(&self) -> Rc<ProviderItems> {
        self.ensure_cached_state();
        self.state.borrow().items_by_provider.clone().expect("o estado em cache tem os itens por provedor")
    }

    /// `getIncompleteProvider()`: os provedores cuja lista é incompleta (a do primeiro item deles).
    pub fn get_incomplete_provider(&self) -> ProviderSet {
        self.ensure_cached_state();
        let mut result = ProviderSet::new();
        for (provider, items) in self.get_items_by_provider().iter() {
            if items.first().is_some_and(|item| item.container.incomplete == Some(true)) {
                result.add(Rc::clone(provider));
            }
        }
        result
    }

    /// `get stats`.
    pub fn stats(&self) -> CompletionStats {
        self.ensure_cached_state();
        self.state.borrow().stats.expect("o estado em cache tem as estatísticas")
    }

    fn ensure_cached_state(&self) {
        if self.state.borrow().refilter_kind != Refilter::Nothing {
            self.create_cached_state();
        }
    }

    fn create_cached_state(&self) {
        let mut items_by_provider = ProviderItems::new();

        let mut label_lengths: Vec<usize> = Vec::new();

        let (line_context, refilter_kind, previous_filtered) = {
            let state = self.state.borrow();
            (state.line_context.clone(), state.refilter_kind, state.filtered_items.clone())
        };
        let leading_line_content: Vec<u16> = line_context.leading_line_content.encode_utf16().collect();
        let character_count_delta = line_context.character_count_delta;
        let mut word: Vec<u16> = Vec::new();
        let mut word_low: Vec<u16> = Vec::new();
        let mut word_text: Rc<str> = Rc::from("");

        // incrementally filter less
        let source: &[CompletionItem] = if refilter_kind == Refilter::All {
            self.items.as_slice()
        } else {
            previous_filtered.as_deref().expect("o filtro incremental parte da lista filtrada").as_slice()
        };
        let mut target: Vec<CompletionItem> = Vec::new();

        // picks a score function based on the number of
        // items that we have to score/filter and based on the
        // user-configuration
        let score_fn: FuzzyScorer =
            if !self.options.filter_graceful || source.len() > 2000 { fuzzy_score } else { fuzzy_score_graceful_aggressive };

        for (i, item) in source.iter().enumerate() {
            if item.is_invalid {
                continue; // SKIP invalid items
            }

            // keep all items by their provider
            match items_by_provider.get_mut(&item.provider) {
                Some(arr) => arr.push(item.clone()),
                None => items_by_provider.set(Rc::clone(&item.provider), vec![item.clone()]),
            }

            // 'word' is that remainder of the current line that we
            // filter and score against. In theory each suggestion uses a
            // different word, but in practice not - that's why we cache
            let overwrite_before = item.position.column - item.edit_start.column;
            let word_len = overwrite_before + character_count_delta - (item.position.column - self.column);
            if word.len() as i32 != word_len {
                word = if word_len == 0 { Vec::new() } else { slice_from_end(&leading_line_content, word_len).to_vec() };
                word_low = JsString::from(word.as_slice()).to_lower_case().units().to_vec();
                word_text = Rc::from(String::from_utf16_lossy(&word));
            }

            // remember the word against which this item was
            // scored
            *item.word.borrow_mut() = Some(Rc::clone(&word_text));

            if word_len == 0 {
                // when there is nothing to score against, don't
                // event try to do. Use a const rank and rely on
                // the fallback-sort using the initial sort order.
                // use a score of `-100` because that is out of the
                // bound of values `fuzzyScore` will return
                *item.score.borrow_mut() = FUZZY_SCORE_DEFAULT.to_vec();
            } else {
                // skip word characters that are whitespace until
                // we have hit the replace range (overwriteBefore)
                let mut word_pos = 0usize;
                while (word_pos as i32) < overwrite_before {
                    match word.get(word_pos) {
                        Some(&ch) if ch == SPACE || ch == TAB => word_pos += 1,
                        _ => break,
                    }
                }

                let label: Vec<u16> = item.text_label.encode_utf16().collect();
                let label_low: Vec<u16> = item.label_low.encode_utf16().collect();
                let filter_text = item.completion.borrow().filter_text.clone();

                if word_pos as i32 >= word_len {
                    // the wordPos at which scoring starts is the whole word
                    // and therefore the same rules as not having a word apply
                    *item.score.borrow_mut() = FUZZY_SCORE_DEFAULT.to_vec();
                } else if let Some(filter_text) = filter_text {
                    // when there is a `filterText` it must match the `word`.
                    // if it matches we check with the label to compute highlights
                    // and if that doesn't yield a result we have no highlights,
                    // despite having the match
                    let filter: Vec<u16> = filter_text.encode_utf16().collect();
                    let filter_low: Vec<u16> = item.filter_text_low.as_deref().unwrap_or("").encode_utf16().collect();
                    let Some(matched) = score_fn(&word, &word_low, word_pos, &filter, &filter_low, 0, Some(&self.fuzzy_score_options)) else {
                        continue; // NO match
                    };
                    if compare_ignore_case(&filter, &label) == 0 {
                        // filterText and label are actually the same -> use good highlights
                        *item.score.borrow_mut() = matched;
                    } else {
                        // re-run the scorer on the label in the hope of a result BUT use the rank
                        // of the filterText-match
                        let mut score = any_score(&word, &word_low, word_pos, &label, &label_low, 0);
                        score[0] = matched[0]; // use score from filterText
                        *item.score.borrow_mut() = score;
                    }
                } else {
                    // by default match `word` against the `label`
                    let Some(matched) = score_fn(&word, &word_low, word_pos, &label, &label_low, 0, Some(&self.fuzzy_score_options)) else {
                        continue; // NO match
                    };
                    *item.score.borrow_mut() = matched;
                }
            }

            item.idx.set(Some(i));
            item.distance.set(self.word_distance.distance(item.position, &item.completion.borrow()));
            target.push(item.clone());

            // update stats
            label_lengths.push(item.text_label.encode_utf16().count());
        }

        sort_by_compare(&mut target, |a, b| f64::from((self.snippet_compare_fn)(a, b)));
        let stats = CompletionStats {
            p_label_len: if label_lengths.is_empty() {
                0
            } else {
                quick_select(label_lengths.len() - 1, &label_lengths, |a, b| *a as f64 - *b as f64)
            },
        };

        let mut state = self.state.borrow_mut();
        state.filtered_items = Some(Rc::new(target));
        state.items_by_provider = Some(Rc::new(items_by_provider));
        state.refilter_kind = Refilter::Nothing;
        state.stats = Some(stats);
    }
}
