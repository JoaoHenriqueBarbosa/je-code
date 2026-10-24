//! Porte de `runtime/SymbolTable.h` e `SymbolTable.cpp`: `SymbolTableEntry` (com `Fast`), o enum
//! `SymbolTable::ScopeType`, `PropagateCloneInvalidationToOriginal` e a classe `SymbolTable`.
//!
//! Divergências da classe `SymbolTable` (sem heap, sem JIT, uma thread só):
//! - Não é `JSCell`: `SymbolTableRef = Rc<RefCell<SymbolTable>>`. O `cell_id` vem do registro central
//!   (`runtime::cell_registry`), que mantém a tabela viva, como o heap.
//! - As travas (`ConcurrentJSLocker`) somem. As sobrecargas "sem trava" são `add`/`set`/
//!   `take_next_scope_offset`; as que recebem o locker são `*_locked` e recebem
//!   `NoLockingNecessaryTag`.
//! - `ScopedArgumentsTable` vira só os dados (`Vec<ScopeOffset>`), sem `WatchpointSet`.
//!   `prepareToWatchScopedArgument`, `m_singleton`, `notifyCreation`, `m_clonedFrom` e a propagação
//!   da invalidação (`Yes`) dependem de watchpoints/JSScope, que não existem sem JIT: só o flag é guardado.
//! - `m_rareData` guarda só os nomes privados (`m_uniqueIDMap`, `m_offsetToVariableMap`,
//!   `m_uniqueTypeSetMap` e a informação de depuração são do perfilador de tipos e do depurador).
//! - `m_map` é um `UncheckedKeyHashMap<RefPtr<UniquedStringImpl>, SymbolTableEntry, IdentifierRepHash>`: aqui é o
//!   `KeyHashMap` (`wtf/key_hash_map.rs`), que reproduz a tabela do WTF, então `iter` percorre na ordem de
//!   bucket do C++ (a que `JSSymbolTableObject::getOwnPropertyNames` e o gerador de bytecode observam).
//!
//! `SymbolTableEntry` só existe na forma fina (`SlimFlag`). A forma gorda (`FatEntry` com
//! `InlineWatchpointSet`) só nasce de `prepareToWatch`, que exige `isWatchable()`, e esta depende de
//! `Options::useJIT()`, que aqui é sempre falso (sem JIT). Logo `isFat()` é sempre falso,
//! `watchpointSet()` é sempre nulo, `inflate`/`freeFatEntry` não têm o que fazer.

use std::cell::RefCell;
use std::rc::Rc;

use crate::parser::variable_environment::PrivateNameEntry;
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::key_hash_map::KeyHashMap;
use crate::wtf::text::string_impl::UniquedKey;
use crate::runtime::constant_mode::{mode_for_is_constant, ConstantMode};
use crate::runtime::property_attribute::{DONT_ENUM, READ_ONLY};
use crate::runtime::scope_offset::ScopeOffset;
use crate::runtime::var_offset::{VarKind, VarOffset};

/// `NoLockingNecessaryTag` / `NoLockingNecessary` de `wtf/Locker.h`, o argumento que o gerador de
/// bytecode passa no lugar do `ConcurrentJSLocker` (o parser e o gerador são de uma thread só).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoLockingNecessaryTag;

pub const NO_LOCKING_NECESSARY: NoLockingNecessaryTag = NoLockingNecessaryTag;

/// `missingSymbolMarker()`.
pub const fn missing_symbol_marker() -> i32 {
    i32::MAX
}

/// `SymbolTable::ScopeType`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScopeType {
    VarScope,
    GlobalLexicalScope,
    LexicalScope,
    CatchScope,
    CatchScopeWithSimpleParameter,
    FunctionNameScope,
}

pub use self::ScopeType as SymbolTableScopeType;

/// `SymbolTable::PropagateCloneInvalidationToOriginal : bool`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PropagateCloneInvalidationToOriginal {
    No,
    Yes,
}

const SLIM_FLAG: isize = 0x1;
const READ_ONLY_FLAG: isize = 0x2;
const DONT_ENUM_FLAG: isize = 0x4;
const NOT_NULL_FLAG: isize = 0x8;
const KIND_BITS_MASK: isize = 0x30;
const SCOPE_KIND_BITS: isize = 0x00;
const STACK_KIND_BITS: isize = 0x20;
const DIRECT_ARGUMENT_KIND_BITS: isize = 0x30;
const FLAG_BITS: u32 = 6;

fn var_offset_from_bits(bits: isize) -> VarOffset {
    let kind_bits = bits & KIND_BITS_MASK;
    let kind = if kind_bits == SCOPE_KIND_BITS {
        VarKind::Scope
    } else if kind_bits == STACK_KIND_BITS {
        VarKind::Stack
    } else {
        VarKind::DirectArgument
    };
    VarOffset::assemble(kind, (bits >> FLAG_BITS) as i32 as u32)
}

fn scope_offset_from_bits(bits: isize) -> ScopeOffset {
    debug_assert!((bits & KIND_BITS_MASK) == SCOPE_KIND_BITS);
    ScopeOffset::new((bits >> FLAG_BITS) as i32 as u32)
}

/// `SymbolTableEntry::Fast`: leitura rápida dos bits, sem consultar a forma gorda.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fast {
    bits: isize,
}

pub type SymbolTableEntryFast = Fast;

impl Default for Fast {
    fn default() -> Self {
        Fast { bits: SLIM_FLAG }
    }
}

impl Fast {
    pub fn is_null(&self) -> bool {
        (self.bits & !SLIM_FLAG) == 0
    }

    pub fn var_offset(&self) -> VarOffset {
        var_offset_from_bits(self.bits)
    }

    /// Falha (assert) se o deslocamento não for de escopo.
    pub fn scope_offset(&self) -> ScopeOffset {
        scope_offset_from_bits(self.bits)
    }

    pub fn is_read_only(&self) -> bool {
        (self.bits & READ_ONLY_FLAG) != 0
    }

    pub fn is_dont_enum(&self) -> bool {
        (self.bits & DONT_ENUM_FLAG) != 0
    }

    pub fn get_attributes(&self) -> u32 {
        let mut attributes = 0;
        if self.is_read_only() {
            attributes |= READ_ONLY;
        }
        if self.is_dont_enum() {
            attributes |= DONT_ENUM;
        }
        attributes
    }

    pub fn is_fat(&self) -> bool {
        (self.bits & SLIM_FLAG) == 0
    }
}

impl From<&SymbolTableEntry> for Fast {
    fn from(entry: &SymbolTableEntry) -> Fast {
        Fast { bits: entry.bits }
    }
}

/// `SymbolTableEntry`. Só movível no C++ (cópia apagada); aqui não implementa `Clone`.
#[derive(Debug, PartialEq, Eq)]
pub struct SymbolTableEntry {
    bits: isize,
}

impl Default for SymbolTableEntry {
    fn default() -> Self {
        SymbolTableEntry { bits: SLIM_FLAG }
    }
}

impl SymbolTableEntry {
    /// `SymbolTableEntry(VarOffset offset, unsigned attributes)`.
    pub fn new(offset: VarOffset, attributes: u32) -> Self {
        let mut entry = SymbolTableEntry { bits: SLIM_FLAG };
        debug_assert!(Self::is_valid_var_offset(offset));
        entry.pack(offset, (attributes & READ_ONLY) != 0, (attributes & DONT_ENUM) != 0);
        entry
    }

    /// `SymbolTableEntry(VarOffset offset)`.
    pub fn from_var_offset(offset: VarOffset) -> Self {
        let mut entry = SymbolTableEntry { bits: SLIM_FLAG };
        debug_assert!(Self::is_valid_var_offset(offset));
        entry.pack(offset, false, false);
        entry
    }

    pub fn swap(&mut self, other: &mut SymbolTableEntry) {
        std::mem::swap(&mut self.bits, &mut other.bits);
    }

    pub fn is_null(&self) -> bool {
        (self.bits & !SLIM_FLAG) == 0
    }

    pub fn var_offset(&self) -> VarOffset {
        var_offset_from_bits(self.bits)
    }

    /// `isWatchable()`: `Options::useJIT()` é falso, então nunca é observável.
    pub fn is_watchable(&self) -> bool {
        false
    }

    /// Falha (assert) se o deslocamento não for de escopo.
    pub fn scope_offset(&self) -> ScopeOffset {
        scope_offset_from_bits(self.bits)
    }

    pub fn get_fast(&self) -> Fast {
        Fast::from(self)
    }

    pub fn get_attributes(&self) -> u32 {
        self.get_fast().get_attributes()
    }

    pub fn set_read_only(&mut self) {
        self.bits |= READ_ONLY_FLAG;
    }

    pub fn is_read_only(&self) -> bool {
        (self.bits & READ_ONLY_FLAG) != 0
    }

    pub fn constant_mode(&self) -> ConstantMode {
        mode_for_is_constant(self.is_read_only())
    }

    pub fn is_dont_enum(&self) -> bool {
        (self.bits & DONT_ENUM_FLAG) != 0
    }

    /// `prepareToWatch()`: só infla se `isWatchable()`, que aqui é falso.
    pub fn prepare_to_watch(&mut self) {
        debug_assert!(!self.is_fat());
    }

    fn is_fat(&self) -> bool {
        (self.bits & SLIM_FLAG) == 0
    }

    fn pack(&mut self, offset: VarOffset, read_only: bool, dont_enum: bool) {
        debug_assert!(!self.is_fat());
        let mut bits: isize = ((offset.raw_offset() as isize) << FLAG_BITS) | NOT_NULL_FLAG | SLIM_FLAG;
        if read_only {
            bits |= READ_ONLY_FLAG;
        }
        if dont_enum {
            bits |= DONT_ENUM_FLAG;
        }
        match offset.kind() {
            VarKind::Scope => bits |= SCOPE_KIND_BITS,
            VarKind::Stack => bits |= STACK_KIND_BITS,
            VarKind::DirectArgument => bits |= DIRECT_ARGUMENT_KIND_BITS,
            VarKind::Invalid => panic!("SymbolTableEntry::pack: VarOffset inválido"),
        }
        self.bits = bits;
    }

    fn is_valid_var_offset(offset: VarOffset) -> bool {
        (((offset.raw_offset() as isize) << FLAG_BITS) >> FLAG_BITS) == offset.raw_offset() as isize
    }
}

/// `const ClassInfo SymbolTable::s_info`.
pub static SYMBOL_TABLE_S_INFO: ClassInfo = ClassInfo { class_name: "SymbolTable", parent_class: None, static_prop_hash_table: None, inherits_js_type_range: None };

/// `SymbolTable*`: referência compartilhada e mutável.
pub type SymbolTableRef = Rc<RefCell<SymbolTable>>;

/// `SymbolTable`.
#[derive(Debug)]
pub struct SymbolTable {
    cell_id: usize,
    /// `JSCell::m_structureID`: a `vm.symbolTableStructure`.
    structure: StructureRef,
    /// `m_map` (`UncheckedKeyHashMap` com `IdentifierRepHash`, iteração na ordem de bucket).
    map: KeyHashMap<SymbolTableEntry>,
    max_scope_offset: ScopeOffset,
    uses_sloppy_eval: bool,
    nested_lexical_scope: bool,
    scope_type: ScopeType,
    propagate_clone_invalidation_to_original: PropagateCloneInvalidationToOriginal,
    /// `m_rareData->m_privateNames`.
    private_names: Vec<(UniquedKey, PrivateNameEntry)>,
    /// `m_arguments`: só os deslocamentos de cada argumento capturado.
    arguments: Option<Vec<ScopeOffset>>,
}

/// Dá a `SymbolTableRef` o `cell_id()` que o `JSValue::from_cell` consome.
pub trait SymbolTableRefExt {
    fn cell_id(&self) -> usize;
}

impl SymbolTableRefExt for SymbolTableRef {
    fn cell_id(&self) -> usize {
        self.borrow().cell_id
    }
}

impl SymbolTable {
    /// `SymbolTable::create(vm)`.
    pub fn create(vm: &VM) -> SymbolTableRef {
        let cell_id = cell_registry::reserve();
        let table = Rc::new(RefCell::new(SymbolTable {
            cell_id,
            structure: vm.symbol_table_structure(),
            map: KeyHashMap::default(),
            max_scope_offset: ScopeOffset::default(),
            uses_sloppy_eval: false,
            nested_lexical_scope: false,
            scope_type: ScopeType::VarScope,
            propagate_clone_invalidation_to_original: PropagateCloneInvalidationToOriginal::No,
            private_names: Vec::new(),
            arguments: None,
        }));
        cell_registry::set(cell_id, CellEntry::SymbolTable(Rc::clone(&table)));
        table
    }

    /// Procura a tabela pelo `cell_id` guardado em `JSValue::Cell`.
    pub fn from_cell_id(cell_id: usize) -> Option<SymbolTableRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::SymbolTable(table)) => Some(table),
            _ => None,
        }
    }

    pub fn cell_id(&self) -> usize {
        self.cell_id
    }

    /// `JSCell::structure()`.
    pub fn structure(&self) -> &StructureRef {
        &self.structure
    }

    /// `SymbolTable::createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: Option<&crate::runtime::js_global_object::JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(vm, global_object, prototype, TypeInfo::new(JSType::CellType, 0), &SYMBOL_TABLE_S_INFO)
    }

    /// `get(key)`: a entrada rápida, ou a nula se a chave não existe.
    pub fn get(&self, key: &UniquedKey) -> Fast {
        match self.map.find(key) {
            Some(entry) => entry.get_fast(),
            None => Fast::default(),
        }
    }

    /// `find(locker, key)->value.setReadOnly()`: marca a entrada existente como somente leitura.
    /// `false` se a chave não existe.
    pub fn set_read_only_for(&mut self, key: &UniquedKey) -> bool {
        match self.map.find_mut(key) {
            Some(entry) => {
                entry.set_read_only();
                true
            }
            None => false,
        }
    }

    /// `find(key)`.
    pub fn find(&self, key: &UniquedKey) -> Option<&SymbolTableEntry> {
        self.map.find(key)
    }

    pub fn contains(&self, key: &UniquedKey) -> bool {
        self.map.contains(key)
    }

    pub fn size(&self) -> usize {
        self.map.len() as usize
    }

    /// `begin()`/`end()`: ordem de bucket do `UncheckedKeyHashMap`.
    pub fn iter(&self) -> impl Iterator<Item = (&UniquedKey, &SymbolTableEntry)> {
        self.map.iter().map(|(key, entry)| (key, entry))
    }

    pub fn for_each(&self, mut f: impl FnMut(&UniquedKey, &SymbolTableEntry)) {
        for (key, entry) in self.map.iter() {
            f(key, entry);
        }
    }

    pub fn max_scope_offset(&self) -> ScopeOffset {
        self.max_scope_offset
    }

    /// Escrita direta de `m_maxScopeOffset` (o gerador a usa depois de preencher a tabela).
    pub fn set_max_scope_offset(&mut self, offset: ScopeOffset) {
        self.max_scope_offset = offset;
    }

    pub fn did_use_scope_offset(&mut self, offset: ScopeOffset) {
        if self.max_scope_offset.is_invalid() || self.max_scope_offset < offset {
            self.max_scope_offset = offset;
        }
    }

    pub fn did_use_var_offset(&mut self, offset: VarOffset) {
        if offset.is_scope() {
            self.did_use_scope_offset(offset.scope_offset());
        }
    }

    pub fn scope_size(&self) -> u32 {
        // O deslocamento inválido mais um dá zero.
        self.max_scope_offset.offset_unchecked().wrapping_add(1)
    }

    pub fn next_scope_offset(&self) -> ScopeOffset {
        ScopeOffset::new(self.scope_size())
    }

    /// `takeNextScopeOffset()`.
    pub fn take_next_scope_offset(&mut self) -> ScopeOffset {
        let result = self.next_scope_offset();
        self.max_scope_offset = result;
        result
    }

    /// `takeNextScopeOffset(const ConcurrentJSLocker&)`.
    pub fn take_next_scope_offset_locked(&mut self, _locker: NoLockingNecessaryTag) -> ScopeOffset {
        self.take_next_scope_offset()
    }

    /// `add(key, entry)`: a chave não pode existir.
    pub fn add(&mut self, key: UniquedKey, entry: SymbolTableEntry) {
        let inserted = self.insert(key, entry, false);
        debug_assert!(inserted);
    }

    /// `add(const ConcurrentJSLocker&, key, entry)`.
    pub fn add_locked(&mut self, _locker: NoLockingNecessaryTag, key: UniquedKey, entry: SymbolTableEntry) {
        self.add(key, entry);
    }

    /// `set(key, entry)`: substitui se já existir.
    pub fn set(&mut self, key: UniquedKey, entry: SymbolTableEntry) {
        self.insert(key, entry, true);
    }

    /// `set(const ConcurrentJSLocker&, key, entry)`.
    pub fn set_locked(&mut self, _locker: NoLockingNecessaryTag, key: UniquedKey, entry: SymbolTableEntry) {
        self.set(key, entry);
    }

    fn insert(&mut self, key: UniquedKey, entry: SymbolTableEntry, replace: bool) -> bool {
        self.did_use_var_offset(entry.var_offset());
        if replace {
            self.map.set(&key, entry).is_new_entry
        } else {
            self.map.add(&key, entry).is_new_entry
        }
    }

    /// `entryFor(locker, offset)`: a entrada cujo deslocamento de escopo é `offset`.
    pub fn entry_for(&self, offset: ScopeOffset) -> Option<&SymbolTableEntry> {
        self.map
            .values()
            .find(|entry| entry.var_offset().is_scope() && entry.var_offset().scope_offset() == offset)
    }

    pub fn has_private_names(&self) -> bool {
        !self.private_names.is_empty()
    }

    pub fn private_names(&self) -> impl Iterator<Item = &(UniquedKey, PrivateNameEntry)> {
        debug_assert!(self.has_private_names());
        self.private_names.iter()
    }

    pub fn add_private_name(&mut self, key: UniquedKey, value: PrivateNameEntry) {
        debug_assert!(!self.has_private_name(&key));
        self.private_names.push((key, value));
    }

    pub fn has_private_name(&self, key: &UniquedKey) -> bool {
        self.private_names.iter().any(|(k, _)| k == key)
    }

    pub fn arguments_length(&self) -> u32 {
        self.arguments.as_ref().map_or(0, |a| a.len() as u32)
    }

    /// `trySetArgumentsLength`: cresce (ou encolhe) a tabela, os novos ficam inválidos. Sempre tem sucesso.
    pub fn try_set_arguments_length(&mut self, _vm: &VM, length: u32) -> bool {
        self.arguments
            .get_or_insert_with(Vec::new)
            .resize(length as usize, ScopeOffset::default());
        true
    }

    pub fn argument_offset(&self, i: u32) -> ScopeOffset {
        self.arguments.as_ref().expect("m_arguments")[i as usize]
    }

    /// `trySetArgumentOffset`.
    pub fn try_set_argument_offset(&mut self, _vm: &VM, i: u32, offset: ScopeOffset) -> bool {
        let arguments = self.arguments.as_mut().expect("m_arguments");
        if i as usize >= arguments.len() {
            return false;
        }
        arguments[i as usize] = offset;
        true
    }

    pub fn uses_sloppy_eval(&self) -> bool {
        self.uses_sloppy_eval
    }

    pub fn set_uses_sloppy_eval(&mut self, uses_sloppy_eval: bool) {
        self.uses_sloppy_eval = uses_sloppy_eval;
    }

    pub fn is_nested_lexical_scope(&self) -> bool {
        self.nested_lexical_scope
    }

    pub fn mark_is_nested_lexical_scope(&mut self) {
        debug_assert!(self.scope_type == ScopeType::LexicalScope);
        self.nested_lexical_scope = true;
    }

    pub fn set_scope_type(&mut self, scope_type: ScopeType) {
        self.scope_type = scope_type;
    }

    pub fn scope_type(&self) -> ScopeType {
        self.scope_type
    }

    pub fn propagate_clone_invalidation_to_original(&self) -> PropagateCloneInvalidationToOriginal {
        self.propagate_clone_invalidation_to_original
    }

    /// `cloneScopePart`: copia só as entradas de escopo (descarta pilha e argumentos diretos),
    /// com os mesmos atributos, mais os argumentos e os nomes privados.
    pub fn clone_scope_part(
        &self,
        vm: &VM,
        propagate: PropagateCloneInvalidationToOriginal,
    ) -> SymbolTableRef {
        let result = SymbolTable::create(vm);
        {
            let mut r = result.borrow_mut();
            r.uses_sloppy_eval = self.uses_sloppy_eval;
            r.nested_lexical_scope = self.nested_lexical_scope;
            r.scope_type = self.scope_type;
            r.arguments = self.arguments.clone();
            for (key, entry) in self.map.iter() {
                if !entry.var_offset().is_scope() {
                    continue;
                }
                let copy = SymbolTableEntry::new(entry.var_offset(), entry.get_attributes());
                r.map.add(key, copy);
            }
            // `result->m_maxScopeOffset = m_maxScopeOffset` (as entradas copiadas não o recalculam).
            r.max_scope_offset = self.max_scope_offset;
            r.private_names = self.private_names.clone();
            r.propagate_clone_invalidation_to_original = propagate;
        }
        result
    }
}

#[cfg(test)]
mod symbol_table_tests {
    use super::*;
    use crate::wtf::text::string_impl::StringImpl;

    fn key(text: &[u8]) -> UniquedKey {
        UniquedKey(StringImpl::create(text))
    }

    #[test]
    fn add_get_and_scope_offsets() {
        let vm = VM::default();
        let table = SymbolTable::create(&vm);
        let mut t = table.borrow_mut();
        assert_eq!(t.scope_size(), 0);
        let a = key(b"a");
        let offset = t.take_next_scope_offset();
        assert_eq!(offset.offset(), 0);
        t.add(a.clone(), SymbolTableEntry::new(VarOffset::from_scope_offset(offset), READ_ONLY));
        assert!(t.contains(&a));
        assert_eq!(t.size(), 1);
        assert!(t.get(&a).is_read_only());
        assert!(t.get(&key(b"b")).is_null());
        assert_eq!(t.take_next_scope_offset().offset(), 1);
        assert_eq!(t.max_scope_offset().offset(), 1);
    }

    #[test]
    fn iterates_in_wtf_hash_map_order_like_bun_global() {
        // Oráculo: o global limpo de um contexto novo do bun 1.4.2, `vm.runInNewContext(src + ';Object.getOwnPropertyNames(globalThis)')`
        // (o global principal tem histórico de inserções do host, não serve). A ordem inserida é a de
        // `ProgramExecutable::initializeGlobalProperties`: `NaN`, `Infinity`, `undefined` (`initStaticGlobals`),
        // depois as funções (`createGlobalFunctionBinding`, na ordem do programa) e só então os `var`
        // (`createGlobalVarBinding`); ambos acabam em `addSymbolTableEntry`, ou seja, tudo na SymbolTable.
        // A lista do bun começa pelas funções, na ordem do programa, porque o NodeVM espelha cada função
        // declarada no objeto sandbox, que `getOwnPropertyNames` enumera antes; o resto vem da SymbolTable.
        fn global_names(functions: &[&str], vars: &[&str]) -> Vec<String> {
            let vm = VM::default();
            let table = SymbolTable::create(&vm);
            let mut t = table.borrow_mut();
            for name in ["NaN", "Infinity", "undefined"].iter().chain(functions).chain(vars) {
                let offset = t.take_next_scope_offset();
                t.add(key(name.as_bytes()), SymbolTableEntry::new(VarOffset::from_scope_offset(offset), 0));
            }
            let table_order = t.iter().map(|(k, _)| String::from_utf8(k.0.span8().to_vec()).unwrap());
            let names: Vec<String> =
                functions.iter().map(|f| f.to_string()).chain(table_order.filter(|n| !functions.contains(&n.as_str()))).collect();
            names
        }

        assert_eq!(
            global_names(&["fn1", "qux"], &["a1", "b2", "zeta", "alpha", "x", "foo", "bar", "baz"]),
            ["fn1", "qux", "a1", "bar", "b2", "zeta", "alpha", "Infinity", "baz", "x", "NaN", "foo", "undefined"]
        );
        assert_eq!(
            global_names(&["b", "c", "z"], &["q", "r"]),
            ["b", "c", "z", "r", "Infinity", "NaN", "q", "undefined"]
        );
        // Contexto novo sem declaração nenhuma: `undefined, NaN, Infinity`.
        assert_eq!(global_names(&[], &[]), ["undefined", "NaN", "Infinity"]);
    }

    #[test]
    fn iterates_infinity_undefined_nan_with_bun_host_static_globals() {
        // Oráculo: `Object.getOwnPropertyNames(globalThis)` do bun 1.4.2 começa por `Infinity, undefined, NaN`.
        // O host (ZigGlobalObject.cpp:2873, `addStaticGlobals`) acrescenta à SymbolTable do global, depois de
        // `initStaticGlobals`, 23 símbolos privados (`@lazy`, 18 funções privadas, `ArrayBuffer`,
        // `internalModuleRegistry`, `processBindingConstants`, `requireMap`). Com 26 entradas a tabela cresce
        // até 64 buckets, onde `NaN`, `Infinity` e `undefined` caem nos buckets 28, 5 e 13 (hash WTF
        // mascarado), ou seja, a ordem `Infinity, undefined, NaN`. Com 8 buckets (contexto limpo) é outra.
        // NÃO EXECUTADO ainda (sem cargo na fatia): confirmar que nenhum símbolo desloca as três chaves.
        use crate::wtf::text::symbol_impl::PrivateSymbolImpl;
        let vm = VM::default();
        let table = SymbolTable::create(&vm);
        let mut t = table.borrow_mut();
        for name in ["NaN", "Infinity", "undefined"] {
            let offset = t.take_next_scope_offset();
            t.add(key(name.as_bytes()), SymbolTableEntry::new(VarOffset::from_scope_offset(offset), 0));
        }
        for index in 0..23 {
            let rep = StringImpl::create(format!("hostPrivate{index}").as_bytes());
            let symbol = PrivateSymbolImpl::create(&rep);
            let offset = t.take_next_scope_offset();
            t.add(UniquedKey(symbol.string_impl().clone()), SymbolTableEntry::new(VarOffset::from_scope_offset(offset), 0));
        }
        let strings: Vec<String> = t
            .iter()
            .filter(|(k, _)| !k.0.is_symbol())
            .map(|(k, _)| String::from_utf8(k.0.span8().to_vec()).unwrap())
            .collect();
        assert_eq!(strings, ["Infinity", "undefined", "NaN"]);
    }

    #[test]
    fn clone_scope_part_drops_non_scope_entries() {
        let vm = VM::default();
        let table = SymbolTable::create(&vm);
        let (s, k) = (key(b"s"), key(b"k"));
        {
            let mut t = table.borrow_mut();
            t.set_scope_type(ScopeType::LexicalScope);
            t.add(s.clone(), SymbolTableEntry::new(VarOffset::from_scope_offset(ScopeOffset::new(0)), DONT_ENUM));
            t.add(
                k.clone(),
                SymbolTableEntry::from_var_offset(VarOffset::from_direct_arguments_offset(
                    crate::runtime::direct_arguments_offset::DirectArgumentsOffset::new(1),
                )),
            );
        }
        let cloned = table.borrow().clone_scope_part(&vm, PropagateCloneInvalidationToOriginal::No);
        let c = cloned.borrow();
        assert_eq!(c.scope_type(), ScopeType::LexicalScope);
        assert!(c.contains(&s) && !c.contains(&k));
        assert!(c.get(&s).is_dont_enum());
        assert_ne!(c.cell_id(), table.cell_id());
        assert!(Rc::ptr_eq(&SymbolTable::from_cell_id(c.cell_id()).unwrap(), &cloned));
    }
}
