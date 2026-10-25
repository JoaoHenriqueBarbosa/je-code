//! Porte de `src/vs/base/browser/ui/dnd/dnd.ts` e `dnd.css`: a imagem que acompanha o ponteiro num arrasto
//! (`.monaco-drag-image`).
//!
//! Do DOM para o Rust:
//! - O elemento é criado no contêiner `.monaco-workbench` mais próximo (senão na raiz, o `document.body`) e
//!   registrado com `setDragImage(imagem, -10, -10)`. O navegador tira um instantâneo dele e o original o remove no
//!   `setTimeout(0)` seguinte; aqui o nó é a imagem que acompanha o ponteiro, e quem termina o arrasto o destrói
//!   ([`discard_drag_image`]).
//! - O CSS vira código: `padding: 1px 7px`, `border-radius: 10px`, `font-size: 12px`, `z-index: 1000`,
//!   `max-width: 120px` com `overflow: hidden` e `text-overflow: ellipsis`, as cores do tema
//!   (`list.activeSelectionBackground`, `list.activeSelectionForeground` e `list.focusOutline` com `outline-offset:
//!   -1px`, que é a borda de 1 px por dentro). O nó fica fora da janela (`position: absolute` sem posição pede
//!   ao navegador um lugar fora da vista) até quem pinta a janela desenhá-lo sob o ponteiro.

use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{Border, NodeId, PointerEvents, Role, Ui};

use crate::dnd::DataTransfer;
use crate::dom::create_el;
use crate::list::list_styles::LIST_FALLBACK_FOREGROUND;
use crate::text::{ellipsize, measure_text, natural_line_height, set_node_text, ui_font};

/// As cores da imagem de arrasto (as de `.monaco-drag-image`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DragImageStyle {
    /// `list.activeSelectionBackground`.
    pub background: Option<Rgba>,
    /// `list.activeSelectionForeground`.
    pub foreground: Option<Rgba>,
    /// `list.focusOutline`.
    pub outline: Option<Rgba>,
}

/// O `padding` horizontal.
const PADDING_X: f64 = 7.0;
/// O `padding` vertical.
const PADDING_Y: f64 = 1.0;
/// O `max-width`.
const MAX_WIDTH: f64 = 120.0;
/// O `font-size`.
const FONT_SIZE: f32 = 12.0;
/// O `border-radius`.
const RADIUS: f64 = 10.0;
/// O `z-index`.
const Z_INDEX: i32 = 1000;
/// Onde o nó espera até ser desenhado sob o ponteiro (fora da vista).
const PARKED: f64 = -10_000.0;

/// `applyDragImage(event, container, label, extraClasses)`.
pub fn apply_drag_image(ui: &Ui, data_transfer: &DataTransfer, container: NodeId, label: &str, extra_classes: &[String], style: &DragImageStyle) {
    let font = ui_font(FONT_SIZE, 400);
    let line_height = natural_line_height(&font);
    let text = ellipsize(label, &font, MAX_WIDTH - 2.0 * PADDING_X);
    let text_width = measure_text(&text, &font);
    let width = (text_width + 2.0 * PADDING_X).min(MAX_WIDTH);
    let height = line_height + 2.0 * PADDING_Y;

    let image = {
        let mut tree = ui.tree_mut();

        // getDragImageContainer
        let mut ancestor = Some(container);
        while let Some(node) = ancestor {
            if tree.has_class(node, "monaco-workbench") {
                break;
            }
            ancestor = tree.parent(node);
        }
        let drag_container = ancestor.unwrap_or_else(|| tree.root());

        let image = create_el(&mut tree, drag_container, Role::Presentation, "monaco-drag-image");
        for class in extra_classes {
            tree.add_class(image, class);
        }
        tree.set_bounds(image, Rect::new(PARKED, PARKED, PARKED + width, PARKED + height));
        {
            let image_style = tree.style_mut(image);
            image_style.background = style.background;
            image_style.border = style.outline.map(|color| Border { width: 1.0, color });
            image_style.radius = RADIUS;
            image_style.clip = true;
            image_style.z_index = Z_INDEX;
            image_style.pointer_events = PointerEvents::None;
        }
        tree.set_label(image, label);

        let text_node = create_el(&mut tree, image, Role::Generic, "");
        tree.set_bounds(text_node, Rect::new(PADDING_X, PADDING_Y, PADDING_X + text_width, PADDING_Y + line_height));
        set_node_text(&mut tree, text_node, &text, &font, style.foreground.unwrap_or(LIST_FALLBACK_FOREGROUND), line_height);
        image
    };

    data_transfer.set_drag_image(image, -10.0, -10.0);
}

/// Destrói a imagem de arrasto do `DataTransfer`, se há (o `dragImage.remove()` do original, que roda quando a
/// operação de arrastar termina).
pub fn discard_drag_image(ui: &Ui, data_transfer: &DataTransfer) {
    if let Some(image) = data_transfer.take_drag_image() {
        ui.remove(image.node);
    }
}
