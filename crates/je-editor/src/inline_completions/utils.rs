//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/utils.ts`: o `ErrorResult`, o `getReadonlyEmptyArray`, o `wait`,
//! as contas de posição (`addPositions`, `subtractPositions`, `substringPos`), o `convertItemsToStableObservables` e o
//! `ObservableContextKeyService`. Os auxiliares que dependem do modelo de edição (`getEndPositionsAfterApplying`,
//! `getModifiedRangesAfterApplying` e `removeTextReplacementCommonSuffixPrefix`) vivem em `inline_completions_model`.
//!
//! Notas de tradução:
//! - `ErrorResult<T = void>` guarda `error: T` e `message?`; `ErrorResult.is(obj)` é a escolha de variante de um
//!   `Result<_, ErrorResult<T>>`, então não há função `is`;
//! - `getReadonlyEmptyArray` devolve uma fatia vazia compartilhada, que em Rust é `&[]`;
//! - `wait(ms, token?)` é uma [`Promise`] que resolve no tempo ou no cancelamento do token (nunca rejeita);
//! - `readonly T[]` observável é `Rc<Vec<T>>`: o `convertItemsToStableObservables` exige `StrictEquals` do item, que é o
//!   `===` das células internas;
//! - `ObservableContextKeyService.bind` tem duas sobrecargas (observável ou função do leitor): são [`ObservableContextKeyService::bind`]
//!   (função) e [`ObservableContextKeyService::bind_observable`] (observável, convertido para [`Dynamic`] por `Into`). O
//!   `ContextKeyValue` do porte é o [`Dynamic`] (a `RawContextKey` não é genérica);
//! - [`dispose_handle`] não existe no original: o `Rc<dyn Trait>` não vira `Rc<dyn IDisposable>` sem esse laço de conversão,
//!   e todo colaborador injetado que o `this._register(...)` do original descartaria passa por ele.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::JsString;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, plain_disposable};
use je_base::observable::base::{IObservable, IReader, Observable};
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::{SettableExt, StrictEquals, autorun, observable_value, transaction};
use je_base::promise::Promise;
use je_base::runtime;
use je_platform::contextkey::{IContextKeyService, RawContextKey};
use je_platform::observable::platform_observable_utils::bind_context_key;

use crate::position::Position;
use crate::position_to_offset::{PositionOffsetTransformer, PositionOffsetTransformerBase};

/// `ErrorResult<T>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorResult<T = ()> {
    pub error: T,
    pub message: Option<String>,
}

impl ErrorResult<()> {
    /// `ErrorResult.message(message)`.
    pub fn message(message: impl Into<String>) -> Self {
        Self { error: (), message: Some(message.into()) }
    }
}

impl<T> ErrorResult<T> {
    pub fn new(error: T, message: Option<String>) -> Self {
        Self { error, message }
    }
}

impl<T: std::fmt::Debug> ErrorResult<T> {
    /// `logError()`.
    pub fn log_error(&self) {
        match self.message.as_deref().filter(|message| !message.is_empty()) {
            Some(message) => eprintln!("ErrorResult: {message} {:?}", self.error),
            None => eprintln!(
                "ErrorResult: An unexpected error-case occurred, usually caused by invalid input. {:?}",
                self.error
            ),
        }
    }
}

/// `getReadonlyEmptyArray()`.
pub fn get_readonly_empty_array<T>() -> &'static [T] {
    &[]
}

/// `addPositions(pos1, pos2)`.
pub fn add_positions(pos1: Position, pos2: Position) -> Position {
    Position::new(
        pos1.line_number + pos2.line_number - 1,
        if pos2.line_number == 1 { pos1.column + pos2.column - 1 } else { pos2.column },
    )
}

/// `subtractPositions(pos1, pos2)`.
pub fn subtract_positions(pos1: Position, pos2: Position) -> Position {
    Position::new(
        pos1.line_number - pos2.line_number + 1,
        if pos1.line_number - pos2.line_number == 0 { pos1.column - pos2.column + 1 } else { pos1.column },
    )
}

/// `substringPos(text, pos)`: o texto a partir da posição (o `substring` do JavaScript limita o deslocamento ao tamanho).
pub fn substring_pos(text: &JsString, pos: Position) -> JsString {
    let transformer = PositionOffsetTransformer::new(&text.to_string_lossy());
    let offset = usize::try_from(transformer.get_offset(pos)).unwrap_or(0).min(text.units().len());
    JsString::from(&text.units()[offset..])
}

/// `convertItemsToStableObservables(items, store)`: um observável por posição da lista, que muda de valor (e não de
/// identidade) quando o item daquela posição muda. A lista externa só troca quando o tamanho muda.
pub fn convert_items_to_stable_observables<T: Clone + StrictEquals + 'static>(
    items: &IObservable<Rc<Vec<T>>>,
    store: &DisposableStore,
) -> IObservable<Rc<Vec<IObservable<T>>>> {
    let result = observable_value::<Rc<Vec<IObservable<T>>>, ()>("result", Rc::new(Vec::new()));
    let inner_observables: Rc<RefCell<Vec<Rc<ObservableValue<T, ()>>>>> = Rc::default();

    let items = Rc::clone(items);
    let (result_to_set, inner_to_update) = (Rc::clone(&result), Rc::clone(&inner_observables));
    store.add(autorun(move |reader| {
        let items_value = items.read(reader);

        transaction(|tx| {
            let mut inner = inner_to_update.borrow_mut();
            if items_value.len() != inner.len() {
                inner.truncate(items_value.len());
                while inner.len() < items_value.len() {
                    let initial_value = items_value[inner.len()].clone();
                    inner.push(observable_value::<T, ()>("item", initial_value));
                }
                let stable_observables: Vec<IObservable<T>> = inner.iter().map(|observable| observable.to_observable()).collect();
                result_to_set.set(Rc::new(stable_observables), Some(tx));
            }
            for (observable, value) in inner.iter().zip(items_value.iter()) {
                observable.set(value.clone(), Some(tx));
            }
        });
    }));

    result
}

/// `ObservableContextKeyService`.
pub struct ObservableContextKeyService {
    context_key_service: Rc<dyn IContextKeyService>,
}

impl ObservableContextKeyService {
    pub fn new(context_key_service: Rc<dyn IContextKeyService>) -> Self {
        Self { context_key_service }
    }

    /// `bind(key, fn)`: a chave recebe o valor que a função calcula, e acompanha as dependências lidas por ela.
    pub fn bind(&self, key: &RawContextKey, compute_value: impl Fn(&dyn IReader) -> Dynamic + 'static) -> DisposableRef {
        bind_context_key(key, &*self.context_key_service, compute_value)
    }

    /// `bind(key, obs)`: a chave acompanha o valor do observável.
    pub fn bind_observable<T: Clone + Into<Dynamic> + 'static>(&self, key: &RawContextKey, observable: &IObservable<T>) -> DisposableRef {
        let observable = Rc::clone(observable);
        self.bind(key, move |reader| observable.read(reader).into())
    }
}

/// `wait(ms, cancellationToken?)`: resolve depois de `ms` milissegundos, ou assim que o token cancelar (sem rejeitar,
/// diferente de `timeout` de `async`).
pub fn wait(ms: u64, cancellation_token: Option<&CancellationToken>) -> Promise<()> {
    let token = cancellation_token.cloned();
    Promise::with_executor(move |resolver| {
        let disposable: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
        let handle = runtime::set_timeout(
            {
                let (disposable, resolver) = (Rc::clone(&disposable), resolver.clone());
                move || {
                    dispose_slot(&disposable);
                    resolver.resolve(());
                }
            },
            ms,
        );
        if let Some(token) = token {
            let listener = token.on_cancellation_requested().listen({
                let disposable = Rc::clone(&disposable);
                move |_| {
                    runtime::clear_timeout(handle);
                    dispose_slot(&disposable);
                    resolver.resolve(());
                }
            });
            *disposable.borrow_mut() = Some(listener);
        }
    })
}

/// `if (d) { d.dispose(); }`.
fn dispose_slot(slot: &Rc<RefCell<Option<DisposableRef>>>) {
    let current = slot.borrow_mut().take();
    if let Some(current) = current {
        current.dispose();
    }
}

/// O descarte de um colaborador compartilhado como `DisposableRef` (o `Rc<dyn Trait>` não vira `Rc<dyn IDisposable>`
/// sem o laço de conversão).
pub fn dispose_handle<T: IDisposable + ?Sized + 'static>(target: &Rc<T>) -> DisposableRef {
    let target = Rc::clone(target);
    plain_disposable(move || target.dispose())
}
