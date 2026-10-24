//! `EventTarget` e `Event` do global. O JavaScriptCore não os define: quem os instala é o bun (WebCore), como
//! propriedades de dados comuns (`writable`, `enumerable`, `configurable`). Medido no bun 1.4.2:
//!
//! - `EventTarget`: `length` 0; o protótipo herda de `Object.prototype` e tem, nesta ordem, `constructor` (não
//!   enumerável), `addEventListener` (`length` 2), `removeEventListener` (2), `dispatchEvent` (1), todos
//!   graváveis, enumeráveis e configuráveis, e `@@toStringTag` "EventTarget". A instância não tem propriedade
//!   própria;
//! - `Event`: `length` 1; o protótipo tem `constructor`, os acessores `type`, `target`, `currentTarget`,
//!   `eventPhase`, `cancelBubble` (com setter), `bubbles`, `cancelable`, `defaultPrevented`, `composed`,
//!   `timeStamp`, `srcElement`, `returnValue` (com setter), os métodos `composedPath`, `stopPropagation`,
//!   `stopImmediatePropagation`, `preventDefault`, `initEvent` (`length` 1), as constantes `NONE`, `CAPTURING_PHASE`,
//!   `AT_TARGET`, `BUBBLING_PHASE` (0 a 3; somente leitura, enumeráveis, não configuráveis, também no construtor) e
//!   `@@toStringTag` "Event". A instância tem uma única chave própria, o acessor `isTrusted` (enumerável, não
//!   configurável, sem setter);
//! - sem `new`: ``TypeError: Use `new Event(...)` instead of `Event(...)` `` com `code` `ERR_ILLEGAL_CONSTRUCTOR`;
//!   `new Event()` é `Not enough arguments` (`ERR_MISSING_ARGS`); `options` que não é objeto (nem `undefined`/`null`)
//!   é `ERR_INVALID_ARG_TYPE`; `this` alheio: métodos lançam `Can only call Event.<método> on instances of Event`
//!   (`ERR_INVALID_THIS`), getters lançam `The Event.<nome> getter can only be used on instances of Event` sem
//!   `code`; o mesmo vale para `EventTarget`;
//! - `dispatchEvent(x)` com `x` que não é `Event`: `Argument 1 ('event') to EventTarget.dispatchEvent must be an
//!   instance of Event` (`ERR_INVALID_ARG_TYPE`); `addEventListener(t, 5)`: `Argument 2 ('listener') to
//!   EventTarget.addEventListener must be an object`; ouvinte `null`/`undefined` é ignorado (o bun avisa em stderr);
//!   `dispatchEvent` de evento em despacho: `Error` `The event "<tipo>" is already being dispatched` com `code`
//!   `ERR_EVENT_RECURSION`;
//! - lista de ouvintes: a identidade é (tipo, ouvinte, `capture`), duplicata é ignorada; a ordem é a de
//!   inserção; `once` remove antes de chamar; ouvinte removido durante o despacho não roda, ouvinte acrescentado
//!   durante o despacho não roda neste despacho; função é chamada com `this` o alvo, objeto com `handleEvent`
//!   é chamado com `this` o objeto; exceção do ouvinte vai ao relatório de erro não capturado e o despacho segue;
//!   `stopImmediatePropagation` interrompe a lista; o retorno de `dispatchEvent` é `false` só se o evento é
//!   `cancelable` e foi cancelado; durante o despacho `target` e `currentTarget` são o alvo e `eventPhase` é 2;
//!   depois, `currentTarget` é `null`, a fase 0 e `target` fica.
//!
//! - `CustomEvent`: `length` 1, o construtor herda de `Event` (e o protótipo de `Event.prototype`), só tem `length`,
//!   `name`, `prototype` como chaves próprias (as constantes de fase vêm de `Event`); o protótipo tem `constructor`,
//!   o acessor `detail` (sem setter), `initCustomEvent` (`length` 1) e `@@toStringTag` "CustomEvent". `detail` é
//!   lido do `init` depois de `composed` (`undefined` e ausente dão `null`) e a identidade do valor é mantida;
//!   `initCustomEvent(type, bubbles, cancelable, detail)` age como `initEvent` (nada muda durante o despacho) e
//!   recoloca o `detail` (`null` se omitido). Brand check: `Can only call CustomEvent.initCustomEvent on instances
//!   of CustomEvent` (`ERR_INVALID_THIS`) e `The CustomEvent.detail getter can only be used on instances of
//!   CustomEvent`; um `Event` puro não passa;
//!
//! JÁ COBERTO: no próprio alvo os ouvintes de `capture` rodam antes dos demais (ordem estável dentro de cada grupo);
//! `passive` faz `preventDefault` e `returnValue = false` não terem efeito durante a chamada daquele ouvinte; a
//! exceção de um ouvinte (um ou vários no mesmo despacho) vai ao relato de erro não capturado do processo
//! (`VM::report_unhandled_error`: `uncaughtException` se houver ouvinte, senão o bloco de `uncaught_report`, saída 1
//! no fim) e o despacho segue.
//!
//! PROPAGAÇÃO (medido no bun 1.4.2, sem divergência): o bun não propaga eventos entre alvos. O `EventPath` do
//! `EventTarget.cpp` é construído só com o próprio alvo (`m_path = { EventContext { &target, 0 } }`), não há
//! `getTheParent` em classe nenhuma, então `bubbles` e `composed` são só guardados. Com `bubbles: true` e
//! `composed: true`, em `EventTarget`, subclasse (mesmo com `getTheParent`/`parentNode` definidos), `MessagePort`,
//! `BroadcastChannel`, `AbortSignal` e `Worker`, todo ouvinte roda uma vez, com `eventPhase` 2, `target` e
//! `currentTarget` o próprio alvo; `composedPath()` devolve `[]` fora do despacho e `[alvo]` durante.
//!
//! DIVERGÊNCIAS (a fazer):
//!
//! - `addEventListener` com a opção `signal`: lida depois de `capture`, `once` e `passive`; valor que não é
//!   `AbortSignal` (`null` inclusive, `undefined` não) é `TypeError: Type error`; sinal já abortado não registra; o
//!   aborto remove o ouvinte (`abort_signal.rs`);
//! - o tipo do evento e dos ouvintes é guardado em UTF-16 (surrogate solto é um tipo distinto de U+FFFD), e a
//!   mensagem de `ERR_EVENT_RECURSION` também sai em UTF-16;
//! - falta o aviso de stderr do ouvinte `null`; os `TypeError` sem `code` ganham `line`/`column` no bun;
//! - o bun registra `onerror`/`onmessage` e dispara `ErrorEvent` no global; `performance` é um `EventTarget`
//!   registrado (`performance.rs`).

use std::cell::RefCell;
use std::collections::HashMap;

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::custom_getter_setter::CustomGetterSetter;
use crate::runtime::property_attribute::CUSTOM_VALUE;
use crate::runtime::property_slot::{GetValueFunc, PutValueFunc};
use crate::runtime::collection_support::{derived_structure, put_to_string_tag};
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_array::construct_array;
use crate::runtime::js_function::put_direct_native_function_without_transition;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_microtask::call_microtask;
use crate::runtime::js_object::{JSFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_null, js_number, js_undefined, EncodedJSValue, JSValue};
use crate::runtime::native_class_support::{
    create_native_class, create_native_subclass, install_global, instance_structure, put_native_accessor, throw_coded_type_error, throw_native_type_error,
};
use crate::runtime::node_error::throw_coded_error_with_message;
use crate::wtf::text::string_concatenate::make_string_dyn;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::string_regexp_support::get_object_property;
use crate::runtime::structure::StructureRef;
use crate::runtime::text_decoder::{check_options, option_flag};
use crate::wtf::text::wtf_string::String as WtfString;

use crate::runtime::string_prototype::code_units;

static EVENT_TARGET_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "EventTarget", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static EVENT_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Event", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static CUSTOM_EVENT_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "CustomEvent", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static ERROR_EVENT_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "ErrorEvent", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static MESSAGE_EVENT_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "MessageEvent", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static CLOSE_EVENT_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "CloseEvent", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

const AT_TARGET: i32 = 2;

/// Um ouvinte registrado: a identidade é (`kind`, `callback`, `capture`).
#[derive(Clone)]
struct Listener {
    kind: Vec<u16>,
    callback: JSValue,
    capture: bool,
    once: bool,
    /// `passive`: durante a chamada, `preventDefault` e `returnValue = false` não têm efeito.
    passive: bool,
    /// O atributo de evento (`onabort`): outra identidade que a de `addEventListener`, uma por tipo.
    attr: bool,
    /// Identifica o registro: remover e registrar de novo o mesmo (tipo, callback, capture) cria outro registro, que
    /// o despacho em andamento não chama.
    id: u64,
}

fn next_listener_id() -> u64 {
    NEXT_LISTENER_ID.with(|next| {
        let id = next.get();
        next.set(id + 1);
        id
    })
}

/// O estado de um `Event`.
#[derive(Clone)]
struct EventState {
    kind: Vec<u16>,
    bubbles: bool,
    cancelable: bool,
    composed: bool,
    default_prevented: bool,
    stop_propagation: bool,
    stop_immediate: bool,
    target: JSValue,
    current_target: JSValue,
    phase: i32,
    dispatching: bool,
    /// `Some` só nos `CustomEvent` (o `detail`, `null` se ausente).
    detail: Option<JSValue>,
    /// A classe da instância: `Event`, `CustomEvent`, `ErrorEvent`, `MessageEvent` ou `CloseEvent`.
    class: &'static str,
    /// Os campos próprios de `ErrorEvent` (`message`, `filename`, `lineno`, `colno`, `error`), `MessageEvent` (`data`,
    /// `origin`, `lastEventId`, `source`, `ports`) e `CloseEvent` (`wasClean`, `code`, `reason`).
    fields: Vec<JSValue>,
    /// `isTrusted`: `true` só nos eventos que o próprio ambiente despacha (o `abort`).
    trusted: bool,
    /// Dentro de um ouvinte `passive`: o cancelamento é ignorado.
    in_passive: bool,
}

/// Os ouvintes dos alvos, os sinais que os registraram e o estado dos eventos seguram valores do programa.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    SIGNAL_LISTENERS.with(|table| {
        for (signal, links) in table.borrow().iter() {
            marker.encoded(*signal);
            for (target, listener) in links {
                marker.value(*target);
                marker.value(listener.callback);
            }
        }
    });
    TARGETS.with(|table| {
        for (target, listeners) in table.borrow().iter() {
            marker.encoded(*target);
            listeners.iter().for_each(|listener| marker.value(listener.callback));
        }
    });
    EVENTS.with(|table| {
        for (event, state) in table.borrow().iter() {
            marker.encoded(*event);
            marker.values(&[state.target, state.current_target]);
            marker.value(state.detail);
            marker.values(&state.fields);
        }
    });
}

thread_local! {
    static NEXT_LISTENER_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// A `Structure` das instâncias de `Event` de cada realm (chave: `cell_id`), para os eventos que o código nativo cria.
    static EVENT_STRUCTURES: RefCell<Vec<(usize, StructureRef)>> = const { RefCell::new(Vec::new()) };
    /// A `Structure` das instâncias de `MessageEvent` de cada realm (chave: `cell_id`), para os eventos que o código
    /// nativo cria (a entrega do `BroadcastChannel`).
    static MESSAGE_EVENT_STRUCTURES: RefCell<Vec<(usize, StructureRef)>> = const { RefCell::new(Vec::new()) };
    /// A `Structure` das instâncias de `ErrorEvent` de cada realm (chave: `cell_id`), para o `error` do `Worker`.
    static ERROR_EVENT_STRUCTURES: RefCell<Vec<(usize, StructureRef)>> = const { RefCell::new(Vec::new()) };
    /// Os ouvintes registrados com a opção `signal`, por sinal (valor codificado): (alvo, ouvinte).
    static SIGNAL_LISTENERS: RefCell<HashMap<EncodedJSValue, Vec<(JSValue, Listener)>>> = RefCell::new(HashMap::new());
    /// Os alvos do programa (valor codificado da célula) e suas listas de ouvintes.
    static TARGETS: RefCell<HashMap<EncodedJSValue, Vec<Listener>>> = RefCell::new(HashMap::new());
    /// Os eventos do programa e o estado de cada um.
    static EVENTS: RefCell<HashMap<EncodedJSValue, EventState>> = RefCell::new(HashMap::new());
}

/// Fim do programa (`cell_registry::reset_program_state`): o estado guardado é do programa.
pub(crate) fn reset_for_program() {
    let _ = TARGETS.try_with(|targets| targets.borrow_mut().clear());
    let _ = EVENTS.try_with(|events| events.borrow_mut().clear());
    let _ = EVENT_STRUCTURES.try_with(|structures| structures.borrow_mut().clear());
    let _ = MESSAGE_EVENT_STRUCTURES.try_with(|structures| structures.borrow_mut().clear());
    let _ = ERROR_EVENT_STRUCTURES.try_with(|structures| structures.borrow_mut().clear());
    let _ = SIGNAL_LISTENERS.try_with(|listeners| listeners.borrow_mut().clear());
}

/// Um `ErrorEvent` `error` criado pelo código nativo (`isTrusted` `true`, campos `message`, `filename`, `lineno`,
/// `colno` e `error`), como o que o `Worker` despacha ao receber a exceção não tratada do filho.
pub(crate) fn create_error_event(global_object: &JSGlobalObject, message: &str, filename: &str, line: u32, column: u32, error: JSValue) -> Result<JSValue, Thrown> {
    let realm = global_object.cell_id();
    let structure = ERROR_EVENT_STRUCTURES.with(|structures| structures.borrow().iter().find(|(id, _)| *id == realm).map(|(_, structure)| structure.clone()));
    let Some(structure) = structure else { return Err(Thrown::Unported("ErrorEvent sem instalação no realm")) };
    let mut state = EventState::new("error".encode_utf16().collect(), false, false, false, None, true);
    state.class = "ErrorEvent";
    let text = |value: &str| string_value(global_object, &value.encode_utf16().collect::<Vec<u16>>());
    state.fields = vec![text(message), text(filename), js_number(f64::from(line)), js_number(f64::from(column)), error];
    Ok(build_event(global_object, &structure, state))
}

/// Registra `instance` como `EventTarget` (lista de ouvintes vazia): o `AbortSignal` herda do `EventTarget`.
pub(crate) fn register_target(instance: JSValue) {
    TARGETS.with(|targets| targets.borrow_mut().insert(instance.encode(), Vec::new()));
}

/// Um `Event` de `kind` criado pelo código nativo, com `isTrusted` `true` (o `abort` do `AbortSignal`).
pub(crate) fn create_trusted_event(global_object: &JSGlobalObject, kind: &str) -> Result<JSValue, Thrown> {
    let realm = global_object.cell_id();
    let structure = EVENT_STRUCTURES.with(|structures| structures.borrow().iter().find(|(id, _)| *id == realm).map(|(_, structure)| structure.clone()));
    let Some(structure) = structure else { return Err(Thrown::Unported("Event sem instalação no realm")) };
    let units: Vec<u16> = kind.encode_utf16().collect();
    Ok(build_event(global_object, &structure, EventState::new(units, false, false, false, None, true)))
}

/// Um `MessageEvent` `message` criado pelo código nativo, com `isTrusted` `true`, `origin` e `lastEventId` vazios,
/// `source` `null` e `ports` um array congelado com as portas transferidas (a entrega do `BroadcastChannel` e da
/// `MessagePort`; medido no bun 1.4.2).
pub(crate) fn create_message_event(global_object: &JSGlobalObject, data: JSValue, ports: &[JSValue]) -> Result<JSValue, Thrown> {
    let realm = global_object.cell_id();
    let structure = MESSAGE_EVENT_STRUCTURES.with(|structures| structures.borrow().iter().find(|(id, _)| *id == realm).map(|(_, structure)| structure.clone()));
    let Some(structure) = structure else { return Err(Thrown::Unported("MessageEvent sem instalação no realm")) };
    let mut state = EventState::new("message".encode_utf16().collect(), false, false, false, None, true);
    state.class = "MessageEvent";
    state.fields = vec![data, string_value(global_object, &[]), string_value(global_object, &[]), js_null(), frozen_array(global_object, ports)?];
    Ok(build_event(global_object, &structure, state))
}

/// Remove os ouvintes que a opção `signal` ligou ao `signal` que abortou.
pub(crate) fn remove_signal_listeners(signal: JSValue) {
    let linked = SIGNAL_LISTENERS.with(|listeners| listeners.borrow_mut().remove(&signal.encode()));
    for (target, listener) in linked.unwrap_or_default() {
        remove_listener(target, &listener);
    }
}

/// O atributo de evento `kind` (`onabort`) de `target`: a função guardada, ou `null`.
pub(crate) fn event_handler(target: JSValue, kind: &str) -> JSValue {
    let units: Vec<u16> = kind.encode_utf16().collect();
    TARGETS.with(|targets| {
        targets.borrow().get(&target.encode()).and_then(|list| list.iter().find(|l| l.attr && l.kind == units).map(|l| l.callback)).unwrap_or_else(js_null)
    })
}

/// `true` quando o global tem algum ouvinte de `kind` (atributo `onmessage` ou `addEventListener`). No bun um ouvinte de
/// `message` no global mantém o processo vivo (medido: `onmessage = f` e `addEventListener("message", f)` seguram o laço;
/// `onmessage = null`, `removeEventListener` e `once` já consumido soltam; `error` não segura).
pub(crate) fn global_has_listener(global_object: &JSGlobalObject, kind: &str) -> bool {
    target_has_listener(global_target(global_object), kind)
}

/// `true` quando `target` tem algum ouvinte de `kind` (atributo ou `addEventListener`).
pub(crate) fn target_has_listener(target: JSValue, kind: &str) -> bool {
    let units: Vec<u16> = kind.encode_utf16().collect();
    TARGETS.with(|targets| targets.borrow().get(&target.encode()).is_some_and(|list| list.iter().any(|l| l.kind == units)))
}

/// Define o atributo de evento `kind` de `target`: um objeto (função ou não) substitui no lugar (a posição na lista
/// é a do primeiro `set`; medido: um objeto não chamável é guardado e devolvido, mas nunca chamado), qualquer
/// outro valor remove o atributo.
pub(crate) fn set_event_handler(target: JSValue, kind: &str, value: JSValue) {
    let units: Vec<u16> = kind.encode_utf16().collect();
    TARGETS.with(|targets| {
        let mut targets = targets.borrow_mut();
        let Some(list) = targets.get_mut(&target.encode()) else { return };
        let existing = list.iter().position(|l| l.attr && l.kind == units);
        match (existing, value.is_object()) {
            (Some(index), true) => list[index].callback = value,
            (Some(index), false) => {
                list.remove(index);
            }
            (None, true) => list.push(Listener { kind: units, callback: value, capture: false, once: false, passive: false, attr: true, id: next_listener_id() }),
            (None, false) => {}
        }
    });
}

pub(crate) fn is_target(this: JSValue) -> bool {
    TARGETS.with(|targets| targets.borrow().contains_key(&this.encode()))
}

/// As funções registradas por `addEventListener` em `target` para o tipo `kind`, na ordem (sem os atributos de
/// evento, `onabort`): o que `events.getEventListeners` e `events.listenerCount` leem de um `EventTarget`. `None`
/// quando `target` não é um `EventTarget`.
pub(crate) fn listener_callbacks(target: JSValue, kind: &[u16]) -> Option<Vec<JSValue>> {
    TARGETS.with(|targets| {
        targets.borrow().get(&target.encode()).map(|list| list.iter().filter(|l| !l.attr && l.kind == kind).map(|l| l.callback).collect())
    })
}

fn event_state(this: JSValue) -> Option<EventState> {
    EVENTS.with(|events| events.borrow().get(&this.encode()).cloned())
}

/// O que o console do bun imprime de `MessageEvent` (`type`, `data`) e `ErrorEvent` (`type`, `message`, `error`): o nome
/// da classe e os pares, sem `isTrusted` nem os membros do protótipo. `None` para outro valor.
pub(crate) fn console_pairs(global_object: &JSGlobalObject, this: JSValue) -> Option<(&'static str, Vec<(&'static str, JSValue)>)> {
    let state = event_state(this)?;
    let kind = string_value(global_object, &state.kind);
    match state.class {
        "MessageEvent" => Some(("MessageEvent", vec![("type", kind), ("data", state.fields[0])])),
        "ErrorEvent" => Some(("ErrorEvent", vec![("type", kind), ("message", state.fields[0]), ("error", state.fields[4])])),
        _ => None,
    }
}

fn update_event(this: JSValue, update: impl FnOnce(&mut EventState)) {
    EVENTS.with(|events| {
        if let Some(state) = events.borrow_mut().get_mut(&this.encode()) {
            update(state);
        }
    });
}

/// O tipo como string JS: as unidades UTF-16 voltam intactas (surrogate solto inclusive).
fn string_value(global_object: &JSGlobalObject, units: &[u16]) -> JSValue {
    JSValue::from_js_string(js_string(global_object.vm(), &wtf_from_units(units)))
}

/// As unidades UTF-16 como `WTF::String` (8 bits quando tudo é ASCII).
fn wtf_from_units(units: &[u16]) -> WtfString {
    if units.iter().all(|&unit| unit < 0x80) {
        let bytes: Vec<u8> = units.iter().map(|&unit| unit as u8).collect();
        WtfString::from_utf8(&bytes)
    } else {
        WtfString::from_utf16(units)
    }
}

/// `ToString(argumento)` como unidades UTF-16 (a identidade do tipo; U+FFFD e surrogate solto são tipos distintos).
pub(crate) fn kind_argument(global_object: &JSGlobalObject, value: JSValue) -> Result<Vec<u16>, Thrown> {
    Ok(code_units(&pending_or(global_object, value.to_wtf_string())?).into_owned())
}

/// A chamada sem `new` de um construtor do grupo.
fn illegal_constructor(global_object: &JSGlobalObject, name: &str) -> HostResult {
    Err(throw_coded_type_error(global_object, &format!("Use `new {name}(...)` instead of `{name}(...)`"), "ERR_ILLEGAL_CONSTRUCTOR"))
}

fn illegal_event_target(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    illegal_constructor(global_object, "EventTarget")
}

fn illegal_event(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    illegal_constructor(global_object, "Event")
}

fn illegal_custom_event(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    illegal_constructor(global_object, "CustomEvent")
}

fn construct_event_target_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let structure = derived_structure(global_object, call, instance_structure)?;
    let instance = JSFinalObject::create(global_object.vm(), &structure).as_value();
    TARGETS.with(|targets| targets.borrow_mut().insert(instance.encode(), Vec::new()));
    Ok(instance)
}

fn missing_args(global_object: &JSGlobalObject) -> Thrown {
    throw_coded_type_error(global_object, "Not enough arguments", "ERR_MISSING_ARGS")
}

/// `detail` de `CustomEvent`: `undefined` vira `null`.
fn detail_or_null(value: JSValue) -> JSValue {
    if value.is_undefined() {
        js_null()
    } else {
        value
    }
}

/// `new Event(type, init)`.
fn construct_event_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    construct_event_of(global_object, call, false)
}

/// `new CustomEvent(type, init)`: o `Event` mais o `detail` do `init`, lido depois de `composed`.
fn construct_custom_event_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    construct_event_of(global_object, call, true)
}

fn construct_event_of(global_object: &JSGlobalObject, call: &HostCall, custom: bool) -> HostResult {
    if call.argument_count() < 1 {
        return Err(missing_args(global_object));
    }
    let kind = kind_argument(global_object, call.argument(0))?;
    let options = call.argument(1);
    // Medido no bun 1.4.2: o `CustomEvent` não acrescenta o ` Received ...` ao texto do `Event`.
    if custom && !(options.is_undefined_or_null() || options.is_object()) {
        return Err(throw_coded_type_error(global_object, "The \"options\" argument must be of type object.", "ERR_INVALID_ARG_TYPE"));
    }
    check_options(global_object, options)?;
    let bubbles = option_flag(global_object, options, "bubbles")?;
    let cancelable = option_flag(global_object, options, "cancelable")?;
    let composed = option_flag(global_object, options, "composed")?;
    let detail = if !custom {
        None
    } else if options.is_object() {
        Some(detail_or_null(get_object_property(global_object, options, &Identifier::from_span(global_object.vm(), b"detail"))?))
    } else {
        Some(js_null())
    };
    let structure = derived_structure(global_object, call, instance_structure)?;
    Ok(build_event(global_object, &structure, EventState::new(kind, bubbles, cancelable, composed, detail, false)))
}

impl EventState {
    fn new(kind: Vec<u16>, bubbles: bool, cancelable: bool, composed: bool, detail: Option<JSValue>, trusted: bool) -> Self {
        EventState {
            kind,
            bubbles,
            cancelable,
            composed,
            default_prevented: false,
            stop_propagation: false,
            stop_immediate: false,
            target: js_null(),
            current_target: js_null(),
            phase: 0,
            dispatching: false,
            class: if detail.is_some() { "CustomEvent" } else { "Event" },
            fields: Vec::new(),
            detail,
            trusted,
            in_passive: false,
        }
    }
}

/// A instância de `Event` com `state`, de `structure` (o acessor próprio `isTrusted` incluído).
fn build_event(global_object: &JSGlobalObject, structure: &StructureRef, state: EventState) -> JSValue {
    let vm = global_object.vm();
    let instance = JSFinalObject::create(vm, structure);
    // `isTrusted` é `[LegacyUnforgeable]`: acessor próprio de cada instância, posto COM transição. A estrutura é a
    // mesma para todos os eventos do realm (`EVENT_STRUCTURES` e irmãs); o `without_transition` a mutaria no primeiro
    // evento e o segundo repetiria a chave (`Structure::add` acusa). A transição `estrutura + isTrusted` fica na tabela.
    let getter = crate::runtime::js_custom_accessor_function::create_host_custom_accessor_getter_function(vm, global_object, "isTrusted", event_is_trusted);
    let accessor = crate::runtime::js_getter_setter::GetterSetter::create_from_values(vm, getter.as_value(), js_undefined());
    instance.put_direct_non_index_accessor(
        vm,
        &crate::runtime::native_class_support::property_key(vm, "isTrusted"),
        &accessor,
        DONT_DELETE | crate::runtime::property_attribute::ACCESSOR,
    );
    let instance = instance.as_value();
    EVENTS.with(|events| events.borrow_mut().insert(instance.encode(), state));
    instance
}

host_function!(call_event_target, illegal_event_target);
host_function!(construct_event_target, construct_event_target_body);
host_function!(call_event, illegal_event);
host_function!(construct_event, construct_event_body);
host_function!(call_custom_event, illegal_custom_event);
host_function!(construct_custom_event, construct_custom_event_body);

/// O membro `name` do dicionário `options` (`undefined` se `options` não é objeto).
fn init_member(global_object: &JSGlobalObject, options: JSValue, name: &str) -> Result<JSValue, Thrown> {
    if !options.is_object() {
        return Ok(js_undefined());
    }
    get_object_property(global_object, options, &Identifier::from_span(global_object.vm(), name.as_bytes()))
}

/// Membro `DOMString` de um dicionário: `undefined` dá `""`.
fn init_string(global_object: &JSGlobalObject, options: JSValue, name: &str) -> Result<JSValue, Thrown> {
    let value = init_member(global_object, options, name)?;
    string_of(global_object, value)
}

fn string_of(global_object: &JSGlobalObject, value: JSValue) -> Result<JSValue, Thrown> {
    if value.is_undefined() {
        return Ok(string_value(global_object, &[]));
    }
    let text = pending_or(global_object, value.to_wtf_string())?;
    Ok(JSValue::from_js_string(js_string(global_object.vm(), &text)))
}

/// Membro `unsigned long` (`bits` 32) ou `unsigned short` (`bits` 16) de um dicionário: `undefined` dá 0.
fn init_unsigned(global_object: &JSGlobalObject, options: JSValue, name: &str, bits: u32) -> Result<JSValue, Thrown> {
    let value = init_member(global_object, options, name)?;
    if value.is_undefined() {
        return Ok(js_number(0));
    }
    let number = pending_or(global_object, value.to_uint32())?;
    Ok(js_number(if bits == 16 { f64::from(number as u16) } else { f64::from(number) }))
}

/// O valor como o bun o mostra na mensagem de `MessageEvent`: `Bun__inspect_singleline` (inspeção numa linha só).
fn describe_member(global_object: &JSGlobalObject, value: JSValue) -> Result<String, Thrown> {
    let mut shown = Vec::new();
    if !crate::runtime::console_format::Formatter::single_line().push_object(global_object, &mut shown, value) {
        return Err(Thrown::Pending);
    }
    Ok(String::from_utf16_lossy(&shown))
}

/// Os campos próprios do dicionário de `class`, na ordem em que o bun os lê (os três de `Event` já foram lidos).
fn read_extra_fields(global_object: &JSGlobalObject, options: JSValue, class: &str) -> Result<Vec<JSValue>, Thrown> {
    match class {
        // Ordem alfabética: colno, error, filename, lineno, message.
        "ErrorEvent" => {
            let colno = init_unsigned(global_object, options, "colno", 32)?;
            let error = detail_or_null(init_member(global_object, options, "error")?);
            let filename = init_string(global_object, options, "filename")?;
            let lineno = init_unsigned(global_object, options, "lineno", 32)?;
            let message = init_string(global_object, options, "message")?;
            Ok(vec![message, filename, lineno, colno, error])
        }
        // data, lastEventId, origin, ports, source.
        "MessageEvent" => {
            let data = detail_or_null(init_member(global_object, options, "data")?);
            let last_event_id = init_string(global_object, options, "lastEventId")?;
            let origin = init_string(global_object, options, "origin")?;
            let ports = init_member(global_object, options, "ports")?;
            let mut port_list: Vec<JSValue> = Vec::new();
            if !ports.is_undefined() {
                // `sequence<MessagePort>`: qualquer iterável via `@@iterator`; o primeiro elemento já não é MessagePort.
                let iterator_method = if ports.is_object() {
                    crate::runtime::iterator_operations::get_value_property(
                        global_object,
                        ports,
                        &PropertyName::from_identifier(&global_object.vm().property_names.iterator_symbol),
                    )?
                } else {
                    js_undefined()
                };
                if crate::runtime::call_data::get_call_data(iterator_method).is_none() {
                    let shown = describe_member(global_object, ports)?;
                    return Err(throw_native_type_error(global_object, &format!("MessageEvent constructor: eventInitDict.ports ({shown}) is not iterable.")));
                }
                let mut index = 0usize;
                crate::runtime::iterator_operations::for_each_in_iterable_with_method(global_object, ports, iterator_method, |element| {
                    if crate::runtime::message_channel::is_port(element) {
                        port_list.push(element);
                    } else {
                        let shown = describe_member(global_object, element)?;
                        let message = format!("MessageEvent constructor: Expected eventInitDict.ports[{index}] (\"{shown}\") to be an instance of MessagePort.");
                        return Err(throw_native_type_error(global_object, &message));
                    }
                    index += 1;
                    Ok(())
                })?;
            }
            let source = init_member(global_object, options, "source")?;
            if !source.is_undefined_or_null() && !crate::runtime::message_channel::is_port(source) {
                let shown = describe_member(global_object, source)?;
                return Err(throw_native_type_error(
                    global_object,
                    &format!("MessageEvent constructor: Expected eventInitDict.source (\"{shown}\") to be an instance of MessagePort."),
                ));
            }
            Ok(vec![data, origin, last_event_id, if source.is_undefined() { js_null() } else { source }, frozen_array(global_object, &port_list)?])
        }
        // code, reason, wasClean.
        _ => {
            let code = init_unsigned(global_object, options, "code", 16)?;
            // Medido no bun 1.4.2: o `reason` símbolo do `CloseEvent` lança o texto do `String(símbolo)` do JS, não o do
            // conversor nativo que os outros eventos usam (`Cannot convert a symbol to a string`).
            let reason = init_member(global_object, options, "reason")?;
            if reason.is_symbol() {
                return Err(Thrown::TypeError("Cannot convert a Symbol value to a string".to_string()));
            }
            let reason = string_of(global_object, reason)?;
            let was_clean = boolean(init_member(global_object, options, "wasClean")?.to_boolean());
            Ok(vec![was_clean, code, reason])
        }
    }
}

/// O array congelado como o `Object.freeze` do JS (o `length` também fica só de leitura, e `Object.isFrozen` é `true`,
/// medido no bun 1.4.2 com o `ports` de `new MessageEvent('x')`); o `freeze` de baixo nível deixa o `length` gravável.
fn frozen_array(global_object: &JSGlobalObject, elements: &[JSValue]) -> Result<JSValue, Thrown> {
    let array = construct_array(global_object.vm(), &global_object.array_structure(), elements);
    crate::runtime::object_constructor::object_constructor_freeze(global_object, &array)?;
    Ok(array.as_value())
}

/// `new ErrorEvent(type, init)`, `new MessageEvent(type, init)` e `new CloseEvent(type, init)`.
fn construct_extra_event(global_object: &JSGlobalObject, call: &HostCall, class: &'static str) -> HostResult {
    if call.argument_count() < 1 {
        return Err(missing_args(global_object));
    }
    let kind = kind_argument(global_object, call.argument(0))?;
    let options = call.argument(1);
    // Medido no bun 1.4.2: `ErrorEvent`, `MessageEvent` e `CloseEvent` recusam o `init` que não é objeto com o
    // `TypeError` genérico do conversor de dicionário (`Type error`, sem `code`).
    if !(options.is_undefined_or_null() || options.is_object()) {
        return Err(Thrown::TypeError("Type error".to_string()));
    }
    let bubbles = option_flag(global_object, options, "bubbles")?;
    let cancelable = option_flag(global_object, options, "cancelable")?;
    let composed = option_flag(global_object, options, "composed")?;
    let fields = read_extra_fields(global_object, options, class)?;
    let structure = derived_structure(global_object, call, instance_structure)?;
    let mut state = EventState::new(kind, bubbles, cancelable, composed, None, false);
    state.class = class;
    state.fields = fields;
    Ok(build_event(global_object, &structure, state))
}

macro_rules! extra_event {
    ($class:literal, $call:ident, $illegal_body:ident, $construct:ident, $construct_body:ident) => {
        fn $illegal_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
            illegal_constructor(global_object, $class)
        }
        fn $construct_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
            construct_extra_event(global_object, call, $class)
        }
        host_function!($call, $illegal_body);
        host_function!($construct, $construct_body);
    };
}

extra_event!("ErrorEvent", call_error_event, error_event_illegal_body, construct_error_event, construct_error_event_body);
extra_event!("MessageEvent", call_message_event, message_event_illegal_body, construct_message_event, construct_message_event_body);
extra_event!("CloseEvent", call_close_event, close_event_illegal_body, construct_close_event, construct_close_event_body);

macro_rules! field_getter {
    ($function:ident, $body:ident, $class:literal, $name:literal, $index:literal) => {
        fn $body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
            Ok(getter_state(global_object, call, $class, "getter", $name)?.fields[$index])
        }
        host_function!($function, $body);
    };
}

field_getter!(error_event_message, error_event_message_body, "ErrorEvent", "message", 0);
field_getter!(error_event_filename, error_event_filename_body, "ErrorEvent", "filename", 1);
field_getter!(error_event_lineno, error_event_lineno_body, "ErrorEvent", "lineno", 2);
field_getter!(error_event_colno, error_event_colno_body, "ErrorEvent", "colno", 3);
field_getter!(error_event_error, error_event_error_body, "ErrorEvent", "error", 4);
field_getter!(message_event_data, message_event_data_body, "MessageEvent", "data", 0);
field_getter!(message_event_origin, message_event_origin_body, "MessageEvent", "origin", 1);
field_getter!(message_event_last_event_id, message_event_last_event_id_body, "MessageEvent", "lastEventId", 2);
field_getter!(message_event_source, message_event_source_body, "MessageEvent", "source", 3);
field_getter!(message_event_ports, message_event_ports_body, "MessageEvent", "ports", 4);
field_getter!(close_event_was_clean, close_event_was_clean_body, "CloseEvent", "wasClean", 0);
field_getter!(close_event_code, close_event_code_body, "CloseEvent", "code", 1);
field_getter!(close_event_reason, close_event_reason_body, "CloseEvent", "reason", 2);

/// `initMessageEvent(type, bubbles, cancelable, data, origin, lastEventId, source, ports)`: como `initEvent`, mais os
/// campos (nada muda durante o despacho).
fn message_event_init_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let before = method_state(global_object, call, "MessageEvent", "initMessageEvent")?;
    init_event(global_object, call, "MessageEvent", "initMessageEvent")?;
    if before.dispatching {
        return Ok(js_undefined());
    }
    let data = detail_or_null(call.argument(3));
    let origin = string_of(global_object, call.argument(4))?;
    let last_event_id = string_of(global_object, call.argument(5))?;
    update_event(call.this_value(), |state| {
        state.fields[0] = data;
        state.fields[1] = origin;
        state.fields[2] = last_event_id;
    });
    Ok(js_undefined())
}

host_function!(message_event_init, message_event_init_body);

/// O estado do `this` se ele é instância de `class` (`Event` aceita também os `CustomEvent`).
fn state_of(call: &HostCall, class: &str) -> Option<EventState> {
    event_state(call.this_value()).filter(|state| class == "Event" || state.class == class)
}

/// O estado do `this` de um getter/setter de `class`, ou o `TypeError` do bun.
fn getter_state(global_object: &JSGlobalObject, call: &HostCall, class: &str, what: &str, name: &str) -> Result<EventState, Thrown> {
    state_of(call, class).ok_or_else(|| throw_native_type_error(global_object, &format!("The {class}.{name} {what} can only be used on instances of {class}")))
}

/// O estado do `this` de um método de `class`, ou o `TypeError` com `ERR_INVALID_THIS`.
fn method_state(global_object: &JSGlobalObject, call: &HostCall, class: &str, name: &str) -> Result<EventState, Thrown> {
    state_of(call, class)
        .ok_or_else(|| throw_coded_type_error(global_object, &format!("Can only call {class}.{name} on instances of {class}"), "ERR_INVALID_THIS"))
}

macro_rules! event_getter {
    ($function:ident, $body:ident, $name:literal, |$global:ident, $state:ident| $value:expr) => {
        fn $body($global: &JSGlobalObject, call: &HostCall) -> HostResult {
            let $state = getter_state($global, call, "Event", "getter", $name)?;
            Ok($value)
        }
        host_function!($function, $body);
    };
}

fn boolean(value: bool) -> JSValue {
    JSValue::Bool(value)
}

event_getter!(event_type, event_type_body, "type", |global, state| string_value(global, &state.kind));
event_getter!(event_target, event_target_body, "target", |_global, state| state.target);
event_getter!(event_current_target, event_current_target_body, "currentTarget", |_global, state| state.current_target);
event_getter!(event_event_phase, event_event_phase_body, "eventPhase", |_global, state| js_number(state.phase));
event_getter!(event_cancel_bubble, event_cancel_bubble_body, "cancelBubble", |_global, state| boolean(state.stop_propagation));
event_getter!(event_bubbles, event_bubbles_body, "bubbles", |_global, state| boolean(state.bubbles));
event_getter!(event_cancelable, event_cancelable_body, "cancelable", |_global, state| boolean(state.cancelable));
event_getter!(event_default_prevented, event_default_prevented_body, "defaultPrevented", |_global, state| boolean(state.default_prevented));
event_getter!(event_composed, event_composed_body, "composed", |_global, state| boolean(state.composed));
// O `timeStamp` do bun 1.4.2 é sempre 0, em eventos construídos e nos disparados pelo runtime (medido).
event_getter!(event_time_stamp, event_time_stamp_body, "timeStamp", |_global, _state| js_number(0.0));
event_getter!(event_src_element, event_src_element_body, "srcElement", |_global, state| state.target);
event_getter!(event_return_value, event_return_value_body, "returnValue", |_global, state| boolean(!state.default_prevented));
event_getter!(event_is_trusted, event_is_trusted_body, "isTrusted", |_global, state| boolean(state.trusted));

fn event_set_cancel_bubble_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    getter_state(global_object, call, "Event", "setter", "cancelBubble")?;
    if call.argument(0).to_boolean() {
        update_event(call.this_value(), |state| state.stop_propagation = true);
    }
    Ok(js_undefined())
}

fn event_set_return_value_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    getter_state(global_object, call, "Event", "setter", "returnValue")?;
    if !call.argument(0).to_boolean() {
        update_event(call.this_value(), |state| state.default_prevented |= state.cancelable && !state.in_passive);
    }
    Ok(js_undefined())
}

fn event_composed_path_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let state = method_state(global_object, call, "Event", "composedPath")?;
    let path: Vec<JSValue> = if state.dispatching { vec![state.current_target] } else { Vec::new() };
    Ok(construct_array(global_object.vm(), &global_object.array_structure(), &path).as_value())
}

fn event_stop_propagation_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    method_state(global_object, call, "Event", "stopPropagation")?;
    update_event(call.this_value(), |state| state.stop_propagation = true);
    Ok(js_undefined())
}

fn event_stop_immediate_propagation_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    method_state(global_object, call, "Event", "stopImmediatePropagation")?;
    update_event(call.this_value(), |state| {
        state.stop_propagation = true;
        state.stop_immediate = true;
    });
    Ok(js_undefined())
}

fn event_prevent_default_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    method_state(global_object, call, "Event", "preventDefault")?;
    update_event(call.this_value(), |state| state.default_prevented |= state.cancelable && !state.in_passive);
    Ok(js_undefined())
}

fn event_init_event_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    init_event(global_object, call, "Event", "initEvent")
}

fn custom_event_init_custom_event_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    init_event(global_object, call, "CustomEvent", "initCustomEvent")
}

/// `initEvent(type, bubbles, cancelable)` e `initCustomEvent(type, bubbles, cancelable, detail)`: durante o despacho
/// os argumentos são validados e nada muda.
fn init_event(global_object: &JSGlobalObject, call: &HostCall, class: &str, name: &str) -> HostResult {
    let state = method_state(global_object, call, class, name)?;
    if call.argument_count() < 1 {
        return Err(missing_args(global_object));
    }
    let kind = kind_argument(global_object, call.argument(0))?;
    if state.dispatching {
        return Ok(js_undefined());
    }
    let (bubbles, cancelable) = (call.argument(1).to_boolean(), call.argument(2).to_boolean());
    update_event(call.this_value(), |state| {
        state.kind = kind;
        state.bubbles = bubbles;
        state.cancelable = cancelable;
        state.default_prevented = false;
        state.stop_propagation = false;
        state.stop_immediate = false;
        state.target = js_null();
        if state.detail.is_some() && class == "CustomEvent" {
            state.detail = Some(detail_or_null(call.argument(3)));
        }
    });
    Ok(js_undefined())
}

fn custom_event_detail_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(getter_state(global_object, call, "CustomEvent", "getter", "detail")?.detail.unwrap_or_else(js_null))
}

host_function!(event_set_cancel_bubble, event_set_cancel_bubble_body);
host_function!(event_set_return_value, event_set_return_value_body);
host_function!(event_composed_path, event_composed_path_body);
host_function!(event_stop_propagation, event_stop_propagation_body);
host_function!(event_stop_immediate_propagation, event_stop_immediate_propagation_body);
host_function!(event_prevent_default, event_prevent_default_body);
host_function!(event_init_event, event_init_event_body);
host_function!(custom_event_init_custom_event, custom_event_init_custom_event_body);
host_function!(custom_event_detail, custom_event_detail_body);

fn check_target(global_object: &JSGlobalObject, call: &HostCall, target: JSValue, method: &str, required: usize) -> Result<(), Thrown> {
    if !is_target(target) {
        return Err(throw_coded_type_error(global_object, &format!("Can only call EventTarget.{method} on instances of EventTarget"), "ERR_INVALID_THIS"));
    }
    if call.argument_count() < required {
        return Err(missing_args(global_object));
    }
    Ok(())
}

/// O ouvinte lido dos argumentos `(type, listener, options)` e, em `addEventListener`, o `signal` das opções;
/// `None` se o ouvinte é `null`/`undefined`. A ordem de leitura das opções é `capture`, `once`, `passive`, `signal`.
fn read_listener(global_object: &JSGlobalObject, call: &HostCall, method: &str) -> Result<Option<(Listener, Option<JSValue>)>, Thrown> {
    let kind = kind_argument(global_object, call.argument(0))?;
    let callback = call.argument(1);
    if callback.is_undefined_or_null() {
        return Ok(None);
    }
    if !callback.is_object() {
        return Err(throw_coded_type_error(global_object, &format!("Argument 2 ('listener') to EventTarget.{method} must be an object"), "ERR_INVALID_ARG_TYPE"));
    }
    let options = call.argument(2);
    let adding = method == "addEventListener";
    let (capture, once, passive) = if options.is_object() {
        let capture = option_flag(global_object, options, "capture")?;
        // `removeEventListener` lê só `capture` (medido).
        if adding {
            (capture, option_flag(global_object, options, "once")?, option_flag(global_object, options, "passive")?)
        } else {
            (capture, false, false)
        }
    } else {
        (options.to_boolean(), false, false)
    };
    let mut signal = None;
    if adding && options.is_object() {
        let value = get_object_property(global_object, options, &Identifier::from_span(global_object.vm(), b"signal"))?;
        if !value.is_undefined() {
            if !crate::runtime::abort_signal::is_signal(value) {
                return Err(throw_native_type_error(global_object, "Type error"));
            }
            signal = Some(value);
        }
    }
    Ok(Some((Listener { kind, callback, capture, once, passive, attr: false, id: next_listener_id() }, signal)))
}

fn same_listener(a: &Listener, b: &Listener) -> bool {
    a.kind == b.kind && a.callback.encode() == b.callback.encode() && a.capture == b.capture && a.attr == b.attr
}

fn event_target_add_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    add_listener_to(global_object, call, prototype_target(global_object, call))
}

/// O alvo dos métodos de `EventTarget.prototype`: o `this`, ou o `globalThis` (que é um `EventTarget`) quando o `this`
/// é `null` ou `undefined`, como no bun 1.4.2 (`addEventListener.call(null)` é `Not enough arguments`, e
/// `dispatchEvent.call(null, new Event('a'))` despacha no global).
fn prototype_target(global_object: &JSGlobalObject, call: &HostCall) -> JSValue {
    let this = call.this_value();
    if this.is_undefined_or_null() { global_target(global_object) } else { this }
}

fn add_listener_to(global_object: &JSGlobalObject, call: &HostCall, target: JSValue) -> HostResult {
    check_target(global_object, call, target, "addEventListener", 2)?;
    if let Some((listener, signal)) = read_listener(global_object, call, "addEventListener")? {
        // Sinal já abortado: o ouvinte nem entra (medido).
        if signal.is_some_and(crate::runtime::abort_signal::is_aborted) {
            return Ok(js_undefined());
        }
        let added = TARGETS.with(|targets| match targets.borrow_mut().get_mut(&target.encode()) {
            Some(list) if !list.iter().any(|existing| same_listener(existing, &listener)) => {
                list.push(listener.clone());
                true
            }
            _ => false,
        });
        if let (true, Some(signal)) = (added, signal) {
            SIGNAL_LISTENERS.with(|links| links.borrow_mut().entry(signal.encode()).or_default().push((target, listener)));
        }
    }
    Ok(js_undefined())
}

fn event_target_remove_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    remove_listener_from(global_object, call, prototype_target(global_object, call))
}

fn remove_listener_from(global_object: &JSGlobalObject, call: &HostCall, target: JSValue) -> HostResult {
    check_target(global_object, call, target, "removeEventListener", 2)?;
    if let Some((listener, _)) = read_listener(global_object, call, "removeEventListener")? {
        remove_listener(target, &listener);
    }
    Ok(js_undefined())
}

fn remove_listener(target: JSValue, listener: &Listener) {
    TARGETS.with(|targets| {
        if let Some(list) = targets.borrow_mut().get_mut(&target.encode()) {
            list.retain(|existing| !same_listener(existing, listener));
        }
    });
}

fn is_registered(target: JSValue, listener: &Listener) -> bool {
    TARGETS.with(|targets| targets.borrow().get(&target.encode()).is_some_and(|list| list.iter().any(|existing| existing.id == listener.id)))
}

fn remove_listener_by_id(target: JSValue, id: u64) {
    TARGETS.with(|targets| {
        if let Some(list) = targets.borrow_mut().get_mut(&target.encode()) {
            list.retain(|existing| existing.id != id);
        }
    });
}

/// Chama um ouvinte; a exceção vai ao relatório de erro não capturado. `false` se o programa foi terminado.
fn invoke(global_object: &JSGlobalObject, target: JSValue, event: JSValue, listener: &Listener) -> Result<bool, Thrown> {
    let callback = listener.callback;
    if listener.attr && !callback.is_callable() {
        return Ok(true);
    }
    let result = if callback.is_callable() {
        call_microtask(global_object, callback, target, &[event], "callback is not a function")
    } else {
        let vm = global_object.vm();
        // A exceção do getter `handleEvent` vai ao relatório de erro e o despacho segue (medido no bun).
        let handler = match get_object_property(global_object, callback, &Identifier::from_span(vm, b"handleEvent")) {
            Ok(handler) => handler,
            Err(thrown) => {
                let Some(exception) = vm.exception() else { return Err(thrown) };
                vm.clear_exception();
                vm.report_unhandled_error(global_object, exception.value());
                return Ok(true);
            }
        };
        call_microtask(global_object, handler, callback, &[event], "'handleEvent' property of event listener should be callable")
    };
    match result {
        Err(crate::runtime::js_promise_host::Thrown::Value(error)) => global_object.vm().report_unhandled_error(global_object, error),
        Err(crate::runtime::js_promise_host::Thrown::Termination) => return Ok(false),
        Ok(_) => {}
    }
    Ok(true)
}

fn event_target_dispatch_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    dispatch_event_on(global_object, call, prototype_target(global_object, call))
}

fn dispatch_event_on(global_object: &JSGlobalObject, call: &HostCall, target: JSValue) -> HostResult {
    check_target(global_object, call, target, "dispatchEvent", 1)?;
    dispatch_event(global_object, target, call.argument(0))
}

/// O alvo das funções globais `addEventListener`, `removeEventListener` e `dispatchEvent`: o `globalThis`, seja qual for o
/// `this` (medido no bun: `addEventListener.call({}, ...)` registra no global).
fn global_target(global_object: &JSGlobalObject) -> JSValue {
    global_object.global_this().map_or_else(js_undefined, |global_this| global_this.as_value())
}

fn global_add_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    add_listener_to(global_object, call, global_target(global_object))
}

fn global_remove_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    remove_listener_from(global_object, call, global_target(global_object))
}

fn global_dispatch_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    dispatch_event_on(global_object, call, global_target(global_object))
}

host_function!(global_add_event_listener, global_add_body);
host_function!(global_remove_event_listener, global_remove_body);
host_function!(global_dispatch_event, global_dispatch_body);

/// `onmessage` e `onerror` do global: atributos de evento do `EventTarget` que é o próprio global. Medido no bun 1.4.2:
/// descritor `{value: null, writable, enumerable, configurable}`; valor que não é objeto vira `null`.
/// O tipo de evento do atributo: `ERROR` escolhe `onerror`, senão `onmessage`.
const fn handler_kind<const ERROR: bool>() -> &'static str {
    if ERROR {
        "error"
    } else {
        "message"
    }
}

fn global_handler_getter<const ERROR: bool>(global_object: &JSGlobalObject, _this: EncodedJSValue, _name: &PropertyName) -> EncodedJSValue {
    event_handler(global_target(global_object), handler_kind::<ERROR>()).encode()
}

fn global_handler_setter<const ERROR: bool>(global_object: &JSGlobalObject, _this: EncodedJSValue, value: EncodedJSValue, _name: &PropertyName) -> bool {
    set_event_handler(global_target(global_object), handler_kind::<ERROR>(), JSValue::decode(value));
    true
}

/// Faz do global um `EventTarget` (a tabela é por programa: `reset_for_program` a esvazia) e instala `addEventListener`,
/// `removeEventListener` e `dispatchEvent`. Roda antes da reordenação das chaves (`ORDER` posiciona os três).
pub fn install_global_event_target(global_object: &JSGlobalObject) {
    register_target(global_target(global_object));
    for (name, length, function) in [
        ("addEventListener", 2, global_add_event_listener as NativeFunction),
        ("removeEventListener", 2, global_remove_event_listener),
        ("dispatchEvent", 1, global_dispatch_event),
    ] {
        crate::runtime::native_class_support::install_global_function(global_object, name, length, function);
    }
}

/// `onmessage` e `onerror` (valores customizados que a reordenação não move): roda depois de `self`, a ordem do bun
/// termina em `self, onmessage, onerror`.
pub fn install_global_event_handlers(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    for (name, getter, setter) in [
        ("onmessage", global_handler_getter::<false> as GetValueFunc, global_handler_setter::<false> as PutValueFunc),
        ("onerror", global_handler_getter::<true>, global_handler_setter::<true>),
    ] {
        let key = PropertyName::from_identifier(&Identifier::from_span(vm, name.as_bytes()));
        global_object.put_direct_custom_getter_setter_without_transition(vm, &key, &CustomGetterSetter::create(vm, getter, Some(setter)), CUSTOM_VALUE);
    }
}

/// `target.dispatchEvent(event)`; `event` que não é `Event` é o `TypeError` do bun.
pub(crate) fn dispatch_event(global_object: &JSGlobalObject, target: JSValue, event: JSValue) -> HostResult {
    let Some(state) = event_state(event) else {
        return Err(throw_coded_type_error(global_object, "Argument 1 ('event') to EventTarget.dispatchEvent must be an instance of Event", "ERR_INVALID_ARG_TYPE"));
    };
    if state.dispatching {
        let message = make_string_dyn(&[&"The event \"", &wtf_from_units(&state.kind), &"\" is already being dispatched"]);
        return Err(throw_coded_error_with_message(global_object, message, "ERR_EVENT_RECURSION"));
    }
    // No global o `target` e o `currentTarget` do evento são o objeto global cru, não o `JSGlobalProxy` que é o
    // `globalThis` (o `toJS` do `GlobalScope` do bun; medido no bun 1.4.2: `e.target === globalThis` é `false`, mas o
    // protótipo é o mesmo e as chaves também). Os ouvintes continuam registrados no `globalThis`.
    let exposed = if target.encode() == global_target(global_object).encode() { global_object.as_value() } else { target };
    update_event(event, |state| {
        state.target = exposed;
        state.current_target = exposed;
        state.phase = AT_TARGET;
        state.dispatching = true;
    });
    let mut snapshot: Vec<Listener> =
        TARGETS.with(|targets| targets.borrow().get(&target.encode()).map(|list| list.iter().filter(|l| l.kind == state.kind).cloned().collect()).unwrap_or_default());
    // No próprio alvo os ouvintes de captura rodam antes dos demais (estável dentro de cada grupo; medido).
    snapshot.sort_by_key(|listener| !listener.capture);
    let mut outcome = Ok(());
    for listener in &snapshot {
        // `stopImmediatePropagation` vale também quando chamado antes do despacho (medido: nenhum ouvinte roda).
        if event_state(event).is_some_and(|state| state.stop_immediate) {
            break;
        }
        if !is_registered(target, listener) {
            continue;
        }
        if listener.once {
            remove_listener_by_id(target, listener.id);
        }
        update_event(event, |state| state.in_passive = listener.passive);
        let result = invoke(global_object, target, event, listener);
        update_event(event, |state| state.in_passive = false);
        match result {
            Ok(true) => {}
            Ok(false) => break,
            Err(thrown) => {
                outcome = Err(thrown);
                break;
            }
        }
    }
    update_event(event, |state| {
        state.current_target = js_null();
        state.phase = 0;
        state.dispatching = false;
        state.stop_propagation = false;
        state.stop_immediate = false;
    });
    outcome?;
    let prevented = event_state(event).is_some_and(|state| state.default_prevented);
    Ok(boolean(!prevented))
}

host_function!(event_target_add, event_target_add_body);
host_function!(event_target_remove, event_target_remove_body);
host_function!(event_target_dispatch, event_target_dispatch_body);

pub(crate) fn put_methods(global_object: &JSGlobalObject, prototype: &crate::runtime::js_object::JSObject, methods: &[(&str, u32, NativeFunction)]) {
    let vm = global_object.vm();
    for (name, length, function) in methods {
        put_direct_native_function_without_transition(
            vm,
            global_object,
            prototype,
            &Identifier::from_span(vm, name.as_bytes()),
            *length,
            *function,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            0,
        );
    }
}

/// Instala `EventTarget` e `Event` no global, nesta ordem de criação (a posição no global vem de `ORDER`).
pub fn install_event_target(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let constructor_key = PropertyName::from_identifier(&vm.property_names.constructor);

    let (target_prototype, target_constructor) =
        create_native_class(global_object, &EVENT_TARGET_PROTOTYPE_S_INFO, &CONSTRUCTOR_S_INFO, "EventTarget", call_event_target, construct_event_target);
    target_prototype.put_direct(vm, &constructor_key, target_constructor.as_value(), DONT_ENUM);
    put_methods(
        global_object,
        &target_prototype,
        &[("addEventListener", 2, event_target_add as NativeFunction), ("removeEventListener", 2, event_target_remove), ("dispatchEvent", 1, event_target_dispatch)],
    );
    put_to_string_tag(vm, &target_prototype, "EventTarget");
    install_global(global_object, "EventTarget", target_constructor.as_value());

    let (prototype, constructor) = crate::runtime::native_class_support::create_native_class_with_length(
        global_object,
        &EVENT_PROTOTYPE_S_INFO,
        &CONSTRUCTOR_S_INFO,
        "Event",
        1,
        call_event,
        construct_event,
    );
    prototype.put_direct(vm, &constructor_key, constructor.as_value(), DONT_ENUM);
    let accessors: [(&str, NativeFunction, Option<NativeFunction>); 12] = [
        ("type", event_type, None),
        ("target", event_target, None),
        ("currentTarget", event_current_target, None),
        ("eventPhase", event_event_phase, None),
        ("cancelBubble", event_cancel_bubble, Some(event_set_cancel_bubble)),
        ("bubbles", event_bubbles, None),
        ("cancelable", event_cancelable, None),
        ("defaultPrevented", event_default_prevented, None),
        ("composed", event_composed, None),
        ("timeStamp", event_time_stamp, None),
        ("srcElement", event_src_element, None),
        ("returnValue", event_return_value, Some(event_set_return_value)),
    ];
    for (name, getter, setter) in accessors {
        put_native_accessor(vm, global_object, &prototype, name, getter, setter, 0);
    }
    put_methods(
        global_object,
        &prototype,
        &[
            ("composedPath", 0, event_composed_path as NativeFunction),
            ("stopPropagation", 0, event_stop_propagation),
            ("stopImmediatePropagation", 0, event_stop_immediate_propagation),
            ("preventDefault", 0, event_prevent_default),
            ("initEvent", 1, event_init_event),
        ],
    );
    for (name, value) in [("NONE", 0), ("CAPTURING_PHASE", 1), ("AT_TARGET", 2), ("BUBBLING_PHASE", 3)] {
        prototype.put_direct(vm, &crate::runtime::native_class_support::property_key(vm, name), js_number(value), READ_ONLY | DONT_DELETE);
    }
    put_to_string_tag(vm, &prototype, "Event");
    for (name, value) in [("NONE", 0), ("CAPTURING_PHASE", 1), ("AT_TARGET", 2), ("BUBBLING_PHASE", 3)] {
        constructor.put_direct(vm, &crate::runtime::native_class_support::property_key(vm, name), js_number(value), READ_ONLY | DONT_DELETE);
    }
    install_global(global_object, "Event", constructor.as_value());
    let event_structure = instance_structure(vm, Some(global_object), prototype.as_value());
    EVENT_STRUCTURES.with(|structures| structures.borrow_mut().push((global_object.cell_id(), event_structure)));
    let (custom_prototype, custom_constructor) = create_native_subclass(
        global_object,
        (prototype.as_value(), constructor.as_value()),
        &CUSTOM_EVENT_PROTOTYPE_S_INFO,
        &CONSTRUCTOR_S_INFO,
        "CustomEvent",
        1,
        call_custom_event,
        construct_custom_event,
    );
    custom_prototype.put_direct(vm, &constructor_key, custom_constructor.as_value(), DONT_ENUM);
    put_native_accessor(vm, global_object, &custom_prototype, "detail", custom_event_detail, None, 0);
    put_methods(global_object, &custom_prototype, &[("initCustomEvent", 1, custom_event_init_custom_event as NativeFunction)]);
    put_to_string_tag(vm, &custom_prototype, "CustomEvent");
    install_global(global_object, "CustomEvent", custom_constructor.as_value());
    type Getter = NativeFunction;
    let extra: [(&'static ClassInfo, &str, NativeFunction, NativeFunction, &[(&str, Getter)], &[(&str, u32, NativeFunction)]); 3] = [
        (
            &ERROR_EVENT_PROTOTYPE_S_INFO,
            "ErrorEvent",
            call_error_event,
            construct_error_event,
            &[
                ("message", error_event_message),
                ("filename", error_event_filename),
                ("lineno", error_event_lineno),
                ("colno", error_event_colno),
                ("error", error_event_error),
            ],
            &[],
        ),
        (
            &MESSAGE_EVENT_PROTOTYPE_S_INFO,
            "MessageEvent",
            call_message_event,
            construct_message_event,
            &[
                ("origin", message_event_origin),
                ("lastEventId", message_event_last_event_id),
                ("source", message_event_source),
                ("data", message_event_data),
                ("ports", message_event_ports),
            ],
            &[("initMessageEvent", 1, message_event_init)],
        ),
        (
            &CLOSE_EVENT_PROTOTYPE_S_INFO,
            "CloseEvent",
            call_close_event,
            construct_close_event,
            &[("wasClean", close_event_was_clean), ("code", close_event_code), ("reason", close_event_reason)],
            &[],
        ),
    ];
    for (info, name, call, construct, getters, methods) in extra {
        let (extra_prototype, extra_constructor) =
            create_native_subclass(global_object, (prototype.as_value(), constructor.as_value()), info, &CONSTRUCTOR_S_INFO, name, 1, call, construct);
        extra_prototype.put_direct(vm, &constructor_key, extra_constructor.as_value(), DONT_ENUM);
        for (getter_name, getter) in getters {
            put_native_accessor(vm, global_object, &extra_prototype, getter_name, *getter, None, 0);
        }
        put_methods(global_object, &extra_prototype, methods);
        put_to_string_tag(vm, &extra_prototype, name);
        install_global(global_object, name, extra_constructor.as_value());
        if name == "ErrorEvent" {
            let error_structure = instance_structure(vm, Some(global_object), extra_prototype.as_value());
            ERROR_EVENT_STRUCTURES.with(|structures| structures.borrow_mut().push((global_object.cell_id(), error_structure)));
        }
        if name == "MessageEvent" {
            let message_structure = instance_structure(vm, Some(global_object), extra_prototype.as_value());
            MESSAGE_EVENT_STRUCTURES.with(|structures| structures.borrow_mut().push((global_object.cell_id(), message_structure)));
        }
    }
    crate::runtime::abort_signal::install_abort(global_object, (target_prototype.as_value(), target_constructor.as_value()));
    crate::runtime::broadcast_channel::install_broadcast_channel(global_object, (target_prototype.as_value(), target_constructor.as_value()));
    crate::runtime::message_channel::install_message_channel(global_object, (target_prototype.as_value(), target_constructor.as_value()));
    crate::runtime::worker::install_worker(global_object, (target_prototype.as_value(), target_constructor.as_value()));
}
