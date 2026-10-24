//! Porte de `runtime/ArrayPrototype.h`, `ArrayPrototypeInlines.h` e `ArrayPrototype.cpp`: o
//! `Array.prototype` (um `JSArray` com `ClassInfo` próprio e `ArrayClass`) e as funções nativas que o C++
//! implementa em C++.
//!
//! O QUE ESTÁ AQUI, e como:
//! - `push`, `pop`, `join`, `toString`, `reverse`, `shift`, `unshift`, `slice`, `splice`, `concat`,
//!   `indexOf`, `lastIndexOf`, `includes`, `fill`, `toReversed` e `with`, cada uma com o caminho rápido
//!   do C++ (`JSArray::push/pop`, leitura densa por `getIndexQuickly`) e o caminho genérico da
//!   especificação (`hasProperty`/`get`/`putByIndex`/`deleteProperty`/`length`).
//! - Os corpos são funções livres sobre `ArrayCall` (VM, global, `this`, argumentos, `newTarget`) e
//!   devolvem `Result<JSValue, ArrayError>`, o mesmo contrato de `js_array.rs`. O invólucro
//!   `NativeFunction` (`fn(&JSGlobalObject, &mut NativeCallFrame) -> EncodedJSValue`) é gerado pelo
//!   macro `array_host_function!`: `run_array_function` lê `this`/argumentos do `NativeCallFrame`, roda o
//!   corpo e converte o `Err` em exceção pendente (`throw_array_error`; `Unported` é `panic!` com o nome do
//!   que falta, como em `throw_put_error`).
//!
//! BUILTINS JS (registrados em `finish_creation` por `put_direct_builtin_function_without_transition`,
//! na ordem do C++, ver `ARRAY_PROTOTYPE_JS_BUILTINS`): `every`, `forEach`, `some`, `filter`, `flatMap`,
//! `reduce`, `reduceRight`, `map`, `find`, `findLast`, `findIndex`, `findLastIndex` e `at` são
//! `JSC_BUILTIN_FUNCTION_WITHOUT_TRANSITION` (`ArrayPrototype.js`, via `BuiltinExecutables`, fontes em
//! `builtins_source.rs`). Entram também os `@forEach`, `@map`, `@includes` e `@pop` privados.
//!
//! LACUNAS (ausentes em vez de falsas, ver CLAUDE.md), e por quê:
//! - `sort` e `toSorted` entram pelo `sortImpl` do C++ (`sortCompact`, `sortCommit`, ordenação por
//!   balde de `String` sem comparador e Powersort `MergeStrategy::Galloping` de `stable_sort.rs` com
//!   comparador). Sem os atalhos de `JSArray` do `sortCompact` e o `appendMemcpy` do `sortCommit`
//!   (só otimizam, o resultado é o do laço geral) e sem o `CachedCall`.
//!   `copyWithin` e `toSpliced` entram sem os atalhos `fastCopyWithin`/`fastToSpliced`.
//!   `keys`, `entries`, `values` e `@@iterator` entram (`JSArrayIterator`; `values` é a
//!   `arrayProtoValuesFunction` do global).
//!   Só `@indexOf` (privado) e `@shift` (privado) entram, porque o alvo existe.
//!
//! ESPÉCIE (`speciesConstructArray`, ArrayPrototypeInlines.h): `species_construct_array` consulta
//! `IsArray` (com `Proxy`), `constructor`, o realm do `Array` de outro realm e `@@species`, e constrói o
//! alvo; `concat`, `slice`, `splice` e `flat` o usam, e `new_array_with_species` é o corpo de
//! `slow_path_new_array_with_species` (o `op_new_array_with_species` de `map`, `filter` e `flatMap`).
//!
//! DIVERGÊNCIAS:
//! - Sem `watchpoints` (`arrayJoinWatchpointSet`, `arraySpeciesWatchpointIsValid`,
//!   `arrayMissingIsConcatSpreadable`, `isHavingABadTime`): `constructor`, `@@species` e
//!   `Symbol.isConcatSpreadable` são sempre consultados (a ordem observável da especificação), e o
//!   atalho do C++ vira o `FastPath` quando a espécie é o `Array` do realm corrente.
//! - `flat` não tem o atalho `JSArray::fastFlat`.
//! - Os alvos de espécie escrevem por `put_direct_index` (`CreateDataPropertyOrThrow`); o array comum do
//!   `FastPath` escreve por `put_by_index`, que só difere quando `Object.prototype` tem setter indexado.
//! - Índices acima de `MAX_ARRAY_INDEX` (nome por string, `ArrayStorage` esparso) e `ToObject` de
//!   primitivo (`NumberObject`, `StringObject`), `ToString`/`ToNumber` de objeto (chamam `toString`/
//!   `valueOf` do usuário) e `Proxy` em `isArray` respondem `Unported` com o nome do que falta.
//! - Os `JSOnlyStringsAndInt32sJoiner` e `fastArrayJoin` viram um laço só que junta por
//!   `join_runs_with_separator`; os atalhos de `StringRecursionChecker` (ciclo em `join`) ficam com o
//!   `StringRecursionChecker` quando ele existir.

use crate::runtime::class_info::ClassInfo;
use crate::runtime::error_messages::UNABLE_TO_DELETE_PROPERTY_ERROR;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::indexing_type::{has_indexed_properties, ARRAY_CLASS, CONTIGUOUS_SHAPE, DOUBLE_SHAPE, INDEXING_SHAPE_MASK, INT32_SHAPE};
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_array::{
    construct_array, is_js_array, put_through_method_table, ArrayError, JSArray, INVALID_ARRAY_LENGTH_ERROR, JS_ARRAY_S_INFO,
    LENGTH_EXCEEDED_THE_MAXIMUM_ARRAY_LENGTH_ERROR,
};
use crate::runtime::js_function::{
    call_host_function_as_constructor, put_direct_builtin_function_without_transition,
    put_direct_native_function_without_transition, JSFunction, JSFunctionRef,
};
use crate::runtime::iteration_kind::IterationKind;
use crate::runtime::js_array_iterator::JSArrayIterator;
use crate::runtime::builtins_source::BuiltinCodeIndex;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSObject, JSObjectHandle, PutError, MAX_STORAGE_VECTOR_LENGTH};
use crate::runtime::js_string::{js_empty_string, js_string};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_boolean, js_number, EncodedJSValue, JSValue};
use crate::runtime::math_common::max_safe_integer_as_uint64;
use crate::runtime::operations::{same_value_zero, strict_equal};
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_offset::INVALID_OFFSET;
use crate::runtime::put_property_slot::{PutContext, PutPropertySlot};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::interpreter::call_frame::NativeCallFrame;
use crate::runtime::error::create_range_error;
use crate::llint::LLIntFailure;
use crate::runtime::array_constructor::{is_array, is_array_constructor, IsArrayCaller, ARRAY_INVALID_LENGTH_ERROR};
use crate::runtime::call_data::{construct_with_error_message, get_call_data};
use crate::runtime::sparse_array_value_map::PutDirectIndexMode;
use crate::wtf::math_extras::truncate_double_to_uint64;
use crate::runtime::host_function_support::{throw_put_error, ObjectRef};
use crate::runtime::object_prototype::object_prototype_to_string;
use crate::runtime::object_to_primitive::call_function;
use crate::runtime::host_call::Thrown;
use crate::runtime::proxy_object::{
    object_delete_property, object_get, object_has_property, object_set, put_error_from_thrown, ProxyObject,
};
use crate::runtime::stable_sort::{array_stable_sort, coerce_comparator_result_to_boolean, MergeStrategy};
use crate::wtf::text::string_impl::{code_point_compare, MAX_LENGTH as MAX_STRING_LENGTH};
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use std::cell::RefCell;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::exception_helpers::create_not_an_object_error;
use crate::runtime::throw_scope::{throw_exception, throw_vm_exception, ThrowScope};
use crate::wtf::text::wtf_string::{join_runs_with_separator, String as WtfString};

/// `const ClassInfo ArrayPrototype::s_info`.
pub static ARRAY_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Array", parent_class: Some(&JS_ARRAY_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// Os `JSC_BUILTIN_FUNCTION_WITHOUT_TRANSITION` de `finishCreation` (ver o cabeçalho): nome, na ordem em
/// que o C++ os registra. Entram em `finish_creation` quando o `BuiltinExecutables` os compilar.
pub const ARRAY_PROTOTYPE_JS_BUILTINS: [&str; 13] = [
    "every", "forEach", "some", "filter", "flatMap", "reduce", "reduceRight", "map", "find", "findLast",
    "findIndex", "findLastIndex", "at",
];

/// O resultado de um corpo de função nativa: o valor, ou o erro que o C++ lançaria.
pub type ArrayResult = Result<JSValue, ArrayError>;

/// O que o C++ lê do `CallFrame`: `thisValue()` e `argument(n)`.
pub struct ArrayCall<'a> {
    pub vm: &'a VM,
    pub global_object: &'a JSGlobalObject,
    pub this_value: JSValue,
    pub args: &'a [JSValue],
    /// `callFrame->newTarget()`: `undefined` numa chamada sem `new`.
    pub new_target: JSValue,
}

impl<'a> ArrayCall<'a> {
    /// `callFrame->argument(i)`: `undefined` além do fim.
    pub fn argument(&self, i: usize) -> JSValue {
        self.args.get(i).copied().unwrap_or_else(JSValue::undefined)
    }

    /// `thisValue.toThis(globalObject, strict).toObject(globalObject)`.
    fn this_object(&self) -> Result<JSObjectHandle, ArrayError> {
        if let Some(object) = JSObject::from_value(&self.this_value) {
            // `Array.prototype.*` só usa `JSObject*`, mas uma `JSFunction` guarda `length`, `name` e
            // `prototype` preguiçosos (o `getOwnPropertySlot` virtual do C++ os materializa). O `Handle` não
            // despacha para ele, então eles são materializados aqui, na ordem do `getOwnSpecialPropertyNames`.
            if let Some(function) = self.this_value.as_js_function() {
                let names = &self.vm.property_names;
                for name in [&names.length, &names.name, &names.prototype] {
                    function.reify_lazy_property_if_needed(self.global_object, &PropertyName::from_identifier(name), false);
                }
                if self.vm.exception().is_some() {
                    return Err(PutError::Pending.into());
                }
            }
            return Ok(object);
        }
        if self.this_value.is_undefined_or_null() {
            // `toObject` de `undefined`/`null`: `createNotAnObjectError` com o `ErrorInstance` acrescentando
            // o texto-fonte da chamada JS em curso (`defaultSourceAppender`).
            let mut scope = ThrowScope::new(self.vm);
            let error = create_not_an_object_error(self.global_object, self.this_value);
            throw_exception(self.global_object, &mut scope, error);
            return Err(PutError::Pending.into());
        }
        // `toObject` de primitivo (`String`, número, booleano, `Symbol`, `BigInt`) ou de `JSFunction`: o
        // invólucro nunca lança aqui, `undefined` e `null` já foram tratados acima.
        match self.this_value.to_object(self.global_object) {
            Some(ObjectRef::Handle(object)) => Ok(object),
            // Inalcançável: a `JSFunction` já foi atendida acima (`JSObject::from_value` a aceita).
            Some(ObjectRef::Function(function)) => {
                JSObject::from_value(&function.as_value()).ok_or_else(|| type_error("undefined is not an object"))
            }
            None => Err(type_error("undefined is not an object")),
        }
    }

    /// `this_object` com a mensagem de `"Array.prototype.NAME requires that |this| not be null or undefined"`.
    fn this_object_requiring_coercible(&self, message: &'static str) -> Result<JSObjectHandle, ArrayError> {
        if self.this_value.is_undefined_or_null() {
            return Err(type_error(message));
        }
        self.this_object()
    }
}

fn type_error(message: &'static str) -> ArrayError {
    ArrayError::Put(PutError::TypeError(message))
}

/// `JSValue::toIntegerOrInfinity(globalObject)` com o `RETURN_IF_EXCEPTION`. A conversão só pode deixar uma
/// exceção pendente (`Thrown::Pending`), que o `ArrayError` carrega como `PutError::Pending`.
fn to_integer_or_infinity(value: JSValue) -> Result<f64, ArrayError> {
    value.to_integer_or_infinity_checked().map_err(|_| PutError::Pending.into())
}

/// `toLength(globalObject, object)`: o `length` de um `JSArray` direto, senão `ToLength(Get(O, "length"))`.
fn to_length(call: &ArrayCall, object: &JSObject) -> Result<u64, ArrayError> {
    if let Some(array) = JSArray::from_cell_id(object.cell_id()) {
        return Ok(u64::from(array.length()));
    }
    let length_name = PropertyName::from_identifier(&call.vm.property_names.length);
    let value = if is_proxy(object) {
        object_get(call.global_object, object, &length_name, object.as_value()).map_err(|thrown| proxy_error(call, thrown))?
    } else {
        object.get(call.vm, &length_name)
    };
    // `RETURN_IF_EXCEPTION`: um getter de `length` que lança devolve `empty` com a exceção pendente.
    propagate_pending(call)?;
    value.to_length_checked().map_err(|_| PutError::Pending.into())
}

/// `argumentClampedIndexFromStartOrEnd<RelativeNegativeIndex::Yes>`.
fn clamped_relative_index(value: JSValue, length: u64, undefined_value: u64) -> Result<u64, ArrayError> {
    if value.is_undefined() {
        return Ok(undefined_value);
    }
    let index = to_integer_or_infinity(value)?;
    Ok(clamp_relative(index, length))
}

fn clamp_relative(index: f64, length: u64) -> u64 {
    if index < 0.0 {
        let shifted = index + length as f64;
        return if shifted < 0.0 { 0 } else { shifted as u64 };
    }
    if index > length as f64 { length } else { index as u64 }
}

/// `argumentClampedIndexFromStartOrEnd<RelativeNegativeIndex::No>`: negativo vira 0.
fn clamped_absolute_index(value: JSValue, length: u64, undefined_value: u64) -> Result<u64, ArrayError> {
    if value.is_undefined() {
        return Ok(undefined_value);
    }
    let index = to_integer_or_infinity(value)?;
    if index < 0.0 {
        return Ok(0);
    }
    Ok(if index > length as f64 { length } else { index as u64 })
}

/// `Identifier::from(vm, uint64_t)`: acima de `u32::MAX` o índice é um nome de propriedade comum (a string
/// decimal do número), não um índice de array; vale até 2^53 - 1, onde o `f64` é exato.
fn large_index_name(call: &ArrayCall, index: u64) -> PropertyName {
    PropertyName::from_identifier(&Identifier::from_double(call.vm, index as f64))
}

/// `Some(index)` quando o índice cabe no caminho por `uint32_t` (`putByIndex` e companhia tratam os que
/// passam de `MAX_ARRAY_INDEX` por nome); `None` manda o chamador pelo caminho por `PropertyName`.
fn index_u32(index: u64) -> Option<u32> {
    // 4294967295 (`u32::MAX`) não é índice de array (`MAX_ARRAY_INDEX` é 4294967294): vai por nome.
    u32::try_from(index).ok().filter(|index| *index <= crate::runtime::identifier::MAX_ARRAY_INDEX)
}

/// `object` é um `Proxy`: os métodos de `JSObject` não despacham os traps, só as funções livres de
/// `proxy_object.rs` (`[[Get]]`, `[[HasProperty]]`, `[[Set]]`, `[[Delete]]`).
fn is_proxy(object: &JSObject) -> bool {
    ProxyObject::from_cell_id(object.cell_id()).is_some()
}

/// O `Thrown` de um trap como o `ArrayError`: a exceção já fica lançada no `VM`.
fn proxy_error(call: &ArrayCall, thrown: Thrown) -> ArrayError {
    ArrayError::Put(put_error_from_thrown(call.global_object, thrown))
}

/// `hasProperty(globalObject, index)`.
fn has_index(call: &ArrayCall, object: &JSObject, index: u64) -> Result<bool, ArrayError> {
    if is_proxy(object) {
        return object_has_property(call.global_object, object, &large_index_name(call, index)).map_err(|thrown| proxy_error(call, thrown));
    }
    match index_u32(index) {
        Some(index) => Ok(object.has_property_by_index(call.vm, index)),
        None => Ok(object.has_property(call.vm, &large_index_name(call, index))),
    }
}

/// `getIndex(globalObject, index)`.
fn get_index(call: &ArrayCall, object: &JSObject, index: u64) -> ArrayResult {
    if is_proxy(object) {
        return object_get(call.global_object, object, &large_index_name(call, index), object.as_value()).map_err(|thrown| proxy_error(call, thrown));
    }
    let value = match index_u32(index) {
        None => object.get(call.vm, &large_index_name(call, index)),
        Some(index) => match JSArray::from_cell_id(object.cell_id()) {
            Some(array) => array.get_by_index(call.vm, index),
            None => object.get_by_index(call.vm, index),
        },
    };
    // `RETURN_IF_EXCEPTION`: getter que lança devolve `empty` com a exceção pendente.
    propagate_pending(call)?;
    Ok(value)
}

/// `getProperty(globalObject, object, index)` (ArrayPrototypeInlines.h): `None` é o `JSValue()` de quem
/// não tem a propriedade.
fn get_property(call: &ArrayCall, object: &JSObject, index: u64) -> Result<Option<JSValue>, ArrayError> {
    if is_proxy(object) {
        if !has_index(call, object, index)? {
            return Ok(None);
        }
        return get_index(call, object, index).map(Some);
    }
    let Some(index) = index_u32(index) else {
        let name = large_index_name(call, index);
        if !object.has_property(call.vm, &name) {
            return Ok(None);
        }
        let value = object.get(call.vm, &name);
        propagate_pending(call)?;
        return Ok(Some(value));
    };
    if let Some(array) = JSArray::from_cell_id(object.cell_id()) {
        let value = dense_get(&array, index);
        if !value.is_empty() {
            return Ok(Some(value));
        }
    }
    if !object.has_property_by_index(call.vm, index) {
        return Ok(None);
    }
    let value = object.get_by_index(call.vm, index);
    propagate_pending(call)?;
    Ok(Some(value))
}

/// `putByIndexInline(globalObject, index, value, true)`.
fn put_index(call: &ArrayCall, object: &JSObject, index: u64, value: JSValue) -> Result<(), ArrayError> {
    if is_proxy(object) {
        let stored = object_set(call.global_object, object, &large_index_name(call, index), value, object.as_value(), true)
            .map_err(|thrown| proxy_error(call, thrown))?;
        if !stored {
            return Err(type_error("Attempted to assign to readonly property."));
        }
        return Ok(());
    }
    match index_u32(index) {
        Some(index) => {
            object.put_by_index(call.vm, index, value, true)?;
        }
        None => {
            let mut slot = PutPropertySlot::new(object.as_value(), true, PutContext::UnknownContext, false);
            object.put(call.vm, &large_index_name(call, index), value, &mut slot)?;
        }
    }
    Ok(())
}

/// `deleteProperty(globalObject, index)` com o `TypeError` de `UnableToDeletePropertyError`.
fn delete_index(call: &ArrayCall, object: &JSObject, index: u64) -> Result<(), ArrayError> {
    let deleted = if is_proxy(object) {
        object_delete_property(call.global_object, object, &large_index_name(call, index)).map_err(|thrown| proxy_error(call, thrown))?
    } else {
        match index_u32(index) {
            Some(index) => object.delete_property_by_index(call.vm, index)?,
            None => object.delete_property(
                call.vm,
                &large_index_name(call, index),
                &mut crate::runtime::delete_property_slot::DeletePropertySlot::default(),
            )?,
        }
    };
    if deleted {
        return Ok(());
    }
    Err(type_error(UNABLE_TO_DELETE_PROPERTY_ERROR))
}

/// `setLength(globalObject, vm, object, length)` (ArrayPrototypeInlines.h): `JSArray::setLength` ou
/// `put("length", length)` estrito.
pub(crate) fn set_length(call: &ArrayCall, object: &JSObject, length: u64) -> Result<(), ArrayError> {
    if let Some(array) = JSArray::from_cell_id(object.cell_id()) {
        let length = u32::try_from(length).map_err(|_| ArrayError::RangeError(INVALID_ARRAY_LENGTH_ERROR))?;
        array.set_length(call.vm, length, true)?;
        return Ok(());
    }
    if is_proxy(object) {
        let stored = object_set(
            call.global_object,
            object,
            &PropertyName::from_identifier(&call.vm.property_names.length),
            js_number(length as f64),
            object.as_value(),
            true,
        )
        .map_err(|thrown| proxy_error(call, thrown))?;
        if !stored {
            return Err(type_error("Attempted to assign to readonly property."));
        }
        return Ok(());
    }
    let mut slot = PutPropertySlot::new(object.as_value(), true, PutContext::UnknownContext, false);
    // `obj->methodTable()->put`: o `ArrayPrototype` (`DerivedArrayType`) cai aqui e precisa do `JSArray::put`.
    put_through_method_table(
        call.vm,
        object,
        &PropertyName::from_identifier(&call.vm.property_names.length),
        js_number(length as f64),
        &mut slot,
    )?;
    Ok(())
}

/// O elemento por `tryGetIndexQuickly` só nas formas que o porte tem (Int32, Double, Contiguous); as
/// demais (`Undecided`, `ArrayStorage`) devolvem `empty` e caem no caminho genérico.
fn dense_get(array: &JSArray, index: u32) -> JSValue {
    match array.cell().indexing_type() & INDEXING_SHAPE_MASK {
        INT32_SHAPE | DOUBLE_SHAPE | CONTIGUOUS_SHAPE => array.try_get_index_quickly(index),
        _ => JSValue::empty(),
    }
}

/// O caminho rápido de leitura: os `length` primeiros elementos de um `JSArray` sem buracos, ou `None`
/// (o chamador cai no caminho genérico).
fn dense_snapshot(object: &JSObject, length: u64) -> Option<Vec<JSValue>> {
    let array = JSArray::from_cell_id(object.cell_id())?;
    if length > u64::from(array.length()) || length > u64::from(MAX_STORAGE_VECTOR_LENGTH) {
        return None;
    }
    let mut values = crate::runtime::fallible_alloc::try_vec_with_capacity(length as usize)?;
    for index in 0..length as u32 {
        let value = dense_get(&array, index);
        if value.is_empty() {
            return None;
        }
        values.push(value);
    }
    Some(values)
}

/// `enum class SpeciesConstructResult`: `Exception` é o `Err`; `FastPath` pede um `Array` comum do realm
/// corrente; `CreatedObject` é o objeto que o construtor de `@@species` devolveu.
pub enum SpeciesConstruct {
    FastPath,
    CreatedObject(ObjectRef),
}

/// O `LLIntFailure` de `construct` como o erro que o corpo de função nativa devolve: a exceção já está
/// pendente (`Pending`), ou é a lacuna do porte.
pub(crate) fn array_error_from_llint(failure: LLIntFailure) -> ArrayError {
    ArrayError::Put(match failure {
        LLIntFailure::Thrown => PutError::Pending,
        LLIntFailure::Unported(what) => PutError::Unported(what),
        LLIntFailure::UnportedOpcode(_) => PutError::Unported("opcode sem handler no interpretador"),
    })
}

/// `speciesConstructArray(globalObject, thisObject, length)` (ArrayPrototypeInlines.h), o
/// ArraySpeciesCreate de https://tc39.github.io/ecma262/#sec-arrayspeciescreate.
///
/// Com o `arraySpeciesWatchpointIsValid` o array comum sai pelo atalho sem ler `constructor` nem `@@species`,
/// como no C++: as leituras não teriam efeito, mas o getter nativo de `@@species` entra no VM e gasta pilha.
/// Fora do atalho, a saída `FastPath` também vale quando a espécie é o próprio `Array` do realm corrente e o
/// comprimento cabe em `uint32` (`new Array(length)` e `constructEmptyArray(length)` são o mesmo array; acima
/// disso o `Array` lança o `RangeError` dele, então o construtor roda de verdade).
pub fn species_construct_array(call: &ArrayCall, this_value: JSValue, length: u64) -> Result<SpeciesConstruct, ArrayError> {
    let global_object = call.global_object;
    let names = &call.vm.property_names;
    // If isArray is false, return ? ArrayCreate(length).
    if !is_array(&this_value, IsArrayCaller::ArrayIsArray)? {
        return Ok(SpeciesConstruct::FastPath);
    }
    // Fast path in the normal case where the user has not set an own constructor and the Array.prototype.constructor is normal.
    // We need prototype check for subclasses of Array, which are Array objects but have a different prototype by default.
    if is_js_array(&this_value) && array_species_watchpoint_is_valid(call.vm, this_value) {
        return Ok(SpeciesConstruct::FastPath);
    }
    let mut constructor = get_value_property(call, this_value, &names.constructor)?;
    let is_current_realm = |value: JSValue| {
        value.as_object().structure().realm().map_or(true, |realm| std::ptr::eq(&*realm, global_object))
    };
    if is_array_constructor(&constructor) && !is_current_realm(constructor) {
        // The Array constructor of another realm is treated as undefined.
        return Ok(SpeciesConstruct::FastPath);
    }
    if constructor.is_object() {
        constructor = get_value_property(call, constructor, &names.species_symbol)?;
        if constructor.is_null() {
            return Ok(SpeciesConstruct::FastPath);
        }
    }
    if constructor.is_undefined() {
        return Ok(SpeciesConstruct::FastPath);
    }
    if length <= u64::from(u32::MAX) && is_array_constructor(&constructor) && is_current_realm(constructor) {
        return Ok(SpeciesConstruct::FastPath);
    }

    let created = construct_with_error_message(
        global_object,
        constructor,
        &[js_number(length as f64)],
        "Species construction did not get a valid constructor",
    )
    .map_err(array_error_from_llint)?;
    Ok(SpeciesConstruct::CreatedObject(created.as_object()))
}

/// `arraySpeciesWatchpointIsValid(vm, thisObject)` (ArrayPrototypeInlines.h). Sem o atalho de
/// `hasCustomProperties`, que só evita a busca de `constructor` quando o objeto não tem propriedade própria.
fn array_species_watchpoint_is_valid(vm: &VM, this_value: JSValue) -> bool {
    let object = this_value.as_object();
    let Some(global_object) = object.structure().realm() else { return false };
    if global_object.array_structure().stored_prototype() != object.get_prototype_direct() {
        return false;
    }
    if global_object.array_species_watchpoint_set.borrow().state() != crate::bytecode::watchpoint::WatchpointState::IsWatched {
        return false;
    }
    object.get_direct_offset(vm, &PropertyName::from_identifier(&vm.property_names.constructor))
        == crate::runtime::property_offset::INVALID_OFFSET
}

/// O alvo de `slow_path_new_array_with_species` (CommonSlowPaths.cpp, `op_new_array_with_species` de
/// `map`, `filter` e `flatMap`): `speciesConstructArray` e, no `FastPath`, `constructEmptyArray(length)`.
/// `array` é o objeto do registrador `m_array`; `length` o `m_length` (`truncateDoubleToUint64`).
/// `construct_empty_array` é o `constructEmptyArray(globalObject, &arrayAllocationProfile, length)` do
/// `FastPath`: o `ArrayAllocationProfile` vive no metadata do op, que o chamador possui, e o
/// `speciesConstructArray` roda código de usuário (que pode reentrar no mesmo `CodeBlock`), então o perfil
/// só é tomado emprestado dentro dessa função.
pub fn new_array_with_species(
    global_object: &JSGlobalObject,
    array: JSValue,
    length: u64,
    construct_empty_array: impl FnOnce(u32) -> Result<JSArray, ArrayError>,
) -> ArrayResult {
    let call = ArrayCall {
        vm: global_object.vm(),
        global_object,
        this_value: array,
        args: &[],
        new_target: JSValue::undefined(),
    };
    match species_construct_array(&call, array, length)? {
        SpeciesConstruct::CreatedObject(object) => Ok(object.as_value()),
        SpeciesConstruct::FastPath => {
            if length > u64::from(u32::MAX) {
                return Err(ArrayError::RangeError(ARRAY_INVALID_LENGTH_ERROR));
            }
            Ok(construct_empty_array(length as u32)?.as_value())
        }
    }
}

/// O alvo de `concat`, `splice` e `flat` quando a espécie é pedida com comprimento 0: o objeto de
/// `speciesConstructArray` ou `constructEmptyArray(globalObject, nullptr)`.
fn species_target(call: &ArrayCall, this_object: &JSObject, length: u64) -> Result<ObjectRef, ArrayError> {
    match species_construct_array(call, this_object.as_value(), length)? {
        SpeciesConstruct::CreatedObject(object) => Ok(object),
        SpeciesConstruct::FastPath => Ok(object_ref(new_array(call, 0)?.as_value())),
    }
}

fn object_ref(value: JSValue) -> ObjectRef {
    ObjectRef::from_value(&value).expect("o valor é um objeto")
}

/// `result->putDirectIndex(globalObject, index, value, 0, PutDirectIndexShouldThrow)`.
pub(crate) fn create_data_property_at(call: &ArrayCall, target: &JSObject, index: u64, value: JSValue) -> Result<(), ArrayError> {
    let Some(index) = index_u32(index) else {
        let name = large_index_name(call, index);
        if !target.put_direct(call.vm, &name, value, 0) {
            return Err(type_error("Attempting to define property on object that is not extensible."));
        }
        return Ok(());
    };
    target.put_direct_index(call.vm, index, value, 0, PutDirectIndexMode::PutDirectIndexShouldThrow)?;
    Ok(())
}

/// O laço de cópia de `slice` e `splice` com espécie: `[start, start + count)` de `source` para `target`
/// (buraco vira buraco) e o `setLength(target, count)` do fim.
fn copy_range_into(call: &ArrayCall, source: &JSObject, target: &JSObject, start: u64, count: u64) -> Result<(), ArrayError> {
    for n in 0..count {
        if let Some(value) = get_property(call, source, start + n)? {
            create_data_property_at(call, target, n, value)?;
        }
    }
    set_length(call, target, count)
}

/// `copy_range` ou, quando a espécie devolveu um objeto, `copy_range_into`: o corpo comum de `slice` e
/// `splice`.
fn species_copy_range(call: &ArrayCall, object: &JSObject, begin: u64, end: u64) -> ArrayResult {
    match species_construct_array(call, object.as_value(), end - begin)? {
        SpeciesConstruct::FastPath => Ok(copy_range(call, object, begin, end)?.as_value()),
        SpeciesConstruct::CreatedObject(target) => {
            copy_range_into(call, object, &target, begin, end - begin)?;
            Ok(target.as_value())
        }
    }
}

/// `Vec` com a capacidade de `length` elementos (no máximo `MAX_STORAGE_VECTOR_LENGTH`): a falta de memória é
/// `OutOfMemory`, nunca um abort.
fn values_with_capacity<T>(length: u64) -> Result<Vec<T>, ArrayError> {
    crate::runtime::fallible_alloc::try_vec_with_capacity(length.min(u64::from(MAX_STORAGE_VECTOR_LENGTH)) as usize)
        .ok_or(ArrayError::Put(PutError::OutOfMemory))
}

/// `constructEmptyArray(globalObject, nullptr, length)`: elementos buraco; a partir de
/// `MIN_ARRAY_STORAGE_CONSTRUCTION_LENGTH` o array nasce em `ArrayStorage` (esparso).
fn new_array(call: &ArrayCall, length: u64) -> Result<JSArray, ArrayError> {
    let length = u32::try_from(length).map_err(|_| ArrayError::RangeError(LENGTH_EXCEEDED_THE_MAXIMUM_ARRAY_LENGTH_ERROR))?;
    crate::runtime::js_global_object_inlines::construct_empty_array(call.vm, call.global_object, None, length)
}

/// Um array novo com `values` (`None` é buraco).
fn array_from_options(call: &ArrayCall, values: &[Option<JSValue>]) -> Result<JSArray, ArrayError> {
    if values.iter().all(Option::is_some) {
        let dense: Vec<JSValue> = values.iter().map(|value| value.unwrap_or_else(JSValue::undefined)).collect();
        return Ok(construct_array(call.vm, &call.global_object.array_structure(), &dense));
    }
    let array = new_array(call, values.len() as u64)?;
    for (index, value) in values.iter().enumerate() {
        if let Some(value) = value {
            create_data_property_at(call, &array, index as u64, *value)?;
        }
    }
    Ok(array)
}

/// `RETURN_IF_EXCEPTION(scope, ...)`: a exceção já pendente no `VM` só se propaga.
fn propagate_pending(call: &ArrayCall) -> Result<(), ArrayError> {
    if call.vm.exception().is_some() {
        return Err(ArrayError::Put(PutError::Pending));
    }
    Ok(())
}

/// `value.toString(globalObject)` já em `WTF::String`: chama `toString` do objeto, lança no símbolo.
fn element_to_wtf_string(call: &ArrayCall, value: JSValue) -> Result<WtfString, ArrayError> {
    let string = value.to_string(call.vm);
    propagate_pending(call)?;
    Ok(string.value())
}

/// `call(globalObject, function, callData, thisValue, args)`.
fn call_value(call: &ArrayCall, function: JSValue, this_value: JSValue, args: &[JSValue]) -> ArrayResult {
    call_function(call.global_object, function, this_value, args).ok_or(ArrayError::Put(PutError::Pending))
}

/// `value.get(globalObject, propertyName)`: o primitivo lê do protótipo do seu invólucro, com o próprio
/// primitivo de receptor.
fn get_value_property(call: &ArrayCall, value: JSValue, name: &Identifier) -> ArrayResult {
    let holder = if value.is_object() { Some(value) } else { value.primitive_wrapper_prototype(call.global_object) };
    let Some(holder) = holder.and_then(|holder| ObjectRef::from_value(&holder)) else {
        return Ok(JSValue::undefined());
    };
    let mut slot = PropertySlot::new(value, InternalMethodType::Get);
    let property_name = PropertyName::from_identifier(name);
    let found = holder.get_property_slot(call.global_object, &property_name, &mut slot);
    propagate_pending(call)?;
    if !found {
        return Ok(JSValue::undefined());
    }
    let result = slot.get_value_for(&property_name);
    propagate_pending(call)?;
    Ok(result)
}

thread_local! {
    /// `vm.stringRecursionCheckFirstObject` e `vm.stringRecursionCheckVisitedObjects`, como uma pilha só
    /// (o `cell_id` de cada objeto em `join`/`toLocaleString`).
    static STRING_RECURSION_VISITED: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

/// Fim do programa (`cell_registry::reset_program_state`): a pilha tem `cell_id` de objetos do programa
/// (sobra se um erro interrompeu um `join`).
/// As raízes do coletor: os objetos que o `join` está visitando.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    STRING_RECURSION_VISITED.with(|visited| visited.borrow().iter().for_each(|cell_id| marker.cell(*cell_id)));
}

pub(crate) fn reset_for_program() {
    let _ = STRING_RECURSION_VISITED.try_with(|visited| visited.borrow_mut().clear());
}

/// `StringRecursionChecker` (`StringRecursionChecker.h`, `USE(BUN_JSC_ADDITIONS)`): `enter` é `None` quando o
/// objeto já está sendo juntado, e o chamador devolve a string vazia (o `earlyReturnValue`). DIVERGÊNCIA: o
/// teste de `isSafeToRecurseSoft` não existe.
struct StringRecursionChecker(usize);

impl StringRecursionChecker {
    fn enter(cell_id: usize) -> Option<StringRecursionChecker> {
        STRING_RECURSION_VISITED.with(|visited| {
            let mut visited = visited.borrow_mut();
            if visited.contains(&cell_id) {
                return None;
            }
            visited.push(cell_id);
            Some(StringRecursionChecker(cell_id))
        })
    }
}

impl Drop for StringRecursionChecker {
    fn drop(&mut self) {
        STRING_RECURSION_VISITED.with(|visited| visited.borrow_mut().retain(|cell_id| *cell_id != self.0));
    }
}

/// `slowJoin` e `fastArrayJoin`: `undefined` e `null` viram a string vazia; `element_string` converte o
/// elemento (`toString` no `join`, `toLocaleString` no `toLocaleString`).
fn join_with_separator(
    call: &ArrayCall,
    object: &JSObject,
    length: u64,
    separator: &WtfString,
    element_string: fn(&ArrayCall, JSValue) -> Result<WtfString, ArrayError>,
) -> ArrayResult {
    let vm = call.vm;
    if length == 0 {
        return Ok(JSValue::from_js_string(js_empty_string(vm)));
    }
    // Corridas `(pedaço, repetições)`: os buracos e os `undefined`/`null` seguidos viram uma entrada só, então a
    // lista cresce sob demanda.
    let mut parts: Vec<(WtfString, u64)> = values_with_capacity(length.min(4096))?;
    // O `StringBuilder` do C++ marca overflow acima de `MaxLength` (OutOfMemoryError) ao juntar. Quando ler os
    // elementos não é observável, os separadores sozinhos já decidem o estouro antes do laço (o C++ chega ao mesmo
    // erro depois de percorrer os buracos); num `Proxy` cada leitura é um trap, e o C++ lê os elementos até o
    // estouro de verdade (medido no bun 1.4.2: `length` 2^32 lê `0`, `1`, ... até o trap lançar).
    let max_length = u64::from(MAX_STRING_LENGTH);
    let mut total = u64::from(separator.length()).saturating_mul(length - 1);
    if total > max_length && !is_proxy(object) {
        return Err(ArrayError::Put(PutError::OutOfMemory));
    }
    let separators = total;
    total = 0;
    // Um elemento objeto roda `toString` do usuário, que pode esvaziar ou mudar o array no meio do laço: os
    // seguintes têm de ser lidos na hora (o `fastJoin` do C++ relê o índice a cada volta), não de uma cópia.
    let snapshot = dense_snapshot(object, length).filter(|values| !values.iter().any(|value| value.is_object()));
    for index in 0..length {
        let element = match &snapshot {
            Some(values) => values[index as usize],
            None => get_index(call, object, index)?,
        };
        if element.is_undefined_or_null() {
            // `appendEmptyString`: conta o buraco sem materializar uma string por elemento.
            match parts.last_mut() {
                Some((piece, repeat)) if piece.length() == 0 => *repeat += 1,
                _ => parts.push((WtfString::default(), 1)),
            }
            continue;
        }
        let part = element_string(call, element)?;
        total += u64::from(part.length());
        if total + u64::from(separator.length()) * index > max_length {
            return Err(ArrayError::Put(PutError::OutOfMemory));
        }
        parts.push((part, 1));
    }
    if total + separators > max_length {
        return Err(ArrayError::Put(PutError::OutOfMemory));
    }
    Ok(JSValue::from_js_string(js_string(vm, &join_runs_with_separator(&parts, separator))))
}

/// `toLocaleString(globalObject, value, locales, options)` (`ArrayPrototype.cpp`), com `locales` e `options`
/// dos argumentos do `Array.prototype.toLocaleString` atual.
fn element_to_locale_string(call: &ArrayCall, value: JSValue) -> Result<WtfString, ArrayError> {
    let to_locale_string_method = get_value_property(call, value, &call.vm.property_names.to_locale_string)?;
    if get_call_data(to_locale_string_method).is_none() {
        return Err(type_error("toLocaleString is not callable"));
    }
    let result = call_value(call, to_locale_string_method, value, &[call.argument(0), call.argument(1)])?;
    element_to_wtf_string(call, result)
}

/// `arrayProtoFuncToLocaleString`.
pub fn array_proto_func_to_locale_string(call: &ArrayCall) -> ArrayResult {
    // 1. Let array be ? ToObject(this value).
    let object = call.this_object()?;
    let Some(_checker) = StringRecursionChecker::enter(object.cell_id()) else {
        return Ok(JSValue::from_js_string(js_empty_string(call.vm)));
    };

    // 2. Let len be ? ToLength(? Get(array, "length")).
    let length = to_length(call, &object)?;

    // 3. Let separator be the String value for the list-separator String appropriate for the host
    // environment's current locale (this is derived in an implementation-defined way).
    join_with_separator(call, &object, length, &WtfString::from_latin1(b","), element_to_locale_string)
}

/// `arrayProtoFuncJoin`.
pub fn array_proto_func_join(call: &ArrayCall) -> ArrayResult {
    let object = call.this_object()?;
    let Some(_checker) = StringRecursionChecker::enter(object.cell_id()) else {
        return Ok(JSValue::from_js_string(js_empty_string(call.vm)));
    };
    // `!vm.isSafeToRecurse()` do `join`: cada nível do ciclo join -> toString -> join paga o custo medido no bun.
    let Some(_stack_frame) = call.vm.enter_logical_frame(crate::runtime::vm::stack_cost::JOIN_LEVEL) else {
        return Err(ArrayError::Put(PutError::StackOverflow));
    };
    let length = to_length(call, &object)?;
    let separator_value = call.argument(0);
    let separator = if separator_value.is_undefined() {
        WtfString::from_latin1(b",")
    } else {
        element_to_wtf_string(call, separator_value)?
    };
    join_with_separator(call, &object, length, &separator, element_to_wtf_string)
}

/// O valor de `name` é a função nativa `host` primordial (a identidade que o watchpoint do C++ vigia).
fn is_original_native(value: JSValue, host: crate::runtime::native_function::NativeFunction) -> bool {
    matches!(
        get_call_data(value),
        crate::runtime::call_data::CallData::Native { function, .. } if function == crate::runtime::native_function::to_tagged(host)
    )
}

/// `canUseDefaultArrayJoinForToString(thisObject)` (ArrayPrototype.cpp): o `arrayJoinWatchpointSet` (o `join` de
/// `Array.prototype` é o original) e a `Structure` original de array (sem propriedade própria, `Array.prototype` de
/// protótipo). O porte confere as propriedades direto, como o `array_species_watchpoint_is_valid`.
fn can_use_default_array_join_for_to_string(global_object: &JSGlobalObject, this_value: JSValue) -> bool {
    let vm = global_object.vm();
    let Some(object) = JSObject::from_value(&this_value) else { return false };
    if !is_js_array(&this_value) || !global_object.is_original_array_structure(&object.structure()) {
        return false;
    }
    let Some(array_prototype) = JSObject::from_value(&global_object.array_structure().stored_prototype()) else { return false };
    let join = array_prototype.get_direct_by_name(vm, &PropertyName::from_identifier(&vm.property_names.join));
    is_original_native(join, array_proto_func_join_host)
}

/// `JSArray::isToPrimitiveFastAndNonObservable()` (JSArray.cpp): além do `join`, que `toString`, `valueOf` e
/// `@@toPrimitive` não foram redefinidos nem aparecem em `Array.prototype` ou `Object.prototype`, e que a cadeia
/// de protótipos é a original. Com isso `toPrimitive` e `toString` do array são o `fastToString`, sem chamada.
pub fn is_to_primitive_fast_and_non_observable(global_object: &JSGlobalObject, this_value: JSValue) -> bool {
    if !can_use_default_array_join_for_to_string(global_object, this_value) {
        return false;
    }
    let vm = global_object.vm();
    let names = &vm.property_names;
    let Some(array_prototype) = JSObject::from_value(&global_object.array_structure().stored_prototype()) else { return false };
    let object_prototype = global_object.object_prototype();
    if array_prototype.get_prototype_direct() != object_prototype.as_value() {
        return false;
    }
    let absent = |object: &JSObject, name: &Identifier| object.get_direct_offset(vm, &PropertyName::from_identifier(name)) == INVALID_OFFSET;
    let to_string = array_prototype.get_direct_by_name(vm, &PropertyName::from_identifier(&names.to_string));
    let value_of = object_prototype.get_direct_by_name(vm, &PropertyName::from_identifier(&names.value_of));
    is_original_native(to_string, array_proto_func_to_string_host)
        && is_original_native(value_of, crate::runtime::object_prototype::object_proto_func_value_of)
        && absent(&array_prototype, &names.value_of)
        && absent(&array_prototype, &names.to_primitive_symbol)
        && absent(&object_prototype, &names.to_primitive_symbol)
}

/// `JSArray::fastToString(globalObject)`: o `join` com a vírgula, sem passar por `call()` (e sem entrada no VM), com
/// a mesma proteção de ciclo e de pilha do `join`. `None` com a exceção pendente.
pub fn fast_to_string(global_object: &JSGlobalObject, array: JSValue) -> Option<JSValue> {
    let call = ArrayCall { vm: global_object.vm(), global_object, this_value: array, args: &[], new_target: JSValue::undefined() };
    match array_proto_func_join(&call) {
        Ok(value) => Some(value),
        Err(error) => {
            throw_array_error(global_object, error);
            None
        }
    }
}

/// `arrayProtoFuncToString`: o `join` do objeto (`Get(array, "join")`); sem `join` chamável é o
/// `Object.prototype.toString` (passo 3). Com `canUseDefaultArrayJoinForToString` é o `fastToString` direto.
pub fn array_proto_func_to_string(call: &ArrayCall) -> ArrayResult {
    // 1. Let array be the result of calling ToObject on the this value.
    let object = call.this_object()?;

    if can_use_default_array_join_for_to_string(call.global_object, call.this_value) {
        return fast_to_string(call.global_object, call.this_value).ok_or(ArrayError::Put(PutError::Pending));
    }

    // 2. Let func be the result of calling the [[Get]] internal method of array with argument "join".
    let function = object.get(call.vm, &PropertyName::from_identifier(&call.vm.property_names.join));
    propagate_pending(call)?;

    // 3. If IsCallable(func) is false, then let func be the standard built-in method Object.prototype.toString (15.2.4.2).
    if get_call_data(function).is_none() {
        return match object_prototype_to_string(call.global_object, call.this_value) {
            Some(string) => Ok(JSValue::from_js_string(string)),
            None => Err(ArrayError::Put(PutError::Pending)),
        };
    }

    // 4. Return the result of calling the [[Call]] internal method of func providing array as the this value and an empty arguments list.
    call_value(call, function, call.this_value, &[])
}

/// `arrayProtoFuncPop`.
pub fn array_proto_func_pop(call: &ArrayCall) -> ArrayResult {
    if let Some(array) = JSArray::from_value(&call.this_value) {
        return array.pop(call.vm);
    }
    let object = call.this_object()?;
    let length = to_length(call, &object)?;
    if length == 0 {
        set_length(call, &object, 0)?;
        return Ok(JSValue::undefined());
    }
    let index = length - 1;
    let result = get_index(call, &object, index)?;
    delete_index(call, &object, index)?;
    set_length(call, &object, index)?;
    Ok(result)
}

/// `arrayProtoFuncPush`.
pub fn array_proto_func_push(call: &ArrayCall) -> ArrayResult {
    if call.args.len() == 1 {
        if let Some(array) = JSArray::from_value(&call.this_value) {
            array.push(call.vm, call.args[0])?;
            return Ok(JSValue::from_u32(array.length()));
        }
    }
    let object = call.this_object()?;
    let length = to_length(call, &object)?;
    let arg_count = call.args.len() as u64;
    if length + arg_count > max_safe_integer_as_uint64() {
        return Err(type_error("push cannot produce an array of length larger than (2 ** 53) - 1"));
    }
    for (n, value) in call.args.iter().enumerate() {
        put_index(call, &object, length + n as u64, *value)?;
    }
    let new_length = length + arg_count;
    set_length(call, &object, new_length)?;
    Ok(js_number(new_length as f64))
}

/// `arrayProtoFuncReverse`.
pub fn array_proto_func_reverse(call: &ArrayCall) -> ArrayResult {
    let object = call.this_object()?;
    let length = to_length(call, &object)?;
    object.ensure_writable(call.vm);

    if let Some(mut values) = dense_snapshot(&object, length) {
        values.reverse();
        for (index, value) in values.into_iter().enumerate() {
            put_index(call, &object, index as u64, value)?;
        }
        return Ok(object.as_value());
    }

    let middle = length / 2;
    for lower in 0..middle {
        let upper = length - lower - 1;
        let lower_value = get_property(call, &object, lower)?;
        let upper_value = get_property(call, &object, upper)?;
        if lower_value.is_none() && upper_value.is_none() {
            continue;
        }
        match upper_value {
            Some(value) => put_index(call, &object, lower, value)?,
            None => delete_index(call, &object, lower)?,
        }
        match lower_value {
            Some(value) => put_index(call, &object, upper, value)?,
            None => delete_index(call, &object, upper)?,
        }
    }
    Ok(object.as_value())
}

/// O deslocamento do miolo de `shift`, `splice` e `unshift` (`shift<ShiftCountForShift/Splice>` e
/// `unshift` de ArrayPrototypeInlines.h): move `[from_start, length)` para `to_start` com buraco
/// propagando como `delete`, na direção que não sobrescreve o que ainda será lido.
fn move_elements(call: &ArrayCall, object: &JSObject, from_start: u64, to_start: u64, length: u64) -> Result<(), ArrayError> {
    if let Some(values) = dense_snapshot(object, length) {
        for (offset, value) in values[from_start as usize..].iter().enumerate() {
            put_index(call, object, to_start + offset as u64, *value)?;
        }
        return Ok(());
    }
    let count = length - from_start;
    let move_one = |k: u64| -> Result<(), ArrayError> {
        match get_property(call, object, from_start + k)? {
            Some(value) => put_index(call, object, to_start + k, value),
            None => delete_index(call, object, to_start + k),
        }
    };
    if to_start < from_start {
        for k in 0..count {
            move_one(k)?;
        }
    } else {
        for k in (0..count).rev() {
            move_one(k)?;
        }
    }
    Ok(())
}

/// Apaga `[from, length)` do fim (`deleteProperty` descendo), o passo 12 de `shift`/`splice`.
fn delete_tail(call: &ArrayCall, object: &JSObject, new_length: u64, old_length: u64) -> Result<(), ArrayError> {
    for k in (new_length..old_length).rev() {
        delete_index(call, object, k)?;
    }
    Ok(())
}

/// `arrayProtoFuncShift`.
pub fn array_proto_func_shift(call: &ArrayCall) -> ArrayResult {
    let object = call.this_object()?;
    let length = to_length(call, &object)?;
    if length == 0 {
        set_length(call, &object, 0)?;
        return Ok(JSValue::undefined());
    }
    let result = get_index(call, &object, 0)?;
    move_elements(call, &object, 1, 0, length)?;
    delete_tail(call, &object, length - 1, length)?;
    set_length(call, &object, length - 1)?;
    Ok(result)
}

/// `arrayProtoFuncUnShift`.
pub fn array_proto_func_unshift(call: &ArrayCall) -> ArrayResult {
    let object = call.this_object()?;
    let length = to_length(call, &object)?;
    let arg_count = call.args.len() as u64;
    if arg_count > 0 {
        if length + arg_count > max_safe_integer_as_uint64() {
            return Err(type_error("unshift cannot produce an array of length larger than (2 ** 53) - 1"));
        }
        // Um `JSArray` não passa de 2^32 - 1 elementos: o `unshift` lança antes de mover (o índice 2^32 - 1 não é
        // índice de array, e `move_elements` o trataria como tal).
        if JSArray::from_cell_id(object.cell_id()).is_some() && length + arg_count > u64::from(u32::MAX) {
            return Err(ArrayError::Put(PutError::RangeError(crate::runtime::js_array::LENGTH_EXCEEDED_THE_MAXIMUM_ARRAY_LENGTH_ERROR)));
        }
        move_elements(call, &object, 0, arg_count, length)?;
    }
    for (k, value) in call.args.iter().enumerate() {
        put_index(call, &object, k as u64, *value)?;
    }
    let new_length = length + arg_count;
    set_length(call, &object, new_length)?;
    Ok(js_number(new_length as f64))
}

/// Copia `[begin, end)` de `object` (buraco continua buraco) para um array novo.
fn copy_range(call: &ArrayCall, object: &JSObject, begin: u64, end: u64) -> Result<JSArray, ArrayError> {
    let count = end - begin;
    if let Some(values) = dense_snapshot(object, end) {
        return Ok(construct_array(call.vm, &call.global_object.array_structure(), &values[begin as usize..end as usize]));
    }
    // `constructEmptyArray(globalObject, nullptr, count)` e `putDirectIndex` só dos elementos presentes
    // (buraco continua buraco); com `count` enorme o array nasce em `ArrayStorage`.
    let result = new_array(call, count)?;
    for k in begin..end {
        if let Some(value) = get_property(call, object, k)? {
            create_data_property_at(call, &result, k - begin, value)?;
        }
    }
    Ok(result)
}

/// `arrayProtoFuncSlice`.
pub fn array_proto_func_slice(call: &ArrayCall) -> ArrayResult {
    let object = call.this_object()?;
    let length = to_length(call, &object)?;
    let begin = clamped_relative_index(call.argument(0), length, 0)?;
    let end = clamped_relative_index(call.argument(1), length, length)?.max(begin);
    species_copy_range(call, &object, begin, end)
}

/// `arrayProtoFuncSplice`.
pub fn array_proto_func_splice(call: &ArrayCall) -> ArrayResult {
    let object = call.this_object()?;
    let length = to_length(call, &object)?;

    if call.args.is_empty() {
        let result = species_target(call, &object, 0)?;
        set_length(call, &result, 0)?;
        set_length(call, &object, length)?;
        return Ok(result.as_value());
    }

    let start = clamped_relative_index(call.argument(0), length, 0)?;
    let (item_count, delete_count) = if call.args.len() == 1 {
        (0, length - start)
    } else {
        (call.args.len() as u64 - 2, clamped_absolute_index(call.argument(1), length - start, 0)?)
    };
    if length - delete_count + item_count > max_safe_integer_as_uint64() {
        return Err(type_error("Splice cannot produce an array of length larger than (2 ** 53) - 1"));
    }

    let result = species_copy_range(call, &object, start, start + delete_count)?;
    // O array devolvido só vive neste local enquanto o deslocamento roda as traps de um Proxy.
    let _result = crate::runtime::gc::RootScope::new(&[result]);
    let new_length = length - delete_count + item_count;
    if item_count < delete_count {
        move_elements(call, &object, start + delete_count, start + item_count, length)?;
        delete_tail(call, &object, new_length, length)?;
    } else if item_count > delete_count {
        // O `unshift<ShiftCountForSplice>` do C++: um `JSArray` não passa de 2^32 - 1 elementos, e o `splice` lança
        // antes de mover (como no `unshift`).
        if JSArray::from_cell_id(object.cell_id()).is_some() && new_length > u64::from(u32::MAX) {
            return Err(ArrayError::Put(PutError::RangeError(crate::runtime::js_array::LENGTH_EXCEEDED_THE_MAXIMUM_ARRAY_LENGTH_ERROR)));
        }
        move_elements(call, &object, start + delete_count, start + item_count, length)?;
    }
    for k in 0..item_count {
        put_index(call, &object, start + k, call.args[(k + 2) as usize])?;
    }
    set_length(call, &object, new_length)?;
    Ok(result)
}

/// `IsConcatSpreadable(value)` (o corpo do laço de `arrayProtoFuncConcat`): o objeto, se `Symbol.isConcatSpreadable`
/// o torna espalhável (ou, sem o símbolo, `IsArray`).
fn concat_spreadable_object(call: &ArrayCall, value: JSValue) -> Result<Option<ObjectRef>, ArrayError> {
    let Some(object) = ObjectRef::from_value(&value) else { return Ok(None) };
    let spreadable = get_value_property(call, value, &call.vm.property_names.is_concat_spreadable_symbol)?;
    let is_spreadable =
        if spreadable.is_undefined() { is_array(&value, IsArrayCaller::ArrayIsArray)? } else { spreadable.to_boolean() };
    Ok(is_spreadable.then_some(object))
}

/// O resultado de `concat` em construção: o array comum do caminho `FastPath` (`constructEmptyArray(0)`) ou
/// o objeto que a espécie criou (escrito por `putDirectIndex`).
enum ConcatSink {
    Elements(JSArray),
    Species(ObjectRef),
}

impl ConcatSink {
    /// `result->putDirectIndex(resultIndex, element)`; `None` é buraco (nada a escrever; o `length` final
    /// vem do `setLength(resultIndex)`).
    fn put(&mut self, call: &ArrayCall, index: u64, element: Option<JSValue>) -> Result<(), ArrayError> {
        match self {
            ConcatSink::Elements(array) => {
                if let Some(element) = element {
                    create_data_property_at(call, array, index, element)?;
                }
            }
            ConcatSink::Species(target) => {
                if let Some(element) = element {
                    create_data_property_at(call, target, index, element)?;
                }
            }
        }
        Ok(())
    }
}

/// `arrayProtoFuncConcat`.
pub fn array_proto_func_concat(call: &ArrayCall) -> ArrayResult {
    let this_object = call.this_object_requiring_coercible("Array.prototype.concat requires that |this| not be null or undefined")?;
    let mut sink = match species_construct_array(call, this_object.as_value(), 0)? {
        SpeciesConstruct::FastPath => ConcatSink::Elements(new_array(call, 0)?),
        SpeciesConstruct::CreatedObject(target) => ConcatSink::Species(target),
    };

    let mut result_index: u64 = 0;
    let this_value = this_object.as_value();
    for current in std::iter::once(this_value).chain(call.args.iter().copied()) {
        if let Some(object) = concat_spreadable_object(call, current)? {
            let length = to_length(call, &object)?;
            if result_index + length > max_safe_integer_as_uint64() {
                return Err(type_error("Length exceeded the maximum array length"));
            }
            if let Some(values) = dense_snapshot(&object, length) {
                for value in values {
                    sink.put(call, result_index, Some(value))?;
                    result_index += 1;
                }
            } else {
                for index in 0..length {
                    let element = get_property(call, &object, index)?;
                    sink.put(call, result_index, element)?;
                    result_index += 1;
                }
            }
        } else {
            if result_index >= max_safe_integer_as_uint64() {
                return Err(type_error("Length exceeded the maximum array length"));
            }
            sink.put(call, result_index, Some(current))?;
            result_index += 1;
        }
    }

    match sink {
        ConcatSink::Elements(array) => {
            set_length(call, &array, result_index)?;
            Ok(array.as_value())
        }
        ConcatSink::Species(target) => {
            set_length(call, &target, result_index)?;
            Ok(target.as_value())
        }
    }
}

/// `flatIntoArray(globalObject, target, source, sourceLength, targetIndex, depth)`.
fn flat_into_array(
    call: &ArrayCall,
    target: &JSObject,
    source: &JSObject,
    source_length: u64,
    mut target_index: u64,
    depth: u64,
) -> Result<u64, ArrayError> {
    use crate::runtime::vm::stack_cost::{FLAT_BASE, FLAT_LEVEL};
    // A recursão do C++ vira pilha explícita (a pilha nativa do Rust não comporta os ~40 mil níveis que o bun
    // aguenta); a ordem das leituras e escritas observáveis é a mesma. Cada nível paga `FLAT_LEVEL` de pilha
    // lógica, e o limite do bun (`!vm.isSafeToRecurse()` na entrada de cada nível) sai como StackOverflow.
    let Some(_base_frame) = call.vm.enter_logical_frame(FLAT_BASE) else {
        return Err(ArrayError::Put(PutError::StackOverflow));
    };
    let mut frames = Vec::new();
    let mut levels: Vec<(ObjectRef, u64, u64, u64)> = Vec::new();
    let Some(first_frame) = call.vm.enter_logical_frame(FLAT_LEVEL) else {
        return Err(ArrayError::Put(PutError::StackOverflow));
    };
    frames.push(first_frame);
    levels.push((object_ref(source.as_value()), source_length, 0, depth));
    while let Some(level) = levels.last_mut() {
        let (current_source, current_length, source_index, current_depth) = (level.0.clone(), level.1, level.2, level.3);
        if source_index >= current_length {
            levels.pop();
            frames.pop();
            continue;
        }
        level.2 += 1;
        let Some(element) = get_property(call, &current_source, source_index)? else { continue };
        if current_depth > 0 && is_array(&element, IsArrayCaller::ArrayIsArray)? {
            let new_depth = if current_depth == u64::MAX { current_depth } else { current_depth - 1 };
            let element_object = element.as_object();
            let element_length = to_length(call, &element_object)?;
            let Some(frame) = call.vm.enter_logical_frame(FLAT_LEVEL) else {
                return Err(ArrayError::Put(PutError::StackOverflow));
            };
            frames.push(frame);
            levels.push((element_object, element_length, 0, new_depth));
        } else {
            if target_index >= max_safe_integer_as_uint64() {
                return Err(type_error("flatten array exceeds 2**52 - 1"));
            }
            create_data_property_at(call, target, target_index, element)?;
            target_index += 1;
        }
    }
    Ok(target_index)
}

/// `arrayProtoFuncFlat`. DIVERGÊNCIA: sem o atalho `JSArray::fastFlat`, só o caminho da especificação.
pub fn array_proto_func_flat(call: &ArrayCall) -> ArrayResult {
    let this_object = call.this_object_requiring_coercible("Array.prototype.flat requires that |this| not be null or undefined")?;
    let length = to_length(call, &this_object)?;

    let mut depth_num: u64 = 1;
    let depth_value = call.argument(0);
    if !depth_value.is_undefined() {
        depth_num = if depth_value.is_int32() {
            u64::try_from(depth_value.as_int32()).unwrap_or(0)
        } else {
            let depth = to_integer_or_infinity(depth_value)?;
            if depth < 0.0 {
                0
            } else if depth.is_infinite() {
                u64::MAX
            } else {
                truncate_double_to_uint64(depth)
            }
        };
    }

    let result = species_target(call, &this_object, 0)?;
    flat_into_array(call, &result, &this_object, length, 0, depth_num)?;
    Ok(result.as_value())
}

/// O índice de partida de `indexOf`/`includes` (`argumentClampedIndexFromStartOrEnd<Yes>`).
fn search_start(call: &ArrayCall, length: u64) -> Result<u64, ArrayError> {
    clamped_relative_index(call.argument(1), length, 0)
}

/// `arrayProtoFuncIndexOf`.
pub fn array_proto_func_index_of(call: &ArrayCall) -> ArrayResult {
    let object = call.this_object()?;
    let length = to_length(call, &object)?;
    if length == 0 {
        return Ok(js_number(-1.0));
    }
    let start = search_start(call, length)?;
    let search = call.argument(0);

    if let Some(values) = dense_snapshot(&object, length) {
        let found = values[start as usize..].iter().position(|value| strict_equal(search, *value));
        return Ok(js_number(found.map_or(-1.0, |offset| (start as usize + offset) as f64)));
    }
    let mut index = start;
    while index < length {
        index = next_present_index(&object, index, length);
        if index >= length {
            break;
        }
        if let Some(element) = get_property(call, &object, index)? {
            if strict_equal(search, element) {
                return Ok(js_number(index as f64));
            }
        }
        index += 1;
    }
    Ok(js_number(-1.0))
}

/// O próximo índice `>= index` (e `<= length`) que pode ter propriedade, pulando os buracos de um array em
/// `ArrayStorage` quando nenhum objeto da cadeia de protótipos tem indexados nem intercepta acessos (o laço genérico
/// de `length` 1e9 de `indexOf` deixa de custar 1e9 consultas).
fn next_present_index(object: &JSObject, index: u64, length: u64) -> u64 {
    if is_proxy(object) || index > u64::from(u32::MAX) || JSArray::from_cell_id(object.cell_id()).is_none() {
        return index;
    }
    if object.any_object_in_chain_may_intercept_indexed_accesses() {
        return index;
    }
    let mut prototype = JSObject::from_value(&object.structure().stored_prototype());
    while let Some(current) = prototype {
        if has_indexed_properties(current.cell().indexing_type()) || is_proxy(&current) {
            return index;
        }
        prototype = JSObject::from_value(&current.structure().stored_prototype());
    }
    match object.next_own_indexed_candidate(index as u32) {
        Some(Some(next)) => u64::from(next),
        Some(None) => length,
        None => index,
    }
}

/// `arrayProtoFuncLastIndexOf`.
pub fn array_proto_func_last_index_of(call: &ArrayCall) -> ArrayResult {
    let object = call.this_object()?;
    let length = to_length(call, &object)?;
    if length == 0 {
        return Ok(js_number(-1.0));
    }
    let mut index = length - 1;
    if call.args.len() >= 2 {
        let mut from = to_integer_or_infinity(call.args[1])?;
        if from < 0.0 {
            from += length as f64;
            if from < 0.0 {
                return Ok(js_number(-1.0));
            }
        }
        if from < length as f64 {
            index = from as u64;
        }
    }
    let search = call.argument(0);

    let snapshot = dense_snapshot(&object, length);
    loop {
        let element = match &snapshot {
            Some(values) => Some(values[index as usize]),
            None => get_property(call, &object, index)?,
        };
        if let Some(element) = element {
            if strict_equal(search, element) {
                return Ok(js_number(index as f64));
            }
        }
        if index == 0 {
            return Ok(js_number(-1.0));
        }
        index -= 1;
    }
}

/// `createArrayIteratorObject`: `toThis(strict).toObject()` e `JSArrayIterator::create`.
fn create_array_iterator_object(call: &ArrayCall, kind: IterationKind) -> ArrayResult {
    let object = call.this_object()?;
    Ok(JSArrayIterator::create(call.vm, &call.global_object.array_iterator_structure(), &object, kind).as_value())
}

/// `arrayProtoFuncValues`.
pub fn array_proto_func_values(call: &ArrayCall) -> ArrayResult {
    create_array_iterator_object(call, IterationKind::Values)
}

/// `arrayProtoFuncEntries`.
pub fn array_proto_func_entries(call: &ArrayCall) -> ArrayResult {
    create_array_iterator_object(call, IterationKind::Entries)
}

/// `arrayProtoFuncKeys`.
pub fn array_proto_func_keys(call: &ArrayCall) -> ArrayResult {
    create_array_iterator_object(call, IterationKind::Keys)
}

/// `arrayProtoFuncIncludes`.
pub fn array_proto_func_includes(call: &ArrayCall) -> ArrayResult {
    let object = call.this_object_requiring_coercible("Array.prototype.includes requires that |this| not be null or undefined")?;
    let length = to_length(call, &object)?;
    if length == 0 {
        return Ok(js_boolean(false));
    }
    let start = search_start(call, length)?;
    if start == length {
        return Ok(js_boolean(false));
    }
    let search = call.argument(0);

    if let Some(values) = dense_snapshot(&object, length) {
        return Ok(js_boolean(values[start as usize..].iter().any(|value| same_value_zero(search, *value))));
    }
    for index in start..length {
        // `getIndex`: o buraco lê `undefined`.
        if same_value_zero(search, get_index(call, &object, index)?) {
            return Ok(js_boolean(true));
        }
    }
    Ok(js_boolean(false))
}

/// `arrayProtoFuncFill`.
pub fn array_proto_func_fill(call: &ArrayCall) -> ArrayResult {
    let object = call.this_object_requiring_coercible("Array.prototype.fill requires that |this| not be null or undefined")?;
    let length = to_length(call, &object)?;
    let start = clamped_relative_index(call.argument(1), length, 0)?;
    let end = clamped_relative_index(call.argument(2), length, length)?;
    let value = call.argument(0);
    for index in start..end {
        put_index(call, &object, index, value)?;
    }
    Ok(object.as_value())
}

/// `ToLength` do resultado de `toReversed`/`with`: `RangeError` acima de `uint32`.
fn checked_result_length(length: u64) -> Result<u32, ArrayError> {
    let length = u32::try_from(length).map_err(|_| ArrayError::RangeError("Array length must be a positive integer of safe magnitude."))?;
    // O C++ falha em `tryCreateUninitializedRestricted` acima do vetor máximo e lança OutOfMemoryError.
    if length > MAX_STORAGE_VECTOR_LENGTH {
        return Err(ArrayError::Put(PutError::OutOfMemory));
    }
    Ok(length)
}

/// `arrayProtoFuncToReversed`.
pub fn array_proto_func_to_reversed(call: &ArrayCall) -> ArrayResult {
    let object = call.this_object_requiring_coercible("Array.prototype.toReversed requires that |this| not be null or undefined")?;
    let length = to_length(call, &object)?;
    checked_result_length(length)?;
    let mut values = values_with_capacity(length)?;
    for k in 0..length {
        values.push(Some(get_index(call, &object, length - k - 1)?));
    }
    Ok(array_from_options(call, &values)?.as_value())
}

/// `arrayProtoFuncWith`.
pub fn array_proto_func_with(call: &ArrayCall) -> ArrayResult {
    let object = call.this_object_requiring_coercible("Array.prototype.with requires that |this| not be null or undefined")?;
    let length = to_length(call, &object)?;

    // `argumentUnclampedIndexFromStartOrEnd`.
    let relative = to_integer_or_infinity(call.argument(0))?;
    let actual = if relative < 0.0 { relative + length as f64 } else { relative };
    if actual >= length as f64 || actual < 0.0 {
        return Err(ArrayError::RangeError("Array index out of range"));
    }
    checked_result_length(length)?;

    let actual = actual as u64;
    let value = call.argument(1);
    let mut values = values_with_capacity(length)?;
    for k in 0..length {
        values.push(Some(if k == actual { value } else { get_index(call, &object, k)? }));
    }
    Ok(array_from_options(call, &values)?.as_value())
}

/// `arrayProtoFuncCopyWithin` (sem o atalho `JSArray::fastCopyWithin`, que só otimiza).
pub fn array_proto_func_copy_within(call: &ArrayCall) -> ArrayResult {
    let object = call.this_object_requiring_coercible("Array.prototype.copyWithin requires that |this| not be null or undefined")?;
    let length = to_length(call, &object)?;
    let to = clamped_relative_index(call.argument(0), length, 0)?;
    let from = clamped_relative_index(call.argument(1), length, 0)?;
    let final_index = clamped_relative_index(call.argument(2), length, length)?;

    if final_index < from {
        return Ok(object.as_value());
    }
    let count = (length - to.max(from)).min(final_index - from);
    if count == 0 {
        return Ok(object.as_value());
    }

    // Sobreposição com `from < to < from + count`: copia de trás para frente.
    let backwards = from < to && to < from + count;
    for step in 0..count {
        let offset = if backwards { count - 1 - step } else { step };
        match get_property(call, &object, from + offset)? {
            Some(value) => put_index(call, &object, to + offset, value)?,
            None => delete_index(call, &object, to + offset)?,
        }
    }
    Ok(object.as_value())
}

/// `arrayProtoFuncToSpliced` (sem o atalho `JSArray::fastToSpliced`).
pub fn array_proto_func_to_spliced(call: &ArrayCall) -> ArrayResult {
    let object = call.this_object_requiring_coercible("Array.prototype.toSpliced requires that |this| not be null or undefined")?;
    let length = to_length(call, &object)?;
    let start = clamped_relative_index(call.argument(0), length, 0)?;

    let (insert_count, delete_count) = match call.args.len() {
        0 => (0, 0),
        1 => (0, length - start),
        count => (count as u64 - 2, clamped_absolute_index(call.argument(1), length - start, 0)?),
    };
    let new_length = length + insert_count - delete_count;
    if new_length >= max_safe_integer_as_uint64() {
        return Err(type_error("Array length exceeds 2**53 - 1"));
    }
    checked_result_length(new_length)?;

    let mut values = values_with_capacity(new_length)?;
    for k in 0..start {
        values.push(Some(get_index(call, &object, k)?));
    }
    for i in 0..insert_count {
        values.push(Some(call.argument((i + 2) as usize)));
    }
    for k in (start + insert_count)..new_length {
        values.push(Some(get_index(call, &object, k + delete_count - insert_count)?));
    }
    Ok(array_from_options(call, &values)?.as_value())
}

/// `SortEntry`: a string de um elemento e o índice dele em `compacted` (desempata a ordenação).
#[derive(Clone)]
struct SortEntry {
    string: WtfString,
    index: u32,
}

/// `radixSortThreshold`, `maxRadixLevel` e `radixBucketCount`.
const RADIX_SORT_THRESHOLD: usize = 14;
const MAX_RADIX_LEVEL: u32 = 32;
const RADIX_BUCKET_COUNT: usize = 257;

/// `codePointCompare(a.substring(depth), b.substring(depth))` das duas strings, cada uma em 8 ou 16 bits.
fn compare_strings_from(a: &WtfString, b: &WtfString, depth: usize) -> std::cmp::Ordering {
    let from = |length: u32| depth.min(length as usize);
    let (a_start, b_start) = (from(a.length()), from(b.length()));
    match (a.is_8bit(), b.is_8bit()) {
        (true, true) => code_point_compare(&a.span8()[a_start..], &b.span8()[b_start..]),
        (true, false) => code_point_compare(&a.span8()[a_start..], &b.span16()[b_start..]),
        (false, true) => code_point_compare(&a.span16()[a_start..], &b.span8()[b_start..]),
        (false, false) => code_point_compare(&a.span16()[a_start..], &b.span16()[b_start..]),
    }
}

/// `sortBucketSort<all8Bit>(entries, scratch, level)`: radix sort de um byte por nível. Com todas as
/// strings em 8 bits o byte é a própria unidade Latin-1; senão é a unidade UTF-16 partida em byte alto e
/// baixo, para os bytes ordenarem como as unidades. O balde 0 guarda as strings que já acabaram. Os
/// empates da folha se desfazem pelo índice original (estabilidade).
fn sort_bucket_sort(all_8bit: bool, entries: &mut [SortEntry], scratch: &mut [SortEntry], level: u32) {
    let size = entries.len();
    let depth = (if all_8bit { level } else { level >> 1 }) as usize;
    if size < RADIX_SORT_THRESHOLD || level > MAX_RADIX_LEVEL {
        if size < 2 {
            return;
        }
        entries.sort_by(|a, b| {
            compare_strings_from(&a.string, &b.string, depth).then_with(|| a.index.cmp(&b.index))
        });
        return;
    }

    let key_of = |entry: &SortEntry| -> usize {
        let string = &entry.string;
        if depth >= string.length() as usize {
            return 0;
        }
        if all_8bit {
            usize::from(string.span8()[depth]) + 1
        } else {
            let code_unit = string.code_unit_at(depth as u32);
            (if level & 1 != 0 { code_unit & 0xFF } else { code_unit >> 8 }) as usize + 1
        }
    };

    let mut counts = [0u32; RADIX_BUCKET_COUNT];
    let mut min_bucket = RADIX_BUCKET_COUNT - 1;
    let mut max_bucket = 0usize;
    for entry in entries.iter() {
        let key = key_of(entry);
        if counts[key] == 0 {
            min_bucket = min_bucket.min(key);
            max_bucket = max_bucket.max(key);
        }
        counts[key] += 1;
    }

    if min_bucket == max_bucket {
        if min_bucket == 0 {
            return;
        }
        sort_bucket_sort(all_8bit, entries, scratch, level + 1);
        return;
    }

    let mut cursors = [0usize; RADIX_BUCKET_COUNT];
    let mut running = 0usize;
    for i in min_bucket..=max_bucket {
        cursors[i] = running;
        running += counts[i] as usize;
    }

    for entry in entries.iter() {
        let key = key_of(entry);
        scratch[cursors[key]] = entry.clone();
        cursors[key] += 1;
    }
    entries.clone_from_slice(&scratch[..size]);

    let mut offset = 0usize;
    for i in min_bucket..=max_bucket {
        let count = counts[i] as usize;
        if i != 0 && count > 1 {
            sort_bucket_sort(all_8bit, &mut entries[offset..offset + count], scratch, level + 1);
        }
        offset += count;
    }
}

/// `sortCompact`: os elementos presentes de `0..length` (`getIfPropertyExists`), sem os `undefined`, e a
/// contagem destes. O C++ tem atalhos para `JSArray` sem buraco que herde do protótipo (lê o butterfly
/// direto); o resultado é o mesmo do laço geral, que é o único caminho aqui.
fn sort_compact(call: &ArrayCall, object: &JSObject, length: u64) -> Result<(u64, Vec<JSValue>), ArrayError> {
    let mut undefined_count = 0u64;
    let mut compacted = values_with_capacity(length)?;
    for index in 0..length {
        let value = get_property(call, object, index)?;
        propagate_pending(call)?;
        if let Some(value) = value {
            if value.is_undefined() {
                undefined_count += 1;
            } else {
                compacted.push(value);
            }
        }
    }
    Ok((undefined_count, compacted))
}

/// `sortStableSort`: `arrayStableSort<MergeStrategy::Galloping>` com o comparador do usuário.
/// `CachedCall` (atalho de chamada de função JS) é só desempenho: cada chamada vai por `call`.
fn sort_stable_sort(call: &ArrayCall, compacted: &mut [JSValue], comparator: JSValue) -> Result<(), ArrayError> {
    // Os frames do `sort` com comparador que ficam na pilha enquanto o comparador roda.
    let Some(_stack_frame) = call.vm.enter_logical_frame(crate::runtime::vm::stack_cost::SORT_COMPARATOR) else {
        return Err(ArrayError::Put(PutError::StackOverflow));
    };
    let mut working_set = compacted.to_vec();
    array_stable_sort(MergeStrategy::Galloping, compacted, &mut working_set, |left, right| {
        let result = call_function(call.global_object, comparator, JSValue::undefined(), &[left, right]).ok_or(Thrown::Pending)?;
        coerce_comparator_result_to_boolean(call.global_object, result)
    })
    .map_err(|_| ArrayError::Put(PutError::Pending))
}

/// `sortCommit`: grava os ordenados a partir de 0, depois os `undefined`, e apaga o resto até `length`.
/// O atalho `JSArray::appendMemcpy` do C++ só copia o butterfly; o efeito é o mesmo.
fn sort_commit(call: &ArrayCall, object: &JSObject, length: u64, sorted: &[JSValue], undefined_count: u64) -> Result<(), ArrayError> {
    let mut index = 0u64;
    for value in sorted {
        put_index(call, object, index, *value)?;
        index += 1;
    }

    let undefined_max = sorted.len() as u64 + undefined_count;
    while index < undefined_max {
        put_index(call, object, index, JSValue::undefined())?;
        index += 1;
    }

    while index < length {
        delete_index(call, object, index)?;
        index += 1;
    }
    Ok(())
}

/// `sortImpl(globalObject, thisObject, length, comparatorValue)`.
fn sort_impl(call: &ArrayCall, object: &JSObject, length: u64, comparator_value: JSValue) -> Result<(), ArrayError> {
    // For compatibility with Firefox and Chrome, do nothing observable
    // to the target array if it has 0 or 1 sortable properties.
    if length < 2 {
        return Ok(());
    }

    let is_string_sort = comparator_value.is_undefined();

    let (undefined_count, mut compacted) = sort_compact(call, object, length)?;

    if is_string_sort {
        let mut entries: Vec<SortEntry> = values_with_capacity(compacted.len() as u64)?;
        let mut all_8bit = true;
        for (index, value) in compacted.iter().enumerate() {
            let string = element_to_wtf_string(call, *value)?;
            all_8bit &= string.is_8bit();
            entries.push(SortEntry { string, index: index as u32 });
        }
        let mut scratch_entries = if entries.len() >= RADIX_SORT_THRESHOLD { entries.clone() } else { Vec::new() };
        sort_bucket_sort(all_8bit, &mut entries, &mut scratch_entries, 0);
        let sorted: Vec<JSValue> = entries.iter().map(|entry| compacted[entry.index as usize]).collect();
        return sort_commit(call, object, length, &sorted, undefined_count);
    }

    sort_stable_sort(call, &mut compacted, comparator_value)?;
    sort_commit(call, object, length, &compacted, undefined_count)
}

/// `comparatorValue` que não é `undefined` nem chamável: o `TypeError` de `message`.
fn require_comparator(comparator_value: JSValue, message: &'static str) -> Result<(), ArrayError> {
    if comparator_value.is_undefined() || comparator_value.is_callable() {
        return Ok(());
    }
    Err(type_error(message))
}

/// `arrayProtoFuncSort`.
pub fn array_proto_func_sort(call: &ArrayCall) -> ArrayResult {
    // https://tc39.es/ecma262/#sec-array.prototype.sort
    let comparator_value = call.argument(0);
    require_comparator(comparator_value, "Array.prototype.sort requires the comparator argument to be a function or undefined")?;

    let object = call.this_object()?;
    let length = to_length(call, &object)?;
    sort_impl(call, &object, length, comparator_value)?;
    Ok(object.as_value())
}

/// `arrayProtoFuncToSorted`.
pub fn array_proto_func_to_sorted(call: &ArrayCall) -> ArrayResult {
    let comparator_value = call.argument(0);
    require_comparator(comparator_value, "Array.prototype.toSorted requires the comparator argument to be a function or undefined")?;

    let object = call.this_object_requiring_coercible("Array.prototype.toSorted requires that |this| not be null or undefined")?;
    let length = to_length(call, &object)?;
    checked_result_length(length)?;

    // `tryCloneArrayFromFast<ArrayFillMode::Undefined>` e o laço de `getIndex` + `putDirectIndex` dão o
    // mesmo array: `Get(O, k)` de todo `k`, com o buraco lido como `undefined` (ou do protótipo).
    let mut values = values_with_capacity(length)?;
    for k in 0..length {
        values.push(Some(get_index(call, &object, k)?));
    }
    let result = array_from_options(call, &values)?;
    sort_impl(call, &result, length, comparator_value)?;
    Ok(result.as_value())
}

/// O `RangeError` ou o `PutError` de um corpo de função nativa como exceção pendente (o que o
/// `ThrowScope` do C++ guarda); `Unported` é `panic!` com o nome do que falta (`throw_put_error`).
pub fn throw_array_error(global_object: &JSGlobalObject, error: ArrayError) {
    match error {
        ArrayError::Put(error) => throw_put_error(global_object, error),
        ArrayError::RangeError(message) => {
            let mut scope = ThrowScope::new(global_object.vm());
            let message = WtfString::from_utf8(message.as_bytes());
            throw_vm_exception(global_object, &mut scope, create_range_error(global_object, &message));
        }
    }
}

/// O invólucro `JSC_DEFINE_HOST_FUNCTION`: monta o `ArrayCall` a partir do `NativeCallFrame`, roda o
/// corpo e devolve o valor codificado, ou o `EncodedJSValue` nulo com a exceção pendente.
pub fn run_array_function(
    global_object: &JSGlobalObject,
    call_frame: &NativeCallFrame<'_>,
    body: fn(&ArrayCall) -> ArrayResult,
) -> EncodedJSValue {
    run_array_function_with_new_target(global_object, call_frame, body, call_frame.this_value())
}

/// `run_array_function` com o `newTarget` dito pelo chamador: o corpo da chamada sem `new`
/// (`callArrayConstructor`) passa `JSValue()`, porque o `this` do quadro nativo não é `newTarget` ali.
pub fn run_array_function_with_new_target(
    global_object: &JSGlobalObject,
    call_frame: &NativeCallFrame<'_>,
    body: fn(&ArrayCall) -> ArrayResult,
    new_target: JSValue,
) -> EncodedJSValue {
    let args = call_frame.arguments_span();
    let call = ArrayCall {
        vm: global_object.vm(),
        global_object,
        this_value: crate::runtime::proxy_object::to_this_strict(call_frame.this_value()),
        args: &args,
        new_target,
    };
    match body(&call) {
        Ok(value) => value.encode(),
        Err(error) => {
            throw_array_error(global_object, error);
            JSValue::empty().encode()
        }
    }
}

/// Define o invólucro `NativeFunction` de um corpo de `ArrayCall`.
macro_rules! array_host_function {
    ($wrapper:ident, $body:path) => {
        fn $wrapper(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
            run_array_function(global_object, call_frame, $body)
        }
    };
}

array_host_function!(array_proto_func_to_string_host, array_proto_func_to_string);
array_host_function!(array_proto_func_to_locale_string_host, array_proto_func_to_locale_string);
array_host_function!(array_proto_func_concat_host, array_proto_func_concat);
array_host_function!(array_proto_func_flat_host, array_proto_func_flat);
array_host_function!(array_proto_func_fill_host, array_proto_func_fill);
array_host_function!(array_proto_func_join_host, array_proto_func_join);
array_host_function!(array_proto_func_pop_host, array_proto_func_pop);
array_host_function!(array_proto_func_push_host, array_proto_func_push);
array_host_function!(array_proto_func_reverse_host, array_proto_func_reverse);
array_host_function!(array_proto_func_shift_host, array_proto_func_shift);
array_host_function!(array_proto_func_slice_host, array_proto_func_slice);
array_host_function!(array_proto_func_splice_host, array_proto_func_splice);
array_host_function!(array_proto_func_unshift_host, array_proto_func_unshift);
array_host_function!(array_proto_func_index_of_host, array_proto_func_index_of);
array_host_function!(array_proto_func_last_index_of_host, array_proto_func_last_index_of);
array_host_function!(array_proto_func_includes_host, array_proto_func_includes);
array_host_function!(array_proto_func_to_reversed_host, array_proto_func_to_reversed);
array_host_function!(array_proto_func_with_host, array_proto_func_with);
array_host_function!(array_proto_func_sort_host, array_proto_func_sort);
array_host_function!(array_proto_func_to_sorted_host, array_proto_func_to_sorted);
array_host_function!(array_proto_func_copy_within_host, array_proto_func_copy_within);
array_host_function!(array_proto_func_to_spliced_host, array_proto_func_to_spliced);
array_host_function!(array_proto_func_values_host, array_proto_func_values);
array_host_function!(array_proto_func_entries_host, array_proto_func_entries);
array_host_function!(array_proto_func_keys_host, array_proto_func_keys);

/// A `JSFunction` de `m_arrayProtoValuesFunction.initLater(...)` do `JSGlobalObject`
/// (`JSFunction::create(vm, owner, 0, "values", arrayProtoFuncValues, Public, ArrayValuesIntrinsic)`).
pub fn create_array_proto_values_function(vm: &VM, global_object: &JSGlobalObject) -> JSFunctionRef {
    JSFunction::create_native(
        vm,
        global_object,
        0,
        vm.property_names.builtin_names().values_public_name().string().string(),
        array_proto_func_values_host,
        ImplementationVisibility::Public,
        Intrinsic::ArrayValuesIntrinsic,
        call_host_function_as_constructor,
    )
}

/// `class ArrayPrototype : public JSArray`: sem campos próprios.
pub struct ArrayPrototype;

impl ArrayPrototype {
    /// `createStructure(vm, globalObject, prototype)`: `DerivedArrayType` (então `isJSArray` é falso e os métodos de
    /// `Array.prototype` tomam o caminho genérico), `ArrayClass`, `JSArray::StructureFlags`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create_with_indexing_type(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::DerivedArrayType, JSArray::STRUCTURE_FLAGS),
            &ARRAY_PROTOTYPE_S_INFO,
            ARRAY_CLASS,
            0,
        )
    }

    /// `create(vm, globalObject, structure)`: `ArrayPrototype(vm, structure)` (`JSArray(vm, structure,
    /// nullptr)`, sem butterfly, comprimento 0) e `finishCreation(vm, globalObject)`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSArray {
        let object = JSObject::allocate(vm, structure);
        let prototype = JSArray::from_cell_id_by_class(object.cell_id()).expect("ArrayPrototype tem JSType DerivedArrayType");
        ArrayPrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm, globalObject)`, na parte que existe (ver LACUNAS do cabeçalho), na ordem do C++.
    fn finish_creation(prototype: &JSArray, vm: &VM, global_object: &JSGlobalObject) {
        let names = &vm.property_names;
        let builtin_names = names.builtin_names();
        let literal = |text: &[u8]| Identifier::from_span(vm, text);
        let define = |name: &Identifier, length: u32, function: NativeFunction, intrinsic: Intrinsic| {
            put_direct_native_function_without_transition(
                vm,
                global_object,
                prototype,
                name,
                length,
                function,
                ImplementationVisibility::Public,
                intrinsic,
                DONT_ENUM,
            );
        };

        // `JSC_BUILTIN_FUNCTION_WITHOUT_TRANSITION(name, xxxCodeGenerator, DontEnum)`.
        let builtin = |name: &Identifier, index: BuiltinCodeIndex| {
            put_direct_builtin_function_without_transition(vm, global_object, prototype, name, index, DONT_ENUM);
        };

        // `toString` é `globalObject->arrayProtoToStringFunction()` (um `LazyProperty` do global que cria a
        // `JSFunction` de `arrayProtoFuncToString`): aqui é a mesma função nativa, na mesma posição (a
        // primeira propriedade). `values` e `@@iterator` são a mesma `arrayProtoValuesFunction()`.
        define(&names.to_string, 0, array_proto_func_to_string_host, Intrinsic::NoIntrinsic);
        let values_function = global_object.array_proto_values_function().as_value();
        prototype.put_direct(vm, &PropertyName::from_identifier(builtin_names.values_public_name()), values_function, DONT_ENUM);
        prototype.put_direct(vm, &PropertyName::from_identifier(&names.iterator_symbol), values_function, DONT_ENUM);
        define(&names.to_locale_string, 0, array_proto_func_to_locale_string_host, Intrinsic::NoIntrinsic);
        define(builtin_names.concat_public_name(), 1, array_proto_func_concat_host, Intrinsic::ArrayConcatIntrinsic);
        define(&names.fill, 1, array_proto_func_fill_host, Intrinsic::NoIntrinsic);
        define(&names.join, 1, array_proto_func_join_host, Intrinsic::ArrayJoinIntrinsic);
        define(&literal(b"pop"), 0, array_proto_func_pop_host, Intrinsic::ArrayPopIntrinsic);
        define(builtin_names.push_public_name(), 1, array_proto_func_push_host, Intrinsic::ArrayPushIntrinsic);
        define(&literal(b"reverse"), 0, array_proto_func_reverse_host, Intrinsic::NoIntrinsic);
        define(builtin_names.shift_public_name(), 0, array_proto_func_shift_host, Intrinsic::ArrayShiftIntrinsic);
        put_direct_native_function_without_transition(
            vm,
            global_object,
            prototype,
            &builtin_names.shift_private_name(),
            0,
            array_proto_func_shift_host,
            ImplementationVisibility::Public,
            Intrinsic::ArrayShiftIntrinsic,
            DONT_ENUM | DONT_DELETE | READ_ONLY,
        );
        define(&names.slice, 2, array_proto_func_slice_host, Intrinsic::ArraySliceIntrinsic);
        define(&names.sort, 1, array_proto_func_sort_host, Intrinsic::ArraySortIntrinsic);
        define(&literal(b"splice"), 2, array_proto_func_splice_host, Intrinsic::ArraySpliceIntrinsic);
        define(&literal(b"unshift"), 1, array_proto_func_unshift_host, Intrinsic::ArrayUnshiftIntrinsic);
        builtin(builtin_names.every_public_name(), BuiltinCodeIndex::ArrayPrototypeEveryCode);
        builtin(builtin_names.for_each_public_name(), BuiltinCodeIndex::ArrayPrototypeForEachCode);
        builtin(builtin_names.some_public_name(), BuiltinCodeIndex::ArrayPrototypeSomeCode);
        define(builtin_names.index_of_public_name(), 1, array_proto_func_index_of_host, Intrinsic::ArrayIndexOfIntrinsic);
        define(&literal(b"lastIndexOf"), 1, array_proto_func_last_index_of_host, Intrinsic::NoIntrinsic);
        builtin(builtin_names.filter_public_name(), BuiltinCodeIndex::ArrayPrototypeFilterCode);
        define(&names.flat, 0, array_proto_func_flat_host, Intrinsic::NoIntrinsic);
        builtin(builtin_names.flat_map_public_name(), BuiltinCodeIndex::ArrayPrototypeFlatMapCode);
        builtin(builtin_names.reduce_public_name(), BuiltinCodeIndex::ArrayPrototypeReduceCode);
        builtin(builtin_names.reduce_right_public_name(), BuiltinCodeIndex::ArrayPrototypeReduceRightCode);
        builtin(builtin_names.map_public_name(), BuiltinCodeIndex::ArrayPrototypeMapCode);
        define(builtin_names.keys_public_name(), 0, array_proto_func_keys_host, Intrinsic::ArrayKeysIntrinsic);
        define(builtin_names.entries_public_name(), 0, array_proto_func_entries_host, Intrinsic::ArrayEntriesIntrinsic);
        builtin(builtin_names.find_public_name(), BuiltinCodeIndex::ArrayPrototypeFindCode);
        builtin(builtin_names.find_last_public_name(), BuiltinCodeIndex::ArrayPrototypeFindLastCode);
        builtin(builtin_names.find_index_public_name(), BuiltinCodeIndex::ArrayPrototypeFindIndexCode);
        builtin(builtin_names.find_last_index_public_name(), BuiltinCodeIndex::ArrayPrototypeFindLastIndexCode);
        define(&names.includes, 1, array_proto_func_includes_host, Intrinsic::ArrayIncludesIntrinsic);
        define(&literal(b"copyWithin"), 2, array_proto_func_copy_within_host, Intrinsic::NoIntrinsic);
        builtin(builtin_names.at_public_name(), BuiltinCodeIndex::ArrayPrototypeAtCode);
        define(&names.to_reversed, 0, array_proto_func_to_reversed_host, Intrinsic::NoIntrinsic);
        define(&names.to_sorted, 1, array_proto_func_to_sorted_host, Intrinsic::NoIntrinsic);
        define(&literal(b"toSpliced"), 2, array_proto_func_to_spliced_host, Intrinsic::NoIntrinsic);
        define(&names.with, 2, array_proto_func_with_host, Intrinsic::NoIntrinsic);

        // putDirectWithoutTransition(@xxxPrivateName, getDirect(xxx), ReadOnly), na ordem do C++.
        let alias_private = |private_name: Identifier, public_name: &Identifier| {
            let function = prototype.get_direct_by_name(vm, &PropertyName::from_identifier(public_name));
            prototype.put_direct_without_transition(vm, &PropertyName::from_identifier(&private_name), function, READ_ONLY);
        };
        alias_private(builtin_names.entries_private_name(), builtin_names.entries_public_name());
        alias_private(builtin_names.for_each_private_name(), builtin_names.for_each_public_name());
        alias_private(builtin_names.includes_private_name(), &names.includes);
        alias_private(builtin_names.index_of_private_name(), builtin_names.index_of_public_name());
        alias_private(builtin_names.keys_private_name(), builtin_names.keys_public_name());
        alias_private(builtin_names.map_private_name(), builtin_names.map_public_name());
        alias_private(builtin_names.pop_private_name(), &literal(b"pop"));
        alias_private(builtin_names.values_private_name(), builtin_names.values_public_name());
        crate::runtime::array_prototype_unscopables::put_array_prototype_unscopables(vm, global_object, prototype);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::js_global_object::JSGlobalObjectRef;
    use std::rc::Rc;

    fn global() -> JSGlobalObjectRef {
        JSGlobalObject::init(&Rc::new(VM::new()))
    }

    fn call<'a>(global: &'a JSGlobalObject, this_value: JSValue, args: &'a [JSValue]) -> ArrayCall<'a> {
        ArrayCall { vm: global.vm(), global_object: global, this_value, args, new_target: JSValue::undefined() }
    }

    /// Um array com `values` (`None` é buraco).
    fn array_of(global: &JSGlobalObject, values: &[Option<JSValue>]) -> JSArray {
        let c = call(global, JSValue::undefined(), &[]);
        let array = new_array(&c, values.len() as u64).unwrap();
        for (index, value) in values.iter().enumerate() {
            if let Some(value) = value {
                put_index(&c, &array, index as u64, *value).unwrap();
            }
        }
        array
    }

    /// Os elementos de `array` (`None` é buraco).
    fn read(global: &JSGlobalObject, array: &JSArray) -> Vec<Option<JSValue>> {
        (0..array.length())
            .map(|index| array.has_property_by_index(global.vm(), index).then(|| array.get_by_index(global.vm(), index)))
            .collect()
    }

    fn text(global: &JSGlobalObject, text: &str) -> JSValue {
        JSValue::from_js_string(js_string(global.vm(), &WtfString::from_latin1(text.as_bytes())))
    }

    fn ints(values: &[i32]) -> Vec<Option<JSValue>> {
        values.iter().map(|value| Some(JSValue::Int32(*value))).collect()
    }

    #[test]
    fn default_sort_orders_by_string_with_undefined_then_holes_at_the_end() {
        let global = global();
        let undefined = JSValue::undefined();
        let array = array_of(&global, &[Some(JSValue::Int32(3)), Some(undefined), None, Some(JSValue::Int32(10)), Some(JSValue::Int32(2)), None]);
        let result = array_proto_func_sort(&call(&global, array.as_value(), &[])).unwrap();
        assert_eq!(result, array.as_value());
        // "10" < "2" < "3" por unidade de código; o undefined vem depois, e os buracos por último.
        assert_eq!(
            read(&global, &array),
            vec![Some(JSValue::Int32(10)), Some(JSValue::Int32(2)), Some(JSValue::Int32(3)), Some(undefined), None, None]
        );
    }

    #[test]
    fn default_sort_is_stable_for_equal_strings() {
        let global = global();
        let (s7a, s7b) = (text(&global, "7"), text(&global, "7"));
        let input = [Some(text(&global, "9")), Some(s7a), Some(JSValue::Int32(1)), Some(JSValue::Int32(7)), Some(s7b)];
        let array = array_of(&global, &input);
        array_proto_func_sort(&call(&global, array.as_value(), &[])).unwrap();
        assert_eq!(read(&global, &array), vec![input[2], input[1], input[3], input[4], input[0]]);
    }

    #[test]
    fn default_sort_radix_path_matches_string_order() {
        let global = global();
        let values: Vec<i32> = (0..300).map(|i| (i * 37) % 300).collect();
        let array = array_of(&global, &ints(&values));
        array_proto_func_sort(&call(&global, array.as_value(), &[])).unwrap();
        let mut expected = values.clone();
        expected.sort_by_key(|value| value.to_string());
        assert_eq!(read(&global, &array), ints(&expected));
    }

    #[test]
    fn default_sort_radix_path_handles_two_byte_strings() {
        let global = global();
        let make = |units: &[u16]| JSValue::from_js_string(js_string(global.vm(), &WtfString::from_utf16(units)));
        let values: Vec<Vec<u16>> = (0..40u16).map(|i| vec![0x4E00 + (i * 7) % 40, 0x61 + i % 3]).collect();
        let array = array_of(&global, &values.iter().map(|units| Some(make(units))).collect::<Vec<_>>());
        array_proto_func_sort(&call(&global, array.as_value(), &[])).unwrap();
        let mut expected = values.clone();
        expected.sort();
        let actual = read(&global, &array);
        assert_eq!(actual.len(), expected.len());
        for (value, units) in actual.iter().zip(&expected) {
            let string = element_to_wtf_string(&call(&global, JSValue::undefined(), &[]), value.unwrap()).unwrap();
            assert_eq!(string.span16(), units.as_slice());
        }
    }

    #[test]
    fn sort_with_fewer_than_two_elements_does_nothing() {
        let global = global();
        let array = array_of(&global, &[Some(JSValue::Int32(1))]);
        array_proto_func_sort(&call(&global, array.as_value(), &[])).unwrap();
        assert_eq!(read(&global, &array), vec![Some(JSValue::Int32(1))]);
        let array = array_of(&global, &[None]);
        array_proto_func_sort(&call(&global, array.as_value(), &[])).unwrap();
        assert_eq!(read(&global, &array), vec![None]);
    }

    #[test]
    fn sort_moves_holes_to_the_end() {
        // bun: `[,1].sort()` dá `[1, <buraco>]` (0 in a é true, 1 in a é false).
        let global = global();
        let array = array_of(&global, &[None, Some(JSValue::Int32(1))]);
        array_proto_func_sort(&call(&global, array.as_value(), &[])).unwrap();
        assert_eq!(read(&global, &array), vec![Some(JSValue::Int32(1)), None]);
    }

    #[test]
    fn sort_rejects_a_comparator_that_is_not_callable() {
        let global = global();
        let array = array_of(&global, &ints(&[2, 1]));
        for argument in [JSValue::Int32(1), JSValue::null(), JSValue::Bool(true)] {
            let args = [argument];
            let error = array_proto_func_sort(&call(&global, array.as_value(), &args)).unwrap_err();
            assert!(matches!(error, ArrayError::Put(PutError::TypeError(message)) if message.contains("requires the comparator")));
            let error = array_proto_func_to_sorted(&call(&global, array.as_value(), &args)).unwrap_err();
            assert!(matches!(error, ArrayError::Put(PutError::TypeError(message)) if message.contains("toSorted requires")));
        }
        assert_eq!(read(&global, &array), ints(&[2, 1]));
    }

    #[test]
    fn to_sorted_copies_and_reads_holes_as_undefined() {
        let global = global();
        let undefined = JSValue::undefined();
        let array = array_of(&global, &[Some(JSValue::Int32(2)), None, Some(JSValue::Int32(1))]);
        let result = array_proto_func_to_sorted(&call(&global, array.as_value(), &[])).unwrap();
        let result = JSArray::from_value(&result).expect("toSorted devolve um array");
        assert_eq!(read(&global, &result), vec![Some(JSValue::Int32(1)), Some(JSValue::Int32(2)), Some(undefined)]);
        // O original não muda, e o buraco continua buraco.
        assert_eq!(read(&global, &array), vec![Some(JSValue::Int32(2)), None, Some(JSValue::Int32(1))]);
    }

    /// Cada `JSC_BUILTIN_FUNCTION_WITHOUT_TRANSITION` de `finishCreation` entra pelo `BuiltinCodeIndex` certo,
    /// com o nome público do C++, `length` 1 (o parâmetro `callback`/`index` de `ArrayPrototype.js`), sem
    /// intrínseco e sem `[[Construct]]`.
    #[test]
    fn js_builtins_are_installed_with_name_length_and_intrinsic() {
        use crate::runtime::executable::ExecutableBaseRef;

        let global = global();
        let vm = global.vm();
        let structure = ArrayPrototype::create_structure(vm, &global, global.object_prototype().as_value());
        let prototype = ArrayPrototype::create(vm, &global, &structure);
        let names = vm.property_names.builtin_names();
        let expected: [(&Identifier, &str); 13] = [
            (names.every_public_name(), "every"),
            (names.for_each_public_name(), "forEach"),
            (names.some_public_name(), "some"),
            (names.filter_public_name(), "filter"),
            (names.flat_map_public_name(), "flatMap"),
            (names.reduce_public_name(), "reduce"),
            (names.reduce_right_public_name(), "reduceRight"),
            (names.map_public_name(), "map"),
            (names.find_public_name(), "find"),
            (names.find_last_public_name(), "findLast"),
            (names.find_index_public_name(), "findIndex"),
            (names.find_last_index_public_name(), "findLastIndex"),
            (names.at_public_name(), "at"),
        ];
        assert_eq!(expected.len(), ARRAY_PROTOTYPE_JS_BUILTINS.len());
        for (position, (identifier, text)) in expected.iter().enumerate() {
            assert_eq!(ARRAY_PROTOTYPE_JS_BUILTINS[position], *text);
            let value = prototype.get_direct_by_name(vm, &PropertyName::from_identifier(identifier));
            let function = value.as_js_function().unwrap_or_else(|| panic!("{text} não é uma JSFunction"));
            assert!(!function.is_host_function(), "{text} é builtin JS");
            let executable = function.js_executable();
            assert_eq!(&executable.borrow().name(), *identifier, "nome de {text}");
            assert_eq!(executable.borrow().parameter_count(), 1, "length de {text}");
            let intrinsic = match function.executable() {
                ExecutableBaseRef::Script(script) => script.intrinsic(),
                ExecutableBaseRef::Native(_) => unreachable!("{text} é builtin JS"),
            };
            assert_eq!(intrinsic, Intrinsic::NoIntrinsic, "intrínseco de {text}");
        }
    }
}
