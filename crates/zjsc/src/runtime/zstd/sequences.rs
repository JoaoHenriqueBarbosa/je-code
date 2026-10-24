//! Codificação das sequências: porte de `lib/compress/zstd_compress_sequences.c` e dos trechos de
//! `zstd_compress.c` (`ZSTD_seqToCodes`, `ZSTD_buildSequencesStatistics`) e de `zstd_internal.h` /
//! `zstd_compress_internal.h` (tabelas predefinidas, `ZSTD_LLcode`, `ZSTD_MLcode`), libzstd 1.5.7.
//!
//! Alvo de 64 bits: `longOffsets` do C só existe com `MEM_32bits()`, então o `ofCode` nunca chega a
//! `STREAM_ACCUMULATOR_MIN` e o caminho de offsets longos não existe aqui.

use super::fse::{
    build_ctable, build_ctable_rle, hist_count, normalize_count, optimal_table_log, write_ncount, BitCStream, CState,
    CTable, FseError, FseResult, FSE_NCOUNTBOUND,
};
use super::literals::{SET_BASIC, SET_COMPRESSED, SET_REPEAT, SET_RLE};
use super::params::highbit32;
use super::seq_store::Sequence;

pub const MAX_LL: u32 = 35;
pub const MAX_ML: u32 = 52;
pub const DEFAULT_MAX_OFF: u32 = 28;
pub const MAX_OFF: u32 = 31;
/// `MaxSeq`: `MAX(MaxLL, MaxML)`.
pub const MAX_SEQ: usize = 52;
pub const ML_FSE_LOG: u32 = 9;
pub const LL_FSE_LOG: u32 = 9;
pub const OFF_FSE_LOG: u32 = 8;
/// `MINMATCH`.
pub const MIN_MATCH: usize = 3;
/// `ZSTD_lazy` (número da estratégia).
const STRATEGY_LAZY: u32 = 4;

pub const LL_BITS: [u8; 36] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 3, 3, 4, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
];
pub const LL_DEFAULT_NORM: [i16; 36] = [
    4, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3, 2, 1, 1, 1, 1, 1, -1, -1, -1, -1,
];
pub const LL_DEFAULT_NORM_LOG: u32 = 6;

pub const ML_BITS: [u8; 53] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 3,
    3, 4, 4, 5, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
];
pub const ML_DEFAULT_NORM: [i16; 53] = [
    1, 4, 3, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, -1, -1, -1, -1, -1, -1, -1,
];
pub const ML_DEFAULT_NORM_LOG: u32 = 6;

pub const OF_DEFAULT_NORM: [i16; 29] =
    [1, 1, 1, 1, 1, 1, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, -1, -1, -1, -1, -1];
pub const OF_DEFAULT_NORM_LOG: u32 = 5;

/// `kInverseProbabilityLog256`.
const INVERSE_PROBABILITY_LOG_256: [u32; 256] = [
    0, 2048, 1792, 1642, 1536, 1453, 1386, 1329, 1280, 1236, 1197, 1162, 1130, 1100, 1073, 1047, 1024, 1001, 980, 960,
    941, 923, 906, 889, 874, 859, 844, 830, 817, 804, 791, 779, 768, 756, 745, 734, 724, 714, 704, 694, 685, 676, 667,
    658, 650, 642, 633, 626, 618, 610, 603, 595, 588, 581, 574, 567, 561, 554, 548, 542, 535, 529, 523, 517, 512, 506,
    500, 495, 489, 484, 478, 473, 468, 463, 458, 453, 448, 443, 438, 434, 429, 424, 420, 415, 411, 407, 402, 398, 394,
    390, 386, 382, 377, 373, 370, 366, 362, 358, 354, 350, 347, 343, 339, 336, 332, 329, 325, 322, 318, 315, 311, 308,
    305, 302, 298, 295, 292, 289, 286, 282, 279, 276, 273, 270, 267, 264, 261, 258, 256, 253, 250, 247, 244, 241, 239,
    236, 233, 230, 228, 225, 222, 220, 217, 215, 212, 209, 207, 204, 202, 199, 197, 194, 192, 190, 187, 185, 182, 180,
    178, 175, 173, 171, 168, 166, 164, 162, 159, 157, 155, 153, 151, 149, 146, 144, 142, 140, 138, 136, 134, 132, 130,
    128, 126, 123, 121, 119, 117, 115, 114, 112, 110, 108, 106, 104, 102, 100, 98, 96, 94, 93, 91, 89, 87, 85, 83, 82,
    80, 78, 76, 74, 73, 71, 69, 67, 66, 64, 62, 61, 59, 57, 55, 54, 52, 50, 49, 47, 46, 44, 42, 41, 39, 37, 36, 34, 33,
    31, 30, 28, 26, 25, 23, 22, 20, 19, 17, 16, 14, 13, 11, 10, 8, 7, 5, 4, 2, 1,
];

/// `LL_Code` de `ZSTD_LLcode`.
const LL_CODE: [u8; 64] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 16, 17, 17, 18, 18, 19, 19, 20, 20, 20, 20, 21, 21, 21,
    21, 22, 22, 22, 22, 22, 22, 22, 22, 23, 23, 23, 23, 23, 23, 23, 23, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24,
    24, 24, 24, 24,
];

/// `ML_Code` de `ZSTD_MLcode`.
const ML_CODE: [u8; 128] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
    31, 32, 32, 33, 33, 34, 34, 35, 35, 36, 36, 36, 36, 37, 37, 37, 37, 38, 38, 38, 38, 38, 38, 38, 38, 39, 39, 39, 39,
    39, 39, 39, 39, 40, 40, 40, 40, 40, 40, 40, 40, 40, 40, 40, 40, 40, 40, 40, 40, 41, 41, 41, 41, 41, 41, 41, 41, 41,
    41, 41, 41, 41, 41, 41, 41, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42,
    42, 42, 42, 42, 42, 42, 42, 42, 42, 42,
];

/// `ZSTD_LLcode`.
pub fn ll_code(lit_length: u32) -> u32 {
    if lit_length > 63 {
        highbit32(lit_length) + 19
    } else {
        u32::from(LL_CODE[lit_length as usize])
    }
}

/// `ZSTD_MLcode`: recebe `mlBase`, ou seja, o tamanho da correspondência menos `MINMATCH`.
pub fn ml_code(ml_base: u32) -> u32 {
    if ml_base > 127 {
        highbit32(ml_base) + 36
    } else {
        u32::from(ML_CODE[ml_base as usize])
    }
}

/// `FSE_repeat`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FseRepeat {
    #[default]
    None,
    Check,
    Valid,
}

/// Códigos de literais, offsets e tamanhos de correspondência de cada sequência (`llCode`, `ofCode`, `mlCode`).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct SeqCodes {
    pub ll: Vec<u8>,
    pub of: Vec<u8>,
    pub ml: Vec<u8>,
}

/// `ZSTD_seqToCodes`. Sem o `longLengthPos` do C: lá o código `MaxLL`/`MaxML` é forçado para tamanhos
/// acima de 65535, e o `highbit32` do tamanho inteiro já dá 35 e 52 para qualquer bloco de até 128 KB.
/// Falha (`Generic`) se alguma sequência tiver `off_base` zero ou correspondência menor que `MINMATCH`.
pub fn seq_to_codes(sequences: &[Sequence]) -> FseResult<SeqCodes> {
    let mut codes = SeqCodes::default();
    for seq in sequences {
        if seq.off_base == 0 || seq.match_length < MIN_MATCH {
            return Err(FseError::Generic);
        }
        codes.ll.push(ll_code(seq.lit_length as u32) as u8);
        codes.of.push(highbit32(seq.off_base) as u8);
        codes.ml.push(ml_code((seq.match_length - MIN_MATCH) as u32) as u8);
    }
    Ok(codes)
}

/// `ZSTD_useLowProbCount`.
fn use_low_prob_count(nb_seq: usize) -> bool {
    nb_seq >= 2048
}

/// `ZSTD_NCountCost`: tamanho em bytes do cabeçalho de contagens normalizadas.
fn ncount_cost(count: &[u32], max: u32, nb_seq: usize, fse_log: u32) -> FseResult<usize> {
    let table_log = optimal_table_log(fse_log, nb_seq, max)?;
    let mut norm = [0i16; MAX_SEQ + 1];
    normalize_count(&mut norm, table_log, count, nb_seq, max, use_low_prob_count(nb_seq))?;
    let mut wksp = [0u8; FSE_NCOUNTBOUND];
    write_ncount(&mut wksp, &norm, max, table_log)
}

/// `ZSTD_entropyCost`: custo em bits da entropia de ordem zero.
fn entropy_cost(count: &[u32], max: u32, total: usize) -> usize {
    let mut cost: u32 = 0;
    for &c in &count[..=max as usize] {
        let mut norm = ((256 * u64::from(c)) / total as u64) as u32;
        if c != 0 && norm == 0 {
            norm = 1;
        }
        cost = cost.wrapping_add(c.wrapping_mul(INVERSE_PROBABILITY_LOG_256[norm as usize & 255]));
    }
    (cost >> 8) as usize
}

/// `FSE_bitCost` com `accuracyLog` fixo em 8 pelo único chamador.
fn fse_bit_cost_symbol(ct: &CTable, symbol: usize, accuracy_log: u32) -> Option<u32> {
    let tt = ct.symbol_tt.get(symbol)?;
    let table_log = ct.table_log;
    let min_nb_bits = tt.delta_nb_bits >> 16;
    let threshold = (min_nb_bits + 1) << 16;
    let table_size = 1u32 << table_log;
    let delta_from_threshold = threshold.wrapping_sub(tt.delta_nb_bits.wrapping_add(table_size));
    let normalized = (delta_from_threshold << accuracy_log) >> table_log;
    let bit_multiplier = 1u32 << accuracy_log;
    Some(((min_nb_bits + 1) * bit_multiplier).wrapping_sub(normalized))
}

/// `ZSTD_fseBitCost`: `None` é o `ERROR(GENERIC)` do C (tabela sem o símbolo, ou com probabilidade zero).
pub fn fse_bit_cost(ct: &CTable, count: &[u32], max: u32) -> Option<usize> {
    const ACCURACY_LOG: u32 = 8;
    if ct.max_symbol_value < max {
        return None;
    }
    let mut cost: usize = 0;
    for s in 0..=max as usize {
        if count[s] == 0 {
            continue;
        }
        let bad_cost = (ct.table_log + 1) << ACCURACY_LOG;
        let bit_cost = fse_bit_cost_symbol(ct, s, ACCURACY_LOG)?;
        if bit_cost >= bad_cost {
            return None;
        }
        cost += count[s] as usize * bit_cost as usize;
    }
    Some(cost >> ACCURACY_LOG)
}

/// `ZSTD_crossEntropyCost`.
pub fn cross_entropy_cost(norm: &[i16], accuracy_log: u32, count: &[u32], max: u32) -> FseResult<usize> {
    let shift = 8 - accuracy_log;
    let mut cost: u32 = 0;
    for s in 0..=max as usize {
        let norm_acc = if norm[s] != -1 { norm[s] as u32 } else { 1 };
        let norm256 = (norm_acc << shift) as usize;
        let inverse = INVERSE_PROBABILITY_LOG_256.get(norm256).copied().ok_or(FseError::Generic)?;
        cost = cost.wrapping_add(count[s].wrapping_mul(inverse));
    }
    Ok((cost >> 8) as usize)
}

/// `ZSTD_selectEncodingType`: devolve `SET_BASIC`, `SET_RLE`, `SET_COMPRESSED` ou `SET_REPEAT`.
/// `strategy` é o número do libzstd (`strategy_number`); `prev_ctable` é a tabela do bloco anterior
/// (só consultada quando `repeat_mode` não é `None`).
#[allow(clippy::too_many_arguments)]
pub fn select_encoding_type(
    repeat_mode: &mut FseRepeat,
    count: &[u32],
    max: u32,
    most_frequent: usize,
    nb_seq: usize,
    fse_log: u32,
    prev_ctable: Option<&CTable>,
    default_norm: &[i16],
    default_norm_log: u32,
    is_default_allowed: bool,
    strategy: u32,
) -> FseResult<u32> {
    if most_frequent == nb_seq {
        *repeat_mode = FseRepeat::None;
        // com no máximo 2 símbolos o basic (5 a 6 bits por símbolo) ganha do RLE (1 byte)
        return Ok(if is_default_allowed && nb_seq <= 2 { SET_BASIC } else { SET_RLE });
    }
    if strategy < STRATEGY_LAZY {
        if is_default_allowed {
            let static_fse_nb_seq_max = 1000;
            let mult = (10 - strategy) as usize;
            let dynamic_fse_nb_seq_min = ((1usize << default_norm_log) * mult) >> 3;
            if *repeat_mode == FseRepeat::Valid && nb_seq < static_fse_nb_seq_max {
                return Ok(SET_REPEAT);
            }
            if nb_seq < dynamic_fse_nb_seq_min || most_frequent < (nb_seq >> (default_norm_log - 1)) {
                *repeat_mode = FseRepeat::None;
                return Ok(SET_BASIC);
            }
        }
    } else {
        let basic_cost =
            if is_default_allowed { cross_entropy_cost(default_norm, default_norm_log, count, max)? } else { usize::MAX };
        let repeat_cost = match (*repeat_mode != FseRepeat::None, prev_ctable) {
            (true, Some(prev)) => fse_bit_cost(prev, count, max).unwrap_or(usize::MAX),
            _ => usize::MAX,
        };
        let compressed_cost = (ncount_cost(count, max, nb_seq, fse_log)? << 3) + entropy_cost(count, max, nb_seq);
        if basic_cost <= repeat_cost && basic_cost <= compressed_cost {
            *repeat_mode = FseRepeat::None;
            return Ok(SET_BASIC);
        }
        if repeat_cost <= compressed_cost {
            return Ok(SET_REPEAT);
        }
    }
    *repeat_mode = FseRepeat::Check;
    Ok(SET_COMPRESSED)
}

/// Resultado de `ZSTD_buildCTable`: os bytes que entram no fluxo (cabeçalho de contagens ou o símbolo RLE)
/// e a tabela de compressão resultante.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuiltTable {
    pub header: Vec<u8>,
    pub ctable: CTable,
}

/// `ZSTD_buildCTable`. `count` é alterado no modo comprimido (o último símbolo perde uma ocorrência),
/// como no C. `dst_capacity` é o espaço que sobra no fluxo para o cabeçalho.
#[allow(clippy::too_many_arguments)]
pub fn build_sequence_ctable(
    dst_capacity: usize,
    fse_log: u32,
    encoding_type: u32,
    count: &mut [u32],
    max: u32,
    code_table: &[u8],
    default_norm: &[i16],
    default_norm_log: u32,
    default_max: u32,
    prev_ctable: Option<&CTable>,
) -> FseResult<BuiltTable> {
    let nb_seq = code_table.len();
    match encoding_type {
        SET_RLE => {
            if dst_capacity == 0 {
                return Err(FseError::DstSizeTooSmall);
            }
            let first = *code_table.first().ok_or(FseError::Generic)?;
            Ok(BuiltTable { header: vec![first], ctable: build_ctable_rle(max as u8) })
        }
        SET_REPEAT => Ok(BuiltTable { header: Vec::new(), ctable: prev_ctable.ok_or(FseError::Generic)?.clone() }),
        SET_BASIC => {
            Ok(BuiltTable { header: Vec::new(), ctable: build_ctable(default_norm, default_max, default_norm_log)? })
        }
        SET_COMPRESSED => {
            let table_log = optimal_table_log(fse_log, nb_seq, max)?;
            let last = *code_table.last().ok_or(FseError::Generic)? as usize;
            let mut nb_seq_1 = nb_seq;
            if count[last] > 1 {
                count[last] -= 1;
                nb_seq_1 -= 1;
            }
            if nb_seq_1 <= 1 {
                return Err(FseError::Generic);
            }
            let mut norm = [0i16; MAX_SEQ + 1];
            normalize_count(&mut norm, table_log, count, nb_seq_1, max, use_low_prob_count(nb_seq_1))?;
            let mut out = vec![0u8; dst_capacity.min(FSE_NCOUNTBOUND)];
            let ncount_size = write_ncount(&mut out, &norm, max, table_log)?;
            out.truncate(ncount_size);
            Ok(BuiltTable { header: out, ctable: build_ctable(&norm, max, table_log)? })
        }
        _ => Err(FseError::Generic),
    }
}

/// `ZSTD_encodeSequences`: fluxo de bits reverso com os três estados FSE. Devolve os bytes do fluxo, ou
/// `DstSizeTooSmall` se não couberem em `dst_capacity`.
pub fn encode_sequences(
    dst_capacity: usize,
    ml_ctable: &CTable,
    of_ctable: &CTable,
    ll_ctable: &CTable,
    codes: &SeqCodes,
    sequences: &[Sequence],
) -> FseResult<Vec<u8>> {
    let nb_seq = sequences.len();
    if nb_seq == 0 || codes.ll.len() != nb_seq || codes.of.len() != nb_seq || codes.ml.len() != nb_seq {
        return Err(FseError::Generic);
    }
    let mut stream = BitCStream::new(dst_capacity).map_err(|_| FseError::DstSizeTooSmall)?;
    let last = nb_seq - 1;
    let ml_base = |seq: &Sequence| (seq.match_length - MIN_MATCH) as u64;
    let mut state_ml = CState::with_symbol(ml_ctable, u32::from(codes.ml[last]))?;
    let mut state_of = CState::with_symbol(of_ctable, u32::from(codes.of[last]))?;
    let mut state_ll = CState::with_symbol(ll_ctable, u32::from(codes.ll[last]))?;
    stream.add_bits(sequences[last].lit_length as u64, u32::from(LL_BITS[codes.ll[last] as usize]));
    stream.add_bits(ml_base(&sequences[last]), u32::from(ML_BITS[codes.ml[last] as usize]));
    stream.add_bits(u64::from(sequences[last].off_base), u32::from(codes.of[last]));
    stream.flush_bits();
    for n in (0..last).rev() {
        let ll_bits = u32::from(LL_BITS[codes.ll[n] as usize]);
        let of_bits = u32::from(codes.of[n]);
        let ml_bits = u32::from(ML_BITS[codes.ml[n] as usize]);
        state_of.encode_symbol(&mut stream, u32::from(codes.of[n]))?;
        state_ml.encode_symbol(&mut stream, u32::from(codes.ml[n]))?;
        state_ll.encode_symbol(&mut stream, u32::from(codes.ll[n]))?;
        // 64-7-(LLFSELog+MLFSELog+OffFSELog) = 31
        if of_bits + ml_bits + ll_bits >= 64 - 7 - (LL_FSE_LOG + ML_FSE_LOG + OFF_FSE_LOG) {
            stream.flush_bits();
        }
        stream.add_bits(sequences[n].lit_length as u64, ll_bits);
        stream.add_bits(ml_base(&sequences[n]), ml_bits);
        if of_bits + ml_bits + ll_bits > 56 {
            stream.flush_bits();
        }
        stream.add_bits(u64::from(sequences[n].off_base), of_bits);
        stream.flush_bits();
    }
    state_ml.flush(&mut stream);
    state_of.flush(&mut stream);
    state_ll.flush(&mut stream);
    let size = stream.close()?;
    Ok(stream.bytes(size).to_vec())
}

/// Tabelas FSE de um bloco (`ZSTD_fseCTables_t`); `None` até a primeira construção.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FseCTables {
    pub litlength: Option<CTable>,
    pub offcode: Option<CTable>,
    pub matchlength: Option<CTable>,
    pub litlength_repeat: FseRepeat,
    pub offcode_repeat: FseRepeat,
    pub matchlength_repeat: FseRepeat,
}

/// `ZSTD_symbolEncodingTypeStats_t` mais os códigos calculados no caminho.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymbolEncodingStats {
    pub ll_type: u32,
    pub of_type: u32,
    pub ml_type: u32,
    /// Os três cabeçalhos concatenados (LL, offsets, ML), o `size` do C.
    pub header: Vec<u8>,
    pub last_count_size: usize,
    pub codes: SeqCodes,
}

/// Uma das três rodadas de `ZSTD_buildSequencesStatistics`: histograma, escolha do modo e construção.
#[allow(clippy::too_many_arguments)]
fn build_one_table(
    codes: &[u8],
    max_symbol: u32,
    fse_log: u32,
    default_norm: &[i16],
    default_norm_log: u32,
    default_max: u32,
    default_allowed_for: impl Fn(u32) -> bool,
    prev: Option<&CTable>,
    prev_repeat: FseRepeat,
    strategy: u32,
    dst_capacity: usize,
) -> FseResult<(u32, BuiltTable, FseRepeat)> {
    let mut count = [0u32; MAX_SEQ + 1];
    let (most_frequent, max) = hist_count(&mut count, max_symbol, codes)?;
    let mut repeat = prev_repeat;
    let encoding_type = select_encoding_type(
        &mut repeat,
        &count,
        max,
        most_frequent as usize,
        codes.len(),
        fse_log,
        prev,
        default_norm,
        default_norm_log,
        default_allowed_for(max),
        strategy,
    )?;
    let built = build_sequence_ctable(
        dst_capacity,
        fse_log,
        encoding_type,
        &mut count,
        max,
        codes,
        default_norm,
        default_norm_log,
        default_max,
        prev,
    )?;
    Ok((encoding_type, built, repeat))
}

/// `ZSTD_buildSequencesStatistics`: escolhe o modo das três tabelas, escreve os cabeçalhos e atualiza
/// `next` (tabelas e modos de repetição). Exige pelo menos uma sequência.
pub fn build_sequences_statistics(
    sequences: &[Sequence],
    prev: &FseCTables,
    next: &mut FseCTables,
    dst_capacity: usize,
    strategy: u32,
) -> FseResult<SymbolEncodingStats> {
    if sequences.is_empty() {
        return Err(FseError::Generic);
    }
    let codes = seq_to_codes(sequences)?;
    let mut header = Vec::new();
    let mut last_count_size = 0;
    let remaining = |header: &[u8]| dst_capacity.saturating_sub(header.len());

    let (ll_type, built, repeat) = build_one_table(
        &codes.ll,
        MAX_LL,
        LL_FSE_LOG,
        &LL_DEFAULT_NORM,
        LL_DEFAULT_NORM_LOG,
        MAX_LL,
        |_| true,
        prev.litlength.as_ref(),
        prev.litlength_repeat,
        strategy,
        remaining(&header),
    )?;
    next.litlength_repeat = repeat;
    next.litlength = Some(built.ctable);
    if ll_type == SET_COMPRESSED {
        last_count_size = built.header.len();
    }
    header.extend_from_slice(&built.header);

    // O offset só aceita a tabela básica se o maior código couber nela (`max <= DefaultMaxOff`).
    let (of_type, built, repeat) = build_one_table(
        &codes.of,
        MAX_OFF,
        OFF_FSE_LOG,
        &OF_DEFAULT_NORM,
        OF_DEFAULT_NORM_LOG,
        DEFAULT_MAX_OFF,
        |max| max <= DEFAULT_MAX_OFF,
        prev.offcode.as_ref(),
        prev.offcode_repeat,
        strategy,
        remaining(&header),
    )?;
    next.offcode_repeat = repeat;
    next.offcode = Some(built.ctable);
    if of_type == SET_COMPRESSED {
        last_count_size = built.header.len();
    }
    header.extend_from_slice(&built.header);

    let (ml_type, built, repeat) = build_one_table(
        &codes.ml,
        MAX_ML,
        ML_FSE_LOG,
        &ML_DEFAULT_NORM,
        ML_DEFAULT_NORM_LOG,
        MAX_ML,
        |_| true,
        prev.matchlength.as_ref(),
        prev.matchlength_repeat,
        strategy,
        remaining(&header),
    )?;
    next.matchlength_repeat = repeat;
    next.matchlength = Some(built.ctable);
    if ml_type == SET_COMPRESSED {
        last_count_size = built.header.len();
    }
    header.extend_from_slice(&built.header);

    Ok(SymbolEncodingStats { ll_type, of_type, ml_type, header, last_count_size, codes })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seq(lit_length: usize, off_base: u32, match_length: usize) -> Sequence {
        Sequence { lit_length, off_base, match_length }
    }

    #[test]
    fn default_norms_sum_to_table_size() {
        assert_eq!(LL_DEFAULT_NORM.iter().map(|&n| if n == -1 { 1 } else { n as i32 }).sum::<i32>(), 64);
        assert_eq!(ML_DEFAULT_NORM.iter().map(|&n| if n == -1 { 1 } else { n as i32 }).sum::<i32>(), 64);
        assert_eq!(OF_DEFAULT_NORM.iter().map(|&n| if n == -1 { 1 } else { n as i32 }).sum::<i32>(), 32);
    }

    #[test]
    fn length_codes() {
        assert_eq!([ll_code(0), ll_code(15), ll_code(16), ll_code(17), ll_code(18)], [0, 15, 16, 16, 17]);
        // LL_Code[40] = 23; 64 passa a ZSTD_highbit32(64) + 19 = 25
        assert_eq!([ll_code(40), ll_code(63), ll_code(64), ll_code(65536)], [23, 24, 25, 35]);
        assert_eq!([ml_code(0), ml_code(31), ml_code(32), ml_code(33), ml_code(127)], [0, 31, 32, 32, 42]);
        // 128: highbit 7 + 36 = 43; 65536: 16 + 36 = 52
        assert_eq!([ml_code(128), ml_code(65536)], [43, 52]);
    }

    #[test]
    fn seq_to_codes_vectors() {
        // offBase 5 => ofCode 2; mlBase 1 => código 1; litLength 2 => código 2
        let codes = seq_to_codes(&[seq(2, 5, 4), seq(100, 1, 3)]).unwrap();
        assert_eq!(codes.ll, vec![2, 25]);
        assert_eq!(codes.of, vec![2, 0]);
        assert_eq!(codes.ml, vec![1, 0]);
        assert!(seq_to_codes(&[seq(0, 0, 3)]).is_err());
        assert!(seq_to_codes(&[seq(0, 1, 2)]).is_err());
    }

    #[test]
    fn costs() {
        // duas ocorrências de dois símbolos: norm 128 cada, kInverse[128] = 256, 4 * 256 >> 8 = 4 bits
        assert_eq!(entropy_cost(&[2, 2], 1, 4), 4);
        // norm {32, 32} em accuracyLog 6: norm256 = 128, 4 * 256 >> 8 = 4
        assert_eq!(cross_entropy_cost(&[32, 32], 6, &[3, 1], 1), Ok(4));
        // -1 conta como 1: norm256 = 4, kInverse[4] = 1536; 1 * 1536 >> 8 = 6
        assert_eq!(cross_entropy_cost(&[-1], 6, &[1], 0), Ok(6));
        // tabela RLE (maxSymbolValue 0): pedir o símbolo 1 é erro, e o símbolo 0 estoura o badCost
        // (deltaNbBits 0 dá minNbBits 0; o custo interpolado passa de 256)
        let rle = build_ctable_rle(0);
        assert_eq!(fse_bit_cost(&rle, &[1, 1], 1), None);
        assert_eq!(fse_bit_cost(&rle, &[1], 0), None);
        // símbolo ausente não conta
        assert_eq!(fse_bit_cost(&rle, &[0], 0), Some(0));
    }

    #[test]
    fn select_rle_and_basic_for_single_symbol() {
        let count = [3u32, 0];
        let mut repeat = FseRepeat::Valid;
        let t = select_encoding_type(&mut repeat, &count, 0, 3, 3, 9, None, &LL_DEFAULT_NORM, 6, true, 2);
        assert_eq!((t, repeat), (Ok(SET_RLE), FseRepeat::None));
        let t = select_encoding_type(&mut repeat, &[2], 0, 2, 2, 9, None, &LL_DEFAULT_NORM, 6, true, 2);
        assert_eq!(t, Ok(SET_BASIC));
        let t = select_encoding_type(&mut repeat, &[2], 0, 2, 2, 9, None, &LL_DEFAULT_NORM, 6, false, 2);
        assert_eq!(t, Ok(SET_RLE));
    }

    #[test]
    fn select_by_heuristic_for_dfast() {
        let count = [5u32, 5];
        // repetição válida com menos de 1000 sequências
        let mut repeat = FseRepeat::Valid;
        let t = select_encoding_type(&mut repeat, &count, 1, 5, 10, 9, None, &LL_DEFAULT_NORM, 6, true, 2);
        assert_eq!((t, repeat), (Ok(SET_REPEAT), FseRepeat::Valid));
        // dynamicFse_nbSeq_min = (64 * 8) >> 3 = 64; 10 < 64 => basic
        let mut repeat = FseRepeat::Check;
        let t = select_encoding_type(&mut repeat, &count, 1, 5, 10, 9, None, &LL_DEFAULT_NORM, 6, true, 2);
        assert_eq!((t, repeat), (Ok(SET_BASIC), FseRepeat::None));
        // 100 sequências, mostFrequent 5 >= 100 >> 5 = 3 => comprimido
        let mut repeat = FseRepeat::None;
        let t = select_encoding_type(&mut repeat, &count, 1, 5, 100, 9, None, &LL_DEFAULT_NORM, 6, true, 2);
        assert_eq!((t, repeat), (Ok(SET_COMPRESSED), FseRepeat::Check));
        // mostFrequent 2 < 3 => basic
        let mut repeat = FseRepeat::None;
        let t = select_encoding_type(&mut repeat, &count, 1, 2, 100, 9, None, &LL_DEFAULT_NORM, 6, true, 2);
        assert_eq!(t, Ok(SET_BASIC));
    }

    #[test]
    fn build_rle_and_basic_tables() {
        let mut count = [0u32; 53];
        let rle =
            build_sequence_ctable(4, 9, SET_RLE, &mut count, 2, &[2, 2, 2], &LL_DEFAULT_NORM, 6, MAX_LL, None).unwrap();
        assert_eq!(rle.header, vec![2]);
        assert_eq!(rle.ctable, build_ctable_rle(2));
        assert_eq!(
            build_sequence_ctable(0, 9, SET_RLE, &mut count, 2, &[2], &LL_DEFAULT_NORM, 6, MAX_LL, None),
            Err(FseError::DstSizeTooSmall)
        );
        let basic =
            build_sequence_ctable(4, 9, SET_BASIC, &mut count, 2, &[2], &OF_DEFAULT_NORM, 5, DEFAULT_MAX_OFF, None)
                .unwrap();
        assert!(basic.header.is_empty());
        assert_eq!(basic.ctable.table_log, 5);
        assert_eq!(basic.ctable.max_symbol_value, DEFAULT_MAX_OFF);
        let repeat =
            build_sequence_ctable(4, 9, SET_REPEAT, &mut count, 2, &[2], &OF_DEFAULT_NORM, 5, 28, Some(&basic.ctable))
                .unwrap();
        assert_eq!(repeat.ctable, basic.ctable);
    }

    #[test]
    fn encode_with_rle_tables() {
        let ll = build_ctable_rle(16);
        let of = build_ctable_rle(0);
        let ml = build_ctable_rle(2);
        // litLength 16 e 17 => código 16 com 1 bit; mlBase 2 (código 2, 0 bits); offBase 1 (código 0)
        let one = [seq(17, 1, 5)];
        let codes = seq_to_codes(&one).unwrap();
        // estados RLE não emitem bits; sobra o bit 1 do litLength e a marca de fim: 0b11
        assert_eq!(encode_sequences(16, &ml, &of, &ll, &codes, &one).unwrap(), vec![0x03]);
        let two = [seq(16, 1, 5), seq(17, 1, 5)];
        let codes = seq_to_codes(&two).unwrap();
        // a última sequência entra primeiro (bit 1), depois a primeira (bit 0), depois a marca: 0b101
        assert_eq!(encode_sequences(16, &ml, &of, &ll, &codes, &two).unwrap(), vec![0x05]);
        assert_eq!(encode_sequences(8, &ml, &of, &ll, &codes, &two), Err(FseError::DstSizeTooSmall));
        assert_eq!(encode_sequences(16, &ml, &of, &ll, &SeqCodes::default(), &[]), Err(FseError::Generic));
    }

    #[test]
    fn statistics_for_identical_sequences_are_rle() {
        let seqs = [seq(2, 5, 4), seq(2, 5, 4), seq(2, 5, 4)];
        let prev = FseCTables::default();
        let mut next = FseCTables::default();
        let stats = build_sequences_statistics(&seqs, &prev, &mut next, 64, 2).unwrap();
        // LL código 2, offset ofCode 2, ML mlBase 1 => código 1; nbSeq 3 > 2 => RLE nas três
        assert_eq!((stats.ll_type, stats.of_type, stats.ml_type), (SET_RLE, SET_RLE, SET_RLE));
        assert_eq!(stats.header, vec![2, 2, 1]);
        assert_eq!(stats.last_count_size, 0);
        assert_eq!(next.litlength, Some(build_ctable_rle(2)));
        assert_eq!(next.offcode_repeat, FseRepeat::None);
        assert_eq!(build_sequences_statistics(&[], &prev, &mut next, 64, 2), Err(FseError::Generic));
    }

    #[test]
    fn statistics_for_two_sequences_use_basic() {
        let seqs = [seq(2, 5, 4), seq(2, 5, 4)];
        let mut next = FseCTables::default();
        let stats = build_sequences_statistics(&seqs, &FseCTables::default(), &mut next, 64, 2).unwrap();
        assert_eq!((stats.ll_type, stats.of_type, stats.ml_type), (SET_BASIC, SET_BASIC, SET_BASIC));
        assert!(stats.header.is_empty());
        assert_eq!(next.offcode.as_ref().map(|c| c.table_log), Some(5));
    }
}
