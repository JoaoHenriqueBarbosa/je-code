//! Tradução de `WTF/wtf/text/StringImpl.h`: a declaração da classe, os enums, as constantes de
//! flags e as funções inline do cabeçalho, mais as funções do `StringImpl.cpp` até a linha 840
//! (criação, substring, conversões de caixa). O resto do `.cpp` (trim, find, replace, equal,
//! UTF-8 etc.) vem nos blocos do fim do arquivo.
//!
//! Modelo (CONVENTIONS, item 1): o buffer é sempre um `Box<[T]>` dono. O buffer interno, o
//! substring e o static do C++ viram cópia; a contagem de referência some porque o `String` é um
//! `Rc<StringImpl>`. O que a contagem de referência do C++ expressava como "estático" vira o campo
//! `is_static`. As flags (`hash_and_flags`) mantêm o mesmo layout de bits do C++.

use std::cell::Cell;
use std::cmp::Ordering;
use std::rc::Rc;

use crate::wtf::ascii_ctype::{
    is_ascii, is_ascii_alpha_caseless_equal, is_ascii_lower, is_ascii_upper, to_ascii_lower, to_ascii_upper, AsciiChar,
};
use crate::wtf::text::string_hasher;
use crate::wtf::unicode::case_mapping::{
    fold_case as icu_fold_case, str_fold_case, str_to_lower, str_to_upper, to_lower, to_upper,
};
use crate::wtf::unicode::character_names::SMALL_LETTER_SHARP_S;

/// `Latin1Character`.
pub type LChar = u8;
/// `char16_t`.
pub type UChar = u16;

use crate::wtf::text::string_common::{
    characters_are_all_ascii, equal_prefix, find_inner, reverse_find_inner, unit, NOT_FOUND,
};
/// `WTF::find` e `WTF::reverseFind` de `StringCommon.h` vivem em `string_common`; os caminhos
/// antigos continuam valendo.
pub use crate::wtf::text::string_common::{find, reverse_find, reverse_find_16_latin1, reverse_find_8_char16};
use crate::wtf::text::string_view::{with_view, with_views, StringView, StringViewData};

/// Valor de `sizeof(StringImpl)` do C++ em x86_64 (refCount 4, length 4, ponteiro 8, hashAndFlags 4,
/// com alinhamento 8). Entra só no cálculo de `is_valid_length` e dos limites de substring.
const CPP_SIZE_OF_STRING_IMPL: usize = 24;

/// `StringImpl::tailOffset<T>()` do C++ (offsetof(m_hashAndFlags) + 4 = 20, arredondado ao
/// alinhamento de T), usado em `allocationSize`.
const CPP_TAIL_OFFSET_8: usize = 20;
const CPP_TAIL_OFFSET_POINTER: usize = 24;

// ---------------------------------------------------------------------------------------------
// CharType
// ---------------------------------------------------------------------------------------------

/// O parâmetro de template `CharacterType` do C++ (`LChar` ou `UChar`).
/// A parte numérica (`SIZE`, `to_u16`, `from_u16`) era a de `ul_common::js_number::CharType` no zjsc;
/// aqui vive no próprio trait (cópia local, sem o `fast_float`).
pub trait CharType: Copy + Into<u32> + Eq + Ord + 'static {
    /// `sizeof(CharacterType)`.
    const SIZE: usize;

    /// O caractere como unidade de código de 16 bits.
    fn to_u16(self) -> u16;

    /// Constrói a partir de uma unidade de 16 bits (trunca no tipo de 8 bits).
    fn from_u16(character: u16) -> Self;

    /// `StringImpl::span<CharacterType>()`.
    fn span_of(string: &StringImpl) -> &[Self];

    /// `StringImpl::create(std::span<const CharacterType>)`.
    fn create(characters: &[Self]) -> Rc<StringImpl>;

    /// `StringView::span<CharacterType>()`.
    fn view_span<'a>(view: StringView<'a>) -> &'a [Self];

    /// `StringView(std::span<const CharacterType>)`.
    fn make_view<'a>(span: &'a [Self]) -> StringView<'a>;
}

impl CharType for u8 {
    const SIZE: usize = 1;

    fn to_u16(self) -> u16 {
        self as u16
    }

    fn from_u16(character: u16) -> Self {
        character as u8
    }

    fn span_of(string: &StringImpl) -> &[u8] {
        string.span8()
    }

    fn create(characters: &[u8]) -> Rc<StringImpl> {
        StringImpl::create(characters)
    }

    fn view_span<'a>(view: StringView<'a>) -> &'a [u8] {
        view.span8()
    }

    fn make_view<'a>(span: &'a [u8]) -> StringView<'a> {
        StringView::from(span)
    }
}

impl CharType for u16 {
    const SIZE: usize = 2;

    fn to_u16(self) -> u16 {
        self
    }

    fn from_u16(character: u16) -> Self {
        character
    }

    fn span_of(string: &StringImpl) -> &[u16] {
        string.span16()
    }

    fn create(characters: &[u16]) -> Rc<StringImpl> {
        StringImpl::create16(characters)
    }

    fn view_span<'a>(view: StringView<'a>) -> &'a [u16] {
        view.span16()
    }

    fn make_view<'a>(span: &'a [u16]) -> StringView<'a> {
        StringView::from(span)
    }
}

// ---------------------------------------------------------------------------------------------
// Enums e constantes de flags
// ---------------------------------------------------------------------------------------------

/// `StringImpl::BufferOwnership`. No porte o buffer é sempre dono; o valor só vive nos bits de
/// flag, como no C++.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum BufferOwnership {
    BufferInternal = 0,
    BufferOwned = 1,
    BufferSubstring = 2,
    BufferExternal = 3,
}

/// `StringImpl::StringKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum StringKind {
    /// Nem símbolo nem atômica.
    StringNormal = 0,
    /// Atômica.
    StringAtom = S_HASH_FLAG_STRING_KIND_IS_ATOM,
    /// Símbolo, não atômica.
    StringSymbol = S_HASH_FLAG_STRING_KIND_IS_SYMBOL,
}

/// `StringImplShape::MaxLength`.
pub const MAX_LENGTH: u32 = i32::MAX as u32;

/// Os 6 bits baixos do hash são flags, mas se reservam 8 porque o `StringHash` só tem 24 bits.
pub const S_FLAG_COUNT: u32 = 8;

const S_FLAG_MASK: u32 = (1u32 << S_FLAG_COUNT) - 1;
const S_FLAG_STRING_KIND_COUNT: u32 = 4;
/// `s_hashFlagNeverAtomize` (`USE(BUN_JSC_ADDITIONS)` vale no `cmakeconfig.h`).
const S_HASH_FLAG_NEVER_ATOMIZE: u32 = 1u32 << 6;

/// DIVERGÊNCIA: no C++ o `UniquedStringImpl*` de um símbolo É o `SymbolImpl` e `isPrivate()` lê as flags
/// dele. Aqui o `StringImpl` do símbolo não aponta de volta, então o bit 7 (livre) marca o símbolo privado.
const S_HASH_FLAG_IS_PRIVATE_SYMBOL: u32 = 1u32 << 7;

const S_HASH_ZERO_VALUE: u32 = 0;
const S_HASH_FLAG_STRING_KIND_IS_ATOM: u32 = 1u32 << S_FLAG_STRING_KIND_COUNT;
const S_HASH_FLAG_STRING_KIND_IS_SYMBOL: u32 = 1u32 << (S_FLAG_STRING_KIND_COUNT + 1);
const S_HASH_MASK_STRING_KIND: u32 = S_HASH_FLAG_STRING_KIND_IS_ATOM | S_HASH_FLAG_STRING_KIND_IS_SYMBOL;
const S_HASH_FLAG_DID_REPORT_COST: u32 = 1u32 << 3;
const S_HASH_FLAG_8BIT_BUFFER: u32 = 1u32 << 2;
const S_HASH_MASK_BUFFER_OWNERSHIP: u32 = (1u32 << 0) | (1u32 << 1);

// ---------------------------------------------------------------------------------------------
// StringImpl
// ---------------------------------------------------------------------------------------------

/// O buffer de caracteres: Latin1 ou UTF-16.
#[derive(Clone, Debug)]
pub enum StringData {
    Latin1(Box<[u8]>),
    Utf16(Box<[u16]>),
}

/// `class StringImpl`. Imutável salvo pelos bits de `hash_and_flags` (o `mutable` do C++).
#[derive(Debug)]
pub struct StringImpl {
    data: StringData,
    /// Hash nos 24 bits altos, flags nos 8 baixos.
    hash_and_flags: Cell<u32>,
    /// O C++ marca a string estática no bit baixo da contagem de referência; sem contagem, é um
    /// campo. Nunca muda depois da construção.
    is_static: bool,
    /// `SymbolImpl::m_hashForSymbolShiftedWithFlagCount`. O C++ guarda o campo no `SymbolImpl`, que
    /// é o próprio objeto; o porte separa os tipos, então o valor mora aqui para que
    /// `existingSymbolAwareHash()` o alcance a partir do `StringImpl`. Zero fora de símbolos.
    hash_for_symbol_shifted_with_flag_count: Cell<u32>,
}

thread_local! {
    /// `StringImpl::s_emptyAtomString`: a string vazia, atômica e estática.
    static EMPTY_ATOM_STRING: Rc<StringImpl> = Rc::new(StringImpl::new_static_empty());
}

impl StringImpl {
    pub const MAX_LENGTH: u32 = MAX_LENGTH;
    pub const S_FLAG_COUNT: u32 = S_FLAG_COUNT;

    // ---- construtores internos -----------------------------------------------------------

    fn construct(data: StringData, hash_and_flags: u32, is_static: bool) -> StringImpl {
        StringImpl {
            data,
            hash_and_flags: Cell::new(hash_and_flags),
            is_static,
            hash_for_symbol_shifted_with_flag_count: Cell::new(0),
        }
    }

    /// Normal de 8 bits com buffer interno (`StringImpl(length, Force8Bit)`).
    fn new8(data: Box<[u8]>) -> StringImpl {
        Self::construct(
            StringData::Latin1(data),
            S_HASH_FLAG_8BIT_BUFFER | StringKind::StringNormal as u32 | BufferOwnership::BufferInternal as u32,
            false,
        )
    }

    /// Normal de 16 bits com buffer interno (`StringImpl(length)`).
    fn new16(data: Box<[u16]>) -> StringImpl {
        Self::construct(
            StringData::Utf16(data),
            S_HASH_ZERO_VALUE | StringKind::StringNormal as u32 | BufferOwnership::BufferInternal as u32,
            false,
        )
    }

    /// `StaticStringImpl("", StringAtom)`: hash calculado na construção e custo já reportado.
    fn new_static_empty() -> StringImpl {
        let hash = string_hasher::compute_literal_hash_and_mask_top8_bits::<u8>(&[]);
        Self::construct(
            StringData::Latin1(Box::new([])),
            S_HASH_FLAG_8BIT_BUFFER
                | S_HASH_FLAG_DID_REPORT_COST
                | StringKind::StringAtom as u32
                | BufferOwnership::BufferInternal as u32
                | (hash << S_FLAG_COUNT),
            true,
        )
    }

    /// `StringImpl(CreateSymbol, std::span<const Latin1Character>)`: símbolo cujo buffer o C++
    /// compartilha com a descrição (BufferSubstring); aqui é cópia.
    pub(crate) fn new_symbol8(characters: &[u8]) -> StringImpl {
        Self::construct(
            StringData::Latin1(characters.into()),
            S_HASH_FLAG_8BIT_BUFFER | StringKind::StringSymbol as u32 | BufferOwnership::BufferSubstring as u32,
            false,
        )
    }

    /// `StringImpl(CreateSymbol, std::span<const char16_t>)`.
    pub(crate) fn new_symbol16(characters: &[u16]) -> StringImpl {
        Self::construct(
            StringData::Utf16(characters.into()),
            S_HASH_ZERO_VALUE | StringKind::StringSymbol as u32 | BufferOwnership::BufferSubstring as u32,
            false,
        )
    }

    /// `StringImpl(CreateSymbol)`: símbolo nulo (buffer vazio de 8 bits).
    pub(crate) fn new_null_symbol() -> StringImpl {
        Self::new_symbol8(&[])
    }

    // ---- create / empty ------------------------------------------------------------------

    /// `StringImpl::empty()`.
    pub fn empty() -> Rc<StringImpl> {
        EMPTY_ATOM_STRING.with(Rc::clone)
    }

    /// `StringImpl::create(std::span<const Latin1Character>)`.
    pub fn create(characters: &[u8]) -> Rc<StringImpl> {
        if characters.is_empty() {
            return Self::empty();
        }
        Rc::new(Self::new8(characters.into()))
    }

    /// `StringImpl::create(std::span<const char16_t>)`: sem estreitar, como o C++.
    pub fn create16(characters: &[u16]) -> Rc<StringImpl> {
        if characters.is_empty() {
            return Self::empty();
        }
        Rc::new(Self::new16(characters.into()))
    }

    /// `StringImpl::create8BitUnconditionally(std::span<const char16_t>)`: o chamador garantiu
    /// Latin1; trunca cada unidade.
    pub fn create8_bit_unconditionally(characters: &[u16]) -> Rc<StringImpl> {
        if characters.is_empty() {
            return Self::empty();
        }
        let narrowed: Vec<u8> = characters.iter().map(|c| *c as u8).collect();
        Rc::new(Self::new8(narrowed.into_boxed_slice()))
    }

    /// `StringImpl::create8BitIfPossible(std::span<const char16_t>)`.
    pub fn create8_bit_if_possible(characters: &[u16]) -> Rc<StringImpl> {
        if characters.is_empty() {
            return Self::empty();
        }
        if characters.iter().all(|c| *c <= 0xFF) {
            return Self::create8_bit_unconditionally(characters);
        }
        Self::create16(characters)
    }

    /// `StringImpl::create(std::span<const char8_t>)`: UTF-8 para UTF-16, `None` se houver
    /// sequência inválida. Entrada vazia dá a string vazia; ASCII puro dá 8 bits.
    pub fn create_from_utf8(code_units: &[u8]) -> Option<Rc<StringImpl>> {
        if code_units.is_empty() {
            return Some(Self::empty());
        }
        if code_units.is_ascii() {
            return Some(Self::create(code_units));
        }
        let text = std::str::from_utf8(code_units).ok()?;
        let units: Vec<u16> = text.encode_utf16().collect();
        Some(Self::create16(&units))
    }

    /// `StringImpl::createSubstringSharingImpl`. O C++ copia quando a cópia cabe no espaço que o
    /// substring ocuparia (`allocationSize<StringImpl*>(1)`) e, para 16 bits, estreita para 8 bits
    /// quando tudo é Latin1; o resto compartilha o dono, o que aqui também é cópia. Os limites de
    /// cópia ficam para manter `is_8bit` igual ao do C++.
    pub fn create_substring_sharing_impl(rep: &StringImpl, offset: u32, length: u32) -> Rc<StringImpl> {
        let (offset, length) = (offset as usize, length as usize);
        if length == 0 {
            return Self::empty();
        }
        let substring_size = CPP_TAIL_OFFSET_POINTER + std::mem::size_of::<usize>();
        match &rep.data {
            StringData::Latin1(data) => Self::create(&data[offset..offset + length]),
            StringData::Utf16(data) => {
                let span = &data[offset..offset + length];
                if substring_size >= CPP_TAIL_OFFSET_8 + length && span.iter().all(|c| *c <= 0xFF) {
                    return Self::create8_bit_unconditionally(span);
                }
                Self::create16(span)
            }
        }
    }

    /// `StringImpl::createByReplacingInCharacters(span<Latin1>, ...)`.
    pub fn create_by_replacing_in_characters8(
        characters: &[u8],
        target: u16,
        replacement: u16,
        index_of_first_target_character: usize,
    ) -> Rc<StringImpl> {
        if replacement <= 0xFF {
            let old_char = target as u8;
            let new_char = replacement as u8;
            let mut data: Vec<u8> = characters[..index_of_first_target_character].to_vec();
            for &character in &characters[index_of_first_target_character..] {
                data.push(if character == old_char { new_char } else { character });
            }
            return Self::create(&data);
        }

        let data: Vec<u16> = characters
            .iter()
            .map(|&character| if character as u16 == target { replacement } else { character as u16 })
            .collect();
        Self::create16(&data)
    }

    /// `StringImpl::createByReplacingInCharacters(span<char16_t>, ...)`.
    pub fn create_by_replacing_in_characters16(
        characters: &[u16],
        target: u16,
        replacement: u16,
        index_of_first_target_character: usize,
    ) -> Rc<StringImpl> {
        let mut data: Vec<u16> = characters[..index_of_first_target_character].to_vec();
        for &character in &characters[index_of_first_target_character..] {
            data.push(if character == target { replacement } else { character });
        }
        Self::create16(&data)
    }

    /// `StringImpl::adopt(Vector&&)`.
    pub fn adopt<T: CharType>(vector: Vec<T>) -> Rc<StringImpl> {
        T::create(&vector)
    }

    // ---- flags estáticas -----------------------------------------------------------------

    pub const fn flag_is_8bit() -> u32 {
        S_HASH_FLAG_8BIT_BUFFER
    }

    pub const fn flag_is_atom() -> u32 {
        S_HASH_FLAG_STRING_KIND_IS_ATOM
    }

    pub const fn flag_is_symbol() -> u32 {
        S_HASH_FLAG_STRING_KIND_IS_SYMBOL
    }

    pub const fn mask_string_kind() -> u32 {
        S_HASH_MASK_STRING_KIND
    }

    /// `StringImpl::isValidLength<CharacterType>`.
    pub fn is_valid_length<T: CharType>(length: usize) -> bool {
        let max = std::cmp::min(
            MAX_LENGTH as usize,
            (u32::MAX as usize - CPP_SIZE_OF_STRING_IMPL) / T::SIZE,
        );
        length <= max
    }

    // ---- acesso ao conteúdo --------------------------------------------------------------

    pub fn length(&self) -> u32 {
        match &self.data {
            StringData::Latin1(data) => data.len() as u32,
            StringData::Utf16(data) => data.len() as u32,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.length() == 0
    }

    pub fn is_8bit(&self) -> bool {
        self.hash_and_flags.get() & S_HASH_FLAG_8BIT_BUFFER != 0
    }

    /// `span8()`: o C++ exige `is8Bit()`; numa string de 16 bits devolve fatia vazia.
    pub fn span8(&self) -> &[u8] {
        match &self.data {
            StringData::Latin1(data) => data,
            StringData::Utf16(_) => &[],
        }
    }

    /// `span16()`: o C++ exige `!is8Bit() || isEmpty()`; numa string de 8 bits devolve fatia vazia.
    pub fn span16(&self) -> &[u16] {
        match &self.data {
            StringData::Utf16(data) => data,
            StringData::Latin1(_) => &[],
        }
    }

    /// `span<CharacterType>()`.
    pub fn span<T: CharType>(&self) -> &[T] {
        T::span_of(self)
    }

    /// `StringImpl::at(i)` (`operator[]`).
    pub fn char_at(&self, i: u32) -> u16 {
        match &self.data {
            StringData::Latin1(data) => data[i as usize] as u16,
            StringData::Utf16(data) => data[i as usize],
        }
    }

    // ---- custo ---------------------------------------------------------------------------

    /// `StringImpl::cost()`. O substring do C++ devolve o custo da base; aqui o símbolo tem cópia
    /// própria e conta o seu.
    pub fn cost(&self) -> usize {
        let flags = self.hash_and_flags.get();
        if flags & S_HASH_FLAG_DID_REPORT_COST != 0 {
            return 0;
        }

        self.hash_and_flags.set(flags | S_HASH_FLAG_DID_REPORT_COST);
        let mut result = self.length() as usize;
        if !self.is_8bit() {
            result <<= 1;
        }
        result
    }

    /// `StringImpl::costDuringGC()`. O C++ divide pela contagem de referência; ela vive no `Rc`,
    /// então o chamador passa `Rc::strong_count`.
    pub fn cost_during_gc(&self, ref_count: usize) -> usize {
        if self.is_static() {
            return 0;
        }

        let mut result = self.length() as usize;
        if !self.is_8bit() {
            result <<= 1;
        }
        result.div_ceil(ref_count)
    }

    // ---- espécie da string ---------------------------------------------------------------

    /// Liga a marca de símbolo privado (ver `S_HASH_FLAG_IS_PRIVATE_SYMBOL`).
    pub(crate) fn mark_private_symbol(&self) {
        self.hash_and_flags.set(self.hash_and_flags.get() | S_HASH_FLAG_IS_PRIVATE_SYMBOL);
    }

    /// `static_cast<SymbolImpl&>(*this).isPrivate()` para um `StringImpl` de espécie símbolo.
    pub fn is_private_symbol(&self) -> bool {
        self.is_symbol() && self.hash_and_flags.get() & S_HASH_FLAG_IS_PRIVATE_SYMBOL != 0
    }

    /// `SymbolImpl::m_hashForSymbolShiftedWithFlagCount`.
    pub(crate) fn hash_for_symbol_shifted_with_flag_count(&self) -> u32 {
        self.hash_for_symbol_shifted_with_flag_count.get()
    }

    pub(crate) fn set_hash_for_symbol_shifted_with_flag_count(&self, value: u32) {
        self.hash_for_symbol_shifted_with_flag_count.set(value);
    }

    /// `StringImpl::symbolAwareHash()`.
    pub fn symbol_aware_hash(&self) -> u32 {
        if self.is_symbol() {
            return self.hash_for_symbol_shifted_with_flag_count.get() >> S_FLAG_COUNT;
        }
        self.hash()
    }

    /// `StringImpl::existingSymbolAwareHash()`.
    pub fn existing_symbol_aware_hash(&self) -> u32 {
        if self.is_symbol() {
            return self.hash_for_symbol_shifted_with_flag_count.get() >> S_FLAG_COUNT;
        }
        self.existing_hash()
    }

    pub fn is_symbol(&self) -> bool {
        self.hash_and_flags.get() & S_HASH_FLAG_STRING_KIND_IS_SYMBOL != 0
    }

    pub fn is_atom(&self) -> bool {
        self.hash_and_flags.get() & S_HASH_FLAG_STRING_KIND_IS_ATOM != 0
    }

    pub fn set_is_atom(&self, is_atom: bool) {
        let flags = self.hash_and_flags.get();
        if is_atom {
            self.hash_and_flags.set(flags | S_HASH_FLAG_STRING_KIND_IS_ATOM);
        } else {
            self.hash_and_flags.set(flags & !S_HASH_FLAG_STRING_KIND_IS_ATOM);
        }
    }

    pub fn is_external(&self) -> bool {
        self.buffer_ownership() == BufferOwnership::BufferExternal
    }

    pub fn is_sub_string(&self) -> bool {
        self.buffer_ownership() == BufferOwnership::BufferSubstring
    }

    pub fn is_static(&self) -> bool {
        self.is_static
    }

    pub fn buffer_ownership(&self) -> BufferOwnership {
        match self.hash_and_flags.get() & S_HASH_MASK_BUFFER_OWNERSHIP {
            0 => BufferOwnership::BufferInternal,
            1 => BufferOwnership::BufferOwned,
            2 => BufferOwnership::BufferSubstring,
            _ => BufferOwnership::BufferExternal,
        }
    }

    /// `canBecomeAtom()` (adição do Bun).
    pub fn can_become_atom(&self) -> bool {
        self.hash_and_flags.get() & S_HASH_FLAG_NEVER_ATOMIZE == 0
    }

    /// `setNeverAtomize()` (adição do Bun).
    pub fn set_never_atomize(&self) {
        self.hash_and_flags.set(self.hash_and_flags.get() | S_HASH_FLAG_NEVER_ATOMIZE);
    }

    // ---- hash ----------------------------------------------------------------------------

    /// Os bits altos de `hash` são sempre vazios; as flags ficam nos baixos, que é mais barato de
    /// acessar. Por isso se desloca ao guardar e ao ler.
    fn set_hash(&self, hash: u32) {
        self.hash_and_flags.set(self.hash_and_flags.get() | (hash << S_FLAG_COUNT));
    }

    fn raw_hash(&self) -> u32 {
        self.hash_and_flags.get() >> S_FLAG_COUNT
    }

    pub fn has_hash(&self) -> bool {
        self.raw_hash() != 0
    }

    pub fn existing_hash(&self) -> u32 {
        self.raw_hash()
    }

    pub fn hash(&self) -> u32 {
        if self.has_hash() {
            self.raw_hash()
        } else {
            self.hash_slow_case()
        }
    }

    fn hash_slow_case(&self) -> u32 {
        self.set_hash(self.concurrent_hash());
        self.existing_hash()
    }

    /// `StringImpl::concurrentHash()`: calcula sem gravar.
    pub fn concurrent_hash(&self) -> u32 {
        match &self.data {
            StringData::Latin1(data) => string_hasher::compute_hash_and_mask_top8_bits::<u8>(data),
            StringData::Utf16(data) => string_hasher::compute_hash_and_mask_top8_bits::<u16>(data),
        }
    }

    // ---- consultas -----------------------------------------------------------------------

    pub fn contains_only_ascii(&self) -> bool {
        match &self.data {
            StringData::Latin1(data) => characters_are_all_ascii(&data[..]),
            StringData::Utf16(data) => characters_are_all_ascii(&data[..]),
        }
    }

    pub fn contains_only_latin1(&self) -> bool {
        match &self.data {
            StringData::Latin1(_) => true,
            StringData::Utf16(data) => data.iter().all(|c| *c <= 0xFF),
        }
    }

    /// `StringImpl::containsOnly<isSpecialCharacter>()`.
    pub fn contains_only(&self, is_special_character: fn(u16) -> bool) -> bool {
        match &self.data {
            StringData::Latin1(data) => contains_only(&data[..], is_special_character),
            StringData::Utf16(data) => contains_only(&data[..], is_special_character),
        }
    }

    /// `find(Latin1Character / char / char16_t, start)`.
    pub fn find_character(&self, character: u16, start: usize) -> usize {
        match &self.data {
            StringData::Latin1(data) => find(&data[..], |c| c == character, start),
            StringData::Utf16(data) => find(&data[..], |c| c == character, start),
        }
    }

    /// `find(CodeUnitMatchFunction, start)`.
    pub fn find_matching(&self, match_function: impl Fn(u16) -> bool, start: usize) -> usize {
        match &self.data {
            StringData::Latin1(data) => find(&data[..], match_function, start),
            StringData::Utf16(data) => find(&data[..], match_function, start),
        }
    }

    /// `StringImpl::removeCharacters(predicate)`: devolve a própria string se nada casa.
    pub fn remove_characters(self: &Rc<Self>, find_match: impl Fn(u16) -> bool) -> Rc<StringImpl> {
        match &self.data {
            StringData::Latin1(data) => remove_characters_impl(self, &data[..], &find_match),
            StringData::Utf16(data) => remove_characters_impl(self, &data[..], &find_match),
        }
    }

    /// `StringImpl::isolatedCopy()`: cópia independente (no C++ a cópia protege contra o uso entre
    /// threads; aqui é sempre uma string nova com o mesmo conteúdo).
    pub fn isolated_copy(&self) -> Rc<StringImpl> {
        match &self.data {
            StringData::Latin1(data) => Self::create(data),
            StringData::Utf16(data) => Self::create16(data),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// StringImpl.cpp (linhas 1 a 840): criação, substring, conversões de caixa
// ---------------------------------------------------------------------------------------------

/// `UTF8ConversionError` de `wtf/text/UTF8ConversionError.h` (`OutOfMemory`, `Invalid`). Só existe
/// aqui porque o `tryReallocate` e o `tryGetUTF8` o devolvem; o módulo próprio ainda não foi
/// portado e este tipo se move para lá.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UTF8ConversionError {
    OutOfMemory,
    Invalid,
}

/// `StringImpl::CaseConvertType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseConvertType {
    Upper,
    Lower,
}

/// `U16_IS_SINGLE(c)`: a unidade não é substituta. As macros do ICU de `utf8_conversion.rs` são
/// privadas daquele módulo, então estas duas vivem aqui.
pub(crate) fn u16_is_single(c: u16) -> bool {
    (c as u32 & 0xFFFF_F800) != 0xD800
}

/// `U16_IS_LEAD(c)`.
fn u16_is_lead(c: u16) -> bool {
    (c as u32 & 0xFFFF_FC00) == 0xD800
}

/// `U16_IS_TRAIL(c)`.
fn u16_is_trail(c: u16) -> bool {
    (c as u32 & 0xFFFF_FC00) == 0xDC00
}

/// `U16_GET_SUPPLEMENTARY(lead, trail)`.
fn u16_get_supplementary(lead: u16, trail: u16) -> u32 {
    ((lead as u32) << 10)
        .wrapping_add(trail as u32)
        .wrapping_sub((0xD800 << 10) + 0xDC00 - 0x10000)
}

/// `locale[index]` de uma `AtomString`. O C++ lê além do fim quando o identificador é curto
/// demais; aqui o que não existe lê como 0, que não é letra e portanto nunca casa.
fn locale_char(locale: &StringImpl, index: u32) -> u16 {
    if index < locale.length() {
        locale.char_at(index)
    } else {
        0
    }
}

/// `needsTurkishCasingRules`.
fn needs_turkish_casing_rules(locale: &StringImpl) -> bool {
    // Either "tr" or "az" locale, with ASCII case insensitive comparison and allowing for an
    // ignored subtag.
    let first = locale_char(locale, 0);
    let second = locale_char(locale, 1);
    ((is_ascii_alpha_caseless_equal(first, b't') && is_ascii_alpha_caseless_equal(second, b'r'))
        || (is_ascii_alpha_caseless_equal(first, b'a') && is_ascii_alpha_caseless_equal(second, b'z')))
        && (locale.length() == 2 || locale_char(locale, 2) == '-' as u16)
}

/// `needsGreekUppercasingRules`.
fn needs_greek_uppercasing_rules(locale: &StringImpl) -> bool {
    // The "el" locale, with ASCII case insensitive comparison and allowing for an ignored subtag.
    is_ascii_alpha_caseless_equal(locale_char(locale, 0), b'e')
        && is_ascii_alpha_caseless_equal(locale_char(locale, 1), b'l')
        && (locale.length() == 2 || locale_char(locale, 2) == '-' as u16)
}

/// `needsLithuanianCasingRules`.
fn needs_lithuanian_casing_rules(locale: &StringImpl) -> bool {
    // The "lt" locale, with ASCII case insensitive comparison and allowing for an ignored subtag.
    is_ascii_alpha_caseless_equal(locale_char(locale, 0), b'l')
        && is_ascii_alpha_caseless_equal(locale_char(locale, 1), b't')
        && (locale.length() == 2 || locale_char(locale, 2) == '-' as u16)
}

impl StringImpl {
    /// `createUninitializedInternalNonEmpty`: o buffer nasce zerado e `fill` o preenche antes de a
    /// string existir (o C++ devolve o span para o chamador escrever).
    pub fn create_uninitialized_internal_non_empty<T: CharType>(
        length: usize,
        fill: impl FnOnce(&mut [T]),
    ) -> Rc<StringImpl> {
        debug_assert!(length != 0);

        // Allocate a single buffer large enough to contain the StringImpl struct as well as the
        // data which it contains.
        if !Self::is_valid_length::<T>(length) {
            // CRASH()
            panic!("StringImpl: comprimento inválido");
        }

        let mut data = vec![T::from_u16(0); length];
        fill(&mut data);
        T::create(&data)
    }

    /// `createUninitializedInternal`.
    fn create_uninitialized_internal<T: CharType>(length: usize, fill: impl FnOnce(&mut [T])) -> Rc<StringImpl> {
        if length == 0 {
            fill(&mut []);
            return Self::empty();
        }

        Self::create_uninitialized_internal_non_empty(length, fill)
    }

    /// `StringImpl::createUninitialized(size_t, std::span<Latin1Character>&)` e a versão de 16
    /// bits: `fill` recebe o buffer a preencher.
    pub fn create_uninitialized<T: CharType>(length: usize, fill: impl FnOnce(&mut [T])) -> Rc<StringImpl> {
        Self::create_uninitialized_internal(length, fill)
    }

    /// `StringImpl::createWithoutCopyingNonEmpty`: o C++ aponta para o buffer do chamador sem
    /// copiar; o porte sempre é dono do buffer, então copia.
    pub fn create_without_copying_non_empty<T: CharType>(characters: &[T]) -> Rc<StringImpl> {
        debug_assert!(!characters.is_empty());
        T::create(characters)
    }

    /// `StringImpl::reallocateInternal`. O C++ exige `hasOneRef()` e buffer interno; o conteúdo
    /// antigo é preservado até o menor dos dois comprimentos, como o `realloc`. `fill` recebe o
    /// novo buffer já com esse conteúdo.
    fn reallocate_internal<T: CharType>(
        original_string: Rc<StringImpl>,
        length: u32,
        fill: impl FnOnce(&mut [T]),
    ) -> Result<Rc<StringImpl>, UTF8ConversionError> {
        debug_assert!(Rc::strong_count(&original_string) == 1);
        debug_assert!(original_string.buffer_ownership() == BufferOwnership::BufferInternal);

        if length == 0 {
            fill(&mut []);
            return Ok(Self::empty());
        }

        // Same as createUninitialized() except here we use fastRealloc.
        if !Self::is_valid_length::<T>(length as usize) {
            return Err(UTF8ConversionError::OutOfMemory);
        }

        let old = original_string.span::<T>();
        let mut data = vec![T::from_u16(0); length as usize];
        let preserved = std::cmp::min(old.len(), data.len());
        data[..preserved].copy_from_slice(&old[..preserved]);
        fill(&mut data);
        Ok(T::create(&data))
    }

    /// `StringImpl::reallocate` (as duas sobrecargas).
    pub fn reallocate<T: CharType>(
        original_string: Rc<StringImpl>,
        length: u32,
        fill: impl FnOnce(&mut [T]),
    ) -> Rc<StringImpl> {
        match Self::try_reallocate(original_string, length, fill) {
            Ok(string_impl) => string_impl,
            // RELEASE_ASSERT(expectedStringImpl)
            Err(_) => panic!("StringImpl::reallocate: sem memória"),
        }
    }

    /// `StringImpl::tryReallocate` (as duas sobrecargas): o tipo `T` exige a largura da string
    /// original (`ASSERT(is8Bit())` para `u8`, `ASSERT(!is8Bit())` para `u16`).
    pub fn try_reallocate<T: CharType>(
        original_string: Rc<StringImpl>,
        length: u32,
        fill: impl FnOnce(&mut [T]),
    ) -> Result<Rc<StringImpl>, UTF8ConversionError> {
        debug_assert!(original_string.is_8bit() == (T::SIZE == 1));
        Self::reallocate_internal(original_string, length, fill)
    }

    /// `StringImpl::createStaticStringImpl(std::span<const Latin1Character>)`.
    pub fn create_static_string_impl8(characters: &[u8]) -> Rc<StringImpl> {
        if characters.is_empty() {
            return Self::empty();
        }
        let mut result = Self::new8(characters.into());
        result.hash();
        result.is_static = true;
        Rc::new(result)
    }

    /// `StringImpl::createStaticStringImpl(std::span<const char16_t>)`.
    pub fn create_static_string_impl16(characters: &[u16]) -> Rc<StringImpl> {
        if characters.is_empty() {
            return Self::empty();
        }
        let result = Self::create8_bit_if_possible(characters);
        result.hash();
        // O `Rc` acabou de nascer e ainda é único, então o campo se muda sem compartilhar.
        match Rc::try_unwrap(result) {
            Ok(mut owned) => {
                owned.is_static = true;
                Rc::new(owned)
            }
            Err(shared) => shared,
        }
    }

    /// `StringImpl::substring(position, length)`; o `length` padrão do C++ é `MaxLength`.
    pub fn substring(self: &Rc<Self>, start: u32, length: u32) -> Rc<StringImpl> {
        let m_length = self.length();
        if start >= m_length {
            return Self::empty();
        }
        let max_length = m_length - start;
        let mut length = length;
        if length >= max_length {
            if start == 0 {
                return Rc::clone(self);
            }
            length = max_length;
        }
        let (start, length) = (start as usize, length as usize);
        if self.is_8bit() {
            return Self::create(&self.span8()[start..start + length]);
        }

        Self::create16(&self.span16()[start..start + length])
    }

    /// `StringImpl::codePointAt(i)`.
    pub fn code_point_at(&self, i: u32) -> u32 {
        if self.is_8bit() {
            return self.span8()[i as usize] as u32;
        }
        let span = self.span16();
        let i = i as usize;
        if u16_is_single(span[i]) {
            return span[i] as u32;
        }
        if i + 1 < self.length() as usize && u16_is_lead(span[i]) && u16_is_trail(span[i + 1]) {
            return u16_get_supplementary(span[i], span[i + 1]);
        }
        span[i] as u32
    }

    /// `StringView(*this).upconvertedCharacters()`: o texto como UTF-16.
    fn upconverted_characters(&self) -> Vec<u16> {
        match &self.data {
            StringData::Latin1(data) => data.iter().map(|c| *c as u16).collect(),
            StringData::Utf16(data) => data.to_vec(),
        }
    }

    // ---- caixa sem locale ----------------------------------------------------------------

    /// `StringImpl::convertToLowercaseWithoutLocale()`.
    pub fn convert_to_lowercase_without_locale(self: &Rc<Self>) -> Rc<StringImpl> {
        // Note: At one time this was a hot function in the Dromaeo benchmark, specifically the
        // no-op code path that may return ourself if we find no upper case letters and no
        // invalid ASCII letters.

        // First scan the string for uppercase and non-ASCII characters:
        if self.is_8bit() {
            let span = self.span8();
            for (i, &character) in span.iter().enumerate() {
                if !is_ascii(character) || is_ascii_upper(character) {
                    return self.convert_to_lowercase_without_locale_starting_at_failing_index8_bit(i as u32);
                }
            }

            return Rc::clone(self);
        }

        self.convert_to_lowercase_without_locale_starting_at_failing_index16_bit(0)
    }

    /// `StringImpl::convertToLowercaseWithoutLocaleStartingAtFailingIndex16Bit`.
    pub fn convert_to_lowercase_without_locale_starting_at_failing_index16_bit(
        self: &Rc<Self>,
        failing_index: u32,
    ) -> Rc<StringImpl> {
        debug_assert!(!self.is_8bit());
        let span = self.span16();
        let failing_index = failing_index as usize;

        // Characters before the failing index are already known to be ASCII with no uppercase
        // among them, so only the rest can decide which of the paths below applies.
        let mut no_upper = true;
        let mut ored: u32 = 0;

        for &character in &span[failing_index..] {
            if is_ascii_upper(character) {
                no_upper = false;
            }
            ored |= character as u32;
        }
        // Nothing to do if the string is all ASCII with no uppercase.
        if no_upper && (ored & !0x7F) == 0 {
            return Rc::clone(self);
        }

        if (ored & !0x7F) == 0 {
            return Self::create_uninitialized_internal_non_empty::<u16>(span.len(), |data16| {
                copy_characters(data16, &span[..failing_index]);
                for i in failing_index..span.len() {
                    data16[i] = to_ascii_lower(span[i]);
                }
            });
        }

        // Do a slower implementation for cases that include non-ASCII characters. O ICU devolve o
        // resultado inteiro, com o comprimento real (as duas passadas do C++ viram uma).
        Self::create16(&str_to_lower(span))
    }

    /// `StringImpl::convertToLowercaseWithoutLocaleStartingAtFailingIndex8Bit`.
    pub fn convert_to_lowercase_without_locale_starting_at_failing_index8_bit(
        self: &Rc<Self>,
        failing_index: u32,
    ) -> Rc<StringImpl> {
        debug_assert!(self.is_8bit());
        let span = self.span8();
        let failing_index = failing_index as usize;

        Self::create_uninitialized_internal_non_empty::<u8>(span.len(), |data8| {
            copy_characters(data8, &span[..failing_index]);

            for i in failing_index..span.len() {
                let character = span[i];
                if is_ascii(character) {
                    data8[i] = to_ascii_lower(character);
                } else {
                    // ASSERT(isLatin1(u_tolower(character)))
                    data8[i] = to_lower(character as u32) as u8;
                }
            }
        })
    }

    /// `StringImpl::convertToUppercaseWithoutLocale()`.
    pub fn convert_to_uppercase_without_locale(self: &Rc<Self>) -> Rc<StringImpl> {
        // This function could be optimized for no-op cases the way
        // convertToLowercaseWithoutLocale() is, but in empirical testing, few actual calls to
        // upper() are no-ops, so it wouldn't be worth the extra time for pre-scanning.

        if self.length() > MAX_LENGTH {
            // CRASH()
            panic!("StringImpl: comprimento acima de MaxLength");
        }

        // First scan the string for uppercase and non-ASCII characters:
        if self.is_8bit() {
            let span = self.span8();
            for (i, &character) in span.iter().enumerate() {
                if !is_ascii(character) || is_ascii_lower(character) {
                    return self.convert_to_uppercase_without_locale_starting_at_failing_index8_bit(i as u32);
                }
            }
            return Rc::clone(self);
        }
        self.convert_to_uppercase_without_locale_upconvert()
    }

    /// `StringImpl::convertToUppercaseWithoutLocaleStartingAtFailingIndex8Bit`.
    pub fn convert_to_uppercase_without_locale_starting_at_failing_index8_bit(
        self: &Rc<Self>,
        failing_index: u32,
    ) -> Rc<StringImpl> {
        debug_assert!(self.is_8bit());
        let span = self.span8();
        let failing_index = failing_index as usize;
        let mut destination = vec![0u8; span.len()];

        copy_characters(&mut destination, &span[..failing_index]);

        // Do a faster loop for the case where all the characters are ASCII.
        let mut ored: u32 = 0;
        for i in failing_index..span.len() {
            let character = span[i];
            ored |= character as u32;
            destination[i] = to_ascii_upper(character);
        }
        if (ored & !0x7F) == 0 {
            return Self::create(&destination);
        }

        // Do a slower implementation for cases that include non-ASCII Latin-1 characters.
        let mut number_sharp_s_characters: usize = 0;

        // There are two special cases.
        //  1. Some Latin-1 characters when converted to upper case are 16 bit characters.
        //  2. Lower case sharp-S converts to "SS" (two characters)
        for i in 0..span.len() {
            let character = span[i];
            if character as u16 == SMALL_LETTER_SHARP_S {
                number_sharp_s_characters += 1;
            }
            // ASSERT(u_toupper(character) <= 0xFFFF)
            let upper = to_upper(character as u32) as u16;
            if upper > 0xFF {
                // Since this upper-cased character does not fit in an 8-bit string, we need to
                // take the 16-bit path.
                return self.convert_to_uppercase_without_locale_upconvert();
            }
            destination[i] = upper as u8;
        }

        if number_sharp_s_characters == 0 {
            return Self::create(&destination);
        }

        // We have numberSSCharacters sharp-s characters, but none of the other special
        // characters.
        if self.length() as usize + number_sharp_s_characters > MAX_LENGTH as usize {
            return Rc::clone(self);
        }
        let mut destination = vec![0u8; span.len() + number_sharp_s_characters];

        let mut destination_index: usize = 0;
        for &character in span {
            if character as u16 == SMALL_LETTER_SHARP_S {
                destination[destination_index] = b'S';
                destination_index += 1;
                destination[destination_index] = b'S';
                destination_index += 1;
            } else {
                // ASSERT(isLatin1(u_toupper(character)))
                destination[destination_index] = to_upper(character as u32) as u8;
                destination_index += 1;
            }
        }

        Self::create(&destination)
    }

    /// `StringImpl::convertToUppercaseWithoutLocaleUpconvert()`.
    fn convert_to_uppercase_without_locale_upconvert(self: &Rc<Self>) -> Rc<StringImpl> {
        let upconverted_characters = self.upconverted_characters();
        self.convert_to_uppercase_without_locale16_bit(&upconverted_characters, 0)
    }

    /// `StringImpl::convertToUppercaseWithoutLocaleStartingAtFailingIndex16Bit`.
    pub fn convert_to_uppercase_without_locale_starting_at_failing_index16_bit(
        self: &Rc<Self>,
        failing_index: u32,
    ) -> Rc<StringImpl> {
        debug_assert!(!self.is_8bit());
        self.convert_to_uppercase_without_locale16_bit(self.span16(), failing_index)
    }

    /// `StringImpl::convertToUppercaseWithoutLocale16Bit`.
    fn convert_to_uppercase_without_locale16_bit(
        self: &Rc<Self>,
        source16: &[u16],
        failing_index: u32,
    ) -> Rc<StringImpl> {
        debug_assert!(source16.len() == self.length() as usize);
        let failing_index = failing_index as usize;
        let mut data16 = vec![0u16; source16.len()];

        // Characters before the failing index are already known to be ASCII that upper-casing
        // leaves alone, so they can be copied across without being tested or converted.
        copy_characters(&mut data16, &source16[..failing_index]);

        // Do a faster loop for the case where all the characters are ASCII.
        let mut ored: u32 = 0;
        for i in failing_index..source16.len() {
            let character = source16[i];
            ored |= character as u32;
            data16[i] = to_ascii_upper(character);
        }
        if (ored & !0x7F) == 0 {
            return Self::create16(&data16);
        }

        // Do a slower implementation for cases that include non-ASCII characters. O ICU devolve o
        // resultado inteiro, com o comprimento real (as duas passadas do C++ viram uma).
        Self::create16(&str_to_upper(source16))
    }

    // ---- caixa com locale ----------------------------------------------------------------

    /// `StringImpl::convertToLowercaseWithLocale(const AtomString&)`: `locale_identifier` é o
    /// `StringImpl` do átomo.
    pub fn convert_to_lowercase_with_locale(self: &Rc<Self>, locale_identifier: &StringImpl) -> Rc<StringImpl> {
        // Use the more-optimized code path most of the time.
        let locale: &str;
        if needs_turkish_casing_rules(locale_identifier) {
            // Passing in the hardcoded locale "tr" is more efficient than allocating memory just
            // to turn localeIdentifier into a C string, and we assume there is no difference
            // between the lowercasing for "tr" and "az" locales.
            // FIXME: Could optimize further by looking for the three sequences that have
            // locale-specific lowercasing.
            locale = "tr";
        } else if needs_lithuanian_casing_rules(locale_identifier) {
            locale = "lt";
        } else {
            return self.convert_to_lowercase_without_locale();
        }

        // FIXME: Could share more code with convertToLowercaseWithoutLocale.

        if self.length() > MAX_LENGTH {
            // CRASH()
            panic!("StringImpl: comprimento acima de MaxLength");
        }

        let source16 = self.upconverted_characters();
        // LOCALE: o `case_mapping` só tem a localidade raiz; `locale` ("tr" ou "lt") ainda não
        // altera o resultado até a tabela com as regras por localidade existir.
        let _ = locale;
        Self::create16(&str_to_lower(&source16))
    }

    /// `StringImpl::convertToUppercaseWithLocale(const AtomString&)`.
    pub fn convert_to_uppercase_with_locale(self: &Rc<Self>, locale_identifier: &StringImpl) -> Rc<StringImpl> {
        // Use the more-optimized code path most of the time.
        let locale: &str;
        if needs_turkish_casing_rules(locale_identifier) && self.find_character('i' as u16, 0) != NOT_FOUND {
            // Passing in the hardcoded locale "tr" is more efficient than allocating memory just
            // to turn localeIdentifier into a C string, and we assume there is no difference
            // between the uppercasing for "tr" and "az" locales.
            locale = "tr";
        } else if needs_greek_uppercasing_rules(locale_identifier) {
            locale = "el";
        } else if needs_lithuanian_casing_rules(locale_identifier) {
            locale = "lt";
        } else {
            return self.convert_to_uppercase_without_locale();
        }

        if self.length() > MAX_LENGTH {
            // CRASH()
            panic!("StringImpl: comprimento acima de MaxLength");
        }

        let source16 = self.upconverted_characters();
        // LOCALE: o `case_mapping` só tem a localidade raiz; `locale` ("tr", "el" ou "lt") ainda
        // não altera o resultado até a tabela com as regras por localidade existir.
        let _ = locale;
        Self::create16(&str_to_upper(&source16))
    }

    /// `StringImpl::foldCase()`.
    pub fn fold_case(self: &Rc<Self>) -> Rc<StringImpl> {
        if self.is_8bit() {
            let span = self.span8();
            let failing_index = match span.iter().position(|&character| !is_ascii(character) || is_ascii_upper(character))
            {
                Some(index) => index,
                // String was all ASCII and no uppercase, so just return as-is.
                None => return Rc::clone(self),
            };

            // SlowPath:
            let need16_bit_characters = span[failing_index..]
                .iter()
                .any(|&character| character == 0xB5 || character == 0xDF);

            if !need16_bit_characters {
                return Self::create_uninitialized_internal_non_empty::<u8>(span.len(), |data8| {
                    copy_characters(data8, &span[..failing_index]);
                    for i in failing_index..span.len() {
                        let character = span[i];
                        if is_ascii(character) {
                            data8[i] = to_ascii_lower(character);
                        } else {
                            // ASSERT(isLatin1(u_foldCase(character, U_FOLD_CASE_DEFAULT)))
                            data8[i] = icu_fold_case(character as u32) as u8;
                        }
                    }
                });
            }
        } else {
            // FIXME: Unclear why we use goto in the 8-bit case, and a different approach in the
            // 16-bit case.
            let mut no_upper = true;
            let mut ored: u32 = 0;
            let span = self.span16();
            for &character in span {
                if is_ascii_upper(character) {
                    no_upper = false;
                }
                ored |= character as u32;
            }
            if (ored & !0x7F) == 0 {
                if no_upper {
                    // String was all ASCII and no uppercase, so just return as-is.
                    return Rc::clone(self);
                }
                return Self::create_uninitialized_internal_non_empty::<u16>(span.len(), |data16| {
                    for i in 0..span.len() {
                        data16[i] = to_ascii_lower(span[i]);
                    }
                });
            }
        }

        if self.length() > MAX_LENGTH {
            // CRASH()
            panic!("StringImpl: comprimento acima de MaxLength");
        }

        let source16 = self.upconverted_characters();

        // u_strFoldCase(..., U_FOLD_CASE_DEFAULT): o ICU devolve o resultado inteiro (as duas
        // passadas do C++ viram uma).
        Self::create16(&str_fold_case(&source16))
    }

    // ---- caixa ASCII ---------------------------------------------------------------------

    /// `StringImpl::convertASCIICase<type, CharacterType>`.
    fn convert_ascii_case<T: CharType + AsciiChar>(
        case_convert_type: CaseConvertType,
        this: &Rc<StringImpl>,
        data: &[T],
    ) -> Rc<StringImpl> {
        let failing_index = data.iter().position(|&character| match case_convert_type {
            CaseConvertType::Lower => is_ascii_upper(character),
            CaseConvertType::Upper => is_ascii_lower(character),
        });
        let failing_index = match failing_index {
            Some(index) => index,
            None => return Rc::clone(this),
        };

        // SlowPath:
        Self::create_uninitialized_internal_non_empty::<T>(data.len(), |new_data| {
            copy_characters(new_data, &data[..failing_index]);
            for i in failing_index..data.len() {
                new_data[i] = match case_convert_type {
                    CaseConvertType::Lower => to_ascii_lower(data[i]),
                    CaseConvertType::Upper => to_ascii_upper(data[i]),
                };
            }
        })
    }

    /// `StringImpl::convertToASCIILowercase()`.
    pub fn convert_to_ascii_lowercase(self: &Rc<Self>) -> Rc<StringImpl> {
        if self.is_8bit() {
            return Self::convert_ascii_case(CaseConvertType::Lower, self, self.span8());
        }
        Self::convert_ascii_case(CaseConvertType::Lower, self, self.span16())
    }

    /// `StringImpl::convertToASCIIUppercase()`.
    pub fn convert_to_ascii_uppercase(self: &Rc<Self>) -> Rc<StringImpl> {
        if self.is_8bit() {
            return Self::convert_ascii_case(CaseConvertType::Upper, self, self.span8());
        }
        Self::convert_ascii_case(CaseConvertType::Upper, self, self.span16())
    }
}

// ---------------------------------------------------------------------------------------------
// copyCharacters
// ---------------------------------------------------------------------------------------------

/// `StringImpl::copyCharacters(span<CharacterType>, span<const CharacterType>)`.
pub fn copy_characters<T: Copy>(destination: &mut [T], source: &[T]) {
    destination[..source.len()].copy_from_slice(source);
}

/// `StringImpl::copyCharacters(span<char16_t>, span<const Latin1Character>)`: alarga.
pub fn copy_characters_widen(destination: &mut [u16], source: &[u8]) {
    for (to, from) in destination.iter_mut().zip(source) {
        *to = *from as u16;
    }
}

/// `StringImpl::copyCharacters(span<Latin1Character>, span<const char16_t>)`: o chamador garante
/// que tudo é Latin1; estreita por truncamento.
pub fn copy_characters_narrow(destination: &mut [u8], source: &[u16]) {
    for (to, from) in destination.iter_mut().zip(source) {
        *to = *from as u8;
    }
}

// ---------------------------------------------------------------------------------------------
// Funções livres inline do cabeçalho
// ---------------------------------------------------------------------------------------------

/// `WTF::containsOnly<isSpecialCharacter>(span)`.
pub fn contains_only<T: CharType>(characters: &[T], is_special_character: fn(u16) -> bool) -> bool {
    characters.iter().all(|c| is_special_character(c.to_u16()))
}

/// `WTF::reverseFindLineTerminator`.
pub fn reverse_find_line_terminator<T: CharType>(characters: &[T], start: usize) -> usize {
    if characters.is_empty() {
        return NOT_FOUND;
    }
    let mut start = start;
    if start >= characters.len() {
        start = characters.len() - 1;
    }
    let mut character: u32 = characters[start].into();
    while character != '\n' as u32 && character != '\r' as u32 {
        if start == 0 {
            return NOT_FOUND;
        }
        start -= 1;
        character = characters[start].into();
    }
    start
}

/// `WTF::codePointCompare`: ordem lexicográfica por unidade de código, depois por tamanho.
pub fn code_point_compare<T1: CharType, T2: CharType>(characters1: &[T1], characters2: &[T2]) -> Ordering {
    let common_length = std::cmp::min(characters1.len(), characters2.len());
    for position in 0..common_length {
        let a: u32 = characters1[position].into();
        let b: u32 = characters2[position].into();
        if a != b {
            return if a > b { Ordering::Greater } else { Ordering::Less };
        }
    }
    characters1.len().cmp(&characters2.len())
}

fn remove_characters_impl<T: CharType>(
    this: &Rc<StringImpl>,
    characters: &[T],
    find_match: &impl Fn(u16) -> bool,
) -> Rc<StringImpl> {
    // Supõe o caso comum de não remover nada.
    let first = match characters.iter().position(|c| find_match(c.to_u16())) {
        Some(index) => index,
        None => return Rc::clone(this),
    };

    let mut data: Vec<T> = characters[..first].to_vec();
    for &character in &characters[first..] {
        if !find_match(character.to_u16()) {
            data.push(character);
        }
    }

    StringImpl::adopt(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_constants_match_cpp() {
        assert_eq!(StringImpl::flag_is_8bit(), 4);
        assert_eq!(StringImpl::flag_is_atom(), 16);
        assert_eq!(StringImpl::flag_is_symbol(), 32);
        assert_eq!(StringImpl::mask_string_kind(), 48);
        assert_eq!(S_HASH_FLAG_NEVER_ATOMIZE, 64);
        assert_eq!(S_HASH_FLAG_DID_REPORT_COST, 8);
    }

    #[test]
    fn create_and_access() {
        let s = StringImpl::create(b"abc");
        assert!(s.is_8bit());
        assert_eq!(s.length(), 3);
        assert_eq!(s.span8(), b"abc");
        assert_eq!(s.char_at(1), b'b' as u16);
        let w = StringImpl::create16(&[0x61, 0x20AC]);
        assert!(!w.is_8bit());
        assert_eq!(w.char_at(1), 0x20AC);
        assert_eq!(w.span::<u16>().len(), 2);
    }

    #[test]
    fn empty_is_static_atom_with_hash() {
        let e = StringImpl::empty();
        assert!(e.is_atom() && e.is_static() && e.is_8bit() && e.is_empty());
        assert!(e.has_hash());
        assert_eq!(e.hash(), string_hasher::compute_hash_and_mask_top8_bits::<u8>(&[]));
        assert!(Rc::ptr_eq(&e, &StringImpl::create(b"")));
        assert_eq!(e.cost(), 0);
    }

    #[test]
    fn hash_is_lazy_and_stable() {
        let s = StringImpl::create(b"length");
        assert!(!s.has_hash());
        let h = s.hash();
        assert!(s.has_hash());
        assert_eq!(s.existing_hash(), h);
        assert_eq!(h, string_hasher::compute_hash_and_mask_top8_bits::<u8>(b"length"));
        // O hash não mexe nas flags.
        assert!(s.is_8bit() && !s.is_atom());
        // Latin1 em 16 bits tem o mesmo hash que em 8 bits.
        let w = StringImpl::create16(&[b'l' as u16, b'e' as u16, b'n' as u16, b'g' as u16, b't' as u16, b'h' as u16]);
        assert_eq!(w.hash(), h);
    }

    #[test]
    fn atom_and_cost_flags() {
        let s = StringImpl::create16(&[0x100, 0x101]);
        s.set_is_atom(true);
        assert!(s.is_atom());
        s.set_is_atom(false);
        assert!(!s.is_atom());
        assert_eq!(s.cost(), 4);
        assert_eq!(s.cost(), 0);
        assert!(s.can_become_atom());
        s.set_never_atomize();
        assert!(!s.can_become_atom());
    }

    #[test]
    fn utf8_and_substring_rules() {
        assert!(StringImpl::create_from_utf8(&[0xFF]).is_none());
        let s = StringImpl::create_from_utf8("aé".as_bytes()).unwrap();
        assert!(!s.is_8bit());
        assert_eq!(s.span16(), &[0x61, 0xE9]);
        let wide = StringImpl::create16(&[0x61, 0x62, 0x63, 0x100]);
        let sub = StringImpl::create_substring_sharing_impl(&wide, 0, 3);
        assert!(sub.is_8bit());
        let sub2 = StringImpl::create_substring_sharing_impl(&wide, 2, 2);
        assert!(!sub2.is_8bit());
    }

    #[test]
    fn find_remove_and_compare() {
        let s = StringImpl::create(b"a-b-c");
        assert_eq!(s.find_character('-' as u16, 0), 1);
        assert_eq!(s.find_character('-' as u16, 2), 3);
        assert_eq!(s.find_character('x' as u16, 0), NOT_FOUND);
        let r = s.remove_characters(|c| c == '-' as u16);
        assert_eq!(r.span8(), b"abc");
        let same = s.remove_characters(|c| c == 'z' as u16);
        assert!(Rc::ptr_eq(&s, &same));
        assert_eq!(reverse_find(b"a\nb\r".as_slice(), b'\n', usize::MAX), 1);
        assert_eq!(reverse_find_line_terminator(b"ab\ncd".as_slice(), usize::MAX), 2);
        assert_eq!(code_point_compare(b"ab".as_slice(), &[0x61u16, 0x62, 0x63]), Ordering::Less);
        assert_eq!(code_point_compare(&[0x100u16], b"z".as_slice()), Ordering::Greater);
    }

    #[test]
    fn valid_length_limits() {
        assert!(StringImpl::is_valid_length::<u8>(MAX_LENGTH as usize));
        assert!(!StringImpl::is_valid_length::<u8>(MAX_LENGTH as usize + 1));
    }

    #[test]
    fn replace_in_characters() {
        let r = StringImpl::create_by_replacing_in_characters8(b"a.b.", '.' as u16, '-' as u16, 1);
        assert_eq!(r.span8(), b"a-b-");
        let w = StringImpl::create_by_replacing_in_characters8(b"a.", '.' as u16, 0x20AC, 1);
        assert_eq!(w.span16(), &[0x61, 0x20AC]);
    }
}

/// Chave de identidade de um `UniquedStringImpl*` do C++: o `StringImpl` internado (átomo ou
/// símbolo) comparado e espalhado pelo endereço, como o C++ compara ponteiros.
#[derive(Clone, Debug)]
pub struct UniquedKey(pub std::rc::Rc<StringImpl>);

impl PartialEq for UniquedKey {
    fn eq(&self, other: &Self) -> bool {
        std::rc::Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for UniquedKey {}

impl std::hash::Hash for UniquedKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::ptr::hash(std::rc::Rc::as_ptr(&self.0), state);
    }
}

/// Testes dos caminhos ASCII do `StringImpl.cpp` (os que não consultam o ICU).
#[cfg(test)]
mod cpp_tests {
    use super::*;

    #[test]
    fn lowercase_ascii_paths() {
        let upper8 = StringImpl::create(b"HeLLo-1");
        let lower8 = upper8.convert_to_lowercase_without_locale();
        assert!(lower8.is_8bit());
        assert_eq!(lower8.span8(), b"hello-1");

        // Sem maiúsculas devolve a própria string.
        let same = lower8.convert_to_lowercase_without_locale();
        assert!(Rc::ptr_eq(&lower8, &same));

        let upper16 = StringImpl::create16(&['A' as u16, 'b' as u16, 'C' as u16]);
        let lower16 = upper16.convert_to_lowercase_without_locale();
        assert!(!lower16.is_8bit());
        assert_eq!(lower16.span16(), &['a' as u16, 'b' as u16, 'c' as u16]);
        let same16 = lower16.convert_to_lowercase_without_locale();
        assert!(Rc::ptr_eq(&lower16, &same16));
    }

    #[test]
    fn uppercase_ascii_paths() {
        let lower8 = StringImpl::create(b"abC-d");
        let upper8 = lower8.convert_to_uppercase_without_locale();
        assert_eq!(upper8.span8(), b"ABC-D");
        let same = upper8.convert_to_uppercase_without_locale();
        assert!(Rc::ptr_eq(&upper8, &same));

        let lower16 = StringImpl::create16(&['a' as u16, 'b' as u16]);
        let upper16 = lower16.convert_to_uppercase_without_locale();
        assert_eq!(upper16.span16(), &['A' as u16, 'B' as u16]);
    }

    #[test]
    fn locale_selection_falls_back_to_plain_conversion() {
        let abc = StringImpl::create(b"ABC");
        let en = StringImpl::create(b"en-US");
        assert_eq!(abc.convert_to_lowercase_with_locale(&en).span8(), b"abc");
        let low = StringImpl::create(b"abc");
        assert_eq!(low.convert_to_uppercase_with_locale(&en).span8(), b"ABC");
        assert!(needs_turkish_casing_rules(&StringImpl::create(b"TR")));
        assert!(needs_turkish_casing_rules(&StringImpl::create(b"az-Latn")));
        assert!(!needs_turkish_casing_rules(&StringImpl::create(b"trx")));
        assert!(needs_greek_uppercasing_rules(&StringImpl::create(b"el-GR")));
        assert!(needs_lithuanian_casing_rules(&StringImpl::create(b"lt")));
        assert!(!needs_lithuanian_casing_rules(&StringImpl::create(b"l")));
    }

    #[test]
    fn fold_case_ascii_paths() {
        let s = StringImpl::create(b"AbC");
        assert_eq!(s.fold_case().span8(), b"abc");
        let lower = StringImpl::create(b"abc");
        assert!(Rc::ptr_eq(&lower, &lower.fold_case()));
        let wide = StringImpl::create16(&['A' as u16, 'b' as u16]);
        assert_eq!(wide.fold_case().span16(), &['a' as u16, 'b' as u16]);
    }

    #[test]
    fn ascii_case_conversion() {
        let s = StringImpl::create(b"aBc\xE9");
        let lower = s.convert_to_ascii_lowercase();
        assert_eq!(lower.span8(), b"abc\xE9");
        let upper = s.convert_to_ascii_uppercase();
        assert_eq!(upper.span8(), b"ABC\xE9");
        assert!(Rc::ptr_eq(&upper, &upper.convert_to_ascii_uppercase()));
        let wide = StringImpl::create16(&['x' as u16, 0x20AC]);
        assert_eq!(wide.convert_to_ascii_uppercase().span16(), &['X' as u16, 0x20AC]);
    }

    #[test]
    fn substring_and_code_point_at() {
        let s = StringImpl::create(b"abcdef");
        assert!(Rc::ptr_eq(&s, &s.substring(0, MAX_LENGTH)));
        assert_eq!(s.substring(2, 3).span8(), b"cde");
        assert_eq!(s.substring(4, MAX_LENGTH).span8(), b"ef");
        assert!(s.substring(6, 1).is_empty());

        let pair = StringImpl::create16(&[0xD83D, 0xDE00, 0x61, 0xD83D]);
        assert_eq!(pair.code_point_at(0), 0x1F600);
        assert_eq!(pair.code_point_at(1), 0xDE00);
        assert_eq!(pair.code_point_at(2), 0x61);
        assert_eq!(pair.code_point_at(3), 0xD83D);
        assert_eq!(s.code_point_at(1), 'b' as u32);
    }

    #[test]
    fn uninitialized_reallocate_and_static() {
        let s = StringImpl::create_uninitialized::<u8>(3, |data| data.copy_from_slice(b"xyz"));
        assert_eq!(s.span8(), b"xyz");
        assert!(StringImpl::create_uninitialized::<u16>(0, |_| {}).is_empty());

        let grown = StringImpl::reallocate::<u8>(s, 5, |data| {
            data[3] = b'!';
            data[4] = b'?';
        });
        assert_eq!(grown.span8(), b"xyz!?");
        let shrunk = StringImpl::reallocate::<u8>(grown, 2, |_| {});
        assert_eq!(shrunk.span8(), b"xy");
        assert!(StringImpl::reallocate::<u8>(shrunk, 0, |_| {}).is_empty());

        let st = StringImpl::create_static_string_impl8(b"abc");
        assert!(st.is_static() && st.has_hash() && st.is_8bit());
        let st16 = StringImpl::create_static_string_impl16(&['a' as u16, 'b' as u16]);
        assert!(st16.is_static() && st16.has_hash() && st16.is_8bit());
        let copy = StringImpl::create_without_copying_non_empty(b"q".as_slice());
        assert_eq!(copy.span8(), b"q");
    }
}

// ---------------------------------------------------------------------------------------------
// StringImpl.cpp (linha 841 até o fim) e a lógica de StringCommon.h / StringView.h que ele chama
// ---------------------------------------------------------------------------------------------

use crate::wtf::ascii_ctype::is_unicode_compatible_ascii_whitespace;
use crate::wtf::text::conversion_mode::ConversionMode;
use crate::wtf::unicode::utf8_conversion::{
    convert_latin1_to_utf8, convert_replacing_invalid_sequences_utf16_to_utf8, convert_utf16_to_utf8,
    ConversionResultCode,
};


/// `isLatin1(char16_t)`.
fn is_latin1(character: u16) -> bool {
    character <= 0xFF
}

/// `isValidCapacityForVector<char8_t>(capacity)`: `capacity <= (UINT_MAX >> 1) / sizeof(T)`.
fn is_valid_capacity_for_vector_u8(capacity: usize) -> bool {
    capacity <= (u32::MAX >> 1) as usize
}

/// `copyCharacters` entre larguras quaisquer: iguais copiam, 8 para 16 alarga, 16 para 8 estreita
/// (o chamador garantiu Latin1). Cobre as quatro sobrecargas de `StringImpl::copyCharacters`.
pub(crate) fn copy_characters_convert<D: CharType, T: CharType>(destination: &mut [D], source: &[T]) {
    debug_assert!(destination.len() >= source.len());
    for (to, from) in destination.iter_mut().zip(source) {
        *to = D::from_u16(from.to_u16());
    }
}

/// O `findWithHash` do `StringImpl::find(std::span<const Latin1Character>, size_t)`: hash de
/// Rabin-Karp de base 31 (aritmética `unsigned`, com volta).
fn find_with_hash<S: CharType>(search_characters: &[S], match_string: &[u8], delta: usize, start: usize) -> usize {
    // Rabin-Karp style rolling hash with base 31.
    const BASE: u32 = 31;
    let mut search_hash: u32 = 0;
    let mut match_hash: u32 = 0;
    let mut base_power: u32 = 1; // base^(matchString.size()-1)
    for i in 0..match_string.len() {
        search_hash = search_hash.wrapping_mul(BASE).wrapping_add(unit(search_characters[i]));
        match_hash = match_hash.wrapping_mul(BASE).wrapping_add(match_string[i] as u32);
        if i != 0 {
            base_power = base_power.wrapping_mul(BASE);
        }
    }

    for i in 0..=delta {
        if search_hash == match_hash && equal_prefix(&search_characters[i..i + match_string.len()], match_string) {
            return start + i;
        }
        if i < delta {
            search_hash = search_hash.wrapping_sub(unit(search_characters[i]).wrapping_mul(base_power));
            search_hash = search_hash
                .wrapping_mul(BASE)
                .wrapping_add(unit(search_characters[i + match_string.len()]));
        }
    }
    NOT_FOUND
}

/// O `equalInner(const StringImpl&, unsigned, StringView)` do `.cpp` (as verificações de faixa
/// são em `unsigned`, como no C++).
fn equal_inner_view(string: &StringImpl, start: u32, match_string: StringView) -> bool {
    let match_length = match_string.length();
    if start > string.length() {
        return false;
    }
    if match_length > string.length() {
        return false;
    }
    if match_length.wrapping_add(start) > string.length() {
        return false;
    }

    with_views!(StringView::from(string), match_string, |s, m| equal_prefix(&s[start as usize..], m))
}

/// O `equalInner(const StringImpl&, unsigned, std::span<const char>)` do `.cpp`.
fn equal_inner_bytes(string: &StringImpl, start: u32, match_string: &[u8]) -> bool {
    debug_assert!(match_string.len() <= string.length() as usize);
    debug_assert!(start as usize + match_string.len() <= string.length() as usize);

    with_view!(StringView::from(string), |s| equal_prefix(&s[start as usize..], match_string))
}

/// O gerador do corpo de `replace(...)`: copia, para cada ocorrência, o trecho de origem antes
/// dela e a substituição. `find_next(start)` é o `find(pattern, srcSegmentStart)` do C++; o tipo de
/// saída `O` é 8 bits só quando origem e substituição são de 8 bits (casos 1 a 4 do C++).
fn replace_segments_build<S: CharType, R: CharType, O: CharType>(
    source: &[S],
    replacement: &[R],
    new_size: usize,
    pattern_length: usize,
    find_next: impl Fn(usize) -> usize,
) -> Rc<StringImpl> {
    StringImpl::create_uninitialized::<O>(new_size, |data| {
        // Construct the new data.
        let mut src_segment_start = 0usize;
        let mut dst_offset = 0usize;

        loop {
            let src_segment_end = find_next(src_segment_start);
            if src_segment_end == NOT_FOUND {
                break;
            }
            let src_segment_length = src_segment_end - src_segment_start;
            copy_characters_convert(
                &mut data[dst_offset..],
                &source[src_segment_start..src_segment_start + src_segment_length],
            );
            dst_offset += src_segment_length;
            copy_characters_convert(&mut data[dst_offset..], replacement);
            dst_offset += replacement.len();
            src_segment_start = src_segment_end + pattern_length;
        }

        copy_characters_convert(&mut data[dst_offset..], &source[src_segment_start..]);
    })
}

/// O corpo de `replace(size_t, size_t, StringView)` depois das verificações.
fn replace_range_build<S: CharType, R: CharType, O: CharType>(
    source: &[S],
    insert: &[R],
    position: usize,
    length_to_replace: usize,
    new_length: usize,
) -> Rc<StringImpl> {
    StringImpl::create_uninitialized::<O>(new_length, |data| {
        copy_characters_convert(data, &source[..position]);
        copy_characters_convert(&mut data[position..], insert);
        copy_characters_convert(&mut data[position + insert.len()..], &source[position + length_to_replace..]);
    })
}

/// `isUnicodeWhitespace` do cabeçalho.
pub fn is_unicode_whitespace(character: u16) -> bool {
    if is_ascii(character) {
        is_unicode_compatible_ascii_whitespace(character)
    } else {
        u_is_u_white_space(character)
    }
}

/// `u_isUWhiteSpace` do ICU restrito ao BMP e fora do ASCII: a propriedade `White_Space` do
/// `PropList.txt` (U+0085, U+00A0, U+1680, U+2000 a U+200A, U+2028, U+2029, U+202F, U+205F,
/// U+3000). O ASCII é tratado antes, em `isUnicodeWhitespace`.
fn u_is_u_white_space(character: u16) -> bool {
    matches!(
        character,
        0x0009..=0x000D
            | 0x0020
            | 0x0085
            | 0x00A0
            | 0x1680
            | 0x2000..=0x200A
            | 0x2028
            | 0x2029
            | 0x202F
            | 0x205F
            | 0x3000
    )
}

/// `equal(const StringImpl&, const StringImpl&)`: o hash já calculado das duas, se diferente, basta.
pub fn equal(a: &StringImpl, b: &StringImpl) -> bool {
    let a_hash = a.raw_hash();
    let b_hash = b.raw_hash();
    if a_hash != b_hash && a_hash != 0 && b_hash != 0 {
        return false;
    }
    crate::wtf::text::string_view::equal(StringView::from(a), StringView::from(b))
}

/// `equal(const StringImpl*, const StringImpl*)`: `equalCommon` sobre ponteiros.
pub fn equal_nullable(a: Option<&StringImpl>, b: Option<&StringImpl>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => std::ptr::eq(x, y) || equal(x, y),
        _ => false,
    }
}

/// `equalInternal`, as duas sobrecargas `equal(const StringImpl*, std::span<const Latin1Character>)`
/// e `equal(const StringImpl*, std::span<const char16_t>)`. O `None` do `b` é a fatia de dados nulos.
pub fn equal_span<T: CharType>(a: Option<&StringImpl>, b: Option<&[T]>) -> bool {
    let a = match a {
        None => return b.is_none(),
        Some(a) => a,
    };
    let b = match b {
        None => return false,
        Some(b) => b,
    };

    if a.length() as usize != b.len() {
        return false;
    }
    if b.is_empty() {
        return true;
    }
    with_view!(StringView::from(a), |s| equal_prefix(s, b))
}

/// `equalIgnoringNullity(StringImpl*, StringImpl*)`.
pub fn equal_ignoring_nullity(a: Option<&StringImpl>, b: Option<&StringImpl>) -> bool {
    if a.is_none() && b.is_some_and(|b| b.length() == 0) {
        return true;
    }
    if b.is_none() && a.is_some_and(|a| a.length() == 0) {
        return true;
    }
    equal_nullable(a, b)
}

/// `equalIgnoringNullity(std::span<const char16_t>, StringImpl*)`.
pub fn equal_ignoring_nullity_span16(a: &[u16], b: Option<&StringImpl>) -> bool {
    let b = match b {
        None => return a.is_empty(),
        Some(b) => b,
    };
    if a.len() != b.length() as usize {
        return false;
    }
    if b.is_8bit() {
        for (a_character, b_character) in a.iter().zip(b.span8()) {
            if *a_character != *b_character as u16 {
                return false;
            }
        }
        return true;
    }
    a == b.span16()
}

/// `equalIgnoringASCIICase(const StringImpl&, const StringImpl&)` (inline do cabeçalho, via
/// `equalIgnoringASCIICaseCommon`).
pub fn equal_ignoring_ascii_case(a: &StringImpl, b: &StringImpl) -> bool {
    crate::wtf::text::string_view::equal_ignoring_ascii_case(StringView::from(a), StringView::from(b))
}

/// `equalIgnoringASCIICase(const StringImpl*, const StringImpl*)`.
pub fn equal_ignoring_ascii_case_nullable(a: Option<&StringImpl>, b: Option<&StringImpl>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => std::ptr::eq(x, y) || equal_ignoring_ascii_case(x, y),
        _ => false,
    }
}

/// `equalIgnoringASCIICaseNonNull(const StringImpl*, const StringImpl*)`: as pontas são
/// referências, então já são as não nulas.
pub use self::equal_ignoring_ascii_case as equal_ignoring_ascii_case_non_null;

impl StringImpl {
    // ---- trim / simplifyWhiteSpace -------------------------------------------------------

    /// `trimMatchedCharacters<CharacterType>(predicate)`.
    fn trim_matched_characters<T: CharType>(self: &Rc<Self>, predicate: &impl Fn(u16) -> bool) -> Rc<StringImpl> {
        let span = self.span::<T>();
        if span.is_empty() {
            return Rc::clone(self);
        }

        let mut start = 0usize;
        let mut end = span.len() - 1;

        // skip matched characters from start
        while start <= end && predicate(span[start].to_u16()) {
            start += 1;
        }

        // only matched characters
        if start > end {
            return Self::empty();
        }

        // skip matched characters from end
        while end != 0 && predicate(span[end].to_u16()) {
            end -= 1;
        }

        if start == 0 && end == span.len() - 1 {
            return Rc::clone(self);
        }
        T::create(&span[start..end + 1])
    }

    /// `StringImpl::trim(CodeUnitMatchFunction)`.
    pub fn trim(self: &Rc<Self>, predicate: impl Fn(u16) -> bool) -> Rc<StringImpl> {
        if self.is_8bit() {
            return self.trim_matched_characters::<u8>(&predicate);
        }
        self.trim_matched_characters::<u16>(&predicate)
    }

    /// `simplifyMatchedCharactersToSpace<CharacterType>(predicate)`.
    fn simplify_matched_characters_to_space<T: CharType>(
        self: &Rc<Self>,
        predicate: &impl Fn(u16) -> bool,
    ) -> Rc<StringImpl> {
        let m_length = self.length() as usize;
        let mut to: Vec<T> = vec![T::from_u16(0); m_length];

        let from = self.span::<T>();
        let mut from_index = 0usize;
        let mut outc = 0usize;
        let mut changed_to_space = false;

        loop {
            while from_index < from.len() && predicate(from[from_index].to_u16()) {
                if from[from_index].to_u16() != ' ' as u16 {
                    changed_to_space = true;
                }
                from_index += 1;
            }
            while from_index < from.len() && !predicate(from[from_index].to_u16()) {
                to[outc] = from[from_index];
                outc += 1;
                from_index += 1;
            }
            if from_index < from.len() {
                to[outc] = T::from_u16(' ' as u16);
                outc += 1;
            } else {
                break;
            }
        }

        if outc != 0 && to[outc - 1].to_u16() == ' ' as u16 {
            outc -= 1;
        }

        if outc == m_length && !changed_to_space {
            return Rc::clone(self);
        }

        to.truncate(outc);

        Self::adopt(to)
    }

    /// `StringImpl::simplifyWhiteSpace(CodeUnitMatchFunction)`.
    pub fn simplify_white_space(self: &Rc<Self>, is_white_space: impl Fn(u16) -> bool) -> Rc<StringImpl> {
        if self.is_8bit() {
            return self.simplify_matched_characters_to_space::<u8>(&is_white_space);
        }
        self.simplify_matched_characters_to_space::<u16>(&is_white_space)
    }

    // ---- find / reverseFind --------------------------------------------------------------

    /// `StringImpl::find(std::span<const Latin1Character>, size_t start)`.
    pub fn find_latin1_span(&self, match_string: &[u8], start: usize) -> usize {
        debug_assert!(!match_string.is_empty());
        debug_assert!(Self::is_valid_length::<u8>(match_string.len()));

        // Check start & matchLength are in range.
        if start > self.length() as usize {
            return NOT_FOUND;
        }
        let search_length = self.length() as usize - start;
        if match_string.len() > search_length {
            return NOT_FOUND;
        }
        // delta is the number of additional times to test; delta == 0 means test only once.
        let delta = search_length - match_string.len();

        // Optimization: keep a running hash of the strings,
        // only call equal if the hashes match.
        with_view!(StringView::from(self), |s| find_with_hash(&s[start..], match_string, delta, start))
    }

    /// `StringImpl::reverseFind(std::span<const Latin1Character>, size_t start)`.
    pub fn reverse_find_latin1_span(&self, match_string: &[u8], start: usize) -> usize {
        debug_assert!(!match_string.is_empty());

        if match_string.len() > self.length() as usize {
            return NOT_FOUND;
        }

        with_view!(StringView::from(self), |s| reverse_find_inner(s, match_string, start))
    }

    /// `StringImpl::find(StringView)`.
    pub fn find_view(&self, match_string: StringView) -> usize {
        // Check for null string to match against
        if match_string.is_null() {
            return NOT_FOUND;
        }
        let match_length = match_string.length() as usize;

        // Optimization 1: fast case for strings of length 1.
        if match_length == 1 {
            return self.find_character(match_string.code_unit_at(0), 0);
        }

        // Check matchLength is in range.
        if match_length > self.length() as usize {
            return NOT_FOUND;
        }

        // Check for empty string to match against
        if match_length == 0 {
            return 0;
        }

        with_views!(StringView::from(self), match_string, |s, m| find_inner(s, m, 0))
    }

    /// `StringImpl::find(StringView, size_t start)`. O `start` vira `unsigned` ao chamar o
    /// `findCommon`, como no C++.
    pub fn find_view_from(&self, match_string: StringView, start: usize) -> usize {
        // Check for null or empty string to match against
        if match_string.is_null() {
            return NOT_FOUND;
        }
        StringView::from(self).find(match_string, start as u32)
    }

    /// `StringImpl::findIgnoringASCIICase(StringView)`.
    pub fn find_ignoring_ascii_case_view(&self, match_string: StringView) -> usize {
        self.find_ignoring_ascii_case_view_from(match_string, 0)
    }

    /// `StringImpl::findIgnoringASCIICase(StringView, size_t start)`.
    pub fn find_ignoring_ascii_case_view_from(&self, match_string: StringView, start: usize) -> usize {
        if match_string.is_null() {
            return NOT_FOUND;
        }
        StringView::from(self).find_ignoring_ascii_case(match_string, start as u32)
    }

    /// `StringImpl::reverseFind(char16_t, size_t start)`; o `start` padrão do C++ é `MaxLength`.
    pub fn reverse_find_character(&self, character: u16, start: usize) -> usize {
        if self.is_8bit() {
            return reverse_find_8_char16(self.span8(), character, start);
        }
        reverse_find(self.span16(), character, start)
    }

    /// `StringImpl::reverseFind(StringView, size_t start)`; o `start` padrão do C++ é `MaxLength`.
    pub fn reverse_find_view(&self, match_string: StringView, start: usize) -> usize {
        // Check for null or empty string to match against
        if match_string.is_null() {
            return NOT_FOUND;
        }
        let match_length = match_string.length() as usize;
        if match_length == 0 {
            return std::cmp::min(start, self.length() as usize);
        }

        // Optimization 1: fast case for strings of length 1.
        if match_length == 1 {
            return self.reverse_find_character(match_string.code_unit_at(0), start);
        }

        // Check start & matchLength are in range.
        if match_length > self.length() as usize {
            return NOT_FOUND;
        }

        with_views!(StringView::from(self), match_string, |s, m| reverse_find_inner(s, m, start))
    }

    // ---- startsWith / endsWith -----------------------------------------------------------

    /// `StringImpl::startsWith(StringView)`.
    pub fn starts_with_view(&self, string: StringView) -> bool {
        if string.is_null() {
            return true;
        }
        StringView::from(self).starts_with(string)
    }

    /// `StringImpl::startsWithIgnoringASCIICase(StringView)`.
    pub fn starts_with_ignoring_ascii_case_view(&self, prefix: StringView) -> bool {
        if prefix.is_null() {
            return false;
        }
        StringView::from(self).starts_with_ignoring_ascii_case(prefix)
    }

    /// `StringImpl::startsWith(char16_t)`.
    pub fn starts_with_character(&self, character: u16) -> bool {
        self.length() != 0 && self.char_at(0) == character
    }

    /// `StringImpl::startsWith(std::span<const char>)`.
    pub fn starts_with_bytes(&self, match_string: &[u8]) -> bool {
        match_string.len() <= self.length() as usize && equal_inner_bytes(self, 0, match_string)
    }

    /// `StringImpl::hasInfixStartingAt(StringView, size_t start)`. O `start` vira `unsigned` no
    /// `equalInner`, como no C++.
    pub fn has_infix_starting_at(&self, match_string: StringView, start: usize) -> bool {
        equal_inner_view(self, start as u32, match_string)
    }

    /// `StringImpl::endsWith(StringView)`.
    pub fn ends_with_view(&self, suffix: StringView) -> bool {
        if suffix.is_null() {
            return false;
        }
        StringView::from(self).ends_with(suffix)
    }

    /// `StringImpl::endsWithIgnoringASCIICase(StringView)`.
    pub fn ends_with_ignoring_ascii_case_view(&self, suffix: StringView) -> bool {
        if suffix.is_null() {
            return false;
        }
        StringView::from(self).ends_with_ignoring_ascii_case(suffix)
    }

    /// `StringImpl::endsWith(char16_t)`.
    pub fn ends_with_character(&self, character: u16) -> bool {
        self.length() != 0 && self.char_at(self.length() - 1) == character
    }

    /// `StringImpl::endsWith(std::span<const char>)`.
    pub fn ends_with_bytes(&self, match_string: &[u8]) -> bool {
        match_string.len() <= self.length() as usize
            && equal_inner_bytes(self, (self.length() as usize - match_string.len()) as u32, match_string)
    }

    /// `StringImpl::hasInfixEndingAt(StringView, size_t end)`.
    pub fn has_infix_ending_at(&self, match_string: StringView, end: usize) -> bool {
        let match_length = match_string.length() as usize;
        end >= match_length && equal_inner_view(self, (end - match_length) as u32, match_string)
    }

    // ---- replace -------------------------------------------------------------------------

    /// `StringImpl::replace(char16_t target, char16_t replacement)`.
    pub fn replace_character(self: &Rc<Self>, target: u16, replacement: u16) -> Rc<StringImpl> {
        if target == replacement {
            return Rc::clone(self);
        }

        // find() devolve notFound para um alvo que não é Latin1 numa string de 8 bits, então o
        // caso de 8 bits com alvo de 16 bits também sai aqui.
        let i = self.find_character(target, 0);
        if i == NOT_FOUND {
            return Rc::clone(self);
        }
        if self.is_8bit() {
            return Self::create_by_replacing_in_characters8(self.span8(), target, replacement, i);
        }
        Self::create_by_replacing_in_characters16(self.span16(), target, replacement, i)
    }

    /// `StringImpl::replace(size_t position, size_t lengthToReplace, StringView)`.
    pub fn replace_range(
        self: &Rc<Self>,
        position: usize,
        length_to_replace: usize,
        string: StringView,
    ) -> Rc<StringImpl> {
        let length = self.length() as usize;
        let position = std::cmp::min(position, length);
        let length_to_replace = std::cmp::min(length_to_replace, length - position);
        // A `StringView` nula tem comprimento 0 e conta como de 8 bits no teste abaixo.
        let length_to_insert = string.length() as usize;
        if length_to_replace == 0 && length_to_insert == 0 {
            return Rc::clone(self);
        }

        if (length - length_to_replace) >= (MAX_LENGTH as usize - length_to_insert) {
            // CRASH()
            panic!("StringImpl::replace: comprimento inválido");
        }

        let new_length = length - length_to_replace + length_to_insert;
        let both_8bit = self.is_8bit() && string.is_8bit();
        with_views!(StringView::from(&**self), string, |source, insert| {
            if both_8bit {
                replace_range_build::<_, _, u8>(source, insert, position, length_to_replace, new_length)
            } else {
                replace_range_build::<_, _, u16>(source, insert, position, length_to_replace, new_length)
            }
        })
    }

    /// `StringImpl::replace(char16_t pattern, StringView replacement)`.
    pub fn replace_character_with_view(self: &Rc<Self>, pattern: u16, replacement: StringView) -> Rc<StringImpl> {
        match replacement.data() {
            StringViewData::Null => Rc::clone(self),
            StringViewData::Latin1(replacement) => self.replace_character_with_span(pattern, replacement),
            StringViewData::Utf16(replacement) => self.replace_character_with_span(pattern, replacement),
        }
    }

    /// `StringImpl::replace(char16_t, std::span<const Latin1Character>)` e
    /// `StringImpl::replace(char16_t, std::span<const char16_t>)`: o mesmo corpo para as duas
    /// larguras de substituição.
    pub fn replace_character_with_span<R: CharType>(self: &Rc<Self>, pattern: u16, replacement: &[R]) -> Rc<StringImpl> {
        let mut src_segment_start = 0usize;
        let mut match_count = 0usize;

        // Count the matches.
        loop {
            src_segment_start = self.find_character(pattern, src_segment_start);
            if src_segment_start == NOT_FOUND {
                break;
            }
            match_count += 1;
            src_segment_start += 1;
        }

        // If we have 0 matches then we don't have to do any more work.
        if match_count == 0 {
            return Rc::clone(self);
        }

        if !replacement.is_empty() && match_count > MAX_LENGTH as usize / replacement.len() {
            // CRASH()
            panic!("StringImpl::replace: comprimento inválido");
        }

        let replace_size = match_count * replacement.len();
        let mut new_size = self.length() as usize - match_count;
        if new_size >= (MAX_LENGTH as usize - replace_size) {
            // CRASH()
            panic!("StringImpl::replace: comprimento inválido");
        }

        new_size += replace_size;

        let both_8bit = self.is_8bit() && R::SIZE == 1;
        let find_next = |start: usize| self.find_character(pattern, start);
        with_view!(StringView::from(&**self), |source| {
            if both_8bit {
                replace_segments_build::<_, _, u8>(source, replacement, new_size, 1, find_next)
            } else {
                replace_segments_build::<_, _, u16>(source, replacement, new_size, 1, find_next)
            }
        })
    }

    /// `StringImpl::replace(StringView pattern, StringView replacement)`.
    pub fn replace_view(self: &Rc<Self>, pattern: StringView, replacement: StringView) -> Rc<StringImpl> {
        if pattern.is_null() || replacement.is_null() {
            return Rc::clone(self);
        }

        let pattern_length = pattern.length();
        if pattern_length == 0 {
            return Rc::clone(self);
        }

        let rep_str_length = replacement.length();
        let mut src_segment_start = 0usize;
        let mut match_count: u32 = 0;

        // Count the matches.
        loop {
            src_segment_start = self.find_view_from(pattern, src_segment_start);
            if src_segment_start == NOT_FOUND {
                break;
            }
            match_count = match_count.wrapping_add(1);
            src_segment_start += pattern_length as usize;
        }

        // If we have 0 matches, we don't have to do any more work
        if match_count == 0 {
            return Rc::clone(self);
        }

        let mut new_size: u32 = self.length().wrapping_sub(match_count.wrapping_mul(pattern_length));
        if rep_str_length != 0 && match_count > MAX_LENGTH / rep_str_length {
            // CRASH()
            panic!("StringImpl::replace: comprimento inválido");
        }

        if new_size > (MAX_LENGTH - match_count.wrapping_mul(rep_str_length)) {
            // CRASH()
            panic!("StringImpl::replace: comprimento inválido");
        }

        new_size = new_size.wrapping_add(match_count.wrapping_mul(rep_str_length));

        // There are 4 cases:
        // 1. This and replacement are both 8 bit.
        // 2. This and replacement are both 16 bit.
        // 3. This is 8 bit and replacement is 16 bit.
        // 4. This is 16 bit and replacement is 8 bit.
        let both_8bit = self.is_8bit() && replacement.is_8bit();
        let find_next = |start: usize| self.find_view_from(pattern, start);
        with_views!(StringView::from(&**self), replacement, |source, replacement_span| {
            if both_8bit {
                replace_segments_build::<_, _, u8>(
                    source,
                    replacement_span,
                    new_size as usize,
                    pattern_length as usize,
                    find_next,
                )
            } else {
                replace_segments_build::<_, _, u16>(
                    source,
                    replacement_span,
                    new_size as usize,
                    pattern_length as usize,
                    find_next,
                )
            }
        })
    }

    // ---- direção de escrita --------------------------------------------------------------

    /// `StringImpl::sizeInBytes()`.
    pub fn size_in_bytes(&self) -> usize {
        // FIXME: support substrings
        let mut size = self.length() as usize;
        if !self.is_8bit() {
            size *= 2;
        }
        size + CPP_SIZE_OF_STRING_IMPL
    }

    // ---- UTF-8 ---------------------------------------------------------------------------

    /// `StringImpl::utf8LengthFromUTF16` (`simdutf::utf8_length_from_utf16le`): cada unidade
    /// substituta conta 2 bytes, de modo que um par vale 4.
    pub fn utf8_length_from_utf16(characters: &[u16]) -> usize {
        characters
            .iter()
            .map(|&character| {
                if character <= 0x7F {
                    1
                } else if character <= 0x7FF {
                    2
                } else if (character & 0xF800) != 0xD800 {
                    3
                } else {
                    2
                }
            })
            .sum()
    }

    /// `StringImpl::tryConvertUTF16ToUTF8` (`simdutf::convert_utf16le_to_utf8_with_errors`): o
    /// número de bytes escritos, ou `notFound` se a origem tem substituto solto ou não cabe.
    pub fn try_convert_utf16_to_utf8(source: &[u16], destination: &mut [u8]) -> usize {
        let result = convert_utf16_to_utf8(source, destination);
        if result.code == ConversionResultCode::Success {
            return result.buffer.len();
        }
        NOT_FOUND
    }

    /// `StringImpl::utf8ForCharactersIntoBuffer`: `buffer` tem `span.len() * 3` bytes.
    pub fn utf8_for_characters_into_buffer(
        span: &[u16],
        mode: ConversionMode,
        buffer: &mut [u8],
    ) -> Result<usize, UTF8ConversionError> {
        debug_assert!(buffer.len() == span.len() * 3);

        let simd_converted_size = Self::try_convert_utf16_to_utf8(span, buffer);
        if simd_converted_size != NOT_FOUND {
            return Ok(simd_converted_size);
        }

        let result = match mode {
            ConversionMode::StrictConversion => convert_utf16_to_utf8(span, buffer),
            // FIXME: Lenient is exactly the same as "replacing unpaired surrogates with FFFD"; we
            // don't need both.
            ConversionMode::StrictConversionReplacingUnpairedSurrogatesWithFFFD | ConversionMode::LenientConversion => {
                convert_replacing_invalid_sequences_utf16_to_utf8(span, buffer)
            }
        };
        if result.code == ConversionResultCode::SourceInvalid {
            return Err(UTF8ConversionError::Invalid);
        }
        Ok(result.buffer.len())
    }

    /// `StringImpl::tryGetUTF8ForCharacters(function, std::span<const Latin1Character>)`.
    pub fn try_get_utf8_for_characters8<R>(
        function: impl FnOnce(&[u8]) -> R,
        characters: &[u8],
    ) -> Result<R, UTF8ConversionError> {
        if characters.is_empty() {
            return Ok(function(&[]));
        }

        // Allocate a buffer big enough to hold all the characters
        // (an individual Latin1Character can only expand to 2 UTF-8 bytes).
        let capacity = match characters.len().checked_mul(2) {
            Some(capacity) if is_valid_capacity_for_vector_u8(capacity) => capacity,
            _ => return Err(UTF8ConversionError::OutOfMemory),
        };

        let mut buffer = vec![0u8; capacity];
        let result = convert_latin1_to_utf8(characters, &mut buffer);
        debug_assert!(result.code == ConversionResultCode::Success); // 2x is sufficient for any conversion from Latin1
        let converted_length = result.buffer.len();
        Ok(function(&buffer[..converted_length]))
    }

    /// `StringImpl::tryGetUTF8ForCharacters(function, std::span<const char16_t>, mode)`.
    pub fn try_get_utf8_for_characters16<R>(
        function: impl FnOnce(&[u8]) -> R,
        characters: &[u16],
        mode: ConversionMode,
    ) -> Result<R, UTF8ConversionError> {
        if characters.is_empty() {
            return Ok(function(&[]));
        }

        let utf8_length = Self::utf8_length_from_utf16(characters);
        if !is_valid_capacity_for_vector_u8(utf8_length) {
            return Err(UTF8ConversionError::OutOfMemory);
        }

        let mut buffer_vector = vec![0u8; utf8_length];
        let simd_converted_size = Self::try_convert_utf16_to_utf8(characters, &mut buffer_vector);
        if simd_converted_size != NOT_FOUND {
            return Ok(function(&buffer_vector[..simd_converted_size]));
        }

        let buffer_size = match characters.len().checked_mul(3) {
            Some(buffer_size) if is_valid_capacity_for_vector_u8(buffer_size) => buffer_size,
            _ => return Err(UTF8ConversionError::OutOfMemory),
        };

        buffer_vector.resize(buffer_size, 0);
        let converted_size = Self::utf8_for_characters_into_buffer(characters, mode, &mut buffer_vector)?;
        Ok(function(&buffer_vector[..converted_size]))
    }

    /// `StringImpl::tryGetUTF8(function, mode)` (o template do cabeçalho).
    pub fn try_get_utf8_with<R>(
        &self,
        function: impl FnOnce(&[u8]) -> R,
        mode: ConversionMode,
    ) -> Result<R, UTF8ConversionError> {
        if self.is_8bit() {
            return Self::try_get_utf8_for_characters8(function, self.span8());
        }
        Self::try_get_utf8_for_characters16(function, self.span16(), mode)
    }

    /// `StringImpl::utf8ForCharacters(std::span<const Latin1Character>)`. O `CString` do C++ vira
    /// os bytes convertidos (sem o NUL final).
    pub fn utf8_for_characters8(source: &[u8]) -> Result<Vec<u8>, UTF8ConversionError> {
        Self::try_get_utf8_for_characters8(|converted| converted.to_vec(), source)
    }

    /// `StringImpl::utf8ForCharacters(std::span<const char16_t>, ConversionMode)`.
    pub fn utf8_for_characters16(characters: &[u16], mode: ConversionMode) -> Result<Vec<u8>, UTF8ConversionError> {
        Self::try_get_utf8_for_characters16(|converted| converted.to_vec(), characters, mode)
    }

    /// `StringImpl::tryGetUTF8(ConversionMode)`.
    pub fn try_get_utf8(&self, mode: ConversionMode) -> Result<Vec<u8>, UTF8ConversionError> {
        if self.is_8bit() {
            return Self::utf8_for_characters8(self.span8());
        }
        Self::utf8_for_characters16(self.span16(), mode)
    }

    /// `StringImpl::utf8(ConversionMode)`.
    pub fn utf8(&self, mode: ConversionMode) -> Vec<u8> {
        match self.try_get_utf8(mode) {
            Ok(string) => string,
            // RELEASE_ASSERT(expectedString)
            Err(_) => panic!("StringImpl::utf8: conversão falhou"),
        }
    }
}

#[cfg(test)]
mod cpp_tests2 {
    use super::*;

    fn latin1(bytes: &[u8]) -> StringView<'_> {
        StringView::from(bytes)
    }

    /// A `StringView` de dados UTF-16.
    fn utf16(units: &[u16]) -> StringView<'_> {
        StringView::from(units)
    }

    /// O `StringImpl*` não nulo a partir do `Rc`.
    fn some(string: &Rc<StringImpl>) -> Option<&StringImpl> {
        Some(&**string)
    }

    #[test]
    fn trim_and_simplify() {
        let spaced = StringImpl::create(b"  ab  ");
        let trimmed = spaced.trim(|c| c == ' ' as u16);
        assert_eq!(trimmed.span8(), b"ab");
        let same = trimmed.trim(|c| c == ' ' as u16);
        assert!(Rc::ptr_eq(&trimmed, &same));
        let blank = StringImpl::create(b"   ").trim(|c| c == ' ' as u16);
        assert!(Rc::ptr_eq(&blank, &StringImpl::empty()));
        let wide = StringImpl::create16(&[0x20, 0x20AC, 0x20]).trim(|c| c == ' ' as u16);
        assert_eq!(wide.span16(), &[0x20AC]);

        let messy = StringImpl::create(b"  a  b\t c ");
        let simple = messy.simplify_white_space(is_unicode_whitespace);
        assert_eq!(simple.span8(), b"a b c");
        let clean = StringImpl::create(b"a b");
        assert!(Rc::ptr_eq(&clean, &clean.simplify_white_space(is_unicode_whitespace)));
        let tab = StringImpl::create(b"a\tb");
        assert_eq!(tab.simplify_white_space(is_unicode_whitespace).span8(), b"a b");
        let all = StringImpl::create(b" \t ");
        assert!(all.simplify_white_space(is_unicode_whitespace).is_empty());
    }

    #[test]
    fn find_variants() {
        let s = StringImpl::create(b"a-b-c-b-c");
        assert_eq!(s.find_latin1_span(b"b-c", 0), 2);
        assert_eq!(s.find_latin1_span(b"b-c", 3), 6);
        assert_eq!(s.find_latin1_span(b"b-c", 7), NOT_FOUND);
        assert_eq!(s.find_latin1_span(b"zz", 0), NOT_FOUND);
        assert_eq!(s.reverse_find_latin1_span(b"b-c", usize::MAX), 6);
        assert_eq!(s.reverse_find_latin1_span(b"b-c", 5), 2);
        let wide = StringImpl::create16(&[0x20AC, 0x61, 0x62, 0x20AC]);
        assert_eq!(wide.find_latin1_span(b"ab", 0), 1);

        let hello = StringImpl::create(b"hello world");
        assert_eq!(hello.find_view(utf16(&[0x6F, 0x20, 0x77])), 4);
        assert_eq!(hello.find_view(StringView::default()), NOT_FOUND);
        assert_eq!(hello.find_view(latin1(b"")), 0);
        assert_eq!(hello.find_view(latin1(b"w")), 6);
        assert_eq!(hello.find_view(utf16(&[0x20AC])), NOT_FOUND);
        assert_eq!(hello.find_view(latin1(b"hello world!")), NOT_FOUND);
        assert_eq!(hello.find_view_from(latin1(b"o"), 5), 7);
        assert_eq!(hello.find_view_from(latin1(b""), 3), 3);
        assert_eq!(hello.find_view_from(latin1(b"o"), 99), NOT_FOUND);
        assert_eq!(hello.find_view_from(StringView::default(), 0), NOT_FOUND);

        let mixed = StringImpl::create(b"Hello");
        assert_eq!(mixed.find_ignoring_ascii_case_view(latin1(b"LL")), 2);
        assert_eq!(mixed.find_ignoring_ascii_case_view_from(latin1(b"L"), 3), 3);
        assert_eq!(mixed.find_ignoring_ascii_case_view(latin1(b"")), 0);
        assert_eq!(mixed.find_ignoring_ascii_case_view(StringView::default()), NOT_FOUND);
        assert_eq!(mixed.find_ignoring_ascii_case_view(latin1(b"xyz")), NOT_FOUND);
    }

    #[test]
    fn reverse_find_variants() {
        let s = StringImpl::create(b"abcabc");
        assert_eq!(s.reverse_find_view(latin1(b"bc"), usize::MAX), 4);
        assert_eq!(s.reverse_find_view(latin1(b"bc"), 3), 1);
        assert_eq!(s.reverse_find_view(latin1(b""), usize::MAX), 6);
        assert_eq!(s.reverse_find_view(StringView::default(), usize::MAX), NOT_FOUND);
        assert_eq!(s.reverse_find_view(latin1(b"a"), usize::MAX), 3);
        assert_eq!(s.reverse_find_view(latin1(b"abcabcd"), usize::MAX), NOT_FOUND);
        assert_eq!(s.reverse_find_character(0x20AC, usize::MAX), NOT_FOUND);
        assert_eq!(s.reverse_find_character('c' as u16, usize::MAX), 5);
        let wide = StringImpl::create16(&[0x61, 0x20AC, 0x61]);
        assert_eq!(wide.reverse_find_character(0x61, usize::MAX), 2);
        assert_eq!(wide.reverse_find_view(utf16(&[0x61, 0x20AC]), usize::MAX), 0);
    }

    #[test]
    fn starts_and_ends_with() {
        let s = StringImpl::create(b"foobar");
        assert!(s.starts_with_view(latin1(b"foo")));
        assert!(s.starts_with_view(StringView::default()));
        assert!(!s.starts_with_view(latin1(b"bar")));
        assert!(s.ends_with_view(latin1(b"bar")));
        assert!(!s.ends_with_view(StringView::default()));
        assert!(!s.ends_with_view(latin1(b"foo")));
        assert!(s.starts_with_ignoring_ascii_case_view(latin1(b"FOO")));
        assert!(!s.starts_with_ignoring_ascii_case_view(StringView::default()));
        assert!(s.ends_with_ignoring_ascii_case_view(latin1(b"BAR")));
        assert!(s.starts_with_character('f' as u16));
        assert!(s.ends_with_character('r' as u16));
        assert!(!StringImpl::empty().starts_with_character('f' as u16));
        assert!(s.starts_with_bytes(b"foob"));
        assert!(!s.starts_with_bytes(b"foobarx"));
        assert!(s.ends_with_bytes(b"obar"));
        assert!(!s.ends_with_bytes(b"obaz"));
        assert!(s.has_infix_starting_at(latin1(b"oba"), 2));
        assert!(!s.has_infix_starting_at(latin1(b"oba"), 3));
        assert!(s.has_infix_ending_at(latin1(b"oba"), 5));
        assert!(!s.has_infix_ending_at(latin1(b"oba"), 2));
        let wide = StringImpl::create16(&[0x61, 0x20AC, 0x62]);
        assert!(wide.starts_with_view(utf16(&[0x61, 0x20AC])));
        assert!(wide.ends_with_view(latin1(b"b")));
    }

    #[test]
    fn replace_variants() {
        let s = StringImpl::create(b"a.b.c");
        assert_eq!(s.replace_character('.' as u16, '-' as u16).span8(), b"a-b-c");
        assert!(Rc::ptr_eq(&s, &s.replace_character('z' as u16, '-' as u16)));
        assert!(Rc::ptr_eq(&s, &s.replace_character('.' as u16, '.' as u16)));
        assert!(Rc::ptr_eq(&s, &s.replace_character(0x20AC, '-' as u16)));
        assert_eq!(s.replace_character('.' as u16, 0x20AC).span16(), &[0x61, 0x20AC, 0x62, 0x20AC, 0x63]);

        let hello = StringImpl::create(b"hello");
        assert_eq!(hello.replace_range(1, 3, latin1(b"ipp")).span8(), b"hippo");
        assert_eq!(hello.replace_range(1, 3, StringView::default()).span8(), b"ho");
        assert_eq!(hello.replace_range(99, 5, latin1(b"!")).span8(), b"hello!");
        assert!(Rc::ptr_eq(&hello, &hello.replace_range(2, 0, StringView::default())));
        let widened = hello.replace_range(0, 1, utf16(&[0x20AC]));
        assert_eq!(widened.span16(), &[0x20AC, 0x65, 0x6C, 0x6C, 0x6F]);
        assert!(hello.replace_range(0, 5, StringView::default()).is_empty());

        let dots = StringImpl::create(b"a.b.");
        assert_eq!(dots.replace_character_with_span('.' as u16, b"--".as_slice()).span8(), b"a--b--");
        assert_eq!(dots.replace_character_with_span('.' as u16, [0x20ACu16].as_slice()).span16(), &[0x61, 0x20AC, 0x62, 0x20AC]);
        assert_eq!(dots.replace_character_with_span('.' as u16, b"".as_slice()).span8(), b"ab");
        assert!(Rc::ptr_eq(&dots, &dots.replace_character_with_span('z' as u16, b"x".as_slice())));
        assert!(Rc::ptr_eq(&dots, &dots.replace_character_with_view('.' as u16, StringView::default())));
        assert_eq!(dots.replace_character_with_view('.' as u16, latin1(b"!")).span8(), b"a!b!");

        let xx = StringImpl::create(b"aXXbXX");
        assert_eq!(xx.replace_view(latin1(b"XX"), latin1(b"-")).span8(), b"a-b-");
        assert_eq!(xx.replace_view(latin1(b"XX"), latin1(b"")).span8(), b"ab");
        assert_eq!(xx.replace_view(latin1(b"XX"), utf16(&[0x20AC])).span16(), &[0x61, 0x20AC, 0x62, 0x20AC]);
        assert!(Rc::ptr_eq(&xx, &xx.replace_view(latin1(b""), latin1(b"-"))));
        assert!(Rc::ptr_eq(&xx, &xx.replace_view(StringView::default(), latin1(b"-"))));
        assert!(Rc::ptr_eq(&xx, &xx.replace_view(latin1(b"XX"), StringView::default())));
        assert!(Rc::ptr_eq(&xx, &xx.replace_view(latin1(b"YY"), latin1(b"-"))));
        let wide = StringImpl::create16(&[0x20AC, 0x61, 0x61, 0x20AC]);
        assert_eq!(wide.replace_view(latin1(b"aa"), latin1(b"b")).span16(), &[0x20AC, 0x62, 0x20AC]);
    }

    #[test]
    fn equality() {
        let a = StringImpl::create(b"abc");
        let b = StringImpl::create16(&[0x61, 0x62, 0x63]);
        assert!(equal(&a, &b));
        a.hash();
        b.hash();
        assert!(equal(&a, &b));
        assert!(!equal(&a, &StringImpl::create(b"abd")));
        let empty = StringImpl::empty();
        assert!(equal_nullable(None, None));
        assert!(!equal_nullable(some(&a), None));
        assert!(equal_nullable(some(&a), some(&b)));
        assert!(equal_span(some(&a), Some(b"abc".as_slice())));
        assert!(equal_span(some(&a), Some([0x61u16, 0x62, 0x63].as_slice())));
        assert!(!equal_span(some(&a), Some(b"ab".as_slice())));
        assert!(!equal_span(some(&a), None::<&[u8]>));
        assert!(equal_span(None, None::<&[u8]>));
        assert!(!equal_span(None, Some(b"".as_slice())));
        assert!(equal_ignoring_nullity(None, some(&empty)));
        assert!(equal_ignoring_nullity(some(&empty), None));
        assert!(!equal_ignoring_nullity(None, some(&a)));
        assert!(equal_ignoring_nullity_span16(&[0x61, 0x62, 0x63], some(&a)));
        assert!(equal_ignoring_nullity_span16(&[], None));
        assert!(!equal_ignoring_nullity_span16(&[0x61], some(&a)));
        let upper = StringImpl::create(b"ABC");
        assert!(equal_ignoring_ascii_case(&a, &upper));
        assert!(equal_ignoring_ascii_case(&b, &upper));
        assert!(equal_ignoring_ascii_case_nullable(None, None));
        assert!(!equal_ignoring_ascii_case_nullable(some(&a), None));
        assert!(equal_ignoring_ascii_case_non_null(&a, &upper));
        assert!(!equal_ignoring_ascii_case(&a, &StringImpl::create(b"ABD")));
    }

    #[test]
    fn utf8_conversion() {
        let two = StringImpl::create16(&[0x61, 0xE9]);
        assert_eq!(two.utf8(ConversionMode::LenientConversion), vec![0x61, 0xC3, 0xA9]);
        let latin = StringImpl::create(b"\xE9");
        assert_eq!(latin.utf8(ConversionMode::StrictConversion), vec![0xC3, 0xA9]);
        assert!(StringImpl::empty().utf8(ConversionMode::LenientConversion).is_empty());
        let astral = StringImpl::create16(&[0xD83D, 0xDE00]);
        assert_eq!(astral.utf8(ConversionMode::StrictConversion), vec![0xF0, 0x9F, 0x98, 0x80]);
        assert_eq!(StringImpl::utf8_length_from_utf16(&[0xD83D, 0xDE00]), 4);

        let lone = StringImpl::create16(&[0x61, 0xD83D]);
        assert_eq!(lone.utf8(ConversionMode::LenientConversion), vec![0x61, 0xEF, 0xBF, 0xBD]);
        assert_eq!(
            lone.utf8(ConversionMode::StrictConversionReplacingUnpairedSurrogatesWithFFFD),
            vec![0x61, 0xEF, 0xBF, 0xBD]
        );
        assert_eq!(lone.try_get_utf8(ConversionMode::StrictConversion), Err(UTF8ConversionError::Invalid));

        let mut buffer = [0u8; 6];
        assert_eq!(StringImpl::try_convert_utf16_to_utf8(&[0x61, 0xD83D], &mut buffer), NOT_FOUND);
        assert_eq!(StringImpl::try_convert_utf16_to_utf8(&[0x61, 0x62], &mut buffer), 2);

        let length = two.try_get_utf8_with(|bytes| bytes.len(), ConversionMode::LenientConversion);
        assert_eq!(length, Ok(3));
    }

    #[test]
    fn size_in_bytes_counts_the_header() {
        assert_eq!(StringImpl::create(b"abc").size_in_bytes(), 3 + CPP_SIZE_OF_STRING_IMPL);
        assert_eq!(StringImpl::create16(&[0x100, 0x101]).size_in_bytes(), 4 + CPP_SIZE_OF_STRING_IMPL);
    }
}
