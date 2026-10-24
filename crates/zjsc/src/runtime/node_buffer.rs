//! O global `Buffer` do bun (`node:buffer`). Forma medida no bun 1.4.2 (detalhes e o que falta em
//! `wip/notes/buffer-plan.md`):
//!
//! - `Buffer` é uma subclasse de `Uint8Array`: o protótipo do construtor é `Uint8Array`, `Buffer.prototype`
//!   herda de `Uint8Array.prototype`, e as instâncias são `JSUint8Array` com `Buffer.prototype`;
//! - `length` 3, `name` "Buffer"; `Buffer(3)` sem `new` também funciona (cai em `Buffer.alloc`);
//! - `Buffer.from(string, encoding)`: `utf8` (padrão, `undefined` e `null` também), `hex`, `base64`,
//!   `base64url`, `latin1`/`binary`, `ascii`, `ucs2`/`utf16le`, sem diferenciar maiúscula de minúscula;
//!   encoding desconhecido: `TypeError` com `code` `ERR_UNKNOWN_ENCODING` e `Unknown encoding: x`;
//! - `hex` para no primeiro par inválido ou no dígito solto; `base64` aceita os dois alfabetos, ignora o que
//!   não é do alfabeto e para no primeiro `=`;
//! - `toString(encoding, start, end)`: `ascii` zera o bit alto, `utf8` troca byte inválido por U+FFFD,
//!   `utf16le` descarta o byte solto do fim.

use std::cell::{Cell, RefCell};

mod access;

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::exception_helpers::error_description_for_value;
use crate::runtime::js_array_buffer::JSArrayBuffer;
use crate::runtime::js_array::{construct_array, is_js_array};
use crate::runtime::object_constructor::construct_empty_object;
use crate::runtime::js_value::{js_boolean, js_number};
use crate::runtime::node_error::{throw_coded_error, throw_coded_range_error, throw_error_with_properties, NetworkProperty};
use crate::runtime::blob::make_blob;
use crate::runtime::js_data_view::JSDataView;
use crate::runtime::text_decoder::input_bytes;
use crate::runtime::url::object_url_state;
use crate::runtime::string_regexp_support::{get_object_index_u64, get_object_property};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::put_direct_native_function_with_display_name;
use crate::runtime::js_generic_typed_array_view::{JSGenericTypedArrayView, JSGenericTypedArrayViewRef};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JS_NON_FINAL_OBJECT_S_INFO;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::JSValue;
use crate::runtime::native_class_support::{create_native_subclass_with_hooks, install_global, property_key, throw_coded_type_error};
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::structure::StructureRef;
use crate::runtime::typed_array_type::TypedArrayType;
use crate::wtf::text::wtf_string::String as WtfString;
use ul_common::codec::{base64_encode, hex_lower, BASE64_STANDARD, BASE64_URL};

/// `const ClassInfo` do protótipo.
static BUFFER_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Buffer", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `const ClassInfo` do construtor (`"Function"`).
pub static BUFFER_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

thread_local! {
    /// A estrutura das instâncias (`JSUint8Array` com `Buffer.prototype`), do programa corrente.
    static BUFFER_STRUCTURE: RefCell<Option<StructureRef>> = const { RefCell::new(None) };
    /// `Buffer.prototype` do programa corrente (o `Buffer.isBuffer` procura por ele na cadeia).
    static BUFFER_PROTOTYPE: RefCell<Option<JSValue>> = const { RefCell::new(None) };
    /// `require("buffer").INSPECT_MAX_BYTES`: quantos bytes o `inspect` de um `Buffer` mostra (padrão 50).
    static INSPECT_MAX_BYTES: Cell<f64> = const { Cell::new(DEFAULT_INSPECT_MAX_BYTES) };
}

/// O valor inicial de `INSPECT_MAX_BYTES`.
const DEFAULT_INSPECT_MAX_BYTES: f64 = 50.0;

/// O valor corrente de `INSPECT_MAX_BYTES` (o setter do módulo `buffer` o altera).
pub(crate) fn inspect_max_bytes() -> f64 {
    INSPECT_MAX_BYTES.with(Cell::get)
}

/// Fim do programa (`cell_registry::reset_program_state`): a estrutura e o protótipo guardados são do programa.
/// As raízes do coletor: o protótipo do `Buffer`.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    marker.value(BUFFER_PROTOTYPE.with(|prototype| *prototype.borrow()));
}

pub(crate) fn reset_for_program() {
    let _ = INSPECT_MAX_BYTES.try_with(|max| max.set(DEFAULT_INSPECT_MAX_BYTES));
    let _ = BUFFER_STRUCTURE.try_with(|structure| structure.borrow_mut().take());
    let _ = BUFFER_PROTOTYPE.try_with(|prototype| prototype.borrow_mut().take());
}

/// O texto do motor como `String` do Rust (cada unidade vira um `char`; par substituto solto vira U+FFFD).
fn wtf_to_rust(text: &WtfString) -> String {
    (0..text.length()).map(|index| char::from_u32(u32::from(text.code_unit_at(index))).unwrap_or('\u{fffd}')).collect()
}

/// As codificações de `Buffer`, depois de normalizar o nome.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Encoding {
    Utf8,
    Hex,
    Base64,
    Base64Url,
    Latin1,
    Ascii,
    Utf16Le,
}

impl Encoding {
    /// O nome (sem diferenciar maiúscula de minúscula) como o `Buffer.isEncoding` do bun o reconhece.
    pub(crate) fn parse(name: &str) -> Option<Encoding> {
        Some(match name.to_ascii_lowercase().as_str() {
            "utf8" | "utf-8" => Encoding::Utf8,
            "hex" => Encoding::Hex,
            "base64" => Encoding::Base64,
            "base64url" => Encoding::Base64Url,
            "latin1" | "binary" => Encoding::Latin1,
            "ascii" => Encoding::Ascii,
            "ucs2" | "ucs-2" | "utf16le" | "utf-16le" => Encoding::Utf16Le,
            _ => return None,
        })
    }
}

/// Lê o argumento de encoding: ausente, `undefined` e `null` são `utf8`; texto desconhecido lança
/// `ERR_UNKNOWN_ENCODING`.
fn encoding_argument(global_object: &JSGlobalObject, value: JSValue) -> Result<Encoding, crate::runtime::host_call::Thrown> {
    if value.is_undefined_or_null() {
        return Ok(Encoding::Utf8);
    }
    let text = pending_or(global_object, value.to_wtf_string())?;
    let name = wtf_to_rust(&text);
    Encoding::parse(&name).ok_or_else(|| throw_coded_type_error(global_object, &format!("Unknown encoding: {name}"), "ERR_UNKNOWN_ENCODING"))
}

fn hex_digit(byte: u8) -> Option<u8> {
    char::from(byte).to_digit(16).map(|digit| digit as u8)
}

/// `hex` do Node: pares de dígitos até o primeiro par inválido; dígito solto no fim é descartado. Cada unidade conta
/// só pelo byte baixo (`U+0131` vale `1`, como no bun).
fn decode_hex(units: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(units.len() / 2);
    for pair in units.chunks_exact(2) {
        let digit = |unit: u16| hex_digit(unit as u8);
        match (digit(pair[0]), digit(pair[1])) {
            (Some(high), Some(low)) => out.push((high << 4) | low),
            _ => break,
        }
    }
    out
}

/// `base64` e `base64url` do Node: os dois alfabetos valem, o que não é do alfabeto é ignorado e o primeiro `=`
/// encerra a leitura. Cada unidade conta só pelo byte baixo (`U+FF41` vale `A`, como no bun).
fn decode_base64(units: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(units.len() / 4 * 3 + 3);
    let (mut accumulator, mut bits) = (0u32, 0u32);
    for &unit in units {
        let value = match unit as u8 {
            byte @ b'A'..=b'Z' => byte - b'A',
            byte @ b'a'..=b'z' => byte - b'a' + 26,
            byte @ b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            _ => continue,
        };
        accumulator = (accumulator << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((accumulator >> bits) as u8);
            accumulator &= (1 << bits) - 1;
        }
    }
    out
}

/// `Buffer.byteLength(texto, 'base64')`: tira até dois `=` do fim e conta pelo tamanho, sem olhar o conteúdo
/// (`'Y*W!'` vale 3), como o `base64_decoded_size` do Node.
fn base64_decoded_size(units: &[u16]) -> usize {
    let mut size = units.len();
    for _ in 0..2 {
        if size > 0 && units[size - 1] as u8 == b'=' {
            size -= 1;
        }
    }
    size / 4 * 3 + (size % 4).saturating_sub(1)
}

/// Os bytes que `encoding` faz de `units` (o texto em unidades UTF-16).
fn encode_units(units: &[u16], encoding: Encoding) -> Vec<u8> {
    match encoding {
        Encoding::Utf8 => String::from_utf16_lossy(units).into_bytes(),
        Encoding::Hex => decode_hex(units),
        Encoding::Base64 | Encoding::Base64Url => decode_base64(units),
        Encoding::Latin1 | Encoding::Ascii => units.iter().map(|&unit| unit as u8).collect(),
        Encoding::Utf16Le => units.iter().flat_map(|unit| unit.to_le_bytes()).collect(),
    }
}

/// O texto de `bytes` em `encoding`, como string do motor.
fn decode_bytes(bytes: &[u8], encoding: Encoding) -> WtfString {
    match encoding {
        Encoding::Utf8 => {
            let units: Vec<u16> = String::from_utf8_lossy(bytes).encode_utf16().collect();
            WtfString::from_utf16(&units)
        }
        Encoding::Hex => WtfString::from_latin1(hex_lower(bytes).as_bytes()),
        Encoding::Base64 => WtfString::from_latin1(&base64_encode(bytes, BASE64_STANDARD, true)),
        Encoding::Base64Url => WtfString::from_latin1(&base64_encode(bytes, BASE64_URL, false)),
        Encoding::Latin1 => WtfString::from_latin1(bytes),
        Encoding::Ascii => WtfString::from_latin1(&bytes.iter().map(|byte| byte & 0x7f).collect::<Vec<u8>>()),
        Encoding::Utf16Le => {
            let units: Vec<u16> = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
            WtfString::from_utf16(&units)
        }
    }
}

/// Um `Buffer` novo com os `bytes`.
pub(crate) fn buffer_from_bytes(global_object: &JSGlobalObject, bytes: &[u8]) -> HostResult {
    let structure = BUFFER_STRUCTURE.with(|slot| slot.borrow().clone()).expect("Buffer chamado antes da instalação");
    let view = JSGenericTypedArrayView::create(global_object, &structure, bytes.len())?;
    view.with_vector_mut(|destination| destination[..bytes.len()].copy_from_slice(bytes));
    Ok(view.as_value())
}

/// Um número como o `ERR_*` do Node o mostra: `-0` vira `0`, `NaN` e infinitos pelo nome, inteiro acima de 2^32 com
/// `_` a cada três dígitos.
fn number_text(number: f64) -> String {
    if number.is_nan() {
        return "NaN".to_string();
    }
    if number.is_infinite() {
        return if number > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if number == 0.0 {
        return "0".to_string();
    }
    let text = wtf_to_rust(&js_number(number).to_wtf_string());
    if number.fract() == 0.0 && number.abs() > 4_294_967_296.0 {
        return with_separators(&text);
    }
    text
}

/// `addNumericalSeparator` do Node: grupos de três contados do fim do texto do número (inclusive em `1.5e+300`).
fn with_separators(text: &str) -> String {
    let start = usize::from(text.starts_with('-'));
    let mut end = text.len();
    let mut grouped = String::new();
    while end >= start + 4 {
        grouped.insert_str(0, &format!("_{}", &text[end - 3..end]));
        end -= 3;
    }
    format!("{}{grouped}", &text[..end])
}

/// `ERR_INVALID_ARG_TYPE` de argumento nomeado: `The "name" argument must be {expected}. Received ...`.
fn invalid_argument_type(global_object: &JSGlobalObject, name: &str, expected: &str, value: JSValue) -> Thrown {
    let message = format!("The \"{name}\" argument must be {expected}. Received {}", received_description(global_object, value));
    throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE")
}

/// `ERR_OUT_OF_RANGE` com o trecho `It must be {rule}.`.
fn out_of_range(global_object: &JSGlobalObject, name: &str, rule: &str, number: f64) -> Thrown {
    let message = format!("The value of \"{name}\" is out of range. It must be {rule}. Received {}", number_text(number));
    throw_coded_range_error(global_object, &message, "ERR_OUT_OF_RANGE")
}

/// `validateInteger(value, name, 0, max)` do Node: número inteiro entre 0 e `max`.
fn validate_integer(global_object: &JSGlobalObject, value: JSValue, name: &str, max: f64) -> Result<f64, Thrown> {
    if !value.is_number() {
        return Err(invalid_argument_type(global_object, name, "of type number", value));
    }
    let number = value.as_number();
    if !number.is_finite() || number.fract() != 0.0 {
        return Err(out_of_range(global_object, name, "an integer", number));
    }
    if number < 0.0 || number > max {
        return Err(out_of_range(global_object, name, &format!(">= 0 && <= {max}"), number));
    }
    Ok(number)
}

/// Os bytes de um `Buffer`/`Uint8Array` passado como argumento `name`; outra coisa lança `ERR_INVALID_ARG_TYPE`.
fn uint8_bytes_argument(global_object: &JSGlobalObject, value: JSValue, name: &str) -> Result<Vec<u8>, Thrown> {
    match JSGenericTypedArrayView::from_value(&value).filter(|view| view.typed_array_type() == TypedArrayType::Uint8) {
        Some(view) => Ok(view.with_view_bytes(|bytes| bytes.to_vec())),
        None => Err(invalid_argument_type(global_object, name, "an instance of Buffer or Uint8Array", value)),
    }
}

/// O limite de `validateOffset` em `Buffer.concat` e do tamanho de `alloc`.
const MAX_BUFFER_LENGTH: f64 = 4_294_967_296.0;

/// O maior inteiro seguro, limite de `offset`/`length` de `Buffer.copyBytesFrom`.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// O trecho `Received ...` das mensagens `ERR_INVALID_ARG_TYPE` do Node, para o valor recebido.
fn received_description(global_object: &JSGlobalObject, value: JSValue) -> String {
    if value.is_undefined() {
        "undefined".to_string()
    } else if value.is_null() {
        "null".to_string()
    } else if value.is_number() {
        format!("type number ({})", number_text(value.as_number()))
    } else if value.is_boolean() {
        format!("type boolean ({})", value.as_boolean())
    } else if value.is_string() {
        format!("type string ('{}')", wtf_to_rust(&value.to_wtf_string()))
    } else if value.is_symbol() {
        format!("type symbol ({})", wtf_to_rust(&error_description_for_value(value)))
    } else if value.is_big_int() {
        format!("type bigint ({}n)", wtf_to_rust(&value.to_wtf_string()))
    } else if value.is_callable() {
        let name = get_object_property(global_object, value, &global_object.vm().property_names.name).ok().filter(|name| name.is_string());
        format!("function {}", name.map(|name| wtf_to_rust(&name.to_wtf_string())).unwrap_or_default())
    } else {
        format!("an instance of {}", wtf_to_rust(&error_description_for_value(value)))
    }
}

/// A mensagem de `ERR_INVALID_ARG_TYPE` de `Buffer.from`.
fn invalid_first_argument(global_object: &JSGlobalObject, value: JSValue) -> Thrown {
    const PREFIX: &str = "The first argument must be of type string or an instance of Buffer, ArrayBuffer, or Array or an Array-like Object. Received ";
    throw_coded_type_error(global_object, &format!("{PREFIX}{}", received_description(global_object, value)), "ERR_INVALID_ARG_TYPE")
}

/// `ERR_BUFFER_OUT_OF_BOUNDS` de `Buffer.from(arrayBuffer, byteOffset, length)`.
fn out_of_bounds(global_object: &JSGlobalObject, what: &str) -> Thrown {
    throw_coded_range_error(global_object, &format!("\"{what}\" is outside of buffer bounds"), "ERR_BUFFER_OUT_OF_BOUNDS")
}

/// `Buffer.from(arrayBuffer, byteOffset, length)`: uma visão que compartilha a memória do `ArrayBuffer`.
fn buffer_over_array_buffer(global_object: &JSGlobalObject, array_buffer: &JSArrayBuffer, call: &HostCall) -> HostResult {
    let buffer = array_buffer.impl_().clone();
    let total = buffer.byte_length();
    let offset_argument = call.argument(1);
    let offset = if offset_argument.is_undefined() { 0.0 } else { offset_argument.to_number() };
    let offset = if offset.is_nan() { 0.0 } else { offset.trunc() };
    if offset < 0.0 || offset > total as f64 {
        return Err(out_of_bounds(global_object, "offset"));
    }
    let offset = offset as usize;
    let length_argument = call.argument(2);
    let length = if length_argument.is_undefined() {
        total - offset
    } else {
        let requested = length_argument.to_number();
        if requested > 0.0 {
            if requested > (total - offset) as f64 {
                return Err(out_of_bounds(global_object, "length"));
            }
            requested as usize
        } else {
            0
        }
    };
    let structure = BUFFER_STRUCTURE.with(|slot| slot.borrow().clone()).expect("Buffer chamado antes da instalação");
    Ok(JSGenericTypedArrayView::create_with_buffer(global_object, &structure, buffer, offset, Some(length))?.as_value())
}

/// `Buffer.from(arrayLike)`: cada elemento vira um byte (módulo 256).
fn buffer_from_array_like(global_object: &JSGlobalObject, object: JSValue, length: f64) -> HostResult {
    let count = if length > 0.0 { length as usize } else { 0 };
    let mut bytes = Vec::with_capacity(count.min(1 << 20));
    for index in 0..count {
        bytes.push(get_object_index_u64(global_object, object, index as u64)?.to_uint32() as u8);
    }
    buffer_from_bytes(global_object, &bytes)
}

/// `Buffer.from(value, encoding)`: string, `ArrayBuffer` (compartilha), `Uint8Array`/`Buffer` (cópia), outra visão ou
/// objeto com `length` numérico (cada elemento módulo 256) e o `{ type: 'Buffer', data }` do `toJSON`.
fn buffer_from_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let value = call.argument(0);
    if value.is_string() {
        let encoding = encoding_argument(global_object, call.argument(1))?;
        let units = crate::runtime::text_encoder::string_units(global_object, value)?;
        return buffer_from_bytes(global_object, &encode_units(&units, encoding));
    }
    if let Some(array_buffer) = JSArrayBuffer::from_value(&value) {
        return buffer_over_array_buffer(global_object, &array_buffer, call);
    }
    if let Some(view) = JSGenericTypedArrayView::from_value(&value) {
        if view.typed_array_type() == TypedArrayType::Uint8 {
            let bytes = view.with_view_bytes(|source| source.to_vec());
            return buffer_from_bytes(global_object, &bytes);
        }
    }
    if value.is_object() && !value.is_callable() {
        let vm = global_object.vm();
        let length = get_object_property(global_object, value, &vm.property_names.length)?;
        if length.is_number() {
            return buffer_from_array_like(global_object, value, length.as_number());
        }
        let kind = get_object_property(global_object, value, &Identifier::from_span(vm, b"type"))?;
        if kind.is_string() && wtf_to_rust(&kind.to_wtf_string()) == "Buffer" {
            let data = get_object_property(global_object, value, &Identifier::from_span(vm, b"data"))?;
            if data.is_object() {
                let data_length = get_object_property(global_object, data, &vm.property_names.length)?;
                if data_length.is_number() {
                    return buffer_from_array_like(global_object, data, data_length.as_number());
                }
            }
        }
    }
    // `Symbol.toPrimitive` com a dica `string` (o `Date`): se der string, é `Buffer.from(string)` (`ERR_INVALID_ARG_TYPE`
    // só quando não der).
    if value.is_object() && !value.is_callable() {
        let vm = global_object.vm();
        let hook = get_object_property(global_object, value, &vm.property_names.to_primitive_symbol)?;
        if hook.is_callable() {
            let primitive = value.to_primitive_preferred(crate::runtime::object_to_primitive::PreferredPrimitiveType::PreferString);
            if primitive.is_string() {
                let units = crate::runtime::text_encoder::string_units(global_object, primitive)?;
                return buffer_from_bytes(global_object, &encode_units(&units, encoding_argument(global_object, call.argument(1))?));
            }
        }
    }
    Err(invalid_first_argument(global_object, value))
}

/// O tamanho de `Buffer.alloc`/`allocUnsafe`: número entre 0 e 2^32 (a parte fracionária cai).
fn size_argument(global_object: &JSGlobalObject, value: JSValue) -> Result<usize, Thrown> {
    if !value.is_number() {
        return Err(invalid_argument_type(global_object, "size", "of type number", value));
    }
    let size = value.as_number();
    if size.is_nan() || !(0.0..=MAX_BUFFER_LENGTH).contains(&size) {
        return Err(out_of_range(global_object, "size", &format!(">= 0 && <= {MAX_BUFFER_LENGTH}"), size));
    }
    Ok(size as usize)
}

/// Os bytes com que `Buffer.alloc(size, fill, encoding)` repete o preenchimento: `fill` string (na codificação),
/// `Uint8Array` ou valor numérico (módulo 256). Vazio e `undefined` não preenchem; string não vazia que não decodifica
/// em nenhum byte, e `Uint8Array` vazio, lançam `ERR_INVALID_ARG_VALUE`.
fn fill_pattern(global_object: &JSGlobalObject, fill: JSValue, encoding_value: JSValue) -> Result<Vec<u8>, Thrown> {
    let invalid = |description: String| {
        throw_coded_type_error(global_object, &format!("The argument 'value' is invalid. Received {description}"), "ERR_INVALID_ARG_VALUE")
    };
    if fill.is_undefined() {
        return Ok(Vec::new());
    }
    if fill.is_string() {
        let encoding = encoding_argument(global_object, encoding_value)?;
        let units = crate::runtime::text_encoder::string_units(global_object, fill)?;
        let pattern = encode_units(&units, encoding);
        if pattern.is_empty() && !units.is_empty() {
            return Err(invalid(inspect_string(&String::from_utf16_lossy(&units))));
        }
        return Ok(pattern);
    }
    if let Some(view) = JSGenericTypedArrayView::from_value(&fill).filter(|view| view.typed_array_type() == TypedArrayType::Uint8) {
        let pattern = view.with_view_bytes(|source| source.to_vec());
        if pattern.is_empty() {
            let description = if is_buffer_value(global_object, fill)? { "<Buffer >" } else { "Uint8Array(0) []" };
            return Err(invalid(description.to_string()));
        }
        return Ok(pattern);
    }
    Ok(vec![fill.to_uint32() as u8])
}

/// `util.inspect` de uma string: aspas simples, ou duplas/crase quando o texto tem a anterior, com os escapes comuns.
fn inspect_string(text: &str) -> String {
    let quote = if !text.contains('\'') {
        '\''
    } else if !text.contains('"') {
        '"'
    } else if !text.contains('`') {
        '`'
    } else {
        '\''
    };
    let mut out = String::from(quote);
    for character in text.chars() {
        match character {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\\' => out.push_str("\\\\"),
            other if other == quote => {
                out.push('\\');
                out.push(other);
            }
            other => out.push(other),
        }
    }
    out.push(quote);
    out
}

/// `Buffer.alloc(size, fill, encoding)`: o `fill_pattern` repetido até o fim; vazio ou ausente deixa zeros.
fn buffer_alloc_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let size = size_argument(global_object, call.argument(0))?;
    let pattern = fill_pattern(global_object, call.argument(1), call.argument(2))?;
    let bytes: Vec<u8> = if pattern.is_empty() { vec![0; size] } else { pattern.iter().copied().cycle().take(size).collect() };
    buffer_from_bytes(global_object, &bytes)
}

/// `Buffer.allocUnsafe(size)` e `allocUnsafeSlow(size)`: mesma validação, conteúdo zerado.
fn buffer_alloc_unsafe_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    buffer_from_bytes(global_object, &vec![0; size_argument(global_object, call.argument(0))?])
}

/// `Buffer.compare(buf1, buf2)`: -1, 0 ou 1, na ordem lexicográfica dos bytes.
fn buffer_compare_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let first = uint8_bytes_argument(global_object, call.argument(0), "buf1")?;
    let second = uint8_bytes_argument(global_object, call.argument(1), "buf2")?;
    Ok(js_number(first.cmp(&second) as i32 as f64))
}

/// `Buffer.concat(list, length)`: os buffers da lista emendados, cortados ou completados com zeros até `length`.
fn buffer_concat_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let list = call.argument(0);
    // Sem argumento o bun devolve um `Buffer` vazio (medido no bun 1.4.2: `Buffer.concat()` dá `[]`).
    if call.argument_count() == 0 {
        return buffer_from_bytes(global_object, &[]);
    }
    if !is_js_array(&list) {
        return Err(invalid_argument_type(global_object, "list", "an instance of Array", list));
    }
    let count = get_object_property(global_object, list, &global_object.vm().property_names.length)?.to_number() as usize;
    if count == 0 {
        return buffer_from_bytes(global_object, &[]);
    }
    let mut parts = Vec::with_capacity(count);
    for index in 0..count {
        let item = get_object_index_u64(global_object, list, index as u64)?;
        parts.push(uint8_bytes_argument(global_object, item, &format!("list[{index}]"))?);
    }
    let length_argument = call.argument(1);
    let length = if length_argument.is_undefined() {
        parts.iter().map(Vec::len).sum()
    } else {
        validate_integer(global_object, length_argument, "length", MAX_BUFFER_LENGTH)? as usize
    };
    let mut bytes: Vec<u8> = parts.concat();
    bytes.resize(length, 0);
    buffer_from_bytes(global_object, &bytes)
}

/// `Buffer.copyBytesFrom(view, offset, length)`: uma cópia dos bytes dos elementos `offset..offset+length` da visão.
fn buffer_copy_bytes_from_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let view_value = call.argument(0);
    let Some(view) = JSGenericTypedArrayView::from_value(&view_value) else {
        return Err(invalid_argument_type(global_object, "view", "of type TypedArray", view_value));
    };
    let element_count = view.length();
    if element_count == 0 {
        return buffer_from_bytes(global_object, &[]);
    }
    let offset_argument = call.argument(1);
    let offset = if offset_argument.is_undefined() { 0.0 } else { validate_integer(global_object, offset_argument, "offset", MAX_SAFE_INTEGER)? };
    let length_argument = call.argument(2);
    let end = if length_argument.is_undefined() { element_count as f64 } else { offset + validate_integer(global_object, length_argument, "length", MAX_SAFE_INTEGER)? };
    let element_size = view.typed_array_type().element_size();
    let first = offset.min(element_count as f64) as usize;
    let last = end.min(element_count as f64) as usize;
    let bytes = view.with_vector(|source| if first < last { source[first * element_size..last * element_size].to_vec() } else { Vec::new() });
    buffer_from_bytes(global_object, &bytes)
}

/// `Buffer.isBuffer(value)`: o protótipo de `value` tem `Buffer.prototype` na cadeia.
fn is_buffer_value(global_object: &JSGlobalObject, value: JSValue) -> Result<bool, Thrown> {
    let Some(buffer_prototype) = BUFFER_PROTOTYPE.with(|slot| *slot.borrow()) else { return Ok(false) };
    let mut current = value;
    if JSGenericTypedArrayView::from_value(&current).is_none() {
        return Ok(false);
    }
    while current.is_object() {
        current = current.get_prototype(global_object)?;
        if current == buffer_prototype {
            return Ok(true);
        }
    }
    Ok(false)
}

fn buffer_is_buffer_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(js_boolean(is_buffer_value(global_object, call.argument(0))?))
}

/// `Buffer.isEncoding(value)`: só string, e só nome conhecido.
fn buffer_is_encoding_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let value = call.argument(0);
    Ok(js_boolean(value.is_string() && Encoding::parse(&wtf_to_rust(&value.to_wtf_string())).is_some()))
}

/// `Buffer.byteLength(value, encoding)`: tamanho de `ArrayBuffer`/visão, ou o que a string ocupa na codificação
/// (nome desconhecido ou ausente é `utf8`).
fn buffer_byte_length_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let value = call.argument(0);
    if value.is_string() {
        let encoding_value = call.argument(1);
        let encoding = if encoding_value.is_string() { Encoding::parse(&wtf_to_rust(&encoding_value.to_wtf_string())) } else { None };
        let units = crate::runtime::text_encoder::string_units(global_object, value)?;
        let length = match encoding.unwrap_or(Encoding::Utf8) {
            Encoding::Latin1 | Encoding::Ascii => units.len(),
            Encoding::Utf16Le => units.len() * 2,
            Encoding::Hex => units.len() / 2,
            Encoding::Base64 | Encoding::Base64Url => base64_decoded_size(&units),
            Encoding::Utf8 => encode_units(&units, Encoding::Utf8).len(),
        };
        return Ok(js_number(length as f64));
    }
    if let Some(array_buffer) = JSArrayBuffer::from_value(&value) {
        return Ok(js_number(array_buffer.impl_().byte_length() as f64));
    }
    if let Some(view) = JSGenericTypedArrayView::from_value(&value) {
        return Ok(js_number(view.byte_length() as f64));
    }
    Err(invalid_argument_type(global_object, "string", "of type string or an instance of Buffer or ArrayBuffer", value))
}

/// `Buffer(...)` e `new Buffer(...)`: número é `Buffer.alloc`, o resto é `Buffer.from`. Medido no bun: NaN e infinitos
/// dão a mensagem de `Buffer.alloc` (`>= 0 && <= N`); número finito fora do intervalo escreve `>= 0 and <= N`.
fn buffer_construct_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let value = call.argument(0);
    if value.is_number() {
        let size = value.as_number();
        if size.is_finite() && !(0.0..=MAX_BUFFER_LENGTH).contains(&size) {
            return Err(out_of_range(global_object, "size", &format!(">= 0 and <= {MAX_BUFFER_LENGTH}"), size));
        }
        return buffer_from_bytes(global_object, &vec![0; size_argument(global_object, value)?]);
    }
    buffer_from_body(global_object, call)
}

/// Um índice de `toString`: número limitado a `0..=length`, o resto vale `default`.
fn clamp_index(value: JSValue, length: usize, default: usize) -> usize {
    if !value.is_number() {
        return default;
    }
    let number = value.as_number();
    if number.is_nan() || number < 0.0 {
        0
    } else if number > length as f64 {
        length
    } else {
        number as usize
    }
}

/// O receptor de `equals`/`compare`/`indexOf`/`lastIndexOf`/`includes`/`write` (medido no bun 1.4.2): um `Uint8Array` serve; `this`
/// `undefined` ou `null` lança o `TypeError` sem código `Cannot convert undefined or null to object`; qualquer outro
/// valor lança `ERR_INVALID_THIS` com `Can only call Buffer.{name} on instances of Buffer`.
fn named_buffer_view(global_object: &JSGlobalObject, call: &HostCall, name: &str) -> Result<JSGenericTypedArrayViewRef, Thrown> {
    let this = call.this_value();
    if this.is_undefined_or_null() {
        return Err(Thrown::type_error("Cannot convert undefined or null to object"));
    }
    JSGenericTypedArrayView::from_value(&this)
        .filter(|view| view.typed_array_type() == TypedArrayType::Uint8)
        .ok_or_else(|| throw_coded_type_error(global_object, &format!("Can only call Buffer.{name} on instances of Buffer"), "ERR_INVALID_THIS"))
}

/// `buffer.toString(encoding, start, end)`.
fn buffer_to_string_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // `toLocaleString` é a mesma função e também acusa `Buffer.toString` (medido no bun 1.4.2).
    let view = named_buffer_view(global_object, call, "toString")?;
    let encoding = encoding_argument(global_object, call.argument(0))?;
    let text = view.with_view_bytes(|bytes| {
        let start = clamp_index(call.argument(1), bytes.len(), 0);
        let end = clamp_index(call.argument(2), bytes.len(), bytes.len());
        if start >= end {
            WtfString::from_latin1(b"")
        } else {
            decode_bytes(&bytes[start..end], encoding)
        }
    });
    Ok(JSValue::from_js_string(js_string(global_object.vm(), &text)))
}

/// `buffer.equals(other)`.
fn buffer_equals_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_bytes = named_buffer_view(global_object, call, "equals")?.with_view_bytes(|bytes| bytes.to_vec());
    let other = uint8_bytes_argument(global_object, call.argument(0), "otherBuffer")?;
    Ok(js_boolean(this_bytes == other))
}

/// `buffer.compare(target, targetStart, targetEnd, sourceStart, sourceEnd)`.
fn buffer_compare_method_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let source = named_buffer_view(global_object, call, "compare")?.with_view_bytes(|bytes| bytes.to_vec());
    let target = uint8_bytes_argument(global_object, call.argument(0), "target")?;
    let bound = |index: usize, default: usize, name: &str, max: usize| -> Result<usize, Thrown> {
        let value = call.argument(index);
        if value.is_undefined() {
            Ok(default)
        } else {
            Ok(validate_integer(global_object, value, name, max as f64)? as usize)
        }
    };
    let target_start = bound(1, 0, "targetStart", MAX_BUFFER_LENGTH as usize)?;
    let target_end = bound(2, target.len(), "targetEnd", target.len())?;
    let source_start = bound(3, 0, "sourceStart", MAX_BUFFER_LENGTH as usize)?;
    let source_end = bound(4, source.len(), "sourceEnd", source.len())?;
    if source_start >= source_end {
        return Ok(js_number(if target_start >= target_end { 0.0 } else { -1.0 }));
    }
    if target_start >= target_end {
        return Ok(js_number(1.0));
    }
    let source_part = &source[source_start.min(source.len())..source_end.min(source.len())];
    let target_part = &target[target_start.min(target.len())..target_end.min(target.len())];
    Ok(js_number(source_part.cmp(target_part) as i32 as f64))
}

/// Índice relativo de `slice`/`subarray`: `undefined` vale `default`, negativo conta do fim, o resto é limitado a `length`.
fn relative_index(value: JSValue, length: usize, default: usize) -> usize {
    if value.is_undefined() {
        return default;
    }
    let number = value.to_number();
    let integer = if number.is_nan() { 0.0 } else { number.trunc() };
    if integer < 0.0 {
        (length as f64 + integer).max(0.0) as usize
    } else {
        integer.min(length as f64) as usize
    }
}

/// `buffer.slice(start, end)` e `buffer.subarray(start, end)`: um `Buffer` sobre a mesma memória.
fn buffer_subarray_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // `subarray` também acusa `Buffer.slice` no erro de receptor (medido no bun 1.4.2).
    let view = named_buffer_view(global_object, call, "slice")?;
    let length = view.length();
    let start = relative_index(call.argument(0), length, 0);
    let end = relative_index(call.argument(1), length, length);
    let count = end.saturating_sub(start);
    let structure = BUFFER_STRUCTURE.with(|slot| slot.borrow().clone()).expect("Buffer chamado antes da instalação");
    Ok(JSGenericTypedArrayView::create_with_buffer(global_object, &structure, view.possibly_shared_buffer(), view.byte_offset() + start, Some(count))?.as_value())
}

/// Os elementos que `Array.from(this)` extrai do receptor de `toJSON` (medido no bun 1.4.2): typed array dá os valores
/// dos elementos, string dá os pontos de código, objeto dá o que `length` e os índices dizem, e número ou booleano dão
/// vazio; `undefined` e `null` lançam o `TypeError` do `Array.from`.
fn to_json_elements(global_object: &JSGlobalObject, this: JSValue) -> Result<Vec<JSValue>, Thrown> {
    let vm = global_object.vm();
    if this.is_undefined_or_null() {
        return Err(Thrown::type_error("Array.from requires an array-like object - not null or undefined"));
    }
    if let Some(view) = JSGenericTypedArrayView::from_value(&this) {
        return (0..view.length()).map(|index| view.get_index_quickly(index)).collect();
    }
    if this.is_string() {
        let units = crate::runtime::text_encoder::string_units(global_object, this)?;
        let mut elements = Vec::new();
        let mut index = 0;
        while index < units.len() {
            let width = if (0xd800..0xdc00).contains(&units[index]) && units.get(index + 1).is_some_and(|next| (0xdc00..0xe000).contains(next)) { 2 } else { 1 };
            elements.push(JSValue::from_js_string(js_string(vm, &WtfString::from_utf16(&units[index..index + width]))));
            index += width;
        }
        return Ok(elements);
    }
    if !this.is_object() {
        return Ok(Vec::new());
    }
    let length = get_object_property(global_object, this, &vm.property_names.length)?.to_number();
    let count = if length.is_nan() || length <= 0.0 { 0 } else { length.min(9_007_199_254_740_991.0) as u64 };
    (0..count).map(|index| get_object_index_u64(global_object, this, index)).collect()
}

/// `buffer.toJSON()`: `{ type: 'Buffer', data: [bytes] }`.
fn buffer_to_json_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let values = to_json_elements(global_object, call.this_value())?;
    let data = construct_array(vm, &global_object.array_structure(), &values).as_value();
    let object = construct_empty_object(global_object);
    let kind = JSValue::from_js_string(js_string(vm, &WtfString::from_latin1(b"Buffer")));
    object.put_direct(vm, &property_key(vm, "type"), kind, 0);
    object.put_direct(vm, &property_key(vm, "data"), data, 0);
    Ok(object.as_value())
}

/// Quantos bytes de `bytes` cabem em `room` sem partir um caractere (UTF-8 e UTF-16LE gravam só caracteres inteiros).
fn whole_prefix_length(bytes: &[u8], room: usize, encoding: Encoding) -> usize {
    let room = room.min(bytes.len());
    match encoding {
        Encoding::Utf8 => {
            let mut end = room;
            while end > 0 && end < bytes.len() && (bytes[end] & 0xc0) == 0x80 {
                end -= 1;
            }
            end
        }
        // Cada unidade UTF-16 é um caractere inteiro: um par substituto pode ser partido ao meio (medido no bun).
        Encoding::Utf16Le => room & !1,
        _ => room,
    }
}

/// `buffer.write(string, offset, length, encoding)`: grava o que cabe e devolve quantos bytes escreveu.
fn buffer_write_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let view = named_buffer_view(global_object, call, "write")?;
    let text = call.argument(0);
    if !text.is_string() {
        return Err(invalid_argument_type(global_object, "string", "of type string", text));
    }
    let buffer_length = view.length();
    let (offset_argument, length_argument, encoding_argument_value) = (call.argument(1), call.argument(2), call.argument(3));
    let (offset, length, encoding_value) = if offset_argument.is_undefined() {
        (0, buffer_length, JSValue::undefined())
    } else if length_argument.is_undefined() && offset_argument.is_string() {
        (0, buffer_length, offset_argument)
    } else {
        let offset = validate_integer(global_object, offset_argument, "offset", buffer_length as f64)? as usize;
        let remaining = buffer_length - offset;
        if length_argument.is_undefined() {
            (offset, remaining, encoding_argument_value)
        } else if length_argument.is_string() {
            (offset, remaining, length_argument)
        } else {
            let length = validate_integer(global_object, length_argument, "length", buffer_length as f64)? as usize;
            (offset, length.min(remaining), encoding_argument_value)
        }
    };
    let encoding = encoding_argument(global_object, encoding_value)?;
    write_encoded(global_object, &view, text, offset, length, encoding)
}

/// Grava em `view[offset..]` no máximo `length` bytes de `text` na codificação, sem partir caractere, e devolve quantos.
fn write_encoded(global_object: &JSGlobalObject, view: &JSGenericTypedArrayViewRef, text: JSValue, offset: usize, length: usize, encoding: Encoding) -> HostResult {
    let units = crate::runtime::text_encoder::string_units(global_object, text)?;
    let encoded = encode_units(&units, encoding);
    let written = whole_prefix_length(&encoded, length, encoding);
    view.with_vector_mut(|destination| destination[offset..offset + written].copy_from_slice(&encoded[..written]));
    Ok(js_number(written as f64))
}

/// Primeira posição de `needle` em `haystack` a partir de `start` (para frente) ou última a partir de `start` (para
/// trás); `step` é o alinhamento das posições (2 em UTF-16LE).
fn find_bytes(haystack: &[u8], needle: &[u8], start: usize, forward: bool, step: usize) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    let last = haystack.len() - needle.len();
    let matches = |position: &usize| position % step == 0 && haystack[*position..*position + needle.len()] == *needle;
    if forward {
        (start..=last).find(matches)
    } else {
        (0..=start.min(last)).rev().find(matches)
    }
}

/// `indexOf`/`lastIndexOf`/`includes`: `bidirectionalIndexOf` do Node (`byteOffset` string é a codificação, negativo
/// conta do fim, NaN vale o começo ou o fim; agulha vazia devolve o deslocamento limitado ao tamanho).
fn buffer_search(global_object: &JSGlobalObject, call: &HostCall, forward: bool, name: &str) -> Result<Option<usize>, Thrown> {
    let haystack = named_buffer_view(global_object, call, name)?.with_view_bytes(|bytes| bytes.to_vec());
    let needle_value = call.argument(0);
    let mut offset_argument = call.argument(1);
    let mut encoding_value = call.argument(2);
    if offset_argument.is_string() {
        encoding_value = offset_argument;
        offset_argument = JSValue::undefined();
    }
    let raw = if offset_argument.is_undefined() { f64::NAN } else { offset_argument.to_number() };
    let mut offset = raw.clamp(-2_147_483_648.0, 2_147_483_647.0);
    if offset.is_nan() {
        offset = if forward { 0.0 } else { haystack.len() as f64 };
    }
    let offset = offset.trunc();
    let (needle, step) = if needle_value.is_number() {
        (vec![needle_value.to_uint32() as u8], 1)
    } else if needle_value.is_string() {
        let encoding = encoding_argument(global_object, encoding_value)?;
        let units = crate::runtime::text_encoder::string_units(global_object, needle_value)?;
        (encode_units(&units, encoding), if encoding == Encoding::Utf16Le { 2 } else { 1 })
    } else if JSGenericTypedArrayView::from_value(&needle_value).is_some_and(|view| view.typed_array_type() == TypedArrayType::Uint8) {
        (uint8_bytes_argument(global_object, needle_value, "value")?, 1)
    } else {
        return Err(invalid_argument_type(global_object, "value", "one of type number or string or an instance of Buffer or Uint8Array", needle_value));
    };
    let length = haystack.len() as f64;
    let adjusted = if offset < 0.0 { offset + length } else { offset };
    if adjusted < 0.0 && !forward {
        return Ok(None);
    }
    let start = adjusted.max(0.0).min(length) as usize;
    if needle.is_empty() {
        return Ok(Some(start));
    }
    Ok(find_bytes(&haystack, &needle, start, forward, step))
}

fn buffer_index_result(found: Option<usize>) -> JSValue {
    js_number(found.map_or(-1.0, |index| index as f64))
}

fn buffer_index_of_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(buffer_index_result(buffer_search(global_object, call, true, "indexOf")?))
}

fn buffer_last_index_of_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(buffer_index_result(buffer_search(global_object, call, false, "lastIndexOf")?))
}

fn buffer_includes_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(js_boolean(buffer_search(global_object, call, true, "includes")?.is_some()))
}

host_function!(buffer_call, buffer_construct_body);
host_function!(buffer_construct, buffer_construct_body);
host_function!(buffer_from, buffer_from_body);
host_function!(buffer_to_string, buffer_to_string_body);
host_function!(buffer_alloc, buffer_alloc_body);
host_function!(buffer_alloc_unsafe, buffer_alloc_unsafe_body);
host_function!(buffer_is_buffer, buffer_is_buffer_body);
host_function!(buffer_is_encoding, buffer_is_encoding_body);
host_function!(buffer_byte_length, buffer_byte_length_body);
host_function!(buffer_compare, buffer_compare_body);
host_function!(buffer_concat, buffer_concat_body);
host_function!(buffer_copy_bytes_from, buffer_copy_bytes_from_body);
host_function!(buffer_compare_method, buffer_compare_method_body);
host_function!(buffer_equals, buffer_equals_body);
host_function!(buffer_includes, buffer_includes_body);
host_function!(buffer_index_of, buffer_index_of_body);
host_function!(buffer_last_index_of, buffer_last_index_of_body);
host_function!(buffer_subarray, buffer_subarray_body);
host_function!(buffer_to_json, buffer_to_json_body);
host_function!(buffer_write, buffer_write_body);

/// `Buffer.poolSize`.
const POOL_SIZE: f64 = 8192.0;

/// Instala `Buffer` no global: protótipo herdando de `Uint8Array.prototype`, construtor herdando de `Uint8Array`.
pub fn install_buffer(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let typed_arrays = &global_object.array_buffer_realm.typed_arrays;
    let parents = (typed_arrays.prototype(TypedArrayType::Uint8).as_value(), typed_arrays.constructor(TypedArrayType::Uint8).as_value());
    // Estáticas na ordem do bun: `alloc ... isEncoding` primeiro, depois `length`, `name`, `prototype` e `poolSize`
    // (`from` e `isBuffer` têm `name` vazio).
    let statics: [access::Method; 10] = [
        ("alloc", "alloc", 1, buffer_alloc),
        ("allocUnsafe", "allocUnsafe", 1, buffer_alloc_unsafe),
        ("allocUnsafeSlow", "allocUnsafeSlow", 1, buffer_alloc_unsafe),
        ("byteLength", "byteLength", 2, buffer_byte_length),
        ("compare", "compare", 2, buffer_compare),
        ("concat", "concat", 2, buffer_concat),
        ("copyBytesFrom", "copyBytesFrom", 1, buffer_copy_bytes_from),
        ("from", "", 3, buffer_from),
        ("isBuffer", "", 1, buffer_is_buffer),
        ("isEncoding", "isEncoding", 1, buffer_is_encoding),
    ];
    let (prototype, constructor) = create_native_subclass_with_hooks(
        global_object,
        parents,
        &BUFFER_PROTOTYPE_S_INFO,
        &BUFFER_CONSTRUCTOR_S_INFO,
        "Buffer",
        3,
        buffer_call,
        buffer_construct,
        |constructor| put_methods(global_object, constructor, &statics),
        |_| {},
    );

    let structure = JSGenericTypedArrayView::create_structure(vm, Some(global_object), TypedArrayType::Uint8, prototype.as_value(), false);
    BUFFER_STRUCTURE.with(|slot| *slot.borrow_mut() = Some(structure));
    BUFFER_PROTOTYPE.with(|slot| *slot.borrow_mut() = Some(prototype.as_value()));

    // Os acessores `offset` e `parent` entram entre `latin1Write` e `readBigInt64`.
    let methods = access::prototype_methods();
    let split = methods.iter().position(|method| method.0 == "readBigInt64").unwrap_or(methods.len());
    put_methods(global_object, &prototype, &methods[..split]);
    access::put_accessors(global_object, &prototype);
    put_methods(global_object, &prototype, &methods[split..]);
    // `toLocaleString` é o mesmo objeto de função que `toString` (medido no bun 1.4.2: `===` dá `true`); a chave
    // mantém a posição de origem.
    let to_string_function = prototype.get(vm, &property_key(vm, "toString"));
    prototype.put_direct(vm, &property_key(vm, "toLocaleString"), to_string_function, 0);
    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
    // Depois de `constructor`, na ordem do bun: `Symbol.toStringTag` (`Uint8Array`, só leitura), o `inspect` sob
    // `Symbol.for('nodejs.util.inspect.custom')` (a mesma função de `inspect`, gravável e enumerável) e `Symbol.species`
    // (o `Buffer`, só leitura, enumerável, não configurável).
    crate::runtime::collection_support::put_to_string_tag(vm, &prototype, "Uint8Array");
    let inspect_key = PropertyName::from_identifier(&Identifier::from_span(vm, b"inspect"));
    let inspect_function = prototype.get(vm, &inspect_key);
    let registered = vm.symbol_registry().symbol_for_key(&crate::wtf::text::string_impl::StringImpl::create(b"nodejs.util.inspect.custom"));
    let inspect_symbol = crate::runtime::symbol::Symbol::create_with_registered_uid(vm, &registered);
    prototype.put_direct(vm, &PropertyName::from_identifier(&Identifier::from_private_name(&inspect_symbol.private_name())), inspect_function, 0);
    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.species_symbol), constructor.as_value(), READ_ONLY | DONT_DELETE);
    constructor.put_direct(vm, &property_key(vm, "poolSize"), js_number(POOL_SIZE), 0);

    install_global(global_object, "Buffer", constructor.as_value());
}

/// `buffer.kMaxLength` e `constants.MAX_LENGTH` (2^32, medido no bun 1.4.2).
const K_MAX_LENGTH: f64 = 4294967296.0;
/// `buffer.kStringMaxLength` e `constants.MAX_STRING_LENGTH` (2^31 - 1).
const K_STRING_MAX_LENGTH: f64 = 2147483647.0;

/// Instalador de `buffer` e `node:buffer` no registro de módulos. As chaves seguem a ordem do bun
/// (`Buffer, SlowBuffer, Blob, File, INSPECT_MAX_BYTES, kMaxLength, kStringMaxLength, constants, atob, btoa,
/// transcode, resolveObjectURL, isAscii, isUtf8`).
pub(crate) fn install_buffer_module(global_object: &JSGlobalObject) -> HostResult {
    let vm = global_object.vm();
    let module = construct_empty_object(global_object);
    // `Buffer`, `Blob`, `File`, `atob` e `btoa` são os mesmos objetos dos globais (`require("buffer").Buffer === Buffer`).
    let from_global = |name: &str| {
        let value = global_object.get(vm, &property_key(vm, name));
        if !value.is_undefined() {
            module.put_direct(vm, &property_key(vm, name), value, 0);
        }
    };
    from_global("Buffer");
    // `SlowBuffer`: função `length` 0 cujo `prototype` é `Buffer.prototype` (somente leitura), então `new SlowBuffer(n)`
    // e a chamada sem `new` dão um `Buffer`.
    let slow_buffer = crate::runtime::js_function::JSFunction::create_native(
        vm,
        global_object,
        0,
        &WtfString::from_latin1(b"SlowBuffer"),
        slow_buffer_function,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        crate::runtime::js_function::call_host_function_as_constructor,
    );
    if let Some(prototype) = BUFFER_PROTOTYPE.with(|slot| *slot.borrow()) {
        slow_buffer.put_direct(vm, &property_key(vm, "prototype"), prototype, READ_ONLY | DONT_ENUM | DONT_DELETE);
    }
    module.put_direct(vm, &property_key(vm, "SlowBuffer"), slow_buffer.as_value(), 0);
    from_global("Blob");
    from_global("File");
    crate::runtime::native_class_support::put_native_accessor(
        vm,
        global_object,
        &module,
        "INSPECT_MAX_BYTES",
        inspect_max_bytes_getter,
        Some(inspect_max_bytes_setter),
        DONT_DELETE,
    );
    module.put_direct(vm, &property_key(vm, "kMaxLength"), js_number(K_MAX_LENGTH), 0);
    module.put_direct(vm, &property_key(vm, "kStringMaxLength"), js_number(K_STRING_MAX_LENGTH), 0);
    let constants = construct_empty_object(global_object);
    constants.put_direct(vm, &property_key(vm, "MAX_LENGTH"), js_number(K_MAX_LENGTH), 0);
    constants.put_direct(vm, &property_key(vm, "MAX_STRING_LENGTH"), js_number(K_STRING_MAX_LENGTH), 0);
    module.put_direct(vm, &property_key(vm, "constants"), constants.as_value(), 0);
    from_global("atob");
    from_global("btoa");
    put_methods(
        global_object,
        &module,
        &[
            ("transcode", "transcode", 3, buffer_transcode),
            ("resolveObjectURL", "resolveObjectURL", 1, buffer_resolve_object_url),
            ("isAscii", "isAscii", 1, buffer_is_ascii),
            ("isUtf8", "isUtf8", 1, buffer_is_utf8),
        ],
    );
    Ok(module.as_value())
}

host_function!(slow_buffer_function, buffer_alloc_unsafe_body);
host_function!(inspect_max_bytes_getter, inspect_max_bytes_getter_body);
host_function!(inspect_max_bytes_setter, inspect_max_bytes_setter_body);
host_function!(buffer_transcode, buffer_transcode_body);
host_function!(buffer_resolve_object_url, buffer_resolve_object_url_body);
host_function!(buffer_is_ascii, buffer_is_ascii_body);
host_function!(buffer_is_utf8, buffer_is_utf8_body);

fn inspect_max_bytes_getter_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(js_number(inspect_max_bytes()))
}

/// `validateNumber(value, "INSPECT_MAX_BYTES", 0)`: número não `NaN` e não negativo (`Infinity` vale).
fn inspect_max_bytes_setter_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let value = call.argument(0);
    if !value.is_number() {
        return Err(invalid_argument_type(global_object, "INSPECT_MAX_BYTES", "of type number", value));
    }
    let number = value.as_number();
    if number.is_nan() || number < 0.0 {
        return Err(out_of_range(global_object, "INSPECT_MAX_BYTES", ">= 0", number));
    }
    INSPECT_MAX_BYTES.with(|max| max.set(number));
    Ok(JSValue::undefined())
}

/// As codificações que `buffer.transcode` aceita (ICU): `utf8`, `ucs2`/`utf16le`, `latin1`/`binary` e `ascii`, sem
/// diferenciar maiúscula; ausente, `undefined`, `null` e texto vazio são `utf8`. Hex e base64 não valem.
fn transcode_encoding(value: JSValue) -> Option<Encoding> {
    if value.is_undefined_or_null() {
        return Some(Encoding::Utf8);
    }
    if !value.is_string() {
        return None;
    }
    let name = wtf_to_rust(&value.to_wtf_string());
    if name.is_empty() {
        return Some(Encoding::Utf8);
    }
    Encoding::parse(&name).filter(|encoding| matches!(encoding, Encoding::Utf8 | Encoding::Latin1 | Encoding::Ascii | Encoding::Utf16Le))
}

/// O erro do ICU de `buffer.transcode`: `Error` com `code` e `errno` (`U_ILLEGAL_ARGUMENT_ERROR` 1,
/// `U_INVALID_CHAR_FOUND` 10).
fn transcode_error(global_object: &JSGlobalObject, code: &str, errno: i32) -> Thrown {
    throw_error_with_properties(
        global_object,
        &format!("Unable to transcode Buffer [{code}]"),
        &[("code", NetworkProperty::Text(code)), ("errno", NetworkProperty::Number(errno))],
    )
}

/// `buffer.transcode(source, from, to)`: decodifica `source` em `from` e recodifica em `to`. UTF-8 inválido na origem
/// vira U+FFFD quando o destino é UTF-8, latin1 ou ascii (`?`) e `U_INVALID_CHAR_FOUND` quando é UCS-2; surrogate solto
/// para UTF-8 também é `U_INVALID_CHAR_FOUND`. Caractere sem representação em latin1/ascii vira `?`. Origem ascii com
/// byte alto vira U+FFFD, salvo para UCS-2 (alargamento direto). UCS-2 para UCS-2 com byte sobrando acrescenta U+FFFD.
fn buffer_transcode_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let source = uint8_bytes_argument(global_object, call.argument(0), "source")?;
    let (Some(from), Some(to)) = (transcode_encoding(call.argument(1)), transcode_encoding(call.argument(2))) else {
        return Err(transcode_error(global_object, "U_ILLEGAL_ARGUMENT_ERROR", 1));
    };
    let invalid_char = || transcode_error(global_object, "U_INVALID_CHAR_FOUND", 10);
    let units: Vec<u16> = match from {
        Encoding::Utf8 => match std::str::from_utf8(&source) {
            Ok(text) => text.encode_utf16().collect(),
            Err(_) if to == Encoding::Utf16Le => return Err(invalid_char()),
            Err(_) => String::from_utf8_lossy(&source).encode_utf16().collect(),
        },
        Encoding::Ascii if to != Encoding::Utf16Le => source.iter().map(|&byte| if byte < 0x80 { u16::from(byte) } else { 0xfffd }).collect(),
        Encoding::Latin1 | Encoding::Ascii => source.iter().map(|&byte| u16::from(byte)).collect(),
        _ => {
            let mut units: Vec<u16> = source.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
            if to == Encoding::Utf16Le && source.len() % 2 == 1 {
                units.push(0xfffd);
            }
            units
        }
    };
    let bytes: Vec<u8> = match to {
        Encoding::Utf16Le => units.iter().flat_map(|unit| unit.to_le_bytes()).collect(),
        Encoding::Utf8 => {
            let mut out = String::new();
            for decoded in char::decode_utf16(units.iter().copied()) {
                out.push(decoded.map_err(|_| invalid_char())?);
            }
            out.into_bytes()
        }
        _ => {
            let limit: u32 = if to == Encoding::Ascii { 0x7f } else { 0xff };
            char::decode_utf16(units.iter().copied()).map(|decoded| decoded.ok().map_or(b'?', |c| if u32::from(c) <= limit { u32::from(c) as u8 } else { b'?' })).collect()
        }
    };
    buffer_from_bytes(global_object, &bytes)
}

/// `buffer.resolveObjectURL(url)`: o `Blob` (ou `File`) novo com o conteúdo registrado por `URL.createObjectURL`, ou
/// `undefined` (URL desconhecida, revogada ou com `#`/`?` extra). O argumento é convertido em texto.
fn buffer_resolve_object_url_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let text = pending_or(global_object, call.argument(0).to_wtf_string())?;
    Ok(object_url_state(&wtf_to_rust(&text)).map_or_else(JSValue::undefined, |state| make_blob(global_object, state)))
}

/// Os bytes de `buffer.isAscii`/`isUtf8`: typed array, `DataView` ou `ArrayBuffer`. Outra coisa lança
/// `ERR_INVALID_ARG_TYPE`; buffer destacado lança `ERR_INVALID_STATE` (e `isUtf8` de uma visão destacada, um
/// `TypeError` sem código, medido no bun 1.4.2).
fn validation_bytes(global_object: &JSGlobalObject, value: JSValue, is_utf8: bool) -> Result<Vec<u8>, Thrown> {
    let view_detached = JSGenericTypedArrayView::from_value(&value).map(|view| view.is_detached()).or_else(|| JSDataView::from_value(&value).map(|view| view.is_detached()));
    let buffer_detached = JSArrayBuffer::from_value(&value).map(|buffer| buffer.impl_().is_detached());
    match (view_detached, buffer_detached) {
        (Some(true), _) if is_utf8 => return Err(Thrown::type_error("ArrayBufferView is detached")),
        (Some(true), _) | (_, Some(true)) => {
            return Err(throw_coded_error(global_object, "Invalid state: Cannot validate on a detached buffer", "ERR_INVALID_STATE"));
        }
        _ => {}
    }
    input_bytes(value).ok_or_else(|| throw_coded_type_error(global_object, "First argument must be an ArrayBufferView", "ERR_INVALID_ARG_TYPE"))
}

fn buffer_is_ascii_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(js_boolean(validation_bytes(global_object, call.argument(0), false)?.is_ascii()))
}

fn buffer_is_utf8_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(js_boolean(std::str::from_utf8(&validation_bytes(global_object, call.argument(0), true)?).is_ok()))
}

/// Grava `methods` em `object`, na ordem dada (chave, `name` da função, `length`, corpo).
fn put_methods(global_object: &JSGlobalObject, object: &crate::runtime::js_object::JSObject, methods: &[access::Method]) {
    let vm = global_object.vm();
    for &(key, display_name, length, function) in methods {
        put_direct_native_function_with_display_name(
            vm,
            global_object,
            object,
            &Identifier::from_span(vm, key.as_bytes()),
            &WtfString::from_latin1(display_name.as_bytes()),
            length,
            function,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            0,
        );
    }
}
