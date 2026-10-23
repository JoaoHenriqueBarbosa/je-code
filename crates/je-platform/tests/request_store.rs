//! Porte de `src/vs/platform/terminal/test/common/requestStore.test.ts` (suíte `RequestStore`), caso a caso e
//! na mesma ordem.
//!
//! Divergências: o `TestInstantiationService` com o `ILogService` falso é a construção direta do
//! [`RequestStore`] com um `LogService` (`ConsoleLogger`), criado antes do `LeakGuard` como o `setup` do
//! original roda antes do `ensureNoDisposablesAreLeakedInTestSuite`. Os testes `async` rodam em
//! `run_with_faked_timers` (o timer de 1 ms dispara no relógio virtual). O
//! `strictEqual(typeof eventArgs?.requestId, 'number')` é garantido pelo tipo (`u64`), e o teste confere que o
//! evento chegou. `fail()` é o `panic!`.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_platform::log::{ConsoleLogger, DEFAULT_LOG_LEVEL, DefaultLogService, LogServiceRef};
use je_platform::terminal::request_store::{RequestEvent, RequestStore};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Reply {
    data: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Request {
    arg: String,
}

fn log_service() -> LogServiceRef {
    DefaultLogService::new(ConsoleLogger::new(DEFAULT_LOG_LEVEL, true), Vec::new())
}

#[test]
fn should_resolve_requests() {
    let log = log_service();
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let request_store = store.add_rc(RequestStore::<Reply, Request>::new(None, log));
    let event_args: Rc<RefCell<Option<RequestEvent<Request>>>> = Rc::default();
    store.add({
        let event_args = event_args.clone();
        request_store.on_create_request().listen(move |e| *event_args.borrow_mut() = Some(e.clone()))
    });
    run_with_faked_timers(async move {
        let request = request_store.create_request(Request { arg: "foo".into() });
        let event = event_args.borrow().clone().expect("o evento onCreateRequest deveria ter disparado");
        assert_eq!(event.args.arg, "foo");
        request_store.accept_reply(event.request_id, Reply { data: "bar".into() });
        let result = request.await.unwrap();
        assert_eq!(result.data, "bar");
    });
}

#[test]
fn should_reject_the_promise_when_the_request_times_out() {
    let log = log_service();
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let request_store = store.add_rc(RequestStore::<Reply, Request>::new(Some(1), log));
    run_with_faked_timers(async move {
        let request = request_store.create_request(Request { arg: "foo".into() });
        let mut threw = false;
        if request.await.is_err() {
            threw = true;
        }
        if !threw {
            panic!("a promessa deveria ter sido rejeitada");
        }
    });
}
