//! Porte de `src/vs/base/common/symbols.ts`.

/// O atraso de `debounce`/`throttle`: um número de milissegundos ou o `MicrotaskDelay` do original
/// (um `Symbol` que pede `queueMicrotask` no lugar de `setTimeout`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delay {
    Millis(u64),
    Microtask,
}
