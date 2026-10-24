//! Porte de `src/vs/workbench/contrib/multiDiffEditor/browser/multiDiffEditorLayoutDebug.ts`: o diff estrutural
//! de estados em JSON (`createJsonPatch`) e a aplicação desse patch num texto JSON (`applyJsonPatchToText`).
//!
//! Divergências:
//! - **O que não entra ainda.** `createMultiDiffEditorLayoutDebugModel` cria um modelo de texto `jsonl` com o
//!   `IModelService.createModel` (que no porte é método do `ModelService` concreto, fora do trait `IModelService`) e
//!   observa o `IMultiDiffEditorLayoutDebugState` do `multiDiffEditorWidgetImpl`, ainda não portado.
//!   `IMultiDiffEditorLayoutDebugStateProvider` e `isMultiDiffEditorLayoutDebugStateProvider` (o `'getLayoutDebugState'
//!   in candidate` é checagem de forma de objeto) são o contrato desse widget. Entram juntos com ele.
//! - **`JsonValue` é [`je_base::json::Value`].** O `undefined` de uma propriedade de objeto é uma chave ausente: o
//!   algoritmo trata propriedade `undefined` como propriedade JSON ausente (ele só olha `previous[key] !== undefined`
//!   e `next[key] === undefined`), então o resultado é o mesmo. `Object.is` (identidade em objetos e arrays) vale só
//!   para primitivos: dois compostos iguais por estrutura caem na recursão e dão o mesmo resultado, uma lista vazia.
//! - `JSONPath` é `Vec<EditSegment>` (o caminho do `jsonEdit`: nome de propriedade ou índice).
//! - `JsonPatch = readonly (JsonPatchOperation | JsonPatch)[]` é `Vec<JsonPatchEntry>`, em que a entrada é uma
//!   operação ou um patch aninhado.
//! - O `Number(segment)` de `fromJsonPointer` aceita aqui o inteiro decimal (com sinal) e o texto vazio (que vale
//!   `0`); os outros formatos numéricos do JavaScript (`0x10`, `1e2`, `1.5`, `Infinity`) viram
//!   [`JsonPatchError::InvalidArrayIndex`], porque o índice do `jsonEdit` é inteiro (e `-1` é "no fim do array").
//! - As exceções (`JSON.parse`, `setProperty`, `applyEdits`) são [`JsonPatchError`].

use std::fmt;

use je_base::JsString;
use je_base::js_string::trim;
use je_base::json::Value;
use je_base::json_edit::{EditSegment, JsonEditError, apply_edits, set_property};
use je_base::json_formatter::FormattingOptions;
use je_base::jsonc::{JsonSyntaxError, json_parse};

/// `JsonPatchOperation`: `{ op: 'add' | 'replace', path, value }` ou `{ op: 'remove', path }`.
#[derive(Debug, Clone, PartialEq)]
pub enum JsonPatchOperation {
    Add { path: String, value: Value },
    Replace { path: String, value: Value },
    Remove { path: String },
}

impl JsonPatchOperation {
    /// O `path` (um JSON Pointer) da operação.
    pub fn path(&self) -> &str {
        match self {
            Self::Add { path, .. } | Self::Replace { path, .. } | Self::Remove { path } => path,
        }
    }
}

/// Uma entrada de [`JsonPatch`]: uma operação ou um patch aninhado.
#[derive(Debug, Clone, PartialEq)]
pub enum JsonPatchEntry {
    Operation(JsonPatchOperation),
    Nested(JsonPatch),
}

/// `JsonPatch`.
pub type JsonPatch = Vec<JsonPatchEntry>;

impl From<JsonPatchOperation> for JsonPatchEntry {
    fn from(operation: JsonPatchOperation) -> Self {
        Self::Operation(operation)
    }
}

/// O que `applyJsonPatchToText` pode lançar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonPatchError {
    /// O `JSON.parse(text)` do `fromJsonPointer`.
    Syntax(JsonSyntaxError),
    /// O `setProperty` ou o `applyEdits` do `jsonEdit`.
    Edit(JsonEditError),
    /// Um segmento de JSON Pointer que não é índice de array inteiro (ver as divergências do módulo).
    InvalidArrayIndex(String),
}

impl fmt::Display for JsonPatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(error) => fmt::Display::fmt(error, f),
            Self::Edit(error) => fmt::Display::fmt(error, f),
            Self::InvalidArrayIndex(segment) => write!(f, "Invalid array index in JSON pointer: '{segment}'"),
        }
    }
}

impl std::error::Error for JsonPatchError {}

impl From<JsonSyntaxError> for JsonPatchError {
    fn from(error: JsonSyntaxError) -> Self {
        Self::Syntax(error)
    }
}

impl From<JsonEditError> for JsonPatchError {
    fn from(error: JsonEditError) -> Self {
        Self::Edit(error)
    }
}

/// `Object.is` nos primitivos (compostos são comparados pela recursão do chamador).
fn is_same_primitive(previous: &Value, next: &Value) -> bool {
    match (previous, next) {
        (Value::Number(a), Value::Number(b)) => (a.is_nan() && b.is_nan()) || (a == b && a.is_sign_negative() == b.is_sign_negative()),
        (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::String(a), Value::String(b)) => a == b,
        _ => false,
    }
}

/// `createJsonPatch(previous, next, path)`.
pub fn create_json_patch(previous: &Value, next: &Value, path: &[EditSegment]) -> JsonPatch {
    if is_same_primitive(previous, next) {
        return Vec::new();
    }
    let child_path = |segment: EditSegment| [path, &[segment][..]].concat();
    match (previous, next) {
        (Value::Array(previous_items), Value::Array(next_items)) => {
            if previous_items.len() != next_items.len() {
                return vec![replace(path, next)];
            }
            let mut operations = Vec::new();
            for (index, (previous_item, next_item)) in previous_items.iter().zip(next_items).enumerate() {
                let index = i64::try_from(index).expect("o índice de um array cabe em i64");
                append_json_patch_operations(&mut operations, &create_json_patch(previous_item, next_item, &child_path(EditSegment::Index(index))));
            }
            operations.into_iter().map(JsonPatchEntry::Operation).collect()
        }
        (Value::Object(previous_object), Value::Object(next_object)) => {
            let mut operations = Vec::new();
            for (key, _) in previous_object.iter() {
                if next_object.get_js(key).is_none() {
                    operations.push(JsonPatchOperation::Remove { path: to_json_pointer(&child_path(EditSegment::Property(key.clone()))) });
                }
            }
            for (key, value) in next_object.iter() {
                let value_path = child_path(EditSegment::Property(key.clone()));
                match previous_object.get_js(key) {
                    None => operations.push(JsonPatchOperation::Add { path: to_json_pointer(&value_path), value: value.clone() }),
                    Some(previous_value) => {
                        append_json_patch_operations(&mut operations, &create_json_patch(previous_value, value, &value_path));
                    }
                }
            }
            operations.into_iter().map(JsonPatchEntry::Operation).collect()
        }
        _ => vec![replace(path, next)],
    }
}

fn replace(path: &[EditSegment], value: &Value) -> JsonPatchEntry {
    JsonPatchOperation::Replace { path: to_json_pointer(path), value: value.clone() }.into()
}

/// `applyJsonPatchToText(text, patch)`.
pub fn apply_json_patch_to_text(text: &str, patch: &JsonPatch) -> Result<String, JsonPatchError> {
    let mut text: Vec<u16> = text.encode_utf16().collect();
    let options = FormattingOptions { tab_size: Some(4), insert_spaces: Some(false), eol: Some(JsString::from("\n")) };
    let mut operations = Vec::new();
    for_each_json_patch_operation(patch, &mut |operation| operations.push(operation.clone()));
    for operation in &operations {
        let path = from_json_pointer(operation.path(), &text)?;
        let value = match operation {
            JsonPatchOperation::Remove { .. } => None,
            JsonPatchOperation::Add { value, .. } | JsonPatchOperation::Replace { value, .. } => Some(value.clone()),
        };
        let edits = set_property(&text, &path, value, &options, None)?;
        text = apply_edits(&text, &edits)?;
    }
    Ok(String::from_utf16_lossy(&text))
}

fn append_json_patch_operations(target: &mut Vec<JsonPatchOperation>, patch: &JsonPatch) {
    for_each_json_patch_operation(patch, &mut |operation| target.push(operation.clone()));
}

fn for_each_json_patch_operation(patch: &JsonPatch, callback: &mut dyn FnMut(&JsonPatchOperation)) {
    for entry in patch {
        match entry {
            JsonPatchEntry::Nested(nested) => for_each_json_patch_operation(nested, callback),
            JsonPatchEntry::Operation(operation) => callback(operation),
        }
    }
}

/// `toJsonPointer`: cada segmento vira `/segmento`, com `~` escapado como `~0` e `/` como `~1`.
fn to_json_pointer(path: &[EditSegment]) -> String {
    path.iter()
        .map(|segment| {
            let text = match segment {
                EditSegment::Property(name) => name.to_string_lossy(),
                EditSegment::Index(index) => index.to_string(),
            };
            format!("/{}", text.replace('~', "~0").replace('/', "~1"))
        })
        .collect()
}

/// O `Number(segment)` de um segmento de índice de array (ver as divergências do módulo).
fn parse_array_index(segment: &str) -> Result<i64, JsonPatchError> {
    let units: Vec<u16> = segment.encode_utf16().collect();
    let trimmed = String::from_utf16_lossy(trim(&units));
    if trimmed.is_empty() {
        return Ok(0);
    }
    trimmed.parse::<i64>().map_err(|_| JsonPatchError::InvalidArrayIndex(segment.to_string()))
}

/// `fromJsonPointer(pointer, text)`: o caminho do `jsonEdit` que o ponteiro nomeia dentro do JSON de `text`
/// (os segmentos que caem num array viram índice; os demais, nome de propriedade).
fn from_json_pointer(pointer: &str, text: &[u16]) -> Result<Vec<EditSegment>, JsonPatchError> {
    if pointer.is_empty() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    let root = json_parse(text)?;
    let null = Value::Null;
    // `None` é o `undefined` de um índice fora do array.
    let mut value: Option<&Value> = Some(&root);
    let after_slash = pointer.chars().next().map_or("", |first| &pointer[first.len_utf8()..]);
    for encoded_segment in after_slash.split('/') {
        let segment = encoded_segment.replace("~1", "/").replace("~0", "~");
        if let Some(Value::Array(items)) = value {
            let index = parse_array_index(&segment)?;
            result.push(EditSegment::Index(index));
            value = usize::try_from(index).ok().and_then(|position| items.get(position));
        } else {
            let name = JsString::from(segment.as_str());
            value = match value {
                Some(Value::Object(object)) => Some(object.get_js(&name).unwrap_or(&null)),
                _ => Some(&null),
            };
            result.push(EditSegment::Property(name));
        }
    }
    Ok(result)
}
