//! Porte de `runtime/GeneratorPrototype.h` e `GeneratorPrototype.cpp`: o `%GeneratorPrototype%`, um
//! `JSNonFinalObject` com `next`, `return` e `throw` (builtins JS) e `@@toStringTag`.
//!
//! `generatorPrototypeTable` (`next`, `return`, `throw`, `JSBuiltin DontEnum|Function 1`) fica no `ClassInfo` e a
//! `Structure` leva `HasStaticPropertyTable`: as três reificam no primeiro acesso. Só o `@@toStringTag` nasce em
//! `finishCreation` (`JSC_TO_STRING_TAG_WITHOUT_TRANSITION`); o `constructor` entra depois, em
//! `link_generator_prototype` (`function_kind_intrinsics.rs`).

use crate::runtime::builtins_source::BuiltinCodeIndex;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::js_value::JSValue;
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::lookup::{builtin_entry};
use crate::runtime::property_attribute::{BUILTIN, DONT_ENUM};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo GeneratorPrototype::s_info`.
pub static GENERATOR_PROTOTYPE_S_INFO: ClassInfo = ClassInfo {
    class_name: "Generator",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: Some(&GENERATOR_PROTOTYPE_TABLE),
    inherits_js_type_range: None,
};

/// `generatorPrototypeTableValues` de `GeneratorPrototype.lut.h`, na ordem do `@begin`.
static GENERATOR_PROTOTYPE_TABLE_VALUES: [HashTableValue; 3] = [
    builtin_entry("next", BuiltinCodeIndex::GeneratorPrototypeNextCode, 1),
    builtin_entry("return", BuiltinCodeIndex::GeneratorPrototypeReturnCode, 1),
    builtin_entry("throw", BuiltinCodeIndex::GeneratorPrototypeThrowCode, 1),
];

/// `generatorPrototypeTable`.
static GENERATOR_PROTOTYPE_TABLE: HashTable = HashTable { class_for_this: None, values: &GENERATOR_PROTOTYPE_TABLE_VALUES };

/// `class GeneratorPrototype final : public JSNonFinalObject`.
pub struct GeneratorPrototype;

impl GeneratorPrototype {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject, prototype)` (`GeneratorPrototypeInlines.h`).
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, GeneratorPrototype::STRUCTURE_FLAGS),
            &GENERATOR_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `GeneratorPrototype(vm, structure)` e `finishCreation(vm)`.
    pub fn create(vm: &VM, _global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        prototype.finish_creation(vm);
        // `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`.
        put_to_string_tag(vm, &prototype, GENERATOR_PROTOTYPE_S_INFO.class_name);
        prototype.structure().set_may_be_prototype(true);
        prototype
    }
}
