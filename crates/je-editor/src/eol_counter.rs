//! Porte de `src/vs/editor/common/core/misc/eolCounter.ts`.

use je_base::char_code;

/// `StringEOL`: os fins de linha vistos num texto, combináveis com `|` (o `Invalid` é um `\r` sozinho).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StringEol(pub u8);

impl StringEol {
    pub const UNKNOWN: StringEol = StringEol(0);
    pub const LF: StringEol = StringEol(1);
    pub const CRLF: StringEol = StringEol(2);
    pub const INVALID: StringEol = StringEol(3);
}

impl std::ops::BitOrAssign for StringEol {
    fn bitor_assign(&mut self, rhs: StringEol) {
        self.0 |= rhs.0;
    }
}

/// O resultado de [`count_eol`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EolCount {
    /// Quantas quebras de linha o texto tem.
    pub eol_count: usize,
    /// Comprimento da primeira linha.
    pub first_line_length: usize,
    /// Comprimento da última linha.
    pub last_line_length: usize,
    /// Os tipos de quebra encontrados.
    pub eol: StringEol,
}

/// `countEOL(text)`.
pub fn count_eol(text: &[u16]) -> EolCount {
    let mut eol_count = 0;
    let mut first_line_length = 0;
    let mut last_line_start = 0;
    let mut eol = StringEol::UNKNOWN;
    let len = text.len();
    let mut i = 0;
    while i < len {
        let chr = text[i];
        if chr == char_code::CarriageReturn {
            if eol_count == 0 {
                first_line_length = i;
            }
            eol_count += 1;
            if i + 1 < len && text[i + 1] == char_code::LineFeed {
                // caso \r\n
                eol |= StringEol::CRLF;
                i += 1; // pula o \n
            } else {
                // caso \r
                eol |= StringEol::INVALID;
            }
            last_line_start = i + 1;
        } else if chr == char_code::LineFeed {
            // caso \n
            eol |= StringEol::LF;
            if eol_count == 0 {
                first_line_length = i;
            }
            eol_count += 1;
            last_line_start = i + 1;
        }
        i += 1;
    }
    if eol_count == 0 {
        first_line_length = len;
    }
    EolCount {
        eol_count,
        first_line_length,
        last_line_length: len - last_line_start,
        eol,
    }
}
