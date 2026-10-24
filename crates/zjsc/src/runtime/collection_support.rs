//! Cola comum de `MapPrototype`/`MapConstructor`, `SetPrototype`/`SetConstructor`,
//! `WeakMapPrototype`/`WeakMapConstructor` e `WeakSetPrototype`/`WeakSetConstructor`: o `CollectionError`
//! das células puras (`js_map.rs`, `js_set.rs`, `js_weak_map.rs`, `js_weak_set.rs`) como exceção do
//! realm, o `JSC_GET_DERIVED_STRUCTURE` dos construtores e o corpo de `callMap`/`callSet`/`callWeakMap`/
//! `callWeakSet` (`throwConstructorCannotBeCalledAsFunctionTypeError`).
//!
//! DIVERGÊNCIA: o C++ lê a estrutura base de `globalObject->mapStructure()` (e as irmãs), que o
//! `JSGlobalObject` do porte ainda não guarda. O construtor tem a `prototype` própria (somente leitura e
//! não configurável), que é o mesmo protótipo do realm: a estrutura base sai de `create_structure` sobre
//! ela e o `StructureCache` (`create_subclass_structure`) devolve sempre a mesma estrutura para o par
//! protótipo e estrutura base.

use crate::interpreter::call_frame::NativeCallFrame;
use crate::runtime::exception_helpers::create_not_an_object_error;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::internal_function::{InternalFunction, InternalFunctionRef, PropertyAdditionMode};
use crate::runtime::iterator_operations::{call_checked, for_each_in_iterable, get_value_property};
use crate::runtime::js_array::construct_array;
use crate::runtime::js_function::{call_host_function_as_constructor, put_direct_native_function_without_transition, JSFunction, JSFunctionRef};
use crate::runtime::js_ordered_hash_table::IteratorStep;
use crate::runtime::js_string::js_string;
use crate::runtime::string_regexp_support::create_iterator_result_object;
use crate::runtime::js_getter_setter::GetterSetter;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_ordered_hash_table::CollectionError;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{EncodedJSValue, JSValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::property_attribute::{ACCESSOR, DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::wtf::text::wtf_string::String as WtfString;
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::runtime::vm::VM;

/// O que uma operação de coleção devolve no erro: a falha da célula pura ou a de uma chamada.
#[derive(Debug)]
pub enum CollectionFailure {
    Collection(CollectionError),
    Thrown(Thrown),
}

impl From<CollectionError> for CollectionFailure {
    fn from(error: CollectionError) -> CollectionFailure {
        CollectionFailure::Collection(error)
    }
}

impl From<Thrown> for CollectionFailure {
    fn from(thrown: Thrown) -> CollectionFailure {
        CollectionFailure::Thrown(thrown)
    }
}

/// `throwVMError(globalObject, scope, createNotAnObjectError(globalObject, thisValue))` e
/// `throwTypeError(globalObject, scope, message)` de `getMap`/`getSet`/`getWeakMap`/`getWeakSet`.
pub fn thrown_from_failure(global_object: &JSGlobalObject, failure: CollectionFailure) -> Thrown {
    match failure {
        CollectionFailure::Collection(CollectionError::NotAnObject(value)) => {
            let mut scope = ThrowScope::new(global_object.vm());
            throw_exception(global_object, &mut scope, create_not_an_object_error(global_object, value));
            Thrown::Pending
        }
        CollectionFailure::Collection(CollectionError::TypeError(message)) => Thrown::type_error(message),
        CollectionFailure::Thrown(thrown) => thrown,
    }
}

/// Roda o corpo de uma função nativa de coleção e converte a falha no `Thrown` do realm.
pub fn run_collection(
    global_object: &JSGlobalObject,
    body: impl FnOnce() -> Result<JSValue, CollectionFailure>,
) -> HostResult {
    body().map_err(|failure| thrown_from_failure(global_object, failure))
}

/// `JSC_GET_DERIVED_STRUCTURE(vm, mapStructure, newTarget, callFrame->jsCallee())` (ver a DIVERGÊNCIA do
/// cabeçalho): `create_base` é o `createStructure(vm, globalObject, prototype)` da célula.
pub fn derived_structure(
    global_object: &JSGlobalObject,
    call: &HostCall,
    create_base: fn(&VM, Option<&JSGlobalObject>, JSValue) -> StructureRef,
) -> Result<StructureRef, Thrown> {
    let (callee, base) = callee_base_structure(global_object, call, create_base)?;
    let new_target = ObjectRef::from_value(&call.new_target()).unwrap_or(callee);
    InternalFunction::create_subclass_structure(global_object, &new_target, base)
}

/// A estrutura de uma chamada sem `new` (`callX`): nunca lê `newTarget()`, que num quadro de chamada é o
/// próprio `this` (`Intl.Collator.call({})` ou `Intl.Collator()` com `this` igual ao objeto `Intl`).
pub fn callee_structure(
    global_object: &JSGlobalObject,
    call: &HostCall,
    create_base: fn(&VM, Option<&JSGlobalObject>, JSValue) -> StructureRef,
) -> Result<StructureRef, Thrown> {
    callee_base_structure(global_object, call, create_base).map(|(_, base)| base)
}

/// O callee e o `createStructure(vm, globalObject, callee.prototype)` da célula.
fn callee_base_structure(
    global_object: &JSGlobalObject,
    call: &HostCall,
    create_base: fn(&VM, Option<&JSGlobalObject>, JSValue) -> StructureRef,
) -> Result<(ObjectRef, StructureRef), Thrown> {
    let vm = global_object.vm();
    let callee = JSObject::from_cell_id(call.callee()).expect("o callee de um construtor de coleção é um JSObject (asObject(callFrame->jsCallee()))");
    let prototype = callee.get(vm, &PropertyName::from_identifier(&vm.property_names.prototype));
    Ok((ObjectRef::Handle(callee), create_base(vm, Some(global_object), prototype)))
}

/// `throwConstructorCannotBeCalledAsFunctionTypeError(globalObject, scope, constructorName)`.
pub fn constructor_cannot_be_called_as_function(constructor_name: &str) -> HostResult {
    Err(Thrown::type_error(&format!("calling {constructor_name} constructor without new is invalid")))
}

/// `JSFunction::create(vm, globalObject, length, name, function, Public, intrinsic)` com
/// `putDirectWithoutTransition(vm, name, function, DontEnum)` e o mesmo valor sob o nome privado do
/// `builtinNames` (`@clear`, `@delete`...), o par que `MapPrototype` e `SetPrototype` repetem.
pub fn put_function_with_private_name(
    vm: &VM,
    global_object: &JSGlobalObject,
    prototype: &JSObject,
    name: &Identifier,
    private_name: Identifier,
    length: u32,
    function: NativeFunction,
    intrinsic: Intrinsic,
) {
    let function = put_direct_native_function_without_transition(
        vm,
        global_object,
        prototype,
        name,
        length,
        function,
        ImplementationVisibility::Public,
        intrinsic,
        DONT_ENUM,
    );
    prototype.put_direct(vm, &PropertyName::from_identifier(&private_name), function.as_value(), DONT_ENUM);
}

/// `putDirectWithoutTransition(vm, name, function, DontEnum)` e o mesmo valor sob o nome privado do
/// `builtinNames`, para a função que já existe (`m_mapProtoEntriesFunction`, `m_setProtoValuesFunction`,
/// o `values` reaproveitado como `keys` e `@@iterator`).
pub fn put_existing_function_with_private_name(
    vm: &VM,
    prototype: &JSObject,
    name: &Identifier,
    private_name: Identifier,
    function: JSValue,
) {
    prototype.put_direct_without_transition(vm, &PropertyName::from_identifier(name), function, DONT_ENUM);
    prototype.put_direct(vm, &PropertyName::from_identifier(&private_name), function, DONT_ENUM);
}

/// `createIteratorResultObject(globalObject, ...)` do passo de `JSMapIterator::next` e
/// `JSSetIterator::next`: `{ value: undefined, done: true }` no fim, `{ value, done: false }` para um item
/// e, em `entries`, o par `[chave, valor]` (`constructArray`) como valor.
pub fn iterator_step_result(global_object: &JSGlobalObject, step: IteratorStep) -> JSValue {
    match iterator_step_value(global_object, step) {
        Some(value) => create_iterator_result_object(global_object, value, false),
        None => create_iterator_result_object(global_object, JSValue::undefined(), true),
    }
}

/// O valor de um passo de `JSMapIterator::next(globalObject, value)` e `JSSetIterator::next(globalObject,
/// value)`: `None` no fim, o item, ou o par `[chave, valor]` (`constructArray`) em `entries`.
pub fn iterator_step_value(global_object: &JSGlobalObject, step: IteratorStep) -> Option<JSValue> {
    match step {
        IteratorStep::Done => None,
        IteratorStep::Item(value) => Some(value),
        IteratorStep::Entry(key, value) => {
            let pair = construct_array(global_object.vm(), &global_object.array_structure(), &[key, value]);
            Some(pair.as_value())
        }
    }
}

/// O acessor `size` (`GetterSetter` do `JSFunction` `"get size"`) sob `size` e `@size`
/// (`DontEnum|Accessor`).
pub fn put_size_accessor(vm: &VM, global_object: &JSGlobalObject, prototype: &JSObject, getter: NativeFunction, intrinsic: Intrinsic) {
    let size_getter = JSFunction::create_native(
        vm,
        global_object,
        0,
        &WtfString::from_latin1(b"get size"),
        getter,
        ImplementationVisibility::Public,
        intrinsic,
        call_host_function_as_constructor,
    );
    let accessor = GetterSetter::create_from_values(vm, size_getter.as_value(), JSValue::undefined());
    let builtin_names = vm.property_names.builtin_names();
    for name in [vm.property_names.size.clone(), builtin_names.size_private_name()] {
        prototype.put_direct_non_index_accessor_without_transition(
            vm,
            &PropertyName::from_identifier(&name),
            &accessor,
            DONT_ENUM | ACCESSOR,
        );
    }
}

/// `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`: `@@toStringTag` com o nome da classe, `DontEnum|ReadOnly`.
pub fn put_to_string_tag(vm: &VM, object: &JSObject, class_name: &str) {
    object.put_direct(
        vm,
        &PropertyName::from_identifier(&vm.property_names.to_string_tag_symbol),
        JSValue::from_js_string(js_string(vm, &WtfString::from_latin1(class_name.as_bytes()))),
        DONT_ENUM | READ_ONLY,
    );
}

/// `JSC_NATIVE_FUNCTION_WITHOUT_TRANSITION(name, function, DontEnum, length, Public)` e a variante com
/// intrínseco (`JSC_NATIVE_INTRINSIC_FUNCTION_WITHOUT_TRANSITION`).
pub fn put_native_function(
    vm: &VM,
    global_object: &JSGlobalObject,
    object: &JSObject,
    name: &Identifier,
    length: u32,
    function: NativeFunction,
    intrinsic: Intrinsic,
) {
    put_direct_native_function_without_transition(
        vm,
        global_object,
        object,
        name,
        length,
        function,
        ImplementationVisibility::Public,
        intrinsic,
        DONT_ENUM,
    );
}

/// `globalFuncSpeciesGetter` (`get [Symbol.species]`): devolve o `this`.
pub fn global_func_species_getter(
    _global_object: &JSGlobalObject,
    call_frame: &mut NativeCallFrame<'_>,
) -> EncodedJSValue {
    crate::runtime::proxy_object::to_this_strict(call_frame.this_value()).encode()
}

/// O acessor `@@species` de um construtor (`GetterSetter::create(vm, globalObject, JSFunction::create(vm,
/// globalObject, 0, "get [Symbol.species]", globalFuncSpeciesGetter, Public, SpeciesGetterIntrinsic),
/// nullptr)` e `putDirectNonIndexAccessorWithoutTransition(vm, speciesSymbol, ..., Accessor | ReadOnly |
/// DontEnum)`), o que `Array`, `Map`, `Set`, `Promise`, `RegExp` e `ArrayBuffer` fazem em `finishCreation`.
pub fn put_species_accessor(vm: &VM, global_object: &JSGlobalObject, constructor: &JSObject) {
    let species_getter = JSFunction::create_native(
        vm,
        global_object,
        0,
        &WtfString::from_latin1(b"get [Symbol.species]"),
        global_func_species_getter,
        ImplementationVisibility::Public,
        Intrinsic::SpeciesGetterIntrinsic,
        call_host_function_as_constructor,
    );
    let species = GetterSetter::create_from_values(vm, species_getter.as_value(), JSValue::undefined());
    constructor.put_direct_non_index_accessor_without_transition(
        vm,
        &PropertyName::from_identifier(&vm.property_names.species_symbol),
        &species,
        ACCESSOR | READ_ONLY | DONT_ENUM,
    );
}

/// `createStructure(vm, globalObject, prototype)` de `MapConstructor` e irmãs: `InternalFunctionType`
/// com o `ClassInfo` `"Function"` da classe (`info`).
pub fn collection_constructor_structure(
    vm: &VM,
    global_object: &JSGlobalObject,
    prototype: JSValue,
    info: &'static ClassInfo,
) -> StructureRef {
    internal_function_structure(vm, global_object, prototype, info, InternalFunction::STRUCTURE_FLAGS)
}

/// A estrutura `InternalFunctionType` com as `flags` da classe (as de `InternalFunction`, ou elas sem
/// `ImplementsDefaultHasInstance` num construtor com `instanceof` próprio, como o `File` do bun).
pub fn internal_function_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue, info: &'static ClassInfo, flags: u32) -> StructureRef {
    Structure::create(
        vm,
        Some(global_object),
        prototype,
        // `StructureFlags = Base::StructureFlags | HasStaticPropertyTable` nas classes com `staticPropHashTable`.
        TypeInfo::new(
            JSType::InternalFunctionType,
            flags | if info.static_prop_hash_table.is_some() { crate::runtime::js_type_info::HAS_STATIC_PROPERTY_TABLE } else { 0 },
        ),
        info,
    )
}

/// `MapConstructor::create(vm, structure, prototype)` e irmãs: `InternalFunction(vm, structure, callX,
/// constructX)` e `finishCreation(vm, prototype)` (`length`, `name`, `prototype` `DontEnum|DontDelete|
/// ReadOnly` e, com `species`, o acessor `@@species` `ReadOnly|DontEnum`). `length` é 0 nas coleções e 1 em
/// `ArrayBuffer` e `DataView`, que repetem o mesmo `finishCreation`.
pub fn create_collection_constructor(
    vm: &VM,
    global_object: &JSGlobalObject,
    structure: StructureRef,
    prototype: &JSObject,
    name: &str,
    length: u32,
    call_function: NativeFunction,
    construct_function: NativeFunction,
    with_species: bool,
) -> InternalFunctionRef {
    create_collection_constructor_with(vm, global_object, structure, prototype, name, length, call_function, construct_function, with_species, |_| {})
}

/// `create_collection_constructor`, com `before_finish` rodando antes de `length` e `name` entrarem: as
/// propriedades da tabela estática do C++ (`supportedLocalesOf`) aparecem na enumeração antes delas.
pub fn create_collection_constructor_with(
    vm: &VM,
    global_object: &JSGlobalObject,
    structure: StructureRef,
    prototype: &JSObject,
    name: &str,
    length: u32,
    call_function: NativeFunction,
    construct_function: NativeFunction,
    with_species: bool,
    before_finish: impl FnOnce(&InternalFunction),
) -> InternalFunctionRef {
    create_collection_constructor_between(vm, global_object, structure, prototype, name, length, call_function, construct_function, with_species, before_finish, |_| {})
}

/// `create_collection_constructor_with`, com `after_name` rodando depois de `length` e `name` e antes de `prototype`: as
/// estáticas que o bun instala no construtor (`Response.error`) saem na enumeração entre `name` e `prototype`.
pub fn create_collection_constructor_between(
    vm: &VM,
    global_object: &JSGlobalObject,
    structure: StructureRef,
    prototype: &JSObject,
    name: &str,
    length: u32,
    call_function: NativeFunction,
    construct_function: NativeFunction,
    with_species: bool,
    before_finish: impl FnOnce(&InternalFunction),
    after_name: impl FnOnce(&InternalFunction),
) -> InternalFunctionRef {
    let constructor = InternalFunction::new(vm, structure, call_function, Some(construct_function));
    before_finish(&constructor);
    constructor.finish_creation(vm, length, &WtfString::from_latin1(name.as_bytes()), PropertyAdditionMode::WithoutStructureTransition);
    after_name(&constructor);
    constructor.put_direct(
        vm,
        &PropertyName::from_identifier(&vm.property_names.prototype),
        prototype.as_value(),
        DONT_ENUM | DONT_DELETE | READ_ONLY,
    );
    if with_species {
        put_species_accessor(vm, global_object, &constructor);
    }
    constructor
}

/// `createStructure` de um construtor que no bun é `JSFunction` sobre `NativeExecutable` (`ArrayBuffer`,
/// `DataView`): `JSFunctionType`, com o `ClassInfo` `"Function"` da classe (`info`, cujo pai é
/// `JS_FUNCTION_S_INFO`).
pub fn native_constructor_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue, info: &'static ClassInfo) -> StructureRef {
    Structure::create(vm, Some(global_object), prototype, TypeInfo::new(JSType::JSFunctionType, JSFunction::STRUCTURE_FLAGS), info)
}

/// `create_collection_constructor` para os construtores `JSFunction` sobre `NativeExecutable`: `length` e `name`
/// preguiçosos (saem primeiro no `ownKeys`), `prototype` (`DontEnum|DontDelete|ReadOnly`) e, com `species`,
/// o acessor `@@species`.
pub fn create_native_collection_constructor(
    vm: &VM,
    global_object: &JSGlobalObject,
    structure: StructureRef,
    prototype: &JSObject,
    name: &str,
    length: u32,
    call_function: NativeFunction,
    construct_function: NativeFunction,
    with_species: bool,
) -> JSFunctionRef {
    let constructor = JSFunction::create_native_with_structure(
        vm,
        global_object,
        structure,
        length,
        &WtfString::from_latin1(name.as_bytes()),
        call_function,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        construct_function,
    );
    constructor.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.prototype), prototype.as_value(), DONT_ENUM | DONT_DELETE | READ_ONLY);
    if with_species {
        put_species_accessor(vm, global_object, &constructor);
    }
    constructor
}

/// O que difere entre `constructMap`, `constructSet`, `constructWeakMap` e `constructWeakSet`: todos
/// criam a célula sobre a estrutura derivada, e, com `iterable`, buscam o adicionador (`set`/`add`) na
/// célula nova e o chamam para cada item de `forEachInIterable`.
///
/// DIVERGÊNCIA: o C++ tem o atalho `canPerformFastSet`/`canPerformFastAdd` (adicionador original, sem
/// observação) que grava direto na tabela e, em `Map`/`Set`, o `clone` quando o iterável é um `JSMap`/
/// `JSSet` de iterador intacto. O caminho observável é o mesmo: aqui sempre vale o genérico, que chama o
/// adicionador (o original faz exatamente o que o atalho faz, com as mesmas mensagens de erro).
pub struct CollectionConstructor {
    /// `JSMap::createStructure` e irmãs.
    pub create_structure: fn(&VM, Option<&JSGlobalObject>, JSValue) -> StructureRef,
    /// `JSMap::create(vm, structure)` e irmãs: o valor da célula nova.
    pub create_cell: fn(&VM, &StructureRef) -> JSValue,
    /// `"set"` (`Map`, `WeakMap`) ou `"add"` (`Set`, `WeakSet`).
    pub adder_name: &'static str,
    /// `"'set' property of a Map should be callable."` e as irmãs.
    pub adder_not_callable_message: &'static str,
    /// `Some(mensagem)` para `Map`/`WeakMap`, que leem `[0]` e `[1]` de cada item objeto; `None`
    /// (`Set`/`WeakSet`) passa o item inteiro. A mensagem é a do item que não é objeto (`""` é o
    /// `throwTypeError(globalObject, scope)` sem mensagem de `Map`).
    pub entry_not_object_message: Option<&'static str>,
}

impl CollectionConstructor {
    /// O corpo de `constructMap` e irmãs.
    pub fn construct(&self, global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
        let vm = global_object.vm();
        let structure = derived_structure(global_object, call, self.create_structure)?;
        let cell = (self.create_cell)(vm, &structure);
        // A coleção nova e o `adder` vivem só neste local Rust enquanto o iterável roda JS.
        let _cell = crate::runtime::gc::RootScope::new(&[cell]);

        let iterable = call.argument(0);
        if iterable.is_undefined_or_null() {
            return Ok(cell);
        }

        let adder_name = Identifier::from_span(vm, self.adder_name.as_bytes());
        let adder = get_value_property(global_object, cell, &PropertyName::from_identifier(&adder_name))?;
        let _adder = crate::runtime::gc::RootScope::new(&[adder]);
        if !adder.is_callable() {
            return Err(Thrown::type_error(self.adder_not_callable_message));
        }

        for_each_in_iterable(global_object, iterable, |next_item| -> Result<(), Thrown> {
            let Some(not_object_message) = self.entry_not_object_message else {
                call_checked(global_object, adder, cell, &[next_item], self.adder_not_callable_message)?;
                return Ok(());
            };
            if !next_item.is_object() {
                return Err(Thrown::type_error(if not_object_message.is_empty() { "Type error" } else { not_object_message }));
            }
            let next_object = next_item.as_object();
            let key = next_object.get_by_index(vm, 0);
            if vm.exception().is_some() {
                return Err(Thrown::Pending);
            }
            let value = next_object.get_by_index(vm, 1);
            if vm.exception().is_some() {
                return Err(Thrown::Pending);
            }
            call_checked(global_object, adder, cell, &[key, value], self.adder_not_callable_message)?;
            Ok(())
        })?;
        Ok(cell)
    }
}

/// Define o que é igual em `MapConstructor`, `SetConstructor`, `WeakMapConstructor` e
/// `WeakSetConstructor`: o `ClassInfo` `"Function"`, o `callX`, o `constructX`, a struct sem campos com
/// `create_structure` e `create`.
///
/// `define_collection_constructor!(MapConstructor, MAP_CONSTRUCTOR_S_INFO, JSMap, "Map", "set",
/// Some(""), true, call_map, construct_map)`: `entry` é o `entry_not_object_message` de
/// `CollectionConstructor` e o penúltimo argumento antes dos nomes é o `@@species`.
#[macro_export]
macro_rules! define_collection_constructor {
    ($struct:ident, $info:ident, $cell:ident, $name:literal, $adder:literal, $entry:expr, $species:literal, $call_fn:ident, $construct_fn:ident) => {
        /// `const ClassInfo ::s_info` (`"Function"`, base `JSFunction`: no bun é um `JSFunction` sobre
        /// `NativeExecutable`).
        pub static $info: $crate::runtime::class_info::ClassInfo = $crate::runtime::class_info::ClassInfo {
            class_name: "Function",
            parent_class: Some(&$crate::runtime::js_function::JS_FUNCTION_S_INFO),
            static_prop_hash_table: None, inherits_js_type_range: None,
        };

        fn create_cell(vm: &$crate::runtime::vm::VM, structure: &$crate::runtime::structure::StructureRef) -> $crate::runtime::js_value::JSValue {
            $cell::create(vm, structure).as_value()
        }

        static CONSTRUCT: $crate::runtime::collection_support::CollectionConstructor =
            $crate::runtime::collection_support::CollectionConstructor {
                create_structure: $cell::create_structure,
                create_cell,
                adder_name: $adder,
                adder_not_callable_message: concat!("'", $adder, "' property of a ", $name, " should be callable."),
                entry_not_object_message: $entry,
            };

        /// `callX`: `throwConstructorCannotBeCalledAsFunctionTypeError`.
        fn call_body(
            _global_object: &$crate::runtime::js_global_object::JSGlobalObject,
            _call: &$crate::runtime::host_call::HostCall,
        ) -> $crate::runtime::host_call::HostResult {
            $crate::runtime::collection_support::constructor_cannot_be_called_as_function($name)
        }

        /// `constructX`.
        fn construct_body(
            global_object: &$crate::runtime::js_global_object::JSGlobalObject,
            call: &$crate::runtime::host_call::HostCall,
        ) -> $crate::runtime::host_call::HostResult {
            CONSTRUCT.construct(global_object, call)
        }

        $crate::host_function!($call_fn, call_body);
        $crate::host_function!($construct_fn, construct_body);

        /// Sem campos próprios: é um `JSFunction`.
        pub struct $struct;

        impl $struct {
            /// `StructureFlags = Base::StructureFlags`.
            pub const STRUCTURE_FLAGS: u32 = $crate::runtime::js_function::JSFunction::STRUCTURE_FLAGS;

            /// `createStructure(vm, globalObject, prototype)`.
            pub fn create_structure(
                vm: &$crate::runtime::vm::VM,
                global_object: &$crate::runtime::js_global_object::JSGlobalObject,
                prototype: $crate::runtime::js_value::JSValue,
            ) -> $crate::runtime::structure::StructureRef {
                $crate::runtime::collection_support::native_constructor_structure(vm, global_object, prototype, &$info)
            }

            /// `create(vm, structure, prototype)`: o construtor e o `finishCreation(vm, prototype)`.
            pub fn create(
                vm: &$crate::runtime::vm::VM,
                global_object: &$crate::runtime::js_global_object::JSGlobalObject,
                structure: $crate::runtime::structure::StructureRef,
                prototype: &$crate::runtime::js_object::JSObject,
            ) -> $crate::runtime::js_function::JSFunctionRef {
                $crate::runtime::collection_support::create_native_collection_constructor(
                    vm,
                    global_object,
                    structure,
                    prototype,
                    $name,
                    0,
                    $call_fn,
                    $construct_fn,
                    $species,
                )
            }
        }
    };
}

/// Define o que é igual em `MapIteratorPrototype` e `SetIteratorPrototype`: o `ClassInfo`, o
/// `JSMapIteratorNextIntrinsic`/`JSSetIteratorNextIntrinsic` do `next` (`mapIteratorProtoFuncNext` e
/// `setIteratorProtoFuncNext`: o passo vem de `step`, o `createIteratorResultObject` é
/// `iterator_step_result`), `finishCreation` (`next` e `@@toStringTag`) e o `create_structure`/`create`.
///
/// `define_collection_iterator_prototype!(MapIteratorPrototype, MAP_ITERATOR_PROTOTYPE_S_INFO, "Map Iterator",
/// Intrinsic::JSMapIteratorNextIntrinsic, map_iterator_proto_next, map_iterator_proto_func_next)`.
#[macro_export]
macro_rules! define_collection_iterator_prototype {
    ($struct:ident, $info:ident, $class_name:literal, $intrinsic:expr, $step:path, $next_fn:ident) => {
        /// `const ClassInfo ::s_info`.
        pub static $info: $crate::runtime::class_info::ClassInfo = $crate::runtime::class_info::ClassInfo {
            class_name: $class_name,
            parent_class: Some(&$crate::runtime::js_object::JS_NON_FINAL_OBJECT_S_INFO),
            static_prop_hash_table: None, inherits_js_type_range: None,
        };

        /// `mapIteratorProtoFuncNext` e `setIteratorProtoFuncNext`.
        fn next_body(
            global_object: &$crate::runtime::js_global_object::JSGlobalObject,
            call: &$crate::runtime::host_call::HostCall,
        ) -> $crate::runtime::host_call::HostResult {
            $crate::runtime::collection_support::run_collection(global_object, || {
                Ok($crate::runtime::collection_support::iterator_step_result(global_object, $step(call.this_value())?))
            })
        }

        $crate::host_function!($next_fn, next_body);

        /// `class ...IteratorPrototype final : public JSNonFinalObject`: sem campos próprios.
        pub struct $struct;

        impl $struct {
            /// `StructureFlags = Base::StructureFlags`.
            pub const STRUCTURE_FLAGS: u32 = $crate::runtime::js_object::JSNonFinalObject::STRUCTURE_FLAGS;

            /// `createStructure(vm, globalObject, prototype)`.
            pub fn create_structure(
                vm: &$crate::runtime::vm::VM,
                global_object: &$crate::runtime::js_global_object::JSGlobalObject,
                prototype: $crate::runtime::js_value::JSValue,
            ) -> $crate::runtime::structure::StructureRef {
                $crate::runtime::structure::Structure::create(
                    vm,
                    Some(global_object),
                    prototype,
                    $crate::runtime::js_type_info::TypeInfo::new($crate::runtime::js_type::JSType::ObjectType, $struct::STRUCTURE_FLAGS),
                    &$info,
                )
            }

            /// `create(vm, globalObject, structure)`: o construtor da classe e o `finishCreation`.
            pub fn create(
                vm: &$crate::runtime::vm::VM,
                global_object: &$crate::runtime::js_global_object::JSGlobalObject,
                structure: &$crate::runtime::structure::StructureRef,
            ) -> $crate::runtime::js_object::JSObjectRef {
                let prototype = $crate::runtime::js_object::JSObject::allocate(vm, structure);
                $struct::finish_creation(&prototype, vm, global_object);
                prototype
            }

            /// `finishCreation(vm, globalObject)`: `next` (`DontEnum`, `length` 0) e `@@toStringTag`.
            fn finish_creation(
                prototype: &$crate::runtime::js_object::JSObject,
                vm: &$crate::runtime::vm::VM,
                global_object: &$crate::runtime::js_global_object::JSGlobalObject,
            ) {
                prototype.finish_creation(vm);
                $crate::runtime::collection_support::put_native_function(vm, global_object, prototype, &vm.property_names.next, 0, $next_fn, $intrinsic);
                $crate::runtime::collection_support::put_to_string_tag(vm, prototype, $info.class_name);
            }
        }
    };
}
