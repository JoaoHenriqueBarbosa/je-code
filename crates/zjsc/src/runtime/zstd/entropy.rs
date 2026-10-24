//! Compressão de entropia de um bloco: `ZSTD_entropyCompressSeqStore_internal` e `ZSTD_entropyCompressSeqStore`
//! (zstd_compress.c, libzstd 1.5.7).

use super::fse::{FseError, FseResult};
use super::huf::{literals_encoder, HufRepeat, HufState};
use super::literals::{compress_literals, min_gain, strategy_number, TableState};
use super::params::Strategy;
use super::seq_store::SeqStore;
use super::sequences::{build_sequences_statistics, encode_sequences, FseCTables};

/// `SUSPECT_UNCOMPRESSIBLE_LITERAL_RATIO`.
const SUSPECT_UNCOMPRESSIBLE_LITERAL_RATIO: usize = 20;
/// `LONGNBSEQ`.
const LONG_NB_SEQ: usize = 0x7F00;

/// `ZSTD_entropyCTables_t`: a tabela Huffman dos literais e as três tabelas FSE das sequências.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EntropyTables {
    pub huf: HufState,
    pub fse: FseCTables,
}

/// `ZSTD_entropyCompressSeqStore_internal`. `Ok(None)` equivale ao retorno 0 do C (a regra do bug do
/// decodificador <= 1.3.4: o bloco sai sem compressão).
fn entropy_compress_internal(
    seq_store: &SeqStore,
    prev: &EntropyTables,
    next: &mut EntropyTables,
    strategy: Strategy,
    dst_capacity: usize,
) -> FseResult<Option<Vec<u8>>> {
    let nb_seq = seq_store.sequences.len();
    let literals = &seq_store.literals;
    let suspect_uncompressible = nb_seq == 0 || literals.len() / nb_seq >= SUSPECT_UNCOMPRESSIBLE_LITERAL_RATIO;

    // Cópia de trabalho do `nextHuf`: só vira o próximo estado quando a tabela é nova.
    let mut working = prev.huf.clone();
    let section = compress_literals(
        literals,
        strategy,
        prev.huf.repeat == HufRepeat::Valid,
        suspect_uncompressible,
        literals_encoder(literals, dst_capacity, &mut working),
    );
    next.huf = match section.table {
        TableState::NewlyBuilt => {
            working.repeat = HufRepeat::Check;
            working
        }
        TableState::Unchanged => prev.huf.clone(),
    };
    let mut out = section.bytes;

    // Cabeçalho de sequências: nbSeq em 1 a 3 bytes.
    if nb_seq < 128 {
        out.push(nb_seq as u8);
    } else if nb_seq < LONG_NB_SEQ {
        out.push(((nb_seq >> 8) + 0x80) as u8);
        out.push(nb_seq as u8);
    } else {
        out.push(0xFF);
        out.extend_from_slice(&((nb_seq - LONG_NB_SEQ) as u16).to_le_bytes());
    }
    if nb_seq == 0 {
        // As tabelas antigas valem como se fossem repetidas.
        next.fse = prev.fse.clone();
        return Ok(Some(out));
    }

    let stats = build_sequences_statistics(
        &seq_store.sequences,
        &prev.fse,
        &mut next.fse,
        dst_capacity.saturating_sub(out.len() + 1),
        strategy_number(strategy),
    )?;
    out.push(((stats.ll_type << 6) + (stats.of_type << 4) + (stats.ml_type << 2)) as u8);
    out.extend_from_slice(&stats.header);

    let (ll, of, ml) = (
        next.fse.litlength.as_ref().ok_or(FseError::Generic)?,
        next.fse.offcode.as_ref().ok_or(FseError::Generic)?,
        next.fse.matchlength.as_ref().ok_or(FseError::Generic)?,
    );
    let bitstream =
        encode_sequences(dst_capacity.saturating_sub(out.len()), ml, of, ll, &stats.codes, &seq_store.sequences)?;
    // zstd <= 1.3.4 acusa corrupção quando `FSE_readNCount` recebe menos de 4 bytes: emite bloco incompressível.
    if stats.last_count_size != 0 && stats.last_count_size + bitstream.len() < 4 {
        return Ok(None);
    }
    out.extend_from_slice(&bitstream);
    Ok(Some(out))
}

/// `ZSTD_entropyCompressSeqStore`: `Ok(None)` quando o bloco não compensa (devolve 0 no C) e deve sair
/// como bloco raw; `src_size` é o tamanho do bloco de entrada.
pub fn entropy_compress_seq_store(
    seq_store: &SeqStore,
    prev: &EntropyTables,
    next: &mut EntropyTables,
    strategy: Strategy,
    dst_capacity: usize,
    src_size: usize,
) -> FseResult<Option<Vec<u8>>> {
    let out = match entropy_compress_internal(seq_store, prev, next, strategy, dst_capacity) {
        // Há espaço para um bloco raw (src_size <= dst_capacity), então falta de espaço vira "sem compressão".
        Err(FseError::DstSizeTooSmall) => return Ok(None),
        other => other?,
    };
    let Some(out) = out else { return Ok(None) };
    if out.len() >= src_size.saturating_sub(min_gain(src_size, strategy)) {
        return Ok(None);
    }
    Ok(Some(out))
}
