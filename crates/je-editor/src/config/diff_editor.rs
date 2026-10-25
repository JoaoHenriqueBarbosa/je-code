//! Porte de `src/vs/editor/common/config/diffEditor.ts` (`diffEditorDefaultOptions`) e do tipo `ValidDiffEditorBaseOptions` de
//! `editorOptions.ts` (`Readonly<Required<IDiffEditorBaseOptions>>`): as opções do editor de diff já validadas, com todos os
//! campos presentes.
//!
//! Divergências:
//! - Os literais de texto do TypeScript (`'off' | 'on' | 'inherit'` de `diffWordWrap` e os nomes de algoritmo de `diffAlgorithm`)
//!   são enums fechados; `diffAlgorithm` usa o [`DiffAlgorithmName`] de `editor_worker`.
//! - `renderSideBySideInlineBreakpoint` e as contagens de linha (`clampedInt` limitado por `MAX_SAFE_SMALL_INTEGER`, que cabe em
//!   `i32`) são `i32`; `maxComputationTime` e `maxFileSize` são `f64` (o `number` do original).
//! - O objeto `diffEditorDefaultOptions` é a constante [`DIFF_EDITOR_DEFAULT_OPTIONS`].

use je_base::observable::StrictEquals;

use crate::editor_worker::DiffAlgorithmName;

impl StrictEquals for DiffAlgorithmName {
    fn strict_equals(&self, other: &Self) -> bool {
        self == other
    }
}

/// `'off' | 'on' | 'inherit'` de `diffWordWrap`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffWordWrap {
    Off,
    On,
    Inherit,
}

impl DiffWordWrap {
    /// O texto da opção no original.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
            Self::Inherit => "inherit",
        }
    }

    /// O valor de um dos textos permitidos (`'off'`, `'on'`, `'inherit'`).
    pub fn from_text(text: &str) -> Option<Self> {
        match text {
            "off" => Some(Self::Off),
            "on" => Some(Self::On),
            "inherit" => Some(Self::Inherit),
            _ => None,
        }
    }
}

impl StrictEquals for DiffWordWrap {
    fn strict_equals(&self, other: &Self) -> bool {
        self == other
    }
}

/// O texto de `DiffAlgorithmName` no original.
pub const fn diff_algorithm_name_as_str(name: DiffAlgorithmName) -> &'static str {
    match name {
        DiffAlgorithmName::Legacy => "legacy",
        DiffAlgorithmName::Advanced => "advanced",
        DiffAlgorithmName::AdvancedExternal => "advanced-external",
        DiffAlgorithmName::AdvancedWasm => "advanced-wasm",
    }
}

/// O `DiffAlgorithmName` de um dos textos permitidos.
pub fn diff_algorithm_name_from_str(text: &str) -> Option<DiffAlgorithmName> {
    match text {
        "legacy" => Some(DiffAlgorithmName::Legacy),
        "advanced" => Some(DiffAlgorithmName::Advanced),
        "advanced-external" => Some(DiffAlgorithmName::AdvancedExternal),
        "advanced-wasm" => Some(DiffAlgorithmName::AdvancedWasm),
        _ => None,
    }
}

/// `experimental` de `IDiffEditorBaseOptions`, com os três campos presentes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValidExperimentalDiffEditorOptions {
    pub show_moves: bool,
    pub show_empty_decorations: bool,
    /// Only applies when `renderSideBySide` is set to false.
    pub use_true_inline_view: bool,
}

/// `hideUnchangedRegions` de `IDiffEditorBaseOptions`, com os quatro campos presentes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValidHideUnchangedRegionsOptions {
    pub enabled: bool,
    pub reveal_line_count: i32,
    pub minimum_line_count: i32,
    pub context_line_count: i32,
}

/// `ValidDiffEditorBaseOptions`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ValidDiffEditorBaseOptions {
    pub enable_split_view_resizing: bool,
    pub split_view_default_ratio: f64,
    pub render_side_by_side: bool,
    pub render_side_by_side_inline_breakpoint: i32,
    pub use_inline_view_when_space_is_limited: bool,
    pub compact_mode: bool,
    pub hide_original_line_numbers: bool,
    pub max_computation_time: f64,
    pub max_file_size: f64,
    pub ignore_trim_whitespace: bool,
    pub render_indicators: bool,
    pub render_margin_revert_icon: bool,
    pub render_gutter_menu: bool,
    pub original_editable: bool,
    pub diff_code_lens: bool,
    pub render_overview_ruler: bool,
    pub diff_word_wrap: DiffWordWrap,
    pub diff_algorithm: DiffAlgorithmName,
    pub accessibility_verbose: bool,
    pub experimental: ValidExperimentalDiffEditorOptions,
    pub is_in_embedded_editor: bool,
    pub only_show_accessible_diff_viewer: bool,
    pub hide_unchanged_regions: ValidHideUnchangedRegionsOptions,
}

/// `diffEditorDefaultOptions`.
pub const DIFF_EDITOR_DEFAULT_OPTIONS: ValidDiffEditorBaseOptions = ValidDiffEditorBaseOptions {
    enable_split_view_resizing: true,
    split_view_default_ratio: 0.5,
    render_side_by_side: true,
    render_margin_revert_icon: true,
    render_gutter_menu: true,
    max_computation_time: 5000.0,
    max_file_size: 50.0,
    ignore_trim_whitespace: true,
    render_indicators: true,
    original_editable: false,
    diff_code_lens: false,
    render_overview_ruler: true,
    diff_word_wrap: DiffWordWrap::Inherit,
    diff_algorithm: DiffAlgorithmName::Advanced,
    accessibility_verbose: false,
    experimental: ValidExperimentalDiffEditorOptions { show_moves: false, show_empty_decorations: true, use_true_inline_view: false },
    hide_unchanged_regions: ValidHideUnchangedRegionsOptions { enabled: false, context_line_count: 3, minimum_line_count: 3, reveal_line_count: 20 },
    is_in_embedded_editor: false,
    only_show_accessible_diff_viewer: false,
    render_side_by_side_inline_breakpoint: 900,
    use_inline_view_when_space_is_limited: true,
    compact_mode: false,
    hide_original_line_numbers: false,
};
