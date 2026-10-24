//! `TextEncoder` do global. O JavaScriptCore não o define: quem o instala é o bun (WebCore), como propriedade
//! de dados comum (`writable`, `enumerable`, `configurable`). Medido no bun 1.4.2:
//!
//! - o construtor é nativo (`[native code]`), `length` 0, `name` "TextEncoder", com `length`, `name` e
//!   `prototype` como únicas chaves próprias; o protótipo do construtor é `Function.prototype`;
//! - `TextEncoder.prototype` herda de `Object.prototype` e tem, nesta ordem: `constructor` (não enumerável),
//!   o acessor `encoding` (enumerável, configurável, sem setter, getter nativo `get encoding`, atributo do WebCore: `String(get)` sai `function encoding()`), `encode`
//!   (`length` 1) e `encodeInto` (`length` 2), ambos graváveis, enumeráveis e configuráveis, e
//!   `@@toStringTag` "TextEncoder" (não gravável, não enumerável);
//! - `new TextEncoder(...)` ignora os argumentos e devolve um objeto sem propriedade própria; subclasse e
//!   `Reflect.construct` com `newTarget` diferente usam o `prototype` dele;
//! - sem `new`: ``TypeError: Use `new TextEncoder(...)` instead of `TextEncoder(...)` `` com `code`
//!   `ERR_ILLEGAL_CONSTRUCTOR`; `this` que não é um `TextEncoder`: `encode` e `encodeInto` lançam
//!   `TypeError: Can only call TextEncoder.<método> on instances of TextEncoder` com `code` `ERR_INVALID_THIS`,
//!   e o getter lança `The TextEncoder.encoding getter can only be used on instances of TextEncoder` sem
//!   `code`;
//! - `encode(x)`: `undefined` (ou sem argumento) é a string vazia, o resto passa por `ToString`; o resultado
//!   é um `Uint8Array` de buffer exato; unidade substituta solta vira U+FFFD (três bytes);
//! - `encodeInto(source, destination)`: sem os dois argumentos, `TypeError: Not enough arguments` com `code`
//!   `ERR_MISSING_ARGS`; a `source` é convertida antes de olhar o destino; qualquer `ArrayBufferView`
//!   (`Uint8Array`, `Uint16Array`, `DataView`, `Buffer`...) serve e é preenchido como bytes, o resto dá
//!   `TypeError: Expected Uint8Array`; só entram pontos de código inteiros; devolve `{ read, written }`
//!   (`read` em unidades UTF-16).
//!
//! DIVERGÊNCIAS:
//!
//! - os erros com `code` saem de `runtime/node_error.rs`, que reproduz o bun (protótipo por código, `code`
//!   herdado, `originalLine`...`stack` próprios); o `code` de `ERR_MISSING_ARGS` é própria nos dois;
//! - os `TypeError` sem `code` lançados por função nativa (o getter de `encoding` em objeto alheio,
//!   `Expected Uint8Array`) ganham `originalLine`, `line`, `column` e `sourceURL` próprios no bun; aqui seguem
//!   sem eles;
//! - a propriedade global entra no fim da ordem de chaves, junto de `DOMException` (no bun fica entre
//!   `TextDecoderStream` e `TextEncoderStream`).

use std::cell::RefCell;
use std::collections::HashSet;

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{derived_structure, put_to_string_tag};
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_data_view::JSDataView;
use crate::runtime::js_function::put_direct_native_function_without_transition;
use crate::runtime::js_generic_typed_array_view::JSGenericTypedArrayView;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{EncodedJSValue, JSValue};
use crate::runtime::native_class_support::{create_native_class, install_global, instance_structure, put_native_accessor, throw_coded_type_error, throw_native_type_error};
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::uint8_array_base64::{create_uint8_array, read_written_object};
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo` do protótipo.
static TEXT_ENCODER_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "TextEncoder", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `const ClassInfo` do construtor (`"Function"`).
static TEXT_ENCODER_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

thread_local! {
    /// As instâncias de `TextEncoder` do programa (o valor codificado da célula), para a conferência do `this`.
    static INSTANCES: RefCell<HashSet<EncodedJSValue>> = RefCell::new(HashSet::new());
}

/// Fim do programa (`cell_registry::reset_program_state`): as instâncias guardadas são do programa.
/// As raízes do coletor: as instâncias (a chave do brand check não pode ser reaproveitada).
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    INSTANCES.with(|instances| instances.borrow().iter().for_each(|instance| marker.encoded(*instance)));
}

pub(crate) fn reset_for_program() {
    let _ = INSTANCES.try_with(|instances| instances.borrow_mut().clear());
}

fn is_instance(this: JSValue) -> bool {
    INSTANCES.with(|instances| instances.borrow().contains(&this.encode()))
}

/// `TextEncoder(...)` sem `new`.
fn call_text_encoder_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(throw_coded_type_error(global_object, "Use `new TextEncoder(...)` instead of `TextEncoder(...)`", "ERR_ILLEGAL_CONSTRUCTOR"))
}

/// `new TextEncoder(...)`: os argumentos são ignorados.
fn construct_text_encoder_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let structure = derived_structure(global_object, call, instance_structure)?;
    let instance = JSFinalObject::create(global_object.vm(), &structure).as_value();
    INSTANCES.with(|instances| instances.borrow_mut().insert(instance.encode()));
    Ok(instance)
}

host_function!(call_text_encoder, call_text_encoder_body);
host_function!(construct_text_encoder, construct_text_encoder_body);

fn text_encoder_encoding_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if !is_instance(call.this_value()) {
        return Err(throw_native_type_error(global_object, "The TextEncoder.encoding getter can only be used on instances of TextEncoder"));
    }
    Ok(JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_latin1(b"utf-8"))))
}

/// A conferência do `this` de `encode` e `encodeInto`.
fn check_this(global_object: &JSGlobalObject, call: &HostCall, method: &str) -> Result<(), Thrown> {
    if is_instance(call.this_value()) {
        return Ok(());
    }
    Err(throw_coded_type_error(global_object, &format!("Can only call TextEncoder.{method} on instances of TextEncoder"), "ERR_INVALID_THIS"))
}

/// As unidades UTF-16 da conversão de `value` para string.
pub(crate) fn string_units(global_object: &JSGlobalObject, value: JSValue) -> Result<Vec<u16>, Thrown> {
    let text = pending_or(global_object, value.to_wtf_string())?;
    Ok((0..text.length()).map(|index| text.code_unit_at(index)).collect())
}

/// Um `Uint8Array` novo com os `bytes`.
pub(crate) fn uint8_array_from(global_object: &JSGlobalObject, bytes: &[u8]) -> HostResult {
    let array = create_uint8_array(global_object, bytes.len())?;
    array.with_vector_mut(|destination| destination[..bytes.len()].copy_from_slice(bytes));
    Ok(array.as_value())
}

fn text_encoder_encode_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    check_this(global_object, call, "encode")?;
    let argument = call.argument(0);
    let units = if argument.is_undefined() { Vec::new() } else { string_units(global_object, argument)? };
    uint8_array_from(global_object, String::from_utf16_lossy(&units).as_bytes())
}

/// O que cabe de `units` em `capacity` bytes de UTF-8: os bytes e quantas unidades UTF-16 foram lidas. Só
/// entram pontos de código inteiros; a unidade substituta solta vira U+FFFD.
fn encode_into_capacity(units: &[u16], capacity: usize) -> (Vec<u8>, usize) {
    let mut bytes: Vec<u8> = Vec::new();
    let mut read = 0;
    let mut buffer = [0u8; 4];
    for decoded in char::decode_utf16(units.iter().copied()) {
        let (character, consumed) = match decoded {
            Ok(character) => (character, character.len_utf16()),
            Err(_) => (char::REPLACEMENT_CHARACTER, 1),
        };
        let encoded = character.encode_utf8(&mut buffer).as_bytes();
        if bytes.len() + encoded.len() > capacity {
            break;
        }
        bytes.extend_from_slice(encoded);
        read += consumed;
    }
    (bytes, read)
}

fn text_encoder_encode_into_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    check_this(global_object, call, "encodeInto")?;
    if call.argument_count() < 2 {
        return Err(throw_coded_type_error(global_object, "Not enough arguments", "ERR_MISSING_ARGS"));
    }
    let units = string_units(global_object, call.argument(0))?;
    let destination = call.argument(1);
    let (bytes, read) = if let Some(view) = JSGenericTypedArrayView::from_value(&destination) {
        let (bytes, read) = encode_into_capacity(&units, view.byte_length());
        view.with_vector_mut(|target| target[..bytes.len()].copy_from_slice(&bytes));
        (bytes, read)
    } else if let Some(view) = JSDataView::from_value(&destination) {
        let (bytes, read) = encode_into_capacity(&units, view.view_byte_length().unwrap_or(0));
        view.write_bytes(0, &bytes);
        (bytes, read)
    } else {
        return Err(throw_native_type_error(global_object, "Expected Uint8Array"));
    };
    Ok(read_written_object(global_object, read, bytes.len()))
}

host_function!(text_encoder_encoding, text_encoder_encoding_body);
host_function!(text_encoder_encode, text_encoder_encode_body);
host_function!(text_encoder_encode_into, text_encoder_encode_into_body);

/// Instala `TextEncoder` no global: protótipo herdando de `Object.prototype`, construtor, acessor e métodos.
pub fn install_text_encoder(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let (prototype, constructor) =
        create_native_class(global_object, &TEXT_ENCODER_PROTOTYPE_S_INFO, &TEXT_ENCODER_CONSTRUCTOR_S_INFO, "TextEncoder", call_text_encoder, construct_text_encoder);

    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
    put_native_accessor(vm, global_object, &prototype, "encoding", text_encoder_encoding, None, 0);
    for (name, length, function) in [("encode", 1, text_encoder_encode as crate::runtime::native_function::NativeFunction), ("encodeInto", 2, text_encoder_encode_into)] {
        put_direct_native_function_without_transition(
            vm,
            global_object,
            &prototype,
            &Identifier::from_span(vm, name.as_bytes()),
            length,
            function,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            0,
        );
    }
    put_to_string_tag(vm, &prototype, "TextEncoder");

    install_global(global_object, "TextEncoder", constructor.as_value());
}
