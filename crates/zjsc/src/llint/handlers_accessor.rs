//! Handlers de acessores e de acesso com receptor: `op_put_getter_by_id`, `op_put_setter_by_id`,
//! `op_put_getter_setter_by_id`, `op_put_getter_by_val`, `op_put_setter_by_val`, `op_set_function_name`,
//! `op_new_reg_exp`, `op_put_by_val_direct`, `op_get_by_id_with_this`, `op_get_by_val_with_this`,
//! `op_put_by_id_with_this` e `op_put_by_val_with_this` (`LLIntSlowPaths.cpp` e `CommonSlowPaths.cpp`).
//!
//! O ponto de entrada é [`run_accessor`], no mesmo formato de `dispatch_ext::run_ext`. Reaproveita o `Ctx`,
//! `object_for_access` (que devolve um `ObjectRef`, `JSFunction` inclusive), `to_property_key`,
//! `try_get_as_uint32_index`, `put_direct_ref_with_reify`, `put_to_object_ref` e `put_by_id_context` de
//! `slow_paths_object`, com as mesmas lacunas (`Unported`) de base primitiva ou escopo e chave que é
//! objeto ou `Symbol` (`undefined`/`null` lançam o `createNotAnObjectError` com o texto-fonte).
//!
//! DIVERGÊNCIAS:
//!
//! - Getter e setter que são `JSFunction` (todo getter escrito em JavaScript) entram: o `GetterSetter` guarda
//!   um `ObjectRef`, e `accessor_function` aceita o que `ObjectRef::from_value` alcança.
//! - `op_put_getter_setter_by_id` tem o ramo `reifyLazyPropertyIfNeeded` de `putDirectAccessorWithReify`
//!   para base `JSFunction`.
//! - `op_set_function_name` com `Symbol` usa o `description` do símbolo (`[descrição]`), e a vazia para o
//!   símbolo sem descrição, como `JSFunction::setFunctionName` com o `uid` do `PrivateName`.
//! - `op_new_reg_exp` usa `areLegacyFeaturesEnabled = true`, como o C++.

use crate::bytecode::bytecode_ops::{
    OpGetByIdWithThis, OpGetByValWithThis, OpNewRegExp, OpPutByIdWithThis, OpPutByValDirect, OpPutByValWithThis,
    OpPutGetterById, OpPutGetterByVal, OpPutGetterSetterById, OpPutSetterById, OpPutSetterByVal, OpSetFunctionName,
};
use crate::bytecode::instruction_stream::Ref as InstructionRef;
use crate::bytecode::opcode::OpcodeID;
use crate::bytecode::virtual_register::VirtualRegister;
use crate::llint::dispatch::Step;
use crate::llint::dispatch_ext::check_exception;
use crate::llint::slow_paths::{put_error_failure, throw_out_of_memory_error};
use crate::llint::slow_paths_arith::SlowPathFrame;
use crate::llint::slow_paths_control::get_js_function;
use crate::llint::slow_paths_object::{
    get_primitive_property, is_primitive_base, object_for_access, put_by_id_context, put_direct_ref_with_reify, put_to_object_ref,
    put_to_primitive, synthesize_prototype_for_access, throw_not_an_object, to_property_key, try_get_as_uint32_index, Ctx,
};
use crate::llint::{LLIntFailure, LLIntResult};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_getter_setter::GetterSetter;
use crate::runtime::js_value::JSValue;
use crate::runtime::js_string::js_string;
use crate::runtime::operations::js_string_concat_three_strings;
use crate::runtime::property_attribute::{DONT_ENUM, READ_ONLY};
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::put_property_slot::{PutContext, PutPropertySlot};
use crate::runtime::reg_exp::RegExp;
use crate::runtime::reg_exp_object::RegExpObject;
use crate::runtime::sparse_array_value_map::PutDirectIndexMode;
use crate::runtime::symbol::Symbol;
use crate::wtf::text::wtf_string::String as WtfString;

/// Getter ou setter do `op_put_*_by_*` (`asObject(getter)`): o valor tem de ser um objeto, `JSFunction`
/// inclusive (o `GetterSetter` guarda um `ObjectRef`).
fn accessor_function(value: JSValue) -> LLIntResult<JSValue> {
    // O gerador só emite estes opcodes com o resultado de uma função (método `get`/`set` de literal ou de
    // classe), sempre uma `JSFunction`: `asObject` não tem outro caso.
    assert!(ObjectRef::from_value(&value).is_some(), "getter ou setter do bytecode que não é objeto");
    Ok(value)
}

/// Qual metade do acessor `putGetter`/`putSetter` define.
#[derive(Clone, Copy)]
enum AccessorKind {
    Getter,
    Setter,
}

/// `JSObject::putGetter` e `JSObject::putSetter`: `defineOwnProperty` com o descritor de uma função só, e
/// `configurable`/`enumerable` conforme os `attributes`.
fn put_accessor(
    f: &mut SlowPathFrame,
    (base, accessor): (VirtualRegister, VirtualRegister),
    name: &PropertyName,
    attributes: u32,
    kind: AccessorKind,
) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let object = object_for_access(f, f.get(base))?;
    let function = accessor_function(f.get(accessor))?;

    let mut descriptor = PropertyDescriptor::default();
    match kind {
        AccessorKind::Getter => descriptor.set_getter(function),
        AccessorKind::Setter => descriptor.set_setter(function),
    }
    if attributes & READ_ONLY == 0 {
        descriptor.set_configurable(true);
    }
    if attributes & DONT_ENUM == 0 {
        descriptor.set_enumerable(true);
    }
    object.define_own_property(ctx.global_object, name, &descriptor, true).map_err(|error| put_error_failure(ctx.global_object, error))?;
    check_exception(f)
}

/// `putGetter`/`putSetter` pelo nome do identificador (`codeBlock->identifier(property)`).
fn put_accessor_by_id(
    f: &mut SlowPathFrame,
    (base, accessor): (VirtualRegister, VirtualRegister),
    (property, attributes): (u32, u32),
    kind: AccessorKind,
) -> LLIntResult<()> {
    let name = PropertyName::from_identifier(&Ctx::new(f).identifier(property));
    put_accessor(f, (base, accessor), &name, attributes, kind)
}

/// `putGetter`/`putSetter` pela chave computada (`subscript.toPropertyKey(globalObject)`).
fn put_accessor_by_val(
    f: &mut SlowPathFrame,
    (base, accessor): (VirtualRegister, VirtualRegister),
    (property, attributes): (VirtualRegister, u32),
    kind: AccessorKind,
) -> LLIntResult<()> {
    let key = to_property_key(&Ctx::new(f), f.get(property))?;
    put_accessor(f, (base, accessor), &PropertyName::from_identifier(&key), attributes, kind)
}

/// `slow_path_put_getter_setter_by_id`: `GetterSetter::create` e `putDirectAccessorWithReify`.
fn put_getter_setter_by_id(f: &mut SlowPathFrame, op: &OpPutGetterSetterById) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let object = object_for_access(f, f.get(op.base))?;
    let (getter, setter) = (f.get(op.getter), f.get(op.setter));
    debug_assert!(!getter.is_undefined() || !setter.is_undefined());
    for function in [getter, setter] {
        if !function.is_undefined() {
            accessor_function(function)?;
        }
    }
    let accessor = GetterSetter::create_from_values(ctx.vm, getter, setter);
    let name = PropertyName::from_identifier(&ctx.identifier(op.property));
    // `putDirectAccessorWithReify`: a `JSFunction` reifica a propriedade preguiçosa antes.
    if let ObjectRef::Function(function) = &object {
        function.reify_lazy_property_if_needed(ctx.global_object, &name, false);
        if ctx.vm.exception().is_some() {
            return Err(LLIntFailure::Thrown);
        }
    }
    object.put_direct_accessor(ctx.vm, &name, accessor, op.attributes).map_err(|error| put_error_failure(ctx.global_object, error))?;
    Ok(())
}

/// `slow_path_set_function_name`: `JSFunction::setFunctionName(globalObject, name)`.
fn set_function_name(f: &mut SlowPathFrame, op: &OpSetFunctionName) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    // `jsCast<JSFunction*>(...)`: o gerador só emite `set_function_name` sobre função ou classe anônima.
    let function = get_js_function(f.get(op.function)).expect("set_function_name sobre valor que não é JSFunction");

    // The "name" property may have been already been defined as part of a property list in an object
    // literal (and therefore reified).
    if function.has_reified_name() {
        return Ok(());
    }

    let value = f.get(op.name);
    let symbol = value.is_cell().then(|| Symbol::from_cell_id(value.as_cell())).flatten();
    let name = if let Some(symbol) = symbol {
        match symbol.description(ctx.vm) {
            None => WtfString::from_latin1(b""),
            Some(description) => js_string_concat_three_strings(
                ctx.vm,
                &js_string(ctx.vm, &WtfString::from_latin1(b"[")),
                &description,
                &js_string(ctx.vm, &WtfString::from_latin1(b"]")),
            )
            .map(|name| name.value())
            .ok_or_else(|| throw_out_of_memory_error(ctx.global_object))?,
        }
    } else if value.is_string() {
        value.as_js_string().value()
    } else {
        // `ASSERT(value.isString())` de `JSFunction::setFunctionName`: o gerador emite `to_property_key`
        // antes do `set_function_name`, então o nome é sempre String ou Symbol.
        unreachable!("set_function_name com nome que não é String nem Symbol");
    };
    let _ = function.reify_name_with(ctx.vm, ctx.global_object, name);
    Ok(())
}

/// `slow_path_new_reg_exp`: `RegExpObject::create(vm, globalObject->regExpStructure(), regExp, true)`.
fn new_reg_exp(f: &mut SlowPathFrame, op: &OpNewRegExp) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let value = f.get(op.regexp);
    // `jsCast<RegExp*>(codeBlock->getConstant(...))`: a constante do `new_regexp` é sempre um `RegExp`.
    let reg_exp = value
        .is_cell()
        .then(|| RegExp::from_cell_id(value.as_cell()))
        .flatten()
        .expect("new_reg_exp com constante que não é RegExp");
    let result = RegExpObject::create(ctx.vm, ctx.global_object.reg_exp_structure(), reg_exp, true);
    f.set(op.dst, result.as_value());
    Ok(())
}

/// `slow_path_put_by_val_direct`: `putDirectIndex` para índice, `putDirectWithReify` para nome.
fn put_by_val_direct(f: &mut SlowPathFrame, op: &OpPutByValDirect) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let base = f.get(op.base);
    let subscript = f.get(op.property);
    let value = f.get(op.value);
    let is_strict = op.ecma_mode.is_strict();
    let object = object_for_access(f, base)?;
    let mode = if is_strict { PutDirectIndexMode::PutDirectIndexShouldThrow } else { PutDirectIndexMode::PutDirectIndexShouldNotThrow };

    if let Some(index) = try_get_as_uint32_index(subscript) {
        object.put_direct_index(ctx.vm, index, value, 0, mode).map_err(|error| put_error_failure(ctx.global_object, error))?;
        return Ok(());
    }

    // Don't put to an object if toString threw an exception.
    let name = PropertyName::from_identifier(&to_property_key(&ctx, subscript)?);
    if let Some(index) = name.parse_index() {
        object.put_direct_index(ctx.vm, index, value, 0, mode).map_err(|error| put_error_failure(ctx.global_object, error))?;
    } else {
        let mut slot = PutPropertySlot::new(base, is_strict, PutContext::UnknownContext, false);
        put_direct_ref_with_reify(&ctx, &object, &name, value, &mut slot)?;
    }
    Ok(())
}

/// `baseValue.get(globalObject, name, slot)` com o `this` do `slot` no receptor (`super.x`).
fn get_with_this(
    f: &SlowPathFrame,
    base: JSValue,
    this_value: JSValue,
    key: WithThisKey,
) -> LLIntResult<JSValue> {
    let ctx = Ctx::new(f);
    // `JSValue::getPropertySlot`: `length` e os índices da string respondem antes do protótipo.
    if base.is_string() {
        let string = base.as_js_string();
        let own_name = match &key {
            WithThisKey::Index(index) => PropertyName::from_identifier(&Identifier::from_u32(ctx.vm, *index)),
            WithThisKey::Name(name) => name.clone(),
        };
        if own_name == ctx.vm.property_names.length {
            return Ok(JSValue::Int32(string.length() as i32));
        }
        if own_name.parse_index().is_some_and(|index| index < string.length()) {
            return get_primitive_property(&ctx, base, &own_name);
        }
    }
    let object = synthesize_prototype_for_access(f, base)?;
    let mut slot = PropertySlot::new(this_value, InternalMethodType::Get);
    let found = match &key {
        WithThisKey::Index(index) => object.get_property_slot_by_index(ctx.vm, *index, &mut slot),
        WithThisKey::Name(name) => object.get_property_slot(ctx.global_object, name, &mut slot),
    };
    if !found {
        return Ok(JSValue::undefined());
    }
    // `slot.getValue(globalObject, name)`: o slot pode ser de getter (`super.x` sobre accessor) ou custom;
    // a exceção de um getter que lança fica pendente no VM e quem chama confere com `check_exception`.
    if slot.is_accessor() {
        // Lacuna do interpretador (`PutError::Unported`) e exceção pendente propagam, não viram `undefined`.
        let value = slot.getter_setter().call_getter(slot.this_value()).map_err(|error| put_error_failure(ctx.global_object, error))?;
        check_exception(f)?;
        return Ok(if value.is_empty() { JSValue::undefined() } else { value });
    }
    Ok(match key {
        WithThisKey::Index(index) => slot.get_value_for_index(ctx.vm, index),
        WithThisKey::Name(name) => slot.get_value_for(&name),
    })
}

/// A chave de `get_*_with_this`: o índice (`isUInt32`) ou o nome.
enum WithThisKey {
    Index(u32),
    Name(PropertyName),
}

/// `slow_path_get_by_id_with_this`.
fn get_by_id_with_this(f: &mut SlowPathFrame, op: &OpGetByIdWithThis) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let name = PropertyName::from_identifier(&ctx.identifier(op.property));
    let result = get_with_this(f, f.get(op.base), f.get(op.this_value), WithThisKey::Name(name))?;
    check_exception(f)?;
    f.set(op.dst, result);
    Ok(())
}

/// `slow_path_get_by_val_with_this`.
fn get_by_val_with_this(f: &mut SlowPathFrame, op: &OpGetByValWithThis) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let (base, this_value, subscript) = (f.get(op.base), f.get(op.this_value), f.get(op.property));
    let key = match try_get_as_uint32_index(subscript) {
        Some(index) => WithThisKey::Index(index),
        None => {
            // `baseValue.requireObjectCoercible(globalObject)`: `undefined` e `null` lançam `createNotAnObjectError`.
            if base.is_undefined_or_null() {
                return Err(throw_not_an_object(f, base));
            }
            WithThisKey::Name(PropertyName::from_identifier(&to_property_key(&ctx, subscript)?))
        }
    };
    let result = get_with_this(f, base, this_value, key)?;
    check_exception(f)?;
    f.set(op.dst, result);
    Ok(())
}

/// `slow_path_put_by_id_with_this`: `baseValue.putInline` com `PutPropertySlot(thisVal, ...)`.
fn put_by_id_with_this(f: &mut SlowPathFrame, op: &OpPutByIdWithThis) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let name = PropertyName::from_identifier(&ctx.identifier(op.property));
    let mut slot = PutPropertySlot::new(
        f.get(op.this_value),
        op.ecma_mode.is_strict(),
        put_by_id_context(ctx.code_block),
        false,
    );
    if is_primitive_base(f.get(op.base)) {
        put_to_primitive(ctx.global_object, f.get(op.base), &name, f.get(op.value), &slot)?;
        return check_exception(f);
    }
    let object = object_for_access(f, f.get(op.base))?;
    put_to_object_ref(&ctx, &object, &name, f.get(op.value), &mut slot)?;
    check_exception(f)
}

/// `slow_path_put_by_val_with_this`: `toPropertyKey` e `baseValue.put` com `PutPropertySlot(thisValue, ...)`.
fn put_by_val_with_this(f: &mut SlowPathFrame, op: &OpPutByValWithThis) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let name = PropertyName::from_identifier(&to_property_key(&ctx, f.get(op.property))?);
    let mut slot = PutPropertySlot::new(f.get(op.this_value), op.ecma_mode.is_strict(), PutContext::UnknownContext, false);
    if is_primitive_base(f.get(op.base)) {
        put_to_primitive(ctx.global_object, f.get(op.base), &name, f.get(op.value), &slot)?;
        return check_exception(f);
    }
    let object = object_for_access(f, f.get(op.base))?;
    put_to_object_ref(&ctx, &object, &name, f.get(op.value), &mut slot)?;
    check_exception(f)
}

/// Os handlers deste arquivo. `None` é o opcode que não é daqui.
pub(super) fn run_accessor(f: &mut SlowPathFrame, instruction: &InstructionRef) -> LLIntResult<Option<Step>> {
    match instruction.opcode_id_enum() {
        OpcodeID::op_put_getter_by_id => {
            let op: OpPutGetterById = instruction.as_op();
            put_accessor_by_id(f, (op.base, op.accessor), (op.property, op.attributes), AccessorKind::Getter)?;
        }
        OpcodeID::op_put_setter_by_id => {
            let op: OpPutSetterById = instruction.as_op();
            put_accessor_by_id(f, (op.base, op.accessor), (op.property, op.attributes), AccessorKind::Setter)?;
        }
        OpcodeID::op_put_getter_setter_by_id => put_getter_setter_by_id(f, &instruction.as_op::<OpPutGetterSetterById>())?,
        OpcodeID::op_put_getter_by_val => {
            let op: OpPutGetterByVal = instruction.as_op();
            put_accessor_by_val(f, (op.base, op.accessor), (op.property, op.attributes), AccessorKind::Getter)?;
        }
        OpcodeID::op_put_setter_by_val => {
            let op: OpPutSetterByVal = instruction.as_op();
            put_accessor_by_val(f, (op.base, op.accessor), (op.property, op.attributes), AccessorKind::Setter)?;
        }
        OpcodeID::op_set_function_name => set_function_name(f, &instruction.as_op::<OpSetFunctionName>())?,
        OpcodeID::op_new_reg_exp => new_reg_exp(f, &instruction.as_op::<OpNewRegExp>())?,
        OpcodeID::op_put_by_val_direct => put_by_val_direct(f, &instruction.as_op::<OpPutByValDirect>())?,
        OpcodeID::op_get_by_id_with_this => get_by_id_with_this(f, &instruction.as_op::<OpGetByIdWithThis>())?,
        OpcodeID::op_get_by_val_with_this => get_by_val_with_this(f, &instruction.as_op::<OpGetByValWithThis>())?,
        OpcodeID::op_put_by_id_with_this => put_by_id_with_this(f, &instruction.as_op::<OpPutByIdWithThis>())?,
        OpcodeID::op_put_by_val_with_this => put_by_val_with_this(f, &instruction.as_op::<OpPutByValWithThis>())?,
        _ => return Ok(None),
    }
    Ok(Some(Step::Next))
}
