//! Widgets de `vs/base/browser/ui` do VS Code em cima do `je-ui`.
//!
//! Cada módulo é o porte de um arquivo do VS Code, com a lógica (tamanhos, proporções, árvore,
//! prioridades, snap) igual à do original e o que ali mexe em `HTMLElement` trocado por nós da árvore do
//! [`je_ui`]. As regras de CSS que o original deixa para a folha de estilo viram código do widget, ver a
//! documentação de cada módulo.

pub mod sash;
pub mod grid;
pub mod gridview;
pub mod paneview;
pub mod splitview;
