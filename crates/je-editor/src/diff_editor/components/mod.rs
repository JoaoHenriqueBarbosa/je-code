//! Porte de `src/vs/editor/browser/widget/diffEditor/components/`: os dois editores (`diffEditorEditors.ts`), o sash
//! (`diffEditorSash.ts`) e as decorações (`diffEditorDecorations.ts`) da fatia 1, e o visualizador acessível
//! (`accessibleDiffViewer.ts`) da fatia 3b e as view zones (`diffEditorViewZones/`): [`diff_editor_view_zones`] tem o `renderLines.ts`, o
//! `inlineDiffDeletedCodeMargin.ts`, o `copySelection.ts` e o `diffEditorViewZones.ts`.

pub mod accessible_diff_viewer;
pub mod diff_editor_decorations;
pub mod diff_editor_editors;
pub mod diff_editor_sash;
pub mod diff_editor_view_zones;
