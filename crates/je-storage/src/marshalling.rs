//! O trecho de `src/vs/base/common/marshalling.ts` que o storage usa: `stringify` e `parse` de
//! objetos como `json::Value`.
//!
//! O `revive` do original devolve `URI`, `RegExp` e `Date` no lugar dos objetos marcados com `$mid`.
//! Aqui o valor continua `json::Value` com o `$mid` à vista, e quem lê um URI do storage chama
//! `URI::revive` sobre ele; `uri_to_value` faz o caminho de ida (o `URI#toJSON`).

use je_base::JsString;
use je_base::dynamic::{json_quote, number_to_js_string};
use je_base::errors::{self, Error};
use je_base::json::{self, Object, Value};
use je_base::uri::URI;

/// `JSON.stringify(obj)`.
pub fn stringify(value: &Value) -> String {
    let mut out = String::new();
    write_value(value, &mut out);
    out
}

fn write_value(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        // `JSON.stringify` troca número não finito por `null`.
        Value::Number(n) if !n.is_finite() => out.push_str("null"),
        Value::Number(n) => out.push_str(&number_to_js_string(*n)),
        Value::String(s) => out.push_str(&json_quote(s)),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(item, out);
            }
            out.push(']');
        }
        Value::Object(object) => {
            out.push('{');
            for (i, (key, item)) in object.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&json_quote(key));
                out.push(':');
                write_value(item, out);
            }
            out.push('}');
        }
    }
}

/// `JSON.parse(text)` (sem o `revive`): texto inválido lança `SyntaxError`, como no original.
pub fn parse(text: &str) -> Value {
    let mut parse_errors = Vec::new();
    // `JSON.parse` é estrito: sem comentários nem vírgula final.
    let options = json::ParseOptions {
        allow_trailing_comma: false,
        disallow_comments: true,
        ..json::ParseOptions::default()
    };
    let units: Vec<u16> = text.encode_utf16().collect();
    match json::parse(&units, &mut parse_errors, options) {
        Some(value) if parse_errors.is_empty() => value,
        _ => errors::throw(Error::named(
            "SyntaxError",
            format!("Unexpected token in JSON: {text}"),
        )),
    }
}

/// `URI#toJSON`, na ordem de propriedades do original.
pub fn uri_to_value(uri: &URI) -> Value {
    let state = uri.to_json();
    let mut object = Object::new();
    object.insert(
        JsString::from("$mid"),
        Value::Number(f64::from(state.mid.unwrap_or(1))),
    );
    if let Some(fs_path) = state.fs_path {
        object.insert("fsPath".into(), Value::String(fs_path));
        if let Some(sep) = state.sep {
            object.insert("_sep".into(), Value::Number(f64::from(sep)));
        }
    }
    let c = state.components;
    for (name, field) in [
        ("external", state.external),
        ("path", c.path),
        ("scheme", c.scheme),
        ("authority", c.authority),
        ("query", c.query),
        ("fragment", c.fragment),
    ] {
        if let Some(field) = field {
            object.insert(name.into(), Value::String(field));
        }
    }
    Value::Object(object)
}
