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
pub mod event;
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
pub mod random;
pub mod dynamic;
pub mod assert;
pub mod arrays;
pub mod arrays_find;
pub mod collections;
pub mod iterator;
pub mod linked_list;
pub mod objects;
pub mod types;
pub mod numbers;
pub mod hash;
pub mod uuid;
pub mod lazy;
pub mod cache;
pub mod uri;
pub mod extpath;
pub mod map;
pub mod cancellation;
pub mod stacktrace;
pub mod symbols;
pub mod ternary_search_tree;
pub mod network;
pub mod resources;
pub mod async_;
pub mod glob;
pub mod platform;
pub mod key_codes;
#[rustfmt::skip]
mod key_codes_tables;
pub mod collator;
pub mod keybindings;
pub mod keybinding_parser;
pub mod keybinding_labels;
pub mod comparers;
pub mod labels;
pub mod mime;
pub mod normalization;
pub mod natural_language;
pub mod filters;
pub mod fuzzy_scorer;
