//! Porte de `src/vs/platform/quickinput/browser/tree/quickInputTreeFilter.ts`.
//!
//! `TreeVisibility` é o de `je-widgets` (`tree::tree_types`), que não é dependência de `je-platform` (ver a nota do
//! [`super::quick_input_tree_sorter`]); o enum daqui tem os mesmos três casos.

use std::cell::{Cell, RefCell};

use je_base::icon_labels::{matches_fuzzy_icon_aware, parse_label_with_icons};
use je_base::js_string::JsString;

use super::quick_input_tree::QuickTreeFilterData;
use crate::quickinput::quick_input::IQuickTreeItem;

/// `TreeVisibility`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeVisibility {
    /// The tree node should be hidden.
    Hidden,
    /// The tree node should be visible.
    Visible,
    /// The tree node should be visible if any of its descendants is visible.
    Recurse,
}

/// `ITreeFilterDataResult<IQuickTreeFilterData>`.
#[derive(Clone, Debug, PartialEq)]
pub struct QuickTreeFilterResult {
    pub visibility: TreeVisibility,
    pub data: QuickTreeFilterData,
}

pub struct QuickInputTreeFilter {
    filter_value: RefCell<String>,
    match_on_label: Cell<bool>,
    match_on_description: Cell<bool>,
}

impl Default for QuickInputTreeFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl QuickInputTreeFilter {
    pub fn new() -> Self {
        Self { filter_value: RefCell::new(String::new()), match_on_label: Cell::new(true), match_on_description: Cell::new(false) }
    }

    pub fn filter_value(&self) -> String {
        self.filter_value.borrow().clone()
    }

    pub fn set_filter_value(&self, value: &str) {
        *self.filter_value.borrow_mut() = value.to_owned();
    }

    pub fn match_on_label(&self) -> bool {
        self.match_on_label.get()
    }

    pub fn set_match_on_label(&self, value: bool) {
        self.match_on_label.set(value);
    }

    pub fn match_on_description(&self) -> bool {
        self.match_on_description.get()
    }

    pub fn set_match_on_description(&self, value: bool) {
        self.match_on_description.set(value);
    }

    pub fn filter(&self, element: &IQuickTreeItem, parent_visibility: TreeVisibility) -> QuickTreeFilterResult {
        let filter_value = self.filter_value.borrow();
        if filter_value.is_empty() || !(self.match_on_label.get() || self.match_on_description.get()) {
            let visibility = if element.children.is_some() { TreeVisibility::Recurse } else { TreeVisibility::Visible };
            return QuickTreeFilterResult { visibility, data: QuickTreeFilterData::default() };
        }

        let query: Vec<u16> = filter_value.encode_utf16().collect();
        let label_highlights =
            if self.match_on_label.get() { matches_fuzzy_icon_aware(&query, &parse_label_with_icons(element.base.label.units()), false) } else { None };
        let description_highlights = if self.match_on_description.get() {
            let description = element.base.description.clone().unwrap_or_else(|| JsString::from(""));
            matches_fuzzy_icon_aware(&query, &parse_label_with_icons(description.units()), false)
        } else {
            None
        };

        let visibility = if parent_visibility == TreeVisibility::Visible {
            // Parent is visible because it had matches, so we show all children
            TreeVisibility::Visible
        } else if label_highlights.is_some() || description_highlights.is_some() {
            // If we have any highlights, we are visible
            TreeVisibility::Visible
        } else if element.children.is_some() {
            // Otherwise, we defer to the children or if no children, we are hidden
            TreeVisibility::Recurse
        } else {
            TreeVisibility::Hidden
        };

        QuickTreeFilterResult { visibility, data: QuickTreeFilterData { label_highlights, description_highlights } }
    }
}
