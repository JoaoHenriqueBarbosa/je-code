//! Descompressão FSE: porte de `lib/common/entropy_common.c` (`FSE_readNCount`) e de
//! `lib/decompress` + `lib/common/fse_decompress.c` (`FSE_buildDTable`, `FSE_decompress_usingDTable`).

use super::super::params::highbit32;
use super::bits::{FwdBits, RevBits};
use super::Error;

pub const FSE_MIN_TABLELOG: u32 = 5;
pub const FSE_TABLELOG_ABSOLUTE_MAX: u32 = 15;

#[derive(Clone, Copy, Default)]
pub struct DEntry {
    pub new_state: u16,
    pub symbol: u8,
    pub nb_bits: u8,
}

pub struct DTable {
    pub table_log: u32,
    pub entries: Vec<DEntry>,
}

pub struct NCount {
    pub normalized: Vec<i16>,
    pub max_symbol: u32,
    pub table_log: u32,
    /// Bytes do cabeçalho consumidos.
    pub consumed: usize,
}

/// `FSE_readNCount`: lê as contagens normalizadas (`max_symbol_value` é o teto do chamador).
pub fn read_ncount(src: &[u8], max_symbol_value: u32) -> Result<NCount, Error> {
    if src.is_empty() {
        return Err(Error::SrcSizeWrong);
    }
    let mut bits = FwdBits::new(src);
    let table_log = (bits.peek(4) & 0xF) + FSE_MIN_TABLELOG;
    bits.skip(4);
    if table_log > FSE_TABLELOG_ABSOLUTE_MAX {
        return Err(Error::TableLogTooLarge);
    }
    let mut nb_bits = table_log + 1;
    let mut remaining: i32 = (1 << table_log) + 1;
    let mut threshold: i32 = 1 << table_log;
    let mut normalized: Vec<i16> = Vec::new();
    let mut previous0 = false;
    while remaining > 1 && normalized.len() as u32 <= max_symbol_value {
        if previous0 {
            // Sequência de zeros: grupos de 2 bits, 3 significa "mais um grupo".
            loop {
                let rep = bits.peek(2);
                bits.skip(2);
                for _ in 0..rep {
                    normalized.push(0);
                }
                if rep != 3 {
                    break;
                }
            }
            if normalized.len() as u32 > max_symbol_value + 1 {
                return Err(Error::MaxSymbolValueTooSmall);
            }
            if normalized.len() as u32 > max_symbol_value {
                break;
            }
        }
        let max = (2 * threshold - 1) - remaining;
        let v = bits.peek(nb_bits) as i32;
        let mut count;
        if (v & (threshold - 1)) < max {
            count = v & (threshold - 1);
            bits.skip(nb_bits - 1);
        } else {
            count = v & (2 * threshold - 1);
            if count >= threshold {
                count -= max;
            }
            bits.skip(nb_bits);
        }
        count -= 1;
        remaining -= count.abs();
        normalized.push(count as i16);
        previous0 = count == 0;
        while remaining < threshold {
            nb_bits -= 1;
            threshold >>= 1;
        }
    }
    if remaining != 1 {
        return Err(Error::Corruption);
    }
    let consumed = bits.bytes_used();
    if consumed > src.len() {
        return Err(Error::SrcSizeWrong);
    }
    let max_symbol = normalized.len() as u32 - 1;
    Ok(NCount { normalized, max_symbol, table_log, consumed })
}

/// `FSE_buildDTable` (espalhamento lento, que dá as mesmas posições da versão rápida).
pub fn build_dtable(normalized: &[i16], table_log: u32) -> Result<DTable, Error> {
    let table_size = 1usize << table_log;
    let mut entries = vec![DEntry::default(); table_size];
    let mut symbol_next = vec![0u16; normalized.len()];
    let mut high_threshold = table_size - 1;
    for (s, &n) in normalized.iter().enumerate() {
        if n == -1 {
            entries[high_threshold].symbol = s as u8;
            high_threshold = high_threshold.wrapping_sub(1);
            symbol_next[s] = 1;
        } else {
            symbol_next[s] = n as u16;
        }
    }
    let step = (table_size >> 1) + (table_size >> 3) + 3;
    let mask = table_size - 1;
    let mut position = 0usize;
    for (s, &n) in normalized.iter().enumerate() {
        for _ in 0..n.max(0) {
            entries[position].symbol = s as u8;
            position = (position + step) & mask;
            while position > high_threshold {
                position = (position + step) & mask;
            }
        }
    }
    if position != 0 {
        return Err(Error::Corruption);
    }
    for entry in entries.iter_mut() {
        let next_state = symbol_next[entry.symbol as usize] as u32;
        symbol_next[entry.symbol as usize] += 1;
        let nb_bits = table_log - highbit32(next_state);
        entry.nb_bits = nb_bits as u8;
        entry.new_state = ((next_state << nb_bits) - table_size as u32) as u16;
    }
    Ok(DTable { table_log, entries })
}

/// Um estado FSE sobre uma `DTable`.
pub struct DState {
    pub state: usize,
}

impl DState {
    pub fn init(bits: &mut RevBits, dt: &DTable) -> Self {
        DState { state: bits.read(dt.table_log) as usize }
    }

    /// `FSE_decodeSymbol`.
    pub fn decode(&mut self, bits: &mut RevBits, dt: &DTable) -> u8 {
        let e = dt.entries[self.state];
        self.state = e.new_state as usize + bits.read(e.nb_bits as u32) as usize;
        e.symbol
    }
}

/// `FSE_decompress` com teto de `max_log` para o `tableLog` e `dst_capacity` símbolos no máximo
/// (usada nos pesos Huffman). Devolve os símbolos e quantos bytes da fonte foram lidos (a fonte toda).
pub fn decompress(src: &[u8], max_symbol_value: u32, max_log: u32, dst_capacity: usize) -> Result<Vec<u8>, Error> {
    let nc = read_ncount(src, max_symbol_value)?;
    if nc.table_log > max_log {
        return Err(Error::TableLogTooLarge);
    }
    let dt = build_dtable(&nc.normalized, nc.table_log)?;
    let mut bits = RevBits::new(&src[nc.consumed..])?;
    let mut s1 = DState::init(&mut bits, &dt);
    let mut s2 = DState::init(&mut bits, &dt);
    let mut out = Vec::new();
    loop {
        if out.len() + 2 > dst_capacity {
            return Err(Error::DstSizeTooSmall);
        }
        out.push(s1.decode(&mut bits, &dt));
        if bits.overflowed() {
            out.push(dt.entries[s2.state].symbol);
            break;
        }
        out.push(s2.decode(&mut bits, &dt));
        if bits.overflowed() {
            out.push(dt.entries[s1.state].symbol);
            break;
        }
    }
    Ok(out)
}
