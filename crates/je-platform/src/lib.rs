//! Porte de `src/vs/platform` do VS Code.
//!
//! Por ora só a configuração (`platform/configuration/common`). Os valores dinâmicos do original
//! (`unknown`, `IStringDictionary<unknown>`) são [`je_base::dynamic::Dynamic`]; o texto que é chave
//! de configuração é [`je_base::JsString`] (UTF-16, como no JavaScript).

pub mod configuration;
pub mod files;
pub mod js;
pub mod log;
pub mod workspace;
