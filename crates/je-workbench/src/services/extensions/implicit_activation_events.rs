//! Porte de `src/vs/platform/extensionManagement/common/implicitActivationEvents.ts`: os eventos de ativação que os pontos de
//! extensão geram sozinhos (`onLanguage:...`, `onCommand:...`), além dos declarados no `package.json`.
//!
//! Divergências:
//! - `IActivationEventsGenerator<T>` recebe as contribuições como JSON ([`Value`], a forma em que `desc.contributes` é lido
//!   pelo serviço de extensões) e devolve a lista de eventos; o `Iterable<string>` é um `Vec<String>`.
//! - `contributes` do porte é uma struct tipada com as chaves desconhecidas em `extra`: ela é serializada para JSON para
//!   iterar os pontos de extensão (`for (const extPointName in desc.contributes)`), na ordem dos campos da struct e depois
//!   as chaves extras, e não na ordem do `package.json`.
//! - O `WeakMap` de cache por descritor não existe: o resultado é função pura das contribuições e dos geradores
//!   registrados, e uma descrição do porte não tem identidade estável (`Rc` clonado por quem a lê).
//! - `createActivationEventsMap` devolve `HashMap<String, Vec<String>>` (a chave é o `ExtensionIdentifier.toKey`).
//! - O global `ImplicitActivationEvents` é um `thread_local` ([`implicit_activation_events`]).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::errors::{self, on_unexpected_error};
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use serde_json::Value;

use super::extension_description_registry::SharedExtensionDescription;
use super::extensions::extension_contributions;

/// `IActivationEventsGenerator<T>`.
pub type ActivationEventsGenerator = Rc<dyn Fn(&[Value]) -> Vec<String>>;

/// `ImplicitActivationEventsImpl`.
#[derive(Default)]
pub struct ImplicitActivationEventsImpl {
    generators: RefCell<HashMap<String, ActivationEventsGenerator>>,
}

impl ImplicitActivationEventsImpl {
    pub fn new() -> Self {
        Self::default()
    }

    /// `register(extensionPointName, generator)`.
    pub fn register(&self, extension_point_name: &str, generator: ActivationEventsGenerator) {
        self.generators.borrow_mut().insert(extension_point_name.to_owned(), generator);
    }

    /// `readActivationEvents(extensionDescription)`.
    ///
    /// This can run correctly only on the renderer process because that is the only place
    /// where all extension points and all implicit activation events generators are known.
    pub fn read_activation_events(&self, extension_description: &ExtensionDescription) -> Vec<String> {
        self.do_read_activation_events(extension_description)
    }

    /// `createActivationEventsMap(extensionDescriptions)`.
    ///
    /// This can run correctly only on the renderer process because that is the only place
    /// where all extension points and all implicit activation events generators are known.
    pub fn create_activation_events_map(&self, extension_descriptions: &[SharedExtensionDescription]) -> HashMap<String, Vec<String>> {
        let mut result: HashMap<String, Vec<String>> = HashMap::new();
        for extension_description in extension_descriptions {
            let activation_events = self.read_activation_events(extension_description);
            if !activation_events.is_empty() {
                result.insert(ExtensionIdentifier::to_key(&extension_description.identifier.value), activation_events);
            }
        }
        result
    }

    fn do_read_activation_events(&self, desc: &ExtensionDescription) -> Vec<String> {
        if desc.manifest.main.is_none() && desc.manifest.browser.is_none() {
            return Vec::new();
        }

        let mut activation_events: Vec<String> = desc.manifest.activation_events.clone().unwrap_or_default();

        for activation_event in activation_events.iter_mut() {
            // TODO@joao: there's no easy way to contribute this
            if activation_event == "onUri" {
                *activation_event = format!("onUri:{}", ExtensionIdentifier::to_key(&desc.identifier.value));
            }
        }

        if desc.manifest.contributes.is_none() {
            // no implicit activation events
            return activation_events;
        }

        for (ext_point_name, contrib) in &extension_contributions(desc) {
            let generator = self.generators.borrow().get(ext_point_name).cloned();
            let Some(generator) = generator else {
                // There's no generator for this extension point
                continue;
            };
            let contrib_arr: Vec<Value> = match contrib {
                Value::Array(items) => items.clone(),
                other => vec![other.clone()],
            };
            match errors::catch(|| generator(&contrib_arr)) {
                Ok(events) => activation_events.extend(events),
                Err(err) => on_unexpected_error(&err),
            }
        }

        activation_events
    }
}

thread_local! {
    static IMPLICIT_ACTIVATION_EVENTS: Rc<ImplicitActivationEventsImpl> = Rc::new(ImplicitActivationEventsImpl::new());
}

/// `ImplicitActivationEvents`.
pub fn implicit_activation_events() -> Rc<ImplicitActivationEventsImpl> {
    IMPLICIT_ACTIVATION_EVENTS.with(Rc::clone)
}
