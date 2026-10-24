//! `UnlinkedFunctionExecutable` mora em `bytecode/` no JavaScriptCore; o bytecompiler também o alcança
//! por este caminho (`runtime::unlinked_function_executable`), que só reexporta o módulo canônico.

pub use crate::bytecode::unlinked_function_executable::{
    ClassElementDefinition, ClassElementDefinitionKind, RareData, UnlinkedFunctionExecutable,
    UnlinkedFunctionExecutableRef, UnlinkedFunctionKind,
};
