//! Porte de `yarr/YarrFlags.h` e `YarrFlags.cpp`.
//!
//! `OptionSet<Flags>` vira `FlagSet`, uma struct sobre `u16` com os mesmos bits do enum `Flags`.

use crate::wtf::option_set::{OptionSet, OptionSetFlag};
use crate::wtf::text::string_impl::CharType;

// As flags devem estar em ordem alfabética: (chave, nome, índice).
// `JSC_REGEXP_FLAGS`.
const REGEXP_FLAGS: [(u8, Flags); 8] = [
    (b'd', Flags::HasIndices),
    (b'g', Flags::Global),
    (b'i', Flags::IgnoreCase),
    (b'm', Flags::Multiline),
    (b's', Flags::DotAll),
    (b'u', Flags::Unicode),
    (b'v', Flags::UnicodeSets),
    (b'y', Flags::Sticky),
];

/// `numberOfFlags`.
pub const NUMBER_OF_FLAGS: usize = REGEXP_FLAGS.len();

/// `enum class Flags : uint16_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Flags {
    HasIndices = 1 << 0,
    Global = 1 << 1,
    IgnoreCase = 1 << 2,
    Multiline = 1 << 3,
    DotAll = 1 << 4,
    Unicode = 1 << 5,
    UnicodeSets = 1 << 6,
    Sticky = 1 << 7,
    // `1 << numberOfFlags`, com numberOfFlags = 8 (as oito flags acima).
    DeletedValue = 1 << 8,
}

impl OptionSetFlag for Flags {
    type Mask = u16;
    const NONE: u16 = 0;
    const ALL: &'static [Flags] = &[
        Flags::HasIndices,
        Flags::Global,
        Flags::IgnoreCase,
        Flags::Multiline,
        Flags::DotAll,
        Flags::Unicode,
        Flags::UnicodeSets,
        Flags::Sticky,
        Flags::DeletedValue,
    ];

    fn bit(self) -> u16 {
        self as u16
    }
}

/// `OptionSet<Flags>`.
pub type FlagSet = OptionSet<Flags>;

/// `FlagsString`: `numberOfFlags + 1` bytes, com o terminador nulo.
pub type FlagsString = [u8; NUMBER_OF_FLAGS + 1];

/// `parseFlags(StringView)`. Recebe as unidades de código da string (`StringView::codeUnits()`).
pub fn parse_flags<C: CharType>(string: &[C]) -> Option<FlagSet> {
    let mut flags = FlagSet::empty();
    for &character in string {
        let unit = character.to_u16();
        let mut found = None;
        for (key, flag) in REGEXP_FLAGS {
            if unit == key as u16 {
                found = Some(flag);
                break;
            }
        }
        match found {
            Some(flag) => {
                if flags.contains(flag) {
                    return None;
                }
                flags.add(flag);
            }
            None => return None,
        }
    }

    // Só se pode especificar uma das flags 'u' e 'v'.
    if flags.contains(Flags::Unicode) && flags.contains(Flags::UnicodeSets) {
        return None;
    }

    Some(flags)
}

/// `flagsString(OptionSet<Flags>)`.
pub fn flags_string(flags: FlagSet) -> FlagsString {
    let mut string = [0u8; NUMBER_OF_FLAGS + 1];
    let mut index = 0;

    for (key, flag) in REGEXP_FLAGS {
        if flags.contains(flag) {
            string[index] = key;
            index += 1;
        }
    }

    assert!(index < string.len());
    string[index] = 0;
    string
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_flags_but_v() {
        let flags = parse_flags::<u8>(b"gimsuyd").unwrap();
        assert!(flags.contains(Flags::Global));
        assert!(flags.contains(Flags::IgnoreCase));
        assert!(flags.contains(Flags::Multiline));
        assert!(flags.contains(Flags::DotAll));
        assert!(flags.contains(Flags::Unicode));
        assert!(flags.contains(Flags::Sticky));
        assert!(flags.contains(Flags::HasIndices));
        assert!(!flags.contains(Flags::UnicodeSets));
        assert_eq!(flags.to_raw(), 0b1011_1111);
    }

    #[test]
    fn parses_utf16_and_empty() {
        let units: Vec<u16> = "gi".encode_utf16().collect();
        assert_eq!(parse_flags(&units).unwrap().to_raw(), 0b110);
        assert_eq!(parse_flags::<u8>(b"").unwrap(), FlagSet::empty());
    }

    #[test]
    fn rejects_repeated_invalid_and_u_with_v() {
        assert!(parse_flags::<u8>(b"gg").is_none());
        assert!(parse_flags::<u8>(b"x").is_none());
        assert!(parse_flags::<u8>(b"G").is_none());
        assert!(parse_flags::<u8>(b"uv").is_none());
        assert!(parse_flags::<u16>(&[0x0100 + b'g' as u16]).is_none());
    }

    #[test]
    fn flags_string_is_alphabetical() {
        let flags = parse_flags::<u8>(b"ysmgdi").unwrap();
        let string = flags_string(flags);
        assert_eq!(&string[..7], b"dgimsy\0");
    }
}
