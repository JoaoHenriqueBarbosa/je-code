//! A ponte entre `JSBigInt` (a aritmética sobre dígitos de `js_big_int*.rs`) e o `JSValue`: o que o
//! C++ faz com `JSBigInt*` embrulhado em `JSValue` em `JSBigInt.h`/`JSBigIntInlines.h`,
//! `JSCJSValue.cpp` (`toBigInt`), `JSCJSValueInlines.h` (`toBigIntOrInt32`) e `OperationsInlines.h`
//! (`compareBigInt`, `compareBigIntToOtherPrimitive`, `bigIntCompare`, o `bigIntOp` de
//! `arithmeticBinaryOp`, `jsInc`, `jsDec`).
//!
//! DIVERGÊNCIAS:
//!
//! - Sem `BigInt32` (`USE(BIGINT32)` é 0): todo BigInt é célula (`CellEntry::BigInt`), então as
//!   sobrecargas com `int32_t` não existem e `isHeapBigInt` é `isBigInt`.
//! - O erro de uma operação sai como o `BigIntError` da camada de dígitos e vira exceção pendente no
//!   `VM` do reino corrente (`current_realm`), como nas demais conversões do porte; o resultado é então
//!   o `JSValue` vazio (o `JSValue()` do C++).
//! - `JSGlobalObject` implementa [`BigIntGlobalObject`] aqui, o que `JSBigInt::toString` e `parseInt`
//!   pedem do global.

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::current_realm::{current_global_object, has_pending_exception};
use crate::runtime::error::{create_range_error, create_syntax_error};
use crate::runtime::exception_helpers::{throw_out_of_memory_error, throw_out_of_memory_error_with_message};
use crate::runtime::host_function_support::throw_vm_type_error;
use crate::runtime::js_big_int::{
    compare, compare_to_double, flip, BigIntError, BigIntGlobalObject, ComparisonMode, ComparisonResult, ErrorParseMode,
    HeapBigIntImpl, ImplResult, JSBigInt, JSBigIntRef,
};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::{js_number_i32, JSValue};
use crate::runtime::math_common::is_integer;
use crate::runtime::object_to_primitive::PreferredPrimitiveType;
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::runtime::vm::VM;
use crate::wtf::text::string_view::StringView;
use crate::wtf::text::wtf_string::String as WtfString;

/// O `bigIntOp` de `arithmeticBinaryOp` e `bitwiseBinaryOp`: `JSBigInt::add`, `sub`, `multiply`...
pub type BigIntBinaryOp = fn(&JSBigInt, &JSBigInt) -> Result<ImplResult, BigIntError>;

/// O `JSBigInt::inc`, `dec` e `bitwiseNot`.
pub type BigIntUnaryOp = fn(&JSBigInt) -> Result<ImplResult, BigIntError>;

/// `maxSafeInteger()` como `uint64_t` (`maxSafeIntegerAsUInt64`): 2^53 - 1.
const MAX_SAFE_INTEGER_AS_U64: u64 = (1 << 53) - 1;

/// `throwOutOfMemoryError` e `throwRangeError` de `JSBigInt.cpp`, com a mensagem exata do C++.
pub fn throw_big_int_error(global_object: &JSGlobalObject, error: &BigIntError) {
    let mut scope = ThrowScope::new(global_object.vm());
    let message = WtfString::from_latin1(error.message().as_bytes());
    match error {
        BigIntError::OutOfMemory => {
            throw_out_of_memory_error(global_object, &mut scope);
        }
        BigIntError::TooBig => {
            throw_out_of_memory_error_with_message(global_object, &mut scope, &message);
        }
        BigIntError::NegativeExponent | BigIntError::InvalidDivisor => {
            throw_exception(global_object, &mut scope, create_range_error(global_object, &message));
        }
    }
}

impl BigIntGlobalObject for JSGlobalObject {
    fn vm(&self) -> &VM {
        JSGlobalObject::vm(self)
    }

    fn throw_syntax_error(&self, _vm: &VM, message: &str) {
        let mut scope = ThrowScope::new(JSGlobalObject::vm(self));
        let error = create_syntax_error(self, &WtfString::from_latin1(message.as_bytes()));
        throw_exception(self, &mut scope, error);
    }

    fn throw_out_of_memory_error(&self, _vm: &VM, message: Option<&str>) {
        let error = match message {
            Some(_) => BigIntError::TooBig,
            None => BigIntError::OutOfMemory,
        };
        throw_big_int_error(self, &error);
    }
}

/// `value.asHeapBigInt()`: o `JSBigInt` da célula, ou `None` se `value` não é BigInt.
pub fn big_int_of(value: JSValue) -> Option<JSBigIntRef> {
    if !value.is_cell() {
        return None;
    }
    match cell_registry::get(value.as_cell()) {
        Some(CellEntry::BigInt(big_int)) => Some(big_int),
        _ => None,
    }
}

/// O `JSValue` de um resultado de operação: o BigInt como célula, ou o `JSValue` vazio com a exceção
/// pendente no `VM` do reino corrente.
pub fn impl_result_value(result: Result<ImplResult, BigIntError>) -> JSValue {
    match result {
        Ok(result) => result.into_js_value(current_global_object().vm()),
        Err(error) => {
            throw_big_int_error(&current_global_object(), &error);
            JSValue::empty()
        }
    }
}

/// O `bigIntOp(globalObject, left, right)` de `arithmeticBinaryOp`, `bitwiseBinaryOp` e `shift`:
/// `left` e `right` são `JSBigInt`.
pub fn big_int_binary_op(left: JSValue, right: JSValue, op: BigIntBinaryOp) -> JSValue {
    let (Some(left), Some(right)) = (big_int_of(left), big_int_of(right)) else {
        unreachable!("big_int_binary_op sem dois BigInt");
    };
    impl_result_value(op(&left, &right))
}

/// `JSBigInt::inc(globalObject, JSBigInt*)`, `dec` e `bitwiseNot` sobre o valor `operand`, que é
/// `JSBigInt`.
pub fn big_int_unary_op(operand: JSValue, op: BigIntUnaryOp) -> JSValue {
    let Some(operand) = big_int_of(operand) else {
        unreachable!("big_int_unary_op sem BigInt");
    };
    impl_result_value(op(&operand))
}

/// `JSBigInt::makeHeapBigIntOrBigInt32(globalObject, int64_t)` e `(…, uint64_t)`, sem o `BigInt32`.
pub fn make_big_int_from_i64(value: i64) -> JSValue {
    impl_result_value(JSBigInt::create_from_i64(value).map(ImplResult::Heap))
}

/// `JSBigInt::makeHeapBigIntOrBigInt32(globalObject, double)`: `value` é um inteiro.
pub fn make_big_int_from_double(value: f64) -> JSValue {
    debug_assert!(is_integer(value));
    if value.abs() <= MAX_SAFE_INTEGER_AS_U64 as f64 {
        return make_big_int_from_i64(value as i64);
    }
    impl_result_value(JSBigInt::create_from_f64(value).map(ImplResult::Heap))
}

/// `JSBigInt::tryExtractDouble(JSValue)`: o número exato que o valor representa, se houver.
pub fn try_extract_double(value: JSValue) -> Option<f64> {
    if value.is_number() {
        return Some(value.as_number());
    }

    let big_int = big_int_of(value)?;
    if big_int.length() == 0 {
        return Some(0.0);
    }

    if big_int.length() != 1 {
        return None;
    }
    let integer = big_int.digit(0);

    if integer <= MAX_SAFE_INTEGER_AS_U64 {
        return Some(if big_int.sign() { -(integer as f64) } else { integer as f64 });
    }

    None
}

/// `JSBigInt::toBigUInt64(JSValue)`.
pub fn to_big_uint64_value(value: JSValue) -> u64 {
    let big_int = big_int_of(value).expect("toBigUInt64 sem BigInt");
    JSBigInt::to_big_uint64_heap(&big_int)
}

/// `JSBigInt::toBigInt64(JSValue)`.
pub fn to_big_int64_value(value: JSValue) -> i64 {
    to_big_uint64_value(value) as i64
}

/// `JSBigInt::compare(JSValue, JSValue)` entre dois BigInt (`compareBigInt` de `OperationsInlines.h`).
pub fn compare_big_int(left: JSValue, right: JSValue) -> ComparisonResult {
    let (Some(left), Some(right)) = (big_int_of(left), big_int_of(right)) else {
        unreachable!("compare_big_int sem dois BigInt");
    };
    compare(&left, &right)
}

/// `compareBigIntToOtherPrimitive(globalObject, v1, primValue)`: `Undefined` com a exceção pendente
/// ou quando a string não é um BigInt.
pub fn compare_big_int_to_other_primitive(v1: &JSBigInt, prim_value: JSValue) -> ComparisonResult {
    debug_assert!(!prim_value.is_big_int());

    if prim_value.is_string() {
        let text = prim_value.as_js_string().value();
        let global_object = current_global_object();
        let big_int_value = JSBigInt::string_to_big_int(&*global_object, StringView::from(&text));
        match big_int_value {
            ImplResult::Empty => return ComparisonResult::Undefined,
            ImplResult::Heap(big_int) => return compare(v1, &big_int),
            ImplResult::BigInt32(value) => return compare_to_double(&HeapBigIntImpl::new(v1), f64::from(value)),
        }
    }

    let number_value = prim_value.to_number();
    if has_pending_exception() {
        return ComparisonResult::Undefined;
    }
    compare_to_double(&HeapBigIntImpl::new(v1), number_value)
}

/// `bigIntCompareResult(comparisonResult, comparisonMode)`.
pub fn big_int_compare_result(comparison_result: ComparisonResult, comparison_mode: ComparisonMode) -> bool {
    if comparison_mode == ComparisonMode::LessThan {
        return comparison_result == ComparisonResult::LessThan;
    }

    debug_assert!(comparison_mode == ComparisonMode::LessThanOrEqual);
    comparison_result == ComparisonResult::LessThan || comparison_result == ComparisonResult::Equal
}

/// `bigIntCompare(globalObject, v1, v2, comparisonMode)`: `v1` ou `v2` é BigInt, os dois primitivos.
/// `false` com a exceção pendente.
pub fn big_int_compare(v1: JSValue, v2: JSValue, comparison_mode: ComparisonMode) -> bool {
    debug_assert!(v1.is_big_int() || v2.is_big_int());

    if v1.is_big_int() && v2.is_big_int() {
        return big_int_compare_result(compare_big_int(v1, v2), comparison_mode);
    }

    if let Some(big_int) = big_int_of(v1) {
        debug_assert!(!v2.is_big_int());
        let comparison_result = compare_big_int_to_other_primitive(&big_int, v2);
        if has_pending_exception() {
            return false;
        }
        return big_int_compare_result(comparison_result, comparison_mode);
    }

    // Here we check inverted because BigInt is the v2
    debug_assert!(!v1.is_big_int());
    let big_int = big_int_of(v2).expect("bigIntCompare sem BigInt");
    let comparison_result = compare_big_int_to_other_primitive(&big_int, v1);
    if has_pending_exception() {
        return false;
    }
    big_int_compare_result(flip(comparison_result), comparison_mode)
}

/// `JSValue::equalSlowCaseInline`, o ramo de `v1.isBigInt()` com `v2` string: `stringToBigInt(v2)` e,
/// se deu BigInt, a comparação de BigInt com BigInt. `v1` é o BigInt.
pub fn big_int_equals_string(v1: JSValue, v2: JSValue) -> bool {
    let big_int = big_int_of(v1).expect("== entre BigInt e string sem BigInt");
    let text = v2.as_js_string().value();
    let global_object = current_global_object();
    match JSBigInt::string_to_big_int(&*global_object, StringView::from(&text)) {
        ImplResult::Empty => false,
        ImplResult::Heap(other) => JSBigInt::equals(&big_int, &other),
        ImplResult::BigInt32(other) => big_int.equals_to_int32(other),
    }
}

/// `JSValue::equalSlowCaseInline`, o último ramo: `v1` é BigInt e `v2` BigInt ou número.
pub fn big_int_equals_big_int_or_number(v1: JSValue, v2: JSValue) -> bool {
    let big_int = big_int_of(v1).expect("== com BigInt sem BigInt");
    if let Some(other) = big_int_of(v2) {
        return JSBigInt::equals(&big_int, &other);
    }
    if v2.is_number() {
        return big_int.equals_to_number(v2);
    }
    false
}

/// `JSValue::strictEqualForCells` entre dois BigInt (`JSBigInt::equals`).
pub fn big_int_strict_equals(v1: JSValue, v2: JSValue) -> bool {
    match (big_int_of(v1), big_int_of(v2)) {
        (Some(left), Some(right)) => JSBigInt::equals(&left, &right),
        _ => false,
    }
}

/// `JSValue::toBigInt(globalObject)` (https://tc39.es/ecma262/#sec-tobigint): o BigInt, ou o `JSValue`
/// vazio com a exceção pendente.
pub fn to_big_int(value: JSValue) -> JSValue {
    let primitive = value.to_primitive_preferred(PreferredPrimitiveType::PreferNumber);
    if primitive.is_empty() {
        return primitive;
    }

    if primitive.is_big_int() {
        return primitive;
    }

    if primitive.is_boolean() {
        return impl_result_value(JSBigInt::create_from_bool(primitive.as_boolean()).map(ImplResult::Heap));
    }

    if primitive.is_string() {
        let text = primitive.as_js_string().value();
        let global_object = current_global_object();
        return JSBigInt::parse_int_string_view(&*global_object, StringView::from(&text), ErrorParseMode::ThrowExceptions)
            .into_js_value(global_object.vm());
    }

    debug_assert!(primitive.is_undefined_or_null() || primitive.is_number() || primitive.is_symbol());
    throw_vm_type_error(&current_global_object(), Some("Invalid argument type in ToBigInt operation"));
    JSValue::empty()
}

/// `tryConvertToStrictInt32(double)`: o `int32_t` que o `double` representa exatamente (e que não é -0).
fn try_convert_to_strict_int32(value: f64) -> Option<i32> {
    let integer = value as i32;
    if f64::from(integer) == value && !(integer == 0 && value.is_sign_negative()) {
        return Some(integer);
    }
    None
}

/// `JSValue::toBigIntOrInt32(globalObject)`: o `int32` ou o BigInt a que o operando de uma operação
/// bit a bit se converte; o `JSValue` vazio com a exceção pendente se a conversão lança.
pub fn to_big_int_or_int32(value: JSValue) -> JSValue {
    if value.is_int32() || value.is_big_int() {
        return value;
    }

    if value.is_double() {
        if let Some(int32_value) = try_convert_to_strict_int32(value.as_double()) {
            return js_number_i32(int32_value);
        }
    }

    let prim_value = value.to_primitive_preferred(PreferredPrimitiveType::PreferNumber);
    if prim_value.is_empty() {
        return prim_value;
    }

    if prim_value.is_int32() || prim_value.is_big_int() {
        return prim_value;
    }

    let int32_value = prim_value.to_int32();
    if has_pending_exception() {
        return JSValue::empty();
    }
    js_number_i32(int32_value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn big(value: i128) -> JSBigInt {
        JSBigInt::create_from_i128(value).unwrap()
    }

    fn heap(result: Result<ImplResult, BigIntError>) -> JSBigInt {
        match result.unwrap() {
            ImplResult::Heap(big_int) => big_int,
            ImplResult::BigInt32(value) => JSBigInt::create_from_i32(value).unwrap(),
            ImplResult::Empty => panic!("resultado vazio"),
        }
    }

    fn text(vm: &VM, big_int: &JSBigInt) -> String {
        String::from_utf8(JSBigInt::try_get_string(vm, big_int, 10).latin1()).unwrap()
    }

    fn eval(vm: &VM, result: Result<ImplResult, BigIntError>) -> String {
        text(vm, &heap(result))
    }

    #[test]
    fn add_and_sub_follow_signs() {
        let vm = VM::new();
        assert_eq!(eval(&vm, JSBigInt::add(&big(5), &big(7))), "12");
        assert_eq!(eval(&vm, JSBigInt::add(&big(5), &big(-7))), "-2");
        assert_eq!(eval(&vm, JSBigInt::add(&big(-5), &big(-7))), "-12");
        assert_eq!(eval(&vm, JSBigInt::sub(&big(5), &big(7))), "-2");
        assert_eq!(eval(&vm, JSBigInt::sub(&big(7), &big(7))), "0");
        // Vai-um entre dígitos.
        assert_eq!(eval(&vm, JSBigInt::add(&big(u64::MAX as i128), &big(1))), "18446744073709551616");
        assert_eq!(eval(&vm, JSBigInt::sub(&big(1i128 << 64), &big(1))), "18446744073709551615");
    }

    #[test]
    fn multiply_divide_and_remainder() {
        let vm = VM::new();
        let a = big(123_456_789_012_345_678_901_234_567_890);
        let b = big(987_654_321_987_654_321);
        let product = heap(JSBigInt::multiply(&a, &b));
        assert_eq!(eval(&vm, JSBigInt::divide(&product, &b)), "123456789012345678901234567890");
        assert_eq!(eval(&vm, JSBigInt::remainder(&product, &b)), "0");
        // a == (a / b) * b + (a % b)
        let quotient = heap(JSBigInt::divide(&a, &b));
        let remainder = heap(JSBigInt::remainder(&a, &b));
        let rebuilt = heap(JSBigInt::add(&heap(JSBigInt::multiply(&quotient, &b)), &remainder));
        assert_eq!(text(&vm, &rebuilt), text(&vm, &a));
        assert_eq!(compare(&remainder, &b), ComparisonResult::LessThan);
        // O quociente e o resto truncam para zero.
        assert_eq!(eval(&vm, JSBigInt::divide(&big(-7), &big(2))), "-3");
        assert_eq!(eval(&vm, JSBigInt::remainder(&big(-7), &big(2))), "-1");
        assert_eq!(eval(&vm, JSBigInt::remainder(&big(7), &big(-2))), "1");
        assert_eq!(JSBigInt::divide(&big(1), &big(0)), Err(BigIntError::InvalidDivisor));
        assert_eq!(JSBigInt::remainder(&big(1), &big(0)), Err(BigIntError::InvalidDivisor));
    }

    #[test]
    fn bitwise_operations_use_twos_complement() {
        let vm = VM::new();
        assert_eq!(eval(&vm, JSBigInt::bitwise_and(&big(12), &big(10))), "8");
        assert_eq!(eval(&vm, JSBigInt::bitwise_or(&big(12), &big(10))), "14");
        assert_eq!(eval(&vm, JSBigInt::bitwise_xor(&big(12), &big(10))), "6");
        assert_eq!(eval(&vm, JSBigInt::bitwise_and(&big(-12), &big(10))), "0");
        assert_eq!(eval(&vm, JSBigInt::bitwise_and(&big(-1), &big(255))), "255");
        assert_eq!(eval(&vm, JSBigInt::bitwise_or(&big(-12), &big(10))), "-2");
        assert_eq!(eval(&vm, JSBigInt::bitwise_xor(&big(-12), &big(10))), "-2");
        assert_eq!(eval(&vm, JSBigInt::bitwise_and(&big(-12), &big(-10))), "-12");
        assert_eq!(eval(&vm, JSBigInt::bitwise_not(&big(0))), "-1");
        assert_eq!(eval(&vm, JSBigInt::bitwise_not(&big(-5))), "4");
    }

    #[test]
    fn shifts_round_towards_negative_infinity() {
        let vm = VM::new();
        assert_eq!(eval(&vm, JSBigInt::left_shift_big_int(&big(1), &big(70))), "1180591620717411303424");
        assert_eq!(eval(&vm, JSBigInt::signed_right_shift(&big(-5), &big(1))), "-3");
        assert_eq!(eval(&vm, JSBigInt::signed_right_shift(&big(5), &big(1))), "2");
        assert_eq!(eval(&vm, JSBigInt::signed_right_shift(&big(5), &big(-2))), "20");
        assert_eq!(eval(&vm, JSBigInt::left_shift_big_int(&big(5), &big(-1))), "2");
        assert_eq!(eval(&vm, JSBigInt::signed_right_shift(&big(-5), &big(1000))), "-1");
        assert_eq!(eval(&vm, JSBigInt::signed_right_shift(&big(5), &big(1000))), "0");
    }

    #[test]
    fn inc_dec_and_as_int_n() {
        let vm = VM::new();
        assert_eq!(eval(&vm, JSBigInt::inc(&big(-1))), "0");
        assert_eq!(eval(&vm, JSBigInt::inc(&big(u64::MAX as i128))), "18446744073709551616");
        assert_eq!(eval(&vm, JSBigInt::dec(&big(0))), "-1");
        assert_eq!(eval(&vm, JSBigInt::dec(&big(1i128 << 64))), "18446744073709551615");
        assert_eq!(eval(&vm, JSBigInt::as_int_n(8, &big(255))), "-1");
        assert_eq!(eval(&vm, JSBigInt::as_int_n(3, &big(-12))), "-4");
        assert_eq!(eval(&vm, JSBigInt::as_uint_n(8, &big(-1))), "255");
        assert_eq!(eval(&vm, JSBigInt::as_uint_n(8, &big(257))), "1");
        assert_eq!(eval(&vm, JSBigInt::as_uint_n(64, &big(-1))), "18446744073709551615");
        assert_eq!(eval(&vm, JSBigInt::as_int_n(64, &big(1i128 << 63))), "-9223372036854775808");
    }

    #[test]
    fn to_number_rounds_to_nearest_even() {
        assert_eq!(JSBigInt::to_number_heap(&big(0)), 0.0);
        assert_eq!(JSBigInt::to_number_heap(&big(-12345)), -12345.0);
        assert_eq!(JSBigInt::to_number_heap(&big((1i128 << 53) + 1)), 9007199254740992.0);
        assert_eq!(JSBigInt::to_number_heap(&big((1i128 << 53) + 3)), 9007199254740996.0);
        assert_eq!(JSBigInt::to_number_heap(&big(1i128 << 100)), 2f64.powi(100));
        assert_eq!(JSBigInt::to_number_heap(&big(1i128 << 120)), 2f64.powi(120));
        assert_eq!(JSBigInt::to_number_heap(&big(u64::MAX as i128)), 18446744073709551616.0);
    }

    #[test]
    fn compares_against_doubles() {
        let five = big(5);
        assert_eq!(compare_to_double(&HeapBigIntImpl::new(&five), 5.0), ComparisonResult::Equal);
        assert_eq!(compare_to_double(&HeapBigIntImpl::new(&five), 5.5), ComparisonResult::LessThan);
        assert_eq!(compare_to_double(&HeapBigIntImpl::new(&five), 4.5), ComparisonResult::GreaterThan);
        assert_eq!(compare_to_double(&HeapBigIntImpl::new(&five), f64::NAN), ComparisonResult::Undefined);
        assert_eq!(compare_to_double(&HeapBigIntImpl::new(&five), f64::INFINITY), ComparisonResult::LessThan);
        assert_eq!(compare_to_double(&HeapBigIntImpl::new(&five), -0.0), ComparisonResult::GreaterThan);
        assert_eq!(compare_to_double(&HeapBigIntImpl::new(&big(0)), -0.0), ComparisonResult::Equal);
        let huge = big(1i128 << 100);
        assert_eq!(compare_to_double(&HeapBigIntImpl::new(&huge), 2f64.powi(100)), ComparisonResult::Equal);
        assert_eq!(compare_to_double(&HeapBigIntImpl::new(&huge), 2f64.powi(101)), ComparisonResult::LessThan);
        assert!(big(7).equals_to_int32(7));
        assert!(!big(7).equals_to_int32(-7));
    }

    #[test]
    fn square_root_and_cube_root() {
        let vm = VM::new();
        assert_eq!(eval(&vm, JSBigInt::sqrt(&big(99))), "9");
        assert_eq!(eval(&vm, JSBigInt::sqrt(&big(100))), "10");
        assert_eq!(eval(&vm, JSBigInt::sqrt(&big(1i128 << 100))), "1125899906842624");
        assert_eq!(eval(&vm, JSBigInt::cbrt(&big(-27))), "-3");
        assert_eq!(eval(&vm, JSBigInt::cbrt(&big(1000))), "10");
        assert_eq!(eval(&vm, JSBigInt::cbrt(&big(999))), "9");
    }

    use crate::runtime::js_big_int::{absolute_compare, compare_double_to_big_int, ParseIntMode, ParseIntSign};

    /// 2^bits, pelo deslocamento (que não passa pela multiplicação).
    fn pow2(bits: i128) -> JSBigInt {
        heap(JSBigInt::left_shift_big_int(&big(1), &big(bits)))
    }

    /// 2^bits - 1: todos os bits ligados.
    fn all_ones(bits: i128) -> JSBigInt {
        heap(JSBigInt::sub(&pow2(bits), &big(1)))
    }

    fn power(base: i128, exponent: i128) -> JSBigInt {
        heap(JSBigInt::exponentiate(&big(base), &big(exponent)))
    }

    fn same(left: &JSBigInt, right: &JSBigInt) -> bool {
        JSBigInt::equals(left, right)
    }

    fn radix_text(vm: &VM, big_int: &JSBigInt, radix: u32) -> Vec<u8> {
        JSBigInt::try_get_string(vm, big_int, radix).latin1()
    }

    const ALPHABET: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

    /// Sequência determinística de palavras de 64 bits (congruencial linear de Knuth).
    fn pseudo_random_words(seed: u64, count: usize) -> Vec<u64> {
        let mut state = seed;
        (0..count)
            .map(|_| {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                state
            })
            .collect()
    }

    fn from_words(words: &[u64]) -> JSBigInt {
        JSBigInt::create_from_words(words, false).unwrap()
    }

    /// Texto de `length` caracteres da base, sem zero à esquerda, com zeros e valores altos no meio.
    fn digit_pattern(radix: usize, length: usize) -> Vec<u8> {
        (0..length).map(|index| if index == 0 { ALPHABET[1] } else { ALPHABET[(index * 7 + 3) % radix] }).collect()
    }

    /// `x == q * y + r` e `|r| < |y|`, só com a soma, a multiplicação e a comparação.
    fn assert_division_invariant(dividend: &JSBigInt, divisor: &JSBigInt) {
        let quotient = heap(JSBigInt::divide(dividend, divisor));
        let remainder = heap(JSBigInt::remainder(dividend, divisor));
        let rebuilt = heap(JSBigInt::add(&heap(JSBigInt::multiply(&quotient, divisor)), &remainder));
        assert!(same(&rebuilt, dividend), "q * y + r diferente do dividendo");
        assert_eq!(
            absolute_compare(&HeapBigIntImpl::new(&remainder), &HeapBigIntImpl::new(divisor)),
            ComparisonResult::LessThan
        );
    }

    #[test]
    fn powers_of_any_radix_print_as_one_followed_by_zeros() {
        let vm = VM::new();
        for radix in [2u32, 3, 5, 7, 8, 10, 12, 16, 32, 36] {
            for exponent in [1i128, 3, 40, 700, 2500] {
                let value = power(radix as i128, exponent);
                let mut expected = vec![b'0'; exponent as usize + 1];
                expected[0] = b'1';
                assert!(radix_text(&vm, &value, radix) == expected, "{radix}^{exponent}");

                // radix^n - 1 tem n dígitos, todos o maior da base.
                let below = heap(JSBigInt::sub(&value, &big(1)));
                let top = ALPHABET[(radix - 1) as usize];
                assert!(radix_text(&vm, &below, radix) == vec![top; exponent as usize], "{radix}^{exponent} - 1");

                // O oposto só ganha o sinal.
                let negative = heap(JSBigInt::unary_minus(&value));
                let mut expected_negative = vec![b'-'];
                expected_negative.extend_from_slice(&expected);
                assert!(radix_text(&vm, &negative, radix) == expected_negative, "-{radix}^{exponent}");
            }
        }
    }

    #[test]
    fn powers_of_ten_and_two_print_exactly_at_every_size() {
        let vm = VM::new();
        // Cobre toStringGeneric abaixo e acima do limiar do formatador de divisão e conquista.
        for exponent in [20i128, 200, 1000, 3000, 20000, 50000] {
            let value = power(10, exponent);
            let mut expected = vec![b'0'; exponent as usize + 1];
            expected[0] = b'1';
            assert!(radix_text(&vm, &value, 10) == expected, "10^{exponent}");
            let below = heap(JSBigInt::sub(&value, &big(1)));
            assert!(radix_text(&vm, &below, 10) == vec![b'9'; exponent as usize], "10^{exponent} - 1");
        }
        assert_eq!(text(&vm, &pow2(64)), "18446744073709551616");
        assert_eq!(text(&vm, &pow2(100)), "1267650600228229401496703205376");
        assert_eq!(text(&vm, &pow2(128)), "340282366920938463463374607431768211456");
    }

    #[test]
    fn products_match_the_closed_form_across_every_algorithm() {
        // (x_digits, y_digits) em dígitos de 64 bits: Comba fixo (1, 2, 4, 8, 16), Comba, schoolbook,
        // Karatsuba (com e sem pedaços), Toom-3 e FFT.
        let shapes: &[(i128, i128)] = &[
            (1, 1),
            (2, 2),
            (4, 4),
            (8, 8),
            (16, 16),
            (3, 3),
            (20, 20),
            (8, 3),
            (40, 3),
            (100, 2),
            (43, 43),
            (44, 44),
            (45, 44),
            (300, 50),
            (100, 100),
            (480, 480),
            (700, 500),
            (1200, 1200),
            (1800, 700),
        ];
        let mut bit_shapes: Vec<(i128, i128)> = shapes.iter().map(|&(x, y)| (x * 64, y * 64)).collect();
        // Operandos que não terminam na fronteira de um dígito.
        bit_shapes.extend_from_slice(&[(100, 37), (129, 65), (127, 127)]);

        for (x_bits, y_bits) in bit_shapes {
            let x = all_ones(x_bits);
            let y = all_ones(y_bits);
            // (2^n - 1)(2^m - 1) = 2^(n+m) - 2^n - 2^m + 1
            let without_x = heap(JSBigInt::sub(&pow2(x_bits + y_bits), &pow2(x_bits)));
            let without_y = heap(JSBigInt::sub(&without_x, &pow2(y_bits)));
            let expected = heap(JSBigInt::add(&without_y, &big(1)));

            let product = heap(JSBigInt::multiply(&x, &y));
            assert!(same(&product, &expected), "{x_bits} bits * {y_bits} bits");
            let swapped = heap(JSBigInt::multiply(&y, &x));
            assert!(same(&swapped, &expected), "{y_bits} bits * {x_bits} bits");

            // O mesmo operando duas vezes é o caminho do quadrado.
            let square_expected = {
                let without = heap(JSBigInt::sub(&pow2(2 * x_bits), &pow2(x_bits + 1)));
                heap(JSBigInt::add(&without, &big(1)))
            };
            let squared = heap(JSBigInt::multiply(&x, &x));
            assert!(same(&squared, &square_expected), "({x_bits} bits)^2");

            // O quociente e o resto devolvem os operandos.
            assert!(same(&heap(JSBigInt::divide(&product, &y)), &x), "{x_bits} bits / {y_bits} bits");
            assert!(same(&heap(JSBigInt::remainder(&product, &y)), &big(0)), "{x_bits} bits % {y_bits} bits");
            let y_minus_one = heap(JSBigInt::sub(&y, &big(1)));
            let almost = heap(JSBigInt::add(&product, &y_minus_one));
            assert!(same(&heap(JSBigInt::divide(&almost, &y)), &x), "(p + y - 1) / y, {x_bits} bits");
            assert!(same(&heap(JSBigInt::remainder(&almost, &y)), &y_minus_one), "(p + y - 1) % y, {x_bits} bits");
        }
    }

    #[test]
    fn recursive_divisions_match_the_closed_form() {
        // (2^N - 1) / (2^m - 1) com N = k*m + r e r < m:
        // 2^N - 1 = 2^r * (2^m - 1) * S + (2^r - 1), onde S = soma de 2^(m*i) para i < k.
        // (m, k, r) em bits: Burnikel-Ziegler (divisor de 57 a 12999 dígitos), Barrett com
        // dividendo até o dobro do divisor, e Barrett em blocos (dividendo maior que o dobro).
        let shapes: &[(i128, i128, i128)] = &[
            (100 * 64 + 9, 1, 60 * 64 + 7),
            (100 * 64 + 9, 3, 5),
            (700 * 64, 2, 64 * 64 + 1),
            (13001 * 64 + 17, 1, 58 * 64 + 3),
            (13001 * 64 + 17, 3, 5),
        ];
        for &(m, k, r) in shapes {
            let divisor = all_ones(m);
            let dividend = all_ones(k * m + r);
            let mut sum = big(0);
            for i in 0..k {
                sum = heap(JSBigInt::add(&sum, &pow2(m * i)));
            }
            let quotient = heap(JSBigInt::left_shift_big_int(&sum, &big(r)));
            assert!(same(&heap(JSBigInt::divide(&dividend, &divisor)), &quotient), "quociente, m={m} k={k} r={r}");
            assert!(same(&heap(JSBigInt::remainder(&dividend, &divisor)), &all_ones(r)), "resto, m={m} k={k} r={r}");
        }
    }

    #[test]
    fn roots_of_powers_of_two_are_exact_at_large_sizes() {
        // sqrt(2^(2k)) = 2^k, sqrt(2^(2k) - 1) = 2^k - 1, cbrt(2^(3k)) = 2^k, cbrt(-(2^(3k) - 1)) = -(2^k - 1).
        for k in [70i128, 64 * 20 + 5, 64 * 700 + 5] {
            let root = pow2(k);
            let below = all_ones(k);
            let sqrt = |value: &JSBigInt| heap(JSBigInt::sqrt(value));
            let cbrt = |value: &JSBigInt| heap(JSBigInt::cbrt(value));
            assert!(same(&sqrt(&pow2(2 * k)), &root), "sqrt(2^{})", 2 * k);
            assert!(same(&sqrt(&all_ones(2 * k)), &below), "sqrt(2^{} - 1)", 2 * k);
            assert!(same(&cbrt(&pow2(3 * k)), &root), "cbrt(2^{})", 3 * k);
            let negative = heap(JSBigInt::unary_minus(&all_ones(3 * k)));
            assert!(same(&cbrt(&negative), &heap(JSBigInt::unary_minus(&below))), "cbrt(-(2^{} - 1))", 3 * k);
        }
    }

    #[test]
    fn pseudo_random_operands_keep_algebraic_identities() {
        // (dígitos de x, dígitos de y): Knuth, Burnikel-Ziegler e as multiplicações grandes.
        for (seed, (x_digits, y_digits)) in [(1u64, (50usize, 45usize)), (2, (300, 50)), (3, (500, 490)), (4, (1300, 700))] {
            let x = from_words(&pseudo_random_words(seed, x_digits));
            let y = from_words(&pseudo_random_words(seed + 100, y_digits));
            let one = big(1);

            // x * (y + 1) == x * y + x
            let product = heap(JSBigInt::multiply(&x, &y));
            let y_plus_one = heap(JSBigInt::add(&y, &one));
            let lhs = heap(JSBigInt::multiply(&x, &y_plus_one));
            let rhs = heap(JSBigInt::add(&product, &x));
            assert!(same(&lhs, &rhs), "distributiva, {x_digits}x{y_digits}");

            // (x * y) / y == x e o resto é zero.
            assert!(same(&heap(JSBigInt::divide(&product, &y)), &x), "(x * y) / y, {x_digits}x{y_digits}");
            assert!(same(&heap(JSBigInt::remainder(&product, &y)), &big(0)), "(x * y) % y, {x_digits}x{y_digits}");

            assert_division_invariant(&x, &y);
            assert_division_invariant(&heap(JSBigInt::unary_minus(&x)), &y);
            assert_division_invariant(&x, &heap(JSBigInt::unary_minus(&y)));
        }
    }

    #[test]
    fn knuth_division_corrects_an_overestimated_quotient_digit() {
        // O caso clássico em que o passo D6 (somar de volta) acontece, em dígitos de 64 bits.
        let dividend = from_words(&[0, 0, 0x8000_0000_0000_0000, 0x7fff_ffff_ffff_ffff]);
        let divisor = from_words(&[1, 0, 0x8000_0000_0000_0000]);
        assert_division_invariant(&dividend, &divisor);
        let dividend = from_words(&[u64::MAX; 6]);
        let divisor = from_words(&[u64::MAX, u64::MAX, 1]);
        assert_division_invariant(&dividend, &divisor);
        let dividend = from_words(&[3, 0, 0, 0x8000_0000_0000_0000]);
        let divisor = from_words(&[1, 0x8000_0000_0000_0000]);
        assert_division_invariant(&dividend, &divisor);
    }

    #[test]
    fn divides_multi_digit_values_by_hand() {
        let vm = VM::new();
        let two_128 = pow2(128);
        let divisor = heap(JSBigInt::add(&pow2(64), &big(1)));
        // 2^128 = (2^64 + 1)(2^64 - 1) + 1
        assert_eq!(eval(&vm, JSBigInt::divide(&two_128, &divisor)), "18446744073709551615");
        assert_eq!(eval(&vm, JSBigInt::remainder(&two_128, &divisor)), "1");
        // 2^128 - 1 = (2^64 + 1)(2^64 - 1)
        assert_eq!(eval(&vm, JSBigInt::divide(&all_ones(128), &divisor)), "18446744073709551615");
        assert_eq!(eval(&vm, JSBigInt::remainder(&all_ones(128), &divisor)), "0");
        // O quociente trunca para zero e o resto leva o sinal do dividendo.
        let negative = heap(JSBigInt::unary_minus(&two_128));
        assert_eq!(eval(&vm, JSBigInt::divide(&negative, &divisor)), "-18446744073709551615");
        assert_eq!(eval(&vm, JSBigInt::remainder(&negative, &divisor)), "-1");
        // 2^128 + 5 = (2^64 - 1)(2^64 + 1) + 6, com divisor de um dígito.
        let plus_five = heap(JSBigInt::add(&two_128, &big(5)));
        assert_eq!(eval(&vm, JSBigInt::divide(&plus_five, &all_ones(64))), "18446744073709551617");
        assert_eq!(eval(&vm, JSBigInt::remainder(&plus_five, &all_ones(64))), "6");
        // Dividendo menor que o divisor.
        assert_eq!(eval(&vm, JSBigInt::divide(&big(5), &pow2(64))), "0");
        assert_eq!(eval(&vm, JSBigInt::remainder(&big(5), &pow2(64))), "5");
        assert_eq!(eval(&vm, JSBigInt::divide(&pow2(64), &pow2(64))), "1");
        assert_eq!(eval(&vm, JSBigInt::remainder(&pow2(64), &pow2(64))), "0");
    }

    #[test]
    fn exponentiation_handles_trivial_bases_and_limits() {
        let vm = VM::new();
        assert_eq!(eval(&vm, JSBigInt::exponentiate(&big(0), &big(0))), "1");
        assert_eq!(eval(&vm, JSBigInt::exponentiate(&big(0), &big(5))), "0");
        assert_eq!(eval(&vm, JSBigInt::exponentiate(&big(1), &big(1000))), "1");
        assert_eq!(eval(&vm, JSBigInt::exponentiate(&big(-1), &big(3))), "-1");
        assert_eq!(eval(&vm, JSBigInt::exponentiate(&big(-1), &big(4))), "1");
        assert_eq!(eval(&vm, JSBigInt::exponentiate(&big(-2), &big(3))), "-8");
        assert_eq!(eval(&vm, JSBigInt::exponentiate(&big(-2), &big(64))), "18446744073709551616");
        assert_eq!(eval(&vm, JSBigInt::exponentiate(&big(2), &big(10))), "1024");
        assert_eq!(eval(&vm, JSBigInt::exponentiate(&big(7), &big(2))), "49");
        assert_eq!(eval(&vm, JSBigInt::exponentiate(&big(3), &big(40))), "12157665459056928801");
        assert_eq!(eval(&vm, JSBigInt::exponentiate(&big(-3), &big(41))), "-36472996377170786403");
        assert_eq!(eval(&vm, JSBigInt::exponentiate(&big(5), &big(30))), "931322574615478515625");
        // Expoente de mais de um dígito só serve para as bases triviais.
        assert_eq!(eval(&vm, JSBigInt::exponentiate(&big(1), &pow2(70))), "1");
        assert_eq!(eval(&vm, JSBigInt::exponentiate(&big(-1), &pow2(70))), "1");
        assert_eq!(JSBigInt::exponentiate(&big(2), &pow2(70)), Err(BigIntError::TooBig));
        assert_eq!(JSBigInt::exponentiate(&big(2), &big(1i128 << 30)), Err(BigIntError::TooBig));
        assert_eq!(JSBigInt::exponentiate(&big(2), &big(-1)), Err(BigIntError::NegativeExponent));
    }

    #[test]
    fn multi_digit_bitwise_follows_twos_complement() {
        let vm = VM::new();
        let two_64 = pow2(64);
        let minus_two_64 = heap(JSBigInt::unary_minus(&two_64));
        let low_ones = all_ones(64);
        // -(2^64) tem 64 zeros embaixo e uns acima.
        assert_eq!(eval(&vm, JSBigInt::bitwise_and(&minus_two_64, &low_ones)), "0");
        assert_eq!(eval(&vm, JSBigInt::bitwise_or(&minus_two_64, &low_ones)), "-1");
        assert_eq!(eval(&vm, JSBigInt::bitwise_xor(&minus_two_64, &low_ones)), "-1");
        assert_eq!(eval(&vm, JSBigInt::bitwise_and(&minus_two_64, &all_ones(65))), "18446744073709551616");
        assert_eq!(eval(&vm, JSBigInt::bitwise_not(&two_64)), "-18446744073709551617");
        assert_eq!(eval(&vm, JSBigInt::bitwise_not(&minus_two_64)), "18446744073709551615");
        assert_eq!(eval(&vm, JSBigInt::bitwise_xor(&big(-1), &two_64)), "-18446744073709551617");

        // -(2^64 + 1) é ~(2^64): todos os bits ligados menos o 64.
        let not_two_64 = big(-(1i128 << 64) - 1);
        let two_64_plus_three = big((1i128 << 64) + 3);
        assert_eq!(eval(&vm, JSBigInt::bitwise_and(&not_two_64, &two_64_plus_three)), "3");
        assert_eq!(eval(&vm, JSBigInt::bitwise_or(&not_two_64, &two_64_plus_three)), "-1");
        assert_eq!(eval(&vm, JSBigInt::bitwise_xor(&not_two_64, &two_64_plus_three)), "-4");

        // Dois negativos.
        assert_eq!(eval(&vm, JSBigInt::bitwise_and(&minus_two_64, &not_two_64)), "-36893488147419103232");
        assert_eq!(eval(&vm, JSBigInt::bitwise_or(&minus_two_64, &not_two_64)), "-1");
        assert_eq!(eval(&vm, JSBigInt::bitwise_xor(&minus_two_64, &not_two_64)), "36893488147419103231");
    }

    #[test]
    fn multi_digit_shifts_round_towards_negative_infinity() {
        let vm = VM::new();
        let two_64 = pow2(64);
        let minus_two_64 = heap(JSBigInt::unary_minus(&two_64));
        assert_eq!(eval(&vm, JSBigInt::left_shift_big_int(&big(3), &big(65))), "110680464442257309696");
        assert_eq!(eval(&vm, JSBigInt::left_shift_big_int(&big(-3), &big(65))), "-110680464442257309696");
        // Deslocamento negativo troca o sentido.
        assert_eq!(eval(&vm, JSBigInt::left_shift_big_int(&big(3), &big(-65))), "0");
        assert_eq!(eval(&vm, JSBigInt::left_shift_big_int(&big(-3), &big(-65))), "-1");
        assert_eq!(eval(&vm, JSBigInt::signed_right_shift(&big(7), &big(-64))), "129127208515966861312");
        assert_eq!(eval(&vm, JSBigInt::signed_right_shift(&two_64, &big(64))), "1");
        // Sem bit perdido, não arredonda.
        assert_eq!(eval(&vm, JSBigInt::signed_right_shift(&minus_two_64, &big(64))), "-1");
        assert_eq!(eval(&vm, JSBigInt::signed_right_shift(&minus_two_64, &big(63))), "-2");
        // Com bit perdido, arredonda para baixo: floor(-(2^64 + 1) / 2^64) == -2.
        let not_two_64 = big(-(1i128 << 64) - 1);
        assert_eq!(eval(&vm, JSBigInt::signed_right_shift(&not_two_64, &big(64))), "-2");
        assert_eq!(eval(&vm, JSBigInt::signed_right_shift(&not_two_64, &big(63))), "-3");
        // O arredondamento que transborda para um dígito novo: floor(-(2^128 - 1) / 2^64) == -(2^64).
        let minus_all_ones = heap(JSBigInt::unary_minus(&all_ones(128)));
        assert_eq!(eval(&vm, JSBigInt::signed_right_shift(&minus_all_ones, &big(64))), "-18446744073709551616");

        // Deslocamento de mais de um dígito: satura em vez de falhar, exceto o para a esquerda.
        let huge = pow2(70);
        assert_eq!(eval(&vm, JSBigInt::signed_right_shift(&two_64, &huge)), "0");
        assert_eq!(eval(&vm, JSBigInt::signed_right_shift(&minus_two_64, &huge)), "-1");
        assert_eq!(eval(&vm, JSBigInt::left_shift_big_int(&big(1), &heap(JSBigInt::unary_minus(&huge)))), "0");
        assert_eq!(JSBigInt::left_shift_big_int(&big(1), &huge), Err(BigIntError::TooBig));
        assert_eq!(JSBigInt::left_shift_big_int(&big(1), &big(1i128 << 30)), Err(BigIntError::TooBig));
        assert_eq!(JSBigInt::left_shift_big_int(&big(1), &big((1i128 << 30) + 1)), Err(BigIntError::TooBig));
    }

    #[test]
    fn compares_against_doubles_at_the_edges_of_precision() {
        let against = |value: &JSBigInt, number: f64| compare_to_double(&HeapBigIntImpl::new(value), number);
        let negate = |value: &JSBigInt| heap(JSBigInt::unary_minus(value));

        // 2^53 é o último inteiro em que o double ainda enxerga o +1.
        let two_53 = pow2(53);
        let two_53_plus_one = heap(JSBigInt::add(&two_53, &big(1)));
        assert_eq!(against(&two_53, 9007199254740992.0), ComparisonResult::Equal);
        assert_eq!(against(&two_53_plus_one, 9007199254740992.0), ComparisonResult::GreaterThan);
        assert_eq!(against(&negate(&two_53_plus_one), -9007199254740992.0), ComparisonResult::LessThan);

        // Dois dígitos: 2^64 - 1, 2^64 e 2^64 + 1 contra 2^64.
        let two_64_as_double = 18446744073709551616.0;
        assert_eq!(against(&all_ones(64), two_64_as_double), ComparisonResult::LessThan);
        assert_eq!(against(&pow2(64), two_64_as_double), ComparisonResult::Equal);
        assert_eq!(against(&heap(JSBigInt::add(&pow2(64), &big(1))), two_64_as_double), ComparisonResult::GreaterThan);

        // Mantissa que atravessa dois dígitos: 2^70 + 2^19 cabe exato em 53 bits.
        let exact = big((1i128 << 70) + (1i128 << 19));
        let as_double = 2f64.powi(70) + 2f64.powi(19);
        assert_eq!(against(&exact, as_double), ComparisonResult::Equal);
        assert_eq!(against(&big((1i128 << 70) + (1i128 << 19) + 1), as_double), ComparisonResult::GreaterThan);
        assert_eq!(against(&big((1i128 << 70) + (1i128 << 19) - 1), as_double), ComparisonResult::LessThan);
        assert_eq!(against(&negate(&exact), -as_double), ComparisonResult::Equal);

        // Parte fracionária e sinais.
        assert_eq!(against(&big(3), 3.5), ComparisonResult::LessThan);
        assert_eq!(against(&big(-3), -3.5), ComparisonResult::GreaterThan);
        assert_eq!(against(&big(-3), -2.5), ComparisonResult::LessThan);
        assert_eq!(against(&big(1), 0.5), ComparisonResult::GreaterThan);
        assert_eq!(against(&big(-1), -0.5), ComparisonResult::LessThan);
        assert_eq!(against(&big(0), 5e-324), ComparisonResult::LessThan);
        assert_eq!(against(&big(1), 5e-324), ComparisonResult::GreaterThan);
        assert_eq!(against(&big(0), -5e-324), ComparisonResult::GreaterThan);

        // O maior double finito, 2^127 e os infinitos.
        assert_eq!(against(&pow2(1024), f64::MAX), ComparisonResult::GreaterThan);
        assert_eq!(against(&pow2(1023), f64::MAX), ComparisonResult::LessThan);
        assert_eq!(against(&all_ones(127), 2f64.powi(127)), ComparisonResult::LessThan);
        assert_eq!(against(&pow2(2000), f64::INFINITY), ComparisonResult::LessThan);
        assert_eq!(against(&pow2(2000), f64::NEG_INFINITY), ComparisonResult::GreaterThan);
        assert_eq!(against(&negate(&pow2(2000)), f64::NEG_INFINITY), ComparisonResult::GreaterThan);
        assert_eq!(against(&big(0), f64::NAN), ComparisonResult::Undefined);

        // O lado do double na frente é o espelho.
        assert_eq!(compare_double_to_big_int(5.5, &HeapBigIntImpl::new(&big(5))), ComparisonResult::GreaterThan);
        assert_eq!(compare_double_to_big_int(5.0, &HeapBigIntImpl::new(&big(5))), ComparisonResult::Equal);
        assert_eq!(compare_double_to_big_int(-5.5, &HeapBigIntImpl::new(&big(-5))), ComparisonResult::LessThan);
    }

    struct TestGlobal {
        vm: VM,
    }

    impl BigIntGlobalObject for TestGlobal {
        fn vm(&self) -> &VM {
            &self.vm
        }

        fn throw_syntax_error(&self, _vm: &VM, _message: &str) {}

        fn throw_out_of_memory_error(&self, _vm: &VM, _message: Option<&str>) {}
    }

    fn parse(global: &TestGlobal, source: &str) -> Option<JSBigInt> {
        match JSBigInt::parse_int_span(global, source.as_bytes(), ErrorParseMode::IgnoreExceptions) {
            ImplResult::Heap(big_int) => Some(big_int),
            ImplResult::BigInt32(value) => Some(JSBigInt::create_from_i32(value).unwrap()),
            ImplResult::Empty => None,
        }
    }

    fn parse_with_radix(global: &TestGlobal, source: &[u8], radix: u32) -> Option<JSBigInt> {
        let result = JSBigInt::parse_int_span_with_radix(
            Some(global as &dyn BigIntGlobalObject),
            &global.vm,
            source,
            0,
            radix,
            ErrorParseMode::IgnoreExceptions,
            ParseIntSign::Unsigned,
            ParseIntMode::DisallowEmptyString,
        );
        match result {
            ImplResult::Heap(big_int) => Some(big_int),
            ImplResult::BigInt32(value) => Some(JSBigInt::create_from_i32(value).unwrap()),
            ImplResult::Empty => None,
        }
    }

    #[test]
    fn parses_literals_prefixes_signs_and_whitespace() {
        let global = TestGlobal { vm: VM::new() };
        let decimal = |source: &str| parse(&global, source).map(|value| text(&global.vm, &value));

        let accepted: &[(&str, &str)] = &[
            ("0xff", "255"),
            ("0XFF", "255"),
            ("0xFf", "255"),
            ("0b101", "5"),
            ("0B11", "3"),
            ("0o17", "15"),
            ("0O777", "511"),
            ("0x0000ff", "255"),
            ("  42  ", "42"),
            ("\t\n 12 \r\n", "12"),
            (" 0x1f ", "31"),
            ("-42", "-42"),
            ("+7", "7"),
            ("00012", "12"),
            ("-0", "0"),
            ("", "0"),
            ("   ", "0"),
            ("0", "0"),
            ("-2147483648", "-2147483648"),
            ("2147483647", "2147483647"),
            ("2147483648", "2147483648"),
            ("0x7FFFFFFF", "2147483647"),
            ("0xFFFFFFFF", "4294967295"),
            ("0x10000000000000000", "18446744073709551616"),
            ("0xffffffffffffffffffff", "1208925819614629174706175"),
            ("1208925819614629174706175", "1208925819614629174706175"),
            ("0b1111111111111111111111111111111111111111111111111111111111111111", "18446744073709551615"),
            ("0o7777777777777777777777", "73786976294838206463"),
        ];
        for (source, expected) in accepted {
            assert_eq!(decimal(*source).as_deref(), Some(*expected), "{source:?}");
        }

        // StringToBigInt não aceita separador, sufixo, expoente nem sinal em literal com prefixo.
        for source in ["1_000", "12n", "1e3", "1.5", "0x", "0b", "0o", "0xG", "0b102", "0o8", "-0x10", "+0xff", "- 1", "1 2", "0x 1", "--1"] {
            assert!(parse(&global, source).is_none(), "{source:?} devia ser recusado");
        }
    }

    #[test]
    fn parsing_inverts_printing_at_every_size_and_radix() {
        let global = TestGlobal { vm: VM::new() };
        let lengths = [1usize, 7, 8, 9, 10, 16, 17, 19, 20, 30, 31, 57, 58, 59, 64, 65, 76, 77, 100, 121, 122, 160, 1000, 4000];

        // Prefixos de literal: potências de dois (empacotamento de bits).
        for (prefix, radix) in [("0x", 16usize), ("0o", 8), ("0b", 2)] {
            for length in lengths {
                let digits = digit_pattern(radix, length);
                let source = format!("{prefix}{}", String::from_utf8(digits.clone()).unwrap());
                let parsed = parse(&global, &source).unwrap_or_else(|| panic!("{source} recusado"));
                assert!(radix_text(&global.vm, &parsed, radix as u32) == digits, "radix {radix}, {length} caracteres");
            }
        }

        // Decimal e bases que não são potência de dois (multiplyAdd e fromStringLarge).
        for radix in [3usize, 5, 7, 10, 11, 36] {
            for length in lengths {
                let digits = digit_pattern(radix, length);
                let parsed = parse_with_radix(&global, &digits, radix as u32).unwrap_or_else(|| panic!("radix {radix}, {length} recusado"));
                assert!(radix_text(&global.vm, &parsed, radix as u32) == digits, "radix {radix}, {length} caracteres");
                // O mesmo texto com o sinal na frente (só a base 10 tem o caminho público).
                if radix == 10 {
                    let source = format!("-{}", String::from_utf8(digits.clone()).unwrap());
                    let negative = parse(&global, &source).unwrap();
                    let mut expected = vec![b'-'];
                    expected.extend_from_slice(&digits);
                    assert!(radix_text(&global.vm, &negative, 10) == expected, "-{length} caracteres");
                }
            }
        }

        // Os nove e as potências de dez, que se sabe de cabeça.
        for exponent in [57i128, 58, 100, 1000, 6000] {
            let nines = "9".repeat(exponent as usize);
            let one_followed_by_zeros = format!("1{}", "0".repeat(exponent as usize));
            let parsed_nines = parse(&global, &nines).unwrap();
            let parsed_power = parse(&global, &one_followed_by_zeros).unwrap();
            assert!(same(&parsed_power, &power(10, exponent)), "10^{exponent}");
            assert!(same(&parsed_nines, &heap(JSBigInt::sub(&power(10, exponent), &big(1)))), "10^{exponent} - 1");
        }
    }

    #[test]
    fn parsing_rejects_one_invalid_character_anywhere() {
        let global = TestGlobal { vm: VM::new() };
        for (radix, length) in [(10u32, 5usize), (10, 200), (16, 200), (2, 100), (7, 300), (36, 100)] {
            let mut digits = digit_pattern(radix as usize, length);
            assert!(parse_with_radix(&global, &digits, radix).is_some());
            let middle = length / 2;
            // Um caractere que não é dígito da base: o próprio `radix` (ou '_' na base 36).
            digits[middle] = if radix == 36 { b'_' } else { ALPHABET[radix as usize] };
            assert!(parse_with_radix(&global, &digits, radix).is_none(), "radix {radix}, {length} caracteres");
            let last = length - 1;
            let mut digits = digit_pattern(radix as usize, length);
            digits[last] = b'_';
            assert!(parse_with_radix(&global, &digits, radix).is_none(), "radix {radix}, {length} caracteres, no fim");
        }
    }

    const OVER_LIMIT_BITS: i128 = (1 << 30) + 1;

    #[test]
    fn exponentiation_past_the_maximum_size_fails_before_allocating() {
        assert_eq!(JSBigInt::exponentiate(&big(2), &big(1 << 30)).err(), Some(BigIntError::TooBig));
        assert_eq!(JSBigInt::exponentiate(&big(3), &big(1 << 40)).err(), Some(BigIntError::TooBig));
        assert_eq!(JSBigInt::exponentiate(&big(2), &big(1i128 << 70)).err(), Some(BigIntError::TooBig));
        assert_eq!(JSBigInt::exponentiate(&big(-7), &big(OVER_LIMIT_BITS)).err(), Some(BigIntError::TooBig));
    }

    #[test]
    fn left_shift_past_the_maximum_size_fails_before_allocating() {
        assert_eq!(JSBigInt::left_shift_big_int(&big(1), &big(OVER_LIMIT_BITS)).err(), Some(BigIntError::TooBig));
        assert_eq!(JSBigInt::left_shift_big_int(&big(1), &big(1 << 30)).err(), Some(BigIntError::TooBig));
        assert_eq!(JSBigInt::left_shift_big_int(&big(-1), &big(1i128 << 80)).err(), Some(BigIntError::TooBig));
    }

    #[test]
    fn as_int_n_and_as_uint_n_past_the_maximum_size_fail_before_allocating() {
        let vm = VM::new();
        assert_eq!(JSBigInt::as_uint_n(OVER_LIMIT_BITS as u64, &big(-1)).err(), Some(BigIntError::TooBig));
        assert_eq!(JSBigInt::as_uint_n(u64::MAX, &big(-5)).err(), Some(BigIntError::TooBig));
        // Positivo com `n` enorme: o próprio valor, sem alocar.
        assert_eq!(eval(&vm, JSBigInt::as_uint_n(u64::MAX, &big(5))), "5");
    }

    #[test]
    fn digit_buffers_report_out_of_memory_instead_of_aborting() {
        use crate::runtime::js_big_int::try_zeroed_digits;
        assert_eq!(try_zeroed_digits(usize::MAX / 4).err(), Some(BigIntError::OutOfMemory));
        assert_eq!(try_zeroed_digits(3).unwrap(), vec![0, 0, 0]);
    }
}
