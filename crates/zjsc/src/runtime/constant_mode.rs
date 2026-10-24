//! Porte de `runtime/ConstantMode.h` e `ConstantMode.cpp` (`printInternal` é só depuração).

/// `enum class ConstantMode { IsConstant, IsVariable }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConstantMode {
    IsConstant,
    IsVariable,
}

pub fn mode_for_is_constant(is_constant: bool) -> ConstantMode {
    if is_constant {
        ConstantMode::IsConstant
    } else {
        ConstantMode::IsVariable
    }
}
