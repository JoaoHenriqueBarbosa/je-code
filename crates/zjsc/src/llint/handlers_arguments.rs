//! Handlers do objeto `arguments`: `op_create_direct_arguments`, `op_create_scoped_arguments`,
//! `op_create_cloned_arguments`, `op_get_from_arguments` e `op_put_to_arguments`
//! (`LLIntSlowPaths.cpp` e os handlers `.asm` de `get_from_arguments`/`put_to_arguments`).
//!
//! O ponto de entrada é [`run_arguments`], no mesmo formato de `dispatch_ext::run_ext`.
//!
//! DIVERGÊNCIAS:
//!
//! - `DirectArguments` e `ClonedArguments` (em `runtime::js_arguments_objects`) e `ScopedArguments` (em
//!   `runtime::js_scoped_arguments`) nascem com `length`, `callee` e `@@iterator` já materializados; os
//!   índices mapeados são despachados por `runtime::generic_arguments`. O `callee` do `ClonedArguments` em
//!   modo estrito é o acessor `%ThrowTypeError%` do realm.
//! - Não existe `op_create_arguments_butterfly` no `BytecodeList.rb` deste JavaScriptCore.

use crate::bytecode::bytecode_ops::{
    OpCreateClonedArguments, OpCreateDirectArguments, OpCreateScopedArguments, OpGetFromArguments, OpPutToArguments,
};
use crate::bytecode::instruction_stream::Ref as InstructionRef;
use crate::bytecode::opcode::OpcodeID;
use crate::llint::dispatch::Step;
use crate::llint::slow_paths::put_error_failure;
use crate::llint::slow_paths_arith::SlowPathFrame;
use crate::llint::slow_paths_control::get_js_function;
use crate::llint::slow_paths_object::Ctx;
use crate::llint::LLIntResult;
use crate::runtime::js_arguments_objects::{create_cloned_arguments, DirectArguments, DirectArgumentsRef};
use crate::runtime::js_function::JSFunctionRef;
use crate::runtime::js_scope::{JSScope, JSScopeRef};
use crate::runtime::js_scoped_arguments::ScopedArguments;
use crate::runtime::js_value::JSValue;

/// `uncheckedDowncast<JSFunction>(callFrame->jsCallee())`: o frame de um código que cria `arguments` é sempre
/// de uma `JSFunction`.
fn js_callee(f: &SlowPathFrame) -> JSFunctionRef {
    get_js_function(f.call_frame.guaranteed_js_value_callee(f.stack))
        .expect("uncheckedDowncast<JSFunction> em callee que não é JSFunction")
}

/// `slow_path_create_direct_arguments`: `DirectArguments::createByCopying(globalObject, callFrame)`.
fn create_direct_arguments(f: &mut SlowPathFrame, op: &OpCreateDirectArguments) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let arguments = f.call_frame.arguments_span(f.stack);
    let callee = f.call_frame.guaranteed_js_value_callee(f.stack);
    let min_capacity = f.code_block.num_parameters().saturating_sub(1) as usize;
    let result = DirectArguments::create_by_copying(ctx.global_object, &arguments, min_capacity, callee)
        .map_err(|error| put_error_failure(ctx.global_object, error))?;
    f.set(op.dst, result.as_value());
    Ok(())
}

/// `slow_path_create_scoped_arguments`: `ScopedArguments::createByCopying(globalObject, callFrame, table, scope)`
/// com `table = scope->symbolTable()->arguments()`.
fn create_scoped_arguments(f: &mut SlowPathFrame, op: &OpCreateScopedArguments) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let scope = match JSScope::from_cell_id(f.call_frame.unchecked_r(f.stack, op.scope).unboxed_cell()) {
        Some(JSScopeRef::LexicalEnvironment(scope)) => scope,
        _ => unreachable!("uncheckedDowncast<JSLexicalEnvironment> em escopo que não é de função"),
    };
    let arguments = f.call_frame.arguments_span(f.stack);
    let callee = js_callee(f);
    let result = ScopedArguments::create_by_copying(ctx.global_object, &arguments, &callee, scope)
        .map_err(|error| put_error_failure(ctx.global_object, error))?;
    f.set(op.dst, result.as_value());
    Ok(())
}

/// `slow_path_create_cloned_arguments`: `ClonedArguments::createWithMachineFrame(..., ArgumentsMode::Cloned)`.
fn create_cloned_arguments_op(f: &mut SlowPathFrame, op: &OpCreateClonedArguments) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let arguments = f.call_frame.arguments_span(f.stack);
    let callee = js_callee(f);
    let result = create_cloned_arguments(ctx.global_object, &arguments, &callee).map_err(|error| put_error_failure(ctx.global_object, error))?;
    f.set(op.dst, result.as_value());
    Ok(())
}

/// `uncheckedDowncast<DirectArguments>(getOperand(arguments))`.
fn direct_arguments_operand(value: JSValue) -> LLIntResult<DirectArgumentsRef> {
    Ok(DirectArguments::from_value(&value).expect("uncheckedDowncast<DirectArguments> em registrador que não é DirectArguments"))
}

/// O handler `.asm` de `op_get_from_arguments`: `DirectArguments_storage[index]`.
fn get_from_arguments(f: &mut SlowPathFrame, op: &OpGetFromArguments) -> LLIntResult<()> {
    let arguments = direct_arguments_operand(f.get(op.arguments))?;
    let value = arguments.storage_at(op.index);
    f.set(op.dst, value);
    Ok(())
}

/// O handler `.asm` de `op_put_to_arguments`: `DirectArguments_storage[index] = value`.
fn put_to_arguments(f: &mut SlowPathFrame, op: &OpPutToArguments) -> LLIntResult<()> {
    let arguments = direct_arguments_operand(f.get(op.arguments))?;
    arguments.set_storage_at(op.index, f.get(op.value));
    Ok(())
}

/// Os handlers deste arquivo. `None` é o opcode que não é daqui.
pub(super) fn run_arguments(f: &mut SlowPathFrame, instruction: &InstructionRef) -> LLIntResult<Option<Step>> {
    match instruction.opcode_id_enum() {
        OpcodeID::op_create_direct_arguments => create_direct_arguments(f, &instruction.as_op::<OpCreateDirectArguments>())?,
        OpcodeID::op_create_scoped_arguments => create_scoped_arguments(f, &instruction.as_op::<OpCreateScopedArguments>())?,
        OpcodeID::op_create_cloned_arguments => create_cloned_arguments_op(f, &instruction.as_op::<OpCreateClonedArguments>())?,
        OpcodeID::op_get_from_arguments => get_from_arguments(f, &instruction.as_op::<OpGetFromArguments>())?,
        OpcodeID::op_put_to_arguments => put_to_arguments(f, &instruction.as_op::<OpPutToArguments>())?,
        _ => return Ok(None),
    }
    Ok(Some(Step::Next))
}
