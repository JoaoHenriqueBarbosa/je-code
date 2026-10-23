//! Testes próprios de `src/vs/base/common/uint.ts` (o VS Code não tem `uint.test.ts`); os valores
//! esperados saem do Node.

use je_base::uint::{constants, to_uint8, to_uint32};

#[test]
fn constants_match_the_original() {
    assert_eq!(constants::MAX_SAFE_SMALL_INTEGER, 1_073_741_824);
    assert_eq!(constants::MIN_SAFE_SMALL_INTEGER, -1_073_741_824);
    assert_eq!(constants::MAX_UINT_8, 255);
    assert_eq!(constants::MAX_UINT_16, 65535);
    assert_eq!(constants::MAX_UINT_32, 4_294_967_295);
    assert_eq!(constants::UNICODE_SUPPLEMENTARY_PLANE_BEGIN, 0x10000);
}

#[test]
fn to_uint8_saturates() {
    assert_eq!(to_uint8(-5.0), 0);
    assert_eq!(to_uint8(0.0), 0);
    assert_eq!(to_uint8(1.9), 1);
    assert_eq!(to_uint8(255.0), 255);
    assert_eq!(to_uint8(256.0), 255);
    assert_eq!(to_uint8(f64::NAN), 0);
    assert_eq!(to_uint8(f64::INFINITY), 255);
}

#[test]
fn to_uint32_saturates_and_wraps_like_bitwise_or() {
    assert_eq!(to_uint32(-1.0), 0);
    assert_eq!(to_uint32(5.9), 5);
    assert_eq!(to_uint32(2_147_483_647.0), 2_147_483_647);
    // `v | 0` reinterpreta como int32 acima de 2^31 - 1.
    assert_eq!(to_uint32(3_000_000_000.0), -1_294_967_296);
    assert_eq!(to_uint32(4_294_967_295.0), -1);
    assert_eq!(to_uint32(4_294_967_296.0), 4_294_967_295);
    assert_eq!(to_uint32(1e12), 4_294_967_295);
    assert_eq!(to_uint32(f64::NAN), 0);
}
