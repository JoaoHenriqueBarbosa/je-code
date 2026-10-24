//! Porte de `runtime/ErrorPrototype.cpp` e `ErrorPrototype.h`: o `Error.prototype` e o algoritmo de
//! `Error.prototype.toString`.
//!
//! DIVERGÊNCIAS (ligação pendente, a `NativeFunction` está sendo redesenhada):
//!
//! - `errorProtoFuncToString` vira a casca fina: `thisValue.toThis(strict)`, `TypeError` se não for
//!   objeto (`throwVMTypeError`), `get(name)` e `get(message)` no objeto, `toWTFString` do que não for
//!   `undefined` (cada um com `RETURN_IF_EXCEPTION`), e então chama `error_to_string` daqui. Quando o
//!   `name` (ou a `message`) já era uma `JSString`, o C++ devolve o próprio valor; aqui devolve-se o
//!   texto equivalente e a casca reaproveita a `JSString` original se quiser.
//! - A tabela estática (`errorPrototypeTable`: `toString`, `DontEnum|Function`, comprimento 0) mora em
//!   `error_natives.rs` e reifica no primeiro acesso; o `finishCreation` do `ErrorPrototypeBase` (`name` e
//!   `message`, ambos `DontEnum`, sem transição) usa `initial_name_and_message` e as constantes abaixo.

use crate::runtime::error_type::{error_type_name, ErrorType};
use crate::runtime::property_attribute::PropertyAttribute;
use crate::runtime::string_prototype::{concat, StringOpError};
use crate::wtf::text::wtf_string::String as WtfString;

/// O `length` de `Error.prototype.toString` (linha `toString ... 0` da tabela).
pub const TO_STRING_LENGTH: u32 = 0;

/// Os atributos de `toString` na tabela estática: `DontEnum|Function` (o `Function` só marca a entrada
/// como função nativa na tabela).
pub const TO_STRING_ATTRIBUTES: u32 = PropertyAttribute::DontEnum as u32 | PropertyAttribute::Function as u32;

/// Os atributos de `name` e `message` no `ErrorPrototypeBase::finishCreation`.
pub const NAME_AND_MESSAGE_ATTRIBUTES: u32 = PropertyAttribute::DontEnum as u32;

/// `ErrorPrototypeBase::finishCreation`: os valores iniciais de `name` e `message`.
pub fn initial_name_and_message(name: &str) -> (WtfString, WtfString) {
    (WtfString::from_utf8(name.as_bytes()), WtfString::from_latin1(b""))
}

/// O `name` que o `Error.prototype` recebe (`ErrorPrototype` usa `"Error"`).
pub fn error_prototype_name() -> &'static str {
    error_type_name(ErrorType::Error)
}

/// `errorProtoFuncToString` depois das leituras: `name` e `message` são `None` quando o valor era
/// `undefined`, senão o `toWTFString` dele.
pub fn error_to_string(name: Option<&WtfString>, message: Option<&WtfString>) -> Result<WtfString, StringOpError> {
    // 4. If name is undefined, then let name be "Error"; else let name be ToString(name).
    let name_string = match name {
        None => WtfString::from_latin1(b"Error"),
        Some(name) => name.clone(),
    };

    // 6/7. If msg is undefined, then let msg be the empty String; else let msg be ToString(msg).
    let message_string = match message {
        None => WtfString::from_latin1(b""),
        Some(message) => message.clone(),
    };

    // 8. If name is the empty String, return msg.
    if name_string.length() == 0 {
        return Ok(message_string);
    }

    // 9. If msg is the empty String, return name.
    if message_string.length() == 0 {
        return Ok(name_string);
    }

    // 10. Return the result of concatenating name, ":", a single space character, and msg.
    concat(&[name_string, WtfString::from_latin1(b": "), message_string])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> WtfString {
        WtfString::from_utf8(text.as_bytes())
    }

    fn text(string: &WtfString) -> String {
        String::from_utf16(&crate::runtime::string_prototype::code_units(string)).unwrap()
    }

    #[test]
    fn to_string_cases() {
        assert_eq!(text(&error_to_string(Some(&s("TypeError")), Some(&s("boom"))).unwrap()), "TypeError: boom");
        assert_eq!(text(&error_to_string(None, None).unwrap()), "Error");
        assert_eq!(text(&error_to_string(Some(&s("")), Some(&s("boom"))).unwrap()), "boom");
        assert_eq!(text(&error_to_string(Some(&s("E")), Some(&s(""))).unwrap()), "E");
        assert_eq!(text(&error_to_string(None, Some(&s("x"))).unwrap()), "Error: x");
        assert_eq!(text(&error_to_string(Some(&s("")), None).unwrap()), "");
    }

    #[test]
    fn initial_properties() {
        let (name, message) = initial_name_and_message(error_prototype_name());
        assert_eq!(text(&name), "Error");
        assert_eq!(message.length(), 0);
    }
}
