//! Porte de `src/vs/workbench/services/environment/test/browser/environmentService.test.ts` (suíte `BrowserWorkbenchEnvironmentService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda do começo de cada caso.
//! - **`TestProductService`.** É o `ProductService::default()` do porte (sem `commit`, como o `product` do ambiente de teste);
//!   `{ ...TestProductService, commit: 'built' }` é o mesmo produto com `commit` preenchido.
//! - **`IWorkbenchConstructionOptions`.** É a [`WorkbenchConstructionOptions`] do porte (só o que o serviço lê, ver
//!   `browser_environment_service.rs`); o `open: async () => true` e o `workspace: undefined` do `workspaceProvider` não são lidos
//!   pelo serviço e não têm campo. O `payload` é a lista de pares `[string, string][]`.
//! - **`assert.deepStrictEqual({ ... }, { ... })`.** Compara os mesmos sete valores, um a um ([`assert_extension_host_state`]): o
//!   `undefined` é `None`, `{ port: null, break: false }` é o `ExtensionHostDebugParams` com `port` `None`, e o objeto `env` é
//!   comparado em profundidade com `assert_deep_strict_equal`. `URI.parse('file:///extension')` é `URI::parse`.
//! - **`extensionDevelopmentLocationKind: ['workspace']`.** É `Some(vec![ExtensionKind::Workspace])`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::{assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::dynamic::Dynamic;
use je_base::uri::URI;
use je_platform::environment::environment::ExtensionKind;
use je_platform::log::LogLevel;
use je_platform::product::{IProductService, ProductService};
use je_platform::resources::uri_file;
use je_workbench::services::environment::browser_environment_service::{
    BrowserWorkbenchEnvironmentService, DevelopmentOptions, WorkbenchConstructionOptions, WorkspaceProviderOptions,
};
use je_workbench::services::environment::environment_service::IWorkbenchEnvironmentService;
use je_platform::environment::environment::EnvironmentService;

//#region Utilitários dos casos

/// `TestProductService`.
fn test_product_service() -> Rc<dyn IProductService> {
    Rc::new(ProductService::default())
}

/// `{ ...TestProductService, commit: 'built' }`.
fn built_product_service() -> Rc<dyn IProductService> {
    Rc::new(ProductService { commit: Some("built".to_owned()), ..ProductService::default() })
}

/// `privilegedPayload`.
fn privileged_payload() -> Vec<(String, String)> {
    [
        ("extensionDevelopmentPath", "file:///extension".to_owned()),
        ("extensionDevelopmentKind", "workspace".to_owned()),
        ("extensionTestsPath", "file:///extension/test.js".to_owned()),
        ("debugRenderer", "true".to_owned()),
        ("debugId", "debug-id".to_owned()),
        ("inspect-brk-extensions", "1234".to_owned()),
        ("extensionEnvironment", r#"{"NODE_OPTIONS":"--import=data:text/javascript,"}"#.to_owned()),
        ("enableProposedApi", String::new()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value))
    .collect()
}

/// `workspaceProvider: { workspace: undefined, trusted: true, payload: privilegedPayload, open: async () => true }`.
fn privileged_workspace_provider() -> Option<WorkspaceProviderOptions> {
    Some(WorkspaceProviderOptions { payload: Some(privileged_payload()) })
}

/// O objeto do `assert.deepStrictEqual` dos três últimos casos, lido do serviço.
struct ExpectedExtensionHostState {
    port: Option<f64>,
    break_: bool,
    debug_id: Option<&'static str>,
    env: Option<Dynamic>,
    debug_renderer: bool,
    is_extension_development: bool,
    extension_development_location_uri: Option<Vec<URI>>,
    extension_development_location_kind: Option<Vec<ExtensionKind>>,
    extension_tests_location_uri: Option<URI>,
    extension_enabled_proposed_api: Option<Vec<String>>,
}

/// `assert.deepStrictEqual({ debugExtensionHost, debugRenderer, isExtensionDevelopment, extensionDevelopmentLocationURI,
/// extensionDevelopmentLocationKind, extensionTestsLocationURI, extensionEnabledProposedApi }, expected)`.
#[track_caller]
fn assert_extension_host_state(environment_service: &BrowserWorkbenchEnvironmentService, expected: ExpectedExtensionHostState) {
    let debug_extension_host = environment_service.debug_extension_host();
    assert_eq!(debug_extension_host.port, expected.port);
    assert_eq!(debug_extension_host.break_, expected.break_);
    assert_eq!(debug_extension_host.debug_id.as_deref(), expected.debug_id);
    match (&debug_extension_host.env, &expected.env) {
        (Some(actual), Some(expected)) => assert_deep_strict_equal(actual, expected),
        (None, None) => {}
        (actual, expected) => panic!("env difere\n  atual:    {actual:?}\n  esperado: {expected:?}"),
    }
    assert_eq!(environment_service.debug_renderer(), expected.debug_renderer);
    assert_eq!(environment_service.is_extension_development(), expected.is_extension_development);
    assert_eq!(environment_service.extension_development_location_uri(), expected.extension_development_location_uri);
    assert_eq!(environment_service.extension_development_kind(), expected.extension_development_location_kind);
    assert_eq!(environment_service.extension_tests_location_uri(), expected.extension_tests_location_uri);
    assert_eq!(environment_service.extension_enabled_proposed_api(), expected.extension_enabled_proposed_api);
}

/// `{ port: 1234, break: true, debugId: 'debug-id', env: { NODE_OPTIONS: '--import=data:text/javascript,' } }` e o restante do
/// estado aceito do `payload`.
fn accepted_extension_host_state() -> ExpectedExtensionHostState {
    ExpectedExtensionHostState {
        port: Some(1234.0),
        break_: true,
        debug_id: Some("debug-id"),
        env: Some(Dynamic::object([("NODE_OPTIONS", Dynamic::from("--import=data:text/javascript,"))])),
        debug_renderer: true,
        is_extension_development: true,
        extension_development_location_uri: Some(vec![URI::parse("file:///extension", false)]),
        extension_development_location_kind: Some(vec![ExtensionKind::Workspace]),
        extension_tests_location_uri: Some(URI::parse("file:///extension/test.js", false)),
        extension_enabled_proposed_api: Some(Vec::new()),
    }
}

//#endregion

#[test]
fn gets_enabled_extension_proposed_api_from_workbench_options() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let empty = BrowserWorkbenchEnvironmentService::new(
        "",
        uri_file("logs"),
        WorkbenchConstructionOptions { enabled_extension_proposed_api: Some(Vec::new()), ..WorkbenchConstructionOptions::default() },
        test_product_service(),
    );
    let populated = BrowserWorkbenchEnvironmentService::new(
        "",
        uri_file("logs"),
        WorkbenchConstructionOptions { enabled_extension_proposed_api: Some(vec!["publisher.extension".to_owned()]), ..WorkbenchConstructionOptions::default() },
        test_product_service(),
    );

    assert_eq!(
        (empty.extension_enabled_proposed_api(), populated.extension_enabled_proposed_api()),
        (Some(Vec::<String>::new()), Some(vec!["publisher.extension".to_owned()])),
    );
}

#[test]
fn ignores_extension_host_options_from_payload_in_production() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let environment_service = BrowserWorkbenchEnvironmentService::new(
        "",
        uri_file("/logs"),
        WorkbenchConstructionOptions {
            development_options: Some(DevelopmentOptions { log_level: Some(LogLevel::Info), ..DevelopmentOptions::default() }),
            workspace_provider: privileged_workspace_provider(),
            ..WorkbenchConstructionOptions::default()
        },
        built_product_service(),
    );

    assert_extension_host_state(
        &environment_service,
        ExpectedExtensionHostState {
            port: None,
            break_: false,
            debug_id: None,
            env: None,
            debug_renderer: false,
            is_extension_development: false,
            extension_development_location_uri: None,
            extension_development_location_kind: None,
            extension_tests_location_uri: None,
            extension_enabled_proposed_api: None,
        },
    );
}

#[test]
fn accepts_extension_host_options_from_payload_in_development() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let environment_service = BrowserWorkbenchEnvironmentService::new(
        "",
        uri_file("/logs"),
        WorkbenchConstructionOptions { workspace_provider: privileged_workspace_provider(), ..WorkbenchConstructionOptions::default() },
        test_product_service(),
    );

    assert_extension_host_state(&environment_service, accepted_extension_host_state());
}

#[test]
fn accepts_extension_host_options_from_payload_in_production_smoke_tests() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let environment_service = BrowserWorkbenchEnvironmentService::new(
        "",
        uri_file("/logs"),
        WorkbenchConstructionOptions {
            development_options: Some(DevelopmentOptions { enable_smoke_test_driver: Some(true), log_level: Some(LogLevel::Info), ..DevelopmentOptions::default() }),
            workspace_provider: privileged_workspace_provider(),
            ..WorkbenchConstructionOptions::default()
        },
        built_product_service(),
    );

    assert_extension_host_state(&environment_service, accepted_extension_host_state());
}
