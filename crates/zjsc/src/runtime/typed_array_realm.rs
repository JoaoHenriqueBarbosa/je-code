//! O que o `JSGlobalObject` guarda dos `TypedArray` (`m_typedArrayProto`, `m_typedArraySuperConstructor`, os
//! doze `m_typedArrayXxx` (`LazyClassStructure`) e os doze `m_resizableOrGrowableSharedTypedArrayXxxStructure`):
//! o protótipo, a estrutura e o construtor de cada tipo, a estrutura das visões sobre buffer redimensionável,
//! e o `init` que os cria (o `INIT_TYPED_ARRAY_LATER` de `JSGlobalObject.cpp:1245`), instala as propriedades
//! globais (`Int8Array`... `BigUint64Array`, todas `DontEnum`) e as `LinkTimeConstant` `Int8Array`...
//! `BigUint64Array`.
//!
//! DIVERGÊNCIA: `LazyClassStructure` e `LazyProperty` são criados de uma vez em `init` (como o resto do porte);
//! o C++ cria na primeira leitura. A ordem de criação (`%TypedArray%.prototype`, `%TypedArray%` e, por tipo,
//! protótipo, estrutura, estrutura redimensionável e construtor) é a do C++.

use std::cell::RefCell;
use std::rc::Rc;

use crate::bytecode::property_condition::ObjectPropertyCondition;
use crate::bytecode::watchpoint::{InlineWatchpointSet, StringFireDetail, WatchpointState};
use crate::runtime::js_global_object_init::HasSpeciesProperty;
use crate::runtime::js_object::{JSObject, JSObjectHandle};
use crate::runtime::object_adaptive_structure_watchpoint::ObjectAdaptiveStructureWatchpoint;
use crate::runtime::object_property_change_adaptive_watchpoint::ObjectPropertyChangeAdaptiveWatchpoint;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::bytecode::bytecode_intrinsics_table::LinkTimeConstant;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_function::JSFunctionRef;
use crate::runtime::js_array_buffer::ClassStructure;
use crate::runtime::js_generic_typed_array_view::JSGenericTypedArrayView;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObjectRef;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::StructureRef;
use crate::runtime::typed_array_constructor::TypedArrayViewConstructor;
use crate::runtime::typed_array_constructors::TypedArrayConstructor;
use crate::runtime::typed_array_prototype::{install_private_functions, TypedArrayViewPrototype};
use crate::runtime::typed_array_type::{TypedArrayType, TYPED_ARRAY_TYPES_EXCLUDING_DATA_VIEW};
use crate::runtime::vm::VM;

/// A `LinkTimeConstant` do construtor de cada tipo (`LinkTimeConstant::type##Array`).
fn link_time_constant_for(type_: TypedArrayType) -> LinkTimeConstant {
    match type_ {
        TypedArrayType::Int8 => LinkTimeConstant::Int8Array,
        TypedArrayType::Uint8 => LinkTimeConstant::Uint8Array,
        TypedArrayType::Uint8Clamped => LinkTimeConstant::Uint8ClampedArray,
        TypedArrayType::Int16 => LinkTimeConstant::Int16Array,
        TypedArrayType::Uint16 => LinkTimeConstant::Uint16Array,
        TypedArrayType::Int32 => LinkTimeConstant::Int32Array,
        TypedArrayType::Uint32 => LinkTimeConstant::Uint32Array,
        TypedArrayType::Float16 => LinkTimeConstant::Float16Array,
        TypedArrayType::Float32 => LinkTimeConstant::Float32Array,
        TypedArrayType::Float64 => LinkTimeConstant::Float64Array,
        TypedArrayType::BigInt64 => LinkTimeConstant::BigInt64Array,
        TypedArrayType::BigUint64 => LinkTimeConstant::BigUint64Array,
        TypedArrayType::NotTypedArray | TypedArrayType::DataView => unreachable!("sem LinkTimeConstant para {type_:?}"),
    }
}

/// A ordem em que `JSGlobalObject.cpp` (a tabela de propriedades estáticas) lista as classes: é a ordem das
/// propriedades globais.
const GLOBAL_PROPERTY_ORDER: [TypedArrayType; 12] = [
    TypedArrayType::Int8,
    TypedArrayType::Int16,
    TypedArrayType::Int32,
    TypedArrayType::Uint8,
    TypedArrayType::Uint8Clamped,
    TypedArrayType::Uint16,
    TypedArrayType::Uint32,
    TypedArrayType::Float16,
    TypedArrayType::Float32,
    TypedArrayType::Float64,
    TypedArrayType::BigInt64,
    TypedArrayType::BigUint64,
];

/// O que `JSGlobalObject` guarda dos `TypedArray`. Os vetores são indexados por `TypedArrayType::to_index`.
#[derive(Debug)]
pub struct TypedArrayRealm {
    /// `m_typedArrayProto`.
    super_prototype: RefCell<Option<JSObjectRef>>,
    /// `m_typedArraySuperConstructor`.
    super_constructor: RefCell<Option<JSFunctionRef>>,
    /// `m_typedArrayXxx`: protótipo, estrutura e construtor de cada tipo.
    classes: RefCell<Vec<ClassStructure>>,
    /// `m_resizableOrGrowableSharedTypedArrayXxxStructure`.
    resizable_or_growable_shared_structures: RefCell<Vec<StructureRef>>,
    /// `m_typedArrayConstructorSpeciesWatchpointSet`: nasce `IsWatched`; o watchpoint abaixo o dispara se o
    /// `@@species` do `%TypedArray%` mudar.
    constructor_species_watchpoint_set: Rc<RefCell<InlineWatchpointSet>>,
    /// `m_typedArrayConstructorSpeciesWatchpoint`.
    constructor_species_watchpoint: RefCell<Option<ObjectPropertyChangeAdaptiveWatchpoint>>,
    /// `m_typedArrayXxxSpeciesWatchpointSet`: nascem `ClearWatchpoint`, `try_install_species_watchpoint` os
    /// põe em `IsWatched` ou os invalida. Indexados por `TypedArrayType::to_index`.
    species_watchpoint_sets: [Rc<RefCell<InlineWatchpointSet>>; TYPED_ARRAY_COUNT],
    /// `m_typedArrayXxxPrototypeConstructorWatchpoint`.
    prototype_constructor_watchpoints: [RefCell<Option<ObjectPropertyChangeAdaptiveWatchpoint>>; TYPED_ARRAY_COUNT],
    /// `m_typedArrayXxxConstructorSpeciesAbsenceWatchpoint`.
    constructor_species_absence_watchpoints: [RefCell<Option<ObjectAdaptiveStructureWatchpoint>>; TYPED_ARRAY_COUNT],
}

/// Os doze tipos de `FOR_EACH_TYPED_ARRAY_TYPE`.
const TYPED_ARRAY_COUNT: usize = 12;

impl Default for TypedArrayRealm {
    fn default() -> Self {
        let set = |state| Rc::new(RefCell::new(InlineWatchpointSet::new(state)));
        TypedArrayRealm {
            super_prototype: RefCell::default(),
            super_constructor: RefCell::default(),
            classes: RefCell::default(),
            resizable_or_growable_shared_structures: RefCell::default(),
            constructor_species_watchpoint_set: set(WatchpointState::IsWatched),
            constructor_species_watchpoint: RefCell::new(None),
            species_watchpoint_sets: std::array::from_fn(|_| set(WatchpointState::ClearWatchpoint)),
            prototype_constructor_watchpoints: std::array::from_fn(|_| RefCell::new(None)),
            constructor_species_absence_watchpoints: std::array::from_fn(|_| RefCell::new(None)),
        }
    }
}

impl TypedArrayRealm {
    /// `typedArraySpeciesWatchpointSet(type)`.
    pub fn species_watchpoint_set(&self, type_: TypedArrayType) -> &Rc<RefCell<InlineWatchpointSet>> {
        &self.species_watchpoint_sets[type_.to_index()]
    }

    /// `typedArrayConstructorSpeciesWatchpointSet()`.
    pub fn constructor_species_watchpoint_set(&self) -> &Rc<RefCell<InlineWatchpointSet>> {
        &self.constructor_species_watchpoint_set
    }

    /// `JSGlobalObject::installTypedArrayConstructorSpeciesWatchpoint(constructor)` (JSGlobalObject.cpp:3551),
    /// que o `finishCreation` do `%TypedArray%` chama logo depois de instalar `of` e `from`. Vigia a troca do
    /// `@@species` do `%TypedArray%`.
    ///
    /// DESVIO: o C++ compara com `typedArraySpeciesGetterSetter()` do global; aqui a condição usa o
    /// `GetterSetter` que `put_species_accessor` instalou no próprio construtor (ver `typed_array_constructor.rs`).
    fn install_constructor_species_watchpoint(&self, vm: &VM, constructor: &JSObjectHandle) {
        let species_name = PropertyName::from_identifier(&vm.property_names.species_symbol);
        let mut slot = PropertySlot::new(constructor.as_value(), InternalMethodType::VMInquiry);
        constructor.get_own_property_slot(vm, &species_name, &mut slot);
        constructor.structure().start_watching_property_for_replacements(vm, slot.cached_offset());
        let species_getter_setter = constructor.get_direct(slot.cached_offset());
        let condition = ObjectPropertyCondition::equivalence(
            constructor.clone(),
            vm.property_names.species_symbol.impl_().expect("@@species tem uid"),
            species_getter_setter,
        );
        let watchpoint = ObjectPropertyChangeAdaptiveWatchpoint::new(condition, Rc::clone(&self.constructor_species_watchpoint_set));
        watchpoint.install(vm);
        *self.constructor_species_watchpoint.borrow_mut() = Some(watchpoint);
    }

    /// `JSGlobalObject::tryInstallTypedArraySpeciesWatchpoint(type)` (JSGlobalObject.cpp:3537). O C++ chama
    /// isto de `speciesWatchpointIsValid` (JSGenericTypedArrayViewPrototypeFunctions.h:86) na primeira vez
    /// que o set do tipo ainda está `ClearWatchpoint`.
    pub fn try_install_species_watchpoint(&self, vm: &VM, global_object: &JSGlobalObject, type_: TypedArrayType) {
        let prototype = JSObject::from_value(&self.prototype(type_).as_value()).expect("protótipo de TypedArray é objeto");
        let constructor = JSObject::from_value(&self.constructor(type_).as_value()).expect("construtor de TypedArray é objeto");
        let watchpoint_set = self.species_watchpoint_set(type_);
        assert!(self.constructor_species_watchpoint.borrow().is_some());
        if constructor.get_prototype_direct() != self.super_constructor().as_value() {
            watchpoint_set.borrow_mut().invalidate(vm, &StringFireDetail::new("Was not able to set up species watchpoint."));
            return;
        }
        global_object.try_install_species_watchpoint(
            vm,
            &prototype,
            &constructor,
            &self.prototype_constructor_watchpoints[type_.to_index()],
            &self.constructor_species_absence_watchpoints[type_.to_index()],
            watchpoint_set,
            HasSpeciesProperty::No,
            JSValue::undefined(),
        );
    }

    /// `m_typedArrayXxxSpeciesWatchpointSet.state()`, para o consumidor (`speciesWatchpointIsValid`) instalar
    /// sob demanda como o C++.
    pub fn species_watchpoint_state(&self, type_: TypedArrayType) -> WatchpointState {
        self.species_watchpoint_set(type_).borrow().state()
    }

    /// A classe de `type_`, já criada por `init`.
    fn class(&self, type_: TypedArrayType) -> ClassStructure {
        self.classes.borrow().get(type_.to_index()).cloned().expect("JSGlobalObject sem as classes de TypedArray")
    }

    /// `typedArrayStructure(type, isResizableOrGrowableShared)`.
    pub fn structure(&self, type_: TypedArrayType, resizable_or_growable_shared: bool) -> StructureRef {
        if resizable_or_growable_shared {
            return self
                .resizable_or_growable_shared_structures
                .borrow()
                .get(type_.to_index())
                .cloned()
                .expect("JSGlobalObject sem as estruturas redimensionáveis de TypedArray");
        }
        self.class(type_).structure
    }

    /// `typedArrayPrototype(type)`.
    pub fn prototype(&self, type_: TypedArrayType) -> JSObjectRef {
        self.class(type_).prototype
    }

    /// `typedArrayConstructor(type)`.
    pub fn constructor(&self, type_: TypedArrayType) -> JSFunctionRef {
        self.class(type_).constructor
    }

    /// `m_typedArrayProto.get(this)`.
    pub fn super_prototype(&self) -> JSObjectRef {
        self.super_prototype.borrow().clone().expect("JSGlobalObject sem o %TypedArray%.prototype")
    }

    /// `m_typedArraySuperConstructor.get(this)`.
    pub fn super_constructor(&self) -> JSFunctionRef {
        self.super_constructor.borrow().clone().expect("JSGlobalObject sem o %TypedArray%")
    }

    /// O `INIT_TYPED_ARRAY_LATER` dos 12 tipos e o `initLater` do `%TypedArray%` e do seu protótipo, mais as
    /// propriedades globais e as `LinkTimeConstant`. `ArrayBufferRealm::init` chama isto depois de criar o
    /// `Object.prototype`, o `Function.prototype` e o `Array.prototype`.
    pub fn init(&self, vm: &VM, global_object: &JSGlobalObject, object_prototype: &JSObjectRef, function_prototype: JSValue) {
        install_private_functions(vm, global_object);

        let constructor_name = PropertyName::from_identifier(&vm.property_names.constructor);

        // `m_typedArrayProto`.
        let super_prototype_structure = TypedArrayViewPrototype::create_structure(vm, global_object, object_prototype.as_value());
        let super_prototype = TypedArrayViewPrototype::create(vm, global_object, &super_prototype_structure);
        super_prototype.did_become_prototype(vm);

        // `m_typedArraySuperConstructor`.
        let super_constructor_structure = TypedArrayViewConstructor::create_structure(vm, global_object, function_prototype);
        let super_constructor =
            TypedArrayViewConstructor::create(vm, global_object, super_constructor_structure, &super_prototype);
        super_prototype.put_direct(vm, &constructor_name, super_constructor.as_value(), DONT_ENUM);
        // `globalObject->installTypedArrayConstructorSpeciesWatchpoint(this)` do fim de
        // `JSTypedArrayViewConstructor::finishCreation`.
        self.install_constructor_species_watchpoint(
            vm,
            &JSObject::from_value(&super_constructor.as_value()).expect("%TypedArray% é objeto"),
        );
        let mut classes = Vec::with_capacity(TYPED_ARRAY_TYPES_EXCLUDING_DATA_VIEW.len());
        let mut resizable_structures = Vec::with_capacity(TYPED_ARRAY_TYPES_EXCLUDING_DATA_VIEW.len());
        for type_ in TYPED_ARRAY_TYPES_EXCLUDING_DATA_VIEW {
            let prototype_structure =
                TypedArrayViewPrototype::create_concrete_structure(vm, global_object, type_, super_prototype.as_value());
            let prototype = TypedArrayViewPrototype::create_concrete(vm, global_object, type_, &prototype_structure);
            prototype.did_become_prototype(vm);

            let structure = JSGenericTypedArrayView::create_structure(vm, Some(global_object), type_, prototype.as_value(), false);
            let resizable_structure =
                JSGenericTypedArrayView::create_structure(vm, Some(global_object), type_, prototype.as_value(), true);

            let constructor_structure =
                TypedArrayConstructor::create_structure(vm, global_object, type_, super_constructor.as_value());
            let constructor = TypedArrayConstructor::create(vm, global_object, constructor_structure, &prototype, type_);
            prototype.put_direct(vm, &constructor_name, constructor.as_value(), DONT_ENUM);

            global_object.set_link_time_constant(link_time_constant_for(type_), constructor.as_value());

            classes.push(ClassStructure { prototype, structure, constructor });
            resizable_structures.push(resizable_structure);
        }

        *self.super_prototype.borrow_mut() = Some(super_prototype);
        *self.super_constructor.borrow_mut() = Some(super_constructor);
        *self.classes.borrow_mut() = classes;
        *self.resizable_or_growable_shared_structures.borrow_mut() = resizable_structures;

        for type_ in GLOBAL_PROPERTY_ORDER {
            let name = Identifier::from_span(vm, type_.class_name().as_bytes());
            global_object.put_direct(vm, &PropertyName::from_identifier(&name), self.constructor(type_).as_value(), DONT_ENUM);
        }
    }
}

#[cfg(test)]
mod species_absence_tests {
    use super::*;
    use crate::runtime::property_name::PropertyName;

    /// A condição de ausência do `@@species` dos construtores concretos usa o `ObjectAdaptiveStructureWatchpoint`
    /// (JSGlobalObject.h: `m_typedArrayXxxConstructorSpeciesAbsenceWatchpoint`): instala sem pânico e o set dispara
    /// quando o construtor ganha um `@@species` próprio.
    #[test]
    fn species_absence_watchpoint_installs_and_fires() {
        let (vm, global_object) = crate::api::eval::new_global_object();
        let realm = &global_object.array_buffer_realm.typed_arrays;
        let type_ = TypedArrayType::BigUint64;
        assert_eq!(realm.species_watchpoint_state(type_), WatchpointState::ClearWatchpoint);
        realm.try_install_species_watchpoint(&vm, &global_object, type_);
        assert_eq!(realm.species_watchpoint_state(type_), WatchpointState::IsWatched);
        assert!(realm.constructor_species_absence_watchpoints[type_.to_index()].borrow().is_some());

        let constructor = JSObject::from_value(&realm.constructor(type_).as_value()).expect("construtor é objeto");
        let species = PropertyName::from_identifier(&vm.property_names.species_symbol);
        constructor.put_direct(&vm, &species, JSValue::undefined(), DONT_ENUM);
        assert_eq!(realm.species_watchpoint_state(type_), WatchpointState::IsInvalidated);
    }
}
