//! Porte de `JavaScriptCore/wasm` (WebAssembly). Plano e ordem das fatias em
//! `wip-notes/wasm-plan.md`.
pub mod page_count;
pub mod wasm_address_type;
pub mod wasm_const_expr_generator;
pub mod wasm_const_expr_interpreter;
pub mod wasm_exception_type;
pub mod wasm_global;
pub mod wasm_instance;
pub mod wasm_call_stack;
pub mod wasm_ipint;
pub mod wasm_memory;
pub mod wasm_table;
pub mod wasm_simd;
pub mod wasm_simd_opcodes;
pub mod wasm_format;
pub mod wasm_function_parser;
mod wasm_function_parser_gc;
mod wasm_function_parser_simd;
pub mod wasm_function_validator;
pub mod wasm_limits;
pub mod wasm_memory_information;
pub mod wasm_module_information;
pub mod wasm_name_section;
pub mod wasm_ops;
pub mod wasm_parser;
pub mod wasm_section_parser;
pub mod wasm_sections;
pub mod wasm_streaming_parser;
