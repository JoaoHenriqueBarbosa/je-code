//! Tradução de `runtime/InferredValue.h` e `runtime/InferredValueInlines.h`.
//!
//! DIVERGÊNCIAS:
//!
//! - `m_data` (`uintptr_t` com o bit `IsThinFlag`, o estado e o ponteiro do valor, ou o ponteiro do
//!   `InferredValueWatchpointSet`) é o enum `Data`: `Thin { state, value }` ou `Fat(Rc<..>)`. As
//!   constantes `IsThinFlag`/`StateMask`/`StateShift`/`ValueMask` e `encodeState`/`decodeState` somem.
//! - O ponteiro para `JSCellType` é fraco (`Weak<T>`): no C++ o valor inferido é uma referência que o
//!   GC deixa de enxergar (o `reconcileWeakReferencesAtGCEnd` invalida quando o valor morreu), e uma
//!   referência forte aqui formaria ciclo com o dono (`FunctionExecutable` <-> `JSFunction`). Sem GC,
//!   `reconcileWeakReferencesAtGCEnd` testa a morte do valor pela contagem do `Weak`.
//! - `vm.writeBarrier(owner, value)` não existe (sem GC incremental), então o `owner` some.
//! - `InferredValueWatchpointSet` é `ThreadSafeRefCounted` (`adoptRef(...).leakRef()` e `deref()`):
//!   `Rc<RefCell<..>>`; o `freeFat` é o `Drop`. `storeStoreFence` e `isCompilationThread` somem.

use std::cell::RefCell;
use std::fmt;
use std::rc::{Rc, Weak};

use crate::bytecode::watchpoint::{FireDetail, StringFireDetail, WatchpointRef, WatchpointSet, WatchpointState};
use crate::runtime::vm::VM;

/// `InferredValue<JSCellType>::InferredValueWatchpointSet`.
pub struct InferredValueWatchpointSet<T> {
    /// A base `WatchpointSet`.
    set: WatchpointSet,
    value: Option<Weak<T>>,
}

impl<T> InferredValueWatchpointSet<T> {
    /// `InferredValueWatchpointSet(WatchpointState, JSCellType*)`.
    fn new(state: WatchpointState, value: Option<Weak<T>>) -> InferredValueWatchpointSet<T> {
        InferredValueWatchpointSet { set: WatchpointSet::new(state), value }
    }

    /// `inferredValue() const`.
    pub fn inferred_value(&self) -> Option<Rc<T>> {
        self.value.as_ref().and_then(Weak::upgrade)
    }

    /// `state()` da base.
    pub fn state(&self) -> WatchpointState {
        self.set.state()
    }

    /// `isBeingWatched()` da base.
    pub fn is_being_watched(&self) -> bool {
        self.set.is_being_watched()
    }

    /// `add(Watchpoint*)` da base.
    pub fn add(&mut self, watchpoint: Option<WatchpointRef>) {
        self.set.add(watchpoint);
    }

    /// `invalidate(VM&, const FireDetail&)`.
    pub fn invalidate(&mut self, vm: &VM, detail: &dyn FireDetail) {
        self.value = None;
        self.set.invalidate(vm, detail);
    }

    /// `notifyWriteSlow(VM&, JSCell*, JSCellType*, const FireDetail&)`.
    fn notify_write_slow(&mut self, vm: &VM, value: &Rc<T>, detail: &dyn FireDetail) {
        match self.set.state() {
            WatchpointState::ClearWatchpoint => {
                self.value = Some(Rc::downgrade(value));
                self.set.start_watching();
            }
            WatchpointState::IsWatched => {
                debug_assert!(self.value.is_some());
                if self.value.as_ref().is_some_and(|stored| std::ptr::eq(stored.as_ptr(), Rc::as_ptr(value))) {
                    return;
                }
                self.invalidate(vm, detail);
            }
            WatchpointState::IsInvalidated => {
                debug_assert!(false, "ASSERT_NOT_REACHED");
            }
        }
    }
}

/// `m_data`: o estado fino (`isThin`) ou o conjunto inflado.
enum Data<T> {
    Thin { state: WatchpointState, value: Option<Weak<T>> },
    Fat(Rc<RefCell<InferredValueWatchpointSet<T>>>),
}

/// `class InferredValue<JSCellType>`.
pub struct InferredValue<T> {
    data: Data<T>,
}

impl<T> Default for InferredValue<T> {
    /// `explicit InferredValue()`: `m_data(encodeState(ClearWatchpoint))`.
    fn default() -> InferredValue<T> {
        InferredValue { data: Data::Thin { state: WatchpointState::ClearWatchpoint, value: None } }
    }
}

impl<T> InferredValue<T> {
    /// `inferredValue()`.
    pub fn inferred_value(&self) -> Option<Rc<T>> {
        match &self.data {
            Data::Fat(fat) => fat.borrow().inferred_value(),
            Data::Thin { value, .. } => value.as_ref().and_then(Weak::upgrade),
        }
    }

    /// `state() const`.
    pub fn state(&self) -> WatchpointState {
        match &self.data {
            Data::Fat(fat) => fat.borrow().state(),
            Data::Thin { state, .. } => *state,
        }
    }

    /// `hasBeenInvalidated() const`.
    pub fn has_been_invalidated(&self) -> bool {
        self.state() == WatchpointState::IsInvalidated
    }

    /// `isStillValid() const`.
    pub fn is_still_valid(&self) -> bool {
        !self.has_been_invalidated()
    }

    /// `add(Watchpoint*)`.
    pub fn add(&mut self, watchpoint: Option<WatchpointRef>) {
        self.inflate().borrow_mut().add(watchpoint);
    }

    /// `invalidate(VM&, const FireDetail&)`.
    pub fn invalidate(&mut self, vm: &VM, detail: &dyn FireDetail) {
        match &mut self.data {
            Data::Fat(fat) => fat.borrow_mut().invalidate(vm, detail),
            Data::Thin { .. } => {
                self.data = Data::Thin { state: WatchpointState::IsInvalidated, value: None };
            }
        }
    }

    /// `isBeingWatched() const`.
    pub fn is_being_watched(&self) -> bool {
        match &self.data {
            Data::Fat(fat) => fat.borrow().is_being_watched(),
            Data::Thin { .. } => false,
        }
    }

    /// `notifyWrite(VM&, JSCell*, JSCellType*, const FireDetail&)`.
    pub fn notify_write(&mut self, vm: &VM, value: &Rc<T>, detail: &dyn FireDetail) {
        if self.state() == WatchpointState::IsInvalidated {
            return;
        }
        self.notify_write_slow(vm, value, detail);
    }

    /// `notifyWrite(VM&, JSCell*, JSCellType*, const char* reason)`.
    pub fn notify_write_with_reason(&mut self, vm: &VM, value: &Rc<T>, reason: &str) {
        if self.state() == WatchpointState::IsInvalidated {
            return;
        }
        self.notify_write_slow(vm, value, &StringFireDetail::new(reason));
    }

    /// `reconcileWeakReferencesAtGCEnd(VM&, CollectionScope)`: o valor morto (sem `Rc` vivo) invalida.
    pub fn reconcile_weak_references_at_gc_end(&mut self, vm: &VM) {
        let alive = match &self.data {
            Data::Fat(fat) => fat.borrow().value.as_ref().map(|value| value.strong_count() > 0),
            Data::Thin { value, .. } => value.as_ref().map(|value| value.strong_count() > 0),
        };
        if alive == Some(false) {
            self.invalidate(vm, &StringFireDetail::new("InferredValue clean-up during GC"));
        }
    }

    /// `inflate()`.
    fn inflate(&mut self) -> Rc<RefCell<InferredValueWatchpointSet<T>>> {
        if let Data::Fat(fat) = &self.data {
            return Rc::clone(fat);
        }
        self.inflate_slow()
    }

    /// `inflateSlow()`.
    fn inflate_slow(&mut self) -> Rc<RefCell<InferredValueWatchpointSet<T>>> {
        let Data::Thin { state, value } = &mut self.data else {
            unreachable!("inflateSlow exige o estado fino");
        };
        let fat = Rc::new(RefCell::new(InferredValueWatchpointSet::new(*state, value.take())));
        self.data = Data::Fat(Rc::clone(&fat));
        fat
    }

    /// `notifyWriteSlow(VM&, JSCell*, JSCellType*, const FireDetail&)`.
    fn notify_write_slow(&mut self, vm: &VM, value: &Rc<T>, detail: &dyn FireDetail) {
        let (state, current) = match &mut self.data {
            Data::Fat(fat) => {
                fat.borrow_mut().notify_write_slow(vm, value, detail);
                return;
            }
            Data::Thin { state, value: current } => (*state, current.as_ref().and_then(Weak::upgrade)),
        };

        match state {
            WatchpointState::ClearWatchpoint => {
                self.data = Data::Thin { state: WatchpointState::IsWatched, value: Some(Rc::downgrade(value)) };
            }
            WatchpointState::IsWatched => {
                // O valor morto já teria invalidado no `finalizeUnconditionally` do C++ (`!vm.heap.isMarked`): o
                // `Weak` vazio invalida aqui, na primeira escrita depois da coleta, que é quando a diferença aparece.
                if current.as_ref().is_some_and(|inferred| Rc::ptr_eq(inferred, value)) {
                    return;
                }
                self.invalidate(vm, detail);
            }
            WatchpointState::IsInvalidated => {
                debug_assert!(false, "ASSERT_NOT_REACHED");
            }
        }
    }
}

impl<T> fmt::Debug for InferredValue<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "InferredValue({})", self.state())
    }
}
