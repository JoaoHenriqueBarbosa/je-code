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

// Conversões e comparações de valor JavaScript que o `contextkey` e os registros usam sobre `any`.
// TODO(integração): mover para `je_base::dynamic` (`to_number`, `to_js_string`, `loose_equals`, `less_than`).

/// `String(value)`: a conversão de texto do JavaScript (`undefined` vira `"undefined"`, array vira os
/// elementos separados por vírgula, objeto vira `[object Object]`).
pub fn to_js_string(value: &Dynamic) -> JsString {
    match value {
        Dynamic::Undefined => JsString::from("undefined"),
        Dynamic::Null => JsString::from("null"),
        Dynamic::Bool(b) => JsString::from(if *b { "true" } else { "false" }),
        Dynamic::Number(n) => JsString::from(je_base::dynamic::number_to_js_string(*n)),
        Dynamic::BigInt(n) => JsString::from(n.to_string()),
        Dynamic::String(s) => s.clone(),
        Dynamic::Array(items) => {
            let mut out = JsString::new();
            for (i, item) in items.to_vec().iter().enumerate() {
                if i > 0 {
                    out.push_unit(b',' as u16);
                }
                if !item.is_nullish() {
                    out.push_units(to_js_string(item).units());
                }
            }
            out
        }
        Dynamic::RegExp(r) => {
            let mut out = JsString::from("/");
            out.push_units(r.source().units());
            out.push_unit(b'/' as u16);
            out.push_units(r.flags().units());
            out
        }
        Dynamic::Function(f) => JsString::from(format!("function {}() {{ [native code] }}", f.name())),
        Dynamic::Object(_) | Dynamic::Date(_) | Dynamic::TypedArray(_) => JsString::from("[object Object]"),
    }
}

/// `Number(text)` (`StringToNumber`): espaço nas pontas, vazio é 0, hexadecimal, octal e binário com
/// prefixo, `Infinity`, e `NaN` para o resto.
pub fn string_to_number(text: &[u16]) -> f64 {
    let trimmed = js_string::trim(text);
    if trimmed.is_empty() {
        return 0.0;
    }
    let s = String::from_utf16_lossy(trimmed);
    let radix = |prefix: &[&str], base: u32| -> Option<f64> {
        let rest = prefix.iter().find_map(|p| s.strip_prefix(p))?;
        if rest.is_empty() || !rest.chars().all(|c| c.is_digit(base)) {
            return Some(f64::NAN);
        }
        Some(rest.chars().fold(0.0, |acc, c| acc * f64::from(base) + f64::from(c.to_digit(base).unwrap_or(0))))
    };
    if let Some(n) = radix(&["0x", "0X"], 16).or_else(|| radix(&["0o", "0O"], 8)).or_else(|| radix(&["0b", "0B"], 2)) {
        return n;
    }
    match s.as_str() {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    if s.chars().any(|c| !(c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))) {
        return f64::NAN;
    }
    s.parse::<f64>().unwrap_or(f64::NAN)
}

/// `Number(value)` / `+value`.
pub fn to_number(value: &Dynamic) -> f64 {
    match value {
        Dynamic::Undefined => f64::NAN,
        Dynamic::Null => 0.0,
        Dynamic::Bool(b) => f64::from(u8::from(*b)),
        Dynamic::Number(n) => *n,
        Dynamic::BigInt(n) => *n as f64,
        Dynamic::String(s) => string_to_number(s.units()),
        other => string_to_number(to_js_string(other).units()),
    }
}

/// `parseFloat(text)`: o prefixo numérico decimal (sem hexadecimal), `NaN` se não houver.
pub fn parse_float(text: &[u16]) -> f64 {
    let s = String::from_utf16_lossy(js_string::trim_start(text));
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    if s[i..].starts_with("Infinity") {
        return if b.first() == Some(&b'-') { f64::NEG_INFINITY } else { f64::INFINITY };
    }
    let digits_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut digits = i - digits_start;
    if i < b.len() && b[i] == b'.' {
        let mut j = i + 1;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        digits += j - i - 1;
        if digits > 0 {
            i = j;
        }
    }
    if digits == 0 {
        return f64::NAN;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let exp_start = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_start {
            i = j;
        }
    }
    s[..i].parse::<f64>().unwrap_or(f64::NAN)
}

/// `a == b` (igualdade abstrata do JavaScript).
pub fn loose_equals(a: &Dynamic, b: &Dynamic) -> bool {
    use Dynamic as D;
    match (a, b) {
        (D::Undefined | D::Null, D::Undefined | D::Null) => true,
        (D::Undefined | D::Null, _) | (_, D::Undefined | D::Null) => false,
        (D::Number(x), D::Number(y)) => x == y,
        (D::String(x), D::String(y)) => x == y,
        (D::Bool(x), D::Bool(y)) => x == y,
        (D::BigInt(x), D::BigInt(y)) => x == y,
        (D::Number(x), D::String(y)) | (D::String(y), D::Number(x)) => *x == string_to_number(y.units()),
        (D::Bool(_), _) => loose_equals(&D::Number(to_number(a)), b),
        (_, D::Bool(_)) => loose_equals(a, &D::Number(to_number(b))),
        (D::BigInt(x), D::Number(y)) | (D::Number(y), D::BigInt(x)) => (*x as f64) == *y,
        (D::BigInt(x), D::String(y)) | (D::String(y), D::BigInt(x)) => (*x as f64) == string_to_number(y.units()),
        _ if a.identity().is_some() && b.identity().is_some() => a.strict_equals(b),
        (_, _) if a.identity().is_some() => loose_equals(&D::String(to_js_string(a)), b),
        (_, _) => loose_equals(a, &D::String(to_js_string(b))),
    }
}

/// O resultado de `a < b` (`false` quando algum lado é `NaN`).
pub fn less_than(a: &Dynamic, b: &Dynamic) -> bool {
    if let (Dynamic::String(x), Dynamic::String(y)) = (a, b) {
        return x.units() < y.units();
    }
    to_number(a) < to_number(b)
}

/// `Object.prototype.hasOwnProperty.call(value, key)` para objetos e arrays (índices).
pub fn has_own_property(value: &Dynamic, key: &JsString) -> bool {
    match value {
        Dynamic::Object(o) => o.has_own(key),
        Dynamic::Array(a) => {
            let k = String::from_utf16_lossy(key.units());
            k.parse::<usize>().is_ok_and(|i| i < a.len() && i.to_string() == k) || k == "length"
        }
        _ => false,
    }
}
