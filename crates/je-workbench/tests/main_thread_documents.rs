//! Porte de `src/vs/workbench/api/test/browser/mainThreadDocuments.test.ts` (suíte `BoundModelReferenceCollection`), caso
//! a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso; o `setup` e o `teardown`
//!   (criar e descartar a coleção) são [`new_collection`] e o `col.dispose()` no fim do caso.
//! - `{ object: {}, dispose() { ... } }` é a [`DisposableRef`] de `to_disposable`: só o `dispose` da referência é lido.
//! - `col.add(uri, ref)` sem o tamanho é `add(uri, ref, 0)`; o caso `max age` (`async`) roda em `run_with_faked_timers`,
//!   onde `await timeout(30)` avança o relógio virtual.
//! - `new BoundModelReferenceCollection(extUri, 10000, 10000, 2)` é o [`BoundModelReferenceLimits`] com os três limites.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::uri::URI;
use je_platform::resources::ext_uri;
use je_workbench::api::browser::main_thread_documents::{BoundModelReferenceCollection, BoundModelReferenceLimits};

/// `col = new BoundModelReferenceCollection(extUri, 15, 75)` do `setup`.
fn new_collection() -> BoundModelReferenceCollection {
    BoundModelReferenceCollection::new(ext_uri(), BoundModelReferenceLimits { max_age: 15, max_length: 75, max_size: 50 })
}

/// `{ object: {}, dispose() { disposed.push(index) } }`.
fn pushing_reference(disposed: &Rc<RefCell<Vec<u32>>>, index: u32) -> DisposableRef {
    let disposed = disposed.clone();
    to_disposable(move || disposed.borrow_mut().push(index))
}

#[test]
fn max_age() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async {
        let col = new_collection();
        let did_dispose = Rc::new(Cell::new(false));

        let flag = did_dispose.clone();
        col.add(URI::parse("test://farboo", false), to_disposable(move || flag.set(true)), 0);

        timeout(30).await;
        assert!(did_dispose.get());

        col.dispose();
    });
}

#[test]
fn max_size() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let col = new_collection();

    let disposed = Rc::new(RefCell::new(Vec::new()));

    col.add(URI::parse("test://farboo", false), pushing_reference(&disposed, 0), 6);

    col.add(URI::parse("test://boofar", false), pushing_reference(&disposed, 1), 6);

    col.add(URI::parse("test://xxxxxxx", false), pushing_reference(&disposed, 2), 70);

    assert_eq!(*disposed.borrow(), vec![0, 1]);

    col.dispose();
}

#[test]
fn max_count() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let mut col = new_collection();

    col.dispose();
    col = BoundModelReferenceCollection::new(ext_uri(), BoundModelReferenceLimits { max_age: 10000, max_length: 10000, max_size: 2 });

    let disposed = Rc::new(RefCell::new(Vec::new()));

    col.add(URI::parse("test://xxxxxxx", false), pushing_reference(&disposed, 0), 0);
    col.add(URI::parse("test://xxxxxxx", false), pushing_reference(&disposed, 1), 0);
    col.add(URI::parse("test://xxxxxxx", false), pushing_reference(&disposed, 2), 0);

    assert_eq!(*disposed.borrow(), vec![0]);

    col.dispose();
}

#[test]
fn dispose_uri() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let col = new_collection();

    let disposed = Rc::new(RefCell::new(Vec::new()));

    col.add(URI::parse("test:///farboo", false), pushing_reference(&disposed, 0), 0);

    col.add(URI::parse("test:///boofar", false), pushing_reference(&disposed, 1), 0);

    col.add(URI::parse("test:///boo/far1", false), pushing_reference(&disposed, 2), 0);

    col.add(URI::parse("test:///boo/far2", false), pushing_reference(&disposed, 3), 0);

    col.add(URI::parse("test:///boo1/far", false), pushing_reference(&disposed, 4), 0);

    col.remove(&URI::parse("test:///unknown", false));
    assert_eq!(disposed.borrow().len(), 0);

    col.remove(&URI::parse("test:///farboo", false));
    assert_eq!(*disposed.borrow(), vec![0]);

    disposed.borrow_mut().clear();

    col.remove(&URI::parse("test:///boo", false));
    assert_eq!(*disposed.borrow(), vec![2, 3]);

    col.dispose();
}
