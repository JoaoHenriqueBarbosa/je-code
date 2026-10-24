//! Tradução de `runtime/BrandedStructure.{h,cpp}`: a `Structure` com marca (`brand`) dos objetos que
//! receberam métodos privados de classe (`#m() {}`) por `op_set_private_brand`.
//!
//! DIVERGÊNCIAS (sem heap, sem GC, camada 3):
//!
//! - O `BrandedStructure` é subclasse de `Structure` no C++ (`StructureVariant::Branded`, alocada no
//!   `brandedStructureSpace`). O `Structure` do porte não é polimórfico: o `BrandedStructure` é o dado que
//!   o `Structure` guarda quando a variante é `Branded` (`Structure::branded()`), e a construção da
//!   estrutura (`Structure::set_brand_transition`) fica em `structure.rs`, onde os campos são visíveis. Por
//!   isso `check_brand` recebe o `Structure` em vez de ser método do próprio.
//! - `m_brand` é um `UniquedStringImpl*` (o `uid()` do `Symbol` privado), aqui o `UniquedKey`, e
//!   `m_parentBrand` (`WriteBarrierStructureID`) é um `StructureRef`. `visitAdditionalChildren` e o
//!   `subspaceFor` não existem (sem GC).

use crate::runtime::structure::{Structure, StructureRef};
use crate::wtf::text::string_impl::UniquedKey;

/// O que a `BrandedStructure` acrescenta à `Structure`: `m_brand` e `m_parentBrand`.
#[derive(Debug)]
pub struct BrandedStructure {
    /// `m_brand`.
    brand: UniquedKey,
    /// `m_parentBrand`: a estrutura anterior, se ela também é com marca (a cadeia que `checkBrand` percorre).
    parent_brand: Option<StructureRef>,
}

impl BrandedStructure {
    /// `BrandedStructure(VM&, Structure* previous, UniquedStringImpl* brand)`: a primeira marca de uma
    /// transição `SetBrand`; `m_parentBrand` é `previous` só quando `previous` já é com marca.
    pub(crate) fn new(previous: &StructureRef, brand: UniquedKey) -> BrandedStructure {
        BrandedStructure {
            brand,
            parent_brand: previous.is_branded_structure().then(|| StructureRef::clone(previous)),
        }
    }

    /// `BrandedStructure(VM&, BrandedStructure* previous)`: a estrutura de uma transição qualquer a partir
    /// de uma com marca herda `m_brand` e o `m_parentBrand` da anterior.
    pub(crate) fn copy_of(previous: &BrandedStructure) -> BrandedStructure {
        BrandedStructure { brand: previous.brand.clone(), parent_brand: previous.parent_brand.clone() }
    }

    /// `checkBrand(Symbol* brand)`: percorre esta estrutura e as marcas dos pais; `brand` é o `uid()` do
    /// símbolo.
    pub fn check_brand(structure: &Structure, brand: &UniquedKey) -> bool {
        let mut current = structure.branded();
        while let Some(branded) = current {
            if branded.brand == *brand {
                return true;
            }
            current = branded.parent_brand.as_deref().and_then(Structure::branded);
        }
        false
    }
}
