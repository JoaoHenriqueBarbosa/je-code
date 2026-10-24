//! Porte de `src/vs/workbench/contrib/bulkEdit`: a aplicação de edições em massa (texto, arquivos, células de notebook e
//! edições opacas).
//!
//! Fora do porte, por falta da infraestrutura de que dependem:
//! - `browser/preview/*` (a vista de pré-visualização das edições): é a UI do `BulkEditPane`.

pub mod browser;
