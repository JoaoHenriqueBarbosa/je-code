//! As cascas finas das funções nativas de `RegExpPrototype.cpp` (`regExpProtoFuncExec`, `...Test`, `...ToString`,
//! `...Compile`, os acessores `regExpProtoGetter*`), a tabela de `RegExpPrototype::finishCreation` e o
//! `RegExp` constructor de `RegExpConstructor.cpp` (`callRegExpConstructor`, `constructWithRegExpConstructor`,
//! `constructRegExp`, `regExpCreate`). Os algoritmos puros estão em `reg_exp_prototype.rs`.
//!
//! LACUNAS, e por quê:
//! - Os acessores `global`..`flags` são instalados por `add_reg_exp_prototype_properties` (um
//!   `JSFunction` `get X` num `GetterSetter`, `put_native_getter`), entre `toString` e os `Symbol.*`, como
//!   no C++.
//! - `RegExp.escape` e os acessores legados `$1`..`$9`, `input`, `lastMatch` etc. estão em
//!   `reg_exp_legacy_natives.rs` (`escape` antes do `@@species`, os `CustomAccessor` depois).
//! - `Realm` diferente de `this` em `compile` não é checado (um só realm).
//! - `flags` e `toString` leem `source`/`flags` e as oito flags do objeto com `get` (o `flagsString`
//!   genérico), inclusive para `RegExpObject`: sem o `regExpFlagsWatchpointIsValid` o atalho do C++ não
//!   dá para saber se vale.
//! - Toda conversão (`toString` do padrão e das flags, `lastIndex`) confere a exceção pendente antes da
//!   próxima, como o `RETURN_IF_EXCEPTION` do C++ (o `toString` do padrão que lança impede o das flags).
//!
//! DIVERGÊNCIAS:
//! - `thisValue == globalObject->regExpPrototype()` usa o campo `reg_exp_prototype` do global.
//! - O erro de construção do `RegExp` (`errorToThrow`) é `SyntaxError` com a mensagem de `Yarr`, ou
//!   `OutOfMemoryError` para `too many nested disjunctions` (`error_to_throw_type`).
//! - `[Symbol.match]`, `[Symbol.matchAll]`, `[Symbol.replace]`, `[Symbol.search]`, `[Symbol.split]` e `test`
//!   só têm o caminho genérico da especificação (`regExpMatchSlow`, `regExpSearchGeneric`,
//!   `regExpSplitSlow`, `regExpReplaceGeneric`, o `matchAll` com `SpeciesConstructor`): os atalhos
//!   `regExpMatchFast`, `regExpSearchFast`, `regExpSplitFast` e o do `matchAll` dependem dos
//!   watchpoints `isSymbol*FastAndNonObservable`, que não existem, e têm o mesmo resultado observável.
//!   `regExpExec` reconhece o `exec` primordial pela `NativeFunction` (`reg_exp_proto_func_exec`), no
//!   lugar de `globalObject->regExpProtoExecFunction()`.

use crate::host_function;
use crate::interpreter::call_frame::NativeCallFrame;
use crate::runtime::call_data::{get_call_data, CallData};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::put_species_accessor;
use crate::runtime::error::{create_syntax_error, create_type_error};
use crate::runtime::exception_helpers::throw_out_of_memory_error;
use crate::runtime::fallible_alloc::try_wtf_vector_with_capacity;
use crate::runtime::host_call::{throw_thrown, HostCall, HostResult, Thrown};
use crate::runtime::host_function_support::{throw_error_object, throw_put_error, throw_vm_type_error, ObjectRef};
use crate::runtime::array_buffer_prototype::put_native_getter;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::internal_function::{
    get_derived_structure_in_realm, InternalFunction, InternalFunctionRef, PropertyAdditionMode, INTERNAL_FUNCTION_S_INFO,
};
use crate::runtime::iterator_operations::call_checked;
use crate::runtime::js_array::construct_array;
use crate::runtime::js_function::{call_host_function_as_constructor, put_direct_native_function_without_transition, JSFunction};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSObject, PutError};
use crate::runtime::js_reg_exp_string_iterator::JSRegExpStringIterator;
use crate::runtime::js_string::{js_string, js_substring, JSStringRef};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::js_value::{js_boolean, js_number, EncodedJSValue, JSValue};
use crate::runtime::native_function::{to_tagged, NativeFunction};
use crate::runtime::operations::same_value;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::reg_exp::RegExp;
use crate::runtime::reg_exp_object::RegExpObject;
use crate::runtime::reg_exp_prototype as algo;
use crate::runtime::string_prototype::{code_units, string_from_units};
use crate::runtime::string_prototype_natives::is_reg_exp;
use crate::runtime::string_regexp_support::{
    advance_string_index, construct_value, contains_unit, find_unit, get_object_index, get_object_property,
    reg_exp_species_constructor, set_object_property, string_value,
    to_string_value, to_uint32_value, to_wtf_string_value, units_value,
};
use crate::runtime::property_offset::INVALID_OFFSET;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::throw_scope::ThrowScope;
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;
use crate::wtf::text::string_view::StringView;
use crate::yarr::yarr_flags::{parse_flags, FlagSet, Flags};

/// `const ClassInfo RegExpConstructor::s_info` (`"Function"`, base `InternalFunction`).
pub static REG_EXP_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo {
        class_name: "Function",
        parent_class: Some(&INTERNAL_FUNCTION_S_INFO),
        static_prop_hash_table: Some(&crate::runtime::reg_exp_legacy_natives::REG_EXP_CONSTRUCTOR_TABLE),
        inherits_js_type_range: None,
    };

/// O resultado de um corpo `Result<JSValue, PutError>` como valor ou exceção pendente.
fn encode_put_result(global_object: &JSGlobalObject, result: Result<JSValue, PutError>) -> EncodedJSValue {
    match result {
        Ok(value) => value.encode(),
        Err(error) => {
            throw_put_error(global_object, error);
            JSValue::empty().encode()
        }
    }
}

fn throw_type(global_object: &JSGlobalObject, message: &str) -> EncodedJSValue {
    throw_error_object(global_object, create_type_error(global_object, &WtfString::from_utf8(message.as_bytes())))
}

/// `regExpProtoFuncExec`: `toStringOrNull`, que com exceção pendente devolve `undefined` sem rodar o `exec`.
fn reg_exp_proto_func_exec(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let Some(reg_exp) = as_reg_exp_object(&call_frame.this_value()) else {
        return throw_type(global_object, algo::BUILTIN_EXEC_NOT_REG_EXP);
    };
    let Ok(string) = to_string_value(global_object, call_frame.argument(0)) else {
        return JSValue::undefined().encode();
    };
    encode_put_result(global_object, reg_exp.exec(global_object, &string))
}

/// `regExpExecWatchpointIsValid(vm, thisObject)` (RegExpPrototypeInlines.h): o protótipo de `this` é o
/// `RegExp.prototype` do realm, as propriedades primordiais vigiadas ainda são as originais
/// (`regExpPrimordialPropertiesWatchpointSet().state() == IsWatched`) e `this` não tem `exec` próprio.
fn reg_exp_exec_watchpoint_is_valid(global_object: &JSGlobalObject, this_object: JSValue) -> bool {
    let Some(object) = ObjectRef::from_value(&this_object) else { return false };
    let Some(reg_exp_prototype) = global_object.reg_exp_prototype.borrow().clone() else { return false };
    if reg_exp_prototype.as_value() != object.get_prototype_direct() {
        return false;
    }
    if global_object.reg_exp_primordial_properties_fired.get() {
        return false;
    }
    let vm = global_object.vm();
    object.get_direct_offset(vm, &PropertyName::from_identifier(&vm.property_names.exec)) == INVALID_OFFSET
}

/// `regExpProtoFuncTest`: com o `exec` primordial vigiado, `this` que não é `RegExpObject` lança antes de
/// qualquer leitura; no resto o `regExpExec` genérico (que usa o `exec` do programa se ele o redefiniu),
/// de resultado igual ao atalho `regExp->test`.
fn reg_exp_test(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_value = call.this_value();
    if !this_value.is_object() {
        return Err(Thrown::type_error("RegExp.prototype.test requires that |this| be an Object"));
    }
    let string = to_string_value(global_object, call.argument(0))?;
    if reg_exp_exec_watchpoint_is_valid(global_object, this_value) && as_reg_exp_object(&this_value).is_none() {
        return Err(Thrown::type_error(algo::BUILTIN_EXEC_NOT_REG_EXP));
    }
    let matched = reg_exp_exec(global_object, this_value, &string)?;
    Ok(js_boolean(!matched.is_null()))
}

host_function!(reg_exp_proto_func_test, reg_exp_test);

/// `regExpProtoFuncToString`.
fn reg_exp_proto_func_to_string_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    algo::reg_exp_proto_func_to_string(global_object, call.this_value())
}

host_function!(reg_exp_proto_func_to_string, reg_exp_proto_func_to_string_body);

/// `RegExpObject` por trás de um valor (`dynamicDowncast<RegExpObject>`).
fn as_reg_exp_object(value: &JSValue) -> Option<std::rc::Rc<RegExpObject>> {
    if value.is_cell() { RegExpObject::from_cell_id(value.as_cell()) } else { None }
}

/// O `RegExp` que o erro de construção recusa: `SyntaxError` com a mensagem do Yarr, ou
/// `OutOfMemoryError` (`errorToThrow`).
fn throw_construction_error(global_object: &JSGlobalObject, reg_exp: &RegExp) -> EncodedJSValue {
    let message = reg_exp.error_message();
    if message.ends_with(b"too many nested disjunctions") {
        let mut scope = ThrowScope::new(global_object.vm());
        throw_out_of_memory_error(global_object, &mut scope);
        return JSValue::empty().encode();
    }
    throw_error_object(global_object, create_syntax_error(global_object, &WtfString::from_utf8(message)))
}

const INVALID_FLAGS: &str = "Invalid flags supplied to RegExp constructor.";

/// `ToString(value)` com a exceção pendente (um `toString` que lança, um `Symbol`) como `Err`: o C++ faz
/// `toWTFString` seguido de `RETURN_IF_EXCEPTION`, e o que vem depois (as flags) não pode rodar.
fn to_text(global_object: &JSGlobalObject, value: &JSValue) -> Result<WtfString, EncodedJSValue> {
    to_wtf_string_value(global_object, *value).map_err(|_| JSValue::empty().encode())
}

/// `patternArg.isUndefined() ? emptyString() : patternArg.toWTFString(globalObject)`.
fn to_pattern(global_object: &JSGlobalObject, pattern: &JSValue) -> Result<WtfString, EncodedJSValue> {
    if pattern.is_undefined() { Ok(WtfString::default()) } else { to_text(global_object, pattern) }
}

/// `Yarr::parseFlags(flags.toWTFString())` com `undefined` como o conjunto vazio: `Err` é a exceção pendente.
fn to_flags(global_object: &JSGlobalObject, flags: &JSValue) -> Result<FlagSet, EncodedJSValue> {
    if flags.is_undefined() {
        return Ok(FlagSet::empty());
    }
    let string = to_text(global_object, flags)?;
    match parse_flags(&*code_units(&string)) {
        Some(parsed) => Ok(parsed),
        None => Err(throw_error_object(global_object, create_syntax_error(global_object, &WtfString::from_utf8(INVALID_FLAGS.as_bytes())))),
    }
}

/// `regExpProtoFuncCompile`.
fn reg_exp_proto_func_compile(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let vm = global_object.vm();
    let Some(this_reg_exp) = as_reg_exp_object(&call_frame.this_value()) else {
        return throw_vm_type_error(global_object, None);
    };
    if !this_reg_exp.are_legacy_features_enabled() {
        return throw_type(global_object, "|this| RegExp object's legacy features are not enabled");
    }

    let arg0 = call_frame.argument(0);
    let arg1 = call_frame.argument(1);
    let reg_exp = if let Some(other) = as_reg_exp_object(&arg0) {
        if !arg1.is_undefined() {
            return throw_type(global_object, "Cannot supply flags when constructing one RegExp from another.");
        }
        other.reg_exp()
    } else {
        let pattern = match to_pattern(global_object, &arg0) {
            Ok(pattern) => pattern,
            Err(thrown) => return thrown,
        };
        let flags = match to_flags(global_object, &arg1) {
            Ok(flags) => flags,
            Err(thrown) => return thrown,
        };
        RegExp::create(vm, &pattern, flags)
    };
    if !reg_exp.is_valid() {
        return throw_construction_error(global_object, &reg_exp);
    }

    this_reg_exp.set_reg_exp(reg_exp);
    match this_reg_exp.set_last_index(js_number(0.0), true) {
        Ok(_) => this_reg_exp.as_value().encode(),
        Err(error) => encode_put_result(global_object, Err(error)),
    }
}

/// `thisValue == globalObject->regExpPrototype()`.
fn is_reg_exp_prototype(global_object: &JSGlobalObject, value: &JSValue) -> bool {
    global_object.reg_exp_prototype.borrow().as_ref().is_some_and(|prototype| *value == prototype.as_value())
}

/// `regExpProtoGetterGlobal` e os outros sete acessores de flag.
fn flag_getter(global_object: &JSGlobalObject, call_frame: &NativeCallFrame<'_>, name: &str) -> EncodedJSValue {
    let this_value = call_frame.this_value();
    if as_reg_exp_object(&this_value).is_none() && is_reg_exp_prototype(global_object, &this_value) {
        return JSValue::undefined().encode();
    }
    encode_put_result(global_object, algo::reg_exp_proto_getter_flag(name, this_value))
}

macro_rules! flag_getter_function {
    ($wrapper:ident, $name:literal) => {
        fn $wrapper(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
            flag_getter(global_object, call_frame, $name)
        }
    };
}

flag_getter_function!(reg_exp_proto_getter_global, "global");
flag_getter_function!(reg_exp_proto_getter_has_indices, "hasIndices");
flag_getter_function!(reg_exp_proto_getter_ignore_case, "ignoreCase");
flag_getter_function!(reg_exp_proto_getter_multiline, "multiline");
flag_getter_function!(reg_exp_proto_getter_dot_all, "dotAll");
flag_getter_function!(reg_exp_proto_getter_sticky, "sticky");
flag_getter_function!(reg_exp_proto_getter_unicode, "unicode");
flag_getter_function!(reg_exp_proto_getter_unicode_sets, "unicodeSets");

/// `regExpProtoGetterSource`: no `RegExp.prototype`, `"(?:)"`.
fn reg_exp_proto_getter_source(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let this_value = call_frame.this_value();
    if as_reg_exp_object(&this_value).is_none() && is_reg_exp_prototype(global_object, &this_value) {
        return JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_latin1(b"(?:)"))).encode();
    }
    encode_put_result(global_object, algo::reg_exp_proto_getter_source(global_object, this_value))
}

/// `regExpProtoGetterFlags`.
fn reg_exp_proto_getter_flags_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    algo::reg_exp_proto_getter_flags(global_object, call.this_value())
}

host_function!(reg_exp_proto_getter_flags, reg_exp_proto_getter_flags_body);

/// Os acessores de `finishCreation` (nome, função nativa do getter, intrínseco), na ordem do C++.
pub static REG_EXP_PROTOTYPE_GETTERS: [(&str, NativeFunction, Intrinsic); 10] = [
    ("global", reg_exp_proto_getter_global, Intrinsic::RegExpGlobalIntrinsic),
    ("dotAll", reg_exp_proto_getter_dot_all, Intrinsic::RegExpDotAllIntrinsic),
    ("hasIndices", reg_exp_proto_getter_has_indices, Intrinsic::RegExpHasIndicesIntrinsic),
    ("ignoreCase", reg_exp_proto_getter_ignore_case, Intrinsic::RegExpIgnoreCaseIntrinsic),
    ("multiline", reg_exp_proto_getter_multiline, Intrinsic::RegExpMultilineIntrinsic),
    ("sticky", reg_exp_proto_getter_sticky, Intrinsic::RegExpStickyIntrinsic),
    ("unicode", reg_exp_proto_getter_unicode, Intrinsic::RegExpUnicodeIntrinsic),
    ("unicodeSets", reg_exp_proto_getter_unicode_sets, Intrinsic::RegExpUnicodeSetsIntrinsic),
    ("source", reg_exp_proto_getter_source, Intrinsic::NoIntrinsic),
    ("flags", reg_exp_proto_getter_flags, Intrinsic::NoIntrinsic),
];

/// A parte de funções de `RegExpPrototype::finishCreation` (`compile`, `exec`, `toString`, os cinco
/// `Symbol.*` e `test`), na ordem do C++; `Base::finishCreation` já foi feito por `RegExpPrototype::create`.
pub fn add_reg_exp_prototype_properties(prototype: &JSObject, vm: &VM, global_object: &JSGlobalObject) {
    let names = &vm.property_names;
    let define = |name: &Identifier, length: u32, function: NativeFunction, intrinsic: Intrinsic| {
        put_direct_native_function_without_transition(
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
    };
    // `JSFunction::create(vm, globalObject, length, "[Symbol.x]"_s, function, ...)` e `putDirectWithoutTransition`.
    let define_symbol = |symbol: &Identifier, name: &[u8], length: u32, function: NativeFunction, intrinsic: Intrinsic| {
        let created = JSFunction::create_native(
            vm,
            global_object,
            length,
            &WtfString::from_latin1(name),
            function,
            ImplementationVisibility::Public,
            intrinsic,
            call_host_function_as_constructor,
        );
        prototype.put_direct_without_transition(vm, &PropertyName::from_identifier(symbol), created.as_value(), DONT_ENUM);
    };
    define(&names.compile, 2, reg_exp_proto_func_compile, Intrinsic::NoIntrinsic);
    define(&names.exec, 1, reg_exp_proto_func_exec, Intrinsic::RegExpExecIntrinsic);
    define(&names.to_string, 0, reg_exp_proto_func_to_string, Intrinsic::NoIntrinsic);
    // `JSC_NATIVE_(INTRINSIC_)GETTER_WITHOUT_TRANSITION(name, getter, DontEnum | Accessor, intrinsic)`.
    for &(name, getter, intrinsic) in REG_EXP_PROTOTYPE_GETTERS.iter() {
        put_native_getter(vm, global_object, prototype, name, getter, intrinsic, DONT_ENUM);
    }
    define_symbol(&names.match_symbol, b"[Symbol.match]", 1, reg_exp_proto_func_match, Intrinsic::RegExpMatchIntrinsic);
    define_symbol(&names.match_all_symbol, b"[Symbol.matchAll]", 1, reg_exp_proto_func_match_all, Intrinsic::NoIntrinsic);
    define_symbol(&names.replace_symbol, b"[Symbol.replace]", 2, reg_exp_proto_func_replace, Intrinsic::NoIntrinsic);
    define_symbol(&names.search_symbol, b"[Symbol.search]", 1, reg_exp_proto_func_search, Intrinsic::RegExpSearchIntrinsic);
    define_symbol(&names.split_symbol, b"[Symbol.split]", 2, reg_exp_proto_func_split, Intrinsic::RegExpSplitIntrinsic);
    define(&names.test, 1, reg_exp_proto_func_test, Intrinsic::RegExpTestIntrinsic);
}

// ------------------------------ regExpExec e os Symbol.* --------------------------

/// `regExpExec != regExpBuiltinExec` (`globalObject->regExpProtoExecFunction()`): a função nativa é a
/// `regExpProtoFuncExec`.
fn is_builtin_exec(value: JSValue) -> bool {
    matches!(get_call_data(value), CallData::Native { function, .. } if function == to_tagged(reg_exp_proto_func_exec))
}

/// `regExpExec(globalObject, thisValue, str)` (https://tc39.es/ecma262/#sec-regexpexec): `thisValue` é
/// objeto. `Err` carrega a exceção pendente ou o `TypeError`.
pub fn reg_exp_exec(global_object: &JSGlobalObject, this_value: JSValue, string: &JSStringRef) -> Result<JSValue, Thrown> {
    let exec = get_object_property(global_object, this_value, &global_object.vm().property_names.exec)?;
    if !is_builtin_exec(exec) && exec.is_callable() {
        let matched = call_checked(global_object, exec, this_value, &[string_value(string)], "Type error")?;
        if !matched.is_null() && !matched.is_object() {
            return Err(Thrown::type_error("The result of RegExp exec must be null or an object"));
        }
        return Ok(matched);
    }
    let Some(reg_exp) = as_reg_exp_object(&this_value) else {
        return Err(Thrown::type_error(algo::BUILTIN_EXEC_NOT_REG_EXP));
    };
    Ok(reg_exp.exec(global_object, string)?)
}

/// O `thisValue` que tem de ser objeto, ou o `TypeError` com a mensagem de cada `Symbol.*`.
fn require_object(this_value: JSValue, message: &str) -> Result<JSValue, Thrown> {
    if this_value.is_object() { Ok(this_value) } else { Err(Thrown::type_error(message)) }
}

/// `constructEmptyArray` seguido de `putDirectIndex` dos valores.
fn array_of(global_object: &JSGlobalObject, values: &[JSValue]) -> JSValue {
    construct_array(global_object.vm(), &global_object.array_structure(), values).as_value()
}

/// `regExpMatchSlow(globalObject, thisObject, string)` (https://tc39.es/ecma262/#sec-regexp.prototype-%symbol.match%).
fn reg_exp_match_slow(global_object: &JSGlobalObject, this_object: JSValue, string: &JSStringRef) -> HostResult {
    let vm = global_object.vm();
    // 4. Let flags be ? ToString(? Get(regexp, "flags")).
    let flags_value = get_object_property(global_object, this_object, &vm.property_names.flags)?;
    let flags = to_wtf_string_value(global_object, flags_value)?;

    // 5. If flags does not contain "g", return ? RegExpExec(regexp, string).
    if !contains_unit(&flags, b'g') {
        return reg_exp_exec(global_object, this_object, string);
    }

    // 6. If flags contains "u" or flags contains "v", let fullUnicode be true.
    let full_unicode = contains_unit(&flags, b'u') || contains_unit(&flags, b'v');

    // 7. Perform ? Set(regexp, "lastIndex", +0, true).
    set_object_property(global_object, this_object, &vm.property_names.last_index, js_number(0.0))?;

    let units = code_units(&string.value()).into_owned();
    let mut matches: Vec<JSValue> = Vec::new();

    // 10. Repeat,
    loop {
        // 10.a. Let result be ? RegExpExec(regexp, string).
        let result = reg_exp_exec(global_object, this_object, string)?;

        // 10.b. If result is null, then return null (sem casamentos) ou o array.
        if result.is_null() {
            if matches.is_empty() {
                return Ok(JSValue::Null);
            }
            return Ok(array_of(global_object, &matches));
        }

        // 10.c. Let matchString be ? ToString(? Get(result, "0")).
        let match_value = get_object_index(global_object, result, 0)?;
        let match_string = to_string_value(global_object, match_value)?;
        matches.push(string_value(&match_string));

        // 10.e. If matchString is the empty String, then
        if match_string.length() == 0 {
            // 10.e.i. Let thisIndex be ℝ(? ToLength(? Get(regexp, "lastIndex"))).
            let last_index_value = get_object_property(global_object, this_object, &vm.property_names.last_index)?;
            let this_index = last_index_value.to_length_checked()?;
            // 10.e.ii. Let nextIndex be AdvanceStringIndex(string, thisIndex, fullUnicode).
            let next_index = advance_string_index(&units, this_index, full_unicode);
            // 10.e.iii. Perform ? Set(regexp, "lastIndex", 𝔽(nextIndex), true).
            set_object_property(global_object, this_object, &vm.property_names.last_index, js_number(next_index as f64))?;
        }
    }
}

/// `regExpProtoFuncMatch`.
fn reg_exp_match(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_object = require_object(call.this_value(), "RegExp.prototype.@@match requires that |this| be an Object")?;
    let string = to_string_value(global_object, call.argument(0))?;
    reg_exp_match_slow(global_object, this_object, &string)
}

/// `regExpSearchGeneric(globalObject, thisObject, str)` sem o atalho de `regExpSearchFast`.
fn reg_exp_search_generic(global_object: &JSGlobalObject, this_object: JSValue, string: &JSStringRef) -> HostResult {
    let vm = global_object.vm();
    let last_index = &vm.property_names.last_index;

    if reg_exp_exec_watchpoint_is_valid(global_object, this_object) && as_reg_exp_object(&this_object).is_none() {
        return Err(Thrown::type_error(algo::BUILTIN_EXEC_NOT_REG_EXP));
    }

    let previous_last_index = get_object_property(global_object, this_object, last_index)?;
    if !same_value(previous_last_index, js_number(0.0)) {
        set_object_property(global_object, this_object, last_index, js_number(0.0))?;
    }

    let matched = reg_exp_exec(global_object, this_object, string)?;

    let current_last_index = get_object_property(global_object, this_object, last_index)?;
    if !same_value(current_last_index, previous_last_index) {
        set_object_property(global_object, this_object, last_index, previous_last_index)?;
    }

    if matched.is_null() {
        return Ok(js_number(-1.0));
    }
    get_object_property(global_object, matched, &vm.property_names.index)
}

/// `regExpProtoFuncSearch`.
fn reg_exp_search(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_object = require_object(call.this_value(), "RegExp.prototype.@@search requires that |this| be an Object")?;
    let string = to_string_value(global_object, call.argument(0))?;
    reg_exp_search_generic(global_object, this_object, &string)
}

/// A parte de `hasObservableSideEffectsForRegExpSplit` que o porte consegue ver: `this` é um `RegExpObject` não
/// sticky de protótipo e `exec` primordiais (`reg_exp_exec_watchpoint_is_valid`) e sem propriedade própria que
/// troque `constructor` ou algum getter de flag. O construtor/`Symbol.species` o chamador já conferiu.
fn reg_exp_split_fast_candidate(global_object: &JSGlobalObject, this_object: JSValue) -> Option<std::rc::Rc<RegExpObject>> {
    let reg_exp_object = as_reg_exp_object(&this_object)?;
    if !reg_exp_exec_watchpoint_is_valid(global_object, this_object) || reg_exp_object.reg_exp().sticky() {
        return None;
    }
    let vm = global_object.vm();
    let names = &vm.property_names;
    let object = ObjectRef::from_value(&this_object)?;
    let shadowed = [
        &names.constructor,
        &names.flags,
        &names.dot_all,
        &names.global,
        &names.has_indices,
        &names.ignore_case,
        &names.multiline,
        &names.sticky,
        &names.unicode,
        &names.unicode_sets,
    ]
    .into_iter()
    .any(|name| object.get_direct_offset(vm, &PropertyName::from_identifier(name)) != INVALID_OFFSET);
    (!shadowed).then_some(reg_exp_object)
}

/// `regExpSplitFast(globalObject, regexp, inputString, limit)`: a busca é do `RegExp` original, sem sticky, a partir
/// de `matchPosition`; cada casamento passa por `performMatch` e fica nos estáticos legados (`RegExp.lastMatch`...),
/// inclusive o que começa em `size` ou depois e encerra o laço. O resultado é o do `regExpSplitSlow`.
fn reg_exp_split_fast(
    global_object: &JSGlobalObject,
    reg_exp_object: &RegExpObject,
    string: &JSStringRef,
    limit_value: JSValue,
) -> HostResult {
    let vm = global_object.vm();
    let lim = if limit_value.is_undefined() { u32::MAX } else { to_uint32_value(global_object, limit_value)? };
    let mut result: Vec<JSValue> = Vec::new();
    if lim == 0 {
        return Ok(array_of(global_object, &result));
    }

    let reg_exp = reg_exp_object.reg_exp();
    let global_data = global_object.reg_exp_global_data();
    let value = string.value();
    let units = code_units(&value).into_owned();
    let size = units.len();
    if size == 0 {
        if global_data.perform_match(global_object, &reg_exp, string, 0).matched() {
            return Ok(array_of(global_object, &result));
        }
        result.push(string_value(string));
        return Ok(array_of(global_object, &result));
    }

    let unicode_matching = reg_exp.flags().contains(Flags::Unicode) || reg_exp.flags().contains(Flags::UnicodeSets);
    let number_of_captures = reg_exp.num_subpatterns() as usize;
    let mut last_match_end: usize = 0;
    let mut match_position: usize = 0;
    while match_position < size {
        let matched = global_data.perform_match(global_object, &reg_exp, string, match_position as u32);
        if !matched.matched() || matched.start >= size {
            break;
        }
        let match_end = matched.end.min(size);
        if match_end == last_match_end {
            match_position = advance_string_index(&units, matched.start as u64, unicode_matching) as usize;
            continue;
        }

        let substring = js_substring(vm, string, last_match_end as u32, (matched.start - last_match_end) as u32);
        result.push(string_value(&substring));
        if result.len() as u64 == u64::from(lim) {
            return Ok(array_of(global_object, &result));
        }
        last_match_end = match_end;

        if number_of_captures > 0 {
            // O mesmo casamento de novo, só para ler os grupos (`performMatch` guarda apenas o intervalo).
            let ovector = reg_exp.match_ovector(StringView::from(&value), matched.start as u32).unwrap_or_default();
            for capture_index in 1..=number_of_captures {
                let (start, end) = (ovector.get(2 * capture_index).copied().unwrap_or(-1), ovector.get(2 * capture_index + 1).copied().unwrap_or(-1));
                let capture = if start < 0 || end < start {
                    JSValue::undefined()
                } else {
                    string_value(&js_substring(vm, string, start as u32, (end - start) as u32))
                };
                result.push(capture);
                if result.len() as u64 == u64::from(lim) {
                    return Ok(array_of(global_object, &result));
                }
            }
        }
        match_position = last_match_end;
    }

    let substring = js_substring(vm, string, last_match_end as u32, (size - last_match_end) as u32);
    result.push(string_value(&substring));
    Ok(array_of(global_object, &result))
}

/// `regExpSplitSlow(globalObject, thisObject, string, limitValue)`
/// (https://tc39.es/ecma262/#sec-regexp.prototype-%symbol.split%), sem o atalho de `regExpSplitFast`.
fn reg_exp_split_slow(global_object: &JSGlobalObject, this_object: JSValue, string: &JSStringRef, limit_value: JSValue) -> HostResult {
    let vm = global_object.vm();
    let last_index = &vm.property_names.last_index;

    // 4. Let speciesCtor be ? SpeciesConstructor(regexp, %RegExp%).
    let species_constructor = reg_exp_species_constructor(global_object, this_object)?;

    // `regExpSplitFast`: com `exec`, `flags` e `Symbol.split` primordiais e o construtor original, o splitter da
    // especificação não é observável, e o JSC casa direto o `RegExp` (o que grava os `RegExp.$1`, `RegExp.input`...).
    if species_constructor == global_object.reg_exp_constructor() {
        if let Some(reg_exp_object) = reg_exp_split_fast_candidate(global_object, this_object) {
            return reg_exp_split_fast(global_object, &reg_exp_object, string, limit_value);
        }
    }

    // 5. Let flags be ? ToString(? Get(regexp, "flags")).
    let flags_value = get_object_property(global_object, this_object, &vm.property_names.flags)?;
    let flags = to_wtf_string_value(global_object, flags_value)?;

    // 6 e 7. unicodeMatching.
    let unicode_matching = contains_unit(&flags, b'u') || contains_unit(&flags, b'v');

    // 8 e 9. newFlags é flags, com "y" no fim se ainda não tem.
    let mut new_flags = code_units(&flags).into_owned();
    if !contains_unit(&flags, b'y') {
        new_flags.push(u16::from(b'y'));
    }

    // 10. Let splitter be ? Construct(speciesCtor, « regexp, newFlags »).
    let splitter = construct_value(global_object, species_constructor, &[this_object, units_value(vm, &new_flags)])?;

    // 11. Let array be ! ArrayCreate(0). 13. lim.
    let mut result: Vec<JSValue> = Vec::new();
    let lim = if limit_value.is_undefined() { u32::MAX } else { to_uint32_value(global_object, limit_value)? };

    // 14. If lim = 0, return array.
    if lim == 0 {
        return Ok(array_of(global_object, &result));
    }

    // 15. If string is the empty String, then
    let units = code_units(&string.value()).into_owned();
    let size = units.len();
    if size == 0 {
        let match_result = reg_exp_exec(global_object, splitter, string)?;
        if !match_result.is_null() {
            return Ok(array_of(global_object, &result));
        }
        result.push(string_value(string));
        return Ok(array_of(global_object, &result));
    }

    // 17. Let lastMatchEnd be 0. 18. Let searchIndex be lastMatchEnd.
    let mut last_match_end: usize = 0;
    let mut search_index: u64 = 0;
    // 19. Repeat, while searchIndex < size,
    while search_index < size as u64 {
        // 19.a. Perform ? Set(splitter, "lastIndex", 𝔽(searchIndex), true).
        set_object_property(global_object, splitter, last_index, js_number(search_index as f64))?;

        // 19.b. Let matchResult be ? RegExpExec(splitter, string).
        let match_result = reg_exp_exec(global_object, splitter, string)?;

        // 19.c. If matchResult is null, then advance.
        if match_result.is_null() {
            search_index = advance_string_index(&units, search_index, unicode_matching);
            continue;
        }

        // 19.d.i. Let matchEnd be ℝ(? ToLength(? Get(splitter, "lastIndex"))). ii. min(matchEnd, size).
        let last_index_value = get_object_property(global_object, splitter, last_index)?;
        let match_end = last_index_value.to_length_checked()?.min(size as u64) as usize;

        // 19.d.iii. If matchEnd = lastMatchEnd, then advance.
        if match_end == last_match_end {
            search_index = advance_string_index(&units, search_index, unicode_matching);
            continue;
        }

        // 19.d.iv.1 a 5. A substring de lastMatchEnd até searchIndex.
        let substring = js_substring(vm, string, last_match_end as u32, (search_index as usize - last_match_end) as u32);
        result.push(string_value(&substring));
        if result.len() as u64 == u64::from(lim) {
            return Ok(array_of(global_object, &result));
        }
        last_match_end = match_end;

        // 19.d.iv.6 e 7. numberOfCaptures = max(LengthOfArrayLike(matchResult) - 1, 0).
        let length_value = get_object_property(global_object, match_result, &vm.property_names.length)?;
        let length = length_value.to_length_checked()?;
        let number_of_captures = if length > 1 { length - 1 } else { 0 };

        // 19.d.iv.9. Repeat, while captureIndex ≤ numberOfCaptures,
        for capture_index in 1..=number_of_captures {
            let next_capture = get_object_index(global_object, match_result, capture_index as u32)?;
            result.push(next_capture);
            if result.len() as u64 == u64::from(lim) {
                return Ok(array_of(global_object, &result));
            }
        }
        // 19.d.iv.10. Set searchIndex to lastMatchEnd.
        search_index = last_match_end as u64;
    }

    // 20 e 21. A substring de lastMatchEnd até size.
    let substring = js_substring(vm, string, last_match_end as u32, (size - last_match_end) as u32);
    result.push(string_value(&substring));
    Ok(array_of(global_object, &result))
}

/// `regExpProtoFuncSplit`.
fn reg_exp_split(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_object = require_object(call.this_value(), "RegExp.prototype.@@split requires that |this| be an Object")?;
    let string = to_string_value(global_object, call.argument(0))?;
    reg_exp_split_slow(global_object, this_object, &string, call.argument(1))
}

/// `isASCIIDigit(ch)`.
fn digit_value(unit: u16) -> Option<usize> {
    (u16::from(b'0')..=u16::from(b'9')).contains(&unit).then(|| usize::from(unit - u16::from(b'0')))
}

/// `getSubstitution(globalObject, matched, str, position, captures, namedCaptures, replacement)`
/// (https://tc39.es/ecma262/#sec-getsubstitution): `captures` guarda `None` onde o C++ guarda a `String` nula.
fn get_substitution(
    global_object: &JSGlobalObject,
    matched: &[u16],
    string: &[u16],
    position: usize,
    captures: &[Option<Vec<u16>>],
    named_captures: Option<JSValue>,
    replacement: &[u16],
) -> Result<Vec<u16>, Thrown> {
    let vm = global_object.vm();
    let Some(mut start) = find_unit(replacement, b'$', 0) else {
        return Ok(replacement.to_vec());
    };

    let tail_pos = position + matched.len();
    let n_captures = captures.len();
    let replacement_length = replacement.len();
    let mut result: Vec<u16> = Vec::new();
    let mut last_start = 0usize;

    loop {
        if start > last_start {
            result.extend_from_slice(&replacement[last_start..start]);
        }

        start += 1;
        if start >= replacement_length {
            result.push(u16::from(b'$'));
            last_start = start;
            break;
        }

        let ch = replacement[start];
        match u8::try_from(ch).unwrap_or(0) {
            b'$' => {
                result.push(u16::from(b'$'));
                start += 1;
            }
            b'&' => {
                result.extend_from_slice(matched);
                start += 1;
            }
            b'`' => {
                if position > 0 {
                    result.extend_from_slice(&string[..position]);
                }
                start += 1;
            }
            b'\'' => {
                if tail_pos < string.len() {
                    result.extend_from_slice(&string[tail_pos..]);
                }
                start += 1;
            }
            b'<' => {
                let group_end = named_captures.and_then(|_| find_unit(replacement, b'>', start + 1));
                match (named_captures, group_end) {
                    (Some(named), Some(group_name_end)) => {
                        let group_name = Identifier::from_string(vm, &string_from_units(&replacement[start + 1..group_name_end]));
                        let capture = get_object_property(global_object, named, &group_name)?;
                        if !capture.is_undefined() {
                            let capture_string = to_wtf_string_value(global_object, capture)?;
                            result.extend_from_slice(&code_units(&capture_string));
                        }
                        start = group_name_end + 1;
                    }
                    _ => {
                        result.extend_from_slice(&[u16::from(b'$'), u16::from(b'<')]);
                        start += 1;
                    }
                }
            }
            _ => {
                if let Some(first_digit) = digit_value(ch) {
                    let original_start = start - 1;
                    start += 1;

                    let mut n = first_digit;
                    if n > n_captures {
                        result.extend_from_slice(&replacement[original_start..start]);
                    } else {
                        if let Some(second_digit) = replacement.get(start).copied().and_then(digit_value) {
                            let two_digits = 10 * n + second_digit;
                            if two_digits <= n_captures {
                                n = two_digits;
                                start += 1;
                            }
                        }
                        if n == 0 {
                            result.extend_from_slice(&replacement[original_start..start]);
                        } else if let Some(capture) = &captures[n - 1] {
                            result.extend_from_slice(capture);
                        }
                    }
                } else {
                    result.push(u16::from(b'$'));
                }
            }
        }

        last_start = start;
        match find_unit(replacement, b'$', last_start) {
            Some(next) => start = next,
            None => break,
        }
    }

    if last_start < replacement_length {
        result.extend_from_slice(&replacement[last_start..]);
    }
    Ok(result)
}

/// `regExpReplaceGeneric(globalObject, thisObject, string, replaceValue)`
/// (https://tc39.es/ecma262/#sec-regexp.prototype-%symbol.replace%).
fn reg_exp_replace_generic(
    global_object: &JSGlobalObject,
    this_object: JSValue,
    string: &JSStringRef,
    replace_value: JSValue,
) -> HostResult {
    let vm = global_object.vm();
    let last_index = &vm.property_names.last_index;
    let units = code_units(&string.value()).into_owned();
    let string_length = units.len();

    // 5. Let functionalReplace be IsCallable(replaceValue).
    let functional_replace = !get_call_data(replace_value).is_none();

    // 6. If functionalReplace is false, then set replaceValue to ? ToString(replaceValue).
    let replacement_string: Vec<u16> = if functional_replace {
        Vec::new()
    } else {
        code_units(&to_wtf_string_value(global_object, replace_value)?).into_owned()
    };

    // 7. Let flags be ? ToString(? Get(rx, "flags")).
    let flags_value = get_object_property(global_object, this_object, &vm.property_names.flags)?;
    let flags = to_wtf_string_value(global_object, flags_value)?;

    // 8 e 9. global e fullUnicode; Set(rx, "lastIndex", +0, true).
    let global = contains_unit(&flags, b'g');
    let mut full_unicode = false;
    if global {
        full_unicode = contains_unit(&flags, b'u') || contains_unit(&flags, b'v');
        set_object_property(global_object, this_object, last_index, js_number(0.0))?;
    }

    // 10 a 12. results.
    // O `MarkedArgumentBuffer results` do C++: os resultados vivem só neste vetor enquanto o callback roda JS.
    let marked = crate::runtime::gc::RootScope::new(&[]);
    let mut results: Vec<JSValue> = Vec::new();
    loop {
        let result = reg_exp_exec(global_object, this_object, string)?;
        if result.is_null() {
            break;
        }
        marked.append(result);
        results.push(result);
        if !global {
            break;
        }

        // 12.c.iii.1. Let matchStr be ? ToString(? Get(result, "0")).
        let match_value = get_object_index(global_object, result, 0)?;
        let match_string = to_string_value(global_object, match_value)?;
        if match_string.length() == 0 {
            let last_index_value = get_object_property(global_object, this_object, last_index)?;
            let this_index = last_index_value.to_length_checked()?;
            let next_index = advance_string_index(&units, this_index, full_unicode);
            set_object_property(global_object, this_object, last_index, js_number(next_index as f64))?;
        }
    }

    // 13 e 14. accumulatedResult e nextSourcePosition.
    let mut accumulated: Vec<u16> = Vec::new();
    let mut next_source_position = 0usize;

    // 15. For each element result of results, do
    for result in results {
        // a. Let resultLength be ? LengthOfArrayLike(result). b. nCaptures = max(resultLength - 1, 0).
        let length_value = get_object_property(global_object, result, &vm.property_names.length)?;
        let result_length = length_value.to_length_checked()?;
        // `unsigned nCaptures = resultLength > 1 ? resultLength - 1 : 0;` do C++ trunca o `uint64_t` para 32 bits
        // (um `length` de 2^32 + 5 dá 4 capturas no bun), por isso o `as u32` é de propósito.
        let n_captures = if result_length > 1 { (result_length - 1) as u32 } else { 0 };

        // c. Let matched be ? ToString(? Get(result, "0")).
        let matched_value = get_object_index(global_object, result, 0)?;
        let matched = code_units(&to_wtf_string_value(global_object, matched_value)?).into_owned();

        // e. Let position be ? ToIntegerOrInfinity(? Get(result, "index")). f. clamp(position, 0, lengthS).
        let position_value = get_object_property(global_object, result, &vm.property_names.index)?;
        let position_double = position_value.to_integer_or_infinity_checked()?;
        let position = position_double.clamp(0.0, string_length as f64) as usize;

        // g. `captures.tryReserveCapacity(nCaptures)` do C++ (um `Vector<String>`, 8 bytes por posição): um
        // `length` gigante vira `OutOfMemoryError` antes de ler qualquer captura, pelo limite de `UINT_MAX` bytes do
        // `WTF::Vector` (o `length` 2^32 dá `Out of memory` no bun mesmo com memória sobrando). A sonda tem o mesmo tamanho.
        if try_wtf_vector_with_capacity::<u64>(n_captures as usize).is_none() {
            return Err(Thrown::OutOfMemory);
        }
        let mut captures: Vec<Option<Vec<u16>>> = Vec::new();
        for n in 1..=n_captures {
            let cap_n = get_object_index(global_object, result, n)?;
            if cap_n.is_undefined() {
                captures.push(None);
            } else {
                captures.push(Some(code_units(&to_wtf_string_value(global_object, cap_n)?).into_owned()));
            }
        }

        // j. Let namedCaptures be ? Get(result, "groups").
        let named_captures_value = get_object_property(global_object, result, &vm.property_names.groups)?;

        let replacement: Vec<u16> = if functional_replace {
            // j.i. Let replacerArgs be « matched », captures, « 𝔽(position), S ».
            let mut replacer_args = vec![units_value(vm, &matched)];
            for capture in &captures {
                replacer_args.push(capture.as_ref().map_or(JSValue::undefined(), |capture| units_value(vm, capture)));
            }
            replacer_args.push(js_number(position as f64));
            replacer_args.push(string_value(string));
            // j.ii. If namedCaptures is not undefined, append namedCaptures to replacerArgs.
            if !named_captures_value.is_undefined() {
                replacer_args.push(named_captures_value);
            }
            // j.iii e iv. Call(replaceValue, undefined, replacerArgs) e ToString.
            let replacement_value = call_checked(global_object, replace_value, JSValue::undefined(), &replacer_args, "Type error")?;
            code_units(&to_wtf_string_value(global_object, replacement_value)?).into_owned()
        } else {
            // k.i. If namedCaptures is not undefined, set namedCaptures to ? ToObject(namedCaptures).
            let named_captures = if named_captures_value.is_undefined() {
                None
            } else {
                Some(named_captures_value.to_object(global_object).ok_or(Thrown::Pending)?.as_value())
            };
            // k.ii. GetSubstitution(matched, S, position, captures, namedCaptures, replaceValue).
            get_substitution(global_object, &matched, &units, position, &captures, named_captures, &replacement_string)?
        };

        // m. If position ≥ nextSourcePosition, then
        if position >= next_source_position {
            accumulated.extend_from_slice(&units[next_source_position..position]);
            accumulated.extend_from_slice(&replacement);
            next_source_position = position + matched.len();
        }
    }

    // 16 e 17. O resto da string depois de nextSourcePosition.
    if next_source_position < string_length {
        accumulated.extend_from_slice(&units[next_source_position..]);
    }
    Ok(units_value(vm, &accumulated))
}

/// `regExpProtoFuncReplace`.
fn reg_exp_replace(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    reg_exp_replace_values(global_object, call.this_value(), call.argument(0), call.argument(1))
}

/// O corpo de `regExpProtoFuncReplace` sobre os valores já tirados do `CallFrame`.
fn reg_exp_replace_values(global_object: &JSGlobalObject, this_value: JSValue, string_value: JSValue, replace_value: JSValue) -> HostResult {
    let this_object = require_object(this_value, "RegExp.prototype.@@replace requires that |this| be an Object")?;
    let string = to_string_value(global_object, string_value)?;
    reg_exp_replace_generic(global_object, this_object, &string, replace_value)
}

/// O atalho de `isSymbolReplaceFastAndNonObservable` em `stringProtoFuncReplace`: quando `method` é o próprio
/// `RegExp.prototype[Symbol.replace]` nativo, o `String.prototype.replace` roda o corpo dele sem abrir um frame
/// `[Symbol.replace]` (o `bun` não mostra esse frame entre o callback e `replace`). `None` para qualquer outro `method`.
pub fn call_builtin_reg_exp_replace(global_object: &JSGlobalObject, method: JSValue, this_value: JSValue, arguments: &[JSValue]) -> Option<HostResult> {
    let is_builtin = matches!(get_call_data(method), CallData::Native { function, .. } if function == to_tagged(reg_exp_proto_func_replace));
    is_builtin.then(|| reg_exp_replace_values(global_object, this_value, arguments[0], arguments[1]))
}

/// `regExpProtoFuncMatchAll` (https://tc39.es/ecma262/#sec-regexp.prototype-%symbol.matchall%), sem o
/// atalho de `isSymbolMatchAllFastAndNonObservable`.
fn reg_exp_match_all(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let this_object = require_object(call.this_value(), "RegExp.prototype.@@matchAll requires that |this| be an Object")?;
    let string = to_string_value(global_object, call.argument(0))?;

    let constructor = reg_exp_species_constructor(global_object, this_object)?;

    let flags_value = get_object_property(global_object, this_object, &vm.property_names.flags)?;
    let flags = to_wtf_string_value(global_object, flags_value)?;

    let matcher = construct_value(global_object, constructor, &[this_object, JSValue::from_js_string(js_string(vm, &flags))])?;

    let last_index_value = get_object_property(global_object, this_object, &vm.property_names.last_index)?;
    let last_index = last_index_value.to_length_checked()?;
    set_object_property(global_object, matcher, &vm.property_names.last_index, js_number(last_index as f64))?;

    let global = contains_unit(&flags, b'g');
    let full_unicode = contains_unit(&flags, b'u') || contains_unit(&flags, b'v');
    let iterator = JSRegExpStringIterator::create_with_initial_values(vm, &global_object.reg_exp_string_iterator_structure());
    iterator.set_reg_exp(matcher);
    iterator.set_string(string_value(&string));
    iterator.set_flags(global, full_unicode, false);
    Ok(iterator.as_value())
}

host_function!(reg_exp_proto_func_match, reg_exp_match);
host_function!(reg_exp_proto_func_match_all, reg_exp_match_all);
host_function!(reg_exp_proto_func_replace, reg_exp_replace);
host_function!(reg_exp_proto_func_search, reg_exp_search);
host_function!(reg_exp_proto_func_split, reg_exp_split);

/// `regExpCreate(globalObject, newTarget, syntaxValue, flagsValue)` para quem não tem `newTarget`
/// (`String.prototype.match`, `search` e `matchAll`): o padrão é `undefined` (vazio) ou o texto de
/// `pattern_value`; `flags` já vêm prontas.
pub fn reg_exp_create_from_value(global_object: &JSGlobalObject, pattern_value: JSValue, flags: FlagSet) -> Result<JSValue, Thrown> {
    let pattern = if pattern_value.is_undefined() {
        WtfString::default()
    } else {
        to_wtf_string_value(global_object, pattern_value)?
    };
    reg_exp_create(global_object, None, 0, &pattern, flags)
}

// ------------------------------ RegExp constructor --------------------------

/// O que `constructRegExp` pode lançar além do que já está pendente.
fn new_target_of(call_frame: &NativeCallFrame<'_>, is_construct: bool) -> Option<JSValue> {
    if !is_construct {
        return None;
    }
    let new_target = call_frame.this_value();
    if new_target.is_empty() || new_target.is_undefined() { None } else { Some(new_target) }
}

/// `getRegExpStructure(globalObject, newTarget)`: sem `newTarget` a estrutura do próprio realm; com ele o
/// `JSC_GET_DERIVED_STRUCTURE`, que lê a base no realm do `newTarget` e pode lançar.
fn get_reg_exp_structure(global_object: &JSGlobalObject, new_target: Option<JSValue>, callee: usize) -> Result<StructureRef, Thrown> {
    match new_target {
        None => Ok(global_object.reg_exp_structure()),
        Some(target) => get_derived_structure_in_realm(global_object, target, callee, |realm| realm.reg_exp_structure()),
    }
}

/// `RegExpObject::create` com a estrutura já resolvida e `areLegacyFeaturesEnabled(globalObject, newTarget)`:
/// `newTarget` ausente ou o próprio `RegExp` (`callee`) mantém o legado.
fn make_reg_exp_object(
    global_object: &JSGlobalObject,
    structure: StructureRef,
    new_target: Option<JSValue>,
    callee: usize,
    reg_exp: crate::runtime::reg_exp::RegExpRef,
) -> JSValue {
    let legacy = new_target.is_none_or(|target| target == JSValue::from_cell(callee));
    RegExpObject::create(global_object.vm(), structure, reg_exp, legacy).as_value()
}

/// `regExpCreate(globalObject, newTarget, pattern, flags)`: o `RegExpObject` ou a exceção pendente.
pub fn reg_exp_create(
    global_object: &JSGlobalObject,
    new_target: Option<JSValue>,
    callee: usize,
    pattern: &WtfString,
    flags: FlagSet,
) -> Result<JSValue, Thrown> {
    let reg_exp = RegExp::create(global_object.vm(), pattern, flags);
    if !reg_exp.is_valid() {
        throw_construction_error(global_object, &reg_exp);
        return Err(Thrown::Pending);
    }
    let structure = get_reg_exp_structure(global_object, new_target, callee)?;
    Ok(make_reg_exp_object(global_object, structure, new_target, callee, reg_exp))
}

/// `constructRegExp(globalObject, args, callee, newTarget)`.
fn construct_reg_exp(global_object: &JSGlobalObject, call_frame: &NativeCallFrame<'_>, is_construct: bool) -> EncodedJSValue {
    let vm = global_object.vm();
    let callee = call_frame.js_callee();
    let new_target = new_target_of(call_frame, is_construct);
    let mut pattern_arg = call_frame.argument(0);
    let mut flags_arg = call_frame.argument(1);

    let is_pattern_reg_exp = as_reg_exp_object(&pattern_arg);
    let construct_as_reg_exp = is_reg_exp(global_object, &pattern_arg);
    if vm.exception().is_some() {
        return JSValue::empty().encode();
    }

    let get = |object: &JSValue, name: &Identifier| {
        ObjectRef::from_value(object)
            .map_or_else(JSValue::undefined, |object| object.get(global_object, &PropertyName::from_identifier(name)))
    };

    if new_target.is_none() && construct_as_reg_exp && flags_arg.is_undefined() {
        let constructor = get(&pattern_arg, &vm.property_names.constructor);
        if vm.exception().is_some() {
            return JSValue::empty().encode();
        }
        if constructor == JSValue::from_cell(callee) {
            return pattern_arg.encode();
        }
    }

    if let Some(source) = is_pattern_reg_exp {
        let mut reg_exp = source.reg_exp();
        let structure = match get_reg_exp_structure(global_object, new_target, callee) {
            Ok(structure) => structure,
            Err(thrown) => {
                throw_thrown(global_object, thrown);
                return JSValue::empty().encode();
            }
        };
        if !flags_arg.is_undefined() {
            let flags = match to_flags(global_object, &flags_arg) {
                Ok(flags) => flags,
                Err(thrown) => return thrown,
            };
            reg_exp = RegExp::create(vm, reg_exp.pattern(), flags);
            if !reg_exp.is_valid() {
                return throw_construction_error(global_object, &reg_exp);
            }
        }
        return make_reg_exp_object(global_object, structure, new_target, callee, reg_exp).encode();
    }

    if construct_as_reg_exp {
        let pattern = get(&pattern_arg, &vm.property_names.source);
        if vm.exception().is_some() {
            return JSValue::empty().encode();
        }
        if flags_arg.is_undefined() {
            flags_arg = get(&pattern_arg, &vm.property_names.flags);
            if vm.exception().is_some() {
                return JSValue::empty().encode();
            }
        }
        pattern_arg = pattern;
    }

    let pattern = match to_pattern(global_object, &pattern_arg) {
        Ok(pattern) => pattern,
        Err(thrown) => return thrown,
    };
    let flags = match to_flags(global_object, &flags_arg) {
        Ok(flags) => flags,
        Err(thrown) => return thrown,
    };
    match reg_exp_create(global_object, new_target, callee, &pattern, flags) {
        Ok(value) => value.encode(),
        Err(thrown) => {
            throw_thrown(global_object, thrown);
            JSValue::empty().encode()
        }
    }
}

/// `callRegExpConstructor`.
fn call_reg_exp_constructor(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    construct_reg_exp(global_object, call_frame, false)
}

/// `constructWithRegExpConstructor`.
fn construct_with_reg_exp_constructor(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    construct_reg_exp(global_object, call_frame, true)
}

/// `class RegExpConstructor : public InternalFunction`.
pub struct RegExpConstructor;

impl RegExpConstructor {
    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::InternalFunctionType, InternalFunction::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE),
            &REG_EXP_CONSTRUCTOR_S_INFO,
        )
    }

    /// `create(vm, structure, regExpPrototype)` e `finishCreation`: `length` 2, `name` "RegExp", `prototype`
    /// (`DontEnum|DontDelete|ReadOnly`) e o `@@species`, no `realm()` da `Structure` (o
    /// `regExpPrototype->realm()` do C++). Liga também o `constructor` do protótipo (`DontEnum`).
    pub fn create(vm: &VM, structure: StructureRef, reg_exp_prototype: &JSObject) -> InternalFunctionRef {
        let global_object = structure.realm().expect("a Structure do RegExp sempre tem realm");
        let constructor =
            InternalFunction::new(vm, structure, call_reg_exp_constructor, Some(construct_with_reg_exp_constructor));
        // Os acessores legados (`REG_EXP_CONSTRUCTOR_TABLE`) não nascem aqui: reificam sob demanda.
        constructor.finish_creation(vm, 2, &WtfString::from_latin1(b"RegExp"), PropertyAdditionMode::WithoutStructureTransition);
        constructor.put_direct_without_transition(
            vm,
            &PropertyName::from_identifier(&vm.property_names.prototype),
            reg_exp_prototype.as_value(),
            DONT_ENUM | DONT_DELETE | READ_ONLY,
        );
        crate::runtime::reg_exp_legacy_natives::install_escape(vm, &global_object, &constructor);
        put_species_accessor(vm, &global_object, &constructor);
        reg_exp_prototype.put_direct_without_transition(
            vm,
            &PropertyName::from_identifier(&vm.property_names.constructor),
            constructor.as_value(),
            DONT_ENUM,
        );
        constructor
    }
}
