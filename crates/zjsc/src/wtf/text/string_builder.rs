//! Porte de `WTF/wtf/text/StringBuilder.h`, `StringBuilder.cpp` e `StringBuilderInternals.h`.
//!
//! Diferenças inevitáveis em relação ao C++ (o `StringImpl` do porte é imutável e não compartilha
//! buffer por substring):
//!
//! - `m_buffer` (um `StringImpl` mutado no lugar) vira um `Buffer` próprio (`Vec<u8>` ou `Vec<u16>`)
//!   cujo comprimento é a capacidade, como o `length()` do `StringImpl` do C++.
//! - `reifyString` com sobra de capacidade copia o prefixo em vez de criar um substring que
//!   compartilha o buffer; `toString` (que antes faz `shrinkToFit`) move o buffer sem copiar.
//! - O buffer nunca é compartilhado, então o ramo `hasOneRef() == false` de `shrink` e de
//!   `reallocateBuffer` não existe: o buffer sempre é realocado no lugar.
//! - `toStringPreserveCapacity` é `const` com `m_string` mutável no C++; aqui recebe `&mut self`.
//! - `StringView`, `StringTypeAdapter` e as tuplas do `StringConcatenate` não estão portados: os
//!   `append(StringTypes...)` viram os métodos `append_*` explícitos abaixo.
//! - `appendQuotedJSONString` (`StringBuilderJSON.cpp`/`.h`, com `EscapedFormsForJSON.h`) está
//!   aqui como `append_quoted_json_string`.

use crate::wtf::ascii_ctype::{lower_nibble_to_lowercase_ascii_hex_digit, upper_nibble_to_lowercase_ascii_hex_digit};
use crate::wtf::dtoa::{number_to_string_and_size, NumberToStringBuffer};
use crate::wtf::text::atom_string::AtomString;
use crate::wtf::text::string_impl::{copy_characters, copy_characters_widen, CharType, StringImpl, MAX_LENGTH};
use crate::wtf::text::wtf_string::String as WtfString;

/// `enum class OverflowPolicy` do `OverflowPolicy.h`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverflowPolicy {
    CrashOnOverflow,
    RecordOverflow,
    AssertNoOverflow,
}

/// `shouldCrashOnOverflow(OverflowPolicy)`. `ASSERT_ENABLED` não vale na build de release do Bun,
/// então `AssertNoOverflow` não derruba.
pub const fn should_crash_on_overflow(policy: OverflowPolicy) -> bool {
    matches!(policy, OverflowPolicy::CrashOnOverflow)
}

/// `static constexpr unsigned maxCapacity = String::MaxLength`.
const MAX_CAPACITY: u32 = WtfString::MAX_LENGTH;

/// O `m_buffer` do C++: o comprimento do vetor é a capacidade.
#[derive(Debug)]
enum Buffer {
    Latin1(Vec<u8>),
    Utf16(Vec<u16>),
}

impl Buffer {
    fn length(&self) -> u32 {
        match self {
            Buffer::Latin1(v) => v.len() as u32,
            Buffer::Utf16(v) => v.len() as u32,
        }
    }

    fn is_8bit(&self) -> bool {
        matches!(self, Buffer::Latin1(_))
    }
}

/// `StringBuilder`.
#[derive(Debug)]
pub struct StringBuilder {
    string: WtfString,
    buffer: Option<Buffer>,
    length: u32,
    should_crash_on_overflow: bool,
}

impl Default for StringBuilder {
    fn default() -> StringBuilder {
        StringBuilder::new()
    }
}

/// `isLatin1(char16_t)`.
fn is_latin1(character: u16) -> bool {
    character <= 0xFF
}

/// `saturatingSum<uint32_t>(a, b)`.
fn saturating_sum(a: u32, b: u32) -> u32 {
    a.saturating_add(b)
}

impl StringBuilder {
    /// `StringBuilder() = default`.
    pub fn new() -> StringBuilder {
        StringBuilder { string: WtfString::default(), buffer: None, length: 0, should_crash_on_overflow: true }
    }

    /// `explicit StringBuilder(OverflowPolicy)`.
    pub fn with_overflow_policy(policy: OverflowPolicy) -> StringBuilder {
        StringBuilder {
            string: WtfString::default(),
            buffer: None,
            length: 0,
            should_crash_on_overflow: should_crash_on_overflow(policy),
        }
    }

    /// `clear()`. Não muda `m_shouldCrashOnOverflow`.
    pub fn clear(&mut self) {
        self.string = WtfString::default();
        self.buffer = None;
        self.length = 0;
    }

    /// `swap(StringBuilder&)`.
    pub fn swap(&mut self, other: &mut StringBuilder) {
        std::mem::swap(self, other);
    }

    /// `didOverflow()`.
    pub fn did_overflow(&mut self) {
        if self.should_crash_on_overflow {
            // CRASH()
            panic!("StringBuilder: estouro de comprimento");
        }
        self.length = u32::MAX;
    }

    pub fn has_overflowed(&self) -> bool {
        self.length > MAX_LENGTH
    }

    pub fn crashes_on_overflow(&self) -> bool {
        self.should_crash_on_overflow
    }

    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    /// `length()`: `RELEASE_ASSERT(!hasOverflowed())`.
    pub fn length(&self) -> u32 {
        assert!(!self.has_overflowed());
        self.length
    }

    /// `capacity()`.
    pub fn capacity(&self) -> u32 {
        match &self.buffer {
            Some(buffer) => buffer.length(),
            None => self.length(),
        }
    }

    /// `is8Bit()`.
    pub fn is_8bit(&self) -> bool {
        match &self.buffer {
            Some(buffer) => buffer.is_8bit(),
            None => self.string.is_8bit(),
        }
    }

    /// `span8()`: vazio se o conteúdo é UTF-16 ou se estourou.
    pub fn span8(&self) -> &[u8] {
        if self.length == 0 || self.has_overflowed() || !self.is_8bit() {
            return &[];
        }
        if !self.string.is_null() {
            debug_assert!(self.string.length() == self.length);
            return self.string.span8();
        }
        match &self.buffer {
            Some(Buffer::Latin1(v)) => &v[..self.length as usize],
            _ => &[],
        }
    }

    /// `span16()`: vazio se o conteúdo é Latin1 ou se estourou.
    pub fn span16(&self) -> &[u16] {
        if self.length == 0 || self.has_overflowed() || self.is_8bit() {
            return &[];
        }
        if !self.string.is_null() {
            debug_assert!(self.string.length() == self.length);
            return self.string.span16();
        }
        match &self.buffer {
            Some(Buffer::Utf16(v)) => &v[..self.length as usize],
            _ => &[],
        }
    }

    /// `operator[](unsigned)`.
    pub fn char_at(&self, i: u32) -> u16 {
        if self.is_8bit() {
            self.span8()[i as usize] as u16
        } else {
            self.span16()[i as usize]
        }
    }

    /// `equal(const StringBuilder&, std::span<const CharacterType>)`: `builder == StringView`.
    pub fn equal<T: CharType>(&self, buffer: &[T]) -> bool {
        if self.has_overflowed() || self.length as usize != buffer.len() {
            return false;
        }
        let other = buffer.iter().map(|&c| Into::<u32>::into(c) as u16);
        if self.is_8bit() {
            self.span8().iter().map(|&c| c as u16).eq(other)
        } else {
            self.span16().iter().copied().eq(other)
        }
    }

    /// `expandedCapacity(capacity, requiredCapacity)`.
    fn expanded_capacity(capacity: u32, required_capacity: u32) -> u32 {
        const MINIMUM_CAPACITY: u32 = 16;
        required_capacity.max(MINIMUM_CAPACITY.max(capacity.saturating_mul(2).min(MAX_CAPACITY)))
    }

    // ---- alocação (StringBuilderInternals.h) ---------------------------------------------

    /// `allocateBuffer<AllocationCharacterType, CurrentCharacterType>(currentCharacters, required)`:
    /// os caracteres atuais são os do próprio builder (`m_string` ou o prefixo de `m_buffer`);
    /// `wide` escolhe `char16_t` como tipo de alocação (nunca se estreita).
    fn allocate_buffer(&mut self, wide: bool, required_capacity: u32) {
        if required_capacity > MAX_LENGTH {
            self.did_overflow();
            return;
        }
        let required = required_capacity as usize;
        let new_buffer = if !wide {
            let mut data = vec![0u8; required];
            let current = self.span8();
            let count = current.len().min(required);
            copy_characters(&mut data[..count], &current[..count]);
            Buffer::Latin1(data)
        } else {
            let mut data = vec![0u16; required];
            if self.is_8bit() {
                let current = self.span8();
                let count = current.len().min(required);
                copy_characters_widen(&mut data[..count], &current[..count]);
            } else {
                let current = self.span16();
                let count = current.len().min(required);
                copy_characters(&mut data[..count], &current[..count]);
            }
            Buffer::Utf16(data)
        };
        self.buffer = Some(new_buffer);
        self.string = WtfString::default();
    }

    /// `reallocateBuffer(unsigned)` e `reallocateBuffer<CharacterType>(unsigned)`.
    fn reallocate_buffer(&mut self, required_capacity: u32) {
        if self.buffer.is_some() {
            // O buffer é só deste builder: realoca no lugar (`StringImpl::tryReallocate`).
            self.string = WtfString::default();
            if required_capacity > MAX_LENGTH {
                self.did_overflow();
                return;
            }
            let required = required_capacity as usize;
            match &mut self.buffer {
                Some(Buffer::Latin1(v)) => v.resize(required, 0),
                Some(Buffer::Utf16(v)) => v.resize(required, 0),
                None => {}
            }
            return;
        }
        let wide = !self.is_8bit();
        self.allocate_buffer(wide, required_capacity);
    }

    /// `extendBufferForAppending<CharacterType>(requiredLength)`: devolve o deslocamento, no
    /// buffer, onde o chamador escreve (o comprimento anterior); `None` se falhou, estourou ou a
    /// capacidade total é 0. O chamador confere o tipo do buffer.
    fn extend_buffer_for_appending(&mut self, required_length: u32) -> Option<usize> {
        if let Some(buffer) = &self.buffer {
            if required_length <= buffer.length() {
                self.string = WtfString::default();
                return Some(std::mem::replace(&mut self.length, required_length) as usize);
            }
        }
        self.extend_buffer_for_appending_slow_case(required_length)
    }

    /// `extendBufferForAppendingSlowCase<CharacterType>(requiredLength)`.
    fn extend_buffer_for_appending_slow_case(&mut self, required_length: u32) -> Option<usize> {
        if required_length == 0 || self.has_overflowed() {
            return None;
        }
        self.reallocate_buffer(Self::expanded_capacity(self.capacity(), required_length));
        if self.has_overflowed() {
            return None;
        }
        Some(std::mem::replace(&mut self.length, required_length) as usize)
    }

    /// `extendBufferForAppendingWithUpconvert(requiredLength)`.
    fn extend_buffer_for_appending_with_upconvert(&mut self, required_length: u32) -> Option<usize> {
        if self.is_8bit() {
            let capacity = Self::expanded_capacity(self.capacity(), required_length);
            self.allocate_buffer(true, capacity);
            if self.has_overflowed() {
                return None;
            }
            return Some(std::mem::replace(&mut self.length, required_length) as usize);
        }
        self.extend_buffer_for_appending(required_length)
    }

    /// O trecho `[from, to)` do buffer Latin1 (invariante: o buffer é Latin1).
    fn latin1_tail(&mut self, from: usize, to: usize) -> &mut [u8] {
        match &mut self.buffer {
            Some(Buffer::Latin1(v)) => &mut v[from..to],
            _ => panic!("StringBuilder: o buffer deveria ser Latin1"),
        }
    }

    /// O trecho `[from, to)` do buffer UTF-16 (invariante: o buffer é UTF-16).
    fn utf16_tail(&mut self, from: usize, to: usize) -> &mut [u16] {
        match &mut self.buffer {
            Some(Buffer::Utf16(v)) => &mut v[from..to],
            _ => panic!("StringBuilder: o buffer deveria ser UTF-16"),
        }
    }

    // ---- append ----------------------------------------------------------------------------

    /// `append(std::span<const char16_t>)`.
    pub fn append_utf16(&mut self, characters: &[u16]) {
        if characters.is_empty() || self.has_overflowed() {
            return;
        }
        if characters.len() == 1 && is_latin1(characters[0]) && self.is_8bit() {
            self.append_latin1_character(characters[0] as u8);
            return;
        }
        assert!(characters.len() < u32::MAX as usize);
        let required = saturating_sum(self.length, characters.len() as u32);
        if let Some(start) = self.extend_buffer_for_appending_with_upconvert(required) {
            self.utf16_tail(start, start + characters.len()).copy_from_slice(characters);
        }
    }

    /// `append(std::span<const Latin1Character>)`.
    pub fn append_latin1(&mut self, characters: &[u8]) {
        if characters.is_empty() || self.has_overflowed() {
            return;
        }
        assert!(characters.len() < u32::MAX as usize);
        let required = saturating_sum(self.length, characters.len() as u32);
        if self.is_8bit() {
            if let Some(start) = self.extend_buffer_for_appending(required) {
                self.latin1_tail(start, start + characters.len()).copy_from_slice(characters);
            }
        } else if let Some(start) = self.extend_buffer_for_appending(required) {
            copy_characters_widen(self.utf16_tail(start, start + characters.len()), characters);
        }
    }

    /// `append(char16_t)`.
    pub fn append_character(&mut self, character: u16) {
        if self.can_write_in_place() {
            let index = self.length as usize;
            match &mut self.buffer {
                Some(Buffer::Utf16(v)) => {
                    v[index] = character;
                    self.length += 1;
                    return;
                }
                Some(Buffer::Latin1(v)) if is_latin1(character) => {
                    v[index] = character as u8;
                    self.length += 1;
                    return;
                }
                _ => {}
            }
        }
        self.append_utf16(&[character]);
    }

    /// `append(Latin1Character)` e `append(char)`.
    pub fn append_latin1_character(&mut self, character: u8) {
        if self.can_write_in_place() {
            let index = self.length as usize;
            match &mut self.buffer {
                Some(Buffer::Latin1(v)) => v[index] = character,
                Some(Buffer::Utf16(v)) => v[index] = character as u16,
                None => {}
            }
            self.length += 1;
            return;
        }
        self.append_latin1(&[character]);
    }

    /// `m_buffer && m_length < m_buffer->length() && m_string.isNull()`.
    fn can_write_in_place(&self) -> bool {
        match &self.buffer {
            Some(buffer) => self.length < buffer.length() && self.string.is_null(),
            None => false,
        }
    }

    /// Acrescenta um ponto de código (`UChar32`): os suplementares viram o par de surrogates,
    /// como `U16_APPEND` + `append(span<char16_t>)`. Valores que não são ponto de código não
    /// acrescentam nada.
    pub fn append_code_point(&mut self, code_point: u32) {
        if code_point <= 0xFFFF {
            self.append_character(code_point as u16);
        } else if code_point <= 0x10FFFF {
            let offset = code_point - 0x10000;
            self.append_utf16(&[(0xD800 + (offset >> 10)) as u16, (0xDC00 + (offset & 0x3FF)) as u16]);
        }
    }

    /// `append(StringView)`: despacha pela largura do trecho.
    fn append_view(&mut self, is_8bit: bool, span8: &[u8], span16: &[u16]) {
        if is_8bit {
            self.append_latin1(span8);
        } else {
            self.append_utf16(span16);
        }
    }

    /// `append(const String&)`.
    pub fn append_string(&mut self, string: &WtfString) {
        // Se anexa a uma string vazia e não há buffer (`reserveCapacity` não foi chamado),
        // apenas retém a string.
        if self.length == 0 && self.buffer.is_none() {
            self.string = string.clone();
            self.length = string.length();
            return;
        }
        self.append_view(string.is_8bit(), string.span8(), string.span16());
    }

    /// `append(const AtomString&)`.
    pub fn append_atom_string(&mut self, string: &AtomString) {
        self.append_string(string.string());
    }

    /// `append(ASCIILiteral)`.
    pub fn append_ascii_literal(&mut self, string: &str) {
        self.append_latin1(string.as_bytes());
    }

    /// `append(const StringBuilder&)`.
    pub fn append_builder(&mut self, other: &StringBuilder) {
        if self.length == 0 && self.buffer.is_none() && !other.string.is_null() {
            // `length()` aqui para estourar sem conferência explícita.
            self.string = other.string.clone();
            self.length = other.length();
            return;
        }
        self.append_view(other.is_8bit(), other.span8(), other.span16());
    }

    /// `appendSubstring(const String&, offset, length = String::MaxLength)`:
    /// `StringView::substring` limita o trecho ao tamanho da string.
    pub fn append_substring(&mut self, string: &WtfString, offset: u32, length: u32) {
        let total = string.length();
        if offset >= total {
            return;
        }
        let length = length.min(total - offset);
        let (start, end) = (offset as usize, (offset + length) as usize);
        if string.is_8bit() {
            self.append_latin1(&string.span8()[start..end]);
        } else {
            self.append_utf16(&string.span16()[start..end]);
        }
    }

    /// `appendQuotedJSONString(const String&)` (`StringBuilderJSON.cpp`).
    pub fn append_quoted_json_string(&mut self, string: &WtfString) {
        if self.has_overflowed() {
            return;
        }

        // Espaço para o pior caso sem realocar: 2 aspas e 6 unidades (`\uNNNN`) por caractere.
        // `CheckedInt32`: estourar o `i32` é estouro do builder.
        let string_length = match i32::try_from(string.length())
            .ok()
            .and_then(|length| length.checked_mul(6))
            .and_then(|length| length.checked_add(2))
        {
            Some(length) => length as u32,
            None => {
                self.did_overflow();
                return;
            }
        };
        let required = saturating_sum(self.length, string_length);
        let total = string_length as usize;

        if self.is_8bit() && string.is_8bit() {
            if let Some(start) = self.extend_buffer_for_appending(required) {
                let output = self.latin1_tail(start, start + total);
                let remaining = quoted_json_content(output, string.span8());
                if remaining != 0 {
                    self.shrink(self.length - remaining as u32);
                }
            }
        } else if let Some(start) = self.extend_buffer_for_appending_with_upconvert(required) {
            let output = self.utf16_tail(start, start + total);
            let remaining = if string.is_8bit() {
                quoted_json_content(output, string.span8())
            } else {
                quoted_json_content(output, string.span16())
            };
            if remaining != 0 {
                self.shrink(self.length - remaining as u32);
            }
        }
    }

    /// `append(number)` via `IntegerToStringConversionTrait<StringBuilder>::flush`.
    pub fn append_number_i32(&mut self, number: i32) {
        self.append_string(&WtfString::number_i32(number));
    }

    pub fn append_number_u32(&mut self, number: u32) {
        self.append_string(&WtfString::number_u32(number));
    }

    pub fn append_number_i64(&mut self, number: i64) {
        self.append_string(&WtfString::number_i64(number));
    }

    pub fn append_number_u64(&mut self, number: u64) {
        self.append_string(&WtfString::number_u64(number));
    }

    /// `append(double)`: `numberToStringAndSize` e depois `append(span<const Latin1Character>)`.
    pub fn append_number_f64(&mut self, number: f64) {
        let mut buffer: NumberToStringBuffer = [0; 124];
        let digits = number_to_string_and_size(number, &mut buffer);
        self.append_latin1(digits);
    }

    /// `append(float)`.
    pub fn append_number_f32(&mut self, number: f32) {
        let mut buffer: NumberToStringBuffer = [0; 124];
        let digits = number_to_string_and_size(number, &mut buffer);
        self.append_latin1(digits);
    }

    // ---- conversão ---------------------------------------------------------------------------

    /// `reifyString()`.
    fn reify_string(&mut self) {
        assert!(!self.has_overflowed());

        // A string já existe?
        if !self.string.is_null() {
            debug_assert!(self.string.length() == self.length);
            return;
        }

        // Vazio.
        if self.length == 0 {
            self.string = WtfString::from(StringImpl::empty());
            return;
        }

        // Tem de estar válida no buffer (sem compartilhar, copia o prefixo).
        let length = self.length as usize;
        self.string = match &self.buffer {
            Some(Buffer::Latin1(v)) => WtfString::from_latin1(&v[..length]),
            Some(Buffer::Utf16(v)) => WtfString::from_utf16(&v[..length]),
            None => WtfString::default(),
        };
    }

    /// `toString()`.
    pub fn to_string(&mut self) -> &WtfString {
        if self.string.is_null() {
            self.shrink_to_fit();
            self.reify_string();
        }
        &self.string
    }

    /// `toStringPreserveCapacity() const` (aqui `&mut self`, ver o cabeçalho do módulo).
    pub fn to_string_preserve_capacity(&mut self) -> &WtfString {
        if self.string.is_null() {
            self.reify_string();
        }
        &self.string
    }

    /// `toAtomString() const`.
    pub fn to_atom_string(&self) -> AtomString {
        if self.is_empty() {
            return AtomString::from_string_impl(Some(&StringImpl::empty()));
        }

        // Se o buffer tem sobra demais, cria a AtomString de uma cópia.
        if self.should_shrink_to_fit() {
            return if self.is_8bit() {
                AtomString::from_latin1(self.span8())
            } else {
                AtomString::from_utf16(self.span16())
            };
        }

        if !self.string.is_null() {
            return AtomString::from_string(&self.string);
        }

        // `length()` aqui para estourar sem conferência explícita.
        let length = self.length() as usize;
        match &self.buffer {
            Some(Buffer::Latin1(v)) => AtomString::from_latin1(&v[..length]),
            Some(Buffer::Utf16(v)) => AtomString::from_utf16(&v[..length]),
            None => AtomString::new(),
        }
    }

    // ---- capacidade --------------------------------------------------------------------------

    /// `shrink(unsigned newLength)`.
    pub fn shrink(&mut self, new_length: u32) {
        if self.has_overflowed() {
            return;
        }

        debug_assert!(new_length <= self.length);
        if new_length >= self.length {
            if new_length > self.length {
                self.did_overflow();
            }
            return;
        }

        self.length = new_length;

        if self.buffer.is_some() {
            // Limpa a string para soltar a referência ao buffer. O buffer é só deste builder,
            // então basta reduzir o comprimento e seguir usando-o.
            self.string = WtfString::default();
            return;
        }

        // Como o comprimento antigo não era 0 e `m_buffer` é nulo, `m_string` é não nula.
        let shortened = self.string.substring_sharing_impl(0, new_length);
        self.string = shortened;
    }

    /// `reserveCapacity(unsigned newCapacity)`.
    pub fn reserve_capacity(&mut self, new_capacity: u32) {
        if self.has_overflowed() {
            return;
        }

        if let Some(buffer) = &self.buffer {
            if new_capacity > buffer.length() {
                self.reallocate_buffer(new_capacity);
            }
        } else if new_capacity > self.length {
            // Sem comprimento, a alocação é Latin1; senão segue a largura de `m_string`.
            let wide = !self.is_8bit();
            self.allocate_buffer(wide, new_capacity);
        }
        debug_assert!(
            self.has_overflowed() || new_capacity == 0 || self.buffer.as_ref().is_some_and(|b| b.length() >= new_capacity)
        );
    }

    /// `shouldShrinkToFit() const`: encolhe se o buffer está 80% cheio ou menos.
    pub fn should_shrink_to_fit(&self) -> bool {
        match &self.buffer {
            Some(buffer) => {
                !self.has_overflowed() && (buffer.length() as u64) > self.length as u64 + (self.length >> 2) as u64
            }
            None => false,
        }
    }

    /// `shrinkToFit()`.
    pub fn shrink_to_fit(&mut self) {
        if self.should_shrink_to_fit() {
            self.reallocate_buffer(self.length);
            self.string = match self.buffer.take() {
                Some(Buffer::Latin1(v)) => WtfString::adopt(v),
                Some(Buffer::Utf16(v)) => WtfString::adopt(v),
                None => WtfString::default(),
            };
        }
    }

    /// `containsOnlyASCII() const`.
    pub fn contains_only_ascii(&self) -> bool {
        if self.is_8bit() {
            self.span8().iter().all(|&c| c < 0x80)
        } else {
            self.span16().iter().all(|&c| c < 0x80)
        }
    }
}

/// `escapedFormsForJSON` (`EscapedFormsForJSON.h`): 0 não escapa, `u` vira `\u00XX`.
const ESCAPED_FORMS_FOR_JSON: [u8; 0x100] = {
    let mut table = [0u8; 0x100];
    let mut i = 0;
    while i < 0x20 {
        table[i] = b'u';
        i += 1;
    }
    table[0x08] = b'b';
    table[0x09] = b't';
    table[0x0A] = b'n';
    table[0x0C] = b'f';
    table[0x0D] = b'r';
    table[b'"' as usize] = b'"';
    table[b'\\' as usize] = b'\\';
    table
};

/// A unidade de saída de `appendEscapedJSONStringContent` (`Latin1Character` ou `char16_t`).
trait JsonOutputUnit: Copy {
    const IS_LATIN1: bool;
    fn from_unit(unit: u16) -> Self;
}

impl JsonOutputUnit for u8 {
    const IS_LATIN1: bool = true;
    fn from_unit(unit: u16) -> u8 {
        unit as u8
    }
}

impl JsonOutputUnit for u16 {
    const IS_LATIN1: bool = false;
    fn from_unit(unit: u16) -> u16 {
        unit
    }
}

/// A unidade de entrada (`Latin1Character` ou `char16_t`).
trait JsonInputUnit: Copy {
    fn unit(self) -> u16;
}

impl JsonInputUnit for u8 {
    fn unit(self) -> u16 {
        self as u16
    }
}

impl JsonInputUnit for u16 {
    fn unit(self) -> u16 {
        self
    }
}

/// `appendEscapedJSONStringContent(output, input)` mais as aspas de `appendQuotedJSONString`:
/// escreve `"`, o conteúdo escapado e `"` no começo de `output`, e devolve quantas unidades
/// de `output` sobraram sem uso (o `output.size()` final do C++).
fn quoted_json_content<O: JsonOutputUnit, I: JsonInputUnit>(output: &mut [O], input: &[I]) -> usize {
    let mut at = 0;
    let mut put = |output: &mut [O], unit: u16| {
        output[at] = O::from_unit(unit);
        at += 1;
    };
    put(output, b'"' as u16);
    let mut i = 0;
    while i < input.len() {
        let character = input[i].unit();
        i += 1;
        if character <= 0xFF {
            let escaped = ESCAPED_FORMS_FOR_JSON[character as usize];
            if escaped == 0 {
                put(output, character);
                continue;
            }
            put(output, b'\\' as u16);
            put(output, escaped as u16);
            if escaped == b'u' {
                put(output, b'0' as u16);
                put(output, b'0' as u16);
                put(output, upper_nibble_to_lowercase_ascii_hex_digit(character as u8) as u16);
                put(output, lower_nibble_to_lowercase_ascii_hex_digit(character as u8) as u16);
            }
            continue;
        }

        // Só se chega aqui com entrada UTF-16; a saída Latin1 do C++ devolveria `false`.
        if O::IS_LATIN1 {
            break;
        }

        if !(0xD800..=0xDFFF).contains(&character) {
            put(output, character);
            continue;
        }

        if i < input.len() {
            let next = input[i].unit();
            if character <= 0xDBFF && (0xDC00..=0xDFFF).contains(&next) {
                put(output, character);
                put(output, next);
                i += 1;
                continue;
            }
        }

        let (upper, lower) = ((character >> 8) as u8, character as u8);
        put(output, b'\\' as u16);
        put(output, b'u' as u16);
        put(output, upper_nibble_to_lowercase_ascii_hex_digit(upper) as u16);
        put(output, lower_nibble_to_lowercase_ascii_hex_digit(upper) as u16);
        put(output, upper_nibble_to_lowercase_ascii_hex_digit(lower) as u16);
        put(output, lower_nibble_to_lowercase_ascii_hex_digit(lower) as u16);
    }
    put(output, b'"' as u16);
    output.len() - at
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quoted(builder: &mut StringBuilder, string: &WtfString) -> std::string::String {
        builder.append_quoted_json_string(string);
        let value = builder.to_string().clone();
        if value.is_8bit() {
            value.span8().iter().map(|&c| c as char).collect()
        } else {
            std::string::String::from_utf16_lossy(value.span16())
        }
    }

    #[test]
    fn quoted_json_latin1_escapes() {
        let mut builder = StringBuilder::new();
        let out = quoted(&mut builder, &WtfString::from_latin1(b"a\"b\\c\n\t\x08\x0c\r\x01\x1f\xe9"));
        assert_eq!(out, "\"a\\\"b\\\\c\\n\\t\\b\\f\\r\\u0001\\u001f\u{e9}\"");
        assert!(builder.is_8bit());
        assert_eq!(builder.length() as usize, out.chars().count());
    }

    #[test]
    fn quoted_json_utf16_surrogates() {
        let mut builder = StringBuilder::new();
        builder.append_ascii_literal("x:");
        let string = WtfString::from_utf16(&[0xD83D, 0xDE00, 0xD800, 0x41, 0xDC00, 0x20AC, 0xDBFF]);
        builder.append_quoted_json_string(&string);
        let expected: Vec<u16> = "x:\"".encode_utf16().chain([0xD83D, 0xDE00]).chain("\\ud800A\\udc00\u{20AC}\\udbff\"".encode_utf16()).collect();
        assert_eq!(builder.span16(), &expected[..]);
        assert_eq!(builder.length() as usize, expected.len());
    }

    #[test]
    fn quoted_json_empty_and_upconvert() {
        let mut builder = StringBuilder::new();
        assert_eq!(quoted(&mut builder, &WtfString::from_latin1(b"")), "\"\"");
        builder.append_character(0x20AC);
        builder.append_quoted_json_string(&WtfString::from_latin1(b"\n\xff"));
        assert_eq!(builder.span16(), &[0x22, 0x22, 0x20AC, 0x22, 0x5C, 0x6E, 0xFF, 0x22]);
    }

    #[test]
    fn appends_latin1_and_promotes_to_utf16() {
        let mut builder = StringBuilder::new();
        builder.append_ascii_literal("abc");
        assert!(builder.is_8bit());
        builder.append_character(0x20AC);
        assert!(!builder.is_8bit());
        builder.append_latin1(b"d");
        assert_eq!(builder.length(), 5);
        assert_eq!(builder.span16(), &[0x61, 0x62, 0x63, 0x20AC, 0x64]);
        assert!(!builder.contains_only_ascii());
    }

    #[test]
    fn to_string_returns_content() {
        let mut builder = StringBuilder::new();
        builder.append_ascii_literal("foo");
        builder.append_string(&WtfString::from_latin1(b"bar"));
        builder.append_number_i32(-12);
        let string = builder.to_string().clone();
        assert!(string.is_8bit());
        assert_eq!(string.span8(), b"foobar-12");
        assert_eq!(builder.to_string_preserve_capacity().length(), 9);
    }

    #[test]
    fn append_string_retains_when_empty() {
        let mut builder = StringBuilder::new();
        builder.append_string(&WtfString::from_utf16(&[0x20AC, 0x41]));
        assert_eq!(builder.length(), 2);
        assert!(!builder.is_8bit());
        builder.append_code_point(0x1F600);
        assert_eq!(builder.span16(), &[0x20AC, 0x41, 0xD83D, 0xDE00]);
    }

    #[test]
    fn numbers() {
        let mut builder = StringBuilder::new();
        builder.append_number_u32(7);
        builder.append_character(b',' as u16);
        builder.append_number_f64(1.5);
        builder.append_character(b',' as u16);
        builder.append_number_u64(18446744073709551615);
        assert_eq!(builder.to_string().span8(), b"7,1.5,18446744073709551615");
    }

    #[test]
    fn reserve_shrink_and_shrink_to_fit() {
        let mut builder = StringBuilder::new();
        builder.reserve_capacity(100);
        assert_eq!(builder.capacity(), 100);
        builder.append_ascii_literal("hello world");
        assert!(builder.should_shrink_to_fit());
        builder.shrink(5);
        assert_eq!(builder.span8(), b"hello");
        assert_eq!(builder.to_string().span8(), b"hello");
        assert!(builder.equal(&b"hello"[..]));
    }

    #[test]
    fn record_overflow() {
        let mut builder = StringBuilder::with_overflow_policy(OverflowPolicy::RecordOverflow);
        builder.reserve_capacity(MAX_LENGTH + 1);
        assert!(builder.has_overflowed());
        assert!(!builder.crashes_on_overflow());
        builder.append_ascii_literal("x");
        assert!(builder.has_overflowed());
    }

    #[test]
    fn to_atom_string_matches() {
        let mut builder = StringBuilder::new();
        builder.append_ascii_literal("atom");
        let atom = builder.to_atom_string();
        assert_eq!(atom.string().span8(), b"atom");
    }
}
