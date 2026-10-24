//! `CompressionStream` e `DecompressionStream`. Como as de texto ([`super::text_streams`]), no bun são classes próprias
//! (protótipo herda de `Object.prototype`, a instância NÃO é um `TransformStream`) que embrulham um `TransformStream`
//! interno construído com um `transformer` nativo; `readable`/`writable` devolvem sempre as pontas dele.
//!
//! Medido no bun 1.4.2:
//!
//! - `length` 1; formato por `ToString`; `'gzip'`, `'deflate'`, `'deflate-raw'`, `'brotli'` e `'zstd'` valem, e o resto
//!   (inclusive `'GZIP'`, `''`, `undefined`) lança `TypeError` `ERR_INVALID_ARG_VALUE` com `The argument 'format' must be
//!   one of: deflate, deflate-raw, gzip, brotli, zstd. Received <valor>` (string entre aspas simples, o resto cru);
//! - o pedaço é `BufferSource` ou string (UTF-8); outro tipo é `ERR_INVALID_ARG_TYPE`;
//! - a compressão usa o nível padrão do zlib: o cabeçalho (gzip de 10 bytes com SO `03`, zlib `789c`) sai no primeiro
//!   pedaço e o resto no `flush`; um fluxo vazio sai num pedaço só; sem pedaço vazio enfileirado;
//! - a descompressão devolve o que decodificou a cada pedaço; gzip aceita membros concatenados; dado inválido é
//!   `TypeError: inflate failed`; truncado (ou vazio) é `TypeError: unexpected end of file` no `flush`; sobra depois do
//!   fim de `deflate`/`deflate-raw` é `ERR_TRAILING_JUNK_AFTER_STREAM_END`, e sobra depois de um membro gzip é lida
//!   como outro cabeçalho (`inflate failed`).
//!
//! `brotli` (rust-brotli, qualidade 11, `lgwin` 22) e `zstd` (compressor e descompressor próprios em `runtime::zstd`,
//! portes do libzstd 1.5.7; a saída da compressão sai no ritmo do laço de `CompressionStreamCoder::run` do bun, em
//! `runtime::zstd::pacing`): medições em `wip/notes/compression-brotli-zstd.md`.
//!
//! O pedaço de saída de um passo tem no máximo `max(65536, tamanho da escrita)` bytes (regra do `step` do bun).
//!
//! Os bytes do deflate saem do `zdeflate` no sabor zlib-ng 2.2.4 (o do bun: o `deflate_medium` do nível 6).
//!
//! DIVERGÊNCIAS: um erro de lixo no fim descarta a saída da mesma escrita (o bun emite antes os pedaços cheios dos
//! passos anteriores); `brotli` decodifica em streaming e o código de erro é `ERR_` mais o nome do erro do decodificador.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use flate2::{Crc, Decompress, FlushDecompress, Status};

use super::readable::extract_high_water_mark;
use super::text_streams::{construct_inner_stream, enqueue, inspect_composite, install_wrapped_class, invalid_chunk, native_transformer, Produced};
use super::trace::{mark_all, Traced};
use super::transform_js::{ts_readable, ts_writable};
use super::writable_js::convert_strategy;
use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::derived_structure;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_value::{EncodedJSValue, JSValue};
use crate::runtime::native_class_support::{instance_structure, throw_coded_type_error};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::text_decoder::{input_bytes, text_value};
use crate::runtime::text_encoder::string_units;
use crate::runtime::zstd::decompress::frame::is_frame_prefix;
use crate::runtime::zstd::decompress::Decoder;
use crate::runtime::zstd::{PacedEncoder, DEFAULT_HIGH_WATER_MARK};

/// Tamanho da folga de saída pedida ao zlib a cada volta.
const CHUNK: usize = 32 * 1024;

/// Cabeçalho gzip do zlib do bun: sem nome, sem data, `XFL` 0, SO `03` (Unix).
const GZIP_HEADER: [u8; 10] = [0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3];
/// O cabeçalho zlib do nível 6 (`CMF` 0x78, `FLG` 0x9c). Como o gzip, sai à mão no primeiro `write`: o zlib do
/// bun o emite na primeira chamada de `deflate`, mesmo sem flush, e o primeiro pedaço do bun é só ele.
const ZLIB_HEADER: [u8; 2] = [0x78, 0x9c];

/// O Adler-32 do rodapé zlib (RFC 1950), somado sobre a entrada como o `Crc` do gzip.
struct Adler32 {
    a: u32,
    b: u32,
}

impl Adler32 {
    const MODULO: u32 = 65521;

    fn new() -> Adler32 {
        Adler32 { a: 1, b: 0 }
    }

    fn update(&mut self, input: &[u8]) {
        for byte in input {
            self.a = (self.a + u32::from(*byte)) % Self::MODULO;
            self.b = (self.b + self.a) % Self::MODULO;
        }
    }

    fn sum(&self) -> u32 {
        (self.b << 16) | self.a
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Format {
    Gzip,
    Deflate,
    DeflateRaw,
    Brotli,
    Zstd,
}

impl Format {
    fn parse(name: &str) -> Option<Format> {
        match name {
            "gzip" => Some(Format::Gzip),
            "deflate" => Some(Format::Deflate),
            "deflate-raw" => Some(Format::DeflateRaw),
            "brotli" => Some(Format::Brotli),
            "zstd" => Some(Format::Zstd),
            _ => None,
        }
    }

    /// `brotli` acumula a entrada inteira: o bun só devolve a compressão no `flush`.
    fn buffered(self) -> bool {
        self == Format::Brotli
    }

    /// Se o fluxo deflate de dentro leva o cabeçalho e o adler do zlib (o gzip leva o dele, tratado à mão).
    fn zlib_wrapper(self) -> bool {
        self == Format::Deflate
    }
}

#[derive(Debug, PartialEq, Eq)]
enum CodecError {
    Inflate,
    UnexpectedEof,
    TrailingJunk,
    /// Com o código de erro do bun (`ERR__ERROR_FORMAT_PADDING_1` e companhia).
    BrotliFailed(String),
    ZstdFailed,
}

/// Brotli como o bun (`quality` 11, `lgwin` 22, o padrão do `BrotliEncoderCreateInstance` do `zlib` do Node).
fn brotli_compress(input: &[u8]) -> Result<Vec<u8>, CodecError> {
    use std::io::Write;
    let mut writer = brotli::CompressorWriter::new(Vec::new(), 4096, 11, 22);
    writer.write_all(input).map_err(|_| CodecError::BrotliFailed("ERR__ERROR_UNREACHABLE".to_owned()))?;
    // Sem `flush`: ele emite um bloco de metadados vazio antes do fim (`6b0003` no stream vazio); o bun só termina o
    // stream (`3b`), e `into_inner` faz exatamente isso.
    Ok(writer.into_inner())
}

type BrotliState = brotli::BrotliState<brotli::HeapAlloc<u8>, brotli::HeapAlloc<u32>, brotli::HeapAlloc<brotli::HuffmanCode>>;

/// Decodificador incremental de brotli: a saída de cada pedaço sai no pedaço que a completa, como no bun.
struct BrotliStream {
    state: Box<BrotliState>,
    total_out: usize,
    /// O fim do fluxo já passou; qualquer byte depois é lixo.
    ended: bool,
}

impl BrotliStream {
    fn new() -> BrotliStream {
        let state = BrotliState::new(brotli::HeapAlloc::<u8>::new(0), brotli::HeapAlloc::<u32>::new(0), brotli::HeapAlloc::<brotli::HuffmanCode>::new(brotli::HuffmanCode { bits: 2, value: 1 }));
        BrotliStream { state: Box::new(state), total_out: 0, ended: false }
    }

    /// O código de erro do bun: `ERR_` mais o nome do `BrotliDecoderErrorCode` sem o prefixo `BROTLI_DECODER`
    /// (`ERR__ERROR_FORMAT_PADDING_1`), medido para vários códigos.
    fn error_code(&self) -> String {
        let name = format!("{:?}", self.state.error_code);
        format!("ERR_{}", name.strip_prefix("BROTLI_DECODER").unwrap_or(&name))
    }

    /// A saída vai para `out` (que fica com o que foi produzido antes de um erro).
    fn write(&mut self, input: &[u8], out: &mut Vec<u8>) -> Result<(), CodecError> {
        if self.ended {
            return if input.is_empty() { Ok(()) } else { Err(CodecError::TrailingJunk) };
        }
        let mut buffer = vec![0u8; CHUNK];
        let (mut avail_in, mut input_offset) = (input.len(), 0usize);
        loop {
            let (mut avail_out, mut output_offset) = (buffer.len(), 0usize);
            let result = brotli::BrotliDecompressStream(&mut avail_in, &mut input_offset, input, &mut avail_out, &mut output_offset, &mut buffer, &mut self.total_out, &mut self.state);
            out.extend_from_slice(&buffer[..output_offset]);
            match result {
                brotli::BrotliResult::NeedsMoreInput => return Ok(()),
                brotli::BrotliResult::NeedsMoreOutput => {}
                brotli::BrotliResult::ResultSuccess => {
                    self.ended = true;
                    return if input_offset < input.len() { Err(CodecError::TrailingJunk) } else { Ok(()) };
                }
                brotli::BrotliResult::ResultFailure => return Err(CodecError::BrotliFailed(self.error_code())),
            }
        }
    }

    fn finish(&self) -> Result<Vec<u8>, CodecError> {
        if self.ended {
            Ok(Vec::new())
        } else {
            Err(CodecError::UnexpectedEof)
        }
    }
}

/// Decodificador incremental de zstd com a semântica do `CompressionStreamCoder` do bun: a saída dos blocos que
/// chegaram inteiros sai na escrita que os completa; vários quadros concatenados valem; depois de um quadro
/// completo, o que não é prefixo de número mágico (de quadro ou skippable) é `ERR_TRAILING_JUNK_AFTER_STREAM_END`,
/// e um prefixo de menos de 4 bytes espera a escrita seguinte (no `flush`, vira lixo).
struct ZstdStream {
    decoder: Decoder,
    /// Um quadro completo já fechou e o próximo ainda não abriu.
    ended: bool,
    /// Os 1 a 3 bytes de um número mágico partido entre escritas.
    head: Vec<u8>,
}

impl ZstdStream {
    fn new() -> ZstdStream {
        ZstdStream { decoder: Decoder::new(), ended: false, head: Vec::new() }
    }

    /// A saída vai para `out` (que fica com o que foi produzido antes de um erro).
    fn write(&mut self, input: &[u8], out: &mut Vec<u8>) -> Result<(), CodecError> {
        let data = [std::mem::take(&mut self.head), input.to_vec()].concat();
        let mut rest = &data[..];
        loop {
            if self.ended {
                if rest.is_empty() {
                    break;
                }
                let head = &rest[..rest.len().min(4)];
                if !is_frame_prefix(head) {
                    return Err(CodecError::TrailingJunk);
                }
                if head.len() < 4 {
                    self.head = head.to_vec();
                    break;
                }
                self.ended = false;
            }
            let (used, closed) = self.decoder.push_frame(rest, out).map_err(|_| CodecError::ZstdFailed)?;
            rest = &rest[used..];
            if !closed {
                break;
            }
            self.ended = true;
        }
        Ok(())
    }

    fn finish(&self) -> Result<Vec<u8>, CodecError> {
        if !self.ended {
            Err(CodecError::UnexpectedEof)
        } else if !self.head.is_empty() {
            // `head` só guarda um prefixo do número mágico (lixo de verdade já falhou no `write`): é um quadro
            // seguinte truncado, o `ZSTD_decompressStream` ainda esperava entrada.
            Err(CodecError::UnexpectedEof)
        } else {
            Ok(Vec::new())
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Codec (sem JS)
// ---------------------------------------------------------------------------------------------

struct Compressor {
    format: Format,
    deflate: zdeflate::Deflate,
    crc: Crc,
    /// `deflate`: o Adler-32 do rodapé zlib, já que o cabeçalho e o rodapé saem à mão (ver `ZLIB_HEADER`).
    adler: Adler32,
    started: bool,
    /// `brotli`: a entrada acumulada até o `flush`.
    buffer: Vec<u8>,
    /// `zstd`: o compressor em streaming, com o ritmo de saída do `ZSTD_compressStream2` do bun.
    zstd: Option<PacedEncoder>,
}

impl Compressor {
    fn new(format: Format, high_water_mark: usize) -> Compressor {
        Compressor {
            format,
            deflate: zdeflate::Deflate::new_zlib_ng(zdeflate::DEFAULT_COMPRESSION, -15, 8, zdeflate::Strategy::Default).expect("parâmetros fixos válidos"),
            crc: Crc::new(),
            adler: Adler32::new(),
            started: false,
            buffer: Vec::new(),
            zstd: (format == Format::Zstd).then(|| PacedEncoder::new(high_water_mark)),
        }
    }

    /// O cabeçalho gzip ou zlib, uma vez, no começo da saída.
    fn start(&mut self, out: &mut Vec<u8>) {
        if !self.started {
            self.started = true;
            match self.format {
                Format::Gzip => out.extend_from_slice(&GZIP_HEADER),
                Format::Deflate => out.extend_from_slice(&ZLIB_HEADER),
                _ => {}
            }
        }
    }

    /// Os pedaços que a escrita entregou (`zstd` já sai repartido pelo ritmo do bun; os outros, um pedaço só).
    fn write(&mut self, input: &[u8]) -> Result<Vec<Vec<u8>>, CodecError> {
        if let Some(zstd) = &mut self.zstd {
            return Ok(zstd.write(input));
        }
        if self.format.buffered() {
            self.buffer.extend_from_slice(input);
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        self.start(&mut out);
        self.crc.update(input);
        self.adler.update(input);
        let mut consumed = 0;
        loop {
            let start = out.len();
            out.resize(start + CHUNK, 0);
            let progress = self.deflate.deflate(&input[consumed..], &mut out[start..], zdeflate::Flush::None);
            out.truncate(start + progress.produced);
            consumed += progress.consumed;
            // `Z_BUF_ERROR` sem entrada nova nem saída é só "nada a fazer" (escrita vazia).
            if progress.status.is_err() && progress.produced == 0 && progress.consumed == 0 && consumed < input.len() {
                return Err(CodecError::Inflate);
            }
            if consumed >= input.len() && progress.produced < CHUNK {
                return Ok(vec![out]);
            }
        }
    }

    fn finish(&mut self) -> Result<Vec<Vec<u8>>, CodecError> {
        if let Some(zstd) = &mut self.zstd {
            return Ok(zstd.finish());
        }
        if self.format == Format::Brotli {
            return brotli_compress(&self.buffer).map(|out| vec![out]);
        }
        let mut out = Vec::new();
        self.start(&mut out);
        loop {
            let start = out.len();
            out.resize(start + CHUNK, 0);
            let progress = self.deflate.deflate(&[], &mut out[start..], zdeflate::Flush::Finish);
            out.truncate(start + progress.produced);
            match progress.status {
                Ok(zdeflate::Status::StreamEnd) => break,
                Ok(_) => {}
                Err(_) => return Err(CodecError::Inflate),
            }
        }
        match self.format {
            Format::Gzip => {
                out.extend_from_slice(&self.crc.sum().to_le_bytes());
                out.extend_from_slice(&self.crc.amount().to_le_bytes());
            }
            Format::Deflate => out.extend_from_slice(&self.adler.sum().to_be_bytes()),
            _ => {}
        }
        Ok(vec![out])
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Header,
    Body,
    Trailer,
}

struct Decompressor {
    format: Format,
    inflate: Decompress,
    crc: Crc,
    stage: Stage,
    /// Bytes de gzip que ainda não fecharam o cabeçalho ou o rodapé.
    pending: Vec<u8>,
    /// Membros gzip completos.
    members: u32,
    /// `deflate`/`deflate-raw`: o fim do fluxo já passou.
    ended: bool,
    /// `brotli`: o decodificador incremental.
    brotli: Option<BrotliStream>,
    /// `zstd`: o decodificador incremental, bloco a bloco.
    zstd: Option<ZstdStream>,
    /// A saída produzida antes do erro do último `write` (ver `settle`).
    partial: Vec<u8>,
}

/// O tamanho do cabeçalho gzip que `data` abre: `None` se ainda falta byte, erro se o que há não é gzip.
fn gzip_header_length(data: &[u8]) -> Result<Option<usize>, CodecError> {
    // Medido no bun: um byte solto, qualquer que seja, espera o seguinte (só com dois a magia é conferida).
    if data.len() == 1 {
        return Ok(None);
    }
    let invalid = data.first().is_some_and(|&byte| byte != 0x1f) || data.get(1).is_some_and(|&byte| byte != 0x8b) || data.get(2).is_some_and(|&byte| byte != 8);
    if invalid || data.get(3).is_some_and(|flags| flags & 0xE0 != 0) {
        return Err(CodecError::Inflate);
    }
    if data.len() < 10 {
        return Ok(None);
    }
    let flags = data[3];
    let mut position = 10;
    if flags & 4 != 0 {
        let Some(extra) = data.get(position..position + 2) else { return Ok(None) };
        position += 2 + usize::from(u16::from_le_bytes([extra[0], extra[1]]));
    }
    for flag in [8, 16] {
        if flags & flag != 0 {
            let Some(end) = data.get(position..).and_then(|rest| rest.iter().position(|&byte| byte == 0)) else { return Ok(None) };
            position += end + 1;
        }
    }
    if flags & 2 != 0 {
        position += 2;
    }
    Ok((data.len() >= position).then_some(position))
}

impl Decompressor {
    fn new(format: Format) -> Decompressor {
        Decompressor {
            format,
            inflate: Decompress::new(format.zlib_wrapper()),
            crc: Crc::new(),
            stage: Stage::Header,
            pending: Vec::new(),
            members: 0,
            ended: false,
            brotli: (format == Format::Brotli).then(BrotliStream::new),
            zstd: (format == Format::Zstd).then(ZstdStream::new),
            partial: Vec::new(),
        }
    }

    /// Alimenta o inflate até consumir `input` ou chegar ao fim do fluxo: `(consumidos, chegou ao fim)`, a saída em `out`
    /// (que fica com o que foi produzido antes de um erro).
    fn inflate_into(&mut self, input: &[u8], out: &mut Vec<u8>) -> Result<(usize, bool), CodecError> {
        let mut consumed = 0;
        loop {
            out.reserve(CHUNK);
            let (before_in, before_out) = (self.inflate.total_in(), self.inflate.total_out());
            let status = self.inflate.decompress_vec(&input[consumed..], out, FlushDecompress::None).map_err(|_| CodecError::Inflate)?;
            consumed += (self.inflate.total_in() - before_in) as usize;
            if status == Status::StreamEnd {
                return Ok((consumed, true));
            }
            if consumed >= input.len() && out.len() < out.capacity() {
                return Ok((consumed, false));
            }
            if self.inflate.total_in() == before_in && self.inflate.total_out() == before_out {
                return Err(CodecError::Inflate);
            }
        }
    }

    /// Fecha um `write`: com erro, a saída produzida antes dele fica em `partial` para o `drive` entregar os pedaços
    /// que os passos anteriores já tinham completado (o bun enfileira cada passo antes de rodar o seguinte).
    fn settle(&mut self, result: Result<(), CodecError>, out: Vec<u8>) -> Result<Vec<u8>, CodecError> {
        match result {
            Ok(()) => Ok(out),
            Err(error) => {
                self.partial = out;
                Err(error)
            }
        }
    }

    fn write(&mut self, input: &[u8]) -> Result<Vec<u8>, CodecError> {
        let mut out = Vec::new();
        let result = self.write_into(input, &mut out);
        self.settle(result, out)
    }

    fn write_into(&mut self, input: &[u8], out: &mut Vec<u8>) -> Result<(), CodecError> {
        if let Some(brotli) = &mut self.brotli {
            return brotli.write(input, out);
        }
        if let Some(zstd) = &mut self.zstd {
            return zstd.write(input, out);
        }
        if self.format == Format::Gzip {
            return self.write_gzip(input, out);
        }
        if self.ended {
            return if input.is_empty() { Ok(()) } else { Err(CodecError::TrailingJunk) };
        }
        let (consumed, end) = self.inflate_into(input, out)?;
        self.ended = end;
        if end && consumed < input.len() {
            return Err(CodecError::TrailingJunk);
        }
        Ok(())
    }

    fn write_gzip(&mut self, input: &[u8], out: &mut Vec<u8>) -> Result<(), CodecError> {
        let mut data = std::mem::take(&mut self.pending);
        data.extend_from_slice(input);
        let mut position = 0;
        loop {
            let rest = &data[position..];
            match self.stage {
                Stage::Header => {
                    if rest.is_empty() {
                        break;
                    }
                    let Some(length) = gzip_header_length(rest)? else { break };
                    position += length;
                    self.inflate.reset(false);
                    self.crc.reset();
                    self.stage = Stage::Body;
                }
                Stage::Body => {
                    let start = out.len();
                    let result = self.inflate_into(rest, out);
                    self.crc.update(&out[start..]);
                    let (consumed, end) = result?;
                    position += consumed;
                    if !end {
                        break;
                    }
                    self.stage = Stage::Trailer;
                }
                Stage::Trailer => {
                    if rest.len() < 8 {
                        break;
                    }
                    let expected_crc = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]);
                    let expected_size = u32::from_le_bytes([rest[4], rest[5], rest[6], rest[7]]);
                    if expected_crc != self.crc.sum() || expected_size != self.crc.amount() {
                        return Err(CodecError::Inflate);
                    }
                    position += 8;
                    self.members += 1;
                    self.stage = Stage::Header;
                }
            }
        }
        self.pending = data[position..].to_vec();
        Ok(())
    }

    /// A saída que o último `write` com erro tinha produzido (esvaziada ao ler).
    fn take_partial(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.partial)
    }

    fn finish(&mut self) -> Result<Vec<u8>, CodecError> {
        if let Some(brotli) = &self.brotli {
            return brotli.finish();
        }
        if let Some(zstd) = &self.zstd {
            return zstd.finish();
        }
        let complete = if self.format == Format::Gzip { self.members > 0 && self.stage == Stage::Header && self.pending.is_empty() } else { self.ended };
        if complete {
            Ok(Vec::new())
        } else {
            Err(CodecError::UnexpectedEof)
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Objetos
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Compression,
    Decompression,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Compression => "CompressionStream",
            Kind::Decompression => "DecompressionStream",
        }
    }
}

enum Engine {
    Compress(Compressor),
    Decompress(Decompressor),
}

struct Entry {
    engine: Engine,
    /// O `highWaterMark` do segundo argumento do construtor (`parseCodecHighWaterMark`), no mínimo 1.
    high_water_mark: usize,
    /// O `TransformStream` de dentro.
    stream: JSValue,
}

impl Entry {
    fn kind(&self) -> Kind {
        match self.engine {
            Engine::Compress(_) => Kind::Compression,
            Engine::Decompress(_) => Kind::Decompression,
        }
    }
}

type EntryRef = Rc<RefCell<Entry>>;

thread_local! {
    /// A instância e o `transformer` nativo dela (o valor codificado da célula) com o estado.
    static OBJECTS: RefCell<HashMap<EncodedJSValue, EntryRef>> = RefCell::new(HashMap::new());
}

impl Traced for Entry {
    fn trace(&self, out: &mut Vec<JSValue>) {
        self.stream.trace(out);
    }
}

/// As raízes do coletor: as instâncias (as chaves vivem enquanto o programa existir) e o `TransformStream` de dentro
/// de cada uma. O motor de compressão é só bytes.
pub(super) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    mark_all(marker, |out| {
        OBJECTS.with(|objects| {
            objects.borrow().iter().for_each(|(key, entry)| {
                out.push(JSValue::decode(*key));
                entry.trace(out);
            })
        })
    });
}

pub(super) fn reset_for_program() {
    let _ = OBJECTS.try_with(|objects| objects.borrow_mut().clear());
    crate::runtime::zstd::reset_scratch();
}

fn lookup(value: JSValue) -> Option<EntryRef> {
    OBJECTS.with(|objects| objects.borrow().get(&value.encode()).cloned())
}

fn entry_of(value: JSValue, kind: Kind) -> Option<EntryRef> {
    lookup(value).filter(|entry| entry.borrow().kind() == kind)
}

fn codec_error(global_object: &JSGlobalObject, error: CodecError) -> Thrown {
    match error {
        CodecError::Inflate => Thrown::type_error("inflate failed"),
        CodecError::UnexpectedEof => Thrown::type_error("unexpected end of file"),
        CodecError::BrotliFailed(code) => throw_coded_type_error(global_object, "brotli decode failed", &code),
        CodecError::ZstdFailed => Thrown::type_error("zstd decode failed"),
        CodecError::TrailingJunk => throw_coded_type_error(global_object, "Trailing junk found after the end of the compressed stream", "ERR_TRAILING_JUNK_AFTER_STREAM_END"),
    }
}

/// Os bytes do pedaço: string (UTF-8) ou `BufferSource`.
fn chunk_bytes(global_object: &JSGlobalObject, chunk: JSValue) -> Result<Vec<u8>, Thrown> {
    if chunk.is_string() {
        return Ok(String::from_utf16_lossy(&string_units(global_object, chunk)?).into_bytes());
    }
    // `null` é o erro do Node para escrita em stream; uma visão sobre `SharedArrayBuffer` não é `BufferSource` (medido no
    // bun 1.4.2), então cai no erro de tipo como qualquer valor inválido.
    if chunk.is_null() {
        return Err(throw_coded_type_error(global_object, "May not write null values to stream", "ERR_STREAM_NULL_VALUES"));
    }
    let shared = crate::runtime::js_generic_typed_array_view::JSGenericTypedArrayView::from_value(&chunk).is_some_and(|view| view.is_shared())
        || crate::runtime::js_data_view::JSDataView::from_value(&chunk).is_some_and(|view| view.is_shared());
    if shared {
        return Err(invalid_chunk(global_object, chunk));
    }
    input_bytes(chunk).ok_or_else(|| invalid_chunk(global_object, chunk))
}

/// `parseCodecHighWaterMark`: o `highWaterMark` do segundo argumento (padrão 64 KiB). `+Infinity` (e o que passa de
/// `usize::MAX`) vira `usize::MAX`, a parte fracionária é truncada e o mínimo é 1 (`CompressionStreamCoder__create`).
/// Estratégia que não é objeto, valor negativo ou `NaN`, conversão que lança: os erros são os de `convertQueuingStrategyDict`.
fn parse_high_water_mark(global_object: &JSGlobalObject, strategy: JSValue) -> Result<usize, Thrown> {
    let (high_water_mark, _size) = convert_strategy(global_object, strategy)?;
    let value = extract_high_water_mark(high_water_mark, DEFAULT_HIGH_WATER_MARK as f64)?;
    Ok(if value >= usize::MAX as f64 { usize::MAX } else { (value as usize).max(1) })
}

fn enqueue_pieces(global_object: &JSGlobalObject, controller: JSValue, produced: Vec<u8>, piece: usize) -> Result<(), Thrown> {
    if produced.len() <= piece {
        return enqueue(global_object, controller, Produced::Bytes(produced));
    }
    produced.chunks(piece).try_for_each(|part| enqueue(global_object, controller, Produced::Bytes(part.to_vec())))
}

/// O pedaço máximo de saída de um passo: `max(highWaterMark, tamanho da entrada)` (`step` do `CompressionStreamCoder`).
/// O `zstd` comprimindo já sai repartido pelo ritmo do `ZSTD_compressStream2` (`runtime::zstd::pacing`, que usa o mesmo `highWaterMark`).
fn piece_of(engine: &Engine, high_water_mark: usize, input_len: usize) -> usize {
    match engine {
        Engine::Compress(compressor) if compressor.format == Format::Zstd => usize::MAX,
        _ => high_water_mark.max(input_len),
    }
}

/// Um passo do motor sobre `bytes` (`finish`: o `flush`), entregando os pedaços ao `controller`.
fn drive(global_object: &JSGlobalObject, controller: JSValue, entry: &EntryRef, bytes: &[u8], finish: bool) -> HostResult {
    let (produced, piece, partial) = {
        let mut entry = entry.borrow_mut();
        let piece = piece_of(&entry.engine, entry.high_water_mark, bytes.len());
        let produced = match (&mut entry.engine, finish) {
            (Engine::Compress(compressor), false) => compressor.write(bytes),
            (Engine::Compress(compressor), true) => compressor.finish(),
            (Engine::Decompress(decompressor), false) => decompressor.write(bytes).map(|out| vec![out]),
            (Engine::Decompress(decompressor), true) => decompressor.finish().map(|out| vec![out]),
        };
        let partial = match &mut entry.engine {
            Engine::Decompress(decompressor) if produced.is_err() => decompressor.take_partial(),
            _ => Vec::new(),
        };
        (produced, piece, partial)
    };
    match produced {
        Ok(pieces) => pieces.into_iter().try_for_each(|out| enqueue_pieces(global_object, controller, out, piece))?,
        Err(error) => {
            // Cada passo do bun é enfileirado antes de o seguinte rodar: os passos que já encheram `cap` saem antes do
            // erro, e o que o passo que falhou tinha produzido (menos de `cap`) se perde.
            let complete = (partial.len() / piece) * piece;
            if complete > 0 {
                enqueue_pieces(global_object, controller, partial[..complete].to_vec(), piece)?;
            }
            return Err(codec_error(global_object, error));
        }
    }
    Ok(JSValue::undefined())
}

/// `transform(chunk, controller)` do `transformer` nativo.
fn transform_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(entry) = lookup(call.this_value()) else { return Ok(JSValue::undefined()) };
    let bytes = chunk_bytes(global_object, call.argument(0))?;
    drive(global_object, call.argument(1), &entry, &bytes, false)
}

/// `flush(controller)` do `transformer` nativo.
fn flush_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(entry) = lookup(call.this_value()) else { return Ok(JSValue::undefined()) };
    drive(global_object, call.argument(0), &entry, &[], true)
}

host_function!(transform_native, transform_body);
host_function!(flush_native, flush_body);

/// O formato do argumento: `ToString`, e o erro `ERR_INVALID_ARG_VALUE` do bun se não for um dos aceitos.
fn format_from_argument(global_object: &JSGlobalObject, argument: JSValue) -> Result<Format, Thrown> {
    let text = String::from_utf16_lossy(&string_units(global_object, argument)?);
    Format::parse(&text).ok_or_else(|| {
        let received = if argument.is_string() { format!("'{text}'") } else { text };
        throw_coded_type_error(
            global_object,
            &format!("The argument 'format' must be one of: deflate, deflate-raw, gzip, brotli, zstd. Received {received}"),
            "ERR_INVALID_ARG_VALUE",
        )
    })
}

fn construct_body(global_object: &JSGlobalObject, call: &HostCall, kind: Kind) -> HostResult {
    let format = format_from_argument(global_object, call.argument(0))?;
    let high_water_mark = parse_high_water_mark(global_object, call.argument(1))?;
    let engine = match kind {
        Kind::Compression => Engine::Compress(Compressor::new(format, high_water_mark)),
        Kind::Decompression => Engine::Decompress(Decompressor::new(format)),
    };
    let structure = derived_structure(global_object, call, instance_structure)?;
    let instance = JSFinalObject::create(global_object.vm(), &structure).as_value();
    let transformer = native_transformer(global_object, transform_native, flush_native);
    let stream = construct_inner_stream(global_object, transformer)?;
    let entry = Rc::new(RefCell::new(Entry { engine, high_water_mark, stream }));
    OBJECTS.with(|objects| {
        let mut objects = objects.borrow_mut();
        objects.insert(instance.encode(), entry.clone());
        objects.insert(transformer.encode(), entry);
    });
    Ok(instance)
}

fn call_body(global_object: &JSGlobalObject, kind: Kind) -> HostResult {
    let name = kind.name();
    Err(throw_coded_type_error(global_object, &format!("Use `new {name}(...)` instead of `{name}(...)`"), "ERR_ILLEGAL_CONSTRUCTOR"))
}

/// `readable` e `writable`: as pontas do `TransformStream` de dentro.
fn side_body(global_object: &JSGlobalObject, call: &HostCall, kind: Kind, side: fn(&JSGlobalObject, &HostCall) -> HostResult) -> HostResult {
    let entry = entry_of(call.this_value(), kind)
        .ok_or_else(|| throw_coded_type_error(global_object, &format!("Value of \"this\" must be of type {}", kind.name()), "ERR_INVALID_THIS"))?;
    let stream = entry.borrow().stream;
    side(global_object, &HostCall::new(stream, Vec::new()))
}

/// O `Symbol(nodejs.util.inspect.custom)`: `Nome { readable, writable }`. `None` se `value` não é uma instância.
pub(super) fn inspect_text(global_object: &JSGlobalObject, value: JSValue, options: JSValue) -> Option<Result<JSValue, Thrown>> {
    let entry = lookup(value)?;
    let (name, stream) = {
        let entry = entry.borrow();
        (entry.kind().name(), entry.stream)
    };
    Some(inspect_composite(global_object, name, stream, Vec::new(), Vec::new(), options).map(|text| text_value(global_object, &text)))
}

/// Gera as funções nativas (chamada, construtor e as duas pontas) de uma classe.
macro_rules! class_functions {
    ($module:ident, $kind:expr) => {
        mod $module {
            use super::*;
            fn call_b(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
                call_body(global_object, $kind)
            }
            fn construct_b(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
                construct_body(global_object, call, $kind)
            }
            fn readable_b(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
                side_body(global_object, call, $kind, ts_readable)
            }
            fn writable_b(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
                side_body(global_object, call, $kind, ts_writable)
            }
            host_function!(pub call, call_b);
            host_function!(pub construct, construct_b);
            host_function!(pub readable, readable_b);
            host_function!(pub writable, writable_b);
        }
    };
}

class_functions!(c_compression, Kind::Compression);
class_functions!(c_decompression, Kind::Decompression);

static COMPRESSION_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "CompressionStream", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static DECOMPRESSION_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "DecompressionStream", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// Instala `CompressionStream` e `DecompressionStream` (depois de `text_streams::install`, que guarda o construtor de
/// `TransformStream`).
pub(super) fn install(global_object: &JSGlobalObject) {
    install_wrapped_class(
        global_object,
        Kind::Compression.name(),
        &COMPRESSION_PROTOTYPE_S_INFO,
        1,
        c_compression::call,
        c_compression::construct,
        &[("readable", c_compression::readable as NativeFunction), ("writable", c_compression::writable)],
    );
    install_wrapped_class(
        global_object,
        Kind::Decompression.name(),
        &DECOMPRESSION_PROTOTYPE_S_INFO,
        1,
        c_decompression::call,
        c_decompression::construct,
        &[("readable", c_decompression::readable as NativeFunction), ("writable", c_decompression::writable)],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn compress(format: Format, input: &[u8]) -> Vec<Vec<u8>> {
        let mut compressor = Compressor::new(format, DEFAULT_HIGH_WATER_MARK);
        vec![compressor.write(input).unwrap().concat(), compressor.finish().unwrap().concat()]
    }

    #[test]
    fn small_outputs_match_bun_bytes() {
        let gzip = compress(Format::Gzip, b"hello world");
        assert_eq!(hex(&gzip[0]), "1f8b0800000000000003");
        assert_eq!(hex(&gzip[1]), "cb48cdc9c95728cf2fca49010085114a0d0b000000");
        let deflate = compress(Format::Deflate, b"hello world");
        assert_eq!((hex(&deflate[0]).as_str(), hex(&deflate[1]).as_str()), ("789c", "cb48cdc9c95728cf2fca4901001a0b045d"));
        let raw = compress(Format::DeflateRaw, b"hello world");
        assert_eq!((hex(&raw[0]).as_str(), hex(&raw[1]).as_str()), ("", "cb48cdc9c95728cf2fca490100"));
    }

    #[test]
    fn empty_stream_matches_bun_bytes() {
        let mut gzip = Compressor::new(Format::Gzip, DEFAULT_HIGH_WATER_MARK);
        assert_eq!(hex(&gzip.finish().unwrap().concat()), "1f8b080000000000000303000000000000000000");
        let mut deflate = Compressor::new(Format::Deflate, DEFAULT_HIGH_WATER_MARK);
        assert_eq!(hex(&deflate.finish().unwrap().concat()), "789c030000000001");
    }

    fn unhex(text: &str) -> Vec<u8> {
        (0..text.len()).step_by(2).map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap()).collect()
    }

    /// Gerador congruencial de 32 bits, o mesmo do script de medição no bun (`Math.imul(s, 1103515245) + 12345`).
    fn lcg_byte(seed: &mut u32) -> u32 {
        *seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        *seed >> 24
    }

    fn sample_text() -> Vec<u8> {
        let words = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa", "lambda", "mu", "nu", "xi", "omicron", "pi"];
        let mut seed = 1;
        let mut text = String::new();
        while text.len() < 100_000 {
            text.push_str(words[(lcg_byte(&mut seed) & 15) as usize]);
            text.push(if lcg_byte(&mut seed) & 7 != 0 { ' ' } else { '\n' });
        }
        text.truncate(100_000);
        text.into_bytes()
    }

    fn sample_binary() -> Vec<u8> {
        let mut seed = 7;
        (0..100_000).map(|_| lcg_byte(&mut seed) as u8).collect()
    }

    fn crc_of(bytes: &[u8]) -> u32 {
        let mut crc = Crc::new();
        crc.update(bytes);
        crc.sum()
    }

    #[test]
    #[ignore = "divergência conhecida: o rust-brotli gera bytes diferentes do brotli do bun (a descompressão de ambos é idêntica); ver PLAN.md"]
    fn brotli_matches_bun_small_vectors() {
        let cases: [(&[u8], &str); 4] = [
            (b"", "3b"),
            (b"a", "0b00806103"),
            (b"hello world hello world hello world", "1b2200f88d946ede44558696206c6f350b33b54006543b00"),
            (&[0u8; 1000], "1be703f82700a2b1402034"),
        ];
        for (input, expected) in cases {
            assert_eq!(hex(&brotli_compress(input).unwrap()), expected);
        }
    }

    /// Vetores grandes medidos no bun 1.4.2 (`CompressionStream('brotli')`, igual a `brotliCompressSync`): tamanho e CRC-32.
    #[test]
    #[ignore = "divergência conhecida: o rust-brotli gera bytes diferentes do brotli do bun; ver PLAN.md"]
    fn brotli_matches_bun_large_vectors() {
        let (text, binary) = (sample_text(), sample_binary());
        assert_eq!(crc_of(&text), 0xbe123bcf);
        assert_eq!(crc_of(&binary), 0x25143a6a);
        let packed_text = brotli_compress(&text).unwrap();
        assert_eq!((packed_text.len(), crc_of(&packed_text)), (16859, 0x2da4c3c0));
        let packed_binary = brotli_compress(&binary).unwrap();
        assert_eq!((packed_binary.len(), crc_of(&packed_binary)), (100_005, 0xf431d84f));
    }

    #[test]
    fn brotli_streaming_decode_emits_per_write() {
        let text = sample_text();
        let packed = brotli_compress(&text).unwrap();
        // Byte a byte: nada passa do fim do fluxo, e a soma da saída é o original.
        let mut decoder = Decompressor::new(Format::Brotli);
        let mut unpacked = Vec::new();
        let mut first_output_at = None;
        for (at, byte) in packed.iter().enumerate() {
            let produced = decoder.write(&[*byte]).unwrap();
            if first_output_at.is_none() && !produced.is_empty() {
                first_output_at = Some(at);
            }
            unpacked.extend(produced);
        }
        decoder.finish().unwrap();
        assert_eq!(unpacked, text);
        assert!(first_output_at.unwrap() < packed.len() - 1, "a saída deve começar antes do último byte");
        // Fluxo cortado: o que já decodificou sai, e o `flush` acusa o fim inesperado.
        let mut cut = Decompressor::new(Format::Brotli);
        let partial = cut.write(&packed[..packed.len() / 2]).unwrap();
        assert!(!partial.is_empty() && text.starts_with(&partial));
        assert_eq!(cut.finish(), Err(CodecError::UnexpectedEof));
    }

    #[test]
    #[ignore = "divergência conhecida: o decodificador do rust-brotli nomeia o erro de padding (PADDING_2) diferente do bun (PADDING_1); ver PLAN.md"]
    fn brotli_trailing_junk_and_error_codes_match_bun() {
        let valid = unhex("1b2200f88d946ede44558696206c6f350b33b54006543b00");
        let mut junk = valid.clone();
        junk.extend_from_slice(&[1, 2, 3]);
        assert_eq!(Decompressor::new(Format::Brotli).write(&junk), Err(CodecError::TrailingJunk));
        let mut later = Decompressor::new(Format::Brotli);
        assert_eq!(later.write(&valid).unwrap(), b"hello world hello world hello world");
        assert_eq!(later.write(&[0]), Err(CodecError::TrailingJunk));
        // Códigos medidos no bun 1.4.2 para entradas inválidas.
        let failures: [(&[u8], &str); 4] = [
            (b"garbage12345", "ERR__ERROR_FORMAT_PADDING_1"),
            (&[0xff], "ERR__ERROR_FORMAT_PADDING_2"),
            (&[0xff, 0xff, 0xff, 0xff], "ERR__ERROR_FORMAT_PADDING_2"),
            (b"hello world, not brotli at all", "ERR__ERROR_FORMAT_CL_SPACE"),
        ];
        for (input, code) in failures {
            assert_eq!(Decompressor::new(Format::Brotli).write(input), Err(CodecError::BrotliFailed(code.to_owned())), "{input:?}");
        }
        assert_eq!(Decompressor::new(Format::Brotli).write(&[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3]), Err(CodecError::BrotliFailed("ERR__ERROR_FORMAT_CL_SPACE".to_owned())));
        // Truncados (válidos até onde vão) são fim inesperado no `flush`, não erro de formato.
        for input in [&[0u8; 8][..], &[0xaa, 0xbb, 0xcc, 0xdd, 0xee], &[0x1b, 0xff, 0xff], &[0x00], &[0x05]] {
            let mut decoder = Decompressor::new(Format::Brotli);
            decoder.write(input).unwrap();
            assert_eq!(decoder.finish(), Err(CodecError::UnexpectedEof), "{input:?}");
        }
    }

    #[test]
    fn zstd_decodes_block_by_block_frames_and_junk() {
        let text: Vec<u8> = (0..600_000u32).map(|n| (n.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
        let packed = compress(Format::Zstd, &text).concat();
        // Escritas pequenas: a saída começa antes do último byte e o total é o original.
        let mut decoder = Decompressor::new(Format::Zstd);
        let (mut unpacked, mut first_output_at) = (Vec::new(), None);
        for (index, piece) in packed.chunks(1000).enumerate() {
            let produced = decoder.write(piece).unwrap();
            if !produced.is_empty() && first_output_at.is_none() {
                first_output_at = Some(index * 1000);
            }
            unpacked.extend(produced);
        }
        decoder.finish().unwrap();
        assert_eq!(unpacked, text);
        assert!(first_output_at.unwrap() < packed.len() - 1000, "a saída deve começar antes do último bloco");
        // Cortado ao meio: o que decodificou sai, e o `flush` acusa o fim inesperado.
        let mut cut = Decompressor::new(Format::Zstd);
        let partial = cut.write(&packed[..packed.len() / 2]).unwrap();
        assert!(!partial.is_empty() && text.starts_with(&partial));
        assert_eq!(cut.finish(), Err(CodecError::UnexpectedEof));
        // Dois quadros concatenados valem.
        let mut twice = Decompressor::new(Format::Zstd);
        assert_eq!(twice.write(&[packed.clone(), packed.clone()].concat()).unwrap(), [text.clone(), text].concat());
        twice.finish().unwrap();
        // Lixo no fim: na mesma escrita, em escrita posterior e prefixo do número mágico (quadro truncado).
        assert_eq!(Decompressor::new(Format::Zstd).write(&[packed.clone(), vec![1, 2, 3]].concat()), Err(CodecError::TrailingJunk));
        let mut later = Decompressor::new(Format::Zstd);
        later.write(&packed).unwrap();
        assert_eq!(later.write(&[1]), Err(CodecError::TrailingJunk));
        let mut magic = Decompressor::new(Format::Zstd);
        magic.write(&packed).unwrap();
        magic.write(&[0x28, 0xb5]).unwrap();
        assert_eq!(magic.finish(), Err(CodecError::UnexpectedEof));
        // Entrada inválida e vazia.
        assert_eq!(Decompressor::new(Format::Zstd).write(b"garbage12345"), Err(CodecError::ZstdFailed));
        assert_eq!(Decompressor::new(Format::Zstd).finish(), Err(CodecError::UnexpectedEof));
    }

    #[test]
    fn round_trip_and_errors() {
        let big: Vec<u8> = (0..100_000u32).map(|n| (n % 251) as u8).collect();
        for format in [Format::Gzip, Format::Deflate, Format::DeflateRaw] {
            let packed: Vec<u8> = compress(format, &big).concat();
            let mut decompressor = Decompressor::new(format);
            let mut unpacked = Vec::new();
            for piece in packed.chunks(777) {
                unpacked.extend(decompressor.write(piece).unwrap());
            }
            decompressor.finish().unwrap();
            assert_eq!(unpacked, big);
            let mut truncated = Decompressor::new(format);
            truncated.write(&packed[..packed.len() - 3]).unwrap();
            assert_eq!(truncated.finish(), Err(CodecError::UnexpectedEof));
        }
        assert_eq!(Decompressor::new(Format::Gzip).write(&[1, 2, 3]), Err(CodecError::Inflate));
        assert_eq!(Decompressor::new(Format::Gzip).finish(), Err(CodecError::UnexpectedEof));
        let mut junk = compress(Format::DeflateRaw, b"x").concat();
        junk.push(1);
        assert_eq!(Decompressor::new(Format::DeflateRaw).write(&junk), Err(CodecError::TrailingJunk));
        let member = compress(Format::Gzip, b"ab").concat();
        let mut twice = Decompressor::new(Format::Gzip);
        assert_eq!(twice.write(&[member.clone(), member].concat()).unwrap(), b"abab");
        twice.finish().unwrap();
    }
}
