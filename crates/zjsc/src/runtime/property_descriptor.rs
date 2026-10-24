//! Tradução de `runtime/PropertyDescriptor.h` e `.cpp`.
//!
//! O getter/setter nulo do C++ (`NullGetterFunction`) é `None` em `GetterSetter` e vira `undefined` no
//! descritor, como o `isGetterNull()` faz. `getterObject`/`setterObject` devolvem o `JSObject*` como
//! [`ObjectRef`] (que alcança `JSFunction`, o que `JSObjectHandle` não alcança).

use crate::runtime::define_property_attributes::DefinePropertyAttributes;
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::js_getter_setter::{GetterSetter, GetterSetterRef};
use crate::runtime::js_object::JSObject;
use crate::runtime::js_custom_accessor_function::{create_custom_getter_function, create_custom_setter_function};
use crate::runtime::js_value::JSValue;
use crate::runtime::operations::{same_value, strict_equal};
use crate::runtime::property_attribute::{ACCESSOR, CUSTOM_ACCESSOR, CUSTOM_ACCESSOR_OR_VALUE, CUSTOM_VALUE, DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::PropertySlot;
use crate::runtime::vm::VM;

const WRITABLE_PRESENT: u32 = 1;
const ENUMERABLE_PRESENT: u32 = 2;
const CONFIGURABLE_PRESENT: u32 = 4;

/// `PropertyDescriptor::defaultAttributes`.
const DEFAULT_ATTRIBUTES: u32 = DONT_DELETE | DONT_ENUM | READ_ONLY;

/// `class PropertyDescriptor`.
#[derive(Clone, Copy, Debug)]
pub struct PropertyDescriptor {
    /// `m_value`: vazio é o `JSValue()` do C++.
    value: JSValue,
    getter: JSValue,
    setter: JSValue,
    attributes: u32,
    seen_attributes: u32,
}

impl Default for PropertyDescriptor {
    /// `PropertyDescriptor()`.
    fn default() -> PropertyDescriptor {
        PropertyDescriptor {
            value: JSValue::empty(),
            getter: JSValue::empty(),
            setter: JSValue::empty(),
            attributes: DEFAULT_ATTRIBUTES,
            seen_attributes: 0,
        }
    }
}

impl PropertyDescriptor {
    /// `PropertyDescriptor(JSValue, unsigned attributes)`.
    pub fn new(value: JSValue, attributes: u32) -> PropertyDescriptor {
        debug_assert!(!value.is_empty());
        PropertyDescriptor {
            value,
            attributes,
            seen_attributes: ENUMERABLE_PRESENT | CONFIGURABLE_PRESENT | WRITABLE_PRESENT,
            ..PropertyDescriptor::default()
        }
    }

    /// `writable()`.
    pub fn writable(&self) -> bool {
        debug_assert!(!self.is_accessor_descriptor());
        self.attributes & READ_ONLY == 0
    }

    /// `enumerable()`.
    pub fn enumerable(&self) -> bool {
        self.attributes & DONT_ENUM == 0
    }

    /// `configurable()`.
    pub fn configurable(&self) -> bool {
        self.attributes & DONT_DELETE == 0
    }

    /// `isDataDescriptor()`.
    pub fn is_data_descriptor(&self) -> bool {
        !self.value.is_empty() || self.seen_attributes & WRITABLE_PRESENT != 0
    }

    /// `isGenericDescriptor()`.
    pub fn is_generic_descriptor(&self) -> bool {
        !self.is_accessor_descriptor() && !self.is_data_descriptor()
    }

    /// `isAccessorDescriptor()`.
    pub fn is_accessor_descriptor(&self) -> bool {
        !self.getter.is_empty() || !self.setter.is_empty()
    }

    /// `attributes()`.
    pub fn attributes(&self) -> u32 {
        self.attributes
    }

    /// `value()`.
    pub fn value(&self) -> JSValue {
        self.value
    }

    /// `getter()`.
    pub fn getter(&self) -> JSValue {
        debug_assert!(self.is_accessor_descriptor());
        self.getter
    }

    /// `setter()`.
    pub fn setter(&self) -> JSValue {
        debug_assert!(self.is_accessor_descriptor());
        self.setter
    }

    /// `getterObject()`: `None` é o `nullptr` de quando o getter não é objeto.
    pub fn getter_object(&self) -> Option<ObjectRef> {
        debug_assert!(self.is_accessor_descriptor() && self.getter_present());
        ObjectRef::from_value(&self.getter)
    }

    /// `setterObject()`.
    pub fn setter_object(&self) -> Option<ObjectRef> {
        debug_assert!(self.is_accessor_descriptor() && self.setter_present());
        ObjectRef::from_value(&self.setter)
    }

    /// `slowGetterSetter(globalObject)`.
    pub fn slow_getter_setter(&self, vm: &VM) -> GetterSetterRef {
        let getter = if !self.getter.is_empty() && !self.getter.is_undefined() { self.getter } else { JSValue::undefined() };
        let setter = if !self.setter.is_empty() && !self.setter.is_undefined() { self.setter } else { JSValue::undefined() };
        GetterSetter::create_from_values(vm, getter, setter)
    }

    /// `setUndefined()`.
    pub fn set_undefined(&mut self) {
        self.value = JSValue::undefined();
        self.attributes = READ_ONLY | DONT_DELETE | DONT_ENUM;
    }

    /// `setDescriptor(JSValue, unsigned attributes)`.
    pub fn set_descriptor(&mut self, value: JSValue, attributes: u32) {
        debug_assert!(!value.is_empty());
        // `PropertyAttribute::CustomValue` não é observável pelo JS, então sai logo na entrada.
        self.attributes = attributes & !CUSTOM_VALUE;
        if let Some(accessor) = GetterSetter::from_value(&value) {
            self.attributes &= !READ_ONLY; // FIXME do C++: we should be able to ASSERT this!

            self.getter = accessor.getter_value_or_undefined();
            self.setter = accessor.setter_value_or_undefined();
            self.seen_attributes = ENUMERABLE_PRESENT | CONFIGURABLE_PRESENT;
        } else {
            self.value = value;
            self.seen_attributes = ENUMERABLE_PRESENT | CONFIGURABLE_PRESENT | WRITABLE_PRESENT;
        }
    }

    /// `setAccessorDescriptor(GetterSetter*, unsigned attributes)`.
    pub fn set_accessor_descriptor_from(&mut self, accessor: &GetterSetter, attributes: u32) {
        debug_assert!(attributes & ACCESSOR != 0);
        debug_assert!(attributes & CUSTOM_VALUE == 0);
        self.attributes = attributes & !READ_ONLY; // FIXME do C++: we should be able to ASSERT this!
        self.getter = accessor.getter_value_or_undefined();
        self.setter = accessor.setter_value_or_undefined();
        self.seen_attributes = ENUMERABLE_PRESENT | CONFIGURABLE_PRESENT;
    }

    /// `setPropertySlot(globalObject, propertyName, slot)`: o descritor da propriedade que o `slot`
    /// achou. O ramo `CustomAccessor` cria o getter e o setter visíveis ao JS no `realm()` do
    /// `slotBase`, como o C++, de onde sai o `VM` que a assinatura daqui não carrega.
    pub fn set_property_slot(&mut self, slot: &PropertySlot, property_name: &PropertyName) {
        if slot.is_accessor() {
            let accessor = slot.getter_setter();
            self.set_accessor_descriptor_from(&accessor, slot.attributes());
        } else if slot.attributes() & CUSTOM_ACCESSOR != 0 {
            debug_assert!(slot.is_custom(), "PropertySlot::TypeCustom is required in case of PropertyAttribute::CustomAccessor");
            self.set_accessor_descriptor((slot.attributes() | ACCESSOR) & !CUSTOM_ACCESSOR);
            let slot_base = slot.slot_base().and_then(JSObject::from_cell_id).expect("slot.slotBase() de CustomAccessor");
            let slot_base_structure = slot_base.structure();
            let Some(slot_base_global_object) = slot_base_structure.realm() else {
                panic!("slotBase de CustomAccessor sem realm na Structure: classe {}", slot_base_structure.class_info().class_name);
            };
            let vm = slot_base_global_object.vm();
            self.set_getter(create_custom_getter_function(vm, &slot_base_global_object, property_name, slot.custom_getter()).as_value());
            if let Some(custom_setter) = slot.custom_setter() {
                self.set_setter(create_custom_setter_function(vm, &slot_base_global_object, property_name, custom_setter).as_value());
            }
        } else {
            self.set_descriptor(slot.get_value_for(property_name), slot.attributes());
        }
    }

    /// `equalTo(globalObject, other)`.
    pub fn equal_to(&self, other: &PropertyDescriptor) -> bool {
        if other.value.is_empty() != self.value.is_empty()
            || other.getter.is_empty() != self.getter.is_empty()
            || other.setter.is_empty() != self.setter.is_empty()
        {
            return false;
        }
        if !self.value.is_empty() && !same_value(other.value, self.value) {
            return false;
        }
        (self.getter.is_empty() || strict_equal(other.getter, self.getter))
            && (self.setter.is_empty() || strict_equal(other.setter, self.setter))
            && self.attributes_equal(other)
    }

    /// `setAccessorDescriptor(unsigned attributes)`.
    pub fn set_accessor_descriptor(&mut self, attributes: u32) {
        debug_assert!(attributes & ACCESSOR != 0);
        debug_assert!(attributes & CUSTOM_ACCESSOR_OR_VALUE == 0);
        self.attributes = attributes & !READ_ONLY;
        self.getter = JSValue::undefined();
        self.setter = JSValue::undefined();
        self.seen_attributes = ENUMERABLE_PRESENT | CONFIGURABLE_PRESENT;
    }

    /// `setWritable(bool)`.
    pub fn set_writable(&mut self, writable: bool) {
        if writable {
            self.attributes &= !READ_ONLY;
        } else {
            self.attributes |= READ_ONLY;
        }
        self.seen_attributes |= WRITABLE_PRESENT;
    }

    /// `setEnumerable(bool)`.
    pub fn set_enumerable(&mut self, enumerable: bool) {
        if enumerable {
            self.attributes &= !DONT_ENUM;
        } else {
            self.attributes |= DONT_ENUM;
        }
        self.seen_attributes |= ENUMERABLE_PRESENT;
    }

    /// `setConfigurable(bool)`.
    pub fn set_configurable(&mut self, configurable: bool) {
        if configurable {
            self.attributes &= !DONT_DELETE;
        } else {
            self.attributes |= DONT_DELETE;
        }
        self.seen_attributes |= CONFIGURABLE_PRESENT;
    }

    /// `setValue(JSValue)`.
    pub fn set_value(&mut self, value: JSValue) {
        self.value = value;
    }

    /// `setSetter(JSValue)`.
    pub fn set_setter(&mut self, setter: JSValue) {
        self.setter = setter;
        self.attributes |= ACCESSOR;
        self.attributes &= !READ_ONLY;
    }

    /// `setGetter(JSValue)`.
    pub fn set_getter(&mut self, getter: JSValue) {
        self.getter = getter;
        self.attributes |= ACCESSOR;
        self.attributes &= !READ_ONLY;
    }

    /// `isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.value.is_empty() && self.getter.is_empty() && self.setter.is_empty() && self.seen_attributes == 0
    }

    /// `writablePresent()`.
    pub fn writable_present(&self) -> bool {
        self.seen_attributes & WRITABLE_PRESENT != 0
    }

    /// `enumerablePresent()`.
    pub fn enumerable_present(&self) -> bool {
        self.seen_attributes & ENUMERABLE_PRESENT != 0
    }

    /// `configurablePresent()`.
    pub fn configurable_present(&self) -> bool {
        self.seen_attributes & CONFIGURABLE_PRESENT != 0
    }

    /// `setterPresent()`.
    pub fn setter_present(&self) -> bool {
        !self.setter.is_empty()
    }

    /// `getterPresent()`.
    pub fn getter_present(&self) -> bool {
        !self.getter.is_empty()
    }

    /// `attributesEqual(const PropertyDescriptor&)`.
    pub fn attributes_equal(&self, other: &PropertyDescriptor) -> bool {
        let mismatch = other.attributes ^ self.attributes;
        let shared_seen = other.seen_attributes & self.seen_attributes;
        if shared_seen & WRITABLE_PRESENT != 0 && mismatch & READ_ONLY != 0 {
            return false;
        }
        if shared_seen & CONFIGURABLE_PRESENT != 0 && mismatch & DONT_DELETE != 0 {
            return false;
        }
        if shared_seen & ENUMERABLE_PRESENT != 0 && mismatch & DONT_ENUM != 0 {
            return false;
        }
        true
    }

    /// `attributesOverridingCurrent(const PropertyDescriptor&)`.
    pub fn attributes_overriding_current(&self, current: &PropertyDescriptor) -> u32 {
        let mut current_attributes = current.attributes;
        if self.is_data_descriptor() && current.is_accessor_descriptor() {
            current_attributes |= READ_ONLY;
        }
        let mut override_mask = 0;
        if self.writable_present() {
            override_mask |= READ_ONLY;
        }
        if self.enumerable_present() {
            override_mask |= DONT_ENUM;
        }
        if self.configurable_present() {
            override_mask |= DONT_DELETE;
        }
        if self.is_accessor_descriptor() {
            override_mask |= ACCESSOR;
        }
        (self.attributes & override_mask) | (current_attributes & !override_mask & !CUSTOM_ACCESSOR)
    }
}

/// `toPropertyDescriptor(JSValue, JSValue, JSValue, DefinePropertyAttributes)`.
pub fn to_property_descriptor(value: JSValue, getter: JSValue, setter: JSValue, attributes: DefinePropertyAttributes) -> PropertyDescriptor {
    // We assume that validation is already done.
    let mut descriptor = PropertyDescriptor::default();
    if let Some(enumerable) = attributes.enumerable() {
        descriptor.set_enumerable(enumerable);
    }
    if let Some(configurable) = attributes.configurable() {
        descriptor.set_configurable(configurable);
    }
    if attributes.has_value() {
        descriptor.set_value(value);
    }
    if let Some(writable) = attributes.writable() {
        descriptor.set_writable(writable);
    }
    if attributes.has_get() {
        descriptor.set_getter(getter);
    }
    if attributes.has_set() {
        descriptor.set_setter(setter);
    }
    descriptor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_empty_and_generic() {
        let descriptor = PropertyDescriptor::default();
        assert!(descriptor.is_empty() && descriptor.is_generic_descriptor());
        assert!(!descriptor.configurable() && !descriptor.enumerable());
    }

    #[test]
    fn setters_track_seen_attributes() {
        let mut descriptor = PropertyDescriptor::default();
        descriptor.set_configurable(true);
        assert!(descriptor.configurable() && descriptor.configurable_present() && !descriptor.writable_present());
        let data = PropertyDescriptor::new(JSValue::undefined(), 0);
        assert!(data.is_data_descriptor() && data.writable() && data.attributes_equal(&data));
    }
}
