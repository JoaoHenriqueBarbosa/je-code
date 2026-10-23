//! Porte de `src/vs/editor/common/core/misc/textModelDefaults.ts`: os padrões do modelo de texto que o
//! `TextModel.DEFAULT_CREATION_OPTIONS` e as opções do editor (`bracketPairColorization`, `tabSize`...)
//! compartilham.

use crate::model::BracketPairColorizationOptions;

/// `EDITOR_MODEL_DEFAULTS`.
pub struct EditorModelDefaults {
    pub tab_size: i32,
    pub indent_size: i32,
    pub insert_spaces: bool,
    pub detect_indentation: bool,
    pub trim_auto_whitespace: bool,
    pub large_file_optimizations: bool,
    pub bracket_pair_colorization_options: BracketPairColorizationOptions,
}

/// `EDITOR_MODEL_DEFAULTS`.
pub const EDITOR_MODEL_DEFAULTS: EditorModelDefaults = EditorModelDefaults {
    tab_size: 4,
    indent_size: 4,
    insert_spaces: true,
    detect_indentation: true,
    trim_auto_whitespace: true,
    large_file_optimizations: true,
    bracket_pair_colorization_options: BracketPairColorizationOptions { enabled: true, independent_color_pool_per_bracket_type: false },
};
