//! Porte de `src/vs/workbench/contrib/mergeEditor/browser/model/rangeUtils.ts`.
//!
//! Ficam de fora `lengthOfRange`, `lengthBetweenPositions` e `addLength`: o corpo de cada uma é idêntico ao
//! de `TextLength::of_range`, `TextLength::between_positions` e `TextLength::add_to_position` do `je-editor`,
//! que o `mapping` usa direto (regra DRY do projeto, nada de função de repasse).

use je_editor::range::Range;
use je_editor::position::Position;

/// Diferente de `Range::contains_position`, o fim do intervalo é exclusivo.
pub fn range_contains_position(range: &Range, position: &Position) -> bool {
    if position.line_number < range.start_line_number || position.line_number > range.end_line_number {
        return false;
    }
    if position.line_number == range.start_line_number && position.column < range.start_column {
        return false;
    }
    if position.line_number == range.end_line_number && position.column >= range.end_column {
        return false;
    }
    true
}

pub fn range_is_before_or_touching(range: &Range, other: &Range) -> bool {
    range.end_line_number < other.start_line_number
        || (range.end_line_number == other.start_line_number && range.end_column <= other.start_column)
}
