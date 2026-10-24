//! `setTimeout`, `setInterval`, `setImmediate` e os três `clear*` do global. O JavaScriptCore não os define:
//! quem os instala é o bun (WebCore/Zig), com propriedades de dados comuns (`writable`, `enumerable`,
//! `configurable`), `length` 1, e devolvendo objetos `Timeout` (setTimeout, setInterval) e `Immediate`
//! (setImmediate), cujos protótipos têm a forma medida no bun 1.4.2:
//!
//! - `Timeout.prototype`: acessores `_destroyed` (só get), `_idleStart`, `_idleTimeout`, `_onTimeout` e
//!   `_repeat` (get e set), os métodos `close`, `hasRef`, `ref`, `refresh`, `unref`, o `constructor` e, por
//!   símbolo, `Symbol.dispose`, `Symbol.toPrimitive` (devolve o id numérico) e `Symbol.toStringTag`;
//! - `Immediate.prototype`: `_destroyed`, `hasRef`, `ref`, `unref`, `constructor` e os mesmos três símbolos.
//!
//! O id é um contador único para timeouts, intervals e immediates. O objeto não tem propriedades próprias.
//!
//! ORDEM DO LAÇO (o que o bun faz, em tempo virtual): esvaziam-se as microtasks; a cada volta rodam os timers
//! vencidos (por `(instante de disparo, sequência)`) e depois os immediates enfileirados até aquele ponto, com as
//! microtasks esvaziadas depois de CADA callback. Um immediate agendado dentro de um immediate roda na volta
//! seguinte. O relógio é o virtual do `waiter_list_manager` (milissegundos, só anda quando o laço o avança): sem
//! nada vencido nem immediate pendente, ele salta ao menor prazo (de timer ou de `Atomics.waitAsync`). Só
//! timers e immediates com `ref` mantêm o laço vivo; os com `unref` rodam enquanto houver algo vivo.
//!
//! O atraso segue o `normalizeTimeout` do bun: `ToNumber`; fora de `[1, 2^31-1]` (inclusive `NaN`) vira 1; o
//! resto é truncado. `setImmediate` ignora o segundo argumento.
//!
//! DIVERGÊNCIAS:
//! - Um `_onTimeout` trocado por valor que não é função falha com `callback is not a function`; o bun diz
//!   `5 is not a function`.
//! - `_idleStart` é o instante virtual do armamento, não o relógio do laço do libuv.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::host_function;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::host_call::{pending_or, HostCall, HostResult};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intl_support::prop;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{call_host_function_as_constructor, put_direct_native_function_without_transition, JSFunction};
use crate::runtime::js_getter_setter::GetterSetter;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_microtask::call_microtask;
use crate::runtime::js_object::{JSObject, JSObjectRef};
use crate::runtime::js_promise_host::Thrown as CallThrown;
use crate::runtime::js_value::JSValue;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::node_error::{throw_coded_type_error, throw_plain_error};
use crate::runtime::object_constructor::construct_empty_object;
use crate::runtime::property_attribute::{ACCESSOR, DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::string_prototype::code_units;
use crate::runtime::waiter_list_manager;
use crate::wtf::text::wtf_string::String as WtfString;

/// Teto de callbacks por drenagem: um `setInterval` que ninguém limpa nunca termina no bun; aqui o teste não trava.
const MAX_DISPATCHES: usize = 1_000_000;
/// Custo virtual, em ms, de uma volta do laço com immediates pendentes (potência de dois: a soma é exata).
const LOOP_PASS_MS: f64 = 1.0 / 64.0;

/// Quanto o laço espera a rede (tempo real) antes de adiantar o relógio virtual até o próximo timer.
const NET_GRACE: std::time::Duration = std::time::Duration::from_millis(2);

/// O maior atraso aceito (`2^31 - 1`).
const MAX_DELAY: f64 = 2_147_483_647.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Timeout,
    Interval,
    Immediate,
}

/// O que um timer faz ao disparar: chama uma função do script, ou roda uma ação nativa (o `AbortSignal.timeout`
/// do bun agenda uma ação do próprio runtime, sem função JS).
#[derive(Clone)]
enum Callback {
    Js(JSValue),
    Native(Rc<dyn Fn(&JSGlobalObject)>),
}

impl Callback {
    /// A função do script (`_onTimeout`); `undefined` numa ação nativa.
    fn js_value(&self) -> JSValue {
        match self {
            Callback::Js(value) => *value,
            Callback::Native(_) => JSValue::undefined(),
        }
    }
}

/// O estado de um `Timeout` ou `Immediate`.
struct Entry {
    kind: Kind,
    id: u32,
    callback: Callback,
    args: Vec<JSValue>,
    /// Intervalo com que o timer é armado de novo (intervalo, `refresh`): o do `setTimeout`, que gravar em
    /// `_idleTimeout` não muda (medido no bun 1.4.2).
    delay: f64,
    /// `_idleTimeout`: o número do armamento até o script gravar outro valor, que o getter devolve como veio.
    idle_timeout: JSValue,
    /// `_idleStart`.
    start: f64,
    /// `_repeat`.
    repeat: JSValue,
    /// Instante virtual de disparo (timers).
    fire_at: f64,
    /// Sequência do armamento: desempata disparos no mesmo instante e invalida o armamento antigo.
    seq: u64,
    destroyed: bool,
    refed: bool,
    /// O contexto assíncrono do agendamento (`AsyncContextFrame::withAsyncContextIfNeeded`), vazio sem rastreamento.
    async_context: JSValue,
}

/// Uma carga de módulo (o `import()` de um módulo ainda não carregado): no bun a leitura do arquivo é uma
/// macrotask, então o corpo do módulo e o `.then` só rodam quando o laço a entrega.
struct Load {
    fire_at: f64,
    seq: u64,
    task: Box<dyn FnOnce(&JSGlobalObject)>,
}

/// Atraso virtual da leitura de um módulo pedido de dentro do laço (ms). Medido no bun 1.4.2 com
/// `/tmp/jscm2/t6.mjs`: o `.then` de um `import()` feito num immediate chegou 0,65; 2,18; 2,76 e 7,82 ms
/// depois, a mediana (2,5) cai entre os timers de 2 e de 3 ms. Pedido antes do laço começar (no corpo do
/// script principal) a leitura termina antes da primeira volta: atraso 0.
const LOAD_LATENCY_IN_LOOP: f64 = 2.5;

#[derive(Default)]
struct State {
    /// `true` nos modos que rodam o laço (`run_event_loop`): só neles a carga de módulo vira tarefa do laço.
    loop_driven: bool,
    /// `true` enquanto o laço despacha (depois do script principal).
    in_loop: bool,
    /// `true` depois de uma exceção fatal não capturada: nenhum callback mais roda e o laço termina.
    halted: bool,
    /// Fim de uma fatia de [`run_event_loop_for`]: o laço sai e a próxima chamada o retoma de onde parou.
    paused: bool,
    loads: Vec<Load>,
    next_id: u32,
    next_seq: u64,
    /// Por `cell_id` do objeto devolvido ao script.
    entries: HashMap<usize, Entry>,
    by_id: HashMap<u32, usize>,
    /// Fila dos immediates, na ordem de chegada.
    immediates: Vec<usize>,
    timeout_prototype: Option<JSValue>,
    immediate_prototype: Option<JSValue>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Fim do programa (`cell_registry::reset_program_state`): o estado guarda callbacks e objetos do programa.
pub(crate) fn reset_for_program() {
    let taken = STATE.try_with(|state| std::mem::take(&mut *state.borrow_mut()));
    drop(taken);
}

/// As raízes do laço para o coletor: o objeto, o callback, os argumentos e o `_repeat` de cada timer ou immediate
/// ainda armado (o `Timeout` pendente do node segura tudo isso), e os protótipos.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    let _ = STATE.try_with(|state| {
        let Ok(state) = state.try_borrow() else {
            marker.abort();
            return;
        };
        for (&cell_id, entry) in state.entries.iter().filter(|(_, entry)| !entry.destroyed) {
            marker.cell(cell_id);
            marker.value(entry.callback.js_value());
            marker.values(&entry.args);
            marker.value(entry.repeat);
            marker.value(entry.async_context);
        }
        marker.value(state.timeout_prototype);
        marker.value(state.immediate_prototype);
    });
}

impl State {
    fn take_seq(&mut self) -> u64 {
        self.next_seq += 1;
        self.next_seq
    }

    /// `true` quando algo mantém o laço vivo: timer ou immediate ativo com `ref`.
    fn alive(&self) -> bool {
        !self.loads.is_empty() || self.entries.values().any(|entry| !entry.destroyed && entry.refed)
    }

    fn has_pending_immediates(&self) -> bool {
        self.immediates.iter().any(|cell| self.entries.get(cell).is_some_and(|entry| !entry.destroyed))
    }

    /// O menor instante de disparo entre os timers ativos.
    fn earliest_fire(&self) -> Option<f64> {
        self.entries
            .values()
            .filter(|entry| entry.kind != Kind::Immediate && !entry.destroyed)
            .map(|entry| entry.fire_at)
            .chain(self.loads.iter().map(|load| load.fire_at))
            .reduce(f64::min)
    }

    /// Arma de novo o timer `cell` a partir de `now` (intervalo, `refresh`).
    fn rearm(&mut self, cell: usize, now: f64) {
        let seq = self.take_seq();
        if let Some(entry) = self.entries.get_mut(&cell) {
            entry.start = now;
            entry.fire_at = now + entry.delay;
            entry.seq = seq;
        }
    }
}

/// `normalizeTimeout` do bun.
fn normalize_delay(value: f64) -> f64 {
    if !(value >= 1.0 && value <= MAX_DELAY) {
        return 1.0;
    }
    value.trunc()
}

pub(crate) fn native(global_object: &JSGlobalObject, name: &str, length: u32, function: NativeFunction) -> JSValue {
    native_function(global_object, name, length, function).as_value()
}

/// Como [`native`], devolvendo a função (para quem precisa pendurar propriedades nela).
pub(crate) fn native_function(global_object: &JSGlobalObject, name: &str, length: u32, function: NativeFunction) -> crate::runtime::js_function::JSFunctionRef {
    native_function_with_constructor(global_object, name, length, function, call_host_function_as_constructor)
}

/// Como [`native_function`], com o `[[Construct]]` escolhido por quem chama.
pub(crate) fn native_function_with_constructor(
    global_object: &JSGlobalObject,
    name: &str,
    length: u32,
    function: NativeFunction,
    constructor: NativeFunction,
) -> crate::runtime::js_function::JSFunctionRef {
    let vm = global_object.vm();
    // Quem pendura propriedades na função não pode transitar a `hostFunctionStructure` compartilhada, que
    // no bun segue folha (o `, Leaf` do dump de bytecode).
    JSFunction::create_native_with_own_structure(
        vm,
        global_object,
        length,
        &WtfString::from_latin1(name.as_bytes()),
        function,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        constructor,
    )
}


fn put_method(global_object: &JSGlobalObject, object: &JSObject, name: &str, length: u32, function: NativeFunction) {
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
        DONT_DELETE,
    );
}

/// O acessor `name` do protótipo: enumerável, não configurável, com as funções `get name` e `set name`.
pub(crate) fn put_accessor(global_object: &JSGlobalObject, object: &JSObject, name: &str, getter: NativeFunction, setter: Option<NativeFunction>) {
    put_accessor_with(global_object, object, name, getter, setter, DONT_DELETE);
}

/// Como [`put_accessor`], com os atributos à escolha (além de `ACCESSOR`): `0` dá acessor enumerável e configurável.
pub(crate) fn put_accessor_with(
    global_object: &JSGlobalObject,
    object: &JSObject,
    name: &str,
    getter: NativeFunction,
    setter: Option<NativeFunction>,
    attributes: u32,
) {
    let vm = global_object.vm();
    let getter_function = native(global_object, &format!("get {name}"), 0, getter);
    let setter_function = setter.map_or_else(JSValue::undefined, |function| native(global_object, &format!("set {name}"), 1, function));
    let accessor = GetterSetter::create_from_values(vm, getter_function, setter_function);
    // SEM transição (`putDirectWithoutTransition`), que muta a estrutura do objeto: só vale para objeto com estrutura
    // própria. `build_prototype` dá uma a cada protótipo; um objeto de `construct_empty_object` a compartilharia com
    // os demais chamadores (foi assim que `Immediate` achou o `_destroyed` de `Timeout` já na estrutura).
    // Medido: com este put, atribuir em modo estrito a `_destroyed` (só getter) não lança, como no bun; com o put
    // com transição lança (ver PLAN.md, questão em aberto).
    object.put_direct_non_index_accessor_without_transition(vm, &prop(vm, name), &accessor, attributes | ACCESSOR);
}

/// Chamar `Timeout()`/`Immediate()` sem `new` lança `ERR_ILLEGAL_CONSTRUCTOR` (medido no bun 1.4.2).
fn illegal_constructor_message(global_object: &JSGlobalObject, class_name: &str) -> HostResult {
    Err(throw_coded_type_error(global_object, &format!("{class_name} constructor cannot be invoked without 'new'"), "ERR_ILLEGAL_CONSTRUCTOR"))
}

fn illegal_timeout_constructor_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    illegal_constructor_message(global_object, "Timeout")
}
host_function!(illegal_timeout_constructor, illegal_timeout_constructor_body);

fn illegal_immediate_constructor_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    illegal_constructor_message(global_object, "Immediate")
}
host_function!(illegal_immediate_constructor, illegal_immediate_constructor_body);

/// `new Timeout()`/`new Immediate()` lança um `Error` simples `<Classe> is not constructible` (medido no bun 1.4.2).
fn not_constructible_body(global_object: &JSGlobalObject, class_name: &str) -> HostResult {
    Err(throw_plain_error(global_object, &format!("{class_name} is not constructible")))
}

fn timeout_not_constructible_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    not_constructible_body(global_object, "Timeout")
}
host_function!(timeout_not_constructible, timeout_not_constructible_body);

fn immediate_not_constructible_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    not_constructible_body(global_object, "Immediate")
}
host_function!(immediate_not_constructible, immediate_not_constructible_body);

/// Lê o estado do objeto `this`.
fn with_entry<R>(this_value: JSValue, read: impl FnOnce(&mut Entry) -> R) -> Option<R> {
    let JSValue::Cell(cell) = this_value else {
        return None;
    };
    STATE.with(|state| state.borrow_mut().entries.get_mut(&cell).map(read))
}

fn destroyed_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(with_entry(call.this_value(), |entry| JSValue::Bool(entry.destroyed)).unwrap_or_else(JSValue::undefined))
}
host_function!(destroyed_getter, destroyed_body);

fn idle_start_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(with_entry(call.this_value(), |entry| JSValue::from_double(entry.start)).unwrap_or_else(JSValue::undefined))
}
host_function!(idle_start_getter, idle_start_body);

fn set_idle_start_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let number = pending_or(global_object, call.argument(0).to_number())?;
    with_entry(call.this_value(), |entry| entry.start = number);
    Ok(JSValue::undefined())
}
host_function!(idle_start_setter, set_idle_start_body);

fn idle_timeout_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(with_entry(call.this_value(), |entry| entry.idle_timeout).unwrap_or_else(JSValue::undefined))
}
host_function!(idle_timeout_getter, idle_timeout_body);

fn set_idle_timeout_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let value = call.argument(0);
    with_entry(call.this_value(), |entry| entry.idle_timeout = value);
    Ok(JSValue::undefined())
}
host_function!(idle_timeout_setter, set_idle_timeout_body);

fn on_timeout_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(with_entry(call.this_value(), |entry| entry.callback.js_value()).unwrap_or_else(JSValue::undefined))
}
host_function!(on_timeout_getter, on_timeout_body);

fn set_on_timeout_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let callback = call.argument(0);
    with_entry(call.this_value(), |entry| entry.callback = Callback::Js(callback));
    Ok(JSValue::undefined())
}
host_function!(on_timeout_setter, set_on_timeout_body);

fn repeat_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(with_entry(call.this_value(), |entry| entry.repeat).unwrap_or_else(JSValue::undefined))
}
host_function!(repeat_getter, repeat_body);

fn set_repeat_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let value = call.argument(0);
    with_entry(call.this_value(), |entry| entry.repeat = value);
    Ok(JSValue::undefined())
}
host_function!(repeat_setter, set_repeat_body);

/// `close()` de `Timeout` e `[Symbol.dispose]()` dos dois: cancela e devolve `this`.
fn close_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_entry(call.this_value(), |entry| entry.destroyed = true);
    Ok(call.this_value())
}
host_function!(timer_close, close_body);

fn has_ref_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(with_entry(call.this_value(), |entry| JSValue::Bool(entry.refed && !entry.destroyed)).unwrap_or_else(JSValue::undefined))
}
host_function!(timer_has_ref, has_ref_body);

fn ref_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_entry(call.this_value(), |entry| entry.refed = true);
    Ok(call.this_value())
}
host_function!(timer_ref, ref_body);

fn unref_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    with_entry(call.this_value(), |entry| entry.refed = false);
    Ok(call.this_value())
}
host_function!(timer_unref, unref_body);

/// `refresh()`: arma de novo a partir de agora, mesmo se já disparou ou foi limpo.
fn refresh_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_value = call.this_value();
    if let JSValue::Cell(cell) = this_value {
        let now = waiter_list_manager::virtual_now();
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            if state.entries.get(&cell).is_some_and(|entry| entry.kind != Kind::Immediate) {
                state.rearm(cell, now);
                if let Some(entry) = state.entries.get_mut(&cell) {
                    entry.destroyed = false;
                }
            }
        });
    }
    Ok(this_value)
}
host_function!(timer_refresh, refresh_body);

/// `[Symbol.toPrimitive]()`: o id numérico.
fn to_primitive_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(with_entry(call.this_value(), |entry| JSValue::from_double(f64::from(entry.id))).unwrap_or_else(JSValue::undefined))
}
host_function!(timer_to_primitive, to_primitive_body);

/// Monta `Timeout.prototype` ou `Immediate.prototype` (e o construtor, que o script não alcança por nome).
fn build_prototype(global_object: &JSGlobalObject, class_name: &str, is_timeout: bool) -> JSValue {
    let vm = global_object.vm();
    // Estrutura própria (não a compartilhada de `construct_empty_object`): os acessores entram sem transição.
    let structure = crate::runtime::native_class_support::instance_structure(vm, Some(global_object), global_object.object_prototype().as_value());
    let prototype = crate::runtime::js_object::JSFinalObject::create(vm, &structure);
    put_accessor(global_object, &prototype, "_destroyed", destroyed_getter, None);
    if is_timeout {
        put_accessor(global_object, &prototype, "_idleStart", idle_start_getter, Some(idle_start_setter));
        put_accessor(global_object, &prototype, "_idleTimeout", idle_timeout_getter, Some(idle_timeout_setter));
        put_accessor(global_object, &prototype, "_onTimeout", on_timeout_getter, Some(on_timeout_setter));
        put_accessor(global_object, &prototype, "_repeat", repeat_getter, Some(repeat_setter));
        put_method(global_object, &prototype, "close", 0, timer_close);
    }
    put_method(global_object, &prototype, "hasRef", 0, timer_has_ref);
    put_method(global_object, &prototype, "ref", 0, timer_ref);
    if is_timeout {
        put_method(global_object, &prototype, "refresh", 0, timer_refresh);
    }
    put_method(global_object, &prototype, "unref", 0, timer_unref);

    let (call, construct) = if is_timeout {
        (illegal_timeout_constructor as NativeFunction, timeout_not_constructible as NativeFunction)
    } else {
        (illegal_immediate_constructor as NativeFunction, immediate_not_constructible as NativeFunction)
    };
    let constructor = native_function_with_constructor(global_object, class_name, 0, call, construct).as_value();
    if let Some(constructor_object) = JSObject::from_value(&constructor) {
        constructor_object.put_direct(vm, &prop(vm, "prototype"), prototype.as_value(), DONT_ENUM | DONT_DELETE | READ_ONLY);
    }
    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor, DONT_ENUM);

    let dispose = native(global_object, "dispose", 1, timer_close);
    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.dispose_symbol), dispose, DONT_ENUM | READ_ONLY);
    let to_primitive = native(global_object, "toPrimitive", 1, timer_to_primitive);
    prototype.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.to_primitive_symbol), to_primitive, DONT_ENUM | READ_ONLY);
    put_to_string_tag(vm, &prototype, class_name);
    prototype.as_value()
}

fn prototype_for(global_object: &JSGlobalObject, is_timeout: bool) -> JSValue {
    let cached = STATE.with(|state| {
        let state = state.borrow();
        if is_timeout { state.timeout_prototype } else { state.immediate_prototype }
    });
    if let Some(prototype) = cached {
        return prototype;
    }
    let prototype = build_prototype(global_object, if is_timeout { "Timeout" } else { "Immediate" }, is_timeout);
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if is_timeout {
            state.timeout_prototype = Some(prototype);
        } else {
            state.immediate_prototype = Some(prototype);
        }
    });
    prototype
}

/// Os avisos do bun para um atraso que é número e sai de `[1, 2^31-1]` (medido: só quando o argumento é um número
/// de fato, string, `null`, `true` e `0` não avisam; `-0` e `0.5` também não). O texto formata o número como o `{}`
/// do f64 (`1000000000000000000000`, não `1e+21`).
fn warn_delay(global_object: &JSGlobalObject, number: f64) {
    let text = if number == f64::INFINITY { String::from("Infinity") } else if number == f64::NEG_INFINITY { String::from("-Infinity") } else { format!("{number}") };
    let (message, kind) = if number.is_nan() {
        ("NaN is not a number.".to_string(), "TimeoutNaNWarning")
    } else if number < 0.0 {
        (format!("{text} is a negative number."), "TimeoutNegativeWarning")
    } else if number > MAX_DELAY {
        (format!("{text} does not fit into a 32-bit signed integer."), "TimeoutOverflowWarning")
    } else {
        return;
    };
    crate::runtime::process_warning::emit_runtime_warning(global_object, &format!("{message}\nTimeout duration was set to 1."), kind);
}

/// O corpo comum de `setTimeout`, `setInterval` e `setImmediate`.
fn create_timer(global_object: &JSGlobalObject, call: &HostCall, kind: Kind, function_name: &str) -> HostResult {
    if call.argument_count() == 0 {
        return Err(throw_coded_type_error(global_object, &format!("{function_name} requires 1 argument (a function)"), "ERR_INVALID_ARG_TYPE"));
    }
    let callback = call.argument(0);
    if !callback.is_callable() {
        return Err(throw_coded_type_error(global_object, &format!("{function_name} expects a function"), "ERR_INVALID_ARG_TYPE"));
    }
    let (delay, first_extra) = if kind == Kind::Immediate {
        (0.0, 1)
    } else {
        let requested = call.argument(1);
        let number = pending_or(global_object, requested.to_number())?;
        if requested.is_number() {
            warn_delay(global_object, number);
        }
        (normalize_delay(number), 2)
    };
    let args: Vec<JSValue> = call.arguments().iter().skip(first_extra).copied().collect();
    Ok(register(global_object, kind, Callback::Js(callback), args, delay, delay).as_value())
}

/// Cria o objeto `Timeout`/`Immediate` e arma a entrada. `fire_delay` é o prazo real de disparo (o `delay` é o
/// `_idleTimeout`). Uma ação nativa não gasta id (medido: o `setTimeout` seguinte a um `AbortSignal.timeout`
/// recebe o id seguinte) e não segura o laço (medido: um `AbortSignal.timeout` sozinho não mantém o processo vivo).
fn register(global_object: &JSGlobalObject, kind: Kind, callback: Callback, args: Vec<JSValue>, delay: f64, fire_delay: f64) -> JSObjectRef {
    let native = matches!(callback, Callback::Native(_));
    let vm = global_object.vm();
    let prototype = prototype_for(global_object, kind != Kind::Immediate);
    let object = construct_empty_object(global_object);
    object.set_prototype_direct(vm, prototype);
    let cell = object.cell_id();

    let now = waiter_list_manager::virtual_now();
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let id = if native {
            0
        } else {
            state.next_id += 1;
            state.next_id
        };
        let seq = state.take_seq();
        state.entries.insert(
            cell,
            Entry {
                kind,
                id,
                callback,
                args,
                delay,
                idle_timeout: JSValue::from_double(delay),
                start: now,
                repeat: if kind == Kind::Interval { JSValue::from_double(delay) } else { JSValue::null() },
                fire_at: now + fire_delay,
                seq,
                destroyed: false,
                refed: !native,
                async_context: vm.captured_async_context(),
            },
        );
        if !native {
            state.by_id.insert(id, cell);
        }
        if kind == Kind::Immediate {
            state.immediates.push(cell);
        }
    });
    object
}

/// Agenda `action` para daqui a `milliseconds` (já truncado), sem função JS: o `AbortSignal.timeout`. Dispara na
/// ordem dos `setTimeout` do mesmo prazo (por instante e sequência). Prazo 0 não é "agora": medido no bun, o
/// timer de 0 ms roda depois dos immediates já enfileirados e antes de qualquer `setTimeout` (mínimo de 1 ms),
/// então vale uma volta do laço (`LOOP_PASS_MS`).
pub(crate) fn schedule_native(global_object: &JSGlobalObject, milliseconds: f64, held: Vec<JSValue>, action: Rc<dyn Fn(&JSGlobalObject)>) {
    schedule_native_with_ref(global_object, milliseconds, held, action, false);
}

/// Como [`schedule_native`], com o timer segurando o laço vivo quando `refed` (a resposta da rede do navegador, o
/// fim de uma fatia de [`run_event_loop_for`]): sem isso o laço só com ele sai sem chegar ao prazo. `held` são os
/// valores que `action` captura: a entrada os segura para o coletor até disparar, como os argumentos de um
/// `setTimeout`.
pub(crate) fn schedule_native_with_ref(
    global_object: &JSGlobalObject,
    milliseconds: f64,
    held: Vec<JSValue>,
    action: Rc<dyn Fn(&JSGlobalObject)>,
    refed: bool,
) {
    let fire_delay = if milliseconds == 0.0 { LOOP_PASS_MS } else { milliseconds };
    let object = register(global_object, Kind::Timeout, Callback::Native(action), held, milliseconds, fire_delay);
    if refed {
        STATE.with(|state| {
            if let Some(entry) = state.borrow_mut().entries.get_mut(&object.cell_id()) {
                entry.refed = true;
            }
        });
    }
}

fn set_timeout_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    create_timer(global_object, call, Kind::Timeout, "setTimeout")
}
host_function!(global_func_set_timeout, set_timeout_body);

fn set_interval_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    create_timer(global_object, call, Kind::Interval, "setInterval")
}
host_function!(global_func_set_interval, set_interval_body);

fn set_immediate_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    create_timer(global_object, call, Kind::Immediate, "setImmediate")
}
host_function!(global_func_set_immediate, set_immediate_body);

/// `clearTimeout`/`clearInterval`/`clearImmediate`. Medido no bun 1.4.2: o objeto devolvido só cancela na própria
/// família (timeout e interval se cancelam entre si, o immediate só pelo `clearImmediate`); o id (número inteiro, ou
/// a string que é exatamente o seu decimal, sem espaço, sinal, zero à esquerda ou fração) procura só entre timeouts e
/// intervals, pelas três funções (`clearImmediate(+timeout)` cancela, `clearImmediate(+immediate)` não). Qualquer
/// outro valor é ignorado.
fn clear(call: &HostCall, immediate: bool) -> HostResult {
    let argument = call.argument(0);
    let id = if argument.is_number() {
        let number = argument.to_number();
        (number >= 0.0 && number.fract() == 0.0 && number <= f64::from(u32::MAX)).then_some(number as u32)
    } else if argument.is_string() {
        let units = code_units(&argument.as_js_string().value()).into_owned();
        let canonical = !units.is_empty() && units.iter().all(|&unit| (b'0' as u16..=b'9' as u16).contains(&unit)) && (units.len() == 1 || units[0] != b'0' as u16);
        canonical.then(|| String::from_utf16_lossy(&units).parse::<u32>().ok()).flatten()
    } else {
        None
    };
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let target = match (argument, id) {
            (JSValue::Cell(cell), _) if state.entries.contains_key(&cell) => Some((cell, immediate)),
            (_, Some(id)) => state.by_id.get(&id).map(|&cell| (cell, false)),
            _ => None,
        };
        if let Some((cell, family_is_immediate)) = target {
            if let Some(entry) = state.entries.get_mut(&cell) {
                if (entry.kind == Kind::Immediate) == family_is_immediate {
                    entry.destroyed = true;
                }
            }
        }
    });
    Ok(JSValue::undefined())
}

fn clear_timeout_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    clear(call, false)
}
host_function!(global_func_clear_timeout, clear_timeout_body);

fn clear_interval_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    clear(call, false)
}
host_function!(global_func_clear_interval, clear_interval_body);

fn clear_immediate_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    clear(call, true)
}
host_function!(global_func_clear_immediate, clear_immediate_body);

fn install(global_object: &JSGlobalObject, table: &[(&str, NativeFunction)]) {
    let vm = global_object.vm();
    for (name, function) in table {
        let identifier = Identifier::from_span(vm, name.as_bytes());
        let value = native(global_object, name, 1, *function);
        global_object.put_direct(vm, &PropertyName::from_identifier(&identifier), value, 0);
    }
}

/// Instala `clearImmediate`, `clearInterval` e `clearTimeout` (no bun vêm antes de `queueMicrotask`).
pub fn add_clear_functions(global_object: &JSGlobalObject) {
    install(
        global_object,
        &[
            ("clearImmediate", global_func_clear_immediate as NativeFunction),
            ("clearInterval", global_func_clear_interval as NativeFunction),
            ("clearTimeout", global_func_clear_timeout as NativeFunction),
        ],
    );
}

/// Instala `setImmediate`, `setInterval` e `setTimeout` (no bun vêm depois de `queueMicrotask`).
pub fn add_set_functions(global_object: &JSGlobalObject) {
    install(
        global_object,
        &[
            ("setImmediate", global_func_set_immediate as NativeFunction),
            ("setInterval", global_func_set_interval as NativeFunction),
            ("setTimeout", global_func_set_timeout as NativeFunction),
        ],
    );
}

/// Chama o callback; a exceção vai ao relatório de erro não capturado (`uncaughtException`) e o laço segue.
fn invoke(global_object: &JSGlobalObject, callback: &Callback, this_value: JSValue, args: &[JSValue], async_context: JSValue) {
    if STATE.with(|state| state.borrow().halted) {
        return;
    }
    // A entrada pode cair durante a chamada (`clearTimeout` de dentro, timeout de disparo único): o que o laço
    // segura aqui fica enraizado até o callback voltar.
    let _roots = crate::runtime::gc::RootScope::new(&[&[callback.js_value(), this_value, async_context][..], args].concat());
    let scope = crate::runtime::js_microtask::AsyncContextSwapScope::new(global_object.vm(), async_context);
    match callback {
        Callback::Js(function) => match call_microtask(global_object, *function, this_value, args, "callback is not a function") {
            Err(CallThrown::Value(error)) => global_object.vm().report_unhandled_error(global_object, error),
            Err(CallThrown::Termination) | Ok(_) => {}
        },
        Callback::Native(action) => action(global_object),
    }
    // As microtarefas que o callback agendou levam o próprio contexto; a drenagem roda fora do dele.
    drop(scope);
    // `process.exit()` descarta as microtarefas pendentes.
    if !crate::runtime::process_exit::exit_requested() {
        global_object.vm().drain_microtasks();
    }
}

/// Chama `function` (um callback de descritor de rede) como uma tarefa do laço: exceção vai ao relatório de erro não
/// capturado e as microtasks rodam depois.
pub(crate) fn call_from_loop(global_object: &JSGlobalObject, function: JSValue, args: &[JSValue]) {
    invoke(global_object, &Callback::Js(function), JSValue::undefined(), args, JSValue::undefined());
}

/// Uma exceção fatal foi relatada: o laço para antes do próximo callback (o processo vai sair com código 1).
pub(crate) fn halt_event_loop() {
    STATE.with(|state| state.borrow_mut().halted = true);
}

/// Declara que o laço vai rodar (os pontos de entrada com timers chamam isto antes do script principal).
pub(crate) fn enable_event_loop() {
    STATE.with(|state| state.borrow_mut().loop_driven = true);
}

/// `true` quando o laço de eventos vai rodar neste programa.
pub(crate) fn event_loop_is_driven() -> bool {
    STATE.with(|state| state.borrow().loop_driven)
}

/// Entrega `task` ao laço como a leitura de um módulo novo (só com `event_loop_is_driven`).
pub(crate) fn schedule_module_load(task: Box<dyn FnOnce(&JSGlobalObject)>) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let latency = if state.in_loop { LOAD_LATENCY_IN_LOOP } else { 0.0 };
        let seq = state.take_seq();
        state.loads.push(Load { fire_at: waiter_list_manager::virtual_now() + latency, seq, task });
    });
}

/// A fase das cargas: as leituras de módulo concluídas, cada uma seguida das microtasks. Vem antes dos timers
/// (medido: um módulo pedido no script principal roda antes de `setImmediate` e de `setTimeout(0)`).
fn run_due_loads(global_object: &JSGlobalObject) {
    let now = waiter_list_manager::virtual_now();
    let mut due: Vec<Load> = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let (due, rest): (Vec<Load>, Vec<Load>) =
            std::mem::take(&mut state.loads).into_iter().partition(|load| load.fire_at <= now);
        state.loads = rest;
        due
    });
    due.sort_by(|left, right| left.fire_at.total_cmp(&right.fire_at).then(left.seq.cmp(&right.seq)));
    for load in due {
        (load.task)(global_object);
        global_object.vm().drain_microtasks();
    }
}

/// A fase dos timers: roda os vencidos em `(instante, sequência)`, cada um seguido das microtasks.
fn run_due_timers(global_object: &JSGlobalObject, budget: &mut usize) {
    let now = waiter_list_manager::virtual_now();
    let mut due: Vec<(f64, u64, usize)> = STATE.with(|state| {
        let state = state.borrow();
        state
            .entries
            .iter()
            .filter(|(_, entry)| entry.kind != Kind::Immediate && !entry.destroyed && entry.fire_at <= now)
            .map(|(cell, entry)| (entry.fire_at, entry.seq, *cell))
            .collect()
    });
    due.sort_by(|left, right| left.0.total_cmp(&right.0).then(left.1.cmp(&right.1)));
    for (_, seq, cell) in due {
        if *budget == 0 {
            return;
        }
        let call = STATE.with(|state| {
            state
                .borrow()
                .entries
                .get(&cell)
                .filter(|entry| !entry.destroyed && entry.seq == seq)
                .map(|entry| (entry.callback.clone(), entry.args.clone(), entry.async_context))
        });
        let Some((callback, args, async_context)) = call else {
            continue;
        };
        *budget -= 1;
        invoke(global_object, &callback, JSValue::Cell(cell), &args, async_context);
        let now = waiter_list_manager::virtual_now();
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            let Some(entry) = state.entries.get(&cell) else {
                return;
            };
            if entry.destroyed || entry.seq != seq {
                return;
            }
            if entry.kind == Kind::Interval {
                state.rearm(cell, now);
            } else if let Some(entry) = state.entries.get_mut(&cell) {
                entry.destroyed = true;
            }
        });
    }
}

/// A fase dos immediates: roda a fila como estava no início (os agendados dentro dela ficam para a próxima volta).
fn run_immediates(global_object: &JSGlobalObject, budget: &mut usize) {
    let queue = STATE.with(|state| std::mem::take(&mut state.borrow_mut().immediates));
    for (position, cell) in queue.iter().enumerate() {
        if *budget == 0 {
            // Devolve o que não rodou, antes dos recém-agendados.
            STATE.with(|state| {
                let mut state = state.borrow_mut();
                let mut rest: Vec<usize> = queue[position..].to_vec();
                rest.append(&mut state.immediates);
                state.immediates = rest;
            });
            return;
        }
        // O bun solta o `ref` do immediate antes de chamar o callback: dentro dele `hasRef()` já é `false`, com
        // `_destroyed` ainda `false` (medido no bun 1.4.2).
        let call = STATE.with(|state| {
            state.borrow_mut().entries.get_mut(cell).filter(|entry| !entry.destroyed).map(|entry| {
                entry.refed = false;
                (entry.callback.clone(), entry.args.clone(), entry.async_context)
            })
        });
        let Some((callback, args, async_context)) = call else {
            continue;
        };
        *budget -= 1;
        invoke(global_object, &callback, JSValue::Cell(*cell), &args, async_context);
        STATE.with(|state| {
            if let Some(entry) = state.borrow_mut().entries.get_mut(cell) {
                entry.destroyed = true;
            }
        });
    }
}

/// O laço de eventos do host em tempo virtual (ver o cabeçalho do módulo): roda até não restar nada vivo
/// nem prazo de `Atomics.waitAsync`.
pub fn run_event_loop(global_object: &JSGlobalObject) {
    if run_event_loop_until_held(global_object) {
        // O processo do bun continuaria vivo, parado, até um sinal.
        loop {
            std::thread::park();
        }
    }
}

/// Roda o laço por `budget_ms` de tempo virtual (o `--virtual-time-budget` do Chromium, em fatias): o que vence até
/// lá dispara, o resto fica na fila para a chamada seguinte, que retoma o relógio de onde esta parou. `false` quando
/// o laço parou de vez (exceção fatal).
pub fn run_event_loop_for(global_object: &JSGlobalObject, budget_ms: f64) -> bool {
    schedule_native_with_ref(global_object, budget_ms, Vec::new(), Rc::new(|_| STATE.with(|state| state.borrow_mut().paused = true)), true);
    run_event_loop_until_held(global_object)
}

/// O laço de `run_event_loop`, mas em vez de parar para sempre quando só restam fontes que seguram o processo sem
/// trabalho pendente (canal com `ref`, ouvinte de `message`), devolve `true`. `false` é o laço que terminou.
pub fn run_event_loop_until_held(global_object: &JSGlobalObject) -> bool {
    let vm = global_object.vm();
    enable_event_loop();
    vm.drain_microtasks();
    // As tarefas do host (entrega dos `PerformanceObserver`) rodam antes da primeira volta: medido, o callback de
    // quem observou no script principal vem depois das microtasks e antes de `setTimeout(0)` e de `setImmediate`.
    crate::runtime::performance_observer::deliver_pending(global_object);
    crate::runtime::broadcast_channel::deliver_pending(global_object);
    crate::runtime::worker_host::deliver_pending(global_object);
    STATE.with(|state| state.borrow_mut().in_loop = true);
    let mut budget = MAX_DISPATCHES;
    loop {
        if STATE.with(|state| state.borrow().halted) {
            return false;
        }
        let runs_immediates = STATE.with(|state| {
            let state = state.borrow();
            state.alive() && state.has_pending_immediates()
        });
        if STATE.with(|state| state.borrow().alive()) {
            run_due_loads(global_object);
            run_due_timers(global_object, &mut budget);
            crate::runtime::net_loop::dispatch(global_object, Some(std::time::Duration::ZERO));
            run_immediates(global_object, &mut budget);
        } else if crate::runtime::net_loop::holds_event_loop() {
            crate::runtime::net_loop::dispatch(global_object, Some(std::time::Duration::ZERO));
        }
        // O que os callbacks da volta enfileiraram: entregue depois dos timers e immediates dela, antes do salto
        // do relógio (medido: uma marca criada num `setTimeout(1)` chega depois do outro timer do mesmo instante e
        // antes do de 2 ms).
        crate::runtime::performance_observer::deliver_pending(global_object);
        crate::runtime::broadcast_channel::deliver_pending(global_object);
        crate::runtime::worker_host::deliver_pending(global_object);
        if budget == 0 {
            return false;
        }
        // A pausa que venceu nesta volta encerra a fatia já: sem isso, com ela como último timer, o laço vazio sairia
        // por baixo com a pausa ainda armada para a fatia seguinte.
        if STATE.with(|state| std::mem::take(&mut state.borrow_mut().paused)) {
            return true;
        }
        let (alive, pending_immediates, earliest) = STATE.with(|state| {
            let state = state.borrow();
            (state.alive(), state.has_pending_immediates(), state.earliest_fire())
        });
        if alive && pending_immediates {
            // Uma volta do laço leva tempo: sem isso, immediates em cadeia nunca deixariam um timer vencer. No bun
            // (medido) um immediate agendado dentro de um immediate roda antes de um setTimeout(0/1/2) agendado no
            // mesmo ponto, uma cadeia de 100 voltas já deixa um setTimeout(1) vencer e uma de 100 não vence um de 5 ms.
            // Uma volta custa então bem menos que 1 ms; 1/64 ms é exato em ponto flutuante e dá ~1,56 ms por 100 voltas.
            waiter_list_manager::advance_virtual_clock_to(waiter_list_manager::virtual_now() + LOOP_PASS_MS);
            continue;
        }
        if !alive && runs_immediates {
            // A volta que rodou immediates termina com a fase de timers da volta seguinte antes de o laço olhar se
            // segue vivo: o timer sem `ref` que venceu nela dispara (medido no bun 1.4.2, `AbortSignal.timeout(0)` com
            // um `setImmediate` aborta depois do immediate; o de 5 ms não, e sem o immediate o de 0 também não).
            waiter_list_manager::advance_virtual_clock_to(waiter_list_manager::virtual_now() + LOOP_PASS_MS);
            run_due_timers(global_object, &mut budget);
            continue;
        }
        let next_timer = if alive { earliest } else { None };
        let next_waiter = waiter_list_manager::next_deadline();
        // Um timer pendente só dispara depois de dar à rede uma chance real de responder (o relógio é virtual, a
        // rede não): uma espera curta antes de adiantar o relógio.
        if next_timer.is_some() && crate::runtime::net_loop::holds_event_loop() && crate::runtime::net_loop::dispatch(global_object, Some(NET_GRACE)) {
            continue;
        }
        match (next_timer, next_waiter) {
            (None, None) => {
                // Servidor ou conexão com `ref`: espera o próximo evento de rede, sem prazo.
                if crate::runtime::net_loop::holds_event_loop() {
                    crate::runtime::net_loop::dispatch(global_object, None);
                    continue;
                }
                // Worker vivo e com `ref`: o filho roda de verdade em paralelo, então o laço espera a mensagem dele
                // (acorda a cada milissegundo para olhar o canal) em vez de terminar ou parar. O `beforeExit` só sai
                // com o laço realmente vazio, por isso esta espera vem antes dele.
                if crate::runtime::worker_host::holds_event_loop() {
                    if !crate::runtime::worker_host::deliver_pending(global_object) {
                        ul_common::time::sleep(std::time::Duration::from_millis(1));
                    }
                    continue;
                }
                // O laço esvaziou: `beforeExit`; timer ou immediate agendado nele revive o laço (o evento se repete).
                crate::runtime::process_exit::emit_before_exit(global_object);
                if STATE.with(|state| {
                    let state = state.borrow();
                    state.halted || state.alive()
                }) {
                    continue;
                }
                // Nada a disparar: o processo só continua vivo, parado, se um canal com `ref` ou um ouvinte de `message`
                // do global o segura (como o bun); sem isso o programa terminou.
                return crate::runtime::broadcast_channel::holds_event_loop()
                    || crate::runtime::message_channel::holds_event_loop()
                    || crate::runtime::event_target::global_has_listener(global_object, "message");
            }
            (Some(timer), waiter) if waiter.map_or(true, |waiter| timer < waiter) => waiter_list_manager::advance_virtual_clock_to(timer),
            _ => {
                waiter_list_manager::run_next_timer(global_object);
                vm.drain_microtasks();
            }
        }
    }
}
