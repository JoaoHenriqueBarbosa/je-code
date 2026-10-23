//! Porte de `src/vs/base/common/stream.ts`: `Readable`, `ReadableStream`, `WriteableStream` e os
//! helpers (`consumeStream`, `listenStream`, `peekStream`, `transform`, `prefixedStream`...).
//!
//! O original emula a forma da API de stream do Node.js. Em Rust:
//!
//! - o `on(evento, callback)` de três sobrecargas é [`ReadableStreamEvents::on`] com o enum
//!   [`StreamListener`]; um callback é um `Rc<dyn Fn>`, e `removeListener` compara por ponteiro
//!   (`indexOf(callback)` do original compara por identidade da função);
//! - um dado chega ao ouvinte por referência (`&T`), porque o mesmo dado vai a todos os ouvintes de
//!   `data`; quem guarda o dado o clona, então `T: Clone` (o `VSBuffer` é um handle barato);
//! - um stream é compartilhado por `Rc`, e os métodos recebem `&self` (estado em `RefCell`). Nenhum
//!   `RefCell` fica emprestado enquanto um ouvinte roda, então um ouvinte pode chamar `pause`,
//!   `removeListener` ou `write` no próprio stream, como no original;
//! - `Readable<T>` é um trait com `read(&mut self) -> Option<T>` (o `null` é `None`), e qualquer
//!   `FnMut() -> Option<T>` é um `Readable`;
//! - `consumeStream` e `peekStream` devolvem [`Promise`] do `runtime`.
//!
//! Pendente: `isReadable`, `isReadableStream` e `isReadableBufferedStream` do original conferem a forma
//! de um valor `unknown` (duck typing). Em Rust o tipo já diz o que o valor é, então não há o que
//! conferir em tempo de execução.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use crate::cancellation::CancellationToken;
use crate::errors::{self, Error};
use crate::promise::{Promise, Resolver};

/// `ReadableStreamEventPayload<T>`.
pub enum ReadableStreamEventPayload<T> {
    Data(T),
    Error(Error),
    End,
}

pub type DataListener<T> = Rc<dyn Fn(&T)>;
pub type ErrorListener = Rc<dyn Fn(&Error)>;
pub type EndListener = Rc<dyn Fn()>;

/// Um ouvinte de um dos três eventos (`'data'`, `'error'`, `'end'`).
pub enum StreamListener<T> {
    Data(DataListener<T>),
    Error(ErrorListener),
    End(EndListener),
}

impl<T> Clone for StreamListener<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Data(l) => Self::Data(l.clone()),
            Self::Error(l) => Self::Error(l.clone()),
            Self::End(l) => Self::End(l.clone()),
        }
    }
}

impl<T> StreamListener<T> {
    pub fn data(f: impl Fn(&T) + 'static) -> Self {
        Self::Data(Rc::new(f))
    }

    pub fn error(f: impl Fn(&Error) + 'static) -> Self {
        Self::Error(Rc::new(f))
    }

    pub fn end(f: impl Fn() + 'static) -> Self {
        Self::End(Rc::new(f))
    }

    /// É o mesmo ouvinte (mesmo evento, mesmo callback)? Identidade, como o `indexOf` do original.
    pub fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Data(a), Self::Data(b)) => Rc::ptr_eq(a, b),
            (Self::Error(a), Self::Error(b)) => Rc::ptr_eq(a, b),
            (Self::End(a), Self::End(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// `ReadableStreamEvents<T>`.
pub trait ReadableStreamEvents<T: 'static> {
    /// `on('data' | 'error' | 'end', callback)`.
    ///
    /// NOTE: PLEASE UNDERSTAND THAT ADDING A DATA LISTENER CAN TURN THE STREAM INTO FLOWING MODE. IT IS
    /// THEREFOR THE LAST LISTENER THAT SHOULD BE ADDED AND NOT THE FIRST. Use [`listen_stream`] as a
    /// helper method to listen to stream events in the right order.
    fn on(&self, listener: StreamListener<T>);
}

/// A interface that emulates the API shape of a node.js readable stream for use in native and web
/// environments.
pub trait ReadableStream<T: 'static>: ReadableStreamEvents<T> {
    /// Stops emitting any events until resume() is called.
    fn pause(&self);

    /// Starts emitting events again after pause() was called.
    fn resume(&self);

    /// Destroys the stream and stops emitting any event.
    fn destroy(&self);

    /// Allows to remove a listener that was previously added.
    fn remove_listener(&self, listener: &StreamListener<T>);
}

/// A interface that emulates the API shape of a node.js readable for use in native and web environments.
pub trait Readable<T> {
    /// Read data from the underlying source. Will return `None` to indicate that no more data can be read.
    fn read(&mut self) -> Option<T>;
}

impl<T, F: FnMut() -> Option<T>> Readable<T> for F {
    fn read(&mut self) -> Option<T> {
        self()
    }
}

/// A interface that emulates the API shape of a node.js writeable stream for use in native and web
/// environments.
pub trait WriteableStream<T: 'static>: ReadableStream<T> {
    /// Writing data to the stream will trigger the on('data') event listener if the stream is flowing and
    /// buffer the data otherwise until the stream is flowing.
    ///
    /// If a `highWaterMark` is configured and writing to the stream reaches this mark, a promise will be
    /// returned that should be awaited on before writing more data. Otherwise there is a risk of buffering
    /// a large number of data chunks without consumer.
    fn write(&self, data: T) -> Option<Promise<()>>;

    /// Signals an error to the consumer of the stream via the on('error') handler if the stream is flowing.
    ///
    /// NOTE: call `end` to signal that the stream has ended, this DOES NOT happen automatically from `error`.
    fn error(&self, error: Error);

    /// Signals the end of the stream to the consumer. If the result is provided, will trigger the on('data')
    /// event listener if the stream is flowing and buffer the data otherwise until the stream is flowing.
    fn end(&self, result: Option<T>);
}

/// A stream that has a buffer already read. Returns the original stream that was read as well as the
/// chunks that got read.
///
/// The `ended` flag indicates if the stream has been fully consumed.
pub struct ReadableBufferedStream<T: 'static> {
    /// The original stream that is being read.
    pub stream: Rc<dyn ReadableStream<T>>,
    /// An array of chunks already read from this stream.
    pub buffer: Vec<T>,
    /// Signals if the stream has ended or not. If not, consumers should continue to read from the stream
    /// until consumed.
    pub ended: bool,
}

impl<T: Clone + 'static> Clone for ReadableBufferedStream<T> {
    fn clone(&self) -> Self {
        Self { stream: self.stream.clone(), buffer: self.buffer.clone(), ended: self.ended }
    }
}

/// `IReducer<T, R>`.
pub type Reducer<T, R = T> = Rc<dyn Fn(Vec<T>) -> R>;

/// `ITransformer<Original, Transformed>`.
pub struct Transformer<Original, Transformed> {
    pub data: Rc<dyn Fn(&Original) -> Transformed>,
    pub error: Option<Rc<dyn Fn(Error) -> Error>>,
}

/// `WriteableStreamOptions`.
#[derive(Clone, Copy, Default)]
pub struct WriteableStreamOptions {
    /// The number of objects to buffer before WriteableStream#write() signals back that the buffer is
    /// full. Can be used to reduce the memory pressure when the stream is not flowing.
    pub high_water_mark: Option<usize>,
}

/// `newWriteableStream`. `reducer` é `None` para o `null` do original: dados que não se reduzem a um só.
pub fn new_writeable_stream<T: Clone + 'static>(reducer: Option<Reducer<T>>, options: Option<WriteableStreamOptions>) -> Rc<WriteableStreamImpl<T>> {
    Rc::new(WriteableStreamImpl::new(reducer, options.unwrap_or_default()))
}

struct State<T: 'static> {
    flowing: bool,
    ended: bool,
    destroyed: bool,
    buffered_data: Vec<T>,
    buffered_errors: Vec<Error>,
    data_listeners: Vec<DataListener<T>>,
    error_listeners: Vec<ErrorListener>,
    end_listeners: Vec<EndListener>,
    pending_write_promises: Vec<Resolver<()>>,
}

pub struct WriteableStreamImpl<T: Clone + 'static> {
    reducer: Option<Reducer<T>>,
    options: WriteableStreamOptions,
    state: RefCell<State<T>>,
}

impl<T: Clone + 'static> WriteableStreamImpl<T> {
    /// `reducer`: a function that reduces the buffered data into a single object; because some objects
    /// can be complex and non-reducible, we also allow passing `None` to skip the reduce step.
    fn new(reducer: Option<Reducer<T>>, options: WriteableStreamOptions) -> Self {
        Self {
            reducer,
            options,
            state: RefCell::new(State {
                flowing: false,
                ended: false,
                destroyed: false,
                buffered_data: Vec::new(),
                buffered_errors: Vec::new(),
                data_listeners: Vec::new(),
                error_listeners: Vec::new(),
                end_listeners: Vec::new(),
                pending_write_promises: Vec::new(),
            }),
        }
    }

    // Os `emit*` copiam a lista de ouvintes (o `slice(0)` do original) para que um ouvinte que mexa na
    // lista não altere a entrega em andamento, e soltam o `RefCell` antes de chamar qualquer um.

    fn emit_data(&self, data: &T) {
        let listeners = self.state.borrow().data_listeners.clone();
        for listener in listeners {
            listener(data);
        }
    }

    fn emit_error(&self, error: &Error) {
        let listeners = self.state.borrow().error_listeners.clone();
        if listeners.is_empty() {
            errors::on_unexpected_error(error); // nobody listened to this error so we log it as unexpected
        } else {
            for listener in listeners {
                listener(error);
            }
        }
    }

    fn emit_end(&self) {
        let listeners = self.state.borrow().end_listeners.clone();
        for listener in listeners {
            listener();
        }
    }

    fn flow_data(&self) {
        // if buffer is empty, nothing to do
        if self.state.borrow().buffered_data.is_empty() {
            return;
        }

        // if buffer data can be reduced into a single object, emit the reduced data
        if let Some(reducer) = &self.reducer {
            let buffered = self.state.borrow().buffered_data.clone();
            let full_data_buffer = reducer(buffered);
            self.emit_data(&full_data_buffer);
        } else {
            // otherwise emit each buffered data instance individually. O `for...of` do original lê o
            // vetor vivo, então um dado escrito por um ouvinte durante a entrega também é entregue.
            let mut index = 0;
            loop {
                let next = self.state.borrow().buffered_data.get(index).cloned();
                let Some(data) = next else { break };
                self.emit_data(&data);
                index += 1;
            }
        }

        let pending = {
            let mut state = self.state.borrow_mut();
            state.buffered_data.clear();
            // when the buffer is empty, resolve all pending writers
            std::mem::take(&mut state.pending_write_promises)
        };
        for resolver in pending {
            resolver.resolve(());
        }
    }

    fn flow_errors(&self) {
        if self.state.borrow().error_listeners.is_empty() {
            return;
        }
        let mut index = 0;
        loop {
            let next = self.state.borrow().buffered_errors.get(index).cloned();
            let Some(error) = next else { break };
            self.emit_error(&error);
            index += 1;
        }
        self.state.borrow_mut().buffered_errors.clear();
    }

    fn flow_end(&self) -> bool {
        if self.state.borrow().ended {
            self.emit_end();

            return !self.state.borrow().end_listeners.is_empty();
        }

        false
    }
}

impl<T: Clone + 'static> ReadableStreamEvents<T> for WriteableStreamImpl<T> {
    fn on(&self, listener: StreamListener<T>) {
        if self.state.borrow().destroyed {
            return;
        }

        match listener {
            StreamListener::Data(callback) => {
                self.state.borrow_mut().data_listeners.push(callback);

                // switch into flowing mode as soon as the first 'data'
                // listener is added and we are not yet in flowing mode
                self.resume();
            }

            StreamListener::End(callback) => {
                self.state.borrow_mut().end_listeners.push(callback);

                // emit 'end' event directly if we are flowing
                // and the end has already been reached
                //
                // finish() when it went through
                if self.state.borrow().flowing && self.flow_end() {
                    self.destroy();
                }
            }

            StreamListener::Error(callback) => {
                self.state.borrow_mut().error_listeners.push(callback);

                // emit buffered 'error' events unless done already
                // now that we know that we have at least one listener
                if self.state.borrow().flowing {
                    self.flow_errors();
                }
            }
        }
    }
}

impl<T: Clone + 'static> ReadableStream<T> for WriteableStreamImpl<T> {
    fn pause(&self) {
        let mut state = self.state.borrow_mut();
        if state.destroyed {
            return;
        }

        state.flowing = false;
    }

    fn resume(&self) {
        {
            let mut state = self.state.borrow_mut();
            if state.destroyed || state.flowing {
                return;
            }
            state.flowing = true;
        }

        // emit buffered events
        self.flow_data();
        self.flow_errors();
        self.flow_end();
    }

    fn destroy(&self) {
        let mut state = self.state.borrow_mut();
        if !state.destroyed {
            state.destroyed = true;
            state.ended = true;

            state.buffered_data.clear();
            state.buffered_errors.clear();

            state.data_listeners.clear();
            state.error_listeners.clear();
            state.end_listeners.clear();

            state.pending_write_promises.clear();
        }
    }

    fn remove_listener(&self, listener: &StreamListener<T>) {
        let mut state = self.state.borrow_mut();
        if state.destroyed {
            return;
        }

        match listener {
            StreamListener::Data(callback) => state.data_listeners.retain_first_not(|l| Rc::ptr_eq(l, callback)),
            StreamListener::End(callback) => state.end_listeners.retain_first_not(|l| Rc::ptr_eq(l, callback)),
            StreamListener::Error(callback) => state.error_listeners.retain_first_not(|l| Rc::ptr_eq(l, callback)),
        }
    }
}

/// `indexOf` seguido de `splice(index, 1)`: remove só a primeira ocorrência.
trait RemoveFirst<L> {
    fn retain_first_not(&mut self, is_target: impl Fn(&L) -> bool);
}

impl<L> RemoveFirst<L> for Vec<L> {
    fn retain_first_not(&mut self, is_target: impl Fn(&L) -> bool) {
        if let Some(index) = self.iter().position(is_target) {
            self.remove(index);
        }
    }
}

impl<T: Clone + 'static> WriteableStream<T> for WriteableStreamImpl<T> {
    fn write(&self, data: T) -> Option<Promise<()>> {
        if self.state.borrow().destroyed {
            return None;
        }

        // flowing: directly send the data to listeners
        if self.state.borrow().flowing {
            self.emit_data(&data);
            return None;
        }

        // not yet flowing: buffer data until flowing
        let mut state = self.state.borrow_mut();
        state.buffered_data.push(data);

        // highWaterMark: if configured, signal back when buffer reached limits
        if let Some(high_water_mark) = self.options.high_water_mark
            && state.buffered_data.len() > high_water_mark
        {
            let (promise, resolver) = Promise::new();
            state.pending_write_promises.push(resolver);
            return Some(promise);
        }
        None
    }

    fn error(&self, error: Error) {
        if self.state.borrow().destroyed {
            return;
        }

        // flowing: directly send the error to listeners
        if self.state.borrow().flowing {
            self.emit_error(&error);
        }
        // not yet flowing: buffer errors until flowing
        else {
            self.state.borrow_mut().buffered_errors.push(error);
        }
    }

    fn end(&self, result: Option<T>) {
        if self.state.borrow().destroyed {
            return;
        }

        // end with data if provided
        if let Some(result) = result {
            self.write(result);
        }

        // flowing: send end event to listeners
        if self.state.borrow().flowing {
            self.emit_end();

            self.destroy();
        }
        // not yet flowing: remember state
        else {
            self.state.borrow_mut().ended = true;
        }
    }
}

/// Helper to fully read a T readable into a T.
pub fn consume_readable<T>(readable: &mut dyn Readable<T>, reducer: impl FnOnce(Vec<T>) -> T) -> T {
    let mut chunks = Vec::new();

    while let Some(chunk) = readable.read() {
        chunks.push(chunk);
    }

    reducer(chunks)
}

/// O resultado de [`peek_readable`]: o dado inteiro, ou um `Readable` que ainda entrega tudo.
pub enum PeekedReadable<T> {
    Value(T),
    Readable(Box<dyn Readable<T>>),
}

/// Helper to read a T readable up to a maximum of chunks. If the limit is reached, will return a readable
/// instead to ensure all data can still be read.
pub fn peek_readable<T: 'static>(mut readable: Box<dyn Readable<T>>, reducer: impl FnOnce(Vec<T>) -> T, max_chunks: usize) -> PeekedReadable<T> {
    let mut chunks: Vec<T> = Vec::new();

    // `None` é o `undefined` do original (nada lido ainda); `Some(None)` é o `null` (fim do readable).
    let mut chunk: Option<Option<T>> = None;
    loop {
        let read = readable.read();
        let stop = read.is_none() || chunks.len() >= max_chunks;
        if stop {
            chunk = Some(read);
            break;
        }
        chunks.push(read.expect("não é o fim"));
    }

    // If the last chunk is null, it means we reached the end of
    // the readable and return all the data at once
    if matches!(chunk, Some(None)) && !chunks.is_empty() {
        return PeekedReadable::Value(reducer(chunks));
    }

    // Otherwise, we still have a chunk, it means we reached the maxChunks
    // value and as such we return a new Readable that first returns
    // the existing read chunks and then continues with reading from
    // the underlying readable.
    let mut pending: std::collections::VecDeque<T> = chunks.into();
    PeekedReadable::Readable(Box::new(move || {
        // First consume chunks from our array
        if let Some(first) = pending.pop_front() {
            return Some(first);
        }

        // Then ensure to return our last read chunk
        if let Some(last_read_chunk) = chunk.take() {
            // `take` marca o chunk como consumido, tenha sido `null` ou valorado.
            return last_read_chunk;
        }

        // Finally delegate back to the Readable
        readable.read()
    }))
}

fn consume_stream_with<T: Clone + 'static, R: Clone + 'static>(
    stream: &dyn ReadableStreamEvents<T>,
    reducer: Option<Reducer<T, R>>,
) -> Promise<Option<R>> {
    let (promise, resolver) = Promise::new();
    let chunks: Rc<RefCell<Vec<T>>> = Rc::default();

    let (on_data_chunks, on_data_has_reducer) = (chunks.clone(), reducer.is_some());
    let (on_error_resolver, on_error_has_reducer): (Resolver<Option<R>>, bool) = (resolver.clone(), reducer.is_some());
    listen_stream(
        stream,
        IStreamListener {
            on_data: Rc::new(move |chunk: &T| {
                if on_data_has_reducer {
                    on_data_chunks.borrow_mut().push(chunk.clone());
                }
            }),
            on_error: Rc::new(move |error: &Error| {
                if on_error_has_reducer {
                    on_error_resolver.reject(error.clone());
                } else {
                    on_error_resolver.resolve(None);
                }
            }),
            on_end: Rc::new(move || match &reducer {
                Some(reducer) => resolver.resolve(Some(reducer(std::mem::take(&mut chunks.borrow_mut())))),
                None => resolver.resolve(None),
            }),
        },
        None,
    );

    promise
}

/// Helper to fully read a T stream into a T, awaiting all the events.
pub fn consume_stream<T: Clone + 'static, R: Clone + 'static>(stream: &dyn ReadableStreamEvents<T>, reducer: Reducer<T, R>) -> Promise<R> {
    consume_stream_with(stream, Some(reducer)).then(|result| result.expect("com reducer o resultado é sempre valorado"))
}

/// Helper to consume a stream fully, awaiting all the events without caring about the data. Resolve
/// also on error, as the original does.
pub fn consume_stream_ignoring_data<T: Clone + 'static>(stream: &dyn ReadableStreamEvents<T>) -> Promise<()> {
    consume_stream_with::<T, ()>(stream, None).then(|_| ())
}

/// `IStreamListener<T>`.
pub struct IStreamListener<T> {
    /// The 'data' event is emitted whenever the stream is relinquishing ownership of a chunk of data to a
    /// consumer.
    pub on_data: Rc<dyn Fn(&T)>,
    /// Emitted when any error occurs.
    pub on_error: Rc<dyn Fn(&Error)>,
    /// The 'end' event is emitted when there is no more data to be consumed from the stream. The 'end'
    /// event will not be emitted unless the data is completely consumed.
    pub on_end: Rc<dyn Fn()>,
}

/// Helper to listen to all events of a T stream in proper order.
pub fn listen_stream<T: 'static>(stream: &dyn ReadableStreamEvents<T>, listener: IStreamListener<T>, token: Option<&CancellationToken>) {
    let cancelled = |token: &Option<CancellationToken>| token.as_ref().is_some_and(CancellationToken::is_cancellation_requested);

    let (t, on_error) = (token.cloned(), listener.on_error);
    stream.on(StreamListener::error(move |error| {
        if !cancelled(&t) {
            on_error(error);
        }
    }));

    let (t, on_end) = (token.cloned(), listener.on_end);
    stream.on(StreamListener::end(move || {
        if !cancelled(&t) {
            on_end();
        }
    }));

    // Adding the `data` listener will turn the stream
    // into flowing mode. As such it is important to
    // add this listener last (DO NOT CHANGE!)
    let (t, on_data) = (token.cloned(), listener.on_data);
    stream.on(StreamListener::data(move |data| {
        if !cancelled(&t) {
            on_data(data);
        }
    }));
}

/// Os três ouvintes de [`peek_stream`], por referência fraca: quem os mantém vivo é o stream, e a conta
/// só serve para removê-los (se o stream já os largou, não há o que remover).
struct PeekListeners<T: 'static> {
    data: Weak<dyn Fn(&T)>,
    error: Weak<dyn Fn(&Error)>,
    end: Weak<dyn Fn()>,
}

struct PeekState<T: Clone + 'static> {
    stream: Rc<dyn ReadableStream<T>>,
    buffer: RefCell<Vec<T>>,
    resolver: Resolver<ReadableBufferedStream<T>>,
    listeners: RefCell<Option<PeekListeners<T>>>,
}

impl<T: Clone + 'static> PeekState<T> {
    /// `streamListeners.dispose()`.
    fn dispose_listeners(&self) {
        let Some(listeners) = self.listeners.borrow_mut().take() else { return };
        if let Some(error) = listeners.error.upgrade() {
            self.stream.remove_listener(&StreamListener::Error(error));
        }
        if let Some(end) = listeners.end.upgrade() {
            self.stream.remove_listener(&StreamListener::End(end));
        }
        if let Some(data) = listeners.data.upgrade() {
            self.stream.remove_listener(&StreamListener::Data(data));
        }
    }

    fn snapshot(&self, ended: bool) -> ReadableBufferedStream<T> {
        ReadableBufferedStream { stream: self.stream.clone(), buffer: self.buffer.borrow().clone(), ended }
    }
}

/// Helper to peek up to `maxChunks` into a stream. The return type signals if the stream has ended or
/// not. If not, caller needs to add a `data` listener to continue reading.
pub fn peek_stream<T: Clone + 'static>(stream: Rc<dyn ReadableStream<T>>, max_chunks: usize) -> Promise<ReadableBufferedStream<T>> {
    let (promise, resolver) = Promise::new();
    let state = Rc::new(PeekState { stream, buffer: RefCell::default(), resolver, listeners: RefCell::new(None) });

    // Data Listener
    let s = state.clone();
    let data_listener: DataListener<T> = Rc::new(move |chunk: &T| {
        // Add to buffer
        s.buffer.borrow_mut().push(chunk.clone());

        // We reached maxChunks and thus need to return
        if s.buffer.borrow().len() > max_chunks {
            // Dispose any listeners and ensure to pause the
            // stream so that it can be consumed again by caller
            s.dispose_listeners();
            s.stream.pause();

            s.resolver.resolve(s.snapshot(false));
        }
    });

    // Error Listener
    let s = state.clone();
    let error_listener: ErrorListener = Rc::new(move |error: &Error| {
        s.dispose_listeners();

        s.resolver.reject(error.clone());
    });

    // End Listener
    let s = state.clone();
    let end_listener: EndListener = Rc::new(move || {
        s.dispose_listeners();

        s.resolver.resolve(s.snapshot(true));
    });

    *state.listeners.borrow_mut() = Some(PeekListeners {
        data: Rc::downgrade(&data_listener),
        error: Rc::downgrade(&error_listener),
        end: Rc::downgrade(&end_listener),
    });

    state.stream.on(StreamListener::Error(error_listener));
    state.stream.on(StreamListener::End(end_listener));

    // Important: leave the `data` listener last because
    // this can turn the stream into flowing mode and we
    // want `error` events to be received as well.
    state.stream.on(StreamListener::Data(data_listener));

    promise
}

/// Helper to create a readable stream from an existing T.
pub fn to_stream<T: Clone + 'static>(t: T, reducer: Reducer<T>) -> Rc<WriteableStreamImpl<T>> {
    let stream = new_writeable_stream(Some(reducer), None);

    stream.end(Some(t));

    stream
}

/// Helper to create an empty stream. O `never` do original não tem valor, então o tipo do dado é a
/// unidade e o reducer lança como o original.
pub fn empty_stream() -> Rc<WriteableStreamImpl<()>> {
    let stream = new_writeable_stream::<()>(Some(Rc::new(|_| errors::throw(Error::new("not supported")))), None);
    stream.end(None);

    stream
}

/// Helper to convert a T into a Readable<T>.
pub fn to_readable<T: 'static>(t: T) -> impl Readable<T> {
    let mut value = Some(t);

    move || value.take()
}

/// Helper to transform a readable stream into another stream.
pub fn transform<Original: 'static, Transformed: Clone + 'static>(
    stream: &dyn ReadableStreamEvents<Original>,
    transformer: Transformer<Original, Transformed>,
    reducer: Reducer<Transformed>,
) -> Rc<WriteableStreamImpl<Transformed>> {
    let target = new_writeable_stream(Some(reducer), None);

    let (on_data_target, on_error_target, on_end_target) = (target.clone(), target.clone(), target.clone());
    listen_stream(
        stream,
        IStreamListener {
            on_data: Rc::new(move |data: &Original| {
                on_data_target.write((transformer.data)(data));
            }),
            on_error: Rc::new(move |error: &Error| {
                on_error_target.error(match &transformer.error {
                    Some(transform_error) => transform_error(error.clone()),
                    None => error.clone(),
                });
            }),
            on_end: Rc::new(move || on_end_target.end(None)),
        },
        None,
    );

    target
}

/// Helper to take an existing readable that will have a prefix injected to the beginning.
pub fn prefixed_readable<T: Clone + 'static>(prefix: T, mut readable: Box<dyn Readable<T>>, reducer: impl Fn(Vec<T>) -> T + 'static) -> impl Readable<T> {
    let mut prefix_handled = false;

    move || {
        let chunk = readable.read();

        // Handle prefix only once
        if !prefix_handled {
            prefix_handled = true;

            // If we have also a read-result, make
            // sure to reduce it to a single result
            if let Some(chunk) = chunk {
                return Some(reducer(vec![prefix.clone(), chunk]));
            }

            // Otherwise, just return prefix directly
            return Some(prefix.clone());
        }

        chunk
    }
}

/// Helper to take an existing stream that will have a prefix injected to the beginning.
pub fn prefixed_stream<T: Clone + 'static>(prefix: T, stream: &dyn ReadableStreamEvents<T>, reducer: Reducer<T>) -> Rc<WriteableStreamImpl<T>> {
    let prefix_handled = Rc::new(std::cell::Cell::new(false));

    let target = new_writeable_stream(Some(reducer.clone()), None);

    let (data_target, error_target, end_target) = (target.clone(), target.clone(), target.clone());
    let (data_handled, end_handled) = (prefix_handled.clone(), prefix_handled);
    let data_prefix = prefix.clone();
    listen_stream(
        stream,
        IStreamListener {
            on_data: Rc::new(move |data: &T| {
                // Handle prefix only once
                if !data_handled.replace(true) {
                    data_target.write(reducer(vec![data_prefix.clone(), data.clone()]));
                    return;
                }

                data_target.write(data.clone());
            }),
            on_error: Rc::new(move |error: &Error| error_target.error(error.clone())),
            on_end: Rc::new(move || {
                // Handle prefix only once
                if !end_handled.replace(true) {
                    end_target.write(prefix.clone());
                }

                end_target.end(None);
            }),
        },
        None,
    );

    target
}
