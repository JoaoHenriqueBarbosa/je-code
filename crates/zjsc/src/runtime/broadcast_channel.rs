//! `BroadcastChannel` do global. O JavaScriptCore não o define: quem o instala é o bun (WebCore), como propriedade de
//! dados comum (`writable`, `enumerable`, `configurable`). Medido no bun 1.4.2 (`scripts/gen-broadcast-channel-golden.js`
//! para a forma e os erros, `scripts/gen-broadcast-channel-delivery-golden.js` para a entrega):
//!
//! - o construtor (`length` 1) herda de `EventTarget` e o protótipo de `EventTarget.prototype`; o protótipo tem, nesta
//!   ordem, `constructor` (não enumerável), os acessores `name`, `onmessage` e `onmessageerror` (os dois com setter), os
//!   métodos `postMessage` (`length` 1), `close`, `ref` e `unref` (`length` 0) e `@@toStringTag` "BroadcastChannel".
//!   Sem `new`: ``Use `new BroadcastChannel(...)` instead of `BroadcastChannel(...)` `` (`ERR_ILLEGAL_CONSTRUCTOR`); sem
//!   argumento: `Not enough arguments` (`ERR_MISSING_ARGS`); o nome passa por `ToString` (símbolo lança);
//! - brand check: métodos lançam `Can only call BroadcastChannel.<método> on instances of BroadcastChannel`
//!   (`ERR_INVALID_THIS`), acessores `The BroadcastChannel.<nome> getter can only be used on instances of BroadcastChannel`;
//! - `postMessage(valor)` clona o valor (`DataCloneError` se não clonável), uma cópia independente por destinatário, e
//!   a entrega é uma tarefa do host: depois das microtasks e `process.nextTick` e antes de `setImmediate` e de qualquer
//!   timer da volta. Os destinatários são os outros canais abertos do mesmo nome existentes no momento do envio (nunca o
//!   remetente), na ordem de criação; a fila é global e na ordem do envio; o que o remetente fecha depois do envio ainda
//!   chega, o que o destinatário fecha antes da entrega não chega. As microtasks esvaziam depois de cada mensagem;
//! - o `MessageEvent` entregue é confiável (`isTrusted`), de tipo `message`, com `origin` e `lastEventId` vazios, `source`
//!   `null` e `ports` vazio; vai por `dispatchEvent` do próprio destinatário (`onmessage` e `addEventListener`);
//! - depois de `close`, `postMessage` lança `InvalidStateError` ("This BroadcastChannel is closed", código 11) e
//!   `close` de novo não faz nada; `ref()` devolve `undefined` e `unref()` devolve o canal.
//!
//! - um canal aberto com `ref` segura o processo vivo, com ou sem `onmessage` (`holds_event_loop`, consultado por
//!   `timers.rs`): sem mais nada a fazer, o laço não termina, como o bun (que só sai por `close`, `unref` ou sinal). Por
//!   isso os testes só rodam o laço com programas que fecham ou dão `unref` nos canais (o golden de forma não roda o laço).

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};

use crate::host_function;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{derived_structure, put_to_string_tag};
use crate::runtime::event_target::{create_message_event, create_trusted_event, dispatch_event, event_handler, put_methods, register_target, set_event_handler, target_has_listener};
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO;
use crate::runtime::js_dom_exception::throw_dom_exception_from_host;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_undefined, EncodedJSValue, JSValue};
use crate::runtime::native_class_support::{
    create_native_subclass, install_global, instance_structure, put_native_accessor, throw_coded_type_error, throw_native_type_error,
};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::string_prototype::code_units;
use crate::runtime::structured_clone::clone_value_for_host;

/// Teto de mensagens entregues numa chamada de `deliver_pending` (dois canais que se respondem para sempre não travam o host).
const MAX_DELIVERIES_PER_CALL: usize = 1_000_000;

static BROADCAST_CHANNEL_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "BroadcastChannel", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// Para onde as mensagens de um ponto de entrega vão: o grupo de mesmo nome (`BroadcastChannel`) ou a porta par
/// (`MessagePort`, ver `message_channel.rs`, que reusa este registro, a fila e a tarefa de entrega).
pub(crate) enum Route {
    /// O nome como unidades UTF-16 (a identidade do grupo) e como string JS (o que `name` devolve).
    Name { units: Vec<u16>, value: JSValue },
    /// A porta par (`None` depois de a porta ser transferida), se algo já ativou esta porta (mensagem postada para ela,
    /// `ref`, `hasRef`) e se `unref` foi chamado; a porta aberta, ativada e com ouvinte segura o laço.
    Port { peer: Option<EncodedJSValue>, activated: bool, unrefed: bool },
}

/// O estado de um ponto de entrega (`BroadcastChannel` ou `MessagePort`).
pub(crate) struct Channel {
    pub(crate) route: Route,
    pub(crate) closed: bool,
    pub(crate) refed: bool,
    /// Ordem de criação: o envio percorre os canais nela.
    serial: u64,
}

/// Uma mensagem a entregar: o destinatário, o dado já clonado e as portas transferidas junto (`event.ports`).
struct Delivery {
    key: EncodedJSValue,
    data: JSValue,
    ports: Vec<JSValue>,
}

thread_local! {
    /// Os pontos de entrega do programa (valor codificado da célula) e o estado de cada um.
    static CHANNELS: RefCell<HashMap<EncodedJSValue, Channel>> = RefCell::new(HashMap::new());
    /// As mensagens a entregar, na ordem do envio.
    static QUEUE: RefCell<VecDeque<Delivery>> = const { RefCell::new(VecDeque::new()) };
    /// As portas que `close()` fechou e ainda não receberam o evento `close`, na ordem das chamadas.
    static CLOSED_PORTS: RefCell<VecDeque<EncodedJSValue>> = const { RefCell::new(VecDeque::new()) };
    static NEXT_SERIAL: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Fim do programa (`cell_registry::reset_program_state`): os canais e a fila guardam objetos do programa.
/// As raízes do coletor: os canais, as portas fechadas e cada entrega na fila (a tarefa do laço segura o destino, o
/// dado e as portas transferidas).
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    CHANNELS.with(|channels| channels.borrow().keys().for_each(|channel| marker.encoded(*channel)));
    CLOSED_PORTS.with(|ports| ports.borrow().iter().for_each(|port| marker.encoded(*port)));
    QUEUE.with(|queue| {
        for delivery in queue.borrow().iter() {
            marker.encoded(delivery.key);
            marker.value(delivery.data);
            marker.values(&delivery.ports);
        }
    });
}

pub(crate) fn reset_for_program() {
    let channels = CHANNELS.try_with(|channels| std::mem::take(&mut *channels.borrow_mut()));
    drop(channels);
    let queue = QUEUE.try_with(|queue| std::mem::take(&mut *queue.borrow_mut()));
    drop(queue);
    let closed_ports = CLOSED_PORTS.try_with(|closed| std::mem::take(&mut *closed.borrow_mut()));
    drop(closed_ports);
    let _ = NEXT_SERIAL.try_with(|serial| serial.set(0));
}

/// Registra `instance` como ponto de entrega (na ordem de criação).
pub(crate) fn register_channel(instance: JSValue, route: Route, refed: bool) {
    let serial = NEXT_SERIAL.with(|next| {
        let serial = next.get();
        next.set(serial + 1);
        serial
    });
    CHANNELS.with(|channels| channels.borrow_mut().insert(instance.encode(), Channel { route, closed: false, refed, serial }));
}

/// Roda `f` no estado de `value`, se ele for um ponto de entrega.
pub(crate) fn with_channel<R>(value: JSValue, f: impl FnOnce(&mut Channel) -> R) -> Option<R> {
    CHANNELS.with(|channels| channels.borrow_mut().get_mut(&value.encode()).map(f))
}

/// Roda `f` em todos os pontos de entrega, na ordem de criação.
pub(crate) fn for_each_channel(mut f: impl FnMut(EncodedJSValue, &mut Channel)) {
    CHANNELS.with(|channels| {
        let mut channels = channels.borrow_mut();
        let mut keys: Vec<(u64, EncodedJSValue)> = channels.iter().map(|(key, channel)| (channel.serial, *key)).collect();
        keys.sort();
        for (_, key) in keys {
            if let Some(channel) = channels.get_mut(&key) {
                f(key, channel);
            }
        }
    });
}

/// Enfileira `data` (já clonado) para `key`; a entrega é uma tarefa do host (`deliver_pending`).
pub(crate) fn enqueue(key: EncodedJSValue, data: JSValue, ports: Vec<JSValue>) {
    QUEUE.with(|queue| queue.borrow_mut().push_back(Delivery { key, data, ports }));
}

/// `port.close()` fechou a porta: o evento `close` dela sai na próxima tarefa do host (`deliver_pending`).
pub(crate) fn enqueue_port_close(key: EncodedJSValue) {
    CLOSED_PORTS.with(|closed| closed.borrow_mut().push_back(key));
}

/// Despacha um evento `close` confiável (sem dado, não borbulha) em `target` e esvazia as microtasks.
fn dispatch_close(global_object: &JSGlobalObject, target: JSValue) {
    if let Ok(event) = create_trusted_event(global_object, "close") {
        // A exceção de um ouvinte já foi para o relatório de erros não capturados dentro de `dispatch_event`.
        let _ = dispatch_event(global_object, target, event);
    }
    global_object.vm().drain_microtasks();
}

/// A porta par de `key`, se ela ainda existe (não foi transferida).
fn port_peer(key: EncodedJSValue) -> Option<EncodedJSValue> {
    with_channel(JSValue::decode(key), |channel| match &channel.route {
        Route::Port { peer, .. } => *peer,
        Route::Name { .. } => None,
    })
    .flatten()
}

/// A porta `old` foi transferida para `new`: o que estava a caminho dela passa a ir para a nova.
pub(crate) fn retarget(old: EncodedJSValue, new: EncodedJSValue) {
    QUEUE.with(|queue| queue.borrow_mut().iter_mut().filter(|delivery| delivery.key == old).for_each(|delivery| delivery.key = new));
}

fn is_channel(value: JSValue) -> bool {
    with_channel(value, |channel| matches!(channel.route, Route::Name { .. })).unwrap_or(false)
}

/// O `this` de um método de `class`, ou o `TypeError` `ERR_INVALID_THIS` do bun.
pub(crate) fn checked_method_this(global_object: &JSGlobalObject, call: &HostCall, class: &str, name: &str, accepts: fn(JSValue) -> bool) -> Result<JSValue, Thrown> {
    let this = call.this_value();
    if accepts(this) {
        Ok(this)
    } else {
        Err(throw_coded_type_error(global_object, &format!("Can only call {class}.{name} on instances of {class}"), "ERR_INVALID_THIS"))
    }
}

/// O `this` de um getter/setter de `class`, ou o `TypeError` do bun (sem `code`).
pub(crate) fn checked_accessor_this(global_object: &JSGlobalObject, call: &HostCall, class: &str, what: &str, name: &str, accepts: fn(JSValue) -> bool) -> Result<JSValue, Thrown> {
    let this = call.this_value();
    if accepts(this) {
        Ok(this)
    } else {
        Err(throw_native_type_error(global_object, &format!("The {class}.{name} {what} can only be used on instances of {class}")))
    }
}

fn method_this(global_object: &JSGlobalObject, call: &HostCall, name: &str) -> Result<JSValue, Thrown> {
    checked_method_this(global_object, call, "BroadcastChannel", name, is_channel)
}

fn accessor_this(global_object: &JSGlobalObject, call: &HostCall, what: &str, name: &str) -> Result<JSValue, Thrown> {
    checked_accessor_this(global_object, call, "BroadcastChannel", what, name, is_channel)
}

fn illegal_call_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(throw_coded_type_error(global_object, "Use `new BroadcastChannel(...)` instead of `BroadcastChannel(...)`", "ERR_ILLEGAL_CONSTRUCTOR"))
}

fn construct_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if call.argument_count() < 1 {
        return Err(throw_coded_type_error(global_object, "Not enough arguments", "ERR_MISSING_ARGS"));
    }
    let name = pending_or(global_object, call.argument(0).to_wtf_string())?;
    let units = code_units(&name).into_owned();
    let name_value = JSValue::from_js_string(js_string(global_object.vm(), &name));
    let structure = derived_structure(global_object, call, instance_structure)?;
    let instance = JSFinalObject::create(global_object.vm(), &structure).as_value();
    register_target(instance);
    register_channel(instance, Route::Name { units, value: name_value }, true);
    Ok(instance)
}

fn name_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = accessor_this(global_object, call, "getter", "name")?;
    Ok(with_channel(this, |channel| match &channel.route {
        Route::Name { value, .. } => *value,
        Route::Port { .. } => js_undefined(),
    })
    .unwrap_or_else(js_undefined))
}

fn onmessage_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(event_handler(accessor_this(global_object, call, "getter", "onmessage")?, "message"))
}

fn set_onmessage_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    set_event_handler(accessor_this(global_object, call, "setter", "onmessage")?, "message", call.argument(0));
    Ok(js_undefined())
}

fn onmessageerror_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(event_handler(accessor_this(global_object, call, "getter", "onmessageerror")?, "messageerror"))
}

fn set_onmessageerror_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    set_event_handler(accessor_this(global_object, call, "setter", "onmessageerror")?, "messageerror", call.argument(0));
    Ok(js_undefined())
}

/// `postMessage(valor)`: clona o valor e enfileira uma cópia para cada outro canal aberto do mesmo nome.
fn post_message_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = method_this(global_object, call, "postMessage")?;
    if call.argument_count() < 1 {
        return Err(throw_coded_type_error(global_object, "Not enough arguments", "ERR_MISSING_ARGS"));
    }
    let (name, closed) = with_channel(this, |channel| match &channel.route {
        Route::Name { units, .. } => (units.clone(), channel.closed),
        Route::Port { .. } => (Vec::new(), true),
    })
    .unwrap_or_default();
    if closed {
        return Err(throw_dom_exception_from_host(global_object, call, "InvalidStateError", "This BroadcastChannel is closed"));
    }
    // A serialização acontece uma vez (o erro é síncrono mesmo sem destinatário); cada destinatário recebe um clone da cópia.
    let serialized = clone_value_for_host(global_object, call, call.argument(0))?;
    let mut recipients: Vec<EncodedJSValue> = Vec::new();
    for_each_channel(|key, channel| {
        if key != this.encode() && !channel.closed && matches!(&channel.route, Route::Name { units, .. } if *units == name) {
            recipients.push(key);
        }
    });
    for key in recipients {
        let copy = clone_value_for_host(global_object, call, serialized)?;
        enqueue(key, copy, Vec::new());
    }
    Ok(js_undefined())
}

/// O nome como `util.inspect` o mostra: aspas simples, ou duplas/crase quando o texto contém a anterior, com as
/// escapadas do `strEscape` do Node (`\n \r \t \b \f`, `\xNN` maiúsculo nos demais controles e em 0x7f a 0x9f, `\uNNNN`
/// minúsculo nos substitutos soltos).
pub(crate) fn quoted_name(units: &[u16]) -> Vec<u16> {
    let contains = |quote: char| units.contains(&(quote as u16));
    let quote = ['\'', '"', '`'].into_iter().find(|quote| !contains(*quote)).unwrap_or('\'');
    let mut text = vec![quote as u16];
    let escape = |text: &mut Vec<u16>, shown: String| text.extend(shown.encode_utf16());
    for (index, &unit) in units.iter().enumerate() {
        let low_follows = (0xdc00..0xe000).contains(&units.get(index + 1).copied().unwrap_or(0));
        let high_precedes = index > 0 && (0xd800..0xdc00).contains(&units[index - 1]);
        match unit {
            0x5c => text.extend([0x5c, 0x5c]),
            0x0a => text.extend([0x5c, u16::from(b'n')]),
            0x0d => text.extend([0x5c, u16::from(b'r')]),
            0x09 => text.extend([0x5c, u16::from(b't')]),
            0x08 => text.extend([0x5c, u16::from(b'b')]),
            0x0c => text.extend([0x5c, u16::from(b'f')]),
            0x00..=0x1f | 0x7f..=0x9f => escape(&mut text, format!("\\x{unit:02X}")),
            0xd800..=0xdbff if !low_follows => escape(&mut text, format!("\\u{unit:x}")),
            0xdc00..=0xdfff if !high_precedes => escape(&mut text, format!("\\u{unit:x}")),
            _ if unit == quote as u16 => text.extend([0x5c, unit]),
            _ => text.push(unit),
        }
    }
    text.push(quote as u16);
    text
}

/// `[Symbol.for('nodejs.util.inspect.custom')](depth, options)` (medido no bun 1.4.2): o primeiro argumento manda,
/// `0` dá `BroadcastChannel [Object]`, `>= 1` dá `{ name, active }` e negativo só o nome da classe.
fn inspect_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = call.this_value();
    if !is_channel(this) {
        // Medido no bun 1.4.2: o inspect custom não usa a mensagem dos métodos (`Can only call ...`).
        return Err(throw_coded_type_error(global_object, "Value of \"this\" must be of type BroadcastChannel", "ERR_INVALID_THIS"));
    }
    let depth = call.argument(0);
    let label = "BroadcastChannel";
    let mut text: Vec<u16> = label.encode_utf16().collect();
    let depth = if depth.is_number() { depth.as_number() } else { f64::INFINITY };
    if depth == 0.0 {
        text.extend(" [Object]".encode_utf16());
    } else if depth > 0.0 {
        let (name, active) = with_channel(this, |channel| match &channel.route {
            Route::Name { units, .. } => (units.clone(), !channel.closed),
            Route::Port { .. } => (Vec::new(), false),
        })
        .unwrap_or_default();
        text.extend(" { name: ".encode_utf16());
        text.extend(quoted_name(&name));
        text.extend(format!(", active: {active} }}").encode_utf16());
    }
    Ok(JSValue::from_js_string(js_string(global_object.vm(), &crate::wtf::text::wtf_string::String::from_utf16(&text))))
}

fn close_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = method_this(global_object, call, "close")?;
    with_channel(this, |channel| channel.closed = true);
    Ok(js_undefined())
}

fn ref_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = method_this(global_object, call, "ref")?;
    set_refed(this, true);
    Ok(js_undefined())
}

fn unref_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = method_this(global_object, call, "unref")?;
    set_refed(this, false);
    Ok(this)
}

/// `true` quando algum canal aberto com `ref` segura o processo vivo (medido no bun 1.4.2: mesmo sem `onmessage`).
pub(crate) fn holds_event_loop() -> bool {
    CHANNELS.with(|channels| channels.borrow().values().any(|channel| matches!(channel.route, Route::Name { .. }) && !channel.closed && channel.refed))
}

fn set_refed(channel: JSValue, refed: bool) {
    with_channel(channel, |state| state.refed = refed);
}

/// A tarefa do host que entrega a fila (chamada por `timers.rs`, junto da dos `PerformanceObserver`): cada mensagem
/// vira um `MessageEvent` despachado no destinatário que ainda está aberto, e as microtasks esvaziam depois de cada uma.
/// O que os ouvintes enviam durante a entrega entra no fim da fila e é entregue na mesma chamada. Uma mensagem para
/// uma `MessagePort` sem ouvinte de `message` espera (na ordem) até a porta ter um, numa chamada futura.
pub(crate) fn deliver_pending(global_object: &JSGlobalObject) {
    // Medido no bun 1.4.2: o `close` da porta fechada sai primeiro; o da porta par (se ainda aberta, e ela passa a
    // fechada) vem na volta seguinte, na ordem de criação das portas, depois das mensagens da própria porta; tudo antes
    // de qualquer timer.
    let closed: Vec<EncodedJSValue> = CLOSED_PORTS.with(|closed| closed.borrow_mut().drain(..).collect());
    let mut peers: Vec<(u64, EncodedJSValue)> = Vec::new();
    for key in closed {
        dispatch_close(global_object, JSValue::decode(key));
        if let Some(peer) = port_peer(key) {
            if let Some(serial) = with_channel(JSValue::decode(peer), |channel| channel.serial) {
                peers.push((serial, peer));
            }
        }
    }
    peers.sort();
    peers.dedup();
    deliver_messages(global_object, &mut peers);
    close_peers_below(global_object, &mut peers, u64::MAX);
}

/// Despacha o `close` das portas pares de `peers` de ordem de criação menor que `below` que ainda estão abertas, e
/// as marca fechadas.
fn close_peers_below(global_object: &JSGlobalObject, peers: &mut Vec<(u64, EncodedJSValue)>, below: u64) {
    let due = peers.iter().take_while(|(serial, _)| *serial < below).count();
    for (_, peer) in peers.drain(..due).collect::<Vec<_>>() {
        if with_channel(JSValue::decode(peer), |channel| !std::mem::replace(&mut channel.closed, true)).unwrap_or(false) {
            dispatch_close(global_object, JSValue::decode(peer));
        }
    }
}

fn deliver_messages(global_object: &JSGlobalObject, peers: &mut Vec<(u64, EncodedJSValue)>) {
    // As entregas tiradas da fila (o lote, as que esperam ouvinte) vivem só nestes locais enquanto os ouvintes rodam JS.
    let marked = crate::runtime::gc::RootScope::new(&[]);
    let mark = |delivery: &Delivery| {
        marked.append(JSValue::decode(delivery.key));
        marked.append(delivery.data);
        delivery.ports.iter().for_each(|port| marked.append(*port));
    };
    let mut waiting: Vec<Delivery> = Vec::new();
    let mut delivered = 0;
    while delivered < MAX_DELIVERIES_PER_CALL {
        let Some(delivery) = QUEUE.with(|queue| queue.borrow_mut().pop_front()) else { break };
        mark(&delivery);
        let Some((open, is_port, serial)) = with_channel(JSValue::decode(delivery.key), |channel| (!channel.closed, matches!(channel.route, Route::Port { .. }), channel.serial)) else { continue };
        close_peers_below(global_object, peers, serial);
        if !open {
            continue;
        }
        let target = JSValue::decode(delivery.key);
        if is_port && !target_has_listener(target, "message") {
            waiting.push(delivery);
            continue;
        }
        // Medido no bun 1.4.2: a porta entrega numa tarefa só tudo o que já está na fila dela, na ordem (o que esperava
        // um ouvinte vem antes), mesmo que um ouvinte a feche no meio; outra porta com mensagem anterior a uma destas
        // espera o lote acabar. O que chega durante o lote fica para um lote seguinte.
        let batch: Vec<Delivery> = if is_port {
            let key = delivery.key;
            let mut batch: Vec<Delivery> = Vec::new();
            let mut index = 0;
            while index < waiting.len() {
                if waiting[index].key == key {
                    batch.push(waiting.remove(index));
                } else {
                    index += 1;
                }
            }
            batch.push(delivery);
            QUEUE.with(|queue| {
                let mut queue = queue.borrow_mut();
                let (same, other): (VecDeque<Delivery>, VecDeque<Delivery>) = queue.drain(..).partition(|queued| queued.key == key);
                *queue = other;
                batch.extend(same);
            });
            batch.iter().for_each(mark);
            batch
        } else {
            vec![delivery]
        };
        for delivery in batch {
            delivered += 1;
            if let Ok(event) = create_message_event(global_object, delivery.data, &delivery.ports) {
                // A exceção de um ouvinte já foi para o relatório de erros não capturados dentro de `dispatch_event`.
                let _ = dispatch_event(global_object, target, event);
            }
            global_object.vm().drain_microtasks();
        }
    }
    QUEUE.with(|queue| {
        let mut queue = queue.borrow_mut();
        for delivery in waiting.into_iter().rev() {
            queue.push_front(delivery);
        }
    });
}

host_function!(call_broadcast_channel, illegal_call_body);
host_function!(construct_broadcast_channel, construct_body);
host_function!(channel_name, name_body);
host_function!(channel_onmessage, onmessage_body);
host_function!(channel_set_onmessage, set_onmessage_body);
host_function!(channel_onmessageerror, onmessageerror_body);
host_function!(channel_set_onmessageerror, set_onmessageerror_body);
host_function!(channel_post_message, post_message_body);
host_function!(channel_close, close_body);
host_function!(channel_inspect, inspect_body);
host_function!(channel_ref, ref_body);
host_function!(channel_unref, unref_body);

/// Instala `BroadcastChannel` no global; `event_target` é o par (protótipo, construtor) do `EventTarget`, de que ele
/// herda. A posição no global vem da tabela `ORDER`.
pub fn install_broadcast_channel(global_object: &JSGlobalObject, event_target: (JSValue, JSValue)) {
    let vm = global_object.vm();
    let (prototype, constructor) = create_native_subclass(
        global_object,
        event_target,
        &BROADCAST_CHANNEL_PROTOTYPE_S_INFO,
        &CONSTRUCTOR_S_INFO,
        "BroadcastChannel",
        1,
        call_broadcast_channel,
        construct_broadcast_channel,
    );
    prototype.put_direct(vm, &crate::runtime::property_name::PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);
    put_native_accessor(vm, global_object, &prototype, "name", channel_name, None, 0);
    put_native_accessor(vm, global_object, &prototype, "onmessage", channel_onmessage, Some(channel_set_onmessage), 0);
    put_native_accessor(vm, global_object, &prototype, "onmessageerror", channel_onmessageerror, Some(channel_set_onmessageerror), 0);
    put_methods(
        global_object,
        &prototype,
        &[
            ("postMessage", 1, channel_post_message as NativeFunction),
            ("close", 0, channel_close),
            ("ref", 0, channel_ref),
            ("unref", 0, channel_unref),
        ],
    );
    put_to_string_tag(vm, &prototype, "BroadcastChannel");
    crate::runtime::streams::put_inspect_custom_with(global_object, &prototype, channel_inspect);
    install_global(global_object, "BroadcastChannel", constructor.as_value());
}
