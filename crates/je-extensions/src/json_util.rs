//! JSON tolerante a comentários e vírgula final (o `parse` de `src/vs/base/common/json.ts`) devolvendo
//! `serde_json::Value`, a forma que o manifesto e os pacotes de tradução usam aqui.

use je_base::json::{self, Value as JsValue};
use je_base::json_error_messages::parse_error_message;
use serde_json::{Map, Number, Value};

/// Converte o valor do `je_base::json` em `serde_json::Value`.
pub fn to_serde(value: &JsValue) -> Value {
    match value {
        JsValue::Null => Value::Null,
        JsValue::Bool(b) => Value::Bool(*b),
        JsValue::Number(n) => {
            if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 {
                Value::Number(Number::from(*n as i64))
            } else {
                Number::from_f64(*n).map_or(Value::Null, Value::Number)
            }
        }
        JsValue::String(s) => Value::String(s.to_string_lossy()),
        JsValue::Array(items) => Value::Array(items.iter().map(to_serde).collect()),
        JsValue::Object(object) => {
            let mut map = Map::new();
            for (key, value) in object.iter() {
                map.insert(key.to_string_lossy(), to_serde(value));
            }
            Value::Object(map)
        }
    }
}

/// O erro de um `parse` com recuperação: a mensagem de cada `ParseError` (a do `getParseErrorMessage`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseIssue {
    pub offset: usize,
    pub length: usize,
    pub message: String,
}

/// `json.parse(text, errors)`: o valor (se houver) e os erros.
pub fn parse_lenient(text: &str) -> (Option<Value>, Vec<ParseIssue>) {
    let mut errors = Vec::new();
    let value = json::parse_str(text, &mut errors);
    let issues = errors.iter().map(|e| ParseIssue { offset: e.offset, length: e.length, message: parse_error_message(e.error) }).collect();
    (value.as_ref().map(to_serde), issues)
}

/// O texto sem o BOM: o `VSBuffer#toString` decodifica com `TextDecoder`, que o descarta.
pub fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

/// `JSON.stringify(value, null, '\t')`.
pub fn to_string_tab_indented(value: &Value) -> String {
    use serde::Serialize;
    let mut buffer = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"\t");
    let mut serializer = serde_json::Serializer::with_formatter(&mut buffer, formatter);
    value.serialize(&mut serializer).expect("serializar em memória não falha");
    String::from_utf8(buffer).expect("o serde_json só escreve UTF-8")
}
