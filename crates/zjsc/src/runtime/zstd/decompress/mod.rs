//! Descompressor zstd de streaming, porte do libzstd 1.5.7 (`lib/decompress`, `ZSTD_decompressStream`).
//!
//! Estado da fatia 1:
//! - `frame`: cabeçalho de quadro, inclusive skippable;
//! - `bits`, `fse` (leitura de contagens e tabela de decodificação), `huf` (`HUF_readStats`, X1 de 1 e 4
//!   fluxos), `literals` (`ZSTD_decodeLiteralsBlock`);
//! - `window`: janela circular; `xxh64`: checksum de conteúdo;
//! - `Decoder`: máquina de estados que recebe a entrada em pedaços e emite cada bloco decodificado
//!   assim que ele fecha (raw, RLE; o bloco comprimido decodifica os literais).
//!
//! - `sequences`: cabeçalho de sequências, tabelas FSE (predefinida, RLE, comprimida, repetida),
//!   `ZSTD_decodeSequence` e `ZSTD_execSequence`.
//!
//! Ligado ao `DecompressionStream('zstd')` em `compression_streams.rs` (`Decoder::push_frame`).

pub mod bits;
pub mod frame;
pub mod fse;
pub mod huf;
pub mod literals;
pub mod sequences;
pub mod window;
pub mod xxh64;

use frame::{FrameHeader, Parsed};
use huf::HufTable;
use sequences::SeqDecoder;
use window::Window;
use xxh64::Xxh64;

/// Códigos de erro do libzstd que esta porta produz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Corruption,
    SrcSizeWrong,
    DstSizeTooSmall,
    TableLogTooLarge,
    MaxSymbolValueTooSmall,
    PrefixUnknown,
    FrameParameterUnsupported,
    FrameParameterWindowTooLarge,
    DictionaryCorrupted,
    ChecksumWrong,
}

enum State {
    FrameStart,
    Skip { remaining: u64 },
    BlockHeader,
    Block { last: bool, kind: u8, size: usize },
    Checksum,
}

/// Decodificador de streaming: `push` recebe bytes comprimidos em qualquer fatiamento.
pub struct Decoder {
    state: State,
    pending: Vec<u8>,
    at: usize,
    header: Option<FrameHeader>,
    window: Window,
    hash: Xxh64,
    frame_output: u64,
    block_size_max: usize,
    huf_table: Option<HufTable>,
    seq: SeqDecoder,
    /// Há um quadro em andamento (entre o cabeçalho e o fim do último bloco/checksum).
    in_frame: bool,
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder {
    pub fn new() -> Self {
        Decoder {
            state: State::FrameStart,
            pending: Vec::new(),
            at: 0,
            header: None,
            window: Window::new(1),
            hash: Xxh64::new(),
            frame_output: 0,
            block_size_max: literals::BLOCK_SIZE_MAX,
            huf_table: None,
            seq: SeqDecoder::new(),
            in_frame: false,
        }
    }

    /// `true` quando o fluxo está numa fronteira de quadro (nada começado e nada pendente).
    pub fn at_frame_boundary(&self) -> bool {
        matches!(self.state, State::FrameStart) && self.pending.len() == self.at
    }

    /// Consome `input`, anexando em `out` o conteúdo de cada bloco que fechou.
    pub fn push(&mut self, input: &[u8], out: &mut Vec<u8>) -> Result<(), Error> {
        self.pending.extend_from_slice(input);
        while self.step(out)? {}
        if self.at > 0 {
            self.pending.drain(..self.at);
            self.at = 0;
        }
        Ok(())
    }

    /// Como `push`, mas para no fim do primeiro quadro (ou quadro skippable) que fecha: devolve quantos bytes de
    /// `input` foram usados e se um quadro fechou. O resto de `input` fica com o chamador (o `CompressionStream`
    /// do bun confere o que vem depois de um quadro antes de abrir o seguinte).
    pub fn push_frame(&mut self, input: &[u8], out: &mut Vec<u8>) -> Result<(usize, bool), Error> {
        self.pending.extend_from_slice(input);
        let mut closed = false;
        while !closed {
            let started = !matches!(self.state, State::FrameStart);
            if !self.step(out)? {
                break;
            }
            closed = started && matches!(self.state, State::FrameStart);
        }
        let leftover = if closed { self.pending.len() - self.at } else { 0 };
        self.pending.truncate(self.pending.len() - leftover);
        self.pending.drain(..self.at);
        self.at = 0;
        Ok((input.len() - leftover.min(input.len()), closed))
    }

    fn avail(&self) -> &[u8] {
        &self.pending[self.at..]
    }

    /// Avança uma transição; `false` quando faltam bytes.
    fn step(&mut self, out: &mut Vec<u8>) -> Result<bool, Error> {
        match self.state {
            State::FrameStart => match frame::parse(self.avail())? {
                Parsed::Need(_) => Ok(false),
                Parsed::Skippable { header_size, skip } => {
                    self.at += header_size;
                    self.state = State::Skip { remaining: skip as u64 };
                    Ok(true)
                }
                Parsed::Frame(h) => {
                    self.at += h.header_size;
                    self.block_size_max = literals::BLOCK_SIZE_MAX.min(h.window_size.max(1) as usize);
                    self.window = Window::new(h.window_size.max(1) as usize);
                    self.hash = Xxh64::new();
                    self.frame_output = 0;
                    self.huf_table = None;
                    self.seq.reset();
                    self.header = Some(h);
                    self.in_frame = true;
                    self.state = State::BlockHeader;
                    Ok(true)
                }
            },
            State::Skip { remaining } => {
                let take = (self.avail().len() as u64).min(remaining) as usize;
                self.at += take;
                let left = remaining - take as u64;
                if left == 0 {
                    self.state = State::FrameStart;
                    return Ok(true);
                }
                self.state = State::Skip { remaining: left };
                Ok(false)
            }
            State::BlockHeader => {
                let a = self.avail();
                if a.len() < 3 {
                    return Ok(false);
                }
                let h = u32::from_le_bytes([a[0], a[1], a[2], 0]);
                let kind = ((h >> 1) & 3) as u8;
                let size = (h >> 3) as usize;
                if kind == 3 || size > self.block_size_max {
                    return Err(Error::Corruption);
                }
                self.at += 3;
                self.state = State::Block { last: h & 1 == 1, kind, size };
                Ok(true)
            }
            State::Block { last, kind, size } => {
                let need = if kind == 1 { 1 } else { size };
                if self.pending.len() - self.at < need {
                    return Ok(false);
                }
                // `body` empresta só `self.pending`; janela, hash, contador e sequências são campos
                // disjuntos, então não há cópia do corpo nem tabela clonada.
                let body = &self.pending[self.at..self.at + need];
                let start = out.len();
                match kind {
                    0 => {
                        out.extend_from_slice(body);
                        self.window.push_slice(&out[start..]);
                    }
                    1 => {
                        out.resize(start + size, body[0]);
                        self.window.push_slice(&out[start..]);
                    }
                    _ => {
                        let lit = literals::decode(body, self.block_size_max, &mut self.huf_table)?;
                        self.seq.decode_block(&body[lit.consumed..], &lit.data, &mut self.window, out, self.block_size_max)?;
                        // A janela já recebeu o bloco inteiro durante a execução das sequências.
                    }
                }
                self.hash.update(&out[start..]);
                self.frame_output += (out.len() - start) as u64;
                self.at += need;
                if last {
                    self.finish_blocks()?;
                } else {
                    self.state = State::BlockHeader;
                }
                Ok(true)
            }
            State::Checksum => {
                let a = self.avail();
                if a.len() < 4 {
                    return Ok(false);
                }
                let stored = u32::from_le_bytes([a[0], a[1], a[2], a[3]]);
                if stored != self.hash.digest() as u32 {
                    return Err(Error::ChecksumWrong);
                }
                self.at += 4;
                self.in_frame = false;
                self.state = State::FrameStart;
                Ok(true)
            }
        }
    }

    fn finish_blocks(&mut self) -> Result<(), Error> {
        let header = self.header.as_ref().ok_or(Error::Corruption)?;
        if let Some(expected) = header.content_size {
            if expected != self.frame_output {
                return Err(Error::Corruption);
            }
        }
        if header.checksum {
            self.state = State::Checksum;
        } else {
            self.in_frame = false;
            self.state = State::FrameStart;
        }
        Ok(())
    }

    /// Fim da entrada: `true` se o fluxo terminou limpo (zstd exige fronteira de quadro).
    pub fn finished(&self) -> bool {
        !self.in_frame && self.at_frame_boundary()
    }
}

/// Decodifica um fluxo zstd inteiro (para os testes do compressor conferirem a ida e volta).
#[cfg(test)]
pub(crate) fn decode_all(stream: &[u8]) -> Vec<u8> {
    let mut decoder = Decoder::new();
    let mut out = Vec::new();
    decoder.push(stream, &mut out).expect("quadro válido");
    assert!(decoder.finished());
    out
}

#[cfg(test)]
mod tests {
    use super::{Decoder, Error};
    use sha2::{Digest, Sha256};

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
    }

    fn decode(frame: &[u8], chunk: usize) -> Result<Vec<u8>, Error> {
        let mut dec = Decoder::new();
        let mut out = Vec::new();
        for part in frame.chunks(chunk) {
            dec.push(part, &mut out)?;
        }
        assert!(dec.finished());
        Ok(out)
    }

    /// xorshift32, o mesmo gerador do script que montou as entradas no bun.
    fn xorshift(seed: u32) -> impl FnMut() -> u32 {
        let mut s = seed;
        move || {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            s
        }
    }

    /// 200 bytes de base repetidos; um byte mutado a cada 16384 a partir da posição 2048.
    fn multi_input(n: usize, seed: u32) -> Vec<u8> {
        let mut r = xorshift(seed);
        let base: Vec<u8> = (0..200).map(|_| 97 + (r() % 26) as u8).collect();
        let mut out: Vec<u8> = (0..n).map(|i| base[i % 200]).collect();
        let mut i = 2048;
        while i < n {
            out[i] = 65 + (r() % 26) as u8;
            i += 16384;
        }
        out
    }

    const MULTI_SHA: &str = "5412c3c47a6cc0713c62864728f3a41435f5851cfa1bc56e460262a7ec01411d";

    // Quadros medidos no bun 1.4.2 (`Bun.zstdCompressSync` com `level`) sobre `multi_input(300000, 12345)`.
    // Três blocos. Nível 1: tabelas predefinidas no primeiro bloco e literais RLE com sequências RLE nos
    // outros dois. Níveis 19 e 22: primeiro bloco com tabelas comprimidas, os outros dois repetem as
    // tabelas e os offsets recentes do bloco anterior.
    const MULTI_L1: &str = "28b52ffda0e0930400cc0600820d271970277549dbfafb697fafd94ab6d9863b40179280aaaa6a800159f0038ae00536e00768e000a9d54b2a377070e8cc3fd7270b798340d429a866a5563a236421e4d83b7fd6a2e60ff3d336d907b89e86778a5e53310dc6e6046f263c2686abf2abc22da6fac1b53b627351de6056a1746d6452529163ac73267a2e81f01edefbebd5a5a585f983300925c3d3087954284468fa451ff9b073565b74de11003417f04204d4d17cc03322a084e603fe888062341ff04204d4d17cc03322a084e603fe888062341ff04204d4d17cc03322500259f3f23752cc0100887757634479576b4a77576d4d6b487a4c651150010034972752349f2752349f2752349f2752349f2752349f2752349f2752349f2752fcff01d5000038644e7942754c6f0750010014f38914cde78914cde78914ff7f";
    const MULTI_L19: &str = "28b52ffda0e09304008c0600028d251680e90181add0a36d0864119d23f204c78c199881aa0706510c421000ecb21fd51cd5e775a2a7f326750c2da6aeab8e76636b4e3934c6ad35e14fffc887d127e849b68afe6a3b7b3f793db38364b5721e7ee14123529c397e4ba683edd2eaba8bbd3139f4219734766da3392c51a5f49cc8ba7207e2f7d1777add3d9275555f10c4968a3549637e256d21ccbed7db98604acdd27aec11a8f07affbf03f05eb71a10feffff7f66f42d10e64d0b0ef3da82cbbc69c1675e5bf098972df8cc9b163ce6b505efcc4f9f8cac79c9490a6c0100405744574a574d484c11fc2d10e64d0b0ef3da82cbbc69c1675e5bf098972df8cc9b163ce6b5059ff9fae77406a50000184e424c07fc0dc0bc68c1316f5b70994fff38cc";
    const MULTI_L22: &str = "28b52ffda0e09304008c0600028d251680e90181add0a36d0864119d23f204c78c199881aa0706510c421000ecb21fd51cd5e775a2a7f326750c2da6aeab8e76636b4e3934c6ad35e14fffc887d127e849b68afe6a3b7b3f793db38364b5721e7ee14123529c397e4ba683edd2eaba8bbd3139f4219734766da3392c51a5f49cc8ba7207e2f7d1777add3d9275555f10c4968a3549637e256d21ccbed7db98604acdd27aec11a8f07affbf03f05eb71a10feffff7f66f42d10e64d0b0ef3da82cbbc69c1675e5bf098972df8cc9b163ce6b505efcc4f9f8cac79c9490a6c0100405744574a574d484c11fc2d10e64d0b0ef3da82cbbc69c1675e5bf098972df8cc9b163ce6b5059ff9fae77406a50000184e424c07fc0dc0bc68c1316f5b70994fff38cc";
    // `x` repetido 8 vezes, nível 19 (bun): literais RLE sem sequências.
    const RLE_LITERALS: &str = "28b52ffd20081d0000417800";

    #[test]
    fn multi_block_frames_match_registered_sha256() {
        let input = multi_input(300_000, 12345);
        assert_eq!(sha256_hex(&input), MULTI_SHA);
        for frame in [MULTI_L1, MULTI_L19, MULTI_L22] {
            for chunk in [usize::MAX, 4096, 1] {
                let out = decode(&hex(frame), chunk).unwrap();
                assert_eq!(out.len(), 300_000);
                assert_eq!(sha256_hex(&out), MULTI_SHA);
            }
        }
    }

    #[test]
    fn rle_literals_without_sequences() {
        assert_eq!(decode(&hex(RLE_LITERALS), 1000).unwrap(), b"xxxxxxxx");
        assert_eq!(decode(&hex(RLE_LITERALS), 1).unwrap(), b"xxxxxxxx");
    }

    /// Quadro com checksum: o `zstd` CLI (`-3 --check`) emite o mesmo quadro do bun com o bit de
    /// checksum ligado no descritor (0xa0 vira 0xa4) e o XXH64 de 32 bits no fim (medido: e1756905).
    fn with_checksum(frame_hex: &str, digest: &str) -> Vec<u8> {
        let mut frame = hex(frame_hex);
        assert_eq!(frame[4], 0xa0);
        frame[4] = 0xa4;
        frame.extend_from_slice(&hex(digest));
        frame
    }

    #[test]
    fn frame_with_checksum() {
        let frame = with_checksum(MULTI_L1, "e1756905");
        for chunk in [usize::MAX, 1000, 3] {
            assert_eq!(sha256_hex(&decode(&frame, chunk).unwrap()), MULTI_SHA);
        }
    }

    #[test]
    fn wrong_checksum_is_rejected() {
        let frame = with_checksum(MULTI_L1, "e1756906");
        assert_eq!(decode(&frame, usize::MAX).unwrap_err(), Error::ChecksumWrong);
    }

    #[test]
    fn truncated_checksum_is_not_finished() {
        let mut frame = with_checksum(MULTI_L1, "e1756905");
        frame.truncate(frame.len() - 1);
        let mut dec = Decoder::new();
        let mut out = Vec::new();
        dec.push(&frame, &mut out).unwrap();
        assert!(!dec.finished());
    }

    #[test]
    fn skippable_frames_around_a_frame() {
        let mut stream = hex("5b2a4d18");
        stream.extend_from_slice(&5u32.to_le_bytes());
        stream.extend_from_slice(b"hello");
        stream.extend_from_slice(&hex("28b52ffd201e4d0000186162630100866e08"));
        stream.extend_from_slice(&hex("502a4d18"));
        stream.extend_from_slice(&0u32.to_le_bytes());
        for chunk in [usize::MAX, 7, 1] {
            assert_eq!(decode(&stream, chunk).unwrap(), "abc".repeat(10).into_bytes());
        }
    }

    #[test]
    fn two_concatenated_frames_reset_state() {
        let mut stream = hex(MULTI_L19);
        stream.extend_from_slice(&hex(MULTI_L1));
        let out = decode(&stream, 1000).unwrap();
        assert_eq!(out.len(), 600_000);
        assert_eq!(sha256_hex(&out[..300_000]), MULTI_SHA);
        assert_eq!(sha256_hex(&out[300_000..]), MULTI_SHA);
    }

    #[test]
    fn repeat_tables_without_previous_block_are_corruption() {
        // Quadro de um bloco com o primeiro bloco do nível 19 trocado por um cabeçalho que pede
        // tabelas repetidas num quadro novo: pula o bloco 1 e começa no 2 (que usa modo repetido).
        let full = hex(MULTI_L19);
        // Cabeçalho do quadro: 4 magic + 1 FHD + 4 FCS = 9 bytes; bloco 1: 3 de cabeçalho mais o corpo.
        let h1 = u32::from_le_bytes([full[9], full[10], full[11], 0]);
        let first_len = 3 + (h1 >> 3) as usize;
        let mut frame = full[..9].to_vec();
        frame.extend_from_slice(&full[9 + first_len..]);
        assert!(decode(&frame, usize::MAX).is_err());
    }
}
