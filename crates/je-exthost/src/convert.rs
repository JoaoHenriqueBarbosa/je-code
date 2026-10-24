//! A fronteira de valores: o `Dynamic` do Rust (o `any` do original) contra o [`JsData`] do motor.
//!
//! As classes da API (`Position`, `Range`, `Uri`...) são `Dynamic::Object` com a propriedade `$$class`
//! (`ext_host_values`), e o [`JsData::Object`] carrega a mesma chave: a travessia é direta, e o motor devolve a instância
//! com o protótipo da classe registrada. Uma função ou promessa JavaScript vira um objeto com carga nativa opaca
//! (`ObjectRef::with_host`), que o lado do Rust devolve ao motor sem abrir.

use std::rc::Rc;

use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::Error;
use zjsc::api::embed_node::{JsData, JsHandle, CLASS_KEY};

/// `Dynamic` para o motor. `Function`, `RegExp`, `Date` e arrays tipados ainda não atravessam: viram `undefined`.
pub fn dynamic_to_js(value: &Dynamic) -> JsData {
    match value {
        Dynamic::Undefined => JsData::Undefined,
        Dynamic::Null => JsData::Null,
        Dynamic::Bool(flag) => JsData::Bool(*flag),
        Dynamic::Number(number) => JsData::Num(*number),
        Dynamic::BigInt(number) => JsData::Num(*number as f64),
        Dynamic::String(text) => JsData::Str(text.to_string_lossy()),
        Dynamic::Array(items) => JsData::Array(items.to_vec().iter().map(dynamic_to_js).collect()),
        Dynamic::Object(object) => match object.host::<JsHandle>() {
            Some(handle) => JsData::Handle(handle),
            None => JsData::Object(object.entries().iter().map(|(key, item)| (key.to_string_lossy(), dynamic_to_js(item))).collect()),
        },
        Dynamic::Function(_) | Dynamic::RegExp(_) | Dynamic::Date(_) | Dynamic::TypedArray(_) => JsData::Undefined,
    }
}

/// O motor para `Dynamic`. Um [`JsData::Handle`] vira objeto com carga nativa.
pub fn js_to_dynamic(value: &JsData) -> Dynamic {
    match value {
        JsData::Undefined => Dynamic::Undefined,
        JsData::Null => Dynamic::Null,
        JsData::Bool(flag) => Dynamic::Bool(*flag),
        JsData::Num(number) => Dynamic::Number(*number),
        JsData::Str(text) => Dynamic::string(text),
        JsData::Array(items) => Dynamic::array(items.iter().map(js_to_dynamic)),
        JsData::Object(fields) => Dynamic::object(fields.iter().map(|(key, item)| (key.as_str(), js_to_dynamic(item)))),
        JsData::Handle(handle) => Dynamic::Object(ObjectRef::with_host(handle.clone())),
    }
}

/// A classe de um objeto (`$$class`), se tiver.
pub fn class_name(value: &JsData) -> Option<&str> {
    match value {
        JsData::Object(fields) => fields.iter().find_map(|(key, item)| match item {
            JsData::Str(name) if key == CLASS_KEY => Some(name.as_str()),
            _ => None,
        }),
        _ => None,
    }
}

/// O campo `name` de um objeto copiado.
pub fn field<'a>(value: &'a JsData, name: &str) -> Option<&'a JsData> {
    match value {
        JsData::Object(fields) => fields.iter().find(|(key, _)| key == name).map(|(_, item)| item),
        _ => None,
    }
}

/// Um erro do Rust como o objeto que o JavaScript lê de um `catch` (`name`, `message`).
pub fn error_to_js(error: &Error) -> JsData {
    JsData::Object(vec![
        (CLASS_KEY.to_owned(), JsData::Str("Error".to_owned())),
        ("name".to_owned(), JsData::Str(error.name.clone())),
        ("message".to_owned(), JsData::Str(error.message.clone())),
    ])
}

/// O motivo de uma rejeição JavaScript como [`Error`]: a `message` de um erro, ou o texto do valor.
pub fn js_to_error(reason: &JsData) -> Error {
    let message = match (field(reason, "message"), reason) {
        (Some(JsData::Str(message)), _) => message.clone(),
        (_, JsData::Str(text)) => text.clone(),
        _ => "rejeição sem mensagem".to_owned(),
    };
    Error::new(message)
}

/// Uma função nativa do Rust como objeto de [`Rc`] pronto para entrar numa tabela de API.
pub type SharedHandle = Rc<JsHandle>;
