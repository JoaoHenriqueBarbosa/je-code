//! Porte de `bytecompiler/LabelScope.h`.
//!
//! O refcount intrusivo se mantém no objeto porque `hasOneRef()` é observado por
//! `breakTargetMayBeBound()`. `LabelScopeRef` faz o papel de `Ref<LabelScope>`/`RefPtr<LabelScope>`
//! (o `LabelScopePtr` do gerador): incrementa ao clonar, decrementa ao soltar.

use std::cell::Cell;
use std::rc::Rc;

use crate::bytecompiler::label::{LabelRef};
use crate::runtime::identifier::Identifier;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelScopeType {
    Loop,
    Switch,
    NamedLabel,
}

pub struct LabelScope {
    ref_count: Cell<i32>,
    type_: LabelScopeType,
    name: Option<Identifier>,
    scope_depth: i32,
    break_target: LabelRef,
    continue_target: Option<LabelRef>,
}

impl crate::wtf::ref_counted::RefCounted for LabelScope {
    fn ref_count(&self) -> i32 {
        self.ref_count.get()
    }
}

impl LabelScope {
    pub fn new(
        type_: LabelScopeType,
        name: Option<Identifier>,
        scope_depth: i32,
        break_target: LabelRef,
        continue_target: Option<LabelRef>,
    ) -> Self {
        LabelScope { ref_count: Cell::new(0), type_, name, scope_depth, break_target, continue_target }
    }

    pub fn break_target(&self) -> &LabelRef {
        &self.break_target
    }

    pub fn continue_target(&self) -> Option<&LabelRef> {
        self.continue_target.as_ref()
    }

    pub fn type_(&self) -> LabelScopeType {
        self.type_
    }

    pub fn name(&self) -> Option<&Identifier> {
        self.name.as_ref()
    }

    pub fn scope_depth(&self) -> i32 {
        self.scope_depth
    }

    pub fn ref_(&self) {
        self.ref_count.set(self.ref_count.get() + 1);
    }

    pub fn deref(&self) {
        self.ref_count.set(self.ref_count.get() - 1);
        debug_assert!(self.ref_count.get() >= 0);
    }

    pub fn has_one_ref(&self) -> bool {
        self.ref_count.get() == 1
    }

    pub fn break_target_may_be_bound(&self) -> bool {
        if !self.has_one_ref() {
            return true;
        }
        if !self.break_target.borrow().has_one_ref() {
            return true;
        }
        self.break_target.borrow().is_bound()
    }
}

/// `Ref<LabelScope>`/`RefPtr<LabelScope>`. O `LabelScope` não muda depois de criado (os membros são
/// `const` no C++), só a contagem, que é `Cell`; por isso o compartilhamento é `Rc<LabelScope>`
/// e o acesso aos alvos é direto, por `Deref`.
pub struct LabelScopeRef {
    scope: Rc<LabelScope>,
}

impl LabelScopeRef {
    pub fn new(scope: &Rc<LabelScope>) -> Self {
        scope.ref_();
        LabelScopeRef { scope: Rc::clone(scope) }
    }

    pub fn get(&self) -> &Rc<LabelScope> {
        &self.scope
    }
}

impl std::ops::Deref for LabelScopeRef {
    type Target = LabelScope;

    fn deref(&self) -> &LabelScope {
        &self.scope
    }
}

impl Clone for LabelScopeRef {
    fn clone(&self) -> Self {
        LabelScopeRef::new(&self.scope)
    }
}

impl Drop for LabelScopeRef {
    fn drop(&mut self) {
        self.scope.deref();
    }
}
