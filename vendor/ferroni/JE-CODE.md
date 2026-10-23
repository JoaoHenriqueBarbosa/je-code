# ferroni vendorizado no je-code

Cópia do `ferroni` 2.5.2 (crates.io, BSD-2-Clause, sebastian-software/ferroni) usada por
`[patch.crates-io]` no `Cargo.toml` da raiz. Mudança local, uma só:

- `src/regsyntax.rs`, `OnigSyntaxOniguruma`: sem `ONIG_SYN_ESC_P_WITH_ONE_CHAR_PROP`, para `\p` sem
  chave ser `p` literal como no Oniguruma do `vscode-oniguruma` do VS Code 1.143.1. Sem isso a gramática
  `log` embutida não compila (H14, `testbench/textmate-probe`). A correção deve ir como PR ao upstream,
  com a sintaxe do VS Code como sintaxe própria em vez de mudar a padrão.
