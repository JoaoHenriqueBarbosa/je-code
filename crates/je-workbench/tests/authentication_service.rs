//! Porte de `src/vs/workbench/services/authentication/test/browser/authenticationService.test.ts` (suíte
//! `AuthenticationService` e suíte `AuthenticationService - tryActivateProvider`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso, e o `setup` é
//!   [`build_service`] (o `disposables.add(...)` do original é `add_rc`). O `teardown` (`dispose()` do serviço)
//!   roda no fim do caso; o guarda descarta o resto.
//! - `TestStorageService` é o `InMemoryStorageService`, `TestProductService` é o `ProductService` padrão e
//!   `NullLogService` é o `NullLogService`. `TestEnvironmentService` não aparece: o serviço só lê dele
//!   `options?.authenticationProviders`, que no `TestEnvironmentService` não existe, então o argumento estático
//!   do serviço (a lista de provedores do ambiente) é vazio (ver `authentication_service.rs`).
//! - `createProvider(overrides)` é [`create_provider`], com os membros sobrescritos como campos de [`Overrides`];
//!   os membros que o original deixa fixos (`supportsMultipleAccounts: false`, `onDidChangeSessions` de um
//!   `Emitter` próprio) são os do [`TestProvider`].
//! - Os casos `async` rodam sob [`run_with_faked_timers`]; `Event.toPromise(evento)` é `.to_promise(None)` e
//!   `await` é `.await`. `assert.rejects(fn, /regex/)` é `unwrap_err()` e a conferência da mensagem.
//! - `assert.deepEqual` sobre objetos e listas é `assert_eq!` (os tipos derivam `PartialEq`); sobre o provedor
//!   devolvido por `getProvider` é a identidade do `Rc` (o mesmo objeto).
//! - `new class extends TestExtensionService { override activateByEvent... }` é [`HangingExtensionService`],
//!   que repassa tudo ao `TestExtensionService` e deixa `activateByEvent` pendente para sempre.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::{Injectable, InstantiationService, ServiceCollection};
use je_platform::log::NullLogService;
use je_platform::product::{I_PRODUCT_SERVICE, IProductService};
use je_platform::storage::{I_STORAGE_SERVICE, InMemoryStorageService, StorageService};
use je_workbench::services::authentication::authentication::{
    AuthenticationProviderInformation, AuthenticationScopeListOrRequest, AuthenticationSession, AuthenticationSessionAccount,
    AuthenticationSessionsChange, AuthenticationSessionsChangeEvent, IAuthenticationCreateSessionOptions, IAuthenticationProvider,
    IAuthenticationProviderSessionOptions, IAuthenticationService, IAuthenticationServiceProviders,
};
use je_workbench::services::authentication::authentication_access_service::{AuthenticationAccessService, IAuthenticationAccessService};
use je_workbench::services::authentication::authentication_service::AuthenticationService;
use je_workbench::services::extensions::extensions::{ActivationKind, IExtensionService};
use je_workbench::test::browser::workbench_test_services::TestExtensionService;
use je_workbench::test::common::workbench_test_services::TestProductService;

fn create_session() -> AuthenticationSession {
    AuthenticationSession::new("session1", "token1", AuthenticationSessionAccount::new("Account", "account"), vec!["test".to_owned()])
}

/// `async () => [createSession()]`.
fn one_session() -> Promise<Vec<AuthenticationSession>> {
    Promise::resolved(vec![create_session()])
}

type GetSessionsFn = Rc<dyn Fn(Option<Vec<String>>, IAuthenticationProviderSessionOptions) -> Promise<Vec<AuthenticationSession>>>;
type CreateSessionFn = Rc<dyn Fn(Vec<String>, IAuthenticationProviderSessionOptions) -> Promise<AuthenticationSession>>;
type RemoveSessionFn = Rc<dyn Fn(&str) -> Promise<()>>;

/// `IAuthenticationProvider` do `createProvider`: um objeto literal, aqui com um fecho por membro.
struct TestProvider {
    id: String,
    label: String,
    resource_server: Option<URI>,
    authorization_servers: Option<Vec<URI>>,
    on_did_change_sessions: Event<AuthenticationSessionsChangeEvent>,
    get_sessions: GetSessionsFn,
    create_session: CreateSessionFn,
    remove_session: RemoveSessionFn,
}

impl IAuthenticationProvider for TestProvider {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn label(&self) -> String {
        self.label.clone()
    }

    fn resource_server(&self) -> Option<URI> {
        self.resource_server.clone()
    }

    fn authorization_servers(&self) -> Option<Vec<URI>> {
        self.authorization_servers.clone()
    }

    fn supports_multiple_accounts(&self) -> bool {
        false
    }

    fn on_did_change_sessions(&self) -> Event<AuthenticationSessionsChangeEvent> {
        self.on_did_change_sessions.clone()
    }

    fn get_sessions(&self, scopes: Option<Vec<String>>, options: IAuthenticationProviderSessionOptions) -> Promise<Vec<AuthenticationSession>> {
        (self.get_sessions)(scopes, options)
    }

    fn create_session(&self, scopes: Vec<String>, options: IAuthenticationProviderSessionOptions) -> Promise<AuthenticationSession> {
        (self.create_session)(scopes, options)
    }

    fn remove_session(&self, session_id: &str) -> Promise<()> {
        (self.remove_session)(session_id)
    }
}

/// `overrides: Partial<IAuthenticationProvider>`.
#[derive(Default)]
struct Overrides {
    id: Option<&'static str>,
    label: Option<&'static str>,
    resource_server: Option<URI>,
    authorization_servers: Option<Vec<URI>>,
    on_did_change_sessions: Option<Event<AuthenticationSessionsChangeEvent>>,
    get_sessions: Option<GetSessionsFn>,
    create_session: Option<CreateSessionFn>,
    remove_session: Option<RemoveSessionFn>,
}

fn create_provider(overrides: Overrides) -> Rc<TestProvider> {
    Rc::new(TestProvider {
        id: overrides.id.unwrap_or("test").to_owned(),
        label: overrides.label.unwrap_or("Test").to_owned(),
        resource_server: overrides.resource_server,
        authorization_servers: overrides.authorization_servers,
        on_did_change_sessions: overrides.on_did_change_sessions.unwrap_or_else(|| Emitter::<AuthenticationSessionsChangeEvent>::new().event()),
        get_sessions: overrides.get_sessions.unwrap_or_else(|| Rc::new(|_, _| Promise::resolved(Vec::new()))),
        create_session: overrides.create_session.unwrap_or_else(|| Rc::new(|_, _| Promise::resolved(create_session()))),
        remove_session: overrides.remove_session.unwrap_or_else(|| Rc::new(|_| Promise::resolved(()))),
    })
}

fn info(id: &str, label: &str) -> AuthenticationProviderInformation {
    AuthenticationProviderInformation { id: id.to_owned(), label: label.to_owned(), authorization_server_globs: None }
}

fn parse(value: &str) -> URI {
    URI::parse(value, false)
}

fn event(added: Vec<AuthenticationSession>, removed: Vec<AuthenticationSession>, changed: Vec<AuthenticationSession>) -> AuthenticationSessionsChangeEvent {
    AuthenticationSessionsChangeEvent { added: Some(added), removed: Some(removed), changed: Some(changed) }
}

/// O `setup` da suíte: o armazenamento, o `AuthenticationAccessService` e o `AuthenticationService` sobre o
/// serviço de extensões dado.
fn build_service(disposables: &LeakGuard, extension_service: Rc<dyn IExtensionService>) -> Rc<AuthenticationService> {
    let storage_service = disposables.add_rc(InMemoryStorageService::new());
    let storage: Rc<dyn StorageService> = Rc::new(storage_service.clone());
    let product: Rc<dyn IProductService> = Rc::new(TestProductService::default());
    let instantiation_service =
        disposables.add_rc(InstantiationService::new(ServiceCollection::new().with(I_STORAGE_SERVICE, storage).with(I_PRODUCT_SERVICE, product)));
    let authentication_access_service: Rc<dyn IAuthenticationAccessService> =
        disposables.add_rc(instantiation_service.create_instance::<AuthenticationAccessService>(()));
    disposables.add_rc(AuthenticationService::create(Vec::new(), (extension_service, authentication_access_service, NullLogService::shared())))
}

fn setup(disposables: &LeakGuard) -> Rc<AuthenticationService> {
    build_service(disposables, TestExtensionService::new())
}

/// Um caso síncrono: `setup`, o corpo e o `teardown`.
fn sync_test(body: impl FnOnce(Rc<AuthenticationService>)) {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let authentication_service = setup(&disposables);
    body(authentication_service.clone());
    // Dispose the authentication service after each test
    authentication_service.dispose();
}

/// Um caso `async`: `setup`, o corpo sob o relógio virtual e o `teardown`.
fn async_test<Fut: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<AuthenticationService>) -> Fut) {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let authentication_service = setup(&disposables);
    let teardown = authentication_service.clone();
    let future = body(authentication_service);
    run_with_faked_timers(async move {
        future.await;
        // Dispose the authentication service after each test
        teardown.dispose();
    });
}

fn scopes(values: &[&str]) -> Option<AuthenticationScopeListOrRequest> {
    Some(AuthenticationScopeListOrRequest::Scopes(values.iter().map(|value| (*value).to_owned()).collect()))
}

// suite('declaredAuthenticationProviders')

#[test]
fn declared_authentication_providers_register_declared_authentication_provider() {
    async_test(|authentication_service| async move {
        let changed = authentication_service.on_did_change_declared_providers().to_promise(None);
        let provider = info("github", "GitHub");
        authentication_service.register_declared_authentication_provider(provider.clone());

        // Assert that the provider is added to the declaredProviders array and the event fires
        assert_eq!(authentication_service.declared_providers().len(), 1);
        assert_eq!(authentication_service.declared_providers()[0], provider);
        changed.await.unwrap();
    });
}

#[test]
fn declared_authentication_providers_unregister_declared_authentication_provider() {
    async_test(|authentication_service| async move {
        let provider = info("github", "GitHub");
        authentication_service.register_declared_authentication_provider(provider.clone());
        let changed = authentication_service.on_did_change_declared_providers().to_promise(None);
        authentication_service.unregister_declared_authentication_provider(&provider.id);

        // Assert that the provider is removed from the declaredProviders array and the event fires
        assert_eq!(authentication_service.declared_providers().len(), 0);
        changed.await.unwrap();
    });
}

// suite('authenticationProviders')

#[test]
fn authentication_providers_is_authentication_provider_registered() {
    async_test(|authentication_service| async move {
        let registered = authentication_service.on_did_register_authentication_provider().to_promise(None);
        let provider = create_provider(Overrides::default());
        assert!(!authentication_service.is_authentication_provider_registered(&provider.id));
        authentication_service.register_authentication_provider(&provider.id, provider.clone());
        assert!(authentication_service.is_authentication_provider_registered(&provider.id));
        let result = registered.await.unwrap();
        assert_eq!(result, info(&provider.id, &provider.label));
    });
}

#[test]
fn authentication_providers_unregister_authentication_provider() {
    async_test(|authentication_service| async move {
        let unregistered = authentication_service.on_did_unregister_authentication_provider().to_promise(None);
        let provider = create_provider(Overrides::default());
        authentication_service.register_authentication_provider(&provider.id, provider.clone());
        assert!(authentication_service.is_authentication_provider_registered(&provider.id));
        authentication_service.unregister_authentication_provider(&provider.id);
        assert!(!authentication_service.is_authentication_provider_registered(&provider.id));
        let result = unregistered.await.unwrap();
        assert_eq!(result, info(&provider.id, &provider.label));
    });
}

#[test]
fn authentication_providers_get_provider_ids() {
    sync_test(|authentication_service| {
        let provider1 = create_provider(Overrides { id: Some("provider1"), label: Some("Provider 1"), ..Overrides::default() });
        let provider2 = create_provider(Overrides { id: Some("provider2"), label: Some("Provider 2"), ..Overrides::default() });

        authentication_service.register_authentication_provider(&provider1.id, provider1.clone());
        authentication_service.register_authentication_provider(&provider2.id, provider2.clone());

        let provider_ids = authentication_service.get_provider_ids();

        // Assert that the providerIds array contains the registered provider ids
        assert_eq!(provider_ids, vec![provider1.id.clone(), provider2.id.clone()]);
    });
}

#[test]
fn authentication_providers_get_provider() {
    sync_test(|authentication_service| {
        let provider = create_provider(Overrides::default());

        authentication_service.register_authentication_provider(&provider.id, provider.clone());

        let retrieved_provider = authentication_service.get_provider(&provider.id);

        // Assert that the retrieved provider is the same as the registered provider
        assert!(Rc::as_ptr(&retrieved_provider).cast::<()>() == Rc::as_ptr(&provider).cast::<()>());
    });
}

#[test]
fn authentication_providers_get_or_activate_provider_id_for_server_should_return_undefined_when_no_provider_matches_the_authorization_server() {
    async_test(|authentication_service| async move {
        let authorization_server = parse("https://example.com");
        let result = authentication_service.get_or_activate_provider_id_for_server(&authorization_server, None).await.unwrap();
        assert_eq!(result, None);
    });
}

#[test]
fn authentication_providers_get_or_activate_provider_id_for_server_should_return_provider_id_if_authorization_server_globs_matches_and_authorization_servers_match() {
    async_test(|authentication_service| async move {
        // Register a declared provider with an authorization server glob
        let provider = AuthenticationProviderInformation {
            id: "github".to_owned(),
            label: "GitHub".to_owned(),
            authorization_server_globs: Some(vec!["https://github.com/*".to_owned()]),
        };
        authentication_service.register_declared_authentication_provider(provider);

        // Register an authentication provider with matching authorization servers
        let auth_provider = create_provider(Overrides {
            id: Some("github"),
            label: Some("GitHub"),
            authorization_servers: Some(vec![parse("https://github.com/login")]),
            ..Overrides::default()
        });
        authentication_service.register_authentication_provider("github", auth_provider);

        // Test with a matching URI
        let authorization_server = parse("https://github.com/login");
        let result = authentication_service.get_or_activate_provider_id_for_server(&authorization_server, None).await.unwrap();

        // Verify the result
        assert_eq!(result, Some("github".to_owned()));
    });
}

#[test]
fn authentication_providers_get_or_activate_provider_id_for_server_should_return_undefined_if_authorization_server_globs_match_but_authorization_servers_do_not_match() {
    async_test(|authentication_service| async move {
        // Register a declared provider with an authorization server glob
        let provider = AuthenticationProviderInformation {
            id: "github".to_owned(),
            label: "GitHub".to_owned(),
            authorization_server_globs: Some(vec!["https://github.com/*".to_owned()]),
        };
        authentication_service.register_declared_authentication_provider(provider);

        // Register an authentication provider with non-matching authorization servers
        let auth_provider = create_provider(Overrides {
            id: Some("github"),
            label: Some("GitHub"),
            authorization_servers: Some(vec![parse("https://github.com/different")]),
            ..Overrides::default()
        });
        authentication_service.register_authentication_provider("github", auth_provider);

        // Test with a non-matching URI
        let authorization_server = parse("https://github.com/login");
        let result = authentication_service.get_or_activate_provider_id_for_server(&authorization_server, None).await.unwrap();

        // Verify the result
        assert_eq!(result, None);
    });
}

#[test]
fn authentication_providers_get_or_activate_provider_id_for_authorization_server_should_check_multiple_providers_and_return_the_first_match() {
    async_test(|authentication_service| async move {
        // Register two declared providers with authorization server globs
        let provider1 = AuthenticationProviderInformation {
            id: "github".to_owned(),
            label: "GitHub".to_owned(),
            authorization_server_globs: Some(vec!["https://github.com/*".to_owned()]),
        };
        let provider2 = AuthenticationProviderInformation {
            id: "microsoft".to_owned(),
            label: "Microsoft".to_owned(),
            authorization_server_globs: Some(vec!["https://login.microsoftonline.com/*".to_owned()]),
        };
        authentication_service.register_declared_authentication_provider(provider1);
        authentication_service.register_declared_authentication_provider(provider2);

        // Register authentication providers
        let github_provider = create_provider(Overrides {
            id: Some("github"),
            label: Some("GitHub"),
            authorization_servers: Some(vec![parse("https://github.com/different")]),
            ..Overrides::default()
        });
        authentication_service.register_authentication_provider("github", github_provider);

        let microsoft_provider = create_provider(Overrides {
            id: Some("microsoft"),
            label: Some("Microsoft"),
            authorization_servers: Some(vec![parse("https://login.microsoftonline.com/common")]),
            ..Overrides::default()
        });
        authentication_service.register_authentication_provider("microsoft", microsoft_provider);

        // Test with a URI that should match the second provider
        let authorization_server = parse("https://login.microsoftonline.com/common");
        let result = authentication_service.get_or_activate_provider_id_for_server(&authorization_server, None).await.unwrap();

        // Verify the result
        assert_eq!(result, Some("microsoft".to_owned()));
    });
}

#[test]
fn authentication_providers_get_or_activate_provider_id_for_server_should_match_when_resource_server_matches_provider_resource_server() {
    async_test(|authentication_service| async move {
        let authorization_server = parse("https://login.microsoftonline.com/common");
        let resource_server = parse("https://graph.microsoft.com");

        // Register an authentication provider with a resourceServer
        let auth_provider = create_provider(Overrides {
            id: Some("microsoft"),
            label: Some("Microsoft"),
            authorization_servers: Some(vec![authorization_server.clone()]),
            resource_server: Some(resource_server.clone()),
            ..Overrides::default()
        });
        authentication_service.register_authentication_provider("microsoft", auth_provider);

        // Test with matching authorization server and resource server
        let result = authentication_service.get_or_activate_provider_id_for_server(&authorization_server, Some(&resource_server)).await.unwrap();

        // Verify the result
        assert_eq!(result, Some("microsoft".to_owned()));
    });
}

#[test]
fn authentication_providers_get_or_activate_provider_id_for_server_should_not_match_when_resource_server_does_not_match_provider_resource_server() {
    async_test(|authentication_service| async move {
        let authorization_server = parse("https://login.microsoftonline.com/common");
        let resource_server = parse("https://graph.microsoft.com");
        let different_resource_server = parse("https://vault.azure.net");

        // Register an authentication provider with a resourceServer
        let auth_provider = create_provider(Overrides {
            id: Some("microsoft"),
            label: Some("Microsoft"),
            authorization_servers: Some(vec![authorization_server.clone()]),
            resource_server: Some(resource_server),
            ..Overrides::default()
        });
        authentication_service.register_authentication_provider("microsoft", auth_provider);

        // Test with matching authorization server but different resource server
        let result =
            authentication_service.get_or_activate_provider_id_for_server(&authorization_server, Some(&different_resource_server)).await.unwrap();

        // Verify the result - should not match because resource servers don't match
        assert_eq!(result, None);
    });
}

#[test]
fn authentication_providers_get_or_activate_provider_id_for_server_should_match_when_provider_has_no_resource_server_and_resource_server_is_provided() {
    async_test(|authentication_service| async move {
        let authorization_server = parse("https://login.microsoftonline.com/common");
        let resource_server = parse("https://graph.microsoft.com");

        // Register an authentication provider without a resourceServer
        let auth_provider = create_provider(Overrides {
            id: Some("microsoft"),
            label: Some("Microsoft"),
            authorization_servers: Some(vec![authorization_server.clone()]),
            ..Overrides::default()
        });
        authentication_service.register_authentication_provider("microsoft", auth_provider);

        // Test with matching authorization server and a resource server
        // Should match because provider has no resourceServer defined
        let result = authentication_service.get_or_activate_provider_id_for_server(&authorization_server, Some(&resource_server)).await.unwrap();

        // Verify the result
        assert_eq!(result, Some("microsoft".to_owned()));
    });
}

#[test]
fn authentication_providers_get_or_activate_provider_id_for_server_should_match_when_provider_has_resource_server_but_no_resource_server_is_provided() {
    async_test(|authentication_service| async move {
        let authorization_server = parse("https://login.microsoftonline.com/common");
        let resource_server = parse("https://graph.microsoft.com");

        // Register an authentication provider with a resourceServer
        let auth_provider = create_provider(Overrides {
            id: Some("microsoft"),
            label: Some("Microsoft"),
            authorization_servers: Some(vec![authorization_server.clone()]),
            resource_server: Some(resource_server),
            ..Overrides::default()
        });
        authentication_service.register_authentication_provider("microsoft", auth_provider);

        // Test with matching authorization server but no resource server provided
        // Should match because no resourceServer is provided to check against
        let result = authentication_service.get_or_activate_provider_id_for_server(&authorization_server, None).await.unwrap();

        // Verify the result
        assert_eq!(result, Some("microsoft".to_owned()));
    });
}

#[test]
fn authentication_providers_get_or_activate_provider_id_for_server_should_distinguish_between_providers_with_same_authorization_server_but_different_resource_servers() {
    async_test(|authentication_service| async move {
        let authorization_server = parse("https://login.microsoftonline.com/common");
        let graph_resource_server = parse("https://graph.microsoft.com");
        let vault_resource_server = parse("https://vault.azure.net");

        // Register first provider with Graph resource server
        let graph_provider = create_provider(Overrides {
            id: Some("microsoft-graph"),
            label: Some("Microsoft Graph"),
            authorization_servers: Some(vec![authorization_server.clone()]),
            resource_server: Some(graph_resource_server.clone()),
            ..Overrides::default()
        });
        authentication_service.register_authentication_provider("microsoft-graph", graph_provider);

        // Register second provider with Vault resource server
        let vault_provider = create_provider(Overrides {
            id: Some("microsoft-vault"),
            label: Some("Microsoft Vault"),
            authorization_servers: Some(vec![authorization_server.clone()]),
            resource_server: Some(vault_resource_server.clone()),
            ..Overrides::default()
        });
        authentication_service.register_authentication_provider("microsoft-vault", vault_provider);

        // Test with Graph resource server - should match the first provider
        let graph_result = authentication_service.get_or_activate_provider_id_for_server(&authorization_server, Some(&graph_resource_server)).await.unwrap();
        assert_eq!(graph_result, Some("microsoft-graph".to_owned()));

        // Test with Vault resource server - should match the second provider
        let vault_result = authentication_service.get_or_activate_provider_id_for_server(&authorization_server, Some(&vault_resource_server)).await.unwrap();
        assert_eq!(vault_result, Some("microsoft-vault".to_owned()));

        // Test with different resource server - should not match either
        let other_resource_server = parse("https://storage.azure.com");
        let no_match_result = authentication_service.get_or_activate_provider_id_for_server(&authorization_server, Some(&other_resource_server)).await.unwrap();
        assert_eq!(no_match_result, None);
    });
}

// suite('authenticationSessions')

#[test]
fn authentication_sessions_get_sessions_base_case() {
    async_test(|authentication_service| async move {
        let is_called = Rc::new(Cell::new(false));
        let called = is_called.clone();
        let provider = create_provider(Overrides {
            get_sessions: Some(Rc::new(move |_, _| {
                called.set(true);
                Promise::resolved(vec![create_session()])
            })),
            ..Overrides::default()
        });
        authentication_service.register_authentication_provider(&provider.id, provider.clone());
        let sessions = authentication_service.get_sessions(&provider.id, None, None, None).await.unwrap();

        assert_eq!(sessions.len(), 1);
        assert!(is_called.get());
    });
}

#[test]
fn authentication_sessions_get_sessions_authorization_server_is_not_registered() {
    async_test(|authentication_service| async move {
        let is_called = Rc::new(Cell::new(false));
        let called = is_called.clone();
        let provider = create_provider(Overrides {
            get_sessions: Some(Rc::new(move |_, _| {
                called.set(true);
                Promise::resolved(vec![create_session()])
            })),
            ..Overrides::default()
        });
        authentication_service.register_authentication_provider(&provider.id, provider.clone());
        let options = IAuthenticationProviderSessionOptions { authorization_server: Some(parse("https://example.com")), ..Default::default() };
        let error = authentication_service.get_sessions(&provider.id, scopes(&[]), Some(options), None).await.unwrap_err();
        assert!(error.message.contains("does not support the authorization server"), "mensagem inesperada: {}", error.message);
        assert!(!is_called.get());
    });
}

#[test]
fn authentication_sessions_create_session() {
    async_test(|authentication_service| async move {
        let emitter = Emitter::<AuthenticationSessionsChangeEvent>::new();
        let firing = emitter.clone();
        let provider = create_provider(Overrides {
            on_did_change_sessions: Some(emitter.event()),
            create_session: Some(Rc::new(move |_, _| {
                let session = create_session();
                firing.fire(event(vec![session.clone()], Vec::new(), Vec::new()));
                Promise::resolved(session)
            })),
            ..Overrides::default()
        });
        let changed = authentication_service.on_did_change_sessions().to_promise(None);
        authentication_service.register_authentication_provider(&provider.id, provider.clone());
        let session = authentication_service
            .create_session(&provider.id, AuthenticationScopeListOrRequest::Scopes(vec!["repo".to_owned()]), None)
            .await
            .unwrap();

        // Assert that the created session matches the expected session and the event fires
        let result = changed.await.unwrap();
        assert_eq!(
            result,
            AuthenticationSessionsChange {
                provider_id: provider.id.clone(),
                label: provider.label.clone(),
                event: event(vec![session], Vec::new(), Vec::new())
            }
        );
    });
}

#[test]
fn authentication_sessions_get_sessions_forwards_resource_option_to_provider() {
    async_test(|authentication_service| async move {
        let received_resource: Rc<RefCell<Option<String>>> = Rc::default();
        let received = received_resource.clone();
        let provider = create_provider(Overrides {
            get_sessions: Some(Rc::new(move |_scopes, options| {
                *received.borrow_mut() = options.resource.clone();
                Promise::resolved(vec![create_session()])
            })),
            ..Overrides::default()
        });
        authentication_service.register_authentication_provider(&provider.id, provider.clone());
        let options = IAuthenticationProviderSessionOptions { resource: Some("https://api.example.com/".to_owned()), ..Default::default() };
        authentication_service.get_sessions(&provider.id, scopes(&["scope"]), Some(options), None).await.unwrap();

        assert_eq!(*received_resource.borrow(), Some("https://api.example.com/".to_owned()));
    });
}

#[test]
fn authentication_sessions_create_session_forwards_resource_option_to_provider() {
    async_test(|authentication_service| async move {
        let received_resource: Rc<RefCell<Option<String>>> = Rc::default();
        let received = received_resource.clone();
        let provider = create_provider(Overrides {
            create_session: Some(Rc::new(move |_scopes, options| {
                *received.borrow_mut() = options.resource.clone();
                Promise::resolved(create_session())
            })),
            ..Overrides::default()
        });
        authentication_service.register_authentication_provider(&provider.id, provider.clone());
        let options = IAuthenticationCreateSessionOptions {
            session_options: IAuthenticationProviderSessionOptions { resource: Some("https://api.example.com/".to_owned()), ..Default::default() },
            activate_immediate: None,
        };
        authentication_service
            .create_session(&provider.id, AuthenticationScopeListOrRequest::Scopes(vec!["scope".to_owned()]), Some(options))
            .await
            .unwrap();

        assert_eq!(*received_resource.borrow(), Some("https://api.example.com/".to_owned()));
    });
}

#[test]
fn authentication_sessions_remove_session() {
    async_test(|authentication_service| async move {
        let emitter = Emitter::<AuthenticationSessionsChangeEvent>::new();
        let session = create_session();
        let firing = emitter.clone();
        let removed = session.clone();
        let provider = create_provider(Overrides {
            on_did_change_sessions: Some(emitter.event()),
            remove_session: Some(Rc::new(move |_| {
                firing.fire(event(Vec::new(), vec![removed.clone()], Vec::new()));
                Promise::resolved(())
            })),
            ..Overrides::default()
        });
        let changed = authentication_service.on_did_change_sessions().to_promise(None);
        authentication_service.register_authentication_provider(&provider.id, provider.clone());
        authentication_service.remove_session(&provider.id, &session.id).await.unwrap();

        let result = changed.await.unwrap();
        assert_eq!(
            result,
            AuthenticationSessionsChange {
                provider_id: provider.id.clone(),
                label: provider.label.clone(),
                event: event(Vec::new(), vec![session], Vec::new())
            }
        );
    });
}

#[test]
fn authentication_sessions_on_did_change_sessions() {
    async_test(|authentication_service| async move {
        let emitter = Emitter::<AuthenticationSessionsChangeEvent>::new();
        let provider = create_provider(Overrides {
            on_did_change_sessions: Some(emitter.event()),
            get_sessions: Some(Rc::new(|_, _| Promise::resolved(Vec::new()))),
            ..Overrides::default()
        });
        authentication_service.register_authentication_provider(&provider.id, provider.clone());

        let changed = authentication_service.on_did_change_sessions().to_promise(None);
        let session = create_session();
        emitter.fire(event(Vec::new(), Vec::new(), vec![session.clone()]));

        let result = changed.await.unwrap();
        assert_eq!(
            result,
            AuthenticationSessionsChange {
                provider_id: provider.id.clone(),
                label: provider.label.clone(),
                event: event(Vec::new(), Vec::new(), vec![session])
            }
        );
    });
}

// suite('AuthenticationService - tryActivateProvider')

/// `new class extends TestExtensionService { override activateByEvent(...) { return new Promise(() => {}); } }`.
struct HangingExtensionService {
    inner: Rc<TestExtensionService>,
}

impl IExtensionService for HangingExtensionService {
    /// Simulate a deadlocked extension host: activateByEvent never resolves.
    fn activate_by_event(&self, _activation_event: &str, _activation_kind: Option<ActivationKind>) -> Promise<()> {
        Promise::with_executor(|_resolver| { /* never resolves */ })
    }

    fn activation_event_is_done(&self, activation_event: &str) -> bool {
        self.inner.activation_event_is_done(activation_event)
    }

    fn get_extension(&self, id: &str) -> Promise<Option<je_workbench::services::extensions::extensions::IExtensionDescription>> {
        self.inner.get_extension(id)
    }

    fn on_did_register_extensions(&self) -> Event<()> {
        self.inner.on_did_register_extensions()
    }

    fn when_installed_extensions_registered(&self) -> Promise<bool> {
        self.inner.when_installed_extensions_registered()
    }

    fn extensions(&self) -> Vec<je_extensions::scanner::ExtensionDescription> {
        self.inner.extensions()
    }
}

#[test]
fn try_activate_provider_should_resolve_when_provider_registers_even_if_activate_by_event_never_resolves_315841() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let first_service = setup(&disposables);

    // Dispose the service created in setup to release the extension point handler,
    // so we can create a new one with a hanging activateByEvent.
    first_service.dispose();

    // Simulate a deadlocked extension host: activateByEvent never resolves.
    let hanging_ext_service = Rc::new(HangingExtensionService { inner: TestExtensionService::new() });
    let authentication_service = build_service(&disposables, hanging_ext_service);

    let provider = create_provider(Overrides { get_sessions: Some(Rc::new(|_, _| one_session())), ..Overrides::default() });

    let teardown = authentication_service.clone();
    run_with_faked_timers(async move {
        // Start getSessions: this calls tryActivateProvider which fires activateByEvent.
        // Since activateByEvent never resolves, the old code would deadlock here.
        let sessions_promise = authentication_service.get_sessions(&provider.id, None, None, None);

        // Simulate the local extension host registering the provider
        // while the webworker host is still stuck.
        authentication_service.register_authentication_provider(&provider.id, provider.clone());

        // The Promise.race in tryActivateProvider should unblock immediately.
        let sessions = sessions_promise.await.unwrap();
        assert_eq!(sessions.len(), 1);
        teardown.dispose();
    });
}

#[test]
fn try_activate_provider_should_resolve_when_activate_by_event_completes_and_provider_is_already_registered() {
    async_test(|authentication_service| async move {
        let provider = create_provider(Overrides { get_sessions: Some(Rc::new(|_, _| one_session())), ..Overrides::default() });
        authentication_service.register_authentication_provider(&provider.id, provider.clone());

        let sessions = authentication_service.get_sessions(&provider.id, None, None, None).await.unwrap();
        assert_eq!(sessions.len(), 1);
    });
}
