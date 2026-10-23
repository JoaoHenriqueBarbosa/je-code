//! Porte de `observableInternal/utils/utilsCancellation.ts`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::cancellation::{CancellationToken, CancellationTokenSource};
use crate::errors::Error;
use crate::lifecycle::{DisposableRef, IDisposable};
use crate::observable::base::{IObservable, IReader};
use crate::observable::debug_name::DebugNameData;
use crate::observable::deps::{StrictEquals, strict_equals};
use crate::observable::observables::base_observable::ObservableExt;
use crate::observable::observables::derived::Derived;
use crate::observable::reactions::autorun::autorun;
use crate::promise::Promise;

struct StateInfo<T> {
    is_finished: bool,
    error: Option<Error>,
    state: T,
}

/// `waitForState(observable, predicate, isError, cancellationToken)`.
///
/// Diferença: `is_error` devolve `Option<Error>` (o motivo da rejeição). O original devolve
/// `true` (rejeita com o próprio estado) ou um valor qualquer, e rejeitar com um valor que não é `Error`
/// não existe nas promessas do porte.
pub fn wait_for_state<T: Clone + 'static>(
    observable: IObservable<T>,
    predicate: impl Fn(&T) -> bool + 'static,
    is_error: Option<Rc<dyn Fn(&T) -> Option<Error>>>,
    cancellation_token: Option<CancellationToken>,
) -> Promise<T> {
    Promise::with_executor(move |resolver| {
        let is_immediate_run = Rc::new(Cell::new(true));
        let should_dispose = Rc::new(Cell::new(false));
        let d_slot: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));
        let state_obs = observable.map(move |state, _| {
            Rc::new(StateInfo { is_finished: predicate(&state), error: is_error.as_ref().and_then(|f| f(&state)), state })
        });
        let (immediate, dispose_flag, slot, resolver_in) = (is_immediate_run.clone(), should_dispose.clone(), d_slot.clone(), resolver.clone());
        let d = autorun(move |reader| {
            let info = state_obs.read(reader);
            if info.is_finished || info.error.is_some() {
                if immediate.get() {
                    // A variável `d` ainda não foi inicializada.
                    dispose_flag.set(true);
                } else {
                    let current = slot.borrow().clone();
                    if let Some(d) = current {
                        d.dispose();
                    }
                }
                match &info.error {
                    Some(e) => resolver_in.reject(e.clone()),
                    None => resolver_in.resolve(info.state.clone()),
                }
            }
        });
        *d_slot.borrow_mut() = Some(d.clone());
        if let Some(token) = cancellation_token {
            let dc_slot: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));
            let (d_in, dc_in, resolver_in) = (d.clone(), dc_slot.clone(), resolver.clone());
            let dc = token.on_cancellation_requested().listen(move |_| {
                d_in.dispose();
                let dc = dc_in.borrow().clone();
                if let Some(dc) = dc {
                    dc.dispose();
                }
                resolver_in.reject(Error::cancellation());
            });
            *dc_slot.borrow_mut() = Some(dc.clone());
            if token.is_cancellation_requested() {
                d.dispose();
                dc.dispose();
                resolver.reject(Error::cancellation());
                return;
            }
        }
        is_immediate_run.set(false);
        if should_dispose.get() {
            d.dispose();
        }
    })
}

/// `waitForState(observable)` com o predicado padrão (`state !== null && state !== undefined`).
pub fn wait_for_state_defined<T: Clone + 'static>(observable: IObservable<Option<T>>) -> Promise<T> {
    wait_for_state(observable, |s| s.is_some(), None, None).then(|s| s.expect("o predicado garante o valor"))
}

/// `derivedWithCancellationToken(computeFn)`.
pub fn derived_with_cancellation_token<T: Clone + StrictEquals + 'static>(
    compute_fn: impl Fn(&dyn IReader, &CancellationToken) -> T + 'static,
) -> IObservable<T> {
    let source: Rc<RefCell<Option<CancellationTokenSource>>> = Rc::new(RefCell::new(None));
    let source_dispose = source.clone();
    Derived::<_, (), ()>::new_untracked(
        DebugNameData::default(),
        move |r| {
            let previous = source.borrow_mut().take();
            if let Some(s) = previous {
                s.dispose_with(true);
            }
            let fresh = CancellationTokenSource::new(None);
            let token = fresh.token();
            *source.borrow_mut() = Some(fresh);
            compute_fn(r, &token)
        },
        Some(Rc::new(move || {
            let current = source_dispose.borrow().clone();
            if let Some(s) = current {
                s.dispose();
            }
        })),
        strict_equals(),
    )
}
