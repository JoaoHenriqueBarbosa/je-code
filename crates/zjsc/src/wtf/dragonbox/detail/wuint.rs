//! Porte de `WTF/wtf/dragonbox/detail/wuint.h`: aritmética de inteiros largos sem sinal.

pub const fn umul64(x: u32, y: u32) -> u64 {
    (x as u64) * (y as u64)
}

/// Resultado de 128 bits da multiplicação de dois inteiros de 64 bits.
pub const fn umul128(x: u64, y: u64) -> u128 {
    (x as u128) * (y as u128)
}

pub const fn umul128_upper64(x: u64, y: u64) -> u64 {
    (((x as u128) * (y as u128)) >> 64) as u64
}

/// Os 128 bits superiores da multiplicação de um inteiro de 64 bits por um de 128 bits.
pub const fn umul192_upper128(x: u64, y: u128) -> u128 {
    let y_high = (y >> 64) as u64;
    let y_low = y as u64;
    let r = umul128(x, y_high);
    r.wrapping_add(umul128_upper64(x, y_low) as u128)
}

/// Os 64 bits superiores da multiplicação de um inteiro de 32 bits por um de 64 bits.
pub const fn umul96_upper64(x: u32, y: u64) -> u64 {
    umul128_upper64((x as u64) << 32, y)
}

/// Os 128 bits inferiores da multiplicação de um inteiro de 64 bits por um de 128 bits.
pub const fn umul192_lower128(x: u64, y: u128) -> u128 {
    (x as u128).wrapping_mul(y)
}

/// Os 64 bits inferiores da multiplicação de um inteiro de 32 bits por um de 64 bits.
pub const fn umul96_lower64(x: u32, y: u64) -> u64 {
    (x as u64).wrapping_mul(y)
}
