//! `AbortController` e `AbortSignal` do global. O JavaScriptCore não os define: quem os instala é o bun (WebCore),
//! como propriedades de dados comuns (`writable`, `enumerable`, `configurable`). Medido no bun 1.4.2:
//!
//! - `AbortController`: `length` 0, herda de `Function.prototype`; o protótipo herda de `Object.prototype` e tem,
//!   nesta ordem, `constructor` (não enumerável), o acessor `signal` (sem setter), `abort` (`length` 0) e
//!   `@@toStringTag` "AbortController". A instância não tem propriedade própria e `signal` devolve sempre o mesmo
//!   objeto. Sem `new`: ``Use `new AbortController(...)` instead of `AbortController(...)` `` (`ERR_ILLEGAL_CONSTRUCTOR`);
//! - `AbortSignal`: `length` 0, o construtor herda de `EventTarget` e o protótipo de `EventTarget.prototype`; chaves
//!   do construtor: `length`, `name`, `prototype`, `abort`, `timeout`, `any` (`any` tem `length` 1); o protótipo tem
//!   `constructor`, os acessores `aborted`, `reason`, `onabort` (com setter), `throwIfAborted` (`length` 0) e
//!   `@@toStringTag` "AbortSignal". `new AbortSignal()` e `AbortSignal()` são `Illegal constructor`
//!   (`ERR_ILLEGAL_CONSTRUCTOR`);
//! - `controller.abort(reason)`: `reason` `undefined` vira um `DOMException` `AbortError` ("The operation was
//!   aborted.", código 20, com `line`, `column`, `sourceURL` e `stack` próprios como o lançado por função nativa);
//!   qualquer outro valor (`null` inclusive) é guardado com a identidade. Abortar de novo não faz nada. O evento
//!   `abort` é um `Event` comum (`bubbles` e `cancelable` falsos) com `isTrusted` `true`, despachado depois de
//!   `aborted` e `reason` valerem; antes dele saem os ouvintes registrados com a opção `signal`;
//! - `onabort`: guarda um objeto qualquer (função ou não; só a função é chamada), valor que não é objeto vira
//!   `null`; ocupa na lista de ouvintes a posição do primeiro `set` e é trocado no lugar;
//! - `throwIfAborted()` lança o `reason`; `AbortSignal.abort(reason)` devolve um sinal já abortado (mesmo `reason`
//!   padrão); `AbortSignal.any(iterable)` devolve um sinal que aborta com o `reason` do primeiro de origem que
//!   abortar (já abortado na criação se algum já estava), dependentes encadeados incluídos, e o evento dele também
//!   é confiável. Elemento que não é `AbortSignal`: `The "signals[<i>]" argument must be an instance of
//!   AbortSignal. Received ...` (`ERR_INVALID_ARG_TYPE`); `AbortSignal.any()`: `signals can not be converted to
//!   sequence`;
//! - brand check: getters lançam `The AbortSignal.<nome> getter can only be used on instances of AbortSignal` (sem
//!   `code`, o mesmo para `AbortController.signal`), métodos `Can only call AbortSignal.throwIfAborted on instances
//!   of AbortSignal` (`ERR_INVALID_THIS`), idem `AbortController.abort`; os métodos estáticos ignoram o `this`.
//!
//! DIVERGÊNCIAS:
//!
//! - `AbortSignal.timeout(ms)` (`length` 1, entre `abort` e `any`): `[EnforceRange] unsigned long long` (`ToNumber`,
//!   truncado; fora de `[0, 2^53 - 1]` é `TypeError` `Value <n> is outside the range [0, 9007199254740991]`, sem
//!   `code`; sem argumento `Not enough arguments`, `ERR_MISSING_ARGS`). Aborta com `DOMException` `TimeoutError`
//!   ("The operation timed out.", código 23, só `stack` própria, vazia) num timer nativo que não mantém o laço vivo
//!   e que não gasta id de `setTimeout`; dispara na ordem dos `setTimeout` do mesmo prazo;
//! - sem pendência. (Conferido com o bun: `any(5)`, `any({})`, `any(true)` e um `@@iterator` não chamável dão
//!   `TypeError` `Type error` sem `code`, o mesmo do `for_each_in_iterable`. A ordem de despacho segue o
//!   `signalAbort` do WebCore: só as raízes guardam dependentes, na ordem de criação, e um `any` de `any` herda as
//!   origens do dependente; a origem despacha primeiro, depois os dependentes, todos já marcados.)

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{derived_structure, put_to_string_tag};
use crate::runtime::event_target::{
    create_trusted_event, dispatch_event, event_handler, put_methods, register_target, remove_signal_listeners, set_event_handler,
};
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO;
use crate::runtime::iterator_operations::for_each_in_iterable;
use crate::runtime::js_dom_exception::{new_detached_dom_exception, new_dom_exception};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_module_loader::describe_received;
use crate::runtime::js_object::{JSFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_value::{js_undefined, EncodedJSValue, JSValue};
use crate::runtime::native_class_support::{
    create_native_class, create_native_subclass, install_global, instance_structure, put_native_accessor, throw_coded_type_error, throw_native_type_error,
};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::structure::StructureRef;
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::runtime::timers::schedule_native;
use crate::wtf::text::conversion_mode::ConversionMode;

/// O maior inteiro exato de um `double` (`Number.MAX_SAFE_INTEGER`), o teto do `unsigned long long` com `[EnforceRange]`.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

static ABORT_CONTROLLER_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "AbortController", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static ABORT_SIGNAL_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "AbortSignal", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// O estado de um `AbortSignal`.
struct SignalState {
    aborted: bool,
    reason: JSValue,
    /// Os sinais de `AbortSignal.any` que abortam junto com este (só as raízes os guardam), na ordem de criação.
    dependents: Vec<JSValue>,
    /// É um sinal de `AbortSignal.any` ainda não abortado.
    dependent: bool,
    /// As origens (sempre raízes) de um sinal dependente.
    sources: Vec<JSValue>,
}

thread_local! {
    /// Os sinais do programa (valor codificado da célula) e o estado de cada um.
    static SIGNALS: RefCell<HashMap<EncodedJSValue, SignalState>> = RefCell::new(HashMap::new());
    /// Os controladores do programa e o sinal de cada um.
    static CONTROLLERS: RefCell<HashMap<EncodedJSValue, JSValue>> = RefCell::new(HashMap::new());
    /// A `Structure` das instâncias de `AbortSignal` de cada realm (chave: `cell_id`).
    static SIGNAL_STRUCTURES: RefCell<Vec<(usize, StructureRef)>> = const { RefCell::new(Vec::new()) };
}

/// Fim do programa (`cell_registry::reset_program_state`): o estado guardado é do programa.
/// As raízes do coletor: os controladores e os sinais deles; o estado dos sinais guarda valores e adia a coleta
/// enquanto existir.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    CONTROLLERS.with(|controllers| {
        for (controller, signal) in controllers.borrow().iter() {
            marker.encoded(*controller);
            marker.value(*signal);
        }
    });
}

/// O estado de cada `AbortSignal` é campo do objeto: o `reason`, os dependentes e as origens vivem com ele.
pub(crate) fn visit_side_table(marker: &mut crate::runtime::gc::Marker, phase: crate::runtime::gc::SidePhase) -> bool {
    crate::runtime::gc::side_table(&SIGNALS, marker, phase, |state, out| {
        out.push(state.reason);
        out.extend_from_slice(&state.dependents);
        out.extend_from_slice(&state.sources);
    })
}

pub(crate) fn reset_for_program() {
    let _ = SIGNALS.try_with(|signals| signals.borrow_mut().clear());
    let _ = CONTROLLERS.try_with(|controllers| controllers.borrow_mut().clear());
    let _ = SIGNAL_STRUCTURES.try_with(|structures| structures.borrow_mut().clear());
}

/// `value` é um `AbortSignal`.
pub(crate) fn is_signal(value: JSValue) -> bool {
    SIGNALS.with(|signals| signals.borrow().contains_key(&value.encode()))
}

/// `value` é um `AbortSignal` já abortado.
pub(crate) fn is_aborted(value: JSValue) -> bool {
    SIGNALS.with(|signals| signals.borrow().get(&value.encode()).is_some_and(|state| state.aborted))
}

fn reason_of(signal: JSValue) -> JSValue {
    SIGNALS.with(|signals| signals.borrow().get(&signal.encode()).map_or_else(js_undefined, |state| state.reason))
}

/// Um `AbortSignal` novo, não abortado.
pub(crate) fn new_signal(global_object: &JSGlobalObject) -> Result<JSValue, Thrown> {
    let realm = global_object.cell_id();
    let structure = SIGNAL_STRUCTURES.with(|structures| structures.borrow().iter().find(|(id, _)| *id == realm).map(|(_, structure)| structure.clone()));
    let Some(structure) = structure else { return Err(Thrown::Unported("AbortSignal sem instalação no realm")) };
    let signal = JSFinalObject::create(global_object.vm(), &structure).as_value();
    register_target(signal);
    let state = SignalState { aborted: false, reason: js_undefined(), dependents: Vec::new(), dependent: false, sources: Vec::new() };
    SIGNALS.with(|signals| signals.borrow_mut().insert(signal.encode(), state));
    Ok(signal)
}

/// O `reason` de `abort(reason)`: `undefined` vira o `DOMException` `AbortError`.
fn abort_reason(global_object: &JSGlobalObject, call: &HostCall) -> Result<JSValue, Thrown> {
    match call.argument(0) {
        reason if reason.is_undefined() => new_dom_exception(global_object, call, "AbortError", "The operation was aborted."),
        reason => Ok(reason),
    }
}

/// `markAborted` do WebCore: marca `signal` com `reason` e esquece as origens dele. Devolve os dependentes que ele
/// guardava (esvaziados), ou `None` se já estava abortado.
fn mark_aborted(signal: JSValue, reason: JSValue) -> Option<Vec<JSValue>> {
    SIGNALS.with(|signals| match signals.borrow_mut().get_mut(&signal.encode()) {
        Some(state) if !state.aborted => {
            state.aborted = true;
            state.reason = reason;
            state.sources.clear();
            Some(std::mem::take(&mut state.dependents))
        }
        _ => None,
    })
}

/// `addSourceSignal` do WebCore: um sinal que já é dependente entrega as origens dele (as raízes), de modo que só
/// as raízes guardam dependentes e a ordem de despacho é a de criação, sem profundidade.
fn add_source(signal: JSValue, source: JSValue) {
    let (is_dependent, inner) = SIGNALS.with(|signals| {
        signals.borrow().get(&source.encode()).map_or((false, Vec::new()), |state| (state.dependent, state.sources.clone()))
    });
    if is_dependent {
        for root in inner {
            add_source(signal, root);
        }
        return;
    }
    SIGNALS.with(|signals| {
        let mut signals = signals.borrow_mut();
        let already = signals.get(&signal.encode()).is_some_and(|state| state.sources.contains(&source));
        if already {
            return;
        }
        if let Some(state) = signals.get_mut(&signal.encode()) {
            state.sources.push(source);
        }
        if let Some(state) = signals.get_mut(&source.encode()) {
            state.dependents.push(signal);
        }
    });
}

/// O algoritmo "signal abort": marca `signal` e os dependentes ainda não abortados, depois roda os passos de aborto
/// (tira os ouvintes registrados com `signal` e despacha `abort`) em `signal` e em cada dependente, na ordem de
/// criação.
fn signal_abort(global_object: &JSGlobalObject, signal: JSValue, reason: JSValue) -> Result<(), Thrown> {
    let Some(dependents) = mark_aborted(signal, reason) else { return Ok(()) };
    let mut order = vec![signal];
    for dependent in dependents {
        if mark_aborted(dependent, reason).is_some() {
            order.push(dependent);
        }
    }
    for aborted in order {
        remove_signal_listeners(aborted);
        let event = create_trusted_event(global_object, "abort")?;
        dispatch_event(global_object, aborted, event)?;
    }
    Ok(())
}

/// `controller.abort(reason)` de um `AbortController` interno (o do `WritableStreamDefaultController`): `undefined`
/// vira o `AbortError` como em `abort()`.
pub(crate) fn abort_with_reason(global_object: &JSGlobalObject, signal: JSValue, reason: JSValue) -> Result<(), Thrown> {
    let reason = if reason.is_undefined() {
        crate::runtime::js_dom_exception::new_detached_dom_exception(global_object, "AbortError", "The operation was aborted.")?
    } else {
        reason
    };
    signal_abort(global_object, signal, reason)
}

fn illegal_signal_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(throw_coded_type_error(global_object, "Illegal constructor", "ERR_ILLEGAL_CONSTRUCTOR"))
}

fn illegal_controller_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(throw_coded_type_error(global_object, "Use `new AbortController(...)` instead of `AbortController(...)`", "ERR_ILLEGAL_CONSTRUCTOR"))
}

fn construct_controller_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let structure = derived_structure(global_object, call, instance_structure)?;
    let controller = JSFinalObject::create(global_object.vm(), &structure).as_value();
    let signal = new_signal(global_object)?;
    CONTROLLERS.with(|controllers| controllers.borrow_mut().insert(controller.encode(), signal));
    Ok(controller)
}

/// O sinal do `this` de `AbortController.signal`, ou o `TypeError` do getter.
fn controller_signal_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    CONTROLLERS
        .with(|controllers| controllers.borrow().get(&call.this_value().encode()).copied())
        .ok_or_else(|| throw_native_type_error(global_object, "The AbortController.signal getter can only be used on instances of AbortController"))
}

fn controller_abort_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let signal = CONTROLLERS.with(|controllers| controllers.borrow().get(&call.this_value().encode()).copied()).ok_or_else(|| {
        throw_coded_type_error(global_object, "Can only call AbortController.abort on instances of AbortController", "ERR_INVALID_THIS")
    })?;
    let reason = abort_reason(global_object, call)?;
    signal_abort(global_object, signal, reason)?;
    Ok(js_undefined())
}

/// O `this` de um getter/setter de `AbortSignal`, ou o `TypeError` do bun.
fn signal_accessor_this(global_object: &JSGlobalObject, call: &HostCall, what: &str, name: &str) -> Result<JSValue, Thrown> {
    let this = call.this_value();
    if is_signal(this) {
        Ok(this)
    } else {
        Err(throw_native_type_error(global_object, &format!("The AbortSignal.{name} {what} can only be used on instances of AbortSignal")))
    }
}

fn signal_aborted_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(JSValue::Bool(is_aborted(signal_accessor_this(global_object, call, "getter", "aborted")?)))
}

fn signal_reason_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(reason_of(signal_accessor_this(global_object, call, "getter", "reason")?))
}

fn signal_onabort_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(event_handler(signal_accessor_this(global_object, call, "getter", "onabort")?, "abort"))
}

fn signal_set_onabort_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = signal_accessor_this(global_object, call, "setter", "onabort")?;
    set_event_handler(this, "abort", call.argument(0));
    Ok(js_undefined())
}

fn signal_throw_if_aborted_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = call.this_value();
    if !is_signal(this) {
        return Err(throw_coded_type_error(global_object, "Can only call AbortSignal.throwIfAborted on instances of AbortSignal", "ERR_INVALID_THIS"));
    }
    if is_aborted(this) {
        return Err(throw_reason(global_object, this));
    }
    Ok(js_undefined())
}

/// Lança o `reason` de `signal` (o que `throwIfAborted()` e o `fetch` de um sinal já abortado rejeitam).
pub(crate) fn throw_reason(global_object: &JSGlobalObject, signal: JSValue) -> Thrown {
    let mut scope = ThrowScope::new(global_object.vm());
    throw_exception(global_object, &mut scope, reason_of(signal));
    Thrown::Pending
}

/// `AbortSignal.abort(reason)`.
fn signal_static_abort_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let signal = new_signal(global_object)?;
    let reason = abort_reason(global_object, call)?;
    mark_aborted(signal, reason);
    Ok(signal)
}

/// `AbortSignal.timeout(milliseconds)`: `[EnforceRange] unsigned long long` (`ToNumber`, truncado, fora de
/// `[0, 2^53 - 1]` é `TypeError`); o sinal aborta com um `DOMException` `TimeoutError` num timer nativo, que não
/// mantém o laço vivo.
fn signal_static_timeout_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if call.argument_count() < 1 {
        return Err(throw_coded_type_error(global_object, "Not enough arguments", "ERR_MISSING_ARGS"));
    }
    let number = pending_or(global_object, call.argument(0).to_number())?;
    let milliseconds = number.trunc();
    if !(0.0..=MAX_SAFE_INTEGER).contains(&milliseconds) {
        let shown = String::from_utf8_lossy(&JSValue::from_double(number).to_wtf_string().utf8(ConversionMode::LenientConversion)).into_owned();
        return Err(Thrown::type_error(&format!("Value {shown} is outside the range [0, 9007199254740991]")));
    }
    let signal = new_signal(global_object)?;
    schedule_native(
        global_object,
        // `-0` (de `-0.5`) vira `0`.
        milliseconds + 0.0,
        vec![signal],
        Rc::new(move |global: &JSGlobalObject| {
            if let Ok(reason) = new_detached_dom_exception(global, "TimeoutError", "The operation timed out.") {
                let _ = signal_abort(global, signal, reason);
            }
        }),
    );
    Ok(signal)
}

/// `AbortSignal.any(signals)`.
fn signal_static_any_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Sem argumento, `null` e `undefined` não viram sequência (medido no bun 1.4.2).
    if call.argument_count() < 1 || call.argument(0).is_undefined_or_null() {
        return Err(throw_coded_type_error(global_object, "signals can not be converted to sequence", "ERR_INVALID_ARG_TYPE"));
    }
    let mut sources = Vec::new();
    for_each_in_iterable(global_object, call.argument(0), |item| {
        if !is_signal(item) {
            let received = describe_received(global_object, item).map(|text| format!(" Received {text}")).unwrap_or_default();
            let message = format!("The \"signals[{}]\" argument must be an instance of AbortSignal.{received}", sources.len());
            return Err(throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE"));
        }
        sources.push(item);
        Ok(())
    })?;
    let signal = new_signal(global_object)?;
    if let Some(aborted) = sources.iter().copied().find(|source| is_aborted(*source)) {
        mark_aborted(signal, reason_of(aborted));
        return Ok(signal);
    }
    SIGNALS.with(|signals| {
        if let Some(state) = signals.borrow_mut().get_mut(&signal.encode()) {
            state.dependent = true;
        }
    });
    for source in sources {
        add_source(signal, source);
    }
    Ok(signal)
}

host_function!(call_abort_signal, illegal_signal_body);
host_function!(construct_abort_signal, illegal_signal_body);
host_function!(call_abort_controller, illegal_controller_body);
host_function!(construct_abort_controller, construct_controller_body);
host_function!(controller_signal, controller_signal_body);
host_function!(controller_abort, controller_abort_body);
host_function!(signal_aborted, signal_aborted_body);
host_function!(signal_reason, signal_reason_body);
host_function!(signal_onabort, signal_onabort_body);
host_function!(signal_set_onabort, signal_set_onabort_body);
host_function!(signal_throw_if_aborted, signal_throw_if_aborted_body);
host_function!(signal_static_abort, signal_static_abort_body);
host_function!(signal_static_timeout, signal_static_timeout_body);
host_function!(signal_static_any, signal_static_any_body);

/// Instala `AbortController` e `AbortSignal` no global; `event_target` é o par (protótipo, construtor) do
/// `EventTarget`, de que o `AbortSignal` herda. A posição no global vem da tabela `ORDER`.
pub fn install_abort(global_object: &JSGlobalObject, event_target: (JSValue, JSValue)) {
    let vm = global_object.vm();
    let constructor_key = crate::runtime::property_name::PropertyName::from_identifier(&vm.property_names.constructor);

    let (controller_prototype, controller_constructor) = create_native_class(
        global_object,
        &ABORT_CONTROLLER_PROTOTYPE_S_INFO,
        &CONSTRUCTOR_S_INFO,
        "AbortController",
        call_abort_controller,
        construct_abort_controller,
    );
    controller_prototype.put_direct(vm, &constructor_key, controller_constructor.as_value(), DONT_ENUM);
    put_native_accessor(vm, global_object, &controller_prototype, "signal", controller_signal, None, 0);
    put_methods(global_object, &controller_prototype, &[("abort", 0, controller_abort as NativeFunction)]);
    put_to_string_tag(vm, &controller_prototype, "AbortController");
    install_global(global_object, "AbortController", controller_constructor.as_value());

    let (prototype, constructor) = create_native_subclass(
        global_object,
        event_target,
        &ABORT_SIGNAL_PROTOTYPE_S_INFO,
        &CONSTRUCTOR_S_INFO,
        "AbortSignal",
        0,
        call_abort_signal,
        construct_abort_signal,
    );
    prototype.put_direct(vm, &constructor_key, constructor.as_value(), DONT_ENUM);
    put_native_accessor(vm, global_object, &prototype, "aborted", signal_aborted, None, 0);
    put_native_accessor(vm, global_object, &prototype, "reason", signal_reason, None, 0);
    put_native_accessor(vm, global_object, &prototype, "onabort", signal_onabort, Some(signal_set_onabort), 0);
    put_methods(global_object, &prototype, &[("throwIfAborted", 0, signal_throw_if_aborted as NativeFunction)]);
    put_to_string_tag(vm, &prototype, "AbortSignal");
    put_methods(
        global_object,
        &constructor,
        &[("abort", 0, signal_static_abort as NativeFunction), ("timeout", 1, signal_static_timeout), ("any", 1, signal_static_any)],
    );
    install_global(global_object, "AbortSignal", constructor.as_value());

    let structure = instance_structure(vm, Some(global_object), prototype.as_value());
    SIGNAL_STRUCTURES.with(|structures| structures.borrow_mut().push((global_object.cell_id(), structure)));
}
