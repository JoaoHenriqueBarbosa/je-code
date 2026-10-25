//! Porte parcial de `src/vs/workbench/browser/parts/editor/breadcrumbsControl.ts`: o que do controle das migalhas
//! já pode ser fiel sobre o que existe no repositório (o `BreadcrumbsWidget` de `je-widgets`, a configuração e o serviço
//! de `breadcrumbs.rs`).
//!
//! Portado aqui:
//! - [`BreadcrumbsControlOptions`] (`IBreadcrumbsControlOptions`) e as constantes estáticas de `BreadcrumbsControl`
//!   (`HEIGHT`, `SCROLLBAR_SIZES`, `SCROLLBAR_VISIBILITY`) em [`BreadcrumbsControl`];
//! - os quatro `RawContextKey` (`CK_Breadcrumbs*`), como funções;
//! - os três `Payload_*`, que no original são objetos comparados por identidade: aqui são tipos unitários guardados no
//!   `Rc<dyn Any>` do evento do widget ([`payload_reveal`], [`payload_reveal_aside`], [`payload_pick`]), e
//!   `_getEditorGroup` é [`editor_group_target`];
//! - a geometria do `getAnchor` do picker ([`compute_picker_placement`]), com as mesmas contas e a mesma ordem.
//!
//! Ainda não portado, e por quê (nenhum destes módulos existe em `crates/`, e o controle depende de todos):
//! `breadcrumbsPicker.ts` (`BreadcrumbsFilePicker`, `BreadcrumbsOutlinePicker`, árvores do `WorkbenchAsyncDataTree`),
//! `ResourceLabels` (`labels.ts`, usado por `FileItem`), `editorTypePicker.ts`, `dnd.ts` (`fillEditorsDragData`,
//! `DraggedEditorIdentifier`), o `IContextMenuService` e o `IHoverService` do workbench, e o `IContextViewService` com
//! `source` e `didPick`/`sideBySide` nos dados do `hideContextView` (o contrato daqui só leva `Option<bool>`). Sem eles
//! `OutlineItem`, `FileItem`, `update`, `_onSelectEvent`, a fábrica e os comandos de foco e navegação não têm sobre o quê
//! assentar sem virar fachada, então ficam para depois do picker.

use std::any::Any;
use std::rc::Rc;

use je_base::nls::localize;
use je_base::scrollable::ScrollbarVisibility;
use je_platform::contextkey::contextkey::RawContextKey;

use je_widgets::breadcrumbs_widget::IBreadcrumbsWidgetStyles;

/// `IBreadcrumbsControlOptions`.
#[derive(Clone, Copy, Debug, Default)]
pub struct BreadcrumbsControlOptions {
    pub show_file_icons: bool,
    pub show_symbol_icons: bool,
    pub show_decoration_colors: bool,
    pub show_placeholder: bool,
    pub drag_editor: bool,
    pub widget_styles: Option<IBreadcrumbsWidgetStyles>,
    /// Mostra o seletor do tipo de editor à direita (só na barra de migalhas abaixo das abas).
    pub show_editor_type_picker: bool,
}

/// O destino de um `Payload_Reveal*`: `SIDE_GROUP` ou `ACTIVE_GROUP`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorGroupTarget {
    /// `SIDE_GROUP`.
    Side,
    /// `ACTIVE_GROUP`.
    Active,
}

/// `Payload_Reveal`.
struct PayloadReveal;
/// `Payload_RevealAside`.
struct PayloadRevealAside;
/// `Payload_Pick`.
struct PayloadPick;

/// `BreadcrumbsControl.Payload_Reveal`.
pub fn payload_reveal() -> Rc<dyn Any> {
    Rc::new(PayloadReveal)
}

/// `BreadcrumbsControl.Payload_RevealAside`.
pub fn payload_reveal_aside() -> Rc<dyn Any> {
    Rc::new(PayloadRevealAside)
}

/// `BreadcrumbsControl.Payload_Pick`.
pub fn payload_pick() -> Rc<dyn Any> {
    Rc::new(PayloadPick)
}

/// Se `payload` é o `Payload_Pick`.
pub fn is_payload_pick(payload: Option<&Rc<dyn Any>>) -> bool {
    payload.is_some_and(|payload| (**payload).is::<PayloadPick>())
}

/// `_getEditorGroup(data)`.
pub fn editor_group_target(payload: Option<&Rc<dyn Any>>) -> Option<EditorGroupTarget> {
    let payload = payload?;
    if (**payload).is::<PayloadRevealAside>() {
        Some(EditorGroupTarget::Side)
    } else if (**payload).is::<PayloadReveal>() {
        Some(EditorGroupTarget::Active)
    } else {
        None
    }
}

thread_local! {
    static CK_POSSIBLE: RawContextKey = RawContextKey::new("breadcrumbsPossible", false, localize("breadcrumbsPossible", "Whether the editor can show breadcrumbs", &[]));
    static CK_VISIBLE: RawContextKey = RawContextKey::new("breadcrumbsVisible", false, localize("breadcrumbsVisible", "Whether breadcrumbs are currently visible", &[]));
    static CK_ACTIVE: RawContextKey = RawContextKey::new("breadcrumbsActive", false, localize("breadcrumbsActive", "Whether breadcrumbs have focus", &[]));
    static CK_HAS_SYMBOLS: RawContextKey = RawContextKey::new("breadcrumbsHasSymbols", false, localize("breadcrumbsHasSymbols", "Whether breadcrumbs contain symbol items", &[]));
}

/// As estáticas de `BreadcrumbsControl`.
pub struct BreadcrumbsControl;

impl BreadcrumbsControl {
    pub const HEIGHT: f64 = 22.0;

    /// `SCROLLBAR_SIZES[sizing]`: `'default'` (3) ou `'large'` (8); outro texto não tem tamanho.
    pub fn scrollbar_size(sizing: &str) -> Option<f64> {
        match sizing {
            "default" => Some(3.0),
            "large" => Some(8.0),
            _ => None,
        }
    }

    /// `SCROLLBAR_VISIBILITY[visibility]`.
    pub fn scrollbar_visibility(visibility: &str) -> Option<ScrollbarVisibility> {
        match visibility {
            "auto" => Some(ScrollbarVisibility::Auto),
            "visible" => Some(ScrollbarVisibility::Visible),
            "hidden" => Some(ScrollbarVisibility::Hidden),
            _ => None,
        }
    }

    /// `CK_BreadcrumbsPossible`.
    pub fn ck_breadcrumbs_possible() -> RawContextKey {
        CK_POSSIBLE.with(Clone::clone)
    }

    /// `CK_BreadcrumbsVisible`.
    pub fn ck_breadcrumbs_visible() -> RawContextKey {
        CK_VISIBLE.with(Clone::clone)
    }

    /// `CK_BreadcrumbsActive`.
    pub fn ck_breadcrumbs_active() -> RawContextKey {
        CK_ACTIVE.with(Clone::clone)
    }

    /// `CK_BreadcrumbsHasSymbols`.
    pub fn ck_breadcrumbs_has_symbols() -> RawContextKey {
        CK_HAS_SYMBOLS.with(Clone::clone)
    }
}

/// O retângulo de um item na tela (`dom.getDomNodePagePosition`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemRect {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

/// O resultado do `getAnchor`: o ponto de âncora e os argumentos de `picker.show(...)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PickerPlacement {
    pub x: f64,
    pub y: f64,
    pub max_height: f64,
    pub width: f64,
    pub arrow_size: f64,
    pub arrow_offset: f64,
}

/// O corpo do `getAnchor` de `_onSelectEvent`. `mouse_x` é o `posx` do `StandardMouseEvent` quando o payload do
/// evento é um clique (senão `None`, e a seta fica a 30% da largura do item).
pub fn compute_picker_placement(window_width: f64, window_height: f64, item: ItemRect, mouse_x: Option<f64>) -> PickerPlacement {
    let max_inner_width = window_width - 8.0; // a little less the full widget
    let mut max_height = (window_height * 0.7).min(300.0);

    let width = max_inner_width.min(240.0_f64.max(max_inner_width / 4.17));
    let arrow_size = 8.0;

    let y = item.top + item.height + arrow_size;
    if y + max_height >= window_height {
        max_height = window_height - y - 30.0; // room for shadow and status bar
    }
    let mut x = item.left;
    if x + width >= max_inner_width {
        x = max_inner_width - width;
    }
    let arrow_offset = match mouse_x {
        Some(posx) => {
            let max_arrow_offset = width - 2.0 * arrow_size;
            let mut offset = posx - x;
            if offset > max_arrow_offset {
                x = (max_inner_width - width).min(x + offset - max_arrow_offset);
                offset = max_arrow_offset;
            }
            offset
        }
        None => (item.left + item.width * 0.3) - x,
    };
    PickerPlacement { x, y, max_height, width, arrow_size, arrow_offset: arrow_offset.max(0.0) }
}
