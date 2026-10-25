//! Porte de `src/vs/workbench/browser/actions/menuMotion.ts`: o movimento dos menus de contexto do workbench, que reaproveita
//! as constantes do `contextview.ts`.
//!
//! Divergência: `getWorkbenchMenuMotionContextMenuOptions(anchor)` devolve um `Pick<IContextMenuDelegate, ...>` literal; o
//! delegado nativo é um trait (com `get_actions` obrigatório), então o `Pick` é a struct [`WorkbenchMenuMotionContextMenuOptions`],
//! com os três membros como métodos. Quem monta o menu pode copiar o que ela devolve para o seu delegado.

use std::rc::Rc;

use je_platform::contextview::context_view::{
    AnchorTarget, CONTEXT_VIEW_MENU_MOTION_CLASS, CONTEXT_VIEW_MENU_MOTION_CLOSE_ANIMATION_DURATION, CONTEXT_VIEW_MENU_MOTION_CLOSING_CLASS, IContextViewCloseAnimation,
    context_view_menu_motion_ancestor_classes,
};
use je_platform::contextview::view_element::ViewElement;

/// `WORKBENCH_MENU_MOTION_CLASS`.
pub const WORKBENCH_MENU_MOTION_CLASS: &str = CONTEXT_VIEW_MENU_MOTION_CLASS;

/// `WORKBENCH_MENU_MOTION_CLOSING_CLASS`.
pub const WORKBENCH_MENU_MOTION_CLOSING_CLASS: &str = CONTEXT_VIEW_MENU_MOTION_CLOSING_CLASS;

/// `WORKBENCH_MENU_MOTION_CLOSE_ANIMATION_DURATION`.
pub const WORKBENCH_MENU_MOTION_CLOSE_ANIMATION_DURATION: f64 = CONTEXT_VIEW_MENU_MOTION_CLOSE_ANIMATION_DURATION;

/// `workbenchMenuCloseAnimation` (que é o `contextViewMenuCloseAnimation`).
pub fn workbench_menu_close_animation() -> IContextViewCloseAnimation {
    IContextViewCloseAnimation {
        class_name: WORKBENCH_MENU_MOTION_CLOSING_CLASS.to_owned(),
        duration: WORKBENCH_MENU_MOTION_CLOSE_ANIMATION_DURATION,
        required_ancestor_classes: context_view_menu_motion_ancestor_classes(),
    }
}

/// `Pick<IContextMenuDelegate, 'getAnchor' | 'getMenuClassName' | 'closeAnimation'>`.
pub struct WorkbenchMenuMotionContextMenuOptions {
    anchor: Rc<ViewElement>,
}

impl WorkbenchMenuMotionContextMenuOptions {
    /// `getAnchor`.
    pub fn get_anchor(&self) -> AnchorTarget {
        AnchorTarget::Element(self.anchor.clone())
    }

    /// `getMenuClassName`.
    pub fn get_menu_class_name(&self) -> Option<String> {
        Some(WORKBENCH_MENU_MOTION_CLASS.to_owned())
    }

    /// `closeAnimation`.
    pub fn close_animation(&self) -> Option<IContextViewCloseAnimation> {
        Some(workbench_menu_close_animation())
    }
}

/// `getWorkbenchMenuMotionContextMenuOptions(anchor)`.
pub fn get_workbench_menu_motion_context_menu_options(anchor: Rc<ViewElement>) -> WorkbenchMenuMotionContextMenuOptions {
    WorkbenchMenuMotionContextMenuOptions { anchor }
}
