//! Porte de `src/vs/editor/common/languages/defaultDocumentColorsComputer.ts`: as cores padrão de um
//! documento (`#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()`, `rgba()`, `hsl()` e `hsla()`, inclusive a
//! sintaxe CSS Level 4 com espaços e `/`).
//!
//! Decisões de tradução:
//! - `RegExpMatchArray` vira [`RegExpMatchArray`]: o `index` e cada entrada do array (o casamento inteiro e
//!   os grupos, `None` para o grupo que não participou), em UTF-16. O `[...texto.matchAll(regex)]` do
//!   original é [`match_all`].
//! - `IColor` e `IColorInformation` (de `languages.ts`) ficam aqui: o `languages.rs` do crate só traz a
//!   parte de tokenização. `IRange` é o [`Range`].
//! - O `Number(...)` do JavaScript sobre os grupos é [`js_number`].
//! - A guarda de `computeDefaultDocumentColors` contra um chamador desconhecido (`!model`, `getValue` ou
//!   `positionAt` que não são funções) não existe: o tipo [`IDocumentColorComputerTarget`] garante os
//!   métodos.

use std::ops::Range as StdRange;

use je_theme::color::{Color, Hsla, format};
use zyarr::Match;

use je_base::js_regexp::JsRegExp;
use crate::position::Position;
use crate::range::Range;

/// `IColor`: componentes em [0, 1].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IColor {
    pub red: f64,
    pub blue: f64,
    pub green: f64,
    pub alpha: f64,
}

/// `IColorInformation`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IColorInformation {
    pub range: Range,
    pub color: IColor,
}

/// `RegExpMatchArray`: o resultado de um casamento, com os textos em UTF-16.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegExpMatchArray {
    /// `index`: onde o casamento começa.
    pub index: Option<usize>,
    /// O casamento inteiro (posição 0) e os grupos de captura; `None` é o `undefined` do grupo que não
    /// participou.
    pub values: Vec<Option<Vec<u16>>>,
}

impl RegExpMatchArray {
    /// O casamento inteiro, `match[0]`.
    pub fn matched(&self) -> &[u16] {
        self.values.first().and_then(Option::as_deref).unwrap_or(&[])
    }

    fn from_match(text: &[u16], found: &Match) -> Self {
        let slice = |range: &StdRange<usize>| text[range.clone()].to_vec();
        Self {
            index: Some(found.start()),
            values: found.captures.iter().map(|capture| capture.as_ref().map(slice)).collect(),
        }
    }
}

/// `[...text.matchAll(regex)]`: todos os casamentos de `regex` (que precisa da flag `g`) em `text`. Parte do
/// `lastIndex` do `regex` sem alterá-lo (o `matchAll` trabalha numa cópia) e, depois de um casamento vazio,
/// avança um índice (um ponto de código com `u` ou `v`).
pub fn match_all(text: &[u16], regex: &JsRegExp) -> Vec<RegExpMatchArray> {
    let flags = regex.flags();
    assert!(flags.global, "String.prototype.matchAll called with a non-global RegExp argument");
    let matcher = JsRegExp::new(regex.source(), &flags.to_flags_string())
        .expect("a cópia do padrão já compilado também compila");
    matcher.set_last_index(regex.last_index());

    let mut result = Vec::new();
    while let Some(found) = matcher.exec(text) {
        result.push(RegExpMatchArray::from_match(text, &found));
        if found.start() == found.end() {
            let end = found.end();
            let paired = flags.either_unicode()
                && text.get(end).is_some_and(|unit| (0xD800..=0xDBFF).contains(unit))
                && text.get(end + 1).is_some_and(|unit| (0xDC00..=0xDFFF).contains(unit));
            matcher.set_last_index(end + if paired { 2 } else { 1 });
        }
    }
    result
}

/// `IDocumentColorComputerTarget`.
pub trait IDocumentColorComputerTarget {
    fn get_value(&self) -> Vec<u16>;
    fn position_at(&self, offset: usize) -> Position;
    fn find_matches(&self, regex: &JsRegExp) -> Vec<RegExpMatchArray>;
}

/// Espaço em branco do JavaScript (`\s` e o que o `Number` e o `trim` descartam).
fn is_js_whitespace(c: char) -> bool {
    c.is_whitespace() || c == '\u{FEFF}'
}

/// `Number(text)` do JavaScript: a cadeia vazia (ou só de espaço) vale 0, os literais hexadecimal, octal e
/// binário sem sinal e o decimal (com sinal, ponto e expoente) são lidos, `Infinity` com sinal vale
/// infinito, o resto é `NaN`.
fn js_number(text: &[u16]) -> f64 {
    let text = String::from_utf16_lossy(text);
    let trimmed = text.trim_matches(is_js_whitespace);
    if trimmed.is_empty() {
        return 0.0;
    }

    let radix_prefixed = |prefix: &str, upper: &str, radix: u32| {
        let digits = trimmed.strip_prefix(prefix).or_else(|| trimmed.strip_prefix(upper))?;
        if digits.is_empty() {
            return Some(f64::NAN);
        }
        let mut value = 0.0;
        for c in digits.chars() {
            value = value * f64::from(radix) + f64::from(c.to_digit(radix)?);
        }
        Some(value)
    };
    for (prefix, upper, radix) in [("0x", "0X", 16), ("0o", "0O", 8), ("0b", "0B", 2)] {
        if trimmed.starts_with(prefix) || trimmed.starts_with(upper) {
            return radix_prefixed(prefix, upper, radix).unwrap_or(f64::NAN);
        }
    }

    let (negative, unsigned) = match trimmed.as_bytes()[0] {
        b'+' => (false, &trimmed[1..]),
        b'-' => (true, &trimmed[1..]),
        _ => (false, trimmed),
    };
    if unsigned == "Infinity" {
        return if negative { f64::NEG_INFINITY } else { f64::INFINITY };
    }

    // StrUnsignedDecimalLiteral: dígitos, ponto opcional e fração, ou só a fração; expoente opcional.
    let bytes = unsigned.as_bytes();
    let count_digits = |from: usize| bytes[from..].iter().take_while(|b| b.is_ascii_digit()).count();
    let mut pos = count_digits(0);
    let mut digits = pos;
    if bytes.get(pos) == Some(&b'.') {
        let fraction = count_digits(pos + 1);
        digits += fraction;
        pos += 1 + fraction;
    }
    if digits == 0 {
        return f64::NAN;
    }
    if matches!(bytes.get(pos), Some(b'e' | b'E')) {
        let mut exponent = pos + 1;
        if matches!(bytes.get(exponent), Some(b'+' | b'-')) {
            exponent += 1;
        }
        let exponent_digits = count_digits(exponent);
        if exponent_digits == 0 {
            return f64::NAN;
        }
        pos = exponent + exponent_digits;
    }
    if pos != bytes.len() {
        return f64::NAN;
    }
    let value: f64 = unsigned.parse().unwrap_or(f64::NAN);
    if negative { -value } else { value }
}

/// `_parseCaptureGroups`: os números de cada entrada do casamento (o inteiro e os grupos) que valem como
/// número: diferentes de `NaN` e de zero, ou zero escrito com algum caractere que não é espaço.
fn parse_capture_groups(match_: &RegExpMatchArray) -> Vec<f64> {
    let mut values = Vec::new();
    for capture_group in &match_.values {
        // O grupo que não participou é `undefined`, e `Number(undefined)` é `NaN`.
        let Some(capture_group) = capture_group else { continue };
        let parsed_number = js_number(capture_group);
        let not_blank = || !String::from_utf16_lossy(capture_group).chars().all(is_js_whitespace);
        if (parsed_number != 0.0 && !parsed_number.is_nan()) || (parsed_number == 0.0 && not_blank()) {
            values.push(parsed_number);
        }
    }
    values
}

fn to_icolor(r: f64, g: f64, b: f64, a: f64) -> IColor {
    IColor { red: r / 255.0, blue: b / 255.0, green: g / 255.0, alpha: a }
}

fn find_range(model: &dyn IDocumentColorComputerTarget, match_: &RegExpMatchArray) -> Option<Range> {
    let index = match_.index?;
    let length = match_.matched().len() as i32;
    let start_position = model.position_at(index);
    Some(Range {
        start_line_number: start_position.line_number,
        start_column: start_position.column,
        end_line_number: start_position.line_number,
        end_column: start_position.column + length,
    })
}

fn find_hex_color_information(range: Option<Range>, hex_value: &str) -> Option<IColorInformation> {
    let range = range?;
    let parsed_hex_color = format::css::parse_hex(hex_value)?;
    let rgba = parsed_hex_color.rgba();
    Some(IColorInformation {
        range,
        color: to_icolor(f64::from(rgba.r), f64::from(rgba.g), f64::from(rgba.b), rgba.a),
    })
}

/// O `parsedRegex[index]` do original, `undefined` (`NaN` nas contas) fora do vetor.
fn nth(values: &[f64], index: usize) -> f64 {
    values.get(index).copied().unwrap_or(f64::NAN)
}

fn find_rgb_color_information(
    range: Option<Range>,
    matches: &[RegExpMatchArray],
    is_alpha: bool,
) -> Option<IColorInformation> {
    let range = range?;
    let [match_] = matches else { return None };
    let parsed_regex = parse_capture_groups(match_);
    Some(IColorInformation {
        range,
        color: to_icolor(
            nth(&parsed_regex, 0),
            nth(&parsed_regex, 1),
            nth(&parsed_regex, 2),
            if is_alpha { nth(&parsed_regex, 3) } else { 1.0 },
        ),
    })
}

fn find_hsl_color_information(
    range: Option<Range>,
    matches: &[RegExpMatchArray],
    is_alpha: bool,
) -> Option<IColorInformation> {
    let range = range?;
    let [match_] = matches else { return None };
    let parsed_regex = parse_capture_groups(match_);
    let color_equivalent = Color::from(Hsla::new(
        nth(&parsed_regex, 0),
        nth(&parsed_regex, 1) / 100.0,
        nth(&parsed_regex, 2) / 100.0,
        if is_alpha { nth(&parsed_regex, 3) } else { 1.0 },
    ));
    let rgba = color_equivalent.rgba();
    Some(IColorInformation {
        range,
        color: to_icolor(f64::from(rgba.r), f64::from(rgba.g), f64::from(rgba.b), rgba.a),
    })
}

/// Validação inicial de RGB e HSL (inclusive a sintaxe CSS Level 4 com `/`) e dos hexadecimais.
const INITIAL_VALIDATION_REGEX: &str = r##"\b(rgb|rgba|hsl|hsla)(\([0-9\s,.\%\/]*\))|^(#)([A-Fa-f0-9]{3})\b|^(#)([A-Fa-f0-9]{4})\b|^(#)([A-Fa-f0-9]{6})\b|^(#)([A-Fa-f0-9]{8})\b|(?<=['"\s])(#)([A-Fa-f0-9]{3})\b|(?<=['"\s])(#)([A-Fa-f0-9]{4})\b|(?<=['"\s])(#)([A-Fa-f0-9]{6})\b|(?<=['"\s])(#)([A-Fa-f0-9]{8})\b"##;

/// Aceita a vírgula (`rgb(255, 0, 0)`) e a sintaxe CSS Level 4 com espaços (`rgb(255 0 0)`).
const RGB_PARAMETERS_REGEX: &str = r"^\(\s*(25[0-5]|2[0-4][0-9]|1[0-9]{2}|[1-9][0-9]|[0-9])\s*[\s,]\s*(25[0-5]|2[0-4][0-9]|1[0-9]{2}|[1-9][0-9]|[0-9])\s*[\s,]\s*(25[0-5]|2[0-4][0-9]|1[0-9]{2}|[1-9][0-9]|[0-9])\s*\)$";

/// Aceita `rgba(255, 0, 0, 0.5)` e `rgba(255 0 0 / 0.5)`.
const RGBA_PARAMETERS_REGEX: &str = r"^\(\s*(25[0-5]|2[0-4][0-9]|1[0-9]{2}|[1-9][0-9]|[0-9])\s*[\s,]\s*(25[0-5]|2[0-4][0-9]|1[0-9]{2}|[1-9][0-9]|[0-9])\s*[\s,]\s*(25[0-5]|2[0-4][0-9]|1[0-9]{2}|[1-9][0-9]|[0-9])\s*(?:[\s,]|[\s]*\/)\s*(0[.][0-9]+|[.][0-9]+|[01][.]|[01])\s*\)$";

const HSL_PARAMETERS_REGEX: &str = r"^\(\s*((?:360(?:\.0+)?|(?:36[0]|3[0-5][0-9]|[12][0-9][0-9]|[1-9]?[0-9])(?:\.\d+)?))\s*[\s,]\s*(100(?:\.0+)?|\d{1,2}[.]\d*|\d{1,2})%\s*[\s,]\s*(100(?:\.0+)?|\d{1,2}[.]\d*|\d{1,2})%\s*\)$";

/// Aceita `hsla(253, 100%, 50%, 0.5)` e `hsla(253 100% 50% / 0.5)`.
const HSLA_PARAMETERS_REGEX: &str = r"^\(\s*((?:360(?:\.0+)?|(?:36[0]|3[0-5][0-9]|[12][0-9][0-9]|[1-9]?[0-9])(?:\.\d+)?))\s*[\s,]\s*(100(?:\.0+)?|\d{1,2}[.]\d*|\d{1,2})%\s*[\s,]\s*(100(?:\.0+)?|\d{1,2}[.]\d*|\d{1,2})%\s*(?:[\s,]|[\s]*\/)\s*(0[.][0-9]+|[.][0-9]+|[01][.]0*|[01])\s*\)$";

fn global_multiline(pattern: &str) -> JsRegExp {
    JsRegExp::from_str(pattern, "gm").expect("o padrão fixo do computador de cores é válido")
}

fn compute_colors(model: &dyn IDocumentColorComputerTarget) -> Vec<IColorInformation> {
    let mut result = Vec::new();
    // Validação inicial para RGB e HSL (inclusive a sintaxe CSS Level 4 com o separador `/`).
    let initial_validation_regex = global_multiline(INITIAL_VALIDATION_REGEX);
    let initial_validation_matches = model.find_matches(&initial_validation_regex);

    // Foram achadas cores em potencial: valida os parâmetros.
    if initial_validation_matches.is_empty() {
        return result;
    }
    let rgb = global_multiline(RGB_PARAMETERS_REGEX);
    let rgba = global_multiline(RGBA_PARAMETERS_REGEX);
    let hsl = global_multiline(HSL_PARAMETERS_REGEX);
    let hsla = global_multiline(HSLA_PARAMETERS_REGEX);

    for initial_match in &initial_validation_matches {
        let initial_capture_groups: Vec<&Vec<u16>> = initial_match.values.iter().flatten().collect();
        let color_scheme = initial_capture_groups.get(1).map(|scheme| String::from_utf16_lossy(scheme));
        let color_parameters = initial_capture_groups.get(2).filter(|parameters| !parameters.is_empty());
        let (Some(color_scheme), Some(color_parameters)) = (color_scheme, color_parameters) else {
            continue;
        };
        let range = || find_range(model, initial_match);
        let color_information = match color_scheme.as_str() {
            // Aceita a vírgula (`rgb(255, 0, 0)`) e a sintaxe CSS Level 4 com espaços (`rgb(255 0 0)`).
            "rgb" => find_rgb_color_information(range(), &match_all(color_parameters, &rgb), false),
            // Aceita `rgba(255, 0, 0, 0.5)` e `rgba(255 0 0 / 0.5)`.
            "rgba" => find_rgb_color_information(range(), &match_all(color_parameters, &rgba), true),
            "hsl" => find_hsl_color_information(range(), &match_all(color_parameters, &hsl), false),
            // Aceita `hsla(253, 100%, 50%, 0.5)` e `hsla(253 100% 50% / 0.5)`.
            "hsla" => find_hsl_color_information(range(), &match_all(color_parameters, &hsla), true),
            "#" => {
                let hex = format!("{color_scheme}{}", String::from_utf16_lossy(color_parameters));
                find_hex_color_information(range(), &hex)
            }
            _ => None,
        };
        if let Some(color_information) = color_information {
            result.push(color_information);
        }
    }
    result
}

/// `computeDefaultDocumentColors`: todas as cores padrão do documento.
pub fn compute_default_document_colors(model: &dyn IDocumentColorComputerTarget) -> Vec<IColorInformation> {
    compute_colors(model)
}
