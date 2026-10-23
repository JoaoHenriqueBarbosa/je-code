//! Porte de `src/vs/base/test/common/iterativePaging.test.ts` (suíte `IterativePagedModel`), caso a caso e
//! na mesma ordem.
//!
//! Os testes `async` rodam em `common::run_with_faked_timers`, e o `store` do original é o `LeakGuard` de
//! `ensure_no_disposables_are_leaked_in_test_suite`. O `Promise.all` do original vira três promessas
//! criadas antes de qualquer espera, aguardadas em seguida.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors;
use je_base::paging::{IIterativePager, IPagedModel, IterativePage, IterativePagedModel};
use je_base::promise::Promise;

type GetNextPageFn<T> = Box<dyn Fn(CancellationToken) -> Promise<IterativePage<T>>>;

/// Um `IIterativePager` literal, como os objetos que o original monta nos casos.
struct TestPager<T: Clone + 'static> {
    first_page: IterativePage<T>,
    get_next_page: GetNextPageFn<T>,
}

impl<T: Clone + 'static> TestPager<T> {
    fn new(first_page: IterativePage<T>, get_next_page: impl Fn(CancellationToken) -> IterativePage<T> + 'static) -> Rc<Self> {
        Rc::new(Self { first_page, get_next_page: Box::new(move |token| Promise::resolved(get_next_page(token))) })
    }
}

impl<T: Clone + 'static> IIterativePager<T> for TestPager<T> {
    fn first_page(&self) -> IterativePage<T> {
        self.first_page.clone()
    }

    fn get_next_page(&self, cancellation_token: CancellationToken) -> Promise<IterativePage<T>> {
        (self.get_next_page)(cancellation_token)
    }
}

fn create_test_pager(page_size: i32, max_pages: i32) -> Rc<TestPager<i32>> {
    let current_page = Rc::new(Cell::new(0));

    let create_page = move |page: i32| {
        let start = page * page_size;
        let items = (0..page_size).map(|i| start + i).collect();
        let has_more = page + 1 < max_pages;
        IterativePage { items, has_more }
    };

    let first_page = create_page(current_page.replace(1));
    TestPager::new(first_page, move |_token| {
        if current_page.get() >= max_pages {
            return IterativePage { items: vec![], has_more: false };
        }
        create_page(current_page.replace(current_page.get() + 1))
    })
}

fn empty_next_page<T>(_token: CancellationToken) -> IterativePage<T> {
    IterativePage { items: vec![], has_more: false }
}

#[test]
fn initial_state() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let pager = create_test_pager(10, 3);
    let model = store.add_rc(Rc::new(IterativePagedModel::new(pager)));

    // Initially first page is loaded, so length should be 10 + 1 sentinel
    assert_eq!(model.length(), 11);
    assert!(model.is_resolved(0));
    assert!(model.is_resolved(9));
    assert!(!model.is_resolved(10)); // sentinel
}

#[test]
fn load_first_page_via_sentinel_access() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let pager = create_test_pager(10, 3);
        let model = store.add_rc(Rc::new(IterativePagedModel::new(pager)));

        // Access an item in the first page (already loaded)
        let item = model.resolve(0, CancellationToken::None).await.unwrap();

        assert_eq!(item, 0);
        assert_eq!(model.length(), 11); // 10 items + 1 sentinel
        assert!(model.is_resolved(0));
        assert!(model.is_resolved(9));
        assert!(!model.is_resolved(10)); // sentinel
    });
}

#[test]
fn load_multiple_pages() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let pager = create_test_pager(10, 3);
        let model = store.add_rc(Rc::new(IterativePagedModel::new(pager)));

        // First page already loaded
        assert_eq!(model.length(), 11);

        // Load second page by accessing its sentinel
        model.resolve(10, CancellationToken::None).await.unwrap();
        assert_eq!(model.length(), 21); // 20 items + 1 sentinel
        assert_eq!(model.get(10), 10); // First item of second page

        // Load third (final) page
        model.resolve(20, CancellationToken::None).await.unwrap();
        assert_eq!(model.length(), 30); // 30 items, no sentinel (no more pages)
    });
}

#[test]
fn on_did_increment_length_event_fires_correctly() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let pager = create_test_pager(10, 3);
        let model = store.add_rc(Rc::new(IterativePagedModel::new(pager)));
        let lengths: Rc<RefCell<Vec<usize>>> = Rc::default();

        let seen = lengths.clone();
        store.add(model.on_did_increment_length().listen(move |length| seen.borrow_mut().push(*length)));

        // Load second page
        model.resolve(10, CancellationToken::None).await.unwrap();

        assert_eq!(lengths.borrow().len(), 1);
        assert_eq!(lengths.borrow()[0], 21); // 20 items + 1 sentinel

        // Load third page
        model.resolve(20, CancellationToken::None).await.unwrap();

        assert_eq!(lengths.borrow().len(), 2);
        assert_eq!(lengths.borrow()[1], 30); // 30 items, no sentinel
    });
}

#[test]
fn accessing_regular_items_does_not_trigger_loading() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let pager = create_test_pager(10, 3);
        let model = store.add_rc(Rc::new(IterativePagedModel::new(pager)));

        let initial_length = model.length();

        // Access items within the loaded range
        assert_eq!(model.get(5), 5);
        assert!(model.is_resolved(5));

        // Length should not change
        assert_eq!(model.length(), initial_length);
    });
}

#[test]
fn reaching_end_of_data_removes_sentinel() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let pager = create_test_pager(10, 3);
        let model = store.add_rc(Rc::new(IterativePagedModel::new(pager)));

        // Load all pages
        model.resolve(10, CancellationToken::None).await.unwrap(); // Page 2
        model.resolve(20, CancellationToken::None).await.unwrap(); // Page 3 (final)

        // After loading all data, there should be no more pages
        assert_eq!(model.length(), 30); // Exactly 30 items, no sentinel

        // Accessing resolved items should work
        assert!(model.is_resolved(29));
        assert!(!model.is_resolved(30));
    });
}

#[test]
fn concurrent_access_to_sentinel_only_loads_once() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let pager = create_test_pager(10, 3);
        let model = store.add_rc(Rc::new(IterativePagedModel::new(pager)));

        // Access sentinel concurrently
        let (first, second, third) =
            (model.resolve(10, CancellationToken::None), model.resolve(10, CancellationToken::None), model.resolve(10, CancellationToken::None));
        let (item1, item2, item3) = (first.await.unwrap(), second.await.unwrap(), third.await.unwrap());

        // All should get the same item
        assert_eq!(item1, 10);
        assert_eq!(item2, 10);
        assert_eq!(item3, 10);
        assert_eq!(model.length(), 21); // 20 items + 1 sentinel
    });
}

#[test]
fn empty_pager_with_no_items() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let empty_pager = TestPager::new(IterativePage { items: vec![], has_more: false }, empty_next_page::<i32>);
    let model = store.add_rc(Rc::new(IterativePagedModel::new(empty_pager)));

    assert_eq!(model.length(), 0);
    assert!(!model.is_resolved(0));
}

#[test]
fn single_page_pager_with_no_more_pages() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let single_page_pager = TestPager::new(IterativePage { items: vec![1, 2, 3], has_more: false }, empty_next_page::<i32>);
    let model = store.add_rc(Rc::new(IterativePagedModel::new(single_page_pager)));

    assert_eq!(model.length(), 3); // No sentinel
    assert!(model.is_resolved(0));
    assert!(model.is_resolved(2));
    assert!(!model.is_resolved(3));
    assert_eq!(model.get(0), 1);
    assert_eq!(model.get(2), 3);
}

#[test]
fn accessing_item_beyond_loaded_range_throws() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let pager = create_test_pager(10, 3);
    let model = store.add_rc(Rc::new(IterativePagedModel::new(pager)));

    // Try to access item beyond current length
    let error = errors::catch(|| model.get(15)).unwrap_err();
    assert!(error.to_string().contains("Item not resolved yet"), "{error}");
}

#[test]
fn resolving_item_beyond_all_pages_throws() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let pager = create_test_pager(10, 3);
        let model = store.add_rc(Rc::new(IterativePagedModel::new(pager)));

        // Load all pages
        model.resolve(10, CancellationToken::None).await.unwrap();
        model.resolve(20, CancellationToken::None).await.unwrap();

        // Try to resolve beyond the last item
        let error = model.resolve(30, CancellationToken::None).await.unwrap_err();
        assert!(error.to_string().contains("Index out of bounds"), "{error}");
    });
}

#[test]
fn cancelled_token_during_initial_resolve() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let pager = create_test_pager(10, 3);
        let model = store.add_rc(Rc::new(IterativePagedModel::new(pager)));

        let cts = CancellationTokenSource::new(None);
        cts.cancel();

        let error = model.resolve(0, cts.token()).await.unwrap_err();
        assert!(error.to_string().contains("Canceled"), "{error}");
        assert!(errors::is_cancellation_error(&error));
    });
}

#[test]
fn event_fires_for_each_page_load() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let pager = create_test_pager(5, 4);
        let model = store.add_rc(Rc::new(IterativePagedModel::new(pager)));
        let lengths: Rc<RefCell<Vec<usize>>> = Rc::default();

        let seen = lengths.clone();
        store.add(model.on_did_increment_length().listen(move |length| seen.borrow_mut().push(*length)));

        // Initially has first page (5 items + 1 sentinel = 6)
        assert_eq!(model.length(), 6);

        // Load page 2
        model.resolve(5, CancellationToken::None).await.unwrap();
        assert_eq!(*lengths.borrow(), vec![11]); // 10 items + 1 sentinel

        // Load page 3
        model.resolve(10, CancellationToken::None).await.unwrap();
        assert_eq!(*lengths.borrow(), vec![11, 16]); // 15 items + 1 sentinel

        // Load page 4 (final)
        model.resolve(15, CancellationToken::None).await.unwrap();
        assert_eq!(*lengths.borrow(), vec![11, 16, 20]); // 20 items, no sentinel
    });
}

#[test]
fn sequential_page_loads_work_correctly() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let pager = create_test_pager(5, 3);
        let model = store.add_rc(Rc::new(IterativePagedModel::new(pager)));

        // Load pages sequentially
        for page in 1..3 {
            let sentinel_index = page * 5;
            model.resolve(sentinel_index, CancellationToken::None).await.unwrap();
        }

        // Verify all items are accessible
        assert_eq!(model.length(), 15); // 3 pages * 5 items, no sentinel
        for i in 0..15 {
            assert_eq!(model.get(i), i as i32);
            assert!(model.is_resolved(i));
        }
    });
}

#[test]
fn accessing_items_after_loading_all_pages() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let pager = create_test_pager(10, 2);
        let model = store.add_rc(Rc::new(IterativePagedModel::new(pager)));

        // Load second page
        model.resolve(10, CancellationToken::None).await.unwrap();

        // No sentinel after loading all pages
        assert_eq!(model.length(), 20);
        assert!(model.is_resolved(19));
        assert!(!model.is_resolved(20));

        // All items should be accessible
        for i in 0..20 {
            assert_eq!(model.get(i), i as i32);
        }
    });
}

#[test]
fn pager_with_varying_page_sizes() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let page_num = Cell::new(0);
        let strings = |items: &[&str], has_more| IterativePage { items: items.iter().map(ToString::to_string).collect::<Vec<String>>(), has_more };
        let varying_pager = TestPager::new(strings(&["a", "b", "c"], true), move |_token| {
            page_num.set(page_num.get() + 1);
            match page_num.get() {
                1 => strings(&["d", "e"], true),
                2 => strings(&["f", "g", "h", "i"], false),
                _ => strings(&[], false),
            }
        });

        let model = store.add_rc(Rc::new(IterativePagedModel::new(varying_pager)));

        assert_eq!(model.length(), 4); // 3 items + 1 sentinel

        // Load second page (2 items)
        model.resolve(3, CancellationToken::None).await.unwrap();
        assert_eq!(model.length(), 6); // 5 items + 1 sentinel
        assert_eq!(model.get(3), "d");

        // Load third page (4 items)
        model.resolve(5, CancellationToken::None).await.unwrap();
        assert_eq!(model.length(), 9); // 9 items, no sentinel
        assert_eq!(model.get(5), "f");
        assert_eq!(model.get(8), "i");
    });
}
