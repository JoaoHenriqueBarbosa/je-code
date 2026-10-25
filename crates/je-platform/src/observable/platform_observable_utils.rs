//! Porte parcial de `src/vs/platform/observable/common/platformObservableUtils.ts`: o `observableConfigValue` e o
//! `bindContextKey`. O `observableContextKey` não é lido por quem usa este módulo e entra quando houver quem o use.
//!
//! Divergências:
//! - `observableConfigValue<T>(key, defaultValue, configurationService)` lê `getValue<T>(key)`, e o `<T>` do TypeScript é só
//!   uma asserção de tipo; aqui o chamador diz como o valor dinâmico vira `T` (`from_dynamic`). O `?? defaultValue` vale quando
//!   o valor é `null`/`undefined` ou quando `from_dynamic` não reconhece o valor (um valor de outro tipo, que o TypeScript
//!   deixaria passar sem checagem).
//! - `bindContextKey<T extends ContextKeyValue>` não é genérico: a `RawContextKey` do porte guarda o valor como
//!   [`Dynamic`] (o `ContextKeyValue`), então `computeValue` devolve um [`Dynamic`]. O `derived` do original devolve o valor
//!   que acabou de escrever na chave; aqui ele devolve `()`, porque o único efeito do observável é escrever na chave (o
//!   `recomputeInitiallyAndOnChange` o recomputa a cada mudança de qualquer jeito).
//! - `debugName` e `DebugLocation` só dão nome ao observável e não entram.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableRef, DisposableStore};
use je_base::observable::base::{IObservable, IReader};
use je_base::observable::{DerivedOptions, StrictEquals, derived_opts, observable_from_event, recompute_initially_and_on_change};

use crate::configuration::service::IConfigurationService;
use crate::contextkey::{IContextKeyService, RawContextKey};

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

/// `bindContextKey(key, service, computeValue)`: atualiza a chave de contexto com um valor derivado de observáveis. O
/// descarte do resultado para de atualizar a chave.
pub fn bind_context_key(
    key: &RawContextKey,
    service: &dyn IContextKeyService,
    compute_value: impl Fn(&dyn IReader) -> Dynamic + 'static,
) -> DisposableRef {
    let bound_key = key.bind_to(service);
    let store = DisposableStore::new();
    let set_context_key = derived_opts(DerivedOptions::<()>::default(), move |reader| {
        bound_key.set(compute_value(reader));
    });
    store.add(recompute_initially_and_on_change(&set_context_key, None));
    store
}
