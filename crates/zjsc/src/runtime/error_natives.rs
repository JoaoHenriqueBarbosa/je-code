//! As cascas finas e a criação dos objetos de `ErrorConstructor.cpp`, `ErrorPrototype.cpp`,
//! `NativeErrorConstructor.cpp` e `NativeErrorPrototype.cpp`: `Error` e os seis construtores nativos
//! (`EvalError`, `RangeError`, `ReferenceError`, `SyntaxError`, `TypeError`, `URIError`), o `Error.prototype`
//! com `toString`, os protótipos nativos e as `ErrorInstance` `Structure` por tipo que o global guarda.
//! Os dados e algoritmos puros estão em `error_constructor.rs`, `error_prototype.rs`,
//! `native_error_constructor.rs` e `native_error_prototype.rs`.
//!
//! LACUNAS, e por quê:
//! - A pilha (`stack`) é materializada na primeira leitura (`ErrorInstance::materialize_stack`, chamada do
//!   `getOwnPropertySlot`): o texto é o
//!   do `Bun` (cabeçalho e linhas `    at`, ver `stack_frame.rs`) e o cabeçalho usa o `name`/`message` do
//!   tipo, não os que o programa atribuir depois. `Error.prepareStackTrace(error, callSites)` roda nessa
//!   materialização, com `CallSite` de `js_call_site.rs` (em `captureStackTrace` roda na chamada, sem
//!   medição contra o `Bun`). `ErrorConstructor::put`/`deleteProperty` do `stackTraceLimit` são os ganchos
//!   [`error_constructor_put`] e [`error_constructor_delete_property`] (chamados por `JSObject::put` e
//!   `delete_property`), que mantêm `JSGlobalObject::stack_trace_limit`; a captura lê esse espelho.
//! - `cause` de `options` entra como propriedade `DontEnum`. `AggregateError` e `SuppressedError` estão em
//!   `aggregate_error.rs` e `suppressed_error.rs` e capturam a pilha pelo quadro nativo (`HostCall`).
//!
//! DIVERGÊNCIAS:
//! - `isErrorSubclass` (`newTarget != callee`) vira o `caller` da captura: os frames até o construtor da
//!   subclasse ficam de fora.
//! - O `ErrorInstance` do porte guarda a mensagem em `ErrorData`; a propriedade `message` (`DontEnum`) que o
//!   `ErrorInstance::finishCreation` grava é feita por [`put_message_property`] logo depois da criação.
//! - Os seis `NativeErrorConstructor<errorType>` são o mesmo código com o tipo como dado.

use crate::interpreter::call_frame::{CallFrame, NativeCallFrame};
use crate::llint::LLIntFailure;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::error::create_range_error;
use crate::runtime::error_constructor::{
    stack_trace_limit_from_value, CAPTURE_STACK_TRACE_LENGTH, CAPTURE_STACK_TRACE_NOT_OBJECT, FUNCTION_ATTRIBUTES,
    IS_ERROR_LENGTH, LENGTH,
};
use crate::runtime::error_instance::{stack_text, ErrorInstance, ErrorInstanceRef};
use crate::runtime::error_prototype::{error_prototype_name, error_to_string, initial_name_and_message};
use crate::runtime::error_type::{error_type_name, ErrorType};
use crate::runtime::exception_helpers::throw_out_of_memory_error;
use crate::runtime::host_call::throw_thrown;
use crate::runtime::host_function_support::{throw_error_object, throw_vm_type_error, ObjectRef};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::collection_support::create_native_collection_constructor;
use crate::runtime::internal_function::get_derived_structure_in_realm;
use crate::runtime::js_function::{put_direct_native_function_without_transition, JSFunction, JSFunctionRef, JS_FUNCTION_S_INFO};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_call_site::prepare_stack_trace;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::{js_empty_string, js_string};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE, OVERRIDES_PUT};
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::js_value::{js_boolean, js_number, EncodedJSValue, JSValue};
use crate::runtime::native_error_constructor::NATIVE_ERROR_TYPES;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::options_list::Options;
use crate::runtime::property_attribute::{DONT_ENUM, FUNCTION};
use crate::runtime::property_name::PropertyName;
use crate::wtf::text::conversion_mode::ConversionMode;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::stack_frame::StackFrame;
use crate::runtime::string_prototype::StringOpError;
use crate::runtime::string_regexp_support::to_wtf_string_value;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::throw_scope::ThrowScope;
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;
use std::rc::Rc;

/// `const ClassInfo NativeErrorPrototype::s_info`, `AggregateErrorPrototype::s_info` e o `ErrorPrototypeBase`
/// (`"Object"`, base `JSNonFinalObject`, sem tabela estática).
pub static ERROR_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Object", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `const ClassInfo ErrorPrototype::s_info` (`"Object"`, base `ErrorPrototypeBase`, `&errorPrototypeTable`).
pub static ERROR_PROTOTYPE_WITH_TABLE_S_INFO: ClassInfo = ClassInfo {
    class_name: "Object",
    parent_class: Some(&ERROR_PROTOTYPE_S_INFO),
    static_prop_hash_table: Some(&ERROR_PROTOTYPE_TABLE),
    inherits_js_type_range: None,
};

/// `errorPrototypeTableValues` de `ErrorPrototype.lut.h`: `toString` (`DontEnum|Function`, comprimento 0).
static ERROR_PROTOTYPE_TABLE_VALUES: [HashTableValue; 1] = [HashTableValue {
    key: "toString",
    attributes: DONT_ENUM | FUNCTION,
    intrinsic: Intrinsic::NoIntrinsic,
    kind: Kind::NativeFunction { function: error_proto_func_to_string, length: crate::runtime::error_prototype::TO_STRING_LENGTH as i32 },
}];

/// `errorPrototypeTable`.
static ERROR_PROTOTYPE_TABLE: HashTable = HashTable { class_for_this: None, values: &ERROR_PROTOTYPE_TABLE_VALUES };

/// `const ClassInfo ErrorConstructor::s_info` e `NativeErrorConstructor::s_info` (`"Function"`, base
/// `JSFunction`: no `bun` 1.4.2 os construtores de erro são `JSFunction` sobre `NativeExecutable`, com `length`
/// e `name` preguiçosos antes de `prototype` no `ownKeys`).
pub static ERROR_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&JS_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// A `Structure` de um construtor de erro: `JSFunctionType` com `prototype` como protótipo. `flags` leva o
/// `OverridesPut` do `ErrorConstructor` (os nativos e o `AggregateError` passam 0).
pub(crate) fn error_constructor_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue, flags: u32) -> StructureRef {
    Structure::create(
        vm,
        Some(global_object),
        prototype,
        TypeInfo::new(JSType::JSFunctionType, JSFunction::STRUCTURE_FLAGS | flags),
        &ERROR_CONSTRUCTOR_S_INFO,
    )
}

/// A criação do construtor de erro como `JSFunction` nativa (`length`, `name` e `prototype`) e a ligação do
/// `constructor` do protótipo (`DontEnum`). Compartilhada por `Error`, os nativos, `AggregateError`,
/// `SuppressedError` e os erros do WebAssembly.
pub(crate) fn create_error_constructor_function(
    vm: &VM,
    global_object: &JSGlobalObject,
    structure: StructureRef,
    prototype: &JSObject,
    name: &str,
    length: u32,
    call: NativeFunction,
    construct: NativeFunction,
) -> JSFunctionRef {
    let constructor = create_native_collection_constructor(vm, global_object, structure, prototype, name, length, call, construct, false);
    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
    constructor
}

/// A propriedade `message` (`DontEnum`) que `ErrorInstance::finishCreation` grava.
pub fn put_message_property(vm: &VM, instance: &ErrorInstance, message: &WtfString) {
    instance.put_direct(
        vm,
        &PropertyName::from_identifier(&vm.property_names.message),
        JSValue::from_js_string(js_string(vm, message)),
        DONT_ENUM,
    );
}

/// `getStackTrace(vm, obj, ...)` de `Error.cpp` (a captura do `ErrorInstance`): sem `stackTraceLimit`
/// (`std::nullopt`, o `Error.stackTraceLimit` não numérico ou apagado) devolve `None` e a instância não
/// guarda pilha; com ele, `Interpreter::getStackTrace(owner, results, framesToSkip, limit, caller)` a
/// partir do frame da função nativa que chama (o próprio frame nativo nunca entra).
pub(crate) fn capture_frames(
    global_object: &JSGlobalObject,
    call_frame: CallFrame,
    caller: Option<usize>,
    include_top_native: bool,
) -> Option<Vec<StackFrame>> {
    // Medido no `bun` 1.4.2: com `Error.stackTraceLimit` 0, negativo, `NaN`, não numérico ou apagado, `new Error`
    // e o erro lançado pelo motor não ganham a propriedade `stack` (`'stack' in e` é `false`). No V8 o 0 dá o
    // cabeçalho sozinho (`Error: zero`, medido no Chromium 154).
    let v8 = crate::runtime::stack_frame::v8_stack_format();
    let limit = global_object.stack_trace_limit.get().filter(|&limit| v8 || limit != 0)? as usize;
    Some(global_object.vm().interpreter().get_stack_trace(global_object.vm(), call_frame, limit, caller, include_top_native))
}

/// O limite de `Error.captureStackTrace`: no `bun`, o `stackTraceLimit` 0, negativo, `NaN`, não numérico ou
/// apagado cai no padrão (`Options::defaultErrorStackTraceLimit`, 10), e a pilha sai com frames.
fn capture_frames_for_capture_stack_trace(global_object: &JSGlobalObject, call_frame: CallFrame, caller: Option<usize>) -> Vec<StackFrame> {
    let limit = global_object.stack_trace_limit.get().filter(|&limit| limit != 0).unwrap_or_else(Options::default_error_stack_trace_limit);
    global_object.vm().interpreter().get_stack_trace(global_object.vm(), call_frame, limit as usize, caller, false)
}

/// `ErrorConstructor::put` antes do `Base::put`: `Error.stackTraceLimit = value` atualiza o espelho do
/// global (número vira `clamp(valor, 0, UINT_MAX)` truncado, qualquer outro valor o limpa). O gancho de
/// `JSObject::put`; só age no construtor `Error` do realm do objeto.
pub fn error_constructor_put(object: &JSObject, vm: &VM, property_name: &PropertyName, value: JSValue) {
    if *property_name != vm.property_names.stack_trace_limit {
        return;
    }
    if let Some(global_object) = error_constructor_realm(object) {
        let number = value.is_number().then(|| value.as_number());
        global_object.stack_trace_limit.set(stack_trace_limit_from_value(number));
    }
}

/// `ErrorConstructor::deleteProperty` antes do `Base::deleteProperty`: apagar `stackTraceLimit` limpa o
/// espelho do global (mesmo que o delete falhe, como no C++).
pub fn error_constructor_delete_property(object: &JSObject, vm: &VM, property_name: &PropertyName) {
    if *property_name != vm.property_names.stack_trace_limit {
        return;
    }
    if let Some(global_object) = error_constructor_realm(object) {
        global_object.stack_trace_limit.set(None);
    }
}

/// O realm de `object` quando ele é o construtor `Error` (`thisObject->globalObject()`).
fn error_constructor_realm(object: &JSObject) -> Option<Rc<JSGlobalObject>> {
    let realm = object.structure().realm()?;
    let constructor = (*realm.error_constructor.borrow())?;
    (constructor.is_cell() && constructor.as_cell() == object.as_value().as_cell()).then_some(realm)
}

/// `errorConstructorCaptureStackTrace`: grava em `object` a propriedade `stack` (`DontEnum`) com o
/// cabeçalho `name: message` do próprio objeto e os frames de quem chamou, a partir de `constructorOpt`;
/// com `Error.prepareStackTrace` definido, o valor que ele devolve para `(object, callSites)`.
fn error_constructor_capture_stack_trace(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let vm = global_object.vm();
    let Some(object) = ObjectRef::from_value(&call_frame.argument(0)) else {
        return throw_vm_type_error(global_object, Some(CAPTURE_STACK_TRACE_NOT_OBJECT));
    };
    let caller_argument = call_frame.argument(1);
    // Medido no `bun` 1.4.2: qualquer objeto vale como `constructorOpt` (`dyn_cast<JSObject*>`); um que não é
    // função nunca aparece na pilha, então todos os frames são descartados (`Error.captureStackTrace(o, {})`).
    // A função ligada por um `bind` casa com o frame do alvo dela; dois `bind` seguidos não casam com nada.
    let caller = caller_argument.is_object().then(|| {
        let target = caller_argument.as_js_function().and_then(|function| function.as_bound_function().map(|bound| bound.target_function()));
        target.filter(JSValue::is_cell).unwrap_or(caller_argument).as_cell()
    });
    if crate::runtime::stack_frame::v8_stack_format() {
        let frames = capture_frames(global_object, call_frame.call_frame(), caller, false);
        crate::runtime::v8_stack::capture_into(global_object, &object, frames);
        return JSValue::undefined().encode();
    }
    let frames = capture_frames_for_capture_stack_trace(global_object, call_frame.call_frame(), caller);

    // Medido no `bun` 1.4.2: num `ErrorInstance` o cabeçalho `name: message` é montado na leitura de `stack`
    // (preguiçoso, como na construção); num objeto qualquer o cabeçalho é sempre `Error`, sem `message`.
    if let Some(error) = ErrorInstance::from_cell_id(object.as_value().as_cell()) {
        error.replace_pending_stack(frames);
        return JSValue::undefined().encode();
    }
    let stack = match prepare_stack_trace(global_object, object.as_value(), &frames) {
        Err(LLIntFailure::Thrown) => return JSValue::empty().encode(),
        Err(unported) => panic!("caminho ainda não portado: {unported:?}"),
        Ok(Some(prepared)) => prepared,
        Ok(None) => JSValue::from_js_string(js_string(vm, &stack_text(&WtfString::from_utf8(b"Error"), &frames, None))),
    };
    // Medido no `bun`: `originalLine,originalColumn,stack` (sem `line`, `column`, `sourceURL`), todos `DontEnum`.
    if let Some(top) = frames.first().filter(|top| top.has_line_and_column_info()) {
        let names = &vm.property_names;
        for (name, value) in [(&names.original_line, top.line()), (&names.original_column, top.column())] {
            object.put_direct(vm, &PropertyName::from_identifier(name), js_number(f64::from(value)), DONT_ENUM);
        }
    }
    object.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.stack), stack, DONT_ENUM);
    JSValue::undefined().encode()
}

/// O `length` de `Error.appendStackTrace`.
const APPEND_STACK_TRACE_LENGTH: u32 = 2;

/// A mensagem de `Error.appendStackTrace` com argumento que não é `Error` (medido no bun).
const APPEND_STACK_TRACE_NOT_ERROR: &str = "First & second argument must be an Error object";

/// A mensagem do `Error.prepareStackTrace` padrão com primeiro argumento que não é `Error` (medido no bun).
const PREPARE_STACK_TRACE_NOT_ERROR: &str = "First argument must be an Error object";

/// O `length` do `Error.prepareStackTrace` padrão.
const PREPARE_STACK_TRACE_LENGTH: u32 = 2;

/// O nome do `Error.prepareStackTrace` padrão; `prepare_stack_trace_hook` o trata como gancho ausente.
pub const DEFAULT_PREPARE_STACK_TRACE_NAME: &str = "ErrorPrepareStackTrace";

/// `Error.prepareStackTrace` padrão do bun: valida o primeiro argumento; o formato do `stack` não passa por ele.
fn error_constructor_default_prepare_stack_trace(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let argument = call_frame.argument(0);
    if !(argument.is_cell() && ErrorInstance::from_cell_id(argument.as_cell()).is_some()) {
        return throw_vm_type_error(global_object, Some(PREPARE_STACK_TRACE_NOT_ERROR));
    }
    JSValue::undefined().encode()
}

/// `Error.appendStackTrace(source, destination)` (extensão do `bun`): acrescenta as linhas de frame do
/// `stack` de `source` ao `stack` de `destination`. Argumento que não é objeto lança `invalid_argument`.
fn error_constructor_append_stack_trace(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let vm = global_object.vm();
    let (Some(source), Some(destination)) =
        (ObjectRef::from_value(&call_frame.argument(0)), ObjectRef::from_value(&call_frame.argument(1)))
    else {
        return throw_vm_type_error(global_object, Some(APPEND_STACK_TRACE_NOT_ERROR));
    };
    let stack_name = PropertyName::from_identifier(&vm.property_names.stack);
    let mut texts = Vec::new();
    for object in [&source, &destination] {
        let value = object.get(global_object, &stack_name);
        if vm.exception().is_some() {
            return JSValue::empty().encode();
        }
        texts.push(if value.is_string() { value.to_wtf_string() } else { WtfString::default() });
    }
    let frame_lines = texts[0].utf8(ConversionMode::LenientConversion);
    let frame_lines = String::from_utf8_lossy(&frame_lines);
    let appended: String = frame_lines.split_once('\n').map(|(_, rest)| format!("\n{rest}")).unwrap_or_default();
    let mut combined = String::from_utf8_lossy(&texts[1].utf8(ConversionMode::LenientConversion)).into_owned();
    combined.push_str(&appended);
    destination.put_direct(vm, &stack_name, JSValue::from_js_string(js_string(vm, &WtfString::from_utf8(combined.as_bytes()))), DONT_ENUM);
    JSValue::undefined().encode()
}

/// `Error.prototype.toString` aplicado a `object` (as leituras de `name` e `message` e o `error_to_string`):
/// `Err` é o valor já lançado.
///
/// `invoke_getters` falso é o cabeçalho de `stack`, o `ErrorInstance::sanitizedToString` do bun: `name` e `message`
/// que são acessores (`get name()` na classe, `defineProperty` com `get`) não são chamados e valem como ausentes
/// (`Error: m`); o `name` só é procurado no próprio objeto e no protótipo direto (`class T extends TypeError {}`
/// dá `Error: m`, `new TypeError` dá `TypeError: m`); o `message`, só no próprio objeto.
pub(crate) fn error_header_of_object(
    global_object: &JSGlobalObject,
    object: &ObjectRef,
    invoke_getters: bool,
) -> Result<WtfString, EncodedJSValue> {
    let vm = global_object.vm();
    let read = |name: &Identifier, sanitized_depth: usize| -> Result<Option<WtfString>, EncodedJSValue> {
        let property_name = PropertyName::from_identifier(name);
        let value = if invoke_getters {
            let value = object.get(global_object, &property_name);
            if vm.exception().is_none() && value.is_undefined() {
                return Ok(None);
            }
            value
        } else {
            let mut found = None;
            let mut current = object.as_value();
            for _ in 0..sanitized_depth {
                let Some(candidate) = ObjectRef::from_value(&current) else { break };
                let mut slot = PropertySlot::new(current, InternalMethodType::VMInquiry);
                if candidate.get_own_property_slot(global_object, &property_name, &mut slot) && !slot.is_accessor() {
                    found = Some(slot.get_value_for(&property_name));
                    break;
                }
                current = candidate.get_prototype_direct();
            }
            // Ausente, `!isPrimitive()` (e o `name` `undefined`) valem como ausentes.
            match found {
                Some(value) if !value.is_object() && !(value.is_undefined() && sanitized_depth == 2) => value,
                _ => return Ok(None),
            }
        };
        if vm.exception().is_some() {
            return Err(JSValue::empty().encode());
        }
        to_wtf_string_value(global_object, value).map(Some).map_err(|_| JSValue::empty().encode())
    };
    let name = read(&vm.property_names.name, 2)?;
    let message = read(&vm.property_names.message, 1)?;
    match error_to_string(name.as_ref(), message.as_ref()) {
        Ok(header) => Ok(header),
        Err(StringOpError::OutOfMemory) => {
            let mut scope = ThrowScope::new(vm);
            throw_out_of_memory_error(global_object, &mut scope);
            Err(JSValue::empty().encode())
        }
        Err(StringOpError::RangeError(message)) => {
            Err(throw_error_object(global_object, create_range_error(global_object, &WtfString::from_utf8(message.as_bytes()))))
        }
    }
}

/// `ErrorInstance::create(globalObject, structure, message, options, ..., errorType, ...)`.
fn create_error_from_arguments(
    global_object: &JSGlobalObject,
    call_frame: &NativeCallFrame<'_>,
    structure: StructureRef,
    error_type: ErrorType,
    subclass_caller: Option<usize>,
) -> EncodedJSValue {
    let vm = global_object.vm();
    let message = call_frame.argument(0);
    let options = call_frame.argument(1);

    let message_string = if message.is_undefined() {
        None
    } else {
        // `message.toWTFString(globalObject)` com `RETURN_IF_EXCEPTION`: `Symbol` ou `toString` que lança
        // deixa a exceção pendente e nenhum `ErrorInstance` é criado.
        match to_wtf_string_value(global_object, message) {
            Ok(string) => Some(string),
            Err(_) => return JSValue::empty().encode(),
        }
    };
    let instance: ErrorInstanceRef =
        ErrorInstance::create(vm, structure, message_string.clone().unwrap_or_default(), error_type);
    // `finishCreation` captura os frames (`m_stackTrace`); `stack` só é formatada na primeira leitura. Sem
    // `stackTraceLimit` o C++ não guarda pilha nenhuma.
    if let Some(frames) = capture_frames(global_object, call_frame.call_frame(), subclass_caller, false) {
        // Medido no `bun` 1.4.2: `Reflect.construct(Error, .., F)` cujo `F` não está na pilha (o `foundCaller` nunca
        // vira verdadeiro) não deixa `stack` própria; os nativos (`TypeError`...) deixam, com os frames que houver.
        if !(frames.is_empty() && subclass_caller.is_some() && error_type == ErrorType::Error) {
            instance.set_pending_stack(frames);
        }
    }
    if let Some(message_string) = &message_string {
        put_message_property(vm, &instance, message_string);
    }

    // `options.cause` (InstallErrorCause): só se `options` é objeto e tem `cause`.
    if options.is_object() {
        if let Some(object) = ObjectRef::from_value(&options) {
            let cause_name = PropertyName::from_identifier(&vm.property_names.cause);
            let mut slot = PropertySlot::new(options, InternalMethodType::HasProperty);
            let has_cause = object.get_property_slot(global_object, &cause_name, &mut slot);
            if vm.exception().is_some() {
                return JSValue::empty().encode();
            }
            if has_cause {
                let cause = object.get(global_object, &cause_name);
                if vm.exception().is_some() {
                    return JSValue::empty().encode();
                }
                instance.put_direct(vm, &cause_name, cause, DONT_ENUM);
            }
        }
    }
    instance.as_value().encode()
}

/// `callErrorConstructor` e `callImpl` dos nativos: `globalObject->errorStructure(errorType)`.
fn call_error(global_object: &JSGlobalObject, call_frame: &NativeCallFrame<'_>, error_type: ErrorType) -> EncodedJSValue {
    create_error_from_arguments(global_object, call_frame, global_object.error_structure_for(error_type), error_type, None)
}

/// `constructErrorConstructor` e `constructImpl` dos nativos: a estrutura deriva de `newTarget`.
fn construct_error(global_object: &JSGlobalObject, call_frame: &NativeCallFrame<'_>, error_type: ErrorType) -> EncodedJSValue {
    let structure = match get_derived_structure_in_realm(
        global_object,
        call_frame.this_value(),
        call_frame.js_callee(),
        |realm| realm.error_structure_for(error_type),
    ) {
        Ok(structure) => structure,
        Err(thrown) => {
            throw_thrown(global_object, thrown);
            return JSValue::empty().encode();
        }
    };
    // `isErrorSubclass = newTarget != callee`: os frames até o construtor da subclasse não entram na pilha.
    let new_target = call_frame.this_value();
    // Medido no `bun` 1.4.2: só o `Error` pula os frames da subclasse; `class X extends TypeError {}` mantém
    // `at new X (unknown:1:28)` e `Reflect.construct(TypeError, [], F)` mantém todos os frames.
    let subclass_caller = (error_type == ErrorType::Error && new_target.is_cell() && new_target.as_cell() != call_frame.js_callee())
        .then(|| new_target.as_cell());
    create_error_from_arguments(global_object, call_frame, structure, error_type, subclass_caller)
}

/// Define o par `call`/`construct` de um tipo de erro.
macro_rules! error_constructor_functions {
    ($call:ident, $construct:ident, $error_type:expr) => {
        fn $call(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
            call_error(global_object, call_frame, $error_type)
        }
        fn $construct(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
            construct_error(global_object, call_frame, $error_type)
        }
    };
}

error_constructor_functions!(call_error_constructor, construct_error_constructor, ErrorType::Error);
error_constructor_functions!(call_eval_error, construct_eval_error, ErrorType::EvalError);
error_constructor_functions!(call_range_error, construct_range_error, ErrorType::RangeError);
error_constructor_functions!(call_reference_error, construct_reference_error, ErrorType::ReferenceError);
error_constructor_functions!(call_syntax_error, construct_syntax_error, ErrorType::SyntaxError);
error_constructor_functions!(call_type_error, construct_type_error, ErrorType::TypeError);
error_constructor_functions!(call_uri_error, construct_uri_error, ErrorType::URIError);

/// `callFunction<errorType>()` e `constructFunction<errorType>()`.
fn constructor_functions(error_type: ErrorType) -> (NativeFunction, NativeFunction) {
    match error_type {
        ErrorType::Error => (call_error_constructor, construct_error_constructor),
        ErrorType::EvalError => (call_eval_error, construct_eval_error),
        ErrorType::RangeError => (call_range_error, construct_range_error),
        ErrorType::ReferenceError => (call_reference_error, construct_reference_error),
        ErrorType::SyntaxError => (call_syntax_error, construct_syntax_error),
        ErrorType::TypeError => (call_type_error, construct_type_error),
        ErrorType::URIError => (call_uri_error, construct_uri_error),
        other => panic!("{other:?} não tem construtor no porte (AggregateError e SuppressedError não existem)"),
    }
}

/// `errorConstructorIsError`.
fn error_constructor_is_error(_global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let argument = call_frame.argument(0);
    let is_error_instance = argument.is_cell() && ErrorInstance::from_cell_id(argument.as_cell()).is_some();
    js_boolean(crate::runtime::error_constructor::is_error(argument.is_object(), is_error_instance)).encode()
}

/// `errorProtoFuncToString`.
fn error_proto_func_to_string(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let vm = global_object.vm();
    let this_value = crate::runtime::proxy_object::to_this_strict(call_frame.this_value());
    let Some(object) = ObjectRef::from_value(&this_value) else {
        return throw_vm_type_error(global_object, None);
    };

    match error_header_of_object(global_object, &object, true) {
        Ok(string) => JSValue::from_js_string(js_string(vm, &string)).encode(),
        Err(thrown) => thrown,
    }
}

/// `ErrorPrototypeBase::createStructure`: `ObjectType`, `JSNonFinalObject::StructureFlags`.
fn prototype_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
    Structure::create(
        vm,
        Some(global_object),
        prototype,
        TypeInfo::new(JSType::ObjectType, JSNonFinalObject::STRUCTURE_FLAGS),
        &ERROR_PROTOTYPE_S_INFO,
    )
}

/// `ErrorPrototype::createStructure`: `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
fn error_prototype_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
    Structure::create(
        vm,
        Some(global_object),
        prototype,
        TypeInfo::new(JSType::ObjectType, JSNonFinalObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE),
        &ERROR_PROTOTYPE_WITH_TABLE_S_INFO,
    )
}

/// `ErrorPrototypeBase::finishCreation(vm, name)`: `name` e `message`, ambos `DontEnum`.
fn create_error_prototype_object(
    vm: &VM,
    structure: &StructureRef,
    name: &str,
    install_first: impl FnOnce(&JSObjectRef),
) -> JSObjectRef {
    let prototype = JSObject::allocate(vm, structure);
    prototype.finish_creation(vm);
    // No bun a ordem de `Error.prototype` é `toString`, `name`, `message`: o que o chamador instala vem antes.
    install_first(&prototype);
    let (name, _) = initial_name_and_message(name);
    prototype.put_direct_without_transition(
        vm,
        &PropertyName::from_identifier(&vm.property_names.name),
        JSValue::from_js_string(js_string(vm, &name)),
        DONT_ENUM,
    );
    prototype.put_direct_without_transition(
        vm,
        &PropertyName::from_identifier(&vm.property_names.message),
        JSValue::from_js_string(js_empty_string(vm)),
        DONT_ENUM,
    );
    prototype
}

/// `ErrorConstructor::finishCreation` e `NativeErrorConstructorBase::finishCreation`: `length` 1, `name` e
/// `prototype`; o `constructor` do protótipo é ligado aqui (`DontEnum`).
fn create_constructor(
    vm: &VM,
    global_object: &JSGlobalObject,
    error_type: ErrorType,
    structure: StructureRef,
    prototype: &JSObject,
) -> JSFunctionRef {
    let (call, construct) = constructor_functions(error_type);
    let constructor =
        create_error_constructor_function(vm, global_object, structure, prototype, error_type_name(error_type), LENGTH, call, construct);
    global_object.put_direct(
        vm,
        &PropertyName::from_identifier(&Identifier::from_span(vm, error_type_name(error_type).as_bytes())),
        constructor.as_value(),
        DONT_ENUM,
    );
    constructor
}

/// O `Error` e os seis nativos: protótipos, estruturas de `ErrorInstance` por tipo (guardadas no global),
/// construtores e as propriedades globais (`DontEnum`). `object_prototype` e `function_prototype` são os
/// valores que `JSGlobalObject::init` já criou.
pub fn init_error_classes(
    vm: &VM,
    global_object: &JSGlobalObject,
    object_prototype: JSValue,
    function_prototype: JSValue,
) {
    // Error.prototype e o `Error`: o `toString` vem de `errorPrototypeTable` e reifica no primeiro acesso.
    let error_prototype_structure = error_prototype_structure(vm, global_object, object_prototype);
    let error_prototype = create_error_prototype_object(vm, &error_prototype_structure, error_prototype_name(), |_| {});
    error_prototype.did_become_prototype(vm);

    let instance_structure = ErrorInstance::create_structure(vm, Some(global_object), error_prototype.as_value());
    global_object.error_structures.borrow_mut().push((ErrorType::Error, instance_structure));

    let constructor_structure = |prototype: JSValue, flags: u32| error_constructor_structure(vm, global_object, prototype, flags);
    // `ErrorConstructor::StructureFlags = Base::StructureFlags | OverridesPut`; os nativos não a têm.
    let error_constructor = create_constructor(
        vm,
        global_object,
        ErrorType::Error,
        constructor_structure(function_prototype, OVERRIDES_PUT),
        &error_prototype,
    );
    // `stackTraceLimit` (`PropertyAttribute::None`), `captureStackTrace` (`DontEnum`, comprimento 0) e `isError`
    // (`DontEnum`, comprimento 1).
    *global_object.error_constructor.borrow_mut() = Some(error_constructor.as_value());
    crate::runtime::call_site_prototype::install_call_site(vm, global_object, object_prototype);
    // No V8 a ordem é `captureStackTrace,isError,stackTraceLimit`, sem as extensões do bun (medido no Chromium 154).
    let v8 = crate::runtime::stack_frame::v8_stack_format();
    let put_stack_trace_limit = || {
        error_constructor.put_direct_without_transition(
            vm,
            &PropertyName::from_identifier(&vm.property_names.stack_trace_limit),
            js_number(f64::from(
                global_object.stack_trace_limit.get().unwrap_or_else(Options::default_error_stack_trace_limit),
            )),
            0,
        );
    };
    if !v8 {
        put_stack_trace_limit();
    }
    put_direct_native_function_without_transition(
        vm,
        global_object,
        &error_constructor,
        &vm.property_names.capture_stack_trace,
        CAPTURE_STACK_TRACE_LENGTH,
        error_constructor_capture_stack_trace,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        FUNCTION_ATTRIBUTES,
    );
    put_direct_native_function_without_transition(
        vm,
        global_object,
        &error_constructor,
        &Identifier::from_span(vm, b"isError".as_slice()),
        IS_ERROR_LENGTH,
        error_constructor_is_error,
        ImplementationVisibility::Public,
        Intrinsic::ErrorIsErrorIntrinsic,
        FUNCTION_ATTRIBUTES,
    );
    if v8 {
        put_stack_trace_limit();
    } else {
        install_bun_stack_extensions(vm, global_object, &error_constructor);
    }

    // Os nativos: protótipo com o `Error.prototype` como protótipo, construtor com o `Error` como protótipo.
    for error_type in NATIVE_ERROR_TYPES {
        let structure = prototype_structure(vm, global_object, error_prototype.as_value());
        let prototype = create_error_prototype_object(vm, &structure, error_type_name(error_type), |_| {});
        prototype.did_become_prototype(vm);
        let instance_structure = ErrorInstance::create_structure(vm, Some(global_object), prototype.as_value());
        global_object.error_structures.borrow_mut().push((error_type, instance_structure));
        create_constructor(vm, global_object, error_type, constructor_structure(error_constructor.as_value(), 0), &prototype);
    }
}

/// Extensões do `bun` (medido: `Object.getOwnPropertyNames(Error)` termina em `appendStackTrace,prepareStackTrace`).
fn install_bun_stack_extensions(vm: &VM, global_object: &JSGlobalObject, error_constructor: &JSFunctionRef) {
    put_direct_native_function_without_transition(
        vm,
        global_object,
        error_constructor,
        &Identifier::from_span(vm, b"appendStackTrace".as_slice()),
        APPEND_STACK_TRACE_LENGTH,
        error_constructor_append_stack_trace,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        FUNCTION_ATTRIBUTES,
    );
    // O padrão do bun é uma função de nome `ErrorPrepareStackTrace` e comprimento 2.
    let default_prepare = crate::runtime::js_function::JSFunction::create_native(
        vm,
        global_object,
        PREPARE_STACK_TRACE_LENGTH,
        &WtfString::from_utf8(DEFAULT_PREPARE_STACK_TRACE_NAME.as_bytes()),
        error_constructor_default_prepare_stack_trace,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        crate::runtime::js_function::call_host_function_as_constructor,
    );
    error_constructor.put_direct_without_transition(
        vm,
        &PropertyName::from_identifier(&Identifier::from_span(vm, b"prepareStackTrace".as_slice())),
        default_prepare.as_value(),
        FUNCTION_ATTRIBUTES,
    );
}
