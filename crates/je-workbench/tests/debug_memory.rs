//! Porte de `src/vs/workbench/contrib/debug/test/browser/debugMemory.test.ts` (suíte `Debug - Memory`, sub-suíte
//! `MemoryRegion`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` de cada caso; o `setup` roda no começo do caso e o
//!   `teardown` (`region.dispose()`) no fim.
//! - O `mockObject<MockSession, 'onDidInvalidateMemory'>()({ onDidInvalidateMemory })` com o `callsFake` de `readMemory` e
//!   `writeMemory` é o `MockSession` com `set_memory_hooks` (as duas funções falsas e o evento).
//! - `VSBuffer.wrap(new Uint8Array([...]))` é `VSBuffer::wrap(vec)`; o `assert.deepStrictEqual` entre os `MemoryRange` compara
//!   `MemoryRange::summary` (tipo, deslocamento, tamanho e bytes), porque o `VSBuffer` não tem `PartialEq`.
//! - `await region.read(...)` roda dentro de uma tarefa local (`run_with_faked_timers`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::Cell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::buffer::{VSBuffer, decode_base64, encode_base64};
use je_base::event::Emitter;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_workbench::contrib::debug::debug_model::MemoryRegion;
use je_workbench::contrib::debug::debug_protocol::Response;
use je_workbench::contrib::debug::debug_types::IDebugSessionFull;
use serde_json::json;
use support::mock_debug::MockSession;

/// `dapResponseCommon`.
fn dap_response(body: serde_json::Value, command: &str) -> Response {
    Response { seq: 1, request_seq: 1, success: true, command: command.to_owned(), message: None, body: Some(body) }
}

/// O `setup`: devolve a memória, o contador de bytes ilegíveis do próximo `readMemory` e a região.
fn setup() -> (Rc<Cell<i64>>, Rc<MemoryRegion>) {
    let memory_buf: Vec<u8> = (0..1024usize).map(|i| i as u8).collect(); // will be 0-255
    let memory = VSBuffer::wrap(memory_buf);
    let invalidate_memory_emitter = Emitter::new();
    let unreadable = Rc::new(Cell::new(0));

    let session = MockSession::new();
    let read_memory_source = memory.clone();
    let read_unreadable = unreadable.clone();
    let write_memory_target = memory;
    session.set_memory_hooks(
        move |_reference, from_offset, count| {
            let res = dap_response(
                json!({
                    "address": "0",
                    "data": encode_base64(&read_memory_source.slice(Some(from_offset), Some(from_offset + (count - read_unreadable.get()).max(0))), true, false),
                    "unreadableBytes": read_unreadable.get(),
                }),
                "someCommand",
            );

            read_unreadable.set(0);

            Promise::resolved(Some(res))
        },
        move |_reference, from_offset, data| {
            let decoded = decode_base64(data);
            write_memory_target.set(&decoded, from_offset as usize);

            Promise::resolved(Some(dap_response(json!({ "bytesWritten": decoded.byte_length(), "offset": from_offset }), "someCommand")))
        },
        invalidate_memory_emitter.event(),
    );

    let session: Rc<dyn IDebugSessionFull> = session;
    let region = MemoryRegion::new("ref", session);
    let _ = invalidate_memory_emitter;
    (unreadable, region)
}

#[test]
fn reads_a_simple_range() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (_unreadable, region) = setup();
    run_with_faked_timers(async move {
        let ranges = region.read(10, 14).await.unwrap();
        let summary: Vec<_> = ranges.iter().map(|range| range.summary()).collect();
        assert_eq!(summary, vec![("valid", 10, 4, vec![10, 11, 12, 13])]);
        region.dispose();
        drop(guard);
    });
}

#[test]
fn reads_a_non_contiguous_range() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (unreadable, region) = setup();
    run_with_faked_timers(async move {
        unreadable.set(3);
        let ranges = region.read(10, 14).await.unwrap();
        let summary: Vec<_> = ranges.iter().map(|range| range.summary()).collect();
        assert_eq!(summary, vec![("valid", 10, 1, vec![10]), ("unreadable", 11, 3, vec![])]);
        region.dispose();
        drop(guard);
    });
}
