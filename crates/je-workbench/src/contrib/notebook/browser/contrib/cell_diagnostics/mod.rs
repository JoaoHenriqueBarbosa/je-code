//! Porte de `src/vs/workbench/contrib/notebook/browser/contrib/cellDiagnostics/cellDiagnostics.ts`: o arquivo só importa as
//! contribuições da pasta, e aqui cada uma é um módulo. Está portada a `cellDiagnosticEditorContrib.ts`; as ações
//! (`cellDiagnosticsActions.ts`) e a barra de status (`diagnosticCellStatusBarContrib.ts`) ficam para as fatias delas.

pub mod cell_diagnostic_editor_contrib;
