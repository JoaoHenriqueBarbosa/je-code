//! Porte de `src/vs/workbench/api/browser/mainThreadSecretState.ts`: a ponte entre o `ISecretStorageService` do workbench e os
//! segredos que as extensões leem e gravam pelo `ExtensionContext.secrets`.
//!
//! Divergências:
//! - `@extHostNamedCustomer(MainContext.MainThreadSecretState)` e o `IExtHostContext` do construtor: o registro de clientes não
//!   existe ainda; o construtor recebe o [`IMainContext`] e pede o proxy de `ExtHostSecretState`. `extends Disposable` é o
//!   [`Disposable`] guardado no campo `base`.
//! - O `IBrowserWorkbenchEnvironmentService` do construtor original não é lido e por isso não é recebido.
//! - O `JSON.parse` de `parseKey` devolve qualquer valor; aqui só um objeto com `extensionId` e `key` em texto vira chave
//!   analisada. Qualquer outro valor (que no original nunca coincidiria com um `extensionId`) é lido como ausente.
//! - O `throw new Error(...)` de `doGetKeys` é a rejeição da promessa devolvida.

use std::rc::Rc;

use je_base::async_::{async_fn, SequencerByKey};
use je_base::errors::Error;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;
use je_platform::log::{LogService, Logger};
use je_platform::secrets::secrets::{ISecretStorageProvider, ISecretStorageService};

use crate::api::common::ext_host_protocol::{ExtHostContext, ExtHostSecretStateShape, IMainContext, MainThreadSecretStateShape};

/// O `{ extensionId, key }` do `JSON.parse(key)`.
struct ParsedKey {
    extension_id: String,
    key: String,
}

/// `parseKey(key)`.
fn parse_key(key: &str) -> Option<ParsedKey> {
    let value: serde_json::Value = serde_json::from_str(key).ok()?;
    let extension_id = value.get("extensionId")?.as_str()?.to_owned();
    let key = value.get("key")?.as_str()?.to_owned();
    Some(ParsedKey { extension_id, key })
}

/// `getKey(extensionId, key)`: `JSON.stringify({ extensionId, key })`.
pub fn secret_state_key(extension_id: &str, key: &str) -> String {
    serde_json::json!({ "extensionId": extension_id, "key": key }).to_string()
}

/// `MainThreadSecretState`.
pub struct MainThreadSecretState {
    base: Rc<Disposable>,
    sequencer: SequencerByKey<String>,
    secret_storage_service: Rc<dyn ISecretStorageService>,
    log_service: Rc<dyn LogService>,
}

impl MainThreadSecretState {
    pub fn new(ext_host_context: &dyn IMainContext, secret_storage_service: Rc<dyn ISecretStorageService>, log_service: Rc<dyn LogService>) -> Rc<Self> {
        let base = Disposable::new();
        let proxy: Rc<dyn ExtHostSecretStateShape> = ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_SECRET_STATE);

        base.register(secret_storage_service.on_did_change_secret().listen(move |e| {
            if let Some(parsed_key) = parse_key(&e) {
                proxy.rpc_on_did_change_password(&parsed_key.extension_id, &parsed_key.key);
            }
        }));

        Rc::new(Self { base, sequencer: SequencerByKey::default(), secret_storage_service, log_service })
    }
}

impl IDisposable for MainThreadSecretState {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl MainThreadSecretStateShape for MainThreadSecretState {
    fn rpc_get_password(&self, extension_id: &str, key: &str) -> Promise<Option<String>> {
        self.log_service.trace(&format!("[mainThreadSecretState] Getting password for {extension_id} extension:  {key}"));
        let (service, log, extension_id, key) = (self.secret_storage_service.clone(), self.log_service.clone(), extension_id.to_owned(), key.to_owned());
        self.sequencer.queue(extension_id.clone(), move || {
            async_fn(async move {
                let full_key = secret_state_key(&extension_id, &key);
                let password = service.get(&full_key).await?;
                let prefix = if password.is_some() { "P" } else { "No p" };
                log.trace(&format!("[mainThreadSecretState] {prefix}assword found for:  {extension_id} {key}"));
                Ok(password)
            })
        })
    }

    fn rpc_set_password(&self, extension_id: &str, key: &str, value: &str) -> Promise<()> {
        self.log_service.trace(&format!("[mainThreadSecretState] Setting password for {extension_id} extension:  {key}"));
        let (service, log, extension_id, key, value) = (self.secret_storage_service.clone(), self.log_service.clone(), extension_id.to_owned(), key.to_owned(), value.to_owned());
        self.sequencer.queue(extension_id.clone(), move || {
            async_fn(async move {
                let full_key = secret_state_key(&extension_id, &key);
                service.set(&full_key, &value).await?;
                log.trace(&format!("[mainThreadSecretState] Password set for:  {extension_id} {key}"));
                Ok(())
            })
        })
    }

    fn rpc_delete_password(&self, extension_id: &str, key: &str) -> Promise<()> {
        self.log_service.trace(&format!("[mainThreadSecretState] Deleting password for {extension_id} extension:  {key}"));
        let (service, log, extension_id, key) = (self.secret_storage_service.clone(), self.log_service.clone(), extension_id.to_owned(), key.to_owned());
        self.sequencer.queue(extension_id.clone(), move || {
            async_fn(async move {
                let full_key = secret_state_key(&extension_id, &key);
                service.delete(&full_key).await?;
                log.trace(&format!("[mainThreadSecretState] Password deleted for:  {extension_id} {key}"));
                Ok(())
            })
        })
    }

    fn rpc_get_keys(&self, extension_id: &str) -> Promise<Vec<String>> {
        self.log_service.trace(&format!("[mainThreadSecretState] Getting keys for {extension_id} extension: "));
        let (service, log, extension_id) = (self.secret_storage_service.clone(), self.log_service.clone(), extension_id.to_owned());
        self.sequencer.queue(extension_id.clone(), move || {
            async_fn(async move {
                let Some(all_keys) = service.keys() else {
                    return Err(Error::new("Secret storage service does not support keys() method"));
                };
                let keys: Vec<String> = all_keys
                    .await?
                    .iter()
                    .filter_map(|key| parse_key(key))
                    .filter(|parsed_key| parsed_key.extension_id == extension_id)
                    .map(|parsed_key| parsed_key.key) // Return only my keys
                    .collect();
                log.trace(&format!("[mainThreadSecretState] Got {}key(s) for:  {extension_id}", keys.len()));
                Ok(keys)
            })
        })
    }
}
