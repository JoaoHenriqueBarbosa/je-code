//! `PerformanceObserver`, `PerformanceObserverEntryList`, `PerformanceResourceTiming` e `PerformanceServerTiming`
//! do global, medidos no bun 1.4.2 (o JavaScriptCore não os define; vêm do WebCore do bun).
//!
//! - Os quatro são propriedades comuns do global, na posição de `Reflect.ownKeys(globalThis)` do bun (a tabela
//!   `ORDER` de `js_global_object_init.rs`). Os protótipos herdam de `Object.prototype`, menos o de
//!   `PerformanceResourceTiming`, que herda de `PerformanceEntry` (o construtor também).
//! - `PerformanceObserverEntryList`, `PerformanceResourceTiming` e `PerformanceServerTiming` não constroem, com
//!   ou sem `new`: `TypeError: Illegal constructor` (`ERR_ILLEGAL_CONSTRUCTOR`). Como nenhuma instância existe
//!   no porte (o bun só as cria ao entregar registros ao callback e ao medir um `fetch`), todo getter e método
//!   deles lança o erro de `this` inválido: os getters `The <Classe>.<nome> getter can only be used on instances
//!   of <Classe>` (sem `code`), os métodos `Can only call <Classe>.<método> on instances of <Classe>`
//!   (`ERR_INVALID_THIS`).
//! - `new PerformanceObserver(callback)` (`length` 1): sem argumento `Not enough arguments` (`ERR_MISSING_ARGS`);
//!   argumento que não é função `Argument 1 ('callback') to the PerformanceObserver constructor must be a
//!   function` (`ERR_INVALID_ARG_TYPE`); sem `new` ``Use `new PerformanceObserver(...)` instead of
//!   `PerformanceObserver(...)` `` (`ERR_ILLEGAL_CONSTRUCTOR`). A instância não tem chave própria.
//! - `observe(options)`: `undefined`/`null`/objeto sem chaves: `no type or entryTypes were provided`; outro
//!   primitivo: `Type error`. Lê `buffered`, `entryTypes` (se definido: não objeto, `null` inclusive, é `Value is
//!   not a sequence` com `ERR_INVALID_ARG_TYPE`; objeto que não é array, `Type error`) e `type`, nessa ordem, e
//!   nunca lê `durationThreshold`. Os dois juntos: `either entryTypes or type must be provided`. Trocar de
//!   `entryTypes` para `type` (ou o contrário) sem `disconnect()` lança o `DOMException` `InvalidModificationError`
//!   (código 13) `observer type can't be changed once registered`. Tipos que não são `mark`, `measure` ou
//!   `resource` são ignorados em silêncio. `entryTypes` substitui a lista, `type` acrescenta.
//! - `takeRecords()` devolve (e esvazia) as marcas e medidas criadas depois do `observe` cujo tipo está na lista.
//! - `PerformanceObserver.supportedEntryTypes`: propriedade de valor customizado (não gravável, enumerável,
//!   configurável) que devolve, a cada leitura, um array novo e congelado com `["mark", "measure", "resource"]`
//!   (`a === b` entre duas leituras é `false`).
//! - `entryTypes` aceita qualquer iterável (array, `Set`, `Map`, gerador, objeto com `Symbol.iterator`), cada
//!   elemento convertido em string; objeto sem `Symbol.iterator` chamável é `Type error`.
//! - `observe({ type, buffered: true })` (só a forma `type`; com `entryTypes` o `buffered` não faz nada) junta as
//!   entradas do buffer da linha do tempo, depois as que já esperavam na fila, e, se houver alguma, chama o
//!   callback NA HORA (síncrono dentro de `observe`), com a fila esvaziada.
//! - Entrega: o que `mark`/`measure` enfileiram é entregue por uma tarefa do host (`timers.rs`): depois das
//!   microtasks do script principal e antes de `setTimeout(0)`/`setImmediate`; dentro do laço, depois dos timers
//!   e immediates da volta. Observadores na ordem de criação; o callback roda com `this` igual ao observador e
//!   recebe `(entryList, observer)`, com as entradas ordenadas por `startTime` (estável). Exceção do callback vai para o relato de erros não capturados.
//!   `takeRecords()` e `disconnect()` esvaziam a fila e portanto cancelam a entrega.
//! - `PerformanceObserverEntryList` (só o observador a cria): `getEntries()`, `getEntriesByType(type)` e
//!   `getEntriesByName(name, type?)` devolvem um array novo (não congelado) a cada chamada; sem argumento é
//!   `Not enough arguments` (`ERR_MISSING_ARGS`); `type` ausente ou `undefined` não filtra.
//!
//! PENDENTE (no bun e ausente aqui): nada desta fatia; a identidade `entry === performance.getEntries()[0]` ainda
//! segue o que `performance.rs` faz (um objeto novo por chamada).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use crate::host_function;
use crate::runtime::call_data::get_call_data;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{derived_structure, put_to_string_tag};
use crate::runtime::custom_getter_setter::CustomGetterSetter;
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::internal_function::{InternalFunctionRef, INTERNAL_FUNCTION_S_INFO};
use crate::runtime::intl_support::prop;
use crate::runtime::iterator_operations::{for_each_in_iterable_with_method, get_value_property};
use crate::runtime::js_array::construct_array;
use crate::runtime::js_dom_exception::throw_dom_exception;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_microtask::call_microtask;
use crate::runtime::js_object::{JSFinalObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_promise_host::Thrown as CallThrown;
use crate::runtime::js_value::{js_null, js_undefined, EncodedJSValue, JSValue};
use crate::runtime::native_class_support::{
    create_native_class, create_native_class_with_length, install_global, instance_structure, put_native_accessor, throw_coded_type_error,
    throw_native_type_error,
};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::object_constructor::object_constructor_freeze;
use crate::runtime::performance::{buffered_entries_of_type, entries_array, literal_value, make_entry_object, put_method, Entry};
use crate::runtime::property_attribute::{CUSTOM_VALUE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;

const SUPPORTED: [&str; 3] = ["mark", "measure", "resource"];

static OBSERVER_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "PerformanceObserver", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static ENTRY_LIST_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "PerformanceObserverEntryList", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static RESOURCE_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "PerformanceResourceTiming", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static SERVER_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "PerformanceServerTiming", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    None,
    EntryTypes,
    Type,
}

struct Observer {
    mode: Mode,
    types: Vec<&'static str>,
    queue: Vec<Entry>,
    callback: JSValue,
    /// Ordem de registro (o primeiro `observe` que escolhe um tipo suportado): a entrega percorre os observadores nela.
    serial: u64,
}

thread_local! {
    static OBSERVERS: RefCell<HashMap<EncodedJSValue, Observer>> = RefCell::new(HashMap::new());
    /// As entradas de cada `PerformanceObserverEntryList` criada para um callback.
    static LISTS: RefCell<HashMap<EncodedJSValue, Vec<Entry>>> = RefCell::new(HashMap::new());
    static LIST_PROTOTYPE: Cell<Option<JSValue>> = const { Cell::new(None) };
    static NEXT_SERIAL: Cell<u64> = const { Cell::new(0) };
}

/// Fim do programa: os observadores guardam objetos do programa.
/// As raízes do coletor: o protótipo e cada observador que observa (a lista de observadores registrados do global o
/// segura, com o callback e a fila).
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    marker.value(LIST_PROTOTYPE.with(Cell::get));
    OBSERVERS.with(|observers| {
        for (object, observer) in observers.borrow().iter().filter(|(_, observer)| observer.mode != Mode::None) {
            marker.encoded(*object);
            marker.value(observer.callback);
            observer.queue.iter().for_each(|entry| marker.value(entry.detail()));
        }
    });
}

/// O observador desconectado e cada lista entregue vivem enquanto o objeto vive.
pub(crate) fn visit_side_table(marker: &mut crate::runtime::gc::Marker, phase: crate::runtime::gc::SidePhase) -> bool {
    let observers = crate::runtime::gc::side_table(&OBSERVERS, marker, phase, |observer, out| {
        out.push(observer.callback);
        out.extend(observer.queue.iter().map(Entry::detail));
    });
    let lists = crate::runtime::gc::side_table(&LISTS, marker, phase, |entries, out| out.extend(entries.iter().map(Entry::detail)));
    observers | lists
}

pub(crate) fn reset_for_program() {
    let taken = OBSERVERS.try_with(|observers| std::mem::take(&mut *observers.borrow_mut()));
    drop(taken);
    let lists = LISTS.try_with(|lists| std::mem::take(&mut *lists.borrow_mut()));
    drop(lists);
    let _ = LIST_PROTOTYPE.try_with(|prototype| prototype.set(None));
    let _ = NEXT_SERIAL.try_with(|serial| serial.set(0));
}

/// Chama o callback de `observer` com a lista de `entries`; a exceção vai para o relato de erros não capturados.
fn call_observer_callback(global_object: &JSGlobalObject, observer: JSValue, callback: JSValue, entries: &[Entry]) {
    let vm = global_object.vm();
    let prototype = LIST_PROTOTYPE.with(Cell::get).unwrap_or_else(js_null);
    let list = JSFinalObject::create(vm, &instance_structure(vm, Some(global_object), prototype)).as_value();
    // A lista do callback vem ordenada por `startTime` (estável): uma medida sem `start` (0) precede as marcas.
    let mut ordered = entries.to_vec();
    ordered.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
    LISTS.with(|lists| lists.borrow_mut().insert(list.encode(), ordered));
    match call_microtask(global_object, callback, observer, &[list, observer], "callback is not a function") {
        Err(CallThrown::Value(error)) => vm.report_unhandled_error(global_object, error),
        Err(CallThrown::Termination) | Ok(_) => {}
    }
}

/// A tarefa do host que entrega as filas pendentes (chamada por `timers.rs`): cada observador com entradas, na
/// ordem de registro, recebe o callback e as microtasks esvaziam depois do último.
pub(crate) fn deliver_pending(global_object: &JSGlobalObject) {
    let mut pending: Vec<(u64, EncodedJSValue)> = OBSERVERS.with(|observers| {
        observers.borrow().iter().filter(|(_, observer)| !observer.queue.is_empty()).map(|(key, observer)| (observer.serial, *key)).collect()
    });
    pending.sort();
    for (_, key) in pending {
        let taken = OBSERVERS.with(|observers| {
            observers.borrow_mut().get_mut(&key).filter(|observer| !observer.queue.is_empty()).map(|observer| (observer.callback, std::mem::take(&mut observer.queue)))
        });
        if let Some((callback, entries)) = taken {
            call_observer_callback(global_object, JSValue::decode(key), callback, &entries);
        }
    }
    global_object.vm().drain_microtasks();
}

/// Entrega `entry` à fila de cada observador registrado para o tipo dela (`takeRecords`).
pub(crate) fn enqueue(entry: &Entry) {
    let entry_type = entry.kind.entry_type();
    OBSERVERS.with(|observers| {
        for observer in observers.borrow_mut().values_mut() {
            if observer.types.contains(&entry_type) {
                observer.queue.push(entry.clone());
            }
        }
    });
}

fn illegal_constructor_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(throw_coded_type_error(global_object, "Illegal constructor", "ERR_ILLEGAL_CONSTRUCTOR"))
}

fn call_observer_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(throw_coded_type_error(global_object, "Use `new PerformanceObserver(...)` instead of `PerformanceObserver(...)`", "ERR_ILLEGAL_CONSTRUCTOR"))
}

fn construct_observer_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if call.argument_count() < 1 {
        return Err(throw_coded_type_error(global_object, "Not enough arguments", "ERR_MISSING_ARGS"));
    }
    if !call.argument(0).is_callable() {
        return Err(throw_coded_type_error(
            global_object,
            "Argument 1 ('callback') to the PerformanceObserver constructor must be a function",
            "ERR_INVALID_ARG_TYPE",
        ));
    }
    let structure = derived_structure(global_object, call, instance_structure)?;
    let object = JSFinalObject::create(global_object.vm(), &structure).as_value();
    OBSERVERS.with(|observers| {
        observers.borrow_mut().insert(object.encode(), Observer { mode: Mode::None, types: Vec::new(), queue: Vec::new(), callback: call.argument(0), serial: 0 })
    });
    Ok(object)
}

fn check_observer(global_object: &JSGlobalObject, call: &HostCall, method: &str) -> Result<(), Thrown> {
    if OBSERVERS.with(|observers| observers.borrow().contains_key(&call.this_value().encode())) {
        return Ok(());
    }
    Err(throw_coded_type_error(global_object, &format!("Can only call PerformanceObserver.{method} on instances of PerformanceObserver"), "ERR_INVALID_THIS"))
}

/// Os tipos suportados que aparecem em `names`, sem repetição, na ordem em que aparecem.
fn supported_in(names: &[crate::wtf::text::wtf_string::String]) -> Vec<&'static str> {
    let mut found: Vec<&'static str> = Vec::new();
    for name in names {
        if let Some(supported) = SUPPORTED.iter().find(|supported| name.equals_latin1(Some(supported.as_bytes()))) {
            if !found.contains(supported) {
                found.push(supported);
            }
        }
    }
    found
}

/// O `entryTypes` de `observe`: uma sequência de strings (qualquer iterável; objeto sem `@@iterator` chamável é
/// `Type error`).
fn entry_types_sequence(global_object: &JSGlobalObject, value: JSValue) -> Result<Vec<crate::wtf::text::wtf_string::String>, Thrown> {
    if !value.is_object() {
        return Err(throw_coded_type_error(global_object, "Value is not a sequence", "ERR_INVALID_ARG_TYPE"));
    }
    let vm = global_object.vm();
    let iterator_method = get_value_property(global_object, value, &PropertyName::from_identifier(&vm.property_names.iterator_symbol))?;
    if get_call_data(iterator_method).is_none() {
        return Err(throw_native_type_error(global_object, "Type error"));
    }
    let mut names = Vec::new();
    for_each_in_iterable_with_method(global_object, value, iterator_method, |element| {
        names.push(pending_or(global_object, element.to_wtf_string())?);
        Ok(())
    })?;
    Ok(names)
}

fn observe_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    check_observer(global_object, call, "observe")?;
    let vm = global_object.vm();
    let options = call.argument(0);
    let mut entry_types = None;
    let mut single_type = None;
    let mut buffered = false;
    if !options.is_undefined_or_null() {
        let Some(options) = ObjectRef::from_value(&options).filter(|_| options.is_object()) else {
            return Err(throw_native_type_error(global_object, "Type error"));
        };
        buffered = pending_or(global_object, options.get(global_object, &prop(vm, "buffered")))?.to_boolean();
        let entry_types_value = pending_or(global_object, options.get(global_object, &prop(vm, "entryTypes")))?;
        if !entry_types_value.is_undefined() {
            entry_types = Some(entry_types_sequence(global_object, entry_types_value)?);
        }
        let type_value = pending_or(global_object, options.get(global_object, &prop(vm, "type")))?;
        if !type_value.is_undefined() {
            single_type = Some(pending_or(global_object, type_value.to_wtf_string())?);
        }
    }
    let message = match (&entry_types, &single_type) {
        (Some(_), Some(_)) => "either entryTypes or type must be provided",
        (None, None) => "no type or entryTypes were provided",
        _ => "",
    };
    if !message.is_empty() {
        return Err(throw_native_type_error(global_object, message));
    }
    let wanted = if entry_types.is_some() { Mode::EntryTypes } else { Mode::Type };
    let this = call.this_value().encode();
    let changed = OBSERVERS.with(|observers| {
        let mut observers = observers.borrow_mut();
        let observer = observers.get_mut(&this).expect("conferido por check_observer");
        if observer.mode != Mode::None && observer.mode != wanted {
            return false;
        }
        match (entry_types.as_deref(), single_type.as_ref()) {
            (Some(names), _) => observer.types = supported_in(names),
            (None, Some(name)) => {
                for supported in supported_in(std::slice::from_ref(name)) {
                    if !observer.types.contains(&supported) {
                        observer.types.push(supported);
                    }
                }
            }
            (None, None) => {}
        }
        // Só um tipo suportado registra o observador (e fixa o modo): `entryTypes: []` ou `type: 'foo'` não registram.
        if observer.types.is_empty() {
            observer.mode = Mode::None;
        } else if observer.mode == Mode::None {
            observer.mode = wanted;
            observer.serial = NEXT_SERIAL.with(|next| {
                next.set(next.get() + 1);
                next.get()
            });
        }
        true
    });
    if !changed {
        return Err(throw_dom_exception(global_object, "InvalidModificationError", "observer type can't be changed once registered"));
    }
    if buffered && entry_types.is_none() {
        deliver_buffered(global_object, call.this_value(), single_type.as_ref());
    }
    Ok(js_undefined())
}

/// `buffered: true`: o buffer do tipo pedido, depois a fila que já esperava, entregues NA HORA ao callback.
fn deliver_buffered(global_object: &JSGlobalObject, observer: JSValue, single_type: Option<&crate::wtf::text::wtf_string::String>) {
    let Some(name) = single_type.and_then(|name| supported_in(std::slice::from_ref(name)).into_iter().next()) else {
        return;
    };
    let mut entries = buffered_entries_of_type(name);
    let taken = OBSERVERS.with(|observers| {
        observers.borrow_mut().get_mut(&observer.encode()).map(|state| (state.callback, std::mem::take(&mut state.queue)))
    });
    let Some((callback, mut queued)) = taken else {
        return;
    };
    entries.append(&mut queued);
    if !entries.is_empty() {
        call_observer_callback(global_object, observer, callback, &entries);
    }
}

fn disconnect_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    check_observer(global_object, call, "disconnect")?;
    OBSERVERS.with(|observers| {
        if let Some(observer) = observers.borrow_mut().get_mut(&call.this_value().encode()) {
            observer.mode = Mode::None;
            observer.types.clear();
            observer.queue.clear();
        }
    });
    Ok(js_undefined())
}

fn take_records_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    check_observer(global_object, call, "takeRecords")?;
    let queue = OBSERVERS.with(|observers| observers.borrow_mut().get_mut(&call.this_value().encode()).map(|observer| std::mem::take(&mut observer.queue)));
    let values: Vec<JSValue> = queue.unwrap_or_default().iter().map(|entry| make_entry_object(global_object, entry)).collect();
    Ok(construct_array(global_object.vm(), &global_object.array_structure(), &values).as_value())
}

host_function!(illegal_constructor, illegal_constructor_body);
host_function!(call_observer, call_observer_body);
host_function!(construct_observer, construct_observer_body);
host_function!(observer_observe, observe_body);
host_function!(observer_disconnect, disconnect_body);
host_function!(observer_take_records, take_records_body);

/// As entradas da lista `this` de `getEntries*`, ou o erro de `this` inválido.
fn list_entries(global_object: &JSGlobalObject, call: &HostCall, method: &str) -> Result<Vec<Entry>, Thrown> {
    if let Some(entries) = LISTS.with(|lists| lists.borrow().get(&call.this_value().encode()).cloned()) {
        return Ok(entries);
    }
    Err(throw_coded_type_error(
        global_object,
        &format!("Can only call PerformanceObserverEntryList.{method} on instances of PerformanceObserverEntryList"),
        "ERR_INVALID_THIS",
    ))
}

fn entry_list_get_entries_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(entries_array(global_object, &list_entries(global_object, call, "getEntries")?))
}

fn entry_list_get_entries_by_type_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let entries = list_entries(global_object, call, "getEntriesByType")?;
    if call.argument_count() < 1 {
        return Err(throw_coded_type_error(global_object, "Not enough arguments", "ERR_MISSING_ARGS"));
    }
    let entry_type = pending_or(global_object, call.argument(0).to_wtf_string())?;
    let kept: Vec<Entry> = entries.into_iter().filter(|entry| entry_type.equals_latin1(Some(entry.kind.entry_type().as_bytes()))).collect();
    Ok(entries_array(global_object, &kept))
}

fn entry_list_get_entries_by_name_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let entries = list_entries(global_object, call, "getEntriesByName")?;
    if call.argument_count() < 1 {
        return Err(throw_coded_type_error(global_object, "Not enough arguments", "ERR_MISSING_ARGS"));
    }
    let name = pending_or(global_object, call.argument(0).to_wtf_string())?;
    let entry_type = if call.argument(1).is_undefined() { None } else { Some(pending_or(global_object, call.argument(1).to_wtf_string())?) };
    let kept: Vec<Entry> = entries
        .into_iter()
        .filter(|entry| entry.name == name && entry_type.as_ref().map_or(true, |wanted| wanted.equals_latin1(Some(entry.kind.entry_type().as_bytes()))))
        .collect();
    Ok(entries_array(global_object, &kept))
}

host_function!(entry_list_get_entries, entry_list_get_entries_body);
host_function!(entry_list_get_entries_by_type, entry_list_get_entries_by_type_body);
host_function!(entry_list_get_entries_by_name, entry_list_get_entries_by_name_body);

/// Os métodos de `PerformanceObserverEntryList.prototype`, na ordem das chaves, com o `length`.
fn entry_list_methods() -> Vec<(&'static str, u32, NativeFunction)> {
    vec![
        ("getEntries", 0, entry_list_get_entries as NativeFunction),
        ("getEntriesByType", 1, entry_list_get_entries_by_type as NativeFunction),
        ("getEntriesByName", 1, entry_list_get_entries_by_name as NativeFunction),
    ]
}

/// `supportedEntryTypes`: um array novo e congelado a cada leitura.
fn supported_entry_types_getter(global_object: &JSGlobalObject, _this: EncodedJSValue, _name: &PropertyName) -> EncodedJSValue {
    let values: Vec<JSValue> = SUPPORTED.iter().map(|name| literal_value(global_object, name)).collect();
    let array = construct_array(global_object.vm(), &global_object.array_structure(), &values);
    object_constructor_freeze(global_object, &array).expect("congelar um array de três strings");
    array.as_value().encode()
}

/// Getter e método de uma classe sem instância no porte: sempre o erro de `this` inválido.
macro_rules! rejecting_members {
    ($getters:ident, $methods:ident, $class:literal,
     getters: [$(($ghost:ident, $gbody:ident, $gjs:literal)),* $(,)?],
     methods: [$(($mhost:ident, $mbody:ident, $mjs:literal, $mlen:literal)),* $(,)?]) => {
        $(
            fn $gbody(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
                Err(throw_native_type_error(global_object, concat!("The ", $class, ".", $gjs, " getter can only be used on instances of ", $class)))
            }
            host_function!($ghost, $gbody);
        )*
        $(
            fn $mbody(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
                Err(throw_coded_type_error(global_object, concat!("Can only call ", $class, ".", $mjs, " on instances of ", $class), "ERR_INVALID_THIS"))
            }
            host_function!($mhost, $mbody);
        )*
        /// Os getters na ordem das chaves do protótipo.
        fn $getters() -> Vec<(&'static str, NativeFunction)> {
            vec![$(($gjs, $ghost as NativeFunction)),*]
        }
        /// Os métodos na ordem das chaves do protótipo, com o `length`.
        fn $methods() -> Vec<(&'static str, u32, NativeFunction)> {
            vec![$(($mjs, $mlen, $mhost as NativeFunction)),*]
        }
    };
}

rejecting_members!(resource_getters, resource_methods, "PerformanceResourceTiming",
    getters: [
        (resource_initiator_type, resource_initiator_type_body, "initiatorType"),
        (resource_next_hop_protocol, resource_next_hop_protocol_body, "nextHopProtocol"),
        (resource_worker_start, resource_worker_start_body, "workerStart"),
        (resource_redirect_start, resource_redirect_start_body, "redirectStart"),
        (resource_redirect_end, resource_redirect_end_body, "redirectEnd"),
        (resource_fetch_start, resource_fetch_start_body, "fetchStart"),
        (resource_domain_lookup_start, resource_domain_lookup_start_body, "domainLookupStart"),
        (resource_domain_lookup_end, resource_domain_lookup_end_body, "domainLookupEnd"),
        (resource_connect_start, resource_connect_start_body, "connectStart"),
        (resource_connect_end, resource_connect_end_body, "connectEnd"),
        (resource_secure_connection_start, resource_secure_connection_start_body, "secureConnectionStart"),
        (resource_request_start, resource_request_start_body, "requestStart"),
        (resource_response_start, resource_response_start_body, "responseStart"),
        (resource_response_end, resource_response_end_body, "responseEnd"),
        (resource_transfer_size, resource_transfer_size_body, "transferSize"),
        (resource_encoded_body_size, resource_encoded_body_size_body, "encodedBodySize"),
        (resource_decoded_body_size, resource_decoded_body_size_body, "decodedBodySize"),
        (resource_server_timing, resource_server_timing_body, "serverTiming"),
    ],
    methods: [(resource_to_json, resource_to_json_body, "toJSON", 0)]);

rejecting_members!(server_getters, server_methods, "PerformanceServerTiming",
    getters: [
        (server_name, server_name_body, "name"),
        (server_duration, server_duration_body, "duration"),
        (server_description, server_description_body, "description"),
    ],
    methods: [(server_to_json, server_to_json_body, "toJSON", 0)]);

/// Prototype (herdando de `parent`, se houver) e construtor ilegal de uma das três classes sem instância.
fn rejecting_class(
    global_object: &JSGlobalObject,
    info: &'static ClassInfo,
    name: &str,
    getters: Vec<(&'static str, NativeFunction)>,
    methods: Vec<(&'static str, u32, NativeFunction)>,
    parent: Option<(&JSObjectRef, &InternalFunctionRef)>,
) -> (JSObjectRef, InternalFunctionRef) {
    let vm = global_object.vm();
    let illegal = illegal_constructor as NativeFunction;
    let (prototype, constructor) = create_native_class(global_object, info, &CONSTRUCTOR_S_INFO, name, illegal, illegal);
    if let Some((parent_prototype, parent_constructor)) = parent {
        prototype.set_prototype_direct(vm, parent_prototype.as_value());
        constructor.set_prototype_direct(vm, parent_constructor.as_value());
    }
    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
    for (getter_name, getter) in getters {
        put_native_accessor(vm, global_object, &prototype, getter_name, getter, None, 0);
    }
    for (method_name, length, function) in methods {
        put_method(global_object, &prototype, method_name, length, function);
    }
    put_to_string_tag(vm, &prototype, name);
    (prototype, constructor)
}

/// Instala os quatro globais; `entry_prototype` e `entry_constructor` são os de `PerformanceEntry`.
pub fn install_performance_observer(global_object: &JSGlobalObject, entry_prototype: &JSObjectRef, entry_constructor: &InternalFunctionRef) {
    let vm = global_object.vm();

    let (observer_prototype, observer_constructor) = create_native_class_with_length(
        global_object,
        &OBSERVER_PROTOTYPE_S_INFO,
        &CONSTRUCTOR_S_INFO,
        "PerformanceObserver",
        1,
        call_observer as NativeFunction,
        construct_observer as NativeFunction,
    );
    observer_prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), observer_constructor.as_value(), DONT_ENUM);
    put_method(global_object, &observer_prototype, "observe", 0, observer_observe as NativeFunction);
    put_method(global_object, &observer_prototype, "disconnect", 0, observer_disconnect as NativeFunction);
    put_method(global_object, &observer_prototype, "takeRecords", 0, observer_take_records as NativeFunction);
    put_to_string_tag(vm, &observer_prototype, "PerformanceObserver");
    let name = prop(vm, "supportedEntryTypes");
    observer_constructor.put_direct_custom_getter_setter_without_transition(
        vm,
        &name,
        &CustomGetterSetter::create(vm, supported_entry_types_getter, None),
        CUSTOM_VALUE | READ_ONLY,
    );

    let (entry_list_prototype, entry_list) =
        rejecting_class(global_object, &ENTRY_LIST_PROTOTYPE_S_INFO, "PerformanceObserverEntryList", Vec::new(), entry_list_methods(), None);
    LIST_PROTOTYPE.with(|prototype| prototype.set(Some(entry_list_prototype.as_value())));
    let (_, resource) = rejecting_class(
        global_object,
        &RESOURCE_PROTOTYPE_S_INFO,
        "PerformanceResourceTiming",
        resource_getters(),
        resource_methods(),
        Some((entry_prototype, entry_constructor)),
    );
    let (_, server) = rejecting_class(global_object, &SERVER_PROTOTYPE_S_INFO, "PerformanceServerTiming", server_getters(), server_methods(), None);

    install_global(global_object, "PerformanceObserver", observer_constructor.as_value());
    install_global(global_object, "PerformanceObserverEntryList", entry_list.as_value());
    install_global(global_object, "PerformanceResourceTiming", resource.as_value());
    install_global(global_object, "PerformanceServerTiming", server.as_value());
}
