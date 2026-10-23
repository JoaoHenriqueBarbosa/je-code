//! Porte de `src/vs/base/common/cache.ts`, só a parte síncrona.
//!
//! Pendente: `Cache<T>` e `CacheResult<T>` (a tarefa recebe um `CancellationToken` e devolve uma
//! `Promise`; dependem de `cancellation.rs`, em andamento, e de `async.ts`, que ainda não existe).
//!
//! Representação das três funções com cache:
//! - A chave padrão é a própria entrada (`identity`); com `getCacheKey`, a chave calculada. O
//!   `unknown` do original vira o tipo `K` (com `Hash + Eq`, que é a igualdade estrita do `Map`
//!   para chaves de valor). Sem `getCacheKey`, `K = TArg` e a chave é um clone da entrada.
//! - O cache usa mutabilidade interior (`&self`), porque a função memoizada pode se chamar de
//!   volta (recursão) enquanto o cache está sendo preenchido; o executor roda fora de qualquer
//!   empréstimo.
//! - `WeakCachedFunction` guarda a entrada por identidade do `Rc` que serve de chave e esquece a
//!   entrada quando a chave morre (o `WeakMap` do original); entradas mortas saem na próxima
//!   chamada a `get`.
//! - `CachedFunction::cached_values` é o `Map<TArg, TComputed>` como lista de pares na ordem de
//!   inserção.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;
use std::rc::{Rc, Weak};

/// `identity`.
pub fn identity<T>(t: T) -> T {
    t
}

/// Cache LRU de tamanho 1: guarda só a última chave e o último valor.
pub struct LruCachedFunction<TArg, TComputed, K = TArg> {
    last_cache: RefCell<Option<TComputed>>,
    last_arg_key: RefCell<Option<K>>,
    fn_: Box<dyn Fn(TArg) -> TComputed>,
    compute_key: Box<dyn Fn(&TArg) -> K>,
}

impl<TArg: Clone + PartialEq + 'static, TComputed: Clone> LruCachedFunction<TArg, TComputed, TArg> {
    /// A chave é a própria entrada.
    pub fn new(fn_: impl Fn(TArg) -> TComputed + 'static) -> Self {
        Self::with_cache_key(TArg::clone, fn_)
    }
}

impl<TArg, TComputed: Clone, K: PartialEq> LruCachedFunction<TArg, TComputed, K> {
    /// `new LRUCachedFunction({ getCacheKey }, fn)`.
    pub fn with_cache_key(get_cache_key: impl Fn(&TArg) -> K + 'static, fn_: impl Fn(TArg) -> TComputed + 'static) -> Self {
        Self { last_cache: RefCell::new(None), last_arg_key: RefCell::new(None), fn_: Box::new(fn_), compute_key: Box::new(get_cache_key) }
    }

    pub fn get(&self, arg: TArg) -> TComputed {
        let key = (self.compute_key)(&arg);
        if self.last_arg_key.borrow().as_ref() != Some(&key) {
            *self.last_arg_key.borrow_mut() = Some(key);
            let computed = (self.fn_)(arg);
            *self.last_cache.borrow_mut() = Some(computed);
        }
        self.last_cache.borrow().clone().expect("valor calculado")
    }
}

/// Cache sem limite que memoriza o resultado da função.
pub struct CachedFunction<TArg, TComputed, K = TArg> {
    map: RefCell<Vec<(TArg, TComputed)>>,
    map2: RefCell<HashMap<K, TComputed>>,
    fn_: Box<dyn Fn(TArg) -> TComputed>,
    compute_key: Box<dyn Fn(&TArg) -> K>,
}

impl<TArg: Clone + 'static, TComputed: Clone> CachedFunction<TArg, TComputed, TArg>
where
    TArg: Hash + Eq,
{
    /// A chave é a própria entrada.
    pub fn new(fn_: impl Fn(TArg) -> TComputed + 'static) -> Self {
        Self::with_cache_key(TArg::clone, fn_)
    }
}

impl<TArg: Clone, TComputed: Clone, K: Hash + Eq> CachedFunction<TArg, TComputed, K> {
    /// `new CachedFunction({ getCacheKey }, fn)`.
    pub fn with_cache_key(get_cache_key: impl Fn(&TArg) -> K + 'static, fn_: impl Fn(TArg) -> TComputed + 'static) -> Self {
        Self { map: RefCell::new(Vec::new()), map2: RefCell::new(HashMap::new()), fn_: Box::new(fn_), compute_key: Box::new(get_cache_key) }
    }

    /// Os pares `(entrada, valor)` já calculados, na ordem de inserção.
    pub fn cached_values(&self) -> Vec<(TArg, TComputed)> {
        self.map.borrow().clone()
    }

    pub fn get(&self, arg: TArg) -> TComputed {
        let key = (self.compute_key)(&arg);
        if let Some(value) = self.map2.borrow().get(&key) {
            return value.clone();
        }
        let value = (self.fn_)(arg.clone());
        self.map.borrow_mut().push((arg, value.clone()));
        self.map2.borrow_mut().insert(key, value.clone());
        value
    }
}

/// Cache sem limite cuja chave é um objeto (`Rc`) e que esquece a entrada quando a chave morre.
pub struct WeakCachedFunction<TArg, TComputed, K> {
    map: RefCell<HashMap<usize, (Weak<K>, TComputed)>>,
    fn_: Box<dyn Fn(TArg) -> TComputed>,
    compute_key: ObjectKeyFn<TArg, K>,
}

type ObjectKeyFn<TArg, K> = Box<dyn Fn(&TArg) -> Rc<K>>;

impl<K: 'static, TComputed: Clone> WeakCachedFunction<Rc<K>, TComputed, K> {
    /// A chave é a própria entrada.
    pub fn new(fn_: impl Fn(Rc<K>) -> TComputed + 'static) -> Self {
        Self::with_cache_key(Rc::clone, fn_)
    }
}

impl<TArg, TComputed: Clone, K> WeakCachedFunction<TArg, TComputed, K> {
    /// `new WeakCachedFunction({ getCacheKey }, fn)`: `get_cache_key` devolve o objeto que serve de chave.
    pub fn with_cache_key(get_cache_key: impl Fn(&TArg) -> Rc<K> + 'static, fn_: impl Fn(TArg) -> TComputed + 'static) -> Self {
        Self { map: RefCell::new(HashMap::new()), fn_: Box::new(fn_), compute_key: Box::new(get_cache_key) }
    }

    pub fn get(&self, arg: TArg) -> TComputed {
        let key = (self.compute_key)(&arg);
        let address = Rc::as_ptr(&key) as *const u8 as usize;
        {
            let mut map = self.map.borrow_mut();
            map.retain(|_, (weak, _)| weak.strong_count() > 0);
            if let Some((_, value)) = map.get(&address) {
                return value.clone();
            }
        }
        let value = (self.fn_)(arg);
        self.map.borrow_mut().insert(address, (Rc::downgrade(&key), value.clone()));
        value
    }
}
