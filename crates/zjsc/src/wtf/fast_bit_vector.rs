//! Porte de `WTF/wtf/FastBitVector.h` e `FastBitVector.cpp`.
//!
//! O C++ monta `FastBitVectorImpl<Words>` com visões preguiçosas (`FastBitVectorAndWords`,
//! `FastBitVectorOrWords`, `FastBitVectorNotWords`) para que `a & b`, `a | b` e `~a` não aloquem. Em
//! Rust as três operações são métodos que devolvem um `FastBitVector` novo (`and`, `or`, `not`): o
//! resultado de cada palavra é o mesmo, e é o que o `operator=`/construtor a partir de visão faria ao
//! materializar. `forEachClearBit` itera `~palavra` em todas as palavras, inclusive nos bits de
//! preenchimento depois de `numBits` (assim é no C++), e aqui também.
//!
//! Não portado: `atomicSetAndCheck` (sem chamador, e exigiria palavras atômicas), `FastBitReference`
//! (o `operator[]`/`at` mutável vira `set`), e `dump(PrintStream&)` vira `Display`.

use std::fmt;

/// `fastBitVectorArrayLength(numBits)`.
pub const fn fast_bit_vector_array_length(num_bits: usize) -> usize {
    (num_bits + 31) / 32
}

/// `findBitInWord(word, index, endIndex, value)` de `BitSet.h`: procura a partir de `index`, e
/// devolve `true` com `index` no primeiro bit igual a `value`; se não houver, `index` termina em
/// `end_index`.
fn find_bit_in_word(word: u32, index: &mut usize, end_index: usize, value: bool) -> bool {
    let mut word = if value { word } else { !word };
    word >>= *index;
    while *index < end_index {
        if word & 1 != 0 {
            return true;
        }
        *index += 1;
        word >>= 1;
    }
    *index = end_index;
    false
}

/// `class FastBitVector` (o `FastBitVectorImpl<FastBitVectorWordOwner>`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FastBitVector {
    words: Vec<u32>,
    num_bits: usize,
}

impl FastBitVector {
    /// `explicit FastBitVector(size_t numBits)`.
    pub fn with_num_bits(num_bits: usize) -> FastBitVector {
        let mut result = FastBitVector::default();
        result.grow(num_bits);
        result
    }

    /// `FastBitVector(FillWith, size_t numBits, bool value)`.
    pub fn filled(num_bits: usize, value: bool) -> FastBitVector {
        let mut result = FastBitVector::with_num_bits(num_bits);
        result.fill(value);
        result
    }

    /// `numBits()`.
    pub fn num_bits(&self) -> usize {
        self.num_bits
    }

    /// `size()`.
    pub fn size(&self) -> usize {
        self.num_bits
    }

    /// `arrayLength()`.
    pub fn array_length(&self) -> usize {
        fast_bit_vector_array_length(self.num_bits)
    }

    /// `word(index)`.
    pub fn word(&self, index: usize) -> u32 {
        self.words[index]
    }

    /// `resize(numBits)` do `FastBitVectorWordOwner`: só cresce o vetor de palavras (`resizeSlow`
    /// tem `RELEASE_ASSERT(newLength >= oldLength)`), com as palavras novas zeradas.
    pub fn resize(&mut self, num_bits: usize) {
        let new_length = fast_bit_vector_array_length(num_bits);
        if self.array_length() != new_length {
            assert!(new_length >= self.array_length());
            self.words.resize(new_length, 0);
        }
        self.num_bits = num_bits;
    }

    /// `setAll()`.
    pub fn set_all(&mut self) {
        self.words.iter_mut().for_each(|word| *word = u32::MAX);
    }

    /// `clearAll()`.
    pub fn clear_all(&mut self) {
        self.words.iter_mut().for_each(|word| *word = 0);
    }

    /// `fill(bool)`.
    pub fn fill(&mut self, value: bool) {
        if value {
            self.set_all();
        } else {
            self.clear_all();
        }
    }

    /// `grow(size_t)`.
    pub fn grow(&mut self, new_size: usize) {
        self.resize(new_size);
    }

    /// `clearRange(begin, end)`.
    pub fn clear_range(&mut self, begin: usize, end: usize) {
        if end - begin < 32 {
            for i in begin..end {
                self.set(i, false);
            }
            return;
        }

        let end_begin_slop = (begin + 31) & !31;
        let begin_end_slop = end & !31;

        for i in begin..end_begin_slop {
            self.set(i, false);
        }
        for i in begin_end_slop..end {
            self.set(i, false);
        }
        for i in (end_begin_slop / 32)..(begin_end_slop / 32) {
            self.words[i] = 0;
        }
    }

    /// `operator=(const FastBitVectorImpl<OtherWords>&)`.
    pub fn assign(&mut self, other: &FastBitVector) {
        if self.num_bits() != other.num_bits() {
            self.resize(other.num_bits());
        }
        for i in (0..self.array_length()).rev() {
            self.words[i] = other.words[i];
        }
    }

    /// `setAndCheck(other)`: devolve `true` se o conteúdo mudou.
    pub fn set_and_check(&mut self, other: &FastBitVector) -> bool {
        let mut changed = false;
        debug_assert!(self.num_bits() == other.num_bits());
        for i in (0..self.array_length()).rev() {
            changed |= self.words[i] != other.words[i];
            self.words[i] = other.words[i];
        }
        changed
    }

    /// `operator|=`.
    pub fn or_assign(&mut self, other: &FastBitVector) {
        debug_assert!(self.num_bits() == other.num_bits());
        for i in (0..self.array_length()).rev() {
            self.words[i] |= other.words[i];
        }
    }

    /// `operator&=`.
    pub fn and_assign(&mut self, other: &FastBitVector) {
        debug_assert!(self.num_bits() == other.num_bits());
        for i in (0..self.array_length()).rev() {
            self.words[i] &= other.words[i];
        }
    }

    /// `operator&` materializado.
    pub fn and(&self, other: &FastBitVector) -> FastBitVector {
        debug_assert!(self.num_bits() == other.num_bits());
        let mut result = self.clone();
        result.and_assign(other);
        result
    }

    /// `operator|` materializado.
    pub fn or(&self, other: &FastBitVector) -> FastBitVector {
        debug_assert!(self.num_bits() == other.num_bits());
        let mut result = self.clone();
        result.or_assign(other);
        result
    }

    /// `operator~` materializado.
    pub fn not(&self) -> FastBitVector {
        FastBitVector { words: self.words.iter().map(|word| !word).collect(), num_bits: self.num_bits }
    }

    /// `at(index) const` e `operator[] const`.
    pub fn at(&self, index: usize) -> bool {
        debug_assert!(index < self.num_bits());
        (self.words[index >> 5] & (1 << (index & 31))) != 0
    }

    /// `at(index) = value` (o `FastBitReference` do C++), com o `RELEASE_ASSERT(index < numBits())`.
    pub fn set(&mut self, index: usize, value: bool) {
        assert!(index < self.num_bits());
        let mask = 1u32 << (index & 31);
        if value {
            self.words[index >> 5] |= mask;
        } else {
            self.words[index >> 5] &= !mask;
        }
    }

    /// `bitCount()`.
    pub fn bit_count(&self) -> usize {
        self.words.iter().map(|word| word.count_ones() as usize).sum()
    }

    /// `isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|word| *word == 0)
    }

    /// `forEachSetBit(func)`.
    pub fn for_each_set_bit(&self, mut func: impl FnMut(usize)) {
        for i in 0..self.array_length() {
            let mut word = self.words[i];
            let mut j = i * 32;
            while word != 0 {
                if word & 1 != 0 {
                    func(j);
                }
                word >>= 1;
                j += 1;
            }
        }
    }

    /// `forEachClearBit(func)`: `(~*this).forEachSetBit(func)`.
    pub fn for_each_clear_bit(&self, func: impl FnMut(usize)) {
        self.not().for_each_set_bit(func);
    }

    /// `forEachBit(value, func)`.
    pub fn for_each_bit(&self, value: bool, func: impl FnMut(usize)) {
        if value {
            self.for_each_set_bit(func);
        } else {
            self.for_each_clear_bit(func);
        }
    }

    /// `findBit(startIndex, value)`: `num_bits()` se não houver.
    pub fn find_bit(&self, start_index: usize, value: bool) -> usize {
        // Se `value` é true isto dá 0; se é false dá UINT_MAX.
        let skip_value: u32 = (value as u32 ^ 1).wrapping_neg();

        let num_words = fast_bit_vector_array_length(self.num_bits);

        let mut word_index = start_index / 32;
        let mut start_index_in_word = start_index - word_index * 32;

        while word_index < num_words {
            let word = self.words[word_index];
            if word != skip_value {
                let mut index = start_index_in_word;
                if find_bit_in_word(word, &mut index, 32, value) {
                    return word_index * 32 + index;
                }
            }

            word_index += 1;
            start_index_in_word = 0;
        }

        self.num_bits()
    }

    /// `findSetBit(index)`.
    pub fn find_set_bit(&self, index: usize) -> usize {
        self.find_bit(index, true)
    }

    /// `findClearBit(index)`.
    pub fn find_clear_bit(&self, index: usize) -> usize {
        self.find_bit(index, false)
    }
}

/// `dump(PrintStream&)`: `1` para o bit ligado e `-` para o desligado.
impl fmt::Display for FastBitVector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for i in 0..self.num_bits() {
            formatter.write_str(if self.at(i) { "1" } else { "-" })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_find_and_count() {
        let mut bits = FastBitVector::with_num_bits(70);
        bits.set(3, true);
        bits.set(40, true);
        bits.set(69, true);
        assert_eq!(bits.bit_count(), 3);
        assert_eq!(bits.find_set_bit(0), 3);
        assert_eq!(bits.find_set_bit(4), 40);
        assert_eq!(bits.find_set_bit(41), 69);
        assert_eq!(bits.find_set_bit(70), 70);
        assert_eq!(bits.find_clear_bit(3), 4);
        let mut seen = Vec::new();
        bits.for_each_set_bit(|index| seen.push(index));
        assert_eq!(seen, vec![3, 40, 69]);
    }

    #[test]
    fn set_and_check_reports_change() {
        let mut a = FastBitVector::with_num_bits(10);
        let mut b = FastBitVector::with_num_bits(10);
        b.set(2, true);
        assert!(a.set_and_check(&b));
        assert!(!a.set_and_check(&b));
        a.or_assign(&b);
        assert!(a.at(2));
        a.clear_range(0, 10);
        assert!(a.is_empty());
    }
}
