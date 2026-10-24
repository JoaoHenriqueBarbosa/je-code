//! Descompressor Huffman dos literais: `HUF_readStats` e `HUF_decompress1X1`/`4X1` de
//! `lib/decompress/huf_decompress.c` (libzstd 1.5.7).
//!
//! O bun escolhe entre a tabela de um símbolo (X1) e a de dois (X2) por heurística de velocidade; os
//! bytes de saída são idênticos, então esta porta usa só a X1 (um símbolo por consulta).

use super::super::params::highbit32;
use super::bits::RevBits;
use super::fse;
use super::Error;

pub const HUF_TABLELOG_MAX: u32 = 12;
const HUF_SYMBOLVALUE_MAX: usize = 255;
const MAX_FSE_TABLELOG_FOR_HUFF_HEADER: u32 = 6;

/// Tabela de decodificação X1: índice = próximos `table_log` bits do fluxo.
#[derive(Clone)]
pub struct HufTable {
    pub table_log: u32,
    symbols: Vec<u8>,
    nb_bits: Vec<u8>,
}

/// `HUF_readStats`: pesos (já com o último deduzido), `tableLog`, e bytes do cabeçalho.
fn read_stats(src: &[u8]) -> Result<(Vec<u8>, u32, usize), Error> {
    let first = *src.first().ok_or(Error::SrcSizeWrong)? as usize;
    let (mut weights, header_size) = if first >= 128 {
        let count = first - 127;
        let packed = count.div_ceil(2);
        if packed + 1 > src.len() {
            return Err(Error::SrcSizeWrong);
        }
        let mut w = Vec::with_capacity(count + 1);
        for n in (0..count).step_by(2) {
            let byte = src[1 + n / 2];
            w.push(byte >> 4);
            if n + 1 < count {
                w.push(byte & 15);
            }
        }
        (w, packed + 1)
    } else {
        if first + 1 > src.len() {
            return Err(Error::SrcSizeWrong);
        }
        let w = fse::decompress(&src[1..1 + first], HUF_SYMBOLVALUE_MAX as u32, MAX_FSE_TABLELOG_FOR_HUFF_HEADER, HUF_SYMBOLVALUE_MAX)?;
        (w, first + 1)
    };
    let count = weights.len();
    if count >= HUF_SYMBOLVALUE_MAX {
        return Err(Error::Corruption);
    }
    let mut rank_stats = [0u32; HUF_TABLELOG_MAX as usize + 1];
    let mut weight_total: u32 = 0;
    for &w in &weights {
        if w as u32 > HUF_TABLELOG_MAX {
            return Err(Error::Corruption);
        }
        rank_stats[w as usize] += 1;
        weight_total += (1u32 << w) >> 1;
    }
    if weight_total == 0 {
        return Err(Error::Corruption);
    }
    let table_log = highbit32(weight_total) + 1;
    if table_log > HUF_TABLELOG_MAX {
        return Err(Error::Corruption);
    }
    let rest = (1u32 << table_log) - weight_total;
    let verif = 1u32 << highbit32(rest);
    let last_weight = highbit32(rest) + 1;
    if verif != rest {
        return Err(Error::Corruption);
    }
    weights.push(last_weight as u8);
    rank_stats[last_weight as usize] += 1;
    if rank_stats[1] < 2 || (rank_stats[1] & 1) != 0 {
        return Err(Error::Corruption);
    }
    Ok((weights, table_log, header_size))
}

/// `HUF_readDTableX1`: lê o cabeçalho e monta a tabela. Devolve também os bytes do cabeçalho.
pub fn read_table(src: &[u8]) -> Result<(HufTable, usize), Error> {
    let (weights, table_log, header_size) = read_stats(src)?;
    let mut rank_start = [0usize; HUF_TABLELOG_MAX as usize + 2];
    let mut rank_count = [0usize; HUF_TABLELOG_MAX as usize + 1];
    for &w in &weights {
        rank_count[w as usize] += 1;
    }
    let mut next = 0usize;
    for w in 1..=table_log as usize {
        rank_start[w] = next;
        next += rank_count[w] << (w - 1);
    }
    let size = 1usize << table_log;
    let mut symbols = vec![0u8; size];
    let mut nb_bits = vec![0u8; size];
    for (s, &w) in weights.iter().enumerate() {
        if w == 0 {
            continue;
        }
        let length = (1usize << w) >> 1;
        let start = rank_start[w as usize];
        rank_start[w as usize] += length;
        for i in start..start + length {
            symbols[i] = s as u8;
            nb_bits[i] = (table_log + 1 - w as u32) as u8;
        }
    }
    Ok((HufTable { table_log, symbols, nb_bits }, header_size))
}

impl HufTable {
    /// `HUF_decompress1X1_usingDTable`: exatamente `count` símbolos e o fluxo inteiro consumido.
    pub fn decode_1x(&self, src: &[u8], count: usize, out: &mut Vec<u8>) -> Result<(), Error> {
        let mut bits = RevBits::new(src)?;
        for _ in 0..count {
            let idx = bits.peek(self.table_log) as usize;
            out.push(self.symbols[idx]);
            bits.skip(self.nb_bits[idx] as u32);
        }
        if !bits.at_end() {
            return Err(Error::Corruption);
        }
        Ok(())
    }

    /// `HUF_decompress4X1_usingDTable`: tabela de saltos de 6 bytes e quatro fluxos.
    pub fn decode_4x(&self, src: &[u8], count: usize, out: &mut Vec<u8>) -> Result<(), Error> {
        if src.len() < 10 || count < 6 {
            return Err(Error::Corruption);
        }
        let l1 = u16::from_le_bytes([src[0], src[1]]) as usize;
        let l2 = u16::from_le_bytes([src[2], src[3]]) as usize;
        let l3 = u16::from_le_bytes([src[4], src[5]]) as usize;
        let body = &src[6..];
        let used = l1 + l2 + l3;
        if used > body.len() {
            return Err(Error::Corruption);
        }
        let l4 = body.len() - used;
        let segment = count.div_ceil(4);
        let last = count - 3 * segment;
        let mut at = 0;
        for (len, n) in [(l1, segment), (l2, segment), (l3, segment), (l4, last)] {
            self.decode_1x(&body[at..at + len], n, out)?;
            at += len;
        }
        Ok(())
    }
}
