//! Seção de sequências de um bloco comprimido: `ZSTD_decodeSeqHeaders`, `ZSTD_buildSeqTable`,
//! `ZSTD_buildFSETable`, `ZSTD_decodeSequence` e `ZSTD_execSequence` (libzstd 1.5.7, alvo de 64 bits).

use super::super::literals::{SET_BASIC, SET_COMPRESSED, SET_REPEAT, SET_RLE};
use super::super::sequences as enc;
use super::bits::RevBits;
use super::fse::{build_dtable, read_ncount};
use super::window::Window;
use super::Error;

/// `LONGNBSEQ`.
const LONG_NB_SEQ: usize = 0x7F00;

/// `ZSTD_seqSymbol`: célula da tabela FSE já com a base e os bits adicionais do símbolo.
#[derive(Clone, Copy, Default)]
struct Cell {
    next_state: u16,
    nb_bits: u8,
    extra_bits: u8,
    base: u32,
}

struct SeqTable {
    table_log: u32,
    cells: Vec<Cell>,
}

/// `LL_base`.
fn ll_base(code: usize) -> u32 {
    const HIGH: [u32; 20] =
        [16, 18, 20, 22, 24, 28, 32, 40, 48, 64, 0x80, 0x100, 0x200, 0x400, 0x800, 0x1000, 0x2000, 0x4000, 0x8000, 0x10000];
    if code < 16 {
        code as u32
    } else {
        HIGH[code - 16]
    }
}

/// `ML_base`.
fn ml_base(code: usize) -> u32 {
    const HIGH: [u32; 21] = [
        35, 37, 39, 41, 43, 47, 51, 59, 67, 83, 99, 0x83, 0x103, 0x203, 0x403, 0x803, 0x1003, 0x2003, 0x4003, 0x8003,
        0x10003,
    ];
    if code < 32 {
        code as u32 + 3
    } else {
        HIGH[code - 32]
    }
}

/// `OF_base`.
fn of_base(code: usize) -> u32 {
    match code {
        0 => 0,
        1 => 1,
        n => (1u32 << n) - 3,
    }
}

/// Qual das três tabelas: define as funções de base e de bits adicionais.
#[derive(Clone, Copy)]
enum Kind {
    LitLength,
    Offset,
    MatchLength,
}

impl Kind {
    fn base(self, code: usize) -> u32 {
        match self {
            Kind::LitLength => ll_base(code),
            Kind::Offset => of_base(code),
            Kind::MatchLength => ml_base(code),
        }
    }

    fn extra_bits(self, code: usize) -> u8 {
        match self {
            Kind::LitLength => enc::LL_BITS[code],
            Kind::Offset => code as u8,
            Kind::MatchLength => enc::ML_BITS[code],
        }
    }

    fn max_symbol(self) -> u32 {
        match self {
            Kind::LitLength => enc::MAX_LL,
            Kind::Offset => enc::MAX_OFF,
            Kind::MatchLength => enc::MAX_ML,
        }
    }

    fn max_log(self) -> u32 {
        match self {
            Kind::LitLength => enc::LL_FSE_LOG,
            Kind::Offset => enc::OFF_FSE_LOG,
            Kind::MatchLength => enc::ML_FSE_LOG,
        }
    }

    fn default_norm(self) -> (&'static [i16], u32) {
        match self {
            Kind::LitLength => (&enc::LL_DEFAULT_NORM, enc::LL_DEFAULT_NORM_LOG),
            Kind::Offset => (&enc::OF_DEFAULT_NORM, enc::OF_DEFAULT_NORM_LOG),
            Kind::MatchLength => (&enc::ML_DEFAULT_NORM, enc::ML_DEFAULT_NORM_LOG),
        }
    }
}

/// `ZSTD_buildFSETable`: a tabela FSE comum mais base e bits adicionais por símbolo.
fn build_table(kind: Kind, normalized: &[i16], table_log: u32) -> Result<SeqTable, Error> {
    let dt = build_dtable(normalized, table_log)?;
    let cells = dt
        .entries
        .iter()
        .map(|e| {
            let code = e.symbol as usize;
            Cell { next_state: e.new_state, nb_bits: e.nb_bits, extra_bits: kind.extra_bits(code), base: kind.base(code) }
        })
        .collect();
    Ok(SeqTable { table_log, cells })
}

/// `ZSTD_buildSeqTable_rle`: tabela de uma célula, sem bits de estado.
fn rle_table(kind: Kind, symbol: usize) -> SeqTable {
    SeqTable {
        table_log: 0,
        cells: vec![Cell { next_state: 0, nb_bits: 0, extra_bits: kind.extra_bits(symbol), base: kind.base(symbol) }],
    }
}

/// `ZSTD_buildSeqTable`: devolve os bytes do cabeçalho consumidos; `slot` guarda a tabela corrente
/// (o modo repetido reaproveita a do bloco anterior, inclusive a predefinida).
fn build_seq_table(kind: Kind, mode: u32, src: &[u8], slot: &mut Option<SeqTable>) -> Result<usize, Error> {
    match mode {
        SET_RLE => {
            let symbol = *src.first().ok_or(Error::SrcSizeWrong)?;
            if symbol as u32 > kind.max_symbol() {
                return Err(Error::Corruption);
            }
            *slot = Some(rle_table(kind, symbol as usize));
            Ok(1)
        }
        SET_BASIC => {
            let (norm, log) = kind.default_norm();
            *slot = Some(build_table(kind, norm, log)?);
            Ok(0)
        }
        SET_REPEAT => {
            if slot.is_none() {
                return Err(Error::Corruption);
            }
            Ok(0)
        }
        SET_COMPRESSED => {
            let nc = read_ncount(src, kind.max_symbol()).map_err(|_| Error::Corruption)?;
            if nc.table_log > kind.max_log() {
                return Err(Error::Corruption);
            }
            *slot = Some(build_table(kind, &nc.normalized, nc.table_log).map_err(|_| Error::Corruption)?);
            Ok(nc.consumed)
        }
        _ => Err(Error::Corruption),
    }
}

/// `seq_t`.
struct Sequence {
    lit_length: usize,
    match_length: usize,
    offset: u64,
}

struct FseState {
    state: usize,
}

impl FseState {
    fn init(bits: &mut RevBits, table: &SeqTable) -> Self {
        FseState { state: bits.read(table.table_log) as usize }
    }

    /// `ZSTD_updateFseStateWithDInfo`.
    fn update(&mut self, bits: &mut RevBits, cell: Cell) {
        self.state = cell.next_state as usize + bits.read(cell.nb_bits as u32) as usize;
    }
}

/// `ZSTD_decodeSequence`: lê uma sequência e atualiza os offsets recentes `rep`.
fn decode_sequence(
    rep: &mut [u64; 3],
    bits: &mut RevBits,
    states: &mut [FseState; 3],
    tables: [&SeqTable; 3],
    is_last: bool,
) -> Sequence {
    let [ll_state, of_state, ml_state] = states;
    let ll = tables[0].cells[ll_state.state];
    let of = tables[1].cells[of_state.state];
    let ml = tables[2].cells[ml_state.state];
    let offset: u64;
    if of.extra_bits > 1 {
        offset = of.base as u64 + bits.read(of.extra_bits as u32) as u64;
        rep[2] = rep[1];
        rep[1] = rep[0];
        rep[0] = offset;
    } else {
        let ll0 = (ll.base == 0) as usize;
        if of.extra_bits == 0 {
            offset = rep[ll0];
            rep[1] = rep[1 - ll0];
            rep[0] = offset;
        } else {
            let code = of.base as usize + ll0 + bits.read(1) as usize;
            let mut temp = if code == 3 { rep[0].wrapping_sub(1) } else { rep[code] };
            if temp == 0 {
                // Offset 0 é inválido: vira -1 e o `exec_sequence` acusa a corrupção.
                temp = u64::MAX;
            }
            if code != 1 {
                rep[2] = rep[1];
            }
            rep[1] = rep[0];
            rep[0] = temp;
            offset = temp;
        }
    }
    let mut match_length = ml.base as usize;
    if ml.extra_bits > 0 {
        match_length += bits.read(ml.extra_bits as u32) as usize;
    }
    let mut lit_length = ll.base as usize;
    if ll.extra_bits > 0 {
        lit_length += bits.read(ll.extra_bits as u32) as usize;
    }
    if !is_last {
        ll_state.update(bits, ll);
        ml_state.update(bits, ml);
        of_state.update(bits, of);
    }
    Sequence { lit_length, match_length, offset }
}

/// Estado de sequências de um quadro: as três tabelas correntes e os três offsets recentes.
pub struct SeqDecoder {
    ll: Option<SeqTable>,
    of: Option<SeqTable>,
    ml: Option<SeqTable>,
    rep: [u64; 3],
}

impl Default for SeqDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl SeqDecoder {
    pub fn new() -> Self {
        SeqDecoder { ll: None, of: None, ml: None, rep: [1, 4, 8] }
    }

    /// Começo de quadro: sem tabelas herdadas e offsets recentes 1, 4, 8.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// `ZSTD_decodeSeqHeaders`: devolve o número de sequências e os bytes do cabeçalho consumidos.
    fn decode_headers(&mut self, src: &[u8]) -> Result<(usize, usize), Error> {
        let first = *src.first().ok_or(Error::SrcSizeWrong)?;
        let mut ip = 1usize;
        let mut nb_seq = first as usize;
        if nb_seq > 0x7F {
            if nb_seq == 0xFF {
                if ip + 2 > src.len() {
                    return Err(Error::SrcSizeWrong);
                }
                nb_seq = u16::from_le_bytes([src[ip], src[ip + 1]]) as usize + LONG_NB_SEQ;
                ip += 2;
            } else {
                let low = *src.get(ip).ok_or(Error::SrcSizeWrong)?;
                nb_seq = ((nb_seq - 0x80) << 8) + low as usize;
                ip += 1;
            }
        }
        if nb_seq == 0 {
            if ip != src.len() {
                return Err(Error::Corruption);
            }
            return Ok((0, ip));
        }
        let modes = *src.get(ip).ok_or(Error::SrcSizeWrong)?;
        if modes & 3 != 0 {
            return Err(Error::Corruption);
        }
        ip += 1;
        ip += build_seq_table(Kind::LitLength, (modes >> 6) as u32, &src[ip..], &mut self.ll)?;
        ip += build_seq_table(Kind::Offset, ((modes >> 4) & 3) as u32, &src[ip..], &mut self.of)?;
        ip += build_seq_table(Kind::MatchLength, ((modes >> 2) & 3) as u32, &src[ip..], &mut self.ml)?;
        Ok((nb_seq, ip))
    }


    /// Decodifica a seção de sequências de um bloco e anexa o conteúdo regenerado em `out` e na
    /// `window` (literais entre as cópias, mais os literais que sobram no fim).
    pub fn decode_block(
        &mut self,
        section: &[u8],
        literals: &[u8],
        window: &mut Window,
        out: &mut Vec<u8>,
        block_size_max: usize,
    ) -> Result<(), Error> {
        let (mut nb_seq, header) = self.decode_headers(section)?;
        let start = out.len();
        let mut lit_pos = 0usize;
        if nb_seq > 0 {
            let mut bits = RevBits::new(&section[header..])?;
            // Campos separados: as tabelas são emprestadas só para leitura e `rep` para escrita.
            let SeqDecoder { ll, of, ml, rep } = self;
            let (ll, of, ml) = match (ll.as_ref(), of.as_ref(), ml.as_ref()) {
                (Some(ll), Some(of), Some(ml)) => (ll, of, ml),
                _ => return Err(Error::Corruption),
            };
            let mut states = [FseState::init(&mut bits, ll), FseState::init(&mut bits, of), FseState::init(&mut bits, ml)];
            while nb_seq > 0 {
                let seq = decode_sequence(rep, &mut bits, &mut states, [ll, of, ml], nb_seq == 1);
                exec_sequence(&seq, literals, &mut lit_pos, window, out, start, block_size_max)?;
                nb_seq -= 1;
            }
            if !bits.at_end() {
                return Err(Error::Corruption);
            }
        }
        let rest = &literals[lit_pos..];
        if out.len() - start + rest.len() > block_size_max {
            return Err(Error::DstSizeTooSmall);
        }
        window.push_slice(rest);
        out.extend_from_slice(rest);
        Ok(())
    }
}

/// `ZSTD_execSequence`: copia os literais da sequência e depois a cópia do passado.
fn exec_sequence(
    seq: &Sequence,
    literals: &[u8],
    lit_pos: &mut usize,
    window: &mut Window,
    out: &mut Vec<u8>,
    start: usize,
    block_size_max: usize,
) -> Result<(), Error> {
    if seq.lit_length > literals.len() - *lit_pos {
        return Err(Error::Corruption);
    }
    if (out.len() - start).saturating_add(seq.lit_length).saturating_add(seq.match_length) > block_size_max {
        return Err(Error::DstSizeTooSmall);
    }
    let lits = &literals[*lit_pos..*lit_pos + seq.lit_length];
    window.push_slice(lits);
    out.extend_from_slice(lits);
    *lit_pos += seq.lit_length;
    let offset = usize::try_from(seq.offset).map_err(|_| Error::Corruption)?;
    window.copy_match(offset, seq.match_length, out).ok_or(Error::Corruption)
}

#[cfg(test)]
mod tests {
    use super::super::Decoder;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
    }

    fn run(frame: &[u8], chunk: usize) -> Vec<u8> {
        let mut dec = Decoder::new();
        let mut out = Vec::new();
        for part in frame.chunks(chunk) {
            dec.push(part, &mut out).unwrap();
        }
        assert!(dec.finished());
        out
    }

    // Quadros medidos no bun 1.4.2 com `Bun.zstdCompressSync`.
    const REPEAT_ABC: &str = "28b52ffd201e4d0000186162630100866e08";
    const HELLO_WORLD: &str = "28b52ffd603800f50100e868656c6c6f20776f726c642c20212c212c212c212c212c212c212c21200f006011c01d5818e0042c0738061601dc8185014ec07280636011c05d00ce2f";
    const LINES: &str = "28b52ffd6090050504003249161790274907c0742dff291ca8dc5d01f6de5b0ea57f51e002c4e4f9bca4cde2f65aa347d95e579333793e2f69b3b8bdd6e851b6d7d5e44c9ecf4bda2c6eaf357a94ed0987014081e130048ac26114078462041886834050180876a811e0efff19e0e7112484e19f1fa5aa5255554a5595aaaa52a9aa5455c510cb03a90a";

    fn lines_text() -> String {
        (0..60).map(|i| format!("line {} of text with words {}\n", i % 7, i % 3)).collect()
    }

    #[test]
    fn abc_repeat() {
        assert_eq!(run(&hex(REPEAT_ABC), 1000), "abc".repeat(10).into_bytes());
    }

    #[test]
    fn hello_world_repeat() {
        let expected = "hello world, hello world, hello world! ".repeat(8).into_bytes();
        assert_eq!(run(&hex(HELLO_WORLD), 1000), expected);
        assert_eq!(run(&hex(HELLO_WORLD), 1), expected);
    }

    #[test]
    fn lines_with_sequences() {
        let expected = lines_text().into_bytes();
        assert_eq!(run(&hex(LINES), 1000), expected);
        assert_eq!(run(&hex(LINES), 7), expected);
    }
}
