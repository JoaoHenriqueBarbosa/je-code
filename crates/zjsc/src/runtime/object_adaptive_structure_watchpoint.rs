//! Porte de `runtime/ObjectAdaptiveStructureWatchpoint.h`, instanciado para `InlineWatchpointSet`.
//!
//! É o watchpoint das condições que só dependem da estrutura (ausência de propriedade): vigia o
//! `transitionWatchpointSet` da estrutura do objeto e, ao disparar, ou reinstala (se a condição ainda é
//! vigiável na nova estrutura) ou dispara o set. No C++ o próprio objeto é o `Watchpoint`; aqui o
//! `Watchpoint` é um `Rc` que o `install` coloca no set da estrutura. O `m_owner` serve só para o
//! `isPendingDestruction()`, sempre falso sem GC, então não é guardado.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use crate::bytecode::property_condition::{ObjectPropertyCondition, PropertyConditionKind, WatchabilityEffort};
use crate::bytecode::watchpoint::{
    FireDetail, InlineWatchpointSet, StringFireDetail, Watchpoint, WatchpointBody, WatchpointRef, WatchpointState, WatchpointType,
};
use crate::runtime::object_property_change_adaptive_watchpoint::ObjectPropertyChangeAdaptiveWatchpoint;
use crate::runtime::vm::VM;

/// O que `JSGlobalObject::tryInstallSpeciesWatchpoint<SpeciesWatchpoint>` exige do parâmetro de template:
/// construir-se de `(owner, condition, set)` e `install(vm)`. As globais Array, Promise, RegExp e
/// ArrayBuffer usam o `ObjectPropertyChangeAdaptiveWatchpoint` (condição de equivalência); os construtores
/// de TypedArray concretos usam o `ObjectAdaptiveStructureWatchpoint` (condição de ausência).
pub trait SpeciesWatchpoint: Sized {
    fn create(condition: ObjectPropertyCondition, watchpoint_set: Rc<RefCell<InlineWatchpointSet>>) -> Self;
    fn install(&self, vm: &VM);
}

impl SpeciesWatchpoint for ObjectPropertyChangeAdaptiveWatchpoint {
    fn create(condition: ObjectPropertyCondition, watchpoint_set: Rc<RefCell<InlineWatchpointSet>>) -> Self {
        ObjectPropertyChangeAdaptiveWatchpoint::new(condition, watchpoint_set)
    }

    fn install(&self, vm: &VM) {
        ObjectPropertyChangeAdaptiveWatchpoint::install(self, vm);
    }
}

impl SpeciesWatchpoint for ObjectAdaptiveStructureWatchpoint {
    fn create(condition: ObjectPropertyCondition, watchpoint_set: Rc<RefCell<InlineWatchpointSet>>) -> Self {
        ObjectAdaptiveStructureWatchpoint::new(condition, watchpoint_set)
    }

    fn install(&self, vm: &VM) {
        ObjectAdaptiveStructureWatchpoint::install(self, vm);
    }
}

/// `fireInternal`: o corpo do `Watchpoint`.
struct Body {
    key: ObjectPropertyCondition,
    watchpoint_set: Rc<RefCell<InlineWatchpointSet>>,
    /// O próprio `Watchpoint` (o `this` do C++), para reinstalar.
    this: Rc<RefCell<Weak<Watchpoint>>>,
}

fn install_on_structure(key: &ObjectPropertyCondition, vm: &VM, watchpoint: &WatchpointRef) {
    assert!(key.is_watchable(vm, WatchabilityEffort::MakeNoChanges));
    key.object().structure().add_transition_watchpoint(Some(watchpoint.clone()));
}

impl WatchpointBody for Body {
    fn fire_internal(&mut self, vm: &VM, _detail: &dyn FireDetail) {
        if self.key.is_watchable(vm, WatchabilityEffort::EnsureWatchability) {
            let this = self.this.borrow().upgrade().expect("o ObjectAdaptiveStructureWatchpoint está vivo enquanto dispara");
            install_on_structure(&self.key, vm, &this);
            return;
        }
        InlineWatchpointSet::fire_all_shared(&self.watchpoint_set, vm, &StringFireDetail::new("Object Property is added."));
    }
}

/// `class ObjectAdaptiveStructureWatchpoint`.
pub struct ObjectAdaptiveStructureWatchpoint {
    key: ObjectPropertyCondition,
    watchpoint: WatchpointRef,
}

impl std::fmt::Debug for ObjectAdaptiveStructureWatchpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObjectAdaptiveStructureWatchpoint").field("key", &self.key).finish()
    }
}

impl ObjectAdaptiveStructureWatchpoint {
    /// O construtor: os quatro `RELEASE_ASSERT`.
    pub fn new(key: ObjectPropertyCondition, watchpoint_set: Rc<RefCell<InlineWatchpointSet>>) -> ObjectAdaptiveStructureWatchpoint {
        assert!(key.kind() != PropertyConditionKind::Equivalence);
        assert!(key.condition().watching_requires_structure_transition_watchpoint());
        assert!(!key.condition().watching_requires_replacement_watchpoint());
        assert!(watchpoint_set.borrow().state() == WatchpointState::IsWatched);
        let this = Rc::new(RefCell::new(Weak::new()));
        let watchpoint = Watchpoint::new(
            WatchpointType::ObjectAdaptiveStructure,
            Box::new(Body { key: key.clone(), watchpoint_set, this: Rc::clone(&this) }),
        );
        *this.borrow_mut() = Rc::downgrade(&watchpoint);
        ObjectAdaptiveStructureWatchpoint { key, watchpoint }
    }

    pub fn key(&self) -> &ObjectPropertyCondition {
        &self.key
    }

    /// `install(vm)`.
    pub fn install(&self, vm: &VM) {
        install_on_structure(&self.key, vm, &self.watchpoint);
    }
}
