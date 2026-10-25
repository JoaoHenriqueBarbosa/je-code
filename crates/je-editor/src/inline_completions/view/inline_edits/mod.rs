//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/`: as views do inline edits.
//!
//! Esta fatia (4c-1) traz a base: `inlineEditsViewInterface` ([`inline_edits_view_interface`]), `inlineEditWithChanges`
//! ([`inline_edit_with_changes`]), `inlineEditsModel` ([`inline_edits_model`]), `theme` ([`theme`]) e `utils/`
//! ([`utils`]). As views em si, o gutter indicator e a `InlineEditsView` entram nas fatias seguintes.
//!
//! Os `.css` do original são especificação: as cores e as medidas viram constantes e dados que o pintor nativo lê.

pub mod inline_edit_with_changes;
pub mod inline_edits_model;
pub mod inline_edits_view_interface;
pub mod theme;
pub mod utils;
