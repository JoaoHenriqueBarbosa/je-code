//! Porte de `src/vs/workbench/contrib/markers`.
//!
//! Estado do painel de problemas (`browser/`):
//! - Fatia 1: `markersTreeViewer.ts` ([`markers_tree_viewer`]: provedor de acessibilidade, delegado de altura,
//!   renderers, filtro, `MarkerViewModel` e `MarkersViewModel`) e `markersViewActions.ts` ([`markers_view_actions`]:
//!   `MarkersFilters`, `QuickFixAction` e `QuickFixActionViewItem`). Entraram junto os módulos de que os dois dependem e
//!   que o porte não tinha: `messages.ts` ([`messages`]) e `common/markers.ts` ([`markers_common`]: `MarkersViewMode`,
//!   `Markers` e `MarkersContextKeys`).
//! - Fatia 2 (em andamento): `markersTable.ts` ([`markers_table`]: a `MarkersTable`, os renderers de coluna, o delegado e o
//!   filtro da tabela) entrou, com o que ela precisa e o porte não tinha: a `Table` (`base/browser/ui/table`, em `je-widgets`),
//!   a `WorkbenchTable` (`platform/list/browser/listService.ts`, em [`crate::browser::workbench_table`]) e o `Link`
//!   (`platform/opener/browser/link.ts`, em [`crate::platform::opener::link`]). De `markersView.ts` entrou só o contrato
//!   `IProblemsWidget` ([`markers_view`]), que a tabela implementa. Ficam o `markersView.ts` (o `MarkersView` como `ViewPane`,
//!   com a árvore `MarkersTree`, a fonte de dados e as ações da barra) e os 3 casos de `test/browser/markersView.test.ts`.

pub mod markers_common;
pub mod markers_filter_options;
pub mod markers_model;
pub mod markers_table;
pub mod markers_tree_viewer;
pub mod markers_view;
pub mod markers_view_actions;
pub mod messages;
