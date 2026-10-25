//! Widgets de `vs/base/browser/ui` do VS Code em cima do `je-ui`.
//!
//! Cada módulo é o porte de um arquivo do VS Code, com a lógica (tamanhos, proporções, árvore,
//! prioridades, snap) igual à do original e o que ali mexe em `HTMLElement` trocado por nós da árvore do
//! [`je_ui`]. As regras de CSS que o original deixa para a folha de estilo viram código do widget, ver a
//! documentação de cada módulo.

pub(crate) mod common;
pub mod context_view_api;
pub mod dom;
pub mod dnd;
pub mod drag_image;
pub mod hover_api;
pub mod text;
pub mod global_pointer_move_monitor;
pub mod sash;
pub mod scrollbar;
pub mod grid;
pub mod list;
pub mod gridview;
pub mod paneview;
pub mod splitview;
pub mod table;
pub mod tree;
pub mod text_field;
pub mod input_box;
pub mod action_view_items;
pub mod action_bar;
pub mod context_view;
pub mod hover_widget;
pub mod button;
pub mod pixel_spinner;
pub mod switch;
pub mod toggle;
pub mod radio;
pub mod icon_labels;
pub mod highlighted_label;
pub mod icon_label;
pub mod simple_icon_label;
pub mod count_badge;
pub mod progress_bar;
pub mod mouse_cursor;
pub mod keybinding_label;
pub mod breadcrumbs_widget;
pub mod centered_view_layout;
pub mod find_input_toggles;
pub mod find_input_base;
pub mod find_input;
pub mod replace_input;
pub mod severity_icon;
pub mod dialog;
pub mod select_box;
pub mod select_box_custom;
pub mod dropdown;
pub mod dropdown_action_view_item;
pub mod toolbar;
pub mod modifier_key_emitter;
pub mod menu;
pub mod menu_bar;
