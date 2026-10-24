//! Tradução de `jsPow`, `jsBitwiseNot`, `shift<isLeft>`, `jsLShift`, `jsRShift`, `jsURShift`,
//! `bitwiseBinaryOp`, `jsBitwiseAnd`, `jsBitwiseOr` e `jsBitwiseXor` (`runtime/OperationsInlines.h`):
//! o que os slow paths de `op_pow`, `op_bitnot`, `op_lshift`, `op_rshift`, `op_urshift`, `op_bitand`,
//! `op_bitor` e `op_bitxor` calculam com `1 << 2`, `a & b`, `1n << 2n`.
//!
//! DIVERGÊNCIAS (as mesmas de `operations.rs`, que descreve o motivo):
//!
//! - Sem `BigInt32` (`USE(BIGINT32)` é 0): `toBigIntOrInt32` devolve `int32` ou BigInt de célula
//!   (`js_big_int_ops::to_big_int_or_int32`), e o `bigIntOp` é o `JSBigInt::bitwiseAnd` & cia.
//! - Sem `JSGlobalObject*`: o reino é o de `current_realm`, e a exceção de uma conversão (o `valueOf` de
//!   um objeto que lança) fica pendente no `VM`; o resultado é então o `JSValue` vazio, e o segundo
//!   operando não chega a ser convertido (o `RETURN_IF_EXCEPTION` do C++).

use crate::runtime::current_realm::current_global_object;
use crate::runtime::host_function_support::throw_vm_type_error;
use crate::runtime::js_big_int::JSBigInt;
use crate::runtime::js_big_int_ops::{big_int_binary_op, big_int_unary_op, to_big_int_or_int32, BigIntBinaryOp};
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::math_common::operation_math_pow;
use crate::runtime::operations::arithmetic_binary_op;

/// `jsPow(globalObject, v1, v2)`: o operador `**`.
pub fn js_pow(v1: JSValue, v2: JSValue) -> JSValue {
    arithmetic_binary_op(
        v1,
        v2,
        operation_math_pow,
        JSBigInt::exponentiate,
        "Invalid mix of BigInt and other type in exponentiation.",
    )
}

/// `jsBitwiseNot(globalObject, v)`: o operador `~`.
pub fn js_bitwise_not(v: JSValue) -> JSValue {
    let operand_numeric = to_big_int_or_int32(v);
    if operand_numeric.is_empty() {
        return operand_numeric;
    }

    if operand_numeric.is_int32() {
        return js_number(f64::from(!operand_numeric.as_int32()));
    }

    debug_assert!(operand_numeric.is_big_int());
    big_int_unary_op(operand_numeric, JSBigInt::bitwise_not)
}

/// `toBigIntOrInt32` dos dois operandos, o segundo só se o primeiro não lançou: `None` com a exceção
/// pendente.
fn to_numeric_pair(v1: JSValue, v2: JSValue) -> Option<(JSValue, JSValue)> {
    let left_numeric = to_big_int_or_int32(v1);
    if left_numeric.is_empty() {
        return None;
    }
    let right_numeric = to_big_int_or_int32(v2);
    if right_numeric.is_empty() {
        return None;
    }
    Some((left_numeric, right_numeric))
}

/// O fim de `shift` e `bitwiseBinaryOp`: com dois BigInt, o `bigIntOp`; senão o `TypeError`
/// "Invalid mix of BigInt and other type in ...".
fn big_int_op_or_mix_error(left: JSValue, right: JSValue, big_int_op: BigIntBinaryOp, error_message: &str) -> JSValue {
    if left.is_big_int() && right.is_big_int() {
        return big_int_binary_op(left, right, big_int_op);
    }

    throw_vm_type_error(&current_global_object(), Some(error_message));
    JSValue::empty()
}

/// `shift<isLeft>(globalObject, v1, v2)`: a contagem usa só os cinco bits baixos
/// (`rightInt32 & 31`); `<<` desloca em complemento de dois sem sinal de estouro (`wrapping_shl`,
/// o `leftInt32 << rightInt32` do C++ sobre `int32_t`) e `>>` propaga o sinal. `shift::<true>` é o
/// `jsLShift` (`<<`) e `shift::<false>` o `jsRShift` (`>>`).
pub fn shift<const IS_LEFT: bool>(v1: JSValue, v2: JSValue) -> JSValue {
    let Some((left_numeric, right_numeric)) = to_numeric_pair(v1, v2) else {
        return JSValue::empty();
    };

    if left_numeric.is_int32() && right_numeric.is_int32() {
        let left_int32 = left_numeric.as_int32();
        let right_int32 = (right_numeric.as_int32() & 31) as u32;
        let result = if IS_LEFT { left_int32.wrapping_shl(right_int32) } else { left_int32 >> right_int32 };
        return js_number(f64::from(result));
    }

    if IS_LEFT {
        big_int_op_or_mix_error(
            left_numeric,
            right_numeric,
            JSBigInt::left_shift_big_int,
            "Invalid mix of BigInt and other type in left shift operation.",
        )
    } else {
        big_int_op_or_mix_error(
            left_numeric,
            right_numeric,
            JSBigInt::signed_right_shift,
            "Invalid mix of BigInt and other type in signed right shift operation.",
        )
    }
}

/// `jsURShift(globalObject, left, right)`: o operador `>>>`. O C++ converte o resultado de
/// `uint32_t` para `int32_t` antes do `jsNumber`, e o `jsNumber(int32_t)` volta a ser o mesmo valor
/// quando é lido como número; aqui o `u32` entra direto como `f64`, que é o valor observável.
pub fn js_urshift(left: JSValue, right: JSValue) -> JSValue {
    let Some((left_numeric, right_numeric)) = to_numeric_pair(left, right) else {
        return JSValue::empty();
    };

    // `toUInt32AfterToNumeric`: `nullopt` quando o operando é BigInt.
    if !left_numeric.is_int32() || !right_numeric.is_int32() {
        throw_vm_type_error(&current_global_object(), Some("BigInt does not support >>> operator"));
        return JSValue::empty();
    }
    let (left, right) = (left_numeric.as_int32() as u32, right_numeric.as_int32() as u32);
    js_number(f64::from(left >> (right & 31)))
}

/// `bitwiseBinaryOp(globalObject, v1, v2, int32Op, bigIntOp, errorMessage)`.
fn bitwise_binary_op(
    v1: JSValue,
    v2: JSValue,
    int32_op: impl Fn(i32, i32) -> i32,
    big_int_op: BigIntBinaryOp,
    error_message: &str,
) -> JSValue {
    let Some((left_numeric, right_numeric)) = to_numeric_pair(v1, v2) else {
        return JSValue::empty();
    };

    if left_numeric.is_int32() && right_numeric.is_int32() {
        return js_number(f64::from(int32_op(left_numeric.as_int32(), right_numeric.as_int32())));
    }

    big_int_op_or_mix_error(left_numeric, right_numeric, big_int_op, error_message)
}

/// `jsBitwiseAnd(globalObject, v1, v2)`: o operador `&`.
pub fn js_bitwise_and(v1: JSValue, v2: JSValue) -> JSValue {
    bitwise_binary_op(
        v1,
        v2,
        |left, right| left & right,
        JSBigInt::bitwise_and,
        "Invalid mix of BigInt and other type in bitwise 'and' operation.",
    )
}

/// `jsBitwiseOr(globalObject, v1, v2)`: o operador `|`.
pub fn js_bitwise_or(v1: JSValue, v2: JSValue) -> JSValue {
    bitwise_binary_op(
        v1,
        v2,
        |left, right| left | right,
        JSBigInt::bitwise_or,
        "Invalid mix of BigInt and other type in bitwise 'or' operation.",
    )
}

/// `jsBitwiseXor(globalObject, v1, v2)`: o operador `^`.
pub fn js_bitwise_xor(v1: JSValue, v2: JSValue) -> JSValue {
    bitwise_binary_op(
        v1,
        v2,
        |left, right| left ^ right,
        JSBigInt::bitwise_xor,
        "Invalid mix of BigInt and other type in bitwise 'xor' operation.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(v: f64) -> JSValue {
        js_number(v)
    }

    #[test]
    fn shifts_use_low_five_bits_and_wrap() {
        assert_eq!(shift::<true>(num(1.0), num(2.0)).as_number(), 4.0);
        assert_eq!(shift::<true>(num(1.0), num(33.0)).as_number(), 2.0);
        assert_eq!(shift::<true>(num(1.0), num(31.0)).as_number(), -2147483648.0);
        assert_eq!(shift::<false>(num(-8.0), num(1.0)).as_number(), -4.0);
        assert_eq!(js_urshift(num(-1.0), num(0.0)).as_number(), 4294967295.0);
        assert_eq!(js_urshift(num(-1.0), num(28.0)).as_number(), 15.0);
    }

    #[test]
    fn bitwise_operators_convert_through_int32() {
        assert_eq!(js_bitwise_and(num(12.0), num(10.0)).as_number(), 8.0);
        assert_eq!(js_bitwise_or(num(12.0), num(10.0)).as_number(), 14.0);
        assert_eq!(js_bitwise_xor(num(12.0), num(10.0)).as_number(), 6.0);
        assert_eq!(js_bitwise_not(num(0.0)).as_number(), -1.0);
        assert_eq!(js_bitwise_or(num(4294967296.0 + 5.0), num(0.0)).as_number(), 5.0);
        assert_eq!(js_bitwise_or(num(f64::NAN), num(0.0)).as_number(), 0.0);
    }

    #[test]
    fn pow_follows_operation_math_pow() {
        assert_eq!(js_pow(num(2.0), num(10.0)).as_number(), 1024.0);
        assert!(js_pow(num(1.0), num(f64::INFINITY)).as_number().is_nan());
    }
}
