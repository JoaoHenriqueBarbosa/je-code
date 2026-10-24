//! Porte de `runtime/ObjectPropertyChangeAdaptiveWatchpoint.h`, instanciado só para
//! `InlineWatchpointSet` (o `WatchpointSet` do template). O `m_owner` serve apenas para o
//! `isPendingDestruction()`, sempre falso sem GC, então não é guardado.

use std::cell::RefCell;
use std::rc::Rc;

use crate::bytecode::adaptive_inferred_property_value_watchpoint_base::AdaptiveInferredPropertyValueWatchpointBase;
use crate::bytecode::property_condition::ObjectPropertyCondition;
use crate::bytecode::watchpoint::{InlineWatchpointSet, StringFireDetail, WatchpointState};
use crate::runtime::vm::VM;

/// `ObjectPropertyChangeAdaptiveWatchpoint<InlineWatchpointSet>`.
pub struct ObjectPropertyChangeAdaptiveWatchpoint {
    base: AdaptiveInferredPropertyValueWatchpointBase,
}

impl std::fmt::Debug for ObjectPropertyChangeAdaptiveWatchpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObjectPropertyChangeAdaptiveWatchpoint").field("key", self.base.key()).finish()
    }
}

impl ObjectPropertyChangeAdaptiveWatchpoint {
    /// O construtor: `RELEASE_ASSERT(watchpointSet.state() == IsWatched)`. O `handleFire` é
    /// `m_watchpointSet.fireAll(vm, StringFireDetail("Object Property is changed."))`.
    pub fn new(condition: ObjectPropertyCondition, watchpoint_set: Rc<RefCell<InlineWatchpointSet>>) -> ObjectPropertyChangeAdaptiveWatchpoint {
        assert!(watchpoint_set.borrow().state() == WatchpointState::IsWatched);
        let base = AdaptiveInferredPropertyValueWatchpointBase::new(
            condition,
            Box::new(move |vm, _detail| {
                InlineWatchpointSet::fire_all_shared(&watchpoint_set, vm, &StringFireDetail::new("Object Property is changed."));
            }),
        );
        ObjectPropertyChangeAdaptiveWatchpoint { base }
    }

    /// `install(vm)`.
    pub fn install(&self, vm: &VM) {
        self.base.install(vm);
    }
}
