//! Porte de `runtime/JSSet.{h,cpp}`, `JSSetIterator.{h,cpp}` e dos algoritmos de `SetPrototype.cpp` /
//! `SetIteratorPrototype.cpp`, como funções puras sobre `JSValue` (mesmo desenho de `js_map.rs`).
//!
//! No `Set` o valor de cada entrada é `empty` (`JSOrderedHashSet`, `SetTraits`), e a chave é
//! normalizada por `normalizeMapKey` (SameValueZero).
//!
//! Fora desta fatia, e por quê: `SetConstructor` (itera o argumento e chama o `add` do protótipo, ver
//! `set_constructor.rs`); `isAddFastAndNonObservable`/`isIteratorProtocolFastAndNonObservable`
//! (watchpoints do `JSGlobalObject`); `Set.prototype.forEach` é JS embutido, o laço vivo está em
//! `set_proto_for_each`. Os métodos de conjunto (`union`...) estão em `set_prototype.rs`.

use std::ops::ControlFlow;

use crate::runtime::iteration_kind::IterationKind;
use crate::runtime::js_ordered_hash_table::{
    define_collection_cell, define_collection_iterator, CollectionError, IteratorStep,
};
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;

define_collection_cell!(JSSet, JSSetRef, Set, JSSetType, JS_SET_S_INFO, "Set");
define_collection_iterator!(
    JSSetIterator,
    JSSetIteratorRef,
    SetIterator,
    JSSetIteratorType,
    JS_SET_ITERATOR_S_INFO,
    "Set Iterator",
    JSSet,
    true
);

pub const SET_NOT_A_SET_MESSAGE: &str = "Set operation called on non-Set object";
pub const SET_ITERATOR_NOT_AN_ITERATOR_MESSAGE: &str =
    "%SetIteratorPrototype%.next requires that |this| be a Set Iterator instance";

impl JSSet {
    /// `JSSet::clone(globalObject, vm, structure)`: as entradas vivas numa célula nova.
    pub fn clone_with_structure(&self, vm: &VM, structure: &StructureRef) -> JSSetRef {
        let copy = JSSet::create(vm, structure);
        *copy.table().borrow_mut() = self.table().borrow().copy_entries();
        copy
    }

    /// `JSSet::add(globalObject, key)`.
    pub fn add(&self, key: JSValue) {
        self.table().borrow_mut().add(key, JSValue::Empty);
    }

    /// `forEachInSetStorage(vm, globalObject, storage, 0, callback)`: laço vivo sobre as chaves, que
    /// aguenta o `visit` mexer no conjunto (o empréstimo da tabela termina antes dele). `Break` encerra
    /// (`IterationStatus::Done`) e o primeiro erro também.
    pub fn for_each_key<E>(&self, mut visit: impl FnMut(JSValue) -> Result<ControlFlow<()>, E>) -> Result<(), E> {
        let cursor = self.table().borrow_mut().new_cursor();
        loop {
            let next = self.table().borrow().next_entry(&cursor);
            let Some((key, _)) = next else { return Ok(()) };
            if visit(key)?.is_break() {
                return Ok(());
            }
        }
    }
}

/// `getSet(globalObject, thisValue)`.
pub fn get_set(this_value: JSValue) -> Result<JSSetRef, CollectionError> {
    if !this_value.is_cell() {
        return Err(CollectionError::NotAnObject(this_value));
    }
    JSSet::from_value(&this_value).ok_or(CollectionError::TypeError(SET_NOT_A_SET_MESSAGE))
}

/// `setProtoFuncAdd`: devolve o próprio `this`.
pub fn set_proto_add(this_value: JSValue, key: JSValue) -> Result<JSValue, CollectionError> {
    get_set(this_value)?.add(key);
    Ok(this_value)
}

/// `setProtoFuncClear`.
pub fn set_proto_clear(this_value: JSValue) -> Result<JSValue, CollectionError> {
    get_set(this_value)?.table().borrow_mut().clear();
    Ok(JSValue::Undefined)
}

/// `setProtoFuncDelete`.
pub fn set_proto_delete(this_value: JSValue, key: JSValue) -> Result<JSValue, CollectionError> {
    Ok(JSValue::Bool(get_set(this_value)?.table().borrow_mut().remove(key)))
}

/// `setProtoFuncHas`.
pub fn set_proto_has(this_value: JSValue, key: JSValue) -> Result<JSValue, CollectionError> {
    Ok(JSValue::Bool(get_set(this_value)?.table().borrow().has(key)))
}

/// `setProtoFuncSize`.
pub fn set_proto_size(this_value: JSValue) -> Result<JSValue, CollectionError> {
    Ok(JSValue::from_u32(get_set(this_value)?.table().borrow().size()))
}

/// `Set.prototype.forEach(callback, thisArg)` (`builtins/SetPrototype.js`): laço vivo;
/// `callback(value, value, set)`. O primeiro erro do callback encerra o laço.
pub fn set_proto_for_each<E: From<CollectionError>>(
    this_value: JSValue,
    mut callback: impl FnMut(JSValue, JSValue, JSValue) -> Result<(), E>,
) -> Result<JSValue, E> {
    let set = get_set(this_value)?;
    set.for_each_key(|key| -> Result<ControlFlow<()>, E> {
        callback(key, key, this_value)?;
        Ok(ControlFlow::Continue(()))
    })?;
    Ok(JSValue::Undefined)
}

/// `createSetIteratorObject(globalObject, callFrame, kind)`: o iterador de `values`/`keys`/`entries`
/// (o `structure` é `globalObject->setIteratorStructure()`; `keys` e `values` são a mesma função).
pub fn create_set_iterator_object(
    vm: &VM,
    structure: &StructureRef,
    this_value: JSValue,
    kind: IterationKind,
) -> Result<JSValue, CollectionError> {
    let set = get_set(this_value)?;
    Ok(JSSetIterator::create(vm, structure, &set, kind).as_value())
}

/// `setIteratorProtoFuncNext`: o passo; o chamador monta o `createIteratorResultObject`.
pub fn set_iterator_proto_next(this_value: JSValue) -> Result<IteratorStep, CollectionError> {
    JSSetIterator::from_value(&this_value)
        .map(|iterator| iterator.next())
        .ok_or(CollectionError::TypeError(SET_ITERATOR_NOT_AN_ITERATOR_MESSAGE))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_set(vm: &VM) -> JSSetRef {
        JSSet::create(vm, &JSSet::create_structure(vm, None, JSValue::Null))
    }

    fn int(i: i32) -> JSValue {
        JSValue::Int32(i)
    }

    #[test]
    fn add_has_delete_size_with_normalized_keys() {
        let vm = VM::new();
        let set = new_set(&vm);
        let this = set.as_value();
        assert_eq!(set_proto_add(this, JSValue::Double(-0.0)).unwrap(), this);
        set_proto_add(this, int(0)).unwrap();
        set_proto_add(this, JSValue::nan()).unwrap();
        set_proto_add(this, JSValue::Double(f64::NAN)).unwrap();
        assert_eq!(set_proto_size(this).unwrap(), int(2));
        assert_eq!(set_proto_has(this, JSValue::Double(0.0)).unwrap(), JSValue::Bool(true));
        assert_eq!(set_proto_delete(this, JSValue::nan()).unwrap(), JSValue::Bool(true));
        assert_eq!(set_proto_delete(this, JSValue::nan()).unwrap(), JSValue::Bool(false));
        set_proto_clear(this).unwrap();
        assert_eq!(set_proto_size(this).unwrap(), int(0));
    }

    #[test]
    fn iteration_kinds_and_live_view() {
        let vm = VM::new();
        let set = new_set(&vm);
        let this = set.as_value();
        let structure = JSSetIterator::create_structure(&vm, None, JSValue::Null);
        set_proto_add(this, int(1)).unwrap();
        set_proto_add(this, int(2)).unwrap();
        let entries = create_set_iterator_object(&vm, &structure, this, IterationKind::Entries).unwrap();
        assert_eq!(set_iterator_proto_next(entries).unwrap(), IteratorStep::Entry(int(1), int(1)));
        set_proto_add(this, int(3)).unwrap();
        assert_eq!(set_iterator_proto_next(entries).unwrap(), IteratorStep::Entry(int(2), int(2)));
        assert_eq!(set_iterator_proto_next(entries).unwrap(), IteratorStep::Entry(int(3), int(3)));
        assert_eq!(set_iterator_proto_next(entries).unwrap(), IteratorStep::Done);
        let values = create_set_iterator_object(&vm, &structure, this, IterationKind::Values).unwrap();
        assert_eq!(set_iterator_proto_next(values).unwrap(), IteratorStep::Item(int(1)));
    }

    #[test]
    fn for_each_sees_additions_and_skips_deletions() {
        let vm = VM::new();
        let set = new_set(&vm);
        let this = set.as_value();
        for i in 0..3 {
            set_proto_add(this, int(i)).unwrap();
        }
        let mut seen = Vec::new();
        set_proto_for_each::<CollectionError>(this, |value, _, _| {
            seen.push(value);
            if value == int(0) {
                set_proto_delete(this, int(1))?;
                set_proto_add(this, int(9))?;
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(seen, vec![int(0), int(2), int(9)]);
    }

    #[test]
    fn for_each_key_breaks_and_clone_is_independent() {
        let vm = VM::new();
        let set = new_set(&vm);
        let this = set.as_value();
        for i in 0..4 {
            set_proto_add(this, int(i)).unwrap();
        }
        let mut seen = Vec::new();
        set.for_each_key::<CollectionError>(|key| {
            seen.push(key);
            Ok(if key == int(1) { ControlFlow::Break(()) } else { ControlFlow::Continue(()) })
        })
        .unwrap();
        assert_eq!(seen, vec![int(0), int(1)]);

        // `clone` copia as entradas vivas: depois dele os dois conjuntos andam sozinhos.
        let copy = set.clone_with_structure(&vm, &JSSet::create_structure(&vm, None, JSValue::Null));
        set_proto_delete(this, int(0)).unwrap();
        copy.add(JSValue::Double(-0.0));
        assert_eq!(copy.table().borrow().size(), 4);
        assert_eq!(set_proto_size(this).unwrap(), int(3));
        assert!(copy.table().borrow().has(int(0)));
    }

    #[test]
    fn wrong_receivers() {
        let vm = VM::new();
        let set = new_set(&vm);
        assert_eq!(set_proto_size(JSValue::Undefined), Err(CollectionError::NotAnObject(JSValue::Undefined)));
        assert_eq!(set_iterator_proto_next(set.as_value()), Err(CollectionError::TypeError(SET_ITERATOR_NOT_AN_ITERATOR_MESSAGE)));
        assert_eq!(
            crate::runtime::js_map::map_proto_size(set.as_value()),
            Err(CollectionError::TypeError(crate::runtime::js_map::MAP_NOT_A_MAP_MESSAGE))
        );
    }
}
