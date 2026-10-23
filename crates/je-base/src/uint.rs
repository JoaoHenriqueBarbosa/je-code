//! Porte de `src/vs/base/common/uint.ts`: limites de inteiros e conversões saturadas.
//!
//! O original recebe `number` (`f64`) e devolve `v | 0` dentro do intervalo, ou o limite (a constante,
//! sem `| 0`) acima dele. `v | 0` trunca em direção a zero e reinterpreta como `int32`: `toUint32`
//! entre `2^31` e `2^32 - 1` dá um valor NEGATIVO, enquanto acima de `2^32 - 1` dá `4294967295`. O
//! retorno é `i64` para guardar os dois casos, que é o comportamento do original.

/// `Constants`.
pub mod constants {
    /// MAX SMI (SMall Integer) do v8: um bit se perde no flag de boxing e outro no sinal.
    pub const MAX_SAFE_SMALL_INTEGER: i32 = 1 << 30;
    /// MIN SMI do v8.
    pub const MIN_SAFE_SMALL_INTEGER: i32 = -(1 << 30);
    /// Maior inteiro sem sinal em 8 bits.
    pub const MAX_UINT_8: u32 = 255;
    /// Maior inteiro sem sinal em 16 bits.
    pub const MAX_UINT_16: u32 = 65535;
    /// Maior inteiro sem sinal em 32 bits.
    pub const MAX_UINT_32: u32 = 4_294_967_295;
    pub const UNICODE_SUPPLEMENTARY_PLANE_BEGIN: u32 = 0x01_0000;
}

/// `v | 0` do JavaScript: `ToInt32` (trunca e dá a volta módulo 2^32; `NaN` e infinitos viram 0).
fn to_int32(v: f64) -> i64 {
    if !v.is_finite() {
        return 0;
    }
    i64::from((v.trunc().rem_euclid(4_294_967_296.0) as u32) as i32)
}

/// `toUint8`.
pub fn to_uint8(v: f64) -> i64 {
    if v < 0.0 {
        return 0;
    }
    if v > f64::from(constants::MAX_UINT_8) {
        return i64::from(constants::MAX_UINT_8);
    }
    to_int32(v)
}

/// `toUint32`. Ver o topo do módulo para o que acontece acima de `2^31 - 1`.
pub fn to_uint32(v: f64) -> i64 {
    if v < 0.0 {
        return 0;
    }
    if v > f64::from(constants::MAX_UINT_32) {
        return i64::from(constants::MAX_UINT_32);
    }
    to_int32(v)
}
