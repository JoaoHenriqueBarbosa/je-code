// Tradução de WTF/wtf/dtoa/fixed-dtoa.h e fixed-dtoa.cc (double-conversion, V8 e Apple).
//
// Mapeamentos: `BufferReference<char>` vira `&mut [u8]`; `int&` vira `&mut i32`; as operações
// unsigned que podem dar a volta no C++ viram `wrapping_*`. Os `ASSERT` viram `debug_assert!`.

use crate::wtf::dtoa::ieee::Double;
use crate::wtf::dtoa::utils::uint64_2part_c;

/// Representa um tipo de 128 bits. Esta classe deveria ser trocada por um tipo nativo nas
/// plataformas que têm inteiro de 128 bits; aqui segue fiel ao C++.
struct UInt128 {
    // Valor == (high_bits << 64) + low_bits
    high_bits: u64,
    low_bits: u64,
}

const MASK32: u64 = 0xFFFFFFFF;

impl UInt128 {
    fn new(high: u64, low: u64) -> Self {
        UInt128 {
            high_bits: high,
            low_bits: low,
        }
    }

    fn multiply(&mut self, multiplicand: u32) {
        let multiplicand = multiplicand as u64;
        let mut accumulator: u64;

        accumulator = (self.low_bits & MASK32).wrapping_mul(multiplicand);
        let mut part = (accumulator & MASK32) as u32;
        accumulator >>= 32;
        accumulator = accumulator.wrapping_add((self.low_bits >> 32).wrapping_mul(multiplicand));
        self.low_bits = (accumulator << 32).wrapping_add(part as u64);
        accumulator >>= 32;
        accumulator = accumulator.wrapping_add((self.high_bits & MASK32).wrapping_mul(multiplicand));
        part = (accumulator & MASK32) as u32;
        accumulator >>= 32;
        accumulator = accumulator.wrapping_add((self.high_bits >> 32).wrapping_mul(multiplicand));
        self.high_bits = (accumulator << 32).wrapping_add(part as u64);
        debug_assert!((accumulator >> 32) == 0);
    }

    fn shift(&mut self, shift_amount: i32) {
        debug_assert!(-64 <= shift_amount && shift_amount <= 64);
        if shift_amount == 0 {
        } else if shift_amount == -64 {
            self.high_bits = self.low_bits;
            self.low_bits = 0;
        } else if shift_amount == 64 {
            self.low_bits = self.high_bits;
            self.high_bits = 0;
        } else if shift_amount <= 0 {
            self.high_bits = self.high_bits.wrapping_shl((-shift_amount) as u32);
            self.high_bits = self
                .high_bits
                .wrapping_add(self.low_bits.wrapping_shr((64 + shift_amount) as u32));
            self.low_bits = self.low_bits.wrapping_shl((-shift_amount) as u32);
        } else {
            self.low_bits = self.low_bits.wrapping_shr(shift_amount as u32);
            self.low_bits = self
                .low_bits
                .wrapping_add(self.high_bits.wrapping_shl((64 - shift_amount) as u32));
            self.high_bits = self.high_bits.wrapping_shr(shift_amount as u32);
        }
    }

    // Modifica *this para *this MOD (2^power).
    // Devolve *this DIV (2^power).
    fn div_mod_power_of_2(&mut self, power: i32) -> i32 {
        if power >= 64 {
            let result = self.high_bits.wrapping_shr((power - 64) as u32) as i32;
            self.high_bits = self
                .high_bits
                .wrapping_sub((result as i64 as u64).wrapping_shl((power - 64) as u32));
            result
        } else {
            let part_low = self.low_bits.wrapping_shr(power as u32);
            let part_high = self.high_bits.wrapping_shl((64 - power) as u32);
            let result = part_low.wrapping_add(part_high) as i32;
            self.high_bits = 0;
            self.low_bits = self.low_bits.wrapping_sub(part_low.wrapping_shl(power as u32));
            result
        }
    }

    fn is_zero(&self) -> bool {
        self.high_bits == 0 && self.low_bits == 0
    }

    fn bit_at(&self, position: i32) -> i32 {
        if position >= 64 {
            (self.high_bits.wrapping_shr((position - 64) as u32) as i32) & 1
        } else {
            (self.low_bits.wrapping_shr(position as u32) as i32) & 1
        }
    }
}

const DOUBLE_SIGNIFICAND_SIZE: i32 = 53; // Inclui o bit escondido.

fn fill_digits32_fixed_length(
    mut number: u32,
    requested_length: i32,
    buffer: &mut [u8],
    length: &mut i32,
) {
    let mut i = requested_length - 1;
    while i >= 0 {
        buffer[(*length + i) as usize] = b'0' + (number % 10) as u8;
        number /= 10;
        i -= 1;
    }
    *length += requested_length;
}

fn fill_digits32(mut number: u32, buffer: &mut [u8], length: &mut i32) {
    let mut number_length: i32 = 0;
    // Preenchemos os dígitos em ordem inversa e os trocamos depois.
    while number != 0 {
        let digit = (number % 10) as i32;
        number /= 10;
        buffer[(*length + number_length) as usize] = (b'0' as i32 + digit) as u8;
        number_length += 1;
    }
    // Troca os dígitos.
    let mut i = *length;
    let mut j = *length + number_length - 1;
    while i < j {
        buffer.swap(i as usize, j as usize);
        i += 1;
        j -= 1;
    }
    *length += number_length;
}

fn fill_digits64_fixed_length(mut number: u64, buffer: &mut [u8], length: &mut i32) {
    const TEN7: u32 = 10000000;
    // Por eficiência, corta o número em 3 partes uint32_t e imprime essas.
    let part2 = (number % TEN7 as u64) as u32;
    number /= TEN7 as u64;
    let part1 = (number % TEN7 as u64) as u32;
    let part0 = (number / TEN7 as u64) as u32;

    fill_digits32_fixed_length(part0, 3, buffer, length);
    fill_digits32_fixed_length(part1, 7, buffer, length);
    fill_digits32_fixed_length(part2, 7, buffer, length);
}

fn fill_digits64(mut number: u64, buffer: &mut [u8], length: &mut i32) {
    const TEN7: u32 = 10000000;
    // Por eficiência, corta o número em 3 partes uint32_t e imprime essas.
    let part2 = (number % TEN7 as u64) as u32;
    number /= TEN7 as u64;
    let part1 = (number % TEN7 as u64) as u32;
    let part0 = (number / TEN7 as u64) as u32;

    if part0 != 0 {
        fill_digits32(part0, buffer, length);
        fill_digits32_fixed_length(part1, 7, buffer, length);
        fill_digits32_fixed_length(part2, 7, buffer, length);
    } else if part1 != 0 {
        fill_digits32(part1, buffer, length);
        fill_digits32_fixed_length(part2, 7, buffer, length);
    } else {
        fill_digits32(part2, buffer, length);
    }
}

fn round_up(buffer: &mut [u8], length: &mut i32, decimal_point: &mut i32) {
    // Um buffer vazio representa 0.
    if *length == 0 {
        buffer[0] = b'1';
        *decimal_point = 1;
        *length = 1;
        return;
    }
    // Arredonda o último dígito até achar um dígito que não era '9' ou até chegar ao primeiro.
    let last = (*length - 1) as usize;
    buffer[last] = buffer[last].wrapping_add(1);
    let mut i = *length - 1;
    while i > 0 {
        if buffer[i as usize] != b'0' + 10 {
            return;
        }
        buffer[i as usize] = b'0';
        buffer[(i - 1) as usize] = buffer[(i - 1) as usize].wrapping_add(1);
        i -= 1;
    }
    // Se o primeiro dígito virou '0' + 10, precisaríamos pô-lo em '0' e acrescentar um '1' na
    // frente. Só chegamos ao primeiro dígito se todos os seguintes eram '9' antes do arredondamento.
    // Agora todos os finais são '0' e basta trocar o primeiro por '1' e atualizar o ponto decimal
    // (que agora está um dígito à direita).
    if buffer[0] == b'0' + 10 {
        buffer[0] = b'1';
        *decimal_point += 1;
    }
}

// O número `fractionals` representa um ponto fixo com o ponto binário no bit (-exponent).
// Pré-condições:
//   -128 <= exponent <= 0.
//   0 <= fractionals * 2^exponent < 1
//   O buffer guarda o resultado.
// A função arredonda o resultado. No arredondamento, dígitos não gerados por esta função podem
// ser atualizados, e a variável decimal_point também. Se esta função gera os dígitos 99 e o
// buffer já continha "199" (dando "19999"), o arredondamento muda o conteúdo para "20000".
fn fill_fractionals(
    mut fractionals: u64,
    exponent: i32,
    fractional_count: i32,
    buffer: &mut [u8],
    length: &mut i32,
    decimal_point: &mut i32,
) {
    debug_assert!(-128 <= exponent && exponent <= 0);
    // 'fractionals' é um ponto fixo com o ponto binário no bit (-exponent). Dentro da função o
    // resto não convertido de fractionals é um ponto fixo com o ponto binário no bit 'point'.
    if -exponent <= 64 {
        // Um número de 64 bits basta.
        debug_assert!(fractionals >> 56 == 0);
        let mut point = -exponent;
        let mut i = 0;
        while i < fractional_count {
            if fractionals == 0 {
                break;
            }
            // Em vez de multiplicar por 10, multiplicamos por 5 e ajustamos a posição do ponto.
            // Assim fractionals não estoura.
            fractionals = fractionals.wrapping_mul(5);
            point -= 1;
            let digit = fractionals.wrapping_shr(point as u32) as i32;
            debug_assert!(digit <= 9);
            buffer[*length as usize] = (b'0' as i32 + digit) as u8;
            *length += 1;
            fractionals = fractionals.wrapping_sub((digit as i64 as u64).wrapping_shl(point as u32));
            i += 1;
        }
        // Se o primeiro bit depois do ponto está ligado, arredondamos para cima.
        debug_assert!(fractionals == 0 || point - 1 >= 0);
        if fractionals != 0 && (fractionals.wrapping_shr((point - 1) as u32) & 1) == 1 {
            round_up(buffer, length, decimal_point);
        }
    } else {
        // Precisamos de 128 bits.
        debug_assert!(64 < -exponent && -exponent <= 128);
        let mut fractionals128 = UInt128::new(fractionals, 0);
        fractionals128.shift(-exponent - 64);
        let mut point = 128;
        let mut i = 0;
        while i < fractional_count {
            if fractionals128.is_zero() {
                break;
            }
            // Como antes: em vez de multiplicar por 10, multiplicamos por 5 e ajustamos o ponto.
            // Esta multiplicação não estoura pelos mesmos motivos.
            fractionals128.multiply(5);
            point -= 1;
            let digit = fractionals128.div_mod_power_of_2(point);
            debug_assert!(digit <= 9);
            buffer[*length as usize] = (b'0' as i32 + digit) as u8;
            *length += 1;
            i += 1;
        }
        if fractionals128.bit_at(point - 1) == 1 {
            round_up(buffer, length, decimal_point);
        }
    }
}

// Remove zeros iniciais e finais.
// Se removeu zeros iniciais, ajusta a posição do ponto decimal.
fn trim_zeros(buffer: &mut [u8], length: &mut i32, decimal_point: &mut i32) {
    while *length > 0 && buffer[(*length - 1) as usize] == b'0' {
        *length -= 1;
    }
    let mut first_non_zero: i32 = 0;
    while first_non_zero < *length && buffer[first_non_zero as usize] == b'0' {
        first_non_zero += 1;
    }
    if first_non_zero != 0 {
        let mut i = first_non_zero;
        while i < *length {
            buffer[(i - first_non_zero) as usize] = buffer[i as usize];
            i += 1;
        }
        *length -= first_non_zero;
        *decimal_point -= first_non_zero;
    }
}

/// Produz os dígitos necessários para imprimir um número com `fractional_count` dígitos depois
/// do ponto decimal. O buffer precisa caber o resultado mais um caractere nulo final.
///
/// Os dígitos produzidos podem ser curtos demais, e então o chamador completa com '0'.
/// Exemplo: `fast_fixed_dtoa(0.001, 5, ...)` pode devolver buffer = "1" e decimal_point = -2.
/// Casos de meio exato arredondam para +/-infinito (longe de 0).
///
/// Só funciona para alguns parâmetros. Se não consegue tratar a entrada, devolve `false`. A
/// saída é terminada em nulo quando a função tem sucesso.
pub fn fast_fixed_dtoa(
    v: f64,
    fractional_count: i32,
    buffer: &mut [u8],
    length: &mut i32,
    decimal_point: &mut i32,
) -> bool {
    const MAX_UINT32: u32 = 0xFFFFFFFF;
    let mut significand: u64 = Double::from_f64(v).significand();
    let exponent: i32 = Double::from_f64(v).exponent();
    // v = significand * 2^exponent (com significand um inteiro de 53 bits).
    // Se o expoente é maior que 20 (podemos ter um número de 73 bits), não sabemos calcular a
    // representação. 2^73 ~= 9.5*10^21.
    if exponent > 20 {
        return false;
    }
    if fractional_count > 20 {
        return false;
    }
    *length = 0;
    // No máximo DOUBLE_SIGNIFICAND_SIZE bits do significand são não nulos.
    // Num inteiro de 64 bits temos 11 zeros seguidos de 53 bits possivelmente não nulos.
    if exponent + DOUBLE_SIGNIFICAND_SIZE > 64 {
        // O expoente precisa ser > 11.
        //
        // Sabemos que v = significand * 2^exponent. Simplificamos dividindo v por 10^17. O
        // quociente entrega os primeiros dígitos, e o resto cabe num número de 64 bits.
        // Dividir por 10^17 equivale a dividir por 5^17*2^17.
        const FIVE17: u64 = uint64_2part_c(0xB1, 0xA2BC2EC5); // 5^17
        let mut divisor: u64 = FIVE17;
        let divisor_power: i32 = 17;
        let mut dividend: u64 = significand;
        let quotient: u32;
        let remainder: u64;
        // Seja v = f * 2^e com f == significand e e == exponent. Precisamos de q (quociente) e
        // r (resto) tais que:
        //   v            = q * 10^17       + r
        //   f * 2^e      = q * 10^17       + r
        //   f * 2^e      = q * 5^17 * 2^17 + r
        // Se e > 17 então
        //   f * 2^(e-17) = q * 5^17        + r/2^17
        // senão
        //   f  = q * 5^17 * 2^(17-e) + r/2^e
        if exponent > divisor_power {
            // Só permitimos expoentes de até 20, logo (17 - e) <= 3
            dividend <<= exponent - divisor_power;
            quotient = (dividend / divisor) as u32;
            remainder = (dividend % divisor) << divisor_power;
        } else {
            divisor <<= divisor_power - exponent;
            quotient = (dividend / divisor) as u32;
            remainder = (dividend % divisor) << exponent;
        }
        fill_digits32(quotient, buffer, length);
        fill_digits64_fixed_length(remainder, buffer, length);
        *decimal_point = *length;
    } else if exponent >= 0 {
        // 0 <= exponent <= 11
        significand <<= exponent;
        fill_digits64(significand, buffer, length);
        *decimal_point = *length;
    } else if exponent > -DOUBLE_SIGNIFICAND_SIZE {
        // Temos que cortar o número.
        let integrals: u64 = significand >> -exponent;
        let fractionals: u64 = significand - (integrals << -exponent);
        if integrals > MAX_UINT32 as u64 {
            fill_digits64(integrals, buffer, length);
        } else {
            fill_digits32(integrals as u32, buffer, length);
        }
        *decimal_point = *length;
        fill_fractionals(fractionals, exponent, fractional_count, buffer, length, decimal_point);
    } else if exponent < -128 {
        // Esta configuração (com no máximo 20 dígitos) significa que todos os dígitos são 0.
        debug_assert!(fractional_count <= 20);
        buffer[0] = b'\0';
        *length = 0;
        *decimal_point = -fractional_count;
    } else {
        *decimal_point = 0;
        fill_fractionals(significand, exponent, fractional_count, buffer, length, decimal_point);
    }
    trim_zeros(buffer, length, decimal_point);
    buffer[*length as usize] = b'\0';
    if *length == 0 {
        // A string está vazia e o decimal_point não tem importância. Imita o dtoa do Gay e o
        // põe em -fractional_count.
        *decimal_point = -fractional_count;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(v: f64, fractional_count: i32) -> (bool, String, i32) {
        let mut buffer = [0u8; 100];
        let mut length = 0;
        let mut decimal_point = 0;
        let ok = fast_fixed_dtoa(v, fractional_count, &mut buffer, &mut length, &mut decimal_point);
        let text = String::from_utf8_lossy(&buffer[..length as usize]).into_owned();
        (ok, text, decimal_point)
    }

    #[test]
    fn simple_values() {
        assert_eq!(run(1.5, 1), (true, "15".to_string(), 1));
        assert_eq!(run(0.001, 5), (true, "1".to_string(), -2));
        assert_eq!(run(123.456, 2), (true, "12346".to_string(), 3));
        assert_eq!(run(0.0, 3), (true, String::new(), -3));
    }

    #[test]
    fn rejects_unsupported() {
        assert!(!run(1e30, 1).0);
        assert!(!run(1.5, 21).0);
    }

    #[test]
    fn rounds_up_to_new_digit() {
        assert_eq!(run(0.999, 2), (true, "1".to_string(), 1));
    }
}
