//! Tradução de `runtime/CodeSpecializationKind.h` e `.cpp`.

use std::fmt;

/// `enum class CodeSpecializationKind : uint8_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CodeSpecializationKind {
    CodeForCall = 0,
    CodeForConstruct = 1,
}

/// `specializationFromIsCall`.
pub fn specialization_from_is_call(is_call: bool) -> CodeSpecializationKind {
    if is_call {
        CodeSpecializationKind::CodeForCall
    } else {
        CodeSpecializationKind::CodeForConstruct
    }
}

/// `specializationFromIsConstruct`.
pub fn specialization_from_is_construct(is_construct: bool) -> CodeSpecializationKind {
    if is_construct {
        CodeSpecializationKind::CodeForConstruct
    } else {
        CodeSpecializationKind::CodeForCall
    }
}

/// `printInternal(PrintStream&, CodeSpecializationKind)`: o `PrintStream` vira `fmt::Write`.
pub fn print_internal(out: &mut dyn fmt::Write, kind: CodeSpecializationKind) -> fmt::Result {
    if kind == CodeSpecializationKind::CodeForCall {
        return out.write_str("Call");
    }

    out.write_str("Construct")
}
