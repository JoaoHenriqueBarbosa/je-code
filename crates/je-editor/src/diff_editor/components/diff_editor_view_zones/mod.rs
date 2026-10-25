//! Porte de `src/vs/editor/browser/widget/diffEditor/components/diffEditorViewZones/`: `renderLines.ts` ([`render_lines`]),
//! `inlineDiffDeletedCodeMargin.ts` ([`inline_diff_deleted_code_margin`]), `copySelection.ts` ([`copy_selection`]) e
//! `diffEditorViewZones.ts` ([`diff_editor_view_zones`]), com o `DiffEditorViewZones` e as duas funções que o arquivo exporta,
//! `allowsTrueInlineDiffRendering` e `rangeIsSingleLine`, reexportadas aqui para quem as importa pelo caminho da pasta.

pub mod copy_selection;
#[allow(clippy::module_inception)]
pub mod diff_editor_view_zones;
pub mod inline_diff_deleted_code_margin;
pub mod render_lines;

pub use diff_editor_view_zones::{allows_true_inline_diff_rendering, range_is_single_line};
