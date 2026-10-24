//! Porte de `bytecode/Watchpoint.h` e `bytecode/Watchpoint.cpp`: `FireDetail`, `StringFireDetail`,
//! `LazyFireDetail`, `Watchpoint`, `WatchpointState`, `WatchpointSet`, `InlineWatchpointSet` e
//! `DeferredWatchpointFire`.
//!
//! `ENABLE(JIT)` e `ENABLE(DFG_JIT)` são ignorados (`CONVENTIONS.md`): o `JSC_WATCHPOINT_TYPES`
//! vale o `JSC_WATCHPOINT_TYPES_WITHOUT_JIT`, sem `StructureTransitionPropertyInlineCacheClearing`,
//! `PropertyInlineCacheClearing` e `AdaptiveStructure`.
//!
//! Divergências:
//!
//! - O `Watchpoint` do C++ é a base de uma hierarquia (`CodeBlockJettisoningWatchpoint`,
//!   `ObjectAdaptiveStructureWatchpoint`, ...) e o `runWithDowncast` despacha por `m_type` para o
//!   `fireInternal` da classe derivada. Aqui a derivada é um `Box<dyn WatchpointBody>` guardado no
//!   `Watchpoint`, e o `Type` fica em `ty` (a derivada o confere com `watchpoint_type`). O
//!   `operator delete` com `freeAfterDestruction` é o `Drop` do `Rc`.
//! - A lista intrusiva (`BasicRawSentinelNode`, `m_set`) é um `VecDeque<WatchpointRef>`: o set é
//!   dono dos watchpoints enquanto eles estão nele (`WatchpointRef = Rc<Watchpoint>`), e a
//!   identidade é `Rc::ptr_eq`. Por isso o `~Watchpoint` (que se remove da lista se o watchpoint
//!   morre antes do disparo) não tem o que fazer: o watchpoint só morre depois de sair do set.
//!   `is_on_list` é o flag que o `isOnList()` do nó intrusivo responde; `WatchpointSet::remove`
//!   é o `remove()` do nó.
//! - `WatchpointSet` é `ThreadSafeRefCounted`: `Rc<RefCell<WatchpointSet>>` (`WatchpointSetRef`).
//!   O `~WatchpointSet` desliga os watchpoints restantes sem disparar (`Drop`).
//! - `WTF::storeStoreFence`, `isCompilationThread` e `refCountDebugger` não têm efeito num único
//!   fio: somem. `DeferGCForAWhile` no `fireAllWatchpoints` some porque não há GC em andamento
//!   durante o disparo (o `Heap` do porte só coleta em pontos de segurança explícitos).
//! - `InlineWatchpointSet` guarda `m_data` como um enum `Thin(state)`/`Fat(WatchpointSetRef)` em vez
//!   de um `uintptr_t` com o bit `IsThinFlag`; `encode_state` e `IS_THIN_FLAG` mantêm a codificação
//!   para quem a lê (o LLInt do C++ lê `m_data`; o interpretador do porte consulta pelos métodos).
//!   O `OBJECT_OFFSETOF` (`offsetOfState`, `offsetOfData`) é offset de JIT/LLInt e não existe.
//! - `FireDetail::dump` escreve num `fmt::Write` no lugar do `PrintStream`.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fmt;
use std::rc::Rc;

use crate::runtime::vm::VM;

/// `class FireDetail`.
pub trait FireDetail {
    /// `virtual void dump(PrintStream&) const { }`.
    fn dump(&self, _out: &mut dyn fmt::Write) -> fmt::Result {
        Ok(())
    }
}

/// `class StringFireDetail`.
#[derive(Clone, Copy, Debug)]
pub struct StringFireDetail<'a> {
    string: &'a str,
}

impl<'a> StringFireDetail<'a> {
    pub fn new(string: &'a str) -> StringFireDetail<'a> {
        StringFireDetail { string }
    }
}

impl FireDetail for StringFireDetail<'_> {
    fn dump(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        out.write_str(self.string)
    }
}

/// `class LazyFireDetail<Functor>`.
pub struct LazyFireDetail<F: Fn(&mut dyn fmt::Write) -> fmt::Result> {
    functor: F,
}

impl<F: Fn(&mut dyn fmt::Write) -> fmt::Result> LazyFireDetail<F> {
    pub fn new(functor: F) -> LazyFireDetail<F> {
        LazyFireDetail { functor }
    }
}

impl<F: Fn(&mut dyn fmt::Write) -> fmt::Result> FireDetail for LazyFireDetail<F> {
    fn dump(&self, out: &mut dyn fmt::Write) -> fmt::Result {
        (self.functor)(out)
    }
}

/// `Watchpoint::Type` (`JSC_WATCHPOINT_TYPES_WITHOUT_JIT`, na ordem do C++).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WatchpointType {
    AdaptiveInferredPropertyValueStructure,
    AdaptiveInferredPropertyValueProperty,
    CodeBlockJettisoning,
    LLIntPrototypeLoadAdaptiveStructure,
    FunctionRareDataAllocationProfileClearing,
    CachedSpecialPropertyAdaptiveStructure,
    StructureChainInvalidation,
    ObjectAdaptiveStructure,
    Chained,
}

/// A parte derivada de um `Watchpoint`: o `fireInternal` que o `runWithDowncast` chama.
pub trait WatchpointBody {
    fn fire_internal(&mut self, vm: &VM, detail: &dyn FireDetail);
}

/// `RefPtr`-equivalente de um `Watchpoint*` guardado num `WatchpointSet`.
pub type WatchpointRef = Rc<Watchpoint>;

/// `class Watchpoint`.
pub struct Watchpoint {
    /// `m_type`.
    ty: WatchpointType,
    /// `isOnList()` do `BasicRawSentinelNode`.
    on_list: Cell<bool>,
    body: RefCell<Box<dyn WatchpointBody>>,
}

impl fmt::Debug for Watchpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Watchpoint").field("ty", &self.ty).field("on_list", &self.on_list.get()).finish()
    }
}

impl Watchpoint {
    /// `Watchpoint(Type)` mais a derivada.
    pub fn new(ty: WatchpointType, body: Box<dyn WatchpointBody>) -> WatchpointRef {
        Rc::new(Watchpoint { ty, on_list: Cell::new(false), body: RefCell::new(body) })
    }

    pub fn watchpoint_type(&self) -> WatchpointType {
        self.ty
    }

    /// `isOnList`.
    pub fn is_on_list(&self) -> bool {
        self.on_list.get()
    }

    /// `Watchpoint::fire`.
    fn fire(&self, vm: &VM, detail: &dyn FireDetail) {
        assert!(!self.is_on_list());
        self.body.borrow_mut().fire_internal(vm, detail);
    }
}

/// `enum WatchpointState : uint8_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WatchpointState {
    ClearWatchpoint = 0,
    IsWatched = 1,
    IsInvalidated = 2,
}

impl fmt::Display for WatchpointState {
    /// `printInternal(PrintStream&, WatchpointState)`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            WatchpointState::ClearWatchpoint => "ClearWatchpoint",
            WatchpointState::IsWatched => "IsWatched",
            WatchpointState::IsInvalidated => "IsInvalidated",
        })
    }
}

/// `RefPtr<WatchpointSet>`.
pub type WatchpointSetRef = Rc<RefCell<WatchpointSet>>;

/// `class WatchpointSet`.
#[derive(Debug)]
pub struct WatchpointSet {
    /// `m_state`.
    state: WatchpointState,
    /// `m_setIsNotEmpty`.
    set_is_not_empty: bool,
    /// `m_set`.
    set: VecDeque<WatchpointRef>,
}

impl WatchpointSet {
    /// `WatchpointSet::create`.
    pub fn create(state: WatchpointState) -> WatchpointSetRef {
        Rc::new(RefCell::new(WatchpointSet::new(state)))
    }

    /// `WatchpointSet(WatchpointState)`.
    pub fn new(state: WatchpointState) -> WatchpointSet {
        WatchpointSet { state, set_is_not_empty: false, set: VecDeque::new() }
    }

    pub fn state(&self) -> WatchpointState {
        self.state
    }

    pub fn is_still_valid(&self) -> bool {
        self.state() != WatchpointState::IsInvalidated
    }

    pub fn has_been_invalidated(&self) -> bool {
        !self.is_still_valid()
    }

    /// `add`: ignora o nulo.
    pub fn add(&mut self, watchpoint: Option<WatchpointRef>) {
        debug_assert!(self.state() != WatchpointState::IsInvalidated);
        let Some(watchpoint) = watchpoint else {
            return;
        };
        watchpoint.on_list.set(true);
        self.set.push_back(watchpoint);
        self.set_is_not_empty = true;
        self.state = WatchpointState::IsWatched;
    }

    /// O `remove()` do nó intrusivo: tira o watchpoint do set sem disparar.
    pub fn remove(&mut self, watchpoint: &WatchpointRef) {
        if let Some(index) = self.set.iter().position(|w| Rc::ptr_eq(w, watchpoint)) {
            self.set.remove(index);
            watchpoint.on_list.set(false);
        }
    }

    pub fn start_watching(&mut self) {
        debug_assert!(self.state != WatchpointState::IsInvalidated);
        if self.state == WatchpointState::IsWatched {
            return;
        }
        self.state = WatchpointState::IsWatched;
    }

    pub fn fire_all(&mut self, vm: &VM, fire_details: &dyn FireDetail) {
        if self.state != WatchpointState::IsWatched {
            return;
        }
        self.fire_all_slow(vm, fire_details);
    }

    pub fn touch(&mut self, vm: &VM, detail: &dyn FireDetail) {
        if self.state() == WatchpointState::ClearWatchpoint {
            self.start_watching();
        } else {
            self.fire_all(vm, detail);
        }
    }

    /// `touch(VM&, const char*)`.
    pub fn touch_with_reason(&mut self, vm: &VM, reason: &str) {
        self.touch(vm, &StringFireDetail::new(reason));
    }

    pub fn invalidate(&mut self, vm: &VM, detail: &dyn FireDetail) {
        if self.state() == WatchpointState::IsWatched {
            self.fire_all(vm, detail);
        }
        self.state = WatchpointState::IsInvalidated;
    }

    /// `invalidate(VM&, const char*)`.
    pub fn invalidate_with_reason(&mut self, vm: &VM, reason: &str) {
        self.invalidate(vm, &StringFireDetail::new(reason));
    }

    pub fn is_being_watched(&self) -> bool {
        self.set_is_not_empty
    }

    /// `fireAll` num set compartilhado: o C++ dispara com o set só em memória, e os watchpoints adaptativos
    /// consultam e mexem em sets (inclusive neste) de dentro do `fireInternal`. Aqui o `RefCell` só fica
    /// emprestado entre um watchpoint e outro, nunca durante o disparo.
    pub fn fire_all_shared(set: &WatchpointSetRef, vm: &VM, detail: &dyn FireDetail) {
        {
            let mut guard = set.borrow_mut();
            if guard.state != WatchpointState::IsWatched {
                return;
            }
            guard.state = WatchpointState::IsInvalidated; // Antes de tudo. Necessário para os watchpoints adaptativos.
        }
        loop {
            let next = set.borrow_mut().set.pop_front();
            let Some(watchpoint) = next else {
                break;
            };
            debug_assert!(watchpoint.is_on_list());
            watchpoint.on_list.set(false);
            watchpoint.fire(vm, detail);
        }
    }

    /// `fireAllSlow(VM&, const FireDetail&)`: só se `isWatched`.
    pub fn fire_all_slow(&mut self, vm: &VM, detail: &dyn FireDetail) {
        debug_assert!(self.state() == WatchpointState::IsWatched);
        self.state = WatchpointState::IsInvalidated; // Antes de tudo. Necessário para os watchpoints adaptativos.
        self.fire_all_watchpoints(vm, detail);
    }

    /// `fireAllSlow(VM&, DeferredWatchpointFire*)`.
    pub fn fire_all_slow_deferred(&mut self, _vm: &VM, deferred_watchpoints: &mut DeferredWatchpointFire) {
        debug_assert!(self.state() == WatchpointState::IsWatched);
        deferred_watchpoints.take_watchpoints_to_fire(self);
        self.state = WatchpointState::IsInvalidated; // Depois da transferência, para o deferred receber o estado atual.
    }

    /// `fireAllSlow(VM&, const char*)`.
    pub fn fire_all_slow_with_reason(&mut self, vm: &VM, reason: &str) {
        self.fire_all_slow(vm, &StringFireDetail::new(reason));
    }

    fn fire_all_watchpoints(&mut self, vm: &VM, detail: &dyn FireDetail) {
        // Os adaptativos precisam ver este set já invalidado.
        assert!(self.has_been_invalidated());

        while let Some(watchpoint) = self.set.pop_front() {
            debug_assert!(watchpoint.is_on_list());
            // Remover antes de disparar permite watchpoints "adaptativos" que se adicionam a outro
            // set quando disparam.
            watchpoint.on_list.set(false);
            watchpoint.fire(vm, detail);
        }
    }

    /// `take`.
    fn take(&mut self, other: &mut WatchpointSet) {
        debug_assert!(self.state() == WatchpointState::ClearWatchpoint);
        self.set.append(&mut other.set);
        self.set_is_not_empty = other.set_is_not_empty;
        self.state = other.state;
        other.set_is_not_empty = false;
    }
}

impl Drop for WatchpointSet {
    /// `~WatchpointSet`: desliga todos os watchpoints, sem disparar.
    fn drop(&mut self) {
        while let Some(watchpoint) = self.set.pop_front() {
            watchpoint.on_list.set(false);
        }
    }
}

/// `InlineWatchpointSet::m_data`: o estado fino ou o `WatchpointSet*` inflado.
#[derive(Debug)]
enum InlineData {
    Thin(WatchpointState),
    Fat(WatchpointSetRef),
}

/// `class InlineWatchpointSet`.
#[derive(Debug)]
pub struct InlineWatchpointSet {
    data: InlineData,
}

impl InlineWatchpointSet {
    /// `IsThinFlag`.
    pub const IS_THIN_FLAG: usize = 1;
    const STATE_SHIFT: usize = 1;

    /// `encodeState`.
    pub const fn encode_state(state: WatchpointState) -> usize {
        ((state as usize) << Self::STATE_SHIFT) | Self::IS_THIN_FLAG
    }

    pub fn new(state: WatchpointState) -> InlineWatchpointSet {
        InlineWatchpointSet { data: InlineData::Thin(state) }
    }

    fn is_thin(&self) -> bool {
        matches!(self.data, InlineData::Thin(_))
    }

    pub fn is_fat(&self) -> bool {
        !self.is_thin()
    }

    pub fn state(&self) -> WatchpointState {
        match &self.data {
            InlineData::Fat(fat) => fat.borrow().state(),
            InlineData::Thin(state) => *state,
        }
    }

    pub fn has_been_invalidated(&self) -> bool {
        self.state() == WatchpointState::IsInvalidated
    }

    pub fn is_still_valid(&self) -> bool {
        !self.has_been_invalidated()
    }

    pub fn add(&mut self, watchpoint: Option<WatchpointRef>) {
        self.inflate().borrow_mut().add(watchpoint);
    }

    pub fn start_watching(&mut self) {
        match &mut self.data {
            InlineData::Fat(fat) => fat.borrow_mut().start_watching(),
            InlineData::Thin(state) => {
                debug_assert!(*state != WatchpointState::IsInvalidated);
                *state = WatchpointState::IsWatched;
            }
        }
    }

    pub fn fire_all(&mut self, vm: &VM, fire_details: &dyn FireDetail) {
        match &mut self.data {
            InlineData::Fat(fat) => fat.borrow_mut().fire_all(vm, fire_details),
            InlineData::Thin(state) => {
                if *state == WatchpointState::ClearWatchpoint {
                    return;
                }
                *state = WatchpointState::IsInvalidated;
            }
        }
    }

    /// `fireAll` de um `InlineWatchpointSet` compartilhado (`RefCell`): o set inflado dispara por
    /// `WatchpointSet::fire_all_shared`, sem manter emprestado o `RefCell` de fora durante o disparo.
    pub fn fire_all_shared(cell: &RefCell<InlineWatchpointSet>, vm: &VM, fire_details: &dyn FireDetail) {
        let fat = cell.borrow().inflated_set_concurrently();
        match fat {
            Some(fat) => WatchpointSet::fire_all_shared(&fat, vm, fire_details),
            None => cell.borrow_mut().fire_all(vm, fire_details),
        }
    }

    /// O `remove()` do nó intrusivo do watchpoint, para o set inflado que o contém (o fino não tem lista).
    pub fn remove(&mut self, watchpoint: &WatchpointRef) {
        if let InlineData::Fat(fat) = &self.data {
            fat.borrow_mut().remove(watchpoint);
        }
    }

    /// `fireAll(VM&, const char*)`.
    pub fn fire_all_with_reason(&mut self, vm: &VM, reason: &str) {
        self.fire_all(vm, &StringFireDetail::new(reason));
    }

    pub fn invalidate(&mut self, vm: &VM, detail: &dyn FireDetail) {
        match &mut self.data {
            InlineData::Fat(fat) => fat.borrow_mut().invalidate(vm, detail),
            InlineData::Thin(state) => *state = WatchpointState::IsInvalidated,
        }
    }

    pub fn touch(&mut self, vm: &VM, detail: &dyn FireDetail) {
        match &mut self.data {
            InlineData::Fat(fat) => {
                if fat.borrow().state() == WatchpointState::IsInvalidated {
                    return;
                }
                fat.borrow_mut().touch(vm, detail);
            }
            InlineData::Thin(state) => {
                if *state == WatchpointState::IsInvalidated {
                    return;
                }
                if *state == WatchpointState::ClearWatchpoint {
                    *state = WatchpointState::IsWatched;
                } else {
                    *state = WatchpointState::IsInvalidated;
                }
            }
        }
    }

    /// `touch(VM&, const char*)`.
    pub fn touch_with_reason(&mut self, vm: &VM, reason: &str) {
        self.touch(vm, &StringFireDetail::new(reason));
    }

    pub fn is_being_watched(&self) -> bool {
        match &self.data {
            InlineData::Fat(fat) => fat.borrow().is_being_watched(),
            InlineData::Thin(_) => false,
        }
    }

    /// `inflate`.
    pub fn inflate(&mut self) -> WatchpointSetRef {
        if let InlineData::Fat(fat) = &self.data {
            return fat.clone();
        }
        self.inflate_slow()
    }

    /// `inflatedSetConcurrently`.
    pub fn inflated_set_concurrently(&self) -> Option<WatchpointSetRef> {
        match &self.data {
            InlineData::Fat(fat) => Some(fat.clone()),
            InlineData::Thin(_) => None,
        }
    }

    fn inflate_slow(&mut self) -> WatchpointSetRef {
        let InlineData::Thin(state) = self.data else {
            unreachable!("inflateSlow chamado num set já inflado");
        };
        let fat = WatchpointSet::create(state);
        self.data = InlineData::Fat(fat.clone());
        fat
    }
}

/// `class DeferredWatchpointFire`.
#[derive(Debug)]
pub struct DeferredWatchpointFire {
    /// `m_watchpointsToFire`.
    watchpoints_to_fire: WatchpointSet,
}

impl DeferredWatchpointFire {
    pub fn new() -> DeferredWatchpointFire {
        DeferredWatchpointFire { watchpoints_to_fire: WatchpointSet::new(WatchpointState::ClearWatchpoint) }
    }

    /// `takeWatchpointsToFire`.
    pub fn take_watchpoints_to_fire(&mut self, watchpoints_to_fire: &mut WatchpointSet) {
        debug_assert!(self.watchpoints_to_fire.state() == WatchpointState::ClearWatchpoint);
        debug_assert!(watchpoints_to_fire.state() == WatchpointState::IsWatched);
        self.watchpoints_to_fire.take(watchpoints_to_fire);
    }

    /// `watchpointsToFire()` (protegido: as derivadas disparam o set).
    pub fn watchpoints_to_fire(&mut self) -> &mut WatchpointSet {
        &mut self.watchpoints_to_fire
    }
}

impl Default for DeferredWatchpointFire {
    fn default() -> DeferredWatchpointFire {
        DeferredWatchpointFire::new()
    }
}
