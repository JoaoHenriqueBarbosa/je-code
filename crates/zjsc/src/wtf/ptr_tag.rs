//! Tradução de `wtf/PtrTag.h`, na configuração sem `CPU(ARM64E)`.
//!
//! DIVERGÊNCIA: sem ptrauth o `PtrTag` não assina nada, é só um nome de tipo. O `enum PtrTag` do
//! C++ (parâmetro de template não tipo) vira um tipo marcador por tag, com o trait `PtrTagType`
//! que carrega o valor, para `CodePtr<JSEntryPtrTag>` continuar escrevendo a tag no tipo.

/// `enum PtrTag`: os valores que o interpretador usa. As demais tags existem só para o JIT.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PtrTag {
    NoPtrTag,
    JSEntryPtrTag,
    JSEntrySlowPathPtrTag,
}

/// A tag como parâmetro de tipo (o `template<PtrTag tag>` do C++).
pub trait PtrTagType: Copy + std::fmt::Debug + PartialEq + Eq + std::hash::Hash {
    const TAG: PtrTag;
}

macro_rules! ptr_tag_marker {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub struct $name;

        impl PtrTagType for $name {
            const TAG: PtrTag = PtrTag::$name;
        }
    };
}

ptr_tag_marker!(NoPtrTag);
ptr_tag_marker!(JSEntryPtrTag);
ptr_tag_marker!(JSEntrySlowPathPtrTag);
