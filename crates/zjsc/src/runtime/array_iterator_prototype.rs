//! Porte de `runtime/ArrayIteratorPrototype.{h,cpp}` e `ArrayIteratorPrototypeInlines.h`: o
//! `%ArrayIteratorPrototype%` (um `JSNonFinalObject` comum, com `next` e `@@toStringTag`).
//!
//! `next` é o builtin JS `arrayIteratorPrototypeNextCodeGenerator` (`builtins/ArrayIteratorPrototype.js`),
//! registrado por `put_direct_builtin_function_without_transition` como no C++. Ele depende dos
//! intrínsecos `@isArrayIterator`, `@getArrayIteratorInternalField` e `@putArrayIteratorInternalField`
//! do gerador de bytecode (campos de `js_array_iterator::Field`) e do `@arrayIteratorNextHelper`
//! (`ArrayIteratorPrototypeArrayIteratorNextHelperCode`, `LinkTimeConstant` do global).
//!
//! `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()` é a propriedade `@@toStringTag` com o `className` do
//! `ClassInfo` (`DontEnum | ReadOnly`), como em `symbol_prototype.rs`.

use crate::runtime::builtins_source::BuiltinCodeIndex;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_function::put_direct_builtin_function_without_transition;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::js_string;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_attribute::{DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo ArrayIteratorPrototype::s_info`.
pub static ARRAY_ITERATOR_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Array Iterator", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class ArrayIteratorPrototype final : public JSNonFinalObject`.
pub struct ArrayIteratorPrototype;

impl ArrayIteratorPrototype {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)` (`ArrayIteratorPrototypeInlines.h`).
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, ArrayIteratorPrototype::STRUCTURE_FLAGS),
            &ARRAY_ITERATOR_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `ArrayIteratorPrototype(vm, structure)` e `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        ArrayIteratorPrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm, globalObject)`.
    fn finish_creation(prototype: &JSObject, vm: &VM, global_object: &JSGlobalObject) {
        prototype.finish_creation(vm);
        put_direct_builtin_function_without_transition(
            vm,
            global_object,
            prototype,
            &vm.property_names.next,
            BuiltinCodeIndex::ArrayIteratorPrototypeNextCode,
            DONT_ENUM,
        );
        // `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`.
        prototype.put_direct(
            vm,
            &PropertyName::from_identifier(&vm.property_names.to_string_tag_symbol),
            JSValue::from_js_string(js_string(vm, &WtfString::from_latin1(ARRAY_ITERATOR_PROTOTYPE_S_INFO.class_name.as_bytes()))),
            DONT_ENUM | READ_ONLY,
        );
        prototype.structure().set_may_be_prototype(true);
    }
}
