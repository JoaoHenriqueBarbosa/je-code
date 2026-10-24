//! Tradução de `runtime/GlobalExecutable.h` e `GlobalExecutable.cpp`.
//!
//! DIVERGÊNCIAS: o C++ guarda `WriteBarrier<UnlinkedCodeBlock> m_unlinkedCodeBlock` e as classes
//! derivadas fazem `std::bit_cast` para o subtipo (`UnlinkedProgramCodeBlock*` etc.). No porte os
//! subtipos do `UnlinkedCodeBlock` são structs distintas, então `GlobalExecutable<U>` é genérica
//! sobre o subtipo guardado e o `bit_cast` some. O `CodeBlock` não precisa disso: `ProgramCodeBlock`,
//! `EvalCodeBlock` e `ModuleProgramCodeBlock` são o mesmo `CodeBlock` com `codeType()` diferente.
//! `visitChildren`, `visitOutputConstraints` e `reconcileWeakReferencesAtGCEnd` (coleta de lixo e
//! os conjuntos do heap) não existem; `DECLARE_INFO`/`s_info` também não.

use std::cell::RefCell;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

use crate::bytecode::code_block::CodeBlockRef;
use crate::parser::parser_modes::{CodeFeatures, LexicallyScopedFeatures};
use crate::runtime::script_executable::ScriptExecutable;
use crate::runtime::vm::VM;

/// `class GlobalExecutable`.
pub struct GlobalExecutable<U> {
    base: ScriptExecutable,
    code_block: Option<CodeBlockRef>,
    unlinked_code_block: Option<Rc<RefCell<U>>>,
    last_line: i32,
    end_column: u32,
}

impl<U> Deref for GlobalExecutable<U> {
    type Target = ScriptExecutable;

    fn deref(&self) -> &ScriptExecutable {
        &self.base
    }
}

impl<U> DerefMut for GlobalExecutable<U> {
    fn deref_mut(&mut self) -> &mut ScriptExecutable {
        &mut self.base
    }
}

impl<U> GlobalExecutable<U> {
    /// `GlobalExecutable(Structure*, VM&, const SourceCode&, ...)`: o chamador monta a base
    /// (`ScriptExecutable::new`) com os mesmos argumentos.
    pub fn new(base: ScriptExecutable) -> GlobalExecutable<U> {
        GlobalExecutable { base, code_block: None, unlinked_code_block: None, last_line: -1, end_column: u32::MAX }
    }

    /// `lastLine()`.
    pub fn last_line(&self) -> u32 {
        self.last_line as u32
    }

    /// `endColumn()`.
    pub fn end_column(&self) -> u32 {
        self.end_column
    }

    /// `recordParse(CodeFeatures, LexicallyScopedFeatures, bool, int lastLine, unsigned endColumn)`: o nome
    /// carrega o sufixo porque a base também tem um `recordParse` (de três argumentos).
    pub fn record_parse_global(
        &mut self,
        features: CodeFeatures,
        lexically_scoped_features: LexicallyScopedFeatures,
        has_captured_variables: bool,
        last_line: i32,
        end_column: u32,
    ) {
        self.base.record_parse(features, lexically_scoped_features, has_captured_variables);
        self.last_line = last_line;
        self.end_column = end_column;
        debug_assert!(end_column != u32::MAX);
    }

    /// `codeBlock()`.
    pub fn code_block(&self) -> Option<CodeBlockRef> {
        self.code_block.clone()
    }

    /// `unlinkedCodeBlock()`.
    pub fn unlinked_code_block(&self) -> Option<&Rc<RefCell<U>>> {
        self.unlinked_code_block.as_ref()
    }

    /// `m_unlinkedCodeBlock.set(vm, this, unlinkedCodeBlock)`.
    pub fn set_unlinked_code_block(&mut self, unlinked_code_block: Rc<RefCell<U>>) {
        self.unlinked_code_block = Some(unlinked_code_block);
    }

    /// `m_unlinkedCodeBlock.clear()`.
    pub fn clear_unlinked_code_block(&mut self) {
        self.unlinked_code_block = None;
    }

    /// `m_codeBlock.clear()`.
    pub fn clear_code_block(&mut self) {
        self.code_block = None;
    }

    /// `replaceCodeBlockWith(VM&, CodeBlock*)`.
    pub fn replace_code_block_with(&mut self, _vm: &VM, new_code_block: Option<CodeBlockRef>) -> Option<CodeBlockRef> {
        let old_code_block = self.code_block();
        self.code_block = new_code_block;
        old_code_block
    }
}
