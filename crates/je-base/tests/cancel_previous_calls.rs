//! Porte de `src/vs/base/test/common/cancelPreviousCalls.test.ts` (suíte `cancelPreviousCalls decorator`),
//! caso a caso e na mesma ordem.
//!
//! O Rust não tem decorator de método (ver `decorators.rs`): cada método decorado do `MockDisposable`
//! guarda um [`CancelPreviousCalls`] e o chama no começo, no lugar do `@cancelPreviousCalls`. O terceiro
//! argumento das chamadas (`CancellationToken | undefined`) é um `Option<CancellationToken>` em
//! [`CallArgs`], e o "tem 3 argumentos" do original vira "o token foi entregue" (`Some`). O
//! `MockDisposable` implementa `IDisposable` (o `extends Disposable` e o `this._register` do original) e é
//! registrado no store do teste. O `async`/`await` do original roda sobre o relógio virtual
//! (`run_with_faked_timers`), com o `timeout` dos helpers no lugar do `new Promise(setTimeout)`.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::cancellation::{CancellationToken, is_cancellation_token};
use je_base::decorators::CancelPreviousCalls;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;

/// Os argumentos com que um método decorado foi chamado.
#[derive(Clone)]
struct CallArgs {
    arg1: i32,
    arg2: String,
    cancellation_token: Option<CancellationToken>,
}

struct MockDisposable {
    cancel_previous_calls1: CancelPreviousCalls,
    cancel_previous_calls2: CancelPreviousCalls,
    /// Arguments that the `do_something_async` method was called with.
    call_args1: RefCell<Vec<CallArgs>>,
    /// Arguments that the `do_something_else_async` method was called with.
    call_args2: RefCell<Vec<CallArgs>>,
}

impl MockDisposable {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            cancel_previous_calls1: CancelPreviousCalls::new(),
            cancel_previous_calls2: CancelPreviousCalls::new(),
            call_args1: RefCell::default(),
            call_args2: RefCell::default(),
        })
    }

    /// Returns the arguments that the `do_something_async` method was called with.
    fn call_arguments1(&self) -> Vec<CallArgs> {
        self.call_args1.borrow().clone()
    }

    /// Returns the arguments that the `do_something_else_async` method was called with.
    fn call_arguments2(&self) -> Vec<CallArgs> {
        self.call_args2.borrow().clone()
    }

    /// `@cancelPreviousCalls async doSomethingAsync(arg1, arg2, cancellationToken?)`.
    fn do_something_async(&self, arg1: i32, arg2: &str, cancellation_token: Option<&CancellationToken>) -> Promise<()> {
        let token = self.cancel_previous_calls1.next_call(cancellation_token);
        self.call_args1.borrow_mut().push(CallArgs { arg1, arg2: arg2.to_string(), cancellation_token: Some(token) });

        timeout(25)
    }

    /// `@cancelPreviousCalls async doSomethingElseAsync(arg1, arg2, cancellationToken?)`.
    fn do_something_else_async(&self, arg1: i32, arg2: &str, cancellation_token: Option<&CancellationToken>) -> Promise<()> {
        let token = self.cancel_previous_calls2.next_call(cancellation_token);
        self.call_args2.borrow_mut().push(CallArgs { arg1, arg2: arg2.to_string(), cancellation_token: Some(token) });

        timeout(25)
    }
}

impl IDisposable for MockDisposable {
    fn dispose(&self) {
        self.cancel_previous_calls1.dispose();
        self.cancel_previous_calls2.dispose();
    }
}

/// O terceiro argumento de uma chamada, conferido como `CancellationToken` (o `args.length === 3` e o
/// `CancellationToken.isCancellationToken(arg3)` do original).
fn token_of(args: &CallArgs, what: &str) -> CancellationToken {
    let token = args.cancellation_token.clone().unwrap_or_else(|| panic!("{what} must be called with '3' arguments"));
    assert!(is_cancellation_token(&token), "{what} must have the 'CancellationToken' as the 3rd argument");
    token
}

#[test]
fn should_call_method_with_cancellation_token() {
    let disposables = Rc::new(ensure_no_disposables_are_leaked_in_test_suite());
    run_with_faked_timers(async move {
        let instance = disposables.add_rc(MockDisposable::new());

        instance.do_something_async(1, "foo", None).await.unwrap();

        let call_arguments = instance.call_arguments1();
        assert_eq!(call_arguments.len(), 1, "The 'doSomethingAsync' method must be called just once.");

        let args = &call_arguments[0];
        let arg3 = token_of(args, "The 'doSomethingAsync' method");

        assert_eq!(args.arg1, 1, "The 'doSomethingAsync' method call must have the correct 1st argument.");
        assert_eq!(args.arg2, "foo", "The 'doSomethingAsync' method call must have the correct 2nd argument.");
        assert!(!arg3.is_cancellation_requested(), "The 'CancellationToken' argument must not yet be cancelled.");
        assert!(instance.call_arguments2().is_empty(), "The 'doSomethingElseAsync' method must not be called.");
    });
}

#[test]
fn cancel_token_of_the_previous_call_when_method_is_called_again() {
    let disposables = Rc::new(ensure_no_disposables_are_leaked_in_test_suite());
    run_with_faked_timers(async move {
        let instance = disposables.add_rc(MockDisposable::new());

        let _ = instance.do_something_async(1, "foo", None);
        timeout(10).await.unwrap();
        let _ = instance.do_something_async(2, "bar", None);

        let call_arguments = instance.call_arguments1();
        assert_eq!(call_arguments.len(), 2, "The 'doSomethingAsync' method must be called twice.");

        let call1_args = &call_arguments[0];
        let call1_token = token_of(call1_args, "The first call of the 'doSomethingAsync' method");

        assert_eq!(call1_args.arg1, 1, "The first call of the 'doSomethingAsync' method must have the correct 1st argument.");
        assert_eq!(call1_args.arg2, "foo", "The first call of the 'doSomethingAsync' method must have the correct 2nd argument.");
        assert!(call1_token.is_cancellation_requested(), "The 'CancellationToken' of the first call must be cancelled.");

        let call2_args = &call_arguments[1];
        let call2_token = token_of(call2_args, "The second call of the 'doSomethingAsync' method");

        assert_eq!(call2_args.arg1, 2, "The second call of the 'doSomethingAsync' method must have the correct 1st argument.");
        assert_eq!(call2_args.arg2, "bar", "The second call of the 'doSomethingAsync' method must have the correct 2nd argument.");
        assert!(!call2_token.is_cancellation_requested(), "The 'CancellationToken' of the second call must be cancelled.");

        assert!(instance.call_arguments2().is_empty(), "The 'doSomethingElseAsync' method must not be called.");
    });
}

#[test]
fn different_method_calls_must_not_interfere_with_each_other() {
    let disposables = Rc::new(ensure_no_disposables_are_leaked_in_test_suite());
    run_with_faked_timers(async move {
        let instance = disposables.add_rc(MockDisposable::new());

        let _ = instance.do_something_async(10, "baz", None);
        timeout(10).await.unwrap();
        let _ = instance.do_something_else_async(25, "qux", None);

        assert_eq!(instance.call_arguments1().len(), 1, "The 'doSomethingAsync' method must be called once.");

        let call1_args = instance.call_arguments1()[0].clone();
        let call1_token = token_of(&call1_args, "The first call of the 'doSomethingAsync' method");

        assert_eq!(call1_args.arg1, 10, "The first call of the 'doSomethingAsync' method must have the correct 1st argument.");
        assert_eq!(call1_args.arg2, "baz", "The first call of the 'doSomethingAsync' method must have the correct 2nd argument.");
        assert!(!call1_token.is_cancellation_requested(), "The 'CancellationToken' of the first call must not be cancelled.");

        assert_eq!(instance.call_arguments2().len(), 1, "The 'doSomethingElseAsync' method must be called once.");

        let call2_args = instance.call_arguments2()[0].clone();
        let call2_token = token_of(&call2_args, "The first call of the 'doSomethingElseAsync' method");

        assert_eq!(call2_args.arg1, 25, "The first call of the 'doSomethingElseAsync' method must have the correct 1st argument.");
        assert_eq!(call2_args.arg2, "qux", "The first call of the 'doSomethingElseAsync' method must have the correct 2nd argument.");
        assert!(!call2_token.is_cancellation_requested(), "The 'CancellationToken' of the second call must be cancelled.");

        let _ = instance.do_something_else_async(105, "uxi", None);

        assert_eq!(instance.call_arguments1().len(), 1, "The 'doSomethingAsync' method must be called once.");
        assert_eq!(instance.call_arguments2().len(), 2, "The 'doSomethingElseAsync' method must be called twice.");

        assert!(!call1_token.is_cancellation_requested(), "The 'CancellationToken' of the first call must not be cancelled.");

        let call3_args = instance.call_arguments2()[1].clone();
        let call3_token = call3_args
            .cancellation_token
            .clone()
            .filter(|token| is_cancellation_token(token))
            .expect("The last argument of the second call of the 'doSomethingElseAsync' method must be a 'CancellationToken'.");

        assert!(call2_token.is_cancellation_requested(), "The 'CancellationToken' of the first call must be cancelled.");
        assert!(!call3_token.is_cancellation_requested(), "The 'CancellationToken' of the second call must not be cancelled.");

        assert_eq!(call3_args.arg1, 105, "The second call of the 'doSomethingElseAsync' method must have the correct 1st argument.");
        assert_eq!(call3_args.arg2, "uxi", "The second call of the 'doSomethingElseAsync' method must have the correct 2nd argument.");
    });
}
