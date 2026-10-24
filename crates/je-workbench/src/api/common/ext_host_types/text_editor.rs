//! Os tipos da API `vscode` do editor de texto que não são classes do `extHostTypes.ts`, mas interfaces de
//! `vscode.d.ts` que o `extHostTextEditor.ts` e os conversores usam: as opções de renderização de decoração
//! (`DecorationRenderOptions` e companhia, `DecorationOptions`) e a informação de diferença de um
//! `TextEditor` (`TextEditorDiffInformation`, de `vscode.proposed.textEditorDiffInformation.d.ts`).
//!
//! Divergências:
//! - As interfaces opcionais (`foo?: T`) são structs com `Option<T>`; `Default` é o objeto vazio `{}`.
//! - `string | ThemeColor` é [`ColorValue`] e `string | Uri` é [`PathOrUri`].
//! - `DecorationRenderOptions extends ThemableDecorationRenderOptions` (e `DecorationInstanceRenderOptions extends
//!   ThemableDecorationInstanceRenderOptions`) são achatados por um campo `base`, como o `IDecorationRenderOptions`
//!   do editor.
//! - `OverviewRulerLane` é o do editor ([`OverviewRulerLane`]), que tem os valores da enumeração da API
//!   (`Left` 1, `Center` 2, `Right` 4, `Full` 7).
//! - `hoverMessage?: MarkdownString | MarkedString | Array<MarkdownString | MarkedString>` é [`HoverMessage`]
//!   (a `MarkdownString` é uma das variantes de [`MarkedString`]); `Range[] | DecorationOptions[]` é
//!   [`DecorationRanges`].

use je_base::themables::ThemeColor;
use je_base::uri::URI;
use je_editor::model::OverviewRulerLane;

use super::{DecorationRangeBehavior, MarkedString, PathOrUri, Range, TextEditorChangeKind};

/// `string | ThemeColor`.
#[derive(Clone, Debug, PartialEq)]
pub enum ColorValue {
    Text(String),
    Theme(ThemeColor),
}

/// `ThemableDecorationAttachmentRenderOptions`.
#[derive(Clone, Default)]
pub struct ThemableDecorationAttachmentRenderOptions {
    pub content_text: Option<String>,
    pub content_icon_path: Option<PathOrUri>,
    pub border: Option<String>,
    pub border_color: Option<ColorValue>,
    pub font_style: Option<String>,
    pub font_weight: Option<String>,
    pub text_decoration: Option<String>,
    pub color: Option<ColorValue>,
    pub background_color: Option<ColorValue>,
    pub margin: Option<String>,
    pub width: Option<String>,
    pub height: Option<String>,
}

/// `ThemableDecorationRenderOptions`.
#[derive(Clone, Default)]
pub struct ThemableDecorationRenderOptions {
    pub background_color: Option<ColorValue>,
    pub outline: Option<String>,
    pub outline_color: Option<ColorValue>,
    pub outline_style: Option<String>,
    pub outline_width: Option<String>,
    pub border: Option<String>,
    pub border_color: Option<ColorValue>,
    pub border_radius: Option<String>,
    pub border_spacing: Option<String>,
    pub border_style: Option<String>,
    pub border_width: Option<String>,
    pub font_style: Option<String>,
    pub font_weight: Option<String>,
    pub text_decoration: Option<String>,
    pub cursor: Option<String>,
    pub color: Option<ColorValue>,
    pub opacity: Option<String>,
    pub letter_spacing: Option<String>,
    pub gutter_icon_path: Option<PathOrUri>,
    pub gutter_icon_size: Option<String>,
    pub overview_ruler_color: Option<ColorValue>,
    pub before: Option<ThemableDecorationAttachmentRenderOptions>,
    pub after: Option<ThemableDecorationAttachmentRenderOptions>,
}

/// `DecorationRenderOptions`.
#[derive(Clone, Default)]
pub struct DecorationRenderOptions {
    pub base: ThemableDecorationRenderOptions,
    pub is_whole_line: Option<bool>,
    pub range_behavior: Option<DecorationRangeBehavior>,
    pub overview_ruler_lane: Option<OverviewRulerLane>,
    pub light: Option<ThemableDecorationRenderOptions>,
    pub dark: Option<ThemableDecorationRenderOptions>,
}

/// `ThemableDecorationInstanceRenderOptions`.
#[derive(Clone, Default)]
pub struct ThemableDecorationInstanceRenderOptions {
    pub before: Option<ThemableDecorationAttachmentRenderOptions>,
    pub after: Option<ThemableDecorationAttachmentRenderOptions>,
}

/// `DecorationInstanceRenderOptions`.
#[derive(Clone, Default)]
pub struct DecorationInstanceRenderOptions {
    pub base: ThemableDecorationInstanceRenderOptions,
    pub light: Option<ThemableDecorationInstanceRenderOptions>,
    pub dark: Option<ThemableDecorationInstanceRenderOptions>,
}

/// `MarkdownString | MarkedString | Array<MarkdownString | MarkedString>`.
#[derive(Clone, Debug)]
pub enum HoverMessage {
    One(MarkedString),
    Many(Vec<MarkedString>),
}

/// `DecorationOptions`.
#[derive(Clone)]
pub struct DecorationOptions {
    pub range: Range,
    pub hover_message: Option<HoverMessage>,
    pub render_options: Option<DecorationInstanceRenderOptions>,
}

/// `Range[] | DecorationOptions[]`: o segundo argumento de `TextEditor#setDecorations`.
#[derive(Clone)]
pub enum DecorationRanges {
    Ranges(Vec<Range>),
    Options(Vec<DecorationOptions>),
}

impl DecorationRanges {
    /// `ranges.length`.
    pub fn len(&self) -> usize {
        match self {
            DecorationRanges::Ranges(ranges) => ranges.len(),
            DecorationRanges::Options(options) => options.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// `TextEditorLineRange`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextEditorLineRange {
    pub start_line_number: f64,
    pub end_line_number_exclusive: f64,
}

/// `TextEditorChange`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextEditorChange {
    pub original: TextEditorLineRange,
    pub modified: TextEditorLineRange,
    pub kind: TextEditorChangeKind,
}

/// `TextEditorDiffInformation`.
#[derive(Clone, Debug, PartialEq)]
pub struct TextEditorDiffInformation {
    pub document_version: f64,
    pub original: Option<URI>,
    pub modified: URI,
    pub changes: Vec<TextEditorChange>,
    pub is_stale: bool,
}
