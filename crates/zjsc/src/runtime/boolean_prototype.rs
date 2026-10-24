//! Porte de `runtime/BooleanPrototype.h` e `BooleanPrototype.cpp`: o `Boolean.prototype` (um
//! `BooleanObject` com valor interno `false`) com `toString` e `valueOf`.
//!
//! `booleanPrototypeTable` (`toString`, `valueOf`, `DontEnum|Function`, comprimento 0) fica no `ClassInfo`,
//! e a `Structure` leva `HasStaticPropertyTable`: as duas funções são reificadas no primeiro acesso.
//!
//! DIVERGÊNCIA: `vm.smallStrings.falseString()` e `trueString()` criam a `JSString` a cada
//! chamada (o cache não tem efeito observável).

use crate::runtime::boolean_object::{BooleanObject, BOOLEAN_OBJECT_S_INFO};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_string::js_string;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::js_value::{js_boolean, JSValue};
use crate::runtime::js_wrapper_object::{JSWrapperObject, JSWrapperObjectRef};
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::lookup::{native_entry};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{DONT_ENUM, FUNCTION};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo BooleanPrototype::s_info`.
pub static BOOLEAN_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo {
        class_name: "Boolean",
        parent_class: Some(&BOOLEAN_OBJECT_S_INFO),
        static_prop_hash_table: Some(&BOOLEAN_PROTOTYPE_TABLE),
        inherits_js_type_range: None,
    };

/// `booleanPrototypeTableValues` de `BooleanPrototype.lut.h`, na ordem do `@begin`.
static BOOLEAN_PROTOTYPE_TABLE_VALUES: [HashTableValue; 2] = [
    native_entry("toString", boolean_proto_host_to_string, 0),
    native_entry("valueOf", boolean_proto_host_value_of, 0),
];

/// `booleanPrototypeTable`.
static BOOLEAN_PROTOTYPE_TABLE: HashTable = HashTable { class_for_this: None, values: &BOOLEAN_PROTOTYPE_TABLE_VALUES };

/// `throwVMTypeError(globalObject, scope)`: a mensagem padrão do `Error.h`.
const DEFAULT_TYPE_ERROR_MESSAGE: &str = "Type error";

/// `thisValue` como booleano: o primitivo, ou o valor interno do `BooleanObject`.
fn this_boolean_value(call: &HostCall) -> Result<JSValue, Thrown> {
    let this_value = call.this_value();
    if this_value.is_boolean() {
        return Ok(this_value);
    }

    match BooleanObject::from_value(&this_value) {
        Some(this_object) => Ok(this_object.internal_value()),
        None => Err(Thrown::type_error(DEFAULT_TYPE_ERROR_MESSAGE)),
    }
}

fn boolean_proto_func_to_string(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let boolean = this_boolean_value(call)?;
    debug_assert!(boolean.is_boolean());
    let text: &[u8] = if boolean.is_true() { b"true" } else { b"false" };
    Ok(JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_latin1(text))))
}

fn boolean_proto_func_value_of(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let boolean = this_boolean_value(call)?;
    Ok(js_boolean(boolean.is_true()))
}

crate::host_function!(boolean_proto_host_to_string, boolean_proto_func_to_string);
crate::host_function!(boolean_proto_host_value_of, boolean_proto_func_value_of);

/// `class BooleanPrototype final : public BooleanObject`: espaço de nomes de `createStructure` e `create`.
pub struct BooleanPrototype;

impl BooleanPrototype {
    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::BooleanObjectType, BooleanPrototype::STRUCTURE_FLAGS),
            &BOOLEAN_PROTOTYPE_S_INFO,
        )
    }

    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
    pub const STRUCTURE_FLAGS: u32 = JSWrapperObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE;

    /// `create(vm, globalObject, structure)`: o `BooleanPrototype(vm, structure)` e o `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: StructureRef) -> JSWrapperObjectRef {
        let prototype = JSWrapperObject::create(vm, structure);
        BooleanPrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm, globalObject)`.
    fn finish_creation(prototype: &JSWrapperObject, vm: &VM, _global_object: &JSGlobalObject) {
        prototype.finish_creation(vm);
        prototype.set_internal_value(js_boolean(false));
        // `toString` e `valueOf` (`booleanPrototypeTable`) não nascem aqui: reificam no primeiro acesso.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitive_this_values_are_accepted() {
        let call = HostCall::new(JSValue::Bool(true), Vec::new());
        assert_eq!(this_boolean_value(&call), Ok(JSValue::Bool(true)));
        let call = HostCall::new(JSValue::Bool(false), Vec::new());
        assert_eq!(this_boolean_value(&call), Ok(JSValue::Bool(false)));
    }

    #[test]
    fn other_this_values_throw_the_default_type_error() {
        for this_value in [JSValue::Undefined, JSValue::Null, JSValue::Int32(1), JSValue::Double(0.5)] {
            let call = HostCall::new(this_value, Vec::new());
            assert_eq!(this_boolean_value(&call), Err(Thrown::type_error("Type error")));
        }
    }
}
