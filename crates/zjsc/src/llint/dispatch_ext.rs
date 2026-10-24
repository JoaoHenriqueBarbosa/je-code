//! Os handlers que o `dispatch.rs` não tem e o desenrolar de exceção do laço: `op_to_this`, `op_construct`,
//! `op_throw`, `op_catch`, os `typeof`/`is_*`, `get_by_id`/`put_by_id`/`get_by_val`/`put_by_val` (via
//! `slow_paths_object`), `new_object`, `new_array`, `create_this`, `instanceof`, `in_by_id`, `del_by_*`,
//! `get_length`, os saltos sobre `null`, a aritmética e o bit a bit sem caminho rápido, as conversões e
//! `switch_imm`/`switch_char`/`switch_string`.
//!
//! O ponto de entrada é [`run_ext`], o braço final do `match` de `Interpreter::dispatch_loop_from`. Opcode
//! que não está aqui devolve `None` e o laço responde `UnportedOpcode`.
//!
//! DIVERGÊNCIAS:
//!
//! - Desenrolar de exceção (`LowLevelInterpreter64.asm` `.handleException` e `Interpreter::unwind`): o C++
//!   procura o `HandlerInfo` no `CodeBlock` do frame que lançou e salta para `handler->target`, que é o
//!   `op_catch`. Aqui o `Err(Thrown)` sai de `dispatch_loop_from`; [`Interpreter::dispatch_loop`] consulta
//!   `handlerForBytecodeIndex(currentVPC, AnyHandler)` e retoma o laço no alvo. O `sp` não precisa de
//!   restauração: `llint_execute` o devolve ao valor do chamador em toda saída, inclusive em erro. A exceção
//!   de terminação (`VM::isTerminationException`) não é capturável e sobe sem consulta.
//! - Os slow paths aritméticos não devolvem resultado e são seguidos de `LLINT_CHECK_EXCEPTION`, como o
//!   `callSlowPath` do `.asm`.
//! - `slow_path_to_this` com `this` primitivo em modo sloppy vira o invólucro (`JSValue::toObject`); `undefined`
//!   e `null` viram o `globalThis`.
//! - `op_get_length` sobre string e `JSArray` lê o comprimento direto (o `.asm` faz o mesmo no caminho
//!   rápido); sobre os demais objetos é o `get` de `length`; sobre número, booleano, `Symbol` e `BigInt` é a
//!   busca no protótipo do invólucro (`get_primitive_property`), sem criá-lo.
//! - O `op_construct` (agora em `dispatch.rs`, com o `op_call`) não passa por `ProtoCallFrame` (só `vmEntryToJavaScript` usa): o `.asm` monta o
//!   cabeçalho do callee no próprio frame, como o `op_call` (`Interpreter::call_function`, parametrizado
//!   no `CodeSpecializationKind`), e a aridade entra em `arity_fixup` no `llint_execute` do callee.

use crate::bytecode::bytecode_ops::{
    OpBitand, OpBitnot, OpBitor, OpBitxor, OpCatch, OpCreateThis, OpDec, OpDelById, OpDelByVal, OpDiv,
    OpEqNull, OpGetById, OpGetByVal, OpGetLength, OpInById, OpInc, OpInstanceof, OpIsBoolean, OpIsCallable, OpIsEmpty,
    OpIsNumber, OpIsObject, OpIsUndefinedOrNull, OpJeqNull, OpJeqPtr, OpJneqNull, OpJneqPtr, OpJnundefinedOrNull, OpJundefinedOrNull,
    OpLshift, OpMod, OpNegate, OpNeqNull, OpNewAsyncFunc, OpNewAsyncFuncExp, OpNewAsyncGeneratorFunc,
    OpNewAsyncGeneratorFuncExp, OpNewFuncExp, OpNewGeneratorFunc, OpNewGeneratorFuncExp, OpNewObject, OpNot, OpPow,
    OpPutById, OpPutByVal, OpRshift,
    OpSwitchChar, OpSwitchImm, OpSwitchString, OpThrow, OpToNumber, OpToNumeric, OpToString, OpToThis, OpTypeof,
    OpTypeofIsFunction, OpTypeofIsObject, OpTypeofIsUndefined, OpUnsigned, OpUrshift,
};
use crate::bytecode::code_block::CodeBlockRef;
use crate::bytecode::instruction_stream::Ref as InstructionRef;
use crate::bytecode::opcode::OpcodeID;
use crate::interpreter::call_frame::CallFrame;
use crate::interpreter::interpreter::Interpreter;
use crate::llint::dispatch::{DecodedLabel, LoopExit, Step};
use crate::llint::slow_paths_arith::{
    slow_path_bitand, slow_path_bitnot, slow_path_bitor, slow_path_bitxor, slow_path_dec, slow_path_div, slow_path_inc,
    slow_path_is_callable, slow_path_lshift, slow_path_mod, slow_path_negate, slow_path_pow, slow_path_rshift,
    slow_path_to_number, slow_path_to_numeric, slow_path_to_string, slow_path_typeof, slow_path_typeof_is_function,
    slow_path_typeof_is_object, slow_path_unsigned, slow_path_urshift, SlowPathFrame,
};
use crate::llint::slow_paths_control::{
    get_js_function, slow_path_new_async_func, slow_path_new_async_func_exp, slow_path_new_async_generator_func,
    slow_path_new_async_generator_func_exp, slow_path_new_func_exp, slow_path_new_generator_func,
    slow_path_new_generator_func_exp, slow_path_retrieve_and_clear_exception_if_catchable, slow_path_throw,
    store_caught_value,
};
use crate::llint::slow_paths_object::{
    slow_path_create_this, slow_path_del_by_id, slow_path_del_by_val, slow_path_get_by_id, slow_path_get_by_val,
    slow_path_in_by_id, slow_path_instanceof, slow_path_new_object, slow_path_put_by_id, slow_path_put_by_val, throw_not_an_object,
};
use crate::llint::{LLIntFailure, LLIntResult};
use crate::runtime::js_array::JSArray;
use crate::runtime::js_global_object::JSGlobalObjectRef;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_scope::JSScope;
use crate::runtime::js_value::{js_boolean, JSValue};
use crate::runtime::property_name::PropertyName;

/// `LLINT_CHECK_EXCEPTION`.
pub(super) fn check_exception(f: &SlowPathFrame) -> LLIntResult<()> {
    if f.vm.has_exception() {
        return Err(LLIntFailure::Thrown);
    }
    Ok(())
}

/// Um slow path que escreve o `dst` e não devolve resultado: o `callSlowPath` do `.asm` e o
/// `LLINT_CHECK_EXCEPTION` que o segue.
macro_rules! plain_slow_path {
    ($f:ident, $instruction:ident, $op:ty, $path:path) => {{
        let op: $op = $instruction.as_op();
        $path($f, &op);
        check_exception($f)?;
        Step::Next
    }};
}

/// Um `op_new_*func*`: o slow path escreve o `dst` e não lança.
macro_rules! new_function_op {
    ($f:ident, $instruction:ident, $op:ty, $path:path) => {{
        let op: $op = $instruction.as_op();
        $path($f, &op);
        Step::Next
    }};
}

/// Um slow path de `slow_paths_object`, que já devolve o `LLIntResult` (exceção pendente no `Err`).
macro_rules! fallible_slow_path {
    ($f:ident, $instruction:ident, $op:ty, $path:path) => {{
        let op: $op = $instruction.as_op();
        $path($f, &op)?;
        Step::Next
    }};
}

/// Um opcode `dst = predicado(operand)` sem slow path no C++ (`.asm` puro).
macro_rules! value_predicate {
    ($f:ident, $instruction:ident, $op:ty, $predicate:expr) => {{
        let op: $op = $instruction.as_op();
        let value = $f.get(op.operand);
        $f.set(op.dst, js_boolean(($predicate)(value)));
        Step::Next
    }};
}

/// Um `jeq_null`/`jneq_null`/`jundefined_or_null`/`jnundefined_or_null`.
macro_rules! null_jump {
    ($f:ident, $instruction:ident, $op:ty, $taken:expr) => {{
        let op: $op = $instruction.as_op();
        if ($taken)($f.get(op.value)) {
            Step::Jump(op.target_label.target(&DecodedLabel))
        } else {
            Step::Next
        }
    }};
}

/// Um `jeq_ptr`/`jneq_ptr`: `bpeq`/`bpneq` do valor contra a constante `specialPointer` (os bits, como no
/// `.asm`). O `m_hasJumped` do `Metadata` do `jneq_ptr` só alimenta o JIT e não existe.
macro_rules! ptr_jump {
    ($f:ident, $instruction:ident, $op:ty, $taken:expr) => {{
        let op: $op = $instruction.as_op();
        if ($taken)($f.get(op.special_pointer).encode() == $f.get(op.value).encode()) {
            Step::Jump(op.target_label.target(&DecodedLabel))
        } else {
            Step::Next
        }
    }};
}

impl Interpreter {
    /// O laço de `dispatch()` com o desenrolar de exceção: roda `dispatch_loop_from` e, quando ele termina
    /// em `Thrown` num frame que tem `HandlerInfo` para o `currentVPC`, retoma no alvo do handler (o
    /// `op_catch`). `op_tail_call` troca o frame corrente pelo do callee (`prepareForTailCall`): o laço
    /// recomeça no pc 0 do callee, com o prólogo dele, sem recursão nativa.
    pub(super) fn dispatch_loop(
        &mut self,
        call_frame: CallFrame,
        code_block: &CodeBlockRef,
        global_object: &JSGlobalObjectRef,
    ) -> LLIntResult<JSValue> {
        let mut call_frame = call_frame;
        let mut code_block = code_block.clone();
        let mut global_object = global_object.clone();
        let mut pc = 0;
        loop {
            match self.dispatch_loop_from(call_frame, &code_block, &global_object, pc) {
                Ok(LoopExit::Return(value)) => return Ok(value),
                Ok(LoopExit::TailCall { frame, entry }) => {
                    (call_frame, code_block, global_object) = self.enter_frame(frame, entry, false)?;
                    pc = 0;
                }
                Err(LLIntFailure::Thrown) => match self.unwind(call_frame, &code_block, &global_object) {
                    Some(target) => pc = target,
                    None => return Err(LLIntFailure::Thrown),
                },
                Err(other) => return Err(other),
            }
        }
    }
}

/// Os handlers deste arquivo. `None` é o opcode sem handler. `#[inline(never)]`: chamada de um só ponto, o
/// otimizador a embutiria em `dispatch_loop_from` e o frame de todos os handlers raros ficaria na pilha nativa a
/// cada nível de JS.
#[inline(never)]
pub(super) fn run_ext(f: &mut SlowPathFrame, instruction: &InstructionRef) -> LLIntResult<Option<Step>> {
    let step = match instruction.opcode_id_enum() {
        // llintOp(op_to_this): slow_path_to_this.
        OpcodeID::op_to_this => {
            to_this(f, &instruction.as_op::<OpToThis>())?;
            Step::Next
        }
        // slow_path_throw: LLINT_THROW(getOperand(value)).
        OpcodeID::op_throw => {
            let op: OpThrow = instruction.as_op();
            slow_path_throw(f, &op);
            return Err(LLIntFailure::Thrown);
        }
        // llintOp(op_catch): a exceção pendente vai para `exception` e o valor lançado para `thrownValue`.
        OpcodeID::op_catch => {
            let op: OpCatch = instruction.as_op();
            let Some(exception) = slow_path_retrieve_and_clear_exception_if_catchable(f) else {
                // Terminação: a exceção segue pendente e sobe.
                return Err(LLIntFailure::Thrown);
            };
            store_caught_value(f, &op, &exception);
            Step::Next
        }

        // slow_path_new_func_exp e as variantes generator/async/async generator (`slow_paths_control`).
        OpcodeID::op_new_func_exp => new_function_op!(f, instruction, OpNewFuncExp, slow_path_new_func_exp),
        OpcodeID::op_new_generator_func => {
            new_function_op!(f, instruction, OpNewGeneratorFunc, slow_path_new_generator_func)
        }
        OpcodeID::op_create_generator => fallible_slow_path!(
            f,
            instruction,
            crate::bytecode::bytecode_ops::OpCreateGenerator,
            crate::llint::slow_paths_generator::slow_path_create_generator
        ),
        OpcodeID::op_create_async_generator => fallible_slow_path!(
            f,
            instruction,
            crate::bytecode::bytecode_ops::OpCreateAsyncGenerator,
            crate::llint::slow_paths_generator::slow_path_create_async_generator
        ),
        OpcodeID::op_new_generator => {
            let op: crate::bytecode::bytecode_ops::OpNewGenerator = instruction.as_op();
            crate::llint::slow_paths_generator::slow_path_new_generator(f, &op);
            Step::Next
        }
        OpcodeID::op_new_async_function_generator => {
            let op: crate::bytecode::bytecode_ops::OpNewAsyncFunctionGenerator = instruction.as_op();
            crate::llint::slow_paths_generator::slow_path_new_async_function_generator(f, &op);
            Step::Next
        }
        OpcodeID::op_create_generator_frame_environment => {
            let op: crate::bytecode::bytecode_ops::OpCreateGeneratorFrameEnvironment = instruction.as_op();
            crate::llint::slow_paths_generator::op_create_generator_frame_environment(f, &op)
        }
        OpcodeID::op_new_generator_func_exp => {
            new_function_op!(f, instruction, OpNewGeneratorFuncExp, slow_path_new_generator_func_exp)
        }
        OpcodeID::op_new_async_func => new_function_op!(f, instruction, OpNewAsyncFunc, slow_path_new_async_func),
        OpcodeID::op_new_async_func_exp => {
            new_function_op!(f, instruction, OpNewAsyncFuncExp, slow_path_new_async_func_exp)
        }
        OpcodeID::op_new_async_generator_func => {
            new_function_op!(f, instruction, OpNewAsyncGeneratorFunc, slow_path_new_async_generator_func)
        }
        OpcodeID::op_new_async_generator_func_exp => {
            new_function_op!(f, instruction, OpNewAsyncGeneratorFuncExp, slow_path_new_async_generator_func_exp)
        }

        // Acesso a propriedade e criação de objeto (slow_paths_object).
        OpcodeID::op_get_by_id => fallible_slow_path!(f, instruction, OpGetById, slow_path_get_by_id),
        OpcodeID::op_put_by_id => fallible_slow_path!(f, instruction, OpPutById, slow_path_put_by_id),
        OpcodeID::op_get_by_val => fallible_slow_path!(f, instruction, OpGetByVal, slow_path_get_by_val),
        OpcodeID::op_put_by_val => fallible_slow_path!(f, instruction, OpPutByVal, slow_path_put_by_val),
        OpcodeID::op_in_by_id => fallible_slow_path!(f, instruction, OpInById, slow_path_in_by_id),
        OpcodeID::op_del_by_id => fallible_slow_path!(f, instruction, OpDelById, slow_path_del_by_id),
        OpcodeID::op_del_by_val => fallible_slow_path!(f, instruction, OpDelByVal, slow_path_del_by_val),
        OpcodeID::op_new_object => fallible_slow_path!(f, instruction, OpNewObject, slow_path_new_object),
        OpcodeID::op_create_this => fallible_slow_path!(f, instruction, OpCreateThis, slow_path_create_this),
        OpcodeID::op_instanceof => fallible_slow_path!(f, instruction, OpInstanceof, slow_path_instanceof),
        OpcodeID::op_get_length => {
            get_length(f, &instruction.as_op::<OpGetLength>())?;
            Step::Next
        }

        // typeof e is_*.
        OpcodeID::op_typeof => plain_slow_path!(f, instruction, OpTypeof, slow_path_typeof),
        OpcodeID::op_typeof_is_object => plain_slow_path!(f, instruction, OpTypeofIsObject, slow_path_typeof_is_object),
        OpcodeID::op_typeof_is_function => {
            plain_slow_path!(f, instruction, OpTypeofIsFunction, slow_path_typeof_is_function)
        }
        OpcodeID::op_is_callable => plain_slow_path!(f, instruction, OpIsCallable, slow_path_is_callable),
        OpcodeID::op_typeof_is_undefined => {
            value_predicate!(f, instruction, OpTypeofIsUndefined, |v: JSValue| v.is_undefined())
        }
        OpcodeID::op_is_empty => value_predicate!(f, instruction, OpIsEmpty, |v: JSValue| v.is_empty()),
        OpcodeID::op_is_undefined_or_null => {
            value_predicate!(f, instruction, OpIsUndefinedOrNull, |v: JSValue| v.is_undefined_or_null())
        }
        OpcodeID::op_is_boolean => value_predicate!(f, instruction, OpIsBoolean, |v: JSValue| v.is_boolean()),
        OpcodeID::op_is_number => value_predicate!(f, instruction, OpIsNumber, |v: JSValue| v.is_number()),
        OpcodeID::op_is_object => value_predicate!(f, instruction, OpIsObject, |v: JSValue| {
            JSObject::from_value(&v).is_some() || get_js_function(v).is_some()
        }),
        OpcodeID::op_eq_null => value_predicate!(f, instruction, OpEqNull, |v: JSValue| v.is_undefined_or_null()),
        OpcodeID::op_neq_null => value_predicate!(f, instruction, OpNeqNull, |v: JSValue| !v.is_undefined_or_null()),
        OpcodeID::op_not => value_predicate!(f, instruction, OpNot, |v: JSValue| !v.to_boolean()),

        // Saltos sobre null/undefined.
        OpcodeID::op_jeq_null => null_jump!(f, instruction, OpJeqNull, |v: JSValue| v.is_undefined_or_null()),
        OpcodeID::op_jneq_null => null_jump!(f, instruction, OpJneqNull, |v: JSValue| !v.is_undefined_or_null()),
        OpcodeID::op_jeq_ptr => ptr_jump!(f, instruction, OpJeqPtr, |equal: bool| equal),
        OpcodeID::op_jneq_ptr => ptr_jump!(f, instruction, OpJneqPtr, |equal: bool| !equal),
        OpcodeID::op_jundefined_or_null => {
            null_jump!(f, instruction, OpJundefinedOrNull, |v: JSValue| v.is_undefined_or_null())
        }
        OpcodeID::op_jnundefined_or_null => {
            null_jump!(f, instruction, OpJnundefinedOrNull, |v: JSValue| !v.is_undefined_or_null())
        }

        // Aritmética, bit a bit e conversões (slow_paths_arith).
        OpcodeID::op_div => plain_slow_path!(f, instruction, OpDiv, slow_path_div),
        OpcodeID::op_mod => plain_slow_path!(f, instruction, OpMod, slow_path_mod),
        OpcodeID::op_pow => plain_slow_path!(f, instruction, OpPow, slow_path_pow),
        OpcodeID::op_inc => plain_slow_path!(f, instruction, OpInc, slow_path_inc),
        OpcodeID::op_dec => plain_slow_path!(f, instruction, OpDec, slow_path_dec),
        OpcodeID::op_negate => plain_slow_path!(f, instruction, OpNegate, slow_path_negate),
        OpcodeID::op_lshift => plain_slow_path!(f, instruction, OpLshift, slow_path_lshift),
        OpcodeID::op_rshift => plain_slow_path!(f, instruction, OpRshift, slow_path_rshift),
        OpcodeID::op_urshift => plain_slow_path!(f, instruction, OpUrshift, slow_path_urshift),
        OpcodeID::op_unsigned => plain_slow_path!(f, instruction, OpUnsigned, slow_path_unsigned),
        OpcodeID::op_bitnot => plain_slow_path!(f, instruction, OpBitnot, slow_path_bitnot),
        OpcodeID::op_bitand => plain_slow_path!(f, instruction, OpBitand, slow_path_bitand),
        OpcodeID::op_bitor => plain_slow_path!(f, instruction, OpBitor, slow_path_bitor),
        OpcodeID::op_bitxor => plain_slow_path!(f, instruction, OpBitxor, slow_path_bitxor),
        OpcodeID::op_to_number => plain_slow_path!(f, instruction, OpToNumber, slow_path_to_number),
        OpcodeID::op_to_numeric => plain_slow_path!(f, instruction, OpToNumeric, slow_path_to_numeric),
        OpcodeID::op_to_string => plain_slow_path!(f, instruction, OpToString, slow_path_to_string),

        // switch_imm, switch_char, switch_string: `jump(offset)` relativo ao começo da instrução.
        OpcodeID::op_switch_imm => {
            let op: OpSwitchImm = instruction.as_op();
            let scrutinee = f.get(op.scrutinee);
            Step::Jump(f.code_block.with_unlinked_switch_jump_table(op.table_index as usize, |table| {
                int32_scrutinee(scrutinee).map_or(table.default_offset, |value| table.offset_for_value(value))
            }))
        }
        OpcodeID::op_switch_char => {
            let op: OpSwitchChar = instruction.as_op();
            let scrutinee = f.get(op.scrutinee);
            Step::Jump(f.code_block.with_unlinked_switch_jump_table(op.table_index as usize, |table| {
                if scrutinee.is_string() {
                    let string = scrutinee.as_js_string();
                    if string.length() == 1 {
                        return table.offset_for_value(i32::from(string.get_value_impl().char_at(0)));
                    }
                }
                table.default_offset
            }))
        }
        OpcodeID::op_switch_string => {
            let op: OpSwitchString = instruction.as_op();
            let scrutinee = f.get(op.scrutinee);
            // `slow_path_switch_string`: `asString(scrutinee)->value(globalObject)` resolve a rope (e lança na
            // falta de memória) antes de procurar na tabela.
            let value = if scrutinee.is_string() {
                let resolved = scrutinee.as_js_string().try_resolve();
                Some(resolved.ok_or_else(|| crate::llint::slow_paths::throw_out_of_memory_error(f.code_block.global_object()))?)
            } else {
                None
            };
            Step::Jump(f.code_block.with_unlinked_string_switch_jump_table(op.table_index as usize, |table| {
                match value.as_ref().and_then(|value| value.impl_()) {
                    Some(value) => table.offset_for_value(value),
                    None => table.default_offset,
                }
            }))
        }
        // slowPathOp(unreachable): `slow_path_unreachable` é `UNREACHABLE_FOR_PLATFORM()`, que no C++ é
        // `RELEASE_ASSERT_NOT_REACHED()`. O gerador só emite o opcode depois de um `throw` incondicional.
        OpcodeID::op_unreachable => unreachable!("slow_path_unreachable: RELEASE_ASSERT_NOT_REACHED()"),
        _ => {
            if let Some(step) = crate::llint::handlers_misc::run_misc(f, instruction)? {
                return Ok(Some(step));
            }
            if let Some(step) = crate::llint::handlers_object::run_object(f, instruction)? {
                return Ok(Some(step));
            }
            if let Some(step) = crate::llint::handlers_scope::run_scope(f, instruction)? {
                return Ok(Some(step));
            }
            if let Some(step) = crate::llint::handlers_accessor::run_accessor(f, instruction)? {
                return Ok(Some(step));
            }
            if let Some(step) = crate::llint::handlers_arguments::run_arguments(f, instruction)? {
                return Ok(Some(step));
            }
            if let Some(step) = crate::llint::handlers_enumerator::run_enumerator(f, instruction)? {
                return Ok(Some(step));
            }
            if let Some(step) = crate::llint::handlers_array::run_array(f, instruction)? {
                return Ok(Some(step));
            }
            if let Some(step) = crate::llint::handlers_async::run_async(f, instruction)? {
                return Ok(Some(step));
            }
            if let Some(step) = crate::llint::handlers_private_brand::run_private_brand(f, instruction)? {
                return Ok(Some(step));
            }
            return crate::llint::handlers_iterator::run_iterator(f, instruction);
        }
    };
    Ok(Some(step))
}

/// `globalObject->globalThis()`: o `JSGlobalObject::create` já o fixou (`setGlobalThis` em `finishCreation`),
/// então o `m_globalThis` nunca é nulo depois da criação, como no C++ (que o lê sem checar).
fn global_this_value(f: &SlowPathFrame) -> JSValue {
    f.code_block.global_object().global_this().expect("globalThis fixado em JSGlobalObject::create").as_value()
}

/// `slow_path_to_this`: `JSValue::toThis(globalObject, ecmaMode)` sobre `srcDst`. Objeto e função ficam; em
/// modo estrito tudo fica; em sloppy `undefined` e `null` viram o `globalThis`.
fn to_this(f: &mut SlowPathFrame, op: &OpToThis) -> LLIntResult<()> {
    let value = f.get(op.src_dst);
    let is_object = JSObject::from_value(&value).is_some() || get_js_function(value).is_some();
    // `f()` dentro de `with`: o `resolve_scope` já devolve o objeto do `with` (`objectAtScope`), então o `this`
    // chega como o próprio objeto. Um `JSScope` aqui (ambiente léxico, global lexical, módulo, ativação de
    // `eval` estrito) é o `JSValue::toThis` do C++: strict dá `undefined`, sloppy o `globalThis`.
    let scope = if value.is_cell() { JSScope::from_cell_id(value.as_cell()) } else { None };
    let result = if scope.is_some() {
        // `JSScope::toThis` (ambiente léxico, global lexical, módulo): strict dá `undefined`, sloppy o `globalThis`.
        if op.ecma_mode.is_strict() {
            JSValue::undefined()
        } else {
            global_this_value(f)
        }
    } else if is_object || op.ecma_mode.is_strict() {
        value
    } else if value.is_undefined_or_null() {
        global_this_value(f)
    } else {
        // `JSValue::toThisSlowCase` sloppy: `toObject` do primitivo (`StringObject`, `NumberObject`,
        // `BooleanObject`, `SymbolObject`, `BigIntObject`).
        let object = value.to_object(&**f.code_block.global_object()).ok_or(LLIntFailure::Thrown)?;
        object.as_value()
    };
    f.set(op.src_dst, result);
    Ok(())
}

/// `op_get_length`: o `length` da string e do `JSArray` direto, o `get` de `length` nos demais objetos.
fn get_length(f: &mut SlowPathFrame, op: &OpGetLength) -> LLIntResult<()> {
    let base = f.get(op.base);
    let vm = f.vm;
    let length = if base.is_string() {
        JSValue::Int32(base.as_js_string().length() as i32)
    } else if let Some(array) = JSArray::from_value(&base) {
        JSValue::from_u32(array.length())
    } else if let Some(function) = base.as_js_function() {
        // `JSFunction::getOwnPropertySlot` materializa `length` sob demanda. Precisa vir antes do
        // ramo de `JSObject`, que não reifica a propriedade preguiçosa.
        let value = crate::runtime::host_function_support::ObjectRef::Function(function)
            .get(&**f.code_block.global_object(), &PropertyName::from_identifier(&vm.property_names.length));
        check_exception(f)?;
        value
    } else if let Some(object) = JSObject::from_value(&base) {
        let value = object.get(vm, &PropertyName::from_identifier(&vm.property_names.length));
        check_exception(f)?;
        value
    } else if base.is_undefined_or_null() {
        // O `.asm` cai no `get_by_id`: `synthesizePrototype` lança `createNotAnObjectError` com o texto-fonte.
        return Err(throw_not_an_object(f, base));
    } else {
        // Número, booleano, `Symbol` e `BigInt`: `get(length)` no protótipo do invólucro, sem criá-lo.
        let ctx = crate::llint::slow_paths_object::Ctx::new(f);
        let name = PropertyName::from_identifier(&vm.property_names.length);
        crate::llint::slow_paths_object::get_primitive_property(&ctx, base, &name)?
    };
    f.set(op.dst, length);
    Ok(())
}

/// `switch_imm`: o escrutínio `int32`, ou o `double` que é inteiro (`d == (int32_t)d`).
fn int32_scrutinee(value: JSValue) -> Option<i32> {
    match value {
        JSValue::Int32(i) => Some(i),
        _ if value.is_number() => {
            let d = value.as_number();
            let i = d as i32;
            (f64::from(i) == d).then_some(i)
        }
        _ => None,
    }
}
