//! Porte de `src/vs/workbench/contrib/markers`.
//!
//! Estado do painel de problemas (`browser/`):
//! - Fatia 1 (esta): `markersTreeViewer.ts` ([`markers_tree_viewer`]: provedor de acessibilidade, delegado de altura,
//!   renderers, filtro, `MarkerViewModel` e `MarkersViewModel`) e `markersViewActions.ts` ([`markers_view_actions`]:
//!   `MarkersFilters`, `QuickFixAction` e `QuickFixActionViewItem`). Entraram junto os módulos de que os dois dependem e
//!   que o porte não tinha: `messages.ts` ([`messages`]) e `common/markers.ts` ([`markers_common`]: `MarkersViewMode`,
//!   `Markers` e `MarkersContextKeys`).
//! - Fatia 2 (a fazer): `markersView.ts` (o `MarkersView` como `ViewPane`, com a árvore `MarkersTree`, a fonte de dados e as
//!   ações da barra) e `markersTable.ts`, com os 3 casos de `test/browser/markersView.test.ts`.

pub mod markers_common;
pub mod markers_filter_options;
pub mod markers_model;
pub mod markers_tree_viewer;
pub mod markers_view_actions;
pub mod messages;
