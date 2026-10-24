//! Porte de `src/vs/workbench/services/extensions/test/common/extensionManifestPropertiesService.test.ts` (suítes
//! `ExtensionManifestPropertiesService - ExtensionKind`, `... - SessionsWindowSupport` e
//! `... - ExtensionUntrustedWorkspaceSupportType`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso das duas primeiras
//!   suítes; o `DisposableStore` do `setup` é o do guarda (`guard.add_rc`). A terceira suíte, como o original, não
//!   confere vazamento.
//! - O `teardown` de `testObject.dispose()` das duas últimas suítes é o `Drop` do fixture da suíte.
//! - `<IExtensionManifest>{ ... }` e `Object.create({ ... })` viram o JSON do manifesto lido por
//!   `ExtensionManifest::from_value` (o `Object.create` só punha as propriedades no protótipo, e o serviço as lê do
//!   mesmo jeito).
//! - `TestInstantiationService.stub(IProductService, ...)`, `stub(IConfigurationService, ...)` e `createInstance`
//!   viram a construção direta do serviço com os serviços de teste: os stubs de um trait inteiro não existem em
//!   Rust, e o `stub(IProductService, { extensionUntrustedWorkspaceSupport })` é o `ProductService` padrão com o
//!   campo preenchido.
//! - `isWeb` é `Platform::detect().is_web`. A terceira suíte, que o original só define fora da web, fica sob
//!   `cfg(not(target_arch = "wasm32"))`.
//! - Os testes `async` (`await setUserConfiguration(...)`) são síncronos: a promessa do
//!   `TestConfigurationService` já nasce resolvida.
//! - O `for (const isBuiltin of [false, true])` gera dois casos de cada `test(...)`, um por valor de `isBuiltin`.
//! - `assert.strictEqual(x, 'limited')` compara com `BoolOrLimited::Limited("limited")`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::platform::Platform;
use je_extensions::manifest::{ExtensionKind, ExtensionManifest};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::log::NullLogService;
use je_workbench::services::extensions::extension_manifest_properties_service::{ExtensionManifestPropertiesService, IExtensionManifestPropertiesService};
use serde_json::{Map, Value, json};
use support::workbench_test_services::{TestWorkspaceTrustEnablementService, test_product_service};

/// `isWeb`.
fn is_web() -> bool {
    Platform::detect().is_web
}

/// `isWeb ? ['workspace', 'web'] : ['workspace']`.
fn workspace_kinds() -> Vec<ExtensionKind> {
    if is_web() { vec![ExtensionKind::Workspace, ExtensionKind::Web] } else { vec![ExtensionKind::Workspace] }
}

/// O valor JSON como valor dinâmico (o objeto de configuração que o `TestConfigurationService` guarda).
fn dynamic_of(value: &Value) -> Dynamic {
    match value {
        Value::Null => Dynamic::Null,
        Value::Bool(boolean) => Dynamic::Bool(*boolean),
        Value::Number(number) => Dynamic::Number(number.as_f64().expect("número finito")),
        Value::String(text) => Dynamic::string(text),
        Value::Array(items) => Dynamic::array(items.iter().map(dynamic_of)),
        Value::Object(entries) => Dynamic::object(entries.iter().map(|(key, entry)| (key.as_str(), dynamic_of(entry)))),
    }
}

/// `<IExtensionManifest>{ ... }`.
fn manifest(properties: Value) -> ExtensionManifest {
    ExtensionManifest::from_value(properties)
}

/// `getExtensionManifest(properties)`: `{ name: 'a', publisher: 'pub', version: '1.0.0', ...properties }`.
fn get_extension_manifest(properties: Value) -> ExtensionManifest {
    let Value::Object(properties) = properties else { panic!("as propriedades do manifesto são um objeto") };
    let mut manifest_properties = Map::new();
    manifest_properties.insert("name".to_owned(), json!("a"));
    manifest_properties.insert("publisher".to_owned(), json!("pub"));
    manifest_properties.insert("version".to_owned(), json!("1.0.0"));
    manifest_properties.extend(properties);
    manifest(Value::Object(manifest_properties))
}

/// `disposables.add(new ExtensionManifestPropertiesService(TestProductService, configurationService,
/// new TestWorkspaceTrustEnablementService(), new NullLogService()))`.
fn create_test_object(guard: &LeakGuard, configuration_service: Rc<TestConfigurationService>) -> Rc<ExtensionManifestPropertiesService> {
    guard.add_rc(ExtensionManifestPropertiesService::new(
        test_product_service(),
        configuration_service,
        TestWorkspaceTrustEnablementService::new(true),
        NullLogService::shared(),
    ))
}

mod extension_kind {
    use super::*;

    /// O `setup` da suíte `ExtensionKind`.
    fn setup() -> (LeakGuard, Rc<ExtensionManifestPropertiesService>) {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let test_object = create_test_object(&guard, TestConfigurationService::new(None));
        (guard, test_object)
    }

    #[test]
    fn declarative_with_extension_dependencies() {
        let (_guard, test_object) = setup();
        assert_eq!(test_object.get_extension_kind(&manifest(json!({ "extensionDependencies": ["ext1"] }))), workspace_kinds());
    }

    #[test]
    fn declarative_extension_pack() {
        let (_guard, test_object) = setup();
        assert_eq!(test_object.get_extension_kind(&manifest(json!({ "extensionPack": ["ext1", "ext2"] }))), workspace_kinds());
    }

    #[test]
    fn declarative_extension_pack_and_extension_dependencies() {
        let (_guard, test_object) = setup();
        assert_eq!(
            test_object.get_extension_kind(&manifest(json!({ "extensionPack": ["ext1", "ext2"], "extensionDependencies": ["ext1", "ext2"] }))),
            workspace_kinds()
        );
    }

    #[test]
    fn declarative_with_unknown_contribution_point_workspace_web_in_web_and_workspace_in_desktop() {
        let (_guard, test_object) = setup();
        assert_eq!(test_object.get_extension_kind(&manifest(json!({ "contributes": { "unknownPoint": { "something": true } } }))), workspace_kinds());
    }

    #[test]
    fn declarative_extension_pack_with_unknown_contribution_point() {
        let (_guard, test_object) = setup();
        assert_eq!(
            test_object.get_extension_kind(&manifest(json!({ "extensionPack": ["ext1", "ext2"], "contributes": { "unknownPoint": { "something": true } } }))),
            workspace_kinds()
        );
    }

    #[test]
    fn simple_declarative_ui_workspace_web() {
        let (_guard, test_object) = setup();
        assert_eq!(test_object.get_extension_kind(&manifest(json!({}))), vec![ExtensionKind::Ui, ExtensionKind::Workspace, ExtensionKind::Web]);
    }

    #[test]
    fn only_browser_web() {
        let (_guard, test_object) = setup();
        assert_eq!(test_object.get_extension_kind(&manifest(json!({ "browser": "main.browser.js" }))), vec![ExtensionKind::Web]);
    }

    #[test]
    fn only_main_workspace() {
        let (_guard, test_object) = setup();
        assert_eq!(test_object.get_extension_kind(&manifest(json!({ "main": "main.js" }))), vec![ExtensionKind::Workspace]);
    }

    #[test]
    fn main_and_browser_workspace_web_in_web_and_workspace_in_desktop() {
        let (_guard, test_object) = setup();
        assert_eq!(test_object.get_extension_kind(&manifest(json!({ "main": "main.js", "browser": "main.browser.js" }))), workspace_kinds());
    }

    #[test]
    fn browser_entry_point_with_workspace_extension_kind_workspace_web_in_web_and_workspace_in_desktop() {
        let (_guard, test_object) = setup();
        assert_eq!(
            test_object.get_extension_kind(&manifest(json!({ "main": "main.js", "browser": "main.browser.js", "extensionKind": ["workspace"] }))),
            workspace_kinds()
        );
    }

    #[test]
    fn only_browser_entry_point_with_out_extension_kind_web() {
        let (_guard, test_object) = setup();
        assert_eq!(test_object.get_extension_kind(&manifest(json!({ "browser": "main.browser.js" }))), vec![ExtensionKind::Web]);
    }

    #[test]
    fn simple_descriptive_with_workspace_ui_extension_kind_workspace_ui_web_in_web_and_workspace_ui_in_desktop() {
        let (_guard, test_object) = setup();
        let mut expected = vec![ExtensionKind::Workspace, ExtensionKind::Ui];
        if is_web() {
            expected.push(ExtensionKind::Web);
        }
        assert_eq!(test_object.get_extension_kind(&manifest(json!({ "extensionKind": ["workspace", "ui"] }))), expected);
    }

    #[test]
    fn opt_out_from_web_through_settings_even_if_it_can_run_in_web() {
        let (guard, _) = setup();
        let configuration = TestConfigurationService::new(Some(dynamic_of(&json!({ "remote": { "extensionKind": { "pub.a": ["-web"] } } }))));
        let test_object = create_test_object(&guard, configuration);
        assert_eq!(
            test_object.get_extension_kind(&manifest(json!({ "browser": "main.browser.js", "publisher": "pub", "name": "a" }))),
            vec![ExtensionKind::Ui, ExtensionKind::Workspace]
        );
    }

    #[test]
    fn opt_out_from_web_and_include_only_workspace_through_settings_even_if_it_can_run_in_web() {
        let (guard, _) = setup();
        let configuration = TestConfigurationService::new(Some(dynamic_of(&json!({ "remote": { "extensionKind": { "pub.a": ["-web", "workspace"] } } }))));
        let test_object = create_test_object(&guard, configuration);
        assert_eq!(
            test_object.get_extension_kind(&manifest(json!({ "browser": "main.browser.js", "publisher": "pub", "name": "a" }))),
            vec![ExtensionKind::Workspace]
        );
    }

    #[test]
    fn extension_cannot_opt_out_from_web() {
        let (_guard, test_object) = setup();
        assert_eq!(test_object.get_extension_kind(&manifest(json!({ "browser": "main.browser.js", "extensionKind": ["-web"] }))), vec![ExtensionKind::Web]);
    }

    #[test]
    fn extension_cannot_opt_into_web() {
        let (_guard, test_object) = setup();
        assert_eq!(
            test_object.get_extension_kind(&manifest(json!({ "main": "main.js", "extensionKind": ["web", "workspace", "ui"] }))),
            vec![ExtensionKind::Workspace, ExtensionKind::Ui]
        );
    }

    #[test]
    fn extension_cannot_opt_into_web_only() {
        let (_guard, test_object) = setup();
        assert_eq!(test_object.get_extension_kind(&manifest(json!({ "main": "main.js", "extensionKind": ["web"] }))), vec![ExtensionKind::Workspace]);
    }
}

mod sessions_window_support {
    use super::*;
    use je_extensions::manifest::EXTENSIONS_ENABLE_AGENTS_WINDOW_CAPABILITY;
    use je_workbench::services::extensions::extension_manifest_properties_service::EXTENSIONS_SUPPORT_AGENTS_WINDOW;

    /// `setup`, `teardown` e `createTestObject` da suíte `SessionsWindowSupport`: o serviço criado por último é
    /// descartado ao fim, e depois o guarda confere o vazamento.
    struct Suite {
        test_object: RefCell<Option<Rc<ExtensionManifestPropertiesService>>>,
        test_configuration_service: Rc<TestConfigurationService>,
        guard: LeakGuard,
    }

    impl Suite {
        fn new() -> Self {
            let guard = ensure_no_disposables_are_leaked_in_test_suite();
            Self { test_object: RefCell::new(None), test_configuration_service: TestConfigurationService::new(None), guard }
        }

        /// `testObject = createTestObject()`.
        fn create_test_object(&self) -> Rc<ExtensionManifestPropertiesService> {
            let test_object = create_test_object(&self.guard, self.test_configuration_service.clone());
            *self.test_object.borrow_mut() = Some(test_object.clone());
            test_object
        }
    }

    impl Drop for Suite {
        fn drop(&mut self) {
            if let Some(test_object) = self.test_object.take() {
                test_object.dispose();
            }
        }
    }

    #[test]
    fn defaults_to_declarative_extensions_without_executable_code_and_supported_contributions() {
        let suite = Suite::new();
        let test_object = suite.create_test_object();

        assert_eq!(
            vec![
                test_object.can_execute_on_sessions_window(&get_extension_manifest(json!({ "contributes": { "themes": [] } })), false),
                test_object.can_execute_on_sessions_window(&get_extension_manifest(json!({ "main": "./out/extension.js", "contributes": { "themes": [] } })), false),
                test_object.can_execute_on_sessions_window(&get_extension_manifest(json!({ "contributes": { "commands": [] } })), false),
            ],
            vec![true, false, false]
        );
    }

    #[test]
    fn preserves_built_in_defaults_when_no_agents_window_support_is_declared() {
        let suite = Suite::new();
        let test_object = suite.create_test_object();

        assert_eq!(
            vec![
                test_object.can_execute_on_sessions_window(&get_extension_manifest(json!({ "main": "./out/extension.js" })), true),
                test_object.can_execute_on_sessions_window(&get_extension_manifest(json!({ "browser": "./out/extension.js" })), true),
                test_object.can_execute_on_sessions_window(&get_extension_manifest(json!({ "contributes": { "views": {} } })), true),
                test_object.can_execute_on_sessions_window(&get_extension_manifest(json!({ "contributes": { "viewsContainers": {} } })), true),
                test_object.can_execute_on_sessions_window(&get_extension_manifest(json!({ "contributes": { "debuggers": [] } })), true),
                test_object.can_execute_on_sessions_window(&get_extension_manifest(json!({ "contributes": { "walkthroughs": [] } })), true),
            ],
            vec![true, true, false, false, false, false]
        );
    }

    /// O corpo de `uses configured sessions window support override for ${...} extensions`.
    fn uses_configured_sessions_window_support_override(is_builtin: bool) {
        let suite = Suite::new();
        let _ = suite.test_configuration_service.set_user_configuration(
            EXTENSIONS_SUPPORT_AGENTS_WINDOW,
            dynamic_of(&json!({ "pub.a": true, "pub.b": false })),
            None,
        );
        let test_object = suite.create_test_object();

        assert_eq!(
            vec![
                test_object.can_execute_on_sessions_window(
                    &get_extension_manifest(json!({ "main": "./out/extension.js", "capabilities": { "agentsWindow": { "supported": false } }, "contributes": { "views": {} } })),
                    is_builtin
                ),
                test_object.can_execute_on_sessions_window(
                    &get_extension_manifest(json!({ "name": "b", "capabilities": { "agentsWindow": { "supported": true } }, "contributes": { "themes": [] } })),
                    is_builtin
                ),
            ],
            vec![true, false]
        );
    }

    #[test]
    fn uses_configured_sessions_window_support_override_for_user_extensions() {
        uses_configured_sessions_window_support_override(false);
    }

    /// O corpo de `uses declared agents window support for ${...} extensions without the proposal`.
    fn uses_declared_agents_window_support_without_the_proposal(is_builtin: bool) {
        let suite = Suite::new();
        let _ = suite.test_configuration_service.set_user_configuration(EXTENSIONS_ENABLE_AGENTS_WINDOW_CAPABILITY, dynamic_of(&json!(true)), None);
        let test_object = suite.create_test_object();

        assert_eq!(
            vec![
                test_object.can_execute_on_sessions_window(
                    &get_extension_manifest(json!({ "main": "./out/extension.js", "capabilities": { "agentsWindow": { "supported": true } }, "contributes": { "commands": [] } })),
                    is_builtin
                ),
                test_object.can_execute_on_sessions_window(
                    &get_extension_manifest(json!({ "browser": "./out/extension.js", "capabilities": { "agentsWindow": { "supported": true } }, "contributes": { "views": {} } })),
                    is_builtin
                ),
                test_object.can_execute_on_sessions_window(
                    &get_extension_manifest(json!({ "capabilities": { "agentsWindow": { "supported": false } }, "contributes": { "themes": [] } })),
                    is_builtin
                ),
            ],
            vec![true, true, false]
        );
    }

    #[test]
    fn uses_declared_agents_window_support_for_user_extensions_without_the_proposal() {
        uses_declared_agents_window_support_without_the_proposal(false);
    }

    #[test]
    fn uses_configured_sessions_window_support_override_for_built_in_extensions() {
        uses_configured_sessions_window_support_override(true);
    }

    #[test]
    fn uses_declared_agents_window_support_for_built_in_extensions_without_the_proposal() {
        uses_declared_agents_window_support_without_the_proposal(true);
    }

    #[test]
    fn ignores_declared_agents_window_support_for_user_extensions_when_the_capability_setting_is_disabled() {
        let suite = Suite::new();
        let _ = suite.test_configuration_service.set_user_configuration(EXTENSIONS_ENABLE_AGENTS_WINDOW_CAPABILITY, dynamic_of(&json!(false)), None);
        let test_object = suite.create_test_object();

        assert_eq!(
            vec![
                test_object.can_execute_on_sessions_window(
                    &get_extension_manifest(json!({ "main": "./out/extension.js", "capabilities": { "agentsWindow": { "supported": true } }, "contributes": { "commands": [] } })),
                    false
                ),
                test_object.can_execute_on_sessions_window(
                    &get_extension_manifest(json!({ "capabilities": { "agentsWindow": { "supported": false } }, "contributes": { "themes": [] } })),
                    false
                ),
                test_object.can_execute_on_sessions_window(
                    &get_extension_manifest(json!({ "main": "./out/extension.js", "capabilities": { "agentsWindow": { "supported": true } }, "contributes": { "views": {} } })),
                    true
                ),
            ],
            vec![false, true, true]
        );
    }
}

// Workspace Trust is disabled in web at the moment
#[cfg(not(target_arch = "wasm32"))]
mod extension_untrusted_workspace_support_type {
    use super::*;
    use je_base::product::{BoolOrLimited, ExtensionUntrustedWorkspaceSupport, IStringDictionary};
    use je_platform::product::ProductService;

    /// `setup`, `teardown` e `assertUntrustedWorkspaceSupport` da suíte `ExtensionUntrustedWorkspaceSupportType`.
    struct Suite {
        test_object: RefCell<Option<Rc<ExtensionManifestPropertiesService>>>,
        test_configuration_service: Rc<TestConfigurationService>,
    }

    impl Suite {
        fn new() -> Self {
            Self { test_object: RefCell::new(None), test_configuration_service: TestConfigurationService::new(None) }
        }

        /// `instantiationService.stub(IProductService, ...)` e `stub(IWorkspaceTrustEnablementService, ...)`, mais
        /// `assertUntrustedWorkspaceSupport(extensionManifest, expected)`.
        fn assert_untrusted_workspace_support(
            &self,
            product_service: ProductService,
            workspace_trust_enabled: bool,
            extension_manifest: ExtensionManifest,
            expected: BoolOrLimited,
        ) {
            let test_object = ExtensionManifestPropertiesService::new(
                Rc::new(product_service),
                self.test_configuration_service.clone(),
                TestWorkspaceTrustEnablementService::new(workspace_trust_enabled),
                NullLogService::shared(),
            );
            *self.test_object.borrow_mut() = Some(test_object.clone());
            let untrusted_workspace_support = test_object.get_extension_untrusted_workspace_support_type(&extension_manifest);

            assert_eq!(untrusted_workspace_support, expected);
        }
    }

    impl Drop for Suite {
        fn drop(&mut self) {
            if let Some(test_object) = self.test_object.take() {
                test_object.dispose();
            }
        }
    }

    /// `stub(IProductService, { extensionUntrustedWorkspaceSupport })`.
    fn product_service_with(extension_untrusted_workspace_support: Value) -> ProductService {
        let dictionary: IStringDictionary<ExtensionUntrustedWorkspaceSupport> =
            serde_json::from_value(extension_untrusted_workspace_support).expect("o extensionUntrustedWorkspaceSupport do produto");
        ProductService { extension_untrusted_workspace_support: Some(dictionary), ..ProductService::default() }
    }

    fn limited() -> BoolOrLimited {
        BoolOrLimited::Limited("limited".to_owned())
    }

    fn boolean(value: bool) -> BoolOrLimited {
        BoolOrLimited::Bool(value)
    }

    /// `testConfigurationService.setUserConfiguration('extensions', { supportUntrustedWorkspaces: ... })`.
    fn set_support_untrusted_workspaces(suite: &Suite, support_untrusted_workspaces: Value) {
        let _ = suite.test_configuration_service.set_user_configuration(
            "extensions",
            dynamic_of(&json!({ "supportUntrustedWorkspaces": support_untrusted_workspaces })),
            None,
        );
    }

    #[test]
    fn test_extension_workspace_trust_request_when_main_entry_point_is_missing() {
        let suite = Suite::new();

        let extension_manifest = get_extension_manifest(json!({}));
        suite.assert_untrusted_workspace_support(ProductService::default(),true, extension_manifest, boolean(true));
    }

    #[test]
    fn test_extension_workspace_trust_request_when_workspace_trust_is_disabled() {
        let suite = Suite::new();

        let extension_manifest = get_extension_manifest(json!({ "main": "./out/extension.js" }));
        suite.assert_untrusted_workspace_support(ProductService::default(),false, extension_manifest, boolean(true));
    }

    #[test]
    fn test_extension_workspace_trust_request_when_true_override_exists_in_settings_json() {
        let suite = Suite::new();

        set_support_untrusted_workspaces(&suite, json!({ "pub.a": { "supported": true } }));
        let extension_manifest = get_extension_manifest(json!({ "main": "./out/extension.js", "capabilities": { "untrustedWorkspaces": { "supported": "limited" } } }));
        suite.assert_untrusted_workspace_support(ProductService::default(),true, extension_manifest, boolean(true));
    }

    #[test]
    fn test_extension_workspace_trust_request_when_override_false_exists_in_settings_json() {
        let suite = Suite::new();

        set_support_untrusted_workspaces(&suite, json!({ "pub.a": { "supported": false } }));
        let extension_manifest = get_extension_manifest(json!({ "main": "./out/extension.js", "capabilities": { "untrustedWorkspaces": { "supported": "limited" } } }));
        suite.assert_untrusted_workspace_support(ProductService::default(),true, extension_manifest, boolean(false));
    }

    #[test]
    fn test_extension_workspace_trust_request_when_override_true_for_the_version_exists_in_settings_json() {
        let suite = Suite::new();

        set_support_untrusted_workspaces(&suite, json!({ "pub.a": { "supported": true, "version": "1.0.0" } }));
        let extension_manifest = get_extension_manifest(json!({ "main": "./out/extension.js", "capabilities": { "untrustedWorkspaces": { "supported": "limited" } } }));
        suite.assert_untrusted_workspace_support(ProductService::default(),true, extension_manifest, boolean(true));
    }

    #[test]
    fn test_extension_workspace_trust_request_when_override_false_for_the_version_exists_in_settings_json() {
        let suite = Suite::new();

        set_support_untrusted_workspaces(&suite, json!({ "pub.a": { "supported": false, "version": "1.0.0" } }));
        let extension_manifest = get_extension_manifest(json!({ "main": "./out/extension.js", "capabilities": { "untrustedWorkspaces": { "supported": "limited" } } }));
        suite.assert_untrusted_workspace_support(ProductService::default(),true, extension_manifest, boolean(false));
    }

    #[test]
    fn test_extension_workspace_trust_request_when_override_for_a_different_version_exists_in_settings_json() {
        let suite = Suite::new();

        set_support_untrusted_workspaces(&suite, json!({ "pub.a": { "supported": true, "version": "2.0.0" } }));
        let extension_manifest = get_extension_manifest(json!({ "main": "./out/extension.js", "capabilities": { "untrustedWorkspaces": { "supported": "limited" } } }));
        suite.assert_untrusted_workspace_support(ProductService::default(),true, extension_manifest, limited());
    }

    #[test]
    fn test_extension_workspace_trust_request_when_default_true_exists_in_product_json() {
        let suite = Suite::new();

        let extension_manifest = get_extension_manifest(json!({ "main": "./out/extension.js" }));
        suite.assert_untrusted_workspace_support(product_service_with(json!({ "pub.a": { "default": true } })), true, extension_manifest, boolean(true));
    }

    #[test]
    fn test_extension_workspace_trust_request_when_default_false_exists_in_product_json() {
        let suite = Suite::new();

        let extension_manifest = get_extension_manifest(json!({ "main": "./out/extension.js" }));
        suite.assert_untrusted_workspace_support(product_service_with(json!({ "pub.a": { "default": false } })), true, extension_manifest, boolean(false));
    }

    #[test]
    fn test_extension_workspace_trust_request_when_override_limited_exists_in_product_json() {
        let suite = Suite::new();

        let extension_manifest = get_extension_manifest(json!({ "main": "./out/extension.js", "capabilities": { "untrustedWorkspaces": { "supported": true } } }));
        suite.assert_untrusted_workspace_support(product_service_with(json!({ "pub.a": { "override": "limited" } })), true, extension_manifest, limited());
    }

    #[test]
    fn test_extension_workspace_trust_request_when_override_false_exists_in_product_json() {
        let suite = Suite::new();

        let extension_manifest = get_extension_manifest(json!({ "main": "./out/extension.js", "capabilities": { "untrustedWorkspaces": { "supported": true } } }));
        suite.assert_untrusted_workspace_support(product_service_with(json!({ "pub.a": { "override": false } })), true, extension_manifest, boolean(false));
    }

    #[test]
    fn test_extension_workspace_trust_request_when_value_exists_in_package_json() {
        let suite = Suite::new();

        let extension_manifest = get_extension_manifest(json!({ "main": "./out/extension.js", "capabilities": { "untrustedWorkspaces": { "supported": "limited" } } }));
        suite.assert_untrusted_workspace_support(ProductService::default(),true, extension_manifest, limited());
    }

    #[test]
    fn test_extension_workspace_trust_request_when_no_value_exists_in_package_json() {
        let suite = Suite::new();

        let extension_manifest = get_extension_manifest(json!({ "main": "./out/extension.js" }));
        suite.assert_untrusted_workspace_support(ProductService::default(),true, extension_manifest, boolean(false));
    }
}
