//! Porte de `runtime/JSPromiseCombinatorsGlobalContext.{h,cpp}`, `JSPromiseCombinatorsContext.{h,cpp}` e
//! `JSPromiseCombinatorsContextInlines.h`: as duas células de `JSCell` puro que `Promise.all`,
//! `allSettled` e `any` usam para contar os elementos pendentes. A global guarda a promessa de
//! resultado (ou a função `resolve`, no caminho lento do `allSettled`), o array de valores e a contagem
//! de elementos restantes; a por elemento aponta para a global e leva o índice.
//!
//! DIVERGÊNCIAS: sem GC, `visitChildren` e as barreiras somem. As células não têm `Structure` (o
//! `vm.promiseCombinatorsContextStructure` do C++ só dá o `JSType` e o `ClassInfo`, que o
//! `CellEntry::js_type` devolve).

use std::cell::Cell;
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_function_support::throw_put_error;
use crate::runtime::js_array::JSArray;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_promise_host::{caught_exception, Thrown};
use crate::runtime::js_value::JSValue;
use crate::runtime::sparse_array_value_map::PutDirectIndexMode;

/// `const ClassInfo JSPromiseCombinatorsGlobalContext::s_info`.
pub static JS_PROMISE_COMBINATORS_GLOBAL_CONTEXT_S_INFO: ClassInfo =
    ClassInfo { class_name: "PromiseCombinatorsGlobalContext", parent_class: None, static_prop_hash_table: None, inherits_js_type_range: None };

/// `const ClassInfo JSPromiseCombinatorsContext::s_info`.
pub static JS_PROMISE_COMBINATORS_CONTEXT_S_INFO: ClassInfo =
    ClassInfo { class_name: "PromiseCombinatorsContext", parent_class: None, static_prop_hash_table: None, inherits_js_type_range: None };

/// `class JSPromiseCombinatorsGlobalContext final : public JSCell`.
#[derive(Debug)]
pub struct JSPromiseCombinatorsGlobalContext {
    cell_id: Cell<usize>,
    /// `m_promise`.
    promise: Cell<JSValue>,
    /// `m_values`.
    values: Cell<JSValue>,
    /// `m_remainingElementsCount`.
    remaining_elements_count: Cell<u64>,
}

/// `JSPromiseCombinatorsGlobalContext*`.
pub type JSPromiseCombinatorsGlobalContextRef = Rc<JSPromiseCombinatorsGlobalContext>;

impl JSPromiseCombinatorsGlobalContext {
    /// `visitChildren`: a promessa e o array de valores.
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        marker.values(&[self.promise.get(), self.values.get()]);
    }
    /// `create(vm, promise, values, remainingElementsCount)`.
    pub fn create(promise: JSValue, values: JSValue, remaining_elements_count: u64) -> JSPromiseCombinatorsGlobalContextRef {
        let cell_id = cell_registry::reserve();
        let context = Rc::new(JSPromiseCombinatorsGlobalContext {
            cell_id: Cell::new(cell_id),
            promise: Cell::new(promise),
            values: Cell::new(values),
            remaining_elements_count: Cell::new(remaining_elements_count),
        });
        cell_registry::set(cell_id, CellEntry::PromiseCombinatorsGlobalContext(Rc::clone(&context)));
        context
    }

    /// `uncheckedDowncast`/`dynamicDowncast<JSPromiseCombinatorsGlobalContext>(JSValue)`.
    pub fn from_value(value: &JSValue) -> Option<JSPromiseCombinatorsGlobalContextRef> {
        let JSValue::Cell(cell_id) = value else { return None };
        match cell_registry::get(*cell_id) {
            Some(CellEntry::PromiseCombinatorsGlobalContext(context)) => Some(context),
            _ => None,
        }
    }

    /// O "endereço" da célula, o que o `JSValue` codifica.
    pub fn cell_id(&self) -> usize {
        self.cell_id.get()
    }

    /// `JSValue(JSCell*)`.
    pub fn as_value(&self) -> JSValue {
        JSValue::from_cell(self.cell_id())
    }

    /// `promise()`.
    pub fn promise(&self) -> JSValue {
        self.promise.get()
    }

    /// `values()`.
    pub fn values(&self) -> JSValue {
        self.values.get()
    }

    /// `remainingElementsCount()`.
    pub fn remaining_elements_count(&self) -> u64 {
        self.remaining_elements_count.get()
    }

    /// `setRemainingElementsCount(count)`.
    pub fn set_remaining_elements_count(&self, count: u64) {
        self.remaining_elements_count.set(count);
    }

    /// `uncheckedDowncast<JSArray>(values())->putDirectIndex(globalObject, index, value)`: define o
    /// índice como propriedade própria (`PutDirectIndexLikePutDirect`, atributos 0), sem passar por
    /// setters indexados de `Array.prototype` ou `Object.prototype`. A exceção (índice acima do vetor
    /// sem espaço, por exemplo) volta como `Thrown`.
    pub fn put_direct_index(&self, global_object: &JSGlobalObject, index: u64, value: JSValue) -> Result<(), Thrown> {
        let array = JSArray::from_value(&self.values()).expect("uncheckedDowncast<JSArray>(values) em valor que não é array");
        let index = u32::try_from(index).expect("Promise.all com mais de 2^32 elementos");
        match array.object().put_direct_index(global_object.vm(), index, value, 0, PutDirectIndexMode::PutDirectIndexLikePutDirect) {
            Ok(_) => Ok(()),
            Err(error) => {
                throw_put_error(global_object, error);
                Err(caught_exception(global_object))
            }
        }
    }
}

/// `class JSPromiseCombinatorsContext final : public JSCell`.
#[derive(Debug)]
pub struct JSPromiseCombinatorsContext {
    cell_id: Cell<usize>,
    /// `m_globalContext`.
    global_context: JSPromiseCombinatorsGlobalContextRef,
    /// `m_index`.
    index: u64,
}

/// `JSPromiseCombinatorsContext*`.
pub type JSPromiseCombinatorsContextRef = Rc<JSPromiseCombinatorsContext>;

impl JSPromiseCombinatorsContext {
    /// `create(vm, globalContext, index)`.
    pub fn create(global_context: &JSPromiseCombinatorsGlobalContextRef, index: u64) -> JSPromiseCombinatorsContextRef {
        let cell_id = cell_registry::reserve();
        let context = Rc::new(JSPromiseCombinatorsContext {
            cell_id: Cell::new(cell_id),
            global_context: Rc::clone(global_context),
            index,
        });
        cell_registry::set(cell_id, CellEntry::PromiseCombinatorsContext(Rc::clone(&context)));
        context
    }

    /// `dynamicDowncast<JSPromiseCombinatorsContext>(JSValue)`.
    pub fn from_value(value: &JSValue) -> Option<JSPromiseCombinatorsContextRef> {
        let JSValue::Cell(cell_id) = value else { return None };
        match cell_registry::get(*cell_id) {
            Some(CellEntry::PromiseCombinatorsContext(context)) => Some(context),
            _ => None,
        }
    }

    /// `JSValue(JSCell*)`.
    pub fn as_value(&self) -> JSValue {
        JSValue::from_cell(self.cell_id.get())
    }

    /// `globalContext()`.
    pub fn global_context(&self) -> &JSPromiseCombinatorsGlobalContextRef {
        &self.global_context
    }

    /// `index()`.
    pub fn index(&self) -> u64 {
        self.index
    }
}
