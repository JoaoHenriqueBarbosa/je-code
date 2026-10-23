//! Porte de `src/vs/editor/common/cursor/cursorDeleteOperations.ts`.
//!
//! Notas de tradução:
//! - A classe estática `DeleteOperations` é a struct sem campos [`DeleteOperations`] com as funções associadas.
//! - `ICursorSimpleModel` é `&dyn CursorSimpleModel` e `Array<ICommand | null>` é
//!   `Vec<Option<Box<dyn Command>>>`; `[boolean, ...]` é uma tupla.
//! - `EditorAutoClosingStrategy` e `EditorAutoClosingEditStrategy` são `&str` (ver `cursor_common.rs`).
//! - `cut` ordena `selections` no lugar (efeito colateral do original), então recebe `&mut [Selection]`. O
//!   comparador do original, `Position.compare(a.start, b.end)`, não é uma ordem total (compara a si mesmo
//!   como "menor"), e o `sort_by` do Rust pode entrar em pânico com isso. O `Array.prototype.sort` do V8 é a
//!   ordenação por inserção binária depois de achar a primeira corrida (`CountAndMakeRun` e
//!   `BinaryInsertionSort` do TimSort), que `je_base::arrays::sort_by_compare` reproduz.

use je_base::JsString;
use je_base::arrays::sort_by_compare;
use je_base::strings;

use crate::cursor_columns;
use crate::cursor_common::{
    CursorConfiguration, CursorSimpleModel, EditOperationResult, EditOperationResultOptions, EditOperationType, is_quote,
};
use crate::cursor_move_operations;
use crate::editor_common::Command;
use crate::language_configuration::AutoClosingPairsMap;
use crate::position::Position;
use crate::range::Range;
use crate::replace_command::ReplaceCommand;
use crate::selection::Selection;

/// `new ReplaceCommand(range, '')`.
fn delete_command(range: Range) -> Box<dyn Command> {
    Box::new(ReplaceCommand::new(range, JsString::new(), None))
}

/// `DeleteOperations`.
pub struct DeleteOperations;

impl DeleteOperations {
    pub fn delete_right(
        prev_edit_operation_type: EditOperationType,
        config: &CursorConfiguration,
        model: &dyn CursorSimpleModel,
        selections: &[Selection],
    ) -> (bool, Vec<Option<Box<dyn Command>>>) {
        let mut commands: Vec<Option<Box<dyn Command>>> = Vec::with_capacity(selections.len());
        let mut should_push_stack_element_before = prev_edit_operation_type != EditOperationType::DeletingRight;
        for selection in selections {
            let delete_selection = Self::get_delete_right_range(selection, model, config);

            if delete_selection.is_empty() {
                // Provavelmente no fim do arquivo: ignora
                commands.push(None);
                continue;
            }

            if delete_selection.start_line_number != delete_selection.end_line_number {
                should_push_stack_element_before = true;
            }

            commands.push(Some(delete_command(delete_selection)));
        }
        (should_push_stack_element_before, commands)
    }

    fn get_delete_right_range(selection: &Selection, model: &dyn CursorSimpleModel, config: &CursorConfiguration) -> Range {
        if !selection.is_empty() {
            return Range::new(
                selection.start_line_number,
                selection.start_column,
                selection.end_line_number,
                selection.end_column,
            );
        }

        let position = selection.get_position();
        let right_of_position = cursor_move_operations::right(config, model, position);

        if config.trim_whitespace_on_delete && right_of_position.line_number != position.line_number {
            // A junção inteligente de linhas (apagando a indentação) está ligada
            // (e) o Delete acontece no fim de uma linha
            let current_line_has_content = model.get_line_first_non_whitespace_column(position.line_number) > 0;
            let first_non_whitespace_column = model.get_line_first_non_whitespace_column(right_of_position.line_number);
            if current_line_has_content && first_non_whitespace_column > 0 {
                // A linha seguinte tem conteúdo
                return Range::new(
                    right_of_position.line_number,
                    first_non_whitespace_column,
                    position.line_number,
                    position.column,
                );
            }
        }

        Range::new(right_of_position.line_number, right_of_position.column, position.line_number, position.column)
    }

    pub fn is_auto_closing_pair_delete(
        auto_closing_delete: &str,
        auto_closing_brackets: &str,
        auto_closing_quotes: &str,
        auto_closing_pairs_open: &AutoClosingPairsMap,
        model: &dyn CursorSimpleModel,
        selections: &[Selection],
        auto_closed_characters: &[Range],
    ) -> bool {
        if auto_closing_brackets == "never" && auto_closing_quotes == "never" {
            return false;
        }
        if auto_closing_delete == "never" {
            return false;
        }

        for selection in selections {
            let position = selection.get_position();

            if !selection.is_empty() {
                return false;
            }

            let line_text = model.get_line_content(position.line_number);
            let line_units = line_text.units();
            if position.column < 2 || position.column >= line_units.len() as i32 + 1 {
                return false;
            }
            let character = line_units[(position.column - 2) as usize];

            let Some(auto_closing_pair_candidates) =
                auto_closing_pairs_open.get(&JsString::from_units(vec![character]))
            else {
                return false;
            };

            if is_quote(&[character]) {
                if auto_closing_quotes == "never" {
                    return false;
                }
            } else if auto_closing_brackets == "never" {
                return false;
            }

            let after_character = line_units[(position.column - 1) as usize];

            let mut found_auto_closing_pair = false;
            for candidate in auto_closing_pair_candidates {
                if candidate.open.encode_utf16().eq([character]) && candidate.close.encode_utf16().eq([after_character]) {
                    found_auto_closing_pair = true;
                }
            }
            if !found_auto_closing_pair {
                return false;
            }

            // Só apaga o par se ele foi inserido automaticamente pelo editor
            if auto_closing_delete == "auto" {
                let found = auto_closed_characters.iter().any(|auto_closed_character| {
                    position.line_number == auto_closed_character.start_line_number
                        && position.column == auto_closed_character.start_column
                });
                if !found {
                    return false;
                }
            }
        }

        true
    }

    fn run_auto_closing_pair_delete(selections: &[Selection]) -> (bool, Vec<Box<dyn Command>>) {
        let commands = selections
            .iter()
            .map(|selection| {
                let position = selection.get_position();
                delete_command(Range::new(position.line_number, position.column - 1, position.line_number, position.column + 1))
            })
            .collect();
        (true, commands)
    }

    pub fn delete_left(
        prev_edit_operation_type: EditOperationType,
        config: &CursorConfiguration,
        model: &dyn CursorSimpleModel,
        selections: &[Selection],
        auto_closed_characters: &[Range],
    ) -> (bool, Vec<Option<Box<dyn Command>>>) {
        if Self::is_auto_closing_pair_delete(
            &config.auto_closing_delete,
            &config.auto_closing_brackets,
            &config.auto_closing_quotes,
            &config.auto_closing_pairs.auto_closing_pairs_open_by_end,
            model,
            selections,
            auto_closed_characters,
        ) {
            let (should_push_stack_element_before, commands) = Self::run_auto_closing_pair_delete(selections);
            return (should_push_stack_element_before, commands.into_iter().map(Some).collect());
        }

        let mut commands: Vec<Option<Box<dyn Command>>> = Vec::with_capacity(selections.len());
        let mut should_push_stack_element_before = prev_edit_operation_type != EditOperationType::DeletingLeft;
        for selection in selections {
            let delete_range = Self::get_delete_left_range(selection, model, config);

            // Ignora intervalos de remoção vazios, que não têm efeito.
            // Acontecem se o cursor está no começo do arquivo.
            if delete_range.is_empty() {
                commands.push(None);
                continue;
            }

            if delete_range.start_line_number != delete_range.end_line_number {
                should_push_stack_element_before = true;
            }

            commands.push(Some(delete_command(delete_range)));
        }
        (should_push_stack_element_before, commands)
    }

    fn get_delete_left_range(selection: &Selection, model: &dyn CursorSimpleModel, config: &CursorConfiguration) -> Range {
        if !selection.is_empty() {
            return Range::new(
                selection.start_line_number,
                selection.start_column,
                selection.end_line_number,
                selection.end_column,
            );
        }

        let position = selection.get_position();

        // Reduz a indentação quando usa paradas de tabulação e o cursor está dentro da indentação
        if config.use_tab_stops && position.column > 1 {
            let line_content = model.get_line_content(position.line_number);

            let last_indentation_column = match strings::first_non_whitespace_index(line_content.units()) {
                // a linha inteira é espaço em branco
                None => line_content.len() as i32 + 1,
                Some(first_non_whitespace_index) => first_non_whitespace_index as i32 + 1,
            };

            if position.column <= last_indentation_column {
                let from_visible_column = config.visible_column_from_column(model, position);
                let to_visible_column = cursor_columns::prev_indent_tab_stop(from_visible_column, config.indent_size);
                let to_column = config.column_from_visible_column(model, position.line_number, to_visible_column);
                return Range::new(position.line_number, to_column, position.line_number, position.column);
            }
        }

        Range::from_positions(Self::get_position_after_delete_left(position, model), position)
    }

    fn get_position_after_delete_left(position: Position, model: &dyn CursorSimpleModel) -> Position {
        if position.column > 1 {
            // Converte colunas a partir de 1 em deslocamentos a partir de 0 e de volta.
            let idx = strings::get_left_delete_offset(
                (position.column - 1) as usize,
                model.get_line_content(position.line_number).units(),
            );
            position.with(position.line_number, idx as i32 + 1)
        } else if position.line_number > 1 {
            let new_line = position.line_number - 1;
            Position::new(new_line, model.get_line_max_column(new_line))
        } else {
            position
        }
    }

    pub fn cut(config: &CursorConfiguration, model: &dyn CursorSimpleModel, selections: &mut [Selection]) -> EditOperationResult {
        let mut commands: Vec<Option<Box<dyn Command>>> = Vec::with_capacity(selections.len());
        let mut last_cut_range: Option<Range> = None;
        sort_by_compare(selections, |a, b| f64::from(Position::compare(&a.get_start_position(), &b.get_end_position())));
        for selection in selections.iter() {
            if selection.is_empty() {
                if config.empty_selection_clipboard {
                    // Isto é um corte de linha inteira

                    let position = selection.get_position();

                    let (start_line_number, start_column, end_line_number, end_column);

                    if position.line_number < model.get_line_count() {
                        // Cortando uma linha no meio do modelo
                        start_line_number = position.line_number;
                        start_column = 1;
                        end_line_number = position.line_number + 1;
                        end_column = 1;
                    } else if position.line_number > 1
                        && last_cut_range.map(|range| range.end_line_number) != Some(position.line_number)
                    {
                        // Cortando a última linha, há mais de 1 linha no modelo e um corte anterior não toca este
                        start_line_number = position.line_number - 1;
                        start_column = model.get_line_max_column(position.line_number - 1);
                        end_line_number = position.line_number;
                        end_column = model.get_line_max_column(position.line_number);
                    } else {
                        // Cortando a única linha que o modelo contém
                        start_line_number = position.line_number;
                        start_column = 1;
                        end_line_number = position.line_number;
                        end_column = model.get_line_max_column(position.line_number);
                    }

                    let delete_selection = Range::new(start_line_number, start_column, end_line_number, end_column);
                    last_cut_range = Some(delete_selection);

                    commands.push((!delete_selection.is_empty()).then(|| delete_command(delete_selection)));
                } else {
                    // Não dá para cortar uma seleção vazia
                    commands.push(None);
                }
            } else {
                commands.push(Some(delete_command(Range::new(
                    selection.start_line_number,
                    selection.start_column,
                    selection.end_line_number,
                    selection.end_column,
                ))));
            }
        }
        EditOperationResult::new(
            EditOperationType::Other,
            commands,
            EditOperationResultOptions {
                should_push_stack_element_before: true,
                should_push_stack_element_after: true,
                follow_up: None,
            },
        )
    }
}
