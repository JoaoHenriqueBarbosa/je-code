//! Porte de `runtime/SetPrototype.{h,cpp}`: o `Set.prototype` (um `JSNonFinalObject` com o `ClassInfo`
//! `"Set"`) e as funções nativas, sobre os algoritmos puros de `js_set.rs`: `entries`, `values` (que é
//! também `keys` e `@@iterator`, a `m_setProtoValuesFunction` do global) e os sete métodos de conjunto
//! (`union`, `intersection`, `difference`, `symmetricDifference`, `isSubsetOf`, `isSupersetOf` e
//! `isDisjointFrom`, com o `GetSetRecord` de `getSetSizeAsInt`, `has` e `keys` do argumento).
//!
//! DIVERGÊNCIAS:
//! - `forEach` é o JS embutido de `builtins/SetPrototype.js` (`setPrototypeForEachCodeGenerator`), pelo
//!   `BuiltinCodeIndex::SetPrototypeForEachCode` e os intrínsecos `@setStorage`/`@setIterationNext`... de
//!   `ordered_hash_table_storage.rs`, como no C++ (na pilha aparece como `forEach (native:1:11)`).
//! - Os métodos de conjunto só têm o caminho genérico. O atalho `fastSet*` (argumento que é um `JSSet`
//!   com `setPrimordialWatchpointIsValid`) lê a tabela do argumento direto e dá o mesmo resultado
//!   observável que o genérico faz chamando o `size`, o `has` e o `keys` originais.
//! - Sem `installSetPrototypeWatchpoint` (os watchpoints não existem).

use std::ops::ControlFlow;

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::builtins_source::BuiltinCodeIndex;
use crate::runtime::collection_support::{
    put_existing_function_with_private_name, put_function_with_private_name, put_native_function, put_size_accessor,
    put_to_string_tag, run_collection,
};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::iteration_kind::IterationKind;
use crate::runtime::iterator_operations::{
    call_checked, for_each_in_iteration_record, for_each_in_iterator_protocol, get_value_property, iterator_close,
    iterator_direct, iterator_step, iterator_value, IterationRecord,
};
use crate::runtime::js_function::{call_host_function_as_constructor, create_builtin_function, JSFunction, JSFunctionRef};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_set::{
    create_set_iterator_object, get_set, set_proto_add, set_proto_clear, set_proto_delete,
    set_proto_has, set_proto_size, JSSet, JSSetRef,
};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo SetPrototype::s_info`.
pub static SET_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Set", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };


fn set_proto_func_add_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    run_collection(global_object, || Ok(set_proto_add(call.this_value(), call.argument(0))?))
}

fn set_proto_func_clear_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    run_collection(global_object, || Ok(set_proto_clear(call.this_value())?))
}

fn set_proto_func_delete_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    run_collection(global_object, || Ok(set_proto_delete(call.this_value(), call.argument(0))?))
}

fn set_proto_func_has_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    run_collection(global_object, || Ok(set_proto_has(call.this_value(), call.argument(0))?))
}

fn set_proto_func_size_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    run_collection(global_object, || Ok(set_proto_size(call.this_value())?))
}

host_function!(set_proto_func_add, set_proto_func_add_body);
host_function!(set_proto_func_clear, set_proto_func_clear_body);
host_function!(set_proto_func_delete, set_proto_func_delete_body);
host_function!(set_proto_func_has, set_proto_func_has_body);
host_function!(set_proto_func_size, set_proto_func_size_body);

/// `createSetIteratorObject(globalObject, callFrame, kind)`: o iterador sobre `globalObject->setIteratorStructure()`.
fn create_set_iterator(global_object: &JSGlobalObject, call: &HostCall, kind: IterationKind) -> HostResult {
    run_collection(global_object, || {
        Ok(create_set_iterator_object(global_object.vm(), &global_object.set_iterator_structure(), call.this_value(), kind)?)
    })
}

fn set_proto_func_values_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    create_set_iterator(global_object, call, IterationKind::Values)
}

fn set_proto_func_entries_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    create_set_iterator(global_object, call, IterationKind::Entries)
}

host_function!(set_proto_func_values, set_proto_func_values_body);
host_function!(set_proto_func_entries, set_proto_func_entries_body);

/// A `JSFunction` de `m_setProtoValuesFunction.initLater(...)` do `JSGlobalObject`
/// (`JSFunction::create(vm, owner, 0, "values", setProtoFuncValues, Public, JSSetValuesIntrinsic)`).
pub fn create_set_proto_values_function(vm: &VM, global_object: &JSGlobalObject) -> JSFunctionRef {
    JSFunction::create_native(
        vm,
        global_object,
        0,
        vm.property_names.builtin_names().values_public_name().string().string(),
        set_proto_func_values,
        ImplementationVisibility::Public,
        Intrinsic::JSSetValuesIntrinsic,
        call_host_function_as_constructor,
    )
}

/// O que `GetSetRecord` (https://tc39.es/ecma262/#sec-getsetrecord) devolve: o `size` já como inteiro,
/// o argumento e os `has` e `keys` dele, ambos já conferidos como chamáveis.
struct SetRecord {
    size: u32,
    other: JSValue,
    has: JSValue,
    keys: JSValue,
    /// O `MarkedArgumentBuffer` da operação: `has`, `keys`, o iterador de `keys()` e o `next` dele vivem só em locais
    /// Rust enquanto os getters e callbacks do outro conjunto rodam JS. Os conjuntos-resultado são `Rc` (raiz pela
    /// contagem) e o `other` vem dos argumentos do frame.
    marked: crate::runtime::gc::RootScope,
}

/// `getSetSizeAsInt(globalObject, value)` (GetSetRecord, passos 1 a 7).
fn get_set_size_as_int(global_object: &JSGlobalObject, value: JSValue) -> Result<u32, Thrown> {
    let vm = global_object.vm();
    if !value.is_object() {
        return Err(Thrown::type_error("Set operation expects first argument to be an object"));
    }
    let raw_size = get_value_property(global_object, value, &PropertyName::from_identifier(&vm.property_names.size))?;
    let number_size = raw_size.to_number();
    if vm.exception().is_some() {
        return Err(Thrown::Pending);
    }
    if number_size.is_nan() {
        return Err(Thrown::type_error("Set operation expects first argument to have non-NaN 'size' property"));
    }
    // `jsNumber(numSize).toIntegerOrInfinity(globalObject)`, com o `NaN` já descartado.
    let integer_or_infinity = number_size.trunc();
    if integer_or_infinity < 0.0 {
        return Err(Thrown::range_error("Set operation expects first argument to have non-negative 'size' property"));
    }
    if integer_or_infinity.is_infinite() {
        return Ok(u32::MAX);
    }
    Ok(integer_or_infinity as u32)
}

/// `GetSetRecord` na ordem do C++: `size`, depois `has` e depois `keys`, cada um com a conferência de
/// `isCallable` (`"Set.prototype.<method> expects other.has to be callable"`).
fn get_set_record(global_object: &JSGlobalObject, other: JSValue, method: &str) -> Result<SetRecord, Thrown> {
    let vm = global_object.vm();
    let marked = crate::runtime::gc::RootScope::new(&[other]);
    let size = get_set_size_as_int(global_object, other)?;
    let has = get_value_property(global_object, other, &PropertyName::from_identifier(&vm.property_names.has))?;
    marked.append(has);
    if !has.is_callable() {
        return Err(Thrown::type_error(&format!("Set.prototype.{method} expects other.has to be callable")));
    }
    let keys = get_value_property(global_object, other, &PropertyName::from_identifier(&vm.property_names.keys))?;
    marked.append(keys);
    if !keys.is_callable() {
        return Err(Thrown::type_error(&format!("Set.prototype.{method} expects other.keys to be callable")));
    }
    Ok(SetRecord { size, other, has, keys, marked })
}

impl SetRecord {
    /// `other.has(key)` como booleano.
    fn other_has(&self, global_object: &JSGlobalObject, key: JSValue) -> Result<bool, Thrown> {
        Ok(call_checked(global_object, self.has, self.other, &[key], "Type error")?.to_boolean())
    }

    /// `other.keys()`: o iterador, ainda sem o `next`.
    fn call_keys(&self, global_object: &JSGlobalObject) -> Result<JSValue, Thrown> {
        let iterator = call_checked(global_object, self.keys, self.other, &[], "Type error")?;
        self.marked.append(iterator);
        Ok(iterator)
    }

    /// `other.keys()` e o `next` dele conferido como chamável
    /// (`"Set.prototype.<method> expects other.keys().next to be callable"`).
    fn keys_iteration_record(&self, global_object: &JSGlobalObject, method: &str) -> Result<IterationRecord, Thrown> {
        let iterator = self.call_keys(global_object)?;
        let next_method = get_value_property(
            global_object,
            iterator,
            &PropertyName::from_identifier(&global_object.vm().property_names.next),
        )?;
        self.marked.append(next_method);
        if !next_method.is_callable() {
            return Err(Thrown::type_error(&format!("Set.prototype.{method} expects other.keys().next to be callable")));
        }
        Ok(IterationRecord { iterator, next_method })
    }
}

/// O laço `while (true)` dos métodos que leem `other.keys()` à mão (`difference`, `symmetricDifference`,
/// `isSupersetOf`, `isDisjointFrom`): `keep_going(value)` falso encerra com `iteratorClose` e devolve
/// `true`; esgotar o iterador devolve `false`.
fn drain_keys(
    global_object: &JSGlobalObject,
    record: IterationRecord,
    mut keep_going: impl FnMut(JSValue) -> bool,
) -> Result<bool, Thrown> {
    loop {
        let Some(result) = iterator_step(global_object, record)? else {
            return Ok(false);
        };
        let value = iterator_value(global_object, result)?;
        if keep_going(value) {
            continue;
        }
        iterator_close(global_object, record.iterator);
        // `scope.release(); iteratorClose(...); return jsBoolean(...)`: a exceção do `return` fica pendente.
        return if global_object.vm().exception().is_some() { Err(Thrown::Pending) } else { Ok(true) };
    }
}

/// `JSSet::create(vm, globalObject->setStructure())`.
fn create_empty_set(global_object: &JSGlobalObject) -> JSSetRef {
    JSSet::create(global_object.vm(), &global_object.set_structure())
}

/// `thisSet->clone(globalObject, vm, globalObject->setStructure())`.
fn clone_set(global_object: &JSGlobalObject, set: &JSSetRef) -> JSSetRef {
    set.clone_with_structure(global_object.vm(), &global_object.set_structure())
}

/// `setProtoFuncUnion`.
fn set_union(global_object: &JSGlobalObject, this_set: &JSSetRef, other: JSValue) -> Result<JSValue, Thrown> {
    // `getSetSizeAsInt` não tem uso aqui, mas é observável.
    let record = get_set_record(global_object, other, "union")?;
    let iteration_record = iterator_direct(global_object, record.call_keys(global_object)?)?;
    record.marked.append(iteration_record.next_method);
    let result = clone_set(global_object, this_set);
    for_each_in_iteration_record(global_object, iteration_record, |key| {
        result.add(key);
        Ok(())
    })?;
    Ok(result.as_value())
}

/// `setProtoFuncIntersection`.
fn set_intersection(global_object: &JSGlobalObject, this_set: &JSSetRef, other: JSValue) -> Result<JSValue, Thrown> {
    let record = get_set_record(global_object, other, "intersection")?;
    let result = create_empty_set(global_object);
    if this_set.table().borrow().size() <= record.size {
        this_set.for_each_key(|key| -> Result<ControlFlow<()>, Thrown> {
            if record.other_has(global_object, key)? {
                result.add(key);
            }
            Ok(ControlFlow::Continue(()))
        })?;
        return Ok(result.as_value());
    }
    let iterator = record.call_keys(global_object)?;
    for_each_in_iterator_protocol(global_object, iterator, |key| {
        if this_set.table().borrow().has(key) {
            result.add(key);
        }
        Ok(())
    })?;
    Ok(result.as_value())
}

/// `setProtoFuncDifference`.
fn set_difference(global_object: &JSGlobalObject, this_set: &JSSetRef, other: JSValue) -> Result<JSValue, Thrown> {
    let record = get_set_record(global_object, other, "difference")?;
    let result = clone_set(global_object, this_set);
    if result.table().borrow().size() <= record.size {
        result.for_each_key(|key| -> Result<ControlFlow<()>, Thrown> {
            if record.other_has(global_object, key)? {
                result.table().borrow_mut().remove(key);
            }
            Ok(ControlFlow::Continue(()))
        })?;
        return Ok(result.as_value());
    }
    let iteration_record = record.keys_iteration_record(global_object, "difference")?;
    drain_keys(global_object, iteration_record, |value| {
        result.table().borrow_mut().remove(value);
        true
    })?;
    Ok(result.as_value())
}

/// `setProtoFuncSymmetricDifference`.
fn set_symmetric_difference(global_object: &JSGlobalObject, this_set: &JSSetRef, other: JSValue) -> Result<JSValue, Thrown> {
    let record = get_set_record(global_object, other, "symmetricDifference")?;
    let iteration_record = record.keys_iteration_record(global_object, "symmetricDifference")?;
    let result = clone_set(global_object, this_set);
    drain_keys(global_object, iteration_record, |value| {
        if this_set.table().borrow().has(value) {
            result.table().borrow_mut().remove(value);
        } else {
            result.add(value);
        }
        true
    })?;
    Ok(result.as_value())
}

/// `setProtoFuncIsSubsetOf`.
fn set_is_subset_of(global_object: &JSGlobalObject, this_set: &JSSetRef, other: JSValue) -> Result<JSValue, Thrown> {
    let record = get_set_record(global_object, other, "isSubsetOf")?;
    if this_set.table().borrow().size() > record.size {
        return Ok(JSValue::Bool(false));
    }
    let mut is_subset = true;
    this_set.for_each_key(|key| -> Result<ControlFlow<()>, Thrown> {
        if record.other_has(global_object, key)? {
            return Ok(ControlFlow::Continue(()));
        }
        is_subset = false;
        Ok(ControlFlow::Break(()))
    })?;
    Ok(JSValue::Bool(is_subset))
}

/// `setProtoFuncIsSupersetOf`.
fn set_is_superset_of(global_object: &JSGlobalObject, this_set: &JSSetRef, other: JSValue) -> Result<JSValue, Thrown> {
    let record = get_set_record(global_object, other, "isSupersetOf")?;
    if this_set.table().borrow().size() < record.size {
        return Ok(JSValue::Bool(false));
    }
    let iteration_record = record.keys_iteration_record(global_object, "isSupersetOf")?;
    let closed_early = drain_keys(global_object, iteration_record, |value| this_set.table().borrow().has(value))?;
    Ok(JSValue::Bool(!closed_early))
}

/// `setProtoFuncIsDisjointFrom`.
fn set_is_disjoint_from(global_object: &JSGlobalObject, this_set: &JSSetRef, other: JSValue) -> Result<JSValue, Thrown> {
    let record = get_set_record(global_object, other, "isDisjointFrom")?;
    if this_set.table().borrow().size() <= record.size {
        let mut is_disjoint = true;
        this_set.for_each_key(|key| -> Result<ControlFlow<()>, Thrown> {
            if !record.other_has(global_object, key)? {
                return Ok(ControlFlow::Continue(()));
            }
            is_disjoint = false;
            Ok(ControlFlow::Break(()))
        })?;
        return Ok(JSValue::Bool(is_disjoint));
    }
    let iteration_record = record.keys_iteration_record(global_object, "isDisjointFrom")?;
    let closed_early = drain_keys(global_object, iteration_record, |value| !this_set.table().borrow().has(value))?;
    Ok(JSValue::Bool(!closed_early))
}

/// O `getSet` do `this` e a operação, com a falha convertida no `Thrown` do realm.
fn run_set_operation(
    global_object: &JSGlobalObject,
    call: &HostCall,
    operation: fn(&JSGlobalObject, &JSSetRef, JSValue) -> Result<JSValue, Thrown>,
) -> HostResult {
    run_collection(global_object, || {
        let this_set = get_set(call.this_value())?;
        Ok(operation(global_object, &this_set, call.argument(0))?)
    })
}

/// A casca de uma função nativa de método de conjunto: o corpo é `run_set_operation` com a operação.
macro_rules! set_operation_function {
    ($host:ident, $body:ident, $operation:ident) => {
        fn $body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
            run_set_operation(global_object, call, $operation)
        }
        host_function!($host, $body);
    };
}

set_operation_function!(set_proto_func_union, set_proto_func_union_body, set_union);
set_operation_function!(set_proto_func_intersection, set_proto_func_intersection_body, set_intersection);
set_operation_function!(set_proto_func_difference, set_proto_func_difference_body, set_difference);
set_operation_function!(set_proto_func_symmetric_difference, set_proto_func_symmetric_difference_body, set_symmetric_difference);
set_operation_function!(set_proto_func_is_subset_of, set_proto_func_is_subset_of_body, set_is_subset_of);
set_operation_function!(set_proto_func_is_superset_of, set_proto_func_is_superset_of_body, set_is_superset_of);
set_operation_function!(set_proto_func_is_disjoint_from, set_proto_func_is_disjoint_from_body, set_is_disjoint_from);

/// `class SetPrototype final : public JSNonFinalObject`: sem campos próprios.
pub struct SetPrototype;

impl SetPrototype {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, SetPrototype::STRUCTURE_FLAGS),
            &SET_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `SetPrototype(vm, structure)` e `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        SetPrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm, globalObject)`, na ordem do C++.
    fn finish_creation(prototype: &JSObject, vm: &VM, global_object: &JSGlobalObject) {
        prototype.finish_creation(vm);

        let names = &vm.property_names;
        let builtin_names = names.builtin_names();

        put_function_with_private_name(vm, global_object, prototype, &names.add, builtin_names.add_private_name(), 1, set_proto_func_add, Intrinsic::JSSetAddIntrinsic);
        put_function_with_private_name(vm, global_object, prototype, &names.clear, builtin_names.clear_private_name(), 0, set_proto_func_clear, Intrinsic::NoIntrinsic);
        put_function_with_private_name(vm, global_object, prototype, &names.delete_keyword, builtin_names.delete_private_name(), 1, set_proto_func_delete, Intrinsic::JSSetDeleteIntrinsic);
        put_function_with_private_name(
            vm,
            global_object,
            prototype,
            builtin_names.entries_public_name(),
            builtin_names.entries_private_name(),
            0,
            set_proto_func_entries,
            Intrinsic::JSSetEntriesIntrinsic,
        );
        // `JSFunction::create(vm, globalObject, setPrototypeForEachCodeGenerator(vm), globalObject)`, sob o nome
        // público e sob `@forEach`.
        let for_each = create_builtin_function(vm, global_object, BuiltinCodeIndex::SetPrototypeForEachCode);
        put_existing_function_with_private_name(vm, prototype, &names.for_each, builtin_names.for_each_private_name(), for_each.as_value());
        put_function_with_private_name(vm, global_object, prototype, &names.has, builtin_names.has_private_name(), 1, set_proto_func_has, Intrinsic::JSSetHasIntrinsic);

        // `keys`, `values` e `@@iterator` são a mesma função, a `m_setProtoValuesFunction` do global.
        let values = global_object.set_proto_values_function().as_value();
        put_existing_function_with_private_name(vm, prototype, builtin_names.keys_public_name(), builtin_names.keys_private_name(), values);

        put_size_accessor(vm, global_object, prototype, set_proto_func_size, Intrinsic::JSSetSizeIntrinsic);

        put_existing_function_with_private_name(vm, prototype, builtin_names.values_public_name(), builtin_names.values_private_name(), values);
        prototype.put_direct_without_transition(vm, &PropertyName::from_identifier(&names.iterator_symbol), values, DONT_ENUM);

        put_to_string_tag(vm, prototype, SET_PROTOTYPE_S_INFO.class_name);

        // `JSC_NATIVE_FUNCTION_WITHOUT_TRANSITION("union"_s, ..., DontEnum, 1, Public)` e as irmãs.
        let set_methods: [(&str, NativeFunction); 7] = [
            ("union", set_proto_func_union),
            ("intersection", set_proto_func_intersection),
            ("difference", set_proto_func_difference),
            ("symmetricDifference", set_proto_func_symmetric_difference),
            ("isSubsetOf", set_proto_func_is_subset_of),
            ("isSupersetOf", set_proto_func_is_superset_of),
            ("isDisjointFrom", set_proto_func_is_disjoint_from),
        ];
        for (name, function) in set_methods {
            put_native_function(vm, global_object, prototype, &Identifier::from_span(vm, name.as_bytes()), 1, function, Intrinsic::NoIntrinsic);
        }
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
        let structure = SetPrototype::create_structure(vm, &global, global.object_prototype().as_value());
        let prototype = SetPrototype::create(vm, &global, &structure);

        let public = prototype.get_direct_by_name(vm, &PropertyName::from_identifier(&vm.property_names.for_each));
        let private = prototype.get_direct_by_name(vm, &PropertyName::from_identifier(&vm.property_names.builtin_names().for_each_private_name()));
        let function = public.as_js_function().expect("forEach não é uma JSFunction");
        assert!(!function.is_host_function(), "forEach deve ser builtin JS");
        assert_eq!(function.js_executable().borrow().parameter_count(), 1);
        assert_eq!(&function.js_executable().borrow().name(), &vm.property_names.for_each);
        assert_eq!(public, private, "@forEach é a mesma função");
    }
}
