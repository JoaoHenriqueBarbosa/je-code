//! `runtime/TypeProfiler.h` e `runtime/ControlFlowProfiler.h`, só a identidade.
//!
//! O `VM` guarda `std::unique_ptr<TypeProfiler>` e `std::unique_ptr<ControlFlowProfiler>`, que ficam
//! nulos até o inspector ligá-los (`enableTypeProfiler`/`enableControlFlowProfiler`); o gerador de
//! bytecode e os executáveis só testam se existem. O conteúdo (`TypeLocation`, `BasicBlockLocation`,
//! as tabelas de consulta) só serve ao inspector e entra quando ele for portado.

/// `class TypeProfiler`.
#[derive(Debug, Default)]
pub struct TypeProfiler {
    _private: (),
}

/// `class ControlFlowProfiler`.
#[derive(Debug, Default)]
pub struct ControlFlowProfiler {
    _private: (),
}
