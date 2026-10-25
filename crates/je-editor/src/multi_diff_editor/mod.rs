//! Porte parcial de `src/vs/editor/browser/widget/multiDiffEditor`: o modelo (`model.ts`) e o gerenciador de itens
//! virtualizados (`virtualizedItemManager.ts`). O `MultiDiffEditorViewModel` (`multiDiffEditorViewModel.ts`) está em
//! `je_workbench::contrib::multi_diff_editor::browser::multi_diff_editor_view_model`, porque depende do `ITextModelService`
//! (`createSyntheticDocument`), que mora no `je-workbench` (ver o topo desse arquivo). As opções das variantes
//! (`multiDiffEditorOptions.ts`) continuam em [`crate::multi_diff_editor_options`]. A vista virtualizada
//! (`compressedVirtualizedScrollView.ts`) está em [`crate::compressed_virtualized_scroll_view`].
//!
//! Fatias que faltam do widget, nesta ordem: `diffEditorItemTemplate.ts`, `multiDiffEditorWidgetImpl.ts` e
//! `multiDiffEditorWidget.ts`, com os 9 casos restantes de `multiDiffEditorWidget.test.ts`.

pub mod model;
pub mod virtualized_item_manager;
