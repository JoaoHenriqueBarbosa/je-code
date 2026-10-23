//! Porte de `src/vs/base/common/jsonSchema.ts`.
//!
//! O original é, quase todo, a interface `IJSONSchema` e tipos de TypeScript (`TypeFromJsonSchema`)
//! que só existem em tempo de compilação. Em Rust um schema é um objeto [`Dynamic`] (as chaves
//! mantêm a ordem do JavaScript e os nós têm identidade), que é o que `getCompressedContent` exige:
//! comparar nós por referência e serializar na ordem de inserção. Os campos conhecidos do
//! `IJSONSchema` (`type`, `properties`, `items`, `$ref`, `defaultSnippets`...) são as chaves do objeto.

use std::collections::HashMap;

use crate::JsString;
use crate::dynamic::{Dynamic, json_quote};
use crate::objects::safe_stringify;

struct Equals {
    schemas: Vec<Dynamic>,
    id: Option<String>,
}

fn is_object(value: &Dynamic) -> bool {
    matches!(value, Dynamic::Array(_) | Dynamic::Object(_) | Dynamic::RegExp(_) | Dynamic::Date(_) | Dynamic::TypedArray(_))
}

/// `getCompressedContent`: o schema serializado, com as subárvores iguais (e longas) trocadas por
/// `{"$ref":"#/$defs/_n"}` e as definições em `$defs`.
pub fn get_compressed_content(schema: &Dynamic) -> String {
    let mut has_dups = false;

    // Visita todos os nós do schema e junta os que são iguais.
    let mut equals: Vec<Equals> = Vec::new();
    let mut equals_by_string: HashMap<String, usize> = HashMap::new();
    let mut node_to_equals: HashMap<usize, usize> = HashMap::new();
    let schema_id = schema.identity();
    traverse_nodes(schema, &mut |next| {
        if next.identity() == schema_id {
            return true;
        }
        let val = safe_stringify(next).unwrap_or_default();
        if val.encode_utf16().count() < 30 {
            // O `$ref` ocupa uns 25 caracteres, então não se ganha nada.
            return true;
        }
        let id = next.identity().expect("objeto tem identidade");
        match equals_by_string.get(&val) {
            None => {
                equals.push(Equals { schemas: vec![next.clone()], id: None });
                equals_by_string.insert(val, equals.len() - 1);
                node_to_equals.insert(id, equals.len() - 1);
                true
            }
            Some(&eq) => {
                equals[eq].schemas.push(next.clone());
                node_to_equals.insert(id, eq);
                has_dups = true;
                false
            }
        }
    });
    equals_by_string.clear();

    if !has_dups {
        return safe_stringify(schema).unwrap_or_default();
    }

    let mut def_node_name = String::from("$defs");
    while schema.own_keys().iter().any(|k| *k == def_node_name.as_str()) {
        def_node_name.push('_');
    }

    // Os schemas que vão para `$defs`; o índice é o id.
    let mut definitions: Vec<Dynamic> = Vec::new();

    let mut stringify = |root: &Dynamic, definitions: &mut Vec<Dynamic>| -> String {
        let root_id = root.identity();
        let mut out = String::new();
        let mut replacer = |value: &Dynamic| -> Option<Dynamic> {
            let id = value.identity()?;
            if Some(id) == root_id {
                return None;
            }
            let eq = &mut equals[*node_to_equals.get(&id)?];
            if eq.schemas.len() > 1 {
                if eq.id.is_none() {
                    eq.id = Some(format!("_{}", definitions.len()));
                    definitions.push(eq.schemas[0].clone());
                }
                let reference = format!("#/{}/{}", def_node_name, eq.id.as_ref().expect("id definido"));
                return Some(Dynamic::object([("$ref", Dynamic::string(&reference))]));
            }
            None
        };
        stringify_with_replacer(root, &mut replacer, &mut out);
        out
    };

    // Serializa o schema trocando as subárvores duplicadas por `$ref`; isso acrescenta itens a `definitions`.
    let str = stringify(schema, &mut definitions);

    // Agora as definições. Cada chamada pode acrescentar mais itens, então o tamanho cresce durante o laço.
    let mut def_strings: Vec<String> = Vec::new();
    let mut i = 0;
    while i < definitions.len() {
        let definition = definitions[i].clone();
        def_strings.push(format!("\"_{i}\":{}", stringify(&definition, &mut definitions)));
        i += 1;
    }
    if !def_strings.is_empty() {
        return format!("{},\"{}\":{{{}}}}}", &str[..str.len() - 1], def_node_name, def_strings.join(","));
    }
    str
}

/// `JSON.stringify(root, replacer)`: `replacer` recebe cada valor que é objeto (menos o próprio
/// `root`, a quem o original devolve inalterado) e devolve o substituto, ou `None` para manter.
fn stringify_with_replacer(value: &Dynamic, replacer: &mut dyn FnMut(&Dynamic) -> Option<Dynamic>, out: &mut String) -> bool {
    let replaced = replacer(value);
    let value = replaced.as_ref().unwrap_or(value);
    match value {
        Dynamic::Undefined | Dynamic::Function(_) => false,
        Dynamic::Array(array) => {
            out.push('[');
            for (i, item) in array.to_vec().iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                if !stringify_with_replacer(item, replacer, out) {
                    out.push_str("null");
                }
            }
            out.push(']');
            true
        }
        Dynamic::Object(_) => {
            out.push('{');
            let mut first = true;
            for (key, item) in value.own_entries() {
                let mark = out.len();
                if !first {
                    out.push(',');
                }
                out.push_str(&json_quote(&key));
                out.push(':');
                if stringify_with_replacer(&item, replacer, out) {
                    first = false;
                } else {
                    out.truncate(mark);
                }
            }
            out.push('}');
            true
        }
        _ => {
            out.push_str(&safe_stringify(value).unwrap_or_else(|| "null".to_string()));
            true
        }
    }
}

/// `traverseNodes`: visita cada nó do schema; `visit` devolve se deve descer nos filhos.
fn traverse_nodes(root: &Dynamic, visit: &mut dyn FnMut(&Dynamic) -> bool) {
    if !is_object(root) {
        return;
    }
    let mut to_walk: Vec<Dynamic> = vec![root.clone()];
    while let Some(next) = to_walk.pop() {
        if !visit(&next) {
            continue;
        }
        let get = |key: &str| next.get(&JsString::from(key));
        let mut push_entry = |entry: Dynamic| {
            if is_object(&entry) {
                to_walk.push(entry);
            }
        };
        for key in [
            "additionalItems",
            "additionalProperties",
            "not",
            "contains",
            "propertyNames",
            "if",
            "then",
            "else",
            "unevaluatedItems",
            "unevaluatedProperties",
        ] {
            push_entry(get(key));
        }
        for key in ["definitions", "$defs", "properties", "patternProperties", "dependencies", "dependentSchemas"] {
            let map = get(key);
            if is_object(&map) {
                // `for (const key in map)`: as propriedades próprias (de um array, os índices).
                for (_, entry) in map.own_entries() {
                    push_entry(entry);
                }
            }
        }
        for key in ["anyOf", "allOf", "oneOf", "prefixItems"] {
            if let Dynamic::Array(array) = get(key) {
                for entry in array.to_vec() {
                    push_entry(entry);
                }
            }
        }
        match get("items") {
            Dynamic::Array(array) => {
                for entry in array.to_vec() {
                    push_entry(entry);
                }
            }
            items => push_entry(items),
        }
    }
}
