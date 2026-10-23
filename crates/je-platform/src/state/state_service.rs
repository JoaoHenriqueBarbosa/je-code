//! Porte de `src/vs/platform/state/node/stateService.ts`: `FileStorage` (o JSON em disco, com gravação
//! imediata ou adiada), `StateReadonlyService` e `StateService` (no original, `StateService`; aqui
//! [`StateServiceImpl`], porque `StateService` é o trait de `state.ts`).
//!
//! Decisões de tradução:
//! - O objeto `storage` (`{ [key: string]: unknown }`) é um objeto dinâmico: remover um item o troca por
//!   `undefined`, e o `JSON.stringify` descarta a chave, como no original.
//! - `IEnvironmentService.stateResource` entra como o recurso, não o serviço de ambiente.
//! - `logService.error(error)` registra o erro como argumento.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::JsString;
use je_base::async_::{ThrottledDelayer, async_fn};
use je_base::buffer::VSBuffer;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::Error;
use je_base::json_formatter::json_stringify;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;

use super::state::{StateReadService, StateService};
use crate::files::file_service::FileService;
use crate::files::files::{FileAtomicOptions, FileOperationError, FileOperationResult, WriteFileOptions};
use crate::js::{from_json, to_json};
use crate::log::{LogArg, LogMessage, LogServiceRef};

/// `SaveStrategy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveStrategy {
    Immediate,
    Delayed,
}

fn is_undefined_or_null(value: &Dynamic) -> bool {
    value.is_nullish()
}

/// `FileStorage`.
pub struct FileStorage {
    base: Rc<Disposable>,
    storage_path: URI,
    log_service: LogServiceRef,
    file_service: Rc<FileService>,
    storage: RefCell<ObjectRef>,
    last_saved_storage_contents: RefCell<String>,
    flush_delayer: Rc<ThrottledDelayer<()>>,
    initializing: RefCell<Option<Promise<()>>>,
    closing: RefCell<Option<Promise<()>>>,
    /// `this.closing !== undefined` visto de dentro das promessas.
    is_closing: Cell<bool>,
}

impl FileStorage {
    pub fn new(storage_path: URI, save_strategy: SaveStrategy, log_service: LogServiceRef, file_service: Rc<FileService>) -> Rc<Self> {
        let base = Disposable::new();
        // buffer saves over a short time
        let flush_delayer = Rc::new(ThrottledDelayer::new(if save_strategy == SaveStrategy::Immediate { 0 } else { 100 }));
        base.register(flush_delayer.clone());
        Rc::new(Self {
            base,
            storage_path,
            log_service,
            file_service,
            storage: RefCell::new(ObjectRef::new()),
            last_saved_storage_contents: RefCell::new(String::new()),
            flush_delayer,
            initializing: RefCell::new(None),
            closing: RefCell::new(None),
            is_closing: Cell::new(false),
        })
    }

    /// `init()`.
    pub fn init(self: &Rc<Self>) -> Promise<()> {
        if self.initializing.borrow().is_none() {
            let promise = self.do_init();
            *self.initializing.borrow_mut() = Some(promise);
        }

        self.initializing.borrow().clone().expect("definido acima")
    }

    fn do_init(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let result: Result<(), Error> = async {
                let contents = this.file_service.read_file(&this.storage_path, None, None).await?.value.to_string();
                *this.last_saved_storage_contents.borrow_mut() = contents.clone();
                let units: Vec<u16> = contents.encode_utf16().collect();
                let parsed = je_base::jsonc::json_parse(&units).map_err(|error| Error::named("SyntaxError", error.message))?;
                // `this.storage = JSON.parse(...)`: só um objeto serve de armazém.
                *this.storage.borrow_mut() = match from_json(&parsed) {
                    Dynamic::Object(object) => object,
                    _ => ObjectRef::new(),
                };
                Ok(())
            }
            .await;
            if let Err(error) = result
                && FileOperationError::from_error(&error).map(|e| e.file_operation_result) != Some(FileOperationResult::FileNotFound)
            {
                this.log_service.error_args(&LogMessage::Text(error.to_string()), &[LogArg::Error(error)]);
            }
            Ok(())
        })
    }

    /// `getItem(key, defaultValue)`.
    pub fn get_item(&self, key: &str, default_value: Dynamic) -> Dynamic {
        let res = self.storage.borrow().get(&JsString::from(key)).unwrap_or(Dynamic::Undefined);
        if is_undefined_or_null(&res) {
            return default_value;
        }

        res
    }

    /// `setItem(key, data)`.
    pub fn set_item(self: &Rc<Self>, key: &str, data: Dynamic) {
        self.set_items(&[(key.to_owned(), data)]);
    }

    /// `setItems(items)`.
    pub fn set_items(self: &Rc<Self>, items: &[(String, Dynamic)]) {
        let mut save = false;

        for (key, data) in items {
            let js_key = JsString::from(key.as_str());
            let current = self.storage.borrow().get(&js_key).unwrap_or(Dynamic::Undefined);

            // Shortcut for data that did not change
            if current.strict_equals(data) {
                continue;
            }

            // Remove items when they are undefined or null
            if is_undefined_or_null(data) {
                if !current.is_undefined() {
                    self.storage.borrow().set(&js_key, Dynamic::Undefined);
                    save = true;
                }
            }
            // Otherwise add an item
            else {
                self.storage.borrow().set(&js_key, data.clone());
                save = true;
            }
        }

        if save {
            let _ = self.save();
        }
    }

    /// `removeItem(key)`.
    pub fn remove_item(self: &Rc<Self>, key: &str) {
        // Only update if the key is actually present (not undefined)
        let js_key = JsString::from(key);
        if !self.storage.borrow().get(&js_key).unwrap_or(Dynamic::Undefined).is_undefined() {
            self.storage.borrow().set(&js_key, Dynamic::Undefined);
            let _ = self.save();
        }
    }

    fn save(self: &Rc<Self>) -> Promise<()> {
        if self.is_closing.get() {
            return Promise::resolved(()); // already about to close
        }

        let this = self.clone();
        self.flush_delayer.trigger(move |_| this.do_save(), None)
    }

    fn do_save(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let Some(initializing) = this.initializing.borrow().clone() else {
                return Ok(()); // if we never initialized, we should not save our state
            };

            // Make sure to wait for init to finish first
            initializing.await?;

            // Return early if the database has not changed
            let serialized_database = to_json(&Dynamic::Object(this.storage.borrow().clone())).map(|value| json_stringify(&value, Some("    "))).unwrap_or_default();
            if serialized_database == *this.last_saved_storage_contents.borrow() {
                return Ok(());
            }

            // Write to disk
            let options = WriteFileOptions { atomic: Some(FileAtomicOptions { postfix: ".vsctmp".to_owned() }), ..Default::default() };
            match this.file_service.write_file(&this.storage_path, VSBuffer::from_string(&serialized_database).into(), Some(options)).await {
                Ok(_) => *this.last_saved_storage_contents.borrow_mut() = serialized_database,
                Err(error) => this.log_service.error_args(&LogMessage::Text(error.to_string()), &[LogArg::Error(error)]),
            }
            Ok(())
        })
    }

    /// `close()`.
    pub fn close(self: &Rc<Self>) -> Promise<()> {
        if self.closing.borrow().is_none() {
            self.is_closing.set(true);
            let this = self.clone();
            // as soon as possible
            let promise = self.flush_delayer.trigger(move |_| this.do_save(), Some(0));
            *self.closing.borrow_mut() = Some(promise);
        }

        self.closing.borrow().clone().expect("definido acima")
    }
}

impl IDisposable for FileStorage {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `StateReadonlyService`.
pub struct StateReadonlyService {
    base: Rc<Disposable>,
    file_storage: Rc<FileStorage>,
}

impl StateReadonlyService {
    pub fn new(save_strategy: SaveStrategy, state_resource: URI, log_service: LogServiceRef, file_service: Rc<FileService>) -> Rc<Self> {
        let base = Disposable::new();
        let file_storage = FileStorage::new(state_resource, save_strategy, log_service, file_service);
        base.register(file_storage.clone());
        Rc::new(Self { base, file_storage })
    }

    /// `init()`.
    pub fn init(&self) -> Promise<()> {
        self.file_storage.init()
    }
}

impl StateReadService for StateReadonlyService {
    fn get_item(&self, key: &str, default_value: Dynamic) -> Dynamic {
        self.file_storage.get_item(key, default_value)
    }
}

impl IDisposable for StateReadonlyService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `StateService` do original (a classe que escreve).
pub struct StateServiceImpl {
    readonly: Rc<StateReadonlyService>,
}

impl StateServiceImpl {
    pub fn new(save_strategy: SaveStrategy, state_resource: URI, log_service: LogServiceRef, file_service: Rc<FileService>) -> Rc<Self> {
        Rc::new(Self { readonly: StateReadonlyService::new(save_strategy, state_resource, log_service, file_service) })
    }

    /// `init()`.
    pub fn init(&self) -> Promise<()> {
        self.readonly.init()
    }
}

impl StateReadService for StateServiceImpl {
    fn get_item(&self, key: &str, default_value: Dynamic) -> Dynamic {
        self.readonly.get_item(key, default_value)
    }
}

impl StateService for StateServiceImpl {
    fn set_item(&self, key: &str, data: Dynamic) {
        self.readonly.file_storage.set_item(key, data);
    }

    fn set_items(&self, items: &[(String, Dynamic)]) {
        self.readonly.file_storage.set_items(items);
    }

    fn remove_item(&self, key: &str) {
        self.readonly.file_storage.remove_item(key);
    }

    fn close(&self) -> Promise<()> {
        self.readonly.file_storage.close()
    }
}

impl IDisposable for StateServiceImpl {
    fn dispose(&self) {
        self.readonly.dispose();
    }
}
