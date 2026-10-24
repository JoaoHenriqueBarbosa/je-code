//! Tradução de `runtime/StructureTransitionTable.h`: `TransitionKind`, as funções livres que o
//! `Structure` consulta e a `StructureTransitionTable`.
//!
//! DIVERGÊNCIA: o C++ guarda `m_data` com um slot único ou um `HashMap` de `Hash::Key`, que empacota o
//! ponteiro, os atributos e o `TransitionKind` num `uintptr_t`, e as entradas são `Weak` (`WeakGCMap`
//! de transições, para o GC poder coletar estruturas sem uso). Aqui é um `HashMap` com a chave em
//! tupla e valores `Weak`, como o `WeakGCMap`: o filho guarda o pai em `previous` (forte) e o pai não
//! segura o filho, então não há ciclo `Rc` e a árvore morre com os objetos que a usam (sem isto, a cadeia
//! de um objeto que virou dicionário, como o global, ficava órfã). `TransitionPropertyAttributes` é `uint8_t`.

use std::collections::HashMap;
use std::rc::{Rc, Weak};

use crate::runtime::indexing_type::{
    has_array_storage, has_contiguous, has_double, has_indexed_properties, has_int32, has_undecided, IndexingType,
    ARRAY_STORAGE_SHAPE, CONTIGUOUS_SHAPE, COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS, COPY_ON_WRITE_ARRAY_WITH_DOUBLE,
    COPY_ON_WRITE_ARRAY_WITH_INT32, DOUBLE_SHAPE, INDEXING_SHAPE_AND_WRITABILITY_MASK, INT32_SHAPE,
    MAY_HAVE_INDEXED_ACCESSORS, SLOW_PUT_ARRAY_STORAGE_SHAPE, UNDECIDED_SHAPE,
};
use crate::runtime::options::Options;
use crate::runtime::structure::{Structure, StructureRef};
use crate::wtf::text::string_impl::UniquedKey;

/// `enum class TransitionKind : uint8_t`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransitionKind {
    Unknown = 0,
    PropertyAddition = 1,
    PropertyDeletion = 2,
    PropertyAttributeChange = 3,

    // Support for transitions not related to properties.
    // If any of these are used, the string portion of the key should be 0.
    AllocateUndecided = 4,
    AllocateInt32 = 5,
    AllocateDouble = 6,
    AllocateContiguous = 7,
    AllocateArrayStorage = 8,
    AllocateSlowPutArrayStorage = 9,
    SwitchToSlowPutArrayStorage = 10,
    AddIndexedAccessors = 11,
    PreventExtensions = 12,
    Seal = 13,
    Freeze = 14,
    BecomePrototype = 15,
    ChangePrototype = 16,

    // Support for transitions related with private brand
    SetBrand = 17,
}

/// `FirstNonPropertyTransitionKind`.
pub const FIRST_NON_PROPERTY_TRANSITION_KIND: TransitionKind = TransitionKind::AllocateUndecided;

/// `changesIndexingType`.
pub fn changes_indexing_type(transition: TransitionKind) -> bool {
    matches!(
        transition,
        TransitionKind::AllocateUndecided
            | TransitionKind::AllocateInt32
            | TransitionKind::AllocateDouble
            | TransitionKind::AllocateContiguous
            | TransitionKind::AllocateArrayStorage
            | TransitionKind::AllocateSlowPutArrayStorage
            | TransitionKind::SwitchToSlowPutArrayStorage
            | TransitionKind::AddIndexedAccessors
    )
}

/// `newIndexingType`.
pub fn new_indexing_type(old_type: IndexingType, transition: TransitionKind) -> IndexingType {
    match transition {
        TransitionKind::AllocateUndecided => {
            debug_assert!(!has_indexed_properties(old_type));
            old_type | UNDECIDED_SHAPE
        }
        TransitionKind::AllocateInt32 => {
            debug_assert!(
                !has_indexed_properties(old_type) || has_undecided(old_type) || old_type == COPY_ON_WRITE_ARRAY_WITH_INT32
            );
            (old_type & !INDEXING_SHAPE_AND_WRITABILITY_MASK) | INT32_SHAPE
        }
        TransitionKind::AllocateDouble => {
            debug_assert!(Options::with(|options| options.allow_double_shape));
            debug_assert!(
                !has_indexed_properties(old_type)
                    || has_undecided(old_type)
                    || has_int32(old_type)
                    || old_type == COPY_ON_WRITE_ARRAY_WITH_DOUBLE
            );
            (old_type & !INDEXING_SHAPE_AND_WRITABILITY_MASK) | DOUBLE_SHAPE
        }
        TransitionKind::AllocateContiguous => {
            debug_assert!(
                !has_indexed_properties(old_type)
                    || has_undecided(old_type)
                    || has_int32(old_type)
                    || has_double(old_type)
                    || old_type == COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS
            );
            (old_type & !INDEXING_SHAPE_AND_WRITABILITY_MASK) | CONTIGUOUS_SHAPE
        }
        TransitionKind::AllocateArrayStorage => {
            debug_assert!(
                !has_indexed_properties(old_type)
                    || has_undecided(old_type)
                    || has_int32(old_type)
                    || has_double(old_type)
                    || has_contiguous(old_type)
            );
            (old_type & !INDEXING_SHAPE_AND_WRITABILITY_MASK) | ARRAY_STORAGE_SHAPE
        }
        TransitionKind::AllocateSlowPutArrayStorage => {
            debug_assert!(
                !has_indexed_properties(old_type)
                    || has_undecided(old_type)
                    || has_int32(old_type)
                    || has_double(old_type)
                    || has_contiguous(old_type)
            );
            (old_type & !INDEXING_SHAPE_AND_WRITABILITY_MASK) | SLOW_PUT_ARRAY_STORAGE_SHAPE
        }
        TransitionKind::SwitchToSlowPutArrayStorage => {
            debug_assert!(has_array_storage(old_type));
            (old_type & !INDEXING_SHAPE_AND_WRITABILITY_MASK) | SLOW_PUT_ARRAY_STORAGE_SHAPE
        }
        TransitionKind::AddIndexedAccessors => old_type | MAY_HAVE_INDEXED_ACCESSORS,
        _ => old_type,
    }
}

/// `preventsExtensions`.
pub fn prevents_extensions(transition: TransitionKind) -> bool {
    matches!(transition, TransitionKind::PreventExtensions | TransitionKind::Seal | TransitionKind::Freeze)
}

/// `setsDontDeleteOnAllProperties`.
pub fn sets_dont_delete_on_all_properties(transition: TransitionKind) -> bool {
    matches!(transition, TransitionKind::Seal | TransitionKind::Freeze)
}

/// `setsReadOnlyOnNonAccessorProperties`.
pub fn sets_read_only_on_non_accessor_properties(transition: TransitionKind) -> bool {
    transition == TransitionKind::Freeze
}

/// `StructureTransitionTable::PointerKey`: o `UniquedStringImpl*` da propriedade, o `JSObject*` do
/// protótipo (`ChangePrototype`), ou nulo (as transições que não são de propriedade).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PointerKey {
    Null,
    Uid(UniquedKey),
    /// A identidade do `JSObject` (o `cell_id`).
    Object(usize),
}

/// `Hash::Key`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    pointer: PointerKey,
    attributes: u8,
    transition_kind: TransitionKind,
}

/// `class StructureTransitionTable`.
#[derive(Debug, Default)]
pub struct StructureTransitionTable {
    map: HashMap<Key, Weak<Structure>>,
}

impl StructureTransitionTable {
    /// `get(PointerKey, attributes, TransitionKind)`. A entrada é `Weak` como no `WeakGCMap` do C++: a
    /// estrutura morta (nenhum objeto a usa mais) é uma transição ausente.
    pub fn get(&self, rep: PointerKey, attributes: u32, transition_kind: TransitionKind) -> Option<StructureRef> {
        debug_assert!(attributes <= u8::MAX as u32);
        debug_assert!(transition_kind != TransitionKind::Unknown);
        self.map.get(&Key { pointer: rep, attributes: attributes as u8, transition_kind }).and_then(Weak::upgrade)
    }

    /// `add(vm, owner, structure)`: a chave vem da própria estrutura (`createKeyFromStructure`), que o
    /// chamador calcula e passa aqui.
    pub fn add(&mut self, rep: PointerKey, attributes: u32, transition_kind: TransitionKind, structure: StructureRef) {
        debug_assert!(attributes <= u8::MAX as u32);
        debug_assert!(transition_kind != TransitionKind::Unknown);
        self.map.insert(Key { pointer: rep, attributes: attributes as u8, transition_kind }, Rc::downgrade(&structure));
    }

    /// Solta as transições e devolve as que ainda estão vivas ao chamador, que as larga fora de qualquer
    /// `borrow` (o `Drop` em cascata de uma estrutura mexe na tabela do pai).
    pub fn take_all(&mut self) -> Vec<StructureRef> {
        std::mem::take(&mut self.map).into_values().filter_map(|weak| weak.upgrade()).collect()
    }
}
