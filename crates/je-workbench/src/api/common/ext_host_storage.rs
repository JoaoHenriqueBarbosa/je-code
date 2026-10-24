//! Porte de `src/vs/workbench/api/common/extHostStorage.ts` e `extHostSecretState.ts`: o lado da extensão do armazenamento de estado
//! (`globalState`, `workspaceState`) e dos segredos (`SecretStorage`), sobre os proxies `MainThreadStorage` e `MainThreadSecretState`.
//!
//! Divergências:
//! - O valor do armazenamento (`object`) é um [`Dynamic`]; o `JSON.parse` de `safeParseValue` é o `json_parse` do `je-base`
//!   (`Dynamic::from_json` sobre o valor analisado), e um valor que não analisa só registra o erro, como o original.
//! - `IExtHostRpcService` é o [`IMainContext`]; `ILogService` é o [`LogServiceRef`].
//! - `parsedValue || defaultValue` (falso para `null`, `0`, `''`, `false`): só `undefined` e `null` caem no padrão aqui, porque o
//!   valor que o workbench guarda é sempre um objeto serializado (`object`), e um objeto é sempre verdadeiro.
//! - `async $onDidChangePassword` é uma promessa já resolvida.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::promise::Promise;
use je_platform::log::LogServiceRef;

use super::ext_host_protocol::{
    ExtHostSecretStateShape, ExtHostStorageShape, IExtensionIdWithVersion, IMainContext, MainContext, MainThreadSecretStateShape, MainThreadStorageShape,
};

/// `IStorageChangeEvent`.
#[derive(Clone, Debug)]
pub struct IStorageChangeEvent {
    pub shared: bool,
    pub key: String,
    pub value: Dynamic,
}

/// `ExtHostStorage`.
pub struct ExtHostStorage {
    proxy: Rc<dyn MainThreadStorageShape>,
    log_service: LogServiceRef,
    on_did_change_storage: Rc<Emitter<IStorageChangeEvent>>,
}

impl ExtHostStorage {
    pub fn new(main_context: &dyn IMainContext, log_service: LogServiceRef) -> Rc<Self> {
        Rc::new(Self { proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_STORAGE), log_service, on_did_change_storage: Emitter::new() })
    }

    /// `onDidChangeStorage`.
    pub fn on_did_change_storage(&self) -> Event<IStorageChangeEvent> {
        self.on_did_change_storage.event()
    }

    pub fn register_extension_storage_keys_to_sync(&self, extension: IExtensionIdWithVersion, keys: Vec<String>) {
        self.proxy.rpc_register_extension_storage_keys_to_sync(extension, keys);
    }

    /// `initializeExtensionStorage(shared, key, defaultValue?)`.
    pub fn initialize_extension_storage(self: &Rc<Self>, shared: bool, key: &str, default_value: Option<Dynamic>) -> Promise<Option<Dynamic>> {
        let (this, key) = (self.clone(), key.to_owned());
        self.proxy.rpc_initialize_extension_storage(shared, &key).then(move |value| {
            let parsed_value = value.filter(|value| !value.is_empty()).and_then(|value| this.safe_parse_value(shared, &key, &value));
            parsed_value.or(default_value)
        })
    }

    pub fn set_value(&self, shared: bool, key: &str, value: Dynamic) -> Promise<()> {
        self.proxy.rpc_set_value(shared, key, value)
    }

    fn safe_parse_value(&self, shared: bool, key: &str, value: &str) -> Option<Dynamic> {
        let units: Vec<u16> = value.encode_utf16().collect();
        match je_base::jsonc::json_parse(&units) {
            Ok(parsed) => Some(Dynamic::from_json(&parsed)),
            Err(error) => {
                // Do not fail this call but log it for diagnostics
                // https://github.com/microsoft/vscode/issues/132777
                self.log_service.error(&format!("[extHostStorage] unexpected error parsing storage contents (extensionId: {key}, global: {shared}): {}", error.message));
                None
            }
        }
    }
}

impl ExtHostStorageShape for ExtHostStorage {
    fn rpc_accept_value(&self, shared: bool, extension_id: &str, value: &str) {
        if let Some(parsed_value) = self.safe_parse_value(shared, extension_id, value) {
            self.on_did_change_storage.fire(IStorageChangeEvent { shared, key: extension_id.to_owned(), value: parsed_value });
        }
    }
}

/// `ExtHostSecretState`.
pub struct ExtHostSecretState {
    proxy: Rc<dyn MainThreadSecretStateShape>,
    on_did_change_password: Rc<Emitter<(String, String)>>,
}

impl ExtHostSecretState {
    pub fn new(main_context: &dyn IMainContext) -> Rc<Self> {
        Rc::new(Self { proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_SECRET_STATE), on_did_change_password: Emitter::new() })
    }

    /// `onDidChangePassword`: `(extensionId, key)`.
    pub fn on_did_change_password(&self) -> Event<(String, String)> {
        self.on_did_change_password.event()
    }

    pub fn get(&self, extension_id: &str, key: &str) -> Promise<Option<String>> {
        self.proxy.rpc_get_password(extension_id, key)
    }

    pub fn store(&self, extension_id: &str, key: &str, value: &str) -> Promise<()> {
        self.proxy.rpc_set_password(extension_id, key, value)
    }

    pub fn delete(&self, extension_id: &str, key: &str) -> Promise<()> {
        self.proxy.rpc_delete_password(extension_id, key)
    }

    pub fn keys(&self, extension_id: &str) -> Promise<Vec<String>> {
        self.proxy.rpc_get_keys(extension_id)
    }
}

impl ExtHostSecretStateShape for ExtHostSecretState {
    fn rpc_on_did_change_password(&self, extension_id: &str, key: &str) -> Promise<()> {
        self.on_did_change_password.fire((extension_id.to_owned(), key.to_owned()));
        Promise::resolved(())
    }
}
