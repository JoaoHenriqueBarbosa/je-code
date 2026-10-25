# alacritty_terminal vendorizado no je-code

Cópia do `alacritty_terminal` 0.25.1 (crates.io, licença Apache-2.0) usada pelo `crates/je-terminal`, aplicada por
`[patch.crates-io]` no `Cargo.toml` da raiz.

Motivo: o crate publicado depende sem condição de `polling` e `home` e compila o `tty` e o `event_loop`, que não
existem no navegador, então o alvo `wasm32-unknown-unknown` não compila. No je-code o emulador (`term`, `grid`,
`vte`) roda em todas as plataformas e a ponte com o processo (pty local) só fora do wasm; no web os bytes chegam
pelo terminal remoto, como no VS Code web.

Removidos da cópia: `tests/`, `Cargo.lock`, `Cargo.toml.orig` e `CHANGELOG.md`.

## Mudanças locais

- `Cargo.toml`: `polling`, `home` e `libc` saíram de `[dependencies]` para
  `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`; `rustix`, `rustix-openpty`, `signal-hook` (unix) e
  `miow`, `piper`, `windows-sys` (windows) ganharam `not(target_arch = "wasm32")` na condição que já tinham. Nenhuma
  delas compila (ou serve) no navegador.
- `src/lib.rs`: os módulos `event_loop`, `thread` e `tty` (os únicos que usam essas dependências, o pty, o `polling`
  e as threads) ficam sob `#[cfg(not(target_arch = "wasm32"))]`. O emulador (`term`, `grid`, `index`, `selection`,
  `vi_mode`, `event`, `sync`, `vte`) não toca nelas e compila no wasm.
