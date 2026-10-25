//! Porte de `src/vs/workbench/contrib/bulkEdit`: a aplicação de edições em massa (texto, arquivos, células de notebook e
//! edições opacas).
//!
//! Fora do porte, por falta da infraestrutura de que dependem:
//! - `browser/preview/bulkEditPane.ts` e `bulkEditPreview.ts#BulkEditPreviewProvider` (a vista de pré-visualização das
//!   edições): é a UI do `BulkEditPane`. O modelo (`BulkFileOperations`) está em `browser/preview/bulk_edit_preview`.

pub mod browser;
