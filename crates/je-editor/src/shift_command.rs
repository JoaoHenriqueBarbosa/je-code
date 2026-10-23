//! Porte de `src/vs/editor/common/commands/shiftCommand.ts`: indentar e desindentar as linhas de uma seleção.
//!
//! Notas de tradução:
//! - O `@ILanguageConfigurationService` injetado no construtor é o parâmetro explícito
//!   `language_configuration_service`.
//! - `cachedStringRepeat` é só um cache de desempenho das repetições; aqui a repetição é refeita a cada
//!   chamada ([`repeat_units`]), com o mesmo resultado.
//! - `strings.firstNonWhitespaceIndex` devolve `-1` no original quando a linha é só espaço em branco; o
//!   índice daqui é `i32` e carrega o mesmo `-1`, porque `indentationEndIndex` também é decrementado e
//!   comparado a `0` em seguida.
//! - O `for` do original, que atualiza `previousLineExtraSpaces = extraSpaces` também nos `continue`, vira
//!   a atribuição no começo de cada volta (o valor lido é o `extraSpaces` da volta anterior).

use crate::cursor_common::CursorSimpleModel;
use std::rc::Rc;

use je_base::JsString;
use je_base::char_code as cc;
use je_base::js_string::is_js_whitespace;
use je_base::strings::first_non_whitespace_index;

use crate::config::editor_options::EditorAutoIndentStrategy;
use crate::cursor_columns::{
    next_indent_tab_stop, next_render_tab_stop, prev_indent_tab_stop, prev_render_tab_stop, visible_column_from_column,
};
use crate::editor_common::{Command, CursorStateComputerData, EditOperationBuilder};
use crate::enter_action::get_enter_action;
use crate::language_configuration_registry::ILanguageConfigurationService;
use crate::range::Range;
use crate::selection::{Selection, SelectionDirection};
use crate::text_model::TextModel;

/// `IShiftCommandOpts`.
#[derive(Clone, Copy)]
pub struct ShiftCommandOpts {
    pub is_unshift: bool,
    pub tab_size: i32,
    pub indent_size: i32,
    pub insert_spaces: bool,
    pub use_tab_stops: bool,
    pub auto_indent: EditorAutoIndentStrategy,
}

/// `cachedStringRepeat(str, count)`: `count <= 0` dá o texto vazio.
fn repeat_units(unit: &[u16], count: i32) -> JsString {
    let mut out = JsString::new();
    for _ in 0..count.max(0) {
        out.push_units(unit);
    }
    out
}

/// `ShiftCommand`.
pub struct ShiftCommand {
    opts: ShiftCommandOpts,
    selection: Selection,
    language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    selection_id: Option<String>,
    use_last_edit_range_for_cursor_end_position: bool,
    selection_start_column_stays_put: bool,
}

impl ShiftCommand {
    /// `ShiftCommand.unshiftIndent`.
    pub fn unshift_indent(line: &[u16], column: i32, tab_size: i32, indent_size: i32, insert_spaces: bool) -> JsString {
        // Determina a coluna visível em que o conteúdo começa.
        let content_start_visible_column = visible_column_from_column(line, column, tab_size);

        if insert_spaces {
            let indent = repeat_units(&[cc::Space], indent_size);
            let desired_tab_stop = prev_indent_tab_stop(content_start_visible_column, indent_size);
            let indent_count = desired_tab_stop / indent_size; // será um inteiro
            repeat_units(indent.units(), indent_count)
        } else {
            let indent = [cc::Tab];
            let desired_tab_stop = prev_render_tab_stop(content_start_visible_column, tab_size);
            let indent_count = desired_tab_stop / tab_size; // será um inteiro
            repeat_units(&indent, indent_count)
        }
    }

    /// `ShiftCommand.shiftIndent`.
    pub fn shift_indent(line: &[u16], column: i32, tab_size: i32, indent_size: i32, insert_spaces: bool) -> JsString {
        // Determina a coluna visível em que o conteúdo começa.
        let content_start_visible_column = visible_column_from_column(line, column, tab_size);

        if insert_spaces {
            let indent = repeat_units(&[cc::Space], indent_size);
            let desired_tab_stop = next_indent_tab_stop(content_start_visible_column, indent_size);
            let indent_count = desired_tab_stop / indent_size; // será um inteiro
            repeat_units(indent.units(), indent_count)
        } else {
            let indent = [cc::Tab];
            let desired_tab_stop = next_render_tab_stop(content_start_visible_column, tab_size);
            let indent_count = desired_tab_stop / tab_size; // será um inteiro
            repeat_units(&indent, indent_count)
        }
    }

    pub fn new(
        range: Selection,
        opts: ShiftCommandOpts,
        language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    ) -> Self {
        Self {
            opts,
            selection: range,
            language_configuration_service,
            selection_id: None,
            use_last_edit_range_for_cursor_end_position: false,
            selection_start_column_stays_put: false,
        }
    }

    fn add_edit_operation(&self, builder: &mut dyn EditOperationBuilder, range: Range, text: JsString) {
        if self.use_last_edit_range_for_cursor_end_position {
            builder.add_tracked_edit_operation(range, Some(text), false);
        } else {
            builder.add_edit_operation(range, Some(text), false);
        }
    }
}

impl Command for ShiftCommand {
    fn get_edit_operations(&mut self, model: &TextModel, builder: &mut dyn EditOperationBuilder) {
        let start_line = self.selection.start_line_number;

        let mut end_line = self.selection.end_line_number;
        if self.selection.end_column == 1 && start_line != end_line {
            end_line -= 1;
        }

        let ShiftCommandOpts { tab_size, indent_size, insert_spaces, is_unshift, .. } = self.opts;
        let should_indent_empty_lines = start_line == end_line;

        if self.opts.use_tab_stops {
            // Se está indentando ou desindentando numa linha só de espaço em branco.
            if self.selection.is_empty() {
                // `/^\s*$/.test(model.getLineContent(startLine))`
                if model.get_line_content(start_line).units().iter().all(|&unit| is_js_whitespace(unit)) {
                    self.use_last_edit_range_for_cursor_end_position = true;
                }
            }

            // Guarda o "desalinhamento" da linha anterior.
            let mut extra_spaces = 0;
            for line_number in start_line..=end_line {
                let previous_line_extra_spaces = extra_spaces;
                extra_spaces = 0;
                let line_text = model.get_line_content(line_number);
                let text = line_text.units();
                let mut indentation_end_index = first_non_whitespace_index(text).map_or(-1, |index| index as i32);

                if is_unshift && (text.is_empty() || indentation_end_index == 0) {
                    // Linha vazia ou sem espaço em branco no começo: nada a fazer.
                    continue;
                }

                if !should_indent_empty_lines && !is_unshift && text.is_empty() {
                    // Não indenta linhas vazias: nada a fazer.
                    continue;
                }

                if indentation_end_index == -1 {
                    // A linha inteira é espaço em branco.
                    indentation_end_index = text.len() as i32;
                }

                if line_number > 1 {
                    let content_start_visible_column = visible_column_from_column(text, indentation_end_index + 1, tab_size);
                    if content_start_visible_column % indent_size != 0 {
                        // A linha atual está "desalinhada", então vê se era de se esperar...
                        // Isso só acontece com vírgulas no fim da indentação.
                        if model.tokenization().is_cheap_to_tokenize(line_number - 1) {
                            let previous_line_max_column = model.get_line_max_column(line_number - 1);
                            let enter_action = get_enter_action(
                                self.opts.auto_indent,
                                model,
                                &Range::new(line_number - 1, previous_line_max_column, line_number - 1, previous_line_max_column),
                                &*self.language_configuration_service,
                            );
                            if let Some(enter_action) = enter_action {
                                extra_spaces = previous_line_extra_spaces;
                                for unit in enter_action.append_text.encode_utf16() {
                                    if extra_spaces >= indent_size {
                                        break;
                                    }
                                    if unit == cc::Space {
                                        extra_spaces += 1;
                                    } else {
                                        break;
                                    }
                                }
                                if enter_action.remove_text != 0 {
                                    extra_spaces = 0.max(extra_spaces - enter_action.remove_text);
                                }

                                // Age como se `prefixSpaces` não fizesse parte da indentação.
                                for _ in 0..extra_spaces {
                                    if indentation_end_index == 0 || text[(indentation_end_index - 1) as usize] != cc::Space {
                                        break;
                                    }
                                    indentation_end_index -= 1;
                                }
                            }
                        }
                    }
                }

                if is_unshift && indentation_end_index == 0 {
                    // Linha sem espaço em branco no começo: nada a fazer.
                    continue;
                }

                let desired_indent = if is_unshift {
                    ShiftCommand::unshift_indent(text, indentation_end_index + 1, tab_size, indent_size, insert_spaces)
                } else {
                    ShiftCommand::shift_indent(text, indentation_end_index + 1, tab_size, indent_size, insert_spaces)
                };

                self.add_edit_operation(builder, Range::new(line_number, 1, line_number, indentation_end_index + 1), desired_indent);
                if line_number == start_line && !self.selection.is_empty() {
                    // Força a coluna inicial a ficar onde está, porque a inserção é depois dela.
                    self.selection_start_column_stays_put = self.selection.start_column <= indentation_end_index + 1;
                }
            }
        } else {
            // Se está indentando ou desindentando numa linha só de espaço em branco.
            if !is_unshift && self.selection.is_empty() && model.get_line_content(start_line).is_empty() {
                self.use_last_edit_range_for_cursor_end_position = true;
            }

            let one_indent = if insert_spaces { repeat_units(&[cc::Space], indent_size) } else { JsString::from_units(vec![cc::Tab]) };

            for line_number in start_line..=end_line {
                let line_text = model.get_line_content(line_number);
                let text = line_text.units();
                let mut indentation_end_index = first_non_whitespace_index(text).map_or(-1, |index| index as i32);

                if is_unshift && (text.is_empty() || indentation_end_index == 0) {
                    // Linha vazia ou sem espaço em branco no começo: nada a fazer.
                    continue;
                }

                if !should_indent_empty_lines && !is_unshift && text.is_empty() {
                    // Não indenta linhas vazias: nada a fazer.
                    continue;
                }

                if indentation_end_index == -1 {
                    // A linha inteira é espaço em branco.
                    indentation_end_index = text.len() as i32;
                }

                if is_unshift && indentation_end_index == 0 {
                    // Linha sem espaço em branco no começo: nada a fazer.
                    continue;
                }

                if is_unshift {
                    indentation_end_index = indentation_end_index.min(indent_size);
                    for i in 0..indentation_end_index {
                        if text[i as usize] == cc::Tab {
                            indentation_end_index = i + 1;
                            break;
                        }
                    }

                    self.add_edit_operation(builder, Range::new(line_number, 1, line_number, indentation_end_index + 1), JsString::new());
                } else {
                    self.add_edit_operation(builder, Range::new(line_number, 1, line_number, 1), one_indent.clone());
                    if line_number == start_line && !self.selection.is_empty() {
                        // Força a coluna inicial a ficar onde está, porque a inserção é depois dela.
                        self.selection_start_column_stays_put = self.selection.start_column == 1;
                    }
                }
            }
        }

        self.selection_id = Some(builder.track_selection(self.selection, None));
    }

    fn compute_cursor_state(&mut self, _model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
        if self.use_last_edit_range_for_cursor_end_position {
            let last_op_range = helper.get_inverse_edit_operations()[0].range;
            return Selection::new(
                last_op_range.end_line_number,
                last_op_range.end_column,
                last_op_range.end_line_number,
                last_op_range.end_column,
            );
        }
        let result = helper.get_tracked_selection(self.selection_id.as_deref().expect("getEditOperations roda antes de computeCursorState"));

        if self.selection_start_column_stays_put {
            // O começo da seleção não deve se mover.
            let initial_start_column = self.selection.start_column;
            let result_start_column = result.start_column;
            if result_start_column <= initial_start_column {
                return result;
            }

            if result.get_direction() == SelectionDirection::Ltr {
                return Selection::new(result.start_line_number, initial_start_column, result.end_line_number, result.end_column);
            }
            return Selection::new(result.end_line_number, result.end_column, result.start_line_number, initial_start_column);
        }

        result
    }
}
