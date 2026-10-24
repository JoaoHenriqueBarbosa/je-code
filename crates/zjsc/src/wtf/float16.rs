//! Porte de `WTF/wtf/Float16.h`: conversões entre o `Float16` (IEEE 754 meia precisão, guardado como os
//! 16 bits) e `float`/`double`.
//!
//! DIVERGÊNCIA: o C++ tem dois caminhos, o `_Float16` do compilador (`HAVE(FLOAT16)`, que arredonda para o
//! par mais próximo direto de `double`) e o de bits do FP16 de Maratyszcza/V8. Rust estável não tem `f16`,
//! então as duas conversões são feitas à mão sobre os bits, com o mesmo resultado do `_Float16`: a
//! conversão de `double` arredonda uma única vez, para o par mais próximo, sem passar por `float`.

/// Os 16 bits de um `Float16`.
pub type Float16Bits = u16;

/// `pow(2, exponent)` exato para `exponent` em `[-1022, 1023]`.
fn power_of_two(exponent: i32) -> f64 {
    debug_assert!((-1022..=1023).contains(&exponent));
    f64::from_bits(((exponent + 1023) as u64) << 52)
}

/// `convertFloat16ToFloat64(h)`.
pub fn convert_float16_to_float64(h: Float16Bits) -> f64 {
    let negative = h & 0x8000 != 0;
    let exponent = i32::from((h >> 10) & 0x1f);
    let mantissa = u32::from(h & 0x3ff);
    let magnitude = match exponent {
        // Zero e denormais: mantissa * 2^-24.
        0 => f64::from(mantissa) * power_of_two(-24),
        0x1f if mantissa == 0 => f64::INFINITY,
        0x1f => f64::NAN,
        _ => f64::from(1024 + mantissa) * power_of_two(exponent - 25),
    };
    if negative { -magnitude } else { magnitude }
}

/// `convertFloat16ToFloat32(h)`: todo `Float16` cabe em `float` sem perda.
pub fn convert_float16_to_float32(h: Float16Bits) -> f32 {
    convert_float16_to_float64(h) as f32
}

/// `convertFloat64ToFloat16(value)`: arredonda para o par mais próximo. NaN vira o NaN quieto `0x7e00`
/// (com o sinal), o estouro vira infinito.
pub fn convert_float64_to_float16(value: f64) -> Float16Bits {
    let bits = value.to_bits();
    let sign = ((bits >> 48) & 0x8000) as u16;
    let exponent_field = ((bits >> 52) & 0x7ff) as i32;
    let mantissa = bits & ((1u64 << 52) - 1);

    if exponent_field == 0x7ff {
        return sign | if mantissa != 0 { 0x7e00 } else { 0x7c00 };
    }
    // Denormais de `double` (e tudo abaixo de 2^-25) arredondam para zero.
    let exponent = exponent_field - 1023;
    if exponent_field == 0 || exponent < -25 {
        return sign;
    }

    let full = (1u64 << 52) | mantissa;
    if exponent >= -14 {
        // Normal em meia precisão: 10 bits de mantissa, descarta 42.
        let shift = 42;
        let truncated = full >> shift;
        let remainder = full & ((1u64 << shift) - 1);
        let halfway = 1u64 << (shift - 1);
        let round_up = remainder > halfway || (remainder == halfway && truncated & 1 == 1);
        let result = (((exponent + 15) as u32) << 10) + (truncated - 1024) as u32 + u32::from(round_up);
        // Estouro, inclusive o vai-um da mantissa para o expoente, vira infinito.
        return sign | if result >= 0x7c00 { 0x7c00 } else { result as u16 };
    }

    // Denormal em meia precisão: a unidade é 2^-24.
    let shift = (28 - exponent) as u32;
    let truncated = full >> shift;
    let remainder = full & ((1u64 << shift) - 1);
    let halfway = 1u64 << (shift - 1);
    let round_up = remainder > halfway || (remainder == halfway && truncated & 1 == 1);
    sign | (truncated + u64::from(round_up)) as u16
}

/// `convertFloat32ToFloat16(f)`.
pub fn convert_float32_to_float16(f: f32) -> Float16Bits {
    convert_float64_to_float16(f64::from(f))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_finite_half() {
        for h in 0..=u16::MAX {
            let value = convert_float16_to_float64(h);
            if value.is_nan() {
                assert!(convert_float16_to_float64(convert_float64_to_float16(value)).is_nan());
            } else {
                assert_eq!(convert_float64_to_float16(value), h, "{h:#06x}");
            }
        }
    }

    #[test]
    fn rounds_to_nearest_even_and_overflows() {
        assert_eq!(convert_float64_to_float16(1.0), 0x3c00);
        assert_eq!(convert_float64_to_float16(65504.0), 0x7bff);
        // 65520 é o ponto médio entre 65504 e o infinito: empata para o par (infinito).
        assert_eq!(convert_float64_to_float16(65520.0), 0x7c00);
        assert_eq!(convert_float64_to_float16(65519.99), 0x7bff);
        // 2^-25 empata entre 0 e o menor denormal: o par é zero.
        assert_eq!(convert_float64_to_float16(2f64.powi(-25)), 0);
        assert_eq!(convert_float64_to_float16(2f64.powi(-25) * 1.0000001), 1);
        assert_eq!(convert_float64_to_float16(-0.0), 0x8000);
        assert_eq!(convert_float64_to_float16(f64::NEG_INFINITY), 0xfc00);
    }
}
