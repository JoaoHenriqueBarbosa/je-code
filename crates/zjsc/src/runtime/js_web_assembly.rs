//! A ponte JS do WebAssembly (`wasm/js/JSWebAssembly.cpp`, `JSWebAssemblyModule`,
//! `WebAssemblyModuleConstructor`, `WebAssemblyModulePrototype`): o objeto `WebAssembly`,
//! `WebAssembly.validate` e `WebAssembly.Module` (construtor, `Module.exports`, `Module.imports`).
//!
//! As instâncias de `Module` reusam a célula `IntlInstance` (que carrega um estado `Box<dyn Any>`),
//! como as classes do `Intl`, até existir uma classe de célula própria do wasm.
//!
//! `WebAssembly.Instance` (`JSWebAssemblyInstance`, `WebAssemblyInstanceConstructor`, a leitura de
//! importações de `WebAssemblyModuleRecord::link`): também uma `IntlInstance`, com o `Rc<Instance>` e o
//! objeto `exports` (protótipo nulo, congelado) no estado.
//!
//! LACUNAS (ver `wip-notes/wasm-plan.md`, Fatia 6):
//!
//! - `Module.customSections`, `Tag`, `Exception`, `compile` e `instantiate` não foram portados.
//!   `CompileError`/`LinkError`/`RuntimeError` vivem em `wasm_errors.rs`.
//! - `Memory`, `Table` e `Global` existem como objetos JS (importação e exportação ligadas em `Instance`).
//!   `Memory.buffer` é um `ArrayBuffer` sobre o mesmo `Rc<RefCell<Vec<u8>>>` da memória; `grow` o destaca.
//!   Faltam: referências não nulas em `Table`/`Global` (`externref`, `funcref`; precisam do GC e da ponte entre
//!   instâncias), `toFixedLengthBuffer`/`toResizableBuffer`/`type()`, memória `shared` como `SharedArrayBuffer`
//!   e Memory64. Exceção importada como Tag e resultado múltiplo de importação JS (iterável) respondem `LinkError`
//!   e `Unported`.
//! - A função exportada acha sua instância por uma tabela `cell_id -> ExportedFunction` do thread (no C++ é
//!   o próprio `WebAssemblyFunction`); as entradas nunca saem até existir célula própria com GC.

use crate::host_function;
use crate::wasm::wasm_exception_type::{error_message_for_exception_type, ExceptionType};
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::host_call::{throw_thrown, HostCall, HostResult, Thrown};
use crate::runtime::exception_helpers::{append_default_source_to_native_message, create_type_error_with_default_appender};
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::runtime::intl_support::{
    array_of, construct_instance, new_object, prop, put, str_value, with_instance, IntlClass, IntlInstance,
};
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::iterator_operations::get_value_property;
use crate::runtime::js_array_buffer::JSArrayBuffer;
use crate::runtime::js_big_int_ops::{make_big_int_from_i64, to_big_int, to_big_int64_value};
use crate::runtime::js_function::{call_host_function_as_constructor, put_direct_native_function_without_transition, JSFunction, JSFunctionRef};
use crate::runtime::identifier::Identifier;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::{js_null, js_number, js_number_i32, js_undefined, JSValue};
use crate::runtime::js_web_assembly_tag::{tag_from_value, TagData, TagState};
use crate::runtime::collection_support::{derived_structure, put_to_string_tag};
use crate::runtime::current_realm::current_global_object;
use crate::runtime::js_getter_setter::GetterSetter;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::ACCESSOR;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_object::{JSNonFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::lookup::{HashTable, HashTableValue, Kind, LazyPropertyCallback};
use crate::runtime::lookup::{lazy_entry, native_function_entry};
use crate::runtime::property_attribute::{DONT_ENUM, FUNCTION, PROPERTY_CALLBACK};
use crate::runtime::vm::VM;
use crate::runtime::array_buffer::{ArrayBuffer, ArrayBufferRef};
use crate::runtime::intl_support::wtf_to_rust;
use crate::runtime::intl_support::to_rust_string;
use crate::runtime::js_promise::{JSPromise, JSPromiseRef};
use crate::runtime::body::ReadKind;
use crate::runtime::headers::header_value;
use crate::runtime::response;
use crate::runtime::js_promise_host::PromiseHost;
use crate::runtime::js_array_buffer::to_js_array_buffer;
use crate::wasm::page_count::PageCount;
use crate::wasm::wasm_address_type::AddressType;
use crate::wasm::wasm_format::{funcref_type, is_ref_to_abstract, TableElementType, TypeIndex, TYPE_F32, TYPE_F64, TYPE_I32, TYPE_I64};
use crate::wasm::wasm_limits::{max_declarable_pages, MAX_TABLE_ENTRIES};
use crate::wasm::wasm_memory::{max_allocatable_bytes, GrowFailReason, Memory};
use crate::wasm::wasm_table::Table;
use crate::runtime::object_constructor::object_constructor_freeze;
use crate::runtime::object_to_primitive::call_function;
use crate::runtime::wasm_errors::{install_wasm_errors, WasmErrorKind};
use crate::wasm::wasm_format::{ExternalKind, Import, Mutability, Type, TypeKind};
use crate::wasm::wasm_global::Global;
use crate::wasm::wasm_instance::{func_ref, func_ref_target, function_wrapper, instance_of, set_function_wrapper, gc_abstract_matches, gc_ref_index, i31_ref_value, null_ref, HostFunction, ImportValue, Instance, WasmError};
use crate::wasm::wasm_ipint::{reference_matches_type, Completion, NestedSuspension};
use crate::runtime::js_web_assembly_gc_object::{gc_object_for, gc_object_of};
use crate::wasm::wasm_module_information::{ModuleInformation, StructuralType};
use crate::wasm::wasm_streaming_parser::validate_module;
use crate::wtf::text::wtf_string::String as WtfString;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// `JSWebAssemblyModule`: a informação do módulo já validada.
struct ModuleState {
    info: Rc<ModuleInformation>,
}

/// `getWasmBufferFromValue`: os bytes de um `ArrayBuffer`, `SharedArrayBuffer` ou visão de buffer.
fn buffer_source_bytes(value: JSValue) -> Option<Vec<u8>> {
    if let Some(buffer) = JSArrayBuffer::to_wrapped_allow_shared(value) {
        return Some(buffer.with_bytes(|bytes| bytes.to_vec()));
    }
    if let JSValue::Cell(cell_id) = value {
        if let Some(CellEntry::TypedArray(view)) = cell_registry::get(cell_id) {
            return Some(view.with_vector(|bytes| bytes.to_vec()));
        }
    }
    None
}

const BUFFER_SOURCE_MESSAGE: &str = "first argument must be an ArrayBufferView or an ArrayBuffer";

/// `throwException(globalObject, scope, createTypeError(globalObject, message, defaultSourceAppender, ...))`:
/// lança o `TypeError` com o ` (evaluating '...')` da chamada JS em curso e devolve a exceção já pendente.
fn throw_type_error_with_source(global_object: &JSGlobalObject, message: &str) -> Thrown {
    let error = create_type_error_with_default_appender(global_object, message, None);
    let mut scope = ThrowScope::new(global_object.vm());
    throw_exception(global_object, &mut scope, error);
    Thrown::Pending
}

/// `createSourceBufferFromValue`: os bytes de um `ArrayBufferView` ou `ArrayBuffer`; qualquer outro valor é
/// o `TypeError` de `getWasmBufferFromValue` (com o texto-fonte da chamada).
fn bytes_from_value(global_object: &JSGlobalObject, value: JSValue) -> Result<Vec<u8>, Thrown> {
    buffer_source_bytes(value).ok_or_else(|| throw_type_error_with_source(global_object, BUFFER_SOURCE_MESSAGE))
}

/// `toNonWrappingUint32(globalObject, value)` (`JSWebAssemblyHelpers.h`): inteiro finito em `[0, 2^32 - 1]` depois
/// de `toNumber` e `trunc`; fora disso `TypeError` (`undefined` vira `NaN` e cai aqui).
fn to_non_wrapping_uint32(global_object: &JSGlobalObject, value: JSValue) -> Result<u64, Thrown> {
    let number = value.to_number();
    check_pending(global_object)?;
    if number.is_finite() {
        let truncated = number.trunc();
        if (0.0..=f64::from(u32::MAX)).contains(&truncated) {
            return Ok(truncated as u64);
        }
    }
    Err(Thrown::type_error("Expect an integer argument in the range: [0, 2^32 - 1]"))
}

/// `Wasm::Module::validateSync` mais o erro: `CompileError` com a mensagem do parser.
fn compile_bytes(bytes: &[u8]) -> Result<ModuleState, Thrown> {
    match validate_module(bytes, true) {
        Ok(info) => Ok(ModuleState { info: Rc::new(info) }),
        Err(message) => Err(Thrown::WebAssembly(WasmErrorKind::Compile, message)),
    }
}

/// `WebAssembly.validate(bytes)`: `Module::validateSync` sem guardar o resultado.
fn validate_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let bytes = bytes_from_value(global_object, call.argument(0))?;
    Ok(crate::runtime::js_value::js_boolean(validate_module(&bytes, true).is_ok()))
}

fn call_module_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(Thrown::type_error("calling WebAssembly.Module constructor without new is invalid"))
}

/// `constructJSWebAssemblyModule`: o `CompileError` síncrono leva o texto-fonte da chamada (o de
/// `WebAssembly.compile` rejeita numa tarefa do laço de eventos, sem ele).
fn construct_module_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    construct_instance(global_object, call, |_| {
        let bytes = bytes_from_value(global_object, call.argument(0))?;
        match compile_bytes(&bytes) {
            Ok(state) => Ok(Box::new(state)),
            Err(Thrown::WebAssembly(kind, message)) => {
                let message = append_default_source_to_native_message(global_object, &message);
                Err(Thrown::WebAssembly(kind, wtf_to_rust(&message)))
            }
            Err(other) => Err(other),
        }
    })
}

/// O nome de `ExternalKind` em `WebAssembly.Module.imports/exports` (`makeString(kind)`).
fn kind_name(kind: ExternalKind) -> &'static str {
    match kind {
        ExternalKind::Function => "function",
        ExternalKind::Table => "table",
        ExternalKind::Memory => "memory",
        ExternalKind::Global => "global",
        ExternalKind::Exception => "tag",
    }
}

/// O `ModuleState` do argumento 0 (`dynamicDowncast<JSWebAssemblyModule>`), com a mensagem do C++ (medida no bun 1.4.2,
/// igual sem argumento, com primitivo e com objeto comum).
fn module_argument<R>(call: &HostCall, member: &str, body: impl FnOnce(&ModuleState) -> R) -> Result<R, Thrown> {
    let message = format!("WebAssembly.Module.{member} called with non WebAssembly.Module argument");
    with_instance::<ModuleState, _>(call.argument(0), &message, |state, _| Ok(body(state)))
}

/// `WebAssembly.Module.exports(module)`: `[{ name, kind }]`.
fn module_exports_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let entries = module_argument(call, "exports", |state| {
        state.info.exports.iter().map(|export| (export.field.clone(), kind_name(export.kind))).collect::<Vec<_>>()
    })?;
    let vm = global_object.vm();
    let items: Vec<JSValue> = entries
        .iter()
        .map(|(name, kind)| {
            let object = new_object(global_object);
            put(global_object, &object, "name", str_value(vm, name));
            put(global_object, &object, "kind", str_value(vm, kind));
            object.as_value()
        })
        .collect();
    Ok(array_of(global_object, &items))
}

/// `WebAssembly.Module.imports(module)`: `[{ module, name, kind }]`.
fn module_imports_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let entries = module_argument(call, "imports", |state| {
        state.info.imports.iter().map(|import| (import.module.clone(), import.field.clone(), kind_name(import.kind))).collect::<Vec<_>>()
    })?;
    let vm = global_object.vm();
    let items: Vec<JSValue> = entries
        .iter()
        .map(|(module, name, kind)| {
            let object = new_object(global_object);
            put(global_object, &object, "module", str_value(vm, module));
            put(global_object, &object, "name", str_value(vm, name));
            put(global_object, &object, "kind", str_value(vm, kind));
            object.as_value()
        })
        .collect();
    Ok(array_of(global_object, &items))
}

/// `WebAssembly.Module.customSections(module, name)`: um `ArrayBuffer` (cópia) por seção de nome igual.
fn module_custom_sections_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if call.argument_count() < 2 {
        return Err(Thrown::type_error("Not enough arguments"));
    }
    let info = module_argument(call, "customSections", |state| Rc::clone(&state.info))?;
    let name = to_rust_string(global_object, call.argument(1))?;
    let items: Vec<JSValue> = info
        .custom_sections
        .iter()
        .filter(|section| section.name == name)
        .map(|section| to_js_array_buffer(global_object, &ArrayBuffer::create_from_span(&section.payload)).as_value())
        .collect();
    Ok(array_of(global_object, &items))
}

// ---------------------------------------------------------------------------------------------
// WebAssembly.compile e WebAssembly.instantiate (JSWebAssembly::compileAndInstantiate)
// ---------------------------------------------------------------------------------------------

/// O valor de uma exceção que um corpo nativo teria lançado: lança no `VM`, lê e limpa a exceção
/// (`catchScope.exception()` + `clearException`). `Pending` é a exceção já pendente.
pub(crate) fn thrown_to_value(global_object: &JSGlobalObject, thrown: Thrown) -> JSValue {
    if !matches!(thrown, Thrown::Pending) {
        throw_thrown(global_object, thrown);
    }
    let vm = global_object.vm();
    let value = vm.exception().map(|exception| exception.value()).unwrap_or_else(js_undefined);
    vm.clear_exception();
    value
}

/// Uma promessa já resolvida com `Ok(valor)` ou rejeitada com a exceção de `Err`. Em todo caminho do C++
/// (`JSWebAssembly::instantiate`/`compile`) o erro vira rejeição e nunca exceção síncrona.
///
/// LACUNA: no JSC o resultado de uma compilação com bytes chega depois, numa tarefa do laço de eventos
/// (depois de todas as microtarefas já enfileiradas, medido no bun: `compile(b).then(f)` roda `f` depois de
/// uma cadeia de oito `then` de `Promise.resolve()`). Sem fila de tarefas do host, a promessa assenta na
/// hora e `f` roda na primeira microtarefa. Só os erros de argumento (TypeError) são rejeição síncrona de
/// verdade também no JSC. O dia em que existir uma fila de tarefas, `settle` passa a enfileirar.
pub(crate) fn settle(global_object: &JSGlobalObject, result: HostResult) -> HostResult {
    let promise = JSPromise::create(global_object.vm(), &global_object.promise_structure());
    match result {
        Ok(value) => promise.resolve(global_object, value),
        Err(thrown) => {
            let error = thrown_to_value(global_object, thrown);
            promise.reject(global_object, error);
        }
    }
    Ok(promise.as_value())
}

/// A criação de um objeto de classe wasm a partir do código nativo (sem `new.target`).
fn create_class_instance(global_object: &JSGlobalObject, class: &'static str, state: Box<dyn std::any::Any>) -> JSValue {
    let structure = WRAPPER_STRUCTURES.with(|structures| structures.borrow().get(class).cloned()).expect("classe wasm não instalada");
    IntlInstance::create(global_object.vm(), &structure, state).as_value()
}

/// `WebAssembly.compile(bytes)`.
fn compile_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    settle(global_object, compile_value(global_object, call.argument(0)))
}

fn compile_value(global_object: &JSGlobalObject, source: JSValue) -> HostResult {
    bytes_from_value(global_object, source)
        .and_then(|bytes| compile_bytes(&bytes))
        .map(|state| create_class_instance(global_object, "Module", Box::new(state)))
}

/// `WebAssembly.instantiate(bytes | module, imports)`: com bytes `{ module, instance }`, com `Module` só a
/// `Instance`.
fn instantiate_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let imports = call.argument(1);
    if !imports.is_undefined() && !imports.is_object() {
        return settle(global_object, Err(throw_type_error_with_source(global_object, "second argument to WebAssembly.instantiate must be undefined or an Object")));
    }
    settle(global_object, instantiate_value(global_object, call.argument(0), imports))
}

fn instantiate_value(global_object: &JSGlobalObject, source: JSValue, imports: JSValue) -> HostResult {
    let module_info = IntlInstance::from_value(&source).and_then(|cell| cell.state::<ModuleState>().map(|state| Rc::clone(&state.info)));
    match module_info {
        Some(info) => link_instance_in_call(global_object, &info, imports).map(|state| create_class_instance(global_object, "Instance", Box::new(state))),
        None => bytes_from_value(global_object, source)
            .and_then(|bytes| compile_bytes(&bytes))
            .and_then(|state| {
                let info = Rc::clone(&state.info);
                let module = create_class_instance(global_object, "Module", Box::new(state));
                let instance = create_class_instance(global_object, "Instance", Box::new(link_instance(global_object, &info, imports)?));
                let result = new_object(global_object);
                put(global_object, &result, "module", module);
                put(global_object, &result, "instance", instance);
                Ok(result.as_value())
            }),
    }
}

host_function!(web_assembly_validate, validate_body);

/// A descrição do argumento no estilo do `ERR_INVALID_ARG_TYPE` do bun ("Received ...").
pub(crate) fn received_description(global_object: &JSGlobalObject, value: JSValue) -> String {
    let vm = global_object.vm();
    if value.is_undefined() {
        return "undefined".to_string();
    }
    if value.is_null() {
        return "null".to_string();
    }
    if value.is_number() {
        return format!("type number ({})", value.as_number());
    }
    if value.is_boolean() {
        return format!("type boolean ({})", value.as_boolean());
    }
    if value.is_string() {
        return format!("type string ('{}')", wtf_to_rust(&value.to_string(vm).value()));
    }
    if value.is_big_int() {
        return format!("type bigint ({}n)", wtf_to_rust(&value.to_string(vm).value()));
    }
    if value.is_cell() {
        if let Some(symbol) = crate::runtime::symbol::Symbol::from_cell_id(value.as_cell()) {
            if let Ok(description) = symbol.try_get_descriptive_string() {
                return format!("type symbol ({})", wtf_to_rust(&description));
            }
        }
    }
    let name = get_value_property(global_object, value, &prop(vm, "constructor"))
        .and_then(|constructor| get_value_property(global_object, constructor, &prop(vm, "name")))
        .ok()
        .filter(|name| name.is_string())
        .map(|name| wtf_to_rust(&name.to_string(vm).value()))
        .unwrap_or_else(|| "Object".to_string());
    format!("an instance of {name}")
}

/// Um `compileStreaming`/`instantiateStreaming` em andamento: a promessa devolvida e, no `instantiate`, as
/// importações (`None` é o `compile`).
struct Streaming {
    promise: JSPromiseRef,
    imports: Option<JSValue>,
}

thread_local! {
    /// `cell_id` de cada reação nativa pendente -> o streaming que ela continua.
    static STREAMINGS: RefCell<HashMap<usize, Rc<Streaming>>> = RefCell::new(HashMap::new());
}

/// Pendura `on_fulfilled` e a rejeição que repassa o motivo em `awaited`, as duas continuando `streaming`.
fn continue_streaming(global_object: &JSGlobalObject, awaited: &JSPromiseRef, streaming: Rc<Streaming>, on_fulfilled: NativeFunction) {
    let fulfilled = crate::runtime::timers::native(global_object, "", 1, on_fulfilled);
    let rejected = crate::runtime::timers::native(global_object, "", 1, streaming_rejected);
    STREAMINGS.with(|table| {
        let mut table = table.borrow_mut();
        for reaction in [fulfilled, rejected] {
            if let JSValue::Cell(id) = reaction {
                table.insert(id, Rc::clone(&streaming));
            }
        }
    });
    awaited.perform_promise_then(global_object, fulfilled, rejected, js_undefined());
}

/// Tira o streaming da reação que roda (e da irmã, que não vai mais rodar).
fn take_streaming(reaction: usize) -> Option<Rc<Streaming>> {
    STREAMINGS.with(|table| {
        let mut table = table.borrow_mut();
        let streaming = table.remove(&reaction)?;
        table.retain(|_, other| !Rc::ptr_eq(other, &streaming));
        Some(streaming)
    })
}

fn finish_streaming(global_object: &JSGlobalObject, streaming: &Streaming, result: HostResult) -> HostResult {
    match result {
        Ok(value) => streaming.promise.resolve(global_object, value),
        Err(thrown) => {
            let error = thrown_to_value(global_object, thrown);
            streaming.promise.reject(global_object, error);
        }
    }
    Ok(js_undefined())
}

/// `compileStreaming(source)` e `instantiateStreaming(source, imports)` do bun 1.4.2 (medidos): a fonte é um
/// `Response` ou uma promessa dele; o `Content-Type` (sem os brancos das pontas, sem caixa) tem de ser
/// `application/wasm`, o status de sucesso e o corpo ainda não lido, nessa ordem.
fn streaming_body(global_object: &JSGlobalObject, call: &HostCall, instantiate: bool) -> HostResult {
    let imports = call.argument(1);
    if instantiate && !imports.is_undefined() && !imports.is_object() {
        return settle(
            global_object,
            Err(throw_type_error_with_source(global_object, "second argument to WebAssembly.instantiateStreaming must be undefined or an Object")),
        );
    }
    let awaited = match crate::runtime::js_promise_host::host_result(global_object, JSPromise::resolved_promise(global_object, call.argument(0))) {
        Ok(awaited) => awaited,
        Err(thrown) => return settle(global_object, Err(thrown)),
    };
    let promise = JSPromise::create(global_object.vm(), &global_object.promise_structure());
    let streaming = Rc::new(Streaming { promise: promise.clone(), imports: instantiate.then_some(imports) });
    continue_streaming(global_object, &awaited, streaming, streaming_response);
    Ok(promise.as_value())
}

/// A fonte resolveu: confere o `Response` e lê o corpo.
fn streaming_response_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(streaming) = take_streaming(call.callee()) else { return Ok(js_undefined()) };
    let source = call.argument(0);
    let checked = response_body_reader(global_object, source).and_then(|state| {
        let read = response::read_response(global_object, source, state, ReadKind::ArrayBuffer)?;
        JSPromise::from_value(&read).ok_or_else(|| Thrown::type_error("WebAssembly response body has already been used"))
    });
    match checked {
        Ok(read) => {
            continue_streaming(global_object, &read, streaming, streaming_bytes);
            Ok(js_undefined())
        }
        Err(thrown) => finish_streaming(global_object, &streaming, Err(thrown)),
    }
}

fn response_body_reader(global_object: &JSGlobalObject, source: JSValue) -> Result<response::ResponseState, Thrown> {
    let Some(state) = response::state_of(source) else {
        return Err(Thrown::type_error(&format!(
            "The \"source\" argument must be an instance of Response or an Promise resolving to Response. Received {}",
            received_description(global_object, source)
        )));
    };
    let content_type = header_value(state.headers, "content-type");
    let mime = content_type.as_deref().map(|value| String::from_utf8_lossy(value).trim_matches(|c: char| c.is_ascii_whitespace()).to_owned());
    if !mime.as_deref().is_some_and(|mime| mime.eq_ignore_ascii_case("application/wasm")) {
        return Err(Thrown::type_error(&format!("WebAssembly response has unsupported MIME type '{}'", mime.as_deref().unwrap_or("null"))));
    }
    if !(200..=299).contains(&state.status) {
        return Err(Thrown::type_error(&format!("WebAssembly response has status code {}", state.status)));
    }
    if state.body.as_ref().is_some_and(|body| body.is_used()) {
        return Err(Thrown::type_error("WebAssembly response body has already been used"));
    }
    Ok(state)
}

/// O corpo chegou: compila (e instancia).
fn streaming_bytes_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(streaming) = take_streaming(call.callee()) else { return Ok(js_undefined()) };
    let result = match streaming.imports {
        Some(imports) => instantiate_value(global_object, call.argument(0), imports),
        None => compile_value(global_object, call.argument(0)),
    };
    finish_streaming(global_object, &streaming, result)
}

fn streaming_rejected_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(streaming) = take_streaming(call.callee()) else { return Ok(js_undefined()) };
    streaming.promise.reject(global_object, call.argument(0));
    Ok(js_undefined())
}

fn compile_streaming_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    streaming_body(global_object, call, false)
}

fn instantiate_streaming_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    streaming_body(global_object, call, true)
}

host_function!(web_assembly_compile_streaming, compile_streaming_body);
host_function!(web_assembly_instantiate_streaming, instantiate_streaming_body);
host_function!(streaming_response, streaming_response_body);
host_function!(streaming_bytes, streaming_bytes_body);
host_function!(streaming_rejected, streaming_rejected_body);host_function!(web_assembly_compile, compile_body);
host_function!(web_assembly_instantiate, instantiate_body);
host_function!(web_assembly_module_custom_sections, module_custom_sections_body);

// ---------------------------------------------------------------------------------------------
// WebAssembly.Instance
// ---------------------------------------------------------------------------------------------

/// `JSWebAssemblyInstance::visitChildren` (o `exports` congelado) e `JSWebAssemblyMemory::visitChildren` (o
/// `ArrayBuffer` que o `buffer` entregou por último).
pub(crate) fn visit_state(state: &dyn std::any::Any, marker: &mut crate::runtime::gc::Marker) {
    if let Some(state) = state.downcast_ref::<InstanceState>() {
        marker.value(state.exports);
    }
}

/// `JSWebAssemblyInstance`: a instância e o objeto `exports` já congelado.
struct InstanceState {
    exports: JSValue,
}

/// O que uma função exportada precisa para ser chamada (o `WebAssemblyFunction`).
pub(crate) struct ExportedFunction {
    pub(crate) instance: Rc<Instance>,
    pub(crate) index: u32,
    pub(crate) arguments: Vec<Type>,
    pub(crate) returns: Vec<Type>,
}

thread_local! {
    /// `cell_id` da função JS exportada -> o que ela chama.
    static EXPORTED_FUNCTIONS: RefCell<HashMap<usize, Rc<ExportedFunction>>> = RefCell::new(HashMap::new());
}

/// Fim do programa (`cell_registry::reset_program_state`): tudo que a ponte JS de WebAssembly guarda por
/// `cell_id` ou por `JSValue` (funções exportadas, externrefs internados, estruturas e wrappers) morre com o VM.
/// As raízes do coletor: os valores externos, os que as instâncias seguram, os construtores e os wrappers.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    EXTERN_VALUES.with(|values| marker.values(&values.borrow()));
    INSTANCE_VALUES.with(|values| marker.values(&values.borrow()));
    crate::wasm::wasm_instance::function_wrappers().into_iter().for_each(|wrapper| marker.encoded(wrapper as i64));
    CLASS_CONSTRUCTORS.with(|constructors| constructors.borrow().values().for_each(|value| marker.value(*value)));
    WRAPPERS.with(|wrappers| wrappers.borrow().values().for_each(|value| marker.value(*value)));
}

/// A compilação em curso vive com a reação que a continua (a promessa e as importações), e a função exportada leva
/// a instância (que não tem valores do programa fora de `INSTANCE_VALUES`).
pub(crate) fn visit_side_table(marker: &mut crate::runtime::gc::Marker, phase: crate::runtime::gc::SidePhase) -> bool {
    let streamings = crate::runtime::gc::side_table(&STREAMINGS, marker, phase, |streaming, out| {
        out.push(streaming.promise.as_value());
        out.extend(streaming.imports);
    });
    let exported = crate::runtime::gc::side_table(&EXPORTED_FUNCTIONS, marker, phase, |_, _| {});
    streamings | exported
}

pub(crate) fn reset_for_program() {
    let _ = INSTANCE_VALUES.try_with(|values| values.borrow_mut().clear());
    let _ = EXPORTED_FUNCTIONS.try_with(|map| map.borrow_mut().clear());
    let _ = EXTERN_VALUES.try_with(|values| values.borrow_mut().clear());
    let _ = EXTERN_INDEX.try_with(|index| index.borrow_mut().clear());
    let _ = WRAPPER_STRUCTURES.try_with(|map| map.borrow_mut().clear());
    let _ = CLASS_CONSTRUCTORS.try_with(|map| map.borrow_mut().clear());
    let _ = WRAPPERS.try_with(|map| map.borrow_mut().clear());
    let _ = STREAMINGS.try_with(|map| map.borrow_mut().clear());
}

/// Parâmetros e resultados da função `function_index_space`.
fn function_signature(info: &ModuleInformation, function_index_space: u32) -> (Vec<Type>, Vec<Type>) {
    match &info.rtt_from_function_index_space(function_index_space as usize).structural {
        StructuralType::Function { arguments, returns } => (arguments.clone(), returns.clone()),
        _ => unreachable!("o espaço de funções só aponta para tipos de função"),
    }
}

/// `importFailMessage`.
fn import_fail_message(import: &Import, before: &str, after: &str) -> String {
    format!("{} {}:{} {}", before, import.module, import.field, after)
}

/// `RETURN_IF_EXCEPTION`: uma exceção de JS pendente no `VM` vence qualquer outro desfecho.
pub(crate) fn check_pending(global_object: &JSGlobalObject) -> Result<(), Thrown> {
    if global_object.vm().exception().is_some() { Err(Thrown::Pending) } else { Ok(()) }
}

/// `toWebAssemblyValue(globalObject, type, value)`: os bits do valor.
pub(crate) fn to_wasm_value(global_object: &JSGlobalObject, value: JSValue, ty: Type) -> Result<u64, Thrown> {
    let bits = match ty.kind {
        TypeKind::I32 => u64::from(value.to_int32() as u32),
        TypeKind::I64 => {
            let big = to_big_int(value);
            check_pending(global_object)?;
            to_big_int64_value(big) as u64
        }
        TypeKind::F32 => u64::from((value.to_number() as f32).to_bits()),
        TypeKind::F64 => value.to_number().to_bits(),
        TypeKind::V128 => return Err(Thrown::type_error("Invalid argument type in ToWebAssemblyValue")),
        _ if value.is_null() && ty.is_nullable() => null_ref(),
        // Medido no bun 1.4.2: `externref` e `anyref` aceitam qualquer valor (1, `undefined`, `{}`, 'a', função) e o devolvem
        // idêntico; qualquer outro tipo de referência rejeita com a mensagem de tipo.
        // O objeto GC opaco é o próprio valor JS do `externref` (internado como qualquer valor); nos demais tipos de
        // referência ele volta a ser a referência global do registro compartilhado (`GcHeap`), que vale em qualquer
        // instância, Table e Global. Tipo definido confere contra o RTT canônico global, sem instância dona.
        _ if gc_object_of(value).is_some() && !is_extern_type(ty) => gc_object_to_wasm(value, ty)?,
        _ if !value.is_null() && accepts_any_js_value(ty) => intern_extern_value(value),
        // Função wasm exportada (inclusive o `WebAssemblyWrapperFunction` de uma importação de JS): a referência dela
        // (o mapa inverso wrapper -> funcref). Em `(ref $t)` confere o tipo da função contra o RTT canônico.
        _ if (ty.is_ref() || ty.is_ref_null() || ty.kind == TypeKind::Funcref) && exported_target(value).is_some() => {
            let reference = exported_func_ref(value);
            if is_abstract_funcref(ty) || reference_matches_type(reference, ty) { reference } else { return Err(Thrown::type_error(REFERENCE_MISMATCH_MESSAGE)) }
        }
        _ => return Err(Thrown::type_error(REFERENCE_MISMATCH_MESSAGE)),
    };
    check_pending(global_object)?;
    Ok(bits)
}

/// `funcref` e `(ref func)`, o tipo heap abstrato de função.
fn is_abstract_funcref(ty: Type) -> bool {
    ty.kind == TypeKind::Funcref || ty.index == TypeIndex::Abstract(TypeKind::Funcref)
}

/// A referência da função wasm exportada `value` (o mapa inverso wrapper -> funcref); `value` precisa ser uma.
fn exported_func_ref(value: JSValue) -> u64 {
    exported_target(value).map_or_else(null_ref, |target| func_ref(target.instance.id(), target.index))
}

/// `ToJSValue` de funcref não nulo: o wrapper da função, criado (e guardado) na instância dona se ainda não existe.
fn func_ref_to_js(reference: u64) -> HostResult {
    let (owner, index) = func_ref_target(reference).ok_or_else(|| Thrown::type_error(REFERENCE_MISMATCH_MESSAGE))?;
    let instance = instance_of(owner).ok_or_else(|| Thrown::type_error("WebAssembly function's instance is no longer available"))?;
    Ok(exported_function_wrapper(&current_global_object(), &instance, index))
}

/// `externref` e `(ref extern)`.
fn is_extern_type(ty: Type) -> bool {
    ty.kind == TypeKind::Externref || is_ref_to_abstract(ty, TypeKind::Externref)
}

/// O tipo heap abstrato de `ty` (`anyref`, `eqref`, `structref`, `arrayref`), `None` para os demais.
fn abstract_gc_kind(ty: Type) -> Option<TypeKind> {
    if ty.kind == TypeKind::Anyref {
        return Some(TypeKind::Anyref);
    }
    match ty.index {
        TypeIndex::Abstract(kind @ (TypeKind::Anyref | TypeKind::Eqref | TypeKind::Structref | TypeKind::Arrayref)) if ty.is_ref() => Some(kind),
        _ => None,
    }
}

/// Um objeto GC embrulhado em `value` para um tipo de referência, sem instância (Table, Global, importação): os tipos
/// abstratos conferem pela forma da célula, os definidos pelo RTT canônico global.
fn gc_object_to_wasm(value: JSValue, ty: Type) -> Result<u64, Thrown> {
    let reference = gc_object_of(value).ok_or_else(|| Thrown::type_error(REFERENCE_MISMATCH_MESSAGE))?;
    let matches = match abstract_gc_kind(ty) {
        Some(kind) => gc_abstract_matches(reference, kind) == Some(true),
        None => reference_matches_type(reference, ty),
    };
    if matches { Ok(reference) } else { Err(Thrown::type_error(REFERENCE_MISMATCH_MESSAGE)) }
}

const REFERENCE_MISMATCH_MESSAGE: &str = "Argument value did not match the reference type";

/// O bit que marca uma referência a um valor JS que não é objeto GC (o invólucro de `externref`, que o
/// `any.convert_extern` mantém como está): `EXTERN_REF_TAG | índice` em `EXTERN_VALUES`.
const EXTERN_REF_TAG: u64 = 1 << 45;

thread_local! {
    /// Os valores JS internados por `intern_extern_value` (sem GC, como os objetos GC).
    static EXTERN_VALUES: RefCell<Vec<JSValue>> = const { RefCell::new(Vec::new()) };
    /// O valor codificado -> o índice em `EXTERN_VALUES`, para o mesmo valor dar os mesmos bits (`ref.eq`).
    static EXTERN_INDEX: RefCell<HashMap<i64, usize>> = RefCell::new(HashMap::new());
    /// Os valores JS que as instâncias seguram fora das células: as funções importadas (o `importFunction` que o
    /// `JSWebAssemblyInstance` visita) e as exceções JS que atravessam o wasm sem virar `externref` (podem ficar
    /// num `exnref`). A instância é `Rc` fora do coletor, então eles vivem como os `externref` internados.
    static INSTANCE_VALUES: RefCell<Vec<JSValue>> = const { RefCell::new(Vec::new()) };
}

/// `externref` (nulo ou não) e `(ref extern)`, `anyref` e `(ref any)`.
fn accepts_any_js_value(ty: Type) -> bool {
    ty.kind == TypeKind::Externref
        || ty.kind == TypeKind::Anyref
        || is_ref_to_abstract(ty, TypeKind::Externref)
        || is_ref_to_abstract(ty, TypeKind::Anyref)
}

/// A referência de um valor omitido (ou `undefined` no construtor): `undefined` do JS em `externref`, nula nos demais
/// (medido no bun 1.4.2: `new Table({element:'externref',initial:1}).get(0)` é `undefined`, a de `anyfunc` é `null`).
fn default_reference(ty: Type) -> u64 {
    if is_extern_type(ty) { intern_extern_value(js_undefined()) } else { null_ref() }
}

/// Os bits de uma referência para o valor JS `value`.
fn intern_extern_value(value: JSValue) -> u64 {
    let key = value.encode();
    let index = EXTERN_INDEX.with(|known| {
        *known.borrow_mut().entry(key).or_insert_with(|| EXTERN_VALUES.with(|values| {
            let mut values = values.borrow_mut();
            values.push(value);
            values.len() - 1
        }))
    });
    EXTERN_REF_TAG | index as u64
}

/// O valor JS por trás de `bits`, se for uma referência internada.
fn extern_value_of(bits: u64) -> Option<JSValue> {
    if bits >> 32 != EXTERN_REF_TAG >> 32 {
        return None;
    }
    EXTERN_VALUES.with(|values| values.borrow().get(bits as u32 as usize).copied())
}

/// `toJSValue(globalObject, type, bits)`: uma referência a objeto GC vira o objeto opaco único do registro.
pub(crate) fn to_js_value(bits: u64, ty: Type) -> Result<JSValue, Thrown> {
    let value = match ty.kind {
        TypeKind::I32 => js_number_i32(bits as u32 as i32),
        TypeKind::I64 => make_big_int_from_i64(bits as i64),
        TypeKind::F32 => js_number(f64::from(f32::from_bits(bits as u32))),
        TypeKind::F64 => js_number(f64::from_bits(bits)),
        _ if bits == null_ref() => js_null(),
        _ if gc_ref_index(bits).is_some() => gc_object_for(&current_global_object(), bits).unwrap_or_else(js_undefined),
        _ if extern_value_of(bits).is_some() => extern_value_of(bits).unwrap_or_else(js_undefined),
        // `i31ref` chega ao JS como Number com sinal: os 31 bits estendidos (`-1` volta `-1`, medido no bun 1.4.2).
        _ if i31_ref_value(bits).is_some() => js_number_i32(((i31_ref_value(bits).unwrap_or(0) << 1) as i32) >> 1),
        _ if func_ref_target(bits).is_some() => return func_ref_to_js(bits),
        // `toJSValue` termina em `RELEASE_ASSERT_NOT_REACHED()` para tudo que não é número, BigInt ou referência.
        _ => unreachable!("RELEASE_ASSERT_NOT_REACHED: toJSValue com tipo/valor sem representação em JS (exnref, v128)"),
    };
    if value.is_empty() { Err(Thrown::Pending) } else { Ok(value) }
}

/// Os resultados de uma função: `undefined`, o valor, ou um array (multi-valor).
pub(crate) fn results_to_js(global_object: &JSGlobalObject, returns: &[Type], results: &[u64]) -> HostResult {
    match returns {
        [] => Ok(js_undefined()),
        [ty] => to_js_value(results[0], *ty),
        _ => {
            let items = returns.iter().zip(results).map(|(ty, bits)| to_js_value(*bits, *ty)).collect::<Result<Vec<_>, _>>()?;
            Ok(array_of(global_object, &items))
        }
    }
}

/// `WasmError` -> o que o JS lança. Uma exceção de JS já pendente (importação que lançou) passa intacta.
pub(crate) fn wasm_error_to_thrown(global_object: &JSGlobalObject, error: WasmError, call: Option<&HostCall>) -> Thrown {
    // A pilha do trap (se `run_frames` fotografou uma) vale só durante a criação do erro abaixo.
    let _trap_stack = crate::wasm::wasm_call_stack::hold_trap_stack();
    if global_object.vm().exception().is_some() {
        return Thrown::Pending;
    }
    match error {
        WasmError::Link(message) => Thrown::WebAssembly(WasmErrorKind::Link, message),
        WasmError::Runtime(message) => {
            // `createJSWebAssemblyRuntimeError` roda com os frames wasm ainda vivos e o frame JS chamador no
            // topo: o erro leva o `(evaluating '...')` dele e um `at unknown` por quadro wasm do trap.
            let message = append_default_source_to_native_message(global_object, &message);
            let error = crate::runtime::wasm_errors::create_wasm_error(global_object, WasmErrorKind::Runtime, &message);
            // A pilha nasce agora, com a fotografia do trap ainda de pé: o desenrolar que captura a das exceções
            // sem pilha roda depois que a guarda cai, e então os quadros wasm já não existem.
            if let Some(frames) = call.and_then(|call| call.capture_stack_frames_with_native(global_object)) {
                error.set_pending_stack(frames);
            }
            throw_thrown_value(global_object, error.as_value());
            Thrown::Pending
        }
        WasmError::Type(message) => Thrown::TypeError(message),
        WasmError::OutOfMemory => Thrown::OutOfMemory,
        WasmError::Exception { tag, payload } => {
            // A `JSTag` volta ao JS como o valor original, sem embrulho.
            if crate::runtime::js_web_assembly_tag::is_js_tag(&tag.0) {
                return match payload.first().map(|bits| to_js_value(*bits, externref_type())) {
                    Some(Ok(value)) => {
                        throw_thrown_value(global_object, value);
                        Thrown::Pending
                    }
                    Some(Err(thrown)) => thrown,
                    // O tipo da `JSTag` é `(externref) -> ()`: todo lançamento por ela leva exatamente um valor.
                    None => unreachable!("JSTag sem payload: o tipo (externref) garante um valor"),
                };
            }
            match crate::runtime::js_web_assembly_exception::create_exception(global_object, &tag.0, &payload) {
                Ok(exception) => {
                    throw_thrown_value(global_object, exception);
                    Thrown::Pending
                }
                Err(thrown) => thrown,
            }
        }
        // `runWebAssemblySuspendingFunction` lança `SuspendError` antes de suspender se `!vm.topJSPIContext`
        // (`outside_promising_refusal`), e o IPInt só deixa `Suspend` sair pelo `Completion::Suspended`.
        WasmError::Suspend(_) => unreachable!("Suspending() wrapper called outside of a promising() context: recusado antes de suspender"),
        WasmError::JsException(thrown) => match thrown.0.downcast_ref::<JSValue>() {
            Some(value) => {
                throw_thrown_value(global_object, *value);
                Thrown::Pending
            }
            // `capture_pending_exception` só embrulha o `JSValue` da exceção pendente.
            None => unreachable!("JsException capturada sem JSValue: invariante de capture_pending_exception"),
        },
    }
}

/// `throwException(globalObject, scope, value)` para um valor JS qualquer.
pub(crate) fn throw_thrown_value(global_object: &JSGlobalObject, value: JSValue) {
    let mut scope = crate::runtime::throw_scope::ThrowScope::new(global_object.vm());
    crate::runtime::throw_scope::throw_exception(global_object, &mut scope, value);
}

/// A exceção de JS pendente que atravessa o wasm (`WasmToJS` + `JSTag`): sai do `VM` e vira
/// `WasmError::JsException`; um `WebAssembly.Exception` volta a ser a exceção wasm com a mesma tag.
pub(crate) fn capture_pending_exception(global_object: &JSGlobalObject) -> WasmError {
    let vm = global_object.vm();
    let Some(exception) = vm.exception() else {
        return WasmError::Runtime("exception thrown by an imported function".to_string());
    };
    if vm.is_termination_exception(&exception) {
        return WasmError::Runtime("terminated".to_string());
    }
    let value = exception.value();
    vm.clear_exception();
    if let Some((tag, values)) = crate::runtime::js_web_assembly_exception::exception_parts(value) {
        let parameters = tag.borrow().parameters.clone();
        let bits: Result<Vec<u64>, Thrown> =
            values.iter().zip(&parameters).map(|(value, ty)| to_wasm_value(global_object, *value, *ty)).collect();
        if let Ok(payload) = bits {
            return WasmError::Exception { tag: crate::wasm::wasm_instance::TagRef(tag), payload };
        }
        vm.clear_exception();
    }
    // Um valor de JS que não é `WebAssembly.Exception` atravessa o wasm como exceção da `JSTag` (payload: o
    // valor, como `externref`), que `catch` com a tag importada, `catch_all` e `catch_all_ref` capturam.
    if let Ok(bits) = to_wasm_value(global_object, value, externref_type()) {
        return WasmError::Exception { tag: crate::wasm::wasm_instance::TagRef(crate::runtime::js_web_assembly_tag::js_tag()), payload: vec![bits] };
    }
    vm.clear_exception();
    INSTANCE_VALUES.with(|values| values.borrow_mut().push(value));
    WasmError::JsException(crate::wasm::wasm_instance::JsThrown(Rc::new(value)))
}

/// O que uma função exportada recebe de JS: os argumentos convertidos em bits (`JSToWasm`).
pub(crate) fn exported_call_bits(global_object: &JSGlobalObject, target: &ExportedFunction, call: &HostCall) -> Result<Vec<u64>, Thrown> {
    // `v128` não cruza a fronteira com o JS: o `JSToWasm` do JSC lança antes de converter qualquer argumento.
    if target.arguments.iter().chain(&target.returns).any(|ty| ty.is_v128()) {
        return Err(Thrown::type_error(error_message_for_exception_type(ExceptionType::TypeErrorInvalidValueUse)));
    }
    let mut bits = Vec::with_capacity(target.arguments.len());
    for (position, ty) in target.arguments.iter().enumerate() {
        bits.push(to_wasm_value(global_object, call.argument(position), *ty)?);
    }
    Ok(bits)
}

/// A função exportada que `value` é, se for uma.
pub(crate) fn exported_target(value: JSValue) -> Option<Rc<ExportedFunction>> {
    let JSValue::Cell(cell_id) = value else {
        return None;
    };
    EXPORTED_FUNCTIONS.with(|table| table.borrow().get(&cell_id).cloned())
}

/// O corpo de toda função exportada (`WebAssemblyFunction::call`).
fn exported_function_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let target = EXPORTED_FUNCTIONS
        .with(|table| table.borrow().get(&call.callee()).cloned())
        .ok_or_else(|| Thrown::type_error("WebAssembly function is not registered"))?;
    let bits = exported_call_bits(global_object, &target, call)?;
    // Uma chamada vinda de JS não é a entrada direta de um `promising`: um `Suspending` dentro dela é erro.
    let anchor = crate::wasm::wasm_call_stack::enter_from_js(call.native_frame_registers());
    let outcome = crate::runtime::js_web_assembly_jspi::with_direct_entry(false, || target.instance.invoke(target.index, &bits));
    drop(anchor);
    match outcome {
        Ok(results) => results_to_js(global_object, &target.returns, &results),
        Err(error) => Err(wasm_error_to_thrown(global_object, error, Some(call))),
    }
}

host_function!(exported_function, exported_function_body);

/// `JSWebAssemblyInstance::ensureFunctionWrapper` para uma função própria da instância: o nome é o índice
/// no espaço de funções e o `length` é o número de parâmetros.
/// `Instance::getFunctionWrapper`/`setFunctionWrapper`: o wrapper é único por função (`t.get(0) === exports.add`).
fn exported_function_wrapper(global_object: &JSGlobalObject, instance: &Rc<Instance>, index: u32) -> JSValue {
    let reference = func_ref(instance.id(), index);
    if let Some(cached) = function_wrapper(reference) {
        return JSValue::decode(cached as i64);
    }
    let value = create_exported_function(global_object, instance, index);
    set_function_wrapper(reference, value.encode() as u64);
    value
}

fn create_exported_function(global_object: &JSGlobalObject, instance: &Rc<Instance>, index: u32) -> JSValue {
    let (arguments, returns) = function_signature(instance.info(), index);
    // A função importada do JS (a de outra instância wasm já chega com o funcref da dona) ganha o wrapper de nome
    // vazio, não o índice (medido no bun 1.4.2: `fn:/0` para o import reexportado ou tirado de `ref.func`).
    let imported = (index as usize) < instance.info().import_function_type_signature_indices.len();
    let name = if imported { String::new() } else { index.to_string() };
    let function = JSFunction::create_native(
        global_object.vm(),
        global_object,
        arguments.len() as u32,
        &WtfString::from_utf8(name.as_bytes()),
        exported_function,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    );
    let value = function.as_value();
    if let JSValue::Cell(cell_id) = value {
        let target = ExportedFunction { instance: Rc::clone(instance), index, arguments, returns };
        EXPORTED_FUNCTIONS.with(|table| table.borrow_mut().insert(cell_id, Rc::new(target)));
    }
    value
}

/// `WasmToJS` com mais de um resultado: `iterableToList(result)`, o tamanho precisa bater com o tipo
/// declarado ("Incorrect number of values returned to Wasm from JS") e cada valor é convertido pelo seu tipo.
pub(crate) fn multi_result_values(global_object: &JSGlobalObject, result: JSValue, returns: &[Type]) -> Result<Vec<u64>, Thrown> {
    use crate::runtime::iterator_operations::{iterator_for_iterable, iterator_step, iterator_value};
    let record = iterator_for_iterable(global_object, result)?;
    let mut items = Vec::with_capacity(returns.len());
    while let Some(step) = iterator_step(global_object, record)? {
        items.push(iterator_value(global_object, step)?);
    }
    if items.len() != returns.len() {
        return Err(Thrown::TypeError("Incorrect number of values returned to Wasm from JS".to_string()));
    }
    items.into_iter().zip(returns).map(|(item, ty)| to_wasm_value(global_object, item, *ty)).collect()
}

/// A importação de função (`WasmToJS`): uma função de outra instância é chamada direto; qualquer outra
/// função JS é chamada com `this` indefinido e o resultado convertido pelo tipo declarado.
fn host_function_for(callee: JSValue, arguments: Vec<Type>, returns: Vec<Type>) -> HostFunction {
    INSTANCE_VALUES.with(|values| values.borrow_mut().push(callee));
    Rc::new(move |bits: &[u64]| {
        if let JSValue::Cell(cell_id) = callee {
            if let Some(target) = EXPORTED_FUNCTIONS.with(|table| table.borrow().get(&cell_id).cloned()) {
                // Chamada direta entre instâncias: nenhum frame de JS no meio, a marca de entrada direta é mantida.
                // Uma suspensão (JSPI) da instância chamada atravessa esta fronteira: a pilha cobre as duas.
                return match target.instance.invoke_resumable(target.index, bits) {
                    Ok(Completion::Done(results)) => Ok(results),
                    Ok(Completion::Suspended(suspender, request)) => {
                        Err(WasmError::Suspend(crate::wasm::wasm_instance::JsThrown(Rc::new(NestedSuspension {
                            owner: target.instance.clone(),
                            suspender: RefCell::new(Some(suspender)),
                            request,
                        }))))
                    }
                    Err(error) => Err(error),
                };
            }
        }
        let global_object = current_global_object();
        // `WebAssembly.Suspending`: chama a função embrulhada, e uma promessa de volta suspende a wasm.
        let suspending = crate::runtime::js_web_assembly_jspi::suspending_function(callee);
        if suspending.is_some() {
            if let Some(message) = crate::runtime::js_web_assembly_jspi::outside_promising_refusal(&global_object) {
                throw_thrown(&global_object, Thrown::WebAssembly(WasmErrorKind::Suspend, message));
                return Err(capture_pending_exception(&global_object));
            }
        }
        let mut values = Vec::with_capacity(arguments.len());
        for (bits, ty) in bits.iter().zip(&arguments) {
            match to_js_value(*bits, *ty) {
                Ok(value) => values.push(value),
                Err(thrown) => {
                    throw_thrown(&global_object, thrown);
                    return Err(capture_pending_exception(&global_object));
                }
            }
        }
        // `None`: a exceção de JS pendente no `VM` atravessa o wasm como `WasmError::JsException`.
        let Some(result) = call_function(&global_object, suspending.unwrap_or(callee), js_undefined(), &values) else {
            return Err(capture_pending_exception(&global_object));
        };
        if suspending.is_some() {
            // Depois de chamar a função (como o JSC): frame de JS entre a importação e o `promising` é `SuspendError`;
            // senão suspende sempre, mesmo com valor ou promessa já assentada.
            if let Some(message) = crate::runtime::js_web_assembly_jspi::js_frames_refusal() {
                throw_thrown(&global_object, Thrown::WebAssembly(WasmErrorKind::Suspend, message.to_string()));
                return Err(capture_pending_exception(&global_object));
            }
            let promise = crate::runtime::js_web_assembly_jspi::promise_for_suspension(&global_object, result);
            let request = crate::runtime::js_web_assembly_jspi::SuspendRequest { promise, returns: returns.clone() };
            return Err(WasmError::Suspend(crate::wasm::wasm_instance::JsThrown(Rc::new(request))));
        }
        let converted = match returns.as_slice() {
            [] => Ok(Vec::new()),
            [ty] => to_wasm_value(&global_object, result, *ty).map(|bits| vec![bits]),
            _ => multi_result_values(&global_object, result, &returns),
        };
        converted.map_err(|thrown| {
            throw_thrown(&global_object, thrown);
            capture_pending_exception(&global_object)
        })
    })
}

/// O valor JS de uma importação de global (`WebAssemblyModuleRecord::link`, caso `Global` sem
/// `WebAssembly.Global`): número ou BigInt, só imutável.
fn global_import(import: &Import, info: &ModuleInformation, value: JSValue) -> Result<ImportValue, Thrown> {
    let link = |after: &str| Thrown::WebAssembly(WasmErrorKind::Link, import_fail_message(import, "imported global", after));
    let declared = &info.globals[import.kind_index as usize];
    if declared.mutability == Mutability::Mutable {
        return Err(link("must be a WebAssembly.Global object since it is mutable"));
    }
    let ty = declared.ty;
    let bits = match ty.kind {
        TypeKind::I64 => {
            if !value.is_big_int() {
                return Err(link("must be a BigInt"));
            }
            to_big_int64_value(value) as u64
        }
        TypeKind::I32 | TypeKind::F32 | TypeKind::F64 => {
            if !value.is_number() {
                return Err(link("must be a number"));
            }
            to_wasm_value(&current_global_object(), value, ty)?
        }
        TypeKind::V128 => return Err(link("cannot be v128")),
        // Medido no bun 1.4.2: `funcref` e `(ref func)` exigem função wasm exportada (e `null` só quando anulável);
        // `externref` aceita qualquer valor, `(ref extern)` recusa só `null`; tipo definido confere o RTT.
        _ if is_abstract_funcref(ty) => {
            if value.is_null() && ty.is_nullable() {
                null_ref()
            } else if exported_target(value).is_some() {
                exported_func_ref(value)
            } else if ty.is_nullable() {
                return Err(link("must be a wasm exported function or null"));
            } else {
                return Err(link("must be a wasm exported function"));
            }
        }
        _ if value.is_null() && ty.is_nullable() => null_ref(),
        _ if value.is_null() && is_extern_type(ty) => return Err(link("must be a non-null value")),
        _ => to_wasm_value(&current_global_object(), value, ty).map_err(|error| match error {
            Thrown::TypeError(_) => link(REFERENCE_MISMATCH_MESSAGE),
            other => other,
        })?,
    };
    Ok(ImportValue::Global(Rc::new(RefCell::new(Global::new(ty, Mutability::Immutable, bits)))))
}

/// A leitura das importações na ordem do módulo (`WebAssemblyModuleRecord::link`, modo `FromJS`). `wasm_imports`
/// recebe, por índice no espaço de funções, o wrapper de cada função importada que já é wasm exportada.
fn read_imports(
    global_object: &JSGlobalObject,
    info: &ModuleInformation,
    import_object: JSValue,
    wasm_imports: &mut Vec<(u32, JSValue)>,
) -> Result<Vec<ImportValue>, Thrown> {
    let vm = global_object.vm();
    if import_object.is_undefined() {
        if !info.imports.is_empty() {
            return Err(Thrown::type_error(NO_IMPORTS_OBJECT_MESSAGE));
        }
        return Ok(Vec::new());
    }
    if !import_object.is_object() {
        return Err(Thrown::type_error("second argument to WebAssembly.Instance must be undefined or an Object"));
    }
    let mut values = Vec::with_capacity(info.imports.len());
    for import in &info.imports {
        let module_value = get_value_property(global_object, import_object, &prop(vm, &import.module))?;
        if !module_value.is_object() {
            return Err(Thrown::TypeError(import_fail_message(import, "import", "must be an object")));
        }
        let value = get_value_property(global_object, module_value, &prop(vm, &import.field))?;
        let link = |before: &str, after: &str| Thrown::WebAssembly(WasmErrorKind::Link, import_fail_message(import, before, after));
        values.push(match import.kind {
            ExternalKind::Function => {
                if !value.is_callable() {
                    return Err(link("import function", "must be callable"));
                }
                let (arguments, returns) = function_signature(info, import.kind_index);
                if let JSValue::Cell(cell_id) = value {
                    if let Some(target) = EXPORTED_FUNCTIONS.with(|table| table.borrow().get(&cell_id).cloned()) {
                        if target.arguments != arguments || target.returns != returns {
                            return Err(link("imported function", "signature doesn't match the provided WebAssembly function's signature"));
                        }
                        // Só a função própria do módulo dona é compartilhada: o wrapper de um import do JS, reexportado
                        // por outra instância, é novo a cada instância (medido no bun: `inst(i.r).r !== i.r`).
                        if (target.index as usize) >= target.instance.info().import_function_type_signature_indices.len() {
                            wasm_imports.push((import.kind_index, value));
                        }
                    }
                }
                ImportValue::Function(host_function_for(value, arguments, returns))
            }
            ExternalKind::Global => match global_from_value(value) {
                Some(global) => ImportValue::Global(global),
                None => global_import(import, info, value)?,
            },
            ExternalKind::Memory => match memory_from_value(value) {
                Some(memory) => ImportValue::Memory(memory),
                None => return Err(link("Memory import", "is not an instance of WebAssembly.Memory")),
            },
            ExternalKind::Table => match table_from_value(value) {
                Some(table) => ImportValue::Table(table),
                None => return Err(link("Table import", "is not an instance of WebAssembly.Table")),
            },
            ExternalKind::Exception => match tag_from_value(value) {
                Some(tag) => ImportValue::Tag(tag),
                None => return Err(link("Tag import", "is not an instance of WebAssembly.Tag")),
            },
        });
    }
    Ok(values)
}

/// O objeto `exports`: protótipo nulo, uma propriedade por export, congelado (`Object.freeze`).
fn create_exports_object(global_object: &JSGlobalObject, instance: &Rc<Instance>, info: &ModuleInformation) -> Result<JSValue, Thrown> {
    let vm = global_object.vm();
    let structure = JSObject::create_structure(vm, Some(global_object), js_null());
    let exports = JSObject::allocate(vm, &structure);
    exports.finish_creation(vm);
    for export in &info.exports {
        let value = match export.kind {
            ExternalKind::Function => exported_function_wrapper(global_object, instance, export.kind_index),
            ExternalKind::Memory => memory_wrapper(global_object, &instance.memory(export.kind_index as usize)),
            ExternalKind::Table => table_wrapper(global_object, &instance.table(export.kind_index as usize)),
            ExternalKind::Global => global_wrapper(global_object, &instance.global(export.kind_index as usize)),
            ExternalKind::Exception => tag_wrapper(global_object, &instance.tag(export.kind_index as usize)),
        };
        put(global_object, &exports, &export.field, value);
    }
    object_constructor_freeze(global_object, &exports)?;
    Ok(exports.as_value())
}

fn call_instance_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(Thrown::type_error("calling WebAssembly.Instance constructor without new is invalid"))
}

/// `constructJSWebAssemblyInstance`.
fn construct_instance_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    construct_instance(global_object, call, |_| {
        let info = IntlInstance::from_value(&call.argument(0))
            .and_then(|cell| cell.state::<ModuleState>().map(|state| Rc::clone(&state.info)))
            .ok_or_else(|| throw_type_error_with_source(global_object, "first argument to WebAssembly.Instance must be a WebAssembly.Module"))?;
        Ok(Box::new(link_instance_in_call(global_object, &info, call.argument(1))?))
    })
}

/// `WebAssemblyModuleRecord::link` + `evaluate`: lê as importações, instancia e monta o `exports`.
fn link_instance(global_object: &JSGlobalObject, info: &Rc<ModuleInformation>, import_object: JSValue) -> Result<InstanceState, Thrown> {
    let mut wasm_imports = Vec::new();
    let imports = read_imports(global_object, info, import_object, &mut wasm_imports)?;
    let instance = match Instance::instantiate(Rc::clone(info), imports) {
        Ok(instance) => Rc::new(instance),
        Err(error) => return Err(wasm_error_to_thrown(global_object, error, None)),
    };
    crate::wasm::wasm_instance::register_instance(&instance);
    // A função wasm importada é a da dona: reexportada ou lida pela tabela, é o mesmo wrapper (`i.r === a.f`).
    for (index, wrapper) in wasm_imports {
        set_function_wrapper(func_ref(instance.id(), index), wrapper.encode() as u64);
    }
    let exports = create_exports_object(global_object, &instance, info)?;
    Ok(InstanceState { exports })
}

/// `link_instance` dentro da chamada nativa (`new WebAssembly.Instance`, `WebAssembly.instantiate(Module)`): os
/// erros do link ganham o ` (evaluating '...')` da chamada, como no C++, que os cria com `defaultSourceAppender`
/// ainda no quadro dela. Só o `instantiate(bytes)` liga depois da compilação assíncrona, sem quadro (medido no bun).
/// O módulo pede importações e não veio objeto de importações. No JSC é o único erro do link criado sem o appender
/// de fonte: nunca leva o `(evaluating '...')` (medido no bun 1.4.2).
const NO_IMPORTS_OBJECT_MESSAGE: &str =
    "can't make WebAssembly.Instance because there is no imports Object and the WebAssembly.Module requires imports";

fn link_instance_in_call(global_object: &JSGlobalObject, info: &Rc<ModuleInformation>, import_object: JSValue) -> Result<InstanceState, Thrown> {
    link_instance(global_object, info, import_object).map_err(|thrown| {
        let with_source = |message: &str| wtf_to_rust(&append_default_source_to_native_message(global_object, message));
        match thrown {
            Thrown::WebAssembly(kind, message) => Thrown::WebAssembly(kind, with_source(&message)),
            Thrown::TypeError(message) if message != NO_IMPORTS_OBJECT_MESSAGE => Thrown::TypeError(with_source(&message)),
            other => other,
        }
    })
}

/// O getter `WebAssembly.Instance.prototype.exports` (`webAssemblyInstanceProtoGetterExports`: a mensagem do
/// `getInstance`, medida no bun 1.4.2 com `undefined`, primitivo, objeto comum e o próprio protótipo).
fn instance_exports_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<InstanceState, _>(
        call.this_value(),
        "expected |this| value to be an instance of WebAssembly.Instance",
        |state, _| Ok(state.exports),
    )
}

// ---------------------------------------------------------------------------------------------
// WebAssembly.Memory, WebAssembly.Table, WebAssembly.Global
// ---------------------------------------------------------------------------------------------

/// `JSWebAssemblyMemory`: a memória e o `ArrayBuffer` que o `buffer` entregou por último.
struct MemoryState {
    memory: Rc<RefCell<Memory>>,
    buffer: RefCell<Option<ArrayBufferRef>>,
}

/// `JSWebAssemblyTable`.
struct TableState {
    table: Rc<RefCell<Table>>,
}

/// `JSWebAssemblyGlobal`.
struct GlobalState {
    global: Rc<RefCell<Global>>,
}

thread_local! {
    /// A estrutura das instâncias de `Memory`, `Table` e `Global` (para embrulhar o que um export entrega).
    static WRAPPER_STRUCTURES: RefCell<HashMap<&'static str, StructureRef>> = RefCell::new(HashMap::new());
    /// Nome da classe -> o construtor criado em `install_web_assembly`, entregue pelos callbacks de `webAssemblyTable`.
    static CLASS_CONSTRUCTORS: RefCell<HashMap<&'static str, JSValue>> = RefCell::new(HashMap::new());
    /// Endereço do objeto wasm (`Rc::as_ptr`) -> o objeto JS que o embrulha, para `exports.mem === exports.mem`
    /// e para o objeto importado ser o mesmo que foi exportado. As entradas nunca saem (sem GC).
    static WRAPPERS: RefCell<HashMap<usize, JSValue>> = RefCell::new(HashMap::new());
}

pub(crate) fn wrapper_key<T>(object: &Rc<RefCell<T>>) -> usize {
    Rc::as_ptr(object) as usize
}

/// `JSWebAssemblyX::create` para um objeto que veio de dentro de uma instância (export): o embrulho que já
/// existe, ou um novo com a estrutura da classe.
fn wrapper_for(global_object: &JSGlobalObject, class: &'static str, key: usize, state: impl FnOnce() -> Box<dyn std::any::Any>) -> JSValue {
    if let Some(value) = WRAPPERS.with(|wrappers| wrappers.borrow().get(&key).copied()) {
        return value;
    }
    let structure = WRAPPER_STRUCTURES.with(|structures| structures.borrow().get(class).cloned()).expect("classe wasm não instalada");
    let value = IntlInstance::create(global_object.vm(), &structure, state()).as_value();
    WRAPPERS.with(|wrappers| wrappers.borrow_mut().insert(key, value));
    value
}

/// O construtor de uma classe: estrutura derivada do `new.target`, estado criado por `make` (com a chave do
/// objeto wasm), e o registro do embrulho.
pub(crate) fn construct_wrapper(
    global_object: &JSGlobalObject,
    call: &HostCall,
    make: impl FnOnce() -> Result<(usize, Box<dyn std::any::Any>), Thrown>,
) -> HostResult {
    let structure = derived_structure(global_object, call, IntlInstance::create_structure)?;
    let (key, state) = make()?;
    let value = IntlInstance::create(global_object.vm(), &structure, state).as_value();
    WRAPPERS.with(|wrappers| wrappers.borrow_mut().insert(key, value));
    Ok(value)
}

fn memory_wrapper(global_object: &JSGlobalObject, memory: &Rc<RefCell<Memory>>) -> JSValue {
    wrapper_for(global_object, "Memory", wrapper_key(memory), || {
        Box::new(MemoryState { memory: Rc::clone(memory), buffer: RefCell::new(None) })
    })
}

fn table_wrapper(global_object: &JSGlobalObject, table: &Rc<RefCell<Table>>) -> JSValue {
    wrapper_for(global_object, "Table", wrapper_key(table), || Box::new(TableState { table: Rc::clone(table) }))
}

fn global_wrapper(global_object: &JSGlobalObject, global: &Rc<RefCell<Global>>) -> JSValue {
    wrapper_for(global_object, "Global", wrapper_key(global), || Box::new(GlobalState { global: Rc::clone(global) }))
}

/// O `WebAssembly.Tag` de um export `Tag` (o embrulho que já existe, ou um novo).
pub(crate) fn tag_wrapper(global_object: &JSGlobalObject, tag: &Rc<RefCell<TagData>>) -> JSValue {
    wrapper_for(global_object, "Tag", wrapper_key(tag), || Box::new(TagState { tag: Rc::clone(tag) }))
}

/// A estrutura registrada de uma classe instalada fora deste arquivo (`Tag`, `Exception`).
pub(crate) fn wrapper_structure(class: &'static str) -> StructureRef {
    WRAPPER_STRUCTURES.with(|structures| structures.borrow().get(class).cloned()).expect("classe wasm não instalada")
}

/// Registra a estrutura dos embrulhos de uma classe instalada fora deste arquivo (`Tag`), com o protótipo dela.
pub(crate) fn register_wrapper_structure(global_object: &JSGlobalObject, name: &'static str, prototype: &JSObject) {
    let structure = IntlInstance::create_structure(global_object.vm(), Some(global_object), prototype.as_value());
    WRAPPER_STRUCTURES.with(|structures| structures.borrow_mut().insert(name, structure));
}

/// A memória de um `WebAssembly.Memory` passado como importação.
fn memory_from_value(value: JSValue) -> Option<Rc<RefCell<Memory>>> {
    IntlInstance::from_value(&value).and_then(|cell| cell.state::<MemoryState>().map(|state| Rc::clone(&state.memory)))
}

fn table_from_value(value: JSValue) -> Option<Rc<RefCell<Table>>> {
    IntlInstance::from_value(&value).and_then(|cell| cell.state::<TableState>().map(|state| Rc::clone(&state.table)))
}

fn global_from_value(value: JSValue) -> Option<Rc<RefCell<Global>>> {
    IntlInstance::from_value(&value).and_then(|cell| cell.state::<GlobalState>().map(|state| Rc::clone(&state.global)))
}

/// `(ref null extern)`.
pub(crate) fn externref_type() -> Type {
    Type::new(TypeKind::RefNull, TypeIndex::Abstract(TypeKind::Externref))
}

/// O campo `name` do descriptor.
fn descriptor_field(global_object: &JSGlobalObject, descriptor: JSValue, name: &str) -> Result<JSValue, Thrown> {
    get_value_property(global_object, descriptor, &prop(global_object.vm(), name))
}

/// `address`: `"i32"` (o padrão) ou `"i64"`, que exige Memory64 (desligado neste porte).
fn address_type_from_descriptor(global_object: &JSGlobalObject, descriptor: JSValue, class: &str) -> Result<AddressType, Thrown> {
    let value = descriptor_field(global_object, descriptor, "address")?;
    if value.is_undefined() {
        return Ok(AddressType::new(false));
    }
    let text = wtf_to_rust(&value.to_string(global_object.vm()).value());
    check_pending(global_object)?;
    match text.as_str() {
        "i32" => Ok(AddressType::new(false)),
        "i64" => Err(Thrown::TypeError(format!("WebAssembly.{class} 'address' of 'i64' requires Memory64 to be enabled"))),
        _ => Err(Thrown::TypeError(format!("WebAssembly.{class} 'address' must be a string of value 'i32' or 'i64'"))),
    }
}

/// `initial` ou `minimum` (nunca os dois), como número de endereço.
fn initial_from_descriptor(global_object: &JSGlobalObject, descriptor: JSValue, class: &str) -> Result<u64, Thrown> {
    let initial = descriptor_field(global_object, descriptor, "initial")?;
    let minimum = descriptor_field(global_object, descriptor, "minimum")?;
    if !initial.is_undefined() && !minimum.is_undefined() {
        return Err(Thrown::TypeError(format!("WebAssembly.{class} 'initial' and 'minimum' options are specified at the same time")));
    }
    let chosen = if initial.is_undefined() { minimum } else { initial };
    to_non_wrapping_uint32(global_object, chosen)
}

// Memory

fn call_memory_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(Thrown::type_error("calling WebAssembly.Memory constructor without new is invalid"))
}

/// `constructJSWebAssemblyMemory`.
fn construct_memory_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let descriptor = call.argument(0);
    if !descriptor.is_object() {
        return Err(Thrown::type_error("WebAssembly.Memory expects its first argument to be an object"));
    }
    let address_type = address_type_from_descriptor(global_object, descriptor, "Memory")?;
    let max_pages = max_declarable_pages(address_type);
    let initial = initial_from_descriptor(global_object, descriptor, "Memory")?;
    if initial > max_pages {
        return Err(Thrown::range_error("WebAssembly.Memory 'initial' page count is too large"));
    }
    let mut maximum = PageCount::default();
    let maximum_value = descriptor_field(global_object, descriptor, "maximum")?;
    if !maximum_value.is_undefined() {
        let size = to_non_wrapping_uint32(global_object, maximum_value)?;
        if size > max_pages {
            return Err(Thrown::range_error("WebAssembly.Memory 'maximum' page count is too large"));
        }
        if initial > size {
            return Err(Thrown::range_error("'maximum' page count must be than greater than or equal to the 'initial' page count"));
        }
        maximum = PageCount::new(size);
    }
    let shared = descriptor_field(global_object, descriptor, "shared")?.to_boolean();
    check_pending(global_object)?;
    if shared && !maximum.has_value() {
        return Err(Thrown::type_error("'maximum' page count must be defined if 'shared' is true"));
    }
    construct_wrapper(global_object, call, || {
        let memory = Memory::try_create(PageCount::new(initial), maximum, shared, address_type).ok_or(Thrown::OutOfMemory)?;
        let memory = Rc::new(RefCell::new(memory));
        Ok((wrapper_key(&memory), Box::new(MemoryState { memory, buffer: RefCell::new(None) })))
    })
}

fn memory_state_error(member: &str) -> String {
    format!("WebAssembly.Memory.prototype.{member} getter called with non WebAssembly.Memory |this| value")
}

/// `JSWebAssemblyMemory::buffer`: o `ArrayBuffer` que enxerga os bytes da memória. Um buffer que ficou para
/// trás (a memória cresceu por dentro do wasm) é destacado e trocado por um novo.
///
/// `wanted` é o tipo pedido: `Some(false)` é `toFixedLengthBuffer`, `Some(true)` é `toResizableBuffer` e `None` é
/// o getter `buffer`, que devolve o que já está associado, seja qual for o tipo (só cria um de comprimento fixo
/// quando não há nenhum).
fn memory_buffer(global_object: &JSGlobalObject, state: &MemoryState, wanted: Option<bool>) -> JSValue {
    let mut cached = state.buffer.borrow_mut();
    let size = state.memory.borrow().size();
    let resizable = wanted.unwrap_or(false);
    if let Some(buffer) = cached.as_ref() {
        // Um redimensionável acompanha o `grow`; um de comprimento fixo vale só enquanto o tamanho não muda.
        let is_resizable = buffer.is_resizable_or_growable_shared();
        let kind_matches = wanted.is_none_or(|want| is_resizable == want);
        if kind_matches && !buffer.is_detached() && (is_resizable || buffer.byte_length() == size) {
            return to_js_array_buffer(global_object, buffer).as_value();
        }
        // Um `SharedArrayBuffer` não se destaca: o antigo fica com o comprimento antigo.
        if !buffer.is_shared() {
            buffer.detach();
        }
    }
    let memory = state.memory.borrow();
    let buffer = if resizable {
        // `memoryMax ? min(memoryMax.bytes(), ceilingBytes) : ceilingBytes`
        let ceiling = max_allocatable_bytes(memory.address_type());
        let declared = memory.maximum().bytes();
        let max_bytes = usize::try_from(if declared == 0 { ceiling } else { declared.min(ceiling) }).unwrap_or(usize::MAX);
        if memory.is_shared() {
            ArrayBuffer::create_from_wasm_memory_growable_shared(memory.bytes_handle(), max_bytes)
        } else {
            ArrayBuffer::create_from_wasm_memory_resizable(memory.bytes_handle(), max_bytes)
        }
    } else if memory.is_shared() {
        ArrayBuffer::create_from_wasm_memory_shared(memory.bytes_handle())
    } else {
        ArrayBuffer::create_from_wasm_memory(memory.bytes_handle())
    };
    let value = to_js_array_buffer(global_object, &buffer).as_value();
    *cached = Some(buffer);
    value
}

fn memory_buffer_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<MemoryState, _>(call.this_value(), &memory_state_error("buffer"), |state, _| Ok(memory_buffer(global_object, state, None)))
}

/// `Memory.prototype.toFixedLengthBuffer`: força um buffer de comprimento fixo (a mensagem de erro é a do getter).
fn memory_to_fixed_length_buffer_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<MemoryState, _>(call.this_value(), &memory_state_error("buffer"), |state, _| Ok(memory_buffer(global_object, state, Some(false))))
}

/// `Memory.prototype.toResizableBuffer`: um `ArrayBuffer` redimensionável (`maxByteLength` do descritor, ou 4 GiB
/// sem `maximum`) sobre os bytes da memória. Divide com `buffer` o mesmo posto: pedir o outro tipo destaca o
/// atual e cria um novo; o redimensionável sobrevive ao `grow` (medido no bun 1.4.2).
fn memory_to_resizable_buffer_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<MemoryState, _>(call.this_value(), &memory_state_error("buffer"), |state, _| Ok(memory_buffer(global_object, state, Some(true))))
}

/// `WebAssembly.Memory.prototype.grow(delta)`: devolve as páginas de antes e destaca o buffer.
fn memory_grow_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // `getMemory` antes de `addressValueToUint64`: o |this| inválido vence a conversão do argumento, e o texto é o
    // do getter de `buffer` (medido no bun 1.4.2). Memory64 está desligado no bun, então só o ramo de 32 bits.
    with_instance::<MemoryState, _>(call.this_value(), &memory_state_error("buffer"), |state, _| {
        let delta = to_non_wrapping_uint32(global_object, call.argument(0))?;
        let grown = state.memory.borrow_mut().grow(PageCount::new(delta));
        match grown {
            Ok(old) => {
                if delta > 0 {
                    let mut cached = state.buffer.borrow_mut();
                    // Numa memória compartilhada o `grow` solta qualquer buffer associado, também o crescível
                    // (o próximo `toResizableBuffer` devolve outro, medido no bun 1.4.2).
                    if cached.as_ref().is_some_and(|buffer| buffer.is_shared() || !buffer.is_resizable_non_shared()) {
                        if let Some(buffer) = cached.take() {
                            if !buffer.is_shared() {
                                buffer.detach();
                            }
                        }
                    }
                }
                Ok(js_number(old.page_count() as f64))
            }
            Err(GrowFailReason::InvalidDelta) => Err(Thrown::range_error("WebAssembly.Memory.grow expects the delta to be a valid page count")),
            Err(GrowFailReason::InvalidGrowSize) => {
                Err(Thrown::range_error("WebAssembly.Memory.grow expects the grown size to be a valid page count"))
            }
            Err(GrowFailReason::WouldExceedMaximum) => {
                Err(Thrown::range_error("WebAssembly.Memory.grow would exceed the memory's declared maximum size"))
            }
            Err(GrowFailReason::OutOfMemory) => Err(Thrown::OutOfMemory),
        }
    })
}

// Table

fn call_table_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(Thrown::type_error("calling WebAssembly.Table constructor without new is invalid"))
}

/// `constructJSWebAssemblyTable`: elemento `funcref`/`anyfunc` ou `externref`; o valor inicial só pode ser nulo
/// (referência não nula precisa do GC e da ponte entre instâncias).
fn construct_table_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let descriptor = call.argument(0);
    if !descriptor.is_object() {
        return Err(Thrown::type_error("WebAssembly.Table expects its first argument to be an object"));
    }
    let address_type = address_type_from_descriptor(global_object, descriptor, "Table")?;
    let element = descriptor_field(global_object, descriptor, "element")?;
    let element_text = wtf_to_rust(&element.to_string(global_object.vm()).value());
    check_pending(global_object)?;
    let (element_type, wasm_type) = match element_text.as_str() {
        "funcref" | "anyfunc" => (TableElementType::Funcref, funcref_type()),
        "externref" => (TableElementType::Externref, externref_type()),
        _ => {
            return Err(Thrown::type_error("WebAssembly.Table expects its 'element' field to be the string 'funcref' or 'externref'"));
        }
    };
    let initial = initial_from_descriptor(global_object, descriptor, "Table")?;
    if !Table::is_valid_length(initial) {
        return Err(Thrown::RangeError(format!("WebAssembly.Table 'initial' value is above the upper bound {MAX_TABLE_ENTRIES}")));
    }
    let maximum_value = descriptor_field(global_object, descriptor, "maximum")?;
    let mut maximum = None;
    if !maximum_value.is_undefined() {
        let size = to_non_wrapping_uint32(global_object, maximum_value)?;
        if initial > size {
            return Err(Thrown::range_error("'maximum' property must be greater than or equal to the 'initial' property"));
        }
        maximum = Some(size);
    }
    let argument = call.argument(1);
    // Medido no bun 1.4.2: só o argumento ausente vale o padrão; `undefined` explícito numa tabela de funções é recusado
    // como qualquer valor que não é função wasm (na `externref` ele é o próprio valor de preenchimento).
    let fill = if argument.is_null() {
        null_ref()
    } else if call.argument_count() < 2 {
        default_reference(wasm_type)
    } else if element_type == TableElementType::Funcref && exported_target(argument).is_none() {
        return Err(Thrown::type_error("WebAssembly.Table.prototype.constructor expects the second argument to be null or an instance of WebAssembly.Function"));
    } else {
        // Função wasm, `externref` ou objeto GC abstrato; o resto cai na mensagem de tipo de referência.
        to_wasm_value(global_object, argument, wasm_type)?
    };
    construct_wrapper(global_object, call, || {
        let mut table = Table::try_create(initial, maximum, element_type, wasm_type, address_type, null_ref())
            .ok_or_else(|| Thrown::range_error("couldn't create Table"))?;
        if fill != null_ref() {
            table.fill_range(0, fill, initial as u32);
        }
        let table = Rc::new(RefCell::new(table));
        Ok((wrapper_key(&table), Box::new(TableState { table })))
    })
}

const TABLE_THIS_MESSAGE: &str = "expected |this| value to be an instance of WebAssembly.Table";

fn table_length_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<TableState, _>(call.this_value(), TABLE_THIS_MESSAGE, |state, _| Ok(JSValue::from_u32(state.table.borrow().length())))
}

fn table_get_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<TableState, _>(call.this_value(), TABLE_THIS_MESSAGE, |state, _| {
        let index = to_non_wrapping_uint32(global_object, call.argument(0))?;
        let table = state.table.borrow();
        if index >= u64::from(table.length()) {
            return Err(Thrown::range_error("WebAssembly.Table.prototype.get expects an integer less than the length of the table"));
        }
        to_js_value(table.get(index as u32), table.wasm_type())
    })
}

/// O valor (nulo ou indefinido por omissão) que `set`/`grow` guardam: só referência nula por enquanto.
fn table_fill_value(call: &HostCall, position: usize, table_type: Type, member: &str) -> Result<u64, Thrown> {
    if call.argument_count() <= position {
        if !table_type.is_nullable() {
            return Err(Thrown::TypeError(format!("WebAssembly.Table.prototype.{member} requires the second argument for non-defaultable table type")));
        }
        return Ok(default_reference(table_type));
    }
    let value = call.argument(position);
    if value.is_null() && table_type.is_nullable() {
        return Ok(null_ref());
    }
    // Função wasm (funcref e tipo definido), `externref` e GC; o resto vira o TypeError de tipo de referência.
    to_wasm_value(&current_global_object(), value, table_type)
}

fn table_set_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<TableState, _>(call.this_value(), TABLE_THIS_MESSAGE, |state, _| {
        let index = to_non_wrapping_uint32(global_object, call.argument(0))?;
        let table_type = state.table.borrow().wasm_type();
        if index >= u64::from(state.table.borrow().length()) {
            return Err(Thrown::range_error("WebAssembly.Table.prototype.set expects an integer less than the length of the table"));
        }
        let value = table_fill_value(call, 1, table_type, "set")?;
        state.table.borrow_mut().set(index as u32, value);
        Ok(js_undefined())
    })
}

fn table_grow_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<TableState, _>(call.this_value(), TABLE_THIS_MESSAGE, |state, _| {
        let delta = to_non_wrapping_uint32(global_object, call.argument(0))?;
        let table_type = state.table.borrow().wasm_type();
        let value = table_fill_value(call, 1, table_type, "grow")?;
        match state.table.borrow_mut().grow(delta, value) {
            Some(old_length) => Ok(JSValue::from_u32(old_length)),
            None => Err(Thrown::range_error("WebAssembly.Table.prototype.grow could not grow the table")),
        }
    })
}

// Global

fn call_global_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(Thrown::type_error("calling WebAssembly.Global constructor without new is invalid"))
}

/// `constructJSWebAssemblyGlobal`: `{ value, mutable }` e o valor inicial (zero ou nulo quando omitido).
fn construct_global_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let descriptor = call.argument(0);
    if !descriptor.is_object() {
        return Err(Thrown::type_error("WebAssembly.Global expects its first argument to be an object"));
    }
    let mutable = descriptor_field(global_object, descriptor, "mutable")?.to_boolean();
    let mutability = if mutable { Mutability::Mutable } else { Mutability::Immutable };
    let value_name = descriptor_field(global_object, descriptor, "value")?;
    let value_text = wtf_to_rust(&value_name.to_string(global_object.vm()).value());
    check_pending(global_object)?;
    let ty = match value_text.as_str() {
        "i32" => TYPE_I32,
        "i64" => TYPE_I64,
        "f32" => TYPE_F32,
        "f64" => TYPE_F64,
        "anyfunc" | "funcref" => funcref_type(),
        "externref" => externref_type(),
        _ => {
            return Err(Thrown::type_error(
                "WebAssembly.Global expects its 'value' field to be the string 'i32', 'i64', 'f32', 'f64', 'anyfunc', 'funcref', or 'externref'",
            ));
        }
    };
    let argument = call.argument(1);
    let bits = if argument.is_undefined() {
        match ty.kind {
            TypeKind::I32 | TypeKind::I64 | TypeKind::F32 | TypeKind::F64 => 0,
            _ => default_reference(ty),
        }
    } else if ty.is_nullable() && !argument.is_null() {
        // `funcref`: só `null` ou função wasm (o C++ confere `isWebAssemblyHostFunction`); `externref`: qualquer valor.
        if ty.kind == TypeKind::RefNull && ty.index == TypeIndex::Abstract(TypeKind::Funcref) && exported_target(argument).is_none() {
            return Err(Thrown::type_error("Argument value did not match the reference type"));
        }
        to_wasm_value(global_object, argument, ty)?
    } else {
        to_wasm_value(global_object, argument, ty)?
    };
    construct_wrapper(global_object, call, || {
        let global = Rc::new(RefCell::new(Global::new(ty, mutability, bits)));
        Ok((wrapper_key(&global), Box::new(GlobalState { global })))
    })
}

const GLOBAL_THIS_MESSAGE: &str = "expected |this| value to be an instance of WebAssembly.Global";

/// O getter `value` e `valueOf`.
fn global_value_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<GlobalState, _>(call.this_value(), GLOBAL_THIS_MESSAGE, |state, _| {
        let global = state.global.borrow();
        to_js_value(global.get(), global.ty())
    })
}

/// O setter `value`.
fn global_set_value_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_instance::<GlobalState, _>(call.this_value(), GLOBAL_THIS_MESSAGE, |state, _| {
        let ty = state.global.borrow().ty();
        if state.global.borrow().mutability() == Mutability::Immutable {
            return Err(Thrown::type_error("WebAssembly.Global.prototype.value attempts to modify immutable global value"));
        }
        let bits = to_wasm_value(global_object, call.argument(0), ty)?;
        state.global.borrow_mut().set(bits);
        Ok(js_undefined())
    })
}

host_function!(call_web_assembly_memory, call_memory_body);
host_function!(construct_web_assembly_memory, construct_memory_body);
host_function!(web_assembly_memory_buffer, memory_buffer_body);
host_function!(web_assembly_memory_grow, memory_grow_body);
host_function!(web_assembly_memory_to_fixed_length_buffer, memory_to_fixed_length_buffer_body);
host_function!(web_assembly_memory_to_resizable_buffer, memory_to_resizable_buffer_body);
host_function!(call_web_assembly_table, call_table_body);
host_function!(construct_web_assembly_table, construct_table_body);
host_function!(web_assembly_table_length, table_length_body);
host_function!(web_assembly_table_get, table_get_body);
host_function!(web_assembly_table_set, table_set_body);
host_function!(web_assembly_table_grow, table_grow_body);
host_function!(call_web_assembly_global, call_global_body);
host_function!(construct_web_assembly_global, construct_global_body);
host_function!(web_assembly_global_value, global_value_body);
host_function!(web_assembly_global_set_value, global_set_value_body);

/// Instala `Memory`, `Table` e `Global` no objeto `WebAssembly` e guarda as estruturas das instâncias.
fn install_memory_table_global(global_object: &JSGlobalObject, namespace: &JSObject, only: &str) {
    let vm = global_object.vm();
    let classes: [(&'static str, NativeFunction, NativeFunction); 3] = [
        ("Memory", call_web_assembly_memory, construct_web_assembly_memory),
        ("Table", call_web_assembly_table, construct_web_assembly_table),
        ("Global", call_web_assembly_global, construct_web_assembly_global),
    ];
    for (name, call, construct) in classes {
        if name != only {
            continue;
        }
        let class = IntlClass { name, length: 1, has_supported_locales_of: false, call, construct };
        // Os membros entram no protótipo antes de `constructor` (bun: `grow,buffer,...,constructor`).
        let prototype = class.install_with(global_object, namespace, |prototype| match name {
            "Memory" => {
                put_enumerable_method(global_object, prototype, "grow", 1, web_assembly_memory_grow);
                put_enumerable_getter(global_object, prototype, "buffer", web_assembly_memory_buffer);
                put_enumerable_method(global_object, prototype, "toFixedLengthBuffer", 0, web_assembly_memory_to_fixed_length_buffer);
                put_enumerable_method(global_object, prototype, "toResizableBuffer", 0, web_assembly_memory_to_resizable_buffer);
            }
            "Table" => {
                put_enumerable_getter(global_object, prototype, "length", web_assembly_table_length);
                put_enumerable_method(global_object, prototype, "grow", 1, web_assembly_table_grow);
                put_enumerable_method(global_object, prototype, "get", 1, web_assembly_table_get);
                put_enumerable_method(global_object, prototype, "set", 1, web_assembly_table_set);
            }
            _ => {
                put_enumerable_method(global_object, prototype, "valueOf", 0, web_assembly_global_value);
                put_value_accessor(global_object, prototype);
            }
        });
        put_to_string_tag(vm, &prototype, &format!("WebAssembly.{name}"));
        let structure = IntlInstance::create_structure(vm, Some(global_object), prototype.as_value());
        WRAPPER_STRUCTURES.with(|structures| structures.borrow_mut().insert(name, structure));
    }
}

/// `value` de `Global.prototype`: acessor com `get value` e `set value` (`PropertyAttribute::Accessor`).
/// A função de um acessor `JSC_NATIVE_GETTER`/setter: uma `JSFunction` nativa comum cujo nome já leva o `get `/`set `
/// (o `toString` mostra `function get value()`, ao contrário do `CustomAccessor`, que mostra o nome puro).
fn native_accessor_function(global_object: &JSGlobalObject, length: u32, name: &str, function: NativeFunction) -> JSFunctionRef {
    JSFunction::create_native(
        global_object.vm(),
        global_object,
        length,
        &WtfString::from_utf8(name.as_bytes()),
        function,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    )
}

fn put_value_accessor(global_object: &JSGlobalObject, prototype: &JSObject) {
    let vm = global_object.vm();
    let getter = native_accessor_function(global_object, 0, "get value", web_assembly_global_value);
    let setter = native_accessor_function(global_object, 1, "set value", web_assembly_global_set_value);
    let accessor = GetterSetter::create_from_values(vm, getter.as_value(), setter.as_value());
    prototype.put_direct_non_index_accessor_without_transition(vm, &prop(vm, "value"), &accessor, ACCESSOR);
}

host_function!(call_web_assembly_instance, call_instance_body);
host_function!(construct_web_assembly_instance, construct_instance_body);
host_function!(web_assembly_instance_exports, instance_exports_body);
host_function!(call_web_assembly_module, call_module_body);
host_function!(construct_web_assembly_module, construct_module_body);
host_function!(web_assembly_module_exports, module_exports_body);
host_function!(web_assembly_module_imports, module_imports_body);

/// `createWebAssemblyX(vm, globalObject)` do C++ (`PropertyCallback`): a classe já existe (ver
/// `install_web_assembly`); o callback devolve o construtor e a reificação da tabela o grava em `WebAssembly`
/// com `DontEnum`.
macro_rules! lazy_class {
    ($callback:ident, $name:literal) => {
        fn $callback(_vm: &VM, _namespace: &JSObject) -> JSValue {
            CLASS_CONSTRUCTORS.with(|map| map.borrow().get($name).copied()).expect("classe wasm não instalada")
        }
    };
}

lazy_class!(create_web_assembly_compile_error, "CompileError");
lazy_class!(create_web_assembly_exception, "Exception");
lazy_class!(create_web_assembly_global, "Global");
lazy_class!(create_web_assembly_instance, "Instance");
lazy_class!(create_web_assembly_link_error, "LinkError");
lazy_class!(create_web_assembly_memory, "Memory");
lazy_class!(create_web_assembly_module, "Module");
lazy_class!(create_web_assembly_runtime_error, "RuntimeError");
lazy_class!(create_web_assembly_table, "Table");
lazy_class!(create_web_assembly_tag, "Tag");

/// `webAssemblyTableValues` de `JSWebAssembly.lut.h`, na ordem do `@begin`.
static WEB_ASSEMBLY_TABLE_VALUES: [HashTableValue; 13] = [
    lazy_entry("CompileError", create_web_assembly_compile_error),
    lazy_entry("Exception", create_web_assembly_exception),
    lazy_entry("Global", create_web_assembly_global),
    lazy_entry("Instance", create_web_assembly_instance),
    lazy_entry("LinkError", create_web_assembly_link_error),
    lazy_entry("Memory", create_web_assembly_memory),
    lazy_entry("Module", create_web_assembly_module),
    lazy_entry("RuntimeError", create_web_assembly_runtime_error),
    lazy_entry("Table", create_web_assembly_table),
    lazy_entry("Tag", create_web_assembly_tag),
    native_function_entry("compile", 0, web_assembly_compile, 1, Intrinsic::NoIntrinsic),
    native_function_entry("instantiate", 0, web_assembly_instantiate, 1, Intrinsic::NoIntrinsic),
    native_function_entry("validate", 0, web_assembly_validate, 1, Intrinsic::NoIntrinsic),
];

/// `webAssemblyTable`.
static WEB_ASSEMBLY_TABLE: HashTable = HashTable { class_for_this: None, values: &WEB_ASSEMBLY_TABLE_VALUES };

/// `const ClassInfo JSWebAssembly::s_info = { "WebAssembly"_s, &Base::s_info, &webAssemblyTable, ... }`.
pub static JS_WEB_ASSEMBLY_S_INFO: ClassInfo = ClassInfo {
    class_name: "WebAssembly",
    parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO),
    static_prop_hash_table: Some(&WEB_ASSEMBLY_TABLE),
    inherits_js_type_range: None,
};

/// `constructorTableWebAssemblyModule` de `WebAssemblyModuleConstructor.lut.h`, na ordem do `@begin`.
static WEB_ASSEMBLY_MODULE_CONSTRUCTOR_TABLE_VALUES: [HashTableValue; 3] = [
    native_function_entry("customSections", 0, web_assembly_module_custom_sections, 2, Intrinsic::NoIntrinsic),
    native_function_entry("imports", 0, web_assembly_module_imports, 1, Intrinsic::NoIntrinsic),
    native_function_entry("exports", 0, web_assembly_module_exports, 1, Intrinsic::NoIntrinsic),
];

static WEB_ASSEMBLY_MODULE_CONSTRUCTOR_TABLE: HashTable =
    HashTable { class_for_this: None, values: &WEB_ASSEMBLY_MODULE_CONSTRUCTOR_TABLE_VALUES };

/// `const ClassInfo WebAssemblyModuleConstructor::s_info` (`"Function"`, `&constructorTableWebAssemblyModule`).
pub static WEB_ASSEMBLY_MODULE_CONSTRUCTOR_S_INFO: ClassInfo = ClassInfo {
    class_name: "Function",
    parent_class: Some(&crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO),
    static_prop_hash_table: Some(&WEB_ASSEMBLY_MODULE_CONSTRUCTOR_TABLE),
    inherits_js_type_range: None,
};

/// `JSWebAssembly::finishCreation` e a propriedade global `WebAssembly`. Devolve o objeto `WebAssembly`; o
/// chamador o põe no global (`DontEnum`), sob `use_wasm`.
///
/// Como no C++, só `@@toStringTag`, `compileStreaming`, `instantiateStreaming`, `JSTag` e a JSPI entram na
/// criação; as dez classes, `compile`, `instantiate` e `validate` (`webAssemblyTable`) nascem no primeiro
/// acesso. As classes em si são criadas aqui (as estruturas dos embrulhos e dos erros precisam existir para
/// exports e erros internos, que no C++ são `LazyClassStructure`), num objeto descartável, e o callback
/// da tabela só entrega o construtor; criar a classe antes não é observável.
pub fn install_web_assembly(global_object: &JSGlobalObject) -> crate::runtime::js_object::JSObjectRef {
    let vm = global_object.vm();
    let structure = Structure::create(
        vm,
        Some(global_object),
        global_object.object_prototype().as_value(),
        TypeInfo::new(JSType::ObjectType, JSNonFinalObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE),
        &JS_WEB_ASSEMBLY_S_INFO,
    );
    let namespace = JSObject::allocate(vm, &structure);
    namespace.finish_creation(vm);
    put_to_string_tag(vm, &namespace, "WebAssembly");

    let scratch_structure = JSObject::create_structure(vm, Some(global_object), global_object.object_prototype().as_value());
    let scratch = JSObject::allocate(vm, &scratch_structure);
    scratch.finish_creation(vm);
    install_wasm_errors(global_object, &scratch, WasmErrorKind::Compile);
    crate::runtime::js_web_assembly_exception::install_exception_class(global_object, &scratch);
    install_memory_table_global(global_object, &scratch, "Global");
    install_instance_class(global_object, &scratch);
    install_wasm_errors(global_object, &scratch, WasmErrorKind::Link);
    install_memory_table_global(global_object, &scratch, "Memory");
    install_module_class(global_object, &scratch);
    install_wasm_errors(global_object, &scratch, WasmErrorKind::Runtime);
    install_memory_table_global(global_object, &scratch, "Table");
    crate::runtime::js_web_assembly_tag::install_tag_class(global_object, &scratch);
    for name in ["CompileError", "Exception", "Global", "Instance", "LinkError", "Memory", "Module", "RuntimeError", "Table", "Tag"] {
        let constructor = scratch.get_direct_by_name(vm, &prop(vm, name));
        CLASS_CONSTRUCTORS.with(|map| map.borrow_mut().insert(name, constructor));
    }
    // No bun estas duas funções são enumeráveis (atributo 0).
    for (name, length, function) in [
        ("compileStreaming", 1, web_assembly_compile_streaming as NativeFunction),
        ("instantiateStreaming", 1, web_assembly_instantiate_streaming),
    ] {
        put_enumerable_method(global_object, &namespace, name, length, function);
    }
    // Depois delas vem o acessor `JSTag` e a JSPI (`promising`, `Suspending`, `SuspendError`).
    crate::runtime::js_web_assembly_tag::install_js_tag(global_object, &namespace);
    crate::runtime::js_web_assembly_jspi::install_jspi(global_object, &namespace);
    namespace
}

/// O número de parâmetros de uma função wasm exportada, ou `None` se o valor não for uma (`promising`).
pub(crate) fn exported_function_arity(value: JSValue) -> Option<u32> {
    let JSValue::Cell(cell_id) = value else {
        return None;
    };
    EXPORTED_FUNCTIONS.with(|table| table.borrow().get(&cell_id).map(|target| target.arguments.len() as u32))
}

/// Método com atributos 0 (enumerável, gravável, configurável), como as funções estáticas de `WebAssembly`.
pub(crate) fn put_enumerable_method(global_object: &JSGlobalObject, object: &JSObject, name: &str, length: u32, function: NativeFunction) {
    let vm = global_object.vm();
    put_direct_native_function_without_transition(
        vm,
        global_object,
        object,
        &Identifier::from_span(vm, name.as_bytes()),
        length,
        function,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        0,
    );
}

/// Acessor enumerável `name` (no bun os membros de WebAssembly são enumeráveis): como `put_getter_on`, sem
/// `DONT_ENUM`.
fn put_enumerable_getter(global_object: &JSGlobalObject, object: &JSObject, name: &str, getter: NativeFunction) {
    let vm = global_object.vm();
    let function = crate::runtime::js_custom_accessor_function::create_host_custom_accessor_getter_function(vm, global_object, name, getter);
    let accessor = GetterSetter::create_from_values(vm, function.as_value(), JSValue::undefined());
    object.put_direct_non_index_accessor_without_transition(vm, &prop(vm, name), &accessor, ACCESSOR);
}

fn install_module_class(global_object: &JSGlobalObject, namespace: &JSObject) {
    let vm = global_object.vm();
    let class = IntlClass {
        name: "Module",
        length: 1,
        has_supported_locales_of: false,
        call: call_web_assembly_module,
        construct: construct_web_assembly_module,
    };
    // Ordem do bun 1.4.2: customSections, imports, exports (`constructorTableWebAssemblyModule`, reificada no
    // primeiro acesso; a tabela vem antes de length/name/prototype).
    let prototype = class.install_with_constructor_info(global_object, namespace, &WEB_ASSEMBLY_MODULE_CONSTRUCTOR_S_INFO, |_| {});
    // `IntlClass::install` escreve "Intl.Module"; o tag do wasm é "WebAssembly.Module".
    put_to_string_tag(vm, &prototype, "WebAssembly.Module");
    let module_structure = IntlInstance::create_structure(vm, Some(global_object), prototype.as_value());
    WRAPPER_STRUCTURES.with(|structures| structures.borrow_mut().insert("Module", module_structure));
}

fn install_instance_class(global_object: &JSGlobalObject, namespace: &JSObject) {
    let vm = global_object.vm();
    let instance_class = IntlClass {
        name: "Instance",
        length: 1,
        has_supported_locales_of: false,
        call: call_web_assembly_instance,
        construct: construct_web_assembly_instance,
    };
    // `exports` é `JSC_NATIVE_GETTER` no C++ (medido no bun 1.4.2: `String(get)` é `function get exports()`), não
    // `CustomAccessor` como `Memory.buffer` e `Table.length`.
    let instance_prototype = instance_class.install_with(global_object, &namespace, |prototype| {
        let getter = native_accessor_function(global_object, 0, "get exports", web_assembly_instance_exports);
        let accessor = GetterSetter::create_from_values(vm, getter.as_value(), JSValue::undefined());
        prototype.put_direct_non_index_accessor_without_transition(vm, &prop(vm, "exports"), &accessor, ACCESSOR);
    });
    put_to_string_tag(vm, &instance_prototype, "WebAssembly.Instance");
    let instance_structure = IntlInstance::create_structure(vm, Some(global_object), instance_prototype.as_value());
    WRAPPER_STRUCTURES.with(|structures| structures.borrow_mut().insert("Instance", instance_structure));
}

#[cfg(test)]
mod streaming_tests {
    use crate::api::eval::{evaluate, new_global_object, program_source, VmFinalizer};
    use crate::runtime::cell_registry::run_program;
    use crate::wtf::text::conversion_mode::ConversionMode;

    /// Roda `script` (que preenche o array global `log`) e devolve o `log` em JSON depois das microtarefas.
    fn run(script: &str) -> String {
        run_program(|| {
            let (vm, global_object) = new_global_object();
            let _finalizer = VmFinalizer::new(&vm);
            evaluate(&global_object, &program_source(&format!("globalThis.log = []; {script}"))).expect("script");
            vm.drain_microtasks();
            let value = evaluate(&global_object, &program_source("JSON.stringify(log)")).expect("log");
            String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
        })
    }

    /// Medido no bun 1.4.2: o erro do link feito na chamada nativa (`new Instance`, `instantiate(Module)`) tem o
    /// ` (evaluating '...')` da chamada; o do `instantiate(bytes)`, que liga depois da compilação, não tem. (O bun
    /// mostra o trecho reimpresso pelo transpilador, com aspas duplas; aqui é o fonte como foi escrito.)
    #[test]
    fn link_errors_carry_the_call_site_only_inside_the_call() {
        let log = run(
            "const b = new Uint8Array([0,97,115,109,1,0,0,0, 2,8,1,1,109,1,103,3,127,0]); const m = new WebAssembly.Module(b); \
             try { new WebAssembly.Instance(m, { m: { g: 'x' } }); } catch (e) { log.push(e.name + ': ' + e.message); } \
             WebAssembly.instantiate(m, { m: { g: 'x' } }).catch(e => log.push('module: ' + e.message)); \
             WebAssembly.instantiate(b, { m: { g: 'x' } }).catch(e => log.push('bytes: ' + e.message));",
        );
        assert_eq!(
            log,
            concat!(
                r#"["LinkError: imported global m:g must be a number (evaluating 'new WebAssembly.Instance(m, { m: { g: 'x' } })')","#,
                r#""module: imported global m:g must be a number (evaluating 'WebAssembly.instantiate(m, { m: { g: 'x' } })')","#,
                r#""bytes: imported global m:g must be a number"]"#
            )
        );
    }

    /// Medido no bun 1.4.2.
    #[test]
    fn response_sources_compile_and_bad_ones_reject_like_bun() {
        let log = run(
            "const b = new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0]); \
             const wasm = (type, status) => new Response(b, { status, headers: { 'content-type': type } }); \
             const note = (p) => p.then(r => log.push(Object.prototype.toString.call(r) + (r.instance ? ' inst' : '')), e => log.push(e.name + ': ' + e.message)); \
             const page = wasm('application/wasm'); Object.defineProperty(page, 'url', { value: 'https://x/a.wasm', configurable: true }); \
             note(WebAssembly.instantiateStreaming(Promise.resolve(page), {})); \
             note(WebAssembly.compileStreaming(wasm(' Application/WASM '))); \
             note(WebAssembly.compileStreaming(new Response(b))); \
             note(WebAssembly.compileStreaming(wasm('application/wasm', 404))); \
             note(WebAssembly.compileStreaming(5)); \
             const used = wasm('application/wasm'); used.arrayBuffer(); note(WebAssembly.compileStreaming(used));",
        );
        assert_eq!(
            log,
            concat!(
                r#"["TypeError: WebAssembly response has unsupported MIME type 'null'","#,
                r#""TypeError: WebAssembly response has status code 404","#,
                r#""TypeError: The \"source\" argument must be an instance of Response or an Promise resolving to Response. Received type number (5)","#,
                r#""TypeError: WebAssembly response body has already been used","#,
                r#""[object Object] inst","[object WebAssembly.Module]"]"#
            )
        );
    }

    /// Medido no bun 1.4.2: a função importada do JS e reexportada tem wrapper de nome vazio; a própria do módulo
    /// tem o índice.
    #[test]
    fn imported_js_function_wrapper_has_empty_name() {
        let log = run(
            "const b = new Uint8Array([0,97,115,109,1,0,0,0, 1,5,1,0x60,0,1,0x7f, 2,9,1,3,101,110,118,1,102,0,0, \
               3,2,1,0, 7,11,2, 1,114,0,0, 3,111,119,110,0,1, 10,6,1,4,0,0x41,7,0x0b]); \
             const e = new WebAssembly.Instance(new WebAssembly.Module(b), { env: { f: () => 1 } }).exports; \
             log.push(e.r.name, e.r.length, e.own.name, e.r());",
        );
        assert_eq!(log, r#"["",0,"1",1]"#);
    }

    /// Medido no bun 1.4.2: a função wasm importada de outra instância é a da dona, reexportada ou por `ref.func`
    /// (`i.r === a.f`, nome "0"); só o import vindo do JS ganha wrapper novo de nome vazio.
    #[test]
    fn imported_wasm_function_is_the_owner_wrapper() {
        let log = run(
            "const prov = new Uint8Array([0,97,115,109,1,0,0,0, 1,5,1,0x60,0,1,0x7f, 3,2,1,0, 7,5,1,1,102,0,0, 10,6,1,4,0,0x41,10,0x0b]); \
             const imp = new Uint8Array([0,97,115,109,1,0,0,0, 1,9,2,0x60,0,1,0x7f,0x60,0,1,0x70, 2,9,1,3,101,110,118,1,102,0,0, \
               3,2,1,1, 7,10,2,1,114,0,0,2,114,102,0,1, 9,5,1,3,0,1,0, 10,6,1,4,0,0xd2,0,0x0b]); \
             const a = new WebAssembly.Instance(new WebAssembly.Module(prov)).exports; \
             const i = new WebAssembly.Instance(new WebAssembly.Module(imp), { env: { f: a.f } }).exports; \
             const j = new WebAssembly.Instance(new WebAssembly.Module(imp), { env: { f: () => 3 } }).exports; \
             log.push(i.r === a.f, i.rf() === a.f, i.r.name, j.r.name, j.r === j.rf(), j.r());",
        );
        assert_eq!(log, r#"[true,true,"0","",true,3]"#);
    }

    /// Medido no bun 1.4.2: o wrapper de um import do JS reexportado por uma instância não é o de quem o importa
    /// depois (`inst(i.r).r !== i.r`), mas dentro de cada instância `r === rf()`.
    #[test]
    fn reexported_js_import_wrapper_is_per_instance() {
        let log = run(
            "const imp = new Uint8Array([0,97,115,109,1,0,0,0, 1,9,2,0x60,0,1,0x7f,0x60,0,1,0x70, 2,9,1,3,101,110,118,1,102,0,0, \
               3,2,1,1, 7,10,2,1,114,0,0,2,114,102,0,1, 9,5,1,3,0,1,0, 10,6,1,4,0,0xd2,0,0x0b]); \
             const inst = (f) => new WebAssembly.Instance(new WebAssembly.Module(imp), { env: { f } }).exports; \
             const j = () => 7; const i = inst(j), k = inst(i.r), l = inst(k.r); \
             log.push(l.r === i.r, l.r === j, l.rf() === i.rf(), i.rf() === i.r, k.rf() === k.r, l.r.name);",
        );
        assert_eq!(log, r#"[false,false,false,true,true,""]"#);
    }

    /// Medido no bun 1.4.2: o "sem objeto de importações" não leva `(evaluating ...)`, os outros erros do link na
    /// chamada levam; o `undefined` explícito numa tabela de funções é recusado, o ausente é o padrão.
    #[test]
    fn link_message_suffix_and_explicit_undefined_table_fill() {
        let log = run(
            "const imp = new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0, 1,5,1,0x60,0,1,0x7f, 2,9,1,3,101,110,118,1,102,0,0])); \
             const T = (f) => { try { f(); return 'ok'; } catch (e) { return e.message; } }; \
             log.push(T(() => new WebAssembly.Instance(imp)), T(() => new WebAssembly.Instance(imp, {}))); \
             log.push(T(() => new WebAssembly.Table({ element: 'anyfunc', initial: 2 }, undefined)), \
               T(() => new WebAssembly.Table({ element: 'anyfunc', initial: 2 })), \
               String(new WebAssembly.Table({ element: 'externref', initial: 1 }, undefined).get(0)));",
        );
        assert_eq!(
            log,
            concat!(
                r#"["can't make WebAssembly.Instance because there is no imports Object and the WebAssembly.Module requires imports","#,
                r#""import env:f must be an object (evaluating 'new WebAssembly.Instance(imp, {})')","#,
                r#""WebAssembly.Table.prototype.constructor expects the second argument to be null or an instance of WebAssembly.Function","#,
                r#""ok","undefined"]"#
            )
        );
    }
}
