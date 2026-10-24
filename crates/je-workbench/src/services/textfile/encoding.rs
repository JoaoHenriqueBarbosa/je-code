//! Porte de `src/vs/workbench/services/textfile/common/encoding.ts`.
//!
//! O original usa `iconv-lite` (codificações) e `jschardet` (detecção). Aqui as codificações vêm de
//! [`super::iconv_lite`] (`encoding_rs`) e a detecção do `chardetng`, ambos Rust puro.
//!
//! Diferenças de forma, sem mudança de resultado:
//! - o `import` assíncrono do `iconv-lite` era a única razão de `DecoderStream.create`, `toEncodeReadable`,
//!   `encodingExists` e `guessEncodingByBuffer` serem `async`; aqui são síncronos, e a sobrecarga de
//!   `detectEncodingFromBuffer` (síncrona sem guess, `Promise` com guess) é uma função síncrona só;
//! - a classe `DecoderStream` só repassava o decoder do `iconv-lite` e não existe: o decoder é
//!   [`Decoder`] (`IDecoderStream`);
//! - um `DecodeStreamError` viaja como [`Error`] de nome `DecodeStreamError` com a propriedade
//!   `decodeStreamErrorKind`; [`DecodeStreamError::from_error`] é o `instanceof`.
//!
//! Diferenças de comportamento:
//! - o `jschardet` não existe em Rust: o `chardetng` o substitui, e a confiança dele dita o resultado.
//!   Texto só ASCII dá `ascii` (ignorado); UTF-8 válido com bytes altos dá `utf8`; um palpite sem
//!   confiança cai em `windows-1252` (o que o prober Latin-1 do `jschardet` fazia). Com candidatos, só
//!   vale um palpite confiante que esteja entre eles; senão vale o recuo por candidatos do original.
//!   Os casos que dependem da estatística do `jschardet` (textos curtos) podem divergir: conferir nos testes;
//! - PENDENTES: cp1125, koi8ru e koi8t (ver [`super::iconv_lite`]); `chardetng` não detecta ISO-2022-JP
//!   (o `jschardet` o reportava por sequências ESC).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use chardetng::EncodingDetector;
use encoding_rs::UTF_8;
use je_base::buffer::{VSBuffer, VSBufferReadableStream};
use je_base::cancellation::CancellationTokenSource;
use je_base::errors::Error;
use je_base::promise::{Promise, Resolver};
use je_base::runtime;
use je_base::stream::{IStreamListener, Readable, ReadableStream, WriteableStream, WriteableStreamImpl, listen_stream, new_writeable_stream};

use super::iconv_lite::{self, Decoder, EncoderOptions};

pub const UTF8: &str = "utf8";
pub const UTF8_WITH_BOM: &str = "utf8bom";
pub const UTF16BE: &str = "utf16be";
pub const UTF16LE: &str = "utf16le";

pub fn is_utf_encoding(encoding: &str) -> bool {
    [UTF8, UTF8_WITH_BOM, UTF16BE, UTF16LE].contains(&encoding)
}

pub const UTF16BE_BOM: [u8; 2] = [0xFE, 0xFF];
pub const UTF16LE_BOM: [u8; 2] = [0xFF, 0xFE];
pub const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

const ZERO_BYTE_DETECTION_BUFFER_MAX_LEN: usize = 512; // number of bytes to look at to decide about a file being binary or not
const NO_ENCODING_GUESS_MIN_BYTES: usize = 512; // when not auto guessing the encoding, small number of bytes are enough
const AUTO_ENCODING_GUESS_MIN_BYTES: usize = 512 * 8; // with auto guessing we want a lot more content to be read for guessing
const AUTO_ENCODING_GUESS_MAX_BYTES: usize = 512 * 128; // set an upper limit for the number of bytes we pass on to the detector

/// `IDecoderStream`.
pub type IDecoderStream = Decoder;

/// `IDecodeStreamOptions`.
pub struct DecodeStreamOptions {
    pub accept_text_only: bool,
    pub guess_encoding: bool,
    pub candidate_guess_encodings: Vec<String>,
    pub min_bytes_required_for_detection: Option<usize>,
    pub overwrite_encoding: Rc<dyn Fn(Option<String>) -> Promise<String>>,
}

/// `IDecodeStreamResult`.
#[derive(Clone)]
pub struct DecodeStreamResult {
    pub stream: Rc<dyn ReadableStream<String>>,
    pub detected: DetectedEncodingResult,
}

/// `DecodeStreamErrorKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeStreamErrorKind {
    /// Error indicating that the stream is binary even though `acceptTextOnly` was specified.
    StreamIsBinary = 1,
}

const DECODE_STREAM_ERROR_NAME: &str = "DecodeStreamError";
const DECODE_STREAM_ERROR_KIND_PROP: &str = "decodeStreamErrorKind";

/// `DecodeStreamError`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeStreamError {
    pub message: String,
    pub decode_stream_error_kind: DecodeStreamErrorKind,
}

impl DecodeStreamError {
    pub fn new(message: impl Into<String>, decode_stream_error_kind: DecodeStreamErrorKind) -> Self {
        Self { message: message.into(), decode_stream_error_kind }
    }

    /// O `error instanceof DecodeStreamError`.
    pub fn from_error(error: &Error) -> Option<Self> {
        if error.name != DECODE_STREAM_ERROR_NAME {
            return None;
        }
        let kind = match error.prop(DECODE_STREAM_ERROR_KIND_PROP)? {
            "1" => DecodeStreamErrorKind::StreamIsBinary,
            _ => return None,
        };
        Some(Self::new(error.message.clone(), kind))
    }
}

impl From<DecodeStreamError> for Error {
    fn from(error: DecodeStreamError) -> Self {
        Error::named(DECODE_STREAM_ERROR_NAME, error.message).with_prop(DECODE_STREAM_ERROR_KIND_PROP, error.decode_stream_error_kind as i32)
    }
}

struct DecodeContext {
    source: VSBufferReadableStream,
    target: Rc<WriteableStreamImpl<String>>,
    options: DecodeStreamOptions,
    min_bytes_required_for_detection: usize,
    buffered_chunks: RefCell<Vec<VSBuffer>>,
    bytes_buffered: Cell<usize>,
    decoder: RefCell<Option<Decoder>>,
    cancellation: CancellationTokenSource,
    resolver: Resolver<DecodeStreamResult>,
}

async fn try_create_decoder(ctx: &Rc<DecodeContext>) -> Result<(), Error> {
    // detect encoding from buffer
    let buffer = VSBuffer::concat(&ctx.buffered_chunks.borrow(), None);
    let mut detected = detect_encoding_from_buffer(
        &ReadResult { buffer: Some(buffer), bytes_read: ctx.bytes_buffered.get() },
        ctx.options.guess_encoding,
        Some(&ctx.options.candidate_guess_encodings),
    );

    // throw early if the source seems binary and we are instructed to only accept text
    if detected.seems_binary && ctx.options.accept_text_only {
        return Err(DecodeStreamError::new("Stream is binary but only text is accepted for decoding", DecodeStreamErrorKind::StreamIsBinary).into());
    }

    // ensure to respect overwrite of encoding
    let encoding = (ctx.options.overwrite_encoding)(detected.encoding.take()).await?;
    detected.encoding = Some(encoding.clone());

    // decode and write buffered content
    let mut decoder = iconv_lite::get_decoder(to_node_encoding(Some(&encoding)))?;
    let buffered = VSBuffer::concat(&ctx.buffered_chunks.borrow(), None);
    let decoded = buffered.with_bytes(|bytes| decoder.write(bytes));
    *ctx.decoder.borrow_mut() = Some(decoder);
    ctx.target.write(decoded);

    ctx.buffered_chunks.borrow_mut().clear();
    ctx.bytes_buffered.set(0);

    // signal to the outside our detected encoding and final decoder stream
    ctx.resolver.resolve(DecodeStreamResult { stream: ctx.target.clone(), detected });
    Ok(())
}

async fn create_decoder(ctx: &Rc<DecodeContext>) {
    if let Err(error) = try_create_decoder(ctx).await {
        // Stop handling anything from the source and target
        ctx.cancellation.cancel();
        ctx.target.destroy();

        ctx.resolver.reject(error);
    }
}

async fn on_chunk(ctx: &Rc<DecodeContext>, chunk: VSBuffer) {
    // if the decoder is ready, we just write directly
    let decoded = ctx.decoder.borrow_mut().as_mut().map(|decoder| chunk.with_bytes(|bytes| decoder.write(bytes)));
    if let Some(decoded) = decoded {
        ctx.target.write(decoded);
        return;
    }

    // otherwise we need to buffer the data until the stream is ready
    ctx.bytes_buffered.set(ctx.bytes_buffered.get() + chunk.byte_length());
    ctx.buffered_chunks.borrow_mut().push(chunk);

    // buffered enough data for encoding detection, create stream
    if ctx.bytes_buffered.get() >= ctx.min_bytes_required_for_detection {
        // pause stream here until the decoder is ready
        ctx.source.pause();

        create_decoder(ctx).await;

        // resume stream now that decoder is ready but outside of this stack to reduce recursion
        let source = ctx.source.clone();
        runtime::set_timeout(move || source.resume(), 0);
    }
}

async fn on_end(ctx: &Rc<DecodeContext>) {
    // we were still waiting for data to do the encoding detection. thus, wrap up starting the stream
    // even without all the data to get things going
    if ctx.decoder.borrow().is_none() {
        create_decoder(ctx).await;
    }

    // end the target with the remainders of the decoder
    let remainder = ctx.decoder.borrow_mut().as_mut().and_then(Decoder::end);
    ctx.target.end(remainder);
}

pub fn to_decode_stream(source: VSBufferReadableStream, options: DecodeStreamOptions) -> Promise<DecodeStreamResult> {
    let min_bytes_required_for_detection = options
        .min_bytes_required_for_detection
        .unwrap_or(if options.guess_encoding { AUTO_ENCODING_GUESS_MIN_BYTES } else { NO_ENCODING_GUESS_MIN_BYTES });

    let (promise, resolver) = Promise::new();
    let cancellation = CancellationTokenSource::new(None);
    let token = cancellation.token();
    let ctx = Rc::new(DecodeContext {
        source,
        target: new_writeable_stream::<String>(Some(Rc::new(|strings: Vec<String>| strings.concat())), None),
        options,
        min_bytes_required_for_detection,
        buffered_chunks: RefCell::new(Vec::new()),
        bytes_buffered: Cell::new(0),
        decoder: RefCell::new(None),
        cancellation,
        resolver,
    });

    let (data_ctx, error_ctx, end_ctx) = (ctx.clone(), ctx.clone(), ctx.clone());
    listen_stream(
        &*ctx.source,
        IStreamListener {
            on_data: Rc::new(move |chunk: &VSBuffer| {
                let (ctx, chunk) = (data_ctx.clone(), chunk.clone());
                runtime::spawn_local(async move { on_chunk(&ctx, chunk).await });
            }),
            on_error: Rc::new(move |error: &Error| error_ctx.target.error(error.clone())), // simply forward to target
            on_end: Rc::new(move || {
                let ctx = end_ctx.clone();
                runtime::spawn_local(async move { on_end(&ctx).await });
            }),
        },
        Some(&token),
    );

    promise
}

/// A BOM que o `toEncodeReadable` devolve sozinho quando nada foi escrito.
fn bom_for(encoding: &str) -> Option<&'static [u8]> {
    match encoding {
        UTF8 | UTF8_WITH_BOM => Some(&UTF8_BOM[..]),
        UTF16BE => Some(&UTF16BE_BOM[..]),
        UTF16LE => Some(&UTF16LE_BOM[..]),
        _ => None,
    }
}

/// As opções de `toEncodeReadable` (`{ addBOM? }`).
pub type EncodeOptions = EncoderOptions;

pub fn to_encode_readable<R: Readable<String> + 'static>(mut readable: R, encoding: &str, options: Option<EncodeOptions>) -> Result<impl Readable<VSBuffer> + use<R>, Error> {
    let mut encoder = iconv_lite::get_encoder(to_node_encoding(Some(encoding)), options)?;
    let add_bom = options.is_some_and(|o| o.add_bom);
    let encoding = encoding.to_string();

    let mut bytes_written = false;
    let mut done = false;

    Ok(move || {
        if done {
            return None;
        }

        let Some(chunk) = readable.read() else {
            done = true;

            // If we are instructed to add a BOM but we detect that no bytes have been written, we must
            // ensure to return the BOM ourselves so that we comply with the contract.
            if !bytes_written && add_bom && let Some(bom) = bom_for(&encoding) {
                return Some(VSBuffer::wrap(bom.to_vec()));
            }

            return encoder.end().map(VSBuffer::wrap);
        };

        bytes_written = true;

        Some(VSBuffer::wrap(encoder.write(&chunk)))
    })
}

pub fn encoding_exists(encoding: &str) -> bool {
    iconv_lite::encoding_exists(to_node_encoding(Some(encoding)))
}

pub fn to_node_encoding(enc: Option<&str>) -> &str {
    match enc {
        // iconv does not distinguish UTF 8 with or without BOM, so we need to help it
        None | Some(UTF8_WITH_BOM) => UTF8,
        Some(enc) => enc,
    }
}

pub fn detect_encoding_by_bom_from_buffer(buffer: Option<&VSBuffer>, bytes_read: usize) -> Option<&'static str> {
    let buffer = buffer?;
    if bytes_read < UTF16BE_BOM.len() {
        return None;
    }

    let b0 = buffer.read_u_int8(0);
    let b1 = buffer.read_u_int8(1);

    // UTF-16 BE
    if b0 == UTF16BE_BOM[0] && b1 == UTF16BE_BOM[1] {
        return Some(UTF16BE);
    }

    // UTF-16 LE
    if b0 == UTF16LE_BOM[0] && b1 == UTF16LE_BOM[1] {
        return Some(UTF16LE);
    }

    if bytes_read < UTF8_BOM.len() {
        return None;
    }

    let b2 = buffer.read_u_int8(2);

    // UTF-8
    if b0 == UTF8_BOM[0] && b1 == UTF8_BOM[1] && b2 == UTF8_BOM[2] {
        return Some(UTF8_WITH_BOM);
    }

    None
}

// we explicitly ignore a specific set of encodings from auto guessing
// - ASCII: we never want this encoding (most UTF-8 files would happily detect as
//          ASCII files and then you could not type non-ASCII characters anymore)
// - UTF-16: we have our own detection logic for UTF-16
// - UTF-32: we do not support this encoding in VSCode
const IGNORE_ENCODINGS: [&str; 3] = ["ascii", "utf-16", "utf-32"];

/// O `jschardet.detect`: o nome (no estilo do `jschardet`) que o `chardetng` acha, ou `None` se o
/// `jschardet` não acharia nada. `candidates` são os nomes `guessableName`, e restringem o resultado.
fn detect_with_chardet(bytes: &[u8], candidates: Option<&[&'static str]>) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    if bytes.is_ascii() {
        return Some("ascii".to_string());
    }

    let mut detector = EncodingDetector::new();
    detector.feed(bytes, false);
    let (encoding, confident) = detector.guess_assess(None, true);

    if confident || encoding == UTF_8 {
        let guessed = to_iconv_lite_encoding(encoding.name());
        return match candidates {
            None => Some(encoding.name().to_string()),
            // GB2312 é um subconjunto do GBK, que é o que o `chardetng` reporta.
            Some(list) => list
                .iter()
                .find(|candidate| {
                    let candidate = to_iconv_lite_encoding(candidate);
                    candidate == guessed || (candidate == "gb2312" && (guessed == "gbk" || guessed == "gb18030"))
                })
                .map(|candidate| (*candidate).to_string()),
        };
    }

    // Sem confiança em nenhum prober: o `jschardet` ficava com o Latin-1 (windows-1252).
    match candidates {
        None => Some("windows-1252".to_string()),
        Some(list) => list.iter().find(|candidate| to_iconv_lite_encoding(candidate) == "windows1252").map(|candidate| (*candidate).to_string()),
    }
}

/// Guesses the encoding from buffer.
fn guess_encoding_by_buffer(buffer: &VSBuffer, candidate_guess_encodings: Option<&[String]>) -> Option<String> {
    // ensure to limit buffer for guessing
    let limited_buffer = buffer.slice(Some(0), i64::try_from(AUTO_ENCODING_GUESS_MAX_BYTES).ok());

    // ensure to convert candidate encodings to detector encoding names if provided
    let candidates: Option<Vec<&'static str>> = candidate_guess_encodings
        .map(|list| list.iter().filter_map(|e| to_jschardet_encoding(e)).collect::<Vec<_>>())
        .filter(|list| !list.is_empty());

    let Some(guessed) = limited_buffer.with_bytes(|bytes| detect_with_chardet(bytes, candidates.as_deref())) else {
        return candidates.and_then(|list| guess_encoding_by_candidates(&limited_buffer, &list));
    };

    if IGNORE_ENCODINGS.contains(&guessed.to_lowercase().as_str()) {
        return None; // see comment above why we ignore some encodings
    }

    Some(to_iconv_lite_encoding(&guessed))
}

fn guess_encoding_by_candidates(buffer: &VSBuffer, candidates: &[&str]) -> Option<String> {
    let bytes = buffer.to_vec();

    // The sample may end in the middle of a UTF-8 character.
    match std::str::from_utf8(&bytes) {
        Ok(_) => return None,
        Err(error) if error.error_len().is_none() => return None,
        Err(_) => {}
    }

    let mut guessed_encoding: Option<String> = None;

    for candidate in candidates {
        let encoding = to_iconv_lite_encoding(candidate);
        if is_utf_encoding(&encoding) {
            continue;
        }

        // Candidato sem codec (as pendências de `iconv_lite`) não prova nada.
        let Ok(decoded) = iconv_lite::decode(&bytes, &encoding) else { continue };
        let Ok(encoded) = iconv_lite::encode(&decoded, &encoding, None) else { continue };
        if encoded != bytes {
            continue;
        }

        if guessed_encoding.as_deref().is_some_and(|guessed| guessed != encoding) {
            return None;
        }

        guessed_encoding = Some(encoding);
    }

    guessed_encoding
}

/// Nomes de `chardetng` (WHATWG) e do `jschardet` que o `iconv-lite` chama de outra coisa.
const JSCHARDET_TO_ICONV_ENCODINGS: [(&str, &str); 3] = [("ibm866", "cp866"), ("big5", "cp950"), ("macintosh", "macroman")];

fn normalize_encoding(encoding_name: &str) -> String {
    encoding_name.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_lowercase()
}

fn to_iconv_lite_encoding(encoding_name: &str) -> String {
    let normalized = normalize_encoding(encoding_name);
    JSCHARDET_TO_ICONV_ENCODINGS.iter().find(|(from, _)| *from == normalized).map_or(normalized.clone(), |(_, to)| (*to).to_string())
}

fn to_jschardet_encoding(encoding_name: &str) -> Option<&'static str> {
    let normalized = normalize_encoding(encoding_name);
    guessable_encodings().find(|encoding| encoding.key == normalized).and_then(|encoding| encoding.guessable_name)
}

/// The encodings that are allowed in a settings file don't match the canonical encoding labels specified
/// by WHATWG. See https://encoding.spec.whatwg.org/#names-and-labels
/// Iconv-lite strips all non-alphanumeric characters, but ripgrep doesn't. For backcompat, allow these labels.
pub fn to_canonical_name(enc: &str) -> String {
    match enc {
        "shiftjis" => "shift-jis".to_string(),
        "utf16le" => "utf-16le".to_string(),
        "utf16be" => "utf-16be".to_string(),
        "big5hkscs" => "big5-hkscs".to_string(),
        "eucjp" => "euc-jp".to_string(),
        "euckr" => "euc-kr".to_string(),
        "koi8r" => "koi8-r".to_string(),
        "koi8u" => "koi8-u".to_string(),
        "macroman" => "x-mac-roman".to_string(),
        "utf8bom" => "utf8".to_string(),
        _ => match windows_code_page_digits(enc) {
            Some(digits) => format!("windows-{digits}"),
            None => enc.to_string(),
        },
    }
}

/// O `enc.match(/windows(\d+)/)`: os dígitos depois da primeira ocorrência de `windows` seguida de dígito.
fn windows_code_page_digits(enc: &str) -> Option<&str> {
    enc.match_indices("windows").find_map(|(index, word)| {
        let rest = &enc[index + word.len()..];
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        (digits > 0).then(|| &rest[..digits])
    })
}

/// `IDetectedEncodingResult`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetectedEncodingResult {
    pub encoding: Option<String>,
    pub seems_binary: bool,
}

/// `IReadResult`.
pub struct ReadResult {
    pub buffer: Option<VSBuffer>,
    pub bytes_read: usize,
}

/// Procura o zero byte dos 512 primeiros bytes: devolve o UTF-16 que isso sugere, ou se parece binário.
fn detect_utf16_or_binary(bytes: &[u8], bytes_read: usize) -> (Option<&'static str>, bool) {
    let mut could_be_utf16le = true; // e.g. 0xAA 0x00
    let mut could_be_utf16be = true; // e.g. 0x00 0xAA
    let mut contains_zero_byte = false;

    // This is a simplified guess to detect UTF-16 BE or LE by just checking if the first 512 bytes have
    // the 0-byte at a specific location. For UTF-16 LE this would be the odd byte index and for UTF-16 BE
    // the even one.
    // Note: this can produce false positives (a binary file that uses a 2-byte encoding of the same format
    // as UTF-16) and false negatives (a UTF-16 file that is using 4 bytes to encode a character).
    for (i, &byte) in bytes.iter().enumerate().take(bytes_read.min(ZERO_BYTE_DETECTION_BUFFER_MAX_LEN)) {
        let is_endian = i % 2 == 1; // assume 2-byte sequences typical for UTF-16
        let is_zero_byte = byte == 0;

        if is_zero_byte {
            contains_zero_byte = true;
        }

        // UTF-16 LE: expect e.g. 0xAA 0x00
        if could_be_utf16le && (is_endian != is_zero_byte) {
            could_be_utf16le = false;
        }

        // UTF-16 BE: expect e.g. 0x00 0xAA
        if could_be_utf16be && (is_endian == is_zero_byte) {
            could_be_utf16be = false;
        }

        // Return if this is neither UTF16-LE nor UTF16-BE and thus treat as binary
        if is_zero_byte && !could_be_utf16le && !could_be_utf16be {
            break;
        }
    }

    // Handle case of 0-byte included
    if !contains_zero_byte {
        (None, false)
    } else if could_be_utf16le {
        (Some(UTF16LE), false)
    } else if could_be_utf16be {
        (Some(UTF16BE), false)
    } else {
        (None, true)
    }
}

pub fn detect_encoding_from_buffer(read_result: &ReadResult, auto_guess_encoding: bool, candidate_guess_encodings: Option<&[String]>) -> DetectedEncodingResult {
    let ReadResult { buffer, bytes_read } = read_result;

    // Always first check for BOM to find out about encoding
    let mut encoding = detect_encoding_by_bom_from_buffer(buffer.as_ref(), *bytes_read);

    // Detect 0 bytes to see if file is binary or UTF-16 LE/BE unless we already know that this file has a
    // UTF-16 encoding
    let mut seems_binary = false;
    if !matches!(encoding, Some(UTF16BE | UTF16LE))
        && let Some(buffer) = buffer
    {
        let (utf16, binary) = buffer.with_bytes(|bytes| detect_utf16_or_binary(bytes, *bytes_read));
        encoding = utf16.or(encoding);
        seems_binary = binary;
    }

    // Auto guess encoding if configured
    if auto_guess_encoding
        && !seems_binary
        && encoding.is_none()
        && let Some(buffer) = buffer
    {
        let sample = buffer.slice(Some(0), Some(i64::try_from(*bytes_read).unwrap_or(i64::MAX)));
        return DetectedEncodingResult { seems_binary: false, encoding: guess_encoding_by_buffer(&sample, candidate_guess_encodings) };
    }

    DetectedEncodingResult { seems_binary, encoding: encoding.map(str::to_string) }
}

/// Um item de `SUPPORTED_ENCODINGS` (`EncodingsMap`), com a chave no campo `key`.
#[derive(Clone, Copy, Debug)]
pub struct SupportedEncoding {
    pub key: &'static str,
    pub label_long: &'static str,
    pub label_short: &'static str,
    pub order: u32,
    pub encode_only: bool,
    pub alias: Option<&'static str>,
    pub guessable_name: Option<&'static str>,
}

impl SupportedEncoding {
    const fn new(key: &'static str, label_long: &'static str, label_short: &'static str, order: u32) -> Self {
        Self { key, label_long, label_short, order, encode_only: false, alias: None, guessable_name: None }
    }

    const fn alias(mut self, alias: &'static str) -> Self {
        self.alias = Some(alias);
        self
    }

    const fn guessable(mut self, name: &'static str) -> Self {
        self.guessable_name = Some(name);
        self
    }

    const fn encode_only(mut self) -> Self {
        self.encode_only = true;
        self
    }
}

/// Na ordem de declaração do original.
pub const SUPPORTED_ENCODINGS: &[SupportedEncoding] = &[
    SupportedEncoding::new("utf8", "UTF-8", "UTF-8", 1).alias("utf8bom").guessable("UTF-8"),
    SupportedEncoding::new("utf8bom", "UTF-8 with BOM", "UTF-8 with BOM", 2).encode_only().alias("utf8"),
    SupportedEncoding::new("utf16le", "UTF-16 LE", "UTF-16 LE", 3).guessable("UTF-16LE"),
    SupportedEncoding::new("utf16be", "UTF-16 BE", "UTF-16 BE", 4).guessable("UTF-16BE"),
    SupportedEncoding::new("windows1252", "Western (Windows 1252)", "Windows 1252", 5).guessable("windows-1252"),
    SupportedEncoding::new("iso88591", "Western (ISO 8859-1)", "ISO 8859-1", 6),
    SupportedEncoding::new("iso88593", "Western (ISO 8859-3)", "ISO 8859-3", 7),
    SupportedEncoding::new("iso885915", "Western (ISO 8859-15)", "ISO 8859-15", 8),
    SupportedEncoding::new("macroman", "Western (Mac Roman)", "Mac Roman", 9),
    SupportedEncoding::new("cp437", "DOS (CP 437)", "CP437", 10),
    SupportedEncoding::new("windows1256", "Arabic (Windows 1256)", "Windows 1256", 11),
    SupportedEncoding::new("iso88596", "Arabic (ISO 8859-6)", "ISO 8859-6", 12),
    SupportedEncoding::new("windows1257", "Baltic (Windows 1257)", "Windows 1257", 13),
    SupportedEncoding::new("iso88594", "Baltic (ISO 8859-4)", "ISO 8859-4", 14),
    SupportedEncoding::new("iso885914", "Celtic (ISO 8859-14)", "ISO 8859-14", 15),
    SupportedEncoding::new("windows1250", "Central European (Windows 1250)", "Windows 1250", 16).guessable("windows-1250"),
    SupportedEncoding::new("iso88592", "Central European (ISO 8859-2)", "ISO 8859-2", 17).guessable("ISO-8859-2"),
    SupportedEncoding::new("cp852", "Central European (CP 852)", "CP 852", 18),
    SupportedEncoding::new("windows1251", "Cyrillic (Windows 1251)", "Windows 1251", 19).guessable("windows-1251"),
    SupportedEncoding::new("cp866", "Cyrillic (CP 866)", "CP 866", 20).guessable("IBM866"),
    SupportedEncoding::new("cp1125", "Cyrillic (CP 1125)", "CP 1125", 21).guessable("IBM1125"),
    SupportedEncoding::new("iso88595", "Cyrillic (ISO 8859-5)", "ISO 8859-5", 22).guessable("ISO-8859-5"),
    SupportedEncoding::new("koi8r", "Cyrillic (KOI8-R)", "KOI8-R", 23).guessable("KOI8-R"),
    SupportedEncoding::new("koi8u", "Cyrillic (KOI8-U)", "KOI8-U", 24),
    SupportedEncoding::new("iso885913", "Estonian (ISO 8859-13)", "ISO 8859-13", 25),
    SupportedEncoding::new("windows1253", "Greek (Windows 1253)", "Windows 1253", 26).guessable("windows-1253"),
    SupportedEncoding::new("iso88597", "Greek (ISO 8859-7)", "ISO 8859-7", 27).guessable("ISO-8859-7"),
    SupportedEncoding::new("windows1255", "Hebrew (Windows 1255)", "Windows 1255", 28).guessable("windows-1255"),
    SupportedEncoding::new("iso88598", "Hebrew (ISO 8859-8)", "ISO 8859-8", 29).guessable("ISO-8859-8"),
    SupportedEncoding::new("iso885910", "Nordic (ISO 8859-10)", "ISO 8859-10", 30),
    SupportedEncoding::new("iso885916", "Romanian (ISO 8859-16)", "ISO 8859-16", 31),
    SupportedEncoding::new("windows1254", "Turkish (Windows 1254)", "Windows 1254", 32),
    SupportedEncoding::new("iso88599", "Turkish (ISO 8859-9)", "ISO 8859-9", 33),
    SupportedEncoding::new("cp857", "Turkish (CP 857)", "CP 857", 34),
    SupportedEncoding::new("windows1258", "Vietnamese (Windows 1258)", "Windows 1258", 35),
    SupportedEncoding::new("gbk", "Simplified Chinese (GBK)", "GBK", 36),
    SupportedEncoding::new("gb18030", "Simplified Chinese (GB18030)", "GB18030", 37),
    SupportedEncoding::new("cp950", "Traditional Chinese (Big5)", "Big5", 38).guessable("Big5"),
    SupportedEncoding::new("big5hkscs", "Traditional Chinese (Big5-HKSCS)", "Big5-HKSCS", 39),
    SupportedEncoding::new("shiftjis", "Japanese (Shift JIS)", "Shift JIS", 40).guessable("SHIFT_JIS"),
    SupportedEncoding::new("eucjp", "Japanese (EUC-JP)", "EUC-JP", 41).guessable("EUC-JP"),
    SupportedEncoding::new("euckr", "Korean (EUC-KR)", "EUC-KR", 42).guessable("EUC-KR"),
    SupportedEncoding::new("windows874", "Thai (Windows 874)", "Windows 874", 43),
    SupportedEncoding::new("iso885911", "Latin/Thai (ISO 8859-11)", "ISO 8859-11", 44),
    SupportedEncoding::new("koi8ru", "Cyrillic (KOI8-RU)", "KOI8-RU", 45),
    SupportedEncoding::new("koi8t", "Tajik (KOI8-T)", "KOI8-T", 46),
    SupportedEncoding::new("gb2312", "Simplified Chinese (GB 2312)", "GB 2312", 47).guessable("GB2312"),
    SupportedEncoding::new("cp865", "Nordic DOS (CP 865)", "CP 865", 48),
    SupportedEncoding::new("cp850", "Western European DOS (CP 850)", "CP 850", 49),
];

/// `GUESSABLE_ENCODINGS`: os de `SUPPORTED_ENCODINGS` que têm `guessableName`.
pub fn guessable_encodings() -> impl Iterator<Item = &'static SupportedEncoding> {
    SUPPORTED_ENCODINGS.iter().filter(|encoding| encoding.guessable_name.is_some())
}
