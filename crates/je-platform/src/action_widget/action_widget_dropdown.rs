//! Porte parcial de `src/vs/platform/actionWidget/browser/actionWidgetDropdown.ts`: só o movimento de abertura e de
//! fechamento que o `ActionWidgetService` e o seu teste usam (`actionWidgetDropdownCloseAnimation` e
//! `withActionWidgetDropdownMotion`).
//!
//! Fica para a fatia do dropdown: o `ActionWidgetDropdown` (um `BaseDropdown` com ícone e rótulo, a montagem dos itens
//! a partir de `IActionWidgetDropdownAction`, a telemetria e o `ActionWidgetDropdownActionViewItem`), que dependem do
//! widget `BaseDropdown`.

use super::action_list::IActionListOptions;
pub use super::action_widget_motion::{ACTION_WIDGET_DROPDOWN_MOTION_CLASS, ACTION_WIDGET_DROPDOWN_MOTION_CLOSING_CLASS};
use crate::contextview::context_view::{
    CONTEXT_VIEW_MENU_MOTION_CLOSE_ANIMATION_DURATION, IContextViewCloseAnimation, context_view_menu_motion_ancestor_classes,
};

/// `actionWidgetDropdownCloseAnimation`.
pub fn action_widget_dropdown_close_animation() -> IContextViewCloseAnimation {
    IContextViewCloseAnimation {
        class_name: ACTION_WIDGET_DROPDOWN_MOTION_CLOSING_CLASS.to_owned(),
        duration: CONTEXT_VIEW_MENU_MOTION_CLOSE_ANIMATION_DURATION,
        required_ancestor_classes: context_view_menu_motion_ancestor_classes(),
    }
}

/// `className?.split(/\s+/).filter(Boolean) ?? []` com a classe do movimento acrescentada se faltar.
fn with_dropdown_motion_class(class_name: Option<&str>) -> String {
    let mut class_names: Vec<&str> = class_name.map(|names| names.split_whitespace().collect()).unwrap_or_default();
    if !class_names.contains(&ACTION_WIDGET_DROPDOWN_MOTION_CLASS) {
        class_names.push(ACTION_WIDGET_DROPDOWN_MOTION_CLASS);
    }
    class_names.join(" ")
}

/// `withActionWidgetDropdownMotion(listOptions)`.
pub fn with_action_widget_dropdown_motion(list_options: Option<&IActionListOptions>) -> IActionListOptions {
    let base = list_options.cloned().unwrap_or_default();
    IActionListOptions {
        class_name: Some(with_dropdown_motion_class(base.class_name.as_deref())),
        widget_class_name: Some(with_dropdown_motion_class(base.widget_class_name.as_deref())),
        close_animation: base.close_animation.clone().or_else(|| Some(action_widget_dropdown_close_animation())),
        ..base
    }
}
