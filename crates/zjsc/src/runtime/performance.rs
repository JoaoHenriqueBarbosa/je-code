//! `performance`, `Performance`, `PerformanceEntry`, `PerformanceMark` e `PerformanceMeasure` do global. O
//! JavaScriptCore não os define: quem os instala é o bun (WebCore). Medido no bun 1.4.2:
//!
//! - `performance` é propriedade de dados comum do global; o valor é uma instância de `Performance` com uma
//!   única chave própria, `now` (função nativa de dados comum, `length` 0, sem `prototype`, que ignora o `this`).
//!   `Performance`, `PerformanceEntry`, `PerformanceMark` e `PerformanceMeasure` são propriedades comuns do
//!   global; os construtores têm `length` 0 (`PerformanceMark` tem 1), só `length`, `name` e `prototype` como
//!   chaves próprias, e `PerformanceMark` e `PerformanceMeasure` herdam de `PerformanceEntry` (construtor e
//!   protótipo).
//! - `Performance`, `PerformanceEntry` e `PerformanceMeasure` não constroem, com ou sem `new`:
//!   `TypeError: Illegal constructor` com `code` `ERR_ILLEGAL_CONSTRUCTOR`. `PerformanceMark` sem `new` lança
//!   ``Use `new PerformanceMark(...)` instead of `PerformanceMark(...)` ``, e `new PerformanceMark()` lança
//!   `Not enough arguments` (`ERR_MISSING_ARGS`). Um `new PerformanceMark(...)` não entra no buffer.
//! - `this` inválido: os métodos lançam `Can only call <Classe>.<método> on instances of <Classe>` com
//!   `ERR_INVALID_THIS`; os getters lançam `The <Classe>.<nome> getter can only be used on instances of <Classe>`
//!   sem `code`. `PerformanceEntry.name` aceita qualquer marca ou medida; `detail` exige a classe certa.
//! - `now()` é monotônico, em milissegundos com fração, contado desde `timeOrigin` (também fracionário, em
//!   milissegundos desde a época). As duas leituras vêm do relógio de `Date.now` (`wtf::date_math`), a fonte
//!   única do porte; o `now` nunca anda para trás, mesmo que o relógio da máquina ande.
//! - `mark(name, options?)`: `name` passa por `ToString`; `options` é `undefined`, `null` ou objeto (qualquer
//!   outra coisa: `TypeError: Type error`); `startTime` passa por `ToNumber` (`NaN`/infinito:
//!   `The provided value is non-finite`; negativo: `Type error`); `detail` é clonado por `structuredClone`
//!   (`undefined` vira `null`). Devolve um `PerformanceMark` novo, e o buffer guarda uma cópia: `getEntries()`
//!   devolve objetos distintos do que `mark` devolveu.
//! - `measure(name, startOrOptions?, end?)`: sem argumentos, `Not enough arguments`. Posicional: `start` e `end`
//!   são nomes de marca (o valor passa por `ToString`, até um número; a marca mais recente de mesmo nome vale;
//!   sem ela, `SyntaxError: No mark named 'x' exists`, com o `end` resolvido antes do `start`); um objeto no
//!   lugar do `start` conta como `undefined` quando não tem `start` nem `end` (o `duration` sozinho é ignorado;
//!   o `detail` vale sempre). Com objeto que tem `start` ou `end`: o terceiro argumento junto e os três valores
//!   `start`, `end` e `duration` juntos dão `Type error`; número em `start`/`end`/`duration` segue a regra do
//!   `startTime` da marca; o resto de `start`/`end` é nome de marca. `end` falta: `start + duration` se os dois
//!   existem, senão agora; `start` falta: `end - duration` se os dois existem, senão 0.
//! - `getEntries()` junta marcas e depois medidas e ordena por `startTime` (estável); `getEntriesByType(type)`
//!   e `getEntriesByName(name, type?)` filtram, sem argumento lançam `The "type" argument must be specified` e
//!   `The "name" argument must be specified` (`ERR_MISSING_ARGS`). `clearMarks(name?)` e `clearMeasures(name?)`
//!   apagam todas, ou as de mesmo nome (`undefined` conta como ausente), e devolvem `undefined`.
//!
//! `Performance` herda de `EventTarget` (construtor e protótipo, medido: `performance instanceof EventTarget`):
//! a cadeia do `performance` é `Performance.prototype` (`constructor`, `timeOrigin`, `timing`, ...,
//! `markResourceTiming`, `@@toStringTag`), `EventTarget.prototype` (`constructor`, `addEventListener`,
//! `removeEventListener`, `dispatchEvent`, `@@toStringTag`) e `Object.prototype`. `performance` é um alvo de
//! eventos registrado (`register_target`): `dispatchEvent(new Event('x'))` chama os ouvintes com `target` e
//! `currentTarget` iguais a ele. O inspect lista as chaves próprias, as do protótipo e as de `EventTarget`.
//! Os globais `PerformanceObserver`, `PerformanceObserverEntryList`, `PerformanceResourceTiming` e
//! `PerformanceServerTiming` estão em `performance_observer.rs`.
//!
//! `timing` (sempre o mesmo `PerformanceTiming`, 21 getters que valem 0, `toJSON` novo a cada chamada, também
//! dentro de `performance.toJSON()` como o próprio objeto), `onresourcetimingbufferfull` (objeto é guardado,
//! o resto vira `null`), `clearResourceTimings`, `setResourceTimingBufferSize` (`ToNumber` do argumento, sem
//! efeito) e `markResourceTiming` (`length` 7, não faz nada nem confere o `this`) medidos no bun 1.4.2.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{derived_structure, put_to_string_tag};
use crate::runtime::event_target::register_target;
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO;
use crate::runtime::intl_support::prop;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_array::construct_array;
use crate::runtime::js_function::put_direct_native_function_without_transition;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_module_record::throw_syntax_error;
use crate::runtime::js_object::{JSFinalObject, JSObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_null, js_number, js_undefined, EncodedJSValue, JSValue};
use crate::runtime::native_class_support::{
    create_native_class, create_native_class_with_length, create_native_subclass, install_global, instance_structure, put_native_accessor, throw_coded_type_error, throw_native_type_error,
};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::object_constructor::construct_empty_object;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::structured_clone::clone_value_for_host;
use crate::runtime::util_inspect::{inspect_named_fields, InspectOptions};
use crate::wtf::date_math::current_time_in_nanoseconds;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo` dos protótipos.
static PERFORMANCE_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Performance", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static ENTRY_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "PerformanceEntry", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static MARK_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "PerformanceMark", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static MEASURE_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "PerformanceMeasure", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static TIMING_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "PerformanceTiming", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `const ClassInfo` dos construtores (`"Function"`), iguais nas quatro classes.
static CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Mark,
    Measure,
}

impl Kind {
    pub(crate) fn entry_type(self) -> &'static str {
        match self {
            Kind::Mark => "mark",
            Kind::Measure => "measure",
        }
    }

    fn class_name(self) -> &'static str {
        match self {
            Kind::Mark => "PerformanceMark",
            Kind::Measure => "PerformanceMeasure",
        }
    }
}

/// O que uma marca ou medida guarda. Uma entrada gravada no buffer (`id` diferente de zero) tem um único
/// objeto de script, o mesmo em `mark`, `getEntries*` e nos observadores, e as propriedades que o script
/// acrescenta nele sobrevivem; as cópias de `Entry` só carregam o `id` para achar esse objeto.
#[derive(Clone)]
pub(crate) struct Entry {
    /// Zero até `record_entry` (um `new PerformanceMark` nunca entra no buffer).
    id: u64,
    pub(crate) kind: Kind,
    pub(crate) name: WtfString,
    pub(crate) start: f64,
    duration: f64,
    detail: JSValue,
}

#[derive(Default)]
struct State {
    /// Instante (ns desde a época) em que o programa começou: o `timeOrigin`. Zero até a instalação.
    origin_ns: i128,
    /// Última leitura do relógio, para o `now` nunca andar para trás.
    last_ns: i128,
    /// O objeto `performance` (a única instância de `Performance`).
    performance: Option<EncodedJSValue>,
    mark_prototype: Option<JSValue>,
    measure_prototype: Option<JSValue>,
    /// O `PerformanceTiming` único de `performance.timing`.
    timing: Option<EncodedJSValue>,
    /// O valor de `onresourcetimingbufferfull` (`None` é `null`).
    on_resource_timing_buffer_full: Option<JSValue>,
    /// Por valor codificado do objeto devolvido ao script.
    objects: HashMap<EncodedJSValue, Entry>,
    /// O objeto único de cada entrada gravada, por `Entry::id`.
    entry_objects: HashMap<u64, JSValue>,
    /// O último `id` entregue por `record_entry`.
    last_entry_id: u64,
    /// O buffer da linha do tempo, na ordem de criação.
    buffer: Vec<Entry>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

impl Entry {
    /// O `detail` da marca ou da medida (o valor que a entrada segura para o coletor).
    pub(crate) fn detail(&self) -> JSValue {
        self.detail
    }
}

/// As raízes do coletor: o objeto `performance`, o `timing`, os protótipos, o tratador do buffer cheio, o objeto único
/// de cada entrada gravada e o `detail` do buffer (a linha do tempo segura tudo isso enquanto existir).
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    STATE.with(|state| {
        let Ok(state) = state.try_borrow() else {
            marker.abort();
            return;
        };
        state.performance.into_iter().chain(state.timing).for_each(|encoded| marker.encoded(encoded));
        marker.values(&[state.mark_prototype, state.measure_prototype, state.on_resource_timing_buffer_full].into_iter().flatten().collect::<Vec<_>>());
        state.entry_objects.values().for_each(|object| marker.value(*object));
        state.buffer.iter().for_each(|entry| marker.value(entry.detail));
    });
}

/// O `Entry` de cada objeto devolvido ao script é campo dele: o `detail` vive com o objeto.
pub(crate) fn visit_side_table(marker: &mut crate::runtime::gc::Marker, phase: crate::runtime::gc::SidePhase) -> bool {
    STATE.with(|state| match state.try_borrow_mut() {
        Ok(mut state) => crate::runtime::gc::side_map(&mut state.objects, marker, phase, |entry, out| out.push(entry.detail)),
        Err(_) => {
            marker.abort();
            false
        }
    })
}

/// Fim do programa (`cell_registry::reset_program_state`): o estado guarda objetos e `detail` do programa.
pub(crate) fn reset_for_program() {
    let taken = STATE.try_with(|state| std::mem::take(&mut *state.borrow_mut()));
    drop(taken);
    crate::runtime::performance_observer::reset_for_program();
}

/// `performance.now()`: milissegundos com fração desde o `timeOrigin`, monotônico. Com o relógio virtual, é o
/// tempo virtual do laço desde o início do programa.
pub(crate) fn now_ms() -> f64 {
    if crate::wtf::date_math::virtual_clock_origin_ns().is_some() {
        let elapsed = crate::runtime::waiter_list_manager::virtual_now();
        return crate::runtime::time_clamper::clamped_now_ms(elapsed).unwrap_or(elapsed);
    }
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let current = current_time_in_nanoseconds();
        if state.origin_ns == 0 {
            state.origin_ns = current;
            state.last_ns = current;
        }
        if current > state.last_ns {
            state.last_ns = current;
        }
        (state.last_ns - state.origin_ns) as f64 / 1_000_000.0
    })
}

/// `performance.timeOrigin`: milissegundos com fração desde a época.
fn time_origin_ms() -> f64 {
    if let Some(origin) = crate::runtime::time_clamper::clamped_time_origin_ms() {
        return origin;
    }
    if let Some(origin) = crate::wtf::date_math::virtual_clock_origin_ns() {
        return origin as f64 / 1_000_000.0;
    }
    now_ms();
    STATE.with(|state| state.borrow().origin_ns as f64 / 1_000_000.0)
}

fn string_value(global_object: &JSGlobalObject, text: &WtfString) -> JSValue {
    JSValue::from_js_string(js_string(global_object.vm(), text))
}

pub(crate) fn literal_value(global_object: &JSGlobalObject, text: &str) -> JSValue {
    string_value(global_object, &WtfString::from_latin1(text.as_bytes()))
}

fn is_performance(this: JSValue) -> bool {
    STATE.with(|state| state.borrow().performance == Some(this.encode()))
}

fn entry_of(this: JSValue) -> Option<Entry> {
    STATE.with(|state| state.borrow().objects.get(&this.encode()).cloned())
}

/// A conferência do `this` de um método de `Performance`.
fn check_performance(global_object: &JSGlobalObject, call: &HostCall, method: &str) -> Result<(), Thrown> {
    if is_performance(call.this_value()) {
        return Ok(());
    }
    Err(throw_coded_type_error(global_object, &format!("Can only call Performance.{method} on instances of Performance"), "ERR_INVALID_THIS"))
}

/// O objeto de script de `entry`: o mesmo a cada chamada para uma entrada gravada no buffer.
pub(crate) fn make_entry_object(global_object: &JSGlobalObject, entry: &Entry) -> JSValue {
    if entry.id != 0 {
        if let Some(existing) = STATE.with(|state| state.borrow().entry_objects.get(&entry.id).copied()) {
            return existing;
        }
    }
    let vm = global_object.vm();
    let prototype = STATE.with(|state| {
        let state = state.borrow();
        match entry.kind {
            Kind::Mark => state.mark_prototype,
            Kind::Measure => state.measure_prototype,
        }
    });
    let structure = instance_structure(vm, Some(global_object), prototype.unwrap_or_else(js_null));
    let object = JSFinalObject::create(vm, &structure).as_value();
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.objects.insert(object.encode(), entry.clone());
        if entry.id != 0 {
            state.entry_objects.insert(entry.id, object);
        }
    });
    object
}

/// O objeto simples de `toJSON`: `name`, `entryType`, `startTime`, `duration` e, se pedido, `detail`.
fn entry_json(global_object: &JSGlobalObject, entry: &Entry, with_detail: bool) -> JSValue {
    let vm = global_object.vm();
    let object = construct_empty_object(global_object);
    object.put_direct(vm, &prop(vm, "name"), string_value(global_object, &entry.name), 0);
    object.put_direct(vm, &prop(vm, "entryType"), literal_value(global_object, entry.kind.entry_type()), 0);
    object.put_direct(vm, &prop(vm, "startTime"), js_number(entry.start), 0);
    object.put_direct(vm, &prop(vm, "duration"), js_number(entry.duration), 0);
    if with_detail {
        object.put_direct(vm, &prop(vm, "detail"), entry.detail, 0);
    }
    object.as_value()
}

fn illegal_constructor_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(throw_coded_type_error(global_object, "Illegal constructor", "ERR_ILLEGAL_CONSTRUCTOR"))
}

fn call_mark_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(throw_coded_type_error(global_object, "Use `new PerformanceMark(...)` instead of `PerformanceMark(...)`", "ERR_ILLEGAL_CONSTRUCTOR"))
}

fn missing_arguments(global_object: &JSGlobalObject) -> Thrown {
    throw_coded_type_error(global_object, "Not enough arguments", "ERR_MISSING_ARGS")
}

/// O valor numérico de `startTime`, `start`, `end` e `duration`: finito e não negativo.
fn checked_time(global_object: &JSGlobalObject, value: JSValue) -> Result<f64, Thrown> {
    let number = pending_or(global_object, value.to_number())?;
    if !number.is_finite() {
        return Err(throw_native_type_error(global_object, "The provided value is non-finite"));
    }
    if number < 0.0 {
        return Err(throw_native_type_error(global_object, "Type error"));
    }
    Ok(number)
}

fn option_value(global_object: &JSGlobalObject, options: &ObjectRef, name: &str) -> Result<JSValue, Thrown> {
    pending_or(global_object, options.get(global_object, &prop(global_object.vm(), name)))
}

fn detail_value(global_object: &JSGlobalObject, call: &HostCall, detail: JSValue) -> Result<JSValue, Thrown> {
    if detail.is_undefined() {
        return Ok(js_null());
    }
    clone_value_for_host(global_object, call, detail)
}

/// A entrada de `new PerformanceMark(name, options)` e de `performance.mark(name, options)`.
fn mark_entry(global_object: &JSGlobalObject, call: &HostCall, name_index: usize) -> Result<Entry, Thrown> {
    if call.argument_count() <= name_index {
        return Err(missing_arguments(global_object));
    }
    let name = pending_or(global_object, call.argument(name_index).to_wtf_string())?;
    let options = call.argument(name_index + 1);
    let mut start = None;
    let mut detail = js_null();
    if !options.is_undefined_or_null() {
        let Some(options) = ObjectRef::from_value(&options) else {
            return Err(throw_native_type_error(global_object, "Type error"));
        };
        let start_value = option_value(global_object, &options, "startTime")?;
        if !start_value.is_undefined() {
            start = Some(checked_time(global_object, start_value)?);
        }
        let detail_option = option_value(global_object, &options, "detail")?;
        detail = detail_value(global_object, call, detail_option)?;
    }
    let start = match start {
        Some(start) => start,
        None => now_ms(),
    };
    Ok(Entry { id: 0, kind: Kind::Mark, name, start, duration: 0.0, detail })
}

fn construct_mark_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let entry = mark_entry(global_object, call, 0)?;
    let structure = derived_structure(global_object, call, instance_structure)?;
    let object = JSFinalObject::create(global_object.vm(), &structure).as_value();
    STATE.with(|state| state.borrow_mut().objects.insert(object.encode(), entry));
    Ok(object)
}

fn performance_mark_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    check_performance(global_object, call, "mark")?;
    let entry = mark_entry(global_object, call, 0)?;
    let entry = record_entry(entry);
    Ok(make_entry_object(global_object, &entry))
}

/// O `startTime` da marca mais recente de nome `name`, ou o `SyntaxError` de `measure`.
fn mark_time(global_object: &JSGlobalObject, name: &WtfString) -> Result<f64, Thrown> {
    let found = STATE.with(|state| state.borrow().buffer.iter().rev().find(|entry| entry.kind == Kind::Mark && entry.name == *name).map(|entry| entry.start));
    match found {
        Some(start) => Ok(start),
        None => {
            let units: Vec<u16> = (0..name.length()).map(|index| name.code_unit_at(index)).collect();
            Err(throw_syntax_error(global_object, &format!("No mark named '{}' exists", String::from_utf16_lossy(&units))))
        }
    }
}

/// Um `start` ou `end` de `measure` em forma de objeto: número é tempo, o resto é nome de marca.
fn options_point(global_object: &JSGlobalObject, value: JSValue) -> Result<f64, Thrown> {
    if value.is_number() {
        return checked_time(global_object, value);
    }
    let name = pending_or(global_object, value.to_wtf_string())?;
    mark_time(global_object, &name)
}

/// Um `start` ou `end` posicional de `measure`: sempre nome de marca.
fn positional_point(global_object: &JSGlobalObject, value: JSValue) -> Result<f64, Thrown> {
    let name = pending_or(global_object, value.to_wtf_string())?;
    mark_time(global_object, &name)
}

fn performance_measure_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    check_performance(global_object, call, "measure")?;
    if call.argument_count() < 1 {
        return Err(missing_arguments(global_object));
    }
    let name = pending_or(global_object, call.argument(0).to_wtf_string())?;
    let second = call.argument(1);
    let third = call.argument(2);
    let mut detail = js_null();
    let (start, end);
    let mut start_value = JSValue::undefined();
    let mut end_value = JSValue::undefined();
    let mut duration_value = JSValue::undefined();
    if let Some(options) = ObjectRef::from_value(&second) {
        start_value = option_value(global_object, &options, "start")?;
        end_value = option_value(global_object, &options, "end")?;
        duration_value = option_value(global_object, &options, "duration")?;
        let detail_option = option_value(global_object, &options, "detail")?;
        detail = detail_value(global_object, call, detail_option)?;
    }
    if !start_value.is_undefined() || !end_value.is_undefined() {
        // Forma com objeto: só vale com `start` ou `end` presente; sem eles o objeto é como `undefined`.
        if !third.is_undefined() || (!start_value.is_undefined() && !end_value.is_undefined() && !duration_value.is_undefined()) {
            return Err(throw_native_type_error(global_object, "Type error"));
        }
        let duration = if duration_value.is_undefined() { None } else { Some(checked_time(global_object, duration_value)?) };
        let end_time = if end_value.is_undefined() { None } else { Some(options_point(global_object, end_value)?) };
        let start_time = if start_value.is_undefined() { None } else { Some(options_point(global_object, start_value)?) };
        end = match (end_time, start_time, duration) {
            (Some(end), _, _) => end,
            (None, Some(start), Some(duration)) => start + duration,
            _ => now_ms(),
        };
        start = match (start_time, duration, end_time) {
            (Some(start), _, _) => start,
            (None, Some(duration), Some(end)) => end - duration,
            _ => 0.0,
        };
    } else {
        let end_time = if third.is_undefined() { None } else { Some(positional_point(global_object, third)?) };
        let start_time = if second.is_undefined_or_null() || ObjectRef::from_value(&second).is_some() { None } else { Some(positional_point(global_object, second)?) };
        end = end_time.unwrap_or_else(now_ms);
        start = start_time.unwrap_or(0.0);
    }
    let entry = record_entry(Entry { id: 0, kind: Kind::Measure, name, start, duration: end - start, detail });
    Ok(make_entry_object(global_object, &entry))
}

/// Guarda `entry` no buffer da linha do tempo, dá a ela o seu `id` e a entrega aos observadores.
fn record_entry(mut entry: Entry) -> Entry {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.last_entry_id += 1;
        entry.id = state.last_entry_id;
        state.buffer.push(entry.clone());
    });
    crate::runtime::performance_observer::enqueue(&entry);
    entry
}

/// As entradas do buffer que passam em `keep`: marcas, depois medidas, ordenadas por `startTime` (estável).
fn collect_entries(keep: impl Fn(&Entry) -> bool) -> Vec<Entry> {
    let mut entries: Vec<Entry> = STATE.with(|state| {
        let state = state.borrow();
        let marks = state.buffer.iter().filter(|entry| entry.kind == Kind::Mark);
        let measures = state.buffer.iter().filter(|entry| entry.kind == Kind::Measure);
        marks.chain(measures).filter(|entry| keep(*entry)).cloned().collect()
    });
    entries.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
    entries
}

pub(crate) fn entries_array(global_object: &JSGlobalObject, entries: &[Entry]) -> JSValue {
    let values: Vec<JSValue> = entries.iter().map(|entry| make_entry_object(global_object, entry)).collect();
    construct_array(global_object.vm(), &global_object.array_structure(), &values).as_value()
}

/// As entradas do buffer do tipo `entry_type` (`observe({ type, buffered: true })`), na ordem de `getEntries`.
pub(crate) fn buffered_entries_of_type(entry_type: &str) -> Vec<Entry> {
    collect_entries(|entry| entry.kind.entry_type() == entry_type)
}

fn get_entries_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    check_performance(global_object, call, "getEntries")?;
    Ok(entries_array(global_object, &collect_entries(|_| true)))
}

fn get_entries_by_type_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    check_performance(global_object, call, "getEntriesByType")?;
    if call.argument_count() < 1 {
        return Err(throw_coded_type_error(global_object, "The \"type\" argument must be specified", "ERR_MISSING_ARGS"));
    }
    let entry_type = pending_or(global_object, call.argument(0).to_wtf_string())?;
    Ok(entries_array(global_object, &collect_entries(|entry| entry_type.equals_latin1(Some(entry.kind.entry_type().as_bytes())))))
}

fn get_entries_by_name_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    check_performance(global_object, call, "getEntriesByName")?;
    if call.argument_count() < 1 {
        return Err(throw_coded_type_error(global_object, "The \"name\" argument must be specified", "ERR_MISSING_ARGS"));
    }
    let name = pending_or(global_object, call.argument(0).to_wtf_string())?;
    let entry_type = if call.argument(1).is_undefined() { None } else { Some(pending_or(global_object, call.argument(1).to_wtf_string())?) };
    let entries = collect_entries(|entry| {
        entry.name == name && entry_type.as_ref().map_or(true, |wanted| wanted.equals_latin1(Some(entry.kind.entry_type().as_bytes())))
    });
    Ok(entries_array(global_object, &entries))
}

/// `clearMarks(name?)` e `clearMeasures(name?)`.
fn clear_entries(global_object: &JSGlobalObject, call: &HostCall, kind: Kind, method: &str) -> HostResult {
    check_performance(global_object, call, method)?;
    let name = if call.argument(0).is_undefined() { None } else { Some(pending_or(global_object, call.argument(0).to_wtf_string())?) };
    STATE.with(|state| {
        state.borrow_mut().buffer.retain(|entry| !(entry.kind == kind && name.as_ref().map_or(true, |wanted| entry.name == *wanted)));
    });
    Ok(js_undefined())
}

fn clear_marks_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    clear_entries(global_object, call, Kind::Mark, "clearMarks")
}

fn clear_measures_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    clear_entries(global_object, call, Kind::Measure, "clearMeasures")
}

fn now_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(js_number(now_ms()))
}

fn time_origin_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if !is_performance(call.this_value()) {
        return Err(throw_native_type_error(global_object, "The Performance.timeOrigin getter can only be used on instances of Performance"));
    }
    Ok(js_number(time_origin_ms()))
}

fn performance_to_json_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    check_performance(global_object, call, "toJSON")?;
    let vm = global_object.vm();
    let object = construct_empty_object(global_object);
    object.put_direct(vm, &prop(vm, "timeOrigin"), js_number(time_origin_ms()), 0);
    object.put_direct(vm, &prop(vm, "timing"), timing_object(), 0);
    Ok(object.as_value())
}

/// O `PerformanceTiming` de `performance.timing`; sempre o mesmo objeto.
fn timing_object() -> JSValue {
    STATE.with(|state| JSValue::decode(state.borrow().timing.expect("performance instalado")))
}

fn timing_accessor_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if !is_performance(call.this_value()) {
        return Err(throw_native_type_error(global_object, "The Performance.timing getter can only be used on instances of Performance"));
    }
    Ok(timing_object())
}

fn on_resource_timing_buffer_full_get_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if !is_performance(call.this_value()) {
        return Err(throw_native_type_error(global_object, "The Performance.onresourcetimingbufferfull getter can only be used on instances of Performance"));
    }
    Ok(STATE.with(|state| state.borrow().on_resource_timing_buffer_full).unwrap_or_else(js_null))
}

/// O valor guardado é o objeto dado; qualquer outra coisa vira `null`.
fn on_resource_timing_buffer_full_set_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if !is_performance(call.this_value()) {
        return Err(throw_native_type_error(global_object, "The Performance.onresourcetimingbufferfull setter can only be used on instances of Performance"));
    }
    let value = call.argument(0);
    STATE.with(|state| state.borrow_mut().on_resource_timing_buffer_full = if ObjectRef::from_value(&value).is_some() { Some(value) } else { None });
    Ok(js_undefined())
}

fn clear_resource_timings_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    check_performance(global_object, call, "clearResourceTimings")?;
    Ok(js_undefined())
}

fn set_resource_timing_buffer_size_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    check_performance(global_object, call, "setResourceTimingBufferSize")?;
    if call.argument_count() < 1 {
        return Err(missing_arguments(global_object));
    }
    pending_or(global_object, call.argument(0).to_number())?;
    Ok(js_undefined())
}

/// `markResourceTiming` não confere o `this`, não lê os argumentos e não guarda nada.
fn mark_resource_timing_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(js_undefined())
}

/// Os 21 getters de `PerformanceTiming.prototype` (sem navegação nenhuma, todos 0) e o `toJSON`.
fn timing_value(global_object: &JSGlobalObject, call: &HostCall, member: &str) -> HostResult {
    if STATE.with(|state| state.borrow().timing == Some(call.this_value().encode())) {
        return Ok(js_number(0.0));
    }
    Err(throw_native_type_error(global_object, &format!("The PerformanceTiming.{member} getter can only be used on instances of PerformanceTiming")))
}

macro_rules! timing_getters {
    ($(($host:ident, $body:ident, $js:literal)),* $(,)?) => {
        $(
            fn $body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
                timing_value(global_object, call, $js)
            }
            host_function!($host, $body);
        )*
        /// Os getters na ordem das chaves de `PerformanceTiming.prototype`.
        fn timing_getter_table() -> Vec<(&'static str, NativeFunction)> {
            vec![$(($js, $host as NativeFunction)),*]
        }
    };
}

timing_getters!(
    (timing_navigation_start, timing_navigation_start_body, "navigationStart"),
    (timing_unload_event_start, timing_unload_event_start_body, "unloadEventStart"),
    (timing_unload_event_end, timing_unload_event_end_body, "unloadEventEnd"),
    (timing_redirect_start, timing_redirect_start_body, "redirectStart"),
    (timing_redirect_end, timing_redirect_end_body, "redirectEnd"),
    (timing_fetch_start, timing_fetch_start_body, "fetchStart"),
    (timing_domain_lookup_start, timing_domain_lookup_start_body, "domainLookupStart"),
    (timing_domain_lookup_end, timing_domain_lookup_end_body, "domainLookupEnd"),
    (timing_connect_start, timing_connect_start_body, "connectStart"),
    (timing_connect_end, timing_connect_end_body, "connectEnd"),
    (timing_secure_connection_start, timing_secure_connection_start_body, "secureConnectionStart"),
    (timing_request_start, timing_request_start_body, "requestStart"),
    (timing_response_start, timing_response_start_body, "responseStart"),
    (timing_response_end, timing_response_end_body, "responseEnd"),
    (timing_dom_loading, timing_dom_loading_body, "domLoading"),
    (timing_dom_interactive, timing_dom_interactive_body, "domInteractive"),
    (timing_dom_content_loaded_event_start, timing_dom_content_loaded_event_start_body, "domContentLoadedEventStart"),
    (timing_dom_content_loaded_event_end, timing_dom_content_loaded_event_end_body, "domContentLoadedEventEnd"),
    (timing_dom_complete, timing_dom_complete_body, "domComplete"),
    (timing_load_event_start, timing_load_event_start_body, "loadEventStart"),
    (timing_load_event_end, timing_load_event_end_body, "loadEventEnd"),
);

/// `PerformanceTiming.prototype.toJSON`: um objeto novo com os 21 campos, todos 0.
fn timing_to_json_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if STATE.with(|state| state.borrow().timing != Some(call.this_value().encode())) {
        return Err(throw_coded_type_error(global_object, "Can only call PerformanceTiming.toJSON on instances of PerformanceTiming", "ERR_INVALID_THIS"));
    }
    let vm = global_object.vm();
    let object = construct_empty_object(global_object);
    for (name, _) in timing_getter_table() {
        object.put_direct(vm, &prop(vm, name), js_number(0.0), 0);
    }
    Ok(object.as_value())
}

/// O getter de `PerformanceEntry` que devolve `project(entry)`.
fn entry_getter(global_object: &JSGlobalObject, call: &HostCall, member: &str, project: impl Fn(&JSGlobalObject, &Entry) -> JSValue) -> HostResult {
    match entry_of(call.this_value()) {
        Some(entry) => Ok(project(global_object, &entry)),
        None => Err(throw_native_type_error(global_object, &format!("The PerformanceEntry.{member} getter can only be used on instances of PerformanceEntry"))),
    }
}

fn entry_name_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    entry_getter(global_object, call, "name", |global_object, entry| string_value(global_object, &entry.name))
}

fn entry_entry_type_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    entry_getter(global_object, call, "entryType", |global_object, entry| literal_value(global_object, entry.kind.entry_type()))
}

fn entry_start_time_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    entry_getter(global_object, call, "startTime", |_, entry| js_number(entry.start))
}

fn entry_duration_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    entry_getter(global_object, call, "duration", |_, entry| js_number(entry.duration))
}

/// O `this` como entrada da classe `kind`, ou o erro do getter (`detail`) ou do método (`toJSON`).
fn kind_entry(global_object: &JSGlobalObject, call: &HostCall, kind: Kind, member: &str, as_method: bool) -> Result<Entry, Thrown> {
    let class_name = kind.class_name();
    match entry_of(call.this_value()) {
        Some(entry) if entry.kind == kind => Ok(entry),
        _ if as_method => Err(throw_coded_type_error(global_object, &format!("Can only call {class_name}.{member} on instances of {class_name}"), "ERR_INVALID_THIS")),
        _ => Err(throw_native_type_error(global_object, &format!("The {class_name}.{member} getter can only be used on instances of {class_name}"))),
    }
}

fn mark_detail_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(kind_entry(global_object, call, Kind::Mark, "detail", false)?.detail)
}

fn measure_detail_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(kind_entry(global_object, call, Kind::Measure, "detail", false)?.detail)
}

fn mark_to_json_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let entry = kind_entry(global_object, call, Kind::Mark, "toJSON", true)?;
    Ok(entry_json(global_object, &entry, true))
}

fn measure_to_json_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let entry = kind_entry(global_object, call, Kind::Measure, "toJSON", true)?;
    Ok(entry_json(global_object, &entry, true))
}

fn entry_to_json_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    match entry_of(call.this_value()) {
        Some(entry) => Ok(entry_json(global_object, &entry, false)),
        None => Err(throw_coded_type_error(global_object, "Can only call PerformanceEntry.toJSON on instances of PerformanceEntry", "ERR_INVALID_THIS")),
    }
}

/// `[Symbol.for('nodejs.util.inspect.custom')](depth, options)` de `PerformanceEntry` (medido no bun 1.4.2): `depth`
/// negativo devolve o `toJSON()` com `detail`; `options.depth` zero ou menos dá `Nome [Object]`; senão
/// `Nome { name, entryType, startTime, duration, detail }` no formato do `util.inspect`, com o nome do construtor.
fn entry_inspect_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(entry) = entry_of(call.this_value()) else {
        return Err(throw_native_type_error(global_object, "this.toJSON is not a function"));
    };
    let depth = call.argument(0);
    if depth.is_number() && depth.as_number() < 0.0 {
        return Ok(entry_json(global_object, &entry, true));
    }
    let constructor = crate::runtime::intl_support::get_property(global_object, call.this_value(), "constructor")?;
    let name_value = crate::runtime::intl_support::get_property(global_object, constructor, "name")?;
    let name = String::from_utf16_lossy(&crate::runtime::literal_parser::wtf_string_to_units(&name_value.to_wtf_string()));
    let Some(options) = InspectOptions::from_options(global_object, call.argument(1))? else {
        return Ok(string_value(global_object, &WtfString::from_utf16(&format!("{name} [Object]").encode_utf16().collect::<Vec<u16>>())));
    };
    let fields = [
        ("name", string_value(global_object, &entry.name)),
        ("entryType", literal_value(global_object, entry.kind.entry_type())),
        ("startTime", js_number(entry.start)),
        ("duration", js_number(entry.duration)),
        ("detail", entry.detail),
    ];
    let text = inspect_named_fields(global_object, &name, &fields, &options)?;
    Ok(string_value(global_object, &WtfString::from_utf16(&text.encode_utf16().collect::<Vec<u16>>())))
}

host_function!(entry_inspect, entry_inspect_body);
host_function!(illegal_constructor, illegal_constructor_body);
host_function!(call_mark, call_mark_body);
host_function!(construct_mark, construct_mark_body);
host_function!(performance_mark, performance_mark_body);
host_function!(performance_measure, performance_measure_body);
host_function!(performance_get_entries, get_entries_body);
host_function!(performance_get_entries_by_type, get_entries_by_type_body);
host_function!(performance_get_entries_by_name, get_entries_by_name_body);
host_function!(performance_clear_marks, clear_marks_body);
host_function!(performance_clear_measures, clear_measures_body);
host_function!(performance_now, now_body);
host_function!(performance_time_origin, time_origin_body);
host_function!(performance_to_json, performance_to_json_body);
host_function!(entry_name, entry_name_body);
host_function!(entry_entry_type, entry_entry_type_body);
host_function!(entry_start_time, entry_start_time_body);
host_function!(entry_duration, entry_duration_body);
host_function!(entry_to_json, entry_to_json_body);
host_function!(mark_detail, mark_detail_body);
host_function!(mark_to_json, mark_to_json_body);
host_function!(measure_detail, measure_detail_body);
host_function!(measure_to_json, measure_to_json_body);
host_function!(performance_timing, timing_accessor_body);
host_function!(performance_on_resource_timing_buffer_full_get, on_resource_timing_buffer_full_get_body);
host_function!(performance_on_resource_timing_buffer_full_set, on_resource_timing_buffer_full_set_body);
host_function!(performance_clear_resource_timings, clear_resource_timings_body);
host_function!(performance_set_resource_timing_buffer_size, set_resource_timing_buffer_size_body);
host_function!(performance_mark_resource_timing, mark_resource_timing_body);
host_function!(timing_to_json, timing_to_json_body);

pub(crate) fn put_method(global_object: &JSGlobalObject, object: &JSObject, name: &str, length: u32, function: NativeFunction) {
    put_method_with_attributes(global_object, object, name, length, function, 0);
}

/// [`put_method`] com os atributos dados (`DONT_ENUM` para o `toJSON` de `PerformanceMark` e `PerformanceMeasure`).
fn put_method_with_attributes(global_object: &JSGlobalObject, object: &JSObject, name: &str, length: u32, function: NativeFunction, attributes: u32) {
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
        attributes,
    );
}

/// Instala `Performance`, `PerformanceEntry`, `PerformanceMark`, `PerformanceMeasure` e `performance` no global.
pub fn install_performance(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let constructor_key = PropertyName::from_identifier(&vm.property_names.constructor);
    let illegal = illegal_constructor as NativeFunction;

    // PerformanceEntry.
    let (entry_prototype, entry_constructor) =
        create_native_class(global_object, &ENTRY_PROTOTYPE_S_INFO, &CONSTRUCTOR_S_INFO, "PerformanceEntry", illegal, illegal);
    entry_prototype.put_direct(vm, &constructor_key, entry_constructor.as_value(), DONT_ENUM);
    let entry_getters: [(&str, NativeFunction); 4] = [
        ("name", entry_name as NativeFunction),
        ("entryType", entry_entry_type as NativeFunction),
        ("startTime", entry_start_time as NativeFunction),
        ("duration", entry_duration as NativeFunction),
    ];
    for (name, getter) in entry_getters {
        put_native_accessor(vm, global_object, &entry_prototype, name, getter, None, 0);
    }
    put_method(global_object, &entry_prototype, "toJSON", 0, entry_to_json as NativeFunction);
    put_to_string_tag(vm, &entry_prototype, "PerformanceEntry");
    crate::runtime::streams::put_inspect_custom_named(global_object, &entry_prototype, "[nodejs.util.inspect.custom]", entry_inspect);

    // PerformanceMark e PerformanceMeasure, filhos de PerformanceEntry.
    let (mark_prototype, mark_constructor) = create_native_class_with_length(
        global_object,
        &MARK_PROTOTYPE_S_INFO,
        &CONSTRUCTOR_S_INFO,
        "PerformanceMark",
        1,
        call_mark as NativeFunction,
        construct_mark as NativeFunction,
    );
    let (measure_prototype, measure_constructor) =
        create_native_class(global_object, &MEASURE_PROTOTYPE_S_INFO, &CONSTRUCTOR_S_INFO, "PerformanceMeasure", illegal, illegal);
    mark_prototype.set_prototype_direct(vm, entry_prototype.as_value());
    measure_prototype.set_prototype_direct(vm, entry_prototype.as_value());
    mark_constructor.set_prototype_direct(vm, entry_constructor.as_value());
    measure_constructor.set_prototype_direct(vm, entry_constructor.as_value());
    mark_prototype.put_direct(vm, &constructor_key, mark_constructor.as_value(), DONT_ENUM);
    measure_prototype.put_direct(vm, &constructor_key, measure_constructor.as_value(), DONT_ENUM);
    put_native_accessor(vm, global_object, &mark_prototype, "detail", mark_detail as NativeFunction, None, 0);
    put_method_with_attributes(global_object, &mark_prototype, "toJSON", 0, mark_to_json as NativeFunction, DONT_ENUM);
    put_to_string_tag(vm, &mark_prototype, "PerformanceMark");
    put_native_accessor(vm, global_object, &measure_prototype, "detail", measure_detail as NativeFunction, None, 0);
    put_method_with_attributes(global_object, &measure_prototype, "toJSON", 0, measure_to_json as NativeFunction, DONT_ENUM);
    put_to_string_tag(vm, &measure_prototype, "PerformanceMeasure");

    // Performance.
    // `Performance` herda de `EventTarget` (construtor e protótipo); `install_event_target` já rodou.
    let global_value = global_object.global_this().map_or_else(js_undefined, |global_this| global_this.as_value());
    let event_target_constructor = crate::runtime::intl_support::get_property(global_object, global_value, "EventTarget").unwrap_or_else(|_| js_undefined());
    let event_target_prototype = crate::runtime::intl_support::get_property(global_object, event_target_constructor, "prototype").unwrap_or_else(|_| js_undefined());
    let (performance_prototype, performance_constructor) = create_native_subclass(
        global_object,
        (event_target_prototype, event_target_constructor),
        &PERFORMANCE_PROTOTYPE_S_INFO,
        &CONSTRUCTOR_S_INFO,
        "Performance",
        0,
        illegal,
        illegal,
    );
    performance_prototype.put_direct(vm, &constructor_key, performance_constructor.as_value(), DONT_ENUM);
    put_native_accessor(vm, global_object, &performance_prototype, "timeOrigin", performance_time_origin as NativeFunction, None, 0);
    put_native_accessor(vm, global_object, &performance_prototype, "timing", performance_timing as NativeFunction, None, 0);
    put_native_accessor(
        vm,
        global_object,
        &performance_prototype,
        "onresourcetimingbufferfull",
        performance_on_resource_timing_buffer_full_get as NativeFunction,
        Some(performance_on_resource_timing_buffer_full_set as NativeFunction),
        0,
    );
    let methods: [(&str, u32, NativeFunction); 11] = [
        ("toJSON", 0, performance_to_json as NativeFunction),
        ("getEntries", 0, performance_get_entries as NativeFunction),
        ("getEntriesByType", 1, performance_get_entries_by_type as NativeFunction),
        ("getEntriesByName", 1, performance_get_entries_by_name as NativeFunction),
        ("clearResourceTimings", 0, performance_clear_resource_timings as NativeFunction),
        ("setResourceTimingBufferSize", 1, performance_set_resource_timing_buffer_size as NativeFunction),
        ("mark", 1, performance_mark as NativeFunction),
        ("clearMarks", 0, performance_clear_marks as NativeFunction),
        ("measure", 1, performance_measure as NativeFunction),
        ("clearMeasures", 0, performance_clear_measures as NativeFunction),
        ("markResourceTiming", 7, performance_mark_resource_timing as NativeFunction),
    ];
    for (name, length, function) in methods {
        put_method(global_object, &performance_prototype, name, length, function);
    }
    put_to_string_tag(vm, &performance_prototype, "Performance");

    // PerformanceTiming: o tipo do `performance.timing`.
    let (timing_prototype, timing_constructor) =
        create_native_class(global_object, &TIMING_PROTOTYPE_S_INFO, &CONSTRUCTOR_S_INFO, "PerformanceTiming", illegal, illegal);
    timing_prototype.put_direct(vm, &constructor_key, timing_constructor.as_value(), DONT_ENUM);
    for (name, getter) in timing_getter_table() {
        put_native_accessor(vm, global_object, &timing_prototype, name, getter, None, 0);
    }
    put_method(global_object, &timing_prototype, "toJSON", 0, timing_to_json as NativeFunction);
    put_to_string_tag(vm, &timing_prototype, "PerformanceTiming");
    let timing = JSFinalObject::create(vm, &instance_structure(vm, Some(global_object), timing_prototype.as_value()));
    // A instância: só `now` como chave própria.
    let structure = instance_structure(vm, Some(global_object), performance_prototype.as_value());
    let performance = JSFinalObject::create(vm, &structure);
    put_method(global_object, &performance, "now", 0, performance_now as NativeFunction);
    register_target(performance.as_value());
    now_ms();
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.performance = Some(performance.as_value().encode());
        state.timing = Some(timing.as_value().encode());
        state.mark_prototype = Some(mark_prototype.as_value());
        state.measure_prototype = Some(measure_prototype.as_value());
    });

    install_global(global_object, "performance", performance.as_value());
    install_global(global_object, "Performance", performance_constructor.as_value());
    install_global(global_object, "PerformanceEntry", entry_constructor.as_value());
    install_global(global_object, "PerformanceMark", mark_constructor.as_value());
    install_global(global_object, "PerformanceMeasure", measure_constructor.as_value());
    install_global(global_object, "PerformanceTiming", timing_constructor.as_value());
    crate::runtime::performance_observer::install_performance_observer(global_object, &entry_prototype, &entry_constructor);
}
