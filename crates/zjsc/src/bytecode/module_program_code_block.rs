//! Porte de `bytecode/ModuleProgramCodeBlock.h` e `ModuleProgramCodeBlock.cpp`.
//!
//! DIVERGÊNCIA: `ModuleProgramCodeBlock` tem o mesmo layout de `CodeBlock`
//! (`static_assert(sizeof(ModuleProgramCodeBlock) == sizeof(CodeBlock))`) e só difere no
//! `Structure` da célula, que o porte não tem; o bloco criado é o `CodeBlockRef` comum. O
//! `dynamicDowncast<UnlinkedModuleProgramCodeBlock>` do `CodeBlock::finishCreation` vira o
//! parâmetro `module_environment_symbol_table_constant_register_offset` do `CodeBlock::create`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::bytecode::code_block::{CodeBlock, CodeBlockRef};
use crate::bytecode::unlinked_code_block::UnlinkedModuleProgramCodeBlock;
use crate::runtime::js_scope::JSScopeRef;
use crate::runtime::module_program_executable::ModuleProgramExecutable;
use crate::runtime::script_executable::ScriptExecutableRef;
use crate::runtime::vm::VM;

/// `class ModuleProgramCodeBlock final : public GlobalCodeBlock`.
pub struct ModuleProgramCodeBlock;

impl ModuleProgramCodeBlock {
    /// `create(VM&, ModuleProgramExecutable* ownerExecutable, UnlinkedModuleProgramCodeBlock*,
    /// JSScope*)`: `None` é o `nullptr` quando o `finishCreation` falha.
    pub fn create(
        vm: &VM,
        owner_executable: &Rc<RefCell<ModuleProgramExecutable>>,
        unlinked_code_block: &Rc<RefCell<UnlinkedModuleProgramCodeBlock>>,
        scope: &JSScopeRef,
    ) -> Option<CodeBlockRef> {
        let (base, offset) = {
            let unlinked = unlinked_code_block.borrow();
            (unlinked.base_ref(), unlinked.module_environment_symbol_table_constant_register_offset())
        };
        CodeBlock::create(vm, &ScriptExecutableRef::ModuleProgram(Rc::clone(owner_executable)), &base, scope, Some(offset))
    }
}
