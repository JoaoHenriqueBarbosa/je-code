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
pub mod electric_character;
pub mod encoded_token_attributes;
pub mod eol_counter;
pub mod fixed_array;
pub mod guides_text_model_part;
pub mod indent_rules;
pub mod indentation;
pub mod indentation_guesser;
pub mod interval_tree;
pub mod language_associations;
pub mod language_brackets_configuration;
pub mod language_configuration;
pub mod language_configuration_registry;
pub mod language_service;
pub mod compressed_virtualized_scroll_layout;
pub mod compressed_virtualized_scroll_view;
pub mod multi_diff_editor_options;
pub mod monarch;
pub mod standalone;
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
pub mod text_model_defaults;
pub mod text_model_bracket_pairs;
pub mod text_model_edit_source;
pub mod text_model_events;
pub mod text_model_guides;
pub mod text_model_search;
pub mod text_model_tokens;
pub mod text_model_text;
pub mod position_offset_transformer_with_text_model;
pub mod text_to_html_tokenizer;
pub mod tokenization_registry;
pub mod tokenization_text_model_part;
pub mod tokenizer_syntax_token_backend;
pub mod undo_redo;
pub mod undo_redo_service;
pub mod word_character_classifier;
pub mod word_helper;

pub mod position_to_offset;

pub mod lines_layout;
pub mod view_model_types;
pub mod line_heights;
pub mod find_section_headers;

pub mod default_document_colors_computer;

pub mod overview_zone_manager;
pub mod semantic_tokens_dto;
pub mod language_selector;
pub mod token_theme;

pub mod inline_decorations;
pub mod line_decorations;
pub mod line_part;
pub mod edits;
pub mod find;

pub mod folding;

pub mod annotations;

pub mod editor_common;
pub mod sort_lines_command;

pub mod snippet;
pub mod copy_lines_command;
pub mod move_caret_command;

pub mod config;
pub mod core_2d;
pub mod inline_completions;

pub mod unicode_text_model_highlighter;

pub mod edit_context;

pub mod view_layer;

pub mod diff;
pub mod diff_editor_view_model;
pub mod link_computer;

pub mod model_line_projection_data;
pub mod monospace_line_breaks_computer;
pub mod column_range;
pub mod text;

pub mod cursor_common;
pub mod input_mode;
pub mod token_store;

pub mod coordinates_converter;

pub mod cursor_word_operations;
pub mod cursor_context;
pub mod one_cursor;
pub mod cursor_collection;
pub mod replace_command;
pub mod cursor_delete_operations;
pub mod cursor_move_operations;
pub mod cursor_column_selection;

pub mod shift_command;
pub mod surround_selection_command;
pub mod column_selection_paste_command;
pub mod indentation_line_processor;
pub mod auto_indent;
pub mod enter_action;

pub mod cursor_type_edit_operations;
pub mod cursor_type_operations;
pub mod trim_trailing_whitespace_command;

pub mod view_line_renderer;
pub mod model_line_projection;

pub mod view_events;
pub mod view_model_event_dispatcher;
pub mod cursor_move_commands;

pub mod comment;
pub mod cursor_events;
pub mod cursor;
pub mod view_model_lines;

pub mod view_layout;
pub mod view_model_decoration;
pub mod view_model_decorations;
pub mod glyph_lanes_model;
pub mod view_lines_viewport_data;
pub mod rgba;
pub mod editor_theme;
pub mod screen_reader_simple_model;
pub mod minimap_tokens_color_tracker;
pub mod view_context;
pub mod rendering_context;
pub mod dynamic_view_overlay;
pub mod word_wrap_indicator;
pub mod whitespace;
pub mod view_model_impl_support;
pub mod gpu;

pub mod services_model;
pub mod text_resource_configuration_service;
pub mod opener_service;
pub mod minimap_char_sheet;
pub mod minimap_char_renderer;
pub mod minimap_pre_baked;
pub mod minimap_char_renderer_factory;
pub mod view_model_impl;

pub mod view_model_impl_api;
pub mod editor_browser;
pub mod hover;

pub mod view_parts;

pub mod code_editor_widget;
pub mod embedded_code_editor_widget;

pub mod trigger_inline_edit_commands_registry;
pub mod editor_extensions;
pub mod core_commands;

pub mod word_operations;
pub mod indent_utils;
pub mod move_lines_command;

pub mod lines_operations;
pub mod view_user_input_events;
pub mod view_controller;
pub mod editor_decorations_collection;

pub mod word_part_operations;

pub mod bracket_matching;
pub mod indentation_contrib;
pub mod multicursor;
pub mod multicursor_find_state;

pub mod suggest;

pub mod services;

pub mod model_service;
pub mod model_undo_redo_participant;
pub mod language_providers;
pub mod language_feature_registry;
pub mod language_features_service;
pub mod smart_select;
pub mod language_feature_debounce;
pub mod outline_model;
pub mod line_selection;
pub mod cursor_undo;
pub mod highlight_decorations;
pub mod textual_highlight_provider;
pub mod word_highlighter;
pub mod code_action;
pub mod bulk_edit_service;
pub mod editor_state;

pub mod editor_web_worker;
pub mod inplace_replace_support;
pub mod in_place_replace;
pub mod in_place_replace_command;
pub mod editor_worker;

pub mod parameter_hints;

pub mod stable_editor_scroll;

pub mod tree_views_dnd;

pub mod get_icon_classes;

pub mod codelens;
pub mod color;
pub mod color_detector;
pub mod format;
pub mod formatting_edit;
pub mod editor_context_keys;
pub mod keybinding_cancellation;
pub mod get_links;
pub mod inlay_hints;
pub mod goto_symbol;
pub mod inlay_hints_locations;
pub mod peek_view;
pub mod click_link_gesture;
pub mod rename;
pub mod semantic_tokens;
pub mod semantic_tokens_provider_styling;
pub mod semantic_tokens_styling;
pub mod semantic_tokens_styling_service;

pub mod observable_code_editor;

pub mod diff_editor_options;
pub mod diff_editor_utils;
pub mod diff_provider_factory_service;
pub mod multi_diff_editor;

pub mod diff_editor;

pub mod sticky_scroll;
