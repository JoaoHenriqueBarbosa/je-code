//! O que as classes nativas do bun (`TextEncoder`, `TextDecoder`) têm em comum: a chave de propriedade por
//! nome, o `TypeError`/`RangeError` com `code`, a estrutura de uma instância sem propriedade própria, o par
//! protótipo e construtor (protótipo herdando de `Object.prototype`, construtor `InternalFunction`) e a
//! propriedade global gravável, enumerável e configurável.

use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{collection_constructor_structure, create_collection_constructor_between};
use crate::runtime::identifier::Identifier;
use crate::runtime::internal_function::{InternalFunction, InternalFunctionRef};
use crate::runtime::js_custom_accessor_function::{create_host_custom_accessor_getter_function, create_host_custom_accessor_setter_function};
use crate::runtime::js_getter_setter::GetterSetter;
use crate::runtime::property_attribute::ACCESSOR;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSFinalObject, JSNonFinalObject, JSObject, JSObjectRef};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_undefined, JSValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// A chave de propriedade de `name`.
pub(crate) fn property_key(vm: &VM, name: &str) -> PropertyName {
    PropertyName::from_identifier(&Identifier::from_span(vm, name.as_bytes()))
}

pub(crate) use crate::runtime::node_error::{throw_coded_range_error, throw_coded_type_error, throw_native_type_error};

/// A estrutura de uma instância (objeto comum sem propriedade própria) com o protótipo dado.
pub(crate) fn instance_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
    JSFinalObject::create_structure(vm, global_object, prototype, JSFinalObject::DEFAULT_INLINE_CAPACITY)
}

/// O protótipo (herdando de `Object.prototype`, `ClassInfo` `prototype_info`) e o construtor nativo de
/// `name` (`length` 0, protótipo `Function.prototype`, `ClassInfo` `constructor_info`). Quem chama põe as
/// propriedades do protótipo, na ordem medida, e o `constructor`.
pub(crate) fn create_native_class(
    global_object: &JSGlobalObject,
    prototype_info: &'static ClassInfo,
    constructor_info: &'static ClassInfo,
    name: &str,
    call_function: NativeFunction,
    construct_function: NativeFunction,
) -> (JSObjectRef, InternalFunctionRef) {
    create_native_class_with_length(global_object, prototype_info, constructor_info, name, 0, call_function, construct_function)
}

/// [`create_native_class`] com o `length` do construtor dado (as estratégias de fila têm 1).
pub(crate) fn create_native_class_with_length(
    global_object: &JSGlobalObject,
    prototype_info: &'static ClassInfo,
    constructor_info: &'static ClassInfo,
    name: &str,
    length: u32,
    call_function: NativeFunction,
    construct_function: NativeFunction,
) -> (JSObjectRef, InternalFunctionRef) {
    let parents = (global_object.object_prototype().as_value(), global_object.function_prototype().as_value());
    create_native_subclass(global_object, parents, prototype_info, constructor_info, name, length, call_function, construct_function)
}

/// [`create_native_class_with_length`] de uma classe que herda de outra nativa: `parents` é o par
/// (protótipo, construtor) da classe base (`CustomEvent` herda de `Event`).
pub(crate) fn create_native_subclass(
    global_object: &JSGlobalObject,
    parents: (JSValue, JSValue),
    prototype_info: &'static ClassInfo,
    constructor_info: &'static ClassInfo,
    name: &str,
    length: u32,
    call_function: NativeFunction,
    construct_function: NativeFunction,
) -> (JSObjectRef, InternalFunctionRef) {
    create_native_subclass_with_statics(global_object, parents, prototype_info, constructor_info, name, length, call_function, construct_function, |_| {})
}

/// [`create_native_subclass`] com `statics` rodando depois de `length` e `name` e antes de `prototype` (a ordem das
/// chaves próprias de `Response`: `length`, `name`, `error`, `json`, `redirect`, `prototype`).
pub(crate) fn create_native_subclass_with_statics(
    global_object: &JSGlobalObject,
    parents: (JSValue, JSValue),
    prototype_info: &'static ClassInfo,
    constructor_info: &'static ClassInfo,
    name: &str,
    length: u32,
    call_function: NativeFunction,
    construct_function: NativeFunction,
    statics: impl FnOnce(&InternalFunction),
) -> (JSObjectRef, InternalFunctionRef) {
    create_native_subclass_with_hooks(
        global_object, parents, prototype_info, constructor_info, name, length, call_function, construct_function, |_| {}, statics,
    )
}

/// [`create_native_subclass_with_statics`] com `before_name` rodando antes de `length` e `name`: as estáticas que o bun
/// instala primeiro (`Buffer.alloc ... isEncoding`) saem na enumeração antes de `length`, `name` e `prototype`.
pub(crate) fn create_native_subclass_with_hooks(
    global_object: &JSGlobalObject,
    parents: (JSValue, JSValue),
    prototype_info: &'static ClassInfo,
    constructor_info: &'static ClassInfo,
    name: &str,
    length: u32,
    call_function: NativeFunction,
    construct_function: NativeFunction,
    before_name: impl FnOnce(&InternalFunction),
    statics: impl FnOnce(&InternalFunction),
) -> (JSObjectRef, InternalFunctionRef) {
    let vm = global_object.vm();
    let prototype_structure =
        Structure::create(vm, Some(global_object), parents.0, TypeInfo::new(JSType::ObjectType, JSNonFinalObject::STRUCTURE_FLAGS), prototype_info);
    let prototype = JSObject::allocate(vm, &prototype_structure);
    prototype.did_become_prototype(vm);
    let constructor_structure = collection_constructor_structure(vm, global_object, parents.1, constructor_info);
    let constructor = create_collection_constructor_between(
        vm,
        global_object,
        constructor_structure,
        &prototype,
        name,
        length,
        call_function,
        construct_function,
        false,
        before_name,
        statics,
    );
    (prototype, constructor)
}

/// Um acessor `CustomAccessor` de host sob `name`: getter `get <name>` (`length` 0) e, se houver, setter
/// `set <name>` (`length` 1); o `toString()` de cada um sai com o nome puro (`function name() { [native code] }`).
pub(crate) fn put_native_accessor(
    vm: &VM,
    global_object: &JSGlobalObject,
    object: &JSObject,
    name: &str,
    getter: NativeFunction,
    setter: Option<NativeFunction>,
    attributes: u32,
) {
    let getter = create_host_custom_accessor_getter_function(vm, global_object, name, getter);
    let setter = setter.map_or_else(js_undefined, |setter| create_host_custom_accessor_setter_function(vm, global_object, name, setter).as_value());
    let accessor = GetterSetter::create_from_values(vm, getter.as_value(), setter);
    // SEM transição: muta a estrutura do objeto, então o objeto precisa de estrutura própria (protótipo de
    // `instance_structure`, global). Um objeto de `construct_empty_object` a compartilha com os outros chamadores:
    // o mesmo nome posto duas vezes acusa em `Structure::add` (ver `timers::build_prototype`).
    object.put_direct_non_index_accessor_without_transition(vm, &property_key(vm, name), &accessor, attributes | ACCESSOR);
}

/// `globalThis[name] = constructor` como propriedade de dados comum (`writable`, `enumerable`, `configurable`).
pub(crate) fn install_global(global_object: &JSGlobalObject, name: &str, constructor: JSValue) {
    install_global_with_attributes(global_object, name, constructor, 0);
}

/// Função nativa global do bun (propriedade de dados comum, `length` dado, não construtor): `reportError`,
/// `queueMicrotask`, `postMessage`.
pub(crate) fn install_global_function(global_object: &JSGlobalObject, name: &str, length: u32, function: NativeFunction) {
    install_global_function_with_visibility(global_object, name, length, function, crate::runtime::implementation_visibility::ImplementationVisibility::Public);
}

/// Como [`install_global_function`], com a visibilidade escolhida: `Private` some do `Error.stack` (o nativo do
/// hospedeiro que a página não enxerga).
pub fn install_global_function_with_visibility(
    global_object: &JSGlobalObject,
    name: &str,
    length: u32,
    function: NativeFunction,
    visibility: crate::runtime::implementation_visibility::ImplementationVisibility,
) {
    let vm = global_object.vm();
    let identifier = Identifier::from_span(vm, name.as_bytes());
    let created = crate::runtime::js_function::JSFunction::create_native(
        vm,
        global_object,
        length,
        identifier.string().string(),
        function,
        visibility,
        crate::runtime::intrinsic::Intrinsic::NoIntrinsic,
        crate::runtime::js_function::call_host_function_as_constructor,
    );
    install_global(global_object, name, created.as_value());
}

/// [`install_global`] com os atributos dados (`DONT_ENUM` para os globais não enumeráveis do bun).
pub(crate) fn install_global_with_attributes(global_object: &JSGlobalObject, name: &str, constructor: JSValue, attributes: u32) {
    let vm = global_object.vm();
    // No próprio objeto global: `globalThis` é um `JSGlobalProxy`, e `putDirect` nele não encaminha ao alvo.
    global_object.put_direct(vm, &property_key(vm, name), constructor, attributes);
}
