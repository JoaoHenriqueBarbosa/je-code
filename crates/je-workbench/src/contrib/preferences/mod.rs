//! Porte de `src/vs/workbench/contrib/preferences`: o que o workbench usa das preferências.

pub mod smart_snippet_inserter;

// Editor de configurações: o modelo da árvore (`browser/settingsTreeModels.ts`), o sumário (`browser/settingsLayout.ts`) e
// os trechos de `browser/preferencesWidgets.ts`, `browser/preferencesSearch.ts` e
// `browser/settingsEditorSettingIndicators.ts` de que o modelo depende.
pub mod common;
pub mod preferences_search;
pub mod preferences_widgets;
pub mod settings_editor2;
pub mod settings_editor_setting_indicators;
pub mod settings_layout;
pub mod settings_tree_models;
