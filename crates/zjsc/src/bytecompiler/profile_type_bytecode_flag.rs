//! Porte de `bytecompiler/ProfileTypeBytecodeFlag.h` e `.cpp`.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProfileTypeBytecodeFlag {
    ProfileTypeBytecodeClosureVar,
    ProfileTypeBytecodeLocallyResolved,
    ProfileTypeBytecodeDoesNotHaveGlobalID,
    ProfileTypeBytecodeFunctionArgument,
    ProfileTypeBytecodeFunctionReturnStatement,
}

/// `printInternal(PrintStream&, ProfileTypeBytecodeFlag)`.
impl fmt::Display for ProfileTypeBytecodeFlag {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(match self {
            ProfileTypeBytecodeFlag::ProfileTypeBytecodeClosureVar => "ProfileTypeBytecodeClosureVar",
            ProfileTypeBytecodeFlag::ProfileTypeBytecodeLocallyResolved => {
                "ProfileTypeBytecodeLocallyResolved"
            }
            ProfileTypeBytecodeFlag::ProfileTypeBytecodeDoesNotHaveGlobalID => {
                "ProfileTypeBytecodeDoesNotHaveGlobalID"
            }
            ProfileTypeBytecodeFlag::ProfileTypeBytecodeFunctionArgument => {
                "ProfileTypeBytecodeFunctionArgument"
            }
            ProfileTypeBytecodeFlag::ProfileTypeBytecodeFunctionReturnStatement => {
                "ProfileTypeBytecodeFunctionReturnStatement"
            }
        })
    }
}
