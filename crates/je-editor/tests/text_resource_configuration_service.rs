//! Porte de `src/vs/editor/test/common/services/textResourceConfigurationService.test.ts` (suíte
//! `TextResourceConfigurationService - Update`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso. O serviço de
//!   configuração do original é criado no corpo da suíte, antes do rastreador, então aqui ele nasce antes do guarda.
//! - `language` e `configurationValue` são variáveis do módulo no original; viram o [`Suite`], criado a cada caso
//!   com o valor inicial (`language = null`, `configurationValue = {}`), e cada caso as atribui como o original.
//! - `configurationValue` (um `IConfigurationValue` literal) vira o [`TestConfigurationValue`], com uma camada
//!   `{ value, override }` por escopo; `inspect()` monta o `ConfigurationInspectValue` a partir de modelos de
//!   configuração por camada (a chave é sempre `a`, e o identificador do override é a linguagem do caso).
//! - As chaves `workspaceValue` e `workspaceFolderValue` de dois casos do original não existem em
//!   `IConfigurationValue` (os campos são `workspace` e `workspaceFolder`) e o serviço nunca as lê, então a camada
//!   fica ausente.
//! - `updateArgs = [...arguments]` é o [`UpdateValueCall`], e o `{ resource, overrideIdentifier }` esperado é o
//!   `ConfigurationUpdateOverrides` equivalente (`overrideIdentifier: undefined` é `override_identifiers: None`).
//! - `instantiationService.stub(IModelService, { getModel() { return null; } })` e o equivalente do
//!   `ILanguageService` são structs de stub parcial: os métodos que o original não define entram em pânico.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::path::Platform;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::language_service::{ILanguageSelection, ILanguageService, I_LANGUAGE_SERVICE};
use je_editor::languages::ILanguageIdCodec;
use je_editor::languages_registry::LanguageNameIdPair;
use je_editor::modes_registry::{LanguageExtensionPoint, LanguageIcon};
use je_editor::services_model::{IModelService, I_MODEL_SERVICE};
use je_editor::text_model::TextModel;
use je_editor::text_resource_configuration_service::{ITextResourceConfigurationService, TextResourceConfigurationService};
use je_platform::configuration::models::{ConfigurationInspectValue, ConfigurationModel};
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::configuration::{ConfigurationOverrides, ConfigurationTarget, ConfigurationUpdateOverrides, Overrides};
use je_platform::log::NullLogService;
use support::test_configuration_service::{TestConfigurationService, UpdateValueCall};
use support::test_instantiation_service::TestInstantiationService;

/// `{ value, override }` de uma camada do `IConfigurationValue`.
struct Layer {
    value: &'static str,
    override_value: Option<&'static str>,
}

fn layer(value: &'static str) -> Layer {
    Layer { value, override_value: None }
}

fn layer_with_override(value: &'static str, override_value: &'static str) -> Layer {
    Layer { value, override_value: Some(override_value) }
}

/// `IConfigurationValue<any>`, com as camadas que os casos usam.
#[derive(Default)]
struct TestConfigurationValue {
    default: Option<Layer>,
    user_local: Option<Layer>,
    user_remote: Option<Layer>,
    workspace: Option<Layer>,
    workspace_folder: Option<Layer>,
    override_identifiers: Option<Vec<&'static str>>,
}

const KEY: &str = "a";

fn layer_model(layer: &Option<Layer>, language: Option<&str>) -> ConfigurationModel {
    let log = NullLogService::shared();
    match layer {
        None => ConfigurationModel::create_empty_model(log),
        Some(layer) => {
            let overrides = match (layer.override_value, language) {
                (Some(override_value), Some(language)) => {
                    vec![Overrides::new(&[language], Dynamic::object([(KEY, Dynamic::from(override_value))]), &[KEY])]
                }
                _ => Vec::new(),
            };
            ConfigurationModel::new(Dynamic::object([(KEY, Dynamic::from(layer.value))]), vec![JsString::from(KEY)], overrides, None, log)
        }
    }
}

/// O `ConfigurationInspectValue` que equivale ao literal `configurationValue` do original.
fn inspect_value(configuration_value: &TestConfigurationValue, language: Option<&str>) -> ConfigurationInspectValue {
    let model = |layer: &Option<Layer>| layer_model(layer, language);
    ConfigurationInspectValue {
        key: JsString::from(KEY),
        overrides: ConfigurationOverrides { resource: None, override_identifier: language.map(JsString::from) },
        value: Dynamic::Undefined,
        override_identifiers: configuration_value
            .override_identifiers
            .as_ref()
            .map(|identifiers| identifiers.iter().map(|identifier| JsString::from(*identifier)).collect()),
        default_configuration: model(&configuration_value.default),
        policy_configuration: None,
        application_configuration: None,
        user_configuration: model(&configuration_value.user_local),
        local_user_configuration: model(&configuration_value.user_local),
        remote_user_configuration: model(&configuration_value.user_remote),
        workspace_configuration: Some(model(&configuration_value.workspace)),
        folder_configuration_model: Some(model(&configuration_value.workspace_folder)),
        memory_configuration_model: ConfigurationModel::create_empty_model(NullLogService::shared()),
    }
}

/// O que o corpo da suíte guarda: o serviço de configuração, as variáveis do módulo e o `updateArgs`.
struct Suite {
    configuration_service: Rc<TestConfigurationService>,
    configuration_value: Rc<RefCell<TestConfigurationValue>>,
    language: Rc<RefCell<Option<String>>>,
    update_args: Rc<RefCell<Option<UpdateValueCall>>>,
}

impl Suite {
    fn new() -> Suite {
        let configuration_service = TestConfigurationService::new(None);
        let configuration_value = Rc::new(RefCell::new(TestConfigurationValue::default()));
        let language: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let update_args = Rc::new(RefCell::new(None));
        configuration_service.set_inspect_override({
            let (configuration_value, language) = (configuration_value.clone(), language.clone());
            move |_, _| inspect_value(&configuration_value.borrow(), language.borrow().as_deref())
        });
        configuration_service.set_update_value_override({
            let update_args = update_args.clone();
            move |call| {
                *update_args.borrow_mut() = Some(call);
                Promise::resolved(())
            }
        });
        Suite { configuration_service, configuration_value, language, update_args }
    }

    /// `language = ...; configurationValue = ...;`
    fn set(&self, language: &str, configuration_value: TestConfigurationValue) {
        *self.language.borrow_mut() = Some(language.to_owned());
        *self.configuration_value.borrow_mut() = configuration_value;
    }

    /// O `setup` da suíte: devolve o `testObject`.
    fn setup(&self, disposables: &LeakGuard) -> Rc<TextResourceConfigurationService> {
        let instantiation_service = disposables.add_rc(TestInstantiationService::new());
        instantiation_service.stub(I_MODEL_SERVICE, Rc::new(ModelServiceStub) as Rc<dyn IModelService>);
        instantiation_service.stub(I_LANGUAGE_SERVICE, Rc::new(LanguageServiceStub { language: self.language.clone() }) as Rc<dyn ILanguageService>);
        instantiation_service.stub(I_CONFIGURATION_SERVICE, self.configuration_service.clone() as Rc<dyn IConfigurationService>);
        disposables.add_rc(instantiation_service.create_instance::<TextResourceConfigurationService>(()))
    }

    fn update_args(&self) -> UpdateValueCall {
        self.update_args.borrow().clone().expect("updateValue foi chamado")
    }
}

/// `{ getModel() { return null; } }`.
struct ModelServiceStub;

impl IModelService for ModelServiceStub {
    fn get_model(&self, _resource: &URI) -> Option<Rc<TextModel>> {
        None
    }
}

/// `{ guessLanguageIdByFilepathOrFirstLine() { return language; } }`: o resto não existe no objeto do original.
struct LanguageServiceStub {
    language: Rc<RefCell<Option<String>>>,
}

fn absent(method: &str) -> ! {
    panic!("`{method}` não existe no stub parcial do ILanguageService")
}

impl ILanguageService for LanguageServiceStub {
    fn language_id_codec(&self) -> Rc<dyn ILanguageIdCodec> {
        absent("languageIdCodec")
    }
    fn on_did_request_basic_language_features(&self) -> Event<String> {
        absent("onDidRequestBasicLanguageFeatures")
    }
    fn on_did_request_rich_language_features(&self) -> Event<String> {
        absent("onDidRequestRichLanguageFeatures")
    }
    fn on_did_change(&self) -> Event<()> {
        absent("onDidChange")
    }
    fn register_language(&self, _def: LanguageExtensionPoint) -> DisposableRef {
        absent("registerLanguage")
    }
    fn is_registered_language_id(&self, _language_id: Option<&str>) -> bool {
        absent("isRegisteredLanguageId")
    }
    fn get_registered_language_ids(&self) -> Vec<String> {
        absent("getRegisteredLanguageIds")
    }
    fn get_sorted_registered_language_names(&self) -> Vec<LanguageNameIdPair> {
        absent("getSortedRegisteredLanguageNames")
    }
    fn get_language_name(&self, _language_id: &str) -> Option<String> {
        absent("getLanguageName")
    }
    fn get_mime_type(&self, _language_id: &str) -> Option<String> {
        absent("getMimeType")
    }
    fn get_icon(&self, _language_id: &str) -> Option<LanguageIcon> {
        absent("getIcon")
    }
    fn get_extensions(&self, _language_id: &str) -> Vec<String> {
        absent("getExtensions")
    }
    fn get_filenames(&self, _language_id: &str) -> Vec<String> {
        absent("getFilenames")
    }
    fn get_configuration_files(&self, _language_id: &str) -> Vec<URI> {
        absent("getConfigurationFiles")
    }
    fn get_language_id_by_language_name(&self, _language_name: &str) -> Option<String> {
        absent("getLanguageIdByLanguageName")
    }
    fn get_language_id_by_mime_type(&self, _mime_type: Option<&str>) -> Option<String> {
        absent("getLanguageIdByMimeType")
    }
    fn guess_language_id_by_filepath_or_first_line(&self, _resource: Option<&URI>, _first_line: Option<&JsString>) -> Option<String> {
        self.language.borrow().clone()
    }
    fn create_by_id(&self, _language_id: Option<&str>) -> Rc<dyn ILanguageSelection> {
        absent("createById")
    }
    fn create_by_mime_type(&self, _mime_type: Option<&str>) -> Rc<dyn ILanguageSelection> {
        absent("createByMimeType")
    }
    fn create_by_filepath_or_first_line(&self, _resource: Option<&URI>, _first_line: Option<&JsString>) -> Rc<dyn ILanguageSelection> {
        absent("createByFilepathOrFirstLine")
    }
    fn request_basic_language_features(&self, _language_id: &str) {
        absent("requestBasicLanguageFeatures")
    }
    fn request_rich_language_features(&self, _language_id: &str) {
        absent("requestRichLanguageFeatures")
    }
}

/// `URI.file('someFile')`.
fn some_file() -> URI {
    URI::file("someFile", Platform::Posix)
}

/// O `['a', value, { resource, overrideIdentifier }, target]` esperado.
fn expected(value: &str, resource: &URI, override_identifier: Option<&str>, target: ConfigurationTarget) -> UpdateValueCall {
    UpdateValueCall {
        key: KEY.to_owned(),
        value: Dynamic::from(value),
        overrides: Some(ConfigurationUpdateOverrides {
            override_identifiers: override_identifier.map(|identifier| vec![JsString::from(identifier)]),
            resource: Some(resource.clone()),
        }),
        target: Some(target),
    }
}

#[test]
fn update_value_writes_without_target_and_overrides_when_no_language_is_defined() {
    let suite = Suite::new();
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_object = suite.setup(&ds);
    let resource = some_file();
    let _ = test_object.update_value(Some(&resource), "a", Dynamic::from("b"), None);
    assert_eq!(suite.update_args(), expected("b", &resource, None, ConfigurationTarget::UserLocal));
}

#[test]
fn update_value_writes_with_target_and_without_overrides_when_no_language_is_defined() {
    let suite = Suite::new();
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_object = suite.setup(&ds);
    let resource = some_file();
    let _ = test_object.update_value(Some(&resource), "a", Dynamic::from("b"), Some(ConfigurationTarget::UserLocal));
    assert_eq!(suite.update_args(), expected("b", &resource, None, ConfigurationTarget::UserLocal));
}

#[test]
fn update_value_writes_into_given_memory_target_without_overrides() {
    let suite = Suite::new();
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_object = suite.setup(&ds);
    suite.set(
        "a",
        TestConfigurationValue {
            default: Some(layer("1")),
            user_local: Some(layer("2")),
            workspace_folder: Some(layer("1")),
            ..Default::default()
        },
    );
    let resource = some_file();

    let _ = test_object.update_value(Some(&resource), "a", Dynamic::from("b"), Some(ConfigurationTarget::Memory));
    assert_eq!(suite.update_args(), expected("b", &resource, None, ConfigurationTarget::Memory));
}

#[test]
fn update_value_writes_into_given_workspace_target_without_overrides() {
    let suite = Suite::new();
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_object = suite.setup(&ds);
    suite.set(
        "a",
        TestConfigurationValue {
            default: Some(layer("1")),
            user_local: Some(layer("2")),
            workspace_folder: Some(layer("2")),
            ..Default::default()
        },
    );
    let resource = some_file();

    let _ = test_object.update_value(Some(&resource), "a", Dynamic::from("b"), Some(ConfigurationTarget::Workspace));
    assert_eq!(suite.update_args(), expected("b", &resource, None, ConfigurationTarget::Workspace));
}

#[test]
fn update_value_writes_into_given_user_target_without_overrides() {
    let suite = Suite::new();
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_object = suite.setup(&ds);
    suite.set(
        "a",
        TestConfigurationValue {
            default: Some(layer("1")),
            user_local: Some(layer("2")),
            workspace_folder: Some(layer("2")),
            ..Default::default()
        },
    );
    let resource = some_file();

    let _ = test_object.update_value(Some(&resource), "a", Dynamic::from("b"), Some(ConfigurationTarget::User));
    assert_eq!(suite.update_args(), expected("b", &resource, None, ConfigurationTarget::User));
}

#[test]
fn update_value_writes_into_given_workspace_folder_target_with_overrides() {
    let suite = Suite::new();
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_object = suite.setup(&ds);
    suite.set(
        "a",
        TestConfigurationValue {
            default: Some(layer("1")),
            user_local: Some(layer("2")),
            workspace_folder: Some(layer_with_override("2", "1")),
            override_identifiers: Some(vec!["a"]),
            ..Default::default()
        },
    );
    let resource = some_file();

    let _ = test_object.update_value(Some(&resource), "a", Dynamic::from("b"), Some(ConfigurationTarget::WorkspaceFolder));
    assert_eq!(suite.update_args(), expected("b", &resource, Some("a"), ConfigurationTarget::WorkspaceFolder));
}

#[test]
fn update_value_writes_into_derived_workspace_folder_target_without_overrides() {
    let suite = Suite::new();
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_object = suite.setup(&ds);
    suite.set(
        "a",
        TestConfigurationValue {
            default: Some(layer("1")),
            user_local: Some(layer("2")),
            workspace_folder: Some(layer("2")),
            ..Default::default()
        },
    );
    let resource = some_file();

    let _ = test_object.update_value(Some(&resource), "a", Dynamic::from("b"), None);
    assert_eq!(suite.update_args(), expected("b", &resource, None, ConfigurationTarget::WorkspaceFolder));
}

#[test]
fn update_value_writes_into_derived_workspace_folder_target_with_overrides() {
    let suite = Suite::new();
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_object = suite.setup(&ds);
    suite.set(
        "a",
        TestConfigurationValue {
            default: Some(layer("1")),
            user_local: Some(layer("2")),
            workspace: Some(layer_with_override("2", "1")),
            workspace_folder: Some(layer_with_override("2", "2")),
            override_identifiers: Some(vec!["a"]),
            ..Default::default()
        },
    );
    let resource = some_file();

    let _ = test_object.update_value(Some(&resource), "a", Dynamic::from("b"), None);
    assert_eq!(suite.update_args(), expected("b", &resource, Some("a"), ConfigurationTarget::WorkspaceFolder));
}

#[test]
fn update_value_writes_into_derived_workspace_target_without_overrides() {
    let suite = Suite::new();
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_object = suite.setup(&ds);
    suite.set(
        "a",
        TestConfigurationValue {
            default: Some(layer("1")),
            user_local: Some(layer("2")),
            workspace: Some(layer("2")),
            ..Default::default()
        },
    );
    let resource = some_file();

    let _ = test_object.update_value(Some(&resource), "a", Dynamic::from("b"), None);
    assert_eq!(suite.update_args(), expected("b", &resource, None, ConfigurationTarget::Workspace));
}

#[test]
fn update_value_writes_into_derived_workspace_target_with_overrides() {
    let suite = Suite::new();
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_object = suite.setup(&ds);
    suite.set(
        "a",
        TestConfigurationValue {
            default: Some(layer("1")),
            user_local: Some(layer("2")),
            workspace: Some(layer_with_override("2", "2")),
            override_identifiers: Some(vec!["a"]),
            ..Default::default()
        },
    );
    let resource = some_file();

    let _ = test_object.update_value(Some(&resource), "a", Dynamic::from("b"), None);
    assert_eq!(suite.update_args(), expected("b", &resource, Some("a"), ConfigurationTarget::Workspace));
}

#[test]
fn update_value_writes_into_derived_workspace_target_with_overrides_and_value_defined_in_folder() {
    let suite = Suite::new();
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_object = suite.setup(&ds);
    suite.set(
        "a",
        TestConfigurationValue {
            default: Some(layer_with_override("1", "3")),
            user_local: Some(layer("2")),
            workspace: Some(layer_with_override("2", "2")),
            workspace_folder: Some(layer("2")),
            override_identifiers: Some(vec!["a"]),
            ..Default::default()
        },
    );
    let resource = some_file();

    let _ = test_object.update_value(Some(&resource), "a", Dynamic::from("b"), None);
    assert_eq!(suite.update_args(), expected("b", &resource, Some("a"), ConfigurationTarget::Workspace));
}

#[test]
fn update_value_writes_into_derived_user_remote_target_without_overrides() {
    let suite = Suite::new();
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_object = suite.setup(&ds);
    suite.set(
        "a",
        TestConfigurationValue {
            default: Some(layer("1")),
            user_local: Some(layer("2")),
            user_remote: Some(layer("2")),
            ..Default::default()
        },
    );
    let resource = some_file();

    let _ = test_object.update_value(Some(&resource), "a", Dynamic::from("b"), None);
    assert_eq!(suite.update_args(), expected("b", &resource, None, ConfigurationTarget::UserRemote));
}

#[test]
fn update_value_writes_into_derived_user_remote_target_with_overrides() {
    let suite = Suite::new();
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_object = suite.setup(&ds);
    suite.set(
        "a",
        TestConfigurationValue {
            default: Some(layer("1")),
            user_local: Some(layer("2")),
            user_remote: Some(layer_with_override("2", "3")),
            override_identifiers: Some(vec!["a"]),
            ..Default::default()
        },
    );
    let resource = some_file();

    let _ = test_object.update_value(Some(&resource), "a", Dynamic::from("b"), None);
    assert_eq!(suite.update_args(), expected("b", &resource, Some("a"), ConfigurationTarget::UserRemote));
}

#[test]
fn update_value_writes_into_derived_user_remote_target_with_overrides_and_value_defined_in_workspace() {
    let suite = Suite::new();
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_object = suite.setup(&ds);
    suite.set(
        "a",
        TestConfigurationValue {
            default: Some(layer("1")),
            user_local: Some(layer("2")),
            user_remote: Some(layer_with_override("2", "3")),
            workspace: Some(layer("3")),
            override_identifiers: Some(vec!["a"]),
            ..Default::default()
        },
    );
    let resource = some_file();

    let _ = test_object.update_value(Some(&resource), "a", Dynamic::from("b"), None);
    assert_eq!(suite.update_args(), expected("b", &resource, Some("a"), ConfigurationTarget::UserRemote));
}

#[test]
fn update_value_writes_into_derived_user_remote_target_with_overrides_and_value_defined_in_workspace_folder() {
    let suite = Suite::new();
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_object = suite.setup(&ds);
    suite.set(
        "a",
        TestConfigurationValue {
            default: Some(layer("1")),
            user_local: Some(layer_with_override("2", "1")),
            user_remote: Some(layer_with_override("2", "3")),
            workspace: Some(layer("3")),
            workspace_folder: Some(layer("3")),
            override_identifiers: Some(vec!["a"]),
        },
    );
    let resource = some_file();

    let _ = test_object.update_value(Some(&resource), "a", Dynamic::from("b"), None);
    assert_eq!(suite.update_args(), expected("b", &resource, Some("a"), ConfigurationTarget::UserRemote));
}

#[test]
fn update_value_writes_into_derived_user_target_without_overrides() {
    let suite = Suite::new();
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_object = suite.setup(&ds);
    suite.set("a", TestConfigurationValue { default: Some(layer("1")), user_local: Some(layer("2")), ..Default::default() });
    let resource = some_file();

    let _ = test_object.update_value(Some(&resource), "a", Dynamic::from("b"), None);
    assert_eq!(suite.update_args(), expected("b", &resource, None, ConfigurationTarget::UserLocal));
}

#[test]
fn update_value_writes_into_derived_user_target_with_overrides() {
    let suite = Suite::new();
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_object = suite.setup(&ds);
    suite.set(
        "a",
        TestConfigurationValue {
            default: Some(layer("1")),
            user_local: Some(layer_with_override("2", "3")),
            override_identifiers: Some(vec!["a"]),
            ..Default::default()
        },
    );
    let resource = some_file();

    let _ = test_object.update_value(Some(&resource), "a", Dynamic::from("2"), None);
    assert_eq!(suite.update_args(), expected("2", &resource, Some("a"), ConfigurationTarget::UserLocal));
}

#[test]
fn update_value_writes_into_derived_user_target_with_overrides_and_value_is_defined_in_remote() {
    let suite = Suite::new();
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_object = suite.setup(&ds);
    suite.set(
        "a",
        TestConfigurationValue {
            default: Some(layer("1")),
            user_local: Some(layer_with_override("2", "3")),
            user_remote: Some(layer("3")),
            override_identifiers: Some(vec!["a"]),
            ..Default::default()
        },
    );
    let resource = some_file();

    let _ = test_object.update_value(Some(&resource), "a", Dynamic::from("2"), None);
    assert_eq!(suite.update_args(), expected("2", &resource, Some("a"), ConfigurationTarget::UserLocal));
}

#[test]
fn update_value_writes_into_derived_user_target_with_overrides_and_value_is_defined_in_workspace() {
    let suite = Suite::new();
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_object = suite.setup(&ds);
    // `workspaceValue: { value: '3' }` do original não é um campo de `IConfigurationValue`: a camada fica ausente.
    suite.set(
        "a",
        TestConfigurationValue {
            default: Some(layer("1")),
            user_local: Some(layer_with_override("2", "3")),
            override_identifiers: Some(vec!["a"]),
            ..Default::default()
        },
    );
    let resource = some_file();

    let _ = test_object.update_value(Some(&resource), "a", Dynamic::from("2"), None);
    assert_eq!(suite.update_args(), expected("2", &resource, Some("a"), ConfigurationTarget::UserLocal));
}

#[test]
fn update_value_writes_into_derived_user_target_with_overrides_and_value_is_defined_in_workspace_folder() {
    let suite = Suite::new();
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_object = suite.setup(&ds);
    // `workspaceFolderValue: { value: '3' }` do original também não é campo de `IConfigurationValue`.
    suite.set(
        "a",
        TestConfigurationValue {
            default: Some(layer_with_override("1", "3")),
            user_local: Some(layer_with_override("2", "3")),
            user_remote: Some(layer("3")),
            override_identifiers: Some(vec!["a"]),
            ..Default::default()
        },
    );
    let resource = some_file();

    let _ = test_object.update_value(Some(&resource), "a", Dynamic::from("2"), None);
    assert_eq!(suite.update_args(), expected("2", &resource, Some("a"), ConfigurationTarget::UserLocal));
}

#[test]
fn update_value_writes_into_derived_user_target_when_overridden_in_default_and_not_in_user() {
    let suite = Suite::new();
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_object = suite.setup(&ds);
    suite.set(
        "a",
        TestConfigurationValue {
            default: Some(layer_with_override("1", "3")),
            user_local: Some(layer("2")),
            override_identifiers: Some(vec!["a"]),
            ..Default::default()
        },
    );
    let resource = some_file();

    let _ = test_object.update_value(Some(&resource), "a", Dynamic::from("2"), None);
    assert_eq!(suite.update_args(), expected("2", &resource, Some("a"), ConfigurationTarget::UserLocal));
}

#[test]
fn update_value_when_not_changed() {
    let suite = Suite::new();
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let test_object = suite.setup(&ds);
    suite.set("a", TestConfigurationValue { default: Some(layer("1")), ..Default::default() });
    let resource = some_file();

    let _ = test_object.update_value(Some(&resource), "a", Dynamic::from("b"), None);
    assert_eq!(suite.update_args(), expected("b", &resource, None, ConfigurationTarget::UserLocal));
}
