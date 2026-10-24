//! JSPI do bun 1.4.2 (`WebAssembly.promising`, `WebAssembly.Suspending`, `WebAssembly.SuspendError`).
//!
//! FORMA ESTÁTICA completa (medida no bun): `promising` é função enumerável de `length` 0; `Suspending` é
//! classe (`length` 1, protótipo só com `constructor`, sem `@@toStringTag`) cujas instâncias são funções de
//! nome `WebAssembly.Suspending`; `SuspendError` é subclasse de `Error` (em `wasm_errors.rs`). As mensagens de
//! erro de argumento são as do bun.
//!
//! COMPORTAMENTO: `promising(f)` devolve uma função (nome `WebAssembly.promising`, `length` de `f`) que roda
//! `f` com `Instance::invoke_resumable` e entrega o resultado numa `Promise` (devolvida na hora, como no bun).
//! Uma importação `Suspending` (reconhecida em `host_function_for`) que devolve promessa faz o interpretador
//! devolver `Completion::Suspended` com os quadros; `suspend` liga `then(onFulfilled, onRejected)` na promessa da
//! importação e a reação chama `Instance::resume`: o valor é convertido ao tipo de retorno da importação, e a
//! rejeição é lançada no ponto da chamada. Uma importação `Suspending` fora de um `promising` lança `SuspendError`
//! ("outside of a promising() context"); com uma chamada vinda de JS entre ela e o `promising` (a marca
//! `DIRECT_ENTRY` cai a `false` em `exported_function_body`; a chamada direta entre instâncias a mantém) lança `SuspendError`
//! ("JavaScript frames found ...").
//!
//! Como `runWebAssemblySuspendingFunction`: o resultado que não é `JSPromise` (valor, thenable) ganha uma promessa
//! nova resolvida com ele, e a suspensão é sempre `performPromiseThen` direto na promessa (sem ler `then`),
//! mesmo já assentada, retomando numa microtarefa. O aviso de frame de JS vem depois de chamar a função.

use crate::host_function;
use crate::runtime::host_call::{throw_thrown, HostCall, HostResult, Thrown};
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intl_support::IntlClass;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::exception_helpers::append_default_source_to_native_message;
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_promise::{JSPromise, JSPromiseRef, Status};
use crate::runtime::js_promise_host::PromiseHost;
use crate::runtime::js_value::{js_undefined, JSValue};
use crate::runtime::js_web_assembly::{
    capture_pending_exception, exported_call_bits, exported_function_arity, exported_target, results_to_js, settle, thrown_to_value,
    throw_thrown_value, wasm_error_to_thrown, ExportedFunction, multi_result_values, to_wasm_value,
};
use crate::runtime::object_to_primitive::call_function;
use crate::runtime::property_attribute::{DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::wasm_errors::WasmErrorKind;
use crate::wasm::wasm_format::Type;
use crate::wasm::wasm_instance::{JsThrown, WasmError};
use crate::wasm::wasm_ipint::{Completion, Suspender};
use crate::wtf::text::conversion_mode::ConversionMode;
use crate::wtf::text::wtf_string::{String as WtfString};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

const NOT_EXPORTED_MESSAGE: &str = "Argument 0 must be a WebAssembly exported function";
const OUTSIDE_PROMISING_MESSAGE: &str = "Suspending() wrapper called outside of a promising() context";
const JS_FRAMES_MESSAGE: &str = "JavaScript frames found between WebAssembly.Suspending and WebAssembly.promising";

thread_local! {
    /// `cell_id` da função devolvida por `promising` -> a função wasm exportada original.
    static PROMISING: RefCell<HashMap<usize, JSValue>> = RefCell::new(HashMap::new());
    /// `cell_id` da instância de `Suspending` -> a função JS embrulhada.
    static SUSPENDING: RefCell<HashMap<usize, JSValue>> = RefCell::new(HashMap::new());
    /// Quantas chamadas `promising` estão ativas (a pilha de promising do JSC).
    static PROMISING_DEPTH: Cell<u32> = Cell::new(0);
    /// A execução wasm em curso foi entrada direta de um `promising` (sem frame de JS por cima).
    static DIRECT_ENTRY: Cell<bool> = Cell::new(false);
    /// `cell_id` de cada reação (`onFulfilled` e `onRejected`) -> a execução suspensa que ela retoma.
    static RESUMPTIONS: RefCell<HashMap<usize, Rc<Resumption>>> = RefCell::new(HashMap::new());
}

/// Fim do programa (`cell_registry::reset_program_state`): todas as chaves são `cell_id` e os valores
/// apontam para células do programa que acabou.
/// As raízes do coletor: as funções de `promising` e `Suspending`.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    for table in [&PROMISING, &SUSPENDING] {
        table.with(|entries| {
            for (cell_id, value) in entries.borrow().iter() {
                marker.cell(*cell_id);
                marker.value(*value);
            }
        });
    }
}

/// A execução suspensa vive com a reação que a retoma, e segura a promessa de `promising`.
pub(crate) fn visit_side_table(marker: &mut crate::runtime::gc::Marker, phase: crate::runtime::gc::SidePhase) -> bool {
    crate::runtime::gc::side_table(&RESUMPTIONS, marker, phase, |resumption, out| out.push(resumption.promise.as_value()))
}

pub(crate) fn reset_for_program() {
    let _ = PROMISING.try_with(|map| map.borrow_mut().clear());
    let _ = SUSPENDING.try_with(|map| map.borrow_mut().clear());
    let _ = PROMISING_DEPTH.try_with(|depth| depth.set(0));
    let _ = DIRECT_ENTRY.try_with(|direct| direct.set(false));
    let _ = RESUMPTIONS.try_with(|map| map.borrow_mut().clear());
}

fn native_function(global_object: &JSGlobalObject, length: u32, name: &str, body: crate::runtime::native_function::NativeFunction) -> JSValue {
    JSFunction::create_native(
        global_object.vm(),
        global_object,
        length,
        &WtfString::from_utf8(name.as_bytes()),
        body,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    )
    .as_value()
}

/// `WebAssembly.promising(fn)`.
fn promising_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let original = call.argument(0);
    let Some(arity) = exported_function_arity(original) else {
        return Err(Thrown::type_error(NOT_EXPORTED_MESSAGE));
    };
    let wrapper = native_function(global_object, arity, "WebAssembly.promising", promising_wrapper);
    if let JSValue::Cell(cell_id) = wrapper {
        PROMISING.with(|table| table.borrow_mut().insert(cell_id, original));
    }
    Ok(wrapper)
}

/// O corpo da função devolvida por `promising`: roda a wasm até terminar ou suspender numa importação
/// `Suspending` que devolveu promessa. A promessa devolvida assenta quando a execução termina (na hora, ou depois
/// de `resume` numa microtarefa).
fn promising_wrapper_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let original = PROMISING
        .with(|table| table.borrow().get(&call.callee()).copied())
        .ok_or_else(|| Thrown::type_error(NOT_EXPORTED_MESSAGE))?;
    let Some(target) = exported_target(original) else {
        return Err(Thrown::type_error(NOT_EXPORTED_MESSAGE));
    };
    let bits = match exported_call_bits(global_object, &target, call) {
        Ok(bits) => bits,
        Err(thrown) => return settle(global_object, Err(thrown)),
    };
    let promise = JSPromise::create(global_object.vm(), &global_object.promise_structure());
    // O frame nativo do wrapper de `promising` é a âncora dos quadros Wasm (o bun o mostra como `at unknown`).
    let anchor = crate::wasm::wasm_call_stack::enter_from_js(call.native_frame_registers());
    let outcome = run_promising(|| target.instance.invoke_resumable(target.index, &bits));
    drop(anchor);
    drive(global_object, &promise, &target, outcome);
    Ok(promise.as_value())
}

/// Roda `body` como a entrada direta de um `promising` (a pilha de promising do JSC tem um nível a mais).
fn run_promising<T>(body: impl FnOnce() -> T) -> T {
    PROMISING_DEPTH.with(|depth| depth.set(depth.get() + 1));
    let result = with_direct_entry(true, body);
    PROMISING_DEPTH.with(|depth| depth.set(depth.get() - 1));
    result
}

/// Roda `body` com a marca "entrada direta de um `promising`" em `direct` e a restaura ao sair.
pub(crate) fn with_direct_entry<T>(direct: bool, body: impl FnOnce() -> T) -> T {
    let previous = DIRECT_ENTRY.with(|flag| flag.replace(direct));
    let result = body();
    DIRECT_ENTRY.with(|flag| flag.set(previous));
    result
}

/// A função embrulhada se `callee` é uma instância de `WebAssembly.Suspending`.
pub(crate) fn suspending_function(callee: JSValue) -> Option<JSValue> {
    let JSValue::Cell(cell_id) = callee else {
        return None;
    };
    SUSPENDING.with(|table| table.borrow().get(&cell_id).copied())
}

/// `!vm.topJSPIContext`: a mensagem do `SuspendError` (com o ` (evaluating '...')` da chamada JS em curso, como
/// o `ErrorInstance` do bun) quando a importação `Suspending` roda fora de um `promising`, antes de chamar a função.
pub(crate) fn outside_promising_refusal(global_object: &JSGlobalObject) -> Option<String> {
    if PROMISING_DEPTH.with(Cell::get) != 0 {
        return None;
    }
    let message = append_default_source_to_native_message(global_object, OUTSIDE_PROMISING_MESSAGE);
    Some(String::from_utf8_lossy(&message.utf8(ConversionMode::LenientConversion)).into_owned())
}

/// `SlicingOutcome::Overrun`: a mensagem do `SuspendError` quando há frame de JS entre a importação e o
/// `promising` (checada depois de chamar a função embrulhada, como o JSC).
pub(crate) fn js_frames_refusal() -> Option<&'static str> {
    if DIRECT_ENTRY.with(Cell::get) {
        None
    } else {
        Some(JS_FRAMES_MESSAGE)
    }
}

/// `runWebAssemblySuspendingFunction`: o resultado que não é `JSPromise` ganha uma promessa nova resolvida com ele
/// ("a especificação exige suspender mesmo com um valor"; thenables são aguardados), e uma `JSPromise` é usada
/// como está (sem consultar `then`).
pub(crate) fn promise_for_suspension(global_object: &JSGlobalObject, result: JSValue) -> JSValue {
    if JSPromise::from_value(&result).is_some() {
        return result;
    }
    let promise = JSPromise::create(global_object.vm(), &global_object.promise_structure());
    promise.resolve(global_object, result);
    promise.as_value()
}

/// O pedido de suspensão de uma importação `Suspending`: a promessa a esperar e os tipos de retorno da importação
/// (para converter o valor com que ela cumpre).
pub(crate) struct SuspendRequest {
    pub(crate) promise: JSValue,
    pub(crate) returns: Vec<Type>,
}

/// A execução suspensa à espera da promessa de uma importação.
struct Resumption {
    promise: JSPromiseRef,
    target: Rc<ExportedFunction>,
    suspender: RefCell<Option<Suspender>>,
    returns: Vec<Type>,
    /// Os `cell_id` das duas reações, para tirá-las da tabela quando uma delas roda.
    reactions: Cell<(usize, usize)>,
}

/// Entrega o desfecho de uma execução à promessa de `promising`: assenta com o resultado ou o erro, ou liga a
/// retomada à promessa da importação.
fn drive(global_object: &JSGlobalObject, promise: &JSPromiseRef, target: &Rc<ExportedFunction>, outcome: Result<Completion, WasmError>) {
    let result = match outcome {
        Ok(Completion::Done(results)) => results_to_js(global_object, &target.returns, &results),
        Err(error) => Err(wasm_error_to_thrown(global_object, error, None)),
        Ok(Completion::Suspended(suspender, request)) => {
            suspend(global_object, promise, target, suspender, &request);
            return;
        }
    };
    finish(global_object, promise, result);
}

fn finish(global_object: &JSGlobalObject, promise: &JSPromiseRef, result: HostResult) {
    match result {
        Ok(value) => promise.resolve(global_object, value),
        Err(thrown) => promise.reject(global_object, thrown_to_value(global_object, thrown)),
    }
}

/// `then(onFulfilled, onRejected)` na promessa da importação: a reação retoma a execução guardada.
fn suspend(
    global_object: &JSGlobalObject,
    promise: &JSPromiseRef,
    target: &Rc<ExportedFunction>,
    suspender: Suspender,
    request: &JsThrown,
) {
    let Some(request) = request.0.downcast_ref::<SuspendRequest>() else {
        // O único produtor de `Suspend` é a importação `Suspending`, que sempre embrulha um `SuspendRequest`.
        unreachable!("pedido de suspensão que não é SuspendRequest");
    };
    let on_fulfilled = native_function(global_object, 1, "", resume_fulfilled);
    let on_rejected = native_function(global_object, 1, "", resume_rejected);
    let (JSValue::Cell(fulfilled_id), JSValue::Cell(rejected_id)) = (on_fulfilled, on_rejected) else {
        return;
    };
    let state = Rc::new(Resumption {
        promise: promise.clone(),
        target: target.clone(),
        suspender: RefCell::new(Some(suspender)),
        returns: request.returns.clone(),
        reactions: Cell::new((fulfilled_id, rejected_id)),
    });
    RESUMPTIONS.with(|table| {
        let mut table = table.borrow_mut();
        table.insert(fulfilled_id, state.clone());
        table.insert(rejected_id, state);
    });
    // `promise->performPromiseThen(vm, globalObject, fulfiller, rejecter, jsUndefined())`: direto na promessa,
    // sem ler `then`.
    match JSPromise::from_value(&request.promise) {
        Some(awaited) => awaited.perform_promise_then(global_object, on_fulfilled, on_rejected, js_undefined()),
        // `promise_for_suspension` entrega sempre uma `JSPromise` (a própria, ou uma nova já resolvida).
        None => unreachable!("promise_for_suspension devolveu algo que não é JSPromise"),
    }
}

/// Tira a retomada da tabela (as duas reações) quando uma delas roda.
fn take_resumption(reaction: usize) -> Option<Rc<Resumption>> {
    RESUMPTIONS.with(|table| {
        let mut table = table.borrow_mut();
        let state = table.remove(&reaction)?;
        let (fulfilled, rejected) = state.reactions.get();
        table.remove(&fulfilled);
        table.remove(&rejected);
        Some(state)
    })
}

/// Reentra no interpretador com o resultado da importação e entrega o novo desfecho.
/// A pilha de `Error.stack` dos quadros guardados no `Suspender` é reposta por `Instance::resume` (uma entrada por
/// quadro); aqui só falta a âncora, que passa a ser o frame nativo da reação que roda agora (o frame nativo
/// original do `promising` já saiu da pilha). No bun os quadros aparecem como `at unknown` sob a importação.
fn continue_run(global_object: &JSGlobalObject, call: &HostCall, state: &Rc<Resumption>, result: Result<Vec<u64>, WasmError>) {
    let Some(suspender) = state.suspender.borrow_mut().take() else {
        return;
    };
    let anchor = crate::wasm::wasm_call_stack::enter_from_js(call.native_frame_registers());
    let outcome = run_promising(|| state.target.instance.resume(suspender, result));
    drop(anchor);
    drive(global_object, &state.promise, &state.target, outcome);
}

/// `onFulfilled(valor)`: converte o valor ao tipo de retorno da importação e retoma.
fn resume_fulfilled_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(state) = take_resumption(call.callee()) else {
        return Ok(js_undefined());
    };
    let converted = match state.returns.as_slice() {
        [] => Ok(Vec::new()),
        [ty] => to_wasm_value(global_object, call.argument(0), *ty).map(|bits| vec![bits]),
        returns => multi_result_values(global_object, call.argument(0), returns),
    };
    let result = converted.map_err(|thrown| {
        throw_thrown(global_object, thrown);
        capture_pending_exception(global_object)
    });
    continue_run(global_object, call, &state, result);
    Ok(js_undefined())
}

/// `onRejected(erro)`: retoma lançando o erro no ponto da chamada da importação.
fn resume_rejected_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(state) = take_resumption(call.callee()) else {
        return Ok(js_undefined());
    };
    throw_thrown_value(global_object, call.argument(0));
    let error = capture_pending_exception(global_object);
    continue_run(global_object, call, &state, Err(error));
    Ok(js_undefined())
}


/// `new WebAssembly.Suspending(fn)`: uma função que só vale dentro de uma chamada `promising`.
fn construct_suspending_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if call.argument_count() == 0 {
        return Err(Thrown::type_error("new WebAssembly.Suspending() requires 1 argument"));
    }
    let function = call.argument(0);
    if !function.is_callable() {
        return Err(Thrown::type_error("Argument 0 must be a function"));
    }
    let wrapper = native_function(global_object, 0, "WebAssembly.Suspending", suspending_wrapper);
    if let JSValue::Cell(cell_id) = wrapper {
        SUSPENDING.with(|table| table.borrow_mut().insert(cell_id, function));
    }
    Ok(wrapper)
}

fn call_suspending_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(Thrown::type_error("calling WebAssembly.Suspending constructor without new is invalid"))
}

/// O corpo da instância de `Suspending`.
fn suspending_wrapper_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if PROMISING_DEPTH.with(Cell::get) == 0 {
        return Err(Thrown::WebAssembly(WasmErrorKind::Suspend, OUTSIDE_PROMISING_MESSAGE.to_string()));
    }
    let function = SUSPENDING
        .with(|table| table.borrow().get(&call.callee()).copied())
        .ok_or_else(|| Thrown::type_error("Argument 0 must be a function"))?;
    let result = call_function(global_object, function, call.this_value(), call.arguments()).ok_or(Thrown::Pending)?;
    let Some(promise) = JSPromise::from_value(&result) else {
        return Ok(result);
    };
    match promise.status() {
        Status::Fulfilled => Ok(promise.settlement_value()),
        Status::Rejected => {
            promise.mark_as_handled();
            throw_thrown_value(global_object, promise.settlement_value());
            Err(Thrown::Pending)
        }
        Status::Pending => Err(Thrown::WebAssembly(WasmErrorKind::Suspend, JS_FRAMES_MESSAGE.to_string())),
    }
}

host_function!(promising, promising_body);
host_function!(promising_wrapper, promising_wrapper_body);
host_function!(resume_fulfilled, resume_fulfilled_body);
host_function!(resume_rejected, resume_rejected_body);
host_function!(call_web_assembly_suspending, call_suspending_body);
host_function!(construct_web_assembly_suspending, construct_suspending_body);
host_function!(suspending_wrapper, suspending_wrapper_body);

/// Instala `promising`, `Suspending` e `SuspendError`, nessa ordem, depois de `JSTag`.
pub(crate) fn install_jspi(global_object: &JSGlobalObject, namespace: &JSObject) {
    let vm = global_object.vm();
    crate::runtime::js_web_assembly::put_enumerable_method(global_object, namespace, "promising", 0, promising);
    let class = IntlClass {
        name: "Suspending",
        length: 1,
        has_supported_locales_of: false,
        call: call_web_assembly_suspending,
        construct: construct_web_assembly_suspending,
    };
    let prototype = class.install(global_object, namespace);
    // `IntlClass::install` grava `Intl.Suspending`; o protótipo do bun não tem `@@toStringTag`.
    let tag = PropertyName::from_identifier(&vm.property_names.to_string_tag_symbol);
    let _ = prototype.delete_property(vm, &tag, &mut crate::runtime::delete_property_slot::DeletePropertySlot::default());
    // No bun o protótipo TEM `constructor` próprio (golden: `["constructor"]`) e o `name` do construtor é
    // `WebAssembly.Suspending`.
    let constructor_key = PropertyName::from_identifier(&vm.property_names.constructor);
    if let JSValue::Cell(cell_id) = prototype.get_direct_by_name(vm, &constructor_key) {
        if let Some(constructor) = JSObject::from_cell_id(cell_id) {
            let full_name = JSValue::from_js_string(crate::runtime::js_string::js_string(vm, &WtfString::from_utf8(b"WebAssembly.Suspending")));
            constructor.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.name), full_name, DONT_ENUM | READ_ONLY);
        }
        if let Some(internal) = crate::runtime::internal_function::InternalFunction::from_cell_id(cell_id) {
            internal.set_original_name(vm, &WtfString::from_utf8(b"WebAssembly.Suspending"));
        }
    }
    crate::runtime::wasm_errors::install_wasm_errors(global_object, namespace, WasmErrorKind::Suspend);
}
