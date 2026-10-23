//! Porte de `src/vs/editor/test/browser/gpu/objectCollectionBuffer.test.ts` (suíte `ObjectCollectionBuffer`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `[{ name: 'a' }, { name: 'b' }] as const` vira `specs(&["a", "b"])`.
//! - Os objetos `{ a: 1, b: 2 }` viram `&[("a", 1.0), ("b", 2.0)]`.
//! - `Array.from(buffer.view.subarray(0, buffer.viewUsedSize))` vira `view[..view_used_size]`.
//! - `buffer.buffer.byteLength` vira `buffer.buffer_byte_length()` (ver o cabeçalho do módulo).
//! - O `changeCount++` do ouvinte vira um `Rc<Cell<i32>>`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::IDisposable;
use je_editor::gpu::object_collection_buffer::{
    ObjectCollectionBuffer, ObjectCollectionBufferPropertySpec, create_object_collection_buffer,
};

fn specs(names: &[&str]) -> Vec<ObjectCollectionBufferPropertySpec> {
    names.iter().map(|name| ObjectCollectionBufferPropertySpec::new(*name)).collect()
}

fn assert_used_data(buffer: &ObjectCollectionBuffer, expected: &[f32]) {
    assert_eq!(&buffer.view()[..buffer.view_used_size()], expected);
}

#[test]
fn create_entry() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let buffer = store.add_rc(create_object_collection_buffer(specs(&["a", "b"]), 5));
    assert_used_data(&buffer, &[]);

    store.add_rc(buffer.create_entry(&[("a", 1.0), ("b", 2.0)]));
    store.add_rc(buffer.create_entry(&[("a", 3.0), ("b", 4.0)]));
    store.add_rc(buffer.create_entry(&[("a", 5.0), ("b", 6.0)]));
    store.add_rc(buffer.create_entry(&[("a", 7.0), ("b", 8.0)]));
    store.add_rc(buffer.create_entry(&[("a", 9.0), ("b", 10.0)]));
    assert_used_data(&buffer, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0]);
}

#[test]
fn create_entry_beyond_capacity() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let buffer = store.add_rc(create_object_collection_buffer(specs(&["a", "b"]), 1));
    store.add_rc(buffer.create_entry(&[("a", 1.0), ("b", 2.0)]));
    assert_eq!(buffer.entry_count(), 1);
    assert_eq!(buffer.buffer_byte_length(), 8);
    buffer.create_entry(&[("a", 3.0), ("b", 4.0)]);
    assert_eq!(buffer.entry_count(), 2);
    assert_eq!(buffer.buffer_byte_length(), 16);
}

#[test]
fn dispose_entry() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let buffer = store.add_rc(create_object_collection_buffer(specs(&["a", "b"]), 5));
    store.add_rc(buffer.create_entry(&[("a", 1.0), ("b", 2.0)]));
    let entry1 = buffer.create_entry(&[("a", 3.0), ("b", 4.0)]);
    store.add_rc(buffer.create_entry(&[("a", 5.0), ("b", 6.0)]));
    let entry2 = buffer.create_entry(&[("a", 7.0), ("b", 8.0)]);
    store.add_rc(buffer.create_entry(&[("a", 9.0), ("b", 10.0)]));
    entry1.dispose();
    entry2.dispose();
    // Data from disposed entries is stale and doesn't need to be validated
    assert_used_data(&buffer, &[1.0, 2.0, 5.0, 6.0, 9.0, 10.0]);
}

#[test]
fn entry_count_view_used_size_buffer_used_size() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let buffer = store.add_rc(create_object_collection_buffer(specs(&["foo", "bar"]), 5));
    assert_eq!(buffer.entry_count(), 0);
    assert_eq!(buffer.buffer_used_size(), 0);
    assert_eq!(buffer.view_used_size(), 0);
    buffer.create_entry(&[("foo", 1.0), ("bar", 2.0)]);
    assert_eq!(buffer.entry_count(), 1);
    assert_eq!(buffer.view_used_size(), 2);
    assert_eq!(buffer.buffer_used_size(), 8);
    let entry = buffer.create_entry(&[("foo", 3.0), ("bar", 4.0)]);
    assert_eq!(buffer.entry_count(), 2);
    assert_eq!(buffer.view_used_size(), 4);
    assert_eq!(buffer.buffer_used_size(), 16);
    entry.dispose();
    assert_eq!(buffer.entry_count(), 1);
    assert_eq!(buffer.view_used_size(), 2);
    assert_eq!(buffer.buffer_used_size(), 8);
}

#[test]
fn entry_get() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let buffer = store.add_rc(create_object_collection_buffer(specs(&["foo", "bar"]), 5));
    let entry = store.add_rc(buffer.create_entry(&[("foo", 1.0), ("bar", 2.0)]));
    assert_eq!(entry.get("foo"), 1.0);
    assert_eq!(entry.get("bar"), 2.0);
}

#[test]
fn entry_set() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let buffer = store.add_rc(create_object_collection_buffer(specs(&["foo", "bar"]), 5));
    let entry = store.add_rc(buffer.create_entry(&[("foo", 1.0), ("bar", 2.0)]));
    let change_count = Rc::new(Cell::new(0));
    store.add(buffer.on_did_change().listen({
        let change_count = change_count.clone();
        move |_| change_count.set(change_count.get() + 1)
    }));
    entry.set("foo", 3.0);
    assert_eq!(change_count.get(), 1);
    assert_eq!(entry.get("foo"), 3.0);
    entry.set("bar", 4.0);
    assert_eq!(change_count.get(), 2);
    assert_eq!(entry.get("bar"), 4.0);
}
