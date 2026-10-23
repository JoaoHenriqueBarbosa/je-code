//! Porte de `src/vs/base/parts/ipc/common/webSocketFraming.ts`: o codec de quadros RFC 6455 (codificação,
//! análise incremental e máscara).
//!
//! Os erros do original (`Error`, `WebSocketFrameTooLargeError`) são [`Error`] devolvidos em `Result`; o
//! `WebSocketFrameTooLargeError` tem o nome `WebSocketFrameTooLargeError` e as propriedades `payloadLength` e
//! `maxPayloadLength`.

use crate::buffer::VSBuffer;
use crate::errors::Error;
use crate::parts::ipc::net::ChunkStream;

const FIRST_BYTE_FINAL_MASK: u8 = 0b1000_0000;
const FIRST_BYTE_COMPRESSED_MASK: u8 = 0b0100_0000;
const FIRST_BYTE_RESERVED_MASK: u8 = 0b0011_0000;
const SECOND_BYTE_MASKED_MASK: u8 = 0b1000_0000;
const PAYLOAD_LENGTH_MASK: u8 = 0b0111_1111;
const EXTENDED_PAYLOAD_LENGTH_16: u8 = 126;
const EXTENDED_PAYLOAD_LENGTH_64: u8 = 127;
const MAXIMUM_32_BIT_PAYLOAD_LENGTH: u64 = 0xffff_ffff;

/// RFC 6455 frame opcode values supported by the framing codec.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebSocketOpcode {
    Continuation = 0x0,
    Text = 0x1,
    Binary = 0x2,
    Close = 0x8,
    Ping = 0x9,
    Pong = 0xA,
}

impl WebSocketOpcode {
    fn from_u8(opcode: u8) -> Result<Self, Error> {
        Ok(match opcode {
            0x0 => Self::Continuation,
            0x1 => Self::Text,
            0x2 => Self::Binary,
            0x8 => Self::Close,
            0x9 => Self::Ping,
            0xA => Self::Pong,
            other => return Err(Error::new(format!("WebSocket frame has reserved opcode {other}."))),
        })
    }
}

/// A parsed RFC 6455 frame with an unmasked payload.
#[derive(Clone, Debug)]
pub struct IWebSocketFrame {
    pub r#final: bool,
    pub compressed: bool,
    pub opcode: WebSocketOpcode,
    pub payload: VSBuffer,
    /// The mask from the wire, including a zero-valued mask, when present.
    pub mask: Option<u32>,
}

/// Options for encoding an RFC 6455 frame.
#[derive(Clone, Copy, Debug)]
pub struct IWebSocketFrameOptions {
    pub r#final: Option<bool>,
    pub compressed: Option<bool>,
    pub opcode: WebSocketOpcode,
    /// Four mask bytes. When supplied, sets MASK and applies it to a payload copy.
    pub mask: Option<u32>,
}

impl IWebSocketFrameOptions {
    pub fn new(opcode: WebSocketOpcode) -> Self {
        Self { r#final: None, compressed: None, opcode, mask: None }
    }
}

/// Options for parsing RFC 6455 frames.
#[derive(Clone, Copy, Debug, Default)]
pub struct IWebSocketFrameParserOptions {
    /// Maximum accepted frame payload length.
    pub max_payload_length: Option<u64>,
    /// Unmask owned payload buffers in place instead of allocating a copy.
    pub unmask_in_place: Option<bool>,
}

/// `WebSocketFrameTooLargeError`: um quadro passou do limite de tamanho configurado.
pub fn web_socket_frame_too_large_error(payload_length: u64, max_payload_length: u64) -> Error {
    Error::named(
        "WebSocketFrameTooLargeError",
        format!("WebSocket frame payload length {payload_length} exceeds the configured limit of {max_payload_length}."),
    )
    .with_prop("payloadLength", payload_length)
    .with_prop("maxPayloadLength", max_payload_length)
}

/// Incrementally parses RFC 6455 frames from arbitrarily chunked input.
pub struct WebSocketFrameParser {
    incoming_data: ChunkStream,
    max_payload_length: u64,
    unmask_in_place: bool,
}

impl WebSocketFrameParser {
    /// Lança (devolve `Err`) se o limite não é um inteiro sem sinal de 32 bits.
    pub fn new(options: IWebSocketFrameParserOptions) -> Result<Self, Error> {
        let max_payload_length = options.max_payload_length.unwrap_or(MAXIMUM_32_BIT_PAYLOAD_LENGTH);
        if max_payload_length > MAXIMUM_32_BIT_PAYLOAD_LENGTH {
            return Err(Error::new("WebSocket frame payload limits must be unsigned 32-bit integers."));
        }
        Ok(Self { incoming_data: ChunkStream::new(), max_payload_length, unmask_in_place: options.unmask_in_place.unwrap_or(false) })
    }

    /// Accepts a network chunk and returns every complete frame it contains.
    pub fn accept_chunk(&mut self, data: VSBuffer) -> Result<Vec<IWebSocketFrame>, Error> {
        if data.byte_length() == 0 {
            return Ok(Vec::new());
        }

        self.incoming_data.accept_chunk(data);
        let mut frames = Vec::new();
        while self.incoming_data.byte_length() >= 2 {
            let initial_header = self.incoming_data.peek(2);
            let first_byte = initial_header.read_u_int8(0);
            let second_byte = initial_header.read_u_int8(1);
            let payload_length_marker = second_byte & PAYLOAD_LENGTH_MASK;
            let extended_payload_length_size = match payload_length_marker {
                EXTENDED_PAYLOAD_LENGTH_16 => 2,
                EXTENDED_PAYLOAD_LENGTH_64 => 8,
                _ => 0,
            };
            let masked = second_byte & SECOND_BYTE_MASKED_MASK != 0;
            let header_length = 2 + extended_payload_length_size + if masked { 4 } else { 0 };
            if self.incoming_data.byte_length() < header_length {
                break;
            }

            let header = self.incoming_data.peek(header_length);
            let payload_length = get_payload_length(&header, payload_length_marker)?;
            if payload_length > self.max_payload_length {
                return Err(web_socket_frame_too_large_error(payload_length, self.max_payload_length));
            }
            validate_frame(first_byte, payload_length)?;
            let opcode = WebSocketOpcode::from_u8(first_byte & 0b0000_1111)?;
            if (self.incoming_data.byte_length() as u64) < header_length as u64 + payload_length {
                break;
            }

            self.incoming_data.read(header_length);
            let payload = self.incoming_data.read(payload_length as usize);
            let mask = masked.then(|| header.read_u_int32_be(header_length - 4));
            let mut unmasked_payload = payload.clone();
            if let Some(mask) = mask {
                if self.unmask_in_place {
                    apply_web_socket_mask(&unmasked_payload, mask)?;
                } else {
                    unmasked_payload = copy_and_apply_web_socket_mask(&payload, mask)?;
                }
            }
            frames.push(IWebSocketFrame {
                r#final: first_byte & FIRST_BYTE_FINAL_MASK != 0,
                compressed: first_byte & FIRST_BYTE_COMPRESSED_MASK != 0,
                opcode,
                payload: unmasked_payload,
                mask,
            });
        }
        Ok(frames)
    }
}

/// Encodes one RFC 6455 frame without mutating the supplied payload.
pub fn encode_web_socket_frame(payload: &VSBuffer, options: IWebSocketFrameOptions) -> Result<VSBuffer, Error> {
    let final_ = options.r#final.unwrap_or(true);
    let compressed = options.compressed.unwrap_or(false);
    let opcode = options.opcode as u8;
    let first_byte = (if final_ { FIRST_BYTE_FINAL_MASK } else { 0 }) | (if compressed { FIRST_BYTE_COMPRESSED_MASK } else { 0 }) | opcode;
    validate_frame(first_byte, payload.byte_length() as u64)?;

    let header_length = get_header_length(payload.byte_length() as u64, options.mask.is_some())?;
    let header = VSBuffer::alloc(header_length);
    header.write_u_int8(first_byte, 0);

    let mask_bit = if options.mask.is_none() { 0 } else { SECOND_BYTE_MASKED_MASK };
    let length = payload.byte_length();
    let mut offset = 2;
    if length < usize::from(EXTENDED_PAYLOAD_LENGTH_16) {
        header.write_u_int8(mask_bit | length as u8, 1);
    } else if length < 1 << 16 {
        header.write_u_int8(mask_bit | EXTENDED_PAYLOAD_LENGTH_16, 1);
        header.write_u_int8((length >> 8) as u8, offset);
        offset += 1;
        header.write_u_int8(length as u8, offset);
        offset += 1;
    } else {
        header.write_u_int8(mask_bit | EXTENDED_PAYLOAD_LENGTH_64, 1);
        header.write_u_int32_be(0, offset);
        offset += 4;
        header.write_u_int32_be(length as u32, offset);
        offset += 4;
    }

    match options.mask {
        None => Ok(VSBuffer::concat(&[header, payload.clone()], None)),
        Some(mask) => {
            header.write_u_int32_be(mask, offset);
            Ok(VSBuffer::concat(&[header, copy_and_apply_web_socket_mask(payload, mask)?], None))
        }
    }
}

/// Applies an RFC 6455 four-byte mask to a buffer in place.
pub fn apply_web_socket_mask(payload: &VSBuffer, mask: u32) -> Result<(), Error> {
    if mask == 0 {
        return Ok(());
    }

    let word_count = payload.byte_length() >> 2;
    for index in 0..word_count {
        let offset = index * 4;
        payload.write_u_int32_be(payload.read_u_int32_be(offset) ^ mask, offset);
    }

    let offset = word_count * 4;
    let remaining_byte_count = payload.byte_length() - offset;
    if remaining_byte_count >= 1 {
        payload.write_u_int8(payload.read_u_int8(offset) ^ ((mask >> 24) & 0xff) as u8, offset);
    }
    if remaining_byte_count >= 2 {
        payload.write_u_int8(payload.read_u_int8(offset + 1) ^ ((mask >> 16) & 0xff) as u8, offset + 1);
    }
    if remaining_byte_count >= 3 {
        payload.write_u_int8(payload.read_u_int8(offset + 2) ^ ((mask >> 8) & 0xff) as u8, offset + 2);
    }
    Ok(())
}

fn get_header_length(payload_length: u64, masked: bool) -> Result<usize, Error> {
    if payload_length > MAXIMUM_32_BIT_PAYLOAD_LENGTH {
        return Err(Error::new("WebSocket payload lengths greater than 2^32 - 1 are not supported."));
    }
    let mask = if masked { 4 } else { 0 };
    Ok(if payload_length < u64::from(EXTENDED_PAYLOAD_LENGTH_16) {
        2 + mask
    } else if payload_length < 1 << 16 {
        4 + mask
    } else {
        10 + mask
    })
}

fn get_payload_length(header: &VSBuffer, marker: u8) -> Result<u64, Error> {
    if marker < EXTENDED_PAYLOAD_LENGTH_16 {
        return Ok(u64::from(marker));
    }
    if marker == EXTENDED_PAYLOAD_LENGTH_16 {
        return Ok(u64::from(header.read_u_int8(2)) * 256 + u64::from(header.read_u_int8(3)));
    }

    let high_bits = header.read_u_int32_be(2);
    if high_bits != 0 {
        return Err(Error::new("WebSocket payload lengths greater than 2^32 - 1 are not supported."));
    }
    Ok(u64::from(header.read_u_int32_be(6)))
}

fn validate_frame(first_byte: u8, payload_length: u64) -> Result<(), Error> {
    if first_byte & FIRST_BYTE_RESERVED_MASK != 0 {
        return Err(Error::new("WebSocket frames must not set RSV2 or RSV3."));
    }

    let opcode = first_byte & 0b0000_1111;
    WebSocketOpcode::from_u8(opcode)?;
    if is_control_opcode(opcode) {
        if first_byte & FIRST_BYTE_FINAL_MASK == 0 {
            return Err(Error::new("WebSocket control frames must be final."));
        }
        if payload_length > 125 {
            return Err(Error::new("WebSocket control frames must not exceed 125 bytes."));
        }
        if first_byte & FIRST_BYTE_COMPRESSED_MASK != 0 {
            return Err(Error::new("WebSocket control frames must not set RSV1."));
        }
    }
    Ok(())
}

fn is_control_opcode(opcode: u8) -> bool {
    opcode & 0b0000_1000 != 0
}

fn copy_and_apply_web_socket_mask(payload: &VSBuffer, mask: u32) -> Result<VSBuffer, Error> {
    let masked_payload = VSBuffer::alloc(payload.byte_length());
    masked_payload.set(payload, 0);
    apply_web_socket_mask(&masked_payload, mask)?;
    Ok(masked_payload)
}
