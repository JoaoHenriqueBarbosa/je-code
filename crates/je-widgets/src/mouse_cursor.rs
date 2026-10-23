//! Porte de `src/vs/base/browser/ui/mouseCursor/mouseCursor.ts` e `mouseCursor.css`: a classe
//! `monaco-mouse-cursor-text`, que dá ao elemento o cursor de texto (`cursor: text`).
//!
//! No `je-ui` o cursor é o [`Cursor`] do estilo do nó, e a classe é só um marcador (o código do VS Code a lê
//! e escreve): [`apply_mouse_cursor_text`] faz as duas coisas, a classe do original e a regra de CSS.

use je_ui::{Cursor, NodeId, Tree};

/// `MOUSE_CURSOR_TEXT_CSS_CLASS_NAME`.
pub const MOUSE_CURSOR_TEXT_CSS_CLASS_NAME: &str = "monaco-mouse-cursor-text";

/// `element.classList.toggle(MOUSE_CURSOR_TEXT_CSS_CLASS_NAME, enabled)` mais `.monaco-mouse-cursor-text { cursor: text }`.
pub fn apply_mouse_cursor_text(tree: &mut Tree, node: NodeId, enabled: bool) {
    tree.toggle_class(node, MOUSE_CURSOR_TEXT_CSS_CLASS_NAME, enabled);
    tree.style_mut(node).cursor = enabled.then_some(Cursor::Text);
}
