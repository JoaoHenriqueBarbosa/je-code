//! Porte de `src/vs/workbench/services/extensions/common/lazyPromise.ts`: `LazyPromise` e `CanceledLazyPromise`.
//!
//! A `LazyPromise` só cria a promessa de verdade quando alguém pede ([`LazyPromise::promise`], o
//! `_ensureActual` que `then`, `catch` e `finally` chamam no original). Rejeitar uma `LazyPromise` que
//! ninguém escuta é seguro de ignorar, mas o erro vai para o tratador de erros inesperados.
//!
//! Divergências:
//! - `then`, `catch` e `finally` do original viram [`LazyPromise::promise`], que devolve a [`Promise`] real
//!   (o `Promise<any>` do JavaScript é a [`Promise<Dynamic>`]); quem encadeia usa os métodos dela.
//! - O valor e o erro de `resolveOk`/`resolveErr` são `any` no original. O valor é um [`Dynamic`] e o erro é
//!   um [`Error`]; o que o JavaScript rejeita sem ser um `Error` (um objeto qualquer, `undefined`) vai
//!   codificado num `Error` (ver `rpc_protocol::rejection_with_value`).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::promise::{Promise, Resolver};

struct LazyState {
    actual: Option<(Promise<Dynamic>, Resolver<Dynamic>)>,
    outcome: Option<Result<Dynamic, Error>>,
}

/// `LazyPromise`. Clonar compartilha o mesmo estado, como duas referências ao mesmo objeto.
#[derive(Clone)]
pub struct LazyPromise {
    state: Rc<RefCell<LazyState>>,
}

impl Default for LazyPromise {
    fn default() -> Self {
        Self::new()
    }
}

impl LazyPromise {
    pub fn new() -> Self {
        Self { state: Rc::new(RefCell::new(LazyState { actual: None, outcome: None })) }
    }

    /// `new CanceledLazyPromise()`: já nasce rejeitada com um `CancellationError`, sem avisar o tratador de erros
    /// inesperados.
    pub fn canceled() -> Self {
        let this = Self::new();
        this.state.borrow_mut().outcome = Some(Err(Error::cancellation()));
        this
    }

    /// Uma `LazyPromise` cuja promessa de verdade já existe (o `Promise.reject(...)` que `_remoteCall` devolve no
    /// lugar dela quando o token já nasce cancelado).
    pub fn from_promise(promise: Promise<Dynamic>) -> Self {
        let this = Self::new();
        // O resolvedor da cópia nunca é usado: a promessa de verdade é a dada.
        let (_unused, resolver) = Promise::new();
        this.state.borrow_mut().actual = Some((promise, resolver));
        this
    }

    /// `_ensureActual()`: a promessa de verdade, criada na primeira chamada.
    pub fn promise(&self) -> Promise<Dynamic> {
        let mut state = self.state.borrow_mut();
        if let Some((promise, _)) = &state.actual {
            return promise.clone();
        }
        let (promise, resolver) = Promise::new();
        match state.outcome.clone() {
            Some(Ok(value)) => resolver.resolve(value),
            Some(Err(error)) => resolver.reject(error),
            None => {}
        }
        state.actual = Some((promise.clone(), resolver));
        promise
    }

    pub fn resolve_ok(&self, value: Dynamic) {
        let resolver = {
            let mut state = self.state.borrow_mut();
            if state.outcome.is_some() {
                return;
            }
            state.outcome = Some(Ok(value.clone()));
            state.actual.as_ref().map(|(_, resolver)| resolver.clone())
        };
        if let Some(resolver) = resolver {
            resolver.resolve(value);
        }
    }

    pub fn resolve_err(&self, error: Error) {
        let resolver = {
            let mut state = self.state.borrow_mut();
            if state.outcome.is_some() {
                return;
            }
            state.outcome = Some(Err(error.clone()));
            state.actual.as_ref().map(|(_, resolver)| resolver.clone())
        };
        match resolver {
            Some(resolver) => resolver.reject(error),
            // Se ninguém está escutando agora, dá para assumir que nunca vão, já que resolver esta promessa é sempre assíncrono.
            None => errors::on_unexpected_error(&error),
        }
    }
}
