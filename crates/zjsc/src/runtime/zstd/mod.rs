//! Compressor zstd, porte fiel do libzstd 1.5.7 (`lib/compress`) em Rust seguro.
//!
//! Alvo: reproduzir bit a bit o que `CompressionStream('zstd')` do bun 1.4.2 produz (nível 3, estratégia
//! dfast, sem checksum; vetores em `wip/notes/compression-brotli-zstd.md`).
//!
//! Estado da fatia 1:
//! - `params`: seleção de parâmetros do `clevels.h` por tamanho de entrada e `ZSTD_adjustCParams`;
//! - `frame`: cabeçalho do quadro (`ZSTD_writeFrameHeader`) e cabeçalho de bloco, blocos raw e RLE;
//! - `seq_store`: o `SeqStore_t` (literais e sequências) com `ZSTD_storeSeq`;
//! - `match_finder`: `ZSTD_compressBlock_doubleFast_noDict_generic`.
//!
//! Fatia 2 (sobre `huf`, `fse`, `literals` e `sequences`):
//! - `entropy`: `ZSTD_entropyCompressSeqStore` (literais, nbSeq, tipos, cabeçalhos e fluxo de sequências);
//! - `presplit`: o pré-divisor de blocos de `ZSTD_optimalBlockSize`;
//! - `block`: `ZSTD_compressBlock_internal`, `ZSTD_compress_frameChunk` e o `Compressor` que atravessa os trechos.

pub mod block;
pub mod entropy;
pub mod decompress;
pub mod frame;
pub mod fse;
pub mod huf;
pub mod literals;
pub mod match_finder;
pub mod pacing;
pub mod params;
pub mod presplit;
pub mod seq_store;
pub mod sequences;
pub mod stream;

pub use pacing::{reset_scratch, PacedEncoder, DEFAULT_HIGH_WATER_MARK};
pub use stream::{compress, compress_declaring_size, ZstdEncoder};
