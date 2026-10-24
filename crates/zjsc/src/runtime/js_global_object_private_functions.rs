//! Os alvos nativos privados das `LinkTimeConstant` que o `JSGlobalObject::init` do C++ cria com
//! `JSFunction::create(..., ImplementationVisibility::Private)`: `globalFuncThrowTypeError`,
//! `globalFuncSetPrototypeDirect`, `globalFuncSetPrototypeDirectOrThrow`, `globalFuncCopyDataProperties`,
//! `globalFuncCloneObject`, `globalFuncToIntegerOrInfinity`, `globalFuncToLength`,
//! `globalFuncHandle{NegativeProxyHasTrap,ProxyGetTrap,PositiveProxySetTrap}Result` (todas de
//! `JSGlobalObjectFunctions.cpp`), `objectPrivateFuncInstanceOf` (`JSObject.cpp`) e `createPrivateSymbol`
//! (`JSGlobalObject.cpp`). A ligação em `m_linkTimeConstants` está em
//! `js_global_object_link_time_constants.rs`.
//!
//! DIVERGÊNCIAS:
//! - `globalFuncCopyDataProperties` e `globalFuncCloneObject` têm o caminho por `Structure` (`canPerformFastPropertyEnumeration`,
//!   leitura das entradas do `PropertyTable` e `putDirect`) e o genérico (`getOwnPropertyNames` com
//!   `DontEnumPropertiesMode::Include`, depois `getOwnPropertySlot` e `putDirectMayBeIndex`). Faltam
//!   `objectCloneFast` e `tryCreateObjectViaCloning` (clonam a `Structure` e o butterfly inteiros) e
//!   `putOwnDataPropertyBatching`, que dependem de `JSFinalObject` em lote, ausente: o resultado
//!   observável é o mesmo, via `putDirect` em ordem.
//! - `staticPropertiesReified`/`reifyAllStaticProperties` não existem no porte (a tabela estática é
//!   instalada por `put_direct` na criação), então a chamada é omitida.
//! - `makeTypeError` (`ErrorInstance::create(...)` com a mensagem como `JSValue`) é a versão sem captura de
//!   pilha (`useCurrentFrame` é `false` no C++, e o porte só captura em `create` de `error_instance.rs` quando
//!   há quadro). Os `mapPrivateFunc*`/`setPrivateFunc*` vivem em `ordered_hash_table_storage.rs`.

use crate::host_function;
use crate::runtime::error_instance::ErrorInstance;
use crate::runtime::error_natives::put_message_property;
use crate::runtime::error_type::ErrorType;
use crate::runtime::enumeration_mode::{DontEnumPropertiesMode, PropertyNameMode};
use crate::runtime::exception_helpers::create_invalid_prototype_error;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::host_function_support::default_has_instance;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::{js_boolean, js_number, js_undefined, JSValue};
use crate::runtime::object_constructor::{construct_empty_object, own_descriptor, own_names};
use crate::parser::parser::IdentifierSet;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::proxy_object::{object_get, ProxyObject};
use crate::runtime::sparse_array_value_map::PutDirectIndexMode;
use crate::runtime::string_regexp_support::to_wtf_string_value;
use crate::runtime::symbol::Symbol;
use crate::runtime::throw_scope::{throw_exception, ThrowScope};

/// `globalFuncThrowTypeError`: `throwVMTypeError(globalObject, scope)`, com a mensagem `"Type error"` de
/// `createTypeError(globalObject)`.
fn global_func_throw_type_error_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(Thrown::type_error("Type error"))
}
host_function!(pub global_func_throw_type_error, global_func_throw_type_error_body);

/// `globalFuncSetPrototypeDirect`: só grava se o valor é objeto ou `null`.
fn global_func_set_prototype_direct_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let value = call.argument(0);
    if value.is_object() || value.is_null() {
        let object = call.this_value().as_object();
        object.set_prototype_direct(global_object.vm(), value);
    }
    Ok(js_undefined())
}
host_function!(pub global_func_set_prototype_direct, global_func_set_prototype_direct_body);

/// `globalFuncSetPrototypeDirectOrThrow`.
fn global_func_set_prototype_direct_or_throw_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let value = call.argument(0);
    if !value.is_object() && !value.is_null() {
        let mut scope = ThrowScope::new(global_object.vm());
        throw_exception(global_object, &mut scope, create_invalid_prototype_error(global_object, value, None));
        return Err(Thrown::Pending);
    }
    let object = call.this_value().as_object();
    object.set_prototype_direct(global_object.vm(), value);
    Ok(js_undefined())
}
host_function!(pub global_func_set_prototype_direct_or_throw, global_func_set_prototype_direct_or_throw_body);

/// O caminho rápido de `globalFuncCopyDataProperties` e `globalFuncCloneObject`: com a `Structure` da origem
/// sem getters, sem `__proto__`, sem índices e sem `getOwnPropertySlot` próprio, lê (em duas fases, como o
/// C++) as chaves e os valores direto do `PropertyTable` e grava com `putDirect`. `true` se tratou o caso.
fn copy_via_structure(
    global_object: &JSGlobalObject,
    target: &JSObject,
    source: &JSObject,
    is_excluded: &impl Fn(&PropertyName) -> bool,
) -> bool {
    let vm = global_object.vm();
    let structure = source.structure();
    if !structure.can_perform_fast_property_enumeration() {
        return false;
    }
    let mut properties: Vec<(PropertyName, JSValue)> = Vec::new();
    for (key, offset, attributes, is_private) in structure.properties_with_privacy() {
        if is_private || attributes & DONT_ENUM != 0 {
            continue;
        }
        let name = PropertyName::from_uid(Some(key), false);
        if is_excluded(&name) {
            continue;
        }
        properties.push((name, source.get_direct(offset)));
    }
    // `putOwnDataPropertyBatching` grava as mesmas propriedades na mesma ordem que este laço.
    for (name, value) in &properties {
        target.put_direct(vm, name, *value, 0);
    }
    true
}

/// O laço genérico de `copyDataProperties` e `cloneObject`: cada propriedade própria enumerável de
/// `source` (strings e símbolos, sem privados) é gravada em `target` com `putDirectMayBeIndex`.
fn copy_enumerable_own_properties(
    global_object: &JSGlobalObject,
    target: &JSObject,
    source: &JSObject,
    is_excluded: impl Fn(&PropertyName) -> bool,
) -> Result<(), Thrown> {
    let vm = global_object.vm();
    // `globalFuncCopyDataProperties`/`globalFuncCloneObject`: a origem com propriedades estáticas ainda não
    // reificadas as reifica antes de qualquer leitura direta da `Structure`.
    if source.has_non_reified_static_properties(&source.structure()) {
        source.reify_all_static_properties(vm);
    }
    if copy_via_structure(global_object, target, source, &is_excluded) {
        return Ok(());
    }
    let names = own_names(global_object, source, PropertyNameMode::StringsAndSymbols, DontEnumPropertiesMode::Include)?;
    for identifier in &names {
        let name = PropertyName::from_identifier(identifier);
        debug_assert!(!name.is_private_name());
        if is_excluded(&name) {
            continue;
        }
        let Some(descriptor) = own_descriptor(global_object, source, &name)? else { continue };
        if !descriptor.enumerable() {
            continue;
        }
        let value = object_get(global_object, source, &name, source.as_value())?;
        match name.parse_index() {
            Some(index) => {
                target.put_direct_index(vm, index, value, 0, PutDirectIndexMode::PutDirectIndexLikePutDirect)?;
            }
            None => {
                target.put_direct(vm, &name, value, 0);
            }
        }
    }
    Ok(())
}

/// `globalFuncCopyDataProperties`: `this` é o `JSFinalObject` alvo, o argumento 0 a origem.
fn global_func_copy_data_properties_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let target_value = call.this_value();
    let target = target_value.as_object();
    debug_assert!(target.is_structure_extensible());

    let source_value = call.argument(0);
    if source_value.is_undefined_or_null() {
        return Ok(target_value);
    }
    let source = source_value.to_object(global_object).ok_or(Thrown::Pending)?;

    // `constantIdentifierSets()[setIndex]` do `CodeBlock` chamador, mais as chaves computadas dos
    // argumentos seguintes (`{[k]: x, ...rest}`).
    let mut excluded_set: Option<IdentifierSet> = None;
    if call.argument_count() > 1 {
        let set_index = as_uint32_as_any_int(call.argument(1)) as usize;
        let code_block = call.caller_code_block(global_object).expect("ASSERT(codeBlock): copyDataProperties chamada de builtin");
        let mut set = code_block.borrow().unlinked_code_block().borrow().constant_identifier_sets()[set_index].clone();
        for index in 2..call.argument_count() {
            // This isn't observable since ObjectPatternNode::bindValue() also performs ToPropertyKey.
            let property_name = property_name_argument(global_object, call, index)?;
            if let Some(uid) = property_name.uid() {
                set.add(uid, ());
            }
        }
        excluded_set = Some(set);
    }

    copy_enumerable_own_properties(global_object, &target, &source, |property_name| {
        debug_assert!(!property_name.is_private_name());
        match (&excluded_set, property_name.uid()) {
            (Some(set), Some(uid)) => set.contains(uid),
            _ => false,
        }
    })?;
    Ok(target_value)
}
host_function!(pub global_func_copy_data_properties, global_func_copy_data_properties_body);

/// `JSValue::asUInt32AsAnyInt()`: o `int32` ou o `double` inteiro, como `uint32`.
fn as_uint32_as_any_int(value: JSValue) -> u32 {
    if value.is_int32() { value.as_int32() as u32 } else { value.as_number() as u32 }
}

/// `globalFuncCloneObject`: `this` é a origem; `undefined` e `null` dão um objeto vazio.
fn global_func_clone_object_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let source_value = call.this_value();
    if source_value.is_undefined_or_null() {
        return Ok(construct_empty_object(global_object).as_value());
    }
    let source = source_value.to_object(global_object).ok_or(Thrown::Pending)?;
    let target = construct_empty_object(global_object);
    copy_enumerable_own_properties(global_object, &target, &source, |_| false)?;
    Ok(target.as_value())
}
host_function!(pub global_func_clone_object, global_func_clone_object_body);

/// `globalFuncToIntegerOrInfinity`.
fn global_func_to_integer_or_infinity_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let argument = call.argument(0);
    if argument.is_int32() {
        return Ok(argument);
    }
    Ok(js_number(argument.to_integer_or_infinity_checked()?))
}
host_function!(pub global_func_to_integer_or_infinity, global_func_to_integer_or_infinity_body);

/// `globalFuncToLength`.
fn global_func_to_length_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let argument = call.argument(0);
    if argument.is_int32() {
        return Ok(js_number(argument.as_int32().max(0)));
    }
    Ok(js_number(argument.to_length_checked()? as f64))
}
host_function!(pub global_func_to_length, global_func_to_length_body);

/// `objectPrivateFuncInstanceOf`: `JSObject::defaultHasInstance(globalObject, value, proto)`.
fn object_private_func_instance_of_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let result = default_has_instance(global_object, call.argument(0), call.argument(1)).ok_or(Thrown::Pending)?;
    Ok(js_boolean(result))
}
host_function!(pub object_private_func_instance_of, object_private_func_instance_of_body);

/// `callFrame->uncheckedArgument(index).toPropertyKey(globalObject)` como `PropertyName`.
fn property_name_argument(global_object: &JSGlobalObject, call: &HostCall, index: usize) -> Result<PropertyName, Thrown> {
    let key = call.argument(index).to_property_key(global_object).ok_or(Thrown::Pending)?;
    Ok(PropertyName::from_identifier(&key))
}

/// `globalFuncHandleNegativeProxyHasTrapResult`.
fn global_func_handle_negative_proxy_has_trap_result_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let target = call.argument(0).as_object();
    let property_name = property_name_argument(global_object, call, 1)?;
    ProxyObject::validate_negative_has_trap_result(global_object, &target, &property_name)?;
    Ok(js_undefined())
}
host_function!(
    pub global_func_handle_negative_proxy_has_trap_result,
    global_func_handle_negative_proxy_has_trap_result_body
);

/// `globalFuncHandleProxyGetTrapResult`.
fn global_func_handle_proxy_get_trap_result_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let trap_result = call.argument(0);
    let target = call.argument(1).as_object();
    let property_name = property_name_argument(global_object, call, 2)?;
    ProxyObject::validate_get_trap_result(global_object, trap_result, &target, &property_name)?;
    Ok(js_undefined())
}
host_function!(pub global_func_handle_proxy_get_trap_result, global_func_handle_proxy_get_trap_result_body);

/// `globalFuncHandlePositiveProxySetTrapResult`.
fn global_func_handle_positive_proxy_set_trap_result_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let target = call.argument(0).as_object();
    let property_name = property_name_argument(global_object, call, 1)?;
    let put_value = call.argument(2);
    ProxyObject::validate_positive_set_trap_result(global_object, &target, &property_name, put_value)?;
    Ok(js_undefined())
}
host_function!(
    pub global_func_handle_positive_proxy_set_trap_result,
    global_func_handle_positive_proxy_set_trap_result_body
);

/// `globalFuncMakeTypeError`: `ErrorInstance::create(globalObject, errorStructure(TypeError), argument(0),
/// undefined, nullptr, TypeNothing, ErrorType::TypeError, false)`. A mensagem `undefined` é a `String()`
/// nula (sem propriedade `message`); qualquer outra passa por `toWTFString`.
fn global_func_make_type_error_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let message_value = call.argument(0);
    let message = if message_value.is_undefined() { crate::wtf::text::wtf_string::String::default() } else { to_wtf_string_value(global_object, message_value)? };
    let instance = ErrorInstance::create(vm, global_object.error_structure_for(ErrorType::TypeError), message.clone(), ErrorType::TypeError);
    if !message.is_null() {
        put_message_property(vm, &instance, &message);
    }
    Ok(instance.as_value())
}
host_function!(pub global_func_make_type_error, global_func_make_type_error_body);

/// `createPrivateSymbol`: `Symbol::create(vm, PrivateSymbolImpl::create(*description.impl()).get())`.
fn create_private_symbol_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let description = to_wtf_string_value(global_object, call.argument(0))?;
    Ok(Symbol::create_private(global_object.vm(), &description).to_primitive())
}
host_function!(pub create_private_symbol, create_private_symbol_body);
