//! Seção de literais de um bloco: `zstd_compress_literals.c` (`ZSTD_compressLiterals` e os casos raw e RLE).
//!
//! O compressor Huffman (`HUF_compress1X_repeat` e `HUF_compress4X_repeat`) entra como parâmetro de
//! `compress_literals`; esta fatia fecha a decisão raw, RLE ou comprimido e o cabeçalho da seção.

use super::params::Strategy;

/// `SymbolEncodingType_e` para literais.
pub const SET_BASIC: u32 = 0;
pub const SET_RLE: u32 = 1;
pub const SET_COMPRESSED: u32 = 2;
pub const SET_REPEAT: u32 = 3;

/// `MIN_LITERALS_FOR_4_STREAMS`.
pub const MIN_LITERALS_FOR_4_STREAMS: usize = 6;
/// `LitHufLog`: profundidade máxima da árvore de literais.
pub const LIT_HUF_LOG: u32 = 11;
/// `HUF_SYMBOLVALUE_MAX`.
pub const HUF_SYMBOLVALUE_MAX: u32 = 255;

/// Número da estratégia como no libzstd (`ZSTD_fast == 1` ... `ZSTD_btultra2 == 9`).
pub fn strategy_number(strategy: Strategy) -> u32 {
    strategy as u32 + 1
}

/// Bandeiras que `ZSTD_compressLiterals` entrega ao compressor Huffman (sem `bmi2`, que não muda o resultado).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HufFlags {
    pub prefer_repeat: bool,
    pub optimal_depth: bool,
    pub suspect_uncompressible: bool,
}

/// Pedido ao compressor Huffman.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HufRequest {
    pub four_streams: bool,
    pub flags: HufFlags,
}

/// Resposta do compressor Huffman: o conteúdo (tabela mais fluxos) e se reaproveitou a tabela anterior
/// (`repeat != HUF_repeat_none` na volta). `None` equivale a `cLitSize == 0` ou erro.
pub struct HufOutput {
    pub bytes: Vec<u8>,
    pub reused_table: bool,
}

/// O que o próximo bloco deve fazer com a tabela Huffman (`nextHuf`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TableState {
    /// `nextHuf` continua igual a `prevHuf` (raw, RLE ou reaproveitamento).
    Unchanged,
    /// Tabela nova: `nextHuf->repeatMode = HUF_repeat_check`.
    NewlyBuilt,
}

pub struct LiteralsSection {
    pub bytes: Vec<u8>,
    pub table: TableState,
}

/// `ZSTD_minGain`.
pub fn min_gain(src_size: usize, strategy: Strategy) -> usize {
    let n = strategy_number(strategy);
    let min_log = if n >= 8 { n - 1 } else { 6 };
    (src_size >> min_log) + 2
}

/// `ZSTD_minLiteralsToCompress`.
fn min_literals_to_compress(strategy: Strategy, repeat_valid: bool) -> usize {
    if repeat_valid {
        6
    } else {
        8usize << (9 - strategy_number(strategy)).min(3)
    }
}

fn header_size(src_size: usize) -> usize {
    1 + usize::from(src_size > 31) + usize::from(src_size > 4095)
}

/// Cabeçalho dos tipos raw e RLE (`ZSTD_noCompressLiterals` e `ZSTD_compressRleLiteralsBlock`).
fn push_simple_header(out: &mut Vec<u8>, kind: u32, src_size: usize) {
    let n = src_size as u32;
    match header_size(src_size) {
        1 => out.push((kind + (n << 3)) as u8),
        2 => out.extend_from_slice(&((kind + (1 << 2) + (n << 4)) as u16).to_le_bytes()),
        _ => out.extend_from_slice(&(kind + (3 << 2) + (n << 4)).to_le_bytes()[..3]),
    }
}

/// `ZSTD_noCompressLiterals`.
pub fn no_compress_literals(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len() + 4);
    push_simple_header(&mut out, SET_BASIC, src.len());
    out.extend_from_slice(src);
    out
}

fn all_bytes_identical(src: &[u8]) -> bool {
    src.iter().all(|&b| b == src[0])
}

/// `ZSTD_compressRleLiteralsBlock`: `src` não vazio e com bytes iguais.
fn rle_literals(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5);
    push_simple_header(&mut out, SET_RLE, src.len());
    out.push(src[0]);
    out
}

/// `ZSTD_compressLiterals` com `disableLiteralCompression == 0`.
///
/// `repeat_valid` é `prevHuf->repeatMode == HUF_repeat_valid`; `encode` executa o `HUF_compress*_repeat`
/// pedido sobre `src` e devolve `None` quando ele daria 0 ou erro.
pub fn compress_literals(
    src: &[u8],
    strategy: Strategy,
    repeat_valid: bool,
    suspect_uncompressible: bool,
    encode: impl FnOnce(HufRequest) -> Option<HufOutput>,
) -> LiteralsSection {
    let raw = || LiteralsSection { bytes: no_compress_literals(src), table: TableState::Unchanged };
    let src_size = src.len();
    if src_size < min_literals_to_compress(strategy, repeat_valid) {
        return raw();
    }
    let lh_size = 3 + usize::from(src_size >= 1024) + usize::from(src_size >= 16 * 1024);
    let mut single_stream = src_size < 256;
    if repeat_valid && lh_size == 3 {
        single_stream = true;
    }
    let n = strategy_number(strategy);
    let flags = HufFlags {
        prefer_repeat: n < 4 && src_size <= 1024,
        optimal_depth: n >= 8,
        suspect_uncompressible,
    };
    let Some(out) = encode(HufRequest { four_streams: !single_stream, flags }) else {
        return raw();
    };
    let c_lit_size = out.bytes.len();
    if c_lit_size == 0 || c_lit_size >= src_size - min_gain(src_size, strategy) {
        return raw();
    }
    if c_lit_size == 1 && (src_size >= 8 || all_bytes_identical(src)) {
        return LiteralsSection { bytes: rle_literals(src), table: TableState::Unchanged };
    }
    let h_type = if out.reused_table { SET_REPEAT } else { SET_COMPRESSED };
    let (s, c) = (src_size as u32, c_lit_size as u32);
    let mut bytes = Vec::with_capacity(lh_size + c_lit_size);
    match lh_size {
        3 => {
            let lhc = h_type + (u32::from(!single_stream) << 2) + (s << 4) + (c << 14);
            bytes.extend_from_slice(&lhc.to_le_bytes()[..3]);
        }
        4 => bytes.extend_from_slice(&(h_type + (2 << 2) + (s << 4) + (c << 18)).to_le_bytes()),
        _ => {
            bytes.extend_from_slice(&(h_type + (3 << 2) + (s << 4) + (c << 22)).to_le_bytes());
            bytes.push((c >> 10) as u8);
        }
    }
    bytes.extend_from_slice(&out.bytes);
    LiteralsSection { bytes, table: if out.reused_table { TableState::Unchanged } else { TableState::NewlyBuilt } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_header_sizes() {
        assert_eq!(no_compress_literals(&[7; 5])[0], (5 << 3) as u8);
        assert_eq!(no_compress_literals(&[7; 40]).len(), 42);
        assert_eq!(no_compress_literals(&vec![7; 5000]).len(), 5003);
    }

    #[test]
    fn small_input_is_raw_for_dfast() {
        // dfast é a estratégia 2: mínimo 8 << min(9 - 2, 3) = 8 << 3 = 64 literais. Abaixo disso sai raw, com o
        // cabeçalho de 2 bytes que `ZSTD_noCompressLiterals` usa acima de 31 bytes.
        let s = compress_literals(&[1; 63], Strategy::DFast, false, false, |_| None);
        assert_eq!(s.bytes.len(), 65);
    }

    #[test]
    fn single_symbol_becomes_rle() {
        let s = compress_literals(&[9; 100], Strategy::DFast, false, false, |_| Some(HufOutput { bytes: vec![0], reused_table: false }));
        let header = ((SET_RLE + (1 << 2) + (100 << 4)) as u16).to_le_bytes();
        assert_eq!(s.bytes, vec![header[0], header[1], 9]);
    }
}
