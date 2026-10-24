//! Porte de `runtime/StringIteratorPrototype.{h,cpp}` e `StringIteratorPrototypeInlines.h`: o
//! `%StringIteratorPrototype%` (um `JSNonFinalObject` comum, com `next` nativo e `@@toStringTag`).

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::put_direct_native_function_without_transition;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::js_string;
use crate::runtime::js_string_iterator::JSStringIterator;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_attribute::{DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::string_regexp_support::create_iterator_result_object;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo StringIteratorPrototype::s_info`.
pub static STRING_ITERATOR_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "String Iterator", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class StringIteratorPrototype final : public JSNonFinalObject`.
pub struct StringIteratorPrototype;

impl StringIteratorPrototype {
    /// `createStructure(vm, globalObject, prototype)` (`StringIteratorPrototypeInlines.h`).
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, JSNonFinalObject::STRUCTURE_FLAGS),
            &STRING_ITERATOR_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: o construtor e o `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        prototype.finish_creation(vm);
        put_direct_native_function_without_transition(
            vm,
            global_object,
            &prototype,
            &vm.property_names.next,
            0,
            string_iterator_proto_func_next,
            ImplementationVisibility::Public,
            Intrinsic::JSStringIteratorNextIntrinsic,
            DONT_ENUM,
        );
        // `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`.
        prototype.put_direct(
            vm,
            &PropertyName::from_identifier(&vm.property_names.to_string_tag_symbol),
            JSValue::from_js_string(js_string(vm, &WtfString::from_latin1(STRING_ITERATOR_PROTOTYPE_S_INFO.class_name.as_bytes()))),
            DONT_ENUM | READ_ONLY,
        );
        prototype.structure().set_may_be_prototype(true);
        prototype
    }
}

/// `stringIteratorProtoFuncNext`.
fn string_iterator_next(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(iterator) = JSStringIterator::from_value(&call.this_value()) else {
        return Err(Thrown::type_error("%StringIteratorPrototype%.next requires that |this| be a String Iterator instance"));
    };
    Ok(match iterator.next_with_advance(global_object.vm()) {
        None => create_iterator_result_object(global_object, JSValue::undefined(), true),
        Some(value) => create_iterator_result_object(global_object, JSValue::from_js_string(value), false),
    })
}

host_function!(string_iterator_proto_func_next, string_iterator_next);
