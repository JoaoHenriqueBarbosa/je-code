//! Tradução de `runtime/DirectEvalExecutable.h` e `DirectEvalExecutable.cpp`.
//!
//! DIVERGÊNCIAS: as mesmas de `indirect_eval_executable.rs` (o executável é o
//! `Rc<RefCell<EvalExecutable>>`, e o bloco de `evalEnabled` não existe). O construtor do C++ só
//! confere o `ASSERT` de `NeedsClassFieldInitializer`/`PrivateBrandRequirement`, que aqui está em `create`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::bytecode::executable_info::{DerivedContextType, EvalContextType, NeedsClassFieldInitializer};
use crate::bytecode::tdz_environment::TDZEnvironment;
use crate::parser::parser_error::ParserError;
use crate::parser::parser_modes::{JSParserScriptMode, LexicallyScopedFeatures, PrivateBrandRequirement};
use crate::parser::source_code::SourceCode;
use crate::parser::variable_environment::PrivateNameEnvironment;
use crate::runtime::code_cache::generate_unlinked_code_block_for_direct_eval;
use crate::runtime::eval_executable::EvalExecutable;
use crate::runtime::indirect_eval_executable::finish_parsed_eval;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::throw_scope::{throw_exception, ThrowScope};

/// `DirectEvalExecutable::create(...)`: nulo (`None`) com o erro de sintaxe lançado no `VM`.
#[allow(clippy::too_many_arguments)]
pub fn create(
    global_object: &JSGlobalObject,
    source: &SourceCode,
    lexically_scoped_features: LexicallyScopedFeatures,
    derived_context_type: DerivedContextType,
    needs_class_field_initializer: NeedsClassFieldInitializer,
    private_brand_requirement: PrivateBrandRequirement,
    is_arrow_function_context: bool,
    is_inside_ordinary_function: bool,
    eval_context_type: EvalContextType,
    variables_under_tdz: Option<&TDZEnvironment>,
    private_name_environment: Option<&PrivateNameEnvironment>,
) -> Option<Rc<RefCell<EvalExecutable>>> {
    debug_assert!(
        (needs_class_field_initializer == NeedsClassFieldInitializer::No && private_brand_requirement == PrivateBrandRequirement::None)
            || derived_context_type == DerivedContextType::DerivedConstructorContext
    );
    let vm = global_object.vm_rc();
    let executable = Rc::new(RefCell::new(EvalExecutable::new(
        source,
        lexically_scoped_features,
        derived_context_type,
        is_arrow_function_context,
        is_inside_ordinary_function,
        eval_context_type,
        needs_class_field_initializer,
        private_brand_requirement,
    )));

    let mut error = ParserError::new();
    let code_generation_mode = global_object.default_code_generation_mode();
    let executable_source = executable.borrow().source().clone();

    // We don't bother with CodeCache here because direct eval uses a specialized DirectEvalCodeCache.
    let unlinked_eval_code = generate_unlinked_code_block_for_direct_eval(
        &vm,
        &executable,
        &executable_source,
        JSParserScriptMode::Classic,
        code_generation_mode,
        &mut error,
        eval_context_type,
        variables_under_tdz,
        private_name_environment,
    );

    match finish_parsed_eval(global_object, executable, unlinked_eval_code, &error) {
        Ok(executable) => Some(executable),
        Err(error) => {
            // `throwVMError(globalObject, scope, error.toErrorObject(globalObject, executable->source()))`.
            let mut scope = ThrowScope::new(global_object.vm());
            throw_exception(global_object, &mut scope, error);
            None
        }
    }
}
