//! Porte de `bytecode/FunctionCodeBlock.h` e `FunctionCodeBlock.cpp`.
//!
//! DIVERGÊNCIA: `FunctionCodeBlock` tem o mesmo layout de `CodeBlock`
//! (`static_assert(sizeof(FunctionCodeBlock) == sizeof(CodeBlock))`) e só difere no `Structure` da
//! célula, que o porte não tem; o bloco criado é o `CodeBlockRef` comum. `s_info`, `subspaceFor` e
//! `createStructure` não existem.

use std::cell::RefCell;
use std::rc::Rc;

use crate::bytecode::code_block::{CodeBlock, CodeBlockRef};
use crate::bytecode::unlinked_code_block::UnlinkedFunctionCodeBlock;
use crate::runtime::function_executable::FunctionExecutable;
use crate::runtime::js_scope::JSScopeRef;
use crate::runtime::script_executable::ScriptExecutableRef;
use crate::runtime::vm::VM;

/// `class FunctionCodeBlock final : public CodeBlock`.
pub struct FunctionCodeBlock;

impl FunctionCodeBlock {
    /// `create(VM&, FunctionExecutable* ownerExecutable, UnlinkedFunctionCodeBlock*, JSScope*)`:
    /// `None` é o `nullptr` quando o `finishCreation` falha.
    pub fn create(
        vm: &VM,
        owner_executable: &Rc<RefCell<FunctionExecutable>>,
        unlinked_code_block: &Rc<RefCell<UnlinkedFunctionCodeBlock>>,
        scope: &JSScopeRef,
    ) -> Option<CodeBlockRef> {
        let base = unlinked_code_block.borrow().base_ref();
        CodeBlock::create(vm, &ScriptExecutableRef::Function(Rc::clone(owner_executable)), &base, scope, None)
    }
}
