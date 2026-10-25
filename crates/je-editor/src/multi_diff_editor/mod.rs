//! Porte parcial de `src/vs/editor/browser/widget/multiDiffEditor`: o modelo (`model.ts`), o gerenciador de itens
//! virtualizados (`virtualizedItemManager.ts`), o template de cada arquivo (`diffEditorItemTemplate.ts`), as cores
//! (`colors.ts`) e o `ActionRunnerWithContext` (`utils.ts`). O `MultiDiffEditorViewModel` (`multiDiffEditorViewModel.ts`)
//! está em `je_workbench::contrib::multi_diff_editor::browser::multi_diff_editor_view_model`, porque depende do
//! `ITextModelService` (`createSyntheticDocument`), que mora no `je-workbench` (ver o topo desse arquivo); a camada do editor
//! lê o item pelo trait de [`view_model`]. As opções das variantes (`multiDiffEditorOptions.ts`) continuam em
//! [`crate::multi_diff_editor_options`]. A vista virtualizada (`compressedVirtualizedScrollView.ts`) está em
//! [`crate::compressed_virtualized_scroll_view`].
//!
//! Módulos de apoio sem arquivo próprio no original: [`workbench_ui_element_factory`] (`workbenchUIElementFactory.ts`, o
//! contrato com a camada do workbench), [`diff_editor_surface`] (o recorte do `DiffEditorWidget` que o template usa, até o
//! widget de diff existir por inteiro) e [`view_model`] (o recorte do `DocumentDiffItemViewModel`).
//!
//! Fatias que faltam do widget, nesta ordem: `multiDiffEditorWidgetImpl.ts` e `multiDiffEditorWidget.ts`, com os 9 casos
//! restantes de `multiDiffEditorWidget.test.ts`.

pub mod colors;
pub mod diff_editor_item_template;
pub mod diff_editor_surface;
pub mod model;
pub mod utils;
pub mod view_model;
pub mod virtualized_item_manager;
pub mod workbench_ui_element_factory;
