//! Handlers dos métodos privados de classe: `op_set_private_brand`, `op_check_private_brand` e
//! `op_has_private_brand` (`#m() {}`, `#x in o`).
//!
//! O ponto de entrada é [`run_private_brand`], no mesmo formato de `dispatch_ext::run_ext`. Cada um é o
//! slow path de `LLIntSlowPaths.cpp` sobre `JSObject::setPrivateBrand`, `checkPrivateBrand` e
//! `hasPrivateBrand` (`JSObjectInlines.h`), que lançam as mensagens exatas de `ExceptionHelpers.cpp`.
//!
//! DIVERGÊNCIAS:
//!
//! - Sem o cache de `Metadata` (`m_oldStructureID`, `m_newStructureID`, `m_structureID`, `m_brand`), como os
//!   demais handlers: o slow path roda sempre.
//! - `op_has_private_brand` com base que não é objeto lança `createInvalidInParameterError` com o texto-fonte
//!   (`throw_invalid_in_parameter`). `op_set_private_brand` com base que não é objeto é o
//!   `ASSERT(baseValue.isObject())` do C++ (o gerador só instala a marca em `this` ou no construtor da
//!   classe): vira `expect`.

use crate::bytecode::bytecode_ops::{OpCheckPrivateBrand, OpHasPrivateBrand, OpSetPrivateBrand};
use crate::bytecode::instruction_stream::Ref as InstructionRef;
use crate::bytecode::opcode::OpcodeID;
use crate::llint::dispatch::Step;
use crate::llint::slow_paths_arith::SlowPathFrame;
use crate::llint::slow_paths_object::{throw_default_appended_type_error, throw_invalid_in_parameter, to_object_for_access, Ctx};
use crate::llint::LLIntResult;
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::js_value::js_boolean;
use crate::runtime::symbol::as_symbol;

/// `slow_path_set_private_brand`: `baseObject->setPrivateBrand(globalObject, brand)`.
fn set_private_brand(f: &mut SlowPathFrame, op: &OpSetPrivateBrand) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let base = f.get(op.base);
    let brand = f.get(op.brand);
    debug_assert!(brand.is_symbol());
    debug_assert!(base.is_object());
    let object = ObjectRef::from_value(&base).expect("ASSERT: baseValue.isObject()");
    object
        .set_private_brand(ctx.vm, &as_symbol(brand))
        .map_err(|message| throw_default_appended_type_error(f, message))
}

/// `slow_path_check_private_brand`: `baseValue.toObject(globalObject)` e `checkPrivateBrand`.
fn check_private_brand(f: &mut SlowPathFrame, op: &OpCheckPrivateBrand) -> LLIntResult<()> {
    let brand = f.get(op.brand);
    debug_assert!(brand.is_symbol());
    let object = to_object_for_access(f, f.get(op.base))?;
    object
        .check_private_brand(&as_symbol(brand))
        .map_err(|message| throw_default_appended_type_error(f, message))
}

/// `slow_path_has_private_brand`: `asObject(baseValue)->hasPrivateBrand(globalObject, brand)`.
fn has_private_brand(f: &mut SlowPathFrame, op: &OpHasPrivateBrand) -> LLIntResult<()> {
    let base = f.get(op.base);
    // `!baseValue.isObject()`: `createInvalidInParameterError` com o texto-fonte.
    if !base.is_object() {
        return Err(throw_invalid_in_parameter(f, base));
    }
    let brand = f.get(op.brand);
    debug_assert!(brand.is_symbol());
    let found = base.as_object().has_private_brand(&as_symbol(brand));
    f.set(op.dst, js_boolean(found));
    Ok(())
}

/// Os handlers deste arquivo. `None` é o opcode que não é daqui.
pub(super) fn run_private_brand(f: &mut SlowPathFrame, instruction: &InstructionRef) -> LLIntResult<Option<Step>> {
    match instruction.opcode_id_enum() {
        OpcodeID::op_set_private_brand => set_private_brand(f, &instruction.as_op::<OpSetPrivateBrand>())?,
        OpcodeID::op_check_private_brand => check_private_brand(f, &instruction.as_op::<OpCheckPrivateBrand>())?,
        OpcodeID::op_has_private_brand => has_private_brand(f, &instruction.as_op::<OpHasPrivateBrand>())?,
        _ => return Ok(None),
    }
    Ok(Some(Step::Next))
}
