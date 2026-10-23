//! Porte de `src/vs/base/test/common/performance.test.ts` (suíte `clearMarks`), caso a caso e na mesma ordem.
//!
//! Notas de tradução: o `setup` (prefixo único por contador) é `setup()` e o `teardown` (`clearMarks()`) é o
//! `Drop` de [`Teardown`]. O singleton de marcas é por thread, e cada teste do Rust roda no seu thread, o que
//! já isola os testes; o contador do original continua para manter os nomes.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::performance::{clear_marks, get_marks, mark};

static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn unique_prefix() -> String {
    format!("test/perf/{}/", TEST_COUNTER.fetch_add(1, Ordering::SeqCst))
}

struct Teardown;

impl Drop for Teardown {
    fn drop(&mut self) {
        clear_marks(None);
    }
}

#[test]
fn clears_a_specific_mark_by_exact_name() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let prefix = unique_prefix();
    let _teardown = Teardown;
    let name_a = format!("{prefix}a");
    let name_b = format!("{prefix}b");
    mark(&name_a, None);
    mark(&name_b, None);

    clear_marks(Some(&name_a));

    let remaining: Vec<String> = get_marks().into_iter().filter(|m| m.name.starts_with(&prefix)).map(|m| m.name).collect();
    assert_eq!(remaining, vec![name_b]);
}

#[test]
fn does_not_clear_marks_with_a_different_name() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let prefix = unique_prefix();
    let _teardown = Teardown;
    let name1 = format!("{prefix}a");
    let name2 = format!("{}b", unique_prefix());
    mark(&name1, None);
    mark(&name2, None);

    clear_marks(Some(&name1));

    assert_eq!(get_marks().iter().filter(|m| m.name == name1).count(), 0);
    assert_eq!(get_marks().iter().filter(|m| m.name == name2).count(), 1);
}
