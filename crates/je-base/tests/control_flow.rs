//! Testes próprios de `src/vs/base/common/controlFlow.ts` (o VS Code não tem `controlFlow.test.ts`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::control_flow::ReentrancyBarrier;

#[test]
fn run_exclusively_or_skip_skips_reentrant_calls() {
    let barrier = Rc::new(ReentrancyBarrier::new());
    let log = Rc::new(RefCell::new(Vec::new()));
    let (b, l) = (barrier.clone(), log.clone());
    barrier.run_exclusively_or_skip(|| {
        l.borrow_mut().push("outer");
        assert!(b.is_occupied());
        let l2 = l.clone();
        b.run_exclusively_or_skip(|| l2.borrow_mut().push("inner"));
    });
    assert_eq!(*log.borrow(), vec!["outer"]);
    assert!(!barrier.is_occupied());
}

#[test]
fn run_exclusively_or_throw_throws_on_reentry_and_releases() {
    let barrier = Rc::new(ReentrancyBarrier::new());
    let b = barrier.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        barrier.run_exclusively_or_throw(|| b.run_exclusively_or_throw(|| {}));
    }));
    let payload = result.unwrap_err();
    let error = je_base::errors::from_panic(payload);
    assert_eq!(error.message, "ReentrancyBarrier: reentrant call detected!");
    // O `finally` devolve a barreira ao estado livre.
    assert!(!barrier.is_occupied());
    barrier.run_exclusively_or_throw(|| {});
}

#[test]
fn barrier_is_released_when_the_runner_panics() {
    let barrier = ReentrancyBarrier::new();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| barrier.run_exclusively_or_skip(|| panic!("x"))));
    assert!(result.is_err());
    assert!(!barrier.is_occupied());
}

#[test]
fn make_exclusive_or_skip_wraps_a_function() {
    let barrier = ReentrancyBarrier::new();
    let count = Cell::new(0);
    let f = barrier.make_exclusive_or_skip(|n: i32| count.set(count.get() + n));
    f(2);
    barrier.run_exclusively_or_skip(|| f(10));
    f(3);
    assert_eq!(count.get(), 5);
}
