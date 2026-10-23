//! Porte de `DefaultConfiguration` de `src/vs/platform/configuration/common/configurations.ts`: o
//! modelo da camada de valores padrão, montado a partir do registro e atualizado quando ele muda.
//!
//! `initialize` é `async` no original sem nenhum `await`: o corpo roda inteiro de forma síncrona, e
//! aqui devolve o modelo direto, sem promessa. `PolicyConfiguration` e `NullPolicyConfiguration`
//! estão em [`super::policy_configuration`].

use std::cell::RefCell;
use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::objects;

use super::models::ConfigurationModel;
use super::registry::{PropertyDictionary, PropertySchema, configuration_registry};
use crate::log::LogServiceRef;

/// O evento `onDidChangeConfiguration`: `{ defaults, properties }`.
#[derive(Clone)]
pub struct DefaultConfigurationChange {
    pub defaults: ConfigurationModel,
    pub properties: Vec<JsString>,
}

/// `DefaultConfiguration`.
pub struct DefaultConfiguration {
    base: Rc<Disposable>,
    log: LogServiceRef,
    on_did_change_configuration: Rc<Emitter<DefaultConfigurationChange>>,
    configuration_model: RefCell<ConfigurationModel>,
}

impl DefaultConfiguration {
    pub fn new(log: LogServiceRef) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_configuration = Emitter::new();
        base.register(on_did_change_configuration.clone());
        Rc::new(Self { base, configuration_model: RefCell::new(ConfigurationModel::create_empty_model(log.clone())), log, on_did_change_configuration })
    }

    /// `onDidChangeConfiguration`.
    pub fn on_did_change_configuration(&self) -> Event<DefaultConfigurationChange> {
        self.on_did_change_configuration.event()
    }

    /// `configurationModel`.
    pub fn configuration_model(&self) -> ConfigurationModel {
        self.configuration_model.borrow().clone()
    }

    /// `initialize()`: monta o modelo e passa a ouvir o registro.
    pub fn initialize(self: &Rc<Self>) -> ConfigurationModel {
        self.reset_configuration_model();
        let this = Rc::downgrade(self);
        let listener: DisposableRef = configuration_registry().on_did_update_configuration().listen(move |update| {
            if let Some(this) = this.upgrade() {
                this.on_did_update_configuration(&update.properties);
            }
        });
        self.base.register(listener);
        self.configuration_model()
    }

    /// `reload()`.
    pub fn reload(&self) -> ConfigurationModel {
        self.reset_configuration_model();
        self.configuration_model()
    }

    fn on_did_update_configuration(&self, properties: &[JsString]) {
        self.update_configuration_model(properties, &self.get_configuration_properties());
        self.on_did_change_configuration.fire(DefaultConfigurationChange { defaults: self.configuration_model(), properties: properties.to_vec() });
    }

    fn reset_configuration_model(&self) {
        *self.configuration_model.borrow_mut() = ConfigurationModel::create_empty_model(self.log.clone());
        let properties = self.get_configuration_properties();
        self.update_configuration_model(&properties.keys(), &properties);
    }

    fn get_configuration_properties(&self) -> PropertyDictionary {
        let registry = configuration_registry();
        let properties = registry.get_configuration_properties().snapshot();
        // Hidden experimental settings need defaults and change events too, while
        // remaining absent from the schemas used by the Settings UI.
        for (key, property) in registry.get_excluded_configuration_properties().entries() {
            if property.get("experiment").is_truthy() {
                properties.set(&key, property);
            }
        }
        properties
    }

    fn update_configuration_model(&self, properties: &[JsString], configuration_properties: &PropertyDictionary) {
        let model = self.configuration_model();
        for key in properties {
            match configuration_properties.get(key) {
                Some(property_schema) => model.set_value(key, self.get_default_value(key, &property_schema)),
                None => model.remove_value(key),
            }
        }
    }

    fn get_default_value(&self, _key: &JsString, property_schema: &PropertySchema) -> Dynamic {
        objects::deep_clone(&property_schema.default_value())
    }
}

impl IDisposable for DefaultConfiguration {
    fn dispose(&self) {
        self.base.dispose();
    }
}
