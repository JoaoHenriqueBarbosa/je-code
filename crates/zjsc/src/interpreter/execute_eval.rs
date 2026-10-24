//! Tradução de `Interpreter::executeEval` (`interpreter/Interpreter.cpp`), o caminho que o `eval`
//! indireto (`globalFuncEval`) e o direto (`op_call_direct_eval`) usam para executar um `EvalExecutable`.
//!
//! DIVERGÊNCIAS:
//!
//! - Segue o formato de `execute_program`: `try_execute_eval` devolve as lacunas do porte como `Err`, e
//!   `execute_eval` o valor, ou o `JSValue` vazio com a exceção pendente no `VM`.
//! - `StrictEvalActivation` é um `JSWithScope` marcado (`JSWithScope::create_strict_eval_activation`), não uma
//!   variante nova de `JSScopeRef`: o `var` de eval estrito vira propriedade do objeto de protótipo nulo
//!   que ele embrulha (`ensureBindingExists`), `isWithScope()` é falso e o `type()` é
//!   `StrictEvalActivationType`.
//! - `varInjectionWatchpointSet().fireAll(...)` não existe (o watchpoint só serve ao JIT e aos
//!   `ResolveType::*WithVarInjectionChecks` do tier otimizado; o LLInt do porte sempre verifica).
//! - Sem `VMEntryScope`, `DeferTraps` e `didEnterVM` (ver `execute_call.rs`). `ENABLE(DFG_JIT)` vale o
//!   ramo `#else` de `vmEntryToJavaScript(jitCode->addressForCall(), &vm, &protoCallFrame)`.
//! - `JSScope::resolveScopeForHoistingFuncDeclInEval` devolve o valor vazio no lugar do `{ }` do
//!   `RETURN_IF_EXCEPTION`; o vazio sem exceção pendente é lido como "não resolvido" (`undefined`).
//! - `ensureBindingExists` é o `JSScopeRef::put` com `shouldThrow = true`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::bytecode::code_block::CodeBlockRef;
use crate::bytecode::executable_info::{DerivedContextType, EvalContextType};
use crate::interpreter::interpreter::Interpreter;
use crate::interpreter::proto_call_frame::ProtoCallFrame;
use crate::llint::llint_jit_code::LLIntEntry;
use crate::llint::slow_paths::throw_stack_overflow_error;
use crate::llint::{LLIntFailure, LLIntResult};
use crate::parser::parser_modes::{NO_LEXICALLY_SCOPED_FEATURES, TAINTED_BY_WITH_SCOPE_LEXICALLY_SCOPED_FEATURE};
use crate::parser::source_code::make_source;
use crate::parser::source_provider::SourceProviderSourceType;
use crate::parser::source_tainted_origin::SourceTaintedOrigin;
use crate::runtime::indirect_eval_executable::try_create as try_create_indirect_eval;
use crate::runtime::source_origin::SourceOrigin;
use crate::wtf::text::text_position::TextPosition;
use crate::runtime::arity_check_mode::ArityCheckMode;
use crate::runtime::batched_transition_optimizer::BatchedTransitionOptimizer;
use crate::runtime::code_specialization_kind::CodeSpecializationKind;
use crate::runtime::error::create_syntax_error;
use crate::runtime::eval_executable::EvalExecutable;
use crate::runtime::exception_helpers::{
    create_error_for_invalid_global_function_declaration, create_error_for_invalid_global_var_declaration,
};
use crate::runtime::identifier::Identifier;
use crate::runtime::js_global_object::{BindingCreationContext, JSGlobalObjectRef};
use crate::runtime::js_scope::{JSScope, JSScopeRef};
use crate::runtime::js_value::{js_undefined, JSValue};
use crate::runtime::js_with_scope::JSWithScope;
use crate::runtime::property_name::PropertyName;
use crate::runtime::script_executable::ScriptExecutableRef;
use crate::runtime::symbol_table::ScopeType;
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::wtf::text::string_concatenate::make_string_dyn;

/// `throwSyntaxError(globalObject, scope, "Can't create duplicate variable in eval: '<ident>'")`.
fn throw_duplicate_variable_in_eval(global_object: &JSGlobalObjectRef, ident: &Identifier) -> LLIntFailure {
    let message = make_string_dyn(&[&"Can't create duplicate variable in eval: '", ident.string().string(), &'\'']);
    let mut throw_scope = ThrowScope::new(global_object.vm());
    throw_exception(global_object, &mut throw_scope, create_syntax_error(global_object, &message));
    LLIntFailure::Thrown
}

/// `JSScope::resolveScopeForHoistingFuncDeclInEval(...)` seguido do `RETURN_IF_EXCEPTION`: `true` quando
/// o identificador não resolve a um escopo de variável (o `isUndefined()` do C++).
fn is_unresolved_for_hoisting(global_object: &JSGlobalObjectRef, scope: &JSScopeRef, ident: &Identifier) -> LLIntResult<bool> {
    let resolved = JSScope::resolve_scope_for_hoisting_func_decl_in_eval(global_object, scope, ident);
    if global_object.vm().exception().is_some() {
        return Err(LLIntFailure::Thrown);
    }
    Ok(resolved.is_empty() || resolved.is_undefined())
}

/// O `variableObject` de um `eval` sloppy: o primeiro escopo que é o global object ou um
/// `JSLexicalEnvironment` de `ScopeType::VarScope` (um `StrictEvalActivation` na cadeia não conta, como no C++).
fn find_variable_object(scope: &JSScopeRef) -> JSScopeRef {
    let mut node = scope.clone();
    loop {
        if node.is_global_object() {
            return node;
        }
        if node.is_js_lexical_environment() {
            if let Some(symbol_table) = node.symbol_table() {
                if symbol_table.borrow().scope_type() == ScopeType::VarScope {
                    return node;
                }
            }
        }
        node = node.next().expect("RELEASE_ASSERT(node)");
    }
}

impl Interpreter {
    /// `Interpreter::executeEval(EvalExecutable*, JSValue thisValue, JSScope*)` com as lacunas do porte
    /// como `Err`. A exceção do JS fica pendente no `VM` e sai como `Err(LLIntFailure::Thrown)`.
    pub fn try_execute_eval(
        &mut self,
        eval: &Rc<RefCell<EvalExecutable>>,
        this_value: JSValue,
        scope: &JSScopeRef,
    ) -> LLIntResult<JSValue> {
        let global_object = scope.realm();
        let vm = global_object.vm();
        debug_assert!(vm.exception().is_none());

        if !vm.is_safe_to_recurse() {
            return Err(throw_stack_overflow_error(&global_object));
        }

        let (top_level_function_decls, variables, function_hoisting_candidates, is_in_strict_context) = {
            let eval = eval.borrow();
            (
                eval.num_top_level_function_decls(),
                eval.variables(),
                eval.function_hoisting_candidates(),
                eval.is_in_strict_context(),
            )
        };
        let mut scope = scope.clone();

        if !variables.is_empty() || top_level_function_decls != 0 || !function_hoisting_candidates.is_empty() {
            let variable_object = if (!variables.is_empty() || top_level_function_decls != 0) && is_in_strict_context {
                scope = JSScopeRef::WithScope(JSWithScope::create_strict_eval_activation(vm, &global_object, Some(scope)));
                scope.clone()
            } else {
                // Como no C++, só esse ramo procura o escopo de variáveis (e achatar o dicionário uncacheable).
                let found = find_variable_object(&scope);
                if found.scope().structure().is_uncacheable_dictionary() {
                    found.scope().flatten_dictionary_object(vm);
                }
                found
            };

            let code_block = self.prepare_eval_code_block(eval, &scope)?;
            // `ASSERT(codeBlock && codeBlock->numParameters() == 1); // 1 parameter for 'this'.`
            debug_assert!(code_block.borrow().num_parameters() == 1);

            let function_decls = code_block.borrow().function_decls().to_vec();
            let _optimizer = BatchedTransitionOptimizer::new(vm, variable_object.scope());

            if !is_in_strict_context {
                for ident in &variables {
                    if is_unresolved_for_hoisting(&global_object, &scope, ident)? {
                        return Err(throw_duplicate_variable_in_eval(&global_object, ident));
                    }
                }

                for function in &function_decls {
                    let name = function.borrow().name();
                    if is_unresolved_for_hoisting(&global_object, &scope, &name)? {
                        return Err(throw_duplicate_variable_in_eval(&global_object, &name));
                    }
                }
            }

            let global_variable_object = match &variable_object {
                JSScopeRef::GlobalObject(global) => Some(Rc::clone(global)),
                _ => None,
            };
            if let Some(global) = &global_variable_object {
                for function in &function_decls {
                    let name = function.borrow().name();
                    if !global.can_declare_global_function(&name) {
                        let error = create_error_for_invalid_global_function_declaration(&global_object, &name);
                        let mut throw_scope = ThrowScope::new(vm);
                        throw_exception(&global_object, &mut throw_scope, error);
                        return Err(LLIntFailure::Thrown);
                    }
                }

                if !variable_object.scope().is_structure_extensible() {
                    for ident in &variables {
                        if !global.can_declare_global_var(ident) {
                            let error = create_error_for_invalid_global_var_declaration(&global_object, ident);
                            let mut throw_scope = ThrowScope::new(vm);
                            throw_exception(&global_object, &mut throw_scope, error);
                            return Err(LLIntFailure::Thrown);
                        }
                    }
                }
            }

            // `ensureBindingExists`.
            let ensure_binding_exists = |ident: &Identifier| -> LLIntResult<()> {
                let exists = if variable_object.is_strict_eval_activation() {
                    variable_object.has_property(&global_object, ident)
                } else {
                    variable_object.scope().has_own_property(vm, &PropertyName::from_identifier(ident))
                };
                if !exists {
                    // `shouldThrow = true`: o `PutError` vira a exceção pendente, como o `ThrowScope` do C++.
                    if let Err(error) = variable_object.put(&global_object, ident, js_undefined(), true, crate::runtime::put_property_slot::PutContext::UnknownContext, false) {
                        crate::runtime::host_function_support::throw_put_error(&global_object, error);
                        return Err(LLIntFailure::Thrown);
                    }
                    if vm.exception().is_some() {
                        return Err(LLIntFailure::Thrown);
                    }
                }
                Ok(())
            };

            if !is_in_strict_context {
                for ident in &function_hoisting_candidates {
                    if !is_unresolved_for_hoisting(&global_object, &scope, ident)? {
                        match &global_variable_object {
                            Some(global) => {
                                if global.can_declare_global_var(ident) {
                                    global.create_global_var_binding(BindingCreationContext::Eval, ident);
                                }
                            }
                            None => ensure_binding_exists(ident)?,
                        }
                    }
                }
            }

            for function in &function_decls {
                let name = function.borrow().name();
                match &global_variable_object {
                    Some(global) => global.create_global_function_binding(BindingCreationContext::Eval, &name),
                    None => ensure_binding_exists(&name)?,
                }
            }

            for ident in &variables {
                match &global_variable_object {
                    Some(global) => global.create_global_var_binding(BindingCreationContext::Eval, ident),
                    None => ensure_binding_exists(ident)?,
                }
            }
        }

        // `EvalCodeBlock* codeBlock = nullptr; JSCallee* callee = globalObject->evalCallee();`
        // Reload CodeBlock. It is possible that we replaced CodeBlock while setting up the environment.
        let code_block = self.prepare_eval_code_block(eval, &scope)?;
        let num_parameters = code_block.borrow().num_parameters();
        debug_assert!(num_parameters == 1);

        let callee = global_object.eval_callee();
        let code_block_id = self.register_code_block(&code_block);
        let jit_code = code_block.borrow().jit_code();
        let entry = LLIntEntry::from_code_ptr(jit_code.address_for_call(ArityCheckMode::ArityCheckNotRequired))
            .expect("RELEASE_ASSERT(JIT desligado): o JITCode do eval é o ponto de entrada do LLInt");

        let global_object_cell = JSScopeRef::GlobalObject(Rc::clone(&global_object)).cell_id();
        let mut proto_call_frame = ProtoCallFrame::default();
        proto_call_frame.init(Some(code_block_id), num_parameters, global_object_cell, callee.cell_id(), this_value, None, 1, Vec::new());

        // eval code only uses scope at the beginning (op_enter).
        // We can replace the current scope for the subsequent run.
        callee.set_scope(Some(scope.clone()));
        let result = self.vm_entry_to_javascript(&global_object, proto_call_frame, entry);
        callee.set_scope(None);
        result
    }

    /// `globalFuncEval(globalObject, callFrame)` (`JSGlobalObjectFunctions.cpp`), o `eval` indireto, sobre o
    /// primeiro argumento `x` e a origem do chamador (`callFrame->callerSourceOrigin(vm)` e
    /// `computeNewSourceTaintedOriginFromStack`, que a host function lê da pilha).
    ///
    /// DIVERGÊNCIAS: um `x` que não é string volta como está (o ramo de `Options::useTrustedTypes()` e
    /// `codeForEval` não existe); `evalEnabled`, `canCompileStrings` e `SourceProfiler::g_profilerHook` não
    /// existem; o pré-parser JSON `LiteralParser::tryEval` (`eval("{\"a\":1}")` e literais JSON em geral)
    /// ainda não está ligado: `runtime/literal_parser.rs` não é módulo e depende de um `JsonHost`.
    pub fn global_func_eval(
        &mut self,
        global_object: &JSGlobalObjectRef,
        x: JSValue,
        caller_source_origin: &SourceOrigin,
        tainted_origin: SourceTaintedOrigin,
        eval_origin: Option<String>,
    ) -> LLIntResult<JSValue> {
        let x = crate::runtime::dynamic_code_guard::guard_eval(global_object, x).map_err(|()| LLIntFailure::Thrown)?;
        if !x.is_string() {
            return Ok(x);
        }
        let program_source = x.to_wtf_string();

        let lexically_scoped_features = if global_object.global_scope_extension().is_some() {
            TAINTED_BY_WITH_SCOPE_LEXICALLY_SCOPED_FEATURE
        } else {
            NO_LEXICALLY_SCOPED_FEATURES
        };
        let source = make_source(
            &program_source,
            caller_source_origin,
            tainted_origin,
            // Bun: o código do `eval` mostra a URL da origem do chamador nas linhas de `stack`.
            caller_source_origin.string().clone(),
            TextPosition::default(),
            SourceProviderSourceType::Program,
        );
        if let Some(provider) = source.provider() {
            provider.mark_eval_code();
        }
        // Formato de pilha do V8: os frames do código mostram de onde o `eval` veio.
        if let (Some(origin), Some(provider)) = (eval_origin, source.provider()) {
            provider.set_eval_origin(origin);
        }
        let eval = try_create_indirect_eval(
            global_object,
            &source,
            lexically_scoped_features,
            DerivedContextType::None,
            false,
            EvalContextType::None,
        )
        .ok_or(LLIntFailure::Thrown)?;

        let this_value = match global_object.global_this() {
            Some(global_this) => global_this.as_value(),
            None => JSValue::from_cell(JSScopeRef::GlobalObject(Rc::clone(global_object)).cell_id()),
        };
        self.try_execute_eval(&eval, this_value, &global_object.global_scope())
    }

    /// `Interpreter::executeEval`: o valor de conclusão do eval, ou o `JSValue` vazio com a exceção
    /// pendente no `VM`.
    pub fn execute_eval(&mut self, eval: &Rc<RefCell<EvalExecutable>>, this_value: JSValue, scope: &JSScopeRef) -> JSValue {
        let result = self.try_execute_eval(eval, this_value, scope);
        Interpreter::value_or_pending_exception(result, &scope.realm())
    }

    /// `eval->prepareForExecution<EvalExecutable>(vm, nullptr, scope, CodeForCall, tempCodeBlock)` seguido do
    /// `RETURN_IF_EXCEPTION`.
    fn prepare_eval_code_block(&self, eval: &Rc<RefCell<EvalExecutable>>, scope: &JSScopeRef) -> LLIntResult<CodeBlockRef> {
        let vm = scope.realm().vm_rc();
        let mut code_block: Option<CodeBlockRef> = None;
        ScriptExecutableRef::Eval(Rc::clone(eval)).prepare_for_execution(
            &vm,
            None,
            scope,
            CodeSpecializationKind::CodeForCall,
            &mut code_block,
        );
        if vm.exception().is_some() {
            return Err(LLIntFailure::Thrown);
        }
        Ok(code_block.expect("ASSERT(codeBlock): prepareForExecution sem exceção pendente devolve um CodeBlock"))
    }
}
