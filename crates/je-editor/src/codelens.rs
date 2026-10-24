//! Porte de `src/vs/editor/contrib/codelens/browser/codelens.ts`: a coleta das lentes de código dos provedores
//! (`getCodeLensModel`) e o modelo que as guarda (`CodeLensModel`, `CodeLensItem`).
//!
//! Decisões de tradução:
//! - `CodeLensModel` é um [`Rc`] devolvido pela promessa: `lenses` é preenchido (e ordenado) antes de o modelo ser
//!   compartilhado, e o `_store` preguiçoso (`this._store ??= new DisposableStore()`) é um `RefCell` que o
//!   [`CodeLensModel::dispose`] esvazia. `CodeLensModel.Empty` (instância estática, que o `dispose` do original deixa
//!   intacta por não ter `_store`) é [`CodeLensModel::empty`], um modelo novo e vazio a cada chamada.
//! - `isDisposable(list)` é sempre verdadeiro: [`CodeLensList`] implementa [`IDisposable`].
//! - A classificação `result.lenses.sort(...)` do original é estável, como o `sort_by` do Rust.
//! - `await Promise.resolve(provider.provideCodeLenses(model, token))` dentro do `try` é o `await` da promessa que o
//!   trait devolve: a rejeição (inclusive a exceção síncrona do provedor, que o RPC já entrega como rejeição) vai para
//!   `onUnexpectedExternalError`.
//! - O comando `_executeCodeLensProvider` (`CommandsRegistry.registerCommand`) lê `ILanguageFeaturesService` e
//!   `IModelService` de um `ServicesAccessor`, que o porte não tem: fica de fora.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::errors::on_unexpected_external_error;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::promise::Promise;

use crate::language_feature_registry::LanguageFeatureRegistry;
use crate::language_features_service::{CodeLens, CodeLensList, CodeLensProvider};
use crate::text_model::TextModel;

/// `CodeLensItem`.
#[derive(Clone)]
pub struct CodeLensItem {
    pub symbol: CodeLens,
    pub provider: Rc<dyn CodeLensProvider>,
}

/// `CodeLensModel`.
#[derive(Default)]
pub struct CodeLensModel {
    pub lenses: Vec<CodeLensItem>,
    store: RefCell<Option<Rc<DisposableStore>>>,
}

impl CodeLensModel {
    /// `CodeLensModel.Empty`: aqui um modelo novo e vazio a cada chamada.
    pub fn empty() -> Self {
        Self::default()
    }

    /// `isDisposed`.
    pub fn is_disposed(&self) -> bool {
        self.store.borrow().as_ref().is_some_and(|store| store.is_disposed())
    }

    /// `add(list, provider)`.
    pub fn add(&mut self, list: &Rc<CodeLensList>, provider: &Rc<dyn CodeLensProvider>) {
        let disposable: DisposableRef = Rc::clone(list) as DisposableRef;
        self.store.borrow_mut().get_or_insert_with(DisposableStore::new).add(disposable);
        for symbol in &list.lenses {
            self.lenses.push(CodeLensItem { symbol: symbol.clone(), provider: Rc::clone(provider) });
        }
    }
}

impl IDisposable for CodeLensModel {
    fn dispose(&self) {
        let store = self.store.borrow().clone();
        if let Some(store) = store {
            store.dispose();
        }
    }
}

/// `getCodeLensModel(registry, model, token)`.
pub fn get_code_lens_model(registry: &LanguageFeatureRegistry<dyn CodeLensProvider>, model: &Rc<TextModel>, token: &CancellationToken) -> Promise<Rc<CodeLensModel>> {
    let providers = registry.ordered(model, false);
    let result: Rc<RefCell<CodeLensModel>> = Rc::default();
    let (model, token) = (Rc::clone(model), token.clone());
    let ranked_providers = providers.clone();

    async_fn(async move {
        let promises: Vec<Promise<()>> = providers
            .iter()
            .map(|provider| {
                let (provider, model, token, result) = (Rc::clone(provider), Rc::clone(&model), token.clone(), Rc::clone(&result));
                async_fn(async move {
                    match provider.provide_code_lenses(&model, &token).await {
                        Ok(Some(list)) => result.borrow_mut().add(&list, &provider),
                        Ok(None) => {}
                        Err(err) => on_unexpected_external_error(&err),
                    }
                    Ok(())
                })
            })
            .collect();

        Promise::all(promises).await?;

        let mut result = result.take();
        if token.is_cancellation_requested() {
            result.dispose();
            return Ok(Rc::new(CodeLensModel::empty()));
        }

        let rank = |provider: &Rc<dyn CodeLensProvider>| ranked_providers.iter().position(|candidate| Rc::ptr_eq(candidate, provider));
        result.lenses.sort_by(|a, b| {
            // sort by lineNumber, provider-rank, and column
            a.symbol
                .range
                .start_line_number
                .cmp(&b.symbol.range.start_line_number)
                .then_with(|| rank(&a.provider).cmp(&rank(&b.provider)))
                .then_with(|| a.symbol.range.start_column.cmp(&b.symbol.range.start_column))
        });
        Ok(Rc::new(result))
    })
}
