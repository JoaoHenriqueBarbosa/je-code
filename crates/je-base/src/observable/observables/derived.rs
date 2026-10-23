//! Porte de `observableInternal/observables/derived.ts` e `derivedImpl.ts`.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::rc::{Rc, Weak};

use super::base_observable::{BaseObservable, ObservableHooks};
use crate::errors::{self, Error};
use crate::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use crate::observable::base::{
    AnyObservable, DebugInfo, DebugNode, IObservable, IObserver, IReader, IReaderWithStore, ISettableObservable, ITransaction, Observable,
    ObservableRef, ObserverRef, Settable, observable_id, observer_id,
};
use crate::observable::change_tracker::{IChangeContext, IChangeTracker};
use crate::observable::debug_name::{DebugNameData, DebugNameOptions, DebugNameSource, DebugOwner};
use crate::observable::deps::{EqualityComparer, LiveSet, StrictEquals, for_each_live, strict_equals, try_finally};
use crate::observable::logging::debug_get_dependency_graph::format_value;

/// `IDerivedReader<TChange>`.
pub trait IDerivedReader<C>: IReaderWithStore {
    /// Reporta um delta, ou força o reporte de mudança mesmo que o valor novo seja igual ao antigo.
    fn report_change(&self, change: C);
}

/// `DerivedState`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DerivedState {
    /// Estado inicial, sem valor anterior: precisa recomputar.
    Initial = 0,
    /// Uma dependência pode ter mudado: é preciso perguntar a elas.
    DependenciesMightHaveChanged = 1,
    /// Uma dependência mudou: precisa recomputar.
    Stale = 2,
    /// Nenhuma mudança reportada: o valor em cache está em dia.
    UpToDate = 3,
}

impl DerivedState {
    fn name(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::DependenciesMightHaveChanged => "dependenciesMightHaveChanged",
            Self::Stale => "stale",
            Self::UpToDate => "upToDate",
        }
    }
}

type ComputeFn<T, S, C> = Rc<dyn Fn(&dyn IDerivedReader<C>, &S) -> T>;
type SetterFn<T, C> = Rc<dyn Fn(T, Option<&dyn ITransaction>, C)>;

/// `Derived<T, TChangeSummary, TChange>`.
pub struct Derived<T: Clone + 'static, S: 'static, C: 'static> {
    base: BaseObservable,
    me: Weak<Self>,
    debug: DebugNameData,
    compute_fn: ComputeFn<T, S, C>,
    change_tracker: Option<Rc<dyn IChangeTracker<S>>>,
    handle_last_observer_removed: Option<Rc<dyn Fn()>>,
    equality: EqualityComparer<T>,
    setter: Option<SetterFn<T, C>>,

    state: Cell<DerivedState>,
    value: RefCell<Option<T>>,
    update_count: Cell<i64>,
    dependencies: RefCell<LiveSet<ObservableRef>>,
    dependencies_to_be_removed: RefCell<LiveSet<ObservableRef>>,
    change_summary: RefCell<Option<Rc<S>>>,
    is_updating: Cell<bool>,
    is_computing: Cell<bool>,
    did_report_change: Cell<bool>,
    is_in_before_update: Cell<bool>,
    is_reader_valid: Cell<bool>,
    store: RefCell<Option<Rc<DisposableStore>>>,
    delayed_store: RefCell<Option<Rc<DisposableStore>>>,
    removed_observer_to_call_end_update_on: RefCell<Option<LiveSet<ObserverRef>>>,
    _c: PhantomData<fn(C)>,
}

impl<T: Clone + 'static, C: 'static> Derived<T, (), C> {
    /// Um derived sem rastreador de mudanças (o resumo é `()`).
    pub fn new_untracked(
        debug: DebugNameData,
        compute_fn: impl Fn(&dyn IDerivedReader<C>) -> T + 'static,
        handle_last_observer_removed: Option<Rc<dyn Fn()>>,
        equality: EqualityComparer<T>,
    ) -> Rc<Self> {
        Self::build(debug, Rc::new(move |r, _| compute_fn(r)), None, (), handle_last_observer_removed, equality, None)
    }
}

impl<T: Clone + 'static, S: 'static, C: 'static> Derived<T, S, C> {
    /// Um derived com rastreador de mudanças.
    pub fn new_tracked(
        debug: DebugNameData,
        compute_fn: impl Fn(&dyn IDerivedReader<C>, &S) -> T + 'static,
        change_tracker: Rc<dyn IChangeTracker<S>>,
        handle_last_observer_removed: Option<Rc<dyn Fn()>>,
        equality: EqualityComparer<T>,
        setter: Option<SetterFn<T, C>>,
    ) -> Rc<Self> {
        let summary = change_tracker.create_change_summary(None);
        Self::build(debug, Rc::new(compute_fn), Some(change_tracker), summary, handle_last_observer_removed, equality, setter)
    }

    fn build(
        debug: DebugNameData,
        compute_fn: ComputeFn<T, S, C>,
        change_tracker: Option<Rc<dyn IChangeTracker<S>>>,
        summary: S,
        handle_last_observer_removed: Option<Rc<dyn Fn()>>,
        equality: EqualityComparer<T>,
        setter: Option<SetterFn<T, C>>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            base: BaseObservable::new(me.clone() as Weak<dyn AnyObservable>),
            me: me.clone(),
            debug,
            compute_fn,
            change_tracker,
            handle_last_observer_removed,
            equality,
            setter,
            state: Cell::new(DerivedState::Initial),
            value: RefCell::new(None),
            update_count: Cell::new(0),
            dependencies: RefCell::new(LiveSet::default()),
            dependencies_to_be_removed: RefCell::new(LiveSet::default()),
            change_summary: RefCell::new(Some(Rc::new(summary))),
            is_updating: Cell::new(false),
            is_computing: Cell::new(false),
            did_report_change: Cell::new(false),
            is_in_before_update: Cell::new(false),
            is_reader_valid: Cell::new(false),
            store: RefCell::new(None),
            delayed_store: RefCell::new(None),
            removed_observer_to_call_end_update_on: RefCell::new(None),
            _c: PhantomData,
        })
    }

    fn as_observer(&self) -> ObserverRef {
        self.me.upgrade().expect("vivo")
    }

    fn as_reader(&self) -> Rc<Self> {
        self.me.upgrade().expect("vivo")
    }

    fn summary(&self) -> Rc<S> {
        self.change_summary.borrow().clone().expect("o resumo existe desde a construção")
    }

    fn cleanup_after_last_observer(&self) {
        // Não rastreamos mais mudanças, então o cache é inválido.
        self.state.set(DerivedState::Initial);
        *self.value.borrow_mut() = None;
        let me = self.as_observer();
        for_each_live(&self.dependencies, |d| {
            d.remove_observer(&me);
            true
        });
        self.dependencies.borrow_mut().clear();
        let store = self.store.borrow_mut().take();
        if let Some(s) = store {
            s.dispose();
        }
        let delayed = self.delayed_store.borrow_mut().take();
        if let Some(s) = delayed {
            s.dispose();
        }
        if let Some(f) = &self.handle_last_observer_removed {
            f();
        }
    }

    fn recompute(&self) {
        let mut did_change = false;
        self.is_computing.set(true);
        self.did_report_change.set(false);
        self.dependencies.swap(&self.dependencies_to_be_removed);

        let me_reader = self.as_reader();
        let result = errors::catch(|| {
            let change_summary = self.summary();
            self.is_reader_valid.set(true);
            if let Some(tracker) = &self.change_tracker {
                self.is_in_before_update.set(true);
                tracker.before_update(&*me_reader, &change_summary);
                self.is_in_before_update.set(false);
                let next = tracker.create_change_summary(Some(&change_summary));
                *self.change_summary.borrow_mut() = Some(Rc::new(next));
            }

            let had_value = self.state.get() != DerivedState::Initial;
            let old_value = self.value.borrow().clone();
            self.state.set(DerivedState::UpToDate);

            let delayed_store = self.delayed_store.borrow_mut().take();
            try_finally(
                || {
                    let store = self.store.borrow_mut().take();
                    if let Some(s) = store {
                        s.dispose();
                    }
                    // Pode chamar `handle_change` indiretamente, o que pode nos invalidar.
                    let new_value = (self.compute_fn)(&*me_reader, &change_summary);
                    *self.value.borrow_mut() = Some(new_value);
                },
                || {
                    self.is_reader_valid.set(false);
                    // Só saímos das dependências que definitivamente não são mais lidas.
                    let me = self.as_observer();
                    for_each_live(&self.dependencies_to_be_removed, |o| {
                        o.remove_observer(&me);
                        true
                    });
                    self.dependencies_to_be_removed.borrow_mut().clear();
                    if let Some(s) = delayed_store {
                        s.dispose();
                    }
                },
            );

            let new_value = self.value.borrow();
            did_change = self.did_report_change.get()
                || (had_value && old_value.as_ref().zip(new_value.as_ref()).is_some_and(|(o, n)| !(self.equality)(o, n)));
        });
        if let Err(e) = result {
            errors::on_bug_indicating_error(&e);
        }
        self.is_computing.set(false);
        if !self.did_report_change.get() && did_change {
            let me = self.base.to_any_observable();
            self.base.for_each_observer(|r| r.handle_change(&*me, &()));
        } else {
            self.did_report_change.set(false);
        }
    }

    fn ensure_reader_valid(&self) {
        if !self.is_reader_valid.get() {
            errors::throw(Error::bug_indicating(Some("The reader object cannot be used outside its compute function!")));
        }
    }

    /// `setValue(newValue, tx, change)`: muda o valor em cache e avisa os observadores (reducer).
    pub fn set_value(&self, new_value: T, tx: &dyn ITransaction, change: C) {
        *self.value.borrow_mut() = Some(new_value);
        let me_obs = self.base.to_any_observable();
        tx.update_observer(self.as_observer(), me_obs.clone());
        self.base.for_each_observer(|d| d.handle_change(&*me_obs, &change));
    }

    /// `debugRecompute()`.
    pub fn debug_recompute(&self) {
        let me = self.base.to_any_observable();
        self.begin_update(&*me);
        if !self.is_computing.get() {
            self.recompute();
        } else {
            self.state.set(DerivedState::Stale);
        }
        self.end_update(&*me);
    }

    pub fn state(&self) -> DerivedState {
        self.state.get()
    }
}

impl<T: Clone + 'static, S: 'static, C: 'static> ObservableHooks for Derived<T, S, C> {
    fn on_last_observer_removed(&self) {
        self.cleanup_after_last_observer();
    }
}

impl<T: Clone + 'static, S: 'static, C: 'static> AnyObservable for Derived<T, S, C> {
    fn report_changes(&self) {
        self.get();
    }

    fn add_observer(&self, observer: ObserverRef) {
        let should_call_begin_update = !self.base.has_observer(&observer) && self.update_count.get() > 0;
        self.base.add_observer(observer.clone(), self);
        if should_call_begin_update {
            let was_removed = {
                let mut removed = self.removed_observer_to_call_end_update_on.borrow_mut();
                removed.as_mut().is_some_and(|s| s.delete(observer_id(&observer)))
            };
            if !was_removed {
                observer.begin_update(self);
            }
        }
    }

    fn remove_observer(&self, observer: &ObserverRef) {
        if self.base.has_observer(observer) && self.update_count.get() > 0 {
            self.removed_observer_to_call_end_update_on
                .borrow_mut()
                .get_or_insert_with(LiveSet::default)
                .add(observer_id(observer), observer.clone());
        }
        self.base.remove_observer(observer, self);
    }

    fn debug_name(&self) -> String {
        self.debug.get_debug_name().unwrap_or_else(|| "(anonymous)".into())
    }

    fn to_any_observable(&self) -> Rc<dyn AnyObservable> {
        self.base.to_any_observable()
    }

    fn debug_info(&self) -> Option<DebugInfo> {
        let value = self.value.borrow();
        Some(DebugInfo {
            source_id: self.id(),
            name: self.debug_name(),
            kind: "derived",
            value: match &*value {
                Some(v) => format_value(v, 50),
                None => "undefined".into(),
            },
            state: self.state.get().name().into(),
            dependencies: self.dependencies.borrow().snapshot().into_iter().map(DebugNode::Observable).collect(),
            observers: self.base.observers().into_iter().map(DebugNode::Observer).collect(),
        })
    }
}

impl<T: Clone + 'static, S: 'static, C: 'static> Observable<T> for Derived<T, S, C> {
    fn get(&self) -> T {
        if self.base.observer_count() == 0 {
            // Sem observadores não sabemos quando limpar, então nada fica em cache (evita vazamento).
            let me_reader = self.as_reader();
            let result = try_finally(
                || {
                    self.is_reader_valid.set(true);
                    match &self.change_tracker {
                        Some(tracker) => {
                            let summary = tracker.create_change_summary(None);
                            tracker.before_update(&*me_reader, &summary);
                            (self.compute_fn)(&*me_reader, &summary)
                        }
                        None => {
                            let summary = self.summary();
                            (self.compute_fn)(&*me_reader, &summary)
                        }
                    }
                },
                || self.is_reader_valid.set(false),
            );
            // Limpa as dependências novas.
            self.cleanup_after_last_observer();
            return result;
        }
        loop {
            // Podemos não receber o aviso de uma dependência que mudou enquanto atualizava, então
            // perguntamos a todas.
            if self.state.get() == DerivedState::DependenciesMightHaveChanged {
                for_each_live(&self.dependencies, |d| {
                    // Pode chamar `handle_change` indiretamente, o que nos deixa `Stale`.
                    d.report_changes();
                    // As outras atualizam sob demanda, então sai cedo.
                    self.state.get() != DerivedState::Stale
                });
            }
            // Se ainda não estamos `Stale`, assumimos que estamos em dia.
            if self.state.get() == DerivedState::DependenciesMightHaveChanged {
                self.state.set(DerivedState::UpToDate);
            }
            if self.state.get() != DerivedState::UpToDate {
                self.recompute();
            }
            // Se a recomputação mudou uma dependência, recomputa de novo.
            if self.state.get() == DerivedState::UpToDate {
                break;
            }
        }
        self.value.borrow().clone().unwrap_or_else(|| {
            errors::throw(Error::bug_indicating(Some("A função de cálculo do derived lançou e não há valor anterior")))
        })
    }

    fn to_observable(&self) -> IObservable<T> {
        self.me.upgrade().expect("vivo")
    }
}

impl<T: Clone + 'static, S: 'static, C: 'static> Settable<T, C> for Derived<T, S, C> {
    fn set_with_change(&self, value: T, tx: Option<&dyn ITransaction>, change: C) {
        match &self.setter {
            Some(set) => set(value, tx, change),
            None => errors::throw(Error::bug_indicating(Some("Este derived não tem setter"))),
        }
    }

    fn to_settable(&self) -> ISettableObservable<T, C> {
        self.me.upgrade().expect("vivo")
    }
}

struct Ctx<'a> {
    observable: &'a dyn AnyObservable,
    change: &'a dyn Any,
}

impl IChangeContext for Ctx<'_> {
    fn changed_observable(&self) -> &dyn AnyObservable {
        self.observable
    }

    fn change_any(&self) -> &dyn Any {
        self.change
    }
}

impl<T: Clone + 'static, S: 'static, C: 'static> IObserver for Derived<T, S, C> {
    fn begin_update(&self, _observable: &dyn AnyObservable) {
        if self.is_updating.get() {
            errors::throw(Error::bug_indicating(Some("Cyclic deriveds are not supported yet!")));
        }
        self.update_count.set(self.update_count.get() + 1);
        self.is_updating.set(true);
        let me = self.base.to_any_observable();
        try_finally(
            || {
                let propagate_begin_update = self.update_count.get() == 1;
                if self.state.get() == DerivedState::UpToDate {
                    self.state.set(DerivedState::DependenciesMightHaveChanged);
                    // Propagar `begin_update` já sinaliza uma possível mudança.
                    if !propagate_begin_update {
                        self.base.for_each_observer(|r| r.handle_possible_change(&*me));
                    }
                }
                if propagate_begin_update {
                    // Isto sinaliza uma possível mudança.
                    self.base.for_each_observer(|r| r.begin_update(&*me));
                }
            },
            || self.is_updating.set(false),
        );
    }

    fn end_update(&self, _observable: &dyn AnyObservable) {
        self.update_count.set(self.update_count.get() - 1);
        if self.update_count.get() == 0 {
            let me = self.base.to_any_observable();
            // `end_update` pode mudar a lista de observadores.
            for r in self.base.observers() {
                r.end_update(&*me);
            }
            let removed = self.removed_observer_to_call_end_update_on.borrow_mut().take();
            if let Some(removed) = removed {
                for r in removed.snapshot() {
                    r.end_update(&*me);
                }
            }
        }
        crate::assert::assert_fn(|| self.update_count.get() >= 0);
    }

    fn handle_possible_change(&self, observable: &dyn AnyObservable) {
        // Nos outros estados os observadores já sabem que podemos ter mudado.
        if self.state.get() == DerivedState::UpToDate
            && self.dependencies.borrow().has(observable.id())
            && !self.dependencies_to_be_removed.borrow().has(observable.id())
        {
            self.state.set(DerivedState::DependenciesMightHaveChanged);
            let me = self.base.to_any_observable();
            self.base.for_each_observer(|r| r.handle_possible_change(&*me));
        }
    }

    fn handle_change(&self, observable: &dyn AnyObservable, change: &dyn Any) {
        let is_dependency = self.dependencies.borrow().has(observable.id()) && !self.dependencies_to_be_removed.borrow().has(observable.id());
        if is_dependency || self.is_in_before_update.get() {
            let mut should_react = false;
            match &self.change_tracker {
                Some(tracker) => {
                    let summary = self.summary();
                    match errors::catch(|| tracker.handle_change(&Ctx { observable, change }, &summary)) {
                        Ok(v) => should_react = v,
                        Err(e) => errors::on_bug_indicating_error(&e),
                    }
                }
                None => should_react = true,
            }
            let was_up_to_date = self.state.get() == DerivedState::UpToDate;
            if should_react && (self.state.get() == DerivedState::DependenciesMightHaveChanged || was_up_to_date) {
                self.state.set(DerivedState::Stale);
                if was_up_to_date {
                    let me = self.base.to_any_observable();
                    self.base.for_each_observer(|r| r.handle_possible_change(&*me));
                }
            }
        }
    }

    fn debug_info(&self) -> Option<DebugInfo> {
        AnyObservable::debug_info(self)
    }
}

impl<T: Clone + 'static, S: 'static, C: 'static> IReader for Derived<T, S, C> {
    fn read_observable_erased(&self, observable: ObservableRef, get: &mut dyn FnMut()) {
        self.ensure_reader_valid();
        // Inscreve antes de ler para habilitar o cache.
        observable.add_observer(self.as_observer());
        // Pode chamar `handle_change` indiretamente, o que nos invalidaria; por isso só agora
        // registramos a dependência.
        get();
        self.dependencies.borrow_mut().add(observable_id(&observable), observable.clone());
        self.dependencies_to_be_removed.borrow_mut().delete(observable_id(&observable));
    }
}

impl<T: Clone + 'static, S: 'static, C: 'static> IReaderWithStore for Derived<T, S, C> {
    fn store(&self) -> Rc<DisposableStore> {
        self.ensure_reader_valid();
        self.store.borrow_mut().get_or_insert_with(DisposableStore::new).clone()
    }

    fn delayed_store(&self) -> Rc<DisposableStore> {
        self.ensure_reader_valid();
        self.delayed_store.borrow_mut().get_or_insert_with(DisposableStore::new).clone()
    }
}

impl<T: Clone + 'static, S: 'static, C: 'static> IDerivedReader<C> for Derived<T, S, C> {
    fn report_change(&self, change: C) {
        self.ensure_reader_valid();
        self.did_report_change.set(true);
        let me = self.base.to_any_observable();
        self.base.for_each_observer(|r| r.handle_change(&*me, &change));
    }
}

// ---------------------------------------------------------------------------------------------------
// derived.ts

/// `IDebugNameData & { equalsFn, onLastObserverRemoved }`.
pub struct DerivedOptions<T> {
    pub owner: Option<DebugOwner>,
    pub debug_name: Option<DebugNameSource>,
    pub equals_fn: Option<EqualityComparer<T>>,
    pub on_last_observer_removed: Option<Rc<dyn Fn()>>,
}

impl<T> Default for DerivedOptions<T> {
    fn default() -> Self {
        Self { owner: None, debug_name: None, equals_fn: None, on_last_observer_removed: None }
    }
}

impl<T> DerivedOptions<T> {
    fn debug_name_data(&self) -> DebugNameData {
        DebugNameData::new(self.owner.clone(), self.debug_name.clone())
    }
}

/// `derived(computeFn)`: o valor só é recomputado quando necessário.
pub fn derived<T: Clone + StrictEquals + 'static>(compute_fn: impl Fn(&dyn IDerivedReader<()>) -> T + 'static) -> IObservable<T> {
    Derived::<_, (), ()>::new_untracked(DebugNameData::default(), compute_fn, None, strict_equals())
}

/// `derived<T, TChange>(computeFn)`: o leitor aceita `report_change` com deltas do tipo `C`.
pub fn derived_with_change<T: Clone + StrictEquals + 'static, C: 'static>(compute_fn: impl Fn(&dyn IDerivedReader<C>) -> T + 'static) -> IObservable<T> {
    Derived::<_, (), C>::new_untracked(DebugNameData::default(), compute_fn, None, strict_equals())
}

/// `derived(owner, computeFn)`.
pub fn derived_owned<T: Clone + StrictEquals + 'static>(owner: DebugOwner, compute_fn: impl Fn(&dyn IDerivedReader<()>) -> T + 'static) -> IObservable<T> {
    Derived::<_, (), ()>::new_untracked(DebugNameData::new(Some(owner), None), compute_fn, None, strict_equals())
}

/// `derivedWithSetter(owner, computeFn, setter)`.
pub fn derived_with_setter<T: Clone + StrictEquals + 'static>(
    owner: Option<DebugOwner>,
    compute_fn: impl Fn(&dyn IReader) -> T + 'static,
    setter: impl Fn(T, Option<&dyn ITransaction>) + 'static,
) -> ISettableObservable<T> {
    let tracker: Rc<dyn IChangeTracker<()>> = Rc::new(NoTracker);
    let d: Rc<Derived<T, (), ()>> = Derived::new_tracked(
        DebugNameData::new(owner, None),
        move |r, _| compute_fn(r),
        tracker,
        None,
        strict_equals(),
        Some(Rc::new(move |v, tx, _| setter(v, tx))),
    );
    d
}

/// Um rastreador que aceita todas as mudanças (equivale a não ter rastreador); só existe para o
/// `derivedWithSetter`, cujo tipo precisa de um resumo `()`.
pub struct NoTracker;

impl IChangeTracker<()> for NoTracker {
    fn create_change_summary(&self, _previous: Option<&()>) {}

    fn handle_change(&self, _ctx: &dyn IChangeContext, _change_summary: &()) -> bool {
        true
    }
}

/// `derivedOpts(options, computeFn)`.
pub fn derived_opts<T: Clone + StrictEquals + 'static>(options: DerivedOptions<T>, compute_fn: impl Fn(&dyn IReader) -> T + 'static) -> IObservable<T> {
    let debug = options.debug_name_data();
    Derived::<_, (), ()>::new_untracked(
        debug,
        move |r| compute_fn(r),
        options.on_last_observer_removed,
        options.equals_fn.unwrap_or_else(strict_equals),
    )
}

/// As opções de `derivedHandleChanges`.
pub struct DerivedHandleChangesOptions<T, S> {
    pub debug: DebugNameOptions,
    pub change_tracker: Rc<dyn IChangeTracker<S>>,
    pub equality_comparer: Option<EqualityComparer<T>>,
}

/// `derivedHandleChanges(options, computeFn)`: o `compute_fn` recebe o resumo de mudanças.
pub fn derived_handle_changes<T: Clone + StrictEquals + 'static, C: 'static, S: 'static>(
    options: DerivedHandleChangesOptions<T, S>,
    compute_fn: impl Fn(&dyn IDerivedReader<C>, &S) -> T + 'static,
) -> IObservable<T> {
    Derived::new_tracked(
        options.debug.into(),
        compute_fn,
        options.change_tracker,
        None,
        options.equality_comparer.unwrap_or_else(strict_equals),
        None,
    )
}

/// `derivedWithStore(computeFn)` (obsoleto no original).
pub fn derived_with_store<T: Clone + StrictEquals + 'static>(compute_fn: impl Fn(&dyn IReader, &DisposableStore) -> T + 'static) -> IObservable<T> {
    // Reatribuído de propósito, caso um observável inativo seja reutilizado depois.
    let store: Rc<RefCell<Rc<DisposableStore>>> = Rc::new(RefCell::new(DisposableStore::new()));
    let store_for_dispose = store.clone();
    Derived::<_, (), ()>::new_untracked(
        DebugNameData::default(),
        move |r| {
            let current = store.borrow().clone();
            let s = if current.is_disposed() {
                let fresh = DisposableStore::new();
                *store.borrow_mut() = fresh.clone();
                fresh
            } else {
                current.clear();
                current
            };
            compute_fn(r, &s)
        },
        Some(Rc::new(move || store_for_dispose.borrow().dispose())),
        strict_equals(),
    )
}

/// `derivedDisposable(computeFn)`: o resultado é descartado na próxima computação ou quando o derived
/// fica sem observadores.
pub fn derived_disposable(compute_fn: impl Fn(&dyn IReader) -> Option<DisposableRef> + 'static) -> IObservable<Option<DisposableRef>> {
    let store: Rc<RefCell<Option<Rc<DisposableStore>>>> = Rc::new(RefCell::new(None));
    let store_for_dispose = store.clone();
    Derived::<_, (), ()>::new_untracked(
        DebugNameData::default(),
        move |r| {
            let s = {
                let mut slot = store.borrow_mut();
                match &*slot {
                    Some(s) => {
                        let s = s.clone();
                        drop(slot);
                        s.clear();
                        s
                    }
                    None => {
                        let s = DisposableStore::new();
                        *slot = Some(s.clone());
                        s
                    }
                }
            };
            let result = compute_fn(r);
            if let Some(res) = &result {
                s.add(res.clone());
            }
            result
        },
        Some(Rc::new(move || {
            let s = store_for_dispose.borrow_mut().take();
            if let Some(s) = s {
                s.dispose();
            }
        })),
        strict_equals(),
    )
}
