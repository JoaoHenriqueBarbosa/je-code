//! Porte de `src/vs/base/common/buffer.ts`: `VSBuffer`, as codificações base64 e hex, as leituras e
//! escritas de inteiros em bytes e os atalhos de stream de buffer.
//!
//! `VSBuffer` embrulha um `Uint8Array`. Aqui o armazenamento é um `Rc<RefCell<Vec<u8>>>` e o `VSBuffer` é
//! uma janela (`início`, `tamanho`) sobre ele, então [`VSBuffer::slice`] compartilha a memória como o
//! `subarray` do original, e copiar o handle (`Clone`) é copiar a referência, como atribuir um objeto em JS.
//! O `clone()` do original, que copia os bytes, é [`VSBuffer::copy`].
//!
//! Diferenças de forma, sem mudança de resultado:
//!
//! - não existe o `Buffer` do Node, então `isNativeBuffer` e a opção `dontUseNodeBuffer` de `fromString`
//!   não existem (a codificação é sempre a do `TextEncoder`);
//! - `fromByteArray` recebe `&[u8]` (o `number[]` do original truncaria cada valor para um byte);
//! - o `set` do original aceita `VSBuffer`, `Uint8Array`, `ArrayBuffer` e views, e lança `Unknown argument`
//!   para o resto; aqui são [`VSBuffer::set`] (outro `VSBuffer`) e [`VSBuffer::set_bytes`], e o resto não
//!   compila;
//! - `readUInt32LE` do original combina os bytes com `|` e devolve um inteiro de 32 bits COM sinal (um byte
//!   alto maior que 127 dá número negativo); o porte devolve `i32` pelo mesmo motivo;
//! - ler ou escrever fora dos bytes dá `undefined`/ignora no original; aqui lança um `RangeError`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::errors::{self, Error};
use crate::promise::Promise;
use crate::stream::{
    self, Readable, ReadableBufferedStream, ReadableStream, ReadableStreamEvents, Reducer, Transformer, WriteableStreamImpl,
    WriteableStreamOptions,
};

fn range_error(message: String) -> ! {
    errors::throw(Error::named("RangeError", message))
}

fn syntax_error(message: String) -> ! {
    errors::throw(Error::named("SyntaxError", message))
}

/// `VSBuffer`.
#[derive(Clone)]
pub struct VSBuffer {
    storage: Rc<RefCell<Vec<u8>>>,
    start: usize,
    len: usize,
}

impl VSBuffer {
    /// `VSBuffer.alloc(byteLength)`. O original pode devolver memória não inicializada; aqui é zerada.
    pub fn alloc(byte_length: usize) -> VSBuffer {
        Self::wrap(vec![0; byte_length])
    }

    /// `VSBuffer.wrap(actual)`: assume os bytes sem copiar.
    pub fn wrap(actual: Vec<u8>) -> VSBuffer {
        let len = actual.len();
        VSBuffer { storage: Rc::new(RefCell::new(actual)), start: 0, len }
    }

    /// `VSBuffer.fromString(source)`: os bytes UTF-8 do texto.
    pub fn from_string(source: &str) -> VSBuffer {
        Self::wrap(source.as_bytes().to_vec())
    }

    /// `VSBuffer.fromByteArray(source)`.
    pub fn from_byte_array(source: &[u8]) -> VSBuffer {
        Self::wrap(source.to_vec())
    }

    /// `VSBuffer.concat(buffers, totalLength?)`.
    pub fn concat(buffers: &[VSBuffer], total_length: Option<usize>) -> VSBuffer {
        let total_length = total_length.unwrap_or_else(|| buffers.iter().map(VSBuffer::byte_length).sum());

        let ret = VSBuffer::alloc(total_length);
        let mut offset = 0;
        for element in buffers {
            ret.set(element, offset);
            offset += element.byte_length();
        }

        ret
    }

    /// `byteLength`.
    pub fn byte_length(&self) -> usize {
        self.len
    }

    /// O `buffer` do original (o `Uint8Array`): lê os bytes da janela.
    pub fn with_bytes<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        f(&self.storage.borrow()[self.start..self.start + self.len])
    }

    fn with_bytes_mut<R>(&self, f: impl FnOnce(&mut [u8]) -> R) -> R {
        f(&mut self.storage.borrow_mut()[self.start..self.start + self.len])
    }

    /// Os bytes da janela, copiados.
    pub fn to_vec(&self) -> Vec<u8> {
        self.with_bytes(<[u8]>::to_vec)
    }

    /// Os dois buffers enxergam a mesma memória (`a.buffer === b.buffer` do original, ou views do mesmo
    /// `ArrayBuffer`)?
    pub fn shares_storage_with(&self, other: &VSBuffer) -> bool {
        Rc::ptr_eq(&self.storage, &other.storage)
    }

    /// `clone()` do original: um buffer novo com uma cópia dos bytes.
    pub fn copy(&self) -> VSBuffer {
        let result = VSBuffer::alloc(self.len);
        result.set(self, 0);
        result
    }

    /// `slice(start?, end?)`: uma janela sobre a mesma memória (`subarray`), com índices negativos
    /// contados do fim e limitados ao tamanho.
    pub fn slice(&self, start: Option<i64>, end: Option<i64>) -> VSBuffer {
        let resolve = |index: Option<i64>, default: usize| match index {
            None => default,
            Some(i) if i < 0 => (self.len as i64 + i).max(0) as usize,
            Some(i) => (i as usize).min(self.len),
        };
        let from = resolve(start, 0);
        let to = resolve(end, self.len).max(from);
        VSBuffer { storage: self.storage.clone(), start: self.start + from, len: to - from }
    }

    /// `set(array: VSBuffer, offset?)`.
    pub fn set(&self, array: &VSBuffer, offset: usize) {
        // Copia antes de escrever: o buffer de origem pode ser uma janela do mesmo armazenamento.
        self.set_bytes(&array.to_vec(), offset);
    }

    /// `set(array: Uint8Array, offset?)`. Lança `RangeError` se não couber.
    pub fn set_bytes(&self, array: &[u8], offset: usize) {
        self.with_bytes_mut(|bytes| {
            if offset > bytes.len() || array.len() > bytes.len() - offset {
                range_error("offset is out of bounds".to_string());
            }
            bytes[offset..offset + array.len()].copy_from_slice(array);
        });
    }

    pub fn read_u_int32_be(&self, offset: usize) -> u32 {
        self.with_bytes(|bytes| read_u_int32_be(bytes, offset))
    }

    pub fn write_u_int32_be(&self, value: u32, offset: usize) {
        self.with_bytes_mut(|bytes| write_u_int32_be(bytes, value, offset));
    }

    pub fn read_u_int32_le(&self, offset: usize) -> i32 {
        self.with_bytes(|bytes| read_u_int32_le(bytes, offset))
    }

    pub fn write_u_int32_le(&self, value: u32, offset: usize) {
        self.with_bytes_mut(|bytes| write_u_int32_le(bytes, value, offset));
    }

    pub fn read_u_int8(&self, offset: usize) -> u8 {
        self.with_bytes(|bytes| read_u_int8(bytes, offset))
    }

    pub fn write_u_int8(&self, value: u8, offset: usize) {
        self.with_bytes_mut(|bytes| write_u_int8(bytes, value, offset));
    }

    /// `indexOf(subarray, offset = 0)`: `-1` se não achar.
    pub fn index_of(&self, subarray: &VSBuffer, offset: usize) -> i64 {
        let needle = subarray.to_vec();
        self.with_bytes(|haystack| binary_index_of(haystack, &needle, offset))
    }

    pub fn equals(&self, other: &VSBuffer) -> bool {
        if Rc::ptr_eq(&self.storage, &other.storage) && self.start == other.start && self.len == other.len {
            return true;
        }

        if self.len != other.len {
            return false;
        }

        self.with_bytes(|a| other.with_bytes(|b| a == b))
    }
}

/// `toString()`: decodifica como UTF-8 sem descartar o BOM (`ignoreBOM: true`), com U+FFFD para o que
/// for inválido.
impl std::fmt::Display for VSBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.with_bytes(|bytes| f.write_str(&String::from_utf8_lossy(bytes)))
    }
}

impl std::fmt::Debug for VSBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.with_bytes(|bytes| write!(f, "VSBuffer({bytes:?})"))
    }
}

/// Like String.indexOf, but works on Uint8Arrays.
/// Uses the boyer-moore-horspool algorithm to be reasonably speedy.
pub fn binary_index_of(haystack: &[u8], needle: &[u8], offset: usize) -> i64 {
    let needle_len = needle.len();
    let haystack_len = haystack.len();

    if needle_len == 0 {
        return 0;
    }

    if needle_len == 1 {
        // `Uint8Array#indexOf(byte, fromIndex)`
        return haystack
            .get(offset..)
            .and_then(|rest| rest.iter().position(|&b| b == needle[0]))
            .map_or(-1, |i| (i + offset) as i64);
    }

    if needle_len > haystack_len.saturating_sub(offset) {
        return -1;
    }

    // find index of the subarray using boyer-moore-horspool algorithm. A tabela do original é um
    // `Uint8Array(256)`, que trunca os deslocamentos de agulhas com 256 bytes ou mais; aqui os
    // deslocamentos não truncam, o que só os torna corretos (e mais rápidos), sem mudar o resultado.
    let mut table = [needle_len; 256];
    for (i, &byte) in needle.iter().enumerate() {
        table[usize::from(byte)] = needle_len - i - 1;
    }

    let mut i = offset + needle_len - 1;
    let mut j = needle_len - 1;
    let mut result = -1;
    while i < haystack_len {
        if haystack[i] == needle[j] {
            if j == 0 {
                result = i as i64;
                break;
            }

            i -= 1;
            j -= 1;
        } else {
            i += (needle_len - j).max(table[usize::from(haystack[i])]);
            j = needle_len - 1;
        }
    }

    result
}

fn byte_at(source: &[u8], offset: usize) -> u8 {
    match source.get(offset) {
        Some(&byte) => byte,
        None => range_error(format!("offset {offset} is out of bounds")),
    }
}

fn byte_at_mut(destination: &mut [u8], offset: usize) -> &mut u8 {
    let len = destination.len();
    match destination.get_mut(offset) {
        Some(byte) => byte,
        None => range_error(format!("offset {offset} is out of bounds for {len} bytes")),
    }
}

pub fn read_u_int16_le(source: &[u8], offset: usize) -> u16 {
    u16::from(byte_at(source, offset)) | (u16::from(byte_at(source, offset + 1)) << 8)
}

pub fn write_u_int16_le(destination: &mut [u8], value: u16, offset: usize) {
    *byte_at_mut(destination, offset) = (value & 0xff) as u8;
    *byte_at_mut(destination, offset + 1) = (value >> 8) as u8;
}

pub fn read_u_int32_be(source: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([byte_at(source, offset), byte_at(source, offset + 1), byte_at(source, offset + 2), byte_at(source, offset + 3)])
}

pub fn write_u_int32_be(destination: &mut [u8], value: u32, offset: usize) {
    for (i, byte) in value.to_be_bytes().into_iter().enumerate() {
        *byte_at_mut(destination, offset + i) = byte;
    }
}

/// O `readUInt32LE` do original, com o sinal que o `|` de inteiros de 32 bits lhe dá.
pub fn read_u_int32_le(source: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes([byte_at(source, offset), byte_at(source, offset + 1), byte_at(source, offset + 2), byte_at(source, offset + 3)])
}

pub fn write_u_int32_le(destination: &mut [u8], value: u32, offset: usize) {
    for (i, byte) in value.to_le_bytes().into_iter().enumerate() {
        *byte_at_mut(destination, offset + i) = byte;
    }
}

pub fn read_u_int8(source: &[u8], offset: usize) -> u8 {
    byte_at(source, offset)
}

pub fn write_u_int8(destination: &mut [u8], value: u8, offset: usize) {
    *byte_at_mut(destination, offset) = value;
}

/// `VSBufferReadable`, `VSBufferReadableStream` e as demais interfaces de stream de `VSBuffer`.
pub type VSBufferReadableStream = Rc<dyn ReadableStream<VSBuffer>>;

fn concat_reducer() -> Reducer<VSBuffer> {
    Rc::new(|chunks| VSBuffer::concat(&chunks, None))
}

pub fn readable_to_buffer(readable: &mut dyn Readable<VSBuffer>) -> VSBuffer {
    stream::consume_readable(readable, |chunks| VSBuffer::concat(&chunks, None))
}

pub fn buffer_to_readable(buffer: VSBuffer) -> impl Readable<VSBuffer> {
    stream::to_readable(buffer)
}

pub fn stream_to_buffer(stream: &dyn ReadableStreamEvents<VSBuffer>) -> Promise<VSBuffer> {
    stream::consume_stream(stream, concat_reducer())
}

pub fn buffered_stream_to_buffer(buffered_stream: ReadableBufferedStream<VSBuffer>) -> Promise<VSBuffer> {
    if buffered_stream.ended {
        return Promise::resolved(VSBuffer::concat(&buffered_stream.buffer, None));
    }

    let already_read = buffered_stream.buffer.clone();
    // Include already read chunks... and all additional chunks
    stream_to_buffer(&*buffered_stream.stream).then(move |rest| {
        let mut chunks = already_read;
        chunks.push(rest);
        VSBuffer::concat(&chunks, None)
    })
}

pub fn buffer_to_stream(buffer: VSBuffer) -> Rc<WriteableStreamImpl<VSBuffer>> {
    stream::to_stream(buffer, concat_reducer())
}

/// O dado de `Uint8Array | string` de `streamToBufferReadableStream`.
#[derive(Clone)]
pub enum BytesOrString {
    Bytes(Vec<u8>),
    Text(String),
}

pub fn stream_to_buffer_readable_stream(stream: &dyn ReadableStreamEvents<BytesOrString>) -> Rc<WriteableStreamImpl<VSBuffer>> {
    stream::transform(
        stream,
        Transformer {
            data: Rc::new(|data| match data {
                BytesOrString::Text(text) => VSBuffer::from_string(text),
                BytesOrString::Bytes(bytes) => VSBuffer::wrap(bytes.clone()),
            }),
            error: None,
        },
        concat_reducer(),
    )
}

pub fn new_writeable_buffer_stream(options: Option<WriteableStreamOptions>) -> Rc<WriteableStreamImpl<VSBuffer>> {
    stream::new_writeable_stream(Some(concat_reducer()), options)
}

pub fn prefixed_buffer_readable(prefix: VSBuffer, readable: Box<dyn Readable<VSBuffer>>) -> impl Readable<VSBuffer> {
    stream::prefixed_readable(prefix, readable, |chunks| VSBuffer::concat(&chunks, None))
}

pub fn prefixed_buffer_stream(prefix: VSBuffer, stream: &dyn ReadableStreamEvents<VSBuffer>) -> Rc<WriteableStreamImpl<VSBuffer>> {
    stream::prefixed_stream(prefix, stream, concat_reducer())
}

/// O estado de `decodeBase64`: acumula 6 bits por caractere e emite um byte por vez.
struct Base64Decoder {
    /// A tabela do original tem `floor(length / 4 * 3)` bytes; escrever além do fim de um `Uint8Array` é
    /// ignorado (o que acontece no preenchimento final de entradas sem `=`), e o contador continua.
    buffer: Vec<u8>,
    bufi: usize,
    building: u32,
    remainder: u8,
}

impl Base64Decoder {
    fn put(&mut self, byte: u32) {
        if let Some(slot) = self.buffer.get_mut(self.bufi) {
            *slot = byte as u8;
        }
        self.bufi += 1;
    }

    fn append(&mut self, value: u32) {
        match self.remainder {
            3 => {
                self.put(self.building | value);
                self.remainder = 0;
            }
            2 => {
                self.put(self.building | (value >> 2));
                self.building = value << 6;
                self.remainder = 3;
            }
            1 => {
                self.put(self.building | (value >> 4));
                self.building = value << 4;
                self.remainder = 2;
            }
            _ => {
                self.building = value << 2;
                self.remainder = 1;
            }
        }
    }
}

/// Decodes base64 to a uint8 array. URL-encoded and unpadded base64 is allowed.
pub fn decode_base64(encoded: &str) -> VSBuffer {
    let units: Vec<u16> = encoded.encode_utf16().collect();
    let mut decoder = Base64Decoder { buffer: vec![0u8; units.len() * 3 / 4], bufi: 0, building: 0, remainder: 0 };

    for &code in &units {
        // See https://datatracker.ietf.org/doc/html/rfc4648#section-4
        match code {
            65..=90 => decoder.append(u32::from(code) - 65), // A-Z starts ranges from char code 65 to 90
            97..=122 => decoder.append(u32::from(code) - 97 + 26), // a-z starts ranges from char code 97 to 122, starting at byte 26
            48..=57 => decoder.append(u32::from(code) - 48 + 52), // 0-9 starts ranges from char code 48 to 58, starting at byte 52
            43 | 45 => decoder.append(62), // "+" or "-" for URLS
            47 | 95 => decoder.append(63), // "/" or "_" for URLS
            61 => break,                   // "="
            _ => syntax_error(format!("Unexpected base64 character {}", String::from_utf16_lossy(&[code]))),
        }
    }

    let unpadded = decoder.bufi;
    while decoder.remainder > 0 {
        decoder.append(0);
    }

    // slice is needed to account for overestimation due to padding
    VSBuffer::wrap(decoder.buffer).slice(Some(0), Some(unpadded as i64))
}

const BASE64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const BASE64_URL_SAFE_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Encodes a buffer to a base64 string.
pub fn encode_base64(buffer: &VSBuffer, padded: bool, url_safe: bool) -> String {
    let dictionary = if url_safe { BASE64_URL_SAFE_ALPHABET } else { BASE64_ALPHABET };
    let at = |index: u8| char::from(dictionary[usize::from(index & 0b111111)]);

    buffer.with_bytes(|buffer| {
        let mut output = String::with_capacity(buffer.len().div_ceil(3) * 4);

        let mut chunks = buffer.chunks_exact(3);
        for chunk in &mut chunks {
            let (a, b, c) = (chunk[0], chunk[1], chunk[2]);

            output.push(at(a >> 2));
            output.push(at(a << 4 | b >> 4));
            output.push(at(b << 2 | c >> 6));
            output.push(at(c));
        }

        match *chunks.remainder() {
            [a] => {
                output.push(at(a >> 2));
                output.push(at(a << 4));
                if padded {
                    output.push_str("==");
                }
            }
            [a, b] => {
                output.push(at(a >> 2));
                output.push(at(a << 4 | b >> 4));
                output.push(at(b << 2));
                if padded {
                    output.push('=');
                }
            }
            _ => {}
        }

        output
    })
}

pub fn encode_hex(buffer: &VSBuffer) -> String {
    buffer.with_bytes(|bytes| bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub fn decode_hex(hex: &str) -> VSBuffer {
    let units: Vec<u16> = hex.encode_utf16().collect();
    if units.len() % 2 != 0 {
        syntax_error("Hex string must have an even length".to_string());
    }
    let out = units.chunks_exact(2).enumerate().map(|(i, pair)| (decode_hex_char(pair[0], i * 2) << 4) | decode_hex_char(pair[1], i * 2 + 1)).collect();
    VSBuffer::wrap(out)
}

fn decode_hex_char(unit: u16, position: usize) -> u8 {
    match unit {
        48..=57 => (unit - 48) as u8,  // '0'-'9'
        97..=102 => (unit - 87) as u8, // 'a'-'f'
        65..=70 => (unit - 55) as u8,  // 'A'-'F'
        _ => syntax_error(format!("Invalid hex character at position {position}")),
    }
}
