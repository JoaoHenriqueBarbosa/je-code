//! Porte de `src/vs/base/test/common/cache.test.ts` (suíte `Cache`), caso a caso e na mesma ordem.
//!
//! Pendente: os três casos do original (`simple value`, `simple error`, `should retry
//! cancellations`) testam `Cache<T>`, que recebe um `CancellationToken` e devolve uma `Promise`; ela
//! depende de `cancellation.rs` (em andamento) e de `timeout` de `async.ts` (ainda não existe). Por
//! isso `Cache` não está em `src/cache.rs`.
//!
//! Os casos `extra_*` abaixo NÃO são do original (que não testa as funções com cache): cobrem
//! `LRUCachedFunction`, `CachedFunction` e `WeakCachedFunction`, portadas junto.

use std::cell::Cell;
use std::rc::Rc;

use je_base::cache::{CachedFunction, LruCachedFunction, WeakCachedFunction};

#[test]
fn extra_lru_cached_function_keeps_only_the_last_key() {
    let calls = Rc::new(Cell::new(0));
    let c = calls.clone();
    let f = LruCachedFunction::new(move |n: i32| {
        c.set(c.get() + 1);
        n * 2
    });

    assert_eq!(f.get(2), 4);
    assert_eq!(f.get(2), 4);
    assert_eq!(calls.get(), 1);
    assert_eq!(f.get(3), 6);
    assert_eq!(f.get(2), 4);
    assert_eq!(calls.get(), 3);
}

#[test]
fn extra_cached_function_memoizes_by_cache_key() {
    let calls = Rc::new(Cell::new(0));
    let c = calls.clone();
    let f = CachedFunction::with_cache_key(
        |s: &String| s.to_lowercase(),
        move |s: String| {
            c.set(c.get() + 1);
            s.len()
        },
    );

    assert_eq!(f.get("Abc".to_string()), 3);
    assert_eq!(f.get("ABC".to_string()), 3);
    assert_eq!(f.get("abcd".to_string()), 4);
    assert_eq!(calls.get(), 2);
    assert_eq!(f.cached_values(), [("Abc".to_string(), 3), ("abcd".to_string(), 4)]);
}

#[test]
fn extra_weak_cached_function_forgets_dead_keys() {
    let calls = Rc::new(Cell::new(0));
    let c = calls.clone();
    let f = WeakCachedFunction::new(move |k: Rc<String>| {
        c.set(c.get() + 1);
        k.len()
    });

    let key = Rc::new("hello".to_string());
    assert_eq!(f.get(key.clone()), 5);
    assert_eq!(f.get(key.clone()), 5);
    assert_eq!(calls.get(), 1);

    // Outro `Rc` com o mesmo conteúdo é outra chave (identidade, não igualdade).
    assert_eq!(f.get(Rc::new("hello".to_string())), 5);
    assert_eq!(calls.get(), 2);
}
