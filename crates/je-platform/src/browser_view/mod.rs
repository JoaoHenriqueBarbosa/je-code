//! Porte de `src/vs/platform/browserView/common`: a lógica pura do navegador embutido (CSS como texto, o
//! histórico, permissões por origem, grupos de visões e o trecho de `browserView.ts` de que dependem),
//! sem DOM nem webview.

pub mod browser_history;
pub mod browser_permissions;
pub mod browser_view;
pub mod browser_view_group;
pub mod css_helpers;
