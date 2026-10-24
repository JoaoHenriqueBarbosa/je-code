//! Tradução parcial de `runtime/PropertySlot.h` e `PropertySlot.cpp`: o `PropertySlot` de propriedade de
//! valor (dado), de accessor (`TypeGetter`, `setGetterSlot`/`setCacheableGetterSlot`) e custom
//! (`TypeCustom`, `setCustom`/`setCacheableCustom`, `GetValueFunc`/`PutValueFunc`).
//!
//! Fora desta fatia, e por quê: `setUndefined`/`setWatchpointSet`, os dados adicionais (`DOMAttribute`,
//! `ModuleNamespace`), `getPureResult` e `setValue(JSString*, ...)` (sem protótipo de `String` ainda).
//! O `VMInquiry` com `DisallowVMEntry` é só o tipo de método interno: o `VM` não bloqueia entrada.
//!
//! DIVERGÊNCIAS:
//!
//! - `getValue(globalObject, propertyName)` é [`PropertySlot::get_value_for`]: o `globalObject` do getter
//!   de função é o realm do próprio getter (`GetterSetter::call_getter`) e o do getter custom é o
//!   `m_slotBase->realm()`, que `setCustom`/`setCacheableCustom` guardam no slot (o `slotBase()` do porte
//!   é um `cell_id`, e `JSObject::from_cell_id` não alcança `JSFunction`). Exceção lançada pelo getter
//!   fica pendente no `VM` e o resultado é `undefined` (não o `JSValue()` vazio do C++: quem lê pelo
//!   porte nunca confere um valor vazio, só `vm.exception()`). O caminho que o porte ainda não tem
//!   (`PutError::Unported` de `callGetter`) vira `panic!` com a mensagem, como o `Thrown::Unported`.
//! - [`PropertySlot::get_value`] é só o `m_data.value` de `TypeValue`: quem sabe que o slot é de valor.

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::identifier::Identifier;
use crate::runtime::js_getter_setter::GetterSetterRef;
use crate::runtime::js_global_object::{JSGlobalObject, JSGlobalObjectRef};
use crate::runtime::js_object::{JSObject, PutError};
use crate::runtime::js_scope::JSScopeRef;
use crate::runtime::js_value::{EncodedJSValue, JSValue};
use crate::runtime::property_attribute::CUSTOM_ACCESSOR;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_offset::{PropertyOffset, INVALID_OFFSET};
use crate::runtime::vm::VM;

/// `CacheabilityType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheabilityType {
    CachingDisallowed,
    CachingAllowed,
}

/// `attributesForStructure(unsigned)`: os atributos só da tabela estática ficam do bit 8 para cima.
pub fn attributes_for_structure(attributes: u32) -> u32 {
    attributes as u8 as u32
}

/// `GetValueFunc`: `EncodedJSValue(JSGlobalObject*, EncodedJSValue thisValue, PropertyName)`.
pub type GetValueFunc = fn(&JSGlobalObject, EncodedJSValue, &PropertyName) -> EncodedJSValue;

/// `PutValueFunc`: `bool(JSGlobalObject*, EncodedJSValue thisValue, EncodedJSValue value, PropertyName)`.
pub type PutValueFunc = fn(&JSGlobalObject, EncodedJSValue, EncodedJSValue, &PropertyName) -> bool;

/// `PropertySlot::InternalMethodType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InternalMethodType {
    /// `[[Get]]` da especificação.
    Get,
    /// `[[HasProperty]]`.
    HasProperty,
    /// `[[GetOwnProperty]]`.
    GetOwnProperty,
    /// O VM só está espiando: `getOwnPropertySlot` não pode ter efeito observável.
    VMInquiry,
}

/// `PropertySlot::PropertyType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PropertyType {
    TypeUnset,
    TypeValue,
    TypeGetter,
    TypeCustom,
}

/// `class PropertySlot`.
#[derive(Debug)]
pub struct PropertySlot {
    value: JSValue,
    /// `m_data.getter.getterSetter`.
    getter_setter: Option<GetterSetterRef>,
    /// `m_data.custom.getValue`.
    custom_getter: Option<GetValueFunc>,
    /// `m_data.custom.putValue`.
    custom_setter: Option<PutValueFunc>,
    this_value: JSValue,
    /// `m_slotBase`, como o `cell_id` do objeto (0 é o `nullptr`).
    slot_base: usize,
    /// O `cell_id` do `m_slotBase->realm()`, só preenchido por `setCustom`/`setCacheableCustom` (0 é nenhum).
    slot_base_realm: usize,
    offset: PropertyOffset,
    attributes: u32,
    internal_method_type: InternalMethodType,
    property_type: PropertyType,
    cacheability: CacheabilityType,
    is_tainted_by_opaque_object: bool,
}

impl PropertySlot {
    /// `PropertySlot(const JSValue thisValue, InternalMethodType, VM* vmForInquiry)`.
    pub fn new(this_value: JSValue, internal_method_type: InternalMethodType) -> PropertySlot {
        PropertySlot {
            value: JSValue::empty(),
            getter_setter: None,
            custom_getter: None,
            custom_setter: None,
            this_value,
            slot_base: 0,
            slot_base_realm: 0,
            offset: INVALID_OFFSET,
            attributes: 0,
            internal_method_type,
            property_type: PropertyType::TypeUnset,
            cacheability: CacheabilityType::CachingDisallowed,
            is_tainted_by_opaque_object: false,
        }
    }

    pub fn this_value(&self) -> JSValue {
        self.this_value
    }

    /// `isCacheable()`.
    pub fn is_cacheable(&self) -> bool {
        self.is_unset() || self.cacheability == CacheabilityType::CachingAllowed
    }

    pub fn is_unset(&self) -> bool {
        self.property_type == PropertyType::TypeUnset
    }

    pub fn is_value(&self) -> bool {
        self.property_type == PropertyType::TypeValue
    }

    /// `isAccessor()`.
    pub fn is_accessor(&self) -> bool {
        self.property_type == PropertyType::TypeGetter
    }

    /// `isCustom()`.
    pub fn is_custom(&self) -> bool {
        self.property_type == PropertyType::TypeCustom
    }

    pub fn is_cacheable_value(&self) -> bool {
        self.is_cacheable() && self.is_value()
    }

    /// `isCacheableCustom()`.
    pub fn is_cacheable_custom(&self) -> bool {
        self.is_cacheable() && self.is_custom()
    }

    pub fn set_is_tainted_by_opaque_object(&mut self) {
        self.is_tainted_by_opaque_object = true;
    }

    pub fn is_tainted_by_opaque_object(&self) -> bool {
        self.is_tainted_by_opaque_object
    }

    pub fn internal_method_type(&self) -> InternalMethodType {
        self.internal_method_type
    }

    /// `isVMInquiry()`.
    pub fn is_vm_inquiry(&self) -> bool {
        self.internal_method_type == InternalMethodType::VMInquiry
    }

    /// `disableCaching()`.
    pub fn disable_caching(&mut self) {
        self.cacheability = CacheabilityType::CachingDisallowed;
    }

    /// `attributes()`.
    pub fn attributes(&self) -> u32 {
        self.attributes
    }

    /// `cachedOffset()`.
    pub fn cached_offset(&self) -> PropertyOffset {
        debug_assert!(self.is_cacheable());
        self.offset
    }

    /// `slotBase()`: o `cell_id` do objeto (`JSObject::from_cell_id` o resolve), `None` é o `nullptr`.
    pub fn slot_base(&self) -> Option<usize> {
        (self.slot_base != 0).then_some(self.slot_base)
    }

    /// `customGetter()` (o `GetValueFunc` de `TypeCustom`).
    pub fn custom_getter(&self) -> GetValueFunc {
        debug_assert!(self.is_custom());
        self.custom_getter.expect("PropertySlot::customGetter sem GetValueFunc (ASSERT do C++)")
    }

    /// `customSetter()`: `None` é o `PutValueFunc` nulo.
    pub fn custom_setter(&self) -> Option<PutValueFunc> {
        debug_assert!(self.is_custom());
        self.custom_setter
    }

    /// `setValue(JSObject* slotBase, unsigned attributes, JSValue value)`.
    pub fn set_value(&mut self, slot_base: &JSObject, attributes: u32, value: JSValue) {
        debug_assert!(attributes == attributes_for_structure(attributes));
        self.value = value;
        self.attributes = attributes;
        self.slot_base = slot_base.cell_id();
        self.property_type = PropertyType::TypeValue;
        debug_assert!(self.cacheability == CacheabilityType::CachingDisallowed);
    }

    /// `setValue(JSObject* slotBase, unsigned attributes, JSValue value, PropertyOffset offset)`.
    pub fn set_value_at_offset(&mut self, slot_base: &JSObject, attributes: u32, value: JSValue, offset: PropertyOffset) {
        debug_assert!(attributes == attributes_for_structure(attributes));
        debug_assert!(!value.is_empty());
        self.value = value;
        self.attributes = attributes;
        self.slot_base = slot_base.cell_id();
        self.property_type = PropertyType::TypeValue;
        self.offset = offset;
        self.cacheability = CacheabilityType::CachingAllowed;
    }

    /// `setCustom(JSObject* slotBase, unsigned attributes, GetValueFunc getValue, PutValueFunc putValue)`.
    pub fn set_custom(
        &mut self,
        slot_base: &JSObject,
        attributes: u32,
        get_value: GetValueFunc,
        put_value: Option<PutValueFunc>,
    ) {
        debug_assert!(attributes == attributes_for_structure(attributes));
        self.fill_custom(slot_base, attributes, get_value, put_value);
        debug_assert!(self.cacheability == CacheabilityType::CachingDisallowed);
    }

    /// `setCacheableCustom(JSObject* slotBase, unsigned attributes, GetValueFunc getValue, PutValueFunc putValue)`.
    pub fn set_cacheable_custom(
        &mut self,
        slot_base: &JSObject,
        attributes: u32,
        get_value: GetValueFunc,
        put_value: Option<PutValueFunc>,
    ) {
        debug_assert!(attributes == attributes_for_structure(attributes));
        self.fill_custom(slot_base, attributes, get_value, put_value);
        self.cacheability = CacheabilityType::CachingAllowed;
    }

    /// O que `setCustom` e `setCacheableCustom` têm em comum.
    fn fill_custom(&mut self, slot_base: &JSObject, attributes: u32, get_value: GetValueFunc, put_value: Option<PutValueFunc>) {
        self.custom_getter = Some(get_value);
        self.custom_setter = put_value;
        self.attributes = attributes;
        self.slot_base = slot_base.cell_id();
        self.slot_base_realm = slot_base.structure().realm().map_or(0, |realm| realm.cell_id());
        self.property_type = PropertyType::TypeCustom;
    }

    /// `setGetterSlot(JSObject* slotBase, unsigned attributes, GetterSetter*)`.
    pub fn set_getter_slot(&mut self, slot_base: &JSObject, attributes: u32, getter_setter: GetterSetterRef) {
        debug_assert!(attributes == attributes_for_structure(attributes));
        self.getter_setter = Some(getter_setter);
        self.attributes = attributes;
        self.slot_base = slot_base.cell_id();
        self.property_type = PropertyType::TypeGetter;
        debug_assert!(self.cacheability == CacheabilityType::CachingDisallowed);
    }

    /// `setCacheableGetterSlot(JSObject* slotBase, unsigned attributes, GetterSetter*, PropertyOffset)`.
    pub fn set_cacheable_getter_slot(
        &mut self,
        slot_base: &JSObject,
        attributes: u32,
        getter_setter: GetterSetterRef,
        offset: PropertyOffset,
    ) {
        debug_assert!(attributes == attributes_for_structure(attributes));
        self.getter_setter = Some(getter_setter);
        self.attributes = attributes;
        self.slot_base = slot_base.cell_id();
        self.property_type = PropertyType::TypeGetter;
        self.offset = offset;
        self.cacheability = CacheabilityType::CachingAllowed;
    }

    /// `getterSetter()` (`m_data.getter.getterSetter`).
    pub fn getter_setter(&self) -> GetterSetterRef {
        debug_assert!(self.is_accessor());
        self.getter_setter.clone().expect("PropertySlot::getterSetter sem GetterSetter (ASSERT do C++)")
    }

    /// O `m_data.value` de `TypeValue` (veja o cabeçalho do módulo).
    pub fn get_value(&self) -> JSValue {
        debug_assert!(self.is_value());
        self.value
    }

    /// `getValue(globalObject, propertyName)`: o valor, o resultado do getter de função
    /// (`functionGetter`) ou o do getter custom (`customGetter`).
    pub fn get_value_for(&self, property_name: &PropertyName) -> JSValue {
        let value = match self.property_type {
            PropertyType::TypeValue => return self.value,
            PropertyType::TypeGetter => match self.getter_setter().call_getter(self.this_value) {
                Ok(value) => value,
                Err(PutError::Unported(what)) => panic!("caminho ainda não portado: {what}"),
                Err(_) => unreachable!("GetterSetter::call_getter só falha com Unported"),
            },
            PropertyType::TypeCustom => self.custom_getter_value(property_name),
            PropertyType::TypeUnset => unreachable!("PropertySlot::getValue de slot sem propriedade"),
        };
        if value.is_empty() {
            JSValue::undefined()
        } else {
            value
        }
    }

    /// `getValue(globalObject, uint64_t propertyName)`: o nome só vira `Identifier` quando o getter é custom.
    pub fn get_value_for_index(&self, vm: &VM, index: u32) -> JSValue {
        if self.is_value() {
            return self.value;
        }
        self.get_value_for(&PropertyName::from_identifier(&Identifier::from_u32(vm, index)))
    }

    /// `customGetter(vm, propertyName)`: o `this` do getter é o do slot para `CustomAccessor` e o próprio
    /// `slotBase()` para `CustomValue`.
    fn custom_getter_value(&self, property_name: &PropertyName) -> JSValue {
        debug_assert!(self.slot_base != 0);
        let global_object = self.slot_base_realm();
        let this_value = if self.attributes & CUSTOM_ACCESSOR != 0 { self.this_value } else { JSValue::from_cell(self.slot_base) };
        JSValue::decode(self.custom_getter()(&global_object, this_value.encode(), property_name))
    }

    /// `m_slotBase->realm()`.
    fn slot_base_realm(&self) -> JSGlobalObjectRef {
        match cell_registry::get(self.slot_base_realm) {
            Some(CellEntry::Scope(JSScopeRef::GlobalObject(global_object))) => global_object,
            _ => panic!("PropertySlot custom sem realm no slotBase (Structure sem JSGlobalObject)"),
        }
    }
}
