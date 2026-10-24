// Tradução de WTF/wtf/dtoa/cached-powers.h e cached-powers.cc (double-conversion).

use crate::wtf::dtoa::diy_fp::DiyFp;

struct CachedPower {
    significand: u64,
    binary_exponent: i16,
    decimal_exponent: i16,
}

const fn cp(significand: u64, binary_exponent: i16, decimal_exponent: i16) -> CachedPower {
    CachedPower {
        significand,
        binary_exponent,
        decimal_exponent,
    }
}

static K_CACHED_POWERS: [CachedPower; 87] = [
    cp(0xfa8fd5a0_081c0288, -1220, -348),
    cp(0xbaaee17f_a23ebf76, -1193, -340),
    cp(0x8b16fb20_3055ac76, -1166, -332),
    cp(0xcf42894a_5dce35ea, -1140, -324),
    cp(0x9a6bb0aa_55653b2d, -1113, -316),
    cp(0xe61acf03_3d1a45df, -1087, -308),
    cp(0xab70fe17_c79ac6ca, -1060, -300),
    cp(0xff77b1fc_bebcdc4f, -1034, -292),
    cp(0xbe5691ef_416bd60c, -1007, -284),
    cp(0x8dd01fad_907ffc3c, -980, -276),
    cp(0xd3515c28_31559a83, -954, -268),
    cp(0x9d71ac8f_ada6c9b5, -927, -260),
    cp(0xea9c2277_23ee8bcb, -901, -252),
    cp(0xaecc4991_4078536d, -874, -244),
    cp(0x823c1279_5db6ce57, -847, -236),
    cp(0xc2109436_4dfb5637, -821, -228),
    cp(0x9096ea6f_3848984f, -794, -220),
    cp(0xd77485cb_25823ac7, -768, -212),
    cp(0xa086cfcd_97bf97f4, -741, -204),
    cp(0xef340a98_172aace5, -715, -196),
    cp(0xb23867fb_2a35b28e, -688, -188),
    cp(0x84c8d4df_d2c63f3b, -661, -180),
    cp(0xc5dd4427_1ad3cdba, -635, -172),
    cp(0x936b9fce_bb25c996, -608, -164),
    cp(0xdbac6c24_7d62a584, -582, -156),
    cp(0xa3ab6658_0d5fdaf6, -555, -148),
    cp(0xf3e2f893_dec3f126, -529, -140),
    cp(0xb5b5ada8_aaff80b8, -502, -132),
    cp(0x87625f05_6c7c4a8b, -475, -124),
    cp(0xc9bcff60_34c13053, -449, -116),
    cp(0x964e858c_91ba2655, -422, -108),
    cp(0xdff97724_70297ebd, -396, -100),
    cp(0xa6dfbd9f_b8e5b88f, -369, -92),
    cp(0xf8a95fcf_88747d94, -343, -84),
    cp(0xb9447093_8fa89bcf, -316, -76),
    cp(0x8a08f0f8_bf0f156b, -289, -68),
    cp(0xcdb02555_653131b6, -263, -60),
    cp(0x993fe2c6_d07b7fac, -236, -52),
    cp(0xe45c10c4_2a2b3b06, -210, -44),
    cp(0xaa242499_697392d3, -183, -36),
    cp(0xfd87b5f2_8300ca0e, -157, -28),
    cp(0xbce50864_92111aeb, -130, -20),
    cp(0x8cbccc09_6f5088cc, -103, -12),
    cp(0xd1b71758_e219652c, -77, -4),
    cp(0x9c400000_00000000, -50, 4),
    cp(0xe8d4a510_00000000, -24, 12),
    cp(0xad78ebc5_ac620000, 3, 20),
    cp(0x813f3978_f8940984, 30, 28),
    cp(0xc097ce7b_c90715b3, 56, 36),
    cp(0x8f7e32ce_7bea5c70, 83, 44),
    cp(0xd5d238a4_abe98068, 109, 52),
    cp(0x9f4f2726_179a2245, 136, 60),
    cp(0xed63a231_d4c4fb27, 162, 68),
    cp(0xb0de6538_8cc8ada8, 189, 76),
    cp(0x83c7088e_1aab65db, 216, 84),
    cp(0xc45d1df9_42711d9a, 242, 92),
    cp(0x924d692c_a61be758, 269, 100),
    cp(0xda01ee64_1a708dea, 295, 108),
    cp(0xa26da399_9aef774a, 322, 116),
    cp(0xf209787b_b47d6b85, 348, 124),
    cp(0xb454e4a1_79dd1877, 375, 132),
    cp(0x865b8692_5b9bc5c2, 402, 140),
    cp(0xc83553c5_c8965d3d, 428, 148),
    cp(0x952ab45c_fa97a0b3, 455, 156),
    cp(0xde469fbd_99a05fe3, 481, 164),
    cp(0xa59bc234_db398c25, 508, 172),
    cp(0xf6c69a72_a3989f5c, 534, 180),
    cp(0xb7dcbf53_54e9bece, 561, 188),
    cp(0x88fcf317_f22241e2, 588, 196),
    cp(0xcc20ce9b_d35c78a5, 614, 204),
    cp(0x98165af3_7b2153df, 641, 212),
    cp(0xe2a0b5dc_971f303a, 667, 220),
    cp(0xa8d9d153_5ce3b396, 694, 228),
    cp(0xfb9b7cd9_a4a7443c, 720, 236),
    cp(0xbb764c4c_a7a44410, 747, 244),
    cp(0x8bab8eef_b6409c1a, 774, 252),
    cp(0xd01fef10_a657842c, 800, 260),
    cp(0x9b10a4e5_e9913129, 827, 268),
    cp(0xe7109bfb_a19c0c9d, 853, 276),
    cp(0xac2820d9_623bf429, 880, 284),
    cp(0x80444b5e_7aa7cf85, 907, 292),
    cp(0xbf21e440_03acdd2d, 933, 300),
    cp(0x8e679c2f_5e44ff8f, 960, 308),
    cp(0xd433179d_9c8cb841, 986, 316),
    cp(0x9e19db92_b4e31ba9, 1013, 324),
    cp(0xeb96bf6e_badf77d9, 1039, 332),
    cp(0xaf87023b_9bf0ee6b, 1066, 340),
];

/// -1 * o primeiro decimal_exponent da tabela.
const K_CACHED_POWERS_OFFSET: i32 = 348;
/// 1 / lg(10)
const K_D_1_LOG2_10: f64 = 0.30102999566398114;

pub struct PowersOfTenCache;

impl PowersOfTenCache {
    /// Nem todas as potências de dez estão em cache. O expoente decimal de dois números
    /// vizinhos do cache difere por `K_DECIMAL_EXPONENT_DISTANCE`.
    pub const K_DECIMAL_EXPONENT_DISTANCE: i32 = 8;

    pub const K_MIN_DECIMAL_EXPONENT: i32 = -348;
    pub const K_MAX_DECIMAL_EXPONENT: i32 = 340;

    /// Devolve uma potência de dez em cache com expoente binário no intervalo
    /// [min_exponent; max_exponent] (limites incluídos).
    pub fn get_cached_power_for_binary_exponent_range(
        min_exponent: i32,
        _max_exponent: i32,
        power: &mut DiyFp,
        decimal_exponent: &mut i32,
    ) {
        let k_q: i32 = DiyFp::K_SIGNIFICAND_SIZE;
        let k: f64 = (((min_exponent + k_q - 1) as f64) * K_D_1_LOG2_10).ceil();
        let foo: i32 = K_CACHED_POWERS_OFFSET;
        let index: i32 = (foo + (k as i32) - 1) / Self::K_DECIMAL_EXPONENT_DISTANCE + 1;
        // ASSERT(0 <= index && index < kCachedPowers.size())
        let cached_power = &K_CACHED_POWERS[index as usize];
        *decimal_exponent = cached_power.decimal_exponent as i32;
        *power = DiyFp::new(
            cached_power.significand,
            cached_power.binary_exponent as i32,
        );
    }

    /// Devolve uma potência de dez em cache x ~= 10^k tal que
    ///   k <= decimal_exponent < k + K_DECIMAL_EXPONENT_DISTANCE.
    /// O expoente pedido deve satisfazer
    ///   K_MIN_DECIMAL_EXPONENT <= requested_exponent e
    ///   requested_exponent < K_MAX_DECIMAL_EXPONENT + K_DECIMAL_EXPONENT_DISTANCE.
    pub fn get_cached_power_for_decimal_exponent(
        requested_exponent: i32,
        power: &mut DiyFp,
        found_exponent: &mut i32,
    ) {
        let index: i32 =
            (requested_exponent + K_CACHED_POWERS_OFFSET) / Self::K_DECIMAL_EXPONENT_DISTANCE;
        let cached_power = &K_CACHED_POWERS[index as usize];
        *power = DiyFp::new(
            cached_power.significand,
            cached_power.binary_exponent as i32,
        );
        *found_exponent = cached_power.decimal_exponent as i32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_exponent_range() {
        // k = ceil((-60 + 63) * log10(2)) = 1; índice = (348 + 1 - 1) / 8 + 1 = 44 (10^4).
        let mut power = DiyFp::default();
        let mut decimal_exponent = 0;
        PowersOfTenCache::get_cached_power_for_binary_exponent_range(
            -60,
            -32,
            &mut power,
            &mut decimal_exponent,
        );
        assert_eq!(decimal_exponent, 4);
        assert_eq!(power.f(), 0x9c40_0000_0000_0000);
        assert_eq!(power.e(), -50);
    }

    #[test]
    fn decimal_exponent() {
        // índice = (0 + 348) / 8 = 43 (10^-4).
        let mut power = DiyFp::default();
        let mut found = 0;
        PowersOfTenCache::get_cached_power_for_decimal_exponent(0, &mut power, &mut found);
        assert_eq!(found, -4);
        assert_eq!(power.f(), 0xd1b7_1758_e219_652c);
        assert_eq!(power.e(), -77);
    }

    #[test]
    fn table_extremes() {
        assert_eq!(K_CACHED_POWERS.len(), 87);
        let first = &K_CACHED_POWERS[0];
        let last = &K_CACHED_POWERS[86];
        assert_eq!(first.decimal_exponent as i32, PowersOfTenCache::K_MIN_DECIMAL_EXPONENT);
        assert_eq!(last.decimal_exponent as i32, PowersOfTenCache::K_MAX_DECIMAL_EXPONENT);
    }
}
