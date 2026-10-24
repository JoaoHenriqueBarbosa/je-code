//! Porte de `src/vs/workbench/services/preferences/common`: o que o editor de preferências valida e mostra.

pub mod preferences_validation;

// Editor de atalhos: os tipos de `common/preferences.ts` e o modelo de `browser/keybindingsEditorModel.ts`.
#[allow(clippy::module_inception)] // `preferences.rs` é o porte de `preferences.ts`, como no original
pub mod preferences;
pub mod keybindings_editor_model;
