//! Porte de `src/vs/platform/configuration/test/common/testConfigurationService.ts`: o `TestConfigurationService`,
//! um serviço de configuração de teste que guarda os valores na memória.
//!
//! Decisões de tradução:
//! - **`IConfigurationService`**: o porte devolve do `inspect` o `ConfigurationInspectValue` concreto, que só o
//!   próprio modelo de configuração constrói. O serviço de teste implementa por isso o recorte que os módulos do
//!   workbench leem ([`GitHubEnterpriseConfigurationService`]), com o `inspect` devolvendo o contrato
//!   [`ConfigurationValue`] ([`TestConfigurationValue`] é o objeto `{ value, defaultValue, userValue, ... }`).
//! - **`sinon.stub(service, 'inspect' | 'updateValue')`**: o JavaScript troca o método da instância; aqui o
//!   serviço guarda o substituto ([`TestConfigurationService::stub_inspect`] e
//!   [`TestConfigurationService::stub_update_value`]) e as chamadas de `updateValue` ficam registradas
//!   ([`TestConfigurationService::update_value_calls`]), o que `stub.getCalls().map(call => call.args)` lê.
//! - **`configuration?: Record<string, unknown>`** é um [`Dynamic`] objeto; `Object.create(null)` é o objeto vazio.
//! - **`getValue(arg1?, arg2?)`**: as duas posições são a seção e os `overrides`
//!   ([`TestConfigurationService::get_value`]).
//! - **`keys()` e `getConfigurationData()`** dependem do registro de configuração e dos modelos, que nenhum
//!   teste do workbench exerce; `reloadConfiguration` devolve `getValue()` (a configuração inteira).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::event::Emitter;
use je_base::path::{Environment, ProcessEnvironment};
use je_base::promise::Promise;
use je_base::ternary_search_tree::TernarySearchTree;
use je_base::uri::URI;
use je_platform::configuration::models::ConfigurationChangeEvent;
use je_platform::configuration::{ConfigurationOverrides, ConfigurationTarget, ConfigurationValue, get_configuration_value};
use je_platform::js::set_property;
use je_workbench::services::accounts::github_enterprise::GitHubEnterpriseConfigurationService;

/// O objeto que `inspect` devolve (`IConfigurationValue`): o valor de uma chave em cada camada, `undefined` onde a
/// camada não define.
#[derive(Clone)]
pub struct TestConfigurationValue {
    pub default_value: Dynamic,
    pub application_value: Dynamic,
    pub user_value: Dynamic,
    pub user_local_value: Dynamic,
    pub user_remote_value: Dynamic,
    pub workspace_value: Dynamic,
    pub workspace_folder_value: Dynamic,
    pub memory_value: Dynamic,
    pub policy_value: Dynamic,
    pub value: Dynamic,
    pub override_identifiers: Option<Vec<JsString>>,
}

impl Default for TestConfigurationValue {
    fn default() -> Self {
        Self {
            default_value: Dynamic::Undefined,
            application_value: Dynamic::Undefined,
            user_value: Dynamic::Undefined,
            user_local_value: Dynamic::Undefined,
            user_remote_value: Dynamic::Undefined,
            workspace_value: Dynamic::Undefined,
            workspace_folder_value: Dynamic::Undefined,
            memory_value: Dynamic::Undefined,
            policy_value: Dynamic::Undefined,
            value: Dynamic::Undefined,
            override_identifiers: None,
        }
    }
}

impl ConfigurationValue for TestConfigurationValue {
    fn default_value(&self) -> Dynamic {
        self.default_value.clone()
    }

    fn application_value(&self) -> Dynamic {
        self.application_value.clone()
    }

    fn user_value(&self) -> Dynamic {
        self.user_value.clone()
    }

    fn user_local_value(&self) -> Dynamic {
        self.user_local_value.clone()
    }

    fn user_remote_value(&self) -> Dynamic {
        self.user_remote_value.clone()
    }

    fn workspace_value(&self) -> Dynamic {
        self.workspace_value.clone()
    }

    fn workspace_folder_value(&self) -> Dynamic {
        self.workspace_folder_value.clone()
    }

    fn memory_value(&self) -> Dynamic {
        self.memory_value.clone()
    }

    fn policy_value(&self) -> Dynamic {
        self.policy_value.clone()
    }

    fn value(&self) -> Dynamic {
        self.value.clone()
    }

    fn override_identifiers(&self) -> Option<Vec<JsString>> {
        self.override_identifiers.clone()
    }
}

/// O que `updateValue` faz: o corpo da classe, ou o substituto que um teste instala com `sinon.stub`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateValueBehavior {
    /// O corpo da classe: `Promise.resolve(undefined)`.
    Original,
    /// `stub.resolves()`.
    Resolves,
    /// `stub.callsFake((key, value) => service.setUserConfiguration(key, value))`.
    SetsUserConfiguration,
}

/// Uma chamada de `updateValue` (`call.args`).
#[derive(Clone)]
pub struct UpdateValueCall {
    pub key: String,
    pub value: Dynamic,
    pub target: ConfigurationTarget,
}

pub struct TestConfigurationService {
    configuration: Dynamic,
    pub on_did_change_configuration_emitter: Rc<Emitter<Rc<ConfigurationChangeEvent>>>,
    configuration_by_root: RefCell<TernarySearchTree<String, Dynamic>>,
    override_identifiers: RefCell<HashMap<String, Vec<JsString>>>,
    inspect_stub: RefCell<Option<Rc<dyn ConfigurationValue>>>,
    update_value_behavior: RefCell<UpdateValueBehavior>,
    update_value_calls: RefCell<Vec<UpdateValueCall>>,
}

fn empty_object() -> Dynamic {
    Dynamic::object(Vec::<(String, Dynamic)>::new())
}

impl TestConfigurationService {
    /// `new TestConfigurationService(configuration?)`.
    pub fn new(configuration: Option<Dynamic>) -> Rc<Self> {
        Rc::new(Self {
            configuration: configuration.unwrap_or_else(empty_object),
            on_did_change_configuration_emitter: Emitter::new(),
            configuration_by_root: RefCell::new(TernarySearchTree::for_paths(false)),
            override_identifiers: RefCell::new(HashMap::new()),
            inspect_stub: RefCell::new(None),
            update_value_behavior: RefCell::new(UpdateValueBehavior::Original),
            update_value_calls: RefCell::new(Vec::new()),
        })
    }

    /// `reloadConfiguration()`.
    pub fn reload_configuration(&self) -> Promise<Dynamic> {
        Promise::resolved(self.get_value(None, None))
    }

    /// `getValue(section?, overrides?)`.
    pub fn get_value(&self, section: Option<&str>, overrides: Option<&ConfigurationOverrides>) -> Dynamic {
        let configuration_for_root = overrides
            .and_then(|overrides| overrides.resource.as_ref())
            .and_then(|resource| self.configuration_by_root.borrow().find_substr(&resource.fs_path(ProcessEnvironment.platform()).to_string_lossy()).cloned());
        let configuration = configuration_for_root.unwrap_or_else(|| self.configuration.clone());
        match section.filter(|section| !section.is_empty()) {
            Some(section) => {
                let direct = configuration.get_str(section);
                if direct.is_nullish() { get_configuration_value(&configuration, section, None) } else { direct }
            }
            None => configuration,
        }
    }

    /// `updateValue(key, value)`, com o comportamento instalado por [`Self::stub_update_value`].
    fn update_value(&self, key: &str, value: Dynamic, target: ConfigurationTarget) -> Promise<()> {
        self.update_value_calls.borrow_mut().push(UpdateValueCall { key: key.to_owned(), value: value.clone(), target });
        let behavior = *self.update_value_behavior.borrow();
        match behavior {
            UpdateValueBehavior::Original | UpdateValueBehavior::Resolves => Promise::resolved(()),
            UpdateValueBehavior::SetsUserConfiguration => self.set_user_configuration(key, value, None),
        }
    }

    /// `setUserConfiguration(key, value, root?)`.
    pub fn set_user_configuration(&self, key: &str, value: Dynamic, root: Option<&URI>) -> Promise<()> {
        match root {
            Some(root) => {
                let root_path = root.fs_path(ProcessEnvironment.platform()).to_string_lossy();
                let existing = self.configuration_by_root.borrow().get(&root_path).cloned();
                let configuration_for_root = existing.unwrap_or_else(empty_object);
                set_property(&configuration_for_root, &JsString::from(key), value);
                self.configuration_by_root.borrow_mut().set(root_path, configuration_for_root);
            }
            None => set_property(&self.configuration, &JsString::from(key), value),
        }
        Promise::resolved(())
    }

    /// `setOverrideIdentifiers(key, identifiers)`.
    pub fn set_override_identifiers(&self, key: &str, identifiers: Vec<JsString>) {
        self.override_identifiers.borrow_mut().insert(key.to_owned(), identifiers);
    }

    /// `inspect(key, overrides?)`.
    pub fn inspect_value(&self, key: &str, overrides: Option<&ConfigurationOverrides>) -> TestConfigurationValue {
        let value = self.get_value(Some(key), overrides);
        TestConfigurationValue {
            default_value: Dynamic::Undefined,
            user_value: value.clone(),
            user_local_value: value.clone(),
            value,
            override_identifiers: self.override_identifiers.borrow().get(key).cloned(),
            ..Default::default()
        }
    }

    /// `sinon.stub(service, 'inspect').returns(value)`.
    pub fn stub_inspect(&self, value: TestConfigurationValue) {
        *self.inspect_stub.borrow_mut() = Some(Rc::new(value));
    }

    /// `sinon.stub(service, 'updateValue')` com o comportamento dado.
    pub fn stub_update_value(&self, behavior: UpdateValueBehavior) {
        *self.update_value_behavior.borrow_mut() = behavior;
    }

    /// `stub.getCalls().map(call => call.args)`, de `updateValue`.
    pub fn update_value_calls(&self) -> Vec<UpdateValueCall> {
        self.update_value_calls.borrow().clone()
    }
}

impl GitHubEnterpriseConfigurationService for TestConfigurationService {
    fn inspect(&self, key: &str) -> Rc<dyn ConfigurationValue> {
        match &*self.inspect_stub.borrow() {
            Some(stub) => stub.clone(),
            None => Rc::new(self.inspect_value(key, None)),
        }
    }

    fn get_value(&self, section: &str) -> Dynamic {
        TestConfigurationService::get_value(self, Some(section), None)
    }

    fn update_value(&self, key: &str, value: Dynamic, target: ConfigurationTarget) -> Promise<()> {
        TestConfigurationService::update_value(self, key, value, target)
    }
}
