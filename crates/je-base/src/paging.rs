//! Porte de `src/vs/base/common/paging.ts`: `IPager`, `PagedModel`, `DelayedPagedModel`,
//! `PageIteratorPager`, `IterativePagedModel` (a "iterativePaging" do VS Code) e `mapPager`.
//!
//! Um pager é um `Rc<dyn IPager<T>>` e um modelo, um `Rc<dyn IPagedModel<T>>`. Os elementos são entregues
//! por clone (`T: Clone`). As funções `async` do original são `async_fn` sobre o `runtime`.
//!
//! No `DelayedPagedModel` e no `PagedModel` as promessas rejeitam com `CancellationError` quando o token
//! já está cancelado ou é cancelado no meio.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use crate::async_::async_fn;
use crate::cancellation::{CancellationToken, CancellationTokenSource};
use crate::errors::{self, Error};
use crate::event::{Emitter, Event};
use crate::lifecycle::{DisposableRef, IDisposable};
use crate::map::OrderedMap;
use crate::promise::Promise;
use crate::runtime;

/// A Pager is a stateless abstraction over a paged collection.
pub trait IPager<T: Clone + 'static> {
    fn first_page(&self) -> Vec<T>;
    fn total(&self) -> usize;
    fn page_size(&self) -> usize;
    fn get_page(&self, page_index: usize, cancellation_token: CancellationToken) -> Promise<Vec<T>>;
}

/// `IIterativePage<T>`.
#[derive(Clone)]
pub struct IterativePage<T> {
    pub items: Vec<T>,
    pub has_more: bool,
}

/// `IIterativePager<T>`.
pub trait IIterativePager<T: Clone + 'static> {
    fn first_page(&self) -> IterativePage<T>;
    fn get_next_page(&self, cancellation_token: CancellationToken) -> Promise<IterativePage<T>>;
}

/// `IPageIterator<T>`.
pub trait IPageIterator<T: Clone + 'static> {
    fn elements(&self) -> Vec<T>;
    fn total(&self) -> usize;
    fn has_next_page(&self) -> bool;
    fn get_next_page(&self, cancellation_token: CancellationToken) -> Promise<Rc<dyn IPageIterator<T>>>;
}

struct Page<T> {
    is_resolved: bool,
    promise: Option<Promise<()>>,
    cts: Option<CancellationTokenSource>,
    promise_indexes: HashSet<usize>,
    elements: Vec<T>,
}

fn create_page<T>(elements: Option<Vec<T>>) -> Page<T> {
    Page {
        is_resolved: elements.is_some(),
        promise: None,
        cts: None,
        promise_indexes: HashSet::new(),
        elements: elements.unwrap_or_default(),
    }
}

/// A PagedModel is a stateful model over an abstracted paged collection.
pub trait IPagedModel<T: Clone + 'static> {
    fn length(&self) -> usize;
    fn on_did_increment_length(&self) -> Event<usize>;
    fn is_resolved(&self, index: usize) -> bool;
    fn get(&self, index: usize) -> T;
    fn resolve(&self, index: usize, cancellation_token: CancellationToken) -> Promise<T>;
}

struct SinglePagePager<T> {
    elements: Vec<T>,
}

impl<T: Clone + 'static> IPager<T> for SinglePagePager<T> {
    fn first_page(&self) -> Vec<T> {
        self.elements.clone()
    }

    fn total(&self) -> usize {
        self.elements.len()
    }

    fn page_size(&self) -> usize {
        self.elements.len()
    }

    fn get_page(&self, _page_index: usize, _cancellation_token: CancellationToken) -> Promise<Vec<T>> {
        Promise::resolved(self.elements.clone())
    }
}

pub fn single_page_pager<T: Clone + 'static>(elements: Vec<T>) -> Rc<dyn IPager<T>> {
    Rc::new(SinglePagePager { elements })
}

pub struct PagedModel<T: Clone + 'static> {
    pager: Rc<dyn IPager<T>>,
    pages: Vec<Rc<RefCell<Page<T>>>>,
}

impl<T: Clone + 'static> PagedModel<T> {
    /// `new PagedModel(pager)`.
    pub fn new(pager: Rc<dyn IPager<T>>) -> Self {
        let page_size = pager.page_size();
        let total_pages = if page_size == 0 { 0 } else { pager.total().div_ceil(page_size) };

        let mut pages = vec![Rc::new(RefCell::new(create_page(Some(pager.first_page()))))];
        pages.extend((0..total_pages.saturating_sub(1)).map(|_| Rc::new(RefCell::new(create_page(None)))));

        Self { pager, pages }
    }

    /// `new PagedModel(array)`: um pager de página única.
    pub fn from_vec(elements: Vec<T>) -> Self {
        Self::new(single_page_pager(elements))
    }

    fn locate(&self, index: usize) -> (usize, usize, Rc<RefCell<Page<T>>>) {
        let page_size = self.pager.page_size();
        let page_index = index / page_size;
        (page_index, index % page_size, self.pages[page_index].clone())
    }
}

impl<T: Clone + 'static> IPagedModel<T> for PagedModel<T> {
    fn length(&self) -> usize {
        self.pager.total()
    }

    fn on_did_increment_length(&self) -> Event<usize> {
        Event::none()
    }

    fn is_resolved(&self, index: usize) -> bool {
        self.locate(index).2.borrow().is_resolved
    }

    fn get(&self, index: usize) -> T {
        let (_, index_in_page, page) = self.locate(index);
        let page = page.borrow();
        page.elements[index_in_page].clone()
    }

    fn resolve(&self, index: usize, cancellation_token: CancellationToken) -> Promise<T> {
        if cancellation_token.is_cancellation_requested() {
            return Promise::rejected(Error::cancellation());
        }

        let (page_index, index_in_page, page) = self.locate(index);

        if page.borrow().is_resolved {
            return Promise::resolved(page.borrow().elements[index_in_page].clone());
        }

        if page.borrow().promise.is_none() {
            let cts = CancellationTokenSource::new(None);
            page.borrow_mut().cts = Some(cts.clone());
            let get_page = self.pager.get_page(page_index, cts.token());
            let settled_page = page.clone();
            let promise = get_page.then_result(move |result| {
                let mut page = settled_page.borrow_mut();
                page.promise = None;
                page.cts = None;
                match result {
                    Ok(elements) => {
                        page.elements = elements;
                        page.is_resolved = true;
                        Ok(())
                    }
                    Err(err) => {
                        page.is_resolved = false;
                        Err(err)
                    }
                }
            });
            page.borrow_mut().promise = Some(promise);
        }

        let cancelled_page = page.clone();
        let listener = cancellation_token.on_cancellation_requested().listen(move |_| {
            // Solta o `RefCell` antes de cancelar: o cancelamento roda código do pager.
            let cts = {
                let mut page = cancelled_page.borrow_mut();
                let Some(cts) = page.cts.clone() else { return };
                page.promise_indexes.remove(&index);
                (page.promise_indexes.is_empty()).then_some(cts)
            };
            if let Some(cts) = cts {
                cts.cancel();
            }
        });

        page.borrow_mut().promise_indexes.insert(index);

        let promise = page.borrow().promise.clone().expect("a promessa da página foi criada acima");
        let resolved_page = page.clone();
        promise.then(move |()| resolved_page.borrow().elements[index_in_page].clone()).finally(move || listener.dispose())
    }
}

pub struct DelayedPagedModel<T: Clone + 'static> {
    model: Rc<dyn IPagedModel<T>>,
    timeout: u64,
}

impl<T: Clone + 'static> DelayedPagedModel<T> {
    /// `new DelayedPagedModel(model, timeout = 500)`.
    pub fn new(model: Rc<dyn IPagedModel<T>>, timeout: Option<u64>) -> Self {
        Self { model, timeout: timeout.unwrap_or(500) }
    }
}

impl<T: Clone + 'static> IPagedModel<T> for DelayedPagedModel<T> {
    fn length(&self) -> usize {
        self.model.length()
    }

    fn on_did_increment_length(&self) -> Event<usize> {
        self.model.on_did_increment_length()
    }

    fn is_resolved(&self, index: usize) -> bool {
        self.model.is_resolved(index)
    }

    fn get(&self, index: usize) -> T {
        self.model.get(index)
    }

    fn resolve(&self, index: usize, cancellation_token: CancellationToken) -> Promise<T> {
        let model = self.model.clone();
        let delay = self.timeout;
        Promise::with_executor(move |resolver| {
            if cancellation_token.is_cancellation_requested() {
                return resolver.reject(Error::cancellation());
            }

            // O `timeoutCancellation` é criado depois do timer e usado dentro dele.
            let timeout_cancellation: Rc<RefCell<Option<DisposableRef>>> = Rc::default();

            let (timer_cancellation, timer_resolver, timer_token) = (timeout_cancellation.clone(), resolver.clone(), cancellation_token.clone());
            let timer = runtime::set_timeout(
                move || {
                    if timer_token.is_cancellation_requested() {
                        return timer_resolver.reject(Error::cancellation());
                    }

                    if let Some(listener) = timer_cancellation.borrow_mut().take() {
                        listener.dispose();
                    }
                    model.resolve(index, timer_token).pipe_to(&timer_resolver);
                },
                delay,
            );

            let (listener_slot, listener_resolver) = (timeout_cancellation.clone(), resolver);
            let listener = cancellation_token.on_cancellation_requested().listen(move |_| {
                runtime::clear_timeout(timer);
                if let Some(listener) = listener_slot.borrow_mut().take() {
                    listener.dispose();
                }
                listener_resolver.reject(Error::cancellation());
            });
            *timeout_cancellation.borrow_mut() = Some(listener);
        })
    }
}

struct PagerState<T: Clone + 'static> {
    cached_pages: RefCell<Vec<Vec<T>>>,
    current_iterator: RefCell<Rc<dyn IPageIterator<T>>>,
    is_complete: Cell<bool>,
    pending_requests: RefCell<OrderedMap<usize, Promise<()>>>,
}

/// A PageIteratorPager wraps an IPageIterator to provide IPager functionality.
/// It caches pages as they are accessed and supports random page access by
/// sequentially loading pages until the requested page is reached.
pub struct PageIteratorPager<T: Clone + 'static> {
    state: Rc<PagerState<T>>,
    first_page: Vec<T>,
    page_size: usize,
    total: usize,
}

impl<T: Clone + 'static> PageIteratorPager<T> {
    pub fn new(initial_iterator: Rc<dyn IPageIterator<T>>) -> Self {
        let first_page = initial_iterator.elements();
        // Use first page size as page size
        let page_size = if first_page.is_empty() { 1 } else { first_page.len() };
        let total = initial_iterator.total();
        let state = Rc::new(PagerState {
            cached_pages: RefCell::new(vec![first_page.clone()]),
            is_complete: Cell::new(!initial_iterator.has_next_page()),
            current_iterator: RefCell::new(initial_iterator),
            pending_requests: RefCell::new(OrderedMap::new()),
        });
        Self { state, first_page, page_size, total }
    }
}

fn out_of_bounds(page_index: usize, state: &PagerState<impl Clone>) -> Error {
    Error::new(format!("Page {page_index} is out of bounds. Total pages: {}", state.cached_pages.borrow().len()))
}

impl<T: Clone + 'static> IPager<T> for PageIteratorPager<T> {
    fn first_page(&self) -> Vec<T> {
        self.first_page.clone()
    }

    fn total(&self) -> usize {
        self.total
    }

    fn page_size(&self) -> usize {
        self.page_size
    }

    fn get_page(&self, page_index: usize, cancellation_token: CancellationToken) -> Promise<Vec<T>> {
        let state = self.state.clone();
        async_fn(async move {
            if cancellation_token.is_cancellation_requested() {
                return Err(Error::cancellation());
            }

            // If we already have this page cached, return it
            if page_index < state.cached_pages.borrow().len() {
                return Ok(state.cached_pages.borrow()[page_index].clone());
            }

            // If we're complete and don't have this page, it doesn't exist
            if state.is_complete.get() {
                return Err(out_of_bounds(page_index, &state));
            }

            // Check if there's already a pending request that will load this index
            // (any pending request for an index >= our requested index)
            let pending = state.pending_requests.borrow().iter().find(|(pending_page_index, _)| **pending_page_index >= page_index).map(|(_, p)| p.clone());

            let promise = match pending {
                Some(promise) => promise,
                None => {
                    let promise = load_pages_until(state.clone(), page_index, cancellation_token);
                    state.pending_requests.borrow_mut().insert(page_index, promise.clone());
                    promise
                }
            };

            let result = async {
                promise.await?;
                if page_index >= state.cached_pages.borrow().len() {
                    return Err(out_of_bounds(page_index, &state));
                }
                Ok(state.cached_pages.borrow()[page_index].clone())
            }
            .await;
            state.pending_requests.borrow_mut().remove(&page_index);
            result
        })
    }
}

fn load_pages_until<T: Clone + 'static>(state: Rc<PagerState<T>>, target_page_index: usize, cancellation_token: CancellationToken) -> Promise<()> {
    async_fn(async move {
        while target_page_index >= state.cached_pages.borrow().len() && state.current_iterator.borrow().has_next_page() {
            if cancellation_token.is_cancellation_requested() {
                return Err(Error::cancellation());
            }

            let iterator = state.current_iterator.borrow().clone();
            let next = iterator.get_next_page(cancellation_token.clone()).await?;
            *state.current_iterator.borrow_mut() = next.clone();
            state.cached_pages.borrow_mut().push(next.elements());
        }
        if !state.current_iterator.borrow().has_next_page() {
            state.is_complete.set(true);
        }
        Ok(())
    })
}

struct IterativeState<T: Clone + 'static> {
    items: RefCell<Vec<T>>,
    has_next_page: Cell<bool>,
    on_did_increment_length: Rc<Emitter<usize>>,
    loading_promise: RefCell<Option<Promise<()>>>,
    pager: Rc<dyn IIterativePager<T>>,
}

impl<T: Clone + 'static> IterativeState<T> {
    /// Returns actual length + 1 if there are more pages (sentinel approach)
    fn length(&self) -> usize {
        self.items.borrow().len() + usize::from(self.has_next_page.get())
    }
}

pub struct IterativePagedModel<T: Clone + 'static> {
    state: Rc<IterativeState<T>>,
}

impl<T: Clone + 'static> IterativePagedModel<T> {
    pub fn new(pager: Rc<dyn IIterativePager<T>>) -> Self {
        let first_page = pager.first_page();
        Self {
            state: Rc::new(IterativeState {
                items: RefCell::new(first_page.items),
                has_next_page: Cell::new(first_page.has_more),
                on_did_increment_length: Emitter::new(),
                loading_promise: RefCell::new(None),
                pager,
            }),
        }
    }
}

async fn load_next_page<T: Clone + 'static>(state: Rc<IterativeState<T>>, cancellation_token: CancellationToken) -> Result<(), Error> {
    if !state.has_next_page.get() {
        return Ok(());
    }

    // If already loading, return the cached promise
    let loading = state.loading_promise.borrow().clone();
    if let Some(loading) = loading {
        return loading.await;
    }

    let page_promise = state.pager.get_next_page(cancellation_token);

    let handler_state = state.clone();
    let loading = page_promise.then_result(move |result| match result {
        Ok(page) => {
            handler_state.items.borrow_mut().extend(page.items);
            handler_state.has_next_page.set(page.has_more);

            // Clear the loading promise before firing the event
            // so that event handlers can trigger loading the next page if needed
            *handler_state.loading_promise.borrow_mut() = None;

            // Fire length update event
            handler_state.on_did_increment_length.fire(handler_state.length());
            Ok(())
        }
        Err(err) => {
            *handler_state.loading_promise.borrow_mut() = None;
            Err(err)
        }
    });
    *state.loading_promise.borrow_mut() = Some(loading.clone());

    loading.await
}

impl<T: Clone + 'static> IPagedModel<T> for IterativePagedModel<T> {
    fn on_did_increment_length(&self) -> Event<usize> {
        self.state.on_did_increment_length.event()
    }

    fn length(&self) -> usize {
        self.state.length()
    }

    /// Sentinel item is never resolved - it triggers loading
    fn is_resolved(&self, index: usize) -> bool {
        let len = self.state.items.borrow().len();
        if index == len && self.state.has_next_page.get() {
            return false; // This will trigger resolve() call
        }
        index < len
    }

    fn get(&self, index: usize) -> T {
        match self.state.items.borrow().get(index) {
            Some(item) => item.clone(),
            None => errors::throw(Error::new("Item not resolved yet")),
        }
    }

    /// When sentinel item is accessed, load next page
    fn resolve(&self, index: usize, cancellation_token: CancellationToken) -> Promise<T> {
        let state = self.state.clone();
        async_fn(async move {
            if cancellation_token.is_cancellation_requested() {
                return Err(Error::cancellation());
            }

            // If trying to resolve the sentinel item, load next page
            if index == state.items.borrow().len() && state.has_next_page.get() {
                load_next_page(state.clone(), cancellation_token).await?;
            }

            // After loading, the requested index should now be valid
            match state.items.borrow().get(index) {
                Some(item) => Ok(item.clone()),
                None => Err(Error::new("Index out of bounds")),
            }
        })
    }
}

impl<T: Clone + 'static> IDisposable for IterativePagedModel<T> {
    fn dispose(&self) {
        self.state.on_did_increment_length.dispose();
    }
}

struct MappedPager<T, R> {
    pager: Rc<dyn IPager<T>>,
    first_page: Vec<R>,
    fn_: Rc<dyn Fn(&T) -> R>,
}

impl<T: Clone + 'static, R: Clone + 'static> IPager<R> for MappedPager<T, R> {
    fn first_page(&self) -> Vec<R> {
        self.first_page.clone()
    }

    fn total(&self) -> usize {
        self.pager.total()
    }

    fn page_size(&self) -> usize {
        self.pager.page_size()
    }

    fn get_page(&self, page_index: usize, token: CancellationToken) -> Promise<Vec<R>> {
        let map = self.fn_.clone();
        self.pager.get_page(page_index, token).then(move |page| page.iter().map(|t| map(t)).collect())
    }
}

/// Similar to array.map, `mapPager` lets you map the elements of an
/// abstract paged collection to another type.
pub fn map_pager<T: Clone + 'static, R: Clone + 'static>(pager: Rc<dyn IPager<T>>, fn_: impl Fn(&T) -> R + 'static) -> Rc<dyn IPager<R>> {
    let fn_: Rc<dyn Fn(&T) -> R> = Rc::new(fn_);
    let first_page = pager.first_page().iter().map(|t| fn_(t)).collect();
    Rc::new(MappedPager { pager, first_page, fn_ })
}
