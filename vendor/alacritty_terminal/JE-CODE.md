# alacritty_terminal vendorizado no je-code

Cópia do `alacritty_terminal` 0.25.1 (crates.io, licença Apache-2.0) usada pelo `crates/je-terminal`, aplicada por
`[patch.crates-io]` no `Cargo.toml` da raiz.

Motivo: o crate publicado depende sem condição de `polling` e `home` e compila o `tty` e o `event_loop`, que não
existem no navegador, então o alvo `wasm32-unknown-unknown` não compila. No je-code o emulador (`term`, `grid`,
`vte`) roda em todas as plataformas e a ponte com o processo (pty local) só fora do wasm; no web os bytes chegam
pelo terminal remoto, como no VS Code web.

Removidos da cópia: `tests/`, `Cargo.lock`, `Cargo.toml.orig` e `CHANGELOG.md`.
