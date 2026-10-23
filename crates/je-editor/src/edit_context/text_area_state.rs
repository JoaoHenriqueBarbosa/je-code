//! Porte de `src/vs/editor/browser/controller/editContext/textArea/textAreaEditContextState.ts`: o estado
//! (valor e seleção) da textarea de entrada do editor e a dedução do que foi digitado.
//!
//! Texto é UTF-16 ([`JsString`]): valor, deslocamentos e contagens são em unidades.
//!
//! Notas de tradução:
//! - `_debugComposition` é sempre `false` no original; os `console.log` de depuração que ele guarda não
//!   foram portados.
//! - `deduceInput` e `deduceAndroidCompositionInput` têm a guarda `if (!previousState)` para o estado
//!   `EMPTY` vindo como `null`; aqui o estado anterior é uma referência, então a guarda não existe.
//! - Os cálculos de diferença usam `i64` porque o original subtrai números que podem passar de zero
//!   (`currentSelectionEnd - currentValue.length`); as contagens que saem como `usize` nunca são negativas.

use std::fmt;

use je_base::JsString;
use je_base::js_string::substring;
use je_base::strings::{common_prefix_length, common_suffix_length};

use crate::edit_context::screen_reader_utils::ISimpleScreenReaderContentState;
use crate::position::Position;
use crate::range::Range;
use crate::selection::SelectionDirection;

/// `ITextAreaWrapper`.
pub trait ITextAreaWrapper {
    fn get_value(&self) -> JsString;
    fn set_value(&mut self, reason: &str, value: &[u16]);

    fn get_selection_start(&self) -> usize;
    fn get_selection_end(&self) -> usize;
    fn set_selection_range(&mut self, reason: &str, selection_start: usize, selection_end: usize);
}

/// `ITypeData`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITypeData {
    pub text: JsString,
    pub replace_prev_char_cnt: usize,
    pub replace_next_char_cnt: usize,
    pub position_delta: i64,
}

/// Uma contagem que o original calcula por subtração e que nunca fica negativa.
fn to_count(value: i64) -> usize {
    value.max(0) as usize
}

/// `TextAreaState`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextAreaState {
    pub value: JsString,
    /// O deslocamento onde a seleção começa dentro de `value`.
    pub selection_start: usize,
    /// O deslocamento onde a seleção termina dentro de `value`.
    pub selection_end: usize,
    /// O intervalo do editor, nas coordenadas da view, que corresponde à seleção dentro de `value`.
    pub selection: Option<Range>,
    /// A contagem de linhas visíveis (com quebra, não só os `\n`) do texto de `value` antes de `selection_start`.
    pub newline_count_before_selection: Option<usize>,
}

impl fmt::Display for TextAreaState {
    /// `toString()`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[ <{}>, selectionStart: {}, selectionEnd: {}]",
            self.value, self.selection_start, self.selection_end
        )
    }
}

impl TextAreaState {
    /// `TextAreaState.EMPTY`.
    pub const EMPTY: TextAreaState = TextAreaState {
        value: JsString::new(),
        selection_start: 0,
        selection_end: 0,
        selection: None,
        newline_count_before_selection: None,
    };

    pub fn new(
        value: JsString,
        selection_start: usize,
        selection_end: usize,
        selection: Option<Range>,
        newline_count_before_selection: Option<usize>,
    ) -> TextAreaState {
        TextAreaState { value, selection_start, selection_end, selection, newline_count_before_selection }
    }

    /// `readFromTextArea(textArea, previousState)`.
    pub fn read_from_text_area(text_area: &dyn ITextAreaWrapper, previous_state: Option<&TextAreaState>) -> TextAreaState {
        let value = text_area.get_value();
        let selection_start = text_area.get_selection_start();
        let selection_end = text_area.get_selection_end();
        let mut newline_count_before_selection = None;
        if let Some(previous_state) = previous_state {
            let value_before_selection_start = substring(value.units(), 0, selection_start as i64);
            let previous_value_before_selection_start =
                substring(previous_state.value.units(), 0, previous_state.selection_start as i64);
            if value_before_selection_start == previous_value_before_selection_start {
                newline_count_before_selection = previous_state.newline_count_before_selection;
            }
        }
        TextAreaState::new(value, selection_start, selection_end, None, newline_count_before_selection)
    }

    /// `collapseSelection()`.
    pub fn collapse_selection(&self) -> TextAreaState {
        if self.selection_start == self.value.len() {
            return self.clone();
        }
        TextAreaState::new(self.value.clone(), self.value.len(), self.value.len(), None, None)
    }

    /// `isWrittenToTextArea(textArea, select)`.
    pub fn is_written_to_text_area(&self, text_area: &dyn ITextAreaWrapper, select: bool) -> bool {
        let values_equal = self.value == text_area.get_value();
        if !select {
            return values_equal;
        }
        let selections_equal =
            self.selection_start == text_area.get_selection_start() && self.selection_end == text_area.get_selection_end();
        selections_equal && values_equal
    }

    /// `writeToTextArea(reason, textArea, select)`.
    pub fn write_to_text_area(&self, reason: &str, text_area: &mut dyn ITextAreaWrapper, select: bool) {
        text_area.set_value(reason, self.value.units());
        if select {
            text_area.set_selection_range(reason, self.selection_start, self.selection_end);
        }
    }

    /// `deduceEditorPosition(offset)`: a âncora, o deslocamento de colunas (com sinal) e a quantidade de
    /// quebras de linha entre a âncora e `offset`.
    pub fn deduce_editor_position(&self, offset: usize) -> (Option<Position>, i64, usize) {
        let value = self.value.units();
        if offset <= self.selection_start {
            let str = substring(value, offset as i64, self.selection_start as i64);
            return Self::finish_deduce_editor_position(self.selection.map(|s| s.get_start_position()), str, -1);
        }
        if offset >= self.selection_end {
            let str = substring(value, self.selection_end as i64, offset as i64);
            return Self::finish_deduce_editor_position(self.selection.map(|s| s.get_end_position()), str, 1);
        }
        let str1 = substring(value, self.selection_start as i64, offset as i64);
        if !str1.contains(&8230) {
            return Self::finish_deduce_editor_position(self.selection.map(|s| s.get_start_position()), str1, 1);
        }
        let str2 = substring(value, offset as i64, self.selection_end as i64);
        Self::finish_deduce_editor_position(self.selection.map(|s| s.get_end_position()), str2, -1)
    }

    fn finish_deduce_editor_position(anchor: Option<Position>, delta_text: &[u16], signum: i64) -> (Option<Position>, i64, usize) {
        let line_feed_cnt = delta_text.iter().filter(|&&unit| unit == 10).count();
        (anchor, signum * delta_text.len() as i64, line_feed_cnt)
    }

    /// `deduceInput(previousState, currentState, couldBeEmojiInput)`.
    pub fn deduce_input(previous_state: &TextAreaState, current_state: &TextAreaState, _could_be_emoji_input: bool) -> ITypeData {
        let previous_len = previous_state.value.len() as i64;
        let current_len = current_state.value.len() as i64;
        let previous_selection_start = previous_state.selection_start as i64;
        let previous_selection_end = previous_state.selection_end as i64;

        let prefix_length = (common_prefix_length(previous_state.value.units(), current_state.value.units()) as i64)
            .min(previous_selection_start)
            .min(current_state.selection_start as i64);
        let suffix_length = (common_suffix_length(previous_state.value.units(), current_state.value.units()) as i64)
            .min(previous_len - previous_selection_end)
            .min(current_len - current_state.selection_end as i64);
        let current_value = JsString::from(substring(current_state.value.units(), prefix_length, current_len - suffix_length));
        let current_selection_start = current_state.selection_start as i64 - prefix_length;
        let current_selection_end = current_state.selection_end as i64 - prefix_length;

        if current_selection_start == current_selection_end {
            // não há seleção atual
            return ITypeData {
                text: current_value,
                replace_prev_char_cnt: to_count(previous_selection_start - prefix_length),
                replace_next_char_cnt: 0,
                position_delta: 0,
            };
        }

        // há uma seleção atual => caso de composição
        ITypeData {
            text: current_value,
            replace_prev_char_cnt: to_count(previous_selection_end - previous_selection_start),
            replace_next_char_cnt: 0,
            position_delta: 0,
        }
    }

    /// `deduceAndroidCompositionInput(previousState, currentState)`.
    pub fn deduce_android_composition_input(previous_state: &TextAreaState, current_state: &TextAreaState) -> ITypeData {
        if previous_state.value == current_state.value {
            return ITypeData {
                text: JsString::new(),
                replace_prev_char_cnt: 0,
                replace_next_char_cnt: 0,
                position_delta: current_state.selection_end as i64 - previous_state.selection_end as i64,
            };
        }

        let previous_len = previous_state.value.len() as i64;
        let current_len = current_state.value.len() as i64;
        let previous_selection_end = previous_state.selection_end as i64;

        let prefix_length = (common_prefix_length(previous_state.value.units(), current_state.value.units()) as i64)
            .min(previous_selection_end);
        let suffix_length = (common_suffix_length(previous_state.value.units(), current_state.value.units()) as i64)
            .min(previous_len - previous_selection_end);
        let previous_value =
            substring(previous_state.value.units(), prefix_length, previous_len - suffix_length);
        let current_value = JsString::from(substring(current_state.value.units(), prefix_length, current_len - suffix_length));
        let previous_selection_end = previous_selection_end - prefix_length;
        let current_selection_end = current_state.selection_end as i64 - prefix_length;

        ITypeData {
            position_delta: current_selection_end - current_value.len() as i64,
            text: current_value,
            replace_prev_char_cnt: to_count(previous_selection_end),
            replace_next_char_cnt: to_count(previous_value.len() as i64 - previous_selection_end),
        }
    }

    /// `fromScreenReaderContentState(screenReaderContentState)`.
    pub fn from_screen_reader_content_state(screen_reader_content_state: &ISimpleScreenReaderContentState) -> TextAreaState {
        let (selection_start, selection_end) = match screen_reader_content_state.selection.get_direction() {
            SelectionDirection::Ltr => {
                (screen_reader_content_state.selection_start, screen_reader_content_state.selection_end)
            }
            SelectionDirection::Rtl => {
                (screen_reader_content_state.selection_end, screen_reader_content_state.selection_start)
            }
        };
        TextAreaState::new(
            screen_reader_content_state.value.clone(),
            selection_start,
            selection_end,
            Some(*screen_reader_content_state.selection),
            Some(screen_reader_content_state.newline_count_before_selection),
        )
    }
}
