//! Laço de blocos do quadro: `ZSTD_compress_frameChunk`, `ZSTD_compressBlock_internal` e a função pública
//! compressão de nível 3 (dfast, sem checksum), libzstd 1.5.7.
//!
//! O `Compressor` guarda o estado que atravessa as chamadas de `ZSTD_compressContinue` (tabelas de hash,
//! repetições, entropia, contadores de `savings`). Quem o dirige é `stream::ZstdEncoder` (um `frameChunk`
//! por 128 KiB de entrada); `compress` e `compress_declaring_size` também passam por ele.
//! O divisor de blocos em tempo de compressão (`ZSTD_blockSplitterEnabled`) só liga a partir de `btopt`, então
//! não participa; o pré-divisor (`ZSTD_optimalBlockSize`) participa e está em `presplit`.

use super::entropy::{entropy_compress_seq_store, EntropyTables};
use super::frame::{rle_byte, write_block_header, write_frame_header, write_raw_block, write_rle_block, BlockType, BLOCK_SIZE_MAX};
use super::sequences::FseRepeat;
use super::match_finder::DoubleFast;
use super::params::{CParams, Strategy};
use super::presplit::split_block;
use super::seq_store::SeqStore;

/// `MIN_CBLOCK_SIZE + ZSTD_blockHeaderSize + 1 + 1`: abaixo disso nem se tenta comprimir.
const MIN_SRC_SIZE_TO_COMPRESS: usize = 2 + 3 + 1 + 1;
/// `rleMaxLength`.
const RLE_MAX_LENGTH: usize = 25;

/// `ZSTD_compressBound`.
fn compress_bound(src_size: usize) -> usize {
    src_size + (src_size >> 8) + if src_size < BLOCK_SIZE_MAX { (BLOCK_SIZE_MAX - src_size) >> 11 } else { 0 }
}

/// Como um bloco sai no quadro.
enum Body {
    Raw,
    Rle(u8),
    Compressed(Vec<u8>),
}

pub(super) struct Compressor {
    strategy: Strategy,
    window_log: u32,
    block_size_max: usize,
    /// Tamanho do conteúdo declarado no cabeçalho (`None` quando desconhecido).
    pledged: Option<u64>,
    matcher: DoubleFast,
    seq_store: SeqStore,
    /// `prevCBlock` e `nextCBlock`: tabelas de entropia e repetições.
    prev: EntropyTables,
    next: EntropyTables,
    prev_rep: [u32; 3],
    is_first_block: bool,
    /// O cabeçalho do quadro já saiu (`stage != ZSTDcs_init`).
    header_written: bool,
    /// O último bloco já saiu (`stage == ZSTDcs_ending`).
    ended: bool,
    /// `consumedSrcSize` e `producedCSize`: a base do `savings` do pré-divisor.
    consumed: u64,
    produced: u64,
}

impl Compressor {
    pub(super) fn new(cparams: &CParams, pledged: Option<u64>) -> Self {
        Compressor {
            strategy: cparams.strategy,
            window_log: cparams.window_log,
            block_size_max: BLOCK_SIZE_MAX.min(1usize << cparams.window_log),
            pledged,
            matcher: DoubleFast::new(cparams),
            seq_store: SeqStore::default(),
            prev: EntropyTables::default(),
            next: EntropyTables::default(),
            prev_rep: [1, 4, 8],
            is_first_block: true,
            header_written: false,
            ended: false,
            consumed: 0,
            produced: 0,
        }
    }

    /// `ZSTD_optimalBlockSize` para dfast (nível de pré-divisão 1 da tabela `splitLevels`); `chunk` é o que
    /// resta do `frameChunk` a partir do bloco.
    fn optimal_block_size(&self, chunk: &[u8], savings: i64) -> usize {
        let remaining = chunk.len();
        if remaining < BLOCK_SIZE_MAX || self.block_size_max < BLOCK_SIZE_MAX {
            return remaining.min(self.block_size_max);
        }
        if savings < 3 {
            return BLOCK_SIZE_MAX;
        }
        split_block(&chunk[..BLOCK_SIZE_MAX])
    }

    /// `ZSTD_compressBlock_internal` (com `frame == 1`) mais a decisão raw/RLE/comprimido do chamador.
    /// `data` é o histórico visível até o fim do bloco.
    fn compress_block(&mut self, data: &[u8], start: usize, end: usize) -> Body {
        let src_size = end - start;
        let mut compressed = None;
        if src_size >= MIN_SRC_SIZE_TO_COMPRESS {
            self.seq_store.clear();
            self.matcher.rep = self.prev_rep;
            let last_literals = self.matcher.compress_block(data, start, end, &mut self.seq_store);
            self.seq_store.literals.extend_from_slice(&data[end - last_literals..end]);
            // Erro de entropia (inalcançável com blocos válidos) cai no bloco raw, que sempre decodifica.
            compressed = entropy_compress_seq_store(
                &self.seq_store,
                &self.prev,
                &mut self.next,
                self.strategy,
                compress_bound(src_size),
                src_size,
            )
            .ok()
            .flatten();

            let c_size = compressed.as_ref().map_or(0, Vec::len);
            if !self.is_first_block && c_size < RLE_MAX_LENGTH {
                if let Some(byte) = rle_byte(&data[start..end]) {
                    // cSize == 1: bloco RLE, sem confirmar repetições nem tabelas.
                    self.finish_block();
                    return Body::Rle(byte);
                }
            }
            if compressed.is_some() {
                // `ZSTD_blockState_confirmRepcodesAndEntropyTables`
                std::mem::swap(&mut self.prev, &mut self.next);
                self.prev_rep = self.matcher.rep;
            }
        }
        self.finish_block();
        compressed.map_or(Body::Raw, Body::Compressed)
    }

    /// Fim comum de `ZSTD_compressBlock_internal`: o modo de repetição `valid` dos offsets volta a `check`.
    fn finish_block(&mut self) {
        if self.prev.fse.offcode_repeat == FseRepeat::Valid {
            self.prev.fse.offcode_repeat = FseRepeat::Check;
        }
    }

    /// `ZSTD_compress_frameChunk` sobre `data[from..]`, com `lastFrameChunk == last_chunk`.
    fn compress_frame_chunk(&mut self, data: &[u8], from: usize, last_chunk: bool, out: &mut Vec<u8>) {
        let total = data.len();
        let mut pos = from;
        let mut savings = self.consumed as i64 - self.produced as i64;
        while pos < total {
            let block_size = self.optimal_block_size(&data[pos..], savings);
            let last = last_chunk && pos + block_size == total;
            let before = out.len();
            match self.compress_block(data, pos, pos + block_size) {
                Body::Raw => write_raw_block(out, &data[pos..pos + block_size], last),
                Body::Rle(byte) => write_rle_block(out, byte, block_size, last),
                Body::Compressed(body) => {
                    write_block_header(out, last, BlockType::Compressed, body.len());
                    out.extend_from_slice(&body);
                }
            }
            savings += block_size as i64 - (out.len() - before) as i64;
            pos += block_size;
            self.is_first_block = false;
            self.ended |= last;
        }
    }

    /// `ZSTD_writeFrameHeader`, uma só vez por quadro.
    fn write_header_once(&mut self, out: &mut Vec<u8>) {
        if !self.header_written {
            write_frame_header(out, self.pledged, self.window_log);
            self.header_written = true;
        }
    }

    /// `ZSTD_compressContinue_internal` em modo de quadro: cabeçalho (se ainda não saiu) e o `frameChunk` de
    /// `data[from..]`; `data` é o histórico inteiro visível até o fim do trecho.
    pub(super) fn compress_chunk(&mut self, data: &[u8], from: usize, last_chunk: bool, out: &mut Vec<u8>) {
        let before = out.len();
        self.write_header_once(out);
        self.compress_frame_chunk(data, from, last_chunk, out);
        self.consumed += (data.len() - from) as u64;
        self.produced += (out.len() - before) as u64;
    }

    /// `ZSTD_writeEpilogue`: o cabeçalho de um quadro sem entrada e o bloco raw vazio e último de um quadro
    /// cujo último bloco ainda não saiu.
    pub(super) fn write_epilogue(&mut self, out: &mut Vec<u8>) {
        self.write_header_once(out);
        if !self.ended {
            write_raw_block(out, &[], true);
            self.ended = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::runtime::zstd::{compress, compress_declaring_size};
    use crate::runtime::zstd::decompress::decode_all as decode;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Vetores medidos no bun 1.4.2 (`wip/notes/compression-brotli-zstd.md`).
    #[test]
    fn matches_bun_vectors() {
        assert_eq!(hex(&compress(b"")), "28b52ffd0058010000");
        assert_eq!(hex(&compress_declaring_size(b"")), "28b52ffd2000010000");
        assert_eq!(hex(&compress(b"a")), "28b52ffd005809000061");
        assert_eq!(
            hex(&compress(b"hello world hello world hello world")),
            "28b52ffd00589500006068656c6c6f20776f726c64200100af4b12"
        );
        assert_eq!(hex(&compress(&[0u8; 1000])), "28b52ffd00584d00001000000100e32b8005");
    }

    #[test]
    fn round_trips_multi_block_inputs() {
        let mut state: u32 = 1;
        let mut next = move || {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (state >> 24) as u8
        };
        let words: [&[u8]; 4] = [b"alpha ", b"beta ", b"gamma ", b"delta\n"];
        let mut text = Vec::new();
        while text.len() < 400_000 {
            text.extend_from_slice(words[usize::from(next() & 3)]);
        }
        let noise: Vec<u8> = (0..300_000).map(|_| next()).collect();
        for input in [text, noise, vec![7u8; 500_000]] {
            assert_eq!(decode(&compress(&input)), input);
        }
    }
}
