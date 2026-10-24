//! Porte de `yarr/YarrMatchingContextHolder.h`, no ramo sem `ENABLE(YARR_JIT)`.
//!
//! O `MatchingContextHolder` guarda o limite de pilha da execução da expressão regular e, quando a
//! chamada vem da thread da VM, marca o `RegExp` em execução na VM até o fim do escopo. O
//! `m_freeList` e os `offsetOf*` só servem ao código gerado do JIT; ficam de fora.

use crate::runtime::vm::VM;
use crate::yarr::yarr::MatchFrom;

/// `class MatchingContextHolder`.
pub struct MatchingContextHolder<'vm> {
    vm: &'vm VM,
    stack_limit: usize,
    match_from: MatchFrom,
}

impl<'vm> MatchingContextHolder<'vm> {
    /// Quanto da pilha da thread atual a recursão pode consumir; o C++ pergunta ao
    /// `StackBounds::recursionLimit`, que o Rust seguro não alcança (mesmo critério do `StackCheck`
    /// do `yarr_pattern_cpp6`).
    const COMPILER_THREAD_STACK_BUDGET: usize = 512 * 1024;

    /// `MatchingContextHolder(VM&, RegExp*, MatchFrom)`. `reg_exp` é a identidade do `RegExp`
    /// (0 é `nullptr`).
    pub fn new(vm: &'vm VM, reg_exp: usize, match_from: MatchFrom) -> MatchingContextHolder<'vm> {
        let stack_limit = if match_from == MatchFrom::VMThread {
            vm.set_executing_reg_exp(reg_exp);
            vm.soft_stack_limit()
        } else {
            MatchingContextHolder::current_stack_pointer().saturating_sub(MatchingContextHolder::COMPILER_THREAD_STACK_BUDGET)
        };
        MatchingContextHolder { vm, stack_limit, match_from }
    }

    #[inline(never)]
    fn current_stack_pointer() -> usize {
        let marker = 0u8;
        std::ptr::addr_of!(marker) as usize
    }

    /// `stackLimit()`.
    pub fn stack_limit(&self) -> usize {
        self.stack_limit
    }
}

impl Drop for MatchingContextHolder<'_> {
    fn drop(&mut self) {
        if self.match_from == MatchFrom::VMThread {
            self.vm.set_executing_reg_exp(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vm_thread_marks_and_clears_executing_reg_exp() {
        let vm = VM::new();
        vm.set_soft_stack_limit(1234);
        {
            let holder = MatchingContextHolder::new(&vm, 7, MatchFrom::VMThread);
            assert_eq!(holder.stack_limit(), 1234);
            assert_eq!(vm.executing_reg_exp(), 7);
        }
        assert_eq!(vm.executing_reg_exp(), 0);
    }

    #[test]
    fn compiler_thread_leaves_vm_alone() {
        let vm = VM::new();
        let holder = MatchingContextHolder::new(&vm, 7, MatchFrom::CompilerThread);
        assert_eq!(vm.executing_reg_exp(), 0);
        assert!(holder.stack_limit() > 0);
    }
}
