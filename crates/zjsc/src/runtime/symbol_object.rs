//! Porte de `runtime/SymbolObject.h`, `SymbolObjectInlines.h` e `SymbolObject.cpp`: o objeto `Symbol` (um
//! `JSWrapperObject` cujo valor interno é o `Symbol`).
//!
//! DIVERGÊNCIA: o `SymbolObject` não tem campos além do `JSWrapperObject`, então a célula é o próprio
//! `JSWrapperObject` (`js_wrapper_object.rs`, `CellEntry::WrapperObject`), distinguido como o
//! `inherits<SymbolObject>` do C++ pelo `ClassInfo` da `Structure` (`SYMBOL_OBJECT_S_INFO`). Como no
//! C++, o `JSType` é o `ObjectType` (não há `SymbolObjectType`).

use std::rc::Rc;

use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JS_NON_FINAL_OBJECT_S_INFO;
use crate::runtime::js_type::JSType;
use crate::runtime::js_value::JSValue;
use crate::runtime::js_wrapper_object::{JSWrapperObject, JSWrapperObjectRef};
use crate::runtime::structure::StructureRef;
use crate::runtime::symbol::{as_symbol, Symbol, SymbolRef};
use crate::runtime::vm::VM;

/// `const ClassInfo SymbolObject::s_info`.
pub static SYMBOL_OBJECT_S_INFO: ClassInfo =
    ClassInfo { class_name: "Symbol", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class SymbolObject final : public JSWrapperObject`.
pub struct SymbolObject;

impl SymbolObject {
    /// `createStructure(vm, globalObject, prototype)` (`SymbolObjectInlines.h`).
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        JSWrapperObject::create_structure(vm, global_object, prototype, JSType::ObjectType, &SYMBOL_OBJECT_S_INFO)
    }

    /// `create(vm, structure, symbol)`: `SymbolObject(vm, structure)` e `finishCreation(vm, symbol)`.
    pub fn create_with_symbol(vm: &VM, structure: StructureRef, symbol: &SymbolRef) -> JSWrapperObjectRef {
        let object = JSWrapperObject::create(vm, structure);
        object.set_internal_value(symbol.to_primitive());
        object
    }

    /// `create(vm, structure)`: o valor interno nasce como `Symbol::create(vm)` (o símbolo sem descrição).
    pub fn create(vm: &VM, structure: StructureRef) -> JSWrapperObjectRef {
        SymbolObject::create_with_symbol(vm, structure, &Symbol::create(vm))
    }

    /// `jsDynamicCast<SymbolObject*>(value)`.
    pub fn from_value(value: &JSValue) -> Option<JSWrapperObjectRef> {
        JSWrapperObject::from_value_of_type(value, JSType::ObjectType)
            .filter(|object| std::ptr::eq(object.structure().class_info(), &SYMBOL_OBJECT_S_INFO))
    }

    /// `internalValue()`: `asSymbol(JSWrapperObject::internalValue())`.
    pub fn internal_value(object: &JSWrapperObject) -> SymbolRef {
        as_symbol(object.internal_value())
    }
}

/// `Symbol::toObject(JSGlobalObject*)` (`Symbol.cpp`): `SymbolObject::create(vm, globalObject->symbolObjectStructure(), this)`.
pub fn symbol_to_object(global_object: &JSGlobalObject, symbol: &SymbolRef) -> JSWrapperObjectRef {
    SymbolObject::create_with_symbol(global_object.vm(), global_object.symbol_object_structure(), symbol)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_the_symbol_and_is_found_by_class_info() {
        let vm = VM::new();
        let structure = SymbolObject::create_structure(&vm, None, JSValue::null());
        let symbol = Symbol::create(&vm);
        let object = SymbolObject::create_with_symbol(&vm, structure, &symbol);
        assert!(Rc::ptr_eq(&SymbolObject::internal_value(&object), &symbol));
        let found = SymbolObject::from_value(&object.as_value()).unwrap();
        assert!(Rc::ptr_eq(&found, &object));
        assert!(SymbolObject::from_value(&symbol.to_primitive()).is_none());
    }
}
