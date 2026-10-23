//! O `TestConfigurationService` (só o que o `ContextKeyService` lê) e um nó de contexto no lugar do
//! `document.createElement('div')` dos testes do `contextkey`.
#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_platform::contextkey::IContextKeyServiceTarget;
use je_platform::contextkey::service::{ContextKeyConfigurationChange, ContextKeyConfigurationService};

/// `TestConfigurationService`: nenhum valor, e um emissor que o teste dispara.
pub struct TestConfigurationService {
    values: RefCell<HashMap<String, Dynamic>>,
    emitter: Rc<Emitter<ContextKeyConfigurationChange>>,
}

impl TestConfigurationService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { values: RefCell::new(HashMap::new()), emitter: Emitter::new() })
    }

    pub fn set_user_configuration(&self, key: &str, value: Dynamic) {
        self.values.borrow_mut().insert(key.to_string(), value);
    }

    pub fn fire_change(&self, change: ContextKeyConfigurationChange) {
        self.emitter.fire(change);
    }
}

impl ContextKeyConfigurationService for TestConfigurationService {
    fn get_value(&self, section: &str) -> Dynamic {
        self.values.borrow().get(section).cloned().unwrap_or(Dynamic::Undefined)
    }

    fn on_did_change_configuration(&self) -> Event<ContextKeyConfigurationChange> {
        self.emitter.event()
    }
}

/// O `HTMLElement` que só tem pai e atributos.
pub struct TestTarget {
    parent: Option<Rc<dyn IContextKeyServiceTarget>>,
    attributes: RefCell<HashMap<String, String>>,
}

impl TestTarget {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { parent: None, attributes: RefCell::new(HashMap::new()) })
    }

    pub fn with_parent(parent: Rc<dyn IContextKeyServiceTarget>) -> Rc<Self> {
        Rc::new(Self { parent: Some(parent), attributes: RefCell::new(HashMap::new()) })
    }
}

impl IContextKeyServiceTarget for TestTarget {
    fn parent_element(&self) -> Option<Rc<dyn IContextKeyServiceTarget>> {
        self.parent.clone()
    }

    fn set_attribute(&self, attr: &str, value: &str) {
        self.attributes.borrow_mut().insert(attr.to_string(), value.to_string());
    }

    fn remove_attribute(&self, attr: &str) {
        self.attributes.borrow_mut().remove(attr);
    }

    fn has_attribute(&self, attr: &str) -> bool {
        self.attributes.borrow().contains_key(attr)
    }

    fn get_attribute(&self, attr: &str) -> Option<String> {
        self.attributes.borrow().get(attr).cloned()
    }
}
