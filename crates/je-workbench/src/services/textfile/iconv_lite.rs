//! O recorte do `iconv-lite` (`@vscode/iconv-lite-umd`) que o `encoding.ts` usa: `getDecoder`,
//! `getEncoder`, `encodingExists`, `decode` e `encode`, sobre o `encoding_rs` (WHATWG, Rust puro).
//!
//! Comportamentos do `iconv-lite` mantidos: o nome é canonizado (minúsculas, só `[0-9a-z]`); o decoder
//! descarta um U+FEFF inicial (`stripBOM`); o encoder com `addBOM` antepõe U+FEFF ao primeiro texto
//! escrito (mesmo vazio, como o `PrependBOMWrapper`); o caractere sem mapeamento sai como `?` (o `encoding_rs` emitiria uma referência numérica).
//!
//! Diferenças e pendências:
//! - o `encoding_rs` segue o WHATWG: `gb2312` e `iso885911` valem `gbk` e `windows874`, `big5hkscs` vale
//!   `big5` (que já traz o HKSCS), e `macroman` é o `macintosh`;
//! - `iso88591` é Latin-1 de verdade (feito aqui), diferente do `windows-1252` que o WHATWG chama assim;
//! - cp437, cp850, cp852, cp857 e cp865 vêm de [`super::code_pages`];
//! - PENDENTES (sem tabela): cp1125, koi8ru e koi8t, então [`encoding_exists`] devolve `false` para eles;
//! - só os apelidos listados em [`ALIASES`] são reconhecidos, não os do `iconv-lite` inteiro.

use encoding_rs::{CoderResult, Decoder as RsDecoder, Encoder as RsEncoder, EncoderResult, Encoding, UTF_16BE, UTF_16LE, UTF_8};
use je_base::errors::Error;

use super::code_pages::{self, CodePage};

const BOM: char = '\u{FEFF}';

/// Apelidos (já canonizados) do `iconv-lite` que apontam para outro nome.
const ALIASES: &[(&str, &str)] = &[
    ("ucs2", "utf16le"),
    ("latin1", "iso88591"),
    ("binary", "iso88591"),
    ("big5", "big5hkscs"),
    ("ibm866", "cp866"),
    ("macintosh", "macroman"),
    ("cp1250", "windows1250"),
    ("cp1251", "windows1251"),
    ("cp1252", "windows1252"),
    ("cp1253", "windows1253"),
    ("cp1254", "windows1254"),
    ("cp1255", "windows1255"),
    ("cp1256", "windows1256"),
    ("cp1257", "windows1257"),
    ("cp1258", "windows1258"),
    ("cp874", "windows874"),
    ("cp936", "gbk"),
];

/// Nome canônico do `iconv-lite` -> rótulo WHATWG do `encoding_rs`.
const WHATWG_LABELS: &[(&str, &str)] = &[
    ("utf8", "utf-8"),
    ("utf16le", "utf-16le"),
    ("utf16be", "utf-16be"),
    ("windows1250", "windows-1250"),
    ("windows1251", "windows-1251"),
    ("windows1252", "windows-1252"),
    ("windows1253", "windows-1253"),
    ("windows1254", "windows-1254"),
    ("windows1255", "windows-1255"),
    ("windows1256", "windows-1256"),
    ("windows1257", "windows-1257"),
    ("windows1258", "windows-1258"),
    ("windows874", "windows-874"),
    ("iso885911", "iso-8859-11"),
    ("iso88592", "iso-8859-2"),
    ("iso88593", "iso-8859-3"),
    ("iso88594", "iso-8859-4"),
    ("iso88595", "iso-8859-5"),
    ("iso88596", "iso-8859-6"),
    ("iso88597", "iso-8859-7"),
    ("iso88598", "iso-8859-8"),
    ("iso88599", "windows-1254"),
    ("iso885910", "iso-8859-10"),
    ("iso885913", "iso-8859-13"),
    ("iso885914", "iso-8859-14"),
    ("iso885915", "iso-8859-15"),
    ("iso885916", "iso-8859-16"),
    ("macroman", "macintosh"),
    ("xmaccyrillic", "x-mac-cyrillic"),
    ("koi8r", "koi8-r"),
    ("koi8u", "koi8-u"),
    ("cp866", "ibm866"),
    ("gbk", "gbk"),
    ("gb2312", "gb2312"),
    ("gb18030", "gb18030"),
    ("cp950", "big5"),
    ("big5hkscs", "big5-hkscs"),
    ("shiftjis", "shift_jis"),
    ("eucjp", "euc-jp"),
    ("euckr", "euc-kr"),
    ("iso2022jp", "iso-2022-jp"),
];

#[derive(Clone, Copy)]
enum Codec {
    Whatwg(&'static Encoding),
    Latin1,
    Base64,
    CodePage(&'static CodePage),
}

const BASE64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// O `buffer.toString('base64')` (com `=` de preenchimento).
fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let (b0, b1, b2) = (group[0], group.get(1).copied(), group.get(2).copied());
        let triple = (u32::from(b0) << 16) | (u32::from(b1.unwrap_or(0)) << 8) | u32::from(b2.unwrap_or(0));
        let sextet = |shift: u32| char::from(BASE64_ALPHABET[usize::try_from((triple >> shift) & 0x3F).unwrap_or(0)]);
        out.push(sextet(18));
        out.push(sextet(12));
        out.push(if b1.is_some() { sextet(6) } else { '=' });
        out.push(if b2.is_some() { sextet(0) } else { '=' });
    }
    out
}

/// O `Buffer.from(text, 'base64')` para um pedaço: devolve os bytes dos grupos completos de 4 caracteres do
/// alfabeto (o resto fica em `pending`); o que não é do alfabeto (inclusive `-` e `_` da variante URL) é
/// ignorado, e `=` encerra o grupo.
fn base64_decode(pending: &mut Vec<u8>, text: &str, last: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for ch in text.chars() {
        let value = match ch {
            'A'..='Z' => u32::from(ch) - u32::from('A'),
            'a'..='z' => u32::from(ch) - u32::from('a') + 26,
            '0'..='9' => u32::from(ch) - u32::from('0') + 52,
            '+' | '-' => 62,
            '/' | '_' => 63,
            _ => continue,
        };
        pending.push(u8::try_from(value).unwrap_or(0));
        if pending.len() == 4 {
            let triple = pending.iter().fold(0u32, |acc, &sextet| (acc << 6) | u32::from(sextet));
            out.extend_from_slice(&triple.to_be_bytes()[1..]);
            pending.clear();
        }
    }
    if last {
        let triple = pending.iter().fold(0u32, |acc, &sextet| (acc << 6) | u32::from(sextet)) << (6 * (4 - pending.len()));
        let produced = pending.len().saturating_sub(1);
        out.extend_from_slice(&triple.to_be_bytes()[1..=produced]);
        pending.clear();
    }
    out
}

/// `iconv._canonicalizeEncoding`.
fn canonical_name(encoding: &str) -> String {
    let lower = encoding.to_lowercase();
    let without_suffix = match lower.rsplit_once(':') {
        Some((head, tail)) if tail.len() == 4 && tail.chars().all(|c| c.is_ascii_digit()) => head,
        _ => lower.as_str(),
    };
    without_suffix.chars().filter(|c| c.is_ascii_digit() || c.is_ascii_lowercase()).collect()
}

fn resolve_codec(canonical: &str) -> Option<Codec> {
    let name = ALIASES.iter().find(|(alias, _)| *alias == canonical).map_or(canonical, |(_, target)| target);
    match name {
        "iso88591" => Some(Codec::Latin1),
        "base64" => Some(Codec::Base64),
        "cp437" => Some(Codec::CodePage(&code_pages::CP437)),
        "cp850" => Some(Codec::CodePage(&code_pages::CP850)),
        "cp852" => Some(Codec::CodePage(&code_pages::CP852)),
        "cp857" => Some(Codec::CodePage(&code_pages::CP857)),
        "cp865" => Some(Codec::CodePage(&code_pages::CP865)),
        _ => {
            let (_, label) = WHATWG_LABELS.iter().find(|(key, _)| *key == name)?;
            Encoding::for_label(label.as_bytes()).map(Codec::Whatwg)
        }
    }
}

fn get_codec(encoding: &str) -> Result<Codec, Error> {
    let canonical = canonical_name(encoding);
    resolve_codec(&canonical).ok_or_else(|| Error::new(format!("Encoding not recognized: '{encoding}' (searched as: '{canonical}')")))
}

/// `iconv.encodingExists`.
pub fn encoding_exists(encoding: &str) -> bool {
    get_codec(encoding).is_ok()
}

enum DecoderKind {
    Whatwg(RsDecoder),
    Latin1,
    /// Os bytes que sobraram de um grupo de 3 (o `StringDecoder` do `base64`).
    Base64(Vec<u8>),
    CodePage(&'static CodePage),
}

/// O decoder em fluxo do `iconv-lite` (`write` por pedaço, `end` no fim).
pub struct Decoder {
    kind: DecoderKind,
    strip_bom: bool,
    /// O `end()` do `TextDecoder` do UTF-8 devolve `''` (e não `undefined`) quando não sobra nada.
    always_text_on_end: bool,
}

fn decode_chunk(decoder: &mut RsDecoder, bytes: &[u8], last: bool) -> String {
    let mut out = String::new();
    let mut consumed = 0;
    loop {
        let needed = decoder.max_utf8_buffer_length(bytes.len() - consumed).unwrap_or(bytes.len() * 3 + 16).max(16);
        out.reserve(needed);
        let (result, read, _) = decoder.decode_to_string(&bytes[consumed..], &mut out, last);
        consumed += read;
        if matches!(result, CoderResult::InputEmpty) {
            return out;
        }
    }
}

impl Decoder {
    fn strip_leading_bom(&mut self, text: String) -> String {
        if self.strip_bom && !text.is_empty() {
            self.strip_bom = false;
            if let Some(rest) = text.strip_prefix(BOM) {
                return rest.to_string();
            }
        }
        text
    }

    pub fn write(&mut self, buffer: &[u8]) -> String {
        let text = match &mut self.kind {
            DecoderKind::Whatwg(decoder) => decode_chunk(decoder, buffer, false),
            DecoderKind::Latin1 => buffer.iter().map(|&b| char::from(b)).collect(),
            DecoderKind::Base64(pending) => {
                pending.extend_from_slice(buffer);
                let complete = pending.len() - pending.len() % 3;
                let rest = pending.split_off(complete);
                let text = base64_encode(pending);
                *pending = rest;
                text
            }
            DecoderKind::CodePage(page) => buffer.iter().map(|&b| page.decode(b)).collect(),
        };
        self.strip_leading_bom(text)
    }

    pub fn end(&mut self) -> Option<String> {
        let text = match &mut self.kind {
            DecoderKind::Whatwg(decoder) => decode_chunk(decoder, &[], true),
            DecoderKind::Base64(pending) => base64_encode(&std::mem::take(pending)),
            DecoderKind::Latin1 | DecoderKind::CodePage(_) => String::new(),
        };
        let text = self.strip_leading_bom(text);
        (self.always_text_on_end || !text.is_empty()).then_some(text)
    }
}

/// `iconv.getDecoder(encoding)`.
pub fn get_decoder(encoding: &str) -> Result<Decoder, Error> {
    let codec = get_codec(encoding)?;
    let (kind, always_text_on_end) = match codec {
        Codec::Whatwg(enc) => (DecoderKind::Whatwg(enc.new_decoder_without_bom_handling()), enc == UTF_8),
        Codec::Latin1 => (DecoderKind::Latin1, false),
        Codec::Base64 => (DecoderKind::Base64(Vec::new()), false),
        Codec::CodePage(page) => (DecoderKind::CodePage(page), false),
    };
    Ok(Decoder { kind, strip_bom: true, always_text_on_end })
}

/// `iconv.decode(buffer, encoding)`.
pub fn decode(buffer: &[u8], encoding: &str) -> Result<String, Error> {
    let mut decoder = get_decoder(encoding)?;
    let mut text = decoder.write(buffer);
    text.extend(decoder.end());
    Ok(text)
}

/// As opções de `iconv.getEncoder(encoding, options)`.
#[derive(Clone, Copy, Default)]
pub struct EncoderOptions {
    pub add_bom: bool,
}

enum EncoderKind {
    Whatwg(RsEncoder),
    Utf16 { big_endian: bool },
    Latin1,
    /// Os caracteres que sobraram de um grupo de 4.
    Base64(Vec<u8>),
    CodePage(&'static CodePage),
}

/// O encoder em fluxo do `iconv-lite`.
pub struct Encoder {
    kind: EncoderKind,
    prepend_bom: bool,
}

fn encode_chunk(encoder: &mut RsEncoder, text: &str, last: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut rest = text;
    loop {
        let needed = encoder.max_buffer_length_from_utf8_without_replacement(rest.len()).unwrap_or(rest.len() * 4 + 16).max(16);
        out.reserve(needed);
        let (result, read) = encoder.encode_from_utf8_to_vec_without_replacement(rest, &mut out, last);
        rest = &rest[read..];
        match result {
            EncoderResult::InputEmpty => return out,
            EncoderResult::OutputFull => {}
            EncoderResult::Unmappable(_) => out.push(b'?'),
        }
    }
}

impl Encoder {
    pub fn write(&mut self, text: &str) -> Vec<u8> {
        let prefixed: String;
        let text = if self.prepend_bom {
            self.prepend_bom = false;
            prefixed = format!("{BOM}{text}");
            prefixed.as_str()
        } else {
            text
        };
        match &mut self.kind {
            EncoderKind::Whatwg(encoder) => encode_chunk(encoder, text, false),
            EncoderKind::Utf16 { big_endian } => text
                .encode_utf16()
                .flat_map(|unit| if *big_endian { unit.to_be_bytes() } else { unit.to_le_bytes() })
                .collect(),
            EncoderKind::Latin1 => text.chars().map(|ch| u8::try_from(u32::from(ch)).unwrap_or(b'?')).collect(),
            EncoderKind::Base64(pending) => base64_decode(pending, text, false),
            EncoderKind::CodePage(page) => text.chars().map(|ch| page.encode(ch).unwrap_or(b'?')).collect(),
        }
    }

    /// O que sobrou do estado do encoder (o ISO-2022-JP volta ao ASCII); `None` se nada.
    pub fn end(&mut self) -> Option<Vec<u8>> {
        match &mut self.kind {
            EncoderKind::Whatwg(encoder) => Some(encode_chunk(encoder, "", true)).filter(|bytes| !bytes.is_empty()),
            EncoderKind::Base64(pending) => Some(base64_decode(pending, "", true)).filter(|bytes| !bytes.is_empty()),
            _ => None,
        }
    }
}

/// `iconv.getEncoder(encoding, options)`.
pub fn get_encoder(encoding: &str, options: Option<EncoderOptions>) -> Result<Encoder, Error> {
    let kind = match get_codec(encoding)? {
        Codec::Whatwg(enc) if enc == UTF_16LE || enc == UTF_16BE => EncoderKind::Utf16 { big_endian: enc == UTF_16BE },
        Codec::Whatwg(enc) => EncoderKind::Whatwg(enc.new_encoder()),
        Codec::Latin1 => EncoderKind::Latin1,
        Codec::Base64 => EncoderKind::Base64(Vec::new()),
        Codec::CodePage(page) => EncoderKind::CodePage(page),
    };
    Ok(Encoder { kind, prepend_bom: options.is_some_and(|o| o.add_bom) })
}

/// `iconv.encode(text, encoding, options)`.
pub fn encode(text: &str, encoding: &str, options: Option<EncoderOptions>) -> Result<Vec<u8>, Error> {
    let mut encoder = get_encoder(encoding, options)?;
    let mut bytes = encoder.write(text);
    bytes.extend(encoder.end().unwrap_or_default());
    Ok(bytes)
}
