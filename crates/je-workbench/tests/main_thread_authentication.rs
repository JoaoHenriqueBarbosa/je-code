//! Porte de `src/vs/workbench/api/test/browser/mainThreadAuthentication.test.ts` (suíte `MainThreadAuthentication`), caso a caso e na mesma
//! ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo do caso.
//! - `Dto<AuthenticationSession>` é o [`AuthenticationSessionDto`] (os `UriComponents` no lugar das `URI`); `authorizationServer.toJSON()` é
//!   `to_json().components` da `URI`. O `{ ...withIcon, account: { ...withIcon.account, icon: URI.from(iconComponents) }, authorizationServer }`
//!   do esperado é a sessão montada campo a campo, e o `icon: undefined` e o `authorizationServer: undefined` são `None`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::js_string::JsString;
use je_base::uri::{UriComponents, URI};
use je_workbench::api::browser::main_thread_authentication::revive_authentication_session;
use je_workbench::api::common::ext_host_protocol::{AuthenticationSessionAccountDto, AuthenticationSessionDto};
use je_workbench::services::authentication::authentication::{AuthenticationSession, AuthenticationSessionAccount};

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
fn revives_session_uris_and_preserves_optional_session_fields() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let authorization_server = URI::parse("https://enterprise.example.com/login/oauth", false);
    let with_icon = AuthenticationSessionDto {
        id: "session-with-icon".to_owned(),
        access_token: "token".to_owned(),
        scopes: vec!["scope".to_owned()],
        account: AuthenticationSessionAccountDto { id: "account-with-icon".to_owned(), label: "Has Icon".to_owned(), icon: Some(icon_components()) },
        authorization_server: Some(authorization_server.to_json().components),
        id_token: Some("id-token".to_owned()),
        expires_after: Some(3600.0),
    };
    let without_icon = AuthenticationSessionDto {
        id: "session-without-icon".to_owned(),
        access_token: "token".to_owned(),
        scopes: vec!["scope".to_owned()],
        account: AuthenticationSessionAccountDto { id: "account-without-icon".to_owned(), label: "No Icon".to_owned(), icon: None },
        authorization_server: None,
        id_token: None,
        expires_after: None,
    };

    assert_eq!(
        vec![revive_authentication_session(&with_icon), revive_authentication_session(&without_icon)],
        vec![
            AuthenticationSession {
                id: "session-with-icon".to_owned(),
                access_token: "token".to_owned(),
                scopes: vec!["scope".to_owned()],
                account: AuthenticationSessionAccount {
                    id: "account-with-icon".to_owned(),
                    label: "Has Icon".to_owned(),
                    icon: Some(URI::from(&icon_components(), false)),
                },
                authorization_server: Some(authorization_server),
                id_token: Some("id-token".to_owned()),
                expires_after: Some(3600.0),
            },
            AuthenticationSession {
                id: "session-without-icon".to_owned(),
                access_token: "token".to_owned(),
                scopes: vec!["scope".to_owned()],
                account: AuthenticationSessionAccount { id: "account-without-icon".to_owned(), label: "No Icon".to_owned(), icon: None },
                authorization_server: None,
                id_token: None,
                expires_after: None,
            },
        ]
    );
}
