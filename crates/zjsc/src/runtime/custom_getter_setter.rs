//! Tradução de `runtime/CustomGetterSetter.h` e `CustomGetterSetter.cpp`: o par de funções nativas
//! (`GetValueFunc`, `PutValueFunc`) de uma propriedade `CustomAccessor` ou `CustomValue`, guardado como
//! valor da propriedade no armazenamento do objeto (`JSObject::putDirectCustomAccessor`).
//!
//! DIVERGÊNCIAS (sem heap e sem GC, camada 3):
//!
//! - É uma célula do `cell_registry` (`CellEntry::CustomGetterSetter`), como `GetterSetter`: valor
//!   imutável por `Rc`, `cell_id` do registro, `JSType` (`CustomGetterSetterType`) vindo da variante. A
//!   `vm.customGetterSetterStructure` não existe ainda, e os `getOwnPropertySlot`/`put`/... estáticos que o
//!   C++ marca `RELEASE_ASSERT_NOT_REACHED` não têm o que traduzir.
//! - `GetValueFunc` e `PutValueFunc` são ponteiros de função do Rust (`property_slot.rs`); o setter nulo
//!   do C++ é `None`. O getter nunca é nulo (`ASSERT(getValue)` em `PropertySlot::setCustom`).
//! - `DOMAttributeGetterSetter` (subclasse de DOM) não existe: o ramo `inherits<DOMAttributeGetterSetter>`
//!   de `fillCustomGetterPropertySlot` fica de fora.
//!
//! Neste módulo moram também os dois `JSObject::putDirectCustom*` (o C++ os declara em `JSObject.h`):
//! `putDirectCustomAccessor` e `putDirectCustomGetterSetterWithoutTransition`.

use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::js_object::{JSObject, PutMode};
use crate::runtime::js_value::JSValue;
use crate::runtime::property_attribute::{CUSTOM_ACCESSOR, CUSTOM_ACCESSOR_OR_VALUE, CUSTOM_VALUE, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::{GetValueFunc, PutValueFunc};
use crate::runtime::put_property_slot::{PutContext, PutPropertySlot, PutType};
use crate::runtime::vm::VM;

/// `class CustomGetterSetter`.
#[derive(Debug)]
pub struct CustomGetterSetter {
    getter: GetValueFunc,
    /// `m_setter`: `None` é o `nullptr`.
    setter: Option<PutValueFunc>,
    cell_id: usize,
}

/// A célula como o resto do porte a enxerga.
pub type CustomGetterSetterRef = Rc<CustomGetterSetter>;

impl CustomGetterSetter {
    /// `create(vm, customGetter, customSetter)`.
    pub fn create(_vm: &VM, getter: GetValueFunc, setter: Option<PutValueFunc>) -> CustomGetterSetterRef {
        let cell_id = cell_registry::reserve();
        let cell = Rc::new(CustomGetterSetter { getter, setter, cell_id });
        cell_registry::set(cell_id, CellEntry::CustomGetterSetter(Rc::clone(&cell)));
        cell
    }

    /// Procura a célula pelo `cell_id` guardado em `JSValue::Cell` (o `uncheckedDowncast<CustomGetterSetter>`).
    pub fn from_cell_id(cell_id: usize) -> Option<CustomGetterSetterRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::CustomGetterSetter(cell)) => Some(cell),
            _ => None,
        }
    }

    /// O `CustomGetterSetter*` de um `JSValue` (`None` se não é a célula de um `CustomGetterSetter`).
    pub fn from_value(value: &JSValue) -> Option<CustomGetterSetterRef> {
        if !value.is_cell() {
            return None;
        }
        CustomGetterSetter::from_cell_id(value.as_cell())
    }

    /// O "endereço" da célula, o que o `JSValue` codifica.
    pub fn cell_id(&self) -> usize {
        self.cell_id
    }

    /// A célula como `JSValue` (o valor guardado na propriedade).
    pub fn as_value(&self) -> JSValue {
        JSValue::from_cell(self.cell_id)
    }

    /// `getter()`.
    pub fn getter(&self) -> GetValueFunc {
        self.getter
    }

    /// `setter()`: `None` é o `nullptr`.
    pub fn setter(&self) -> Option<PutValueFunc> {
        self.setter
    }
}

impl JSObject {
    /// `putDirectCustomAccessor(vm, propertyName, value, attributes)`: o `value` é a célula do
    /// `CustomGetterSetter`. Sem `CustomAccessor` nos atributos a propriedade é um `CustomValue` (o C++
    /// usa este método para os dois, ver o FIXME dele).
    pub fn put_direct_custom_accessor(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        custom: &CustomGetterSetter,
        mut attributes: u32,
    ) -> bool {
        debug_assert!(property_name.parse_index().is_none());
        if attributes & CUSTOM_ACCESSOR == 0 {
            attributes |= CUSTOM_VALUE;
        }

        let mut slot = PutPropertySlot::new(self.as_value(), false, PutContext::UnknownContext, false);
        let result = self
            .put_direct_internal(vm, property_name, custom.as_value(), attributes, &mut slot, PutMode::PutModeDefineOwnProperty)
            .is_none();

        debug_assert!(slot.type_() == PutType::NewProperty);

        let structure = self.structure();
        if attributes & READ_ONLY != 0 {
            structure.set_contains_read_only_properties();
        }
        structure
            .set_has_any_kind_of_getter_setter_properties_with_proto_check(*property_name == vm.property_names.underscore_proto);
        result
    }

    /// `putDirectCustomGetterSetterWithoutTransition(vm, propertyName, value, attributes)`.
    pub fn put_direct_custom_getter_setter_without_transition(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        custom: &CustomGetterSetter,
        attributes: u32,
    ) {
        debug_assert!(property_name.parse_index().is_none());
        debug_assert!(attributes & CUSTOM_ACCESSOR_OR_VALUE != 0);

        let structure = self.structure();
        let offset = self.prepare_to_put_direct_without_transition(vm, property_name, attributes, &structure);
        self.put_direct_offset(vm, offset, custom.as_value());

        if attributes & READ_ONLY != 0 {
            structure.set_contains_read_only_properties();
        }
        structure
            .set_has_any_kind_of_getter_setter_properties_with_proto_check(*property_name == vm.property_names.underscore_proto);
    }
}
