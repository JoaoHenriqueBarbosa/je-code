//! Tradução de `bytecode/CodeType.h`.

/// `enum CodeType : uint8_t { GlobalCode, EvalCode, FunctionCode, ModuleCode }`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CodeType {
    GlobalCode = 0,
    EvalCode = 1,
    FunctionCode = 2,
    ModuleCode = 3,
}
