//! Porte de `runtime/RegExpObject.h`, `RegExpObject.cpp` e, de `RegExpObjectInlines.h`, o que o
//! `lastIndex` e o `exec`/`test` usam (`getRegExpObjectLastIndexAsUnsigned`, `execInline`,
//! `matchInline`).
//!
//! DIVERGÊNCIAS (heap ausente, camada 3; mesmo padrão de `error_instance.rs`):
//!
//! - `m_regExpAndFlags` (ponteiro com dois bits de flag) vira o `RegExpRef` mais um campo de flags com
//!   os mesmos bits (`LAST_INDEX_IS_NOT_WRITABLE_FLAG`, `LEGACY_FEATURES_DISABLED_FLAG`).
//!   `offsetOf*`, `allocationSize` e `subspaceFor` existem só para o layout de memória e somem.
//!   `visitChildren` some com o GC. O registro é `CellEntry::RegExpObject`.
//! - Erros lançáveis (`throwTypeError`, `typeError(..., shouldThrow, ...)`) saem como
//!   `Err(PutError::TypeError(..))`, como em `js_object.rs`.
//! - Os overrides `getOwnPropertySlot`, `put`, `deleteProperty` e `defineOwnProperty` ainda não têm
//!   despacho virtual: cada um trata só o `lastIndex` e devolve `None` (delegar à base `JSObject`)
//!   para qualquer outro nome. Do `put` falta o `slot.setCustomValue(...)` com os setters de
//!   `lastIndex`, e do `defineOwnProperty` o `regExpLastIndexWritableWatchpointSet().fireAll`
//!   (nenhum dos dois existe no porte). `getOwnSpecialPropertyNames` espera o
//!   `PropertyNameArrayBuilder`.
//! - `isSymbol{Match,Search,MatchAll,Replace,Split}FastAndNonObservable` esperam os watchpoint sets do
//!   `JSGlobalObject` e a `regExpPrototype`.
//! - `matchInline`/`execInline` portam o tratamento de `lastIndex` por inteiro e passam pelo
//!   `RegExpGlobalData::performMatch`/`recordMatch` e por `createRegExpMatchesArray`
//!   (`reg_exp_global_data.rs`, `reg_exp_matches_array.rs`). `matchGlobal` e os `collectMatches`
//!   esperam o `JSArray` de resultados de `String.prototype.match` e o `RegExpSubstringGlobalAtomCache`.
//! - `string->view(globalObject)` só entra pelo comprimento (`JSString::length`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::current_realm::has_pending_exception;
use crate::runtime::error_messages::{
    READONLY_PROPERTY_CHANGE_ERROR, READONLY_PROPERTY_WRITE_ERROR, UNCONFIGURABLE_PROPERTY_CHANGE_ACCESS_MECHANISM_ERROR,
    UNCONFIGURABLE_PROPERTY_CHANGE_CONFIGURABILITY_ERROR, UNCONFIGURABLE_PROPERTY_CHANGE_ENUMERABILITY_ERROR,
    UNCONFIGURABLE_PROPERTY_CHANGE_WRITABILITY_ERROR,
};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{type_error, JSNonFinalObject, JSObject, PutError, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::JSStringRef;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{
    TypeInfo, OVERRIDES_GET_OWN_PROPERTY_SLOT, OVERRIDES_GET_OWN_SPECIAL_PROPERTY_NAMES, OVERRIDES_PUT,
};
use crate::runtime::js_value::{js_null, js_number, JSValue};
use crate::runtime::match_result::MatchResult;
use crate::runtime::operations::same_value;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::PropertySlot;
use crate::runtime::put_property_slot::PutPropertySlot;
use crate::runtime::reg_exp::RegExpRef;
use crate::runtime::reg_exp_matches_array::create_reg_exp_matches_array;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo RegExpObject::s_info`.
pub static REG_EXP_OBJECT_S_INFO: ClassInfo =
    ClassInfo { class_name: "RegExp", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `lastIndexIsNotWritableFlag`.
pub const LAST_INDEX_IS_NOT_WRITABLE_FLAG: u8 = 0b01;
/// `legacyFeaturesDisabledFlag`.
pub const LEGACY_FEATURES_DISABLED_FLAG: u8 = 0b10;

/// `class RegExpObject final : public JSNonFinalObject`.
pub struct RegExpObject {
    base: JSNonFinalObject,
    /// `m_regExpAndFlags`, a parte do ponteiro (`regExp()`).
    reg_exp: RefCell<RegExpRef>,
    /// `m_regExpAndFlags & flagsMask`.
    flags: Cell<u8>,
    /// `m_lastIndex`.
    last_index: Cell<JSValue>,
}

/// O `RegExpObject*`.
pub type RegExpObjectRef = Rc<RegExpObject>;

impl std::fmt::Debug for RegExpObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegExpObject")
            .field("cell_id", &self.base.cell_id())
            .field("flags", &self.flags.get())
            .field("last_index", &self.last_index.get())
            .finish()
    }
}

impl std::ops::Deref for RegExpObject {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl RegExpObject {
    /// `visitChildren`: o `lastIndex` (a `RegExp` é `Rc`, já raiz).
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        marker.value(self.last_index.get());
    }
    /// `StructureFlags = Base::StructureFlags | OverridesGetOwnPropertySlot |
    /// OverridesGetOwnSpecialPropertyNames | OverridesPut`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS
        | OVERRIDES_GET_OWN_PROPERTY_SLOT
        | OVERRIDES_GET_OWN_SPECIAL_PROPERTY_NAMES
        | OVERRIDES_PUT;

    /// `createStructure(vm, globalObject, prototype)` (RegExpObjectInlines.h).
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::RegExpObjectType, RegExpObject::STRUCTURE_FLAGS),
            &REG_EXP_OBJECT_S_INFO,
        )
    }

    /// `create(vm, structure, regExp, areLegacyFeaturesEnabled)`: o construtor (`lastIndex` é 0 e
    /// gravável) e o `finishCreation`, mais o registro da célula.
    pub fn create(vm: &VM, structure: StructureRef, reg_exp: RegExpRef, are_legacy_features_enabled: bool) -> RegExpObjectRef {
        let flags = if are_legacy_features_enabled { 0 } else { LEGACY_FEATURES_DISABLED_FLAG };
        let cell_id = cell_registry::reserve();
        let object = Rc::new(RegExpObject {
            base: JSNonFinalObject::new(vm, structure),
            reg_exp: RefCell::new(reg_exp),
            flags: Cell::new(flags),
            last_index: Cell::new(js_number(0.0)),
        });
        object.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::RegExpObject(Rc::clone(&object)));
        debug_assert_eq!(object.type_(), JSType::RegExpObjectType);
        object
    }

    /// `create(vm, structure, regExp, lastIndex)`: com o legado habilitado.
    pub fn create_with_last_index(vm: &VM, structure: StructureRef, reg_exp: RegExpRef, last_index: JSValue) -> RegExpObjectRef {
        let object = RegExpObject::create(vm, structure, reg_exp, true);
        object.last_index.set(last_index);
        object
    }

    /// Procura a célula pelo `cell_id` guardado em `JSValue::Cell`.
    pub fn from_cell_id(cell_id: usize) -> Option<RegExpObjectRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::RegExpObject(object)) => Some(object),
            _ => None,
        }
    }

    /// `setRegExp(vm, regExp)`: preserva as flags.
    pub fn set_reg_exp(&self, reg_exp: RegExpRef) {
        *self.reg_exp.borrow_mut() = reg_exp;
    }

    /// `regExp()`.
    pub fn reg_exp(&self) -> RegExpRef {
        Rc::clone(&self.reg_exp.borrow())
    }

    /// `setLastIndex(globalObject, uint64_t)`: lança `TypeError` se `lastIndex` não é gravável.
    pub fn set_last_index_number(&self, last_index: u64) -> Result<bool, PutError> {
        if self.last_index_is_writable() {
            self.last_index.set(js_number(last_index as f64));
            return Ok(true);
        }
        Err(PutError::TypeError(READONLY_PROPERTY_WRITE_ERROR))
    }

    /// `setLastIndex(globalObject, JSValue, shouldThrow)`.
    pub fn set_last_index(&self, last_index: JSValue, should_throw: bool) -> Result<bool, PutError> {
        if self.last_index_is_writable() {
            self.last_index.set(last_index);
            return Ok(true);
        }
        type_error(should_throw, READONLY_PROPERTY_WRITE_ERROR)
    }

    /// `getLastIndex()`.
    pub fn get_last_index(&self) -> JSValue {
        self.last_index.get()
    }

    /// `lastIndexIsWritable()`.
    pub fn last_index_is_writable(&self) -> bool {
        self.flags.get() & LAST_INDEX_IS_NOT_WRITABLE_FLAG == 0
    }

    /// `areLegacyFeaturesEnabled()`.
    pub fn are_legacy_features_enabled(&self) -> bool {
        self.flags.get() & LEGACY_FEATURES_DISABLED_FLAG == 0
    }

    /// `setLastIndexIsNotWritable()`.
    fn set_last_index_is_not_writable(&self) {
        self.flags.set(self.flags.get() | LAST_INDEX_IS_NOT_WRITABLE_FLAG);
    }

    /// `getOwnPropertySlot(object, globalObject, propertyName, slot)`: o `lastIndex` é uma propriedade
    /// de dados não configurável, não enumerável, gravável conforme a flag; o resto é da base.
    pub fn get_own_property_slot(&self, vm: &VM, property_name: &PropertyName, slot: &mut PropertySlot) -> bool {
        if *property_name == vm.property_names.last_index {
            let attributes = if self.last_index_is_writable() {
                DONT_DELETE | DONT_ENUM
            } else {
                DONT_DELETE | DONT_ENUM | READ_ONLY
            };
            slot.set_value(self, attributes, self.get_last_index());
            return true;
        }
        let base: &JSObject = self;
        base.get_own_property_slot(vm, property_name, slot)
    }

    /// `deleteProperty`: `Some(false)` para o `lastIndex` (não configurável); `None` delega à base.
    pub fn delete_property(&self, vm: &VM, property_name: &PropertyName) -> Option<bool> {
        if *property_name == vm.property_names.last_index {
            return Some(false);
        }
        None
    }

    /// `put(cell, globalObject, propertyName, value, slot)`: `None` delega à base.
    pub fn put(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        value: JSValue,
        slot: &mut PutPropertySlot,
    ) -> Option<Result<bool, PutError>> {
        if *property_name != vm.property_names.last_index {
            return None;
        }
        if !self.last_index_is_writable() {
            return Some(type_error(slot.is_strict_mode(), READONLY_PROPERTY_WRITE_ERROR));
        }
        if slot.this_value() != self.as_value() {
            return Some(self.define_property_on_receiver(vm, property_name, value, slot));
        }
        Some(self.set_last_index(value, slot.is_strict_mode()))
    }

    /// `defineOwnProperty(object, globalObject, propertyName, descriptor, shouldThrow)`: `None` delega
    /// à base.
    pub fn define_own_property(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        descriptor: &PropertyDescriptor,
        should_throw: bool,
    ) -> Option<Result<bool, PutError>> {
        if *property_name != vm.property_names.last_index {
            return None;
        }
        if descriptor.configurable_present() && descriptor.configurable() {
            return Some(type_error(should_throw, UNCONFIGURABLE_PROPERTY_CHANGE_CONFIGURABILITY_ERROR));
        }
        if descriptor.enumerable_present() && descriptor.enumerable() {
            return Some(type_error(should_throw, UNCONFIGURABLE_PROPERTY_CHANGE_ENUMERABILITY_ERROR));
        }
        if descriptor.is_accessor_descriptor() {
            return Some(type_error(should_throw, UNCONFIGURABLE_PROPERTY_CHANGE_ACCESS_MECHANISM_ERROR));
        }
        if !self.last_index_is_writable() {
            if descriptor.writable_present() && descriptor.writable() {
                return Some(type_error(should_throw, UNCONFIGURABLE_PROPERTY_CHANGE_WRITABILITY_ERROR));
            }
            if !descriptor.value().is_empty() && !same_value(self.get_last_index(), descriptor.value()) {
                return Some(type_error(should_throw, READONLY_PROPERTY_CHANGE_ERROR));
            }
            return Some(Ok(true));
        }
        if !descriptor.value().is_empty() {
            if let Err(error) = self.set_last_index(descriptor.value(), false) {
                return Some(Err(error));
            }
        }
        if descriptor.writable_present() && !descriptor.writable() {
            // Falta `realm()->regExpLastIndexWritableWatchpointSet().fireAll(...)`.
            self.set_last_index_is_not_writable();
        }
        Some(Ok(true))
    }

    /// `getRegExpObjectLastIndexAsUnsigned(globalObject, regExpObject, input)`: `u32::MAX` (`UINT_MAX`)
    /// quando o `lastIndex` passa do comprimento da entrada. `Err(Pending)` é o `RETURN_IF_EXCEPTION` do
    /// `toIntegerOrInfinity` (um `valueOf` que lança, um `Symbol`, um `BigInt`).
    pub fn last_index_as_unsigned(&self, input_length: u32) -> Result<u32, PutError> {
        let js_last_index = self.get_last_index();
        if js_last_index.is_uint32() {
            let last_index = js_last_index.as_uint32();
            if last_index > input_length {
                return Ok(u32::MAX);
            }
            return Ok(last_index);
        }
        let double_last_index = js_last_index.to_integer_or_infinity();
        if has_pending_exception() {
            return Err(PutError::Pending);
        }
        if double_last_index > f64::from(input_length) {
            return Ok(u32::MAX);
        }
        Ok(if double_last_index < 0.0 { 0 } else { double_last_index as u32 })
    }

    /// `match(globalObject, string)` (`matchInline` em RegExpObjectInlines.h, que o `.cpp` só chama).
    pub fn match_(&self, global_object: &JSGlobalObject, string: &JSStringRef) -> Result<MatchResult, PutError> {
        let last_index = self.last_index_as_unsigned(string.length())?;
        let reg_exp = self.reg_exp();
        let global_data = global_object.reg_exp_global_data();
        if !reg_exp.global_or_sticky() {
            return Ok(global_data.perform_match(global_object, &reg_exp, string, 0));
        }

        if last_index == u32::MAX {
            self.set_last_index_number(0)?;
            return Ok(MatchResult::failed());
        }

        let result = global_data.perform_match(global_object, &reg_exp, string, last_index);
        self.set_last_index_number(result.end as u64)?;
        Ok(result)
    }

    /// `test(globalObject, string)`: `!!match(globalObject, string)`.
    pub fn test(&self, global_object: &JSGlobalObject, string: &JSStringRef) -> Result<bool, PutError> {
        Ok(self.match_(global_object, string)?.matched())
    }

    /// `execInline(globalObject, string, result)`: o array de resultados (ou `null`) e o
    /// `MatchResult`, com o `recordMatch` do `RegExpGlobalData` (`RegExp.$1` e companhia).
    pub fn exec_inline(&self, global_object: &JSGlobalObject, string: &JSStringRef) -> Result<(JSValue, MatchResult), PutError> {
        let mut last_index = self.last_index_as_unsigned(string.length())?;
        let reg_exp = self.reg_exp();
        let global_or_sticky = reg_exp.global_or_sticky();
        if last_index == u32::MAX && global_or_sticky {
            self.set_last_index_number(0)?;
            return Ok((js_null(), MatchResult::failed()));
        }

        if !global_or_sticky {
            last_index = 0;
        }

        let Some((array, result)) = create_reg_exp_matches_array(global_object, string, &reg_exp, last_index) else {
            if global_or_sticky {
                self.set_last_index_number(0)?;
            }
            return Ok((js_null(), MatchResult::failed()));
        };

        // O `setLastIndex` vem antes do `recordMatch`: se o `lastIndex` não é gravável o `TypeError` sai
        // sem tocar nas estáticas legadas (`RegExp.$1`...).
        if global_or_sticky {
            self.set_last_index_number(result.end as u64)?;
        }
        global_object.reg_exp_global_data().record_match(&reg_exp, string, result, false);
        Ok((array.as_value(), result))
    }

    /// `exec(globalObject, string)`.
    pub fn exec(&self, global_object: &JSGlobalObject, string: &JSStringRef) -> Result<JSValue, PutError> {
        Ok(self.exec_inline(global_object, string)?.0)
    }
}
