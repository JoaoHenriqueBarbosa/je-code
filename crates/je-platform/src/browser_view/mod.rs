//! Porte de `src/vs/platform/browserView/common`: a lógica pura do navegador embutido (CSS como texto e
//! o histórico), sem DOM nem webview.

pub mod browser_history;
pub mod css_helpers;
//! Porte de `src/vs/platform/browserView/common`: permissões por origem, grupos de visões do
//! navegador e o trecho de `browserView.ts` de que eles dependem.

pub mod browser_permissions;
pub mod browser_view;
pub mod browser_view_group;
