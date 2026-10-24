//! Os originais que o `getIterationMode` (`IteratorOperations.cpp`) compara com o `JSGlobalObject`:
//! `%MapIteratorPrototype%.next`, `%SetIteratorPrototype%.next`, `%StringIteratorPrototype%.next`,
//! `%IteratorPrototype%[Symbol.iterator]` e `String.prototype[Symbol.iterator]`.
//!
//! DIVERGÊNCIAS: o C++ vigia esses pontos com `m_mapIteratorProtocolWatchpointSet`,
//! `m_setIteratorProtocolWatchpointSet` e `m_stringIteratorProtocolWatchpointSet` (um watchpoint por
//! propriedade, mais a ausência de `return` na cadeia). Sem watchpoint, a validade é a conferência direta
//! do `next` atual contra o valor original gravado na criação (como `array_iterator_protocol_is_intact`); o
//! `@@iterator` de cada protótipo é coberto pela identidade com o `symbolIterator` que o bytecode leu, e a
//! ausência de `return` não é conferida (o laço rápido nunca chama `return`, o `for-of` lê o `return` do
//! iterador pelo bytecode).

use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObjectRef;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_name::PropertyName;

/// Os valores originais, gravados por `init` (e por `string_prototype_natives` para o `@@iterator` de
/// `String.prototype`).
#[derive(Debug, Default)]
pub struct IterationProtocolOriginals {
    pub map_iterator_next: Option<JSValue>,
    pub set_iterator_next: Option<JSValue>,
    pub string_iterator_next: Option<JSValue>,
    pub string_iterator_prototype: Option<JSObjectRef>,
    pub iterator_proto_symbol_iterator_function: Option<JSValue>,
    pub string_proto_symbol_iterator_function: Option<JSValue>,
}

impl JSGlobalObject {
    /// `prototype.next` ainda é `original`.
    fn iterator_prototype_next_is(&self, prototype: &JSObjectRef, original: Option<JSValue>) -> bool {
        let Some(original) = original else { return false };
        let vm = self.vm();
        prototype.get_direct_by_name(vm, &PropertyName::from_identifier(&vm.property_names.next)) == original
    }

    /// `mapIteratorProtocolWatchpointSet().isStillValid()` (ver as DIVERGÊNCIAS do cabeçalho).
    pub fn map_iterator_protocol_is_intact(&self) -> bool {
        let original = self.iteration_protocol.borrow().map_iterator_next;
        self.iterator_prototype_next_is(&self.map_iterator_prototype(), original)
    }

    /// `setIteratorProtocolWatchpointSet().isStillValid()`.
    pub fn set_iterator_protocol_is_intact(&self) -> bool {
        let original = self.iteration_protocol.borrow().set_iterator_next;
        self.iterator_prototype_next_is(&self.set_iterator_prototype(), original)
    }

    /// `stringIteratorProtocolWatchpointSet().isStillValid()`.
    pub fn string_iterator_protocol_is_intact(&self) -> bool {
        let (original, prototype) = {
            let originals = self.iteration_protocol.borrow();
            (originals.string_iterator_next, originals.string_iterator_prototype.clone())
        };
        prototype.is_some_and(|prototype| self.iterator_prototype_next_is(&prototype, original))
    }

    /// `iteratorProtoSymbolIteratorFunctionConcurrently()`: a `JSFunction` original de
    /// `%IteratorPrototype%[Symbol.iterator]`.
    pub fn iterator_proto_symbol_iterator_function(&self) -> JSValue {
        self.iteration_protocol
            .borrow()
            .iterator_proto_symbol_iterator_function
            .expect("JSGlobalObject sem iteratorProtoSymbolIteratorFunction")
    }

    /// `stringProtoSymbolIteratorFunctionConcurrently()`.
    pub fn string_proto_symbol_iterator_function(&self) -> JSValue {
        self.iteration_protocol
            .borrow()
            .string_proto_symbol_iterator_function
            .expect("JSGlobalObject sem stringProtoSymbolIteratorFunction")
    }
}
