//! Porte de `src/vs/editor/common/services/semanticTokensDto.ts`: o DTO dos tokens semânticos e a sua
//! codificação binária (`encodeSemanticTokensDto` e `decodeSemanticTokensDto`).
//!
//! Layout dos bytes (palavras de 32 bits em little-endian): `id`, o tipo (`1` completo, `2` delta) e depois:
//! no completo, o tamanho de `data` e os tokens; no delta, a quantidade de deltas e, para cada um, `start`,
//! `deleteCount`, o tamanho de `data` (zero quando não há) e os tokens.
//!
//! Diferenças de forma, sem mudança de resultado:
//!
//! - o original decide por `platform.isLittleEndian()` entre reinterpretar a memória do `Uint32Array` e
//!   inverter os bytes; aqui se escolhe sempre o caminho portável, `u32::to_le_bytes` e
//!   `u32::from_le_bytes`, que dá os mesmos bytes em qualquer máquina e nunca altera o buffer de entrada
//!   (o original inverte o buffer no lugar em máquinas big-endian);
//! - o `Uint32Array` é um `Vec<u32>`, e o `subarray` do decode (que compartilha a memória) vira uma cópia;
//! - a checagem de alinhamento (`byteOffset % 4`) do original não existe: ler por `from_le_bytes` não
//!   depende de alinhamento;
//! - ler além do fim dos bytes dá `undefined` no original (tamanhos e índices viram `NaN`); aqui lança um
//!   `RangeError`, como `VSBuffer` no `je-base`.

use je_base::buffer::VSBuffer;
use je_base::errors::{self, Error};

/// `IFullSemanticTokensDto` (`type: 'full'`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FullSemanticTokensDto {
    pub id: u32,
    pub data: Vec<u32>,
}

/// O elemento de `deltas` de `IDeltaSemanticTokensDto`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticTokensDelta {
    pub start: u32,
    pub delete_count: u32,
    pub data: Option<Vec<u32>>,
}

/// `IDeltaSemanticTokensDto` (`type: 'delta'`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeltaSemanticTokensDto {
    pub id: u32,
    pub deltas: Vec<SemanticTokensDelta>,
}

/// `ISemanticTokensDto`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SemanticTokensDto {
    Full(FullSemanticTokensDto),
    Delta(DeltaSemanticTokensDto),
}

/// `EncodedSemanticTokensType`.
const ENCODED_FULL: u32 = 1;
const ENCODED_DELTA: u32 = 2;

fn encode_semantic_tokens_dto_size(semantic_tokens: &SemanticTokensDto) -> usize {
    // id + type
    let mut result = 2;
    match semantic_tokens {
        // data length + data
        SemanticTokensDto::Full(full) => result += 1 + full.data.len(),
        SemanticTokensDto::Delta(delta) => {
            // delta count
            result += 1;
            // start + deleteCount + data length
            result += 3 * delta.deltas.len();
            for d in &delta.deltas {
                if let Some(data) = &d.data {
                    result += data.len();
                }
            }
        }
    }
    result
}

/// `encodeSemanticTokensDto`.
pub fn encode_semantic_tokens_dto(semantic_tokens: &SemanticTokensDto) -> VSBuffer {
    let mut dest: Vec<u32> = Vec::with_capacity(encode_semantic_tokens_dto_size(semantic_tokens));
    match semantic_tokens {
        SemanticTokensDto::Full(full) => {
            dest.push(full.id);
            dest.push(ENCODED_FULL);
            dest.push(full.data.len() as u32);
            dest.extend_from_slice(&full.data);
        }
        SemanticTokensDto::Delta(delta) => {
            dest.push(delta.id);
            dest.push(ENCODED_DELTA);
            dest.push(delta.deltas.len() as u32);
            for d in &delta.deltas {
                dest.push(d.start);
                dest.push(d.delete_count);
                match &d.data {
                    Some(data) => {
                        dest.push(data.len() as u32);
                        dest.extend_from_slice(data);
                    }
                    None => dest.push(0),
                }
            }
        }
    }
    // `toLittleEndianBuffer`
    VSBuffer::wrap(dest.iter().flat_map(|word| word.to_le_bytes()).collect())
}

/// Leitor sequencial das palavras little-endian (`fromLittleEndianBuffer` mais `src[offset++]`).
struct Words {
    bytes: Vec<u8>,
    offset: usize,
}

impl Words {
    fn next(&mut self) -> u32 {
        let at = self.offset * 4;
        match self.bytes.get(at..at + 4) {
            Some(word) => {
                self.offset += 1;
                u32::from_le_bytes([word[0], word[1], word[2], word[3]])
            }
            None => errors::throw(Error::named("RangeError", format!("word {} is out of bounds", self.offset))),
        }
    }

    /// `src.subarray(offset, offset + length)`, com o fim limitado ao tamanho, e avança `length`.
    fn take(&mut self, length: usize) -> Vec<u32> {
        let total = self.bytes.len() / 4;
        let from = self.offset.min(total);
        let to = (self.offset + length).min(total);
        self.offset += length;
        self.bytes[from * 4..to * 4].chunks_exact(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect()
    }
}

/// `decodeSemanticTokensDto`.
pub fn decode_semantic_tokens_dto(buff: &VSBuffer) -> SemanticTokensDto {
    let mut src = Words { bytes: buff.to_vec(), offset: 0 };
    let id = src.next();
    let ty = src.next();
    if ty == ENCODED_FULL {
        let length = src.next() as usize;
        let data = src.take(length);
        return SemanticTokensDto::Full(FullSemanticTokensDto { id, data });
    }
    let delta_count = src.next();
    let mut deltas = Vec::new();
    for _ in 0..delta_count {
        let start = src.next();
        let delete_count = src.next();
        let length = src.next() as usize;
        let data = if length > 0 { Some(src.take(length)) } else { None };
        deltas.push(SemanticTokensDelta { start, delete_count, data });
    }
    SemanticTokensDto::Delta(DeltaSemanticTokensDto { id, deltas })
}
