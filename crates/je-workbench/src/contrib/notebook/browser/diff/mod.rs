//! Porte parcial de `src/vs/workbench/contrib/notebook/browser/diff`: a lógica do diff de notebooks (os view models, o
//! despachante de eventos de layout, o cálculo de altura dos editores e os contratos que eles usam). O editor de diff, a
//! lista, as ações e o overview ruler (`notebookDiffEditor.ts`, `notebookDiffList.ts`, `notebookDiffActions.ts`,
//! `notebookDiffOverviewRuler.ts`, `diffComponents.ts`, `diffElementOutputs.ts`, `unchangedEditorRegions.ts`,
//! `notebookMultiDiffEditor*.ts`) são UI e ficam para a fatia dos widgets nativos.

pub mod diff_cell_editor_options;
pub mod diff_element_view_model;
pub mod diff_nested_cell_view_model;
pub mod editor_height_calculator;
pub mod event_dispatcher;
pub mod notebook_diff_editor_browser;
pub mod notebook_diff_view_model;
