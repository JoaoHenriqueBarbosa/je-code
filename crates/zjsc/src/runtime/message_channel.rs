//! `MessageChannel` e `MessagePort` do global. O JavaScriptCore não os define: quem os instala é o bun (WebCore), como
//! propriedades de dados comuns. Medido no bun 1.4.2 (`scripts/gen-message-channel-golden.js`):
//!
//! - `MessageChannel` (`length` 0) herda de `Function.prototype`, o protótipo tem `constructor` e os acessores `port1` e
//!   `port2` (só getter, enumeráveis); sem `new`: ``Use `new MessageChannel(...)` instead of `MessageChannel(...)` ``
//!   (`ERR_CONSTRUCT_CALL_REQUIRED`); `MessagePort` herda de `EventTarget` e não constrói (`Constructor cannot be called`,
//!   `ERR_CONSTRUCT_CALL_INVALID`, com ou sem `new`). O protótipo tem, nesta ordem, `constructor`, os acessores
//!   `onmessage` e `onmessageerror` e os métodos `postMessage` (`length` 1), `start`, `close`, `ref`, `unref`, `hasRef`
//!   (todos enumeráveis); brand check como o do `BroadcastChannel`;
//! - o par nasce emparelhado; `postMessage` clona (uma cópia, `DataCloneError` se não clonável) e a entrega é uma tarefa
//!   do host (a do `BroadcastChannel`, em `broadcast_channel.rs`): depois das microtasks e antes de `setImmediate` e dos
//!   timers. A mensagem para uma porta sem ouvinte de `message` espera até ela ter um (`onmessage` ou
//!   `addEventListener`; `start()` não é necessário e não muda nada). Fechar a porta que enviou não cancela o que já saiu;
//!   fechar a que recebe descarta; `postMessage`, `start` e `close` numa porta fechada não fazem nada. A porta entrega em
//!   lote tudo o que está na fila dela (`m1, n2, m3` sai `m1, m3, n2`), e fechá-la dentro do lote não corta o resto;
//! - `postMessage(valor, transfer)`: `transfer` é uma lista (iterável) ou `{ transfer }`; aceita `MessagePort` (a porta
//!   original fica destacada e muda, `hasRef()` falso; a nova chega em `event.ports`, ligada ao mesmo par, e herda as
//!   mensagens que já estavam a caminho) e `ArrayBuffer` (destacado no remetente); erros em `DataCloneError`;
//! - o `MessageEvent` entregue é como o do `BroadcastChannel` (`isTrusted`, `source` `null`, `ports` congelado);
//! - `hasRef()` é falso ao nascer, `ref()` o liga e `unref()` o desliga (todos devolvem `undefined`). O laço (medido numa
//!   grade de 70 programas, `timeout 2 bun`): uma porta aberta e sem `unref` o segura em dois casos. (A) tem ouvinte de
//!   `message` (`onmessage` ou `addEventListener`, `start` irrelevante) e o par está aberto e também tem ouvinte;
//!   `unref` numa só das duas não solta (a outra segura), nas duas solta. (B) `ref()` foi chamado e ela não tem ouvinte
//!   próprio (com ouvinte próprio, só vale (A)); fechar a própria porta solta, fechar o par não. Mensagem postada,
//!   `hasRef()` e uma porta só com ouvinte não seguram (conferido com `timeout 2 bun`: `b.hasRef();` sozinho e
//!   `b.hasRef(); b.onmessage = f;` terminam com 0; a leitura não liga o `ref`, só `ref()` liga). A grade
//!   (`gen-message-channel-loop-golden.js`) roda sem timer que feche as portas e registra, por programa, a saída, o código
//!   e se o processo ainda estava vivo ao fim do limite; o lado Rust observa o mesmo com `evaluate_main_script_reporting_hold`. Um ouvinte removido ou `onmessage = null` deixa de contar.
//! - ainda sem medir/portar: porta dentro do próprio dado clonado (aninhada), `messageerror`.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::host_function;
use crate::runtime::broadcast_channel::{checked_accessor_this, checked_method_this, enqueue, for_each_channel, register_channel, retarget, with_channel, Route};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{derived_structure, put_to_string_tag};
use crate::runtime::event_target::{event_handler, put_methods, register_target, set_event_handler, target_has_listener};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO;
use crate::runtime::iterator_operations::get_value_property;
use crate::runtime::js_dom_exception::throw_dom_exception_from_host;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_array_buffer::JSArrayBuffer;
use crate::runtime::js_object::{JSFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_value::{js_boolean, js_undefined, EncodedJSValue, JSValue};
use crate::runtime::native_class_support::{
    create_native_class, create_native_subclass, install_global, instance_structure, property_key, put_native_accessor, throw_coded_type_error,
};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::StructureRef;
use crate::runtime::structured_clone::{clone_value_with_ports, is_object_cell, push_transfer_buffer, transfer_items};

static CHANNEL_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "MessageChannel", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static PORT_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "MessagePort", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

thread_local! {
    /// A `Structure` das instâncias de `MessagePort` de cada realm (chave: `cell_id`), para as portas que o código nativo cria.
    static PORT_STRUCTURES: RefCell<Vec<(usize, StructureRef)>> = const { RefCell::new(Vec::new()) };
    /// Os `MessageChannel` do programa (valor codificado) e o par de portas de cada um.
    static CHANNEL_PORTS: RefCell<HashMap<EncodedJSValue, (JSValue, JSValue)>> = RefCell::new(HashMap::new());
}

/// Fim do programa (`cell_registry::reset_program_state`): as portas e as estruturas são do programa.
/// As raízes do coletor: cada canal e as duas portas dele.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    CHANNEL_PORTS.with(|channels| {
        for (channel, (first, second)) in channels.borrow().iter() {
            marker.encoded(*channel);
            marker.values(&[*first, *second]);
        }
    });
}

pub(crate) fn reset_for_program() {
    let _ = PORT_STRUCTURES.try_with(|structures| structures.borrow_mut().clear());
    let channels = CHANNEL_PORTS.try_with(|channels| std::mem::take(&mut *channels.borrow_mut()));
    drop(channels);
}

/// `true` quando `value` é uma `MessagePort`.
pub(crate) fn is_port(value: JSValue) -> bool {
    with_channel(value, |channel| matches!(channel.route, Route::Port { .. })).unwrap_or(false)
}

fn is_message_channel(value: JSValue) -> bool {
    CHANNEL_PORTS.with(|channels| channels.borrow().contains_key(&value.encode()))
}

/// Uma porta nova (aberta, sem ouvinte), ligada a `peer`.
fn create_port(global_object: &JSGlobalObject, peer: Option<EncodedJSValue>) -> Result<JSValue, Thrown> {
    let realm = global_object.cell_id();
    let structure = PORT_STRUCTURES.with(|structures| structures.borrow().iter().find(|(id, _)| *id == realm).map(|(_, structure)| structure.clone()));
    let Some(structure) = structure else { return Err(Thrown::Unported("MessagePort sem instalação no realm")) };
    let port = JSFinalObject::create(global_object.vm(), &structure).as_value();
    register_target(port);
    register_channel(port, Route::Port { peer, activated: false, unrefed: false }, false);
    Ok(port)
}

fn port_this(global_object: &JSGlobalObject, call: &HostCall, name: &str) -> Result<JSValue, Thrown> {
    checked_method_this(global_object, call, "MessagePort", name, is_port)
}

fn port_accessor_this(global_object: &JSGlobalObject, call: &HostCall, what: &str, name: &str) -> Result<JSValue, Thrown> {
    checked_accessor_this(global_object, call, "MessagePort", what, name, is_port)
}

fn channel_accessor_this(global_object: &JSGlobalObject, call: &HostCall, name: &str) -> Result<JSValue, Thrown> {
    checked_accessor_this(global_object, call, "MessageChannel", "getter", name, is_message_channel)
}

fn call_channel_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(throw_coded_type_error(global_object, "Use `new MessageChannel(...)` instead of `MessageChannel(...)`", "ERR_CONSTRUCT_CALL_REQUIRED"))
}

fn illegal_port_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(throw_coded_type_error(global_object, "Constructor cannot be called", "ERR_CONSTRUCT_CALL_INVALID"))
}

fn construct_channel_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let structure = derived_structure(global_object, call, instance_structure)?;
    let instance = JSFinalObject::create(global_object.vm(), &structure).as_value();
    let port1 = create_port(global_object, None)?;
    let port2 = create_port(global_object, Some(port1.encode()))?;
    with_channel(port1, |channel| channel.route = Route::Port { peer: Some(port2.encode()), activated: false, unrefed: false });
    CHANNEL_PORTS.with(|channels| channels.borrow_mut().insert(instance.encode(), (port1, port2)));
    Ok(instance)
}

fn port1_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = channel_accessor_this(global_object, call, "port1")?;
    Ok(CHANNEL_PORTS.with(|channels| channels.borrow().get(&this.encode()).map_or_else(js_undefined, |ports| ports.0)))
}

fn port2_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = channel_accessor_this(global_object, call, "port2")?;
    Ok(CHANNEL_PORTS.with(|channels| channels.borrow().get(&this.encode()).map_or_else(js_undefined, |ports| ports.1)))
}

fn onmessage_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(event_handler(port_accessor_this(global_object, call, "getter", "onmessage")?, "message"))
}

fn set_onmessage_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    set_event_handler(port_accessor_this(global_object, call, "setter", "onmessage")?, "message", call.argument(0));
    Ok(js_undefined())
}

fn onmessageerror_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(event_handler(port_accessor_this(global_object, call, "getter", "onmessageerror")?, "messageerror"))
}

fn set_onmessageerror_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    set_event_handler(port_accessor_this(global_object, call, "setter", "onmessageerror")?, "messageerror", call.argument(0));
    Ok(js_undefined())
}

/// Os elementos do 2º argumento de `postMessage`: uma lista iterável ou `{ transfer }`.
fn transfer_argument(global_object: &JSGlobalObject, argument: JSValue) -> Result<Vec<JSValue>, Thrown> {
    const LIST_MESSAGE: &str = "Optional transferList argument must be an iterable";
    if argument.is_undefined_or_null() {
        return Ok(Vec::new());
    }
    if !is_object_cell(&argument) {
        return Err(throw_coded_type_error(global_object, LIST_MESSAGE, "ERR_INVALID_ARG_TYPE"));
    }
    let vm = global_object.vm();
    let iterator = get_value_property(global_object, argument, &PropertyName::from_identifier(&vm.property_names.iterator_symbol))?;
    if iterator.is_callable() {
        return transfer_items(global_object, argument, LIST_MESSAGE);
    }
    let list = get_value_property(global_object, argument, &property_key(vm, "transfer"))?;
    if list.is_undefined() {
        Ok(Vec::new())
    } else {
        transfer_items(global_object, list, "Optional options.transfer argument must be an iterable")
    }
}

/// `true` quando a porta foi transferida (destacada) ou fechada: nenhuma das duas pode ir numa lista de transferência.
fn is_detached(port: JSValue) -> bool {
    with_channel(port, |channel| channel.closed || matches!(channel.route, Route::Port { peer: None, .. })).unwrap_or(false)
}

/// Valida `port` como porta a transferir (sem repetição, nem destacada nem fechada) e a acrescenta a `ports`.
pub(crate) fn push_transfer_port(global_object: &JSGlobalObject, call: &HostCall, port: JSValue, ports: &mut Vec<JSValue>) -> Result<(), Thrown> {
    let clone_error = |message: &str| throw_dom_exception_from_host(global_object, call, "DataCloneError", message);
    if ports.iter().any(|seen| seen.encode() == port.encode()) {
        return Err(clone_error("Transfer list contains duplicate MessagePort"));
    }
    if is_detached(port) {
        return Err(clone_error("MessagePort in transfer list is already detached"));
    }
    ports.push(port);
    Ok(())
}

/// Move o estado de `old` para `new` (uma porta recém-criada): mesmo par, e o que estava a caminho de `old` passa a ir
/// para ela.
fn commit_port_transfer(old: JSValue, new: JSValue) {
    let peer = with_channel(old, |channel| match &mut channel.route {
        Route::Port { peer, .. } => {
            channel.closed = true;
            peer.take()
        }
        Route::Name { .. } => None,
    })
    .flatten();
    with_channel(new, |channel| {
        if let Route::Port { peer: slot, .. } = &mut channel.route {
            *slot = peer;
        }
    });
    if let Some(peer) = peer {
        with_channel(JSValue::decode(peer), |channel| {
            if let Route::Port { peer, .. } = &mut channel.route {
                *peer = Some(new.encode());
            }
        });
    }
    retarget(old.encode(), new.encode());
}

fn post_message_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = port_this(global_object, call, "postMessage")?;
    if call.argument_count() < 1 {
        return Err(throw_coded_type_error(global_object, "Not enough arguments", "ERR_MISSING_ARGS"));
    }
    let clone_error = |message: &str| throw_dom_exception_from_host(global_object, call, "DataCloneError", message);
    let mut ports: Vec<JSValue> = Vec::new();
    let mut buffers = Vec::new();
    for item in transfer_argument(global_object, call.argument(1))? {
        if is_port(item) {
            if item.encode() == this.encode() {
                return Err(clone_error("Transfer list contains source port"));
            }
            push_transfer_port(global_object, call, item, &mut ports)?;
        } else if is_object_cell(&item) && JSArrayBuffer::from_value(&item).is_some() {
            push_transfer_buffer(global_object, call, &item, &mut buffers)?;
        } else {
            return Err(clone_error("Found invalid value in transferList."));
        }
    }
    let (closed, peer) = with_channel(this, |channel| match &channel.route {
        Route::Port { peer, .. } => (channel.closed, *peer),
        Route::Name { .. } => (true, None),
    })
    .unwrap_or((true, None));
    // As portas novas nascem antes do clone, que as põe no lugar das originais em todo o dado; se o clone falha ou a
    // porta que envia está fechada, elas ficam fechadas e as originais intactas.
    let mut pairs: Vec<(JSValue, JSValue)> = Vec::with_capacity(ports.len());
    for port in &ports {
        pairs.push((*port, create_port(global_object, None)?));
    }
    let discard = |pairs: &[(JSValue, JSValue)]| {
        for (_, new) in pairs {
            with_channel(*new, |channel| channel.closed = true);
        }
    };
    let cloned = match clone_value_with_ports(global_object, call, call.argument(0), &pairs) {
        Ok(cloned) => cloned,
        Err(error) => {
            discard(&pairs);
            return Err(error);
        }
    };
    if closed {
        discard(&pairs);
        return Ok(js_undefined());
    }
    for buffer in &buffers {
        buffer.detach();
    }
    for (old, new) in &pairs {
        commit_port_transfer(*old, *new);
    }
    if let Some(peer) = peer {
        enqueue(peer, cloned, pairs.iter().map(|(_, new)| *new).collect());
    }
    Ok(js_undefined())
}


fn start_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    port_this(global_object, call, "start")?;
    Ok(js_undefined())
}

fn close_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = port_this(global_object, call, "close")?;
    // Só a primeira chamada dispara o evento `close`; uma porta já transferida (destacada) não tem par nem evento.
    // Fechar a própria porta solta o `ref()` dela (regra B do cabeçalho); o fechamento que vem do par não solta.
    let first_close = with_channel(this, |channel| {
        if let Route::Port { activated, .. } = &mut channel.route {
            *activated = false;
        }
        !std::mem::replace(&mut channel.closed, true)
    })
    .unwrap_or(false);
    if first_close {
        crate::runtime::broadcast_channel::enqueue_port_close(this.encode());
    }
    Ok(js_undefined())
}

fn ref_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = port_this(global_object, call, "ref")?;
    with_channel(this, |channel| {
        channel.refed = true;
        let open = !channel.closed;
        if let Route::Port { activated, unrefed, .. } = &mut channel.route {
            // Numa porta já fechada, o `ref()` não volta a segurar o laço.
            *activated = open;
            *unrefed = false;
        }
    });
    Ok(js_undefined())
}

fn unref_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = port_this(global_object, call, "unref")?;
    with_channel(this, |channel| {
        channel.refed = false;
        if let Route::Port { unrefed, .. } = &mut channel.route {
            *unrefed = true;
        }
    });
    Ok(js_undefined())
}

fn has_ref_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = port_this(global_object, call, "hasRef")?;
    Ok(js_boolean(with_channel(this, |channel| !channel.closed && channel.refed).unwrap_or(false)))
}

/// `true` quando algum par de portas abertas segura o processo vivo (ver o cabeçalho do módulo).
pub(crate) fn holds_event_loop() -> bool {
    let mut candidates: Vec<(EncodedJSValue, Option<EncodedJSValue>, bool, bool)> = Vec::new();
    for_each_channel(|key, channel| {
        if let Route::Port { peer, activated, unrefed: false } = &channel.route {
            candidates.push((key, *peer, *activated, channel.closed));
        }
    });
    candidates.into_iter().any(|(key, peer, ref_called, closed)| {
        let has_listener = target_has_listener(JSValue::decode(key), "message");
        // Regra B: `ref()` explícito sem ouvinte próprio a segura, mesmo depois que o par fechou e o fechamento chegou
        // a ela (medido no bun 1.4.2: `b.ref(); a.close();` não termina); o `close()` da própria porta já soltou o `ref`.
        if ref_called && !has_listener {
            return true;
        }
        // Regra A: porta aberta com ouvinte cujo par está aberto e também tem ouvinte.
        !closed
            && has_listener
            && peer.is_some_and(|peer| {
                with_channel(JSValue::decode(peer), |channel| !channel.closed).unwrap_or(false) && target_has_listener(JSValue::decode(peer), "message")
            })
    })
}

host_function!(call_message_channel, call_channel_body);
host_function!(construct_message_channel, construct_channel_body);
host_function!(call_message_port, illegal_port_body);
host_function!(channel_port1, port1_body);
host_function!(channel_port2, port2_body);
host_function!(port_onmessage, onmessage_body);
host_function!(port_set_onmessage, set_onmessage_body);
host_function!(port_onmessageerror, onmessageerror_body);
host_function!(port_set_onmessageerror, set_onmessageerror_body);
host_function!(port_post_message, post_message_body);
host_function!(port_start, start_body);
host_function!(port_close, close_body);
host_function!(port_ref, ref_body);
host_function!(port_unref, unref_body);
host_function!(port_has_ref, has_ref_body);

/// Instala `MessageChannel` e `MessagePort` no global; `event_target` é o par (protótipo, construtor) do `EventTarget`,
/// de que `MessagePort` herda. A posição no global vem da tabela `ORDER`.
pub fn install_message_channel(global_object: &JSGlobalObject, event_target: (JSValue, JSValue)) {
    let vm = global_object.vm();
    let constructor_key = PropertyName::from_identifier(&vm.property_names.constructor);

    let (port_prototype, port_constructor) =
        create_native_subclass(global_object, event_target, &PORT_PROTOTYPE_S_INFO, &CONSTRUCTOR_S_INFO, "MessagePort", 0, call_message_port, call_message_port);
    port_prototype.put_direct(vm, &constructor_key, port_constructor.as_value(), DONT_ENUM);
    put_native_accessor(vm, global_object, &port_prototype, "onmessage", port_onmessage, Some(port_set_onmessage), 0);
    put_native_accessor(vm, global_object, &port_prototype, "onmessageerror", port_onmessageerror, Some(port_set_onmessageerror), 0);
    put_methods(
        global_object,
        &port_prototype,
        &[
            ("postMessage", 1, port_post_message as NativeFunction),
            ("start", 0, port_start),
            ("close", 0, port_close),
            ("ref", 0, port_ref),
            ("unref", 0, port_unref),
            ("hasRef", 0, port_has_ref),
        ],
    );
    put_to_string_tag(vm, &port_prototype, "MessagePort");
    let port_structure = instance_structure(vm, Some(global_object), port_prototype.as_value());
    PORT_STRUCTURES.with(|structures| structures.borrow_mut().push((global_object.cell_id(), port_structure)));
    install_global(global_object, "MessagePort", port_constructor.as_value());

    let (channel_prototype, channel_constructor) =
        create_native_class(global_object, &CHANNEL_PROTOTYPE_S_INFO, &CONSTRUCTOR_S_INFO, "MessageChannel", call_message_channel, construct_message_channel);
    channel_prototype.put_direct(vm, &constructor_key, channel_constructor.as_value(), DONT_ENUM);
    put_native_accessor(vm, global_object, &channel_prototype, "port1", channel_port1, None, 0);
    put_native_accessor(vm, global_object, &channel_prototype, "port2", channel_port2, None, 0);
    put_to_string_tag(vm, &channel_prototype, "MessageChannel");
    install_global(global_object, "MessageChannel", channel_constructor.as_value());
}
