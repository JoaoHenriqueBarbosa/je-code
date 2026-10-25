//! Porte de `src/vs/workbench/contrib/modernUI`: a atualização visual (Modern UI) do workbench.

// As configurações do vidro fosco e as migrações das chaves antigas (trecho do `workbench.contribution.ts`).
pub mod frosted_glass_configuration;

// O material do vidro fosco (`browser/media/frostedGlass.css`).
pub mod frosted_glass;

// A contribuição que liga o vidro fosco nos contêineres (`electron-browser/frostedGlass.contribution.ts`).
pub mod frosted_glass_contribution;

// A contribuição que liga as classes do Modern UI nos contêineres (`browser/modernUI.contribution.ts`).
pub mod modern_ui_contribution;

// O fundo do cabeçalho do grupo (`browser/media/tabs.css` e `connectedEditorTabs.css`).
pub mod tabs_header;

// O estilo resolvido das regras de `floatingPanels.css` e de `modernUI/browser/media/*.css` (o `getComputedStyle`).
pub mod resolved_style;
