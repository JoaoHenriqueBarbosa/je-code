//! JavaScriptCore traduzido para Rust. Ver `CONVENTIONS.md`.
#![forbid(unsafe_code)]
// Constante de token (ou de enum) que falta importar vira, num `match`, um padrão de captura que
// casa tudo: o parser tomava `0b2` por `this`. Estas duas lints pegam o erro na compilação.
#![deny(non_snake_case, unreachable_patterns)]

// Os testes de unidade incluem `tests/common/std_host.rs`, escrito com o nome da crate como os de integração.
#[cfg(test)]
extern crate self as zjsc;

pub mod wtf;
pub mod yarr;
pub mod bytecode;
pub mod bytecompiler;
pub mod runtime;
pub mod interpreter;
pub mod llint;
pub mod parser;
pub mod debugger;
pub mod api;
pub mod wasm;
