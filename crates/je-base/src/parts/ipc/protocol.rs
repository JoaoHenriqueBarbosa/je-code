//! A codificação binária do IPC do VS Code (`serialize` e `deserialize` de `ipc.ts`), byte a byte.
//!
//! Um valor é um byte de tipo seguido do conteúdo:
//!
//! | tipo | valor            | conteúdo                                         |
//! | ---- | ---------------- | ------------------------------------------------ |
//! | 0    | `undefined`      | nada                                             |
//! | 1    | `string`         | comprimento (VQL) e bytes UTF-8                  |
//! | 2    | `Buffer` nativo do Node | comprimento (VQL) e bytes                 |
//! | 3    | `VSBuffer`       | comprimento (VQL) e bytes                        |
//! | 4    | `Array`          | número de itens (VQL) e os itens                 |
//! | 5    | outro            | comprimento (VQL) e o `JSON.stringify` em UTF-8  |
//! | 6    | inteiro de 32 bits | o inteiro em VQL                               |
//!
//! O VQL é o de 7 bits por byte, menos significativo primeiro, com o bit alto marcando continuação;
//! um inteiro negativo ocupa 5 bytes (o deslocamento é sem sinal, como o `>>>` do original).

use crate::buffer::VSBuffer;
use crate::errors::{self, Error};
use crate::json;
use crate::marshalling::{Marshalled, from_json, json_stringify};

/// `IReader`.
pub trait IReader {
    fn read(&mut self, bytes: usize) -> VSBuffer;
}

/// `IWriter`.
pub trait IWriter {
    fn write(&mut self, buffer: VSBuffer);
}

/// `readIntVQL`. Se os bytes acabam no meio, o byte que falta vale 0 (o `undefined & 0x7f` do original).
fn read_int_vql(reader: &mut dyn IReader) -> i32 {
    let mut value: u32 = 0;
    let mut n: u32 = 0;
    loop {
        let next = reader.read(1);
        let byte = if next.byte_length() == 0 { 0 } else { next.read_u_int8(0) };
        // O deslocamento do JavaScript usa só os 5 bits baixos da contagem.
        value |= (u32::from(byte) & 0b0111_1111) << (n & 31);
        if byte & 0b1000_0000 == 0 {
            return value as i32;
        }
        n += 7;
    }
}

/// `writeInt32VQL`.
fn write_int32_vql(writer: &mut dyn IWriter, value: i32) {
    let mut v = value as u32;
    if v == 0 {
        writer.write(VSBuffer::wrap(vec![0]));
        return;
    }
    let mut bytes = Vec::with_capacity(5);
    while v != 0 {
        let mut byte = (v & 0b0111_1111) as u8;
        v >>= 7;
        if v > 0 {
            byte |= 0b1000_0000;
        }
        bytes.push(byte);
    }
    writer.write(VSBuffer::wrap(bytes));
}

/// `BufferReader`.
pub struct BufferReader {
    buffer: VSBuffer,
    pos: usize,
}

impl BufferReader {
    pub fn new(buffer: VSBuffer) -> Self {
        Self { buffer, pos: 0 }
    }
}

impl IReader for BufferReader {
    fn read(&mut self, bytes: usize) -> VSBuffer {
        let result = self.buffer.slice(Some(self.pos as i64), Some((self.pos + bytes) as i64));
        self.pos += result.byte_length();
        result
    }
}

/// `BufferWriter`.
#[derive(Default)]
pub struct BufferWriter {
    buffers: Vec<VSBuffer>,
}

impl BufferWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// `buffer`: tudo o que foi escrito, junto.
    pub fn buffer(&self) -> VSBuffer {
        VSBuffer::concat(&self.buffers, None)
    }

    /// `dispose`: solta os buffers (para a pilha de um erro de serialização não os segurar).
    pub fn dispose(&mut self) {
        self.buffers.clear();
    }
}

impl IWriter for BufferWriter {
    fn write(&mut self, buffer: VSBuffer) {
        self.buffers.push(buffer);
    }
}

mod data_type {
    pub const UNDEFINED: u8 = 0;
    pub const STRING: u8 = 1;
    pub const BUFFER: u8 = 2;
    pub const VS_BUFFER: u8 = 3;
    pub const ARRAY: u8 = 4;
    pub const OBJECT: u8 = 5;
    pub const INT: u8 = 6;
}

fn write_type_and_bytes(writer: &mut dyn IWriter, data_type: u8, bytes: Vec<u8>) {
    writer.write(VSBuffer::wrap(vec![data_type]));
    write_int32_vql(writer, bytes.len() as i32);
    writer.write(VSBuffer::wrap(bytes));
}

/// `(data | 0) === data`: um número que é um inteiro de 32 bits.
fn as_int32(n: f64) -> Option<i32> {
    let int = n as i32;
    (f64::from(int) == n).then_some(int)
}

/// `serialize`. Um `Uint8Array` comum ([`Marshalled::Bytes`]) vai como objeto JSON (`{"0":9,"1":8}`): só um `Buffer` do
/// Node (`VSBuffer.isNativeBuffer`) usa o tipo 2, e o porte não tem esse tipo de valor para enviar. A leitura do
/// tipo 2 devolve um [`Marshalled::Bytes`], o `Uint8Array` que o original devolve.
pub fn serialize(writer: &mut dyn IWriter, data: &Marshalled) {
    match data {
        Marshalled::Undefined => writer.write(VSBuffer::wrap(vec![data_type::UNDEFINED])),
        Marshalled::String(s) => write_type_and_bytes(writer, data_type::STRING, s.to_string_lossy().into_bytes()),
        Marshalled::Buffer(buffer) => write_type_and_bytes(writer, data_type::VS_BUFFER, buffer.to_vec()),
        Marshalled::Array(items) => {
            writer.write(VSBuffer::wrap(vec![data_type::ARRAY]));
            write_int32_vql(writer, items.len() as i32);
            for item in items {
                serialize(writer, item);
            }
        }
        Marshalled::Number(n) if as_int32(*n).is_some() => {
            writer.write(VSBuffer::wrap(vec![data_type::INT]));
            write_int32_vql(writer, as_int32(*n).expect("conferido no guarda"));
        }
        other => {
            // `JSON.stringify` devolve `undefined` só para a raiz `undefined`, que o primeiro ramo já tratou.
            let json = json_stringify(other).map(|j| j.to_string_lossy()).unwrap_or_default();
            write_type_and_bytes(writer, data_type::OBJECT, json.into_bytes());
        }
    }
}

/// `deserialize`. Um byte de tipo desconhecido (ou a falta dele) dá `undefined`, como o `switch` do original.
/// JSON inválido num valor do tipo objeto lança `SyntaxError`.
pub fn deserialize(reader: &mut dyn IReader) -> Marshalled {
    let tag = reader.read(1);
    if tag.byte_length() == 0 {
        return Marshalled::Undefined;
    }
    match tag.read_u_int8(0) {
        data_type::UNDEFINED => Marshalled::Undefined,
        data_type::STRING => {
            let length = read_int_vql(reader);
            Marshalled::String(String::from_utf8_lossy(&reader.read(length.max(0) as usize).to_vec()).into_owned().into())
        }
        data_type::BUFFER => {
            let length = read_int_vql(reader);
            Marshalled::Bytes(reader.read(length.max(0) as usize).to_vec())
        }
        data_type::VS_BUFFER => {
            let length = read_int_vql(reader);
            Marshalled::Buffer(reader.read(length.max(0) as usize))
        }
        data_type::ARRAY => {
            let length = read_int_vql(reader);
            Marshalled::Array((0..length.max(0)).map(|_| deserialize(reader)).collect())
        }
        data_type::OBJECT => {
            let length = read_int_vql(reader);
            let text = String::from_utf8_lossy(&reader.read(length.max(0) as usize).to_vec()).into_owned();
            let mut parse_errors = Vec::new();
            match json::parse_str(&text, &mut parse_errors).filter(|_| parse_errors.is_empty()) {
                Some(value) => from_json(&value),
                None => errors::throw(Error::named("SyntaxError", format!("Unexpected token in JSON: {text}"))),
            }
        }
        data_type::INT => Marshalled::Number(f64::from(read_int_vql(reader))),
        _ => Marshalled::Undefined,
    }
}
