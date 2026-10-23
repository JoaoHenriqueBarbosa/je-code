//! Porte de `src/vs/base/common/marshalling.ts`: `stringify`, `parse` e `revive`, a ida e volta de valores
//! pelo protocolo com `URI`, `RegExp` e `Date` marcados por `$mid`.
//!
//! O original opera sobre `any`. O valor aqui é [`Marshalled`], o JSON com as variantes que um `revive`
//! produz ([`Uri`](Marshalled::Uri), [`RegExp`](Marshalled::RegExp), [`Date`](Marshalled::Date)) e o
//! [`VSBuffer`](Marshalled::Buffer), que o `revive` devolve sem tocar. Um objeto simples guarda as chaves
//! na ordem dada.
//!
//! Diferenças de forma:
//!
//! - `parse` usa o parser de JSON do `je-base` (o do `jsonc`) sem comentários, sem vírgula final e sem
//!   conteúdo vazio, e lança `SyntaxError` se houver qualquer erro; é o `JSON.parse` para JSON bem formado;
//! - o `source` de um `RegExp` é o texto dado ao construir (o `RegExp#source` do JS reescreve `/` como `\/`
//!   e o vazio como `(?:)`);
//! - o `Date` revivido de uma string só entende o formato ISO 8601; outra string dá `NaN`;
//! - o `stringify` de um [`VSBuffer`] é o do original (`{"buffer":{"0":..},"byteLength":n}`), que nunca é
//!   revivido de volta.

use std::rc::Rc;

use zyarr::RegExp;

use crate::buffer::VSBuffer;
use crate::dynamic::{date_to_iso_string, json_quote, number_to_js_string};
use crate::errors::{self, Error};
use crate::js_string::JsString;
use crate::json::{self, ParseOptions, Value};
use crate::marshalling_ids::MarshalledId;
use crate::path::Platform;
use crate::uri::{UriComponents, UriState, URI};

/// O valor que `stringify` escreve e `revive` devolve.
#[derive(Debug, Clone)]
pub enum Marshalled {
    /// `undefined`: some de um objeto no `JSON.stringify` e vira `null` num array.
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(JsString),
    Array(Vec<Marshalled>),
    Object(Vec<(JsString, Marshalled)>),
    Uri(URI),
    RegExp(Rc<RegExpValue>),
    /// O valor de tempo em milissegundos, `NaN` se inválido.
    Date(f64),
    Buffer(VSBuffer),
    /// Um `Uint8Array` ou `Buffer` nativo, que o `JSON.stringify` escreve como `{"0":..,"1":..}`.
    Bytes(Vec<u8>),
}

/// Um `RegExp` compilado, com o texto e as flags que o criaram.
#[derive(Debug)]
pub struct RegExpValue {
    pub source: JsString,
    pub flags: String,
    pub regexp: RegExp,
}

impl RegExpValue {
    /// `new RegExp(source, flags)`; lança `SyntaxError` se a expressão ou as flags forem inválidas.
    pub fn new(source: &JsString, flags: &JsString) -> RegExpValue {
        match RegExp::new(source.units(), flags.units()) {
            Ok(regexp) => RegExpValue { source: source.clone(), flags: regexp.flags().to_flags_string(), regexp },
            Err(error) => errors::throw(Error::named("SyntaxError", format!("Invalid regular expression: {}", error.message()))),
        }
    }
}

impl Marshalled {
    pub(crate) fn key<'a>(&'a self, name: &str) -> Option<&'a Marshalled> {
        match self {
            Marshalled::Object(entries) => entries.iter().find(|(k, _)| k == name).map(|(_, v)| v),
            _ => None,
        }
    }
}

/// O JSON como valor, sem reviver nada.
pub fn from_json(value: &Value) -> Marshalled {
    match value {
        Value::Null => Marshalled::Null,
        Value::Bool(b) => Marshalled::Bool(*b),
        Value::Number(n) => Marshalled::Number(*n),
        Value::String(s) => Marshalled::String(s.clone()),
        Value::Array(items) => Marshalled::Array(items.iter().map(from_json).collect()),
        Value::Object(object) => Marshalled::Object(object.iter().map(|(k, v)| (k.clone(), from_json(v))).collect()),
    }
}

pub fn stringify(obj: &Marshalled) -> JsString {
    let mut out = String::new();
    write_json(obj, &mut out, false);
    JsString::from(out)
}

/// `JSON.stringify(obj)` sem o `replacer` do `marshalling.ts`: um `RegExp` vira `{}` (o `source` e as flags
/// não são enumeráveis). É a serialização do `Object` do protocolo do IPC. `None` é o `undefined` que o
/// `JSON.stringify` devolve para um valor que não é JSON (`undefined` na raiz).
pub fn json_stringify(obj: &Marshalled) -> Option<JsString> {
    if matches!(obj, Marshalled::Undefined) {
        return None;
    }
    let mut out = String::new();
    write_json(obj, &mut out, true);
    Some(JsString::from(out))
}

/// `plain`: sem o `replacer` do `marshalling.ts` (ver [`json_stringify`]).
fn write_json(value: &Marshalled, out: &mut String, plain: bool) {
    match value {
        Marshalled::Undefined | Marshalled::Null => out.push_str("null"),
        Marshalled::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Marshalled::Number(n) => out.push_str(&if n.is_finite() { number_to_js_string(*n) } else { "null".to_string() }),
        Marshalled::String(s) => out.push_str(&json_quote(s)),
        Marshalled::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json(item, out, plain);
            }
            out.push(']');
        }
        Marshalled::Object(entries) => {
            out.push('{');
            let mut first = true;
            for (key, item) in entries.iter().filter(|(_, item)| !matches!(item, Marshalled::Undefined)) {
                if !first {
                    out.push(',');
                }
                first = false;
                out.push_str(&json_quote(key));
                out.push(':');
                write_json(item, out, plain);
            }
            out.push('}');
        }
        // URI is done via toJSON-member
        Marshalled::Uri(uri) => write_uri(&uri.to_json(), out),
        Marshalled::RegExp(_) if plain => out.push_str("{}"),
        Marshalled::RegExp(regexp) => {
            out.push_str(&format!(
                "{{\"$mid\":{},\"source\":{},\"flags\":{}}}",
                MarshalledId::Regexp as i32,
                json_quote(&regexp.source),
                json_quote(&JsString::from(regexp.flags.as_str()))
            ));
        }
        Marshalled::Date(time_value) => match date_to_iso_string(*time_value) {
            Some(iso) => out.push_str(&json_quote(&JsString::from(iso))),
            None => out.push_str("null"),
        },
        Marshalled::Bytes(bytes) => {
            let entries: Vec<String> = bytes.iter().enumerate().map(|(i, b)| format!("\"{i}\":{b}")).collect();
            out.push_str(&format!("{{{}}}", entries.join(",")));
        }
        Marshalled::Buffer(buffer) => {
            let bytes: Vec<String> = buffer.to_vec().iter().enumerate().map(|(i, b)| format!("\"{i}\":{b}")).collect();
            out.push_str(&format!("{{\"buffer\":{{{}}},\"byteLength\":{}}}", bytes.join(","), buffer.byte_length()));
        }
    }
}

/// `URI#toJSON`, na ordem em que o original monta o objeto: `$mid`, `fsPath`, `_sep`, `external`, `path`,
/// `scheme`, `authority`, `query`, `fragment`.
fn write_uri(state: &UriState, out: &mut String) {
    let mut fields: Vec<String> = Vec::new();
    let text = |name: &str, value: &Option<JsString>, fields: &mut Vec<String>| {
        if let Some(value) = value {
            fields.push(format!("\"{name}\":{}", json_quote(value)));
        }
    };
    fields.push(format!("\"$mid\":{}", state.mid.unwrap_or(MarshalledId::Uri as i32)));
    text("fsPath", &state.fs_path, &mut fields);
    if let Some(sep) = state.sep {
        fields.push(format!("\"_sep\":{sep}"));
    }
    text("external", &state.external, &mut fields);
    text("path", &state.components.path, &mut fields);
    text("scheme", &state.components.scheme, &mut fields);
    text("authority", &state.components.authority, &mut fields);
    text("query", &state.components.query, &mut fields);
    text("fragment", &state.components.fragment, &mut fields);
    out.push_str(&format!("{{{}}}", fields.join(",")));
}

/// `JSON.parse` e `revive`. Lança `SyntaxError` se o texto não for JSON.
pub fn parse(text: &[u16], platform: Platform) -> Marshalled {
    let mut parse_errors = Vec::new();
    let options = ParseOptions { disallow_comments: true, allow_trailing_comma: false, allow_empty_content: false };
    let value = json::parse(text, &mut parse_errors, options);
    let Some(value) = value.filter(|_| parse_errors.is_empty()) else {
        errors::throw(Error::named("SyntaxError", "Unexpected token in JSON"));
    };
    revive(from_json(&value), 0, platform)
}

fn string_of(value: Option<&Marshalled>) -> Option<JsString> {
    match value {
        Some(Marshalled::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

fn revive_uri(obj: &Marshalled, platform: Platform) -> URI {
    URI::revive(&uri_state_of(obj), platform)
}

/// O [`UriState`] de um objeto `$mid: Uri` (a forma de `URI#toJSON`).
pub(crate) fn uri_state_of(obj: &Marshalled) -> UriState {
    let text = |name: &str| string_of(obj.key(name));
    UriState {
        mid: Some(MarshalledId::Uri as i32),
        external: text("external"),
        fs_path: text("fsPath"),
        sep: match obj.key("_sep") {
            Some(Marshalled::Number(n)) if *n == 1.0 => Some(1),
            _ => None,
        },
        components: UriComponents {
            scheme: text("scheme"),
            authority: text("authority"),
            path: text("path"),
            query: text("query"),
            fragment: text("fragment"),
        },
    }
}

/// O objeto que `URI#toJSON` produz para o estado, com as chaves na ordem do original.
pub(crate) fn uri_state_to_object(state: &UriState) -> Marshalled {
    let mut entries: Vec<(JsString, Marshalled)> = vec![(JsString::from("$mid"), Marshalled::Number(f64::from(state.mid.unwrap_or(MarshalledId::Uri as i32))))];
    let mut text = |name: &str, value: &Option<JsString>| {
        if let Some(value) = value {
            entries.push((JsString::from(name), Marshalled::String(value.clone())));
        }
    };
    text("fsPath", &state.fs_path);
    if let Some(sep) = state.sep {
        entries.push((JsString::from("_sep"), Marshalled::Number(f64::from(sep))));
    }
    let mut text = |name: &str, value: &Option<JsString>| {
        if let Some(value) = value {
            entries.push((JsString::from(name), Marshalled::String(value.clone())));
        }
    };
    text("external", &state.external);
    text("path", &state.components.path);
    text("scheme", &state.components.scheme);
    text("authority", &state.components.authority);
    text("query", &state.components.query);
    text("fragment", &state.components.fragment);
    Marshalled::Object(entries)
}

/// `new Date(value)` para o que o `revive` recebe em `source`.
fn date_from(value: Option<&Marshalled>) -> f64 {
    match value {
        Some(Marshalled::Number(n)) if n.is_finite() && n.abs() <= 8.64e15 => n.trunc(),
        Some(Marshalled::String(s)) => {
            s.to_string_lossy().parse::<jiff::Timestamp>().map_or(f64::NAN, |timestamp| timestamp.as_millisecond() as f64)
        }
        _ => f64::NAN,
    }
}

pub fn revive(obj: Marshalled, depth: usize, platform: Platform) -> Marshalled {
    // `!obj`: `null`, `0`, `''` e `false` passam como estão.
    let falsy = match &obj {
        Marshalled::Undefined | Marshalled::Null => true,
        Marshalled::Bool(b) => !b,
        Marshalled::Number(n) => *n == 0.0 || n.is_nan(),
        Marshalled::String(s) => s.is_empty(),
        _ => false,
    };
    if falsy || depth > 200 {
        return obj;
    }

    match obj {
        Marshalled::Array(items) => Marshalled::Array(items.into_iter().map(|item| revive(item, depth + 1, platform)).collect()),
        Marshalled::Object(_) => {
            if let Some(Marshalled::Number(mid)) = obj.key("$mid") {
                match MarshalledId::from_i32(*mid as i32).filter(|id| f64::from(*id as i32) == *mid) {
                    Some(MarshalledId::Uri) => return Marshalled::Uri(revive_uri(&obj, platform)),
                    Some(MarshalledId::Regexp) => {
                        let text = |name: &str| match obj.key(name) {
                            Some(Marshalled::String(s)) => s.clone(),
                            _ => JsString::new(),
                        };
                        return Marshalled::RegExp(Rc::new(RegExpValue::new(&text("source"), &text("flags"))));
                    }
                    Some(MarshalledId::Date) => return Marshalled::Date(date_from(obj.key("source"))),
                    _ => {}
                }
            }
            let Marshalled::Object(entries) = obj else { unreachable!("casado como objeto acima") };
            // walk object
            Marshalled::Object(entries.into_iter().map(|(key, value)| (key, revive(value, depth + 1, platform))).collect())
        }
        other => other,
    }
}
