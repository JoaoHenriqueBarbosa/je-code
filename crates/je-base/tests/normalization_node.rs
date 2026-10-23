//! Conferência de `normalization` contra o `String.prototype.normalize` do Node (V8 com ICU) e contra o
//! `tryNormalizeToBase` do original, rodado no Node, em casos com acento composto e decomposto, hangul,
//! compatibilidade, ordem canônica e surrogates isolados. Cada linha é `entrada | NFC | NFD | base`, em
//! unidades UTF-16 em hexadecimal (`-` é o texto vazio).

use je_base::normalization::{normalize_nfc, normalize_nfd, try_normalize_to_base};

const GOLDEN: &[&str] = &[
    "- | - | - | -",
    "61 62 63 | 61 62 63 | 61 62 63 | 61 62 63",
    "63 61 66 e9 | 63 61 66 e9 | 63 61 66 65 301 | 63 61 66 65",
    "63 61 66 65 301 | 63 61 66 e9 | 63 61 66 65 301 | 63 61 66 65 301",
    "c5 | c5 | 41 30a | 61",
    "41 30a | c5 | 41 30a | 61 30a",
    "212b | c5 | 41 30a | 61",
    "fb01 | fb01 | fb01 | fb01",
    "1e9b 323 | 1e9b 323 | 17f 323 307 | 1e9b 323",
    "d55c ad6d c5b4 | d55c ad6d c5b4 | 1112 1161 11ab 1100 116e 11a8 110b 1165 | d55c ad6d c5b4",
    "1112 1161 11ab | d55c | 1112 1161 11ab | 1112 1161 11ab",
    "1c6 | 1c6 | 1c6 | 1c6",
    "61 d800 62 | 61 d800 62 | 61 d800 62 | 61 d800 62",
    "d800 301 | d800 301 | d800 301 | d800 301",
    "dc00 65 301 | dc00 e9 | dc00 65 301 | dc00 65 301",
    "d83d de00 65 301 | d83d de00 e9 | d83d de00 65 301 | d83d de00 65 301",
    "dc 6e ef 308 | dc 6e ef 308 | 55 308 6e 69 308 308 | fc 6e ef 308",
    "149 | 149 | 149 | 149",
    "344 | 308 301 | 308 301 | 344",
    "aa | aa | aa | aa",
    "c5 323 | 1ea0 30a | 41 323 30a | e5 323",
    "323 30a | 323 30a | 323 30a | 323 30a",
    "1c5 | 1c5 | 1c5 | 1c6",
];

fn units(field: &str) -> Vec<u16> {
    if field == "-" { Vec::new() } else { field.split(' ').map(|h| u16::from_str_radix(h, 16).unwrap()).collect() }
}

#[test]
fn normalization_matches_node() {
    for line in GOLDEN {
        let fields: Vec<Vec<u16>> = line.split(" | ").map(units).collect();
        let input = &fields[0];
        assert_eq!(normalize_nfc(input).units(), fields[1], "NFC de {line}");
        assert_eq!(normalize_nfd(input).units(), fields[2], "NFD de {line}");
        assert_eq!(try_normalize_to_base(input).units(), fields[3], "base de {line}");
    }
}
