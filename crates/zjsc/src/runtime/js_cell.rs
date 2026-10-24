//! Tradução de `runtime/JSCell.h`, `JSCellInlines.h` e `JSCell.cpp`: o cabeçalho de toda célula
//! (`m_structureID`, `m_indexingTypeAndMisc`, `m_type`, `m_flags`, `m_cellState`) e as consultas de tipo.
//!
//! DIVERGÊNCIA (heap ausente, camada 3): no C++ toda célula herda de `JSCell` e mora no heap do GC; o
//! `JSValue` guarda o ponteiro. Aqui `JSCell` é o cabeçalho que cada tipo de célula tem como campo
//! (composição: `JSObject.cell`), e o `JSValue::Cell(usize)` guarda o `cell_id` que o registro central
//! (`cell_registry`) atribui; `cell_registry::cell_type(id)` é o `JSCell::type()` de um id solto.
//!
//! O `m_structureID` guarda o `StructureRef` (a `Structure` por `Rc`) em vez do ID de 32 bits. Sem
//! GC, `m_cellState` é só o valor inicial `DefinitelyWhite` (nada o consulta além do `setCellState`).
//!
//! Fora desta fatia, e por quê: o `MethodTable` e o despacho virtual (`put`, `getOwnPropertySlot`...
//! são métodos de cada tipo em `js_object` por enquanto), `cellLock`/`JSCellLock` (uma thread só),
//! `getString`/`toPrimitive`/`toNumber`/`toObject`/`toStringSlowCase` (dependem de `JSGlobalObject` e
//! das conversões), `isCallable`/`isConstructor`/`getCallData` (`JSFunction`), `isValidCallee`,
//! `dump`/`estimatedSize`/`analyzeHeap`/`visitChildren` (GC), `reportZappedCellAndCrash`,
//! `CreatingEarlyCellTag`/`CreatingWellDefinedBuiltinCellTag` (a `Structure` não é célula) e `setStructureIDDirectly`/
//! `clearStructure` (nuking do GC concorrente).

use std::cell::{Cell, RefCell};

use crate::runtime::class_info::ClassInfo;
use crate::runtime::indexing_type::{IndexingType, ALL_ARRAY_TYPES, ALL_ARRAY_TYPES_AND_HISTORY, ALL_WRITABLE_ARRAY_TYPES};
use crate::runtime::js_type::{is_object_type, JSType};
use crate::runtime::js_type_info::{InlineTypeFlags, TypeInfo, TYPE_INFO_PER_CELL_BIT};
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;

/// `enum class CellState : uint8_t` (heap/CellState.h).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellState {
    /// The object is either currently being scanned, or it has finished being scanned, or this is a
    /// full collection and it's actually a white object.
    PossiblyBlack = 0,
    /// The object is in eden.
    DefinitelyWhite = 1,
    /// This sorta means that the object is grey.
    PossiblyGrey = 2,
}

/// O cabeçalho de `class JSCell`.
#[derive(Debug)]
pub struct JSCell {
    /// `m_structureID`.
    structure: RefCell<StructureRef>,
    /// `m_indexingTypeAndMisc`.
    indexing_type_and_misc: Cell<IndexingType>,
    /// `m_type`.
    type_: Cell<JSType>,
    /// `m_flags`.
    flags: Cell<InlineTypeFlags>,
    /// `m_cellState`.
    cell_state: Cell<CellState>,
}

impl JSCell {
    /// `JSCell(VM&, Structure*)`: copia do `TypeInfoBlob` da estrutura o indexing mode, o tipo e as
    /// flags inline, com o estado `DefinitelyWhite`.
    pub fn new(_vm: &VM, structure: &StructureRef) -> JSCell {
        JSCell {
            structure: RefCell::new(StructureRef::clone(structure)),
            indexing_type_and_misc: Cell::new(structure.indexing_mode_including_history()),
            type_: Cell::new(structure.type_info().type_()),
            flags: Cell::new(structure.type_info().inline_type_flags()),
            cell_state: Cell::new(CellState::DefinitelyWhite),
        }
    }

    // Querying the type.

    /// `isString()`.
    pub fn is_string(&self) -> bool {
        self.type_.get() == JSType::StringType
    }

    /// `isHeapBigInt()`.
    pub fn is_heap_big_int(&self) -> bool {
        self.type_.get() == JSType::HeapBigIntType
    }

    /// `isSymbol()`.
    pub fn is_symbol(&self) -> bool {
        self.type_.get() == JSType::SymbolType
    }

    /// `isObject()`.
    pub fn is_object(&self) -> bool {
        is_object_type(self.type_.get())
    }

    /// `isGetterSetter()`.
    pub fn is_getter_setter(&self) -> bool {
        self.type_.get() == JSType::GetterSetterType
    }

    /// `isCustomGetterSetter()`.
    pub fn is_custom_getter_setter(&self) -> bool {
        self.type_.get() == JSType::CustomGetterSetterType
    }

    /// `isProxy()`.
    pub fn is_proxy(&self) -> bool {
        self.type_.get() == JSType::GlobalProxyType || self.type_.get() == JSType::ProxyObjectType
    }

    /// `isAPIValueWrapper()`.
    pub fn is_api_value_wrapper(&self) -> bool {
        self.type_.get() == JSType::APIValueWrapperType
    }

    /// `type()`.
    pub fn type_(&self) -> JSType {
        self.type_.get()
    }

    /// `indexingTypeAndMisc()`.
    pub fn indexing_type_and_misc(&self) -> IndexingType {
        self.indexing_type_and_misc.get()
    }

    /// `indexingMode()`.
    pub fn indexing_mode(&self) -> IndexingType {
        self.indexing_type_and_misc() & ALL_ARRAY_TYPES
    }

    /// `indexingType()`.
    pub fn indexing_type(&self) -> IndexingType {
        self.indexing_type_and_misc() & ALL_WRITABLE_ARRAY_TYPES
    }

    /// `structureID()`.
    pub fn structure_id(&self) -> u32 {
        self.structure.borrow().id()
    }

    /// `structure()`.
    pub fn structure(&self) -> StructureRef {
        StructureRef::clone(&self.structure.borrow())
    }

    /// `setStructure(VM&, Structure*)`.
    pub fn set_structure(&self, _vm: &VM, structure: &StructureRef) {
        debug_assert!(std::ptr::eq(structure.class_info(), self.structure.borrow().class_info()));
        *self.structure.borrow_mut() = StructureRef::clone(structure);
        self.flags.set(TypeInfo::merge_inline_type_flags(structure.type_info().inline_type_flags(), self.flags.get()));
        self.type_.set(structure.type_info().type_());
        let new_indexing_type = structure.indexing_mode_including_history();
        if self.indexing_type_and_misc.get() != new_indexing_type {
            debug_assert!(new_indexing_type & !ALL_ARRAY_TYPES_AND_HISTORY == 0);
            let old_value = self.indexing_type_and_misc.get();
            self.indexing_type_and_misc.set((old_value & !ALL_ARRAY_TYPES_AND_HISTORY) | new_indexing_type);
        }
    }

    /// `inlineTypeFlags()`.
    pub fn inline_type_flags(&self) -> InlineTypeFlags {
        self.flags.get()
    }

    /// `classInfo()` (Structure.h): `structure()->classInfoForCells()`.
    pub fn class_info(&self) -> &'static ClassInfo {
        self.structure.borrow().class_info()
    }

    /// `className()`.
    pub fn class_name(&self) -> &'static str {
        self.class_info().class_name
    }

    /// `inherits(const ClassInfo*)` (Structure.h) e `inheritsSlow`.
    pub fn inherits(&self, info: &ClassInfo) -> bool {
        self.class_info().is_sub_class_of(info)
    }

    /// `cellState()`.
    pub fn cell_state(&self) -> CellState {
        self.cell_state.get()
    }

    /// `setCellState(CellState)`.
    pub fn set_cell_state(&self, state: CellState) {
        self.cell_state.set(state);
    }

    /// `perCellBit()`.
    pub fn per_cell_bit(&self) -> bool {
        TypeInfo::per_cell_bit(self.inline_type_flags())
    }

    /// `setPerCellBit(bool)`.
    pub fn set_per_cell_bit(&self, value: bool) {
        if value == self.per_cell_bit() {
            return;
        }

        if value {
            self.flags.set(self.flags.get() | TYPE_INFO_PER_CELL_BIT as InlineTypeFlags);
        } else {
            self.flags.set(self.flags.get() & !(TYPE_INFO_PER_CELL_BIT as InlineTypeFlags));
        }
    }
}
