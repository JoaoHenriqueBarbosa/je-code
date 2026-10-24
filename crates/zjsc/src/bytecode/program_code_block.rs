//! Porte de `bytecode/ProgramCodeBlock.h` e `ProgramCodeBlock.cpp`.
//!
//! DIVERGÊNCIA: `ProgramCodeBlock` tem o mesmo layout de `CodeBlock`
//! (`static_assert(sizeof(ProgramCodeBlock) == sizeof(CodeBlock))`) e só difere no `Structure` da
//! célula, que o porte não tem; o bloco criado é o `CodeBlockRef` comum. `s_info`, `subspaceFor` e
//! `createStructure` não existem.

use std::cell::RefCell;
use std::rc::Rc;

use crate::bytecode::code_block::{CodeBlock, CodeBlockRef};
use crate::bytecode::unlinked_code_block::UnlinkedProgramCodeBlock;
use crate::runtime::js_scope::JSScopeRef;
use crate::runtime::program_executable::ProgramExecutable;
use crate::runtime::script_executable::ScriptExecutableRef;
use crate::runtime::vm::VM;

/// `class ProgramCodeBlock final : public GlobalCodeBlock`.
pub struct ProgramCodeBlock;

impl ProgramCodeBlock {
    /// `create(VM&, ProgramExecutable* ownerExecutable, UnlinkedProgramCodeBlock*, JSScope*)`:
    /// `None` é o `nullptr` quando o `finishCreation` falha.
    pub fn create(
        vm: &VM,
        owner_executable: &Rc<RefCell<ProgramExecutable>>,
        unlinked_code_block: &Rc<RefCell<UnlinkedProgramCodeBlock>>,
        scope: &JSScopeRef,
    ) -> Option<CodeBlockRef> {
        let base = unlinked_code_block.borrow().base_ref();
        CodeBlock::create(vm, &ScriptExecutableRef::Program(Rc::clone(owner_executable)), &base, scope, None)
    }
}
