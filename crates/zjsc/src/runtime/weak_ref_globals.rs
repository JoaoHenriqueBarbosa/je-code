//! A parte de `JSGlobalObject::init(VM&)` que cria os `ClassStructure` `WeakRef` e `FinalizationRegistry`
//! (`m_weakObjectRefStructure`, `m_finalizationRegistryStructure`, JSGlobalObject.cpp, tabela
//! `globalObjectTable`: `WeakRef` e `FinalizationRegistry`, `DontEnum|ClassStructure`).
//!
//! DIVERGÊNCIA: o `LazyClassStructure` cria protótipo, estrutura e construtor na primeira leitura; aqui é
//! eager, na ordem de `install_json_reflect_and_collections` (`js_global_object_init.rs`), que é o que o
//! `WeakMap` e o `WeakSet` já fazem. O global não guarda as duas estruturas: os construtores derivam a
//! estrutura do `prototype` próprio (ver `collection_support::derived_structure`).

use crate::runtime::finalization_registry_constructor::FinalizationRegistryConstructor;
use crate::runtime::finalization_registry_prototype::FinalizationRegistryPrototype;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObjectRef;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::weak_object_ref_constructor::WeakObjectRefConstructor;
use crate::runtime::weak_object_ref_prototype::WeakObjectRefPrototype;

/// Cria `WeakRef` e `FinalizationRegistry` (protótipo com `didBecomePrototype`, construtor, o `constructor`
/// do protótipo como `DontEnum` e a propriedade global `DontEnum`).
pub fn install_weak_refs(global_object: &JSGlobalObject, object_prototype: &JSObjectRef, function_prototype: JSValue) {
    let vm = global_object.vm();
    let object_prototype_value = object_prototype.as_value();
    let constructor_name = PropertyName::from_identifier(&vm.property_names.constructor);
    let put_global = |name: &[u8], value: JSValue| {
        global_object.put_direct(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, name)), value, DONT_ENUM);
    };

    // `WeakRef`.
    let weak_ref_prototype_structure = WeakObjectRefPrototype::create_structure(vm, global_object, object_prototype_value);
    let weak_ref_prototype = WeakObjectRefPrototype::create(vm, global_object, &weak_ref_prototype_structure);
    weak_ref_prototype.did_become_prototype(vm);
    let weak_ref_constructor_structure = WeakObjectRefConstructor::create_structure(vm, global_object, function_prototype);
    let weak_ref_constructor = WeakObjectRefConstructor::create(vm, global_object, weak_ref_constructor_structure, &weak_ref_prototype);
    weak_ref_prototype.put_direct(vm, &constructor_name, weak_ref_constructor.as_value(), DONT_ENUM);
    put_global(b"WeakRef", weak_ref_constructor.as_value());

    // `FinalizationRegistry`.
    let registry_prototype_structure = FinalizationRegistryPrototype::create_structure(vm, global_object, object_prototype_value);
    let registry_prototype = FinalizationRegistryPrototype::create(vm, global_object, &registry_prototype_structure);
    registry_prototype.did_become_prototype(vm);
    let registry_constructor_structure = FinalizationRegistryConstructor::create_structure(vm, global_object, function_prototype);
    let registry_constructor =
        FinalizationRegistryConstructor::create(vm, global_object, registry_constructor_structure, &registry_prototype);
    registry_prototype.put_direct(vm, &constructor_name, registry_constructor.as_value(), DONT_ENUM);
    put_global(b"FinalizationRegistry", registry_constructor.as_value());
}
