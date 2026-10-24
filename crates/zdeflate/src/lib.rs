//! Compressor deflate do zlib 1.3.1 (o `deflate.c` e o `trees.c`), traduzido sem `unsafe`, com a
//! saída idêntica byte a byte à do zlib do Debian 13 em todos os níveis, estratégias, modos de
//! `flush` e invólucros (deflate cru, zlib e gzip).
//!
//! A API segue o `z_stream`: cada chamada de [`Deflate::deflate`] recebe a entrada disponível e um
//! buffer de saída com capacidade fixa, e devolve quanto consumiu e quanto produziu. Isso importa
//! porque o zlib decide o tamanho dos blocos armazenados (nível 0) pelo `avail_out`, então quem
//! quer reproduzir um programa C precisa chamar com os mesmos tamanhos de buffer que ele usa.
//! [`compress`] é o `compress2`, para quem comprime tudo de uma vez.
//!
//! Fora do porte: `deflateSetDictionary`, `deflateGetDictionary`, `deflateParams`, `deflateTune`,
//! `deflateCopy`, `deflatePending` e `deflatePrime`, que nenhum programa portado usa.

mod deflate;
mod medium;
mod trees;

pub use deflate::compress;

pub const MIN_MATCH: usize = 3;
pub const MAX_MATCH: usize = 258;
const MIN_LOOKAHEAD: usize = MAX_MATCH + MIN_MATCH + 1;
const WIN_INIT: usize = MAX_MATCH;
const NIL: u32 = 0;
const TOO_FAR: u32 = 4096;

const LENGTH_CODES: usize = 29;
const LITERALS: usize = 256;
const L_CODES: usize = LITERALS + 1 + LENGTH_CODES;
const D_CODES: usize = 30;
const BL_CODES: usize = 19;
const HEAP_SIZE: usize = 2 * L_CODES + 1;
const MAX_BITS: usize = 15;

const INIT_STATE: i32 = 42;
const GZIP_STATE: i32 = 57;
const EXTRA_STATE: i32 = 69;
const NAME_STATE: i32 = 73;
const COMMENT_STATE: i32 = 91;
const HCRC_STATE: i32 = 103;
const BUSY_STATE: i32 = 113;
const FINISH_STATE: i32 = 666;

/// `OS_CODE` do zlib compilado para Unix.
const OS_CODE: u8 = 3;
const PRESET_DICT: u32 = 0x20;
const DEF_MEM_LEVEL: i32 = 8;
const MAX_MEM_LEVEL: i32 = 9;

/// Nível padrão (`Z_DEFAULT_COMPRESSION`), que o zlib troca por 6.
pub const DEFAULT_COMPRESSION: i32 = -1;

/// O parâmetro `flush` do `deflate()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flush {
    None = 0,
    Partial = 1,
    Sync = 2,
    Full = 3,
    Finish = 4,
    Block = 5,
}

impl Flush {
    /// `RANK`: põe o `Z_BLOCK` entre o `Z_NO_FLUSH` e o `Z_PARTIAL_FLUSH`.
    fn rank(self) -> i32 {
        let f = self as i32;
        f * 2 - if f > 4 { 9 } else { 0 }
    }
}

/// A estratégia (`Z_FILTERED`, `Z_HUFFMAN_ONLY`, `Z_RLE`, `Z_FIXED`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strategy {
    Default = 0,
    Filtered = 1,
    HuffmanOnly = 2,
    Rle = 3,
    Fixed = 4,
}

/// O que um `deflate()` bem-sucedido devolve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// `Z_OK`: progrediu, chame de novo.
    Ok,
    /// `Z_STREAM_END`: o fluxo terminou (só com [`Flush::Finish`]).
    StreamEnd,
}

/// Os erros do zlib que fazem sentido aqui.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// `Z_STREAM_ERROR`: parâmetro inválido ou uso fora de ordem.
    Stream,
    /// `Z_BUF_ERROR`: nada a fazer (sem espaço de saída ou sem entrada nova).
    Buf,
}

/// O que uma chamada do `deflate()` fez: entrada consumida e saída produzida (que podem ser
/// diferentes de zero mesmo num erro, como no `z_stream`) e o retorno.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub consumed: usize,
    pub produced: usize,
    pub status: Result<Status, Error>,
}

/// O cabeçalho gzip do `deflateSetHeader` (`gz_header`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GzHeader {
    pub text: bool,
    pub time: u32,
    pub os: u8,
    pub extra: Option<Vec<u8>>,
    /// Sem o zero final, que o zlib grava.
    pub name: Option<Vec<u8>>,
    pub comment: Option<Vec<u8>>,
    pub hcrc: bool,
}

/// Nó de árvore: `fc` é a frequência ou o código, `dl` é o pai ou o comprimento, como as uniões
/// do `ct_data`.
#[derive(Clone, Copy, Debug, Default)]
struct Ct {
    fc: u16,
    dl: u16,
}

/// O `next_in`/`avail_in` e o `next_out`/`avail_out` de uma chamada. A entrada inteira fica
/// visível porque o `deflate_stored` relê os últimos bytes consumidos (`next_in - w_size`).
struct Stream<'a, 'b> {
    input: &'a [u8],
    in_pos: usize,
    output: &'b mut [u8],
    out_pos: usize,
}

impl Stream<'_, '_> {
    fn avail_in(&self) -> usize {
        self.input.len() - self.in_pos
    }

    fn avail_out(&self) -> usize {
        self.output.len() - self.out_pos
    }
}

/// O resultado das funções de compressão (`block_state`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BlockState {
    NeedMore,
    BlockDone,
    FinishStarted,
    FinishDone,
}

/// Um fluxo de compressão: o `z_stream` com o `deflate_state`.
pub struct Deflate {
    total_in: u64,
    total_out: u64,
    adler: u32,
    data_type: i32,

    status: i32,
    p: trees::Pending,
    pending_buf_size: usize,
    /// 0 cru, 1 zlib, 2 gzip; negativo depois do trailer.
    wrap: i32,
    gzhead: Option<GzHeader>,
    gzindex: usize,
    last_flush: i32,

    w_size: usize,
    w_bits: u32,
    w_mask: usize,
    window: Vec<u8>,
    window_size: usize,
    prev: Vec<u16>,
    head: Vec<u16>,
    ins_h: u32,
    hash_size: usize,
    hash_mask: u32,
    hash_shift: u32,
    block_start: i64,

    match_length: u32,
    prev_match: u32,
    match_available: bool,
    strstart: u32,
    match_start: u32,
    lookahead: u32,
    prev_length: u32,
    max_chain_length: u32,
    /// Também o `max_insert_length` dos níveis 1 a 3.
    max_lazy_match: u32,
    level: i32,
    strategy: Strategy,
    good_match: u32,
    nice_match: u32,

    t: Box<trees::Trees>,
    /// Bytes no fim da janela ainda fora da tabela de hash.
    insert: u32,
    /// Até onde a janela já foi escrita ou zerada.
    high_water: usize,
    /// O sabor zlib-ng 2.2.4 (o do bun): nos níveis 3 a 6 o `deflate_medium` com o hash de 4 bytes.
    ng: bool,
}

impl std::fmt::Debug for Deflate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Deflate").field("level", &self.level).field("strategy", &self.strategy).field("wrap", &self.wrap).field("total_in", &self.total_in).field("total_out", &self.total_out).finish_non_exhaustive()
    }
}

/// `adler32`.
pub fn adler32(adler: u32, data: &[u8]) -> u32 {
    const BASE: u32 = 65521;
    const NMAX: usize = 5552;
    let mut a = adler & 0xffff;
    let mut b = adler >> 16;
    for chunk in data.chunks(NMAX) {
        for &c in chunk {
            a += c as u32;
            b += a;
        }
        a %= BASE;
        b %= BASE;
    }
    (b << 16) | a
}

fn crc32(crc: u32, data: &[u8]) -> u32 {
    let mut h = crc32fast::Hasher::new_with_initial(crc);
    h.update(data);
    h.finalize()
}

/// (good_length, max_lazy, nice_length, max_chain) por nível: `configuration_table`.
const CONFIG: [(u32, u32, u32, u32); 10] =
    [(0, 0, 0, 0), (4, 4, 8, 4), (4, 5, 16, 8), (4, 6, 32, 32), (4, 4, 16, 16), (8, 16, 32, 32), (8, 16, 128, 128), (8, 32, 128, 256), (32, 128, 258, 1024), (32, 258, 258, 4096)];

/// A `configuration_table` do zlib-ng 2.2.4 com o `deflate_medium` (só os níveis 3 e 4 mudam).
const CONFIG_NG: [(u32, u32, u32, u32); 10] =
    [(0, 0, 0, 0), (0, 0, 0, 0), (4, 4, 8, 4), (4, 6, 16, 6), (4, 12, 32, 24), (8, 16, 32, 32), (8, 16, 128, 128), (8, 32, 128, 256), (32, 128, 258, 1024), (32, 258, 258, 4096)];

impl Deflate {
    /// `deflateInit2`: `window_bits` de 8 a 15 dá o invólucro zlib, negativo dá deflate cru e
    /// somado a 16 dá gzip; `mem_level` de 1 a 9 (o padrão é 8).
    pub fn new(level: i32, window_bits: i32, mem_level: i32, strategy: Strategy) -> Result<Deflate, Error> {
        let level = if level == DEFAULT_COMPRESSION { 6 } else { level };
        let mut wrap = 1;
        let mut window_bits = window_bits;
        if window_bits < 0 {
            wrap = 0;
            if window_bits < -15 {
                return Err(Error::Stream);
            }
            window_bits = -window_bits;
        } else if window_bits > 15 {
            wrap = 2;
            window_bits -= 16;
        }
        if !(1..=MAX_MEM_LEVEL).contains(&mem_level) || !(8..=15).contains(&window_bits) || !(0..=9).contains(&level) || (window_bits == 8 && wrap != 1) {
            return Err(Error::Stream);
        }
        if window_bits == 8 {
            window_bits = 9;
        }
        let w_bits = window_bits as u32;
        let w_size = 1usize << w_bits;
        let hash_bits = mem_level as u32 + 7;
        let hash_size = 1usize << hash_bits;
        let lit_bufsize = 1usize << (mem_level + 6);
        let mut d = Deflate {
            total_in: 0,
            total_out: 0,
            adler: 0,
            data_type: trees::Z_UNKNOWN,
            status: INIT_STATE,
            p: trees::Pending::new(lit_bufsize * 4),
            pending_buf_size: lit_bufsize * 4,
            wrap,
            gzhead: None,
            gzindex: 0,
            last_flush: 0,
            w_size,
            w_bits,
            w_mask: w_size - 1,
            window: vec![0; 2 * w_size],
            window_size: 2 * w_size,
            prev: vec![0; w_size],
            head: vec![0; hash_size],
            ins_h: 0,
            hash_size,
            hash_mask: (hash_size - 1) as u32,
            hash_shift: (hash_bits + MIN_MATCH as u32 - 1) / MIN_MATCH as u32,
            block_start: 0,
            match_length: 0,
            prev_match: 0,
            match_available: false,
            strstart: 0,
            match_start: 0,
            lookahead: 0,
            prev_length: 0,
            max_chain_length: 0,
            max_lazy_match: 0,
            level,
            strategy,
            good_match: 0,
            nice_match: 0,
            t: trees::Trees::new(lit_bufsize),
            insert: 0,
            high_water: 0,
            ng: false,
        };
        d.reset();
        Ok(d)
    }

    /// O `deflateInit2` do zlib-ng 2.2.4 (o compressor do bun, que o `CompressionStream` reproduz): nos níveis
    /// 3 a 6, fora de `Z_HUFFMAN_ONLY` e `Z_RLE`, comprime com o `deflate_medium`. Os outros níveis seguem o
    /// zlib (o `deflate_quick` do nível 1 e o `deflate_slow` com o hash do zlib-ng não foram portados).
    pub fn new_zlib_ng(level: i32, window_bits: i32, mem_level: i32, strategy: Strategy) -> Result<Deflate, Error> {
        let mut d = Deflate::new(level, window_bits, mem_level, strategy)?;
        d.ng = true;
        if d.medium() {
            d.head = vec![0; medium::NG_HASH_SIZE];
        }
        d.lm_init();
        Ok(d)
    }

    /// Se este fluxo comprime com o `deflate_medium` do zlib-ng.
    fn medium(&self) -> bool {
        self.ng && (3..=6).contains(&self.level) && !matches!(self.strategy, Strategy::HuffmanOnly | Strategy::Rle)
    }

    /// `deflateInit`: invólucro zlib com os parâmetros padrão.
    pub fn zlib(level: i32) -> Result<Deflate, Error> {
        Deflate::new(level, 15, DEF_MEM_LEVEL, Strategy::Default)
    }

    /// `deflateReset`: recomeça o fluxo com os mesmos parâmetros. A janela e a marca até onde ela
    /// foi escrita sobrevivem, como no C.
    pub fn reset(&mut self) {
        self.total_in = 0;
        self.total_out = 0;
        self.data_type = trees::Z_UNKNOWN;
        self.p.pending = 0;
        self.p.out = 0;
        if self.wrap < 0 {
            self.wrap = -self.wrap;
        }
        self.status = if self.wrap == 2 { GZIP_STATE } else { INIT_STATE };
        self.adler = if self.wrap == 2 { 0 } else { 1 };
        self.last_flush = -2;
        self.p.bi_buf = 0;
        self.p.bi_valid = 0;
        self.t.reset();
        self.lm_init();
    }

    fn lm_init(&mut self) {
        self.head.fill(NIL as u16);
        let (good, lazy, nice, chain) = if self.medium() { CONFIG_NG[self.level as usize] } else { CONFIG[self.level as usize] };
        self.max_lazy_match = lazy;
        self.good_match = good;
        self.nice_match = nice;
        self.max_chain_length = chain;
        self.strstart = 0;
        self.block_start = 0;
        self.lookahead = 0;
        self.insert = 0;
        self.match_length = (MIN_MATCH - 1) as u32;
        // O zlib-ng começa o `prev_length` em zero; o `longest_match` dele lê isso como `STD_MIN_MATCH - 1`.
        self.prev_length = if self.medium() { 0 } else { (MIN_MATCH - 1) as u32 };
        self.match_available = false;
        self.ins_h = 0;
    }

    /// `deflateSetHeader`: só para o invólucro gzip, antes da primeira chamada.
    pub fn set_header(&mut self, head: GzHeader) -> Result<(), Error> {
        if self.wrap != 2 {
            return Err(Error::Stream);
        }
        self.gzhead = Some(head);
        Ok(())
    }

    pub fn total_in(&self) -> u64 {
        self.total_in
    }

    pub fn total_out(&self) -> u64 {
        self.total_out
    }

    /// O adler32 (zlib) ou o crc32 (gzip) da entrada até aqui.
    pub fn adler(&self) -> u32 {
        self.adler
    }

    /// `data_type`: 0 binário, 1 texto, 2 desconhecido (o primeiro bloco ainda não saiu).
    pub fn data_type(&self) -> i32 {
        self.data_type
    }

    /// `deflateBound`: o maior tamanho comprimido possível para `source_len` bytes de entrada.
    pub fn bound(&self, source_len: u64) -> u64 {
        let fixedlen = source_len + (source_len >> 3) + (source_len >> 8) + (source_len >> 9) + 4;
        let storelen = source_len + (source_len >> 5) + (source_len >> 7) + (source_len >> 11) + 7;
        let wraplen: u64 = match self.wrap {
            0 => 0,
            1 => 6 + if self.strstart != 0 { 4 } else { 0 },
            2 => {
                let mut w = 18;
                if let Some(h) = &self.gzhead {
                    if let Some(e) = &h.extra {
                        w += 2 + (e.len() as u64 & 0xffff);
                    }
                    if let Some(n) = &h.name {
                        w += n.len() as u64 + 1;
                    }
                    if let Some(c) = &h.comment {
                        w += c.len() as u64 + 1;
                    }
                    if h.hcrc {
                        w += 2;
                    }
                }
                w
            }
            _ => 6,
        };
        let hash_bits = self.hash_size.trailing_zeros();
        if self.w_bits != 15 || hash_bits != 8 + 7 {
            return (if self.w_bits <= hash_bits && self.level != 0 { fixedlen } else { storelen }) + wraplen;
        }
        source_len + (source_len >> 12) + (source_len >> 14) + (source_len >> 25) + 13 - 6 + wraplen
    }

    /// `read_buf`: tira até `size` bytes da entrada, atualizando o adler32 ou o crc32 e o
    /// `total_in`.
    fn read_buf<'a>(&mut self, strm: &mut Stream<'a, '_>, size: usize) -> &'a [u8] {
        let len = strm.avail_in().min(size);
        let data = &strm.input[strm.in_pos..strm.in_pos + len];
        if len == 0 {
            return data;
        }
        strm.in_pos += len;
        if self.wrap == 1 {
            self.adler = adler32(self.adler, data);
        } else if self.wrap == 2 {
            self.adler = crc32(self.adler, data);
        }
        self.total_in += len as u64;
        data
    }

    /// `flush_pending`: copia para a saída o quanto couber do `pending_buf`.
    fn flush_pending(&mut self, strm: &mut Stream) {
        self.p.bi_flush();
        let len = self.p.pending.min(strm.avail_out());
        if len == 0 {
            return;
        }
        strm.output[strm.out_pos..strm.out_pos + len].copy_from_slice(&self.p.buf[self.p.out..self.p.out + len]);
        strm.out_pos += len;
        self.p.out += len;
        self.total_out += len as u64;
        self.p.pending -= len;
        if self.p.pending == 0 {
            self.p.out = 0;
        }
    }
}
