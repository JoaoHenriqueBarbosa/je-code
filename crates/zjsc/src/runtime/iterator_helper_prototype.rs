//! Porte de `runtime/JSIteratorHelperPrototype.{h,cpp}` e `JSIteratorHelperPrototypeInlines.h`: o
//! `%IteratorHelperPrototype%`, um `JSNonFinalObject` com `next` e `return` (builtins JS) e `@@toStringTag`.
//!
//! `jsIteratorHelperPrototypeTable` (`next`, `return`, `JSBuiltin DontEnum|Function 0`) fica no `ClassInfo` e a
//! `Structure` leva `HasStaticPropertyTable`: as duas reificam no primeiro acesso. Só o `@@toStringTag` nasce em
//! `finishCreation`.

use crate::runtime::builtins_source::BuiltinCodeIndex;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::lookup::{builtin_entry};
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::js_value::JSValue;
use crate::runtime::property_attribute::{BUILTIN, DONT_ENUM};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo JSIteratorHelperPrototype::s_info`.
pub static JS_ITERATOR_HELPER_PROTOTYPE_S_INFO: ClassInfo = ClassInfo {
    class_name: "Iterator Helper",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: Some(&JS_ITERATOR_HELPER_PROTOTYPE_TABLE),
    inherits_js_type_range: None,
};

/// `jsIteratorHelperPrototypeTableValues` de `JSIteratorHelperPrototype.lut.h`, na ordem do `@begin`.
static JS_ITERATOR_HELPER_PROTOTYPE_TABLE_VALUES: [HashTableValue; 2] = [
    builtin_entry("next", BuiltinCodeIndex::JsIteratorHelperPrototypeNextCode, 0),
    builtin_entry("return", BuiltinCodeIndex::JsIteratorHelperPrototypeReturnCode, 0),
];

/// `jsIteratorHelperPrototypeTable`.
static JS_ITERATOR_HELPER_PROTOTYPE_TABLE: HashTable =
    HashTable { class_for_this: None, values: &JS_ITERATOR_HELPER_PROTOTYPE_TABLE_VALUES };

/// `class JSIteratorHelperPrototype final : public JSNonFinalObject`.
pub struct JSIteratorHelperPrototype;

impl JSIteratorHelperPrototype {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject, prototype)` (`JSIteratorHelperPrototypeInlines.h`).
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, JSIteratorHelperPrototype::STRUCTURE_FLAGS),
            &JS_ITERATOR_HELPER_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: o construtor e `finishCreation(vm)`.
    pub fn create(vm: &VM, _global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        prototype.finish_creation(vm);
        // `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`.
        put_to_string_tag(vm, &prototype, JS_ITERATOR_HELPER_PROTOTYPE_S_INFO.class_name);
        prototype.structure().set_may_be_prototype(true);
        prototype
    }
}
