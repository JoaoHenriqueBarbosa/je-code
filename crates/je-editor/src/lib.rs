//! Porte de `src/vs/editor/common` do VS Code: o núcleo do editor.
//!
//! Esta primeira fatia traz `core` (posição, intervalo, seleção, contagem de fins de linha, classificadores
//! de caractere, mudança de texto) e o buffer de texto do modelo (`pieceTreeTextBuffer`: a árvore rubro-negra,
//! o piece tree, o buffer e o construtor). Texto segue a semântica do JavaScript, como no `je-base`:
//! comprimentos, colunas e deslocamentos contam unidades UTF-16.

pub mod character_classifier;
pub mod cursor_columns;
pub mod edit_operation;
pub mod edit_stack;
pub mod eol_counter;
pub mod indentation;
pub mod indentation_guesser;
pub mod interval_tree;
pub mod js_reg_exp;
pub mod mirror_text_model;
pub mod model;
pub mod model_utils;
pub mod piece_tree_base;
pub mod piece_tree_text_buffer;
pub mod piece_tree_text_buffer_builder;
pub mod position;
pub mod prefix_sum_computer;
pub mod range;
pub mod rb_tree_base;
pub mod selection;
pub mod string_builder;
pub mod text_change;
pub mod text_model;
pub mod text_model_edit_source;
pub mod text_model_events;
pub mod text_model_search;
pub mod undo_redo;
pub mod undo_redo_service;
pub mod word_character_classifier;
pub mod word_helper;
