//! Porte parcial de `src/vs/workbench/api/common/extHostXaaAuthProvider.ts`: as funções puras que os testes exercitam
//! (`IDP_SCOPES`, `cacheKey`, `isExpired` e `toSession`) e o tipo `IResourceCacheEntry`.
//!
//! Ainda sem porte: o mixin `XaifyAuthProvider` (a classe `XaaAuthenticationProvider`), que estende o `DynamicAuthProvider` de
//! `extHostAuthentication.ts` (não portado, ver o cabeçalho de `ext_host_authentication.rs`) e usa o `fetch`, o
//! `fetchAuthorizationServerMetadata` e o `Response`; `safeText` (só serve a esse mixin).
//!
//! Divergências:
//! - `isExpired(entry: { token: { expires_in? }, created_at }, now = Date.now())` recebe o [`ExpiryEntry`] (o que a função lê do
//!   registro) e `now: Option<f64>`, em que `None` é o valor padrão (`Date.now()`).
//! - `cacheKey` ordena os escopos por unidades UTF-16, que é o `sort()` padrão do JavaScript.
//! - `vscode.AuthenticationSessionAccountInformation` é o [`AuthenticationSessionAccount`] (sem ícone).
//! - `claims.sub || 'unknown'` e as cadeias de `||` tratam a string vazia como ausente (o `non_empty_claim` de
//!   `ext_host_authentication.rs`).

use je_base::hash::string_hash;
use je_base::js_string::JsString;
use je_base::oauth::{get_claims_from_jwt, IAuthorizationTokenResponse};
use je_base::runtime;

use super::ext_host_authentication::non_empty_claim;
use crate::services::authentication::authentication::{AuthenticationSession, AuthenticationSessionAccount};

/// Scopes used when bootstrapping the IdP session for an XAA flow.
///
/// `openid` is required because the ID-JAG token exchange uses the IdP-issued `id_token` as `subject_token` (per
/// draft-ietf-oauth-identity-assertion-authz-grant section 3.1, the subject token MUST be of type
/// `urn:ietf:params:oauth:token-type:id_token`). `offline_access` is requested so we get a refresh token for the IdP session.
pub const IDP_SCOPES: [&str; 2] = ["openid", "offline_access"];

/// `IResourceCacheEntry`.
#[derive(Clone, Debug, PartialEq)]
pub struct ResourceCacheEntry {
    pub resource: String,
    pub scopes: Vec<String>,
    pub token: IAuthorizationTokenResponse,
    /// Fallback identity (the IdP login account) for sessions built from this token, used when the resource token has no id_token of
    /// its own.
    pub account: AuthenticationSessionAccount,
    pub created_at: f64,
}

/// O que [`is_expired`] lê do registro: `entry.token.expires_in` e `entry.created_at`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ExpiryEntry {
    pub expires_in: Option<f64>,
    pub created_at: f64,
}

impl From<&ResourceCacheEntry> for ExpiryEntry {
    fn from(entry: &ResourceCacheEntry) -> Self {
        Self { expires_in: entry.token.expires_in.map(|expires_in| expires_in.0), created_at: entry.created_at }
    }
}

/// Cache key for resource-scoped tokens. Exported for testing.
pub fn cache_key<S: AsRef<str>>(resource: &str, scopes: &[S]) -> String {
    let mut sorted: Vec<&str> = scopes.iter().map(AsRef::as_ref).collect();
    sorted.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    format!("{resource}|{}", sorted.join(" "))
}

/// Returns true if the cached token is past (or within 60s of) its expiry. Pure and exported for testing.
///
/// Mints fresh ID-JAG assertions are usually short-lived (minutes). We treat tokens as expired 60s before their nominal expiry to
/// avoid clock skew and in-flight redemptions racing past `exp`. Tokens without `expires_in` defined are treated as never-expiring
/// (cached until the process exits); `expires_in: 0` is treated as immediately expired.
pub fn is_expired(entry: ExpiryEntry, now: Option<f64>) -> bool {
    let Some(expires_in) = entry.expires_in else {
        return false;
    };
    let now = now.unwrap_or_else(|| runtime::date_now() as f64);
    now > entry.created_at + (expires_in * 1000.0) - 60_000.0
}

/// Builds a session from a token response. Identity precedence: the token's own `id_token`, then `fallback_account` (the IdP login
/// identity), then a generic default. Never the `access_token`, which for XAA is an opaque resource credential. Exported for testing.
pub fn to_session(token: &IAuthorizationTokenResponse, scopes: &[String], fallback_account: Option<&AuthenticationSessionAccount>) -> AuthenticationSession {
    let mut account: Option<AuthenticationSessionAccount> = None;
    if let Some(id_token) = &token.id_token {
        // Ignore: the id_token wasn't a decodable JWT.
        if let Ok(claims) = get_claims_from_jwt(id_token) {
            let claims = Some(claims);
            account = Some(AuthenticationSessionAccount {
                id: non_empty_claim(&claims, "sub").unwrap_or_else(|| "unknown".to_owned()),
                label: non_empty_claim(&claims, "preferred_username")
                    .or_else(|| non_empty_claim(&claims, "name"))
                    .or_else(|| non_empty_claim(&claims, "email"))
                    .unwrap_or_else(|| "XAA".to_owned()),
                icon: None,
            });
        }
    }
    let account = account.or_else(|| fallback_account.cloned()).unwrap_or_else(|| AuthenticationSessionAccount::new("XAA", "unknown"));
    AuthenticationSession {
        id: string_hash(JsString::from(token.access_token.as_str()).units(), 0).to_string(),
        access_token: token.access_token.clone(),
        account,
        scopes: scopes.to_vec(),
        id_token: token.id_token.clone(),
        expires_after: None,
        authorization_server: None,
    }
}
