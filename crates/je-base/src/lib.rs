//! Porte de `src/vs/base/common` do VS Code.
//!
//! Texto segue a semântica do JavaScript: deslocamentos, comprimentos e colunas contam unidades
//! UTF-16, porque é assim que o VS Code inteiro (modelo do editor, protocolo das extensões, settings)
//! mede posição. O tipo [`JsString`] guarda o texto nessa forma, inclusive surrogates isolados, que
//! um `String` do Rust não representa.

pub mod json;
pub mod json_error_messages;
pub mod char_code;
pub mod errors;
pub mod lifecycle;
pub mod js_string;
pub mod nls;
pub mod runtime;
pub mod strings;
#[rustfmt::skip]
mod strings_tables;
pub mod path;
pub mod promise;

pub use js_string::JsString;
