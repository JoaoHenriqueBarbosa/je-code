//! Porte de `src/vs/platform/browserView/common/browserViewGroup.ts`: o filtro que escolhe quais
//! visões do navegador entram num grupo e o portão de visibilidade das automações em sandbox.
//!
//! Divergências:
//! - As interfaces `IBrowserViewGroup` e `IBrowserViewGroupService` (grupo e serviço de grupos entre
//!   processos, com `Event`s por grupo, mensagens CDP e `ISandboxNetworkRestrictions`) não foram
//!   portadas: são contratos de IPC sobre tipos do CDP e do navegador, sem lógica. O que tem lógica,
//!   o filtro e as duas funções de casamento, está aqui.
//! - `IBrowserViewGroupFilter` é [`BrowserViewGroupFilter`], com `Option` nos três campos opcionais.
//!   O tipo [`BrowserViewOwner`] e as audiências vêm de [`super::browser_view`].

use super::browser_view::{BrowserViewAudience, BrowserViewOwner, matches_browser_view_audience};

/// `ipcBrowserViewGroupChannelName`.
pub const IPC_BROWSER_VIEW_GROUP_CHANNEL_NAME: &str = "browserViewGroup";

/// `IBrowserViewGroupFilter`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrowserViewGroupFilter {
    /// Exige posse de agente e armazenamento isolado para esta sessão, mesmo nas visões adicionadas
    /// explicitamente.
    pub sandbox_session_id: Option<String>,
    /// Inclui as visões concedidas a esta audiência.
    pub audience: Option<BrowserViewAudience>,
    /// Inclui estas visões, quaisquer que sejam as audiências.
    pub browser_ids: Option<Vec<String>>,
}

/// `matchesBrowserViewGroupFilter`.
pub fn matches_browser_view_group_filter(browser_id: &str, audiences: &[BrowserViewAudience], filter: &BrowserViewGroupFilter) -> bool {
    filter.browser_ids.as_ref().is_some_and(|ids| ids.iter().any(|id| id == browser_id))
        || filter.audience.as_ref().is_some_and(|audience_filter| audiences.iter().any(|audience| matches_browser_view_audience(audience_filter, audience)))
}

/// `matchesBrowserViewSandboxSession`: o portão de visibilidade compartilhado entre a automação em
/// sandbox e a lista de páginas do workbench.
pub fn matches_browser_view_sandbox_session(owner: Option<&BrowserViewOwner>, sandbox_session_id: Option<&str>, expected_session_id: Option<&str>) -> bool {
    let Some(expected) = expected_session_id else { return true };
    matches!(owner, Some(BrowserViewOwner::Agent { session_id }) if session_id == expected && sandbox_session_id == Some(expected))
}
