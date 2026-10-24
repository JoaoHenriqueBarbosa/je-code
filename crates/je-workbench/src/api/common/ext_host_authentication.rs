//! Porte parcial de `src/vs/workbench/api/common/extHostAuthentication.ts`: `reviveAccountIcon`, o tipo `IAuthorizationToken` e o
//! `TokenStore`.
//!
//! Ainda sem porte (o resto do arquivo): `ExtHostAuthentication` e `DynamicAuthProvider` (com o `TaskSingler`, o `SequencerByKey`, o
//! `XaaifyAuthProvider` de `extHostXaaAuthProvider.ts`, o `IExtHostWindow`, o `IExtHostUrlsService`, o `IExtHostRpcService`, o
//! `MainThreadAuthenticationShape` e o `ExtHostAuthenticationShape` do protocolo), porque dependem de `fetchDynamicRegistration`
//! e do `fetch` do `oauth.ts` (não portados, ver `je_base::oauth`), de `URL` e de `Response`. Pela mesma razão os três casos da suíte
//! `DynamicAuthProvider` de `extHostAuthentication.test.ts` ficam de fora (ver `tests/ext_host_authentication.rs`).
//!
//! Divergências:
//! - `reviveAccountIcon<T extends { icon?: Uri | UriComponents }>(account: T)` é genérico no original; aqui recebe o
//!   [`AuthenticationSessionAccountDto`] (a forma que chega do RPC) e devolve a [`AuthenticationSessionAccount`] com o ícone revivido.
//! - `IAuthorizationTokenResponse & { created_at }` é o [`IAuthorizationToken`] com a resposta em `response` e `created_at` ao lado.
//! - Os arrays do `ISettableObservable<IAuthorizationToken[]>` e do `IObservable<AuthenticationSession[]>` são `Rc<Vec<_>>`: o `===` do
//!   JavaScript sobre array é identidade, que é a comparação de `Rc` (`previousSessions === currentSessions` é `Rc::ptr_eq`).
//! - `vscode.AuthenticationSession` e o evento `AuthenticationProviderAuthenticationSessionsChangeEvent` são os tipos do serviço de
//!   autenticação do workbench ([`AuthenticationSession`] e [`AuthenticationSessionsChangeEvent`], com `changed: []` como `Some(vec![])`).
//! - `claims?.sub || 'unknown'` e as demais cadeias de `||` tratam a string vazia como ausente ([`non_empty_claim`]).
//! - `stringHash(token.access_token, 0).toString()` é `string_hash` sobre as unidades UTF-16 do token.
//! - `void this._persistence.set(currentTokens)`: o `set` da persistência é um fecho que não devolve nada.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::hash::string_hash;
use je_base::js_string::JsString;
use je_base::json::Value;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::oauth::{get_claims_from_jwt, IAuthorizationTokenResponse};
use je_base::observable::base::{IObservable, ISettableObservable, Observable, SettableExt};
use je_base::observable::{autorun, derived_opts, observable_value, DerivedOptions};
use je_base::uri::URI;
use je_platform::log::LoggerRef;

use crate::api::common::ext_host_protocol::AuthenticationSessionAccountDto;
use crate::services::authentication::authentication::{AuthenticationSession, AuthenticationSessionAccount, AuthenticationSessionsChangeEvent};

/// `reviveAccountIcon`: the account icon is a `vscode.Uri` that does not survive being sent over the RPC boundary, so it needs to be
/// revived when an account is received from the main thread.
pub fn revive_account_icon(account: &AuthenticationSessionAccountDto) -> AuthenticationSessionAccount {
    AuthenticationSessionAccount {
        label: account.label.clone(),
        id: account.id.clone(),
        icon: account.icon.as_ref().map(|components| URI::from(components, false)),
    }
}

/// `IAuthorizationToken`: a resposta do servidor de autorização mais o momento de criação.
#[derive(Clone, Debug, PartialEq)]
pub struct IAuthorizationToken {
    pub response: IAuthorizationTokenResponse,
    /// The time when the token was created, in milliseconds since the epoch.
    pub created_at: f64,
}

/// O `{ onDidChange, set }` de persistência do `TokenStore`.
pub struct TokenStorePersistence {
    pub on_did_change: Event<Vec<IAuthorizationToken>>,
    pub set: Box<dyn Fn(Vec<IAuthorizationToken>)>,
}

type Tokens = Rc<Vec<IAuthorizationToken>>;
type Sessions = Rc<Vec<AuthenticationSession>>;

/// `token[name]` quando é um texto não vazio (o `||` do JavaScript).
fn non_empty_claim(claims: &Option<Value>, name: &str) -> Option<String> {
    let Some(Value::Object(object)) = claims else {
        return None;
    };
    match object.get(name) {
        Some(Value::String(text)) if !text.is_empty() => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// `TokenStore`.
pub struct TokenStore {
    tokens_observable: ISettableObservable<Tokens>,
    sessions_observable: IObservable<Sessions>,
    on_did_change_sessions: Rc<Emitter<AuthenticationSessionsChangeEvent>>,
    persistence: TokenStorePersistence,
    logger: LoggerRef,
    disposable: Rc<DisposableStore>,
}

impl TokenStore {
    pub fn new(persistence: TokenStorePersistence, initial_tokens: Vec<IAuthorizationToken>, logger: LoggerRef) -> Rc<Self> {
        let disposable = DisposableStore::new();
        let tokens_observable: ISettableObservable<Tokens> = observable_value::<Tokens, ()>("tokens", Rc::new(initial_tokens));
        let equals_fn: Rc<dyn Fn(&Sessions, &Sessions) -> bool> =
            Rc::new(|a, b| a.len() == b.len() && a.iter().zip(b.iter()).all(|(a, b)| a.access_token == b.access_token));
        let tokens_for_sessions = tokens_observable.clone();
        let sessions_observable: IObservable<Sessions> = derived_opts(DerivedOptions { equals_fn: Some(equals_fn), ..DerivedOptions::default() }, move |reader| {
            Rc::new(tokens_for_sessions.read(reader).iter().map(|token| Self::get_session_from_token(&token.response)).collect::<Vec<_>>())
        });
        let on_did_change_sessions = Emitter::new();
        let this = Rc::new(Self { tokens_observable, sessions_observable, on_did_change_sessions, persistence, logger, disposable });
        this.disposable.add(this.register_change_event_autorun());
        let tokens_to_update = this.tokens_observable.clone();
        this.disposable.add(this.persistence.on_did_change.listen(move |tokens| tokens_to_update.set(Rc::new(tokens.clone()), None)));
        this
    }

    /// `onDidChangeSessions`.
    pub fn on_did_change_sessions(&self) -> Event<AuthenticationSessionsChangeEvent> {
        self.on_did_change_sessions.event()
    }

    pub fn tokens(&self) -> Vec<IAuthorizationToken> {
        self.tokens_observable.get().as_ref().clone()
    }

    pub fn sessions(&self) -> Vec<AuthenticationSession> {
        self.sessions_observable.get().as_ref().clone()
    }

    pub fn update(&self, added: &[IAuthorizationToken], removed: &[IAuthorizationToken]) {
        self.logger.trace(&format!("Updating tokens: added {}, removed {}", added.len(), removed.len()));
        let mut current_tokens: Vec<IAuthorizationToken> = self.tokens_observable.get().as_ref().clone();
        for token in removed {
            if let Some(index) = current_tokens.iter().position(|t| t.response.access_token == token.response.access_token) {
                current_tokens.remove(index);
            }
        }
        for token in added {
            match current_tokens.iter().position(|t| t.response.access_token == token.response.access_token) {
                None => current_tokens.push(token.clone()),
                Some(index) => current_tokens[index] = token.clone(),
            }
        }
        if !added.is_empty() || !removed.is_empty() {
            self.tokens_observable.set(Rc::new(current_tokens.clone()), None);
            (self.persistence.set)(current_tokens.clone());
        }
        self.logger.trace(&format!("Tokens updated: {} tokens stored.", current_tokens.len()));
    }

    fn register_change_event_autorun(&self) -> je_base::lifecycle::DisposableRef {
        let previous_sessions: RefCell<Sessions> = RefCell::new(Rc::new(Vec::new()));
        let sessions_observable = self.sessions_observable.clone();
        let logger = self.logger.clone();
        let emitter = self.on_did_change_sessions.clone();
        autorun(move |reader| {
            logger.trace("Checking for session changes...");
            let current_sessions = sessions_observable.read(reader);
            if Rc::ptr_eq(&previous_sessions.borrow(), &current_sessions) {
                logger.trace("No session changes detected.");
                return;
            }

            if current_sessions.is_empty() {
                // If currentSessions is undefined, all previous sessions are considered removed
                logger.trace("All sessions removed.");
                let previous = previous_sessions.borrow().clone();
                if !previous.is_empty() {
                    emitter.fire(AuthenticationSessionsChangeEvent { added: Some(Vec::new()), removed: Some(previous.as_ref().clone()), changed: Some(Vec::new()) });
                    *previous_sessions.borrow_mut() = Rc::new(Vec::new());
                }
                return;
            }

            let previous = previous_sessions.borrow().clone();

            // Find added sessions
            let added: Vec<AuthenticationSession> =
                current_sessions.iter().filter(|current| !previous.iter().any(|prev| prev.access_token == current.access_token)).cloned().collect();

            // Find removed sessions
            let removed: Vec<AuthenticationSession> =
                previous.iter().filter(|prev| !current_sessions.iter().any(|current| current.access_token == prev.access_token)).cloned().collect();

            // Fire the event if there are any changes
            if !added.is_empty() || !removed.is_empty() {
                logger.trace(&format!("Sessions changed: added {}, removed {}", added.len(), removed.len()));
                emitter.fire(AuthenticationSessionsChangeEvent { added: Some(added), removed: Some(removed), changed: Some(Vec::new()) });
            }

            // Update previous sessions reference
            *previous_sessions.borrow_mut() = current_sessions;
        })
    }

    fn get_session_from_token(token: &IAuthorizationTokenResponse) -> AuthenticationSession {
        let mut claims: Option<Value> = None;
        if let Some(id_token) = &token.id_token {
            // log
            claims = get_claims_from_jwt(id_token).ok();
        }
        if claims.is_none() {
            // log
            claims = get_claims_from_jwt(&token.access_token).ok();
        }
        // An explicit empty `token.scope` is authoritative (createSession/refresh stamp the requested scopes onto the token); only fall back to the JWT claims when scope is genuinely absent.
        let split = |scope: &str| scope.split(' ').map(str::to_owned).collect::<Vec<_>>();
        let scopes = match &token.scope {
            Some(scope) => {
                if scope.is_empty() {
                    Vec::new()
                } else {
                    split(scope)
                }
            }
            None => non_empty_claim(&claims, "scope").map_or_else(Vec::new, |scope| split(&scope)),
        };
        AuthenticationSession {
            id: string_hash(JsString::from(token.access_token.as_str()).units(), 0).to_string(),
            access_token: token.access_token.clone(),
            account: AuthenticationSessionAccount {
                id: non_empty_claim(&claims, "sub").unwrap_or_else(|| "unknown".to_owned()),
                // O original anota aqui que o rótulo padrão não deveria dizer "MCP"; o valor é o dele.
                label: non_empty_claim(&claims, "preferred_username")
                    .or_else(|| non_empty_claim(&claims, "name"))
                    .or_else(|| non_empty_claim(&claims, "email"))
                    .unwrap_or_else(|| "MCP".to_owned()),
                icon: None,
            },
            scopes,
            id_token: token.id_token.clone(),
            expires_after: None,
            authorization_server: None,
        }
    }
}

impl IDisposable for TokenStore {
    fn dispose(&self) {
        self.disposable.dispose();
    }
}
