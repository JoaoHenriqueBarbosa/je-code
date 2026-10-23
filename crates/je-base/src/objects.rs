//! Porte de `src/vs/base/common/objects.ts`.
//!
//! Tudo opera sobre [`Dynamic`](crate::dynamic::Dynamic), o `any` do original; ver `dynamic.rs`
//! para a representação (identidade por `Rc`, sem cadeia de protótipos, ordem de propriedades do
//! JavaScript). Consequências para este módulo:
//! - `for (const key in obj)` e `key in obj` olham só propriedades próprias; o original também
//!   enxergaria as herdadas de `Object.prototype` (`constructor`, `toString`...), que aqui não
//!   existem como propriedade.
//! - `mixin` só escreve em objetos simples; num array tipado como destino ignora a escrita.
//! - `deepClone` de um `Date` devolve `{}` e de um array tipado devolve um objeto com as chaves de
//!   índice, porque o original copia só `Object.entries` (efeito real do código original, não
//!   escolha do porte); `RegExp` volta como veio. `deepFreeze` de um array tipado não vazio na raiz
//!   lança `TypeError`, como `Object.freeze`.
//! - `safeStringify` devolve `None` quando o `JSON.stringify` devolveria `undefined` (valor de topo
//!   `undefined` ou função). O texto sai compacto, com os números formatados como `String(n)` e
//!   `NaN`/`Infinity` como `null`.
//! - `deepStrictEqual` do teste original não é função deste módulo.

use std::collections::HashSet;

use crate::dynamic::{Dynamic, ObjectRef, Truthy, date_to_iso_string, json_quote, number_to_js_string};
use crate::errors::{self, Error};
use crate::js_string::JsString;
use crate::types::{is_object, is_typed_array};

fn typeof_object(value: &Dynamic) -> bool {
    value.type_of() == "object"
}

/// Cópia profunda (arrays e objetos; `RegExp` volta como veio, ver o topo do módulo).
pub fn deep_clone(obj: &Dynamic) -> Dynamic {
    if !obj.is_truthy() || !typeof_object(obj) {
        return obj.clone();
    }
    if matches!(obj, Dynamic::RegExp(_)) {
        return obj.clone();
    }
    let clone_entry = |value: &Dynamic| if value.is_truthy() && typeof_object(value) { deep_clone(value) } else { value.clone() };
    match obj {
        Dynamic::Array(array) => Dynamic::array(array.to_vec().iter().map(clone_entry)),
        _ => Dynamic::object(obj.own_entries().into_iter().map(|(key, value)| (key, clone_entry(&value)))),
    }
}

/// Congela o objeto e, em profundidade, tudo que ele alcança por propriedades próprias.
pub fn deep_freeze(obj: &Dynamic) -> Dynamic {
    if !obj.is_truthy() || !typeof_object(obj) {
        return obj.clone();
    }
    let mut stack = vec![obj.clone()];
    let mut index = 0;
    while index < stack.len() {
        let current = stack[index].clone();
        index += 1;
        match &current {
            Dynamic::Array(a) => a.freeze(),
            Dynamic::Object(o) => o.freeze(),
            Dynamic::TypedArray(t) if !t.is_empty() => {
                errors::throw(Error::type_error("Cannot freeze array buffer views with elements"));
            }
            _ => {}
        }
        for (_, prop) in current.own_entries() {
            if typeof_object(&prop) && !prop.is_null() && !is_frozen(&prop) && !is_typed_array(&prop) {
                stack.push(prop);
            }
        }
    }
    obj.clone()
}

fn is_frozen(value: &Dynamic) -> bool {
    match value {
        Dynamic::Array(a) => a.is_frozen(),
        Dynamic::Object(o) => o.is_frozen(),
        _ => false,
    }
}

/// Clona trocando o que `changer` quiser trocar: se ele devolver algo diferente de `undefined`, esse
/// valor entra no lugar do original e a descida para ali.
pub fn clone_and_change(obj: &Dynamic, changer: &dyn Fn(&Dynamic) -> Dynamic) -> Dynamic {
    clone_and_change_inner(obj, changer, &mut HashSet::new())
}

fn clone_and_change_inner(obj: &Dynamic, changer: &dyn Fn(&Dynamic) -> Dynamic, seen: &mut HashSet<usize>) -> Dynamic {
    if obj.is_nullish() {
        return obj.clone();
    }
    let changed = changer(obj);
    if !changed.is_undefined() {
        return changed;
    }
    if let Dynamic::Array(array) = obj {
        return Dynamic::array(array.to_vec().iter().map(|e| clone_and_change_inner(e, changer, seen)));
    }
    if is_object(obj) {
        let id = obj.identity().expect("objeto tem identidade");
        if !seen.insert(id) {
            errors::throw(Error::new("Cannot clone recursive data-structure"));
        }
        let result = ObjectRef::new();
        for (key, value) in obj.own_entries() {
            result.set(&key, clone_and_change_inner(&value, changer, seen));
        }
        seen.remove(&id);
        return Dynamic::Object(result);
    }
    obj.clone()
}

/// Copia as propriedades de `source` para `destination`. `overwrite` (padrão `true`) controla se as
/// que já existem em `destination` são sobrescritas (objetos dos dois lados se mesclam em
/// profundidade). Se `destination` não é um objeto, devolve `source`.
pub fn mixin(destination: &Dynamic, source: &Dynamic, overwrite: bool) -> Dynamic {
    let Dynamic::Object(dest) = destination else {
        return if is_object(destination) { destination.clone() } else { source.clone() };
    };
    if is_object(source) {
        for key in source.own_keys() {
            let source_value = source.get(&key);
            if dest.has_own(&key) {
                if overwrite {
                    let dest_value = dest.get(&key).expect("propriedade existe");
                    if is_object(&dest_value) && is_object(&source_value) {
                        mixin(&dest_value, &source_value, overwrite);
                    } else {
                        dest.set(&key, source_value);
                    }
                }
            } else {
                dest.set(&key, source_value);
            }
        }
    }
    destination.clone()
}

/// Igualdade estrutural: primitivos por `===`, arrays por elemento, objetos pelas chaves (sem
/// importar a ordem) e valores. Dois `Date` (ou dois `RegExp`) quaisquer são iguais, porque não têm
/// chaves próprias, como no original.
pub fn equals(one: &Dynamic, other: &Dynamic) -> bool {
    if one.strict_equals(other) {
        return true;
    }
    if one.is_nullish() || other.is_nullish() {
        return false;
    }
    if one.type_of() != other.type_of() {
        return false;
    }
    if one.type_of() != "object" {
        return false;
    }
    if one.is_array() != other.is_array() {
        return false;
    }
    if let (Dynamic::Array(a), Dynamic::Array(b)) = (one, other) {
        let (a, b) = (a.to_vec(), b.to_vec());
        return a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| equals(x, y));
    }
    let mut one_keys = one.own_keys();
    one_keys.sort();
    let mut other_keys = other.own_keys();
    other_keys.sort();
    if one_keys != other_keys {
        return false;
    }
    one_keys.iter().all(|key| equals(&one.get(key), &other.get(key)))
}

/// Como `JSON.stringify`, mas quebra as referências circulares (`"[Circular]"`) e escreve `bigint`
/// como `[BigInt n]`. Só os ancestrais atuais contam, então referências compartilhadas entre irmãos
/// saem completas.
pub fn safe_stringify(obj: &Dynamic) -> Option<String> {
    let mut out = String::new();
    safe_stringify_value(obj, &mut Vec::new(), &mut out).then_some(out)
}

/// Escreve o valor e devolve `true`, ou devolve `false` (sem escrever) se o `JSON.stringify`
/// omitiria a propriedade (`undefined` e função).
fn safe_stringify_value(value: &Dynamic, ancestors: &mut Vec<usize>, out: &mut String) -> bool {
    match value {
        Dynamic::Undefined | Dynamic::Function(_) => return false,
        Dynamic::Null => out.push_str("null"),
        Dynamic::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Dynamic::Number(n) => out.push_str(&if n.is_finite() { number_to_js_string(*n) } else { "null".to_string() }),
        Dynamic::BigInt(n) => out.push_str(&json_quote(&JsString::from(format!("[BigInt {n}]")))),
        Dynamic::String(s) => out.push_str(&json_quote(s)),
        Dynamic::Date(d) => match date_to_iso_string(d.time_value()) {
            Some(iso) => out.push_str(&json_quote(&JsString::from(iso))),
            None => out.push_str("null"),
        },
        Dynamic::Array(_) | Dynamic::Object(_) | Dynamic::RegExp(_) | Dynamic::TypedArray(_) => {
            let id = value.identity().expect("objeto tem identidade");
            if ancestors.contains(&id) {
                out.push_str("\"[Circular]\"");
                return true;
            }
            ancestors.push(id);
            if let Dynamic::Array(array) = value {
                out.push('[');
                for (i, item) in array.to_vec().iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    if !safe_stringify_value(item, ancestors, out) {
                        out.push_str("null");
                    }
                }
                out.push(']');
            } else {
                out.push('{');
                let mut first = true;
                for (key, item) in value.own_entries() {
                    let mark = out.len();
                    if !first {
                        out.push(',');
                    }
                    out.push_str(&json_quote(&key));
                    out.push(':');
                    if safe_stringify_value(&item, ancestors, out) {
                        first = false;
                    } else {
                        out.truncate(mark);
                    }
                }
                out.push('}');
            }
            ancestors.pop();
        }
    }
    true
}

/// Como `JSON.stringify`, mas com as chaves ordenadas em todos os níveis, para que entradas
/// estruturalmente iguais deem sempre a mesma string (chave de cache, por exemplo).
///
/// - Propriedades `undefined` são omitidas; referências circulares viram `"[Circular]"`.
/// - `undefined` no topo dá `"undefined"`; qualquer outra falha (um `bigint` no caminho) dá `""`.
pub fn stable_stringify(value: &Dynamic) -> String {
    if value.is_undefined() {
        return "undefined".to_string();
    }
    stable_stringify_inner(value, &mut HashSet::new()).unwrap_or_default()
}

/// `Err` é o `JSON.stringify` lançando (`bigint`).
fn stable_stringify_inner(value: &Dynamic, seen: &mut HashSet<usize>) -> Result<String, ()> {
    if !typeof_object(value) || value.is_null() {
        return match value {
            Dynamic::BigInt(_) => Err(()),
            Dynamic::Undefined | Dynamic::Function(_) => Ok("null".to_string()),
            Dynamic::Null => Ok("null".to_string()),
            Dynamic::Bool(b) => Ok(b.to_string()),
            Dynamic::Number(n) => Ok(if n.is_finite() { number_to_js_string(*n) } else { "null".to_string() }),
            Dynamic::String(s) => Ok(json_quote(s)),
            _ => unreachable!("os demais são objetos"),
        };
    }
    let id = value.identity().expect("objeto tem identidade");
    if seen.contains(&id) {
        return Ok("\"[Circular]\"".to_string());
    }
    seen.insert(id);
    let result = if let Dynamic::Array(array) = value {
        let parts: Result<Vec<String>, ()> = array.to_vec().iter().map(|v| stable_stringify_inner(v, seen)).collect();
        format!("[{}]", parts?.join(","))
    } else {
        let mut entries = value.own_entries();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let mut parts = Vec::new();
        for (key, item) in entries {
            if item.is_undefined() {
                continue;
            }
            parts.push(format!("{}:{}", json_quote(&key), stable_stringify_inner(&item, seen)?));
        }
        format!("{{{}}}", parts.join(","))
    };
    // Só os ancestrais contam: referências compartilhadas entre irmãos saem completas.
    seen.remove(&id);
    Ok(result)
}

/// Um objeto com as chaves de `target` cujo valor difere do de `base` (comparação estrutural, não
/// profunda: o valor de `target` entra inteiro). Chaves só de `base` não contam.
pub fn distinct(base: &Dynamic, target: &Dynamic) -> Dynamic {
    let result = ObjectRef::new();
    if !base.is_truthy() || !target.is_truthy() {
        return Dynamic::Object(result);
    }
    for key in target.own_keys() {
        let target_value = target.get(&key);
        if !equals(&base.get(&key), &target_value) {
            result.set(&key, target_value);
        }
    }
    Dynamic::Object(result)
}

/// `target[key]` ignorando a caixa da chave; se nenhuma chave casa (ou casa a chave vazia), lê `key` direto.
pub fn get_case_insensitive(target: &Dynamic, key: &JsString) -> Dynamic {
    let lowercase_key = key.to_lower_case();
    let equivalent_key = target.own_keys().into_iter().find(|k| k.to_lower_case() == lowercase_key);
    match equivalent_key {
        Some(k) if !k.is_empty() => target.get(&k),
        _ => target.get(key),
    }
}

/// Objeto só com as entradas em que `predicate(chave, valor)` vale.
pub fn filter(obj: &Dynamic, predicate: impl Fn(&JsString, &Dynamic) -> bool) -> Dynamic {
    Dynamic::object(obj.own_entries().into_iter().filter(|(key, value)| predicate(key, value)))
}

/// Objeto com as mesmas chaves e `fn_(valor, chave)` como valores.
pub fn map_values(obj: &Dynamic, mut fn_: impl FnMut(&Dynamic, &JsString) -> Dynamic) -> Dynamic {
    Dynamic::object(obj.own_entries().into_iter().map(|(key, value)| {
        let mapped = fn_(&value, &key);
        (key, mapped)
    }))
}
