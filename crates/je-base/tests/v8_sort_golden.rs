//! Conferência de `sort_by_compare` contra o `Array.prototype.sort` do V8 rodando no Node, com comparadores
//! determinísticos que não são ordem total (`random`: resultado arbitrário; `noisy`: ordem por chave com 10% dos pares
//! invertidos) e com uma ordem total com empates (`consistent`, que confere a estabilidade).
//!
//! O arquivo `golden/v8_sort.jsonl` é gerado por `oracle-harness/v8-sort/gen.js`, e a primeira linha registra as versões do
//! Node e do V8. Para cada modo, tamanho (0 a 70, e 100 a 2000) e semente, o porte tem de reproduzir a quantidade de
//! chamadas ao comparador, o hash FNV-1a da sequência de pares passados a ele e o hash do vetor final.

use je_base::arrays::sort_by_compare;

fn mix(mut x: u32) -> u32 {
    x = (x ^ (x >> 16)).wrapping_mul(0x7feb352d);
    x = (x ^ (x >> 15)).wrapping_mul(0x846ca68b);
    x ^ (x >> 16)
}

fn pair_hash(seed: u32, a: u32, b: u32) -> u32 {
    mix(seed.wrapping_mul(0x9e3779b1) ^ (a + 1).wrapping_mul(0x85ebca6b) ^ (b + 7).wrapping_mul(0xc2b2ae35))
}

fn fnv(mut h: u32, v: u32) -> u32 {
    for i in 0..4 {
        h = (h ^ ((v >> (8 * i)) & 0xff)).wrapping_mul(0x01000193);
    }
    h
}

fn compare(mode: &str, seed: u32, a: u32, b: u32) -> f64 {
    match mode {
        "random" => (pair_hash(seed, a, b) % 3) as f64 - 1.0,
        "noisy" => {
            let (ka, kb) = (mix(seed ^ a) % 50, mix(seed ^ b) % 50);
            let r = (ka as i64 - kb as i64).signum() as f64;
            if pair_hash(seed, a, b) % 10 == 0 { -r } else { r }
        }
        "consistent" => (mix(seed ^ a) % 50) as f64 - (mix(seed ^ b) % 50) as f64,
        other => panic!("modo desconhecido: {other}"),
    }
}

#[test]
fn sort_by_compare_matches_v8() {
    let golden = include_str!("golden/v8_sort.jsonl");
    let mut failures = Vec::new();
    let mut checked = 0;
    for line in golden.lines().filter(|line| line.starts_with('{')) {
        let case: serde_json::Value = serde_json::from_str(line).unwrap();
        let mode = case["mode"].as_str().unwrap();
        let n = case["n"].as_u64().unwrap() as u32;
        let seed = case["seed"].as_u64().unwrap() as u32;
        let mut array: Vec<u32> = (0..n).collect();
        let (calls, seq) = (std::cell::Cell::new(0u64), std::cell::Cell::new(0x811c9dc5u32));
        sort_by_compare(&mut array, |a, b| {
            calls.set(calls.get() + 1);
            seq.set(fnv(fnv(seq.get(), *a), *b));
            compare(mode, seed, *a, *b)
        });
        let result = array.iter().fold(0x811c9dc5u32, |h, v| fnv(h, *v));
        let expected = (case["calls"].as_u64().unwrap(), case["seq"].as_u64().unwrap() as u32, case["result"].as_u64().unwrap() as u32);
        if (calls.get(), seq.get(), result) != expected {
            failures.push(format!("{mode} n={n} seed={seed}: calls {} seq {} result {} (esperado {expected:?})", calls.get(), seq.get(), result));
        }
        checked += 1;
    }
    assert_eq!(checked, 1200);
    assert!(failures.is_empty(), "{} de {checked} divergem:\n{}", failures.len(), failures.iter().take(20).cloned().collect::<Vec<_>>().join("\n"));
}
