//! Porte de `runtime/MapPrototype.{h,cpp}`: o `Map.prototype` (um `JSNonFinalObject` com o `ClassInfo`
//! `"Map"`) e as funções nativas, sobre os algoritmos puros de `js_map.rs`, inclusive `entries`
//! (a `m_mapProtoEntriesFunction` do global, também o `@@iterator`), `keys` e `values`.
//!
//! `Symbol.species` e `groupBy` são do `MapConstructor` (`map_constructor.rs`).
//!
//! `forEach` é o JS embutido de `builtins/MapPrototype.js` (`mapPrototypeForEachCodeGenerator`), pelo
//! `BuiltinCodeIndex::MapPrototypeForEachCode` e os intrínsecos `@mapStorage`/`@mapIterationNext`... de
//! `ordered_hash_table_storage.rs`, como no C++ (na pilha aparece como `forEach (native:1:11)`).
//!
//! DIVERGÊNCIAS:
//! - Sem `installMapPrototypeWatchpoint` (os watchpoints não existem).

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{
    put_existing_function_with_private_name, put_function_with_private_name, put_native_function, put_size_accessor,
    put_to_string_tag, run_collection, CollectionFailure,
};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::iteration_kind::IterationKind;
use crate::runtime::iterator_operations::call_checked;
use crate::runtime::builtins_source::BuiltinCodeIndex;
use crate::runtime::js_function::{call_host_function_as_constructor, create_builtin_function, JSFunction, JSFunctionRef};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_map::{
    create_map_iterator_object, get_map, map_proto_clear, map_proto_delete, map_proto_get,
    map_proto_get_or_insert, map_proto_get_or_insert_computed, map_proto_has, map_proto_set, map_proto_size,
    MAP_GET_OR_INSERT_COMPUTED_NOT_CALLABLE_MESSAGE,
};
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo MapPrototype::s_info`.
pub static MAP_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Map", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

fn map_proto_func_clear_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    run_collection(global_object, || Ok(map_proto_clear(call.this_value())?))
}

fn map_proto_func_delete_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    run_collection(global_object, || Ok(map_proto_delete(call.this_value(), call.argument(0))?))
}

fn map_proto_func_get_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    run_collection(global_object, || Ok(map_proto_get(call.this_value(), call.argument(0))?))
}

fn map_proto_func_has_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    run_collection(global_object, || Ok(map_proto_has(call.this_value(), call.argument(0))?))
}

fn map_proto_func_set_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    run_collection(global_object, || Ok(map_proto_set(call.this_value(), call.argument(0), call.argument(1))?))
}

fn map_proto_func_size_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    run_collection(global_object, || Ok(map_proto_size(call.this_value())?))
}

fn map_proto_func_get_or_insert_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    run_collection(global_object, || Ok(map_proto_get_or_insert(call.this_value(), call.argument(0), call.argument(1))?))
}

/// `mapProtoFuncGetOrInsertComputed`: o `getMap` vem antes da conferência do `callback`.
fn map_proto_func_get_or_insert_computed_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    run_collection(global_object, || {
        get_map(call.this_value())?;
        let callback = call.argument(1);
        if !callback.is_callable() {
            return Err(Thrown::type_error(MAP_GET_OR_INSERT_COMPUTED_NOT_CALLABLE_MESSAGE).into());
        }
        map_proto_get_or_insert_computed(call.this_value(), call.argument(0), |key| -> Result<JSValue, CollectionFailure> {
            Ok(call_checked(global_object, callback, JSValue::undefined(), &[key], MAP_GET_OR_INSERT_COMPUTED_NOT_CALLABLE_MESSAGE)?)
        })
    })
}

host_function!(map_proto_func_clear, map_proto_func_clear_body);
host_function!(map_proto_func_delete, map_proto_func_delete_body);
host_function!(map_proto_func_get, map_proto_func_get_body);
host_function!(map_proto_func_has, map_proto_func_has_body);
host_function!(map_proto_func_set, map_proto_func_set_body);
host_function!(map_proto_func_size, map_proto_func_size_body);
host_function!(map_proto_func_get_or_insert, map_proto_func_get_or_insert_body);
host_function!(map_proto_func_get_or_insert_computed, map_proto_func_get_or_insert_computed_body);

/// `createMapIteratorObject(globalObject, callFrame, kind)`: o iterador sobre `globalObject->mapIteratorStructure()`.
fn create_map_iterator(global_object: &JSGlobalObject, call: &HostCall, kind: IterationKind) -> HostResult {
    run_collection(global_object, || {
        Ok(create_map_iterator_object(global_object.vm(), &global_object.map_iterator_structure(), call.this_value(), kind)?)
    })
}

fn map_proto_func_entries_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    create_map_iterator(global_object, call, IterationKind::Entries)
}

fn map_proto_func_keys_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    create_map_iterator(global_object, call, IterationKind::Keys)
}

fn map_proto_func_values_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    create_map_iterator(global_object, call, IterationKind::Values)
}

host_function!(map_proto_func_entries, map_proto_func_entries_body);
host_function!(map_proto_func_keys, map_proto_func_keys_body);
host_function!(map_proto_func_values, map_proto_func_values_body);

/// A `JSFunction` de `m_mapProtoEntriesFunction.initLater(...)` do `JSGlobalObject`
/// (`JSFunction::create(vm, owner, 0, "entries", mapProtoFuncEntries, Public, JSMapEntriesIntrinsic)`).
pub fn create_map_proto_entries_function(vm: &VM, global_object: &JSGlobalObject) -> JSFunctionRef {
    JSFunction::create_native(
        vm,
        global_object,
        0,
        vm.property_names.builtin_names().entries_public_name().string().string(),
        map_proto_func_entries,
        ImplementationVisibility::Public,
        Intrinsic::JSMapEntriesIntrinsic,
        call_host_function_as_constructor,
    )
}

/// `class MapPrototype final : public JSNonFinalObject`: sem campos próprios.
pub struct MapPrototype;

impl MapPrototype {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, MapPrototype::STRUCTURE_FLAGS),
            &MAP_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `MapPrototype(vm, structure)` e `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        MapPrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm, globalObject)`, na ordem do C++ (ver as LACUNAS do cabeçalho).
    fn finish_creation(prototype: &JSObject, vm: &VM, global_object: &JSGlobalObject) {
        prototype.finish_creation(vm);

        let names = &vm.property_names;
        let builtin_names = names.builtin_names();
        // Função pública e a mesma sob o nome privado do `builtinNames` (`@clear`, `@delete`...).
        let define_with_private = |name: &Identifier, private_name: Identifier, length: u32, function: NativeFunction, intrinsic: Intrinsic| {
            put_function_with_private_name(vm, global_object, prototype, name, private_name, length, function, intrinsic);
        };

        define_with_private(&names.clear, builtin_names.clear_private_name(), 0, map_proto_func_clear, Intrinsic::NoIntrinsic);
        define_with_private(&names.delete_keyword, builtin_names.delete_private_name(), 1, map_proto_func_delete, Intrinsic::JSMapDeleteIntrinsic);

        // `entries`, e o `@@iterator` mais abaixo, são a `m_mapProtoEntriesFunction` do global.
        let entries = global_object.map_proto_entries_function().as_value();
        put_existing_function_with_private_name(vm, prototype, builtin_names.entries_public_name(), builtin_names.entries_private_name(), entries);

        // `JSFunction::create(vm, globalObject, mapPrototypeForEachCodeGenerator(vm), globalObject)`, sob o nome
        // público e sob `@forEach`.
        let for_each = create_builtin_function(vm, global_object, BuiltinCodeIndex::MapPrototypeForEachCode);
        put_existing_function_with_private_name(vm, prototype, &names.for_each, builtin_names.for_each_private_name(), for_each.as_value());
        define_with_private(&names.get, builtin_names.get_private_name(), 1, map_proto_func_get, Intrinsic::JSMapGetIntrinsic);
        define_with_private(&names.has, builtin_names.has_private_name(), 1, map_proto_func_has, Intrinsic::JSMapHasIntrinsic);
        define_with_private(builtin_names.keys_public_name(), builtin_names.keys_private_name(), 0, map_proto_func_keys, Intrinsic::JSMapKeysIntrinsic);
        define_with_private(&names.set, builtin_names.set_dup_private_name(), 2, map_proto_func_set, Intrinsic::JSMapSetIntrinsic);

        // JSC_NATIVE_FUNCTION_WITHOUT_TRANSITION("getOrInsert"_s, ...) e "getOrInsertComputed".
        let functions: [(&str, NativeFunction); 2] = [
            ("getOrInsert", map_proto_func_get_or_insert),
            ("getOrInsertComputed", map_proto_func_get_or_insert_computed),
        ];
        for (name, function) in functions {
            put_native_function(vm, global_object, prototype, &Identifier::from_span(vm, name.as_bytes()), 2, function, Intrinsic::NoIntrinsic);
        }

        // `size`: o acessor `get size`, também sob `@size`.
        put_size_accessor(vm, global_object, prototype, map_proto_func_size, Intrinsic::JSMapSizeIntrinsic);

        define_with_private(builtin_names.values_public_name(), builtin_names.values_private_name(), 0, map_proto_func_values, Intrinsic::JSMapValuesIntrinsic);

        prototype.put_direct_without_transition(vm, &PropertyName::from_identifier(&names.iterator_symbol), entries, DONT_ENUM);
        put_to_string_tag(vm, prototype, MAP_PROTOTYPE_S_INFO.class_name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    /// `forEach` é um builtin JS (não nativo) de nome `forEach` e `length` 1, a mesma função sob `@forEach`.
    #[test]
    fn for_each_is_js_builtin_under_public_and_private_name() {
        let global = JSGlobalObject::init(&Rc::new(VM::new()));
        let vm = global.vm();
        let structure = MapPrototype::create_structure(vm, &global, global.object_prototype().as_value());
        let prototype = MapPrototype::create(vm, &global, &structure);

        let public = prototype.get_direct_by_name(vm, &PropertyName::from_identifier(&vm.property_names.for_each));
        let private = prototype.get_direct_by_name(vm, &PropertyName::from_identifier(&vm.property_names.builtin_names().for_each_private_name()));
        let function = public.as_js_function().expect("forEach não é uma JSFunction");
        assert!(!function.is_host_function(), "forEach deve ser builtin JS");
        assert_eq!(function.js_executable().borrow().parameter_count(), 1);
        assert_eq!(&function.js_executable().borrow().name(), &vm.property_names.for_each);
        assert_eq!(public, private, "@forEach é a mesma função");
    }
}
