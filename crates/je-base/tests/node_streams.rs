//! Porte de `src/vs/base/test/node/nodeStreams.test.ts` (suíte `StreamSplitter`), caso a caso e na mesma
//! ordem.
//!
//! O `Writable` do Node do original é um fecho que consome um chunk (ver `node_streams.rs`), e o `done()`
//! do mocha é uma conferência de que o callback do `end` rodou (o porte é síncrono).

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::node::node_streams::StreamSplitter;

/// Liga um destino que guarda cada chunk como texto (`chunks.push(chunk.toString())`).
fn pipe_into_chunks(splitter: &StreamSplitter) -> Rc<RefCell<Vec<String>>> {
    let chunks: Rc<RefCell<Vec<String>>> = Rc::default();
    let sink = chunks.clone();
    splitter.pipe(move |chunk| sink.borrow_mut().push(String::from_utf8_lossy(chunk).into_owned()));
    chunks
}

#[test]
fn should_split_a_stream_on_a_single_character_splitter() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let splitter = StreamSplitter::new("\n");
    let chunks = pipe_into_chunks(&splitter);

    splitter.write("hello\nwor");
    splitter.write("ld\n");
    splitter.write("foo\nbar\nz");
    let done = Cell::new(false);
    splitter.end(|| {
        assert_eq!(*chunks.borrow(), ["hello\n", "world\n", "foo\n", "bar\n", "z"]);
        done.set(true);
    });
    assert!(done.get());
}

#[test]
fn should_split_a_stream_on_a_multi_character_splitter() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let splitter = StreamSplitter::new("---");
    let chunks = pipe_into_chunks(&splitter);

    splitter.write("hello---wor");
    splitter.write("ld---");
    splitter.write("foo---bar---z");
    let done = Cell::new(false);
    splitter.end(|| {
        assert_eq!(*chunks.borrow(), ["hello---", "world---", "foo---", "bar---", "z"]);
        done.set(true);
    });
    assert!(done.get());
}
