//! Porte de `src/vs/base/common/hotReloadHelpers.ts`: ligar observáveis à recarga a quente.
//!
//! `createHotClass` usa `clazz.name` como identidade da classe; em Rust não há essa propriedade e o nome é
//! um argumento.

use std::cell::RefCell;
use std::rc::Rc;

use crate::dynamic::Dynamic;
use crate::event::Event;
use crate::hot_reload::{AcceptNewExportsHandler, HotReloadArgs, is_hot_reload_enabled, register_hot_reload_handler};
use crate::observable::base::{IObservable, IReader, ISettableObservable, SettableExt};
use crate::observable::deps::StrictEquals;
use crate::observable::observables::const_observable::const_observable;
use crate::observable::observables::observable_signal_from_event::observable_signal_from_event;
use crate::observable::observables::observable_value::observable_value;

/// `readHotReloadableExport`.
pub fn read_hot_reloadable_export<T>(value: T, reader: Option<&dyn IReader>) -> T
where
    T: Clone + Into<Dynamic>,
{
    observe_hot_reloadable_exports(&[value.clone().into()], reader);
    value
}

/// `observeHotReloadableExports`: relê o observável quando um módulo que exporta algum dos `values` é
/// recarregado.
pub fn observe_hot_reloadable_exports(values: &[Dynamic], reader: Option<&dyn IReader>) {
    if is_hot_reload_enabled() {
        let values: Vec<Dynamic> = values.to_vec();
        let o = observable_signal_from_event(
            "reload",
            Event::<()>::new(move |listener| {
                let values = values.clone();
                register_hot_reload_handler(Rc::new(move |args: &HotReloadArgs| -> Option<AcceptNewExportsHandler> {
                    if !args.old_exports.values().any(|v| values.iter().any(|x| x.strict_equals(v))) {
                        return None;
                    }
                    let listener = listener.clone();
                    Some(Rc::new(move |_new_exports| {
                        listener(&());
                        true
                    }))
                }))
            }),
        );
        o.read_opt(reader);
    }
}

thread_local! {
    static CLASSES: RefCell<std::collections::HashMap<String, Rc<dyn std::any::Any>>> = RefCell::new(std::collections::HashMap::new());
}

/// `createHotClass`: um observável da classe, que muda quando a classe de mesmo nome é recarregada.
pub fn create_hot_class<T: Clone + StrictEquals + 'static>(clazz: T, id: &str) -> IObservable<T> {
    if !is_hot_reload_enabled() {
        return const_observable(clazz);
    }

    let existing = CLASSES.with(|c| c.borrow().get(id).cloned());
    match existing.and_then(|e| e.downcast::<ISettableObservable<T>>().ok()) {
        Some(existing) => {
            let target = (*existing).clone();
            crate::runtime::set_timeout(move || target.set(clazz, None), 0);
            let observable: IObservable<T> = (*existing).clone();
            observable
        }
        None => {
            let created: ISettableObservable<T> = observable_value::<T, ()>(id, clazz);
            CLASSES.with(|c| c.borrow_mut().insert(id.to_owned(), Rc::new(created.clone())));
            let observable: IObservable<T> = created;
            observable
        }
    }
}
