//! Porte de `bytecompiler/StaticPropertyAnalysis.h`.
//!
//! `StaticPropertyAnalysis::record()` está em `BytecodeGenerator.cpp` no C++ e fica aqui junto da
//! classe. Depende de `crate::bytecode::instruction_stream::JSInstructionStream::MutableRef`
//! (ainda não portado) e dos `OpNewObject`/`OpCreateThis` gerados de `Bytecodes.h`.
//! A contagem de referências é a observada por `hasOneRef()` no `StaticPropertyAnalyzer`, então é
//! intrusiva: `StaticPropertyAnalysisRef` incrementa ao clonar e decrementa ao soltar.

use std::cell::{Ref, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use crate::bytecode::bytecode_ops::{OpCreateThis, OpNewObject};
use crate::bytecode::instruction_stream::JSInstructionStreamMutableRef;
use crate::bytecode::opcode::OpcodeID;

/// "Reference count indicates number of live registers that alias this object."
pub struct StaticPropertyAnalysis {
    ref_count: i32,
    instruction_ref: JSInstructionStreamMutableRef,
    property_indexes: HashSet<u32>,
}

pub type StaticPropertyAnalysisNode = Rc<RefCell<StaticPropertyAnalysis>>;

impl StaticPropertyAnalysis {
    /// `create(MutableRef&&)`: a contagem nasce 1 (`adoptRef`), por isso devolve a alça já contada.
    pub fn create(instruction_ref: JSInstructionStreamMutableRef) -> StaticPropertyAnalysisRef {
        let node = Rc::new(RefCell::new(StaticPropertyAnalysis {
            ref_count: 0,
            instruction_ref,
            property_indexes: HashSet::new(),
        }));
        StaticPropertyAnalysisRef::new(&node)
    }

    pub fn add_property_index(&mut self, property_index: u32) {
        self.property_indexes.insert(property_index);
    }

    pub fn record(&mut self) {
        let size = self.property_indexes.len() as u32;
        match self.instruction_ref.opcode_id_enum() {
            OpcodeID::op_new_object => {
                self.instruction_ref.cast_mut::<OpNewObject>().set_inline_capacity(size, &mut || 255);
            }
            OpcodeID::op_create_this => {
                self.instruction_ref.cast_mut::<OpCreateThis>().set_inline_capacity(size, &mut || 255);
            }
            // `ASSERT_NOT_REACHED()`: só aborta em build de depuração; em release o C++ não faz nada.
            _ => debug_assert!(false, "StaticPropertyAnalysis::record: opcode inesperado"),
        }
    }

    pub fn property_index_count(&self) -> i32 {
        self.property_indexes.len() as i32
    }

    pub fn has_one_ref(&self) -> bool {
        self.ref_count == 1
    }

    pub fn ref_count(&self) -> i32 {
        self.ref_count
    }
}

/// `Ref<StaticPropertyAnalysis>`/`RefPtr<StaticPropertyAnalysis>`.
pub struct StaticPropertyAnalysisRef {
    analysis: StaticPropertyAnalysisNode,
}

impl StaticPropertyAnalysisRef {
    pub fn new(analysis: &StaticPropertyAnalysisNode) -> Self {
        analysis.borrow_mut().ref_count += 1;
        StaticPropertyAnalysisRef { analysis: Rc::clone(analysis) }
    }

    pub fn get(&self) -> &StaticPropertyAnalysisNode {
        &self.analysis
    }

    pub fn borrow(&self) -> Ref<'_, StaticPropertyAnalysis> {
        self.analysis.borrow()
    }

    pub fn borrow_mut(&self) -> std::cell::RefMut<'_, StaticPropertyAnalysis> {
        self.analysis.borrow_mut()
    }

    /// `copyRef()`.
    pub fn copy_ref(&self) -> Self {
        self.clone()
    }
}

impl Clone for StaticPropertyAnalysisRef {
    fn clone(&self) -> Self {
        StaticPropertyAnalysisRef::new(&self.analysis)
    }
}

impl Drop for StaticPropertyAnalysisRef {
    fn drop(&mut self) {
        let mut analysis = self.analysis.borrow_mut();
        analysis.ref_count -= 1;
        debug_assert!(analysis.ref_count >= 0);
    }
}
