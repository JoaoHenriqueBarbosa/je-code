//! Porte de `runtime/JSPropertyNameEnumerator.{h,cpp}` e de `JSPropertyNameEnumeratorInlines.h`: a célula
//! imutável que o `for-in` percorre (`op_get_property_enumerator` a cria, `op_enumerator_next` a consome).
//!
//! DIVERGÊNCIAS:
//!
//! - É uma célula do `cell_registry` (`CellEntry::PropertyNameEnumerator`), valor imutável por `Rc`, com o
//!   `JSType` `CellType` do cabeçalho `JSCell`. O `vm.emptyPropertyNameEnumerator()` é a
//!   `LinkTimeConstant::EmptyPropertyNameEnumerator` do realm (posta por `js_global_object_init`), porque o
//!   bytecode do `for-in` a compara por identidade.
//! - Sem `m_cachedStructureID`, `m_cachedInlineCapacity` nem o cache do `StructureRareData`
//!   (`cachedPropertyNameEnumerator`, `normalizePrototypeChain`): o porte não tem a travessia rápida de
//!   propriedades por estrutura (`canAccessPropertiesQuicklyForEnumeration`), então a célula nasce sempre pelo
//!   ramo que o C++ toma quando essa travessia não vale (`indexedLength = 0`, `numberStructureProperties = 0`):
//!   só `GenericMode`. O resultado observável é o mesmo, o `OwnStructureMode` é só um atalho de cache. Os
//!   campos `indexed_length` e `end_structure_property_index` continuam em `create` para o dia em que o
//!   atalho entrar.
//! - `getEnumerablePropertyNames` usa o `getOwnPropertyNames` de `own_property_names` no lugar do
//!   `methodTable()->getOwnPropertyNames` (o `JSObject` do porte não tem a tabela de métodos virtual), e o
//!   `hasEnumerableProperty` de `JSObject.cpp` mora aqui como função livre, porque só o enumerador o usa.

use std::rc::Rc;

use crate::bytecode::bytecode_intrinsics_table::LinkTimeConstant;
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::enumeration_mode::{DontEnumPropertiesMode, PrivateSymbolMode, PropertyNameMode};
use crate::runtime::host_call::Thrown;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_string::{js_string, JSStringRef};
use crate::runtime::js_type::JSType;
use crate::runtime::js_value::JSValue;
use crate::runtime::proxy_object::object_get_own_property_names;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_name_array::PropertyNameArrayBuilder;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_type_info::{TypeInfo, STRUCTURE_IS_IMMORTAL};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo JSPropertyNameEnumerator::s_info`.
pub static PROPERTY_NAME_ENUMERATOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "JSPropertyNameEnumerator", parent_class: None, static_prop_hash_table: None, inherits_js_type_range: None };

/// `JSPropertyNameEnumerator::InitMode`.
pub const INIT_MODE: u8 = 0;
/// `JSPropertyNameEnumerator::IndexedMode`.
pub const INDEXED_MODE: u8 = 1 << 0;
/// `JSPropertyNameEnumerator::OwnStructureMode`.
pub const OWN_STRUCTURE_MODE: u8 = 1 << 1;
/// `JSPropertyNameEnumerator::GenericMode`.
pub const GENERIC_MODE: u8 = 1 << 2;

/// `JSObject::maximumPrototypeChainDepth`.
const MAXIMUM_PROTOTYPE_CHAIN_DEPTH: u32 = 40000;

/// `class JSPropertyNameEnumerator`.
#[derive(Debug)]
pub struct JSPropertyNameEnumerator {
    /// `m_propertyNames`.
    property_names: Vec<JSStringRef>,
    cell_id: usize,
    /// `JSCell::m_structureID`: a `vm.propertyNameEnumeratorStructure`.
    structure: StructureRef,
    /// `m_indexedLength`.
    indexed_length: u32,
    /// `m_endStructurePropertyIndex`.
    end_structure_property_index: u32,
    /// `m_flags`.
    flags: u8,
}

/// O `JSPropertyNameEnumerator*`.
pub type JSPropertyNameEnumeratorRef = Rc<JSPropertyNameEnumerator>;

impl JSPropertyNameEnumerator {
    /// `JSPropertyNameEnumerator::createStructure(vm, globalObject, prototype)`: `TypeInfo(CellType, StructureFlags)`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(vm, global_object, prototype, TypeInfo::new(JSType::CellType, STRUCTURE_IS_IMMORTAL), &PROPERTY_NAME_ENUMERATOR_S_INFO)
    }

    /// `JSCell::structure()`.
    pub fn structure(&self) -> &StructureRef {
        &self.structure
    }

    /// `JSPropertyNameEnumerator::create(vm, structure, indexedLength, numberStructureProperties, propertyNames)`
    /// (a `structure` é a `vm.propertyNameEnumeratorStructure`).
    pub fn create(
        vm: &VM,
        indexed_length: u32,
        number_structure_properties: u32,
        property_names: Vec<JSStringRef>,
    ) -> JSPropertyNameEnumeratorRef {
        let end_generic_property_index = property_names.len() as u32;
        let mut flags = 0;
        if indexed_length != 0 {
            flags |= INDEXED_MODE;
        }
        if number_structure_properties != 0 {
            flags |= OWN_STRUCTURE_MODE;
        }
        if end_generic_property_index - number_structure_properties != 0 {
            flags |= GENERIC_MODE;
        }

        let cell_id = cell_registry::reserve();
        let cell = Rc::new(JSPropertyNameEnumerator {
            property_names,
            cell_id,
            structure: vm.property_name_enumerator_structure(),
            indexed_length,
            end_structure_property_index: number_structure_properties,
            flags,
        });
        cell_registry::set(cell_id, CellEntry::PropertyNameEnumerator(Rc::clone(&cell)));
        cell
    }

    /// O `JSPropertyNameEnumerator*` de um `JSValue` (`None` se não é a célula de um enumerador).
    pub fn from_value(value: &JSValue) -> Option<JSPropertyNameEnumeratorRef> {
        if !value.is_cell() {
            return None;
        }
        match cell_registry::get(value.as_cell()) {
            Some(CellEntry::PropertyNameEnumerator(cell)) => Some(cell),
            _ => None,
        }
    }

    /// `JSType::CellType`: o cabeçalho `JSCell` do enumerador.
    pub fn js_type(&self) -> JSType {
        JSType::CellType
    }

    /// A célula como `JSValue`.
    pub fn as_value(&self) -> JSValue {
        JSValue::from_cell(self.cell_id)
    }

    /// `indexedLength()`.
    pub fn indexed_length(&self) -> u32 {
        self.indexed_length
    }

    /// `endStructurePropertyIndex()`.
    pub fn end_structure_property_index(&self) -> u32 {
        self.end_structure_property_index
    }

    /// `sizeOfPropertyNames()` (`endGenericPropertyIndex()`).
    pub fn size_of_property_names(&self) -> u32 {
        self.property_names.len() as u32
    }

    /// `flags()`.
    pub fn flags(&self) -> u8 {
        self.flags
    }

    /// `propertyNameAtIndex(index)`: `None` é o `nullptr` além do fim.
    pub fn property_name_at_index(&self, index: u32) -> Option<JSStringRef> {
        self.property_names.get(index as usize).cloned()
    }

    /// `computeNext(globalObject, base, index, mode)`: o próximo nome do `for-in`, `None` quando acabou.
    /// `mode` e `index` são os registradores do `op_enumerator_next`, atualizados no lugar.
    pub fn compute_next(
        &self,
        global_object: &JSGlobalObject,
        base: &ObjectRef,
        index: &mut u32,
        mode: &mut u8,
    ) -> Result<Option<JSStringRef>, Thrown> {
        let vm = global_object.vm();
        debug_assert!(self.indexed_length() != 0 || self.size_of_property_names() != 0);

        *index += 1;
        if *mode == INIT_MODE {
            *mode = INDEXED_MODE;
            *index = 0;
        }

        if *mode == INDEXED_MODE {
            while *index < self.indexed_length() && !has_enumerable_property(vm, base, |slot| {
                base.get_property_slot_by_index(vm, *index, slot)
            })? {
                *index += 1;
            }

            if *index < self.indexed_length() {
                return Ok(Some(js_string(vm, Identifier::from_u32(vm, *index).string().string())));
            }

            if self.size_of_property_names() == 0 {
                return Ok(None);
            }

            *mode = OWN_STRUCTURE_MODE;
            *index = 0;
        }

        // `OwnStructureMode` e `GenericMode`. O ramo `index < endStructurePropertyIndex() && base->structureID()
        // == cachedStructureID()` não existe (ver o cabeçalho): sempre confere `hasEnumerableProperty`.
        let mut name = None;
        loop {
            if *index >= self.size_of_property_names() {
                break;
            }
            let Some(candidate) = self.property_name_at_index(*index) else { break };
            let identifier = Identifier::from_string(vm, &candidate.value());
            let property_name = PropertyName::from_identifier(&identifier);
            if has_enumerable_property(vm, base, |slot| base.get_property_slot(global_object, &property_name, slot))? {
                name = Some(candidate);
                break;
            }
            *index += 1;
        }

        if *index >= self.end_structure_property_index() && *index < self.size_of_property_names() {
            *mode = GENERIC_MODE;
        }
        Ok(name)
    }
}

/// `JSObject::hasEnumerableProperty(globalObject, propertyName)` e a versão por índice: `find` é o
/// `getPropertySlot` que preenche o `PropertySlot` de `GetOwnProperty`.
fn has_enumerable_property(
    vm: &crate::runtime::vm::VM,
    base: &JSObject,
    find: impl FnOnce(&mut PropertySlot) -> bool,
) -> Result<bool, Thrown> {
    let mut slot = PropertySlot::new(base.as_value(), InternalMethodType::GetOwnProperty);
    let has_property = find(&mut slot);
    if vm.exception().is_some() {
        return Err(Thrown::Pending);
    }
    if !has_property {
        return Ok(false);
    }
    Ok(slot.attributes() & DONT_ENUM == 0
        || slot.slot_base().and_then(JSObject::from_cell_id).is_some_and(|slot_base| {
            slot_base.structure().type_info().get_own_property_slot_may_be_wrong_about_dont_enum()
        }))
}

/// `getEnumerablePropertyNames(globalObject, base, propertyNames, indexedLength, structurePropertyCount)`,
/// o ramo que não é a travessia rápida por estrutura: os nomes próprios de `base` e de cada protótipo.
fn get_enumerable_property_names(
    global_object: &JSGlobalObject,
    base: &ObjectRef,
    property_names: &mut PropertyNameArrayBuilder<'_>,
) -> Result<(), Thrown> {
    get_property_names_with(global_object, base, property_names, |object, names| {
        // This ensures Proxy's [[GetOwnProperty]] trap is invoked only once per property, by OpHasEnumerableProperty.
        let mode = if object.type_() == JSType::ProxyObjectType {
            DontEnumPropertiesMode::Include
        } else {
            DontEnumPropertiesMode::Exclude
        };
        object_get_own_property_names(global_object, object, names, mode)
    })
}

/// A travessia de `JSObject::getPropertyNames`: os nomes próprios de `base` e de cada protótipo, cada objeto pelo
/// `own_property_names` dado (o `getEnumerablePropertyNames` muda o modo para o `Proxy`; o `getPropertyNames` comum
/// usa o mesmo modo em todos).
pub(crate) fn get_property_names_with(
    global_object: &JSGlobalObject,
    base: &ObjectRef,
    property_names: &mut PropertyNameArrayBuilder<'_>,
    own_property_names: impl Fn(&JSObject, &mut PropertyNameArrayBuilder<'_>) -> Result<(), Thrown>,
) -> Result<(), Thrown> {
    own_property_names(base, property_names)?;

    let mut object = base.clone();
    let mut prototype_count = 0;
    loop {
        let prototype = object.get_prototype(global_object)?;
        if prototype.is_null() {
            return Ok(());
        }

        prototype_count += 1;
        if prototype_count > MAXIMUM_PROTOTYPE_CHAIN_DEPTH {
            return Err(Thrown::StackOverflow);
        }

        object = ObjectRef::from_value(&prototype)
            .expect("asObject(prototype): o protótipo não nulo de um objeto é um objeto");
        own_property_names(&object, property_names)?;
    }
}

/// `propertyNameEnumerator(globalObject, base)`.
pub fn property_name_enumerator(
    global_object: &JSGlobalObject,
    base: &ObjectRef,
) -> Result<JSPropertyNameEnumeratorRef, Thrown> {
    let vm = global_object.vm();
    let mut property_names = PropertyNameArrayBuilder::new(vm, PropertyNameMode::Strings, PrivateSymbolMode::Exclude);
    get_enumerable_property_names(global_object, base, &mut property_names)?;

    let names: Vec<JSStringRef> = property_names
        .release_data()
        .property_name_vector()
        .iter()
        .map(|identifier| js_string(vm, identifier.string().string()))
        .collect();
    if names.is_empty() {
        return Ok(empty_property_name_enumerator(global_object));
    }
    Ok(JSPropertyNameEnumerator::create(vm, 0, 0, names))
}

/// `vm.emptyPropertyNameEnumerator()`: a célula compartilhada que o `for-in` compara por identidade
/// (`emitJumpIfEmptyPropertyNameEnumerator`), guardada como `LinkTimeConstant` no realm pelo `init`.
pub fn empty_property_name_enumerator(global_object: &JSGlobalObject) -> JSPropertyNameEnumeratorRef {
    JSPropertyNameEnumerator::from_value(&global_object.link_time_constant(LinkTimeConstant::EmptyPropertyNameEnumerator))
        .expect("LinkTimeConstant::EmptyPropertyNameEnumerator sem JSPropertyNameEnumerator")
}
