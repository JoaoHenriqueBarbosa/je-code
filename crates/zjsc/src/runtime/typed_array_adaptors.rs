//! Porte de `runtime/TypedArrayAdaptors.h` e `runtime/ToNativeFromValue.h`: o que cada `Adaptor`
//! (`Int8Adaptor`... `BigUint64Adaptor`, `Uint8ClampedAdaptor`, `Float16Adaptor`) faz com um elemento.
//!
//! DIVERGÊNCIA: no C++ o `Adaptor` é um parâmetro de template e o elemento é o tipo nativo (`int8_t`,
//! `float`, `Float16`...). Aqui o elemento nativo é `NativeElement`, os bits do elemento em ordem de bytes
//! little-endian estendidos com zeros num `u64`, e o `Adaptor` é o `TypedArrayType`: cada função recebe o
//! tipo e despacha por `match`. O resultado é o mesmo, com uma só cópia do algoritmo em vez de doze
//! instanciações (o que também serve ao DRY do projeto). `canConvertToJSQuickly` só existe para a JIT e
//! o `JSBigInt` do `toJSValue` é o único `Adaptor` que aloca.

use crate::runtime::host_call::Thrown;
use crate::runtime::js_big_int::{ImplResult, JSBigInt};
use crate::runtime::js_big_int_ops::{impl_result_value, make_big_int_from_i64, to_big_int, to_big_uint64_value};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::{js_number_i32, purify_nan, pnan, JSValue};
use crate::runtime::math_common::to_int32;
use crate::runtime::typed_array_type::TypedArrayType;
use crate::wtf::float16::{convert_float16_to_float64, convert_float64_to_float16};

/// Os bits do elemento (`Adaptor::Type`), little-endian e zero-estendidos.
pub type NativeElement = u64;

/// O que `static_cast<Type>(integer)` guarda: os `element_size` bytes baixos.
fn truncate(type_: TypedArrayType, value: u64) -> NativeElement {
    match type_.element_size() {
        1 => value & 0xff,
        2 => value & 0xffff,
        4 => value & 0xffff_ffff,
        _ => value,
    }
}

/// O valor inteiro do elemento, com sinal para os tipos com sinal (o `Type` convertido para `int64_t`).
pub fn integer_value(type_: TypedArrayType, element: NativeElement) -> i128 {
    debug_assert!(!type_.is_float());
    match type_ {
        TypedArrayType::Int8 => i128::from(element as u8 as i8),
        TypedArrayType::Int16 => i128::from(element as u16 as i16),
        TypedArrayType::Int32 => i128::from(element as u32 as i32),
        TypedArrayType::BigInt64 => i128::from(element as i64),
        _ => i128::from(element),
    }
}

/// O valor de um elemento de ponto flutuante como `double` (`static_cast<double>(value)`).
pub fn float_value(type_: TypedArrayType, element: NativeElement) -> f64 {
    match type_ {
        TypedArrayType::Float16 => convert_float16_to_float64(element as u16),
        TypedArrayType::Float32 => f64::from(f32::from_bits(element as u32)),
        TypedArrayType::Float64 => f64::from_bits(element),
        _ => unreachable!("float_value de tipo que não é de ponto flutuante"),
    }
}

/// Os bits de um `double` já convertido para o tipo de ponto flutuante (`static_cast<Type>(double)`).
fn float_bits(type_: TypedArrayType, value: f64) -> NativeElement {
    match type_ {
        TypedArrayType::Float16 => u64::from(convert_float64_to_float16(value)),
        TypedArrayType::Float32 => u64::from((value as f32).to_bits()),
        TypedArrayType::Float64 => value.to_bits(),
        _ => unreachable!("float_bits de tipo que não é de ponto flutuante"),
    }
}

/// Lê um elemento dos `element_size` primeiros bytes de `bytes` (ordem do host, little-endian).
pub fn read_element(type_: TypedArrayType, bytes: &[u8]) -> NativeElement {
    let size = type_.element_size();
    let mut buffer = [0u8; 8];
    buffer[..size].copy_from_slice(&bytes[..size]);
    u64::from_le_bytes(buffer)
}

/// Grava um elemento nos `element_size` primeiros bytes de `bytes`.
pub fn write_element(type_: TypedArrayType, bytes: &mut [u8], element: NativeElement) {
    let size = type_.element_size();
    bytes[..size].copy_from_slice(&element.to_le_bytes()[..size]);
}

/// `Adaptor::toJSValue(globalObject, value)`: `Err(Pending)` se a alocação do BigInt lançou.
pub fn to_js_value(type_: TypedArrayType, element: NativeElement) -> Result<JSValue, Thrown> {
    let value = match type_ {
        TypedArrayType::Int8 | TypedArrayType::Int16 | TypedArrayType::Int32 => js_number_i32(integer_value(type_, element) as i32),
        TypedArrayType::Uint8 | TypedArrayType::Uint8Clamped | TypedArrayType::Uint16 => js_number_i32(element as i32),
        TypedArrayType::Uint32 => JSValue::from_u32(element as u32),
        TypedArrayType::Float16 | TypedArrayType::Float32 | TypedArrayType::Float64 => {
            JSValue::double_number(purify_nan(float_value(type_, element)))
        }
        TypedArrayType::BigInt64 => make_big_int_from_i64(element as i64),
        TypedArrayType::BigUint64 => impl_result_value(JSBigInt::create_from_u64(element).map(ImplResult::Heap)),
        TypedArrayType::NotTypedArray | TypedArrayType::DataView => unreachable!("toJSValue de {type_:?}"),
    };
    if value.is_empty() { Err(Thrown::Pending) } else { Ok(value) }
}

/// `Adaptor::toNativeFromInt32(value)`.
pub fn to_native_from_int32(type_: TypedArrayType, value: i32) -> NativeElement {
    match type_ {
        TypedArrayType::Uint8Clamped => value.clamp(0, 255) as u64,
        TypedArrayType::Float16 | TypedArrayType::Float32 | TypedArrayType::Float64 => float_bits(type_, f64::from(value)),
        // `static_cast<int64_t>(int32_t)` estende o sinal, e os dois BigInt guardam os mesmos 64 bits.
        TypedArrayType::BigInt64 | TypedArrayType::BigUint64 => i64::from(value) as u64,
        _ => truncate(type_, value as u32 as u64),
    }
}

/// `Adaptor::toNativeFromUint32(value)`.
pub fn to_native_from_uint32(type_: TypedArrayType, value: u32) -> NativeElement {
    match type_ {
        TypedArrayType::Uint8Clamped => u64::from(value.min(255)),
        TypedArrayType::Float16 | TypedArrayType::Float32 | TypedArrayType::Float64 => float_bits(type_, f64::from(value)),
        TypedArrayType::BigInt64 | TypedArrayType::BigUint64 => u64::from(value),
        _ => truncate(type_, u64::from(value)),
    }
}

/// `Adaptor::toNativeFromDouble(value)`.
pub fn to_native_from_double(type_: TypedArrayType, value: f64) -> NativeElement {
    match type_ {
        TypedArrayType::Uint8Clamped => {
            if value.is_nan() || value < 0.0 {
                0
            } else if value > 255.0 {
                255
            } else {
                // `lrint`: arredonda para o par mais próximo.
                value.round_ties_even() as u64
            }
        }
        TypedArrayType::Float16 | TypedArrayType::Float32 | TypedArrayType::Float64 => float_bits(type_, value),
        TypedArrayType::BigInt64 | TypedArrayType::BigUint64 => (value as i64) as u64,
        // `truncateDoubleToInt32` com o recurso a `toInt32`: o `ToInt32` da especificação.
        _ => truncate(type_, to_int32(value) as u32 as u64),
    }
}

/// `Adaptor::toNativeFromUndefined()`.
pub fn to_native_from_undefined(type_: TypedArrayType) -> NativeElement {
    match type_ {
        TypedArrayType::Float16 | TypedArrayType::Float32 | TypedArrayType::Float64 => float_bits(type_, pnan()),
        _ => 0,
    }
}

/// `Adaptor::convertTo<OtherAdaptor>(value)`: o elemento do tipo `from` convertido para o tipo `to`.
pub fn convert_to(from: TypedArrayType, to: TypedArrayType, element: NativeElement) -> NativeElement {
    if from.is_float() {
        return to_native_from_double(to, float_value(from, element));
    }
    if from.is_big_int() {
        if to.is_float() {
            let value = if from == TypedArrayType::BigInt64 { element as i64 as f64 } else { element as f64 };
            return to_native_from_double(to, value);
        }
        // `static_cast<typename OtherAdaptor::Type>(int64)`: os bits baixos.
        return truncate(to, element);
    }
    if from == TypedArrayType::Uint32 {
        return to_native_from_uint32(to, element as u32);
    }
    to_native_from_int32(to, integer_value(from, element) as i32)
}

/// `toNativeFromValue<Adaptor>(globalObject, value)`: `Err(Pending)` se a conversão lançou.
pub fn to_native_from_value(global_object: &JSGlobalObject, type_: TypedArrayType, value: JSValue) -> Result<NativeElement, Thrown> {
    if type_.is_big_int() {
        // `value.toBigInt64(globalObject)` e `toBigUInt64`: `toBigInt` e os 64 bits baixos.
        let big_int = to_big_int(value);
        if big_int.is_empty() {
            return Err(Thrown::Pending);
        }
        return Ok(to_big_uint64_value(big_int));
    }
    if let JSValue::Int32(integer) = value {
        return Ok(to_native_from_int32(type_, integer));
    }
    if value.is_number() {
        return Ok(to_native_from_double(type_, value.as_double()));
    }
    let number = value.to_number();
    if global_object.vm().exception().is_some() {
        return Err(Thrown::Pending);
    }
    Ok(to_native_from_double(type_, number))
}

/// O menor e o maior valor de um tipo inteiro (`minValue` e `maxValue`).
fn integer_range(type_: TypedArrayType) -> (i128, i128) {
    match type_ {
        TypedArrayType::Int8 => (i128::from(i8::MIN), i128::from(i8::MAX)),
        TypedArrayType::Int16 => (i128::from(i16::MIN), i128::from(i16::MAX)),
        TypedArrayType::Int32 => (i128::from(i32::MIN), i128::from(i32::MAX)),
        TypedArrayType::Uint8 | TypedArrayType::Uint8Clamped => (0, i128::from(u8::MAX)),
        TypedArrayType::Uint16 => (0, i128::from(u16::MAX)),
        TypedArrayType::Uint32 => (0, i128::from(u32::MAX)),
        TypedArrayType::BigInt64 => (i128::from(i64::MIN), i128::from(i64::MAX)),
        TypedArrayType::BigUint64 => (0, i128::from(u64::MAX)),
        _ => unreachable!("integer_range de tipo que não é inteiro"),
    }
}

/// `Adaptor::toNativeFromInt32WithoutCoercion(value)`.
fn to_native_from_int32_without_coercion(type_: TypedArrayType, value: i32) -> Option<NativeElement> {
    if type_.is_float() {
        return Some(to_native_from_int32(type_, value));
    }
    let (minimum, maximum) = integer_range(type_);
    let value_wide = i128::from(value);
    if value_wide < minimum || value_wide > maximum {
        return None;
    }
    Some(to_native_from_int32(type_, value))
}

/// `Adaptor::toNativeFromDoubleWithoutCoercion(value)`.
fn to_native_from_double_without_coercion(type_: TypedArrayType, value: f64) -> Option<NativeElement> {
    if type_.is_float() {
        if value.is_nan() || value.is_infinite() {
            return Some(float_bits(type_, value));
        }
        let element = float_bits(type_, value);
        if float_value(type_, element) != value {
            return None;
        }
        let (minimum, maximum) = match type_ {
            TypedArrayType::Float16 => (-65504.0, 65504.0),
            TypedArrayType::Float32 => (f64::from(f32::MIN), f64::from(f32::MAX)),
            _ => (f64::MIN, f64::MAX),
        };
        if value < minimum || value > maximum {
            return None;
        }
        return Some(element);
    }
    // `static_cast<Type>(truncateDoubleToInt64(value))` e a prova de que voltou ao mesmo `double`.
    let element = truncate(type_, (value as i64) as u64);
    if integer_value(type_, element) as f64 != value {
        return None;
    }
    Some(element)
}

/// `toNativeFromValueWithoutCoercion<Adaptor>(value)`.
pub fn to_native_from_value_without_coercion(type_: TypedArrayType, value: JSValue) -> Option<NativeElement> {
    if type_.is_big_int() {
        if !value.is_big_int() {
            return None;
        }
        return Some(to_big_uint64_value(value));
    }
    if !value.is_number() {
        return None;
    }
    if let JSValue::Int32(integer) = value {
        return to_native_from_int32_without_coercion(type_, integer);
    }
    to_native_from_double_without_coercion(type_, value.as_double())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_conversions_wrap_and_clamp() {
        assert_eq!(to_native_from_int32(TypedArrayType::Int8, -1), 0xff);
        assert_eq!(integer_value(TypedArrayType::Int8, 0xff), -1);
        assert_eq!(to_native_from_int32(TypedArrayType::Uint8, 257), 1);
        assert_eq!(to_native_from_int32(TypedArrayType::Uint8Clamped, 300), 255);
        assert_eq!(to_native_from_int32(TypedArrayType::Uint8Clamped, -3), 0);
        assert_eq!(to_native_from_double(TypedArrayType::Uint8Clamped, 2.5), 2);
        assert_eq!(to_native_from_double(TypedArrayType::Uint8Clamped, 3.5), 4);
        assert_eq!(to_native_from_double(TypedArrayType::Uint8, 4294967297.0), 1);
        assert_eq!(to_native_from_double(TypedArrayType::Int32, f64::NAN), 0);
        assert_eq!(to_native_from_undefined(TypedArrayType::Uint8), 0);
        assert!(float_value(TypedArrayType::Float32, to_native_from_undefined(TypedArrayType::Float32)).is_nan());
    }

    #[test]
    fn element_bytes_round_trip() {
        let mut bytes = [0u8; 8];
        write_element(TypedArrayType::Uint16, &mut bytes, 0x1234);
        assert_eq!(&bytes[..2], &[0x34, 0x12]);
        assert_eq!(read_element(TypedArrayType::Uint16, &bytes), 0x1234);
        write_element(TypedArrayType::Float64, &mut bytes, 1.5f64.to_bits());
        assert_eq!(float_value(TypedArrayType::Float64, read_element(TypedArrayType::Float64, &bytes)), 1.5);
    }

    #[test]
    fn convert_between_types() {
        let minus_one = to_native_from_int32(TypedArrayType::Int8, -1);
        assert_eq!(convert_to(TypedArrayType::Int8, TypedArrayType::Uint16, minus_one), 0xffff);
        let big = to_native_from_double(TypedArrayType::Float32, 1.5);
        assert_eq!(convert_to(TypedArrayType::Float32, TypedArrayType::Int8, big), 1);
        assert_eq!(convert_to(TypedArrayType::Uint32, TypedArrayType::Float64, 0xffff_ffff), 4294967295.0f64.to_bits());
    }

    #[test]
    fn without_coercion_matches_exactly() {
        assert_eq!(to_native_from_value_without_coercion(TypedArrayType::Uint8, JSValue::Int32(256)), None);
        assert_eq!(to_native_from_value_without_coercion(TypedArrayType::Uint8, JSValue::Int32(255)), Some(255));
        assert_eq!(to_native_from_value_without_coercion(TypedArrayType::Int8, JSValue::Double(1.5)), None);
        assert_eq!(to_native_from_value_without_coercion(TypedArrayType::Int8, JSValue::Double(-0.0)), Some(0));
        assert_eq!(to_native_from_value_without_coercion(TypedArrayType::Int8, JSValue::Double(f64::NAN)), None);
        assert_eq!(to_native_from_value_without_coercion(TypedArrayType::Float32, JSValue::Double(0.1)), None);
        assert!(to_native_from_value_without_coercion(TypedArrayType::Float32, JSValue::Double(f64::NAN)).is_some());
        assert_eq!(to_native_from_value_without_coercion(TypedArrayType::Uint8, JSValue::Undefined), None);
    }
}
