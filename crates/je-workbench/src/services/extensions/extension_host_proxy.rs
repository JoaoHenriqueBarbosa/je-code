//! Porte parcial de `src/vs/workbench/services/extensions/common/extensionHostProxy.ts`: só o resultado de
//! `resolveAuthority` (`IResolveAuthorityErrorResult`, `IResolveAuthorityOKResult`, `IResolveAuthorityResult`), que o
//! `IExtensionHostManager` devolve ao `AbstractExtensionService`.
//!
//! Fora do porte: a interface `IExtensionHostProxy` (o lado do workbench do protocolo com o host), que só o `ExtensionHostManager`
//! concreto usa, e que depende de `IRemoteConnectionData` e de `VSBuffer` nos testes de latência.
//!
//! Divergências: `message: string | undefined` é `Option<String>` e `detail: unknown` é `Option<Dynamic>`.

use je_base::dynamic::Dynamic;
use je_platform::remote::remote_authority_resolver::{RemoteAuthorityResolverErrorCode, ResolverResult};

/// O `error` de `IResolveAuthorityErrorResult`.
#[derive(Clone, Debug)]
pub struct ResolveAuthorityError {
    pub message: Option<String>,
    pub code: RemoteAuthorityResolverErrorCode,
    pub detail: Option<Dynamic>,
}

/// `IResolveAuthorityResult = IResolveAuthorityErrorResult | IResolveAuthorityOKResult`.
#[derive(Clone, Debug)]
pub enum IResolveAuthorityResult {
    /// `IResolveAuthorityErrorResult` (`type: 'error'`).
    Error(ResolveAuthorityError),
    /// `IResolveAuthorityOKResult` (`type: 'ok'`).
    Ok(ResolverResult),
}
