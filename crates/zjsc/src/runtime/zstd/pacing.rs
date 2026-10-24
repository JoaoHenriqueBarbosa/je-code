//! O ritmo com que o `CompressionStream('zstd')` do bun 1.4.2 entrega a saída: o laço de `CompressionStreamCoder::run`
//! (`src/runtime/webcore/CompressionStreamCoder.rs`) sobre o `ZSTD_compressStream_generic` do libzstd 1.5.7
//! (`lib/compress/zstd_compress.c`).
//!
//! A regra, derivada do código e não de medição:
//!
//! - cada escrita é um passo cuja saída tem no máximo `cap = max(highWaterMark, tamanho da escrita)` bytes
//!   (`highWaterMark` 65536); um passo que atinge `cap` pede outro (`more`) e o pedaço sai inteiro, sem emendar;
//! - cada volta chama `ZSTD_compressStream2` com um espaço de saída de `min(cap - len, folga do Vec)`; a folga é o que
//!   `try_reserve(min(restante, 16 KiB))` deixa: o `Vec` dobra de capacidade, então os espaços crescem 16, 16, 32, 64,
//!   128 KiB... dentro de um passo;
//! - o `Vec` de um passo na thread do JS é o rascunho da VM (capacidade preservada até 256 KiB); escrita maior que
//!   128 KiB roda fora da thread com um `Vec` novo;
//! - o libzstd só comprime um bloco quando 128 KiB de entrada estão carregados; se o espaço de saída cabe o pior caso
//!   (`ZSTD_compressBound`), o bloco vai direto; senão vai para o buffer interno e sai aos poucos, e o que não coube
//!   fica retido até a próxima volta (a escrita seguinte ou o `close`);
//! - com `ZSTD_e_continue` o passo termina quando toda a entrada foi consumida, mesmo com saída retida.

use std::cell::Cell;

use super::frame::BLOCK_SIZE_MAX;
use super::stream::{compress_declaring_size, ZstdEncoder};

/// O `highWaterMark` padrão do bun para os fluxos de compressão (`kDefaultCodecHighWaterMark`).
pub const DEFAULT_HIGH_WATER_MARK: usize = 64 * 1024;
/// Crescimento pedido ao `Vec` de saída a cada volta.
const SPARE_GROWTH: usize = 16 * 1024;
/// O rascunho da VM não guarda `Vec` maior que isto.
const SCRATCH_KEEP: usize = 256 * 1024;
/// Acima disto a escrita roda fora da thread do JS, com um `Vec` novo.
const ASYNC_THRESHOLD: usize = 128 * 1024;

thread_local! {
    /// A capacidade do `Vec` de rascunho da VM (`take_compression_scratch`), que sobrevive de um passo ao outro.
    static SCRATCH_CAPACITY: Cell<usize> = const { Cell::new(0) };
}

/// Esquece o rascunho, como uma VM nova.
pub fn reset_scratch() {
    SCRATCH_CAPACITY.with(|capacity| capacity.set(0));
}

/// `ZSTD_compressBound`.
fn compress_bound(source: usize) -> usize {
    source + (source >> 8) + if source < BLOCK_SIZE_MAX { (BLOCK_SIZE_MAX - source) >> 11 } else { 0 }
}

/// O `Vec` de saída de um passo: só o comprimento e a capacidade (com o crescimento do `Vec`) importam.
struct StepOutput {
    bytes: Vec<u8>,
    capacity: usize,
}

impl StepOutput {
    /// `spare(out, cap)` do bun: o espaço que a próxima volta recebe.
    fn spare(&mut self, cap: usize) -> usize {
        let budget = cap - self.bytes.len();
        let required = self.bytes.len() + budget.min(SPARE_GROWTH);
        if required > self.capacity {
            self.capacity = required.max(self.capacity * 2).max(8);
        }
        (self.capacity - self.bytes.len()).min(budget)
    }
}

/// O resto de uma escrita cujo último passo parou em `cap`.
struct Pending {
    input: Vec<u8>,
    cap: usize,
    finish: bool,
    off_thread: bool,
}

/// O compressor em streaming com o ritmo de saída do bun.
pub struct PacedEncoder {
    /// `None` depois do fim do quadro (ou se o quadro foi gerado direto, sem o codificador).
    encoder: Option<ZstdEncoder>,
    /// O `highWaterMark` do fluxo: o piso de `cap` em cada passo (`CompressionStreamCoder::step`).
    high_water_mark: usize,
    /// Houve alguma chamada de `ZSTD_compressStream2` (decide se o cabeçalho declara o tamanho).
    started: bool,
    /// Entrada carregada que ainda não completou um bloco.
    loaded: Vec<u8>,
    /// Bloco comprimido no buffer interno e o quanto dele já saiu.
    held: Vec<u8>,
    flushed: usize,
    flushing: bool,
    frame_ended: bool,
    pending: Option<Pending>,
}

impl PacedEncoder {
    pub fn new(high_water_mark: usize) -> PacedEncoder {
        PacedEncoder { encoder: Some(ZstdEncoder::new()), high_water_mark, started: false, loaded: Vec::new(), held: Vec::new(), flushed: 0, flushing: false, frame_ended: false, pending: None }
    }

    /// Uma escrita (`ZSTD_e_continue`): os pedaços que cada passo entregou, na ordem.
    pub fn write(&mut self, input: &[u8]) -> Vec<Vec<u8>> {
        self.run_chunk(input, false)
    }

    /// O `close` (`ZSTD_e_end`): os pedaços finais.
    pub fn finish(&mut self) -> Vec<Vec<u8>> {
        self.run_chunk(&[], true)
    }

    fn run_chunk(&mut self, input: &[u8], finish: bool) -> Vec<Vec<u8>> {
        let mut pieces = Vec::new();
        let mut first = Some((input, finish));
        loop {
            let (output, more) = self.step(first.take());
            if !output.is_empty() {
                pieces.push(output);
            }
            if !more {
                return pieces;
            }
        }
    }

    /// Um passo: `Some((entrada, finish))` abre uma escrita, `None` continua a pendente.
    fn step(&mut self, fresh: Option<(&[u8], bool)>) -> (Vec<u8>, bool) {
        let (data, cap, finish, off_thread) = match (self.pending.take(), fresh) {
            (Some(pending), _) => (pending.input, pending.cap, pending.finish, pending.off_thread),
            (None, Some((input, finish))) => (input.to_vec(), self.high_water_mark.max(input.len()), finish, input.len() > ASYNC_THRESHOLD),
            (None, None) => return (Vec::new(), false),
        };
        let capacity = if off_thread { 0 } else { SCRATCH_CAPACITY.with(Cell::get) };
        let mut out = StepOutput { bytes: Vec::new(), capacity };
        let mut consumed = 0;
        let more = loop {
            if out.bytes.len() >= cap {
                self.pending = Some(Pending { input: data[consumed..].to_vec(), cap, finish, off_thread });
                break true;
            }
            let room = out.spare(cap);
            let (used, remaining) = self.compress_stream(&data[consumed..], finish, room, &mut out.bytes);
            consumed += used;
            if consumed == data.len() && (!finish || remaining == 0) {
                break false;
            }
        };
        if !off_thread {
            SCRATCH_CAPACITY.with(|cell| cell.set(if out.capacity <= SCRATCH_KEEP { out.capacity } else { 0 }));
        }
        (out.bytes, more)
    }

    /// `ZSTD_compressStream2`: devolve quanta entrada usou e quanto da saída segue retido.
    fn compress_stream(&mut self, input: &[u8], end: bool, room: usize, out: &mut Vec<u8>) -> (usize, usize) {
        let first_call = !self.started;
        self.started = true;
        let start = out.len();
        let mut used = 0;
        loop {
            if self.flushing {
                let to_flush = self.held.len() - self.flushed;
                let n = to_flush.min(room - (out.len() - start));
                out.extend_from_slice(&self.held[self.flushed..self.flushed + n]);
                self.flushed += n;
                if n != to_flush {
                    break;
                }
                self.held.clear();
                self.flushed = 0;
                self.flushing = false;
                if self.frame_ended {
                    break;
                }
            }
            let left = input.len() - used;
            let space = room - (out.len() - start);
            if end && self.loaded.is_empty() && space >= compress_bound(left) {
                // Atalho do libzstd: o fim do quadro direto no buffer de saída.
                if first_call && self.flushed == 0 && self.held.is_empty() && self.encoder.is_some() {
                    // Primeira chamada já com `ZSTD_e_end`: o tamanho do conteúdo vai no cabeçalho.
                    out.extend(compress_declaring_size(&input[used..]));
                } else if let Some(mut encoder) = self.encoder.take() {
                    out.extend(encoder.write(&input[used..]));
                    out.extend(encoder.finish());
                }
                used = input.len();
                self.encoder = None;
                self.frame_ended = true;
                break;
            }
            let take = (BLOCK_SIZE_MAX - self.loaded.len()).min(left);
            self.loaded.extend_from_slice(&input[used..used + take]);
            used += take;
            if !end && self.loaded.len() < BLOCK_SIZE_MAX {
                break;
            }
            let last = end && used == input.len();
            let block = std::mem::take(&mut self.loaded);
            let Some(mut encoder) = self.encoder.take() else { break };
            let mut bytes = encoder.write(&block);
            if last {
                bytes.extend(encoder.finish());
            } else {
                self.encoder = Some(encoder);
            }
            self.frame_ended = last;
            if space >= compress_bound(block.len()) {
                out.extend(bytes);
                if last {
                    break;
                }
            } else {
                self.held = bytes;
                self.flushed = 0;
                self.flushing = true;
            }
        }
        (used, self.held.len() - self.flushed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sizes(pieces: &[Vec<u8>]) -> Vec<usize> {
        pieces.iter().map(Vec::len).collect()
    }

    /// Entrada binária das medições (`state = imul(state, 1103515245) + 12345`, byte `state >> 24`).
    fn binary(n: usize, seed: u32) -> Vec<u8> {
        let mut state = seed;
        (0..n)
            .map(|_| {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                (state >> 24) as u8
            })
            .collect()
    }

    /// Medido no bun 1.4.2 num processo novo (rascunho vazio): `w:` na escrita, `end:` no close.
    #[test]
    fn fresh_process_binary_vectors() {
        for (n, write, end) in [
            (131_072, vec![16384], vec![65536, 49164]),
            (262_144, vec![262_144], vec![15]),
            (65_536, vec![], vec![65536, 9]),
            (131_073, vec![131_073, 8], vec![4]),
            (1_000_000, vec![917_531], vec![65536, 16963]),
        ] {
            reset_scratch();
            let mut encoder = PacedEncoder::new(DEFAULT_HIGH_WATER_MARK);
            assert_eq!(sizes(&encoder.write(&binary(n, 3))), write, "write {n}");
            assert_eq!(sizes(&encoder.finish()), end, "end {n}");
        }
    }
}
