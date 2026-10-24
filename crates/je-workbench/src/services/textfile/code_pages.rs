//! Páginas de código de um byte que o `iconv-lite` tem e o `encoding_rs` (WHATWG) não: cp437, cp850,
//! cp852, cp857, cp865, cp1125, koi8ru e koi8t (as três últimas conferidas contra o
//! `encodings/sbcs-data-generated.js` do `iconv-lite`). Cada tabela guarda só a metade alta (bytes 0x80 a 0xFF); a baixa é ASCII.
//! Byte sem mapeamento decodifica como U+FFFD e o caractere sem byte codifica como `?`, como o
//! `iconv-lite`.

use std::sync::OnceLock;

/// Uma página de código de um byte.
pub struct CodePage {
    high_half: &'static str,
    table: OnceLock<[char; 128]>,
}

impl CodePage {
    const fn new(high_half: &'static str) -> Self {
        Self { high_half, table: OnceLock::new() }
    }

    fn table(&self) -> &[char; 128] {
        self.table.get_or_init(|| {
            let mut table = ['\u{FFFD}'; 128];
            for (slot, ch) in table.iter_mut().zip(self.high_half.chars()) {
                *slot = ch;
            }
            table
        })
    }

    pub fn decode(&self, byte: u8) -> char {
        if byte < 0x80 { char::from(byte) } else { self.table()[usize::from(byte - 0x80)] }
    }

    /// `None` se o caractere não existe na página.
    pub fn encode(&self, ch: char) -> Option<u8> {
        if ch.is_ascii() {
            return u8::try_from(u32::from(ch)).ok();
        }
        if ch == '\u{FFFD}' {
            return None;
        }
        let index = self.table().iter().position(|&c| c == ch)?;
        u8::try_from(index + 0x80).ok()
    }
}

pub static CP437: CodePage = CodePage::new(concat!(
    "ÇüéâäàåçêëèïîìÄÅ",
    "ÉæÆôöòûùÿÖÜ¢£¥₧ƒ",
    "áíóúñÑªº¿⌐¬½¼¡«»",
    "░▒▓│┤╡╢╖╕╣║╗╝╜╛┐",
    "└┴┬├─┼╞╟╚╔╩╦╠═╬╧",
    "╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀",
    "αß\u{393}\u{3c0}\u{3a3}\u{3c3}\u{b5}\u{3c4}\u{3a6}\u{398}\u{3a9}\u{3b4}∞\u{3c6}\u{3b5}∩",
    "≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{a0}",
));

pub static CP850: CodePage = CodePage::new(concat!(
    "ÇüéâäàåçêëèïîìÄÅ",
    "ÉæÆôöòûùÿÖÜø£Ø×ƒ",
    "áíóúñÑªº¿®¬½¼¡«»",
    "░▒▓│┤ÁÂÀ©╣║╗╝¢¥┐",
    "└┴┬├─┼ãÃ╚╔╩╦╠═╬¤",
    "ðÐÊËÈıÍÎÏ┘┌█▄¦Ì▀",
    "ÓßÔÒõÕ\u{b5}þÞÚÛÙýÝ¯´",
    "\u{ad}±‗¾¶§÷¸°¨·¹³²■\u{a0}",
));

pub static CP852: CodePage = CodePage::new(concat!(
    "ÇüéâäůćçłëŐőîŹÄĆ",
    "ÉĹĺôöĽľŚśÖÜŤťŁ×č",
    "áíóúĄąŽžĘę¬źČş«»",
    "░▒▓│┤ÁÂĚŞ╣║╗╝Żż┐",
    "└┴┬├─┼Ăă╚╔╩╦╠═╬¤",
    "đĐĎËďŇÍÎě┘┌█▄ŢŮ▀",
    "ÓßÔŃńňŠšŔÚŕŰýÝţ´",
    "\u{ad}˝˛ˇ˘§÷¸°¨˙űŘř■\u{a0}",
));

pub static CP857: CodePage = CodePage::new(concat!(
    "ÇüéâäàåçêëèïîıÄÅ",
    "ÉæÆôöòûùİÖÜø£ØŞş",
    "áíóúñÑĞğ¿®¬½¼¡«»",
    "░▒▓│┤ÁÂÀ©╣║╗╝¢¥┐",
    "└┴┬├─┼ãÃ╚╔╩╦╠═╬¤",
    "ºªÊËÈ\u{fffd}ÍÎÏ┘┌█▄¦Ì▀",
    "ÓßÔÒõÕ\u{b5}\u{fffd}×ÚÛÙìÿ¯´",
    "\u{ad}±\u{fffd}¾¶§÷¸°¨·¹³²■\u{a0}",
));

/// cp437 com ø, Ø e ¤ nas posições 0x9B, 0x9D e 0xAF.
pub static CP865: CodePage = CodePage::new(concat!(
    "ÇüéâäàåçêëèïîìÄÅ",
    "ÉæÆôöòûùÿÖÜø£Ø₧ƒ",
    "áíóúñÑªº¿⌐¬½¼¡«¤",
    "░▒▓│┤╡╢╖╕╣║╗╝╜╛┐",
    "└┴┬├─┼╞╟╚╔╩╦╠═╬╧",
    "╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀",
    "αß\u{393}\u{3c0}\u{3a3}\u{3c3}\u{b5}\u{3c4}\u{3a6}\u{398}\u{3a9}\u{3b4}∞\u{3c6}\u{3b5}∩",
    "≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{a0}",
));

pub static CP1125: CodePage = CodePage::new(concat!(
    "АБВГДЕЖЗИЙКЛМНОП",
    "РСТУФХЦЧШЩЪЫЬЭЮЯ",
    "абвгдежзийклмноп",
    "░▒▓│┤╡╢╖╕╣║╗╝╜╛┐",
    "└┴┬├─┼╞╟╚╔╩╦╠═╬╧",
    "╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀",
    "рстуфхцчшщъыьэюя",
    "ЁёҐґЄєІіЇї·√№¤■\u{a0}",
));

pub static KOI8RU: CodePage = CodePage::new(concat!(
    "─│┌┐└┘├┤┬┴┼▀▄█▌▐",
    "░▒▓⌠■∙√≈≤≥\u{a0}⌡°²·÷",
    "═║╒ёє╔ії╗╘╙╚╛ґў╞",
    "╟╠╡ЁЄ╣ІЇ╦╧╨╩╪ҐЎ©",
    "юабцдефгхийклмно",
    "пярстужвьызшэщчъ",
    "ЮАБЦДЕФГХИЙКЛМНО",
    "ПЯРСТУЖВЬЫЗШЭЩЧЪ",
));

pub static KOI8T: CodePage = CodePage::new(concat!(
    "қғ‚Ғ„…†‡\u{fffd}‰ҳ‹ҲҷҶ\u{fffd}",
    "Қ‘’“”•\u{2013}\u{2014}\u{fffd}™\u{fffd}›\u{fffd}\u{fffd}\u{fffd}\u{fffd}",
    "\u{fffd}ӯӮё¤ӣ¦§\u{fffd}\u{fffd}\u{fffd}«¬\u{ad}®\u{fffd}",
    "°±²Ё\u{fffd}Ӣ¶·\u{fffd}№\u{fffd}»\u{fffd}\u{fffd}\u{fffd}©",
    "юабцдефгхийклмно",
    "пярстужвьызшэщчъ",
    "ЮАБЦДЕФГХИЙКЛМНО",
    "ПЯРСТУЖВЬЫЗШЭЩЧЪ",
));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_table_has_128_entries_and_round_trips() {
        for page in [&CP437, &CP850, &CP852, &CP857, &CP865, &CP1125, &KOI8RU, &KOI8T] {
            assert_eq!(page.high_half.chars().count(), 128);
            for byte in 0x80..=0xFFu8 {
                let ch = page.decode(byte);
                if ch != '\u{FFFD}' {
                    assert_eq!(page.encode(ch), Some(byte));
                }
            }
        }
    }
}
