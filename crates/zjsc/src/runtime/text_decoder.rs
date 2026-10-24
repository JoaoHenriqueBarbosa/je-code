//! `TextDecoder` do global. O JavaScriptCore não o define: quem o instala é o bun (WebCore), como propriedade
//! de dados comum (`writable`, `enumerable`, `configurable`). Medido no bun 1.4.2 (golden em
//! `tests/golden/text_decoder_bun.tsv`):
//!
//! - o construtor é nativo, `length` 0, com `length`, `name` e `prototype` como únicas chaves próprias;
//! - `TextDecoder.prototype` herda de `Object.prototype` e tem, nesta ordem: `decode` (`length` 1, gravável,
//!   enumerável, não configurável), os acessores `encoding`, `fatal` e `ignoreBOM` (enumeráveis, não configuráveis, sem
//!   setter, getters nativos `get encoding`...), `constructor` (não enumerável) e `@@toStringTag` "TextDecoder";
//! - `new TextDecoder(label, options)`: o rótulo `undefined` é "utf-8", o resto passa por `ToString`; o rótulo
//!   perde espaço ASCII nas pontas (tab, LF, FF, CR, espaço) e vira minúsculo ASCII antes de casar com a tabela
//!   do WHATWG Encoding; rótulo desconhecido é `RangeError` `Unsupported encoding label "x"` (a mensagem repete o
//!   rótulo original) com `code` `ERR_ENCODING_NOT_SUPPORTED`, conferido antes das `options`. `options` que não é
//!   objeto (nem `undefined`/`null`) é `TypeError` `The "options" argument must be of type object. Received ...`
//!   (`ERR_INVALID_ARG_TYPE`); as chaves `fatal` e `ignoreBOM` são lidas nesta ordem e coagidas para booleano;
//! - sem `new`: `TypeError` "TextDecoder constructor cannot be invoked without 'new'" (`ERR_ILLEGAL_CONSTRUCTOR`);
//!   `this` inválido: `decode` lança `Expected this to be instanceof TextDecoder, but received ...`
//!   (`ERR_INVALID_THIS`) e os getters `The TextDecoder.<nome> getter can only be used on instances of
//!   TextDecoder` sem `code`;
//! - `decode(input, options)`: confere o `this`, depois as `options` (e lê `stream`), depois o `input`.
//!   `undefined` é vazio; `ArrayBuffer`, `SharedArrayBuffer`, `DataView` e qualquer typed array servem (visão
//!   destacada ou fora dos limites é vazia); o resto é `TypeError` "TextDecoder.decode expects an ArrayBuffer or
//!   TypedArray" (`ERR_INVALID_ARG_TYPE`);
//! - o BOM inicial é removido a menos de `ignoreBOM`, só no começo do fluxo: o estado "já vi dados" liga com o
//!   primeiro bloco de bytes não vazio completo (bytes presos de uma sequência incompleta não contam), desliga
//!   no `decode` sem `stream` e não é tocado por um erro; no `fatal` o erro é `TypeError` "The encoded data was
//!   not valid for encoding <nome>" (`ERR_ENCODING_INVALID_ENCODED_DATA`) e descarta os bytes presos;
//! - `stream: true` prende o fim incompleto (UTF-8: prefixo de sequência válida; UTF-16: byte ímpar e unidade
//!   substituta alta final), que o `decode` seguinte completa; sem `stream` o resto vira U+FFFD (ou erro).
//!
//! Codificações: `utf-8`, `utf-16le`, `utf-16be` e as 30 de byte único do WHATWG (`windows-1252` com os rótulos
//! `latin1`, `ascii`, ...; `iso-8859-2`..`16`, `windows-874`, `windows-1250`..`1258`, `koi8-r`, `koi8-u`, `macintosh`,
//! `x-mac-cyrillic`, `ibm866`, `x-user-defined`), cujas tabelas e rótulos são medidos no bun por
//! `scripts/gen-text-decoder-tables.js` (`text_decoder_single_byte_data.rs`); o byte sem mapeamento é U+FFFD, ou
//! `TypeError` com `fatal`. `euc-kr` (e seus rótulos) tem a tabela de ponteiros medida por
//! `scripts/gen-text-decoder-multibyte-tables.js` (`text_decoder_euc_kr_data.rs`) e o decodificador do WHATWG: lead
//! `0x81..=0xFE` mais trail `0x41..=0xFE`, trail ASCII sem par volta ao fluxo, o lead no fim do bloco fica preso com
//! `stream`. `shift_jis` e `euc-jp` compartilham a jis0208 (e o euc-jp tem a jis0212 e a katakana de meia largura),
//! medidas por `scripts/gen-text-decoder-multibyte-tables.js jis` (`text_decoder_jis_data.rs`), com o mesmo estado de
//! fluxo (o prefixo incompleto, até 3 bytes no euc-jp, fica preso com `stream`). `iso-2022-jp` (rótulos `iso-2022-jp` e
//! `csiso2022jp`) é o decodificador de estados do WHATWG (ASCII, Roman, Katakana, Lead, Trail, Escape start, Escape, com
//! a flag de saída que torna erro o segundo escape seguido), sobre a mesma jis0208; o estado persiste entre `decode`
//! com `stream` (um ESC solto ou um lead ficam para o bloco seguinte) e volta ao inicial no `decode` sem `stream` e no
//! erro do `fatal`. O modelo foi conferido contra o bun por `scripts/check-text-decoder-iso-2022-jp.js`.
//! `gb18030` e `gbk` (o `encoding` devolve `gbk` para os rótulos do `gbk`) são o mesmo decodificador do WHATWG: ASCII,
//! `0x80` é U+20AC, pares de dois bytes pela tabela medida e quatro bytes (lead, dígito, lead, dígito) pela tabela de
//! intervalos medida (`text_decoder_gb18030_data.rs`, com os pontos fora do BMP); o prefixo incompleto (até 3 bytes) fica
//! preso com `stream`, e o terceiro ou o quarto byte fora do intervalo devolve ao fluxo tudo menos o lead. O modelo foi
//! conferido contra o bun por `scripts/check-text-decoder-gb18030.js`.
//!
//! DIVERGÊNCIAS:
//!
//! - o bun recusa `replacement` e seus rótulos (`hz-gb-2312`, `iso-2022-kr`...) como rótulo desconhecido, e o porte
//!   também (todas as outras codificações do WHATWG estão portadas; `scripts/check-text-decoder-labels.js` confere os
//!   228 rótulos da tabela, com caixa e espaço, contra o bun: 0 divergências);
//! - o `code` de `ERR_ILLEGAL_CONSTRUCTOR` e de `ERR_INVALID_THIS` é própria aqui e herdada no bun (ver
//!   `text_encoder.rs`); os erros nativos do bun ganham `originalLine`, `line`, `column` e `sourceURL`;
//! - a propriedade global entra no fim da ordem de chaves (no bun é o índice 39).

use std::cell::RefCell;
use std::collections::HashMap;

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{derived_structure, put_to_string_tag};
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_array_buffer::JSArrayBuffer;
use crate::runtime::js_data_view::JSDataView;
use crate::runtime::js_function::put_direct_native_function_without_transition;
use crate::runtime::js_generic_typed_array_view::JSGenericTypedArrayView;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_module_loader::{describe_received, rust_string};
use crate::runtime::js_object::{JSFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{EncodedJSValue, JSValue};
use crate::runtime::native_class_support::{
    create_native_class, install_global, instance_structure, put_native_accessor, throw_coded_range_error, throw_coded_type_error,
};
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM};
use crate::runtime::property_name::PropertyName;
use crate::runtime::string_regexp_support::get_object_property;
use crate::runtime::text_decoder_big5_data as big5_data;
use crate::runtime::text_decoder_euc_kr_data as euc_kr_data;
use crate::runtime::text_decoder_gb18030_data as gb18030_data;
use crate::runtime::text_decoder_jis_data as jis_data;
use crate::runtime::text_decoder_single_byte_data::{SINGLE_BYTE_ENCODINGS, UNDEFINED};
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo` do protótipo.
static TEXT_DECODER_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "TextDecoder", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `const ClassInfo` do construtor (`"Function"`).
static TEXT_DECODER_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// As codificações portadas.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Encoding {
    Utf8,
    Utf16Le,
    Utf16Be,
    /// Uma de `SINGLE_BYTE_ENCODINGS` (o índice), as de byte único do WHATWG.
    SingleByte(usize),
    /// `euc-kr` (WHATWG: a extensão Unified Hangul Code, tabela em `text_decoder_euc_kr_data.rs`).
    EucKr,
    /// `shift_jis` (WHATWG: jis0208 pelo ponteiro do shift_jis, em `text_decoder_jis_data.rs`).
    ShiftJis,
    /// `euc-jp` (WHATWG: jis0208 e jis0212 em `text_decoder_jis_data.rs`, mais a katakana de meia largura).
    EucJp,
    /// `iso-2022-jp` (WHATWG: máquina de estados com escapes ESC, jis0208 em dois bytes de 7 bits; o estado vive em
    /// `DecoderState::iso_2022_jp`).
    Iso2022Jp,
    /// `gb18030` e `gbk` (WHATWG: o mesmo decodificador, tabelas em `text_decoder_gb18030_data.rs`; `gbk` só muda o nome
    /// que o getter `encoding` devolve).
    Gb18030 { gbk: bool },
    /// `big5` (WHATWG: tabela `u32` em `text_decoder_big5_data.rs`, quatro ponteiros dão dois pontos de código).
    Big5,
}

impl Encoding {
    /// O nome canônico (o que o getter `encoding` devolve).
    pub(crate) fn name(self) -> &'static str {
        match self {
            Encoding::Utf8 => "utf-8",
            Encoding::Utf16Le => "utf-16le",
            Encoding::Utf16Be => "utf-16be",
            Encoding::SingleByte(index) => SINGLE_BYTE_ENCODINGS[index].name,
            Encoding::Gb18030 { gbk: true } => "gbk",
            Encoding::Gb18030 { gbk: false } => "gb18030",
            Encoding::EucKr => "euc-kr",
            Encoding::ShiftJis => "shift_jis",
            Encoding::EucJp => "euc-jp",
            Encoding::Iso2022Jp => "iso-2022-jp",
            Encoding::Big5 => "big5",
        }
    }

    /// A marca de ordem de bytes que abre o fluxo.
    fn bom(self) -> &'static [u8] {
        match self {
            Encoding::Utf8 => &[0xEF, 0xBB, 0xBF],
            Encoding::Utf16Le => &[0xFF, 0xFE],
            Encoding::Utf16Be => &[0xFE, 0xFF],
            Encoding::Gb18030 { .. } => &[],
            Encoding::SingleByte(_) | Encoding::EucKr | Encoding::ShiftJis | Encoding::EucJp | Encoding::Iso2022Jp | Encoding::Big5 => &[],
        }
    }
}

/// O WHATWG Encoding "get an encoding": sem espaço ASCII nas pontas, minúsculo ASCII, contra a tabela de rótulos.
pub(crate) fn parse_label(label: &str) -> Option<Encoding> {
    let trimmed = label.trim_matches(|c| matches!(c, '\t' | '\n' | '\x0c' | '\r' | ' ')).to_ascii_lowercase();
    match trimmed.as_str() {
        "unicode-1-1-utf-8" | "unicode11utf8" | "unicode20utf8" | "utf-8" | "utf8" | "x-unicode20utf8" => Some(Encoding::Utf8),
        "csunicode" | "iso-10646-ucs-2" | "ucs-2" | "unicode" | "unicodefeff" | "utf-16" | "utf-16le" => Some(Encoding::Utf16Le),
        "unicodefffe" | "utf-16be" => Some(Encoding::Utf16Be),
        other if gb18030_data::GB18030_LABELS.contains(&other) => Some(Encoding::Gb18030 { gbk: false }),
        other if gb18030_data::GBK_LABELS.contains(&other) => Some(Encoding::Gb18030 { gbk: true }),
        other if euc_kr_data::LABELS.contains(&other) => Some(Encoding::EucKr),
        other if jis_data::SHIFT_JIS_LABELS.contains(&other) => Some(Encoding::ShiftJis),
        other if jis_data::EUC_JP_LABELS.contains(&other) => Some(Encoding::EucJp),
        "csiso2022jp" | "iso-2022-jp" => Some(Encoding::Iso2022Jp),
        other if big5_data::LABELS.contains(&other) => Some(Encoding::Big5),
        other => SINGLE_BYTE_ENCODINGS.iter().position(|encoding| encoding.labels.contains(&other)).map(Encoding::SingleByte),
    }
}

/// O estado de um decodificador: as opções do construtor, os bytes presos de uma sequência incompleta e se o
/// fluxo já passou do ponto onde o BOM vale.
#[derive(Debug)]
pub(crate) struct DecoderState {
    pub(crate) encoding: Encoding,
    pub(crate) fatal: bool,
    pub(crate) ignore_bom: bool,
    pending: Vec<u8>,
    bom_seen: bool,
    /// O estado do `iso-2022-jp` (modo, modo de saída, flag de saída e lead), que persiste entre `decode` com `stream`.
    iso_2022_jp: IsoJpState,
}

impl DecoderState {
    pub(crate) fn new(encoding: Encoding, fatal: bool, ignore_bom: bool) -> DecoderState {
        DecoderState { encoding, fatal, ignore_bom, pending: Vec::new(), bom_seen: false, iso_2022_jp: IsoJpState::new() }
    }
}

/// Onde acaba a parte completa de `data` quando o fluxo continua: o fim incompleto fica preso.
fn complete_length(encoding: Encoding, data: &[u8]) -> usize {
    match encoding {
        Encoding::Utf8 => match data.utf8_chunks().last() {
            // O último trecho inválido é um prefixo de sequência válida: espera o resto.
            Some(chunk) if std::str::from_utf8(chunk.invalid()).err().is_some_and(|error| error.error_len().is_none()) => {
                data.len() - chunk.invalid().len()
            }
            _ => data.len(),
        },
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let even = data.len() & !1;
            // Unidade substituta alta no fim: espera a baixa.
            if even >= 2 && (0xD800..0xDC00).contains(&utf16_unit(encoding, &data[even - 2..even])) {
                even - 2
            } else {
                even
            }
        }
        // O estado do iso-2022-jp fica no `IsoJpState`, não em bytes presos.
        Encoding::Gb18030 { .. } => held_length(data, |index| gb18030_next(data, index).map(|(advance, _)| advance)),
        Encoding::SingleByte(_) | Encoding::Iso2022Jp => data.len(),
        Encoding::EucKr | Encoding::ShiftJis | Encoding::EucJp => held_length(data, |index| multibyte_next(encoding, data, index).map(|(advance, _)| advance)),
        Encoding::Big5 => held_length(data, |index| big5_next(data, index).map(|(advance, _)| advance)),
    }
}

/// Onde acaba a parte completa de `data` num decodificador de vários bytes: um lead no fim (fora da sequência de um lead
/// anterior) espera o resto. `step` é o avanço em `index`, ou `None` para o lead no fim.
fn held_length(data: &[u8], step: impl Fn(usize) -> Option<usize>) -> usize {
    let mut index = 0;
    while index < data.len() {
        match step(index) {
            Some(advance) => index += advance,
            None => return index,
        }
    }
    data.len()
}

/// O ponto de código do ponteiro de quatro bytes do gb18030, pela tabela de intervalos medida (`None` é sem mapeamento).
fn gb18030_four_byte_point(pointer: u32) -> Option<u32> {
    let ranges = &gb18030_data::GB18030_RANGES;
    let (start, point) = ranges[ranges.partition_point(|&(start, _)| start <= pointer) - 1];
    (point != 0).then(|| point + (pointer - start))
}

/// `gb18030` (e `gbk`) do WHATWG em `data[index]`: ASCII, `0x80` é U+20AC, lead `0x81..=0xFE` com trail `0x40..=0x7E` ou
/// `0x80..=0xFE` (dois bytes) ou com `0x30..=0x39`, lead `0x81..=0xFE` e dígito (quatro bytes). Devolve quantos bytes
/// consome e o ponto de código (`None` é erro); `None` de fora é o prefixo incompleto no fim dos dados, que espera o resto
/// (até 3 bytes). Um trail ASCII sem par volta ao fluxo (consome só o lead); no de quatro bytes, o terceiro ou o quarto
/// fora do intervalo devolve tudo menos o lead ao fluxo; o trail não ASCII sem par é consumido.
fn gb18030_next(data: &[u8], index: usize) -> Option<(usize, Option<u32>)> {
    let first = data[index];
    match first {
        0x00..=0x7F => return Some((1, Some(u32::from(first)))),
        0x80 => return Some((1, Some(0x20AC))),
        0x81..=0xFE => {}
        _ => return Some((1, None)),
    }
    let second = *data.get(index + 1)?;
    if (0x30..=0x39).contains(&second) {
        let third = *data.get(index + 2)?;
        if !(0x81..=0xFE).contains(&third) {
            return Some((1, None));
        }
        let fourth = *data.get(index + 3)?;
        if !(0x30..=0x39).contains(&fourth) {
            return Some((1, None));
        }
        let pointer = ((u32::from(first - 0x81) * 10 + u32::from(second - 0x30)) * 126 + u32::from(third - 0x81)) * 10 + u32::from(fourth - 0x30);
        return Some((4, gb18030_four_byte_point(pointer)));
    }
    let offset = match second {
        0x40..=0x7E => Some(usize::from(second - 0x40)),
        0x80..=0xFE => Some(usize::from(second - 0x41)),
        _ => None,
    };
    let point = offset
        .map(|offset| gb18030_data::GB18030_INDEX[usize::from(first - gb18030_data::LEAD_MIN) * gb18030_data::TRAIL_COUNT + offset])
        .filter(|&point| point != 0);
    match point {
        Some(point) => Some((2, Some(u32::from(point)))),
        None if second < 0x80 => Some((1, None)),
        None => Some((2, None)),
    }
}

/// `big5` do WHATWG: lead `0x81..=0xFE` mais trail `0x40..=0x7E` ou `0xA1..=0xFE`, ponteiro `(lead - 0x81) * 157 +
/// trail - offset`. Devolve quantos bytes consome e os pontos de código (o segundo é 0 quando há um só; quatro ponteiros
/// dão dois); `None` de fora é o lead no fim dos dados. O trail ASCII sem par volta ao fluxo, o não ASCII é consumido.
fn big5_next(data: &[u8], index: usize) -> Option<(usize, Option<(u32, u32)>)> {
    let byte = data[index];
    if byte < 0x80 {
        return Some((1, Some((u32::from(byte), 0))));
    }
    if !(big5_data::LEAD_MIN..=big5_data::LEAD_MAX).contains(&byte) {
        return Some((1, None));
    }
    let trail = *data.get(index + 1)?;
    let trail_index = match trail {
        0x40..=0x7E => Some(usize::from(trail - 0x40)),
        0xA1..=0xFE => Some(usize::from(trail - 0xA1) + 63),
        _ => None,
    };
    let pointer = trail_index.map(|offset| usize::from(byte - big5_data::LEAD_MIN) * big5_data::TRAIL_COUNT + offset);
    let mapped = pointer.map(|pointer| match big5_data::BIG5_PAIRS.iter().find(|pair| pair.0 == pointer) {
        Some(&(_, first, second)) => (first, second),
        None => (big5_data::BIG5_INDEX[pointer], 0),
    });
    match mapped.filter(|&(first, _)| first != 0) {
        Some(points) => Some((2, Some(points))),
        None if trail < 0x80 => Some((1, None)),
        None => Some((2, None)),
    }
}

/// O passo do decodificador de vários bytes do WHATWG em `data[index]`: quantos bytes consome e o ponto de código
/// (`None` é erro). `None` de fora é o lead no fim dos dados, que espera o resto da sequência.
fn multibyte_next(encoding: Encoding, data: &[u8], index: usize) -> Option<(usize, Option<u16>)> {
    match encoding {
        Encoding::ShiftJis => shift_jis_next(data, index),
        Encoding::EucJp => euc_jp_next(data, index),
        _ => euc_kr_next(data, index),
    }
}

/// O fim de uma sequência de vários bytes cujo último byte é `trail`, depois de `consumed` bytes já aceitos: a entrada
/// 0 (ou a falta dela) é erro. Um trail ASCII que não casa volta ao fluxo (só os `consumed` bytes são o erro); um
/// trail não ASCII que não casa é consumido junto.
fn multibyte_finish(consumed: usize, mapped: Option<u16>, trail: u8) -> Option<(usize, Option<u16>)> {
    match mapped.filter(|&unit| unit != 0) {
        Some(unit) => Some((consumed + 1, Some(unit))),
        None if trail < 0x80 => Some((consumed, None)),
        None => Some((consumed + 1, None)),
    }
}

/// `euc-kr` do WHATWG: lead `0x81..=0xFE` mais trail `0x41..=0xFE`.
fn euc_kr_next(data: &[u8], index: usize) -> Option<(usize, Option<u16>)> {
    let byte = data[index];
    if byte < 0x80 {
        return Some((1, Some(u16::from(byte))));
    }
    if !(euc_kr_data::LEAD_MIN..=euc_kr_data::LEAD_MAX).contains(&byte) {
        return Some((1, None));
    }
    let trail = *data.get(index + 1)?;
    let mapped = trail
        .checked_sub(euc_kr_data::TRAIL_MIN)
        .filter(|offset| usize::from(*offset) < euc_kr_data::TRAIL_COUNT)
        .map(|offset| euc_kr_data::EUC_KR_INDEX[usize::from(byte - euc_kr_data::LEAD_MIN) * euc_kr_data::TRAIL_COUNT + usize::from(offset)]);
    multibyte_finish(1, mapped, trail)
}

/// A katakana de meia largura: `0xA1..=0xDF` (sozinho no shift_jis, depois de `0x8E` no euc-jp) vira U+FF61...
fn half_width_katakana(byte: u8) -> Option<u16> {
    (0xA1..=0xDF).contains(&byte).then(|| 0xFF61 + u16::from(byte - 0xA1))
}

/// `shift_jis` do WHATWG: `0x00..=0x80` é o próprio ponto de código, `0xA1..=0xDF` é katakana, lead `0x81..=0x9F` e
/// `0xE0..=0xFC` mais trail `0x40..=0x7E` e `0x80..=0xFC` indexam a jis0208 pelo ponteiro `lead_index * 188 +
/// trail_index`; o resto é erro.
fn shift_jis_next(data: &[u8], index: usize) -> Option<(usize, Option<u16>)> {
    let byte = data[index];
    if byte <= 0x80 {
        return Some((1, Some(u16::from(byte))));
    }
    if let Some(unit) = half_width_katakana(byte) {
        return Some((1, Some(unit)));
    }
    if !matches!(byte, 0x81..=0x9F | 0xE0..=0xFC) {
        return Some((1, None));
    }
    let trail = *data.get(index + 1)?;
    let lead_index = usize::from(byte - if byte < 0xA0 { 0x81 } else { 0xC1 });
    let trail_index = match trail {
        0x40..=0x7E => Some(usize::from(trail - 0x40)),
        0x80..=0xFC => Some(usize::from(trail - 0x41)),
        _ => None,
    };
    let mapped = trail_index.map(|offset| jis_data::JIS0208_INDEX[lead_index * jis_data::SJIS_TRAIL_COUNT + offset]);
    multibyte_finish(1, mapped, trail)
}

/// O ponteiro de 94 * 94 do euc-jp para um par `lead`/`trail` em `0xA1..=0xFE`.
fn euc_jp_pointer(lead: u8, trail: u8) -> Option<usize> {
    let cell = |byte: u8| (0xA1..=0xFE).contains(&byte).then(|| usize::from(byte - 0xA1));
    Some(cell(lead)? * jis_data::EUC_TRAIL_COUNT + cell(trail)?)
}

/// `euc-jp` do WHATWG: ASCII, `0x8E` + katakana, `0x8F` + par da jis0212, par da jis0208 em `0xA1..=0xFE`.
fn euc_jp_next(data: &[u8], index: usize) -> Option<(usize, Option<u16>)> {
    let byte = data[index];
    if byte < 0x80 {
        return Some((1, Some(u16::from(byte))));
    }
    if !matches!(byte, 0x8E | 0x8F | 0xA1..=0xFE) {
        return Some((1, None));
    }
    let second = *data.get(index + 1)?;
    match byte {
        0x8E => multibyte_finish(1, half_width_katakana(second), second),
        0x8F if (0xA1..=0xFE).contains(&second) => {
            let trail = *data.get(index + 2)?;
            let mapped = euc_jp_pointer(second, trail).map(|pointer| jis_data::JIS0212_INDEX[pointer]);
            multibyte_finish(2, mapped, trail)
        }
        0x8F => multibyte_finish(1, None, second),
        _ => multibyte_finish(1, euc_jp_pointer(byte, second).map(|pointer| jis_data::JIS0208_INDEX[pointer]), second),
    }
}

/// Os estados do decodificador `iso-2022-jp` do WHATWG.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum IsoJpMode {
    Ascii,
    Roman,
    Katakana,
    Lead,
    Trail,
    EscapeStart,
    Escape,
}

/// As variáveis do decodificador `iso-2022-jp`: o estado, o estado de saída (o que vale depois de um escape inválido),
/// a flag de saída (ligada por um escape, desligada por qualquer saída; um segundo escape seguido é erro) e o byte
/// lead (do par jis0208 ou do primeiro byte do escape).
#[derive(Clone, Copy, Debug)]
struct IsoJpState {
    mode: IsoJpMode,
    output_mode: IsoJpMode,
    output_flag: bool,
    lead: u8,
}

impl IsoJpState {
    fn new() -> IsoJpState {
        IsoJpState { mode: IsoJpMode::Ascii, output_mode: IsoJpMode::Ascii, output_flag: false, lead: 0 }
    }
}

/// Corre o decodificador `iso-2022-jp` sobre `input`, chamando `emit` com cada ponto de código (`None` é erro).
/// `finish` é o fim do fluxo (o `decode` sem `stream`): trata o fim da fila e deixa o estado inicial; sem ele o
/// estado (inclusive um ESC solto) fica para o próximo bloco.
fn iso_2022_jp_run(state: &mut IsoJpState, input: &[u8], finish: bool, mut emit: impl FnMut(Option<u16>)) {
    // Os bytes devolvidos à fila (no máximo dois) vêm antes do resto da entrada.
    let mut front: Vec<u8> = Vec::new();
    let mut rest = input.iter().copied();
    loop {
        let byte = front.pop().or_else(|| rest.next());
        if byte.is_none() && !finish {
            return;
        }
        let printable = |b: u8, low: u8, high: u8| (low..=high).contains(&b);
        match state.mode {
            IsoJpMode::Ascii | IsoJpMode::Roman | IsoJpMode::Katakana | IsoJpMode::Lead => {
                let Some(byte) = byte else { break };
                if byte == 0x1B {
                    state.mode = IsoJpMode::EscapeStart;
                    continue;
                }
                state.output_flag = false;
                let plain = byte <= 0x7F && byte != 0x0E && byte != 0x0F;
                match state.mode {
                    IsoJpMode::Ascii if plain => emit(Some(u16::from(byte))),
                    IsoJpMode::Roman if plain => emit(Some(match byte {
                        0x5C => 0x00A5,
                        0x7E => 0x203E,
                        other => u16::from(other),
                    })),
                    IsoJpMode::Katakana if printable(byte, 0x21, 0x5F) => emit(Some(0xFF61 - 0x21 + u16::from(byte))),
                    IsoJpMode::Lead if printable(byte, 0x21, 0x7E) => {
                        state.lead = byte;
                        state.mode = IsoJpMode::Trail;
                    }
                    _ => emit(None),
                }
            }
            IsoJpMode::Trail => match byte {
                Some(0x1B) => {
                    state.mode = IsoJpMode::EscapeStart;
                    emit(None);
                }
                Some(byte) if printable(byte, 0x21, 0x7E) => {
                    state.mode = IsoJpMode::Lead;
                    let pointer = usize::from(state.lead - 0x21) * jis_data::EUC_TRAIL_COUNT + usize::from(byte - 0x21);
                    let unit = jis_data::JIS0208_INDEX[pointer];
                    emit((unit != 0).then_some(unit));
                }
                // Outro byte, ou o fim da fila (que o estado lead trata em seguida): erro e volta a esperar um lead.
                _ => {
                    state.mode = IsoJpMode::Lead;
                    emit(None);
                }
            },
            IsoJpMode::EscapeStart => match byte {
                Some(byte @ (0x24 | 0x28)) => {
                    state.lead = byte;
                    state.mode = IsoJpMode::Escape;
                }
                _ => {
                    front.extend(byte);
                    state.output_flag = false;
                    state.mode = state.output_mode;
                    emit(None);
                }
            },
            IsoJpMode::Escape => {
                let lead = std::mem::take(&mut state.lead);
                let next = match (lead, byte) {
                    (0x28, Some(0x42)) => Some(IsoJpMode::Ascii),
                    (0x28, Some(0x4A)) => Some(IsoJpMode::Roman),
                    (0x28, Some(0x49)) => Some(IsoJpMode::Katakana),
                    (0x24, Some(0x40 | 0x42)) => Some(IsoJpMode::Lead),
                    _ => None,
                };
                match next {
                    Some(mode) => {
                        state.mode = mode;
                        state.output_mode = mode;
                        // Dois escapes sem saída entre eles: o segundo é erro.
                        if std::mem::replace(&mut state.output_flag, true) {
                            emit(None);
                        }
                    }
                    None => {
                        // O lead e o byte voltam à fila, nessa ordem (o último a entrar em `front` sai primeiro).
                        front.extend(byte);
                        front.push(lead);
                        state.output_flag = false;
                        state.mode = state.output_mode;
                        emit(None);
                    }
                }
            }
        }
    }
    *state = IsoJpState::new();
}

/// Um `decode` do `iso-2022-jp` com o estado do decodificador; `Err(())` é dado inválido com `fatal`, que também
/// devolve o estado ao inicial.
fn decode_iso_2022_jp(state: &mut DecoderState, input: &[u8], stream: bool) -> Result<String, ()> {
    let mut text = String::with_capacity(input.len());
    let mut invalid = false;
    iso_2022_jp_run(&mut state.iso_2022_jp, input, !stream, |unit| match unit {
        Some(unit) => text.extend(char::from_u32(u32::from(unit))),
        None => {
            invalid = true;
            text.push(char::REPLACEMENT_CHARACTER);
        }
    });
    if invalid && state.fatal {
        state.iso_2022_jp = IsoJpState::new();
        return Err(());
    }
    Ok(text)
}

fn utf16_unit(encoding: Encoding, pair: &[u8]) -> u16 {
    if encoding == Encoding::Utf16Be {
        u16::from_be_bytes([pair[0], pair[1]])
    } else {
        u16::from_le_bytes([pair[0], pair[1]])
    }
}

/// Decodifica `body` inteiro; `Err(())` é dado inválido com `fatal`.
fn decode_body(encoding: Encoding, fatal: bool, body: &[u8]) -> Result<String, ()> {
    match encoding {
        Encoding::Utf8 if fatal => std::str::from_utf8(body).map(str::to_owned).map_err(|_| ()),
        Encoding::Utf8 => Ok(String::from_utf8_lossy(body).into_owned()),
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let units = body.chunks_exact(2).map(|pair| utf16_unit(encoding, pair));
            let mut text = String::new();
            for decoded in char::decode_utf16(units) {
                match decoded {
                    Ok(character) => text.push(character),
                    Err(_) if fatal => return Err(()),
                    Err(_) => text.push(char::REPLACEMENT_CHARACTER),
                }
            }
            if body.len() % 2 == 1 {
                if fatal {
                    return Err(());
                }
                text.push(char::REPLACEMENT_CHARACTER);
            }
            Ok(text)
        }
        Encoding::Gb18030 { .. } => {
            let mut text = String::with_capacity(body.len());
            let mut index = 0;
            while index < body.len() {
                // O prefixo preso no fim, sem `stream`, é erro (um só, para os bytes que sobram).
                let (advance, point) = gb18030_next(body, index).unwrap_or((body.len() - index, None));
                index += advance;
                match point.and_then(char::from_u32) {
                    Some(character) => text.push(character),
                    None if fatal => return Err(()),
                    None => text.push(char::REPLACEMENT_CHARACTER),
                }
            }
            Ok(text)
        }
        Encoding::SingleByte(index) => {
            let high = &SINGLE_BYTE_ENCODINGS[index].high;
            let mut text = String::with_capacity(body.len());
            for &byte in body {
                let unit = if byte < 0x80 { u16::from(byte) } else { high[usize::from(byte - 0x80)] };
                if unit == UNDEFINED {
                    // O byte sem mapeamento: erro com `fatal`, U+FFFD sem.
                    if fatal {
                        return Err(());
                    }
                    text.push(char::REPLACEMENT_CHARACTER);
                } else {
                    text.extend(char::from_u32(u32::from(unit)));
                }
            }
            Ok(text)
        }
        Encoding::EucKr | Encoding::ShiftJis | Encoding::EucJp => {
            let mut text = String::with_capacity(body.len());
            let mut index = 0;
            while index < body.len() {
                // O lead preso no fim, sem `stream`, é erro (um só, para os bytes que sobram).
                let (advance, unit) = multibyte_next(encoding, body, index).unwrap_or((body.len() - index, None));
                index += advance;
                match unit {
                    Some(unit) => text.extend(char::from_u32(u32::from(unit))),
                    None if fatal => return Err(()),
                    None => text.push(char::REPLACEMENT_CHARACTER),
                }
            }
            Ok(text)
        }
        // `decode_chunk` desvia o iso-2022-jp para o estado dele; aqui é o fluxo inteiro de uma vez.
        Encoding::Iso2022Jp => decode_iso_2022_jp(&mut DecoderState::new(encoding, fatal, false), body, false),
        Encoding::Big5 => {
            let mut text = String::with_capacity(body.len());
            let mut index = 0;
            while index < body.len() {
                let (advance, points) = big5_next(body, index).unwrap_or((body.len() - index, None));
                index += advance;
                match points {
                    Some((first, second)) => text.extend(std::iter::once(first).chain((second != 0).then_some(second)).filter_map(char::from_u32)),
                    None if fatal => return Err(()),
                    None => text.push(char::REPLACEMENT_CHARACTER),
                }
            }
            Ok(text)
        }
    }
}

/// `decode(input, { stream })` no estado `state`.
pub(crate) fn decode_chunk(state: &mut DecoderState, input: &[u8], stream: bool) -> Result<String, ()> {
    if state.encoding == Encoding::Iso2022Jp {
        // Sem bytes presos nem BOM: o estado do decodificador persiste no próprio `IsoJpState`.
        return decode_iso_2022_jp(state, input, stream);
    }
    // Um erro deixa os bytes presos descartados e o estado do BOM como estava.
    let mut data = std::mem::take(&mut state.pending);
    data.extend_from_slice(input);
    let complete = if stream { complete_length(state.encoding, &data) } else { data.len() };
    let tail = data.split_off(complete);
    let mut body: &[u8] = &data;
    let mut bom_seen = state.bom_seen;
    if !body.is_empty() {
        let bom = state.encoding.bom();
        if !bom_seen && !state.ignore_bom && body.starts_with(bom) {
            body = &body[bom.len()..];
        }
        bom_seen = true;
    }
    let text = decode_body(state.encoding, state.fatal, body)?;
    state.pending = tail;
    state.bom_seen = stream && bom_seen;
    Ok(text)
}

thread_local! {
    /// Os decodificadores do programa (valor codificado da célula para o estado), para a conferência do `this`.
    static DECODERS: RefCell<HashMap<EncodedJSValue, DecoderState>> = RefCell::new(HashMap::new());
}

/// Fim do programa (`cell_registry::reset_program_state`): os estados guardados são do programa.
/// As raízes do coletor: as instâncias (o estado é só de bytes).
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    DECODERS.with(|decoders| decoders.borrow().keys().for_each(|decoder| marker.encoded(*decoder)));
}

pub(crate) fn reset_for_program() {
    let _ = DECODERS.try_with(|decoders| decoders.borrow_mut().clear());
}

fn with_decoder<R>(this: JSValue, use_state: impl FnOnce(&mut DecoderState) -> R) -> Option<R> {
    DECODERS.with(|decoders| decoders.borrow_mut().get_mut(&this.encode()).map(use_state))
}

pub(crate) fn text_value(global_object: &JSGlobalObject, text: &str) -> JSValue {
    JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf8(text.as_bytes())))
}

/// `TextDecoder(...)` sem `new`.
fn call_text_decoder_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(throw_coded_type_error(global_object, "TextDecoder constructor cannot be invoked without 'new'", "ERR_ILLEGAL_CONSTRUCTOR"))
}

/// `options` que não é objeto (nem `undefined`/`null`).
pub(crate) fn check_options(global_object: &JSGlobalObject, options: JSValue) -> Result<(), Thrown> {
    if options.is_undefined_or_null() || options.is_object() {
        return Ok(());
    }
    let received = describe_received(global_object, options).map(|text| format!(" Received {text}")).unwrap_or_default();
    Err(throw_coded_type_error(global_object, &format!("The \"options\" argument must be of type object.{received}"), "ERR_INVALID_ARG_TYPE"))
}

/// `Boolean(options[name])`; `options` ausente é `false`.
pub(crate) fn option_flag(global_object: &JSGlobalObject, options: JSValue, name: &str) -> Result<bool, Thrown> {
    if !options.is_object() {
        return Ok(false);
    }
    let value = get_object_property(global_object, options, &Identifier::from_span(global_object.vm(), name.as_bytes()))?;
    Ok(value.to_boolean())
}

/// Os argumentos `(label, options)` do construtor de `TextDecoder` e de `TextDecoderStream`, já validados.
pub(crate) fn decoder_from_arguments(global_object: &JSGlobalObject, call: &HostCall) -> Result<DecoderState, Thrown> {
    let label_argument = call.argument(0);
    let label = if label_argument.is_undefined() {
        "utf-8".to_owned()
    } else {
        rust_string(&pending_or(global_object, label_argument.to_wtf_string())?)
    };
    let Some(encoding) = parse_label(&label) else {
        return Err(throw_coded_range_error(global_object, &format!("Unsupported encoding label \"{label}\""), "ERR_ENCODING_NOT_SUPPORTED"));
    };
    let options = call.argument(1);
    check_options(global_object, options)?;
    let fatal = option_flag(global_object, options, "fatal")?;
    let ignore_bom = option_flag(global_object, options, "ignoreBOM")?;
    Ok(DecoderState::new(encoding, fatal, ignore_bom))
}

/// `new TextDecoder(label, options)`.
fn construct_text_decoder_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let state = decoder_from_arguments(global_object, call)?;
    let structure = derived_structure(global_object, call, instance_structure)?;
    let instance = JSFinalObject::create(global_object.vm(), &structure).as_value();
    DECODERS.with(|decoders| decoders.borrow_mut().insert(instance.encode(), state));
    Ok(instance)
}

host_function!(call_text_decoder, call_text_decoder_body);
host_function!(construct_text_decoder, construct_text_decoder_body);

/// Os bytes de `input`: typed array, `DataView` ou `ArrayBuffer`; `None` para o resto.
pub(crate) fn input_bytes(input: JSValue) -> Option<Vec<u8>> {
    if let Some(view) = JSGenericTypedArrayView::from_value(&input) {
        let length = view.byte_length();
        return Some(view.with_vector(|vector| vector[..length.min(vector.len())].to_vec()));
    }
    if let Some(view) = JSDataView::from_value(&input) {
        let mut bytes = vec![0; view.view_byte_length().unwrap_or(0)];
        view.read_bytes(0, &mut bytes);
        return Some(bytes);
    }
    JSArrayBuffer::from_value(&input).map(|buffer| buffer.impl_().with_bytes(<[u8]>::to_vec))
}

fn text_decoder_decode_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = call.this_value();
    if with_decoder(this, |_| ()).is_none() {
        let received = describe_received(global_object, this).map(|text| format!(", but received {text}")).unwrap_or_default();
        return Err(throw_coded_type_error(
            global_object,
            &format!("Expected this to be instanceof TextDecoder{received}"),
            "ERR_INVALID_THIS",
        ));
    }
    let options = call.argument(1);
    check_options(global_object, options)?;
    let stream = option_flag(global_object, options, "stream")?;
    let input = call.argument(0);
    let bytes = if input.is_undefined() {
        Vec::new()
    } else {
        match input_bytes(input) {
            Some(bytes) => bytes,
            None => {
                return Err(throw_coded_type_error(
                    global_object,
                    "TextDecoder.decode expects an ArrayBuffer or TypedArray",
                    "ERR_INVALID_ARG_TYPE",
                ))
            }
        }
    };
    let decoded = with_decoder(this, |state| decode_chunk(state, &bytes, stream).map_err(|()| state.encoding.name()));
    match decoded {
        Some(Ok(text)) => Ok(text_value(global_object, &text)),
        Some(Err(name)) => Err(invalid_encoded_data(global_object, name)),
        None => Ok(text_value(global_object, "")),
    }
}

/// `ERR_ENCODING_INVALID_ENCODED_DATA`: o erro de `fatal` do `TextDecoder` e do `TextDecoderStream`.
pub(crate) fn invalid_encoded_data(global_object: &JSGlobalObject, encoding_name: &str) -> Thrown {
    throw_coded_type_error(global_object, &format!("The encoded data was not valid for encoding {encoding_name}"), "ERR_ENCODING_INVALID_ENCODED_DATA")
}

/// O corpo de um getter: confere o `this` e devolve o valor.
fn getter_body(call: &HostCall, name: &str, read: impl FnOnce(&DecoderState) -> JSValue) -> HostResult {
    with_decoder(call.this_value(), |state| read(state))
        .ok_or_else(|| Thrown::type_error(&format!("The TextDecoder.{name} getter can only be used on instances of TextDecoder")))
}

fn text_decoder_encoding_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    getter_body(call, "encoding", |state| text_value(global_object, state.encoding.name()))
}

fn text_decoder_fatal_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    getter_body(call, "fatal", |state| JSValue::Bool(state.fatal))
}

fn text_decoder_ignore_bom_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    getter_body(call, "ignoreBOM", |state| JSValue::Bool(state.ignore_bom))
}

host_function!(text_decoder_decode, text_decoder_decode_body);
host_function!(text_decoder_encoding, text_decoder_encoding_body);
host_function!(text_decoder_fatal, text_decoder_fatal_body);
host_function!(text_decoder_ignore_bom, text_decoder_ignore_bom_body);

/// Instala `TextDecoder` no global: protótipo herdando de `Object.prototype`, construtor, `decode`, acessores.
pub fn install_text_decoder(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let (prototype, constructor) =
        create_native_class(global_object, &TEXT_DECODER_PROTOTYPE_S_INFO, &TEXT_DECODER_CONSTRUCTOR_S_INFO, "TextDecoder", call_text_decoder, construct_text_decoder);

    put_direct_native_function_without_transition(
        vm,
        global_object,
        &prototype,
        &Identifier::from_span(vm, b"decode"),
        1,
        text_decoder_decode,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        DONT_DELETE,
    );
    put_native_accessor(vm, global_object, &prototype, "encoding", text_decoder_encoding, None, DONT_DELETE);
    put_native_accessor(vm, global_object, &prototype, "fatal", text_decoder_fatal, None, DONT_DELETE);
    put_native_accessor(vm, global_object, &prototype, "ignoreBOM", text_decoder_ignore_bom, None, DONT_DELETE);
    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
    put_to_string_tag(vm, &prototype, "TextDecoder");

    install_global(global_object, "TextDecoder", constructor.as_value());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_all(encoding: Encoding, fatal: bool, chunks: &[(&[u8], bool)]) -> Vec<Result<String, ()>> {
        let mut state = DecoderState::new(encoding, fatal, false);
        chunks.iter().map(|(bytes, stream)| decode_chunk(&mut state, bytes, *stream)).collect()
    }

    #[test]
    fn utf8_stream_splits_a_sequence() {
        let out = decode_all(Encoding::Utf8, false, &[(&[0xE2], true), (&[0x82], true), (&[0xAC], true), (&[], false)]);
        assert_eq!(out, vec![Ok(String::new()), Ok(String::new()), Ok("\u{20AC}".to_owned()), Ok(String::new())]);
    }

    #[test]
    fn bom_only_at_the_start_of_the_stream() {
        let out = decode_all(Encoding::Utf8, false, &[(&[0x61], true), (&[0xEF, 0xBB, 0xBF, 0x62], false), (&[0xEF, 0xBB, 0xBF, 0x63], false)]);
        assert_eq!(out, vec![Ok("a".to_owned()), Ok("\u{FEFF}b".to_owned()), Ok("c".to_owned())]);
    }

    #[test]
    fn utf16_holds_odd_byte_and_high_surrogate() {
        let out = decode_all(Encoding::Utf16Le, false, &[(&[0x3D], true), (&[0xD8], true), (&[0x00, 0xDE], false)]);
        assert_eq!(out, vec![Ok(String::new()), Ok(String::new()), Ok("\u{1F600}".to_owned())]);
    }

    #[test]
    fn fatal_truncated_utf8_is_an_error() {
        assert_eq!(decode_all(Encoding::Utf8, true, &[(&[0xE2, 0x82], false)]), vec![Err(())]);
    }

    #[test]
    fn euc_kr_pairs_and_error_recovery() {
        let out = decode_all(Encoding::EucKr, false, &[(&[0xC7, 0xD1, 0x41, 0xC7, 0x41, 0xC7, 0x80, 0x42, 0xC9, 0xA1], false)]);
        // 한 A, lead + trail ASCII (U+FFFD e o ASCII volta), lead + 0x80 (os dois consumidos), par sem mapeamento.
        assert_eq!(out, vec![Ok("\u{D55C}A\u{FFFD}A\u{FFFD}B\u{FFFD}".to_owned())]);
    }

    #[test]
    fn euc_kr_holds_a_trailing_lead_with_stream() {
        let out = decode_all(Encoding::EucKr, false, &[(&[0xC7], true), (&[0xD1, 0xC7], true), (&[], false)]);
        assert_eq!(out, vec![Ok(String::new()), Ok("\u{D55C}".to_owned()), Ok("\u{FFFD}".to_owned())]);
        assert_eq!(decode_all(Encoding::EucKr, true, &[(&[0xC7, 0x41], false)]), vec![Err(())]);
    }

    #[test]
    fn shift_jis_and_euc_jp_recovery() {
        // 0x81 0x5C (U+2015), 0x81 0x7F (lead + trail ASCII sem par volta ao fluxo), katakana, 0x80, 0xA0.
        let out = decode_all(Encoding::ShiftJis, false, &[(&[0x81, 0x5C, 0x81, 0x7F, 0xA1, 0x80, 0xA0], false)]);
        assert_eq!(out, vec![Ok("\u{2015}\u{FFFD}\u{7F}\u{FF61}\u{80}\u{FFFD}".to_owned())]);
        // 0x8F 0xA2 0x41 (jis0212 sem par: um erro e o ASCII volta), 0x8E 0xA1, 0x8F 0xA2 no fim sem stream (um erro só).
        let out = decode_all(Encoding::EucJp, false, &[(&[0x8F, 0xA2, 0x41, 0x8E, 0xA1, 0x8F, 0xA2], false)]);
        assert_eq!(out, vec![Ok("\u{FFFD}A\u{FF61}\u{FFFD}".to_owned())]);
    }

    #[test]
    fn euc_jp_holds_a_partial_sequence_with_stream() {
        let out = decode_all(Encoding::EucJp, false, &[(&[0x8F], true), (&[0xA2], true), (&[0xAF], false)]);
        assert_eq!(out, vec![Ok(String::new()), Ok(String::new()), Ok("\u{2D8}".to_owned())]);
        assert_eq!(decode_all(Encoding::EucJp, true, &[(&[0x8F, 0xA2], false)]), vec![Err(())]);
    }

    #[test]
    fn big5_pairs_digraphs_and_recovery() {
        // 0xA4 0x40 (U+4E00), 0x88 0x62 (dois pontos de código), 0x81 0x41 (sem par: lead + trail ASCII volta ao fluxo),
        // 0x81 0xA1 (sem par, trail não ASCII consumido), 0xFF.
        let out = decode_all(Encoding::Big5, false, &[(&[0xA4, 0x40, 0x88, 0x62, 0x81, 0x41, 0x81, 0xA1, 0xFF], false)]);
        assert_eq!(out, vec![Ok("\u{4E00}\u{CA}\u{304}\u{FFFD}A\u{FFFD}\u{FFFD}".to_owned())]);
        let out = decode_all(Encoding::Big5, false, &[(&[0xA4], true), (&[0x40, 0xA4], true), (&[], false)]);
        assert_eq!(out, vec![Ok(String::new()), Ok("\u{4E00}".to_owned()), Ok("\u{FFFD}".to_owned())]);
        assert_eq!(parse_label("big5-hkscs").map(Encoding::name), Some("big5"));
    }

    #[test]
    fn iso_2022_jp_escapes_errors_and_stream_state() {
        let (esc, kanji, ascii) = ([0x1B, 0x24, 0x42], [0x1B, 0x28, 0x42], [0x1B, 0x28, 0x4A]);
        let join = |parts: &[&[u8]]| parts.concat();
        // Troca de estado e um par jis0208 (亜), depois o ASCII; Roman mapeia 0x5C e 0x7E; ESC $ C inválido volta em ASCII.
        let out = decode_all(Encoding::Iso2022Jp, false, &[(&join(&[&esc, &[0x30, 0x21], &kanji, &[0x41]]), false)]);
        assert_eq!(out, vec![Ok("\u{4E9C}A".to_owned())]);
        let out = decode_all(Encoding::Iso2022Jp, false, &[(&join(&[&ascii, &[0x5C, 0x7E, 0x41]]), false)]);
        assert_eq!(out, vec![Ok("\u{A5}\u{203E}A".to_owned())]);
        let out = decode_all(Encoding::Iso2022Jp, false, &[(&[0x1B, 0x24, 0x43, 0x41, 0x1B], false)]);
        assert_eq!(out, vec![Ok("\u{FFFD}$CA\u{FFFD}".to_owned())]);
        // Dois escapes seguidos: o segundo é erro; um lead solto no fim é erro; ESC no fim com `fatal` é erro.
        let out = decode_all(Encoding::Iso2022Jp, false, &[(&join(&[&kanji, &kanji, &[0x41]]), false), (&join(&[&esc, &[0x30]]), false)]);
        assert_eq!(out, vec![Ok("\u{FFFD}A".to_owned()), Ok("\u{FFFD}".to_owned())]);
        assert_eq!(decode_all(Encoding::Iso2022Jp, true, &[(&[0x1B], false)]), vec![Err(())]);
        // `stream` guarda o modo e o ESC solto; o `decode` sem `stream` volta ao ASCII.
        let out = decode_all(Encoding::Iso2022Jp, false, &[(&esc, true), (&[0x30], true), (&[0x21, 0x1B], true), (&[], false), (&[0x30, 0x21], false)]);
        assert_eq!(out, vec![Ok(String::new()), Ok(String::new()), Ok("\u{4E9C}".to_owned()), Ok("\u{FFFD}".to_owned()), Ok("0!".to_owned())]);
    }

    #[test]
    fn gb18030_two_four_byte_and_recovery() {
        // 0x80 (U+20AC), 0xA2E3 (U+20AC), 0x81 0x30 0x81 0x30 (U+0080), 0x90 0x30 0x81 0x30 (U+10000), 0xA1A4 (U+00B7),
        // 0x81 0x7F (lead + trail ASCII volta ao fluxo), 0x81 0xFF (consumidos), 0xFF, 0x81 0x30 0x41 (o 0 e o A voltam).
        let bytes = [0x80, 0xA2, 0xE3, 0x81, 0x30, 0x81, 0x30, 0x90, 0x30, 0x81, 0x30, 0xA1, 0xA4, 0x81, 0x7F, 0x81, 0xFF, 0xFF, 0x81, 0x30, 0x41];
        let out = decode_all(Encoding::Gb18030 { gbk: false }, false, &[(&bytes, false)]);
        assert_eq!(out, vec![Ok("\u{20AC}\u{20AC}\u{80}\u{10000}\u{B7}\u{FFFD}\u{7F}\u{FFFD}\u{FFFD}\u{FFFD}0A".to_owned())]);
        // Quatro bytes sem mapeamento (0x84 0x31 0xA5 0x30) consomem os quatro.
        let out = decode_all(Encoding::Gb18030 { gbk: true }, false, &[(&[0x84, 0x31, 0xA5, 0x30, 0x41], false)]);
        assert_eq!(out, vec![Ok("\u{FFFD}A".to_owned())]);
    }

    #[test]
    fn gb18030_holds_up_to_three_bytes_with_stream() {
        let encoding = Encoding::Gb18030 { gbk: false };
        let out = decode_all(encoding, false, &[(&[0x81], true), (&[0x30], true), (&[0x81], true), (&[0x30, 0x41], false)]);
        assert_eq!(out, vec![Ok(String::new()), Ok(String::new()), Ok(String::new()), Ok("\u{80}A".to_owned())]);
        // Um prefixo que só vira erro com o byte seguinte: 0x81 0x30 0x81 e depois 0x41 (o terceiro e o quarto voltam).
        let out = decode_all(encoding, false, &[(&[0x81, 0x30, 0x81], true), (&[0x41], false)]);
        assert_eq!(out, vec![Ok(String::new()), Ok("\u{FFFD}0\u{4E04}".to_owned())]);
        assert_eq!(decode_all(encoding, true, &[(&[0x81, 0x30, 0x81], false)]), vec![Err(())]);
        assert_eq!(parse_label("GB2312").map(Encoding::name), Some("gbk"));
        assert_eq!(parse_label("gb18030").map(Encoding::name), Some("gb18030"));
    }

    #[test]
    fn labels_normalize() {
        assert_eq!(parse_label("CSISO2022JP").map(Encoding::name), Some("iso-2022-jp"));
        assert_eq!(parse_label("iso-2022-jp-2"), None);
        assert_eq!(parse_label("sjis").map(Encoding::name), Some("shift_jis"));
        assert_eq!(parse_label("x-euc-jp").map(Encoding::name), Some("euc-jp"));
        assert_eq!(parse_label("Korean").map(Encoding::name), Some("euc-kr"));
        assert_eq!(parse_label("x-euc-kr"), None);
        assert_eq!(parse_label(" UTF8\n"), Some(Encoding::Utf8));
        assert_eq!(parse_label("latin1").map(Encoding::name), Some("windows-1252"));
        assert_eq!(parse_label("Cyrillic ").map(Encoding::name), Some("iso-8859-5"));
        assert_eq!(parse_label("\u{b}utf-8"), None);
    }
}
