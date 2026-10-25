//! Porte de `src/vs/workbench/contrib/preferences`: o que o workbench usa das preferências.

pub mod smart_snippet_inserter;

// Editor de configurações: o modelo da árvore (`browser/settingsTreeModels.ts`), o sumário (`browser/settingsLayout.ts`),
// o modelo do sumário lateral (`browser/tocTree.ts`), o filtro e a resolução da árvore (`browser/settingsTree.ts`), a lógica
// do editor (`browser/settingsEditor2.ts`) e os trechos de `browser/preferencesWidgets.ts`, `browser/preferencesSearch.ts` e
// `browser/settingsEditorSettingIndicators.ts` de que o modelo depende.
pub mod common;
pub mod keybindings_editor_contribution;
pub mod preferences_search;
pub mod preferences_widgets;
pub mod settings_editor2;
pub mod settings_editor_setting_indicators;
pub mod settings_layout;
pub mod settings_search_menu;
pub mod settings_tree;
pub mod settings_tree_models;
pub mod settings_widgets;
pub mod toc_tree;
