//! Porte parcial de `src/vs/workbench/contrib/multiDiffEditor/browser/multiDiffEditorInput.ts`: só o identificador do tipo de
//! editor (`MultiDiffEditorInput.ID`), que quem ainda não depende do `MultiDiffEditorInput` usa no lugar do `instanceof`
//! (a comparação com o `editor_type_id` da entrada). O resto da classe entra com o editor de múltiplos diffs.

/// `MultiDiffEditorInput.ID`.
pub const MULTI_DIFF_EDITOR_INPUT_ID: &str = "workbench.input.multiDiffEditor";
