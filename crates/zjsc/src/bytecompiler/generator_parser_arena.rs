//! `BytecodeGenerator::parserArena()` (`BytecodeGenerator.h:378`):
//! `ParserArena& parserArena() const { return m_scopeNode->parserArena(); }`.
//!
//! DIVERGÊNCIA de empréstimo: o `ParserArena` mora dentro do `ScopeNode` (`ParserArenaRoot`), e o
//! `ScopeNode` está emprestado (`Ref`) enquanto o `emitBytecode` dele roda, então devolver
//! `&mut ParserArena` quebraria o `RefCell`. O `ParserArena` do porte só carrega o
//! `IdentifierArena` (compartilhado por `Rc`), e é isso que a visão devolve: quem chama faz
//! `generator.parser_arena().identifier_arena().borrow_mut().make_identifier(...)`, com o mesmo
//! efeito do C++ (a mesma arena, os mesmos identificadores internados nela).

use std::cell::RefCell;
use std::rc::Rc;

use crate::bytecompiler::bytecode_generator::BytecodeGenerator;
use crate::parser::parser_arena::IdentifierArena;

/// Visão de `ParserArena&` sobre a arena do `ScopeNode` do gerador.
pub struct ParserArenaView {
    identifier_arena: Rc<RefCell<IdentifierArena>>,
}

impl ParserArenaView {
    /// `ParserArena::identifierArena()`.
    pub fn identifier_arena(&self) -> Rc<RefCell<IdentifierArena>> {
        Rc::clone(&self.identifier_arena)
    }
}

impl BytecodeGenerator {
    /// `parserArena()`.
    pub fn parser_arena(&self) -> ParserArenaView {
        ParserArenaView { identifier_arena: self.scope_node.borrow().arena_root.arena.shared_identifier_arena() }
    }
}
