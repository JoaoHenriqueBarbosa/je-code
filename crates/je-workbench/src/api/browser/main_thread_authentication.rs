//! Porte parcial de `src/vs/workbench/api/browser/mainThreadAuthentication.ts`: a revivificação das sessões que chegam do host de
//! extensões.
//!
//! Portado: [`revive_authentication_session`] (`reviveAuthenticationSession`), que é o que `mainThreadAuthentication.test.ts` exercita.
//!
//! Ainda sem porte (o resto do arquivo), porque depende de peças que não existem no porte: `prepareSessionRequest`,
//! `MainThreadAuthenticationProvider`, `MainThreadAuthenticationProviderWithChallenges` e `MainThreadAuthentication` precisam do
//! `MainThreadAuthenticationShape` e do `ExtHostAuthenticationShape` do protocolo (só os DTOs de sessão foram portados, ver
//! `ext_host_protocol.rs`), de `withoutWorkbenchOnlySessionOptions`, do `IDynamicAuthenticationProviderStorageService`, do
//! `ISecretStorageService`, do `IURLService`, do `IClipboardService`, do `mcpOAuthClientSecretStorageKey` e da configuração de MCP
//! (`mcpConfiguration.ts`), do `fetchAuthorizationServerMetadata` (de `oauth.ts`, que usa `fetch`; ver `je_base::oauth`) e da
//! `IExtensionService` com `ActivationKind`.
//!
//! Divergências:
//! - `Dto<AuthenticationSession>` é [`AuthenticationSessionDto`] (os `UriComponents` no lugar das `URI`); `URI.revive` de componentes é
//!   `URI::from` sem rigor (o `revive` do original só reconstrói as partes), e a ausência (`undefined`) é `None`.

use je_base::uri::{UriComponents, URI};

use crate::api::common::ext_host_protocol::AuthenticationSessionDto;
use crate::services::authentication::authentication::{AuthenticationSession, AuthenticationSessionAccount};

/// `URI.revive(components)`, com a ausência preservada.
pub fn revive_uri(components: Option<&UriComponents>) -> Option<URI> {
    components.map(|components| URI::from(components, false))
}

/// `reviveAuthenticationSession`: revives session URIs received from the extension host.
pub fn revive_authentication_session(session: &AuthenticationSessionDto) -> AuthenticationSession {
    AuthenticationSession {
        id: session.id.clone(),
        access_token: session.access_token.clone(),
        account: AuthenticationSessionAccount {
            label: session.account.label.clone(),
            id: session.account.id.clone(),
            icon: revive_uri(session.account.icon.as_ref()),
        },
        scopes: session.scopes.clone(),
        id_token: session.id_token.clone(),
        expires_after: session.expires_after,
        authorization_server: revive_uri(session.authorization_server.as_ref()),
    }
}
