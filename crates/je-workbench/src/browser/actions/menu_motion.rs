//! Porte de `src/vs/workbench/browser/actions/menuMotion.ts`: o movimento dos menus de contexto do workbench, que reaproveita
//! as constantes do `contextview.ts`.
//!
//! Divergência: `getWorkbenchMenuMotionContextMenuOptions(anchor)` devolve um `Pick<IContextMenuDelegate, ...>` literal; o
//! delegado nativo é um trait, e quem monta o menu (`GlobalCompositeBar`) implementa `get_anchor`, `get_menu_class_name` e
//! `close_animation` com [`WORKBENCH_MENU_MOTION_CLASS`] e [`workbench_menu_close_animation`] diretamente.

use je_platform::contextview::context_view::{
    CONTEXT_VIEW_MENU_MOTION_CLASS, CONTEXT_VIEW_MENU_MOTION_CLOSE_ANIMATION_DURATION, CONTEXT_VIEW_MENU_MOTION_CLOSING_CLASS, IContextViewCloseAnimation,
    context_view_menu_motion_ancestor_classes,
};

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
