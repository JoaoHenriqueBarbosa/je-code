//! Porte de `runtime/StringObject.h` e `StringObject.cpp`: o objeto `String` (um `JSWrapperObject`
//! cujo valor interno é a `JSString`), registrado no `cell_registry` como `CellEntry::StringObject`.
//!
//! DIVERGÊNCIAS (mesmo padrão de `reg_exp_object.rs`):
//!
//! - `JSWrapperObject` (a base) não existe como tipo: o `m_internalValue` mora aqui. Como `StringObject`
//!   só guarda `JSString*`, o campo é um `JSStringRef` (o `internalValue()` do C++ devolve `JSString*`).
//!   `offsetOfInternalValue`, `allocationSize`, `subspaceFor` e `visitChildren` somem com o layout e o GC.
//! - `getOwnPropertySlot` é [`StringObject::get_own_property_slot`] (quem consulta despacha à mão, ver
//!   `own_descriptor` em `object_constructor.rs`). Os overrides `put`, `defineOwnProperty`,
//!   `deleteProperty` e `getOwnPropertyNames` ainda não têm despacho virtual. Ficam como consultas puras sobre o
//!   índice e o comprimento (`can_get_index`, `is_string_own_index`, `is_string_own_property`,
//!   `delete_property_by_index`, `own_index_names`) que o despacho vai chamar; a delegação à base
//!   `JSObject` e o `PropertyDescriptor` do caractere
//!   (`{[[Writable]]: false, [[Enumerable]]: true, [[Configurable]]: false}`) entram com ele.

use std::cell::RefCell;
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::{js_empty_string, js_substring, JSStringRef};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{
    TypeInfo, OVERRIDES_GET_OWN_PROPERTY_SLOT, OVERRIDES_GET_OWN_SPECIAL_PROPERTY_NAMES, OVERRIDES_PUT,
};
use crate::runtime::js_value::JSValue;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::PropertySlot;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo StringObject::s_info`.
pub static STRING_OBJECT_S_INFO: ClassInfo =
    ClassInfo { class_name: "String", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class StringObject : public JSWrapperObject`.
pub struct StringObject {
    base: JSNonFinalObject,
    /// `m_internalValue`.
    internal_value: RefCell<JSStringRef>,
}

/// O `StringObject*`.
pub type StringObjectRef = Rc<StringObject>;

impl std::fmt::Debug for StringObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StringObject")
            .field("cell_id", &self.base.cell_id())
            .field("internal_value", &self.internal_value.borrow().value())
            .finish()
    }
}

impl std::ops::Deref for StringObject {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl StringObject {
    /// `StructureFlags = Base::StructureFlags | OverridesGetOwnPropertySlot |
    /// OverridesGetOwnSpecialPropertyNames | OverridesPut`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS
        | OVERRIDES_GET_OWN_PROPERTY_SLOT
        | OVERRIDES_GET_OWN_SPECIAL_PROPERTY_NAMES
        | OVERRIDES_PUT;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::StringObjectType, StringObject::STRUCTURE_FLAGS),
            &STRING_OBJECT_S_INFO,
        )
    }

    /// `create(vm, structure, string)`: o construtor mais `finishCreation(vm, string)`, e o registro da
    /// célula.
    pub fn create_with_string(vm: &VM, structure: StructureRef, string: JSStringRef) -> StringObjectRef {
        let cell_id = cell_registry::reserve();
        let object = Rc::new(StringObject { base: JSNonFinalObject::new(vm, structure), internal_value: RefCell::new(string) });
        object.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::StringObject(Rc::clone(&object)));
        debug_assert!(matches!(object.type_(), JSType::StringObjectType | JSType::DerivedStringObjectType));
        object
    }

    /// `create(vm, structure)`: o valor interno nasce como a string vazia (o `String.prototype` é isto).
    pub fn create(vm: &VM, structure: StructureRef) -> StringObjectRef {
        StringObject::create_with_string(vm, structure, js_empty_string(vm))
    }

    /// Procura a célula pelo `cell_id` guardado em `JSValue::Cell`.
    pub fn from_cell_id(cell_id: usize) -> Option<StringObjectRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::StringObject(object)) => Some(object),
            _ => None,
        }
    }

    /// `JSValue(JSCell*)`.
    pub fn as_value(&self) -> JSValue {
        JSValue::from_cell(self.base.cell_id())
    }

    /// `internalValue()`.
    pub fn internal_value(&self) -> JSStringRef {
        Rc::clone(&self.internal_value.borrow())
    }

    /// `setInternalValue(vm, string)`.
    pub fn set_internal_value(&self, string: JSStringRef) {
        *self.internal_value.borrow_mut() = string;
    }

    /// `JSString::canGetIndex(i)` sobre o valor interno.
    pub fn can_get_index(&self, index: u32) -> bool {
        index < self.internal_value.borrow().length()
    }

    /// O índice do `isStringOwnProperty` e do `getOwnPropertySlotByIndex`: o caractere existe.
    pub fn is_string_own_index(&self, index: u32) -> bool {
        self.can_get_index(index)
    }

    /// `isStringOwnProperty(globalObject, object, propertyName)`: `length` ou um índice dentro da string.
    pub fn is_string_own_property(&self, vm: &VM, property_name: &PropertyName) -> bool {
        if *property_name == vm.property_names.length {
            return true;
        }
        match property_name.parse_index() {
            Some(index) => self.can_get_index(index),
            None => false,
        }
    }

    /// `StringObject::getOwnPropertySlot(cell, globalObject, propertyName, slot)`: o
    /// `getStringPropertySlot` do valor interno (`length` e os caracteres, `getIndex` é a substring de um
    /// caractere) e, se não é dele, o `JSObject::getOwnPropertySlot` da base.
    ///
    /// DIVERGÊNCIA: o `PropertySlot` do porte só aceita `JSObject` como `slotBase`; o do C++ é o próprio
    /// `JSString`, aqui o `StringObject`.
    pub fn get_own_property_slot(&self, vm: &VM, property_name: &PropertyName, slot: &mut PropertySlot) -> bool {
        let string = self.internal_value();
        if *property_name == vm.property_names.length {
            slot.set_value(self, DONT_ENUM | DONT_DELETE | READ_ONLY, JSValue::from_u32(string.length()));
            return true;
        }
        if let Some(index) = property_name.parse_index().filter(|index| *index < string.length()) {
            let character = JSValue::from_js_string(js_substring(vm, &string, index, 1));
            slot.set_value(self, DONT_DELETE | READ_ONLY, character);
            return true;
        }
        JSObject::get_own_property_slot(self, vm, property_name, slot)
    }

    /// `deletePropertyByIndex`: `Some(false)` quando o índice é um caractere (não apaga); `None` delega à
    /// base `JSObject`.
    pub fn delete_property_by_index(&self, index: u32) -> Option<bool> {
        if self.can_get_index(index) {
            return Some(false);
        }
        None
    }

    /// Os índices que `getOwnPropertyNames` acrescenta antes das propriedades da base: `0..length`.
    pub fn own_index_names(&self) -> std::ops::Range<u32> {
        0..self.internal_value.borrow().length()
    }
}

/// `constructString(vm, globalObject, string)`: `StringObject::create` com a `stringObjectStructure()` do
/// global e o valor interno já convertido (a conversão `toString` é de quem chama).
pub fn construct_string(vm: &VM, global_object: &JSGlobalObject, string: JSStringRef) -> StringObjectRef {
    StringObject::create_with_string(vm, global_object.string_object_structure(), string)
}
