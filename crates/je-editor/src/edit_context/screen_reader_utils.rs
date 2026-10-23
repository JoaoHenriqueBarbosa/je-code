//! Porte de `src/vs/editor/browser/controller/editContext/screenReaderUtils.ts` e da interface
//! `ISimpleModel` (`src/vs/editor/common/viewModel/screenReaderSimpleModel.ts`).
//!
//! Texto é UTF-16 ([`JsString`]): comprimentos e deslocamentos contam unidades.
//!
//! Fora do porte, por dependerem de serviços ainda inexistentes: `ariaLabelForScreenReaderContent`
//! (precisa de `IComputedEditorOptions`, `IKeybindingService`, `AccessibilitySupport` e `nls`).

use je_base::JsString;

use crate::model::EndOfLinePreference;
use crate::position::Position;
use crate::range::Range;
use crate::selection::{Selection, SelectionDirection};
use crate::text_model::TextModel;

/// `ISimpleModel`: o pedaço do modelo que o conteúdo para leitor de tela consulta.
pub trait ISimpleModel {
    fn get_line_content(&self, line_number: i32) -> JsString;
    fn get_line_count(&self) -> i32;
    fn get_line_max_column(&self, line_number: i32) -> i32;
    fn get_value_in_range(&self, range: &Range, eol: EndOfLinePreference) -> JsString;
    fn get_value_length_in_range(&self, range: &Range, eol: EndOfLinePreference) -> usize;
    fn modify_position(&self, position: Position, offset: i64) -> Position;
}

impl ISimpleModel for TextModel {
    fn get_line_content(&self, line_number: i32) -> JsString {
        TextModel::get_line_content(self, line_number)
    }

    fn get_line_count(&self) -> i32 {
        TextModel::get_line_count(self)
    }

    fn get_line_max_column(&self, line_number: i32) -> i32 {
        TextModel::get_line_max_column(self, line_number)
    }

    fn get_value_in_range(&self, range: &Range, eol: EndOfLinePreference) -> JsString {
        self.get_value_in_range_with(range, eol)
    }

    fn get_value_length_in_range(&self, range: &Range, eol: EndOfLinePreference) -> usize {
        self.get_value_length_in_range_with(range, eol)
    }

    fn modify_position(&self, position: Position, offset: i64) -> Position {
        TextModel::modify_position(self, position, offset)
    }
}

/// `IPagedScreenReaderStrategy<T>`.
pub trait IPagedScreenReaderStrategy<T> {
    fn from_editor_selection(&self, model: &dyn ISimpleModel, selection: &Selection, lines_per_page: i32, trim_long_text: bool) -> T;
}

/// `ISimpleScreenReaderContentState`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ISimpleScreenReaderContentState {
    pub value: JsString,
    /// O deslocamento onde a seleção começa dentro de `value`.
    pub selection_start: usize,
    /// O deslocamento onde a seleção termina dentro de `value`.
    pub selection_end: usize,
    /// O intervalo do editor, nas coordenadas da view, que corresponde à seleção dentro de `value`.
    pub selection: Selection,
    /// A posição do começo de `value` no editor.
    pub start_position_within_editor: Position,
    /// A contagem de linhas visíveis (com quebra, não só os `\n`) do texto de `value` antes de `selection_start`.
    pub newline_count_before_selection: usize,
}

/// `…` (U+2026), o `String.fromCharCode(8230)` do original.
const ELLIPSIS: u16 = 8230;

/// `SimplePagedScreenReaderStrategy`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SimplePagedScreenReaderStrategy;

impl SimplePagedScreenReaderStrategy {
    fn get_page_of_line(line_number: i32, lines_per_page: i32) -> i32 {
        (line_number - 1).div_euclid(lines_per_page)
    }

    fn get_range_for_page(page: i32, lines_per_page: i32) -> Range {
        let offset = page * lines_per_page;
        let start_line_number = offset + 1;
        let end_line_number = offset + lines_per_page;
        Range::new(start_line_number, 1, end_line_number + 1, 1)
    }
}

impl IPagedScreenReaderStrategy<ISimpleScreenReaderContentState> for SimplePagedScreenReaderStrategy {
    fn from_editor_selection(
        &self,
        model: &dyn ISimpleModel,
        selection: &Selection,
        lines_per_page: i32,
        trim_long_text: bool,
    ) -> ISimpleScreenReaderContentState {
        // O Chromium lida muito mal com texto de até poucos milhares de caracteres,
        // então corta o texto para não travar a interface inteira.
        const LIMIT_CHARS: usize = 500;

        let selection_start_page = Self::get_page_of_line(selection.start_line_number, lines_per_page);
        let selection_start_page_range = Self::get_range_for_page(selection_start_page, lines_per_page);

        let selection_end_page = Self::get_page_of_line(selection.end_line_number, lines_per_page);
        let selection_end_page_range = Self::get_range_for_page(selection_end_page, lines_per_page);

        let mut pretext_range = selection_start_page_range
            .intersect_ranges(&Range::new(1, 1, selection.start_line_number, selection.start_column))
            .expect("a página da seleção cruza o texto antes dela");
        if trim_long_text && model.get_value_length_in_range(&pretext_range, EndOfLinePreference::Lf) > LIMIT_CHARS {
            let pretext_start = model.modify_position(pretext_range.get_end_position(), -(LIMIT_CHARS as i64));
            pretext_range = Range::from_positions(pretext_start, pretext_range.get_end_position());
        }
        let pretext = model.get_value_in_range(&pretext_range, EndOfLinePreference::Lf);

        let last_line = model.get_line_count();
        let last_line_max_column = model.get_line_max_column(last_line);
        let mut posttext_range = selection_end_page_range
            .intersect_ranges(&Range::new(selection.end_line_number, selection.end_column, last_line, last_line_max_column))
            .expect("a página da seleção cruza o texto depois dela");
        if trim_long_text && model.get_value_length_in_range(&posttext_range, EndOfLinePreference::Lf) > LIMIT_CHARS {
            let posttext_end = model.modify_position(posttext_range.get_start_position(), LIMIT_CHARS as i64);
            posttext_range = Range::from_positions(posttext_range.get_start_position(), posttext_end);
        }
        let posttext = model.get_value_in_range(&posttext_range, EndOfLinePreference::Lf);

        let mut text: JsString;
        if selection_start_page == selection_end_page || selection_start_page + 1 == selection_end_page {
            // pega a seleção inteira
            text = model.get_value_in_range(selection, EndOfLinePreference::Lf);
        } else {
            let selection_range1 = selection_start_page_range
                .intersect_ranges(selection)
                .expect("a seleção cruza a página inicial");
            let selection_range2 = selection_end_page_range
                .intersect_ranges(selection)
                .expect("a seleção cruza a página final");
            text = model.get_value_in_range(&selection_range1, EndOfLinePreference::Lf);
            text.push_unit(ELLIPSIS);
            text.push_units(model.get_value_in_range(&selection_range2, EndOfLinePreference::Lf).units());
        }
        if trim_long_text && text.len() > 2 * LIMIT_CHARS {
            let units = text.units();
            let mut trimmed = JsString::from(&units[..LIMIT_CHARS]);
            trimmed.push_unit(ELLIPSIS);
            trimmed.push_units(&units[units.len() - LIMIT_CHARS..]);
            text = trimmed;
        }

        let (selection_start, selection_end) = if selection.get_direction() == SelectionDirection::Ltr {
            (pretext.len(), pretext.len() + text.len())
        } else {
            (pretext.len() + text.len(), pretext.len())
        };

        let mut value = pretext;
        value.push_units(text.units());
        value.push_units(posttext.units());

        ISimpleScreenReaderContentState {
            value,
            selection: *selection,
            selection_start,
            selection_end,
            start_position_within_editor: pretext_range.get_start_position(),
            newline_count_before_selection: (pretext_range.end_line_number - pretext_range.start_line_number) as usize,
        }
    }
}

/// `newlinecount`: quantos `\n` há no texto.
pub fn newlinecount(text: &[u16]) -> usize {
    text.iter().filter(|&&unit| unit == 10).count()
}
