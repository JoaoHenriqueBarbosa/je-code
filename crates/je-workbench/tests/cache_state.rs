//! Porte de `src/vs/workbench/contrib/search/test/common/cacheState.test.ts` (suíte `FileQueryCacheState`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - **`ensureNoDisposablesAreLeakedInTestSuite`.** É o [`LeakGuard`] criado no começo de cada caso.
//! - **`await`.** Cada `await` do original sobre uma promessa já resolvida (ou sobre `DeferredPromise#complete`) esvazia as
//!   microtarefas (`runtime::run_microtasks`), o que deixa os trechos assíncronos do estado (`load` e `dispose`) rodarem antes
//!   da próxima conferência; `await cache.awaitDisposal(n)` esvazia as microtarefas e confere que a promessa resolveu (no
//!   original o teste ficaria pendurado se ela não resolvesse).
//! - **`DeferredPromise<any>`.** O valor das promessas de carga nunca é lido, então são `DeferredPromise<()>`, e
//!   `complete(null)` é `complete(())`. `error('loading failed')` rejeita com um `Error` de mesmo texto.
//! - **`errors.setUnexpectedErrorHandler(() => null)`.** O tratador original de antes (`getUnexpectedErrorHandler`) é o do
//!   guarda de vazamentos, restaurado com `record_unexpected_errors`.
//! - **`Object.assign({ cacheKey }, this.baseQuery)`.** É a cópia da consulta base com o `cacheKey` preenchido.
//! - **`cache.loading[key]`, `cache.disposing[key]`** são mapas por chave; `_awaitDisposal` guarda os resolvedores por contagem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, record_unexpected_errors};
use je_base::errors::{self, Error};
use je_base::glob::Expression;
use je_base::promise::{DeferredPromise, Promise, Resolver};
use je_base::runtime;
use je_workbench::contrib::search::common::cache_state::FileQueryCacheState;
use je_workbench::services::search::search::{IFileQuery, IFileQueryProps};

struct MockCache {
    cache_keys: RefCell<Vec<String>>,
    loading: RefCell<HashMap<String, DeferredPromise<()>>>,
    disposing: RefCell<HashMap<String, DeferredPromise<()>>>,
    await_disposal: RefCell<HashMap<usize, Vec<Resolver<()>>>>,
    base_query: RefCell<IFileQuery>,
}

impl MockCache {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            cache_keys: RefCell::new(Vec::new()),
            loading: RefCell::new(HashMap::new()),
            disposing: RefCell::new(HashMap::new()),
            await_disposal: RefCell::new(HashMap::new()),
            base_query: RefCell::new(IFileQueryProps::new(Vec::new())),
        })
    }

    fn query(&self, cache_key: &str) -> IFileQuery {
        self.cache_keys.borrow_mut().push(cache_key.to_string());
        IFileQuery { cache_key: Some(cache_key.to_string()), ..self.base_query.borrow().clone() }
    }

    fn load(&self, query: &IFileQuery) -> Promise<()> {
        let promise = DeferredPromise::<()>::new();
        let result = promise.promise.clone();
        self.loading.borrow_mut().insert(query.cache_key.clone().expect("a consulta do cache tem chave"), promise);
        result
    }

    fn dispose(&self, cache_key: &str) -> Promise<()> {
        let promise = DeferredPromise::<()>::new();
        let result = promise.promise.clone();
        self.disposing.borrow_mut().insert(cache_key.to_string(), promise);
        let n = self.disposing.borrow().len();
        let done = self.await_disposal.borrow_mut().remove(&n).unwrap_or_default();
        for resolver in done {
            resolver.resolve(());
        }
        result
    }

    /// `awaitDisposal(n)`: a promessa que resolve quando houver `n` chaves descartadas.
    fn await_disposal(&self, n: usize) -> Promise<()> {
        let (promise, resolver) = Promise::<()>::new();
        if n == self.disposing.borrow().len() {
            resolver.resolve(());
        } else {
            self.await_disposal.borrow_mut().entry(n).or_default().push(resolver);
        }
        promise
    }

    /// `cache.loading[cacheKey].complete(null)` seguido do `await`.
    fn complete_loading(&self, cache_key: &str) {
        let promise = self.loading.borrow().get(cache_key).map(|promise| promise.complete(())).expect("a carga está registrada");
        drop(promise);
        runtime::run_microtasks();
    }

    /// `cache.loading[cacheKey].error('loading failed')` seguido do `await`.
    fn fail_loading(&self, cache_key: &str) {
        let promise = self.loading.borrow().get(cache_key).map(|promise| promise.error(Error::new("loading failed"))).expect("a carga está registrada");
        drop(promise);
        runtime::run_microtasks();
    }

    /// `await cache.awaitDisposal(n)`.
    fn wait_for_disposal(&self, n: usize) {
        let promise = self.await_disposal(n);
        runtime::run_microtasks();
        assert!(promise.is_settled(), "o descarte {n} aconteceu");
    }
}

fn create_cache_state(cache: &Rc<MockCache>, previous: Option<Rc<FileQueryCacheState>>) -> Rc<FileQueryCacheState> {
    let (query_cache, load_cache, dispose_cache) = (cache.clone(), cache.clone(), cache.clone());
    FileQueryCacheState::new(
        Rc::new(move |cache_key| query_cache.query(cache_key)),
        Rc::new(move |query| load_cache.load(query)),
        Rc::new(move |cache_key| dispose_cache.dispose(cache_key)),
        previous,
    )
}

#[test]
fn reuse_old_cache_key_until_new_cache_is_loaded() {
    let _guard: LeakGuard = ensure_no_disposables_are_leaked_in_test_suite();

    let cache = MockCache::new();

    let first = create_cache_state(&cache, None);
    let first_key = first.cache_key();
    assert!(!first.is_loaded());
    assert!(!first.is_updating());

    first.load();
    assert!(!first.is_loaded());
    assert!(first.is_updating());

    cache.complete_loading(&first_key);
    assert!(first.is_loaded());
    assert!(!first.is_updating());

    let second = create_cache_state(&cache, Some(first.clone()));
    second.load();
    assert!(second.is_loaded());
    assert!(second.is_updating());
    cache.wait_for_disposal(0);
    assert_eq!(second.cache_key(), first_key); // still using old cacheKey

    let second_key = cache.cache_keys.borrow()[1].clone();
    cache.complete_loading(&second_key);
    assert!(second.is_loaded());
    assert!(!second.is_updating());
    cache.wait_for_disposal(1);
    assert_eq!(second.cache_key(), second_key);
}

#[test]
fn do_not_spawn_additional_load_if_previous_is_still_loading() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let cache = MockCache::new();

    let first = create_cache_state(&cache, None);
    let first_key = first.cache_key();
    first.load();
    assert!(!first.is_loaded());
    assert!(first.is_updating());
    assert_eq!(cache.loading.borrow().len(), 1);

    let second = create_cache_state(&cache, Some(first.clone()));
    second.load();
    assert!(!second.is_loaded());
    assert!(second.is_updating());
    assert_eq!(cache.cache_keys.borrow().len(), 2);
    assert_eq!(cache.loading.borrow().len(), 1); // still only one loading
    assert_eq!(second.cache_key(), first_key);

    cache.complete_loading(&first_key);
    assert!(second.is_loaded());
    assert!(!second.is_updating());
    cache.wait_for_disposal(0);
}

#[test]
fn do_not_use_previous_cache_key_if_query_changed() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let cache = MockCache::new();

    let first = create_cache_state(&cache, None);
    let first_key = first.cache_key();
    first.load();
    cache.complete_loading(&first_key);
    assert!(first.is_loaded());
    assert!(!first.is_updating());
    cache.wait_for_disposal(0);

    cache.base_query.borrow_mut().common.exclude_pattern = Some(Expression::from_entries([("**/node_modules", true)]));
    let second = create_cache_state(&cache, Some(first.clone()));
    assert!(!second.is_loaded());
    assert!(!second.is_updating());
    cache.wait_for_disposal(1);

    second.load();
    assert!(!second.is_loaded());
    assert!(second.is_updating());
    assert_ne!(second.cache_key(), first_key); // not using old cacheKey
    let second_key = cache.cache_keys.borrow()[1].clone();
    assert_eq!(second.cache_key(), second_key);

    cache.complete_loading(&second_key);
    assert!(second.is_loaded());
    assert!(!second.is_updating());
    cache.wait_for_disposal(1);
}

#[test]
fn dispose_propagates() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let cache = MockCache::new();

    let first = create_cache_state(&cache, None);
    let first_key = first.cache_key();
    first.load();
    cache.complete_loading(&first_key);
    let second = create_cache_state(&cache, Some(first.clone()));
    assert!(second.is_loaded());
    assert!(!second.is_updating());
    cache.wait_for_disposal(0);

    second.dispose();
    assert!(!second.is_loaded());
    assert!(!second.is_updating());
    cache.wait_for_disposal(1);
    assert!(cache.disposing.borrow().contains_key(&first_key));
}

#[test]
fn keep_using_old_cache_key_when_loading_fails() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let cache = MockCache::new();

    let first = create_cache_state(&cache, None);
    let first_key = first.cache_key();
    first.load();
    cache.complete_loading(&first_key);

    let second = create_cache_state(&cache, Some(first.clone()));
    second.load();
    let second_key = cache.cache_keys.borrow()[1].clone();
    errors::set_unexpected_error_handler(|_| {});
    cache.fail_loading(&second_key);
    record_unexpected_errors();
    assert!(second.is_loaded());
    assert!(!second.is_updating());
    assert_eq!(cache.loading.borrow().len(), 2);
    cache.wait_for_disposal(0);
    assert_eq!(second.cache_key(), first_key); // keep using old cacheKey

    let third = create_cache_state(&cache, Some(second.clone()));
    third.load();
    assert!(third.is_loaded());
    assert!(third.is_updating());
    assert_eq!(cache.loading.borrow().len(), 3);
    cache.wait_for_disposal(0);
    assert_eq!(third.cache_key(), first_key);

    let third_key = cache.cache_keys.borrow()[2].clone();
    cache.complete_loading(&third_key);
    assert!(third.is_loaded());
    assert!(!third.is_updating());
    assert_eq!(cache.loading.borrow().len(), 3);
    cache.wait_for_disposal(2);
    assert_eq!(third.cache_key(), third_key); // recover with next successful load
}
