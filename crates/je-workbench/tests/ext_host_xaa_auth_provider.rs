//! Porte de `src/vs/workbench/api/test/browser/extHostXaaAuthProvider.test.ts` (suítes `XaaAuthProvider helpers` e `XaaAuthProvider
//! toSession`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - `isExpired({ token: {}, created_at: 0 }, ...)` é o [`ExpiryEntry`] com `expires_in` ausente (`None`) e o `now` explícito em `Some`.
//! - `Number.MAX_SAFE_INTEGER` é `9_007_199_254_740_991.0`.
//! - `assert.deepStrictEqual(session.account, { id, label })` compara com a [`AuthenticationSessionAccount`] sem ícone.
//! - `jwt(claims)` recebe o JSON dos claims já escrito (o `JSON.stringify` preserva a ordem das chaves, que é a escrita aqui) e
//!   `encodeBase64(VSBuffer.fromString(...))` é `encode_base64` com preenchimento e alfabeto padrão.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::buffer::{encode_base64, VSBuffer};
use je_base::oauth::IAuthorizationTokenResponse;
use je_workbench::api::common::ext_host_xaa_auth_provider::{cache_key, is_expired, to_session, ExpiryEntry, IDP_SCOPES};
use je_workbench::services::authentication::authentication::AuthenticationSessionAccount;

/// Minimal unsigned JWT: `getClaimsFromJWT` only base64-decodes the header/payload segments.
fn jwt(claims: &str) -> String {
    let segment = |value: &str| encode_base64(&VSBuffer::from_string(value), true, false);
    format!("{}.{}.sig", segment(r#"{"alg":"none"}"#), segment(claims))
}

/// `{ access_token, token_type: 'Bearer', id_token? }`.
fn token(access_token: String, id_token: Option<String>) -> IAuthorizationTokenResponse {
    IAuthorizationTokenResponse {
        access_token,
        token_type: "Bearer".to_owned(),
        expires_in: None,
        refresh_token: None,
        scope: None,
        id_token,
        extra: Default::default(),
    }
}

// suite('XaaAuthProvider helpers')

#[test]
fn cache_key_is_scope_order_independent() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(cache_key("https://r.example.com", &["b", "a"]), cache_key("https://r.example.com", &["a", "b"]));
}

#[test]
fn cache_key_distinguishes_different_audiences() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_ne!(cache_key("https://r1.example.com", &["s"]), cache_key("https://r2.example.com", &["s"]));
}

#[test]
fn is_expired_treats_tokens_without_expires_in_as_never_expiring() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert!(!is_expired(ExpiryEntry { expires_in: None, created_at: 0.0 }, Some(9_007_199_254_740_991.0)));
}

#[test]
fn is_expired_flags_tokens_within_60s_of_expiry_as_expired() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let created_at = 1_000_000.0;
    let expires_in = 3600.0;
    let entry = ExpiryEntry { expires_in: Some(expires_in), created_at };
    // 60s before nominal expiry -> already expired due to safety margin
    let just_inside_margin = created_at + (expires_in * 1000.0) - 30_000.0;
    assert!(is_expired(entry, Some(just_inside_margin)));
    // well before expiry
    let earlier = created_at + 1000.0;
    assert!(!is_expired(entry, Some(earlier)));
}

#[test]
fn is_expired_treats_expires_in_zero_as_immediately_expired() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    // Distinguish from `expires_in === undefined` (which means "never"); zero must mean "already expired" so a malformed/edge-case AS
    // response can't be served from cache.
    assert!(is_expired(ExpiryEntry { expires_in: Some(0.0), created_at: 1_000_000.0 }, Some(1_000_000.0)));
}

#[test]
fn idp_scopes_requests_an_openid_session_with_refresh() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let mut scopes = IDP_SCOPES.to_vec();
    scopes.sort();
    assert_eq!(scopes, vec!["offline_access", "openid"]);
}

// suite('XaaAuthProvider toSession')

#[test]
fn prefers_the_identity_in_the_sessions_own_id_token_over_the_fallback_account() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    // The token passed to toSession is the response for THIS session, so its id_token is the authoritative identity. The fallback
    // account comes from a different (IdP) token response and must only fill in when this token carries no id_token of its own.
    let token = token("opaque".to_owned(), Some(jwt(r#"{"sub":"session-sub","preferred_username":"session@contoso.com"}"#)));
    let session = to_session(&token, &["mcp:proxy".to_owned()], Some(&AuthenticationSessionAccount::new("idp@contoso.com", "idp-sub")));
    assert_eq!(session.account, AuthenticationSessionAccount::new("session@contoso.com", "session-sub"));
}

#[test]
fn falls_back_to_the_threaded_account_when_the_token_has_no_id_token() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    // The common resource-session case: the resource token carries no id_token, so the session adopts the IdP identity, keeping it
    // aligned with account enumeration (getAccounts).
    let token = token("opaque-resource-token".to_owned(), None);
    let session = to_session(&token, &["mcp:proxy".to_owned()], Some(&AuthenticationSessionAccount::new("user@contoso.com", "idp-sub")));
    assert_eq!(session.account, AuthenticationSessionAccount::new("user@contoso.com", "idp-sub"));
}

#[test]
fn never_mines_identity_from_the_access_token() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    // The access token is a bearer credential for the resource server, opaque to us by design. Even when it is a JWT with identity
    // claims and there is neither an id_token nor a fallback, we must not read it.
    let token = token(jwt(r#"{"sub":"resource-sub","preferred_username":"do-not-use"}"#), None);
    let session = to_session(&token, &["mcp:proxy".to_owned()], None);
    assert_eq!(session.account, AuthenticationSessionAccount::new("XAA", "unknown"));
}
