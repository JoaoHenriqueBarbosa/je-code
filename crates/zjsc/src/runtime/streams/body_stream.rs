//! O `ReadableStream` de um corpo em memória (`Blob.prototype.stream()`, `Response.body`, `Request.body`). Medido no bun
//! 1.4.2: não é stream de bytes (`getReader({mode:'byob'})` lança `A BYOB reader requires a ReadableStream with an
//! underlying byte source`, `type` é `undefined`), entrega `Uint8Array` e parte o conteúdo em dois pedaços quando passa de
//! 16384 bytes (o primeiro com 16384, o resto no segundo); conteúdo vazio fecha sem nenhum pedaço.

use std::cell::RefCell;
use std::rc::Rc;

use super::readable::{create_native, is_locked_or_disturbed, is_stream, read_all, resolved_with, take_error, type_error_value, NativeSource, ReadableHandle, Sinks};
use super::trace::hold;
use crate::runtime::blob::string_bytes;
use crate::runtime::text_decoder::{decode_chunk, input_bytes, text_value, DecoderState, Encoding};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::uint8_array_base64::create_uint8_array;

/// O tamanho do primeiro pedaço entregue (e o corte do resto).
const FIRST_CHUNK: usize = 16384;

/// A fonte nativa: tudo já foi enfileirado na criação, então `pull` e `cancel` não têm o que fazer.
struct Memory;

impl NativeSource for Memory {
    fn pull(&self, global_object: &JSGlobalObject) -> JSValue {
        resolved_with(global_object, JSValue::undefined())
    }

    fn cancel(&self, global_object: &JSGlobalObject, _reason: JSValue) -> JSValue {
        resolved_with(global_object, JSValue::undefined())
    }

    fn trace(&self, _out: &mut Vec<JSValue>) {}
}

/// Um `ReadableStream` novo que entrega `bytes` e fecha.
pub(crate) fn bytes_stream(global_object: &JSGlobalObject, bytes: &[u8]) -> JSValue {
    let (value, handle) = create_native(global_object, Rc::new(Memory), 1.0, None, JSValue::undefined());
    let pieces: &[&[u8]] = if bytes.len() > FIRST_CHUNK { &[&bytes[..FIRST_CHUNK], &bytes[FIRST_CHUNK..]] } else { &[bytes] };
    for piece in pieces.iter().filter(|piece| !piece.is_empty()) {
        if let Ok(array) = create_uint8_array(global_object, piece.len()) {
            array.with_vector_mut(|destination| destination[..piece.len()].copy_from_slice(piece));
            let _ = handle.enqueue(global_object, array.as_value());
        }
    }
    handle.close(global_object);
    value
}

/// Um `ReadableStream` novo que já nasce em erro com `error` (a leitura que o `Blob` de arquivo não pôde fazer).
pub(crate) fn errored_stream(global_object: &JSGlobalObject, error: JSValue) -> JSValue {
    let (value, handle) = create_native(global_object, Rc::new(Memory), 1.0, None, JSValue::undefined());
    handle.error(global_object, error);
    value
}

/// O stream está travado por um leitor ou já foi lido: o que `bodyUsed` e a segunda leitura do corpo olham.
pub(crate) fn stream_used(value: JSValue) -> bool {
    is_locked_or_disturbed(value)
}

/// `value` é um `ReadableStream` (o corpo que o usuário passa a `new Response(stream)`).
pub(crate) fn is_readable_stream(value: JSValue) -> bool {
    is_stream(value)
}

/// O que `read_stream_bytes` faz com um pedaço que não é texto, `ArrayBuffer` nem view (medido no bun 1.4.2, sem `code`).
#[derive(Clone, Copy)]
pub(crate) enum Chunks {
    /// Rejeita `Expected text, ArrayBuffer or ArrayBufferView` (o corpo de `Response`, `text()` e `json()` do stream).
    Text,
    /// Rejeita `Expected an ArrayBuffer, ArrayBufferView, or string chunk` (`bytes()` do stream).
    Bytes,
    /// Converte como `String(pedaço)`, então `{}` vira `[object Object]` e símbolo rejeita (`blob()` do stream).
    Stringify,
}

/// Lê o `ReadableStream` do usuário até o fim e entrega todos os bytes a `done` (ou o erro: do stream, ou o `TypeError`
/// de um pedaço que `chunks` não aceita). Medido no bun 1.4.2: string vira UTF-8, `ArrayBuffer` e qualquer view valem
/// como bytes. Devolve `false` se `stream` já estava travado ou lido. `held` são os valores JS que `done` captura (a
/// promessa que ele liquida): o coletor os visita até a leitura acabar.
pub(crate) fn read_stream_bytes(
    global_object: &JSGlobalObject,
    stream: JSValue,
    chunks: Chunks,
    held: &[JSValue],
    done: impl Fn(&JSGlobalObject, Result<Vec<u8>, JSValue>) + 'static,
) -> bool {
    let buffer = Rc::new(RefCell::new(Vec::new()));
    let sink = buffer.clone();
    let sinks = Rc::new(Sinks {
        chunk: Box::new(move |global_object, chunk| {
            let bytes = if chunk.is_string() {
                string_bytes(global_object, chunk).map_err(|_| take_error(global_object))?
            } else {
                match (input_bytes(chunk), chunks) {
                    (Some(bytes), _) => bytes,
                    (None, Chunks::Stringify) => string_bytes(global_object, chunk).map_err(|_| take_error(global_object))?,
                    (None, Chunks::Text) => return Err(type_error_value(global_object, "Expected text, ArrayBuffer or ArrayBufferView")),
                    (None, Chunks::Bytes) => return Err(type_error_value(global_object, "Expected an ArrayBuffer, ArrayBufferView, or string chunk")),
                }
            };
            sink.borrow_mut().extend_from_slice(&bytes);
            Ok(())
        }),
        end: Box::new(move |global_object, result| done(global_object, result.map(|()| std::mem::take(&mut *buffer.borrow_mut())))),
        held: vec![hold(held.to_vec())],
    });
    read_all(global_object, stream, sinks)
}

/// Entrega a `handle` o texto decodificado de `bytes` (nada se vazio), como um pedaço string.
fn enqueue_text(global_object: &JSGlobalObject, handle: &ReadableHandle, state: &mut DecoderState, bytes: &[u8], last: bool) {
    if let Ok(text) = decode_chunk(state, bytes, !last) {
        if !text.is_empty() {
            let _ = handle.enqueue(global_object, text_value(global_object, &text));
        }
    }
}

/// `textStream()` de `Response`, `Request`: um `ReadableStream` de pedaços string (UTF-8, o BOM do começo sai, byte
/// inválido vira U+FFFD, sequência partida entre pedaços se junta). Medido no bun 1.4.2: o conteúdo em memória sai num
/// pedaço só (40000 bytes dão um pedaço de 40000), vazio fecha sem pedaço; num corpo `ReadableStream` do usuário cada
/// pedaço vira um texto e um pedaço que não é view nem `ArrayBuffer` (string inclusive) erra o stream com `TypeError`
/// `Body.textStream() received a chunk that is not a BufferSource`.
pub(crate) fn text_stream(global_object: &JSGlobalObject, source: Option<JSValue>, bytes: &[u8]) -> JSValue {
    let (value, handle) = create_native(global_object, Rc::new(Memory), 1.0, None, JSValue::undefined());
    let state = Rc::new(RefCell::new(DecoderState::new(Encoding::Utf8, false, false)));
    let Some(source) = source else {
        enqueue_text(global_object, &handle, &mut state.borrow_mut(), bytes, true);
        handle.close(global_object);
        return value;
    };
    let (chunk_handle, chunk_state) = (handle.clone(), state.clone());
    let end_handle = handle;
    let sinks = Rc::new(Sinks {
        chunk: Box::new(move |global_object, chunk| {
            let bytes = input_bytes(chunk).ok_or_else(|| type_error_value(global_object, "Body.textStream() received a chunk that is not a BufferSource"))?;
            enqueue_text(global_object, &chunk_handle, &mut chunk_state.borrow_mut(), &bytes, false);
            Ok(())
        }),
        end: Box::new(move |global_object, result| match result {
            Ok(()) => {
                enqueue_text(global_object, &end_handle, &mut state.borrow_mut(), &[], true);
                end_handle.close(global_object);
            }
            Err(error) => end_handle.error(global_object, error),
        }),
        // Só o `ReadableHandle` (estado registrado num `OBJECTS`) e o decodificador (bytes): nada de valor JS.
        held: Vec::new(),
    });
    read_all(global_object, source, sinks);
    value
}
