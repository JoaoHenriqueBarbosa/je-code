//! Porte de `runtime/WrapForValidIteratorPrototype.{h,cpp}` e `WrapForValidIteratorPrototypeInlines.h`: o
//! `%WrapForValidIteratorPrototype%`, um `JSNonFinalObject` com `next` e `return` (builtins JS,
//! `WrapForValidIteratorPrototype.js`) e `@@toStringTag`.

use crate::runtime::builtins_source::BuiltinCodeIndex;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_function::put_direct_builtin_function_without_transition;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo WrapForValidIteratorPrototype::s_info`.
pub static WRAP_FOR_VALID_ITERATOR_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Iterator", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class WrapForValidIteratorPrototype final : public JSNonFinalObject`.
pub struct WrapForValidIteratorPrototype;

impl WrapForValidIteratorPrototype {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)` (`WrapForValidIteratorPrototypeInlines.h`).
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, WrapForValidIteratorPrototype::STRUCTURE_FLAGS),
            &WRAP_FOR_VALID_ITERATOR_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `finishCreation(vm, globalObject)`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        prototype.finish_creation(vm);
        let builtins: [(&[u8], BuiltinCodeIndex); 2] = [
            (b"next", BuiltinCodeIndex::WrapForValidIteratorPrototypeNextCode),
            (b"return", BuiltinCodeIndex::WrapForValidIteratorPrototypeReturnCode),
        ];
        for (name, index) in builtins {
            put_direct_builtin_function_without_transition(vm, global_object, &prototype, &Identifier::from_span(vm, name), index, DONT_ENUM);
        }
        // `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`.
        put_to_string_tag(vm, &prototype, WRAP_FOR_VALID_ITERATOR_PROTOTYPE_S_INFO.class_name);
        prototype.structure().set_may_be_prototype(true);
        prototype
    }
}
