//! Tradução de `WTF/wtf/text/StringConcatenate.h`: `tryMakeString` e `makeString`.
//!
//! No C++ cada argumento passa por um `StringTypeAdapter` (comprimento, 8 ou 16 bits, escrita). Aqui
//! o trait `StringTypeAdapter` faz o mesmo papel, com implementações para `&str`, `String` da WTF e
//! `char`. `tryMakeString` devolve `None` onde o C++ devolve a `String` nula (soma do comprimento
//! passa de `String::MaxLength`).
//!
//! `&str` do Rust é UTF-8; o adaptador o trata como texto: Latin1 se todos os pontos de código
//! cabem em 8 bits, UTF-16 caso contrário.
//!
//! Não portados: os adaptadores de número (`FormattedNumber`, `HexNumber`), `Indentation`,
//! `ASCIILiteral` separado (um `&str` ASCII já é Latin1) e `makeStringByInserting`/`Joining`.

use crate::wtf::text::string_impl::MAX_LENGTH;
use crate::wtf::text::wtf_string::String as WtfString;

/// `StringTypeAdapter<T>`: o que a concatenação precisa saber de cada pedaço.
pub trait StringTypeAdapter {
    /// `length()`.
    fn length(&self) -> usize;
    /// `is8Bit()`.
    fn is_8bit(&self) -> bool;
    /// `writeTo(LChar*)`; só chamado se todos os pedaços são de 8 bits.
    fn write_to_8bit(&self, destination: &mut Vec<u8>);
    /// `writeTo(UChar*)`.
    fn write_to_16bit(&self, destination: &mut Vec<u16>);
}

impl StringTypeAdapter for &str {
    fn length(&self) -> usize {
        if self.is_8bit() {
            self.chars().count()
        } else {
            self.encode_utf16().count()
        }
    }

    fn is_8bit(&self) -> bool {
        self.chars().all(|c| (c as u32) <= 0xFF)
    }

    fn write_to_8bit(&self, destination: &mut Vec<u8>) {
        destination.extend(self.chars().map(|c| c as u32 as u8));
    }

    fn write_to_16bit(&self, destination: &mut Vec<u16>) {
        destination.extend(self.encode_utf16());
    }
}

impl StringTypeAdapter for char {
    fn length(&self) -> usize {
        self.len_utf16()
    }

    fn is_8bit(&self) -> bool {
        (*self as u32) <= 0xFF
    }

    fn write_to_8bit(&self, destination: &mut Vec<u8>) {
        destination.push(*self as u32 as u8);
    }

    fn write_to_16bit(&self, destination: &mut Vec<u16>) {
        let mut buffer = [0u16; 2];
        destination.extend_from_slice(self.encode_utf16(&mut buffer));
    }
}

impl StringTypeAdapter for &WtfString {
    fn length(&self) -> usize {
        WtfString::length(self) as usize
    }

    fn is_8bit(&self) -> bool {
        WtfString::is_8bit(self)
    }

    fn write_to_8bit(&self, destination: &mut Vec<u8>) {
        destination.extend_from_slice(self.span8());
    }

    fn write_to_16bit(&self, destination: &mut Vec<u16>) {
        if WtfString::is_8bit(self) {
            destination.extend(self.span8().iter().map(|&c| u16::from(c)));
        } else {
            destination.extend_from_slice(self.span16());
        }
    }
}

impl StringTypeAdapter for WtfString {
    fn length(&self) -> usize {
        WtfString::length(self) as usize
    }

    fn is_8bit(&self) -> bool {
        WtfString::is_8bit(self)
    }

    fn write_to_8bit(&self, destination: &mut Vec<u8>) {
        <&WtfString as StringTypeAdapter>::write_to_8bit(&self, destination)
    }

    fn write_to_16bit(&self, destination: &mut Vec<u16>) {
        <&WtfString as StringTypeAdapter>::write_to_16bit(&self, destination)
    }
}

/// `tryMakeString(strings...)` sobre pedaços do mesmo tipo.
pub fn try_make_string<A: StringTypeAdapter>(parts: &[A]) -> Option<WtfString> {
    let refs: Vec<&dyn StringTypeAdapter> = parts.iter().map(|part| part as &dyn StringTypeAdapter).collect();
    try_make_string_dyn(&refs)
}

/// `tryMakeString(strings...)` com pedaços de tipos diferentes.
pub fn try_make_string_dyn(parts: &[&dyn StringTypeAdapter]) -> Option<WtfString> {
    let mut total: usize = 0;
    let mut are_all_8bit = true;
    for part in parts {
        total = total.checked_add(part.length())?;
        if total > MAX_LENGTH as usize {
            return None;
        }
        are_all_8bit &= part.is_8bit();
    }

    if total == 0 {
        return Some(WtfString::from_latin1(&[]));
    }

    if are_all_8bit {
        let mut buffer: Vec<u8> = Vec::with_capacity(total);
        for part in parts {
            part.write_to_8bit(&mut buffer);
        }
        Some(WtfString::from_latin1(&buffer))
    } else {
        let mut buffer: Vec<u16> = Vec::with_capacity(total);
        for part in parts {
            part.write_to_16bit(&mut buffer);
        }
        Some(WtfString::from_utf16(&buffer))
    }
}

/// `makeString(strings...)`: como `tryMakeString`, mas o estouro é `CRASH()`.
pub fn make_string_dyn(parts: &[&dyn StringTypeAdapter]) -> WtfString {
    match try_make_string_dyn(parts) {
        Some(string) => string,
        None => panic!("makeString: overflow do comprimento da string"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concatenates_latin1_and_utf16() {
        let s = try_make_string(&["abc", "def"]).unwrap();
        assert!(s.is_8bit());
        assert_eq!(s.span8(), b"abcdef");
        let wide = try_make_string(&["a", "\u{20AC}"]).unwrap();
        assert!(!wide.is_8bit());
        assert_eq!(wide.span16(), &[0x61, 0x20AC]);
        let mixed = make_string_dyn(&[&"x", &WtfString::from_latin1(b"yz"), &'!']);
        assert_eq!(mixed.span8(), b"xyz!");
    }
}
