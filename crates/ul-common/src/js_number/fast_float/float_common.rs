//! Porte de `WTF/wtf/fast_float/fast_float.h`, seções `FASTFLOAT_CONSTEXPR_FEATURE_DETECT_H` e
//! `FASTFLOAT_FLOAT_COMMON_H` (linhas 93 a 1603 do amálgama).
//!
//! Mapeamentos:
//!
//! - Macros de detecção de recurso (`FASTFLOAT_CONSTEXPR14/20`, `FASTFLOAT_HAS_BIT_CAST`,
//!   `FASTFLOAT_IF_CONSTEXPR17`, `fastfloat_really_inline`, `fastfloat_unlikely`, SIMD, endianness,
//!   `FASTFLOAT_64BIT`) somem: em Linux x86_64 valem 64 bits, little endian, e nenhuma delas muda o
//!   resultado. `FASTFLOAT_ASSERT`/`FASTFLOAT_DEBUG_ASSERT` avaliam o argumento e o descartam.
//! - `cpp20_and_in_constexpr()` é sempre falso em tempo de execução; os ramos `constexpr` genéricos
//!   (`leading_zeroes_generic`, `umul128_generic`...) ficam como funções próprias, como no C++.
//! - `UC` (tipo de caractere) vira genérico sobre `CharType` (`u8`/`u16`). `T` (`float`/`double`)
//!   vira genérico sobre o trait `BinaryFormat`, implementado para `f32` e `f64`.
//! - Os tipos `std::float16_t`/`std::bfloat16_t` só existem sob `__STDCPP_FLOAT16_T__`, que o libc++
//!   usado pelo Bun não define; os ramos somem.
//! - `from_chars_result_t<UC>` guarda um ponteiro; aqui o ponteiro é o índice (`ptr: usize`) no slice
//!   de entrada, então o struct não precisa do parâmetro `UC`.
//! - As sobrecargas por `T` das funções (`max_mantissa_fast_path()` e `max_mantissa_fast_path(power)`)
//!   viram dois métodos: `max_mantissa_fast_path` e `max_mantissa_fast_path_at`.
//! - Constantes viram `pub const` em maiúsculas (`invalid_am_bias` vira `INVALID_AM_BIAS`).
#![allow(non_camel_case_types, non_upper_case_globals)]

use crate::js_number::CharType;
use std::ops::{
    BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign, Div, Index, Mul, Neg, Not,
};

// ---------------------------------------------------------------------------------------------
// std::errc
// ---------------------------------------------------------------------------------------------

/// `std::errc`, reduzido aos valores que o fast_float usa. O valor `success` é o `std::errc()`
/// padrão (zero).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(i32)]
pub enum errc {
    #[default]
    success = 0,
    invalid_argument = 22,
    result_out_of_range = 34,
}

// ---------------------------------------------------------------------------------------------
// chars_format
// ---------------------------------------------------------------------------------------------

/// `enum class chars_format : uint64_t`, um conjunto de bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct chars_format(pub u64);

pub mod detail {
    use super::chars_format;

    pub const basic_json_fmt: chars_format = chars_format(1 << 5);
    pub const basic_fortran_fmt: chars_format = chars_format(1 << 6);

    /// `adjust_for_feature_macros`: `FASTFLOAT_ALLOWS_LEADING_PLUS` e `FASTFLOAT_SKIP_WHITE_SPACE`
    /// não são definidos pela WTF, então o formato volta como veio.
    pub const fn adjust_for_feature_macros(fmt: chars_format) -> chars_format {
        fmt
    }
}

impl chars_format {
    pub const scientific: chars_format = chars_format(1 << 0);
    pub const fixed: chars_format = chars_format(1 << 2);
    pub const hex: chars_format = chars_format(1 << 3);
    pub const no_infnan: chars_format = chars_format(1 << 4);
    // RFC 8259: https://datatracker.ietf.org/doc/html/rfc8259#section-6
    pub const json: chars_format = chars_format(
        detail::basic_json_fmt.0
            | Self::fixed.0
            | Self::scientific.0
            | Self::no_infnan.0,
    );
    // Extensão da RFC 8259 em que, por exemplo, "inf" e "nan" são permitidos.
    pub const json_or_infnan: chars_format =
        chars_format(detail::basic_json_fmt.0 | Self::fixed.0 | Self::scientific.0);
    pub const fortran: chars_format =
        chars_format(detail::basic_fortran_fmt.0 | Self::fixed.0 | Self::scientific.0);
    pub const general: chars_format = chars_format(Self::fixed.0 | Self::scientific.0);
    pub const allow_leading_plus: chars_format = chars_format(1 << 7);
    pub const skip_white_space: chars_format = chars_format(1 << 8);
}

impl Not for chars_format {
    type Output = chars_format;
    fn not(self) -> chars_format {
        chars_format(!self.0)
    }
}

impl BitAnd for chars_format {
    type Output = chars_format;
    fn bitand(self, rhs: chars_format) -> chars_format {
        chars_format(self.0 & rhs.0)
    }
}

impl BitOr for chars_format {
    type Output = chars_format;
    fn bitor(self, rhs: chars_format) -> chars_format {
        chars_format(self.0 | rhs.0)
    }
}

impl BitXor for chars_format {
    type Output = chars_format;
    fn bitxor(self, rhs: chars_format) -> chars_format {
        chars_format(self.0 ^ rhs.0)
    }
}

impl BitAndAssign for chars_format {
    fn bitand_assign(&mut self, rhs: chars_format) {
        *self = *self & rhs;
    }
}

impl BitOrAssign for chars_format {
    fn bitor_assign(&mut self, rhs: chars_format) {
        *self = *self | rhs;
    }
}

impl BitXorAssign for chars_format {
    fn bitxor_assign(&mut self, rhs: chars_format) {
        *self = *self ^ rhs;
    }
}

// ---------------------------------------------------------------------------------------------
// from_chars_result_t, parse_options_t
// ---------------------------------------------------------------------------------------------

/// `from_chars_result_t<UC>`: `ptr` é o índice, no slice de entrada, logo depois do número lido.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct from_chars_result_t {
    pub ptr: usize,
    pub ec: errc,
}

impl from_chars_result_t {
    /// `explicit operator bool`: verdadeiro quando `ec == std::errc()`.
    pub fn as_bool(&self) -> bool {
        self.ec == errc::success
    }
}

/// `using from_chars_result = from_chars_result_t<char>;`
pub type from_chars_result = from_chars_result_t;

/// `parse_options_t<UC>`.
#[derive(Clone, Copy, Debug)]
pub struct parse_options_t<UC: CharType> {
    /// Quais formatos de número são aceitos.
    pub format: chars_format,
    /// O caractere usado como ponto decimal.
    pub decimal_point: UC,
    /// A base usada para inteiros.
    pub base: i32,
}

impl<UC: CharType> parse_options_t<UC> {
    /// O construtor do C++; os argumentos padrão são `chars_format::general`, `UC('.')` e `10`
    /// (use `parse_options_t::default()`).
    pub fn new(fmt: chars_format, dot: UC, b: i32) -> Self {
        parse_options_t { format: fmt, decimal_point: dot, base: b }
    }
}

impl<UC: CharType> Default for parse_options_t<UC> {
    fn default() -> Self {
        Self::new(chars_format::general, UC::from_u16(b'.' as u16), 10)
    }
}

/// `using parse_options = parse_options_t<char>;`
pub type parse_options = parse_options_t<u8>;

// ---------------------------------------------------------------------------------------------
// Comparação ASCII sem distinção de caixa
// ---------------------------------------------------------------------------------------------

/// `cpp20_and_in_constexpr()`: `std::is_constant_evaluated()` é sempre falso em tempo de execução.
pub const fn cpp20_and_in_constexpr() -> bool {
    false
}

/// A máscara por unidade de código do C++ (`0x2020...`, `0x0020...`): o bit 5 de cada unidade.
/// As operações do C++ trabalham em palavras de 64 bits, mas o `|` não cruza unidades, então
/// comparar unidade a unidade é exatamente a mesma conta.
fn lane_mask<UC: CharType>() -> Option<u32> {
    if UC::SIZE == 1 || UC::SIZE == 2 || UC::SIZE == 4 {
        Some(0x20)
    } else {
        None
    }
}

/// O valor numérico do caractere (`UC` vira inteiro nas contas do C++).
fn code_of<UC: CharType>(c: UC) -> u32 {
    c.into()
}

fn masked_equal<UC: CharType>(actual: UC, expected: UC, mask: u32) -> bool {
    (code_of(actual) | mask) == (code_of(expected) | mask)
}

/// `(actual | 32) == expected`: o último caractere dos ramos de 16 e 32 bits não mascara o esperado.
fn or32_equals<UC: CharType>(actual: UC, expected: UC) -> bool {
    (code_of(actual) | 32) == code_of(expected)
}

/// `fastfloat_strncasecmp3`.
pub fn fastfloat_strncasecmp3<UC: CharType>(
    actual_mixedcase: &[UC],
    expected_lowercase: &[UC],
) -> bool {
    let Some(mask) = lane_mask::<UC>() else {
        return false;
    };
    // `cpp20_and_in_constexpr()` é falso, vale o ramo das palavras de 64 bits.
    if UC::SIZE == 1 || UC::SIZE == 2 {
        // memcpy de 3 unidades, OR com a máscara nos dois lados e comparação da palavra.
        for i in 0..3 {
            if !masked_equal(actual_mixedcase[i], expected_lowercase[i], mask) {
                return false;
            }
        }
        true
    } else if UC::SIZE == 4 {
        for i in 0..2 {
            if !masked_equal(actual_mixedcase[i], expected_lowercase[i], mask) {
                return false;
            }
        }
        or32_equals(actual_mixedcase[2], expected_lowercase[2])
    } else {
        false
    }
}

/// `fastfloat_strncasecmp5`.
pub fn fastfloat_strncasecmp5<UC: CharType>(
    actual_mixedcase: &[UC],
    expected_lowercase: &[UC],
) -> bool {
    // `cpp20_and_in_constexpr()` é falso, vale o ramo das palavras de 64 bits.
    if UC::SIZE == 1 {
        for i in 0..5 {
            if !masked_equal(actual_mixedcase[i], expected_lowercase[i], 0x20) {
                return false;
            }
        }
        true
    } else if UC::SIZE == 2 {
        for i in 0..4 {
            if !masked_equal(actual_mixedcase[i], expected_lowercase[i], 0x20) {
                return false;
            }
        }
        or32_equals(actual_mixedcase[4], expected_lowercase[4])
    } else if UC::SIZE == 4 {
        for i in 0..4 {
            if !masked_equal(actual_mixedcase[i], expected_lowercase[i], 0x20) {
                return false;
            }
        }
        or32_equals(actual_mixedcase[4], expected_lowercase[4])
    } else {
        false
    }
}

/// `fastfloat_strncasecmp`: compara duas strings ASCII sem distinguir a caixa.
pub fn fastfloat_strncasecmp<UC: CharType>(
    actual_mixedcase: &[UC],
    expected_lowercase: &[UC],
    length: usize,
) -> bool {
    let Some(mask) = lane_mask::<UC>() else {
        return false;
    };
    // `cpp20_and_in_constexpr()` é falso: o C++ copia blocos de `8 / sizeof(UC)` unidades.
    let mut sz: usize = 8 / UC::SIZE;
    let mut i: usize = 0;
    while i < length {
        sz = if sz < (length - i) { sz } else { length - i };
        for j in 0..sz {
            if !masked_equal(actual_mixedcase[i + j], expected_lowercase[i + j], mask) {
                return false;
            }
        }
        i += sz;
    }
    true
}

// ---------------------------------------------------------------------------------------------
// span, value128
// ---------------------------------------------------------------------------------------------

/// `span<T>`: um ponteiro e um comprimento. `ptr` é o slice que começa no primeiro elemento do
/// span (pode ser mais longo que `length`, como o ponteiro do C++ aponta para dentro do buffer).
#[derive(Clone, Copy, Debug)]
pub struct span<'a, T> {
    pub ptr: &'a [T],
    pub length: usize,
}

impl<'a, T> span<'a, T> {
    pub fn new(ptr: &'a [T], length: usize) -> Self {
        span { ptr, length }
    }

    pub fn len(&self) -> usize {
        self.length
    }
}

impl<'a, T> Default for span<'a, T> {
    fn default() -> Self {
        span { ptr: &[], length: 0 }
    }
}

impl<'a, T> Index<usize> for span<'a, T> {
    type Output = T;

    fn index(&self, index: usize) -> &T {
        &self.ptr[index]
    }
}

/// `value128`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct value128 {
    pub low: u64,
    pub high: u64,
}

impl value128 {
    pub const fn new(low: u64, high: u64) -> Self {
        value128 { low, high }
    }
}

// ---------------------------------------------------------------------------------------------
// Contagem de zeros e multiplicação de 128 bits
// ---------------------------------------------------------------------------------------------

/// `leading_zeroes_generic(input_num, last_bit = 0)`.
pub fn leading_zeroes_generic(mut input_num: u64, mut last_bit: i32) -> i32 {
    if input_num & 0xffffffff00000000u64 != 0 {
        input_num >>= 32;
        last_bit |= 32;
    }
    if input_num & 0xffff0000u64 != 0 {
        input_num >>= 16;
        last_bit |= 16;
    }
    if input_num & 0xff00u64 != 0 {
        input_num >>= 8;
        last_bit |= 8;
    }
    if input_num & 0xf0u64 != 0 {
        input_num >>= 4;
        last_bit |= 4;
    }
    if input_num & 0xcu64 != 0 {
        input_num >>= 2;
        last_bit |= 2;
    }
    if input_num & 0x2u64 != 0 {
        /* input_num >>= 1; */
        last_bit |= 1;
    }
    63 - last_bit
}

/// `leading_zeroes`: o resultado é indefinido com zero no C++ (`__builtin_clzll`); a conta do
/// Rust devolve 64.
pub fn leading_zeroes(input_num: u64) -> i32 {
    input_num.leading_zeros() as i32
}

/// `countr_zero_generic_32`.
pub fn countr_zero_generic_32(mut input_num: u32) -> i32 {
    if input_num == 0 {
        return 32;
    }
    let mut last_bit: i32 = 0;
    if input_num & 0x0000FFFF == 0 {
        input_num >>= 16;
        last_bit |= 16;
    }
    if input_num & 0x00FF == 0 {
        input_num >>= 8;
        last_bit |= 8;
    }
    if input_num & 0x0F == 0 {
        input_num >>= 4;
        last_bit |= 4;
    }
    if input_num & 0x3 == 0 {
        input_num >>= 2;
        last_bit |= 2;
    }
    if input_num & 0x1 == 0 {
        last_bit |= 1;
    }
    last_bit
}

/// `countr_zero_32`: `input_num == 0 ? 32 : __builtin_ctz(input_num)`.
pub fn countr_zero_32(input_num: u32) -> i32 {
    input_num.trailing_zeros() as i32
}

/// `emulu`: emulação lenta para 32 bits.
pub const fn emulu(x: u32, y: u32) -> u64 {
    (x as u64).wrapping_mul(y as u64)
}

/// `umul128_generic`: devolve o `lo` e escreve o `hi` em `hi`.
pub fn umul128_generic(ab: u64, cd: u64, hi: &mut u64) -> u64 {
    let ad: u64 = emulu((ab >> 32) as u32, cd as u32);
    let bd: u64 = emulu(ab as u32, cd as u32);
    let adbc: u64 = ad.wrapping_add(emulu(ab as u32, (cd >> 32) as u32));
    let adbc_carry: u64 = (adbc < ad) as u64;
    let lo: u64 = bd.wrapping_add(adbc << 32);
    *hi = emulu((ab >> 32) as u32, (cd >> 32) as u32)
        .wrapping_add(adbc >> 32)
        .wrapping_add(adbc_carry << 32)
        .wrapping_add((lo < bd) as u64);
    lo
}

/// `full_multiplication`: o produto de 64 por 64 bits. Em 64 bits o C++ usa `__uint128_t`, que é
/// o `u128` do Rust.
pub fn full_multiplication(a: u64, b: u64) -> value128 {
    let r: u128 = (a as u128) * (b as u128);
    value128 { low: r as u64, high: (r >> 64) as u64 }
}

// ---------------------------------------------------------------------------------------------
// adjusted_mantissa
// ---------------------------------------------------------------------------------------------

/// `adjusted_mantissa`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct adjusted_mantissa {
    pub mantissa: u64,
    /// Um valor negativo indica um resultado inválido.
    pub power2: i32,
}

/// Bias para obter o expoente real de um `adjusted_mantissa` inválido.
pub const INVALID_AM_BIAS: i32 = -0x8000;

/// Usada em `binary_format_lookup_tables<T>::max_mantissa`.
pub const CONSTANT_55555: u64 = 5 * 5 * 5 * 5 * 5;

// ---------------------------------------------------------------------------------------------
// binary_format
// ---------------------------------------------------------------------------------------------

/// `binary_format<T>` junto com `binary_format_lookup_tables<T>` e `is_supported_float_type<T>`:
/// as constantes do formato binário de `f32` e `f64`.
pub trait BinaryFormat:
    Copy + PartialOrd + Mul<Output = Self> + Div<Output = Self> + Neg<Output = Self> + 'static
{
    /// `equiv_uint_t<T>`: `u32` para `f32`, `u64` para `f64`.
    type EquivUint: Copy + PartialEq + Eq + Into<u64>;

    fn mantissa_explicit_bits() -> i32;
    fn minimum_exponent() -> i32;
    fn infinite_power() -> i32;
    fn sign_index() -> i32;
    /// Usado quando `fegetround() == FE_TONEAREST`.
    fn min_exponent_fast_path() -> i32;
    fn max_exponent_fast_path() -> i32;
    fn max_exponent_round_to_even() -> i32;
    fn min_exponent_round_to_even() -> i32;
    fn largest_power_of_ten() -> i32;
    fn smallest_power_of_ten() -> i32;
    fn max_digits() -> usize;
    fn exponent_mask() -> Self::EquivUint;
    fn mantissa_mask() -> Self::EquivUint;
    fn hidden_bit_mask() -> Self::EquivUint;

    /// `binary_format_lookup_tables<T>::powers_of_ten`.
    fn powers_of_ten() -> &'static [Self];
    /// `binary_format_lookup_tables<T>::max_mantissa`.
    fn max_mantissa() -> &'static [u64];

    /// A palavra de bits (`equiv_uint`) vira o número; `word` é truncada à largura do tipo.
    fn from_word(word: u64) -> Self;
    /// `std::numeric_limits<T>::infinity()`.
    fn infinity() -> Self;
    /// `std::numeric_limits<T>::quiet_NaN()`.
    fn quiet_nan() -> Self;
    /// `static_cast<T>(uint64_t)`.
    fn from_u64(value: u64) -> Self;
    /// `static_cast<T>(int64_t)`.
    fn from_i64(value: i64) -> Self;

    /// `max_mantissa_fast_path()`, usado quando `fegetround() == FE_TONEAREST`.
    fn max_mantissa_fast_path() -> u64 {
        2u64 << Self::mantissa_explicit_bits()
    }

    /// `max_mantissa_fast_path(int64_t power)`: o chamador garante `0 <= power <= max_exponent_fast_path`.
    fn max_mantissa_fast_path_at(power: i64) -> u64 {
        Self::max_mantissa()[power as usize]
    }

    /// `exact_power_of_ten(int64_t power)`.
    fn exact_power_of_ten(power: i64) -> Self {
        Self::powers_of_ten()[power as usize]
    }
}

static F64_POWERS_OF_TEN: [f64; 23] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
    1e17, 1e18, 1e19, 1e20, 1e21, 1e22,
];

// Maior inteiro v tal que (5**index * v) <= 1<<53.
// 0x20000000000000 == 1 << 53
static F64_MAX_MANTISSA: [u64; 24] = [
    0x20000000000000,
    0x20000000000000 / 5,
    0x20000000000000 / (5 * 5),
    0x20000000000000 / (5 * 5 * 5),
    0x20000000000000 / (5 * 5 * 5 * 5),
    0x20000000000000 / (CONSTANT_55555),
    0x20000000000000 / (CONSTANT_55555 * 5),
    0x20000000000000 / (CONSTANT_55555 * 5 * 5),
    0x20000000000000 / (CONSTANT_55555 * 5 * 5 * 5),
    0x20000000000000 / (CONSTANT_55555 * 5 * 5 * 5 * 5),
    0x20000000000000 / (CONSTANT_55555 * CONSTANT_55555),
    0x20000000000000 / (CONSTANT_55555 * CONSTANT_55555 * 5),
    0x20000000000000 / (CONSTANT_55555 * CONSTANT_55555 * 5 * 5),
    0x20000000000000 / (CONSTANT_55555 * CONSTANT_55555 * 5 * 5 * 5),
    0x20000000000000 / (CONSTANT_55555 * CONSTANT_55555 * CONSTANT_55555),
    0x20000000000000 / (CONSTANT_55555 * CONSTANT_55555 * CONSTANT_55555 * 5),
    0x20000000000000 / (CONSTANT_55555 * CONSTANT_55555 * CONSTANT_55555 * 5 * 5),
    0x20000000000000 / (CONSTANT_55555 * CONSTANT_55555 * CONSTANT_55555 * 5 * 5 * 5),
    0x20000000000000 / (CONSTANT_55555 * CONSTANT_55555 * CONSTANT_55555 * 5 * 5 * 5 * 5),
    0x20000000000000 / (CONSTANT_55555 * CONSTANT_55555 * CONSTANT_55555 * CONSTANT_55555),
    0x20000000000000
        / (CONSTANT_55555 * CONSTANT_55555 * CONSTANT_55555 * CONSTANT_55555 * 5),
    0x20000000000000
        / (CONSTANT_55555 * CONSTANT_55555 * CONSTANT_55555 * CONSTANT_55555 * 5 * 5),
    0x20000000000000
        / (CONSTANT_55555 * CONSTANT_55555 * CONSTANT_55555 * CONSTANT_55555 * 5 * 5 * 5),
    0x20000000000000
        / (CONSTANT_55555 * CONSTANT_55555 * CONSTANT_55555 * CONSTANT_55555 * 5 * 5 * 5 * 5),
];

static F32_POWERS_OF_TEN: [f32; 11] =
    [1e0f32, 1e1f32, 1e2f32, 1e3f32, 1e4f32, 1e5f32, 1e6f32, 1e7f32, 1e8f32, 1e9f32, 1e10f32];

// Maior inteiro v tal que (5**index * v) <= 1<<24.
// 0x1000000 == 1<<24
static F32_MAX_MANTISSA: [u64; 12] = [
    0x1000000,
    0x1000000 / 5,
    0x1000000 / (5 * 5),
    0x1000000 / (5 * 5 * 5),
    0x1000000 / (5 * 5 * 5 * 5),
    0x1000000 / (CONSTANT_55555),
    0x1000000 / (CONSTANT_55555 * 5),
    0x1000000 / (CONSTANT_55555 * 5 * 5),
    0x1000000 / (CONSTANT_55555 * 5 * 5 * 5),
    0x1000000 / (CONSTANT_55555 * 5 * 5 * 5 * 5),
    0x1000000 / (CONSTANT_55555 * CONSTANT_55555),
    0x1000000 / (CONSTANT_55555 * CONSTANT_55555 * 5),
];

impl BinaryFormat for f64 {
    type EquivUint = u64;

    fn mantissa_explicit_bits() -> i32 {
        52
    }

    fn minimum_exponent() -> i32 {
        -1023
    }

    fn infinite_power() -> i32 {
        0x7FF
    }

    fn sign_index() -> i32 {
        63
    }

    // FLT_EVAL_METHOD vale 0 em Linux x86_64.
    fn min_exponent_fast_path() -> i32 {
        -22
    }

    fn max_exponent_fast_path() -> i32 {
        22
    }

    fn max_exponent_round_to_even() -> i32 {
        23
    }

    fn min_exponent_round_to_even() -> i32 {
        -4
    }

    fn largest_power_of_ten() -> i32 {
        308
    }

    fn smallest_power_of_ten() -> i32 {
        -342
    }

    fn max_digits() -> usize {
        769
    }

    fn exponent_mask() -> u64 {
        0x7FF0000000000000
    }

    fn mantissa_mask() -> u64 {
        0x000FFFFFFFFFFFFF
    }

    fn hidden_bit_mask() -> u64 {
        0x0010000000000000
    }

    fn powers_of_ten() -> &'static [f64] {
        &F64_POWERS_OF_TEN
    }

    fn max_mantissa() -> &'static [u64] {
        &F64_MAX_MANTISSA
    }

    fn from_word(word: u64) -> f64 {
        f64::from_bits(word)
    }

    fn infinity() -> f64 {
        f64::INFINITY
    }

    fn quiet_nan() -> f64 {
        f64::NAN
    }

    fn from_u64(value: u64) -> f64 {
        value as f64
    }

    fn from_i64(value: i64) -> f64 {
        value as f64
    }
}

impl BinaryFormat for f32 {
    type EquivUint = u32;

    fn mantissa_explicit_bits() -> i32 {
        23
    }

    fn minimum_exponent() -> i32 {
        -127
    }

    fn infinite_power() -> i32 {
        0xFF
    }

    fn sign_index() -> i32 {
        31
    }

    // FLT_EVAL_METHOD vale 0 em Linux x86_64.
    fn min_exponent_fast_path() -> i32 {
        -10
    }

    fn max_exponent_fast_path() -> i32 {
        10
    }

    fn max_exponent_round_to_even() -> i32 {
        10
    }

    fn min_exponent_round_to_even() -> i32 {
        -17
    }

    fn largest_power_of_ten() -> i32 {
        38
    }

    fn smallest_power_of_ten() -> i32 {
        -64
    }

    fn max_digits() -> usize {
        114
    }

    fn exponent_mask() -> u32 {
        0x7F800000
    }

    fn mantissa_mask() -> u32 {
        0x007FFFFF
    }

    fn hidden_bit_mask() -> u32 {
        0x00800000
    }

    fn powers_of_ten() -> &'static [f32] {
        &F32_POWERS_OF_TEN
    }

    fn max_mantissa() -> &'static [u64] {
        &F32_MAX_MANTISSA
    }

    fn from_word(word: u64) -> f32 {
        f32::from_bits(word as u32)
    }

    fn infinity() -> f32 {
        f32::INFINITY
    }

    fn quiet_nan() -> f32 {
        f32::NAN
    }

    fn from_u64(value: u64) -> f32 {
        value as f32
    }

    fn from_i64(value: i64) -> f32 {
        value as f32
    }
}

/// `to_float`: monta o número a partir do sinal e da mantissa ajustada. O C++ faz as contas em
/// `equiv_uint`; aqui elas rodam em `u64` e `from_word` trunca à largura do tipo, o que dá os
/// mesmos bits (o `|` e o `<<` só carregam bits para cima, e o `power2` negativo estende o sinal
/// como a conversão `int32_t` para `equiv_uint`).
pub fn to_float<T: BinaryFormat>(negative: bool, am: adjusted_mantissa, value: &mut T) {
    let mut word: u64 = am.mantissa;
    word |= (am.power2 as i64 as u64) << (T::mantissa_explicit_bits() as u32);
    word |= (negative as u64) << (T::sign_index() as u32);
    *value = T::from_word(word);
}

// ---------------------------------------------------------------------------------------------
// space_lut, is_space
// ---------------------------------------------------------------------------------------------

#[rustfmt::skip]
const SPACE_LUT_RAW: [u8; 256] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// `space_lut<>::value`.
pub static SPACE_LUT_VALUE: [bool; 256] = {
    let mut out = [false; 256];
    let mut i = 0;
    while i < 256 {
        out[i] = SPACE_LUT_RAW[i] != 0;
        i += 1;
    }
    out
};

/// `is_space`.
pub fn is_space<UC: CharType>(c: UC) -> bool {
    let code: u32 = c.into();
    code < 256 && SPACE_LUT_VALUE[code as u8 as usize]
}

/// `int_cmp_zeros<UC>()`: oito caracteres `'0'` empacotados em 64 bits.
pub fn int_cmp_zeros<UC: CharType>() -> u64 {
    if UC::SIZE == 1 {
        0x3030303030303030
    } else if UC::SIZE == 2 {
        (b'0' as u64) << 48 | (b'0' as u64) << 32 | (b'0' as u64) << 16 | (b'0' as u64)
    } else {
        (b'0' as u64) << 32 | (b'0' as u64)
    }
}

/// `int_cmp_len<UC>()`.
pub fn int_cmp_len<UC: CharType>() -> i32 {
    (std::mem::size_of::<u64>() / UC::SIZE) as i32
}

/// `str_const_nan<UC>()`: "nan" como caracteres de `UC`.
pub fn str_const_nan<UC: CharType>() -> [UC; 3] {
    [UC::from_u16(b'n' as u16), UC::from_u16(b'a' as u16), UC::from_u16(b'n' as u16)]
}

/// `str_const_inf<UC>()`: "infinity" como caracteres de `UC`.
pub fn str_const_inf<UC: CharType>() -> [UC; 8] {
    [
        UC::from_u16(b'i' as u16),
        UC::from_u16(b'n' as u16),
        UC::from_u16(b'f' as u16),
        UC::from_u16(b'i' as u16),
        UC::from_u16(b'n' as u16),
        UC::from_u16(b'i' as u16),
        UC::from_u16(b't' as u16),
        UC::from_u16(b'y' as u16),
    ]
}

// ---------------------------------------------------------------------------------------------
// int_luts
// ---------------------------------------------------------------------------------------------

/// `int_luts<>::chdigit`.
#[rustfmt::skip]
pub static INT_LUTS_CHDIGIT: [u8; 256] = [
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 0,   1,   2,   3,   4,   5,   6,   7,   8,   9,   255, 255,
    255, 255, 255, 255, 255, 10,  11,  12,  13,  14,  15,  16,  17,  18,  19,
    20,  21,  22,  23,  24,  25,  26,  27,  28,  29,  30,  31,  32,  33,  34,
    35,  255, 255, 255, 255, 255, 255, 10,  11,  12,  13,  14,  15,  16,  17,
    18,  19,  20,  21,  22,  23,  24,  25,  26,  27,  28,  29,  30,  31,  32,
    33,  34,  35,  255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255,
];

/// `int_luts<>::maxdigits_u64`.
#[rustfmt::skip]
pub static INT_LUTS_MAXDIGITS_U64: [usize; 35] = [
    64, 41, 32, 28, 25, 23, 22, 21, 20, 19, 18, 18, 17, 17, 16, 16, 16, 16,
    15, 15, 15, 15, 14, 14, 14, 14, 14, 14, 14, 13, 13, 13, 13, 13, 13,
];

/// `int_luts<>::min_safe_u64`.
#[rustfmt::skip]
pub static INT_LUTS_MIN_SAFE_U64: [u64; 35] = [
    9223372036854775808,  12157665459056928801, 4611686018427387904,
    7450580596923828125,  4738381338321616896,  3909821048582988049,
    9223372036854775808,  12157665459056928801, 10000000000000000000,
    5559917313492231481,  2218611106740436992,  8650415919381337933,
    2177953337809371136,  6568408355712890625,  1152921504606846976,
    2862423051509815793,  6746640616477458432,  15181127029874798299,
    1638400000000000000,  3243919932521508681,  6221821273427820544,
    11592836324538749809, 876488338465357824,   1490116119384765625,
    2481152873203736576,  4052555153018976267,  6502111422497947648,
    10260628712958602189, 15943230000000000000, 787662783788549761,
    1152921504606846976,  1667889514952984961,  2386420683693101056,
    3379220508056640625,  4738381338321616896,
];

/// `ch_to_digit`: o valor do dígito em base até 36, ou 255 se o caractere não for um dígito.
/// Caracteres acima de 255 caem no índice zero (que vale 255), como a máscara do C++.
pub fn ch_to_digit<UC: CharType>(c: UC) -> u8 {
    let code: u32 = c.into();
    let mask: u32 = if code & !0xFFu32 == 0 { u32::MAX } else { 0 };
    INT_LUTS_CHDIGIT[(code & mask) as u8 as usize]
}

/// `max_digits_u64`.
pub fn max_digits_u64(base: i32) -> usize {
    INT_LUTS_MAXDIGITS_U64[(base - 2) as usize]
}

/// `min_safe_u64`: se um u64 tem exatamente `max_digits_u64()` dígitos, este é o valor abaixo do
/// qual ele estourou com certeza.
pub fn min_safe_u64(base: i32) -> u64 {
    INT_LUTS_MIN_SAFE_U64[(base - 2) as usize]
}
