//! Porte de `bytecode/EvalCodeBlock.h`, `EvalCodeBlock.cpp` e `GlobalCodeBlock.h`.
//!
//! DIVERGÊNCIA: `EvalCodeBlock` e `GlobalCodeBlock` têm o mesmo layout de `CodeBlock`
//! (`static_assert(sizeof(EvalCodeBlock) == sizeof(CodeBlock))`) e só diferem no `Structure` da
//! célula, que o porte não tem; o bloco criado é o `CodeBlockRef` comum. `s_info`, `subspaceFor` e
//! `createStructure` não existem. `unlinkedEvalCodeBlock()` é o `UnlinkedEvalCodeBlock` guardado no
//! `EvalExecutable` (o `uncheckedDowncast` do C++).

use std::cell::RefCell;
use std::rc::Rc;

use crate::bytecode::code_block::{CodeBlock, CodeBlockRef};
use crate::bytecode::unlinked_code_block::UnlinkedEvalCodeBlock;
use crate::runtime::eval_executable::EvalExecutable;
use crate::runtime::js_scope::JSScopeRef;
use crate::runtime::script_executable::ScriptExecutableRef;
use crate::runtime::vm::VM;

/// `class EvalCodeBlock final : public GlobalCodeBlock`.
pub struct EvalCodeBlock;

impl EvalCodeBlock {
    /// `create(VM&, EvalExecutable* ownerExecutable, UnlinkedEvalCodeBlock*, JSScope*)`: `None` é o
    /// `nullptr` quando o `finishCreation` falha.
    pub fn create(
        vm: &VM,
        owner_executable: &Rc<RefCell<EvalExecutable>>,
        unlinked_code_block: &Rc<RefCell<UnlinkedEvalCodeBlock>>,
        scope: &JSScopeRef,
    ) -> Option<CodeBlockRef> {
        let base = unlinked_code_block.borrow().base_ref();
        CodeBlock::create(vm, &ScriptExecutableRef::Eval(Rc::clone(owner_executable)), &base, scope, None)
    }
}
