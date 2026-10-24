//! Tradução de `runtime/FunctionRareData.h` e `FunctionRareData.cpp`.
//!
//! DIVERGÊNCIAS:
//!
//! - `FunctionRareData` é um `JSCell` no C++; aqui é `Rc<FunctionRareData>` (`FunctionRareDataRef`) e
//!   os campos que mudam depois da criação ficam em `RefCell`/`Cell`, porque o `JSFunction` guarda o
//!   mesmo `Rc` e o devolve por `rareData()`. Sem GC: `destroy`, `createStructure`, `subspaceFor`,
//!   `visitChildren`, `offsetOf...`, `DECLARE_INFO` e `needsDestruction` não existem, nem o
//!   `WriteBarrier` (`m_executable`, `m_boundFunctionStructureID` são referências comuns).
//! - O `ObjectAllocationProfileWithPrototype` do porte guarda `HeapRef` (índice de célula, 0 é nulo,
//!   `bytecode/object_allocation_profile.rs`). O dono do `set(vm, owner, ...)` é a própria
//!   `FunctionRareData`, que não é célula do registro: o `owner` é 0, e o `setPrototype` da
//!   `ObjectAllocationProfileWithPrototype` o ignora.
//! - `m_allocationProfileClearingWatchpoint` (`unique_ptr<AllocationProfileClearingWatchpoint>`) é
//!   `Option<WatchpointRef>`: o `Watchpoint` do porte é o `Rc<Watchpoint>` com o corpo derivado
//!   (`WatchpointBody`). O `PackedCellPtr<FunctionRareData>` do corpo é `Weak<FunctionRareData>` para
//!   não fechar ciclo com o campo acima.
//! - `clear(const char*)` usa `vm()` do `JSCell`; aqui o `VM` é argumento.
//! - O `initializeProfile` do `ObjectAllocationProfileBase` ainda não recebe `constructor` nem
//!   `functionRareData` (o ramo poly proto de `ObjectAllocationProfileInlines.h`), então a chamada
//!   abaixo passa o que ele aceita; o ramo entra quando `object_allocation_profile.rs` o ganhar.

use std::cell::{Cell, RefCell, RefMut};
use std::rc::{Rc, Weak};

use crate::bytecode::internal_function_allocation_profile::InternalFunctionAllocationProfile;
use crate::bytecode::object_allocation_profile::{initialize_profile, ObjectAllocationProfileWithPrototype};
use crate::bytecode::watchpoint::{
    FireDetail, InlineWatchpointSet, Watchpoint, WatchpointBody, WatchpointRef, WatchpointState, WatchpointType,
};
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::executable::ExecutableBaseRef;
use crate::runtime::js_function::JSFunctionRef;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObjectRef;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_name::PropertyName;
use crate::runtime::script_executable::ScriptExecutableRef;
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;

/// `FunctionRareData*`.
pub type FunctionRareDataRef = Rc<FunctionRareData>;

/// `class FunctionRareData`.
pub struct FunctionRareData {
    // Ideally, there would only be one allocation profile for subclassing but due to Reflect.construct we
    // have two. There are some pros and cons in comparison to our current system to using the same profile
    // for both JS constructors and subclasses of builtin constructors:
    //
    // 1) + Uses less memory.
    // 2) + Conceptually simplier as there is only one profile.
    // 3) - We would need a check in all JSFunction object creations (both with classes and without) that the
    //      new.target's profiled structure has a JSFinalObject ClassInfo. This is needed, for example, if we have
    //      `Reflect.construct(Array, args, myConstructor)` since myConstructor will be the new.target of Array
    //      the Array constructor will set the allocation profile of myConstructor to hold an Array structure
    //
    // We don't really care about 1) since this memory is rare and small in total. 2) is unfortunate but is
    // probably outweighed by the cost of 3).
    object_allocation_profile: RefCell<ObjectAllocationProfileWithPrototype>,
    allocation_profile_watchpoint_set: RefCell<InlineWatchpointSet>,
    internal_function_allocation_profile: RefCell<InternalFunctionAllocationProfile>,
    bound_function_structure_id: RefCell<Option<StructureRef>>,
    executable: ExecutableBaseRef,
    allocation_profile_clearing_watchpoint: RefCell<Option<WatchpointRef>>,
    has_reified_length: Cell<bool>,
    has_reified_name: Cell<bool>,
    has_modified_length_for_bound_or_non_host_function: Cell<bool>,
    has_modified_name_for_bound_or_non_host_function: Cell<bool>,
}

impl FunctionRareData {
    /// `create(VM&, ExecutableBase*)` mais o construtor privado.
    pub fn create(_vm: &VM, executable: ExecutableBaseRef) -> FunctionRareDataRef {
        Rc::new(FunctionRareData {
            object_allocation_profile: RefCell::new(ObjectAllocationProfileWithPrototype::default()),
            // We initialize blind so that changes to the prototype after function creation but before
            // the first allocation don't disable optimizations. This isn't super important, since the
            // function is unlikely to allocate a rare data until the first allocation anyway.
            allocation_profile_watchpoint_set: RefCell::new(InlineWatchpointSet::new(WatchpointState::ClearWatchpoint)),
            internal_function_allocation_profile: RefCell::new(InternalFunctionAllocationProfile::default()),
            bound_function_structure_id: RefCell::new(None),
            executable,
            allocation_profile_clearing_watchpoint: RefCell::new(None),
            has_reified_length: Cell::new(false),
            has_reified_name: Cell::new(false),
            has_modified_length_for_bound_or_non_host_function: Cell::new(false),
            has_modified_name_for_bound_or_non_host_function: Cell::new(false),
        })
    }

    /// `objectAllocationProfile()`.
    pub fn object_allocation_profile(&self) -> RefMut<'_, ObjectAllocationProfileWithPrototype> {
        self.object_allocation_profile.borrow_mut()
    }

    /// `objectAllocationStructure()`: 0 é o `nullptr`.
    pub fn object_allocation_structure(&self) -> u32 {
        use crate::bytecode::object_allocation_profile::ObjectAllocationProfileBase;
        self.object_allocation_profile.borrow().structure()
    }

    /// `objectAllocationPrototype()`: 0 é o `nullptr`.
    pub fn object_allocation_prototype(&self) -> u32 {
        self.object_allocation_profile.borrow().prototype()
    }

    /// `allocationProfileWatchpointSet()`.
    pub fn allocation_profile_watchpoint_set(&self) -> RefMut<'_, InlineWatchpointSet> {
        self.allocation_profile_watchpoint_set.borrow_mut()
    }

    /// `clear(const char* reason)`.
    pub fn clear(&self, vm: &VM, reason: &str) {
        self.object_allocation_profile.borrow_mut().clear();
        self.internal_function_allocation_profile.borrow_mut().clear();
        self.allocation_profile_watchpoint_set.borrow_mut().fire_all_with_reason(vm, reason);
    }

    /// `initializeObjectAllocationProfile(VM&, JSGlobalObject*, JSObject* prototype, size_t inlineCapacity,
    /// JSFunction* constructor)`.
    pub fn initialize_object_allocation_profile(
        &self,
        vm: &VM,
        global_object: &Rc<JSGlobalObject>,
        prototype: &JSObjectRef,
        mut inline_capacity: usize,
        constructor: Option<&JSFunctionRef>,
    ) {
        self.initialize_allocation_profile_watchpoint_set();
        // For class constructors, we deploy a heuristics which counts private and public fields as a part of inlineCapacity.
        // Right now, static-analyzer in the BytecodeGenerator cannot know these properties because they are separate CodeBlock
        // from the normal constructors. This offers a bit better heuristics than just directly using inlineCapacity
        if let Some(constructor) = constructor {
            let mut field_count: usize = 0;
            let mut current: Option<JSFunctionRef> = Some(Rc::clone(constructor));
            const MAX_SUPER_DEPTH: usize = 32;
            let initializer_name = PropertyName::from_identifier(&vm.property_names.builtin_names().instance_field_initializer_private_name());
            let mut depth = 0;
            while let Some(current_function) = current.take() {
                if depth >= MAX_SUPER_DEPTH {
                    break;
                }

                let ExecutableBaseRef::Script(ScriptExecutableRef::Function(executable)) = current_function.executable()
                else {
                    break;
                };
                if !executable.borrow().is_class_constructor_function() {
                    break;
                }

                let initializer_value = current_function.get_direct_by_name(vm, &initializer_name);
                if let Some(initializer_function) = Self::as_function(&initializer_value) {
                    if !initializer_function.is_host_function() {
                        let initializer_executable = initializer_function.js_executable();
                        let unlinked_executable = Rc::clone(initializer_executable.borrow().unlinked_executable());
                        if let Some(definitions) = unlinked_executable.borrow().class_element_definitions() {
                            field_count += definitions.len();
                        }
                    }
                }

                let prototype = current_function.get_prototype_direct();
                if prototype == JSValue::Empty {
                    break;
                }
                current = Self::as_function(&prototype);
                depth += 1;
            }
            inline_capacity = field_count.max(inline_capacity);
        }
        initialize_profile(
            &mut *self.object_allocation_profile.borrow_mut(),
            vm,
            global_object,
            0,
            prototype,
            inline_capacity as u32,
        );
    }

    /// `dynamicDowncast<JSFunction>(JSValue)`.
    fn as_function(value: &JSValue) -> Option<JSFunctionRef> {
        let JSValue::Cell(cell_id) = value else { return None };
        match cell_registry::get(*cell_id) {
            Some(CellEntry::Function(function)) => Some(function),
            _ => None,
        }
    }

    /// `isObjectAllocationProfileInitialized()`.
    pub fn is_object_allocation_profile_initialized(&self) -> bool {
        use crate::bytecode::object_allocation_profile::ObjectAllocationProfileBase;
        !self.object_allocation_profile.borrow().is_null()
    }

    /// `internalFunctionAllocationStructure()`.
    pub fn internal_function_allocation_structure(&self) -> Option<StructureRef> {
        self.internal_function_allocation_profile.borrow().structure()
    }

    /// `createInternalFunctionAllocationStructureFromBase(VM&, JSGlobalObject* baseGlobalObject,
    /// JSObject* prototype, Structure* baseStructure)`.
    pub fn create_internal_function_allocation_structure_from_base(
        &self,
        vm: &VM,
        base_global_object: &JSGlobalObject,
        prototype: &JSObjectRef,
        base_structure: &StructureRef,
    ) -> StructureRef {
        self.initialize_allocation_profile_watchpoint_set();
        self.internal_function_allocation_profile.borrow_mut().create_allocation_structure_from_base(
            vm,
            base_global_object,
            prototype,
            base_structure,
            &mut self.allocation_profile_watchpoint_set.borrow_mut(),
        )
    }

    /// `clearInternalFunctionAllocationProfile(VM&, const char* reason)`.
    pub fn clear_internal_function_allocation_profile(&self, vm: &VM, reason: &str) {
        self.internal_function_allocation_profile.borrow_mut().clear();
        self.allocation_profile_watchpoint_set.borrow_mut().fire_all_with_reason(vm, reason);
    }

    /// `initializeAllocationProfileWatchpointSet()`.
    pub fn initialize_allocation_profile_watchpoint_set(&self) {
        let mut set = self.allocation_profile_watchpoint_set.borrow_mut();
        if set.is_still_valid() {
            set.start_watching();
        }
    }

    /// `getBoundFunctionStructure()`.
    pub fn get_bound_function_structure(&self) -> Option<StructureRef> {
        self.bound_function_structure_id.borrow().clone()
    }

    /// `setBoundFunctionStructure(VM&, Structure*)`.
    pub fn set_bound_function_structure(&self, _vm: &VM, structure: StructureRef) {
        *self.bound_function_structure_id.borrow_mut() = Some(structure);
    }

    /// `executable()`.
    pub fn executable(&self) -> ExecutableBaseRef {
        self.executable.clone()
    }

    /// `hasReifiedLength()`.
    pub fn has_reified_length(&self) -> bool {
        self.has_reified_length.get()
    }

    /// `setHasReifiedLength()`.
    pub fn set_has_reified_length(&self) {
        self.has_reified_length.set(true);
    }

    /// `hasReifiedName()`.
    pub fn has_reified_name(&self) -> bool {
        self.has_reified_name.get()
    }

    /// `setHasReifiedName()`.
    pub fn set_has_reified_name(&self) {
        self.has_reified_name.set(true);
    }

    /// `hasModifiedLengthForBoundOrNonHostFunction()`.
    pub fn has_modified_length_for_bound_or_non_host_function(&self) -> bool {
        self.has_modified_length_for_bound_or_non_host_function.get()
    }

    /// `setHasModifiedLengthForBoundOrNonHostFunction()`.
    pub fn set_has_modified_length_for_bound_or_non_host_function(&self) {
        self.has_modified_length_for_bound_or_non_host_function.set(true);
    }

    /// `hasModifiedNameForBoundOrNonHostFunction()`.
    pub fn has_modified_name_for_bound_or_non_host_function(&self) -> bool {
        self.has_modified_name_for_bound_or_non_host_function.get()
    }

    /// `setHasModifiedNameForBoundOrNonHostFunction()`.
    pub fn set_has_modified_name_for_bound_or_non_host_function(&self) {
        self.has_modified_name_for_bound_or_non_host_function.set(true);
    }

    /// `hasAllocationProfileClearingWatchpoint()`.
    pub fn has_allocation_profile_clearing_watchpoint(&self) -> bool {
        self.allocation_profile_clearing_watchpoint.borrow().is_some()
    }

    /// `createAllocationProfileClearingWatchpoint()`.
    pub fn create_allocation_profile_clearing_watchpoint(self: &Rc<FunctionRareData>) -> WatchpointRef {
        assert!(!self.has_allocation_profile_clearing_watchpoint());
        let watchpoint = Watchpoint::new(
            WatchpointType::FunctionRareDataAllocationProfileClearing,
            Box::new(AllocationProfileClearingWatchpoint { rare_data: Rc::downgrade(self) }),
        );
        *self.allocation_profile_clearing_watchpoint.borrow_mut() = Some(Rc::clone(&watchpoint));
        watchpoint
    }
}

/// `class FunctionRareData::AllocationProfileClearingWatchpoint`.
struct AllocationProfileClearingWatchpoint {
    rare_data: Weak<FunctionRareData>,
}

impl WatchpointBody for AllocationProfileClearingWatchpoint {
    /// `fireInternal(VM&, const FireDetail&)`.
    fn fire_internal(&mut self, vm: &VM, _detail: &dyn FireDetail) {
        if let Some(rare_data) = self.rare_data.upgrade() {
            rare_data.clear(vm, "AllocationProfileClearingWatchpoint fired.");
        }
    }
}
