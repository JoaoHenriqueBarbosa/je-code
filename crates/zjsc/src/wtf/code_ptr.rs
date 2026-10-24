//! Tradução de `wtf/CodePtr.h`, na configuração sem ptrauth e sem `CPU(ARM_THUMB2)`.
//!
//! DIVERGÊNCIAS: o `CodePtr` do C++ guarda o endereço de código de máquina (`void*`). O
//! interpretador Rust não gera código de máquina; o valor guardado é o identificador do entrypoint
//! que o `LLIntJITCode` entrega, um `usize` opaco. Os construtores a partir de ponteiro de função
//! (`encodeFunc`), `retagged`, `untaggedPtr` com assinatura e as variantes de convenção de chamada
//! (`FunctionAttributes`) não existem sem código nativo. Sem ptrauth, `taggedPtr()` e
//! `untaggedPtr()` são o mesmo valor.

use std::fmt;
use std::marker::PhantomData;

use crate::wtf::ptr_tag::PtrTagType;

/// `template<PtrTag tag> class CodePtr`. Nulo (`CodePtr()`, `m_value == nullptr`) é o `0`.
pub struct CodePtr<Tag: PtrTagType> {
    value: usize,
    _tag: PhantomData<Tag>,
}

impl<Tag: PtrTagType> CodePtr<Tag> {
    /// `CodePtr()` / `CodePtr(nullptr)`.
    pub fn null() -> CodePtr<Tag> {
        CodePtr { value: 0, _tag: PhantomData }
    }

    /// `explicit CodePtr(void* value)`: `ASSERT(value)`.
    pub fn new(value: usize) -> CodePtr<Tag> {
        debug_assert!(value != 0);
        CodePtr { value, _tag: PhantomData }
    }

    /// `taggedPtr()`.
    pub fn tagged_ptr(&self) -> usize {
        self.value
    }

    /// `untaggedPtr()`.
    pub fn untagged_ptr(&self) -> usize {
        self.value
    }

    /// `operator bool()`.
    pub fn is_non_null(&self) -> bool {
        self.value != 0
    }
}

impl<Tag: PtrTagType> Clone for CodePtr<Tag> {
    fn clone(&self) -> CodePtr<Tag> {
        *self
    }
}

impl<Tag: PtrTagType> Copy for CodePtr<Tag> {}

impl<Tag: PtrTagType> Default for CodePtr<Tag> {
    fn default() -> CodePtr<Tag> {
        CodePtr::null()
    }
}

impl<Tag: PtrTagType> PartialEq for CodePtr<Tag> {
    fn eq(&self, other: &CodePtr<Tag>) -> bool {
        self.value == other.value
    }
}

impl<Tag: PtrTagType> Eq for CodePtr<Tag> {}

impl<Tag: PtrTagType> std::hash::Hash for CodePtr<Tag> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

impl<Tag: PtrTagType> fmt::Debug for CodePtr<Tag> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CodePtr({:#x})", self.value)
    }
}
