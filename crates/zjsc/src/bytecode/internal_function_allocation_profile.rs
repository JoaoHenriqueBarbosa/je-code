//! Tradução de `bytecode/InternalFunctionAllocationProfile.h`.
//!
//! DIVERGÊNCIAS:
//!
//! - `WriteBarrierStructureID` é `Option<StructureRef>` (o `StructureID` é `Structure::id()`); o
//!   `owner` do `set(vm, owner, ...)` some junto com a barreira de escrita. `visitAggregate`,
//!   `offsetOfStructureID` e `storeStoreFence` não existem.
//! - `StructureCache::emptyStructureForPrototypeFromBaseStructure` é de `runtime/StructureCache.h`,
//!   ainda não portado (o `object_allocation_profile.rs` já assume o mesmo `structure_cache()` do
//!   `JSGlobalObject`); a chamada abaixo usa o nome snake_case determinístico.

use crate::bytecode::watchpoint::InlineWatchpointSet;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObjectRef;
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::StructureRef;
use crate::runtime::structure_cache::ShouldCacheStructure;
use crate::runtime::vm::VM;

/// `class InternalFunctionAllocationProfile`.
#[derive(Default)]
pub struct InternalFunctionAllocationProfile {
    structure_id: Option<StructureRef>,
}

impl InternalFunctionAllocationProfile {
    /// `structure()`.
    pub fn structure(&self) -> Option<StructureRef> {
        self.structure_id.clone()
    }

    /// `createAllocationStructureFromBase(VM&, JSGlobalObject*, JSCell* owner, JSObject* prototype,
    /// Structure* base, InlineWatchpointSet&)`.
    pub fn create_allocation_structure_from_base(
        &mut self,
        vm: &VM,
        base_global_object: &JSGlobalObject,
        prototype: &JSObjectRef,
        base_structure: &StructureRef,
        watchpoint_set: &mut InlineWatchpointSet,
    ) -> StructureRef {
        debug_assert!(base_structure.has_mono_proto());

        // FIXME do C++ (bug 177318): Implement polymorphic prototypes for subclasses of builtin types.
        let structure = if base_structure.stored_prototype() == JSValue::Cell(prototype.cell_id()) {
            base_structure.clone()
        } else {
            // A structure already here means this profile keeps rotating between bases, so our own
            // memoization is not working for this function and only the global cache bounds the churn.
            let should_cache_structure =
                if self.structure_id.is_some() { ShouldCacheStructure::Yes } else { ShouldCacheStructure::No };
            base_global_object.structure_cache().empty_structure_for_prototype_from_base_structure(
                base_global_object,
                prototype,
                base_structure,
                should_cache_structure,
            )
        };

        // It's possible to get here because some JSFunction got passed to two different InternalFunctions. e.g.
        // function Foo() { }
        // Reflect.construct(Promise, [], Foo);
        // Reflect.construct(Int8Array, [], Foo);
        if self.structure_id.as_ref().is_some_and(|current| current.id() != structure.id()) {
            watchpoint_set.fire_all_with_reason(vm, "InternalFunctionAllocationProfile rotated to a new structure");
        }

        self.structure_id = Some(structure.clone());
        structure
    }

    /// `clear()`.
    pub fn clear(&mut self) {
        self.structure_id = None;
    }
}
