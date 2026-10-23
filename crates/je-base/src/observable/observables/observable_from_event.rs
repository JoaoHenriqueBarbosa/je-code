//! Porte de `observableInternal/observables/observableFromEvent.ts`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use super::base_observable::{BaseObservable, ObservableHooks};
use crate::event::Event;
use crate::lifecycle::DisposableRef;
use crate::observable::base::{AnyObservable, DebugInfo, DebugNode, IObservable, ITransaction, Observable, ObserverRef};
use crate::observable::debug_name::{DebugNameData, DebugNameOptions, DebugOwner};
use crate::observable::deps::{EqualityComparer, StrictEquals, strict_equals, try_finally};
use crate::observable::logging::debug_get_dependency_graph::format_value;
use crate::observable::transaction::subtransaction;

type GetTransaction = Rc<dyn Fn() -> Option<Rc<dyn ITransaction>>>;

thread_local! {
    static GLOBAL_TRANSACTION: RefCell<Option<Rc<dyn ITransaction>>> = const { RefCell::new(None) };
}

/// As opções de `observableFromEventOpts`.
pub struct FromEventOptions<T> {
    pub debug: DebugNameOptions,
    pub equals_fn: Option<EqualityComparer<T>>,
    pub get_transaction: Option<GetTransaction>,
}

impl<T> Default for FromEventOptions<T> {
    fn default() -> Self {
        Self { debug: DebugNameOptions::default(), equals_fn: None, get_transaction: None }
    }
}

fn global_transaction() -> Option<Rc<dyn ITransaction>> {
    GLOBAL_TRANSACTION.with(|g| g.borrow().clone())
}

/// `observableFromEvent(event, getValue)`. `get_value` recebe `None` quando chamado sem evento.
pub fn observable_from_event<T: Clone + StrictEquals + 'static, A: 'static>(event: Event<A>, get_value: impl Fn(Option<&A>) -> T + 'static) -> IObservable<T> {
    observable_from_event_opts(FromEventOptions::default(), event, get_value)
}

/// `observableFromEvent(owner, event, getValue)`.
pub fn observable_from_event_owned<T: Clone + StrictEquals + 'static, A: 'static>(
    owner: DebugOwner,
    event: Event<A>,
    get_value: impl Fn(Option<&A>) -> T + 'static,
) -> IObservable<T> {
    observable_from_event_opts(FromEventOptions { debug: DebugNameOptions::owned(owner), ..FromEventOptions::default() }, event, get_value)
}

/// `observableFromEventOpts`.
pub fn observable_from_event_opts<T: Clone + StrictEquals + 'static, A: 'static>(
    options: FromEventOptions<T>,
    event: Event<A>,
    get_value: impl Fn(Option<&A>) -> T + 'static,
) -> IObservable<T> {
    let get_tx = options.get_transaction;
    FromEventObservable::new(
        options.debug.into(),
        event,
        Rc::new(get_value),
        Rc::new(move || get_tx.as_ref().and_then(|f| f()).or_else(global_transaction)),
        options.equals_fn.unwrap_or_else(strict_equals),
    )
}

/// `observableFromEvent.batchEventsGlobally(tx, fn)`.
pub fn batch_events_globally(tx: Rc<dyn ITransaction>, f: impl FnOnce()) {
    let did_set = GLOBAL_TRANSACTION.with(|g| {
        let mut g = g.borrow_mut();
        if g.is_none() {
            *g = Some(tx);
            true
        } else {
            false
        }
    });
    try_finally(f, || {
        if did_set {
            GLOBAL_TRANSACTION.with(|g| *g.borrow_mut() = None);
        }
    });
}

/// `FromEventObservable<TArgs, T>`.
pub struct FromEventObservable<A: 'static, T: Clone + 'static> {
    base: BaseObservable,
    me: Weak<Self>,
    debug: DebugNameData,
    event: Event<A>,
    get_value: Rc<dyn Fn(Option<&A>) -> T>,
    get_transaction: GetTransaction,
    equality: EqualityComparer<T>,
    value: RefCell<Option<T>>,
    has_value: Cell<bool>,
    subscription: RefCell<Option<DisposableRef>>,
}

impl<A: 'static, T: Clone + 'static> FromEventObservable<A, T> {
    fn new(
        debug: DebugNameData,
        event: Event<A>,
        get_value: Rc<dyn Fn(Option<&A>) -> T>,
        get_transaction: GetTransaction,
        equality: EqualityComparer<T>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            base: BaseObservable::new(me.clone() as Weak<dyn AnyObservable>),
            me: me.clone(),
            debug,
            event,
            get_value,
            get_transaction,
            equality,
            value: RefCell::new(None),
            has_value: Cell::new(false),
            subscription: RefCell::new(None),
        })
    }

    fn handle_event(&self, args: Option<&A>) {
        let new_value = (self.get_value)(args);
        let old_value = self.value.borrow().clone();
        let had_value = self.has_value.get();
        let did_change = !had_value || !old_value.as_ref().is_some_and(|old| (self.equality)(old, &new_value));
        if did_change {
            *self.value.borrow_mut() = Some(new_value);
            self.has_value.set(true);
            if had_value {
                let tx = (self.get_transaction)();
                subtransaction(tx.as_deref(), |tx| {
                    let me = self.base.to_any_observable();
                    self.base.for_each_observer(|o| {
                        tx.update_observer(o.clone(), me.clone());
                        o.handle_change(&*me, &());
                    });
                });
            }
        }
    }
}

impl<A: 'static, T: Clone + 'static> ObservableHooks for FromEventObservable<A, T> {
    fn on_first_observer_added(&self) {
        let me = self.me.clone();
        let sub = self.event.listen(move |args| {
            if let Some(this) = me.upgrade() {
                this.handle_event(Some(args));
            }
        });
        *self.subscription.borrow_mut() = Some(sub);
    }

    fn on_last_observer_removed(&self) {
        let sub = self.subscription.borrow_mut().take();
        if let Some(sub) = sub {
            sub.dispose();
        }
        self.has_value.set(false);
        *self.value.borrow_mut() = None;
    }
}

impl<A: 'static, T: Clone + 'static> AnyObservable for FromEventObservable<A, T> {
    fn report_changes(&self) {
        self.get();
    }

    fn add_observer(&self, observer: ObserverRef) {
        self.base.add_observer(observer, self);
    }

    fn remove_observer(&self, observer: &ObserverRef) {
        self.base.remove_observer(observer, self);
    }

    fn debug_name(&self) -> String {
        match self.debug.get_debug_name() {
            Some(n) => format!("From Event: {n}"),
            None => "From Event".into(),
        }
    }

    fn to_any_observable(&self) -> Rc<dyn AnyObservable> {
        self.base.to_any_observable()
    }

    fn debug_info(&self) -> Option<DebugInfo> {
        let value = self.value.borrow();
        Some(DebugInfo {
            source_id: self.id(),
            name: self.debug_name(),
            kind: "fromEvent",
            value: match &*value {
                Some(v) => format_value(v, 50),
                None => "undefined".into(),
            },
            state: if self.has_value.get() { "upToDate" } else { "initial" }.into(),
            dependencies: Vec::new(),
            observers: self.base.observers().into_iter().map(DebugNode::Observer).collect(),
        })
    }
}

impl<A: 'static, T: Clone + 'static> Observable<T> for FromEventObservable<A, T> {
    fn get(&self) -> T {
        if self.subscription.borrow().is_some() {
            if !self.has_value.get() {
                self.handle_event(None);
            }
            self.value.borrow().clone().expect("handle_event guardou o valor")
        } else {
            // Sem cache: não há inscritos para mantê-lo atualizado.
            (self.get_value)(None)
        }
    }

    fn to_observable(&self) -> IObservable<T> {
        self.me.upgrade().expect("vivo")
    }
}

use crate::lifecycle::IDisposable;
