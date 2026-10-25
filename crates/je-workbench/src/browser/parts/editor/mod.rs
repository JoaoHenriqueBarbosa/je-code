//! Porte de `src/vs/workbench/browser/parts/editor`: o que as partes de editor têm de modelo e lógica, sem os widgets
//! (migalhas, o editor modal, o `EditorPart` e o grupo de editor lógicos).

pub mod breadcrumbs;
pub mod breadcrumbs_control;
pub mod breadcrumbs_model;
pub mod breadcrumbs_picker;
pub mod editor;
pub mod editor_auto_save;
pub mod editor_group_view;
pub mod editor_header_control;
pub mod editor_pane;
pub mod editor_tabs_control;
pub mod multi_editor_tabs_control;
pub mod editor_type_picker;
pub mod editor_panes;
pub mod editor_part;
pub mod editors_observer;
pub mod editor_parts;
pub mod modal_editor_part;
pub mod text_editor;

pub mod multi_row_editor_tabs_control;
