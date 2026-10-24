//! Porte de `runtime/JSCustomGetterFunction.{h,cpp}` e `runtime/JSCustomSetterFunction.{h,cpp}`: as
//! `JSFunction` que o `getOwnPropertyDescriptor` de uma propriedade `CustomAccessor` expõe ao JS
//! (`PropertyDescriptor::setPropertySlot`), cada uma guardando o ponteiro nativo e o nome da propriedade.
//!
//! DIVERGÊNCIAS:
//!
//! - As duas são subclasses de `JSFunction` no C++, com `ClassInfo` e `Structure`
//!   (`customGetterFunctionStructure()`/`customSetterFunctionStructure()`) próprios. Como
//!   `JSBoundFunction`, elas compartilham o `CellEntry::Function`: os dados moram em
//!   `JSFunction::custom_accessor` (vazio nas funções comuns), e a `Structure` é a `hostFunctionStructure`
//!   do global (o `ClassInfo` a mais não é observável).
//! - As duas só diferem no ponteiro, no prefixo do nome e no comprimento, então a criação é uma função
//!   só (`create`) e o `JSCustomGetterFunction::create`/`JSCustomSetterFunction::create` são os dois
//!   chamadores finos dela, sem repasse.
//! - `DOMAttributeAnnotation`/`DOMAttributeGetterSetter` não existem (são do DOM, que o porte não tem):
//!   o `domAttribute()` é sempre `std::nullopt`, então o ramo `throwVMDOMAttributeGetterTypeError` do
//!   getter não existe.
//! - Sem GC, `destroy` some.

use crate::interpreter::call_frame::NativeCallFrame;
use crate::runtime::host_call::HostCall;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction, JSFunctionRef};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_undefined, EncodedJSValue, JSValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::{GetValueFunc, PutValueFunc};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::{empty_string, make_string_by_joining, String as WtfString};

/// `CustomFunctionPointer`: o ponteiro nativo, que no C++ é `GetValueFunc` num e `PutValueFunc` no outro.
#[derive(Clone, Copy, Debug)]
pub enum CustomFunctionPointer {
    Getter(GetValueFunc),
    Setter(PutValueFunc),
    /// O acessor cujo ponteiro é uma `NativeFunction` própria (`create_host_custom_accessor_getter_function`): a função
    /// chama o ponteiro direto, e a marca só diz que ela é um `CustomAccessor` (o console do bun imprime o valor).
    Host,
}

/// `m_propertyName` e `m_getter`/`m_setter` de `JSCustomGetterFunction`/`JSCustomSetterFunction`.
#[derive(Clone, Debug)]
pub struct CustomAccessorFunction {
    property_name: Identifier,
    pointer: CustomFunctionPointer,
    /// O atributo nativo escrito à mão no bun (`HashTableValue` `CustomAccessor` sem `ReadOnly` e sem setter, como
    /// `MIMEType.prototype.essence`): a escrita é ignorada, mesmo em modo estrito, em vez de lançar.
    ignores_put: std::cell::Cell<bool>,
}

impl CustomAccessorFunction {
    /// Escrever neste acessor sem setter não lança.
    pub fn ignores_put(&self) -> bool {
        self.ignores_put.get()
    }

    /// Marca o acessor como de escrita ignorada.
    pub fn set_ignores_put(&self) {
        self.ignores_put.set(true);
    }

    /// `propertyName()`.
    pub fn property_name(&self) -> &Identifier {
        &self.property_name
    }

    /// `getter()`/`customFunctionPointer()` de `JSCustomGetterFunction`.
    pub fn getter(&self) -> Option<GetValueFunc> {
        match self.pointer {
            CustomFunctionPointer::Getter(getter) => Some(getter),
            CustomFunctionPointer::Setter(_) | CustomFunctionPointer::Host => None,
        }
    }

    /// `setter()`/`customFunctionPointer()` de `JSCustomSetterFunction`.
    pub fn setter(&self) -> Option<PutValueFunc> {
        match self.pointer {
            CustomFunctionPointer::Setter(setter) => Some(setter),
            CustomFunctionPointer::Getter(_) | CustomFunctionPointer::Host => None,
        }
    }
}

/// `uncheckedDowncast<JSCustomGetterFunction|JSCustomSetterFunction>(callFrame->jsCallee())`.
fn callee_custom_accessor(call: &HostCall) -> CustomAccessorFunction {
    let function = JSValue::from_cell(call.callee())
        .as_js_function()
        .expect("jsCallee de função custom accessor que não é um JSFunction");
    function
        .custom_accessor
        .get()
        .cloned()
        .expect("uncheckedDowncast<JSCustomGetterFunction|JSCustomSetterFunction> em função sem ponteiro nativo")
}

/// `customGetterFunctionCall(globalObject, callFrame)`.
fn custom_getter_function_call(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let call = HostCall::read(call_frame);
    let custom = callee_custom_accessor(&call);
    let getter = custom.getter().expect("customGetterFunctionCall em função que não é um JSCustomGetterFunction");
    getter(global_object, call.this_value().encode(), &PropertyName::from_identifier(custom.property_name()))
}

/// `customSetterFunctionCall(globalObject, callFrame)`.
fn custom_setter_function_call(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let call = HostCall::read(call_frame);
    let custom = callee_custom_accessor(&call);
    let setter = custom.setter().expect("customSetterFunctionCall em função que não é um JSCustomSetterFunction");
    setter(
        global_object,
        call.this_value().encode(),
        call.argument(0).encode(),
        &PropertyName::from_identifier(custom.property_name()),
    );
    js_undefined().encode()
}

/// O nome ECMAScript `"<prefix><name>"` posto à mão na função de host (`ensureRareData` +
/// `setHasReifiedName` + `putDirect(name)`), com o `NativeExecutable` guardando o nome puro: o `toString()`
/// sai `function foo() { [native code] }` e o `bind` parte de `foo`.
fn put_prefixed_name(vm: &VM, created: &JSFunctionRef, prefix: &str, public_name: &WtfString) {
    let name = make_string_by_joining(&[WtfString::from_latin1(prefix.as_bytes()), public_name.clone()], &empty_string());
    created.ensure_rare_data(vm).set_has_reified_name();
    created.put_direct(
        vm,
        &PropertyName::from_identifier(&vm.property_names.name),
        JSValue::from_js_string(js_string(vm, &name)),
        READ_ONLY | DONT_ENUM,
    );
}

/// O getter de host de um acessor `CustomAccessor` do C++ cujo ponteiro é uma `NativeFunction` própria
/// (`Intl.Locale.prototype.baseName`, `WebAssembly.Memory.prototype.buffer`, ...): o mesmo desenho de
/// `JSCustomGetterFunction::create`, com o `NativeExecutable` guardando o nome puro e a propriedade `name`
/// reificada como `get <name>`. Medido no bun: `name` é `get baseName`, mas `String(fn)`, `bind().name`
/// (`bound baseName`) e `Function.prototype.toString` partem de `baseName`.
pub fn create_host_custom_accessor_getter_function(
    vm: &VM,
    global_object: &JSGlobalObject,
    name: &str,
    getter: NativeFunction,
) -> JSFunctionRef {
    let public_name = WtfString::from_utf8(name.as_bytes());
    let created = JSFunction::create_native_with_own_structure(
        vm,
        global_object,
        0,
        &public_name,
        getter,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    );
    put_prefixed_name(vm, &created, "get ", &public_name);
    let _ = created.custom_accessor.set(CustomAccessorFunction {
        property_name: Identifier::from_string(vm, &public_name),
        pointer: CustomFunctionPointer::Host,
        ignores_put: std::cell::Cell::new(false),
    });
    created
}

/// O setter de host de um acessor `CustomAccessor` do C++: como [`create_host_custom_accessor_getter_function`],
/// com `set <name>` em `name` e `length` 1.
pub fn create_host_custom_accessor_setter_function(vm: &VM, global_object: &JSGlobalObject, name: &str, setter: NativeFunction) -> JSFunctionRef {
    let public_name = WtfString::from_utf8(name.as_bytes());
    let created = JSFunction::create_native_with_own_structure(
        vm,
        global_object,
        1,
        &public_name,
        setter,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    );
    put_prefixed_name(vm, &created, "set ", &public_name);
    created
}

/// O getter de `putDirectNativeIntrinsicGetter`/`reifyStaticAccessor`/`JSGlobalObject::__proto__`
/// (`RegExp.prototype.global`, `ArrayBuffer.prototype.byteLength`, `Map.prototype.size`, ...): o
/// `JSFunction::create(vm, globalObject, 0, makeString("get "_s, name), ...)` põe o `get <name>` no próprio
/// `NativeExecutable`, e a propriedade `name` sai dele, sem reificação à mão. Medido no bun: `name`,
/// `String(fn)` e `bind().name` enxergam `get <name>`. O setter de `__proto__` é igual com `set `.
pub fn create_host_getter_function(
    vm: &VM,
    global_object: &JSGlobalObject,
    name: &str,
    getter: NativeFunction,
    intrinsic: Intrinsic,
) -> JSFunctionRef {
    let prefixed_name = WtfString::from_utf8(format!("get {name}").as_bytes());
    JSFunction::create_native(
        vm,
        global_object,
        0,
        &prefixed_name,
        getter,
        ImplementationVisibility::Public,
        intrinsic,
        call_host_function_as_constructor,
    )
}

/// O corpo comum de `JSCustomGetterFunction::create` e `JSCustomSetterFunction::create`: a função de
/// host `function` (o `NativeExecutable` leva o nome puro, para o `toString()` sair `function foo() {
/// [native code] }`) com o ponteiro nativo guardado e o nome ECMAScript `"<prefix><name>"` posto à mão
/// (`ensureRareData` + `setHasReifiedName` + `putDirect(name)`).
fn create(
    vm: &VM,
    global_object: &JSGlobalObject,
    property_name: &PropertyName,
    prefix: &str,
    length: u32,
    function: NativeFunction,
    pointer: CustomFunctionPointer,
) -> JSFunctionRef {
    let public_name = property_name.public_name().map_or_else(empty_string, |uid| WtfString::from(std::rc::Rc::clone(&uid.0)));
    let created = JSFunction::create_native_with_own_structure(
        vm,
        global_object,
        length,
        &public_name,
        function,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    );
    let initialized = created
        .custom_accessor
        .set(CustomAccessorFunction { property_name: Identifier::from_uid(vm, property_name.uid()), pointer, ignores_put: std::cell::Cell::new(false) });
    debug_assert!(initialized.is_ok());
    put_prefixed_name(vm, &created, prefix, &public_name);
    created
}

/// `JSCustomGetterFunction::create(vm, globalObject, propertyName, getter)` (o `domAttribute` não existe).
pub fn create_custom_getter_function(
    vm: &VM,
    global_object: &JSGlobalObject,
    property_name: &PropertyName,
    getter: GetValueFunc,
) -> JSFunctionRef {
    create(vm, global_object, property_name, "get ", 0, custom_getter_function_call, CustomFunctionPointer::Getter(getter))
}

/// `JSCustomSetterFunction::create(vm, globalObject, propertyName, setter)`.
pub fn create_custom_setter_function(
    vm: &VM,
    global_object: &JSGlobalObject,
    property_name: &PropertyName,
    setter: PutValueFunc,
) -> JSFunctionRef {
    create(vm, global_object, property_name, "set ", 1, custom_setter_function_call, CustomFunctionPointer::Setter(setter))
}
