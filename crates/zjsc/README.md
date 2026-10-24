# zjsc

O JavaScriptCore (WebKit do Bun 1.4.2) traduzido para Rust sem `unsafe`: o motor JavaScript que roda as
extensões do VS Code no host de extensões do je-code (Fase 5, `wip/notes/exthost-plan.md`).

## Origem

- Projeto: pseudo-linus, clonado em `ref/pseudo-linus`, commit `9e3d0767` (2026-10-12), caminho `crates/zjsc`.
- Cópia de `Cargo.toml`, `src/`, `data/unicode`, `LICENSE.md` e `NOTICE.md`, mais `tests/golden/math_bun.tsv`
  (o `include_str!` do teste de `Math` em `src/runtime/math_common.rs`). Ficaram de fora `tests/` (367 MB de
  golden e test262), `upstream/` (fontes C++ de referência), `derived/`, `scripts/`, `bench/`, `examples/`,
  `wip-notes/` e `vendor/`.
- Licença: `LGPL-2.0-or-later AND BSD-2-Clause` (`LICENSE.md` e `NOTICE.md` copiados sem alteração). O código do
  Bun é MIT; o do WebKit, descrito no `NOTICE.md`.
- Dependências do mesmo repositório, copiadas ao lado: `crates/ul-common`, `crates/sysabi`, `crates/zdeflate`.

## Mudanças locais (lista completa)

1. `Cargo.toml`: `icu_collator` aponta para `../../vendor/icu_collator` (o vendor do je-code, que é a mesma
   versão 2.3.1 com a correção do surrogate solitário), porque duas cópias do mesmo pacote colidem no
   `Cargo.lock`. Saíram o `[dev-dependencies] bun_transpile` (só os testes do zjsc o instalavam, e a pasta
   `tests/` não veio) e os `[profile.*]` (perfil só vale na raiz do workspace).
2. `crates/sysabi/build.rs`: lê `data/linux_facts.json` (cópia de `testbench/golden/linux-facts/linux_facts.json`
   do pseudo-linus) em vez de um caminho fora da crate. `Cargo.toml` de `sysabi` e `ul-common`: `publish = false`
   e `sysabi` por caminho, no lugar de `workspace = true`.

## Pendência de DRY

`crates/zyarr` é o YARR extraído deste `src/yarr`: o código existe nas duas crates. Fundir (o zjsc dependendo do
`zyarr`) exige separar o `yarr` da `VM` e das opções do motor; fica como trabalho registrado, não feito.
