//! Compressor zstd em streaming: `ZSTD_compressStream2` com `ZSTD_e_continue` nas escritas e `ZSTD_e_end` no fim.
//!
//! O libzstd acumula a entrada num buffer de um bloco (128 KiB) e só comprime quando ele enche: cada escrita
//! que completa 128 KiB entrega os blocos já comprimidos (o cabeçalho do quadro sai com o primeiro), e o
//! resto fica retido até o fim. No fim o que sobrou vira o último bloco; se nada sobrou, o quadro fecha com
//! o bloco raw vazio e último (`ZSTD_writeEpilogue`). O estado (tabelas de hash, repetições, entropia,
//! `savings` do pré-divisor) atravessa os trechos, no `Compressor` de `block`.
//!
//! O histórico inteiro fica retido em memória: os índices das tabelas são posições absolutas na entrada, e a
//! janela de 2 MiB do nível 3 é imposta pelo próprio buscador (`lowest_index`).

use super::block::Compressor;
use super::frame::BLOCK_SIZE_MAX;
use super::params::level3_cparams;

pub struct ZstdEncoder {
    /// Toda a entrada recebida até agora.
    history: Vec<u8>,
    /// Quanto da entrada já foi comprimido (sempre múltiplo de 128 KiB).
    done: usize,
    compressor: Compressor,
}

impl ZstdEncoder {
    /// Quadro sem tamanho declarado, nível 3, sem checksum: o `CompressionStream('zstd')` do bun.
    pub fn new() -> Self {
        Self::with_pledged(None)
    }

    /// Quadro que declara o tamanho do conteúdo no cabeçalho (segmento único quando cabe).
    fn with_pledged(pledged: Option<u64>) -> Self {
        ZstdEncoder { history: Vec::new(), done: 0, compressor: Compressor::new(&level3_cparams(pledged), pledged) }
    }

    /// `ZSTD_e_continue`: recebe `input` e devolve os bytes que esta escrita liberou (vazio enquanto o
    /// buffer de 128 KiB não enche).
    pub fn write(&mut self, input: &[u8]) -> Vec<u8> {
        self.history.extend_from_slice(input);
        let mut out = Vec::new();
        while self.history.len() - self.done >= BLOCK_SIZE_MAX {
            let end = self.done + BLOCK_SIZE_MAX;
            self.compressor.compress_chunk(&self.history[..end], self.done, false, &mut out);
            self.done = end;
        }
        out
    }

    /// `ZSTD_e_end`: comprime o que sobrou como último bloco e fecha o quadro.
    pub fn finish(mut self) -> Vec<u8> {
        let mut out = Vec::new();
        self.compressor.compress_chunk(&self.history, self.done, true, &mut out);
        self.compressor.write_epilogue(&mut out);
        out
    }
}

/// Comprime `input` como o `CompressionStream('zstd')` do bun numa escrita só: nível 3, sem checksum, tamanho
/// do conteúdo desconhecido no cabeçalho (janela de 2 MiB).
pub fn compress(input: &[u8]) -> Vec<u8> {
    let mut encoder = ZstdEncoder::new();
    let mut out = encoder.write(input);
    out.extend(encoder.finish());
    out
}

/// Como `compress`, mas declarando o tamanho do conteúdo no cabeçalho (o que o libzstd faz quando o fluxo é
/// fechado sem nenhuma escrita: tamanho 0, segmento único).
pub fn compress_declaring_size(input: &[u8]) -> Vec<u8> {
    let mut encoder = ZstdEncoder::with_pledged(Some(input.len() as u64));
    let mut out = encoder.write(input);
    out.extend(encoder.finish());
    out
}

impl Default for ZstdEncoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use crate::runtime::zstd::decompress::decode_all;

    fn sha256_hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Gerador de texto das medições no bun 1.4.2 (`wip/notes/compression-brotli-zstd.md`), determinístico:
    /// `state = seed; repete { state = (imul(state, 1103515245) + 12345) mod 2^32; anexa words[(state >> 24) % 6] }`
    /// com `words = ["alpha ", "beta ", "gamma ", "delta\n", "epsilon ", "zeta "]`, até `n` bytes (corta o excesso).
    /// O mesmo laço em JavaScript (`Math.imul`, `>>> 0`) produziu as entradas cujos tamanhos e SHA-256 de saída
    /// estão nos testes: os quatro conferiram com o bun em 2026-10-09.
    fn text(n: usize, seed: u32) -> Vec<u8> {
        let words: [&[u8]; 6] = [b"alpha ", b"beta ", b"gamma ", b"delta\n", b"epsilon ", b"zeta "];
        let mut state = seed;
        let mut out = Vec::new();
        while out.len() < n {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
            out.extend_from_slice(words[((state >> 24) % 6) as usize]);
        }
        out.truncate(n);
        out
    }

    /// Gerador binário das medições: `state = (imul(state, 1103515245) + 12345) mod 2^32` e o byte `state >> 24`
    /// por passo, `n` passos.
    fn binary(n: usize, seed: u32) -> Vec<u8> {
        let mut state = seed;
        (0..n)
            .map(|_| {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                (state >> 24) as u8
            })
            .collect()
    }

    /// Escreve em fatias de `step` bytes e devolve os pedaços não vazios que cada escrita liberou, mais o fim.
    fn pieces(input: &[u8], step: usize) -> Vec<Vec<u8>> {
        let mut encoder = ZstdEncoder::new();
        let mut out: Vec<Vec<u8>> = input.chunks(step).map(|piece| encoder.write(piece)).filter(|p| !p.is_empty()).collect();
        out.push(encoder.finish());
        out
    }

    #[test]
    fn small_inputs_release_nothing_before_the_end() {
        for input in [&b""[..], b"a", b"hello world hello world hello world", &[0u8; 1000]] {
            let mut encoder = ZstdEncoder::new();
            assert!(encoder.write(input).is_empty());
            assert_eq!(encoder.finish(), compress(input));
        }
    }

    /// Medido no bun: `text(131072, 2)` solta 18757 bytes na escrita e 3 (o bloco raw vazio e último) no fim;
    /// com um byte a mais, 18757 e 4.
    #[test]
    fn exact_block_multiple_closes_with_the_empty_raw_block() {
        for (n, tail) in [(131_072, 3), (131_073, 4)] {
            let sizes: Vec<usize> = pieces(&text(n, 2), n).iter().map(Vec::len).collect();
            assert_eq!(sizes, [18757, tail]);
        }
    }

    #[test]
    fn releases_blocks_on_the_write_that_completes_128_kib() {
        let input = text(300_000, 1);
        let sizes: Vec<usize> = pieces(&input, 1000).iter().map(Vec::len).collect();
        assert_eq!(sizes, [18749, 18640, 5411]);
        let whole = pieces(&input, input.len());
        assert_eq!(whole.iter().map(Vec::len).collect::<Vec<_>>(), [37389, 5411]);
        let mib: Vec<usize> = pieces(&text(1 << 20, 2), 1000).iter().map(Vec::len).collect();
        assert_eq!(mib, [18757, 18671, 18684, 18671, 18634, 18645, 18689, 18648, 3]);
    }

    #[test]
    fn output_matches_registered_sha256() {
        let cases: [(Vec<u8>, usize, &str); 4] = [
            (text(300_000, 1), 42800, "648a8fe20bd8eb71c0d87c1ec0e04466cf04cdf1b1ffee23adb499b28b86729d"),
            (text(1 << 20, 2), 149_402, "bd07fe953c76f8883d216cc5c9b3a1c1d6e3e2d2340c33405c5830cf0fc5a8fa"),
            (binary(1 << 20, 3), 1_048_609, "0634a6fb0f53410efeb7a0fb69c37a4f457fc38512b2ccfa7da99a109753dde5"),
            (text(3 << 20, 4), 447_437, "4cb71f3eb728ce996522330042b88c6740b7c8ff4b338e54389fe81734645b65"),
        ];
        for (input, size, sha) in cases {
            for step in [input.len(), 1000, 65_536] {
                let frame = pieces(&input, step).concat();
                assert_eq!(frame.len(), size);
                assert_eq!(sha256_hex(&frame), sha);
                assert_eq!(decode_all(&frame), input);
            }
        }
    }
}
