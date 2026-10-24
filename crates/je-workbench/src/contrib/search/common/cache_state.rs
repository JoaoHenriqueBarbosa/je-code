//! Porte de `src/vs/workbench/contrib/search/common/cacheState.ts`: o estado de um cache de consulta de arquivos
//! (`FileQueryCacheState`), que reaproveita a chave do cache anterior enquanto o novo carrega.
//!
//! Notas de tradução:
//! - **Funções do construtor.** `cacheQuery`, `loadFn` e `disposeFn` são os tipos [`CacheQueryFn`], [`LoadFn`] e [`DisposeFn`];
//!   `loadFn` devolve `Promise<unknown>`, que aqui é `Promise<()>` (o valor nunca é lido).
//! - **Estado compartilhado.** O estado é um `Rc`: `load` devolve `this` e o estado anterior é encadeado por referência. Os
//!   trechos assíncronos (`load` e `dispose` são IIFEs `async` no original) seguram o estado por referência fraca, para a
//!   promessa guardada em `loadPromise` não formar um ciclo com ele.
//! - **`Object.assign({}, query, { cacheKey: null })` e `equals`.** São a comparação das consultas (`PartialEq`) depois de
//!   limpar o `cacheKey` das duas.
//! - **`loadPromise` rejeitada.** O `try/catch` do `load` relança o erro, como o `then_result` devolvendo o mesmo `Err`; a
//!   rejeição sem tratador é reportada como erro inesperado, como no original.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::id_generator::default_generator_next_id;
use je_base::promise::Promise;

use crate::services::search::search::IFileQuery;

/// O `cacheQuery` do construtor.
pub type CacheQueryFn = Rc<dyn Fn(&str) -> IFileQuery>;
/// O `loadFn` do construtor.
pub type LoadFn = Rc<dyn Fn(&IFileQuery) -> Promise<()>>;
/// O `disposeFn` do construtor.
pub type DisposeFn = Rc<dyn Fn(&str) -> Promise<()>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoadingPhase {
    Created = 1,
    Loading = 2,
    Loaded = 3,
    Errored = 4,
    Disposed = 5,
}

/// `FileQueryCacheState`.
pub struct FileQueryCacheState {
    this: Weak<FileQueryCacheState>,
    cache_key: String,
    query: IFileQuery,
    loading_phase: Cell<LoadingPhase>,
    load_promise: RefCell<Option<Promise<()>>>,
    load_fn: LoadFn,
    dispose_fn: DisposeFn,
    previous_cache_state: RefCell<Option<Rc<FileQueryCacheState>>>,
}

impl FileQueryCacheState {
    pub fn new(cache_query: CacheQueryFn, load_fn: LoadFn, dispose_fn: DisposeFn, previous_cache_state: Option<Rc<FileQueryCacheState>>) -> Rc<Self> {
        let cache_key = default_generator_next_id();
        let query = cache_query(&cache_key);
        let state = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            cache_key,
            query,
            loading_phase: Cell::new(LoadingPhase::Created),
            load_promise: RefCell::new(None),
            load_fn,
            dispose_fn,
            previous_cache_state: RefCell::new(previous_cache_state),
        });
        let previous = state.previous_cache_state.borrow().clone();
        if let Some(previous) = previous {
            let current = IFileQuery { cache_key: None, ..state.query.clone() };
            let previous_query = IFileQuery { cache_key: None, ..previous.query.clone() };
            if current != previous_query {
                previous.dispose();
                *state.previous_cache_state.borrow_mut() = None;
            }
        }
        state
    }

    pub fn cache_key(&self) -> String {
        let previous = self.previous_cache_state.borrow().clone();
        match previous {
            Some(previous) if self.loading_phase.get() != LoadingPhase::Loaded => previous.cache_key(),
            _ => self.cache_key.clone(),
        }
    }

    pub fn is_loaded(&self) -> bool {
        let is_loaded = self.loading_phase.get() == LoadingPhase::Loaded;

        let previous = self.previous_cache_state.borrow().clone();
        match previous {
            Some(previous) if !is_loaded => previous.is_loaded(),
            _ => is_loaded,
        }
    }

    pub fn is_updating(&self) -> bool {
        let is_updating = self.loading_phase.get() == LoadingPhase::Loading;

        let previous = self.previous_cache_state.borrow().clone();
        match previous {
            Some(previous) if !is_updating => previous.is_updating(),
            _ => is_updating,
        }
    }

    pub fn load(&self) -> Rc<FileQueryCacheState> {
        let this = self.this.upgrade().expect("o estado está vivo enquanto é usado");
        if self.is_updating() {
            return this;
        }

        self.loading_phase.set(LoadingPhase::Loading);

        let state = self.this.clone();
        let load_promise = (self.load_fn)(&self.query).then_result(move |result| {
            let Some(state) = state.upgrade() else {
                return result;
            };
            match result {
                Ok(_) => {
                    state.loading_phase.set(LoadingPhase::Loaded);

                    let previous = state.previous_cache_state.borrow_mut().take();
                    if let Some(previous) = previous {
                        previous.dispose();
                    }
                    Ok(())
                }
                Err(error) => {
                    state.loading_phase.set(LoadingPhase::Errored);

                    Err(error)
                }
            }
        });
        *self.load_promise.borrow_mut() = Some(load_promise);

        this
    }

    pub fn dispose(&self) {
        let load_promise = self.load_promise.borrow().clone();
        if let Some(load_promise) = load_promise {
            let state = self.this.clone();
            // `await this.loadPromise` com o erro ignorado; o `disposeFn` não é esperado.
            let _ = load_promise.then_result(move |_| {
                if let Some(state) = state.upgrade() {
                    state.loading_phase.set(LoadingPhase::Disposed);
                    let _ = (state.dispose_fn)(&state.cache_key);
                }
                Ok(())
            });
        } else {
            self.loading_phase.set(LoadingPhase::Disposed);
        }

        let previous = self.previous_cache_state.borrow_mut().take();
        if let Some(previous) = previous {
            previous.dispose();
        }
    }
}
