//! Porte de `src/vs/base/common/layout.ts`: posiciona uma vista (menu, hover, widget) ao lado de uma
//! âncora dentro de uma viewport, nas duas dimensões.

use crate::range::{IRange, intersects};

/// `IAnchor`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IAnchor {
    pub x: f64,
    pub y: f64,
    pub width: Option<f64>,
    pub height: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorAlignment {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorPosition {
    Below,
    Above,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorAxisAlignment {
    Vertical,
    Horizontal,
}

/// `IRect`: `IPosition` e `ISize` juntos.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IRect {
    pub top: f64,
    pub left: f64,
    pub width: f64,
    pub height: f64,
}

/// `ISize`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ISize {
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutAnchorPosition {
    Before,
    After,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LayoutAnchorMode {
    #[default]
    Avoid,
    Align,
}

/// `ILayoutAnchor`. `mode` ausente é `Avoid`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ILayoutAnchor {
    pub offset: f64,
    pub size: f64,
    pub mode: Option<LayoutAnchorMode>,
    pub position: LayoutAnchorPosition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutResult {
    Ok,
    Flipped,
    Overlap,
}

/// `ILayoutResult`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ILayoutResult {
    pub position: f64,
    pub result: LayoutResult,
}

/// `layout`: posiciona uma vista de uma dimensão ao lado de uma âncora na viewport e devolve o
/// deslocamento da vista dentro dela.
pub fn layout(viewport_size: f64, view_size: f64, anchor: ILayoutAnchor) -> ILayoutResult {
    let align = anchor.mode == Some(LayoutAnchorMode::Align);
    let layout_after_anchor_boundary = if align { anchor.offset } else { anchor.offset + anchor.size };
    let layout_before_anchor_boundary = if align { anchor.offset + anchor.size } else { anchor.offset };

    if anchor.position == LayoutAnchorPosition::Before {
        if view_size <= viewport_size - layout_after_anchor_boundary {
            // Caso feliz: depois da âncora.
            return ILayoutResult { position: layout_after_anchor_boundary, result: LayoutResult::Ok };
        }

        if view_size <= layout_before_anchor_boundary {
            // Caso aceitável: antes da âncora.
            return ILayoutResult { position: layout_before_anchor_boundary - view_size, result: LayoutResult::Flipped };
        }

        // Caso triste: por cima da âncora.
        ILayoutResult { position: (viewport_size - view_size).max(0.0), result: LayoutResult::Overlap }
    } else {
        if view_size <= layout_before_anchor_boundary {
            // Caso feliz: antes da âncora.
            return ILayoutResult { position: layout_before_anchor_boundary - view_size, result: LayoutResult::Ok };
        }

        if view_size <= viewport_size - layout_after_anchor_boundary && layout_before_anchor_boundary < view_size / 2.0 {
            // Caso aceitável: depois da âncora.
            return ILayoutResult { position: layout_after_anchor_boundary, result: LayoutResult::Flipped };
        }

        // Caso triste: por cima da âncora.
        ILayoutResult { position: 0.0, result: LayoutResult::Overlap }
    }
}

/// `ILayout2DOptions`. Cada campo ausente usa o padrão do original.
#[derive(Clone, Copy, Debug, Default)]
pub struct ILayout2DOptions {
    /// Padrão: esquerda.
    pub anchor_alignment: Option<AnchorAlignment>,
    /// Padrão: abaixo.
    pub anchor_position: Option<AnchorPosition>,
    /// Padrão: vertical.
    pub anchor_axis_alignment: Option<AnchorAxisAlignment>,
}

/// `ILayout2DResult`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ILayout2DResult {
    pub top: f64,
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
    pub anchor_alignment: AnchorAlignment,
    pub anchor_position: AnchorPosition,
}

fn flip_alignment(alignment: AnchorAlignment) -> AnchorAlignment {
    if alignment == AnchorAlignment::Left { AnchorAlignment::Right } else { AnchorAlignment::Left }
}

fn flip_position(position: AnchorPosition) -> AnchorPosition {
    if position == AnchorPosition::Below { AnchorPosition::Above } else { AnchorPosition::Below }
}

fn range_of(offset: f64, size: f64) -> IRange {
    IRange { start: offset, end: offset + size }
}

/// `layout2d`.
pub fn layout2d(viewport: IRect, view: ISize, anchor: IRect, options: Option<ILayout2DOptions>) -> ILayout2DResult {
    let options = options.unwrap_or_default();
    let mut anchor_alignment = options.anchor_alignment.unwrap_or(AnchorAlignment::Left);
    let mut anchor_position = options.anchor_position.unwrap_or(AnchorPosition::Below);
    let anchor_axis_alignment = options.anchor_axis_alignment.unwrap_or(AnchorAxisAlignment::Vertical);

    let before_or_after = |before: bool| if before { LayoutAnchorPosition::Before } else { LayoutAnchorPosition::After };
    let (top, left);

    if anchor_axis_alignment == AnchorAxisAlignment::Vertical {
        let vertical_anchor = ILayoutAnchor {
            offset: anchor.top - viewport.top,
            size: anchor.height,
            mode: None,
            position: before_or_after(anchor_position == AnchorPosition::Below),
        };
        let mut horizontal_anchor = ILayoutAnchor {
            offset: anchor.left,
            size: anchor.width,
            mode: Some(LayoutAnchorMode::Align),
            position: before_or_after(anchor_alignment == AnchorAlignment::Left),
        };

        let vertical_layout_result = layout(viewport.height, view.height, vertical_anchor);
        top = vertical_layout_result.position + viewport.top;

        if vertical_layout_result.result == LayoutResult::Flipped {
            anchor_position = flip_position(anchor_position);
        }

        // Se a vista cruza a âncora na vertical, é preciso evitá-la.
        if intersects(range_of(top, view.height), range_of(vertical_anchor.offset, vertical_anchor.size)) {
            horizontal_anchor.mode = Some(LayoutAnchorMode::Avoid);
        }

        let horizontal_layout_result = layout(viewport.width, view.width, horizontal_anchor);
        left = horizontal_layout_result.position;

        if horizontal_layout_result.result == LayoutResult::Flipped {
            anchor_alignment = flip_alignment(anchor_alignment);
        }
    } else {
        let horizontal_anchor = ILayoutAnchor {
            offset: anchor.left,
            size: anchor.width,
            mode: None,
            position: before_or_after(anchor_alignment == AnchorAlignment::Left),
        };
        let mut vertical_anchor = ILayoutAnchor {
            offset: anchor.top,
            size: anchor.height,
            mode: Some(LayoutAnchorMode::Align),
            position: before_or_after(anchor_position == AnchorPosition::Below),
        };

        let horizontal_layout_result = layout(viewport.width, view.width, horizontal_anchor);
        left = horizontal_layout_result.position;

        if horizontal_layout_result.result == LayoutResult::Flipped {
            anchor_alignment = flip_alignment(anchor_alignment);
        }

        // Se a vista cruza a âncora na horizontal, é preciso evitá-la.
        if intersects(range_of(left, view.width), range_of(horizontal_anchor.offset, horizontal_anchor.size)) {
            vertical_anchor.mode = Some(LayoutAnchorMode::Avoid);
        }

        let vertical_layout_result = layout(viewport.height, view.height, vertical_anchor);
        top = vertical_layout_result.position + viewport.top;

        if vertical_layout_result.result == LayoutResult::Flipped {
            anchor_position = flip_position(anchor_position);
        }
    }

    let right = viewport.width - (left + view.width);
    let bottom = viewport.height - (top + view.height);

    ILayout2DResult { top, left, bottom, right, anchor_alignment, anchor_position }
}
