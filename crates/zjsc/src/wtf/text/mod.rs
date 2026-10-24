//! Porte de `WTF/wtf/text`.
pub mod conversion_mode;
pub mod string_hasher;
pub mod string_impl;
pub mod atom_string;
pub mod atom_string_impl;
pub mod uniqued_string_impl;
pub mod symbol_impl;
pub mod wtf_string;
pub mod text_position;
pub mod string_view;
pub mod string_common;
pub mod string_builder;
pub mod string_concatenate;
pub mod base64;

pub use string_concatenate::{make_string_dyn, try_make_string, try_make_string_dyn};
