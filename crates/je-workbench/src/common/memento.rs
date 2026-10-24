//! Porte de `src/vs/workbench/common/memento.ts`: o `Memento`, o objeto de estado que um componente carrega do
//! armazenamento, edita e grava de volta.
//!
//! Notas de tradução:
//! - **`Partial<T>`.** O memento devolvido por `getMemento` é um objeto compartilhado e mutável (o chamador atribui
//!   e apaga propriedades, e quem pede de novo vê a mesma instância), então é o [`ObjectRef`] do `Dynamic`. Um valor
//!   gravado que não seja objeto JSON carrega como objeto vazio, porque o `Partial<T>` do tipo só admite objeto.
//! - **Estáticos.** Os quatro mapas `applicationMementos`, `applicationSharedMementos`, `profileMementos` e
//!   `workspaceMementos` são um `thread_local` com um mapa por escopo (o escopo escolhe o mapa, e o código do
//!   original para cada escopo era o mesmo). Cada thread (cada teste) tem os seus, como cada processo de teste do
//!   original tem o seu módulo.
//! - **`try`/`catch` de `doLoad`.** A leitura que lança (pânico com `Error`) vira [`errors::catch`], e o texto do
//!   erro inesperado traz o escopo pelo valor numérico do `StorageScope`, como o `${this.scope}` do original.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::json::{Object, Value};
use je_base::lifecycle::DisposableStore;
use je_base::types::is_empty_object;
use je_platform::js::to_json;
use je_platform::storage::{StorageScope, StorageService, StorageTarget, StorageValueChangeEvent};
use je_storage::StorageValue;

/// O índice do mapa do escopo, na ordem em que `saveMemento` percorre (workspace, perfil, aplicação, compartilhado).
fn scope_slot(scope: StorageScope) -> usize {
    match scope {
        StorageScope::Workspace => 0,
        StorageScope::Profile => 1,
        StorageScope::Application => 2,
        StorageScope::ApplicationShared => 3,
    }
}

thread_local! {
    /// Os mementos já carregados, por escopo e por id.
    static MEMENTOS: RefCell<[HashMap<String, Rc<ScopedMemento>>; 4]> = RefCell::new(Default::default());
}

/// `Memento<T extends object>`.
pub struct Memento {
    id: String,
    storage_service: Rc<dyn StorageService>,
}

impl Memento {
    const COMMON_PREFIX: &'static str = "memento/";

    pub fn new(id: &str, storage_service: Rc<dyn StorageService>) -> Self {
        Self { id: format!("{}{id}", Self::COMMON_PREFIX), storage_service }
    }

    fn scoped(&self, scope: StorageScope) -> Option<Rc<ScopedMemento>> {
        MEMENTOS.with(|mementos| mementos.borrow()[scope_slot(scope)].get(&self.id).cloned())
    }

    pub fn get_memento(&self, scope: StorageScope, target: StorageTarget) -> ObjectRef {
        let scoped = self.scoped(scope).unwrap_or_else(|| {
            let created = ScopedMemento::new(&self.id, scope, target, self.storage_service.clone());
            MEMENTOS.with(|mementos| mementos.borrow_mut()[scope_slot(scope)].insert(self.id.clone(), created.clone()));
            created
        });
        scoped.get_memento()
    }

    pub fn on_did_change_value(&self, scope: StorageScope, disposables: &DisposableStore) -> Event<StorageValueChangeEvent> {
        self.storage_service.on_did_change_value(scope, Some(&self.id), disposables)
    }

    pub fn save_memento(&self) {
        let scopes = [StorageScope::Workspace, StorageScope::Profile, StorageScope::Application, StorageScope::ApplicationShared];
        for scoped in scopes.into_iter().filter_map(|scope| self.scoped(scope)) {
            scoped.save();
        }
    }

    pub fn reload_memento(&self, scope: StorageScope) {
        if let Some(scoped) = self.scoped(scope) {
            scoped.reload();
        }
    }

    pub fn clear(scope: StorageScope) {
        MEMENTOS.with(|mementos| mementos.borrow_mut()[scope_slot(scope)].clear());
    }
}

struct ScopedMemento {
    id: String,
    scope: StorageScope,
    target: StorageTarget,
    storage_service: Rc<dyn StorageService>,
    memento_obj: ObjectRef,
}

impl ScopedMemento {
    fn new(id: &str, scope: StorageScope, target: StorageTarget, storage_service: Rc<dyn StorageService>) -> Rc<Self> {
        let memento_obj = Self::do_load(id, scope, storage_service.as_ref());
        Rc::new(Self { id: id.to_string(), scope, target, storage_service, memento_obj })
    }

    fn do_load(id: &str, scope: StorageScope, storage_service: &dyn StorageService) -> ObjectRef {
        match errors::catch(|| storage_service.get_object(id, scope, Some(Value::Object(Object::new())))) {
            Ok(Some(value)) => {
                if let Dynamic::Object(object) = Dynamic::from_json(&value) {
                    return object;
                }
            }
            Ok(None) => {}
            Err(error) => {
                // Seeing reports from users unable to open editors
                // from memento parsing exceptions. Log the contents
                // to diagnose further
                // https://github.com/microsoft/vscode/issues/102251
                let contents = storage_service.get(id, scope, None).unwrap_or_else(|| "undefined".to_string());
                errors::on_unexpected_error(&Error::new(format!(
                    "[memento]: failed to parse contents: {error} (id: {id}, scope: {}, contents: {contents})",
                    scope as i32
                )));
            }
        }

        ObjectRef::new()
    }

    fn get_memento(&self) -> ObjectRef {
        self.memento_obj.clone()
    }

    fn reload(&self) {
        // Clear old
        for name in self.memento_obj.keys() {
            self.memento_obj.delete(&name);
        }

        // Assign new
        for (name, value) in Self::do_load(&self.id, self.scope, self.storage_service.as_ref()).entries() {
            self.memento_obj.set(&name, value);
        }
    }

    fn save(&self) {
        let memento = Dynamic::Object(self.memento_obj.clone());
        match to_json(&memento) {
            Some(value) if !is_empty_object(&memento) => {
                self.storage_service.store(&self.id, StorageValue::Object(value), self.scope, self.target);
            }
            _ => self.storage_service.remove(&self.id, self.scope),
        }
    }
}
