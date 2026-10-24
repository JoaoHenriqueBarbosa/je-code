//! Tradução de `runtime/JSAsyncFromSyncIterator.{h,cpp}` e `JSAsyncFromSyncIteratorInlines.h`: a célula
//! `JSAsyncFromSyncIterator` (`CellEntry::AsyncFromSyncIterator`), o iterador assíncrono que embrulha um
//! síncrono (https://tc39.es/ecma262/#sec-async-from-sync-iterator-objects).
//!
//! Fora desta fatia: `driveAsyncFromSyncIteratorWithDriver` e `asyncFromSyncIteratorNext` (dependem do
//! despacho de microtarefas e do `MicrotaskCallCache`).
//!
//! DIVERGÊNCIAS (heap ausente):
//! - O `CompactPointerTuple<JSObject*, IterationMode> m_syncIterator` vira dois campos (o `cell_id` do
//!   iterador e o `IterationMode`), e o `CompactPointerTuple<JSObject*, bool> m_target`, o `cell_id` do
//!   alvo e o `closeSyncIteratorOnRejection`.
//! - `WriteBarrier` vira `Cell`, e `visitChildren` e `vm.writeBarrier` somem.
//! - `USE(BUN_JSC_ADDITIONS)` vale (`target()`).

use std::cell::Cell;
use std::rc::Rc;

use crate::bytecode::op_metadata::IterationMode;
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectHandle};
use crate::runtime::js_value::{js_undefined, JSValue};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo JSAsyncFromSyncIterator::s_info`.
pub static JS_ASYNC_FROM_SYNC_ITERATOR_S_INFO: crate::runtime::class_info::ClassInfo = crate::runtime::class_info::ClassInfo {
    class_name: "AsyncFromSyncIterator",
    parent_class: Some(&crate::runtime::js_object::JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: None, inherits_js_type_range: None,
};

/// `class JSAsyncFromSyncIterator final : public JSNonFinalObject`.
pub struct JSAsyncFromSyncIterator {
    base: JSNonFinalObject,
    /// `m_syncIterator.pointer()`.
    sync_iterator: JSValue,
    /// `m_syncIterator.type()`.
    iteration_mode: IterationMode,
    /// `m_nextMethod`.
    next_method: JSValue,
    /// `m_target.pointer()`: o `cell_id` do alvo, `None` entre os passos.
    target: Cell<Option<usize>>,
    /// `m_target.type()`.
    close_sync_iterator_on_rejection: Cell<bool>,
    /// `m_cachedResult`.
    cached_result: Cell<JSValue>,
}

/// `JSAsyncFromSyncIterator*`.
pub type JSAsyncFromSyncIteratorRef = Rc<JSAsyncFromSyncIterator>;

impl std::ops::Deref for JSAsyncFromSyncIterator {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl JSAsyncFromSyncIterator {
    /// `visitChildren`: o iterador síncrono, o `next`, o alvo e o resultado guardado.
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        marker.values(&[self.sync_iterator, self.next_method, self.cached_result.get()]);
        if let Some(target) = self.target.get() {
            marker.cell(target);
        }
    }
    /// `create(vm, structure, syncIterator, nextMethod, iterationMode)`.
    pub fn create(
        vm: &VM,
        structure: &StructureRef,
        sync_iterator: &JSObject,
        next_method: JSValue,
        iteration_mode: IterationMode,
    ) -> JSAsyncFromSyncIteratorRef {
        let cell_id = cell_registry::reserve();
        let iterator = Rc::new(JSAsyncFromSyncIterator {
            base: JSNonFinalObject::new(vm, Rc::clone(structure)),
            sync_iterator: sync_iterator.as_value(),
            iteration_mode,
            next_method,
            target: Cell::new(None),
            close_sync_iterator_on_rejection: Cell::new(false),
            cached_result: Cell::new(js_undefined()),
        });
        iterator.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::AsyncFromSyncIterator(Rc::clone(&iterator)));
        iterator
    }

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(
        vm: &VM,
        global_object: Option<&crate::runtime::js_global_object::JSGlobalObject>,
        prototype: JSValue,
    ) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            crate::runtime::js_type_info::TypeInfo::new(
                crate::runtime::js_type::JSType::JSAsyncFromSyncIteratorType,
                JSNonFinalObject::STRUCTURE_FLAGS,
            ),
            &JS_ASYNC_FROM_SYNC_ITERATOR_S_INFO,
        )
    }

    /// Procura a célula pelo `cell_id` guardado em `JSValue::Cell`.
    pub fn from_cell_id(cell_id: usize) -> Option<JSAsyncFromSyncIteratorRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::AsyncFromSyncIterator(iterator)) => Some(iterator),
            _ => None,
        }
    }

    /// `dynamicDowncast<JSAsyncFromSyncIterator>(value)`.
    pub fn from_value(value: &JSValue) -> Option<JSAsyncFromSyncIteratorRef> {
        match value {
            JSValue::Cell(cell_id) => JSAsyncFromSyncIterator::from_cell_id(*cell_id),
            _ => None,
        }
    }

    /// `syncIterator()`.
    pub fn sync_iterator(&self) -> JSObjectHandle {
        JSObject::from_value(&self.sync_iterator).expect("o iterador síncrono é um objeto")
    }

    /// `nextMethod()`.
    pub fn next_method(&self) -> JSValue {
        self.next_method
    }

    /// `iterationMode()`.
    pub fn iteration_mode(&self) -> IterationMode {
        self.iteration_mode
    }

    /// `setTarget(vm, target, closeSyncIteratorOnRejection)`.
    pub fn set_target(&self, target: JSValue, close_sync_iterator_on_rejection: bool) {
        // O alvo é uma promessa ou o driver; o driver de um módulo com `await` de topo é o próprio
        // registro do módulo, que é uma célula mas não um `JSObject` do registro.
        self.target.set(Some(target.as_cell()));
        self.close_sync_iterator_on_rejection.set(close_sync_iterator_on_rejection);
    }

    /// `extractTarget()`: devolve o alvo e o bit, e zera o par (`m_target = { }`).
    pub fn extract_target(&self) -> (Option<JSValue>, bool) {
        let target = self.target.take().map(JSValue::from_cell);
        let close = self.close_sync_iterator_on_rejection.replace(false);
        (target, close)
    }

    /// `target()`: a promessa ou o driver que o passo pendente liquida ou retoma; `None` entre passos.
    pub fn target(&self) -> Option<JSValue> {
        self.target.get().map(JSValue::from_cell)
    }

    /// `cachedDriverResult()`.
    pub fn cached_driver_result(&self) -> JSValue {
        self.cached_result.get()
    }

    /// `setCachedDriverResult(vm, result)`.
    pub fn set_cached_driver_result(&self, result: &JSObject) {
        self.cached_result.set(result.as_value());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::js_type::JSType;
    use crate::runtime::js_value::js_null;

    #[test]
    fn target_round_trip() {
        let vm = VM::new();
        let structure = JSAsyncFromSyncIterator::create_structure(&vm, None, js_null());
        let plain = JSNonFinalObject::create_structure(&vm, None, js_null());
        let sync = JSObject::allocate(&vm, &plain);
        let iterator = JSAsyncFromSyncIterator::create(&vm, &structure, &sync, js_undefined(), IterationMode::AsyncFromSync);
        assert_eq!(cell_registry::cell_type(iterator.cell_id()), Some(JSType::JSAsyncFromSyncIteratorType));
        assert_eq!(iterator.target(), None);
        assert_eq!(iterator.cached_driver_result(), js_undefined());
        let target = JSObject::allocate(&vm, &plain);
        iterator.set_target(target.as_value(), true);
        assert_eq!(iterator.target(), Some(target.as_value()));
        assert_eq!(iterator.extract_target(), (Some(target.as_value()), true));
        assert_eq!(iterator.target(), None);
        assert_eq!(iterator.sync_iterator().cell_id(), sync.cell_id());
    }
}
