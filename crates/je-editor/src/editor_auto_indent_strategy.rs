//! Porte de `EditorAutoIndentStrategy` de `src/vs/editor/common/config/editorOptions.ts`.
//!
//! Só o enum: o resto de `editorOptions.ts` (opções do editor) ainda não foi portado. Quando o porte
//! de `editorOptions.ts` chegar, este módulo é absorvido por ele.

/// `EditorAutoIndentStrategy`: as opções de indentação automática do editor, em ordem crescente (o
/// original compara com `>=`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EditorAutoIndentStrategy {
    None = 0,
    Keep = 1,
    Brackets = 2,
    Advanced = 3,
    Full = 4,
}
