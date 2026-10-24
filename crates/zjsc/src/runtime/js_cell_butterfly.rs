//! Tradução de `runtime/JSCellButterfly.{h,cpp}` e `JSCellButterflyInlines.h`, a superfície que o
//! bytecompiler usa: `tryCreate(vm, structure, length)`, `setIndex`, `get`, os comprimentos, a
//! identidade da célula e as duas `Structure` de `VM` que o `ArrayNode::emitBytecode` consulta
//! (`vm.cellButterflyStructure(indexingType)` e `vm.cellButterflyOnlyAtomStringsStructure`).
//!
//! Fora desta fatia, e por quê: `create`/`tryCreate(vm, indexingType, length)`,
//! `createFromClonedArguments`, `createFromDirectArguments`,
//! `createFromScopedArguments`, `createFromSet`, `createFromString`, `tryCreateFromArgList`,
//! `copyToArguments`, `visitChildren` e `toButterfly`/`fromButterfly` (dependem de `JSArray`,
//! `JSGlobalObject`, `ArgList`, do GC e do layout de memória do `Butterfly`).
//!
//! DIVERGÊNCIA (heap ausente, camada 3): no C++ a célula vive no heap do GC, logo depois do
//! `JSCell` header vem o `IndexingHeader` e o butterfly com os `WriteBarrier<Unknown>`. Aqui a célula
//! é um valor compartilhado por `Rc` e o butterfly é um vetor com a mesma semântica:
//!
//! - o `JSValue::Cell(usize)` guarda o `cell_id` atribuído pelo registro central (`cell_registry`),
//!   que mantém a célula viva, sem coleta;
//! - a `Structure` é real (`StructureRef`), criada no `VM::new` por `JSCellButterfly::create_structure`
//!   (`rawImmutableButterflyStructure` por indexing type e `cellButterflyOnlyAtomStringsStructure`);
//! - o armazenamento `Double` guarda `f64` cru (`contiguousDouble()`), os demais guardam `JSValue`;
//! - o `JSCellButterfly(vm, structure, length)` só inicializa os elementos do shape `Contiguous` com
//!   `JSValue()`; nos shapes `Int32` e `Double` o C++ deixa a memória sem inicializar, e a regra do
//!   `setIndex` ("Only call this if you just allocated this butterfly") garante que todo índice é
//!   escrito antes de ler. Aqui esses shapes começam em zero.

use std::cell::RefCell;
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::fallible_alloc::try_filled_vec;
use crate::runtime::indexing_type::{
    array_index_from_indexing_type, has_contiguous, has_double, IndexingType, COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS,
    COPY_ON_WRITE_ARRAY_WITH_DOUBLE, COPY_ON_WRITE_ARRAY_WITH_INT32, NUMBER_OF_INDEXING_SHAPES,
};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, STRUCTURE_IS_IMMORTAL};
use crate::runtime::js_value::{js_null, JSValue};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::options::Options;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `IndexingHeader::maximumLength`.
pub const MAXIMUM_LENGTH: u32 = 0x1000_0000;

/// `const ClassInfo JSCellButterfly::s_info`.
pub static CELL_BUTTERFLY_S_INFO: ClassInfo = ClassInfo { class_name: "Cell Butterfly", parent_class: None, static_prop_hash_table: None, inherits_js_type_range: None };

/// `JSCellButterfly::StructureFlags`: `Base::StructureFlags | StructureIsImmortal`.
const STRUCTURE_FLAGS: u32 = STRUCTURE_IS_IMMORTAL;

/// As `Structure*` que o `VM::new` cria: `rawImmutableButterflyStructure(CopyOnWriteArrayWith{Int32,Double,Contiguous})`
/// e `cellButterflyOnlyAtomStringsStructure` (VM.cpp:387-394).
#[derive(Debug)]
pub struct CellButterflyStructures {
    immutable: [StructureRef; 3],
    only_atom_strings: StructureRef,
}

impl CellButterflyStructures {
    /// VM.cpp:387-394. Sem `allowDoubleShape`, a de `Double` é a mesma de `Contiguous`.
    pub fn create(vm: &VM) -> CellButterflyStructures {
        let int32 = JSCellButterfly::create_structure(vm, None, js_null(), COPY_ON_WRITE_ARRAY_WITH_INT32);
        let contiguous = JSCellButterfly::create_structure(vm, None, js_null(), COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS);
        let double = if Options::with(|options| options.allow_double_shape) {
            JSCellButterfly::create_structure(vm, None, js_null(), COPY_ON_WRITE_ARRAY_WITH_DOUBLE)
        } else {
            Rc::clone(&contiguous)
        };
        // This is only for JSCellButterfly filled with atom strings.
        let only_atom_strings = JSCellButterfly::create_structure(vm, None, js_null(), COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS);
        CellButterflyStructures { immutable: [int32, double, contiguous], only_atom_strings }
    }
}

impl VM {
    /// `VM::cellButterflyStructure(IndexingType)` (`rawImmutableButterflyStructure(indexingType)`).
    pub fn cell_butterfly_structure(&self, indexing_type: IndexingType) -> StructureRef {
        let index = array_index_from_indexing_type(indexing_type) - NUMBER_OF_INDEXING_SHAPES as u32;
        assert!(index < 3, "cellButterflyStructure fora dos modos copy-on-write: {indexing_type:#x}");
        Rc::clone(&self.cell_butterfly_structures().immutable[index as usize])
    }

    /// `VM::cellButterflyOnlyAtomStringsStructure`: só para `JSCellButterfly` cheio de strings atômicas.
    pub fn cell_butterfly_only_atom_strings_structure(&self) -> StructureRef {
        Rc::clone(&self.cell_butterfly_structures().only_atom_strings)
    }
}

/// O butterfly: `contiguous()` (`WriteBarrier<Unknown>`) ou `contiguousDouble()` (`double`).
#[derive(Debug)]
enum Butterfly {
    Values(Vec<JSValue>),
    Doubles(Vec<f64>),
}

/// `class JSCellButterfly`.
#[derive(Debug)]
pub struct JSCellButterfly {
    structure: StructureRef,
    cell_id: usize,
    butterfly: RefCell<Butterfly>,
}

/// Referência compartilhada, o `JSCellButterfly*` do C++.
pub type JSCellButterflyRef = Rc<JSCellButterfly>;

impl JSCellButterfly {
    /// `JSCellButterfly::createStructure(vm, globalObject, prototype, indexingType)`.
    pub fn create_structure(
        vm: &VM,
        global_object: Option<&crate::runtime::js_global_object::JSGlobalObject>,
        prototype: JSValue,
        indexing_type: IndexingType,
    ) -> StructureRef {
        Structure::create_with_indexing_type(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::JSCellButterflyType, STRUCTURE_FLAGS),
            &CELL_BUTTERFLY_S_INFO,
            indexing_type,
            0,
        )
    }

    /// `JSCellButterfly::tryCreate(VM&, Structure*, unsigned length)`: `None` é o `nullptr` de
    /// `length > IndexingHeader::maximumLength` ou da falta de memória.
    pub fn try_create(_vm: &VM, structure: StructureRef, length: u32) -> Option<JSCellButterflyRef> {
        let indexing_type = structure.indexing_mode();
        if length > MAXIMUM_LENGTH {
            return None;
        }

        let butterfly = if has_double(indexing_type) {
            Butterfly::Doubles(try_filled_vec(0.0, length as usize)?)
        } else if has_contiguous(indexing_type) {
            Butterfly::Values(try_filled_vec(JSValue::empty(), length as usize)?)
        } else {
            Butterfly::Values(try_filled_vec(JSValue::Int32(0), length as usize)?)
        };

        let cell_id = cell_registry::reserve();
        let cell = Rc::new(JSCellButterfly { structure, cell_id, butterfly: RefCell::new(butterfly) });
        cell_registry::set(cell_id, CellEntry::CellButterfly(Rc::clone(&cell)));
        Some(cell)
    }

    /// Procura a célula pelo `cell_id` guardado em `JSValue::Cell`; `None` se o id não é de um
    /// `JSCellButterfly`.
    pub fn from_cell_id(cell_id: usize) -> Option<JSCellButterflyRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::CellButterfly(cell)) => Some(cell),
            _ => None,
        }
    }

    /// `JSCellButterfly::visitChildren`: os elementos do vetor (o `spread` os guarda aqui até o `new_array_with_spread`
    /// ou a chamada com `...` os copiar).
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        (0..self.length()).for_each(|index| marker.value(self.get(index)));
    }

    /// Identidade da célula (o valor que `JSValue::from_cell` guarda).
    pub fn cell_id(&self) -> usize {
        self.cell_id
    }

    /// `Structure* JSCell::structure()`.
    pub fn structure(&self) -> &StructureRef {
        &self.structure
    }

    /// `JSCell::indexingType()`.
    pub fn indexing_type(&self) -> IndexingType {
        self.structure.indexing_mode()
    }

    /// `isOnlyAtomStringsStructure(vm, butterfly)`: `structure() == vm.cellButterflyOnlyAtomStringsStructure`.
    pub fn is_only_atom_strings_structure(&self, vm: &VM) -> bool {
        Rc::ptr_eq(&self.structure, &vm.cell_butterfly_only_atom_strings_structure())
    }

    /// `publicLength()`.
    pub fn public_length(&self) -> u32 {
        match &*self.butterfly.borrow() {
            Butterfly::Values(values) => values.len() as u32,
            Butterfly::Doubles(doubles) => doubles.len() as u32,
        }
    }

    /// `vectorLength()`: o construtor iguala os dois comprimentos.
    pub fn vector_length(&self) -> u32 {
        self.public_length()
    }

    /// `length()`.
    pub fn length(&self) -> u32 {
        self.public_length()
    }

    /// `get(unsigned index)`.
    pub fn get(&self, index: u32) -> JSValue {
        match &*self.butterfly.borrow() {
            Butterfly::Values(values) => values[index as usize],
            Butterfly::Doubles(doubles) => {
                let value = doubles[index as usize];
                // Holes are not supported yet.
                debug_assert!(!value.is_nan());
                JSValue::double_number(value)
            }
        }
    }

    /// `setIndex(VM&, unsigned index, JSValue)`. Only call this if you just allocated this butterfly.
    pub fn set_index(&self, _vm: &VM, index: u32, value: JSValue) {
        match &mut *self.butterfly.borrow_mut() {
            Butterfly::Doubles(doubles) => doubles[index as usize] = value.as_number(),
            Butterfly::Values(values) => values[index as usize] = value,
        }
    }

    /// `JSCellButterfly::createFromArray(globalObject, vm, array)`, o `op_spread` rápido. `None` é o
    /// `nullptr` da falta de memória. Sem butterfly compartilhado, o atalho copy-on-write (reusar o
    /// butterfly do array) não existe: sempre copia, e buraco vira `undefined`.
    pub fn create_from_array(
        vm: &VM,
        array: &crate::runtime::js_array::JSArray,
    ) -> Option<JSCellButterflyRef> {
        let length = array.length();
        let result = JSCellButterfly::try_create(vm, vm.cell_butterfly_structure(COPY_ON_WRITE_ARRAY_WITH_CONTIGUOUS), length)?;
        for i in 0..length {
            // Holes are assumed to have read as undefined.
            let value = array.try_get_index_quickly(i);
            result.set_index(vm, i, if value.is_empty() { JSValue::undefined() } else { value });
        }
        Some(result)
    }
}
