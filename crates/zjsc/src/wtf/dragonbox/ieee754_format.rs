//! Porte de `WTF/wtf/dragonbox/ieee754_format.h`.
//!
//! `default_float_traits<T>` do C++ é o trait `FloatTraits`, implementado por `f32` e `f64`. O
//! `carrier_uint` do C++ (`uint32_t` ou `uint64_t`) vira sempre `u64` aqui, com os bits do
//! binary32 nos 32 bits baixos e zeros acima; toda operação que no C++ truncaria na largura de
//! 32 bits é truncada de forma explícita (`remove_sign_bit_and_shift`, `compute_mul` do binary32).

use crate::wtf::dragonbox::detail::log::{
    floor_log2, floor_log5_pow2, floor_log5_pow2_minus_log5_3,
};
use crate::wtf::dragonbox::detail::util::{compute_power, count_factors};

/// `ieee754_binary32` e `ieee754_binary64`: a especificação de codificação do formato.
pub trait FloatFormat {
    const SIGNIFICAND_BITS: i32;
    const EXPONENT_BITS: i32;
    const MIN_EXPONENT: i32;
    const MAX_EXPONENT: i32;
    const EXPONENT_BIAS: i32;
    const DECIMAL_DIGITS: i32;

    /// `impl::kappa`: 1 para binary32 e 2 para binary64.
    const KAPPA: i32;

    /// `impl::case_shorter_interval_left_endpoint_lower_threshold`.
    const CASE_SHORTER_INTERVAL_LEFT_ENDPOINT_LOWER_THRESHOLD: i32 = 2;
    /// `impl::case_shorter_interval_left_endpoint_upper_threshold`.
    const CASE_SHORTER_INTERVAL_LEFT_ENDPOINT_UPPER_THRESHOLD: i32 = 2 + floor_log2(
        compute_power(
            count_factors(5, (1u64 << (Self::SIGNIFICAND_BITS + 2)) - 1) + 1,
            10,
        ) / 3,
    );

    /// `impl::case_shorter_interval_right_endpoint_lower_threshold`.
    const CASE_SHORTER_INTERVAL_RIGHT_ENDPOINT_LOWER_THRESHOLD: i32 = 0;
    /// `impl::case_shorter_interval_right_endpoint_upper_threshold`.
    const CASE_SHORTER_INTERVAL_RIGHT_ENDPOINT_UPPER_THRESHOLD: i32 = 2 + floor_log2(
        compute_power(
            count_factors(5, (1u64 << (Self::SIGNIFICAND_BITS + 1)) + 1) + 1,
            10,
        ) / 3,
    );

    /// `impl::shorter_interval_tie_lower_threshold`.
    const SHORTER_INTERVAL_TIE_LOWER_THRESHOLD: i32 =
        -floor_log5_pow2_minus_log5_3(Self::SIGNIFICAND_BITS + 4) - 2 - Self::SIGNIFICAND_BITS;
    /// `impl::shorter_interval_tie_upper_threshold`.
    const SHORTER_INTERVAL_TIE_UPPER_THRESHOLD: i32 =
        -floor_log5_pow2(Self::SIGNIFICAND_BITS + 2) - 2 - Self::SIGNIFICAND_BITS;
}

pub struct Ieee754Binary32;

impl FloatFormat for Ieee754Binary32 {
    const SIGNIFICAND_BITS: i32 = 23;
    const EXPONENT_BITS: i32 = 8;
    const MIN_EXPONENT: i32 = -126;
    const MAX_EXPONENT: i32 = 127;
    const EXPONENT_BIAS: i32 = -127;
    const DECIMAL_DIGITS: i32 = 9;
    const KAPPA: i32 = 1;
}

pub struct Ieee754Binary64;

impl FloatFormat for Ieee754Binary64 {
    const SIGNIFICAND_BITS: i32 = 52;
    const EXPONENT_BITS: i32 = 11;
    const MIN_EXPONENT: i32 = -1022;
    const MAX_EXPONENT: i32 = 1023;
    const EXPONENT_BIAS: i32 = -1023;
    const DECIMAL_DIGITS: i32 = 17;
    const KAPPA: i32 = 2;
}

/// `default_float_traits<T>`: como interpretar um padrão de bits como número de ponto flutuante.
/// Só `f32` (binary32) e `f64` (binary64) a implementam.
pub trait FloatTraits: Copy {
    /// `format`. O limite `ComputeMulImpl` espelha o `compute_mul_impl<format>` que o `impl` do
    /// C++ especializa por formato, o que deixa os usos genéricos sem repetir a cláusula.
    type Format: FloatFormat + crate::wtf::dragonbox::dragonbox::ComputeMulImpl;

    /// `carrier_bits`.
    const CARRIER_BITS: i32;

    /// `float_to_carrier`: os bits do número, nos bits baixos de um `u64`.
    fn float_to_carrier(self) -> u64;

    /// `extract_exponent_bits`: os bits do expoente alinhados ao bit menos significativo, sem
    /// ajuste de viés.
    fn extract_exponent_bits(u: u64) -> u32 {
        ((u >> Self::Format::SIGNIFICAND_BITS) as u32) & ((1u32 << Self::Format::EXPONENT_BITS) - 1)
    }

    /// `extract_significand_bits`: os bits do significando, sem o bit implícito.
    fn extract_significand_bits(u: u64) -> u64 {
        u & ((1u64 << Self::Format::SIGNIFICAND_BITS) - 1)
    }

    /// `remove_exponent_bits`: tira os bits do expoente, deixando o sinal e o significando.
    fn remove_exponent_bits(u: u64, exponent_bits: u32) -> u64 {
        u ^ ((exponent_bits as u64) << Self::Format::SIGNIFICAND_BITS)
    }

    /// `remove_sign_bit_and_shift`: desloca um bit à esquerda, na largura do portador, o que tira
    /// o bit de sinal.
    fn remove_sign_bit_and_shift(u: u64) -> u64 {
        if Self::CARRIER_BITS == 32 {
            ((u as u32) << 1) as u64
        } else {
            u << 1
        }
    }

    /// `binary_exponent`.
    fn binary_exponent(exponent_bits: u32) -> i32 {
        if exponent_bits == 0 {
            Self::Format::MIN_EXPONENT
        } else {
            exponent_bits as i32 + Self::Format::EXPONENT_BIAS
        }
    }

    /// `binary_significand`.
    fn binary_significand(significand_bits: u64, exponent_bits: u32) -> u64 {
        if exponent_bits == 0 {
            significand_bits
        } else {
            significand_bits | (1u64 << Self::Format::SIGNIFICAND_BITS)
        }
    }

    fn is_nonzero(u: u64) -> bool {
        Self::remove_sign_bit_and_shift(u) != 0
    }

    fn is_positive(u: u64) -> bool {
        u < (1u64 << (Self::Format::SIGNIFICAND_BITS + Self::Format::EXPONENT_BITS))
    }

    fn is_negative(u: u64) -> bool {
        !Self::is_positive(u)
    }

    fn is_finite(exponent_bits: u32) -> bool {
        exponent_bits != ((1u32 << Self::Format::EXPONENT_BITS) - 1)
    }

    fn has_all_zero_significand_bits(u: u64) -> bool {
        Self::remove_sign_bit_and_shift(u) == 0
    }

    fn has_even_significand_bits(u: u64) -> bool {
        u % 2 == 0
    }
}

impl FloatTraits for f32 {
    type Format = Ieee754Binary32;
    const CARRIER_BITS: i32 = 32;

    fn float_to_carrier(self) -> u64 {
        self.to_bits() as u64
    }
}

impl FloatTraits for f64 {
    type Format = Ieee754Binary64;
    const CARRIER_BITS: i32 = 64;

    fn float_to_carrier(self) -> u64 {
        self.to_bits()
    }
}

/// `float_bits<T>`: o padrão de bits de um número e as consultas sobre ele.
#[derive(Clone, Copy)]
pub struct FloatBits<T: FloatTraits> {
    pub u: u64,
    _marker: std::marker::PhantomData<T>,
}

impl<T: FloatTraits> FloatBits<T> {
    /// `float_bits(T float_value)`.
    pub fn new(float_value: T) -> Self {
        FloatBits { u: float_value.float_to_carrier(), _marker: std::marker::PhantomData }
    }

    pub fn extract_exponent_bits(&self) -> u32 {
        T::extract_exponent_bits(self.u)
    }

    pub fn extract_significand_bits(&self) -> u64 {
        T::extract_significand_bits(self.u)
    }

    pub fn remove_exponent_bits(&self, exponent_bits: u32) -> SignedSignificandBits<T> {
        SignedSignificandBits::new(T::remove_exponent_bits(self.u, exponent_bits))
    }

    pub fn binary_exponent_of(exponent_bits: u32) -> i32 {
        T::binary_exponent(exponent_bits)
    }

    pub fn binary_exponent(&self) -> i32 {
        Self::binary_exponent_of(self.extract_exponent_bits())
    }

    pub fn binary_significand_of(significand_bits: u64, exponent_bits: u32) -> u64 {
        T::binary_significand(significand_bits, exponent_bits)
    }

    pub fn binary_significand(&self) -> u64 {
        Self::binary_significand_of(self.extract_significand_bits(), self.extract_exponent_bits())
    }

    pub fn is_nonzero(&self) -> bool {
        T::is_nonzero(self.u)
    }

    pub fn is_positive(&self) -> bool {
        T::is_positive(self.u)
    }

    pub fn is_negative(&self) -> bool {
        T::is_negative(self.u)
    }

    pub fn is_finite_with(&self, exponent_bits: u32) -> bool {
        T::is_finite(exponent_bits)
    }

    pub fn is_finite(&self) -> bool {
        T::is_finite(self.extract_exponent_bits())
    }

    pub fn has_even_significand_bits(&self) -> bool {
        T::has_even_significand_bits(self.u)
    }
}

/// `signed_significand_bits<T>`: sinal e significando, sem os bits do expoente.
#[derive(Clone, Copy)]
pub struct SignedSignificandBits<T: FloatTraits> {
    pub u: u64,
    _marker: std::marker::PhantomData<T>,
}

impl<T: FloatTraits> SignedSignificandBits<T> {
    pub fn new(bit_pattern: u64) -> Self {
        SignedSignificandBits { u: bit_pattern, _marker: std::marker::PhantomData }
    }

    pub fn remove_sign_bit_and_shift(&self) -> u64 {
        T::remove_sign_bit_and_shift(self.u)
    }

    pub fn is_positive(&self) -> bool {
        T::is_positive(self.u)
    }

    pub fn is_negative(&self) -> bool {
        T::is_negative(self.u)
    }

    pub fn has_all_zero_significand_bits(&self) -> bool {
        T::has_all_zero_significand_bits(self.u)
    }

    pub fn has_even_significand_bits(&self) -> bool {
        T::has_even_significand_bits(self.u)
    }
}
