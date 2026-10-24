//! Porte de `src/vs/editor/contrib/smartSelect/browser/bracketSelections.ts`: o provedor de intervalos de seleção por
//! colchetes (`BracketSelectionRangeProvider`), que expande para dentro e para fora de cada par que envolve a posição
//! e, em pares de várias linhas, para o que vem antes do colchete de abrir (`xxx {` ou o `xxx` da linha de cima).
//!
//! Representação:
//! - A busca é cooperativa: o original varre colchetes por no máximo `_maxDuration` milissegundos e cede a vez com
//!   `setTimeout` para continuar numa nova rodada (no máximo `_maxRounds`). As funções `_bracketsRightYield` e
//!   `_bracketsLeftYield` são [`brackets_right_yield`] e [`brackets_left_yield`], que avisam o fim pelo [`Resolver`]
//!   da `Promise<void>` do original, e o `await new Promise(...)` de `provideSelectionRanges` é o `await` de uma tarefa
//!   [`spawn_local`](je_base::runtime::spawn_local).
//! - `_maxDuration` (`public static`, que o teste ajusta) é o `thread_local` [`BracketSelectionRangeProvider::max_duration`],
//!   lido e escrito por [`BracketSelectionRangeProvider::set_max_duration`]; `_maxRounds` é a constante [`MAX_ROUNDS`].
//! - `Map<string, LinkedList<Range>>` é um `HashMap<JsString, VecDeque<Range>>` (`push` é `push_back`, `shift` é
//!   `pop_front`) e `Map<string, number>` das contagens é um `HashMap<JsString, i32>`.
//! - O `if (!pos)` do original (posição ausente) não existe: a posição sempre é uma [`Position`].

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use je_base::JsString;
use je_base::cancellation::CancellationToken;
use je_base::promise::{Promise, Resolver};
use je_base::runtime;

use crate::cursor_common::CursorSimpleModel;
use crate::language_brackets_configuration::BracketKind;
use crate::language_providers::{ProviderResult, SelectionRange, SelectionRangeProvider};
use crate::position::Position;
use crate::range::Range;
use crate::text_model::TextModel;

/// `_maxRounds`.
const MAX_ROUNDS: u32 = 2;

/// Os fechamentos que ainda esperam a abertura, por texto do colchete (o `ranges` do original).
type Ranges = Rc<RefCell<HashMap<JsString, VecDeque<Range>>>>;

/// O `bucket` do original: os intervalos achados para uma posição.
type Bucket = Rc<RefCell<Vec<SelectionRange>>>;

thread_local! {
    /// `_maxDuration`: 30 milissegundos.
    static MAX_DURATION: Cell<u64> = const { Cell::new(30) };
}

/// `BracketSelectionRangeProvider`.
pub struct BracketSelectionRangeProvider;

impl BracketSelectionRangeProvider {
    /// `BracketSelectionRangeProvider._maxDuration` (leitura).
    pub fn max_duration() -> u64 {
        MAX_DURATION.with(Cell::get)
    }

    /// `BracketSelectionRangeProvider._maxDuration = ms` (escrita).
    pub fn set_max_duration(ms: u64) {
        MAX_DURATION.with(|max_duration| max_duration.set(ms));
    }
}

impl SelectionRangeProvider for BracketSelectionRangeProvider {
    fn provide_selection_ranges(
        &self,
        model: &Rc<TextModel>,
        positions: &[Position],
        _token: &CancellationToken,
    ) -> ProviderResult<Vec<Vec<SelectionRange>>> {
        let (promise, resolver) = Promise::new();
        let model = Rc::clone(model);
        let positions = positions.to_vec();

        runtime::spawn_local(async move {
            let mut result: Vec<Vec<SelectionRange>> = Vec::new();

            for position in positions {
                let bucket: Bucket = Rc::default();
                let ranges: Ranges = Rc::default();
                let (right, right_resolver) = Promise::<()>::new();
                brackets_right_yield(right_resolver, 0, &model, position, &ranges);
                let _ = right.await;

                let (left, left_resolver) = Promise::<()>::new();
                brackets_left_yield(left_resolver, 0, &model, position, &ranges, &bucket);
                let _ = left.await;

                result.push(bucket.take());
            }

            resolver.resolve(Some(result));
        });

        promise
    }
}

/// `_bracketsRightYield`: varre os colchetes à direita da posição e guarda os fechamentos sem abertura à direita.
fn brackets_right_yield(resolve: Resolver<()>, round: u32, model: &Rc<TextModel>, mut pos: Position, ranges: &Ranges) {
    let mut counts: HashMap<JsString, i32> = HashMap::new();
    let t1 = runtime::date_now();
    loop {
        if round >= MAX_ROUNDS {
            resolve.resolve(());
            break;
        }
        let Some(bracket) = model.bracket_pairs().find_next_bracket(pos) else {
            resolve.resolve(());
            break;
        };
        let d = runtime::date_now().saturating_sub(t1);
        if d > BracketSelectionRangeProvider::max_duration() {
            let (model, ranges) = (Rc::clone(model), Rc::clone(ranges));
            runtime::set_timeout(move || brackets_right_yield(resolve, round + 1, &model, pos, &ranges), 0);
            break;
        }
        match &bracket.bracket_info {
            BracketKind::Opening(opening) => {
                let key = opening.bracket_text.clone();
                // wait for closing
                *counts.entry(key).or_insert(0) += 1;
            }
            BracketKind::Closing(closing) => {
                let key = closing.opening_brackets[0].clone();
                // process closing
                let val = counts.get(&key).copied().unwrap_or(0) - 1;
                counts.insert(key.clone(), val.max(0));
                if val < 0 {
                    ranges.borrow_mut().entry(key).or_default().push_back(bracket.range);
                }
            }
        }
        pos = bracket.range.get_end_position();
    }
}

/// `_bracketsLeftYield`: varre os colchetes à esquerda da posição e casa cada abertura sem fechamento à esquerda com
/// o fechamento guardado, acrescentando ao `bucket` o interior, o par e o que vem antes dele.
fn brackets_left_yield(
    resolve: Resolver<()>,
    round: u32,
    model: &Rc<TextModel>,
    mut pos: Position,
    ranges: &Ranges,
    bucket: &Bucket,
) {
    let mut counts: HashMap<JsString, i32> = HashMap::new();
    let t1 = runtime::date_now();
    loop {
        if round >= MAX_ROUNDS && ranges.borrow().is_empty() {
            resolve.resolve(());
            break;
        }
        let Some(bracket) = model.bracket_pairs().find_prev_bracket(pos) else {
            resolve.resolve(());
            break;
        };
        let d = runtime::date_now().saturating_sub(t1);
        if d > BracketSelectionRangeProvider::max_duration() {
            let (model, ranges, bucket) = (Rc::clone(model), Rc::clone(ranges), Rc::clone(bucket));
            runtime::set_timeout(move || brackets_left_yield(resolve, round + 1, &model, pos, &ranges, &bucket), 0);
            break;
        }
        match &bracket.bracket_info {
            BracketKind::Closing(closing) => {
                let key = closing.opening_brackets[0].clone();
                // wait for opening
                *counts.entry(key).or_insert(0) += 1;
            }
            BracketKind::Opening(opening) => {
                let key = opening.bracket_text.clone();
                // opening
                let val = counts.get(&key).copied().unwrap_or(0) - 1;
                counts.insert(key.clone(), val.max(0));
                if val < 0 {
                    let closing = {
                        let mut ranges = ranges.borrow_mut();
                        match ranges.get_mut(&key) {
                            Some(list) => {
                                let closing = list.pop_front();
                                if list.is_empty() {
                                    ranges.remove(&key);
                                }
                                closing
                            }
                            None => None,
                        }
                    };
                    if let Some(closing) = closing {
                        let inner_bracket =
                            Range::from_positions(bracket.range.get_end_position(), closing.get_start_position());
                        let outer_bracket =
                            Range::from_positions(bracket.range.get_start_position(), closing.get_end_position());
                        let mut bucket = bucket.borrow_mut();
                        bucket.push(SelectionRange { range: inner_bracket });
                        bucket.push(SelectionRange { range: outer_bracket });
                        add_bracket_leading(model, outer_bracket, &mut bucket);
                    }
                }
            }
        }
        pos = bracket.range.get_start_position();
    }
}

/// `_addBracketLeading`: para um par de várias linhas, o que vem antes do colchete de abrir na mesma linha
/// (`xxxxxxxx {`) ou na linha de cima (`xxxxxxxx` e `{` na linha seguinte).
fn add_bracket_leading(model: &TextModel, bracket: Range, bucket: &mut Vec<SelectionRange>) {
    if bracket.start_line_number == bracket.end_line_number {
        return;
    }
    // xxxxxxxx {
    //
    // }
    let start_line = bracket.start_line_number;
    let column = model.get_line_first_non_whitespace_column(start_line);
    if column != 0 && column != bracket.start_column {
        bucket.push(SelectionRange {
            range: Range::from_positions(Position::new(start_line, column), bracket.get_end_position()),
        });
        bucket.push(SelectionRange {
            range: Range::from_positions(Position::new(start_line, 1), bracket.get_end_position()),
        });
    }

    // xxxxxxxx
    // {
    //
    // }
    let above_line = start_line - 1;
    if above_line > 0 {
        let column = model.get_line_first_non_whitespace_column(above_line);
        if column == bracket.start_column && column != model.get_line_last_non_whitespace_column(above_line) {
            bucket.push(SelectionRange {
                range: Range::from_positions(Position::new(above_line, column), bracket.get_end_position()),
            });
            bucket.push(SelectionRange {
                range: Range::from_positions(Position::new(above_line, 1), bracket.get_end_position()),
            });
        }
    }
}
