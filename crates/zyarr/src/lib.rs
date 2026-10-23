//! YARR, o motor de `RegExp` do JavaScriptCore, em Rust sem `unsafe`, extraído do `zjsc` do
//! pseudo-linus (ver o README). A semântica é a do ECMAScript, a mesma que o VS Code usa via V8.
//!
//! Os módulos `wtf` e `yarr` são o porte do upstream, com os nomes originais para facilitar a
//! ressincronização; a API segura é o módulo `regexp`, reexportado aqui (`RegExp`, `Match`, `Flags`).

// Os módulos copiados mantêm o estilo do upstream (nomes do C++, itens que só o resto do JSC usa).
#[allow(dead_code, unused, non_camel_case_types, non_snake_case, non_upper_case_globals)]
pub mod wtf;
#[allow(dead_code, unused, non_camel_case_types, non_snake_case, non_upper_case_globals)]
pub mod yarr;

mod regexp;
pub use regexp::{ErrorKind, Flags, Match, RegExp, RegExpError};
