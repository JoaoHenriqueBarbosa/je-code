//! As cascas finas das funções globais de `JSGlobalObjectFunctions.cpp` (`globalFuncParseInt`,
//! `globalFuncParseFloat`, `globalFuncIsNaN`, `globalFuncIsFinite`, `globalFuncEscape`, `globalFuncUnescape`,
//! `globalFuncDecodeURI`, `globalFuncDecodeURIComponent`, `globalFuncEncodeURI`, `globalFuncEncodeURIComponent`) e
//! a tabela `globalObjectTable` de `JSGlobalObject.cpp` (nome, `length`, intrínseco, `DontEnum`) que as
//! instala no global, mais `m_parseIntFunction` e `m_parseFloatFunction`.
//!
//! DIVERGÊNCIAS:
//! - `globalObjectTable` é uma tabela estática preguiçosa (`reifyStaticProperties`); aqui as propriedades
//!   entram em `JSGlobalObject::init` (`add_global_functions`), na ordem da tabela. `parseInt` e
//!   `parseFloat` (`CellProperty` sobre os `LazyProperty` `m_parseIntFunction`/`m_parseFloatFunction`) são
//!   criadas ali também e guardadas nos campos do global.
//! - `eval` (`globalFuncEval`) é criado ali também, guardado em `set_eval_function` e na `LinkTimeConstant`
//!   `EvalFunction`. O restante da tabela (`globalThis`, construtores, namespaces) espera os respectivos
//!   objetos.
//! - `toStringView`/`toNumber` de objeto (que chamam `toString`/`valueOf` do usuário) seguem a lacuna de
//!   `JSValue::to_string`/`to_number`.

use crate::bytecode::bytecode_intrinsics_table::LinkTimeConstant;
use crate::host_function;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::js_getter_setter::{GetterSetter, GetterSetterRef};
use crate::runtime::error_messages::RESTRICTED_PROPERTY_ACCESS_ERROR;
use crate::runtime::vm::VM;
use crate::runtime::js_object::JSObject;
use crate::runtime::proxy_object::{object_set_prototype, to_this_strict};
use crate::interpreter::call_frame::NativeCallFrame;
use crate::llint::LLIntFailure;
use crate::parser::source_tainted_origin::SourceTaintedOrigin;
use crate::runtime::current_realm::{current_global_object, CurrentRealmScope};
use crate::interpreter::caller_source_origin::caller_source_origin;
use crate::runtime::error::{create_type_error, create_uri_error};
use crate::runtime::exception_helpers::throw_out_of_memory_error;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{call_host_function_as_constructor, put_direct_native_function_without_transition, JSFunction};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_global_object_functions as pure;
use crate::runtime::js_global_object_functions::GlobalFunctionError;
use crate::runtime::js_value::{js_boolean, js_number, js_undefined, EncodedJSValue, JSValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{ACCESSOR, DONT_ENUM};
use crate::runtime::property_name::PropertyName;
use crate::runtime::string_prototype_natives::to_wtf_string_or_type_error;
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::wtf::text::wtf_string::String as WtfString;

/// `toStringView(globalObject, value, ...)`: as unidades do `toString` do valor, ou a exceção pendente
/// (`None`).
fn string_units(global_object: &JSGlobalObject, value: &JSValue) -> Option<Vec<u16>> {
    match to_wtf_string_or_type_error(value) {
        Ok(string) => Some(pure::units_of(&string)),
        Err(message) => {
            let mut scope = ThrowScope::new(global_object.vm());
            let message = WtfString::from_utf8(message.as_bytes());
            throw_exception(global_object, &mut scope, create_type_error(global_object, &message));
            None
        }
    }
}

/// O resultado de `encode`/`decode`/`escape`/`unescape` como valor, ou a exceção pendente.
fn uri_result(global_object: &JSGlobalObject, result: Result<WtfString, GlobalFunctionError>) -> EncodedJSValue {
    let mut scope = ThrowScope::new(global_object.vm());
    match result {
        Ok(string) => {
            JSValue::from_js_string(crate::runtime::js_string::js_string(global_object.vm(), &string)).encode()
        }
        Err(GlobalFunctionError::UriError(message)) => {
            let message = WtfString::from_utf8(message.as_bytes());
            throw_exception(global_object, &mut scope, create_uri_error(global_object, &message));
            JSValue::empty().encode()
        }
        Err(GlobalFunctionError::OutOfMemory) => {
            throw_out_of_memory_error(global_object, &mut scope);
            JSValue::empty().encode()
        }
    }
}

/// Uma função global de string para string (`encode`, `decode`, `escape`, `unescape`).
fn string_function(
    global_object: &JSGlobalObject,
    call_frame: &NativeCallFrame<'_>,
    body: impl FnOnce(&[u16]) -> Result<WtfString, GlobalFunctionError>,
) -> EncodedJSValue {
    match string_units(global_object, &call_frame.argument(0)) {
        Some(units) => uri_result(global_object, body(&units)),
        None => JSValue::empty().encode(),
    }
}

/// `globalFuncParseInt`.
fn global_func_parse_int(global_object: & JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let value = call_frame.argument(0);
    let radix_value = call_frame.argument(1);

    if value.is_number() && (radix_value.is_undefined_or_null() || (radix_value.is_int32() && radix_value.as_int32() == 10)) {
        if value.is_int32() {
            return value.encode();
        }
        if let Some(result) = pure::parse_int_number(value.as_number()) {
            return js_number(result).encode();
        }
    }

    // "If ToString throws, we shouldn't call ToInt32."
    match string_units(global_object, &value) {
        Some(units) => js_number(pure::parse_int_string(&units, radix_value.to_int32())).encode(),
        None => JSValue::empty().encode(),
    }
}

/// `globalFuncParseFloat`.
fn global_func_parse_float(global_object: & JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let value = call_frame.argument(0);
    if value.is_number() {
        if value.is_int32() {
            return value.encode();
        }
        return js_number(pure::parse_float_number(value.as_number())).encode();
    }
    match string_units(global_object, &value) {
        Some(units) => js_number(pure::parse_float(&units)).encode(),
        None => JSValue::empty().encode(),
    }
}

/// `globalFuncIsNaN`.
fn global_func_is_nan(_global_object: & JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    js_boolean(call_frame.argument(0).to_number().is_nan()).encode()
}

/// `globalFuncIsFinite`.
pub(crate) fn global_func_is_finite(_global_object: & JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    js_boolean(call_frame.argument(0).to_number().is_finite()).encode()
}

fn global_func_escape(global_object: & JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    string_function(global_object, call_frame, pure::escape)
}

fn global_func_unescape(global_object: & JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    string_function(global_object, call_frame, pure::unescape)
}

fn global_func_decode_uri(global_object: & JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    string_function(global_object, call_frame, pure::decode_uri)
}

fn global_func_decode_uri_component(global_object: & JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    string_function(global_object, call_frame, pure::decode_uri_component)
}

fn global_func_encode_uri(global_object: & JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    string_function(global_object, call_frame, pure::encode_uri)
}

fn global_func_encode_uri_component(global_object: & JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    string_function(global_object, call_frame, pure::encode_uri_component)
}

/// `globalFuncEval`: o `eval` indireto sobre o primeiro argumento, por `Interpreter::global_func_eval`
/// (`execute_eval.rs`). A exceção pendente (inclusive o `SyntaxError` do parser) volta como o valor vazio.
///
/// A origem é a do chamador (`caller_source_origin`); uma lacuna do interpretador (`Unported`) aborta com o nome dela.
fn global_func_eval(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let _realm = CurrentRealmScope::enter(global_object);
    let global_object = current_global_object();
    // Os frames de `globalFuncEval` e `Interpreter::executeEval` que ficam na pilha enquanto o código roda.
    let Some(_stack_frame) = global_object.vm().enter_logical_frame(crate::runtime::vm::stack_cost::INDIRECT_EVAL) else {
        let mut scope = crate::runtime::throw_scope::ThrowScope::new(global_object.vm());
        crate::runtime::exception_helpers::throw_stack_overflow_error(&global_object, &mut scope);
        return JSValue::empty().encode();
    };
    let source_origin = caller_source_origin(&global_object.vm().interpreter(), call_frame.call_frame());
    let eval_origin = crate::runtime::stack_frame::v8_stack_format()
        .then(|| crate::runtime::v8_stack::eval_origin(&global_object, call_frame.call_frame()));
    let result = global_object.vm().interpreter().global_func_eval(
        &global_object,
        call_frame.argument(0),
        &source_origin,
        SourceTaintedOrigin::Untainted,
        eval_origin,
    );
    match result {
        Ok(value) => value.encode(),
        Err(LLIntFailure::Thrown) => JSValue::empty().encode(),
        Err(unported) => panic!("eval: {unported:?} ainda não portado"),
    }
}

/// `globalFuncProtoGetter`: `thisValue.toThis(globalObject, strict).getPrototype(globalObject)`. O `toThis`
/// estrito devolve `undefined` para um `this` que herda de `JSScope` (o global, o ambiente léxico), então o
/// getter herdado chamado com o objeto global como `this` (`__proto__` solto) lança `TypeError`.
fn global_func_proto_getter_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    to_this_strict(call.this_value()).get_prototype(global_object)
}
host_function!(global_func_proto_getter, global_func_proto_getter_body);

/// `globalFuncProtoSetter`: `Object.prototype.__proto__` como `setPrototype` (primitivo e valor que
/// não é objeto nem `null` são ignorados, "to match Mozilla").
fn global_func_proto_setter_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_value = to_this_strict(call.this_value());
    if this_value.is_undefined_or_null() {
        return Err(Thrown::type_error("Object.prototype.__proto__ called on null or undefined"));
    }

    let value = call.argument(0);
    // Setting __proto__ of a primitive should have no effect.
    let Some(this_object) = ObjectRef::from_value(&this_value) else {
        return Ok(js_undefined());
    };

    // Setting __proto__ to a non-object, non-null value is silently ignored to match Mozilla.
    if !value.is_object() && !value.is_null() {
        return Ok(js_undefined());
    }

    object_set_prototype(global_object, &this_object, value, true)?;
    Ok(js_undefined())
}
host_function!(global_func_proto_setter, global_func_proto_setter_body);

/// `globalFuncThrowTypeErrorArgumentsCalleeAndCaller`: `throwVMTypeError(globalObject, scope, RestrictedPropertyAccessError)`.
fn global_func_throw_type_error_arguments_callee_and_caller_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(Thrown::type_error(RESTRICTED_PROPERTY_ACCESS_ERROR))
}
host_function!(
    global_func_throw_type_error_arguments_callee_and_caller,
    global_func_throw_type_error_arguments_callee_and_caller_body
);

/// O inicializador do `m_throwTypeErrorArgumentsCalleeGetterSetter` (`JSGlobalObject::init`): um `JSFunction`
/// de nome vazio e `length` 0, congelado (`thrower->freeze(vm)`), como getter e setter do mesmo `GetterSetter`.
///
/// O `freeze` do porte não passa pelo `getOwnSpecialPropertyNames` da função, então `length` e `name` são
/// materializados antes (o C++ os materializa dentro do `getOwnPropertyNames` que o `freeze` chama).
pub fn create_throw_type_error_arguments_callee_getter_setter(vm: &VM, global_object: &JSGlobalObject) -> GetterSetterRef {
    let thrower = JSFunction::create_native(
        vm,
        global_object,
        0,
        &WtfString::from_latin1(b""),
        global_func_throw_type_error_arguments_callee_and_caller,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    );
    for name in [&vm.property_names.length, &vm.property_names.name] {
        let _ = thrower.reify_lazy_property_if_needed(global_object, &PropertyName::from_identifier(name), false);
    }
    thrower.freeze(vm);
    GetterSetter::create_from_values(vm, thrower.as_value(), thrower.as_value())
}

/// O acessor `__proto__` do `Object.prototype` (`JSGlobalObject::init`, logo depois do `ObjectPrototype`):
/// `GetterSetter` de dois `JSFunction` `"get __proto__"` (`UnderscoreProtoIntrinsic`) e `"set __proto__"`,
/// ambos com `length` 0, `Accessor|DontEnum`.
pub fn add_underscore_proto_accessor(global_object: &JSGlobalObject, object_prototype: &JSObject) {
    let vm = global_object.vm();
    let create = |name: &[u8], function: NativeFunction, intrinsic: Intrinsic| {
        JSFunction::create_native(
            vm,
            global_object,
            0,
            &WtfString::from_latin1(name),
            function,
            ImplementationVisibility::Public,
            intrinsic,
            call_host_function_as_constructor,
        )
    };
    let getter = create(b"get __proto__", global_func_proto_getter, Intrinsic::UnderscoreProtoIntrinsic);
    let setter = create(b"set __proto__", global_func_proto_setter, Intrinsic::NoIntrinsic);
    let accessor = GetterSetter::create_from_values(vm, getter.as_value(), setter.as_value());
    object_prototype.put_direct_non_index_accessor_without_transition(
        vm,
        &PropertyName::from_identifier(&vm.property_names.underscore_proto),
        &accessor,
        ACCESSOR | DONT_ENUM,
    );
}

/// A parte portada de `globalObjectTable` (todas `DontEnum|Function`, `length` 1) e os `LazyProperty`
/// `m_parseIntFunction` (`length` 2, `ParseIntIntrinsic`) e `m_parseFloatFunction` (`length` 1).
pub fn add_global_functions(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let literal = |text: &[u8]| Identifier::from_span(vm, text);
    let define = |name: &[u8], function: NativeFunction, intrinsic: Intrinsic| {
        put_direct_native_function_without_transition(
            vm,
            global_object,
            global_object,
            &literal(name),
            1,
            function,
            ImplementationVisibility::Public,
            intrinsic,
            DONT_ENUM,
        );
    };
    // O `queueMicrotask` é do bun (WebCore), não do JSC: ver `queue_microtask.rs`. No bun ele vem logo
    // depois de `Infinity`, `undefined`, `NaN` e dos globais web (`addEventListener`...`prompt`), 84
    // posições antes de `parseFloat`; sem os globais web do bun, a posição equivalente é a primeira
    // depois das três constantes, antes de `isNaN`.
    // `setTimeout` e companhia também são do bun (`timers.rs`); na ordem de chaves medida no bun, os
    // `clear*` vêm antes de `queueMicrotask` e os `set*` depois.
    // `atob` e `btoa` (bun/WebCore, `base64_globals.rs`) vêm antes dos `clear*`.
    crate::runtime::dialogs::add_alert(global_object);
    crate::runtime::base64_globals::add_base64_functions(global_object);
    crate::runtime::timers::add_clear_functions(global_object);
    crate::runtime::dialogs::add_confirm(global_object);
    crate::runtime::queue_microtask::add_queue_microtask(global_object);
    // `reportError` e `structuredClone` (bun/WebCore): o primeiro entre `queueMicrotask` e os `set*`, o segundo
    // depois deles.
    crate::runtime::report_error::add_report_error(global_object);
    crate::runtime::post_message::add_post_message(global_object);
    crate::runtime::dialogs::add_prompt(global_object);
    crate::runtime::timers::add_set_functions(global_object);
    crate::runtime::structured_clone::add_structured_clone(global_object);
    crate::runtime::process_object::add_process(global_object);
    define(b"isNaN", global_func_is_nan, Intrinsic::GlobalIsNaNIntrinsic);
    define(b"isFinite", global_func_is_finite, Intrinsic::GlobalIsFiniteIntrinsic);
    define(b"escape", global_func_escape, Intrinsic::NoIntrinsic);
    define(b"unescape", global_func_unescape, Intrinsic::NoIntrinsic);
    define(b"decodeURI", global_func_decode_uri, Intrinsic::NoIntrinsic);
    define(b"decodeURIComponent", global_func_decode_uri_component, Intrinsic::NoIntrinsic);
    define(b"encodeURI", global_func_encode_uri, Intrinsic::NoIntrinsic);
    define(b"encodeURIComponent", global_func_encode_uri_component, Intrinsic::NoIntrinsic);

    let create = |name: &Identifier, length: u32, function: NativeFunction, intrinsic: Intrinsic| {
        let created = JSFunction::create_native(
            vm,
            global_object,
            length,
            name.string().string(),
            function,
            ImplementationVisibility::Public,
            intrinsic,
            call_host_function_as_constructor,
        );
        global_object.put_direct(vm, &PropertyName::from_identifier(name), created.as_value(), DONT_ENUM);
        created
    };
    // `m_linkTimeConstants[LinkTimeConstant::evalFunction]` (`JSFunction::create(vm, this, 1, "eval",
    // globalFuncEval)`), que a propriedade global `eval` (`initializeEvalFunction`, `DontEnum`) devolve
    // e que o `op_call_direct_eval` e o `op_jneq_ptr` do `eval(...spread)` comparam com o callee.
    // A ordem medida no bun é `eval`, `globalThis`, `parseInt`, `parseFloat`.
    let eval = create(&vm.property_names.eval, 1, global_func_eval, Intrinsic::NoIntrinsic);
    global_object.set_eval_function(eval.as_value().as_cell());
    global_object.set_link_time_constant(LinkTimeConstant::EvalFunction, eval.as_value());

    // A propriedade `globalThis` da `globalObjectTable` (`DontEnum`, o valor do `m_globalThis`).
    if let Some(global_this) = global_object.global_this() {
        global_object.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.global_this), global_this.as_value(), DONT_ENUM);
    }

    let parse_int = create(&vm.property_names.parse_int, 2, global_func_parse_int, Intrinsic::ParseIntIntrinsic);
    let parse_float = create(&vm.property_names.parse_float, 1, global_func_parse_float, Intrinsic::NoIntrinsic);
    *global_object.parse_int_function.borrow_mut() = Some(parse_int.as_value());
    *global_object.parse_float_function.borrow_mut() = Some(parse_float.as_value());
}

impl JSGlobalObject {
    /// `parseIntFunction()` (o valor da `JSFunction`).
    pub fn parse_int_function(&self) -> JSValue {
        self.parse_int_function.borrow().expect("JSGlobalObject sem parseIntFunction")
    }

    /// `parseFloatFunction()` (o valor da `JSFunction`).
    pub fn parse_float_function(&self) -> JSValue {
        self.parse_float_function.borrow().expect("JSGlobalObject sem parseFloatFunction")
    }
}
