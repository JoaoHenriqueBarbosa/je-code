//! `vscode.authentication`: `getSession`, `getAccounts`, `onDidChangeSessions` e `registerAuthenticationProvider`.
//!
//! É a ponte para o `ExtHostAuthentication` portado (`extHostAuthentication.ts`): `getSession` e `getAccounts` falam com o cliente
//! `MainThreadAuthentication` do workbench (`$ensureProvider`, `$getSession`, `$getAccounts`), e `registerAuthenticationProvider` registra
//! o provedor da extensão (`$registerAuthenticationProvider`, `$sendDidChangeSessions`, `$unregisterAuthenticationProvider`); a thread
//! principal pede as sessões ao provedor pelo `ExtHostAuthenticationShape` (`$getSessions`, `$createSession`, `$removeSession`).
//!
//! O provedor da extensão chega por REFERÊNCIA (`registerAuthenticationProvider` é uma função por referência), então os métodos de classe
//! e o `this` dele funcionam; `onDidChangeSessions` do provedor é assinado a cada `Event.buffer` do `ExtHostAuthentication`.
//!
//! Falta: `authentication.hasSession` (a proposta `authSession`, que o host nunca habilita) e, no original, o `DynamicAuthProvider` (ver
//! `je_workbench::api::common::ext_host_authentication`).
//!
//! Divergências:
//! - O `vscode` é um só para todas as extensões: `getSession` pede em nome da extensão `je.exthost`, e as propostas de API nunca estão
//!   habilitadas. As que o `extHost.api.impl.ts` confere (`authLearnMore` em `learnMore`, `authIssuers` em `authorizationServer`,
//!   `clientId`, `resource` e `supportedAuthorizationServers`, `authSessionAudience` em `audience`) lançam o erro do original.
//! - As chaves de `options` que o original repassa sem conhecer (`[key: string]: any`) não atravessam a ponte.
//! - O resultado do provedor passa pelo motor como cópia: as propriedades extras de uma `AuthenticationSession` (além das declaradas em
//!   `vscode.d.ts`) não chegam à thread principal.
//! - `getSession` devolve a sessão como objeto de dados (`id`, `accessToken`, `account`, `scopes`, `idToken`...): copiada, a identidade
//!   que o original devolveria (um objeto novo a cada chamada, por causa do `{ ...session }`) é a mesma coisa.

use std::rc::Rc;

use je_base::errors::{on_unexpected_error, Error};
use je_base::event::{Event, Listener};
use je_base::lifecycle::{plain_disposable, DisposableRef};
use je_base::promise::MaybePromise;
use je_workbench::api::common::ext_host_authentication::{uri_components, AuthenticationProvider, AuthenticationProviderOptions, ExtHostAuthentication};
use je_workbench::api::common::ext_host_protocol::{
    AuthenticationGetSessionOptions, AuthenticationInteractive, AuthenticationInteractiveOptions, AuthenticationSessionAccountDto,
};
use je_workbench::services::authentication::authentication::{
    AuthenticationProviderInformation, AuthenticationScopeListOrRequest, AuthenticationSession, AuthenticationSessionAccount, AuthenticationSessionsChangeEvent,
    IAuthenticationConstraint, IAuthenticationCreateSessionOptions, IAuthenticationProviderSessionOptions, IAuthenticationWwwAuthenticateRequest,
};
use je_workbench::services::extensions::extensions::check_proposed_api_enabled;
use zjsc::api::embed_node::{JsData, JsHandle, NodeRunner};

use crate::api::commands::{convert_maybe, promise_to_js};
use crate::api::disposable::DisposableTable;
use crate::api::events::event_function;
use crate::api::js_object::{JsObjects, JsResult};
use crate::api::languages::host_extension;
use crate::api::HOST_EXTENSION;
use crate::convert::{exported, field, is_nullish, opt_bool, opt_number, opt_string, string_arg, uri_from_js, uri_to_js};

// ---- as sessões e as contas

fn account_of(value: &JsData) -> Result<AuthenticationSessionAccount, String> {
    let label = opt_string(field(value, "label")).ok_or("AuthenticationSession.account.label precisa ser texto")?;
    let id = opt_string(field(value, "id")).ok_or("AuthenticationSession.account.id precisa ser texto")?;
    let icon = match field(value, "icon") {
        Some(icon) if !is_nullish(Some(icon)) => Some(uri_from_js(icon)?),
        _ => None,
    };
    Ok(AuthenticationSessionAccount { label, id, icon })
}

fn account_to_js(account: &AuthenticationSessionAccount) -> JsData {
    let mut fields = vec![("label".to_owned(), JsData::Str(account.label.clone())), ("id".to_owned(), JsData::Str(account.id.clone()))];
    if let Some(icon) = &account.icon {
        fields.push(("icon".to_owned(), uri_to_js(icon)));
    }
    JsData::Object(fields)
}

/// `vscode.AuthenticationSession`.
fn session_of(value: &JsData) -> Result<AuthenticationSession, String> {
    let text = |name: &str| opt_string(field(value, name)).ok_or_else(|| format!("AuthenticationSession.{name} precisa ser texto"));
    let account = field(value, "account").filter(|account| !is_nullish(Some(*account))).ok_or("AuthenticationSession.account é obrigatório")?;
    let scopes = match field(value, "scopes") {
        Some(JsData::Array(items)) => items
            .iter()
            .map(|item| opt_string(Some(item)).ok_or_else(|| "AuthenticationSession.scopes precisa ser uma lista de textos".to_owned()))
            .collect::<Result<Vec<_>, String>>()?,
        _ => return Err("AuthenticationSession.scopes precisa ser uma lista de textos".to_owned()),
    };
    let authorization_server = match field(value, "authorizationServer") {
        Some(uri) if !is_nullish(Some(uri)) => Some(uri_from_js(uri)?),
        _ => None,
    };
    Ok(AuthenticationSession {
        id: text("id")?,
        access_token: text("accessToken")?,
        account: account_of(account)?,
        scopes,
        id_token: opt_string(field(value, "idToken")),
        expires_after: opt_number(field(value, "expiresAfter")),
        authorization_server,
    })
}

fn session_to_js(session: &AuthenticationSession) -> JsData {
    let mut fields = vec![
        ("id".to_owned(), JsData::Str(session.id.clone())),
        ("accessToken".to_owned(), JsData::Str(session.access_token.clone())),
        ("account".to_owned(), account_to_js(&session.account)),
        ("scopes".to_owned(), JsData::Array(session.scopes.iter().cloned().map(JsData::Str).collect())),
    ];
    if let Some(id_token) = &session.id_token {
        fields.push(("idToken".to_owned(), JsData::Str(id_token.clone())));
    }
    if let Some(expires_after) = session.expires_after {
        fields.push(("expiresAfter".to_owned(), JsData::Num(expires_after)));
    }
    if let Some(authorization_server) = &session.authorization_server {
        fields.push(("authorizationServer".to_owned(), uri_to_js(authorization_server)));
    }
    JsData::Object(fields)
}

/// `readonly AuthenticationSession[] | undefined` (o `added`, `removed` ou `changed` de um evento).
fn sessions_field(event: &JsData, name: &str) -> Result<Option<Vec<AuthenticationSession>>, String> {
    match field(event, name) {
        Some(JsData::Array(items)) => items.iter().map(session_of).collect::<Result<Vec<_>, String>>().map(Some),
        Some(other) if !is_nullish(Some(other)) => Err(format!("AuthenticationProviderAuthenticationSessionsChangeEvent.{name} precisa ser uma lista de AuthenticationSession")),
        _ => Ok(None),
    }
}

// ---- as opções de `getSession`

/// `AuthenticationGetSessionPresentationOptions` e `AuthenticationForceNewSessionOptions`: `learnMore` só com a proposta `authLearnMore`.
fn interactive_of(value: Option<&JsData>) -> Result<Option<AuthenticationInteractive>, String> {
    match value {
        None | Some(JsData::Undefined) | Some(JsData::Null) => Ok(None),
        Some(JsData::Bool(flag)) => Ok(Some(AuthenticationInteractive::Flag(*flag))),
        Some(options @ JsData::Object(_)) => {
            let learn_more = match field(options, "learnMore") {
                Some(learn_more) if !is_nullish(Some(learn_more)) => {
                    check_proposed_api_enabled(&host_extension(), "authLearnMore").map_err(|error| error.message)?;
                    Some(uri_components(&uri_from_js(learn_more)?))
                }
                _ => None,
            };
            Ok(Some(AuthenticationInteractive::Options(AuthenticationInteractiveOptions {
                detail: opt_string(field(options, "detail")),
                learn_more,
                session_to_recreate: None,
            })))
        }
        Some(_) => Err("createIfNone e forceNewSession precisam ser um booleano ou um objeto".to_owned()),
    }
}

/// `vscode.AuthenticationGetSessionOptions`, com as conferências de proposta do `extHost.api.impl.ts`.
fn get_session_options_of(value: Option<&JsData>) -> Result<AuthenticationGetSessionOptions, String> {
    let Some(options) = value.filter(|options| !is_nullish(Some(*options))) else { return Ok(AuthenticationGetSessionOptions::default()) };
    let present = |name: &str| field(options, name).is_some_and(|value| !matches!(value, JsData::Undefined));
    let create_if_none = interactive_of(field(options, "createIfNone"))?;
    let force_new_session = interactive_of(field(options, "forceNewSession"))?;
    let authorization_server = match field(options, "authorizationServer") {
        Some(uri) if !is_nullish(Some(uri)) => Some(uri_components(&uri_from_js(uri)?)),
        _ => None,
    };
    if authorization_server.is_some() || present("clientId") || present("resource") {
        check_proposed_api_enabled(&host_extension(), "authIssuers").map_err(|error| error.message)?;
    }
    if present("audience") {
        check_proposed_api_enabled(&host_extension(), "authSessionAudience").map_err(|error| error.message)?;
    }
    let account = match field(options, "account") {
        Some(account) if !is_nullish(Some(account)) => Some(AuthenticationSessionAccountDto {
            label: opt_string(field(account, "label")).ok_or("getSession: account.label precisa ser texto")?,
            id: opt_string(field(account, "id")).ok_or("getSession: account.id precisa ser texto")?,
            icon: None,
        }),
        _ => None,
    };
    Ok(AuthenticationGetSessionOptions {
        silent: opt_bool(field(options, "silent")),
        account,
        authorization_server,
        client_id: opt_string(field(options, "clientId")),
        resource: opt_string(field(options, "resource")),
        audience: opt_string(field(options, "audience")),
        extra: None,
        clear_session_preference: opt_bool(field(options, "clearSessionPreference")),
        create_if_none,
        force_new_session,
    })
}

/// `readonly string[] | AuthenticationWwwAuthenticateRequest`.
fn scopes_or_request_of(value: Option<&JsData>) -> Result<AuthenticationScopeListOrRequest, String> {
    let strings = |items: &[JsData], what: &str| items.iter().map(|item| opt_string(Some(item)).ok_or_else(|| format!("{what} precisa ser uma lista de textos"))).collect::<Result<Vec<_>, String>>();
    match value {
        Some(JsData::Array(items)) => Ok(AuthenticationScopeListOrRequest::Scopes(strings(items, "getSession: os escopos")?)),
        Some(request @ JsData::Object(_)) => {
            let www_authenticate = opt_string(field(request, "wwwAuthenticate")).ok_or("getSession: wwwAuthenticate precisa ser texto")?;
            let fallback_scopes = match field(request, "fallbackScopes") {
                Some(JsData::Array(items)) => Some(strings(items, "getSession: fallbackScopes")?),
                _ => None,
            };
            Ok(AuthenticationScopeListOrRequest::Request(IAuthenticationWwwAuthenticateRequest { www_authenticate, fallback_scopes }))
        }
        _ => Err("getSession: os escopos precisam ser uma lista de textos ou um AuthenticationWwwAuthenticateRequest".to_owned()),
    }
}

// ---- o provedor da extensão

/// As opções que o provedor recebe (`vscode.AuthenticationProviderSessionOptions` e, de brinde, as chaves da thread principal).
fn provider_options_fields(options: &IAuthenticationProviderSessionOptions) -> Vec<(String, JsData)> {
    let mut fields: Vec<(String, JsData)> = Vec::new();
    if let Some(silent) = options.silent {
        fields.push(("silent".to_owned(), JsData::Bool(silent)));
    }
    if let Some(account) = &options.account {
        fields.push(("account".to_owned(), account_to_js(account)));
    }
    if let Some(authorization_server) = &options.authorization_server {
        fields.push(("authorizationServer".to_owned(), uri_to_js(authorization_server)));
    }
    for (name, value) in [("clientId", &options.client_id), ("resource", &options.resource), ("audience", &options.audience)] {
        if let Some(value) = value {
            fields.push((name.to_owned(), JsData::Str(value.clone())));
        }
    }
    fields
}

fn session_options_to_js(options: &IAuthenticationProviderSessionOptions) -> JsData {
    JsData::Object(provider_options_fields(options))
}

fn create_options_to_js(options: &IAuthenticationCreateSessionOptions) -> JsData {
    let mut fields = provider_options_fields(&options.session_options);
    if let Some(activate_immediate) = options.activate_immediate {
        fields.push(("activateImmediate".to_owned(), JsData::Bool(activate_immediate)));
    }
    JsData::Object(fields)
}

/// `IAuthenticationConstraint`: `{ challenges: [{ scheme, params }], fallbackScopes? }`.
fn constraint_to_js(constraint: &IAuthenticationConstraint) -> JsData {
    let challenges = constraint
        .challenges
        .iter()
        .map(|challenge| {
            let params = challenge.params.iter().map(|(name, value)| (name.clone(), JsData::Str(value.clone()))).collect();
            JsData::Object(vec![("scheme".to_owned(), JsData::Str(challenge.scheme.clone())), ("params".to_owned(), JsData::Object(params))])
        })
        .collect();
    let mut fields = vec![("challenges".to_owned(), JsData::Array(challenges))];
    if let Some(fallback_scopes) = &constraint.fallback_scopes {
        fields.push(("fallbackScopes".to_owned(), JsData::Array(fallback_scopes.iter().cloned().map(JsData::Str).collect())));
    }
    JsData::Object(fields)
}

/// O `vscode.AuthenticationProvider` da extensão (por referência, com o `this` e os métodos da classe).
struct JsAuthenticationProvider {
    js: JsObjects,
    table: Rc<DisposableTable>,
    target: JsData,
    /// A função de evento `onDidChangeSessions` do provedor.
    on_did_change_sessions: Rc<JsHandle>,
}

impl JsAuthenticationProvider {
    fn new(js: &JsObjects, table: &Rc<DisposableTable>, provider: &Rc<JsHandle>) -> Result<Rc<dyn AuthenticationProvider>, String> {
        let target = JsData::Handle(provider.clone());
        for required in ["getSessions", "createSession", "removeSession"] {
            if !js.is_function(&target, required) {
                return Err(format!("registerAuthenticationProvider: o provedor precisa de {required}"));
            }
        }
        let on_did_change_sessions = match js.property(&target, "onDidChangeSessions") {
            JsData::Handle(event) if js.is_function(&target, "onDidChangeSessions") => event,
            _ => return Err("registerAuthenticationProvider: o provedor precisa de onDidChangeSessions".to_owned()),
        };
        Ok(Rc::new(Self { js: js.clone(), table: table.clone(), target, on_did_change_sessions }))
    }

    /// `provider[method](...args)` com o resultado convertido por `convert`.
    fn call<T: Clone + 'static>(&self, method: &str, args: Vec<JsData>, convert: impl Fn(&NodeRunner, JsResult) -> Result<T, String> + 'static) -> MaybePromise<T> {
        let runner = self.js.runner().clone();
        convert_maybe(self.js.invoke(&self.target, method, args), move |result| convert(&runner, result).map_err(Error::new))
    }
}

/// O resultado de `getSessions` e `getSessionsFromChallenges`: uma lista de sessões.
fn sessions_of(runner: &NodeRunner, result: JsResult) -> Result<Vec<AuthenticationSession>, String> {
    match result {
        JsResult::Many(items) => items.iter().map(|item| session_of(&exported(runner, item))).collect(),
        _ => Err("o provedor precisa devolver uma lista de AuthenticationSession".to_owned()),
    }
}

/// O resultado de `createSession` e `createSessionFromChallenges`: uma sessão.
fn single_session_of(runner: &NodeRunner, result: JsResult) -> Result<AuthenticationSession, String> {
    match result {
        JsResult::Single(item) => session_of(&exported(runner, &item)),
        _ => Err("o provedor precisa devolver uma AuthenticationSession".to_owned()),
    }
}

/// O evento `{ added, removed, changed }` que o provedor dispara.
fn change_event_of(runner: &NodeRunner, result: &JsResult) -> Result<AuthenticationSessionsChangeEvent, String> {
    let JsResult::Single(value) = result else { return Err("onDidChangeSessions precisa disparar { added, removed, changed }".to_owned()) };
    let data = exported(runner, value);
    Ok(AuthenticationSessionsChangeEvent { added: sessions_field(&data, "added")?, removed: sessions_field(&data, "removed")?, changed: sessions_field(&data, "changed")? })
}

impl AuthenticationProvider for JsAuthenticationProvider {
    fn on_did_change_sessions(&self) -> Event<AuthenticationSessionsChangeEvent> {
        let (js, table, event) = (self.js.clone(), self.table.clone(), self.on_did_change_sessions.clone());
        Event::new(move |listener: Listener<AuthenticationSessionsChangeEvent>| -> DisposableRef {
            let runner = js.runner().clone();
            let subscribed = js.listen(&event, move |result| match change_event_of(&runner, &result) {
                Ok(change) => listener(&change),
                Err(message) => on_unexpected_error(&Error::new(message)),
            });
            match subscribed {
                Ok(Some(id)) => {
                    let table = table.clone();
                    plain_disposable(move || table.dispose(id))
                }
                Ok(None) => plain_disposable(|| {}),
                Err(message) => {
                    on_unexpected_error(&Error::new(message));
                    plain_disposable(|| {})
                }
            }
        })
    }

    fn get_sessions(&self, scopes: Option<&[String]>, options: &IAuthenticationProviderSessionOptions) -> MaybePromise<Vec<AuthenticationSession>> {
        let scopes = scopes.map_or(JsData::Undefined, |scopes| JsData::Array(scopes.iter().cloned().map(JsData::Str).collect()));
        self.call("getSessions", vec![scopes, session_options_to_js(options)], sessions_of)
    }

    fn create_session(&self, scopes: &[String], options: &IAuthenticationCreateSessionOptions) -> MaybePromise<AuthenticationSession> {
        let scopes = JsData::Array(scopes.iter().cloned().map(JsData::Str).collect());
        self.call("createSession", vec![scopes, create_options_to_js(options)], single_session_of)
    }

    fn remove_session(&self, session_id: &str) -> MaybePromise<()> {
        self.call("removeSession", vec![JsData::Str(session_id.to_owned())], |_, _| Ok(()))
    }

    fn get_sessions_from_challenges(&self, constraint: &IAuthenticationConstraint, options: &IAuthenticationProviderSessionOptions) -> Option<MaybePromise<Vec<AuthenticationSession>>> {
        self.js
            .is_function(&self.target, "getSessionsFromChallenges")
            .then(|| self.call("getSessionsFromChallenges", vec![constraint_to_js(constraint), session_options_to_js(options)], sessions_of))
    }

    fn create_session_from_challenges(&self, constraint: &IAuthenticationConstraint, options: &IAuthenticationCreateSessionOptions) -> Option<MaybePromise<AuthenticationSession>> {
        self.js
            .is_function(&self.target, "createSessionFromChallenges")
            .then(|| self.call("createSessionFromChallenges", vec![constraint_to_js(constraint), create_options_to_js(options)], single_session_of))
    }
}

// ---- o namespace

/// `vscode.authentication.registerAuthenticationProvider(id, label, provider, options?)`.
fn register_authentication_provider(
    js: &JsObjects,
    table: &Rc<DisposableTable>,
    authentication: &Rc<ExtHostAuthentication>,
    args: &[JsData],
) -> Result<JsData, String> {
    let id = string_arg(args, 0, "registerAuthenticationProvider: o id")?;
    let label = string_arg(args, 1, "registerAuthenticationProvider: o label")?;
    let Some(JsData::Handle(provider)) = args.get(2) else { return Err("registerAuthenticationProvider: o provedor precisa ser um objeto".to_owned()) };
    let options = args.get(3).cloned().unwrap_or(JsData::Undefined);
    if !is_nullish(Some(&js.property(&options, "supportedAuthorizationServers"))) {
        check_proposed_api_enabled(&host_extension(), "authIssuers").map_err(|error| error.message)?;
    }
    let provider_options = AuthenticationProviderOptions {
        supports_multiple_accounts: opt_bool(Some(&js.property(&options, "supportsMultipleAccounts"))),
        supported_authorization_servers: None,
        supports_challenges: opt_bool(Some(&js.property(&options, "supportsChallenges"))),
    };
    let provider = JsAuthenticationProvider::new(js, table, provider)?;
    Ok(table.to_js(authentication.register_authentication_provider(&id, &label, provider, Some(provider_options))))
}

/// O namespace `authentication`.
pub fn namespace(runner: &NodeRunner, js: &JsObjects, authentication: &Rc<ExtHostAuthentication>, table: &Rc<DisposableTable>) -> JsData {
    let get_session = {
        let (runner, authentication) = (runner.clone(), authentication.clone());
        runner.clone().make_function(
            "getSession",
            3,
            Rc::new(move |args| {
                let provider_id = string_arg(args, 0, "getSession: o providerId")?;
                let scopes_or_request = scopes_or_request_of(args.get(1))?;
                let options = get_session_options_of(args.get(2))?;
                let session = authentication.get_session(&host_extension(), &provider_id, scopes_or_request, options);
                Ok(promise_to_js(&runner, session, |session| session.as_ref().map_or(JsData::Undefined, session_to_js)))
            }),
        )
    };
    let get_accounts = {
        let (runner, authentication) = (runner.clone(), authentication.clone());
        runner.clone().make_function(
            "getAccounts",
            1,
            Rc::new(move |args| {
                let provider_id = string_arg(args, 0, "getAccounts: o providerId")?;
                Ok(promise_to_js(&runner, authentication.get_accounts(&provider_id), |accounts| JsData::Array(accounts.iter().map(account_to_js).collect())))
            }),
        )
    };
    let register = {
        let (js, table, authentication) = (js.clone(), table.clone(), authentication.clone());
        runner.make_function_by_reference("registerAuthenticationProvider", 4, Rc::new(move |args| register_authentication_provider(&js, &table, &authentication, args)))
    };
    let on_did_change_sessions = event_function(
        runner,
        table,
        "onDidChangeSessions",
        authentication.get_extension_scoped_sessions_event(HOST_EXTENSION),
        |provider: &AuthenticationProviderInformation| {
            JsData::Object(vec![(
                "provider".to_owned(),
                JsData::Object(vec![("id".to_owned(), JsData::Str(provider.id.clone())), ("label".to_owned(), JsData::Str(provider.label.clone()))]),
            )])
        },
    );
    JsData::Object(vec![
        ("getSession".to_owned(), JsData::Handle(Rc::new(get_session))),
        ("getAccounts".to_owned(), JsData::Handle(Rc::new(get_accounts))),
        ("onDidChangeSessions".to_owned(), on_did_change_sessions),
        ("registerAuthenticationProvider".to_owned(), JsData::Handle(Rc::new(register))),
    ])
}
