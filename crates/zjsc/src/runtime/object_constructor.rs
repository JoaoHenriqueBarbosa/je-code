//! Porte de `runtime/ObjectConstructor.h` e `ObjectConstructor.cpp`: o construtor `Object` (um
//! `InternalFunction`) e as funções estáticas `Object.keys`, `values`, `entries`, `assign`, `create`,
//! `defineProperty`/`defineProperties`, `getOwnPropertyNames`/`Symbols`/`Descriptor(s)`,
//! `getPrototypeOf`/`setPrototypeOf`, `freeze`/`seal`/`preventExtensions` e os `is*`, `is` e `hasOwn`.
//!
//! O corpo de cada função é uma função comum sobre `HostCall` (ver `host_call.rs`); o `host_function!`
//! gera o `JSC_DEFINE_HOST_FUNCTION`.
//!
//! DIVERGÊNCIAS e LACUNAS, e por quê:
//! - A tabela estática (`ObjectConstructor.lut.h`) é `OBJECT_CONSTRUCTOR_TABLE`, reificada no primeiro
//!   acesso (`lookup.rs`); `finishCreation` põe só `prototype`, as privadas, `hasOwn` e `groupBy`.
//! - Só o caminho genérico das funções: os atalhos do C++ por estrutura (`canPerformFastPropertyEnumeration`,
//!   `cachedPropertyNames`, `propertyDescriptorFastPathWatchpointSet`, o lote de `Object.assign`) são
//!   otimização de desempenho com a mesma semântica, e dependem de `StructureRareData` e watchpoints.
//!   `constructObjectFromPropertyDescriptor` usa só o caminho "Slow", que produz a mesma ordem de chaves
//!   que o caminho rápido.
//! - `preventExtensions`, `seal` e `freeze` de objeto com propriedades indexadas convertem para
//!   `ArrayStorage` em modo esparso (`JSObject::enter_dictionary_indexing_mode`, em
//!   `js_object_array_storage.rs`), como o C++. O `[[DefineOwnProperty]]` do `Array` (`length`) é o de
//!   `JSArray::define_own_property`, despachado à mão por `define_own_property_of` (o `JSObject` do
//!   porte não tem a tabela de métodos virtual). `JSFunction` materializa `name`/`length`/`prototype` pelo
//!   `getOwnPropertySlot` (`own_descriptor`) e pelo `getOwnSpecialPropertyNames` (`own_names`).
//! - `StringObject` sobrescreve `getOwnPropertySlot`/`defineOwnProperty` (caracteres e `length`): o
//!   `getOwnPropertySlot` é o de `string_object.rs`; o `defineOwnProperty` ainda é o da base.
//! - Consultas de propriedade própria em `JSArray` (`length`) e `StringObject` (caracteres) são despachadas
//!   à mão em `own_descriptor`, pois o `JSObject` do porte ainda não tem a tabela de métodos virtual.

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::enumeration_mode::{DontEnumPropertiesMode, PrivateSymbolMode, PropertyNameMode};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::indexing_type::has_indexed_properties;
use crate::runtime::internal_function::{
    get_function_realm, InternalFunction, InternalFunctionRef, PropertyAdditionMode, INTERNAL_FUNCTION_S_INFO,
};
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_array::{construct_array, JSArray};
use crate::runtime::builtins_source::BuiltinCodeIndex;
use crate::runtime::js_function::{put_direct_builtin_function_without_transition, put_direct_native_function_without_transition};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSFinalObject, JSObject, JSObjectRef};
use crate::runtime::js_string::js_owned_string;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::lookup::{native_entry_with_intrinsic};
use crate::runtime::js_value::{js_boolean, js_undefined, JSValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::operations::same_value;
use crate::runtime::own_property_names::get_own_property_names_with_special;
use crate::runtime::property_attribute::{BUILTIN, DONT_DELETE, DONT_ENUM, FUNCTION, READ_ONLY};
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_name_array::PropertyNameArrayBuilder;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::proxy_object::{
    object_define_own_property, object_get, object_get_own_property_descriptor, object_get_own_property_names,
    object_has_property, object_is_extensible, object_prevent_extensions, object_set, object_set_prototype,
};
use crate::runtime::put_property_slot::{PutContext, PutPropertySlot};
use crate::runtime::sparse_array_value_map::PutDirectIndexMode;
use crate::runtime::string_object::StringObject;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::symbol::Symbol;
use crate::runtime::vm::VM;

/// `const ClassInfo ObjectConstructor::s_info` (`"Function"`).
pub static OBJECT_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo {
        class_name: "Function",
        parent_class: Some(&INTERNAL_FUNCTION_S_INFO),
        static_prop_hash_table: Some(&OBJECT_CONSTRUCTOR_TABLE),
        inherits_js_type_range: None,
    };

/// `PrototypeValueCanOnlyBeAnObjectOrNullTypeError` (`JSObject.cpp`).
const PROTOTYPE_VALUE_CAN_ONLY_BE_AN_OBJECT_OR_NULL_TYPE_ERROR: &str = "Prototype value can only be an object or null";
use crate::runtime::error_messages::READONLY_PROPERTY_WRITE_ERROR;

// ---------------------------------------------------------------------------------------------
// Auxiliares do C++ que o resto do porte ainda não tem.
// ---------------------------------------------------------------------------------------------

/// `JSValue::toObject(globalObject)`: a exceção já está pendente quando falha.
fn to_object(global_object: &JSGlobalObject, value: JSValue) -> Result<ObjectRef, Thrown> {
    value.to_object(global_object).ok_or(Thrown::Pending)
}

/// `JSValue::toPropertyKey(globalObject)`.
fn to_property_key(global_object: &JSGlobalObject, value: JSValue) -> Result<Identifier, Thrown> {
    value.to_property_key(global_object).ok_or(Thrown::Pending)
}

/// A chave de propriedade (`Identifier`) como o `JSValue` que `Object.keys` devolve: texto ou `Symbol`.
pub(crate) fn identifier_to_js_value(vm: &VM, identifier: &Identifier) -> JSValue {
    if identifier.is_symbol() {
        let key = identifier.impl_().expect("símbolo sem StringImpl");
        return Symbol::for_key(vm, &key).to_primitive();
    }
    JSValue::from_js_string(js_owned_string(vm, identifier.string().string()))
}

/// `constructEmptyObject(globalObject)` (`JSFinalObject::createDefaultEmptyObject`).
pub(crate) fn construct_empty_object(global_object: &JSGlobalObject) -> JSObjectRef {
    JSFinalObject::create(global_object.vm(), &global_object.object_structure_for_object_constructor())
}

/// `constructEmptyArray(globalObject, ...)` com os valores já prontos.
pub fn construct_array_of(global_object: &JSGlobalObject, values: &[JSValue]) -> JSArray {
    construct_array(global_object.vm(), &global_object.array_structure(), values)
}

/// O `PropertyNameArrayBuilder` cheio por `getOwnPropertyNames`, devolvido como lista de `Identifier`.
/// A função acrescenta `length`, `name` e `prototype` antes das propriedades da estrutura
/// (`JSFunction::getOwnSpecialPropertyNames`); o `Proxy` responde pelo trap `ownKeys`.
pub(crate) fn own_names(
    global_object: &JSGlobalObject,
    object: &JSObject,
    property_name_mode: PropertyNameMode,
    dont_enum_properties_mode: DontEnumPropertiesMode,
) -> Result<Vec<Identifier>, Thrown> {
    let vm = global_object.vm();
    let mut properties = PropertyNameArrayBuilder::new(vm, property_name_mode, PrivateSymbolMode::Exclude);
    match object.as_value().as_js_function() {
        Some(function) => get_own_property_names_with_special(vm, object, &mut properties, dont_enum_properties_mode, |names| {
            function.get_own_special_property_names(global_object, names, dont_enum_properties_mode)
        })?,
        None => object_get_own_property_names(global_object, object, &mut properties, dont_enum_properties_mode)?,
    }
    Ok(properties.iter().cloned().collect())
}

/// `object->getOwnPropertyDescriptor(globalObject, propertyName, descriptor)` com o despacho à mão das
/// classes que sobrescrevem `getOwnPropertySlot` (ver o cabeçalho); o `Proxy` responde pelo trap
/// `getOwnPropertyDescriptor`.
pub(crate) fn own_descriptor(
    global_object: &JSGlobalObject,
    object: &JSObject,
    property_name: &PropertyName,
) -> Result<Option<PropertyDescriptor>, Thrown> {
    if object.type_() == JSType::ProxyObjectType {
        return object_get_own_property_descriptor(global_object, object, property_name);
    }
    let vm = global_object.vm();
    let mut descriptor = PropertyDescriptor::default();
    let mut slot = PropertySlot::new(object.as_value(), InternalMethodType::GetOwnProperty);
    let found = if let Some(array) = JSArray::from_value_by_class(&object.as_value()) {
        array.get_own_property_slot(vm, property_name, &mut slot)
    } else if let Some(function) = object.as_value().as_js_function() {
        // `JSFunction::getOwnPropertySlot` materializa `name`, `length` e `prototype`; a exceção da
        // materialização fica pendente no `VM`.
        let found = function.get_own_property_slot(global_object, property_name, &mut slot);
        if vm.exception().is_some() {
            return Err(Thrown::Pending);
        }
        found
    } else if let Some(string_object) = StringObject::from_cell_id(object.cell_id()) {
        string_object.get_own_property_slot(vm, property_name, &mut slot)
    } else {
        // O `getOwnPropertySlot` do namespace de módulo lança o `ReferenceError` do binding em TDZ e devolve falso: a
        // exceção pendente aborta a busca em vez de virar "não existe".
        let found = object.get_own_property_descriptor(vm, property_name, &mut descriptor);
        if vm.exception().is_some() {
            return Err(Thrown::Pending);
        }
        return Ok(found.then_some(descriptor));
    };
    if !found {
        return Ok(None);
    }
    descriptor.set_property_slot(&slot, property_name);
    Ok(Some(descriptor))
}

/// `getIfPropertyExists(globalObject, propertyName)`: `None` quando a propriedade não existe.
fn get_if_property_exists(global_object: &JSGlobalObject, object: &JSObject, property_name: &PropertyName) -> Result<Option<JSValue>, Thrown> {
    if !object_has_property(global_object, object, property_name)? {
        return Ok(None);
    }
    object_get(global_object, object, property_name, object.as_value()).map(Some)
}

/// `putOwnDataPropertyMayBeIndex(globalObject, propertyName, value, slot)`.
pub(crate) fn put_own_data_property_may_be_index(
    vm: &VM,
    object: &JSObject,
    property_name: &PropertyName,
    value: JSValue,
) -> Result<(), Thrown> {
    match property_name.parse_index() {
        // `putDirectIndex(globalObject, index, value, 0, PutDirectIndexLikePutDirect)`: não consulta setters
        // de índice na cadeia de protótipos (o `put_by_index` consultaria).
        Some(index) => {
            object.put_direct_index(vm, index, value, 0, PutDirectIndexMode::PutDirectIndexLikePutDirect)?;
        }
        None => {
            object.put_direct(vm, property_name, value, 0);
        }
    }
    Ok(())
}

/// `object->methodTable()->preventExtensions(object, globalObject)`: a de `JSObject` não lança nem falha
/// (`Proxy`, que sobrescreve, não existe no porte), mas os chamadores já tratam o `Result` do método virtual.
pub(crate) fn prevent_extensions(vm: &VM, object: &JSObject) -> Result<(), Thrown> {
    if let Some(thrown) = crate::runtime::js_web_assembly_gc_object::prevent_extensions(object) {
        return Err(thrown);
    }
    object.prevent_extensions(vm);
    Ok(())
}

/// `object->defineOwnProperty(globalObject, propertyName, descriptor, throwException)`: o `Array` tem o
/// `[[DefineOwnProperty]]` próprio (`length`, e a checagem de `length` gravável nos índices); o `Proxy`, a
/// `JSFunction` e os demais seguem o despacho de `proxy_object.rs`.
pub(crate) fn define_own_property_of(
    global_object: &JSGlobalObject,
    object: &JSObject,
    property_name: &PropertyName,
    descriptor: &PropertyDescriptor,
    throw_exception: bool,
) -> Result<bool, Thrown> {
    if let Some(array) = JSArray::from_value_by_class(&object.as_value()) {
        return Ok(array.define_own_property(global_object.vm(), property_name, descriptor, throw_exception)?);
    }
    object_define_own_property(global_object, object, property_name, descriptor, throw_exception)
}

/// `JSObject::setPrototypeWithCycleCheck(vm, globalObject, prototype, shouldThrowIfCantSet = true)`.
pub(crate) fn set_prototype_with_cycle_check(vm: &VM, object: &JSObject, prototype: JSValue) -> Result<(), Thrown> {
    if let Some(thrown) = crate::runtime::js_web_assembly_gc_object::set_prototype(object) {
        return Err(thrown);
    }
    if object.structure().type_info().is_immutable_prototype_exotic_object() {
        // https://tc39.github.io/ecma262/#sec-set-immutable-prototype
        if same_value(object.get_prototype_direct(), prototype) {
            return Ok(());
        }
        return Err(Thrown::type_error("Cannot set prototype of immutable prototype object"));
    }

    if same_value(object.get_prototype_direct(), prototype) {
        return Ok(());
    }
    if !object.is_structure_extensible() {
        return Err(Thrown::type_error(READONLY_PROPERTY_WRITE_ERROR));
    }
    if !prototype.is_object() && !prototype.is_null() {
        return Err(Thrown::type_error(PROTOTYPE_VALUE_CAN_ONLY_BE_AN_OBJECT_OR_NULL_TYPE_ERROR));
    }

    let mut next_prototype = prototype;
    while next_prototype.is_object() {
        if same_value(next_prototype, object.as_value()) {
            return Err(Thrown::type_error("cyclic __proto__ value"));
        }
        let next = next_prototype.as_object();
        if next.type_() == JSType::ProxyObjectType {
            break;
        }
        next_prototype = next.get_prototype_direct();
    }

    object.set_prototype_direct(vm, prototype);
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// ObjectConstructor.h: descritores, chaves, atribuição.
// ---------------------------------------------------------------------------------------------

/// `constructObjectFromPropertyDescriptor(globalObject, descriptor)` (https://tc39.es/ecma262/#sec-frompropertydescriptor),
/// o caminho `constructObjectFromPropertyDescriptorSlow`.
pub fn construct_object_from_property_descriptor(global_object: &JSGlobalObject, descriptor: &PropertyDescriptor) -> JSObjectRef {
    let vm = global_object.vm();
    let names = &vm.property_names;
    let result = construct_empty_object(global_object);
    if !descriptor.value().is_empty() {
        result.put_direct(vm, &PropertyName::from_identifier(&names.value), descriptor.value(), 0);
    }
    if descriptor.writable_present() {
        result.put_direct(vm, &PropertyName::from_identifier(&names.writable), js_boolean(descriptor.writable()), 0);
    }
    if descriptor.getter_present() {
        result.put_direct(vm, &PropertyName::from_identifier(&names.get), descriptor.getter(), 0);
    }
    if descriptor.setter_present() {
        result.put_direct(vm, &PropertyName::from_identifier(&names.set), descriptor.setter(), 0);
    }
    if descriptor.enumerable_present() {
        result.put_direct(vm, &PropertyName::from_identifier(&names.enumerable), js_boolean(descriptor.enumerable()), 0);
    }
    if descriptor.configurable_present() {
        result.put_direct(vm, &PropertyName::from_identifier(&names.configurable), js_boolean(descriptor.configurable()), 0);
    }
    result
}

/// `ownPropertyKeys(globalObject, object, propertyNameMode, dontEnumPropertiesMode)`.
pub fn own_property_keys(
    global_object: &JSGlobalObject,
    object: &JSObject,
    property_name_mode: PropertyNameMode,
    dont_enum_properties_mode: DontEnumPropertiesMode,
) -> Result<JSArray, Thrown> {
    let vm = global_object.vm();
    let properties = own_names(global_object, object, property_name_mode, dont_enum_properties_mode)?;
    let keys: Vec<JSValue> = properties.iter().map(|identifier| identifier_to_js_value(vm, identifier)).collect();
    Ok(construct_array_of(global_object, &keys))
}

/// `toPropertyDescriptor(globalObject, value, descriptor)` (o caminho genérico).
pub fn to_property_descriptor(global_object: &JSGlobalObject, value: JSValue) -> Result<PropertyDescriptor, Thrown> {
    let vm = global_object.vm();
    let Some(description) = ObjectRef::from_value(&value) else {
        return Err(Thrown::type_error("Property description must be an object."));
    };
    let description = &*description;
    let names = &vm.property_names;
    let lookup = |identifier: &Identifier| get_if_property_exists(global_object, description, &PropertyName::from_identifier(identifier));

    let mut descriptor = PropertyDescriptor::default();
    if let Some(enumerable) = lookup(&names.enumerable)? {
        descriptor.set_enumerable(enumerable.to_boolean());
    }
    if let Some(configurable) = lookup(&names.configurable)? {
        descriptor.set_configurable(configurable.to_boolean());
    }
    if let Some(value) = lookup(&names.value)? {
        descriptor.set_value(value);
    }
    if let Some(writable) = lookup(&names.writable)? {
        descriptor.set_writable(writable.to_boolean());
    }
    if let Some(getter) = lookup(&names.get)? {
        if !getter.is_undefined() && !getter.is_callable() {
            return Err(Thrown::type_error("Getter must be a function."));
        }
        descriptor.set_getter(getter);
    }
    if let Some(setter) = lookup(&names.set)? {
        if !setter.is_undefined() && !setter.is_callable() {
            return Err(Thrown::type_error("Setter must be a function."));
        }
        descriptor.set_setter(setter);
    }

    if !descriptor.is_accessor_descriptor() {
        return Ok(descriptor);
    }
    if !descriptor.value().is_empty() {
        return Err(Thrown::type_error("Invalid property.  'value' present on property with getter or setter."));
    }
    if descriptor.writable_present() {
        return Err(Thrown::type_error("Invalid property.  'writable' present on property with getter or setter."));
    }
    Ok(descriptor)
}

/// `objectConstructorGetOwnPropertyDescriptor(globalObject, object, propertyName)`.
pub fn object_constructor_get_own_property_descriptor_of(
    global_object: &JSGlobalObject,
    object: &JSObject,
    property_name: &Identifier,
) -> Result<JSValue, Thrown> {
    match own_descriptor(global_object, object, &PropertyName::from_identifier(property_name))? {
        Some(descriptor) => Ok(construct_object_from_property_descriptor(global_object, &descriptor).as_value()),
        None => Ok(js_undefined()),
    }
}

/// `objectConstructorGetOwnPropertyDescriptors(globalObject, object)`.
pub fn object_constructor_get_own_property_descriptors_of(
    global_object: &JSGlobalObject,
    object: &JSObject,
) -> Result<JSValue, Thrown> {
    let vm = global_object.vm();
    let properties = own_names(global_object, object, PropertyNameMode::StringsAndSymbols, DontEnumPropertiesMode::Include)?;
    let descriptors = construct_empty_object(global_object);
    for property_name in &properties {
        let name = PropertyName::from_identifier(property_name);
        let Some(descriptor) = own_descriptor(global_object, object, &name)? else { continue };
        let from_descriptor = construct_object_from_property_descriptor(global_object, &descriptor);
        put_own_data_property_may_be_index(vm, &descriptors, &name, from_descriptor.as_value())?;
    }
    Ok(descriptors.as_value())
}

/// `objectAssignGeneric(globalObject, vm, target, source)`.
pub fn object_assign_generic(global_object: &JSGlobalObject, target: &ObjectRef, source: &ObjectRef) -> Result<(), Thrown> {
    let source_object: &JSObject = source;
    // `ObjectConstructor.cpp`: origem e alvo com propriedades estáticas não reificadas as reificam antes.
    let vm = global_object.vm();
    for object in [source_object, &**target] {
        if object.has_non_reified_static_properties(&object.structure()) {
            object.reify_all_static_properties(vm);
        }
    }
    let properties = own_names(global_object, source_object, PropertyNameMode::StringsAndSymbols, DontEnumPropertiesMode::Include)?;
    for property_name in &properties {
        debug_assert!(!property_name.is_private_name());
        let name = PropertyName::from_identifier(property_name);
        let Some(descriptor) = own_descriptor(global_object, source_object, &name)? else { continue };
        if !descriptor.enumerable() {
            continue;
        }
        let value = object_get(global_object, source_object, &name, source_object.as_value())?;
        match target {
            // `JSFunction::put` materializa `name`, `length` e `prototype` antes de gravar.
            ObjectRef::Function(function) => {
                let mut slot = PutPropertySlot::new(target.as_value(), true, PutContext::UnknownContext, false);
                function.put(global_object, &name, value, &mut slot)?;
            }
            ObjectRef::Handle(_) => {
                object_set(global_object, target, &name, value, target.as_value(), true)?;
            }
        }
    }
    Ok(())
}

/// `EnumerableOwnProperties(O, kind)` para `values` e `entries` (o caminho genérico de `objectValues`
/// e de `objectConstructorEntries`).
fn enumerable_own_properties(global_object: &JSGlobalObject, object: &ObjectRef, entries: bool) -> Result<JSValue, Thrown> {
    let vm = global_object.vm();
    let object: &JSObject = object;
    let properties = own_names(global_object, object, PropertyNameMode::Strings, DontEnumPropertiesMode::Include)?;
    let mut results = Vec::new();
    for property_name in &properties {
        let name = PropertyName::from_identifier(property_name);
        let Some(descriptor) = own_descriptor(global_object, object, &name)? else { continue };
        if !descriptor.enumerable() {
            continue;
        }
        let value = object_get(global_object, object, &name, object.as_value())?;
        results.push(if entries {
            let key = identifier_to_js_value(vm, property_name);
            construct_array_of(global_object, &[key, value]).as_value()
        } else {
            value
        });
    }
    Ok(construct_array_of(global_object, &results).as_value())
}

/// As propriedades próprias enumeráveis de `target_value` (chaves string) como pares (nome, valor), na ordem de
/// `Object.keys`. Usado para montar os exports nomeados de um módulo embutido.
pub fn enumerable_own_entries(global_object: &JSGlobalObject, target_value: JSValue) -> Result<Vec<(Identifier, JSValue)>, Thrown> {
    let target = to_object(global_object, target_value)?;
    let object: &JSObject = &target;
    let properties = own_names(global_object, object, PropertyNameMode::Strings, DontEnumPropertiesMode::Include)?;
    let mut entries = Vec::new();
    for property_name in &properties {
        let name = PropertyName::from_identifier(property_name);
        let Some(descriptor) = own_descriptor(global_object, object, &name)? else { continue };
        if !descriptor.enumerable() {
            continue;
        }
        entries.push((property_name.clone(), object_get(global_object, object, &name, object.as_value())?));
    }
    Ok(entries)
}

/// `objectValues(vm, globalObject, targetValue)`.
pub fn object_values(global_object: &JSGlobalObject, target_value: JSValue) -> HostResult {
    let target = to_object(global_object, target_value)?;
    enumerable_own_properties(global_object, &target, false)
}

// ---------------------------------------------------------------------------------------------
// Níveis de integridade.
// ---------------------------------------------------------------------------------------------

/// `enum class IntegrityLevel`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum IntegrityLevel {
    Sealed,
    Frozen,
}

/// `setIntegrityLevel<level>(globalObject, vm, object)` (https://tc39.github.io/ecma262/#sec-setintegritylevel).
fn set_integrity_level(global_object: &JSGlobalObject, object: &JSObject, level: IntegrityLevel) -> Result<(), Thrown> {
    if !object_prevent_extensions(global_object, object)? {
        return Err(Thrown::type_error(match level {
            IntegrityLevel::Sealed => "Unable to prevent extension in Object.seal",
            IntegrityLevel::Frozen => "Unable to prevent extension in Object.freeze",
        }));
    }
    let properties = own_names(global_object, object, PropertyNameMode::StringsAndSymbols, DontEnumPropertiesMode::Include)?;
    for property_name in &properties {
        let name = PropertyName::from_identifier(property_name);
        let mut descriptor = PropertyDescriptor::default();
        match level {
            IntegrityLevel::Sealed => descriptor.set_configurable(false),
            IntegrityLevel::Frozen => {
                let Some(current) = own_descriptor(global_object, object, &name)? else { continue };
                if !current.is_accessor_descriptor() {
                    descriptor.set_writable(false);
                }
                descriptor.set_configurable(false);
            }
        }
        define_own_property_of(global_object, object, &name, &descriptor, true)?;
    }
    Ok(())
}

/// `testIntegrityLevel<level>(globalObject, vm, object)` (https://tc39.es/ecma262/#sec-testintegritylevel).
fn test_integrity_level(global_object: &JSGlobalObject, object: &JSObject, level: IntegrityLevel) -> Result<bool, Thrown> {
    if object_is_extensible(global_object, object)? {
        return Ok(false);
    }
    let keys = own_names(global_object, object, PropertyNameMode::StringsAndSymbols, DontEnumPropertiesMode::Include)?;
    for property_name in &keys {
        let name = PropertyName::from_identifier(property_name);
        let Some(descriptor) = own_descriptor(global_object, object, &name)? else { continue };
        if descriptor.configurable() {
            return Ok(false);
        }
        if level == IntegrityLevel::Frozen && descriptor.is_data_descriptor() && descriptor.writable() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `is<JSFinalObject>(object) && !hasIndexedProperties(object->indexingType())`: o atalho por estrutura.
fn is_final_object_without_indexed_properties(object: &JSObject) -> bool {
    object.type_() == JSType::FinalObjectType
        && !has_indexed_properties(object.cell().indexing_type())
        && !crate::runtime::process_env::is_env(object)
}

/// `objectConstructorSeal(globalObject, object)`.
pub fn object_constructor_seal(global_object: &JSGlobalObject, object: &JSObject) -> Result<(), Thrown> {
    let vm = global_object.vm();
    if is_final_object_without_indexed_properties(object) {
        object.seal(vm);
        return Ok(());
    }
    set_integrity_level(global_object, object, IntegrityLevel::Sealed)
}

/// `objectConstructorFreeze(globalObject, object)`. O `Err` é o `TypeError` ou a exceção do
/// `[[DefineOwnProperty]]` (accessor indexado, por exemplo), pendente no `VM` como no C++.
pub fn object_constructor_freeze(global_object: &JSGlobalObject, object: &JSObject) -> Result<(), Thrown> {
    let vm = global_object.vm();
    if is_final_object_without_indexed_properties(object) {
        object.freeze(vm);
        return Ok(());
    }
    set_integrity_level(global_object, object, IntegrityLevel::Frozen)
}

// ---------------------------------------------------------------------------------------------
// As funções nativas (`JSC_DEFINE_HOST_FUNCTION`).
// ---------------------------------------------------------------------------------------------

/// `constructObjectWithNewTarget(globalObject, callFrame, newTarget)`: `Object([value])`.
fn construct_object_with_new_target(global_object: &JSGlobalObject, call: &HostCall, new_target: Option<JSValue>) -> HostResult {
    // We need to check newTarget condition in this caller side instead of InternalFunction::createSubclassStructure
    // side. Since if we found this condition is met, we should not fall into the type conversion in the step 3.
    // 1. If NewTarget is neither undefined nor the active function, then
    if let Some(new_target) = new_target {
        if new_target.is_cell() && new_target.as_cell() != call.callee() {
            // a. Return ? OrdinaryCreateFromConstructor(NewTarget, "%ObjectPrototype%").
            let function_global_object = get_function_realm(new_target)?;
            let base_structure = function_global_object.object_structure_for_object_constructor();
            let object_structure =
                InternalFunction::create_subclass_structure(global_object, &new_target.as_object(), base_structure)?;
            return Ok(JSFinalObject::create(global_object.vm(), &object_structure).as_value());
        }
    }

    // 2. If value is null, undefined or not supplied, return ObjectCreate(%ObjectPrototype%).
    let argument = call.argument(0);
    if argument.is_undefined_or_null() {
        return Ok(construct_empty_object(global_object).as_value());
    }
    // 3. Return ToObject(value).
    Ok(to_object(global_object, argument)?.as_value())
}

/// `constructWithObjectConstructor`.
fn construct_with_object_constructor_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    construct_object_with_new_target(global_object, call, Some(call.new_target()))
}

/// `callObjectConstructor`.
fn call_object_constructor_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    construct_object_with_new_target(global_object, call, None)
}

/// `objectConstructorGetPrototypeOf`.
fn object_constructor_get_prototype_of_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // `callFrame->argument(0).getPrototype(globalObject)`: `undefined`/`null` lançam `createNotAnObjectError`.
    call.argument(0).get_prototype(global_object)
}

/// `objectConstructorSetPrototypeOf`.
fn object_constructor_set_prototype_of_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let object_value = call.argument(0);
    if object_value.is_undefined_or_null() {
        return Err(Thrown::type_error("Cannot set prototype of undefined or null"));
    }

    let proto_value = call.argument(1);
    if !proto_value.is_object() && !proto_value.is_null() {
        return Err(Thrown::type_error(PROTOTYPE_VALUE_CAN_ONLY_BE_AN_OBJECT_OR_NULL_TYPE_ERROR));
    }

    let object = to_object(global_object, object_value)?;
    object_set_prototype(global_object, &object, proto_value, true)?;
    Ok(object_value)
}

/// `objectConstructorGetOwnPropertyDescriptor`.
fn object_constructor_get_own_property_descriptor_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let object = to_object(global_object, call.argument(0))?;
    let property_name = to_property_key(global_object, call.argument(1))?;
    object_constructor_get_own_property_descriptor_of(global_object, &object, &property_name)
}

/// `objectConstructorGetOwnPropertyDescriptors`.
fn object_constructor_get_own_property_descriptors_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let object = to_object(global_object, call.argument(0))?;
    object_constructor_get_own_property_descriptors_of(global_object, &object)
}

/// `ownPropertyKeys` sobre `toObject(argument(0))`, o corpo de `getOwnPropertyNames`, `getOwnPropertySymbols` e `keys`.
fn own_keys_of_argument(
    global_object: &JSGlobalObject,
    call: &HostCall,
    property_name_mode: PropertyNameMode,
    dont_enum_properties_mode: DontEnumPropertiesMode,
) -> HostResult {
    let object = to_object(global_object, call.argument(0))?;
    Ok(own_property_keys(global_object, &object, property_name_mode, dont_enum_properties_mode)?.as_value())
}

/// `objectConstructorGetOwnPropertyNames`.
fn object_constructor_get_own_property_names_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    own_keys_of_argument(global_object, call, PropertyNameMode::Strings, DontEnumPropertiesMode::Include)
}

/// `objectConstructorGetOwnPropertySymbols`.
fn object_constructor_get_own_property_symbols_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    own_keys_of_argument(global_object, call, PropertyNameMode::Symbols, DontEnumPropertiesMode::Include)
}

/// `objectConstructorKeys`.
fn object_constructor_keys_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    own_keys_of_argument(global_object, call, PropertyNameMode::Strings, DontEnumPropertiesMode::Exclude)
}

/// `objectConstructorAssign`.
fn object_constructor_assign_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let target_value = call.argument(0);
    if target_value.is_undefined_or_null() {
        return Err(Thrown::type_error("Object.assign requires that input parameter not be null or undefined"));
    }
    let target = to_object(global_object, target_value)?;
    for index in 1..call.argument_count() {
        let source_value = call.argument(index);
        if source_value.is_undefined_or_null() {
            continue;
        }
        let source = to_object(global_object, source_value)?;
        object_assign_generic(global_object, &target, &source)?;
    }
    Ok(target.as_value())
}

/// `objectConstructorEntries`.
fn object_constructor_entries_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if call.argument(0).is_undefined_or_null() {
        return Err(Thrown::type_error("Object.entries requires that input parameter not be null or undefined"));
    }
    let target = to_object(global_object, call.argument(0))?;
    enumerable_own_properties(global_object, &target, true)
}

/// `objectConstructorValues`.
fn object_constructor_values_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if call.argument(0).is_undefined_or_null() {
        return Err(Thrown::type_error("Object.values requires that input parameter not be null or undefined"));
    }
    object_values(global_object, call.argument(0))
}

/// `objectConstructorDefineProperty`.
fn object_constructor_define_property_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let argument = call.argument(0);
    if !argument.is_object() {
        return Err(Thrown::type_error("Properties can only be defined on Objects."));
    }
    let object = argument.as_object();
    let property_name = to_property_key(global_object, call.argument(1))?;
    let descriptor = to_property_descriptor(global_object, call.argument(2))?;
    define_own_property_of(global_object, &object, &PropertyName::from_identifier(&property_name), &descriptor, true)?;
    Ok(object.as_value())
}

/// `defineProperties(globalObject, object, properties)` (`definePropertiesSlow`): converte todos os
/// descritores antes de definir qualquer um.
fn define_properties(global_object: &JSGlobalObject, object: &ObjectRef, properties: &ObjectRef) -> HostResult {
    let properties: &JSObject = properties;
    let property_names = own_names(global_object, properties, PropertyNameMode::StringsAndSymbols, DontEnumPropertiesMode::Include)?;

    let mut enumerable_names = Vec::new();
    let mut descriptors = Vec::new();
    for property_name in &property_names {
        debug_assert!(!property_name.is_private_name());
        let name = PropertyName::from_identifier(property_name);
        let Some(own) = own_descriptor(global_object, properties, &name)? else { continue };
        if !own.enumerable() {
            continue;
        }
        let prop = object_get(global_object, properties, &name, properties.as_value())?;
        descriptors.push(to_property_descriptor(global_object, prop)?);
        enumerable_names.push(name);
    }

    for (name, descriptor) in enumerable_names.iter().zip(descriptors.iter()) {
        define_own_property_of(global_object, object, name, descriptor, true)?;
    }
    Ok(object.as_value())
}

/// `objectConstructorDefineProperties`.
fn object_constructor_define_properties_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let argument = call.argument(0);
    if !argument.is_object() {
        return Err(Thrown::type_error("Properties can only be defined on Objects."));
    }
    let object = argument.as_object();
    let properties = to_object(global_object, call.argument(1))?;
    define_properties(global_object, &object, &properties)
}

/// `objectConstructorCreate`.
fn object_constructor_create_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let proto = call.argument(0);
    if !proto.is_object() && !proto.is_null() {
        return Err(Thrown::type_error("Object prototype may only be an Object or null."));
    }

    // `constructEmptyObject(globalObject, asObject(proto))` ou `nullPrototypeObjectStructure()`.
    let structure: StructureRef = JSFinalObject::create_structure(vm, Some(global_object), proto, JSFinalObject::DEFAULT_INLINE_CAPACITY);
    let new_object = JSFinalObject::create(vm, &structure);
    if call.argument(1).is_undefined() {
        return Ok(new_object.as_value());
    }

    let properties = to_object(global_object, call.argument(1))?;
    let new_object = new_object.as_value().as_object();
    define_properties(global_object, &new_object, &properties)
}

/// `objectConstructorSeal`.
fn object_constructor_seal_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // 1. If Type(O) is not Object, return O.
    let object_value = call.argument(0);
    if !object_value.is_object() {
        return Ok(object_value);
    }
    object_constructor_seal(global_object, &object_value.as_object())?;
    Ok(object_value)
}

/// `objectConstructorFreeze`.
fn object_constructor_freeze_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // 1. If Type(O) is not Object, return O.
    let object_value = call.argument(0);
    if !object_value.is_object() {
        return Ok(object_value);
    }
    object_constructor_freeze(global_object, &object_value.as_object())?;
    Ok(object_value)
}

/// `objectConstructorPreventExtensions`.
fn object_constructor_prevent_extensions_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let argument = call.argument(0);
    if !argument.is_object() {
        return Ok(argument);
    }
    if !object_prevent_extensions(global_object, &argument.as_object())? {
        return Err(Thrown::type_error("Unable to prevent extension in Object.preventExtensions"));
    }
    Ok(argument)
}

/// `objectConstructorIsSealed`/`objectConstructorIsFrozen`: o atalho por estrutura e depois `TestIntegrityLevel`.
fn is_integrity_level(global_object: &JSGlobalObject, call: &HostCall, level: IntegrityLevel) -> HostResult {
    // 1. If Type(O) is not Object, return true.
    let argument = call.argument(0);
    if !argument.is_object() {
        return Ok(js_boolean(true));
    }
    let object = argument.as_object();
    let object: &JSObject = &object;
    // Quick check for final objects.
    if is_final_object_without_indexed_properties(object) {
        let structure = object.structure();
        return Ok(js_boolean(if level == IntegrityLevel::Sealed { structure.is_sealed() } else { structure.is_frozen() }));
    }
    // 2. Return ? TestIntegrityLevel(O, level).
    Ok(js_boolean(test_integrity_level(global_object, object, level)?))
}

/// `objectConstructorIsSealed`.
fn object_constructor_is_sealed_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    is_integrity_level(global_object, call, IntegrityLevel::Sealed)
}

/// `objectConstructorIsFrozen`.
fn object_constructor_is_frozen_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    is_integrity_level(global_object, call, IntegrityLevel::Frozen)
}

/// `objectConstructorIsExtensible`.
fn object_constructor_is_extensible_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let argument = call.argument(0);
    if !argument.is_object() {
        return Ok(js_boolean(false));
    }
    Ok(js_boolean(object_is_extensible(global_object, &argument.as_object())?))
}

/// `objectConstructorIs`.
fn object_constructor_is_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(js_boolean(same_value(call.argument(0), call.argument(1))))
}

/// `objectConstructorHasOwn`.
fn object_constructor_has_own_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let base = to_object(global_object, call.argument(0))?;
    let property_name = to_property_key(global_object, call.argument(1))?;
    let found = own_descriptor(global_object, &base, &PropertyName::from_identifier(&property_name))?;
    Ok(js_boolean(found.is_some()))
}

host_function!(call_object_constructor, call_object_constructor_body);
host_function!(construct_with_object_constructor, construct_with_object_constructor_body);
host_function!(object_constructor_get_prototype_of, object_constructor_get_prototype_of_body);
host_function!(object_constructor_set_prototype_of, object_constructor_set_prototype_of_body);
host_function!(object_constructor_get_own_property_descriptor, object_constructor_get_own_property_descriptor_body);
host_function!(object_constructor_get_own_property_descriptors, object_constructor_get_own_property_descriptors_body);
host_function!(object_constructor_get_own_property_names, object_constructor_get_own_property_names_body);
host_function!(object_constructor_get_own_property_symbols, object_constructor_get_own_property_symbols_body);
host_function!(object_constructor_keys, object_constructor_keys_body);
host_function!(object_constructor_assign, object_constructor_assign_body);
host_function!(object_constructor_entries, object_constructor_entries_body);
host_function!(object_constructor_values, object_constructor_values_body);
host_function!(object_constructor_define_property, object_constructor_define_property_body);
host_function!(object_constructor_define_properties, object_constructor_define_properties_body);
host_function!(object_constructor_create, object_constructor_create_body);
host_function!(object_constructor_seal_host, object_constructor_seal_body);
host_function!(object_constructor_freeze_host, object_constructor_freeze_body);
host_function!(object_constructor_prevent_extensions, object_constructor_prevent_extensions_body);
host_function!(object_constructor_is_sealed, object_constructor_is_sealed_body);
host_function!(object_constructor_is_frozen, object_constructor_is_frozen_body);
host_function!(object_constructor_is_extensible, object_constructor_is_extensible_body);
host_function!(pub object_constructor_is, object_constructor_is_body);
host_function!(object_constructor_has_own, object_constructor_has_own_body);

/// `objectConstructorTableValues` de `ObjectConstructor.lut.h`, na ordem do `@begin` (`fromEntries` é `JSBuiltin`).
static OBJECT_CONSTRUCTOR_TABLE_VALUES: [HashTableValue; 21] = [
    native_entry_with_intrinsic("getPrototypeOf", object_constructor_get_prototype_of, 1, Intrinsic::ObjectGetPrototypeOfIntrinsic),
    native_entry_with_intrinsic("setPrototypeOf", object_constructor_set_prototype_of, 2, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("getOwnPropertyDescriptor", object_constructor_get_own_property_descriptor, 2, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("getOwnPropertyDescriptors", object_constructor_get_own_property_descriptors, 1, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("getOwnPropertyNames", object_constructor_get_own_property_names, 1, Intrinsic::ObjectGetOwnPropertyNamesIntrinsic),
    native_entry_with_intrinsic("getOwnPropertySymbols", object_constructor_get_own_property_symbols, 1, Intrinsic::ObjectGetOwnPropertySymbolsIntrinsic),
    native_entry_with_intrinsic("keys", object_constructor_keys, 1, Intrinsic::ObjectKeysIntrinsic),
    native_entry_with_intrinsic("defineProperty", object_constructor_define_property, 3, Intrinsic::ObjectDefinePropertyIntrinsic),
    native_entry_with_intrinsic("defineProperties", object_constructor_define_properties, 2, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("create", object_constructor_create, 2, Intrinsic::ObjectCreateIntrinsic),
    native_entry_with_intrinsic("seal", object_constructor_seal_host, 1, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("freeze", object_constructor_freeze_host, 1, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("preventExtensions", object_constructor_prevent_extensions, 1, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("isSealed", object_constructor_is_sealed, 1, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("isFrozen", object_constructor_is_frozen, 1, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("isExtensible", object_constructor_is_extensible, 1, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("is", object_constructor_is, 2, Intrinsic::ObjectIsIntrinsic),
    native_entry_with_intrinsic("assign", object_constructor_assign, 2, Intrinsic::ObjectAssignIntrinsic),
    native_entry_with_intrinsic("values", object_constructor_values, 1, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("entries", object_constructor_entries, 1, Intrinsic::NoIntrinsic),
    HashTableValue {
        key: "fromEntries",
        attributes: DONT_ENUM | BUILTIN,
        intrinsic: Intrinsic::NoIntrinsic,
        kind: Kind::BuiltinGenerator { generator: BuiltinCodeIndex::ObjectConstructorFromEntriesCode, length: 1 },
    },
];

/// `objectConstructorTable`.
static OBJECT_CONSTRUCTOR_TABLE: HashTable = HashTable { class_for_this: None, values: &OBJECT_CONSTRUCTOR_TABLE_VALUES };

/// `class ObjectConstructor final : public InternalFunction`: sem campos próprios, é o `InternalFunction`.
pub struct ObjectConstructor;

impl ObjectConstructor {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
    pub const STRUCTURE_FLAGS: u32 = InternalFunction::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject, prototype)` (`ObjectConstructorInlines.h`).
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::InternalFunctionType, ObjectConstructor::STRUCTURE_FLAGS),
            &OBJECT_CONSTRUCTOR_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure, objectPrototype)`: `ObjectConstructor(vm, structure)`
    /// (`InternalFunction(vm, structure, callObjectConstructor, constructWithObjectConstructor)`) e
    /// `finishCreation(vm, globalObject, objectPrototype)`.
    pub fn create(
        vm: &VM,
        global_object: &JSGlobalObject,
        structure: StructureRef,
        object_prototype: &JSObject,
    ) -> InternalFunctionRef {
        let constructor = InternalFunction::new(vm, structure, call_object_constructor, Some(construct_with_object_constructor));
        // As entradas de `objectConstructorTable` (e `fromEntries`) são da lut: reificadas no primeiro acesso.
        constructor.finish_creation(vm, 1, vm.property_names.object.string().string(), PropertyAdditionMode::WithoutStructureTransition);
        constructor.put_direct(
            vm,
            &PropertyName::from_identifier(&vm.property_names.prototype),
            object_prototype.as_value(),
            DONT_ENUM | DONT_DELETE | READ_ONLY,
        );

        // As funções sob os nomes privados que os builtins JS usam (`@getPrototypeOf`...).
        let builtin_names = vm.property_names.builtin_names();
        // `hasOwn` público fica entre `@values` e `@hasOwn`, como em `finishCreation`.
        let public_has_own = (vm.property_names.has_own.clone(), object_constructor_has_own as NativeFunction, 2, Intrinsic::ObjectHasOwnIntrinsic);
        let private_functions: [(Identifier, NativeFunction, u32, Intrinsic); 10] = [
            (builtin_names.get_prototype_of_private_name(), object_constructor_get_prototype_of, 1, Intrinsic::NoIntrinsic),
            (builtin_names.get_own_property_descriptor_private_name(), object_constructor_get_own_property_descriptor, 2, Intrinsic::NoIntrinsic),
            (builtin_names.get_own_property_names_private_name(), object_constructor_get_own_property_names, 1, Intrinsic::NoIntrinsic),
            (builtin_names.get_own_property_symbols_private_name(), object_constructor_get_own_property_symbols, 1, Intrinsic::NoIntrinsic),
            (builtin_names.keys_private_name(), object_constructor_keys, 1, Intrinsic::NoIntrinsic),
            (builtin_names.define_property_private_name(), object_constructor_define_property, 3, Intrinsic::NoIntrinsic),
            (builtin_names.create_private_name(), object_constructor_create, 2, Intrinsic::NoIntrinsic),
            (builtin_names.values_private_name(), object_constructor_values, 1, Intrinsic::NoIntrinsic),
            public_has_own,
            (builtin_names.has_own_private_name(), object_constructor_has_own, 2, Intrinsic::ObjectHasOwnIntrinsic),
        ];

        for (name, function, length, intrinsic) in private_functions {
            put_direct_native_function_without_transition(
                vm,
                global_object,
                &constructor,
                &name,
                length,
                function,
                ImplementationVisibility::Public,
                intrinsic,
                DONT_ENUM,
            );
        }

        // `groupBy` (`JSC_BUILTIN_FUNCTION_WITHOUT_TRANSITION`), por último.
        put_direct_builtin_function_without_transition(
            vm,
            global_object,
            &constructor,
            builtin_names.group_by_public_name(),
            BuiltinCodeIndex::ObjectConstructorGroupByCode,
            DONT_ENUM,
        );
        constructor
    }
}
