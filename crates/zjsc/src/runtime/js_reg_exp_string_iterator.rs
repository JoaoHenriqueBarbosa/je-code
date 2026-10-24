//! Porte de `runtime/JSRegExpStringIterator.{h,cpp}` e `JSRegExpStringIteratorInlines.h`: a célula do
//! iterador de `String.prototype.matchAll` e de `RegExp.prototype[Symbol.matchAll]`
//! (`JSInternalFieldObjectImpl<3>`: `RegExp`, `String` e as flags `Global`, `FullUnicode` e `Done`).
//!
//! DIVERGÊNCIAS:
//! - Os campos são um `RefCell<[JSValue; 3]>`, como em `js_array_iterator.rs`.
//! - `nextImpl` só tem o caminho genérico (`regExpExec` e `ToString(Get(match, "0"))` para detectar o
//!   casamento vazio): o atalho por `execInline` depende de `regExpExecWatchpointIsValid`, que não
//!   existe, e dá o mesmo resultado observável quando o `exec` não foi redefinido.
//! - `regExpStringIteratorPrivateFuncCreate` (o `LinkTimeConstant::regExpStringIteratorCreate`) fica de
//!   fora: só os builtins `RegExpPrototype.js`, que esta versão do JSC não tem, o chamam.

use std::cell::RefCell;
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::Thrown;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::reg_exp_prototype_natives::reg_exp_exec;
use crate::runtime::string_prototype::code_units;
use crate::runtime::string_regexp_support::{
    advance_string_index, create_iterator_result_object, get_object_index, get_object_property, set_object_property,
    to_string_value,
};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `enum class Field`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    RegExp = 0,
    String = 1,
    Flags = 2,
}

/// `enum class FlagBit`.
const GLOBAL: u8 = 1 << 0;
const FULL_UNICODE: u8 = 1 << 1;
const DONE: u8 = 1 << 2;

/// `const ClassInfo JSRegExpStringIterator::s_info`.
pub static JS_REG_EXP_STRING_ITERATOR_S_INFO: ClassInfo = ClassInfo {
    class_name: "RegExpStringIterator",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: None, inherits_js_type_range: None,
};

/// `class JSRegExpStringIterator final : public JSInternalFieldObjectImpl<3>`.
pub struct JSRegExpStringIterator {
    base: JSNonFinalObject,
    fields: RefCell<[JSValue; 3]>,
}

/// A referência à célula, o `*` do C++.
pub type JSRegExpStringIteratorRef = Rc<JSRegExpStringIterator>;

impl std::ops::Deref for JSRegExpStringIterator {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

crate::runtime::js_internal_field_object_impl::impl_internal_fields_for_ref_cell!(JSRegExpStringIterator);

impl JSRegExpStringIterator {
    /// `createWithInitialValues(vm, structure)` (`initialValues()`: `{ null, null, 0 }`) e o registro da
    /// célula.
    pub fn create_with_initial_values(vm: &VM, structure: &StructureRef) -> JSRegExpStringIteratorRef {
        let cell_id = cell_registry::reserve();
        let cell = Rc::new(JSRegExpStringIterator {
            base: JSNonFinalObject::new(vm, Rc::clone(structure)),
            fields: RefCell::new([JSValue::Null, JSValue::Null, JSValue::Int32(0)]),
        });
        cell.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::RegExpStringIterator(Rc::clone(&cell)));
        cell
    }

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::JSRegExpStringIteratorType, JSNonFinalObject::STRUCTURE_FLAGS),
            &JS_REG_EXP_STRING_ITERATOR_S_INFO,
        )
    }

    /// `dynamicDowncast<JSRegExpStringIterator>(value)`.
    pub fn from_value(value: &JSValue) -> Option<JSRegExpStringIteratorRef> {
        match value {
            JSValue::Cell(cell_id) => match cell_registry::get(*cell_id) {
                Some(CellEntry::RegExpStringIterator(cell)) => Some(cell),
                _ => None,
            },
            _ => None,
        }
    }

    fn internal_field(&self, field: Field) -> JSValue {
        self.fields.borrow()[field as usize]
    }

    fn set_internal_field(&self, field: Field, value: JSValue) {
        self.fields.borrow_mut()[field as usize] = value;
    }

    fn flags(&self) -> u8 {
        self.internal_field(Field::Flags).as_int32() as u8
    }

    /// `setRegExp(vm, regExp)`.
    pub fn set_reg_exp(&self, reg_exp: JSValue) {
        self.set_internal_field(Field::RegExp, reg_exp);
    }

    /// `setString(vm, string)`.
    pub fn set_string(&self, string: JSValue) {
        self.set_internal_field(Field::String, string);
    }

    /// `setFlags(global, fullUnicode, done)`.
    pub fn set_flags(&self, global: bool, full_unicode: bool, done: bool) {
        let flags = (if global { GLOBAL } else { 0 }) | (if full_unicode { FULL_UNICODE } else { 0 }) | (if done { DONE } else { 0 });
        self.set_internal_field(Field::Flags, JSValue::Int32(i32::from(flags)));
    }

    /// `setDone(done)`.
    fn set_done(&self) {
        self.set_internal_field(Field::Flags, JSValue::Int32(i32::from(self.flags() | DONE)));
    }

    /// `isDone()`.
    fn is_done(&self) -> bool {
        self.flags() & DONE != 0
    }

    /// `nextImpl(globalObject)`: o resultado de `RegExpExec` (o `null` encerra).
    fn next_impl(&self, global_object: &JSGlobalObject) -> Result<JSValue, Thrown> {
        let vm = global_object.vm();
        let reg_exp = self.internal_field(Field::RegExp);
        let string = self.internal_field(Field::String).as_js_string();
        let flags = self.flags();
        let global = flags & GLOBAL != 0;
        let full_unicode = flags & FULL_UNICODE != 0;

        // 9. Let match be ? RegExpExec(R, S).
        let matched = reg_exp_exec(global_object, reg_exp, &string)?;

        // 11.a.i. `ToString(Get(match, "0"))` só é observado para detectar o casamento vazio.
        let mut is_empty_match = false;
        if !matched.is_null() && global {
            let match_value = get_object_index(global_object, matched, 0)?;
            is_empty_match = to_string_value(global_object, match_value)?.length() == 0;
        }

        // 10. If match is null, set O.[[Done]] to true and return.
        if matched.is_null() {
            self.set_done();
            return Ok(JSValue::Null);
        }

        // 11.b. If global is false, set O.[[Done]] to true.
        if !global {
            self.set_done();
            return Ok(matched);
        }

        // 11.a.ii. If matchStr is the empty String, advance R's lastIndex past it.
        if is_empty_match {
            let last_index_value = get_object_property(global_object, reg_exp, &vm.property_names.last_index)?;
            let this_index = last_index_value.to_length_checked()?;
            let units = code_units(&string.value()).into_owned();
            let next_index = advance_string_index(&units, this_index, full_unicode);
            set_object_property(global_object, reg_exp, &vm.property_names.last_index, js_number(next_index as f64))?;
        }
        Ok(matched)
    }

    /// `next(globalObject)`: o objeto de resultado do iterador.
    pub fn next(&self, global_object: &JSGlobalObject) -> Result<JSValue, Thrown> {
        // 4. If O.[[Done]] is true, return CreateIteratorResultObject(undefined, true).
        if self.is_done() {
            return Ok(create_iterator_result_object(global_object, JSValue::undefined(), true));
        }
        let matched = self.next_impl(global_object)?;
        if matched.is_null() {
            return Ok(create_iterator_result_object(global_object, JSValue::undefined(), true));
        }
        Ok(create_iterator_result_object(global_object, matched, false))
    }
}
