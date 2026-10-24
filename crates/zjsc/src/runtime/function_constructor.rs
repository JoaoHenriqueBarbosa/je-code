//! Tradução de `runtime/FunctionConstructor.cpp`: `constructFunction` (o `new Function(...)`, e os
//! construtores de `GeneratorFunction`, `AsyncFunction` e `AsyncGeneratorFunction`) e as funções nativas
//! de `call` e `construct` dos quatro construtores (`FunctionConstructor.cpp`,
//! `GeneratorFunctionConstructor.cpp`, `AsyncFunctionConstructor.cpp` e
//! `AsyncGeneratorFunctionConstructor.cpp`, que só diferem no modo e no nome).
//!
//! DIVERGÊNCIAS:
//!
//! - As classes `FunctionConstructor`, `GeneratorFunctionConstructor`, `AsyncFunctionConstructor` e
//!   `AsyncGeneratorFunctionConstructor` são um só `InternalFunction` parametrizado pelo
//!   `FunctionConstructionMode` (`create_function_construction_constructor`), porque os quatro `.cpp` são o
//!   mesmo código com outro modo e outro nome.
//! - `ArgList` é `&[JSValue]`. A sobrecarga com `CallFrame*` do C++ vira o chamador passando
//!   `source_origin` (de `HostCall::caller_source_origin`) e `tainted_origin` (`Untainted`, pois
//!   `computeNewSourceTaintedOriginFromStack` não existe) a [`construct_function`].
//! - `JSValue::toWTFString` lança pelo `VM` (devolve vazio com a exceção pendente): `stringify_function`
//!   confere a exceção depois de cada conversão e para na primeira, como o `RETURN_IF_EXCEPTION`.
//! - Os blocos de `trustedTypesEnforcement` e `evalEnabled` (`reportViolationForUnsafeEval`,
//!   `canCompileStrings`, `EvalError`) não existem: o `JSGlobalObject` do porte não tem `evalEnabled`
//!   nem a `GlobalObjectMethodTable` do embedder, então `new Function` está sempre habilitado.
//! - `getFunctionRealm(newTarget)` vem de `internal_function::get_function_realm`.
//! - O corpo e os parâmetros se juntam num só `StringBuilder` para os casos de 0, 1, 2 ou mais
//!   argumentos; o texto resultante e `functionConstructorParametersEndPosition` são os mesmos que os
//!   quatro ramos do `stringifyFunction` produzem (o C++ separa por desempenho).

use std::rc::Rc;

use crate::host_function;
use crate::llint::slow_paths::thrown_failure;
use crate::llint::{LLIntFailure, LLIntResult};
use crate::parser::parser_modes::{
    FunctionConstructionMode, LexicallyScopedFeatures, NO_LEXICALLY_SCOPED_FEATURES,
    TAINTED_BY_WITH_SCOPE_LEXICALLY_SCOPED_FEATURE,
};
use crate::parser::source_tainted_origin::SourceTaintedOrigin;
use crate::runtime::exception_helpers::throw_out_of_memory_error;
use crate::runtime::function_executable::{FunctionExecutable, OVERRIDE_LINE_NUMBER_NOT_FOUND};
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::identifier::Identifier;
use crate::runtime::internal_function::{get_function_realm, InternalFunction, InternalFunctionRef, PropertyAdditionMode, INTERNAL_FUNCTION_S_INFO};
use crate::runtime::iterator_operations::thrown_from_llint;
use crate::runtime::js_async_function::JSAsyncFunction;
use crate::runtime::js_async_generator_function::JSAsyncGeneratorFunction;
use crate::runtime::js_function::JSFunction;
use crate::runtime::js_generator_function::JSGeneratorFunction;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSObject, JSObjectHandle};
use crate::runtime::js_value::JSValue;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::source_origin::SourceOrigin;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::string_regexp_support::to_wtf_string_value;
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::runtime::vm::VM;
use crate::wtf::text::string_builder::{OverflowPolicy, StringBuilder};
use crate::wtf::text::text_position::TextPosition;
use crate::wtf::text::wtf_string::String as WtfString;

/// `functionConstructorPrefix(FunctionConstructionMode)`.
pub fn function_constructor_prefix(function_construction_mode: FunctionConstructionMode) -> &'static str {
    match function_construction_mode {
        FunctionConstructionMode::Function => "function ",
        FunctionConstructionMode::Generator => "function* ",
        FunctionConstructionMode::Async => "async function ",
        FunctionConstructionMode::AsyncGenerator => "async function* ",
    }
}

/// Por que `stringify_function` não produziu o programa.
enum StringifyFailure {
    /// Uma conversão (`toString`) lançou: a exceção já está pendente no `VM` (`RETURN_IF_EXCEPTION`).
    Pending,
    /// Estouro do `StringBuilder`: o `throwOutOfMemoryError` do C++, que quem chama lança.
    OutOfMemory,
}

/// `stringifyFunction(...)`: o texto do programa e o `functionConstructorParametersEndPosition`. Cada
/// argumento é convertido por `toString` na ordem em que aparece (parâmetros e depois o corpo), e a
/// primeira exceção interrompe as conversões seguintes, como o `RETURN_IF_EXCEPTION` do C++.
///
/// How we stringify functions is sometimes important for web compatibility.
/// See https://bugs.webkit.org/show_bug.cgi?id=24350.
fn stringify_function(
    global_object: &JSGlobalObject,
    args: &[JSValue],
    function_name: &Identifier,
    function_construction_mode: FunctionConstructionMode,
) -> Result<(WtfString, Option<i32>), StringifyFailure> {
    let mut builder = StringBuilder::with_overflow_policy(OverflowPolicy::RecordOverflow);
    builder.append_ascii_literal(function_constructor_prefix(function_construction_mode));
    builder.append_string(function_name.string().string());
    builder.append_ascii_literal("(");

    // Todos os argumentos menos o último são parâmetros; o último é o corpo (nenhum argumento: corpo vazio).
    let parameter_count = args.len().saturating_sub(1);
    for (index, arg) in args.iter().take(parameter_count).enumerate() {
        let text = to_wtf_string_value(global_object, *arg).map_err(|_| StringifyFailure::Pending)?;
        if index != 0 {
            builder.append_ascii_literal(",");
        }
        builder.append_string(&text);
    }
    if builder.has_overflowed() {
        return Err(StringifyFailure::OutOfMemory);
    }

    // Só com parâmetros e corpo (dois ou mais argumentos) há posição de fim dos parâmetros.
    let parameters_end_position = if args.len() >= 2 { Some((builder.length() + "\n)".len() as u32) as i32) } else { None };

    let body = match args.last() {
        Some(body) => Some(to_wtf_string_value(global_object, *body).map_err(|_| StringifyFailure::Pending)?),
        None => None,
    };
    builder.append_ascii_literal("\n) {\n");
    if let Some(body) = &body {
        builder.append_string(body);
    }
    builder.append_ascii_literal("\n}");
    if builder.has_overflowed() {
        return Err(StringifyFailure::OutOfMemory);
    }
    Ok((builder.to_string().clone(), parameters_end_position))
}

/// `constructFunction(globalObject, args, functionName, sourceOrigin, sourceURL, taintedOrigin, position,
/// functionConstructionMode, newTarget)`, ECMA 15.3.2 The Function Constructor.
///
/// Devolve a função criada, ou `Err(LLIntFailure::Thrown)` com a exceção pendente no `VM`.
#[allow(clippy::too_many_arguments)]
pub fn construct_function(
    global_object: &JSGlobalObject,
    args: &[JSValue],
    function_name: &Identifier,
    source_origin: &SourceOrigin,
    source_url: &WtfString,
    tainted_origin: SourceTaintedOrigin,
    position: &TextPosition,
    function_construction_mode: FunctionConstructionMode,
    new_target: Option<JSValue>,
) -> LLIntResult<JSObjectHandle> {
    let vm = global_object.vm();
    let (code, function_constructor_parameters_end_position) = match stringify_function(global_object, args, function_name, function_construction_mode) {
        Ok(program) => program,
        Err(StringifyFailure::Pending) => return Err(LLIntFailure::Thrown),
        Err(StringifyFailure::OutOfMemory) => {
            let mut scope = ThrowScope::new(vm);
            throw_out_of_memory_error(global_object, &mut scope);
            return Err(LLIntFailure::Thrown);
        }
    };

    let code = match crate::runtime::dynamic_code_guard::guard_function_source(global_object, code) {
        Ok(code) => code,
        Err(()) => return Err(LLIntFailure::Thrown),
    };
    let lexically_scoped_features: LexicallyScopedFeatures = if global_object.global_scope_extension().is_some() {
        TAINTED_BY_WITH_SCOPE_LEXICALLY_SCOPED_FEATURE
    } else {
        NO_LEXICALLY_SCOPED_FEATURES
    };
    construct_function_skipping_eval_enabled_check(
        global_object,
        code,
        lexically_scoped_features,
        function_name,
        source_origin,
        source_url,
        tainted_origin,
        position,
        OVERRIDE_LINE_NUMBER_NOT_FOUND,
        function_constructor_parameters_end_position,
        function_construction_mode,
        new_target,
    )
}

/// `constructFunction(globalObject, callFrame, args, functionConstructionMode, newTarget)`: o `anonymous`
/// do `vm.propertyNames`, sem URL e na posição inicial; a origem e a origem contaminada vêm do chamador
/// (`callFrame->callerSourceOrigin(vm)` e `computeNewSourceTaintedOriginFromStack`).
pub fn construct_function_for_caller(
    global_object: &JSGlobalObject,
    args: &[JSValue],
    caller_source_origin: &SourceOrigin,
    tainted_origin: SourceTaintedOrigin,
    function_construction_mode: FunctionConstructionMode,
    new_target: Option<JSValue>,
) -> LLIntResult<JSObjectHandle> {
    construct_function(
        global_object,
        args,
        &global_object.vm().property_names().anonymous,
        caller_source_origin,
        // Medido no bun 1.4.2: o frame de `new Function` mostra `anonymous (file:///arq.js:3:17)`, a URL da origem do
        // chamador, como o `eval` (`execute_eval.rs`); com a URL vazia o frame saía só `anonymous`.
        &caller_source_origin.string().clone(),
        tainted_origin,
        &TextPosition::default(),
        function_construction_mode,
        new_target,
    )
}

/// `constructFunctionSkippingEvalEnabledCheck(...)`: o `FunctionExecutable::fromGlobalCode` e a função
/// sobre ele, no escopo global.
#[allow(clippy::too_many_arguments)]
pub fn construct_function_skipping_eval_enabled_check(
    global_object: &JSGlobalObject,
    program: WtfString,
    lexically_scoped_features: LexicallyScopedFeatures,
    function_name: &Identifier,
    source_origin: &SourceOrigin,
    source_url: &WtfString,
    tainted_origin: SourceTaintedOrigin,
    position: &TextPosition,
    override_line_number: i32,
    function_constructor_parameters_end_position: Option<i32>,
    function_construction_mode: FunctionConstructionMode,
    new_target: Option<JSValue>,
) -> LLIntResult<JSObjectHandle> {
    let vm = global_object.vm();

    let mut exception: Option<JSObjectHandle> = None;
    let function: Option<Rc<std::cell::RefCell<FunctionExecutable>>> = FunctionExecutable::from_global_code(
        function_name,
        global_object,
        program,
        source_origin,
        tainted_origin,
        source_url,
        position,
        lexically_scoped_features,
        &mut exception,
        override_line_number,
        function_constructor_parameters_end_position,
        function_construction_mode,
    );
    let Some(function) = function else {
        let mut scope = ThrowScope::new(vm);
        throw_exception(global_object, &mut scope, exception.expect("fromGlobalCode nulo sem exceção"));
        return Err(LLIntFailure::Thrown);
    };

    // `needsSubclassStructure = newTarget && newTarget != globalObject->functionConstructor()`: o
    // `structureGlobalObject` é o `getFunctionRealm(newTarget)` e a estrutura sai dele.
    let needs_subclass_structure = new_target
        .filter(|new_target| *new_target != JSValue::from_cell(global_object.function_constructor().cell_id()));
    let realm_holder = match needs_subclass_structure {
        Some(new_target) => Some(get_function_realm(new_target).map_err(|thrown| thrown_failure(global_object, thrown))?),
        None => None,
    };
    let structure_global_object: &JSGlobalObject = realm_holder.as_deref().unwrap_or(global_object);
    let mut structure = match function_construction_mode {
        FunctionConstructionMode::Function => JSFunction::select_structure_for_new_func_exp(structure_global_object, &function),
        FunctionConstructionMode::Generator => structure_global_object.generator_function_structure(),
        FunctionConstructionMode::Async => structure_global_object.async_function_structure(),
        FunctionConstructionMode::AsyncGenerator => structure_global_object.async_generator_function_structure(),
    };

    if let Some(new_target) = needs_subclass_structure {
        let new_target = crate::runtime::host_function_support::ObjectRef::from_value(&new_target).expect("asObject(newTarget)");
        structure = InternalFunction::create_subclass_structure(global_object, &new_target, structure)
            .map_err(|thrown| thrown_failure(global_object, thrown))?;
    }

    let scope = global_object.global_scope();
    let created = match function_construction_mode {
        FunctionConstructionMode::Function => JSFunction::create_with_structure(vm, global_object, &function, scope, structure),
        FunctionConstructionMode::Generator => JSGeneratorFunction::create_with_structure(vm, global_object, &function, scope, structure),
        FunctionConstructionMode::Async => JSAsyncFunction::create_with_structure(vm, global_object, &function, scope, structure),
        FunctionConstructionMode::AsyncGenerator => {
            JSAsyncGeneratorFunction::create_with_structure(vm, global_object, &function, scope, structure)
        }
    };
    Ok(JSObject::from_cell_id(created.cell_id()).expect("função recém criada fora do registro de células"))
}

/// O corpo de `callFunctionConstructor`, `constructWithFunctionConstructor` e dos pares equivalentes de
/// `GeneratorFunction`, `AsyncFunction` e `AsyncGeneratorFunction`: `constructFunction(globalObject,
/// callFrame, args, mode, newTarget)`. A chamada sem `new` passa `None`; `new` passa o `new.target`.
///
/// DIVERGÊNCIA: `computeNewSourceTaintedOriginFromStack` não existe (`call_frame.rs`): a contaminada é
/// `Untainted`, como no `eval` indireto (`js_global_object_functions_natives.rs`). A origem vem de
/// `HostCall::caller_source_origin`.
fn construct_function_host(
    global_object: &JSGlobalObject,
    call: &HostCall,
    function_construction_mode: FunctionConstructionMode,
    new_target: Option<JSValue>,
) -> HostResult {
    let function = construct_function_for_caller(
        global_object,
        call.arguments(),
        &call.caller_source_origin(global_object),
        SourceTaintedOrigin::Untainted,
        function_construction_mode,
        new_target,
    )
    .map_err(thrown_from_llint)?;
    // Formato de pilha do V8: os frames da função criada mostram de onde ela veio (`eval at f (url:L:C)`).
    if crate::runtime::stack_frame::v8_stack_format() {
        if let (Some(frame), Some(created)) = (call.native_frame(), function.as_value().as_js_function()) {
            if !created.is_host_function() {
                if let Some(provider) = created.js_executable().borrow().source().provider() {
                    provider.set_eval_origin(crate::runtime::v8_stack::eval_origin(global_object, frame));
                }
            }
        }
    }
    Ok(function.as_value())
}

/// As duas funções nativas de um construtor de função: o `call` (`new.target` ausente) e o `construct`.
macro_rules! function_construction_host_functions {
    ($mode:expr, $call_body:ident, $construct_body:ident, $call_host:ident, $construct_host:ident) => {
        fn $call_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
            construct_function_host(global_object, call, $mode, None)
        }

        fn $construct_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
            construct_function_host(global_object, call, $mode, Some(call.new_target()))
        }

        host_function!($call_host, $call_body);
        host_function!($construct_host, $construct_body);
    };
}

// `callFunctionConstructor` e `constructWithFunctionConstructor`.
function_construction_host_functions!(
    FunctionConstructionMode::Function,
    call_function_constructor_body,
    construct_with_function_constructor_body,
    call_function_constructor,
    construct_with_function_constructor
);
// `callGeneratorFunctionConstructor` e `constructGeneratorFunctionConstructor`.
function_construction_host_functions!(
    FunctionConstructionMode::Generator,
    call_generator_function_constructor_body,
    construct_generator_function_constructor_body,
    call_generator_function_constructor,
    construct_generator_function_constructor
);
// `callAsyncFunctionConstructor` e `constructAsyncFunctionConstructor`.
function_construction_host_functions!(
    FunctionConstructionMode::Async,
    call_async_function_constructor_body,
    construct_async_function_constructor_body,
    call_async_function_constructor,
    construct_async_function_constructor
);
// `callAsyncGeneratorFunctionConstructor` e `constructAsyncGeneratorFunctionConstructor`.
function_construction_host_functions!(
    FunctionConstructionMode::AsyncGenerator,
    call_async_generator_function_constructor_body,
    construct_async_generator_function_constructor_body,
    call_async_generator_function_constructor,
    construct_async_generator_function_constructor
);

/// `const ClassInfo FunctionConstructor::s_info` (`"Function"`, base `InternalFunction`): é o que o
/// `isWellKnownConstructor` do `deepEquals` reconhece; os construtores de generator, async e async generator têm o
/// `InternalFunction::s_info` dos irmãos.
pub static FUNCTION_CONSTRUCTOR_S_INFO: ClassInfo = ClassInfo {
    class_name: "Function",
    parent_class: Some(&INTERNAL_FUNCTION_S_INFO),
    static_prop_hash_table: None,
    inherits_js_type_range: None,
};

/// `FunctionConstructor::createStructure(vm, globalObject, prototype)`.
pub fn create_function_constructor_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
    Structure::create(
        vm,
        Some(global_object),
        prototype,
        TypeInfo::new(JSType::InternalFunctionType, InternalFunction::STRUCTURE_FLAGS),
        &FUNCTION_CONSTRUCTOR_S_INFO,
    )
}

/// `FunctionConstructor::create(vm, structure, functionPrototype)` e os `create` de
/// `GeneratorFunctionConstructor`, `AsyncFunctionConstructor` e `AsyncGeneratorFunctionConstructor`: o
/// `InternalFunction` com o `call` e o `construct` do modo, `finishCreation(vm, 1, name,
/// PropertyAdditionMode::WithoutStructureTransition)` e `prototype` (`DontEnum|DontDelete|ReadOnly`).
/// `structure` é a de `createStructure(vm, globalObject, prototype)` (`InternalFunction::create_structure`),
/// cujo protótipo é o `Function.prototype` (o `Function`) ou o próprio construtor `Function` (os demais).
pub fn create_function_construction_constructor(
    vm: &VM,
    structure: StructureRef,
    function_construction_mode: FunctionConstructionMode,
    prototype: &JSObject,
) -> InternalFunctionRef {
    let (name, call, construct): (&[u8], NativeFunction, NativeFunction) = match function_construction_mode {
        FunctionConstructionMode::Function => (b"Function", call_function_constructor, construct_with_function_constructor),
        FunctionConstructionMode::Generator => {
            (b"GeneratorFunction", call_generator_function_constructor, construct_generator_function_constructor)
        }
        FunctionConstructionMode::Async => (b"AsyncFunction", call_async_function_constructor, construct_async_function_constructor),
        FunctionConstructionMode::AsyncGenerator => {
            (b"AsyncGeneratorFunction", call_async_generator_function_constructor, construct_async_generator_function_constructor)
        }
    };
    let constructor = InternalFunction::new(vm, structure, call, Some(construct));
    constructor.finish_creation(vm, 1, &WtfString::from_latin1(name), PropertyAdditionMode::WithoutStructureTransition);
    constructor.put_direct(
        vm,
        &PropertyName::from_identifier(&vm.property_names.prototype),
        prototype.as_value(),
        DONT_ENUM | DONT_DELETE | READ_ONLY,
    );
    constructor
}
