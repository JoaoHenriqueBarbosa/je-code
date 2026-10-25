//! Porte de `src/vs/workbench/api/common/extHostMemento.ts`: o `vscode.Memento` do `ExtensionContext` (`globalState` e `workspaceState`),
//! sobre o [`ExtHostStorage`].
//!
//! Divergências:
//! - O valor guardado (`{ [n: string]: any }`) é uma lista ordenada de pares `(chave, Dynamic)`: o `Object.entries` do original
//!   mantém a ordem de inserção, e a lista também. `JSON.parse(JSON.stringify(value))` de `update` é o `json_stringify` seguido do
//!   `json_parse` do `je-base`.
//! - `get` antes de `whenReady` assentar devolve o padrão (o original lança um `TypeError` por ler `undefined[key]`; a ponte do host só
//!   entrega o objeto depois de `whenReady`).
//! - `RunOnceScheduler` com atraso 0 é o [`RunOnceScheduler`] do `je-base` (a gravação sai no próximo turno do laço).

use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;

use je_base::async_::RunOnceScheduler;
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::{DeferredPromise, Promise};

use super::ext_host_protocol::IExtensionIdWithVersion;
use super::ext_host_storage::ExtHostStorage;

/// O valor de um memento: os pares na ordem de inserção.
type MementoEntries = Vec<(String, Dynamic)>;

/// Os pares de um objeto lido do armazenamento (`undefined` e qualquer outro tipo viram vazio).
fn entries_of(value: &Dynamic) -> MementoEntries {
    match value {
        Dynamic::Object(_) => value.own_entries().into_iter().map(|(key, item)| (key.to_string_lossy(), item)).collect(),
        _ => Vec::new(),
    }
}

/// `JSON.parse(JSON.stringify(value))`: o objeto como o workbench o devolverá, sem compartilhar estado com o chamador.
fn json_roundtrip(value: &Dynamic) -> Dynamic {
    json_stringify(value)
        .and_then(|text| {
            let units: Vec<u16> = text.encode_utf16().collect();
            je_base::jsonc::json_parse(&units).ok()
        })
        .map_or_else(|| value.clone(), |parsed| Dynamic::from_json(&parsed))
}

/// O estado que o agendador de gravação também enxerga.
struct MementoState {
    id: String,
    shared: bool,
    storage: Rc<ExtHostStorage>,
    value: RefCell<Option<MementoEntries>>,
    deferred_promises: RefCell<Vec<(String, Rc<DeferredPromise<()>>)>>,
}

impl MementoState {
    /// O corpo do `RunOnceScheduler`: grava o valor e completa (ou rejeita) as promessas que esperavam.
    fn flush(&self) {
        let records = std::mem::take(&mut *self.deferred_promises.borrow_mut());
        let entries = self.value.borrow().clone().unwrap_or_default();
        let value = Dynamic::object(entries.iter().map(|(key, item)| (key.as_str(), item.clone())));
        let _ = self.storage.set_value(self.shared, &self.id, value).then_result(move |outcome| {
            for (_, record) in &records {
                match &outcome {
                    Ok(()) => {
                        let _ = record.complete(());
                    }
                    Err(error) => {
                        let _ = record.error(error.clone());
                    }
                }
            }
            Ok(())
        });
    }
}

/// `ExtensionMemento`.
pub struct ExtensionMemento {
    state: Rc<MementoState>,
    init: Promise<()>,
    storage_listener: DisposableRef,
    scheduler: Rc<RunOnceScheduler>,
}

impl ExtensionMemento {
    pub fn new(id: &str, global: bool, storage: Rc<ExtHostStorage>) -> Rc<Self> {
        let state = Rc::new(MementoState {
            id: id.to_owned(),
            shared: global,
            storage: storage.clone(),
            value: RefCell::new(None),
            deferred_promises: RefCell::new(Vec::new()),
        });

        let init_state = state.clone();
        let init = storage.initialize_extension_storage(global, id, Some(Dynamic::object(Vec::<(&str, Dynamic)>::new()))).then(move |value| {
            *init_state.value.borrow_mut() = Some(value.as_ref().map(entries_of).unwrap_or_default());
        });

        let listener_state = state.clone();
        let storage_listener = storage.on_did_change_storage().listen(move |event| {
            if event.shared == listener_state.shared && event.key == listener_state.id {
                *listener_state.value.borrow_mut() = Some(entries_of(&event.value));
            }
        });

        let scheduler_state = state.clone();
        let scheduler = RunOnceScheduler::new(move || scheduler_state.flush(), 0);

        Rc::new(Self { state, init, storage_listener, scheduler })
    }

    /// `keys()`: sem os valores `undefined`, que ficam no `_value` até o `onDidChangeStorage` rodar.
    pub fn keys(&self) -> Vec<String> {
        self.state.value.borrow().iter().flatten().filter(|(_, value)| !value.is_undefined()).map(|(key, _)| key.clone()).collect()
    }

    /// `whenReady`.
    pub fn when_ready(&self) -> Promise<()> {
        self.init.clone()
    }

    /// `get(key, defaultValue?)`.
    pub fn get(&self, key: &str, default_value: Option<Dynamic>) -> Option<Dynamic> {
        let value = self.state.value.borrow().iter().flatten().find(|(name, _)| name == key).map(|(_, value)| value.clone());
        match value {
            Some(value) if !value.is_undefined() => Some(value),
            _ => default_value,
        }
    }

    /// `update(key, value)`.
    pub fn update(&self, key: &str, value: Dynamic) -> Promise<()> {
        // Prevent the value from being as-is for until we have received the change event from the main side by emulating the treatment
        // of values via JSON parsing and stringifying. (https://github.com/microsoft/vscode/issues/209479)
        let stored = if matches!(value, Dynamic::Object(_) | Dynamic::Array(_)) { json_roundtrip(&value) } else { value };
        {
            let mut guard = self.state.value.borrow_mut();
            let entries = guard.get_or_insert_with(Vec::new);
            match entries.iter_mut().find(|(name, _)| name == key) {
                Some(entry) => entry.1 = stored,
                None => entries.push((key.to_owned(), stored)),
            }
        }

        let record = self.state.deferred_promises.borrow().iter().find(|(name, _)| name == key).map(|(_, promise)| promise.promise.clone());
        if let Some(record) = record {
            return record;
        }

        let promise = Rc::new(DeferredPromise::new());
        let result = promise.promise.clone();
        self.state.deferred_promises.borrow_mut().push((key.to_owned(), promise));

        if !self.scheduler.is_scheduled() {
            self.scheduler.schedule(None);
        }
        result
    }
}

impl IDisposable for ExtensionMemento {
    fn dispose(&self) {
        self.storage_listener.dispose();
    }
}

/// `ExtensionGlobalMemento`: o memento compartilhado entre workspaces, com as chaves que o Settings Sync leva.
pub struct ExtensionGlobalMemento {
    memento: Rc<ExtensionMemento>,
    extension_version: String,
}

impl ExtensionGlobalMemento {
    pub fn new(extension_identifier: &str, extension_version: &str, storage: Rc<ExtHostStorage>) -> Rc<Self> {
        Rc::new(Self { memento: ExtensionMemento::new(extension_identifier, true, storage), extension_version: extension_version.to_owned() })
    }

    /// O `ExtensionMemento` que o global estende (o `super` do original).
    pub fn memento(&self) -> &Rc<ExtensionMemento> {
        &self.memento
    }

    /// `setKeysForSync(keys)`.
    pub fn set_keys_for_sync(&self, keys: Vec<String>) {
        self.memento.state.storage.register_extension_storage_keys_to_sync(
            IExtensionIdWithVersion { id: self.memento.state.id.clone(), version: self.extension_version.clone() },
            keys,
        );
    }
}

impl Deref for ExtensionGlobalMemento {
    type Target = ExtensionMemento;

    fn deref(&self) -> &ExtensionMemento {
        &self.memento
    }
}

impl IDisposable for ExtensionGlobalMemento {
    fn dispose(&self) {
        self.memento.dispose();
    }
}
