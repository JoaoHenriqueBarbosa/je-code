//! Porte de `src/vs/workbench/contrib/customEditor/common/customEditorModelManager.ts`: o `CustomEditorModelManager`, que
//! guarda os modelos dos editores personalizados por recurso e tipo de visão, com a contagem de referências.
//!
//! Divergências:
//! - O `Map<string, ...>` (que mantém a ordem de inserção) é uma lista de pares chave e entrada, compartilhada
//!   (`Rc<RefCell<...>>`) com os descartes das referências. As entradas são `Rc` (a identidade `this._references.get(key)
//!   === entry` é `Rc::ptr_eq`) e o contador é um `Cell`.
//! - `IReference<ICustomEditorModel>` é a [`Reference`]; o `dispose` de chamada única (`createSingleCallFunction`) é o
//!   `to_disposable`.
//! - `getAllModels` percorre uma foto das entradas do começo da chamada (o original percorre o mapa vivo enquanto espera
//!   cada modelo).
//! - `add` com a chave já registrada lança (`Model already exists`) pelo [`errors::throw`], e o `catch` do modelo que falha é a
//!   conferência do resultado da promessa (que o torna tratado, como o `.catch` do original).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::errors::{self, Error};
use je_base::lifecycle::{IDisposable, Reference, to_disposable};
use je_base::promise::Promise;
use je_base::uri::URI;

use super::custom_editor::{CustomEditorModelReference, ICustomEditorModel, ICustomEditorModelManager};

struct Entry {
    view_type: String,
    model: Promise<Rc<dyn ICustomEditorModel>>,
    counter: Cell<i64>,
}

type References = Rc<RefCell<Vec<(String, Rc<Entry>)>>>;

/// `CustomEditorModelManager`.
#[derive(Default)]
pub struct CustomEditorModelManager {
    references: References,
}

impl CustomEditorModelManager {
    pub fn new() -> Self {
        Self::default()
    }

    fn key(resource: &URI, view_type: &str) -> String {
        format!("{}@@@{view_type}", resource.to_string(false))
    }

    fn entry_for(&self, key: &str) -> Option<Rc<Entry>> {
        self.references.borrow().iter().find(|(entry_key, _)| entry_key == key).map(|(_, entry)| entry.clone())
    }

    fn remove(references: &References, key: &str) {
        references.borrow_mut().retain(|(entry_key, _)| entry_key != key);
    }

    fn is_current(references: &References, key: &str, entry: &Rc<Entry>) -> bool {
        references.borrow().iter().any(|(entry_key, current)| entry_key == key && Rc::ptr_eq(current, entry))
    }

    fn dispose_model(model: &Promise<Rc<dyn ICustomEditorModel>>) {
        // nothing to dispose when the model failed to resolve
        let _ = model.then_result(|result| {
            if let Ok(model) = result {
                model.dispose();
            }
            Ok(())
        });
    }
}

impl ICustomEditorModelManager for CustomEditorModelManager {
    fn get_all_models(&self, resource: &URI) -> Promise<Vec<Rc<dyn ICustomEditorModel>>> {
        let key_start = format!("{}@@@", resource.to_string(false));
        let entries: Vec<Rc<Entry>> = self.references.borrow().iter().filter(|(key, _)| key.starts_with(&key_start)).map(|(_, entry)| entry.clone()).collect();
        async_fn(async move {
            let mut models = Vec::new();
            for entry in entries {
                // Skip models that failed to resolve
                if let Ok(model) = entry.model.clone().await {
                    models.push(model);
                }
            }
            Ok(models)
        })
    }

    fn get(&self, resource: &URI, view_type: &str) -> Promise<Option<Rc<dyn ICustomEditorModel>>> {
        match self.entry_for(&Self::key(resource, view_type)) {
            Some(entry) => entry.model.then(Some),
            None => Promise::resolved(None),
        }
    }

    fn try_retain(&self, resource: &URI, view_type: &str) -> Option<Promise<CustomEditorModelReference>> {
        let key = Self::key(resource, view_type);
        let entry = self.entry_for(&key)?;

        entry.counter.set(entry.counter.get() + 1);

        let references = self.references.clone();
        let retained = entry.clone();
        Some(retained.model.then(move |model| {
            Reference::new(
                model,
                to_disposable(move || {
                    // The model may already be disposed and replaced, for example after its resource was deleted
                    entry.counter.set(entry.counter.get() - 1);
                    if entry.counter.get() <= 0 && Self::is_current(&references, &key, &entry) {
                        Self::dispose_model(&entry.model);
                        Self::remove(&references, &key);
                    }
                }),
            )
        }))
    }

    fn add(&self, resource: &URI, view_type: &str, model: Promise<Rc<dyn ICustomEditorModel>>) -> Promise<CustomEditorModelReference> {
        let key = Self::key(resource, view_type);
        if self.entry_for(&key).is_some() {
            errors::throw(Error::new("Model already exists"));
        }

        let entry = Rc::new(Entry { view_type: view_type.to_owned(), model: model.clone(), counter: Cell::new(0) });
        self.references.borrow_mut().push((key.clone(), entry.clone()));

        // Forget models that fail to resolve so that opening the resource again creates a new model
        let references = self.references.clone();
        let _ = model.then_result(move |result| {
            if result.is_err() && Self::is_current(&references, &key, &entry) {
                Self::remove(&references, &key);
            }
            Ok(())
        });

        self.try_retain(resource, view_type).expect("o modelo acabou de ser registrado")
    }

    fn dispose_all_models_for_view(&self, view_type: &str) {
        let matching: Vec<(String, Rc<Entry>)> = self.references.borrow().iter().filter(|(_, entry)| entry.view_type == view_type).cloned().collect();
        for (key, entry) in matching {
            Self::dispose_model(&entry.model);
            Self::remove(&self.references, &key);
        }
    }

    fn dispose_all_models_for_resource(&self, resource: &URI) {
        let key_start = format!("{}@@@", resource.to_string(false));
        let matching: Vec<(String, Rc<Entry>)> = self.references.borrow().iter().filter(|(key, _)| key.starts_with(&key_start)).cloned().collect();
        for (key, entry) in matching {
            Self::dispose_model(&entry.model);
            Self::remove(&self.references, &key);
        }
    }
}
