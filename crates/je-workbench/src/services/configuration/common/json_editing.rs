//! Porte de `src/vs/workbench/services/configuration/common/jsonEditing.ts`: o contrato do serviço que edita arquivos
//! JSON e o erro que ele lança. A implementação (`JSONEditingService`, de `jsonEditingService.ts`) edita o arquivo por
//! um modelo de texto e fica com o serviço de modelos de texto.
//!
//! `JSONPath` é a lista de segmentos do `jsonEdit` ([`EditSegment`]). `JSONEditingError` (uma subclasse de `Error` com
//! `code`) é um [`Error`] nomeado que carrega o código na propriedade `code`.

use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::json_edit::EditSegment;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;

/// `JSONEditingErrorCode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonEditingErrorCode {
    /// Error when trying to write to a file that contains JSON errors.
    ErrorInvalidFile = 0,
}

/// `JSONEditingError`.
pub fn json_editing_error(message: impl Into<String>, code: JsonEditingErrorCode) -> Error {
    Error::named("JSONEditingError", message).with_prop("code", code as i32)
}

/// O `code` de um [`json_editing_error`], se o erro for um.
pub fn json_editing_error_code(error: &Error) -> Option<JsonEditingErrorCode> {
    match (error.name.as_str(), error.prop("code")) {
        ("JSONEditingError", Some("0")) => Some(JsonEditingErrorCode::ErrorInvalidFile),
        _ => None,
    }
}

/// `IJSONValue`.
#[derive(Clone)]
pub struct JsonValue {
    pub path: Vec<EditSegment>,
    pub value: Dynamic,
}

/// `IJSONEditingService`.
pub trait IJsonEditingService {
    fn write(&self, resource: &URI, values: Vec<JsonValue>, save: bool) -> Promise<()>;
}

service_id!(pub I_JSON_EDITING_SERVICE: dyn IJsonEditingService = "jsonEditingService");
