//! Tradução de `WTF/wtf/text/StringView.h` e `StringView.cpp`.
//!
//! Modelo: `StringView` é uma referência não dona a caracteres Latin1 ou UTF-16. A visão nula do
//! C++ (`m_characters == nullptr`) é `StringViewData::Null`; a visão vazia não nula é uma fatia de
//! comprimento 0. A `Null` conta como de 8 bits e de comprimento 0, como o C++.
//!
//! O que o `StringCommon.h` faz sobre fatias vive em `string_common`; aqui está o que opera sobre a
//! visão (`findCommon`, `startsWith`, `equalCommon`...). Os métodos de `String`/`AtomString` que o
//! `StringView.h` define inline (`String::find(StringView)` etc.) já existem em `wtf_string` e
//! `atom_string`, que chamam `StringImpl` com uma `StringView`.
//!
//! Construtores: `StringView(const String&)`, `(const AtomString&)`, `(const StringImpl&)`,
//! `(const StringImpl*)`, `(std::span<const Latin1Character>)` (que cobre `fromLatin1` e o
//! `ASCIILiteral`) e `(std::span<const char16_t>)` são as implementações de `From`.
//!
//! Funções que repassam a outra (`startsWith`, `findCommon`, `equalIgnoringNullity`...) existem uma
//! vez só: o método ou a função livre canônica, sem o repasse, conforme a regra DRY do projeto.
//!
//! Fora deste porte, e por quê:
//!
//! - `StringView(const void*, unsigned, bool)` e `rawCharacters()`: expõem ponteiro cru, e o porte
//!   é `forbid(unsafe_code)`. Use as fatias.
//! - `GraphemeClusters`: depende do `NonSharedCharacterBreakIterator` (`TextBreakIterator.h`) e do
//!   `ubrk_following` do ICU, que não existem no porte.
//! - `find(AdaptiveStringSearcherTables&, ...)`: depende do `AdaptiveStringSearcher.h`, ainda não
//!   portado.
//! - `normalizedNFC` e `StringViewWithUnderlyingString`: dependem do `unorm2` do ICU, e a estrutura
//!   guarda uma visão que aponta para a própria `String` (autorreferência, impossível em Rust seguro).
//! - `StringTypeAdapter<StringView>` (infraestrutura do `makeString`), `VectorTraits`, a variável
//!   `underlyingString` do `CHECK_STRINGVIEW_LIFETIME` (só com `ASSERT_ENABLED`; o empréstimo do Rust
//!   cobre o que ela verifica), `invalidate` (vazio sem aquela verificação) e `show()` (depuração).
//! - A verificação por ponteiro (`a.m_characters == b.m_characters`) de `equal`: é só atalho, o
//!   resultado é o mesmo.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::rc::Rc;

use crate::wtf::ascii_ctype::{is_ascii_upper, to_ascii_lower, to_ascii_upper};
use crate::wtf::text::atom_string::AtomString;
use crate::wtf::text::atom_string_impl::AtomStringImpl;
use crate::wtf::text::string_common::{
    equal_ignoring_ascii_case_with_length, equal_letters_ignoring_ascii_case_with_length, equal_prefix, find,
    find_ignoring_ascii_case_spans, find_inner, reverse_find, reverse_find_8_char16, reverse_find_inner, unit,
    NOT_FOUND,
};
use crate::wtf::text::conversion_mode::ConversionMode;
use crate::wtf::text::string_hasher;
use crate::wtf::text::string_impl::{
    self, copy_characters_convert, u16_is_single, CaseConvertType, CharType, StringImpl,
    UTF8ConversionError,
};
use crate::wtf::text::wtf_string::{
    characters_to_double, characters_to_float, is_well_formed_utf16, String as WtfString,
};
use crate::wtf::unicode::utf8_conversion::{u16_get_supplementary, u16_is_lead, u16_is_trail};

// ---------------------------------------------------------------------------------------------
// Despacho sobre a largura dos caracteres
// ---------------------------------------------------------------------------------------------

/// Executa `$body` com `$x` ligado à fatia tipada (`&[u8]` ou `&[u16]`) de uma `StringView`: o
/// equivalente dos `if (is8Bit()) ... else ...` do C++ sobre `span8()`/`span16()`. A visão nula
/// entra como a fatia Latin1 vazia.
macro_rules! with_view {
    ($view:expr, |$x:ident| $body:expr) => {
        match $crate::wtf::text::string_view::StringView::data(&$view) {
            $crate::wtf::text::string_view::StringViewData::Null => {
                let $x: &[u8] = &[];
                $body
            }
            $crate::wtf::text::string_view::StringViewData::Latin1($x) => $body,
            $crate::wtf::text::string_view::StringViewData::Utf16($x) => $body,
        }
    };
}
pub(crate) use with_view;

/// Como `with_view!`, para o par de fatias dos quatro casos (8/8, 8/16, 16/8, 16/16) do C++.
macro_rules! with_views {
    ($a:expr, $b:expr, |$x:ident, $y:ident| $body:expr) => {
        $crate::wtf::text::string_view::with_view!($a, |$x| {
            $crate::wtf::text::string_view::with_view!($b, |$y| $body)
        })
    };
}
pub(crate) use with_views;

// ---------------------------------------------------------------------------------------------
// class StringView
// ---------------------------------------------------------------------------------------------

/// Os caracteres de uma `StringView`. `Null` é o `m_characters == nullptr` do C++.
#[derive(Clone, Copy, Debug)]
pub enum StringViewData<'a> {
    Null,
    Latin1(&'a [u8]),
    Utf16(&'a [u16]),
}

/// `class StringView`: referência não dona a uma string.
#[derive(Clone, Copy, Debug)]
pub struct StringView<'a> {
    data: StringViewData<'a>,
}

impl Default for StringView<'_> {
    /// `StringView()`: a visão nula.
    fn default() -> Self {
        StringView { data: StringViewData::Null }
    }
}

impl<'a> From<&'a StringImpl> for StringView<'a> {
    /// `StringView(const StringImpl&)`.
    fn from(string: &'a StringImpl) -> StringView<'a> {
        if string.is_8bit() {
            StringView { data: StringViewData::Latin1(string.span8()) }
        } else {
            StringView { data: StringViewData::Utf16(string.span16()) }
        }
    }
}

impl<'a> From<&'a Rc<StringImpl>> for StringView<'a> {
    /// `StringView(const StringImpl&)` a partir do `Ref`/`RefPtr` do C++.
    fn from(string: &'a Rc<StringImpl>) -> StringView<'a> {
        StringView::from(&**string)
    }
}

impl<'a> From<Option<&'a StringImpl>> for StringView<'a> {
    /// `StringView(const StringImpl*)`: o ponteiro nulo dá a visão nula.
    fn from(string: Option<&'a StringImpl>) -> StringView<'a> {
        match string {
            None => StringView::default(),
            Some(string) => StringView::from(string),
        }
    }
}

impl<'a> From<&'a WtfString> for StringView<'a> {
    /// `StringView(const String&)`: a `String` nula dá a visão nula, a vazia dá a visão vazia.
    fn from(string: &'a WtfString) -> StringView<'a> {
        StringView::from(string.impl_().map(|string| &**string))
    }
}

impl<'a> From<&'a AtomString> for StringView<'a> {
    /// `StringView(const AtomString&)`.
    fn from(atom_string: &'a AtomString) -> StringView<'a> {
        StringView::from(atom_string.string())
    }
}

impl<'a> From<&'a [u8]> for StringView<'a> {
    /// `StringView(std::span<const Latin1Character>)`, `StringView::fromLatin1` e
    /// `StringView(ASCIILiteral)`.
    fn from(characters: &'a [u8]) -> StringView<'a> {
        StringView { data: StringViewData::Latin1(characters) }
    }
}

impl<'a> From<&'a [u16]> for StringView<'a> {
    /// `StringView(std::span<const char16_t>)`.
    fn from(characters: &'a [u16]) -> StringView<'a> {
        StringView { data: StringViewData::Utf16(characters) }
    }
}

impl PartialEq for StringView<'_> {
    /// `operator==(StringView, StringView)`: `equal(a, b)` (nulo e vazio são iguais).
    fn eq(&self, other: &Self) -> bool {
        equal(*self, *other)
    }
}

impl Eq for StringView<'_> {}

impl<'a> StringView<'a> {
    /// Os caracteres da visão, para o despacho por largura (`with_view!`).
    pub fn data(&self) -> StringViewData<'a> {
        self.data
    }

    /// `length()`.
    pub fn length(&self) -> u32 {
        match self.data {
            StringViewData::Null => 0,
            StringViewData::Latin1(characters) => characters.len() as u32,
            StringViewData::Utf16(characters) => characters.len() as u32,
        }
    }

    /// `isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.length() == 0
    }

    /// `isNull()` (e o `explicit operator bool`, que é o seu contrário).
    pub fn is_null(&self) -> bool {
        matches!(self.data, StringViewData::Null)
    }

    /// `is8Bit()`: a visão nula conta como de 8 bits.
    pub fn is_8bit(&self) -> bool {
        !matches!(self.data, StringViewData::Utf16(_))
    }

    /// `sizeInBytes()`.
    pub fn size_in_bytes(&self) -> u32 {
        self.length() * if self.is_8bit() { 1 } else { 2 }
    }

    /// `span8()`: o C++ exige `is8Bit()`.
    pub fn span8(&self) -> &'a [u8] {
        match self.data {
            StringViewData::Null => &[],
            StringViewData::Latin1(characters) => characters,
            StringViewData::Utf16(_) => panic!("StringView::span8: a visão é de 16 bits"),
        }
    }

    /// `span16()` (e `unsafeSpan16()`): o C++ exige `!is8Bit() || isEmpty()`.
    pub fn span16(&self) -> &'a [u16] {
        match self.data {
            StringViewData::Null => &[],
            StringViewData::Latin1(characters) if characters.is_empty() => &[],
            StringViewData::Latin1(_) => panic!("StringView::span16: a visão é de 8 bits e não vazia"),
            StringViewData::Utf16(characters) => characters,
        }
    }

    /// `span<CharacterType>()`.
    pub fn span<T: CharType>(&self) -> &'a [T] {
        T::view_span(*self)
    }

    /// `hash()`.
    pub fn hash(&self) -> u32 {
        if self.is_8bit() {
            return string_hasher::compute_hash_and_mask_top8_bits::<u8>(self.span8());
        }
        string_hasher::compute_hash_and_mask_top8_bits::<u16>(self.span16())
    }

    /// `containsOnlyASCII()`.
    pub fn contains_only_ascii(&self) -> bool {
        with_view!(self, |characters| crate::wtf::text::string_common::characters_are_all_ascii(characters))
    }

    // ---- acesso aos caracteres -----------------------------------------------------------

    /// `codeUnitAt(index)` e `operator[]`.
    pub fn code_unit_at(&self, index: u32) -> u16 {
        if self.is_8bit() {
            return self.span8()[index as usize] as u16;
        }
        self.span16()[index as usize]
    }

    /// `codePointAt(index)`.
    pub fn code_point_at(&self, index: u32) -> u32 {
        debug_assert!(index < self.length());
        if self.is_8bit() {
            return self.span8()[index as usize] as u32;
        }
        let characters = self.span16();
        let i = index as usize;
        if u16_is_single(characters[i]) {
            return characters[i] as u32;
        }
        if i + 1 < self.length() as usize && u16_is_lead(characters[i] as u32) && u16_is_trail(characters[i + 1] as u32)
        {
            return u16_get_supplementary(characters[i] as u32, characters[i + 1] as u32);
        }
        characters[i] as u32
    }

    /// `codePointBefore(index)`: o `U16_PREV(*this, 0, offset, codePoint)` do ICU.
    pub fn code_point_before(&self, index: u32) -> u32 {
        debug_assert!(index > 0 && index <= self.length());
        let position = index - 1;
        let mut code_point = self.code_unit_at(position) as u32;
        if u16_is_trail(code_point) && position > 0 {
            let lead = self.code_unit_at(position - 1) as u32;
            if u16_is_lead(lead) {
                code_point = u16_get_supplementary(lead, code_point);
            }
        }
        code_point
    }

    /// `codeUnits()`.
    pub fn code_units(&self) -> CodeUnits<'a> {
        CodeUnits { string_view: *self }
    }

    /// `codePoints()`.
    pub fn code_points(&self) -> CodePoints<'a> {
        CodePoints { string_view: *self }
    }

    // ---- conversões ----------------------------------------------------------------------

    /// `toString()`: o `String(std::span)` do C++ dá a `String` nula para dados nulos.
    pub fn to_string(&self) -> WtfString {
        match self.data {
            StringViewData::Null => WtfString::default(),
            StringViewData::Latin1(characters) => WtfString::from_latin1(characters),
            StringViewData::Utf16(characters) => WtfString::from_utf16(characters),
        }
    }

    /// `toStringWithoutCopying()`: `StringImpl::createWithoutCopying`, que dá a string vazia para
    /// fatia vazia (nula inclusive). O porte sempre é dono do buffer, então copia.
    pub fn to_string_without_copying(&self) -> WtfString {
        with_view!(self, |characters| {
            if characters.is_empty() {
                WtfString::from(StringImpl::empty())
            } else {
                WtfString::from(StringImpl::create_without_copying_non_empty(characters))
            }
        })
    }

    /// `toAtomString()`: o `AtomString(std::span)` do C++ dá o átomo nulo para dados nulos.
    pub fn to_atom_string(&self) -> AtomString {
        match self.data {
            StringViewData::Null => AtomString::new(),
            StringViewData::Latin1(characters) => AtomString::from_latin1(characters),
            StringViewData::Utf16(characters) => AtomString::from_utf16(characters),
        }
    }

    /// `toExistingAtomString()`: o átomo só se já existir na tabela, senão o nulo.
    pub fn to_existing_atom_string(&self) -> AtomString {
        if self.is_8bit() {
            return AtomString::from_atom_impl(AtomStringImpl::look_up(self.span8()));
        }
        AtomString::from_atom_impl(AtomStringImpl::look_up16(self.span16()))
    }

    /// `toFloat(isValid)`: o valor e o `isValid`.
    pub fn to_float(&self) -> (f32, bool) {
        if self.is_8bit() {
            return characters_to_float(self.span8());
        }
        characters_to_float(self.span16())
    }

    /// `toDouble(isValid)`: o valor e o `isValid`.
    pub fn to_double(&self) -> (f64, bool) {
        if self.is_8bit() {
            return characters_to_double(self.span8());
        }
        characters_to_double(self.span16())
    }

    /// `tryGetUTF8(mode)`: os bytes UTF-8 (o `CString` do C++ sem o NUL final). A visão nula dá
    /// o vazio.
    pub fn try_get_utf8(&self, mode: ConversionMode) -> Result<Vec<u8>, UTF8ConversionError> {
        match self.data {
            StringViewData::Null => Ok(Vec::new()),
            StringViewData::Latin1(characters) => StringImpl::utf8_for_characters8(characters),
            StringViewData::Utf16(characters) => StringImpl::utf8_for_characters16(characters, mode),
        }
    }

    /// `utf8(mode)`: o C++ faz `RELEASE_ASSERT` no resultado de `tryGetUTF8`.
    pub fn utf8(&self, mode: ConversionMode) -> Vec<u8> {
        match self.try_get_utf8(mode) {
            Ok(string) => string,
            Err(_) => panic!("StringView::utf8: conversão falhou"),
        }
    }

    /// `tryGetUTF8(function, mode)` (o template do cabeçalho): chama `function` com os bytes.
    pub fn try_get_utf8_with<R>(
        &self,
        function: impl FnOnce(&[u8]) -> R,
        mode: ConversionMode,
    ) -> Result<R, UTF8ConversionError> {
        if self.is_8bit() {
            return StringImpl::try_get_utf8_for_characters8(function, self.span8());
        }
        StringImpl::try_get_utf8_for_characters16(function, self.span16(), mode)
    }

    /// `upconvertedCharacters()` (`UpconvertedCharactersWithSize<N>`): o texto como UTF-16, sem
    /// copiar quando já é de 16 bits. O `get()`/`span()` do C++ é o `Deref` do `Cow`.
    pub fn upconverted_characters(&self) -> Cow<'a, [u16]> {
        if !self.is_8bit() {
            return Cow::Borrowed(self.span16());
        }
        let source = self.span8();
        let mut upconverted: Vec<u16> = vec![0; source.len()];
        copy_characters_convert(&mut upconverted, source);
        Cow::Owned(upconverted)
    }

    /// `getCharacters<CharacterType>(destination)`.
    pub fn get_characters<D: CharType>(&self, destination: &mut [D]) {
        if self.is_8bit() {
            self.get_characters8(destination);
        } else {
            self.get_characters16(destination);
        }
    }

    /// `getCharacters8<CharacterType>(destination)`.
    pub fn get_characters8<D: CharType>(&self, destination: &mut [D]) {
        copy_characters_convert(destination, self.span8());
    }

    /// `getCharacters16<CharacterType>(destination)`.
    pub fn get_characters16<D: CharType>(&self, destination: &mut [D]) {
        copy_characters_convert(destination, self.span16());
    }

    /// `getCharactersWithASCIICase(type, destination)`, as duas sobrecargas (`Latin1Character` e
    /// `char16_t`). O C++ exige que o destino seja ao menos tão largo quanto a origem
    /// (`static_assert`), e na versão de 8 bits, `is8Bit()`.
    pub fn get_characters_with_ascii_case<D: CharType>(&self, case_convert_type: CaseConvertType, destination: &mut [D]) {
        match self.data {
            StringViewData::Null => {}
            StringViewData::Latin1(source) => get_characters_with_ascii_case_internal(case_convert_type, destination, source),
            StringViewData::Utf16(source) => {
                assert!(D::SIZE >= 2, "StringView::getCharactersWithASCIICase: destino mais estreito que a origem");
                get_characters_with_ascii_case_internal(case_convert_type, destination, source);
            }
        }
    }

    // ---- substrings ----------------------------------------------------------------------

    /// `substring(start, length)`; o `length` padrão do C++ é `std::numeric_limits<unsigned>::max()`.
    pub fn substring(&self, start: u32, length: u32) -> StringView<'a> {
        if start >= self.length() {
            return empty_string_view();
        }
        let max_length = self.length() - start;

        let mut length = length;
        if length >= max_length {
            if start == 0 {
                return *self;
            }
            length = max_length;
        }

        let (start, length) = (start as usize, length as usize);
        if self.is_8bit() {
            return StringView::from(&self.span8()[start..start + length]);
        }
        StringView::from(&self.span16()[start..start + length])
    }

    /// `left(length)`.
    pub fn left(&self, length: u32) -> StringView<'a> {
        self.substring(0, length)
    }

    /// `right(length)`.
    pub fn right(&self, length: u32) -> StringView<'a> {
        self.substring(self.length().wrapping_sub(length), length)
    }

    /// `trim(CodeUnitMatchFunction)`.
    pub fn trim(&self, predicate: impl Fn(u16) -> bool) -> StringView<'a> {
        if self.is_8bit() {
            return self.trim_characters(self.span8(), &predicate);
        }
        self.trim_characters(self.span16(), &predicate)
    }

    /// O `trim<CharacterType>(span, predicate)` privado do C++.
    fn trim_characters<T: CharType, P: Fn(u16) -> bool>(&self, characters: &'a [T], predicate: &P) -> StringView<'a> {
        let length = self.length();
        if length == 0 {
            return *self;
        }

        let mut start: u32 = 0;
        let mut end: u32 = length - 1;

        while start <= end && predicate(characters[start as usize].to_u16()) {
            start += 1;
        }

        if start > end {
            return empty_string_view();
        }

        while end != 0 && predicate(characters[end as usize].to_u16()) {
            end -= 1;
        }

        if start == 0 && end == length - 1 {
            return *self;
        }

        T::make_view(&characters[start as usize..end as usize + 1])
    }

    /// `split(separator)`: sem as entradas vazias.
    pub fn split(&self, separator: u16) -> SplitResult<'a> {
        SplitResult { string: *self, separator, allow_empty_entries: false }
    }

    /// `splitAllowingEmptyEntries(separator)`.
    pub fn split_allowing_empty_entries(&self, separator: u16) -> SplitResult<'a> {
        SplitResult { string: *self, separator, allow_empty_entries: true }
    }

    // ---- busca ---------------------------------------------------------------------------

    /// `find(char16_t / Latin1Character / char, start)`; o `start` padrão do C++ é 0.
    pub fn find_character(&self, character: u16, start: u32) -> usize {
        with_view!(self, |characters| find(characters, |c| c == character, start as usize))
    }

    /// `find(CodeUnitMatchFunction, start)`; o `start` padrão do C++ é 0.
    pub fn find_matching(&self, match_function: impl Fn(u16) -> bool, start: u32) -> usize {
        with_view!(self, |characters| find(characters, &match_function, start as usize))
    }

    /// `find(StringView, start)` (e o `findCommon` do cabeçalho); o `start` padrão do C++ é 0.
    pub fn find(&self, match_string: StringView, start: u32) -> usize {
        let start = start as usize;
        let needle_length = match_string.length();

        if needle_length == 1 {
            let first_character = match_string.code_unit_at(0);
            return self.find_character(first_character, start as u32);
        }

        if start > self.length() as usize {
            return NOT_FOUND;
        }

        if needle_length == 0 {
            return start;
        }

        let search_length = self.length() as usize - start;
        if needle_length as usize > search_length {
            return NOT_FOUND;
        }

        with_views!(*self, match_string, |haystack, needle| find_inner(&haystack[start..], needle, start))
    }

    /// O `find(std::span<const Latin1Character> match, unsigned start)` privado do C++ (o `find`
    /// que recebe um `ASCIILiteral`). `match_characters` não pode ser vazio.
    pub fn find_latin1(&self, match_characters: &[u8], start: u32) -> usize {
        debug_assert!(!match_characters.is_empty());
        let length = self.length();
        if start > length {
            return NOT_FOUND;
        }

        let search_length = length - start;
        if match_characters.len() > search_length as usize {
            return NOT_FOUND;
        }

        with_view!(self, |characters| find_inner(&characters[start as usize..], match_characters, start as usize))
    }

    /// `reverseFind(char16_t, index)`; o `index` padrão do C++ é `std::numeric_limits<unsigned>::max()`.
    pub fn reverse_find_character(&self, character: u16, index: u32) -> usize {
        match self.data {
            StringViewData::Null => NOT_FOUND,
            StringViewData::Latin1(characters) => reverse_find_8_char16(characters, character, index as usize),
            StringViewData::Utf16(characters) => reverse_find(characters, character, index as usize),
        }
    }

    /// `reverseFind(StringView, start)`; o `start` padrão do C++ é `std::numeric_limits<unsigned>::max()`.
    pub fn reverse_find(&self, match_string: StringView, start: u32) -> usize {
        if self.is_null() || match_string.is_null() {
            return NOT_FOUND;
        }

        if match_string.is_empty() {
            return std::cmp::min(start, self.length()) as usize;
        }

        // Check start & matchLength are in range.
        if match_string.length() > self.length() {
            return NOT_FOUND;
        }

        with_views!(*self, match_string, |haystack, needle| reverse_find_inner(haystack, needle, start as usize))
    }

    /// O `reverseFind(std::span<const Latin1Character> match, unsigned start)` privado do C++ (o
    /// `reverseFind` que recebe um `ASCIILiteral`). `match_characters` não pode ser vazio.
    pub fn reverse_find_latin1(&self, match_characters: &[u8], start: u32) -> usize {
        debug_assert!(!match_characters.is_empty());
        if match_characters.len() > self.length() as usize {
            return NOT_FOUND;
        }

        with_view!(self, |characters| reverse_find_inner(characters, match_characters, start as usize))
    }

    /// `findIgnoringASCIICase(StringView, start)` (e o `findIgnoringASCIICase` livre do cabeçalho);
    /// sem o `start`, o C++ passa 0.
    pub fn find_ignoring_ascii_case(&self, string_to_find: StringView, start: u32) -> usize {
        let start = start as usize;
        let source_string_length = self.length() as usize;
        let match_length = string_to_find.length() as usize;
        if match_length == 0 {
            return std::cmp::min(start, source_string_length);
        }

        // Check start & matchLength are in range.
        if start > source_string_length {
            return NOT_FOUND;
        }
        let search_length = source_string_length - start;
        if match_length > search_length {
            return NOT_FOUND;
        }

        with_views!(*self, string_to_find, |source, to_find| find_ignoring_ascii_case_spans(source, to_find, start))
    }

    /// `contains(char16_t)`.
    pub fn contains_character(&self, character: u16) -> bool {
        self.find_character(character, 0) != NOT_FOUND
    }

    /// `contains(CodeUnitMatchFunction)`.
    pub fn contains_matching(&self, match_function: impl Fn(u16) -> bool) -> bool {
        self.find_matching(match_function, 0) != NOT_FOUND
    }

    /// `contains(StringView)` (e `contains(ASCIILiteral)`).
    pub fn contains(&self, string: StringView) -> bool {
        self.find(string, 0) != NOT_FOUND
    }

    /// `containsIgnoringASCIICase(StringView)`.
    pub fn contains_ignoring_ascii_case(&self, match_string: StringView) -> bool {
        self.find_ignoring_ascii_case(match_string, 0) != NOT_FOUND
    }

    /// `containsIgnoringASCIICase(StringView, start)`.
    pub fn contains_ignoring_ascii_case_from(&self, match_string: StringView, start_offset: u32) -> bool {
        self.find_ignoring_ascii_case(match_string, start_offset) != NOT_FOUND
    }

    /// `containsOnly<isSpecialCharacter>()`.
    pub fn contains_only(&self, is_special_character: fn(u16) -> bool) -> bool {
        with_view!(self, |characters| string_impl::contains_only(characters, is_special_character))
    }

    /// `startsWith(char16_t)`.
    pub fn starts_with_character(&self, character: u16) -> bool {
        self.length() != 0 && self.code_unit_at(0) == character
    }

    /// `startsWith(StringView)` (e o `startsWith` livre do cabeçalho).
    pub fn starts_with(&self, prefix: StringView) -> bool {
        if prefix.length() > self.length() {
            return false;
        }
        with_views!(*self, prefix, |reference, prefix_characters| equal_prefix(reference, prefix_characters))
    }

    /// `startsWithIgnoringASCIICase(StringView)` (e o `startsWithIgnoringASCIICase` livre).
    pub fn starts_with_ignoring_ascii_case(&self, prefix: StringView) -> bool {
        if prefix.length() > self.length() {
            return false;
        }
        with_views!(*self, prefix, |reference, prefix_characters| {
            equal_ignoring_ascii_case_with_length(reference, prefix_characters, prefix.length() as usize)
        })
    }

    /// `hasInfixStartingAt(prefix, start)`.
    pub fn has_infix_starting_at(&self, prefix: StringView, start: u32) -> bool {
        if start > self.length() {
            return false;
        }
        self.substring(start, u32::MAX).starts_with(prefix)
    }

    /// `endsWith(char16_t)`.
    pub fn ends_with_character(&self, character: u16) -> bool {
        self.length() != 0 && self.code_unit_at(self.length() - 1) == character
    }

    /// `endsWith(StringView)` (e o `endsWith` livre do cabeçalho).
    pub fn ends_with(&self, suffix: StringView) -> bool {
        let suffix_length = suffix.length();
        let reference_length = self.length();
        if suffix_length > reference_length {
            return false;
        }

        let start_offset = (reference_length - suffix_length) as usize;
        with_views!(*self, suffix, |reference, suffix_characters| {
            equal_prefix(&reference[start_offset..], suffix_characters)
        })
    }

    /// `endsWithIgnoringASCIICase(StringView)` (e o livre do cabeçalho).
    pub fn ends_with_ignoring_ascii_case(&self, suffix: StringView) -> bool {
        let suffix_length = suffix.length();
        let reference_length = self.length();
        if suffix_length > reference_length {
            return false;
        }

        let start_offset = (reference_length - suffix_length) as usize;
        with_views!(*self, suffix, |reference, suffix_characters| {
            equal_ignoring_ascii_case_with_length(&reference[start_offset..], suffix_characters, suffix_length as usize)
        })
    }

    /// `hasInfixEndingAt(suffix, end)`.
    pub fn has_infix_ending_at(&self, suffix: StringView, end: u32) -> bool {
        if end < suffix.length() {
            return false;
        }
        let start = end - suffix.length();
        self.has_infix_starting_at(suffix, start)
    }

    // ---- caixa ASCII ---------------------------------------------------------------------

    /// `convertToASCIILowercase()`.
    pub fn convert_to_ascii_lowercase(&self) -> WtfString {
        match self.data {
            StringViewData::Null => WtfString::default(),
            StringViewData::Latin1(input) => convert_ascii_case(CaseConvertType::Lower, input),
            StringViewData::Utf16(input) => convert_ascii_case(CaseConvertType::Lower, input),
        }
    }

    /// `convertToASCIIUppercase()`.
    pub fn convert_to_ascii_uppercase(&self) -> WtfString {
        match self.data {
            StringViewData::Null => WtfString::default(),
            StringViewData::Latin1(input) => convert_ascii_case(CaseConvertType::Upper, input),
            StringViewData::Utf16(input) => convert_ascii_case(CaseConvertType::Upper, input),
        }
    }

    /// `convertToASCIILowercaseAtom()`.
    pub fn convert_to_ascii_lowercase_atom(&self) -> AtomString {
        match self.data {
            StringViewData::Null => AtomString::new(),
            StringViewData::Latin1(input) => {
                if has_ascii_upper(input) {
                    return AtomString::from_string_owned(convert_ascii_case(CaseConvertType::Lower, input));
                }
                // Fast path when the StringView is already all lowercase.
                AtomString::from_latin1(input)
            }
            StringViewData::Utf16(input) => {
                if has_ascii_upper(input) {
                    return AtomString::from_string_owned(convert_ascii_case(CaseConvertType::Lower, input));
                }
                // Fast path when the StringView is already all lowercase.
                AtomString::from_utf16(input)
            }
        }
    }

    /// `convertToSingleCodePoint()`: o ponto de código único, ou `None` se a visão é vazia ou tem
    /// mais de um.
    pub fn convert_to_single_code_point(&self) -> Option<u32> {
        let mut iterator = self.code_points().into_iter();
        let character = iterator.next()?;
        match iterator.next() {
            None => Some(character),
            Some(_) => None,
        }
    }
}

/// `convertASCIICase<type, CharacterType>(span)`: a entrada não é nula (o chamador trata a nula).
fn convert_ascii_case<T: CharType>(case_convert_type: CaseConvertType, input: &[T]) -> WtfString {
    WtfString::create_uninitialized::<T>(input.len(), |characters| {
        for (destination, character) in characters.iter_mut().zip(input) {
            let converted = match case_convert_type {
                CaseConvertType::Lower => to_ascii_lower(character.to_u16()),
                CaseConvertType::Upper => to_ascii_upper(character.to_u16()),
            };
            *destination = T::from_u16(converted);
        }
    })
}

/// O laço de `convertASCIILowercaseAtom<CharacterType>(span)`: há alguma maiúscula ASCII?
fn has_ascii_upper<T: CharType>(input: &[T]) -> bool {
    input.iter().any(|character| is_ascii_upper(character.to_u16()))
}

/// `getCharactersWithASCIICaseInternal`: o `zippedRange(destination, source)` pára no menor.
fn get_characters_with_ascii_case_internal<D: CharType, S: CharType>(
    case_convert_type: CaseConvertType,
    destination: &mut [D],
    source: &[S],
) {
    for (destination_character, character) in destination.iter_mut().zip(source) {
        let converted = match case_convert_type {
            CaseConvertType::Lower => to_ascii_lower(character.to_u16()),
            CaseConvertType::Upper => to_ascii_upper(character.to_u16()),
        };
        *destination_character = D::from_u16(converted);
    }
}

// ---------------------------------------------------------------------------------------------
// StringView::CodeUnits e StringView::CodePoints
// ---------------------------------------------------------------------------------------------

/// `StringView::CodeUnits`.
#[derive(Clone, Copy, Debug)]
pub struct CodeUnits<'a> {
    string_view: StringView<'a>,
}

impl<'a> IntoIterator for CodeUnits<'a> {
    type Item = u16;
    type IntoIter = CodeUnitsIterator<'a>;

    /// `begin()` até `end()`.
    fn into_iter(self) -> CodeUnitsIterator<'a> {
        CodeUnitsIterator { string_view: self.string_view, index: 0, end: self.string_view.length() }
    }
}

/// `StringView::CodeUnits::Iterator`.
#[derive(Clone, Debug)]
pub struct CodeUnitsIterator<'a> {
    string_view: StringView<'a>,
    index: u32,
    end: u32,
}

impl Iterator for CodeUnitsIterator<'_> {
    type Item = u16;

    fn next(&mut self) -> Option<u16> {
        if self.index == self.end {
            return None;
        }
        let code_unit = self.string_view.code_unit_at(self.index);
        self.index += 1;
        Some(code_unit)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = (self.end - self.index) as usize;
        (remaining, Some(remaining))
    }
}

impl DoubleEndedIterator for CodeUnitsIterator<'_> {
    fn next_back(&mut self) -> Option<u16> {
        if self.index == self.end {
            return None;
        }
        self.end -= 1;
        Some(self.string_view.code_unit_at(self.end))
    }
}

/// `StringView::CodePoints`.
#[derive(Clone, Copy, Debug)]
pub struct CodePoints<'a> {
    string_view: StringView<'a>,
}

impl<'a> CodePoints<'a> {
    /// `codePointAt(index)`: o iterador posicionado em `index`, que segue até o fim.
    pub fn code_point_at(&self, index: u32) -> CodePointsIterator<'a> {
        CodePointsIterator::new(self.string_view, index)
    }
}

impl<'a> IntoIterator for CodePoints<'a> {
    type Item = u32;
    type IntoIter = CodePointsIterator<'a>;

    /// `begin()` até `end()`.
    fn into_iter(self) -> CodePointsIterator<'a> {
        CodePointsIterator::new(self.string_view, 0)
    }
}

/// `U16_FWD_1(s, i, length)`: o índice depois do ponto de código que começa em `i`.
fn u16_fwd_1(characters: &[u16], i: usize) -> usize {
    let is_lead = u16_is_lead(characters[i] as u32);
    let next = i + 1;
    if is_lead && next != characters.len() && u16_is_trail(characters[next] as u32) {
        return next + 1;
    }
    next
}

/// `U16_BACK_1(s, 0, i)`: o índice do início do ponto de código que termina antes de `i`.
fn u16_back_1(characters: &[u16], i: usize) -> usize {
    let previous = i - 1;
    if u16_is_trail(characters[previous] as u32) && previous > 0 && u16_is_lead(characters[previous - 1] as u32) {
        return previous - 1;
    }
    previous
}

/// `U16_GET(s, 0, 0, length, c)` na posição `i`: um par substituto válido a partir de `i` forma
/// um ponto de código só.
fn u16_get(characters: &[u16], i: usize) -> u32 {
    let code_unit = characters[i] as u32;
    if u16_is_lead(code_unit) && i + 1 != characters.len() && u16_is_trail(characters[i + 1] as u32) {
        return u16_get_supplementary(code_unit, characters[i + 1] as u32);
    }
    code_unit
}

/// `StringView::CodePoints::Iterator`, bidirecional como o C++ (`operator++` e `operator--`).
#[derive(Clone, Debug)]
pub struct CodePointsIterator<'a> {
    string_view: StringView<'a>,
    current: usize,
    back: usize,
}

impl<'a> CodePointsIterator<'a> {
    /// `Iterator(StringView, unsigned index)`: o `characters.subspan(index)` não passa do fim.
    pub fn new(string_view: StringView<'a>, index: u32) -> CodePointsIterator<'a> {
        assert!(index <= string_view.length(), "StringView::CodePoints::Iterator: índice além do fim");
        CodePointsIterator { string_view, current: index as usize, back: string_view.length() as usize }
    }
}

impl Iterator for CodePointsIterator<'_> {
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        if self.current >= self.back {
            return None;
        }
        match self.string_view.data() {
            StringViewData::Utf16(characters) => {
                let code_point = u16_get(characters, self.current);
                self.current = u16_fwd_1(characters, self.current);
                Some(code_point)
            }
            _ => {
                let code_point = self.string_view.span8()[self.current] as u32;
                self.current += 1;
                Some(code_point)
            }
        }
    }
}

impl DoubleEndedIterator for CodePointsIterator<'_> {
    fn next_back(&mut self) -> Option<u32> {
        if self.current >= self.back {
            return None;
        }
        match self.string_view.data() {
            StringViewData::Utf16(characters) => {
                self.back = u16_back_1(characters, self.back);
                Some(u16_get(characters, self.back))
            }
            _ => {
                self.back -= 1;
                Some(self.string_view.span8()[self.back] as u32)
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// StringView::SplitResult
// ---------------------------------------------------------------------------------------------

/// `StringView::SplitResult`.
#[derive(Clone, Copy, Debug)]
pub struct SplitResult<'a> {
    string: StringView<'a>,
    separator: u16,
    allow_empty_entries: bool,
}

impl<'a> IntoIterator for SplitResult<'a> {
    type Item = StringView<'a>;
    type IntoIter = SplitResultIterator<'a>;

    /// `begin()` até `end()`.
    fn into_iter(self) -> SplitResultIterator<'a> {
        // Iterator(const SplitResult&)
        let mut iterator = SplitResultIterator {
            result: self,
            position: 0,
            length: 0,
            is_done: self.string.is_empty() && !self.allow_empty_entries,
        };
        iterator.find_next_substring();
        iterator
    }
}

/// `StringView::SplitResult::Iterator`.
#[derive(Clone, Debug)]
pub struct SplitResultIterator<'a> {
    result: SplitResult<'a>,
    position: u32,
    length: u32,
    is_done: bool,
}

impl SplitResultIterator<'_> {
    /// `findNextSubstring()`.
    fn find_next_substring(&mut self) {
        loop {
            let separator_position = self.result.string.find_character(self.result.separator, self.position);
            if separator_position == NOT_FOUND {
                break;
            }
            if self.result.allow_empty_entries || separator_position > self.position as usize {
                self.length = (separator_position - self.position as usize) as u32;
                return;
            }
            self.position += 1;
        }
        self.length = self.result.string.length() - self.position;
        if self.length == 0 && !self.result.allow_empty_entries {
            self.is_done = true;
        }
    }
}

impl<'a> Iterator for SplitResultIterator<'a> {
    type Item = StringView<'a>;

    /// `operator*` seguido de `operator++`.
    fn next(&mut self) -> Option<StringView<'a>> {
        if self.is_done {
            return None;
        }
        debug_assert!(self.position <= self.result.string.length());
        let entry = self.result.string.substring(self.position, self.length);

        self.position += self.length;
        if self.position < self.result.string.length() {
            self.position += 1;
            self.find_next_substring();
        } else {
            self.is_done = true;
        }
        Some(entry)
    }
}

// ---------------------------------------------------------------------------------------------
// Funções livres
// ---------------------------------------------------------------------------------------------

/// `append(Vector<CharacterType, inlineCapacity>&, StringView)`.
pub fn append<T: CharType>(buffer: &mut Vec<T>, string: StringView) {
    let old_size = buffer.len();
    buffer.resize(old_size + string.length() as usize, T::from_u16(0));
    string.get_characters(&mut buffer[old_size..]);
}

/// `equal(StringView, StringView, unsigned length)`: o `equalCommon(a, b, length)`, que só olha o
/// `length` para o caso zero e compara o primeiro caractere e depois todo o resto de `b`.
pub fn equal_with_length(a: StringView, b: StringView, length: u32) -> bool {
    if length == 0 {
        return true;
    }

    with_views!(a, b, |a_characters, b_characters| {
        if a_characters.is_empty() || b_characters.is_empty() {
            // O C++ lê `front()` de fatia vazia (comportamento indefinido): sem o que comparar.
            a_characters.len() == b_characters.len()
        } else {
            unit(a_characters[0]) == unit(b_characters[0]) && equal_prefix(&a_characters[1..], &b_characters[1..])
        }
    })
}

/// `equal(StringView, StringView)`: o `equalCommon(a, b)` (nulo e vazio são iguais).
pub fn equal(a: StringView, b: StringView) -> bool {
    let length = a.length();
    if length != b.length() {
        return false;
    }

    equal_with_length(a, b, length)
}

/// `equal(StringView, std::span<const Latin1Character>)` e `equal(StringView, ASCIILiteral)`.
/// `None` é a fatia de dados nulos. Tradução literal do C++, inclusive o `false` da visão vazia
/// contra fatia não nula e o `!a.isEmpty()` contra a nula, que é o que o original faz.
pub fn equal_latin1(a: StringView, b: Option<&[u8]>) -> bool {
    let b = match b {
        None => return !a.is_empty(),
        Some(b) => b,
    };
    if a.is_empty() {
        return false;
    }

    if a.length() as usize != b.len() {
        return false;
    }

    with_view!(a, |characters| equal_prefix(characters, b))
}

/// `equalIgnoringASCIICase(StringView, StringView)`: o `equalIgnoringASCIICaseCommon`.
pub fn equal_ignoring_ascii_case(a: StringView, b: StringView) -> bool {
    if a.length() != b.length() {
        return false;
    }

    with_views!(a, b, |a_characters, b_characters| {
        equal_ignoring_ascii_case_with_length(a_characters, b_characters, b.length() as usize)
    })
}

/// `equalIgnoringASCIICase(StringView, ASCIILiteral)`: o `equalIgnoringASCIICaseCommon(a, const char*)`.
pub fn equal_ignoring_ascii_case_latin1(a: StringView, b: &[u8]) -> bool {
    if a.length() as usize != b.len() {
        return false;
    }

    with_view!(a, |characters| equal_ignoring_ascii_case_with_length(characters, b, b.len()))
}

/// `equalRespectingNullity(StringView, StringView)`.
pub fn equal_respecting_nullity(a: StringView, b: StringView) -> bool {
    if a.is_empty() && b.is_empty() {
        return a.is_null() == b.is_null();
    }

    equal(a, b)
}

/// `equalIgnoringNullity(StringView, StringView)`: o `equal`, que ignora a nulidade.
pub use self::equal as equal_ignoring_nullity;

/// `equalLettersIgnoringASCIICase(StringView, ASCIILiteral)`: o `equalLettersIgnoringASCIICaseCommon`.
/// `lowercase_letters` já em minúsculas (letra, dígito ou pontuação de 0x21 a 0x3F).
pub fn equal_letters_ignoring_ascii_case(string: StringView, lowercase_letters: &[u8]) -> bool {
    if string.length() as usize != lowercase_letters.len() {
        return false;
    }
    with_view!(string, |characters| {
        equal_letters_ignoring_ascii_case_with_length(characters, lowercase_letters, lowercase_letters.len())
    })
}

/// `startsWithLettersIgnoringASCIICase(StringView, ASCIILiteral)`: o
/// `startsWithLettersIgnoringASCIICaseCommon`.
pub fn starts_with_letters_ignoring_ascii_case(string: StringView, lowercase_letters: &[u8]) -> bool {
    if lowercase_letters.is_empty() {
        return true;
    }
    if (string.length() as usize) < lowercase_letters.len() {
        return false;
    }
    with_view!(string, |characters| {
        equal_letters_ignoring_ascii_case_with_length(characters, lowercase_letters, lowercase_letters.len())
    })
}

/// `emptyStringView()`: a visão vazia não nula (`""_span`).
pub fn empty_string_view<'a>() -> StringView<'a> {
    StringView::from(&[] as &[u8])
}

/// `codePointCompare(StringView, StringView)`.
pub fn code_point_compare(lhs: StringView, rhs: StringView) -> Ordering {
    with_views!(lhs, rhs, |lhs_characters, rhs_characters| {
        string_impl::code_point_compare(lhs_characters, rhs_characters)
    })
}

/// `codePointCompareLessThan(StringView, StringView)`.
pub fn code_point_compare_less_than(a: StringView, b: StringView) -> bool {
    code_point_compare(a, b) == Ordering::Less
}

/// `hasUnpairedSurrogate(StringView)`: as visões de 8 bits não têm substitutos.
pub fn has_unpaired_surrogate(string: StringView) -> bool {
    if string.is_8bit() {
        return false;
    }
    !is_well_formed_utf16(string.span16())
}

/// `makeStringByReplacingAll(const String&, StringView target, StringView replacement)`.
pub fn make_string_by_replacing_all(string: &WtfString, target: StringView, replacement: StringView) -> WtfString {
    match string.impl_() {
        Some(rc) => WtfString::from(rc.replace_view(target, replacement)),
        None => string.clone(),
    }
}

/// `makeStringByReplacing(const String&, unsigned start, unsigned length, StringView replacement)`.
pub fn make_string_by_replacing(string: &WtfString, start: u32, length: u32, replacement: StringView) -> WtfString {
    match string.impl_() {
        Some(rc) => WtfString::from(rc.replace_range(start as usize, length as usize, replacement)),
        None => string.clone(),
    }
}

/// `makeStringByReplacingAll(const String&, char16_t target, StringView replacement)`.
pub fn make_string_by_replacing_all_character(string: &WtfString, target: u16, replacement: StringView) -> WtfString {
    match string.impl_() {
        Some(rc) => WtfString::from(rc.replace_character_with_view(target, replacement)),
        None => string.clone(),
    }
}

/// `makeStringByReplacingAll(StringView, char16_t target, char16_t replacement)`.
pub fn make_string_by_replacing_all_characters(string: StringView, target: u16, replacement: u16) -> WtfString {
    // find() is SIMD-accelerated, and its Latin1 overload returns notFound for a
    // non-Latin1 target, so an 8-bit string with a 16-bit target is handled here too.
    let index = string.find_character(target, 0);
    if index == NOT_FOUND {
        return string.to_string();
    }
    if string.is_8bit() {
        return WtfString::from(StringImpl::create_by_replacing_in_characters8(
            string.span8(),
            target,
            replacement,
            index,
        ));
    }
    WtfString::from(StringImpl::create_by_replacing_in_characters16(string.span16(), target, replacement, index))
}

/// `makeStringBySimplifyingNewLinesSlowCase<CharacterType>(string, firstCarriageReturn)`.
fn make_string_by_simplifying_new_lines_slow_case_characters<T: CharType>(
    characters: &[T],
    first_carriage_return: u32,
) -> WtfString {
    let length = characters.len();
    let first_carriage_return = first_carriage_return as usize;
    let mut result_length = first_carriage_return;
    let result = WtfString::create_uninitialized::<T>(length, |result_characters| {
        result_characters[..first_carriage_return].copy_from_slice(&characters[..first_carriage_return]);
        let mut i = first_carriage_return;
        while i < length {
            if unit(characters[i]) != '\r' as u32 {
                result_characters[result_length] = characters[i];
                result_length += 1;
            } else {
                result_characters[result_length] = T::from_u16('\n' as u16);
                result_length += 1;
                if i + 1 < length && unit(characters[i + 1]) == '\n' as u32 {
                    i += 1;
                }
            }
            i += 1;
        }
    });
    if result_length < length {
        if let Some(result_impl) = result.impl_() {
            return WtfString::from(StringImpl::create_substring_sharing_impl(result_impl, 0, result_length as u32));
        }
    }
    result
}

/// `makeStringBySimplifyingNewLinesSlowCase(const String&, unsigned firstCarriageReturnOffset)`.
pub fn make_string_by_simplifying_new_lines_slow_case(string: &WtfString, first_carriage_return: u32) -> WtfString {
    if string.is_8bit() {
        return make_string_by_simplifying_new_lines_slow_case_characters(string.span8(), first_carriage_return);
    }
    make_string_by_simplifying_new_lines_slow_case_characters(string.span16(), first_carriage_return)
}

/// `makeStringBySimplifyingNewLines(const String&)`: `\r\n` e `\r` viram `\n`.
pub fn make_string_by_simplifying_new_lines(string: &WtfString) -> WtfString {
    let first_carriage_return = string.find_character('\r' as u16, 0);
    if first_carriage_return == NOT_FOUND {
        return string.clone();
    }
    make_string_by_simplifying_new_lines_slow_case(string, first_carriage_return as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn latin1(bytes: &[u8]) -> StringView<'_> {
        StringView::from(bytes)
    }

    fn entries(result: SplitResult<'_>) -> Vec<Vec<u8>> {
        result.into_iter().map(|entry| entry.span8().to_vec()).collect()
    }

    #[test]
    fn null_empty_and_lengths() {
        let null = StringView::default();
        assert!(null.is_null() && null.is_empty() && null.is_8bit());
        assert_eq!(null.length(), 0);
        let empty = empty_string_view();
        assert!(!empty.is_null() && empty.is_empty());
        assert!(equal(null, empty));
        assert!(!equal_respecting_nullity(null, empty));
        assert!(equal_respecting_nullity(null, null));
        let wide_units = [0x61u16, 0x20AC];
        let wide = StringView::from(&wide_units[..]);
        assert!(!wide.is_8bit());
        assert_eq!(wide.size_in_bytes(), 4);
        assert_eq!(wide.code_unit_at(1), 0x20AC);
    }

    #[test]
    fn from_string_and_impl() {
        assert!(StringView::from(&WtfString::default()).is_null());
        let hello = WtfString::from_latin1(b"hello");
        let view = StringView::from(&hello);
        assert_eq!(view.span8(), b"hello");
        assert!(StringView::from(&WtfString::from_latin1(b"")).is_empty());
        assert!(!StringView::from(&WtfString::from_latin1(b"")).is_null());
        let wide = WtfString::from_utf16(&[0x20AC]);
        assert!(!StringView::from(&wide).is_8bit());
        assert_eq!(view.to_string(), hello);
        assert!(StringView::default().to_string().is_null());
    }

    #[test]
    fn substring_left_right() {
        let view = latin1(b"abcdef");
        assert_eq!(view.substring(2, 3).span8(), b"cde");
        assert_eq!(view.substring(4, u32::MAX).span8(), b"ef");
        assert!(view.substring(6, 1).is_empty());
        assert_eq!(view.left(2).span8(), b"ab");
        assert_eq!(view.right(2).span8(), b"ef");
        assert_eq!(view.substring(0, u32::MAX).span8(), b"abcdef");
    }

    #[test]
    fn find_family() {
        let view = latin1(b"hello world");
        assert_eq!(view.find(latin1(b"o w"), 0), 4);
        assert_eq!(view.find(latin1(b"o"), 5), 7);
        assert_eq!(view.find(latin1(b""), 3), 3);
        assert_eq!(view.find(latin1(b"zz"), 0), NOT_FOUND);
        assert_eq!(view.find(StringView::from([0x20ACu16].as_slice()), 0), NOT_FOUND);
        assert_eq!(view.find_character('w' as u16, 0), 6);
        assert_eq!(view.find_matching(|c| c == 'd' as u16, 0), 10);
        assert_eq!(view.find_latin1(b"world", 0), 6);
        assert_eq!(view.reverse_find(latin1(b"o"), u32::MAX), 7);
        assert_eq!(view.reverse_find(latin1(b"lo"), u32::MAX), 3);
        assert_eq!(view.reverse_find(latin1(b""), 4), 4);
        assert_eq!(view.reverse_find(StringView::default(), 4), NOT_FOUND);
        assert_eq!(view.reverse_find_latin1(b"or", u32::MAX), 7);
        assert_eq!(view.reverse_find_character('l' as u16, u32::MAX), 9);
        assert_eq!(view.find_ignoring_ascii_case(latin1(b"WORLD"), 0), 6);
        assert!(view.contains(latin1(b"lo w")));
        assert!(view.contains_ignoring_ascii_case(latin1(b"LO W")));
        assert!(view.contains_character('h' as u16));
    }

    #[test]
    fn starts_and_ends_with() {
        let view = latin1(b"Hello");
        assert!(view.starts_with(latin1(b"He")));
        assert!(!view.starts_with(latin1(b"he")));
        assert!(view.starts_with_ignoring_ascii_case(latin1(b"hE")));
        assert!(view.ends_with(latin1(b"llo")));
        assert!(view.ends_with_ignoring_ascii_case(latin1(b"LLO")));
        assert!(view.starts_with_character('H' as u16));
        assert!(view.ends_with_character('o' as u16));
        assert!(view.has_infix_starting_at(latin1(b"ell"), 1));
        assert!(!view.has_infix_starting_at(latin1(b"ell"), 2));
        assert!(view.has_infix_ending_at(latin1(b"ell"), 4));
        assert!(!view.has_infix_ending_at(latin1(b"ell"), 2));
    }

    #[test]
    fn equal_functions() {
        assert!(equal(latin1(b"abc"), StringView::from([0x61u16, 0x62, 0x63].as_slice())));
        assert!(!equal(latin1(b"abc"), latin1(b"abd")));
        assert!(equal_ignoring_ascii_case(latin1(b"AbC"), latin1(b"aBc")));
        assert!(equal_ignoring_ascii_case_latin1(latin1(b"AbC"), b"abc"));
        assert!(equal_letters_ignoring_ascii_case(latin1(b"HeLLo"), b"hello"));
        assert!(!equal_letters_ignoring_ascii_case(latin1(b"HeLLo!"), b"hello"));
        assert!(starts_with_letters_ignoring_ascii_case(latin1(b"HeLLo!"), b"hello"));
        assert!(latin1(b"abc") == latin1(b"abc"));
        assert!(equal_latin1(latin1(b"abc"), Some(b"abc".as_slice())));
        assert!(equal_latin1(latin1(b"abc"), None));
    }

    #[test]
    fn code_points_and_units() {
        let pair_units = [0xD83Du16, 0xDE00, 0x61, 0xD83D];
        let pair = StringView::from(&pair_units[..]);
        assert_eq!(pair.code_point_at(0), 0x1F600);
        assert_eq!(pair.code_point_at(1), 0xDE00);
        assert_eq!(pair.code_point_at(3), 0xD83D);
        assert_eq!(pair.code_point_before(2), 0x1F600);
        assert_eq!(pair.code_point_before(3), 0x61);
        let points: Vec<u32> = pair.code_points().into_iter().collect();
        assert_eq!(points, vec![0x1F600, 0x61, 0xD83D]);
        let reversed: Vec<u32> = pair.code_points().into_iter().rev().collect();
        assert_eq!(reversed, vec![0xD83D, 0x61, 0x1F600]);
        let units: Vec<u16> = latin1(b"ab").code_units().into_iter().collect();
        assert_eq!(units, vec![0x61, 0x62]);
        assert_eq!(StringView::from([0xD83Du16, 0xDE00].as_slice()).convert_to_single_code_point(), Some(0x1F600));
        assert_eq!(latin1(b"ab").convert_to_single_code_point(), None);
        assert_eq!(latin1(b"").convert_to_single_code_point(), None);
    }

    #[test]
    fn split_results() {
        assert_eq!(entries(latin1(b"a,,b").split(',' as u16)), vec![b"a".to_vec(), b"b".to_vec()]);
        assert_eq!(
            entries(latin1(b"a,,b").split_allowing_empty_entries(',' as u16)),
            vec![b"a".to_vec(), b"".to_vec(), b"b".to_vec()]
        );
        assert!(entries(latin1(b"").split(',' as u16)).is_empty());
        assert_eq!(entries(latin1(b"").split_allowing_empty_entries(',' as u16)), vec![b"".to_vec()]);
        assert!(entries(latin1(b",,").split(',' as u16)).is_empty());
    }

    #[test]
    fn trim_and_case() {
        let view = latin1(b"  ab  ");
        assert_eq!(view.trim(|c| c == ' ' as u16).span8(), b"ab");
        assert!(latin1(b"   ").trim(|c| c == ' ' as u16).is_empty());
        assert_eq!(latin1(b"aBc").convert_to_ascii_lowercase(), WtfString::from_latin1(b"abc"));
        assert_eq!(latin1(b"aBc").convert_to_ascii_uppercase(), WtfString::from_latin1(b"ABC"));
        assert!(StringView::default().convert_to_ascii_lowercase().is_null());
        let mut destination = [0u16; 3];
        latin1(b"aBc").get_characters_with_ascii_case(CaseConvertType::Upper, &mut destination);
        assert_eq!(destination, [0x41, 0x42, 0x43]);
    }

    #[test]
    fn conversions_and_replace() {
        assert_eq!(latin1(b"caf\xE9").utf8(ConversionMode::LenientConversion), "café".as_bytes());
        assert_eq!(StringView::default().utf8(ConversionMode::LenientConversion), Vec::<u8>::new());
        assert_eq!(&*latin1(b"ab").upconverted_characters(), &[0x61u16, 0x62]);
        assert_eq!(latin1(b"1.5").to_double(), (1.5, true));
        let mut buffer: Vec<u16> = vec![0x78];
        append(&mut buffer, latin1(b"yz"));
        assert_eq!(buffer, vec![0x78, 0x79, 0x7A]);
        assert_eq!(
            make_string_by_replacing_all_characters(latin1(b"a.b."), '.' as u16, '-' as u16),
            WtfString::from_latin1(b"a-b-")
        );
        assert_eq!(
            make_string_by_replacing_all(
                &WtfString::from_latin1(b"aXXbXX"),
                latin1(b"XX"),
                StringView::from([0x20ACu16].as_slice())
            ),
            WtfString::from_utf16(&[0x61, 0x20AC, 0x62, 0x20AC])
        );
        assert_eq!(
            make_string_by_simplifying_new_lines(&WtfString::from_latin1(b"a\r\nb\rc\n")),
            WtfString::from_latin1(b"a\nb\nc\n")
        );
        assert!(!has_unpaired_surrogate(latin1(b"abc")));
        assert!(has_unpaired_surrogate(StringView::from([0xD83Du16].as_slice())));
        assert_eq!(code_point_compare(latin1(b"ab"), StringView::from([0x61u16, 0x62, 0x63].as_slice())), Ordering::Less);
        assert!(code_point_compare_less_than(latin1(b"a"), latin1(b"b")));
    }
}
