//! Porte de `src/vs/workbench/services/authentication/browser/authenticationService.ts`: o serviço que guarda os
//! provedores de autenticação declarados e registrados, repassa sessões, ativa o provedor sob demanda e
//! encaminha os eventos de mudança de sessão.
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados (`@IExtensionService`, `@IAuthenticationAccessService`,
//!   `@ILogService`) são a tupla `Deps` do [`Injectable`]. O `@IBrowserWorkbenchEnvironmentService` só é lido
//!   por `environmentService.options?.authenticationProviders`, e o porte não tem esse serviço do navegador: a
//!   lista chega como o argumento estático `Args` (vazio quando o ambiente não define `options`, como no
//!   `TestEnvironmentService`). A classe herda de `Disposable`, aqui um `Disposable` guardado no campo `base`.
//! - **Dois traits.** O serviço implementa [`IAuthenticationService`] (os membros que os outros serviços
//!   chamam) e [`IAuthenticationServiceProviders`] (o resto), que juntos são o `IAuthenticationService` do
//!   original (ver `authentication.rs`).
//! - **`registerSingleton(..., InstantiationType.Delayed)`** vira [`register`], mas `Eager`: o identificador
//!   `IAuthenticationService` do porte é o do trait reduzido e não declara proxy de instanciação atrasada.
//! - **Ponto de extensão `authentication`.** O registro de pontos de extensão do porte não tem `setHandler`, nem
//!   o esquema JSON e o gerador de eventos de ativação do descritor. [`register_extension_point`] registra o
//!   ponto; o corpo do tratador é [`AuthenticationService::handle_extension_point_delta`] (como o
//!   `handle_delta` dos registros de problem matcher); o gerador é [`authentication_activation_events`]; e o
//!   `authenticationDefinitionSchema` (só descrições e `additionalProperties: false` para o editor de
//!   `package.json`) fica para o esquema JSON do ponto de extensão.
//! - **Funções `async`** são `async_fn` (o corpo roda já, até o primeiro `await`). Num `async` do original que
//!   lança, o `?` rejeita; um `throw` síncrono dos métodos que não são `async` é [`errors::throw`].
//! - **`getSessions`/`createSession`.** O `await getSessions(...)` do `else throw No authentication provider ...`
//!   é inalcançável no original (`tryActivateProvider` devolve o provedor ou lança), então não existe aqui.
//!   `{ ...options }` é um clone. `IAuthenticationCreateSessionOptions` leva `activateImmediate` junto ao
//!   provedor no original (chave a mais do índice `[key: string]: any`); aqui o provedor recebe só as opções de
//!   sessão, e `activateImmediate` é lido pelo serviço. Os membros opcionais `getSessionsFromChallenges` e
//!   `createSessionFromChallenges` ausentes são `None` (ver [`IAuthenticationProvider`]).
//! - **`Promise.race([activationPromise, providerRegistered])`** compara promessas de tipos diferentes; cada uma
//!   vira `Promise<()>` antes, com uma rodada de microtarefa a mais por `then`.
//! - **`getCurrentAuthenticationSessionInfo`**: o `console.error` é `eprintln!`, e o `JSON.parse` do valor
//!   guardado só aceita o objeto com `id`, `accessToken` e `providerId` textuais (as demais chaves do objeto
//!   são descartadas; `canSignOut` que não é booleano vira ausente).
//! - **`providerId`/`id` do `registerAuthenticationProvider(id, provider)`** são independentes, como no
//!   original: o mapa usa `id`, e `getProviderIds` devolve o `provider.id` de cada entrada.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use je_base::JsString;
use je_base::async_::{async_fn, race_cancellation, race_timeout};
use je_base::cancellation::CancellationTokenSource;
use je_base::errors::{self, Error};
use je_base::event::{Disposables, Emitter, Event};
use je_base::glob::{GlobOptions, match_pattern};
use je_base::json::Value;
use je_base::jsonc::json_parse;
use je_base::lifecycle::{Disposable, DisposableMap, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::oauth::{IAuthorizationProtectedResourceMetadata, IAuthorizationServerMetadata, parse_www_authenticate_header};
use je_base::promise::Promise;
use je_base::strings::equals_ignore_case;
use je_base::uri::URI;
use je_instantiation::{Injectable, SyncDescriptor, register_singleton_descriptor, service_impl};
use je_platform::log::LogService;
use je_platform::product::IProductService;
use je_platform::secrets::secrets::ISecretStorageService;

use super::authentication::{
    AuthenticationProviderInformation, AuthenticationScopeListOrRequest, AuthenticationSession, AuthenticationSessionAccount,
    AuthenticationSessionsChange, AuthenticationSessionsChangeEvent, I_AUTHENTICATION_SERVICE, IAuthenticationConstraint, IAuthenticationCreateSessionOptions,
    IAuthenticationGetSessionsOptions, IAuthenticationProvider, IAuthenticationProviderHostDelegate, IAuthenticationService,
    IAuthenticationServiceProviders,
};
use super::authentication_access_service::IAuthenticationAccessService;
use crate::services::extensions::extensions::{ActivationKind, IExtensionService};
use crate::services::extensions::extensions_registry::{ExtensionPoint, ExtensionPointDelta, ExtensionPointDescriptor, extensions_registry};

/// `getAuthenticationProviderActivationEvent`.
pub fn get_authentication_provider_activation_event(id: &str) -> String {
    format!("onAuthenticationRequest:{id}")
}

// TODO: pull this out into its own service
/// `AuthenticationSessionInfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthenticationSessionInfo {
    pub id: String,
    pub access_token: String,
    pub provider_id: String,
    pub can_sign_out: Option<bool>,
}

/// `getCurrentAuthenticationSessionInfo`.
pub fn get_current_authentication_session_info(
    secret_storage_service: Rc<dyn ISecretStorageService>,
    product_service: Rc<dyn IProductService>,
) -> Promise<Option<AuthenticationSessionInfo>> {
    async_fn(async move {
        let authentication_session_value = secret_storage_service.get(&format!("{}.loginAccount", product_service.url_protocol())).await?;
        if let Some(value) = authentication_session_value.filter(|value| !value.is_empty()) {
            match json_parse(JsString::from(value.as_str()).as_ref()) {
                Ok(Value::Object(object)) => {
                    let text = |key: &str| match object.get(key) {
                        Some(Value::String(text)) => Some(text.to_string_lossy()),
                        _ => None,
                    };
                    if let (Some(id), Some(access_token), Some(provider_id)) = (text("id"), text("accessToken"), text("providerId")) {
                        let can_sign_out = match object.get("canSignOut") {
                            Some(Value::Bool(can_sign_out)) => Some(*can_sign_out),
                            _ => None,
                        };
                        return Ok(Some(AuthenticationSessionInfo { id, access_token, provider_id, can_sign_out }));
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    // This is a best effort operation.
                    eprintln!("Failed parsing current auth session value: {error:?}");
                }
            }
        }
        Ok(None)
    })
}

/// `authenticationExtPoint = ExtensionsRegistry.registerExtensionPoint({ extensionPoint: 'authentication', ... })`.
pub fn register_extension_point() -> Rc<ExtensionPoint> {
    extensions_registry().register_extension_point(ExtensionPointDescriptor {
        extension_point: "authentication".to_string(),
        default_extension_kind: None,
        can_handle_resolver: None,
    })
}

/// O `activationEventsGenerator` do ponto de extensão: `onAuthenticationRequest:<id>` para cada provedor com `id`.
pub fn authentication_activation_events(authentication_providers: &[AuthenticationProviderInformation]) -> Vec<String> {
    authentication_providers
        .iter()
        .filter(|provider| !provider.id.is_empty())
        .map(|provider| format!("onAuthenticationRequest:{}", provider.id))
        .collect()
}

/// `isFalsyOrWhitespace(text)` sobre um texto que já existe.
fn is_blank(text: &str) -> bool {
    text.trim_start().is_empty()
}

/// O estado do serviço, em `Rc` para as funções `async` e os ouvintes o segurarem.
struct Inner {
    on_did_register_authentication_provider: Rc<Emitter<AuthenticationProviderInformation>>,
    on_did_unregister_authentication_provider: Rc<Emitter<AuthenticationProviderInformation>>,
    on_did_change_sessions: Rc<Emitter<AuthenticationSessionsChange>>,
    on_did_change_declared_providers: Rc<Emitter<()>>,

    /// `_authenticationProviders`: um `Map`, que guarda a ordem da primeira inserção de cada chave.
    authentication_providers: RefCell<Vec<(String, Rc<dyn IAuthenticationProvider>)>>,
    authentication_provider_disposables: Rc<DisposableMap<String, DisposableStore>>,
    dynamic_authentication_provider_ids: RefCell<HashSet<String>>,

    delegates: RefCell<Vec<Rc<dyn IAuthenticationProviderHostDelegate>>>,

    disposed_source: CancellationTokenSource,

    declared_providers: RefCell<Vec<AuthenticationProviderInformation>>,

    extension_service: Rc<dyn IExtensionService>,
    log_service: Rc<dyn LogService>,
}

/// `AuthenticationService`.
pub struct AuthenticationService {
    base: Rc<Disposable>,
    inner: Rc<Inner>,
}

impl Injectable for AuthenticationService {
    /// `environmentService.options?.authenticationProviders` (vazia quando o ambiente não define `options`).
    type Args = Vec<Rc<dyn IAuthenticationProvider>>;
    type Deps = (Rc<dyn IExtensionService>, Rc<dyn IAuthenticationAccessService>, Rc<dyn LogService>);

    fn create(environment_providers: Self::Args, (extension_service, authentication_access_service, log_service): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_register_authentication_provider = Emitter::new();
        base.register(on_did_register_authentication_provider.clone());
        let on_did_unregister_authentication_provider = Emitter::new();
        base.register(on_did_unregister_authentication_provider.clone());
        let on_did_change_sessions = Emitter::new();
        base.register(on_did_change_sessions.clone());
        let on_did_change_declared_providers = Emitter::new();
        base.register(on_did_change_declared_providers.clone());
        let authentication_provider_disposables = DisposableMap::new();
        base.register(authentication_provider_disposables.clone());

        let disposed_source = CancellationTokenSource::new(None);
        {
            let disposed_source = disposed_source.clone();
            base.register(to_disposable(move || disposed_source.dispose_with(true)));
        }

        let inner = Rc::new(Inner {
            on_did_register_authentication_provider,
            on_did_unregister_authentication_provider,
            on_did_change_sessions,
            on_did_change_declared_providers,
            authentication_providers: RefCell::new(Vec::new()),
            authentication_provider_disposables,
            dynamic_authentication_provider_ids: RefCell::new(HashSet::new()),
            delegates: RefCell::new(Vec::new()),
            disposed_source,
            declared_providers: RefCell::new(Vec::new()),
            extension_service,
            log_service,
        });

        {
            let on_did_change_sessions = inner.on_did_change_sessions.clone();
            base.register(authentication_access_service.on_did_change_extension_session_access().listen(move |e| {
                // The access has changed, not the actual session itself but extensions depend on this event firing
                // when they have gained access to an account so this fires that event.
                on_did_change_sessions.fire(AuthenticationSessionsChange {
                    provider_id: e.provider_id.clone(),
                    label: e.account_name.clone(),
                    event: AuthenticationSessionsChangeEvent { added: Some(Vec::new()), changed: Some(Vec::new()), removed: Some(Vec::new()) },
                });
            }));
        }

        let service = Rc::new(Self { base, inner });
        service.inner.register_env_contributed_authentication_providers(environment_providers);
        service
    }
}

service_impl!(AuthenticationService => dyn IAuthenticationService, disposable);

/// `registerSingleton(IAuthenticationService, AuthenticationService, InstantiationType.Delayed)`, `Eager` (ver o
/// topo do arquivo); o ambiente do navegador, sem `options`, não contribui provedores.
pub fn register() {
    register_singleton_descriptor(
        I_AUTHENTICATION_SERVICE,
        SyncDescriptor::<dyn IAuthenticationService>::new::<AuthenticationService>(Vec::new()),
    );
}

impl AuthenticationService {
    /// O corpo do `authenticationExtPoint.setHandler((_extensions, { added, removed }) => ...)`: `delta.added` e
    /// `delta.removed` são as listas de provedores que cada extensão contribui.
    pub fn handle_extension_point_delta(&self, delta: &ExtensionPointDelta<Vec<AuthenticationProviderInformation>>) {
        self.inner.handle_extension_point_delta(delta);
    }
}

impl Inner {
    fn provider(&self, id: &str) -> Option<Rc<dyn IAuthenticationProvider>> {
        self.authentication_providers.borrow().iter().find(|(key, _)| key == id).map(|(_, provider)| provider.clone())
    }

    fn is_disposed(&self) -> bool {
        self.disposed_source.token().is_cancellation_requested()
    }

    fn register_env_contributed_authentication_providers(self: &Rc<Self>, providers: Vec<Rc<dyn IAuthenticationProvider>>) {
        for provider in providers {
            self.register_declared_authentication_provider(AuthenticationProviderInformation {
                id: provider.id(),
                label: provider.label(),
                authorization_server_globs: None,
            });
            self.register_authentication_provider(&provider.id(), provider);
        }
    }

    fn handle_extension_point_delta(&self, delta: &ExtensionPointDelta<Vec<AuthenticationProviderInformation>>) {
        self.log_service.debug(&format!(
            "Found authentication providers. added: {}, removed: {}",
            delta.added.len(),
            delta.removed.len()
        ));
        for point in &delta.added {
            for provider in &point.value {
                if is_blank(&provider.id) {
                    point.collector.error("An authentication contribution must specify an id.");
                    continue;
                }

                if is_blank(&provider.label) {
                    point.collector.error("An authentication contribution must specify a label.");
                    continue;
                }

                if !self.declared_providers.borrow().iter().any(|p| p.id == provider.id) {
                    self.register_declared_authentication_provider(provider.clone());
                    self.log_service.debug(&format!("Declared authentication provider: {}", provider.id));
                } else {
                    point.collector.error(&format!("This authentication id '{}' has already been registered", provider.id));
                }
            }
        }

        let removed_ext_points: Vec<&AuthenticationProviderInformation> = delta.removed.iter().flat_map(|r| r.value.iter()).collect();
        for point in removed_ext_points {
            let provider = self.declared_providers.borrow().iter().find(|provider| provider.id == point.id).cloned();
            if let Some(provider) = provider {
                self.unregister_declared_authentication_provider(&provider.id);
                self.log_service.debug(&format!("Undeclared authentication provider: {}", provider.id));
            }
        }
    }

    fn register_declared_authentication_provider(&self, provider: AuthenticationProviderInformation) {
        if is_blank(&provider.id) {
            errors::throw(Error::new("An authentication contribution must specify an id."));
        }
        if is_blank(&provider.label) {
            errors::throw(Error::new("An authentication contribution must specify a label."));
        }
        if self.declared_providers.borrow().iter().any(|p| p.id == provider.id) {
            errors::throw(Error::new(format!("This authentication id '{}' has already been registered", provider.id)));
        }
        self.declared_providers.borrow_mut().push(provider);
        self.on_did_change_declared_providers.fire(());
    }

    fn unregister_declared_authentication_provider(&self, id: &str) {
        let index = self.declared_providers.borrow().iter().position(|provider| provider.id == id);
        if let Some(index) = index {
            self.declared_providers.borrow_mut().remove(index);
        }
        self.on_did_change_declared_providers.fire(());
    }

    fn register_authentication_provider(&self, id: &str, authentication_provider: Rc<dyn IAuthenticationProvider>) {
        {
            let mut providers = self.authentication_providers.borrow_mut();
            match providers.iter_mut().find(|(key, _)| key == id) {
                Some(entry) => entry.1 = authentication_provider.clone(),
                None => providers.push((id.to_owned(), authentication_provider.clone())),
            }
        }
        let disposable_store = DisposableStore::new();
        {
            let on_did_change_sessions = self.on_did_change_sessions.clone();
            let provider = authentication_provider.clone();
            let provider_id = id.to_owned();
            disposable_store.add(authentication_provider.on_did_change_sessions().listen(move |e| {
                on_did_change_sessions.fire(AuthenticationSessionsChange {
                    provider_id: provider_id.clone(),
                    label: provider.label(),
                    event: e.clone(),
                });
            }));
        }
        if let Some(disposable) = authentication_provider.as_disposable() {
            disposable_store.add(disposable);
        }
        self.authentication_provider_disposables.set(id.to_owned(), disposable_store, false);
        self.on_did_register_authentication_provider.fire(AuthenticationProviderInformation {
            id: id.to_owned(),
            label: authentication_provider.label(),
            authorization_server_globs: None,
        });
    }

    fn unregister_authentication_provider(&self, id: &str) {
        let provider = self.provider(id);
        if let Some(provider) = provider {
            self.authentication_providers.borrow_mut().retain(|(key, _)| key != id);
            // If this is a dynamic provider, remove it from the set of dynamic providers
            self.dynamic_authentication_provider_ids.borrow_mut().remove(id);
            self.on_did_unregister_authentication_provider.fire(AuthenticationProviderInformation {
                id: id.to_owned(),
                label: provider.label(),
                authorization_server_globs: None,
            });
        }
        self.authentication_provider_disposables.delete_and_dispose(&id.to_owned());
    }

    fn get_provider(&self, id: &str) -> Rc<dyn IAuthenticationProvider> {
        match self.provider(id) {
            Some(provider) => provider,
            None => errors::throw(Error::new(format!("No authentication provider '{id}' is currently registered."))),
        }
    }

    fn get_accounts(self: &Rc<Self>, id: &str) -> Promise<Vec<AuthenticationSessionAccount>> {
        let this = self.clone();
        let id = id.to_owned();
        async_fn(async move {
            // TODO: Cache this
            let sessions = this.get_sessions(&id, None, None, false).await?;
            let mut accounts: Vec<AuthenticationSessionAccount> = Vec::new();
            let mut seen_accounts: HashSet<String> = HashSet::new();
            for session in sessions {
                if !seen_accounts.contains(&session.account.label) {
                    seen_accounts.insert(session.account.label.clone());
                    accounts.push(session.account);
                }
            }
            Ok(accounts)
        })
    }

    fn get_sessions(
        self: &Rc<Self>,
        id: &str,
        scope_list_or_request: Option<AuthenticationScopeListOrRequest>,
        options: Option<IAuthenticationGetSessionsOptions>,
        activate_immediate: bool,
    ) -> Promise<Vec<AuthenticationSession>> {
        let this = self.clone();
        let id = id.to_owned();
        async_fn(async move {
            if this.is_disposed() {
                return Ok(Vec::new());
            }

            let auth_provider = match this.provider(&id) {
                Some(provider) => provider,
                None => this.try_activate_provider(&id, activate_immediate).await?,
            };
            // Check if the authorization server is in the list of supported authorization servers
            if let Some(server) = options.as_ref().and_then(|options| options.authorization_server.as_ref()) {
                // Skip the resource server check since the auth provider id contains a specific resource server
                // TODO@TylerLeonhardt: this can change when we have providers that support multiple resource servers
                if !Self::matches_provider(auth_provider.as_ref(), server, None) {
                    return Err(Error::new(format!(
                        "The authentication provider '{id}' does not support the authorization server '{}'.",
                        server.to_string(true)
                    )));
                }
            }
            let provider_options = options.unwrap_or_default();
            match scope_list_or_request {
                Some(AuthenticationScopeListOrRequest::Request(request)) => {
                    let constraint = IAuthenticationConstraint {
                        challenges: parse_www_authenticate_header(&request.www_authenticate),
                        fallback_scopes: request.fallback_scopes,
                    };
                    match auth_provider.get_sessions_from_challenges(constraint, provider_options) {
                        Some(sessions) => Ok(sessions.await?),
                        None => Err(Error::new(format!("The authentication provider '{id}' does not support getting sessions from challenges."))),
                    }
                }
                Some(AuthenticationScopeListOrRequest::Scopes(scopes)) => Ok(auth_provider.get_sessions(Some(scopes), provider_options).await?),
                None => Ok(auth_provider.get_sessions(None, provider_options).await?),
            }
        })
    }

    fn create_session(
        self: &Rc<Self>,
        id: &str,
        scope_list_or_request: AuthenticationScopeListOrRequest,
        options: Option<IAuthenticationCreateSessionOptions>,
    ) -> Promise<AuthenticationSession> {
        let this = self.clone();
        let id = id.to_owned();
        async_fn(async move {
            if this.is_disposed() {
                return Err(Error::new("Authentication service is disposed."));
            }

            let activate_immediate = options.as_ref().and_then(|options| options.activate_immediate).unwrap_or(false);
            let auth_provider = match this.provider(&id) {
                Some(provider) => provider,
                None => this.try_activate_provider(&id, activate_immediate).await?,
            };
            let provider_options = options.map(|options| options.session_options).unwrap_or_default();
            match scope_list_or_request {
                AuthenticationScopeListOrRequest::Request(request) => {
                    let constraint = IAuthenticationConstraint {
                        challenges: parse_www_authenticate_header(&request.www_authenticate),
                        fallback_scopes: request.fallback_scopes,
                    };
                    match auth_provider.create_session_from_challenges(constraint, provider_options) {
                        Some(session) => Ok(session.await?),
                        None => Err(Error::new(format!("The authentication provider '{id}' does not support creating sessions from challenges."))),
                    }
                }
                AuthenticationScopeListOrRequest::Scopes(scopes) => Ok(auth_provider.create_session(scopes, provider_options).await?),
            }
        })
    }

    fn remove_session(self: &Rc<Self>, id: &str, session_id: &str) -> Promise<()> {
        let this = self.clone();
        let id = id.to_owned();
        let session_id = session_id.to_owned();
        async_fn(async move {
            if this.is_disposed() {
                return Err(Error::new("Authentication service is disposed."));
            }

            match this.provider(&id) {
                Some(auth_provider) => Ok(auth_provider.remove_session(&session_id).await?),
                None => Err(Error::new(format!("No authentication provider '{id}' is currently registered."))),
            }
        })
    }

    fn get_or_activate_provider_id_for_server(self: &Rc<Self>, authorization_server: &URI, resource_server: Option<&URI>) -> Promise<Option<String>> {
        let this = self.clone();
        let authorization_server = authorization_server.clone();
        let resource_server = resource_server.cloned();
        async_fn(async move {
            let registered: Vec<Rc<dyn IAuthenticationProvider>> =
                this.authentication_providers.borrow().iter().map(|(_, provider)| provider.clone()).collect();
            for provider in registered {
                if Self::matches_provider(provider.as_ref(), &authorization_server, resource_server.as_ref()) {
                    return Ok(Some(provider.id()));
                }
            }

            let auth_server_str = authorization_server.to_string(true).to_string_lossy();
            let options = GlobOptions { ignore_case: Some(true), ..GlobOptions::default() };
            let providers: Vec<AuthenticationProviderInformation> = this
                .declared_providers
                .borrow()
                .iter()
                // Only consider providers that are not already registered since we already checked them
                .filter(|p| this.provider(&p.id).is_none())
                .filter(|p| p.authorization_server_globs.as_ref().is_some_and(|globs| globs.iter().any(|i| match_pattern(i.as_str(), &auth_server_str, &options))))
                .cloned()
                .collect();

            // TODO:@TylerLeonhardt fan out?
            for provider in providers {
                let active_provider = this.try_activate_provider(&provider.id, true).await?;
                // Check the resolved authorization servers
                if Self::matches_provider(active_provider.as_ref(), &authorization_server, resource_server.as_ref()) {
                    return Ok(Some(active_provider.id()));
                }
            }
            Ok(None)
        })
    }

    fn create_dynamic_authentication_provider(
        self: &Rc<Self>,
        authorization_server: &URI,
        server_metadata: &IAuthorizationServerMetadata,
        resource: Option<&IAuthorizationProtectedResourceMetadata>,
        client_id: Option<&str>,
        client_secret: Option<&str>,
    ) -> Promise<Option<Rc<dyn IAuthenticationProvider>>> {
        let this = self.clone();
        let authorization_server = authorization_server.clone();
        let server_metadata = server_metadata.clone();
        let resource = resource.cloned();
        let client_id = client_id.map(str::to_owned);
        let client_secret = client_secret.map(str::to_owned);
        async_fn(async move {
            let delegate = this.delegates.borrow().first().cloned();
            let Some(delegate) = delegate else {
                this.log_service.error("No authentication provider host delegate found");
                return Ok(None);
            };
            let provider_id =
                delegate.create(&authorization_server, &server_metadata, resource.as_ref(), client_id.as_deref(), client_secret.as_deref()).await?;
            if let Some(provider) = this.provider(&provider_id) {
                this.log_service.debug(&format!("Created dynamic authentication provider: {provider_id}"));
                this.dynamic_authentication_provider_ids.borrow_mut().insert(provider_id);
                return Ok(Some(provider));
            }
            this.log_service.error(&format!("Failed to create dynamic authentication provider: {provider_id}"));
            Ok(None)
        })
    }

    fn create_or_get_xaa_provider(self: &Rc<Self>, issuer: &URI) -> Promise<Option<String>> {
        let this = self.clone();
        let issuer = issuer.clone();
        async_fn(async move {
            let provider_id = format!("xaa:{}", issuer.to_string(true));
            if this.provider(&provider_id).is_some() {
                return Ok(Some(provider_id));
            }
            let delegate = this.delegates.borrow().iter().find(|d| d.supports_xaa()).cloned();
            let Some(delegate) = delegate else {
                this.log_service.error("No authentication provider host delegate supports XAA");
                return Ok(None);
            };
            let Some(created) = delegate.create_xaa(&issuer) else {
                return Err(Error::new("delegate.createXaa is not a function"));
            };
            let created = created.await?;
            if this.provider(&created).is_some() {
                this.log_service.debug(&format!("Created XAA authentication provider: {created}"));
                return Ok(Some(created));
            }
            this.log_service.error(&format!("Failed to create XAA authentication provider for issuer: {}", issuer.to_string(true)));
            Ok(None)
        })
    }

    fn register_authentication_provider_host_delegate(self: &Rc<Self>, delegate: Rc<dyn IAuthenticationProviderHostDelegate>) -> DisposableRef {
        {
            let mut delegates = self.delegates.borrow_mut();
            delegates.push(delegate.clone());
            delegates.sort_by(|a, b| b.priority().total_cmp(&a.priority()));
        }

        let this = self.clone();
        to_disposable(move || {
            let index = this.delegates.borrow().iter().position(|candidate| Rc::ptr_eq(candidate, &delegate));
            if let Some(index) = index {
                this.delegates.borrow_mut().remove(index);
            }
        })
    }

    fn matches_provider(provider: &dyn IAuthenticationProvider, authorization_server: &URI, resource_server: Option<&URI>) -> bool {
        // If a resourceServer is provided and the provider has a resourceServer defined, they must match
        if let (Some(resource_server), Some(provider_resource_server)) = (resource_server, provider.resource_server()) {
            let resource_server_str = resource_server.to_string(true);
            let provider_resource_server_str = provider_resource_server.to_string(true);
            if !equals_ignore_case(provider_resource_server_str.as_ref(), resource_server_str.as_ref()) {
                return false;
            }
        }

        if let Some(authorization_servers) = provider.authorization_servers() {
            let auth_server_str = authorization_server.to_string(true);
            let options = GlobOptions { ignore_case: Some(true), ..GlobOptions::default() };
            for server in authorization_servers {
                let str = server.to_string(true);
                if equals_ignore_case(str.as_ref(), auth_server_str.as_ref())
                    || match_pattern(str.to_string_lossy().as_str(), &auth_server_str.to_string_lossy(), &options)
                {
                    return true;
                }
            }
        }
        false
    }

    fn try_activate_provider(self: &Rc<Self>, provider_id: &str, activate_immediate: bool) -> Promise<Rc<dyn IAuthenticationProvider>> {
        let this = self.clone();
        let provider_id = provider_id.to_owned();
        async_fn(async move {
            let store = DisposableStore::new();
            let result = this.try_activate_provider_in(&provider_id, activate_immediate, &store).await;
            store.dispose();
            result
        })
    }

    /// O corpo do `try` de `tryActivateProvider`; o `finally { store.dispose() }` é de quem chama.
    async fn try_activate_provider_in(
        self: &Rc<Self>,
        provider_id: &str,
        activate_immediate: bool,
        store: &Rc<DisposableStore>,
    ) -> Result<Rc<dyn IAuthenticationProvider>, Error> {
        // Don't await activateByEvent exclusively: one or more extension
        // hosts may be blocked (e.g. webworker waiting on remote authority),
        // causing a deadlock. Instead, race with the provider being
        // registered so we can proceed as soon as any host delivers it. (#315841)
        let activation_promise = self.extension_service.activate_by_event(
            &get_authentication_provider_activation_event(provider_id),
            Some(if activate_immediate { ActivationKind::Immediate } else { ActivationKind::Normal }),
        );

        if let Some(provider) = self.provider(provider_id) {
            return Ok(provider);
        }
        if self.is_disposed() {
            return Err(Error::new("Authentication service is disposed."));
        }

        let registered_event = {
            let provider_id = provider_id.to_owned();
            self.on_did_register_authentication_provider.event().filter(move |e| e.id == provider_id, Some(&**store))
        };
        let registration = registered_event.to_promise(Some(Disposables::Store(store.clone())));
        let provider_registered = race_cancellation(&registration, &self.disposed_source.token());

        // Wait for either activation to complete or the provider to register.
        Promise::race(vec![activation_promise.then(|_| ()), provider_registered.then(|_| ())]).await?;

        if let Some(provider) = self.provider(provider_id) {
            return Ok(provider);
        }

        // TODO: Remove this timeout and figure out a better way to ensure auth providers
        // are registered _during_ extension activation.
        let result = race_timeout(&provider_registered, 5000, None).await?;
        if let Some(provider) = self.provider(provider_id) {
            return Ok(provider);
        }
        if result.flatten().is_none() {
            return Err(Error::new(format!("Timed out waiting for authentication provider '{provider_id}' to register.")));
        }
        Err(Error::new(format!("No authentication provider '{provider_id}' is currently registered.")))
    }
}

impl IAuthenticationService for AuthenticationService {
    fn on_did_unregister_authentication_provider(&self) -> Event<AuthenticationProviderInformation> {
        self.inner.on_did_unregister_authentication_provider.event()
    }

    fn on_did_change_declared_providers(&self) -> Event<()> {
        self.inner.on_did_change_declared_providers.event()
    }

    fn declared_providers(&self) -> Vec<AuthenticationProviderInformation> {
        self.inner.declared_providers.borrow().clone()
    }

    fn is_authentication_provider_registered(&self, id: &str) -> bool {
        self.inner.provider(id).is_some()
    }

    fn on_did_register_authentication_provider(&self) -> Event<AuthenticationProviderInformation> {
        self.inner.on_did_register_authentication_provider.event()
    }

    fn on_did_change_sessions(&self) -> Event<AuthenticationSessionsChange> {
        self.inner.on_did_change_sessions.event()
    }

    fn get_provider_ids(&self) -> Vec<String> {
        self.inner.authentication_providers.borrow().iter().map(|(_, provider)| provider.id()).collect()
    }

    fn get_accounts(&self, id: &str) -> Promise<Vec<AuthenticationSessionAccount>> {
        self.inner.get_accounts(id)
    }

    fn get_sessions(
        &self,
        id: &str,
        scope_list_or_request: Option<AuthenticationScopeListOrRequest>,
        options: Option<IAuthenticationGetSessionsOptions>,
        activate_immediate: Option<bool>,
    ) -> Promise<Vec<AuthenticationSession>> {
        self.inner.get_sessions(id, scope_list_or_request, options, activate_immediate.unwrap_or(false))
    }

    fn create_session(
        &self,
        provider_id: &str,
        scope_list_or_request: AuthenticationScopeListOrRequest,
        options: Option<IAuthenticationCreateSessionOptions>,
    ) -> Promise<AuthenticationSession> {
        self.inner.create_session(provider_id, scope_list_or_request, options)
    }
}

impl IAuthenticationServiceProviders for AuthenticationService {
    fn register_declared_authentication_provider(&self, provider: AuthenticationProviderInformation) {
        self.inner.register_declared_authentication_provider(provider);
    }

    fn unregister_declared_authentication_provider(&self, id: &str) {
        self.inner.unregister_declared_authentication_provider(id);
    }

    fn is_dynamic_authentication_provider(&self, id: &str) -> bool {
        self.inner.dynamic_authentication_provider_ids.borrow().contains(id)
    }

    fn register_authentication_provider(&self, id: &str, provider: Rc<dyn IAuthenticationProvider>) {
        self.inner.register_authentication_provider(id, provider);
    }

    fn unregister_authentication_provider(&self, id: &str) {
        self.inner.unregister_authentication_provider(id);
    }

    fn get_provider(&self, id: &str) -> Rc<dyn IAuthenticationProvider> {
        self.inner.get_provider(id)
    }

    fn remove_session(&self, provider_id: &str, session_id: &str) -> Promise<()> {
        self.inner.remove_session(provider_id, session_id)
    }

    fn get_or_activate_provider_id_for_server(&self, authorization_server: &URI, resource_server: Option<&URI>) -> Promise<Option<String>> {
        self.inner.get_or_activate_provider_id_for_server(authorization_server, resource_server)
    }

    fn register_authentication_provider_host_delegate(&self, delegate: Rc<dyn IAuthenticationProviderHostDelegate>) -> DisposableRef {
        self.inner.register_authentication_provider_host_delegate(delegate)
    }

    fn create_dynamic_authentication_provider(
        &self,
        authorization_server: &URI,
        server_metadata: &IAuthorizationServerMetadata,
        resource: Option<&IAuthorizationProtectedResourceMetadata>,
        client_id: Option<&str>,
        client_secret: Option<&str>,
    ) -> Promise<Option<Rc<dyn IAuthenticationProvider>>> {
        self.inner.create_dynamic_authentication_provider(authorization_server, server_metadata, resource, client_id, client_secret)
    }

    fn create_or_get_xaa_provider(&self, issuer: &URI) -> Promise<Option<String>> {
        self.inner.create_or_get_xaa_provider(issuer)
    }
}

impl IDisposable for AuthenticationService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
