//! A View do editor (porte de `src/vs/editor/browser/viewParts` e `viewModel` do Monaco), sem DOM.
//!
//! - [`layout`]: a geometria (margem, números de linha, conteúdo) e as conversões posição e pixel.
//! - [`view`]: o estado da view (rolagem, cursor, camadas de linha em cache) e a pintura pela lista de
//!   exibição do `je-render`.
//! - [`input`]: ponteiro, teclado, edição, desfazer e área de transferência.
//! - [`environment`]: o catálogo de extensões, a linguagem, a configuração de linguagem e a tokenização.
//! - [`cursor`]: o mínimo do cursor (movimento e seleção por palavra e por linha).
//!
//! O `je-code` cria uma [`EditorView`] por arquivo aberto, repassa os eventos da janela e pinta o editor
//! dentro da moldura do workbench.

pub mod clipboard;
pub mod colors;
pub mod cursor;
pub mod environment;
pub mod input;
pub mod layout;
mod minimap;
pub mod text;
pub mod view;

pub use colors::EditorColors;
pub use environment::EditorEnvironment;
pub use input::{EditKind, EditorKey};
pub use layout::{FontMetrics, ViewLayout};
pub use view::EditorView;
