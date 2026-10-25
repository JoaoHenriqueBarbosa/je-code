//! O contrato de `contextview.ts` e de `contextView.ts` (`IContextViewService`) que o `ActionWidgetService` consome: a âncora
//! (`HTMLElement | StandardMouseEvent | IAnchor`), a animação de fechamento, o `IDelegate` e o serviço.
//!
//! Divergências:
//! - **`onHide(data)`.** O `data: unknown` que o `ActionWidgetService` passa é o `didCancel`, então é `Option<bool>`.
//! - **`IOpenContextView`.** O `showContextView` devolve `{ close }` no original; o `ActionWidgetService` descarta o retorno,
//!   então o contrato não o devolve.
//! - **`onDOMEvent`, `useWindowContainerForShadowRoot`.** Os eventos de DOM do contêiner e a raiz de sombra não existem na
//!   árvore nativa; entram com os widgets que os usam.

use std::rc::Rc;

use je_base::layout::{AnchorAlignment, AnchorAxisAlignment, AnchorPosition, IAnchor, IRect};
use je_base::lifecycle::DisposableRef;
use je_instantiation::service_id;

use super::view_element::ViewElement;

pub const CONTEXT_VIEW_MENU_MOTION_CLASS: &str = "context-view-menu-motion";
pub const CONTEXT_VIEW_MENU_MOTION_CLOSING_CLASS: &str = "context-view-menu-motion-closing";
pub const CONTEXT_VIEW_MENU_MOTION_CLOSE_ANIMATION_DURATION: f64 = 150.0;
pub const CONTEXT_VIEW_CLOSE_ANIMATION_DURATION_VARIABLE: &str = "--vscode-context-view-close-animation-duration";

/// `CONTEXT_VIEW_MENU_MOTION_ANCESTOR_CLASSES`: `[['modern-ui', 'modern-ui-frosted-glass'], 'monaco-enable-motion']`. Cada
/// elemento é uma exigência; as classes dentro dele são alternativas (`some`), e uma `string` do original é a lista de uma
/// classe só.
pub fn context_view_menu_motion_ancestor_classes() -> Vec<Vec<String>> {
    vec![vec!["modern-ui".to_owned(), "modern-ui-frosted-glass".to_owned()], vec!["monaco-enable-motion".to_owned()]]
}

/// `IContextViewCloseAnimation`.
#[derive(Clone, Debug, PartialEq)]
pub struct IContextViewCloseAnimation {
    pub class_name: String,
    pub duration: f64,
    pub required_ancestor_classes: Vec<Vec<String>>,
}

/// `hasRequiredAncestorClasses(element, classNames)`: o elemento ou algum ancestral tem todas as exigências.
pub fn has_required_ancestor_classes(element: &Rc<ViewElement>, class_names: &[Vec<String>]) -> bool {
    if class_names.is_empty() {
        return true;
    }
    let mut candidate = Some(element.clone());
    while let Some(current) = candidate {
        if class_names.iter().all(|requirement| requirement.iter().any(|class_name| current.class_list_contains(class_name))) {
            return true;
        }
        candidate = current.parent_element();
    }
    false
}

/// `HTMLElement | StandardMouseEvent | IAnchor`.
#[derive(Clone)]
pub enum AnchorTarget {
    /// Posiciona a visão no elemento.
    Element(Rc<ViewElement>),
    /// `StandardMouseEvent`: `posx` e `posy`.
    Mouse { posx: f64, posy: f64 },
    /// Um retângulo avulso.
    Anchor(IAnchor),
}

/// `getAnchorRect(anchor)`.
pub fn get_anchor_rect(anchor: &AnchorTarget) -> IRect {
    match anchor {
        AnchorTarget::Element(element) => element.rect(),
        AnchorTarget::Anchor(anchor) => IRect {
            top: anchor.y,
            left: anchor.x,
            width: anchor.width.filter(|width| *width != 0.0).unwrap_or(1.0),
            height: anchor.height.filter(|height| *height != 0.0).unwrap_or(2.0),
        },
        // We are about to position the context view where the mouse cursor is. To prevent the view being exactly under the
        // mouse when showing and thus potentially triggering an action within, we treat the mouse location like a small sized
        // block element.
        AnchorTarget::Mouse { posx, posy } => IRect { top: *posy, left: *posx, width: 2.0, height: 2.0 },
    }
}

/// `IContextViewDelegate` (`IDelegate` do `contextview.ts`).
pub trait IContextViewDelegate {
    fn get_anchor(&self) -> AnchorTarget;
    /// `render(container)`: monta o conteúdo no contêiner e devolve o que desmonta.
    fn render(&self, container: &Rc<ViewElement>) -> Option<DisposableRef>;
    fn can_relayout(&self) -> bool {
        true
    }
    fn layout(&self) {}
    fn anchor_alignment(&self) -> Option<AnchorAlignment> {
        None
    }
    fn anchor_position(&self) -> Option<AnchorPosition> {
        None
    }
    fn anchor_axis_alignment(&self) -> Option<AnchorAxisAlignment> {
        None
    }
    fn focus(&self) {}
    /// `onHide(data)`.
    fn on_hide(&self, _did_cancel: Option<bool>) {}
    fn close_animation(&self) -> Option<IContextViewCloseAnimation> {
        None
    }
    /// `layer`: as visões de camada maior ficam por cima (`undefined` é `None`).
    fn layer(&self) -> Option<i32> {
        None
    }
}

/// `IContextViewService`.
pub trait IContextViewService {
    /// `showContextView(delegate, container, shadowRoot)`.
    fn show_context_view(&self, delegate: Rc<dyn IContextViewDelegate>, container: Option<Rc<ViewElement>>, shadow_root: bool);
    /// `hideContextView(data)`.
    fn hide_context_view(&self, data: Option<bool>);
    fn layout(&self);
    /// `getContextViewElement()`.
    fn get_context_view_element(&self) -> Rc<ViewElement>;
}

service_id!(pub I_CONTEXT_VIEW_SERVICE: dyn IContextViewService = "contextViewService");
