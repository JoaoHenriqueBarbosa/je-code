//! Porte de `runtime/FunctionPrototype.h`, `FunctionPrototypeInlines.h` e `FunctionPrototype.cpp`: o
//! `Function.prototype`, um `InternalFunction` (chamável, devolve `undefined`) com o `ClassInfo` próprio.
//!
//! LACUNAS de `addFunctionProperties`, e por quê:
//! - `call` e `apply` são builtins JS (`FunctionPrototype.js`) e entram por
//!   `put_direct_builtin_function_without_transition`; o `callFunction`/`applyFunction` que o C++ devolve
//!   ao global não é guardado (o porte ainda não os consulta).
//! - `arguments` e `caller` são `CustomGetterSetter` (`argumentsGetter`, `callerGetter`,
//!   `callerAndArgumentsSetter`) sobre o `StackVisitor` (`interpreter/stack_visitor.rs`), a partir do
//!   `vm.topCallFrame` que o laço e a chamada de função nativa gravam. Sem `JSRemoteFunction` o teste dele em
//!   `RetrieveCallerFunctionFunctor` não existe.
//! - `functionProtoFuncToString`, `functionProtoFuncSymbolHasInstance` (com o ramo de `JSBoundFunction`) e
//!   `functionProtoFuncBind` (com `JSBoundFunction::create`) estão completos; o ramo de
//!   `JSRemoteFunction` de `JSFunction::toString` não existe enquanto esse tipo não for portado, e o
//!   `source` de taint do `bind` (`sourceTaintedOriginFromStack`) não existe.
//! - `callFunctionPrototype` está completo.

use crate::custom_getter;
use crate::interpreter::call_frame::{CallFrame, NativeCallFrame};
use crate::interpreter::stack_visitor::{IterationStatus, StackVisitor};
use crate::parser::parser_modes::{is_async_function_parse_mode, is_generator_parse_mode, SourceParseMode};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::current_realm::CurrentRealmScope;
use crate::runtime::custom_getter_setter::CustomGetterSetter;
use crate::runtime::error_messages::RESTRICTED_PROPERTY_ACCESS_ERROR;
use crate::runtime::host_call::{throw_thrown, HostResult, Thrown};
use crate::runtime::host_function_support::{default_has_instance, throw_vm_type_error};
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::js_bound_function::{object_has_instance, JSBoundFunction};
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::internal_function::{InternalFunction, InternalFunctionRef, PropertyAdditionMode, INTERNAL_FUNCTION_S_INFO};
use crate::runtime::builtins_source::BuiltinCodeIndex;
use crate::runtime::js_function::{
    call_host_function_as_constructor, put_direct_builtin_function_without_transition,
    put_direct_native_function_without_transition, JSFunction, JSFunctionRef,
};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_string::js_empty_string;
use crate::runtime::js_string_builder::js_make_nontrivial_string;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_boolean, js_null, js_undefined, EncodedJSValue, JSValue};
use crate::runtime::property_attribute::{CUSTOM_ACCESSOR, DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo FunctionPrototype::s_info`.
pub static FUNCTION_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `callFunctionPrototype`: https://tc39.es/ecma262/#sec-properties-of-the-function-prototype-object
pub fn call_function_prototype(_global_object: &JSGlobalObject, _call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    js_undefined().encode()
}

/// `functionProtoFuncToString`.
fn function_proto_func_to_string(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let this_value = call_frame.this_value();
    if let Some(function) = this_value.as_js_function() {
        return match function.to_string(global_object) {
            Some(string) => JSValue::from_js_string(string).encode(),
            None => JSValue::empty().encode(),
        };
    }

    if this_value.is_cell() {
        if let Some(function) = InternalFunction::from_cell_id(this_value.as_cell()) {
            let name = function.name();
            return native_code_string(global_object, &name);
        }
    }

    if this_value.is_object() && this_value.is_callable() {
        let class_name = this_value.as_object().class_info().class_name;
        return native_code_string(global_object, &class_name);
    }

    throw_vm_type_error(global_object, None)
}

/// `jsMakeNontrivialString(globalObject, "function "_s, name, "() { [native code] }"_s)`, codificado.
fn native_code_string(global_object: &JSGlobalObject, name: &dyn crate::wtf::text::string_concatenate::StringTypeAdapter) -> EncodedJSValue {
    match js_make_nontrivial_string(global_object, &[&"function ", name, &"() { [native code] }"]) {
        Some(string) => JSValue::from_js_string(string).encode(),
        None => JSValue::empty().encode(),
    }
}

/// `functionProtoFuncBind`. LACUNA: ver o cabeçalho do módulo (`JSBoundFunction`).
fn function_proto_func_bind(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let global_object: &JSGlobalObject = global_object;
    let vm = global_object.vm();

    let this_value = call_frame.this_value();
    if !this_value.is_callable() {
        return throw_vm_type_error(global_object, Some("|this| is not a function inside Function.prototype.bind"));
    }
    let target = this_value.as_object();

    let argument_count = call_frame.argument_count();
    let (bound_this, bound_args, num_bound_args) = if argument_count > 1 {
        (call_frame.unchecked_argument(0), call_frame.arguments_span().split_off(1), argument_count - 1)
    } else {
        (call_frame.argument(0), Vec::new(), 0)
    };

    let (length, name) = if this_value.as_js_function().is_some_and(|function| function.can_assume_name_and_length_are_original()) {
        // Do nothing! 'length' and 'name' computation are lazily done.
        // And this is totally OK since we know that wrapped functions have canAssumeNameAndLengthAreOriginal condition
        // at the time of creation of JSBoundFunction.
        (f64::NAN, None) // Defer computation.
    } else {
        let mut length = 0.0_f64;
        let length_name = PropertyName::from_identifier(&vm.property_names.length);
        // `target->hasProperty` pelo `methodTable()`: um `Proxy` responde pelo trap `getOwnPropertyDescriptor`.
        let mut slot = crate::runtime::property_slot::PropertySlot::new(this_value, crate::runtime::property_slot::InternalMethodType::GetOwnProperty);
        let found = match crate::runtime::proxy_object::own_property_slot(global_object, this_value, &length_name, &mut slot) {
            Ok(found) => found,
            Err(thrown) => {
                throw_thrown(global_object, thrown);
                return JSValue::empty().encode();
            }
        };
        if found {
            let length_value = target.get(global_object, &length_name);
            if vm.exception().is_some() {
                return JSValue::empty().encode();
            }
            if length_value.is_number() {
                length = length_value.to_integer_or_infinity();
                if length > num_bound_args as f64 {
                    length -= num_bound_args as f64;
                } else {
                    length = 0.0;
                }
            }
        }
        let name_value = target.get(global_object, &PropertyName::from_identifier(&vm.property_names.name));
        if vm.exception().is_some() {
            return JSValue::empty().encode();
        }
        (length, Some(if name_value.is_string() { name_value.as_js_string() } else { js_empty_string(vm) }))
    };

    match JSBoundFunction::create(vm, global_object, this_value, bound_this, &bound_args, length, name) {
        Some(function) => function.as_value().encode(),
        None => JSValue::empty().encode(),
    }
}

/// `functionProtoFuncSymbolHasInstance`.
fn function_proto_func_symbol_has_instance(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let global_object: &JSGlobalObject = global_object;
    let vm = global_object.vm();

    let this_value = call_frame.this_value();

    // 1. If IsCallable(constructor) is false, return false.
    if !this_value.is_callable() {
        return js_boolean(false).encode();
    }

    let instance = call_frame.argument(0);

    // 2. If constructor has a [[BoundTargetFunction]] internal slot, then
    if let Some(bound_function) = this_value.as_js_function().filter(|function| function.as_bound_function().is_some()) {
        // 2.a. Let boundConstructor be constructor.[[BoundTargetFunction]].
        // 2.b. Return ? InstanceofOperator(instance, boundConstructor).
        let bound = bound_function.as_bound_function().expect("filtrado por as_bound_function");
        return match object_has_instance(global_object, bound.target_function(), instance) {
            Some(result) => js_boolean(result).encode(),
            None => JSValue::empty().encode(),
        };
    }

    // 3. If instance is not an Object, return false.
    if !instance.is_object() {
        return js_boolean(false).encode();
    }

    let this_object = this_value.as_object();
    let prototype = this_object.get(global_object, &PropertyName::from_identifier(&vm.property_names.prototype));
    if vm.exception().is_some() {
        return JSValue::empty().encode();
    }

    match default_has_instance(global_object, instance, prototype) {
        Some(result) => js_boolean(result).encode(),
        None => JSValue::empty().encode(),
    }
}

/// https://github.com/claudepache/es-legacy-function-reflection/blob/master/spec.md#isallowedreceiverfunctionforcallerandargumentsfunc-expectedrealm (except step 3)
fn is_allowed_receiver_function_for_caller_and_arguments(function: &JSFunction) -> bool {
    if function.is_host_or_builtin_function() {
        return false;
    }

    let executable = function.js_executable();
    let executable = executable.borrow();
    if executable.implementation_visibility() != ImplementationVisibility::Public {
        return false;
    }
    !executable.is_in_strict_context()
        && executable.parse_mode() == SourceParseMode::NormalFunctionMode
        && !executable.is_class_constructor_function()
}

/// O `vm.topCallFrame` (0 é nulo).
fn top_call_frame(global_object: &JSGlobalObject) -> Option<CallFrame> {
    Some(global_object.vm().top_call_frame()).filter(|&call_frame| call_frame != 0).map(CallFrame::create)
}

/// `retrieveArguments(vm, callFrame, functionObj)` com o `RetrieveArgumentsFunctor`: o `arguments` do
/// frame mais interno cujo `callee` é `function`, ou `null`.
fn retrieve_arguments(global_object: &JSGlobalObject, function: &JSFunction) -> HostResult {
    let interpreter = global_object.vm().interpreter();
    let mut result = Ok(js_null());
    StackVisitor::visit(&interpreter, top_call_frame(global_object), false, |frame| {
        if !frame.callee().is_cell() || frame.callee().as_cell() != function.cell_id() {
            return IterationStatus::Continue;
        }

        result = frame.create_arguments(&interpreter).map(|arguments| arguments.as_value()).map_err(Thrown::from);
        IterationStatus::Done
    });
    result
}

/// `argumentsGetter`: https://github.com/claudepache/es-legacy-function-reflection/blob/master/spec.md#get-functionprototypearguments
fn function_proto_arguments(global_object: &JSGlobalObject, this_value: JSValue, _property_name: &PropertyName) -> HostResult {
    match this_value.as_js_function().filter(|function| is_allowed_receiver_function_for_caller_and_arguments(function)) {
        Some(function) => retrieve_arguments(global_object, &function),
        None => Err(Thrown::type_error(RESTRICTED_PROPERTY_ACCESS_ERROR)),
    }
}
custom_getter!(arguments_getter, function_proto_arguments);

/// `retrieveCallerFunction(vm, callFrame, functionObj)` com o `RetrieveCallerFunctionFunctor`: o `callee` do
/// frame logo acima do mais interno cujo `callee` é `function`, pulando função ligada, `JSRemoteFunction`,
/// `Proxy` e função de
/// implementação privada; `null` se não há.
fn retrieve_caller_function(global_object: &JSGlobalObject, function: &JSFunction) -> JSValue {
    let interpreter = global_object.vm().interpreter();
    let mut has_found_frame = false;
    let mut has_skipped_to_caller_frame = false;
    let mut result = js_null();
    StackVisitor::visit(&interpreter, top_call_frame(global_object), false, |frame| {
        if !frame.callee().is_cell() {
            return IterationStatus::Continue;
        }

        let callee = frame.callee().as_cell();
        if !has_found_frame && callee != function.cell_id() {
            return IterationStatus::Continue;
        }

        has_found_frame = true;
        if !has_skipped_to_caller_frame {
            has_skipped_to_caller_frame = true;
            return IterationStatus::Continue;
        }

        if callee != 0 {
            let callee_value = JSValue::from_cell(callee);
            let callee_function = callee_value.as_js_function();
            if callee_function.as_ref().is_some_and(|function| function.as_bound_function().is_some() || function.is_remote_function())
                || JSObject::from_cell_id(callee).is_some_and(|object| object.type_() == JSType::ProxyObjectType)
            {
                return IterationStatus::Continue;
            }
            if callee_function.is_some_and(|function| function.executable().implementation_visibility() != ImplementationVisibility::Public) {
                return IterationStatus::Continue;
            }

            result = callee_value;
        }
        IterationStatus::Done
    });
    result
}

/// `callerGetter`: https://github.com/claudepache/es-legacy-function-reflection/blob/master/spec.md#get-functionprototypecaller
fn function_proto_caller(global_object: &JSGlobalObject, this_value: JSValue, _property_name: &PropertyName) -> HostResult {
    let Some(function) = this_value.as_js_function().filter(|function| is_allowed_receiver_function_for_caller_and_arguments(function))
    else {
        return Err(Thrown::type_error(RESTRICTED_PROPERTY_ACCESS_ERROR));
    };

    let caller = retrieve_caller_function(global_object, &function);
    if caller.is_null() {
        return Ok(js_null());
    }

    // 11. If caller is not an ECMAScript function object, return null.
    let Some(caller_function) = caller.as_js_function().filter(|function| !function.is_host_or_builtin_function()) else {
        return Ok(js_null());
    };

    let executable = caller_function.js_executable();
    let executable = executable.borrow();
    // 12. If caller.[[Strict]] is true, return null.
    if executable.is_in_strict_context() {
        return Ok(js_null());
    }

    // Prevent bodies (private implementations) of generator / async functions from being exposed.
    // They expect to be called by @generatorResume() & friends with certain arguments, and crash otherwise.
    // 14. If caller.[[ECMAScriptCode]] is a GeneratorBody, an AsyncFunctionBody, an AsyncGeneratorBody, or an AsyncConciseBody, return null.
    let parse_mode = executable.parse_mode();
    if is_generator_parse_mode(parse_mode) || is_async_function_parse_mode(parse_mode) {
        return Ok(js_null());
    }

    Ok(caller)
}
custom_getter!(caller_getter, function_proto_caller);

/// `callerAndArgumentsSetter`: lança o `TypeError` para o receptor que não é permitido e devolve `true`
/// em qualquer caso (o `bool` do C++ não depende do lançamento, por isso não é o `custom_setter!`).
fn caller_and_arguments_setter(
    global_object: &JSGlobalObject,
    this_value: EncodedJSValue,
    _value: EncodedJSValue,
    _property_name: &PropertyName,
) -> bool {
    let _realm = CurrentRealmScope::enter(global_object);
    let allowed = JSValue::decode(this_value)
        .as_js_function()
        .is_some_and(|function| is_allowed_receiver_function_for_caller_and_arguments(&function));
    if !allowed {
        throw_thrown(global_object, Thrown::type_error(RESTRICTED_PROPERTY_ACCESS_ERROR));
    }
    true
}

/// `class FunctionPrototype : public InternalFunction`: sem campos próprios, é o `InternalFunction`.
pub struct FunctionPrototype;

impl FunctionPrototype {
    /// `createStructure(vm, globalObject, prototype)` (`FunctionPrototypeInlines.h`): `StructureFlags` é o
    /// do `InternalFunction`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::InternalFunctionType, InternalFunction::STRUCTURE_FLAGS),
            &FUNCTION_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, structure)`: `FunctionPrototype(vm, structure)` (`InternalFunction(vm, structure,
    /// callFunctionPrototype, nullptr)`) e `finishCreation(vm, String())`.
    pub fn create(vm: &VM, structure: StructureRef) -> InternalFunctionRef {
        let prototype = InternalFunction::new(vm, structure, call_function_prototype, None);
        prototype.finish_creation(vm, 0, &WtfString::default(), PropertyAdditionMode::WithoutStructureTransition);
        prototype
    }

    /// `addFunctionProperties(vm, globalObject, &callFunction, &applyFunction, &hasInstanceSymbolFunction)`,
    /// na parte que existe (ver as LACUNAS do cabeçalho): devolve o `hasInstanceSymbolFunction`.
    pub fn add_function_properties(
        function_prototype: &InternalFunction,
        vm: &VM,
        global_object: &JSGlobalObject,
    ) -> JSFunctionRef {
        put_direct_native_function_without_transition(
            vm,
            global_object,
            function_prototype,
            &vm.property_names.builtin_names().to_string_public_name(),
            0,
            function_proto_func_to_string,
            ImplementationVisibility::Public,
            Intrinsic::FunctionToStringIntrinsic,
            DONT_ENUM,
        );
        put_direct_builtin_function_without_transition(
            vm,
            global_object,
            function_prototype,
            vm.property_names.builtin_names().apply_public_name(),
            BuiltinCodeIndex::FunctionPrototypeApplyCode,
            DONT_ENUM,
        );
        put_direct_builtin_function_without_transition(
            vm,
            global_object,
            function_prototype,
            vm.property_names.builtin_names().call_public_name(),
            BuiltinCodeIndex::FunctionPrototypeCallCode,
            DONT_ENUM,
        );
        put_direct_native_function_without_transition(
            vm,
            global_object,
            function_prototype,
            &vm.property_names.bind,
            1,
            function_proto_func_bind,
            ImplementationVisibility::Public,
            Intrinsic::FunctionBindIntrinsic,
            DONT_ENUM,
        );

        for (name, getter) in [(&vm.property_names.arguments, arguments_getter as crate::runtime::property_slot::GetValueFunc), (&vm.property_names.caller, caller_getter as crate::runtime::property_slot::GetValueFunc)] {
            function_prototype.put_direct_custom_getter_setter_without_transition(
                vm,
                &PropertyName::from_identifier(name),
                &CustomGetterSetter::create(vm, getter, Some(caller_and_arguments_setter)),
                DONT_ENUM | CUSTOM_ACCESSOR,
            );
        }

        let has_instance_symbol_function = JSFunction::create_native(
            vm,
            global_object,
            1,
            &WtfString::from_latin1(b"[Symbol.hasInstance]"),
            function_proto_func_symbol_has_instance,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            call_host_function_as_constructor,
        );
        function_prototype.put_direct(
            vm,
            &PropertyName::from_identifier(&vm.property_names.has_instance_symbol),
            has_instance_symbol_function.as_value(),
            DONT_DELETE | READ_ONLY | DONT_ENUM,
        );
        has_instance_symbol_function
    }
}
