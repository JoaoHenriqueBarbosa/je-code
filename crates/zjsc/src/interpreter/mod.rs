//! Porte de `JavaScriptCore/interpreter`.
pub mod call_frame;
pub mod callee_bits;
pub mod cloop_stack;
pub mod proto_call_frame;
pub mod register;
pub mod interpreter;
pub mod execute_call;
pub mod execute_eval;
pub mod execute_module_program;
pub mod unwind;
pub mod stack_visitor;
pub mod caller_source_origin;
