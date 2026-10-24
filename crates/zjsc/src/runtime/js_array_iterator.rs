//! Porte de `runtime/JSArrayIterator.{h,cpp}` e `JSArrayIteratorInlines.h`: a célula do iterador de
//! `Array.prototype.values/keys/entries` (`JSInternalFieldObjectImpl<3>`).
//!
//! DIVERGÊNCIAS:
//! - `JSInternalFieldObjectImpl<3>` é um `JSNonFinalObject` com um `RefCell<[JSValue; 3]>`; o C++ grava
//!   os campos com `WriteBarrier`, aqui não há GC. `Field` indexa o vetor, como no C++ (os intrínsecos
//!   `getArrayIteratorInternalField`/`putArrayIteratorInternalField` do `ArrayIteratorPrototype.js` leem e
//!   gravam por esse índice).
//! - `create` recebe só o `IterationKind` (o C++ tem a sobrecarga com `JSValue kind` e a com
//!   `IterationKind`, a segunda só repassa a primeira); o `kind` fica guardado como `jsNumber(kind)`.
//! - `next` não tem `JSGlobalObject`: o elemento sai de `JSArray::get_by_index` (que cai em
//!   `JSObject::get(index)` no buraco, como o `getIndex`) e o par de `entries` usa a `Structure` de
//!   `ArrayWithContiguous` que o chamador passa (`constructArrayPair`; o global do porte ainda só guarda a
//!   de `ArrayWithUndecided`).

use std::cell::RefCell;
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::iteration_kind::IterationKind;
use crate::runtime::js_array::{construct_array_pair, JSArray};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `JSInternalFieldObjectImpl<3>`.
pub const NUMBER_OF_INTERNAL_FIELDS: u32 = 3;

/// `JSArrayIterator::doneIndex`.
pub const DONE_INDEX: i64 = -1;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Index = 0,
    IteratedObject = 1,
    Kind = 2,
}

/// `const ClassInfo JSArrayIterator::s_info`.
pub static JS_ARRAY_ITERATOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "ArrayIterator", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class JSArrayIterator final : public JSInternalFieldObjectImpl<3>`.
pub struct JSArrayIterator {
    base: JSNonFinalObject,
    fields: RefCell<[JSValue; NUMBER_OF_INTERNAL_FIELDS as usize]>,
}

/// A referência à célula, o `*` do C++.
pub type JSArrayIteratorRef = Rc<JSArrayIterator>;

impl std::ops::Deref for JSArrayIterator {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

crate::runtime::js_internal_field_object_impl::impl_internal_fields_for_ref_cell!(JSArrayIterator);

impl JSArrayIterator {
    /// `initialValues()`: `{ jsNumber(0), jsNull(), jsNumber(0) }`.
    fn initial_values() -> [JSValue; NUMBER_OF_INTERNAL_FIELDS as usize] {
        [JSValue::Int32(0), JSValue::Null, JSValue::Int32(0)]
    }

    /// `JSArrayIterator(vm, structure)` + `finishCreation(vm)` (os campos com os valores iniciais) e o
    /// registro da célula.
    fn allocate(vm: &VM, structure: &StructureRef) -> JSArrayIteratorRef {
        let cell_id = cell_registry::reserve();
        let cell = Rc::new(JSArrayIterator {
            base: JSNonFinalObject::new(vm, Rc::clone(structure)),
            fields: RefCell::new(JSArrayIterator::initial_values()),
        });
        cell.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::ArrayIterator(Rc::clone(&cell)));
        cell
    }

    /// `create(vm, structure, iteratedObject, kind)`.
    pub fn create(vm: &VM, structure: &StructureRef, iterated_object: &JSObject, kind: IterationKind) -> JSArrayIteratorRef {
        let iterator = JSArrayIterator::allocate(vm, structure);
        iterator.set_internal_field(Field::IteratedObject, iterated_object.as_value());
        iterator.set_internal_field(Field::Kind, JSValue::from_u32(kind as u32));
        iterator
    }

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::JSArrayIteratorType, JSNonFinalObject::STRUCTURE_FLAGS),
            &JS_ARRAY_ITERATOR_S_INFO,
        )
    }

    /// Procura a célula pelo `cell_id` guardado em `JSValue::Cell`.
    pub fn from_cell_id(cell_id: usize) -> Option<JSArrayIteratorRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::ArrayIterator(cell)) => Some(cell),
            _ => None,
        }
    }

    /// `dynamicDowncast<JSArrayIterator>(value)`.
    pub fn from_value(value: &JSValue) -> Option<JSArrayIteratorRef> {
        match value {
            JSValue::Cell(cell_id) => JSArrayIterator::from_cell_id(*cell_id),
            _ => None,
        }
    }

    /// `internalField(field).get()`.
    pub fn internal_field(&self, field: Field) -> JSValue {
        self.fields.borrow()[field as usize]
    }

    /// `internalField(field).set(vm, this, value)`.
    pub fn set_internal_field(&self, field: Field, value: JSValue) {
        self.fields.borrow_mut()[field as usize] = value;
    }

    /// `kind()`.
    pub fn kind(&self) -> IterationKind {
        match self.internal_field(Field::Kind).as_uint32() {
            0 => IterationKind::Keys,
            1 => IterationKind::Values,
            2 => IterationKind::Entries,
            other => unreachable!("JSArrayIterator com kind {other}"),
        }
    }

    /// `iteratedObject()`.
    pub fn iterated_object(&self) -> JSValue {
        self.internal_field(Field::IteratedObject)
    }

    /// `index()`: `asAnyInt()` do campo.
    pub fn index(&self) -> i64 {
        let value = self.internal_field(Field::Index);
        if value.is_int32() {
            i64::from(value.as_int32())
        } else {
            value.as_double() as i64
        }
    }

    /// `setIndex(index)`: `jsNumber(index)`.
    pub fn set_index(&self, index: i64) {
        self.set_internal_field(Field::Index, JSValue::from_double(index as f64));
    }

    /// `nextWithAdvance()`: o índice a ler do array iterado, já avançando, ou `None` se esgotou. Só vale
    /// quando o objeto iterado é um `JSArray`.
    pub fn next_with_advance(&self) -> Option<u32> {
        // `downcast<JSArray>(iteratedObject())`: por classe (o `Array.prototype` é `DerivedArrayType`).
        let array = JSArray::from_value_by_class(&self.iterated_object()).expect("JSArrayIterator::nextWithAdvance sem JSArray");
        let index = self.index();
        debug_assert!(index == DONE_INDEX || index >= 0);
        if index == DONE_INDEX || index >= i64::from(array.length()) {
            self.set_index(DONE_INDEX);
            return None;
        }
        self.set_index(index + 1);
        Some(index as u32)
    }

    /// `next(globalObject, value)`: o valor do passo, ou `None` ao terminar ou com exceção pendente no `VM`
    /// (o chamador confere `vm.has_exception()`, como o `RETURN_IF_EXCEPTION` do C++; ver DIVERGÊNCIAS sobre
    /// `pair_structure`).
    pub fn next(&self, vm: &VM, pair_structure: &StructureRef) -> Option<JSValue> {
        let index = self.next_with_advance()?;
        let kind = self.kind();
        if kind == IterationKind::Keys {
            return Some(JSValue::from_u32(index));
        }
        let array = JSArray::from_value_by_class(&self.iterated_object()).expect("JSArrayIterator::next sem JSArray");
        let element = array.get_by_index(vm, index);
        // `RETURN_IF_EXCEPTION(scope, false)`: a exceção do getter indexado fica no `VM` e o chamador a confere.
        if vm.has_exception() {
            return None;
        }
        if kind == IterationKind::Values {
            return Some(element);
        }
        Some(construct_array_pair(vm, pair_structure, JSValue::from_u32(index), element).as_value())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::indexing_type::ARRAY_WITH_CONTIGUOUS;
    use crate::runtime::js_array::construct_array;

    fn array(vm: &VM, values: &[JSValue]) -> JSArray {
        construct_array(vm, &JSArray::create_structure(vm, None, JSValue::Null, ARRAY_WITH_CONTIGUOUS), values)
    }

    #[test]
    fn values_keys_and_entries_walk_the_array() {
        let vm = VM::new();
        let pair_structure = JSArray::create_structure(&vm, None, JSValue::Null, ARRAY_WITH_CONTIGUOUS);
        let structure = JSArrayIterator::create_structure(&vm, None, JSValue::Null);
        let target = array(&vm, &[JSValue::Int32(10), JSValue::Int32(20)]);

        let values = JSArrayIterator::create(&vm, &structure, &target, IterationKind::Values);
        assert_eq!(values.next(&vm, &pair_structure), Some(JSValue::Int32(10)));
        assert_eq!(values.next(&vm, &pair_structure), Some(JSValue::Int32(20)));
        assert_eq!(values.next(&vm, &pair_structure), None);
        assert_eq!(values.index(), DONE_INDEX);
        assert_eq!(values.next(&vm, &pair_structure), None);

        let keys = JSArrayIterator::create(&vm, &structure, &target, IterationKind::Keys);
        assert_eq!(keys.next(&vm, &pair_structure), Some(JSValue::Int32(0)));
        assert_eq!(keys.next(&vm, &pair_structure), Some(JSValue::Int32(1)));
        assert_eq!(keys.next(&vm, &pair_structure), None);

        let entries = JSArrayIterator::create(&vm, &structure, &target, IterationKind::Entries);
        let pair = JSArray::from_value(&entries.next(&vm, &pair_structure).unwrap()).unwrap();
        assert_eq!(pair.length(), 2);
        assert_eq!(pair.get_by_index(&vm, 0), JSValue::Int32(0));
        assert_eq!(pair.get_by_index(&vm, 1), JSValue::Int32(10));
    }

    #[test]
    fn initial_values_and_type() {
        let vm = VM::new();
        let structure = JSArrayIterator::create_structure(&vm, None, JSValue::Null);
        let iterator = JSArrayIterator::allocate(&vm, &structure);
        assert_eq!(iterator.index(), 0);
        assert_eq!(iterator.internal_field(Field::IteratedObject), JSValue::Null);
        assert_eq!(iterator.kind(), IterationKind::Keys);
        assert_eq!(cell_registry::cell_type(iterator.cell_id()), Some(JSType::JSArrayIteratorType));
        assert!(JSArrayIterator::from_value(&iterator.as_value()).is_some());
    }
}
