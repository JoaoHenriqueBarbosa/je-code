//! Porte de `src/vs/platform/userDataSync/common/userDataSync.ts`, só o que o portado até agora usa: o trait
//! `IUserDataSyncUtilService`, `IConflictSetting`, `getDisallowedIgnoredSettings`, `IStorageValue` e
//! `SYNC_SERVICE_URL_TYPE`.
//!
//! Divergências de tipo: `IStringDictionary<string>` é `HashMap<JsString, JsString>`; os métodos `async`
//! devolvem [`Promise`], que se espera com `.await`; `resolveFormattingOptions` recebe `Option<&URI>`
//! porque a implementação de teste do original declara o parâmetro como opcional (`file?: URI`);
//! `any | undefined` (em `IConflictSetting`) é `Option<Value>`.

use std::collections::HashMap;

use je_base::JsString;
use je_base::dynamic::Truthy;
use je_base::json::Value;
use je_base::json_formatter::FormattingOptions;
use je_base::promise::Promise;
use je_base::uri::URI;

use crate::configuration::registry::configuration_registry;
use crate::storage::StorageScope;

/// `IStorageValue`.
#[derive(Clone, Debug, PartialEq)]
pub struct IStorageValue {
    pub version: f64,
    pub value: JsString,
    pub scope: Option<StorageScope>,
}

/// `SYNC_SERVICE_URL_TYPE`: chave sincronizada só na web.
pub const SYNC_SERVICE_URL_TYPE: &str = "sync.store.url.type";

/// `IUserDataSyncUtilService`.
pub trait IUserDataSyncUtilService {
    /// Resolve cada atalho do usuário (`alt+c`, `cmd+k cmd+c`...) para a forma normalizada, que independe
    /// do layout do teclado.
    fn resolve_user_bindings(&self, userbindings: Vec<JsString>) -> Promise<HashMap<JsString, JsString>>;

    fn resolve_formatting_options(&self, resource: Option<&URI>) -> Promise<FormattingOptions>;

    fn resolve_default_core_ignored_settings(&self) -> Promise<Vec<JsString>>;
}

/// `IConflictSetting`: um setting em conflito, com o valor de cada lado (`None` é `undefined`).
#[derive(Clone, Debug, PartialEq)]
pub struct IConflictSetting {
    pub key: JsString,
    pub local_value: Option<Value>,
    pub remote_value: Option<Value>,
}

/// `getDisallowedIgnoredSettings`: as chaves registradas que não podem ser ignoradas na sincronização
/// (`disallowSyncIgnore`).
pub fn get_disallowed_ignored_settings() -> Vec<JsString> {
    configuration_registry()
        .get_configuration_properties()
        .entries()
        .into_iter()
        .filter(|(_, property)| property.get("disallowSyncIgnore").is_truthy())
        .map(|(setting, _)| setting)
        .collect()
}
