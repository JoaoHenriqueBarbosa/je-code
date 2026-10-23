//! O trecho de `src/vs/editor/common/model.ts` que a API do buffer de texto usa: preferências de fim de
//! linha, snapshot, operações de edição, resultado de `applyEdits` e dados de busca.
//!
//! Textos são UTF-16 ([`JsString`]); o `text: string | null` das operações é `Option<JsString>`.

use std::rc::Rc;

use je_base::JsString;

use crate::js_reg_exp::JsRegExp;
use crate::range::Range;
use crate::text_change::TextChange;
use crate::word_character_classifier::WordCharacterClassifier;

/// `EndOfLinePreference`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndOfLinePreference {
    /// O fim de linha do próprio buffer.
    TextDefined = 0,
    /// `\n`.
    Lf = 1,
    /// `\r\n`.
    CrLf = 2,
}

/// `DefaultEndOfLine`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultEndOfLine {
    Lf = 1,
    CrLf = 2,
}

/// O fim de linha de um buffer: `'\r\n' | '\n'` do original.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eol {
    Lf,
    CrLf,
}

impl Eol {
    /// O texto do fim de linha.
    pub fn as_units(self) -> &'static [u16] {
        match self {
            Eol::Lf => &[10],
            Eol::CrLf => &[13, 10],
        }
    }

    pub fn len(self) -> usize {
        self.as_units().len()
    }

    pub fn is_empty(self) -> bool {
        false
    }
}

/// `ISingleEditOperationIdentifier`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SingleEditOperationIdentifier {
    pub major: i32,
    pub minor: i32,
}

/// `FindMatch`: o intervalo e, se pedido, os grupos (`None` para o grupo que não participou).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindMatch {
    pub range: Range,
    pub matches: Option<Vec<Option<JsString>>>,
}

/// `ITextSnapshot`: um iterador de pedaços (em torno de 64 KB) que devolve `None` ao terminar.
pub trait TextSnapshot {
    fn read(&mut self) -> Option<JsString>;
}

/// `ValidAnnotatedEditOperation`.
#[derive(Clone, Debug)]
pub struct ValidAnnotatedEditOperation {
    pub identifier: Option<SingleEditOperationIdentifier>,
    pub range: Range,
    pub text: Option<JsString>,
    pub force_move_markers: bool,
    pub is_auto_whitespace_edit: bool,
    pub is_tracked: bool,
}

/// `IValidEditOperation`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidEditOperation {
    pub identifier: Option<SingleEditOperationIdentifier>,
    /// O intervalo a trocar; vazio emula uma inserção.
    pub range: Range,
    /// O texto novo; vazio emula uma remoção.
    pub text: JsString,
    pub text_change: TextChange,
}

/// `IInternalModelContentChange`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InternalModelContentChange {
    pub range: Range,
    pub range_offset: usize,
    pub range_length: usize,
    pub text: JsString,
    pub force_move_markers: bool,
}

/// `ApplyEditsResult`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplyEditsResult {
    pub reverse_edits: Option<Vec<ValidEditOperation>>,
    pub changes: Vec<InternalModelContentChange>,
    pub trim_auto_whitespace_line_numbers: Option<Vec<i32>>,
}

/// `SearchData`.
#[derive(Debug)]
pub struct SearchData {
    /// A regex a procurar.
    pub regex: JsRegExp,
    /// O classificador de separadores de palavra, para buscar por palavra inteira.
    pub word_separators: Option<Rc<WordCharacterClassifier>>,
    /// O texto simples a procurar, quando dá para evitar a regex.
    pub simple_search: Option<JsString>,
}

impl SearchData {
    pub fn new(regex: JsRegExp, word_separators: Option<Rc<WordCharacterClassifier>>, simple_search: Option<JsString>) -> Self {
        Self {
            regex,
            word_separators,
            simple_search,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------
// Opções, decorações e eventos da camada do `TextModel` (o resto de `model.ts`).
// ---------------------------------------------------------------------------------------------------------

/// `EndOfLineSequence`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndOfLineSequence {
    Lf = 0,
    CrLf = 1,
}

impl EndOfLineSequence {
    /// O fim de linha do buffer que essa sequência pede.
    pub fn eol(self) -> Eol {
        match self {
            EndOfLineSequence::Lf => Eol::Lf,
            EndOfLineSequence::CrLf => Eol::CrLf,
        }
    }
}

impl From<Eol> for EndOfLineSequence {
    fn from(eol: Eol) -> Self {
        match eol {
            Eol::Lf => EndOfLineSequence::Lf,
            Eol::CrLf => EndOfLineSequence::CrLf,
        }
    }
}

/// `TrackedRangeStickiness`: a ordem dos valores é a de `TRACKED_RANGE_OPTIONS` do `textModel.ts`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TrackedRangeStickiness {
    AlwaysGrowsWhenTypingAtEdges = 0,
    NeverGrowsWhenTypingAtEdges = 1,
    GrowsOnlyWhenTypingBefore = 2,
    GrowsOnlyWhenTypingAfter = 3,
}

impl TrackedRangeStickiness {
    pub fn from_u8(value: u8) -> Self {
        match value & 3 {
            0 => TrackedRangeStickiness::AlwaysGrowsWhenTypingAtEdges,
            1 => TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges,
            2 => TrackedRangeStickiness::GrowsOnlyWhenTypingBefore,
            _ => TrackedRangeStickiness::GrowsOnlyWhenTypingAfter,
        }
    }
}

/// `OverviewRulerLane`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverviewRulerLane {
    Left = 1,
    Center = 2,
    Right = 4,
    Full = 7,
}

/// `GlyphMarginLane`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlyphMarginLane {
    Left = 1,
    Center = 2,
    Right = 3,
}

/// `MinimapPosition`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MinimapPosition {
    Inline = 1,
    Gutter = 2,
}

/// `MinimapSectionHeaderStyle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MinimapSectionHeaderStyle {
    Normal = 1,
    Underlined = 2,
}

/// `TextDirection`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextDirection {
    Ltr = 0,
    Rtl = 1,
}

/// `InjectedTextCursorStops`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjectedTextCursorStops {
    Both,
    Right,
    Left,
    None,
}

/// `string | ThemeColor`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecorationColor {
    Color(String),
    /// `ThemeColor`: o id da cor do tema.
    Theme(String),
}

/// `IMarkdownString`, só o conteúdo.
// TODO(integração): trocar pelo `htmlContent.ts` do je-base quando for portado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkdownString {
    pub value: String,
}

/// `IModelDecorationGlyphMarginOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelDecorationGlyphMarginInit {
    pub position: GlyphMarginLane,
    pub persist_lane: Option<bool>,
}

/// `IModelDecorationOverviewRulerOptions`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelDecorationOverviewRulerInit {
    pub color: Option<DecorationColor>,
    pub dark_color: Option<DecorationColor>,
    pub position: Option<OverviewRulerLane>,
}

/// `IModelDecorationMinimapOptions`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelDecorationMinimapInit {
    pub color: Option<DecorationColor>,
    pub dark_color: Option<DecorationColor>,
    pub position: MinimapPosition,
    pub section_header_style: Option<MinimapSectionHeaderStyle>,
    pub section_header_text: Option<String>,
}

/// `InjectedTextOptions`.
///
/// Pendente: `tokens` (`TokenArray`, vem com o `lineTokens.ts`) e `attachedData` (`unknown`, sem
/// consumidor nesta camada).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InjectedTextOptions {
    pub content: String,
    pub inline_class_name: Option<String>,
    pub inline_class_name_affects_letter_spacing: bool,
    /// `widthInEm`: aceita qualquer número, a normalização de `ModelDecorationInjectedTextOptions` descarta
    /// o que não é finito e não negativo.
    pub width_in_em: Option<f64>,
    pub cursor_stops: Option<InjectedTextCursorStops>,
}

/// `IModelDecorationOptions`: a entrada de `deltaDecorations`, com todo campo opcional no valor neutro.
/// Use `..Default::default()` e preencha o que importa; `register`/`create_dynamic` normalizam para
/// [`crate::text_model::ModelDecorationOptions`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelDecorationOptionsInit {
    pub description: String,
    pub stickiness: Option<TrackedRangeStickiness>,
    pub class_name: Option<String>,
    pub should_fill_line_on_line_break: Option<bool>,
    pub block_class_name: Option<String>,
    pub block_is_after_end: Option<bool>,
    pub block_does_not_collapse: Option<bool>,
    pub block_padding: Option<[i32; 4]>,
    pub glyph_margin_hover_message: Option<Vec<MarkdownString>>,
    pub hover_message: Option<Vec<MarkdownString>>,
    pub line_number_hover_message: Option<Vec<MarkdownString>>,
    pub is_whole_line: bool,
    pub show_if_collapsed: bool,
    pub collapse_on_replace_edit: bool,
    pub z_index: i32,
    pub overview_ruler: Option<ModelDecorationOverviewRulerInit>,
    pub minimap: Option<ModelDecorationMinimapInit>,
    pub glyph_margin_class_name: Option<String>,
    pub glyph_margin: Option<ModelDecorationGlyphMarginInit>,
    pub line_height: Option<f64>,
    pub font_family: Option<String>,
    pub font_size: Option<String>,
    pub font_weight: Option<String>,
    pub font_style: Option<String>,
    pub lines_decorations_class_name: Option<String>,
    pub lines_decorations_tooltip: Option<String>,
    pub line_number_class_name: Option<String>,
    pub first_line_decoration_class_name: Option<String>,
    pub margin_class_name: Option<String>,
    pub inline_class_name: Option<String>,
    pub inline_class_name_affects_letter_spacing: bool,
    pub before_content_class_name: Option<String>,
    pub after_content_class_name: Option<String>,
    pub after: Option<InjectedTextOptions>,
    pub before: Option<InjectedTextOptions>,
    pub hide_in_comment_tokens: Option<bool>,
    pub hide_in_string_tokens: Option<bool>,
    pub affects_font: Option<bool>,
    pub text_direction: Option<TextDirection>,
}

/// `IModelDeltaDecoration`.
#[derive(Clone, Debug)]
pub struct ModelDeltaDecoration {
    pub range: Range,
    pub options: Rc<crate::text_model::ModelDecorationOptions>,
}

/// `IModelDecoration`.
#[derive(Clone, Debug)]
pub struct ModelDecoration {
    pub id: String,
    pub owner_id: i32,
    pub range: Range,
    pub options: Rc<crate::text_model::ModelDecorationOptions>,
}

/// `BracketPairColorizationOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BracketPairColorizationOptions {
    pub enabled: bool,
    pub independent_color_pool_per_bracket_type: bool,
}

/// `number | 'tabSize'` do `indentSize`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndentSize {
    TabSize,
    Size(i32),
}

/// `IModelOptionsChangedEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelOptionsChangedEvent {
    pub tab_size: bool,
    pub indent_size: bool,
    pub insert_spaces: bool,
    pub trim_auto_whitespace: bool,
}

/// `TextModelResolvedOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextModelResolvedOptions {
    pub tab_size: i32,
    pub indent_size: i32,
    indent_size_is_tab_size: bool,
    pub insert_spaces: bool,
    pub default_eol: DefaultEndOfLine,
    pub trim_auto_whitespace: bool,
    pub bracket_pair_colorization_options: BracketPairColorizationOptions,
}

impl TextModelResolvedOptions {
    pub fn new(
        tab_size: i32,
        indent_size: IndentSize,
        insert_spaces: bool,
        default_eol: DefaultEndOfLine,
        trim_auto_whitespace: bool,
        bracket_pair_colorization_options: BracketPairColorizationOptions,
    ) -> Self {
        let tab_size = tab_size.max(1);
        let (indent_size, indent_size_is_tab_size) = match indent_size {
            IndentSize::TabSize => (tab_size, true),
            IndentSize::Size(size) => (size.max(1), false),
        };
        Self {
            tab_size,
            indent_size,
            indent_size_is_tab_size,
            insert_spaces,
            default_eol,
            trim_auto_whitespace,
            bracket_pair_colorization_options,
        }
    }

    /// `originalIndentSize`.
    pub fn original_indent_size(&self) -> IndentSize {
        if self.indent_size_is_tab_size {
            IndentSize::TabSize
        } else {
            IndentSize::Size(self.indent_size)
        }
    }

    pub fn create_change_event(&self, new_opts: &TextModelResolvedOptions) -> ModelOptionsChangedEvent {
        ModelOptionsChangedEvent {
            tab_size: self.tab_size != new_opts.tab_size,
            indent_size: self.indent_size != new_opts.indent_size,
            insert_spaces: self.insert_spaces != new_opts.insert_spaces,
            trim_auto_whitespace: self.trim_auto_whitespace != new_opts.trim_auto_whitespace,
        }
    }
}

/// `ITextModelCreationOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextModelCreationOptions {
    pub tab_size: i32,
    pub indent_size: IndentSize,
    pub insert_spaces: bool,
    pub detect_indentation: bool,
    pub trim_auto_whitespace: bool,
    pub default_eol: DefaultEndOfLine,
    pub is_for_simple_widget: bool,
    pub large_file_optimizations: bool,
    pub bracket_pair_colorization_options: BracketPairColorizationOptions,
}

/// `IRelaxedTextModelCreationOptions` do `testTextModel.ts`: cada campo ausente cai no padrão do modelo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RelaxedTextModelCreationOptions {
    pub tab_size: Option<i32>,
    pub indent_size: Option<IndentSize>,
    pub insert_spaces: Option<bool>,
    pub detect_indentation: Option<bool>,
    pub trim_auto_whitespace: Option<bool>,
    pub default_eol: Option<DefaultEndOfLine>,
    pub is_for_simple_widget: Option<bool>,
    pub large_file_optimizations: Option<bool>,
    pub bracket_colorization_options: Option<BracketPairColorizationOptions>,
}

/// `ITextModelUpdateOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextModelUpdateOptions {
    pub tab_size: Option<i32>,
    pub indent_size: Option<IndentSize>,
    pub insert_spaces: Option<bool>,
    pub trim_auto_whitespace: Option<bool>,
    pub bracket_colorization_options: Option<BracketPairColorizationOptions>,
}

/// `PositionAffinity`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PositionAffinity {
    Left = 0,
    Right = 1,
    None = 2,
    LeftOfInjectedText = 3,
    RightOfInjectedText = 4,
}

/// `ICursorStateComputer`: recebe as operações inversas e devolve as seleções resultantes.
pub type CursorStateComputer<'a> = &'a mut dyn FnMut(&[ValidEditOperation]) -> Option<Vec<crate::selection::Selection>>;
