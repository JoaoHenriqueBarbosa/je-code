//! Um nó de contexto no lugar do `document.createElement('div')` dos testes do `contextkey`.
#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_platform::contextkey::IContextKeyServiceTarget;

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
