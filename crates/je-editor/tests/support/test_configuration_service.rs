//! Porte de `src/vs/platform/configuration/test/common/testConfigurationService.ts`: o `TestConfigurationService`.
//!
//! Divergências:
//! - **`getValue`**: as sobrecargas `getValue(section?, overrides?)` e `getValue(overrides)` são
//!   `get_value(section, overrides)`, a forma do trait `IConfigurationService` do `je-platform`.
//! - **`inspect`** devolve o [`ConfigurationInspectValue`] do `je-platform`, montado com as camadas que o literal
//!   do original preenche: `value` (consolidado), `userValue` e `userLocalValue` com o valor, `defaultValue`
//!   `undefined` (modelo vazio) e `overrideIdentifiers`. As chaves com ponto viram a árvore de valores, como
//!   `getConfigurationValue` espera.
//! - **`override` de método**: as subclasses anônimas dos testes que sobrescrevem `inspect` e `updateValue` viram os
//!   ganchos [`TestConfigurationService::set_inspect_override`] e
//!   [`TestConfigurationService::set_update_value_override`]. Sem gancho, o comportamento é o da classe base.
//! - **`updateValue(key, value)`** recebe também os `overrides` e o alvo do trait, e a base os ignora.
//! - **`reloadConfiguration<T>()`** devolve `Promise<()>` (o trait não carrega o valor).
//! - **`Record<string, unknown>`** é um [`Dynamic`] objeto (compartilhado, como a referência do original).
//! - O `TernarySearchTree.forPaths()` usa a caixa do caminho (`ignorePathCasing` falso, o padrão).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::path::{Environment, ProcessEnvironment};
use je_base::promise::Promise;
use je_base::ternary_search_tree::TernarySearchTree;
use je_base::uri::URI;
use je_platform::configuration::models::{ConfigurationChangeEvent, ConfigurationInspectValue, ConfigurationKeys, ConfigurationModel};
use je_platform::configuration::registry::configuration_registry;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::{
    ConfigurationData, ConfigurationOverrides, ConfigurationTarget, ConfigurationUpdateOverrides, get_configuration_value, to_values_tree,
};
use je_platform::js::set_property;
use je_platform::log::NullLogService;

/// Os argumentos de uma chamada a `updateValue` (o `[...arguments]` que os testes guardam).
#[derive(Clone, Debug, PartialEq)]
pub struct UpdateValueCall {
    pub key: String,
    pub value: Dynamic,
    pub overrides: Option<ConfigurationUpdateOverrides>,
    pub target: Option<ConfigurationTarget>,
}

type InspectOverride = Box<dyn Fn(&str, &ConfigurationOverrides) -> ConfigurationInspectValue>;
type UpdateValueOverride = Box<dyn Fn(UpdateValueCall) -> Promise<()>>;

/// `TestConfigurationService`.
pub struct TestConfigurationService {
    configuration: Dynamic,
    configuration_by_root: RefCell<TernarySearchTree<String, Dynamic>>,
    override_identifiers: RefCell<HashMap<String, Vec<String>>>,
    pub on_did_change_configuration_emitter: Rc<Emitter<Rc<ConfigurationChangeEvent>>>,
    inspect_override: RefCell<Option<InspectOverride>>,
    update_value_override: RefCell<Option<UpdateValueOverride>>,
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
    pub fn new(configuration: Option<Dynamic>) -> Rc<Self> {
        Rc::new(Self {
            configuration: configuration.unwrap_or_else(empty_record),
            configuration_by_root: RefCell::new(TernarySearchTree::for_paths(false)),
            override_identifiers: RefCell::new(HashMap::new()),
            on_did_change_configuration_emitter: Emitter::new(),
            inspect_override: RefCell::new(None),
            update_value_override: RefCell::new(None),
        })
    }

    /// Faz o `inspect` devolver o que o gancho montar (o `override inspect()` da subclasse do original).
    pub fn set_inspect_override(&self, hook: impl Fn(&str, &ConfigurationOverrides) -> ConfigurationInspectValue + 'static) {
        *self.inspect_override.borrow_mut() = Some(Box::new(hook));
    }

    /// Faz o `updateValue` chamar o gancho (o `override updateValue()` da subclasse do original).
    pub fn set_update_value_override(&self, hook: impl Fn(UpdateValueCall) -> Promise<()> + 'static) {
        *self.update_value_override.borrow_mut() = Some(Box::new(hook));
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
    pub fn set_override_identifiers(&self, key: &str, identifiers: Vec<String>) {
        self.override_identifiers.borrow_mut().insert(key.to_owned(), identifiers);
    }

    /// O modelo de uma camada que só tem o valor da chave (`{ [key]: value }`, com a árvore dos pontos).
    fn layer_with_value(key: &str, value: &Dynamic) -> ConfigurationModel {
        let contents = to_values_tree(&Dynamic::object([(key.to_owned(), value.clone())]), &|_| {});
        ConfigurationModel::new(contents, vec![JsString::from(key)], Vec::new(), None, NullLogService::shared())
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
        let configuration_for_root = overrides
            .and_then(|overrides| overrides.resource.as_ref())
            .and_then(|resource| self.configuration_by_root.borrow().find_substr(&fs_path_key(resource)).cloned());
        let configuration = configuration_for_root.unwrap_or_else(|| self.configuration.clone());
        match section.filter(|section| !section.is_empty()) {
            Some(section) => {
                let direct = configuration.get(&JsString::from(section));
                if direct.is_nullish() { get_configuration_value(&configuration, section, None) } else { direct }
            }
            None => configuration,
        }
    }

    fn update_value(&self, key: &str, value: Dynamic, overrides: Option<ConfigurationUpdateOverrides>, target: Option<ConfigurationTarget>) -> Promise<()> {
        match &*self.update_value_override.borrow() {
            Some(hook) => hook(UpdateValueCall { key: key.to_owned(), value, overrides, target }),
            None => Promise::resolved(()),
        }
    }

    fn inspect(&self, key: &str, overrides: &ConfigurationOverrides) -> ConfigurationInspectValue {
        if let Some(hook) = &*self.inspect_override.borrow() {
            return hook(key, overrides);
        }
        let value = IConfigurationService::get_value(self, Some(key), Some(overrides));
        let user = Self::layer_with_value(key, &value);
        ConfigurationInspectValue {
            key: JsString::from(key),
            overrides: overrides.clone(),
            value,
            override_identifiers: self
                .override_identifiers
                .borrow()
                .get(key)
                .map(|identifiers| identifiers.iter().map(|identifier| JsString::from(identifier.as_str())).collect()),
            default_configuration: ConfigurationModel::create_empty_model(NullLogService::shared()),
            policy_configuration: None,
            application_configuration: None,
            user_configuration: user.clone(),
            local_user_configuration: user,
            remote_user_configuration: ConfigurationModel::create_empty_model(NullLogService::shared()),
            workspace_configuration: None,
            folder_configuration_model: None,
            memory_configuration_model: ConfigurationModel::create_empty_model(NullLogService::shared()),
        }
    }

    fn reload_configuration(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn keys(&self) -> ConfigurationKeys {
        ConfigurationKeys {
            default: configuration_registry().get_configuration_properties().keys(),
            policy: Vec::new(),
            user: self.configuration.own_keys(),
            workspace: Vec::new(),
            workspace_folder: Vec::new(),
        }
    }
}
