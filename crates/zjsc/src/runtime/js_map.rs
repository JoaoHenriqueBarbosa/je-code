//! Porte de `runtime/JSMap.{h,cpp}`, `JSMapIterator.{h,cpp}` e dos algoritmos de
//! `MapPrototype.cpp` / `MapIteratorPrototype.cpp`, como funções puras sobre `JSValue`.
//!
//! As células (`JSMap`, `JSMapIterator`) saem de `define_collection_cell!` e
//! `define_collection_iterator!` (`js_ordered_hash_table`), registradas como `CellEntry::Map` e
//! `CellEntry::MapIterator`. Cada `mapProtoFunc*` vira uma função `map_proto_*` que recebe o `this`
//! (`callFrame->thisValue()`) e os argumentos já como `JSValue`, e devolve `Result<_, CollectionError>`;
//! o chamador (a ligação com `NativeFunction`, que outro agente redesenha) cria a exceção no realm.
//!
//! Fora desta fatia, e por quê:
//! - `JSMap::set` fast path, `isSetFastAndNonObservable`, `isIteratorProtocolFastAndNonObservable`
//!   (dependem dos watchpoints do `JSGlobalObject`);
//! - `MapConstructor` (`new Map(iterable)` itera e chama o `set` do protótipo, precisa do protocolo de
//!   iteração e de `call`);
//! - `Map.prototype.forEach` é JS embutido no JSC (`builtins/MapPrototype.js`); aqui está o laço vivo
//!   em `map_proto_for_each`, que só um chamador nativo usa (o embutido usa os intrínsecos de
//!   `ordered_hash_table_storage.rs`);
//! - o `Symbol.iterator`/`toStringTag` e a instalação das propriedades (`finishCreation` do protótipo).

use crate::runtime::iteration_kind::IterationKind;
use crate::runtime::js_ordered_hash_table::{
    define_collection_cell, define_collection_iterator, CollectionError, IteratorStep,
};
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;

define_collection_cell!(JSMap, JSMapRef, Map, JSMapType, JS_MAP_S_INFO, "Map");
define_collection_iterator!(
    JSMapIterator,
    JSMapIteratorRef,
    MapIterator,
    JSMapIteratorType,
    JS_MAP_ITERATOR_S_INFO,
    "Map Iterator",
    JSMap,
    false
);

pub const MAP_NOT_A_MAP_MESSAGE: &str = "Map operation called on non-Map object";
pub const MAP_ITERATOR_NOT_AN_ITERATOR_MESSAGE: &str =
    "%MapIteratorPrototype%.next requires that |this| be a Map Iterator instance";
pub const MAP_GET_OR_INSERT_COMPUTED_NOT_CALLABLE_MESSAGE: &str =
    "Map.prototype.getOrInsertComputed requires the callback argument to be callable.";

impl JSMap {
    /// `JSMap::clone(globalObject, vm, structure)`: as entradas vivas numa célula nova.
    pub fn clone_with_structure(&self, vm: &VM, structure: &StructureRef) -> JSMapRef {
        let copy = JSMap::create(vm, structure);
        *copy.table().borrow_mut() = self.table().borrow().copy_entries();
        copy
    }

    /// `JSMap::set(globalObject, key, value)`.
    pub fn set(&self, key: JSValue, value: JSValue) {
        self.table().borrow_mut().add(key, value);
    }
}

/// `getMap(globalObject, thisValue)`.
pub fn get_map(this_value: JSValue) -> Result<JSMapRef, CollectionError> {
    if !this_value.is_cell() {
        return Err(CollectionError::NotAnObject(this_value));
    }
    JSMap::from_value(&this_value).ok_or(CollectionError::TypeError(MAP_NOT_A_MAP_MESSAGE))
}

/// `mapProtoFuncClear`.
pub fn map_proto_clear(this_value: JSValue) -> Result<JSValue, CollectionError> {
    get_map(this_value)?.table().borrow_mut().clear();
    Ok(JSValue::Undefined)
}

/// `mapProtoFuncDelete`.
pub fn map_proto_delete(this_value: JSValue, key: JSValue) -> Result<JSValue, CollectionError> {
    Ok(JSValue::Bool(get_map(this_value)?.table().borrow_mut().remove(key)))
}

/// `mapProtoFuncGet`.
pub fn map_proto_get(this_value: JSValue, key: JSValue) -> Result<JSValue, CollectionError> {
    Ok(get_map(this_value)?.table().borrow().get(key))
}

/// `mapProtoFuncHas`.
pub fn map_proto_has(this_value: JSValue, key: JSValue) -> Result<JSValue, CollectionError> {
    Ok(JSValue::Bool(get_map(this_value)?.table().borrow().has(key)))
}

/// `mapProtoFuncSet`: devolve o próprio `this`.
pub fn map_proto_set(this_value: JSValue, key: JSValue, value: JSValue) -> Result<JSValue, CollectionError> {
    get_map(this_value)?.set(key, value);
    Ok(this_value)
}

/// `mapProtoFuncSize`.
pub fn map_proto_size(this_value: JSValue) -> Result<JSValue, CollectionError> {
    Ok(JSValue::from_u32(get_map(this_value)?.table().borrow().size()))
}

/// `mapProtoFuncGetOrInsert`.
pub fn map_proto_get_or_insert(this_value: JSValue, key: JSValue, value: JSValue) -> Result<JSValue, CollectionError> {
    let map = get_map(this_value)?;
    let mut table = map.table().borrow_mut();
    if let Some(existing) = table.get_entry(key) {
        return Ok(existing);
    }
    table.add(key, value);
    Ok(value)
}

/// `mapProtoFuncGetOrInsertComputed`: `callback(key)` roda sem a tabela emprestada (pode mexer no
/// mapa), e a inserção depois dele revê a chave (`getOrInsert` do C++: "there is a chance that callback
/// inserts an entry for this |key|", e então o `add` troca o valor). A conferência de `isCallable` do
/// `valueCallback` é do chamador, que usa `MAP_GET_OR_INSERT_COMPUTED_NOT_CALLABLE_MESSAGE`.
pub fn map_proto_get_or_insert_computed<E: From<CollectionError>>(
    this_value: JSValue,
    key: JSValue,
    callback: impl FnOnce(JSValue) -> Result<JSValue, E>,
) -> Result<JSValue, E> {
    let map = get_map(this_value)?;
    let key = crate::runtime::js_ordered_hash_table::normalize_map_key(key);
    if let Some(existing) = map.table().borrow().get_entry(key) {
        return Ok(existing);
    }
    let value = callback(key)?;
    map.table().borrow_mut().add(key, value);
    Ok(value)
}

/// `Map.prototype.forEach(callback, thisArg)` (`builtins/MapPrototype.js`): laço vivo sobre um cursor
/// próprio; `callback(value, key, map)`. O primeiro erro do callback encerra o laço.
pub fn map_proto_for_each<E: From<CollectionError>>(
    this_value: JSValue,
    mut callback: impl FnMut(JSValue, JSValue, JSValue) -> Result<(), E>,
) -> Result<JSValue, E> {
    let map = get_map(this_value)?;
    let cursor = map.table().borrow_mut().new_cursor();
    loop {
        // O empréstimo termina antes do callback, que pode mexer no mapa.
        let next = map.table().borrow().next_entry(&cursor);
        let Some((key, value)) = next else { break };
        callback(value, key, this_value)?;
    }
    Ok(JSValue::Undefined)
}

/// `createMapIteratorObject(globalObject, callFrame, kind)`: o iterador de `keys`/`values`/`entries`
/// (o `structure` é `globalObject->mapIteratorStructure()`).
pub fn create_map_iterator_object(
    vm: &VM,
    structure: &StructureRef,
    this_value: JSValue,
    kind: IterationKind,
) -> Result<JSValue, CollectionError> {
    let map = get_map(this_value)?;
    Ok(JSMapIterator::create(vm, structure, &map, kind).as_value())
}

/// `mapIteratorProtoFuncNext`: o passo; o chamador monta o `createIteratorResultObject`.
pub fn map_iterator_proto_next(this_value: JSValue) -> Result<IteratorStep, CollectionError> {
    JSMapIterator::from_value(&this_value)
        .map(|iterator| iterator.next())
        .ok_or(CollectionError::TypeError(MAP_ITERATOR_NOT_AN_ITERATOR_MESSAGE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::js_ordered_hash_table::normalize_map_key;

    fn new_map(vm: &VM) -> JSMapRef {
        JSMap::create(vm, &JSMap::create_structure(vm, None, JSValue::Null))
    }

    fn int(i: i32) -> JSValue {
        JSValue::Int32(i)
    }

    #[test]
    fn same_value_zero_keys() {
        let vm = VM::new();
        let map = new_map(&vm);
        let this = map.as_value();
        map_proto_set(this, JSValue::Double(-0.0), int(1)).unwrap();
        assert_eq!(map_proto_get(this, int(0)).unwrap(), int(1));
        map_proto_set(this, JSValue::nan(), int(2)).unwrap();
        assert_eq!(map_proto_get(this, JSValue::Double(f64::NAN)).unwrap(), int(2));
        assert_eq!(map_proto_get(this, JSValue::Double(3.0)).unwrap(), JSValue::Undefined);
        map_proto_set(this, JSValue::Double(3.0), int(3)).unwrap();
        assert_eq!(map_proto_get(this, int(3)).unwrap(), int(3));
        assert_eq!(map_proto_size(this).unwrap(), int(3));
        assert_eq!(normalize_map_key(JSValue::Double(-0.0)), int(0));
        // -0 fica guardada como +0.
        let mut seen = Vec::new();
        map_proto_for_each::<CollectionError>(this, |_, key, _| {
            seen.push(key);
            Ok(())
        })
        .unwrap();
        assert_eq!(seen[0], int(0));
    }

    #[test]
    fn string_keys_compare_by_content() {
        let vm = VM::new();
        let map = new_map(&vm);
        let this = map.as_value();
        let a = JSValue::from_js_string(crate::runtime::js_string::js_string(
            &vm,
            &crate::wtf::text::wtf_string::String::from_latin1(b"abc"),
        ));
        let b = JSValue::from_js_string(crate::runtime::js_string::js_string(
            &vm,
            &crate::wtf::text::wtf_string::String::from_latin1(b"abc"),
        ));
        map_proto_set(this, a, int(7)).unwrap();
        assert_eq!(map_proto_has(this, b).unwrap(), JSValue::Bool(true));
        assert_eq!(map_proto_delete(this, b).unwrap(), JSValue::Bool(true));
        assert_eq!(map_proto_size(this).unwrap(), int(0));
    }

    #[test]
    fn object_keys_by_identity() {
        let vm = VM::new();
        let map = new_map(&vm);
        let other = new_map(&vm);
        let this = map.as_value();
        map_proto_set(this, other.as_value(), int(1)).unwrap();
        assert_eq!(map_proto_get(this, other.as_value()).unwrap(), int(1));
        assert_eq!(map_proto_get(this, this).unwrap(), JSValue::Undefined);
    }

    #[test]
    fn iterator_is_live_and_survives_delete_and_clear() {
        let vm = VM::new();
        let map = new_map(&vm);
        let this = map.as_value();
        let structure = JSMapIterator::create_structure(&vm, None, JSValue::Null);
        for i in 0..3 {
            map_proto_set(this, int(i), int(i * 10)).unwrap();
        }
        let iterator = create_map_iterator_object(&vm, &structure, this, IterationKind::Entries).unwrap();
        assert_eq!(map_iterator_proto_next(iterator).unwrap(), IteratorStep::Entry(int(0), int(0)));
        map_proto_delete(this, int(1)).unwrap();
        map_proto_set(this, int(9), int(90)).unwrap();
        assert_eq!(map_iterator_proto_next(iterator).unwrap(), IteratorStep::Entry(int(2), int(20)));
        assert_eq!(map_iterator_proto_next(iterator).unwrap(), IteratorStep::Entry(int(9), int(90)));
        assert_eq!(map_iterator_proto_next(iterator).unwrap(), IteratorStep::Done);
        map_proto_set(this, int(5), int(5)).unwrap();
        assert_eq!(map_iterator_proto_next(iterator).unwrap(), IteratorStep::Done);

        let keys = create_map_iterator_object(&vm, &structure, this, IterationKind::Keys).unwrap();
        assert_eq!(map_iterator_proto_next(keys).unwrap(), IteratorStep::Item(int(0)));
        map_proto_clear(this).unwrap();
        map_proto_set(this, int(7), int(70)).unwrap();
        assert_eq!(map_iterator_proto_next(keys).unwrap(), IteratorStep::Item(int(7)));
    }

    #[test]
    fn compaction_keeps_cursor_on_the_same_entry() {
        let vm = VM::new();
        let map = new_map(&vm);
        let this = map.as_value();
        let structure = JSMapIterator::create_structure(&vm, None, JSValue::Null);
        for i in 0..100 {
            map_proto_set(this, int(i), int(i)).unwrap();
        }
        let values = create_map_iterator_object(&vm, &structure, this, IterationKind::Values).unwrap();
        for _ in 0..60 {
            map_iterator_proto_next(values).unwrap();
        }
        for i in 0..90 {
            map_proto_delete(this, int(i)).unwrap();
        }
        assert_eq!(map_iterator_proto_next(values).unwrap(), IteratorStep::Item(int(90)));
        assert_eq!(map_proto_size(this).unwrap(), int(10));
    }

    #[test]
    fn get_or_insert_and_errors() {
        let vm = VM::new();
        let map = new_map(&vm);
        let this = map.as_value();
        assert_eq!(map_proto_get_or_insert(this, int(1), int(5)).unwrap(), int(5));
        assert_eq!(map_proto_get_or_insert(this, int(1), int(6)).unwrap(), int(5));
        assert_eq!(map_proto_get_or_insert_computed::<CollectionError>(this, int(2), |key| Ok(key)).unwrap(), int(2));
        assert_eq!(map_proto_get(this, int(2)).unwrap(), int(2));
        assert_eq!(map_proto_size(JSValue::Int32(1)), Err(CollectionError::NotAnObject(JSValue::Int32(1))));
        let not_a_map = JSMapIterator::create(
            &vm,
            &JSMapIterator::create_structure(&vm, None, JSValue::Null),
            &map,
            IterationKind::Keys,
        )
        .as_value();
        assert_eq!(map_proto_size(not_a_map), Err(CollectionError::TypeError(MAP_NOT_A_MAP_MESSAGE)));
        assert_eq!(map_iterator_proto_next(this), Err(CollectionError::TypeError(MAP_ITERATOR_NOT_AN_ITERATOR_MESSAGE)));
    }

    #[test]
    fn clone_copies_live_entries() {
        let vm = VM::new();
        let map = new_map(&vm);
        let this = map.as_value();
        map_proto_set(this, int(1), int(2)).unwrap();
        let copy = map.clone_with_structure(&vm, &JSMap::create_structure(&vm, None, JSValue::Null));
        map_proto_set(this, int(3), int(4)).unwrap();
        assert_eq!(copy.table().borrow().size(), 1);
        assert_eq!(copy.table().borrow().get(int(1)), int(2));
    }
}
