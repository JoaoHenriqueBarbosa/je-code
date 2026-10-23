//! Testes próprios de `src/vs/base/common/functional.ts` (o VS Code não tem `functional.test.ts`).

use std::cell::Cell;
use std::rc::Rc;

use je_base::functional::create_single_call_function;

#[test]
fn runs_once_and_returns_the_first_result() {
    let calls = Rc::new(Cell::new(0));
    let counter = calls.clone();
    let f = create_single_call_function(
        move |x: i32| {
            counter.set(counter.get() + 1);
            x * 2
        },
        None,
    );
    assert_eq!(f(21), 42);
    assert_eq!(f(100), 42);
    assert_eq!(calls.get(), 1);
}

#[test]
fn did_run_callback_runs_once_after_the_function() {
    let log = Rc::new(std::cell::RefCell::new(Vec::new()));
    let (a, b) = (log.clone(), log.clone());
    let f = create_single_call_function(
        move |()| {
            a.borrow_mut().push("fn");
            1
        },
        Some(Box::new(move || b.borrow_mut().push("did-run"))),
    );
    f(());
    f(());
    assert_eq!(*log.borrow(), vec!["fn", "did-run"]);
}

#[test]
fn did_run_callback_runs_even_if_the_function_panics() {
    let ran = Rc::new(Cell::new(false));
    let flag = ran.clone();
    let f = create_single_call_function(|()| -> i32 { panic!("falha") }, Some(Box::new(move || flag.set(true))));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(())));
    assert!(result.is_err());
    assert!(ran.get());
    // `didCall` já era verdadeiro: a segunda chamada não repete a função e devolve o valor inicial.
    assert_eq!(f(()), 0);
}
