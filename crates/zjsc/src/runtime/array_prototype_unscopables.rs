//! A parte final de `ArrayPrototype::finishCreation` (`runtime/ArrayPrototype.cpp`, depois dos
//! `putDirectWithoutTransition` dos nomes privados): o objeto de `Array.prototype[Symbol.unscopables]`.
//!
//! DIVERGÊNCIA: `globalObject->nullPrototypeObjectStructure()` (`m_nullPrototypeObjectStructure`,
//! JSGlobalObject.cpp:1295) não existe no `JSGlobalObject` do porte; a estrutura sai de
//! `JSFinalObject::create_structure` com o mesmo protótipo nulo e a mesma capacidade inline, uma por chamada
//! (o `ArrayPrototype::finishCreation` roda uma vez por realm).

use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSFinalObject, JSObject};
use crate::runtime::js_value::JSValue;
use crate::runtime::property_attribute::{DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::vm::VM;

/// `unscopables` de `ArrayPrototype::finishCreation`: objeto de protótipo nulo, em modo dicionário, com os
/// nomes de método que o `with` não enxerga, gravado em `Array.prototype` como `@@unscopables`
/// (`DontEnum|ReadOnly`).
pub fn put_array_prototype_unscopables(vm: &VM, global_object: &JSGlobalObject, array_prototype: &JSObject) {
    let structure = JSFinalObject::create_structure(vm, Some(global_object), JSValue::Null, JSFinalObject::DEFAULT_INLINE_CAPACITY);
    let unscopables = JSFinalObject::create(vm, &structure);
    unscopables.convert_to_dictionary(vm);

    let builtin_names = vm.property_names.builtin_names();
    let names = [
        builtin_names.at_public_name(),
        &vm.property_names.copy_within,
        builtin_names.entries_public_name(),
        &vm.property_names.fill,
        builtin_names.find_public_name(),
        builtin_names.find_index_public_name(),
        builtin_names.find_last_public_name(),
        builtin_names.find_last_index_public_name(),
        &vm.property_names.flat,
        builtin_names.flat_map_public_name(),
        &vm.property_names.includes,
        builtin_names.keys_public_name(),
        &vm.property_names.to_reversed,
        &vm.property_names.to_sorted,
        &vm.property_names.to_spliced,
        builtin_names.values_public_name(),
    ];
    for name in names {
        unscopables.put_direct(vm, &PropertyName::from_identifier(name), JSValue::Bool(true), 0);
    }

    array_prototype.put_direct_without_transition(
        vm,
        &PropertyName::from_identifier(&vm.property_names.unscopables_symbol),
        unscopables.as_value(),
        DONT_ENUM | READ_ONLY,
    );
}
