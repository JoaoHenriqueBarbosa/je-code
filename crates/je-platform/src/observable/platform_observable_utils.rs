//! Porte parcial de `src/vs/platform/observable/common/platformObservableUtils.ts`: só o `observableConfigValue`. O
//! `bindContextKey` e o `observableContextKey` não são lidos por quem usa este módulo e entram quando houver quem os use.
//!
//! Divergências:
//! - `observableConfigValue<T>(key, defaultValue, configurationService)` lê `getValue<T>(key)`, e o `<T>` do TypeScript é só
//!   uma asserção de tipo; aqui o chamador diz como o valor dinâmico vira `T` (`from_dynamic`). O `?? defaultValue` vale quando
//!   o valor é `null`/`undefined` ou quando `from_dynamic` não reconhece o valor (um valor de outro tipo, que o TypeScript
//!   deixaria passar sem checagem).
//! - `debugName` e `DebugLocation` só dão nome ao observável e não entram.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::observable::base::IObservable;
use je_base::observable::{StrictEquals, observable_from_event};

use crate::configuration::service::IConfigurationService;

/// `observableConfigValue(key, defaultValue, configurationService)`: um observável que muda quando a chave de configuração muda.
pub fn observable_config_value<T: Clone + StrictEquals + 'static>(
    key: &str,
    default_value: T,
    configuration_service: Rc<dyn IConfigurationService>,
    from_dynamic: impl Fn(&Dynamic) -> Option<T> + 'static,
) -> IObservable<T> {
    let watched_key = key.to_string();
    let event = configuration_service.on_did_change_configuration().filter(move |e| e.affects_configuration(watched_key.as_str(), None), None);
    let key = key.to_string();
    observable_from_event(event, move |_| {
        let value = configuration_service.get_value(Some(&key), None);
        if value.is_nullish() { default_value.clone() } else { from_dynamic(&value).unwrap_or_else(|| default_value.clone()) }
    })
}
