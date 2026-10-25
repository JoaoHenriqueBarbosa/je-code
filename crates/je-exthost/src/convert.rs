//! A fronteira de valores: o `Dynamic` do Rust (o `any` do original) contra o [`JsData`] do motor.
//!
//! As classes da API (`Position`, `Range`, `Uri`...) são `Dynamic::Object` com a propriedade `$$class`
//! (`ext_host_values`), e o [`JsData::Object`] carrega a mesma chave: a travessia é direta, e o motor devolve a instância
//! com o protótipo da classe registrada. Uma função ou promessa JavaScript vira um objeto com carga nativa opaca
//! (`ObjectRef::with_host`), que o lado do Rust devolve ao motor sem abrir.

use std::rc::Rc;

use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::Error;
use je_base::uri::URI;
use je_workbench::api::common::ext_host_file_system_consumer::FILE_SYSTEM_ERROR_SUFFIX;
use je_workbench::api::common::ext_host_values::ApiValue;
use zjsc::api::embed_node::{JsData, JsHandle, NodeRunner, CLASS_KEY};

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

/// O argumento `index` como texto, ou o erro que descreve `what`.
pub fn string_arg(args: &[JsData], index: usize, what: &str) -> Result<String, String> {
    match args.get(index) {
        Some(JsData::Str(text)) => Ok(text.clone()),
        _ => Err(format!("{what} precisa ser texto")),
    }
}

/// O argumento por referência como dado: o objeto (um [`JsData::Handle`]) é copiado, o resto fica como veio.
pub fn exported(runner: &NodeRunner, value: &JsData) -> JsData {
    match value {
        JsData::Handle(handle) => runner.export(handle),
        other => other.clone(),
    }
}

/// O texto de um valor que é texto; `None` para `undefined`, `null` e qualquer outro tipo.
pub fn opt_string(value: Option<&JsData>) -> Option<String> {
    match value {
        Some(JsData::Str(text)) => Some(text.clone()),
        _ => None,
    }
}

/// O número de um valor que é número.
pub fn opt_number(value: Option<&JsData>) -> Option<f64> {
    match value {
        Some(JsData::Num(number)) => Some(*number),
        _ => None,
    }
}

/// O booleano de um valor que é booleano.
pub fn opt_bool(value: Option<&JsData>) -> Option<bool> {
    match value {
        Some(JsData::Bool(flag)) => Some(*flag),
        _ => None,
    }
}

/// O texto de uma propriedade opcional: `undefined` quando ausente.
pub fn text_or_undefined(value: Option<String>) -> JsData {
    value.map_or(JsData::Undefined, JsData::Str)
}

/// `value === undefined || value === null` (um argumento ausente conta como `undefined`).
pub fn is_nullish(value: Option<&JsData>) -> bool {
    matches!(value, None | Some(JsData::Undefined) | Some(JsData::Null))
}

/// O valor da API (`ApiValue`, com `$$class`) como dado do motor.
pub fn api_to_js<T: ApiValue>(value: &T) -> JsData {
    dynamic_to_js(&value.to_dynamic())
}

/// O `vscode.Uri` do motor como [`URI`]; `Err` com a mensagem do `TypeError` se o valor não é um `Uri`.
pub fn uri_from_js(value: &JsData) -> Result<URI, String> {
    URI::from_dynamic(&js_to_dynamic(value)).map_err(|error| error.message)
}

/// O [`URI`] como o `vscode.Uri` que o motor entrega (a instância carrega o protótipo da classe `Uri`).
pub fn uri_to_js(uri: &URI) -> JsData {
    api_to_js(uri)
}

/// Um erro do Rust como o objeto que o JavaScript lê de um `catch` (`name`, `message`).
pub fn error_to_js(error: &Error) -> JsData {
    JsData::Object(vec![
        (CLASS_KEY.to_owned(), JsData::Str("Error".to_owned())),
        ("name".to_owned(), JsData::Str(error.name.clone())),
        ("message".to_owned(), JsData::Str(error.message.clone())),
    ])
}

/// O motivo de uma rejeição JavaScript como [`Error`]: a `message` de um erro, ou o texto do valor. Um `FileSystemError` (o `name`
/// termina em ` (FileSystemError)`) mantém o `name`, que carrega o código que o `ExtHostConsumerFileSystem` reconhece.
pub fn js_to_error(reason: &JsData) -> Error {
    let message = match (field(reason, "message"), reason) {
        (Some(JsData::Str(message)), _) => message.clone(),
        (_, JsData::Str(text)) => text.clone(),
        _ => "rejeição sem mensagem".to_owned(),
    };
    match field(reason, "name") {
        Some(JsData::Str(name)) if name.ends_with(FILE_SYSTEM_ERROR_SUFFIX) => Error::named(name.clone(), message),
        _ => Error::new(message),
    }
}

/// O byte de um número inteiro entre 0 e 255.
fn byte_of(item: &JsData) -> Result<u8, String> {
    match item {
        JsData::Num(number) if number.fract() == 0.0 && (0.0..256.0).contains(number) => Ok(*number as u8),
        _ => Err("os bytes precisam ser números inteiros entre 0 e 255".to_owned()),
    }
}

/// Os bytes de um `Uint8Array` que a extensão passou. O `Uint8Array` não atravessa o motor: os bytes chegam como uma lista de números,
/// um texto (os bytes UTF-8) ou um `Uint8Array` copiado como objeto de índices.
pub fn bytes_of(value: Option<&JsData>) -> Result<Vec<u8>, String> {
    match value {
        Some(JsData::Str(text)) => Ok(text.clone().into_bytes()),
        Some(JsData::Array(items)) => items.iter().map(byte_of).collect(),
        Some(JsData::Object(fields)) => {
            let mut indexed: Vec<(usize, &JsData)> = fields.iter().filter_map(|(key, item)| key.parse::<usize>().ok().map(|index| (index, item))).collect();
            indexed.sort_by_key(|(index, _)| *index);
            indexed.into_iter().map(|(_, item)| byte_of(item)).collect()
        }
        _ => Err("os bytes precisam ser uma lista de números".to_owned()),
    }
}

/// Os bytes como a lista de números que o motor entrega no lugar de um `Uint8Array`.
pub fn bytes_to_js(bytes: &[u8]) -> JsData {
    JsData::Array(bytes.iter().map(|byte| JsData::Num(f64::from(*byte))).collect())
}

/// Uma função nativa do Rust como objeto de [`Rc`] pronto para entrar numa tabela de API.
pub type SharedHandle = Rc<JsHandle>;
