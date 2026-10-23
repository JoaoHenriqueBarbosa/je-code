//! Porte de `src/vs/editor/test/browser/gpu/bufferDirtyTracker.test.ts` (suíte `BufferDirtyTracker`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `setup` (`bdt = new BufferDirtyTracker()`) vira `new_tracker()`, chamado no começo de cada caso.
//! - `flag(index)` e `flag(index, length)` são `flag` e `flag_range` (ver o cabeçalho do módulo).
//! - `number | undefined` é `Option<usize>`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::gpu::buffer_dirty_tracker::{BufferDirtyTracker, IBufferDirtyTrackerReader};

fn new_tracker() -> BufferDirtyTracker {
    BufferDirtyTracker::new()
}

fn assert_state(bdt: &BufferDirtyTracker, data_offset: Option<usize>, dirty_size: Option<usize>) {
    assert_eq!(bdt.data_offset(), data_offset);
    assert_eq!(bdt.dirty_size(), dirty_size);
    assert_eq!(bdt.is_dirty(), data_offset.is_some());
}

#[test]
fn flag_index() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let bdt = new_tracker();
    assert_eq!(bdt.flag(0), 0);
    assert_state(&bdt, Some(0), Some(1));
    assert_eq!(bdt.flag(31), 31);
    assert_state(&bdt, Some(0), Some(32));
    bdt.clear();
    assert_state(&bdt, None, None);
    assert_eq!(bdt.flag(10), 10);
    assert_state(&bdt, Some(10), Some(1));
    assert_eq!(bdt.flag(15), 15);
    assert_state(&bdt, Some(10), Some(6));
}

#[test]
fn flag_index_length() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let bdt = new_tracker();
    bdt.flag_range(0, 32);
    assert_state(&bdt, Some(0), Some(32));
    bdt.clear();
    assert_state(&bdt, None, None);
    bdt.flag_range(10, 6);
    assert_state(&bdt, Some(10), Some(6));
}
