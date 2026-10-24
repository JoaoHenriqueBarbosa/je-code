//! A View do editor (porte de `src/vs/editor/browser/viewParts` e `viewModel` do Monaco), sem DOM.
//!
//! - [`layout`]: a geometria (margem, números de linha, conteúdo) e as conversões posição e pixel.
//! - [`view`]: o estado da view (rolagem, cursor, camadas de linha em cache) e a pintura pela lista de
//!   exibição do `je-render`.
//! - [`input`]: ponteiro, teclado, edição, desfazer e área de transferência, como comandos do `CodeEditorWidget`.
//! - `core`: o `CodeEditorWidget` do `je-editor` (cursores, comandos, ações de linha, multicursor) que a view conduz.
//! - `shortcuts`: a resolução de atalhos pelo `KeybindingsRegistry`, com acordes em sequência.
//! - [`services`]: os serviços mínimos que o widget recebe (notificação, comandos, tema) e o controlador de busca.
//! - [`environment`]: o catálogo de extensões, a linguagem, a configuração de linguagem e a tokenização.
//!
//! O `je-code` cria uma [`EditorView`] por arquivo aberto, repassa os eventos da janela e pinta o editor
//! dentro da moldura do workbench.

pub mod clipboard;
pub mod colors;
mod core;
pub mod environment;
pub mod folding;
pub mod input;
mod shortcuts;
pub mod services;
pub mod settings;
pub mod layout;
mod minimap;
pub mod text;
pub mod view;

pub use colors::EditorColors;
pub use environment::EditorEnvironment;
pub use input::{CopiedText, EditorKey};
pub use layout::{FontMetrics, ViewLayout};
pub use settings::EditorSettings;
pub use shortcuts::set_user_keybindings;
pub use view::{EditorView, GutterChange, GutterKind};
