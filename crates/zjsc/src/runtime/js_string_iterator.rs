//! Porte de `runtime/JSStringIterator.{h,cpp}` e `JSStringIteratorInlines.h`: a célula do iterador de
//! `String.prototype[Symbol.iterator]` (`JSInternalFieldObjectImpl<2>`).
//!
//! DIVERGÊNCIAS:
//! - `JSInternalFieldObjectImpl<2>` é um `JSNonFinalObject` com um `RefCell<[JSValue; 2]>`, como em
//!   `js_array_iterator.rs` (sem GC não há `WriteBarrier`).
//! - `advance` e `nextWithAdvance` usam o `next_code_point` de `string_prototype.rs`, o mesmo passo do
//!   `JSStringIterator::advance` (par substituto inteiro, unidade solta sozinha), em vez de
//!   `jsSubstring`/`jsSingleCharacterString`, e não têm exceção possível (sem rope não há resolução
//!   que falhe).

use std::cell::RefCell;
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::{js_string, JSStringRef};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::string_prototype::next_code_point;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `JSStringIterator::doneIndex`.
pub const DONE_INDEX: i32 = -1;

/// `enum class Field`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Index = 0,
    IteratedString = 1,
}

/// `const ClassInfo JSStringIterator::s_info`.
pub static JS_STRING_ITERATOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "String Iterator", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class JSStringIterator final : public JSInternalFieldObjectImpl<2>`.
pub struct JSStringIterator {
    base: JSNonFinalObject,
    fields: RefCell<[JSValue; 2]>,
}

/// A referência à célula, o `*` do C++.
pub type JSStringIteratorRef = Rc<JSStringIterator>;

impl std::ops::Deref for JSStringIterator {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

crate::runtime::js_internal_field_object_impl::impl_internal_fields_for_ref_cell!(JSStringIterator);

impl JSStringIterator {
    /// `JSStringIterator(vm, structure)` + `finishCreation(vm)` (`initialValues()`: `{ 0, null }`) e o
    /// registro da célula.
    fn allocate(vm: &VM, structure: &StructureRef) -> JSStringIteratorRef {
        let cell_id = cell_registry::reserve();
        let cell = Rc::new(JSStringIterator {
            base: JSNonFinalObject::new(vm, Rc::clone(structure)),
            fields: RefCell::new([JSValue::Int32(0), JSValue::Null]),
        });
        cell.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::StringIterator(Rc::clone(&cell)));
        cell
    }

    /// `create(vm, structure, iteratedString)`.
    pub fn create(vm: &VM, structure: &StructureRef, iterated_string: &JSStringRef) -> JSStringIteratorRef {
        let iterator = JSStringIterator::allocate(vm, structure);
        iterator.set_internal_field(Field::Index, JSValue::Int32(0));
        iterator.set_internal_field(Field::IteratedString, JSValue::from_js_string(Rc::clone(iterated_string)));
        iterator
    }

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::JSStringIteratorType, JSNonFinalObject::STRUCTURE_FLAGS),
            &JS_STRING_ITERATOR_S_INFO,
        )
    }

    /// `dynamicDowncast<JSStringIterator>(value)`.
    pub fn from_value(value: &JSValue) -> Option<JSStringIteratorRef> {
        match value {
            JSValue::Cell(cell_id) => match cell_registry::get(*cell_id) {
                Some(CellEntry::StringIterator(cell)) => Some(cell),
                _ => None,
            },
            _ => None,
        }
    }

    /// `internalField(field).get()`.
    pub fn internal_field(&self, field: Field) -> JSValue {
        self.fields.borrow()[field as usize]
    }

    /// `internalField(field).set(vm, this, value)`.
    fn set_internal_field(&self, field: Field, value: JSValue) {
        self.fields.borrow_mut()[field as usize] = value;
    }

    /// `nextWithAdvance(globalObject, vm)`: o próximo ponto de código como string, ou `None` ao esgotar
    /// (o índice vira `doneIndex`).
    pub fn next_with_advance(&self, vm: &VM) -> Option<JSStringRef> {
        let position = self.internal_field(Field::Index).as_int32();
        let iterated = self.internal_field(Field::IteratedString).as_js_string();
        // `static_cast<unsigned>(position) >= length` também pega `doneIndex`.
        let step = if position < 0 { None } else { next_code_point(&iterated.value(), position as u32) };
        match step {
            Some((value, next_position)) => {
                self.set_internal_field(Field::Index, JSValue::Int32(next_position as i32));
                Some(js_string(vm, &value))
            }
            None => {
                self.set_internal_field(Field::Index, JSValue::Int32(DONE_INDEX));
                None
            }
        }
    }
}
