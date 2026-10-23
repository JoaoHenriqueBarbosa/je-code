//! Casos próprios do porte de `paging.ts`: o original só testa o `PagedModel` (ver `paging.rs`); aqui
//! entram `IterativePagedModel`, `PageIteratorPager`, `DelayedPagedModel` e `mapPager`.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::run_with_faked_timers;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors;
use je_base::paging::{
    DelayedPagedModel, IIterativePager, IPageIterator, IPager, IPagedModel, IterativePage, IterativePagedModel, PageIteratorPager, PagedModel, map_pager,
};
use je_base::promise::Promise;
use je_base::runtime;

struct TwoPages {
    next_calls: Cell<usize>,
}

impl IIterativePager<i32> for TwoPages {
    fn first_page(&self) -> IterativePage<i32> {
        IterativePage { items: vec![1, 2], has_more: true }
    }

    fn get_next_page(&self, _token: CancellationToken) -> Promise<IterativePage<i32>> {
        self.next_calls.set(self.next_calls.get() + 1);
        Promise::resolved(IterativePage { items: vec![3], has_more: false })
    }
}

#[test]
fn iterative_paged_model_loads_on_the_sentinel() {
    run_with_faked_timers(async {
        let pager = Rc::new(TwoPages { next_calls: Cell::new(0) });
        let model = IterativePagedModel::new(pager.clone());
        let lengths: Rc<RefCell<Vec<usize>>> = Rc::default();
        let seen = lengths.clone();
        let _listener = model.on_did_increment_length().listen(move |length| seen.borrow_mut().push(*length));

        // O tamanho conta o sentinela enquanto há mais páginas.
        assert_eq!(model.length(), 3);
        assert!(model.is_resolved(1));
        assert!(!model.is_resolved(2));
        assert!(errors::catch(|| model.get(2)).is_err());

        // Duas leituras do sentinela juntas carregam uma página só.
        let (first, second) = (model.resolve(2, CancellationToken::None), model.resolve(2, CancellationToken::None));
        assert_eq!(first.await.unwrap(), 3);
        assert_eq!(second.await.unwrap(), 3);
        assert_eq!(pager.next_calls.get(), 1);
        assert_eq!(model.length(), 3);
        assert!(model.is_resolved(2));
        assert_eq!(model.get(2), 3);
        assert_eq!(*lengths.borrow(), vec![3]);

        // Fora da lista, sem mais páginas.
        assert!(model.resolve(3, CancellationToken::None).await.is_err());
        assert!(errors::is_cancellation_error(&model.resolve(0, CancellationToken::Cancelled).await.unwrap_err()));
    });
}

struct CountingIterator {
    page: usize,
    pages: Rc<Vec<Vec<i32>>>,
    loads: Rc<Cell<usize>>,
}

impl IPageIterator<i32> for CountingIterator {
    fn elements(&self) -> Vec<i32> {
        self.pages[self.page].clone()
    }

    fn total(&self) -> usize {
        self.pages.iter().map(Vec::len).sum()
    }

    fn has_next_page(&self) -> bool {
        self.page + 1 < self.pages.len()
    }

    fn get_next_page(&self, _token: CancellationToken) -> Promise<Rc<dyn IPageIterator<i32>>> {
        self.loads.set(self.loads.get() + 1);
        let next: Rc<dyn IPageIterator<i32>> = Rc::new(CountingIterator { page: self.page + 1, pages: self.pages.clone(), loads: self.loads.clone() });
        Promise::resolved(next)
    }
}

#[test]
fn page_iterator_pager_loads_pages_in_sequence_and_caches_them() {
    run_with_faked_timers(async {
        let loads = Rc::new(Cell::new(0));
        let pages = Rc::new(vec![vec![0, 1], vec![2, 3], vec![4, 5]]);
        let pager = PageIteratorPager::new(Rc::new(CountingIterator { page: 0, pages, loads: loads.clone() }));
        assert_eq!((pager.page_size(), pager.total(), pager.first_page()), (2, 6, vec![0, 1]));

        // Pedir a página 2 carrega a 1 e a 2.
        assert_eq!(pager.get_page(2, CancellationToken::None).await.unwrap(), vec![4, 5]);
        assert_eq!(loads.get(), 2);
        assert_eq!(pager.get_page(1, CancellationToken::None).await.unwrap(), vec![2, 3]);
        assert_eq!(loads.get(), 2);

        // Depois da última, é fora do limite.
        let error = pager.get_page(3, CancellationToken::None).await.unwrap_err();
        assert!(error.to_string().contains("Page 3 is out of bounds. Total pages: 3"), "{error}");

        let source = CancellationTokenSource::new(None);
        source.cancel();
        assert!(errors::is_cancellation_error(&pager.get_page(0, source.token()).await.unwrap_err()));
    });
}

#[test]
fn page_iterator_pager_shares_one_load_between_concurrent_requests() {
    run_with_faked_timers(async {
        let loads = Rc::new(Cell::new(0));
        let pages = Rc::new(vec![vec![0, 1], vec![2, 3], vec![4, 5]]);
        let pager = PageIteratorPager::new(Rc::new(CountingIterator { page: 0, pages, loads: loads.clone() }));

        let (two, one) = (pager.get_page(2, CancellationToken::None), pager.get_page(1, CancellationToken::None));
        assert_eq!(one.await.unwrap(), vec![2, 3]);
        assert_eq!(two.await.unwrap(), vec![4, 5]);
        assert_eq!(loads.get(), 2);
    });
}

#[test]
fn map_pager_maps_the_first_page_and_the_others() {
    run_with_faked_timers(async {
        let pager = map_pager(je_base::paging::single_page_pager(vec![1, 2, 3]), |n| n * 10);
        assert_eq!((pager.first_page(), pager.total(), pager.page_size()), (vec![10, 20, 30], 3, 3));
        assert_eq!(pager.get_page(0, CancellationToken::None).await.unwrap(), vec![10, 20, 30]);
    });
}

#[test]
fn delayed_paged_model_waits_and_honours_cancellation() {
    run_with_faked_timers(async {
        let model = DelayedPagedModel::new(Rc::new(PagedModel::from_vec(vec![7, 8, 9])), Some(100));
        assert_eq!((model.length(), model.is_resolved(1), model.get(1)), (3, true, 8));

        let started = runtime::now();
        assert_eq!(model.resolve(2, CancellationToken::None).await.unwrap(), 9);
        assert_eq!(runtime::now() - started, 100);

        // Cancelar antes do prazo rejeita sem esperar o resto.
        let source = Rc::new(CancellationTokenSource::new(None));
        let pending = model.resolve(0, source.token());
        let cancelling = source.clone();
        runtime::set_timeout(move || cancelling.cancel(), 10);
        let started = runtime::now();
        assert!(errors::is_cancellation_error(&pending.await.unwrap_err()));
        assert_eq!(runtime::now() - started, 10);

        // Token já cancelado: rejeita direto.
        assert!(errors::is_cancellation_error(&model.resolve(0, CancellationToken::Cancelled).await.unwrap_err()));
    });
}
