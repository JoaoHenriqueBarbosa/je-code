//! O `trees.c` do zlib 1.3.1: árvores de Huffman, a saída de bits e a escolha entre bloco
//! armazenado, estático e dinâmico. Sem `LIT_MEM` (como o Debian compila), os símbolos ficam no
//! `sym_buf`, três bytes por símbolo.
//!
//! O `ct_data` do C tem duas uniões (`Freq`/`Code` e `Dad`/`Len`) que o código reaproveita de
//! propósito: o `gen_bitlen` lê o comprimento do pai pelo mesmo campo onde estava o pai. [`Ct`]
//! guarda os dois campos com a mesma sobreposição.

use crate::{BL_CODES, Ct, D_CODES, HEAP_SIZE, L_CODES, LENGTH_CODES, LITERALS, MAX_BITS, MAX_MATCH, MIN_MATCH, Strategy};

const MAX_BL_BITS: usize = 7;
const END_BLOCK: usize = 256;
const REP_3_6: usize = 16;
const REPZ_3_10: usize = 17;
const REPZ_11_138: usize = 18;
const STORED_BLOCK: u32 = 0;
const STATIC_TREES: u32 = 1;
const DYN_TREES: u32 = 2;
/// Largura do `bi_buf`.
const BUF_SIZE: i32 = 16;

pub const Z_BINARY: i32 = 0;
pub const Z_TEXT: i32 = 1;
pub const Z_UNKNOWN: i32 = 2;

const EXTRA_LBITS: [u8; LENGTH_CODES] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const EXTRA_DBITS: [u8; D_CODES] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
const EXTRA_BLBITS: [u8; BL_CODES] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 3, 7];
const BL_ORDER: [u8; BL_CODES] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

const ZERO: Ct = Ct { fc: 0, dl: 0 };

/// As tabelas que o `tr_static_init` monta (o `trees.h` do zlib é a mesma coisa pré-calculada).
struct Static {
    ltree: [Ct; L_CODES + 2],
    dtree: [Ct; D_CODES],
    dist_code: [u8; 512],
    length_code: [u8; MAX_MATCH - MIN_MATCH + 1],
    base_length: [u16; LENGTH_CODES],
    base_dist: [u16; D_CODES],
}

static ST: Static = Static::init();

impl Static {
    const fn init() -> Static {
        let mut s = Static {
            ltree: [ZERO; L_CODES + 2],
            dtree: [ZERO; D_CODES],
            dist_code: [0; 512],
            length_code: [0; MAX_MATCH - MIN_MATCH + 1],
            base_length: [0; LENGTH_CODES],
            base_dist: [0; D_CODES],
        };
        let mut length = 0usize;
        let mut code = 0usize;
        while code < LENGTH_CODES - 1 {
            s.base_length[code] = length as u16;
            let mut n = 0;
            while n < (1 << EXTRA_LBITS[code]) {
                s.length_code[length] = code as u8;
                length += 1;
                n += 1;
            }
            code += 1;
        }
        // O comprimento 258 tem duas codificações (284 mais 5 bits ou 285); fica a mais curta.
        s.length_code[length - 1] = code as u8;

        let mut dist = 0usize;
        code = 0;
        while code < 16 {
            s.base_dist[code] = dist as u16;
            let mut n = 0;
            while n < (1 << EXTRA_DBITS[code]) {
                s.dist_code[dist] = code as u8;
                dist += 1;
                n += 1;
            }
            code += 1;
        }
        dist >>= 7;
        while code < D_CODES {
            s.base_dist[code] = (dist << 7) as u16;
            let mut n = 0;
            while n < (1 << (EXTRA_DBITS[code] - 7)) {
                s.dist_code[256 + dist] = code as u8;
                dist += 1;
                n += 1;
            }
            code += 1;
        }

        let mut bl_count = [0u16; MAX_BITS + 1];
        let mut n = 0;
        while n < L_CODES + 2 {
            let len = if n <= 143 {
                8
            } else if n <= 255 {
                9
            } else if n <= 279 {
                7
            } else {
                8
            };
            s.ltree[n].dl = len;
            bl_count[len as usize] += 1;
            n += 1;
        }
        // Os códigos 286 e 287 não existem, mas entram para a árvore ser canônica.
        gen_codes(&mut s.ltree, L_CODES + 1, &bl_count);
        n = 0;
        while n < D_CODES {
            s.dtree[n].dl = 5;
            s.dtree[n].fc = bi_reverse(n as u32, 5) as u16;
            n += 1;
        }
        s
    }
}

/// O `static_tree_desc`.
struct StaticDesc {
    stree: Option<&'static [Ct]>,
    extra: &'static [u8],
    extra_base: usize,
    elems: usize,
    max_length: usize,
}

static L_DESC: StaticDesc = StaticDesc { stree: Some(&ST.ltree), extra: &EXTRA_LBITS, extra_base: LITERALS + 1, elems: L_CODES, max_length: MAX_BITS };
static D_DESC: StaticDesc = StaticDesc { stree: Some(&ST.dtree), extra: &EXTRA_DBITS, extra_base: 0, elems: D_CODES, max_length: MAX_BITS };
static BL_DESC: StaticDesc = StaticDesc { stree: None, extra: &EXTRA_BLBITS, extra_base: 0, elems: BL_CODES, max_length: MAX_BL_BITS };

/// Inverte os `len` primeiros bits de `code`.
const fn bi_reverse(mut code: u32, mut len: u32) -> u32 {
    let mut res = 0;
    loop {
        res |= code & 1;
        code >>= 1;
        res <<= 1;
        len -= 1;
        if len == 0 {
            break;
        }
    }
    res >> 1
}

/// Gera os códigos de uma árvore a partir dos comprimentos (`Len`) e da contagem por comprimento.
const fn gen_codes(tree: &mut [Ct], max_code: usize, bl_count: &[u16; MAX_BITS + 1]) {
    let mut next_code = [0u16; MAX_BITS + 1];
    let mut code: u32 = 0;
    let mut bits = 1;
    while bits <= MAX_BITS {
        code = (code + bl_count[bits - 1] as u32) << 1;
        next_code[bits] = code as u16;
        bits += 1;
    }
    let mut n = 0;
    while n <= max_code {
        let len = tree[n].dl as usize;
        if len != 0 {
            tree[n].fc = bi_reverse(next_code[len] as u32, len as u32) as u16;
            next_code[len] = next_code[len].wrapping_add(1);
        }
        n += 1;
    }
}

/// `d_code`: o código de uma distância menos um.
fn d_code(dist: usize) -> usize {
    if dist < 256 { ST.dist_code[dist] as usize } else { ST.dist_code[256 + (dist >> 7)] as usize }
}

/// O `pending_buf` com o acumulador de bits (`bi_buf`, `bi_valid`).
pub(crate) struct Pending {
    pub buf: Vec<u8>,
    /// Bytes no buffer (`pending`).
    pub pending: usize,
    /// Início do que ainda não foi copiado para a saída (`pending_out`).
    pub out: usize,
    pub bi_buf: u16,
    pub bi_valid: i32,
}

impl Pending {
    pub fn new(size: usize) -> Pending {
        Pending { buf: vec![0; size], pending: 0, out: 0, bi_buf: 0, bi_valid: 0 }
    }

    pub fn put_byte(&mut self, c: u8) {
        self.buf[self.pending] = c;
        self.pending += 1;
    }

    /// `put_short`: LSB primeiro.
    fn put_short(&mut self, w: u16) {
        self.put_byte(w as u8);
        self.put_byte((w >> 8) as u8);
    }

    /// `putShortMSB` do `deflate.c`.
    pub fn put_short_msb(&mut self, b: u32) {
        self.put_byte((b >> 8) as u8);
        self.put_byte(b as u8);
    }

    fn send_bits(&mut self, value: u32, length: i32) {
        if self.bi_valid > BUF_SIZE - length {
            self.bi_buf |= (value << self.bi_valid) as u16;
            self.put_short(self.bi_buf);
            self.bi_buf = ((value as u16) as u32 >> (BUF_SIZE - self.bi_valid)) as u16;
            self.bi_valid += length - BUF_SIZE;
        } else {
            self.bi_buf |= (value << self.bi_valid) as u16;
            self.bi_valid += length;
        }
    }

    fn send_code(&mut self, c: usize, tree: &[Ct]) {
        self.send_bits(tree[c].fc as u32, tree[c].dl as i32);
    }

    /// `bi_flush`: deixa no máximo 7 bits no acumulador.
    pub fn bi_flush(&mut self) {
        if self.bi_valid == 16 {
            self.put_short(self.bi_buf);
            self.bi_buf = 0;
            self.bi_valid = 0;
        } else if self.bi_valid >= 8 {
            self.put_byte(self.bi_buf as u8);
            self.bi_buf >>= 8;
            self.bi_valid -= 8;
        }
    }

    /// `bi_windup`: alinha a saída no byte.
    pub fn bi_windup(&mut self) {
        if self.bi_valid > 8 {
            self.put_short(self.bi_buf);
        } else if self.bi_valid > 0 {
            self.put_byte(self.bi_buf as u8);
        }
        self.bi_buf = 0;
        self.bi_valid = 0;
    }
}

/// O rascunho da construção das árvores: o heap, as profundidades e os tamanhos do bloco.
struct Builder {
    heap: [usize; HEAP_SIZE],
    heap_len: usize,
    heap_max: usize,
    depth: [u8; HEAP_SIZE],
    bl_count: [u16; MAX_BITS + 1],
    /// Tamanho do bloco em bits com as árvores ótimas e com as estáticas.
    opt_len: u64,
    static_len: u64,
}

/// `smaller`: a frequência decide, a profundidade desempata.
fn smaller(tree: &[Ct], n: usize, m: usize, depth: &[u8]) -> bool {
    tree[n].fc < tree[m].fc || (tree[n].fc == tree[m].fc && depth[n] <= depth[m])
}

impl Builder {
    /// `pqdownheap`: desce o nó `k` até restaurar o heap.
    fn pqdownheap(&mut self, tree: &[Ct], mut k: usize) {
        let v = self.heap[k];
        let mut j = k << 1;
        while j <= self.heap_len {
            if j < self.heap_len && smaller(tree, self.heap[j + 1], self.heap[j], &self.depth) {
                j += 1;
            }
            if smaller(tree, v, self.heap[j], &self.depth) {
                break;
            }
            self.heap[k] = self.heap[j];
            k = j;
            j <<= 1;
        }
        self.heap[k] = v;
    }

    /// `gen_bitlen`: os comprimentos ótimos, limitados a `max_length` pelo ajuste do C.
    fn gen_bitlen(&mut self, tree: &mut [Ct], max_code: usize, desc: &StaticDesc) {
        let max_length = desc.max_length;
        self.bl_count = [0; MAX_BITS + 1];
        tree[self.heap[self.heap_max]].dl = 0;
        let mut overflow = 0i32;
        let mut h = self.heap_max + 1;
        while h < HEAP_SIZE {
            let n = self.heap[h];
            h += 1;
            let mut bits = tree[tree[n].dl as usize].dl as usize + 1;
            if bits > max_length {
                bits = max_length;
                overflow += 1;
            }
            // O `Dad` de `n` não serve mais e vira o `Len`.
            tree[n].dl = bits as u16;
            if n > max_code {
                continue;
            }
            self.bl_count[bits] += 1;
            let xbits = if n >= desc.extra_base { desc.extra[n - desc.extra_base] as u64 } else { 0 };
            let f = tree[n].fc as u64;
            self.opt_len = self.opt_len.wrapping_add(f * (bits as u64 + xbits));
            if let Some(st) = desc.stree {
                self.static_len = self.static_len.wrapping_add(f * (st[n].dl as u64 + xbits));
            }
        }
        if overflow == 0 {
            return;
        }
        loop {
            let mut bits = max_length - 1;
            while self.bl_count[bits] == 0 {
                bits -= 1;
            }
            self.bl_count[bits] -= 1;
            self.bl_count[bits + 1] += 2;
            self.bl_count[max_length] -= 1;
            overflow -= 2;
            if overflow <= 0 {
                break;
            }
        }
        let mut h = HEAP_SIZE;
        for bits in (1..=max_length).rev() {
            let mut n = self.bl_count[bits];
            while n != 0 {
                h -= 1;
                let m = self.heap[h];
                if m > max_code {
                    continue;
                }
                if tree[m].dl as usize != bits {
                    let delta = (bits as u64).wrapping_sub(tree[m].dl as u64).wrapping_mul(tree[m].fc as u64);
                    self.opt_len = self.opt_len.wrapping_add(delta);
                    tree[m].dl = bits as u16;
                }
                n -= 1;
            }
        }
    }

    /// `build_tree`: monta a árvore de Huffman e gera os códigos; devolve o `max_code`.
    fn build_tree(&mut self, tree: &mut [Ct], desc: &StaticDesc) -> usize {
        let elems = desc.elems;
        let mut max_code: i32 = -1;
        self.heap_len = 0;
        self.heap_max = HEAP_SIZE;
        for n in 0..elems {
            if tree[n].fc != 0 {
                self.heap_len += 1;
                self.heap[self.heap_len] = n;
                max_code = n as i32;
                self.depth[n] = 0;
            } else {
                tree[n].dl = 0;
            }
        }
        // O formato exige ao menos dois códigos com frequência.
        while self.heap_len < 2 {
            let node = if max_code < 2 {
                max_code += 1;
                max_code as usize
            } else {
                0
            };
            self.heap_len += 1;
            self.heap[self.heap_len] = node;
            tree[node].fc = 1;
            self.depth[node] = 0;
            self.opt_len = self.opt_len.wrapping_sub(1);
            if let Some(st) = desc.stree {
                self.static_len = self.static_len.wrapping_sub(st[node].dl as u64);
            }
        }
        let max_code = max_code as usize;
        for n in (1..=self.heap_len / 2).rev() {
            self.pqdownheap(tree, n);
        }
        let mut node = elems;
        loop {
            // `pqremove`
            let n = self.heap[1];
            self.heap[1] = self.heap[self.heap_len];
            self.heap_len -= 1;
            self.pqdownheap(tree, 1);
            let m = self.heap[1];
            self.heap_max -= 1;
            self.heap[self.heap_max] = n;
            self.heap_max -= 1;
            self.heap[self.heap_max] = m;
            tree[node].fc = tree[n].fc.wrapping_add(tree[m].fc);
            self.depth[node] = self.depth[n].max(self.depth[m]).wrapping_add(1);
            tree[n].dl = node as u16;
            tree[m].dl = node as u16;
            self.heap[1] = node;
            node += 1;
            self.pqdownheap(tree, 1);
            if self.heap_len < 2 {
                break;
            }
        }
        self.heap_max -= 1;
        self.heap[self.heap_max] = self.heap[1];
        self.gen_bitlen(tree, max_code, desc);
        gen_codes(tree, max_code, &self.bl_count);
        max_code
    }
}

/// Os limites de repetição depois de emitir um comprimento (`max_count`, `min_count`).
fn repeat_limits(curlen: i32, nextlen: i32) -> (i32, i32) {
    if nextlen == 0 {
        (138, 3)
    } else if curlen == nextlen {
        (6, 3)
    } else {
        (7, 4)
    }
}

/// `scan_tree`: conta, na árvore dos comprimentos, os códigos que vão descrever `tree`.
fn scan_tree(bl_tree: &mut [Ct], tree: &mut [Ct], max_code: usize) {
    let mut prevlen = -1i32;
    let mut nextlen = tree[0].dl as i32;
    let mut count = 0i32;
    let (mut max_count, mut min_count) = if nextlen == 0 { (138, 3) } else { (7, 4) };
    tree[max_code + 1].dl = 0xffff;
    for n in 0..=max_code {
        let curlen = nextlen;
        nextlen = tree[n + 1].dl as i32;
        count += 1;
        if count < max_count && curlen == nextlen {
            continue;
        } else if count < min_count {
            bl_tree[curlen as usize].fc = bl_tree[curlen as usize].fc.wrapping_add(count as u16);
        } else if curlen != 0 {
            if curlen != prevlen {
                bl_tree[curlen as usize].fc += 1;
            }
            bl_tree[REP_3_6].fc += 1;
        } else if count <= 10 {
            bl_tree[REPZ_3_10].fc += 1;
        } else {
            bl_tree[REPZ_11_138].fc += 1;
        }
        count = 0;
        prevlen = curlen;
        (max_count, min_count) = repeat_limits(curlen, nextlen);
    }
}

/// `send_tree`: emite `tree` comprimida com os códigos de `bl_tree` (a guarda já foi posta pelo
/// `scan_tree`).
fn send_tree(p: &mut Pending, bl_tree: &[Ct], tree: &[Ct], max_code: usize) {
    let mut prevlen = -1i32;
    let mut nextlen = tree[0].dl as i32;
    let mut count = 0i32;
    let (mut max_count, mut min_count) = if nextlen == 0 { (138, 3) } else { (7, 4) };
    for n in 0..=max_code {
        let curlen = nextlen;
        nextlen = tree[n + 1].dl as i32;
        count += 1;
        if count < max_count && curlen == nextlen {
            continue;
        } else if count < min_count {
            loop {
                p.send_code(curlen as usize, bl_tree);
                count -= 1;
                if count == 0 {
                    break;
                }
            }
        } else if curlen != 0 {
            if curlen != prevlen {
                p.send_code(curlen as usize, bl_tree);
                count -= 1;
            }
            p.send_code(REP_3_6, bl_tree);
            p.send_bits((count - 3) as u32, 2);
        } else if count <= 10 {
            p.send_code(REPZ_3_10, bl_tree);
            p.send_bits((count - 3) as u32, 3);
        } else {
            p.send_code(REPZ_11_138, bl_tree);
            p.send_bits((count - 11) as u32, 7);
        }
        count = 0;
        prevlen = curlen;
        (max_count, min_count) = repeat_limits(curlen, nextlen);
    }
}

/// O estado das árvores e o buffer de símbolos do bloco corrente.
pub(crate) struct Trees {
    dyn_ltree: [Ct; HEAP_SIZE],
    dyn_dtree: [Ct; 2 * D_CODES + 1],
    bl_tree: [Ct; 2 * BL_CODES + 1],
    l_max_code: usize,
    d_max_code: usize,
    b: Builder,
    /// Distância em dois bytes (LSB primeiro) e literal ou comprimento menos 3, por símbolo.
    pub sym_buf: Vec<u8>,
    pub sym_next: usize,
    pub sym_end: usize,
    /// Correspondências no bloco corrente.
    pub matches: u32,
}

impl Trees {
    /// `_tr_init`, com o `sym_buf` do tamanho do `lit_bufsize`.
    pub fn new(lit_bufsize: usize) -> Box<Trees> {
        let mut t = Box::new(Trees {
            dyn_ltree: [ZERO; HEAP_SIZE],
            dyn_dtree: [ZERO; 2 * D_CODES + 1],
            bl_tree: [ZERO; 2 * BL_CODES + 1],
            l_max_code: 0,
            d_max_code: 0,
            b: Builder { heap: [0; HEAP_SIZE], heap_len: 0, heap_max: 0, depth: [0; HEAP_SIZE], bl_count: [0; MAX_BITS + 1], opt_len: 0, static_len: 0 },
            sym_buf: vec![0; lit_bufsize * 3],
            sym_next: 0,
            sym_end: (lit_bufsize - 1) * 3,
            matches: 0,
        });
        t.init_block();
        t
    }

    /// `_tr_init` de um fluxo que recomeça (o `deflateReset`).
    pub fn reset(&mut self) {
        self.init_block();
    }

    fn init_block(&mut self) {
        for c in &mut self.dyn_ltree[..L_CODES] {
            c.fc = 0;
        }
        for c in &mut self.dyn_dtree[..D_CODES] {
            c.fc = 0;
        }
        for c in &mut self.bl_tree[..BL_CODES] {
            c.fc = 0;
        }
        self.dyn_ltree[END_BLOCK].fc = 1;
        self.b.opt_len = 0;
        self.b.static_len = 0;
        self.sym_next = 0;
        self.matches = 0;
    }

    /// `_tr_tally` de um literal; devolve se o bloco tem de ser despejado.
    pub fn tally_lit(&mut self, c: u8) -> bool {
        self.sym_buf[self.sym_next] = 0;
        self.sym_buf[self.sym_next + 1] = 0;
        self.sym_buf[self.sym_next + 2] = c;
        self.sym_next += 3;
        self.dyn_ltree[c as usize].fc = self.dyn_ltree[c as usize].fc.wrapping_add(1);
        self.sym_next == self.sym_end
    }

    /// `_tr_tally` de uma correspondência (`len` já sem o `MIN_MATCH`).
    pub fn tally_dist(&mut self, dist: u32, len: u32) -> bool {
        let len = len as u8;
        let dist = dist as u16;
        self.sym_buf[self.sym_next] = dist as u8;
        self.sym_buf[self.sym_next + 1] = (dist >> 8) as u8;
        self.sym_buf[self.sym_next + 2] = len;
        self.sym_next += 3;
        self.matches += 1;
        let dist = dist.wrapping_sub(1) as usize;
        let lc = ST.length_code[len as usize] as usize + LITERALS + 1;
        self.dyn_ltree[lc].fc = self.dyn_ltree[lc].fc.wrapping_add(1);
        let dc = d_code(dist);
        self.dyn_dtree[dc].fc = self.dyn_dtree[dc].fc.wrapping_add(1);
        self.sym_next == self.sym_end
    }

    /// `build_bl_tree`: devolve o índice em `BL_ORDER` do último código de comprimento a enviar.
    fn build_bl_tree(&mut self) -> usize {
        scan_tree(&mut self.bl_tree, &mut self.dyn_ltree, self.l_max_code);
        scan_tree(&mut self.bl_tree, &mut self.dyn_dtree, self.d_max_code);
        self.b.build_tree(&mut self.bl_tree, &BL_DESC);
        let mut max_blindex = BL_CODES - 1;
        while max_blindex >= 3 {
            if self.bl_tree[BL_ORDER[max_blindex] as usize].dl != 0 {
                break;
            }
            max_blindex -= 1;
        }
        self.b.opt_len = self.b.opt_len.wrapping_add(3 * (max_blindex as u64 + 1) + 5 + 5 + 4);
        max_blindex
    }

    fn send_all_trees(&self, p: &mut Pending, lcodes: usize, dcodes: usize, blcodes: usize) {
        p.send_bits((lcodes - 257) as u32, 5);
        p.send_bits((dcodes - 1) as u32, 5);
        p.send_bits((blcodes - 4) as u32, 4);
        for rank in 0..blcodes {
            p.send_bits(self.bl_tree[BL_ORDER[rank] as usize].dl as u32, 3);
        }
        send_tree(p, &self.bl_tree, &self.dyn_ltree, lcodes - 1);
        send_tree(p, &self.bl_tree, &self.dyn_dtree, dcodes - 1);
    }

    /// `compress_block`: os símbolos do bloco com as árvores dadas.
    fn compress_block(&self, p: &mut Pending, ltree: &[Ct], dtree: &[Ct]) {
        let mut sx = 0;
        while sx < self.sym_next {
            let mut dist = self.sym_buf[sx] as usize | (self.sym_buf[sx + 1] as usize) << 8;
            let mut lc = self.sym_buf[sx + 2] as u32;
            sx += 3;
            if dist == 0 {
                p.send_code(lc as usize, ltree);
            } else {
                let code = ST.length_code[lc as usize] as usize;
                p.send_code(code + LITERALS + 1, ltree);
                let extra = EXTRA_LBITS[code];
                if extra != 0 {
                    lc -= ST.base_length[code] as u32;
                    p.send_bits(lc, extra as i32);
                }
                dist -= 1;
                let code = d_code(dist);
                p.send_code(code, dtree);
                let extra = EXTRA_DBITS[code];
                if extra != 0 {
                    dist -= ST.base_dist[code] as usize;
                    p.send_bits(dist as u32, extra as i32);
                }
            }
        }
        p.send_code(END_BLOCK, ltree);
    }

    /// `detect_data_type`: texto se não houver byte da lista de bloqueio e houver um permitido.
    fn detect_data_type(&self) -> i32 {
        let mut block_mask: u32 = 0xf3ff_c07f;
        for n in 0..=31 {
            if block_mask & 1 != 0 && self.dyn_ltree[n].fc != 0 {
                return Z_BINARY;
            }
            block_mask >>= 1;
        }
        if self.dyn_ltree[9].fc != 0 || self.dyn_ltree[10].fc != 0 || self.dyn_ltree[13].fc != 0 {
            return Z_TEXT;
        }
        if self.dyn_ltree[32..LITERALS].iter().any(|c| c.fc != 0) {
            return Z_TEXT;
        }
        Z_BINARY
    }

    /// `_tr_flush_block`: escolhe entre armazenado, estático e dinâmico e emite o bloco. `buf` é a
    /// janela a partir do `block_start` (ausente quando ele é negativo).
    #[allow(clippy::too_many_arguments)]
    pub fn flush_block(&mut self, p: &mut Pending, buf: Option<&[u8]>, stored_len: u64, last: bool, level: i32, strategy: Strategy, data_type: &mut i32) {
        let mut max_blindex = 0;
        let mut opt_lenb;
        let static_lenb;
        if level > 0 {
            if *data_type == Z_UNKNOWN {
                *data_type = self.detect_data_type();
            }
            self.l_max_code = self.b.build_tree(&mut self.dyn_ltree, &L_DESC);
            self.d_max_code = self.b.build_tree(&mut self.dyn_dtree, &D_DESC);
            max_blindex = self.build_bl_tree();
            opt_lenb = (self.b.opt_len.wrapping_add(3 + 7)) >> 3;
            static_lenb = (self.b.static_len.wrapping_add(3 + 7)) >> 3;
            if static_lenb <= opt_lenb || strategy == Strategy::Fixed {
                opt_lenb = static_lenb;
            }
        } else {
            opt_lenb = stored_len + 5;
            static_lenb = opt_lenb;
        }
        let last_bit = last as u32;
        match buf {
            Some(buf) if stored_len + 4 <= opt_lenb => self.stored_block(p, &buf[..stored_len as usize], last),
            _ if static_lenb == opt_lenb => {
                p.send_bits((STATIC_TREES << 1) + last_bit, 3);
                self.compress_block(p, &ST.ltree, &ST.dtree);
            }
            _ => {
                p.send_bits((DYN_TREES << 1) + last_bit, 3);
                self.send_all_trees(p, self.l_max_code + 1, self.d_max_code + 1, max_blindex + 1);
                self.compress_block(p, &self.dyn_ltree, &self.dyn_dtree);
            }
        }
        self.init_block();
        if last {
            p.bi_windup();
        }
    }

    /// `_tr_stored_block`.
    pub fn stored_block(&self, p: &mut Pending, buf: &[u8], last: bool) {
        p.send_bits((STORED_BLOCK << 1) + last as u32, 3);
        p.bi_windup();
        let len = buf.len();
        p.put_short(len as u16);
        p.put_short(!(len as u16));
        p.buf[p.pending..p.pending + len].copy_from_slice(buf);
        p.pending += len;
    }

    /// `_tr_align`: um bloco estático vazio, para o inflate ter folga.
    pub fn align(&self, p: &mut Pending) {
        p.send_bits(STATIC_TREES << 1, 3);
        p.send_code(END_BLOCK, &ST.ltree);
        p.bi_flush();
    }
}

