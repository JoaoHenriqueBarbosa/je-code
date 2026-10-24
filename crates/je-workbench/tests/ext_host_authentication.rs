//! Porte de `src/vs/workbench/api/test/browser/extHostAuthentication.test.ts` (suítes `TokenStore`, `DynamicAuthProvider` e `Account Icon
//! Revival`), caso a caso e na mesma ordem.
//!
//! Casos de fora:
//! - Os três casos da suíte `DynamicAuthProvider` (`does not rotate the client while silently refreshing a token`, `refresh sends scope
//!   without resource for Microsoft auth, and resource without scope otherwise` e `authorization URL requests offline_access when
//!   advertised and omits resource for Microsoft auth`): exigem o `DynamicAuthProvider` (e com ele o `fetchDynamicRegistration` e o
//!   `fetch` de `oauth.ts`, o `Response`, o `URL`, o `IExtHostWindow`, o `IExtHostUrlsService`, o `IExtHostProgress` com
//!   `withProgressFromSource` e o `MainThreadAuthenticationShape`), que ainda não têm porte (ver o cabeçalho de
//!   `api/common/ext_host_authentication.rs`). Não viram `#[test]` até existirem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso (`disposables` do original é o guarda).
//! - `jwt(claims)` recebe o JSON dos claims já escrito (o `JSON.stringify` do original preserva a ordem das chaves, que é a escrita aqui);
//!   `encodeBase64(VSBuffer.fromString(...))` é `encode_base64` com preenchimento e alfabeto padrão.
//! - `new NullLogger()` é o `NullLogger` da plataforma, descartado com o guarda.
//! - `persistence = { onDidChange: emitter.event, set: () => { } }` é o [`TokenStorePersistence`] com o `set` vazio.
//! - Os objetos de teste de `IAuthorizationToken` omitem `scope` (`undefined`) com `None`; o `{ ...withIcon, icon: URI.from(...) }` é a
//!   conta revivida montada campo a campo.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::buffer::{encode_base64, VSBuffer};
use je_base::event::Emitter;
use je_base::js_string::JsString;
use je_base::oauth::IAuthorizationTokenResponse;
use je_base::uri::{UriComponents, URI};
use je_platform::log::{LoggerRef, NullLogger};
use je_workbench::api::common::ext_host_authentication::{revive_account_icon, IAuthorizationToken, TokenStore, TokenStorePersistence};
use je_workbench::api::common::ext_host_protocol::AuthenticationSessionAccountDto;
use je_workbench::services::authentication::authentication::AuthenticationSessionAccount;

/// Builds a structurally-valid JWT carrying the given claims.
fn jwt(claims: &str) -> String {
    let segment = |value: &str| encode_base64(&VSBuffer::from_string(value), true, false);
    format!("{}.{}.signature", segment(r#"{"alg":"none","typ":"JWT"}"#), segment(claims))
}

/// `{ access_token, token_type: 'Bearer', scope?, created_at: 0 }`.
fn token(access_token: String, scope: Option<&str>) -> IAuthorizationToken {
    IAuthorizationToken {
        response: IAuthorizationTokenResponse {
            access_token,
            token_type: "Bearer".to_owned(),
            expires_in: None,
            refresh_token: None,
            scope: scope.map(str::to_owned),
            id_token: None,
            extra: Default::default(),
        },
        created_at: 0.0,
    }
}

/// `{ scheme: 'https', authority: 'example.com', path: '/avatar.png', query: '', fragment: '' }`.
fn icon_components() -> UriComponents {
    UriComponents {
        scheme: Some(JsString::from("https")),
        authority: Some(JsString::from("example.com")),
        path: Some(JsString::from("/avatar.png")),
        query: Some(JsString::from("")),
        fragment: Some(JsString::from("")),
    }
}

#[test]
fn derives_session_scopes_from_the_stored_token_scope_falling_back_to_jwt_claims_only_when_scope_is_absent() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();

    let on_did_change = disposables.add_rc(Emitter::new());
    let null_logger = Rc::new(NullLogger::new());
    disposables.add_rc(null_logger.clone());
    let logger: LoggerRef = null_logger;
    let persistence = TokenStorePersistence { on_did_change: on_did_change.event(), set: Box::new(|_| {}) };
    let store = disposables.add_rc(TokenStore::new(
        persistence,
        vec![
            // Explicit empty scope must win over the scopes embedded in the JWT claims.
            token(jwt(r#"{"sub":"a","scope":"menu:read orders:create orders:cancel"}"#), Some("")),
            // Absent scope (undefined) falls back to the JWT claims.
            token(jwt(r#"{"sub":"b","scope":"menu:read orders:create"}"#), None),
            // A non-empty scope is authoritative over the JWT claims.
            token(jwt(r#"{"sub":"c","scope":"ignored:claim"}"#), Some("read write")),
        ],
        logger,
    ));

    let scopes: Vec<Vec<String>> = store.sessions().into_iter().map(|session| session.scopes).collect();
    let expected: Vec<Vec<String>> = vec![vec![], vec!["menu:read".to_owned(), "orders:create".to_owned()], vec!["read".to_owned(), "write".to_owned()]];
    assert_eq!(scopes, expected);
}

#[test]
fn revive_account_icon_revives_a_present_icon_into_a_uri_and_leaves_a_missing_icon_undefined() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let with_icon = AuthenticationSessionAccountDto { id: "account-with-icon".to_owned(), label: "Has Icon".to_owned(), icon: Some(icon_components()) };
    let without_icon = AuthenticationSessionAccountDto { id: "account-without-icon".to_owned(), label: "No Icon".to_owned(), icon: None };

    assert_eq!(
        vec![revive_account_icon(&with_icon), revive_account_icon(&without_icon)],
        vec![
            AuthenticationSessionAccount { id: "account-with-icon".to_owned(), label: "Has Icon".to_owned(), icon: Some(URI::from(&icon_components(), false)) },
            AuthenticationSessionAccount { id: "account-without-icon".to_owned(), label: "No Icon".to_owned(), icon: None },
        ]
    );
}
