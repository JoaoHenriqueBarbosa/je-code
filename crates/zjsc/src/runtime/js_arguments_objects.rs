//! Porte de `runtime/DirectArguments.{h,cpp}` e de `runtime/ClonedArguments.{h,cpp}`: os objetos que
//! `op_create_direct_arguments` e `op_create_cloned_arguments` devolvem para o `arguments` de uma função.
//! O mixin de índices mapeados (`GenericArgumentsImpl`) está em `generic_arguments.rs` e o
//! `ScopedArguments` em `js_scoped_arguments.rs`.
//!
//! DIVERGÊNCIAS:
//!
//! - O `JSObject` do porte não despacha `getOwnPropertySlot` nem `put` por classe para as chaves
//!   `length`, `callee` e `@@iterator`. Então os dois objetos nascem já com o que o C++ materializa sob
//!   demanda (`ClonedArguments::materializeSpecials`, `DirectArguments::overrideThings`). Os três objetos
//!   materializam sob demanda, como o C++ (ver `materialize_specials`): `callee` e `@@iterator` no
//!   `ClonedArguments` (o `length` nasce com ele), `length`, `callee` e `@@iterator` no `DirectArguments` e no
//!   `ScopedArguments` (`overrodeThings`, `m_mappedArguments` no direto). A ordem das chaves próprias é a do
//!   C++. O `JSType` e o `ClassInfo` (`"Arguments"`) são os do C++, sem as flags `Overrides*` (os índices são
//!   despachados por `generic_arguments::exotic_of`).
//! - `DirectArguments` guarda os argumentos no `storage` (`m_storage`, de `max(length, minCapacity)`
//!   posições), como o C++, e os elementos do `JSObject` só passam a existir para o índice que um
//!   `defineProperty` materializa. `m_mappedArguments` é o `unmapped` (vazio é nulo) e
//!   `m_modifiedArgumentsDescriptor` está no `ArgumentsState`. Por isso `delete arguments[i]` e
//!   `defineProperty` desligam o parâmetro `i` do objeto, como no C++.
//! - `ClonedArguments` nasce com o `callee` de `materializeSpecials`: o acessor `%ThrowTypeError%` do
//!   realm (`throwTypeErrorArgumentsCalleeGetterSetter`) em modo estrito ou com lista de parâmetros que
//!   não é simples, e o valor `callee` (atributos `0`) nos demais. O `m_callee` do C++ some junto com o
//!   `specialsMaterialized`.
//! - A `Structure` vem do `StructureCache` do realm (chave: protótipo e `ClassInfo`), no papel do
//!   `directArgumentsStructure()`/`clonedArgumentsStructure()`/`scopedArgumentsStructure()` do `JSGlobalObject`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::generic_arguments::{exotic_of, ArgumentsState, MappedArguments};
use crate::runtime::property_name_array::PropertyNameArrayBuilder;
use crate::runtime::js_function::{JSFunctionRef, FunctionExecutableRef};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, PutError, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_null, JSValue};
use crate::runtime::property_slot::PropertySlot;
use crate::runtime::vm::VM;
use crate::runtime::property_attribute::{ACCESSOR, DONT_DELETE, DONT_ENUM};
use crate::runtime::property_name::PropertyName;
use crate::runtime::sparse_array_value_map::PutDirectIndexMode;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::structure_cache::ShouldCacheStructure;

/// `const ClassInfo DirectArguments::s_info`.
pub static DIRECT_ARGUMENTS_S_INFO: ClassInfo =
    ClassInfo { class_name: "Arguments", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
/// `const ClassInfo ClonedArguments::s_info`.
pub static CLONED_ARGUMENTS_S_INFO: ClassInfo =
    ClassInfo { class_name: "Arguments", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class DirectArguments final : public GenericArgumentsImpl<DirectArguments>`.
pub struct DirectArguments {
    base: JSNonFinalObject,
    /// `m_length`.
    length: u32,
    /// `m_storage[0..max(m_length, m_minCapacity)]`: os argumentos passados e, depois deles, os parâmetros
    /// que a chamada não passou (`undefined`).
    storage: RefCell<Vec<JSValue>>,
    /// `m_mappedArguments` (os índices que deixaram de ser mapeados); vazio é o ponteiro nulo.
    unmapped: RefCell<Vec<bool>>,
    /// `m_callee`.
    callee: JSValue,
    state: ArgumentsState,
}

/// O `DirectArguments*`.
pub type DirectArgumentsRef = Rc<DirectArguments>;

impl std::fmt::Debug for DirectArguments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirectArguments").field("cell_id", &self.base.cell_id()).field("length", &self.length).finish()
    }
}

impl std::ops::Deref for DirectArguments {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

/// `globalObject->directArgumentsStructure()`, `clonedArgumentsStructure()` e `scopedArgumentsStructure()`:
/// a `Structure` vazia do `JSType` e `ClassInfo` pedidos, com o `Object.prototype` do realm.
pub(crate) fn arguments_structure(global_object: &JSGlobalObject, type_: JSType, class_info: &'static ClassInfo) -> StructureRef {
    let vm = global_object.vm();
    // `ClonedArguments::StructureFlags` tem `OverridesGetOwnSpecialPropertyNames`; `GenericArgumentsImpl`, `OverridesGetOwnPropertyNames`.
    // Sem elas `canPerformFastPropertyEnumeration` deixaria o `{...arguments}` copiar só as propriedades da `Structure`.
    let names_flag = if type_ == JSType::ClonedArgumentsType {
        crate::runtime::js_type_info::OVERRIDES_GET_OWN_SPECIAL_PROPERTY_NAMES
    } else {
        crate::runtime::js_type_info::OVERRIDES_GET_OWN_PROPERTY_NAMES
    };
    let base_structure = Structure::create(
        vm,
        Some(global_object),
        js_null(),
        TypeInfo::new(type_, JSNonFinalObject::STRUCTURE_FLAGS | names_flag),
        class_info,
    );
    global_object.structure_cache().empty_structure_for_prototype_from_base_structure(
        global_object,
        &global_object.object_prototype(),
        &base_structure,
        ShouldCacheStructure::Yes,
    )
}

/// A propriedade `callee` que `overrideThings` e `materializeSpecials` criam.
pub(crate) enum CalleeProperty {
    /// `putDirect(vm, callee, value, attributes)`.
    Value { value: JSValue, attributes: u32 },
    /// `putDirectAccessor(globalObject, callee, throwTypeErrorArgumentsCalleeGetterSetter(), DontDelete | DontEnum | Accessor)`.
    ThrowTypeError,
}

/// `length`, `callee` e `Symbol.iterator`, nesta ordem, como `overrideThings` os cria.
pub(crate) fn put_arguments_specials(
    global_object: &JSGlobalObject,
    object: &JSObject,
    length: u32,
    callee: CalleeProperty,
) -> Result<(), PutError> {
    put_arguments_length(global_object, object, length);
    put_callee_and_iterator(global_object, object, callee)
}

/// O `length` do `ClonedArguments` (`putDirectOffset(clonedArgumentsLengthPropertyOffset)`), a primeira
/// propriedade da `Structure`.
fn put_arguments_length(global_object: &JSGlobalObject, object: &JSObject, length: u32) {
    let vm = global_object.vm();
    object.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.length), JSValue::from_u32(length), DONT_ENUM);
}

/// `callee` e depois `Symbol.iterator`, como `materializeSpecials` os cria.
fn put_callee_and_iterator(global_object: &JSGlobalObject, object: &JSObject, callee: CalleeProperty) -> Result<(), PutError> {
    let vm = global_object.vm();
    let callee_name = PropertyName::from_identifier(&vm.property_names.callee);
    match callee {
        CalleeProperty::Value { value, attributes } => {
            object.put_direct(vm, &callee_name, value, attributes);
        }
        CalleeProperty::ThrowTypeError => {
            let accessor = global_object.throw_type_error_arguments_callee_getter_setter();
            object.put_direct_accessor(vm, &callee_name, accessor, DONT_DELETE | DONT_ENUM | ACCESSOR)?;
        }
    }
    let values_function: JSFunctionRef = global_object.array_proto_values_function();
    object.put_direct(
        vm,
        &PropertyName::from_identifier(&vm.property_names.iterator_symbol),
        JSValue::from_cell(values_function.cell_id()),
        DONT_ENUM,
    );
    Ok(())
}

thread_local! {
    /// O `m_callee` dos `ClonedArguments` que ainda não materializaram `callee` e `@@iterator`
    /// (`specialsMaterialized()` é falso), por `cell_id`.
    static PENDING_CLONED_SPECIALS: RefCell<std::collections::HashMap<usize, JSFunctionRef>> =
        RefCell::new(std::collections::HashMap::new());
}

/// Fim do programa (`cell_registry::reset_program_state`): os `callee` pendentes são funções do programa
/// (o `ClonedArguments` do C++ os guarda no próprio objeto) e os `cell_id` voltam a ser reaproveitados.
/// As raízes do coletor: os objetos `arguments` à espera das funções especiais clonadas (as funções são `Rc`).
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    PENDING_CLONED_SPECIALS.with(|pending| pending.borrow().keys().for_each(|cell_id| marker.cell(*cell_id)));
}

pub(crate) fn reset_for_program() {
    let taken = PENDING_CLONED_SPECIALS.try_with(|pending| std::mem::take(&mut *pending.borrow_mut()));
    drop(taken);
}

/// Os especiais (`length`, `callee`, `@@iterator`) que um objeto `arguments` ainda não materializou.
enum PendingSpecials {
    /// `ClonedArguments` com `specialsMaterialized()` falso: o `m_callee`. `length` já existe.
    Cloned(JSFunctionRef),
    /// `DirectArguments` e `ScopedArguments` com `overrodeThings()` falso.
    Mapped(Rc<dyn MappedArguments>),
}

/// Os especiais ainda por materializar do objeto (`None` se não é um objeto `arguments` ou já materializou).
fn pending_specials(object: &JSObject) -> Option<PendingSpecials> {
    let type_ = object.type_();
    if type_ == JSType::ClonedArgumentsType {
        return PENDING_CLONED_SPECIALS.with(|pending| pending.borrow().get(&object.cell_id()).cloned()).map(PendingSpecials::Cloned);
    }
    exotic_of(object).filter(|arguments| !arguments.overrode_things()).map(PendingSpecials::Mapped)
}

/// Os nomes que o objeto materializa sob demanda: `callee` e `@@iterator` no `ClonedArguments`, e também
/// `length` no `DirectArguments` e `ScopedArguments` (o `ident == length || callee || iteratorSymbol` do
/// `GenericArgumentsImpl`).
fn is_special_name(vm: &VM, name: &PropertyName, pending: &PendingSpecials) -> bool {
    *name == vm.property_names.callee
        || *name == vm.property_names.iterator_symbol
        || (matches!(pending, PendingSpecials::Mapped(_)) && *name == vm.property_names.length)
}

/// `ClonedArguments::callee` conforme o executável: o acessor `%ThrowTypeError%` em modo estrito ou com
/// lista de parâmetros não simples, o valor (atributos `0`) nos demais.
fn cloned_callee_is_accessor(callee: &JSFunctionRef) -> bool {
    let executable: FunctionExecutableRef = callee.js_executable();
    let executable = executable.borrow();
    executable.is_in_strict_context() || executable.uses_non_simple_parameter_list()
}

/// `ClonedArguments::materializeSpecialsIfNecessary(globalObject)` e `overrideThingsIfNecessary(globalObject)`.
fn materialize_specials(global_object: &JSGlobalObject, object: &JSObject, pending: PendingSpecials) -> Result<(), PutError> {
    match pending {
        PendingSpecials::Cloned(callee) => {
            PENDING_CLONED_SPECIALS.with(|pending| pending.borrow_mut().remove(&object.cell_id()));
            let property = if cloned_callee_is_accessor(&callee) {
                CalleeProperty::ThrowTypeError
            } else {
                CalleeProperty::Value { value: callee.as_value(), attributes: 0 }
            };
            put_callee_and_iterator(global_object, object, property)
        }
        PendingSpecials::Mapped(arguments) => arguments.override_things(global_object),
    }
}

/// Os ganchos de `put`, `deleteProperty` e `defineOwnProperty` dos objetos `arguments`: materializam os
/// especiais antes quando o nome é um deles. `Ok(false)` se nada precisava.
pub(crate) fn materialize_specials_for_property(vm: &VM, object: &JSObject, name: &PropertyName) -> Result<bool, PutError> {
    let Some(pending) = pending_specials(object) else { return Ok(false) };
    if !is_special_name(vm, name, &pending) {
        return Ok(false);
    }
    let Some(realm) = object.structure().realm() else { return Ok(false) };
    materialize_specials(&realm, object, pending)?;
    Ok(true)
}

/// `getOwnSpecialPropertyNames` (`ClonedArguments`) e o ramo `Include && !overrodeThings()` do
/// `GenericArgumentsImpl::getOwnPropertyNames`: o `ClonedArguments` materializa, os outros dois só
/// acrescentam os três nomes (antes das propriedades da `Structure`).
pub(crate) fn special_property_names(
    object: &JSObject,
    property_names: &mut PropertyNameArrayBuilder<'_>,
) -> Result<(), PutError> {
    match pending_specials(object) {
        None => Ok(()),
        Some(PendingSpecials::Mapped(_)) => {
            let names = &property_names.vm().property_names;
            let identifiers = [names.length.clone(), names.callee.clone(), names.iterator_symbol.clone()];
            for identifier in &identifiers {
                property_names.add(identifier);
            }
            Ok(())
        }
        Some(pending) => {
            let Some(realm) = object.structure().realm() else { return Ok(()) };
            materialize_specials(&realm, object, pending)
        }
    }
}

/// `getOwnPropertySlot` dos objetos `arguments` antes da materialização: os especiais respondem sem virar
/// propriedade. `None` quando o nome não é especial ou já materializou.
pub(crate) fn special_own_slot(vm: &VM, object: &JSObject, name: &PropertyName, slot: &mut PropertySlot) -> Option<bool> {
    let pending = pending_specials(object)?;
    if !is_special_name(vm, name, &pending) {
        return None;
    }
    let realm = object.structure().realm()?;
    if *name == vm.property_names.length {
        let PendingSpecials::Mapped(arguments) = &pending else { return None };
        slot.set_value(object, DONT_ENUM, JSValue::from_u32(arguments.internal_length()));
    } else if *name == vm.property_names.callee {
        match &pending {
            PendingSpecials::Cloned(callee) if cloned_callee_is_accessor(callee) => {
                slot.set_getter_slot(object, DONT_DELETE | DONT_ENUM | ACCESSOR, realm.throw_type_error_arguments_callee_getter_setter());
            }
            PendingSpecials::Cloned(callee) => slot.set_value(object, 0, callee.as_value()),
            PendingSpecials::Mapped(arguments) => slot.set_value(object, DONT_ENUM, arguments.callee()),
        }
    } else {
        let values_function: JSFunctionRef = realm.array_proto_values_function();
        slot.set_value(object, DONT_ENUM, JSValue::from_cell(values_function.cell_id()));
    }
    Some(true)
}

impl DirectArguments {
    /// `visitChildren`: os argumentos e o callee.
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        marker.value(self.callee);
        match self.storage.try_borrow() {
            Ok(storage) => marker.values(&storage),
            Err(_) => marker.abort(),
        }
    }
    /// `DirectArguments::createByCopying(globalObject, callFrame)`: `arguments` são os `argumentCount()`
    /// argumentos do frame e `min_capacity` o `numParameters() - 1` do `CodeBlock`.
    pub fn create_by_copying(
        global_object: &JSGlobalObject,
        arguments: &[JSValue],
        min_capacity: usize,
        callee: JSValue,
    ) -> Result<DirectArgumentsRef, PutError> {
        let vm = global_object.vm();
        let structure = arguments_structure(global_object, JSType::DirectArgumentsType, &DIRECT_ARGUMENTS_S_INFO);
        let mut storage = arguments.to_vec();
        storage.resize(arguments.len().max(min_capacity), JSValue::undefined());

        let cell_id = cell_registry::reserve();
        let result = Rc::new(DirectArguments {
            base: JSNonFinalObject::new(vm, structure),
            length: arguments.len() as u32,
            storage: RefCell::new(storage),
            unmapped: RefCell::new(Vec::new()),
            callee,
            state: ArgumentsState::default(),
        });
        result.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::DirectArguments(Rc::clone(&result)));

        // `length`, `callee` e `@@iterator` só passam a existir em `overrideThings`.
        Ok(result)
    }

    /// O `DirectArguments*` de um `JSValue` (`None` se não é a célula de um `DirectArguments`).
    pub fn from_value(value: &JSValue) -> Option<DirectArgumentsRef> {
        if !value.is_cell() {
            return None;
        }
        match cell_registry::get(value.as_cell()) {
            Some(CellEntry::DirectArguments(arguments)) => Some(arguments),
            _ => None,
        }
    }

    /// A célula como `JSValue`.
    pub fn as_value(&self) -> JSValue {
        JSValue::from_cell(self.base.cell_id())
    }

    /// `argument(offset)` (o `op_get_from_arguments`).
    pub fn storage_at(&self, offset: u32) -> JSValue {
        self.storage.borrow()[offset as usize]
    }

    /// `argument(offset).set(vm, this, value)` (o `op_put_to_arguments`).
    pub fn set_storage_at(&self, offset: u32, value: JSValue) {
        self.storage.borrow_mut()[offset as usize] = value;
    }
}

impl MappedArguments for DirectArguments {
    fn object(&self) -> &JSObject {
        self
    }

    fn state(&self) -> &ArgumentsState {
        &self.state
    }

    fn internal_length(&self) -> u32 {
        self.length
    }

    fn modified_length(&self) -> u32 {
        self.length
    }

    /// `i < m_length && (!m_mappedArguments || !m_mappedArguments.at(i))`.
    fn is_mapped_argument(&self, index: u32) -> bool {
        index < self.length && !self.unmapped.borrow().get(index as usize).copied().unwrap_or(false)
    }

    fn get_index_quickly(&self, index: u32) -> JSValue {
        debug_assert!(self.is_mapped_argument(index));
        self.storage_at(index)
    }

    fn set_index_quickly(&self, index: u32, value: JSValue) {
        debug_assert!(self.is_mapped_argument(index));
        self.set_storage_at(index, value);
    }

    fn callee(&self) -> JSValue {
        self.callee
    }

    /// `overrodeThings()`: `!!m_mappedArguments`.
    fn overrode_things(&self) -> bool {
        !self.unmapped.borrow().is_empty()
    }

    /// `overrideThings`: `length`, `callee`, `@@iterator` e depois o `m_mappedArguments` (sempre alguma
    /// coisa, mesmo com `m_length` zero).
    fn override_things(&self, global_object: &JSGlobalObject) -> Result<(), PutError> {
        put_arguments_specials(global_object, self, self.length, CalleeProperty::Value { value: self.callee, attributes: DONT_ENUM })?;
        *self.unmapped.borrow_mut() =
            crate::runtime::fallible_alloc::try_filled_vec(false, self.length.max(1) as usize).ok_or(PutError::OutOfMemory)?;
        Ok(())
    }

    /// `unmapArgument`: `overrideThingsIfNecessary` e marca o índice.
    fn unmap_argument(&self, index: u32) -> Result<(), PutError> {
        self.override_things_if_necessary()?;
        self.unmapped.borrow_mut()[index as usize] = true;
        Ok(())
    }
}

/// `ClonedArguments::createWithMachineFrame(globalObject, callFrame, ArgumentsMode::Cloned)`: `arguments`
/// são os `argumentCount()` argumentos do frame e `callee` o `jsCallee()`.
pub fn create_cloned_arguments(
    global_object: &JSGlobalObject,
    arguments: &[JSValue],
    callee: &JSFunctionRef,
) -> Result<JSObjectRef, PutError> {
    let vm = global_object.vm();
    let structure = arguments_structure(global_object, JSType::ClonedArgumentsType, &CLONED_ARGUMENTS_S_INFO);
    let result = JSObject::allocate(vm, &structure);
    for (index, argument) in arguments.iter().enumerate() {
        result.put_direct_index(vm, index as u32, *argument, 0, PutDirectIndexMode::PutDirectIndexLikePutDirect)?;
    }

    // `length` nasce com o objeto; `callee` e `@@iterator` só em `materializeSpecials`.
    put_arguments_length(global_object, &result, arguments.len() as u32);
    PENDING_CLONED_SPECIALS.with(|pending| pending.borrow_mut().insert(result.cell_id(), callee.clone()));
    Ok(result)
}
