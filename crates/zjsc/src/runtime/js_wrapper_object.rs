//! Porte de `runtime/JSWrapperObject.h` (e `JSWrapperObject.cpp`): a base dos objetos que embrulham um
//! valor primitivo (`Number`, `Boolean`; `String` tem o `StringObject` próprio, `Symbol` e `BigInt`
//! entram quando existirem), registrada como `CellEntry::WrapperObject`.
//!
//! DIVERGÊNCIAS (mesmo padrão de `string_object.rs`):
//!
//! - `JSInternalFieldObjectImpl<1>` (o campo interno `WrappedValue`) é um `Cell<JSValue>`;
//!   `offsetOfInternalField`, `internalValueOffset`, `allocationSize`, `subspaceFor` e `visitChildren`
//!   somem com o layout e o GC.
//! - Uma só struct serve a todas as subclasses sem campos próprios: quem a distingue é o `JSType` da
//!   `Structure` (`NumberObjectType`, `BooleanObjectType`), que `dynamicDowncast<NumberObject>` confere
//!   com o `ClassInfo`; `from_value_of_type` é esse downcast.

use std::cell::Cell;
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `class JSWrapperObject : public JSInternalFieldObjectImpl<1>`.
pub struct JSWrapperObject {
    base: JSNonFinalObject,
    /// `internalField(Field::WrappedValue)`.
    internal_value: Cell<JSValue>,
}

/// O `JSWrapperObject*`.
pub type JSWrapperObjectRef = Rc<JSWrapperObject>;

impl std::fmt::Debug for JSWrapperObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JSWrapperObject")
            .field("cell_id", &self.base.cell_id())
            .field("internal_value", &self.internal_value.get())
            .finish()
    }
}

impl std::ops::Deref for JSWrapperObject {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl JSWrapperObject {
    /// `visitChildren`: o valor embrulhado.
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        marker.value(self.internal_value.get());
    }
    /// `StructureFlags = Base::StructureFlags` (nenhuma flag): é a de `NumberObject` e `BooleanObject`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `NumberObject::createStructure`, `BooleanObject::createStructure`: `Structure::create(vm,
    /// globalObject, prototype, TypeInfo(type, StructureFlags), info())`.
    pub fn create_structure(
        vm: &VM,
        global_object: Option<&JSGlobalObject>,
        prototype: JSValue,
        js_type: JSType,
        class_info: &'static ClassInfo,
    ) -> StructureRef {
        Structure::create(vm, global_object, prototype, TypeInfo::new(js_type, JSWrapperObject::STRUCTURE_FLAGS), class_info)
    }
    /// `JSWrapperObject(vm, structure)` mais o registro da célula: o campo interno nasce `undefined`
    /// (`JSInternalFieldObjectImpl` inicializa os campos com `jsUndefined()`).
    pub fn create(vm: &VM, structure: StructureRef) -> JSWrapperObjectRef {
        let cell_id = cell_registry::reserve();
        let object = Rc::new(JSWrapperObject { base: JSNonFinalObject::new(vm, structure), internal_value: Cell::new(JSValue::Undefined) });
        object.base.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::WrapperObject(Rc::clone(&object)));
        object
    }

    /// Procura a célula pelo `cell_id` guardado em `JSValue::Cell`.
    pub fn from_cell_id(cell_id: usize) -> Option<JSWrapperObjectRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::WrapperObject(object)) => Some(object),
            _ => None,
        }
    }

    /// `dynamicDowncast<NumberObject>(value)` e `dynamicDowncast<BooleanObject>(value)`: o valor é uma
    /// célula embrulhadora cujo `JSType` é `js_type`.
    pub fn from_value_of_type(value: &JSValue, js_type: JSType) -> Option<JSWrapperObjectRef> {
        let JSValue::Cell(cell_id) = value else {
            return None;
        };
        JSWrapperObject::from_cell_id(*cell_id).filter(|object| object.type_() == js_type)
    }

    /// `JSValue(JSCell*)`.
    pub fn as_value(&self) -> JSValue {
        JSValue::from_cell(self.base.cell_id())
    }

    /// `internalValue()`.
    pub fn internal_value(&self) -> JSValue {
        self.internal_value.get()
    }

    /// `setInternalValue(vm, value)`.
    pub fn set_internal_value(&self, value: JSValue) {
        debug_assert!(!value.is_empty());
        debug_assert!(JSObject::from_value(&value).is_none());
        self.internal_value.set(value);
    }
}
