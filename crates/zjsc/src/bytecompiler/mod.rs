//! Porte de `JavaScriptCore/bytecompiler`: os headers pequenos, o `BytecodeGenerator`
//! (`bytecode_generator`, com os fragmentos juntados por `include!`) e o `NodesCodegen`
//! (`nodes_codegen`, idem).

pub mod existing_variable_mode;
pub mod label;
pub mod label_scope;
pub mod profile_type_bytecode_flag;
pub mod register_id;
pub mod static_property_analysis;
pub mod static_property_analyzer;
pub mod bytecode_generator_base;
pub mod identifier_map;
pub mod generator_parser_arena;
pub mod bytecode_generator;
pub mod bytecode_generatorification;
pub mod nodes_codegen;
