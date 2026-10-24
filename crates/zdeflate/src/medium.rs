//! O `deflate_medium` do zlib-ng 2.2.4 (níveis 3 a 6 quando a estratégia não é `Z_HUFFMAN_ONLY` nem
//! `Z_RLE`), com o que ele traz junto do zlib-ng: o hash multiplicativo de 4 bytes numa tabela de 65536
//! cabeças (`insert_string_tpl.h`), a inserção que não repete a posição que já é a cabeça, e o
//! `longest_match` sem a variante lenta (`match_tpl.h`). É o compressor do bun (o binário dele traz o
//! zlib-ng no modo de compatibilidade, `1.3.1.zlib-ng`), e o `CompressionStream` usa o nível 6.
//!
//! A ordem das operações segue o C linha a linha, porque é ela que decide os bytes da saída. Os campos
//! de `struct match` são `uint16_t` no C; as posições aqui cabem neles (a janela tem no máximo 65536
//! bytes), então `u32` não muda nada.

use crate::{BlockState, Deflate, Flush, MAX_MATCH, MIN_LOOKAHEAD, MIN_MATCH, Stream};

/// `WANT_MIN_MATCH`: o zlib-ng só emite correspondência de 4 bytes ou mais no `deflate_medium`.
const WANT_MIN_MATCH: u32 = 4;
/// `HASH_SIZE` do zlib-ng, fixo (não depende do `memLevel`).
pub(crate) const NG_HASH_SIZE: usize = 65536;
/// `EARLY_EXIT_TRIGGER_LEVEL`: abaixo dele o `longest_match` para na primeira candidata que não melhora.
const EARLY_EXIT_TRIGGER_LEVEL: i32 = 5;

/// `struct match` do `deflate_medium.c`.
#[derive(Clone, Copy, Default)]
struct Match {
    match_start: u32,
    match_length: u32,
    strstart: u32,
    orgstart: u32,
}

impl Deflate {
    /// Um byte da janela; o que passa do fim lê zero (no C, o preenchimento da alocação).
    fn window_byte(&self, at: usize) -> u8 {
        self.window.get(at).copied().unwrap_or(0)
    }

    /// `zng_memread_4` em little-endian seguido de `HASH_CALC` e `HASH_MASK`.
    fn ng_hash(&self, at: usize) -> usize {
        let val = u32::from_le_bytes([self.window_byte(at), self.window_byte(at + 1), self.window_byte(at + 2), self.window_byte(at + 3)]);
        (val.wrapping_mul(2_654_435_761) >> 16) as usize & (NG_HASH_SIZE - 1)
    }

    /// `QUICK_INSERT_STRING`: põe `str` na tabela e devolve a cabeça anterior da cadeia.
    pub(crate) fn ng_quick_insert_string(&mut self, str: u32) -> u32 {
        let hm = self.ng_hash(str as usize);
        let head = self.head[hm];
        if u32::from(head) != str {
            self.prev[str as usize & self.w_mask] = head;
            self.head[hm] = str as u16;
        }
        u32::from(head)
    }

    /// `INSERT_STRING`: põe as `count` cadeias a partir de `str` na tabela.
    pub(crate) fn ng_insert_string(&mut self, str: u32, count: u32) {
        for idx in str..str + count {
            let hm = self.ng_hash(idx as usize);
            let head = self.head[hm];
            if u32::from(head) != idx & 0xffff {
                self.prev[idx as usize & self.w_mask] = head;
                self.head[hm] = idx as u16;
            }
        }
    }

    /// O deslocamento do byte final que o `longest_match` confere primeiro, para o `best_len` corrente.
    fn ng_end_offset(best_len: u32) -> usize {
        let mut offset = best_len - 1;
        if best_len >= 4 {
            offset -= 2;
            if best_len >= 8 {
                offset -= 4;
            }
        }
        offset as usize
    }

    /// A largura da comparação rápida (`zng_memcmp_2`, `_4` ou `_8`) para o `best_len` corrente.
    fn ng_probe_width(best_len: u32) -> usize {
        if best_len < 4 {
            2
        } else if best_len >= 8 {
            8
        } else {
            4
        }
    }

    fn ng_equal(&self, a: usize, b: usize, width: usize) -> bool {
        (0..width).all(|i| self.window_byte(a + i) == self.window_byte(b + i))
    }

    /// `LONGEST_MATCH` sem `LONGEST_MATCH_SLOW`.
    fn ng_longest_match(&mut self, mut cur_match: u32) -> u32 {
        let strstart = self.strstart;
        let scan = strstart as usize;
        let wmask = self.w_mask;
        let lookahead = self.lookahead;
        let mut best_len = if self.prev_length != 0 { self.prev_length } else { MIN_MATCH as u32 - 1 };
        let mut offset = Self::ng_end_offset(best_len);
        let mut chain_length = self.max_chain_length;
        if best_len >= self.good_match {
            chain_length >>= 2;
        }
        let nice_match = self.nice_match;
        let max_dist = self.max_dist();
        let limit = if strstart > max_dist { strstart - max_dist } else { 0 };
        let early_exit = self.level < EARLY_EXIT_TRIGGER_LEVEL;
        loop {
            if cur_match >= strstart {
                break;
            }
            let m = cur_match as usize;
            let width = Self::ng_probe_width(best_len);
            if self.ng_equal(m + offset, scan + offset, width) && self.ng_equal(m, scan, width) {
                // `COMPARE256(scan + 2, match + 2) + 2`.
                let mut len = 2usize;
                while len < MAX_MATCH && self.window_byte(scan + len) == self.window_byte(m + len) {
                    len += 1;
                }
                let len = len as u32;
                if len > best_len {
                    self.match_start = cur_match;
                    if len > lookahead {
                        return lookahead;
                    }
                    best_len = len;
                    if best_len >= nice_match {
                        return best_len;
                    }
                    offset = Self::ng_end_offset(best_len);
                } else if early_exit {
                    break;
                }
            }
            // `GOTO_NEXT_CHAIN`.
            chain_length -= 1;
            if chain_length == 0 {
                return best_len;
            }
            cur_match = u32::from(self.prev[cur_match as usize & wmask]);
            if cur_match <= limit {
                return best_len;
            }
        }
        best_len
    }

    /// `emit_match`: a correspondência curta demais sai como literais.
    fn ng_emit_match(&mut self, mut m: Match) -> bool {
        let mut bflush = false;
        if m.match_length < WANT_MIN_MATCH {
            while m.match_length != 0 {
                bflush |= self.t.tally_lit(self.window[m.strstart as usize]);
                self.lookahead -= 1;
                m.strstart += 1;
                m.match_length -= 1;
            }
            return bflush;
        }
        bflush |= self.t.tally_dist(m.strstart - m.match_start, m.match_length - MIN_MATCH as u32);
        self.lookahead -= m.match_length;
        bflush
    }

    /// `insert_match`.
    fn ng_insert_match(&mut self, mut m: Match) {
        if self.lookahead <= m.match_length + WANT_MIN_MATCH {
            return;
        }
        // A cadeia em `strstart` já está na tabela.
        m.strstart += 1;
        m.match_length -= 1;
        if m.match_length < WANT_MIN_MATCH - 1 {
            if m.match_length > 0 && m.strstart >= m.orgstart {
                if m.strstart + m.match_length - 1 >= m.orgstart {
                    self.ng_insert_string(m.strstart, m.match_length);
                } else {
                    self.ng_insert_string(m.strstart, m.orgstart - m.strstart + 1);
                }
            }
            return;
        }
        if m.strstart >= m.orgstart {
            if m.strstart + m.match_length - 1 >= m.orgstart {
                self.ng_insert_string(m.strstart, m.match_length);
            } else {
                self.ng_insert_string(m.strstart, m.orgstart - m.strstart + 1);
            }
        } else if m.orgstart < m.strstart + m.match_length {
            self.ng_insert_string(m.orgstart, m.strstart + m.match_length - m.orgstart);
        }
    }

    /// `fizzle_matches`: puxa a próxima correspondência para a esquerda quando a corrente encolhe a um byte.
    fn ng_fizzle_matches(&self, current: &mut Match, next: &mut Match) {
        if current.match_length <= 1 {
            return;
        }
        if current.match_length > 1 + next.match_start {
            return;
        }
        if current.match_length > 1 + next.strstart {
            return;
        }
        let first_match = (next.match_start + 1 - current.match_length) as usize;
        let first_orig = (next.strstart + 1 - current.match_length) as usize;
        if self.window_byte(first_match) != self.window_byte(first_orig) {
            return;
        }
        let mut c = *current;
        let mut n = *next;
        let max_dist = self.max_dist();
        let limit = if next.strstart > max_dist { next.strstart - max_dist } else { 0 };
        let mut changed = 0;
        let mut at_match = n.match_start as i64 - 1;
        let mut at_orig = n.strstart as i64 - 1;
        while at_match >= 0 && at_orig >= 0 && self.window_byte(at_match as usize) == self.window_byte(at_orig as usize) {
            if c.match_length < 1 || n.strstart <= limit || n.match_length >= 256 || n.match_start <= 1 {
                break;
            }
            n.strstart -= 1;
            n.match_start -= 1;
            n.match_length += 1;
            c.match_length -= 1;
            at_match -= 1;
            at_orig -= 1;
            changed += 1;
        }
        if changed == 0 {
            return;
        }
        if c.match_length <= 1 && n.match_length != 2 {
            n.orgstart += 1;
            *current = c;
            *next = n;
        }
    }

    /// Procura a correspondência que começa em `strstart` (cabeça `hash_head` já inserida); o segundo valor
    /// diz se a busca rodou (senão a correspondência é um literal de um byte).
    fn ng_find(&mut self, hash_head: u32) -> (Match, bool) {
        let mut m = Match { strstart: self.strstart, orgstart: self.strstart, ..Match::default() };
        let dist = i64::from(self.strstart) - i64::from(hash_head);
        if dist <= i64::from(self.max_dist()) && dist > 0 && hash_head != 0 {
            m.match_length = self.ng_longest_match(hash_head);
            m.match_start = self.match_start;
            (m, true)
        } else {
            m.match_start = 0;
            m.match_length = 1;
            (m, false)
        }
    }

    /// `deflate_medium`.
    pub(crate) fn deflate_medium(&mut self, strm: &mut Stream<'_, '_>, flush: Flush) -> BlockState {
        let early_exit = self.level < 5;
        let mut current;
        let mut next = Match::default();
        loop {
            if self.lookahead < MIN_LOOKAHEAD as u32 {
                self.fill_window(strm);
                if self.lookahead < MIN_LOOKAHEAD as u32 && flush == Flush::None {
                    return BlockState::NeedMore;
                }
                if self.lookahead == 0 {
                    break;
                }
                next.match_length = 0;
            }

            if !early_exit && next.match_length > 0 {
                current = next;
                next.match_length = 0;
            } else {
                let mut hash_head = 0;
                if self.lookahead >= WANT_MIN_MATCH {
                    hash_head = self.ng_quick_insert_string(self.strstart);
                }
                let (found, searched) = self.ng_find(hash_head);
                current = found;
                if searched {
                    if current.match_length < WANT_MIN_MATCH {
                        current.match_length = 1;
                    }
                    if current.match_start >= current.strstart {
                        // Pode acontecer depois de alguns recomeços.
                        current.match_length = 1;
                    }
                }
            }

            self.ng_insert_match(current);

            // Olha uma posição à frente.
            if !early_exit
                && self.lookahead > MIN_LOOKAHEAD as u32
                && current.strstart + current.match_length < (self.window_size - MIN_LOOKAHEAD) as u32
            {
                self.strstart = current.strstart + current.match_length;
                let hash_head = self.ng_quick_insert_string(self.strstart);
                let (found, searched) = self.ng_find(hash_head);
                next = found;
                if searched {
                    if next.match_start >= next.strstart {
                        next.match_length = 1;
                    }
                    if next.match_length < WANT_MIN_MATCH {
                        next.match_length = 1;
                    } else {
                        self.ng_fizzle_matches(&mut current, &mut next);
                    }
                }
                self.strstart = current.strstart;
            } else {
                next.match_length = 0;
            }

            let bflush = self.ng_emit_match(current);
            self.strstart += current.match_length;
            if bflush {
                self.flush_block_only(strm, false);
                if strm.avail_out() == 0 {
                    return BlockState::NeedMore;
                }
            }
        }
        self.insert = self.strstart.min(MIN_MATCH as u32 - 1);
        self.finish_blocks(strm, flush)
    }
}
