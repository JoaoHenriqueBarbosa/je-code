//! Porte de `src/vs/workbench/api/common/extHostWebviewMessaging.ts`: a serialização das mensagens trocadas com um
//! webview (`postMessage`), em JSON com os buffers binários fora do texto, trocados por referências.
//!
//! Divergências:
//! - A mensagem (`unknown`) é [`Dynamic`], e o `JSON.stringify` com `replacer` é um percurso do valor que troca cada
//!   buffer por uma referência e depois o [`json_stringify`]. O [`Dynamic`] só representa um tipo de buffer binário, o
//!   `Uint8Array` ([`Dynamic::TypedArray`]); `ArrayBuffer` e as demais visões (`Int8Array`, `Float32Array`, ...) não
//!   existem como valor. Na ida, todo buffer vira uma referência com a visão `Uint8Array` (`byteOffset` 0 e `byteLength` o
//!   tamanho) e o dedup do `ArrayBufferSet` é pela identidade do próprio buffer. Na volta, uma referência sem visão
//!   (`ArrayBuffer`) ou com uma visão que não é `Uint8Array` lê como um `Uint8Array` com os mesmos bytes; o tipo de visão
//!   desconhecido continua sendo um erro (`Unknown array buffer view type`).
//! - O `JSON.stringify(undefined)` (que devolve `undefined`) é o texto vazio, que o `JSON.parse` do outro lado também
//!   rejeita. O `JSON.parse` é o leitor de JSON do porte, que também aceita comentários e vírgula final.
//! - `deserializeWebviewMessage` devolve os `arrayBuffers` como cópias dos bytes (`Vec<Vec<u8>>`).

use je_base::buffer::VSBuffer;
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::Error;
use je_base::js_string::JsString;
use je_base::json;

use super::ext_host_protocol::{WebviewMessageArrayBufferReference, WebviewMessageArrayBufferView, WebviewMessageArrayBufferViewType};

const REFERENCE_KEY: &str = "$$vscode_array_buffer_reference$$";

/// O `{ message: string; buffers: VSBuffer[] }` de `serializeWebviewMessage`.
#[derive(Clone, Debug)]
pub struct SerializedWebviewMessage {
    pub message: String,
    pub buffers: Vec<VSBuffer>,
}

/// O `{ message: unknown; arrayBuffers: ArrayBuffer[] }` de `deserializeWebviewMessage`.
#[derive(Clone, Debug)]
pub struct DeserializedWebviewMessage {
    pub message: Dynamic,
    pub array_buffers: Vec<Vec<u8>>,
}

/// `ArrayBufferSet`: os buffers distintos, na ordem em que apareceram.
#[derive(Default)]
struct ArrayBufferSet {
    buffers: Vec<(Option<usize>, Vec<u8>)>,
}

impl ArrayBufferSet {
    /// `add(buffer)`: o índice do buffer (o de uma inserção anterior, se a identidade já está no conjunto).
    fn add(&mut self, identity: Option<usize>, bytes: Vec<u8>) -> usize {
        if let Some(identity) = identity
            && let Some(index) = self.buffers.iter().position(|(known, _)| *known == Some(identity))
        {
            return index;
        }
        self.buffers.push((identity, bytes));
        self.buffers.len() - 1
    }
}

/// O objeto JSON de uma [`WebviewMessageArrayBufferReference`].
fn reference_to_dynamic(reference: &WebviewMessageArrayBufferReference) -> Dynamic {
    let mut entries: Vec<(&str, Dynamic)> = vec![(REFERENCE_KEY, Dynamic::Bool(true)), ("index", Dynamic::Number(reference.index as f64))];
    if let Some(view) = &reference.view {
        entries.push((
            "view",
            Dynamic::object([
                ("type", Dynamic::Number(f64::from(view.view_type.value()))),
                ("byteLength", Dynamic::Number(view.byte_length as f64)),
                ("byteOffset", Dynamic::Number(view.byte_offset as f64)),
            ]),
        ));
    }
    Dynamic::object(entries)
}

/// O `replacer` do `JSON.stringify`, aplicado a todo o valor.
fn replace_buffers(value: &Dynamic, buffers: &mut ArrayBufferSet) -> Dynamic {
    match value {
        Dynamic::TypedArray(typed) => {
            let bytes = typed.to_vec();
            let byte_length = bytes.len();
            let index = buffers.add(value.identity(), bytes);
            reference_to_dynamic(&WebviewMessageArrayBufferReference {
                index,
                view: Some(WebviewMessageArrayBufferView { view_type: WebviewMessageArrayBufferViewType::Uint8Array, byte_length, byte_offset: 0 }),
            })
        }
        Dynamic::Array(items) => Dynamic::array(items.to_vec().iter().map(|item| replace_buffers(item, buffers)).collect::<Vec<_>>()),
        Dynamic::Object(object) => {
            let entries: Vec<(JsString, Dynamic)> = object.entries().into_iter().map(|(key, item)| (key, replace_buffers(&item, buffers))).collect();
            Dynamic::object(entries)
        }
        other => other.clone(),
    }
}

/// `serializeWebviewMessage(message, { serializeBuffersForPostMessage })`.
pub fn serialize_webview_message(message: &Dynamic, serialize_buffers_for_post_message: bool) -> SerializedWebviewMessage {
    if serialize_buffers_for_post_message {
        // Extract all ArrayBuffers from the message and replace them with references.
        let mut array_buffers = ArrayBufferSet::default();
        let replaced = replace_buffers(message, &mut array_buffers);
        let serialized_message = json_stringify(&replaced).unwrap_or_default();
        let buffers = array_buffers.buffers.into_iter().map(|(_, bytes)| VSBuffer::wrap(bytes)).collect();
        SerializedWebviewMessage { message: serialized_message, buffers }
    } else {
        SerializedWebviewMessage { message: json_stringify(message).unwrap_or_default(), buffers: Vec::new() }
    }
}

/// A visão de uma referência lida do JSON.
enum ReadView {
    /// Sem `view`: a referência é a um `ArrayBuffer`.
    Absent,
    Known(WebviewMessageArrayBufferView),
    /// O `type` não nomeia nenhuma visão: o `default: throw` do original.
    Unknown,
}

/// A leitura de uma referência a buffer do objeto JSON, se o objeto for uma.
fn read_reference(value: &Dynamic) -> Option<(usize, ReadView)> {
    let Dynamic::Object(_) = value else {
        return None;
    };
    if !matches!(value.get_str(REFERENCE_KEY), Dynamic::Bool(true)) {
        return None;
    }
    let number = |value: Dynamic| match value {
        Dynamic::Number(number) if number >= 0.0 => Some(number as usize),
        _ => None,
    };
    let index = number(value.get_str("index"))?;
    let view = value.get_str("view");
    if !matches!(view, Dynamic::Object(_)) {
        return Some((index, ReadView::Absent));
    }
    let byte_length = number(view.get_str("byteLength")).unwrap_or(0);
    let byte_offset = number(view.get_str("byteOffset")).unwrap_or(0);
    let read_view = match view.get_str("type") {
        Dynamic::Number(view_type) => match WebviewMessageArrayBufferViewType::from_value(view_type) {
            Some(view_type) => ReadView::Known(WebviewMessageArrayBufferView { view_type, byte_length, byte_offset }),
            None => ReadView::Unknown,
        },
        _ => ReadView::Unknown,
    };
    Some((index, read_view))
}

/// O `reviver` do `JSON.parse`: troca cada referência pelo buffer que ela aponta.
fn revive_buffers(value: &Dynamic, array_buffers: &[Vec<u8>]) -> Result<Dynamic, Error> {
    if let Some((index, view)) = read_reference(value) {
        let Some(bytes) = array_buffers.get(index) else {
            return Ok(Dynamic::Undefined);
        };
        return match view {
            ReadView::Unknown => Err(Error::new("Unknown array buffer view type")),
            ReadView::Known(view) => {
                let end = view.byte_offset.saturating_add(view.byte_length).min(bytes.len());
                let start = view.byte_offset.min(end);
                Ok(Dynamic::typed_array(bytes[start..end].to_vec()))
            }
            ReadView::Absent => Ok(Dynamic::typed_array(bytes.clone())),
        };
    }

    match value {
        Dynamic::Array(items) => {
            let revived = items.to_vec().iter().map(|item| revive_buffers(item, array_buffers)).collect::<Result<Vec<_>, Error>>()?;
            Ok(Dynamic::array(revived))
        }
        Dynamic::Object(object) => {
            let mut entries: Vec<(JsString, Dynamic)> = Vec::new();
            for (key, item) in object.entries() {
                entries.push((key, revive_buffers(&item, array_buffers)?));
            }
            Ok(Dynamic::object(entries))
        }
        other => Ok(other.clone()),
    }
}

/// `deserializeWebviewMessage(jsonMessage, buffers)`.
pub fn deserialize_webview_message(json_message: &str, buffers: &[VSBuffer]) -> Result<DeserializedWebviewMessage, Error> {
    let array_buffers: Vec<Vec<u8>> = buffers.iter().map(VSBuffer::to_vec).collect();

    let mut errors = Vec::new();
    let parsed = json::parse_str(json_message, &mut errors).filter(|_| errors.is_empty()).ok_or_else(|| Error::new("Unexpected token in JSON"))?;
    let message = Dynamic::from_json(&parsed);
    let message = if buffers.is_empty() { message } else { revive_buffers(&message, &array_buffers)? };
    Ok(DeserializedWebviewMessage { message, array_buffers })
}
