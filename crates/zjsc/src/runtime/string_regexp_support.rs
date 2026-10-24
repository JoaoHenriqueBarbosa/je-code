//! O que `String.prototype` e `RegExp.prototype` usam em comum nas funções que lêem e gravam
//! propriedades de objetos arbitrários (`replace`, `match`, `split`, `@@replace`, `@@split`...): as
//! conversões `toString`/`toLength` com a exceção pendente do `VM`, `get`/`put` de `JSValue` que
//! alcançam as propriedades que o `JSObject` do porte ainda não despacha virtualmente (`lastIndex` de
//! `RegExpObject`, `length` de `JSArray`), `advanceStringIndex` (`RegExpObjectInlines.h`),
//! `createIteratorResultObject` (`IteratorOperations.cpp`) e o `SpeciesConstructor` de `RegExp`.
//!
//! DIVERGÊNCIA: o `JSObject::get` do porte não tem despacho virtual (o `getOwnPropertySlot` de
//! `RegExpObject` e de `JSArray` não é chamado pela base), então `get_object_property` e
//! `set_object_property` tratam à mão os dois nomes que o C++ resolve por override.

use std::rc::Rc;

use crate::runtime::call_data::{construct_with_error_message, get_construct_data};
use crate::runtime::error::create_type_error;
use crate::runtime::exception_helpers::error_description_for_value;
use crate::runtime::host_call::Thrown;
use crate::runtime::identifier::Identifier;
use crate::runtime::iterator_operations::thrown_from_llint;
use crate::runtime::js_array::JSArray;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSFinalObject;
use crate::runtime::js_string::{js_string, JSStringRef};
use crate::runtime::js_value::{js_boolean, JSValue};
use crate::runtime::property_name::PropertyName;
use crate::runtime::put_property_slot::{PutContext, PutPropertySlot};
use crate::runtime::reg_exp_object::RegExpObject;
use crate::runtime::string_prototype::{code_units, find_units, string_from_units};
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `RETURN_IF_EXCEPTION(scope, ...)`.
pub fn check_exception(global_object: &JSGlobalObject) -> Result<(), Thrown> {
    if global_object.vm().exception().is_some() { Err(Thrown::Pending) } else { Ok(()) }
}

/// `value.toString(globalObject)`: `Err(Pending)` se a conversão lançou.
pub fn to_string_value(global_object: &JSGlobalObject, value: JSValue) -> Result<JSStringRef, Thrown> {
    let string = value.to_string(global_object.vm());
    check_exception(global_object)?;
    Ok(string)
}

/// `value.toWTFString(globalObject)`.
pub fn to_wtf_string_value(global_object: &JSGlobalObject, value: JSValue) -> Result<WtfString, Thrown> {
    Ok(to_string_value(global_object, value)?.value())
}

/// `value.toUInt32(globalObject)` com o `RETURN_IF_EXCEPTION`.
pub fn to_uint32_value(global_object: &JSGlobalObject, value: JSValue) -> Result<u32, Thrown> {
    let integer = value.to_uint32();
    check_exception(global_object)?;
    Ok(integer)
}

/// `asObject(object)->get(globalObject, propertyName)`: `object` já é um objeto.
pub fn get_object_property(global_object: &JSGlobalObject, object: JSValue, name: &Identifier) -> Result<JSValue, Thrown> {
    let vm = global_object.vm();
    if *name == vm.property_names.last_index {
        if let Some(reg_exp) = RegExpObject::from_cell_id(object.as_cell()) {
            return Ok(reg_exp.get_last_index());
        }
    }
    if *name == vm.property_names.length {
        if let Some(array) = JSArray::from_value(&object) {
            return Ok(JSValue::from_u32(array.length()));
        }
    }
    let object = object.as_object();
    let value = object.get(global_object, &PropertyName::from_identifier(name));
    check_exception(global_object)?;
    Ok(value)
}

/// `asObject(object)->get(globalObject, index)`.
pub fn get_object_index(global_object: &JSGlobalObject, object: JSValue, index: u32) -> Result<JSValue, Thrown> {
    let vm = global_object.vm();
    if let Some(array) = JSArray::from_value(&object) {
        let value = array.get_by_index(vm, index);
        check_exception(global_object)?;
        return Ok(value);
    }
    let object = object.as_object();
    let value = object.get_by_index(vm, index);
    check_exception(global_object)?;
    Ok(value)
}

/// `asObject(object)->get(globalObject, index)` com um índice de `uint64_t` (o `get(globalObject,
/// Identifier::from(vm, index))` do C++ acima de `MAX_ARRAY_INDEX`).
pub fn get_object_index_u64(global_object: &JSGlobalObject, object: JSValue, index: u64) -> Result<JSValue, Thrown> {
    if let Some(index) = u32::try_from(index).ok().filter(|&index| index <= crate::runtime::identifier::MAX_ARRAY_INDEX) {
        return get_object_index(global_object, object, index);
    }
    let vm = global_object.vm();
    let name = Identifier::from_span(vm, index.to_string().as_bytes());
    let value = object.as_object().get(global_object, &PropertyName::from_identifier(&name));
    check_exception(global_object)?;
    Ok(value)
}

/// `Set(object, propertyName, value, true)`: o `put` com `shouldThrow`.
pub fn set_object_property(global_object: &JSGlobalObject, object: JSValue, name: &Identifier, value: JSValue) -> Result<(), Thrown> {
    let vm = global_object.vm();
    if *name == vm.property_names.last_index {
        if let Some(reg_exp) = RegExpObject::from_cell_id(object.as_cell()) {
            reg_exp.set_last_index(value, true)?;
            return Ok(());
        }
    }
    let target = object.as_object();
    let name = PropertyName::from_identifier(name);
    let mut slot = PutPropertySlot::new(object, true, PutContext::UnknownContext, false);
    let Some(lookup) = target.for_property_lookup(global_object, &name) else {
        return Err(Thrown::Pending);
    };
    // `JSObject::put` despacha o `Proxy` na entrada (o `ProxyObject::put` do C++ é virtual).
    lookup.put(vm, &name, value, &mut slot)?;
    check_exception(global_object)
}

/// O `flags` (ou outro texto) contém `unit` (`String::contains(char)`).
pub fn contains_unit(text: &WtfString, unit: u8) -> bool {
    code_units(text).contains(&u16::from(unit))
}

/// O `JSValue` de uma `JSString`.
pub fn string_value(string: &JSStringRef) -> JSValue {
    JSValue::from_js_string(Rc::clone(string))
}

/// O `JSValue` de um texto em unidades UTF-16 (`jsString(vm, String)`).
pub fn units_value(vm: &VM, units: &[u16]) -> JSValue {
    JSValue::from_js_string(js_string(vm, &string_from_units(units)))
}

/// `find(character, from)` sobre unidades UTF-16.
pub fn find_unit(units: &[u16], unit: u8, from: usize) -> Option<usize> {
    find_units(units, &[u16::from(unit)], from)
}

/// `advanceStringIndex(str, strSize, index, isUnicode)` (`RegExpObjectInlines.h`).
pub fn advance_string_index(units: &[u16], index: u64, is_unicode: bool) -> u64 {
    if !is_unicode {
        return index + 1;
    }
    // `advanceStringUnicode`: passa o par substituto inteiro.
    let size = units.len() as u64;
    if index + 1 >= size {
        return index + 1;
    }
    let first = units[index as usize];
    if !(0xD800..0xDC00).contains(&first) {
        return index + 1;
    }
    let second = units[index as usize + 1];
    if !(0xDC00..0xE000).contains(&second) {
        return index + 1;
    }
    index + 2
}

/// `createIteratorResultObject(globalObject, value, done)`: `{ value, done }`.
pub fn create_iterator_result_object(global_object: &JSGlobalObject, value: JSValue, done: bool) -> JSValue {
    let vm = global_object.vm();
    let object = JSFinalObject::create(vm, &global_object.object_structure_for_object_constructor());
    object.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.value), value, 0);
    object.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.done), js_boolean(done), 0);
    object.as_value()
}

/// `SpeciesConstructor(thisObject, %RegExp%)` como `regExpSplitSlow` e `regExpProtoFuncMatchAll` o
/// fazem (as mesmas mensagens): `Err` com a exceção pendente ou o `TypeError`.
pub fn reg_exp_species_constructor(global_object: &JSGlobalObject, this_object: JSValue) -> Result<JSValue, Thrown> {
    let vm = global_object.vm();
    let default_constructor = global_object.reg_exp_constructor();
    let constructor_value = get_object_property(global_object, this_object, &vm.property_names.constructor)?;
    if constructor_value.is_undefined() {
        return Ok(default_constructor);
    }
    if !constructor_value.is_object() {
        return Err(Thrown::type_error("|this|.constructor is not an Object or undefined"));
    }
    let species = get_object_property(global_object, constructor_value, &vm.property_names.species_symbol)?;
    if species.is_undefined_or_null() {
        return Ok(default_constructor);
    }
    // `isConstructor()`.
    if !get_construct_data(species).is_none() {
        return Ok(species);
    }
    Err(Thrown::type_error("|this|.constructor[Symbol.species] is not a constructor"))
}

/// `construct(globalObject, constructor, args)` com a exceção pendente como `Err`.
pub fn construct_value(global_object: &JSGlobalObject, constructor: JSValue, args: &[JSValue]) -> Result<JSValue, Thrown> {
    construct_with_error_message(global_object, constructor, args, "Type error").map_err(thrown_from_llint)
}

/// `throwTypeError(globalObject, scope, makeString(errorDescriptionForValue(value), " is not a function"_s))`:
/// a mensagem leva o texto da descrição inteiro (inclusive fora do Latin-1), então o erro é lançado aqui
/// e o `Thrown::Pending` devolvido.
pub fn not_a_function_error(global_object: &JSGlobalObject, value: JSValue) -> Thrown {
    let mut units = code_units(&error_description_for_value(value)).into_owned();
    units.extend(" is not a function".encode_utf16());
    let mut scope = ThrowScope::new(global_object.vm());
    throw_exception(global_object, &mut scope, create_type_error(global_object, &string_from_units(&units)));
    Thrown::Pending
}
