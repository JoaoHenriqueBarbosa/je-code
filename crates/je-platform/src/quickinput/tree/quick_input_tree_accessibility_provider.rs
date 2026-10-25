//! Porte de `src/vs/platform/quickinput/browser/tree/quickInputTreeAccessibilityProvider.ts`: o `QuickTreeAccessibilityProvider`,
//! que dá à árvore o rótulo, o papel e o estado marcado que o leitor de tela lê.
//!
//! Um item é o mesmo item de um evento quando tem o mesmo `id` (ver [`same_tree_item`]).

use std::rc::Rc;

use je_base::event::Event;
use je_base::icon_labels::get_codicon_aria_label;
use je_base::lifecycle::DisposableRef;
use je_base::nls::localize;

use crate::quickinput::quick_input::{IQuickTreeCheckboxEvent, IQuickTreeItem, QuickTreeItemChecked};

/// O `e.item === element` do original: itens com o mesmo `id` (o controlador dá um `id` a todo item da árvore).
pub fn same_tree_item(a: &IQuickTreeItem, b: &IQuickTreeItem) -> bool {
    a.base.id.is_some() && a.base.id == b.base.id
}

/// Accessibility provider for QuickTree.
pub struct QuickTreeAccessibilityProvider {
    on_checked_event: Event<IQuickTreeCheckboxEvent>,
}

impl QuickTreeAccessibilityProvider {
    pub fn new(on_checked_event: Event<IQuickTreeCheckboxEvent>) -> Self {
        Self { on_checked_event }
    }

    pub fn get_widget_aria_label(&self) -> String {
        localize("quickTree", "Quick Tree", &[])
    }

    pub fn get_aria_label(&self, element: &IQuickTreeItem) -> String {
        if let Some(aria_label) = element.base.aria_label.as_ref().filter(|label| !label.is_empty()) {
            return aria_label.clone();
        }
        [Some(element.base.label.clone()), element.base.description.clone()]
            .into_iter()
            .map(|part| get_codicon_aria_label(part.as_ref().map(|part| part.units())).to_string())
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn get_widget_role(&self) -> &'static str {
        "tree"
    }

    pub fn get_role(&self, _element: &IQuickTreeItem) -> &'static str {
        "checkbox"
    }

    /// O `value` de `isChecked(element)`: `'mixed'` ou `!!checked`.
    pub fn is_checked(&self, element: &IQuickTreeItem) -> QuickTreeItemChecked {
        match element.checked {
            Some(QuickTreeItemChecked::Mixed) => QuickTreeItemChecked::Mixed,
            checked => QuickTreeItemChecked::Bool(checked == Some(QuickTreeItemChecked::Bool(true))),
        }
    }

    /// O `onDidChange` de `isChecked(element)`: avisa quando o estado do `element` muda.
    pub fn on_did_change_checked(&self, element: &IQuickTreeItem, on_change: Rc<dyn Fn()>) -> DisposableRef {
        let element = element.clone();
        self.on_checked_event.listen(move |event| {
            if same_tree_item(&event.item, &element) {
                on_change();
            }
        })
    }
}
