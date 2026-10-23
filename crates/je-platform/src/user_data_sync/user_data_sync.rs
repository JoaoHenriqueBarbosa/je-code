//! Porte do trait `IUserDataSyncUtilService` de `src/vs/platform/userDataSync/common/userDataSync.ts`.
//!
//! Divergências de tipo: `IStringDictionary<string>` é `HashMap<JsString, JsString>`; os métodos `async`
//! devolvem [`Promise`], que se espera com `.await`; `resolveFormattingOptions` recebe `Option<&URI>`
//! porque a implementação de teste do original declara o parâmetro como opcional (`file?: URI`).

use std::collections::HashMap;

use je_base::JsString;
use je_base::json_formatter::FormattingOptions;
use je_base::promise::Promise;
use je_base::uri::URI;

/// `IUserDataSyncUtilService`.
pub trait IUserDataSyncUtilService {
    /// Resolve cada atalho do usuário (`alt+c`, `cmd+k cmd+c`...) para a forma normalizada, que independe
    /// do layout do teclado.
    fn resolve_user_bindings(&self, userbindings: Vec<JsString>) -> Promise<HashMap<JsString, JsString>>;

    fn resolve_formatting_options(&self, resource: Option<&URI>) -> Promise<FormattingOptions>;

    fn resolve_default_core_ignored_settings(&self) -> Promise<Vec<JsString>>;
}
