//! O resto do `deflate.c` do zlib 1.3.1: a função `deflate()` com os cabeçalhos e os trailers,
//! o `fill_window`, a busca de correspondências e as cinco funções de compressão (armazenado,
//! rápida, preguiçosa, RLE e só Huffman). A ordem das operações segue o C linha a linha, porque é
//! ela que decide onde cada bloco termina e, portanto, os bytes da saída.

use crate::{
    BUSY_STATE, BlockState, COMMENT_STATE, Deflate, EXTRA_STATE, Error, FINISH_STATE, Flush, GZIP_STATE, HCRC_STATE, INIT_STATE, MAX_MATCH, MIN_LOOKAHEAD, MIN_MATCH, NAME_STATE, NIL, OS_CODE, PRESET_DICT, Progress, Status, Strategy,
    Stream, TOO_FAR, WIN_INIT, crc32,
};

/// Maior bloco armazenado (`MAX_STORED`).
const MAX_STORED: usize = 65535;
/// `Z_DEFLATED`.
const Z_DEFLATED: u32 = 8;

/// `RANK` de um `last_flush` guardado (que pode ser -1 ou -2).
fn rank(f: i32) -> i32 {
    f * 2 - if f > 4 { 9 } else { 0 }
}

impl Deflate {
    /// `deflate()`: comprime o quanto der de `input` em `output`. Como no `z_stream`, quem chama
    /// repassa a entrada não consumida na chamada seguinte.
    pub fn deflate(&mut self, input: &[u8], output: &mut [u8], flush: Flush) -> Progress {
        let mut strm = Stream { input, in_pos: 0, output, out_pos: 0 };
        let status = self.run(&mut strm, flush);
        Progress { consumed: strm.in_pos, produced: strm.out_pos, status }
    }

    fn run(&mut self, strm: &mut Stream<'_, '_>, flush: Flush) -> Result<Status, Error> {
        if self.status == FINISH_STATE && flush != Flush::Finish {
            return Err(Error::Stream);
        }
        if strm.avail_out() == 0 {
            return Err(Error::Buf);
        }
        let old_flush = self.last_flush;
        self.last_flush = flush as i32;

        // Despeja o que sobrou da chamada anterior.
        if self.p.pending != 0 {
            self.flush_pending(strm);
            if strm.avail_out() == 0 {
                // Sem espaço: a próxima chamada não pode tomar a falta de entrada por erro.
                self.last_flush = -1;
                return Ok(Status::Ok);
            }
        } else if strm.avail_in() == 0 && flush.rank() <= rank(old_flush) && flush != Flush::Finish {
            return Err(Error::Buf);
        }
        if self.status == FINISH_STATE && strm.avail_in() != 0 {
            return Err(Error::Buf);
        }

        if self.status == INIT_STATE && self.wrap == 0 {
            self.status = BUSY_STATE;
        }
        if self.status == INIT_STATE {
            // Cabeçalho zlib.
            let mut header = (Z_DEFLATED + ((self.w_bits - 8) << 4)) << 8;
            let level_flags = if self.strategy as i32 >= Strategy::HuffmanOnly as i32 || self.level < 2 {
                0
            } else if self.level < 6 {
                1
            } else if self.level == 6 {
                2
            } else {
                3
            };
            header |= level_flags << 6;
            if self.strstart != 0 {
                header |= PRESET_DICT;
            }
            header += 31 - (header % 31);
            self.p.put_short_msb(header);
            if self.strstart != 0 {
                self.p.put_short_msb(self.adler >> 16);
                self.p.put_short_msb(self.adler & 0xffff);
            }
            self.adler = 1;
            self.status = BUSY_STATE;
            self.flush_pending(strm);
            if self.p.pending != 0 {
                self.last_flush = -1;
                return Ok(Status::Ok);
            }
        }

        let head = self.gzhead.clone().unwrap_or_default();
        if self.status == GZIP_STATE {
            self.adler = 0;
            self.p.put_byte(31);
            self.p.put_byte(139);
            self.p.put_byte(8);
            let xfl = if self.level == 9 {
                2
            } else if self.strategy as i32 >= Strategy::HuffmanOnly as i32 || self.level < 2 {
                4
            } else {
                0
            };
            if self.gzhead.is_none() {
                for _ in 0..5 {
                    self.p.put_byte(0);
                }
                self.p.put_byte(xfl);
                self.p.put_byte(OS_CODE);
                self.status = BUSY_STATE;
                self.flush_pending(strm);
                if self.p.pending != 0 {
                    self.last_flush = -1;
                    return Ok(Status::Ok);
                }
            } else {
                let flags = u8::from(head.text) + if head.hcrc { 2 } else { 0 } + if head.extra.is_some() { 4 } else { 0 } + if head.name.is_some() { 8 } else { 0 } + if head.comment.is_some() { 16 } else { 0 };
                self.p.put_byte(flags);
                for shift in [0, 8, 16, 24] {
                    self.p.put_byte((head.time >> shift) as u8);
                }
                self.p.put_byte(xfl);
                self.p.put_byte(head.os);
                if let Some(extra) = &head.extra {
                    self.p.put_byte(extra.len() as u8);
                    self.p.put_byte((extra.len() >> 8) as u8);
                }
                if head.hcrc {
                    self.adler = crc32(self.adler, &self.p.buf[..self.p.pending]);
                }
                self.gzindex = 0;
                self.status = EXTRA_STATE;
            }
        }
        if self.status == EXTRA_STATE {
            if let Some(extra) = &head.extra {
                let mut beg = self.p.pending;
                let mut left = (extra.len() & 0xffff) - self.gzindex;
                while self.p.pending + left > self.pending_buf_size {
                    let copy = self.pending_buf_size - self.p.pending;
                    let at = self.p.pending;
                    self.p.buf[at..at + copy].copy_from_slice(&extra[self.gzindex..self.gzindex + copy]);
                    self.p.pending = self.pending_buf_size;
                    self.hcrc_update(head.hcrc, beg);
                    self.gzindex += copy;
                    self.flush_pending(strm);
                    if self.p.pending != 0 {
                        self.last_flush = -1;
                        return Ok(Status::Ok);
                    }
                    beg = 0;
                    left -= copy;
                }
                let at = self.p.pending;
                self.p.buf[at..at + left].copy_from_slice(&extra[self.gzindex..self.gzindex + left]);
                self.p.pending += left;
                self.hcrc_update(head.hcrc, beg);
                self.gzindex = 0;
            }
            self.status = NAME_STATE;
        }
        if self.status == NAME_STATE {
            if let Some(name) = &head.name {
                if self.put_gz_string(strm, name, head.hcrc) {
                    return Ok(Status::Ok);
                }
            }
            self.status = COMMENT_STATE;
        }
        if self.status == COMMENT_STATE {
            if let Some(comment) = &head.comment {
                if self.put_gz_string(strm, comment, head.hcrc) {
                    return Ok(Status::Ok);
                }
            }
            self.status = HCRC_STATE;
        }
        if self.status == HCRC_STATE {
            if head.hcrc {
                if self.p.pending + 2 > self.pending_buf_size {
                    self.flush_pending(strm);
                    if self.p.pending != 0 {
                        self.last_flush = -1;
                        return Ok(Status::Ok);
                    }
                }
                self.p.put_byte(self.adler as u8);
                self.p.put_byte((self.adler >> 8) as u8);
                self.adler = 0;
            }
            self.status = BUSY_STATE;
            // O cabeçalho sai inteiro antes da compressão.
            self.flush_pending(strm);
            if self.p.pending != 0 {
                self.last_flush = -1;
                return Ok(Status::Ok);
            }
        }

        // Comprime a entrada, ou emite o flush pedido.
        if strm.avail_in() != 0 || self.lookahead != 0 || (flush != Flush::None && self.status != FINISH_STATE) {
            let bstate = if self.level == 0 {
                self.deflate_stored(strm, flush)
            } else {
                match self.strategy {
                    Strategy::HuffmanOnly => self.deflate_huff(strm, flush),
                    Strategy::Rle => self.deflate_rle(strm, flush),
                    _ if self.medium() => self.deflate_medium(strm, flush),
                    _ if self.level <= 3 => self.deflate_fast(strm, flush),
                    _ => self.deflate_slow(strm, flush),
                }
            };
            if bstate == BlockState::FinishStarted || bstate == BlockState::FinishDone {
                self.status = FINISH_STATE;
            }
            if bstate == BlockState::NeedMore || bstate == BlockState::FinishStarted {
                if strm.avail_out() == 0 {
                    self.last_flush = -1;
                }
                return Ok(Status::Ok);
                // Com `NeedMore` e saída sobrando, a entrada acabou: a próxima chamada sem
                // entrada nova e com o mesmo flush devolve `Z_BUF_ERROR`.
            }
            if bstate == BlockState::BlockDone {
                if flush == Flush::Partial {
                    self.t.align(&mut self.p);
                } else if flush != Flush::Block {
                    // Bloco armazenado vazio, para o `Z_SYNC_FLUSH` e o `Z_FULL_FLUSH`.
                    self.t.stored_block(&mut self.p, &[], false);
                    if flush == Flush::Full {
                        self.head.fill(NIL as u16);
                        if self.lookahead == 0 {
                            self.strstart = 0;
                            self.block_start = 0;
                            self.insert = 0;
                        }
                    }
                }
                self.flush_pending(strm);
                if strm.avail_out() == 0 {
                    self.last_flush = -1;
                    return Ok(Status::Ok);
                }
            }
        }

        if flush != Flush::Finish {
            return Ok(Status::Ok);
        }
        if self.wrap <= 0 {
            return Ok(Status::StreamEnd);
        }
        // Trailer.
        if self.wrap == 2 {
            for shift in [0, 8, 16, 24] {
                self.p.put_byte((self.adler >> shift) as u8);
            }
            for shift in [0, 8, 16, 24] {
                self.p.put_byte((self.total_in >> shift) as u8);
            }
        } else {
            self.p.put_short_msb(self.adler >> 16);
            self.p.put_short_msb(self.adler & 0xffff);
        }
        self.flush_pending(strm);
        // O trailer só sai uma vez.
        if self.wrap > 0 {
            self.wrap = -self.wrap;
        }
        Ok(if self.p.pending != 0 { Status::Ok } else { Status::StreamEnd })
    }

    /// `HCRC_UPDATE`: soma ao crc do cabeçalho o que entrou no `pending_buf` desde `beg`.
    fn hcrc_update(&mut self, hcrc: bool, beg: usize) {
        if hcrc && self.p.pending > beg {
            self.adler = crc32(self.adler, &self.p.buf[beg..self.p.pending]);
        }
    }

    /// O nome ou o comentário do cabeçalho gzip, com o zero final. Devolve verdadeiro quando a
    /// saída encheu no meio e a chamada tem de retornar `Z_OK`.
    fn put_gz_string(&mut self, strm: &mut Stream<'_, '_>, s: &[u8], hcrc: bool) -> bool {
        let mut beg = self.p.pending;
        loop {
            if self.p.pending == self.pending_buf_size {
                self.hcrc_update(hcrc, beg);
                self.flush_pending(strm);
                if self.p.pending != 0 {
                    self.last_flush = -1;
                    return true;
                }
                beg = 0;
            }
            let val = s.get(self.gzindex).copied().unwrap_or(0);
            self.gzindex += 1;
            self.p.put_byte(val);
            if val == 0 {
                break;
            }
        }
        self.hcrc_update(hcrc, beg);
        self.gzindex = 0;
        false
    }

    /// `MAX_DIST`: a maior distância aceita, para que a janela nunca leia o que já saiu dela.
    pub(crate) fn max_dist(&self) -> u32 {
        (self.w_size - MIN_LOOKAHEAD) as u32
    }

    /// `UPDATE_HASH`.
    fn update_hash(&mut self, c: u8) {
        self.ins_h = ((self.ins_h << self.hash_shift) ^ c as u32) & self.hash_mask;
    }

    /// `INSERT_STRING`: põe a cadeia que começa em `s` na tabela de hash e devolve a cabeça
    /// anterior da cadeia.
    fn insert_string(&mut self, s: usize) -> u32 {
        self.update_hash(self.window[s + MIN_MATCH - 1]);
        let h = self.head[self.ins_h as usize];
        self.prev[s & self.w_mask] = h;
        self.head[self.ins_h as usize] = s as u16;
        h as u32
    }

    /// `slide_hash`: as posições da tabela descem `w_size`; as que saíram da janela viram NIL.
    fn slide_hash(&mut self) {
        let wsize = self.w_size;
        for m in self.head.iter_mut().chain(self.prev.iter_mut()) {
            *m = if *m as usize >= wsize { (*m as usize - wsize) as u16 } else { NIL as u16 };
        }
    }

    /// `fill_window`: lê entrada até ter `MIN_LOOKAHEAD` bytes à frente (ou a entrada acabar),
    /// deslizando a janela quando necessário, e zera até `WIN_INIT` bytes depois dos dados para
    /// que a busca de correspondências nunca compare memória não inicializada.
    pub(crate) fn fill_window(&mut self, strm: &mut Stream<'_, '_>) {
        let wsize = self.w_size;
        loop {
            let mut more = self.window_size - self.lookahead as usize - self.strstart as usize;
            if self.strstart as usize >= wsize + self.max_dist() as usize {
                self.window.copy_within(wsize..wsize + wsize - more, 0);
                self.match_start = self.match_start.wrapping_sub(wsize as u32);
                self.strstart -= wsize as u32;
                self.block_start -= wsize as i64;
                if self.insert > self.strstart {
                    self.insert = self.strstart;
                }
                self.slide_hash();
                more += wsize;
            }
            if strm.avail_in() == 0 {
                break;
            }
            let at = self.strstart as usize + self.lookahead as usize;
            let data = self.read_buf(strm, more);
            let n = data.len();
            self.window[at..at + n].copy_from_slice(data);
            self.lookahead += n as u32;

            // Põe na tabela os bytes que esperavam o que vem depois deles.
            if self.medium() {
                if self.lookahead + self.insert >= MIN_MATCH as u32 {
                    let str = self.strstart - self.insert;
                    // `max_chain_length` nunca passa de 1024 nos níveis do `deflate_medium`.
                    if str >= 1 {
                        self.ng_quick_insert_string(str + 2 - MIN_MATCH as u32);
                    }
                    let mut count = self.insert;
                    if self.lookahead == 1 {
                        count -= 1;
                    }
                    if count > 0 {
                        self.ng_insert_string(str, count);
                        self.insert -= count;
                    }
                }
            } else if self.lookahead + self.insert >= MIN_MATCH as u32 {
                let mut s = (self.strstart - self.insert) as usize;
                self.ins_h = self.window[s] as u32;
                self.update_hash(self.window[s + 1]);
                while self.insert != 0 {
                    self.update_hash(self.window[s + MIN_MATCH - 1]);
                    self.prev[s & self.w_mask] = self.head[self.ins_h as usize];
                    self.head[self.ins_h as usize] = s as u16;
                    s += 1;
                    self.insert -= 1;
                    if self.lookahead + self.insert < MIN_MATCH as u32 {
                        break;
                    }
                }
            }
            if !(self.lookahead < MIN_LOOKAHEAD as u32 && strm.avail_in() != 0) {
                break;
            }
        }

        if self.high_water < self.window_size {
            let curr = self.strstart as usize + self.lookahead as usize;
            if self.high_water < curr {
                let init = (self.window_size - curr).min(WIN_INIT);
                self.window[curr..curr + init].fill(0);
                self.high_water = curr + init;
            } else if self.high_water < curr + WIN_INIT {
                let init = (curr + WIN_INIT - self.high_water).min(self.window_size - self.high_water);
                self.window[self.high_water..self.high_water + init].fill(0);
                self.high_water += init;
            }
        }
    }

    /// `longest_match`: percorre a cadeia de hash a partir de `cur_match` e devolve o comprimento
    /// da maior correspondência (no máximo o `lookahead`), deixando o início em `match_start`.
    fn longest_match(&mut self, mut cur_match: u32) -> u32 {
        let mut chain_length = self.max_chain_length;
        let scan = self.strstart as usize;
        let mut best_len = self.prev_length as usize;
        let mut nice_match = self.nice_match as usize;
        let max_dist = self.max_dist();
        let limit = if self.strstart > max_dist { self.strstart - max_dist } else { NIL };
        let wmask = self.w_mask;
        let mut match_start = self.match_start;
        let w = &self.window;
        let mut scan_end1 = w[scan + best_len - 1];
        let mut scan_end = w[scan + best_len];

        if self.prev_length >= self.good_match {
            chain_length >>= 2;
        }
        if nice_match > self.lookahead as usize {
            nice_match = self.lookahead as usize;
        }
        loop {
            let m = cur_match as usize;
            // Descarta logo quem não estende a melhor até aqui nem começa igual.
            if w[m + best_len] == scan_end && w[m + best_len - 1] == scan_end1 && w[m] == w[scan] && w[m + 1] == w[scan + 1] {
                // O byte 2 não é comparado: com o hash igual e `hash_bits >= 8` ele bate. O laço
                // desenrolado do C para no primeiro byte diferente ou ao chegar em `strend`, o
                // que dá o mesmo comprimento desta comparação byte a byte.
                let mut len = MIN_MATCH;
                while len < MAX_MATCH && w[scan + len] == w[m + len] {
                    len += 1;
                }
                if len > best_len {
                    match_start = cur_match;
                    best_len = len;
                    if len >= nice_match {
                        break;
                    }
                    scan_end1 = w[scan + best_len - 1];
                    scan_end = w[scan + best_len];
                }
            }
            cur_match = self.prev[m & wmask] as u32;
            if cur_match <= limit {
                break;
            }
            chain_length -= 1;
            if chain_length == 0 {
                break;
            }
        }
        self.match_start = match_start;
        (best_len as u32).min(self.lookahead)
    }

    /// `FLUSH_BLOCK_ONLY`: fecha o bloco corrente e manda o que der para a saída.
    pub(crate) fn flush_block_only(&mut self, strm: &mut Stream<'_, '_>, last: bool) {
        let stored_len = (self.strstart as i64 - self.block_start) as u64;
        let buf = if self.block_start >= 0 { Some(&self.window[self.block_start as usize..]) } else { None };
        self.t.flush_block(&mut self.p, buf, stored_len, last, self.level, self.strategy, &mut self.data_type);
        self.block_start = self.strstart as i64;
        self.flush_pending(strm);
    }

    /// O fim comum das funções de compressão depois que a entrada acabou.
    pub(crate) fn finish_blocks(&mut self, strm: &mut Stream<'_, '_>, flush: Flush) -> BlockState {
        if flush == Flush::Finish {
            self.flush_block_only(strm, true);
            if strm.avail_out() == 0 {
                return BlockState::FinishStarted;
            }
            return BlockState::FinishDone;
        }
        if self.t.sym_next != 0 {
            self.flush_block_only(strm, false);
            if strm.avail_out() == 0 {
                return BlockState::NeedMore;
            }
        }
        BlockState::BlockDone
    }

    /// `deflate_stored`: nível 0. Copia direto da entrada para a saída quando dá, e senão junta na
    /// janela; o tamanho dos blocos depende do `avail_out`.
    fn deflate_stored(&mut self, strm: &mut Stream<'_, '_>, flush: Flush) -> BlockState {
        let f = flush as i32;
        let no_flush = Flush::None as i32;
        let finish = Flush::Finish as i32;
        let mut min_block = (self.pending_buf_size - 5).min(self.w_size);
        let mut last = false;
        let mut used = strm.avail_in();
        loop {
            let mut len = MAX_STORED;
            let mut have = (self.p.bi_valid as usize + 42) >> 3;
            if strm.avail_out() < have {
                break;
            }
            have = strm.avail_out() - have;
            let mut left = (self.strstart as i64 - self.block_start) as usize;
            if len > left + strm.avail_in() {
                len = left + strm.avail_in();
            }
            if len > have {
                len = have;
            }
            if len < min_block && ((len == 0 && f != finish) || f == no_flush || len != left + strm.avail_in()) {
                break;
            }
            last = f == finish && len == left + strm.avail_in();
            self.t.stored_block(&mut self.p, &[], last);
            // Corrige o tamanho no cabeçalho do bloco.
            let pend = self.p.pending;
            self.p.buf[pend - 4] = len as u8;
            self.p.buf[pend - 3] = (len >> 8) as u8;
            self.p.buf[pend - 2] = !len as u8;
            self.p.buf[pend - 1] = (!len >> 8) as u8;
            self.flush_pending(strm);

            if left != 0 {
                if left > len {
                    left = len;
                }
                let bs = self.block_start as usize;
                strm.output[strm.out_pos..strm.out_pos + left].copy_from_slice(&self.window[bs..bs + left]);
                strm.out_pos += left;
                self.total_out += left as u64;
                self.block_start += left as i64;
                len -= left;
            }
            if len != 0 {
                let data = self.read_buf(strm, len);
                strm.output[strm.out_pos..strm.out_pos + len].copy_from_slice(data);
                strm.out_pos += len;
                self.total_out += len as u64;
            }
            if last {
                break;
            }
        }

        // Guarda na janela o fim do que foi copiado direto, para um eventual dicionário.
        used -= strm.avail_in();
        if used != 0 {
            if used >= self.w_size {
                self.t.matches = 2;
                self.window[..self.w_size].copy_from_slice(&strm.input[strm.in_pos - self.w_size..strm.in_pos]);
                self.strstart = self.w_size as u32;
                self.insert = self.strstart;
            } else {
                if self.window_size - self.strstart as usize <= used {
                    // Desliza a janela.
                    self.strstart -= self.w_size as u32;
                    self.window.copy_within(self.w_size..self.w_size + self.strstart as usize, 0);
                    if self.t.matches < 2 {
                        self.t.matches += 1;
                    }
                    if self.insert > self.strstart {
                        self.insert = self.strstart;
                    }
                }
                let at = self.strstart as usize;
                self.window[at..at + used].copy_from_slice(&strm.input[strm.in_pos - used..strm.in_pos]);
                self.strstart += used as u32;
                self.insert += (used as u32).min(self.w_size as u32 - self.insert);
            }
            self.block_start = self.strstart as i64;
        }
        if self.high_water < self.strstart as usize {
            self.high_water = self.strstart as usize;
        }

        if last {
            return BlockState::FinishDone;
        }
        if f != no_flush && f != finish && strm.avail_in() == 0 && self.strstart as i64 == self.block_start {
            return BlockState::BlockDone;
        }

        // Enche a janela com o que der da entrada.
        let mut have = self.window_size - self.strstart as usize;
        if strm.avail_in() > have && self.block_start >= self.w_size as i64 {
            self.block_start -= self.w_size as i64;
            self.strstart -= self.w_size as u32;
            self.window.copy_within(self.w_size..self.w_size + self.strstart as usize, 0);
            if self.t.matches < 2 {
                self.t.matches += 1;
            }
            have += self.w_size;
            if self.insert > self.strstart {
                self.insert = self.strstart;
            }
        }
        if have > strm.avail_in() {
            have = strm.avail_in();
        }
        if have != 0 {
            let at = self.strstart as usize;
            let data = self.read_buf(strm, have);
            self.window[at..at + have].copy_from_slice(data);
            self.strstart += have as u32;
            self.insert += (have as u32).min(self.w_size as u32 - self.insert);
        }
        if self.high_water < self.strstart as usize {
            self.high_water = self.strstart as usize;
        }

        // Emite um bloco da janela se ele já tem o tamanho mínimo, ou se o flush pede.
        have = (self.p.bi_valid as usize + 42) >> 3;
        have = (self.pending_buf_size - have).min(MAX_STORED);
        min_block = have.min(self.w_size);
        let left = (self.strstart as i64 - self.block_start) as usize;
        if left >= min_block || ((left != 0 || f == finish) && f != no_flush && strm.avail_in() == 0 && left <= have) {
            let len = left.min(have);
            last = f == finish && strm.avail_in() == 0 && len == left;
            let bs = self.block_start as usize;
            self.t.stored_block(&mut self.p, &self.window[bs..bs + len], last);
            self.block_start += len as i64;
            self.flush_pending(strm);
        }
        if last { BlockState::FinishStarted } else { BlockState::NeedMore }
    }

    /// `deflate_fast`: níveis 1 a 3, sem avaliação preguiçosa; só insere na tabela as cadeias
    /// de correspondências curtas.
    fn deflate_fast(&mut self, strm: &mut Stream<'_, '_>, flush: Flush) -> BlockState {
        loop {
            if self.lookahead < MIN_LOOKAHEAD as u32 {
                self.fill_window(strm);
                if self.lookahead < MIN_LOOKAHEAD as u32 && flush == Flush::None {
                    return BlockState::NeedMore;
                }
                if self.lookahead == 0 {
                    break;
                }
            }
            let mut hash_head = NIL;
            if self.lookahead >= MIN_MATCH as u32 {
                hash_head = self.insert_string(self.strstart as usize);
            }
            if hash_head != NIL && self.strstart - hash_head <= self.max_dist() {
                self.match_length = self.longest_match(hash_head);
            }
            let bflush;
            if self.match_length >= MIN_MATCH as u32 {
                bflush = self.t.tally_dist(self.strstart - self.match_start, self.match_length - MIN_MATCH as u32);
                self.lookahead -= self.match_length;
                // O `max_lazy_match` faz aqui o papel do `max_insert_length`.
                if self.match_length <= self.max_lazy_match && self.lookahead >= MIN_MATCH as u32 {
                    self.match_length -= 1;
                    loop {
                        self.strstart += 1;
                        self.insert_string(self.strstart as usize);
                        self.match_length -= 1;
                        if self.match_length == 0 {
                            break;
                        }
                    }
                    self.strstart += 1;
                } else {
                    self.strstart += self.match_length;
                    self.match_length = 0;
                    self.ins_h = self.window[self.strstart as usize] as u32;
                    self.update_hash(self.window[self.strstart as usize + 1]);
                }
            } else {
                bflush = self.t.tally_lit(self.window[self.strstart as usize]);
                self.lookahead -= 1;
                self.strstart += 1;
            }
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

    /// `deflate_slow`: níveis 4 a 9, com avaliação preguiçosa: só aceita uma correspondência se
    /// a do byte seguinte não for maior.
    fn deflate_slow(&mut self, strm: &mut Stream<'_, '_>, flush: Flush) -> BlockState {
        loop {
            if self.lookahead < MIN_LOOKAHEAD as u32 {
                self.fill_window(strm);
                if self.lookahead < MIN_LOOKAHEAD as u32 && flush == Flush::None {
                    return BlockState::NeedMore;
                }
                if self.lookahead == 0 {
                    break;
                }
            }
            let mut hash_head = NIL;
            if self.lookahead >= MIN_MATCH as u32 {
                hash_head = self.insert_string(self.strstart as usize);
            }
            self.prev_length = self.match_length;
            self.prev_match = self.match_start;
            self.match_length = MIN_MATCH as u32 - 1;

            if hash_head != NIL && self.prev_length < self.max_lazy_match && self.strstart - hash_head <= self.max_dist() {
                self.match_length = self.longest_match(hash_head);
                // Correspondência de 3 bytes muito distante custa mais que os literais.
                if self.match_length <= 5 && (self.strategy == Strategy::Filtered || (self.match_length == MIN_MATCH as u32 && self.strstart - self.match_start > TOO_FAR)) {
                    self.match_length = MIN_MATCH as u32 - 1;
                }
            }

            if self.prev_length >= MIN_MATCH as u32 && self.match_length <= self.prev_length {
                let max_insert = self.strstart + self.lookahead - MIN_MATCH as u32;
                let bflush = self.t.tally_dist(self.strstart - 1 - self.prev_match, self.prev_length - MIN_MATCH as u32);
                // Insere as cadeias da correspondência, menos a primeira (já inserida) e a
                // última (que entra na próxima volta).
                self.lookahead -= self.prev_length - 1;
                self.prev_length -= 2;
                loop {
                    self.strstart += 1;
                    if self.strstart <= max_insert {
                        self.insert_string(self.strstart as usize);
                    }
                    self.prev_length -= 1;
                    if self.prev_length == 0 {
                        break;
                    }
                }
                self.match_available = false;
                self.match_length = MIN_MATCH as u32 - 1;
                self.strstart += 1;
                if bflush {
                    self.flush_block_only(strm, false);
                    if strm.avail_out() == 0 {
                        return BlockState::NeedMore;
                    }
                }
            } else if self.match_available {
                // A correspondência anterior perdeu: sai o byte anterior como literal.
                let bflush = self.t.tally_lit(self.window[self.strstart as usize - 1]);
                if bflush {
                    self.flush_block_only(strm, false);
                }
                self.strstart += 1;
                self.lookahead -= 1;
                if strm.avail_out() == 0 {
                    return BlockState::NeedMore;
                }
            } else {
                self.match_available = true;
                self.strstart += 1;
                self.lookahead -= 1;
            }
        }
        if self.match_available {
            self.t.tally_lit(self.window[self.strstart as usize - 1]);
            self.match_available = false;
        }
        self.insert = self.strstart.min(MIN_MATCH as u32 - 1);
        self.finish_blocks(strm, flush)
    }

    /// `deflate_rle`: só correspondências de distância 1 (repetições do byte anterior).
    fn deflate_rle(&mut self, strm: &mut Stream<'_, '_>, flush: Flush) -> BlockState {
        loop {
            if self.lookahead <= MAX_MATCH as u32 {
                self.fill_window(strm);
                if self.lookahead <= MAX_MATCH as u32 && flush == Flush::None {
                    return BlockState::NeedMore;
                }
                if self.lookahead == 0 {
                    break;
                }
            }
            self.match_length = 0;
            if self.lookahead >= MIN_MATCH as u32 && self.strstart > 0 {
                let s = self.strstart as usize;
                let w = &self.window;
                let prev = w[s - 1];
                if prev == w[s] && prev == w[s + 1] && prev == w[s + 2] {
                    // Mesmo critério do laço desenrolado: para no primeiro byte diferente ou em
                    // `MAX_MATCH`.
                    let mut len = MIN_MATCH;
                    while len < MAX_MATCH && w[s + len] == prev {
                        len += 1;
                    }
                    self.match_length = (len as u32).min(self.lookahead);
                }
            }
            let bflush;
            if self.match_length >= MIN_MATCH as u32 {
                bflush = self.t.tally_dist(1, self.match_length - MIN_MATCH as u32);
                self.lookahead -= self.match_length;
                self.strstart += self.match_length;
                self.match_length = 0;
            } else {
                bflush = self.t.tally_lit(self.window[self.strstart as usize]);
                self.lookahead -= 1;
                self.strstart += 1;
            }
            if bflush {
                self.flush_block_only(strm, false);
                if strm.avail_out() == 0 {
                    return BlockState::NeedMore;
                }
            }
        }
        self.insert = 0;
        self.finish_blocks(strm, flush)
    }

    /// `deflate_huff`: só literais, sem busca de correspondências.
    fn deflate_huff(&mut self, strm: &mut Stream<'_, '_>, flush: Flush) -> BlockState {
        loop {
            if self.lookahead == 0 {
                self.fill_window(strm);
                if self.lookahead == 0 {
                    if flush == Flush::None {
                        return BlockState::NeedMore;
                    }
                    break;
                }
            }
            self.match_length = 0;
            let bflush = self.t.tally_lit(self.window[self.strstart as usize]);
            self.lookahead -= 1;
            self.strstart += 1;
            if bflush {
                self.flush_block_only(strm, false);
                if strm.avail_out() == 0 {
                    return BlockState::NeedMore;
                }
            }
        }
        self.insert = 0;
        self.finish_blocks(strm, flush)
    }
}

/// `compress2`: comprime `source` inteiro com o invólucro zlib, numa saída do tamanho do
/// `compressBound`, como o C faz quando quem chama usa o `compressBound`.
pub fn compress(source: &[u8], level: i32) -> Result<Vec<u8>, Error> {
    let mut d = Deflate::zlib(level)?;
    let n = source.len() as u64;
    let bound = n + (n >> 12) + (n >> 14) + (n >> 25) + 13;
    let mut out = vec![0; bound as usize];
    let pr = d.deflate(source, &mut out, Flush::Finish);
    match pr.status {
        Ok(Status::StreamEnd) => {
            out.truncate(pr.produced);
            Ok(out)
        }
        Ok(Status::Ok) => Err(Error::Buf),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GzHeader, adler32};

    /// Leitor de bits do inflate de teste (LSB primeiro).
    struct Bits<'a> {
        d: &'a [u8],
        pos: usize,
        bit: u32,
        cnt: u32,
    }

    impl Bits<'_> {
        fn bits(&mut self, n: u32) -> u32 {
            let mut v = self.bit;
            while self.cnt < n {
                v |= (self.d[self.pos] as u32) << self.cnt;
                self.pos += 1;
                self.cnt += 8;
            }
            self.bit = if n == 32 { 0 } else { v >> n };
            self.cnt -= n;
            v & ((1u32 << n) - 1)
        }
    }

    /// Código canônico no estilo do `puff.c`: contagem por comprimento e símbolos ordenados.
    struct Huff {
        count: [i32; 16],
        symbol: Vec<i32>,
    }

    fn build(lengths: &[u8]) -> Huff {
        let mut count = [0i32; 16];
        for &l in lengths {
            count[l as usize] += 1;
        }
        let mut offs = [0i32; 16];
        for len in 1..15 {
            offs[len + 1] = offs[len] + count[len];
        }
        let mut symbol = vec![0; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbol[offs[l as usize] as usize] = sym as i32;
                offs[l as usize] += 1;
            }
        }
        count[0] = 0;
        Huff { count, symbol }
    }

    fn decode(b: &mut Bits, h: &Huff) -> i32 {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= b.bits(1) as i32;
            let count = h.count[len];
            if code - count < first {
                return h.symbol[(index + (code - first)) as usize];
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        panic!("código inválido");
    }

    const LBASE: [u32; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
    const LEXT: [u32; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
    const DBASE: [u32; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
    const DEXT: [u32; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

    /// Inflate cru mínimo, só para conferir a ida e volta. Devolve os dados e onde o fluxo
    /// terminou.
    fn inflate_raw(d: &[u8]) -> (Vec<u8>, usize) {
        let mut b = Bits { d, pos: 0, bit: 0, cnt: 0 };
        let mut out = Vec::new();
        loop {
            let last = b.bits(1);
            match b.bits(2) {
                0 => {
                    b.bit = 0;
                    b.cnt = 0;
                    let len = d[b.pos] as usize | (d[b.pos + 1] as usize) << 8;
                    let nlen = d[b.pos + 2] as usize | (d[b.pos + 3] as usize) << 8;
                    assert_eq!(len, !nlen & 0xffff);
                    b.pos += 4;
                    out.extend_from_slice(&d[b.pos..b.pos + len]);
                    b.pos += len;
                }
                t @ (1 | 2) => {
                    let (lit, dist) = if t == 1 {
                        let mut l = [0u8; 288];
                        for (i, x) in l.iter_mut().enumerate() {
                            *x = if i < 144 {
                                8
                            } else if i < 256 {
                                9
                            } else if i < 280 {
                                7
                            } else {
                                8
                            };
                        }
                        (build(&l), build(&[5u8; 30]))
                    } else {
                        let nlen = b.bits(5) as usize + 257;
                        let ndist = b.bits(5) as usize + 1;
                        let ncode = b.bits(4) as usize + 4;
                        const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
                        let mut cl = [0u8; 19];
                        for &o in &ORDER[..ncode] {
                            cl[o] = b.bits(3) as u8;
                        }
                        let ch = build(&cl);
                        let mut lens = Vec::new();
                        while lens.len() < nlen + ndist {
                            let sym = decode(&mut b, &ch);
                            match sym {
                                0..=15 => lens.push(sym as u8),
                                16 => {
                                    let p = *lens.last().unwrap();
                                    for _ in 0..3 + b.bits(2) {
                                        lens.push(p);
                                    }
                                }
                                17 => lens.extend(std::iter::repeat_n(0, 3 + b.bits(3) as usize)),
                                _ => lens.extend(std::iter::repeat_n(0, 11 + b.bits(7) as usize)),
                            }
                        }
                        (build(&lens[..nlen]), build(&lens[nlen..]))
                    };
                    loop {
                        let sym = decode(&mut b, &lit);
                        if sym < 256 {
                            out.push(sym as u8);
                        } else if sym == 256 {
                            break;
                        } else {
                            let s = sym as usize - 257;
                            let len = LBASE[s] + b.bits(LEXT[s]);
                            let ds = decode(&mut b, &dist) as usize;
                            let dd = (DBASE[ds] + b.bits(DEXT[ds])) as usize;
                            for _ in 0..len {
                                out.push(out[out.len() - dd]);
                            }
                        }
                    }
                }
                _ => panic!("tipo de bloco inválido"),
            }
            if last == 1 {
                return (out, b.pos);
            }
        }
    }

    /// Desembrulha o zlib, conferindo cabeçalho e adler32.
    fn unzlib(z: &[u8]) -> Vec<u8> {
        assert_eq!((z[0] as u32 * 256 + z[1] as u32) % 31, 0);
        assert_eq!(z[0], 0x78);
        let (out, end) = inflate_raw(&z[2..]);
        assert_eq!(2 + end + 4, z.len());
        let a = u32::from_be_bytes(z[z.len() - 4..].try_into().unwrap());
        assert_eq!(a, adler32(1, &out));
        out
    }

    /// Dados de teste: texto com repetições e trechos pseudoaleatórios.
    fn sample(n: usize) -> Vec<u8> {
        let mut x: u32 = 12345;
        let mut v = Vec::with_capacity(n);
        while v.len() < n {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            match (x >> 16) % 4 {
                0 => v.extend_from_slice(b"the quick brown fox jumps over the lazy dog\n"),
                1 => v.extend(std::iter::repeat_n((x >> 8) as u8, ((x >> 20) % 300) as usize)),
                _ => v.push((x >> 24) as u8),
            }
        }
        v.truncate(n);
        v
    }

    #[test]
    fn empty_level_0() {
        assert_eq!(compress(b"", 0).unwrap(), [0x78, 0x01, 0x01, 0x00, 0x00, 0xff, 0xff, 0x00, 0x00, 0x00, 0x01]);
    }

    #[test]
    fn empty_level_6() {
        assert_eq!(compress(b"", 6).unwrap(), [0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01]);
    }

    #[test]
    fn hello_level_6() {
        let src = b"hello hello hello hello\n";
        let z = compress(src, 6).unwrap();
        assert_eq!(&z[..2], &[0x78, 0x9c]);
        assert_eq!(unzlib(&z), src);
        // As repetições viram uma correspondência: menor que a entrada.
        assert!(z.len() < src.len());
    }

    #[test]
    fn roundtrip_levels_and_strategies() {
        let src = sample(200_000);
        for level in 0..=9 {
            assert_eq!(unzlib(&compress(&src, level).unwrap()), src, "nível {level}");
        }
        for strategy in [Strategy::Filtered, Strategy::HuffmanOnly, Strategy::Rle, Strategy::Fixed] {
            let mut d = Deflate::new(6, 15, 8, strategy).unwrap();
            let mut out = vec![0; src.len() * 2 + 100];
            let pr = d.deflate(&src, &mut out, Flush::Finish);
            assert_eq!(pr.status, Ok(Status::StreamEnd));
            assert_eq!(unzlib(&out[..pr.produced]), src, "{strategy:?}");
        }
    }

    /// Entrada e saída em pedaços pequenos dão os mesmos bytes que de uma vez nos níveis acima
    /// de 0 (no nível 0 o tamanho dos blocos depende do `avail_out`, como no C).
    #[test]
    fn chunked_matches_one_shot() {
        let src = sample(50_000);
        for level in [1, 6, 9] {
            let whole = compress(&src, level).unwrap();
            let mut d = Deflate::zlib(level).unwrap();
            let mut got = Vec::new();
            let mut pos = 0;
            let mut buf = [0u8; 7];
            loop {
                let end = (pos + 13).min(src.len());
                let flush = if end == src.len() { Flush::Finish } else { Flush::None };
                let pr = d.deflate(&src[pos..end], &mut buf, flush);
                pos += pr.consumed;
                got.extend_from_slice(&buf[..pr.produced]);
                match pr.status {
                    Ok(Status::StreamEnd) => break,
                    Ok(Status::Ok) | Err(Error::Buf) => {}
                    Err(e) => panic!("{e:?}"),
                }
            }
            assert_eq!(got, whole, "nível {level}");
        }
        let mut d = Deflate::zlib(0).unwrap();
        let mut got = Vec::new();
        let mut pos = 0;
        let mut buf = [0u8; 100];
        loop {
            let pr = d.deflate(&src[pos..], &mut buf, Flush::Finish);
            pos += pr.consumed;
            got.extend_from_slice(&buf[..pr.produced]);
            if pr.status == Ok(Status::StreamEnd) {
                break;
            }
        }
        assert_eq!(unzlib(&got), src);
    }

    #[test]
    fn sync_flush_and_gzip_header() {
        let src = sample(10_000);
        let mut d = Deflate::new(6, 31, 8, Strategy::Default).unwrap();
        d.set_header(GzHeader { name: Some(b"a.txt".to_vec()), comment: Some(b"c".to_vec()), extra: Some(vec![1, 2, 3]), hcrc: true, time: 7, ..Default::default() }).unwrap();
        let mut out = vec![0; 30_000];
        let pr1 = d.deflate(&src[..5000], &mut out, Flush::Sync);
        assert_eq!(pr1.status, Ok(Status::Ok));
        // O flush síncrono termina num bloco armazenado vazio.
        assert_eq!(&out[pr1.produced - 4..pr1.produced], &[0, 0, 0xff, 0xff]);
        let n1 = pr1.produced;
        let pr2 = d.deflate(&src[5000..], &mut out[n1..], Flush::Finish);
        assert_eq!(pr2.status, Ok(Status::StreamEnd));
        let g = &out[..n1 + pr2.produced];
        assert_eq!(&g[..4], &[0x1f, 0x8b, 8, 2 | 4 | 8 | 16]);
        // Cabeçalho: 10 bytes, extra (2 + 3), nome e comentário com o zero, crc16.
        let hlen = 10 + 5 + 6 + 2 + 2;
        let hcrc = crc32(0, &g[..hlen - 2]) as u16;
        assert_eq!(u16::from_le_bytes([g[hlen - 2], g[hlen - 1]]), hcrc);
        let (data, end) = inflate_raw(&g[hlen..]);
        assert_eq!(data, src);
        let t = &g[hlen + end..];
        assert_eq!(t.len(), 8);
        assert_eq!(u32::from_le_bytes(t[..4].try_into().unwrap()), crc32(0, &src));
        assert_eq!(u32::from_le_bytes(t[4..].try_into().unwrap()), src.len() as u32);
    }
}
