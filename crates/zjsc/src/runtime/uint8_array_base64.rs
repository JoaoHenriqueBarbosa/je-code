//! Porte de `Uint8Array.fromBase64`, `Uint8Array.fromHex` (`runtime/JSGenericTypedArrayViewConstructor.cpp`) e
//! de `Uint8Array.prototype.setFromBase64`, `setFromHex`, `toBase64` e `toHex`
//! (`runtime/JSGenericTypedArrayViewPrototype.cpp`): https://tc39.es/proposal-arraybuffer-base64/spec/.
//!
//! A decodificação e a codificação base64 estão em `wtf/text/base64.rs`.
//!
//! DIVERGÊNCIAS:
//!
//! - `decodeHex` do C++ é vetorial (SIMD) com recuo para o laço escalar; aqui é só o laço escalar, que escreve
//!   os mesmos bytes (os pares válidos antes do primeiro caractere ruim) e falha no mesmo ponto.
//! - O `throwOutOfMemoryError(globalObject, scope, "generated string is too long"_s)` de `toBase64` e `toHex`
//!   é `Thrown::OutOfMemory`, que não leva mensagem própria.

use ul_common::codec::hex_lower;
use ul_common::ctype::hex_value;

use crate::host_function;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::put_direct_native_function_without_transition;
use crate::runtime::js_generic_typed_array_view::{check_typed_array_in_bounds, JSGenericTypedArrayView, JSGenericTypedArrayViewRef};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_module_record::throw_syntax_error;
use crate::runtime::js_object::{JSFinalObject, JSObject};
use crate::runtime::js_string::{js_empty_string, js_string};
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::identifier::Identifier;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::string_regexp_support::get_object_property;
use crate::runtime::typed_array_type::TypedArrayType;
use crate::runtime::vm::VM;
use crate::wtf::text::base64::{
    base64_encode_to_string_return_none_if_overflow, from_base64, max_length_from_base64, Alphabet, FromBase64ShouldThrowError,
    LastChunkHandling, OutputSizeIsMaxLength,
};
use crate::wtf::text::string_impl::StringImpl;
use crate::wtf::text::wtf_string::String as WtfString;

/// `WTF::String == "literal"_s`.
fn string_equals(string: &WtfString, text: &str) -> bool {
    if string.is_8bit() {
        return string.span8() == text.as_bytes();
    }
    let units = string.span16();
    units.len() == text.len() && units.iter().zip(text.bytes()).all(|(unit, byte)| *unit == u16::from(byte))
}

/// `dynamicDowncast<JSUint8Array>(callFrame->thisValue())`, ou o `TypeError` de cada método.
fn this_uint8_array(call: &HostCall, method: &str) -> Result<JSGenericTypedArrayViewRef, Thrown> {
    match JSGenericTypedArrayView::from_value(&call.this_value()) {
        Some(view) if view.typed_array_type() == TypedArrayType::Uint8 => Ok(view),
        _ => Err(Thrown::type_error(&format!("{method} requires that |this| be a Uint8Array"))),
    }
}

/// `dynamicDowncast<JSString>(argument)`: o texto, ou o `TypeError` com a mensagem dada.
fn string_value(value: JSValue, message: &str) -> Result<WtfString, Thrown> {
    if !value.is_string() {
        return Err(Thrown::type_error(message));
    }
    Ok(value.as_js_string().value())
}

/// Lê `options.alphabet` (a lógica repetida em `fromBase64`, `setFromBase64` e `toBase64`).
fn read_alphabet(global_object: &JSGlobalObject, options: JSValue, method: &str) -> Result<Alphabet, Thrown> {
    let alphabet_value = get_object_property(global_object, options, &global_object.vm().property_names.alphabet)?;
    if alphabet_value.is_undefined() {
        return Ok(Alphabet::Base64);
    }
    let message = format!("{method} requires that alphabet be \"base64\" or \"base64url\"");
    let alphabet = string_value(alphabet_value, &message)?;
    if string_equals(&alphabet, "base64url") {
        return Ok(Alphabet::Base64URL);
    }
    if !string_equals(&alphabet, "base64") {
        return Err(Thrown::type_error(&message));
    }
    Ok(Alphabet::Base64)
}

/// Lê `options.lastChunkHandling`.
fn read_last_chunk_handling(global_object: &JSGlobalObject, options: JSValue, method: &str) -> Result<LastChunkHandling, Thrown> {
    let value = get_object_property(global_object, options, &global_object.vm().property_names.last_chunk_handling)?;
    if value.is_undefined() {
        return Ok(LastChunkHandling::Loose);
    }
    let message = format!("{method} requires that lastChunkHandling be \"loose\", \"strict\", or \"stop-before-partial\"");
    let handling = string_value(value, &message)?;
    if string_equals(&handling, "strict") {
        return Ok(LastChunkHandling::Strict);
    }
    if string_equals(&handling, "stop-before-partial") {
        return Ok(LastChunkHandling::StopBeforePartial);
    }
    if !string_equals(&handling, "loose") {
        return Err(Thrown::type_error(&message));
    }
    Ok(LastChunkHandling::Loose)
}

/// O argumento `options` de `fromBase64` e `setFromBase64`: `alphabet` e `lastChunkHandling`.
fn read_decode_options(global_object: &JSGlobalObject, options: JSValue, method: &str) -> Result<(Alphabet, LastChunkHandling), Thrown> {
    if options.is_undefined() {
        return Ok((Alphabet::Base64, LastChunkHandling::Loose));
    }
    if !options.is_object() {
        return Err(Thrown::type_error(&format!("{method} requires that options be an object")));
    }
    let alphabet = read_alphabet(global_object, options, method)?;
    let last_chunk_handling = read_last_chunk_handling(global_object, options, method)?;
    Ok((alphabet, last_chunk_handling))
}

/// `decodeHex(span, result)`: `true` é o `WTF::notFound` (tudo decodificado). Escreve os pares válidos antes do
/// primeiro caractere que não é hexadecimal.
fn decode_hex<T: Copy + Into<u32>>(span: &[T], result: &mut [u8]) -> bool {
    debug_assert!(span.len() == result.len() * 2);
    let digit = |unit: T| u8::try_from(Into::<u32>::into(unit)).ok().and_then(hex_value);
    for (pair, byte) in span.chunks_exact(2).zip(result.iter_mut()) {
        let Some(tens) = digit(pair[0]) else { return false };
        let Some(ones) = digit(pair[1]) else { return false };
        *byte = (tens * 16 + ones) as u8;
    }
    true
}

/// `decodeHex` sobre o texto, 8 ou 16 bits.
fn decode_hex_string(string: &WtfString, result: &mut [u8]) -> bool {
    if string.is_8bit() {
        return decode_hex(string.span8(), result);
    }
    decode_hex(string.span16(), result)
}

/// `JSUint8Array::createUninitialized(globalObject, typedArrayStructure(TypeUint8, false), length)`.
pub(crate) fn create_uint8_array(global_object: &JSGlobalObject, length: usize) -> Result<JSGenericTypedArrayViewRef, Thrown> {
    let structure = global_object.array_buffer_realm.typed_arrays.structure(TypedArrayType::Uint8, false);
    JSGenericTypedArrayView::create_uninitialized(global_object, &structure, length)
}

fn uint8_array_constructor_from_base64_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    const METHOD: &str = "Uint8Array.fromBase64";
    let string = string_value(call.argument(0), "Uint8Array.fromBase64 requires a string")?;
    let (alphabet, last_chunk_handling) = read_decode_options(global_object, call.argument(1), METHOD)?;

    let mut output = crate::runtime::fallible_alloc::try_filled_vec(0u8, max_length_from_base64(&string))
        .ok_or(Thrown::OutOfMemory)?;
    let (should_throw_error, read_length, write_length) =
        from_base64(&string, &mut output, alphabet, last_chunk_handling, OutputSizeIsMaxLength::No);
    if should_throw_error == FromBase64ShouldThrowError::Yes {
        return Err(throw_syntax_error(global_object, "Uint8Array.fromBase64 requires a valid base64 string"));
    }
    debug_assert!(read_length <= string.length() as usize);

    let array = create_uint8_array(global_object, write_length)?;
    array.with_vector_mut(|bytes| bytes[..write_length].copy_from_slice(&output[..write_length]));
    Ok(array.as_value())
}

fn uint8_array_constructor_from_hex_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let string = string_value(call.argument(0), "Uint8Array.fromHex requires a string")?;
    if string.length() % 2 != 0 {
        return Err(throw_syntax_error(global_object, "Uint8Array.fromHex requires a string of even length"));
    }

    let count = (string.length() / 2) as usize;
    let array = create_uint8_array(global_object, count)?;
    let success = array.with_vector_mut(|bytes| decode_hex_string(&string, &mut bytes[..count]));
    if !success {
        return Err(throw_syntax_error(
            global_object,
            "Uint8Array.prototype.fromHex requires a string containing only \"0123456789abcdefABCDEF\"",
        ));
    }
    Ok(array.as_value())
}

/// O objeto `{ read, written }` que `setFromBase64` e `setFromHex` devolvem.
pub(crate) fn read_written_object(global_object: &JSGlobalObject, read: usize, written: usize) -> JSValue {
    let vm = global_object.vm();
    let result = JSFinalObject::create(vm, &global_object.object_structure_for_object_constructor());
    result.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.read), js_number(read as f64), 0);
    result.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.written), js_number(written as f64), 0);
    result.as_value()
}

fn uint8_array_prototype_set_from_base64_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    const METHOD: &str = "Uint8Array.prototype.setFromBase64";
    let view = this_uint8_array(call, METHOD)?;
    let string = string_value(call.argument(0), "Uint8Array.prototype.setFromBase64 requires a string")?;
    let (alphabet, last_chunk_handling) = read_decode_options(global_object, call.argument(1), METHOD)?;

    check_typed_array_in_bounds(&view)?;

    let length = view.length();
    let (should_throw_error, read_length, write_length) = view.with_vector_mut(|bytes| {
        from_base64(&string, &mut bytes[..length], alphabet, last_chunk_handling, OutputSizeIsMaxLength::Yes)
    });
    debug_assert!(read_length <= string.length() as usize);
    if should_throw_error == FromBase64ShouldThrowError::Yes {
        return Err(throw_syntax_error(global_object, "Uint8Array.prototype.setFromBase64 requires a valid base64 string"));
    }
    Ok(read_written_object(global_object, read_length, write_length))
}

fn uint8_array_prototype_set_from_hex_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let view = this_uint8_array(call, "Uint8Array.prototype.setFromHex")?;
    check_typed_array_in_bounds(&view)?;

    let string = string_value(call.argument(0), "Uint8Array.prototype.setFromHex requires a string")?;
    if string.length() % 2 != 0 {
        return Err(throw_syntax_error(global_object, "Uint8Array.prototype.setFromHex requires a string of even length"));
    }

    let written_count = ((string.length() / 2) as usize).min(view.length());
    let read_count = written_count * 2;
    let success = view.with_vector_mut(|bytes| {
        let result = &mut bytes[..written_count];
        if string.is_8bit() {
            return decode_hex(&string.span8()[..read_count], result);
        }
        decode_hex(&string.span16()[..read_count], result)
    });
    if !success {
        return Err(throw_syntax_error(
            global_object,
            "Uint8Array.prototype.setFromHex requires a string containing only \"0123456789abcdefABCDEF\"",
        ));
    }
    Ok(read_written_object(global_object, read_count, written_count))
}

fn uint8_array_prototype_to_base64_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    const METHOD: &str = "Uint8Array.prototype.toBase64";
    let vm = global_object.vm();
    let view = this_uint8_array(call, METHOD)?;

    let mut url = false;
    let mut omit_padding = false;
    let options = call.argument(0);
    if !options.is_undefined() {
        if !options.is_object() {
            return Err(Thrown::type_error("Uint8Array.prototype.toBase64 requires that options be an object"));
        }
        url = read_alphabet(global_object, options, METHOD)? == Alphabet::Base64URL;
        omit_padding = get_object_property(global_object, options, &vm.property_names.omit_padding)?.to_boolean();
    }

    check_typed_array_in_bounds(&view)?;

    let length = view.length();
    let result = view.with_vector(|bytes| base64_encode_to_string_return_none_if_overflow(&bytes[..length], url, omit_padding));
    let Some(result) = result else {
        return Err(Thrown::OutOfMemory);
    };
    Ok(JSValue::from_js_string(js_string(vm, &result)))
}

fn uint8_array_prototype_to_hex_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let view = this_uint8_array(call, "Uint8Array.prototype.toHex")?;
    check_typed_array_in_bounds(&view)?;

    let length = view.length();
    if length == 0 {
        return Ok(JSValue::from_js_string(js_empty_string(vm)));
    }

    let Some(result_length) = length.checked_mul(2).filter(|length| StringImpl::is_valid_length::<u8>(*length)) else {
        return Err(Thrown::OutOfMemory);
    };
    debug_assert!(result_length == length * 2);
    let digits = view.with_vector(|bytes| hex_lower(&bytes[..length]));
    Ok(JSValue::from_js_string(js_string(vm, &WtfString::from_latin1(digits.as_bytes()))))
}

host_function!(uint8_array_constructor_from_base64, uint8_array_constructor_from_base64_body);
host_function!(uint8_array_constructor_from_hex, uint8_array_constructor_from_hex_body);
host_function!(uint8_array_prototype_set_from_base64, uint8_array_prototype_set_from_base64_body);
host_function!(uint8_array_prototype_set_from_hex, uint8_array_prototype_set_from_hex_body);
host_function!(uint8_array_prototype_to_base64, uint8_array_prototype_to_base64_body);
host_function!(uint8_array_prototype_to_hex, uint8_array_prototype_to_hex_body);

/// `JSC_NATIVE_FUNCTION_WITHOUT_TRANSITION(name, function, DontEnum, length, Public)` para cada entrada.
fn install_native_functions(vm: &VM, global_object: &JSGlobalObject, target: &JSObject, functions: &[(&str, u32, NativeFunction)]) {
    for (name, length, function) in functions {
        put_direct_native_function_without_transition(
            vm,
            global_object,
            target,
            &Identifier::from_span(vm, name.as_bytes()),
            *length,
            *function,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            DONT_ENUM,
        );
    }
}

/// O `if constexpr (std::is_same_v<ViewClass, JSUint8Array>)` de
/// `JSGenericTypedArrayViewConstructor::finishCreation`: `fromBase64` e `fromHex` no `Uint8Array`.
pub fn install_constructor_functions(vm: &VM, global_object: &JSGlobalObject, constructor: &JSObject) {
    install_native_functions(
        vm,
        global_object,
        constructor,
        &[("fromBase64", 1, uint8_array_constructor_from_base64), ("fromHex", 1, uint8_array_constructor_from_hex)],
    );
}

/// O `if constexpr (std::is_same_v<ViewClass, JSUint8Array>)` de
/// `JSGenericTypedArrayViewPrototype::finishCreation`: `setFromBase64`, `setFromHex`, `toBase64` e `toHex` no
/// `Uint8Array.prototype`.
pub fn install_prototype_functions(vm: &VM, global_object: &JSGlobalObject, prototype: &JSObject) {
    install_native_functions(
        vm,
        global_object,
        prototype,
        &[
            ("setFromBase64", 1, uint8_array_prototype_set_from_base64),
            ("setFromHex", 1, uint8_array_prototype_set_from_hex),
            ("toBase64", 0, uint8_array_prototype_to_base64),
            ("toHex", 0, uint8_array_prototype_to_hex),
        ],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_equals_compares_8_and_16_bit_text() {
        assert!(string_equals(&WtfString::from_latin1(b"base64url"), "base64url"));
        assert!(!string_equals(&WtfString::from_latin1(b"base64"), "base64url"));
        assert!(!string_equals(&WtfString::from_latin1(b"Base64"), "base64"));
        assert!(!string_equals(&WtfString::from_latin1(b""), "strict"));
    }

    #[test]
    fn decode_hex_accepts_both_cases() {
        let mut out = [0u8; 4];
        assert!(decode_hex(b"cafeBABE".as_slice(), &mut out));
        assert_eq!(out, [0xca, 0xfe, 0xba, 0xbe]);
        let wide: Vec<u16> = "00ff10Ab".encode_utf16().collect();
        assert!(decode_hex(wide.as_slice(), &mut out));
        assert_eq!(out, [0x00, 0xff, 0x10, 0xab]);
    }

    #[test]
    fn decode_hex_writes_valid_pairs_before_the_first_bad_character() {
        // Spec de `setFromHex`: "aabbzzcc" grava os dois primeiros bytes e falha no terceiro par.
        let mut out = [0u8; 4];
        assert!(!decode_hex(b"aabbzzcc".as_slice(), &mut out));
        assert_eq!(out, [0xaa, 0xbb, 0, 0]);
        // Um par com o segundo dígito ruim não grava o byte.
        let mut out = [0u8; 2];
        assert!(!decode_hex(b"aag0".as_slice(), &mut out));
        assert_eq!(out, [0xaa, 0]);
    }

    #[test]
    fn decode_hex_rejects_code_units_above_latin1() {
        let mut out = [0u8; 1];
        let wide: Vec<u16> = vec![0x0161, u16::from(b'a')];
        assert!(!decode_hex(wide.as_slice(), &mut out));
        // 0x0130 & 0xdf não pode mascarar como dígito: acima de 255 nunca é hexadecimal.
        let wide: Vec<u16> = vec![u16::from(b'a'), 0x0141];
        assert!(!decode_hex(wide.as_slice(), &mut out));
    }

    #[test]
    fn decode_hex_empty_is_success() {
        let mut out = [0u8; 0];
        assert!(decode_hex::<u8>(&[], &mut out));
    }
}
