//! Tradução de `WTF/wtf/BitVector.h` e `BitVector.cpp`.
//!
//! No C++ o vetor ocupa uma palavra (63 bits embutidos, o bit 63 marca "embutido") ou aponta para
//! um bloco alocado fora. Em Rust seguro a palavra mágica vira o enum `Bits`; o que se observa
//! (`size()`, os valores devolvidos por `set`/`clear`, igualdade, contagem) é o mesmo.
//!
//! Não portado: `hash()` (usa `IntHash<uintptr_t>`, que ainda não existe na WTF), `dump` (`PrintStream`),
//! as marcas `EmptyValue`/`DeletedValue` (só servem à `HashTable` do C++), `CFBitVectorRef` (Cocoa),
//! e `findBitSimple`/`equalsSlowCaseSimple`/`equalsSlowCaseFast` como verificadores duplicados: o
//! resultado é o de `findBitFast` e da comparação palavra a palavra.

const BITS_IN_POINTER: usize = 64;
const MAX_INLINE_BITS: usize = BITS_IN_POINTER - 1;

#[derive(Clone, Debug)]
enum Bits {
    /// `isInline()`: os 63 bits baixos da palavra (o bit 63, o marcador, não é guardado).
    Inline(u64),
    /// `OutOfLineBits`: `num_bits` é sempre múltiplo de 64 e `words.len() == num_bits / 64`.
    OutOfLine { num_bits: usize, words: Vec<u64> },
}

/// `WTF::BitVector`.
#[derive(Clone, Debug)]
pub struct BitVector {
    bits: Bits,
}

impl Default for BitVector {
    fn default() -> BitVector {
        BitVector::new()
    }
}

impl BitVector {
    /// `BitVector()`.
    pub fn new() -> BitVector {
        BitVector { bits: Bits::Inline(0) }
    }

    /// `BitVector(size_t numBits)`.
    pub fn with_size(num_bits: usize) -> BitVector {
        let mut vector = BitVector::new();
        vector.ensure_size(num_bits);
        vector
    }

    fn is_inline(&self) -> bool {
        matches!(self.bits, Bits::Inline(_))
    }

    /// `OutOfLineBits::create`: arredonda para múltiplo de 64, tudo zerado.
    fn create_out_of_line(num_bits: usize) -> Bits {
        let num_bits = num_bits.div_ceil(BITS_IN_POINTER) * BITS_IN_POINTER;
        Bits::OutOfLine { num_bits, words: vec![0; num_bits / BITS_IN_POINTER] }
    }

    fn words(&self) -> &[u64] {
        match &self.bits {
            Bits::Inline(word) => std::slice::from_ref(word),
            Bits::OutOfLine { words, .. } => words,
        }
    }

    fn words_mut(&mut self) -> &mut [u64] {
        match &mut self.bits {
            Bits::Inline(word) => std::slice::from_mut(word),
            Bits::OutOfLine { words, .. } => words,
        }
    }

    /// `size()`.
    pub fn size(&self) -> usize {
        match &self.bits {
            Bits::Inline(_) => MAX_INLINE_BITS,
            Bits::OutOfLine { num_bits, .. } => *num_bits,
        }
    }

    /// `ensureSize`.
    pub fn ensure_size(&mut self, num_bits: usize) {
        if num_bits <= self.size() {
            return;
        }
        self.resize_out_of_line(num_bits, 0);
    }

    /// `resize`: como `ensureSize`, mas aceita reduzir.
    pub fn resize(&mut self, num_bits: usize) {
        if num_bits <= MAX_INLINE_BITS {
            if let Bits::OutOfLine { words, .. } = &self.bits {
                let front = words[0] & !(1u64 << MAX_INLINE_BITS);
                self.bits = Bits::Inline(front);
            }
            return;
        }
        self.resize_out_of_line(num_bits, 0);
    }

    /// `clearAll`.
    pub fn clear_all(&mut self) {
        match &mut self.bits {
            Bits::Inline(word) => *word = 0,
            Bits::OutOfLine { words, .. } => words.iter_mut().for_each(|word| *word = 0),
        }
    }

    pub fn quick_get(&self, bit: usize) -> bool {
        assert!(bit < self.size());
        self.words()[bit / BITS_IN_POINTER] & (1u64 << (bit & (BITS_IN_POINTER - 1))) != 0
    }

    pub fn quick_set(&mut self, bit: usize) -> bool {
        assert!(bit < self.size());
        let mask = 1u64 << (bit & (BITS_IN_POINTER - 1));
        let word = &mut self.words_mut()[bit / BITS_IN_POINTER];
        let result = *word & mask != 0;
        *word |= mask;
        result
    }

    pub fn quick_clear(&mut self, bit: usize) -> bool {
        assert!(bit < self.size());
        let mask = 1u64 << (bit & (BITS_IN_POINTER - 1));
        let word = &mut self.words_mut()[bit / BITS_IN_POINTER];
        let result = *word & mask != 0;
        *word &= !mask;
        result
    }

    /// `quickSet(bit, value)`.
    pub fn quick_set_value(&mut self, bit: usize, value: bool) -> bool {
        if value {
            self.quick_set(bit)
        } else {
            self.quick_clear(bit)
        }
    }

    pub fn get(&self, bit: usize) -> bool {
        if bit >= self.size() {
            return false;
        }
        self.quick_get(bit)
    }

    pub fn contains(&self, bit: usize) -> bool {
        self.get(bit)
    }

    /// `set(bit)`: devolve o valor anterior.
    pub fn set(&mut self, bit: usize, value: bool) -> bool {
        if value {
            self.ensure_size(bit + 1);
            self.quick_set(bit)
        } else {
            self.clear(bit)
        }
    }

    /// `set(bit)` sem o valor: o overload de um argumento.
    pub fn set_bit(&mut self, bit: usize) -> bool {
        self.set(bit, true)
    }

    /// `add`: devolve se o bit passou de falso para verdadeiro.
    pub fn add(&mut self, bit: usize) -> bool {
        !self.set_bit(bit)
    }

    /// `ensureSizeAndSet`.
    pub fn ensure_size_and_set(&mut self, bit: usize, size: usize) -> bool {
        self.ensure_size(size);
        self.quick_set(bit)
    }

    pub fn clear(&mut self, bit: usize) -> bool {
        if bit >= self.size() {
            return false;
        }
        self.quick_clear(bit)
    }

    pub fn remove(&mut self, bit: usize) -> bool {
        self.clear(bit)
    }

    /// `merge`.
    pub fn merge(&mut self, other: &BitVector) {
        if let (Bits::Inline(a), Bits::Inline(b)) = (&mut self.bits, &other.bits) {
            *a |= *b;
            return;
        }
        // `mergeSlow`: `other` embutido cabe na primeira palavra; fora, cresce antes.
        if let Bits::Inline(b) = &other.bits {
            self.words_mut()[0] |= *b;
            return;
        }
        self.ensure_size(other.size());
        let b = other.words();
        let a = self.words_mut();
        for i in 0..b.len() {
            a[i] |= b[i];
        }
    }

    /// `filter`.
    pub fn filter(&mut self, other: &BitVector) {
        match (&mut self.bits, &other.bits) {
            (Bits::Inline(a), Bits::Inline(b)) => *a &= *b,
            (Bits::Inline(a), Bits::OutOfLine { words, .. }) => *a &= words[0] & !(1u64 << MAX_INLINE_BITS),
            (Bits::OutOfLine { words, .. }, Bits::Inline(b)) => {
                words[0] &= *b;
                words[1..].iter_mut().for_each(|word| *word = 0);
            }
            (Bits::OutOfLine { words: a, .. }, Bits::OutOfLine { words: b, .. }) => {
                let common = a.len().min(b.len());
                for i in 0..common {
                    a[i] &= b[i];
                }
                if a.len() > b.len() {
                    a[b.len()..].iter_mut().for_each(|word| *word = 0);
                }
            }
        }
    }

    /// `exclude`.
    pub fn exclude(&mut self, other: &BitVector) {
        match (&mut self.bits, &other.bits) {
            (Bits::Inline(a), Bits::Inline(b)) => *a &= !*b,
            (Bits::Inline(a), Bits::OutOfLine { words, .. }) => *a &= !(words[0] & !(1u64 << MAX_INLINE_BITS)),
            (Bits::OutOfLine { words, .. }, Bits::Inline(b)) => words[0] &= !*b,
            (Bits::OutOfLine { words: a, .. }, Bits::OutOfLine { words: b, .. }) => {
                let common = a.len().min(b.len());
                for i in 0..common {
                    a[i] &= !b[i];
                }
            }
        }
    }

    /// `bitCount`.
    pub fn bit_count(&self) -> usize {
        self.words().iter().map(|word| word.count_ones() as usize).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.words().iter().all(|word| *word == 0)
    }

    /// `findBit(index, value)`: o índice do primeiro bit igual a `value` a partir de `index`, ou
    /// `size()` se não houver.
    pub fn find_bit(&self, index: usize, value: bool) -> usize {
        let size = self.size();
        let words = self.words();
        let mut index = index;
        while index < size {
            let word_index = index / BITS_IN_POINTER;
            let in_word = index % BITS_IN_POINTER;
            let word = if value { words[word_index] } else { !words[word_index] };
            let masked = word >> in_word;
            if masked != 0 {
                let found = word_index * BITS_IN_POINTER + in_word + masked.trailing_zeros() as usize;
                // O embutido só tem 63 bits válidos.
                return found.min(size);
            }
            index = (word_index + 1) * BITS_IN_POINTER;
        }
        size
    }

    /// `forEachSetBit`.
    pub fn for_each_set_bit(&self, mut func: impl FnMut(usize)) {
        self.for_each_set_bit_from(0, &mut func);
    }

    /// `forEachSetBit(startIndex, func)`.
    pub fn for_each_set_bit_from(&self, start_index: usize, func: &mut impl FnMut(usize)) {
        let mut index = self.find_bit(start_index, true);
        while index < self.size() {
            func(index);
            index = self.find_bit(index + 1, true);
        }
    }

    /// `begin()`/`end()`: os índices dos bits ligados, em ordem.
    pub fn iter(&self) -> BitVectorIter<'_> {
        BitVectorIter { bit_vector: self, index: self.find_bit(0, true) }
    }

    /// `outOfLineMemoryUse(bitCount)`.
    pub fn out_of_line_memory_use_for(bit_count: usize) -> u32 {
        if bit_count <= MAX_INLINE_BITS {
            return 0;
        }
        Self::byte_count(bit_count) as u32
    }

    pub fn out_of_line_memory_use(&self) -> u32 {
        Self::out_of_line_memory_use_for(self.size())
    }

    fn byte_count(bit_count: usize) -> usize {
        (bit_count + 7) >> 3
    }

    /// `shiftRightByMultipleOf64`.
    pub fn shift_right_by_multiple_of_64(&mut self, shift_in_bits: usize) {
        assert!(shift_in_bits % 64 == 0);
        let shift_in_words = shift_in_bits / BITS_IN_POINTER;
        let num_bits = self.size() + shift_in_bits;
        self.resize_out_of_line(num_bits, shift_in_words);
    }

    /// `resizeOutOfLine`.
    fn resize_out_of_line(&mut self, num_bits: usize, shift_in_words: usize) {
        debug_assert!(num_bits > MAX_INLINE_BITS);
        let mut new_bits = Self::create_out_of_line(num_bits);
        let old_size = self.size();
        if let Bits::OutOfLine { words: new_words, .. } = &mut new_bits {
            match &self.bits {
                Bits::Inline(word) => {
                    new_words[shift_in_words] = *word & !(1u64 << MAX_INLINE_BITS);
                }
                Bits::OutOfLine { words: old_words, .. } => {
                    if num_bits > old_size {
                        new_words[shift_in_words..shift_in_words + old_words.len()].copy_from_slice(old_words);
                    } else {
                        let count = new_words.len();
                        new_words.copy_from_slice(&old_words[..count]);
                    }
                }
            }
        }
        self.bits = new_bits;
    }
}

impl PartialEq for BitVector {
    /// `operator==`: bits além do tamanho de um dos lados contam como zero.
    fn eq(&self, other: &BitVector) -> bool {
        let (a, b) = (self.words(), other.words());
        let common = a.len().min(b.len());
        a[..common] == b[..common] && a[common..].iter().all(|word| *word == 0) && b[common..].iter().all(|word| *word == 0)
    }
}

impl Eq for BitVector {}

/// `BitVector::iterator`, como `Iterator` de Rust.
pub struct BitVectorIter<'a> {
    bit_vector: &'a BitVector,
    index: usize,
}

impl Iterator for BitVectorIter<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        if self.index >= self.bit_vector.size() {
            return None;
        }
        let current = self.index;
        self.index = self.bit_vector.find_bit(current + 1, true);
        Some(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grows_out_of_line() {
        let mut v = BitVector::new();
        assert_eq!(v.size(), 63);
        assert!(!v.set(3, true));
        assert!(v.set(3, true));
        assert!(!v.set(100, true));
        assert_eq!(v.size(), 128);
        assert!(v.get(3) && v.get(100) && !v.get(101));
        assert_eq!(v.bit_count(), 2);
        assert_eq!(v.iter().collect::<Vec<_>>(), vec![3, 100]);
        v.resize(10);
        assert_eq!(v.size(), 63);
        assert!(v.get(3) && !v.get(100));
    }

    #[test]
    fn equality_ignores_trailing_zeros() {
        let mut a = BitVector::new();
        let mut b = BitVector::with_size(200);
        a.set(5, true);
        b.set(5, true);
        assert_eq!(a, b);
        b.set(150, true);
        assert_ne!(a, b);
    }

    #[test]
    fn find_bit_and_set_ops() {
        let mut a = BitVector::new();
        a.set(1, true);
        a.set(70, true);
        assert_eq!(a.find_bit(2, true), 70);
        assert_eq!(a.find_bit(0, false), 0);
        assert_eq!(a.find_bit(71, true), a.size());
        let mut b = BitVector::new();
        b.set(1, true);
        a.exclude(&b);
        assert!(!a.get(1) && a.get(70));
        a.filter(&b);
        assert!(a.is_empty());
    }
}
