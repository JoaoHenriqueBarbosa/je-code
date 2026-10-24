//! `ReadableByteStreamTee` do WebKit/spec: o `tee` de um stream de bytes. Os dois ramos são streams de bytes nativos
//! (`getReader({ mode: 'byob' })` funciona neles). A origem fica travada por um leitor que troca de tipo conforme o
//! `byobRequest` do ramo que puxou: sem `byobRequest` lê com o leitor padrão e enfileira nos dois ramos (o segundo com
//! uma cópia); com `byobRequest` lê com o leitor BYOB sobre a view dele, responde ao ramo que puxou com
//! `respondWithNewView` e enfileira uma cópia no outro. Cancelamento e erro seguem o mesmo molde do `tee` padrão
//! ([`super::tee::Cancellation`]).
use super::tee::{cancel_branch, resolve_unless_both_canceled, CancelRef, Cancellation};
use super::*;
use crate::runtime::text_decoder::input_bytes;
use crate::runtime::uint8_array_base64::create_uint8_array;

struct Shared {
    stream: StreamRef,
    /// O leitor atual da origem: padrão ou BYOB.
    reader: ReaderRef,
    reading: bool,
    read_again: [bool; 2],
    branches: [Option<ByteRef>; 2],
    cancellation: CancelRef,
}

type SharedRef = Rc<RefCell<Shared>>;

impl Traced for Shared {
    /// O stream, o leitor e os ramos são estado de um `OBJECTS`; só o cancelamento segura valores próprios.
    fn trace(&self, out: &mut Vec<JSValue>) {
        self.cancellation.trace(out);
    }
}

/// Um ramo do `tee`: a fonte nativa do stream de bytes devolvido em `index`.
struct Branch {
    shared: SharedRef,
    index: usize,
}

impl NativeSource for Branch {
    fn pull(&self, global_object: &JSGlobalObject) -> JSValue {
        pull(global_object, &self.shared, self.index);
        resolved_with(global_object, JSValue::undefined())
    }

    fn cancel(&self, global_object: &JSGlobalObject, reason: JSValue) -> JSValue {
        let cancellation = self.shared.borrow().cancellation.clone();
        cancel_branch(global_object, &cancellation, self.index, reason)
    }

    fn trace(&self, out: &mut Vec<JSValue>) {
        self.shared.trace(out);
    }
}

/// `CloneAsUint8Array`: um `Uint8Array` novo com os bytes da view.
fn clone_as_uint8_array(global_object: &JSGlobalObject, chunk: JSValue) -> Result<JSValue, JSValue> {
    let bytes = input_bytes(chunk).ok_or_else(|| type_error_value(global_object, "ReadableStream.prototype.tee received a chunk that is not a BufferSource"))?;
    let array = create_uint8_array(global_object, bytes.len()).map_err(|thrown| error_of(global_object, thrown))?;
    array.with_vector_mut(|destination| destination.copy_from_slice(&bytes));
    Ok(array.as_value())
}

fn branch_of(shared: &SharedRef, index: usize) -> Option<ByteRef> {
    shared.borrow().branches[index].clone()
}

fn canceled_of(shared: &SharedRef) -> [bool; 2] {
    let cancellation = shared.borrow().cancellation.clone();
    let canceled = cancellation.borrow().canceled;
    canceled
}

/// O erro de uma operação sobre um ramo que o `tee` ignora (a spec as faz com `!`): retira a exceção do `VM`.
fn ignore(global_object: &JSGlobalObject, outcome: Result<impl Sized, Thrown>) {
    if outcome.is_err() {
        take_error(global_object);
    }
}

/// `ReadableByteStreamControllerEnqueue` no ramo, se ele ainda aceita.
fn enqueue_into(global_object: &JSGlobalObject, shared: &SharedRef, index: usize, chunk: JSValue) {
    if let Some(branch) = branch_of(shared, index).filter(|branch| branch.borrow().core.can_close_or_enqueue()) {
        ignore(global_object, byte_enqueue_view(global_object, &branch, chunk));
    }
}

/// `ReadableByteStreamControllerClose` no ramo, se ele ainda aceita.
fn close_branch(global_object: &JSGlobalObject, shared: &SharedRef, index: usize) {
    if let Some(branch) = branch_of(shared, index).filter(|branch| branch.borrow().core.can_close_or_enqueue()) {
        ignore(global_object, byte_close(global_object, &branch));
    }
}

/// O ramo tem `byobRequest`: há um `pullInto` pendente.
fn has_byob_request(shared: &SharedRef, index: usize) -> bool {
    branch_of(shared, index).is_some_and(|branch| branch.borrow().core.byob_request_view().is_some())
}

/// `ReadableByteStreamControllerRespond(branch.controller, 0)`.
fn respond_zero(global_object: &JSGlobalObject, shared: &SharedRef, index: usize) {
    if let Some(branch) = branch_of(shared, index) {
        ignore(global_object, byte_respond(global_object, &branch, 0));
    }
}

/// `ReadableByteStreamControllerRespondWithNewView(branch.controller.byobRequest, view)`.
fn respond_with_view(global_object: &JSGlobalObject, shared: &SharedRef, index: usize, view: JSValue) {
    let Some(branch) = branch_of(shared, index) else { return };
    let request = byob_request_value(global_object, &branch).map(request_of);
    match request {
        Ok(Some(request)) => ignore(global_object, respond_with_new_view_on(global_object, &request, view)),
        Ok(None) => {}
        Err(_) => {
            take_error(global_object);
        }
    }
}

/// O erro de clonagem: os dois ramos entram em erro, a origem é cancelada e a promessa de cancelamento se resolve.
fn fail_both(global_object: &JSGlobalObject, shared: &SharedRef, error: JSValue) {
    for index in 0..2 {
        if let Some(branch) = branch_of(shared, index) {
            ignore(global_object, byte_controller_error(global_object, &branch, error));
        }
    }
    let (stream, cancellation) = {
        let inner = shared.borrow();
        (inner.stream.clone(), inner.cancellation.clone())
    };
    let outcome = stream_cancel(global_object, &stream, error);
    let promise = cancellation.borrow().promise;
    resolve(global_object, promise, outcome);
}

/// `forwardReaderError`: se o leitor `reader` (ainda o atual) falha, os dois ramos entram em erro.
fn forward_reader_error(global_object: &JSGlobalObject, shared: &SharedRef, reader: &ReaderRef) {
    let closed = reader.borrow().closed;
    let on_error = shared.clone();
    let current = reader.clone();
    let rejected = reaction(global_object, move |global_object, error| {
        if !Rc::ptr_eq(&on_error.borrow().reader, &current) {
            return Ok(());
        }
        for index in 0..2 {
            if let Some(branch) = branch_of(&on_error, index) {
                byte_controller_error(global_object, &branch, error)?;
            }
        }
        let cancellation = on_error.borrow().cancellation.clone();
        resolve_unless_both_canceled(global_object, &cancellation);
        Ok(())
    });
    let fulfilled = reaction(global_object, |_, _| Ok(()));
    then(global_object, closed, fulfilled, rejected, JSValue::undefined());
}

/// O leitor da origem do tipo `kind`: solta o atual e adquire um novo se o tipo é outro.
fn use_reader(global_object: &JSGlobalObject, shared: &SharedRef, kind: ReaderKind) -> Result<ReaderRef, Thrown> {
    let current = shared.borrow().reader.clone();
    if current.borrow().kind == kind {
        return Ok(current);
    }
    let stream = shared.borrow().stream.clone();
    reader_release(global_object, &current);
    let value = create_reader(global_object, &stream, &reader_structure(global_object, kind), kind)?;
    let reader = reader_of(value, kind).ok_or_else(|| invalid_this(global_object, "ReadableStream"))?;
    shared.borrow_mut().reader = reader.clone();
    forward_reader_error(global_object, shared, &reader);
    Ok(reader)
}

/// `pull1Algorithm`/`pull2Algorithm`.
fn pull(global_object: &JSGlobalObject, shared: &SharedRef, index: usize) {
    {
        let mut inner = shared.borrow_mut();
        if inner.reading {
            inner.read_again[index] = true;
            return;
        }
        inner.reading = true;
    }
    if start_read(global_object, shared, index).is_err() {
        take_error(global_object);
        shared.borrow_mut().reading = false;
    }
}

/// Lê a origem pelo leitor padrão (ramo sem `byobRequest`) ou pelo BYOB (sobre a view do `byobRequest`).
fn start_read(global_object: &JSGlobalObject, shared: &SharedRef, index: usize) -> Result<(), Thrown> {
    let Some(branch) = branch_of(shared, index) else { return Ok(()) };
    let view = request_of(byob_request_value(global_object, &branch)?).and_then(|request| {
        let view = request.borrow().view;
        view
    });
    let stream = shared.borrow().stream.clone();
    match view {
        None => {
            let promise = new_promise(global_object);
            let reader = use_reader(global_object, shared, ReaderKind::Default)?;
            reader_read_request(global_object, &stream, &reader, promise)?;
            on_read(global_object, shared, promise, default_chunk);
        }
        Some(view) => {
            let reader = use_reader(global_object, shared, ReaderKind::Byob)?;
            match byob_read(global_object, &reader, view, JSValue::undefined()) {
                Ok(read) => on_read(global_object, shared, read, move |global_object, shared, result| byob_chunk(global_object, shared, result, index)),
                Err(_) => shared.borrow_mut().reading = false,
            }
        }
    }
    Ok(())
}

/// Liga `handler` ao resultado do pedido de leitura; a rejeição só desliga `reading`.
fn on_read(global_object: &JSGlobalObject, shared: &SharedRef, promise: JSValue, handler: impl Fn(&JSGlobalObject, &SharedRef, JSValue) -> Result<(), Thrown> + 'static) {
    let on_chunk = shared.clone();
    let fulfilled = reaction(global_object, move |global_object, result| handler(global_object, &on_chunk, result));
    let on_error = shared.clone();
    let rejected = reaction(global_object, move |_, _| {
        on_error.borrow_mut().reading = false;
        Ok(())
    });
    then(global_object, promise, fulfilled, rejected, JSValue::undefined());
}

/// O fim dos passos de `chunk`: `reading` desliga e um `pull` adiado roda (o do ramo 1 antes do 2).
fn finish_pull(global_object: &JSGlobalObject, shared: &SharedRef) {
    let again = {
        let mut inner = shared.borrow_mut();
        inner.reading = false;
        inner.read_again
    };
    if let Some(index) = again.iter().position(|&again| again) {
        pull(global_object, shared, index);
    }
}

/// Os passos de `close` dos dois leitores: fecha os ramos que não cancelaram e conclui o `byobRequest` pendente de cada
/// um (`byob` é o ramo que puxou com o leitor BYOB e o último pedaço devolvido por ele).
fn close_branches(global_object: &JSGlobalObject, shared: &SharedRef, byob: Option<(usize, JSValue)>) {
    shared.borrow_mut().reading = false;
    let canceled = canceled_of(shared);
    for index in 0..2 {
        if !canceled[index] {
            close_branch(global_object, shared, index);
        }
    }
    match byob {
        None => {
            for index in 0..2 {
                if has_byob_request(shared, index) {
                    respond_zero(global_object, shared, index);
                }
            }
        }
        Some((index, chunk)) => {
            let other = 1 - index;
            if !chunk.is_undefined() {
                if !canceled[index] {
                    respond_with_view(global_object, shared, index, chunk);
                }
                if !canceled[other] && has_byob_request(shared, other) {
                    respond_zero(global_object, shared, other);
                }
            }
        }
    }
    let cancellation = shared.borrow().cancellation.clone();
    resolve_unless_both_canceled(global_object, &cancellation);
}

/// Os passos de `chunk`/`close` do pedido de leitura do leitor padrão.
fn default_chunk(global_object: &JSGlobalObject, shared: &SharedRef, result: JSValue) -> Result<(), Thrown> {
    if matches!(get_property(global_object, result, "done")?, JSValue::Bool(true)) {
        close_branches(global_object, shared, None);
        return Ok(());
    }
    let chunk = get_property(global_object, result, "value")?;
    shared.borrow_mut().read_again = [false; 2];
    let canceled = canceled_of(shared);
    let mut second = chunk;
    if !canceled[0] && !canceled[1] {
        match clone_as_uint8_array(global_object, chunk) {
            Ok(copy) => second = copy,
            Err(error) => {
                fail_both(global_object, shared, error);
                return Ok(());
            }
        }
    }
    for (index, value) in [chunk, second].into_iter().enumerate() {
        if !canceled[index] {
            enqueue_into(global_object, shared, index, value);
        }
    }
    finish_pull(global_object, shared);
    Ok(())
}

/// Os passos de `chunk`/`close` do pedido de leitura-em-view do leitor BYOB, para o ramo `index` que puxou.
fn byob_chunk(global_object: &JSGlobalObject, shared: &SharedRef, result: JSValue, index: usize) -> Result<(), Thrown> {
    let chunk = get_property(global_object, result, "value")?;
    if matches!(get_property(global_object, result, "done")?, JSValue::Bool(true)) {
        close_branches(global_object, shared, Some((index, chunk)));
        return Ok(());
    }
    shared.borrow_mut().read_again = [false; 2];
    let canceled = canceled_of(shared);
    let other = 1 - index;
    if canceled[other] {
        if !canceled[index] {
            respond_with_view(global_object, shared, index, chunk);
        }
    } else {
        let copy = match clone_as_uint8_array(global_object, chunk) {
            Ok(copy) => copy,
            Err(error) => {
                fail_both(global_object, shared, error);
                return Ok(());
            }
        };
        if !canceled[index] {
            respond_with_view(global_object, shared, index, chunk);
        }
        enqueue_into(global_object, shared, other, copy);
    }
    finish_pull(global_object, shared);
    Ok(())
}

/// `ReadableByteStreamTee(stream)`: trava a origem com um leitor padrão e devolve `[ramo1, ramo2]`.
pub(super) fn tee_bytes(global_object: &JSGlobalObject, stream: StreamRef) -> Result<[JSValue; 2], Thrown> {
    let reader_value = create_reader(global_object, &stream, &reader_structure(global_object, ReaderKind::Default), ReaderKind::Default)?;
    let reader = reader_of(reader_value, ReaderKind::Default).ok_or_else(|| invalid_this(global_object, "ReadableStream"))?;
    let cancellation = Cancellation::create(global_object, &stream);
    let shared = Rc::new(RefCell::new(Shared { stream, reader: reader.clone(), reading: false, read_again: [false; 2], branches: [None, None], cancellation }));
    let mut values = [JSValue::undefined(); 2];
    for (index, value) in values.iter_mut().enumerate() {
        let (branch, state) = create_native_bytes(global_object, Rc::new(Branch { shared: shared.clone(), index }), 0.0);
        shared.borrow_mut().branches[index] = Some(state);
        *value = branch;
    }
    forward_reader_error(global_object, &shared, &reader);
    Ok(values)
}
