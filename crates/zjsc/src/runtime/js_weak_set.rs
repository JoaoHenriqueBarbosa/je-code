//! Porte de `runtime/JSWeakSet.{h,cpp}` e dos algoritmos de `WeakSetPrototype.cpp`, como funções
//! puras sobre `JSValue`. O armazenamento e a divergência de GC são os de `js_weak_map.rs` (a tabela
//! guarda o `cell_id` da chave, o valor é `empty`).
//!
//! Fora desta fatia: `WeakSetConstructor` (itera o argumento e chama o `add` do protótipo).

use crate::runtime::js_ordered_hash_table::{define_collection_cell, CollectionError};
use crate::runtime::js_value::JSValue;
use crate::runtime::js_weak_map::{can_be_held_weakly, get_weak_receiver};

define_collection_cell!(JSWeakSet, JSWeakSetRef, WeakSet, JSWeakSetType, JS_WEAK_SET_S_INFO, "WeakSet");

pub const WEAK_SET_INVALID_VALUE_ERROR: &str = "WeakSet values must be objects or non-registered symbols";
pub const WEAK_SET_NOT_AN_OBJECT_MESSAGE: &str = "Called WeakSet function on non-object";
pub const WEAK_SET_WRONG_RECEIVER_MESSAGE: &str = "Called WeakSet function on a non-WeakSet object";

/// `getWeakSet(globalObject, value)`.
pub fn get_weak_set(this_value: JSValue) -> Result<JSWeakSetRef, CollectionError> {
    get_weak_receiver(this_value, JSWeakSet::from_value, WEAK_SET_NOT_AN_OBJECT_MESSAGE, WEAK_SET_WRONG_RECEIVER_MESSAGE)
}

/// `protoFuncWeakSetDelete`: valor que não é célula dá `false`.
pub fn weak_set_proto_delete(this_value: JSValue, key: JSValue) -> Result<JSValue, CollectionError> {
    let set = get_weak_set(this_value)?;
    if !key.is_cell() {
        return Ok(JSValue::Bool(false));
    }
    Ok(JSValue::Bool(set.table().borrow_mut().remove(key)))
}

/// `protoFuncWeakSetHas`: valor que não é célula dá `false`.
pub fn weak_set_proto_has(this_value: JSValue, key: JSValue) -> Result<JSValue, CollectionError> {
    let set = get_weak_set(this_value)?;
    if !key.is_cell() {
        return Ok(JSValue::Bool(false));
    }
    Ok(JSValue::Bool(set.table().borrow().has(key)))
}

/// `protoFuncWeakSetAdd`: devolve o próprio `this`.
pub fn weak_set_proto_add(this_value: JSValue, key: JSValue) -> Result<JSValue, CollectionError> {
    let set = get_weak_set(this_value)?;
    if !can_be_held_weakly(key) {
        return Err(CollectionError::TypeError(WEAK_SET_INVALID_VALUE_ERROR));
    }
    set.table().borrow_mut().add(key, JSValue::Empty);
    Ok(this_value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::vm::VM;

    #[test]
    fn add_has_delete() {
        let vm = VM::new();
        let set = JSWeakSet::create(&vm, &JSWeakSet::create_structure(&vm, None, JSValue::Null));
        let this = set.as_value();
        let key = JSWeakSet::create(&vm, &JSWeakSet::create_structure(&vm, None, JSValue::Null)).as_value();
        assert_eq!(weak_set_proto_add(this, key).unwrap(), this);
        assert_eq!(weak_set_proto_has(this, key).unwrap(), JSValue::Bool(true));
        assert_eq!(weak_set_proto_delete(this, key).unwrap(), JSValue::Bool(true));
        assert_eq!(weak_set_proto_has(this, key).unwrap(), JSValue::Bool(false));
        assert_eq!(weak_set_proto_has(this, JSValue::Int32(3)).unwrap(), JSValue::Bool(false));
        assert_eq!(
            weak_set_proto_add(this, JSValue::Int32(3)),
            Err(CollectionError::TypeError(WEAK_SET_INVALID_VALUE_ERROR))
        );
        assert_eq!(
            weak_set_proto_add(JSValue::Undefined, key),
            Err(CollectionError::TypeError(WEAK_SET_NOT_AN_OBJECT_MESSAGE))
        );
    }
}
