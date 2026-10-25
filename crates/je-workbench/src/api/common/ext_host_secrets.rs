//! Porte de `src/vs/workbench/api/common/extHostSecrets.ts`: o `vscode.SecretStorage` do `ExtensionContext`, sobre o
//! [`ExtHostSecretState`].
//!
//! Divergências:
//! - `Event.map(Event.filter(...))` é um [`Emitter`] que o listener do `onDidChangePassword` alimenta com a chave, só para a extensão
//!   dona (o `disposables` do original é a assinatura que `dispose()` solta).
//! - `this.#secretState.keys(id) || []` é só a promessa de chaves (ela nunca é `undefined` no porte).

use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::Promise;

use super::ext_host_storage::ExtHostSecretState;

/// `ExtensionSecrets`.
pub struct ExtensionSecrets {
    id: String,
    secret_state: Rc<ExtHostSecretState>,
    on_did_change: Rc<Emitter<String>>,
    subscription: DisposableRef,
}

impl ExtensionSecrets {
    /// `extension_identifier` é o `ExtensionIdentifier.toKey(extensionDescription.identifier)`.
    pub fn new(extension_identifier: &str, secret_state: Rc<ExtHostSecretState>) -> Rc<Self> {
        let id = extension_identifier.to_owned();
        let on_did_change = Emitter::new();
        let (own_id, sink) = (id.clone(), on_did_change.clone());
        let subscription = secret_state.on_did_change_password().listen(move |(extension_id, key)| {
            if *extension_id == own_id {
                sink.fire(key.clone());
            }
        });
        Rc::new(Self { id, secret_state, on_did_change, subscription })
    }

    /// `onDidChange`: a chave que mudou.
    pub fn on_did_change(&self) -> Event<String> {
        self.on_did_change.event()
    }

    pub fn get(&self, key: &str) -> Promise<Option<String>> {
        self.secret_state.get(&self.id, key)
    }

    pub fn store(&self, key: &str, value: &str) -> Promise<()> {
        self.secret_state.store(&self.id, key, value)
    }

    pub fn delete(&self, key: &str) -> Promise<()> {
        self.secret_state.delete(&self.id, key)
    }

    pub fn keys(&self) -> Promise<Vec<String>> {
        self.secret_state.keys(&self.id)
    }
}

impl IDisposable for ExtensionSecrets {
    fn dispose(&self) {
        self.subscription.dispose();
    }
}
