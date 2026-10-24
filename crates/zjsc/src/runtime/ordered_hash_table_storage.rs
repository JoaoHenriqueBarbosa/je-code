//! Os alvos nativos privados `mapPrivateFuncMap{Storage,IterationNext,IterationEntry,IterationEntryKey,
//! IterationEntryValue}` (`runtime/MapConstructor.cpp`) e `setPrivateFuncSet{Storage,IterationNext,
//! IterationEntry,IterationEntryKey}` (`runtime/SetConstructor.cpp`), que os builtins JS
//! `Map.prototype.forEach` e `Set.prototype.forEach` (`builtins/MapPrototype.js`, `SetPrototype.js`) chamam
//! como `@mapStorage`, `@mapIterationNext`... Ligados em `js_global_object_link_time_constants.rs`.
//!
//! DIVERGÊNCIA DE REPRESENTAÇÃO (heap ausente): no C++ o "storage" é o próprio `JSMap::Storage`
//! (`JSCellButterfly` com os buckets encadeados): `mapStorage` devolve `storageOrSentinel`,
//! `mapIterationNext(storage, entry)` segue o rastro de transição de `rehash`/`clear` para traduzir o
//! `entry` e devolve o storage (ou a sentinela quando acaba), e `mapIterationEntry*(storage)` lê a entrada
//! que o `next` deixou guardada no storage. O porte guarda a tabela em `OrderedTable` com `Cursor` (ver
//! `js_ordered_hash_table.rs`), sem rastro de transição, então aqui o storage é uma célula leve:
//!
//! - uma `JSCellButterfly` vazia registrada no `cell_registry` (o mesmo tipo de célula do `Storage` do
//!   C++, e o mesmo que a sentinela `vm.orderedHashTableSentinel()`), que dá ao JS um valor opaco para
//!   passar entre as chamadas;
//! - o estado por trás dela (a coleção iterada, o `Cursor` registrado na tabela, a entrada e o par
//!   chave/valor que o último `next` devolveu) mora numa tabela por thread indexada pelo `cell_id`.
//!
//! O `Cursor` faz o papel do rastro: a compactação o ajusta e o `clear` o leva a 0, que é exatamente a
//! tradução de `entry` que o C++ faz ao seguir as transições. Por isso o argumento `entry` de
//! `IterationNext` (o `entryAnterior + 1` que o builtin calcula) é só conferido: o cursor já está na
//! posição certa depois de remoções, inserções e `clear` durante o `forEach`. A ordem observável é a do
//! C++: entradas acrescentadas durante a iteração são visitadas, as removidas ainda não visitadas são
//! puladas, e `clear` recomeça do zero com as entradas novas.
//!
//! Quando o `next` chega ao fim, a célula e o estado são liberados (no C++ o GC faz isso) e o resultado é
//! a sentinela. Um `forEach` que sai por exceção do callback deixa a célula e o cursor registrados até o
//! fim da thread: sem GC no porte não há quando soltá-los.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::host_function;
use crate::runtime::cell_registry;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_map::{JSMap, JSMapRef};
use crate::runtime::js_ordered_hash_table::{try_create_storage, Cursor, OrderedTable};
use crate::runtime::js_set::{JSSet, JSSetRef};
use crate::runtime::js_value::JSValue;

/// A coleção iterada: `JSMap` ou `JSSet`.
enum Collection {
    Map(JSMapRef),
    Set(JSSetRef),
}

impl Collection {
    /// `uncheckedDowncast<JSMap>` / `uncheckedDowncast<JSSet>` do `this` que o builtin já validou com
    /// `@isMap` / `@isSet`.
    fn from_value<const IS_SET: bool>(value: JSValue) -> Collection {
        let collection = if IS_SET {
            JSSet::from_value(&value).map(Collection::Set)
        } else {
            JSMap::from_value(&value).map(Collection::Map)
        };
        collection.expect("storage de coleção pedido para um valor que não é a coleção esperada")
    }

    fn table(&self) -> &RefCell<OrderedTable> {
        match self {
            Collection::Map(map) => map.table(),
            Collection::Set(set) => set.table(),
        }
    }
}

/// O que o `JSMap::Storage` guarda para a iteração do `forEach`.
struct StorageState {
    collection: Collection,
    /// `Entry` e `Position` do iterador: a posição na tabela, ajustada por compactação e `clear`.
    cursor: Cursor,
    /// `getIterationEntry`: a posição da entrada que o último `next` devolveu.
    entry: u32,
    /// `getIterationEntryKey` e `getIterationEntryValue` (vazio no `Set`).
    key: JSValue,
    value: JSValue,
}

thread_local! {
    static STORAGES: RefCell<HashMap<usize, StorageState>> = RefCell::new(HashMap::new());
}

/// Fim do programa (`cell_registry::reset_program_state`): a chave é o `cell_id` da coleção.
/// As raízes do coletor: cada armazenamento e a chave e o valor que ele guarda.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    STORAGES.with(|storages| {
        for (cell_id, state) in storages.borrow().iter() {
            marker.cell(*cell_id);
            marker.values(&[state.key, state.value]);
        }
    });
}

pub(crate) fn reset_for_program() {
    // Tira o mapa e solta depois do `borrow`: o `Drop` dos valores pode voltar aqui.
    let taken = STORAGES.try_with(|storages| std::mem::take(&mut *storages.borrow_mut()));
    drop(taken);
}

/// O `cell_id` de um argumento que o builtin garante ser célula (`ASSERT(argument(0).isCell())`).
fn cell_id_of(value: JSValue) -> usize {
    match value {
        JSValue::Cell(cell_id) => cell_id,
        _ => panic!("storage de coleção: o argumento não é uma célula"),
    }
}

/// `uncheckedDowncast<Storage>(cell)`: roda `body` sobre o estado da célula.
fn with_storage<R>(storage: JSValue, body: impl FnOnce(&mut StorageState) -> R) -> R {
    let cell_id = cell_id_of(storage);
    STORAGES.with(|storages| {
        let mut storages = storages.borrow_mut();
        let state = storages.get_mut(&cell_id).expect("storage de coleção desconhecido (a iteração já terminou?)");
        body(state)
    })
}

/// `mapPrivateFuncMapStorage` / `setPrivateFuncSetStorage`: `storageOrSentinel`. A coleção sempre tem a
/// tabela, então o storage é sempre criado (o `m_storage` nulo do C++ só existe para economizar a alocação
/// de uma coleção vazia, e iterar uma coleção vazia não chama o callback de nenhum jeito).
fn storage_body<const IS_SET: bool>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let collection = Collection::from_value::<IS_SET>(call.argument(0));
    let cursor = collection.table().borrow_mut().new_cursor();
    let cell = try_create_storage(global_object.vm(), 0).expect("tryCreate(vm, 0) devolveu nulo");
    let cell_id = cell.cell_id();
    STORAGES.with(|storages| {
        storages.borrow_mut().insert(
            cell_id,
            StorageState { collection, cursor, entry: 0, key: JSValue::empty(), value: JSValue::empty() },
        );
    });
    Ok(JSValue::from_cell(cell_id))
}

/// `mapPrivateFuncMapIterationNext` / `setPrivateFuncSetIterationNext`: a próxima entrada viva a partir
/// do cursor (ver o cabeçalho sobre o argumento `entry`); `nextAndUpdateIterationEntry` devolve o storage,
/// ou a sentinela quando acaba.
fn iteration_next_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let storage = call.argument(0);
    let sentinel_id = global_object.vm().ordered_hash_table_sentinel();
    let sentinel = JSValue::from_cell(sentinel_id);
    let cell_id = cell_id_of(storage);
    if cell_id == sentinel_id {
        return Ok(sentinel);
    }
    debug_assert!(call.argument(1).is_int32());
    let found = with_storage(storage, |state| {
        // O empréstimo da tabela termina aqui; o callback do `forEach` só roda depois, no JS.
        let next = state.collection.table().borrow().next_entry(&state.cursor);
        let (key, value) = next?;
        state.entry = (state.cursor.get() - 1) as u32;
        state.key = key;
        state.value = value;
        Some(())
    });
    if found.is_some() {
        return Ok(storage);
    }
    STORAGES.with(|storages| storages.borrow_mut().remove(&cell_id));
    cell_registry::remove(cell_id);
    Ok(sentinel)
}

/// `mapPrivateFuncMapIterationEntry` / `setPrivateFuncSetIterationEntry`: `getIterationEntry`.
fn iteration_entry_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(JSValue::from_u32(with_storage(call.argument(0), |state| state.entry)))
}

/// `mapPrivateFuncMapIterationEntryKey` / `setPrivateFuncSetIterationEntryKey`: `getIterationEntryKey`.
fn iteration_entry_key_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(with_storage(call.argument(0), |state| state.key))
}

/// `mapPrivateFuncMapIterationEntryValue`: `getIterationEntryValue`.
fn iteration_entry_value_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(with_storage(call.argument(0), |state| state.value))
}

host_function!(pub map_private_func_map_storage, storage_body::<false>);
host_function!(pub map_private_func_map_iteration_next, iteration_next_body);
host_function!(pub map_private_func_map_iteration_entry, iteration_entry_body);
host_function!(pub map_private_func_map_iteration_entry_key, iteration_entry_key_body);
host_function!(pub map_private_func_map_iteration_entry_value, iteration_entry_value_body);
host_function!(pub set_private_func_set_storage, storage_body::<true>);
host_function!(pub set_private_func_set_iteration_next, iteration_next_body);
host_function!(pub set_private_func_set_iteration_entry, iteration_entry_body);
host_function!(pub set_private_func_set_iteration_entry_key, iteration_entry_key_body);
