//! Tradução de `runtime/EvalExecutable.h`, `EvalExecutable.cpp` e `EvalExecutableInlines.h`.
//!
//! DIVERGÊNCIAS (ver `executable.rs`, `script_executable.rs` e `global_executable.rs`):
//! `EvalCodeBlock*`/`UnlinkedEvalCodeBlock*` do `bit_cast` são o `CodeBlockRef` comum e o subtipo
//! genérico de `GlobalExecutable<U>`. Os acessores que no C++ devolvem `std::span` sobre o
//! `UnlinkedEvalCodeBlock` devolvem cópia (o bloco vive atrás de `RefCell`). `createStructure`,
//! `subspaceFor`, `visitChildren`, `destroy` e `DECLARE_INFO` são maquinaria de heap e não existem.
//! O construtor é `pub(crate)` (protegido no C++): quem o chama são `DirectEvalExecutable` e
//! `IndirectEvalExecutable`, que embrulham esta struct.

use std::cell::RefCell;
use std::rc::Rc;

use crate::bytecode::executable_info::{DerivedContextType, EvalContextType, NeedsClassFieldInitializer};
use crate::bytecode::unlinked_code_block::UnlinkedEvalCodeBlock;
use crate::bytecode::unlinked_function_executable::UnlinkedFunctionExecutableRef;
use crate::parser::parser_modes::{LexicallyScopedFeatures, PrivateBrandRequirement};
use crate::parser::source_code::SourceCode;
use crate::parser::source_provider::SourceProviderSourceType;
use crate::runtime::global_executable::GlobalExecutable;
use crate::runtime::identifier::Identifier;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_type::JSType;
use crate::runtime::script_executable::{ScriptExecutable, TemplateObjectMap};
use crate::runtime::vm::VM;
use std::ops::{Deref, DerefMut};

/// `class EvalExecutable`.
pub struct EvalExecutable {
    base: GlobalExecutable<UnlinkedEvalCodeBlock>,
    needs_class_field_initializer: NeedsClassFieldInitializer,
    private_brand_requirement: PrivateBrandRequirement,
    template_object_map: Option<Box<TemplateObjectMap>>,
}

crate::parser::nodes::inherit!(EvalExecutable => GlobalExecutable<UnlinkedEvalCodeBlock>);

impl EvalExecutable {
    /// `EvalExecutable(JSGlobalObject*, const SourceCode&, LexicallyScopedFeatures, DerivedContextType,
    /// bool isArrowFunctionContext, bool isInsideOrdinaryFunction, EvalContextType, NeedsClassFieldInitializer,
    /// PrivateBrandRequirement)`. O `vm.evalExecutableStructure` é o `JSType::EvalExecutableType`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        source: &SourceCode,
        lexically_scoped_features: LexicallyScopedFeatures,
        derived_context_type: DerivedContextType,
        is_arrow_function_context: bool,
        is_inside_ordinary_function: bool,
        eval_context_type: EvalContextType,
        needs_class_field_initializer: NeedsClassFieldInitializer,
        private_brand_requirement: PrivateBrandRequirement,
    ) -> EvalExecutable {
        let base = ScriptExecutable::new(
            JSType::EvalExecutableType,
            source,
            lexically_scoped_features,
            derived_context_type,
            is_arrow_function_context,
            is_inside_ordinary_function,
            eval_context_type,
            Intrinsic::NoIntrinsic,
        );
        debug_assert!(
            source.provider().expect("EvalExecutable sem SourceProvider").source_type() == SourceProviderSourceType::Program
        );
        EvalExecutable {
            base: GlobalExecutable::new(base),
            needs_class_field_initializer,
            private_brand_requirement,
            template_object_map: None,
        }
    }

    /// `unlinkedCodeBlock()`, que o `bit_cast` do C++ tipa como `UnlinkedEvalCodeBlock*`: falha se ainda não há.
    fn unlinked(&self) -> Rc<RefCell<UnlinkedEvalCodeBlock>> {
        Rc::clone(self.unlinked_code_block().expect("EvalExecutable sem UnlinkedEvalCodeBlock"))
    }

    /// `numVariables()`.
    pub fn num_variables(&self) -> usize {
        self.unlinked().borrow().num_variables()
    }

    /// `variables()`.
    pub fn variables(&self) -> Vec<Identifier> {
        self.unlinked().borrow().variables().to_vec()
    }

    /// `numFunctionHoistingCandidates()`.
    pub fn num_function_hoisting_candidates(&self) -> usize {
        self.unlinked().borrow().num_function_hoisting_candidates()
    }

    /// `functionHoistingCandidates()`.
    pub fn function_hoisting_candidates(&self) -> Vec<Identifier> {
        self.unlinked().borrow().function_hoisting_candidates().to_vec()
    }

    /// `numTopLevelFunctionDecls()`.
    pub fn num_top_level_function_decls(&self) -> usize {
        self.unlinked().borrow().base_ref().borrow().number_of_function_decls()
    }

    /// `topLevelFunctionDecls()`.
    pub fn top_level_function_decls(&self) -> Vec<UnlinkedFunctionExecutableRef> {
        self.unlinked().borrow().base_ref().borrow().function_decls().to_vec()
    }

    /// `allowDirectEvalCache()`.
    pub fn allow_direct_eval_cache(&self) -> bool {
        self.unlinked().borrow().allow_direct_eval_cache()
    }

    /// `needsClassFieldInitializer()`.
    pub fn needs_class_field_initializer(&self) -> NeedsClassFieldInitializer {
        self.needs_class_field_initializer
    }

    /// `privateBrandRequirement()`.
    pub fn private_brand_requirement(&self) -> PrivateBrandRequirement {
        self.private_brand_requirement
    }

    /// `ensureTemplateObjectMap(VM&)`.
    pub fn ensure_template_object_map(&mut self, _vm: &VM) -> &mut TemplateObjectMap {
        ScriptExecutable::ensure_template_object_map_impl(&mut self.template_object_map)
    }
}
