//! O despacho dos ganchos exóticos de `JSGenericTypedArrayView` (`getOwnPropertySlot`,
//! `getOwnPropertySlotByIndex`, `put`, `putByIndex`, `defineOwnProperty`, `deleteProperty`,
//! `deletePropertyByIndex`) para o `JSObject` do porte, que não tem a tabela de métodos virtual
//! (`methodTable()->getOwnPropertySlot` e irmãs): o `JSObject` pergunta aqui, com `self`, se é uma das 12
//! visões, e se for responde no lugar do caso comum. É o mesmo papel que o `Proxy` tem em
//! `js_object.rs` (`get_property_slot_from_proxy`, `put_from_proxy`).
//!
//! Cada função devolve `None` quando o `JSObject` deve responder (o objeto não é uma visão, ou a visão não
//! trata aquele nome: `Base::getOwnPropertySlot` e irmãs).
//!
//! Os erros que o C++ deixa como exceção pendente (`throwTypeError`, `RETURN_IF_EXCEPTION`) são lançados no
//! realm da `Structure` da visão: o `Thrown` vira exceção pendente e a função responde `false` (leitura) ou
//! `PutError::Pending` (escrita).

use crate::runtime::host_call::{throw_thrown, Thrown};
use crate::runtime::js_generic_typed_array_view::{JSGenericTypedArrayView, JSGenericTypedArrayViewRef};
use crate::runtime::js_global_object::JSGlobalObjectRef;
use crate::runtime::js_object::{JSObject, PutError};
use crate::runtime::js_value::JSValue;
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::PropertySlot;
use crate::runtime::proxy_object::put_error_from_thrown;
use crate::runtime::typed_array_type::is_typed_view;
use crate::runtime::vm::VM;

/// A visão que é este objeto, se o `JSType` dele é o de uma das 12 visões.
fn view_of(object: &JSObject) -> Option<JSGenericTypedArrayViewRef> {
    if !is_typed_view(object.type_()) {
        return None;
    }
    JSGenericTypedArrayView::from_cell_id(object.cell_id())
}

/// O `globalObject` dos ganchos: o realm da `Structure` da visão.
fn realm_of(object: &JSObject) -> JSGlobalObjectRef {
    object.structure().realm().expect("TypedArray sem realm na Structure")
}

/// O `RETURN_IF_EXCEPTION` de uma leitura: o erro vira exceção pendente.
fn throw_in_realm(object: &JSObject, thrown: Thrown) {
    throw_thrown(&realm_of(object), thrown);
}

/// O resultado de uma escrita da visão como o do `JSObject`.
fn put_result(object: &JSObject, result: Result<Option<bool>, Thrown>) -> Option<Result<bool, PutError>> {
    match result {
        Ok(answer) => answer.map(Ok),
        Err(thrown) => Some(Err(put_error_from_thrown(&realm_of(object), thrown))),
    }
}

/// `getOwnPropertySlotByIndex(thisObject, globalObject, i, slot)`.
pub fn get_own_property_slot_by_index(object: &JSObject, index: u32, slot: &mut PropertySlot) -> Option<bool> {
    let view = view_of(object)?;
    Some(view.get_own_property_slot_by_index(index, slot).unwrap_or_else(|thrown| {
        throw_in_realm(object, thrown);
        false
    }))
}

/// `getOwnPropertySlot(thisObject, globalObject, propertyName, slot)`.
pub fn get_own_property_slot(object: &JSObject, vm: &VM, property_name: &PropertyName, slot: &mut PropertySlot) -> Option<bool> {
    let view = view_of(object)?;
    view.get_own_property_slot(vm, property_name, slot).unwrap_or_else(|thrown| {
        throw_in_realm(object, thrown);
        Some(false)
    })
}

/// `put(cell, globalObject, propertyName, value, slot)`; `receiver` é o `slot.thisValue()` e `should_throw`
/// o `slot.isStrictMode()`.
pub fn put(
    object: &JSObject,
    property_name: &PropertyName,
    value: JSValue,
    receiver: JSValue,
    should_throw: bool,
) -> Option<Result<bool, PutError>> {
    let view = view_of(object)?;
    let result = view.put(&realm_of(object), property_name, value, receiver, should_throw);
    put_result(object, result)
}

/// `putByIndex(cell, globalObject, i, value, shouldThrow)`.
pub fn put_by_index(object: &JSObject, index: u32, value: JSValue) -> Option<Result<bool, PutError>> {
    let view = view_of(object)?;
    let result = view.put_by_index(&realm_of(object), index, value).map(Some);
    put_result(object, result)
}

/// `defineOwnProperty(object, globalObject, propertyName, descriptor, shouldThrow)`.
pub fn define_own_property(
    object: &JSObject,
    property_name: &PropertyName,
    descriptor: &PropertyDescriptor,
    should_throw: bool,
) -> Option<Result<bool, PutError>> {
    let view = view_of(object)?;
    let result = view.define_own_property(&realm_of(object), property_name, descriptor, should_throw);
    put_result(object, result)
}

/// `deleteProperty(cell, globalObject, propertyName, slot)`.
pub fn delete_property(object: &JSObject, vm: &VM, property_name: &PropertyName) -> Option<bool> {
    view_of(object)?.delete_property(vm, property_name)
}

/// `deletePropertyByIndex(cell, globalObject, i)`.
pub fn delete_property_by_index(object: &JSObject, index: u32) -> Option<bool> {
    Some(view_of(object)?.delete_property_by_index(index))
}

/// `JSGenericResizableOrGrowableSharedTypedArrayView::preventExtensions`
/// (https://tc39.es/ecma262/#sec-typedarray-preventextensions): `true` quando a visão recusa o
/// `[[PreventExtensions]]` (comprimento automático, ou sobre buffer redimensionável não compartilhado).
pub fn refuses_prevent_extensions(object: &JSObject) -> bool {
    view_of(object).is_some_and(|view| view.is_auto_length() || view.is_resizable_non_shared())
}
