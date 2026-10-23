//! Porte de `src/vs/workbench/services/suggest/browser/simpleCompletionModel.ts`: `LineContext` e o
//! `SimpleCompletionModel`, que filtra (fuzzy) e ordena as completions contra o texto digitado.
//!
//! Diferenças do original:
//! - O modelo é genérico no item `T` e no tipo `C` da completion dele (`T extends SimpleCompletionItem`):
//!   `T` precisa expor o [`SimpleCompletionItem`] base por `AsRef`/`AsMut`.
//! - Os itens ficam no modelo e `_filteredItems` guarda os índices deles em `items` (no original são as
//!   mesmas referências); [`SimpleCompletionModel::items`] devolve as referências na ordem filtrada.
//! - Os getters que recalculam o cache (`items`, `stats`) pedem `&mut self`.
//! - `lineContext` é a leitura [`SimpleCompletionModel::line_context`] e a troca
//!   [`SimpleCompletionModel::set_line_context`].
//! - `leadingLineContent` é [`JsString`] (posições em UTF-16) e `characterCountDelta` é `i64`.
//! - `quickSelect(labelLengths.length - .85, ...)`: o `nth` fracionário só entra nas comparações
//!   `nth < tamanho`, que para inteiro `k` dão o mesmo resultado com o `floor` do `nth`. Como o tamanho é
//!   pelo menos 1 nesse ramo, `floor(n - .85)` é `n - 1`, que é o que o porte passa ao `quick_select`.
//! - A ordenação usa `sort_by_compare` do `je-base` (estável, como o `Array.prototype.sort`).
//! - Sem função de comparação (`_rawCompareFn` indefinido) o `sort()` do JavaScript, com todos os itens
//!   iguais para ele, mantém a ordem, então o porte não ordena.

use std::marker::PhantomData;

use je_base::JsString;
use je_base::arrays::{CompareResult, quick_select, sort_by_compare};
use je_base::char_code as cc;
use je_base::filters::{FuzzyScoreOptions, FuzzyScorerFn, fuzzy_score, fuzzy_score_graceful_aggressive};

use super::simple_completion_item::{ISimpleCompletionData, SimpleCompletionItem, default_score};

/// `ISimpleCompletionStats`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ISimpleCompletionStats {
    pub p_label_len: usize,
}

/// `LineContext`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineContext {
    pub leading_line_content: JsString,
    pub character_count_delta: i64,
}

impl LineContext {
    pub fn new(leading_line_content: impl Into<JsString>, character_count_delta: i64) -> Self {
        Self { leading_line_content: leading_line_content.into(), character_count_delta }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Refilter {
    Nothing,
    All,
    Incr,
}

/// `_rawCompareFn`: `(leadingLineContent, a, b) => number`.
pub type RawCompareFn<T> = fn(&JsString, &T, &T) -> CompareResult;

/// O objeto `_options` (`// TODO: Pass in options` no original).
#[derive(Clone, Copy, Debug, Default)]
struct ModelOptions {
    filter_graceful: Option<bool>,
}

/// `SimpleCompletionModel`.
pub struct SimpleCompletionModel<T, C> {
    items: Vec<T>,
    line_context: LineContext,
    raw_compare_fn: Option<RawCompareFn<T>>,

    stats: Option<ISimpleCompletionStats>,
    /// Os índices, em `items`, dos itens que passaram no filtro, na ordem de exibição.
    filtered_items: Option<Vec<usize>>,
    refilter_kind: Refilter,
    fuzzy_score_options: FuzzyScoreOptions,
    options: ModelOptions,
    completion: PhantomData<C>,
}

/// `str.slice(start)` do JavaScript, em unidades UTF-16 (negativo conta do fim).
fn slice_from(units: &[u16], start: i64) -> &[u16] {
    let len = units.len() as i64;
    let from = if start < 0 { (len + start).max(0) } else { start.min(len) };
    &units[from as usize..]
}

impl<T, C> SimpleCompletionModel<T, C>
where
    T: AsRef<SimpleCompletionItem<C>> + AsMut<SimpleCompletionItem<C>>,
    C: ISimpleCompletionData,
{
    pub fn new(items: Vec<T>, line_context: LineContext, raw_compare_fn: Option<RawCompareFn<T>>) -> Self {
        Self {
            items,
            line_context,
            raw_compare_fn,
            stats: None,
            filtered_items: None,
            refilter_kind: Refilter::All,
            fuzzy_score_options: FuzzyScoreOptions { first_match_can_be_weak: true, ..FuzzyScoreOptions::DEFAULT },
            options: ModelOptions::default(),
            completion: PhantomData,
        }
    }

    /// `items`: os itens filtrados e ordenados.
    pub fn items(&mut self) -> Vec<&T> {
        self.ensure_cached_state();
        let filtered = self.filtered_items.as_ref().expect("o cache foi criado");
        filtered.iter().map(|&index| &self.items[index]).collect()
    }

    /// `stats`.
    pub fn stats(&mut self) -> ISimpleCompletionStats {
        self.ensure_cached_state();
        self.stats.expect("o cache foi criado")
    }

    /// `lineContext` (getter).
    pub fn line_context(&self) -> &LineContext {
        &self.line_context
    }

    /// `lineContext` (setter).
    pub fn set_line_context(&mut self, value: LineContext) {
        if self.line_context.leading_line_content != value.leading_line_content
            || self.line_context.character_count_delta != value.character_count_delta
        {
            self.refilter_kind = if self.line_context.character_count_delta < value.character_count_delta
                && self.filtered_items.is_some()
            {
                Refilter::Incr
            } else {
                Refilter::All
            };
            self.line_context = value;
        }
    }

    pub fn force_refilter_all(&mut self) {
        self.refilter_kind = Refilter::All;
    }

    fn ensure_cached_state(&mut self) {
        if self.refilter_kind != Refilter::Nothing {
            self.create_cached_state();
        }
    }

    fn create_cached_state(&mut self) {
        let mut label_lengths: Vec<usize> = Vec::new();

        let leading_line_content = self.line_context.leading_line_content.clone();
        let character_count_delta = self.line_context.character_count_delta;
        let fuzzy_score_options = self.fuzzy_score_options;
        let mut word: Vec<u16> = Vec::new();
        let mut word_low = JsString::new();

        // filtra de forma incremental, olhando menos itens
        let source: Vec<usize> = match (self.refilter_kind, &self.filtered_items) {
            (Refilter::All, _) | (_, None) => (0..self.items.len()).collect(),
            (_, Some(filtered)) => filtered.clone(),
        };
        let mut target: Vec<usize> = Vec::new();

        // escolhe a função de pontuação pelo número de itens a pontuar e pela configuração do usuário
        let score_fn: &FuzzyScorerFn = if !self.options.filter_graceful.unwrap_or(false) || source.len() > 2000 {
            &fuzzy_score
        } else {
            &fuzzy_score_graceful_aggressive
        };

        for (i, &item_index) in source.iter().enumerate() {
            let item: &mut SimpleCompletionItem<C> = self.items[item_index].as_mut();

            if item.is_invalid {
                continue; // PULA os itens inválidos
            }

            // `word` é o resto da linha contra o qual filtramos e pontuamos. Em teoria cada sugestão usa um
            // `word` diferente, mas na prática não: por isso o cache

            let overwrite_before = item.completion.replacement_range().map_or(0, |range| range[1] - range[0]);
            let word_len = overwrite_before + character_count_delta;
            if word.len() as i64 != word_len {
                word = if word_len == 0 { Vec::new() } else { slice_from(leading_line_content.units(), -word_len).to_vec() };
                word_low = JsString::from(word.as_slice()).to_lower_case();
            }

            // lembra o `word` contra o qual o item foi pontuado
            item.word = Some(JsString::from(word.as_slice()));
            if word_len == 0 {
                // quando não há nada contra o que pontuar, nem tenta: usa um posto constante e deixa a
                // ordem inicial desempatar. A pontuação `-100` está fora dos valores que `fuzzyScore` devolve
                item.score = default_score();
            } else {
                // pula os caracteres do `word` que são espaço até chegar ao intervalo de substituição
                let mut word_pos = 0usize;
                while (word_pos as i64) < overwrite_before {
                    match word.get(word_pos) {
                        Some(&cc::Space | &cc::Tab) => word_pos += 1,
                        _ => break,
                    }
                }

                if word_pos as i64 >= word_len {
                    // a posição em que a pontuação começa é o `word` inteiro, então valem as mesmas regras de
                    // não ter `word`
                    item.score = default_score();
                } else {
                    // por padrão casa o `word` com o rótulo
                    let matched = score_fn(
                        &word,
                        word_low.units(),
                        word_pos,
                        item.text_label.units(),
                        item.label_low.units(),
                        0,
                        Some(&fuzzy_score_options),
                    );
                    if matched.is_none() && !word.is_empty() {
                        continue; // SEM casamento
                    }
                    // usa a ordenação padrão quando o `word` é vazio
                    item.score = matched.unwrap_or_else(default_score);
                }
            }
            item.idx = Some(i);
            target.push(item_index);

            // atualiza as estatísticas
            label_lengths.push(item.text_label.len());
        }

        if let Some(compare) = self.raw_compare_fn {
            let items = &self.items;
            sort_by_compare(&mut target, |a, b| compare(&leading_line_content, &items[*a], &items[*b]));
        }
        self.filtered_items = Some(target);
        self.refilter_kind = Refilter::Nothing;

        self.stats = Some(ISimpleCompletionStats {
            p_label_len: if label_lengths.is_empty() {
                0
            } else {
                quick_select(label_lengths.len() - 1, &label_lengths, |a, b| *a as f64 - *b as f64)
            },
        });
    }
}
