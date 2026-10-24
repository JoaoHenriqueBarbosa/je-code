//! Tradução de `JavaScriptCore/parser/VariableEnvironment.h`, `VariableEnvironmentInlines.h` e
//! `VariableEnvironment.cpp`.
//!
//! Chave: o `UniquedStringImpl*` do C++ é `UniquedKey` (identidade por ponteiro).
//!
//! ORDEM: `InlineMap` (até 9 entradas em ordem de inserção, depois tabela de hash) e as
//! `UncheckedKeyHashMap`/`UncheckedKeyHashSet` da WTF iteram em ordem de tabela de hash, que o
//! `BytecodeGenerator` torna observável (alocação dos registradores dos `let`). O porte usa
//! `wtf::key_hash_map::KeyHashMap`, reprodução fiel do `HashTable.h`/`InlineMap.h` com o hash real
//! do `StringImpl` (`IdentifierRepHash`); `OrderedKeyMap` é o nome antigo, agora o `HashTable`.
//!
//! O que vira nada: `WTF_MAKE_TZONE_ALLOCATED`, os `HashTraits` (`needsDestruction`, valores vazio
//! e removido de `CompactTDZEnvironmentKey`), `swap` e `operator=` do `VariableEnvironment`
//! (`std::mem::swap` e atribuição, com `Clone` no lugar do construtor de cópia) e os `friend` dos
//! `Cached*`.

use std::cell::{Ref, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use crate::runtime::identifier::Identifier;
use crate::wtf::text::string_impl::UniquedKey;

pub use crate::wtf::key_hash_map::AddResult;
use crate::wtf::key_hash_map::KeyHashMap;

/// `UncheckedKeyHashMap`/`UncheckedKeyHashSet` com `IdentifierRepHash` (ver `wtf::key_hash_map`).
pub type OrderedKeyMap<V> = KeyHashMap<V>;

/// `VariableEnvironment::Map`: `InlineMap` de capacidade inline 9.
type InlineVariableMap = KeyHashMap<VariableEnvironmentEntry, 9>;

// ---------------------------------------------------------------------------------------------
// VariableEnvironmentEntry
// ---------------------------------------------------------------------------------------------

/// `struct VariableEnvironmentEntry`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VariableEnvironmentEntry {
    bits: u16,
}

impl VariableEnvironmentEntry {
    // `enum Traits : uint16_t`.
    const IS_CAPTURED: u16 = 1 << 0;
    const IS_CONST: u16 = 1 << 1;
    const IS_VAR: u16 = 1 << 2;
    const IS_LET: u16 = 1 << 3;
    const IS_EXPORTED: u16 = 1 << 4;
    const IS_IMPORTED: u16 = 1 << 5;
    const IS_IMPORTED_NAMESPACE: u16 = 1 << 6;
    const IS_FUNCTION: u16 = 1 << 7;
    const IS_PARAMETER: u16 = 1 << 8;
    const IS_SLOPPY_MODE_HOISTED_FUNCTION: u16 = 1 << 9;
    const IS_PRIVATE_FIELD: u16 = 1 << 10;
    const IS_PRIVATE_METHOD: u16 = 1 << 11;
    const IS_PRIVATE_GETTER: u16 = 1 << 12;
    const IS_PRIVATE_SETTER: u16 = 1 << 13;
    const IS_FUNCTION_DECLARATION: u16 = 1 << 14;
    const IS_USING: u16 = 1 << 15;

    pub fn is_captured(&self) -> bool {
        self.bits & Self::IS_CAPTURED != 0
    }
    pub fn is_const(&self) -> bool {
        self.bits & Self::IS_CONST != 0
    }
    pub fn is_var(&self) -> bool {
        self.bits & Self::IS_VAR != 0
    }
    pub fn is_let(&self) -> bool {
        self.bits & Self::IS_LET != 0
    }
    pub fn is_exported(&self) -> bool {
        self.bits & Self::IS_EXPORTED != 0
    }
    pub fn is_imported(&self) -> bool {
        self.bits & Self::IS_IMPORTED != 0
    }
    pub fn is_imported_namespace(&self) -> bool {
        self.bits & Self::IS_IMPORTED_NAMESPACE != 0
    }
    pub fn is_function(&self) -> bool {
        self.bits & Self::IS_FUNCTION != 0
    }
    pub fn is_function_declaration(&self) -> bool {
        self.bits & Self::IS_FUNCTION_DECLARATION != 0
    }
    pub fn is_parameter(&self) -> bool {
        self.bits & Self::IS_PARAMETER != 0
    }
    pub fn is_sloppy_mode_hoisted_function(&self) -> bool {
        self.bits & Self::IS_SLOPPY_MODE_HOISTED_FUNCTION != 0
    }
    pub fn is_private_field(&self) -> bool {
        self.bits & Self::IS_PRIVATE_FIELD != 0
    }
    pub fn is_private_method(&self) -> bool {
        self.bits & Self::IS_PRIVATE_METHOD != 0
    }
    pub fn is_private_setter(&self) -> bool {
        self.bits & Self::IS_PRIVATE_SETTER != 0
    }
    pub fn is_private_getter(&self) -> bool {
        self.bits & Self::IS_PRIVATE_GETTER != 0
    }
    pub fn is_using(&self) -> bool {
        self.bits & Self::IS_USING != 0
    }

    pub fn set_is_captured(&mut self) {
        self.bits |= Self::IS_CAPTURED;
    }
    pub fn set_is_const(&mut self) {
        self.bits |= Self::IS_CONST;
    }
    pub fn set_is_var(&mut self) {
        self.bits |= Self::IS_VAR;
    }
    pub fn set_is_let(&mut self) {
        self.bits |= Self::IS_LET;
    }
    pub fn set_is_exported(&mut self) {
        self.bits |= Self::IS_EXPORTED;
    }
    pub fn set_is_imported(&mut self) {
        self.bits |= Self::IS_IMPORTED;
    }
    pub fn set_is_imported_namespace(&mut self) {
        self.bits |= Self::IS_IMPORTED_NAMESPACE;
    }
    pub fn set_is_function(&mut self) {
        self.bits |= Self::IS_FUNCTION;
    }
    pub fn set_is_function_declaration(&mut self) {
        self.bits |= Self::IS_FUNCTION_DECLARATION;
    }
    pub fn set_is_parameter(&mut self) {
        self.bits |= Self::IS_PARAMETER;
    }
    pub fn set_is_sloppy_mode_hoisted_function(&mut self) {
        self.bits |= Self::IS_SLOPPY_MODE_HOISTED_FUNCTION;
    }
    pub fn set_is_private_field(&mut self) {
        self.bits |= Self::IS_PRIVATE_FIELD;
    }
    pub fn set_is_private_method(&mut self) {
        self.bits |= Self::IS_PRIVATE_METHOD;
    }
    pub fn set_is_private_setter(&mut self) {
        self.bits |= Self::IS_PRIVATE_SETTER;
    }
    pub fn set_is_private_getter(&mut self) {
        self.bits |= Self::IS_PRIVATE_GETTER;
    }
    pub fn set_is_using(&mut self) {
        self.bits |= Self::IS_USING;
    }

    pub fn clear_is_var(&mut self) {
        self.bits &= !Self::IS_VAR;
    }

    pub fn bits(&self) -> u16 {
        self.bits
    }

    /// `dump`: `hex(m_bits)` (maiúsculas, sem preenchimento).
    pub fn dump(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        write!(out, "{:X}", self.bits)
    }
}

// ---------------------------------------------------------------------------------------------
// PrivateNameEntry
// ---------------------------------------------------------------------------------------------

/// `struct PrivateNameEntry`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrivateNameEntry {
    bits: u16,
}

impl PrivateNameEntry {
    pub const PRIVATE_CLASS_BRAND_OFFSET: u32 = 0;
    pub const PRIVATE_BRAND_OFFSET: u32 = 1;

    // `enum Traits : uint16_t`. O C++ combina valores com `|` e converte de volta com
    // `static_cast<Traits>`, então o porte os trata como `u16` soltos.
    pub const NONE: u16 = 0;
    pub const IS_METHOD: u16 = 1 << 0;
    pub const IS_GETTER: u16 = 1 << 1;
    pub const IS_SETTER: u16 = 1 << 2;
    pub const IS_STATIC: u16 = 1 << 3;

    /// `PrivateNameEntry(uint16_t traits = 0)`.
    pub fn new(traits: u16) -> PrivateNameEntry {
        PrivateNameEntry { bits: traits }
    }

    pub fn is_method(&self) -> bool {
        self.bits & Self::IS_METHOD != 0
    }
    pub fn is_setter(&self) -> bool {
        self.bits & Self::IS_SETTER != 0
    }
    pub fn is_getter(&self) -> bool {
        self.bits & Self::IS_GETTER != 0
    }
    pub fn is_field(&self) -> bool {
        !self.is_private_method_or_accessor()
    }
    pub fn is_static(&self) -> bool {
        self.bits & Self::IS_STATIC != 0
    }

    pub fn is_private_method_or_accessor(&self) -> bool {
        self.is_method() || self.is_setter() || self.is_getter()
    }

    pub fn bits(&self) -> u16 {
        self.bits
    }
}

/// `PrivateNameEnvironment` (ORDEM: mapa de hash no C++).
pub type PrivateNameEnvironment = OrderedKeyMap<PrivateNameEntry>;

// ---------------------------------------------------------------------------------------------
// VariableEnvironment
// ---------------------------------------------------------------------------------------------

/// `VariableEnvironment::PrivateDeclarationResult`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivateDeclarationResult {
    Success,
    DuplicatedName,
    InvalidStaticNonStatic,
}

/// `VariableEnvironment::RareData`.
#[derive(Clone, Debug, Default)]
pub struct RareData {
    pub private_names: PrivateNameEnvironment,
}

/// `class VariableEnvironment`. `Clone` é o construtor de cópia e o `operator=`.
#[derive(Clone, Debug, Default)]
pub struct VariableEnvironment {
    /// `m_map` (`InlineMap` de capacidade inline 9; ver ORDEM no topo).
    map: InlineVariableMap,
    is_everything_captured: bool,
    has_await_using_declaration: bool,
    rare_data: Option<Box<RareData>>,
}

impl VariableEnvironment {
    pub const INLINE_MAP_CAPACITY: u32 = 9;

    /// `begin()`/`end()`.
    pub fn iter(&self) -> impl Iterator<Item = &(UniquedKey, VariableEnvironmentEntry)> + '_ {
        self.map.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut (UniquedKey, VariableEnvironmentEntry)> + '_ {
        self.map.iter_mut()
    }

    /// `add(const RefPtr<UniquedStringImpl>&)`.
    pub fn add(&mut self, identifier: &UniquedKey) -> AddResult<'_, VariableEnvironmentEntry> {
        self.map.add(identifier, VariableEnvironmentEntry::default())
    }

    /// `add(const Identifier&)`.
    pub fn add_identifier(&mut self, identifier: &Identifier) -> AddResult<'_, VariableEnvironmentEntry> {
        self.add(&identifier.impl_().expect("Identifier::impl() nulo: Identifier.h:96 não tem asserção, o C++ desreferencia o StringImpl* nulo (UB)"))
    }

    /// `addPrivateName(const RefPtr<UniquedStringImpl>&)`.
    pub fn add_private_name(&mut self, identifier: &UniquedKey) -> AddResult<'_, PrivateNameEntry> {
        let rare_data = self.rare_data.get_or_insert_with(|| Box::new(RareData::default()));
        rare_data.private_names.add(identifier, PrivateNameEntry::default())
    }

    /// `addPrivateName(const Identifier&)`.
    pub fn add_private_name_identifier(&mut self, identifier: &Identifier) -> AddResult<'_, PrivateNameEntry> {
        self.add_private_name(&identifier.impl_().expect("Identifier::impl() nulo: Identifier.h:96 não tem asserção, o C++ desreferencia o StringImpl* nulo (UB)"))
    }

    pub fn size(&self) -> u32 {
        self.map.len() + self.private_names_size()
    }

    pub fn map_size(&self) -> u32 {
        self.map.len()
    }

    pub fn contains(&self, identifier: &UniquedKey) -> bool {
        self.map.contains(identifier)
    }

    pub fn remove(&mut self, identifier: &UniquedKey) -> bool {
        self.map.remove(identifier)
    }

    /// `find`: `None` é o `end()`.
    pub fn find(&self, identifier: &UniquedKey) -> Option<&VariableEnvironmentEntry> {
        self.map.find(identifier)
    }

    pub fn find_mut(&mut self, identifier: &UniquedKey) -> Option<&mut VariableEnvironmentEntry> {
        self.map.find_mut(identifier)
    }

    pub fn mark_variable_as_captured_if_defined(&mut self, identifier: &UniquedKey) {
        if let Some(entry) = self.map.find_mut(identifier) {
            entry.set_is_captured();
        }
    }

    pub fn mark_variable_as_captured(&mut self, identifier: &UniquedKey) {
        let entry = self.map.find_mut(identifier).expect("RELEASE_ASSERT: variável a capturar não está no ambiente");
        entry.set_is_captured();
    }

    pub fn mark_all_variables_as_captured(&mut self) {
        if self.is_everything_captured {
            return;
        }

        self.is_everything_captured = true; // Para consultas rápidas.
        // Toda entrada precisa ficar capturada para quando se itera `map` e se chama
        // `entry.is_captured()`.
        for value in self.map.values_mut() {
            value.set_is_captured();
        }
    }

    pub fn has_captured_variables(&self) -> bool {
        if self.is_everything_captured {
            return self.size() > 0;
        }
        for value in self.map.values() {
            if value.is_captured() {
                return true;
            }
        }
        false
    }

    pub fn captures(&self, identifier: &UniquedKey) -> bool {
        if self.is_everything_captured {
            return true;
        }

        match self.map.find(identifier) {
            None => false,
            Some(entry) => entry.is_captured(),
        }
    }

    pub fn mark_variable_as_imported(&mut self, identifier: &UniquedKey) {
        let entry = self.map.find_mut(identifier).expect("RELEASE_ASSERT: variável a importar não está no ambiente");
        entry.set_is_imported();
    }

    pub fn mark_variable_as_exported(&mut self, identifier: &UniquedKey) {
        // Invariante: o parser só marca após `hasDeclaredVariable`/`hasLexicallyDeclaredVariable`; do contrário já falhou com semanticFail.
        let entry = self.map.find_mut(identifier).expect("RELEASE_ASSERT: variável a exportar não está no ambiente");
        entry.set_is_exported();
    }

    pub fn is_everything_captured(&self) -> bool {
        self.is_everything_captured
    }

    pub fn has_using_declaration(&self) -> bool {
        self.map.values().any(|value| value.is_using())
    }

    pub fn using_declaration_count(&self) -> u32 {
        self.map.values().filter(|value| value.is_using()).count() as u32
    }

    pub fn has_await_using_declaration(&self) -> bool {
        self.has_await_using_declaration
    }

    pub fn set_has_await_using_declaration(&mut self) {
        self.has_await_using_declaration = true;
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty() && self.private_names_size() == 0
    }

    /// `declarePrivateField(const RefPtr<UniquedStringImpl>&)`.
    pub fn declare_private_field(&mut self, identifier: &UniquedKey) -> AddResult<'_, VariableEnvironmentEntry> {
        self.get_or_add_private_name(identifier);
        let mut entry = VariableEnvironmentEntry::default();
        entry.set_is_private_field();
        entry.set_is_const();
        entry.set_is_captured();
        self.map.add(identifier, entry)
    }

    /// `declarePrivateField(const Identifier&)`.
    pub fn declare_private_field_identifier(&mut self, identifier: &Identifier) -> AddResult<'_, VariableEnvironmentEntry> {
        self.declare_private_field(&identifier.impl_().expect("Identifier::impl() nulo: Identifier.h:96 não tem asserção, o C++ desreferencia o StringImpl* nulo (UB)"))
    }

    /// `declarePrivateMethod(const RefPtr<UniquedStringImpl>&, Traits)`. `additional_traits` são
    /// os `PrivateNameEntry::IS_*`.
    pub fn declare_private_method(&mut self, identifier: &UniquedKey, additional_traits: u16) -> bool {
        let rare_data = self.rare_data.get_or_insert_with(|| Box::new(RareData::default()));

        if !rare_data.private_names.contains(identifier) {
            let meta = PrivateNameEntry::new(PrivateNameEntry::IS_METHOD | additional_traits);

            let mut entry = VariableEnvironmentEntry::default();
            entry.set_is_private_method();
            entry.set_is_const();
            entry.set_is_captured();
            self.map.add(identifier, entry);

            let add_result = rare_data.private_names.add(identifier, meta);
            return add_result.is_new_entry;
        }

        false // Erro: declarando um nome privado duplicado.
    }

    /// `declarePrivateMethod(const Identifier&)`.
    pub fn declare_private_method_identifier(&mut self, identifier: &Identifier) -> bool {
        self.declare_private_method(&identifier.impl_().expect("Identifier::impl() nulo: Identifier.h:96 não tem asserção, o C++ desreferencia o StringImpl* nulo (UB)"), PrivateNameEntry::NONE)
    }

    /// `declareStaticPrivateMethod(const Identifier&)`.
    pub fn declare_static_private_method(&mut self, identifier: &Identifier) -> bool {
        self.declare_private_method(&identifier.impl_().expect("Identifier::impl() nulo: Identifier.h:96 não tem asserção, o C++ desreferencia o StringImpl* nulo (UB)"), PrivateNameEntry::IS_METHOD | PrivateNameEntry::IS_STATIC)
    }

    pub fn declare_private_accessor(&mut self, identifier: &UniquedKey, accessor_traits: PrivateNameEntry) -> PrivateDeclarationResult {
        let rare_data = self.rare_data.get_or_insert_with(|| Box::new(RareData::default()));

        let Some(current_entry) = rare_data.private_names.find(identifier).copied() else {
            let meta = PrivateNameEntry::new(accessor_traits.bits());

            let mut entry = VariableEnvironmentEntry::default();
            if accessor_traits.is_setter() {
                entry.set_is_private_setter();
            } else {
                debug_assert!(accessor_traits.is_getter());
                entry.set_is_private_getter();
            }
            entry.set_is_const();
            entry.set_is_captured();
            self.map.add(identifier, entry);

            rare_data.private_names.add(identifier, meta);
            return PrivateDeclarationResult::Success;
        };

        if (accessor_traits.is_setter() && !current_entry.is_getter()) || (accessor_traits.is_getter() && !current_entry.is_setter()) {
            return PrivateDeclarationResult::DuplicatedName;
        }

        if accessor_traits.is_static() != current_entry.is_static() {
            return PrivateDeclarationResult::InvalidStaticNonStatic;
        }

        let meta = PrivateNameEntry::new(current_entry.bits() | accessor_traits.bits());
        rare_data.private_names.set(identifier, meta);

        let entry = self.map.find_mut(identifier);
        debug_assert!(entry.is_some());
        if let Some(entry) = entry {
            if accessor_traits.is_setter() {
                entry.set_is_private_setter();
            } else {
                debug_assert!(accessor_traits.is_getter());
                entry.set_is_private_getter();
            }
        }

        PrivateDeclarationResult::Success
    }

    /// `declarePrivateSetter(const RefPtr<UniquedStringImpl>&, Traits)`.
    pub fn declare_private_setter(&mut self, identifier: &UniquedKey, modifier_traits: u16) -> PrivateDeclarationResult {
        self.declare_private_accessor(identifier, PrivateNameEntry::new(PrivateNameEntry::IS_SETTER | modifier_traits))
    }

    /// `declarePrivateSetter(const Identifier&)`.
    pub fn declare_private_setter_identifier(&mut self, identifier: &Identifier) -> PrivateDeclarationResult {
        self.declare_private_setter(&identifier.impl_().expect("Identifier::impl() nulo: Identifier.h:96 não tem asserção, o C++ desreferencia o StringImpl* nulo (UB)"), PrivateNameEntry::NONE)
    }

    /// `declareStaticPrivateSetter(const Identifier&)`.
    pub fn declare_static_private_setter(&mut self, identifier: &Identifier) -> PrivateDeclarationResult {
        self.declare_private_setter(&identifier.impl_().expect("Identifier::impl() nulo: Identifier.h:96 não tem asserção, o C++ desreferencia o StringImpl* nulo (UB)"), PrivateNameEntry::IS_STATIC)
    }

    /// `declarePrivateGetter(const RefPtr<UniquedStringImpl>&, Traits)`.
    pub fn declare_private_getter(&mut self, identifier: &UniquedKey, modifier_traits: u16) -> PrivateDeclarationResult {
        self.declare_private_accessor(identifier, PrivateNameEntry::new(PrivateNameEntry::IS_GETTER | modifier_traits))
    }

    /// `declarePrivateGetter(const Identifier&)`.
    pub fn declare_private_getter_identifier(&mut self, identifier: &Identifier) -> PrivateDeclarationResult {
        self.declare_private_getter(&identifier.impl_().expect("Identifier::impl() nulo: Identifier.h:96 não tem asserção, o C++ desreferencia o StringImpl* nulo (UB)"), PrivateNameEntry::NONE)
    }

    /// `declareStaticPrivateGetter(const Identifier&)`.
    pub fn declare_static_private_getter(&mut self, identifier: &Identifier) -> PrivateDeclarationResult {
        self.declare_private_getter(&identifier.impl_().expect("Identifier::impl() nulo: Identifier.h:96 não tem asserção, o C++ desreferencia o StringImpl* nulo (UB)"), PrivateNameEntry::IS_STATIC)
    }

    /// `privateNames()`. O C++ exige `privateNamesSize() > 0` (ASSERT); sem `RareData` o porte
    /// devolve um iterador vazio.
    // ORDEM: o C++ itera a tabela de hash.
    pub fn private_names(&self) -> impl Iterator<Item = &(UniquedKey, PrivateNameEntry)> + '_ {
        self.rare_data.iter().flat_map(|rare_data| rare_data.private_names.iter())
    }

    pub fn private_names_size(&self) -> u32 {
        match &self.rare_data {
            None => 0,
            Some(rare_data) => rare_data.private_names.len(),
        }
    }

    pub fn private_name_environment(&self) -> Option<&PrivateNameEnvironment> {
        self.rare_data.as_ref().map(|rare_data| &rare_data.private_names)
    }

    pub fn private_name_environment_mut(&mut self) -> Option<&mut PrivateNameEnvironment> {
        self.rare_data.as_mut().map(|rare_data| &mut rare_data.private_names)
    }

    pub fn has_static_private_method_or_accessor(&self) -> bool {
        if self.rare_data.is_none() {
            return false;
        }

        for (_, entry) in self.private_names() {
            if entry.is_private_method_or_accessor() && entry.is_static() {
                return true;
            }
        }

        false
    }

    pub fn has_instance_private_method_or_accessor(&self) -> bool {
        if self.rare_data.is_none() {
            return false;
        }

        for (_, entry) in self.private_names() {
            if entry.is_private_method_or_accessor() && !entry.is_static() {
                return true;
            }
        }

        false
    }

    pub fn has_private_name(&self, identifier: &Identifier) -> bool {
        match &self.rare_data {
            None => false,
            Some(rare_data) => rare_data.private_names.contains(&identifier.impl_().expect("Identifier::impl() nulo: Identifier.h:96 não tem asserção, o C++ desreferencia o StringImpl* nulo (UB)")),
        }
    }

    pub fn add_private_names_from(&mut self, private_name_environment: Option<&PrivateNameEnvironment>) {
        let Some(private_name_environment) = private_name_environment else {
            return;
        };

        let rare_data = self.rare_data.get_or_insert_with(|| Box::new(RareData::default()));

        // ORDEM: o C++ itera a tabela de hash de origem.
        for (key, value) in private_name_environment.iter() {
            rare_data.private_names.add(key, *value);
        }
    }

    pub fn dump(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        let mut did_print = false; // CommaPrinter(", ")
        // ORDEM: o C++ itera `m_map`.
        for (key, value) in self.map.iter() {
            if did_print {
                out.write_str(", ")?;
            }
            did_print = true;
            let string = &key.0;
            let characters: Box<dyn Iterator<Item = u16> + '_> = if string.is_8bit() {
                Box::new(string.span8().iter().map(|&character| character as u16))
            } else {
                Box::new(string.span16().iter().copied())
            };
            for decoded in char::decode_utf16(characters) {
                out.write_char(decoded.unwrap_or(char::REPLACEMENT_CHARACTER))?;
            }
            out.write_str(" => ")?;
            value.dump(out)?;
        }
        Ok(())
    }

    fn get_or_add_private_name(&mut self, identifier: &UniquedKey) -> &mut PrivateNameEntry {
        let rare_data = self.rare_data.get_or_insert_with(|| Box::new(RareData::default()));
        rare_data.private_names.add(identifier, PrivateNameEntry::default()).value
    }
}

// ---------------------------------------------------------------------------------------------
// TDZ
// ---------------------------------------------------------------------------------------------

/// `TDZEnvironment` (ORDEM: conjunto de hash no C++; só as chaves importam, o valor é `()`).
pub type TDZEnvironment = OrderedKeyMap<()>;

/// `CompactTDZEnvironment::Variables` (`Variant<Compact, Inflated>`).
#[derive(Debug)]
enum Variables {
    Compact(Vec<UniquedKey>),
    Inflated(TDZEnvironment),
}

/// `class CompactTDZEnvironment`. `m_variables` é `mutable` no C++ (a inflação é preguiçosa), daí
/// o `RefCell`.
#[derive(Debug)]
pub struct CompactTDZEnvironment {
    variables: RefCell<Variables>,
    hash: u32,
}

impl CompactTDZEnvironment {
    pub fn new(env: &TDZEnvironment) -> CompactTDZEnvironment {
        let mut hash = 0u32; // XOR é comutativo, a ordem não importa aqui.
        // ORDEM: o C++ itera a tabela de hash, mas o vetor é ordenado logo depois.
        let mut variables: Vec<UniquedKey> = env
            .iter()
            .map(|(key, _)| {
                hash ^= key.0.hash();
                key.clone()
            })
            .collect();

        Self::sort_compact(&mut variables);
        CompactTDZEnvironment { variables: RefCell::new(Variables::Compact(variables)), hash }
    }

    pub fn hash(&self) -> u32 {
        self.hash
    }

    /// `sortCompact`: ordena pelo endereço do `UniquedStringImpl`.
    pub fn sort_compact(compact: &mut [UniquedKey]) {
        compact.sort_unstable_by_key(|key| Rc::as_ptr(&key.0) as usize);
    }

    /// `toTDZEnvironment`.
    pub fn to_tdz_environment(&self) -> Ref<'_, TDZEnvironment> {
        if matches!(&*self.variables.borrow(), Variables::Inflated(_)) {
            return self.inflated();
        }
        self.to_tdz_environment_slow()
    }

    fn to_tdz_environment_slow(&self) -> Ref<'_, TDZEnvironment> {
        let mut inflated = TDZEnvironment::default();
        {
            let variables = self.variables.borrow();
            let Variables::Compact(compact) = &*variables else {
                unreachable!("to_tdz_environment_slow só roda com variáveis compactas");
            };
            for key in compact.iter() {
                let add_result = inflated.add(key, ());
                debug_assert!(add_result.is_new_entry);
            }
        }
        *self.variables.borrow_mut() = Variables::Inflated(inflated);
        self.inflated()
    }

    fn inflated(&self) -> Ref<'_, TDZEnvironment> {
        Ref::map(self.variables.borrow(), |variables| match variables {
            Variables::Inflated(inflated) => inflated,
            Variables::Compact(_) => unreachable!("o ambiente já foi inflado"),
        })
    }
}

fn compact_equals_inflated(compact: &[UniquedKey], inflated: &TDZEnvironment) -> bool {
    if compact.len() as u32 != inflated.len() {
        return false;
    }
    compact.iter().all(|ident| inflated.contains(ident))
}

impl PartialEq for CompactTDZEnvironment {
    fn eq(&self, other: &CompactTDZEnvironment) -> bool {
        if std::ptr::eq(self, other) {
            return true;
        }

        if self.hash != other.hash {
            return false;
        }

        let variables = self.variables.borrow();
        let other_variables = other.variables.borrow();
        match (&*variables, &*other_variables) {
            (Variables::Compact(compact), Variables::Compact(other_compact)) => compact == other_compact,
            (Variables::Compact(compact), Variables::Inflated(other_inflated)) => compact_equals_inflated(compact, other_inflated),
            (Variables::Inflated(inflated), Variables::Compact(other_compact)) => compact_equals_inflated(other_compact, inflated),
            (Variables::Inflated(inflated), Variables::Inflated(other_inflated)) => inflated.has_same_keys(other_inflated),
        }
    }
}

impl Eq for CompactTDZEnvironment {}

/// `struct CompactTDZEnvironmentKey`: o ponteiro do C++ é um `Rc`; `hash` e `equal` viram `Hash` e
/// `Eq`.
#[derive(Clone, Debug)]
pub struct CompactTDZEnvironmentKey {
    environment: Rc<CompactTDZEnvironment>,
}

impl CompactTDZEnvironmentKey {
    pub fn new(environment: Rc<CompactTDZEnvironment>) -> CompactTDZEnvironmentKey {
        CompactTDZEnvironmentKey { environment }
    }

    pub fn environment(&self) -> &Rc<CompactTDZEnvironment> {
        &self.environment
    }
}

impl PartialEq for CompactTDZEnvironmentKey {
    fn eq(&self, other: &CompactTDZEnvironmentKey) -> bool {
        *self.environment == *other.environment
    }
}

impl Eq for CompactTDZEnvironmentKey {}

impl Hash for CompactTDZEnvironmentKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u32(self.environment.hash());
    }
}

/// `class CompactTDZEnvironmentMap`: intern dos ambientes compactos, com contagem de uso por
/// entrada. Sempre dentro de um `Rc` (o `RefCounted` do C++).
#[derive(Debug, Default)]
pub struct CompactTDZEnvironmentMap {
    map: RefCell<HashMap<CompactTDZEnvironmentKey, u32>>,
}

impl CompactTDZEnvironmentMap {
    pub fn new() -> Rc<CompactTDZEnvironmentMap> {
        Rc::new(CompactTDZEnvironmentMap::default())
    }

    /// `get(const TDZEnvironment&)`: se o ambiente já existe, o novo é descartado (o `delete` do
    /// C++ é o `drop` do `Rc`).
    pub fn get(self: &Rc<Self>, env: &TDZEnvironment) -> CompactTDZEnvironmentMapHandle {
        let environment = Rc::new(CompactTDZEnvironment::new(env));
        let (handle, _is_new_entry) = self.get_environment(environment);
        handle
    }

    /// `get(CompactTDZEnvironment*, bool& isNewEntry)`: devolve também `isNewEntry`.
    pub fn get_environment(self: &Rc<Self>, environment: Rc<CompactTDZEnvironment>) -> (CompactTDZEnvironmentMapHandle, bool) {
        let key = CompactTDZEnvironmentKey::new(Rc::clone(&environment));
        let mut map = self.map.borrow_mut();
        let stored = map.get_key_value(&key).map(|(stored_key, _)| Rc::clone(stored_key.environment()));
        match stored {
            None => {
                map.insert(key, 1);
                (CompactTDZEnvironmentMapHandle::new(environment, Rc::clone(self)), true)
            }
            Some(stored_environment) => {
                if let Some(count) = map.get_mut(&key) {
                    *count += 1;
                }
                (CompactTDZEnvironmentMapHandle::new(stored_environment, Rc::clone(self)), false)
            }
        }
    }
}

/// `CompactTDZEnvironmentMap::Handle`: uma referência contada a um ambiente do mapa. O destrutor
/// (`Drop`) decrementa e remove a entrada ao chegar a zero; a cópia incrementa; o `move` e as
/// atribuições são nativos do Rust. O `Handle() = default` é `Default` (sem mapa).
#[derive(Debug, Default)]
pub struct CompactTDZEnvironmentMapHandle {
    inner: Option<(Rc<CompactTDZEnvironment>, Rc<CompactTDZEnvironmentMap>)>,
}

impl CompactTDZEnvironmentMapHandle {
    /// `Handle(CompactTDZEnvironment&, CompactTDZEnvironmentMap&)`: não incrementa; quem chama já
    /// contou.
    pub fn new(environment: Rc<CompactTDZEnvironment>, map: Rc<CompactTDZEnvironmentMap>) -> CompactTDZEnvironmentMapHandle {
        CompactTDZEnvironmentMapHandle { inner: Some((environment, map)) }
    }

    /// `explicit operator bool`.
    pub fn is_valid(&self) -> bool {
        self.inner.is_some()
    }

    pub fn environment(&self) -> &CompactTDZEnvironment {
        let (environment, _) = self.inner.as_ref().expect("Handle sem ambiente (o C++ desreferenciaria nulo)");
        environment
    }
}

impl Clone for CompactTDZEnvironmentMapHandle {
    fn clone(&self) -> CompactTDZEnvironmentMapHandle {
        if let Some((environment, map)) = &self.inner {
            let key = CompactTDZEnvironmentKey::new(Rc::clone(environment));
            let mut table = map.map.borrow_mut();
            let count = table.get_mut(&key).expect("RELEASE_ASSERT: ambiente do Handle ausente do mapa");
            *count += 1;
        }
        CompactTDZEnvironmentMapHandle { inner: self.inner.clone() }
    }
}

impl Drop for CompactTDZEnvironmentMapHandle {
    fn drop(&mut self) {
        // Sem mapa: foi movido para outro Handle (no Rust, `Default`).
        let Some((environment, map)) = self.inner.take() else {
            return;
        };

        let key = CompactTDZEnvironmentKey::new(environment);
        let mut table = map.map.borrow_mut();
        let remaining = {
            let count = table.get_mut(&key).expect("RELEASE_ASSERT: ambiente do Handle ausente do mapa");
            *count -= 1;
            *count
        };
        if remaining == 0 {
            table.remove(&key);
        }
    }
}

/// `class TDZEnvironmentLink`.
#[derive(Debug)]
pub struct TDZEnvironmentLink {
    handle: CompactTDZEnvironmentMapHandle,
    parent: Option<Rc<TDZEnvironmentLink>>,
}

impl TDZEnvironmentLink {
    pub fn create(handle: CompactTDZEnvironmentMapHandle, parent: Option<Rc<TDZEnvironmentLink>>) -> Rc<TDZEnvironmentLink> {
        Rc::new(TDZEnvironmentLink { handle, parent })
    }

    pub fn contains(&self, impl_: &UniquedKey) -> bool {
        self.handle.environment().to_tdz_environment().contains(impl_)
    }

    pub fn parent(&self) -> Option<&TDZEnvironmentLink> {
        self.parent.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wtf::text::string_impl::StringImpl;

    fn key(text: &[u8]) -> UniquedKey {
        UniquedKey(StringImpl::create(text))
    }

    #[test]
    fn entry_bits() {
        let mut entry = VariableEnvironmentEntry::default();
        entry.set_is_using();
        entry.set_is_var();
        entry.clear_is_var();
        assert_eq!(entry.bits(), 1 << 15);
        assert!(entry.is_using() && !entry.is_var());
        let mut text = String::new();
        entry.dump(&mut text).unwrap();
        assert_eq!(text, "8000");
    }

    #[test]
    fn capture_and_size() {
        let (a, b) = (key(b"a"), key(b"b"));
        let mut env = VariableEnvironment::default();
        assert!(env.add(&a).is_new_entry);
        assert!(!env.add(&a).is_new_entry);
        env.add(&b);
        assert!(!env.has_captured_variables());
        env.mark_variable_as_captured(&a);
        assert!(env.captures(&a) && !env.captures(&b));
        env.mark_all_variables_as_captured();
        assert!(env.captures(&b) && env.has_captured_variables());
        assert_eq!(env.size(), 2);
    }

    #[test]
    fn private_accessors() {
        let name = key(b"x");
        let mut env = VariableEnvironment::default();
        assert_eq!(env.declare_private_getter(&name, PrivateNameEntry::NONE), PrivateDeclarationResult::Success);
        assert_eq!(env.declare_private_getter(&name, PrivateNameEntry::NONE), PrivateDeclarationResult::DuplicatedName);
        assert_eq!(env.declare_private_setter(&name, PrivateNameEntry::IS_STATIC), PrivateDeclarationResult::InvalidStaticNonStatic);
        assert_eq!(env.declare_private_setter(&name, PrivateNameEntry::NONE), PrivateDeclarationResult::Success);
        assert!(env.find(&name).is_some_and(|entry| entry.is_private_getter() && entry.is_private_setter()));
        assert!(env.has_instance_private_method_or_accessor() && !env.has_static_private_method_or_accessor());
        assert!(!env.declare_private_method(&name, PrivateNameEntry::NONE));
    }

    #[test]
    fn tdz_map_counts_and_interns() {
        let (a, b) = (key(b"a"), key(b"b"));
        let mut tdz = TDZEnvironment::default();
        tdz.add(&a, ());
        tdz.add(&b, ());
        let map = CompactTDZEnvironmentMap::new();
        let first = map.get(&tdz);
        let second = map.get(&tdz);
        assert!(std::ptr::eq(first.environment(), second.environment()));
        assert_eq!(map.map.borrow().len(), 1);
        let copy = first.clone();
        drop(first);
        drop(second);
        assert_eq!(map.map.borrow().len(), 1);
        drop(copy);
        assert_eq!(map.map.borrow().len(), 0);
    }

    #[test]
    fn tdz_link_contains() {
        let (a, b) = (key(b"a"), key(b"b"));
        let mut tdz = TDZEnvironment::default();
        tdz.add(&a, ());
        let map = CompactTDZEnvironmentMap::new();
        let link = TDZEnvironmentLink::create(map.get(&tdz), None);
        assert!(link.contains(&a) && !link.contains(&b));
        assert!(link.parent().is_none());
    }
}
