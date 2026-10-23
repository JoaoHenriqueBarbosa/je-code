//! Porte de `src/vs/base/test/common/sseParser.test.ts` (suíte `SSEParser`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `receivedEvents` é um `Rc<RefCell<Vec<_>>>` compartilhado com o fecho do analisador, e o `setup` do
//!   mocha é a função `setup`, chamada no começo de cada teste.
//! - `toUint8Array` é `str::as_bytes`; `receivedEvents[0].id === undefined` é `id == None`.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::sse_parser::{ISSEEvent, SSEParser};

type Events = Rc<RefCell<Vec<ISSEEvent>>>;

fn setup() -> (Events, SSEParser) {
    let received_events: Events = Rc::default();
    let sink = received_events.clone();
    let parser = SSEParser::new(move |event| sink.borrow_mut().push(event));
    (received_events, parser)
}

fn event(r#type: &str, data: &str) -> ISSEEvent {
    ISSEEvent { r#type: r#type.to_owned(), data: data.to_owned(), id: None, retry: None }
}

#[test]
fn handles_basic_events() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (received_events, mut parser) = setup();

    parser.feed(b"data: hello world\n\n");

    let events = received_events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].r#type, "message");
    assert_eq!(events[0].data, "hello world");
}

#[test]
fn handles_events_with_multiple_data_fields() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (received_events, mut parser) = setup();

    parser.feed(b"data: first line\ndata: second line\n\n");

    let events = received_events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "first line\nsecond line");
}

#[test]
fn handles_events_with_explicit_event_type() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (received_events, mut parser) = setup();

    parser.feed(b"event: custom\ndata: hello world\n\n");

    let events = received_events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].r#type, "custom");
    assert_eq!(events[0].data, "hello world");
}

#[test]
fn handles_events_with_explicit_event_type_crlf() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (received_events, mut parser) = setup();

    parser.feed(b"event: custom\r\ndata: hello world\r\n\r\n");

    let events = received_events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].r#type, "custom");
    assert_eq!(events[0].data, "hello world");
}

#[test]
fn stream_processing_chunks() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (received_events, mut parser) = setup();

    for lf in ["\n", "\r\n", "\r"] {
        let message = format!("event: custom{lf}data: hello world{lf}{lf}event: custom2{lf}data: hello world2{lf}{lf}").into_bytes();
        for chunk_size in 1..5 {
            received_events.borrow_mut().clear();

            for chunk in message.chunks(chunk_size) {
                parser.feed(chunk);
            }

            assert_eq!(
                *received_events.borrow(),
                vec![event("custom", "hello world"), event("custom2", "hello world2")],
                "Failed for chunk size {chunk_size} and line ending {lf:?}"
            );
        }
    }
}

#[test]
fn handles_events_with_id() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (received_events, mut parser) = setup();

    parser.feed(b"event: custom\ndata: hello\nid: 123\n\n");

    let events = received_events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].r#type, "custom");
    assert_eq!(events[0].data, "hello");
    assert_eq!(events[0].id.as_deref(), Some("123"));
    assert_eq!(parser.get_last_event_id(), Some("123"));
}

#[test]
fn ignores_comments() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (received_events, mut parser) = setup();

    parser.feed(b"event: custom\n:this is a comment\ndata: hello\n\n");

    let events = received_events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "hello");
}

#[test]
fn handles_retry_field() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (received_events, mut parser) = setup();

    parser.feed(b"retry: 5000\ndata: hello\n\n");

    let events = received_events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "hello");
    assert_eq!(events[0].retry, Some(5000.0));
    assert_eq!(parser.get_reconnection_time(), Some(5000.0));
}

#[test]
fn handles_invalid_retry_field() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (received_events, mut parser) = setup();

    parser.feed(b"retry: invalid\ndata: hello\n\n");

    let events = received_events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "hello");
    assert_eq!(events[0].retry, None);
    assert_eq!(parser.get_reconnection_time(), None);
}

#[test]
fn ignores_fields_with_null_character_in_id() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (received_events, mut parser) = setup();

    parser.feed(b"id: 12\0 3\ndata: hello\n\n");

    let events = received_events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].id, None);
    assert_eq!(parser.get_last_event_id(), None);
}

#[test]
fn handles_fields_with_no_value() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (received_events, mut parser) = setup();

    parser.feed(b"data\nid\n\n");

    let events = received_events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "");
    assert_eq!(events[0].id.as_deref(), Some(""));
}

#[test]
fn handles_fields_with_space_after_colon() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (received_events, mut parser) = setup();

    parser.feed(b"data: hello\nevent: custom\nid: 123\n\n");

    let events = received_events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "hello");
    assert_eq!(events[0].r#type, "custom");
    assert_eq!(events[0].id.as_deref(), Some("123"));
}

#[test]
fn handles_different_line_endings_lf() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (received_events, mut parser) = setup();

    parser.feed(b"data: hello\n\n");

    let events = received_events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "hello");
}

#[test]
fn handles_different_line_endings_cr() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (received_events, mut parser) = setup();

    parser.feed(b"data: hello\r\r");

    let events = received_events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "hello");
}

#[test]
fn handles_different_line_endings_crlf() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (received_events, mut parser) = setup();

    parser.feed(b"data: hello\r\n\r\n");

    let events = received_events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "hello");
}

#[test]
fn handles_empty_data_with_blank_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (received_events, mut parser) = setup();

    parser.feed(b"data:\n\n");

    let events = received_events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "");
}

#[test]
fn ignores_events_with_no_data_after_blank_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (received_events, mut parser) = setup();

    parser.feed(b"event: custom\n\n");

    assert_eq!(received_events.borrow().len(), 0);
}

#[test]
fn supports_chunked_data() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (received_events, mut parser) = setup();

    parser.feed(b"event: cus");
    parser.feed(b"tom\nda");
    parser.feed(b"ta: hello\n");
    parser.feed(b"\n");

    let events = received_events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].r#type, "custom");
    assert_eq!(events[0].data, "hello");
}

#[test]
fn supports_spec_example() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (received_events, mut parser) = setup();

    // Example from the spec
    parser.feed(b":This is a comment\ndata: first event\nid: 1\n\n");
    parser.feed(b"data:second event\nid\n\n");
    parser.feed(b"data:  third event\n\n");

    let events = received_events.borrow();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].data, "first event");
    assert_eq!(events[0].id.as_deref(), Some("1"));
    assert_eq!(events[1].data, "second event");
    assert_eq!(events[1].id.as_deref(), Some(""));
    assert_eq!(events[2].data, " third event");
}

#[test]
fn resets_correctly() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (received_events, mut parser) = setup();

    parser.feed(b"data: hello\n");
    parser.reset();
    parser.feed(b"data: world\n\n");

    let events = received_events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "world");
}
