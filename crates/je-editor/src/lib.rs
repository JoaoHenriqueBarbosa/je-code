//! Porte de `src/vs/editor/common` do VS Code: o núcleo do editor.
//!
//! Esta primeira fatia traz `core` (posição, intervalo, seleção, contagem de fins de linha, classificadores
//! de caractere, mudança de texto) e o buffer de texto do modelo (`pieceTreeTextBuffer`: a árvore rubro-negra,
//! o piece tree, o buffer e o construtor). Texto segue a semântica do JavaScript, como no `je-base`:
//! comprimentos, colunas e deslocamentos contam unidades UTF-16.
//!
//! Depois: o `TextModel`, a tokenização (`encoded_token_attributes`, `line_tokens`, `contiguous_*`,
//! `sparse_*`, `text_model_tokens`, `tokenizer_syntax_token_backend`, `tokenization_text_model_part`, o
//! `tokenization_registry`), o serviço de linguagem (`language_service`, `languages_registry`,
//! `language_associations`, `modes_registry`) e a configuração de linguagem (`language_configuration*`,
//! `character_pair`, `electric_character`, `on_enter`, `indent_rules`, `rich_edit_brackets`). A gramática
//! TextMate e a leitura das extensões ficam no crate `je-languages`.

pub mod attached_views;
pub mod bracket_pairs_text_model_part;
pub mod character_classifier;
pub mod character_pair;
pub mod contiguous_multiline_tokens;
pub mod contiguous_tokens_editing;
pub mod contiguous_tokens_store;
pub mod cursor_atomic_move_operations;
pub mod cursor_columns;
pub mod edit_operation;
pub mod edit_stack;
pub mod editor_auto_indent_strategy;
pub mod electric_character;
pub mod encoded_token_attributes;
pub mod eol_counter;
pub mod fixed_array;
pub mod guides_text_model_part;
pub mod indent_rules;
pub mod indentation;
pub mod indentation_guesser;
pub mod interval_tree;
pub mod js_reg_exp;
pub mod language_associations;
pub mod language_brackets_configuration;
pub mod language_configuration;
pub mod language_configuration_registry;
pub mod language_service;
pub mod languages;
pub mod languages_registry;
pub mod line_range;
pub mod line_tokens;
pub mod mirror_text_model;
pub mod model;
pub mod model_utils;
pub mod modes_registry;
pub mod null_tokenize;
pub mod offset_range;
pub mod on_enter;
pub mod piece_tree_base;
pub mod piece_tree_text_buffer;
pub mod piece_tree_text_buffer_builder;
pub mod position;
pub mod prefix_sum_computer;
pub mod range;
pub mod rb_tree_base;
pub mod rich_edit_brackets;
pub mod selection;
pub mod sparse_multiline_tokens;
pub mod sparse_tokens_store;
pub mod string_builder;
pub mod supports;
pub mod text_change;
pub mod text_model;
pub mod text_model_bracket_pairs;
pub mod text_model_edit_source;
pub mod text_model_events;
pub mod text_model_guides;
pub mod text_model_search;
pub mod text_model_tokens;
pub mod tokenization_registry;
pub mod tokenization_text_model_part;
pub mod tokenizer_syntax_token_backend;
pub mod undo_redo;
pub mod undo_redo_service;
pub mod word_character_classifier;
pub mod word_helper;

pub mod position_to_offset;

pub mod default_document_colors_computer;
