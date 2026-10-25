//! Porte parcial de `src/vs/editor/browser/widget/multiDiffEditor`: o modelo (`model.ts`). O `MultiDiffEditorViewModel`
//! (`multiDiffEditorViewModel.ts`) está em `je_workbench::contrib::multi_diff_editor::browser::multi_diff_editor_view_model`,
//! porque depende do `ITextModelService` (`createSyntheticDocument`), que mora no `je-workbench` (ver o topo desse arquivo). As
//! opções das variantes (`multiDiffEditorOptions.ts`) continuam em [`crate::multi_diff_editor_options`].

pub mod model;
