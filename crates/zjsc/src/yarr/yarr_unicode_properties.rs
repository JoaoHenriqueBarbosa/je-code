//! Porte de `yarr/YarrUnicodeProperties.h` e `YarrUnicodeProperties.cpp`. As tabelas
//! (`UnicodePatternTables.h`) estão em `unicode_pattern_tables`, geradas por script.

use crate::wtf::text::wtf_string::String;
use crate::yarr::unicode_pattern_tables::{
    unicode_character_class_may_contain_strings, BINARY_PROPERTY_HASH_TABLE, CLASSES,
    GENERAL_CATEGORY_HASH_TABLE, SCRIPT_EXTENSION_HASH_TABLE, SCRIPT_HASH_TABLE,
    SEQUENCE_PROPERTY_HASH_TABLE,
};
use crate::yarr::yarr::BuiltInCharacterClassID;
use crate::yarr::yarr_pattern::{CharacterClass, CharacterClassWidths, CharacterRange, CompileMode};

/// `struct HashIndex`.
pub struct HashIndex {
    pub value: i16,
    pub next: i16,
}

/// `struct HashValue`.
pub struct HashValue {
    pub key: &'static str,
    pub index: i32,
}

/// `struct HashTable`.
pub struct HashTable {
    pub number_of_values: i32,
    pub index_mask: i32,
    pub values: &'static [HashValue],
    pub index: &'static [HashIndex],
}

impl HashTable {
    /// `HashTable::entry`.
    pub fn entry(&self, key: &String) -> i32 {
        let mut index_entry = (key.hash() & self.index_mask as u32) as usize;
        let mut value_index = self.index[index_entry].value;
        if value_index == -1 {
            return -1;
        }
        loop {
            let value = &self.values[value_index as usize];
            if *key == String::from_latin1(value.key.as_bytes()) {
                return value.index;
            }
            let next = self.index[index_entry].next;
            if next == -1 {
                return -1;
            }
            index_entry = next as usize;
            value_index = self.index[index_entry].value;
            debug_assert!(value_index != -1);
        }
    }
}

/// Uma `createCharacterClassN()` do C++: os argumentos do construtor e, nas propriedades de
/// sequência, as strings concatenadas em `.0` com o tamanho de cada uma em `.1`.
pub struct ClassData {
    pub matches8: &'static [u32],
    pub ranges8: &'static [CharacterRange],
    pub matches32: &'static [u32],
    pub ranges32: &'static [CharacterRange],
    pub widths: CharacterClassWidths,
    pub strings: Option<(&'static [u32], &'static [u8])>,
}

fn with_offset(property_index: i32) -> Option<BuiltInCharacterClassID> {
    if property_index == -1 {
        return None;
    }
    Some(BuiltInCharacterClassID(
        (BuiltInCharacterClassID::BaseUnicodePropertyID.0 as i32 + property_index) as u32,
    ))
}

/// `unicodeMatchPropertyValue`.
pub fn unicode_match_property_value(
    unicode_property_name: String,
    unicode_property_value: String,
) -> Option<BuiltInCharacterClassID> {
    let is = |name: &str| unicode_property_name == String::from_latin1(name.as_bytes());
    let property_index = if is("Script") || is("sc") {
        SCRIPT_HASH_TABLE.entry(&unicode_property_value)
    } else if is("Script_Extensions") || is("scx") {
        SCRIPT_EXTENSION_HASH_TABLE.entry(&unicode_property_value)
    } else if is("General_Category") || is("gc") {
        GENERAL_CATEGORY_HASH_TABLE.entry(&unicode_property_value)
    } else {
        -1
    };
    with_offset(property_index)
}

/// `unicodeMatchProperty`.
pub fn unicode_match_property(
    unicode_property_value: String,
    compile_mode: CompileMode,
) -> Option<BuiltInCharacterClassID> {
    let mut property_index = BINARY_PROPERTY_HASH_TABLE.entry(&unicode_property_value);
    if property_index == -1 {
        property_index = GENERAL_CATEGORY_HASH_TABLE.entry(&unicode_property_value);
    }
    if property_index == -1 && compile_mode == CompileMode::UnicodeSets {
        property_index = SEQUENCE_PROPERTY_HASH_TABLE.entry(&unicode_property_value);
    }
    with_offset(property_index)
}

fn property_index(unicode_class_id: BuiltInCharacterClassID) -> u32 {
    // Subtração `unsigned` do C++: as classes embutidas (`\d`, `\w`) dão a volta e caem fora da tabela.
    unicode_class_id.0.wrapping_sub(BuiltInCharacterClassID::BaseUnicodePropertyID.0)
}

/// `createUnicodeCharacterClassFor`.
pub fn create_unicode_character_class_for(unicode_class_id: BuiltInCharacterClassID) -> Box<CharacterClass> {
    let data = &CLASSES[property_index(unicode_class_id) as usize];
    let mut class = CharacterClass::with_matches(
        data.matches8,
        data.ranges8,
        data.matches32,
        data.ranges32,
        data.widths,
    );
    if let Some((mut chars, lengths)) = data.strings {
        class.strings.reserve_exact(lengths.len());
        for &length in lengths {
            let (string, rest) = chars.split_at(length as usize);
            class.strings.push(string.to_vec());
            chars = rest;
        }
        class.in_canonical_form = true;
    }
    Box::new(class)
}

/// `characterClassMayContainStrings`.
pub fn character_class_may_contain_strings(unicode_class_id: BuiltInCharacterClassID) -> bool {
    unicode_character_class_may_contain_strings(property_index(unicode_class_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> String {
        String::from_latin1(text.as_bytes())
    }

    #[test]
    fn lookups() {
        let any = unicode_match_property(s("Any"), CompileMode::Unicode).unwrap();
        assert_eq!(any, BuiltInCharacterClassID::BaseUnicodePropertyID);
        let lu = unicode_match_property(s("Lu"), CompileMode::Unicode).unwrap();
        let upper = unicode_match_property_value(s("gc"), s("Uppercase_Letter")).unwrap();
        assert_eq!(lu, upper);
        assert!(unicode_match_property(s("RGI_Emoji"), CompileMode::Unicode).is_none());
        let emoji = unicode_match_property(s("RGI_Emoji"), CompileMode::UnicodeSets).unwrap();
        assert!(character_class_may_contain_strings(emoji));
        assert_eq!(create_unicode_character_class_for(emoji).strings.len(), 2760);
        assert!(unicode_match_property_value(s("Script"), s("Greek")).is_some());
        assert!(unicode_match_property_value(s("Script"), s("Nope")).is_none());
        let ascii = create_unicode_character_class_for(unicode_match_property(s("ASCII"), CompileMode::Unicode).unwrap());
        assert_eq!(ascii.ranges8, vec![CharacterRange::new(0, 0x7f)]);
    }

    /// O gerador (`generateYarrUnicodePropertyTables.py`) usa o rapidhash de `hasher.py` mascarado em
    /// 24 bits, e `String::hash` usa o `StringHasher` do WTF (o mesmo rapidhash com
    /// `computeHashAndMaskTop8Bits`). Se divergissem, alguma chave de alguma tabela não seria achada.
    #[test]
    fn every_table_key_is_found_with_its_own_index() {
        let tables: [(&str, &HashTable); 5] = [
            ("general_category", &GENERAL_CATEGORY_HASH_TABLE),
            ("binary_property", &BINARY_PROPERTY_HASH_TABLE),
            ("script", &SCRIPT_HASH_TABLE),
            ("script_extension", &SCRIPT_EXTENSION_HASH_TABLE),
            ("sequence_property", &SEQUENCE_PROPERTY_HASH_TABLE),
        ];
        for (name, table) in tables {
            assert_eq!(table.values.len(), table.number_of_values as usize, "{name}");
            for value in table.values {
                assert_eq!(table.entry(&s(value.key)), value.index, "tabela {name}, chave {}", value.key);
            }
        }
    }

    #[test]
    fn hash_ignores_string_width() {
        // Latin1 e UTF-16 só com Latin1 têm o mesmo hash, como no WTF; a tabela é gerada sobre ASCII.
        let wide = String::from_utf16(&"Greek".encode_utf16().collect::<Vec<u16>>());
        assert_eq!(wide.hash(), s("Greek").hash());
        assert_eq!(SCRIPT_HASH_TABLE.entry(&wide), SCRIPT_HASH_TABLE.entry(&s("Greek")));
        assert_ne!(SCRIPT_HASH_TABLE.entry(&wide), -1);
    }

    #[test]
    fn real_property_values_are_found() {
        let scripts = [
            "Latin", "Latn", "Greek", "Grek", "Cyrillic", "Cyrl", "Han", "Hani", "Arabic", "Hebrew", "Thai",
            "Hiragana", "Katakana",
        ];
        for name in scripts {
            assert!(unicode_match_property_value(s("Script"), s(name)).is_some(), "Script={name}");
            assert!(unicode_match_property_value(s("sc"), s(name)).is_some(), "sc={name}");
            assert!(unicode_match_property_value(s("Script_Extensions"), s(name)).is_some(), "Script_Extensions={name}");
            assert!(unicode_match_property_value(s("scx"), s(name)).is_some(), "scx={name}");
        }
        for name in ["Lu", "Ll", "Nd", "Lowercase_Letter", "Uppercase_Letter"] {
            assert!(unicode_match_property_value(s("General_Category"), s(name)).is_some(), "General_Category={name}");
            assert!(unicode_match_property_value(s("gc"), s(name)).is_some(), "gc={name}");
        }
        let binary = [
            "Alphabetic", "Alpha", "Emoji", "ASCII_Hex_Digit", "AHex", "White_Space", "Uppercase", "Lowercase",
            "ID_Start", "Any", "ASCII", "Assigned", "Hex_Digit",
        ];
        for name in binary {
            assert!(unicode_match_property(s(name), CompileMode::Unicode).is_some(), "{name}");
        }
        // Aliases curtos e longos nomeiam a mesma classe.
        for (short, long) in [("Lu", "Uppercase_Letter"), ("Ll", "Lowercase_Letter"), ("AHex", "ASCII_Hex_Digit"), ("Alpha", "Alphabetic")] {
            assert_eq!(
                unicode_match_property(s(short), CompileMode::Unicode),
                unicode_match_property(s(long), CompileMode::Unicode),
                "{short} contra {long}"
            );
        }
        assert_eq!(
            unicode_match_property_value(s("Script"), s("Latn")),
            unicode_match_property_value(s("sc"), s("Latin"))
        );
        // Propriedades de sequência só existem com a flag v.
        for name in ["RGI_Emoji", "Basic_Emoji", "Emoji_Keycap_Sequence"] {
            assert!(unicode_match_property(s(name), CompileMode::Unicode).is_none(), "{name} sem v");
            assert!(unicode_match_property(s(name), CompileMode::UnicodeSets).is_some(), "{name} com v");
        }
    }

    #[test]
    fn invalid_names_return_none() {
        for name in ["", "latin", "LATIN", "Latin ", "Lux", "Alphabetical", "Emoji_", "NotAProperty", "Script=Latin", "\u{e9}"] {
            assert!(unicode_match_property(s(name), CompileMode::Unicode).is_none(), "{name:?}");
            assert!(unicode_match_property(s(name), CompileMode::UnicodeSets).is_none(), "{name:?} v");
            assert!(unicode_match_property_value(s("Script"), s(name)).is_none(), "Script={name:?}");
            assert!(unicode_match_property_value(s("gc"), s(name)).is_none(), "gc={name:?}");
        }
        // Nome de propriedade desconhecido ou valor no lugar errado.
        assert!(unicode_match_property_value(s("Nope"), s("Latin")).is_none());
        assert!(unicode_match_property_value(s("gc"), s("Latin")).is_none());
        assert!(unicode_match_property_value(s("Script"), s("Lu")).is_none());
    }
}
