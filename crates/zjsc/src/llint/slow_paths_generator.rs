//! Slow paths de geradores de `runtime/CommonSlowPaths.cpp` e de `llint/LowLevelInterpreter.asm`:
//! `slow_path_create_generator`, `slow_path_create_async_generator`, `slow_path_new_generator`,
//! `slow_path_new_async_function_generator` e o handler `op_create_generator_frame_environment`.
//!
//! DIVERGÊNCIAS:
//!
//! - Sem a metadata `m_cachedCallee` (só alimenta o JIT), como nos demais slow paths do porte.
//! - `createInternalFieldObject` recebe o `callee` por `ObjectRef::from_value`, que alcança `JSFunction`
//!   (o `get` de `prototype` materializa o `reifyLazyPrototype`). O `asObject(callee)` do C++ é invariante
//!   (o `callee` de gerador é sempre a `JSFunction` do gerador): vira `expect`.
//! - `op_create_generator_frame_environment` é `notSupported()` no `.asm` (marcador reescrito pela
//!   `BytecodeGeneratorification` antes da execução): chegar nele é `crash()`.

use crate::bytecode::bytecode_ops::{
    OpCreateAsyncGenerator, OpCreateGenerator, OpCreateGeneratorFrameEnvironment, OpNewAsyncFunctionGenerator, OpNewGenerator,
};
use crate::bytecode::virtual_register::VirtualRegister;
use crate::llint::slow_paths_arith::SlowPathFrame;
use crate::llint::LLIntResult;
use crate::runtime::internal_function::InternalFunction;
use crate::runtime::js_async_function_generator::JSAsyncFunctionGenerator;
use crate::runtime::js_async_generator::JSAsyncGenerator;
use crate::runtime::js_generator::JSGenerator;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;

/// `createInternalFieldObject<JSClass>(globalObject, vm, codeBlock, bytecode, constructorAsObject,
/// baseStructure)`: a estrutura vem de `createSubclassStructure` sobre o `callee`, e o objeto de
/// `JSClass::create`. `create` devolve o valor da célula criada.
fn create_internal_field_object(
    f: &SlowPathFrame,
    callee: VirtualRegister,
    base_structure: impl FnOnce(&VM, &JSGlobalObject) -> StructureRef,
    create: impl FnOnce(&VM, &StructureRef) -> JSValue,
) -> LLIntResult<JSValue> {
    let global_object = &**f.code_block.global_object();
    let constructor = ObjectRef::from_value(&f.get(callee)).expect("ASSERT: asObject(GET(bytecode.m_callee).jsValue())");
    let base = base_structure(f.vm, global_object);
    let structure = InternalFunction::create_subclass_structure(global_object, &constructor, base)
        .map_err(|thrown| crate::llint::slow_paths::thrown_failure(global_object, thrown))?;
    Ok(create(f.vm, &structure))
}

/// `slow_path_create_generator`.
pub fn slow_path_create_generator(f: &mut SlowPathFrame, op: &OpCreateGenerator) -> LLIntResult<()> {
    let result = create_internal_field_object(
        f,
        op.callee,
        |_, global| global.generator_structure(),
        |vm, structure| JSGenerator::create(vm, structure).as_value(),
    )?;
    f.set(op.dst, result);
    Ok(())
}

/// `slow_path_create_async_generator`.
pub fn slow_path_create_async_generator(f: &mut SlowPathFrame, op: &OpCreateAsyncGenerator) -> LLIntResult<()> {
    let result = create_internal_field_object(
        f,
        op.callee,
        |_, global| global.async_generator_structure(),
        |vm, structure| JSAsyncGenerator::create(vm, structure).as_value(),
    )?;
    f.set(op.dst, result);
    Ok(())
}

/// `slow_path_new_generator`: `JSGenerator::create(vm, globalObject->generatorStructure())`.
pub fn slow_path_new_generator(f: &mut SlowPathFrame, op: &OpNewGenerator) {
    let global_object = &**f.code_block.global_object();
    let structure = global_object.generator_structure();
    f.set(op.dst, JSGenerator::create(f.vm, &structure).as_value());
}

/// `slow_path_new_async_function_generator`:
/// `JSAsyncFunctionGenerator::create(vm, globalObject->asyncFunctionGeneratorStructure())`.
pub fn slow_path_new_async_function_generator(f: &mut SlowPathFrame, op: &OpNewAsyncFunctionGenerator) {
    let global_object = &**f.code_block.global_object();
    let structure = global_object.async_function_generator_structure();
    f.set(op.dst, JSAsyncFunctionGenerator::create(f.vm, &structure).as_value());
}

/// `llintOp(op_create_generator_frame_environment, ..., notSupported())`: o marcador some na
/// `BytecodeGeneratorification`, então executá-lo é `crash()`.
pub fn op_create_generator_frame_environment(_f: &mut SlowPathFrame, _op: &OpCreateGeneratorFrameEnvironment) -> ! {
    unreachable!("op_create_generator_frame_environment: notSupported()")
}
