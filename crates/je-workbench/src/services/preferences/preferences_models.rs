//! Porte parcial de `src/vs/workbench/services/preferences/common/preferencesModels.ts`: só o `nullRange`, o intervalo
//! sentinela das configurações que não têm posição num arquivo (as do registro de configuração), e o `isNullRange`.
//! O resto do arquivo (`SettingsEditorModel`, `Settings2EditorModel`, `DefaultSettings`...) entra com o porte das telas
//! de preferências.
//!
//! Divergência: `IRange` é o [`Range`] do `je-editor` (o mesmo tipo), que aqui se escreve como literal, porque
//! `Range::new` reordena início e fim e o sentinela não é um intervalo válido.

use je_editor::range::Range;

/// `nullRange`.
pub const NULL_RANGE: Range = Range { start_line_number: -1, start_column: -1, end_line_number: -1, end_column: -1 };

/// `isNullRange`.
pub fn is_null_range(range: &Range) -> bool {
    range.start_line_number == -1 && range.start_column == -1 && range.end_line_number == -1 && range.end_column == -1
}
