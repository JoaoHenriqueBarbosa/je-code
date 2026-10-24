//! Porte de `runtime/NumberPrototype.h`, `NumberPrototypeInlines.h` e `NumberPrototype.cpp`: o
//! `Number.prototype` (um `NumberObject` com valor interno 0), `toString` com `radix`, `toFixed`,
//! `toExponential`, `toPrecision`, `valueOf` e `toLocaleString`, mais `toStringWithRadix`,
//! `numberToString` e `extractToStringRadixArgument`.
//!
//! DIVERGÊNCIAS e lacunas:
//!
//! - `numberPrototypeTable` (`toLocaleString`, `valueOf`, `toFixed`, `toExponential`, `toPrecision`) fica
//!   no `ClassInfo` e a `Structure` leva `HasStaticPropertyTable`: as cinco reificam no primeiro acesso.
//!   `toString` (`numberProtoToStringFunction`) segue eager no `finishCreation`, como no C++.
//! - `installNumberPrototypeWatchpoint` não existe (o `JSGlobalObject` do porte não tem os conjuntos de
//!   watchpoint).
//! - `toLocaleString` passa por `intl_number_format::to_locale_string`: sem `locales` nem `options` é o
//!   `defaultNumberFormat()` do locale padrão (en-US); com eles, o `IntlNumberFormat` de
//!   `intl_number_format.rs` (locales cobertas: en e pt-BR).
//! - `NumericStrings` (o cache de `JSString` por inteiro e por `double`) não tem efeito observável e
//!   não é portado; `int32ToString`, `int52ToString` e `numberToString` criam a `JSString` a cada
//!   chamada. `int32ToString` e `int52ToString` só servem ao JIT e não são portados; `numberToString`
//!   escreve o mesmo texto que `numberToStringInternal` (`to_string_with_radix`).

use std::cell::Cell;

use crate::runtime::big_integer::BigInteger;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::default_number_format::NumericInput;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::intl_number_format::to_locale_string;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::put_direct_native_function_without_transition;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_string::{js_string, JSStringRef};
use crate::runtime::js_type::JSType;
use crate::runtime::js_typeof::js_type_string_for_value;
use crate::runtime::js_value::{js_number, js_number_i32, JSValue};
use crate::runtime::js_wrapper_object::{JSWrapperObject, JSWrapperObjectRef};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::number_object::{NumberObject, NUMBER_OBJECT_S_INFO};
use crate::runtime::parse_int::RADIX_DIGITS;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::lookup::{native_entry};
use crate::runtime::property_attribute::{DONT_ENUM, FUNCTION};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::uint16_with_fraction::Uint16WithFraction;
use crate::runtime::vm::VM;
use crate::wtf::dragonbox::dragonbox_to_chars;
use crate::wtf::dtoa::double_conversion::DoubleToStringConverter;
use crate::wtf::dtoa::utils::StringBuilder;
use crate::wtf::dtoa::{number_to_fixed_precision_string, number_to_fixed_width_string, NumberToStringBuffer};
use crate::wtf::math_extras::truncate_double_to_int32;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo NumberPrototype::s_info`.
pub static NUMBER_PROTOTYPE_S_INFO: ClassInfo = ClassInfo {
    class_name: "Number",
    parent_class: Some(&NUMBER_OBJECT_S_INFO),
    static_prop_hash_table: Some(&NUMBER_PROTOTYPE_TABLE),
    inherits_js_type_range: None,
};

/// `numberPrototypeTableValues` de `NumberPrototype.lut.h`, na ordem do `@begin`.
static NUMBER_PROTOTYPE_TABLE_VALUES: [HashTableValue; 5] = [
    native_entry("toLocaleString", number_proto_host_to_locale_string, 0),
    native_entry("valueOf", number_proto_host_value_of, 0),
    native_entry("toFixed", number_proto_host_to_fixed, 1),
    native_entry("toExponential", number_proto_host_to_exponential, 1),
    native_entry("toPrecision", number_proto_host_to_precision, 1),
];

/// `numberPrototypeTable`.
static NUMBER_PROTOTYPE_TABLE: HashTable = HashTable { class_for_this: None, values: &NUMBER_PROTOTYPE_TABLE_VALUES };

/// `radixDigits[digit]`.
fn radix_digit(digit: u32) -> u8 {
    RADIX_DIGITS[digit as usize]
}

/// `int52ToStringWithRadix` e `toStringWithRadixInternal(int32_t, radix)`: os dois escrevem o inteiro
/// com sinal na base `radix` (o de 32 bits é este com o valor estendido).
fn integer_to_string_with_radix(value: i64, radix: u32) -> Vec<u8> {
    let mut positive_number = value.unsigned_abs();

    let mut digits = Vec::new();
    // Always loop at least once, to emit at least '0'.
    loop {
        digits.push(radix_digit((positive_number % radix as u64) as u32));
        positive_number /= radix as u64;
        if positive_number == 0 {
            break;
        }
    }
    if value < 0 {
        digits.push(b'-');
    }
    digits.reverse();
    digits
}

/// `toStringWithRadixInternal(buffer, originalNumber, radix)`: o número é finito e `radix` está em
/// [2, 36]. O texto é ASCII.
fn to_string_with_radix_internal(original_number: f64, radix: u32) -> Vec<u8> {
    debug_assert!(original_number.is_finite());
    debug_assert!((2..=36).contains(&radix));

    // Extract the sign.
    let is_negative = original_number < 0.0;
    let mut number = original_number;
    if original_number.is_sign_negative() {
        number = -original_number;
    }
    let mut integer_part = number.floor();

    // O texto depois da parte inteira: o ponto e os dígitos da fração, quando há.
    let mut fraction_text: Vec<u8> = Vec::new();

    // Check if the value has a fractional part to convert.
    let fraction_part = number - integer_part;
    if fraction_part == 0.0 {
        // We do not need to care the negative zero (-0) since it is also converted to "0" in all the radix.
        // (`numberOfInt52Bits - 1` é 51.)
        if integer_part < (1i64 << 51) as f64 {
            return integer_to_string_with_radix(original_number as i64, radix);
        }
    } else {
        // We use this to test for odd values in odd radix bases.
        // Where the base is even, (e.g. 10), to determine whether a value is even we need only
        // consider the least significant digit. For example, 124 in base 10 is even, because '4'
        // is even. if the radix is odd, then the radix raised to an integer power is also odd.
        // E.g. in base 5, 124 represents (1 * 125 + 2 * 25 + 4 * 5). Since each digit in the value
        // is multiplied by an odd number, the result is even if the sum of all digits is even.
        //
        // For the integer portion of the result, we only need test whether the integer value is
        // even or odd. For each digit of the fraction added, we should invert our idea of whether
        // the number is odd if the new digit is odd.
        //
        // Also initialize digit to this value; for even radix values we only need track whether
        // the last individual digit was odd.
        let integer_part_is_odd = integer_part <= 0x1FFFFFFFFFFFFFu64 as f64 && (integer_part as i64) & 1 != 0;
        let mut is_odd_in_odd_radix = integer_part_is_odd;
        let mut digit = integer_part_is_odd as u32;

        // Write the decimal point now.
        fraction_text.push(b'.');

        // Higher precision representation of the fractional part.
        let mut fraction = Uint16WithFraction::new(fraction_part, 0);

        let mut needs_rounding_up = false;

        // Calculate the delta from the current number to the next & previous possible IEEE numbers.
        // (`nextafter` de um número finito positivo é somar ou subtrair 1 da codificação.)
        let next_number = f64::from_bits(number.to_bits() + 1);
        let last_number = f64::from_bits(number.to_bits() - 1);
        debug_assert!(next_number.is_finite() && !next_number.is_sign_negative());
        debug_assert!(last_number.is_finite() && !last_number.is_sign_negative());
        let delta_next_double = next_number - number;
        let delta_last_double = number - last_number;
        debug_assert!(delta_next_double.is_finite() && !delta_next_double.is_sign_negative());
        debug_assert!(delta_last_double.is_finite() && !delta_last_double.is_sign_negative());

        // We track the delta from the current value to the next, to track how many digits of the
        // fraction we need to write. For example, if the value we are converting is precisely
        // 1.2345, so far we have written the digits "1.23" to a string leaving a remainder of
        // 0.45, and we want to determine whether we can round off, or whether we need to keep
        // appending digits ('4'). We can stop adding digits provided that then next possible
        // lower IEEE value is further from 1.23 than the remainder we'd be rounding off (0.45),
        // which is to say, less than 1.2255. Put another way, the delta between the prior
        // possible value and this number must be more than 2x the remainder we'd be rounding off
        // (or more simply half the delta between numbers must be greater than the remainder).
        //
        // Similarly we need track the delta to the next possible value, to dertermine whether
        // to round up. In almost all cases (other than at exponent boundaries) the deltas to
        // prior and subsequent values are identical, so we don't need track then separately.
        // (O C++ tem um laço com um só `halfDelta` para o caso de deltas iguais, e outro idêntico com
        // `halfDeltaNext` e `halfDeltaLast`; sendo iguais os dois, o segundo faz o mesmo que o primeiro.)
        // Pre-multiply by 0.5.
        let mut half_delta_next = Uint16WithFraction::new(delta_next_double, 1);
        let mut half_delta_last = Uint16WithFraction::new(delta_last_double, 1);

        loop {
            // examine the remainder to determine whether we should be considering rounding
            // up or down. If remainder is precisely 0.5 rounding is to even.
            let d_compare_point5 = fraction.compare_point5();
            let is_odd = if radix & 1 != 0 { is_odd_in_odd_radix } else { digit & 1 != 0 };
            if d_compare_point5 > 0 || (d_compare_point5 == 0 && is_odd) {
                // Check for rounding up; are we closer to the value we'd round off to than
                // the next IEEE value would be?
                if fraction.sum_greater_than_one(&half_delta_next) {
                    needs_rounding_up = true;
                    break;
                }
            } else if fraction.less_than(&half_delta_last) {
                // Check for rounding down; are we closer to the value we'd round off to than
                // the prior IEEE value would be?
                break;
            }

            // Write a digit to the string.
            fraction.multiply_assign(radix as u16);
            digit = fraction.floor_and_subtract();
            fraction_text.push(radix_digit(digit));
            // Keep track whether the portion written is currently even, if the radix is odd.
            if digit & 1 != 0 {
                is_odd_in_odd_radix = !is_odd_in_odd_radix;
            }

            // Shift the fractions by radix.
            half_delta_next.multiply_assign(radix as u16);
            half_delta_last.multiply_assign(radix as u16);
        }

        // Check if the fraction needs rounding off (flag set in the loop writing digits, above).
        if needs_rounding_up {
            // Whilst the last digit is the maximum in the current radix, remove it.
            // e.g. rounding up the last digit in "12.3999" is the same as rounding up the
            // last digit in "12.3" - both round up to "12.4".
            while fraction_text.last() == Some(&radix_digit(radix - 1)) {
                fraction_text.pop();
            }

            // Radix digits are sequential in ascii/unicode, except for '9' and 'a'.
            // E.g. the first 'if' case handles rounding 67.89 to 67.8a in base 16.
            // The 'else if' case handles rounding of all other digits.
            match fraction_text.last_mut() {
                Some(last) if *last == b'9' => *last = b'a',
                Some(last) if *last != b'.' => *last += 1,
                _ => {
                    // One other possibility - there may be no digits to round up in the fraction
                    // (or all may be been rounded off already), in which case we may need to
                    // round into the integer portion of the number. Remove the decimal point.
                    fraction_text.pop();
                    // In order to get here there must have been a non-zero fraction, in which case
                    // there must be at least one bit of the value's mantissa not in use in the
                    // integer part of the number. As such, adding to the integer part should not
                    // be able to lose precision.
                    debug_assert!((integer_part + 1.0) - integer_part == 1.0);
                    integer_part += 1.0;
                }
            }
        } else {
            // We only need to check for trailing zeros if the value does not get rounded up.
            while fraction_text.last() == Some(&b'0') {
                fraction_text.pop();
            }
        }
    }

    let mut units = BigInteger::new(integer_part);

    let mut integer_text = Vec::new();
    // Always loop at least once, to emit at least '0'.
    loop {
        // Read a single digit and write it to the front of the string.
        // Divide by radix to remove one digit from the value.
        let digit = units.divide(radix);
        integer_text.push(radix_digit(digit));
        if units.is_zero() {
            break;
        }
    }

    // If the number is negative, prepend '-'.
    if is_negative {
        integer_text.push(b'-');
    }
    integer_text.reverse();
    integer_text.extend_from_slice(&fraction_text);
    integer_text
}

/// `toStringWithRadix(doubleValue, radix)`.
pub fn to_string_with_radix(double_value: f64, radix: i32) -> WtfString {
    debug_assert!((2..=36).contains(&radix));

    let integer_value = truncate_double_to_int32(double_value);
    if integer_value as f64 == double_value {
        return WtfString::from_latin1(&integer_to_string_with_radix(integer_value as i64, radix as u32));
    }

    if radix == 10 || !double_value.is_finite() {
        return WtfString::number_f64(double_value);
    }

    if V8_RADIX_STRINGS.with(Cell::get) {
        return WtfString::from_latin1(&double_to_radix_c_string(double_value, radix as u32));
    }
    WtfString::from_latin1(&to_string_with_radix_internal(double_value, radix as u32))
}

thread_local! {
    /// O programa é um navegador: `toString(radix)` de número não inteiro segue o V8, não o JSC.
    static V8_RADIX_STRINGS: Cell<bool> = const { Cell::new(false) };
}

/// Liga (o navegador) ou desliga o `toString(radix)` do V8 para o programa em curso.
pub fn set_v8_radix_strings(enabled: bool) {
    V8_RADIX_STRINGS.with(|flag| flag.set(enabled));
}

/// Fim do programa: volta ao JSC.
pub(crate) fn reset_for_program() {
    let _ = V8_RADIX_STRINGS.try_with(|flag| flag.set(false));
}

/// `Double(value).Exponent()` do V8: o expoente com a mantissa de 53 bits tomada como inteiro.
fn v8_double_exponent(value: f64) -> i32 {
    let biased = ((value.to_bits() >> 52) & 0x7FF) as i32;
    if biased == 0 { 1 - 0x3FF - 52 } else { biased - 0x3FF - 52 }
}

/// `DoubleToRadixCString(value, radix)` do V8 (`src/numbers/conversions.cc`): dígitos da fração até a precisão do
/// `double` (meio ulp) e a cauda arredondada para o par com propagação do vai-um; a parte inteira por divisões. O
/// número é finito e não é inteiro de 32 bits.
fn double_to_radix_c_string(value: f64, radix: u32) -> Vec<u8> {
    let negative = value < 0.0;
    let value = value.abs();
    let mut integer = value.floor();
    let mut fraction = value - integer;
    let next_up = |x: f64| f64::from_bits(x.to_bits() + 1);
    let mut delta = (0.5 * (next_up(value) - value)).max(next_up(0.0));
    let mut fraction_text: Vec<u8> = Vec::new();
    if fraction >= delta {
        loop {
            fraction *= radix as f64;
            delta *= radix as f64;
            let digit = fraction as u32;
            fraction_text.push(RADIX_DIGITS[digit as usize]);
            fraction -= digit as f64;
            if (fraction > 0.5 || (fraction == 0.5 && digit & 1 == 1)) && fraction + delta > 1.0 {
                // Arredonda para cima, propagando o vai-um; se passar da vírgula, soma à parte inteira.
                loop {
                    let Some(last) = fraction_text.pop() else {
                        integer += 1.0;
                        break;
                    };
                    let digit = RADIX_DIGITS.iter().position(|&c| c == last).unwrap_or(0) as u32;
                    if digit + 1 < radix {
                        fraction_text.push(RADIX_DIGITS[digit as usize + 1]);
                        break;
                    }
                }
                break;
            }
            if fraction < delta {
                break;
            }
        }
    }
    let mut integer_text: Vec<u8> = Vec::new();
    while v8_double_exponent(integer / radix as f64) > 0 {
        integer /= radix as f64;
        integer_text.push(b'0');
    }
    loop {
        let remainder = integer % radix as f64;
        integer_text.push(RADIX_DIGITS[remainder as usize]);
        integer = (integer - remainder) / radix as f64;
        if integer <= 0.0 {
            break;
        }
    }
    if negative {
        integer_text.push(b'-');
    }
    integer_text.reverse();
    if !fraction_text.is_empty() {
        integer_text.push(b'.');
        integer_text.extend_from_slice(&fraction_text);
    }
    integer_text
}

/// `numberToString(vm, doubleValue, radix)`.
pub fn number_to_string(vm: &VM, double_value: f64, radix: i32) -> JSStringRef {
    js_string(vm, &to_string_with_radix(double_value, radix))
}

/// `extractToStringRadixArgument(globalObject, radixValue, throwScope)`.
pub fn extract_to_string_radix_argument(radix_value: JSValue) -> Result<i32, Thrown> {
    if radix_value.is_undefined() {
        return Ok(10);
    }

    if radix_value.is_int32() {
        let radix = radix_value.as_int32();
        if (2..=36).contains(&radix) {
            return Ok(radix);
        }
    } else {
        // Um Symbol ou objeto cuja conversão lança deixa a exceção pendente: ela vence o RangeError.
        let radix_double = radix_value.to_integer_or_infinity_checked()?;
        if (2.0..=36.0).contains(&radix_double) {
            return Ok(radix_double as i32);
        }
    }

    Err(Thrown::range_error("toString() radix argument must be between 2 and 36"))
}

/// `toThisNumber(thisValue, x)` e `throwVMToThisNumberError`: o número de `this`, ou o `TypeError`.
fn this_number_value(call: &HostCall) -> Result<f64, Thrown> {
    let this_value = call.this_value();
    if this_value.is_number() {
        return Ok(this_value.as_number());
    }

    if let Some(number_object) = NumberObject::from_value(&this_value) {
        return Ok(number_object.internal_value().as_number());
    }

    Err(Thrown::type_error(&format!("thisNumberValue called on incompatible {}", js_type_string_for_value(this_value))))
}

/// `jsString(vm, String(text))` como valor, para o texto ASCII que a WTF produz.
fn ascii_string_value(vm: &VM, text: &[u8]) -> JSValue {
    JSValue::from_js_string(js_string(vm, &WtfString::from_latin1(text)))
}

/// `jsString(vm, String::number(x))` como valor.
fn number_string_value(vm: &VM, x: f64) -> JSValue {
    JSValue::from_js_string(js_string(vm, &WtfString::number_f64(x)))
}

// toExponential converts a number to a string, always formatting as an exponential.
// This method takes an optional argument specifying a number of *decimal places*
// to round the significand to (or, put another way, this method optionally rounds
// to argument-plus-one significant figures).
fn number_proto_func_to_exponential(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let x = this_number_value(call)?;

    let arg = call.argument(0);
    // Perform ToInteger on the argument before remaining steps (RETURN_IF_EXCEPTION: Symbol e getter que lança).
    let decimal_places_double = arg.to_integer_or_infinity_checked()?;

    // Handle NaN and Infinity.
    if !x.is_finite() {
        return Ok(number_string_value(vm, x));
    }

    if !(0.0..=100.0).contains(&decimal_places_double) {
        return Err(Thrown::range_error("toExponential() argument must be between 0 and 100"));
    }
    let decimal_places = decimal_places_double as i32;

    // Round if the argument is not undefined, always format as exponential.
    let mut buffer: NumberToStringBuffer = [0; 124];
    let length = {
        let mut builder = StringBuilder::new(&mut buffer[..]);
        builder.reset();
        if arg.is_undefined() {
            dragonbox_to_chars::to_exponential(x, &mut builder);
        } else {
            let converter = DoubleToStringConverter::ecma_script_converter();
            converter.to_exponential(x, decimal_places, &mut builder);
        }
        builder.finalize().len()
    };
    Ok(ascii_string_value(vm, &buffer[..length]))
}

// toFixed converts a number to a string, always formatting as an a decimal fraction.
// This method takes an argument specifying a number of decimal places to round the
// significand to. However when converting large values (1e+21 and above) this
// method will instead fallback to calling ToString.
fn number_proto_func_to_fixed(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let x = this_number_value(call)?;

    let decimal_places_double = call.argument(0).to_integer_or_infinity_checked()?;
    if !(0.0..=100.0).contains(&decimal_places_double) {
        return Err(Thrown::range_error("toFixed() argument must be between 0 and 100"));
    }
    let decimal_places = decimal_places_double as i32;

    // 15.7.4.5.7 states "If x >= 10^21, then let m = ToString(x)"
    // This also covers Ininity, and structure the check so that NaN
    // values are also handled by numberToString
    if !(x.abs() < 1e+21) {
        return Ok(number_string_value(vm, x));
    }

    // The check above will return false for NaN or Infinity, these will be
    // handled by numberToString.
    debug_assert!(x.is_finite());

    let mut buffer: NumberToStringBuffer = [0; 124];
    Ok(ascii_string_value(vm, number_to_fixed_width_string(x, decimal_places as u32, &mut buffer)))
}

// toPrecision converts a number to a string, taking an argument specifying a
// number of significant figures to round the significand to. For positive
// exponent, all values that can be represented using a decimal fraction will
// be, e.g. when rounding to 3 s.f. any value up to 999 will be formated as a
// decimal, whilst 1000 is converted to the exponential representation 1.00e+3.
// For negative exponents values >= 1e-6 are formated as decimal fractions,
// with smaller values converted to exponential representation.
fn number_proto_func_to_precision(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let x = this_number_value(call)?;

    let arg = call.argument(0);
    // To precision called with no argument is treated as ToString.
    if arg.is_undefined() {
        return Ok(number_string_value(vm, x));
    }

    // Perform ToInteger on the argument before remaining steps (RETURN_IF_EXCEPTION: Symbol e getter que lança).
    let significant_figures_double = arg.to_integer_or_infinity_checked()?;

    // Handle NaN and Infinity.
    if !x.is_finite() {
        return Ok(number_string_value(vm, x));
    }

    if !(1.0..=100.0).contains(&significant_figures_double) {
        return Err(Thrown::range_error("toPrecision() argument must be between 1 and 100"));
    }
    let significant_figures = significant_figures_double as i32;

    // TrailingZerosPolicy::Keep
    let mut buffer: NumberToStringBuffer = [0; 124];
    Ok(ascii_string_value(vm, number_to_fixed_precision_string(x, significant_figures as u32, &mut buffer, false)))
}

fn number_proto_func_to_string(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let double_value = this_number_value(call)?;

    let radix = extract_to_string_radix_argument(call.argument(0))?;

    Ok(JSValue::from_js_string(number_to_string(global_object.vm(), double_value, radix)))
}

fn number_proto_func_to_locale_string(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let x = this_number_value(call)?;

    // `IntlNumberFormat::create` + `initializeNumberFormat` (ou o `defaultNumberFormat()` sem argumentos).
    let text = to_locale_string(global_object, call.argument(0), call.argument(1), &NumericInput::Double(x))?;
    Ok(JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf8(text.as_bytes()))))
}

fn number_proto_func_value_of(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let x = this_number_value(call)?;
    Ok(js_number(x))
}

crate::host_function!(number_proto_host_to_exponential, number_proto_func_to_exponential);
crate::host_function!(number_proto_host_to_fixed, number_proto_func_to_fixed);
crate::host_function!(number_proto_host_to_precision, number_proto_func_to_precision);
crate::host_function!(number_proto_host_to_string, number_proto_func_to_string);
crate::host_function!(number_proto_host_to_locale_string, number_proto_func_to_locale_string);
crate::host_function!(number_proto_host_value_of, number_proto_func_value_of);

/// `class NumberPrototype final : public NumberObject`: espaço de nomes de `createStructure` e `create`.
pub struct NumberPrototype;

impl NumberPrototype {
    /// `createStructure(vm, globalObject, prototype)` (`NumberPrototypeInlines.h`).
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::NumberObjectType, NumberPrototype::STRUCTURE_FLAGS),
            &NUMBER_PROTOTYPE_S_INFO,
        )
    }

    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
    pub const STRUCTURE_FLAGS: u32 = JSWrapperObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE;

    /// `create(vm, globalObject, structure)`: o `NumberPrototype(vm, structure)` e o `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: StructureRef) -> JSWrapperObjectRef {
        let prototype = JSWrapperObject::create(vm, structure);
        NumberPrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm, globalObject)`.
    fn finish_creation(prototype: &JSWrapperObject, vm: &VM, global_object: &JSGlobalObject) {
        prototype.finish_creation(vm);
        prototype.set_internal_value(js_number_i32(0));

        // Só `toString` (o `numberProtoToStringFunction()` do global) nasce aqui; as cinco da
        // `numberPrototypeTable` reificam no primeiro acesso.
        put_direct_native_function_without_transition(
            vm,
            global_object,
            prototype,
            &vm.property_names.to_string,
            1,
            number_proto_host_to_string,
            ImplementationVisibility::Public,
            Intrinsic::NumberPrototypeToStringIntrinsic,
            DONT_ENUM,
        );
        prototype.structure().set_may_be_prototype(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn radix(value: f64, radix: i32) -> String {
        String::from_utf8_lossy(&to_string_with_radix(value, radix).latin1()).into_owned()
    }

    #[test]
    fn integers_in_every_radix() {
        assert_eq!(radix(255.0, 16), "ff");
        assert_eq!(radix(-255.0, 2), "-11111111");
        assert_eq!(radix(0.0, 7), "0");
        assert_eq!(radix(-0.0, 7), "0");
        assert_eq!(radix(35.0, 36), "z");
        assert_eq!(radix(-2147483648.0, 16), "-80000000");
        assert_eq!(radix(4294967296.0, 16), "100000000");
        assert_eq!(radix(9007199254740992.0, 2), format!("1{}", "0".repeat(53)));
        assert_eq!(radix(1e21, 16), "3635c9adc5dea00000");
    }

    #[test]
    fn fractions_round_trip_the_shortest_representation() {
        assert_eq!(radix(0.5, 2), "0.1");
        assert_eq!(radix(-0.5, 2), "-0.1");
        assert_eq!(radix(255.5, 16), "ff.8");
        assert_eq!(radix(-255.5, 16), "-ff.8");
        assert_eq!(radix(0.5, 36), "0.i");
        assert_eq!(radix(0.1, 2), "0.0001100110011001100110011001100110011001100110011001101");
        assert_eq!(radix(0.1, 16), "0.1999999999999a");
        assert_eq!(radix(4294967296.5, 16), "100000000.8");
    }

    #[test]
    fn radix_ten_and_special_values_use_the_decimal_formatter() {
        assert_eq!(radix(0.1, 10), "0.1");
        assert_eq!(radix(f64::NAN, 2), "NaN");
        assert_eq!(radix(f64::INFINITY, 16), "Infinity");
        assert_eq!(radix(f64::NEG_INFINITY, 16), "-Infinity");
    }

    #[test]
    fn radix_argument_extraction() {
        assert_eq!(extract_to_string_radix_argument(JSValue::Undefined), Ok(10));
        assert_eq!(extract_to_string_radix_argument(JSValue::Int32(16)), Ok(16));
        assert_eq!(extract_to_string_radix_argument(JSValue::Double(2.9)), Ok(2));
        assert!(extract_to_string_radix_argument(JSValue::Int32(1)).is_err());
        assert!(extract_to_string_radix_argument(JSValue::Int32(37)).is_err());
        assert!(extract_to_string_radix_argument(JSValue::Double(f64::NAN)).is_err());
    }

    #[test]
    fn this_number_error_names_the_type() {
        let call = HostCall::new(JSValue::Undefined, Vec::new());
        assert_eq!(
            this_number_value(&call),
            Err(Thrown::type_error("thisNumberValue called on incompatible undefined"))
        );
        let call = HostCall::new(JSValue::Double(1.5), Vec::new());
        assert_eq!(this_number_value(&call), Ok(1.5));
        let call = HostCall::new(JSValue::Int32(3), Vec::new());
        assert_eq!(this_number_value(&call), Ok(3.0));
    }

    #[test]
    fn exact_binary_fractions_in_power_of_two_radices() {
        assert_eq!(radix(0.75, 2), "0.11");
        assert_eq!(radix(10.5, 2), "1010.1");
        assert_eq!(radix(-0.0625, 2), "-0.0001");
        assert_eq!(radix(0.25, 4), "0.1");
        assert_eq!(radix(35.5, 36), "z.i");
        assert_eq!(radix(0.5, 8), "0.4");
    }

    #[test]
    fn largest_double_below_one_keeps_every_bit() {
        // 1 - 2^-53 é exatamente 53 bits 1 depois do ponto, e nenhuma forma mais curta o identifica.
        let below_one = f64::from_bits(1.0f64.to_bits() - 1);
        assert_eq!(radix(below_one, 2), format!("0.{}", "1".repeat(53)));
        assert_eq!(radix(-below_one, 2), format!("-0.{}", "1".repeat(53)));
    }

    #[test]
    fn fraction_digits_never_end_in_zero() {
        // 1 + 2^-52 em base 3: a fração termina num dígito diferente de zero (os zeros finais são cortados
        // quando não há arredondamento para cima) e a parte inteira continua 1.
        let value = f64::from_bits(1.0f64.to_bits() + 1);
        let text = radix(value, 3);
        assert!(text.starts_with("1."), "{text}");
        assert!(!text.ends_with('0'), "{text}");
    }

    #[test]
    fn integers_above_int32_use_the_integer_path() {
        assert_eq!(radix(2147483648.0, 16), "80000000");
        assert_eq!(radix(-2147483649.0, 16), "-80000001");
        assert_eq!(radix(4503599627370496.0, 16), "10000000000000");
        assert_eq!(radix(9007199254740993.0, 10), "9007199254740992");
        assert_eq!(radix(1e21, 10), "1e+21");
    }

    /// O `DoubleToRadixCString` do V8, medido no Chromium 154; o JSC (padrão) arredonda a cauda de outro jeito.
    #[test]
    fn v8_radix_strings_round_the_tail_like_chromium() {
        assert_eq!(radix(0.123456789, 36), "0.4fzzzxjylrz");
        set_v8_radix_strings(true);
        let cases = [
            (0.123456789, 36, "0.4fzzzxjylrx"),
            (0.9876543210123, 36, "0.zk00001xj0a"),
            (0.000123456, 36, "0.005rcwu8qgexl"),
            (2f64.powi(-30) + 0.1, 7, "0.04620462051066354516"),
        ];
        let got: Vec<String> = cases.iter().map(|(value, base, _)| radix(*value, *base)).collect();
        reset_for_program();
        assert_eq!(got, cases.iter().map(|(_, _, text)| text.to_string()).collect::<Vec<_>>());
    }

    #[test]
    fn radix_argument_truncates_and_rejects() {
        assert_eq!(extract_to_string_radix_argument(JSValue::Double(36.9)), Ok(36));
        assert_eq!(extract_to_string_radix_argument(JSValue::Double(2.0)), Ok(2));
        assert!(extract_to_string_radix_argument(JSValue::Double(1.9)).is_err());
        assert!(extract_to_string_radix_argument(JSValue::Double(37.0)).is_err());
        assert!(extract_to_string_radix_argument(JSValue::Double(f64::INFINITY)).is_err());
        assert!(extract_to_string_radix_argument(JSValue::Int32(i32::MIN)).is_err());
    }
}
