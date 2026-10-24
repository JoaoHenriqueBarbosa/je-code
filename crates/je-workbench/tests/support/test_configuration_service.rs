//! Porte de `src/vs/platform/configuration/test/common/testConfigurationService.ts`: o `IConfigurationService` dos
//! testes, que guarda os valores num objeto simples (sem camadas, sem arquivo).
//!
//! Notas de tradução:
//! - As sobrecargas de `getValue(arg1?: string | IConfigurationOverrides, arg2?: IConfigurationOverrides)` são a
//!   forma do trait: seção e `overrides` opcionais.
//! - `inspect` devolve o `ConfigurationInspectValue` do porte, que só se monta a partir de uma
//!   [`Configuration`] de camadas: o valor vai na camada do usuário local (o `value`, o `userValue` e o
//!   `userLocalValue` do original são o mesmo valor, e o resto fica `undefined`). Os `overrideIdentifiers`
//!   registrados com [`TestConfigurationService::set_override_identifiers`] viram um bloco de sobreposição do
//!   modelo, de onde o `Configuration` os lê de volta.
//! - `reloadConfiguration<T>(): Promise<T>` é o `Promise<()>` do trait.
//! - `getConfigurationData` devolve `None` (o `null` do original).
#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::map::ResourceMap;
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::ternary_search_tree::TernarySearchTree;
use je_base::uri::URI;
use je_platform::configuration::models::{Configuration, ConfigurationChangeEvent, ConfigurationInspectValue, ConfigurationKeys, ConfigurationModel};
use je_platform::configuration::registry::configuration_registry;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::{ConfigurationData, ConfigurationOverrides, ConfigurationTarget, ConfigurationUpdateOverrides, Overrides, get_configuration_value};
use je_platform::js::{object_keys, set_property};
use je_platform::log::{LogServiceRef, NullLogService};

/// `TestConfigurationService`.
pub struct TestConfigurationService {
    configuration: Dynamic,
    pub on_did_change_configuration_emitter: Rc<Emitter<Rc<ConfigurationChangeEvent>>>,
    configuration_by_root: RefCell<TernarySearchTree<String, Dynamic>>,
    override_identifiers: RefCell<HashMap<String, Vec<String>>>,
}

/// O `fsPath` de um recurso, a chave dos `configurationByRoot`.
fn fs_path_of(resource: &URI) -> String {
    resource.fs_path(Platform::detect().path_flavor()).to_string_lossy()
}

fn empty_object() -> Dynamic {
    Dynamic::object(Vec::<(&str, Dynamic)>::new())
}

impl TestConfigurationService {
    /// `constructor(configuration?)`: sem argumento, o objeto vazio.
    pub fn new(configuration: Option<Dynamic>) -> Rc<Self> {
        Rc::new(Self {
            configuration: configuration.unwrap_or_else(empty_object),
            on_did_change_configuration_emitter: Emitter::new(),
            configuration_by_root: RefCell::new(TernarySearchTree::for_paths(false)),
            override_identifiers: RefCell::new(HashMap::new()),
        })
    }

    /// `setUserConfiguration`.
    pub fn set_user_configuration(&self, key: &str, value: Dynamic, root: Option<&URI>) -> Promise<()> {
        match root {
            Some(root) => {
                let fs_path = fs_path_of(root);
                let mut configuration_by_root = self.configuration_by_root.borrow_mut();
                let config_for_root = configuration_by_root.get(&fs_path).cloned().unwrap_or_else(empty_object);
                set_property(&config_for_root, &JsString::from(key), value);
                configuration_by_root.set(fs_path, config_for_root);
            }
            None => set_property(&self.configuration, &JsString::from(key), value),
        }

        Promise::resolved(())
    }

    /// `setOverrideIdentifiers`.
    pub fn set_override_identifiers(&self, key: &str, identifiers: Vec<String>) {
        self.override_identifiers.borrow_mut().insert(key.to_owned(), identifiers);
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
        let mut configuration = None;
        if let Some(resource) = overrides.and_then(|overrides| overrides.resource.as_ref()) {
            configuration = self.configuration_by_root.borrow().find_substr(&fs_path_of(resource)).cloned();
        }
        let configuration = configuration.unwrap_or_else(|| self.configuration.clone());
        match section.filter(|section| !section.is_empty()) {
            Some(section) => {
                // `configuration[arg1] ?? getConfigurationValue(configuration, arg1)`
                let direct = configuration.get_str(section);
                if direct.is_nullish() { get_configuration_value(&configuration, section, None) } else { direct }
            }
            None => configuration,
        }
    }

    fn update_value(&self, _key: &str, _value: Dynamic, _overrides: Option<ConfigurationUpdateOverrides>, _target: Option<ConfigurationTarget>) -> Promise<()> {
        Promise::resolved(())
    }

    fn inspect(&self, key: &str, overrides: &ConfigurationOverrides) -> ConfigurationInspectValue {
        let value = self.get_value(Some(key), Some(overrides));
        let log: LogServiceRef = NullLogService::shared();

        let user = ConfigurationModel::create_empty_model(log.clone());
        if !value.is_undefined() {
            user.set_value(key, value);
        }
        let override_blocks: Vec<Overrides> = self
            .override_identifiers
            .borrow()
            .get(key)
            .map(|identifiers| vec![Overrides::new(identifiers, user.contents(), &[key])])
            .unwrap_or_default();
        let user = ConfigurationModel::new(user.contents(), user.keys(), override_blocks, None, log.clone());

        let empty = || ConfigurationModel::create_empty_model(log.clone());
        Configuration::new(empty(), empty(), empty(), user, empty(), empty(), ResourceMap::new(), empty(), ResourceMap::new(), log.clone()).inspect(
            key,
            overrides,
            None,
        )
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
