//! Porte de `src/vs/base/test/common/stream.test.ts` (suíte `Stream`), caso a caso e na mesma ordem.
//!
//! Os testes `async` rodam em `common::run_with_faked_timers`. Os `string[]` do original são `Vec<String>`
//! e um `reducer` é um `Rc<dyn Fn(Vec<T>) -> T>`.
//!
//! Pendente: `is_readable`, `is_readable_stream` e `is_readable_buffered_stream` conferem a forma de um
//! valor `unknown` (`undefined`, `Object.create(null)`), um duck typing que em Rust o tipo já decide (ver
//! o topo de `stream.rs`).

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{run_with_faked_timers, timeout};
use je_base::cancellation::CancellationTokenSource;
use je_base::errors::Error;
use je_base::stream::{
    IStreamListener, PeekedReadable, Readable, ReadableStream, ReadableStreamEvents, Reducer, StreamListener, Transformer, WriteableStream,
    WriteableStreamImpl, WriteableStreamOptions, consume_readable, consume_stream, consume_stream_ignoring_data, listen_stream,
    new_writeable_stream, peek_readable, peek_stream, prefixed_readable, prefixed_stream, to_readable, to_stream, transform,
};
use je_base::runtime;

fn join_reducer() -> Reducer<String> {
    Rc::new(|strings| strings.join(","))
}

fn strings_stream(options: Option<WriteableStreamOptions>) -> Rc<WriteableStreamImpl<String>> {
    new_writeable_stream(Some(join_reducer()), options)
}

fn s(text: &str) -> String {
    text.to_string()
}

#[test]
fn writeable_stream_basics() {
    let stream = strings_stream(None);

    let error = Rc::new(Cell::new(false));
    let e = error.clone();
    stream.on(StreamListener::error(move |_| e.set(true)));

    let end = Rc::new(Cell::new(false));
    let e = end.clone();
    stream.on(StreamListener::end(move || e.set(true)));

    stream.write(s("Hello"));

    let chunks: Rc<RefCell<Vec<String>>> = Rc::default();
    let c = chunks.clone();
    stream.on(StreamListener::data(move |data: &String| c.borrow_mut().push(data.clone())));

    assert_eq!(chunks.borrow()[0], "Hello");

    stream.write(s("World"));
    assert_eq!(chunks.borrow()[1], "World");

    assert!(!error.get());
    assert!(!end.get());

    stream.pause();
    stream.write(s("1"));
    stream.write(s("2"));
    stream.write(s("3"));

    assert_eq!(chunks.borrow().len(), 2);

    stream.resume();

    assert_eq!(chunks.borrow().len(), 3);
    assert_eq!(chunks.borrow()[2], "1,2,3");

    stream.error(Error::new(""));
    assert!(error.get());

    error.set(false);
    stream.error(Error::new(""));
    assert!(error.get());

    stream.end(Some(s("Final Bit")));
    assert_eq!(chunks.borrow().len(), 4);
    assert_eq!(chunks.borrow()[3], "Final Bit");
    assert!(end.get());

    stream.destroy();

    stream.write(s("Unexpected"));
    assert_eq!(chunks.borrow().len(), 4);
}

#[test]
fn stream_with_non_reducible_messages() {
    /// A complex object that cannot be reduced to a single object.
    #[derive(Clone)]
    struct TestMessage {
        value: String,
    }
    let message = |value: &str| TestMessage { value: s(value) };

    let stream = new_writeable_stream::<TestMessage>(None, None);

    let error = Rc::new(Cell::new(false));
    let e = error.clone();
    stream.on(StreamListener::error(move |_| e.set(true)));

    let end = Rc::new(Cell::new(false));
    let e = end.clone();
    stream.on(StreamListener::end(move || e.set(true)));

    stream.write(message("Hello"));

    let chunks: Rc<RefCell<Vec<TestMessage>>> = Rc::default();
    let c = chunks.clone();
    stream.on(StreamListener::data(move |data: &TestMessage| c.borrow_mut().push(data.clone())));

    assert_eq!(chunks.borrow()[0].value, "Hello");

    stream.write(message("World"));

    assert_eq!(chunks.borrow()[1].value, "World");

    assert!(!error.get());
    assert!(!end.get());

    stream.pause();
    stream.write(message("1"));
    stream.write(message("2"));
    stream.write(message("3"));

    assert_eq!(chunks.borrow().len(), 2);

    stream.resume();

    assert_eq!(chunks.borrow().len(), 5);

    assert_eq!(chunks.borrow()[2].value, "1");
    assert_eq!(chunks.borrow()[3].value, "2");
    assert_eq!(chunks.borrow()[4].value, "3");

    stream.error(Error::new(""));
    assert!(error.get());

    error.set(false);
    stream.error(Error::new(""));
    assert!(error.get());

    stream.end(Some(message("Final Bit")));
    assert_eq!(chunks.borrow().len(), 6);

    assert_eq!(chunks.borrow()[5].value, "Final Bit");

    assert!(end.get());

    stream.destroy();

    stream.write(message("Unexpected"));
    assert_eq!(chunks.borrow().len(), 6);
}

#[test]
fn writeable_stream_end_with_empty_string_works() {
    run_with_faked_timers(async {
        let reducer: Reducer<String> = Rc::new(|strings| if strings.is_empty() { s("error") } else { strings.join(",") });
        let stream = new_writeable_stream(Some(reducer.clone()), None);
        stream.end(Some(s("")));

        let result = consume_stream(&*stream, reducer).await.unwrap();
        assert_eq!(result, "");
    });
}

#[test]
fn writeable_stream_end_with_error_works() {
    run_with_faked_timers(async {
        let reducer: Reducer<Error> = Rc::new(|errors| errors[0].clone());
        let stream = new_writeable_stream(Some(reducer.clone()), None);
        stream.end(Some(Error::new("error")));

        let result = consume_stream(&*stream, reducer).await.unwrap();
        assert_eq!(result.to_string(), "Error: error");
    });
}

#[test]
fn writeable_stream_remove_listener() {
    let stream = strings_stream(None);

    let error = Rc::new(Cell::new(false));
    let e = error.clone();
    let error_listener = StreamListener::error(move |_| e.set(true));
    stream.on(error_listener.clone());

    let data = Rc::new(Cell::new(false));
    let d = data.clone();
    let data_listener = StreamListener::data(move |_: &String| d.set(true));
    stream.on(data_listener.clone());

    stream.write(s("Hello"));
    assert!(data.get());

    data.set(false);
    stream.remove_listener(&data_listener);

    stream.write(s("World"));
    assert!(!data.get());

    stream.error(Error::new(""));
    assert!(error.get());

    error.set(false);
    stream.remove_listener(&error_listener);

    // always leave at least one error listener to streams to avoid unexpected errors during test running
    stream.on(StreamListener::error(|_| {}));
    stream.error(Error::new(""));
    assert!(!error.get());
}

#[test]
fn writeable_stream_high_water_mark() {
    run_with_faked_timers(async {
        let stream = strings_stream(Some(WriteableStreamOptions { high_water_mark: Some(3) }));

        let mut res = stream.write(s("1"));
        assert!(res.is_none());

        res = stream.write(s("2"));
        assert!(res.is_none());

        res = stream.write(s("3"));
        assert!(res.is_none());

        let promise1 = stream.write(s("4"));
        assert!(promise1.is_some());

        let promise2 = stream.write(s("5"));
        assert!(promise2.is_some());

        let drained1 = Rc::new(Cell::new(false));
        let d = drained1.clone();
        runtime::spawn_local(async move {
            promise1.unwrap().await.unwrap();
            d.set(true);
        });

        let drained2 = Rc::new(Cell::new(false));
        let d = drained2.clone();
        runtime::spawn_local(async move {
            promise2.unwrap().await.unwrap();
            d.set(true);
        });

        let data: Rc<RefCell<Option<String>>> = Rc::default();
        let d = data.clone();
        stream.on(StreamListener::data(move |chunk: &String| *d.borrow_mut() = Some(chunk.clone())));
        assert!(data.borrow().as_deref().is_some_and(|d| !d.is_empty()));

        timeout(0).await.unwrap();
        assert!(drained1.get());
        assert!(drained2.get());
    });
}

fn array_to_readable(array: &[&str]) -> Box<dyn Readable<String>> {
    // `array.shift() || null`: a string vazia também termina a leitura.
    let mut items: std::collections::VecDeque<String> = array.iter().map(ToString::to_string).collect();
    Box::new(move || items.pop_front().filter(|item| !item.is_empty()))
}

fn join(strings: Vec<String>) -> String {
    strings.join(",")
}

#[test]
fn consume_readable_case() {
    let mut readable = array_to_readable(&["1", "2", "3", "4", "5"]);
    let consumed = consume_readable(&mut *readable, join);
    assert_eq!(consumed, "1,2,3,4,5");
}

#[test]
fn peek_readable_case() {
    for i in 0..5 {
        let readable = array_to_readable(&["1", "2", "3", "4", "5"]);

        match peek_readable(readable, join, i) {
            PeekedReadable::Value(_) => panic!("Unexpected result"),
            PeekedReadable::Readable(mut consumed_or_readable) => {
                let consumed = consume_readable(&mut *consumed_or_readable, join);
                assert_eq!(consumed, "1,2,3,4,5");
            }
        }
    }

    let readable = array_to_readable(&["1", "2", "3", "4", "5"]);
    let PeekedReadable::Value(consumed) = peek_readable(readable, join, 5) else { panic!("esperava o valor") };
    assert_eq!(consumed, "1,2,3,4,5");

    let readable = array_to_readable(&["1", "2", "3", "4", "5"]);
    let PeekedReadable::Value(consumed) = peek_readable(readable, join, 6) else { panic!("esperava o valor") };
    assert_eq!(consumed, "1,2,3,4,5");
}

#[test]
fn peek_readable_error_handling() {
    run_with_faked_timers(async {
        let identity: Reducer<String> = Rc::new(|mut data| data.remove(0));

        // 0 Chunks
        let mut stream = new_writeable_stream(Some(identity.clone()), None);

        let error: Rc<RefCell<Option<Error>>> = Rc::default();
        let (e, st) = (error.clone(), stream.clone());
        let mut promise = {
            let (promise, resolver) = je_base::promise::Promise::<()>::new();
            runtime::spawn_local(async move {
                if let Err(err) = peek_stream(st, 1).await {
                    *e.borrow_mut() = Some(err);
                }
                resolver.resolve(());
            });
            promise
        };

        stream.error(Error::new(""));
        promise.await.unwrap();

        assert!(error.borrow().is_some());

        // 1 Chunk
        stream = new_writeable_stream(Some(identity.clone()), None);

        *error.borrow_mut() = None;
        let (e, st) = (error.clone(), stream.clone());
        promise = {
            let (promise, resolver) = je_base::promise::Promise::<()>::new();
            runtime::spawn_local(async move {
                if let Err(err) = peek_stream(st, 1).await {
                    *e.borrow_mut() = Some(err);
                }
                resolver.resolve(());
            });
            promise
        };

        stream.write(s("foo"));
        stream.error(Error::new(""));
        promise.await.unwrap();

        assert!(error.borrow().is_some());

        // 2 Chunks
        stream = new_writeable_stream(Some(identity), None);

        *error.borrow_mut() = None;
        let (e, st) = (error.clone(), stream.clone());
        promise = {
            let (promise, resolver) = je_base::promise::Promise::<()>::new();
            runtime::spawn_local(async move {
                if let Err(err) = peek_stream(st, 1).await {
                    *e.borrow_mut() = Some(err);
                }
                resolver.resolve(());
            });
            promise
        };

        stream.write(s("foo"));
        stream.write(s("bar"));
        stream.error(Error::new(""));
        promise.await.unwrap();

        assert!(error.borrow().is_none());

        let e = error.clone();
        stream.on(StreamListener::error(move |err: &Error| *e.borrow_mut() = Some(err.clone())));
        stream.on(StreamListener::data(|_: &String| {}));
        assert!(error.borrow().is_some());
    });
}

fn readable_to_stream(mut readable: Box<dyn Readable<String>>) -> Rc<WriteableStreamImpl<String>> {
    let stream = strings_stream(None);

    // Simulate async behavior
    let target = stream.clone();
    runtime::set_timeout(
        move || {
            while let Some(chunk) = readable.read() {
                target.write(chunk);
            }

            target.end(None);
        },
        0,
    );

    stream
}

#[test]
fn consume_stream_case() {
    run_with_faked_timers(async {
        let stream = readable_to_stream(array_to_readable(&["1", "2", "3", "4", "5"]));
        let consumed = consume_stream(&*stream, join_reducer()).await.unwrap();
        assert_eq!(consumed, "1,2,3,4,5");
    });
}

#[test]
fn consume_stream_without_reducer() {
    run_with_faked_timers(async {
        let stream = readable_to_stream(array_to_readable(&["1", "2", "3", "4", "5"]));
        consume_stream_ignoring_data(&*stream).await.unwrap();
    });
}

#[test]
fn consume_stream_without_reducer_and_error() {
    run_with_faked_timers(async {
        let stream = strings_stream(None);
        stream.error(Error::new(""));

        consume_stream_ignoring_data(&*stream).await.unwrap();
    });
}

/// Os três campos que `listenStream` preenche nos testes: o último dado, e os sinais de erro e de fim.
#[derive(Default)]
struct Seen {
    error: Cell<bool>,
    end: Cell<bool>,
    data: RefCell<String>,
}

fn seen_listener(seen: &Rc<Seen>) -> IStreamListener<String> {
    let (d, e, n) = (seen.clone(), seen.clone(), seen.clone());
    IStreamListener {
        on_data: Rc::new(move |data: &String| *d.data.borrow_mut() = data.clone()),
        on_error: Rc::new(move |_| e.error.set(true)),
        on_end: Rc::new(move || n.end.set(true)),
    }
}

#[test]
fn listen_stream_case() {
    let stream = strings_stream(None);
    let seen = Rc::new(Seen::default());

    listen_stream(&*stream, seen_listener(&seen), None);

    stream.write(s("Hello"));

    assert_eq!(*seen.data.borrow(), "Hello");

    stream.write(s("World"));
    assert_eq!(*seen.data.borrow(), "World");

    assert!(!seen.error.get());
    assert!(!seen.end.get());

    stream.error(Error::new(""));
    assert!(seen.error.get());

    stream.end(Some(s("Final Bit")));
    assert!(seen.end.get());
}

#[test]
fn listen_stream_cancellation() {
    let stream = strings_stream(None);
    let seen = Rc::new(Seen::default());

    let cts = CancellationTokenSource::new(None);

    listen_stream(&*stream, seen_listener(&seen), Some(&cts.token()));

    cts.cancel();

    stream.write(s("Hello"));
    assert_eq!(*seen.data.borrow(), "");

    stream.write(s("World"));
    assert_eq!(*seen.data.borrow(), "");

    stream.error(Error::new(""));
    assert!(!seen.error.get());

    stream.end(Some(s("Final Bit")));
    assert!(!seen.end.get());
}

fn same_stream(a: &Rc<dyn ReadableStream<String>>, b: &Rc<WriteableStreamImpl<String>>) -> bool {
    std::ptr::eq(Rc::as_ptr(a).cast::<()>(), Rc::as_ptr(b).cast::<()>())
}

#[test]
fn peek_stream_case() {
    run_with_faked_timers(async {
        for i in 0..5 {
            let stream = readable_to_stream(array_to_readable(&["1", "2", "3", "4", "5"]));

            let result = peek_stream(stream.clone(), i).await.unwrap();
            assert!(same_stream(&result.stream, &stream));
            if result.ended {
                panic!("Unexpected result, stream should not have ended yet");
            } else {
                assert_eq!(result.buffer.len(), i + 1, "maxChunks: {i}");

                let additional_result: Rc<RefCell<Vec<String>>> = Rc::default();
                let additional = additional_result.clone();
                consume_stream(
                    &*stream,
                    Rc::new(move |strings: Vec<String>| {
                        additional.borrow_mut().extend(strings.clone());

                        strings.join(",")
                    }),
                )
                .await
                .unwrap();

                let all: Vec<String> = result.buffer.iter().cloned().chain(additional_result.borrow().iter().cloned()).collect();
                assert_eq!(all.join(","), "1,2,3,4,5");
            }
        }

        let stream = readable_to_stream(array_to_readable(&["1", "2", "3", "4", "5"]));
        let result = peek_stream(stream.clone(), 5).await.unwrap();
        assert!(same_stream(&result.stream, &stream));
        assert_eq!(result.buffer.join(","), "1,2,3,4,5");
        assert!(result.ended);

        let stream = readable_to_stream(array_to_readable(&["1", "2", "3", "4", "5"]));
        let result = peek_stream(stream.clone(), 6).await.unwrap();
        assert!(same_stream(&result.stream, &stream));
        assert_eq!(result.buffer.join(","), "1,2,3,4,5");
        assert!(result.ended);
    });
}

#[test]
fn to_stream_case() {
    run_with_faked_timers(async {
        let stream = to_stream(s("1,2,3,4,5"), join_reducer());
        let consumed = consume_stream(&*stream, join_reducer()).await.unwrap();
        assert_eq!(consumed, "1,2,3,4,5");
    });
}

#[test]
fn to_readable_case() {
    let mut readable = to_readable(s("1,2,3,4,5"));
    let consumed = consume_readable(&mut readable, join);
    assert_eq!(consumed, "1,2,3,4,5");
}

#[test]
fn transform_case() {
    run_with_faked_timers(async {
        let source = strings_stream(None);

        let result = transform(&*source, Transformer { data: Rc::new(|string: &String| format!("{string}{string}")), error: None }, join_reducer());

        // Simulate async behavior
        let src = source.clone();
        runtime::set_timeout(
            move || {
                src.write(s("1"));
                src.write(s("2"));
                src.write(s("3"));
                src.write(s("4"));
                src.end(Some(s("5")));
            },
            0,
        );

        let consumed = consume_stream(&*result, join_reducer()).await.unwrap();
        assert_eq!(consumed, "11,22,33,44,55");
    });
}

#[test]
fn events_are_delivered_even_if_a_listener_is_removed_during_delivery() {
    let stream = strings_stream(None);

    let listener1_called = Rc::new(Cell::new(false));
    let listener2_called = Rc::new(Cell::new(false));

    // O `listener1` do original se remove de dentro dele mesmo; aqui ele guarda a si mesmo num `RefCell`.
    let listener1_slot: Rc<RefCell<Option<StreamListener<String>>>> = Rc::default();
    let (c, slot, st) = (listener1_called.clone(), listener1_slot.clone(), stream.clone());
    let listener1 = StreamListener::end(move || {
        if let Some(me) = slot.borrow().clone() {
            st.remove_listener(&me);
        }
        c.set(true);
    });
    *listener1_slot.borrow_mut() = Some(listener1.clone());
    let c = listener2_called.clone();
    let listener2 = StreamListener::end(move || c.set(true));
    stream.on(listener1);
    stream.on(listener2);
    stream.on(StreamListener::data(|_: &String| {}));
    stream.end(Some(s("")));

    assert!(listener1_called.get());
    assert!(listener2_called.get());
}

#[test]
fn prefixed_readable_case() {
    // Basic
    let mut readable = prefixed_readable(s("1,2"), array_to_readable(&["3", "4", "5"]), join);
    assert_eq!(consume_readable(&mut readable, join), "1,2,3,4,5");

    // Empty
    let mut readable = prefixed_readable(s("empty"), array_to_readable(&[]), join);
    assert_eq!(consume_readable(&mut readable, join), "empty");
}

#[test]
fn prefixed_stream_case() {
    run_with_faked_timers(async {
        // Basic
        let stream = strings_stream(None);
        stream.write(s("3"));
        stream.write(s("4"));
        stream.write(s("5"));
        stream.end(None);

        let prefix_stream = prefixed_stream(s("1,2"), &*stream, join_reducer());
        assert_eq!(consume_stream(&*prefix_stream, join_reducer()).await.unwrap(), "1,2,3,4,5");

        // Empty
        let stream = strings_stream(None);
        stream.end(None);

        let prefix_stream = prefixed_stream(s("1,2"), &*stream, join_reducer());
        assert_eq!(consume_stream(&*prefix_stream, join_reducer()).await.unwrap(), "1,2");

        // Error
        let stream = strings_stream(None);
        stream.error(Error::new("fail"));

        let prefix_stream = prefixed_stream(s("error"), &*stream, join_reducer());

        let error = consume_stream(&*prefix_stream, join_reducer()).await;
        assert!(error.is_err());
    });
}
