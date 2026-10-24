//! Tradução de `runtime/IndirectEvalExecutable.h` e `IndirectEvalExecutable.cpp`.
//!
//! DIVERGÊNCIAS:
//!
//! - `IndirectEvalExecutable` só acrescenta um construtor a `EvalExecutable`, então `create` e
//!   `tryCreate` devolvem o `Rc<RefCell<EvalExecutable>>` (o tipo que `ScriptExecutableRef::Eval` guarda).
//! - O `ErrorHandlerFunctor` do `createImpl` vira o `Result`: `create` entrega o objeto de erro como
//!   `resultingError` (o `Err`), e `tryCreate` o lança no `VM`.
//! - O bloco `!globalObject->evalEnabled()` (`reportViolationForUnsafeEval` e `EvalError`) não existe:
//!   `evalEnabled`, `trustedTypesEnforcement` e a `GlobalObjectMethodTable` do embedder não foram
//!   portados, então o `eval` está sempre habilitado.
//! - `subspaceFor`, `finishCreation` e `allocateCell` são maquinaria de heap.

use std::cell::RefCell;
use std::rc::Rc;

use crate::bytecode::executable_info::{DerivedContextType, EvalContextType, NeedsClassFieldInitializer};
use crate::bytecode::unlinked_code_block::UnlinkedEvalCodeBlock;
use crate::parser::parser_error::ParserError;
use crate::parser::parser_modes::{LexicallyScopedFeatures, PrivateBrandRequirement};
use crate::parser::source_code::SourceCode;
use crate::runtime::eval_executable::EvalExecutable;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObjectHandle;
use crate::runtime::throw_scope::{throw_exception, ThrowScope};

/// `constexpr bool insideOrdinaryFunction = false`.
const INSIDE_ORDINARY_FUNCTION: bool = false;

/// `IndirectEvalExecutable::createImpl(...)`: o executável, ou o objeto de erro do `ParserError`.
fn create_impl(
    global_object: &JSGlobalObject,
    source: &SourceCode,
    lexically_scoped_features: LexicallyScopedFeatures,
    derived_context_type: DerivedContextType,
    is_arrow_function_context: bool,
    eval_context_type: EvalContextType,
) -> Result<Rc<RefCell<EvalExecutable>>, JSObjectHandle> {
    let vm = global_object.vm_rc();
    let executable = Rc::new(RefCell::new(EvalExecutable::new(
        source,
        lexically_scoped_features,
        derived_context_type,
        is_arrow_function_context,
        INSIDE_ORDINARY_FUNCTION,
        eval_context_type,
        NeedsClassFieldInitializer::No,
        PrivateBrandRequirement::None,
    )));

    let mut error = ParserError::new();
    let code_generation_mode = global_object.default_code_generation_mode();
    let executable_source = executable.borrow().source().clone();

    let unlinked_eval_code =
        vm.code_cache().get_unlinked_eval_code_block(&vm, &executable, &executable_source, code_generation_mode, &mut error, eval_context_type);

    finish_parsed_eval(global_object, executable, unlinked_eval_code, &error)
}

/// O que `IndirectEvalExecutable::createImpl` e `DirectEvalExecutable::create` fazem igual depois do
/// parse: avisa o depurador, devolve o objeto de erro se o parse falhou, e senão guarda o
/// `UnlinkedEvalCodeBlock` no executável (`m_unlinkedCodeBlock.set(vm, executable, unlinkedEvalCode)`).
pub(crate) fn finish_parsed_eval(
    global_object: &JSGlobalObject,
    executable: Rc<RefCell<EvalExecutable>>,
    unlinked_eval_code: Option<Rc<RefCell<UnlinkedEvalCodeBlock>>>,
    error: &ParserError,
) -> Result<Rc<RefCell<EvalExecutable>>, JSObjectHandle> {
    let source = executable.borrow().source().clone();
    if global_object.has_debugger() {
        global_object.debugger().source_parsed(
            global_object,
            source.provider().expect("EvalExecutable sem SourceProvider"),
            error.line(),
            error.message(),
        );
    }

    if error.is_valid() {
        return Err(error.to_error_object(global_object, &source).expect("ParserError válido").as_object());
    }

    executable
        .borrow_mut()
        .set_unlinked_code_block(unlinked_eval_code.expect("o cache de código sem erro devolveu um UnlinkedEvalCodeBlock nulo"));
    Ok(executable)
}

/// `IndirectEvalExecutable::create(globalObject, source, features, derivedContextType, isArrowFunctionContext,
/// evalContextType, NakedPtr<JSObject>& resultingError)`: o `Err` é o `resultingError`.
pub fn create(
    global_object: &JSGlobalObject,
    source: &SourceCode,
    lexically_scoped_features: LexicallyScopedFeatures,
    derived_context_type: DerivedContextType,
    is_arrow_function_context: bool,
    eval_context_type: EvalContextType,
) -> Result<Rc<RefCell<EvalExecutable>>, JSObjectHandle> {
    create_impl(global_object, source, lexically_scoped_features, derived_context_type, is_arrow_function_context, eval_context_type)
}

/// `IndirectEvalExecutable::tryCreate(...)`: nulo (`None`) com o erro de sintaxe lançado no `VM`.
pub fn try_create(
    global_object: &JSGlobalObject,
    source: &SourceCode,
    lexically_scoped_features: LexicallyScopedFeatures,
    derived_context_type: DerivedContextType,
    is_arrow_function_context: bool,
    eval_context_type: EvalContextType,
) -> Option<Rc<RefCell<EvalExecutable>>> {
    match create(global_object, source, lexically_scoped_features, derived_context_type, is_arrow_function_context, eval_context_type) {
        Ok(executable) => Some(executable),
        Err(error) => {
            // `throwVMError(globalObject, scope, error->toErrorObject(globalObject, source))`.
            let mut scope = ThrowScope::new(global_object.vm());
            throw_exception(global_object, &mut scope, error);
            None
        }
    }
}
