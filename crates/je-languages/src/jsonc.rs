//! O JSON com comentários e vírgula sobrando dos `package.json`, temas e `language-configuration.json`,
//! lido pelo `jsonc-parser` portado do `je-base` e entregue como `serde_json::Value` (o formato que o
//! ferriki-textmate consome).

use je_base::json::{self, Value as JsValue};
use serde_json::{Map, Number, Value};

/// Converte o valor do `je-base` (strings UTF-16, objeto com a ordem do JavaScript) para `serde_json`.
/// Surrogates isolados viram U+FFFD.
pub fn to_serde(value: &JsValue) -> Value {
    match value {
        JsValue::Null => Value::Null,
        JsValue::Bool(b) => Value::Bool(*b),
        JsValue::Number(n) => Number::from_f64(*n).map_or(Value::Null, Value::Number),
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

/// `parse(text)` tolerante: comentários e vírgula final aceitos; erros de sintaxe não impedem o resultado
/// parcial, como no `jsonc-parser` do VS Code, que só devolve a lista de erros.
pub fn parse(text: &str) -> Result<Value, String> {
    let text = text.trim_start_matches('\u{feff}');
    let mut errors = Vec::new();
    json::parse_str(text, &mut errors).map(|v| to_serde(&v)).ok_or_else(|| "JSON vazio ou inválido".to_owned())
}
