//! Porte de `src/vs/editor/common/viewModel/viewModelDecoration.ts`: `ViewModelDecoration`,
//! `isModelDecorationVisible`, `isModelDecorationInComment` e `isModelDecorationInString`.
//!
//! Divergência de tradução: o porte já existe em [`crate::view_model_types`], onde o `inline_decorations` e o
//! `IViewModel` o consomem (`testTokensInRange` incluído, como função privada de lá). Este módulo só reexporta
//! esses itens sob o nome do arquivo original, para o caminho `view_model_decoration::...` valer como no
//! TypeScript, sem uma segunda cópia do código.

pub use crate::view_model_types::{
    ViewModelDecoration, is_model_decoration_in_comment, is_model_decoration_in_string, is_model_decoration_visible,
};
