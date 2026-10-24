//! `atob` e `btoa` do global. O JavaScriptCore não os define: quem os instala é o bun (WebCore), como
//! propriedades de dados comuns (`writable`, `enumerable`, `configurable`), `length` 1 e `name` igual ao
//! identificador. Na ordem de chaves do bun vêm logo depois dos globais web (`addEventListener`, `alert`) e
//! antes de `clearImmediate`.
//!
//! `atob` segue o "forgiving-base64 decode" do WHATWG: remove o espaço ASCII (tab, LF, FF, CR, espaço); se o
//! comprimento é múltiplo de 4, remove até dois `=` do fim; resto 1 módulo 4 ou caractere fora do alfabeto
//! (inclusive `=` sobrando) é erro; os bits extras do último bloco são descartados. O resultado é uma string
//! Latin1, um caractere por byte. `btoa` aceita só código de unidade até U+00FF.
//!
//! O erro de caractere inválido (`atob` e `btoa`) é um `DOMException` `InvalidCharacterError` (código 5), o
//! de `js_dom_exception.rs`, lançado a partir do frame nativo para ganhar `line`, `column` e `stack`.

use ul_common::codec::{base64_decode, base64_encode, BASE64_STANDARD};

use crate::host_function;
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::native_class_support::throw_native_type_error;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_dom_exception::throw_dom_exception_from_host;
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_name::PropertyName;
use crate::wtf::text::wtf_string::String as WtfString;

const INVALID_CHARACTERS_MESSAGE: &str = "The string contains invalid characters.";

/// O `DOMException` `InvalidCharacterError`, lançado como exceção pendente.
fn invalid_character_error(global_object: &JSGlobalObject, call: &HostCall) -> Thrown {
    throw_dom_exception_from_host(global_object, call, "InvalidCharacterError", INVALID_CHARACTERS_MESSAGE)
}

/// Os códigos de unidade da conversão do primeiro argumento para string, ou o `TypeError` de argumento
/// ausente (`atob requires 1 argument (a string)`).
fn string_argument(global_object: &JSGlobalObject, call: &HostCall, function: &str) -> Result<Vec<u16>, Thrown> {
    if call.argument_count() < 1 {
        return Err(throw_native_type_error(global_object, &format!("{function} requires 1 argument (a string)")));
    }
    let text = pending_or(global_object, call.argument(0).to_wtf_string())?;
    Ok((0..text.length()).map(|index| text.code_unit_at(index)).collect())
}

fn latin1_value(global_object: &JSGlobalObject, bytes: &[u8]) -> JSValue {
    JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_latin1(bytes)))
}

/// O "forgiving-base64 decode": `None` quando a entrada é inválida.
fn forgiving_base64_decode(units: &[u16]) -> Option<Vec<u8>> {
    let mut data: Vec<u8> = Vec::with_capacity(units.len());
    for &unit in units {
        if matches!(unit, 0x09 | 0x0A | 0x0C | 0x0D | 0x20) {
            continue;
        }
        data.push(u8::try_from(unit).ok()?);
    }
    if data.len() % 4 == 0 {
        for _ in 0..2 {
            if data.last() == Some(&b'=') {
                data.pop();
            }
        }
    }
    if data.len() % 4 == 1 || data.iter().any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/'))) {
        return None;
    }
    // O decodificador comum exige o preenchimento; os bits extras do último bloco ele já descarta.
    while data.len() % 4 != 0 {
        data.push(b'=');
    }
    base64_decode(&data, true).ok()
}

fn atob_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let units = string_argument(global_object, call, "atob")?;
    match forgiving_base64_decode(&units) {
        Some(bytes) => Ok(latin1_value(global_object, &bytes)),
        None => Err(invalid_character_error(global_object, call)),
    }
}

fn btoa_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let units = string_argument(global_object, call, "btoa")?;
    let Some(bytes) = units.iter().map(|&unit| u8::try_from(unit).ok()).collect::<Option<Vec<u8>>>() else {
        return Err(invalid_character_error(global_object, call));
    };
    Ok(latin1_value(global_object, &base64_encode(&bytes, BASE64_STANDARD, true)))
}

host_function!(global_func_atob, atob_body);
host_function!(global_func_btoa, btoa_body);

/// Instala `atob` e `btoa` no global como propriedades de dados comuns.
pub fn add_base64_functions(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    type Native = crate::runtime::native_function::NativeFunction;
    for (name, function) in [("atob", global_func_atob as Native), ("btoa", global_func_btoa as Native)] {
        let identifier = Identifier::from_span(vm, name.as_bytes());
        let created = JSFunction::create_native(
            vm,
            global_object,
            1,
            &WtfString::from_latin1(name.as_bytes()),
            function,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            call_host_function_as_constructor,
        );
        global_object.put_direct(vm, &PropertyName::from_identifier(&identifier), created.as_value(), 0);
    }
}
