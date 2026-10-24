//! Porte de `bytecode/ObjectAllocationProfile.h` e `ObjectAllocationProfileInlines.h`:
//! `ObjectAllocationProfileBase`, `ObjectAllocationProfile` e `ObjectAllocationProfileWithPrototype`.
//! O struct `ObjectAllocationProfile` (`m_allocator`, `m_structure`) vive em `bytecode/op_metadata.rs`,
//! que o metadata de `op_new_object` precisa; aqui entram o comportamento dele e o do
//! `ObjectAllocationProfileWithPrototype`.
//!
//! O template `ObjectAllocationProfileBase<Derived>` vira o trait `ObjectAllocationProfileBase`: o
//! `static_cast<Derived*>(this)->setPrototype(...)` é o `set_prototype` do trait.
//!
//! O `JSGlobalObject*` é `Rc<JSGlobalObject>`; `structureCache()` é `JSGlobalObject::structure_cache`
//! (`runtime/structure_cache.rs`) e o `Heap` é o reduzido de `runtime/heap.rs`.
//!
//! Divergências (células são `HeapRef`):
//!
//! - `possibleDefaultPropertyCount` é método do template base no C++ e aqui é a função livre
//!   `possible_default_property_count`. O `realmMayBeNull()` do protótipo não existe (a `Structure`
//!   ainda não guarda o `m_realm`): o realm do protótipo é o `global_object` recebido, o que vale
//!   com um realm só, e o caso de realm nulo some.
//! - `final_object_allocator_for` é a consulta ao `Heap` reduzido (sem coleta nem blocos): o
//!   `Allocator` é identificado pelo `cellSize` do size class.
//! - O `StructureID` do `Structure*` guardado em `m_structure` é o `id()` da estrutura; a
//!   `StructureCache` mantém a estrutura viva (mapa forte).

//! - `WriteBarrier<Structure>`/`WriteBarrier<JSObject>` são `HeapRef` (0 é o `nullptr`); o `owner`
//!   do `WriteBarrier::set` é o `HeapRef` do dono e a barreira é do `Heap` do porte, então
//!   `set(vm, owner, x)` vira atribuição. `visitAggregate`, `dependentLoadLoadFence` e
//!   `storeStoreFence` somem (a marcação do `Heap` lê os campos; um fio só).
//! - `Allocator` é o `u64` do `m_allocator` (0 é o `Allocator()` vazio); `allocator_for` devolve o
//!   identificador e o `cellSize()` do size class (`subspaceFor<JSFinalObject>(vm)->allocatorFor(
//!   allocationSize, EnsureAllocator)`).
//! - O ramo `constructor != nullptr` (poly proto, `FunctionExecutable::cachedPolyProtoStructure`,
//!   `ensurePolyProtoWatchpoint`, `FunctionRareData::createAllocationProfileClearingWatchpoint`)
//!   entra com `FunctionRareData`/`FunctionExecutable`, que não existem ainda; o chamador do
//!   `op_new_object` (`CodeBlock::finishCreation`, `link_objectAllocationProfile`) passa
//!   `constructor = nullptr` e `functionRareData = nullptr`, que é o que este porte cobre, com
//!   `isPolyProto = false` e `executable = nullptr`.
//! - `offsetOfAllocator`, `offsetOfStructure`, `offsetOfPrototype` são offsets de JIT/LLInt: somem.

use std::rc::Rc;

use crate::bytecode::op_metadata::{HeapRef, ObjectAllocationProfile};
use crate::runtime::heap::AllocatorInfo;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSFinalObject, JSObject, JSObjectRef};
use crate::runtime::js_type::JSType;
use crate::runtime::vm::VM;

/// `JSFinalObject::defaultInlineCapacity` e `maxInlineCapacity` (`JSObject.h`).
pub const DEFAULT_INLINE_CAPACITY: u32 = JSFinalObject::DEFAULT_INLINE_CAPACITY;
pub const MAX_INLINE_CAPACITY: u32 = JSFinalObject::MAX_INLINE_CAPACITY;
/// `sizeof(JSFinalObject)` sem a butterfly: cabeçalho da célula (8) e ponteiro da butterfly (8).
const FINAL_OBJECT_HEADER_SIZE: usize = 16;

/// `JSFinalObject::allocationSize(inlineCapacity)`: o cabeçalho mais `inlineCapacity` slots de 8 bytes.
pub fn final_object_allocation_size(inline_capacity: u32) -> usize {
    FINAL_OBJECT_HEADER_SIZE + inline_capacity as usize * 8
}

/// `ObjectAllocationProfileBase<Derived>`.
pub trait ObjectAllocationProfileBase {
    /// O estado comum (`m_allocator` e `m_structure`).
    fn base(&self) -> &ObjectAllocationProfile;
    fn base_mut(&mut self) -> &mut ObjectAllocationProfile;

    /// `Derived::setPrototype(VM&, JSCell* owner, JSObject*)`.
    fn set_prototype(&mut self, vm: &VM, owner: HeapRef, prototype: HeapRef);

    /// `isNull`.
    fn is_null(&self) -> bool {
        self.base().structure == 0
    }

    /// `structure()`: 0 é o `nullptr`.
    fn structure(&self) -> HeapRef {
        self.base().structure
    }

    /// `ObjectAllocationProfileBase::clear` (protegido).
    fn clear_base(&mut self) {
        let base = self.base_mut();
        base.allocator = 0;
        base.structure = 0;
        debug_assert!(self.is_null());
    }
}

impl ObjectAllocationProfileBase for ObjectAllocationProfile {
    fn base(&self) -> &ObjectAllocationProfile {
        self
    }

    fn base_mut(&mut self) -> &mut ObjectAllocationProfile {
        self
    }

    /// `ObjectAllocationProfile::setPrototype`: não guarda nada.
    fn set_prototype(&mut self, _vm: &VM, _owner: HeapRef, _prototype: HeapRef) {}
}

/// `class ObjectAllocationProfileWithPrototype`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ObjectAllocationProfileWithPrototype {
    base: ObjectAllocationProfile,
    /// `m_prototype`.
    prototype: HeapRef,
}

impl ObjectAllocationProfileWithPrototype {
    /// `prototype()`.
    pub fn prototype(&self) -> HeapRef {
        self.prototype
    }

    /// `clear()`.
    pub fn clear(&mut self) {
        self.clear_base();
        self.prototype = 0;
        debug_assert!(self.is_null());
    }
}

impl ObjectAllocationProfileBase for ObjectAllocationProfileWithPrototype {
    fn base(&self) -> &ObjectAllocationProfile {
        &self.base
    }

    fn base_mut(&mut self) -> &mut ObjectAllocationProfile {
        &mut self.base
    }

    fn set_prototype(&mut self, _vm: &VM, _owner: HeapRef, prototype: HeapRef) {
        self.prototype = prototype;
    }
}

/// `ObjectAllocationProfileBase::possibleDefaultPropertyCount` (`ObjectAllocationProfileInlines.h`).
pub fn possible_default_property_count(global_object: &JSGlobalObject, prototype: &JSObjectRef) -> usize {
    if Rc::ptr_eq(prototype, &global_object.object_prototype()) {
        return 0;
    }

    let mut count = 0;
    // `getPropertyNamesFromStructure` com `StringsAndSymbols`, `PrivateSymbolMode::Include` e
    // `DontEnumPropertiesMode::Include`: todas as chaves da estrutura, na ordem de inserção.
    for (_, offset, _) in prototype.structure().properties() {
        let value = prototype.get_direct(offset);

        // Funções são comuns e costumam ser da classe, não sobrescritas.
        if JSObject::from_value(&value).is_some_and(|object| object.type_() == JSType::JSFunctionType) {
            continue;
        }

        count += 1;
    }
    count
}

/// O trecho de `ObjectAllocationProfileBase::initializeProfile` que escolhe a capacidade inline e o
/// allocator do size class (`ObjectAllocationProfileInlines.h`), em função própria para quem só
/// precisa da `Structure` resultante (`slow_path_new_object`, que não tem o índice de metadata do
/// perfil) sem repetir a conta.
pub fn inline_capacity_for(
    vm: &VM,
    global_object: &JSGlobalObject,
    prototype: &JSObjectRef,
    mut inferred_inline_capacity: u32,
    is_poly_proto: bool,
) -> (u32, Option<AllocatorInfo>) {
    let default_inline_capacity = DEFAULT_INLINE_CAPACITY;
    let max_inline_capacity = MAX_INLINE_CAPACITY;
    let mut inline_capacity;
    if inferred_inline_capacity < default_inline_capacity {
        // Tenta encolher o objeto pela análise estática.
        inferred_inline_capacity += possible_default_property_count(global_object, prototype) as u32;

        if inferred_inline_capacity == 0 {
            // Objeto vazio é raro: o analisador provavelmente não viu o inicializador real
            // (acontece com funções auxiliares).
            inferred_inline_capacity = default_inline_capacity;
        } else if inferred_inline_capacity > default_inline_capacity {
            // As propriedades padrão são palpites fracos: não deixam um objeto pequeno virar grande.
            inferred_inline_capacity = default_inline_capacity;
        }

        inline_capacity = inferred_inline_capacity;
        debug_assert!(inline_capacity < max_inline_capacity);
    } else {
        // Objeto normal ou grande.
        inline_capacity = inferred_inline_capacity;
        if inline_capacity > max_inline_capacity {
            inline_capacity = max_inline_capacity;
        }
    }

    if is_poly_proto {
        inline_capacity += 1;
        inline_capacity = inline_capacity.min(max_inline_capacity);
    }

    debug_assert!(inline_capacity > 0);
    debug_assert!(inline_capacity <= max_inline_capacity);

    let allocation_size = final_object_allocation_size(inline_capacity);
    let allocator = vm.heap().final_object_allocator_for(allocation_size);

    // Aproveita a capacidade extra do size class.
    if let Some(allocator) = allocator {
        let slop = (allocator.cell_size - allocation_size) / 8;
        inline_capacity += slop as u32;
        if inline_capacity > max_inline_capacity {
            inline_capacity = max_inline_capacity;
        }
    }

    (inline_capacity, allocator)
}

/// `ObjectAllocationProfileBase::initializeProfile` (`ObjectAllocationProfileInlines.h`), sem o
/// `constructor` (ver o topo): `inferred_inline_capacity` é o `inferredInlineCapacity`.
pub fn initialize_profile<P: ObjectAllocationProfileBase>(
    profile: &mut P,
    vm: &VM,
    global_object: &Rc<JSGlobalObject>,
    owner: HeapRef,
    prototype: &JSObjectRef,
    inferred_inline_capacity: u32,
) {
    debug_assert!(profile.base().allocator == 0);
    debug_assert!(profile.base().structure == 0);

    // `isPolyProto` e `executable` só mudam com `constructor`.
    let is_poly_proto = false;

    let (inline_capacity, allocator) =
        inline_capacity_for(vm, global_object, prototype, inferred_inline_capacity, is_poly_proto);

    let structure = global_object
        .structure_cache()
        .empty_object_structure_for_prototype(global_object, prototype, inline_capacity, is_poly_proto)
        .id() as HeapRef;

    // `isPolyProto` é falso: o `m_allocator` guarda o do size class.
    profile.base_mut().allocator = allocator.map_or(0, |allocator| allocator.allocator);

    profile.base_mut().structure = structure;
    profile.set_prototype(vm, owner, prototype.cell_id() as HeapRef);
}
