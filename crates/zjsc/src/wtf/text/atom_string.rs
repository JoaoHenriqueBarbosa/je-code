//! Tradução de `WTF/wtf/text/AtomString.{h,cpp}`.
//!
//! `AtomString` é uma `String` cujo `StringImpl` é um átomo (ou, pelo construtor `UniquedStringImpl`,
//! um símbolo). A igualdade e o hash são pelo ponteiro do `StringImpl`, como o `operator==` do C++.
//!
//! Fora desta fatia, e por quê:
//!
//! - as sobrecargas com `StringView` (`contains`, `find`, `startsWith`, `endsWith` e as variantes
//!   `IgnoringASCIICase`), `equalIgnoringASCIICase`, `equalLettersIgnoringASCIICase`,
//!   `startsWithLettersIgnoringASCIICase`, `toDouble`/`toFloat` e `makeStringByReplacingAll`: o
//!   `StringView` e os métodos de `String` correspondentes ainda não existem no porte;
//! - `number(float)` e `number(double)`: dependem do `numberToStringAndSize` de `wtf/dtoa.h`, que
//!   ainda não foi portado. As inteiras (`number_i32`, `number_u32`, `number_u64`) estão aqui;
//! - o que é só `USE(CF)`, `USE(FOUNDATION)`, `OS(WINDOWS)` e `show()` de depuração.

use std::hash::{Hash, Hasher};
use std::rc::Rc;

use crate::wtf::ascii_ctype::{is_ascii_lower, is_ascii_upper, to_ascii_lower, to_ascii_upper};
use crate::wtf::text::atom_string_impl::{equal_characters, equal_string_impl, AtomStringImpl};
use crate::wtf::text::string_impl::{CaseConvertType, StringImpl};
use crate::wtf::text::wtf_string::String as WtfString;
use crate::wtf::unicode::character_names::REPLACEMENT_CHARACTER;

/// `notFound`.
const NOT_FOUND: usize = usize::MAX;

/// `convertASCIICase`: abaixo desse comprimento a conversão usa um buffer local, sem alocar um
/// `StringImpl` (é provável que o resultado já esteja na tabela de átomos).
const LOCAL_BUFFER_SIZE: usize = 100;

/// O `String(RefPtr<StringImpl>)` do C++. Concentra o único ponto que depende de como o campo de
/// `wtf_string::String` é exposto.
fn string_from_impl(m_impl: Option<Rc<StringImpl>>) -> WtfString {
    WtfString { m_impl }
}

/// `String::impl()`.
fn impl_of(string: &WtfString) -> Option<&Rc<StringImpl>> {
    string.m_impl.as_ref()
}

/// `String::releaseImpl()`.
fn take_impl(string: &mut WtfString) -> Option<Rc<StringImpl>> {
    string.m_impl.take()
}

/// `class AtomString`.
#[derive(Clone, Default, Debug)]
pub struct AtomString {
    m_string: WtfString,
}

/// `operator==(const AtomString&, const AtomString&)`: `a.impl() == b.impl()`.
impl PartialEq for AtomString {
    fn eq(&self, other: &Self) -> bool {
        match (self.impl_(), other.impl_()) {
            (None, None) => true,
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Eq for AtomString {}

impl Hash for AtomString {
    /// O hash padrão do `AtomString` no C++ é o do ponteiro.
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.impl_().map(Rc::as_ptr).hash(state);
    }
}

impl AtomString {
    /// `AtomString()`: nulo.
    pub fn new() -> AtomString {
        AtomString::default()
    }

    /// `AtomString(std::span<const Latin1Character>)`.
    pub fn from_latin1(characters: &[u8]) -> AtomString {
        AtomString::from_atom_impl(Some(AtomStringImpl::add(characters)))
    }

    /// `AtomString(std::span<const char16_t>)`.
    pub fn from_utf16(characters: &[u16]) -> AtomString {
        AtomString::from_atom_impl(Some(AtomStringImpl::add16(characters)))
    }

    /// `AtomString(AtomStringImpl*)`, `AtomString(RefPtr<AtomStringImpl>&&)` e
    /// `AtomString(Ref<AtomStringImpl>&&)`: o átomo entra como está, sem verificação.
    pub fn from_atom_impl(string: Option<Rc<StringImpl>>) -> AtomString {
        AtomString {
            m_string: string_from_impl(string),
        }
    }

    /// `AtomString(StringImpl*)`.
    pub fn from_string_impl(string: Option<&Rc<StringImpl>>) -> AtomString {
        AtomString::from_atom_impl(AtomStringImpl::add_string_impl_option(string))
    }

    /// `AtomString(const StaticStringImpl&)`.
    pub fn from_static_string_impl(string: &Rc<StringImpl>) -> AtomString {
        AtomString::from_atom_impl(Some(AtomStringImpl::add_static(string)))
    }

    /// `explicit AtomString(const String&)`.
    pub fn from_string(string: &WtfString) -> AtomString {
        AtomString::from_string_impl(impl_of(string))
    }

    /// `explicit AtomString(String&&)`.
    pub fn from_string_owned(mut string: WtfString) -> AtomString {
        AtomString::from_atom_impl(take_impl(&mut string).map(AtomStringImpl::add_rc))
    }

    /// `AtomString(StringImpl* baseString, unsigned start, unsigned length)`.
    pub fn from_substring(base_string: Option<&Rc<StringImpl>>, start: u32, length: u32) -> AtomString {
        AtomString::from_atom_impl(AtomStringImpl::add_substring(base_string, start, length))
    }

    /// `AtomString(UniquedStringImpl* uid)`: o símbolo ou átomo entra sem passar pela tabela.
    pub fn from_uniqued(uid: Option<Rc<StringImpl>>) -> AtomString {
        AtomString::from_atom_impl(uid)
    }

    /// `AtomString(ASCIILiteral)`: o literal nulo dá o átomo nulo, o vazio dá o átomo vazio.
    pub fn from_ascii_literal(literal: Option<&[u8]>) -> AtomString {
        match literal {
            None => AtomString::new(),
            Some(characters) if characters.is_empty() => empty_atom(),
            Some(characters) => AtomString::from_atom_impl(Some(AtomStringImpl::add_literal(characters))),
        }
    }

    /// `AtomString::lookUp(std::span<const char16_t>)`.
    pub fn look_up(characters: &[u16]) -> AtomString {
        AtomString::from_atom_impl(AtomStringImpl::look_up16(characters))
    }

    /// `AtomString::fromUTF8`: `null` se o UTF-8 for inválido. Entrada vazia dá o átomo vazio.
    pub fn from_utf8(characters: &[u8]) -> AtomString {
        if characters.is_empty() {
            return empty_atom();
        }
        AtomString::from_atom_impl(AtomStringImpl::add_utf8(characters))
    }

    /// `existingHash()`.
    pub fn existing_hash(&self) -> u32 {
        self.impl_().map_or(0, |string| string.existing_hash())
    }

    /// `string()` (e `operator const String&()`).
    pub fn string(&self) -> &WtfString {
        &self.m_string
    }

    /// `releaseString()`.
    pub fn release_string(&mut self) -> WtfString {
        std::mem::take(&mut self.m_string)
    }

    /// `impl()`.
    pub fn impl_(&self) -> Option<&Rc<StringImpl>> {
        impl_of(&self.m_string)
    }

    /// `releaseImpl()`.
    pub fn release_impl(&mut self) -> Option<Rc<StringImpl>> {
        take_impl(&mut self.m_string)
    }

    /// `is8Bit()`: a string nula é de 8 bits.
    pub fn is_8bit(&self) -> bool {
        self.impl_().is_none_or(|string| string.is_8bit())
    }

    /// `span8()`.
    pub fn span8(&self) -> &[u8] {
        self.impl_().map_or(&[], |string| string.span8())
    }

    /// `span16()`.
    pub fn span16(&self) -> &[u16] {
        self.impl_().map_or(&[], |string| string.span16())
    }

    /// `length()`.
    pub fn length(&self) -> u32 {
        self.impl_().map_or(0, |string| string.length())
    }

    /// `operator[](unsigned)`: a posição precisa existir, como no C++.
    pub fn char_at(&self, i: u32) -> u16 {
        match self.impl_() {
            Some(string) => string.char_at(i),
            None => panic!("AtomString::char_at em átomo nulo"),
        }
    }

    /// `AtomString::number(int)`.
    pub fn number_i32(number: i32) -> AtomString {
        AtomString::from_latin1(number.to_string().as_bytes())
    }

    /// `AtomString::number(unsigned)`.
    pub fn number_u32(number: u32) -> AtomString {
        AtomString::from_latin1(number.to_string().as_bytes())
    }

    /// `AtomString::number(unsigned long)` e `number(unsigned long long)`: são o mesmo tipo de 64
    /// bits no Linux x86_64.
    pub fn number_u64(number: u64) -> AtomString {
        AtomString::from_latin1(number.to_string().as_bytes())
    }

    /// `AtomString::number(double)`: o `numberToStringAndSize` (dtoa).
    pub fn number_f64(number: f64) -> AtomString {
        AtomString::from_string_owned(WtfString::number_f64(number))
    }

    /// `contains(char16_t)`.
    pub fn contains_char(&self, character: u16) -> bool {
        self.find_char(character, 0) != NOT_FOUND
    }

    /// `find(char16_t, size_t start)`.
    pub fn find_char(&self, character: u16, start: usize) -> usize {
        self.impl_()
            .map_or(NOT_FOUND, |string| string.find_character(character, start))
    }

    /// `find(CodeUnitMatchFunction, size_t start)`.
    pub fn find_matching(&self, match_function: impl Fn(u16) -> bool, start: usize) -> usize {
        self.impl_()
            .map_or(NOT_FOUND, |string| string.find_matching(match_function, start))
    }

    /// `startsWith(char16_t)`.
    pub fn starts_with_char(&self, character: u16) -> bool {
        self.impl_()
            .is_some_and(|string| string.length() != 0 && string.char_at(0) == character)
    }

    /// `endsWith(char16_t)`.
    pub fn ends_with_char(&self, character: u16) -> bool {
        self.impl_()
            .is_some_and(|string| string.length() != 0 && string.char_at(string.length() - 1) == character)
    }

    /// `isNull()`.
    pub fn is_null(&self) -> bool {
        self.impl_().is_none()
    }

    /// `isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.length() == 0
    }

    /// `convertToASCIILowercase()`.
    pub fn convert_to_ascii_lowercase(&self) -> AtomString {
        self.convert_ascii_case(CaseConvertType::Lower)
    }

    /// `convertToASCIIUppercase()`.
    pub fn convert_to_ascii_uppercase(&self) -> AtomString {
        self.convert_ascii_case(CaseConvertType::Upper)
    }

    /// `convertASCIICase<type>()`.
    fn convert_ascii_case(&self, case_type: CaseConvertType) -> AtomString {
        let Some(string) = self.impl_() else {
            return null_atom();
        };

        // Convert short strings without allocating a new StringImpl, since there's a good chance
        // these strings are already in the atom string table and so no memory allocation will be
        // required.
        let length = string.length() as usize;
        if string.is_8bit() && length <= LOCAL_BUFFER_SIZE {
            let characters = string.span8();
            let failing_index = characters.iter().position(|&character| match case_type {
                CaseConvertType::Lower => is_ascii_upper(character),
                CaseConvertType::Upper => is_ascii_lower(character),
            });
            let Some(failing_index) = failing_index else {
                return self.clone();
            };
            let mut local_buffer = [0u8; LOCAL_BUFFER_SIZE];
            local_buffer[..failing_index].copy_from_slice(&characters[..failing_index]);
            for i in failing_index..length {
                local_buffer[i] = match case_type {
                    CaseConvertType::Lower => to_ascii_lower(characters[i]),
                    CaseConvertType::Upper => to_ascii_upper(characters[i]),
                };
            }
            return AtomString::from_latin1(&local_buffer[..length]);
        }

        let converted_string = match case_type {
            CaseConvertType::Lower => string.convert_to_ascii_lowercase(),
            CaseConvertType::Upper => string.convert_to_ascii_uppercase(),
        };
        if Rc::ptr_eq(&converted_string, string) {
            return self.clone();
        }

        AtomString::from_string_impl(Some(&converted_string))
    }

    /// `operator==(const AtomString&, const String&)`: igualdade por conteúdo.
    pub fn equals_string(&self, other: &WtfString) -> bool {
        equal_string_impl(self.impl_().map(|string| &**string), impl_of(other).map(|string| &**string))
    }

    /// `operator==(const AtomString&, const Vector<char16_t>&)`.
    pub fn equals_utf16(&self, other: &[u16]) -> bool {
        self.impl_().is_some_and(|string| equal_characters(string, other))
    }
}

/// `nullAtom()`.
pub fn null_atom() -> AtomString {
    AtomString::new()
}

/// `emptyAtom()`.
pub fn empty_atom() -> AtomString {
    AtomString::from_atom_impl(Some(StringImpl::empty()))
}

/// `String::toExistingAtomString()`: o átomo só se ele já existe na tabela.
pub fn to_existing_atom_string(string: &WtfString) -> AtomString {
    let Some(string_impl) = impl_of(string) else {
        return AtomString::new();
    };
    if string_impl.is_atom() {
        return AtomString::from_atom_impl(Some(string_impl.clone()));
    }
    AtomString::from_atom_impl(AtomStringImpl::look_up_impl(Some(string_impl)))
}

/// `hasUnpairedSurrogate(StringView)`.
fn has_unpaired_surrogate(string: &StringImpl) -> bool {
    if string.is_8bit() {
        return false;
    }
    let characters = string.span16();
    let mut i = 0;
    while i < characters.len() {
        let c = characters[i];
        if (0xD800..0xDC00).contains(&c) && characters.get(i + 1).is_some_and(|next| (0xDC00..0xE000).contains(next)) {
            i += 2;
            continue;
        }
        if (0xD800..0xE000).contains(&c) {
            return true;
        }
        i += 1;
    }
    false
}

/// `replaceUnpairedSurrogatesWithReplacementCharacterInternal`: cada ponto de código substituto
/// vira U+FFFD, os pares ficam. O resultado sempre tem U+FFFD, então é de 16 bits em qualquer
/// caminho.
fn replace_unpaired_surrogates_internal(string: &StringImpl) -> Rc<StringImpl> {
    // Slow path: https://infra.spec.whatwg.org/#javascript-string-convert
    // Replaces unpaired surrogates with the replacement character.
    let characters = string.span16();
    let mut result: Vec<u16> = Vec::with_capacity(characters.len());
    let mut i = 0;
    while i < characters.len() {
        let c = characters[i];
        if (0xD800..0xDC00).contains(&c) && characters.get(i + 1).is_some_and(|next| (0xDC00..0xE000).contains(next)) {
            result.push(c);
            result.push(characters[i + 1]);
            i += 2;
            continue;
        }
        result.push(if (0xD800..0xE000).contains(&c) { REPLACEMENT_CHARACTER } else { c });
        i += 1;
    }
    StringImpl::create16(&result)
}

/// `replaceUnpairedSurrogatesWithReplacementCharacter(AtomString&&)`.
pub fn replace_unpaired_surrogates_with_replacement_character_atom(string: AtomString) -> AtomString {
    // Fast path for the case where there are no unpaired surrogates.
    let Some(string_impl) = string.impl_() else {
        return string;
    };
    if !has_unpaired_surrogate(string_impl) {
        return string;
    }
    AtomString::from_string_impl(Some(&replace_unpaired_surrogates_internal(string_impl)))
}

/// `replaceUnpairedSurrogatesWithReplacementCharacter(String&&)`.
pub fn replace_unpaired_surrogates_with_replacement_character(string: WtfString) -> WtfString {
    // Fast path for the case where there are no unpaired surrogates.
    let Some(string_impl) = impl_of(&string) else {
        return string;
    };
    if !has_unpaired_surrogate(string_impl) {
        return string;
    }
    string_from_impl(Some(replace_unpaired_surrogates_internal(string_impl)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn null_and_empty() {
        assert!(null_atom().is_null());
        assert!(null_atom().is_empty());
        assert_eq!(null_atom().length(), 0);
        assert!(null_atom().is_8bit());
        assert!(!empty_atom().is_null());
        assert!(empty_atom().is_empty());
        assert_eq!(null_atom(), AtomString::new());
        assert_ne!(null_atom(), empty_atom());
        assert_eq!(AtomString::from_latin1(b""), empty_atom());
        assert_eq!(AtomString::from_ascii_literal(Some(b"")), empty_atom());
        assert!(AtomString::from_ascii_literal(None).is_null());
    }

    #[test]
    fn equality_is_by_pointer() {
        let a = AtomString::from_latin1(b"same-atom");
        let b = AtomString::from_utf16(&wide("same-atom"));
        assert_eq!(a, b);
        assert_ne!(a, AtomString::from_latin1(b"other-atom"));
        assert_eq!(a.existing_hash(), a.impl_().unwrap().existing_hash());
        assert_ne!(a.existing_hash(), 0);
        assert_eq!(null_atom().existing_hash(), 0);

        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(a.clone());
        assert!(set.contains(&b));
        assert!(!set.contains(&AtomString::from_latin1(b"other-atom")));
    }

    #[test]
    fn accessors() {
        let atom = AtomString::from_latin1(b"hello");
        assert_eq!(atom.length(), 5);
        assert_eq!(atom.span8(), b"hello");
        assert!(atom.span16().is_empty());
        assert_eq!(atom.char_at(1), 'e' as u16);
        assert_eq!(atom.find_char('l' as u16, 0), 2);
        assert_eq!(atom.find_char('l' as u16, 3), 3);
        assert_eq!(atom.find_char('z' as u16, 0), NOT_FOUND);
        assert!(atom.contains_char('h' as u16));
        assert!(!atom.contains_char('H' as u16));
        assert!(atom.starts_with_char('h' as u16));
        assert!(atom.ends_with_char('o' as u16));
        assert!(!null_atom().starts_with_char('h' as u16));
        assert_eq!(atom.find_matching(|c| c == 'o' as u16, 0), 4);
        assert_eq!(null_atom().find_char('a' as u16, 0), NOT_FOUND);
    }

    #[test]
    fn substring_and_utf8() {
        let base = StringImpl::create(b"abcdef");
        assert_eq!(AtomString::from_substring(Some(&base), 1, 3), AtomString::from_latin1(b"bcd"));
        assert!(AtomString::from_substring(None, 1, 3).is_null());

        assert_eq!(AtomString::from_utf8(b"abc"), AtomString::from_latin1(b"abc"));
        assert_eq!(AtomString::from_utf8(b""), empty_atom());
        assert!(AtomString::from_utf8(&[0xFF]).is_null());
        assert_eq!(AtomString::from_utf8("\u{20AC}".as_bytes()).span16(), &[0x20AC]);
    }

    #[test]
    fn numbers() {
        assert_eq!(AtomString::number_i32(-42), AtomString::from_latin1(b"-42"));
        assert_eq!(AtomString::number_u32(0), AtomString::from_latin1(b"0"));
        assert_eq!(AtomString::number_u64(u64::MAX).span8(), b"18446744073709551615");
    }

    #[test]
    fn ascii_case_conversion() {
        let mixed = AtomString::from_latin1(b"HeLLo");
        let lower = mixed.convert_to_ascii_lowercase();
        assert_eq!(lower, AtomString::from_latin1(b"hello"));
        assert_eq!(lower.convert_to_ascii_lowercase(), lower);
        assert_eq!(mixed.convert_to_ascii_uppercase(), AtomString::from_latin1(b"HELLO"));
        assert_eq!(null_atom().convert_to_ascii_lowercase(), null_atom());

        // Acima do buffer local, pelo caminho do `StringImpl`.
        let long_upper = vec![b'A'; 150];
        let long = AtomString::from_latin1(&long_upper);
        assert_eq!(long.convert_to_ascii_lowercase().span8(), vec![b'a'; 150].as_slice());
        let long_lower = AtomString::from_latin1(&vec![b'b'; 150]);
        assert_eq!(long_lower.convert_to_ascii_lowercase(), long_lower);

        // 16 bits sem maiúsculas ASCII fica igual.
        let euro = AtomString::from_utf16(&[0x20AC, 0x61]);
        assert_eq!(euro.convert_to_ascii_lowercase(), euro);
        assert_eq!(euro.convert_to_ascii_uppercase().span16(), &[0x20AC, 0x41]);
    }

    #[test]
    fn unpaired_surrogates() {
        let clean = AtomString::from_utf16(&wide("ok\u{1F600}"));
        assert_eq!(replace_unpaired_surrogates_with_replacement_character_atom(clean.clone()), clean);

        let broken = AtomString::from_utf16(&[0x61, 0xD83D, 0x62, 0xDE00, 0xD83D, 0xDE00]);
        let fixed = replace_unpaired_surrogates_with_replacement_character_atom(broken);
        assert_eq!(fixed.span16(), &[0x61, 0xFFFD, 0x62, 0xFFFD, 0xD83D, 0xDE00]);
        assert!(fixed.impl_().unwrap().is_atom());

        assert!(replace_unpaired_surrogates_with_replacement_character_atom(null_atom()).is_null());
    }

    #[test]
    fn existing_atom_string() {
        let atom = AtomString::from_latin1(b"existing-atom");
        let plain = string_from_impl(Some(StringImpl::create(b"existing-atom")));
        assert_eq!(to_existing_atom_string(&plain), atom);
        let unknown = string_from_impl(Some(StringImpl::create(b"unknown-atom-text")));
        assert!(to_existing_atom_string(&unknown).is_null());
        assert!(to_existing_atom_string(&WtfString::default()).is_null());
        assert!(atom.equals_string(&plain));
        assert!(atom.equals_utf16(&wide("existing-atom")));
        assert!(!null_atom().equals_utf16(&[]));
        assert_eq!(AtomString::from_string(&plain), atom);
        assert_eq!(AtomString::from_string_owned(plain), atom);
    }
}
