//! `extHostTypeConverters.ts`: as conversões do editor de texto: `TextEditorLineNumbersStyle`, as opções de
//! decoração (`ThemableDecorationAttachmentRenderOptions`, `ThemableDecorationRenderOptions`,
//! `DecorationRangeBehavior`, `DecorationRenderOptions`), `fromRangeOrRangeWithMessage` e `pathOrURIToURI`.
//!
//! Divergências:
//! - `typeof options === 'undefined' ? options : ...` das conversões de opções temáveis é o `Option` de quem
//!   chama; as conversões recebem o valor presente.
//! - O `isDecorationOptionsArr` (que olha o primeiro elemento) é o `match` sobre [`types::DecorationRanges`]: a
//!   variante diz o que o array contém, e um array vazio vale como os dois (a conversão dá `[]` nos dois casos).
//! - `rangeBehavior ? DecorationRangeBehavior.from(...) : undefined` trata `OpenOpen` (valor `0`) como ausente,
//!   como o original (o padrão do editor já é o mesmo comportamento).
//! - `contentIconPath ? pathOrURIToURI(...) : undefined` e `gutterIconPath ? ... : undefined` tratam o caminho
//!   de texto vazio como ausente.
//! - Em `fromRangeOrRangeWithMessage` o original passa `renderOptions` como está (`<any>`, "URI vs Uri"); aqui o
//!   `DecorationInstanceRenderOptions` tipado é convertido com as mesmas conversões de `before` e `after` (cor
//!   de tema para `DecorationColor`, caminho para `URI`).
//! - `TextEditorLineNumbersStyle.to` recebe o `RenderLineNumbersType::Custom` (que o original trata no `default`)
//!   como `On`.

use je_base::uri::URI;
use je_editor::config::editor_options::line_numbers::RenderLineNumbersType;
use je_editor::model::{DecorationColor, TrackedRangeStickiness};
use je_editor::services::decoration_render_options::{
    ContentDecorationRenderOptions, DecorationRenderOptions as EditorDecorationRenderOptions, ThemeDecorationRenderOptions,
};
use je_platform::resources::host_platform;

use super::geometry::Range;
use super::markup::MarkdownString;
use crate::api::common::ext_host_protocol::{
    DecorationInstanceRenderOptionsDto, DecorationOptionsDto, MarkdownStringOrArray, ThemeDecorationInstanceRenderOptionsDto,
};
use crate::api::common::ext_host_types as types;

/// `typeConverters.TextEditorLineNumbersStyle`.
pub struct TextEditorLineNumbersStyle;

impl TextEditorLineNumbersStyle {
    pub fn from(style: types::TextEditorLineNumbersStyle) -> RenderLineNumbersType {
        match style {
            types::TextEditorLineNumbersStyle::Off => RenderLineNumbersType::Off,
            types::TextEditorLineNumbersStyle::Relative => RenderLineNumbersType::Relative,
            types::TextEditorLineNumbersStyle::Interval => RenderLineNumbersType::Interval,
            types::TextEditorLineNumbersStyle::On => RenderLineNumbersType::On,
        }
    }

    pub fn to(style: RenderLineNumbersType) -> types::TextEditorLineNumbersStyle {
        match style {
            RenderLineNumbersType::Off => types::TextEditorLineNumbersStyle::Off,
            RenderLineNumbersType::Relative => types::TextEditorLineNumbersStyle::Relative,
            RenderLineNumbersType::Interval => types::TextEditorLineNumbersStyle::Interval,
            RenderLineNumbersType::On | RenderLineNumbersType::Custom => types::TextEditorLineNumbersStyle::On,
        }
    }
}

/// `pathOrURIToURI(value)`: um caminho de texto vira o `URI.file`.
pub fn path_or_uri_to_uri(value: &types::PathOrUri) -> URI {
    match value {
        types::PathOrUri::Path(path) => URI::file(path.as_str(), host_platform()),
        types::PathOrUri::Uri(uri) => uri.clone(),
    }
}

/// `path ? pathOrURIToURI(path) : undefined`.
fn icon_path(value: &Option<types::PathOrUri>) -> Option<URI> {
    value.as_ref().filter(|path| !matches!(path, types::PathOrUri::Path(text) if text.is_empty())).map(path_or_uri_to_uri)
}

/// `<string | types.ThemeColor>value`: o texto é uma cor CSS, a `ThemeColor` é uma cor do tema.
fn color(value: &Option<types::ColorValue>) -> Option<DecorationColor> {
    value.as_ref().map(|value| match value {
        types::ColorValue::Text(text) => DecorationColor::Color(text.clone()),
        types::ColorValue::Theme(theme_color) => DecorationColor::Theme(theme_color.id.clone()),
    })
}

/// `typeConverters.ThemableDecorationAttachmentRenderOptions`.
pub struct ThemableDecorationAttachmentRenderOptions;

impl ThemableDecorationAttachmentRenderOptions {
    pub fn from(options: &types::ThemableDecorationAttachmentRenderOptions) -> ContentDecorationRenderOptions {
        ContentDecorationRenderOptions {
            content_text: options.content_text.clone(),
            content_icon_path: icon_path(&options.content_icon_path),
            border: options.border.clone(),
            border_color: color(&options.border_color),
            font_style: options.font_style.clone(),
            font_weight: options.font_weight.clone(),
            text_decoration: options.text_decoration.clone(),
            color: color(&options.color),
            background_color: color(&options.background_color),
            margin: options.margin.clone(),
            width: options.width.clone(),
            height: options.height.clone(),
            ..ContentDecorationRenderOptions::default()
        }
    }
}

/// `typeConverters.ThemableDecorationRenderOptions`.
pub struct ThemableDecorationRenderOptions;

impl ThemableDecorationRenderOptions {
    pub fn from(options: &types::ThemableDecorationRenderOptions) -> ThemeDecorationRenderOptions {
        ThemeDecorationRenderOptions {
            background_color: color(&options.background_color),
            outline: options.outline.clone(),
            outline_color: color(&options.outline_color),
            outline_style: options.outline_style.clone(),
            outline_width: options.outline_width.clone(),
            border: options.border.clone(),
            border_color: color(&options.border_color),
            border_radius: options.border_radius.clone(),
            border_spacing: options.border_spacing.clone(),
            border_style: options.border_style.clone(),
            border_width: options.border_width.clone(),
            font_style: options.font_style.clone(),
            font_weight: options.font_weight.clone(),
            text_decoration: options.text_decoration.clone(),
            cursor: options.cursor.clone(),
            color: color(&options.color),
            opacity: options.opacity.clone(),
            letter_spacing: options.letter_spacing.clone(),
            gutter_icon_path: icon_path(&options.gutter_icon_path),
            gutter_icon_size: options.gutter_icon_size.clone(),
            overview_ruler_color: color(&options.overview_ruler_color),
            before: options.before.as_ref().map(ThemableDecorationAttachmentRenderOptions::from),
            after: options.after.as_ref().map(ThemableDecorationAttachmentRenderOptions::from),
            ..ThemeDecorationRenderOptions::default()
        }
    }
}

/// `typeConverters.DecorationRangeBehavior`.
pub struct DecorationRangeBehavior;

impl DecorationRangeBehavior {
    pub fn from(value: types::DecorationRangeBehavior) -> TrackedRangeStickiness {
        match value {
            types::DecorationRangeBehavior::OpenOpen => TrackedRangeStickiness::AlwaysGrowsWhenTypingAtEdges,
            types::DecorationRangeBehavior::ClosedClosed => TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges,
            types::DecorationRangeBehavior::OpenClosed => TrackedRangeStickiness::GrowsOnlyWhenTypingBefore,
            types::DecorationRangeBehavior::ClosedOpen => TrackedRangeStickiness::GrowsOnlyWhenTypingAfter,
        }
    }
}

/// `typeConverters.DecorationRenderOptions`.
pub struct DecorationRenderOptions;

impl DecorationRenderOptions {
    pub fn from(options: &types::DecorationRenderOptions) -> EditorDecorationRenderOptions {
        EditorDecorationRenderOptions {
            base: ThemableDecorationRenderOptions::from(&options.base),
            is_whole_line: options.is_whole_line,
            // `options.rangeBehavior ? ... : undefined`: o valor `0` (`OpenOpen`) é falso
            range_behavior: options.range_behavior.filter(|behavior| behavior.value() != 0).map(DecorationRangeBehavior::from),
            overview_ruler_lane: options.overview_ruler_lane,
            light: options.light.as_ref().map(ThemableDecorationRenderOptions::from),
            dark: options.dark.as_ref().map(ThemableDecorationRenderOptions::from),
        }
    }
}

fn themable_instance_render_options(options: &types::ThemableDecorationInstanceRenderOptions) -> ThemeDecorationInstanceRenderOptionsDto {
    ThemeDecorationInstanceRenderOptionsDto {
        before: options.before.as_ref().map(ThemableDecorationAttachmentRenderOptions::from),
        after: options.after.as_ref().map(ThemableDecorationAttachmentRenderOptions::from),
    }
}

fn decoration_instance_render_options(options: &types::DecorationInstanceRenderOptions) -> DecorationInstanceRenderOptionsDto {
    DecorationInstanceRenderOptionsDto {
        base: themable_instance_render_options(&options.base),
        light: options.light.as_ref().map(themable_instance_render_options),
        dark: options.dark.as_ref().map(themable_instance_render_options),
    }
}

/// `fromRangeOrRangeWithMessage(ranges)`.
pub fn from_range_or_range_with_message(ranges: &types::DecorationRanges) -> Vec<DecorationOptionsDto> {
    match ranges {
        types::DecorationRanges::Options(options) => options
            .iter()
            .map(|option| DecorationOptionsDto {
                range: Range::from(&option.range),
                hover_message: match &option.hover_message {
                    Some(types::HoverMessage::Many(messages)) => Some(MarkdownStringOrArray::Many(MarkdownString::from_many(messages))),
                    // `r.hoverMessage ? MarkdownString.from(...) : undefined`: o texto vazio é falso
                    Some(types::HoverMessage::One(types::MarkedString::Text(text))) if text.is_empty() => None,
                    Some(types::HoverMessage::One(message)) => Some(MarkdownStringOrArray::One(MarkdownString::from(message))),
                    None => None,
                },
                render_options: option.render_options.as_ref().map(decoration_instance_render_options),
            })
            .collect(),
        types::DecorationRanges::Ranges(ranges) => {
            ranges.iter().map(|range| DecorationOptionsDto { range: Range::from(range), hover_message: None, render_options: None }).collect()
        }
    }
}
