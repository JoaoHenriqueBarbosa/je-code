//! Porte de `observableInternal/reactions/autorunImpl.ts`.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use crate::errors::{self, Error};
use crate::lifecycle::{DisposableStore, IDisposable, mark_as_disposed, id_of_ref, track_disposable};
use crate::observable::base::{
    AnyObservable, DebugInfo, DebugNode, IObserver, IReader, IReaderWithStore, ObservableRef, ObserverRef, observable_id,
};
use crate::observable::change_tracker::{IChangeContext, IChangeTracker};
use crate::observable::debug_name::DebugNameData;
use crate::observable::deps::{LiveSet, for_each_live, try_finally};

/// `AutorunState`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AutorunState {
    DependenciesMightHaveChanged = 1,
    Stale = 2,
    UpToDate = 3,
}

impl AutorunState {
    fn name(self) -> &'static str {
        match self {
            Self::DependenciesMightHaveChanged => "dependenciesMightHaveChanged",
            Self::Stale => "stale",
            Self::UpToDate => "upToDate",
        }
    }
}

type RunFn<S> = Rc<dyn Fn(&dyn IReaderWithStore, &S)>;

/// `AutorunObserver<TChangeSummary>`.
pub struct AutorunObserver<S: 'static> {
    me: Weak<Self>,
    debug: DebugNameData,
    run_fn: RunFn<S>,
    change_tracker: Option<Rc<dyn IChangeTracker<S>>>,
    state: Cell<AutorunState>,
    update_count: Cell<i64>,
    disposed: Cell<bool>,
    dependencies: RefCell<LiveSet<ObservableRef>>,
    dependencies_to_be_removed: RefCell<LiveSet<ObservableRef>>,
    change_summary: RefCell<Option<Rc<S>>>,
    is_running: Cell<bool>,
    iteration: Cell<u32>,
    store: RefCell<Option<Rc<DisposableStore>>>,
    delayed_store: RefCell<Option<Rc<DisposableStore>>>,
}

impl AutorunObserver<()> {
    pub fn new_untracked(debug: DebugNameData, run_fn: impl Fn(&dyn IReaderWithStore) + 'static) -> Rc<Self> {
        Self::create(debug, Rc::new(move |r, _| run_fn(r)), None, ())
    }
}

impl<S: 'static> AutorunObserver<S> {
    pub fn new_tracked(debug: DebugNameData, run_fn: impl Fn(&dyn IReaderWithStore, &S) + 'static, change_tracker: Rc<dyn IChangeTracker<S>>) -> Rc<Self> {
        let summary = change_tracker.create_change_summary(None);
        Self::create(debug, Rc::new(run_fn), Some(change_tracker), summary)
    }

    /// O construtor do original: já roda a primeira vez e registra no rastreador de vazamentos.
    fn create(debug: DebugNameData, run_fn: RunFn<S>, change_tracker: Option<Rc<dyn IChangeTracker<S>>>, summary: S) -> Rc<Self> {
        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            debug,
            run_fn,
            change_tracker,
            state: Cell::new(AutorunState::Stale),
            update_count: Cell::new(0),
            disposed: Cell::new(false),
            dependencies: RefCell::new(LiveSet::default()),
            dependencies_to_be_removed: RefCell::new(LiveSet::default()),
            change_summary: RefCell::new(Some(Rc::new(summary))),
            is_running: Cell::new(false),
            iteration: Cell::new(0),
            store: RefCell::new(None),
            delayed_store: RefCell::new(None),
        });
        this.run();
        track_disposable(this)
    }

    fn as_observer(&self) -> ObserverRef {
        self.me.upgrade().expect("vivo")
    }

    pub fn debug_name(&self) -> String {
        self.debug.get_debug_name().unwrap_or_else(|| "(anonymous)".into())
    }

    fn run(&self) {
        self.dependencies.swap(&self.dependencies_to_be_removed);
        self.state.set(AutorunState::UpToDate);
        try_finally(
            || {
                if !self.disposed.get() {
                    let change_summary = self.change_summary.borrow().clone().expect("o resumo existe");
                    let delayed_store = self.delayed_store.borrow_mut().take();
                    let me = self.me.upgrade().expect("vivo");
                    let result = errors::catch(|| {
                        self.is_running.set(true);
                        if let Some(tracker) = &self.change_tracker {
                            tracker.before_update(&*me, &change_summary);
                            let next = tracker.create_change_summary(Some(&change_summary)); // chamada externa
                            *self.change_summary.borrow_mut() = Some(Rc::new(next));
                        }
                        let store = self.store.borrow_mut().take();
                        if let Some(s) = store {
                            s.dispose();
                        }
                        (self.run_fn)(&*me, &change_summary); // chamada externa
                    });
                    if let Err(e) = result {
                        errors::on_bug_indicating_error(&e);
                    }
                    self.is_running.set(false);
                    if let Some(s) = delayed_store {
                        s.dispose();
                    }
                }
            },
            || {
                // Só saímos das dependências que definitivamente não são mais lidas.
                let me = self.as_observer();
                for_each_live(&self.dependencies_to_be_removed, |o| {
                    o.remove_observer(&me); // chamada externa
                    true
                });
                self.dependencies_to_be_removed.borrow_mut().clear();
            },
        );
    }

    fn is_dependency(&self, observable: &dyn AnyObservable) -> bool {
        self.dependencies.borrow().has(observable.id()) && !self.dependencies_to_be_removed.borrow().has(observable.id())
    }

    fn check_iterations(&self) -> bool {
        if self.iteration.get() > 100 {
            errors::on_bug_indicating_error(&Error::bug_indicating(Some(&format!("Autorun '{}' is stuck in an infinite update loop.", self.debug_name()))));
            return true;
        }
        false
    }

    fn ensure_running(&self) {
        if !self.is_running.get() {
            errors::throw(Error::bug_indicating(Some("The reader object cannot be used outside its compute function!")));
        }
    }

    /// `debugRerun()`.
    pub fn debug_rerun(&self) {
        if !self.is_running.get() {
            self.run();
        } else {
            self.state.set(AutorunState::Stale);
        }
    }
}

impl<S: 'static> IDisposable for AutorunObserver<S> {
    fn dispose(&self) {
        if self.disposed.get() {
            return;
        }
        self.disposed.set(true);
        let me = self.as_observer();
        for_each_live(&self.dependencies, |o| {
            o.remove_observer(&me); // chamada externa
            true
        });
        self.dependencies.borrow_mut().clear();
        let store = self.store.borrow().clone();
        if let Some(s) = store {
            s.dispose();
        }
        let delayed = self.delayed_store.borrow().clone();
        if let Some(s) = delayed {
            s.dispose();
        }
        mark_as_disposed(id_of_ref(self));
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

impl<S: 'static> IObserver for AutorunObserver<S> {
    fn begin_update(&self, _observable: &dyn AnyObservable) {
        if self.state.get() == AutorunState::UpToDate {
            self.check_iterations();
            self.state.set(AutorunState::DependenciesMightHaveChanged);
        }
        self.update_count.set(self.update_count.get() + 1);
    }

    fn end_update(&self, _observable: &dyn AnyObservable) {
        try_finally(
            || {
                if self.update_count.get() == 1 {
                    self.iteration.set(1);
                    loop {
                        if self.check_iterations() {
                            return;
                        }
                        if self.state.get() == AutorunState::DependenciesMightHaveChanged {
                            self.state.set(AutorunState::UpToDate);
                            for_each_live(&self.dependencies, |d| {
                                d.report_changes(); // chamada externa
                                // As outras dependências atualizam sob demanda.
                                self.state.get() != AutorunState::Stale
                            });
                        }
                        self.iteration.set(self.iteration.get() + 1);
                        if self.state.get() != AutorunState::UpToDate {
                            self.run(); // chamada externa indireta
                        }
                        if self.state.get() == AutorunState::UpToDate {
                            break;
                        }
                    }
                }
            },
            || self.update_count.set(self.update_count.get() - 1),
        );
        crate::assert::assert_fn(|| self.update_count.get() >= 0);
    }

    fn handle_possible_change(&self, observable: &dyn AnyObservable) {
        if self.state.get() == AutorunState::UpToDate && self.is_dependency(observable) {
            self.check_iterations();
            self.state.set(AutorunState::DependenciesMightHaveChanged);
        }
    }

    fn handle_change(&self, observable: &dyn AnyObservable, change: &dyn Any) {
        if self.is_dependency(observable) {
            let should_react = match &self.change_tracker {
                Some(tracker) => {
                    let summary = self.change_summary.borrow().clone().expect("o resumo existe");
                    errors::catch(|| tracker.handle_change(&Ctx { observable, change }, &summary))
                }
                None => Ok(true),
            };
            match should_react {
                Ok(true) => {
                    self.check_iterations();
                    self.state.set(AutorunState::Stale);
                }
                Ok(false) => {}
                Err(e) => errors::on_bug_indicating_error(&e),
            }
        }
    }

    fn debug_info(&self) -> Option<DebugInfo> {
        Some(DebugInfo {
            source_id: (self as *const Self).cast::<()>() as usize,
            name: self.debug_name(),
            kind: "autorun",
            value: "undefined".into(),
            state: self.state.get().name().into(),
            dependencies: self.dependencies.borrow().snapshot().into_iter().map(DebugNode::Observable).collect(),
            observers: Vec::new(),
        })
    }
}

impl<S: 'static> IReader for AutorunObserver<S> {
    fn read_observable_erased(&self, observable: ObservableRef, get: &mut dyn FnMut()) {
        self.ensure_running();
        // Caso a função de execução descarte o autorun.
        if self.disposed.get() {
            get(); // chamada externa
            return;
        }
        observable.add_observer(self.as_observer()); // chamada externa
        get(); // chamada externa
        self.dependencies.borrow_mut().add(observable_id(&observable), observable.clone());
        self.dependencies_to_be_removed.borrow_mut().delete(observable_id(&observable));
    }
}

impl<S: 'static> IReaderWithStore for AutorunObserver<S> {
    fn store(&self) -> Rc<DisposableStore> {
        self.ensure_running();
        if self.disposed.get() {
            errors::throw(Error::bug_indicating(Some("Cannot access store after dispose")));
        }
        self.store.borrow_mut().get_or_insert_with(DisposableStore::new).clone()
    }

    fn delayed_store(&self) -> Rc<DisposableStore> {
        self.ensure_running();
        if self.disposed.get() {
            errors::throw(Error::bug_indicating(Some("Cannot access store after dispose")));
        }
        self.delayed_store.borrow_mut().get_or_insert_with(DisposableStore::new).clone()
    }
}
