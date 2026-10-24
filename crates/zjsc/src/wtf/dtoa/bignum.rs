//! Porte de `WTF/wtf/dtoa/bignum.h` e `bignum.cc` (double-conversion do V8).
//!
//! Inteiro grande sem sinal de capacidade fixa, com expoente em unidades de bigit.

// Copyright 2010 the V8 project authors. All rights reserved.
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are
// met:
//
//     * Redistributions of source code must retain the above copyright
//       notice, this list of conditions and the following disclaimer.
//     * Redistributions in binary form must reproduce the above
//       copyright notice, this list of conditions and the following
//       disclaimer in the documentation and/or other materials provided
//       with the distribution.
//     * Neither the name of Google Inc. nor the names of its
//       contributors may be used to endorse or promote products derived
//       from this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
// "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
// LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
// A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
// OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
// SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
// LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
// DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
// THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
// (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
// OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use crate::wtf::ascii_ctype::to_ascii_hex_value;

type Chunk = u32;
type DoubleChunk = u64;

// 3584 = 128 * 28. Representa 2^3584 > 10^1000 com exatidão. O Bignum codifica
// números bem maiores, pois carrega um expoente.
pub const K_MAX_SIGNIFICANT_BITS: i32 = 3584;

const K_CHUNK_SIZE: i32 = (std::mem::size_of::<Chunk>() * 8) as i32;
// Com bigit de 28 bits perdemos alguns bits, mas um double ainda cabe com folga
// em dois chunks e, mais importante, dá para usar a multiplicação de Comba.
const K_BIGIT_SIZE: i32 = 28;
const K_BIGIT_MASK: Chunk = (1 << K_BIGIT_SIZE) - 1;
// Toda instância aloca K_BIGIT_CAPACITY chunks. Bignums não crescem.
pub const K_BIGIT_CAPACITY: usize = (K_MAX_SIGNIFICANT_BITS / K_BIGIT_SIZE) as usize;

/// `ReadUInt64`: lê `digits_to_read` dígitos decimais a partir de `from`.
fn read_uint64(buffer: &[u8], from: i32, digits_to_read: i32) -> u64 {
    let mut result: u64 = 0;
    let mut i = from;
    while i < from + digits_to_read {
        let digit: i32 = buffer[i as usize] as i32 - '0' as i32;
        debug_assert!((0..=9).contains(&digit));
        result = result.wrapping_mul(10).wrapping_add(digit as i64 as u64);
        i += 1;
    }
    result
}

fn size_in_hex_chars(mut number: Chunk) -> i32 {
    debug_assert!(number > 0);
    let mut result = 0;
    while number != 0 {
        number >>= 4;
        result += 1;
    }
    result
}

fn hex_char_of_value(value: i32) -> u8 {
    debug_assert!((0..=16).contains(&value));
    if value < 10 {
        return (value + '0' as i32) as u8;
    }
    (value - 10 + 'A' as i32) as u8
}

pub struct Bignum {
    bigits: [Chunk; K_BIGIT_CAPACITY],
    used_digits: i32,
    // O valor do Bignum é valor(bigits) * 2^(exponent * K_BIGIT_SIZE).
    exponent: i32,
}

impl Default for Bignum {
    fn default() -> Self {
        Self::new()
    }
}

impl Bignum {
    pub const K_MAX_SIGNIFICANT_BITS: i32 = K_MAX_SIGNIFICANT_BITS;

    pub fn new() -> Bignum {
        Bignum {
            bigits: [0; K_BIGIT_CAPACITY],
            used_digits: 0,
            exponent: 0,
        }
    }

    // Garantido caber em um bigit.
    pub fn assign_uint16(&mut self, value: u16) {
        debug_assert!(K_BIGIT_SIZE >= 16);
        self.zero();
        if value == 0 {
            return;
        }

        self.ensure_capacity(1);
        self.bigits[0] = value as Chunk;
        self.used_digits = 1;
    }

    pub fn assign_uint64(&mut self, mut value: u64) {
        const K_UINT64_SIZE: i32 = 64;

        self.zero();
        if value == 0 {
            return;
        }

        let needed_bigits = K_UINT64_SIZE / K_BIGIT_SIZE + 1;
        self.ensure_capacity(needed_bigits);
        for i in 0..needed_bigits {
            self.bigits[i as usize] = (value & K_BIGIT_MASK as u64) as Chunk;
            value >>= K_BIGIT_SIZE;
        }
        self.used_digits = needed_bigits;
        self.clamp();
    }

    pub fn assign_bignum(&mut self, other: &Bignum) {
        self.exponent = other.exponent;
        for i in 0..other.used_digits as usize {
            self.bigits[i] = other.bigits[i];
        }
        // Zera os dígitos excedentes (se havia).
        for i in other.used_digits as usize..self.used_digits as usize {
            self.bigits[i] = 0;
        }
        self.used_digits = other.used_digits;
    }

    pub fn assign_decimal_string(&mut self, value: &[u8]) {
        // 2^64 = 18446744073709551616 > 10^19
        const K_MAX_UINT64_DECIMAL_DIGITS: i32 = 19;
        self.zero();
        let mut length = value.len() as i32;
        let mut pos: i32 = 0;
        // Digamos que cada dígito precisa de 4 bits.
        while length >= K_MAX_UINT64_DECIMAL_DIGITS {
            let digits = read_uint64(value, pos, K_MAX_UINT64_DECIMAL_DIGITS);
            pos += K_MAX_UINT64_DECIMAL_DIGITS;
            length -= K_MAX_UINT64_DECIMAL_DIGITS;
            self.multiply_by_power_of_ten(K_MAX_UINT64_DECIMAL_DIGITS);
            self.add_uint64(digits);
        }
        let digits = read_uint64(value, pos, length);
        self.multiply_by_power_of_ten(length);
        self.add_uint64(digits);
        self.clamp();
    }

    pub fn assign_hex_string(&mut self, value: &[u8]) {
        self.zero();
        let length = value.len() as i32;

        let needed_bigits = length * 4 / K_BIGIT_SIZE + 1;
        self.ensure_capacity(needed_bigits);
        let mut string_index = length - 1;
        for i in 0..needed_bigits - 1 {
            // Estes bigits são garantidamente "cheios".
            let mut current_bigit: Chunk = 0;
            for j in 0..K_BIGIT_SIZE / 4 {
                let hex = to_ascii_hex_value(value[string_index as usize]) as Chunk;
                string_index -= 1;
                current_bigit = current_bigit.wrapping_add(hex << (j * 4));
            }
            self.bigits[i as usize] = current_bigit;
        }
        self.used_digits = needed_bigits - 1;

        let mut most_significant_bigit: Chunk = 0;
        let mut j = 0;
        while j <= string_index {
            most_significant_bigit <<= 4;
            most_significant_bigit =
                most_significant_bigit.wrapping_add(to_ascii_hex_value(value[j as usize]) as Chunk);
            j += 1;
        }
        if most_significant_bigit != 0 {
            self.bigits[self.used_digits as usize] = most_significant_bigit;
            self.used_digits += 1;
        }
        self.clamp();
    }

    pub fn add_uint64(&mut self, operand: u64) {
        if operand == 0 {
            return;
        }
        let mut other = Bignum::new();
        other.assign_uint64(operand);
        self.add_bignum(&other);
    }

    pub fn add_bignum(&mut self, other: &Bignum) {
        debug_assert!(self.is_clamped());
        debug_assert!(other.is_clamped());

        // Se este tem expoente maior que other, anexa bigits zero a este.
        // Depois da chamada, exponent <= other.exponent.
        self.align(other);

        // Há duas possibilidades:
        //   aaaaaaaaaaa 0000  (os 0s representam o expoente de a)
        //     bbbbb 00000000
        //   ----------------
        //   ccccccccccc 0000
        // ou
        //    aaaaaaaaaa 0000
        //  bbbbbbbbb 0000000
        //  -----------------
        //  cccccccccccc 0000
        // Nos dois casos pode ser preciso um bigit de vai-um.

        self.ensure_capacity(1 + self.bigit_length().max(other.bigit_length()) - self.exponent);
        let mut carry: Chunk = 0;
        let mut bigit_pos = other.exponent - self.exponent;
        debug_assert!(bigit_pos >= 0);
        for i in 0..other.used_digits as usize {
            let sum: Chunk = self.bigits[bigit_pos as usize]
                .wrapping_add(other.bigits[i])
                .wrapping_add(carry);
            self.bigits[bigit_pos as usize] = sum & K_BIGIT_MASK;
            carry = sum >> K_BIGIT_SIZE;
            bigit_pos += 1;
        }

        while carry != 0 {
            let sum: Chunk = self.bigits[bigit_pos as usize].wrapping_add(carry);
            self.bigits[bigit_pos as usize] = sum & K_BIGIT_MASK;
            carry = sum >> K_BIGIT_SIZE;
            bigit_pos += 1;
        }
        self.used_digits = bigit_pos.max(self.used_digits);
        debug_assert!(self.is_clamped());
    }

    // Precondição: this >= other.
    pub fn subtract_bignum(&mut self, other: &Bignum) {
        debug_assert!(self.is_clamped());
        debug_assert!(other.is_clamped());
        // Exigimos que this seja maior que other.
        debug_assert!(Bignum::less_equal(other, self));

        self.align(other);

        let offset = other.exponent - self.exponent;
        let mut borrow: Chunk = 0;
        let mut i: i32 = 0;
        while i < other.used_digits {
            debug_assert!(borrow == 0 || borrow == 1);
            let difference: Chunk = self.bigits[(i + offset) as usize]
                .wrapping_sub(other.bigits[i as usize])
                .wrapping_sub(borrow);
            self.bigits[(i + offset) as usize] = difference & K_BIGIT_MASK;
            borrow = difference >> (K_CHUNK_SIZE - 1);
            i += 1;
        }
        while borrow != 0 {
            let difference: Chunk = self.bigits[(i + offset) as usize].wrapping_sub(borrow);
            self.bigits[(i + offset) as usize] = difference & K_BIGIT_MASK;
            borrow = difference >> (K_CHUNK_SIZE - 1);
            i += 1;
        }
        self.clamp();
    }

    pub fn shift_left(&mut self, shift_amount: i32) {
        if self.used_digits == 0 {
            return;
        }
        self.exponent += shift_amount / K_BIGIT_SIZE;
        let local_shift = shift_amount % K_BIGIT_SIZE;
        self.ensure_capacity(self.used_digits + 1);
        self.bigits_shift_left(local_shift);
    }

    pub fn multiply_by_uint32(&mut self, factor: u32) {
        if factor == 1 {
            return;
        }
        if factor == 0 {
            self.zero();
            return;
        }
        if self.used_digits == 0 {
            return;
        }

        // O produto de um bigit pelo fator tem K_BIGIT_SIZE + 32 bits. O número
        // mais 1 (do vai-um) tem de caber em um double chunk.
        debug_assert!(64 >= K_BIGIT_SIZE + 32 + 1);
        let mut carry: DoubleChunk = 0;
        for i in 0..self.used_digits as usize {
            let product: DoubleChunk = (factor as DoubleChunk)
                .wrapping_mul(self.bigits[i] as DoubleChunk)
                .wrapping_add(carry);
            self.bigits[i] = (product & K_BIGIT_MASK as DoubleChunk) as Chunk;
            carry = product >> K_BIGIT_SIZE;
        }
        while carry != 0 {
            self.ensure_capacity(self.used_digits + 1);
            self.bigits[self.used_digits as usize] = (carry & K_BIGIT_MASK as DoubleChunk) as Chunk;
            self.used_digits += 1;
            carry >>= K_BIGIT_SIZE;
        }
    }

    pub fn multiply_by_uint64(&mut self, factor: u64) {
        if factor == 1 {
            return;
        }
        if factor == 0 {
            self.zero();
            return;
        }
        debug_assert!(K_BIGIT_SIZE < 32);
        let mut carry: u64 = 0;
        let low: u64 = factor & 0xFFFF_FFFF;
        let high: u64 = factor >> 32;
        for i in 0..self.used_digits as usize {
            let product_low: u64 = low.wrapping_mul(self.bigits[i] as u64);
            let product_high: u64 = high.wrapping_mul(self.bigits[i] as u64);
            let tmp: u64 = (carry & K_BIGIT_MASK as u64).wrapping_add(product_low);
            self.bigits[i] = (tmp & K_BIGIT_MASK as u64) as Chunk;
            carry = (carry >> K_BIGIT_SIZE)
                .wrapping_add(tmp >> K_BIGIT_SIZE)
                .wrapping_add(product_high << (32 - K_BIGIT_SIZE));
        }
        while carry != 0 {
            self.ensure_capacity(self.used_digits + 1);
            self.bigits[self.used_digits as usize] = (carry & K_BIGIT_MASK as u64) as Chunk;
            self.used_digits += 1;
            carry >>= K_BIGIT_SIZE;
        }
    }

    pub fn multiply_by_power_of_ten(&mut self, exponent: i32) {
        const K_FIVE27: u64 = 0x6765c793_fa10079d;
        const K_FIVE1: u16 = 5;
        const K_FIVE2: u16 = K_FIVE1 * 5;
        const K_FIVE3: u16 = K_FIVE2 * 5;
        const K_FIVE4: u16 = K_FIVE3 * 5;
        const K_FIVE5: u16 = K_FIVE4 * 5;
        const K_FIVE6: u16 = K_FIVE5 * 5;
        const K_FIVE7: u32 = K_FIVE6 as u32 * 5;
        const K_FIVE8: u32 = K_FIVE7 * 5;
        const K_FIVE9: u32 = K_FIVE8 * 5;
        const K_FIVE10: u32 = K_FIVE9 * 5;
        const K_FIVE11: u32 = K_FIVE10 * 5;
        const K_FIVE12: u32 = K_FIVE11 * 5;
        const K_FIVE13: u32 = K_FIVE12 * 5;
        const K_FIVE1_TO_12: [u32; 12] = [
            K_FIVE1 as u32,
            K_FIVE2 as u32,
            K_FIVE3 as u32,
            K_FIVE4 as u32,
            K_FIVE5 as u32,
            K_FIVE6 as u32,
            K_FIVE7,
            K_FIVE8,
            K_FIVE9,
            K_FIVE10,
            K_FIVE11,
            K_FIVE12,
        ];

        debug_assert!(exponent >= 0);
        if exponent == 0 {
            return;
        }
        if self.used_digits == 0 {
            return;
        }

        // Deslocamos por exponent no fim, logo antes de retornar.
        let mut remaining_exponent = exponent;
        while remaining_exponent >= 27 {
            self.multiply_by_uint64(K_FIVE27);
            remaining_exponent -= 27;
        }
        while remaining_exponent >= 13 {
            self.multiply_by_uint32(K_FIVE13);
            remaining_exponent -= 13;
        }
        if remaining_exponent > 0 {
            self.multiply_by_uint32(K_FIVE1_TO_12[(remaining_exponent - 1) as usize]);
        }
        self.shift_left(exponent);
    }

    pub fn times10(&mut self) {
        self.multiply_by_uint32(10)
    }

    pub fn square(&mut self) {
        debug_assert!(self.is_clamped());
        let product_length = 2 * self.used_digits;
        self.ensure_capacity(product_length);

        // Multiplicação de Comba: calcula cada coluna separadamente.
        // Exemplo: r = a2a1a0 * b2b1b0.
        //    r =  1    * a0b0 +
        //        10    * (a1b0 + a0b1) +
        //        100   * (a2b0 + a1b1 + a0b2) +
        //        1000  * (a2b1 + a1b2) +
        //        10000 * a2b2
        //
        // No pior caso acumulamos nb-digits produtos de dígito*dígito.
        //
        // Os bits adicionais de um DoubleChunk precisam bastar para somar
        // used_digits de Bigit*Bigit.
        if (1 << (2 * (K_CHUNK_SIZE - K_BIGIT_SIZE))) <= self.used_digits {
            // UNIMPLEMENTED() é ASSERT_NOT_REACHED(): só vale em depuração.
            debug_assert!(false, "UNIMPLEMENTED");
        }
        let mut accumulator: DoubleChunk = 0;
        // Primeiro desloca os dígitos para não sobrescrevê-los.
        let copy_offset = self.used_digits;
        for i in 0..self.used_digits {
            self.bigits[(copy_offset + i) as usize] = self.bigits[i as usize];
        }
        // Dois laços para evitar alguns 'if' dentro do laço.
        for i in 0..self.used_digits {
            // Processa o dígito temporário i com potência i.
            // A soma dos dois índices tem de ser igual a i.
            let mut bigit_index1 = i;
            let mut bigit_index2 = 0;
            // Soma todos os subprodutos.
            while bigit_index1 >= 0 {
                let chunk1: Chunk = self.bigits[(copy_offset + bigit_index1) as usize];
                let chunk2: Chunk = self.bigits[(copy_offset + bigit_index2) as usize];
                accumulator = accumulator
                    .wrapping_add((chunk1 as DoubleChunk).wrapping_mul(chunk2 as DoubleChunk));
                bigit_index1 -= 1;
                bigit_index2 += 1;
            }
            self.bigits[i as usize] = (accumulator as Chunk) & K_BIGIT_MASK;
            accumulator >>= K_BIGIT_SIZE;
        }
        for i in self.used_digits..product_length {
            let mut bigit_index1 = self.used_digits - 1;
            let mut bigit_index2 = i - bigit_index1;
            // Invariante: a soma dos dois índices continua igual a i.
            // O laço interno roda 0 vezes na última iteração, esvaziando o acumulador.
            while bigit_index2 < self.used_digits {
                let chunk1: Chunk = self.bigits[(copy_offset + bigit_index1) as usize];
                let chunk2: Chunk = self.bigits[(copy_offset + bigit_index2) as usize];
                accumulator = accumulator
                    .wrapping_add((chunk1 as DoubleChunk).wrapping_mul(chunk2 as DoubleChunk));
                bigit_index1 -= 1;
                bigit_index2 += 1;
            }
            // O bigits[i] sobrescrito nunca será lido nas iterações seguintes,
            // porque bigit_index1 e bigit_index2 são sempre maiores que
            // i - used_digits.
            self.bigits[i as usize] = (accumulator as Chunk) & K_BIGIT_MASK;
            accumulator >>= K_BIGIT_SIZE;
        }
        // Como o resultado cabe dentro do número, o acumulador deve ser 0 agora.
        debug_assert!(accumulator == 0);

        // Não esquecer de atualizar used_digits e o expoente.
        self.used_digits = product_length;
        self.exponent *= 2;
        self.clamp();
    }

    /// `AssignPowerUInt16`.
    pub fn assign_power_uint16(&mut self, mut base: u16, power_exponent: i32) {
        debug_assert!(base != 0);
        debug_assert!(power_exponent >= 0);
        if power_exponent == 0 {
            self.assign_uint16(1);
            return;
        }
        self.zero();
        let mut shifts: i32 = 0;
        // Esperamos base no intervalo 2-32, e na maioria das vezes 10.
        // Não vale a pena implementar algoritmos diferentes para contar os bits.
        while (base & 1) == 0 {
            base >>= 1;
            shifts += 1;
        }
        let mut bit_size: i32 = 0;
        let mut tmp_base: i32 = base as i32;
        while tmp_base != 0 {
            tmp_base >>= 1;
            bit_size += 1;
        }
        let final_size = bit_size * power_exponent;
        // 1 bigit extra para o deslocamento e outro para o final_size arredondado.
        self.ensure_capacity(final_size / K_BIGIT_SIZE + 2);

        // Exponenciação da esquerda para a direita.
        let mut mask: i32 = 1;
        while power_exponent >= mask {
            mask <<= 1;
        }

        // A máscara aponta agora para o bit acima do 1-bit mais significativo de
        // power_exponent. Descarta o primeiro 1-bit.
        mask >>= 2;
        let mut this_value: u64 = base as u64;

        let mut delayed_multiplication = false;
        let max_32bits: u64 = 0xFFFF_FFFF;
        while mask != 0 && this_value <= max_32bits {
            this_value = this_value.wrapping_mul(this_value);
            // Verifica que há espaço em this_value para a multiplicação.
            // Os primeiros bit_size bits têm de ser 0.
            if (power_exponent & mask) != 0 {
                debug_assert!(bit_size > 0);
                let base_bits_mask: u64 = !((1u64 << (64 - bit_size)) - 1);
                let high_bits_zero = (this_value & base_bits_mask) == 0;
                if high_bits_zero {
                    this_value = this_value.wrapping_mul(base as u64);
                } else {
                    delayed_multiplication = true;
                }
            }
            mask >>= 1;
        }
        self.assign_uint64(this_value);
        if delayed_multiplication {
            self.multiply_by_uint32(base as u32);
        }

        // Agora faz o mesmo como bignum.
        while mask != 0 {
            self.square();
            if (power_exponent & mask) != 0 {
                self.multiply_by_uint32(base as u32);
            }
            mask >>= 1;
        }

        // E por fim soma os deslocamentos guardados.
        self.shift_left(shifts * power_exponent);
    }

    // Precondição: this/other < 16 bits.
    pub fn divide_modulo_int_bignum(&mut self, other: &Bignum) -> u16 {
        debug_assert!(self.is_clamped());
        debug_assert!(other.is_clamped());
        debug_assert!(other.used_digits > 0);

        // Caso fácil: se temos menos dígitos que o divisor, o resultado é 0.
        // Nota: isso cobre também o caso this == 0.
        if self.bigit_length() < other.bigit_length() {
            return 0;
        }

        self.align(other);

        let mut result: u16 = 0;

        // Começa removendo múltiplos de 'other' até os dois números terem a
        // mesma quantidade de dígitos.
        while self.bigit_length() > other.bigit_length() {
            // Esta abordagem ingênua é extremamente ineficiente se `this` dividido
            // por other for grande. A função existe para doubleToString, em que
            // o resultado deve ser pequeno (menor que 10).
            debug_assert!(
                other.bigits[(other.used_digits - 1) as usize] >= ((1 << K_BIGIT_SIZE) / 16)
            );
            debug_assert!(self.bigits[(self.used_digits - 1) as usize] < 0x10000);
            // Remove os múltiplos do primeiro dígito.
            // Exemplo: this = 23 e other = 9 -> remove 2 múltiplos.
            let top = self.bigits[(self.used_digits - 1) as usize];
            result = result.wrapping_add(top as u16);
            self.subtract_times(other, top as i32);
        }

        debug_assert!(self.bigit_length() == other.bigit_length());

        // Os dois bignums têm o mesmo tamanho agora.
        // Como other tem mais de 0 dígitos, o acesso a bigits[used_digits - 1]
        // é seguro.
        let this_bigit: Chunk = self.bigits[(self.used_digits - 1) as usize];
        let other_bigit: Chunk = other.bigits[(other.used_digits - 1) as usize];

        if other.used_digits == 1 {
            // Atalho para o caso fácil (e comum).
            let quotient: i32 = (this_bigit / other_bigit) as i32;
            self.bigits[(self.used_digits - 1) as usize] =
                this_bigit.wrapping_sub(other_bigit.wrapping_mul(quotient as Chunk));
            debug_assert!(quotient < 0x10000);
            result = result.wrapping_add(quotient as u16);
            self.clamp();
            return result;
        }

        let division_estimate: i32 = (this_bigit / (other_bigit + 1)) as i32;
        debug_assert!(division_estimate < 0x10000);
        result = result.wrapping_add(division_estimate as u16);
        self.subtract_times(other, division_estimate);

        if other_bigit.wrapping_mul((division_estimate + 1) as Chunk) > this_bigit {
            // Nem vale tentar subtrair. Mesmo que os dígitos restantes de other
            // fossem 0, outra subtração seria demais.
            return result;
        }

        while Bignum::less_equal(other, self) {
            self.subtract_bignum(other);
            result = result.wrapping_add(1);
        }
        result
    }

    /// `ToHexString`: escreve em `buffer` o texto hexadecimal terminado em NUL.
    pub fn to_hex_string(&self, buffer: &mut [u8]) -> bool {
        debug_assert!(self.is_clamped());
        // Cada bigit precisa ser imprimível como caracteres hexadecimais inteiros.
        debug_assert!(K_BIGIT_SIZE % 4 == 0);
        const K_HEX_CHARS_PER_BIGIT: i32 = K_BIGIT_SIZE / 4;

        if self.used_digits == 0 {
            if buffer.len() < 2 {
                return false;
            }
            buffer[0] = b'0';
            buffer[1] = 0;
            return true;
        }
        // Somamos 1 para o '\0' terminador.
        let needed_chars = (self.bigit_length() - 1) * K_HEX_CHARS_PER_BIGIT
            + size_in_hex_chars(self.bigits[(self.used_digits - 1) as usize])
            + 1;
        if needed_chars > buffer.len() as i32 {
            return false;
        }
        let mut string_index = needed_chars - 1;
        buffer[string_index as usize] = 0;
        string_index -= 1;
        for _ in 0..self.exponent {
            for _ in 0..K_HEX_CHARS_PER_BIGIT {
                buffer[string_index as usize] = b'0';
                string_index -= 1;
            }
        }
        for i in 0..(self.used_digits - 1) as usize {
            let mut current_bigit: Chunk = self.bigits[i];
            for _ in 0..K_HEX_CHARS_PER_BIGIT {
                buffer[string_index as usize] = hex_char_of_value((current_bigit & 0xF) as i32);
                string_index -= 1;
                current_bigit >>= 4;
            }
        }
        // E por fim o último bigit.
        let mut most_significant_bigit: Chunk = self.bigits[(self.used_digits - 1) as usize];
        while most_significant_bigit != 0 {
            buffer[string_index as usize] = hex_char_of_value((most_significant_bigit & 0xF) as i32);
            string_index -= 1;
            most_significant_bigit >>= 4;
        }
        true
    }

    fn bigit_at(&self, index: i32) -> Chunk {
        if index >= self.bigit_length() {
            return 0;
        }
        if index < self.exponent {
            return 0;
        }
        self.bigits[(index - self.exponent) as usize]
    }

    // Retorna
    //  -1 se a < b,
    //   0 se a == b, e
    //  +1 se a > b.
    pub fn compare(a: &Bignum, b: &Bignum) -> i32 {
        debug_assert!(a.is_clamped());
        debug_assert!(b.is_clamped());
        let bigit_length_a = a.bigit_length();
        let bigit_length_b = b.bigit_length();
        if bigit_length_a < bigit_length_b {
            return -1;
        }
        if bigit_length_a > bigit_length_b {
            return 1;
        }
        let mut i = bigit_length_a - 1;
        while i >= a.exponent.min(b.exponent) {
            let bigit_a: Chunk = a.bigit_at(i);
            let bigit_b: Chunk = b.bigit_at(i);
            if bigit_a < bigit_b {
                return -1;
            }
            if bigit_a > bigit_b {
                return 1;
            }
            // Senão são iguais até este dígito. Tenta o próximo.
            i -= 1;
        }
        0
    }

    pub fn equal(a: &Bignum, b: &Bignum) -> bool {
        Bignum::compare(a, b) == 0
    }

    pub fn less_equal(a: &Bignum, b: &Bignum) -> bool {
        Bignum::compare(a, b) <= 0
    }

    pub fn less(a: &Bignum, b: &Bignum) -> bool {
        Bignum::compare(a, b) < 0
    }

    // Retorna Compare(a + b, c).
    pub fn plus_compare(a: &Bignum, b: &Bignum, c: &Bignum) -> i32 {
        debug_assert!(a.is_clamped());
        debug_assert!(b.is_clamped());
        debug_assert!(c.is_clamped());
        if a.bigit_length() < b.bigit_length() {
            return Bignum::plus_compare(b, a, c);
        }
        if a.bigit_length() + 1 < c.bigit_length() {
            return -1;
        }
        if a.bigit_length() > c.bigit_length() {
            return 1;
        }
        // O expoente codifica bigits 0. Se há mais dígitos 0 em 'a' do que
        // dígitos em 'b', o tamanho de 'a'+'b' em bigits é igual ao de 'a'.
        if a.exponent >= b.bigit_length() && a.bigit_length() < c.bigit_length() {
            return -1;
        }

        let mut borrow: Chunk = 0;
        // A partir de min_exponent todos os dígitos são == 0. Não precisa comparar.
        let min_exponent = a.exponent.min(b.exponent).min(c.exponent);
        let mut i = c.bigit_length() - 1;
        while i >= min_exponent {
            let chunk_a: Chunk = a.bigit_at(i);
            let chunk_b: Chunk = b.bigit_at(i);
            let chunk_c: Chunk = c.bigit_at(i);
            let sum: Chunk = chunk_a.wrapping_add(chunk_b);
            if sum > chunk_c.wrapping_add(borrow) {
                return 1;
            } else {
                borrow = chunk_c.wrapping_add(borrow).wrapping_sub(sum);
                if borrow > 1 {
                    return -1;
                }
                borrow <<= K_BIGIT_SIZE;
            }
            i -= 1;
        }
        if borrow == 0 {
            return 0;
        }
        -1
    }

    // Retorna a + b == c.
    pub fn plus_equal(a: &Bignum, b: &Bignum, c: &Bignum) -> bool {
        Bignum::plus_compare(a, b, c) == 0
    }

    // Retorna a + b <= c.
    pub fn plus_less_equal(a: &Bignum, b: &Bignum, c: &Bignum) -> bool {
        Bignum::plus_compare(a, b, c) <= 0
    }

    // Retorna a + b < c.
    pub fn plus_less(a: &Bignum, b: &Bignum, c: &Bignum) -> bool {
        Bignum::plus_compare(a, b, c) < 0
    }

    fn ensure_capacity(&self, size: i32) {
        if size > K_BIGIT_CAPACITY as i32 {
            // UNREACHABLE() é abort().
            panic!("Bignum: capacidade excedida");
        }
    }

    fn clamp(&mut self) {
        while self.used_digits > 0 && self.bigits[(self.used_digits - 1) as usize] == 0 {
            self.used_digits -= 1;
        }
        if self.used_digits == 0 {
            // Zero.
            self.exponent = 0;
        }
    }

    fn is_clamped(&self) -> bool {
        self.used_digits == 0 || self.bigits[(self.used_digits - 1) as usize] != 0
    }

    fn zero(&mut self) {
        for i in 0..self.used_digits as usize {
            self.bigits[i] = 0;
        }
        self.used_digits = 0;
        self.exponent = 0;
    }

    fn align(&mut self, other: &Bignum) {
        if self.exponent > other.exponent {
            // Se "X" representa um dígito "escondido" (pelo expoente), estamos no
            // seguinte caso (a == this, b == other):
            // a:  aaaaaaXXXX   ou a:   aaaaaXXX
            // b:     bbbbbbX      b: bbbbbbbbXX
            // Trocamos alguns dos dígitos escondidos (X) de a por dígitos 0.
            // a:  aaaaaa000X   ou a:   aaaaa0XX
            let zero_digits = self.exponent - other.exponent;
            self.ensure_capacity(self.used_digits + zero_digits);
            let mut i = self.used_digits - 1;
            while i >= 0 {
                self.bigits[(i + zero_digits) as usize] = self.bigits[i as usize];
                i -= 1;
            }
            for i in 0..zero_digits {
                self.bigits[i as usize] = 0;
            }
            self.used_digits += zero_digits;
            self.exponent -= zero_digits;
            debug_assert!(self.used_digits >= 0);
            debug_assert!(self.exponent >= 0);
        }
    }

    // Exige que this tenha capacidade suficiente (sem testes).
    // Atualiza used_digits se preciso.
    // shift_amount tem de ser < K_BIGIT_SIZE.
    fn bigits_shift_left(&mut self, shift_amount: i32) {
        debug_assert!(shift_amount < K_BIGIT_SIZE);
        debug_assert!(shift_amount >= 0);
        let mut carry: Chunk = 0;
        for i in 0..self.used_digits as usize {
            let new_carry: Chunk = self.bigits[i] >> (K_BIGIT_SIZE - shift_amount);
            self.bigits[i] = ((self.bigits[i] << shift_amount).wrapping_add(carry)) & K_BIGIT_MASK;
            carry = new_carry;
        }
        if carry != 0 {
            self.bigits[self.used_digits as usize] = carry;
            self.used_digits += 1;
        }
    }

    // BigitLength inclui os dígitos "escondidos" codificados no expoente.
    fn bigit_length(&self) -> i32 {
        self.used_digits + self.exponent
    }

    fn subtract_times(&mut self, other: &Bignum, factor: i32) {
        debug_assert!(self.exponent <= other.exponent);
        if factor < 3 {
            for _ in 0..factor {
                self.subtract_bignum(other);
            }
            return;
        }
        let mut borrow: Chunk = 0;
        let exponent_diff = other.exponent - self.exponent;
        for i in 0..other.used_digits {
            let product: DoubleChunk =
                (factor as DoubleChunk).wrapping_mul(other.bigits[i as usize] as DoubleChunk);
            let remove: DoubleChunk = (borrow as DoubleChunk).wrapping_add(product);
            // A subtração é feita em 64 bits e truncada para Chunk, como no C++.
            let difference: Chunk = (self.bigits[(i + exponent_diff) as usize] as DoubleChunk)
                .wrapping_sub(remove & K_BIGIT_MASK as DoubleChunk)
                as Chunk;
            self.bigits[(i + exponent_diff) as usize] = difference & K_BIGIT_MASK;
            borrow = (((difference >> (K_CHUNK_SIZE - 1)) as DoubleChunk)
                .wrapping_add(remove >> K_BIGIT_SIZE)) as Chunk;
        }
        let mut i = other.used_digits + exponent_diff;
        while i < self.used_digits {
            if borrow == 0 {
                return;
            }
            let difference: Chunk = self.bigits[i as usize].wrapping_sub(borrow);
            self.bigits[i as usize] = difference & K_BIGIT_MASK;
            borrow = difference >> (K_CHUNK_SIZE - 1);
            i += 1;
        }
        self.clamp();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &Bignum) -> String {
        let mut buffer = [0u8; 128];
        assert!(b.to_hex_string(&mut buffer));
        let end = buffer.iter().position(|&c| c == 0).unwrap();
        String::from_utf8(buffer[..end].to_vec()).unwrap()
    }

    #[test]
    fn assign_decimal_string_and_hex() {
        let mut b = Bignum::new();
        b.assign_decimal_string(b"12345");
        assert_eq!(hex(&b), "3039");
        b.assign_decimal_string(b"0");
        assert_eq!(hex(&b), "0");
        b.assign_decimal_string(b"100000000000000000000");
        assert_eq!(hex(&b), "56BC75E2D63100000");
    }

    #[test]
    fn assign_integers() {
        let mut b = Bignum::new();
        b.assign_uint16(0xFFFF);
        assert_eq!(hex(&b), "FFFF");
        b.assign_uint64(0x0123_4567_89AB_CDEF);
        assert_eq!(hex(&b), "123456789ABCDEF".to_uppercase());
        b.assign_uint64(0);
        assert_eq!(hex(&b), "0");
    }

    #[test]
    fn square_of_u32_max() {
        let mut b = Bignum::new();
        b.assign_uint64(0xFFFF_FFFF);
        b.square();
        assert_eq!(hex(&b), "FFFFFFFE00000001");
    }

    #[test]
    fn power_and_shift() {
        let mut b = Bignum::new();
        b.assign_power_uint16(10, 20);
        assert_eq!(hex(&b), "56BC75E2D63100000");
        let mut c = Bignum::new();
        c.assign_uint16(1);
        c.shift_left(100);
        assert_eq!(hex(&c), "10000000000000000000000000");
    }

    #[test]
    fn add_subtract_compare() {
        let mut a = Bignum::new();
        let mut b = Bignum::new();
        a.assign_uint64(0xFFFF_FFFF_FFFF_FFFF);
        b.assign_uint64(1);
        let mut c = Bignum::new();
        c.assign_bignum(&a);
        c.add_bignum(&b);
        assert_eq!(hex(&c), "10000000000000000");
        assert_eq!(Bignum::compare(&a, &c), -1);
        assert!(Bignum::less(&a, &c));
        assert!(Bignum::plus_equal(&a, &b, &c));
        c.subtract_bignum(&b);
        assert!(Bignum::equal(&a, &c));
    }

    #[test]
    fn multiply_and_divide() {
        let mut a = Bignum::new();
        a.assign_uint64(23);
        let mut b = Bignum::new();
        b.assign_uint64(9);
        assert_eq!(a.divide_modulo_int_bignum(&b), 2);
        assert_eq!(hex(&a), "5");

        let mut m = Bignum::new();
        m.assign_uint64(0xFFFF_FFFF);
        m.multiply_by_uint32(0xFFFF_FFFF);
        assert_eq!(hex(&m), "FFFFFFFE00000001");
        m.assign_uint64(0xFFFF_FFFF);
        m.multiply_by_uint64(0xFFFF_FFFF_FFFF_FFFF);
        assert_eq!(hex(&m), "FFFFFFFEFFFFFFFF00000001");
        m.assign_uint64(7);
        m.times10();
        assert_eq!(hex(&m), "46");
    }

    #[test]
    fn hex_string_roundtrip() {
        let mut b = Bignum::new();
        b.assign_hex_string(b"123456789ABCDEF0123456789");
        assert_eq!(hex(&b), "123456789ABCDEF0123456789");
        let mut small = [0u8; 3];
        assert!(!b.to_hex_string(&mut small));
    }
}
