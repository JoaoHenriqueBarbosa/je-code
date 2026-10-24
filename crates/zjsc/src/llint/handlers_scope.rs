//! Handlers de escopo, frame e geradores que não tinham braço no laço: `op_get_parent_scope`,
//! `op_push_with_scope`, `op_create_lexical_environment`, `op_resolve_scope_for_hoisting_func_decl_in_eval`,
//! `op_create_rest`, `op_create_generator`, `op_create_async_generator`, `op_new_generator` e
//! `op_new_async_function_generator`.
//!
//! O ponto de entrada é [`run_scope`], no mesmo formato de `dispatch_ext::run_ext`. Os de gerador e o de
//! ambiente léxico só chamam os slow paths que já existem em `slow_paths_generator` e `slow_paths_control`.
//!
//! DIVERGÊNCIAS:
//!
//! - `op_push_with_scope` converte o operando com `toObject`: objeto passa, primitivo vira o invólucro e
//!   `undefined`/`null` lançam `createNotAnObjectError`.
//! - `op_create_rest` usa `JSGlobalObject::array_structure` no lugar de `restParameterStructure()` (a única
//!   estrutura de array que o porte tem; o `JSArray` converte a forma ao gravar os índices), como o
//!   `op_new_array`.
//! - Não estão aqui: os de `arguments` (`handlers_arguments`), os `op_enumerator_*` (`handlers_enumerator`),
//!   `op_spread`, `op_new_array_with_spread` e `op_iterator_*` (`handlers_iterator`); `op_new_promise` e
//!   `op_create_promise` (sem `promiseStructure()` no `JSGlobalObject`), `op_has_private_name`,
//!   `op_has_private_name`, `op_get_private_name`,
//!   `op_put_private_name` (sem campos privados no `JSObject`), `op_new_array_buffer`,
//!   `op_new_array_with_species` (agora em `handlers_object`), `op_async_iterator_*`,
//!   `op_yield` e `op_create_generator_frame_environment`
//!   (`notSupported()` no `.asm`: o gerador de geradores os reescreve) e `op_unreachable` (`crash()`).

use std::rc::Rc;

use crate::bytecode::bytecode_ops::{
    OpCreateAsyncGenerator, OpCreateGenerator, OpCreateLexicalEnvironment, OpCreateRest, OpGetParentScope,
    OpNewAsyncFunctionGenerator, OpNewGenerator, OpPushWithScope, OpResolveScopeForHoistingFuncDeclInEval,
};
use crate::bytecode::instruction_stream::Ref as InstructionRef;
use crate::bytecode::opcode::OpcodeID;
use crate::llint::dispatch::Step;
use crate::llint::dispatch_ext::check_exception;
use crate::llint::slow_paths::scope_of;
use crate::llint::slow_paths_arith::SlowPathFrame;
use crate::llint::slow_paths_control::{get_js_function, slow_path_create_lexical_environment};
use crate::llint::slow_paths_generator::{
    slow_path_create_async_generator, slow_path_create_generator, slow_path_new_async_function_generator,
    slow_path_new_generator,
};
use crate::llint::{LLIntFailure, LLIntResult};
use crate::runtime::js_array::construct_array;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_scope::{JSScope, JSScopeRef};
use crate::runtime::js_value::JSValue;
use crate::runtime::js_with_scope::JSWithScope;

/// `loadp JSScope::m_next[scope]`: o escopo pai do registrador `scope`.
fn get_parent_scope(f: &mut SlowPathFrame, op: &OpGetParentScope) -> LLIntResult<()> {
    let scope = scope_of(f.get(op.scope))?;
    // `loadp JSScope::m_next`: o `JSGlobalObject` (fim da cadeia) não tem pai, e o `loadp` guardaria o ponteiro nulo.
    f.set(op.dst, scope.next().map_or_else(JSValue::empty, |parent| parent.into_js_value()));
    Ok(())
}

/// `slow_path_push_with_scope`: `JSWithScope::create(vm, globalObject, currentScope, toObject(newScope))`.
fn push_with_scope(f: &mut SlowPathFrame, op: &OpPushWithScope) -> LLIntResult<()> {
    let raw_scope = f.get(op.new_scope);
    let new_scope = if JSObject::from_value(&raw_scope).is_some() || get_js_function(raw_scope).is_some() {
        raw_scope
    } else {
        // `toObject(globalObject, newScope)`: `undefined` e `null` lançam `createNotAnObjectError` com o texto-fonte
        // do `ExpressionInfo` do `with` (`undefined is not an object (evaluating 'undefined')`), o resto vira o invólucro.
        if raw_scope.is_undefined_or_null() {
            return Err(crate::llint::slow_paths_object::throw_not_an_object(f, raw_scope));
        }
        let object = raw_scope.to_object(&**f.code_block.global_object()).ok_or(LLIntFailure::Thrown)?;
        object.as_value()
    };
    let current_scope = scope_of(f.get(op.current_scope))?;
    let with_scope = JSWithScope::create(f.vm, f.code_block.global_object(), Some(current_scope), new_scope);
    f.set(op.dst, JSScopeRef::WithScope(Rc::clone(&with_scope)).into_js_value());
    Ok(())
}

/// `slow_path_resolve_scope_for_hoisting_func_decl_in_eval`: o valor vazio é o `{ }` do `CHECK_EXCEPTION`.
fn resolve_scope_for_hoisting_func_decl_in_eval(
    f: &mut SlowPathFrame,
    op: &OpResolveScopeForHoistingFuncDeclInEval,
) -> LLIntResult<()> {
    let ident = f.code_block.identifier(op.property as usize);
    let scope = scope_of(f.get(op.scope))?;
    let resolved = JSScope::resolve_scope_for_hoisting_func_decl_in_eval(f.code_block.global_object(), &scope, &ident);
    check_exception(f)?;
    f.set(op.dst, resolved);
    Ok(())
}

/// `slow_path_create_rest`: o array com os argumentos a partir de `numParametersToSkip`.
fn create_rest(f: &mut SlowPathFrame, op: &OpCreateRest) {
    let skip = op.num_parameters_to_skip as usize;
    let values: Vec<JSValue> = f.call_frame.arguments_span(f.stack).into_iter().skip(skip).collect();
    let structure = f.code_block.global_object().array_structure();
    let array = construct_array(f.vm, &structure, &values);
    f.set(op.dst, JSValue::from_cell(array.cell_id()));
}

/// Os handlers deste arquivo. `None` é o opcode que não é daqui.
pub(super) fn run_scope(f: &mut SlowPathFrame, instruction: &InstructionRef) -> LLIntResult<Option<Step>> {
    match instruction.opcode_id_enum() {
        OpcodeID::op_get_parent_scope => get_parent_scope(f, &instruction.as_op::<OpGetParentScope>())?,
        OpcodeID::op_push_with_scope => push_with_scope(f, &instruction.as_op::<OpPushWithScope>())?,
        OpcodeID::op_create_lexical_environment => {
            slow_path_create_lexical_environment(f, &instruction.as_op::<OpCreateLexicalEnvironment>());
        }
        OpcodeID::op_resolve_scope_for_hoisting_func_decl_in_eval => {
            resolve_scope_for_hoisting_func_decl_in_eval(f, &instruction.as_op::<OpResolveScopeForHoistingFuncDeclInEval>())?
        }
        OpcodeID::op_create_rest => create_rest(f, &instruction.as_op::<OpCreateRest>()),
        OpcodeID::op_create_generator => slow_path_create_generator(f, &instruction.as_op::<OpCreateGenerator>())?,
        OpcodeID::op_create_async_generator => {
            slow_path_create_async_generator(f, &instruction.as_op::<OpCreateAsyncGenerator>())?
        }
        OpcodeID::op_new_generator => slow_path_new_generator(f, &instruction.as_op::<OpNewGenerator>()),
        OpcodeID::op_new_async_function_generator => {
            slow_path_new_async_function_generator(f, &instruction.as_op::<OpNewAsyncFunctionGenerator>())
        }
        _ => return Ok(None),
    }
    Ok(Some(Step::Next))
}
