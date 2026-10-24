//! Porte parcial de `src/vs/workbench/api/common/extHostAuthentication.ts`: `reviveAccountIcon`, o tipo `IAuthorizationToken`, o
//! `TokenStore` e a classe `ExtHostAuthentication` (a parte que não é dinâmica).
//!
//! Portado de `ExtHostAuthentication`: `getExtensionScopedSessionsEvent`, `getSession`, `getAccounts`, `registerAuthenticationProvider`
//! e os métodos do `ExtHostAuthenticationShape` (`$createSession`, `$removeSession`, `$getSessions`, `$getSessionsFromChallenges`,
//! `$createSessionFromChallenges`, `$onDidChangeAuthenticationSessions`, `$onDidUnregisterAuthenticationProvider`), com o `TaskSingler`
//! e o `SequencerByKey`.
//!
//! Ainda sem porte (o recorte): `DynamicAuthProvider` (com o `XaaifyAuthProvider` de `extHostXaaAuthProvider.ts`, o `IExtHostWindow`, o
//! `IExtHostUrlsService` e o `IExtHostProgress`) e, com ele, `$registerDynamicAuthProvider`, `$registerXaaAuthProvider` e
//! `$onDidChangeDynamicAuthProviderTokens` (e os campos `_dynamicAuthProviderCtor`, `_xaaAuthProviderCtor` e
//! `_onDidDynamicAuthProviderTokensChange`), porque dependem de `fetchDynamicRegistration` e do `fetch` do `oauth.ts` (não portados, ver
//! `je_base::oauth`), de `URL` e de `Response`. Pela mesma razão os três casos da suíte `DynamicAuthProvider` de
//! `extHostAuthentication.test.ts` ficam de fora (ver `tests/ext_host_authentication.rs`). O `IExtHostRpcService`, o
//! `IExtHostInitDataService` e o `ILoggerService` do construtor reduzem-se ao contexto do RPC e ao serviço de log.
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
//! - `vscode.AuthenticationProvider` é o trait [`AuthenticationProvider`]; os métodos opcionais `getSessionsFromChallenges` e
//!   `createSessionFromChallenges` (o `typeof provider.x === 'function'` do original) devolvem `None` quando o provedor não os tem.
//!   Os métodos devolvem [`MaybePromise`] (o `Thenable` do original).
//! - `getSession` recebe a [`AuthenticationGetSessionOptions`] do protocolo (o `Dto`) e `requestingExtension` como a
//!   [`ExtensionDescription`]; o `URI.revive` das opções e do `account.icon` em `$getSessions`, `$createSession` e nas variantes de
//!   desafio é a identidade (as opções já chegam com `URI`, ver `ext_host_protocol.rs`), então o original não tem equivalente aqui.
//!   A sessão que volta da thread principal é revivida com `revive_authentication_session`.
//! - As chaves de `options` que o `getAuthenticationSessionRequestKey` lê além das conhecidas (`clearSessionPreference`, `createIfNone`
//!   e `forceNewSession`, já com `getInteractiveOptionsForRequestKey` aplicado) entram em `extra` ([`IAuthenticationProviderSessionOptions`]).
//! - `registerAuthenticationProvider` devolve um [`Disposable`] da API. O `this._logService.error(msg, error)` é `error_args` com o
//!   erro como argumento.
//! - `private _onDidChangeSessions = new Emitter<vscode.AuthenticationSessionsChangeEvent & { extensionIdFilter }>`: o payload é a
//!   struct `SessionsChangeNotification`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::async_::{async_fn, await_maybe, SequencerByKey};
use je_base::dynamic::{json_stringify, Dynamic};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::hash::string_hash;
use je_base::js_string::JsString;
use je_base::json::Value;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::oauth::{get_claims_from_jwt, IAuthorizationTokenResponse};
use je_base::observable::base::{IObservable, ISettableObservable, Observable, SettableExt};
use je_base::observable::{autorun, derived_opts, observable_value, DerivedOptions};
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::{UriComponents, URI};
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::{LogArg, LogMessage, LogServiceRef, Logger, LoggerRef};

use crate::api::browser::main_thread_authentication::revive_authentication_session;
use crate::api::common::ext_host_protocol::{
    AuthenticationGetSessionOptions, AuthenticationInteractive, AuthenticationSessionAccountDto, AuthenticationSessionDto, AuthenticationSessionsChangeEventDto,
    ExtHostAuthenticationShape, IMainContext, IRegisterAuthenticationProviderDetails, MainContext, MainThreadAuthenticationShape,
};
use crate::api::common::ext_host_types::Disposable;
use crate::services::authentication::authentication::{
    get_authentication_session_request_key, AuthenticationProviderInformation, AuthenticationScopeListOrRequest, AuthenticationSession, AuthenticationSessionAccount,
    AuthenticationSessionsChangeEvent, IAuthenticationConstraint, IAuthenticationCreateSessionOptions, IAuthenticationGetSessionsOptions,
    IAuthenticationProviderSessionOptions, INTERNAL_AUTH_PROVIDER_PREFIX,
};

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
pub(crate) fn non_empty_claim(claims: &Option<Value>, name: &str) -> Option<String> {
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

// ---- ExtHostAuthentication

/// `vscode.AuthenticationProviderOptions` (com `supportedAuthorizationServers` e `supportsChallenges`, que o original só aceita com as
/// propostas `authIssuers` e `authenticationChallenges`).
#[derive(Clone, Default)]
pub struct AuthenticationProviderOptions {
    pub supports_multiple_accounts: Option<bool>,
    pub supported_authorization_servers: Option<Vec<URI>>,
    pub supports_challenges: Option<bool>,
}

/// `vscode.AuthenticationProvider`.
pub trait AuthenticationProvider {
    /// `onDidChangeSessions`: assinar devolve o `Disposable` da assinatura.
    fn on_did_change_sessions(&self) -> Event<AuthenticationSessionsChangeEvent>;

    fn get_sessions(&self, scopes: Option<&[String]>, options: &IAuthenticationProviderSessionOptions) -> MaybePromise<Vec<AuthenticationSession>>;

    fn create_session(&self, scopes: &[String], options: &IAuthenticationCreateSessionOptions) -> MaybePromise<AuthenticationSession>;

    fn remove_session(&self, session_id: &str) -> MaybePromise<()>;

    /// `getSessionsFromChallenges`, ou `None` se o provedor não o tem.
    fn get_sessions_from_challenges(
        &self,
        _constraint: &IAuthenticationConstraint,
        _options: &IAuthenticationProviderSessionOptions,
    ) -> Option<MaybePromise<Vec<AuthenticationSession>>> {
        None
    }

    /// `createSessionFromChallenges`, ou `None` se o provedor não o tem.
    fn create_session_from_challenges(
        &self,
        _constraint: &IAuthenticationConstraint,
        _options: &IAuthenticationCreateSessionOptions,
    ) -> Option<MaybePromise<AuthenticationSession>> {
        None
    }
}

/// `ProviderWithMetadata`.
pub struct ProviderWithMetadata {
    pub label: String,
    pub provider: Rc<dyn AuthenticationProvider>,
    pub disposable: Option<Rc<DisposableStore>>,
    pub options: AuthenticationProviderOptions,
}

/// O payload de `_onDidChangeSessions`: `vscode.AuthenticationSessionsChangeEvent & { extensionIdFilter?: string[] }`.
#[derive(Clone)]
struct SessionsChangeNotification {
    provider: AuthenticationProviderInformation,
    extension_id_filter: Option<Vec<String>>,
}

/// `TaskSingler`: quem pede a mesma chave enquanto a tarefa roda recebe a mesma promessa.
struct TaskSingler<T: Clone + 'static> {
    in_flight_promises: Rc<RefCell<HashMap<String, Promise<T>>>>,
}

impl<T: Clone + 'static> TaskSingler<T> {
    fn new() -> Self {
        Self { in_flight_promises: Rc::new(RefCell::new(HashMap::new())) }
    }

    fn get_or_create(&self, key: String, promise_factory: impl FnOnce() -> Promise<T>) -> Promise<T> {
        let in_flight = self.in_flight_promises.borrow().get(&key).cloned();
        if let Some(in_flight) = in_flight {
            return in_flight;
        }

        let (promises, finished_key) = (self.in_flight_promises.clone(), key.clone());
        let promise = promise_factory().finally(move || {
            promises.borrow_mut().remove(&finished_key);
        });
        self.in_flight_promises.borrow_mut().insert(key, promise.clone());
        promise
    }
}

/// `UriComponents` de uma [`URI`] (o `toJSON` sem o estado em cache).
pub fn uri_components(uri: &URI) -> UriComponents {
    uri.to_json().components
}

/// `Dto<AuthenticationSession>`: o inverso de `reviveAuthenticationSession`.
pub fn authentication_session_to_dto(session: &AuthenticationSession) -> AuthenticationSessionDto {
    AuthenticationSessionDto {
        id: session.id.clone(),
        access_token: session.access_token.clone(),
        account: AuthenticationSessionAccountDto {
            label: session.account.label.clone(),
            id: session.account.id.clone(),
            icon: session.account.icon.as_ref().map(uri_components),
        },
        scopes: session.scopes.clone(),
        id_token: session.id_token.clone(),
        expires_after: session.expires_after,
        authorization_server: session.authorization_server.as_ref().map(uri_components),
    }
}

/// `Dto<AuthenticationSessionsChangeEvent>`.
fn change_event_to_dto(event: &AuthenticationSessionsChangeEvent) -> AuthenticationSessionsChangeEventDto {
    let to_dtos = |sessions: &Option<Vec<AuthenticationSession>>| sessions.as_ref().map(|sessions| sessions.iter().map(authentication_session_to_dto).collect());
    AuthenticationSessionsChangeEventDto { added: to_dtos(&event.added), removed: to_dtos(&event.removed), changed: to_dtos(&event.changed) }
}

/// `getInteractiveOptionsForRequestKey`: `{ detail, learnMore: learnMore?.toString() }` ou o booleano.
fn interactive_options_for_request_key(options: &Option<AuthenticationInteractive>) -> Dynamic {
    match options {
        None => Dynamic::Undefined,
        Some(AuthenticationInteractive::Flag(flag)) => Dynamic::Bool(*flag),
        Some(AuthenticationInteractive::Options(options)) => Dynamic::object([
            ("detail", options.detail.as_deref().map_or(Dynamic::Undefined, Dynamic::string)),
            (
                "learnMore",
                options.learn_more.as_ref().map_or(Dynamic::Undefined, |components| Dynamic::String(URI::from(components, false).to_string(false))),
            ),
        ]),
    }
}

/// O segundo argumento do `getAuthenticationSessionRequestKey` de `getSession`: `{ ...options, account: { id, label },
/// authorizationServer: URI.revive(...), createIfNone: ..., forceNewSession: ... }`.
fn request_key_options(options: &AuthenticationGetSessionOptions) -> IAuthenticationProviderSessionOptions {
    let mut extra: Vec<(JsString, Dynamic)> = match &options.extra {
        Some(Dynamic::Object(object)) => object.entries(),
        _ => Vec::new(),
    };
    extra.push((JsString::from("clearSessionPreference"), options.clear_session_preference.map_or(Dynamic::Undefined, Dynamic::Bool)));
    extra.push((JsString::from("createIfNone"), interactive_options_for_request_key(&options.create_if_none)));
    extra.push((JsString::from("forceNewSession"), interactive_options_for_request_key(&options.force_new_session)));
    IAuthenticationProviderSessionOptions {
        silent: options.silent,
        account: options.account.as_ref().map(revive_account_icon),
        authorization_server: options.authorization_server.as_ref().map(|components| URI::from(components, false)),
        client_id: options.client_id.clone(),
        resource: options.resource.clone(),
        audience: options.audience.clone(),
        extra: Some(Dynamic::object(extra)),
    }
}

struct Shared {
    proxy: Rc<dyn MainThreadAuthenticationShape>,
    log_service: LogServiceRef,
    authentication_providers: RefCell<HashMap<String, Rc<ProviderWithMetadata>>>,
    provider_operations: SequencerByKey<String>,
    on_did_change_sessions: Rc<Emitter<SessionsChangeNotification>>,
    get_session_task_singler: TaskSingler<Option<AuthenticationSession>>,
}

impl Shared {
    fn provider_data(&self, provider_id: &str) -> Result<Rc<ProviderWithMetadata>, Error> {
        self.authentication_providers
            .borrow()
            .get(provider_id)
            .cloned()
            .ok_or_else(|| Error::new(format!("Unable to find authentication provider with handle: {provider_id}")))
    }

    /// O corpo comum de `$createSession`, `$removeSession`, `$getSessions` e das variantes de desafio: `this._providerOperations.queue(
    /// providerId, async () => { const providerData = ...; if (providerData) { return await ... } throw new Error(...) })`.
    fn with_provider<T: Clone + 'static>(
        self: &Rc<Self>,
        provider_id: &str,
        operation: impl FnOnce(Rc<dyn AuthenticationProvider>) -> Result<MaybePromise<T>, Error> + 'static,
    ) -> Promise<T> {
        let (shared, provider_id) = (self.clone(), provider_id.to_owned());
        self.provider_operations.queue(provider_id.clone(), move || {
            async_fn(async move {
                let provider = shared.provider_data(&provider_id)?.provider.clone();
                await_maybe(operation(provider)?).await
            })
        })
    }
}

/// `ExtHostAuthentication`.
pub struct ExtHostAuthentication {
    shared: Rc<Shared>,
}

impl ExtHostAuthentication {
    /// `new ExtHostAuthentication(extHostRpc, ..., logService)`.
    pub fn new(main_context: &dyn IMainContext, log_service: LogServiceRef) -> Rc<Self> {
        Rc::new(Self {
            shared: Rc::new(Shared {
                proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_AUTHENTICATION),
                log_service,
                authentication_providers: RefCell::new(HashMap::new()),
                provider_operations: SequencerByKey::new(),
                on_did_change_sessions: Emitter::new(),
                get_session_task_singler: TaskSingler::new(),
            }),
        })
    }

    /// This sets up an event that will fire when the auth sessions change with a built-in filter for the extensionId if a session change
    /// only affects a specific extension.
    pub fn get_extension_scoped_sessions_event(&self, extension_id: &str) -> Event<AuthenticationProviderInformation> {
        let normalized_extension_id = extension_id.to_lowercase();
        self.shared.on_did_change_sessions.event().chain(move |chain| {
            let normalized_extension_id = normalized_extension_id.clone();
            chain
                .filter(move |notification: &SessionsChangeNotification| {
                    notification.extension_id_filter.as_ref().is_none_or(|filter| filter.contains(&normalized_extension_id))
                })
                .map(|notification| notification.provider)
        })
    }

    pub fn get_session(
        &self,
        requesting_extension: &ExtensionDescription,
        provider_id: &str,
        scopes_or_request: AuthenticationScopeListOrRequest,
        options: AuthenticationGetSessionOptions,
    ) -> Promise<Option<AuthenticationSession>> {
        let extension_id = ExtensionIdentifier::to_key(&requesting_extension.identifier.value);
        let extension_name = requesting_extension
            .manifest
            .display_name
            .clone()
            .filter(|display_name| !display_name.is_empty())
            .unwrap_or_else(|| requesting_extension.manifest.name.clone());
        let (shared, provider_id) = (self.shared.clone(), provider_id.to_owned());
        async_fn(async move {
            let request_key = get_authentication_session_request_key(&scopes_or_request, &request_key_options(&options));
            let singler_key =
                json_stringify(&Dynamic::array([Dynamic::string(&extension_id), Dynamic::string(&provider_id), Dynamic::string(&request_key)])).unwrap_or_default();

            let task_shared = shared.clone();
            shared
                .get_session_task_singler
                .get_or_create(singler_key, move || {
                    async_fn(async move {
                        task_shared.proxy.rpc_ensure_provider(&provider_id).await?;
                        let session = task_shared.proxy.rpc_get_session(&provider_id, scopes_or_request, &extension_id, &extension_name, options).await?;
                        Ok(session.map(|session| revive_authentication_session(&session)))
                    })
                })
                .await
        })
    }

    pub fn get_accounts(&self, provider_id: &str) -> Promise<Vec<AuthenticationSessionAccount>> {
        let (shared, provider_id) = (self.shared.clone(), provider_id.to_owned());
        async_fn(async move {
            shared.proxy.rpc_ensure_provider(&provider_id).await?;
            let accounts = shared.proxy.rpc_get_accounts(&provider_id).await?;
            Ok(accounts.iter().map(revive_account_icon).collect())
        })
    }

    pub fn register_authentication_provider(
        &self,
        id: &str,
        label: &str,
        provider: Rc<dyn AuthenticationProvider>,
        options: Option<AuthenticationProviderOptions>,
    ) -> DisposableRef {
        let disposables = DisposableStore::new();
        // Capture changes before queueing, but forward them only after the main thread acknowledges registration.
        let buffered_session_changes = provider.on_did_change_sessions().buffer("authentication provider registration", false, Vec::new(), Some(&disposables));
        let options = options.unwrap_or_default();
        let details = IRegisterAuthenticationProviderDetails {
            id: id.to_owned(),
            label: label.to_owned(),
            supports_multiple_accounts: options.supports_multiple_accounts.unwrap_or(false),
            supported_authorization_servers: options.supported_authorization_servers.as_ref().map(|servers| servers.iter().map(uri_components).collect()),
            supports_challenges: options.supports_challenges,
            resource_server: None,
        };
        let provider_data = Rc::new(ProviderWithMetadata { label: label.to_owned(), provider, disposable: Some(disposables.clone()), options });
        let id = id.to_owned();

        // register
        let (shared, register_id, register_data) = (self.shared.clone(), id.clone(), provider_data.clone());
        let _ = self.shared.provider_operations.queue(id.clone(), move || {
            async_fn(async move {
                // This use to be synchronous, but that wasn't an accurate representation because the main thread may have unregistered
                // the provider in the meantime. I don't see how this could really be done synchronously, so we just say first one wins.
                let already_registered = shared.authentication_providers.borrow().contains_key(&register_id);
                if already_registered {
                    disposables.dispose();
                    shared.log_service.error(&format!("An authentication provider with id '{register_id}' is already registered. The existing provider will not be replaced."));
                    return Ok(());
                }
                shared.authentication_providers.borrow_mut().insert(register_id.clone(), register_data);
                match shared.proxy.rpc_register_authentication_provider(details).await {
                    Ok(()) => {
                        let (proxy, changed_id) = (shared.proxy.clone(), register_id);
                        disposables.add(buffered_session_changes.listen(move |event| {
                            let _ = proxy.rpc_send_did_change_sessions(&changed_id, change_event_to_dto(event));
                        }));
                    }
                    Err(error) => {
                        disposables.dispose();
                        shared.authentication_providers.borrow_mut().remove(&register_id);
                        shared.log_service.error_args(&LogMessage::from(format!("Failed to register authentication provider '{register_id}'.")), &[LogArg::Error(error)]);
                    }
                }
                Ok(())
            })
        });

        // unregister
        let shared = self.shared.clone();
        let unregister: DisposableRef = Rc::new(Disposable::new(move || {
            let (shared, task_shared, id, provider_data) = (shared.clone(), shared.clone(), id.clone(), provider_data.clone());
            let _ = shared.provider_operations.queue(id.clone(), move || {
                async_fn(async move {
                    let current = task_shared.authentication_providers.borrow().get(&id).cloned();
                    if current.is_some_and(|current| Rc::ptr_eq(&current, &provider_data)) {
                        if let Some(disposables) = &provider_data.disposable {
                            disposables.dispose();
                        }
                        task_shared.authentication_providers.borrow_mut().remove(&id);
                        task_shared.proxy.rpc_unregister_authentication_provider(&id).await?;
                    }
                    Ok(())
                })
            });
        }));
        unregister
    }
}

impl ExtHostAuthenticationShape for ExtHostAuthentication {
    fn rpc_get_sessions(&self, id: &str, scopes: Option<Vec<String>>, options: IAuthenticationGetSessionsOptions) -> Promise<Vec<AuthenticationSession>> {
        self.shared.with_provider(id, move |provider| Ok(provider.get_sessions(scopes.as_deref(), &options)))
    }

    fn rpc_create_session(&self, id: &str, scopes: Vec<String>, options: IAuthenticationCreateSessionOptions) -> Promise<AuthenticationSession> {
        self.shared.with_provider(id, move |provider| Ok(provider.create_session(&scopes, &options)))
    }

    fn rpc_get_sessions_from_challenges(
        &self,
        id: &str,
        constraint: IAuthenticationConstraint,
        options: IAuthenticationGetSessionsOptions,
    ) -> Promise<Vec<AuthenticationSession>> {
        let provider_id = id.to_owned();
        self.shared.with_provider(id, move |provider| {
            // Check if provider supports challenges
            provider
                .get_sessions_from_challenges(&constraint, &options)
                .ok_or_else(|| Error::new(format!("Authentication provider with handle: {provider_id} does not support getSessionsFromChallenges")))
        })
    }

    fn rpc_create_session_from_challenges(&self, id: &str, constraint: IAuthenticationConstraint, options: IAuthenticationCreateSessionOptions) -> Promise<AuthenticationSession> {
        let provider_id = id.to_owned();
        self.shared.with_provider(id, move |provider| {
            // Check if provider supports challenges
            provider
                .create_session_from_challenges(&constraint, &options)
                .ok_or_else(|| Error::new(format!("Authentication provider with handle: {provider_id} does not support createSessionFromChallenges")))
        })
    }

    fn rpc_remove_session(&self, id: &str, session_id: &str) -> Promise<()> {
        let session_id = session_id.to_owned();
        self.shared.with_provider(id, move |provider| Ok(provider.remove_session(&session_id)))
    }

    fn rpc_on_did_change_authentication_sessions(&self, id: &str, label: &str, extension_id_filter: Option<Vec<String>>) -> Promise<()> {
        // Don't fire events for the internal auth providers
        if !id.starts_with(INTERNAL_AUTH_PROVIDER_PREFIX) {
            let provider = AuthenticationProviderInformation { id: id.to_owned(), label: label.to_owned(), ..AuthenticationProviderInformation::default() };
            self.shared.on_did_change_sessions.fire(SessionsChangeNotification { provider, extension_id_filter });
        }
        Promise::resolved(())
    }

    fn rpc_on_did_unregister_authentication_provider(&self, id: &str) -> Promise<()> {
        let (shared, id) = (self.shared.clone(), id.to_owned());
        self.shared.provider_operations.queue(id.clone(), move || {
            async_fn(async move {
                let provider_data = shared.authentication_providers.borrow().get(&id).cloned();
                if let Some(provider_data) = provider_data {
                    if let Some(disposables) = &provider_data.disposable {
                        disposables.dispose();
                    }
                    shared.authentication_providers.borrow_mut().remove(&id);
                }
                Ok(())
            })
        })
    }
}
