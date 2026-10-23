//! Porte de `src/vs/editor/common/model/textModelTokens.ts`: a tokenização por linha com estado
//! (`TokenizerWithStateStore`, `TrackingTokenizationStateStore`, a fila de intervalos de linhas inválidas) e
//! o tokenizador em segundo plano que trabalha em fatias com orçamento de tempo (`DefaultBackgroundTokenizer`).
//!
//! Decisões de tradução:
//! - O estado do tokenizador é [`State`] (`Rc<dyn IState>`) em todo lugar (o `TState` genérico do original
//!   nunca é instanciado com outro tipo).
//! - O modelo é guardado como `Weak<TextModel>`: o modelo é dono da tokenização, e uma referência forte de
//!   volta formaria um ciclo de `Rc` que o coletor do JavaScript desfaz sozinho e o `Rc` não.
//! - O `try/catch` de `safeTokenize` vira [`errors::catch`], que captura o pânico e o entrega ao
//!   `onUnexpectedError`.
//! - `StopWatch.create(false)` é um `Instant`; `Date.now()` e `setTimeout0` vêm do `runtime` do `je-base`.

use crate::cursor_common::CursorSimpleModel;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Instant;

use je_base::JsString;
use je_base::async_::{IdleDeadline, run_when_global_idle};
use je_base::errors::{self, Error};
use je_base::lifecycle::IDisposable;
use je_base::runtime;

use crate::contiguous_multiline_tokens::ContiguousMultilineTokensBuilder;
use crate::eol_counter::count_eol;
use crate::encoded_token_attributes::standard_token_type;
use crate::fixed_array::FixedArray;
use crate::languages::{
    EncodedTokenizationResult, IBackgroundTokenizationStore, IBackgroundTokenizer, ILanguageIdCodec, ITokenizationSupport,
    State,
};
use crate::line_range::LineRange;
use crate::line_tokens::{IViewLineTokens, LineTokens};
use crate::null_tokenize::null_tokenize_encoded;
use crate::offset_range::OffsetRange;
use crate::position::Position;
use crate::text_model::TextModel;
use crate::text_model_events::ModelContentChange;

/// `Constants.CHEAP_TOKENIZATION_LENGTH_LIMIT`.
const CHEAP_TOKENIZATION_LENGTH_LIMIT: usize = 2048;

/// `Number.MAX_SAFE_INTEGER` (o número de linha "infinito").
const MAX_SAFE_INTEGER: i32 = i32::MAX;

/// A primeira linha com estado final inválido e o estado em que ela começa.
pub struct FirstInvalidLine {
    pub line_number: i32,
    pub start_state: State,
}

/// `TokenizerWithStateStore`.
pub struct TokenizerWithStateStore {
    initial_state: State,
    pub store: RefCell<TrackingTokenizationStateStore>,
    pub tokenization_support: Rc<dyn ITokenizationSupport>,
}

impl TokenizerWithStateStore {
    pub fn new(line_count: i32, tokenization_support: Rc<dyn ITokenizationSupport>) -> Self {
        Self {
            initial_state: tokenization_support.get_initial_state(),
            store: RefCell::new(TrackingTokenizationStateStore::new(line_count)),
            tokenization_support,
        }
    }

    pub fn get_start_state(&self, line_number: i32) -> Option<State> {
        self.store.borrow().get_start_state(line_number, &self.initial_state)
    }

    pub fn get_first_invalid_line(&self) -> Option<FirstInvalidLine> {
        self.store.borrow().get_first_invalid_line(&self.initial_state)
    }
}

/// `TokenizerWithStateStoreAndTextModel`.
pub struct TokenizerWithStateStoreAndTextModel {
    pub base: TokenizerWithStateStore,
    pub text_model: Weak<TextModel>,
    pub language_id_codec: Rc<dyn ILanguageIdCodec>,
}

impl TokenizerWithStateStoreAndTextModel {
    pub fn new(
        line_count: i32,
        tokenization_support: Rc<dyn ITokenizationSupport>,
        text_model: Weak<TextModel>,
        language_id_codec: Rc<dyn ILanguageIdCodec>,
    ) -> Self {
        Self { base: TokenizerWithStateStore::new(line_count, tokenization_support), text_model, language_id_codec }
    }

    pub fn store(&self) -> &RefCell<TrackingTokenizationStateStore> {
        &self.base.store
    }

    pub fn get_first_invalid_line(&self) -> Option<FirstInvalidLine> {
        self.base.get_first_invalid_line()
    }

    fn model(&self) -> Rc<TextModel> {
        self.text_model.upgrade().expect("o modelo de texto foi descartado")
    }

    /// `updateTokensUntilLine(builder, lineNumber)`.
    pub fn update_tokens_until_line(&self, builder: &mut ContiguousMultilineTokensBuilder, line_number: i32) {
        let model = self.model();
        let language_id = model.get_language_id();

        loop {
            let Some(line_to_tokenize) = self.base.get_first_invalid_line() else { break };
            if line_to_tokenize.line_number > line_number {
                break;
            }

            let text = model.get_line_content(line_to_tokenize.line_number);
            let r = safe_tokenize(
                &self.language_id_codec,
                &language_id,
                Some(&self.base.tokenization_support),
                &text,
                true,
                &line_to_tokenize.start_state,
            );
            builder.add(line_to_tokenize.line_number, r.tokens);
            self.base.store.borrow_mut().set_end_state(line_to_tokenize.line_number, r.end_state);
        }
    }

    /// `getTokenTypeIfInsertingCharacter(position, character)`.
    pub fn get_token_type_if_inserting_character(&self, position: Position, character: &JsString) -> u32 {
        let Some(line_start_state) = self.base.get_start_state(position.line_number) else {
            return standard_token_type::OTHER;
        };
        let model = self.model();
        let language_id = model.get_language_id();
        let line_content = model.get_line_content(position.line_number);

        // Create the text as if `character` was inserted
        let split = ((position.column - 1).max(0) as usize).min(line_content.len());
        let mut text = line_content.slice(0, split);
        text.push_units(character.units());
        text.push_units(&line_content.units()[split..]);

        let r = safe_tokenize(
            &self.language_id_codec,
            &language_id,
            Some(&self.base.tokenization_support),
            &text,
            true,
            &line_start_state,
        );
        let line_tokens = LineTokens::new(r.tokens, text, self.language_id_codec.clone());
        if line_tokens.get_count() == 0 {
            return standard_token_type::OTHER;
        }
        let token_index = line_tokens.find_token_index_at_offset(position.column - 1);
        line_tokens.get_standard_token_type(token_index)
    }

    /// `tokenizeLinesAt(lineNumber, lines)`.
    pub fn tokenize_lines_at(&self, line_number: i32, lines: &[JsString]) -> Option<Vec<LineTokens>> {
        let line_start_state = self.base.get_start_state(line_number)?;
        let language_id = self.model().get_language_id();
        let mut result = Vec::new();
        let mut state = line_start_state;
        for line in lines {
            let r = safe_tokenize(
                &self.language_id_codec,
                &language_id,
                Some(&self.base.tokenization_support),
                line,
                true,
                &state,
            );
            result.push(LineTokens::new(r.tokens, line.clone(), self.language_id_codec.clone()));
            state = r.end_state;
        }
        Some(result)
    }

    /// `hasAccurateTokensForLine(lineNumber)`.
    pub fn has_accurate_tokens_for_line(&self, line_number: i32) -> bool {
        line_number < self.base.store.borrow().get_first_invalid_end_state_line_number_or_max()
    }

    /// `isCheapToTokenize(lineNumber)`.
    pub fn is_cheap_to_tokenize(&self, line_number: i32) -> bool {
        let first_invalid_line_number = self.base.store.borrow().get_first_invalid_end_state_line_number_or_max();
        if line_number < first_invalid_line_number {
            return true;
        }
        line_number == first_invalid_line_number && self.model().get_line_length(line_number) < CHEAP_TOKENIZATION_LENGTH_LIMIT
    }

    /// `tokenizeHeuristically(builder, startLineNumber, endLineNumber)`: devolve `heuristicTokens`.
    pub fn tokenize_heuristically(&self, builder: &mut ContiguousMultilineTokensBuilder, start_line_number: i32, end_line_number: i32) -> bool {
        let first_invalid = self.base.store.borrow().get_first_invalid_end_state_line_number_or_max();
        if end_line_number <= first_invalid {
            // nothing to do
            return false;
        }

        if start_line_number <= first_invalid {
            self.update_tokens_until_line(builder, end_line_number);
            return false;
        }

        // From this point on all uses of the heuristics are for the lines that are not tokenized yet.
        let model = self.model();
        let mut state = self.guess_start_state(&model, start_line_number);
        let language_id = model.get_language_id();

        for line_number in start_line_number..=end_line_number {
            let text = model.get_line_content(line_number);
            let r = safe_tokenize(&self.language_id_codec, &language_id, Some(&self.base.tokenization_support), &text, true, &state);
            builder.add(line_number, r.tokens);
            state = r.end_state;
        }
        true
    }

    fn guess_start_state(&self, model: &TextModel, line_number: i32) -> State {
        let (likely_relevant_lines, initial_state) = find_likely_relevant_lines(model, line_number, Some(&self.base));
        let initial_state = initial_state.unwrap_or_else(|| self.base.tokenization_support.get_initial_state());

        let language_id = model.get_language_id();
        let mut state = initial_state;
        for line in &likely_relevant_lines {
            let r = safe_tokenize(&self.language_id_codec, &language_id, Some(&self.base.tokenization_support), line, false, &state);
            state = r.end_state;
        }
        state
    }
}

/// `findLikelyRelevantLines(model, lineNumber, store?)`.
pub fn find_likely_relevant_lines(
    model: &TextModel,
    line_number: i32,
    store: Option<&TokenizerWithStateStore>,
) -> (Vec<JsString>, Option<State>) {
    let mut non_whitespace_column = model.get_line_first_non_whitespace_column(line_number);
    let mut likely_relevant_lines: Vec<JsString> = Vec::new();
    let mut initial_state: Option<State> = None;
    let mut i = line_number - 1;
    while non_whitespace_column > 1 && i >= 1 {
        let new_non_whitespace_index = model.get_line_first_non_whitespace_column(i);

        if new_non_whitespace_index == 0 {
            i -= 1;
            continue;
        }

        if new_non_whitespace_index < non_whitespace_column {
            likely_relevant_lines.push(model.get_line_content(i));
            non_whitespace_column = new_non_whitespace_index;
            initial_state = store.and_then(|s| s.get_start_state(i));
            if initial_state.is_some() {
                break;
            }
        }
        i -= 1;
    }

    likely_relevant_lines.reverse();
    (likely_relevant_lines, initial_state)
}

/// `TrackingTokenizationStateStore`.
pub struct TrackingTokenizationStateStore {
    tokenization_state_store: TokenizationStateStore,
    invalid_end_states_line_numbers: RangePriorityQueueImpl,
    line_count: i32,
}

impl TrackingTokenizationStateStore {
    pub fn new(line_count: i32) -> Self {
        let mut invalid = RangePriorityQueueImpl::new();
        invalid.add_range(OffsetRange::new(1, line_count + 1));
        Self { tokenization_state_store: TokenizationStateStore::new(), invalid_end_states_line_numbers: invalid, line_count }
    }

    pub fn get_end_state(&self, line_number: i32) -> Option<State> {
        self.tokenization_state_store.get_end_state(line_number)
    }

    /// Devolve `true` se o estado mudou (o próximo estado inicial passa a ser inválido).
    pub fn set_end_state(&mut self, line_number: i32, state: State) -> bool {
        self.invalid_end_states_line_numbers.delete(line_number);
        let r = self.tokenization_state_store.set_end_state(line_number, state);
        if r && line_number < self.line_count {
            // because the state changed, we cannot trust the next state anymore and have to invalidate it.
            self.invalid_end_states_line_numbers.add_range(OffsetRange::new(line_number + 1, line_number + 2));
        }
        r
    }

    pub fn accept_change(&mut self, range: LineRange, new_line_count: i32) {
        self.line_count += new_line_count - range.length();
        self.tokenization_state_store.accept_change(range, new_line_count);
        self.invalid_end_states_line_numbers
            .add_range_and_resize(OffsetRange::new(range.start_line_number, range.end_line_number_exclusive), new_line_count);
    }

    pub fn accept_changes(&mut self, changes: &[ModelContentChange]) {
        for c in changes {
            let eol_count = count_eol(c.text.units()).eol_count as i32;
            self.accept_change(LineRange::new(c.range.start_line_number, c.range.end_line_number + 1), eol_count + 1);
        }
    }

    pub fn invalidate_end_state_range(&mut self, range: LineRange) {
        self.invalid_end_states_line_numbers.add_range(OffsetRange::new(range.start_line_number, range.end_line_number_exclusive));
    }

    pub fn get_first_invalid_end_state_line_number(&self) -> Option<i32> {
        self.invalid_end_states_line_numbers.min()
    }

    pub fn get_first_invalid_end_state_line_number_or_max(&self) -> i32 {
        self.get_first_invalid_end_state_line_number().filter(|&n| n != 0).unwrap_or(MAX_SAFE_INTEGER)
    }

    pub fn all_states_valid(&self) -> bool {
        self.invalid_end_states_line_numbers.min().is_none()
    }

    pub fn get_start_state(&self, line_number: i32, initial_state: &State) -> Option<State> {
        if line_number == 1 {
            return Some(initial_state.clone());
        }
        self.get_end_state(line_number - 1)
    }

    pub fn get_first_invalid_line(&self, initial_state: &State) -> Option<FirstInvalidLine> {
        let line_number = self.get_first_invalid_end_state_line_number()?;
        let Some(start_state) = self.get_start_state(line_number, initial_state) else {
            errors::throw(Error::bug_indicating(Some("Start state must be defined")));
        };
        Some(FirstInvalidLine { line_number, start_state })
    }
}

/// `TokenizationStateStore`.
pub struct TokenizationStateStore {
    line_end_states: FixedArray<Option<State>>,
}

impl TokenizationStateStore {
    pub fn new() -> Self {
        Self { line_end_states: FixedArray::new(None) }
    }

    pub fn get_end_state(&self, line_number: i32) -> Option<State> {
        self.line_end_states.get(line_number as usize)
    }

    /// Devolve `true` se o estado mudou.
    pub fn set_end_state(&mut self, line_number: i32, state: State) -> bool {
        let old_state = self.line_end_states.get(line_number as usize);
        if let Some(old) = old_state
            && old.equals(&*state)
        {
            return false;
        }
        self.line_end_states.set(line_number as usize, Some(state));
        true
    }

    pub fn accept_change(&mut self, range: LineRange, mut new_line_count: i32) {
        let mut length = range.length();
        if new_line_count > 0 && length > 0 {
            // Keep the last state, even though it is unrelated.
            // But if the new state happens to agree with this last state, then we know we can stop tokenizing.
            length -= 1;
            new_line_count -= 1;
        }
        self.line_end_states.replace(range.start_line_number as usize, length as usize, new_line_count as usize);
    }

    pub fn accept_changes(&mut self, changes: &[ModelContentChange]) {
        for c in changes {
            let eol_count = count_eol(c.text.units()).eol_count as i32;
            self.accept_change(LineRange::new(c.range.start_line_number, c.range.end_line_number + 1), eol_count + 1);
        }
    }
}

impl Default for TokenizationStateStore {
    fn default() -> Self {
        Self::new()
    }
}

/// `RangePriorityQueueImpl`: intervalos de linhas ordenados, sem sobreposição.
#[derive(Default)]
pub struct RangePriorityQueueImpl {
    ranges: Vec<OffsetRange>,
}

impl RangePriorityQueueImpl {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_ranges(&self) -> &[OffsetRange] {
        &self.ranges
    }

    pub fn min(&self) -> Option<i32> {
        self.ranges.first().map(|r| r.start)
    }

    pub fn remove_min(&mut self) -> Option<i32> {
        let range = *self.ranges.first()?;
        if range.start + 1 == range.end_exclusive {
            self.ranges.remove(0);
        } else {
            self.ranges[0] = OffsetRange::new(range.start + 1, range.end_exclusive);
        }
        Some(range.start)
    }

    pub fn delete(&mut self, value: i32) {
        let Some(idx) = self.ranges.iter().position(|r| r.contains(value)) else { return };
        let range = self.ranges[idx];
        if range.start == value {
            if range.end_exclusive == value + 1 {
                self.ranges.remove(idx);
            } else {
                self.ranges[idx] = OffsetRange::new(value + 1, range.end_exclusive);
            }
        } else if range.end_exclusive == value + 1 {
            self.ranges[idx] = OffsetRange::new(range.start, value);
        } else {
            self.ranges.splice(idx..idx + 1, [OffsetRange::new(range.start, value), OffsetRange::new(value + 1, range.end_exclusive)]);
        }
    }

    pub fn add_range(&mut self, range: OffsetRange) {
        OffsetRange::add_range(range, &mut self.ranges);
    }

    pub fn add_range_and_resize(&mut self, range: OffsetRange, new_length: i32) {
        let mut idx_first_might_be_intersecting = 0;
        while !(idx_first_might_be_intersecting >= self.ranges.len() || range.start <= self.ranges[idx_first_might_be_intersecting].end_exclusive) {
            idx_first_might_be_intersecting += 1;
        }
        let mut idx_first_is_after = idx_first_might_be_intersecting;
        while !(idx_first_is_after >= self.ranges.len() || range.end_exclusive < self.ranges[idx_first_is_after].start) {
            idx_first_is_after += 1;
        }
        let delta = new_length - range.length();

        for i in idx_first_is_after..self.ranges.len() {
            self.ranges[i] = self.ranges[i].delta(delta);
        }

        if idx_first_might_be_intersecting == idx_first_is_after {
            let new_range = OffsetRange::new(range.start, range.start + new_length);
            if !new_range.is_empty() {
                self.ranges.insert(idx_first_might_be_intersecting, new_range);
            }
        } else {
            let start = range.start.min(self.ranges[idx_first_might_be_intersecting].start);
            let end_ex = range.end_exclusive.max(self.ranges[idx_first_is_after - 1].end_exclusive);

            let new_range = OffsetRange::new(start, end_ex + delta);
            if !new_range.is_empty() {
                self.ranges.splice(idx_first_might_be_intersecting..idx_first_is_after, [new_range]);
            } else {
                self.ranges.drain(idx_first_might_be_intersecting..idx_first_is_after);
            }
        }
    }

    /// `toString()`.
    pub fn to_display_string(&self) -> String {
        self.ranges.iter().map(ToString::to_string).collect::<Vec<_>>().join(" + ")
    }
}

/// `safeTokenize(...)`: tokeniza com o suporte (um pânico vira `onUnexpectedError`) e, se não houver suporte
/// ou ele falhar, cai no tokenizador nulo. Os tokens voltam em offsets de fim.
pub fn safe_tokenize(
    language_id_codec: &Rc<dyn ILanguageIdCodec>,
    language_id: &str,
    tokenization_support: Option<&Rc<dyn ITokenizationSupport>>,
    text: &JsString,
    has_eol: bool,
    state: &State,
) -> EncodedTokenizationResult {
    let mut r: Option<EncodedTokenizationResult> = None;

    if let Some(support) = tokenization_support {
        let cloned = state.clone_state();
        match errors::catch(|| support.tokenize_encoded(text, has_eol, &cloned)) {
            Ok(result) => r = Some(result),
            Err(e) => errors::on_unexpected_error(&e),
        }
    }

    let mut r = r.unwrap_or_else(|| null_tokenize_encoded(language_id_codec.encode_language_id(language_id), Some(state.clone())));

    LineTokens::convert_to_end_offset(&mut r.tokens, text.len());
    r
}

/// `DefaultBackgroundTokenizer`.
pub struct DefaultBackgroundTokenizer {
    me: Weak<DefaultBackgroundTokenizer>,
    tokenizer_with_state_store: Rc<TokenizerWithStateStoreAndTextModel>,
    background_token_store: Rc<dyn IBackgroundTokenizationStore>,
    is_disposed: Cell<bool>,
    is_scheduled: Cell<bool>,
}

impl DefaultBackgroundTokenizer {
    pub fn new(
        tokenizer_with_state_store: Rc<TokenizerWithStateStoreAndTextModel>,
        background_token_store: Rc<dyn IBackgroundTokenizationStore>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            tokenizer_with_state_store,
            background_token_store,
            is_disposed: Cell::new(false),
            is_scheduled: Cell::new(false),
        })
    }

    fn model(&self) -> Option<Rc<TextModel>> {
        self.tokenizer_with_state_store.text_model.upgrade()
    }

    fn is_attached(&self) -> bool {
        self.model().is_some_and(|m| m.is_attached_to_editor())
    }

    pub fn handle_changes(&self) {
        self.begin_background_tokenization();
    }

    fn begin_background_tokenization(&self) {
        if self.is_scheduled.get() || !self.is_attached() || !self.has_lines_to_tokenize() {
            return;
        }

        self.is_scheduled.set(true);
        let me = self.me.clone();
        run_when_global_idle(
            move |deadline| {
                let Some(me) = me.upgrade() else { return };
                me.is_scheduled.set(false);
                me.background_tokenize_with_deadline(deadline);
            },
            None,
        );
    }

    /// Tokenize until the deadline occurs, but try to yield every 1-2ms.
    fn background_tokenize_with_deadline(self: &Rc<Self>, deadline: IdleDeadline) {
        // Read the time remaining from the `deadline` immediately because it is unclear how
        // the time remaining varies.
        let end_time = runtime::date_now() + deadline.time_remaining();
        self.execute(end_time);
    }

    fn execute(self: &Rc<Self>, end_time: u64) {
        if self.is_disposed.get() || !self.is_attached() || !self.has_lines_to_tokenize() {
            // disposed in the meantime or detached or finished
            return;
        }

        self.background_tokenize_for_at_least_1ms();

        if runtime::date_now() < end_time {
            // There is still time before reaching the deadline, so yield to the browser and then
            // continue execution
            let me = self.clone();
            runtime::set_timeout(move || me.execute(end_time), 0);
        } else {
            // The deadline has been reached, so schedule a new idle callback if necessary
            self.begin_background_tokenization();
        }
    }

    /// Tokenize for at least 1ms.
    fn background_tokenize_for_at_least_1ms(&self) {
        let Some(model) = self.model() else { return };
        let line_count = model.get_line_count();
        let mut builder = ContiguousMultilineTokensBuilder::new();
        let sw = Instant::now();

        loop {
            if sw.elapsed().as_secs_f64() * 1000.0 > 1.0 {
                // the comparison is intentionally > 1ms, such that we always tokenize at least one line
                break;
            }

            let tokenized_line_number = self.tokenize_one_invalid_line(&mut builder, &model);

            if tokenized_line_number >= line_count {
                break;
            }
            if !self.has_lines_to_tokenize() {
                break;
            }
        }

        self.background_token_store.set_tokens(builder.finalize());
        self.check_finished();
    }

    fn has_lines_to_tokenize(&self) -> bool {
        !self.tokenizer_with_state_store.store().borrow().all_states_valid()
    }

    fn tokenize_one_invalid_line(&self, builder: &mut ContiguousMultilineTokensBuilder, model: &TextModel) -> i32 {
        let Some(first_invalid_line) = self.tokenizer_with_state_store.get_first_invalid_line() else {
            return model.get_line_count() + 1;
        };
        self.tokenizer_with_state_store.update_tokens_until_line(builder, first_invalid_line.line_number);
        first_invalid_line.line_number
    }

    pub fn check_finished(&self) {
        if self.is_disposed.get() {
            return;
        }
        if self.tokenizer_with_state_store.store().borrow().all_states_valid() {
            self.background_token_store.background_tokenization_finished();
        }
    }
}

impl IDisposable for DefaultBackgroundTokenizer {
    fn dispose(&self) {
        self.is_disposed.set(true);
    }
}

impl IBackgroundTokenizer for DefaultBackgroundTokenizer {
    fn request_tokens(&self, start_line_number: i32, end_line_number_exclusive: i32) {
        self.tokenizer_with_state_store
            .store()
            .borrow_mut()
            .invalidate_end_state_range(LineRange::new(start_line_number, end_line_number_exclusive));
    }
}
