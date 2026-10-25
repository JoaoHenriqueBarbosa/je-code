//! Porte de `src/vs/workbench/contrib/preferences/test/browser/settingsTreeModels.test.ts` (suítes `SettingsTree managed default descriptions`, `SettingsTree Agents Window density`, `SettingsTree managed sandbox`, `SettingsTree ExP assignments`, `SettingsTree deprecation warnings`, `SettingsTree sandbox network search ordering`, `SettingsTree search results` e `SettingsTree`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - Os dublês de serviços (`TestSettingsConfiguration`, `StubManagedSettingsService` sobre o `ManagedStore`, `StubLanguageService`),
//!   o `create_services`, o `ConfigurationRegistration` e os construtores de configurações vivem em
//!   `support/preferences_support.rs`, compartilhados com `settings_editor2.rs` e `settings_tree.rs`.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` de cada teste (inclusive o da suíte `SettingsTree`, que o
//!   chama no fim). O `TestInstantiationService` não existe: os serviços do original (`instantiationService.stub(...)`) são o
//!   [`SettingsTreeServices`] que [`create_services`] monta, e `createInstance(Model, ...)` é o construtor do modelo com ele.
//! - `suiteSetup(() => registry.registerConfiguration(...))` e `suiteTeardown(() => registry.deregisterConfigurations(...))`
//!   são a [`ConfigurationRegistration`], criada no começo de cada teste da suíte (cada teste roda num thread com o seu
//!   registro). `terminalContribConfiguration` e `chatNetworkDomainConfigurationProperties` são as propriedades que as
//!   duas funções portadas devolvem, filtradas por `managedSettingsPresentation` como no original.
//! - A subclasse de `TestConfigurationService` que sobrescreve `inspect` e responde `isSettingAppliedForAllProfiles()` é o
//!   [`TestSettingsConfiguration`], que delega ao `TestConfigurationService` e implementa o
//!   `IWorkbenchConfigurationService`. O `super.inspect()` da subclasse é o `inspect` de um segundo
//!   `TestConfigurationService` sobre o mesmo objeto de configuração ([`TestSettingsConfiguration::base`]), e
//!   `configuration.inspect = ...` é `stub_inspect_with`.
//! - `mock<ISetting>()` com campos sobrescritos é o `ISetting` com `..ISetting::default()`. Os `ILanguageService` e
//!   `IUserDataProfilesService` parciais: o `ILanguageService` é o [`StubLanguageService`] (só `isRegisteredLanguageId`
//!   responde, o resto falha como o `undefined` falharia), e o `IUserDataProfilesService` do `getIndicatorsLabelAriaLabel`
//!   não é passado, porque o original nunca o lê (ver `settings_editor_setting_indicators.rs`).
//! - O `IManagedSettingsService` parcial dos testes do sandbox (`managed` é um objeto que o teste muda) é o
//!   [`StubManagedSettingsService`] sobre o [`ManagedStore`]; atribuir `undefined` a uma chave é gravar `None` (a leitura é a
//!   mesma de uma chave ausente) e `delete` é [`ManagedStore::delete`].
//! - `assert.deepStrictEqual` sobre objetos é o [`assert_deep_strict_equal`] sobre [`Dynamic`] (objetos montados com
//!   [`obj`]); sobre estruturas tipadas é `assert_eq!`. Os `ConfigurationTarget` de `SettingsTarget` são o enum
//!   [`SettingsTarget`].
//! - `sinon.stub(console, 'warn')` é o [`WarnSpy`], e `consoleWarn.callCount` é `call_count()`.
//! - Os `for` de fora de um `test(...)` (valores de `allowedHosts`, de `enabled` e `allowBypass`, de alvo e de acesso local) são
//!   laços dentro do teste de Rust: o original registra um teste por combinação, e um `#[test]` conta o literal `test(...)`.
//! - `await configuration.setUserConfiguration(...)` é a chamada direta (o efeito do `TestConfigurationService` é imediato).
//! - `createModel` de cada suíte devolve a struct da suíte (com `read`), no lugar do objeto literal do original.
//! - O `ILanguageService` e o produto do `TestProductService` são os de `je_workbench::test`, e o ambiente é o
//!   `TestEnvironmentService` com `isSessionsWindow`.

mod support;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::support::preferences_support::common::{LeakGuard, assert_deep_strict_equal, dyn_strs, ensure_no_disposables_are_leaked_in_test_suite, node, obj, prop_with, s};
use crate::support::preferences_support::{
    ConfigurationRegistration, ManagedStore, StubManagedSettingsService, TestSettingsConfiguration, copy_of, create_services, first_defined, first_element, match_of,
    mock_setting, new_test_service, null_managed_settings_service, register_sandbox_presentation_configuration, search_result, toc_entry, view_state,
};
use je_base::console::WarnSpy;
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::policy::PolicyValue;
use je_editor::range::Range;
use je_platform::configuration::registry::ConfigurationScope;
use je_platform::configuration::test_configuration_service::{TestConfigurationService, TestConfigurationValue};
use je_platform::network_filter::settings::AgentNetworkDomainSettingId;
use je_platform::policy::copilot_managed_settings::{
    COPILOT_SANDBOX_ALLOWED_HOSTS_KEY, COPILOT_SANDBOX_ALLOW_BYPASS_KEY, COPILOT_SANDBOX_ALLOW_DEV_TOOL_ACCESS_KEY, COPILOT_SANDBOX_ALLOW_LOCAL_NETWORK_KEY,
    COPILOT_SANDBOX_ALLOW_OUTBOUND_KEY, COPILOT_SANDBOX_DENIED_PATHS_KEY, COPILOT_SANDBOX_ENABLED_KEY, COPILOT_SANDBOX_LSP_SERVERS_KEY,
    COPILOT_SANDBOX_MCP_SERVERS_KEY, COPILOT_SANDBOX_READONLY_PATHS_KEY,
};
use je_platform::sandbox::settings::AgentSandboxSettingId;
use je_workbench::contrib::chat::model_selection::{default_model_policy_description_function, get_default_model_policy_description};
use je_workbench::contrib::preferences::common::preferences::{EXP_ASSIGNMENT_SETTING_TAG, POLICY_SETTING_TAG};
use je_workbench::contrib::preferences::preferences_search::SettingMatches;
use je_workbench::contrib::preferences::preferences_widgets::SettingsTarget;
use je_workbench::contrib::preferences::settings_editor_setting_indicators::get_indicators_label_aria_label;
use je_workbench::contrib::preferences::settings_tree_models::{
    IParsedQuery, ISettingsEditorViewState, SearchResultIdx, SearchResultModel, SettingDisplayFormat, SettingsTreeGroupElement, SettingsTreeModel,
    SettingsTreeSettingElement, ViewStateRef, parse_query, sanitize_id, setting_key_to_display_format,
};
use je_workbench::services::configuration::common::experimental_settings::{ExperimentalSettingsService, IExperimentalSettingsService};
use je_workbench::services::layout::layout_service::{LayoutSettings, ModernUiDensity};
use je_workbench::services::preferences::preferences::{ISetting, ISettingMatch, SettingMatchType};

// ---------------------------------------------------------------------------------------------------------------------
// Construtores comuns às suítes (os dublês e a configuração de teste vivem em `support/preferences_support.rs`).

/// `{ value, managed, policyFilter }`.
fn element_state(value: Dynamic, managed: bool, policy_filter: bool) -> Dynamic {
    obj(vec![("value", value), ("managed", Dynamic::Bool(managed)), ("policyFilter", Dynamic::Bool(policy_filter))])
}

fn boolean(value: bool) -> Dynamic {
    Dynamic::Bool(value)
}

fn number(value: usize) -> Dynamic {
    Dynamic::Number(value as f64)
}

fn array(items: Vec<Dynamic>) -> Dynamic {
    Dynamic::array(items)
}

/// Os elementos de configuração dos filhos da raiz (`filter((child): child is SettingsTreeSettingElement => child instanceof SettingsTreeSettingElement)`).
fn setting_children(root: &Rc<SettingsTreeGroupElement>) -> Vec<Rc<SettingsTreeSettingElement>> {
    root.children().iter().filter_map(|child| child.as_setting().cloned()).collect()
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('SettingsTree managed default descriptions')

#[test]
fn explains_the_overridden_value_in_scope_and_restores_it_when_policy_is_removed() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let key = "test.managedDefaultModelDescription";
    let _registration = ConfigurationRegistration::new(node(
        "managedDefaultDescriptionTest",
        vec![(key, prop_with("{ 'type': 'string', 'default': '' }", vec![("policyValueDescription", Dynamic::Function(default_model_policy_description_function()))]))],
    ));

    for target in [SettingsTarget::UserLocal, SettingsTarget::Workspace] {
        let policy = new_test_service(&store, Some(obj(vec![(key, s("opus"))])));
        let workspace = new_test_service(&store, Some(obj(vec![(key, s("auto"))])));
        let configuration = TestSettingsConfiguration::new(&store, Some(obj(vec![(key, s("gemini"))])));
        let base = configuration.base(&store);
        {
            let (policy, workspace) = (policy.clone(), workspace.clone());
            configuration.service.stub_inspect_with(move |setting, overrides| {
                let result = base.inspect(setting, overrides);
                let policy_value = policy.get_value(Some(setting), None);
                let workspace_value = workspace.get_value(Some(setting), None);
                let mut inspected = copy_of(&*result);
                inspected.value = first_defined(&[&policy_value, &workspace_value, &result.value()]);
                inspected.workspace_value = workspace_value;
                inspected.policy_value = policy_value;
                Rc::new(inspected)
            });
        }
        let (services, _) = create_services(&store, &configuration, false, null_managed_settings_service());
        let model = store.add_rc(SettingsTreeModel::new(view_state(target.clone()), true, services.clone()));
        model.update(&toc_entry("test", "Test", vec![Rc::new(mock_setting(key, "string"))]));
        let element = first_element(&model, key);
        let read = || {
            element.inspect_self();
            obj(vec![
                ("value", element.value()),
                ("managed", boolean(element.has_policy_value())),
                ("description", element.policy_value_description().map_or(Dynamic::Undefined, |description| s(&description))),
                ("accessible", s(&get_indicators_label_aria_label(&*element, &*configuration, &*services.language_service))),
            ])
        };
        let managed = read();
        let _ = policy.set_user_configuration(key, s("auto"), None);
        let changed = read();
        let _ = policy.set_user_configuration(key, Dynamic::Undefined, None);
        let removed = read();
        let saved = if target == SettingsTarget::UserLocal { "gemini" } else { "auto" };
        let described = |policy_value: &str| s(&get_default_model_policy_description(&s(policy_value), &s(saved)));
        let inspected = configuration.inspect(key);
        assert_deep_strict_equal(
            &obj(vec![
                ("managed", managed),
                ("changed", changed),
                ("removed", obj(vec![("value", removed.get_str("value")), ("managed", removed.get_str("managed")), ("description", removed.get_str("description"))])),
                ("savedUser", inspected.user_local_value()),
                ("savedWorkspace", inspected.workspace_value()),
            ]),
            &obj(vec![
                ("managed", obj(vec![("value", s("opus")), ("managed", boolean(true)), ("description", described("opus")), ("accessible", described("opus"))])),
                ("changed", obj(vec![("value", s("auto")), ("managed", boolean(true)), ("description", described("auto")), ("accessible", described("auto"))])),
                ("removed", obj(vec![("value", s(saved)), ("managed", boolean(false)), ("description", Dynamic::Undefined)])),
                ("savedUser", s("gemini")),
                ("savedWorkspace", s("auto")),
            ]),
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('SettingsTree Agents Window density')

/// O `createModel(isSessionsWindow, settingsTarget)` da suíte.
struct DensityModel {
    workspace: Rc<TestConfigurationService>,
    model: Rc<SearchResultModel>,
    key: &'static str,
}

impl DensityModel {
    fn new(store: &LeakGuard, is_sessions_window: bool, settings_target: SettingsTarget) -> Self {
        let key = LayoutSettings::ModernUiDensity.as_str();
        let defaults = new_test_service(store, Some(obj(vec![(key, s(ModernUiDensity::Default.as_str()))])));
        let workspace = new_test_service(store, None);
        let configuration = TestSettingsConfiguration::new(store, Some(obj(vec![(key, s(ModernUiDensity::Compact.as_str()))])));
        let base = configuration.base(store);
        {
            let (defaults, workspace) = (defaults.clone(), workspace.clone());
            configuration.service.stub_inspect_with(move |setting, overrides| {
                let inspected = base.inspect(setting, overrides);
                let default_value = defaults.get_value(Some(setting), None);
                let workspace_value = workspace.get_value(Some(setting), None);
                let mut result = copy_of(&*inspected);
                result.value = first_defined(&[&workspace_value, &inspected.user_value(), &default_value]);
                result.default_value = default_value;
                result.workspace_value = workspace_value;
                Rc::new(result)
            });
        }
        let (services, _) = create_services(store, &configuration, is_sessions_window, null_managed_settings_service());
        let model = store.add_rc(SearchResultModel::new(view_state(settings_target), None, true, services));
        let setting = ISetting {
            r#enum: Some(vec![ModernUiDensity::Default.as_str().to_owned(), ModernUiDensity::Compact.as_str().to_owned()]),
            scope: Some(ConfigurationScope::Window),
            ..mock_setting(key, "string")
        };
        model.set_result(SearchResultIdx::Local, Some(search_result(vec![match_of(setting, SettingMatchType::None)], false)));
        Self { workspace, model, key }
    }

    fn read(&self) -> Dynamic {
        let element = first_element(&self.model, self.key);
        element.inspect_self();
        obj(vec![("value", element.value()), ("defaultValue", element.default_value()), ("configured", boolean(element.is_configured()))])
    }
}

fn density_state(value: ModernUiDensity, default_value: ModernUiDensity, configured: bool) -> Dynamic {
    obj(vec![("value", s(value.as_str())), ("defaultValue", s(default_value.as_str())), ("configured", boolean(configured))])
}

#[test]
fn shows_inherited_density_until_an_agents_window_override_is_set() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let density = DensityModel::new(&store, true, SettingsTarget::Workspace);
    let inherited = density.read();
    let _ = density.workspace.set_user_configuration(density.key, s(ModernUiDensity::Default.as_str()), None);
    let overridden = density.read();
    let _ = density.workspace.set_user_configuration(density.key, Dynamic::Undefined, None);

    let inherited_state = || density_state(ModernUiDensity::Compact, ModernUiDensity::Default, false);
    assert_deep_strict_equal(
        &obj(vec![("inherited", inherited), ("overridden", overridden), ("reset", density.read())]),
        &obj(vec![
            ("inherited", inherited_state()),
            ("overridden", density_state(ModernUiDensity::Default, ModernUiDensity::Default, true)),
            ("reset", inherited_state()),
        ]),
    );
}

#[test]
fn preserves_the_existing_editor_window_and_user_scope_display() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_workspace = DensityModel::new(&store, false, SettingsTarget::Workspace).read();
    let agents_user = DensityModel::new(&store, true, SettingsTarget::UserLocal).read();
    assert_deep_strict_equal(
        &obj(vec![("editorWorkspace", editor_workspace), ("agentsUser", agents_user)]),
        &obj(vec![
            ("editorWorkspace", density_state(ModernUiDensity::Default, ModernUiDensity::Default, false)),
            ("agentsUser", density_state(ModernUiDensity::Compact, ModernUiDensity::Default, true)),
        ]),
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('SettingsTree managed sandbox')

fn local_paths() -> Dynamic {
    obj(vec![("readwritePaths", dyn_strs(&["local.write"])), ("readonlyPaths", dyn_strs(&["local.read"])), ("deniedPaths", dyn_strs(&["local.deny"]))])
}

/// O `createModel(...)` da suíte `SettingsTree managed sandbox`.
struct SandboxModel {
    model: Rc<SearchResultModel>,
    managed: Rc<ManagedStore>,
    configuration: Rc<TestSettingsConfiguration>,
    keys: Vec<&'static str>,
}

impl SandboxModel {
    fn new(store: &LeakGuard, settings_target: SettingsTarget, local_access: bool, setting_keys: Option<Vec<&'static str>>, initial_managed: Vec<(&str, PolicyValue)>) -> Self {
        let configuration = TestSettingsConfiguration::new(
            store,
            Some(obj(vec![
                (AgentSandboxSettingId::AgentSandboxEnabled.as_str(), s("off")),
                (AgentSandboxSettingId::AgentSandboxAllowUnsandboxedCommands.as_str(), boolean(local_access)),
                (AgentSandboxSettingId::AgentSandboxAllowNetwork.as_str(), boolean(local_access)),
                (AgentSandboxSettingId::AgentSandboxMcpServers.as_str(), boolean(local_access)),
                (AgentSandboxSettingId::AgentSandboxLspServers.as_str(), boolean(local_access)),
                (AgentSandboxSettingId::AgentSandboxAllowDevToolAccess.as_str(), boolean(local_access)),
                (AgentSandboxSettingId::AgentSandboxAllowLocalNetwork.as_str(), boolean(local_access)),
                (AgentNetworkDomainSettingId::AllowedNetworkDomains.as_str(), dyn_strs(&["local.example"])),
                (AgentSandboxSettingId::AgentSandboxUserConfiguredPaths.as_str(), local_paths()),
            ])),
        );
        let managed = Rc::new(ManagedStore::default());
        for (key, value) in initial_managed {
            managed.set(key, Some(value));
        }
        let (services, _) = create_services(store, &configuration, false, Rc::new(StubManagedSettingsService::new(managed.clone())));
        let model = store.add_rc(SearchResultModel::new(view_state(settings_target), None, true, services));
        let keys = setting_keys.unwrap_or_else(|| {
            vec![
                AgentSandboxSettingId::AgentSandboxEnabled.as_str(),
                AgentSandboxSettingId::AgentSandboxAllowUnsandboxedCommands.as_str(),
                AgentSandboxSettingId::AgentSandboxAllowNetwork.as_str(),
            ]
        });
        let matches = keys
            .iter()
            .map(|key| {
                let type_ = if *key == AgentSandboxSettingId::AgentSandboxUserConfiguredPaths.as_str() {
                    "object"
                } else if *key == AgentNetworkDomainSettingId::AllowedNetworkDomains.as_str() {
                    "array"
                } else if *key == AgentSandboxSettingId::AgentSandboxEnabled.as_str() {
                    "string"
                } else {
                    "boolean"
                };
                let setting = ISetting {
                    array_item_type: if *key == AgentNetworkDomainSettingId::AllowedNetworkDomains.as_str() { Some("string".to_owned()) } else { None },
                    scope: Some(ConfigurationScope::Resource),
                    ..mock_setting(key, type_)
                };
                match_of(setting, SettingMatchType::None)
            })
            .collect();
        model.set_result(SearchResultIdx::Local, Some(search_result(matches, false)));
        Self { model, managed, configuration, keys }
    }

    /// O `createModel()` sem argumentos: `USER_LOCAL`, acesso local e as três configurações padrão.
    fn with_defaults(store: &LeakGuard) -> Self {
        Self::new(store, SettingsTarget::UserLocal, true, None, Vec::new())
    }

    fn read(&self) -> Vec<Dynamic> {
        self.keys
            .iter()
            .map(|key| {
                let element = first_element(&self.model, key);
                element.inspect_self();
                let mut policy_filter = HashSet::new();
                policy_filter.insert(POLICY_SETTING_TAG.to_owned());
                element_state(element.value(), element.has_policy_value(), element.matches_all_tags(Some(&policy_filter)))
            })
            .collect()
    }

    /// `keys.map(key => configuration.getValue(key))`.
    fn configured(&self, keys: &[&str]) -> Dynamic {
        array(keys.iter().map(|key| self.configuration.get_value(key)).collect())
    }
}

fn texts_state(value: &[&str], managed: bool, policy_filter: bool) -> Dynamic {
    element_state(dyn_strs(value), managed, policy_filter)
}

#[test]
fn managed_allowlist_locks_allowed_domains_and_restores_local_preferences_after_removal() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let _registration = register_sandbox_presentation_configuration("sandboxPresentationTest");
    let key = AgentNetworkDomainSettingId::AllowedNetworkDomains.as_str();
    for allowed_hosts in [vec![], vec!["managed.example", "*.managed.example"]] {
        let sandbox = SandboxModel::new(&store, SettingsTarget::UserLocal, true, Some(vec![key]), Vec::new());
        let initial = sandbox.read();
        sandbox.managed.set(COPILOT_SANDBOX_ALLOWED_HOSTS_KEY, Some(PolicyValue::String(json_stringify(&dyn_strs(&allowed_hosts)).expect("JSON.stringify"))));
        let locked = sandbox.read();
        sandbox.managed.delete(COPILOT_SANDBOX_ALLOWED_HOSTS_KEY);
        assert_deep_strict_equal(
            &obj(vec![("initial", array(initial.clone())), ("locked", array(locked)), ("removed", array(sandbox.read())), ("stored", sandbox.configuration.get_value(key))]),
            &obj(vec![
                ("initial", array(vec![texts_state(&["local.example"], false, false)])),
                ("locked", array(vec![texts_state(&allowed_hosts, true, true)])),
                ("removed", array(initial)),
                ("stored", dyn_strs(&["local.example"])),
            ]),
        );
    }
}

#[test]
fn ignores_malformed_managed_allowlists_in_the_presentation_and_preserves_local_values() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let _registration = register_sandbox_presentation_configuration("sandboxPresentationTest");
    let key = AgentNetworkDomainSettingId::AllowedNetworkDomains.as_str();
    let sandbox = SandboxModel::new(&store, SettingsTarget::UserLocal, true, Some(vec![key]), Vec::new());
    let console_warn = WarnSpy::install();
    let values = [
        PolicyValue::String("not JSON".to_owned()),
        PolicyValue::String("[\"managed.example\",1]".to_owned()),
        PolicyValue::String("{}".to_owned()),
        PolicyValue::String("null".to_owned()),
        PolicyValue::Boolean(false),
    ];
    let settings: Vec<Dynamic> = values
        .into_iter()
        .map(|value| {
            sandbox.managed.set(COPILOT_SANDBOX_ALLOWED_HOSTS_KEY, Some(value));
            array(sandbox.read())
        })
        .collect();
    assert_deep_strict_equal(
        &obj(vec![("settings", array(settings)), ("stored", sandbox.configuration.get_value(key)), ("warnings", number(console_warn.call_count()))]),
        &obj(vec![
            ("settings", array((0..5).map(|_| array(vec![texts_state(&["local.example"], false, false)])).collect())),
            ("stored", dyn_strs(&["local.example"])),
            ("warnings", number(5)),
        ]),
    );
    console_warn.restore();
}

#[test]
fn composes_managed_filesystem_presentation_with_local_paths_and_restores_preferences_on_removal() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let _registration = register_sandbox_presentation_configuration("sandboxPresentationTest");
    let key = AgentSandboxSettingId::AgentSandboxUserConfiguredPaths.as_str();
    let sandbox = SandboxModel::new(&store, SettingsTarget::UserLocal, true, Some(vec![key]), Vec::new());
    let initial = sandbox.read();
    sandbox.managed.set(COPILOT_SANDBOX_DENIED_PATHS_KEY, Some(PolicyValue::String("[\"managed.deny\"]".to_owned())));
    let denied = sandbox.read();
    sandbox.managed.set(COPILOT_SANDBOX_READONLY_PATHS_KEY, Some(PolicyValue::String("[]".to_owned())));
    let restricted = sandbox.read();
    sandbox.managed.delete(COPILOT_SANDBOX_DENIED_PATHS_KEY);
    sandbox.managed.delete(COPILOT_SANDBOX_READONLY_PATHS_KEY);
    let paths = |readwrite: &[&str], readonly: &[&str], denied: &[&str]| {
        obj(vec![("readwritePaths", dyn_strs(readwrite)), ("readonlyPaths", dyn_strs(readonly)), ("deniedPaths", dyn_strs(denied))])
    };
    assert_deep_strict_equal(
        &obj(vec![
            ("initial", array(initial.clone())),
            ("denied", array(denied)),
            ("restricted", array(restricted)),
            ("removed", array(sandbox.read())),
            ("stored", sandbox.configuration.get_value(key)),
        ]),
        &obj(vec![
            ("initial", array(vec![element_state(local_paths(), false, false)])),
            ("denied", array(vec![element_state(paths(&["local.write"], &["local.read"], &["local.deny", "managed.deny"]), true, true)])),
            ("restricted", array(vec![element_state(paths(&["local.write"], &[], &["local.deny", "managed.deny"]), true, true)])),
            ("removed", array(initial)),
            ("stored", local_paths()),
        ]),
    );
}

#[test]
fn ignores_malformed_managed_filesystem_paths_without_hiding_saved_local_paths() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let _registration = register_sandbox_presentation_configuration("sandboxPresentationTest");
    let key = AgentSandboxSettingId::AgentSandboxUserConfiguredPaths.as_str();
    let console_warn = WarnSpy::install();
    let sandbox = SandboxModel::new(
        &store,
        SettingsTarget::UserLocal,
        true,
        Some(vec![key]),
        vec![(COPILOT_SANDBOX_DENIED_PATHS_KEY, PolicyValue::String("private.enterprise.path".to_owned()))],
    );
    let values = [
        PolicyValue::String("private.enterprise.path".to_owned()),
        PolicyValue::String("[123]".to_owned()),
        PolicyValue::String("{}".to_owned()),
        PolicyValue::String("null".to_owned()),
        PolicyValue::Boolean(false),
    ];
    let settings: Vec<Dynamic> = values
        .into_iter()
        .map(|value| {
            sandbox.managed.set(COPILOT_SANDBOX_DENIED_PATHS_KEY, Some(value));
            array(sandbox.read())
        })
        .collect();
    assert_deep_strict_equal(
        &obj(vec![("settings", array(settings)), ("stored", sandbox.configuration.get_value(key))]),
        &obj(vec![
            ("settings", array((0..5).map(|_| array(vec![element_state(local_paths(), false, false)])).collect())),
            ("stored", local_paths()),
        ]),
    );
    console_warn.restore();
}

#[test]
fn resolved_enabled_and_allow_bypass() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let _registration = register_sandbox_presentation_configuration("sandboxPresentationTest");
    for enabled in [None, Some(false), Some(true)] {
        for allow_bypass in [None, Some(false), Some(true)] {
            let sandbox = SandboxModel::with_defaults(&store);
            sandbox.managed.set(COPILOT_SANDBOX_ENABLED_KEY, enabled.map(PolicyValue::Boolean));
            sandbox.managed.set(COPILOT_SANDBOX_ALLOW_BYPASS_KEY, allow_bypass.map(PolicyValue::Boolean));
            let required = enabled == Some(true);
            let bypass_restricted = (required && allow_bypass != Some(true)) || allow_bypass == Some(false);

            let keys = sandbox.keys.clone();
            assert_deep_strict_equal(
                &obj(vec![("settings", array(sandbox.read())), ("configured", sandbox.configured(&keys))]),
                &obj(vec![
                    (
                        "settings",
                        array(vec![
                            element_state(s(if required { "on" } else { "off" }), required, required),
                            element_state(boolean(!bypass_restricted), bypass_restricted, bypass_restricted),
                            element_state(boolean(true), false, false),
                        ]),
                    ),
                    ("configured", array(vec![s("off"), boolean(true), boolean(true)])),
                ]),
            );
        }
    }
}

const SANDBOX_TARGETS: [fn() -> SettingsTarget; 3] = [|| SettingsTarget::UserLocal, || SettingsTarget::UserRemote, || SettingsTarget::Workspace];

#[test]
fn managed_setting_preserves_local_value_in_target() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let _registration = register_sandbox_presentation_configuration("sandboxPresentationTest");
    for target in SANDBOX_TARGETS {
        for local_access in [false, true] {
            let cases = [
                (AgentSandboxSettingId::AgentSandboxMcpServers.as_str(), COPILOT_SANDBOX_MCP_SERVERS_KEY, COPILOT_SANDBOX_LSP_SERVERS_KEY, true),
                (AgentSandboxSettingId::AgentSandboxLspServers.as_str(), COPILOT_SANDBOX_LSP_SERVERS_KEY, COPILOT_SANDBOX_MCP_SERVERS_KEY, true),
                (AgentSandboxSettingId::AgentSandboxAllowDevToolAccess.as_str(), COPILOT_SANDBOX_ALLOW_DEV_TOOL_ACCESS_KEY, COPILOT_SANDBOX_ALLOW_OUTBOUND_KEY, false),
                (AgentSandboxSettingId::AgentSandboxAllowLocalNetwork.as_str(), COPILOT_SANDBOX_ALLOW_LOCAL_NETWORK_KEY, COPILOT_SANDBOX_ALLOW_OUTBOUND_KEY, false),
            ];
            for (key, policy_key, other_policy_key, managed_value) in cases {
                let sandbox = SandboxModel::new(&store, target(), local_access, Some(vec![key]), Vec::new());
                let initial = sandbox.read();
                sandbox.managed.set(other_policy_key, Some(PolicyValue::Boolean(managed_value)));
                let independent = sandbox.read();
                sandbox.managed.set(policy_key, Some(PolicyValue::Boolean(managed_value)));
                let required = sandbox.read();
                sandbox.managed.set(policy_key, Some(PolicyValue::Boolean(!managed_value)));
                let optional = sandbox.read();
                sandbox.managed.delete(policy_key);
                assert_deep_strict_equal(
                    &obj(vec![
                        ("independent", array(independent)),
                        ("required", array(required)),
                        ("optional", array(optional)),
                        ("removed", array(sandbox.read())),
                        ("stored", sandbox.configuration.get_value(key)),
                    ]),
                    &obj(vec![
                        ("independent", array(initial.clone())),
                        ("required", array(vec![element_state(boolean(managed_value), true, true)])),
                        ("optional", array(initial.clone())),
                        ("removed", array(initial)),
                        ("stored", boolean(local_access)),
                    ]),
                );
            }
        }
    }
}

#[test]
fn managed_access_restrictions_preserve_local_value_in_target() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let _registration = register_sandbox_presentation_configuration("sandboxPresentationTest");
    for target in SANDBOX_TARGETS {
        for local_access in [false, true] {
            let sandbox = SandboxModel::new(&store, target(), local_access, None, Vec::new());
            let initial = sandbox.read();
            sandbox.managed.set(COPILOT_SANDBOX_ALLOW_BYPASS_KEY, Some(PolicyValue::Boolean(false)));
            sandbox.managed.set(COPILOT_SANDBOX_ALLOW_OUTBOUND_KEY, Some(PolicyValue::Boolean(false)));
            let denied = sandbox.read()[1..].to_vec();
            sandbox.managed.set(COPILOT_SANDBOX_ALLOW_BYPASS_KEY, Some(PolicyValue::Boolean(true)));
            sandbox.managed.set(COPILOT_SANDBOX_ALLOW_OUTBOUND_KEY, Some(PolicyValue::Boolean(true)));
            let allowed = sandbox.read();
            sandbox.managed.delete(COPILOT_SANDBOX_ALLOW_BYPASS_KEY);
            sandbox.managed.delete(COPILOT_SANDBOX_ALLOW_OUTBOUND_KEY);
            let keys = sandbox.keys.clone();
            assert_deep_strict_equal(
                &obj(vec![("denied", array(denied)), ("allowed", array(allowed)), ("removed", array(sandbox.read())), ("configured", sandbox.configured(&keys[1..]))]),
                &obj(vec![
                    ("denied", array(vec![element_state(boolean(false), true, true), element_state(boolean(false), true, true)])),
                    ("allowed", array(initial.clone())),
                    ("removed", array(initial)),
                    ("configured", array(vec![boolean(local_access), boolean(local_access)])),
                ]),
            );
        }
    }
}

#[test]
fn refreshes_existing_rows_and_restores_preferences_after_removal_in_target() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let _registration = register_sandbox_presentation_configuration("sandboxPresentationTest");
    for target in SANDBOX_TARGETS {
        let sandbox = SandboxModel::new(&store, target(), true, None, Vec::new());
        let initial = sandbox.read();
        sandbox.managed.set(COPILOT_SANDBOX_ENABLED_KEY, Some(PolicyValue::Boolean(true)));
        let required = sandbox.read();
        sandbox.managed.set(COPILOT_SANDBOX_ALLOW_BYPASS_KEY, Some(PolicyValue::Boolean(true)));
        let bypass_allowed = sandbox.read();
        sandbox.managed.delete(COPILOT_SANDBOX_ENABLED_KEY);
        sandbox.managed.delete(COPILOT_SANDBOX_ALLOW_BYPASS_KEY);

        let mut expected_bypass_allowed = vec![element_state(s("on"), true, true)];
        expected_bypass_allowed.extend(initial[1..].iter().cloned());
        assert_deep_strict_equal(
            &obj(vec![("required", array(required[..2].to_vec())), ("bypassAllowed", array(bypass_allowed)), ("removed", array(sandbox.read()))]),
            &obj(vec![
                ("required", array(vec![element_state(s("on"), true, true), element_state(boolean(false), true, true)])),
                ("bypassAllowed", array(expected_bypass_allowed)),
                ("removed", array(initial)),
            ]),
        );
    }
}

#[test]
fn runtime_restrictions_take_precedence_in_the_ui_and_leave_configuration_policies_intact() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let _registration = register_sandbox_presentation_configuration("sandboxPresentationTest");
    let sandbox = SandboxModel::with_defaults(&store);
    let reader = sandbox.configuration.base(&store);
    sandbox.configuration.service.stub_inspect_with(move |key, _| Rc::new(TestConfigurationValue { policy_value: reader.get_value(Some(key), None), ..Default::default() }));
    sandbox.managed.set(COPILOT_SANDBOX_ENABLED_KEY, Some(PolicyValue::Boolean(true)));
    let required = sandbox.read()[..2].to_vec();
    sandbox.managed.delete(COPILOT_SANDBOX_ENABLED_KEY);
    assert_deep_strict_equal(
        &obj(vec![("required", array(required)), ("removed", array(sandbox.read()[..2].to_vec()))]),
        &obj(vec![
            ("required", array(vec![element_state(s("on"), true, true), element_state(boolean(false), true, true)])),
            ("removed", array(vec![element_state(s("off"), true, true), element_state(boolean(true), true, true)])),
        ]),
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('SettingsTree ExP assignments')

/// O `createModel()` da suíte `SettingsTree ExP assignments`.
struct ExpModel {
    assignments: Rc<ExperimentalSettingsService>,
    model: Rc<SearchResultModel>,
    view_state: ViewStateRef,
    configuration: Rc<TestSettingsConfiguration>,
}

impl ExpModel {
    fn new(store: &LeakGuard) -> Self {
        let configuration = TestSettingsConfiguration::new(store, Some(obj(vec![("test.modified", boolean(true))])));
        let (services, assignments) = create_services(store, &configuration, false, null_managed_settings_service());
        let mut tag_filters = HashSet::new();
        tag_filters.insert(EXP_ASSIGNMENT_SETTING_TAG.to_owned());
        let view_state = Rc::new(RefCell::new(ISettingsEditorViewState { tag_filters: Some(tag_filters), ..ISettingsEditorViewState::new(SettingsTarget::UserLocal) }));
        let model = store.add_rc(SearchResultModel::new(view_state.clone(), None, true, services));
        let matches = ["test.assigned", "test.experimental", "test.modified", "test.spoofed"]
            .iter()
            .map(|key| {
                let tags = if *key == "test.experimental" {
                    vec!["experimental".to_owned()]
                } else if *key == "test.spoofed" {
                    vec![EXP_ASSIGNMENT_SETTING_TAG.to_owned()]
                } else {
                    Vec::new()
                };
                match_of(ISetting { scope: Some(ConfigurationScope::Resource), tags: Some(tags), ..mock_setting(key, "boolean") }, SettingMatchType::None)
            })
            .collect();
        model.set_result(SearchResultIdx::Local, Some(search_result(matches, false)));
        Self { assignments, model, view_state, configuration }
    }

    fn read(&self) -> Dynamic {
        obj(vec![
            ("count", number(self.model.get_unique_results_count())),
            (
                "settings",
                array(
                    setting_children(&self.model.root())
                        .iter()
                        .map(|child| obj(vec![("key", s(&child.setting.key)), ("assigned", boolean(child.has_ex_p_assignment()))]))
                        .collect(),
                ),
            ),
        ])
    }

    fn add_tag_filter(&self, tag: &str) {
        self.view_state.borrow_mut().tag_filters.as_mut().expect("o estado tem filtros de tag").insert(tag.to_owned());
    }

    fn remove_tag_filter(&self, tag: &str) {
        self.view_state.borrow_mut().tag_filters.as_mut().expect("o estado tem filtros de tag").remove(tag);
    }
}

/// `{ count, settings: [{ key, assigned: true }...] }`.
fn assigned_state(keys: &[&str]) -> Dynamic {
    obj(vec![
        ("count", number(keys.len())),
        ("settings", array(keys.iter().map(|key| obj(vec![("key", s(key)), ("assigned", boolean(true))])).collect())),
    ])
}

#[test]
fn filters_real_assignments_not_experimental_or_statically_supplied_tags() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let exp = ExpModel::new(&store);
    let empty = exp.read();
    exp.assignments.set_assignment("test.assigned", true);
    exp.assignments.set_assignment("test.modified", true);
    exp.model.update_children();
    let assigned = exp.read();
    exp.assignments.set_assignment("test.assigned", false);
    exp.model.update_children();

    assert_deep_strict_equal(
        &obj(vec![("empty", empty), ("assigned", assigned), ("removed", exp.read())]),
        &obj(vec![("empty", assigned_state(&[])), ("assigned", assigned_state(&["test.assigned", "test.modified"])), ("removed", assigned_state(&["test.modified"]))]),
    );
}

#[test]
fn composes_with_modified_and_settings_scope_without_changing_assignment_state() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let exp = ExpModel::new(&store);
    exp.assignments.set_assignment("test.assigned", true);
    exp.assignments.set_assignment("test.modified", true);
    exp.add_tag_filter("modified");
    exp.model.update_children();
    let user = exp.read();
    exp.view_state.borrow_mut().settings_target = SettingsTarget::Workspace;
    exp.model.update_children();
    let workspace = exp.read();
    exp.remove_tag_filter("modified");
    exp.model.update_children();

    assert_deep_strict_equal(
        &obj(vec![("user", user), ("workspace", workspace), ("assignedInWorkspace", exp.read())]),
        &obj(vec![
            ("user", assigned_state(&["test.modified"])),
            ("workspace", assigned_state(&[])),
            ("assignedInWorkspace", assigned_state(&["test.assigned", "test.modified"])),
        ]),
    );
}

#[test]
fn resetting_a_modified_setting_leaves_its_assignment_intact() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let exp = ExpModel::new(&store);
    exp.assignments.set_assignment("test.modified", true);
    exp.add_tag_filter("modified");
    exp.model.update_children();
    let before_reset = exp.read();
    exp.configuration.set_user_configuration("test.modified", Dynamic::Undefined);
    exp.model.update_children();
    let after_reset = exp.read();
    exp.remove_tag_filter("modified");
    exp.model.update_children();

    assert_deep_strict_equal(
        &obj(vec![("beforeReset", before_reset), ("afterReset", after_reset), ("assignmentOnly", exp.read())]),
        &obj(vec![("beforeReset", assigned_state(&["test.modified"])), ("afterReset", assigned_state(&[])), ("assignmentOnly", assigned_state(&["test.modified"]))]),
    );
}

#[test]
fn policy_values_do_not_hide_an_assignment_or_mark_it_as_user_modified() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let exp = ExpModel::new(&store);
    exp.assignments.set_assignment("test.modified", true);
    let reader = exp.configuration.base(&store);
    exp.configuration.service.stub_inspect_with(move |key, _| {
        let value = reader.get_value(Some(key), None);
        Rc::new(TestConfigurationValue { value: value.clone(), policy_value: value, ..Default::default() })
    });
    exp.model.update_children();
    let element = first_element(&exp.model, "test.modified");
    let mut assignment_filter = HashSet::new();
    assignment_filter.insert(EXP_ASSIGNMENT_SETTING_TAG.to_owned());
    assert_deep_strict_equal(
        &obj(vec![
            ("assigned", boolean(element.has_ex_p_assignment())),
            ("policy", boolean(element.has_policy_value())),
            ("modified", boolean(element.is_configured())),
            ("matchesAssignment", boolean(element.matches_all_tags(Some(&assignment_filter)))),
        ]),
        &obj(vec![("assigned", boolean(true)), ("policy", boolean(true)), ("modified", boolean(false)), ("matchesAssignment", boolean(true))]),
    );
}

#[test]
fn parses_assignment_and_modified_filters_separately_from_free_text() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        parse_query("@modified @tag:expassigned font"),
        IParsedQuery {
            tags: vec![EXP_ASSIGNMENT_SETTING_TAG.to_owned(), "modified".to_owned()],
            extension_filters: vec![],
            feature_filters: vec![],
            id_filters: vec![],
            language_filter: None,
            query: "font".to_owned(),
        }
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('SettingsTree deprecation warnings')

/// `search ? SearchResultModel : SettingsTreeModel`, com o `update` e o `root` que o teste usa dos dois.
enum TreeModel {
    Plain(Rc<SettingsTreeModel>),
    Search(Rc<SearchResultModel>),
}

impl TreeModel {
    fn update(&self, settings: &[Rc<ISetting>]) {
        match self {
            Self::Search(model) => {
                let matches = settings.iter().map(|setting| ISettingMatch::new(setting.clone(), Some(Vec::new()), SettingMatchType::None, 0.0, 0.0)).collect();
                model.set_result(SearchResultIdx::Local, Some(search_result(matches, false)));
            }
            Self::Plain(model) => model.update(&toc_entry("deprecationWarnings", "", settings.to_vec())),
        }
    }

    fn root(&self) -> Rc<SettingsTreeGroupElement> {
        match self {
            Self::Search(model) => model.root(),
            Self::Plain(model) => model.root(),
        }
    }
}

#[test]
fn keeps_opted_in_warnings_visible_without_exposing_unconfigured_deprecated_settings() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    for search in [false, true] {
        let configuration = TestSettingsConfiguration::new(&store, Some(obj(vec![("test.configured", boolean(true))])));
        let (services, _) = create_services(&store, &configuration, false, null_managed_settings_service());
        let view_state = view_state(SettingsTarget::UserLocal);
        let model = if search {
            TreeModel::Search(store.add_rc(SearchResultModel::new(view_state, None, true, services)))
        } else {
            TreeModel::Plain(store.add_rc(SettingsTreeModel::new(view_state, true, services)))
        };
        let settings: Vec<Rc<ISetting>> = ["test.upcoming", "test.deprecated", "test.configured", "test.explicitlyHidden", "test.normal"]
            .iter()
            .map(|key| {
                Rc::new(ISetting {
                    scope: Some(ConfigurationScope::Resource),
                    deprecation_message: if *key == "test.normal" { None } else { Some("This setting will be deprecated soon.".to_owned()) },
                    deprecation_message_show_in_settings: if *key == "test.upcoming" {
                        Some(true)
                    } else if *key == "test.explicitlyHidden" {
                        Some(false)
                    } else {
                        None
                    },
                    ..mock_setting(key, "boolean")
                })
            })
            .collect();
        let visible = || {
            array(
                setting_children(&model.root())
                    .iter()
                    .map(|child| {
                        obj(vec![("key", s(&child.setting.key)), ("warning", child.setting.deprecation_message.as_deref().map_or(Dynamic::Undefined, s))])
                    })
                    .collect(),
            )
        };
        model.update(&settings);
        let initial = visible();
        configuration.set_user_configuration("test.configured", Dynamic::Undefined);
        model.update(&settings);
        let warning = |key: &str, warning: Option<&str>| obj(vec![("key", s(key)), ("warning", warning.map_or(Dynamic::Undefined, s))]);
        let deprecated = Some("This setting will be deprecated soon.");
        assert_deep_strict_equal(
            &obj(vec![("initial", initial), ("reset", visible())]),
            &obj(vec![
                ("initial", array(vec![warning("test.upcoming", deprecated), warning("test.configured", deprecated), warning("test.normal", None)])),
                ("reset", array(vec![warning("test.upcoming", deprecated), warning("test.normal", None)])),
            ]),
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('SettingsTree sandbox network search ordering')

#[test]
fn sandbox_search_puts_user_configured_paths_before_allowed_and_denied_domains() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let configuration = TestSettingsConfiguration::new(&store, None);
    let (services, _) = create_services(&store, &configuration, false, null_managed_settings_service());
    let keys = [
        AgentSandboxSettingId::AgentSandboxUserConfiguredPaths.as_str(),
        AgentNetworkDomainSettingId::AllowedNetworkDomains.as_str(),
        AgentNetworkDomainSettingId::DeniedNetworkDomains.as_str(),
    ];
    let state = Rc::new(RefCell::new(ISettingsEditorViewState { query: Some("sandbox".to_owned()), ..ISettingsEditorViewState::new(SettingsTarget::UserLocal) }));
    let order: HashMap<String, f64> = keys.iter().enumerate().map(|(index, key)| ((*key).to_owned(), index as f64)).collect();
    let model = store.add_rc(SearchResultModel::new(state, Some(order), true, services));
    let matches: Vec<ISettingMatch> = keys
        .iter()
        .rev()
        .map(|key| {
            let setting = ISetting {
                scope: Some(ConfigurationScope::Application),
                key_range: Range { start_line_number: 1, start_column: 1, end_line_number: 1, end_column: key.len() as i32 + 1 },
                ..mock_setting(key, "array")
            };
            let matched = SettingMatches::new("sandbox", &setting, true, &*configuration);
            ISettingMatch::new(Rc::new(setting), Some(matched.matches), matched.match_type, matched.key_match_score, 0.0)
        })
        .collect();
    model.set_result(SearchResultIdx::Local, Some(search_result(matches, false)));
    let actual: Vec<(String, SettingMatchType)> =
        model.get_unique_search_results().expect("há resultados").filter_matches.iter().map(|m| (m.setting.key.clone(), m.match_type)).collect();
    let expected: Vec<(String, SettingMatchType)> =
        keys.iter().map(|key| ((*key).to_owned(), SettingMatchType::AllWordsInSettingsLabel | SettingMatchType::ContiguousQueryInSettingId)).collect();
    assert_eq!(actual, expected);
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('SettingsTree search results')

fn create_search_results_model(store: &LeakGuard) -> Rc<SearchResultModel> {
    let configuration = TestSettingsConfiguration::new(store, None);
    let (services, _) = create_services(store, &configuration, false, null_managed_settings_service());
    let state = Rc::new(RefCell::new(ISettingsEditorViewState { query: Some("test".to_owned()), ..ISettingsEditorViewState::new(SettingsTarget::UserLocal) }));
    store.add_rc(SearchResultModel::new(state, None, true, services))
}

/// `createMatch(key)`.
fn create_match(key: &str) -> ISettingMatch {
    match_of(ISetting { scope: Some(ConfigurationScope::Application), ..mock_setting(key, "boolean") }, SettingMatchType::RemoteMatch)
}

#[test]
fn refreshes_after_local_results_are_cleared() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_search_results_model(&store);
    model.set_result(SearchResultIdx::Local, Some(search_result(vec![create_match("test.local")], true)));
    model.set_result(SearchResultIdx::Local, None);
    model.update_children();

    assert_eq!(model.get_unique_search_results(), Some(search_result(vec![], false)));
    assert_eq!(model.get_unique_results_count(), 0);
}

#[test]
fn retains_remote_results_without_local_results() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_search_results_model(&store);
    let matched = create_match("test.remote");
    model.set_result(SearchResultIdx::Local, None);
    model.set_result(SearchResultIdx::Remote, Some(search_result(vec![matched.clone()], false)));
    model.update_children();

    let keys: Vec<String> = model.root().children().iter().map(|child| child.as_setting().map_or_else(|| child.id(), |setting| setting.setting.key.clone())).collect();
    assert_eq!(model.get_unique_search_results(), Some(search_result(vec![matched], false)));
    assert_eq!(keys, vec!["test.remote".to_owned()]);
    assert_eq!(model.get_unique_results_count(), 1);
}

#[test]
fn switches_from_ai_results_without_local_results() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_search_results_model(&store);
    model.set_show_ai_results(true);
    model.set_result(SearchResultIdx::Embeddings, Some(search_result(vec![create_match("test.ai")], false)));
    model.set_show_ai_results(false);
    model.update_children();

    assert_eq!(model.get_unique_search_results(), Some(search_result(vec![], false)));
    assert_eq!(model.get_unique_results_count(), 0);
}

#[test]
fn preserves_local_exact_matches_and_remote_deduplication() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_search_results_model(&store);
    let local_match = create_match("test.local");
    let remote_match = create_match("test.remote");
    model.set_result(SearchResultIdx::Local, Some(search_result(vec![local_match.clone()], true)));
    model.set_result(SearchResultIdx::Remote, Some(search_result(vec![local_match.clone(), remote_match.clone()], false)));

    assert_eq!(model.get_unique_search_results(), Some(search_result(vec![local_match, remote_match], true)));
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('SettingsTree')

/// `{ category, label }`.
fn display(category: &str, label: &str) -> SettingDisplayFormat {
    SettingDisplayFormat { category: category.to_owned(), label: label.to_owned() }
}

/// `settingKeyToDisplayFormat(key, groupId)`.
fn display_of(key: &str, group_id: &str) -> SettingDisplayFormat {
    setting_key_to_display_format(key, group_id, false)
}

#[test]
fn setting_key_to_display_format_sandbox_outbound_connections() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let allow_network = AgentSandboxSettingId::AgentSandboxAllowNetwork.as_str();
    assert_eq!(
        vec![
            display_of(allow_network, ""),
            display_of(allow_network, "chat"),
            display_of(allow_network, "chat.agent.sandbox"),
            display_of(allow_network, "chat.agent.sandbox.network"),
            display_of("other.allowNetwork", ""),
        ],
        vec![
            display("Chat › Agent › Sandbox › Network", "Allow Outbound Connections"),
            display("Agent › Sandbox › Network", "Allow Outbound Connections"),
            display("Network", "Allow Outbound Connections"),
            display("", "Allow Outbound Connections"),
            display("Other", "Allow Network"),
        ]
    );
}

#[test]
fn setting_key_to_display_format_allowed_network_domains() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let allowed = AgentNetworkDomainSettingId::AllowedNetworkDomains.as_str();
    assert_eq!(
        vec![display_of(allowed, ""), display_of(allowed, "chat"), display_of(allowed, "chat.agent.sandbox"), display_of(allowed, "chat.agent.sandbox.network")],
        vec![
            display("Chat › Agent › Sandbox › Network", "Allowed Domains"),
            display("Agent › Sandbox › Network", "Allowed Domains"),
            display("Network", "Allowed Domains"),
            display("", "Allowed Domains"),
        ]
    );
}

#[test]
fn setting_key_to_display_format_denied_network_domains() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let denied = AgentNetworkDomainSettingId::DeniedNetworkDomains.as_str();
    assert_eq!(
        vec![display_of(denied, ""), display_of(denied, "chat.agent.sandbox"), display_of(denied, "chat.agent.sandbox.network")],
        vec![display("Chat › Agent › Sandbox › Network", "Denied Domains"), display("Network", "Denied Domains"), display("", "Denied Domains")]
    );
}

#[test]
fn setting_key_to_display_format_sandbox_bypass() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let bypass = AgentSandboxSettingId::AgentSandboxAllowUnsandboxedCommands.as_str();
    assert_eq!(
        vec![display_of(bypass, ""), display_of(bypass, "chat"), display_of(bypass, "chat.agent.sandbox"), display_of("other.allowUnsandboxedCommands", "")],
        vec![
            display("Chat › Agent › Sandbox", "Allow Sandbox Bypass"),
            display("Agent › Sandbox", "Allow Sandbox Bypass"),
            display("", "Allow Sandbox Bypass"),
            display("Other", "Allow Unsandboxed Commands"),
        ]
    );
}

#[test]
fn setting_key_to_display_format_sandbox_local_network() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let keys = [AgentSandboxSettingId::AgentSandboxAllowNetwork.as_str(), AgentSandboxSettingId::AgentSandboxAllowLocalNetwork.as_str()];
    let displays: Vec<SettingDisplayFormat> =
        ["", "chat", "chat.agent.sandbox", "chat.agent.sandbox.network"].iter().map(|group| display_of(AgentSandboxSettingId::AgentSandboxAllowLocalNetwork.as_str(), group)).collect();
    assert_eq!(keys, ["chat.agent.sandbox.network.allowNetwork", "chat.agent.sandbox.network.allowLocalNetwork"]);
    assert_eq!(
        displays,
        vec![
            display("Chat › Agent › Sandbox › Network", "Allow Local Network"),
            display("Agent › Sandbox › Network", "Allow Local Network"),
            display("Network", "Allow Local Network"),
            display("", "Allow Local Network"),
        ]
    );
}

#[test]
fn setting_key_to_display_format_basic_keys() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(display_of("foo.bar", ""), display("Foo", "Bar"));
    assert_eq!(display_of("foo.bar.etc", ""), display("Foo › Bar", "Etc"));
    assert_eq!(display_of("fooBar.etcSomething", ""), display("Foo Bar", "Etc Something"));
    assert_eq!(display_of("foo", ""), display("", "Foo"));
    assert_eq!(display_of("foo.1leading.number", ""), display("Foo › 1leading", "Number"));
    assert_eq!(display_of("foo.1Leading.number", ""), display("Foo › 1 Leading", "Number"));
}

#[test]
fn setting_key_to_display_format_sandbox_placeholder_settings() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        vec![
            display_of(AgentSandboxSettingId::AgentSandboxMcpServers.as_str(), "chat.agent.sandbox"),
            display_of(AgentSandboxSettingId::AgentSandboxLspServers.as_str(), "chat.agent.sandbox"),
        ],
        vec![display("", "Sandbox MCP Servers"), display("", "Sandbox LSP Servers")]
    );
}

#[test]
fn setting_key_to_display_format_sandbox_developer_tool_file_system_access() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let key = AgentSandboxSettingId::AgentSandboxAllowDevToolAccess.as_str();
    let displays: Vec<SettingDisplayFormat> = ["", "chat", "chat.agent.sandbox", "chat.agent.sandbox.fileSystem"].iter().map(|group| display_of(key, group)).collect();
    assert_eq!(key, "chat.agent.sandbox.fileSystem.allowDevToolAccess");
    assert_eq!(
        displays,
        vec![
            display("Chat › Agent › Sandbox › File System", "Allow Dev Tool Access"),
            display("Agent › Sandbox › File System", "Allow Dev Tool Access"),
            display("File System", "Allow Dev Tool Access"),
            display("", "Allow Dev Tool Access"),
        ]
    );
}

#[test]
fn setting_key_to_display_format_sandbox_credential_authentication() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    for (key, label) in [(AgentSandboxSettingId::AgentSandboxAuthenticateGit.as_str(), "Authenticate git"), (AgentSandboxSettingId::AgentSandboxAuthenticateGh.as_str(), "Authenticate gh")] {
        let displays: Vec<SettingDisplayFormat> = ["", "chat.agent.sandbox", "chat.agent.sandbox.credentials"].iter().map(|group| display_of(key, group)).collect();
        assert_eq!(displays, vec![display("Chat › Agent › Sandbox › Credentials", label), display("Credentials", label), display("", label)]);
    }
}

#[test]
fn setting_key_to_display_format_sandbox_user_configured_paths() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let displays: Vec<SettingDisplayFormat> = ["", "chat.agent.sandbox", "chat.agent.sandbox.fileSystem"]
        .iter()
        .map(|group| display_of(AgentSandboxSettingId::AgentSandboxUserConfiguredPaths.as_str(), group))
        .collect();
    assert_eq!(
        displays,
        vec![display("Chat › Agent › Sandbox › File System", "User-Configured Paths"), display("File System", "User-Configured Paths"), display("", "User-Configured Paths")]
    );
}

#[test]
fn setting_key_to_display_format_with_category() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(display_of("foo.bar", "foo"), display("", "Bar"));
    assert_eq!(display_of("disableligatures.ligatures", "disableligatures"), display("", "Ligatures"));
    assert_eq!(display_of("foo.bar.etc", "foo"), display("Bar", "Etc"));
    assert_eq!(display_of("fooBar.etcSomething", "foo"), display("Foo Bar", "Etc Something"));
    assert_eq!(display_of("foo.bar.etc", "foo/bar"), display("", "Etc"));
    assert_eq!(display_of("foo.bar.etc", "something/foo"), display("Bar", "Etc"));
    assert_eq!(display_of("bar.etc", "something.bar"), display("", "Etc"));
    assert_eq!(display_of("fooBar.etc", "fooBar"), display("", "Etc"));
    assert_eq!(display_of("fooBar.somethingElse.etc", "fooBar"), display("Something Else", "Etc"));
}

#[test]
fn setting_key_to_display_format_known_acronym_or_term() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(display_of("css.someCssSetting", ""), display("CSS", "Some CSS Setting"));
    assert_eq!(display_of("powershell.somePowerShellSetting", ""), display("PowerShell", "Some PowerShell Setting"));
    assert_eq!(display_of("ocaml.server.extendedHover", ""), display("OCaml › Server", "Extended Hover"));
}

#[test]
fn parse_query_cases() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let test_parse_query = |input: &str, expected: IParsedQuery| assert_eq!(parse_query(input), expected, "{input}");
    let parsed = |tags: &[&str], extension_filters: &[&str], query: &str, feature_filters: &[&str], id_filters: &[&str], language_filter: Option<&str>| IParsedQuery {
        tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
        extension_filters: extension_filters.iter().map(|filter| (*filter).to_owned()).collect(),
        query: query.to_owned(),
        feature_filters: feature_filters.iter().map(|filter| (*filter).to_owned()).collect(),
        id_filters: id_filters.iter().map(|filter| (*filter).to_owned()).collect(),
        language_filter: language_filter.map(str::to_owned),
    };

    test_parse_query("", parsed(&[], &[], "", &[], &[], None));
    test_parse_query("@modified", parsed(&["modified"], &[], "", &[], &[], None));
    test_parse_query("@tag:foo", parsed(&["foo"], &[], "", &[], &[], None));
    test_parse_query("@modified foo", parsed(&["modified"], &[], "foo", &[], &[], None));
    test_parse_query("@tag:foo @modified", parsed(&["foo", "modified"], &[], "", &[], &[], None));
    test_parse_query("@tag:foo @modified my query", parsed(&["foo", "modified"], &[], "my query", &[], &[], None));
    test_parse_query("test @modified query", parsed(&["modified"], &[], "test  query", &[], &[], None));
    test_parse_query("test @modified", parsed(&["modified"], &[], "test", &[], &[], None));
    test_parse_query("query has @ for some reason", parsed(&[], &[], "query has @ for some reason", &[], &[], None));
    test_parse_query("@ext:github.vscode-pull-request-github", parsed(&[], &["github.vscode-pull-request-github"], "", &[], &[], None));
    test_parse_query("@ext:github.vscode-pull-request-github,vscode.git", parsed(&[], &["github.vscode-pull-request-github", "vscode.git"], "", &[], &[], None));
    test_parse_query("@feature:scm", parsed(&[], &[], "", &["scm"], &[], None));
    test_parse_query("@feature:scm,terminal", parsed(&[], &[], "", &["scm", "terminal"], &[], None));
    test_parse_query("@id:files.autoSave", parsed(&[], &[], "", &[], &["files.autoSave"], None));
    test_parse_query("@id:files.autoSave,terminal.integrated.commandsToSkipShell", parsed(&[], &[], "", &[], &["files.autoSave", "terminal.integrated.commandsToSkipShell"], None));
    test_parse_query("@lang:cpp", parsed(&[], &[], "", &[], &[], Some("cpp")));
    test_parse_query("@lang:cpp,python", parsed(&[], &[], "", &[], &[], Some("cpp")));
}

#[test]
fn sanitize_id_replaces_all_dots_and_slashes() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        vec![sanitize_id("root.editor.font.size"), sanitize_id("group/subgroup/setting.key"), sanitize_id("no-special-chars"), sanitize_id("single.dot")],
        vec!["root_editor_font_size", "group_subgroup_setting_key", "no-special-chars", "single_dot"]
    );
}
