//! `WritableStream`, `WritableStreamDefaultWriter` e `WritableStreamDefaultController` na camada JS, portados de
//! `JSWritableStream.cpp`, `JSWritableStreamDefaultWriter.cpp` e `JSWritableStreamDefaultController.cpp` do bun.
//!
//! A máquina de estados é o `Core<JSValue>` de [`super::writable`]: as promessas viram ids numa arena
//! (`Stream::promises`) e tudo que o C++ faz no mundo JS sai como `Effect`. [`drain`] é a ÚNICA função que aplica os
//! efeitos (o `TransformStream` usa a mesma, em [`super::effects`]). As reações de promessa são as de `readable.rs`
//! ([`reaction`], fechos num registro por função), e o registro de objetos é o mesmo molde: valor da célula para
//! `Rc<RefCell<..>>`, zerado em `reset_for_program`. Nenhum empréstimo atravessa uma chamada a JS: o `Core` é mexido
//! num bloco curto, os efeitos saem do `Core` e só então rodam.
//!
//! DIVERGÊNCIAS: `pipeTo`/`pipeThrough` não usam isto ainda.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::effects::{drain, promise_of, EffectState, PromiseOp, Promises};
use super::trace::{mark_all, Traced};
use super::readable::{
    call_js, callable_member, extract_high_water_mark, invalid_state, invalid_this, invoke_returning_promise, prototype_of, range_error_value, reaction, rejected_with,
    resolved_with, rethrow, take_error, text_value, then, type_error_value,
};
use super::writable::{Core, Effect, State, WriteOutcome, P};
use crate::runtime::abort_signal::{abort_with_reason, new_signal};
use crate::runtime::collection_support::derived_structure;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::intl_support::{get_property, to_number_checked};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSFinalObject;
use crate::runtime::js_value::{js_number, EncodedJSValue, JSValue};
use crate::runtime::native_class_support::{instance_structure, throw_native_type_error};
use crate::runtime::object_to_primitive::call_function;
use crate::runtime::structure::StructureRef;

struct Stream {
    core: Core<JSValue>,
    promises: Promises,
    /// `SinkAlgorithmSlots`: `None` é o trio de algoritmos trivial (depois de `ClearAlgorithms`).
    algorithms: Option<SinkAlgorithms>,
    size: Option<JSValue>,
    controller: JSValue,
    signal: JSValue,
}

/// Um sink nativo (`SinkKind::Transform` e afins): `write`, `close` e `abort` rodam em Rust e devolvem a promessa do
/// algoritmo (uma promessa JS; `resolved_with` embrulha o que não for).
pub(super) trait NativeSink {
    fn write(&self, global_object: &JSGlobalObject, chunk: JSValue) -> JSValue;
    fn close(&self, global_object: &JSGlobalObject) -> JSValue;
    fn abort(&self, global_object: &JSGlobalObject, reason: JSValue) -> JSValue;
    /// Os valores JS que o sink segura além do que está num `OBJECTS` (o `visitChildren` dele).
    fn trace(&self, out: &mut Vec<JSValue>);
}

/// Qual braço roda `write`/`close`/`abort`: o `underlyingSink` do usuário ou um sink nativo.
#[derive(Clone)]
enum SinkAlgorithms {
    JavaScript { sink: JSValue, write: Option<JSValue>, close: Option<JSValue>, abort: Option<JSValue> },
    Native(Rc<dyn NativeSink>),
}

impl SinkAlgorithms {
    fn run_write(&self, global_object: &JSGlobalObject, chunk: JSValue, controller: JSValue) -> JSValue {
        match self {
            SinkAlgorithms::JavaScript { sink, write: Some(function), .. } => invoke_returning_promise(global_object, *function, *sink, &[chunk, controller]),
            SinkAlgorithms::JavaScript { .. } => resolved_with(global_object, JSValue::undefined()),
            SinkAlgorithms::Native(native) => resolved_with(global_object, native.write(global_object, chunk)),
        }
    }

    fn run_close(&self, global_object: &JSGlobalObject) -> JSValue {
        match self {
            SinkAlgorithms::JavaScript { sink, close: Some(function), .. } => invoke_returning_promise(global_object, *function, *sink, &[]),
            SinkAlgorithms::JavaScript { .. } => resolved_with(global_object, JSValue::undefined()),
            SinkAlgorithms::Native(native) => resolved_with(global_object, native.close(global_object)),
        }
    }

    fn run_abort(&self, global_object: &JSGlobalObject, reason: JSValue) -> JSValue {
        match self {
            SinkAlgorithms::JavaScript { sink, abort: Some(function), .. } => invoke_returning_promise(global_object, *function, *sink, &[reason]),
            SinkAlgorithms::JavaScript { .. } => resolved_with(global_object, JSValue::undefined()),
            SinkAlgorithms::Native(native) => resolved_with(global_object, native.abort(global_object, reason)),
        }
    }
}

struct Writer {
    stream: StreamRef,
    /// `false` depois de `releaseLock()`.
    bound: bool,
    /// `(ready, closed)` do escritor solto, já rejeitados.
    released: Option<(P, P)>,
}

type StreamRef = Rc<RefCell<Stream>>;
type WriterRef = Rc<RefCell<Writer>>;

#[derive(Clone)]
enum Obj {
    Stream(StreamRef),
    Writer(WriterRef),
    Controller(StreamRef),
}

thread_local! {
    static OBJECTS: RefCell<HashMap<EncodedJSValue, Obj>> = RefCell::new(HashMap::new());
}

impl Traced for Stream {
    fn trace(&self, out: &mut Vec<JSValue>) {
        self.core.for_each_value(|value| out.push(*value));
        self.promises.trace(out);
        self.algorithms.trace(out);
        self.size.trace(out);
        self.controller.trace(out);
        self.signal.trace(out);
    }
}

impl Traced for SinkAlgorithms {
    fn trace(&self, out: &mut Vec<JSValue>) {
        match self {
            SinkAlgorithms::JavaScript { sink, write, close, abort } => {
                sink.trace(out);
                write.trace(out);
                close.trace(out);
                abort.trace(out);
            }
            SinkAlgorithms::Native(native) => native.trace(out),
        }
    }
}

/// As raízes do coletor: os objetos gravaveis (as chaves vivem enquanto o programa existir) e o que o estado deles
/// segura. O escritor não segura valor próprio (as promessas dele moram na arena do stream).
pub(super) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    mark_all(marker, |out| {
        OBJECTS.with(|objects| {
            objects.borrow().iter().for_each(|(key, object)| {
                out.push(JSValue::decode(*key));
                match object {
                    Obj::Stream(state) | Obj::Controller(state) => state.trace(out),
                    Obj::Writer(_) => {}
                }
            })
        })
    });
}

pub(super) fn reset_for_program() {
    let _ = OBJECTS.try_with(|objects| objects.borrow_mut().clear());
}

fn lookup(value: JSValue) -> Option<Obj> {
    OBJECTS.with(|objects| objects.borrow().get(&value.encode()).cloned())
}

/// `value` é um `WritableStream`.
pub(super) fn is_stream(value: JSValue) -> bool {
    stream_of(value).is_some()
}

fn stream_of(value: JSValue) -> Option<StreamRef> {
    match lookup(value)? {
        Obj::Stream(stream) => Some(stream),
        _ => None,
    }
}

fn writer_of(value: JSValue) -> Option<WriterRef> {
    match lookup(value)? {
        Obj::Writer(writer) => Some(writer),
        _ => None,
    }
}

fn controller_stream_of(value: JSValue) -> Option<StreamRef> {
    match lookup(value)? {
        Obj::Controller(stream) => Some(stream),
        _ => None,
    }
}

fn register(value: JSValue, object: Obj) {
    OBJECTS.with(|objects| objects.borrow_mut().insert(value.encode(), object));
}

// ---------------------------------------------------------------------------------------------
// Drenagem dos efeitos (a função é a de `effects`)
// ---------------------------------------------------------------------------------------------

impl EffectState for Stream {
    type Effect = Effect<JSValue>;

    fn promises(&mut self) -> &mut Promises {
        &mut self.promises
    }

    fn take_effects(&mut self) -> Vec<Effect<JSValue>> {
        std::mem::take(&mut self.core.effects)
    }

    fn classify(effect: Effect<JSValue>) -> Result<(P, PromiseOp), Effect<JSValue>> {
        match effect {
            Effect::NewPromise(id) => Ok((id, PromiseOp::New)),
            Effect::Resolve(id) => Ok((id, PromiseOp::Resolve)),
            Effect::Reject(id, error) => Ok((id, PromiseOp::Reject(error))),
            Effect::MarkHandled(id) => Ok((id, PromiseOp::MarkHandled)),
            other => Err(other),
        }
    }

    fn apply_other(global_object: &JSGlobalObject, stream: &StreamRef, effect: Effect<JSValue>) -> Result<(), Thrown> {
        apply(global_object, stream, effect)
    }
}

/// `WritableStreamDefaultControllerClearAlgorithms`.
fn clear_algorithms(stream: &StreamRef) {
    let mut inner = stream.borrow_mut();
    inner.algorithms = None;
    inner.size = None;
}

/// Reage ao resultado de um algoritmo do sink: o `Core` recebe `Ok`/`Err` e os efeitos novos são drenados.
fn settle(global_object: &JSGlobalObject, stream: &StreamRef, promise: JSValue, done: fn(&mut Core<JSValue>, Result<(), JSValue>)) {
    let for_fulfilled = stream.clone();
    let on_fulfilled = reaction(global_object, move |global_object, _| {
        done(&mut for_fulfilled.borrow_mut().core, Ok(()));
        drain(global_object, &for_fulfilled)
    });
    let for_rejected = stream.clone();
    let on_rejected = reaction(global_object, move |global_object, error| {
        done(&mut for_rejected.borrow_mut().core, Err(error));
        drain(global_object, &for_rejected)
    });
    then(global_object, promise, on_fulfilled, on_rejected, JSValue::undefined());
}

fn apply(global_object: &JSGlobalObject, stream: &StreamRef, effect: Effect<JSValue>) -> Result<(), Thrown> {
    match effect {
        Effect::NewPromise(_) | Effect::Resolve(_) | Effect::Reject(..) | Effect::MarkHandled(_) => unreachable!("efeito de promessa é de `effects::drain`"),
        Effect::SinkWrite(chunk) => {
            let (algorithms, controller) = {
                let inner = stream.borrow();
                (inner.algorithms.clone(), inner.controller)
            };
            let result = match algorithms {
                Some(algorithms) => algorithms.run_write(global_object, chunk, controller),
                None => resolved_with(global_object, JSValue::undefined()),
            };
            settle(global_object, stream, result, Core::on_write_done);
        }
        Effect::SinkClose => {
            let algorithms = stream.borrow().algorithms.clone();
            let result = match algorithms {
                Some(algorithms) => algorithms.run_close(global_object),
                None => resolved_with(global_object, JSValue::undefined()),
            };
            clear_algorithms(stream);
            settle(global_object, stream, result, Core::on_close_done);
        }
        Effect::SinkAbort(reason) => {
            let algorithms = stream.borrow().algorithms.clone();
            let result = match algorithms {
                Some(algorithms) => algorithms.run_abort(global_object, reason),
                None => resolved_with(global_object, JSValue::undefined()),
            };
            clear_algorithms(stream);
            settle(global_object, stream, result, Core::on_abort_steps_done);
        }
        Effect::AbortSignal(reason) => {
            let signal = stream.borrow().signal;
            abort_with_reason(global_object, signal, reason)?;
        }
        // O `Core` já zerou a fila; os algoritmos do sink continuam até o `abort` ou o `close` rodar.
        Effect::ErrorSteps => {}
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Construtor
// ---------------------------------------------------------------------------------------------

struct SinkDict {
    start: Option<JSValue>,
    write: Option<JSValue>,
    close: Option<JSValue>,
    abort: Option<JSValue>,
    has_type: bool,
}

/// `convertUnderlyingSinkDict`: a ordem de leitura das chaves é a do C++.
fn convert_sink(global_object: &JSGlobalObject, underlying: JSValue) -> Result<SinkDict, Thrown> {
    let mut dict = SinkDict { start: None, write: None, close: None, abort: None, has_type: false };
    if underlying.is_undefined_or_null() {
        return Ok(dict);
    }
    dict.abort = callable_member(global_object, underlying, "underlying sink", "abort")?;
    dict.close = callable_member(global_object, underlying, "underlying sink", "close")?;
    dict.start = callable_member(global_object, underlying, "underlying sink", "start")?;
    dict.has_type = !get_property(global_object, underlying, "type")?.is_undefined();
    dict.write = callable_member(global_object, underlying, "underlying sink", "write")?;
    Ok(dict)
}

/// `convertQueuingStrategyDict`: `(highWaterMark, size)`.
pub(super) fn convert_strategy(global_object: &JSGlobalObject, strategy: JSValue) -> Result<(Option<f64>, Option<JSValue>), Thrown> {
    if strategy.is_undefined_or_null() {
        return Ok((None, None));
    }
    if !strategy.is_object() {
        return Err(throw_native_type_error(global_object, "The queuing strategy must be an object"));
    }
    let raw = get_property(global_object, strategy, "highWaterMark")?;
    let high_water_mark = if raw.is_undefined() { None } else { Some(to_number_checked(global_object, raw)?) };
    let size = callable_member(global_object, strategy, "queuing strategy", "size")?;
    Ok((high_water_mark, size))
}

/// `new WritableStream(underlyingSink, strategy)`.
pub(super) fn construct_stream(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let argument = call.argument(0);
    let underlying = if argument.is_undefined() {
        JSValue::null()
    } else if !argument.is_object() {
        return Err(throw_native_type_error(global_object, "WritableStream constructor takes an object as first argument"));
    } else {
        argument
    };
    let (strategy_high_water_mark, size) = convert_strategy(global_object, call.argument(1))?;
    let structure = derived_structure(global_object, call, instance_structure)?;
    let vm = global_object.vm();
    let value = JSFinalObject::create(vm, &structure).as_value();
    let dict = convert_sink(global_object, underlying)?;
    if dict.has_type {
        return Err(Thrown::range_error("The underlying sink's 'type' property is reserved and must not be present"));
    }
    let high_water_mark = extract_high_water_mark(strategy_high_water_mark, 1.0)?;
    let algorithms = SinkAlgorithms::JavaScript { sink: underlying, write: dict.write, close: dict.close, abort: dict.abort };
    let stream = build_stream(global_object, value, algorithms, high_water_mark, size)?;
    let controller = stream.borrow().controller;
    let start_result = match dict.start {
        Some(function) => call_function(global_object, function, underlying, &[controller]).ok_or(Thrown::Pending)?,
        None => JSValue::undefined(),
    };
    finish_start(global_object, stream, start_result);
    Ok(value)
}

/// Cria e registra o estado, o controlador e o sinal de um `WritableStream` cujo objeto JS é `value` (ainda sem `start`).
fn build_stream(global_object: &JSGlobalObject, value: JSValue, algorithms: SinkAlgorithms, high_water_mark: f64, size: Option<JSValue>) -> Result<StreamRef, Thrown> {
    let vm = global_object.vm();
    let controller_structure = instance_structure(vm, Some(global_object), prototype_of("WritableStreamDefaultController"));
    let controller = JSFinalObject::create(vm, &controller_structure).as_value();
    let signal = new_signal(global_object)?;
    let stream = Rc::new(RefCell::new(Stream {
        core: Core::new(high_water_mark),
        promises: Promises::new(),
        algorithms: Some(algorithms),
        size,
        controller,
        signal,
    }));
    register(value, Obj::Stream(stream.clone()));
    register(controller, Obj::Controller(stream.clone()));
    Ok(stream)
}

/// O fim de `SetUpWritableStreamDefaultController`: o resultado de `start` passa por uma promessa e uma reação.
fn finish_start(global_object: &JSGlobalObject, stream: StreamRef, start_result: JSValue) {
    let for_fulfilled = stream.clone();
    let on_fulfilled = reaction(global_object, move |global_object, _| {
        for_fulfilled.borrow_mut().core.on_start_fulfilled();
        drain(global_object, &for_fulfilled)
    });
    let for_rejected = stream;
    let on_rejected = reaction(global_object, move |global_object, error| {
        for_rejected.borrow_mut().core.on_start_rejected(error);
        drain(global_object, &for_rejected)
    });
    let start_promise = super::readable::fresh_resolved_with(global_object, start_result);
    then(global_object, start_promise, on_fulfilled, on_rejected, JSValue::undefined());
}

/// O lado do `WritableStreamDefaultController` que o Rust enxerga num stream criado por [`create_native`].
#[derive(Clone)]
pub(super) struct WritableHandle {
    stream: StreamRef,
}

impl WritableHandle {
    pub(super) fn is_writable(&self) -> bool {
        self.stream.borrow().core.state == State::Writable
    }

    pub(super) fn is_erroring(&self) -> bool {
        self.stream.borrow().core.state == State::Erroring
    }

    pub(super) fn is_errored(&self) -> bool {
        self.stream.borrow().core.state == State::Errored
    }

    pub(super) fn stored_error(&self) -> Option<JSValue> {
        self.stream.borrow().core.stored_error
    }

    /// `WritableStreamDefaultControllerErrorIfNeeded`.
    pub(super) fn error_if_needed(&self, global_object: &JSGlobalObject, error: JSValue) -> Result<(), Thrown> {
        self.stream.borrow_mut().core.controller_error(error);
        drain(global_object, &self.stream)
    }
}

/// `WritableStream::create` com sink nativo: sem objeto JS de `underlyingSink`. `start` é o resultado do algoritmo de
/// start (uma promessa ou valor; o stream só aceita escritas depois que ela cumpre). Devolve o objeto JS do stream e
/// o [`WritableHandle`].
pub(super) fn create_native(
    global_object: &JSGlobalObject,
    sink: Rc<dyn NativeSink>,
    high_water_mark: f64,
    size: Option<JSValue>,
    start: JSValue,
) -> Result<(JSValue, WritableHandle), Thrown> {
    let structure = instance_structure(global_object.vm(), Some(global_object), prototype_of("WritableStream"));
    let value = JSFinalObject::create(global_object.vm(), &structure).as_value();
    let stream = build_stream(global_object, value, SinkAlgorithms::Native(sink), high_water_mark, size)?;
    finish_start(global_object, stream.clone(), start);
    Ok((value, WritableHandle { stream }))
}

// ---------------------------------------------------------------------------------------------
// WritableStream
// ---------------------------------------------------------------------------------------------

/// Uma promessa rejeitada com `TypeError(message)` (o `this` alheio dos métodos que devolvem promessa).
fn rejected_type_error(global_object: &JSGlobalObject, message: &str) -> HostResult {
    let error = type_error_value(global_object, message);
    Ok(rejected_with(global_object, error))
}

fn rejected_invalid_state(global_object: &JSGlobalObject, message: &str) -> HostResult {
    let error = invalid_state(global_object, message);
    Ok(rejected_with(global_object, error))
}

/// `writableStreamAbort` na ordem do C++: o `AbortSignal` roda os ouvintes ANTES de o `Core` mudar de estado, e o
/// `Core` reconfere o estado depois deles.
fn abort_stream(global_object: &JSGlobalObject, stream: &StreamRef, reason: JSValue) -> HostResult {
    let settled = stream.borrow_mut().core.abort_if_settled();
    let id = match settled {
        Some(id) => id,
        None => {
            let signal = stream.borrow().signal;
            abort_with_reason(global_object, signal, reason)?;
            stream.borrow_mut().core.abort_after_signal(reason)
        }
    };
    drain(global_object, stream)?;
    Ok(promise_of(stream, id))
}

/// `writableStreamClose` mais a drenagem: a promessa do pedido de fechamento.
fn close_stream(global_object: &JSGlobalObject, stream: &StreamRef) -> HostResult {
    let closed = stream.borrow_mut().core.close();
    match closed {
        Ok(id) => {
            drain(global_object, stream)?;
            Ok(promise_of(stream, id))
        }
        Err(_) => rejected_invalid_state(global_object, "Cannot close a WritableStream that is closed or errored"),
    }
}

pub(super) fn ws_locked(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let stream = stream_of(call.this_value()).ok_or_else(|| invalid_this(global_object, "WritableStream"))?;
    let locked = stream.borrow().core.is_locked();
    Ok(JSValue::Bool(locked))
}

pub(super) fn ws_abort(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(stream) = stream_of(call.this_value()) else {
        return rejected_type_error(global_object, "WritableStream.prototype.abort can only be called on a WritableStream");
    };
    if stream.borrow().core.is_locked() {
        return rejected_invalid_state(global_object, "Cannot abort a locked WritableStream");
    }
    abort_stream(global_object, &stream, call.argument(0))
}

pub(super) fn ws_close(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(stream) = stream_of(call.this_value()) else {
        return rejected_type_error(global_object, "WritableStream.prototype.close can only be called on a WritableStream");
    };
    if stream.borrow().core.is_locked() {
        return rejected_invalid_state(global_object, "Cannot close a locked WritableStream");
    }
    if stream.borrow().core.close_queued_or_in_flight() {
        return rejected_invalid_state(global_object, "Cannot close a WritableStream that is already closing");
    }
    close_stream(global_object, &stream)
}

/// `AcquireWritableStreamDefaultWriter` com a estrutura dada.
fn create_writer(global_object: &JSGlobalObject, stream: &StreamRef, structure: &StructureRef) -> HostResult {
    let acquired = stream.borrow_mut().core.acquire_writer();
    if acquired.is_err() {
        return Err(rethrow(global_object, invalid_state(global_object, "WritableStream is locked")));
    }
    drain(global_object, stream)?;
    let value = JSFinalObject::create(global_object.vm(), structure).as_value();
    register(value, Obj::Writer(Rc::new(RefCell::new(Writer { stream: stream.clone(), bound: true, released: None }))));
    Ok(value)
}

pub(super) fn ws_get_writer(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let stream = stream_of(call.this_value()).ok_or_else(|| invalid_this(global_object, "WritableStream"))?;
    let structure = instance_structure(global_object.vm(), Some(global_object), prototype_of("WritableStreamDefaultWriter"));
    create_writer(global_object, &stream, &structure)
}

/// `new WritableStreamDefaultWriter(stream)` com um `WritableStream` válido.
pub(super) fn construct_writer(global_object: &JSGlobalObject, call: &HostCall) -> Option<HostResult> {
    let stream = stream_of(call.argument(0))?;
    Some(match derived_structure(global_object, call, instance_structure) {
        Ok(structure) => create_writer(global_object, &stream, &structure),
        Err(thrown) => Err(thrown),
    })
}

// ---------------------------------------------------------------------------------------------
// WritableStreamDefaultWriter
// ---------------------------------------------------------------------------------------------

const NOT_BOUND: &str = "Writer is not bound to a WritableStream";

pub(super) fn wr_closed(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(writer) = writer_of(call.this_value()) else {
        return rejected_type_error(global_object, "The 'closed' getter can only be used on a WritableStreamDefaultWriter");
    };
    let (stream, released) = {
        let inner = writer.borrow();
        (inner.stream.clone(), inner.released)
    };
    let id = match released {
        Some((_, closed)) => closed,
        None => stream.borrow().core.writer_closed().expect("escritor preso"),
    };
    Ok(promise_of(&stream, id))
}

pub(super) fn wr_ready(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(writer) = writer_of(call.this_value()) else {
        return rejected_type_error(global_object, "The 'ready' getter can only be used on a WritableStreamDefaultWriter");
    };
    let (stream, released) = {
        let inner = writer.borrow();
        (inner.stream.clone(), inner.released)
    };
    let id = match released {
        Some((ready, _)) => ready,
        None => {
            let id = stream.borrow_mut().core.writer_ready().expect("escritor preso");
            drain(global_object, &stream)?;
            id
        }
    };
    Ok(promise_of(&stream, id))
}

pub(super) fn wr_desired_size(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let writer = writer_of(call.this_value()).ok_or_else(|| invalid_this(global_object, "WritableStreamDefaultWriter"))?;
    let (stream, bound) = {
        let inner = writer.borrow();
        (inner.stream.clone(), inner.bound)
    };
    if !bound {
        return Err(rethrow(global_object, invalid_state(global_object, NOT_BOUND)));
    }
    let size = stream.borrow().core.writer_desired_size();
    Ok(size.map_or_else(JSValue::null, js_number))
}

/// O escritor preso a um stream, ou a promessa rejeitada com o erro do método (`this` alheio ou escritor solto).
fn bound_writer(global_object: &JSGlobalObject, call: &HostCall, method: &str) -> Result<(WriterRef, StreamRef), JSValue> {
    let Some(writer) = writer_of(call.this_value()) else {
        let error = type_error_value(global_object, &format!("WritableStreamDefaultWriter.prototype.{method} can only be called on a WritableStreamDefaultWriter"));
        return Err(rejected_with(global_object, error));
    };
    let (stream, bound) = {
        let inner = writer.borrow();
        (inner.stream.clone(), inner.bound)
    };
    if !bound {
        let error = invalid_state(global_object, NOT_BOUND);
        return Err(rejected_with(global_object, error));
    }
    Ok((writer, stream))
}

pub(super) fn wr_abort(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let (_, stream) = match bound_writer(global_object, call, "abort") {
        Ok(found) => found,
        Err(rejected) => return Ok(rejected),
    };
    abort_stream(global_object, &stream, call.argument(0))
}

pub(super) fn wr_close(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let (_, stream) = match bound_writer(global_object, call, "close") {
        Ok(found) => found,
        Err(rejected) => return Ok(rejected),
    };
    if stream.borrow().core.close_queued_or_in_flight() {
        return rejected_invalid_state(global_object, "Cannot close a WritableStream that is already closing");
    }
    close_stream(global_object, &stream)
}

pub(super) fn wr_release_lock(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let writer = writer_of(call.this_value()).ok_or_else(|| invalid_this(global_object, "WritableStreamDefaultWriter"))?;
    let (stream, bound) = {
        let inner = writer.borrow();
        (inner.stream.clone(), inner.bound)
    };
    if !bound {
        return Ok(JSValue::undefined());
    }
    let released_error = invalid_state(global_object, "Writer has been released");
    let released = stream.borrow_mut().core.release_lock(released_error);
    drain(global_object, &stream)?;
    let mut inner = writer.borrow_mut();
    inner.bound = false;
    inner.released = released;
    Ok(JSValue::undefined())
}

/// `writableStreamDefaultControllerGetChunkSize` mais a validação de `EnqueueValueWithSize`: `Err` é o RangeError que
/// o `Core` entrega a `controller.error`. Um `size()` que lança já erra o controlador e vale 1.
fn chunk_size(global_object: &JSGlobalObject, stream: &StreamRef, chunk: JSValue) -> Result<f64, JSValue> {
    let Some(function) = stream.borrow().size else { return Ok(1.0) };
    let computed = match call_js(global_object, function, JSValue::undefined(), &[chunk]) {
        Ok(value) => to_number_checked(global_object, value).map_err(|_| take_error(global_object)),
        Err(error) => Err(error),
    };
    match computed {
        Err(error) => {
            stream.borrow_mut().core.controller_error(error);
            Ok(1.0)
        }
        Ok(size) if !size.is_finite() || size < 0.0 => Err(range_error_value(global_object, "The queuing strategy's chunk size must be a non-negative, finite number")),
        Ok(size) => Ok(size),
    }
}

pub(super) fn wr_write(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let (writer, stream) = match bound_writer(global_object, call, "write") {
        Ok(found) => found,
        Err(rejected) => return Ok(rejected),
    };
    let chunk = call.argument(0);
    let size = chunk_size(global_object, &stream, chunk);
    drain(global_object, &stream)?;
    if !writer.borrow().bound {
        return rejected_type_error(global_object, "This WritableStreamDefaultWriter was released while the queuing strategy's size() was running");
    }
    let outcome = stream.borrow_mut().core.writer_write(chunk, size);
    drain(global_object, &stream)?;
    match outcome {
        WriteOutcome::Promise(id) => Ok(promise_of(&stream, id)),
        WriteOutcome::Reject(error) => Ok(rejected_with(global_object, error)),
        WriteOutcome::RejectTypeError(message) => rejected_type_error(global_object, message),
    }
}

// ---------------------------------------------------------------------------------------------
// WritableStreamDefaultController
// ---------------------------------------------------------------------------------------------

pub(super) fn wc_signal(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let stream = controller_stream_of(call.this_value()).ok_or_else(|| invalid_this(global_object, "WritableStreamDefaultController"))?;
    let signal = stream.borrow().signal;
    Ok(signal)
}

pub(super) fn wc_error(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let stream = controller_stream_of(call.this_value()).ok_or_else(|| invalid_this(global_object, "WritableStreamDefaultController"))?;
    stream.borrow_mut().core.controller_error(call.argument(0));
    drain(global_object, &stream)?;
    Ok(JSValue::undefined())
}

/// O texto do `Symbol(nodejs.util.inspect.custom)` do `WritableStream`; `None` para o resto.
pub(super) fn inspect_text(global_object: &JSGlobalObject, value: JSValue, exhausted: bool) -> Option<JSValue> {
    let stream = stream_of(value)?;
    if exhausted {
        return Some(text_value(global_object, "WritableStream [Object]"));
    }
    let inner = stream.borrow();
    let state = match inner.core.state {
        State::Writable => "writable",
        State::Erroring => "erroring",
        State::Errored => "errored",
        State::Closed => "closed",
    };
    Some(text_value(global_object, &format!("WritableStream {{ locked: {}, state: '{state}' }}", inner.core.is_locked())))
}
