//! Porte do caminho genérico de `runtime/IteratorOperations.{h,cpp}`: `iteratorForIterable`,
//! `iteratorNext`, `iteratorComplete`, `iteratorValue`, `iteratorStep`, `iteratorClose` e
//! `forEachInIterable` (o laço de `forEachInIterationRecord`).
//!
//! DIVERGÊNCIAS e LACUNAS:
//! - Os atalhos de `JSMap`/`JSSet` com `isIteratorProtocolFastAndNonObservable` e o `CachedCall` não
//!   existem; os de `getIterationMode` (`FastArray`, `FastMap`, `FastSet`, `FastString` e os de iterador
//!   aberto) existem, com a conferência direta do `next` do protótipo no lugar dos watchpoints (ver
//!   `iteration_protocol.rs`). O caminho genérico é o observável (chama `@@iterator`, `next`, `done`,
//!   `value` e `return`).
//! - `createAsyncFromSyncIterator` e `createAsyncFromSyncIteratorForIterable` seguem o
//!   `getIterationMode`/`fastSyncIteratorForIterable` do C++.
//! - O `callback` devolve `Result<(), Thrown>`; um `Err` é lançado (`throw_thrown`) e em seguida o
//!   `iteratorClose` roda como no C++ (`if (scope.exception()) { iteratorClose(...) }`), preservando a
//!   exceção original.
//! - `JSValue::get` de primitivo (string iterável, número sem `@@iterator`) busca no protótipo do
//!   wrapper (`toObject`) com o primitivo como receptor. `undefined` e `null` lançam
//!   `createNotAnObjectError` como o C++.

use std::rc::Rc;

use crate::bytecode::op_metadata::IterationMode;
use crate::llint::LLIntFailure;
use crate::runtime::call_data::{call, get_call_data};
use crate::runtime::js_async_from_sync_iterator::{JSAsyncFromSyncIterator, JSAsyncFromSyncIteratorRef};
use crate::runtime::js_object::{JSObject, JSObjectHandle};
use crate::runtime::iteration_kind::IterationKind;
use crate::runtime::js_array::{is_js_array, JSArray};
use crate::runtime::js_array_iterator::JSArrayIterator;
use crate::runtime::js_map::{JSMap, JSMapIterator};
use crate::runtime::js_set::{JSSet, JSSetIterator};
use crate::runtime::js_string_iterator::JSStringIterator;
use crate::runtime::exception::Exception;
use crate::runtime::exception_helpers::create_not_an_object_error;
use crate::runtime::host_call::{throw_thrown, HostCall, HostResult, Thrown};
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction, JSFunctionRef};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfStringValue;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::property_offset::PropertyOffset;
use crate::runtime::throw_scope::{throw_exception, ThrowScope};

/// `iteratorResultObjectValuePropertyOffset`: o deslocamento de `value` nos objetos com a estrutura de
/// resultado de iterador (`createIteratorResultObject`, que põe `value` e depois `done`).
pub const ITERATOR_RESULT_OBJECT_VALUE_PROPERTY_OFFSET: PropertyOffset = 0;

/// `iteratorResultObjectDonePropertyOffset`.
pub const ITERATOR_RESULT_OBJECT_DONE_PROPERTY_OFFSET: PropertyOffset = 1;

/// O `LLIntFailure` de `call`/`construct` como o que a função nativa lança: a exceção JS já está
/// pendente (`Pending`) ou é a lacuna do porte.
pub fn thrown_from_llint(failure: LLIntFailure) -> Thrown {
    match failure {
        LLIntFailure::Thrown => Thrown::Pending,
        LLIntFailure::Unported(what) => Thrown::Unported(what),
        LLIntFailure::UnportedOpcode(_) => Thrown::Unported("opcode sem handler no interpretador"),
    }
}

/// `value.get(globalObject, propertyName)` sobre um `JSValue`: `Err(Pending)` se o getter lançou.
pub fn get_value_property(global_object: &JSGlobalObject, value: JSValue, property_name: &PropertyName) -> Result<JSValue, Thrown> {
    let vm = global_object.vm();
    if value.is_undefined_or_null() {
        let mut scope = ThrowScope::new(vm);
        throw_exception(global_object, &mut scope, create_not_an_object_error(global_object, value));
        return Err(Thrown::Pending);
    }
    // `JSValue::get` de primitivo: a busca anda pelo protótipo do wrapper (`toObject`), mas o receptor
    // do getter continua sendo o primitivo (`PropertySlot(thisValue, ...)`).
    let result = if value.is_object() {
        value.as_object().get(global_object, property_name)
    } else {
        let wrapper = value.to_object(global_object).ok_or(Thrown::Pending)?;
        let mut slot = PropertySlot::new(value, InternalMethodType::Get);
        if wrapper.get_property_slot(global_object, property_name, &mut slot) {
            slot.get_value_for(property_name)
        } else {
            JSValue::undefined()
        }
    };
    if vm.exception().is_some() {
        return Err(Thrown::Pending);
    }
    Ok(result)
}

/// `call(globalObject, function, callData, thisValue, args)` com a checagem de `callData` do C++
/// (`getCallDataInline` + `throwTypeError(globalObject, scope, message)` se não é chamável).
pub fn call_checked(
    global_object: &JSGlobalObject,
    function: JSValue,
    this_value: JSValue,
    args: &[JSValue],
    not_callable_message: &str,
) -> Result<JSValue, Thrown> {
    let call_data = get_call_data(function);
    if call_data.is_none() {
        return Err(Thrown::type_error(not_callable_message));
    }
    call(global_object, function, &call_data, this_value, args).map_err(thrown_from_llint)
}

/// `struct IterationRecord { JSValue iterator; JSValue nextMethod; }`.
#[derive(Clone, Copy, Debug)]
pub struct IterationRecord {
    pub iterator: JSValue,
    pub next_method: JSValue,
}

/// `iteratorForIterable(globalObject, iterable)`.
pub fn iterator_for_iterable(global_object: &JSGlobalObject, iterable: JSValue) -> Result<IterationRecord, Thrown> {
    let vm = global_object.vm();
    let iterator_function = get_value_property(global_object, iterable, &PropertyName::from_identifier(&vm.property_names.iterator_symbol))?;
    iterator_for_iterable_with_method(global_object, iterable, iterator_function)
}

/// `iteratorForIterable(globalObject, object, iteratorMethod)`: o `@@iterator` já foi lido pelo chamador.
pub fn iterator_for_iterable_with_method(
    global_object: &JSGlobalObject,
    iterable: JSValue,
    iterator_function: JSValue,
) -> Result<IterationRecord, Thrown> {
    let vm = global_object.vm();
    let iterator = call_checked(global_object, iterator_function, iterable, &[], "Type error")?;
    if !iterator.is_object() {
        return Err(Thrown::type_error("Type error"));
    }
    let next_method = get_value_property(global_object, iterator, &PropertyName::from_identifier(&vm.property_names.next))?;
    Ok(IterationRecord { iterator, next_method })
}

/// `iteratorDirect(globalObject, object)`: o próprio objeto é o iterador, e `next` vem dele.
pub fn iterator_direct(global_object: &JSGlobalObject, object: JSValue) -> Result<IterationRecord, Thrown> {
    let vm = global_object.vm();
    let next_method = get_value_property(global_object, object, &PropertyName::from_identifier(&vm.property_names.next))?;
    Ok(IterationRecord { iterator: object, next_method })
}

/// `iteratorNext(globalObject, iterationRecord, argument)`: o resultado tem de ser um objeto.
pub fn iterator_next(global_object: &JSGlobalObject, record: IterationRecord) -> Result<JSValue, Thrown> {
    let result = call_checked(global_object, record.next_method, record.iterator, &[], "Type error")?;
    if !result.is_object() {
        return Err(Thrown::type_error("Iterator result interface is not an object."));
    }
    Ok(result)
}

/// `iteratorComplete(globalObject, iterResult)`.
pub fn iterator_complete(global_object: &JSGlobalObject, iter_result: JSValue) -> Result<bool, Thrown> {
    let vm = global_object.vm();
    let done = get_value_property(global_object, iter_result, &PropertyName::from_identifier(&vm.property_names.done))?;
    Ok(done.to_boolean())
}

/// `iteratorValue(globalObject, iterResult)`.
pub fn iterator_value(global_object: &JSGlobalObject, iter_result: JSValue) -> Result<JSValue, Thrown> {
    let vm = global_object.vm();
    get_value_property(global_object, iter_result, &PropertyName::from_identifier(&vm.property_names.value))
}

/// `iteratorStep(globalObject, iterationRecord)`: `None` é o `jsBoolean(false)` (terminou).
pub fn iterator_step(global_object: &JSGlobalObject, record: IterationRecord) -> Result<Option<JSValue>, Thrown> {
    let result = iterator_next(global_object, record)?;
    if iterator_complete(global_object, result)? {
        return Ok(None);
    }
    Ok(Some(result))
}

/// `iteratorClose(globalObject, iterator)`: guarda a exceção pendente, chama `iterator.return` e a
/// relança por cima de qualquer outra (a original vence), como o C++.
pub fn iterator_close(global_object: &JSGlobalObject, iterator: JSValue) {
    let vm = global_object.vm();
    let exception: Option<Rc<Exception>> = vm.exception();
    if exception.is_some() {
        vm.clear_exception();
    }
    let rethrow = |exception: Option<Rc<Exception>>| {
        if let Some(exception) = exception {
            let mut scope = ThrowScope::new(vm);
            throw_exception(global_object, &mut scope, exception);
        }
    };

    let return_function = match get_value_property(global_object, iterator, &PropertyName::from_identifier(&vm.property_names.return_keyword)) {
        Ok(value) => value,
        Err(thrown) => {
            // Exceção de `get`: a original, se houver, a substitui.
            if exception.is_some() {
                vm.clear_exception();
                rethrow(exception);
            } else {
                throw_thrown(global_object, thrown);
            }
            return;
        }
    };

    if return_function.is_undefined_or_null() {
        rethrow(exception);
        return;
    }

    let call_data = get_call_data(return_function);
    if call_data.is_none() {
        if exception.is_some() {
            rethrow(exception);
        } else {
            throw_thrown(global_object, Thrown::type_error("Type error"));
        }
        return;
    }

    let inner_result = call(global_object, return_function, &call_data, iterator, &[]);
    if exception.is_some() {
        vm.clear_exception();
        rethrow(exception);
        return;
    }
    match inner_result {
        Err(failure) => throw_thrown(global_object, thrown_from_llint(failure)),
        Ok(value) if !value.is_object() => {
            throw_thrown(global_object, Thrown::type_error("Iterator result interface is not an object."));
        }
        Ok(_) => {}
    }
}

/// `forEachInIterationRecord(globalObject, iterationRecord, callback)`.
pub fn for_each_in_iteration_record(
    global_object: &JSGlobalObject,
    record: IterationRecord,
    mut callback: impl FnMut(JSValue) -> Result<(), Thrown>,
) -> Result<(), Thrown> {
    // O iterador e o `next` só existem neste local Rust enquanto o JS do laço roda (a pilha C++ do JSC, que o
    // coletor varre de modo conservador).
    let _record = crate::runtime::gc::RootScope::new(&[record.iterator, record.next_method]);
    loop {
        let Some(next) = iterator_step(global_object, record)? else {
            return Ok(());
        };
        let next_value = iterator_value(global_object, next)?;
        if let Err(thrown) = callback(next_value) {
            throw_thrown(global_object, thrown);
            iterator_close(global_object, record.iterator);
            return Err(Thrown::Pending);
        }
    }
}

/// `forEachInIteratorProtocol(globalObject, iterable, callback)`, no caminho genérico: `iterable` já é
/// o iterador (`iteratorDirect`), sem chamar `@@iterator`.
pub fn for_each_in_iterator_protocol(
    global_object: &JSGlobalObject,
    iterable: JSValue,
    callback: impl FnMut(JSValue) -> Result<(), Thrown>,
) -> Result<(), Thrown> {
    let record = iterator_direct(global_object, iterable)?;
    for_each_in_iteration_record(global_object, record, callback)
}

/// `forEachInIterable(globalObject, iterable, callback)`, no caminho genérico (ver o cabeçalho).
pub fn for_each_in_iterable(
    global_object: &JSGlobalObject,
    iterable: JSValue,
    callback: impl FnMut(JSValue) -> Result<(), Thrown>,
) -> Result<(), Thrown> {
    let vm = global_object.vm();
    let iterator_method = get_value_property(global_object, iterable, &PropertyName::from_identifier(&vm.property_names.iterator_symbol))?;
    for_each_in_iterable_with_method_unchecked(global_object, iterable, iterator_method, callback)
}

/// `forEachInIterable(globalObject, iterable, iteratorMethod, callback)`: o `@@iterator` já foi lido pelo
/// chamador.
pub fn for_each_in_iterable_with_method(
    global_object: &JSGlobalObject,
    iterable: JSValue,
    iterator_method: JSValue,
    callback: impl FnMut(JSValue) -> Result<(), Thrown>,
) -> Result<(), Thrown> {
    // `validateIterable` + `getIteratorErrorMessage`: o `@@iterator` não chamável diz `{} is not iterable`
    // (e não o `Type error` do `iteratorForIterable`).
    if get_call_data(iterator_method).is_none() {
        return Err(Thrown::type_error(&crate::llint::handlers_iterator::not_iterable_message(iterable)));
    }
    for_each_in_iterable_with_method_unchecked(global_object, iterable, iterator_method, callback)
}

/// O corpo do `forEachInIterable` com o `@@iterator` lido: o `IterationMode::FastArray` percorre os índices
/// sem criar o iterador nem chamar `next`. Se o callback falha, o `iteratorClose` roda sobre um `JSArrayIterator`
/// criado na hora, posicionado depois do elemento: o `return` de `%ArrayIteratorPrototype%` pode ter sido definido
/// pelo usuário (o iterador de array primordial não tem), e o JSC o chama.
fn for_each_in_iterable_with_method_unchecked(
    global_object: &JSGlobalObject,
    iterable: JSValue,
    iterator_method: JSValue,
    mut callback: impl FnMut(JSValue) -> Result<(), Thrown>,
) -> Result<(), Thrown> {
    if get_iteration_mode(global_object, iterable, iterator_method) == IterationMode::FastArray {
        let array = JSArray::from_value(&iterable).expect("isJSArray(iterable)");
        let vm = global_object.vm();
        let mut index = 0;
        while index < array.length() {
            let value = array.get_by_index(vm, index);
            if vm.exception().is_some() {
                return Err(Thrown::Pending);
            }
            if let Err(thrown) = callback(value) {
                throw_thrown(global_object, thrown);
                let iterated_object = JSObject::from_value(&iterable).expect("isJSArray(iterable)");
                let iterator = JSArrayIterator::create(vm, &global_object.array_iterator_structure(), &iterated_object, IterationKind::Values);
                iterator.set_index(i64::from(index) + 1);
                let _iterator = crate::runtime::gc::RootScope::new(&[iterator.as_value()]);
                iterator_close(global_object, iterator.as_value());
                return Err(Thrown::Pending);
            }
            index += 1;
        }
        return Ok(());
    }
    let record = iterator_for_iterable_with_method(global_object, iterable, iterator_method)?;
    for_each_in_iteration_record(global_object, record, callback)
}

/// `getIterationMode(vm, globalObject, iterable, symbolIterator)`: o `IterationMode` rápido que o par
/// iterável e `@@iterator` permite, ou `Generic`. Os watchpoints do C++ são a conferência direta do `next`
/// do protótipo do iterador (ver `iteration_protocol.rs`); o `@@iterator` é comparado por identidade com o
/// original, e o iterador já aberto só vale com a `Structure` primordial do realm.
pub fn get_iteration_mode(global_object: &JSGlobalObject, iterable: JSValue, symbol_iterator: JSValue) -> IterationMode {
    if !iterable.is_cell() {
        return IterationMode::Generic;
    }
    let is_iterator_proto_symbol_iterator = symbol_iterator == global_object.iterator_proto_symbol_iterator_function();

    if is_js_array(&iterable) {
        if !global_object.array_iterator_protocol_is_intact() || symbol_iterator != global_object.array_proto_values_function().as_value() {
            return IterationMode::Generic;
        }
        return IterationMode::FastArray;
    }

    if let Some(array_iterator) = JSArrayIterator::from_value(&iterable) {
        if !global_object.array_iterator_protocol_is_intact() || !is_iterator_proto_symbol_iterator {
            return IterationMode::Generic;
        }
        if array_iterator.structure().id() != global_object.array_iterator_structure().id() {
            return IterationMode::Generic;
        }
        // As vias rápidas exigem um `JSArray` puro (typed array e `arguments` seguem outro caminho).
        if !is_js_array(&array_iterator.iterated_object()) {
            return IterationMode::Generic;
        }
        return match array_iterator.kind() {
            IterationKind::Values => IterationMode::FastArrayValues,
            IterationKind::Keys => IterationMode::FastArrayKeys,
            IterationKind::Entries => IterationMode::FastArrayEntries,
        };
    }

    if JSMap::from_value(&iterable).is_some() {
        if !global_object.map_iterator_protocol_is_intact() || symbol_iterator != global_object.map_proto_entries_function().as_value() {
            return IterationMode::Generic;
        }
        return IterationMode::FastMap;
    }

    if let Some(map_iterator) = JSMapIterator::from_value(&iterable) {
        if !global_object.map_iterator_protocol_is_intact()
            || !is_iterator_proto_symbol_iterator
            || map_iterator.structure().id() != global_object.map_iterator_structure().id()
        {
            return IterationMode::Generic;
        }
        return match map_iterator.kind() {
            IterationKind::Keys => IterationMode::FastMapKeys,
            IterationKind::Values => IterationMode::FastMapValues,
            IterationKind::Entries => IterationMode::FastMapEntries,
        };
    }

    if JSSet::from_value(&iterable).is_some() {
        if !global_object.set_iterator_protocol_is_intact() || symbol_iterator != global_object.set_proto_values_function().as_value() {
            return IterationMode::Generic;
        }
        return IterationMode::FastSet;
    }

    if let Some(set_iterator) = JSSetIterator::from_value(&iterable) {
        if !global_object.set_iterator_protocol_is_intact()
            || !is_iterator_proto_symbol_iterator
            || set_iterator.structure().id() != global_object.set_iterator_structure().id()
        {
            return IterationMode::Generic;
        }
        return match set_iterator.kind() {
            IterationKind::Values | IterationKind::Keys => IterationMode::FastSetValues,
            IterationKind::Entries => IterationMode::FastSetEntries,
        };
    }

    if iterable.is_string() {
        if !global_object.string_iterator_protocol_is_intact()
            || symbol_iterator != global_object.string_proto_symbol_iterator_function()
        {
            return IterationMode::Generic;
        }
        return IterationMode::FastString;
    }

    IterationMode::Generic
}

/// `fastSyncIteratorForIterable(vm, globalObject, iterable, symbolIterator, reuseMode)`: o iterador primordial
/// que o `@@iterator` devolveria (criado direto) ou o próprio iterável quando já é um iterador rápido (então
/// o modo vai em `reuse_mode`). `None` é o `nullptr`: o modo é `Generic`, `FastAsyncGenerator` ou
/// `AsyncFromSync`.
fn fast_sync_iterator_for_iterable(
    global_object: &JSGlobalObject,
    iterable: JSValue,
    symbol_iterator: JSValue,
    reuse_mode: &mut Option<IterationMode>,
) -> Option<JSObjectHandle> {
    let vm = global_object.vm();
    let mode = get_iteration_mode(global_object, iterable, symbol_iterator);
    let created = match mode {
        IterationMode::FastArray => {
            let array = JSObject::from_value(&iterable).expect("isJSArray(iterable)");
            JSArrayIterator::create(vm, &global_object.array_iterator_structure(), &array, IterationKind::Values).as_value()
        }
        IterationMode::FastMap => {
            let map = JSMap::from_value(&iterable).expect("FastMap sem JSMap");
            JSMapIterator::create(vm, &global_object.map_iterator_structure(), &map, IterationKind::Entries).as_value()
        }
        IterationMode::FastSet => {
            let set = JSSet::from_value(&iterable).expect("FastSet sem JSSet");
            JSSetIterator::create(vm, &global_object.set_iterator_structure(), &set, IterationKind::Values).as_value()
        }
        IterationMode::FastString => {
            // Um contêiner primitivo: o `@@iterator` aloca um `JSStringIterator` primordial novo.
            JSStringIterator::create(vm, &global_object.string_iterator_structure(), &iterable.as_js_string()).as_value()
        }
        // Iteradores: `@@iterator` devolve o próprio iterador, que é reaproveitado.
        IterationMode::FastArrayValues
        | IterationMode::FastArrayKeys
        | IterationMode::FastArrayEntries
        | IterationMode::FastMapKeys
        | IterationMode::FastMapValues
        | IterationMode::FastMapEntries
        | IterationMode::FastSetValues
        | IterationMode::FastSetEntries => {
            *reuse_mode = Some(mode);
            iterable
        }
        IterationMode::FastAsyncGenerator | IterationMode::AsyncFromSync | IterationMode::Generic => return None,
    };
    Some(JSObject::from_value(&created).expect("iterador rápido é objeto"))
}


/// `createAsyncFromSyncIterator(globalObject, syncIterator, knownMode)`: o modo vem de `known_mode` ou de
/// `getIterationMode(syncIterator, %IteratorPrototype%[@@iterator])`; só o `Generic` lê `next` (os modos
/// rápidos dirigem o iterador sem chamar o `next`).
pub fn create_async_from_sync_iterator(
    global_object: &JSGlobalObject,
    sync_iterator: &JSObject,
    known_mode: Option<IterationMode>,
) -> Result<JSAsyncFromSyncIteratorRef, Thrown> {
    let vm = global_object.vm();
    let iteration_mode = known_mode.unwrap_or_else(|| {
        get_iteration_mode(global_object, sync_iterator.as_value(), global_object.iterator_proto_symbol_iterator_function())
    });
    let next_method = if iteration_mode == IterationMode::Generic {
        get_value_property(global_object, sync_iterator.as_value(), &PropertyName::from_identifier(&vm.property_names.next))?
    } else {
        JSValue::undefined()
    };
    Ok(JSAsyncFromSyncIterator::create(
        vm,
        &global_object.async_from_sync_iterator_structure(),
        sync_iterator,
        next_method,
        iteration_mode,
    ))
}

/// `createAsyncFromSyncIteratorForIterable(globalObject, iterable)`: lê `iterable[@@iterator]` (o
/// primitivo é encaixotado pelo `get`), tenta o iterador rápido (`fastSyncIteratorForIterable`) e, sem ele,
/// chama, confere que devolveu objeto e o embrulha.
pub fn create_async_from_sync_iterator_for_iterable(
    global_object: &JSGlobalObject,
    iterable: JSValue,
) -> Result<JSAsyncFromSyncIteratorRef, Thrown> {
    let vm = global_object.vm();
    let sync_method = get_value_property(global_object, iterable, &PropertyName::from_identifier(&vm.property_names.iterator_symbol))?;

    let mut reuse_mode = None;
    if let Some(fast_iterator) = fast_sync_iterator_for_iterable(global_object, iterable, sync_method, &mut reuse_mode) {
        return create_async_from_sync_iterator(global_object, &fast_iterator, reuse_mode);
    }

    let iterator = call_checked(global_object, sync_method, iterable, &[], "iterable should have an iterator symbol")?;
    let iterator_object = JSObject::from_value(&iterator).ok_or_else(|| Thrown::type_error("iterator method should return an object"))?;
    create_async_from_sync_iterator(global_object, &iterator_object, None)
}

/// `asyncFromSyncIteratorCreatePrivate`: só objeto pode ser embrulhado.
fn async_from_sync_iterator_create_private(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let sync_iterator = JSObject::from_value(&call.argument(0))
        .ok_or_else(|| Thrown::type_error("Only objects can be wrapped by async-from-sync wrapper"))?;
    Ok(create_async_from_sync_iterator(global_object, &sync_iterator, None)?.as_value())
}
crate::host_function!(async_from_sync_iterator_create_private_host, async_from_sync_iterator_create_private);

/// A `JSFunction` de `m_linkTimeConstants[LinkTimeConstant::asyncFromSyncIteratorCreate]`
/// (`JSFunction::create(vm, owner, 1, "asyncFromSyncIteratorCreate", asyncFromSyncIteratorCreatePrivate,
/// ImplementationVisibility::Private, NoIntrinsic)`).
pub fn create_async_from_sync_iterator_create_function(vm: &VM, global_object: &JSGlobalObject) -> JSFunctionRef {
    JSFunction::create_native(
        vm,
        global_object,
        1,
        &WtfStringValue::from_latin1(b"asyncFromSyncIteratorCreate"),
        async_from_sync_iterator_create_private_host,
        ImplementationVisibility::Private,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::eval::new_global_object;
    use crate::runtime::js_string::js_string;
    use crate::wtf::text::wtf_string::String as WtfString;

    fn new_map(global_object: &JSGlobalObject) -> JSValue {
        let map = JSMap::create(global_object.vm(), &global_object.map_structure());
        map.set(JSValue::Int32(1), JSValue::Int32(10));
        map.as_value()
    }

    fn new_set(global_object: &JSGlobalObject) -> JSValue {
        let set = JSSet::create(global_object.vm(), &global_object.set_structure());
        set.add(JSValue::Int32(1));
        set.as_value()
    }

    fn map_iterator(global_object: &JSGlobalObject, kind: IterationKind) -> JSValue {
        let map = JSMap::from_value(&new_map(global_object)).unwrap();
        JSMapIterator::create(global_object.vm(), &global_object.map_iterator_structure(), &map, kind).as_value()
    }

    fn set_iterator(global_object: &JSGlobalObject, kind: IterationKind) -> JSValue {
        let set = JSSet::from_value(&new_set(global_object)).unwrap();
        JSSetIterator::create(global_object.vm(), &global_object.set_iterator_structure(), &set, kind).as_value()
    }

    #[test]
    fn container_modes_need_the_original_symbol_iterator() {
        let (vm, global_object) = new_global_object();
        let map = new_map(&global_object);
        assert_eq!(get_iteration_mode(&global_object, map, global_object.map_proto_entries_function().as_value()), IterationMode::FastMap);
        assert_eq!(get_iteration_mode(&global_object, map, global_object.set_proto_values_function().as_value()), IterationMode::Generic);

        let set = new_set(&global_object);
        assert_eq!(get_iteration_mode(&global_object, set, global_object.set_proto_values_function().as_value()), IterationMode::FastSet);
        assert_eq!(get_iteration_mode(&global_object, set, JSValue::undefined()), IterationMode::Generic);

        let string = JSValue::from_js_string(js_string(&vm, &WtfString::from_latin1(b"ab")));
        assert_eq!(get_iteration_mode(&global_object, string, global_object.string_proto_symbol_iterator_function()), IterationMode::FastString);
        assert_eq!(get_iteration_mode(&global_object, JSValue::Int32(1), global_object.string_proto_symbol_iterator_function()), IterationMode::Generic);
    }

    #[test]
    fn opened_iterators_map_their_kind_to_a_mode() {
        let (_vm, global_object) = new_global_object();
        let symbol_iterator = global_object.iterator_proto_symbol_iterator_function();
        for (kind, expected) in [
            (IterationKind::Keys, IterationMode::FastMapKeys),
            (IterationKind::Values, IterationMode::FastMapValues),
            (IterationKind::Entries, IterationMode::FastMapEntries),
        ] {
            assert_eq!(get_iteration_mode(&global_object, map_iterator(&global_object, kind), symbol_iterator), expected);
        }
        // `keys` e `values` de `Set` são o mesmo modo.
        for (kind, expected) in [
            (IterationKind::Keys, IterationMode::FastSetValues),
            (IterationKind::Values, IterationMode::FastSetValues),
            (IterationKind::Entries, IterationMode::FastSetEntries),
        ] {
            assert_eq!(get_iteration_mode(&global_object, set_iterator(&global_object, kind), symbol_iterator), expected);
        }
        // O `@@iterator` que não é o de `%IteratorPrototype%` derruba o modo.
        assert_eq!(
            get_iteration_mode(&global_object, map_iterator(&global_object, IterationKind::Keys), JSValue::undefined()),
            IterationMode::Generic
        );
    }

    #[test]
    fn patched_iterator_prototype_next_invalidates_the_protocol() {
        let (vm, global_object) = new_global_object();
        let symbol_iterator = global_object.iterator_proto_symbol_iterator_function();
        let map = new_map(&global_object);
        let set = new_set(&global_object);
        let string = JSValue::from_js_string(js_string(&vm, &WtfString::from_latin1(b"ab")));
        assert!(global_object.map_iterator_protocol_is_intact());
        assert!(global_object.set_iterator_protocol_is_intact());
        assert!(global_object.string_iterator_protocol_is_intact());

        let next = PropertyName::from_identifier(&vm.property_names.next);
        global_object.map_iterator_prototype().put_direct(&vm, &next, JSValue::Int32(1), 0);
        assert!(!global_object.map_iterator_protocol_is_intact());
        assert_eq!(get_iteration_mode(&global_object, map, global_object.map_proto_entries_function().as_value()), IterationMode::Generic);
        assert_eq!(
            get_iteration_mode(&global_object, map_iterator(&global_object, IterationKind::Keys), symbol_iterator),
            IterationMode::Generic
        );
        // O vizinho continua intacto: o protocolo é por tipo de iterador.
        assert!(global_object.set_iterator_protocol_is_intact());
        assert_eq!(get_iteration_mode(&global_object, set, global_object.set_proto_values_function().as_value()), IterationMode::FastSet);

        global_object.set_iterator_prototype().put_direct(&vm, &next, JSValue::Int32(1), 0);
        assert_eq!(get_iteration_mode(&global_object, set, global_object.set_proto_values_function().as_value()), IterationMode::Generic);

        let string_prototype = global_object.iteration_protocol.borrow().string_iterator_prototype.clone().unwrap();
        string_prototype.put_direct(&vm, &next, JSValue::Int32(1), 0);
        assert_eq!(get_iteration_mode(&global_object, string, global_object.string_proto_symbol_iterator_function()), IterationMode::Generic);
    }

    #[test]
    fn iterator_with_an_own_property_loses_the_fast_mode() {
        let (vm, global_object) = new_global_object();
        let symbol_iterator = global_object.iterator_proto_symbol_iterator_function();
        let iterator = map_iterator(&global_object, IterationKind::Entries);
        assert_eq!(get_iteration_mode(&global_object, iterator, symbol_iterator), IterationMode::FastMapEntries);
        // Uma propriedade própria muda a `Structure`: o `next` próprio pode estar sombreando o primordial.
        let next = PropertyName::from_identifier(&vm.property_names.next);
        JSObject::from_value(&iterator).unwrap().put_direct(&vm, &next, JSValue::Int32(1), 0);
        assert_eq!(get_iteration_mode(&global_object, iterator, symbol_iterator), IterationMode::Generic);
    }

    #[test]
    fn async_from_sync_wrapper_drives_primordial_iterators_directly() {
        let (_vm, global_object) = new_global_object();
        let wrapper = create_async_from_sync_iterator_for_iterable(&global_object, new_set(&global_object)).ok().unwrap();
        assert_eq!(wrapper.iteration_mode(), IterationMode::FastSetValues);
        assert!(wrapper.next_method().is_undefined());

        let wrapper = create_async_from_sync_iterator_for_iterable(&global_object, new_map(&global_object)).ok().unwrap();
        assert_eq!(wrapper.iteration_mode(), IterationMode::FastMapEntries);

        let wrapper = create_async_from_sync_iterator_for_iterable(&global_object, set_iterator(&global_object, IterationKind::Entries))
            .ok()
            .unwrap();
        assert_eq!(wrapper.iteration_mode(), IterationMode::FastSetEntries);
    }

    #[test]
    fn async_from_sync_wrapper_of_a_string_reads_next() {
        // O `JSStringIterator` não tem modo rápido de iterador aberto: o wrapper fica `Generic` e lê `next`.
        let (vm, global_object) = new_global_object();
        let string = JSValue::from_js_string(js_string(&vm, &WtfString::from_latin1(b"ab")));
        let wrapper = create_async_from_sync_iterator_for_iterable(&global_object, string).ok().unwrap();
        assert_eq!(wrapper.iteration_mode(), IterationMode::Generic);
        assert!(!wrapper.next_method().is_undefined());
    }
}

