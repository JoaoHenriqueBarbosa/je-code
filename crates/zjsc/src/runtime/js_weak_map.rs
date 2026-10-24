//! Porte de `runtime/JSWeakMap.{h,cpp}`, `WeakMapImpl.{h,cpp}` e dos algoritmos de
//! `WeakMapPrototype.cpp`, como funções puras sobre `JSValue` (mesmo desenho de `js_map.rs`).
//!
//! DIVERGÊNCIA (GC ausente): no C++ o `WeakMapImpl` guarda as chaves como `WriteBarrier` fracas e o
//! coletor limpa os buckets das chaves mortas (`finalizeUnconditionally`, `visitOutputConstraints`), e
//! o hash é `jsWeakMapHash(cell)`, a identidade do ponteiro. Aqui a tabela é a `OrderedTable` comum,
//! indexada pelo `cell_id` da chave (a identidade), e a entrada vive até o `delete`. Quando o `Heap` de
//! marcação e varredura chegar, a varredura passa a remover as entradas cujas chaves foram coletadas
//! (a ordem de inserção nunca é observável num `WeakMap`).
//!
//! Fora desta fatia: `WeakMapConstructor` (itera o argumento e chama o `set` do protótipo).

use crate::runtime::js_object::JSObject;
use crate::runtime::js_ordered_hash_table::{define_collection_cell, CollectionError};
use crate::runtime::js_value::JSValue;
use crate::runtime::symbol::Symbol;

define_collection_cell!(JSWeakMap, JSWeakMapRef, WeakMap, JSWeakMapType, JS_WEAK_MAP_S_INFO, "WeakMap");

pub const WEAK_MAP_INVALID_KEY_ERROR: &str = "WeakMap keys must be objects or non-registered symbols";
pub const WEAK_MAP_NOT_AN_OBJECT_MESSAGE: &str = "Called WeakMap function on non-object";
pub const WEAK_MAP_WRONG_RECEIVER_MESSAGE: &str = "Called WeakMap function on a non-WeakMap object";
pub const WEAK_MAP_GET_OR_INSERT_COMPUTED_NOT_CALLABLE_MESSAGE: &str =
    "WeakMap.prototype.getOrInsertComputed requires the callback argument to be callable.";

/// `canBeHeldWeakly(JSValue)`: objeto ou símbolo não registrado (`Symbol.for` não vale).
pub fn can_be_held_weakly(value: JSValue) -> bool {
    if JSObject::from_value(&value).is_some() {
        return true;
    }
    match value {
        JSValue::Cell(cell_id) => Symbol::from_cell_id(cell_id).is_some_and(|symbol| !symbol.uid().is_registered()),
        _ => false,
    }
}

/// O `getWeakMap`/`getWeakSet` do protótipo: `this` precisa ser objeto (`isObject`) e da classe certa.
pub(crate) fn get_weak_receiver<T>(
    this_value: JSValue,
    downcast: impl FnOnce(&JSValue) -> Option<T>,
    not_an_object: &'static str,
    wrong_receiver: &'static str,
) -> Result<T, CollectionError> {
    if JSObject::from_value(&this_value).is_none() {
        return Err(CollectionError::TypeError(not_an_object));
    }
    downcast(&this_value).ok_or(CollectionError::TypeError(wrong_receiver))
}

/// `getWeakMap(globalObject, value)`.
pub fn get_weak_map(this_value: JSValue) -> Result<JSWeakMapRef, CollectionError> {
    get_weak_receiver(this_value, JSWeakMap::from_value, WEAK_MAP_NOT_AN_OBJECT_MESSAGE, WEAK_MAP_WRONG_RECEIVER_MESSAGE)
}

/// `protoFuncWeakMapDelete`: chave que não é célula dá `false`.
pub fn weak_map_proto_delete(this_value: JSValue, key: JSValue) -> Result<JSValue, CollectionError> {
    let map = get_weak_map(this_value)?;
    if !key.is_cell() {
        return Ok(JSValue::Bool(false));
    }
    Ok(JSValue::Bool(map.table().borrow_mut().remove(key)))
}

/// `protoFuncWeakMapGet`: chave que não é célula dá `undefined`.
pub fn weak_map_proto_get(this_value: JSValue, key: JSValue) -> Result<JSValue, CollectionError> {
    let map = get_weak_map(this_value)?;
    if !key.is_cell() {
        return Ok(JSValue::Undefined);
    }
    Ok(map.table().borrow().get(key))
}

/// `protoFuncWeakMapHas`: chave que não é célula dá `false`.
pub fn weak_map_proto_has(this_value: JSValue, key: JSValue) -> Result<JSValue, CollectionError> {
    let map = get_weak_map(this_value)?;
    if !key.is_cell() {
        return Ok(JSValue::Bool(false));
    }
    Ok(JSValue::Bool(map.table().borrow().has(key)))
}

/// `protoFuncWeakMapSet`: devolve o próprio `this`.
pub fn weak_map_proto_set(this_value: JSValue, key: JSValue, value: JSValue) -> Result<JSValue, CollectionError> {
    let map = get_weak_map(this_value)?;
    if !can_be_held_weakly(key) {
        return Err(CollectionError::TypeError(WEAK_MAP_INVALID_KEY_ERROR));
    }
    map.table().borrow_mut().add(key, value);
    Ok(this_value)
}

/// `protoFuncWeakMapGetOrInsert`.
pub fn weak_map_proto_get_or_insert(this_value: JSValue, key: JSValue, value: JSValue) -> Result<JSValue, CollectionError> {
    let map = get_weak_map(this_value)?;
    if !can_be_held_weakly(key) {
        return Err(CollectionError::TypeError(WEAK_MAP_INVALID_KEY_ERROR));
    }
    let mut table = map.table().borrow_mut();
    if let Some(existing) = table.get_entry(key) {
        return Ok(existing);
    }
    table.add(key, value);
    Ok(value)
}

/// `protoFuncWeakMapGetOrInsertComputed`: a conferência de `callData` do `valueCallback` é do chamador
/// (`WEAK_MAP_GET_OR_INSERT_COMPUTED_NOT_CALLABLE_MESSAGE`), que a faz depois da conferência da chave,
/// como o C++; esta função recebe o callback já resolvido. O callback roda sem a tabela emprestada, e
/// o `add` depois dele troca o valor se ele inseriu a chave.
pub fn weak_map_proto_get_or_insert_computed<E: From<CollectionError>>(
    this_value: JSValue,
    key: JSValue,
    callback: impl FnOnce(JSValue) -> Result<JSValue, E>,
) -> Result<JSValue, E> {
    let map = get_weak_map(this_value)?;
    if !can_be_held_weakly(key) {
        return Err(CollectionError::TypeError(WEAK_MAP_INVALID_KEY_ERROR).into());
    }
    if let Some(existing) = map.table().borrow().get_entry(key) {
        return Ok(existing);
    }
    let value = callback(key)?;
    map.table().borrow_mut().add(key, value);
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::vm::VM;

    fn new_weak_map(vm: &VM) -> JSWeakMapRef {
        JSWeakMap::create(vm, &JSWeakMap::create_structure(vm, None, JSValue::Null))
    }

    #[test]
    fn set_get_has_delete_by_identity() {
        let vm = VM::new();
        let map = new_weak_map(&vm);
        let this = map.as_value();
        let key = new_weak_map(&vm).as_value();
        let other = new_weak_map(&vm).as_value();
        assert_eq!(weak_map_proto_set(this, key, JSValue::Int32(1)).unwrap(), this);
        assert_eq!(weak_map_proto_get(this, key).unwrap(), JSValue::Int32(1));
        assert_eq!(weak_map_proto_has(this, other).unwrap(), JSValue::Bool(false));
        assert_eq!(weak_map_proto_delete(this, key).unwrap(), JSValue::Bool(true));
        assert_eq!(weak_map_proto_has(this, key).unwrap(), JSValue::Bool(false));
    }

    #[test]
    fn invalid_keys_and_receivers() {
        let vm = VM::new();
        let map = new_weak_map(&vm);
        let this = map.as_value();
        assert_eq!(
            weak_map_proto_set(this, JSValue::Int32(1), JSValue::Null),
            Err(CollectionError::TypeError(WEAK_MAP_INVALID_KEY_ERROR))
        );
        assert_eq!(weak_map_proto_get(this, JSValue::Int32(1)).unwrap(), JSValue::Undefined);
        assert_eq!(weak_map_proto_has(this, JSValue::Undefined).unwrap(), JSValue::Bool(false));
        assert_eq!(
            weak_map_proto_has(JSValue::Int32(1), this),
            Err(CollectionError::TypeError(WEAK_MAP_NOT_AN_OBJECT_MESSAGE))
        );
        let set = crate::runtime::js_set::JSSet::create(
            &vm,
            &crate::runtime::js_set::JSSet::create_structure(&vm, None, JSValue::Null),
        );
        assert_eq!(
            weak_map_proto_has(set.as_value(), this),
            Err(CollectionError::TypeError(WEAK_MAP_WRONG_RECEIVER_MESSAGE))
        );
    }

    #[test]
    fn symbols_as_keys_unless_registered() {
        let vm = VM::new();
        let map = new_weak_map(&vm);
        let this = map.as_value();
        let symbol = JSValue::from_cell(Symbol::create(&vm).cell_id());
        assert!(can_be_held_weakly(symbol));
        weak_map_proto_set(this, symbol, JSValue::Int32(4)).unwrap();
        assert_eq!(weak_map_proto_get(this, symbol).unwrap(), JSValue::Int32(4));
        assert!(!can_be_held_weakly(JSValue::Null));
    }

    #[test]
    fn get_or_insert_family() {
        let vm = VM::new();
        let map = new_weak_map(&vm);
        let this = map.as_value();
        let key = new_weak_map(&vm).as_value();
        assert_eq!(weak_map_proto_get_or_insert(this, key, JSValue::Int32(1)).unwrap(), JSValue::Int32(1));
        assert_eq!(weak_map_proto_get_or_insert(this, key, JSValue::Int32(2)).unwrap(), JSValue::Int32(1));
        let other = new_weak_map(&vm).as_value();
        let computed = weak_map_proto_get_or_insert_computed::<CollectionError>(this, other, |_| Ok(JSValue::Int32(8))).unwrap();
        assert_eq!(computed, JSValue::Int32(8));
        assert_eq!(weak_map_proto_get(this, other).unwrap(), JSValue::Int32(8));
        assert_eq!(
            weak_map_proto_get_or_insert_computed::<CollectionError>(this, JSValue::Int32(1), |_| Ok(JSValue::Null)),
            Err(CollectionError::TypeError(WEAK_MAP_INVALID_KEY_ERROR))
        );
    }
}
