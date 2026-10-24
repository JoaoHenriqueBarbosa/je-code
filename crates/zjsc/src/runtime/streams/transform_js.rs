//! `TransformStream` e `TransformStreamDefaultController` na camada JS, portados de `JSTransformStream.cpp`,
//! `JSTransformStreamDefaultController.cpp` e `TransformStreamOperations.cpp` do bun.
//!
//! A máquina de estados é o `Core<JSValue>` de [`super::transform`]; a arena de promessas e [`drain`] são os de
//! [`super::effects`], os mesmos do `WritableStream`. O par `readable`/`writable` é nativo: o `ReadableStream` vem de
//! `readable::create_native` e o `WritableStream` de `writable_js::create_native`, ambos ligados ao mesmo estado por
//! uma [`Bridge`] (o `SourceKind::Transform`/`SinkKind::Transform` do C++). As duas pontas do par que o `Core`
//! pergunta e comanda (`Readable`/`Writable`) são adaptadores finos sobre `ReadableHandle`/`WritableHandle`.
//!
//! O `Core` sai do estado só enquanto um passo síncrono roda ([`run`]) e volta antes de qualquer código do usuário.
//! O `enqueue` empurra o chunk (onde roda o `size()` do usuário) com o `Core` fora de `run`, em três tempos, então um
//! `enqueue`/`error`/`terminate`/`desiredSize` chamado de dentro do `size()` age de verdade e na ordem do bun (o
//! chunk reentrante entra na fila antes do que o disparou). O `None` de `run` fica só para o par ainda não montado
//! (chamada de dentro do `start`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::effects::{drain, promise_of, EffectState, PromiseOp, Promises, P};
use super::trace::{hold, mark_all, Traced};
use super::readable::{
    self, callable_member, error_of, extract_high_water_mark, invalid_this, invoke_returning_promise, new_promise, prototype_of, reaction, reaction_holding, rejected_with, resolve,
    resolved_with, rethrow, then, type_error_value, NativeSource, ReadableHandle,
};
use super::transform::{Core, Effect, EnqueueError, Kind as TransformerKind, Readable, Writable};
use super::writable_js::{self, convert_strategy, NativeSink, WritableHandle};
use crate::runtime::collection_support::derived_structure;
use crate::runtime::gc::RootScope;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::intl_support::get_property;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSFinalObject;
use crate::runtime::js_value::{js_number, EncodedJSValue, JSValue};
use crate::runtime::native_class_support::{instance_structure, throw_native_type_error};
use crate::runtime::object_to_primitive::call_function;

/// As duas pontas do par, criadas depois do estado (a `Bridge` precisa dele antes).
#[derive(Clone)]
struct Sides {
    readable: JSValue,
    writable: JSValue,
    read: ReadableHandle,
    write: WritableHandle,
}

struct Transform {
    core: Option<Core<JSValue>>,
    promises: Promises,
    /// O objeto `transformer` (`null` sem ele) e seus três algoritmos.
    transformer: JSValue,
    transform: Option<JSValue>,
    flush: Option<JSValue>,
    cancel: Option<JSValue>,
    controller: JSValue,
    sides: Option<Sides>,
    /// Os algoritmos do usuário ainda valiam quando o `close`/`abort`/`cancel` em curso começou.
    live: bool,
}

type TransformRef = Rc<RefCell<Transform>>;

#[derive(Clone)]
enum Obj {
    Stream(TransformRef),
    Controller(TransformRef),
}

thread_local! {
    static OBJECTS: RefCell<HashMap<EncodedJSValue, Obj>> = RefCell::new(HashMap::new());
}

impl Traced for Transform {
    fn trace(&self, out: &mut Vec<JSValue>) {
        if let Some(core) = &self.core {
            core.for_each_value(|value| out.push(*value));
        }
        self.promises.trace(out);
        self.transformer.trace(out);
        self.transform.trace(out);
        self.flush.trace(out);
        self.cancel.trace(out);
        self.controller.trace(out);
        if let Some(sides) = &self.sides {
            sides.readable.trace(out);
            sides.writable.trace(out);
        }
    }
}

/// As raízes do coletor: os objetos de transformação (as chaves vivem enquanto o programa existir) e o que o estado
/// deles segura. As duas pontas do par moram nos `OBJECTS` de `readable` e `writable_js`.
pub(super) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    mark_all(marker, |out| {
        OBJECTS.with(|objects| {
            objects.borrow().iter().for_each(|(key, object)| {
                out.push(JSValue::decode(*key));
                match object {
                    Obj::Stream(state) | Obj::Controller(state) => state.trace(out),
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

fn stream_of(value: JSValue) -> Option<TransformRef> {
    match lookup(value)? {
        Obj::Stream(stream) => Some(stream),
        Obj::Controller(_) => None,
    }
}

fn controller_stream_of(value: JSValue) -> Option<TransformRef> {
    match lookup(value)? {
        Obj::Controller(stream) => Some(stream),
        Obj::Stream(_) => None,
    }
}

fn register(value: JSValue, object: Obj) {
    OBJECTS.with(|objects| objects.borrow_mut().insert(value.encode(), object));
}

// ---------------------------------------------------------------------------------------------
// As pontas do par vistas pelo Core
// ---------------------------------------------------------------------------------------------

struct ReadSide<'a> {
    global_object: &'a JSGlobalObject,
    handle: &'a ReadableHandle,
}

impl Readable<JSValue> for ReadSide<'_> {
    fn controller_present(&self) -> bool {
        self.handle.controller_present()
    }

    fn can_close_or_enqueue(&self) -> bool {
        self.handle.can_close_or_enqueue()
    }

    fn enqueue(&mut self, chunk: JSValue) -> Result<(), JSValue> {
        self.handle.enqueue(self.global_object, chunk)
    }

    fn has_backpressure(&self) -> bool {
        self.handle.has_backpressure()
    }

    fn close(&mut self) {
        self.handle.close(self.global_object);
    }

    fn error(&mut self, error: JSValue) {
        self.handle.error(self.global_object, error);
    }

    fn desired_size(&self) -> Option<f64> {
        self.handle.desired_size()
    }

    fn is_errored(&self) -> bool {
        self.handle.is_errored()
    }

    fn stored_error(&self) -> Option<JSValue> {
        self.handle.stored_error()
    }
}

struct WriteSide<'a> {
    global_object: &'a JSGlobalObject,
    handle: &'a WritableHandle,
}

impl Writable<JSValue> for WriteSide<'_> {
    fn is_writable(&self) -> bool {
        self.handle.is_writable()
    }

    fn is_erroring(&self) -> bool {
        self.handle.is_erroring()
    }

    fn is_errored(&self) -> bool {
        self.handle.is_errored()
    }

    fn stored_error(&self) -> Option<JSValue> {
        self.handle.stored_error()
    }

    fn error_if_needed(&mut self, error: JSValue) {
        // Só o ouvinte do `AbortSignal` do escritor pode lançar aqui; o C++ também não propaga.
        let _ = self.handle.error_if_needed(self.global_object, error);
    }
}

/// Roda `operation` (síncrona, sem código do usuário) com o `Core` fora do estado e as duas pontas do par; `None` se
/// o par ainda não existe.
fn run<T>(global_object: &JSGlobalObject, stream: &TransformRef, operation: impl FnOnce(&mut Core<JSValue>, &mut ReadSide<'_>, &mut WriteSide<'_>) -> T) -> Option<T> {
    let sides = stream.borrow().sides.clone()?;
    let mut core = stream.borrow_mut().core.take()?;
    let mut read = ReadSide { global_object, handle: &sides.read };
    let mut write = WriteSide { global_object, handle: &sides.write };
    let outcome = operation(&mut core, &mut read, &mut write);
    stream.borrow_mut().core = Some(core);
    Some(outcome)
}

// ---------------------------------------------------------------------------------------------
// Efeitos
// ---------------------------------------------------------------------------------------------

impl EffectState for Transform {
    type Effect = Effect<JSValue>;

    fn promises(&mut self) -> &mut Promises {
        &mut self.promises
    }

    fn take_effects(&mut self) -> Vec<Effect<JSValue>> {
        self.core.as_mut().map(|core| std::mem::take(&mut core.effects)).unwrap_or_default()
    }

    fn classify(effect: Effect<JSValue>) -> Result<(P, PromiseOp), Effect<JSValue>> {
        match effect {
            Effect::NewPromise(id) => Ok((id, PromiseOp::New)),
            Effect::Resolve(id) => Ok((id, PromiseOp::Resolve)),
            Effect::Reject(id, error) => Ok((id, PromiseOp::Reject(error))),
            other => Err(other),
        }
    }

    fn apply_other(global_object: &JSGlobalObject, stream: &TransformRef, effect: Effect<JSValue>) -> Result<(), Thrown> {
        match effect {
            Effect::CallTransform(chunk, result) => call_transform(global_object, stream, chunk, result),
            Effect::ResolveWith(outer, inner) => {
                super::readable::resolve(global_object, promise_of(stream, outer), promise_of(stream, inner));
            }
            Effect::CallFlush => call_flush(global_object, stream),
            Effect::CallCancel(reason, from_sink) => call_cancel(global_object, stream, reason, from_sink),
            Effect::NewPromise(_) | Effect::Resolve(_) | Effect::Reject(..) => unreachable!("efeito de promessa é de `effects::drain`"),
        }
        Ok(())
    }
}

/// O que fazer com o resultado de um algoritmo: o `Core` recebe `Ok`/`Err` e os efeitos novos são drenados.
type Done = Rc<dyn Fn(&mut Core<JSValue>, &mut ReadSide<'_>, &mut WriteSide<'_>, Result<(), JSValue>)>;

fn finish(global_object: &JSGlobalObject, stream: &TransformRef, done: &Done, outcome: Result<(), JSValue>) -> Result<(), Thrown> {
    run(global_object, stream, |core, read, write| done.as_ref()(core, read, write, outcome));
    drain(global_object, stream)
}

/// `held` lista os valores JS que `done` captura (o motivo do cancelamento): o coletor os visita enquanto as reações
/// viverem.
fn settle(global_object: &JSGlobalObject, stream: &TransformRef, promise: JSValue, done: Done, held: Vec<Rc<dyn Traced>>) {
    let (for_fulfilled, done_fulfilled) = (stream.clone(), done.clone());
    let on_fulfilled = reaction_holding(global_object, held.clone(), move |global_object, _| finish(global_object, &for_fulfilled, &done_fulfilled, Ok(())));
    let for_rejected = stream.clone();
    let on_rejected = reaction_holding(global_object, held, move |global_object, error| finish(global_object, &for_rejected, &done, Err(error)));
    then(global_object, promise, on_fulfilled, on_rejected, JSValue::undefined());
}

/// `defaultTransformAlgorithm`: `controller.enqueue(chunk)`; o que ele lança vira promessa rejeitada.
fn default_transform(global_object: &JSGlobalObject, stream: &TransformRef, chunk: JSValue) -> JSValue {
    match enqueue(global_object, stream, chunk) {
        Ok(()) => resolved_with(global_object, JSValue::undefined()),
        Err(error) => rejected_with(global_object, error),
    }
}

fn call_transform(global_object: &JSGlobalObject, stream: &TransformRef, chunk: JSValue, result: P) {
    let (transform, transformer, controller, cleared) = {
        let inner = stream.borrow();
        (inner.transform, inner.transformer, inner.controller, inner.core.as_ref().map_or(true, |core| core.algorithms_cleared))
    };
    let promise = match transform {
        Some(function) if !cleared => invoke_returning_promise(global_object, function, transformer, &[chunk, controller]),
        _ => default_transform(global_object, stream, chunk),
    };
    settle(global_object, stream, promise, Rc::new(move |core, read, write, outcome| core.on_transform_done(read, write, result, outcome)), Vec::new());
}

fn call_flush(global_object: &JSGlobalObject, stream: &TransformRef) {
    let (flush, transformer, controller, live) = {
        let inner = stream.borrow();
        (inner.flush, inner.transformer, inner.controller, inner.live)
    };
    let promise = match flush {
        Some(function) if live => invoke_returning_promise(global_object, function, transformer, &[controller]),
        _ => resolved_with(global_object, JSValue::undefined()),
    };
    settle(global_object, stream, promise, Rc::new(|core, read, _, outcome| core.on_flush_done(read, outcome)), Vec::new());
}

fn call_cancel(global_object: &JSGlobalObject, stream: &TransformRef, reason: JSValue, from_sink: bool) {
    let (cancel, transformer, live) = {
        let inner = stream.borrow();
        (inner.cancel, inner.transformer, inner.live)
    };
    let promise = match cancel {
        Some(function) if live => invoke_returning_promise(global_object, function, transformer, &[reason]),
        _ => resolved_with(global_object, JSValue::undefined()),
    };
    settle(global_object, stream, promise, Rc::new(move |core, read, write, outcome| core.on_cancel_done(read, write, from_sink, reason, outcome)), vec![hold(reason)]);
}

// ---------------------------------------------------------------------------------------------
// Fonte e sink nativos
// ---------------------------------------------------------------------------------------------

/// `SourceKind::Transform` e `SinkKind::Transform`: os algoritmos do par rodam no `Core`.
struct Bridge(TransformRef);

impl Bridge {
    /// `close`/`abort`/`cancel`: guarda se os algoritmos do usuário ainda valem, roda o passo do `Core`, drena e
    /// devolve a promessa do pedido de fim.
    fn finish_algorithm(&self, global_object: &JSGlobalObject, step: impl FnOnce(&mut Core<JSValue>) -> P) -> JSValue {
        let live = self.0.borrow().core.as_ref().is_some_and(|core| !core.algorithms_cleared);
        self.0.borrow_mut().live = live;
        let id = run(global_object, &self.0, |core, _, _| step(core));
        let _ = drain(global_object, &self.0);
        id.map_or_else(|| resolved_with(global_object, JSValue::undefined()), |id| promise_of(&self.0, id))
    }
}

impl NativeSource for Bridge {
    fn pull(&self, global_object: &JSGlobalObject) -> JSValue {
        let id = run(global_object, &self.0, |core, _, _| core.source_pull()).flatten();
        let _ = drain(global_object, &self.0);
        id.map_or_else(|| resolved_with(global_object, JSValue::undefined()), |id| promise_of(&self.0, id))
    }

    fn cancel(&self, global_object: &JSGlobalObject, reason: JSValue) -> JSValue {
        self.finish_algorithm(global_object, |core| core.source_cancel(reason))
    }

    /// O estado da ponte é o `Transform`, registrado num `OBJECTS` (`visit_gc_roots`).
    fn trace(&self, _out: &mut Vec<JSValue>) {}
}

impl NativeSink for Bridge {
    fn write(&self, global_object: &JSGlobalObject, chunk: JSValue) -> JSValue {
        let steps = run(global_object, &self.0, |core, _, _| (core.backpressure, core.sink_write(chunk), core.backpressure_change_promise()));
        let _ = drain(global_object, &self.0);
        let Some((backpressure, result, change)) = steps else { return resolved_with(global_object, JSValue::undefined()) };
        if let (true, Some(change)) = (backpressure, change) {
            let stream = self.0.clone();
            let on_fulfilled = reaction(global_object, move |global_object, _| {
                run(global_object, &stream, |core, _, write| core.on_backpressure_change_fulfilled(&*write, result));
                drain(global_object, &stream)
            });
            then(global_object, promise_of(&self.0, change), on_fulfilled, JSValue::undefined(), JSValue::undefined());
        }
        promise_of(&self.0, result)
    }

    fn close(&self, global_object: &JSGlobalObject) -> JSValue {
        self.finish_algorithm(global_object, |core| core.sink_close())
    }

    fn abort(&self, global_object: &JSGlobalObject, reason: JSValue) -> JSValue {
        self.finish_algorithm(global_object, |core| core.sink_abort(reason))
    }

    fn trace(&self, _out: &mut Vec<JSValue>) {}
}

// ---------------------------------------------------------------------------------------------
// Construtor
// ---------------------------------------------------------------------------------------------

struct TransformerDict {
    cancel: Option<JSValue>,
    flush: Option<JSValue>,
    start: Option<JSValue>,
    transform: Option<JSValue>,
    has_readable_type: bool,
    has_writable_type: bool,
}

/// `convertTransformerDict`: a ordem de leitura das chaves é a do C++.
fn convert_transformer(global_object: &JSGlobalObject, transformer: JSValue) -> Result<TransformerDict, Thrown> {
    let mut dict = TransformerDict { cancel: None, flush: None, start: None, transform: None, has_readable_type: false, has_writable_type: false };
    if transformer.is_undefined_or_null() {
        return Ok(dict);
    }
    dict.cancel = callable_member(global_object, transformer, "transformer", "cancel")?;
    dict.flush = callable_member(global_object, transformer, "transformer", "flush")?;
    dict.has_readable_type = !get_property(global_object, transformer, "readableType")?.is_undefined();
    dict.start = callable_member(global_object, transformer, "transformer", "start")?;
    dict.transform = callable_member(global_object, transformer, "transformer", "transform")?;
    dict.has_writable_type = !get_property(global_object, transformer, "writableType")?.is_undefined();
    Ok(dict)
}

/// `new TransformStream(transformer, writableStrategy, readableStrategy)`.
pub(super) fn construct_stream(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let argument = call.argument(0);
    let transformer = if argument.is_undefined() {
        JSValue::null()
    } else if !argument.is_object() {
        return Err(throw_native_type_error(global_object, "TransformStream constructor takes an object as first argument"));
    } else {
        argument
    };
    let (writable_high_water_mark, writable_size) = convert_strategy(global_object, call.argument(1))?;
    let (readable_high_water_mark, readable_size) = convert_strategy(global_object, call.argument(2))?;
    let structure = derived_structure(global_object, call, instance_structure)?;
    let vm = global_object.vm();
    let value = JSFinalObject::create(vm, &structure).as_value();
    // A instância, o controlador, a promessa de start e o retorno do `start` só vivem nestes locais até o estado os
    // segurar (`register` e `sides`); cada alocação do construtor é ponto seguro.
    let _value = RootScope::new(&[value]);
    let dict = convert_transformer(global_object, transformer)?;
    if dict.has_readable_type {
        return Err(Thrown::range_error("The transformer's 'readableType' property is reserved and must not be present"));
    }
    if dict.has_writable_type {
        return Err(Thrown::range_error("The transformer's 'writableType' property is reserved and must not be present"));
    }
    let readable_high_water_mark = extract_high_water_mark(readable_high_water_mark, 0.0)?;
    let writable_high_water_mark = extract_high_water_mark(writable_high_water_mark, 1.0)?;

    let controller_structure = instance_structure(vm, Some(global_object), prototype_of("TransformStreamDefaultController"));
    let controller = JSFinalObject::create(vm, &controller_structure).as_value();
    let _controller = RootScope::new(&[controller]);
    let kind = if transformer.is_object() { TransformerKind::JavaScript } else { TransformerKind::Identity };
    let stream = Rc::new(RefCell::new(Transform {
        core: Some(Core::new(kind)),
        promises: Promises::new(),
        transformer,
        transform: dict.transform,
        flush: dict.flush,
        cancel: dict.cancel,
        controller,
        sides: None,
        live: true,
    }));
    register(value, Obj::Stream(stream.clone()));
    register(controller, Obj::Controller(stream.clone()));
    drain(global_object, &stream)?;

    let start_promise = new_promise(global_object);
    let _start_promise = RootScope::new(&[start_promise]);
    let bridge = Rc::new(Bridge(stream.clone()));
    let (writable, write) = writable_js::create_native(global_object, bridge.clone(), writable_high_water_mark, writable_size, start_promise)?;
    let _writable = RootScope::new(&[writable]);
    let (readable, read) = readable::create_native(global_object, bridge, readable_high_water_mark, readable_size, start_promise);
    stream.borrow_mut().sides = Some(Sides { readable, writable, read, write });

    // Um `start` que lança sai do construtor (a promessa de start nunca resolve); senão ela resolve com o retorno.
    let start_result = match dict.start {
        Some(function) => call_function(global_object, function, transformer, &[controller]).ok_or(Thrown::Pending)?,
        None => JSValue::undefined(),
    };
    let _start_result = RootScope::new(&[start_result]);
    resolve(global_object, start_promise, start_result);
    Ok(value)
}

// ---------------------------------------------------------------------------------------------
// TransformStream
// ---------------------------------------------------------------------------------------------

pub(super) fn ts_readable(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let stream = stream_of(call.this_value()).ok_or_else(|| invalid_this(global_object, "TransformStream"))?;
    let readable = stream.borrow().sides.as_ref().map_or_else(JSValue::undefined, |sides| sides.readable);
    Ok(readable)
}

pub(super) fn ts_writable(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let stream = stream_of(call.this_value()).ok_or_else(|| invalid_this(global_object, "TransformStream"))?;
    let writable = stream.borrow().sides.as_ref().map_or_else(JSValue::undefined, |sides| sides.writable);
    Ok(writable)
}

/// O texto do `Symbol(nodejs.util.inspect.custom)` do `TransformStream`: `readable`, `writable` e `backpressure`
/// (medido no bun 1.4.2). `None` se `value` não é um `TransformStream`.
pub(super) fn inspect_text(global_object: &JSGlobalObject, value: JSValue, options: JSValue) -> Option<Result<JSValue, Thrown>> {
    let stream = stream_of(value)?;
    let backpressure = stream.borrow().core.as_ref().is_some_and(|core| core.backpressure);
    let tail = vec![format!("backpressure: {backpressure}")];
    let text = super::text_streams::inspect_composite(global_object, "TransformStream", value, Vec::new(), tail, options);
    Some(text.map(|text| readable::text_value(global_object, &text)))
}

// ---------------------------------------------------------------------------------------------
// TransformStreamDefaultController
// ---------------------------------------------------------------------------------------------

/// `transformStreamDefaultControllerEnqueue` mais a drenagem; `Err` é a exceção que o método lança.
fn enqueue(global_object: &JSGlobalObject, stream: &TransformRef, chunk: JSValue) -> Result<(), JSValue> {
    // Três tempos: confere o readable, empurra o chunk com o `Core` fora de `run` (o `size()` do usuário pode voltar ao
    // controlador e aquela volta pega o `Core` livre) e fecha as contas com o resultado.
    let outcome = match run(global_object, stream, |_, read, _| Core::<JSValue>::enqueue_check(&*read)) {
        Some(Ok(())) => {
            let sides = stream.borrow().sides.clone();
            sides.and_then(|sides| {
                let pushed = sides.read.enqueue(global_object, chunk);
                run(global_object, stream, |core, read, write| core.enqueue_finish(read, write, pushed))
            })
        }
        other => other,
    };
    match outcome {
        Some(Err(EnqueueError::Closed(message))) => Err(type_error_value(global_object, message)),
        Some(Err(EnqueueError::Stored(error))) => {
            let _ = drain(global_object, stream);
            Err(error.unwrap_or_else(JSValue::undefined))
        }
        Some(Ok(())) | None => drain(global_object, stream).map_err(|thrown| error_of(global_object, thrown)),
    }
}

pub(super) fn tc_desired_size(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let stream = controller_stream_of(call.this_value()).ok_or_else(|| invalid_this(global_object, "TransformStreamDefaultController"))?;
    let inner = stream.borrow();
    let size = match (inner.core.as_ref(), inner.sides.as_ref()) {
        (Some(core), Some(sides)) => core.desired_size(&ReadSide { global_object, handle: &sides.read }),
        _ => None,
    };
    Ok(size.map_or_else(JSValue::null, js_number))
}

pub(super) fn tc_enqueue(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let stream = controller_stream_of(call.this_value()).ok_or_else(|| invalid_this(global_object, "TransformStreamDefaultController"))?;
    enqueue(global_object, &stream, call.argument(0)).map_err(|error| rethrow(global_object, error))?;
    Ok(JSValue::undefined())
}

pub(super) fn tc_error(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let stream = controller_stream_of(call.this_value()).ok_or_else(|| invalid_this(global_object, "TransformStreamDefaultController"))?;
    let error = call.argument(0);
    run(global_object, &stream, |core, read, write| core.error(read, write, error));
    drain(global_object, &stream)?;
    Ok(JSValue::undefined())
}

pub(super) fn tc_terminate(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let stream = controller_stream_of(call.this_value()).ok_or_else(|| invalid_this(global_object, "TransformStreamDefaultController"))?;
    let terminated = type_error_value(global_object, "The TransformStream has been terminated");
    run(global_object, &stream, |core, read, write| core.controller_terminate(read, write, terminated));
    drain(global_object, &stream)?;
    Ok(JSValue::undefined())
}
