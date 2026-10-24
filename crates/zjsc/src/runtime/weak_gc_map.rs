//! Tradução de `WeakGCMap<StringImpl*, JSString, PtrHash<StringImpl*>>`, o
//! `vm.atomStringToJSStringMap` (VM.h:723), na parte que o bytecompiler usa: `ensureValue`.
//!
//! Fora desta fatia, e por quê: `get`, `set`, `remove`, `contains`, `pruneStaleEntries` e o
//! registro no `Heap` (`WeakGCMap(VM&)` chama `vm.heap.registerWeakGCHashTable`), que só existem
//! com o GC.
//!
//! DIVERGÊNCIA (heap ausente, camada 3): o mapa é fraco no C++ (a entrada some quando a `JSString`
//! é coletada). Aqui nada é coletado, então as entradas são fortes e nunca morrem. A chave é a
//! identidade do `StringImpl` (`PtrHash`); a entrada guarda também o `Rc<StringImpl>` para que o
//! endereço usado como chave nunca seja reaproveitado por outra string.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::runtime::js_string::JSStringRef;
use crate::runtime::js_value::JSValue;
use crate::wtf::text::string_impl::StringImpl;

/// `WeakGCMap<StringImpl*, JSString, PtrHash<StringImpl*>>`.
#[derive(Debug, Default)]
pub struct AtomStringToJSStringMap {
    map: RefCell<HashMap<usize, (Rc<StringImpl>, JSStringRef)>>,
}

impl AtomStringToJSStringMap {
    /// `ensureValue(key, functor)`: devolve a `JSString` da chave, criando-a com `functor` quando
    /// não há. O C++ devolve `JSString*`, que os chamadores convertem em `JSValue`; o resultado já
    /// vem como `JSValue::from_js_string`, o único uso do bytecompiler.
    pub fn ensure_value(&self, key: &Rc<StringImpl>, functor: impl FnOnce() -> JSStringRef) -> JSValue {
        let identity = Rc::as_ptr(key) as usize;
        if let Some((_, existing)) = self.map.borrow().get(&identity) {
            return JSValue::from_js_string(Rc::clone(existing));
        }
        // O functor roda sem o empréstimo do mapa, como o C++ (ele pode alocar `JSString`).
        let value = functor();
        let mut map = self.map.borrow_mut();
        let (_, stored) = map.entry(identity).or_insert_with(|| (Rc::clone(key), value));
        JSValue::from_js_string(Rc::clone(stored))
    }
}
