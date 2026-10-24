//! Handlers do `for-in`: `op_get_property_enumerator`, `op_enumerator_next`, `op_enumerator_get_by_val`,
//! `op_enumerator_in_by_val`, `op_enumerator_has_own_property` e `op_enumerator_put_by_val`
//! (`CommonSlowPaths.cpp`, `CommonSlowPathsInlines.h` e `JSPropertyNameEnumerator`).
//!
//! O ponto de entrada é [`run_enumerator`], no mesmo formato de `dispatch_ext::run_ext`.
//!
//! DIVERGÊNCIAS:
//!
//! - O enumerador nunca guarda estrutura em cache (ver `js_property_name_enumerator`), então os caminhos
//!   `OwnStructureMode` com `structureID == cachedStructureID` (e os atalhos do `.asm` que leem o
//!   `JSPropertyNameEnumerator::m_cachedStructureID`) nunca casam: o `OwnStructureMode` cai no mesmo caminho
//!   do `GenericMode`, como o C++ faz quando a estrutura não confere. Sem `m_enumeratorMetadata` nem
//!   `ArrayProfile`.
//! - O `for-in` sobre primitivo (`String` e os demais) usa `JSValue::toObject` como o C++
//!   (`get_property_enumerator`, `enumerator_next`, `has_own_property`); `enumerator_get_by_val` também lê pelo
//!   invólucro, em vez do `JSValue::get` do primitivo (só difere num getter estrito de `String.prototype` que
//!   observa o `this`). `undefined` e `null` dão o enumerador vazio.
//! - Base `JSFunction` entra em todos (`ObjectRef`); `op_enumerator_put_by_val` sobre primitivo usa
//!   `put_to_primitive` (`JSValue::putToPrimitive`: sloppy ignora, strict lança "Attempted to assign to
//!   readonly property."); o `op_enumerator_in_by_val` sobre não objeto lança `createInvalidInParameterError` (`object_for_in`).

use crate::bytecode::bytecode_intrinsics_table::LinkTimeConstant;
use crate::bytecode::bytecode_ops::{
    OpEnumeratorGetByVal, OpEnumeratorHasOwnProperty, OpEnumeratorInByVal, OpEnumeratorNext, OpEnumeratorPutByVal,
    OpGetPropertyEnumerator,
};
use crate::bytecode::instruction_stream::Ref as InstructionRef;
use crate::bytecode::opcode::OpcodeID;
use crate::bytecode::virtual_register::VirtualRegister;
use crate::llint::dispatch::Step;
use crate::llint::dispatch_ext::check_exception;
use crate::llint::slow_paths::{put_error_failure, thrown_failure};
use crate::llint::slow_paths_arith::SlowPathFrame;
use crate::llint::slow_paths_object::{
    function_has_property, is_primitive_base, object_for_access, object_for_in, put_to_object_ref, put_to_primitive,
    to_object_for_access, Ctx,
};
use crate::llint::{LLIntFailure, LLIntResult};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_property_name_enumerator::{
    empty_property_name_enumerator, property_name_enumerator, JSPropertyNameEnumerator, JSPropertyNameEnumeratorRef,
    INDEXED_MODE,
};
use crate::runtime::js_value::{js_boolean, JSValue};
use crate::runtime::property_name::PropertyName;
use crate::runtime::put_property_slot::{PutContext, PutPropertySlot};

/// `uncheckedDowncast<JSPropertyNameEnumerator>(GET(reg).jsValue())`.
fn enumerator_operand(f: &SlowPathFrame, reg: VirtualRegister) -> LLIntResult<JSPropertyNameEnumeratorRef> {
    JSPropertyNameEnumerator::from_value(&f.get(reg))
        .ok_or(LLIntFailure::Unported("registrador do enumerador sem JSPropertyNameEnumerator"))
}

/// A chave que os `op_enumerator_*` consultam: o `index` no `IndexedMode`, o nome (`asString(propertyName)`,
/// `toIdentifier`) nos demais.
enum EnumeratorKey {
    Index(u32),
    Name(PropertyName),
}

/// `mode`, `index` e `propertyName` dos registradores do `for-in` como a chave do acesso.
fn enumerator_key(
    f: &SlowPathFrame,
    (mode, index, property_name): (VirtualRegister, VirtualRegister, VirtualRegister),
) -> LLIntResult<EnumeratorKey> {
    if f.get(mode).as_uint32() == u32::from(INDEXED_MODE) {
        return Ok(EnumeratorKey::Index(f.get(index).as_uint32()));
    }
    // `getByVal`/`putByVal` aplicam `toPropertyKey` ao registrador: string (o normal do for-in), mas também
    // número ou símbolo se o programa o sobrescreveu.
    let identifier = f.get(property_name).to_property_key(Ctx::new(f).global_object).ok_or(LLIntFailure::Thrown)?;
    Ok(EnumeratorKey::Name(PropertyName::from_identifier(&identifier)))
}

/// `slow_path_get_property_enumerator`.
fn get_property_enumerator(f: &mut SlowPathFrame, op: &OpGetPropertyEnumerator) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let base_value = f.get(op.base);
    let enumerator = if base_value.is_undefined_or_null() {
        empty_property_name_enumerator(ctx.global_object)
    } else {
        let base = to_object_for_access(f, base_value)?;
        property_name_enumerator(ctx.global_object, &base).map_err(|thrown| thrown_failure(ctx.global_object, thrown))?
    };
    f.set(op.dst, enumerator.as_value());
    Ok(())
}

/// `slow_path_enumerator_next`: `computeNext` e a atualização de `mode`, `index` e `propertyName`; o fim é a
/// `sentinelString`.
fn enumerator_next(f: &mut SlowPathFrame, op: &OpEnumeratorNext) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let mut mode = f.get(op.mode).as_uint32() as u8;
    let mut index = f.get(op.index).as_uint32();
    let enumerator = enumerator_operand(f, op.enumerator)?;
    let base = to_object_for_access(f, f.get(op.base))?;

    let name = enumerator
        .compute_next(ctx.global_object, &base, &mut index, &mut mode)
        .map_err(|thrown| thrown_failure(ctx.global_object, thrown))?;
    check_exception(f)?;

    f.set(op.mode, JSValue::from_u32(u32::from(mode)));
    f.set(op.index, JSValue::from_u32(index));
    let property_name = match name {
        Some(name) => JSValue::from_js_string(name),
        None => ctx.global_object.link_time_constant(LinkTimeConstant::SentinelString),
    };
    f.set(op.property_name, property_name);
    Ok(())
}

/// `slow_path_enumerator_get_by_val` (`opEnumeratorGetByVal`): `baseValue.get(index)` ou `get(propertyName)`.
fn enumerator_get_by_val(f: &mut SlowPathFrame, op: &OpEnumeratorGetByVal) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let key = enumerator_key(f, (op.mode, op.index, op.property_name))?;
    let base = to_object_for_access(f, f.get(op.base))?;
    let value = match key {
        EnumeratorKey::Index(index) => base.get_by_index(ctx.vm, index),
        EnumeratorKey::Name(name) => base.get(ctx.global_object, &name),
    };
    check_exception(f)?;
    f.set(op.dst, value);
    Ok(())
}

/// `slow_path_enumerator_in_by_val`: `hasProperty(index)` ou `opInByVal` com o nome.
fn enumerator_in_by_val(f: &mut SlowPathFrame, op: &OpEnumeratorInByVal) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let key = enumerator_key(f, (op.mode, op.index, op.property_name))?;
    // `!baseVal.isObject()`: `createInvalidInParameterError`.
    if let Some(function) = f.get(op.base).as_js_function() {
        // `JSFunction` não sobrescreve `hasProperty(index)`: o índice é um nome como outro qualquer.
        let name = match key {
            EnumeratorKey::Index(index) => PropertyName::from_identifier(&Identifier::from_u32(ctx.vm, index)),
            EnumeratorKey::Name(name) => name,
        };
        let found = function_has_property(&ctx, &function, &name)?;
        f.set(op.dst, js_boolean(found));
        return Ok(());
    }
    let base = object_for_in(f, f.get(op.base))?;
    let found = match key {
        EnumeratorKey::Index(index) => base.has_property_by_index(ctx.vm, index),
        EnumeratorKey::Name(name) => base.has_property(ctx.vm, &name),
    };
    check_exception(f)?;
    f.set(op.dst, js_boolean(found));
    Ok(())
}

/// `slow_path_enumerator_has_own_property`: `hasOwnProperty(index)` ou `objectPrototypeHasOwnProperty`.
fn enumerator_has_own_property(f: &mut SlowPathFrame, op: &OpEnumeratorHasOwnProperty) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let mut key = enumerator_key(f, (op.mode, op.index, op.property_name))?;
    let base_value = f.get(op.base);
    // `baseValue.getObject()`: o atalho por índice só vale para objeto; primitivo (`String`) cai no
    // `toIdentifier(propertyName)` + `objectPrototypeHasOwnProperty`, mesmo no `IndexedMode`.
    if matches!(key, EnumeratorKey::Index(_)) && ObjectRef::from_value(&base_value).is_none() {
        let identifier = f.get(op.property_name).to_property_key(ctx.global_object).ok_or(LLIntFailure::Thrown)?;
        key = EnumeratorKey::Name(PropertyName::from_identifier(&identifier));
    }
    let base = to_object_for_access(f, base_value)?;
    let found = match key {
        EnumeratorKey::Index(index) => base.has_own_property_by_index(ctx.vm, index),
        EnumeratorKey::Name(name) => base.has_own_property(ctx.global_object, &name),
    };
    check_exception(f)?;
    f.set(op.dst, js_boolean(found));
    Ok(())
}

/// `slow_path_enumerator_put_by_val` (`opEnumeratorPutByVal`): `putByIndex` ou `baseValue.put(propertyName)`.
fn enumerator_put_by_val(f: &mut SlowPathFrame, op: &OpEnumeratorPutByVal) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let key = enumerator_key(f, (op.mode, op.index, op.property_name))?;
    let base_value = f.get(op.base);
    let value = f.get(op.value);
    let is_strict = op.ecma_mode.is_strict();
    if is_primitive_base(base_value) {
        // `baseValue.putByIndex` / `baseValue.put(globalObject, propertyName, ...)` sobre primitivo:
        // `JSValue::putToPrimitive` (índice vira nome, como em `slow_path_put_by_val`).
        let name = match key {
            EnumeratorKey::Index(index) => PropertyName::from_identifier(&Identifier::from_u32(ctx.vm, index)),
            EnumeratorKey::Name(name) => name,
        };
        let slot = PutPropertySlot::new(base_value, is_strict, PutContext::UnknownContext, false);
        put_to_primitive(ctx.global_object, base_value, &name, value, &slot)?;
        return check_exception(f);
    }
    let base = object_for_access(f, base_value)?;
    match key {
        EnumeratorKey::Index(index) => {
            base.put_by_index(ctx.vm, index, value, is_strict).map_err(|error| put_error_failure(ctx.global_object, error))?;
        }
        EnumeratorKey::Name(name) => {
            let mut slot = PutPropertySlot::new(base_value, is_strict, PutContext::UnknownContext, false);
            put_to_object_ref(&ctx, &base, &name, value, &mut slot)?;
        }
    }
    check_exception(f)
}

/// Os handlers deste arquivo. `None` é o opcode que não é daqui.
pub(super) fn run_enumerator(f: &mut SlowPathFrame, instruction: &InstructionRef) -> LLIntResult<Option<Step>> {
    match instruction.opcode_id_enum() {
        OpcodeID::op_get_property_enumerator => get_property_enumerator(f, &instruction.as_op::<OpGetPropertyEnumerator>())?,
        OpcodeID::op_enumerator_next => enumerator_next(f, &instruction.as_op::<OpEnumeratorNext>())?,
        OpcodeID::op_enumerator_get_by_val => enumerator_get_by_val(f, &instruction.as_op::<OpEnumeratorGetByVal>())?,
        OpcodeID::op_enumerator_in_by_val => enumerator_in_by_val(f, &instruction.as_op::<OpEnumeratorInByVal>())?,
        OpcodeID::op_enumerator_has_own_property => {
            enumerator_has_own_property(f, &instruction.as_op::<OpEnumeratorHasOwnProperty>())?
        }
        OpcodeID::op_enumerator_put_by_val => enumerator_put_by_val(f, &instruction.as_op::<OpEnumeratorPutByVal>())?,
        _ => return Ok(None),
    }
    Ok(Some(Step::Next))
}
