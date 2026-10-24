//! Porte de `runtime/VarOffset.h` e `VarOffset.cpp` (`dump` e `printInternal` são só depuração).

use crate::bytecode::virtual_register::VirtualRegister;
pub use crate::runtime::direct_arguments_offset::DirectArgumentsOffset;
pub use crate::runtime::scope_offset::ScopeOffset;

/// `enum class VarKind : uint8_t`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VarKind {
    Invalid,
    Scope,
    Stack,
    DirectArgument,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VarOffset {
    kind: VarKind,
    offset: u32,
}

impl Default for VarOffset {
    fn default() -> Self {
        VarOffset { kind: VarKind::Invalid, offset: u32::MAX }
    }
}

impl VarOffset {
    /// `explicit VarOffset(VirtualRegister stackOffset)`.
    pub fn from_virtual_register(stack_offset: VirtualRegister) -> Self {
        if !stack_offset.is_valid() {
            VarOffset::default()
        } else {
            VarOffset { kind: VarKind::Stack, offset: stack_offset.offset() as u32 }
        }
    }

    /// `explicit VarOffset(ScopeOffset scopeOffset)`.
    pub fn from_scope_offset(scope_offset: ScopeOffset) -> Self {
        if scope_offset.is_invalid() {
            VarOffset::default()
        } else {
            VarOffset { kind: VarKind::Scope, offset: scope_offset.offset() }
        }
    }

    /// `explicit VarOffset(DirectArgumentsOffset capturedArgumentsOffset)`.
    pub fn from_direct_arguments_offset(captured_arguments_offset: DirectArgumentsOffset) -> Self {
        if captured_arguments_offset.is_invalid() {
            VarOffset::default()
        } else {
            VarOffset { kind: VarKind::DirectArgument, offset: captured_arguments_offset.offset() }
        }
    }

    pub fn assemble(kind: VarKind, offset: u32) -> Self {
        let result = VarOffset { kind, offset };
        result.check_sanity();
        result
    }

    pub fn is_valid(&self) -> bool {
        self.kind != VarKind::Invalid
    }

    pub fn kind(&self) -> VarKind {
        self.kind
    }

    pub fn is_stack(&self) -> bool {
        self.kind == VarKind::Stack
    }

    pub fn is_scope(&self) -> bool {
        self.kind == VarKind::Scope
    }

    pub fn is_direct_argument(&self) -> bool {
        self.kind == VarKind::DirectArgument
    }

    pub fn stack_offset_unchecked(&self) -> VirtualRegister {
        if !self.is_stack() {
            return VirtualRegister::default();
        }
        VirtualRegister::new(self.offset as i32)
    }

    pub fn scope_offset_unchecked(&self) -> ScopeOffset {
        if !self.is_scope() {
            return ScopeOffset::default();
        }
        ScopeOffset::new(self.offset)
    }

    pub fn captured_arguments_offset_unchecked(&self) -> DirectArgumentsOffset {
        if !self.is_direct_argument() {
            return DirectArgumentsOffset::default();
        }
        DirectArgumentsOffset::new(self.offset)
    }

    pub fn stack_offset(&self) -> VirtualRegister {
        debug_assert!(self.is_stack());
        VirtualRegister::new(self.offset as i32)
    }

    pub fn scope_offset(&self) -> ScopeOffset {
        debug_assert!(self.is_scope());
        ScopeOffset::new(self.offset)
    }

    pub fn captured_arguments_offset(&self) -> DirectArgumentsOffset {
        debug_assert!(self.is_direct_argument());
        DirectArgumentsOffset::new(self.offset)
    }

    pub fn raw_offset(&self) -> u32 {
        debug_assert!(self.is_valid());
        self.offset
    }

    pub fn check_sanity(&self) {
        match self.kind {
            VarKind::Invalid => debug_assert!(self.offset == u32::MAX),
            VarKind::Scope => debug_assert!(self.scope_offset().is_valid()),
            VarKind::Stack => debug_assert!(self.stack_offset().is_valid()),
            VarKind::DirectArgument => debug_assert!(self.captured_arguments_offset().is_valid()),
        }
    }
}
