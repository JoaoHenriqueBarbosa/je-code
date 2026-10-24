//! Porte de `bytecode/AdaptiveInferredPropertyValueWatchpointBase.{h,cpp}`.
//!
//! No C++ a base é abstrata (`handleFire` e `isValid` virtuais) e os dois `Watchpoint` internos
//! (`StructureWatchpoint`, `PropertyWatchpoint`) recuperam o pai por aritmética de ponteiro
//! (`OBJECT_OFFSETOF`). Aqui o estado vive num `Rc<Inner>` e cada `Watchpoint` guarda um `Weak` dele; o
//! `handleFire` da subclasse é o closure que `new` recebe. `isValid()` é `true` na base e, na única
//! subclasse portada (`ObjectPropertyChangeAdaptiveWatchpoint`), `!m_owner->isPendingDestruction()`, que
//! sem GC também é sempre `true`.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use crate::bytecode::property_condition::{ObjectPropertyCondition, PropertyConditionKind, WatchabilityEffort};
use crate::bytecode::watchpoint::{FireDetail, Watchpoint, WatchpointBody, WatchpointRef, WatchpointSetRef, WatchpointType};
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;

/// `handleFire(VM&, const FireDetail&)`.
type HandleFire = Box<dyn Fn(&VM, &dyn FireDetail)>;

struct Inner {
    /// `m_key`.
    key: ObjectPropertyCondition,
    /// `m_structureWatchpoint`.
    structure_watchpoint: WatchpointRef,
    /// `m_propertyWatchpoint`.
    property_watchpoint: WatchpointRef,
    /// A `Structure` cujo `transitionWatchpointSet` tem o `m_structureWatchpoint` (o `remove()` intrusivo
    /// do C++ não precisa saber o set; aqui precisa).
    installed_structure: RefCell<Option<StructureRef>>,
    /// O set de substituição que tem o `m_propertyWatchpoint`.
    installed_property_set: RefCell<Option<WatchpointSetRef>>,
    handle_fire: HandleFire,
}

/// `StructureWatchpoint::fireInternal` e `PropertyWatchpoint::fireInternal`: ambos chamam `parent->fire`.
struct Part {
    parent: Weak<Inner>,
}

impl WatchpointBody for Part {
    fn fire_internal(&mut self, vm: &VM, detail: &dyn FireDetail) {
        if let Some(parent) = self.parent.upgrade() {
            parent.fire(vm, detail);
        }
    }
}

/// `class AdaptiveInferredPropertyValueWatchpointBase`.
pub struct AdaptiveInferredPropertyValueWatchpointBase {
    inner: Rc<Inner>,
}

impl AdaptiveInferredPropertyValueWatchpointBase {
    /// O construtor: `RELEASE_ASSERT(key.kind() == PropertyCondition::Equivalence)`.
    pub fn new(key: ObjectPropertyCondition, handle_fire: HandleFire) -> AdaptiveInferredPropertyValueWatchpointBase {
        assert!(key.kind() == PropertyConditionKind::Equivalence);
        let inner = Rc::new_cyclic(|weak: &Weak<Inner>| Inner {
            key,
            structure_watchpoint: Watchpoint::new(
                WatchpointType::AdaptiveInferredPropertyValueStructure,
                Box::new(Part { parent: weak.clone() }),
            ),
            property_watchpoint: Watchpoint::new(
                WatchpointType::AdaptiveInferredPropertyValueProperty,
                Box::new(Part { parent: weak.clone() }),
            ),
            installed_structure: RefCell::new(None),
            installed_property_set: RefCell::new(None),
            handle_fire,
        });
        AdaptiveInferredPropertyValueWatchpointBase { inner }
    }

    pub fn key(&self) -> &ObjectPropertyCondition {
        &self.inner.key
    }

    /// `install(VM&)`.
    pub fn install(&self, vm: &VM) {
        self.inner.install(vm);
    }
}

impl Inner {
    /// `AdaptiveInferredPropertyValueWatchpointBase::install`.
    fn install(&self, vm: &VM) {
        debug_assert!(self.key.is_watchable(vm, WatchabilityEffort::MakeNoChanges)); // Isto é muito caro.

        let structure = self.key.object().structure();
        structure.add_transition_watchpoint(Some(self.structure_watchpoint.clone()));
        *self.installed_structure.borrow_mut() = Some(structure.clone());

        let offset = structure.get(vm, &PropertyName::from_uid(Some(self.key.uid().clone()), false));
        let set = structure
            .property_replacement_watchpoint_set(offset)
            .expect("install exige o set de substituição que isWatchable já garantiu");
        set.borrow_mut().add(Some(self.property_watchpoint.clone()));
        *self.installed_property_set.borrow_mut() = Some(set);
    }

    /// `AdaptiveInferredPropertyValueWatchpointBase::fire`.
    fn fire(&self, vm: &VM, detail: &dyn FireDetail) {
        // Um dos watchpoints disparou, mas o outro não. Garante que nenhum esteja em set algum, o que
        // simplifica reinstalar tudo do zero.
        if self.structure_watchpoint.is_on_list() {
            if let Some(structure) = self.installed_structure.borrow().as_ref() {
                structure.transition_watchpoint_set().borrow_mut().remove(&self.structure_watchpoint);
            }
        }
        if self.property_watchpoint.is_on_list() {
            if let Some(set) = self.installed_property_set.borrow().as_ref() {
                set.borrow_mut().remove(&self.property_watchpoint);
            }
        }

        // `isValid()` é sempre verdadeiro (ver o cabeçalho do módulo).
        if self.key.is_watchable(vm, WatchabilityEffort::EnsureWatchability) {
            self.install(vm);
            return;
        }

        (self.handle_fire)(vm, detail);
    }
}
