//! Utilitários de valor JavaScript que o `je-base` ainda não tem e que o porte da configuração usa.
//!
//! TODO(integração): mover para `je_base::dynamic` (`Key`, `from_json`, `for_in_entries`,
//! `set_property`, `delete_property`) e `je_base::js_string` (`split`, `trim`).

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::js_string;
use je_base::json::Value;

/// Texto que serve de chave: `&str`, `String`, `JsString` e referências a eles.
pub trait Key {
    fn to_js(&self) -> JsString;
}

impl Key for str {
    fn to_js(&self) -> JsString {
        JsString::from(self)
    }
}

impl Key for String {
    fn to_js(&self) -> JsString {
        JsString::from(self.as_str())
    }
}

impl Key for JsString {
    fn to_js(&self) -> JsString {
        self.clone()
    }
}

impl<T: Key + ?Sized> Key for &T {
    fn to_js(&self) -> JsString {
        (**self).to_js()
    }
}

/// O valor JSON que o `parse` devolve, como valor dinâmico.
pub fn from_json(value: &Value) -> Dynamic {
    match value {
        Value::Null => Dynamic::Null,
        Value::Bool(b) => Dynamic::Bool(*b),
        Value::Number(n) => Dynamic::Number(*n),
        Value::String(s) => Dynamic::String(s.clone()),
        Value::Array(items) => Dynamic::array(items.iter().map(from_json)),
        Value::Object(object) => Dynamic::object(object.iter().map(|(k, v)| (k.clone(), from_json(v)))),
    }
}

/// `for (const key in value)` filtrado por `Object.hasOwn`: as propriedades próprias enumeráveis de
/// objetos e arrays, e os índices de uma string (que `for...in` também enumera). Vazio para o resto.
pub fn for_in_entries(value: &Dynamic) -> Vec<(JsString, Dynamic)> {
    match value {
        Dynamic::String(s) => s
            .units()
            .iter()
            .enumerate()
            .map(|(i, u)| (JsString::from(i.to_string()), Dynamic::String(JsString::from_units(vec![*u]))))
            .collect(),
        _ => value.own_entries(),
    }
}

/// `Object.keys(value)`.
pub fn object_keys(value: &Dynamic) -> Vec<JsString> {
    for_in_entries(value).into_iter().map(|(k, _)| k).collect()
}

/// O índice de array canônico que `key` representa, se representa.
fn index_of_key(key: &JsString) -> Option<usize> {
    let units = key.units();
    if units.is_empty() || units.len() > 10 || (units.len() > 1 && units[0] == u16::from(b'0')) {
        return None;
    }
    let mut n: u64 = 0;
    for &u in units {
        if !(u16::from(b'0')..=u16::from(b'9')).contains(&u) {
            return None;
        }
        n = n * 10 + u64::from(u - u16::from(b'0'));
    }
    usize::try_from(n).ok().filter(|&n| n != u32::MAX as usize)
}

/// `target[key] = value`. Objetos recebem a propriedade; arrays recebem só chaves de índice (uma
/// propriedade nomeada num array não é representável, ver `dynamic.rs`); o resto ignora a escrita.
/// Em objeto congelado lança `TypeError`, como o JavaScript em modo estrito.
pub fn set_property(target: &Dynamic, key: &JsString, value: Dynamic) {
    match target {
        Dynamic::Object(o) => o.set(key, value),
        Dynamic::Array(a) => {
            if let Some(index) = index_of_key(key) {
                a.set(index, value);
            }
        }
        _ => {}
    }
}

/// `delete target[key]`. Só objetos têm propriedade removível aqui.
pub fn delete_property(target: &Dynamic, key: &JsString) {
    if let Dynamic::Object(o) = target {
        o.delete(key);
    }
}

/// `typeof value === 'object' && value !== null`.
pub fn is_non_null_object(value: &Dynamic) -> bool {
    value.type_of() == "object" && !value.is_null()
}

/// `text.split(separator)` para um separador de uma unidade.
pub fn split(text: &JsString, separator: u16) -> Vec<JsString> {
    text.units().split(|&u| u == separator).map(|part| JsString::from_units(part.to_vec())).collect()
}

/// `text.trim()`.
pub fn trim(text: &JsString) -> JsString {
    JsString::from(js_string::trim(text.units()))
}

/// `Object.isFrozen(value)`: os primitivos contam como congelados.
pub fn is_frozen(value: &Dynamic) -> bool {
    match value {
        Dynamic::Object(o) => o.is_frozen(),
        Dynamic::Array(a) => a.is_frozen(),
        Dynamic::Undefined | Dynamic::Null | Dynamic::Bool(_) | Dynamic::Number(_) | Dynamic::BigInt(_) | Dynamic::String(_) => true,
        _ => false,
    }
}

/// O texto de um valor dinâmico que é string; o resto sai na forma de depuração (só para mensagens).
pub fn dynamic_to_text(value: &Dynamic) -> String {
    match value {
        Dynamic::String(s) => s.to_string_lossy(),
        other => format!("{other:?}"),
    }
}

/// `JSON.stringify` à moda do `setProperty`: o valor dinâmico como valor JSON, ou `None` quando o
/// `stringify` devolveria `undefined` (`undefined` e função). Dentro de uma lista o que não tem forma em JSON
/// vira `null`, e dentro de um objeto a propriedade some; `NaN` e infinitos viram `null`.
pub fn to_json(value: &Dynamic) -> Option<Value> {
    match value {
        Dynamic::Undefined | Dynamic::Function(_) => None,
        Dynamic::Null => Some(Value::Null),
        Dynamic::Bool(b) => Some(Value::Bool(*b)),
        Dynamic::Number(n) => Some(if n.is_finite() { Value::Number(*n) } else { Value::Null }),
        Dynamic::BigInt(n) => Some(Value::String(JsString::from(n.to_string()))),
        Dynamic::String(s) => Some(Value::String(s.clone())),
        Dynamic::Date(date) => Some(je_base::dynamic::date_to_iso_string(date.time_value()).map_or(Value::Null, |iso| Value::String(JsString::from(iso)))),
        Dynamic::Array(items) => Some(Value::Array(items.to_vec().iter().map(|item| to_json(item).unwrap_or(Value::Null)).collect())),
        Dynamic::Object(object) => {
            let mut result = je_base::json::Object::new();
            for (key, item) in object.entries() {
                if let Some(item) = to_json(&item) {
                    result.insert(key, item);
                }
            }
            Some(Value::Object(result))
        }
        Dynamic::RegExp(_) | Dynamic::TypedArray(_) => Some(Value::Object(je_base::json::Object::new())),
    }
}
