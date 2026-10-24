//! Tradução de `runtime/Operations.h`, `Operations.cpp`, `OperationsInlines.h` e das comparações de
//! `JSCJSValueInlines.h` (`equalSlowCaseInline`, `strictEqual`) para valores primitivos e
//! `JSString`: o que os slow paths aritméticos de `1+1`, `a+1` e `"a"+1` precisam.
//!
//! DIVERGÊNCIAS e o que fica de fora (depende de objetos, símbolos e BigInt no heap, camada 3):
//!
//! - Sem `JSGlobalObject*` e sem `ThrowScope`: com primitivos e strings nenhuma operação lança,
//!   exceto o `OutOfMemoryError` de `jsString(globalObject, ...)` quando o comprimento soma mais que
//!   `i32::MAX`. Esse caso devolve `None` em [`js_string_concat`], [`js_add`], [`js_add_non_number`]
//!   e [`js_add_slow_case`]; o `throwOutOfMemoryError(globalObject, scope)` entra nesse ponto quando o
//!   `ErrorInstance` existir.
//! - `JSRopeString` existe (`js_string.rs`): `js_add`, `js_add_non_number`, `js_add_slow_case` e o
//!   `strcat` do LLInt concatenam `JSStringRef` por rope (`js_string_concat_strings`, `RopeBuilder`)
//!   sem materializar o texto; primitivos que não são string viram `JSString` antes de entrar na
//!   rope. Só `js_string_concat` (sobre `WtfString`) materializa, e fica para quem já tem texto
//!   (nome de função com prefixo, `reify`), onde o resultado precisa ser um `WtfString`.
//! - Célula que não é `JSString`: as conversões de `JSValue` (`js_value_conversions`) tratam objeto,
//!   `Symbol` e `JSBigInt`, e uma exceção delas fica pendente no `VM` (o `RETURN_IF_EXCEPTION` do C++
//!   vira a conferência de `vm.exception()` e o `JSValue::empty()`/`false` de volta). A aritmética e a
//!   comparação de `JSBigInt` (`compareBigInt`, `bigIntCompare`, `JSBigInt::add/sub/...`,
//!   `equalsToNumber`, `stringToBigInt`) estão em `js_big_int_ops`; a mistura de `BigInt` com número
//!   lança o TypeError "Invalid mix of BigInt and other type in ...".
//! - `jsTypeStringForValue`, `jsTypeofIsObject`, `jsTypeofIsFunction`, `normalizePrototypeChain`,
//!   `jsStringFromRegisterArray`, `getByValWithIndex*`, os deslocamentos e as operações bit a bit
//!   (`shift`, `jsLShift`, `bitwiseBinaryOp`...), `jsPow` e `jsBitwiseNot` não fazem parte desta
//!   fatia.

use crate::runtime::current_realm::{current_global_object, has_pending_exception};
use crate::runtime::host_function_support::throw_vm_type_error;
use crate::runtime::js_big_int::{ComparisonMode, JSBigInt};
use crate::runtime::js_big_int_ops::{
    big_int_binary_op, big_int_compare, big_int_equals_big_int_or_number, big_int_equals_string, big_int_strict_equals,
    big_int_unary_op, BigIntBinaryOp, BigIntUnaryOp,
};
use crate::runtime::js_string::{js_empty_string, js_string, JSString, JSStringRef, RopeFibers, MAX_INTERNAL_ROPE_LENGTH, MAX_LENGTH};
use std::rc::Rc;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::js_value_conversions::cell_masquerades_as_undefined;
use crate::runtime::object_to_primitive::PreferredPrimitiveType;
use crate::runtime::vm::VM;
use crate::wtf::text::string_view::{code_point_compare_less_than, equal, StringView};
use crate::wtf::text::wtf_string::String as WtfString;

/// `JSString::equal(globalObject, JSString*)` para strings sem rope: `WTF::equal` dos dois textos.
fn js_string_equal(a: &JSStringRef, b: &JSStringRef) -> bool {
    let a = a.value();
    let b = b.value();
    equal(StringView::from(&a), StringView::from(&b))
}

/// `jsString(globalObject, JSString*, JSString*)`, `jsString(globalObject, const String&, JSString*)`,
/// `jsString(globalObject, JSString*, const String&)` e `jsString(globalObject, const String&, const
/// String&)` (`OperationsInlines.h`): a concatenação das quatro, com as `JSString` passadas pelo
/// texto (`JSString::value`). `None` é o `throwOutOfMemoryError` do comprimento que soma mais que
/// `i32::MAX` (`sumOverflows<int32_t>`).
pub fn js_string_concat(vm: &VM, left: &WtfString, right: &WtfString) -> Option<JSStringRef> {
    let length1 = left.length();
    if length1 == 0 {
        return Some(js_string(vm, right));
    }
    let length2 = right.length();
    if length2 == 0 {
        return Some(js_string(vm, left));
    }
    if length1 as u64 + length2 as u64 > i32::MAX as u64 {
        return None;
    }

    if left.is_8bit() && right.is_8bit() {
        // A falta de memória é `None`, o mesmo `throwOutOfMemoryError` do estouro de comprimento.
        let mut characters: Vec<u8> = crate::runtime::fallible_alloc::try_vec_with_capacity((length1 + length2) as usize)?;
        characters.extend_from_slice(left.span8());
        characters.extend_from_slice(right.span8());
        return Some(js_string(vm, &WtfString::adopt(characters)));
    }

    let mut characters: Vec<u16> = crate::runtime::fallible_alloc::try_vec_with_capacity((length1 + length2) as usize)?;
    for part in [left, right] {
        if part.is_8bit() {
            characters.extend(part.span8().iter().map(|&c| c as u16));
        } else {
            characters.extend_from_slice(part.span16());
        }
    }
    Some(js_string(vm, &WtfString::adopt(characters)))
}

/// `sumOverflows<int32_t>(lengths...)`: a soma, feita em `u64`, passa de `JSString::MaxLength`.
fn sum_overflows(lengths: &[u32]) -> bool {
    lengths.iter().map(|&length| u64::from(length)).sum::<u64>() > u64::from(MAX_LENGTH)
}

/// `jsString(globalObject, JSString*, JSString*)` (`OperationsInlines.h`): lado vazio devolve o outro, o
/// comprimento acima de `MaxLength` é `None` (o `throwOutOfMemoryError`), senão uma rope sem copiar texto.
pub fn js_string_concat_strings(vm: &VM, s1: &JSStringRef, s2: &JSStringRef) -> Option<JSStringRef> {
    let length1 = s1.length();
    if length1 == 0 {
        return Some(Rc::clone(s2));
    }
    let length2 = s2.length();
    if length2 == 0 {
        return Some(Rc::clone(s1));
    }
    if sum_overflows(&[length1, length2]) {
        return None;
    }
    Some(JSString::create_rope(vm, RopeFibers::from_two(Rc::clone(s1), Rc::clone(s2))))
}

/// `jsString(globalObject, JSString*, JSString*, JSString*)`: os lados vazios saem e o que sobra cai nas
/// variantes de uma e duas fibras.
pub fn js_string_concat_three_strings(vm: &VM, s1: &JSStringRef, s2: &JSStringRef, s3: &JSStringRef) -> Option<JSStringRef> {
    if s1.length() == 0 {
        return js_string_concat_strings(vm, s2, s3);
    }
    if s2.length() == 0 {
        return js_string_concat_strings(vm, s1, s3);
    }
    if s3.length() == 0 {
        return js_string_concat_strings(vm, s1, s2);
    }
    if sum_overflows(&[s1.length(), s2.length(), s3.length()]) {
        return None;
    }
    Some(JSString::create_rope(vm, RopeFibers::from_three(Rc::clone(s1), Rc::clone(s2), Rc::clone(s3))))
}

/// `JSRopeString::RopeBuilder<RecordOverflow>`. DIVERGÊNCIA: só a variante `RecordOverflow` (a que
/// `jsStringFromRegisterArray` usa); a `CrashOnOverflow` não tem chamador no porte.
pub struct RopeBuilder<'a> {
    vm: &'a VM,
    strings: Vec<JSStringRef>,
    length: u32,
    overflowed: bool,
}

impl<'a> RopeBuilder<'a> {
    pub fn new(vm: &'a VM) -> RopeBuilder<'a> {
        RopeBuilder { vm, strings: Vec::with_capacity(MAX_INTERNAL_ROPE_LENGTH), length: 0, overflowed: false }
    }

    /// `hasOverflowed()`.
    pub fn has_overflowed(&self) -> bool {
        self.overflowed
    }

    /// `append(JSString*)`: `false` quando a soma passa de `MaxLength` (e o construtor fica estourado).
    /// Strings vazias são puladas. Com as três fibras cheias, `expand` funde-as numa rope nova.
    pub fn append(&mut self, string: &JSStringRef) -> bool {
        if self.overflowed {
            return false;
        }
        let string_length = string.length();
        if string_length == 0 {
            return true;
        }
        if self.strings.len() == MAX_INTERNAL_ROPE_LENGTH {
            self.expand();
        }
        let sum = u64::from(self.length) + u64::from(string_length);
        if sum > u64::from(MAX_LENGTH) {
            self.overflowed = true;
            return false;
        }
        self.strings.push(Rc::clone(string));
        self.length = sum as u32;
        true
    }

    /// `expand()`: as três fibras viram uma rope, que passa a ser a primeira.
    fn expand(&mut self) {
        debug_assert!(!self.overflowed && self.strings.len() == MAX_INTERNAL_ROPE_LENGTH && self.length != 0);
        let mut fibers = self.strings.drain(..);
        let (first, second, third) = (fibers.next(), fibers.next(), fibers.next());
        drop(fibers);
        let (Some(first), Some(second), Some(third)) = (first, second, third) else {
            unreachable!("expand() com menos de três fibras");
        };
        let rope = JSString::create_rope(self.vm, RopeFibers::from_three(first, second, third));
        self.strings.push(rope);
    }

    /// `release()`: a string do acumulado (vazia, a própria fibra, ou rope de duas ou três) e o construtor
    /// volta ao zero. Estourado é um `RELEASE_ASSERT` no C++.
    pub fn release(&mut self) -> JSStringRef {
        assert!(!self.overflowed, "RopeBuilder::release() depois de estourar");
        let mut strings = std::mem::take(&mut self.strings).into_iter();
        let result = match (strings.next(), strings.next(), strings.next()) {
            (None, ..) => {
                debug_assert_eq!(self.length, 0);
                js_empty_string(self.vm)
            }
            (Some(first), None, _) => first,
            (Some(first), Some(second), None) => JSString::create_rope(self.vm, RopeFibers::from_two(first, second)),
            (Some(first), Some(second), Some(third)) => {
                JSString::create_rope(self.vm, RopeFibers::from_three(first, second, third))
            }
        };
        debug_assert_eq!(result.length(), self.length);
        self.length = 0;
        result
    }

    /// `length()`.
    pub fn length(&self) -> u32 {
        debug_assert!(!self.overflowed);
        self.length
    }
}

/// `jsAddSlowCase(globalObject, v1, v2)` (`Operations.cpp`). `Some(JSValue::empty())` é o `JSValue()` do
/// `RETURN_IF_EXCEPTION`: a exceção está pendente no `VM`.
pub fn js_add_slow_case(vm: &VM, v1: JSValue, v2: JSValue) -> Option<JSValue> {
    let p1 = v1.to_primitive();
    if p1.is_empty() {
        return Some(p1);
    }
    let p2 = v2.to_primitive();
    if p2.is_empty() {
        return Some(p2);
    }

    if p1.is_string() {
        return concat_with_primitive(vm, &p1.as_js_string(), p2, false);
    }

    if p2.is_string() {
        return concat_with_primitive(vm, &p2.as_js_string(), p1, true);
    }

    Some(arithmetic_binary_op(p1, p2, |left, right| left + right, JSBigInt::add, "Invalid mix of BigInt and other type in addition."))
}

/// O `jsString(globalObject, asString(p1), p2String)` de `jsAddSlowCase` e de `jsAddNonNumber`, com o
/// `toString` (célula) ou `toWTFString` (imediato) de `other`. `string_is_right` é o caso `p2.isString()`,
/// em que a string fica à direita.
fn concat_with_primitive(vm: &VM, string: &JSStringRef, other: JSValue, string_is_right: bool) -> Option<JSValue> {
    // O primitivo que não é string vira `JSString` antes de entrar na rope; a `string` não é lida.
    let other_string = if other.is_cell() { other.to_string(vm) } else { js_string(vm, &other.to_wtf_string()) };
    if vm.exception().is_some() {
        return Some(JSValue::empty());
    }
    let result = if string_is_right {
        js_string_concat_strings(vm, &other_string, string)
    } else {
        js_string_concat_strings(vm, string, &other_string)
    };
    result.map(JSValue::from_js_string)
}

/// `jsAddNonNumber(globalObject, v1, v2)` (`OperationsInlines.h`).
pub fn js_add_non_number(vm: &VM, v1: JSValue, v2: JSValue) -> Option<JSValue> {
    debug_assert!(!v1.is_number() || !v2.is_number());

    if v1.is_string() && !v2.is_object() {
        let s1 = v1.as_js_string();
        if v2.is_string() {
            return js_string_concat_strings(vm, &s1, &v2.as_js_string()).map(JSValue::from_js_string);
        }
        let s2 = v2.to_wtf_string();
        if vm.exception().is_some() {
            return Some(JSValue::empty());
        }
        return js_string_concat_strings(vm, &s1, &js_string(vm, &s2)).map(JSValue::from_js_string);
    }

    // All other cases are pretty uncommon
    js_add_slow_case(vm, v1, v2)
}

/// `jsAdd(globalObject, v1, v2)`: o operador `+`.
pub fn js_add(vm: &VM, v1: JSValue, v2: JSValue) -> Option<JSValue> {
    if v1.is_number() && v2.is_number() {
        return Some(js_number(v1.as_number() + v2.as_number()));
    }

    js_add_non_number(vm, v1, v2)
}

/// `arithmeticBinaryOp(globalObject, v1, v2, doubleOp, bigIntOp, errorMessage)`. O `JSValue` vazio é o
/// `JSValue()` com a exceção pendente no `VM` (a de `toNumeric`, a do `bigIntOp` ou o `TypeError` de
/// `errorMessage`, que o C++ lança ao misturar `BigInt` com número).
pub fn arithmetic_binary_op(
    v1: JSValue,
    v2: JSValue,
    double_op: impl Fn(f64, f64) -> f64,
    big_int_op: BigIntBinaryOp,
    error_message: &str,
) -> JSValue {
    let left_numeric = v1.to_numeric();
    if left_numeric.is_empty() {
        return left_numeric;
    }
    let right_numeric = v2.to_numeric();
    if right_numeric.is_empty() {
        return right_numeric;
    }

    if left_numeric.is_number() && right_numeric.is_number() {
        return js_number(double_op(left_numeric.as_number(), right_numeric.as_number()));
    }

    if left_numeric.is_big_int() && right_numeric.is_big_int() {
        return big_int_binary_op(left_numeric, right_numeric, big_int_op);
    }

    throw_vm_type_error(&current_global_object(), Some(error_message));
    JSValue::empty()
}

/// `jsSub(globalObject, v1, v2)`: o operador `-`.
pub fn js_sub(v1: JSValue, v2: JSValue) -> JSValue {
    arithmetic_binary_op(v1, v2, |left, right| left - right, JSBigInt::sub, "Invalid mix of BigInt and other type in subtraction.")
}

/// `jsMul(globalObject, v1, v2)`: o operador `*`.
pub fn js_mul(v1: JSValue, v2: JSValue) -> JSValue {
    arithmetic_binary_op(
        v1,
        v2,
        |left, right| left * right,
        JSBigInt::multiply,
        "Invalid mix of BigInt and other type in multiplication.",
    )
}

/// `jsDiv(globalObject, v1, v2)`: o operador `/`.
pub fn js_div(v1: JSValue, v2: JSValue) -> JSValue {
    arithmetic_binary_op(v1, v2, |left, right| left / right, JSBigInt::divide, "Invalid mix of BigInt and other type in division.")
}

/// `jsRemainder(globalObject, v1, v2)`: o operador `%`. `Math::fmodDouble` é o `fmod` da libm, que
/// no Rust é o `%` de `f64`.
pub fn js_remainder(v1: JSValue, v2: JSValue) -> JSValue {
    arithmetic_binary_op(
        v1,
        v2,
        |left, right| left % right,
        JSBigInt::remainder,
        "Invalid mix of BigInt and other type in remainder.",
    )
}

/// `jsInc(globalObject, v)`.
pub fn js_inc(v: JSValue) -> JSValue {
    unary_numeric_op(v, 1.0, JSBigInt::inc)
}

/// `jsDec(globalObject, v)`.
pub fn js_dec(v: JSValue) -> JSValue {
    unary_numeric_op(v, -1.0, JSBigInt::dec)
}

/// O `toNumeric` e a soma de `delta` de `jsInc` e `jsDec`; `big_int_op` é o `JSBigInt::inc`/`dec`.
fn unary_numeric_op(v: JSValue, delta: f64, big_int_op: BigIntUnaryOp) -> JSValue {
    let operand_numeric = v.to_numeric();
    if operand_numeric.is_empty() {
        return operand_numeric;
    }

    if operand_numeric.is_number() {
        return js_number(operand_numeric.as_number() + delta);
    }

    debug_assert!(operand_numeric.is_big_int());
    big_int_unary_op(operand_numeric, big_int_op)
}

/// `toPrimitiveNumeric(globalObject, v, p, n)`: `Some((p, n, resultado))`, ou `None` com a exceção
/// pendente. `p.isBigInt()` devolve `true` sem calcular `n`.
fn to_primitive_numeric(v: JSValue) -> Option<(JSValue, f64, bool)> {
    let p = v.to_primitive_preferred(PreferredPrimitiveType::PreferNumber);
    if p.is_empty() {
        return None;
    }
    if p.is_big_int() {
        return Some((p, 0.0, true));
    }

    let n = p.to_number();
    if has_pending_exception() {
        return None;
    }
    Some((p, n, !p.is_string()))
}

/// A comparação de duas strings de `jsLess`/`jsLessEq`: `codePointCompareLessThan(s1, s2)`.
fn js_string_less_than(s1: &JSStringRef, s2: &JSStringRef) -> bool {
    let s1 = s1.value();
    let s2 = s2.value();
    code_point_compare_less_than(StringView::from(&s1), StringView::from(&s2))
}

/// `toPrimitiveNumeric` dos dois operandos na ordem de `LEFT_FIRST`: `None` com a exceção pendente.
fn to_primitive_numeric_pair<const LEFT_FIRST: bool>(
    v1: JSValue,
    v2: JSValue,
) -> Option<((JSValue, f64, bool), (JSValue, f64, bool))> {
    if LEFT_FIRST {
        let first = to_primitive_numeric(v1)?;
        let second = to_primitive_numeric(v2)?;
        Some((first, second))
    } else {
        let second = to_primitive_numeric(v2)?;
        let first = to_primitive_numeric(v1)?;
        Some((first, second))
    }
}

// See ES5 11.8.1/11.8.2/11.8.5 for definition of leftFirst, this value ensures correct
// evaluation ordering for argument conversions for '<' and '>'. For '<' pass the value
// true, for leftFirst, for '>' pass the value false (and reverse operand order).
/// `jsLess<leftFirst>(globalObject, v1, v2)`: `false` com a exceção pendente.
pub fn js_less<const LEFT_FIRST: bool>(v1: JSValue, v2: JSValue) -> bool {
    if v1.is_int32() && v2.is_int32() {
        return v1.as_int32() < v2.as_int32();
    }

    if v1.is_number() && v2.is_number() {
        return v1.as_number() < v2.as_number();
    }

    if v1.is_string() && v2.is_string() {
        return js_string_less_than(&v1.as_js_string(), &v2.as_js_string());
    }

    let Some(((p1, n1, was_not_string1), (p2, n2, was_not_string2))) = to_primitive_numeric_pair::<LEFT_FIRST>(v1, v2)
    else {
        return false;
    };

    if was_not_string1 || was_not_string2 {
        if p1.is_big_int() || p2.is_big_int() {
            return big_int_compare(p1, p2, ComparisonMode::LessThan);
        }
        return n1 < n2;
    }

    js_string_less_than(&p1.as_js_string(), &p2.as_js_string())
}

// See ES5 11.8.3/11.8.4/11.8.5 for definition of leftFirst, this value ensures correct
// evaluation ordering for argument conversions for '<=' and '=>'. For '<=' pass the
// value true, for leftFirst, for '=>' pass the value false (and reverse operand order).
/// `jsLessEq<leftFirst>(globalObject, v1, v2)`: `false` com a exceção pendente.
pub fn js_less_eq<const LEFT_FIRST: bool>(v1: JSValue, v2: JSValue) -> bool {
    if v1.is_int32() && v2.is_int32() {
        return v1.as_int32() <= v2.as_int32();
    }

    if v1.is_number() && v2.is_number() {
        return v1.as_number() <= v2.as_number();
    }

    if v1.is_string() && v2.is_string() {
        return !js_string_less_than(&v2.as_js_string(), &v1.as_js_string());
    }

    let Some(((p1, n1, was_not_string1), (p2, n2, was_not_string2))) = to_primitive_numeric_pair::<LEFT_FIRST>(v1, v2)
    else {
        return false;
    };

    if was_not_string1 || was_not_string2 {
        if p1.is_big_int() || p2.is_big_int() {
            return big_int_compare(p1, p2, ComparisonMode::LessThanOrEqual);
        }
        return n1 <= n2;
    }

    !js_string_less_than(&p2.as_js_string(), &p1.as_js_string())
}

/// `operator==(JSValue, JSValue)`: igualdade dos 64 bits codificados.
fn same_bits(v1: JSValue, v2: JSValue) -> bool {
    v1.encode() == v2.encode()
}

/// `JSValue::strictEqualForCells(globalObject, JSCell*, JSCell*)`.
fn strict_equal_for_cells(v1: JSValue, v2: JSValue) -> bool {
    if v1.is_string() && v2.is_string() {
        return js_string_equal(&v1.as_js_string(), &v2.as_js_string());
    }
    if v1.is_big_int() && v2.is_big_int() {
        return big_int_strict_equals(v1, v2);
    }
    same_bits(v1, v2)
}

/// `JSValue::strictEqual(globalObject, v1, v2)`: o operador `===` (ECMA 11.9.3).
pub fn strict_equal(v1: JSValue, v2: JSValue) -> bool {
    if v1.is_int32() && v2.is_int32() {
        return same_bits(v1, v2);
    }

    if v1.is_number() && v2.is_number() {
        return v1.as_number() == v2.as_number();
    }

    if v1.is_cell() && v2.is_cell() {
        return strict_equal_for_cells(v1, v2);
    }

    same_bits(v1, v2)
}

/// `sameValue(globalObject, a, b)` (JSCJSValueInlines.h): https://tc39.github.io/ecma262/#sec-samevalue.
pub fn same_value(a: JSValue, b: JSValue) -> bool {
    if same_bits(a, b) {
        return true;
    }

    if !a.is_number() {
        return strict_equal(a, b);
    }
    if !b.is_number() {
        return false;
    }
    let x = a.as_number();
    let y = b.as_number();
    let x_is_nan = x.is_nan();
    let y_is_nan = y.is_nan();
    if x_is_nan || y_is_nan {
        return x_is_nan && y_is_nan;
    }
    x.to_bits() == y.to_bits()
}

/// `sameValueZero(globalObject, a, b)` (JSCJSValueInlines.h): como `strictEqual`, mas `NaN` é igual a `NaN`.
pub fn same_value_zero(a: JSValue, b: JSValue) -> bool {
    if a.is_number() && b.is_number() {
        let (x, y) = (a.as_number(), b.as_number());
        return x == y || (x.is_nan() && y.is_nan());
    }
    strict_equal(a, b)
}

/// `JSValue::equal(globalObject, v1, v2)`: o operador `==` (ECMA 11.9.3).
pub fn loose_equal(v1: JSValue, v2: JSValue) -> bool {
    if v1.is_int32() && v2.is_int32() {
        return same_bits(v1, v2);
    }

    equal_slow_case(v1, v2)
}

/// `JSValue::equalSlowCase(globalObject, v1, v2)` / `equalSlowCaseInline`. `false` com a exceção
/// pendente. A igualdade que envolve `JSBigInt` (`JSBigInt::equals`, `equalsToNumber`, `stringToBigInt`)
/// ainda não está ligada ao `JSValue`.
pub fn equal_slow_case(v1: JSValue, v2: JSValue) -> bool {
    let (mut v1, mut v2) = (v1, v2);

    loop {
        if v1.is_number() {
            if v2.is_number() {
                return v1.as_number() == v2.as_number();
            }
            // Guaranteeing that if we have a number it is v2 makes some of the cases below simpler.
            std::mem::swap(&mut v1, &mut v2);
        }

        // This deals with Booleans, BigInt32, Objects, and is a shortcut for a few more types.
        // It has to come here and not before, because it is NOT true that NaN == NaN
        if same_bits(v1, v2) {
            return true;
        }

        if v1.is_undefined_or_null() {
            if v2.is_undefined_or_null() {
                return true;
            }
            if !v2.is_cell() {
                return false;
            }
            return cell_masquerades_as_undefined(&v2);
        }

        if v2.is_undefined_or_null() {
            if !v1.is_cell() {
                return false;
            }
            return cell_masquerades_as_undefined(&v1);
        }

        if v1.is_object() {
            if v2.is_object() {
                return false; // v1 == v2 is already dealt with previously
            }
            let p1 = v1.to_primitive();
            if p1.is_empty() {
                return false;
            }
            v1 = p1;
            if v1.is_int32() && v2.is_int32() {
                return same_bits(v1, v2);
            }
            continue;
        }

        if v2.is_object() {
            let p2 = v2.to_primitive();
            if p2.is_empty() {
                return false;
            }
            v2 = p2;
            if v1.is_int32() && v2.is_int32() {
                return same_bits(v1, v2);
            }
            continue;
        }

        if v1.is_symbol() || v2.is_symbol() {
            return false; // v1 == v2 is already dealt with previously
        }

        let s1 = v1.is_string();
        let s2 = v2.is_string();
        if s1 {
            if s2 {
                return js_string_equal(&v1.as_js_string(), &v2.as_js_string());
            }
            std::mem::swap(&mut v1, &mut v2);
            // We are guaranteed to enter the next case, so losing the invariant of only v2 being a number is fine
        }
        if s1 || s2 {
            // We are guaranteed that the string is v2 (thanks to the swap above)
            if v1.is_big_int() {
                return big_int_equals_string(v1, v2);
            }
            debug_assert!(v1.is_number() || v1.is_boolean());
            let d1 = v1.to_number();
            let d2 = v2.to_number();
            return d1 == d2;
        }

        if v1.is_boolean() {
            if v2.is_number() {
                return (v1.as_boolean() as i32 as f64) == v2.as_number();
            }
            v1 = js_number(v1.to_number());
            // We fallthrough to the BigInt/Number comparison below
            // We just need one more swap to repair the rule that only v2 is allowed to be a number in these comparisons
            std::mem::swap(&mut v1, &mut v2);
        } else if v2.is_boolean() {
            v2 = js_number(v2.to_number());
            // We fallthrough to the BigInt/Number comparison below
        }

        if v1.is_big_int() {
            return big_int_equals_big_int_or_number(v1, v2);
        }

        return false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::js_value::{js_boolean, js_null, js_undefined};

    fn string_value(vm: &VM, text: &str) -> JSValue {
        JSValue::from_js_string(js_string(vm, &WtfString::from_latin1(text.as_bytes())))
    }

    fn text_of(value: JSValue) -> Vec<u8> {
        assert!(value.is_string());
        value.as_js_string().value().latin1()
    }

    #[test]
    fn add_numbers() {
        let vm = VM::new();
        assert_eq!(js_add(&vm, JSValue::Int32(1), JSValue::Int32(1)), Some(JSValue::Int32(2)));
        // 2^31 não cabe em int32: o resultado vira double.
        assert_eq!(
            js_add(&vm, JSValue::Int32(i32::MAX), JSValue::Int32(1)),
            Some(JSValue::Double(2147483648.0))
        );
        assert_eq!(js_add(&vm, JSValue::Double(0.5), JSValue::Double(0.5)), Some(JSValue::Int32(1)));
        assert_eq!(
            js_add(&vm, JSValue::Double(0.1), JSValue::Double(0.2)),
            Some(JSValue::Double(0.30000000000000004))
        );
        // -0 + -0 é -0 e jsNumber mantém double (não é int32 estrito).
        let negative_zero = js_add(&vm, JSValue::Double(-0.0), JSValue::Double(-0.0)).unwrap();
        assert!(negative_zero.is_double() && negative_zero.as_double().is_sign_negative());
        // -0 + 0 é +0, que é int32.
        assert_eq!(js_add(&vm, JSValue::Double(-0.0), JSValue::Int32(0)), Some(JSValue::Int32(0)));
    }

    #[test]
    fn add_non_numbers() {
        let vm = VM::new();
        // null + 1: toNumeric(null) é 0.
        assert_eq!(js_add(&vm, js_null(), JSValue::Int32(1)), Some(JSValue::Int32(1)));
        assert_eq!(js_add(&vm, js_boolean(true), JSValue::Int32(1)), Some(JSValue::Int32(2)));
        assert_eq!(js_add(&vm, js_boolean(true), js_boolean(true)), Some(JSValue::Int32(2)));
        assert!(js_add(&vm, js_undefined(), JSValue::Int32(1)).unwrap().as_double().is_nan());
        // "a" + 1, 1 + "a", "a" + "b" e as conversões do operando não string.
        let a = string_value(&vm, "a");
        assert_eq!(text_of(js_add(&vm, a, JSValue::Int32(1)).unwrap()), b"a1");
        assert_eq!(text_of(js_add(&vm, JSValue::Int32(1), a).unwrap()), b"1a");
        assert_eq!(text_of(js_add(&vm, a, string_value(&vm, "b")).unwrap()), b"ab");
        assert_eq!(text_of(js_add(&vm, a, js_undefined()).unwrap()), b"aundefined");
        assert_eq!(text_of(js_add(&vm, js_null(), a).unwrap()), b"nulla");
        assert_eq!(text_of(js_add(&vm, a, js_boolean(false)).unwrap()), b"afalse");
        assert_eq!(text_of(js_add(&vm, a, JSValue::Double(-0.0)).unwrap()), b"a0");
        assert_eq!(text_of(js_add(&vm, a, JSValue::Double(1.5)).unwrap()), b"a1.5");
        assert_eq!(text_of(js_add(&vm, a, JSValue::nan()).unwrap()), b"aNaN");
        // Operando vazio.
        let empty = string_value(&vm, "");
        assert_eq!(text_of(js_add(&vm, empty, a).unwrap()), b"a");
        assert_eq!(text_of(js_add(&vm, a, empty).unwrap()), b"a");
        assert_eq!(text_of(js_add(&vm, empty, JSValue::Int32(7)).unwrap()), b"7");
    }

    #[test]
    fn concat_widths() {
        let vm = VM::new();
        let wide = WtfString::from_utf16(&[0x20ac, 0x61]);
        let narrow = WtfString::from_latin1(&[0xe9, 0x62]);
        let result = js_string_concat(&vm, &narrow, &wide).unwrap().value();
        assert!(!result.is_8bit());
        assert_eq!(result.span16(), &[0xe9, 0x62, 0x20ac, 0x61]);
        let result = js_string_concat(&vm, &wide, &narrow).unwrap().value();
        assert_eq!(result.span16(), &[0x20ac, 0x61, 0xe9, 0x62]);
        let result = js_string_concat(&vm, &narrow, &narrow).unwrap().value();
        assert!(result.is_8bit());
        assert_eq!(result.span8(), &[0xe9, 0x62, 0xe9, 0x62]);
    }

    #[test]
    fn arithmetic() {
        let vm = VM::new();
        assert_eq!(js_sub(JSValue::Int32(5), JSValue::Int32(7)), JSValue::Int32(-2));
        assert_eq!(js_sub(string_value(&vm, "1"), JSValue::Int32(1)), JSValue::Int32(0));
        assert_eq!(js_sub(string_value(&vm, "0x10"), string_value(&vm, " 6 ")), JSValue::Int32(10));
        assert!(js_sub(string_value(&vm, "x"), JSValue::Int32(1)).as_double().is_nan());
        assert!(js_sub(js_undefined(), JSValue::Int32(1)).as_double().is_nan());
        assert_eq!(js_sub(js_null(), JSValue::Int32(1)), JSValue::Int32(-1));
        assert_eq!(js_sub(js_boolean(true), js_boolean(false)), JSValue::Int32(1));
        // 0 - 0.0 é +0; -0 - 0 é -0.
        assert_eq!(js_sub(JSValue::Int32(0), JSValue::Double(0.0)), JSValue::Int32(0));
        let negative_zero = js_sub(JSValue::Double(-0.0), JSValue::Int32(0));
        assert!(negative_zero.is_double() && negative_zero.as_double().is_sign_negative());
        assert_eq!(js_mul(string_value(&vm, "3"), string_value(&vm, "4")), JSValue::Int32(12));
        assert_eq!(js_div(JSValue::Int32(1), JSValue::Int32(0)), JSValue::Double(f64::INFINITY));
        assert_eq!(js_div(JSValue::Int32(1), JSValue::Int32(4)), JSValue::Double(0.25));
        assert_eq!(js_remainder(JSValue::Int32(5), JSValue::Int32(3)), JSValue::Int32(2));
        assert_eq!(js_remainder(JSValue::Int32(-5), JSValue::Int32(3)), JSValue::Int32(-2));
        // fmod(-4, 2) é -0.
        let negative_zero = js_remainder(JSValue::Int32(-4), JSValue::Int32(2));
        assert!(negative_zero.is_double() && negative_zero.as_double().is_sign_negative());
        assert!(js_remainder(JSValue::Int32(1), JSValue::Int32(0)).as_double().is_nan());
        assert_eq!(js_inc(string_value(&vm, "5")), JSValue::Int32(6));
        assert_eq!(js_inc(js_null()), JSValue::Int32(1));
        assert_eq!(js_dec(JSValue::Int32(i32::MIN)), JSValue::Double(-2147483649.0));
        assert_eq!(js_dec(js_boolean(true)), JSValue::Int32(0));
    }

    #[test]
    fn less_than() {
        let vm = VM::new();
        let s = |text: &str| string_value(&vm, text);
        assert!(js_less::<true>(JSValue::Int32(1), JSValue::Int32(2)));
        assert!(!js_less::<true>(JSValue::Int32(2), JSValue::Int32(2)));
        assert!(js_less::<true>(JSValue::Int32(1), JSValue::Double(1.5)));
        assert!(!js_less::<true>(JSValue::nan(), JSValue::Int32(1)));
        assert!(!js_less::<true>(JSValue::Int32(1), JSValue::nan()));
        // Duas strings: ordem por unidade de código ("B" é 66, "a" é 97; "10" < "9").
        assert!(js_less::<true>(s("a"), s("b")));
        assert!(!js_less::<true>(s("a"), s("B")));
        assert!(js_less::<true>(s("10"), s("9")));
        assert!(js_less::<true>(s("a"), s("ab")));
        assert!(!js_less::<true>(s("ab"), s("a")));
        // Uma string e um número: comparação numérica.
        assert!(!js_less::<true>(s("10"), JSValue::Int32(9)));
        assert!(js_less::<true>(s("9"), JSValue::Int32(10)));
        assert!(!js_less::<true>(s("x"), JSValue::Int32(1)));
        assert!(!js_less::<true>(js_undefined(), JSValue::Int32(1)));
        assert!(js_less::<true>(js_null(), JSValue::Int32(1)));
        assert!(js_less::<false>(js_null(), JSValue::Int32(1)));
        assert!(js_less::<true>(js_boolean(false), js_boolean(true)));
        assert!(js_less::<true>(JSValue::Double(-0.0), JSValue::Double(0.5)));
        assert!(!js_less::<true>(JSValue::Double(-0.0), JSValue::Int32(0)));
    }

    #[test]
    fn less_than_or_equal() {
        let vm = VM::new();
        let s = |text: &str| string_value(&vm, text);
        assert!(js_less_eq::<true>(JSValue::Int32(2), JSValue::Int32(2)));
        assert!(!js_less_eq::<true>(JSValue::Int32(3), JSValue::Int32(2)));
        assert!(!js_less_eq::<true>(JSValue::nan(), JSValue::nan()));
        assert!(js_less_eq::<true>(JSValue::Double(-0.0), JSValue::Int32(0)));
        assert!(js_less_eq::<true>(s("a"), s("a")));
        assert!(js_less_eq::<true>(s("a"), s("b")));
        assert!(!js_less_eq::<true>(s("b"), s("a")));
        assert!(js_less_eq::<false>(js_null(), JSValue::Int32(0)));
        assert!(!js_less_eq::<true>(js_undefined(), JSValue::Int32(0)));
        assert!(js_less_eq::<true>(s("5"), JSValue::Int32(5)));
    }

    /// Um reino novo, o corrente até o fim do teste: o que as operações com `BigInt` e as que lançam
    /// precisam (`current_global_object`).
    fn realm() -> (crate::runtime::js_global_object::JSGlobalObjectRef, crate::runtime::current_realm::CurrentRealmScope) {
        use crate::runtime::js_global_object::JSGlobalObject;
        let vm = std::rc::Rc::new(VM::new());
        let structure = JSGlobalObject::create_structure(&vm, js_null());
        let global_object = JSGlobalObject::create(&vm, structure, js_null());
        let scope = crate::runtime::current_realm::CurrentRealmScope::enter(&global_object);
        (global_object, scope)
    }

    /// A mensagem da exceção pendente, que a leitura limpa.
    fn take_thrown_message(vm: &VM) -> Vec<u8> {
        use crate::runtime::error_instance::ErrorInstance;
        let exception = vm.exception().expect("exceção pendente");
        let error = ErrorInstance::from_cell_id(exception.value().as_cell()).expect("ErrorInstance");
        let message = error.message().latin1();
        vm.clear_exception();
        message
    }

    fn big(value: i64) -> JSValue {
        crate::runtime::js_big_int_ops::make_big_int_from_i64(value)
    }

    fn big_text(vm: &VM, value: JSValue) -> Vec<u8> {
        assert!(value.is_big_int());
        value.to_string(vm).value().latin1()
    }

    #[test]
    fn arithmetic_with_big_int() {
        let (global_object, _scope) = realm();
        let vm = global_object.vm();
        assert_eq!(big_text(vm, js_sub(big(5), big(7))), b"-2");
        assert_eq!(big_text(vm, js_mul(big(6), big(7))), b"42");
        assert_eq!(big_text(vm, js_div(big(7), big(2))), b"3");
        assert_eq!(big_text(vm, js_remainder(big(-7), big(3))), b"-1");
        assert_eq!(big_text(vm, js_add_slow_case(vm, big(2), big(3)).unwrap()), b"5");
        assert_eq!(big_text(vm, js_inc(big(9))), b"10");
        assert_eq!(big_text(vm, js_dec(big(0))), b"-1");
        assert_eq!(big_text(vm, crate::runtime::operations_bitwise::js_pow(big(2), big(70))), b"1180591620717411303424");
        // BigInt com string e com operando objeto-primitivo vira concatenação, não aritmética.
        let text = string_value(vm, "x");
        assert_eq!(text_of(js_add(vm, text, big(12)).unwrap()), b"x12");
        assert_eq!(text_of(js_add(vm, big(12), text).unwrap()), b"12x");
    }

    #[test]
    fn mixing_big_int_with_other_types_throws() {
        let (global_object, _scope) = realm();
        let vm = global_object.vm();
        let cases: [(&dyn Fn() -> JSValue, &[u8]); 7] = [
            (&|| js_sub(big(1), JSValue::Int32(1)), b"Invalid mix of BigInt and other type in subtraction."),
            (&|| js_mul(JSValue::Int32(1), big(1)), b"Invalid mix of BigInt and other type in multiplication."),
            (&|| js_div(big(1), JSValue::Double(0.5)), b"Invalid mix of BigInt and other type in division."),
            (&|| js_remainder(big(1), js_boolean(true)), b"Invalid mix of BigInt and other type in remainder."),
            (
                &|| crate::runtime::operations_bitwise::js_pow(js_null(), big(1)),
                b"Invalid mix of BigInt and other type in exponentiation.",
            ),
            (&|| js_add(vm, big(1), JSValue::Int32(1)).unwrap(), b"Invalid mix of BigInt and other type in addition."),
            // `undefined + 1n`: toNumeric(undefined) é NaN, um número, então também mistura.
            (&|| js_add(vm, js_undefined(), big(1)).unwrap(), b"Invalid mix of BigInt and other type in addition."),
        ];
        for (operation, message) in cases {
            assert!(operation().is_empty());
            assert_eq!(take_thrown_message(vm), message);
        }
    }

    #[test]
    fn to_number_of_big_int_throws() {
        let (global_object, _scope) = realm();
        let vm = global_object.vm();
        assert_eq!(big(3).to_number(), 0.0);
        assert_eq!(take_thrown_message(vm), b"Conversion from 'BigInt' to 'number' is not allowed.");
        // `toNumeric` mantém o BigInt, e `toString` o descreve em decimal.
        assert!(big(3).to_numeric().is_big_int());
        assert_eq!(big_text(vm, big(-30)), b"-30");
    }

    #[test]
    fn comparisons_with_big_int() {
        let (global_object, _scope) = realm();
        let vm = global_object.vm();
        let s = |text: &str| string_value(vm, text);
        assert!(js_less::<true>(big(1), JSValue::Int32(2)));
        assert!(!js_less::<true>(big(2), JSValue::Int32(2)));
        assert!(js_less_eq::<true>(big(2), JSValue::Double(2.0)));
        assert!(js_less::<true>(JSValue::Double(1.5), big(2)));
        assert!(!js_less::<true>(big(2), JSValue::Double(1.5)));
        assert!(!js_less::<true>(big(1), JSValue::nan()));
        assert!(!js_less_eq::<true>(JSValue::nan(), big(1)));
        assert!(js_less::<true>(big(1), JSValue::Double(f64::INFINITY)));
        assert!(js_less::<false>(JSValue::Double(f64::NEG_INFINITY), big(1)));
        // BigInt e string: a string vira BigInt (StringToBigInt); se não é um, a comparação é indefinida (false).
        assert!(js_less::<true>(big(2), s("3")));
        assert!(js_less::<true>(s("1"), big(2)));
        assert!(js_less_eq::<true>(big(3), s(" 3 ")));
        assert!(!js_less::<true>(big(1), s("x")));
        assert!(!js_less_eq::<true>(s("x"), big(1)));
        assert!(js_less::<true>(big(-1), big(0)));
        assert!(js_less_eq::<false>(big(7), big(7)));
        assert!(js_less::<true>(js_boolean(false), big(1)));
        assert!(js_less::<true>(js_null(), big(1)));
        assert!(vm.exception().is_none());
    }

    #[test]
    fn equality_with_big_int() {
        let (global_object, _scope) = realm();
        let vm = global_object.vm();
        let s = |text: &str| string_value(vm, text);
        assert!(loose_equal(big(1), JSValue::Int32(1)));
        assert!(loose_equal(JSValue::Double(1.0), big(1)));
        assert!(!loose_equal(big(1), JSValue::Double(1.5)));
        assert!(!loose_equal(big(1), JSValue::nan()));
        assert!(loose_equal(big(1), s("1")));
        assert!(loose_equal(s("0x10"), big(16)));
        assert!(!loose_equal(big(1), s("x")));
        assert!(loose_equal(big(1), js_boolean(true)));
        assert!(loose_equal(js_boolean(false), big(0)));
        assert!(!loose_equal(big(1), js_null()));
        assert!(!loose_equal(big(0), js_undefined()));
        assert!(loose_equal(big(5), big(5)));
        assert!(!strict_equal(big(1), JSValue::Int32(1)));
        assert!(strict_equal(big(9), big(9)));
        assert!(!strict_equal(big(9), big(8)));
        assert!(same_value(big(9), big(9)));
        assert!(same_value_zero(big(9), big(9)));
        assert!(vm.exception().is_none());
    }

    #[test]
    fn strict_equality() {
        let vm = VM::new();
        let s = |text: &str| string_value(&vm, text);
        assert!(strict_equal(JSValue::Int32(1), JSValue::Int32(1)));
        assert!(strict_equal(JSValue::Int32(1), JSValue::Double(1.0)));
        assert!(!strict_equal(JSValue::nan(), JSValue::nan()));
        assert!(strict_equal(JSValue::Double(-0.0), JSValue::Int32(0)));
        assert!(strict_equal(JSValue::Double(-0.0), JSValue::Double(0.0)));
        // Duas JSString distintas com o mesmo texto.
        assert!(strict_equal(s("abc"), s("abc")));
        assert!(!strict_equal(s("abc"), s("abd")));
        assert!(!strict_equal(s("abc"), s("ab")));
        assert!(!strict_equal(JSValue::Int32(1), s("1")));
        assert!(!strict_equal(js_null(), js_undefined()));
        assert!(strict_equal(js_null(), js_null()));
        assert!(strict_equal(js_undefined(), js_undefined()));
        assert!(strict_equal(js_boolean(true), js_boolean(true)));
        assert!(!strict_equal(js_boolean(true), JSValue::Int32(1)));
        assert!(!strict_equal(js_boolean(false), js_null()));
    }

    #[test]
    fn loose_equality() {
        let vm = VM::new();
        let s = |text: &str| string_value(&vm, text);
        assert!(loose_equal(JSValue::Int32(1), JSValue::Int32(1)));
        assert!(!loose_equal(JSValue::Int32(1), JSValue::Int32(2)));
        assert!(loose_equal(JSValue::Int32(1), JSValue::Double(1.0)));
        assert!(!loose_equal(JSValue::nan(), JSValue::nan()));
        assert!(loose_equal(JSValue::Double(-0.0), JSValue::Int32(0)));
        // undefined e null só são iguais entre si.
        assert!(loose_equal(js_null(), js_undefined()));
        assert!(loose_equal(js_undefined(), js_undefined()));
        assert!(!loose_equal(js_null(), JSValue::Int32(0)));
        assert!(!loose_equal(js_undefined(), JSValue::Int32(0)));
        assert!(!loose_equal(js_null(), js_boolean(false)));
        assert!(!loose_equal(js_null(), s("")));
        assert!(!loose_equal(s("null"), js_null()));
        assert!(!loose_equal(js_undefined(), s("undefined")));
        // string com string, com número e com booleano.
        assert!(loose_equal(s("a"), s("a")));
        assert!(!loose_equal(s("a"), s("b")));
        assert!(loose_equal(s("1"), JSValue::Int32(1)));
        assert!(loose_equal(JSValue::Int32(1), s("1")));
        assert!(loose_equal(s(""), JSValue::Int32(0)));
        assert!(loose_equal(JSValue::Int32(0), s("")));
        assert!(loose_equal(s(" 0x10 "), JSValue::Int32(16)));
        assert!(!loose_equal(s("x"), JSValue::nan()));
        assert!(loose_equal(s("0"), js_boolean(false)));
        assert!(loose_equal(js_boolean(true), s("1")));
        assert!(!loose_equal(js_boolean(true), s("2")));
        assert!(loose_equal(js_boolean(true), JSValue::Int32(1)));
        assert!(loose_equal(JSValue::Int32(0), js_boolean(false)));
        assert!(loose_equal(JSValue::Double(1.0), js_boolean(true)));
        assert!(!loose_equal(JSValue::Int32(2), js_boolean(true)));
        assert!(loose_equal(js_boolean(true), js_boolean(true)));
        assert!(!loose_equal(js_boolean(true), js_boolean(false)));
    }
}
