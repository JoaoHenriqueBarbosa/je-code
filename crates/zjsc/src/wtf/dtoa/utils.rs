// Tradução de WTF/wtf/dtoa/utils.h (double-conversion, V8 e Apple).
//
// Mapeamentos: `Vector<T>`/`BufferReference<T>` viram `&[T]`/`&mut [T]`; as macros de
// plataforma, de MSVC e de tipos inteiros somem; `UINT64_2PART_C` vira `uint64_2part_c`.

/// `typedef uint16_t uc16`.
pub type Uc16 = u16;

/// `UINT64_2PART_C(a, b)`: `a` e `b` são os dois pedaços de 32 bits, escritos em hexadecimal no
/// C++ (`UINT64_2PART_C(0x12345678,90123456)`); aqui chegam já como valores.
pub const fn uint64_2part_c(a: u32, b: u32) -> u64 {
    ((a as u64) << 32) + b as u64
}

/// `kCharSize = sizeof(char)`.
pub const CHAR_SIZE: i32 = 1;

/// `Max`: o maior dos dois parâmetros.
pub fn max<T: PartialOrd>(a: T, b: T) -> T {
    if a < b { b } else { a }
}

/// `Min`: o menor dos dois parâmetros.
pub fn min<T: PartialOrd>(a: T, b: T) -> T {
    if a < b { a } else { b }
}

/// `StrLength`: comprimento de uma string C. O fim é o primeiro byte zero; sem zero, o fim da
/// fatia.
pub fn str_length(string: &[u8]) -> i32 {
    let length = string.iter().position(|&c| c == 0).unwrap_or(string.len());
    debug_assert!(length == (length as i32) as usize);
    length as i32
}

/// Auxiliar para montar strings de resultado num buffer de caracteres. O objetivo da classe é
/// usar operações seguras que conferem os limites do buffer.
pub struct StringBuilder<'a> {
    buffer: &'a mut [u8],
    position: i32,
}

impl<'a> StringBuilder<'a> {
    pub fn new(buffer: &'a mut [u8]) -> Self {
        StringBuilder { buffer, position: 0 }
    }

    pub fn size(&self) -> i32 {
        self.buffer.len() as i32
    }

    /// O buffer que o builder preenche, inteiro. No C++ o chamador tem o próprio ponteiro para o
    /// buffer (`truncateTrailingZeros` lê o texto montado por ele); no Rust o builder detém a
    /// referência mutável, então a leitura passa por aqui.
    pub fn buffer(&self) -> &[u8] {
        self.buffer
    }

    /// Posição atual no builder.
    pub fn position(&self) -> i32 {
        debug_assert!(!self.is_finalized());
        self.position
    }

    /// Zera a posição.
    pub fn reset(&mut self) {
        self.position = 0;
    }

    /// Acrescenta um caractere. Caractere 0 não é permitido; `finalize` termina a string.
    pub fn add_character(&mut self, c: u8) {
        debug_assert!(c != 0);
        debug_assert!(!self.is_finalized() && self.position < self.buffer.len() as i32);
        self.buffer[self.position as usize] = c;
        self.position += 1;
    }

    /// Acrescenta uma string inteira (o comprimento vem de `str_length`).
    pub fn add_string(&mut self, s: &[u8]) {
        self.add_substring(s, str_length(s));
    }

    /// Acrescenta os primeiros `n` caracteres de `s`, que precisa ter caracteres suficientes.
    pub fn add_substring(&mut self, s: &[u8], n: i32) {
        debug_assert!(!self.is_finalized() && self.position + n < self.buffer.len() as i32);
        debug_assert!(n as usize <= s.iter().position(|&c| c == 0).unwrap_or(s.len()));
        let start = self.position as usize;
        let n = n as usize;
        self.buffer[start..start + n].copy_from_slice(&s[..n]);
        self.position += n as i32;
    }

    /// Sobrecarga `AddSubstring(std::span<const char>)`: acrescenta a fatia inteira.
    pub fn add_substring_span(&mut self, s: &[u8]) {
        debug_assert!(!self.is_finalized() && (self.position as usize) + s.len() < self.buffer.len());
        debug_assert!(!s.contains(&0));
        let start = self.position as usize;
        self.buffer[start..start + s.len()].copy_from_slice(s);
        self.position += s.len() as i32;
    }

    /// Acrescenta `count` caracteres `c`. Com contagem zero nada é acrescentado.
    pub fn add_padding(&mut self, c: u8, count: usize) {
        for _ in 0..count {
            self.add_character(c);
        }
    }

    pub fn remove_characters(&mut self, start: usize, end: usize) {
        debug_assert!(start <= end);
        debug_assert!(end as i32 <= self.position);
        let position = self.position as usize;
        self.buffer.copy_within(end..position, start);
        self.position -= (end - start) as i32;
    }

    /// Termina a string com 0 e devolve o trecho escrito (sem o terminador).
    pub fn finalize(&mut self) -> &mut [u8] {
        debug_assert!(!self.is_finalized() && self.position < self.buffer.len() as i32);
        let length = if self.position < 0 { 0 } else { self.position as usize };
        self.buffer[length] = 0;
        // Garante que ninguém conseguiu pôr um caractere 0 no meio da string.
        debug_assert!(
            self.buffer.iter().position(|&c| c == 0).unwrap_or(self.buffer.len()) == length
        );
        self.position = -1;
        debug_assert!(self.is_finalized());
        &mut self.buffer[..length]
    }

    fn is_finalized(&self) -> bool {
        self.position < 0
    }
}

impl Drop for StringBuilder<'_> {
    // `~StringBuilder() { if (!is_finalized()) Finalize(); }`
    fn drop(&mut self) {
        if !self.is_finalized() {
            self.finalize();
        }
    }
}

/// `validShortestRepresentation`.
pub const fn valid_shortest_representation(
    exponent: i32,
    decimal_in_shortest_low: i32,
    decimal_in_shortest_high: i32,
) -> bool {
    decimal_in_shortest_low <= exponent && exponent < decimal_in_shortest_high
}

pub const DEFAULT_DECIMAL_IN_SHORTEST_LOW: i32 = -6;
pub const DEFAULT_DECIMAL_IN_SHORTEST_HIGH: i32 = 21;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_part_constant() {
        assert_eq!(uint64_2part_c(0x7FF00000, 0x00000000), 0x7FF0_0000_0000_0000);
        assert_eq!(uint64_2part_c(0x000FFFFF, 0xFFFFFFFF), 0x000F_FFFF_FFFF_FFFF);
    }

    #[test]
    fn min_max_and_str_length() {
        assert_eq!(max(3, 5), 5);
        assert_eq!(min(3, 5), 3);
        assert_eq!(str_length(b"abc\0def"), 3);
        assert_eq!(str_length(b"abcd"), 4);
    }

    #[test]
    fn shortest_representation_bounds() {
        assert!(valid_shortest_representation(-6, DEFAULT_DECIMAL_IN_SHORTEST_LOW, DEFAULT_DECIMAL_IN_SHORTEST_HIGH));
        assert!(!valid_shortest_representation(21, DEFAULT_DECIMAL_IN_SHORTEST_LOW, DEFAULT_DECIMAL_IN_SHORTEST_HIGH));
    }

    #[test]
    fn string_builder_builds_and_finalizes() {
        let mut buf = [0xAAu8; 16];
        let mut builder = StringBuilder::new(&mut buf);
        assert_eq!(builder.size(), 16);
        builder.add_character(b'1');
        builder.add_string(b"23\0");
        builder.add_padding(b'0', 3);
        builder.add_substring(b"xyz", 2);
        assert_eq!(builder.position(), 8);
        builder.remove_characters(4, 6);
        assert_eq!(builder.position(), 6);
        let out = builder.finalize();
        assert_eq!(out, b"1230xy");
    }

    #[test]
    fn string_builder_reset() {
        let mut buf = [0u8; 8];
        let mut builder = StringBuilder::new(&mut buf);
        builder.add_substring_span(b"abc");
        builder.reset();
        assert_eq!(builder.position(), 0);
        builder.add_character(b'z');
        assert_eq!(builder.finalize(), b"z");
    }
}
