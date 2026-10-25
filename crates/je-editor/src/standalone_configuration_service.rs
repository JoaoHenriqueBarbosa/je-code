//! Porte de `StandaloneConfigurationService` de `src/vs/editor/standalone/browser/standaloneServices.ts`: o
//! `IConfigurationService` do editor standalone, que guarda os valores só na camada de memória sobre os
//! valores padrão do registro.
//!
//! Decisões de tradução:
//! - **`getValue(arg1?, arg2?)`** é a forma do trait [`IConfigurationService`]: seção e `overrides` opcionais.
//! - **`updateValue(key, value, arg3?, arg4?)`** ignora `overrides` e `target`, como o original.
//! - **`updateValues`** é [`StandaloneConfigurationService::update_values`], com a comparação `===` do original
//!   ([`Dynamic::strict_equals`]).

use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::map::ResourceMap;
use je_base::promise::Promise;
use je_platform::configuration::defaults::DefaultConfiguration;
use je_platform::configuration::models::{Configuration, ConfigurationChangeEvent, ConfigurationKeys, ConfigurationModel, PreviousConfiguration};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::{
    ConfigurationChange, ConfigurationData, ConfigurationOverrides, ConfigurationTarget, ConfigurationUpdateOverrides, ConfigurationValue,
};
use je_platform::log::LogServiceRef;

/// `StandaloneConfigurationService`.
pub struct StandaloneConfigurationService {
    on_did_change_configuration: Rc<Emitter<Rc<ConfigurationChangeEvent>>>,
    configuration: Rc<Configuration>,
    log_service: LogServiceRef,
}

impl StandaloneConfigurationService {
    pub fn new(log_service: LogServiceRef) -> Rc<Self> {
        let on_did_change_configuration = Emitter::new();
        let default_configuration = DefaultConfiguration::new(log_service.clone());
        let empty = || ConfigurationModel::create_empty_model(log_service.clone());
        let configuration = Configuration::new(
            default_configuration.reload(),
            empty(),
            empty(),
            empty(),
            empty(),
            empty(),
            ResourceMap::new(),
            empty(),
            ResourceMap::new(),
            log_service.clone(),
        );
        default_configuration.dispose();
        Rc::new(Self { on_did_change_configuration, configuration: Rc::new(configuration), log_service })
    }

    /// `updateValues(values)`: grava na memória os pares cujo valor mudou e dispara um só evento com as chaves
    /// alteradas.
    pub fn update_values(&self, values: Vec<(&str, Dynamic)>) -> Promise<()> {
        let previous = PreviousConfiguration { workspace: None, data: self.configuration.to_data() };

        let mut changed_keys: Vec<JsString> = Vec::new();
        for (key, value) in values {
            if IConfigurationService::get_value(self, Some(key), None).strict_equals(&value) {
                continue;
            }
            self.configuration.update_value(key, value, &ConfigurationUpdateOverrides::default());
            changed_keys.push(JsString::from(key));
        }

        if !changed_keys.is_empty() {
            let event = ConfigurationChangeEvent::new(
                ConfigurationChange { keys: changed_keys, overrides: Vec::new() },
                Some(previous),
                self.configuration.clone(),
                None,
                self.log_service.clone(),
            );
            event.source.set(Some(ConfigurationTarget::Memory));
            self.on_did_change_configuration.fire(Rc::new(event));
        }

        Promise::resolved(())
    }
}

impl IConfigurationService for StandaloneConfigurationService {
    fn on_did_change_configuration(&self) -> Event<Rc<ConfigurationChangeEvent>> {
        self.on_did_change_configuration.event()
    }

    fn get_configuration_data(&self) -> Option<ConfigurationData> {
        None
    }

    fn get_value(&self, section: Option<&str>, overrides: Option<&ConfigurationOverrides>) -> Dynamic {
        let section = section.map(JsString::from);
        self.configuration.get_value(section.as_ref(), &overrides.cloned().unwrap_or_default(), None)
    }

    fn update_value(&self, key: &str, value: Dynamic, _overrides: Option<ConfigurationUpdateOverrides>, _target: Option<ConfigurationTarget>) -> Promise<()> {
        self.update_values(vec![(key, value)])
    }

    fn inspect(&self, key: &str, overrides: &ConfigurationOverrides) -> Rc<dyn ConfigurationValue> {
        Rc::new(self.configuration.inspect(key, overrides, None))
    }

    fn reload_configuration(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn keys(&self) -> ConfigurationKeys {
        self.configuration.keys(None)
    }
}
