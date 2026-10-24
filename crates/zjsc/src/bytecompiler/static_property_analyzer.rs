//! Porte de `bytecompiler/StaticPropertyAnalyzer.h`.
//!
//! "Used for flow-insensitive static analysis of the number of properties assigned to an object.
//! We use this analysis with other runtime data to produce an optimization guess. This analysis
//! is understood to be lossy, and it's OK if it turns out to be wrong sometimes."

use std::collections::HashMap;

use crate::bytecode::instruction_stream::JSInstructionStreamMutableRef;
use crate::bytecompiler::register_id::RegisterID;
use crate::bytecompiler::static_property_analysis::{StaticPropertyAnalysis, StaticPropertyAnalysisRef};

#[derive(Default)]
pub struct StaticPropertyAnalyzer {
    analyses: HashMap<i32, StaticPropertyAnalysisRef>,
}

impl StaticPropertyAnalyzer {
    pub fn create_this(&mut self, dst: &RegisterID, instruction_ref: JSInstructionStreamMutableRef) {
        let is_new_entry = !self.analyses.contains_key(&dst.index());
        if is_new_entry {
            self.analyses.insert(dst.index(), StaticPropertyAnalysis::create(instruction_ref));
        }
        // Can't have two 'this' in the same constructor.
        debug_assert!(is_new_entry);
    }

    pub fn new_object(&mut self, dst: &RegisterID, instruction_ref: JSInstructionStreamMutableRef) {
        let analysis = StaticPropertyAnalysis::create(instruction_ref);
        match self.analyses.get_mut(&dst.index()) {
            None => {
                self.analyses.insert(dst.index(), analysis.copy_ref());
            }
            Some(existing) => {
                let old = std::mem::replace(existing, analysis);
                Self::kill_analysis(Some(&old));
            }
        }
    }

    /// `propertyIndex` é um índice num conjunto unificado de strings.
    pub fn put_by_id(&mut self, dst: &RegisterID, property_index: u32) {
        let Some(analysis) = self.analyses.get(&dst.index()) else {
            return;
        };
        analysis.borrow_mut().add_property_index(property_index);
    }

    pub fn mov(&mut self, dst: &RegisterID, src: &RegisterID) {
        let analysis = self.analyses.get(&src.index()).cloned();
        let Some(analysis) = analysis else {
            self.kill_register(dst);
            return;
        };

        match self.analyses.get_mut(&dst.index()) {
            None => {
                self.analyses.insert(dst.index(), analysis);
            }
            Some(existing) => {
                let old = std::mem::replace(existing, analysis);
                Self::kill_analysis(Some(&old));
            }
        }
    }

    fn kill_analysis(analysis: Option<&StaticPropertyAnalysisRef>) {
        let Some(analysis) = analysis else {
            return;
        };
        // Aliases for this object still exist, so it might acquire more properties.
        if !analysis.borrow().has_one_ref() {
            return;
        }
        analysis.borrow_mut().record();
    }

    /// `kill(RegisterID*)`.
    ///
    /// We observe kills in order to avoid piling on properties to an object after its bytecode
    /// register has been recycled. Since this is a simple static analysis, we just try to catch the
    /// simplest cases, so we accept kills to any registers except for registers that have no
    /// inferred properties yet.
    pub fn kill_register(&mut self, dst: &RegisterID) {
        let Some(analysis) = self.analyses.get(&dst.index()) else {
            return;
        };
        if analysis.borrow().property_index_count() == 0 {
            return;
        }

        Self::kill_analysis(Some(analysis));
        self.analyses.remove(&dst.index());
    }

    /// `kill()`.
    pub fn kill(&mut self) {
        while let Some(key) = self.analyses.keys().next().copied() {
            let taken = self.analyses.remove(&key);
            Self::kill_analysis(taken.as_ref());
        }
    }
}
