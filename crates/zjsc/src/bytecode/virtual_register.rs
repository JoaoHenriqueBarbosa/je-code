//! Porte de `bytecode/VirtualRegister.h` e `VirtualRegister.cpp`.
//!
//! `CallFrame::thisArgumentOffset()` e `CallFrameSlot` são resolvidos para Linux x86_64
//! (`CallerFrameAndPC::sizeInRegisters` = 2), logo `codeBlock` = 2, `callee` = 3,
//! `argumentCountIncludingThis` = 4, `thisArgument` = 5, `firstArgument` = 6.

use std::fmt;
use std::ops::{Add, AddAssign, Sub, SubAssign};

use crate::bytecompiler::register_id::RegisterID;

/// `FirstConstantRegisterIndex` de `BytecodeConventions.h`.
pub const FIRST_CONSTANT_REGISTER_INDEX: i32 = 0x4000_0000;
/// `FirstConstantRegisterIndex8` de `BytecodeConventions.h`.
pub const FIRST_CONSTANT_REGISTER_INDEX8: i32 = 16;
/// `FirstConstantRegisterIndex16` de `BytecodeConventions.h`.
pub const FIRST_CONSTANT_REGISTER_INDEX16: i32 = 64;

/// `CallFrameSlot` de `CallFrame.h`, com os valores inteiros do C++.
pub mod call_frame_slot {
    pub const CODE_BLOCK: i32 = 2;
    pub const CALLEE: i32 = CODE_BLOCK + 1;
    pub const ARGUMENT_COUNT_INCLUDING_THIS: i32 = CALLEE + 1;
    pub const THIS_ARGUMENT: i32 = ARGUMENT_COUNT_INCLUDING_THIS + 1;
    pub const FIRST_ARGUMENT: i32 = THIS_ARGUMENT + 1;
}

/// `CallFrame::thisArgumentOffset()`.
const THIS_ARGUMENT_OFFSET: i32 = call_frame_slot::THIS_ARGUMENT;

/// `sizeof(Register)`.
const SIZEOF_REGISTER: i32 = 8;

pub fn virtual_register_is_local(operand: i32) -> bool {
    operand < 0
}

pub fn virtual_register_is_argument(operand: i32) -> bool {
    operand >= 0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VirtualRegister {
    virtual_register: i32,
}

impl Default for VirtualRegister {
    fn default() -> Self {
        VirtualRegister { virtual_register: Self::INVALID_VIRTUAL_REGISTER }
    }
}

impl VirtualRegister {
    pub const INVALID_VIRTUAL_REGISTER: i32 = 0x3fff_ffff;
    pub const FIRST_CONSTANT_REGISTER_INDEX: i32 = FIRST_CONSTANT_REGISTER_INDEX;

    pub fn new(virtual_register: i32) -> Self {
        VirtualRegister { virtual_register }
    }

    /// `VirtualRegister(RegisterID*)`.
    pub fn from_register_id(reg: &RegisterID) -> Self {
        VirtualRegister { virtual_register: reg.raw_virtual_register().virtual_register }
    }

    pub fn is_valid(&self) -> bool {
        self.virtual_register != Self::INVALID_VIRTUAL_REGISTER
    }

    pub fn is_local(&self) -> bool {
        virtual_register_is_local(self.virtual_register)
    }

    pub fn is_argument(&self) -> bool {
        virtual_register_is_argument(self.virtual_register)
    }

    pub fn is_header(&self) -> bool {
        self.virtual_register >= 0 && self.virtual_register < call_frame_slot::THIS_ARGUMENT
    }

    pub fn is_constant(&self) -> bool {
        self.virtual_register >= Self::FIRST_CONSTANT_REGISTER_INDEX
    }

    pub fn to_local(&self) -> i32 {
        debug_assert!(self.is_local());
        Self::operand_to_local(self.virtual_register)
    }

    pub fn to_argument(&self) -> i32 {
        debug_assert!(self.is_argument());
        Self::operand_to_argument(self.virtual_register)
    }

    pub fn to_constant_index(&self) -> i32 {
        debug_assert!(self.is_constant());
        self.virtual_register - Self::FIRST_CONSTANT_REGISTER_INDEX
    }

    pub fn offset(&self) -> i32 {
        self.virtual_register
    }

    pub fn offset_in_bytes(&self) -> i32 {
        self.virtual_register.wrapping_mul(SIZEOF_REGISTER)
    }

    fn local_to_operand(local: i32) -> i32 {
        -1 - local
    }

    fn operand_to_local(operand: i32) -> i32 {
        -1 - operand
    }

    fn operand_to_argument(operand: i32) -> i32 {
        operand - THIS_ARGUMENT_OFFSET
    }

    fn argument_to_operand(argument: i32) -> i32 {
        argument + THIS_ARGUMENT_OFFSET
    }
}

impl From<i32> for VirtualRegister {
    fn from(value: i32) -> Self {
        VirtualRegister::new(value)
    }
}

impl Add<i32> for VirtualRegister {
    type Output = VirtualRegister;
    fn add(self, value: i32) -> VirtualRegister {
        VirtualRegister::new(self.offset().wrapping_add(value))
    }
}

impl Sub<i32> for VirtualRegister {
    type Output = VirtualRegister;
    fn sub(self, value: i32) -> VirtualRegister {
        VirtualRegister::new(self.offset().wrapping_sub(value))
    }
}

impl Add<VirtualRegister> for VirtualRegister {
    type Output = VirtualRegister;
    fn add(self, value: VirtualRegister) -> VirtualRegister {
        VirtualRegister::new(self.offset().wrapping_add(value.offset()))
    }
}

impl Sub<VirtualRegister> for VirtualRegister {
    type Output = VirtualRegister;
    fn sub(self, value: VirtualRegister) -> VirtualRegister {
        VirtualRegister::new(self.offset().wrapping_sub(value.offset()))
    }
}

impl AddAssign<i32> for VirtualRegister {
    fn add_assign(&mut self, value: i32) {
        *self = *self + value;
    }
}

impl SubAssign<i32> for VirtualRegister {
    fn sub_assign(&mut self, value: i32) {
        *self = *self - value;
    }
}

/// `VirtualRegister::dump(PrintStream&)`.
impl fmt::Display for VirtualRegister {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.is_valid() {
            return out.write_str("<invalid>");
        }
        if self.is_header() {
            if self.virtual_register == call_frame_slot::CODE_BLOCK {
                out.write_str("codeBlock")?;
            } else if self.virtual_register == call_frame_slot::CALLEE {
                out.write_str("callee")?;
            } else if self.virtual_register == call_frame_slot::ARGUMENT_COUNT_INCLUDING_THIS {
                out.write_str("argumentCountIncludingThis")?;
            } else if self.virtual_register == 0 {
                out.write_str("callerFrame")?;
            } else if self.virtual_register == 1 {
                out.write_str("returnPC")?;
            }
            return Ok(());
        }
        if self.is_constant() {
            return write!(out, "const{}", self.to_constant_index());
        }
        if self.is_argument() {
            if self.to_argument() == 0 {
                return out.write_str("this");
            }
            return write!(out, "arg{}", self.to_argument());
        }
        if self.is_local() {
            return write!(out, "loc{}", self.to_local());
        }
        unreachable!()
    }
}

/// `virtualRegisterForLocal`.
pub fn virtual_register_for_local(local: i32) -> VirtualRegister {
    VirtualRegister::new(VirtualRegister::local_to_operand(local))
}

/// `virtualRegisterForArgumentIncludingThis`.
pub fn virtual_register_for_argument_including_this(argument: i32, offset: i32) -> VirtualRegister {
    VirtualRegister::new(VirtualRegister::argument_to_operand(argument) + offset)
}
