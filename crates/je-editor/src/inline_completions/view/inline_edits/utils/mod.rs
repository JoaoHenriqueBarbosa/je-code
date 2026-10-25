//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/utils/`.
//!
//! - [`utils`]: `utils.ts`;
//! - [`flex_box_layout`]: `flexBoxLayout.ts`;
//! - `towersLayout.ts` vive em `inline_completions::towers_layout` (porte da fatia 0) e é reexportado aqui, sem
//!   segunda cópia.

pub mod flex_box_layout;
pub mod utils;

pub use crate::inline_completions::towers_layout;
