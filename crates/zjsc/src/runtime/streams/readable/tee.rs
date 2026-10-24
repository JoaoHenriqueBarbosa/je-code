//! `ReadableStream.prototype.tee`: o `ReadableStreamDefaultTee` do WebKit. O stream de origem fica travado por um
//! leitor padrão visível (`locked` vira `true`), e os dois ramos são streams nativos cujo `pull` lê a origem uma vez e
//! entrega o pedaço a quem não cancelou; o mesmo valor vai aos dois. Uma origem de bytes segue o
//! `ReadableByteStreamTee` de `byte_tee.rs`.
use super::byte_tee::tee_bytes;
use super::*;
use crate::runtime::js_array::construct_array;

/// O estado de cancelamento dos dois ramos, comum ao `tee` padrão e ao de bytes: quem cancelou, o motivo e a promessa
/// que o cancelamento do último ramo (ou o fim da origem) resolve.
pub(super) struct Cancellation {
    stream: StreamRef,
    pub(super) canceled: [bool; 2],
    reasons: [JSValue; 2],
    pub(super) promise: JSValue,
}

pub(super) type CancelRef = Rc<RefCell<Cancellation>>;

impl Traced for Cancellation {
    fn trace(&self, out: &mut Vec<JSValue>) {
        self.reasons.trace(out);
        self.promise.trace(out);
    }
}

impl Cancellation {
    pub(super) fn create(global_object: &JSGlobalObject, stream: &StreamRef) -> CancelRef {
        Rc::new(RefCell::new(Cancellation { stream: stream.clone(), canceled: [false; 2], reasons: [JSValue::undefined(); 2], promise: new_promise(global_object) }))
    }
}

/// Resolve a promessa de cancelamento se algum ramo ainda lê (`canceled1 false` ou `canceled2 false`).
pub(super) fn resolve_unless_both_canceled(global_object: &JSGlobalObject, cancellation: &CancelRef) {
    let (canceled, promise) = {
        let inner = cancellation.borrow();
        (inner.canceled, inner.promise)
    };
    if !canceled[0] || !canceled[1] {
        resolve(global_object, promise, JSValue::undefined());
    }
}

/// Os passos de `cancel1Algorithm`/`cancel2Algorithm`: com os dois ramos cancelados, cancela a origem com o par de motivos.
pub(super) fn cancel_branch(global_object: &JSGlobalObject, cancellation: &CancelRef, index: usize, reason: JSValue) -> JSValue {
    let both = {
        let mut inner = cancellation.borrow_mut();
        inner.canceled[index] = true;
        inner.reasons[index] = reason;
        inner.canceled[0] && inner.canceled[1]
    };
    if both {
        let (stream, reasons) = {
            let inner = cancellation.borrow();
            (inner.stream.clone(), inner.reasons)
        };
        let composite = construct_array(global_object.vm(), &global_object.array_structure(), &reasons).as_value();
        let outcome = stream_cancel(global_object, &stream, composite);
        let promise = cancellation.borrow().promise;
        resolve(global_object, promise, outcome);
    }
    cancellation.borrow().promise
}

struct Shared {
    stream: StreamRef,
    reader: ReaderRef,
    reading: bool,
    read_again: bool,
    branches: [Option<ReadableHandle>; 2],
    cancellation: CancelRef,
}

type SharedRef = Rc<RefCell<Shared>>;

impl Traced for Shared {
    /// O stream, o leitor e os ramos são estado de um `OBJECTS`; só o cancelamento segura valores próprios.
    fn trace(&self, out: &mut Vec<JSValue>) {
        self.cancellation.trace(out);
    }
}

/// Um ramo do `tee`: a fonte nativa do stream devolvido em `index`.
struct Branch {
    shared: SharedRef,
    index: usize,
}

impl NativeSource for Branch {
    fn pull(&self, global_object: &JSGlobalObject) -> JSValue {
        pull(global_object, &self.shared);
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

fn branches_of(shared: &SharedRef) -> [Option<ReadableHandle>; 2] {
    shared.borrow().branches.clone()
}

fn pull(global_object: &JSGlobalObject, shared: &SharedRef) {
    let (stream, reader) = {
        let mut inner = shared.borrow_mut();
        if inner.reading {
            inner.read_again = true;
            return;
        }
        inner.reading = true;
        (inner.stream.clone(), inner.reader.clone())
    };
    let promise = new_promise(global_object);
    if reader_read_request(global_object, &stream, &reader, promise).is_err() {
        take_error(global_object);
        shared.borrow_mut().reading = false;
        return;
    }
    let on_chunk = shared.clone();
    let fulfilled = reaction(global_object, move |global_object, result| {
        deliver(global_object, &on_chunk, result)
    });
    let on_error = shared.clone();
    let rejected = reaction(global_object, move |_, _| {
        on_error.borrow_mut().reading = false;
        Ok(())
    });
    then(global_object, promise, fulfilled, rejected, JSValue::undefined());
}

/// Os passos de `chunk`/`close` do pedido de leitura: entrega aos ramos que não cancelaram.
fn deliver(global_object: &JSGlobalObject, shared: &SharedRef, result: JSValue) -> Result<(), Thrown> {
    let branches = branches_of(shared);
    let done = get_property(global_object, result, "done")?;
    let cancellation = shared.borrow().cancellation.clone();
    let canceled = cancellation.borrow().canceled;
    if matches!(done, JSValue::Bool(true)) {
        shared.borrow_mut().reading = false;
        for (branch, canceled) in branches.iter().zip(canceled) {
            if let (Some(branch), false) = (branch, canceled) {
                if branch.can_close_or_enqueue() {
                    branch.close(global_object);
                }
            }
        }
        resolve_unless_both_canceled(global_object, &cancellation);
        return Ok(());
    }
    let chunk = get_property(global_object, result, "value")?;
    shared.borrow_mut().read_again = false;
    for (branch, canceled) in branches.iter().zip(canceled) {
        if let (Some(branch), false) = (branch, canceled) {
            if branch.can_close_or_enqueue() {
                let _ = branch.enqueue(global_object, chunk);
            }
        }
    }
    let again = {
        let mut inner = shared.borrow_mut();
        inner.reading = false;
        inner.read_again
    };
    if again {
        pull(global_object, shared);
    }
    Ok(())
}

/// `tee()`: trava a origem e devolve `[ramo1, ramo2]`.
pub(in super::super) fn rs_tee(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let values = tee_of(global_object, call.this_value()).ok_or_else(|| invalid_this(global_object, "ReadableStream"))??;
    Ok(construct_array(global_object.vm(), &global_object.array_structure(), &values).as_value())
}

/// O `tee` de `value` (a origem de um corpo, por exemplo): `None` se não é um `ReadableStream`, `Err` se está travado.
pub(crate) fn tee_of(global_object: &JSGlobalObject, value: JSValue) -> Option<Result<[JSValue; 2], Thrown>> {
    stream_of(value).map(|stream| if stream.borrow().bytes { tee_bytes(global_object, stream) } else { tee_stream(global_object, stream) })
}

fn tee_stream(global_object: &JSGlobalObject, stream: StreamRef) -> Result<[JSValue; 2], Thrown> {
    let reader_value = create_reader(global_object, &stream, &reader_structure(global_object, ReaderKind::Default), ReaderKind::Default)?;
    let reader = reader_of(reader_value, ReaderKind::Default).ok_or_else(|| invalid_this(global_object, "ReadableStream"))?;
    let closed = reader.borrow().closed;
    let cancellation = Cancellation::create(global_object, &stream);
    let shared = Rc::new(RefCell::new(Shared { stream, reader, reading: false, read_again: false, branches: [None, None], cancellation: cancellation.clone() }));
    let mut values = [JSValue::undefined(); 2];
    for (index, value) in values.iter_mut().enumerate() {
        let (branch, handle) = create_native(global_object, Rc::new(Branch { shared: shared.clone(), index }), 1.0, None, JSValue::undefined());
        shared.borrow_mut().branches[index] = Some(handle);
        *value = branch;
    }
    let on_closed_error = shared.clone();
    let rejected = reaction(global_object, move |global_object, error| {
        for branch in branches_of(&on_closed_error).iter().flatten() {
            branch.error(global_object, error);
        }
        let canceled = cancellation.borrow().canceled;
        if !canceled[0] && !canceled[1] {
            let promise = cancellation.borrow().promise;
            resolve(global_object, promise, JSValue::undefined());
        }
        Ok(())
    });
    let fulfilled = reaction(global_object, |_, _| Ok(()));
    then(global_object, closed, fulfilled, rejected, JSValue::undefined());
    Ok(values)
}
