//! Slow paths de controle de `llint/LLIntSlowPaths.cpp`: `slow_path_jtrue`/`jfalse`, `slow_path_throw`,
//! `slow_path_retrieve_and_clear_exception_if_catchable` (o `op_catch`), `slow_path_handle_traps`
//! (`op_check_traps`), `slow_path_new_func`, `slow_path_new_func_exp` (também o arrow, que no
//! `BytecodeList.rb` desta versão é `new_func_exp`), `slow_path_create_lexical_environment`,
//! `slow_path_iterator_open_get_next`, `slow_path_iterator_next_get_done`,
//! `slow_path_iterator_next_get_value`, `slow_path_arityCheck`, e a parte de montagem de frame de
//! `commonCallDirectEval`, de `varargsSetup` e de `setUpCall`.
//!
//! O que NÃO tem slow path no C++ e por isso não aparece aqui: `op_call`, `op_call_ignore_result`,
//! `op_tail_call`, `op_construct`, `op_ret`, `op_enter`, `op_loop_hint` e `op_jmp` são offlineasm puro
//! (o `call` passa por `llint_default_call`/`llint_virtual_call`/`llint_polymorphic_call`, que dependem
//! de `CallLinkInfo` e de código de máquina, que o porte não tem). `op_get_iterator`,
//! `op_new_arrow_func_exp` e `op_pop_with_scope` não existem no `BytecodeList.rb` desta versão.
//!
//! DIVERGÊNCIAS:
//!
//! - Cada função recebe o [`SlowPathFrame`] e o `Op*` decodificado. Quem tem `LLINT_BRANCH` devolve se o
//!   desvio foi tomado (o laço soma `JUMP_OFFSET`); quem tem `LLINT_THROW`/`LLINT_CHECK_EXCEPTION`
//!   devolve `Err(Thrown)` com a exceção já pendente na VM (`returnToThrow` é do laço).
//! - Sem `performLLIntGetByID`, sem metadata de cache e sem `ValueProfile` (só alimentam o JIT): os
//!   `get` de `next`/`done`/`value` são `JSObject::get`. Como o `get` do porte não lança, não há
//!   `LLINT_CHECK_EXCEPTION` depois dele.
//! - `slow_path_handle_traps`: não há `VMTraps`, então não existe `handleTraps`; devolve a exceção
//!   pendente (`throwScope.exception()`), que é o que o C++ devolve depois de tratá-los.
//! - `slow_path_arityCheck`: sem `convertToZombieFrame` e sem `ErrorHandlingScope` (nenhum dos dois
//!   existe), só o `throwStackOverflowError`.
//! - Fora, por dependerem de runtime ausente: `slow_path_new_reg_exp` (`RegExpObject`),
//!   `push_with_scope` (não é
//!   slow path; precisa de `JSWithScope`), `slow_path_size_frame_for_varargs` e `setupVarargsFrame`
//!   (`sizeOfVarargs`, `loadVarargs`), `eval()` e `createNotAFunctionError`/`createNotAConstructorError`.
//!   Para a chamada, [`classify_callee`] faz a triagem de `setUpCall` e [`prepare_direct_eval_frame`] e
//!   [`finish_varargs_frame`] montam o frame; quem lança o erro de "não é função" é o laço, até o
//!   `createNotAFunctionError` existir.

use std::rc::Rc;

use crate::bytecode::bytecode_index::BytecodeIndex;
use crate::bytecode::bytecode_ops::{
    OpCallDirectEval, OpCatch, OpCreateLexicalEnvironment, OpIteratorNext, OpJfalse, OpJtrue, OpNewFunc,
    OpNewAsyncFunc, OpNewAsyncFuncExp, OpNewAsyncGeneratorFunc, OpNewAsyncGeneratorFuncExp, OpNewFuncExp,
    OpNewGeneratorFunc, OpNewGeneratorFuncExp, OpThrow,
};
use crate::runtime::js_async_function::JSAsyncFunction;
use crate::runtime::js_async_generator_function::JSAsyncGeneratorFunction;
use crate::runtime::js_generator_function::JSGeneratorFunction;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::vm::VM;
use crate::bytecode::virtual_register::VirtualRegister;
use crate::interpreter::call_frame::{CallFrame, HEADER_SIZE_IN_REGISTERS};
use crate::llint::llint_entrypoint::MAX_FRAME_EXTENT_FOR_SLOW_PATH_CALL_IN_REGISTERS;
use crate::llint::slow_paths_arith::SlowPathFrame;
use crate::runtime::arity_check_mode::ArityCheckMode;
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::code_specialization_kind::CodeSpecializationKind;
use crate::runtime::construct_ability::ConstructAbility;
use crate::runtime::error::{create_stack_overflow_error, create_type_error};
use crate::runtime::identifier::Identifier;
use crate::runtime::js_function::{FunctionExecutableRef, JSFunction, JSFunctionRef};
use crate::runtime::js_lexical_environment::JSLexicalEnvironment;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_scope::{JSScope, JSScopeRef};
use crate::runtime::js_value::JSValue;
use crate::runtime::property_name::PropertyName;
use crate::runtime::stack_alignment::stack_alignment_registers;
use crate::runtime::symbol_table::SymbolTable;
use crate::runtime::throw_scope::{IntoException, ThrowScope};
use crate::runtime::vm::Exception;
use crate::wtf::math_extras::round_up_to_multiple_of;
use crate::wtf::text::wtf_string::String as WtfString;

/// A exceção já está pendente na VM (`throwException` feito): o laço faz o `returnToThrow`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Thrown;

/// O que um slow path que pode lançar devolve.
pub type ControlResult<T = ()> = Result<T, Thrown>;

// ---------------------------------------------------------------------------------------------
// Auxiliares dos macros `LLINT_THROW` e `getOperand`
// ---------------------------------------------------------------------------------------------

/// `LLINT_THROW(exceptionToThrow)` sem o `returnToThrow`.
fn throw_pending(f: &SlowPathFrame, thrown: impl IntoException) -> Thrown {
    let mut scope = ThrowScope::new(f.vm);
    scope.throw_exception(f.code_block.global_object(), thrown);
    Thrown
}

/// `LLINT_THROW(createTypeError(globalObject, message))`.
fn throw_type_error(f: &SlowPathFrame, message: &[u8]) -> Thrown {
    let error = create_type_error(f.code_block.global_object(), &WtfString::from_latin1(message));
    throw_pending(f, error)
}

/// `callFrame->uncheckedR(reg).Register::scope()`.
fn scope_in_register(f: &SlowPathFrame, reg: VirtualRegister) -> JSScopeRef {
    let cell_id = f.call_frame.unchecked_r(f.stack, reg).unboxed_cell();
    // Invariante (o `jsCast<JSScope*>` do C++): o gerador de bytecode só coloca escopo nesse registrador.
    JSScope::from_cell_id(cell_id).expect("registrador de escopo sem JSScope")
}

/// `value.isObject()`.
fn is_object(value: JSValue) -> bool {
    JSObject::from_value(&value).is_some()
}

/// `performLLIntGetByID` sobre um objeto (ver as divergências do cabeçalho).
fn get_property(f: &SlowPathFrame, base: JSValue, name: &Identifier) -> ControlResult<JSValue> {
    // Invariante: os três chamadores já checaram `is_object` (ou `done == false` após a checagem em `get_done`).
    let object = JSObject::from_value(&base).expect("get_property sobre valor que não é objeto");
    let value = object.get(f.vm, &PropertyName::from_identifier(name));
    // O getter pode lançar (`get next() { throw ... }`): a exceção fica pendente no `VM` e o `RETURN_IF_EXCEPTION`
    // do C++ impede de gravar o `undefined` no registrador como se a leitura tivesse dado certo.
    if f.vm.exception().is_some() {
        return Err(Thrown);
    }
    Ok(value)
}

/// A mensagem de `slow_path_iterator_*` para o resultado que não é objeto.
const ITERATOR_RESULT_NOT_OBJECT: &[u8] = b"Iterator result interface is not an object.";

// ---------------------------------------------------------------------------------------------
// Desvios, throw, catch e traps
// ---------------------------------------------------------------------------------------------

/// `slow_path_jtrue`: `LLINT_BRANCH(toBoolean(condition))`.
pub fn slow_path_jtrue(f: &mut SlowPathFrame, op: &OpJtrue) -> bool {
    f.get(op.condition).to_boolean()
}

/// `slow_path_jfalse`: `LLINT_BRANCH(!toBoolean(condition))`.
pub fn slow_path_jfalse(f: &mut SlowPathFrame, op: &OpJfalse) -> bool {
    !f.get(op.condition).to_boolean()
}

/// `slow_path_throw`: `LLINT_THROW(getOperand(value))`.
pub fn slow_path_throw(f: &mut SlowPathFrame, op: &OpThrow) -> Thrown {
    throw_pending(f, f.get(op.value))
}

/// `slow_path_retrieve_and_clear_exception_if_catchable`: a exceção pendente, já limpa da VM, ou `None`
/// quando ela não pode ser capturada (terminação). Pendente é invariante (`RELEASE_ASSERT`).
pub fn slow_path_retrieve_and_clear_exception_if_catchable(f: &mut SlowPathFrame) -> Option<Rc<Exception>> {
    let scope = ThrowScope::new(f.vm);
    let exception = scope.exception().expect("catch sem exceção pendente");
    if !scope.try_clear_exception() {
        return None;
    }
    Some(exception)
}

/// A escrita do `op_catch` depois do slow path (`LowLevelInterpreter64.asm`): o registrador `exception`
/// recebe a célula `Exception` (`JSValue::from_cell`) e `thrownValue` recebe o valor lançado.
pub fn store_caught_value(f: &mut SlowPathFrame, op: &OpCatch, exception: &Exception) {
    f.set(op.exception, JSValue::from_cell(exception.cell_id()));
    f.set(op.thrown_value, exception.value());
}

/// `slow_path_handle_traps` (`op_check_traps`): a exceção pendente depois de tratar os traps.
pub fn slow_path_handle_traps(f: &mut SlowPathFrame) -> Option<Rc<Exception>> {
    f.vm.exception()
}

// ---------------------------------------------------------------------------------------------
// Funções e escopos
// ---------------------------------------------------------------------------------------------

/// O corpo comum de `slow_path_new_func` e `slow_path_new_func_exp`: `JSFunction::create`.
fn new_function(f: &mut SlowPathFrame, dst: VirtualRegister, scope: VirtualRegister, executable: &FunctionExecutableRef) {
    let scope = scope_in_register(f, scope);
    let function = JSFunction::create(f.vm, f.code_block.global_object(), executable, scope);
    f.set(dst, JSValue::from_cell(function.cell_id()));
}

/// `slow_path_new_func`: `codeBlock->functionDecl(functionDecl)`.
pub fn slow_path_new_func(f: &mut SlowPathFrame, op: &OpNewFunc) {
    let executable = f.code_block.function_decl(op.function_decl as usize).clone();
    new_function(f, op.dst, op.scope, &executable);
}

/// `slow_path_new_func_exp`: `codeBlock->functionExpr(functionDecl)`.
pub fn slow_path_new_func_exp(f: &mut SlowPathFrame, op: &OpNewFuncExp) {
    let executable = f.code_block.function_expr(op.function_decl as usize).clone();
    new_function(f, op.dst, op.scope, &executable);
}

/// O corpo comum dos seis `slow_path_new_{generator,async,async_generator}_func[_exp]`: igual ao de
/// `new_function`, trocando só a fábrica (`JSGeneratorFunction::create` etc.).
fn new_special_function(
    f: &mut SlowPathFrame,
    dst: VirtualRegister,
    scope: VirtualRegister,
    executable: &FunctionExecutableRef,
    create: fn(&VM, &JSGlobalObject, &FunctionExecutableRef, JSScopeRef) -> JSFunctionRef,
) {
    let scope = scope_in_register(f, scope);
    let function = create(f.vm, f.code_block.global_object(), executable, scope);
    f.set(dst, JSValue::from_cell(function.cell_id()));
}

/// `slow_path_new_generator_func`: `JSGeneratorFunction::create(.., codeBlock->functionDecl(i), scope)`.
pub fn slow_path_new_generator_func(f: &mut SlowPathFrame, op: &OpNewGeneratorFunc) {
    let executable = f.code_block.function_decl(op.function_decl as usize).clone();
    new_special_function(f, op.dst, op.scope, &executable, JSGeneratorFunction::create);
}

/// `slow_path_new_generator_func_exp`: `codeBlock->functionExpr(functionDecl)`.
pub fn slow_path_new_generator_func_exp(f: &mut SlowPathFrame, op: &OpNewGeneratorFuncExp) {
    let executable = f.code_block.function_expr(op.function_decl as usize).clone();
    new_special_function(f, op.dst, op.scope, &executable, JSGeneratorFunction::create);
}

/// `slow_path_new_async_func`.
pub fn slow_path_new_async_func(f: &mut SlowPathFrame, op: &OpNewAsyncFunc) {
    let executable = f.code_block.function_decl(op.function_decl as usize).clone();
    new_special_function(f, op.dst, op.scope, &executable, JSAsyncFunction::create);
}

/// `slow_path_new_async_func_exp`.
pub fn slow_path_new_async_func_exp(f: &mut SlowPathFrame, op: &OpNewAsyncFuncExp) {
    let executable = f.code_block.function_expr(op.function_decl as usize).clone();
    new_special_function(f, op.dst, op.scope, &executable, JSAsyncFunction::create);
}

/// `slow_path_new_async_generator_func`.
pub fn slow_path_new_async_generator_func(f: &mut SlowPathFrame, op: &OpNewAsyncGeneratorFunc) {
    let executable = f.code_block.function_decl(op.function_decl as usize).clone();
    new_special_function(f, op.dst, op.scope, &executable, JSAsyncGeneratorFunction::create);
}

/// `slow_path_new_async_generator_func_exp`.
pub fn slow_path_new_async_generator_func_exp(f: &mut SlowPathFrame, op: &OpNewAsyncGeneratorFuncExp) {
    let executable = f.code_block.function_expr(op.function_decl as usize).clone();
    new_special_function(f, op.dst, op.scope, &executable, JSAsyncGeneratorFunction::create);
}

/// `slow_path_create_lexical_environment`: `JSLexicalEnvironment::create`.
pub fn slow_path_create_lexical_environment(f: &mut SlowPathFrame, op: &OpCreateLexicalEnvironment) {
    let current_scope = scope_in_register(f, op.scope);
    // Invariante: `symbol_table` é constante do bytecode, emitida pelo gerador (`jsCast<SymbolTable*>`).
    let symbol_table = SymbolTable::from_cell_id(f.get(op.symbol_table).as_cell()).expect("symbolTable sem SymbolTable");
    let initial_value = f.get(op.initial_value);
    let environment = JSLexicalEnvironment::create_in_global_object(
        f.vm,
        f.code_block.global_object(),
        Some(current_scope),
        symbol_table,
        initial_value,
    );
    f.set(op.dst, JSValue::from_cell(environment.cell_id()));
}

// ---------------------------------------------------------------------------------------------
// Iteradores
// ---------------------------------------------------------------------------------------------

/// `slow_path_iterator_open_get_next` e `slow_path_async_iterator_open_get_next` (o mesmo corpo para os dois
/// ops): `iterator.next` para o registrador `next`.
pub fn slow_path_iterator_open_get_next(f: &mut SlowPathFrame, iterator: VirtualRegister, next: VirtualRegister) -> ControlResult {
    let iterator = f.get(iterator);
    if !is_object(iterator) {
        return Err(throw_type_error(f, ITERATOR_RESULT_NOT_OBJECT));
    }
    let result = get_property(f, iterator, &f.vm.property_names.next)?;
    f.set(next, result);
    Ok(())
}

/// `slow_path_iterator_next_get_done`: `iteratorReturn.done` para o registrador `done`. O
/// `iteratorReturn` está em `value`.
pub fn slow_path_iterator_next_get_done(f: &mut SlowPathFrame, op: &OpIteratorNext) -> ControlResult {
    let iterator_return = f.get(op.value);
    if !is_object(iterator_return) {
        return Err(throw_type_error(f, ITERATOR_RESULT_NOT_OBJECT));
    }
    let result = get_property(f, iterator_return, &f.vm.property_names.done)?;
    f.set(op.done, result);
    Ok(())
}

/// `slow_path_iterator_next_get_value`: `iteratorReturn.value` para `value`, salvo se `done`.
pub fn slow_path_iterator_next_get_value(f: &mut SlowPathFrame, op: &OpIteratorNext) -> ControlResult {
    let iterator_return = f.get(op.value);
    if f.get(op.done).to_boolean() {
        return Ok(());
    }
    let result = get_property(f, iterator_return, &f.vm.property_names.value)?;
    f.set(op.value, result);
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Arity check
// ---------------------------------------------------------------------------------------------

/// `CommonSlowPaths::numberOfStackPaddingSlots`.
pub fn number_of_stack_padding_slots(number_of_parameters: u32, argument_count_including_this: usize) -> usize {
    if argument_count_including_this >= number_of_parameters as usize {
        return 0;
    }
    let alignment = stack_alignment_registers() as usize;
    let header = HEADER_SIZE_IN_REGISTERS as usize;
    let aligned_frame_size = round_up_to_multiple_of(alignment, argument_count_including_this + header);
    let aligned_frame_size_for_parameters = round_up_to_multiple_of(alignment, number_of_parameters as usize + header);
    aligned_frame_size_for_parameters - aligned_frame_size
}

/// `numberOfExtraSlots`.
fn number_of_extra_slots(argument_count_including_this: usize) -> usize {
    let frame_size = argument_count_including_this + HEADER_SIZE_IN_REGISTERS as usize;
    round_up_to_multiple_of(stack_alignment_registers() as usize, frame_size) - frame_size
}

/// `numberOfStackPaddingSlotsWithExtraSlots`.
fn number_of_stack_padding_slots_with_extra_slots(number_of_parameters: u32, argument_count_including_this: usize) -> usize {
    if argument_count_including_this >= number_of_parameters as usize {
        return 0;
    }
    number_of_stack_padding_slots(number_of_parameters, argument_count_including_this)
        + number_of_extra_slots(argument_count_including_this)
}

/// `slow_path_arityCheck`: `Ok(slotsToAdd)`, ou `Err(Thrown)` com o `RangeError` de estouro de pilha
/// pendente (`arityCheckFor` devolveu -1).
pub fn slow_path_arity_check(f: &mut SlowPathFrame) -> ControlResult<usize> {
    let argument_count = f.call_frame.argument_count_including_this(f.stack);
    let number_of_parameters = f.code_block.num_parameters();
    debug_assert!(argument_count < number_of_parameters as usize);
    let slots_to_add = number_of_stack_padding_slots_with_extra_slots(number_of_parameters, argument_count);
    let aligned_slots = round_up_to_multiple_of(stack_alignment_registers() as usize, slots_to_add);
    let new_stack_pointer = f.call_frame.registers() as isize
        - aligned_slots as isize
        - f.code_block.num_callee_locals() as isize
        - MAX_FRAME_EXTENT_FOR_SLOW_PATH_CALL_IN_REGISTERS as isize;
    if !f.stack.ensure_capacity_for(new_stack_pointer) {
        let error = create_stack_overflow_error(f.code_block.global_object());
        return Err(throw_pending(f, error));
    }
    Ok(slots_to_add)
}

/// `setUpCall`/`llint_*_call`: `calleeFrame->argumentCountIncludingThis() < codeBlock->numParameters()`.
pub fn arity_check_mode_for(argument_count_including_this: usize, number_of_parameters: u32) -> ArityCheckMode {
    if argument_count_including_this < number_of_parameters as usize {
        ArityCheckMode::MustCheckArity
    } else {
        ArityCheckMode::ArityCheckNotRequired
    }
}

// ---------------------------------------------------------------------------------------------
// Chamadas
// ---------------------------------------------------------------------------------------------

/// O resultado da triagem de `setUpCall` (a parte anterior a `prepareForExecution` e ao entrypoint).
pub enum CalleeClass {
    /// `executable->isHostFunction()`: o entrypoint é `entrypointFor(kind, MustCheckArity)`.
    HostFunction(JSFunctionRef),
    /// `FunctionExecutable`: segue `prepareForExecution` e a escolha de `ArityCheckMode`.
    Function(JSFunctionRef, FunctionExecutableRef),
    /// `kind` é construct e `constructAbility() == CannotConstruct`: `createNotAConstructorError`.
    NotAConstructor(JSFunctionRef),
    /// `getJSFunction(callee)` é nulo: `InternalFunction` ou `handleHostCall` (nativa ou "não é função").
    NotAJSFunction,
}

/// `getJSFunction(JSValue)`.
pub fn get_js_function(value: JSValue) -> Option<JSFunctionRef> {
    if !value.is_cell() {
        return None;
    }
    match cell_registry::get(value.as_cell())? {
        CellEntry::Function(function) => Some(function),
        _ => None,
    }
}

/// A triagem de `setUpCall(calleeFrame, kind, calleeAsValue)`.
pub fn classify_callee(kind: CodeSpecializationKind, callee: JSValue) -> CalleeClass {
    let Some(function) = get_js_function(callee) else {
        return CalleeClass::NotAJSFunction;
    };
    if function.is_host_function() {
        return CalleeClass::HostFunction(function);
    }
    let executable = function.js_executable();
    if kind == CodeSpecializationKind::CodeForConstruct && executable.borrow().construct_ability() == ConstructAbility::CannotConstruct {
        return CalleeClass::NotAConstructor(function);
    }
    CalleeClass::Function(function, executable)
}

/// O que `commonCallDirectEval` monta antes de chamar `eval(...)`.
pub struct DirectEvalFrame {
    pub callee_frame: CallFrame,
    pub callee: JSValue,
    pub caller_scope: JSScopeRef,
    pub this_value: JSValue,
}

/// `commonCallDirectEval` até o `eval(...)` (que o porte não tem): monta o `calleeFrame` em
/// `callFrame - argv`. `return_point` é o `genericReturnPointEntrypoint` do tamanho do opcode.
pub fn prepare_direct_eval_frame(
    f: &mut SlowPathFrame,
    op: &OpCallDirectEval,
    bytecode_index: BytecodeIndex,
    return_point: usize,
) -> DirectEvalFrame {
    let callee = f.call_frame.unchecked_r(f.stack, op.callee).js_value();
    let callee_frame = CallFrame::create((f.call_frame.registers() as isize - op.argv as isize) as usize);
    callee_frame.set_argument_count_including_this(f.stack, op.argc as i32);
    callee_frame.set_caller_frame(f.stack, Some(f.call_frame));
    callee_frame.set_callee(f.stack, callee.as_cell());
    callee_frame.set_return_pc(f.stack, return_point);
    callee_frame.set_code_block(f.stack, None);
    f.call_frame.set_current_vpc(f.stack, bytecode_index);
    // Invariante: o registrador de escopo do `op_call_direct_eval` é preenchido pelo gerador (`jsCast<JSScope*>`).
    let caller_scope = JSScope::from_cell_id(f.get(op.scope).as_cell()).expect("scope de eval sem JSScope");
    let this_value = f.get(op.this_value);
    DirectEvalFrame { callee_frame, callee, caller_scope, this_value }
}

/// O fim de `varargsSetup`, depois de `setupVarargsFrameAndSetThis`: liga o `calleeFrame` ao chamador,
/// grava o callee e registra o `currentVPC`.
pub fn finish_varargs_frame(f: &mut SlowPathFrame, callee_frame: CallFrame, callee: JSValue, bytecode_index: BytecodeIndex) {
    callee_frame.set_caller_frame(f.stack, Some(f.call_frame));
    callee_frame.set_callee(f.stack, callee.as_cell());
    f.call_frame.set_current_vpc(f.stack, bytecode_index);
}
