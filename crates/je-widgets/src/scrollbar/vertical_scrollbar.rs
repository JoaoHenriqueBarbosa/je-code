//! Porte de `src/vs/base/browser/ui/scrollbar/verticalScrollbar.ts`. A lógica vive em
//! [`Scrollbar`](crate::scrollbar::abstract_scrollbar::Scrollbar) com [`Axis::Vertical`]; este módulo é
//! só o construtor com o nome do original.

use std::rc::Rc;

use je_base::scrollable::Scrollable;
use je_ui::Ui;

use crate::scrollbar::abstract_scrollbar::{Axis, Scrollbar, ScrollbarHost};
use crate::scrollbar::scrollable_element_options::ScrollableElementResolvedOptions;

/// `new VerticalScrollbar(scrollable, options, host)`.
pub fn vertical_scrollbar(ui: &Ui, scrollable: Rc<Scrollable>, options: &ScrollableElementResolvedOptions, host: Rc<dyn ScrollbarHost>) -> Rc<Scrollbar> {
    Scrollbar::new(ui, Axis::Vertical, scrollable, options, host)
}
