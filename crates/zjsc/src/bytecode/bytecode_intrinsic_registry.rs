//! Tradução de `JavaScriptCore/bytecode/BytecodeIntrinsicRegistry.{h,cpp}`.
//!
//! As listas de nomes (`JSC_COMMON_BYTECODE_INTRINSIC_*_EACH_NAME`, `JSC_FOREACH_LINK_TIME_CONSTANTS`)
//! vêm de `bytecode_intrinsics_table.rs`, gerado por `scripts/gen-bytecode-intrinsics.py`.
//!
//! Fora desta fatia, e por quê:
//!
//! - o `EmitterType` (ponteiro para `BytecodeIntrinsicNode::emit_intrinsic_<name>`) vira o enum
//!   `BytecodeIntrinsicEmitter`; os corpos dos emitters são do bytecompiler;
//! - os `m_<name>` (`Strong<Unknown>`) e os `<name>Value(BytecodeGenerator&)`, inclusive
//!   `orderedHashTableSentinelValue`: dependem de `JSValue`, `Strong` e `BytecodeGenerator`;
//! - a tabela `m_bytecodeIntrinsicMap` é indexada pelo `UniquedKey` do nome privado, e o nome privado
//!   de cada intrínseco (`vm.propertyNames->builtinNames().<name>PrivateName()`) chega por
//!   um resolvedor, porque `BuiltinNames` ainda não foi portado.

use std::collections::HashMap;

use crate::bytecode::bytecode_intrinsics_table::{
    BytecodeIntrinsicEmitter, LinkTimeConstant, EMITTER_TABLE, LINK_TIME_CONSTANT_TABLE,
};
use crate::runtime::identifier::Identifier;
use crate::wtf::text::string_impl::UniquedKey;

/// `BytecodeIntrinsicRegistry::Type`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Type {
    Emitter = 0,
    LinkTimeConstant = 1,
}

/// `BytecodeIntrinsicRegistry::Entry`: a união do C++ vira um enum; `Entry()` (emitter nulo) é
/// `Entry::default()`, o `Emitter(None)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Emitter(Option<BytecodeIntrinsicEmitter>),
    LinkTimeConstant(LinkTimeConstant),
}

impl Default for Entry {
    fn default() -> Entry {
        Entry::Emitter(None)
    }
}

impl From<BytecodeIntrinsicEmitter> for Entry {
    fn from(emitter: BytecodeIntrinsicEmitter) -> Entry {
        Entry::Emitter(Some(emitter))
    }
}

impl From<LinkTimeConstant> for Entry {
    fn from(link_time_constant: LinkTimeConstant) -> Entry {
        Entry::LinkTimeConstant(link_time_constant)
    }
}

impl Entry {
    /// `type()`.
    pub fn type_(&self) -> Type {
        match self {
            Entry::Emitter(_) => Type::Emitter,
            Entry::LinkTimeConstant(_) => Type::LinkTimeConstant,
        }
    }

    /// `linkTimeConstant()`. No C++ a leitura de uma entrada `Emitter` pela união é indefinida;
    /// aqui é um erro de uso.
    pub fn link_time_constant(&self) -> LinkTimeConstant {
        match self {
            Entry::LinkTimeConstant(constant) => *constant,
            Entry::Emitter(_) => unreachable!("Entry::link_time_constant em entrada Emitter"),
        }
    }

    /// `emitter()`: o emitter, ou `None` para o `Entry()` nulo.
    pub fn emitter(&self) -> Option<BytecodeIntrinsicEmitter> {
        match self {
            Entry::Emitter(emitter) => *emitter,
            Entry::LinkTimeConstant(_) => unreachable!("Entry::emitter em entrada LinkTimeConstant"),
        }
    }
}

/// `class BytecodeIntrinsicRegistry`.
#[derive(Debug)]
pub struct BytecodeIntrinsicRegistry {
    m_bytecode_intrinsic_map: HashMap<UniquedKey, Entry>,
}

impl BytecodeIntrinsicRegistry {
    /// `BytecodeIntrinsicRegistry(VM&)`: `private_name` devolve o `<name>PrivateName().impl()` do
    /// `BuiltinNames` para um nome base; nome sem privado correspondente fica fora do mapa (no C++
    /// o `BuiltinNames` tem todos).
    ///
    /// A ordem é a do C++: funções e constantes (`emit_intrinsic_*`) e depois os link-time constants.
    /// `HashMap::add` do C++ não sobrescreve chave existente, então a primeira inserção vence.
    pub fn new(private_name: impl Fn(&str) -> Option<UniquedKey>) -> BytecodeIntrinsicRegistry {
        let mut map = HashMap::new();
        let emitters = EMITTER_TABLE.iter().map(|(name, emitter)| (*name, Entry::from(*emitter)));
        let constants = LINK_TIME_CONSTANT_TABLE.iter().map(|(name, constant)| (*name, Entry::from(*constant)));
        for (name, entry) in emitters.chain(constants) {
            if let Some(key) = private_name(name) {
                map.entry(key).or_insert(entry);
            }
        }
        BytecodeIntrinsicRegistry { m_bytecode_intrinsic_map: map }
    }

    /// `lookup(const Identifier&)`.
    pub fn lookup(&self, ident: &Identifier) -> Option<Entry> {
        if !ident.is_private_name() {
            return None;
        }
        self.m_bytecode_intrinsic_map.get(&ident.impl_()?).copied()
    }
}
