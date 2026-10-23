//! Porte de `src/vs/base/test/common/paging.test.ts` (suíte `PagedModel`), caso a caso e na mesma ordem.
//!
//! Os testes `async` rodam em `common::run_with_faked_timers`, e o `store` do original é o `LeakGuard` de
//! `ensure_no_disposables_are_leaked_in_test_suite`. Uma asserção que o original faz dentro de um `then`
//! solto vira `unwrap` aqui, para uma falha reprovar o teste.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::async_::disposable_timeout;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::{self, Error};
use je_base::paging::{IPager, IPagedModel, PagedModel};
use je_base::promise::Promise;
use je_base::runtime;

type GetPageFn = Box<dyn Fn(usize, CancellationToken) -> Promise<Vec<i32>>>;

fn get_page(page_index: usize, cancellation_token: CancellationToken) -> Promise<Vec<i32>> {
    if cancellation_token.is_cancellation_requested() {
        return Promise::rejected(Error::cancellation());
    }

    Promise::resolved((0..5).map(|i| i + (page_index as i32 * 5)).collect())
}

struct TestPager {
    get_page: GetPageFn,
}

impl TestPager {
    fn new(get_page_fn: Option<GetPageFn>) -> Rc<Self> {
        Rc::new(Self { get_page: get_page_fn.unwrap_or_else(|| Box::new(get_page)) })
    }
}

impl IPager<i32> for TestPager {
    fn first_page(&self) -> Vec<i32> {
        vec![0, 1, 2, 3, 4]
    }

    fn total(&self) -> usize {
        100
    }

    fn page_size(&self) -> usize {
        5
    }

    fn get_page(&self, page_index: usize, cancellation_token: CancellationToken) -> Promise<Vec<i32>> {
        (self.get_page)(page_index, cancellation_token)
    }
}

#[test]
fn is_resolved() {
    let pager = TestPager::new(None);
    let model = PagedModel::new(pager);

    assert!(model.is_resolved(0));
    assert!(model.is_resolved(1));
    assert!(model.is_resolved(2));
    assert!(model.is_resolved(3));
    assert!(model.is_resolved(4));
    assert!(!model.is_resolved(5));
    assert!(!model.is_resolved(6));
    assert!(!model.is_resolved(7));
    assert!(!model.is_resolved(8));
    assert!(!model.is_resolved(9));
    assert!(!model.is_resolved(10));
    assert!(!model.is_resolved(99));
}

#[test]
fn resolve_single() {
    run_with_faked_timers(async {
        let pager = TestPager::new(None);
        let model = PagedModel::new(pager);

        assert!(!model.is_resolved(5));

        model.resolve(5, CancellationToken::None).await.unwrap();
        assert!(model.is_resolved(5));
    });
}

#[test]
fn resolve_page() {
    run_with_faked_timers(async {
        let pager = TestPager::new(None);
        let model = PagedModel::new(pager);

        assert!(!model.is_resolved(5));
        assert!(!model.is_resolved(6));
        assert!(!model.is_resolved(7));
        assert!(!model.is_resolved(8));
        assert!(!model.is_resolved(9));
        assert!(!model.is_resolved(10));

        model.resolve(5, CancellationToken::None).await.unwrap();
        assert!(model.is_resolved(5));
        assert!(model.is_resolved(6));
        assert!(model.is_resolved(7));
        assert!(model.is_resolved(8));
        assert!(model.is_resolved(9));
        assert!(!model.is_resolved(10));
    });
}

#[test]
fn resolve_page_2() {
    run_with_faked_timers(async {
        let pager = TestPager::new(None);
        let model = PagedModel::new(pager);

        assert!(!model.is_resolved(5));
        assert!(!model.is_resolved(6));
        assert!(!model.is_resolved(7));
        assert!(!model.is_resolved(8));
        assert!(!model.is_resolved(9));
        assert!(!model.is_resolved(10));

        model.resolve(10, CancellationToken::None).await.unwrap();
        assert!(!model.is_resolved(5));
        assert!(!model.is_resolved(6));
        assert!(!model.is_resolved(7));
        assert!(!model.is_resolved(8));
        assert!(!model.is_resolved(9));
        assert!(model.is_resolved(10));
    });
}

#[test]
fn preemptive_cancellation_works() {
    run_with_faked_timers(async {
        let pager = TestPager::new(Some(Box::new(|_, _| panic!("o pager não devia ser chamado"))));

        let model = PagedModel::new(pager);

        match model.resolve(5, CancellationToken::Cancelled).await {
            Ok(_) => panic!("devia rejeitar"),
            Err(err) => assert!(errors::is_cancellation_error(&err)),
        }
    });
}

#[test]
fn cancellation_works() {
    run_with_faked_timers(async {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let store_for_pager = store.store();
        let pager = TestPager::new(Some(Box::new(move |_, token| {
            Promise::with_executor(|resolver| {
                store_for_pager.add(token.on_cancellation_requested().listen(move |_| resolver.reject(Error::cancellation())));
            })
        })));

        let model = PagedModel::new(pager);
        let token_source = Rc::new(CancellationTokenSource::new(None));
        store.add_rc(token_source.clone());

        let promise = model.resolve(5, token_source.token());

        let source = token_source.clone();
        runtime::set_timeout(move || source.cancel(), 10);

        let err = promise.await.expect_err("devia rejeitar");
        assert!(errors::is_cancellation_error(&err));
    });
}

#[test]
fn same_page_cancellation_works() {
    run_with_faked_timers(async {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let state = Rc::new(RefCell::new("idle".to_string()));

        let (pager_state, store_for_pager) = (state.clone(), store.store());
        let pager = TestPager::new(Some(Box::new(move |_page_index, token| {
            *pager_state.borrow_mut() = "resolving".to_string();

            let (state, store) = (pager_state.clone(), store_for_pager.clone());
            Promise::with_executor(move |resolver| {
                store.add(token.on_cancellation_requested().listen(move |_| {
                    *state.borrow_mut() = "idle".to_string();
                    resolver.reject(Error::cancellation());
                }));
            })
        })));

        let model = PagedModel::new(pager);

        assert_eq!(*state.borrow(), "idle");

        let token_source1 = Rc::new(CancellationTokenSource::new(None));
        let promise1 = model.resolve(5, token_source1.token());

        assert_eq!(*state.borrow(), "resolving");

        let token_source2 = Rc::new(CancellationTokenSource::new(None));
        let promise2 = model.resolve(6, token_source2.token());

        assert_eq!(*state.borrow(), "resolving");

        let (state_a, store_a) = (state.clone(), store.store());
        let (source1, source2) = (token_source1.clone(), token_source2.clone());
        store.add(disposable_timeout(
            move || {
                assert_eq!(*state_a.borrow(), "resolving");
                source1.cancel();
                assert_eq!(*state_a.borrow(), "resolving");

                let state_b = state_a.clone();
                store_a.add(disposable_timeout(
                    move || {
                        assert_eq!(*state_b.borrow(), "resolving");
                        source2.cancel();
                        assert_eq!(*state_b.borrow(), "idle");
                    },
                    10,
                    None,
                ));
            },
            10,
            None,
        ));

        assert!(errors::is_cancellation_error(&promise1.await.expect_err("devia rejeitar")));
        assert!(errors::is_cancellation_error(&promise2.await.expect_err("devia rejeitar")));
    });
}
