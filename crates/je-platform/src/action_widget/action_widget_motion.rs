//! Porte parcial de `src/vs/platform/actionWidget/browser/actionWidgetMotion.ts`: as classes do movimento do widget e o fim da
//! animação de entrada.
//!
//! Divergência: o `finishActionWidgetOpeningAnimation` do original percorre `widget.getAnimations()` e termina a
//! `CSSAnimation` `action-widget-scale-open`. O widget nativo não tem animação de CSS; ele guarda se a entrada terminou
//! ([`ViewElement::finish_opening_animation`]) e o pintor lê esse estado.

use crate::contextview::view_element::ViewElement;

pub const ACTION_WIDGET_ANIMATED_CLASS: &str = "action-widget-animated";
pub const ACTION_WIDGET_DROPDOWN_MOTION_CLASS: &str = "action-widget-dropdown";
pub const ACTION_WIDGET_DROPDOWN_MOTION_CLOSING_CLASS: &str = "action-widget-dropdown-closing";

/// Completes the scale entrance before measuring or positioning dependent UI.
pub fn finish_action_widget_opening_animation(widget: &ViewElement) {
    widget.finish_opening_animation();
}
