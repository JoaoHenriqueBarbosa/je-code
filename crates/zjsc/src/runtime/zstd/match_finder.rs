//! Buscador de correspondências dfast: `ZSTD_compressBlock_doubleFast_noDict_generic` (zstd_double_fast.c).
//!
//! Os índices das tabelas são os do libzstd: a posição no buffer mais `WINDOW_START_INDEX` (2), de modo que
//! o índice 0 significa "vazio". O buffer é a entrada inteira; cada bloco é uma faixa `[start, end)` e o
//! prefixo (blocos anteriores) continua visível, como numa compressão de um só passo.

use super::params::CParams;
use super::seq_store::{offset_to_offbase, SeqStore, REPCODE1_OFFBASE};

/// `ZSTD_WINDOW_START_INDEX`.
const WINDOW_START_INDEX: u32 = 2;
/// `HASH_READ_SIZE`.
const HASH_READ_SIZE: usize = 8;
/// `kSearchStrength`: o passo cresce a cada `1 << 8` posições sem correspondência.
const SEARCH_STRENGTH: u32 = 8;

const PRIME_4: u32 = 2_654_435_761;
const PRIME_5: u64 = 889_523_592_379;
const PRIME_6: u64 = 227_718_039_650_203;
const PRIME_7: u64 = 58_295_818_150_454_627;
const PRIME_8: u64 = 0xCF1B_BCDC_B7A5_6463;

fn read32(data: &[u8], pos: usize) -> u32 {
    data.get(pos..pos + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn read64(data: &[u8], pos: usize) -> u64 {
    data.get(pos..pos + 8).map_or(0, |b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

/// `ZSTD_hashPtr`: hash de `mls` bytes em `bits` bits.
fn hash_ptr(data: &[u8], pos: usize, bits: u32, mls: u32) -> usize {
    match mls {
        5 => (((read64(data, pos) << 24).wrapping_mul(PRIME_5)) >> (64 - bits)) as usize,
        6 => (((read64(data, pos) << 16).wrapping_mul(PRIME_6)) >> (64 - bits)) as usize,
        7 => (((read64(data, pos) << 8).wrapping_mul(PRIME_7)) >> (64 - bits)) as usize,
        8 => ((read64(data, pos).wrapping_mul(PRIME_8)) >> (64 - bits)) as usize,
        _ => (read32(data, pos).wrapping_mul(PRIME_4) >> (32 - bits)) as usize,
    }
}

/// `ZSTD_count`: bytes iguais entre `a` e `b` (com `b < a`), sem passar de `limit` em `a`.
fn count(data: &[u8], mut a: usize, mut b: usize, limit: usize) -> usize {
    let start = a;
    while a < limit && data.get(a) == data.get(b) {
        a += 1;
        b += 1;
    }
    a - start
}

/// Como o laço interno termina.
enum Found {
    /// Repetição já armazenada (`_match_stored`).
    Stored,
    /// Correspondência curta: procurar uma longa em `ip + 1` (`_search_next_long`).
    ShortAt { ip: usize, matchs0: usize, idxl1: u32, hl1: usize },
    /// Correspondência pronta (`_match_found`).
    Match { ip: usize, offset: u32, length: usize, hl1: usize },
    /// Acabou o bloco (`_cleanup`).
    Cleanup,
}

pub struct DoubleFast {
    hash_long: Vec<u32>,
    hash_small: Vec<u32>,
    hash_bits_long: u32,
    hash_bits_small: u32,
    min_match: u32,
    window_log: u32,
    /// Os três offsets de repetição (`rep[ZSTD_REP_NUM]`), iniciados em 1, 4 e 8.
    pub rep: [u32; 3],
}

impl DoubleFast {
    pub fn new(c: &CParams) -> Self {
        DoubleFast {
            hash_long: vec![0; 1usize << c.hash_log],
            hash_small: vec![0; 1usize << c.chain_log],
            hash_bits_long: c.hash_log,
            hash_bits_small: c.chain_log,
            min_match: c.min_match,
            window_log: c.window_log,
            rep: [1, 4, 8],
        }
    }

    /// `ZSTD_getLowestPrefixIndex` sem dicionário e com `lowLimit == WINDOW_START_INDEX`.
    fn lowest_index(&self, curr: u32) -> u32 {
        let max_distance = 1u32 << self.window_log;
        if curr - WINDOW_START_INDEX > max_distance {
            curr - max_distance
        } else {
            WINDOW_START_INDEX
        }
    }

    fn hash_l(&self, data: &[u8], pos: usize) -> usize {
        hash_ptr(data, pos, self.hash_bits_long, 8)
    }

    fn hash_s(&self, data: &[u8], pos: usize) -> usize {
        hash_ptr(data, pos, self.hash_bits_small, self.min_match)
    }

    /// Insere a posição `pos` nas duas tabelas (usado na "inserção complementar" e nas repetições imediatas).
    fn insert_both(&mut self, data: &[u8], pos: usize) {
        let idx = pos as u32 + WINDOW_START_INDEX;
        let hl = self.hash_l(data, pos);
        let hs = self.hash_s(data, pos);
        self.hash_long[hl] = idx;
        self.hash_small[hs] = idx;
    }

    /// Comprime o bloco `data[start..end]` em sequências e devolve o tamanho dos literais finais
    /// (`end - anchor`), que o chamador guarda com `ZSTD_storeLastLiterals`.
    pub fn compress_block(&mut self, data: &[u8], start: usize, end: usize, seq: &mut SeqStore) -> usize {
        let ilimit = end as i64 - HASH_READ_SIZE as i64;
        let prefix_lowest_index = self.lowest_index(end as u32 + WINDOW_START_INDEX);
        let prefix_lowest = (prefix_lowest_index - WINDOW_START_INDEX) as usize;
        let mut offset_1 = self.rep[0];
        let mut offset_2 = self.rep[1];
        let (mut saved_1, mut saved_2) = (0u32, 0u32);
        let step_incr = 1usize << SEARCH_STRENGTH;
        let mut anchor = start;
        let mut ip = start;

        ip += usize::from(ip == prefix_lowest);
        {
            let current = ip as u32 + WINDOW_START_INDEX;
            let max_rep = current - self.lowest_index(current);
            if offset_2 > max_rep {
                saved_2 = offset_2;
                offset_2 = 0;
            }
            if offset_1 > max_rep {
                saved_1 = offset_1;
                offset_1 = 0;
            }
        }

        'outer: loop {
            let mut step = 1usize;
            let mut next_step = ip + step_incr;
            let mut ip1 = ip + step;
            if ip1 as i64 > ilimit {
                break 'outer;
            }
            let mut hl0 = self.hash_l(data, ip);
            let mut idxl0 = self.hash_long[hl0];
            let mut curr;

            let found = loop {
                let hs0 = self.hash_s(data, ip);
                let idxs0 = self.hash_small[hs0];
                curr = ip as u32 + WINDOW_START_INDEX;
                self.hash_long[hl0] = curr;
                self.hash_small[hs0] = curr;

                // repetição 1, sem dicionário
                if offset_1 > 0 {
                    let back = ip + 1 - offset_1 as usize;
                    if read32(data, back) == read32(data, ip + 1) {
                        let length = count(data, ip + 1 + 4, back + 4, end) + 4;
                        ip += 1;
                        seq.store(&data[anchor..ip], REPCODE1_OFFBASE, length);
                        ip += length;
                        anchor = ip;
                        break Found::Stored;
                    }
                }

                let hl1 = self.hash_l(data, ip1);

                // correspondência longa no prefixo
                if idxl0 >= prefix_lowest_index {
                    let m = (idxl0 - WINDOW_START_INDEX) as usize;
                    if read64(data, m) == read64(data, ip) {
                        let mut length = count(data, ip + 8, m + 8, end) + 8;
                        let offset = (ip - m) as u32;
                        let mut mm = m;
                        while ip > anchor && mm > prefix_lowest && data[ip - 1] == data[mm - 1] {
                            ip -= 1;
                            mm -= 1;
                            length += 1;
                        }
                        break Found::Match { ip, offset, length, hl1 };
                    }
                }

                let idxl1 = self.hash_long[hl1];

                // correspondência curta no prefixo
                if idxs0 >= prefix_lowest_index {
                    let m = (idxs0 - WINDOW_START_INDEX) as usize;
                    if read32(data, m) == read32(data, ip) {
                        break Found::ShortAt { ip, matchs0: m, idxl1, hl1 };
                    }
                }

                if ip1 >= next_step {
                    step += 1;
                    next_step += step_incr;
                }
                ip = ip1;
                ip1 += step;
                hl0 = hl1;
                idxl0 = idxl1;
                if ip1 as i64 > ilimit {
                    break Found::Cleanup;
                }
            };

            let pending = match found {
                Found::Cleanup => break 'outer,
                Found::Stored => None,
                Found::ShortAt { ip: sip, matchs0, idxl1, hl1 } => {
                    let mut length = count(data, sip + 4, matchs0 + 4, end) + 4;
                    let mut offset = (sip - matchs0) as u32;
                    let mut mp = sip;
                    let mut ms = matchs0;
                    if idxl1 > prefix_lowest_index {
                        let m1 = (idxl1 - WINDOW_START_INDEX) as usize;
                        if read64(data, m1) == read64(data, ip1) {
                            let l1len = count(data, ip1 + 8, m1 + 8, end) + 8;
                            if l1len > length {
                                mp = ip1;
                                length = l1len;
                                offset = (mp - m1) as u32;
                                ms = m1;
                            }
                        }
                    }
                    while mp > anchor && ms > prefix_lowest && data[mp - 1] == data[ms - 1] {
                        mp -= 1;
                        ms -= 1;
                        length += 1;
                    }
                    Some((mp, offset, length, hl1))
                }
                Found::Match { ip: mip, offset, length, hl1 } => Some((mip, offset, length, hl1)),
            };

            if let Some((match_ip, offset, length, hl1)) = pending {
                ip = match_ip;
                offset_2 = offset_1;
                offset_1 = offset;
                if step < 4 {
                    self.hash_long[hl1] = ip1 as u32 + WINDOW_START_INDEX;
                }
                seq.store(&data[anchor..ip], offset_to_offbase(offset), length);
                ip += length;
                anchor = ip;
            }

            if ip as i64 <= ilimit {
                // inserção complementar: o índice `curr + 2` (posição `curr`) e o fim da correspondência
                let insert_pos = curr as usize;
                let (hl_a, hs_a) = (self.hash_l(data, insert_pos), self.hash_s(data, insert_pos));
                let (hl_b, hs_b) = (self.hash_l(data, ip - 2), self.hash_s(data, ip - 1));
                self.hash_long[hl_a] = curr + 2;
                self.hash_long[hl_b] = (ip - 2) as u32 + WINDOW_START_INDEX;
                self.hash_small[hs_a] = curr + 2;
                self.hash_small[hs_b] = (ip - 1) as u32 + WINDOW_START_INDEX;

                // repetição imediata
                while ip as i64 <= ilimit
                    && offset_2 > 0
                    && read32(data, ip) == read32(data, ip - offset_2 as usize)
                {
                    let back = ip - offset_2 as usize;
                    let r_length = count(data, ip + 4, back + 4, end) + 4;
                    std::mem::swap(&mut offset_1, &mut offset_2);
                    self.insert_both(data, ip);
                    seq.store(&data[anchor..anchor], REPCODE1_OFFBASE, r_length);
                    ip += r_length;
                    anchor = ip;
                }
            }
        }

        // _cleanup
        if saved_1 != 0 && offset_1 != 0 {
            saved_2 = saved_1;
        }
        self.rep[0] = if offset_1 != 0 { offset_1 } else { saved_1 };
        self.rep[1] = if offset_2 != 0 { offset_2 } else { saved_2 };
        end - anchor
    }
}
