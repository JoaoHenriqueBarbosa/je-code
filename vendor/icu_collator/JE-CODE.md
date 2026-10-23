# icu_collator vendorizado no je-code

Cópia do `icu_collator` 2.3.1 (ICU4X, licença Unicode-3.0) como o pseudo-linus a vendoriza em
`crates/zjsc/vendor/icu_collator` (commit `9e3d0767` de 2026-10-12). É a mesma versão e o mesmo código que o
`Intl.Collator` do zjsc usa, para o `compareFileNames` e companhia do je-code ordenarem como o V8/ICU.

Mudança local em relação ao 2.3.1 do crates.io (feita no pseudo-linus, ver `src/comparison.rs` e `src/docs.rs`):
surrogate solitário em UTF-16 recebe o peso implícito do próprio code unit, como o ICU4C, em vez de virar U+FFFD.

Removidos da cópia: `tests/`, `benches/`, `Cargo.lock` e as seções `[[test]]`, `[[bench]]` e `dev-dependencies`
do `Cargo.toml`. Usada como dependência por caminho em `crates/je-base`.
