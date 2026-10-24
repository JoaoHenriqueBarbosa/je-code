//! `ReadableStream.prototype.pipeTo` e `pipeThrough` (o `ReadableStreamPipeTo` do WebKit/bun).
//!
//! O canal usa só a superfície pública do leitor e do escritor (`read`, `write`, `ready`, `closed`, `cancel`,
//! `abort`, `close`, `releaseLock`), como o C++ faz com os objetos JS que ele adquire. As promessas e as reações são as
//! de [`super::readable`]; nenhum empréstimo atravessa uma chamada a JS.
//!
//! Ordem: espera `writer.ready`, lê, escreve sem esperar a escrita, repete. Os quatro monitores (origem fechada ou
//! com erro, destino fechado ou com erro, sinal) levam ao `shutdown`, que espera as escritas em voo, roda as ações
//! (abort, cancel, close), solta os travamentos e só então liquida a promessa devolvida.

use std::cell::RefCell;
use std::rc::Rc;

use super::readable::{call_js, mark_handled, new_promise, reaction_holding, reject, rejected_with, resolve, resolved_with, take_error, text_value, then, type_error_value};
use super::trace::{hold, Traced};
use crate::runtime::gc::RootScope;
use super::{readable, reject_with_thrown, throw_invalid_this, writable_js, Kind};
use crate::runtime::abort_signal::is_signal;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::intl_support::get_property;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::native_class_support::throw_native_type_error;

/// Uma chamada de método adiada: objeto, nome e argumentos.
type Action = (JSValue, &'static str, Vec<JSValue>);

/// As chamadas adiadas de um `shutdown`, para a closure que as roda declarar o que segura.
#[derive(Clone)]
struct Actions(Vec<Action>);

impl Traced for Actions {
    fn trace(&self, out: &mut Vec<JSValue>) {
        for (object, _, arguments) in &self.0 {
            object.trace(out);
            arguments.trace(out);
        }
    }
}

struct Pipe {
    reader: JSValue,
    writer: JSValue,
    promise: JSValue,
    prevent_abort: bool,
    prevent_cancel: bool,
    prevent_close: bool,
    shutting_down: bool,
    finished: bool,
    pending_writes: Vec<JSValue>,
    /// A promessa do `read` em voo: a origem fecha (`closed` resolve) quando o último pedaço sai da fila, antes de a
    /// leitura dele entregar o valor, e o fechamento do destino espera essa leitura para não perder o pedaço.
    reading: Option<JSValue>,
}

type PipeRef = Rc<RefCell<Pipe>>;

impl Traced for Pipe {
    fn trace(&self, out: &mut Vec<JSValue>) {
        self.reader.trace(out);
        self.writer.trace(out);
        self.promise.trace(out);
        self.pending_writes.trace(out);
        self.reading.trace(out);
    }
}

/// `object[name](...args)`; `Err` é o valor lançado.
fn invoke(global_object: &JSGlobalObject, object: JSValue, name: &str, arguments: &[JSValue]) -> Result<JSValue, JSValue> {
    let function = get_property(global_object, object, name).map_err(|_| take_error(global_object))?;
    call_js(global_object, function, object, arguments)
}

fn property(global_object: &JSGlobalObject, object: JSValue, name: &str) -> Result<JSValue, JSValue> {
    get_property(global_object, object, name).map_err(|_| take_error(global_object))
}

fn ignore(global_object: &JSGlobalObject) -> JSValue {
    reaction_holding(global_object, Vec::new(), |_, _| Ok(()))
}

type JoinDone = Box<dyn FnOnce(&JSGlobalObject, Option<JSValue>)>;

/// O que o `join` espera: quantas promessas faltam, a primeira rejeição, o que fazer no fim e o que o `done` captura.
struct Join {
    remaining: usize,
    first_error: Option<JSValue>,
    done: Option<JoinDone>,
    held: Vec<Rc<dyn Traced>>,
}

type JoinState = Rc<RefCell<Join>>;

impl Traced for Join {
    fn trace(&self, out: &mut Vec<JSValue>) {
        self.first_error.trace(out);
        self.held.iter().for_each(|held| held.trace(out));
    }
}

fn settle(global_object: &JSGlobalObject, state: &JoinState, error: Option<JSValue>) {
    let finished = {
        let mut inner = state.borrow_mut();
        inner.remaining -= 1;
        if inner.first_error.is_none() {
            inner.first_error = error;
        }
        let first_error = inner.first_error;
        if inner.remaining == 0 { inner.done.take().map(|done| (done, first_error)) } else { None }
    };
    if let Some((done, first_error)) = finished {
        done(global_object, first_error);
    }
}

/// Espera todas as promessas; `done` recebe a primeira rejeição, se houve. `held` lista os valores JS que `done`
/// captura.
fn join(global_object: &JSGlobalObject, promises: Vec<JSValue>, done: JoinDone, held: Vec<Rc<dyn Traced>>) {
    if promises.is_empty() {
        done(global_object, None);
        return;
    }
    let state: JoinState = Rc::new(RefCell::new(Join { remaining: promises.len(), first_error: None, done: Some(done), held }));
    for promise in promises {
        let promise = resolved_with(global_object, promise);
        let for_ok = state.clone();
        let for_bad = state.clone();
        let on_ok = reaction_holding(global_object, vec![hold(state.clone())], move |global_object, _| {
            settle(global_object, &for_ok, None);
            Ok(())
        });
        let on_bad = reaction_holding(global_object, vec![hold(state.clone())], move |global_object, error| {
            settle(global_object, &for_bad, Some(error));
            Ok(())
        });
        then(global_object, promise, on_ok, on_bad, JSValue::undefined());
    }
}

/// Solta os dois travamentos e liquida a promessa devolvida por `pipeTo`.
fn finalize(global_object: &JSGlobalObject, pipe: &PipeRef, error: Option<JSValue>) {
    let (reader, writer, promise) = {
        let mut inner = pipe.borrow_mut();
        if inner.finished {
            return;
        }
        inner.finished = true;
        (inner.reader, inner.writer, inner.promise)
    };
    let _ = invoke(global_object, writer, "releaseLock", &[]);
    let _ = invoke(global_object, reader, "releaseLock", &[]);
    match error {
        Some(error) => reject(global_object, promise, error),
        None => resolve(global_object, promise, JSValue::undefined()),
    }
}

fn run_actions(global_object: &JSGlobalObject, pipe: &PipeRef, error: Option<JSValue>, actions: Vec<Action>) {
    // Medido no bun (`startPipeAbortBothActions`): as duas ações do abort do sinal começam uma atrás da outra, mas o
    // `cancel` da origem só chega ao `cancel` do usuário depois do `abort` do destino (abort primeiro, cancel no mesmo
    // tempo): a segunda ação em diante parte numa reação, um tempo depois.
    let calls = actions
        .into_iter()
        .enumerate()
        .map(|(index, (object, name, arguments))| {
            if index == 0 {
                return invoke(global_object, object, name, &arguments).unwrap_or_else(|thrown| rejected_with(global_object, thrown));
            }
            let deferred = new_promise(global_object);
            let held = hold(Actions(vec![(object, name, arguments.clone())]));
            let start = reaction_holding(global_object, vec![held], move |global_object, _| {
                match invoke(global_object, object, name, &arguments) {
                    Ok(result) => resolve(global_object, deferred, result),
                    Err(thrown) => reject(global_object, deferred, thrown),
                }
                Ok(())
            });
            then(global_object, resolved_with(global_object, JSValue::undefined()), start, ignore(global_object), JSValue::undefined());
            deferred
        })
        .collect();
    let held = vec![hold(pipe.clone()), hold(error)];
    let pipe = pipe.clone();
    join(global_object, calls, Box::new(move |global_object, action_error| finalize(global_object, &pipe, action_error.or(error))), held);
}

/// `ReadableStreamPipeTo` shutdown: espera as escritas em voo, roda as ações, solta os travamentos.
fn shutdown(global_object: &JSGlobalObject, pipe: &PipeRef, error: Option<JSValue>, actions: Vec<Action>) {
    let pending = {
        let mut inner = pipe.borrow_mut();
        if inner.shutting_down {
            return;
        }
        inner.shutting_down = true;
        std::mem::take(&mut inner.pending_writes)
    };
    let held = vec![hold(pipe.clone()), hold(error), hold(Actions(actions.clone()))];
    let pipe = pipe.clone();
    join(global_object, pending, Box::new(move |global_object, _| run_actions(global_object, &pipe, error, actions)), held);
}

fn is_stopped(pipe: &PipeRef) -> bool {
    let inner = pipe.borrow();
    inner.shutting_down || inner.finished
}

/// Um passo do laço: lê um pedaço e o escreve sem esperar a escrita.
fn read_step(global_object: &JSGlobalObject, pipe: &PipeRef) {
    if is_stopped(pipe) {
        return;
    }
    let (reader, writer) = {
        let inner = pipe.borrow();
        (inner.reader, inner.writer)
    };
    // `desiredSize` nulo: destino fechando ou com erro, os monitores cuidam do resto.
    if matches!(property(global_object, writer, "desiredSize"), Ok(JSValue::Null)) {
        return;
    }
    let read = match invoke(global_object, reader, "read", &[]) {
        Ok(read) => resolved_with(global_object, read),
        Err(_) => return,
    };
    pipe.borrow_mut().reading = Some(read);
    let for_chunk = pipe.clone();
    let on_chunk = reaction_holding(global_object, vec![hold(pipe.clone())], move |global_object, result| {
        for_chunk.borrow_mut().reading = None;
        if is_stopped(&for_chunk) {
            return Ok(());
        }
        let Ok(done) = property(global_object, result, "done") else { return Ok(()) };
        if done.to_boolean() {
            return Ok(());
        }
        let Ok(value) = property(global_object, result, "value") else { return Ok(()) };
        let writer = for_chunk.borrow().writer;
        if let Ok(write) = invoke(global_object, writer, "write", &[value]) {
            mark_handled(write);
            for_chunk.borrow_mut().pending_writes.push(write);
        }
        pump(global_object, &for_chunk);
        Ok(())
    });
    then(global_object, read, on_chunk, ignore(global_object), JSValue::undefined());
}

/// Espera `writer.ready` e então dá um passo.
fn pump(global_object: &JSGlobalObject, pipe: &PipeRef) {
    if is_stopped(pipe) {
        return;
    }
    let writer = pipe.borrow().writer;
    let Ok(ready) = property(global_object, writer, "ready") else { return };
    let ready = resolved_with(global_object, ready);
    let for_ready = pipe.clone();
    let on_ready = reaction_holding(global_object, vec![hold(pipe.clone())], move |global_object, _| {
        read_step(global_object, &for_ready);
        Ok(())
    });
    then(global_object, ready, on_ready, ignore(global_object), JSValue::undefined());
}

/// A origem acabou: fecha o destino (salvo `preventClose`) depois das escritas em voo.
fn close_destination(global_object: &JSGlobalObject, pipe: &PipeRef) {
    let (writer, prevent_close) = {
        let inner = pipe.borrow();
        (inner.writer, inner.prevent_close)
    };
    let actions = if prevent_close { Vec::new() } else { vec![(writer, "close", Vec::new())] };
    shutdown(global_object, pipe, None, actions);
}

/// Liga os monitores da origem e do destino.
fn watch_closed(global_object: &JSGlobalObject, pipe: &PipeRef) {
    let (reader, writer) = {
        let inner = pipe.borrow();
        (inner.reader, inner.writer)
    };
    if let Ok(closed) = property(global_object, reader, "closed") {
        let closed = resolved_with(global_object, closed);
        let for_closed = pipe.clone();
        let on_closed = reaction_holding(global_object, vec![hold(pipe.clone())], move |global_object, _| {
            let in_flight = for_closed.borrow().reading;
            let Some(read) = in_flight else {
                close_destination(global_object, &for_closed);
                return Ok(());
            };
            let after_read = for_closed.clone();
            let on_read = reaction_holding(global_object, vec![hold(for_closed.clone())], move |global_object, _| {
                close_destination(global_object, &after_read);
                Ok(())
            });
            then(global_object, read, on_read, ignore(global_object), JSValue::undefined());
            Ok(())
        });
        let for_errored = pipe.clone();
        let on_errored = reaction_holding(global_object, vec![hold(pipe.clone())], move |global_object, error| {
            let (writer, prevent_abort) = {
                let inner = for_errored.borrow();
                (inner.writer, inner.prevent_abort)
            };
            let actions = if prevent_abort { Vec::new() } else { vec![(writer, "abort", vec![error])] };
            shutdown(global_object, &for_errored, Some(error), actions);
            Ok(())
        });
        then(global_object, closed, on_closed, on_errored, JSValue::undefined());
    }
    if let Ok(closed) = property(global_object, writer, "closed") {
        let closed = resolved_with(global_object, closed);
        let for_closed = pipe.clone();
        let on_closed = reaction_holding(global_object, vec![hold(pipe.clone())], move |global_object, _| {
            let error = type_error_value(global_object, "The destination WritableStream closed before all of the data could be piped to it");
            cancel_source(global_object, &for_closed, error);
            Ok(())
        });
        let for_errored = pipe.clone();
        let on_errored = reaction_holding(global_object, vec![hold(pipe.clone())], move |global_object, error| {
            cancel_source(global_object, &for_errored, error);
            Ok(())
        });
        then(global_object, closed, on_closed, on_errored, JSValue::undefined());
    }
}

/// O destino fechou ou errou: cancela a origem (salvo `preventCancel`) e encerra com o erro.
fn cancel_source(global_object: &JSGlobalObject, pipe: &PipeRef, error: JSValue) {
    let (reader, prevent_cancel) = {
        let inner = pipe.borrow();
        (inner.reader, inner.prevent_cancel)
    };
    let actions = if prevent_cancel { Vec::new() } else { vec![(reader, "cancel", vec![error])] };
    shutdown(global_object, pipe, Some(error), actions);
}

/// O que o sinal faz ao abortar: aborta o destino e cancela a origem, nessa ordem, com o `reason`.
fn on_signal_abort(global_object: &JSGlobalObject, pipe: &PipeRef, signal: JSValue) {
    let error = property(global_object, signal, "reason").unwrap_or_else(|thrown| thrown);
    let (reader, writer, prevent_abort, prevent_cancel) = {
        let inner = pipe.borrow();
        (inner.reader, inner.writer, inner.prevent_abort, inner.prevent_cancel)
    };
    let mut actions: Vec<Action> = Vec::new();
    if !prevent_abort {
        actions.push((writer, "abort", vec![error]));
    }
    if !prevent_cancel {
        actions.push((reader, "cancel", vec![error]));
    }
    shutdown(global_object, pipe, Some(error), actions);
}

/// `ReadableStreamPipeTo` depois da validação: adquire leitor e escritor, liga os monitores e inicia o laço.
fn start_pipe(global_object: &JSGlobalObject, source: JSValue, dest: JSValue, options: JSValue) -> Result<JSValue, JSValue> {
    let flag = |name: &str| -> Result<bool, JSValue> {
        if options.is_object() { Ok(property(global_object, options, name)?.to_boolean()) } else { Ok(false) }
    };
    let prevent_abort = flag("preventAbort")?;
    let prevent_cancel = flag("preventCancel")?;
    let prevent_close = flag("preventClose")?;
    let signal = if options.is_object() { property(global_object, options, "signal")? } else { JSValue::undefined() };
    if !signal.is_undefined() && !is_signal(signal) {
        return Err(type_error_value(global_object, "The pipe options' 'signal' property must be an AbortSignal"));
    }
    if property(global_object, source, "locked")?.to_boolean() {
        return Err(type_error_value(global_object, "Cannot pipe a locked ReadableStream"));
    }
    if property(global_object, dest, "locked")?.to_boolean() {
        return Err(type_error_value(global_object, "Cannot pipe to a locked WritableStream"));
    }
    let reader = invoke(global_object, source, "getReader", &[])?;
    let writer = invoke(global_object, dest, "getWriter", &[])?;
    let promise = new_promise(global_object);
    // O leitor, o escritor e a promessa só vivem nestes locais até as reações do `pipe` os segurarem.
    let _locals = RootScope::new(&[reader, writer, promise, signal]);
    let pipe = Rc::new(RefCell::new(Pipe { reader, writer, promise, prevent_abort, prevent_cancel, prevent_close, shutting_down: false, finished: false, pending_writes: Vec::new(), reading: None }));
    if !signal.is_undefined() {
        if property(global_object, signal, "aborted")?.to_boolean() {
            on_signal_abort(global_object, &pipe, signal);
            return Ok(promise);
        }
        let for_signal = pipe.clone();
        let listener = reaction_holding(global_object, vec![hold(pipe.clone()), hold(signal)], move |global_object, _| {
            on_signal_abort(global_object, &for_signal, signal);
            Ok(())
        });
        let _ = invoke(global_object, signal, "addEventListener", &[text_value(global_object, "abort"), listener]);
    }
    watch_closed(global_object, &pipe);
    pump(global_object, &pipe);
    Ok(promise)
}

pub(super) fn rs_pipe_to(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let source = call.this_value();
    if !readable::is_stream(source) {
        let thrown = throw_invalid_this(global_object, Kind::ReadableStream);
        return Ok(reject_with_thrown(global_object, thrown));
    }
    let dest = call.argument(0);
    if !writable_js::is_stream(dest) {
        let error = type_error_value(global_object, "ReadableStream.prototype.pipeTo requires a WritableStream destination");
        return Ok(rejected_with(global_object, error));
    }
    Ok(start_pipe(global_object, source, dest, call.argument(1)).unwrap_or_else(|error| rejected_with(global_object, error)))
}

pub(super) fn rs_pipe_through(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let source = call.this_value();
    if !readable::is_stream(source) {
        return Err(throw_invalid_this(global_object, Kind::ReadableStream));
    }
    let transform = call.argument(0);
    if !transform.is_object() {
        return Err(throw_native_type_error(global_object, "pipeThrough() expects an object with 'readable' and 'writable' properties"));
    }
    let readable = get_property(global_object, transform, "readable")?;
    if !readable::is_stream(readable) {
        return Err(throw_native_type_error(global_object, "The transform's 'readable' property must be a ReadableStream"));
    }
    let writable = get_property(global_object, transform, "writable")?;
    if !writable_js::is_stream(writable) {
        return Err(throw_native_type_error(global_object, "The transform's 'writable' property must be a WritableStream"));
    }
    if get_property(global_object, source, "locked")?.to_boolean() {
        return Err(throw_native_type_error(global_object, "Cannot pipe a locked ReadableStream"));
    }
    if get_property(global_object, writable, "locked")?.to_boolean() {
        return Err(throw_native_type_error(global_object, "Cannot pipe to a locked WritableStream"));
    }
    match start_pipe(global_object, source, writable, call.argument(1)) {
        Ok(promise) => mark_handled(promise),
        Err(error) => return Err(super::readable::rethrow(global_object, error)),
    }
    Ok(readable)
}
