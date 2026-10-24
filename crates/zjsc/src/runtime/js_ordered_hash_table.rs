//! Tradução da superfície mínima de `runtime/JSOrderedHashTable.h` que o `VM` e o bytecompiler
//! usam: `JSOrderedHashMap::createSentinel(VM&)` (`MapTraits`/`SetTraits` têm o mesmo corpo) e a
//! criação do `Storage` vazio de `JSOrderedHashTableHelper::tryCreate(VM&, int length)`.
//!
//! Fora desta fatia, e por quê: `JSOrderedHashTable` como classe (`m_storage`, `materializeIfNeeded`,
//! `tryGetStorage`, `storageOrSentinel`), `tryCreate(JSGlobalObject*, ...)`, `copyImpl`, as buscas
//! e as inserções do `JSOrderedHashTableHelper.h` (dependem de `JSGlobalObject`, do `ThrowScope`,
//! do hash de `JSValue` e do GC).
//!
//! DIVERGÊNCIA (heap ausente, camada 3): no C++ o `Storage` do `Helper` é um `JSCellButterfly`
//! (`using Storage = JSCellButterfly`), então a sentinela é uma célula desse tipo, de tamanho 0 e
//! com a estrutura copy-on-write `Contiguous` (`vm.cellButterflyStructure(CopyOnWriteArrayWithContiguous)`).
//! Aqui ela é a mesma `JSCellButterfly` do registro central (`cell_registry`), cujo `cell_id`
//! é a identidade que `JSValue::Cell` guarda e que
//! `vm.orderedHashTableSentinel()` compara. O registro mantém a célula viva, no papel do GC.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::indexing_type::COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS;
use crate::runtime::iteration_kind::IterationKind;
use crate::runtime::js_cell_butterfly::{JSCellButterfly, JSCellButterflyRef};
use crate::runtime::js_value::JSValue;
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `JSOrderedHashTableHelper::tryCreate(VM&, int length)`: `Storage::tryCreate(vm,
/// vm.cellButterflyStructure(CopyOnWriteArrayWithContiguous), length)`.
pub(crate) fn try_create_storage(vm: &VM, length: u32) -> Option<JSCellButterflyRef> {
    // FIXME do C++: "Why is this CopyOnWrite? We definitely modify it...".
    JSCellButterfly::try_create(vm, vm.cell_butterfly_structure(COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS), length)
}

/// `class JSOrderedHashMap`, só o que o `VM` usa.
pub struct JSOrderedHashMap;

impl JSOrderedHashMap {
    /// `static JSCell* createSentinel(VM& vm) { return Helper::tryCreate(vm, 0); }`. O comprimento 0
    /// nunca passa de `IndexingHeader::maximumLength`, então a criação não falha.
    pub fn create_sentinel(vm: &VM) -> JSCellButterflyRef {
        try_create_storage(vm, 0).expect("JSOrderedHashMap::createSentinel: tryCreate(vm, 0) devolveu nulo")
    }
}

// ---------------------------------------------------------------------------------------------
// Tabela ordenada de `JSValue` (o `JSOrderedHashTable<Traits>` de `JSMap`, `JSSet`, `JSWeakMap` e
// `JSWeakSet`).
//
// DIVERGÊNCIA (heap ausente): no C++ o `Storage` é um `JSCellButterfly` com buckets encadeados, a
// remoção deixa o `deletedValue` e o `rehash` deixa um rastro (`Helper::transitionAndWriteBarrier`)
// que os iteradores seguem. Aqui a tabela é um `Vec` de entradas na ordem de inserção mais um
// `HashMap<HashKey, índice>`; remover deixa um buraco (`None`), e a compactação ajusta os cursores
// vivos dos iteradores no lugar do rastro de transição. A semântica observável é a do ECMAScript: a
// iteração é viva, vê entradas acrescentadas depois, pula as removidas, e `clear` leva todo cursor
// de volta ao começo. A igualdade é a SameValueZero de `normalizeMapKey` mais a comparação por
// conteúdo de `JSString` e `JSBigInt` (o `jsMapHash`/`HashMapHelper`); objetos e símbolos por
// identidade (o `cell_id`).
// ---------------------------------------------------------------------------------------------

/// A chave já normalizada, em forma de `Eq + Hash`.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum HashKey {
    Undefined,
    Null,
    Bool(bool),
    /// Os bits do `f64` normalizado (int32 entra como `f64`; `-0` e NaN já foram normalizados).
    Number(u64),
    /// `JSString`: igualdade por conteúdo.
    String(WtfString),
    /// `JSBigInt`: sinal e dígitos.
    BigInt(bool, Vec<u64>),
    /// Objeto, símbolo ou outra célula: identidade.
    Cell(usize),
}

/// `normalizeMapKey(JSValue)` (`HashMapHelper.h`): NaN vira o NaN puro, `-0` e todo double inteiro
/// viram int32. (O `tryConvertToBigInt32` do `isHeapBigInt` não existe: não há `BigInt32`.)
pub fn normalize_map_key(key: JSValue) -> JSValue {
    match key {
        JSValue::Double(d) => {
            if d.is_nan() {
                return JSValue::nan();
            }
            let truncated = d as i32;
            if truncated as f64 == d {
                // `-0.0 == 0.0`: o `-0` vira `+0`.
                return JSValue::Int32(truncated);
            }
            key
        }
        _ => key,
    }
}

/// A chave de hash de um valor (normaliza antes).
pub fn hash_key(key: JSValue) -> HashKey {
    match normalize_map_key(key) {
        JSValue::Undefined => HashKey::Undefined,
        JSValue::Null => HashKey::Null,
        JSValue::Bool(b) => HashKey::Bool(b),
        JSValue::Int32(i) => HashKey::Number((i as f64).to_bits()),
        JSValue::Double(d) => HashKey::Number(d.to_bits()),
        JSValue::Cell(cell_id) => match cell_registry::get(cell_id) {
            Some(CellEntry::String(string)) => HashKey::String(string.value()),
            Some(CellEntry::BigInt(big_int)) => HashKey::BigInt(big_int.sign(), big_int.digits().to_vec()),
            _ => HashKey::Cell(cell_id),
        },
        JSValue::Empty | JSValue::Deleted => {
            debug_assert!(false, "chave vazia ou deletada em tabela ordenada");
            HashKey::Cell(0)
        }
    }
}

/// A posição de um iterador na tabela, compartilhada com ela para a compactação poder ajustá-la.
pub type Cursor = Rc<Cell<usize>>;

/// O resultado de um passo de `JSMapIterator::next` e `JSSetIterator::next`; o chamador monta o
/// `{ value, done }` e o par `[chave, valor]` (`createIteratorResultObject`, `constructArray`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum IteratorStep {
    Done,
    Item(JSValue),
    Entry(JSValue, JSValue),
}

/// Erro das operações de `Map`, `Set`, `WeakMap` e `WeakSet`: o chamador cria a exceção no realm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CollectionError {
    /// `createNotAnObjectError(globalObject, thisValue)`.
    NotAnObject(JSValue),
    /// `throwTypeError(globalObject, scope, message)`.
    TypeError(&'static str),
}

const COMPACTION_MINIMUM_LENGTH: usize = 32;

/// `JSOrderedHashTable`: entradas `(chave, valor)` na ordem de inserção. No `Set` o valor é `empty`.
#[derive(Default)]
pub struct OrderedTable {
    entries: Vec<Option<(JSValue, JSValue)>>,
    index: HashMap<HashKey, usize>,
    live: usize,
    cursors: Vec<Weak<Cell<usize>>>,
}

impl OrderedTable {
    /// `JSOrderedHashTable::visitChildren` (o `m_storage`): as chaves e os valores vivos.
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        for (key, value) in self.entries.iter().flatten() {
            marker.value(*key);
            marker.value(*value);
        }
    }

    /// O passo de efêmeros de `WeakMap` (`WeakMapImpl::visitOutputConstraints`): o valor de chave marcada é
    /// marcado. Devolve se marcou algo novo, para o coletor repetir até o ponto fixo.
    pub(crate) fn visit_ephemerons(&self, marker: &mut crate::runtime::gc::Marker) -> bool {
        let mut progressed = false;
        for (key, value) in self.entries.iter().flatten() {
            if let (JSValue::Cell(key), JSValue::Cell(value)) = (key, value) {
                if !marker.is_doomed(*key) && marker.is_doomed(*value) {
                    marker.cell(*value);
                    progressed = true;
                }
            }
        }
        progressed
    }

    /// `WeakMapImpl::finalizeUnconditionally`: as entradas de chave que a varredura vai soltar saem.
    pub(crate) fn remove_dead_keys(&mut self, marker: &crate::runtime::gc::Marker) {
        let dead: Vec<JSValue> =
            self.entries.iter().flatten().map(|(key, _)| *key).filter(|key| matches!(key, JSValue::Cell(id) if marker.is_doomed(*id))).collect();
        for key in dead {
            self.remove(key);
        }
    }

    /// `size()`: `aliveEntryCount`.
    pub fn size(&self) -> u32 {
        self.live as u32
    }

    /// `has(globalObject, key)`.
    pub fn has(&self, key: JSValue) -> bool {
        self.index.contains_key(&hash_key(key))
    }

    /// O valor da chave, `None` se ausente (o `JSValue()` vazio do `getImpl`).
    pub fn get_entry(&self, key: JSValue) -> Option<JSValue> {
        let position = *self.index.get(&hash_key(key))?;
        self.entries[position].map(|(_, value)| value)
    }

    /// `JSOrderedHashMap::get`: `undefined` quando ausente.
    pub fn get(&self, key: JSValue) -> JSValue {
        self.get_entry(key).unwrap_or(JSValue::Undefined)
    }

    /// `add(globalObject, key, value)`: insere ou troca o valor (a chave guardada é a normalizada, e a
    /// primeira inserida fica, como no C++).
    pub fn add(&mut self, key: JSValue, value: JSValue) {
        let key = normalize_map_key(key);
        let hash = hash_key(key);
        if let Some(&position) = self.index.get(&hash) {
            if let Some(entry) = self.entries[position].as_mut() {
                entry.1 = value;
            }
            return;
        }
        self.index.insert(hash, self.entries.len());
        self.entries.push(Some((key, value)));
        self.live += 1;
    }

    /// `remove(globalObject, key)`.
    pub fn remove(&mut self, key: JSValue) -> bool {
        let Some(position) = self.index.remove(&hash_key(key)) else {
            return false;
        };
        self.entries[position] = None;
        self.live -= 1;
        if self.entries.len() >= COMPACTION_MINIMUM_LENGTH && self.live * 2 < self.entries.len() {
            self.compact();
        }
        true
    }

    /// `clear(globalObject)`: os cursores vivos voltam ao começo, onde as próximas entradas entram.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.index.clear();
        self.live = 0;
        self.cursors.retain(|cursor| match cursor.upgrade() {
            Some(cursor) => {
                cursor.set(0);
                true
            }
            None => false,
        });
    }

    /// Um cursor novo na posição 0, registrado para a compactação.
    pub fn new_cursor(&mut self) -> Cursor {
        let cursor: Cursor = Rc::new(Cell::new(0));
        self.cursors.push(Rc::downgrade(&cursor));
        cursor
    }

    /// A próxima entrada viva a partir do cursor, que avança para depois dela.
    pub fn next_entry(&self, cursor: &Cursor) -> Option<(JSValue, JSValue)> {
        let start = cursor.get();
        for position in start..self.entries.len() {
            if let Some(entry) = self.entries[position] {
                cursor.set(position + 1);
                return Some(entry);
            }
        }
        cursor.set(self.entries.len());
        None
    }

    /// `JSMap::clone`/`Helper::copy`: as entradas vivas numa tabela nova (sem os cursores).
    pub fn copy_entries(&self) -> OrderedTable {
        let mut copy = OrderedTable::default();
        for (key, value) in self.entries.iter().flatten() {
            copy.add(*key, *value);
        }
        copy
    }

    /// Tira os buracos e leva os cursores vivos para a mesma entrada lógica.
    fn compact(&mut self) {
        let mut before = Vec::with_capacity(self.entries.len() + 1);
        let mut alive = 0usize;
        for entry in &self.entries {
            before.push(alive);
            if entry.is_some() {
                alive += 1;
            }
        }
        before.push(alive);
        self.entries.retain(Option::is_some);
        self.index.clear();
        for (position, entry) in self.entries.iter().enumerate() {
            if let Some((key, _)) = entry {
                self.index.insert(hash_key(*key), position);
            }
        }
        let length = self.entries.len();
        self.cursors.retain(|cursor| match cursor.upgrade() {
            Some(cursor) => {
                cursor.set(before[cursor.get().min(before.len() - 1)].min(length));
                true
            }
            None => false,
        });
    }
}

/// `JSMapIterator::next` / `JSSetIterator::next`: no `Set` o par de `entries` é `[valor, valor]`.
pub fn iterator_step(table: &RefCell<OrderedTable>, cursor: &Cursor, kind: IterationKind, is_set: bool) -> IteratorStep {
    let Some((key, value)) = table.borrow().next_entry(cursor) else {
        return IteratorStep::Done;
    };
    match kind {
        IterationKind::Keys => IteratorStep::Item(key),
        IterationKind::Values => IteratorStep::Item(if is_set { key } else { value }),
        IterationKind::Entries => IteratorStep::Entry(key, if is_set { key } else { value }),
    }
}

/// A célula de coleção (`JSMap`, `JSSet`, `JSWeakMap`, `JSWeakSet`): `JSNonFinalObject` com a
/// `OrderedTable` no lugar do `m_storage`; o `JSType` vem da `Structure`.
macro_rules! define_collection_cell {
    ($name:ident, $ref_name:ident, $variant:ident, $js_type:ident, $info:ident, $class_name:literal) => {
        /// `const ClassInfo ::s_info`.
        pub static $info: $crate::runtime::class_info::ClassInfo = $crate::runtime::class_info::ClassInfo {
            class_name: $class_name,
            parent_class: Some(&$crate::runtime::js_object::JS_NON_FINAL_OBJECT_S_INFO),
            static_prop_hash_table: None, inherits_js_type_range: None,
        };

        pub struct $name {
            base: $crate::runtime::js_object::JSNonFinalObject,
            table: ::std::cell::RefCell<$crate::runtime::js_ordered_hash_table::OrderedTable>,
        }

        /// A referência à célula, o `*` do C++.
        pub type $ref_name = ::std::rc::Rc<$name>;

        impl ::std::ops::Deref for $name {
            type Target = $crate::runtime::js_object::JSNonFinalObject;

            fn deref(&self) -> &$crate::runtime::js_object::JSNonFinalObject {
                &self.base
            }
        }

        impl $name {
            /// `create(vm, structure)`: tabela vazia (o `m_storage` nulo).
            pub fn create(vm: &$crate::runtime::vm::VM, structure: &$crate::runtime::structure::StructureRef) -> $ref_name {
                let cell_id = $crate::runtime::cell_registry::reserve();
                let cell = ::std::rc::Rc::new($name {
                    base: $crate::runtime::js_object::JSNonFinalObject::new(vm, ::std::rc::Rc::clone(structure)),
                    table: ::std::cell::RefCell::new($crate::runtime::js_ordered_hash_table::OrderedTable::default()),
                });
                cell.set_cell_id(cell_id);
                $crate::runtime::cell_registry::set(cell_id, $crate::runtime::cell_registry::CellEntry::$variant(::std::rc::Rc::clone(&cell)));
                cell
            }

            /// `createStructure(vm, globalObject, prototype)`.
            pub fn create_structure(
                vm: &$crate::runtime::vm::VM,
                global_object: Option<&$crate::runtime::js_global_object::JSGlobalObject>,
                prototype: $crate::runtime::js_value::JSValue,
            ) -> $crate::runtime::structure::StructureRef {
                $crate::runtime::structure::Structure::create(
                    vm,
                    global_object,
                    prototype,
                    $crate::runtime::js_type_info::TypeInfo::new(
                        $crate::runtime::js_type::JSType::$js_type,
                        $crate::runtime::js_object::JSNonFinalObject::STRUCTURE_FLAGS,
                    ),
                    &$info,
                )
            }

            /// Procura a célula pelo `cell_id` guardado em `JSValue::Cell`.
            pub fn from_cell_id(cell_id: usize) -> Option<$ref_name> {
                match $crate::runtime::cell_registry::get(cell_id) {
                    Some($crate::runtime::cell_registry::CellEntry::$variant(cell)) => Some(cell),
                    _ => None,
                }
            }

            /// `dynamicDowncast<...>(value)`.
            pub fn from_value(value: &$crate::runtime::js_value::JSValue) -> Option<$ref_name> {
                match value {
                    $crate::runtime::js_value::JSValue::Cell(cell_id) => $name::from_cell_id(*cell_id),
                    _ => None,
                }
            }

            /// A tabela ordenada (o `m_storage`).
            pub fn table(&self) -> &::std::cell::RefCell<$crate::runtime::js_ordered_hash_table::OrderedTable> {
                &self.table
            }
        }
    };
}

/// A célula de iterador (`JSMapIterator`, `JSSetIterator`): `JSInternalFieldObjectImpl<4>` com os campos
/// `Entry`, `IteratedObject`, `Storage` e `Kind`, na ordem do C++.
///
/// DIVERGÊNCIAS (heap ausente):
/// - `Entry` mora no cursor da tabela (`Cursor`, a posição que a compactação ajusta no lugar do rastro de
///   transição do `Storage`): `field(Entry)` lê o cursor como `jsNumber` e `set_field(Entry, n)` o grava; a
///   posição 0 do vetor `fields` não é usada.
/// - Não há célula `Storage`: o campo nasce `JSValue()` (o `storage()` nulo da coleção vazia) e a tabela
///   da coleção faz o papel do armazenamento. `Storage == vm.orderedHashTableSentinel()` continua sendo
///   "fechado" (`markClosed`); a sentinela fica guardada na criação porque `next` não recebe o `VM`.
/// - `$collection_cell` é a célula da coleção iterada (`JSMap`, `JSSet`).
macro_rules! define_collection_iterator {
    ($name:ident, $ref_name:ident, $variant:ident, $js_type:ident, $info:ident, $class_name:literal, $collection_cell:ident, $is_set:literal) => {
        /// `JSInternalFieldObjectImpl<N>` com `N` igual a `JSMapIteratorNumberOFInternalFields`.
        pub const NUMBER_OF_INTERNAL_FIELDS: u32 = 4;

        /// `enum class Field : uint8_t`.
        #[repr(u8)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Field {
            Entry = 0,
            IteratedObject = 1,
            Storage = 2,
            Kind = 3,
        }

        /// `const ClassInfo ::s_info`.
        pub static $info: $crate::runtime::class_info::ClassInfo = $crate::runtime::class_info::ClassInfo {
            class_name: $class_name,
            parent_class: Some(&$crate::runtime::js_object::JS_NON_FINAL_OBJECT_S_INFO),
            static_prop_hash_table: None, inherits_js_type_range: None,
        };

        pub struct $name {
            base: $crate::runtime::js_object::JSNonFinalObject,
            fields: ::std::cell::RefCell<[$crate::runtime::js_value::JSValue; NUMBER_OF_INTERNAL_FIELDS as usize]>,
            cursor: $crate::runtime::js_ordered_hash_table::Cursor,
            sentinel: usize,
        }

        impl $crate::runtime::js_internal_field_object_impl::InternalFields for $name {
            fn field_count(&self) -> u32 {
                NUMBER_OF_INTERNAL_FIELDS
            }

            fn field(&self, index: u32) -> $crate::runtime::js_value::JSValue {
                if index == Field::Entry as u32 {
                    return $crate::runtime::js_value::JSValue::from_u32(self.cursor.get() as u32);
                }
                self.fields.borrow()[index as usize]
            }

            fn set_field(&self, index: u32, value: $crate::runtime::js_value::JSValue) {
                if index == Field::Entry as u32 {
                    let entry = if value.is_int32() { value.as_int32() as usize } else { value.as_double() as usize };
                    self.cursor.set(entry);
                    return;
                }
                self.fields.borrow_mut()[index as usize] = value;
            }
        }

        /// A referência à célula, o `*` do C++.
        pub type $ref_name = ::std::rc::Rc<$name>;

        impl ::std::ops::Deref for $name {
            type Target = $crate::runtime::js_object::JSNonFinalObject;

            fn deref(&self) -> &$crate::runtime::js_object::JSNonFinalObject {
                &self.base
            }
        }

        impl $name {
            /// `initialValues()`: `{ jsNumber(0), jsNull(), JSValue(), jsNumber(0) }`.
            fn initial_values() -> [$crate::runtime::js_value::JSValue; NUMBER_OF_INTERNAL_FIELDS as usize] {
                [
                    $crate::runtime::js_value::JSValue::Int32(0),
                    $crate::runtime::js_value::JSValue::Null,
                    $crate::runtime::js_value::JSValue::empty(),
                    $crate::runtime::js_value::JSValue::Int32(0),
                ]
            }

            /// O construtor e o registro da célula, com os campos em `fields` e o cursor dado.
            fn allocate(
                vm: &$crate::runtime::vm::VM,
                structure: &$crate::runtime::structure::StructureRef,
                fields: [$crate::runtime::js_value::JSValue; NUMBER_OF_INTERNAL_FIELDS as usize],
                cursor: $crate::runtime::js_ordered_hash_table::Cursor,
            ) -> $ref_name {
                let cell_id = $crate::runtime::cell_registry::reserve();
                let cell = ::std::rc::Rc::new($name {
                    base: $crate::runtime::js_object::JSNonFinalObject::new(vm, ::std::rc::Rc::clone(structure)),
                    fields: ::std::cell::RefCell::new(fields),
                    cursor,
                    sentinel: vm.ordered_hash_table_sentinel(),
                });
                cell.set_cell_id(cell_id);
                $crate::runtime::cell_registry::set(cell_id, $crate::runtime::cell_registry::CellEntry::$variant(::std::rc::Rc::clone(&cell)));
                cell
            }

            /// `create(vm, structure, iteratedObject, kind)` e o `finishCreation(vm, iteratedObject, kind)`:
            /// `Entry` 0, `IteratedObject`, `Storage` (o `storage()` da coleção, aqui nulo) e `Kind`.
            pub fn create(
                vm: &$crate::runtime::vm::VM,
                structure: &$crate::runtime::structure::StructureRef,
                collection: &::std::rc::Rc<$collection_cell>,
                kind: $crate::runtime::iteration_kind::IterationKind,
            ) -> $ref_name {
                let cursor = collection.table().borrow_mut().new_cursor();
                let mut fields = $name::initial_values();
                fields[Field::IteratedObject as usize] = collection.as_value();
                fields[Field::Kind as usize] = $crate::runtime::js_value::JSValue::from_u32(kind as u32);
                $name::allocate(vm, structure, fields, cursor)
            }

            /// `createWithInitialValues(vm, structure)`: sem coleção, o cursor não está em nenhuma tabela.
            pub fn create_with_initial_values(
                vm: &$crate::runtime::vm::VM,
                structure: &$crate::runtime::structure::StructureRef,
            ) -> $ref_name {
                $name::allocate(vm, structure, $name::initial_values(), ::std::rc::Rc::new(::std::cell::Cell::new(0)))
            }

            /// `createStructure(vm, globalObject, prototype)`.
            pub fn create_structure(
                vm: &$crate::runtime::vm::VM,
                global_object: Option<&$crate::runtime::js_global_object::JSGlobalObject>,
                prototype: $crate::runtime::js_value::JSValue,
            ) -> $crate::runtime::structure::StructureRef {
                $crate::runtime::structure::Structure::create(
                    vm,
                    global_object,
                    prototype,
                    $crate::runtime::js_type_info::TypeInfo::new(
                        $crate::runtime::js_type::JSType::$js_type,
                        $crate::runtime::js_object::JSNonFinalObject::STRUCTURE_FLAGS,
                    ),
                    &$info,
                )
            }

            /// Procura a célula pelo `cell_id` guardado em `JSValue::Cell`.
            pub fn from_cell_id(cell_id: usize) -> Option<$ref_name> {
                match $crate::runtime::cell_registry::get(cell_id) {
                    Some($crate::runtime::cell_registry::CellEntry::$variant(cell)) => Some(cell),
                    _ => None,
                }
            }

            /// `dynamicDowncast<...>(value)`.
            pub fn from_value(value: &$crate::runtime::js_value::JSValue) -> Option<$ref_name> {
                match value {
                    $crate::runtime::js_value::JSValue::Cell(cell_id) => $name::from_cell_id(*cell_id),
                    _ => None,
                }
            }

            /// `internalField(field).get()`.
            pub fn internal_field(&self, field: Field) -> $crate::runtime::js_value::JSValue {
                $crate::runtime::js_internal_field_object_impl::InternalFields::field(self, field as u32)
            }

            /// `internalField(field).set(vm, this, value)`.
            pub fn set_internal_field(&self, field: Field, value: $crate::runtime::js_value::JSValue) {
                $crate::runtime::js_internal_field_object_impl::InternalFields::set_field(self, field as u32, value);
            }

            /// `kind()`.
            pub fn kind(&self) -> $crate::runtime::iteration_kind::IterationKind {
                match self.internal_field(Field::Kind).as_uint32() {
                    0 => $crate::runtime::iteration_kind::IterationKind::Keys,
                    1 => $crate::runtime::iteration_kind::IterationKind::Values,
                    2 => $crate::runtime::iteration_kind::IterationKind::Entries,
                    other => unreachable!("iterador com kind {other}"),
                }
            }

            /// `iteratedObject()`.
            pub fn iterated_object(&self) -> ::std::rc::Rc<$collection_cell> {
                $collection_cell::from_value(&self.internal_field(Field::IteratedObject))
                    .expect("uncheckedDowncast do IteratedObject do iterador")
            }

            /// `entry()`: a posição do cursor.
            pub fn entry(&self) -> usize {
                self.cursor.get()
            }

            /// `setEntry(vm, entry)`.
            pub fn set_entry(&self, entry: usize) {
                self.cursor.set(entry);
            }

            /// `Storage == vm.orderedHashTableSentinel()`.
            fn is_closed(&self) -> bool {
                self.internal_field(Field::Storage) == $crate::runtime::js_value::JSValue::from_cell(self.sentinel)
            }

            /// `markClosed(sentinel)` / `close(vm)`.
            pub fn close(&self) {
                self.set_internal_field(Field::Storage, $crate::runtime::js_value::JSValue::from_cell(self.sentinel));
            }

            /// `nextWithAdvance`/`next(globalObject, value)`: depois de `Done` o iterador fica fechado
            /// para sempre (o C++ troca o `Storage` pela sentinela).
            pub fn next(&self) -> $crate::runtime::js_ordered_hash_table::IteratorStep {
                if self.is_closed() {
                    return $crate::runtime::js_ordered_hash_table::IteratorStep::Done;
                }
                let step = $crate::runtime::js_ordered_hash_table::iterator_step(
                    self.iterated_object().table(),
                    &self.cursor,
                    self.kind(),
                    $is_set,
                );
                if step == $crate::runtime::js_ordered_hash_table::IteratorStep::Done {
                    self.close();
                }
                step
            }
        }
    };
}

pub(crate) use define_collection_cell;
pub(crate) use define_collection_iterator;
