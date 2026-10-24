//! Slow paths aritméticos, bit a bit, de comparação e de conversão de `runtime/CommonSlowPaths.cpp`
//! (`slow_path_add`, `slow_path_sub`, `slow_path_mul`, `slow_path_div`, `slow_path_mod`,
//! `slow_path_pow`, `slow_path_lshift`, `slow_path_rshift`, `slow_path_urshift`, `slow_path_bitand`,
//! `slow_path_bitor`, `slow_path_bitxor`, `slow_path_bitnot`, `slow_path_unsigned`, `slow_path_inc`,
//! `slow_path_dec`, `slow_path_negate`, `slow_path_to_number`, `slow_path_to_numeric`,
//! `slow_path_to_string`, `slow_path_eq`, `neq`, `stricteq`, `nstricteq`, `slow_path_typeof`,
//! `typeof_is_object`, `typeof_is_function`, `is_callable`) e de `llint/LLIntSlowPaths.cpp`
//! (`slow_path_less`, `lesseq`, `greater`, `greatereq`).
//!
//! DIVERGÊNCIAS:
//!
//! - Cada função recebe o [`SlowPathFrame`] (VM, `CallFrame`, `CLoopStack` e `CodeBlock`) e o `Op*`
//!   já decodificado, no lugar de `(CallFrame*, const JSInstruction* pc)`. `getOperand(callFrame,
//!   reg)` (`GET_C`) é [`SlowPathFrame::get`], que lê a constante do `CodeBlock` quando o registrador
//!   é constante; `RETURN` é [`SlowPathFrame::set`] sobre o `dst`.
//! - Sem `BinaryArithProfile`/`UnaryArithProfile` (`observeLHSAndRHS`, `RETURN_WITH_PROFILING`,
//!   `updateArithProfileForBinaryArithOp`): o profiling só alimenta o JIT, que o porte não tem. O
//!   `profile_index` dos ops é ignorado.
//! - Exceção: `Err(OutOfMemory)` é o `throwOutOfMemoryError` do `+` de strings com comprimento acima de
//!   `i32::MAX` (`js_add`). Uma exceção de `toPrimitive`, `toNumber` e afins (o `valueOf` de um objeto que
//!   lança, `Symbol` em `+`) fica pendente no `VM`: o slow path não escreve o `dst` (o `CHECK_EXCEPTION`
//!   do C++) e quem despacha confere `vm.exception()` (`plain_slow_path!` em `dispatch_ext`).
//! - Fora, por depender de `JSBigInt` ligado ao `JSValue`: a aritmética, `unaryMinus`, `inc`/`dec` e a
//!   comparação de BigInt (panicam com a lacuna dita); os `is_object`/`is_constructor`, que dependem do
//!   `Structure` e do `ExecutableBase` do `JSFunction`. `typeof_is_undefined`, `is_callable`,
//!   `typeof_is_object` e `typeof_is_function` seguem `js_typeof`.
//! - `slow_path_negate` faz `toPrimitive(PreferNumber)` e depois `-toNumber`, ou `JSBigInt::unaryMinus`.

use crate::bytecode::bytecode_ops::{
    OpAdd, OpBitand, OpBitnot, OpBitor, OpBitxor, OpDec, OpDiv, OpEq, OpGreater, OpGreatereq, OpInc, OpIsCallable,
    OpLess, OpLesseq, OpLshift, OpMod, OpMul, OpNeq, OpNegate, OpNstricteq, OpPow, OpRshift, OpStricteq, OpSub, OpToNumber,
    OpToNumeric, OpToString, OpTypeof, OpTypeofIsFunction, OpTypeofIsObject, OpUnsigned, OpUrshift,
};
use crate::bytecode::code_block::CodeBlock;
use crate::bytecode::virtual_register::VirtualRegister;
use crate::interpreter::call_frame::CallFrame;
use crate::interpreter::cloop_stack::CLoopStack;
use crate::interpreter::register::Register;
use crate::runtime::current_realm::has_pending_exception;
use crate::runtime::js_big_int::JSBigInt;
use crate::runtime::js_big_int_ops::big_int_unary_op;
use crate::runtime::js_string::js_string;
use crate::runtime::js_typeof::{js_type_string_for_value, js_typeof_is_function, js_typeof_is_object};
use crate::runtime::js_value::{js_boolean, js_number, JSValue};
use crate::runtime::object_to_primitive::PreferredPrimitiveType;
use crate::runtime::operations::{
    js_add, js_dec, js_div, js_inc, js_less, js_less_eq, js_mul, js_remainder, js_sub, loose_equal, strict_equal,
};
use crate::runtime::operations_bitwise::{
    js_bitwise_and, js_bitwise_not, js_bitwise_or, js_bitwise_xor, js_pow, js_urshift, shift,
};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `throwOutOfMemoryError(globalObject, scope)`: a única exceção destes slow paths (ver o cabeçalho).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutOfMemory;

/// O que um slow path devolve: `Ok(())` com o `dst` já escrito, ou a exceção pendente.
pub type SlowPathResult = Result<(), OutOfMemory>;

/// O contexto dos macros `BEGIN()`/`GET_C`/`RETURN` de `CommonSlowPaths.cpp`.
pub struct SlowPathFrame<'a> {
    pub vm: &'a VM,
    pub call_frame: CallFrame,
    pub stack: &'a CLoopStack,
    pub code_block: &'a CodeBlock,
}

impl SlowPathFrame<'_> {
    /// `getOperand(callFrame, reg)` / `GET_C(reg).jsValue()`.
    pub fn get(&self, reg: VirtualRegister) -> JSValue {
        if reg.is_constant() {
            self.code_block.get_constant(reg)
        } else {
            self.call_frame.unchecked_r(self.stack, reg).js_value()
        }
    }

    /// Onde o erro nasce: o `CodeBlock` e o `BytecodeIndex` da instrução em execução (o `currentVPC` que o
    /// laço grava antes de cada instrução), de onde o `appendSourceToErrorMessage` tira o texto-fonte.
    pub fn error_site(&self) -> crate::runtime::exception_helpers::BlockErrorSite<'_> {
        crate::runtime::exception_helpers::BlockErrorSite {
            code_block: self.code_block,
            bytecode_index: self.call_frame.bytecode_index(self.stack),
            visible_caller: crate::runtime::exception_helpers::visible_caller_site(self.vm, self.call_frame),
        }
    }

    /// `RETURN(value)`: `callFrame->uncheckedR(dst) = value`.
    pub fn set(&mut self, dst: VirtualRegister, value: JSValue) {
        self.call_frame.set_unchecked_r(self.stack, dst, Register::from(value));
    }

    /// O molde de todo slow path de dois operandos: lê `lhs` e `rhs`, escreve `dst`. O resultado vazio
    /// é a exceção pendente no `VM` (o `CHECK_EXCEPTION` do C++): o `dst` não é escrito.
    fn binary(&mut self, dst: VirtualRegister, lhs: VirtualRegister, rhs: VirtualRegister, op: impl Fn(JSValue, JSValue) -> JSValue) {
        let value = op(self.get(lhs), self.get(rhs));
        if !value.is_empty() {
            self.set(dst, value);
        }
    }

    /// O molde de todo slow path de um operando: lê `operand`, escreve `dst` (não escreve o resultado
    /// vazio, a exceção pendente).
    fn unary(&mut self, dst: VirtualRegister, operand: VirtualRegister, op: impl Fn(JSValue) -> JSValue) {
        let value = op(self.get(operand));
        if !value.is_empty() {
            self.set(dst, value);
        }
    }

    /// `jsString` de um texto fixo (`jsTypeStringForValue`).
    fn type_string(&self, value: JSValue) -> JSValue {
        let text = WtfString::from_latin1(js_type_string_for_value(value).as_bytes());
        JSValue::from_js_string(js_string(self.vm, &text))
    }
}

// ---------------------------------------------------------------------------------------------
// Aritmética
// ---------------------------------------------------------------------------------------------

/// `slow_path_add`: `jsAdd`. `Err(OutOfMemory)` é só o estouro de comprimento; uma exceção das
/// conversões fica pendente no `VM` (`Ok` sem escrever o `dst`).
pub fn slow_path_add(f: &mut SlowPathFrame, op: &OpAdd) -> SlowPathResult {
    let result = js_add(f.vm, f.get(op.lhs), f.get(op.rhs)).ok_or(OutOfMemory)?;
    if !result.is_empty() {
        f.set(op.dst, result);
    }
    Ok(())
}

/// `slow_path_sub`: `jsSub`.
pub fn slow_path_sub(f: &mut SlowPathFrame, op: &OpSub) {
    f.binary(op.dst, op.lhs, op.rhs, js_sub);
}

/// `slow_path_mul`: `jsMul`.
pub fn slow_path_mul(f: &mut SlowPathFrame, op: &OpMul) {
    f.binary(op.dst, op.lhs, op.rhs, js_mul);
}

/// `slow_path_div`: `jsDiv`.
pub fn slow_path_div(f: &mut SlowPathFrame, op: &OpDiv) {
    f.binary(op.dst, op.lhs, op.rhs, js_div);
}

/// `slow_path_mod`: `jsRemainder`.
pub fn slow_path_mod(f: &mut SlowPathFrame, op: &OpMod) {
    f.binary(op.dst, op.lhs, op.rhs, js_remainder);
}

/// `slow_path_pow`: `jsPow`.
pub fn slow_path_pow(f: &mut SlowPathFrame, op: &OpPow) {
    f.binary(op.dst, op.lhs, op.rhs, js_pow);
}

/// `slow_path_inc`: `jsInc` sobre `srcDst`, escrito no próprio registrador.
pub fn slow_path_inc(f: &mut SlowPathFrame, op: &OpInc) {
    f.unary(op.src_dst, op.src_dst, js_inc);
}

/// `slow_path_dec`: `jsDec` sobre `srcDst`.
pub fn slow_path_dec(f: &mut SlowPathFrame, op: &OpDec) {
    f.unary(op.src_dst, op.src_dst, js_dec);
}

/// `slow_path_negate`: `toPrimitive(PreferNumber)` e depois `jsNumber(-toNumber)`, ou
/// `JSBigInt::unaryMinus` se o primitivo é BigInt.
pub fn slow_path_negate(f: &mut SlowPathFrame, op: &OpNegate) {
    f.unary(op.dst, op.operand, |value| {
        let primitive = value.to_primitive_preferred(PreferredPrimitiveType::PreferNumber);
        if primitive.is_empty() {
            return primitive;
        }
        if primitive.is_big_int() {
            return big_int_unary_op(primitive, JSBigInt::unary_minus);
        }
        let number = primitive.to_number();
        if has_pending_exception() {
            return JSValue::empty();
        }
        js_number(-number)
    });
}

// ---------------------------------------------------------------------------------------------
// Bit a bit e deslocamentos
// ---------------------------------------------------------------------------------------------

/// `slow_path_lshift`: `jsLShift`.
pub fn slow_path_lshift(f: &mut SlowPathFrame, op: &OpLshift) {
    f.binary(op.dst, op.lhs, op.rhs, shift::<true>);
}

/// `slow_path_rshift`: `jsRShift`.
pub fn slow_path_rshift(f: &mut SlowPathFrame, op: &OpRshift) {
    f.binary(op.dst, op.lhs, op.rhs, shift::<false>);
}

/// `slow_path_urshift`: `jsURShift`.
pub fn slow_path_urshift(f: &mut SlowPathFrame, op: &OpUrshift) {
    f.binary(op.dst, op.lhs, op.rhs, js_urshift);
}

/// `slow_path_unsigned`: `jsNumber(toUInt32)`.
pub fn slow_path_unsigned(f: &mut SlowPathFrame, op: &OpUnsigned) {
    f.unary(op.dst, op.operand, |value| js_number(f64::from(value.to_uint32())));
}

/// `slow_path_bitnot`: `jsBitwiseNot`.
pub fn slow_path_bitnot(f: &mut SlowPathFrame, op: &OpBitnot) {
    f.unary(op.dst, op.operand, js_bitwise_not);
}

/// `slow_path_bitand`: `jsBitwiseAnd`.
pub fn slow_path_bitand(f: &mut SlowPathFrame, op: &OpBitand) {
    f.binary(op.dst, op.lhs, op.rhs, js_bitwise_and);
}

/// `slow_path_bitor`: `jsBitwiseOr`.
pub fn slow_path_bitor(f: &mut SlowPathFrame, op: &OpBitor) {
    f.binary(op.dst, op.lhs, op.rhs, js_bitwise_or);
}

/// `slow_path_bitxor`: `jsBitwiseXor`.
pub fn slow_path_bitxor(f: &mut SlowPathFrame, op: &OpBitxor) {
    f.binary(op.dst, op.lhs, op.rhs, js_bitwise_xor);
}

// ---------------------------------------------------------------------------------------------
// Comparação
// ---------------------------------------------------------------------------------------------

/// `slow_path_less` (`LLIntSlowPaths.cpp`): `jsLess<true>(lhs, rhs)`.
pub fn slow_path_less(f: &mut SlowPathFrame, op: &OpLess) {
    f.binary(op.dst, op.lhs, op.rhs, |a, b| js_boolean(js_less::<true>(a, b)));
}

/// `slow_path_lesseq`: `jsLessEq<true>(lhs, rhs)`.
pub fn slow_path_lesseq(f: &mut SlowPathFrame, op: &OpLesseq) {
    f.binary(op.dst, op.lhs, op.rhs, |a, b| js_boolean(js_less_eq::<true>(a, b)));
}

/// `slow_path_greater`: `jsLess<false>(rhs, lhs)`.
pub fn slow_path_greater(f: &mut SlowPathFrame, op: &OpGreater) {
    f.binary(op.dst, op.rhs, op.lhs, |a, b| js_boolean(js_less::<false>(a, b)));
}

/// `slow_path_greatereq`: `jsLessEq<false>(rhs, lhs)`.
pub fn slow_path_greatereq(f: &mut SlowPathFrame, op: &OpGreatereq) {
    f.binary(op.dst, op.rhs, op.lhs, |a, b| js_boolean(js_less_eq::<false>(a, b)));
}

/// `slow_path_eq`: `JSValue::equal`.
pub fn slow_path_eq(f: &mut SlowPathFrame, op: &OpEq) {
    f.binary(op.dst, op.lhs, op.rhs, |a, b| js_boolean(loose_equal(a, b)));
}

/// `slow_path_neq`: `!JSValue::equal`.
pub fn slow_path_neq(f: &mut SlowPathFrame, op: &OpNeq) {
    f.binary(op.dst, op.lhs, op.rhs, |a, b| js_boolean(!loose_equal(a, b)));
}

/// `slow_path_stricteq`: `JSValue::strictEqual`.
pub fn slow_path_stricteq(f: &mut SlowPathFrame, op: &OpStricteq) {
    f.binary(op.dst, op.lhs, op.rhs, |a, b| js_boolean(strict_equal(a, b)));
}

/// `slow_path_nstricteq`: `!JSValue::strictEqual`.
pub fn slow_path_nstricteq(f: &mut SlowPathFrame, op: &OpNstricteq) {
    f.binary(op.dst, op.lhs, op.rhs, |a, b| js_boolean(!strict_equal(a, b)));
}

// ---------------------------------------------------------------------------------------------
// typeof, is_* e conversões
// ---------------------------------------------------------------------------------------------

/// `slow_path_typeof`: `jsTypeStringForValue`.
pub fn slow_path_typeof(f: &mut SlowPathFrame, op: &OpTypeof) {
    let value = f.get(op.value);
    let result = f.type_string(value);
    f.set(op.dst, result);
}

/// `slow_path_typeof_is_object`: `jsTypeofIsObject`.
pub fn slow_path_typeof_is_object(f: &mut SlowPathFrame, op: &OpTypeofIsObject) {
    f.unary(op.dst, op.operand, |value| js_boolean(js_typeof_is_object(value)));
}

/// `slow_path_typeof_is_function`: `jsTypeofIsFunction`.
pub fn slow_path_typeof_is_function(f: &mut SlowPathFrame, op: &OpTypeofIsFunction) {
    f.unary(op.dst, op.operand, |value| js_boolean(js_typeof_is_function(value)));
}

/// `slow_path_is_callable`: `JSValue::isCallable`, que é o `typeof` "function" do porte.
pub fn slow_path_is_callable(f: &mut SlowPathFrame, op: &OpIsCallable) {
    f.unary(op.dst, op.operand, |value| js_boolean(js_typeof_is_function(value)));
}

/// `slow_path_to_number`: `jsNumber(toNumber)`.
pub fn slow_path_to_number(f: &mut SlowPathFrame, op: &OpToNumber) {
    f.unary(op.dst, op.operand, |value| {
        let number = value.to_number();
        if has_pending_exception() {
            return JSValue::empty();
        }
        js_number(number)
    });
}

/// `slow_path_to_numeric`: `toNumeric`.
pub fn slow_path_to_numeric(f: &mut SlowPathFrame, op: &OpToNumeric) {
    f.unary(op.dst, op.operand, |value| value.to_numeric());
}

/// `slow_path_to_string`: `toString`.
pub fn slow_path_to_string(f: &mut SlowPathFrame, op: &OpToString) {
    let vm = f.vm;
    f.unary(op.dst, op.operand, |value| {
        let string = value.to_string(vm);
        if vm.exception().is_some() {
            return JSValue::empty();
        }
        JSValue::from_js_string(string)
    });
}
