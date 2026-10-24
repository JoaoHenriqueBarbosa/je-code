//! `BytecodeGenerator::ExistingVariableMode` de `bytecompiler/BytecodeGenerator.h:420`
//! (`enum ExistingVariableMode { VerifyExisting, IgnoreExisting };`).
//!
//! No C++ é enum aninhado da classe; fica em módulo próprio porque o `BytecodeGenerator` é
//! juntado por `include!` e o enum precisa de um lugar único para os dois arquivos importarem.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExistingVariableMode {
    VerifyExisting,
    IgnoreExisting,
}
