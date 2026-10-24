//! Porte parcial de `src/vs/workbench/services/authentication/browser/authenticationMcpService.ts`: só a
//! interface [`IAuthenticationMcpService`], que os serviços de acesso, de uso e de consulta chamam. A classe
//! `AuthenticationMcpService` (diálogos, seletor rápido, selo de atividade e a fila de pedidos de login) é
//! UI e runtime de MCP, fora do recorte, e não entra.
//!
//! Divergências:
//! - **A interface** tem só as preferências de conta (`onDidChangeAccountPreference`, `getAccountPreference`,
//!   `updateAccountPreference`, `removeAccountPreference`), que são o que os serviços portados chamam. Os
//!   membros de preferência de sessão (obsoletos), `selectSession`, `requestSessionAccess` e
//!   `requestNewSession` entram quando o `AuthenticationService` for portado (o tipo `AuthenticationSession`
//!   já existe em `authentication.rs`).
//! - **O evento** emite o `{ mcpServerIds, providerId }` anônimo do original, a struct
//!   [`McpAccountPreferenceChange`].

use je_base::event::Event;
use je_instantiation::service_id;

use super::authentication::AuthenticationSessionAccount;

/// O `{ mcpServerIds: string[]; providerId: string }` de `onDidChangeAccountPreference`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpAccountPreferenceChange {
    pub mcp_server_ids: Vec<String>,
    pub provider_id: String,
}

// Pendência anotada no original por Tyler Leonhardt: mover isto para o `MainThreadAuthentication`.
/// `IAuthenticationMcpService`, reduzido às preferências de conta (ver o topo do arquivo).
pub trait IAuthenticationMcpService {
    /// Fires when an account preference for a specific provider has changed for the specified MCP servers.
    /// Does not fire when an account preference is removed.
    fn on_did_change_account_preference(&self) -> Event<McpAccountPreferenceChange>;

    /// Returns the accountName (also known as account.label) to pair with `IAuthenticationMcpAccessService`
    /// to get the account preference, or `None` if there is no preference set.
    ///
    /// - `mcp_server_id`: the MCP server id to get the preference for.
    /// - `provider_id`: the authentication provider id.
    fn get_account_preference(&self, mcp_server_id: &str, provider_id: &str) -> Option<String>;

    /// Sets the account preference for the given provider and MCP server.
    fn update_account_preference(&self, mcp_server_id: &str, provider_id: &str, account: &AuthenticationSessionAccount);

    /// Removes the account preference for the given provider and MCP server.
    fn remove_account_preference(&self, mcp_server_id: &str, provider_id: &str);
}

service_id!(pub I_AUTHENTICATION_MCP_SERVICE: dyn IAuthenticationMcpService = "IAuthenticationMcpService");
