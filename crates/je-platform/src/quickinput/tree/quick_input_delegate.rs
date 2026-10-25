//! Porte de `src/vs/platform/quickinput/browser/tree/quickInputDelegate.ts`: o `QuickInputTreeDelegate`, que dá a altura e o
//! modelo de cada linha da árvore.

use super::quick_input_tree_renderer::QuickInputTreeRenderer;
use crate::quickinput::quick_input::IQuickTreeItem;

/// Delegate for QuickInputTree that provides height and template information.
pub struct QuickInputTreeDelegate;

impl QuickInputTreeDelegate {
    pub fn get_height(&self, _element: &IQuickTreeItem) -> f64 {
        22.0
    }

    pub fn get_template_id(&self, _element: &IQuickTreeItem) -> &'static str {
        QuickInputTreeRenderer::ID
    }
}
