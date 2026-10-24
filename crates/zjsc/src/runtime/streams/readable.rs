//! `ReadableStream` com fonte subjacente do tipo padrão, `ReadableStreamDefaultController` e
//! `ReadableStreamDefaultReader`, portados de `JSReadableStream.cpp`, `JSReadableStreamDefaultController.cpp`,
//! `JSReadableStreamDefaultReader.cpp` e `ReadableStreamOperations.cpp` do bun (fatia 3 de
//! `wip-notes/streams-plan.md`).
//!
//! O estado vive em `Rc<RefCell<..>>` guardados num registro por valor da célula (o mesmo molde de `mod.rs`), zerado
//! em `reset_for_program`. Nenhum empréstimo de `RefCell` atravessa uma chamada a JS. As reações de promessa do C++
//! (`jsWebStreamsHandler_*`, com o contexto em `argument(1)`) são funções nativas que procuram o fecho num registro
//! pelo `callee` ([`reaction`]).
//!
//! A ordem de microtasks segue o C++: o resultado de `start` e o de `pull` passam por uma promessa e uma reação; a
//! leitura com fila não vazia e sem pedidos pendentes resolve na hora, sem `ReadRequest`; `enqueue` com leitura
//! pendente resolve o pedido direto.
//!
//! DIVERGÊNCIAS:
//!
//! - `type: 'bytes'` cria o `ReadableByteStreamController` sobre o [`ByteCore`] (`enqueue`, `close`, `error`,
//!   `desiredSize`, `byobRequest`, `start` e `pull`, leitura do leitor padrão, com `autoAllocateChunkSize`);
//!   `type: 'direct'` vira um stream padrão;
//! - `getReader({ mode: 'byob' })` e `new ReadableStreamBYOBReader(stream)` criam o leitor BYOB (`read(view, { min })`,
//!   `cancel`, `closed`, `releaseLock`); o `ReadableStreamBYOBRequest` (`view`, `respond`, `respondWithNewView`) vive
//!   num `ArrayBuffer` próprio, copiado para a arena do núcleo antes de cada operação que o lê;
//! - `tee` vive em `readable/tee.rs` (origem padrão) e `readable/byte_tee.rs` (origem de bytes);`readMany`, `values` e o texto do `inspect.custom` do leitor continuam fora.

use crate::runtime::js_promise_host::PromiseHost;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use crate::host_function;
use crate::runtime::collection_support::derived_structure;
use crate::runtime::host_call::{throw_thrown, HostCall, HostResult, Thrown};
use super::bytes::{msg, Action, ByteCore, Chunk, PullInto, ReaderKind};
use crate::runtime::typed_array_type::TypedArrayType;
use super::effects::{drain, EffectState, PromiseOp, Promises, P};
use super::trace::{hold, mark_all, take_contended, Traced};
use crate::runtime::array_buffer::{ArrayBuffer, ArrayBufferRef};
use crate::runtime::js_module_loader::{describe_received, rust_string};
use crate::runtime::gc::RootScope;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intl_support::{get_property, to_number_checked};
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_data_view::JSDataView;
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction};
use crate::runtime::js_generic_typed_array_view::JSGenericTypedArrayView;
use crate::runtime::text_encoder::uint8_array_from;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSFinalObject;
use crate::runtime::js_promise::JSPromise;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_number, EncodedJSValue, JSValue};
use crate::runtime::native_class_support::{instance_structure, throw_coded_type_error, throw_native_type_error};
use crate::runtime::node_error::throw_coded_range_error;
use crate::runtime::object_to_primitive::call_function;
use crate::runtime::string_regexp_support::create_iterator_result_object;
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::wtf::text::wtf_string::String as WtfString;

mod byte_tee;
mod tee;
pub(super) use tee::rs_tee;
pub(crate) use tee::tee_of;

/// Faz o leitor do stream visível em `locked` (o leitor interno de `read_all` é escondido) ou, sem leitor, trava o
/// stream com um leitor padrão que ninguém solta: o `locked` de `body` depois de `textStream()`.
pub(crate) fn lock_stream(global_object: &JSGlobalObject, value: JSValue) {
    let Some(stream) = stream_of(value) else { return };
    let existing = stream.borrow().reader.clone();
    match existing {
        Some(reader) => reader.borrow_mut().hidden = false,
        None => {
            let _ = create_reader(global_object, &stream, &reader_structure(global_object, ReaderKind::Default), ReaderKind::Default);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Readable,
    Closed,
    Errored,
}

impl State {
    fn name(self) -> &'static str {
        match self {
            State::Readable => "readable",
            State::Closed => "closed",
            State::Errored => "errored",
        }
    }
}

type StreamRef = Rc<RefCell<Stream>>;
type ReaderRef = Rc<RefCell<Reader>>;
type ControllerRef = Rc<RefCell<Controller>>;

struct Stream {
    state: State,
    stored_error: JSValue,
    reader: Option<ReaderRef>,
    controller: Option<ControllerRef>,
    disturbed: bool,
    /// `type: 'bytes'`.
    bytes: bool,
    /// O controlador de bytes (só em `type: 'bytes'`).
    byte_controller: Option<ByteRef>,
}

pub(super) type ByteRef = Rc<RefCell<ByteState>>;

/// O `ReadableByteStreamController`: o [`ByteCore`] mais o que o liga ao mundo JS. As [`Action`]s que o núcleo
/// devolve ficam em `effects` e saem por [`drain`], a única função que as aplica.
pub(super) struct ByteState {
    stream: StreamRef,
    core: ByteCore,
    algorithms: Option<SourceAlgorithms>,
    value: JSValue,
    effects: Vec<Action>,
    promises: Promises,
    /// O `byobRequest` atual (criado sob demanda pelo getter).
    request: Option<RequestRef>,
}

impl EffectState for ByteState {
    type Effect = Action;

    fn promises(&mut self) -> &mut Promises {
        &mut self.promises
    }

    fn take_effects(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.effects)
    }

    fn classify(effect: Action) -> Result<(P, PromiseOp), Action> {
        Err(effect)
    }

    fn apply_other(global_object: &JSGlobalObject, state: &Rc<RefCell<Self>>, effect: Action) -> Result<(), Thrown> {
        match effect {
            Action::FulfillDefaultRead(chunk) => {
                let view = chunk_view(global_object, state, chunk)?;
                let stream = state.borrow().stream.clone();
                fulfill_read_request(global_object, &stream, view, false);
            }
            Action::CloseStream => {
                state.borrow_mut().algorithms = None;
                let stream = state.borrow().stream.clone();
                // Os read-into pendentes do leitor BYOB não são resolvidos pelo `ReadableStreamClose`: só o
                // `respond(0)` os conclui (com a view vazia), então ficam guardados enquanto o stream fecha.
                let held = stream.borrow().reader.clone().filter(|reader| reader.borrow().kind == ReaderKind::Byob).map(|reader| {
                    let pending = std::mem::take(&mut reader.borrow_mut().requests);
                    (reader, pending)
                });
                stream_close(global_object, &stream);
                if let Some((reader, pending)) = held {
                    reader.borrow_mut().requests = pending;
                }
            }
            // O descritor do `autoAllocateChunkSize` (leitor padrão): o pedido de leitura recebe um `Uint8Array` sobre
            // os bytes preenchidos; sem bytes (stream fechado) o `CloseStream` já resolveu os pedidos.
            Action::FulfillByobRead { pull_into, .. } if pull_into.reader == ReaderKind::Default => {
                if pull_into.bytes_filled > 0 {
                    // Medido no bun: a view é um `Uint8Array` dos bytes preenchidos sobre o buffer INTEIRO do descritor
                    // (`respond(1)` de um buffer de 4 bytes dá `byteLength` 1 com `buffer.byteLength` 4).
                    let view = byob_view(global_object, state, &pull_into, pull_into.bytes_filled)?;
                    let stream = state.borrow().stream.clone();
                    fulfill_read_request(global_object, &stream, view, false);
                }
            }
            // O leitor BYOB: a view é refeita do mesmo tipo sobre o buffer devolvido e resolve o read-into mais antigo.
            Action::FulfillByobRead { pull_into, elements, done } => {
                let stream = state.borrow().stream.clone();
                let reader = stream.borrow().reader.clone().filter(|reader| reader.borrow().kind == ReaderKind::Byob);
                if let Some(reader) = reader {
                    let view = byob_view(global_object, state, &pull_into, elements)?;
                    let request = reader.borrow_mut().requests.pop_front();
                    if let Some(promise) = request {
                        let result = create_iterator_result_object(global_object, view, done);
                        resolve(global_object, promise, result);
                    }
                }
            }
            Action::InvalidateByobRequest => invalidate_request(state),
        }
        Ok(())
    }
}

/// O `ReadableStreamBYOBRequest` de um `pullInto`: o objeto persiste até ser invalidado (`respond`, `enqueue`, erro,
/// cancelamento), quando a view é zerada (o `ArrayBuffer` dela desanexa) e `respond` passa a lançar.
struct ByobRequest {
    value: JSValue,
    controller: Option<ByteRef>,
    view: Option<JSValue>,
    buffer: Option<ArrayBufferRef>,
}

type RequestRef = Rc<RefCell<ByobRequest>>;

/// `ReadableByteStreamControllerInvalidateBYOBRequest`.
fn invalidate_request(state: &ByteRef) {
    let request = state.borrow_mut().request.take();
    if let Some(request) = request {
        let mut inner = request.borrow_mut();
        inner.controller = None;
        inner.view = None;
        if let Some(buffer) = inner.buffer.take() {
            buffer.detach();
        }
    }
}

/// O que o usuário escreveu na view do `byobRequest` volta para o buffer do `pullInto` na arena (a view vive num
/// `ArrayBuffer` próprio). Roda antes de qualquer operação que lê esse buffer.
fn sync_request_to_core(state: &ByteRef) {
    let request = state.borrow().request.clone();
    let Some(buffer) = request.and_then(|request| request.borrow().buffer.clone()) else { return };
    if buffer.is_detached() {
        return;
    }
    let data = buffer.with_bytes(<[u8]>::to_vec);
    let mut inner = state.borrow_mut();
    if let Some((id, ..)) = inner.core.byob_request_view() {
        inner.core.buffers.write(id, 0, &data);
    }
}

/// A view de `FulfillByobRead`: o mesmo tipo da view que o usuário passou (`view_kind` é o `TypedArrayType` + 0),
/// sobre um `ArrayBuffer` novo com os bytes do descritor, com `elements` elementos a partir de `byte_offset`.
fn byob_view(global_object: &JSGlobalObject, state: &ByteRef, descriptor: &PullInto, elements: usize) -> Result<JSValue, Thrown> {
    let data = state.borrow().core.buffers.bytes(descriptor.buffer).to_vec();
    let buffer = ArrayBuffer::create_from_bytes(data);
    let kind = TypedArrayType::from_index((descriptor.view_kind as usize).saturating_sub(1));
    let realm = &global_object.array_buffer_realm;
    if kind == TypedArrayType::DataView {
        let structure = realm.data_view_structure(false);
        return Ok(JSDataView::create(global_object, &structure, buffer, descriptor.byte_offset, Some(elements))?.as_value());
    }
    let structure = realm.typed_arrays.structure(kind, false);
    Ok(JSGenericTypedArrayView::create_with_buffer(global_object, &structure, buffer, descriptor.byte_offset, Some(elements))?.as_value())
}

struct Reader {
    /// `Default` ou `Byob`: o tipo do leitor, que decide quem atende `requests`.
    kind: ReaderKind,
    stream: Option<StreamRef>,
    /// As promessas de `read()` pendentes (o `JSReadRequest` do leitor padrão ou a fila de read-into do BYOB).
    requests: VecDeque<JSValue>,
    closed: JSValue,
    /// Leitor interno da leitura do corpo (`text()` e afins): só aparece em `locked` se a leitura não esvazia o stream
    /// de uma vez (ver `visibly_locked`).
    hidden: bool,
}

struct Controller {
    stream: StreamRef,
    queue: VecDeque<(JSValue, f64)>,
    total: f64,
    started: bool,
    close_requested: bool,
    pull_again: bool,
    pulling: bool,
    high_water_mark: f64,
    size: Option<JSValue>,
    /// `SourceAlgorithmSlots`: `None` é o par de algoritmos trivial (depois de `ClearAlgorithms`).
    algorithms: Option<SourceAlgorithms>,
    value: JSValue,
}

/// Uma fonte nativa (`SourceKind::Transform` e afins): `pull` e `cancel` rodam em Rust e devolvem a promessa do
/// algoritmo (uma promessa JS; `resolved_with` embrulha o que não for).
pub(super) trait NativeSource {
    fn pull(&self, global_object: &JSGlobalObject) -> JSValue;
    fn cancel(&self, global_object: &JSGlobalObject, reason: JSValue) -> JSValue;
    /// Os valores JS que a fonte segura além do que está num `OBJECTS` (o `visitChildren` dela).
    fn trace(&self, out: &mut Vec<JSValue>);
}

/// Qual braço roda `pull`/`cancel` do controlador: o `underlyingSource` do usuário ou uma fonte nativa.
#[derive(Clone)]
enum SourceAlgorithms {
    JavaScript { source: JSValue, pull: Option<JSValue>, cancel: Option<JSValue> },
    Native(Rc<dyn NativeSource>),
}

impl SourceAlgorithms {
    /// O algoritmo `pull`; `controller_value` é o argumento que o `pull` do usuário recebe.
    fn run_pull(&self, global_object: &JSGlobalObject, controller_value: JSValue) -> JSValue {
        match self {
            SourceAlgorithms::JavaScript { source, pull: Some(function), .. } => invoke_returning_promise(global_object, *function, *source, &[controller_value]),
            SourceAlgorithms::JavaScript { .. } => resolved_with(global_object, JSValue::undefined()),
            SourceAlgorithms::Native(native) => resolved_with(global_object, native.pull(global_object)),
        }
    }

    fn run_cancel(&self, global_object: &JSGlobalObject, reason: JSValue) -> JSValue {
        match self {
            SourceAlgorithms::JavaScript { source, cancel: Some(function), .. } => invoke_returning_promise(global_object, *function, *source, &[reason]),
            SourceAlgorithms::JavaScript { .. } => resolved_with(global_object, JSValue::undefined()),
            SourceAlgorithms::Native(native) => resolved_with(global_object, native.cancel(global_object, reason)),
        }
    }
}

#[derive(Clone)]
enum Obj {
    Stream(StreamRef),
    Reader(ReaderRef),
    Controller(ControllerRef),
    ByteController(ByteRef),
    Request(RequestRef),
}

type Reaction = Rc<dyn Fn(&JSGlobalObject, JSValue) -> Result<(), Thrown>>;

/// Uma reação registrada: o código da closure e o que ela captura em valores JS (o contexto que o C++ guarda nos
/// slots da função `jsWebStreamsHandler_*`), visitado enquanto a função da reação viver.
struct Handler {
    run: Reaction,
    held: Vec<Rc<dyn Traced>>,
}

thread_local! {
    static OBJECTS: RefCell<HashMap<EncodedJSValue, Obj>> = RefCell::new(HashMap::new());
    static HANDLERS: RefCell<HashMap<usize, Handler>> = RefCell::new(HashMap::new());
    static PROTOTYPES: RefCell<HashMap<&'static str, EncodedJSValue>> = RefCell::new(HashMap::new());
}

impl Traced for Stream {
    fn trace(&self, out: &mut Vec<JSValue>) {
        self.stored_error.trace(out);
    }
}

impl Traced for Reader {
    fn trace(&self, out: &mut Vec<JSValue>) {
        self.requests.trace(out);
        self.closed.trace(out);
    }
}

impl Traced for Controller {
    fn trace(&self, out: &mut Vec<JSValue>) {
        self.queue.iter().for_each(|(chunk, _)| chunk.trace(out));
        self.size.trace(out);
        self.algorithms.trace(out);
        self.value.trace(out);
    }
}

impl Traced for ByteState {
    fn trace(&self, out: &mut Vec<JSValue>) {
        self.algorithms.trace(out);
        self.value.trace(out);
        self.promises.trace(out);
        self.request.trace(out);
    }
}

impl Traced for ByobRequest {
    fn trace(&self, out: &mut Vec<JSValue>) {
        self.value.trace(out);
        self.view.trace(out);
    }
}

impl Traced for SourceAlgorithms {
    fn trace(&self, out: &mut Vec<JSValue>) {
        match self {
            SourceAlgorithms::JavaScript { source, pull, cancel } => {
                source.trace(out);
                pull.trace(out);
                cancel.trace(out);
            }
            SourceAlgorithms::Native(native) => native.trace(out),
        }
    }
}

impl Obj {
    /// Os valores do estado do objeto (o objeto JS da chave o chamador já pôs).
    fn trace(&self, out: &mut Vec<JSValue>) {
        match self {
            Obj::Stream(state) => state.trace(out),
            Obj::Reader(state) => state.trace(out),
            Obj::Controller(state) => state.trace(out),
            Obj::ByteController(state) => state.trace(out),
            Obj::Request(state) => state.trace(out),
        }
    }
}

/// As raízes do coletor: os protótipos, os objetos de stream (as chaves vivem enquanto o programa existir, como as
/// instâncias de `INSTANCES`) e o que o estado de cada um segura. As reações são efêmeras da função delas
/// ([`visit_side_table`]).
pub(super) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    PROTOTYPES.with(|prototypes| prototypes.borrow().values().for_each(|prototype| marker.encoded(*prototype)));
    mark_all(marker, |out| {
        OBJECTS.with(|objects| {
            objects.borrow().iter().for_each(|(key, object)| {
                out.push(JSValue::decode(*key));
                object.trace(out);
            })
        })
    });
}

/// As reações como tabela lateral da função da reação: o contexto dela é filho da função, e a entrada sai com ela.
pub(crate) fn visit_side_table(marker: &mut crate::runtime::gc::Marker, phase: crate::runtime::gc::SidePhase) -> bool {
    let progressed = crate::runtime::gc::side_table(&HANDLERS, marker, phase, |handler, out| handler.held.iter().for_each(|held| held.trace(out)));
    if take_contended() {
        marker.abort();
    }
    progressed
}

pub(super) fn reset_for_program() {
    let _ = OBJECTS.try_with(|objects| objects.borrow_mut().clear());
    let _ = HANDLERS.try_with(|handlers| handlers.borrow_mut().clear());
    let _ = PROTOTYPES.try_with(|prototypes| prototypes.borrow_mut().clear());
}

pub(super) fn register_prototype(name: &'static str, prototype: JSValue) {
    PROTOTYPES.with(|prototypes| prototypes.borrow_mut().insert(name, prototype.encode()));
}

pub(super) fn prototype_of(name: &str) -> JSValue {
    PROTOTYPES.with(|prototypes| prototypes.borrow().get(name).copied()).map_or_else(JSValue::undefined, JSValue::decode)
}

/// `value` é um `ReadableStream`.
pub(super) fn is_stream(value: JSValue) -> bool {
    stream_of(value).is_some()
}

fn stream_of(value: JSValue) -> Option<StreamRef> {
    OBJECTS.with(|objects| match objects.borrow().get(&value.encode()) {
        Some(Obj::Stream(stream)) => Some(stream.clone()),
        _ => None,
    })
}

/// O leitor de `value`, só se for do tipo pedido (os membros do leitor padrão não aceitam o BYOB e vice-versa).
fn reader_of(value: JSValue, kind: ReaderKind) -> Option<ReaderRef> {
    OBJECTS.with(|objects| match objects.borrow().get(&value.encode()) {
        Some(Obj::Reader(reader)) if reader.borrow().kind == kind => Some(reader.clone()),
        _ => None,
    })
}

fn controller_of(value: JSValue) -> Option<ControllerRef> {
    OBJECTS.with(|objects| match objects.borrow().get(&value.encode()) {
        Some(Obj::Controller(controller)) => Some(controller.clone()),
        _ => None,
    })
}

fn byte_of(value: JSValue) -> Option<ByteRef> {
    OBJECTS.with(|objects| match objects.borrow().get(&value.encode()) {
        Some(Obj::ByteController(state)) => Some(state.clone()),
        _ => None,
    })
}

fn request_of(value: JSValue) -> Option<RequestRef> {
    OBJECTS.with(|objects| match objects.borrow().get(&value.encode()) {
        Some(Obj::Request(request)) => Some(request.clone()),
        _ => None,
    })
}

fn register(value: JSValue, object: Obj) {
    OBJECTS.with(|objects| objects.borrow_mut().insert(value.encode(), object));
}

// ---------------------------------------------------------------------------------------------
// Erros e promessas
// ---------------------------------------------------------------------------------------------

/// A exceção pendente no `VM`, retirada de lá.
pub(super) fn take_error(global_object: &JSGlobalObject) -> JSValue {
    let vm = global_object.vm();
    let error = vm.exception().map_or_else(JSValue::undefined, |exception| exception.value());
    vm.clear_exception();
    error
}

/// O valor do erro que `thrown` acabou de lançar.
pub(super) fn error_of(global_object: &JSGlobalObject, _thrown: Thrown) -> JSValue {
    take_error(global_object)
}

pub(super) fn invalid_state(global_object: &JSGlobalObject, message: &str) -> JSValue {
    let thrown = throw_coded_type_error(global_object, &format!("Invalid state: {message}"), "ERR_INVALID_STATE");
    error_of(global_object, thrown)
}

pub(super) fn type_error_value(global_object: &JSGlobalObject, message: &str) -> JSValue {
    let thrown = throw_native_type_error(global_object, message);
    error_of(global_object, thrown)
}

pub(super) fn range_error_value(global_object: &JSGlobalObject, message: &str) -> JSValue {
    throw_thrown(global_object, Thrown::range_error(message));
    take_error(global_object)
}

/// Lança de novo um valor já retirado do `VM`.
pub(super) fn rethrow(global_object: &JSGlobalObject, error: JSValue) -> Thrown {
    let mut scope = ThrowScope::new(global_object.vm());
    throw_exception(global_object, &mut scope, error);
    Thrown::Pending
}

pub(super) fn invalid_this(global_object: &JSGlobalObject, class_name: &str) -> Thrown {
    throw_coded_type_error(global_object, &format!("Value of \"this\" must be of type {class_name}"), "ERR_INVALID_THIS")
}

pub(super) fn call_js(global_object: &JSGlobalObject, function: JSValue, this_value: JSValue, arguments: &[JSValue]) -> Result<JSValue, JSValue> {
    call_function(global_object, function, this_value, arguments).ok_or_else(|| take_error(global_object))
}

pub(super) fn new_promise(global_object: &JSGlobalObject) -> JSValue {
    JSPromise::create(global_object.vm(), &global_object.promise_structure()).as_value()
}

pub(super) fn resolve(global_object: &JSGlobalObject, promise: JSValue, value: JSValue) {
    if let Some(promise) = JSPromise::from_value(&promise) {
        promise.resolve(global_object, value);
    }
}

pub(super) fn reject(global_object: &JSGlobalObject, promise: JSValue, value: JSValue) {
    if let Some(promise) = JSPromise::from_value(&promise) {
        promise.reject(global_object, value);
    }
}

pub(super) fn mark_handled(promise: JSValue) {
    if let Some(promise) = JSPromise::from_value(&promise) {
        promise.mark_as_handled();
    }
}

/// `promiseResolvedWith`: uma promessa nativa passa como está, o resto vira uma promessa resolvida com o valor.
pub(super) fn resolved_with(global_object: &JSGlobalObject, value: JSValue) -> JSValue {
    if JSPromise::from_value(&value).is_some() {
        return value;
    }
    let promise = new_promise(global_object);
    resolve(global_object, promise, value);
    promise
}

/// `promiseResolvedWith` do bun: sempre uma promessa nova resolvida com o valor, então uma promessa de `start` entra pela
/// tarefa do thenable (dois tempos a mais que `resolved_with`, que devolve a própria promessa; medido no oráculo com o
/// `TransformStream`, cujo `start` interno é uma promessa).
pub(super) fn fresh_resolved_with(global_object: &JSGlobalObject, value: JSValue) -> JSValue {
    let promise = new_promise(global_object);
    resolve(global_object, promise, value);
    promise
}

pub(super) fn rejected_with(global_object: &JSGlobalObject, error: JSValue) -> JSValue {
    JSPromise::rejected_promise(global_object, error).as_value()
}

/// `performPromiseThen(promise, onFulfilled, onRejected, result)`.
pub(super) fn then(global_object: &JSGlobalObject, promise: JSValue, on_fulfilled: JSValue, on_rejected: JSValue, result: JSValue) {
    if let Some(promise) = JSPromise::from_value(&promise) {
        promise.perform_promise_then(global_object, on_fulfilled, on_rejected, result);
    }
}

fn reaction_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let handler = HANDLERS.with(|handlers| handlers.borrow().get(&call.callee()).map(|handler| handler.run.clone()));
    if let Some(handler) = handler {
        handler(global_object, call.argument(0))?;
    }
    Ok(JSValue::undefined())
}
host_function!(reaction_native, reaction_body);

/// Uma função nativa que roda `handler(argument(0))` e só captura estado de stream registrado num `OBJECTS` (que o
/// coletor já visita): o `jsWebStreamsHandler_*` com o contexto no fecho.
pub(super) fn reaction(global_object: &JSGlobalObject, handler: impl Fn(&JSGlobalObject, JSValue) -> Result<(), Thrown> + 'static) -> JSValue {
    reaction_holding(global_object, Vec::new(), handler)
}

/// Como [`reaction`], para a closure que captura valores JS ou estado fora de um `OBJECTS`: `held` lista o que ela
/// captura, e o coletor o visita enquanto a função da reação viver (o contexto nos slots da função, no C++).
pub(super) fn reaction_holding(
    global_object: &JSGlobalObject,
    held: Vec<Rc<dyn Traced>>,
    handler: impl Fn(&JSGlobalObject, JSValue) -> Result<(), Thrown> + 'static,
) -> JSValue {
    let function = JSFunction::create_native(
        global_object.vm(),
        global_object,
        1,
        &WtfString::from_latin1(b""),
        reaction_native,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    );
    let value = function.as_value();
    if let JSValue::Cell(cell_id) = value {
        HANDLERS.with(|handlers| handlers.borrow_mut().insert(cell_id, Handler { run: Rc::new(handler), held }));
    }
    value
}

/// `invokeCallbackReturningPromise`: a chamada que lança vira promessa rejeitada.
pub(super) fn invoke_returning_promise(global_object: &JSGlobalObject, function: JSValue, this_value: JSValue, arguments: &[JSValue]) -> JSValue {
    match call_js(global_object, function, this_value, arguments) {
        Ok(value) => resolved_with(global_object, value),
        Err(error) => rejected_with(global_object, error),
    }
}

// ---------------------------------------------------------------------------------------------
// ReadableStream: operações
// ---------------------------------------------------------------------------------------------

fn has_pending_reads(stream: &StreamRef) -> bool {
    let reader = stream.borrow().reader.clone();
    let Some(reader) = reader else { return false };
    let empty = reader.borrow().requests.is_empty();
    !empty
}

/// `ReadableStreamFulfillReadRequest(stream, chunk, done)`.
fn fulfill_read_request(global_object: &JSGlobalObject, stream: &StreamRef, chunk: JSValue, done: bool) {
    let reader = stream.borrow().reader.clone();
    let Some(reader) = reader else { return };
    let request = reader.borrow_mut().requests.pop_front();
    if let Some(promise) = request {
        let result = create_iterator_result_object(global_object, if done { JSValue::undefined() } else { chunk }, done);
        resolve(global_object, promise, result);
    }
}

/// `ReadableStreamClose(stream)`.
fn stream_close(global_object: &JSGlobalObject, stream: &StreamRef) {
    stream.borrow_mut().state = State::Closed;
    let reader = stream.borrow().reader.clone();
    let Some(reader) = reader else { return };
    let closed = reader.borrow().closed;
    resolve(global_object, closed, JSValue::undefined());
    let requests: Vec<JSValue> = reader.borrow_mut().requests.drain(..).collect();
    for promise in requests {
        let result = create_iterator_result_object(global_object, JSValue::undefined(), true);
        resolve(global_object, promise, result);
    }
}

/// `ReadableStreamError(stream, e)`.
fn stream_error(global_object: &JSGlobalObject, stream: &StreamRef, error: JSValue) {
    {
        let mut inner = stream.borrow_mut();
        inner.state = State::Errored;
        inner.stored_error = error;
    }
    let reader = stream.borrow().reader.clone();
    let Some(reader) = reader else { return };
    let closed = reader.borrow().closed;
    reject(global_object, closed, error);
    mark_handled(closed);
    let requests: Vec<JSValue> = reader.borrow_mut().requests.drain(..).collect();
    for promise in requests {
        reject(global_object, promise, error);
    }
}

/// `ReadableStreamCancel(stream, reason)`: devolve a promessa.
fn stream_cancel(global_object: &JSGlobalObject, stream: &StreamRef, reason: JSValue) -> JSValue {
    stream.borrow_mut().disturbed = true;
    let state = stream.borrow().state;
    match state {
        State::Closed => return resolved_with(global_object, JSValue::undefined()),
        State::Errored => {
            let error = stream.borrow().stored_error;
            return rejected_with(global_object, error);
        }
        State::Readable => {}
    }
    stream_close(global_object, stream);
    let controller = stream.borrow().controller.clone();
    let byte_controller = stream.borrow().byte_controller.clone();
    let source_cancel = match (controller, byte_controller) {
        (Some(controller), _) => controller_cancel_steps(global_object, &controller, reason),
        (None, Some(state)) => byte_cancel_steps(global_object, &state, reason),
        (None, None) => resolved_with(global_object, JSValue::undefined()),
    };
    let result = new_promise(global_object);
    then(global_object, source_cancel, reaction(global_object, |_, _| Ok(())), JSValue::undefined(), result);
    result
}

// ---------------------------------------------------------------------------------------------
// ReadableStreamDefaultController: operações
// ---------------------------------------------------------------------------------------------

fn can_close_or_enqueue(controller: &ControllerRef) -> bool {
    let inner = controller.borrow();
    let state = inner.stream.borrow().state;
    !inner.close_requested && state == State::Readable
}

fn desired_size(controller: &ControllerRef) -> Option<f64> {
    let inner = controller.borrow();
    let state = inner.stream.borrow().state;
    match state {
        State::Errored => None,
        State::Closed => Some(0.0),
        State::Readable => Some(inner.high_water_mark - inner.total),
    }
}

fn should_call_pull(controller: &ControllerRef) -> bool {
    if !can_close_or_enqueue(controller) || !controller.borrow().started {
        return false;
    }
    let stream = controller.borrow().stream.clone();
    if has_pending_reads(&stream) {
        return true;
    }
    desired_size(controller).is_some_and(|size| size > 0.0)
}

/// `ReadableStreamDefaultControllerClearAlgorithms`.
fn clear_algorithms(controller: &ControllerRef) {
    let mut inner = controller.borrow_mut();
    inner.algorithms = None;
    inner.size = None;
}

fn reset_queue(controller: &ControllerRef) {
    let mut inner = controller.borrow_mut();
    inner.queue.clear();
    inner.total = 0.0;
}

fn controller_cancel_steps(global_object: &JSGlobalObject, controller: &ControllerRef, reason: JSValue) -> JSValue {
    reset_queue(controller);
    let algorithms = controller.borrow().algorithms.clone();
    let result = match algorithms {
        Some(algorithms) => algorithms.run_cancel(global_object, reason),
        None => resolved_with(global_object, JSValue::undefined()),
    };
    clear_algorithms(controller);
    result
}

/// `ReadableStreamDefaultControllerCallPullIfNeeded`.
fn call_pull_if_needed(global_object: &JSGlobalObject, controller: &ControllerRef) -> Result<(), Thrown> {
    if !should_call_pull(controller) {
        return Ok(());
    }
    {
        let mut inner = controller.borrow_mut();
        if inner.pulling {
            inner.pull_again = true;
            return Ok(());
        }
        inner.pulling = true;
    }
    let (algorithms, value) = {
        let inner = controller.borrow();
        (inner.algorithms.clone(), inner.value)
    };
    let for_fulfilled = controller.clone();
    let on_fulfilled = reaction(global_object, move |global_object, _| {
        let again = {
            let mut inner = for_fulfilled.borrow_mut();
            inner.pulling = false;
            std::mem::take(&mut inner.pull_again)
        };
        if again {
            call_pull_if_needed(global_object, &for_fulfilled)?;
        }
        Ok(())
    });
    let for_rejected = controller.clone();
    let on_rejected = reaction(global_object, move |global_object, error| controller_error(global_object, &for_rejected, error));
    // As duas reações só vivem neste local enquanto o `pull` do usuário roda JS.
    let _reactions = RootScope::new(&[on_fulfilled, on_rejected]);
    let pull_promise = match algorithms {
        Some(algorithms) => algorithms.run_pull(global_object, value),
        None => resolved_with(global_object, JSValue::undefined()),
    };
    then(global_object, pull_promise, on_fulfilled, on_rejected, JSValue::undefined());
    Ok(())
}

/// `ReadableStreamDefaultControllerClose`.
fn controller_close(global_object: &JSGlobalObject, controller: &ControllerRef) {
    if !can_close_or_enqueue(controller) {
        return;
    }
    controller.borrow_mut().close_requested = true;
    if controller.borrow().queue.is_empty() {
        clear_algorithms(controller);
        let stream = controller.borrow().stream.clone();
        stream_close(global_object, &stream);
    }
}

/// `ReadableStreamDefaultControllerError`.
fn controller_error(global_object: &JSGlobalObject, controller: &ControllerRef, error: JSValue) -> Result<(), Thrown> {
    let stream = controller.borrow().stream.clone();
    if stream.borrow().state != State::Readable {
        return Ok(());
    }
    reset_queue(controller);
    clear_algorithms(controller);
    stream_error(global_object, &stream, error);
    Ok(())
}

/// `ReadableStreamDefaultControllerEnqueue`.
fn controller_enqueue(global_object: &JSGlobalObject, controller: &ControllerRef, chunk: JSValue) -> Result<(), Thrown> {
    if !can_close_or_enqueue(controller) {
        return Ok(());
    }
    let stream = controller.borrow().stream.clone();
    if has_pending_reads(&stream) {
        fulfill_read_request(global_object, &stream, chunk, false);
    } else {
        let size_function = controller.borrow().size;
        let mut failure: Option<JSValue> = None;
        let mut chunk_size = 1.0;
        if let Some(function) = size_function {
            match call_js(global_object, function, JSValue::undefined(), &[chunk]) {
                Ok(value) => match to_number_checked(global_object, value) {
                    Ok(number) => chunk_size = number,
                    Err(_) => failure = Some(take_error(global_object)),
                },
                Err(error) => failure = Some(error),
            }
        }
        if failure.is_none() && (!chunk_size.is_finite() || chunk_size < 0.0) {
            failure = Some(range_error_value(global_object, "The queuing strategy's chunk size must be a non-negative, finite number"));
        }
        if let Some(error) = failure {
            controller_error(global_object, controller, error)?;
            return Err(rethrow(global_object, error));
        }
        let mut inner = controller.borrow_mut();
        inner.queue.push_back((chunk, chunk_size));
        inner.total += chunk_size;
    }
    call_pull_if_needed(global_object, controller)
}

/// `dequeueChunkForRead`: a fila não está vazia.
fn dequeue_chunk_for_read(global_object: &JSGlobalObject, controller: &ControllerRef) -> Result<JSValue, Thrown> {
    let (chunk, close_requested, now_empty) = {
        let mut inner = controller.borrow_mut();
        let (chunk, size) = inner.queue.pop_front().unwrap_or((JSValue::undefined(), 0.0));
        inner.total -= size;
        if inner.queue.is_empty() {
            inner.total = 0.0;
        }
        (chunk, inner.close_requested, inner.queue.is_empty())
    };
    if close_requested && now_empty {
        clear_algorithms(controller);
        let stream = controller.borrow().stream.clone();
        stream_close(global_object, &stream);
    } else {
        call_pull_if_needed(global_object, controller)?;
    }
    Ok(chunk)
}

// ---------------------------------------------------------------------------------------------
// ReadableByteStreamController: operações
// ---------------------------------------------------------------------------------------------

/// O tipo do leitor atual e quantos pedidos de leitura ele tem pendentes.
fn reader_state(stream: &StreamRef) -> (ReaderKind, usize) {
    match stream.borrow().reader.clone() {
        Some(reader) => (reader.borrow().kind, reader.borrow().requests.len()),
        None => (ReaderKind::None, 0),
    }
}

/// Empresta o [`ByteCore`] a `operation`, junta as [`Action`]s que ela produziu e as aplica com [`drain`].
pub(super) fn run_core<R>(
    global_object: &JSGlobalObject,
    state: &ByteRef,
    operation: impl FnOnce(&mut ByteCore, &mut Vec<Action>) -> R,
) -> Result<R, Thrown> {
    let result = {
        let mut inner = state.borrow_mut();
        let mut out = Vec::new();
        let result = operation(&mut inner.core, &mut out);
        inner.effects.extend(out);
        result
    };
    drain(global_object, state)?;
    Ok(result)
}

/// Um `Uint8Array` novo sobre os bytes do chunk (a arena do núcleo é dona do buffer do chunk).
fn chunk_view(global_object: &JSGlobalObject, state: &ByteRef, chunk: Chunk) -> Result<JSValue, Thrown> {
    let bytes = state.borrow().core.buffers.bytes(chunk.buffer).get(chunk.offset..chunk.offset + chunk.length).unwrap_or(&[]).to_vec();
    uint8_array_from(global_object, &bytes)
}

/// `ReadableByteStreamControllerCallPullIfNeeded`.
pub(super) fn byte_call_pull_if_needed(global_object: &JSGlobalObject, state: &ByteRef) -> Result<(), Thrown> {
    let stream = state.borrow().stream.clone();
    let (kind, requests) = reader_state(&stream);
    {
        let mut inner = state.borrow_mut();
        if !inner.core.should_call_pull(kind, requests) {
            return Ok(());
        }
        if inner.core.pulling {
            inner.core.pull_again = true;
            return Ok(());
        }
        inner.core.pulling = true;
    }
    let (algorithms, value) = {
        let inner = state.borrow();
        (inner.algorithms.clone(), inner.value)
    };
    let for_fulfilled = state.clone();
    let on_fulfilled = reaction(global_object, move |global_object, _| {
        let again = {
            let mut inner = for_fulfilled.borrow_mut();
            inner.core.pulling = false;
            std::mem::take(&mut inner.core.pull_again)
        };
        if again {
            byte_call_pull_if_needed(global_object, &for_fulfilled)?;
        }
        Ok(())
    });
    let for_rejected = state.clone();
    let on_rejected = reaction(global_object, move |global_object, error| byte_controller_error(global_object, &for_rejected, error));
    let _reactions = RootScope::new(&[on_fulfilled, on_rejected]);
    let pull_promise = match algorithms {
        Some(algorithms) => algorithms.run_pull(global_object, value),
        None => resolved_with(global_object, JSValue::undefined()),
    };
    then(global_object, pull_promise, on_fulfilled, on_rejected, JSValue::undefined());
    Ok(())
}

/// `ReadableByteStreamControllerError`.
pub(super) fn byte_controller_error(global_object: &JSGlobalObject, state: &ByteRef, error: JSValue) -> Result<(), Thrown> {
    let stream = state.borrow().stream.clone();
    if stream.borrow().state != State::Readable {
        return Ok(());
    }
    {
        let mut inner = state.borrow_mut();
        inner.core.clear_pending_pull_intos();
        inner.core.reset_queue();
        inner.core.errored = true;
        inner.algorithms = None;
    }
    invalidate_request(state);
    stream_error(global_object, &stream, error);
    Ok(())
}

/// O `[[CancelSteps]]` do controlador de bytes (o stream já foi fechado por `ReadableStreamCancel`).
fn byte_cancel_steps(global_object: &JSGlobalObject, state: &ByteRef, reason: JSValue) -> JSValue {
    let algorithms = {
        let mut inner = state.borrow_mut();
        inner.core.closed = true;
        inner.core.clear_pending_pull_intos();
        inner.core.reset_queue();
        inner.algorithms.take()
    };
    invalidate_request(state);
    match algorithms {
        Some(algorithms) => algorithms.run_cancel(global_object, reason),
        None => resolved_with(global_object, JSValue::undefined()),
    }
}

/// O `[[PullSteps]]` do controlador de bytes para o leitor padrão: com fila, resolve a leitura na hora com um
/// `Uint8Array`; sem fila, guarda o pedido e puxa (com `autoAllocateChunkSize`, o `pull` ganha um `byobRequest`
/// sobre um buffer novo).
fn byte_pull_steps(global_object: &JSGlobalObject, state: &ByteRef, reader: &ReaderRef, promise: JSValue) -> Result<(), Thrown> {
    if let Some(chunk) = run_core(global_object, state, |core, out| core.dequeue_for_default_read(out))? {
        let view = chunk_view(global_object, state, chunk)?;
        let result = create_iterator_result_object(global_object, view, false);
        resolve(global_object, promise, result);
    } else {
        let auto_allocate = state.borrow().core.auto_allocate_chunk_size;
        if let Some(size) = auto_allocate {
            state.borrow_mut().core.push_auto_allocate(size, TypedArrayType::Uint8 as u32);
        }
        reader.borrow_mut().requests.push_back(promise);
    }
    byte_call_pull_if_needed(global_object, state)
}

/// O que o `PullInto` e o `enqueue` precisam de uma `ArrayBufferView` (`TypedArray` ou `DataView`).
struct ViewInfo {
    buffer: ArrayBufferRef,
    kind: TypedArrayType,
    byte_offset: usize,
    byte_length: usize,
    /// Elementos da view (`byteLength` para o `DataView`).
    length: usize,
    /// Desanexada, de buffer desanexado ou de buffer que não se transfere.
    detached: bool,
}

/// A [`ViewInfo`] de `value`; `None` se `value` não é uma view.
fn view_info(value: JSValue) -> Option<ViewInfo> {
    let (buffer, kind, byte_offset, byte_length, view_detached) = if let Some(view) = JSGenericTypedArrayView::from_value(&value) {
        (view.possibly_shared_buffer(), view.typed_array_type(), view.byte_offset(), view.byte_length(), view.is_detached())
    } else {
        let view = JSDataView::from_value(&value)?;
        let byte_length = view.view_byte_length();
        (view.possibly_shared_buffer().clone(), TypedArrayType::DataView, view.byte_offset_raw(), byte_length.unwrap_or(0), byte_length.is_none())
    };
    let detached = view_detached || buffer.is_detached() || !buffer.is_detachable();
    Some(ViewInfo { buffer, kind, byte_offset, byte_length, length: byte_length / kind.element_size(), detached })
}

/// Os bytes de uma `ArrayBufferView` (`TypedArray` ou `DataView`) e o buffer dela; `None` se `value` não é uma view.
/// `Err(())` é a view de zero bytes, desanexada ou de buffer não destacável.
fn view_contents(value: JSValue) -> Option<Result<(Vec<u8>, ArrayBufferRef), ()>> {
    let info = view_info(value)?;
    if info.detached || info.byte_length == 0 {
        return Some(Err(()));
    }
    let end = info.byte_offset + info.byte_length;
    let data = info.buffer.with_bytes(|bytes| bytes.get(info.byte_offset..end).map(<[u8]>::to_vec));
    Some(data.map(|data| (data, info.buffer)).ok_or(()))
}

fn this_byte_controller(global_object: &JSGlobalObject, call: &HostCall) -> Result<ByteRef, Thrown> {
    byte_of(call.this_value()).ok_or_else(|| invalid_this(global_object, "ReadableByteStreamController"))
}

fn already_closed(global_object: &JSGlobalObject) -> Thrown {
    throw_coded_type_error(global_object, "Invalid state: ReadableStream is already closed", "ERR_INVALID_STATE")
}

pub(super) fn bc_desired_size(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let state = this_byte_controller(global_object, call)?;
    let size = state.borrow().core.desired_size();
    Ok(size.map_or_else(JSValue::null, js_number))
}

/// `byobRequest`: o objeto persiste enquanto o `pullInto` da frente for o mesmo; `null` sem `pullInto` pendente. A view
/// é sempre um `Uint8Array` sobre uma cópia do buffer do descritor (o que o usuário escreve volta por
/// [`sync_request_to_core`]).
pub(super) fn bc_byob_request(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let state = this_byte_controller(global_object, call)?;
    byob_request_value(global_object, &state)
}

/// O getter `byobRequest` de um controlador de bytes.
pub(super) fn byob_request_value(global_object: &JSGlobalObject, state: &ByteRef) -> HostResult {
    let existing = state.borrow().request.clone();
    if let Some(request) = existing {
        return Ok(request.borrow().value);
    }
    let front = state.borrow().core.byob_request_view();
    let Some((id, offset, length, _)) = front else { return Ok(JSValue::null()) };
    let data = state.borrow().core.buffers.bytes(id).to_vec();
    let buffer = ArrayBuffer::create_from_bytes(data);
    let structure = global_object.array_buffer_realm.typed_arrays.structure(TypedArrayType::Uint8, false);
    let view = JSGenericTypedArrayView::create_with_buffer(global_object, &structure, buffer.clone(), offset, Some(length))?.as_value();
    let request_structure = instance_structure(global_object.vm(), Some(global_object), prototype_of("ReadableStreamBYOBRequest"));
    let value = JSFinalObject::create(global_object.vm(), &request_structure).as_value();
    let request = Rc::new(RefCell::new(ByobRequest { value, controller: Some(state.clone()), view: Some(view), buffer: Some(buffer) }));
    register(value, Obj::Request(request.clone()));
    state.borrow_mut().request = Some(request);
    Ok(value)
}

fn this_request(global_object: &JSGlobalObject, call: &HostCall) -> Result<RequestRef, Thrown> {
    request_of(call.this_value()).ok_or_else(|| invalid_this(global_object, "ReadableStreamBYOBRequest"))
}

/// O controlador de um `byobRequest` ainda válido; invalidado, `respond` lança.
fn request_controller(global_object: &JSGlobalObject, request: &RequestRef) -> Result<ByteRef, Thrown> {
    let controller = request.borrow().controller.clone();
    controller.ok_or_else(|| rethrow(global_object, invalid_state(global_object, msg::INVALIDATED)))
}

pub(super) fn bq_view(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let request = this_request(global_object, call)?;
    let view = request.borrow().view;
    Ok(view.unwrap_or_else(JSValue::null))
}

/// O erro de `respond`/`respondWithNewView` para a mensagem que o núcleo devolveu; `received` é o texto que o bun
/// acrescenta às mensagens da view nova.
fn respond_failure(global_object: &JSGlobalObject, message: &str, received: &str) -> Thrown {
    match message {
        msg::INVALIDATED => rethrow(global_object, invalid_state(global_object, message)),
        msg::RESPOND_TOO_MANY => Thrown::range_error(message),
        msg::NEW_VIEW_LENGTH | msg::NEW_VIEW_POSITION => throw_coded_range_error(global_object, &format!("{message} Received {received}"), "ERR_INVALID_ARG_VALUE"),
        _ => throw_native_type_error(global_object, message),
    }
}

/// O fim de `respond` e `respondWithNewView`: o núcleo já fez o trabalho; o `pull` pode ser preciso de novo.
fn finish_respond(global_object: &JSGlobalObject, state: &ByteRef, outcome: Result<(), &'static str>, received: &str) -> HostResult {
    outcome.map_err(|message| respond_failure(global_object, message, received))?;
    byte_call_pull_if_needed(global_object, state)?;
    Ok(JSValue::undefined())
}

pub(super) fn bq_respond(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let request = this_request(global_object, call)?;
    let bytes_written = enforce_range_u64(global_object, call.argument(0))?;
    let state = request_controller(global_object, &request)?;
    byte_respond(global_object, &state, bytes_written)
}

/// `ReadableByteStreamControllerRespond(controller, bytesWritten)`.
pub(super) fn byte_respond(global_object: &JSGlobalObject, state: &ByteRef, bytes_written: usize) -> HostResult {
    sync_request_to_core(state);
    let outcome = run_core(global_object, state, |core, out| core.respond(bytes_written, out))?;
    finish_respond(global_object, state, outcome, "")
}

/// Meia precisão (IEEE 754 binary16) para `f64`, para o `Float16Array`.
fn half_to_f64(bits: u16) -> f64 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = i32::from((bits >> 10) & 0x1f);
    let fraction = f64::from(bits & 0x3ff);
    match exponent {
        0 => sign * fraction * 2f64.powi(-24),
        0x1f if fraction == 0.0 => sign * f64::INFINITY,
        0x1f => f64::NAN,
        _ => sign * (1.0 + fraction / 1024.0) * 2f64.powi(exponent - 15),
    }
}

/// Um elemento da view como o `util.inspect` do bun o mostra: inteiros e números como `String(n)` (o `-0` sai `0`),
/// `BigInt` com o sufixo `n`.
fn received_element(kind: TypedArrayType, raw: &[u8]) -> String {
    let number = |value: f64| rust_string(&WtfString::number_f64(value));
    match kind {
        TypedArrayType::Int8 => (raw[0] as i8).to_string(),
        TypedArrayType::Int16 => i16::from_le_bytes([raw[0], raw[1]]).to_string(),
        TypedArrayType::Uint16 => u16::from_le_bytes([raw[0], raw[1]]).to_string(),
        TypedArrayType::Int32 => i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]).to_string(),
        TypedArrayType::Uint32 => u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]).to_string(),
        TypedArrayType::Float16 => number(half_to_f64(u16::from_le_bytes([raw[0], raw[1]]))),
        TypedArrayType::Float32 => number(f64::from(f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))),
        TypedArrayType::Float64 => number(f64::from_le_bytes([raw[0], raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7]])),
        TypedArrayType::BigInt64 => format!("{}n", i64::from_le_bytes([raw[0], raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7]])),
        TypedArrayType::BigUint64 => format!("{}n", u64::from_le_bytes([raw[0], raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7]])),
        _ => raw[0].to_string(),
    }
}

/// O texto `Received ...` de uma view como o `util.inspect` do bun a mostra (`Uint8Array(3) [ 4, 5, 6 ]`,
/// `Float64Array(2) [ 1.5, 2 ]`, `BigInt64Array(1) [ 1n ]`, `DataView(2) [ 0, 0 ]` com os bytes), sem o corte de 100
/// itens do Node (medido no bun 1.4.2 com 105 elementos).
fn received_view(info: &ViewInfo) -> String {
    let name = match info.kind {
        TypedArrayType::Int8 => "Int8Array",
        TypedArrayType::Uint8 => "Uint8Array",
        TypedArrayType::Uint8Clamped => "Uint8ClampedArray",
        TypedArrayType::Int16 => "Int16Array",
        TypedArrayType::Uint16 => "Uint16Array",
        TypedArrayType::Int32 => "Int32Array",
        TypedArrayType::Uint32 => "Uint32Array",
        TypedArrayType::Float16 => "Float16Array",
        TypedArrayType::Float32 => "Float32Array",
        TypedArrayType::Float64 => "Float64Array",
        TypedArrayType::BigInt64 => "BigInt64Array",
        TypedArrayType::BigUint64 => "BigUint64Array",
        _ => "DataView",
    };
    let end = info.byte_offset + info.byte_length;
    let bytes = info.buffer.with_bytes(|bytes| bytes.get(info.byte_offset..end).map(<[u8]>::to_vec).unwrap_or_default());
    // O `DataView` mostra os bytes (elemento de um byte); `length` já é o número de elementos da view.
    let step = if info.kind == TypedArrayType::DataView { 1 } else { info.kind.element_size() };
    let element_kind = if info.kind == TypedArrayType::DataView { TypedArrayType::Uint8 } else { info.kind };
    let shown: Vec<String> = bytes.chunks_exact(step).map(|raw| received_element(element_kind, raw)).collect();
    if shown.is_empty() {
        format!("{name}(0) []")
    } else {
        format!("{name}({}) [ {} ]", shown.len(), shown.join(", "))
    }
}

pub(super) fn bq_respond_with_new_view(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let request = this_request(global_object, call)?;
    respond_with_new_view_on(global_object, &request, call.argument(0))
}

/// `respondWithNewView(value)` sobre o `byobRequest` dado.
pub(super) fn respond_with_new_view_on(global_object: &JSGlobalObject, request: &RequestRef, value: JSValue) -> HostResult {
    let Some(info) = view_info(value) else { return Err(invalid_buffer_argument(global_object, "view", value)) };
    let state = request_controller(global_object, request)?;
    if info.detached {
        return Err(throw_native_type_error(global_object, msg::DETACHED));
    }
    sync_request_to_core(&state);
    let received = received_view(&info);
    let data = info.buffer.with_bytes(<[u8]>::to_vec);
    let outcome = run_core(global_object, &state, |core, out| core.respond_with_new_view(data, info.byte_offset, info.byte_length, out))?;
    if outcome.is_ok() {
        // `TransferArrayBuffer`: o buffer da view passa para o núcleo.
        info.buffer.detach();
    }
    finish_respond(global_object, &state, outcome, &received)
}

/// `TypeError ERR_INVALID_ARG_TYPE` de um argumento que devia ser `Buffer`, `TypedArray` ou `DataView`.
fn invalid_buffer_argument(global_object: &JSGlobalObject, name: &str, value: JSValue) -> Thrown {
    let received = if value.is_undefined() { "undefined".to_string() } else { describe_received(global_object, value).unwrap_or_default() };
    let message = format!("The \"{name}\" argument must be an instance of Buffer, TypedArray, or DataView. Received {received}");
    throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE")
}

pub(super) fn bc_close(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let state = this_byte_controller(global_object, call)?;
    byte_close(global_object, &state)?;
    Ok(JSValue::undefined())
}

/// `ReadableByteStreamControllerClose` com a validação de `close()`.
pub(super) fn byte_close(global_object: &JSGlobalObject, state: &ByteRef) -> Result<(), Thrown> {
    if !state.borrow().core.can_close_or_enqueue() {
        return Err(already_closed(global_object));
    }
    if let Err(message) = run_core(global_object, state, |core, out| core.close(out))? {
        let error = type_error_value(global_object, message);
        byte_controller_error(global_object, state, error)?;
        return Err(rethrow(global_object, error));
    }
    Ok(())
}

pub(super) fn bc_error(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let state = this_byte_controller(global_object, call)?;
    byte_controller_error(global_object, &state, call.argument(0))?;
    Ok(JSValue::undefined())
}

pub(super) fn bc_enqueue(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let state = this_byte_controller(global_object, call)?;
    if call.argument_count() == 0 {
        return Err(throw_native_type_error(global_object, "Not enough arguments"));
    }
    byte_enqueue_view(global_object, &state, call.argument(0))
}

/// `ReadableByteStreamControllerEnqueue(controller, chunk)` com as validações de `enqueue()`.
pub(super) fn byte_enqueue_view(global_object: &JSGlobalObject, state: &ByteRef, chunk: JSValue) -> HostResult {
    let contents = match view_contents(chunk) {
        Some(contents) => contents,
        None => return Err(invalid_buffer_argument(global_object, "buffer", chunk)),
    };
    let Ok((data, buffer)) = contents else {
        return Err(throw_coded_type_error(global_object, "Invalid state: chunk ArrayBuffer is zero-length or detached", "ERR_INVALID_STATE"));
    };
    if !state.borrow().core.can_close_or_enqueue() {
        return Err(already_closed(global_object));
    }
    // O que o usuário escreveu no `byobRequest` entra no buffer do `pullInto` antes de o chunk preenchê-lo.
    sync_request_to_core(state);
    // `TransferArrayBuffer`: o dono do buffer passa a ser o núcleo, o do usuário fica desanexado.
    buffer.detach();
    let stream = state.borrow().stream.clone();
    let (kind, requests) = reader_state(&stream);
    run_core(global_object, state, |core, out| out.extend(core.enqueue(data, kind, requests)))?;
    byte_call_pull_if_needed(global_object, state)?;
    Ok(JSValue::undefined())
}

// ---------------------------------------------------------------------------------------------
// Leitor
// ---------------------------------------------------------------------------------------------

/// `ReadableStreamDefaultReaderRead(reader, readRequest)` com um pedido do tipo `Promise`.
fn reader_read_request(global_object: &JSGlobalObject, stream: &StreamRef, reader: &ReaderRef, promise: JSValue) -> Result<(), Thrown> {
    stream.borrow_mut().disturbed = true;
    let state = stream.borrow().state;
    match state {
        State::Closed => {
            let result = create_iterator_result_object(global_object, JSValue::undefined(), true);
            resolve(global_object, promise, result);
        }
        State::Errored => {
            let error = stream.borrow().stored_error;
            reject(global_object, promise, error);
        }
        State::Readable => {
            let controller = stream.borrow().controller.clone();
            let byte_controller = stream.borrow().byte_controller.clone();
            match (controller, byte_controller) {
                (Some(controller), _) => {
                    if !controller.borrow().queue.is_empty() {
                        let chunk = dequeue_chunk_for_read(global_object, &controller)?;
                        let result = create_iterator_result_object(global_object, chunk, false);
                        resolve(global_object, promise, result);
                    } else {
                        reader.borrow_mut().requests.push_back(promise);
                        call_pull_if_needed(global_object, &controller)?;
                    }
                }
                (None, Some(state)) => byte_pull_steps(global_object, &state, reader, promise)?,
                (None, None) => reader.borrow_mut().requests.push_back(promise),
            }
        }
    }
    Ok(())
}

/// `ReadableStreamReaderGenericRelease` seguido de `ReadableStreamDefaultReaderErrorReadRequests`.
fn reader_release(global_object: &JSGlobalObject, reader: &ReaderRef) {
    let Some(stream) = reader.borrow().stream.clone() else { return };
    let release_error = invalid_state(global_object, "Reader released");
    if stream.borrow().state == State::Readable {
        let closed = reader.borrow().closed;
        reject(global_object, closed, release_error);
    } else {
        let rejected = rejected_with(global_object, release_error);
        reader.borrow_mut().closed = rejected;
    }
    let closed = reader.borrow().closed;
    mark_handled(closed);
    // `ReadableByteStreamControllerReleaseSteps`: os `pullInto` pendentes ficam sem leitor.
    let byte_controller = stream.borrow().byte_controller.clone();
    if let Some(state) = byte_controller {
        state.borrow_mut().core.release_pull_into_readers();
    }
    stream.borrow_mut().reader = None;
    reader.borrow_mut().stream = None;
    let error = invalid_state(global_object, "Releasing reader");
    let requests: Vec<JSValue> = reader.borrow_mut().requests.drain(..).collect();
    for promise in requests {
        reject(global_object, promise, error);
    }
}

/// `AcquireReadableStreamDefaultReader` / `AcquireReadableStreamBYOBReader` com a estrutura dada.
fn create_reader(global_object: &JSGlobalObject, stream: &StreamRef, structure: &crate::runtime::structure::StructureRef, kind: ReaderKind) -> HostResult {
    if stream.borrow().reader.is_some() {
        return Err(rethrow(global_object, invalid_state(global_object, "ReadableStream is locked")));
    }
    let value = JSFinalObject::create(global_object.vm(), structure).as_value();
    let state = stream.borrow().state;
    let closed = match state {
        State::Readable => new_promise(global_object),
        State::Closed => resolved_with(global_object, JSValue::undefined()),
        State::Errored => {
            let error = stream.borrow().stored_error;
            let promise = rejected_with(global_object, error);
            mark_handled(promise);
            promise
        }
    };
    let reader = Rc::new(RefCell::new(Reader { kind, stream: Some(stream.clone()), requests: VecDeque::new(), closed, hidden: false }));
    stream.borrow_mut().reader = Some(reader.clone());
    register(value, Obj::Reader(reader));
    Ok(value)
}

fn reader_structure(global_object: &JSGlobalObject, kind: ReaderKind) -> crate::runtime::structure::StructureRef {
    let name = if kind == ReaderKind::Byob { "ReadableStreamBYOBReader" } else { "ReadableStreamDefaultReader" };
    instance_structure(global_object.vm(), Some(global_object), prototype_of(name))
}

/// `AcquireReadableStreamBYOBReader`: só um stream de bytes tem leitor BYOB.
fn acquire_byob_reader(global_object: &JSGlobalObject, stream: &StreamRef) -> HostResult {
    if !stream.borrow().bytes {
        return Err(throw_native_type_error(global_object, "A BYOB reader requires a ReadableStream with an underlying byte source"));
    }
    create_reader(global_object, stream, &reader_structure(global_object, ReaderKind::Byob), ReaderKind::Byob)
}

/// `new ReadableStreamDefaultReader(stream)` / `new ReadableStreamBYOBReader(stream)` com um `ReadableStream` válido.
pub(super) fn construct_reader(global_object: &JSGlobalObject, call: &HostCall, kind: ReaderKind) -> Option<HostResult> {
    let stream = stream_of(call.argument(0))?;
    if kind == ReaderKind::Byob && !stream.borrow().bytes {
        return Some(acquire_byob_reader(global_object, &stream));
    }
    Some(match derived_structure(global_object, call, instance_structure) {
        Ok(structure) => create_reader(global_object, &stream, &structure, kind),
        Err(thrown) => Err(thrown),
    })
}

// ---------------------------------------------------------------------------------------------
// Construtor
// ---------------------------------------------------------------------------------------------

struct Source {
    start: Option<JSValue>,
    pull: Option<JSValue>,
    cancel: Option<JSValue>,
    bytes: bool,
    /// `autoAllocateChunkSize` já validado (`[EnforceRange] unsigned long long` maior que zero).
    auto_allocate_chunk_size: Option<usize>,
}

/// `[EnforceRange] unsigned long long` de WebIDL.
fn enforce_range_u64(global_object: &JSGlobalObject, value: JSValue) -> Result<usize, Thrown> {
    const MAX_SAFE_INTEGER: f64 = 9007199254740991.0;
    let number = to_number_checked(global_object, value)?;
    let truncated = number.trunc();
    if !number.is_finite() || !(0.0..=MAX_SAFE_INTEGER).contains(&truncated) {
        let shown = if number.is_nan() { "NaN".to_string() } else if number.is_infinite() { if number > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() } } else { format!("{number}") };
        return Err(throw_native_type_error(global_object, &format!("Value {shown} is outside the range [0, {MAX_SAFE_INTEGER:.0}]")));
    }
    Ok(truncated as usize)
}

/// `autoAllocateChunkSize`: o `[EnforceRange]` mais a regra de que zero é erro.
fn enforce_range_size(global_object: &JSGlobalObject, value: JSValue) -> Result<usize, Thrown> {
    let size = enforce_range_u64(global_object, value)?;
    if size == 0 {
        return Err(throw_native_type_error(global_object, "autoAllocateChunkSize must be greater than 0"));
    }
    Ok(size)
}

/// O trecho `Received ...` das mensagens `ERR_INVALID_ARG_*` do Node para um primitivo.
fn received_primitive(value: JSValue) -> String {
    let text = crate::runtime::js_module_loader::rust_string(&value.to_wtf_string());
    if value.is_string() {
        format!("'{text}'")
    } else {
        text
    }
}

fn convert_strategy(global_object: &JSGlobalObject, strategy: JSValue) -> Result<(Option<f64>, Option<JSValue>), Thrown> {
    if strategy.is_undefined_or_null() {
        return Ok((None, None));
    }
    if !strategy.is_object() {
        return Err(throw_native_type_error(global_object, "ReadableStream constructor takes an object as second argument, if any"));
    }
    let mut high_water_mark = None;
    let raw = get_property(global_object, strategy, "highWaterMark")?;
    if !raw.is_undefined() {
        high_water_mark = Some(to_number_checked(global_object, raw)?);
    }
    let mut size = None;
    let raw = get_property(global_object, strategy, "size")?;
    if !raw.is_undefined() {
        if !raw.is_callable() {
            return Err(throw_native_type_error(global_object, "The queuing strategy's 'size' property must be a function"));
        }
        size = Some(raw);
    }
    Ok((high_water_mark, size))
}

/// Um membro de dicionário WebIDL do tipo callback: ausente é `None`, presente e não chamável é `TypeError`
/// (`The <owner>'s '<name>' property must be a function`).
pub(super) fn callable_member(global_object: &JSGlobalObject, source: JSValue, owner: &str, name: &str) -> Result<Option<JSValue>, Thrown> {
    let member = get_property(global_object, source, name)?;
    if member.is_undefined() {
        return Ok(None);
    }
    if !member.is_callable() {
        return Err(throw_native_type_error(global_object, &format!("The {owner}'s '{name}' property must be a function")));
    }
    Ok(Some(member))
}

fn convert_source(global_object: &JSGlobalObject, underlying: JSValue) -> Result<Source, Thrown> {
    let mut source = Source { start: None, pull: None, cancel: None, bytes: false, auto_allocate_chunk_size: None };
    if underlying.is_undefined_or_null() {
        return Ok(source);
    }
    let auto_allocate = get_property(global_object, underlying, "autoAllocateChunkSize")?;
    if !auto_allocate.is_undefined() {
        source.auto_allocate_chunk_size = Some(enforce_range_size(global_object, auto_allocate)?);
    }
    source.cancel = callable_member(global_object, underlying, "underlying source", "cancel")?;
    source.pull = callable_member(global_object, underlying, "underlying source", "pull")?;
    source.start = callable_member(global_object, underlying, "underlying source", "start")?;
    let kind = get_property(global_object, underlying, "type")?;
    if !kind.is_undefined() {
        let text = crate::runtime::js_module_loader::rust_string(&kind.to_wtf_string());
        match text.as_str() {
            "bytes" => source.bytes = true,
            "direct" => {}
            other => {
                return Err(throw_native_type_error(
                    global_object,
                    &format!("'{other}' is not a valid underlying source 'type'; expected \"bytes\", \"direct\", or undefined"),
                ))
            }
        }
    }
    Ok(source)
}

/// `ExtractHighWaterMark(strategy, default)`.
pub(super) fn extract_high_water_mark(high_water_mark: Option<f64>, default: f64) -> Result<f64, Thrown> {
    match high_water_mark {
        None => Ok(default),
        Some(value) if value.is_nan() || value < 0.0 => Err(Thrown::range_error("The queuing strategy's highWaterMark must be a non-negative, non-NaN number")),
        Some(value) => Ok(value),
    }
}

/// Cria e registra o `ReadableStreamDefaultController` de `stream` com os algoritmos dados (ainda sem `start`).
fn create_controller(
    global_object: &JSGlobalObject,
    stream: &StreamRef,
    algorithms: SourceAlgorithms,
    high_water_mark: f64,
    size: Option<JSValue>,
) -> ControllerRef {
    let structure = instance_structure(global_object.vm(), Some(global_object), prototype_of("ReadableStreamDefaultController"));
    let value = JSFinalObject::create(global_object.vm(), &structure).as_value();
    let controller = Rc::new(RefCell::new(Controller {
        stream: stream.clone(),
        queue: VecDeque::new(),
        total: 0.0,
        started: false,
        close_requested: false,
        pull_again: false,
        pulling: false,
        high_water_mark,
        size,
        algorithms: Some(algorithms),
        value,
    }));
    register(value, Obj::Controller(controller.clone()));
    stream.borrow_mut().controller = Some(controller.clone());
    controller
}

/// O fim de `SetUpReadableStreamDefaultController`: o resultado de `start` passa por uma promessa e uma reação.
fn finish_start(global_object: &JSGlobalObject, controller: ControllerRef, start_result: JSValue) {
    let for_fulfilled = controller.clone();
    let on_fulfilled = reaction(global_object, move |global_object, _| {
        for_fulfilled.borrow_mut().started = true;
        call_pull_if_needed(global_object, &for_fulfilled)
    });
    let for_rejected = controller;
    let on_rejected = reaction(global_object, move |global_object, error| controller_error(global_object, &for_rejected, error));
    let start_promise = fresh_resolved_with(global_object, start_result);
    then(global_object, start_promise, on_fulfilled, on_rejected, JSValue::undefined());
}

/// `SetUpReadableStreamDefaultControllerFromUnderlyingSource`.
fn set_up_default_controller(
    global_object: &JSGlobalObject,
    stream: &StreamRef,
    underlying: JSValue,
    source: &Source,
    high_water_mark: f64,
    size: Option<JSValue>,
) -> Result<(), Thrown> {
    let algorithms = SourceAlgorithms::JavaScript { source: underlying, pull: source.pull, cancel: source.cancel };
    let controller = create_controller(global_object, stream, algorithms, high_water_mark, size);
    let value = controller.borrow().value;
    let start_result = match source.start {
        Some(function) => call_function(global_object, function, underlying, &[value]).ok_or(Thrown::Pending)?,
        None => JSValue::undefined(),
    };
    finish_start(global_object, controller, start_result);
    Ok(())
}

/// O lado do `ReadableStreamDefaultController` que o Rust enxerga num stream criado por [`create_native`]
/// (o que o C++ faz com `JSReadableStreamDefaultController*` em `SourceKind::Transform`). Os métodos são os
/// mesmos `ReadableStreamDefaultController*` que os membros do protótipo chamam.
#[derive(Clone)]
pub(super) struct ReadableHandle {
    stream: StreamRef,
    controller: ControllerRef,
}

impl ReadableHandle {
    /// O controlador ainda existe (não foi desmontado).
    pub(super) fn controller_present(&self) -> bool {
        self.stream.borrow().controller.is_some()
    }

    pub(super) fn can_close_or_enqueue(&self) -> bool {
        can_close_or_enqueue(&self.controller)
    }

    /// `ReadableStreamDefaultControllerEnqueue`; `Err` é a exceção do `size()` (já retirada do `VM`).
    pub(super) fn enqueue(&self, global_object: &JSGlobalObject, chunk: JSValue) -> Result<(), JSValue> {
        controller_enqueue(global_object, &self.controller, chunk).map_err(|_| take_error(global_object))
    }

    /// `ReadableStreamDefaultControllerHasBackpressure`: `!ShouldCallPull`.
    pub(super) fn has_backpressure(&self) -> bool {
        !should_call_pull(&self.controller)
    }

    pub(super) fn close(&self, global_object: &JSGlobalObject) {
        controller_close(global_object, &self.controller);
    }

    pub(super) fn error(&self, global_object: &JSGlobalObject, error: JSValue) {
        let _ = controller_error(global_object, &self.controller, error);
    }

    pub(super) fn desired_size(&self) -> Option<f64> {
        desired_size(&self.controller)
    }

    pub(super) fn is_errored(&self) -> bool {
        self.stream.borrow().state == State::Errored
    }

    pub(super) fn stored_error(&self) -> Option<JSValue> {
        let inner = self.stream.borrow();
        (inner.state == State::Errored).then_some(inner.stored_error)
    }

    /// O objeto JS do `ReadableStreamDefaultController` (o argumento de `transform`/`start` do usuário).
    pub(super) fn controller_value(&self) -> JSValue {
        self.controller.borrow().value
    }
}

/// `ReadableStream::create` com fonte nativa: sem objeto JS de `underlyingSource`. `start` é o resultado do
/// algoritmo de start (uma promessa ou valor; o stream só puxa depois que ela cumpre). Devolve o objeto JS do stream
/// e o [`ReadableHandle`] do controlador.
pub(super) fn create_native(
    global_object: &JSGlobalObject,
    source: Rc<dyn NativeSource>,
    high_water_mark: f64,
    size: Option<JSValue>,
    start: JSValue,
) -> (JSValue, ReadableHandle) {
    let structure = instance_structure(global_object.vm(), Some(global_object), prototype_of("ReadableStream"));
    let value = JSFinalObject::create(global_object.vm(), &structure).as_value();
    let stream = Rc::new(RefCell::new(Stream { state: State::Readable, stored_error: JSValue::undefined(), reader: None, controller: None, disturbed: false, bytes: false, byte_controller: None }));
    register(value, Obj::Stream(stream.clone()));
    let controller = create_controller(global_object, &stream, SourceAlgorithms::Native(source), high_water_mark, size);
    finish_start(global_object, controller.clone(), start);
    (value, ReadableHandle { stream, controller })
}

/// Cria o `ReadableByteStreamController` de `stream` e o registra; devolve o estado e o objeto JS do controlador.
fn new_byte_controller(global_object: &JSGlobalObject, stream: &StreamRef, algorithms: SourceAlgorithms, high_water_mark: f64, auto_allocate_chunk_size: Option<usize>) -> (ByteRef, JSValue) {
    let structure = instance_structure(global_object.vm(), Some(global_object), prototype_of("ReadableByteStreamController"));
    let value = JSFinalObject::create(global_object.vm(), &structure).as_value();
    let state = Rc::new(RefCell::new(ByteState {
        stream: stream.clone(),
        core: ByteCore::new(high_water_mark, auto_allocate_chunk_size),
        algorithms: Some(algorithms),
        value,
        effects: Vec::new(),
        promises: Promises::new(),
        request: None,
    }));
    register(value, Obj::ByteController(state.clone()));
    stream.borrow_mut().byte_controller = Some(state.clone());
    (state, value)
}

/// Arma a reação do resultado de `start`: marca `started` e puxa (a mesma promessa e reação do controlador padrão).
fn start_byte_controller(global_object: &JSGlobalObject, state: ByteRef, start_result: JSValue) {
    let for_fulfilled = state.clone();
    let on_fulfilled = reaction(global_object, move |global_object, _| {
        for_fulfilled.borrow_mut().core.started = true;
        byte_call_pull_if_needed(global_object, &for_fulfilled)
    });
    let on_rejected = reaction(global_object, move |global_object, error| byte_controller_error(global_object, &state, error));
    let start_promise = fresh_resolved_with(global_object, start_result);
    then(global_object, start_promise, on_fulfilled, on_rejected, JSValue::undefined());
}

/// `CreateReadableByteStream` com fonte nativa (o `tee` de um stream de bytes): sem `start`, sem `autoAllocateChunkSize`.
/// Devolve o objeto JS do stream e o estado do controlador.
fn create_native_bytes(global_object: &JSGlobalObject, source: Rc<dyn NativeSource>, high_water_mark: f64) -> (JSValue, ByteRef) {
    let structure = instance_structure(global_object.vm(), Some(global_object), prototype_of("ReadableStream"));
    let value = JSFinalObject::create(global_object.vm(), &structure).as_value();
    let stream = Rc::new(RefCell::new(Stream { state: State::Readable, stored_error: JSValue::undefined(), reader: None, controller: None, disturbed: false, bytes: true, byte_controller: None }));
    register(value, Obj::Stream(stream.clone()));
    let (state, _) = new_byte_controller(global_object, &stream, SourceAlgorithms::Native(source), high_water_mark, None);
    start_byte_controller(global_object, state.clone(), JSValue::undefined());
    (value, state)
}

/// `SetUpReadableByteStreamControllerFromUnderlyingSource`: cria o controlador de bytes, roda `start` e arma a reação
/// que marca `started` e puxa (a mesma promessa e reação do controlador padrão).
fn set_up_byte_controller(global_object: &JSGlobalObject, stream: &StreamRef, underlying: JSValue, source: &Source, high_water_mark: f64) -> Result<(), Thrown> {
    let algorithms = SourceAlgorithms::JavaScript { source: underlying, pull: source.pull, cancel: source.cancel };
    let (state, value) = new_byte_controller(global_object, stream, algorithms, high_water_mark, source.auto_allocate_chunk_size);
    let start_result = match source.start {
        Some(function) => call_function(global_object, function, underlying, &[value]).ok_or(Thrown::Pending)?,
        None => JSValue::undefined(),
    };
    start_byte_controller(global_object, state, start_result);
    Ok(())
}

/// `new ReadableStream(underlyingSource, strategy)`.
pub(super) fn construct_stream(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let argument = call.argument(0);
    let underlying = if argument.is_undefined() {
        JSValue::null()
    } else if !argument.is_object() {
        return Err(throw_native_type_error(global_object, "ReadableStream constructor takes an object as first argument"));
    } else {
        argument
    };
    let (strategy_high_water_mark, strategy_size) = convert_strategy(global_object, call.argument(1))?;
    let structure = derived_structure(global_object, call, instance_structure)?;
    let value = JSFinalObject::create(global_object.vm(), &structure).as_value();
    let source = convert_source(global_object, underlying)?;
    let stream = Rc::new(RefCell::new(Stream {
        state: State::Readable,
        stored_error: JSValue::undefined(),
        reader: None,
        controller: None,
        disturbed: false,
        bytes: source.bytes,
        byte_controller: None,
    }));
    register(value, Obj::Stream(stream.clone()));
    if source.bytes {
        if strategy_size.is_some() {
            return Err(Thrown::range_error("The queuing strategy of a readable byte stream cannot have a size function"));
        }
        let high_water_mark = extract_high_water_mark(strategy_high_water_mark, 0.0)?;
        set_up_byte_controller(global_object, &stream, underlying, &source, high_water_mark)?;
    } else {
        let high_water_mark = extract_high_water_mark(strategy_high_water_mark, 1.0)?;
        set_up_default_controller(global_object, &stream, underlying, &source, high_water_mark, strategy_size)?;
    }
    Ok(value)
}

// ---------------------------------------------------------------------------------------------
// Membros do protótipo
// ---------------------------------------------------------------------------------------------

/// Travado por um leitor ou já lido (`disturbed`): o `bodyUsed` de um corpo que é um stream. `false` para o que não é stream.
pub(super) fn is_locked_or_disturbed(value: JSValue) -> bool {
    stream_of(value).is_some_and(|stream| {
        let inner = stream.borrow();
        inner.reader.is_some() || inner.disturbed
    })
}

/// Por que `text()`, `bytes()`, `json()` e `blob()` do stream não podem ler `value` (medido no bun 1.4.2: travado por um
/// leitor do usuário, ou já lido/cancelado), com a mensagem do `ERR_INVALID_STATE`; `None` se podem.
pub(super) fn unavailable_reason(value: JSValue) -> Option<&'static str> {
    let stream = stream_of(value)?;
    if visibly_locked(&stream) {
        return Some("Invalid state: ReadableStream is locked");
    }
    is_locked_or_disturbed(value).then_some("Invalid state: ReadableStream has already been used")
}

/// Travado por um leitor do usuário ou pelo leitor interno da leitura do corpo (`text()` e afins) enquanto ela está em
/// andamento, como no bun (`locked: true` até a leitura terminar). O bun esvazia de uma vez o stream cujos pedaços já
/// estão na fila com o fechamento pedido, e nesse caso o leitor interno nunca chega a ser visível.
fn visibly_locked(stream: &StreamRef) -> bool {
    let inner = stream.borrow();
    let Some(reader) = inner.reader.as_ref() else { return false };
    if !reader.borrow().hidden {
        return true;
    }
    closed_queue_len(&inner).is_none()
}

/// Quantos pedaços restam na fila quando o fechamento já foi pedido, seja no controlador padrão, seja no de bytes;
/// `None` quando o fechamento não foi pedido (ou não há controlador).
fn closed_queue_len(stream: &Stream) -> Option<usize> {
    if let Some(controller) = stream.controller.as_ref() {
        let controller = controller.borrow();
        return controller.close_requested.then(|| controller.queue.len());
    }
    let state = stream.byte_controller.as_ref()?.borrow();
    state.core.close_requested.then(|| state.core.queued_chunks())
}

/// Os dois sorvedouros de [`read_all`]: `chunk` recebe cada pedaço lido (`Err` aborta a leitura com esse motivo) e
/// `end` recebe o fim (`Ok` quando o stream fechou, `Err` com o erro do stream ou do sorvedouro). `held` lista o que as
/// closures capturam em valores JS (o coletor o visita enquanto a leitura durar).
pub(super) struct Sinks {
    pub(super) chunk: Box<dyn Fn(&JSGlobalObject, JSValue) -> Result<(), JSValue>>,
    pub(super) end: Box<dyn Fn(&JSGlobalObject, Result<(), JSValue>)>,
    pub(super) held: Vec<Rc<dyn Traced>>,
}

pub(super) type SinksRef = Rc<Sinks>;

impl Traced for Sinks {
    fn trace(&self, out: &mut Vec<JSValue>) {
        self.held.iter().for_each(|held| held.trace(out));
    }
}

/// Lê o `ReadableStream` `value` até o fim por um leitor interno (que não conta em `locked`), em cadeia de promessas:
/// cada leitura reage na seguinte, sem bloquear. Devolve `false` se `value` não é um stream ou já está travado/lido.
pub(super) fn read_all(global_object: &JSGlobalObject, value: JSValue, sinks: SinksRef) -> bool {
    let Some(stream) = stream_of(value) else { return false };
    if is_locked_or_disturbed(value) {
        return false;
    }
    let Ok(reader_value) = create_reader(global_object, &stream, &reader_structure(global_object, ReaderKind::Default), ReaderKind::Default) else {
        return false;
    };
    let Some(reader) = reader_of(reader_value, ReaderKind::Default) else { return false };
    reader.borrow_mut().hidden = true;
    if drain_closed_queue(global_object, &stream, &reader, &sinks) {
        return true;
    }
    pump(global_object, stream, reader, sinks);
    true
}

/// Como o bun: o stream cujos pedaços já estão na fila com o fechamento pedido é esvaziado de uma vez, de forma
/// síncrona, e o leitor interno é solto antes de `read_all` voltar (o stream fica destravado e fechado). Só a entrega
/// dos pedaços ao sorvedouro segue em cadeia de promessas. Devolve `false` quando o stream não está nesse estado.
fn drain_closed_queue(global_object: &JSGlobalObject, stream: &StreamRef, reader: &ReaderRef, sinks: &SinksRef) -> bool {
    let pending = {
        let inner = stream.borrow();
        match closed_queue_len(&inner) {
            Some(len) if inner.state == State::Readable => len,
            _ => return false,
        }
    };
    let mut reads = Vec::with_capacity(pending);
    for _ in 0..pending {
        let promise = new_promise(global_object);
        if reader_read_request(global_object, stream, reader, promise).is_err() {
            let error = take_error(global_object);
            reader_release(global_object, reader);
            (sinks.end)(global_object, Err(error));
            return true;
        }
        reads.push(promise);
    }
    reader_release(global_object, reader);
    deliver_drained(global_object, reads, 0, sinks.clone());
    true
}

/// Entrega ao sorvedouro, em ordem, os pedaços já lidos por [`drain_closed_queue`]; no fim chama `end`.
fn deliver_drained(global_object: &JSGlobalObject, reads: Vec<JSValue>, index: usize, sinks: SinksRef) {
    let Some(&promise) = reads.get(index) else {
        (sinks.end)(global_object, Ok(()));
        return;
    };
    let held = vec![hold(reads.clone()), hold(sinks.clone())];
    let next_sinks = sinks.clone();
    let fulfilled = reaction_holding(global_object, held.clone(), move |global_object, result| {
        let chunk = get_property(global_object, result, "value")?;
        match (next_sinks.chunk)(global_object, chunk) {
            Ok(()) => deliver_drained(global_object, reads.clone(), index + 1, next_sinks.clone()),
            Err(error) => (next_sinks.end)(global_object, Err(error)),
        }
        Ok(())
    });
    let rejected = reaction_holding(global_object, held, move |global_object, error| {
        (sinks.end)(global_object, Err(error));
        Ok(())
    });
    then(global_object, promise, fulfilled, rejected, JSValue::undefined());
}

fn pump(global_object: &JSGlobalObject, stream: StreamRef, reader: ReaderRef, sinks: SinksRef) {
    let promise = new_promise(global_object);
    if reader_read_request(global_object, &stream, &reader, promise).is_err() {
        let error = take_error(global_object);
        reader_release(global_object, &reader);
        (sinks.end)(global_object, Err(error));
        return;
    }
    let held = vec![hold(sinks.clone())];
    let (next_stream, next_reader, next_sinks) = (stream, reader.clone(), sinks.clone());
    let fulfilled = reaction_holding(global_object, held.clone(), move |global_object, result| {
        let done = get_property(global_object, result, "done")?;
        if matches!(done, JSValue::Bool(true)) {
            reader_release(global_object, &next_reader);
            (next_sinks.end)(global_object, Ok(()));
            return Ok(());
        }
        let chunk = get_property(global_object, result, "value")?;
        match (next_sinks.chunk)(global_object, chunk) {
            Ok(()) => pump(global_object, next_stream.clone(), next_reader.clone(), next_sinks.clone()),
            Err(error) => {
                reader_release(global_object, &next_reader);
                (next_sinks.end)(global_object, Err(error));
            }
        }
        Ok(())
    });
    let rejected = reaction_holding(global_object, held, move |global_object, error| {
        reader_release(global_object, &reader);
        (sinks.end)(global_object, Err(error));
        Ok(())
    });
    then(global_object, promise, fulfilled, rejected, JSValue::undefined());
}

pub(super) fn rs_locked(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let stream = stream_of(call.this_value()).ok_or_else(|| invalid_this(global_object, "ReadableStream"))?;
    let locked = visibly_locked(&stream);
    Ok(JSValue::Bool(locked))
}

pub(super) fn rs_cancel(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(stream) = stream_of(call.this_value()) else {
        let error = type_error_value(global_object, "ReadableStream.prototype.cancel can only be called on a ReadableStream");
        return Ok(rejected_with(global_object, error));
    };
    if stream.borrow().reader.is_some() {
        let error = type_error_value(global_object, "Cannot cancel a locked ReadableStream");
        return Ok(rejected_with(global_object, error));
    }
    Ok(stream_cancel(global_object, &stream, call.argument(0)))
}

pub(super) fn rs_get_reader(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let stream = stream_of(call.this_value()).ok_or_else(|| invalid_this(global_object, "ReadableStream"))?;
    let options = call.argument(0);
    if !options.is_undefined_or_null() {
        if !options.is_object() {
            let type_name = if options.is_string() { "string" } else if options.is_number() { "number" } else { "boolean" };
            let message = format!("The \"options\" argument must be of type object. Received type {type_name} ({})", received_primitive(options));
            return Err(throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE"));
        }
        let mode = get_property(global_object, options, "mode")?;
        if !mode.is_undefined() {
            let text = if mode.is_object() { None } else { Some(crate::runtime::js_module_loader::rust_string(&mode.to_wtf_string())) };
            if text.as_deref() != Some("byob") {
                let message = format!("The property 'options.mode' is invalid. Received {}", received_primitive(mode));
                return Err(throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_VALUE"));
            }
            if !stream.borrow().bytes {
                return Err(throw_native_type_error(global_object, "A BYOB reader requires a ReadableStream with an underlying byte source"));
            }
            return acquire_byob_reader(global_object, &stream);
        }
    }
    create_reader(global_object, &stream, &reader_structure(global_object, ReaderKind::Default), ReaderKind::Default)
}

/// O `closed` dos dois leitores; `class_name` entra na mensagem de `this` inválido.
fn reader_closed(global_object: &JSGlobalObject, call: &HostCall, kind: ReaderKind, class_name: &str) -> HostResult {
    match reader_of(call.this_value(), kind) {
        Some(reader) => {
            let closed = reader.borrow().closed;
            Ok(closed)
        }
        None => {
            let error = type_error_value(global_object, &format!("The 'closed' getter can only be used on a {class_name}"));
            Ok(rejected_with(global_object, error))
        }
    }
}

pub(super) fn dr_closed(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    reader_closed(global_object, call, ReaderKind::Default, "ReadableStreamDefaultReader")
}

pub(super) fn br_closed(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    reader_closed(global_object, call, ReaderKind::Byob, "ReadableStreamBYOBReader")
}

/// O `cancel` dos dois leitores.
fn reader_cancel(global_object: &JSGlobalObject, call: &HostCall, kind: ReaderKind, class_name: &str) -> HostResult {
    let Some(reader) = reader_of(call.this_value(), kind) else {
        let error = type_error_value(global_object, &format!("{class_name}.prototype.cancel can only be called on a {class_name}"));
        return Ok(rejected_with(global_object, error));
    };
    let stream = reader.borrow().stream.clone();
    let Some(stream) = stream else {
        let error = invalid_state(global_object, "The reader is not attached to a stream");
        return Ok(rejected_with(global_object, error));
    };
    Ok(stream_cancel(global_object, &stream, call.argument(0)))
}

pub(super) fn dr_cancel(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    reader_cancel(global_object, call, ReaderKind::Default, "ReadableStreamDefaultReader")
}

pub(super) fn br_cancel(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    reader_cancel(global_object, call, ReaderKind::Byob, "ReadableStreamBYOBReader")
}

/// `read(view, { min })` do leitor BYOB, depois de achar o leitor; `Err` é o valor com que a promessa rejeita.
fn byob_read(global_object: &JSGlobalObject, reader: &ReaderRef, view: JSValue, options: JSValue) -> Result<JSValue, JSValue> {
    let thrown_value = |thrown: Thrown| error_of(global_object, thrown);
    let Some(info) = view_info(view) else { return Err(type_error_value(global_object, msg::VIEW_REQUIRED)) };
    if info.detached {
        return Err(type_error_value(global_object, msg::DETACHED));
    }
    if info.byte_length == 0 {
        return Err(invalid_state(global_object, msg::VIEW_EMPTY));
    }
    let mut min = 1;
    if !options.is_undefined_or_null() {
        if !options.is_object() {
            return Err(type_error_value(global_object, msg::MIN_OPTIONS));
        }
        let value = get_property(global_object, options, "min").map_err(thrown_value)?;
        if !value.is_undefined() {
            min = enforce_range_u64(global_object, value).map_err(thrown_value)?;
        }
    }
    if min == 0 {
        return Err(type_error_value(global_object, msg::MIN_ZERO));
    }
    if min > info.length {
        return Err(range_error_value(global_object, msg::MIN_TOO_BIG));
    }
    let Some(stream) = reader.borrow().stream.clone() else {
        return Err(invalid_state(global_object, "The reader is not attached to a stream"));
    };
    stream.borrow_mut().disturbed = true;
    if stream.borrow().state == State::Errored {
        let error = stream.borrow().stored_error;
        return Ok(rejected_with(global_object, error));
    }
    let Some(state) = stream.borrow().byte_controller.clone() else {
        return Err(type_error_value(global_object, "A BYOB reader requires a ReadableStream with an underlying byte source"));
    };
    // `TransferArrayBuffer`: os bytes passam para o núcleo e o buffer do usuário fica desanexado.
    let data = info.buffer.with_bytes(<[u8]>::to_vec);
    info.buffer.detach();
    let promise = new_promise(global_object);
    reader.borrow_mut().requests.push_back(promise);
    let element_size = info.kind.element_size();
    let outcome = run_core(global_object, &state, |core, out| {
        core.pull_into(data, info.byte_offset, info.byte_length, element_size, min, info.kind as u32, out)
    })
    .map_err(thrown_value)?;
    if let Err(message) = outcome {
        let error = type_error_value(global_object, message);
        byte_controller_error(global_object, &state, error).map_err(thrown_value)?;
        return Ok(promise);
    }
    byte_call_pull_if_needed(global_object, &state).map_err(thrown_value)?;
    Ok(promise)
}

pub(super) fn br_read(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(reader) = reader_of(call.this_value(), ReaderKind::Byob) else {
        let error = type_error_value(global_object, "ReadableStreamBYOBReader.prototype.read can only be called on a ReadableStreamBYOBReader");
        return Ok(rejected_with(global_object, error));
    };
    Ok(byob_read(global_object, &reader, call.argument(0), call.argument(1)).unwrap_or_else(|error| rejected_with(global_object, error)))
}

pub(super) fn br_release_lock(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let reader = reader_of(call.this_value(), ReaderKind::Byob).ok_or_else(|| invalid_this(global_object, "ReadableStreamBYOBReader"))?;
    reader_release(global_object, &reader);
    Ok(JSValue::undefined())
}

pub(super) fn dr_read(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(reader) = reader_of(call.this_value(), ReaderKind::Default) else {
        let error = type_error_value(global_object, "ReadableStreamDefaultReader.prototype.read can only be called on a ReadableStreamDefaultReader");
        return Ok(rejected_with(global_object, error));
    };
    let stream = reader.borrow().stream.clone();
    let Some(stream) = stream else {
        let error = invalid_state(global_object, "The reader is not attached to a stream");
        return Ok(rejected_with(global_object, error));
    };
    // Fila com chunk e nada esperando: resolve na hora, sem pedido de leitura.
    let controller = stream.borrow().controller.clone();
    if let Some(controller) = controller {
        let readable = stream.borrow().state == State::Readable;
        if readable && reader.borrow().requests.is_empty() && !controller.borrow().queue.is_empty() {
            stream.borrow_mut().disturbed = true;
            return Ok(match dequeue_chunk_for_read(global_object, &controller) {
                Ok(chunk) => {
                    let result = create_iterator_result_object(global_object, chunk, false);
                    resolved_with(global_object, result)
                }
                Err(_) => rejected_with(global_object, take_error(global_object)),
            });
        }
    }
    let promise = new_promise(global_object);
    Ok(match reader_read_request(global_object, &stream, &reader, promise) {
        Ok(()) => promise,
        Err(_) => rejected_with(global_object, take_error(global_object)),
    })
}

pub(super) fn dr_release_lock(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let reader = reader_of(call.this_value(), ReaderKind::Default).ok_or_else(|| invalid_this(global_object, "ReadableStreamDefaultReader"))?;
    reader_release(global_object, &reader);
    Ok(JSValue::undefined())
}

fn this_controller(global_object: &JSGlobalObject, call: &HostCall) -> Result<ControllerRef, Thrown> {
    controller_of(call.this_value()).ok_or_else(|| invalid_this(global_object, "ReadableStreamDefaultController"))
}

pub(super) fn dc_desired_size(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let controller = this_controller(global_object, call)?;
    Ok(desired_size(&controller).map_or_else(JSValue::null, js_number))
}

pub(super) fn dc_close(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let controller = this_controller(global_object, call)?;
    if !can_close_or_enqueue(&controller) {
        return Err(throw_coded_type_error(global_object, "Invalid state: Controller is already closed", "ERR_INVALID_STATE"));
    }
    controller_close(global_object, &controller);
    Ok(JSValue::undefined())
}

pub(super) fn dc_enqueue(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let controller = this_controller(global_object, call)?;
    if !can_close_or_enqueue(&controller) {
        return Err(throw_coded_type_error(global_object, "Invalid state: Controller is already closed", "ERR_INVALID_STATE"));
    }
    controller_enqueue(global_object, &controller, call.argument(0))?;
    Ok(JSValue::undefined())
}

pub(super) fn dc_error(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let controller = this_controller(global_object, call)?;
    controller_error(global_object, &controller, call.argument(0))?;
    Ok(JSValue::undefined())
}

/// O texto do `Symbol(nodejs.util.inspect.custom)` das instâncias cujo texto não precisa de `util.inspect`
/// (`ReadableStream` e o controlador). `None` para o resto.
pub(super) fn inspect_text(global_object: &JSGlobalObject, value: JSValue, exhausted: bool) -> Option<JSValue> {
    let text = if let Some(stream) = stream_of(value) {
        let inner = stream.borrow();
        // `options.depth` esgotado: o bun mostra só o nome e `[Object]`.
        if exhausted {
            return Some(text_value(global_object, "ReadableStream [Object]"));
        }
        format!("ReadableStream {{ locked: {}, state: '{}', supportsBYOB: {} }}", inner.reader.as_ref().is_some_and(|reader| !reader.borrow().hidden), inner.state.name(), inner.bytes)
    } else if controller_of(value).is_some() {
        "ReadableStreamDefaultController {}".to_string()
    } else if byte_of(value).is_some() {
        "ReadableByteStreamController {}".to_string()
    } else {
        return None;
    };
    Some(text_value(global_object, &text))
}

/// Uma string JS com o texto (ASCII).
pub(super) fn text_value(global_object: &JSGlobalObject, text: &str) -> JSValue {
    JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_latin1(text.as_bytes())))
}
