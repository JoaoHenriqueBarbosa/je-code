//! Porte de `src/vs/workbench/api/common/extHost.api.impl.ts`, só o que a suíte `ExtHost API` exercita:
//! [`get_terminal_internal_options`] (`getTerminalInternalOptions`). O resto do arquivo (`createApiFactoryAndRegisterActors`,
//! a fábrica da superfície `vscode.*`) depende do host de extensões completo e fica de fora.
//!
//! Divergências:
//! - [`TerminalOptions`] é o `vscode.TerminalOptions` reduzido ao que `getTerminalInternalOptions` lê
//!   (`isRemoteResolverTerminal`, da proposta `terminalRemoteResolver`) e aos campos escalares. `shellArgs`, `cwd`, `env`,
//!   `iconPath`, `color` e `location` dependem de tipos que o porte do terminal do host ainda não tem.
//! - [`TerminalInternalOptions`] (`ITerminalInternalOptions`, de `extHostTerminalService.ts`) só tem
//!   `isRemoteResolverTerminal`, o único campo que a função preenche; os demais (`cwd`, `isFeatureTerminal`,
//!   `forceShellIntegration`, `useShellEnvironment`, `resolvedExtHostIdentifier`, `location`) entram junto com o porte de
//!   `extHostTerminalService.ts`.
//! - `checkProposedApiEnabled` lança no original: aqui [`get_terminal_internal_options`] devolve [`Result`].

use je_base::errors::Error;
use je_extensions::scanner::ExtensionDescription;

use crate::services::extensions::extensions::check_proposed_api_enabled;

/// `vscode.TerminalOptions` (ver as divergências no topo do módulo).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TerminalOptions {
    pub name: Option<String>,
    pub shell_path: Option<String>,
    pub strict_env: Option<bool>,
    pub hide_from_user: Option<bool>,
    pub message: Option<String>,
    pub is_transient: Option<bool>,
    pub shell_integration_nonce: Option<String>,
    /// `isRemoteResolverTerminal` (proposta `terminalRemoteResolver`).
    pub is_remote_resolver_terminal: Option<bool>,
}

/// `ITerminalInternalOptions` (ver as divergências no topo do módulo).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerminalInternalOptions {
    pub is_remote_resolver_terminal: Option<bool>,
}

/// `getTerminalInternalOptions(extension, options)`.
pub fn get_terminal_internal_options(extension: &ExtensionDescription, options: &TerminalOptions) -> Result<Option<TerminalInternalOptions>, Error> {
    if options.is_remote_resolver_terminal == Some(true) {
        check_proposed_api_enabled(extension, "terminalRemoteResolver")?;
        return Ok(Some(TerminalInternalOptions { is_remote_resolver_terminal: Some(true) }));
    }
    Ok(None)
}
