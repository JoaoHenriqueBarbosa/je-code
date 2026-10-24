//! Os métodos de `Buffer.prototype` que leem e gravam bytes: `read*`/`write*` (inteiros, `BigInt`, `float`, `double`,
//! com `byteLength` variável), `fill`, `copy`, `swap16/32/64`, `xxxSlice`/`xxxWrite`, `toLocaleString` e `inspect`.
//! Mensagens e limites medidos no bun 1.4.2 (ver `wip/notes/buffer-plan.md`):
//!
//! - o `offset` de leitura e escrita é inteiro entre 0 e `length - largura` (`>= 0 and <= N`, com `and`); buffer menor
//!   que a largura lança `ERR_BUFFER_OUT_OF_BOUNDS`; o `value` de escrita é comparado como veio (`127.9` em `writeInt8`
//!   lança) e só depois truncado;
//! - `fill` e `copy` seguem o Node (`>= 0 && <= N`); `xxxSlice` com índice fora lança `Index out of range`.

use super::*;
use crate::host_function;
use crate::runtime::js_big_int::{ImplResult, JSBigInt};
use crate::runtime::js_big_int_ops::impl_result_value;
use crate::runtime::js_value::js_undefined;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::native_function::NativeFunction;

type View = JSGenericTypedArrayViewRef;

/// O que o método diz quando o `this` não é um `Uint8Array`.
enum Receiver {
    /// `The "buf" argument must be of type Buffer` (`read*` e `write*`).
    Buffer,
    /// `ERR_INVALID_THIS`: `Can only call Buffer.{name} on instances of Buffer`.
    InvalidThis(&'static str),
    /// `The "source" argument must be an instance of Buffer or Uint8Array` (`copy`).
    Source,
    /// `Expected ArrayBufferView` (`xxxSlice`/`xxxWrite`).
    View,
}

/// O receptor do método (medido no bun 1.4.2): `read*`, `write*` e `xxxSlice`/`xxxWrite` aceitam qualquer typed array
/// (o limite vem de `length` em elementos, os bytes vêm da memória); `fill`, `swap*`, `inspect` e `copy` exigem
/// `Uint8Array`, e `this` `undefined` ou `null` nos de `ERR_INVALID_THIS` lança `Cannot convert undefined or null to object`.
fn receiver(global_object: &JSGlobalObject, call: &HostCall, kind: Receiver) -> Result<View, Thrown> {
    let this = call.this_value();
    if matches!(kind, Receiver::InvalidThis(_)) && this.is_undefined_or_null() {
        return Err(Thrown::type_error("Cannot convert undefined or null to object"));
    }
    let any_typed_array = matches!(kind, Receiver::Buffer | Receiver::View);
    JSGenericTypedArrayView::from_value(&this).filter(|view| any_typed_array || view.typed_array_type() == TypedArrayType::Uint8).ok_or_else(|| match kind {
        Receiver::Buffer => invalid_argument_type(global_object, "buf", "of type Buffer", this),
        Receiver::InvalidThis(name) => throw_coded_type_error(global_object, &format!("Can only call Buffer.{name} on instances of Buffer"), "ERR_INVALID_THIS"),
        Receiver::Source => invalid_argument_type(global_object, "source", "an instance of Buffer or Uint8Array", this),
        Receiver::View => Thrown::type_error("Expected ArrayBufferView"),
    })
}

/// O `this` de `read*`/`write*`: um typed array, ou `None` para um `DataView`. Com `DataView` o bun segue a mesma ordem
/// de validação do `Buffer` (`byteLength`, valor, tipo do `offset`) e só no `offset` numérico falha, porque mede o limite
/// com `length` indefinido (ver `data_view_offset_error`).
fn buffer_receiver(global_object: &JSGlobalObject, call: &HostCall) -> Result<Option<View>, Thrown> {
    if crate::runtime::js_data_view::JSDataView::from_value(&call.this_value()).is_some() {
        return Ok(None);
    }
    receiver(global_object, call, Receiver::Buffer).map(Some)
}

/// O erro de `offset` de um `DataView` como `this`: o teto do `offset` vira `NaN` e nenhum `offset` numérico inteiro
/// passa (medido no bun 1.4.2).
fn data_view_offset_error(global_object: &JSGlobalObject, offset: JSValue) -> Thrown {
    if offset.is_undefined() {
        return out_of_range(global_object, "offset", ">= 0 and <= NaN", 0.0);
    }
    match integer_argument(global_object, offset, "offset") {
        Ok(number) => out_of_range(global_object, "offset", ">= 0 and <= NaN", number),
        Err(thrown) => thrown,
    }
}

/// Valida um argumento numérico inteiro: não número é `ERR_INVALID_ARG_TYPE`, `NaN` e fração são `an integer`.
fn integer_argument(global_object: &JSGlobalObject, value: JSValue, name: &str) -> Result<f64, Thrown> {
    if !value.is_number() {
        return Err(invalid_argument_type(global_object, name, "of type number", value));
    }
    let number = value.as_number();
    if number.is_nan() || (number.is_finite() && number.fract() != 0.0) {
        return Err(out_of_range(global_object, name, "an integer", number));
    }
    Ok(number)
}

/// O `offset` de uma leitura ou escrita de `width` bytes (ausente vale 0), com o `view` que ele indexa. Um `DataView`
/// (`view` ausente) sempre falha no `offset`.
fn checked_offset<'a>(global_object: &JSGlobalObject, value: JSValue, view: Option<&'a View>, width: usize) -> Result<(&'a View, usize), Thrown> {
    let Some(view) = view else {
        return Err(data_view_offset_error(global_object, value));
    };
    let length = view.length();
    if length < width {
        return Err(throw_coded_range_error(global_object, "Attempt to access memory outside buffer bounds", "ERR_BUFFER_OUT_OF_BOUNDS"));
    }
    if value.is_undefined() {
        return Ok((view, 0));
    }
    let number = integer_argument(global_object, value, "offset")?;
    let max = (length - width) as f64;
    if number < 0.0 || number > max {
        return Err(out_of_range(global_object, "offset", &format!(">= 0 and <= {max}"), number));
    }
    Ok((view, number as usize))
}

/// O `byteLength` de `readUIntLE` e companhia: inteiro de 1 a 6.
fn checked_byte_length(global_object: &JSGlobalObject, value: JSValue) -> Result<usize, Thrown> {
    let number = integer_argument(global_object, value, "byteLength")?;
    if !(1.0..=6.0).contains(&number) {
        return Err(out_of_range(global_object, "byteLength", ">= 1 and <= 6", number));
    }
    Ok(number as usize)
}

/// Os bytes como número sem sinal, na ordem pedida.
fn assemble(bytes: &[u8], little: bool) -> u64 {
    if little {
        bytes.iter().rev().fold(0, |accumulator, &byte| accumulator << 8 | u64::from(byte))
    } else {
        bytes.iter().fold(0, |accumulator, &byte| accumulator << 8 | u64::from(byte))
    }
}

/// Estende o sinal de um número de `width` bytes.
fn sign_extend(value: u64, width: usize) -> i64 {
    let shift = 64 - 8 * width as u32;
    ((value << shift) as i64) >> shift
}

/// Grava os `width` bytes baixos de `raw` em `view[offset..]`, na ordem pedida.
fn store(view: &View, offset: usize, width: usize, raw: u64, little: bool) {
    let bytes = raw.to_le_bytes();
    view.with_vector_mut(|destination| {
        let target = &mut destination[offset..offset + width];
        if little {
            target.copy_from_slice(&bytes[..width]);
        } else {
            for (slot, byte) in target.iter_mut().zip(bytes[..width].iter().rev()) {
                *slot = *byte;
            }
        }
    });
}

fn read_raw(global_object: &JSGlobalObject, view: Option<&View>, offset: JSValue, width: usize, little: bool) -> Result<u64, Thrown> {
    let (view, offset) = checked_offset(global_object, offset, view, width)?;
    Ok(view.with_vector(|bytes| assemble(&bytes[offset..offset + width], little)))
}

fn read_integer(global_object: &JSGlobalObject, view: Option<&View>, offset: JSValue, width: usize, signed: bool, little: bool) -> HostResult {
    let raw = read_raw(global_object, view, offset, width, little)?;
    Ok(js_number(if signed { sign_extend(raw, width) as f64 } else { raw as f64 }))
}

fn read_fixed<const WIDTH: usize, const SIGNED: bool, const LITTLE: bool>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    read_integer(global_object, buffer_receiver(global_object, call)?.as_ref(), call.argument(0), WIDTH, SIGNED, LITTLE)
}

/// Nas formas de `byteLength` variável o `offset` é obrigatório: ausente ou não numérico é `ERR_INVALID_ARG_TYPE`, antes
/// de qualquer checagem de limite (medido no bun; as formas de largura fixa tratam ausente como 0).
fn require_number_offset(global_object: &JSGlobalObject, value: JSValue) -> Result<(), Thrown> {
    if value.is_number() {
        Ok(())
    } else {
        Err(invalid_argument_type(global_object, "offset", "of type number", value))
    }
}

fn read_variable<const SIGNED: bool, const LITTLE: bool>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let view = buffer_receiver(global_object, call)?;
    require_number_offset(global_object, call.argument(0))?;
    let width = checked_byte_length(global_object, call.argument(1))?;
    read_integer(global_object, view.as_ref(), call.argument(0), width, SIGNED, LITTLE)
}

fn read_float<const WIDTH: usize, const LITTLE: bool>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let view = buffer_receiver(global_object, call)?;
    let raw = read_raw(global_object, view.as_ref(), call.argument(0), WIDTH, LITTLE)?;
    Ok(js_number(if WIDTH == 4 { f64::from(f32::from_bits(raw as u32)) } else { f64::from_bits(raw) }))
}

fn read_big<const SIGNED: bool, const LITTLE: bool>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let view = buffer_receiver(global_object, call)?;
    let raw = read_raw(global_object, view.as_ref(), call.argument(0), 8, LITTLE)?;
    let number = if SIGNED { i128::from(raw as i64) } else { i128::from(raw) };
    pending_or(global_object, impl_result_value(JSBigInt::create_from_i128(number).map(ImplResult::Heap)))
}

/// Limites de um inteiro de `width` bytes e o trecho `It must be ...` do Node (`2 ** N` a partir de 5 bytes).
fn integer_limits(width: usize, signed: bool) -> (f64, f64, String) {
    let bits = 8 * width as i32;
    let (min, max) = if signed { (-(2f64.powi(bits - 1)), 2f64.powi(bits - 1) - 1.0) } else { (0.0, 2f64.powi(bits) - 1.0) };
    let rule = if width <= 4 {
        format!(">= {min} and <= {max}")
    } else if signed {
        format!(">= -(2 ** {0}) and < 2 ** {0}", bits - 1)
    } else {
        format!(">= 0 and < 2 ** {bits}")
    };
    (min, max, rule)
}

/// A ordem do bun (igual com `DataView`): o `value` vira número, o tipo do `offset` é checado (obrigatório nas formas de
/// `byteLength` variável), a faixa do `value` e por fim o limite do `offset`.
fn write_integer(global_object: &JSGlobalObject, view: Option<&View>, call: &HostCall, width: usize, signed: bool, little: bool, offset_required: bool) -> HostResult {
    let value = crate::runtime::intl_support::to_number_checked(global_object, call.argument(0))?;
    if offset_required || !call.argument(1).is_undefined() {
        require_number_offset(global_object, call.argument(1))?;
    }
    let (min, max, rule) = integer_limits(width, signed);
    if value > max || value < min {
        return Err(out_of_range(global_object, "value", &rule, value));
    }
    let (view, offset) = checked_offset(global_object, call.argument(1), view, width)?;
    store(view, offset, width, value.trunc() as i64 as u64, little);
    Ok(js_number((offset + width) as f64))
}

fn write_fixed<const WIDTH: usize, const SIGNED: bool, const LITTLE: bool>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    write_integer(global_object, buffer_receiver(global_object, call)?.as_ref(), call, WIDTH, SIGNED, LITTLE, false)
}

fn write_variable<const SIGNED: bool, const LITTLE: bool>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let view = buffer_receiver(global_object, call)?;
    let width = checked_byte_length(global_object, call.argument(2))?;
    write_integer(global_object, view.as_ref(), call, width, SIGNED, LITTLE, true)
}

fn write_float<const WIDTH: usize, const LITTLE: bool>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let view = buffer_receiver(global_object, call)?;
    let value = crate::runtime::intl_support::to_number_checked(global_object, call.argument(0))?;
    let (view, offset) = checked_offset(global_object, call.argument(1), view.as_ref(), WIDTH)?;
    let raw = if WIDTH == 4 { u64::from((value as f32).to_bits()) } else { value.to_bits() };
    store(view, offset, WIDTH, raw, LITTLE);
    Ok(js_number((offset + WIDTH) as f64))
}

fn write_big<const SIGNED: bool, const LITTLE: bool>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let view = buffer_receiver(global_object, call)?;
    let value = call.argument(0);
    if !value.is_big_int() {
        return Err(invalid_argument_type(global_object, "value", "of type bigint", value));
    }
    let text = wtf_to_rust(&value.to_wtf_string());
    let number: i128 = text.parse().unwrap_or(i128::MAX);
    let (min, max, rule) = if SIGNED { (-(1i128 << 63), (1i128 << 63) - 1, ">= -(2n ** 63n) and < 2n ** 63n") } else { (0, (1i128 << 64) - 1, ">= 0n and < 2n ** 64n") };
    if number < min || number > max {
        let shown = if number.unsigned_abs() > 4_294_967_296 { with_separators(&text) } else { text };
        let message = format!("The value of \"value\" is out of range. It must be {rule}. Received {shown}n");
        return Err(throw_coded_range_error(global_object, &message, "ERR_OUT_OF_RANGE"));
    }
    let (view, offset) = checked_offset(global_object, call.argument(1), view.as_ref(), 8)?;
    store(view, offset, 8, number as u64, LITTLE);
    Ok(js_number((offset + 8) as f64))
}

/// `swap16/32/64`: inverte cada grupo de `WIDTH` bytes no lugar.
fn swap<const WIDTH: usize>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let view = receiver(global_object, call, Receiver::InvalidThis(match WIDTH {
        2 => "swap16",
        4 => "swap32",
        _ => "swap64",
    }))?;
    if view.length() % WIDTH != 0 {
        let message = format!("Buffer size must be a multiple of {}-bits", 8 * WIDTH);
        return Err(throw_coded_range_error(global_object, &message, "ERR_INVALID_BUFFER_SIZE"));
    }
    view.with_vector_mut(|bytes| bytes.chunks_exact_mut(WIDTH).for_each(<[u8]>::reverse));
    Ok(call.this_value())
}

/// O padrão de `fill`: o de `Buffer.alloc`, exceto que um `Uint8Array` vazio lança `Buffer cannot be empty`.
fn fill_pattern_for_fill(global_object: &JSGlobalObject, value: JSValue, encoding_value: JSValue) -> Result<Vec<u8>, Thrown> {
    if JSGenericTypedArrayView::from_value(&value).is_some_and(|view| view.typed_array_type() == TypedArrayType::Uint8 && view.length() == 0) {
        return Err(throw_coded_type_error(global_object, "Buffer cannot be empty", "ERR_INVALID_ARG_VALUE"));
    }
    fill_pattern(global_object, value, encoding_value)
}

/// `buffer.fill(value, offset, end, encoding)`:`offset` ou `end` string são a codificação quando `value` é string.
fn fill_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let view = receiver(global_object, call, Receiver::InvalidThis("fill"))?;
    let length = view.length();
    let value = call.argument(0);
    let (mut offset_value, mut end_value, mut encoding_value) = (call.argument(1), call.argument(2), call.argument(3));
    if value.is_string() {
        if offset_value.is_string() {
            (encoding_value, offset_value, end_value) = (offset_value, JSValue::undefined(), JSValue::undefined());
        } else if end_value.is_string() {
            (encoding_value, end_value) = (end_value, JSValue::undefined());
        }
    }
    let start = if offset_value.is_undefined() { 0 } else { validate_integer(global_object, offset_value, "offset", MAX_BUFFER_LENGTH)? as usize };
    let end = if end_value.is_undefined() { length } else { validate_integer(global_object, end_value, "end", length as f64)? as usize };
    let mut pattern = fill_pattern_for_fill(global_object, value, encoding_value)?;
    if pattern.is_empty() {
        pattern.push(0);
    }
    if start < end {
        view.with_vector_mut(|bytes| {
            for (index, byte) in bytes[start..end].iter_mut().enumerate() {
                *byte = pattern[index % pattern.len()];
            }
        });
    }
    Ok(call.this_value())
}

/// Argumento inteiro tolerante de `copy` e `xxxSlice`: ausente vale `default`, `NaN` vale 0, fração cai.
fn lenient_integer(value: JSValue, default: f64) -> f64 {
    if value.is_undefined() {
        return default;
    }
    let number = value.to_number();
    if number.is_nan() {
        0.0
    } else {
        number.trunc()
    }
}

/// `buffer.copy(target, targetStart, sourceStart, sourceEnd)`: devolve quantos bytes copiou.
fn copy_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let view = receiver(global_object, call, Receiver::Source)?;
    let target_value = call.argument(0);
    let Some(target) = JSGenericTypedArrayView::from_value(&target_value).filter(|target| target.typed_array_type() == TypedArrayType::Uint8) else {
        return Err(invalid_argument_type(global_object, "target", "an instance of Buffer or Uint8Array", target_value));
    };
    let source = view.with_view_bytes(|bytes| bytes.to_vec());
    let source_length = source.len() as f64;
    // No `copy` do bun os infinitos valem 0, como o `NaN`.
    let finite = |value: JSValue, default: f64| Some(lenient_integer(value, default)).filter(|number| number.is_finite()).unwrap_or(0.0);
    let target_start = finite(call.argument(1), 0.0);
    if target_start < 0.0 {
        return Err(out_of_range(global_object, "targetStart", ">= 0", target_start));
    }
    let source_start = finite(call.argument(2), 0.0);
    if source_start < 0.0 || source_start > source_length {
        return Err(out_of_range(global_object, "sourceStart", &format!(">= 0 && <= {source_length}"), source_start));
    }
    let source_end = finite(call.argument(3), source_length);
    if source_end < 0.0 {
        return Err(out_of_range(global_object, "sourceEnd", ">= 0", source_end));
    }
    let source_end = source_end.min(source_length);
    let target_length = target.length() as f64;
    if target_start >= target_length || source_start >= source_end {
        return Ok(js_number(0.0));
    }
    let count = (source_end - source_start).min(target_length - target_start) as usize;
    let (from, to) = (source_start as usize, target_start as usize);
    target.with_vector_mut(|bytes| bytes[to..to + count].copy_from_slice(&source[from..from + count]));
    Ok(js_number(count as f64))
}

// Códigos das codificações nos parâmetros genéricos de `slice_body` e `string_write` (uma `enum` não serve de `const`).
const UTF8: u8 = 0;
const HEX: u8 = 1;
const BASE64: u8 = 2;
const BASE64_URL: u8 = 3;
const LATIN1: u8 = 4;
const ASCII: u8 = 5;
const UTF16_LE: u8 = 6;

fn encoding_by_code(code: u8) -> Encoding {
    [Encoding::Utf8, Encoding::Hex, Encoding::Base64, Encoding::Base64Url, Encoding::Latin1, Encoding::Ascii, Encoding::Utf16Le][usize::from(code)]
}

/// `buffer.xxxSlice(start, end)`: índice negativo ou `end` além do fim lança `Index out of range`.
fn slice_body<const CODE: u8>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let view = receiver(global_object, call, Receiver::View)?;
    let length = view.length() as f64;
    let start = lenient_integer(call.argument(0), 0.0);
    let end = lenient_integer(call.argument(1), length);
    if start < 0.0 || end < 0.0 || end > length {
        return Err(throw_coded_range_error(global_object, "Index out of range", "ERR_OUT_OF_RANGE"));
    }
    let text = if start >= end {
        WtfString::from_latin1(b"")
    } else {
        view.with_view_bytes(|bytes| decode_bytes(&bytes[start as usize..end as usize], encoding_by_code(CODE)))
    };
    Ok(JSValue::from_js_string(js_string(global_object.vm(), &text)))
}

/// `buffer.xxxWrite(string, offset, length)`: `offset` ou `length` `NaN` não gravam nada, fora do buffer lança
/// `ERR_BUFFER_OUT_OF_BOUNDS`.
fn string_write<const CODE: u8>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let view = receiver(global_object, call, Receiver::View)?;
    let total = view.length() as f64;
    let argument = call.argument(0);
    let text = if argument.is_string() {
        argument
    } else {
        JSValue::from_js_string(js_string(global_object.vm(), &pending_or(global_object, argument.to_wtf_string())?))
    };
    let number = |value: JSValue, default: f64| if value.is_undefined() { default } else { value.to_number() };
    let offset = number(call.argument(1), 0.0);
    if offset.is_nan() {
        return Ok(js_number(0.0));
    }
    let offset = offset.trunc();
    if offset < 0.0 || offset > total {
        return Err(out_of_bounds(global_object, "offset"));
    }
    let remaining = total - offset;
    let length = number(call.argument(2), remaining);
    if length.is_nan() {
        return Ok(js_number(0.0));
    }
    let length = length.trunc();
    if length < 0.0 || length > remaining {
        return Err(out_of_bounds(global_object, "length"));
    }
    write_encoded(global_object, &view, text, offset as usize, length as usize, encoding_by_code(CODE))
}

/// `buffer.inspect()` e `buffer[Symbol.for('nodejs.util.inspect.custom')]` (a mesma função, medido no bun 1.4.2):
/// `<Buffer 61 62>` com no máximo `INSPECT_MAX_BYTES` bytes (50 por padrão) e `... N more bytes` para o resto. Com
/// o limite em 0 o texto sai `<Buffer ... N more bytes >` (espaço no fim, como no bun). Com um segundo argumento objeto
/// (o `options` do console) as propriedades próprias de texto entram depois dos bytes: `<Buffer 61 62, x: 1>`.
fn inspect_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let view = receiver(global_object, call, Receiver::InvalidThis("inspect"))?;
    let max = super::inspect_max_bytes();
    let (shown, length) = view.with_view_bytes(|bytes| {
        let count = (bytes.len() as f64).min(max.trunc()) as usize;
        (bytes[..count].iter().map(|byte| format!("{byte:02x}")).collect::<Vec<_>>().join(" "), bytes.len())
    });
    let mut text = shown;
    let remaining = length as f64 - max;
    if remaining > 0.0 {
        let noun = if remaining == 1.0 { "byte" } else { "bytes" };
        let tail = format!("... {} more {noun}", super::number_text(remaining));
        if text.is_empty() {
            text = format!("{tail} ");
        } else {
            text.push_str(&format!(" {tail}"));
        }
    }
    if call.argument(1).is_object() {
        let extras = crate::runtime::util_inspect::inspect_buffer_extras(global_object, call.this_value())?;
        if !extras.is_empty() {
            text.push_str(", ");
            text.push_str(&extras);
        }
    }
    Ok(JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf16(&format!("<Buffer {text}>").encode_utf16().collect::<Vec<u16>>()))))
}

/// Getter `offset`: o `byteOffset` do Buffer; `undefined` fora de um Buffer (medido).
fn offset_getter_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(JSGenericTypedArrayView::from_value(&call.this_value()).filter(|view| view.typed_array_type() == TypedArrayType::Uint8).map_or_else(js_undefined, |view| js_number(view.byte_offset() as f64)))
}

/// Getter `parent`: o `ArrayBuffer` do Buffer; `undefined` fora de um Buffer (medido).
fn parent_getter_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(JSGenericTypedArrayView::from_value(&call.this_value())
        .filter(|view| view.typed_array_type() == TypedArrayType::Uint8)
        .map_or_else(js_undefined, |view| crate::runtime::js_array_buffer::to_js_array_buffer(global_object, &view.possibly_shared_buffer()).as_value()))
}

host_function!(h_offset_getter, offset_getter_body);
host_function!(h_parent_getter, parent_getter_body);

/// Os acessores `offset` e `parent` de `Buffer.prototype` (não enumeráveis, só getter).
pub(super) fn put_accessors(global_object: &JSGlobalObject, prototype: &crate::runtime::js_object::JSObject) {
    let vm = global_object.vm();
    crate::runtime::native_class_support::put_native_accessor(vm, global_object, prototype, "offset", h_offset_getter, None, DONT_ENUM);
    crate::runtime::native_class_support::put_native_accessor(vm, global_object, prototype, "parent", h_parent_getter, None, DONT_ENUM);
}

macro_rules! host_functions {
    ($($name:ident => $body:path),* $(,)?) => { $(host_function!($name, $body);)* };
}

host_functions! {
    h_ascii_slice => slice_body::<ASCII>, h_ascii_write => string_write::<ASCII>,
    h_base64_slice => slice_body::<BASE64>, h_base64_write => string_write::<BASE64>,
    h_base64url_slice => slice_body::<BASE64_URL>, h_base64url_write => string_write::<BASE64_URL>,
    h_hex_slice => slice_body::<HEX>, h_hex_write => string_write::<HEX>,
    h_latin1_slice => slice_body::<LATIN1>, h_latin1_write => string_write::<LATIN1>,
    h_ucs2_slice => slice_body::<UTF16_LE>, h_ucs2_write => string_write::<UTF16_LE>,
    h_utf8_slice => slice_body::<UTF8>, h_utf8_write => string_write::<UTF8>,
    h_copy => copy_body, h_fill => fill_body, h_inspect => inspect_body,
    h_swap16 => swap::<2>, h_swap32 => swap::<4>, h_swap64 => swap::<8>,
    rd_big_i64 => read_big::<true, true>, rd_big_i64_be => read_big::<true, false>, rd_big_i64_le => read_big::<true, true>,
    rd_big_u64 => read_big::<false, true>, rd_big_u64_be => read_big::<false, false>, rd_big_u64_le => read_big::<false, true>,
    rd_double => read_float::<8, true>, rd_double_be => read_float::<8, false>, rd_double_le => read_float::<8, true>,
    rd_float => read_float::<4, true>, rd_float_be => read_float::<4, false>, rd_float_le => read_float::<4, true>,
    rd_i16 => read_fixed::<2, true, true>, rd_i16_be => read_fixed::<2, true, false>, rd_i16_le => read_fixed::<2, true, true>,
    rd_i32 => read_fixed::<4, true, true>, rd_i32_be => read_fixed::<4, true, false>, rd_i32_le => read_fixed::<4, true, true>,
    rd_i8 => read_fixed::<1, true, true>,
    rd_int_be => read_variable::<true, false>, rd_int_le => read_variable::<true, true>,
    rd_u16_be => read_fixed::<2, false, false>, rd_u16_le => read_fixed::<2, false, true>,
    rd_u32_be => read_fixed::<4, false, false>, rd_u32_le => read_fixed::<4, false, true>,
    rd_u8 => read_fixed::<1, false, true>,
    rd_uint_be => read_variable::<false, false>, rd_uint_le => read_variable::<false, true>,
    wr_big_i64_be => write_big::<true, false>, wr_big_i64_le => write_big::<true, true>,
    wr_big_u64_be => write_big::<false, false>, wr_big_u64_le => write_big::<false, true>,
    wr_double => write_float::<8, true>, wr_double_be => write_float::<8, false>, wr_double_le => write_float::<8, true>,
    wr_float => write_float::<4, true>, wr_float_be => write_float::<4, false>, wr_float_le => write_float::<4, true>,
    wr_i16_be => write_fixed::<2, true, false>, wr_i16_le => write_fixed::<2, true, true>,
    wr_i32_be => write_fixed::<4, true, false>, wr_i32_le => write_fixed::<4, true, true>,
    wr_i8 => write_fixed::<1, true, true>,
    wr_int_be => write_variable::<true, false>, wr_int_le => write_variable::<true, true>,
    wr_u16 => write_fixed::<2, false, true>, wr_u16_be => write_fixed::<2, false, false>, wr_u16_le => write_fixed::<2, false, true>,
    wr_u32 => write_fixed::<4, false, true>, wr_u32_be => write_fixed::<4, false, false>, wr_u32_le => write_fixed::<4, false, true>,
    wr_u8 => write_fixed::<1, false, true>,
    wr_uint_be => write_variable::<false, false>, wr_uint_le => write_variable::<false, true>,
}

/// Uma entrada do protótipo: chave, `name` da função, `length` e o corpo.
pub(super) type Method = (&'static str, &'static str, u32, NativeFunction);

thread_local! {
    /// A tabela montada uma vez por thread (as chaves dos apelidos `Uint` são vazadas uma única vez).
    static METHODS: std::cell::OnceCell<Vec<Method>> = const { std::cell::OnceCell::new() };
}

/// Os métodos de `Buffer.prototype` na ordem do bun, com os apelidos `Uint` no fim (o `name` deles é o da forma `UInt`).
/// Ficam de fora os acessores `offset` e `parent`.
pub(super) fn prototype_methods() -> Vec<Method> {
    METHODS.with(|methods| methods.get_or_init(build_methods).clone())
}

fn build_methods() -> Vec<Method> {
    let plain = |key: &'static str, length: u32, function: NativeFunction| -> Method { (key, key, length, function) };
    let base: Vec<Method> = vec![
        plain("asciiSlice", 2, h_ascii_slice),
        plain("asciiWrite", 3, h_ascii_write),
        plain("base64Slice", 2, h_base64_slice),
        plain("base64Write", 3, h_base64_write),
        plain("base64urlSlice", 2, h_base64url_slice),
        plain("base64urlWrite", 3, h_base64url_write),
        plain("compare", 5, buffer_compare_method),
        plain("copy", 4, h_copy),
        plain("equals", 1, buffer_equals),
        plain("fill", 4, h_fill),
        plain("hexSlice", 2, h_hex_slice),
        plain("hexWrite", 3, h_hex_write),
        plain("includes", 3, buffer_includes),
        plain("indexOf", 3, buffer_index_of),
        plain("inspect", 2, h_inspect),
        plain("lastIndexOf", 3, buffer_last_index_of),
        plain("latin1Slice", 2, h_latin1_slice),
        plain("latin1Write", 3, h_latin1_write),
        plain("readBigInt64", 1, rd_big_i64),
        plain("readBigInt64BE", 1, rd_big_i64_be),
        plain("readBigInt64LE", 1, rd_big_i64_le),
        plain("readBigUInt64", 1, rd_big_u64),
        plain("readBigUInt64BE", 1, rd_big_u64_be),
        plain("readBigUInt64LE", 1, rd_big_u64_le),
        plain("readDouble", 1, rd_double),
        plain("readDoubleBE", 1, rd_double_be),
        plain("readDoubleLE", 1, rd_double_le),
        plain("readFloat", 1, rd_float),
        plain("readFloatBE", 1, rd_float_be),
        plain("readFloatLE", 1, rd_float_le),
        plain("readInt16", 1, rd_i16),
        plain("readInt16BE", 1, rd_i16_be),
        plain("readInt16LE", 1, rd_i16_le),
        plain("readInt32", 1, rd_i32),
        plain("readInt32BE", 1, rd_i32_be),
        plain("readInt32LE", 1, rd_i32_le),
        plain("readInt8", 1, rd_i8),
        plain("readIntBE", 2, rd_int_be),
        plain("readIntLE", 2, rd_int_le),
        plain("readUInt16BE", 1, rd_u16_be),
        plain("readUInt16LE", 1, rd_u16_le),
        plain("readUInt32BE", 1, rd_u32_be),
        plain("readUInt32LE", 1, rd_u32_le),
        plain("readUInt8", 1, rd_u8),
        plain("readUIntBE", 2, rd_uint_be),
        plain("readUIntLE", 2, rd_uint_le),
        plain("slice", 2, buffer_subarray),
        plain("subarray", 2, buffer_subarray),
        plain("swap16", 0, h_swap16),
        plain("swap32", 0, h_swap32),
        plain("swap64", 0, h_swap64),
        ("toJSON", "", 0, buffer_to_json),
        ("toLocaleString", "toString", 4, buffer_to_string),
        plain("toString", 4, buffer_to_string),
        plain("ucs2Slice", 2, h_ucs2_slice),
        plain("ucs2Write", 3, h_ucs2_write),
        plain("utf16leSlice", 2, h_ucs2_slice),
        plain("utf16leWrite", 3, h_ucs2_write),
        plain("utf8Slice", 2, h_utf8_slice),
        plain("utf8Write", 3, h_utf8_write),
        plain("write", 4, buffer_write),
        plain("writeBigInt64BE", 3, wr_big_i64_be),
        plain("writeBigInt64LE", 3, wr_big_i64_le),
        plain("writeBigUInt64BE", 3, wr_big_u64_be),
        plain("writeBigUInt64LE", 3, wr_big_u64_le),
        plain("writeDouble", 2, wr_double),
        plain("writeDoubleBE", 2, wr_double_be),
        plain("writeDoubleLE", 2, wr_double_le),
        plain("writeFloat", 2, wr_float),
        plain("writeFloatBE", 2, wr_float_be),
        plain("writeFloatLE", 2, wr_float_le),
        plain("writeInt16BE", 2, wr_i16_be),
        plain("writeInt16LE", 2, wr_i16_le),
        plain("writeInt32BE", 2, wr_i32_be),
        plain("writeInt32LE", 2, wr_i32_le),
        plain("writeInt8", 2, wr_i8),
        plain("writeIntBE", 3, wr_int_be),
        plain("writeIntLE", 3, wr_int_le),
        plain("writeUInt16", 2, wr_u16),
        plain("writeUInt16BE", 2, wr_u16_be),
        plain("writeUInt16LE", 2, wr_u16_le),
        plain("writeUInt32", 2, wr_u32),
        plain("writeUInt32BE", 2, wr_u32_be),
        plain("writeUInt32LE", 2, wr_u32_le),
        plain("writeUInt8", 2, wr_u8),
        plain("writeUIntBE", 3, wr_uint_be),
        plain("writeUIntLE", 3, wr_uint_le),
    ];
    const ALIASES: [&str; 20] = [
        "readUIntBE", "readUIntLE", "readUInt8", "readUInt16BE", "readUInt16LE", "readUInt32BE", "readUInt32LE", "readBigUInt64BE", "readBigUInt64LE",
        "writeUIntBE", "writeUIntLE", "writeUInt8", "writeUInt16", "writeUInt16BE", "writeUInt16LE", "writeUInt32", "writeUInt32BE", "writeUInt32LE",
        "writeBigUInt64BE", "writeBigUInt64LE",
    ];
    let aliases: Vec<Method> = ALIASES
        .iter()
        .map(|name| {
            let &(key, display, length, function) = base.iter().find(|entry| entry.0 == *name).expect("apelido de método que existe");
            (Box::leak(key.replace("UInt", "Uint").into_boxed_str()) as &'static str, display, length, function)
        })
        .collect();
    base.into_iter().chain(aliases).collect()
}
