//! Porte de `src/charsetprober.js`: a interface comum dos probers e os três filtros de buffer.

use crate::Error;

pub trait CharsetProber {
    fn reset(&mut self);

    /// `null` no original vira `None`.
    fn get_charset_name(&self) -> Option<String>;

    /// O original lança `Unimplemented method getSupportedCharsetNames()` nos probers que não a
    /// definem, e nenhum chamador a usa neles; aqui o padrão é a lista só com o nome do prober.
    fn get_supported_charset_names(&self) -> Vec<String> {
        self.get_charset_name().into_iter().collect()
    }

    fn feed(&mut self, buf: &[u8]) -> Result<u8, Error>;

    fn get_state(&self) -> u8;

    fn get_confidence(&self) -> f64;
}

/// `buf.slice(start, end)` do JavaScript: índices negativos contam do fim e os fora do intervalo são aparados.
pub fn js_slice(buf: &[u8], start: i64, end: Option<i64>) -> &[u8] {
    let len = buf.len() as i64;
    let clamp = |index: i64| if index < 0 { (len + index).max(0) } else { index.min(len) };
    let from = clamp(start);
    let to = end.map_or(len, clamp);
    if from >= to { &[] } else { &buf[from as usize..to as usize] }
}

/// `filterHighBitOnly`: `aBuf.replace(/[\x00-\x7F]+/g, " ")`.
pub fn filter_high_bit_only(buf: &[u8]) -> Vec<u8> {
    replace_runs(buf, |byte| byte <= 0x7F)
}

/// `filterWithoutEnglishLetters`: `aBuf.replace(/[A-Za-z]+/g, " ")`.
pub fn filter_without_english_letters(buf: &[u8]) -> Vec<u8> {
    replace_runs(buf, |byte| byte.is_ascii_alphabetic())
}

/// Cada sequência máxima de bytes que satisfaz `in_run` vira um único espaço.
fn replace_runs(buf: &[u8], in_run: impl Fn(u8) -> bool) -> Vec<u8> {
    let mut result = Vec::with_capacity(buf.len());
    let mut inside = false;
    for &byte in buf {
        if in_run(byte) {
            if !inside {
                result.push(b' ');
                inside = true;
            }
        } else {
            result.push(byte);
            inside = false;
        }
    }
    result
}

/// Devolve uma cópia de `buf` que só guarda as sequências de letras inglesas e bytes altos que não estão
/// entre `<>`. A exceção são as tags PHP, que começam em `<?` e acabam em `?>`. Vale para qualquer script
/// que misture letras inglesas e ASCII estendido, mas hoje só o `Latin1Prober` o usa.
pub fn remove_xml_tags(buf: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(buf.len());
    let mut in_tag = false;
    let mut prev = 0usize;

    for (curr, &c) in buf.iter().enumerate() {
        if c == b'>' && (curr == 0 || buf[curr - 1] != b'?') {
            prev = curr + 1;
            in_tag = false;
        } else if c == b'<' && buf.get(curr + 1) != Some(&b'?') {
            if curr > prev && !in_tag {
                result.extend_from_slice(&buf[prev..curr]);
                result.push(b' ');
            }
            in_tag = true;
        }
    }

    if !in_tag {
        result.extend_from_slice(&buf[prev..]);
    }

    result
}
