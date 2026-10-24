//! Handlers de objeto que não tinham braço no laço: `op_get_internal_field`, `op_put_internal_field`,
//! `op_get_prototype_of`, `op_get_by_id_direct`, `op_in_by_val`, `op_define_data_property`,
//! `op_define_accessor_property`, `op_throw_static_error`, `op_has_private_name`,
//! `op_get_private_name`, `op_put_private_name` e `op_has_structure_with_flags`.
//!
//! O ponto de entrada é [`run_object`], no mesmo formato de `dispatch_ext::run_ext`. Reaproveita o `Ctx`,
//! `object_for_access`, `to_property_key` e `try_get_as_uint32_index` de `slow_paths_object`, com as mesmas
//! lacunas (`Unported`) de base primitiva e chave que é objeto ou `Symbol` (base `undefined`/`null` lança o
//! `createNotAnObjectError` com o texto-fonte).
//!
//! DIVERGÊNCIAS:
//!
//! - `op_get_internal_field` e `op_put_internal_field` cobrem toda célula com `JSInternalFieldObjectImpl` que o
//!   porte tem, pelo `CellEntry::internal_fields` (geradores, `JSIteratorHelper`, `JSWrapForValidIterator`,
//!   `DisposableStack`, `AsyncDisposableStack`, `ProxyObject`, `JSArrayIterator`, `JSStringIterator`,
//!   `JSRegExpStringIterator`). `JSMapIterator` e `JSSetIterator` (4 campos no C++) guardam o estado fora do
//!   índice de campo e ficam `Unported`; `JSPromise` e `JSAsyncFromSyncIterator` não são
//!   `JSInternalFieldObjectImpl` no C++ e os builtins não os acessam por estes opcodes.
//! - `op_get_prototype_of` é o `JSValue::getPrototype` (`host_function_support`), que cobre objeto, função e
//!   primitivo com invólucro.
//! - `op_get_by_id_direct` é `getOwnPropertySlot` (virtual: despacha `Proxy`) mais `slot.getValue` (propriedade
//!   própria, sem subir a cadeia), sobre `JSObject::from_value`; sem cache de `Metadata`, como os demais.
//! - Os campos privados (`#x`) são propriedades próprias de chave `PrivateName` (`getPrivateFieldSlot`,
//!   `definePrivateField`, `setPrivateField` sem o ramo `WebAssemblyGCObjectType`); funcionam em função (os
//!   `static #x`). `op_set_private_brand`, `op_check_private_brand` e `op_has_private_brand` estão em
//!   `handlers_private_brand`. `has_private_name` com base que não é objeto lança
//!   `createInvalidInParameterError` com o texto-fonte. Sem o `SourceAppender` nas demais mensagens.
//! - Não estão aqui: os de acessor, `op_set_function_name`, `op_new_reg_exp`, `op_put_by_val_direct` e as
//!   variantes `*_with_this` (`handlers_accessor`).

use crate::bytecode::bytecode_ops::{
    OpDefineAccessorProperty, OpDefineDataProperty, OpGetByIdDirect, OpGetInternalField, OpGetPrivateName,
    OpGetPrototypeOf, OpHasPrivateName, OpHasStructureWithFlags, OpInByVal, OpPutInternalField,
    OpPutPrivateName, OpThrowStaticError,
};
use crate::bytecode::instruction_stream::Ref as InstructionRef;
use crate::bytecode::opcode::OpcodeID;
use crate::bytecode::virtual_register::VirtualRegister;
use crate::llint::dispatch::Step;
use crate::llint::dispatch_ext::check_exception;
use crate::llint::slow_paths::{put_error_failure, thrown_failure, throw_error_object};
use crate::llint::slow_paths_arith::SlowPathFrame;
use crate::llint::slow_paths_object::{function_has_property, object_for_access, object_for_in, scope_base, scope_has_property, throw_default_appended_type_error, throw_not_an_object, throw_invalid_in_parameter, throw_invalid_private_name, to_object_for_access, to_property_key, try_get_as_uint32_index, Ctx};
use crate::llint::{LLIntFailure, LLIntResult};
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::define_property_attributes::DefinePropertyAttributes;
use crate::runtime::error::create_error_with_extension;
use crate::runtime::error_messages::REDEFINED_PRIVATE_NAME_ERROR;
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::js_object::{JSObject, PutError};
use crate::runtime::property_offset::{is_valid_offset, PropertyOffset};
use crate::runtime::js_array::ArrayError;
use crate::runtime::js_internal_field_object_impl::InternalFields;
use crate::runtime::js_value::{js_boolean, JSValue};
use crate::runtime::property_descriptor::to_property_descriptor;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::proxy_object::own_property_slot;

/// Aplica `access` aos campos internos da célula que `base` guarda (`CellEntry::internal_fields`, que cobre
/// toda classe com `JSInternalFieldObjectImpl` que o porte tem).
fn with_internal_fields<R>(base: JSValue, access: impl FnOnce(&dyn InternalFields) -> R) -> LLIntResult<R> {
    let entry = match base {
        JSValue::Cell(cell_id) => cell_registry::get(cell_id),
        _ => None,
    };
    let fields = entry.as_ref().and_then(CellEntry::internal_fields).ok_or(NO_INTERNAL_FIELDS)?;
    Ok(access(fields))
}

const NO_INTERNAL_FIELDS: LLIntFailure = LLIntFailure::Unported(
    "campo interno de célula sem JSInternalFieldObjectImpl no porte (JSMapIterator, JSSetIterator, JSPromise)",
);

/// `llintOpWithProfile(op_get_internal_field)`: `internalFields[index]`.
fn get_internal_field(f: &mut SlowPathFrame, op: &OpGetInternalField) -> LLIntResult<()> {
    let value = with_internal_fields(f.get(op.base), |fields| fields.field(op.index))?;
    f.set(op.dst, value);
    Ok(())
}

/// `llintOp(op_put_internal_field)`: `internalFields[index] = value`.
fn put_internal_field(f: &mut SlowPathFrame, op: &OpPutInternalField) -> LLIntResult<()> {
    let value = f.get(op.value);
    with_internal_fields(f.get(op.base), |fields| fields.set_field(op.index, value))
}

/// `slow_path_get_prototype_of`: `value.getPrototype(globalObject)` (objeto, função, primitivo com invólucro,
/// e o `TypeError` de `undefined` e `null`).
fn get_prototype_of(f: &mut SlowPathFrame, op: &OpGetPrototypeOf) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    // `JSValue::synthesizePrototype`: `createNotAnObjectError` leva o texto-fonte da instrução (`evaluating 'super.m'`).
    let value = f.get(op.value);
    if value.is_undefined_or_null() {
        return Err(throw_not_an_object(f, value));
    }
    let prototype = f.get(op.value).get_prototype(ctx.global_object).map_err(|thrown| thrown_failure(ctx.global_object, thrown))?;
    f.set(op.dst, prototype);
    Ok(())
}

/// `slow_path_get_by_id_direct`: `getOwnPropertySlot`, e `undefined` quando a propriedade própria não existe.
fn get_by_id_direct(f: &mut SlowPathFrame, op: &OpGetByIdDirect) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let ident = ctx.identifier(op.property);
    let base = f.get(op.base);
    object_for_access(f, base)?;
    let mut slot = PropertySlot::new(base, InternalMethodType::GetOwnProperty);
    let name = PropertyName::from_identifier(&ident);
    let found = own_property_slot(ctx.global_object, base, &name, &mut slot).map_err(|thrown| thrown_failure(ctx.global_object, thrown))?;
    check_exception(f)?;
    let result = if found { slot.get_value_for(&name) } else { JSValue::undefined() };
    check_exception(f)?;
    f.set(op.dst, result);
    Ok(())
}

/// `slow_path_in_by_val` (`CommonSlowPaths::opInByVal`): índice por `hasProperty(index)`, o resto pela chave.
fn in_by_val(f: &mut SlowPathFrame, op: &OpInByVal) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    // `!baseVal.isObject()`: `createInvalidInParameterError`.
    let subscript = f.get(op.property);
    if let Some(function) = f.get(op.base).as_js_function() {
        // `JSFunction` não sobrescreve `hasProperty(index)`: o índice é um nome como outro qualquer.
        let key = match try_get_as_uint32_index(subscript) {
            Some(index) => crate::runtime::identifier::Identifier::from_u32(ctx.vm, index),
            None => to_property_key(&ctx, subscript)?,
        };
        let found = function_has_property(&ctx, &function, &PropertyName::from_identifier(&key))?;
        f.set(op.dst, js_boolean(found));
        return Ok(());
    }
    if let Some(scope) = scope_base(f.get(op.base)) {
        // O índice é um nome como outro qualquer para a base que é escopo (`SymbolTable` primeiro).
        let key = match try_get_as_uint32_index(subscript) {
            Some(index) => crate::runtime::identifier::Identifier::from_u32(ctx.vm, index),
            None => to_property_key(&ctx, subscript)?,
        };
        let found = scope_has_property(&ctx, &scope, &PropertyName::from_identifier(&key))?;
        f.set(op.dst, js_boolean(found));
        return Ok(());
    }
    let object = object_for_in(f, f.get(op.base))?;
    let found = match try_get_as_uint32_index(subscript) {
        Some(index) => object.has_property_by_index(ctx.vm, index),
        None => {
            let name = to_property_key(&ctx, subscript)?;
            object.has_property(ctx.vm, &PropertyName::from_identifier(&name))
        }
    };
    f.set(op.dst, js_boolean(found));
    Ok(())
}

/// O fim comum de `define_data_property` e `define_accessor_property`: `toPropertyKey`, o
/// `PropertyDescriptor` de `toPropertyDescriptor(value, getter, setter, attributes)` e
/// `methodTable()->defineOwnProperty(base, globalObject, name, descriptor, true)`.
fn define_property(
    f: &mut SlowPathFrame,
    (base, property, attributes): (VirtualRegister, VirtualRegister, VirtualRegister),
    (value, getter, setter): (JSValue, JSValue, JSValue),
) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let object = object_for_access(f, f.get(base))?;
    let name = to_property_key(&ctx, f.get(property))?;
    let attributes = f.get(attributes);
    debug_assert!(attributes.is_int32());
    let descriptor = to_property_descriptor(value, getter, setter, DefinePropertyAttributes::from_raw(attributes.as_int32() as u32));
    object
        .define_own_property(ctx.global_object, &PropertyName::from_identifier(&name), &descriptor, true)
        .map_err(|error| put_error_failure(ctx.global_object, error))?;
    check_exception(f)
}

/// O `RangeError` ou o `PutError` de uma função de `array_prototype`/`array_constructor` como a falha do slow
/// path (a exceção fica pendente).
pub(super) fn array_failure(ctx: &Ctx, error: ArrayError) -> LLIntFailure {
    match error {
        ArrayError::Put(error) => put_error_failure(ctx.global_object, error),
        ArrayError::RangeError(message) => put_error_failure(ctx.global_object, PutError::RangeError(message)),
    }
}

/// `slow_path_throw_static_error`: `createError(globalObject, errorType, message)`.
fn throw_static_error(f: &mut SlowPathFrame, op: &OpThrowStaticError) -> LLIntFailure {
    let message = f.get(op.message);
    debug_assert!(message.is_string());
    let global_object = f.code_block.global_object();
    let error = create_error_with_extension(global_object, op.error_type, &message.as_js_string().value());
    throw_error_object(global_object, error)
}

/// `getOperand(property)` como a chave privada (`subscript.toPropertyKey(globalObject)`); o operando é um
/// `Symbol` privado.
fn private_name(f: &SlowPathFrame, reg: VirtualRegister) -> LLIntResult<PropertyName> {
    let ctx = Ctx::new(f);
    let subscript = f.get(reg);
    debug_assert!(subscript.is_symbol());
    let identifier = subscript.to_property_key(ctx.global_object).ok_or(LLIntFailure::Thrown)?;
    Ok(PropertyName::from_identifier(&identifier))
}

/// `JSObject::getPrivateFieldSlot`: o deslocamento do campo privado próprio, se existe.
fn private_field_offset(ctx: &Ctx, object: &JSObject, name: &PropertyName) -> Option<PropertyOffset> {
    let (offset, _attributes) = object.structure().get_with_attributes(ctx.vm, name);
    is_valid_offset(offset).then_some(offset)
}

/// `slow_path_has_private_name`: `hasPrivateField`.
fn has_private_name(f: &mut SlowPathFrame, op: &OpHasPrivateName) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let base = f.get(op.base);
    // `!baseValue.isObject()`: `createInvalidInParameterError` com o texto-fonte.
    if !base.is_object() {
        return Err(throw_invalid_in_parameter(f, base));
    }
    let name = private_name(f, op.property)?;
    let found = private_field_offset(&ctx, &base.as_object(), &name).is_some();
    f.set(op.dst, js_boolean(found));
    Ok(())
}

/// `slow_path_get_private_name`: `getPrivateField`, que lança `createInvalidPrivateNameError` sem o campo.
fn get_private_name(f: &mut SlowPathFrame, op: &OpGetPrivateName) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let object = to_object_for_access(f, f.get(op.base))?;
    let name = private_name(f, op.property)?;
    let offset = private_field_offset(&ctx, &object, &name)
        .ok_or_else(|| throw_invalid_private_name(f))?;
    f.set(op.dst, object.get_direct(offset));
    Ok(())
}

/// `slow_path_put_private_name`: `definePrivateField` (lança `createRedefinedPrivateNameError` se já existe)
/// ou `setPrivateField` (lança `createInvalidPrivateNameError` se não existe), e `putDirect`.
fn put_private_name(f: &mut SlowPathFrame, op: &OpPutPrivateName) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let object = to_object_for_access(f, f.get(op.base))?;
    let name = private_name(f, op.property)?;
    let exists = private_field_offset(&ctx, &object, &name).is_some();
    if op.put_kind.is_define() {
        if exists {
            return Err(throw_default_appended_type_error(f, REDEFINED_PRIVATE_NAME_ERROR));
        }
    } else {
        debug_assert!(op.put_kind.is_set());
        if !exists {
            return Err(throw_invalid_private_name(f));
        }
    }
    object.put_direct(ctx.vm, &name, f.get(op.value), 0);
    check_exception(f)
}

/// `slow_path_has_structure_with_flags`: `object->structure()->hasAnyOfBitFieldFlags(flags)`.
fn has_structure_with_flags(f: &mut SlowPathFrame, op: &OpHasStructureWithFlags) -> LLIntResult<()> {
    let operand = f.get(op.operand);
    debug_assert!(operand.is_object());
    let object = ObjectRef::from_value(&operand).expect("ASSERT: asObject(GET_C(bytecode.m_operand).jsValue())");
    f.set(op.dst, js_boolean(object.structure().has_any_of_bit_field_flags(op.flags)));
    Ok(())
}

/// Os handlers deste arquivo. `None` é o opcode que não é daqui.
pub(super) fn run_object(f: &mut SlowPathFrame, instruction: &InstructionRef) -> LLIntResult<Option<Step>> {
    match instruction.opcode_id_enum() {
        OpcodeID::op_has_private_name => has_private_name(f, &instruction.as_op::<OpHasPrivateName>())?,
        OpcodeID::op_get_private_name => get_private_name(f, &instruction.as_op::<OpGetPrivateName>())?,
        OpcodeID::op_put_private_name => put_private_name(f, &instruction.as_op::<OpPutPrivateName>())?,
        OpcodeID::op_has_structure_with_flags => has_structure_with_flags(f, &instruction.as_op::<OpHasStructureWithFlags>())?,
        OpcodeID::op_get_internal_field => get_internal_field(f, &instruction.as_op::<OpGetInternalField>())?,
        OpcodeID::op_put_internal_field => put_internal_field(f, &instruction.as_op::<OpPutInternalField>())?,
        OpcodeID::op_get_prototype_of => get_prototype_of(f, &instruction.as_op::<OpGetPrototypeOf>())?,
        OpcodeID::op_get_by_id_direct => get_by_id_direct(f, &instruction.as_op::<OpGetByIdDirect>())?,
        OpcodeID::op_in_by_val => in_by_val(f, &instruction.as_op::<OpInByVal>())?,
        OpcodeID::op_define_data_property => {
            let op: OpDefineDataProperty = instruction.as_op();
            let value = f.get(op.value);
            define_property(f, (op.base, op.property, op.attributes), (value, JSValue::undefined(), JSValue::undefined()))?;
        }
        OpcodeID::op_define_accessor_property => {
            let op: OpDefineAccessorProperty = instruction.as_op();
            let (getter, setter) = (f.get(op.getter), f.get(op.setter));
            define_property(f, (op.base, op.property, op.attributes), (JSValue::undefined(), getter, setter))?;
        }
        OpcodeID::op_throw_static_error => return Err(throw_static_error(f, &instruction.as_op::<OpThrowStaticError>())),
        _ => return Ok(None),
    }
    Ok(Some(Step::Next))
}
