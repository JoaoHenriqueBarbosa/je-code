//! Porte de `runtime/JSWeakObjectRef.{h,cpp}` e `JSWeakObjectRefInlines.h`: a célula de `WeakRef`
//! (`JSNonFinalObject` com o alvo, objeto ou símbolo não registrado).
//!
//! DIVERGÊNCIAS:
//! - Sem `writeBarrier` (o coletor para o mundo). O `visitChildren` e o `finalizeUnconditionally` são
//!   `visit_children` e `finalize`, que o coletor chama.
//! - O `TypeInfo` é `ObjectType` (o C++ não tem um `JSType` próprio para `WeakRef`), então o tipo da célula
//!   sai do `TypeInfo` da `Structure`, como em `CellEntry::Object`.

use std::cell::Cell;
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo JSWeakObjectRef::s_info`.
pub static JS_WEAK_OBJECT_REF_S_INFO: ClassInfo =
    ClassInfo { class_name: "WeakRef", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class JSWeakObjectRef final : public JSNonFinalObject`.
pub struct JSWeakObjectRef {
    base: JSNonFinalObject,
    /// `m_value`: o alvo.
    value: Cell<JSValue>,
    /// `m_lastAccessVersion`.
    last_access_version: Cell<u64>,
}

/// A referência à célula, o `*` do C++.
pub type JSWeakObjectRefRef = Rc<JSWeakObjectRef>;

impl std::ops::Deref for JSWeakObjectRef {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl JSWeakObjectRef {
    /// `createStructure(vm, globalObject, prototype)`: `TypeInfo(ObjectType, StructureFlags)`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::ObjectType, JSNonFinalObject::STRUCTURE_FLAGS),
            &JS_WEAK_OBJECT_REF_S_INFO,
        )
    }

    /// `create(vm, structure, target)`: o `finishCreation(vm, target)` grava o alvo.
    pub fn create(vm: &VM, structure: &StructureRef, target: JSValue) -> JSWeakObjectRefRef {
        let cell_id = cell_registry::reserve();
        let cell = Rc::new(JSWeakObjectRef {
            base: JSNonFinalObject::new(vm, Rc::clone(structure)),
            value: Cell::new(target),
            last_access_version: Cell::new(vm.current_weak_ref_version()),
        });
        cell.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::WeakObjectRef(Rc::clone(&cell)));
        cell
    }

    /// `dynamicDowncast<JSWeakObjectRef>(value)`.
    pub fn from_value(value: &JSValue) -> Option<JSWeakObjectRefRef> {
        match value {
            JSValue::Cell(cell_id) => match cell_registry::get(*cell_id) {
                Some(CellEntry::WeakObjectRef(cell)) => Some(cell),
                _ => None,
            },
            _ => None,
        }
    }

    /// `deref(vm)`: o alvo, ou `undefined` se foi coletado (`m_value` nulo). Ler segura o alvo até o fim da
    /// tarefa (`m_lastAccessVersion = vm.currentWeakRefVersion()`).
    pub fn deref_target(&self, vm: &VM) -> JSValue {
        let value = self.value.get();
        if value.is_empty() {
            JSValue::Undefined
        } else {
            self.last_access_version.set(vm.current_weak_ref_version());
            value
        }
    }

    /// `visitChildren`: o alvo só é forte se foi criado ou lido na versão corrente.
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        if self.last_access_version.get() == marker.weak_ref_version() {
            marker.value(self.value.get());
        }
    }

    /// `finalizeUnconditionally`: o alvo que a varredura vai soltar sai da referência.
    pub(crate) fn finalize(&self, marker: &crate::runtime::gc::Marker) {
        if let JSValue::Cell(cell_id) = self.value.get() {
            if marker.is_doomed(cell_id) {
                self.value.set(JSValue::empty());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deref_gives_the_target() {
        let vm = VM::new();
        let structure = JSWeakObjectRef::create_structure(&vm, None, JSValue::Null);
        let target = JSWeakObjectRef::create(&vm, &structure, JSValue::Int32(0));
        let reference = JSWeakObjectRef::create(&vm, &structure, target.as_value());
        assert_eq!(reference.deref_target(&vm), target.as_value());
        assert!(JSWeakObjectRef::from_value(&reference.as_value()).is_some());
        assert!(JSWeakObjectRef::from_value(&JSValue::Undefined).is_none());
    }
}
