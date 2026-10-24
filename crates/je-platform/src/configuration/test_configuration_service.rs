//! Porte de `src/vs/platform/configuration/test/common/testConfigurationService.ts`: o `TestConfigurationService`,
//! o `IConfigurationService` dos testes de todas as camadas, que guarda os valores num objeto simples (sem camadas,
//! sem arquivo). É um só, como no original, e público para os testes de `je-editor`, `je-workbench` e `je-platform`.
//!
//! Decisões de tradução:
//! - **`getValue(arg1?, arg2?)`** (`arg1` seção ou `overrides`) é a forma do trait [`IConfigurationService`]: seção
//!   e `overrides` opcionais.
//! - **`inspect`** devolve o [`ConfigurationValue`] (o `IConfigurationValue` do original), aqui o
//!   [`TestConfigurationValue`]: o literal `{ value, defaultValue: undefined, userValue, userLocalValue,
//!   overrideIdentifiers }`, com `undefined` onde o original não preenche.
//! - **`updateValue(key, value)`** do original só resolve. As chamadas ficam registradas
//!   ([`TestConfigurationService::update_value_calls`], o `stub.getCalls().map(call => call.args)` do sinon).
//! - **`sinon.stub(service, 'inspect' | 'updateValue')`** e as subclasses anônimas que sobrescrevem `inspect`: o
//!   JavaScript troca o método da instância; aqui o serviço guarda o substituto
//!   ([`TestConfigurationService::stub_inspect`], [`TestConfigurationService::stub_inspect_with`] e
//!   [`TestConfigurationService::stub_update_value`]). Sem substituto, o comportamento é o da classe.
//! - **`configuration?: Record<string, unknown>`** é um [`Dynamic`] objeto (compartilhado, como a referência do
//!   original); `Object.create(null)` é o objeto vazio.
//! - **`reloadConfiguration<T>(): Promise<T>`** é o `Promise<()>` do trait.
//! - O `TernarySearchTree.forPaths()` usa a caixa do caminho (`ignorePathCasing` falso, o padrão).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::path::{Environment, ProcessEnvironment};
use je_base::promise::Promise;
use je_base::ternary_search_tree::TernarySearchTree;
use je_base::uri::URI;

use super::models::{ConfigurationChangeEvent, ConfigurationKeys};
use super::registry::configuration_registry;
use super::service::IConfigurationService;
use super::{ConfigurationData, ConfigurationOverrides, ConfigurationTarget, ConfigurationUpdateOverrides, ConfigurationValue, get_configuration_value};
use crate::js::{object_keys, set_property};

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

/// O que `updateValue` faz depois de registrar a chamada: o corpo da classe (`Promise.resolve(undefined)`, que é o
/// `stub.resolves()`) ou o `stub.callsFake((key, value) => service.setUserConfiguration(key, value))`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateValueBehavior {
    Resolves,
    SetsUserConfiguration,
}

/// Os argumentos de uma chamada a `updateValue` (o `[...arguments]` que os testes guardam).
#[derive(Clone, Debug, PartialEq)]
pub struct UpdateValueCall {
    pub key: String,
    pub value: Dynamic,
    pub overrides: Option<ConfigurationUpdateOverrides>,
    pub target: Option<ConfigurationTarget>,
}

type InspectStub = Box<dyn Fn(&str, &ConfigurationOverrides) -> Rc<dyn ConfigurationValue>>;

/// O substituto de `getValue` (a subclasse que o sobrescreve): recebe a seção e os `overrides` que o chamador deu.
type GetValueStub = Box<dyn Fn(Option<&str>, Option<&ConfigurationOverrides>) -> Dynamic>;

/// `TestConfigurationService`.
pub struct TestConfigurationService {
    configuration: Dynamic,
    pub on_did_change_configuration_emitter: Rc<Emitter<Rc<ConfigurationChangeEvent>>>,
    configuration_by_root: RefCell<TernarySearchTree<String, Dynamic>>,
    override_identifiers: RefCell<HashMap<String, Vec<JsString>>>,
    inspect_stub: RefCell<Option<InspectStub>>,
    get_value_stub: RefCell<Option<GetValueStub>>,
    update_value_behavior: Cell<UpdateValueBehavior>,
    update_value_calls: RefCell<Vec<UpdateValueCall>>,
}

/// `root.fsPath`: a chave da árvore de raízes.
fn fs_path_key(resource: &URI) -> String {
    resource.fs_path(ProcessEnvironment.platform()).to_string_lossy()
}

/// Um `Object.create(null)`.
fn empty_record() -> Dynamic {
    Dynamic::object(Vec::<(&str, Dynamic)>::new())
}

impl TestConfigurationService {
    /// `new TestConfigurationService(configuration?)`: sem argumento, o objeto vazio.
    pub fn new(configuration: Option<Dynamic>) -> Rc<Self> {
        Rc::new(Self {
            configuration: configuration.unwrap_or_else(empty_record),
            on_did_change_configuration_emitter: Emitter::new(),
            configuration_by_root: RefCell::new(TernarySearchTree::for_paths(false)),
            override_identifiers: RefCell::new(HashMap::new()),
            inspect_stub: RefCell::new(None),
            get_value_stub: RefCell::new(None),
            update_value_behavior: Cell::new(UpdateValueBehavior::Resolves),
            update_value_calls: RefCell::new(Vec::new()),
        })
    }

    /// `setUserConfiguration(key, value, root?)`.
    pub fn set_user_configuration(&self, key: &str, value: Dynamic, root: Option<&URI>) -> Promise<()> {
        let key = JsString::from(key);
        match root {
            Some(root) => {
                let root_key = fs_path_key(root);
                let config_for_root = self.configuration_by_root.borrow().get(&root_key).cloned().unwrap_or_else(empty_record);
                set_property(&config_for_root, &key, value);
                self.configuration_by_root.borrow_mut().set(root_key, config_for_root);
            }
            None => set_property(&self.configuration, &key, value),
        }
        Promise::resolved(())
    }

    /// `setOverrideIdentifiers(key, identifiers)`.
    pub fn set_override_identifiers(&self, key: &str, identifiers: Vec<JsString>) {
        self.override_identifiers.borrow_mut().insert(key.to_owned(), identifiers);
    }

    /// `sinon.stub(service, 'inspect').returns(value)`.
    pub fn stub_inspect(&self, value: TestConfigurationValue) {
        let value: Rc<dyn ConfigurationValue> = Rc::new(value);
        self.stub_inspect_with(move |_, _| value.clone());
    }

    /// `override inspect()` de uma subclasse (ou `callsFake`): o `inspect` devolve o que o gancho montar.
    pub fn stub_inspect_with(&self, hook: impl Fn(&str, &ConfigurationOverrides) -> Rc<dyn ConfigurationValue> + 'static) {
        *self.inspect_stub.borrow_mut() = Some(Box::new(hook));
    }

    /// `override getValue()` de uma subclasse (o `MockInputsConfigurationService` do resolvedor de variáveis): o `getValue`
    /// devolve o que o gancho montar, no lugar do comportamento da classe.
    pub fn stub_get_value_with(&self, hook: impl Fn(Option<&str>, Option<&ConfigurationOverrides>) -> Dynamic + 'static) {
        *self.get_value_stub.borrow_mut() = Some(Box::new(hook));
    }

    /// `sinon.stub(service, 'updateValue')` com o comportamento dado.
    pub fn stub_update_value(&self, behavior: UpdateValueBehavior) {
        self.update_value_behavior.set(behavior);
    }

    /// `stub.getCalls().map(call => call.args)`, de `updateValue`.
    pub fn update_value_calls(&self) -> Vec<UpdateValueCall> {
        self.update_value_calls.borrow().clone()
    }
}

impl IConfigurationService for TestConfigurationService {
    fn on_did_change_configuration(&self) -> Event<Rc<ConfigurationChangeEvent>> {
        self.on_did_change_configuration_emitter.event()
    }

    fn get_configuration_data(&self) -> Option<ConfigurationData> {
        None
    }

    fn get_value(&self, section: Option<&str>, overrides: Option<&ConfigurationOverrides>) -> Dynamic {
        if let Some(stub) = &*self.get_value_stub.borrow() {
            return stub(section, overrides);
        }
        let configuration_for_root = overrides
            .and_then(|overrides| overrides.resource.as_ref())
            .and_then(|resource| self.configuration_by_root.borrow().find_substr(&fs_path_key(resource)).cloned());
        let configuration = configuration_for_root.unwrap_or_else(|| self.configuration.clone());
        match section.filter(|section| !section.is_empty()) {
            Some(section) => {
                // `configuration[arg1] ?? getConfigurationValue(configuration, arg1)`
                let direct = configuration.get_str(section);
                if direct.is_nullish() { get_configuration_value(&configuration, section, None) } else { direct }
            }
            None => configuration,
        }
    }

    fn update_value(&self, key: &str, value: Dynamic, overrides: Option<ConfigurationUpdateOverrides>, target: Option<ConfigurationTarget>) -> Promise<()> {
        self.update_value_calls.borrow_mut().push(UpdateValueCall { key: key.to_owned(), value: value.clone(), overrides, target });
        match self.update_value_behavior.get() {
            UpdateValueBehavior::Resolves => Promise::resolved(()),
            UpdateValueBehavior::SetsUserConfiguration => self.set_user_configuration(key, value, None),
        }
    }

    fn inspect(&self, key: &str, overrides: &ConfigurationOverrides) -> Rc<dyn ConfigurationValue> {
        if let Some(stub) = &*self.inspect_stub.borrow() {
            return stub(key, overrides);
        }
        let value = self.get_value(Some(key), Some(overrides));
        Rc::new(TestConfigurationValue {
            user_value: value.clone(),
            user_local_value: value.clone(),
            value,
            override_identifiers: self.override_identifiers.borrow().get(key).cloned(),
            ..Default::default()
        })
    }

    fn reload_configuration(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn keys(&self) -> ConfigurationKeys {
        ConfigurationKeys {
            default: configuration_registry().get_configuration_properties().keys(),
            policy: Vec::new(),
            user: object_keys(&self.configuration),
            workspace: Vec::new(),
            workspace_folder: Vec::new(),
        }
    }
}
