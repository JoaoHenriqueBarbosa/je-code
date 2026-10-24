//! Porte de `src/vs/workbench/contrib/bulkEdit`: a aplicação de edições em massa (texto, arquivos e edições opacas).
//!
//! Fora do porte, por falta da infraestrutura de que dependem:
//! - `browser/bulkCellEdits.ts` (`ResourceNotebookCellEdit` e `BulkCellEdits`): usa o `INotebookEditorModelResolverService`
//!   e o `NotebookTextModel.applyEdits`, que não têm porte.
//! - `browser/preview/*` (a vista de pré-visualização das edições): é a UI do `BulkEditPane`.

pub mod browser;
