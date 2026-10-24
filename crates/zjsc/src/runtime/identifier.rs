//! Tradução de `JavaScriptCore/runtime/Identifier.{h,cpp}` e `IdentifierInlines.h`.
//!
//! Fora desta fatia, e por quê:
//!
//! - `Identifier::from(VM&, double)`, `identifierToJSValue`, `identifierToSafePublicJSValue` e
//!   `dump(PrintStream&)`: dependem de `NumericStrings`, `JSString`, `Symbol` e `PrintStream`,
//!   que são das camadas seguintes. `from_u32`/`from_i32` usam `AtomString::number_*`;
//! - `IdentifierRepHash`, `IdentifierSet` e `IdentifierMap`: a chave `UniquedKey` e os `HashMap`
//!   do Rust os substituem;
//! - `checkCurrentAtomStringTable`: só existe sob `NDEBUG` desligado;
//! - `smallStrings.singleCharacterStringRep(c)`: devolve o átomo de um caractere, o mesmo que a
//!   tabela de átomos dá a `AtomStringImpl::add`, então o porte passa direto pela tabela.

use std::rc::Rc;

use crate::runtime::private_name::PrivateName;
use crate::runtime::vm::VM;
use crate::wtf::text::atom_string::AtomString;
use crate::wtf::text::conversion_mode::ConversionMode;
use crate::wtf::text::string_impl::{equal_span, CharType, StringImpl, UniquedKey};
use crate::wtf::text::symbol_impl::SymbolImpl;
use crate::wtf::text::wtf_string::String as WtfString;

/// `MAX_ARRAY_INDEX` de `ArrayConventions.h`: `MAX_STORAGE_VECTOR_INDEX - 1`, com o índice
/// máximo do vetor de armazenamento em `0xFFFFFFFF`.
pub const MAX_ARRAY_INDEX: u32 = 0xFFFF_FFFE;

/// `isIndex(uint32_t)`.
pub fn is_index(index: u32) -> bool {
    index <= MAX_ARRAY_INDEX
}

/// `parseIndex(std::span<const CharType>)`. `CharType` é `u8` (`LChar`) ou `u16` (`UChar`).
pub fn parse_index<T: Copy + Into<u32>>(characters: &[T]) -> Option<u32> {
    // An empty string is not a number.
    let (first, mut rest) = characters.split_first()?;

    // Get the first character, turning it into a digit.
    let mut value: u32 = (*first).into().wrapping_sub('0' as u32);
    if value > 9 {
        return None;
    }

    // Check for leading zeros. If the first characher is 0, then the
    // length of the string must be one - e.g. "042" is not equal to "42".
    if value == 0 && characters.len() > 1 {
        return None;
    }

    while let Some((front, tail)) = rest.split_first() {
        // Multiply value by 10, checking for overflow out of 32 bits.
        if value > 0xFFFF_FFFFu32 / 10 {
            return None;
        }
        value = value.wrapping_mul(10);

        // Get the next character, turning it into a digit.
        let mut new_value: u32 = (*front).into().wrapping_sub('0' as u32);
        if new_value > 9 {
            return None;
        }

        // Add in the old value, checking for overflow out of 32 bits.
        new_value = new_value.wrapping_add(value);
        if new_value < value {
            return None;
        }
        value = new_value;
        rest = tail;
    }

    if !is_index(value) {
        return None;
    }
    Some(value)
}

/// `parseIndex(const StringImpl&)`.
pub fn parse_index_impl(string: &StringImpl) -> Option<u32> {
    if string.is_8bit() {
        parse_index(string.span8())
    } else {
        parse_index(string.span16())
    }
}

/// `parseIndex(const Identifier&)`.
pub fn parse_index_identifier(identifier: &Identifier) -> Option<u32> {
    let uid = identifier.impl_()?;
    if uid.0.is_symbol() {
        return None;
    }
    parse_index_impl(&uid.0)
}

/// `class Identifier`.
///
/// `m_private` não existe no C++: lá o `SymbolImpl` é o próprio `StringImpl` e `isPrivate()` lê as
/// flags dele. No porte o `StringImpl` do símbolo não aponta de volta para o `SymbolImpl`, então o
/// `Identifier` carrega a flag que `isPrivateName()` consulta.
#[derive(Clone, Default, Debug, PartialEq, Eq, Hash)]
pub struct Identifier {
    m_string: AtomString,
    m_private: bool,
}

impl Identifier {
    /// `Identifier(EmptyIdentifierFlag)`.
    pub fn empty_identifier() -> Identifier {
        let empty = StringImpl::empty();
        debug_assert!(empty.is_atom());
        Identifier { m_string: AtomString::from_atom_impl(Some(empty)), m_private: false }
    }

    /// `Identifier()`.
    pub fn null_identifier() -> Identifier {
        Identifier::default()
    }

    /// `string()`.
    pub fn string(&self) -> &AtomString {
        &self.m_string
    }

    /// `impl()`: o `UniquedStringImpl*`, nulo como `None`.
    pub fn impl_(&self) -> Option<UniquedKey> {
        self.m_string.impl_().map(|string| UniquedKey(Rc::clone(string)))
    }

    /// `length()`.
    pub fn length(&self) -> u32 {
        self.m_string.length()
    }

    /// `utf8()`: o `CString` do C++ sem o terminador; a string nula dá bytes vazios.
    pub fn utf8(&self) -> Vec<u8> {
        match self.m_string.impl_() {
            Some(string) => string.utf8(ConversionMode::LenientConversion),
            None => Vec::new(),
        }
    }

    /// `fromString(VM&, std::span<const Latin1Character>)`, `fromString(VM&, ASCIILiteral)` e
    /// `fromString(VM&, std::span<const char16_t>)`.
    pub fn from_span<T: CharType>(_vm: &VM, characters: &[T]) -> Identifier {
        if characters.is_empty() {
            return Identifier::empty_identifier();
        }
        // `AtomStringImpl::add(span)`: a versão de 16 bits vira 8 bits quando tudo cabe em Latin1
        // (`create8BitIfPossible`), como no `Identifier::fromString` do C++.
        let m_string = if T::SIZE == 1 {
            let narrow: Vec<u8> = characters.iter().map(|&c| c.to_u16() as u8).collect();
            AtomString::from_latin1(&narrow)
        } else {
            let wide: Vec<u16> = characters.iter().map(|&c| c.to_u16()).collect();
            AtomString::from_utf16(&wide)
        };
        Identifier { m_string, m_private: false }
    }

    /// `createLatin1(VM&, std::span<const char16_t>)`: cada unidade cabe em Latin1 por contrato.
    pub fn create_latin1(_vm: &VM, characters: &[u16]) -> Identifier {
        let narrow: Vec<u8> = characters.iter().map(|&c| c as u8).collect();
        Identifier { m_string: AtomString::from_latin1(&narrow), m_private: false }
    }

    /// `equal(const StringImpl*, std::span<const CharacterType>)`.
    pub fn equal<T: CharType>(r: Option<UniquedKey>, characters: &[T]) -> bool {
        equal_span(r.as_ref().map(|key| &*key.0), Some(characters))
    }

    /// `from(VM&, double)`.
    pub fn from_double(_vm: &VM, value: f64) -> Identifier {
        Identifier { m_string: AtomString::number_f64(value), m_private: false }
    }

    /// `fromString(VM&, const String&)`: sempre átomo; a espécie símbolo é descartada.
    pub fn from_string(_vm: &VM, string: &WtfString) -> Identifier {
        Identifier { m_string: AtomString::from_string(string), m_private: false }
    }

    /// `fromString(VM&, AtomStringImpl*)` e `fromString(VM&, Ref<AtomStringImpl>&&)`.
    pub fn from_atom_impl(_vm: &VM, string: Option<Rc<StringImpl>>) -> Identifier {
        Identifier { m_string: AtomString::from_atom_impl(string), m_private: false }
    }

    /// `fromString(VM&, const AtomString&)`.
    pub fn from_atom_string(_vm: &VM, string: &AtomString) -> Identifier {
        Identifier { m_string: string.clone(), m_private: false }
    }

    /// `fromUid(VM&, UniquedStringImpl* uid)`. A marca de privado vive no `StringImpl` do símbolo
    /// (ver `StringImpl::is_private_symbol`), então ela sobrevive como no C++, onde o uid é o símbolo.
    pub fn from_uid(_vm: &VM, uid: Option<&UniquedKey>) -> Identifier {
        match uid {
            None => Identifier::default(),
            Some(uid) => {
                debug_assert!(uid.0.is_symbol() || uid.0.is_atom());
                Identifier { m_string: AtomString::from_uniqued(Some(Rc::clone(&uid.0))), m_private: uid.0.is_private_symbol() }
            }
        }
    }

    /// `fromUid(SymbolImpl&)` e `Identifier(SymbolImpl& uid)`.
    pub fn from_uid_symbol(symbol: &SymbolImpl) -> Identifier {
        Identifier {
            m_string: AtomString::from_uniqued(Some(Rc::clone(symbol.string_impl()))),
            m_private: symbol.is_private(),
        }
    }

    /// `fromUid(const PrivateName&)`.
    pub fn from_private_name(name: &PrivateName) -> Identifier {
        Identifier::from_uid_symbol(name.uid())
    }

    /// `from(VM&, unsigned)`.
    pub fn from_u32(_vm: &VM, value: u32) -> Identifier {
        Identifier { m_string: AtomString::number_u32(value), m_private: false }
    }

    /// `from(VM&, int)`.
    pub fn from_i32(_vm: &VM, value: i32) -> Identifier {
        Identifier { m_string: AtomString::number_i32(value), m_private: false }
    }

    /// `isNull()`.
    pub fn is_null(&self) -> bool {
        self.m_string.is_null()
    }

    /// `isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.m_string.is_empty()
    }

    /// `isSymbol()`.
    pub fn is_symbol(&self) -> bool {
        !self.is_null() && self.m_string.impl_().is_some_and(|string| string.is_symbol())
    }

    /// `isPrivateName()`.
    pub fn is_private_name(&self) -> bool {
        self.is_symbol() && self.m_private
    }

    /// `parseIndex(const Identifier&)`: o índice de array que o identificador nomeia, se nomear.
    pub fn as_index(&self) -> Option<u32> {
        parse_index_identifier(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index_of(text: &str) -> Option<u32> {
        let vm = VM::default();
        Identifier::from_span(&vm, text.as_bytes()).as_index()
    }

    #[test]
    fn parse_index_boundaries() {
        assert_eq!(index_of("0"), Some(0));
        assert_eq!(index_of("01"), None);
        assert_eq!(index_of("4294967294"), Some(4294967294));
        assert_eq!(index_of("4294967295"), None);
        assert_eq!(index_of("4294967296"), None);
        assert_eq!(index_of("99999999999"), None);
        assert_eq!(index_of(""), None);
        assert_eq!(index_of("1a"), None);
        assert_eq!(index_of("-1"), None);
    }

    #[test]
    fn parse_index_utf16_and_symbols() {
        let vm = VM::default();
        let wide: Vec<u16> = "123".encode_utf16().collect();
        assert_eq!(Identifier::from_span(&vm, &wide).as_index(), Some(123));
        assert_eq!(Identifier::null_identifier().as_index(), None);
        let name = PrivateName::with_description(&StringImpl::create(b"12"));
        let id = Identifier::from_private_name(&name);
        assert!(id.is_symbol());
        assert_eq!(id.as_index(), None);
        assert!(!id.is_private_name());
        let private = PrivateName::with_private_symbol(&StringImpl::create(b"p"));
        assert!(Identifier::from_private_name(&private).is_private_name());
    }

    #[test]
    fn null_empty_and_equality() {
        let vm = VM::default();
        assert!(Identifier::null_identifier().is_null());
        assert!(Identifier::empty_identifier().is_empty());
        assert!(!Identifier::empty_identifier().is_null());
        assert_eq!(Identifier::from_span(&vm, b""), Identifier::empty_identifier());
        let a = Identifier::from_span(&vm, b"abc");
        assert_eq!(a, Identifier::from_span(&vm, b"abc"));
        assert_eq!(a.length(), 3);
        assert_eq!(a.utf8(), b"abc".to_vec());
    }
}
