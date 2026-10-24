//! Porte de `src/vs/workbench/contrib/externalUriOpener/common/contributedOpeners.ts`: o `ContributedExternalUriOpenersStore`,
//! que lembra (no `Memento` do perfil) quais openers externos de URI cada extensão registrou, para que a configuração
//! `workbench.externalUriOpeners` conheça os ids antes de a extensão ativar, e descarta o que veio de extensões que não
//! estão mais instaladas ou habilitadas.
//!
//! Divergências:
//! - `RegisteredExternalOpener` (`{ extensionId, isCurrentlyRegistered }`) é um [`ObjectRef`]: o original guarda o MESMO objeto
//!   no `Map` `_openers` e no memento (`_mementoObject[id] = entry`), e `add` muda `isCurrentlyRegistered` do existente
//!   sem gravar de novo, então a mudança só chega ao armazenamento no próximo `saveMemento`. A identidade compartilhada
//!   é o `ObjectRef`.
//! - O `Map` `_openers` é um [`IndexMap`] (a ordem de inserção dá a ordem dos ids no esquema). `delete` é `shift_remove`.
//! - Um valor do memento que não é objeto com `extensionId` em texto (o que esta classe nunca grava) é ignorado na carga;
//!   no original o `value.extensionId` ficaria `undefined`.
//! - `invalidateOpenersOnExtensionsChanged` é uma função `async` ([`async_fn`]) chamada sem `await`, como no original; o
//!   `Map` é percorrido sobre uma cópia dos pares, o que equivale ao `Map` vivo porque não há `await` dentro do laço.
//! - `IStorageService` é o [`StorageService`] de `je_platform::storage`; o `_register(extensionService.onDidChange...)`
//!   guarda o descarte do ouvinte no [`Disposable`] base.

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::JsString;
use je_base::async_::async_fn;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};

use super::configuration::update_contributed_openers;
use crate::common::memento::Memento;
use crate::services::extensions::extensions::IExtensionService;

const STORAGE_ID: &str = "externalUriOpeners";

fn key(name: &str) -> JsString {
    JsString::from(name)
}

/// `entry.extensionId`.
fn extension_id_of(entry: &ObjectRef) -> String {
    match entry.get(&key("extensionId")) {
        Some(Dynamic::String(extension_id)) => extension_id.to_string_lossy(),
        _ => String::new(),
    }
}

/// `entry.isCurrentlyRegistered`.
fn is_currently_registered(entry: &ObjectRef) -> bool {
    matches!(entry.get(&key("isCurrentlyRegistered")), Some(Dynamic::Bool(true)))
}

struct Inner {
    openers: RefCell<IndexMap<String, ObjectRef>>,
    memento: Memento,
    memento_object: ObjectRef,
    extension_service: Rc<dyn IExtensionService>,
}

impl Inner {
    fn add(&self, id: &str, extension_id: &str, currently_registered: bool) {
        let existing = self.openers.borrow().get(id).cloned();
        if let Some(existing) = existing {
            existing.set(&key("isCurrentlyRegistered"), Dynamic::Bool(is_currently_registered(&existing) || currently_registered));
            return;
        }

        let entry = ObjectRef::from_entries([("extensionId", Dynamic::string(extension_id)), ("isCurrentlyRegistered", Dynamic::Bool(currently_registered))]);
        self.openers.borrow_mut().insert(id.to_owned(), entry.clone());

        self.memento_object.set(&key(id), Dynamic::Object(entry));
        self.memento.save_memento();

        self.update_schema();
    }

    fn delete(&self, id: &str) {
        self.openers.borrow_mut().shift_remove(id);

        self.memento_object.delete(&key(id));
        self.memento.save_memento();

        self.update_schema();
    }

    /// `invalidateOpenersOnExtensionsChanged`.
    fn invalidate_openers_on_extensions_changed(self: &Rc<Self>) -> Promise<()> {
        let this = Rc::clone(self);
        async_fn(async move {
            this.extension_service.when_installed_extensions_registered().await?;
            let registered_extensions = this.extension_service.extensions();

            let openers: Vec<(String, ObjectRef)> = this.openers.borrow().iter().map(|(id, entry)| (id.clone(), entry.clone())).collect();
            for (id, entry) in openers {
                let entry_extension_id = extension_id_of(&entry);
                match registered_extensions.iter().find(|registered| registered.identifier.value == entry_extension_id) {
                    Some(extension) => {
                        if !this.extension_service.can_remove_extension(extension) {
                            // The extension is running. We should have registered openers at this point
                            if !is_currently_registered(&entry) {
                                this.delete(&id);
                            }
                        }
                    }
                    // The opener came from an extension that is no longer enabled/installed
                    None => this.delete(&id),
                }
            }
            Ok(())
        })
    }

    fn update_schema(&self) {
        let mut ids = Vec::new();
        let mut descriptions = Vec::new();

        for (id, entry) in self.openers.borrow().iter() {
            ids.push(id.clone());
            descriptions.push(extension_id_of(entry));
        }

        update_contributed_openers(&ids, &descriptions);
    }
}

/// `ContributedExternalUriOpenersStore`.
pub struct ContributedExternalUriOpenersStore {
    base: Rc<Disposable>,
    inner: Rc<Inner>,
}

impl ContributedExternalUriOpenersStore {
    pub fn new(storage_service: Rc<dyn StorageService>, extension_service: Rc<dyn IExtensionService>) -> Rc<Self> {
        let base = Disposable::new();

        let memento = Memento::new(STORAGE_ID, storage_service);
        let memento_object = memento.get_memento(StorageScope::Profile, StorageTarget::Machine);
        let inner = Rc::new(Inner { openers: RefCell::new(IndexMap::new()), memento, memento_object, extension_service: Rc::clone(&extension_service) });

        for (id, value) in inner.memento_object.entries() {
            if let Dynamic::Object(value) = value
                && let Some(Dynamic::String(extension_id)) = value.get(&key("extensionId"))
            {
                inner.add(&id.to_string_lossy(), &extension_id.to_string_lossy(), false);
            }
        }

        let _ = inner.invalidate_openers_on_extensions_changed();

        let on_did_change_extensions = Rc::clone(&inner);
        base.register(extension_service.on_did_change_extensions().listen(move |_| {
            let _ = on_did_change_extensions.invalidate_openers_on_extensions_changed();
        }));
        let on_did_change_extensions_status = Rc::clone(&inner);
        base.register(extension_service.on_did_change_extensions_status().listen(move |_| {
            let _ = on_did_change_extensions_status.invalidate_openers_on_extensions_changed();
        }));

        Rc::new(Self { base, inner })
    }

    pub fn did_register_opener(&self, id: &str, extension_id: &str) {
        self.inner.add(id, extension_id, true);
    }

    pub fn delete(&self, id: &str) {
        self.inner.delete(id);
    }
}

impl IDisposable for ContributedExternalUriOpenersStore {
    fn dispose(&self) {
        self.base.dispose();
    }
}
