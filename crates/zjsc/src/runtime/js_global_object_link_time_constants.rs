//! As `m_linkTimeConstants[...]` de `JSGlobalObject::init` (`runtime/JSGlobalObject.cpp:1196-2254`) que
//! os outros módulos do porte ainda não ligam: os `INIT_PRIVATE_GLOBAL` de
//! `JSC_FOREACH_BUILTIN_LINK_TIME_CONSTANT`, os construtores (`Object`, `Array`, `RegExp`, `String`,
//! `AggregateError`, `ReferenceError`, `SuppressedError`), os `GetterSetter` de `RegExp.prototype`, as
//! funções que o C++ tira de protótipos (`regExpBuiltinExec`, `regExpPrototypeSymbol*`,
//! `hasOwnPropertyFunction`, `callFunction`, `applyFunction`) e as `JSFunction` privadas cujo corpo
//! nativo já existe no porte. A lista completa, com o que falta e do que depende, está em
//! `wip-notes/link-time-constants.md`.
//!
//! DIVERGÊNCIAS:
//! - O C++ guarda cada uma num `LazyProperty` (`initLater`) que roda na primeira leitura; aqui todas são
//!   criadas de uma vez, no fim de `init`, como o resto do porte faz (`set_link_time_constant`).
//! - Os construtores são lidos das propriedades do global (`DontEnum`, recém-gravadas por `init`) porque o
//!   `JSGlobalObject` do porte não tem os campos `m_objectConstructor`, `m_stringConstructor` e
//!   `m_xxxErrorStructure.constructor()`; no fim de `init` elas ainda são as originais.
//! - O `Array` só é ligado se `init` já criou o `ArrayConstructor` (hoje não cria): sem a propriedade a
//!   constante fica vazia e `link_time_constant` aborta ao lê-la, em vez de gravar um valor inventado.
//! - Os cinco `INIT_PRIVATE_GLOBAL` que `js_global_object_init.rs` já cria (`arrayIteratorNextHelper` e
//!   os quatro de `DisposableStackPrototype.js`) não se repetem aqui.

use crate::bytecode::bytecode_intrinsics_table::LinkTimeConstant;
use crate::runtime::array_constructor::{array_constructor_is_array_host, array_constructor_private_from_fast_without_map_fn_host};
use crate::runtime::builtins_source::BuiltinCodeIndex;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{call_host_function_as_constructor, create_builtin_function, JSFunction};
use crate::runtime::js_getter_setter::GetterSetter;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_global_object_functions_natives::global_func_is_finite;
use crate::runtime::js_global_object_private_functions::{
    create_private_symbol, global_func_clone_object, global_func_copy_data_properties,
    global_func_handle_negative_proxy_has_trap_result, global_func_handle_positive_proxy_set_trap_result,
    global_func_handle_proxy_get_trap_result, global_func_make_type_error, global_func_set_prototype_direct,
    global_func_set_prototype_direct_or_throw, global_func_throw_type_error, global_func_to_integer_or_infinity,
    global_func_to_length, object_private_func_instance_of,
};
use crate::runtime::json_object_native::{json_proto_func_parse, json_proto_func_stringify};
use crate::runtime::math_object::math_proto_func_min;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::object_constructor::object_constructor_is;
use crate::runtime::ordered_hash_table_storage::{
    map_private_func_map_iteration_entry, map_private_func_map_iteration_entry_key,
    map_private_func_map_iteration_entry_value, map_private_func_map_iteration_next, map_private_func_map_storage,
    set_private_func_set_iteration_entry, set_private_func_set_iteration_entry_key,
    set_private_func_set_iteration_next, set_private_func_set_storage,
};
use crate::runtime::property_name::PropertyName;
use crate::runtime::reflect_object::reflect_object_own_keys;
use crate::runtime::string_prototype_natives::string_proto_func_substring;
use crate::wtf::text::wtf_string::String as WtfString;

/// Uma `JSFunction::create(vm, owner, length, name, function, visibility, intrinsic)` guardada num
/// `LinkTimeConstant`.
struct NativeLinkTimeFunction {
    constant: LinkTimeConstant,
    length: u32,
    name: &'static str,
    function: NativeFunction,
    visibility: ImplementationVisibility,
    intrinsic: Intrinsic,
}

/// Liga os `LinkTimeConstant` descritos no cabeçalho. Deve rodar no fim de `JSGlobalObject::init`, depois
/// de `Object`, `String`, `RegExp`, dos erros derivados e de `Function.prototype`/`Object.prototype`/
/// `RegExp.prototype` terem suas propriedades.
pub fn init_link_time_constants(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let name = |identifier: &Identifier| PropertyName::from_identifier(identifier);
    let by_span = |text: &[u8]| PropertyName::from_identifier(&Identifier::from_span(vm, text));

    // `INIT_PRIVATE_GLOBAL(funcName, code)` de `JSC_FOREACH_BUILTIN_LINK_TIME_CONSTANT`:
    // `JSFunction::create(vm, globalObject, code ## CodeGenerator(vm), globalObject)`.
    use BuiltinCodeIndex as Code;
    let builtin_code_constants = [
        (LinkTimeConstant::BuiltinMapIterable, Code::IteratorHelpersBuiltinMapIterableCode),
        (LinkTimeConstant::BuiltinSetIterable, Code::IteratorHelpersBuiltinSetIterableCode),
        (LinkTimeConstant::CloseAllIterators, Code::JsIteratorConstructorCloseAllIteratorsCode),
        (LinkTimeConstant::CreateArrayWithoutPrototype, Code::InjectedScriptSourceCreateArrayWithoutPrototypeCode),
        (LinkTimeConstant::CreateInspectorInjectedScript, Code::InjectedScriptSourceCreateInspectorInjectedScriptCode),
        (LinkTimeConstant::CreateObjectWithoutPrototype, Code::InjectedScriptSourceCreateObjectWithoutPrototypeCode),
        (LinkTimeConstant::CrossRealmThrow, Code::ShadowRealmPrototypeCrossRealmThrowCode),
        (LinkTimeConstant::DefaultAsyncFromAsyncArrayLike, Code::ArrayConstructorDefaultAsyncFromAsyncArrayLikeCode),
        (LinkTimeConstant::DefaultAsyncFromAsyncIterator, Code::ArrayConstructorDefaultAsyncFromAsyncIteratorCode),
        (LinkTimeConstant::FlatIntoArray, Code::ArrayPrototypeFlatIntoArrayCode),
        (LinkTimeConstant::FlatIntoArrayWithCallback, Code::ArrayPrototypeFlatIntoArrayWithCallbackCode),
        (LinkTimeConstant::GeneratorResume, Code::GeneratorPrototypeGeneratorResumeCode),
        (LinkTimeConstant::GetIteratorFlattenable, Code::JsIteratorConstructorGetIteratorFlattenableCode),
        (LinkTimeConstant::GetIteratorSync, Code::JsIteratorConstructorGetIteratorSyncCode),
        (LinkTimeConstant::GetOptionsObject, Code::JsIteratorConstructorGetOptionsObjectCode),
        (LinkTimeConstant::IteratorCloseAllNormal, Code::JsIteratorConstructorIteratorCloseAllNormalCode),
        (LinkTimeConstant::IteratorZip, Code::JsIteratorConstructorIteratorZipCode),
        (LinkTimeConstant::PerformIteration, Code::IteratorHelpersPerformIterationCode),
        (LinkTimeConstant::PerformProxyObjectGet, Code::ProxyHelpersPerformProxyObjectGetCode),
        (LinkTimeConstant::PerformProxyObjectGetByVal, Code::ProxyHelpersPerformProxyObjectGetByValCode),
        (LinkTimeConstant::PerformProxyObjectHas, Code::ProxyHelpersPerformProxyObjectHasCode),
        (LinkTimeConstant::PerformProxyObjectHasByVal, Code::ProxyHelpersPerformProxyObjectHasByValCode),
        (LinkTimeConstant::PerformProxyObjectSetByValSloppy, Code::ProxyHelpersPerformProxyObjectSetByValSloppyCode),
        (LinkTimeConstant::PerformProxyObjectSetByValStrict, Code::ProxyHelpersPerformProxyObjectSetByValStrictCode),
        (LinkTimeConstant::PerformProxyObjectSetSloppy, Code::ProxyHelpersPerformProxyObjectSetSloppyCode),
        (LinkTimeConstant::PerformProxyObjectSetStrict, Code::ProxyHelpersPerformProxyObjectSetStrictCode),
        (LinkTimeConstant::RemoveFirstFromList, Code::JsIteratorConstructorRemoveFirstFromListCode),
        (LinkTimeConstant::WrapRemoteValue, Code::ShadowRealmPrototypeWrapRemoteValueCode),
        (LinkTimeConstant::WrappedIterator, Code::IteratorHelpersWrappedIteratorCode),
    ];
    for (constant, index) in builtin_code_constants {
        let function = create_builtin_function(vm, global_object, index);
        global_object.set_link_time_constant(constant, function.as_value());
    }

    // As `JSFunction::create(init.vm, init.owner, length, name, function, visibility, intrinsic)` dos
    // `initLater` cujo corpo nativo existe (JSGlobalObject.cpp:1196, 1922, 1968, 2126, 2135, 2153, 2203, 2206).
    use ImplementationVisibility::{Private, Public};
    let native_functions = [
        NativeLinkTimeFunction {
            constant: LinkTimeConstant::StringSubstring,
            length: 2,
            name: "substring",
            function: string_proto_func_substring,
            visibility: Public,
            intrinsic: Intrinsic::StringPrototypeSubstringIntrinsic,
        },
        NativeLinkTimeFunction {
            constant: LinkTimeConstant::IsArray,
            length: 1,
            name: "isArray",
            function: array_constructor_is_array_host,
            visibility: Public,
            intrinsic: Intrinsic::ArrayIsArrayIntrinsic,
        },
        NativeLinkTimeFunction {
            constant: LinkTimeConstant::ArrayFromFastWithoutMapFn,
            length: 2,
            name: "arrayFromFastWithoutMapFn",
            function: array_constructor_private_from_fast_without_map_fn_host,
            visibility: Private,
            intrinsic: Intrinsic::NoIntrinsic,
        },
        NativeLinkTimeFunction {
            constant: LinkTimeConstant::OwnKeys,
            length: 1,
            name: "ownKeys",
            function: reflect_object_own_keys,
            visibility: Private,
            intrinsic: Intrinsic::ReflectOwnKeysIntrinsic,
        },
        NativeLinkTimeFunction {
            constant: LinkTimeConstant::IsFinite,
            length: 1,
            name: "isFinite",
            function: global_func_is_finite,
            visibility: Private,
            intrinsic: Intrinsic::GlobalIsFiniteIntrinsic,
        },
        NativeLinkTimeFunction {
            constant: LinkTimeConstant::Min,
            length: 0,
            name: "min",
            function: math_proto_func_min,
            visibility: Private,
            intrinsic: Intrinsic::MinIntrinsic,
        },
        NativeLinkTimeFunction {
            constant: LinkTimeConstant::SameValue,
            length: 2,
            name: "is",
            function: object_constructor_is,
            visibility: Private,
            intrinsic: Intrinsic::ObjectIsIntrinsic,
        },
        NativeLinkTimeFunction {
            constant: LinkTimeConstant::ThrowTypeErrorFunction,
            length: 0,
            name: "",
            function: global_func_throw_type_error,
            visibility: Public,
            intrinsic: Intrinsic::NoIntrinsic,
        },
        NativeLinkTimeFunction {
            constant: LinkTimeConstant::SetPrototypeDirect,
            length: 2,
            name: "setPrototypeDirect",
            function: global_func_set_prototype_direct,
            visibility: Private,
            intrinsic: Intrinsic::NoIntrinsic,
        },
        NativeLinkTimeFunction {
            constant: LinkTimeConstant::SetPrototypeDirectOrThrow,
            length: 2,
            name: "setPrototypeDirectOrThrow",
            function: global_func_set_prototype_direct_or_throw,
            visibility: Private,
            intrinsic: Intrinsic::NoIntrinsic,
        },
        NativeLinkTimeFunction {
            constant: LinkTimeConstant::CopyDataProperties,
            length: 2,
            name: "copyDataProperties",
            function: global_func_copy_data_properties,
            visibility: Private,
            intrinsic: Intrinsic::NoIntrinsic,
        },
        NativeLinkTimeFunction {
            constant: LinkTimeConstant::CloneObject,
            length: 0,
            name: "cloneObject",
            function: global_func_clone_object,
            visibility: Private,
            intrinsic: Intrinsic::NoIntrinsic,
        },
        NativeLinkTimeFunction {
            constant: LinkTimeConstant::ToIntegerOrInfinity,
            length: 1,
            name: "toIntegerOrInfinity",
            function: global_func_to_integer_or_infinity,
            visibility: Private,
            intrinsic: Intrinsic::ToIntegerOrInfinityIntrinsic,
        },
        NativeLinkTimeFunction {
            constant: LinkTimeConstant::ToLength,
            length: 1,
            name: "toLength",
            function: global_func_to_length,
            visibility: Private,
            intrinsic: Intrinsic::ToLengthIntrinsic,
        },
        NativeLinkTimeFunction {
            constant: LinkTimeConstant::MakeTypeError,
            length: 0,
            name: "makeTypeError",
            function: global_func_make_type_error,
            visibility: Private,
            intrinsic: Intrinsic::NoIntrinsic,
        },
        NativeLinkTimeFunction {
            constant: LinkTimeConstant::InstanceOf,
            length: 0,
            name: "instanceOf",
            function: object_private_func_instance_of,
            visibility: Private,
            intrinsic: Intrinsic::NoIntrinsic,
        },
        NativeLinkTimeFunction {
            constant: LinkTimeConstant::HandleNegativeProxyHasTrapResult,
            length: 2,
            name: "handleNegativeProxyHasTrapResult",
            function: global_func_handle_negative_proxy_has_trap_result,
            visibility: Private,
            intrinsic: Intrinsic::NoIntrinsic,
        },
        NativeLinkTimeFunction {
            constant: LinkTimeConstant::HandleProxyGetTrapResult,
            length: 3,
            name: "handleProxyGetTrapResult",
            function: global_func_handle_proxy_get_trap_result,
            visibility: Private,
            intrinsic: Intrinsic::NoIntrinsic,
        },
        NativeLinkTimeFunction {
            constant: LinkTimeConstant::HandlePositiveProxySetTrapResult,
            length: 3,
            name: "handlePositiveProxySetTrapResult",
            function: global_func_handle_positive_proxy_set_trap_result,
            visibility: Private,
            intrinsic: Intrinsic::NoIntrinsic,
        },
        NativeLinkTimeFunction {
            constant: LinkTimeConstant::CreatePrivateSymbol,
            length: 1,
            name: "createPrivateSymbol",
            function: create_private_symbol,
            visibility: Private,
            intrinsic: Intrinsic::NoIntrinsic,
        },
        NativeLinkTimeFunction {
            constant: LinkTimeConstant::JsonParse,
            length: 1,
            name: "parse",
            function: json_proto_func_parse,
            visibility: Private,
            intrinsic: Intrinsic::NoIntrinsic,
        },
        NativeLinkTimeFunction {
            constant: LinkTimeConstant::JsonStringify,
            length: 2,
            name: "stringify",
            function: json_proto_func_stringify,
            visibility: Private,
            intrinsic: Intrinsic::NoIntrinsic,
        },
    ];
    // `mapIterationNext`... e `setIterationNext`... (JSGlobalObject.cpp:1979-2005): todas com `length` 0 e
    // visibilidade privada; o `Storage` é a célula leve de `ordered_hash_table_storage.rs`.
    let collection_storage_functions: [(LinkTimeConstant, &str, NativeFunction, Intrinsic); 9] = [
        (LinkTimeConstant::MapIterationNext, "mapIterationNext", map_private_func_map_iteration_next, Intrinsic::JSMapIterationNextIntrinsic),
        (LinkTimeConstant::MapIterationEntry, "mapIterationEntry", map_private_func_map_iteration_entry, Intrinsic::JSMapIterationEntryIntrinsic),
        (LinkTimeConstant::MapStorage, "mapStorage", map_private_func_map_storage, Intrinsic::JSMapStorageIntrinsic),
        (LinkTimeConstant::MapIterationEntryKey, "mapIterationEntryKey", map_private_func_map_iteration_entry_key, Intrinsic::JSMapIterationEntryKeyIntrinsic),
        (LinkTimeConstant::MapIterationEntryValue, "mapIterationEntryValue", map_private_func_map_iteration_entry_value, Intrinsic::JSMapIterationEntryValueIntrinsic),
        (LinkTimeConstant::SetIterationNext, "setIterationNext", set_private_func_set_iteration_next, Intrinsic::JSSetIterationNextIntrinsic),
        (LinkTimeConstant::SetIterationEntry, "setIterationEntry", set_private_func_set_iteration_entry, Intrinsic::JSSetIterationEntryIntrinsic),
        (LinkTimeConstant::SetIterationEntryKey, "setIterationEntryKey", set_private_func_set_iteration_entry_key, Intrinsic::JSSetIterationEntryKeyIntrinsic),
        (LinkTimeConstant::SetStorage, "setStorage", set_private_func_set_storage, Intrinsic::JSSetStorageIntrinsic),
    ];
    let native_functions = native_functions.into_iter().chain(collection_storage_functions.map(
        |(constant, name, function, intrinsic)| NativeLinkTimeFunction {
            constant,
            length: 0,
            name,
            function,
            visibility: Private,
            intrinsic,
        },
    ));

    for entry in native_functions {
        let function = JSFunction::create_native(
            vm,
            global_object,
            entry.length,
            &WtfString::from_latin1(entry.name.as_bytes()),
            entry.function,
            entry.visibility,
            entry.intrinsic,
            call_host_function_as_constructor,
        );
        global_object.set_link_time_constant(entry.constant, function.as_value());
    }

    crate::runtime::js_module_loader::install_import_module_constant(global_object);

    // `m_linkTimeConstants[Object / RegExp / String / AggregateError / ReferenceError / SuppressedError]`
    // (os construtores, ver as DIVERGÊNCIAS do cabeçalho). `Array` espera o `ArrayConstructor` de `init`.
    let global_constructors = [
        (LinkTimeConstant::Object, b"Object".as_slice()),
        (LinkTimeConstant::String, b"String".as_slice()),
        (LinkTimeConstant::AggregateError, b"AggregateError".as_slice()),
        (LinkTimeConstant::ReferenceError, b"ReferenceError".as_slice()),
        (LinkTimeConstant::SuppressedError, b"SuppressedError".as_slice()),
    ];
    for (constant, property) in global_constructors {
        let constructor = global_object.get_direct_by_name(vm, &by_span(property));
        assert!(!constructor.is_empty(), "o global não tem o construtor {}", String::from_utf8_lossy(property));
        global_object.set_link_time_constant(constant, constructor);
    }
    let array_constructor = global_object.get_direct_by_name(vm, &by_span(b"Array"));
    if !array_constructor.is_empty() {
        global_object.set_link_time_constant(LinkTimeConstant::Array, array_constructor);
    }
    global_object.set_link_time_constant(LinkTimeConstant::RegExp, global_object.reg_exp_constructor());

    // `getGetterById(this, m_regExpPrototype.get(), vm.propertyNames->flags)` e os outros nove acessores.
    let reg_exp_prototype =
        global_object.reg_exp_prototype.borrow().clone().expect("JSGlobalObject sem regExpPrototype");
    let reg_exp_getters = [
        (LinkTimeConstant::RegExpProtoFlagsGetter, "flags"),
        (LinkTimeConstant::RegExpProtoHasIndicesGetter, "hasIndices"),
        (LinkTimeConstant::RegExpProtoGlobalGetter, "global"),
        (LinkTimeConstant::RegExpProtoIgnoreCaseGetter, "ignoreCase"),
        (LinkTimeConstant::RegExpProtoMultilineGetter, "multiline"),
        (LinkTimeConstant::RegExpProtoSourceGetter, "source"),
        (LinkTimeConstant::RegExpProtoStickyGetter, "sticky"),
        (LinkTimeConstant::RegExpProtoUnicodeGetter, "unicode"),
        (LinkTimeConstant::RegExpProtoDotAllGetter, "dotAll"),
        (LinkTimeConstant::RegExpProtoUnicodeSetsGetter, "unicodeSets"),
    ];
    for (constant, property) in reg_exp_getters {
        let value = reg_exp_prototype.get_direct_by_name(vm, &by_span(property.as_bytes()));
        let getter_setter = GetterSetter::from_value(&value).expect("RegExp.prototype sem o acessor esperado");
        global_object.set_link_time_constant(constant, getter_setter.as_value());
    }

    // `m_regExpPrototype->getDirect(vm, exec / @@match / @@matchAll / @@replace)`.
    let reg_exp_prototype_methods = [
        (LinkTimeConstant::RegExpBuiltinExec, &vm.property_names.exec),
        (LinkTimeConstant::RegExpPrototypeSymbolMatch, &vm.property_names.match_symbol),
        (LinkTimeConstant::RegExpPrototypeSymbolMatchAll, &vm.property_names.match_all_symbol),
        (LinkTimeConstant::RegExpPrototypeSymbolReplace, &vm.property_names.replace_symbol),
    ];
    for (constant, identifier) in reg_exp_prototype_methods {
        let value = reg_exp_prototype.get_direct_by_name(vm, &name(identifier));
        assert!(!value.is_empty(), "RegExp.prototype sem o método de {:?}", constant);
        global_object.set_link_time_constant(constant, value);
    }

    // `objectPrototype()->get(this, vm.propertyNames->hasOwnProperty)` (RELEASE_ASSERT de `JSFunction`).
    let has_own_property = global_object.object_prototype().get_direct_by_name(vm, &name(&vm.property_names.has_own_property));
    assert!(has_own_property.is_callable(), "Object.prototype.hasOwnProperty não é uma JSFunction");
    global_object.set_link_time_constant(LinkTimeConstant::HasOwnPropertyFunction, has_own_property);

    // `callFunction` e `applyFunction` que `m_functionPrototype->addFunctionProperties` devolve: os
    // builtins `call` e `apply` gravados no `Function.prototype`.
    let function_prototype = global_object.function_prototype();
    let builtin_names = vm.property_names.builtin_names();
    for (constant, identifier) in [
        (LinkTimeConstant::CallFunction, builtin_names.call_public_name()),
        (LinkTimeConstant::ApplyFunction, builtin_names.apply_public_name()),
    ] {
        let value = function_prototype.get_direct_by_name(vm, &name(identifier));
        assert!(!value.is_empty(), "Function.prototype sem o builtin de {:?}", constant);
        global_object.set_link_time_constant(constant, value);
    }
}
