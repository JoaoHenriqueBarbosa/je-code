//! `op_call_varargs`, `op_tail_call_varargs`, `op_construct_varargs`, `op_super_construct_varargs` e
//! `op_call_direct_eval`: `doCallVarargs` (`LowLevelInterpreter64.asm`) com `slow_path_size_frame_for_varargs` e
//! `varargsSetup` (`LLIntSlowPaths.cpp`), e `commonCallDirectEval` mais `eval(...)` (`Interpreter.cpp`).
//! `sizeOfVarargs`, `sizeFrameForVarargs`, `loadVarargs`, `setupVarargsFrameAndSetThis` e
//! `calleeFrameForVarargs` (`Interpreter.cpp`, `InterpreterInlines.h`) vivem aqui, como funções do `Interpreter`
//! que só o laço de despacho chama.
//!
//! O C++ `op_tail_call_forward_arguments` não existe nesta versão do JavaScriptCore (`BytecodeList.rb` só tem
//! as quatro famílias `*_varargs`), então `sizeFrameForForwardArguments` e `setupForwardArgumentsFrame` não têm
//! chamador e não foram portadas.
//!
//! DIVERGÊNCIAS:
//!
//! - `op_tail_call_varargs` troca o frame como `op_tail_call` (ver `dispatch.rs`). O `Metadata` de
//!   `DataOnlyCallLinkInfo` (`updateMaxArgumentCountIncludingThisForVarargs`) só
//!   alimenta o JIT e não existe.
//! - `loadVarargs` copia de `JSCellButterfly` (o `op_spread`) e, para o resto, lê cada índice com
//!   `JSObject::get` (o `JSArray::copyToArguments` e o laço genérico do C++ têm esse mesmo resultado
//!   observável: o vetor rápido é só atalho). `DirectArguments`, `ScopedArguments`, `ClonedArguments` e os
//!   typed arrays caem no laço genérico (`length` e índices lidos por `get`, o mesmo resultado dos
//!   `copyToArguments` específicos). Célula que não é objeto (nem String, Symbol, BigInt) é o
//!   `RELEASE_ASSERT(arguments.isObject())` do C++.
//! - `eval(...)` não tem o `DirectEvalCodeCache` nem o pré-parser JSON `LiteralParser::tryEval` (as lacunas
//!   de `global_func_eval`, em `execute_eval.rs`), nem `trustedTypes`/`evalEnabled`. A origem da fonte é
//!   `Untainted` (`computeNewSourceTaintedOriginFromStack` não existe). O callee só é o `eval` quando o
//!   `JSGlobalObject` guarda o `evalFunction` (`JSGlobalObject::set_eval_function`); sem ele o
//!   `op_call_direct_eval` chama o callee como uma chamada comum, que é o que o C++ faz com um callee
//!   diferente do `eval`.

use crate::bytecode::bytecode_index::BytecodeIndex;
use crate::bytecode::bytecode_ops::{
    OpCallDirectEval, OpCallVarargs, OpConstructVarargs, OpSuperConstructVarargs, OpTailCallVarargs,
};
use crate::bytecode::code_block::CodeBlockRef;
use crate::bytecode::code_type::CodeType;
use crate::bytecode::executable_info::{DerivedContextType, EvalContextType};
use crate::bytecode::virtual_register::VirtualRegister;
use crate::interpreter::call_frame::{CallFrame, HEADER_SIZE_IN_REGISTERS};
use crate::interpreter::interpreter::Interpreter;
use crate::llint::dispatch::CallOutcome;
use crate::llint::slow_paths::{throw_error_object, throw_stack_overflow_error, SlowPathFrame};
use crate::llint::{LLIntFailure, LLIntResult};
use crate::parser::parser_modes::{is_function_parse_mode, LexicallyScopedFeatures, STRICT_MODE_LEXICALLY_SCOPED_FEATURE};
use crate::parser::source_code::make_source;
use crate::parser::source_provider::{SourceProvider, SourceProviderSourceType};
use crate::parser::source_tainted_origin::SourceTaintedOrigin;
use crate::parser::variable_environment::{PrivateNameEnvironment, TDZEnvironment};
use crate::runtime::cell_registry;
use crate::runtime::code_specialization_kind::CodeSpecializationKind;
use crate::runtime::direct_eval_executable;
use crate::runtime::exception_helpers::{create_invalid_function_apply_parameter_error, ErrorSite};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::js_array::JSArray;
use crate::runtime::js_cell_butterfly::JSCellButterfly;
use crate::runtime::js_global_object::JSGlobalObjectRef;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_scope::{JSScope, JSScopeRef};
use crate::runtime::js_type::JSType;
use crate::runtime::js_value::JSValue;
use crate::runtime::json_object::throw_json_error;
use crate::runtime::literal_parser::{literal_parse, LiteralParseKind, ParseOutcome};
use crate::runtime::math_common::max_safe_integer;
use crate::runtime::property_name::PropertyName;
use crate::runtime::stack_alignment::stack_alignment_registers;
use crate::wtf::math_extras::round_up_to_multiple_of;
use crate::wtf::text::text_position::TextPosition;

/// Os operandos que `op_call_varargs`, `op_tail_call_varargs`, `op_construct_varargs` e
/// `op_super_construct_varargs` têm em comum (`doCallVarargs`).
pub(super) struct VarargsInfo {
    kind: CodeSpecializationKind,
    pub(super) dst: VirtualRegister,
    callee: VirtualRegister,
    this_value: VirtualRegister,
    arguments: VirtualRegister,
    first_free: VirtualRegister,
    first_var_arg: i32,
    /// `op_tail_call_varargs`: o frame do callee substitui o do chamador (`prepareForTailCall`).
    tail: bool,
}

/// `From<Op*> for VarargsInfo`: as quatro structs repetem os mesmos campos (`valueProfile` não é lido).
macro_rules! varargs_info_from {
    ($($op:ty => $kind:ident, $tail:literal),* $(,)?) => {
        $(impl From<$op> for VarargsInfo {
            fn from(op: $op) -> VarargsInfo {
                VarargsInfo {
                    kind: CodeSpecializationKind::$kind,
                    dst: op.dst,
                    callee: op.callee,
                    this_value: op.this_value,
                    arguments: op.arguments,
                    first_free: op.first_free,
                    first_var_arg: op.first_var_arg,
                    tail: $tail,
                }
            }
        })*
    };
}

varargs_info_from! {
    OpCallVarargs => CodeForCall, false,
    OpTailCallVarargs => CodeForCall, true,
    OpConstructVarargs => CodeForConstruct, false,
    OpSuperConstructVarargs => CodeForConstruct, false,
}

/// Os operandos de `op_call_direct_eval` (`commonCallDirectEval`).
pub(super) struct DirectEvalInfo {
    pub(super) dst: VirtualRegister,
    callee: VirtualRegister,
    argc: u32,
    argv: u32,
    this_value: VirtualRegister,
    scope: VirtualRegister,
    lexically_scoped_features: u32,
}

impl From<OpCallDirectEval> for DirectEvalInfo {
    fn from(op: OpCallDirectEval) -> DirectEvalInfo {
        DirectEvalInfo {
            dst: op.dst,
            callee: op.callee,
            argc: op.argc,
            argv: op.argv,
            this_value: op.this_value,
            scope: op.scope,
            lexically_scoped_features: op.lexically_scoped_features,
        }
    }
}

/// `calleeFrameForVarargs(callFrame, numUsedStackSlots, argumentCountIncludingThis)`: o índice do frame do
/// callee, alinhado em tamanho e em deslocamento; `None` quando o frame desceria abaixo do fim da pilha.
fn callee_frame_for_varargs(call_frame: CallFrame, num_used_stack_slots: usize, argument_count_including_this: usize) -> Option<usize> {
    let alignment = stack_alignment_registers() as usize;
    let header = HEADER_SIZE_IN_REGISTERS as usize;
    // We want the new frame to be allocated on a stack aligned offset with a stack aligned size.
    let argument_count_including_this = round_up_to_multiple_of(alignment, argument_count_including_this + header) - header;
    // Align the frame offset here.
    let padded_callee_frame_offset =
        round_up_to_multiple_of(alignment, num_used_stack_slots + argument_count_including_this + header);
    call_frame.registers().checked_sub(padded_callee_frame_offset)
}

/// `clampToUnsigned(toLength(globalObject, object))` do `length` já lido: `ToLength` completo, com o
/// `toPrimitive` do objeto, `Symbol` e `BigInt` lançando `TypeError` (`RETURN_IF_EXCEPTION` do C++).
fn to_length_clamped_to_unsigned(global_object: &JSGlobalObjectRef, length: JSValue) -> LLIntResult<u32> {
    let number = length.to_number();
    if global_object.vm().exception().is_some() {
        return Err(LLIntFailure::Thrown);
    }
    let integer = if number.is_nan() { 0.0 } else { number.trunc() };
    Ok(integer.clamp(0.0, max_safe_integer()).min(f64::from(u32::MAX)) as u32)
}

impl Interpreter {
    /// `sizeOfVarargs(globalObject, arguments, firstVarArgOffset)`.
    fn size_of_varargs(
        &self,
        global_object: &JSGlobalObjectRef,
        arguments: JSValue,
        first_var_arg_offset: u32,
        site: &ErrorSite<'_>,
    ) -> LLIntResult<u32> {
        let invalid_parameter = || {
            throw_error_object(global_object, create_invalid_function_apply_parameter_error(global_object, arguments, Some(site)))
        };
        if !arguments.is_cell() {
            if arguments.is_undefined_or_null() {
                return Ok(0);
            }
            return Err(invalid_parameter());
        }

        let cell_id = arguments.as_cell();
        let vm = global_object.vm();
        let mut length = if let Some(butterfly) = JSCellButterfly::from_cell_id(cell_id) {
            butterfly.length()
        } else if matches!(
            cell_registry::cell_type(cell_id),
            Some(JSType::StringType | JSType::SymbolType | JSType::HeapBigIntType)
        ) {
            return Err(invalid_parameter());
        } else if let Some(array) = JSArray::from_cell_id(cell_id) {
            array.length()
        } else {
            // `ObjectRef` e não `JSObject`: a função materializa o `length` preguiçoso no `getOwnPropertySlot`.
            // `default: RELEASE_ASSERT(arguments.isObject())`.
            let object = ObjectRef::from_value(&arguments).expect("RELEASE_ASSERT: argumentos de varargs é objeto");
            let length = object.get(global_object, &PropertyName::from_identifier(&vm.property_names.length));
            if vm.exception().is_some() {
                return Err(LLIntFailure::Thrown);
            }
            to_length_clamped_to_unsigned(global_object, length)?
        };

        if length as usize > Interpreter::MAX_ARGUMENTS {
            return Err(throw_stack_overflow_error(global_object));
        }
        length = length.saturating_sub(first_var_arg_offset);
        Ok(length)
    }

    /// `sizeFrameForVarargs`: o número de argumentos, depois de conferir que o frame do callee cabe na pilha.
    /// Devolve também o índice do frame do callee (`calleeFrameForVarargs(callFrame, numUsedStackSlots, length + 1)`).
    fn size_frame_for_varargs(
        &self,
        global_object: &JSGlobalObjectRef,
        call_frame: CallFrame,
        arguments: JSValue,
        num_used_stack_slots: usize,
        first_var_arg_offset: u32,
        site: &ErrorSite<'_>,
    ) -> LLIntResult<(u32, CallFrame)> {
        let length = self.size_of_varargs(global_object, arguments, first_var_arg_offset, site)?;
        match callee_frame_for_varargs(call_frame, num_used_stack_slots, length as usize + 1) {
            Some(base) if length as usize <= Interpreter::MAX_ARGUMENTS && self.stack.ensure_capacity_for(base as isize) => {
                Ok((length, CallFrame::create(base)))
            }
            _ => Err(throw_stack_overflow_error(global_object)),
        }
    }

    /// `loadVarargs(globalObject, firstElementDest, arguments, offset, length)`: os argumentos do callee a
    /// partir de `arguments`, começando do índice `offset`; o que falta é `undefined`.
    fn load_varargs(
        &self,
        global_object: &JSGlobalObjectRef,
        callee_frame: CallFrame,
        arguments: JSValue,
        offset: u32,
        length: u32,
    ) -> LLIntResult<()> {
        if !arguments.is_cell() || length == 0 {
            return Ok(());
        }
        let vm = global_object.vm();
        let cell_id = arguments.as_cell();
        if let Some(butterfly) = JSCellButterfly::from_cell_id(cell_id) {
            // JSCellButterfly::copyToArguments.
            for i in 0..length {
                let value = match i.checked_add(offset) {
                    Some(index) if index < butterfly.length() => butterfly.get(index),
                    _ => JSValue::undefined(),
                };
                callee_frame.set_argument(&self.stack, i as usize, value);
            }
            return Ok(());
        }
        // `default: RELEASE_ASSERT(arguments.isObject())`: `sizeOfVarargs` já rejeitou String, Symbol e BigInt.
        let object = JSObject::from_cell_id(cell_id).expect("RELEASE_ASSERT: argumentos de varargs é objeto");
        for i in 0..length {
            let value = object.get_by_index(vm, i + offset);
            if vm.exception().is_some() {
                return Err(LLIntFailure::Thrown);
            }
            callee_frame.set_argument(&self.stack, i as usize, value);
        }
        Ok(())
    }

    /// `doCallVarargs`: `slow_path_size_frame_for_varargs`, `varargsSetup` (`setupVarargsFrameAndSetThis`) e a
    /// chamada em `calleeFrame` (`setUpCall`). `#[inline(never)]` mantém os locais fora do frame de
    /// `dispatch_loop_from`, que fica na pilha nativa a cada nível de JS.
    #[inline(never)]
    pub(super) fn call_varargs(
        &mut self,
        call_frame: CallFrame,
        code_block: &CodeBlockRef,
        global_object: &JSGlobalObjectRef,
        info: VarargsInfo,
        pc: u32,
        return_pc: u32,
    ) -> LLIntResult<CallOutcome> {
        let VarargsInfo { kind, callee, this_value, arguments, first_free, first_var_arg, tail, .. } = info;
        let vm = global_object.vm();
        let (callee_value, this_value, arguments) = {
            let block = code_block.borrow();
            let f = SlowPathFrame { vm, call_frame, stack: &self.stack, code_block: &block };
            (f.get(callee), f.get(this_value), f.get(arguments))
        };
        // O frame visível acima de um privado lê o `currentVPC` do frame corrente: grava antes de qualquer erro.
        call_frame.set_current_vpc(&mut self.stack, BytecodeIndex::from_offset(pc));
        let site = ErrorSite { code_block, bytecode_index: BytecodeIndex::from_offset(pc), vm, call_frame };

        // slow_path_size_frame_for_varargs: `numUsedStackSlots = -firstFree.offset()`.
        let num_used_stack_slots = (-first_free.offset()) as usize;
        let (length, callee_frame) =
            self.size_frame_for_varargs(global_object, call_frame, arguments, num_used_stack_slots, first_var_arg as u32, &site)?;

        // varargsSetup: setupVarargsFrameAndSetThis. Como o `sp` do C++ já desceu ao frame do callee
        // (`slow_path_size_frame_for_varargs`), o JS que `loadVarargs` roda (getter de índice, proxy) monta os
        // frames dele abaixo dos argumentos já copiados, sem sobrescrevê-los.
        let saved_sp = self.sp();
        self.set_sp(callee_frame.registers());
        let loaded = self.load_varargs(global_object, callee_frame, arguments, first_var_arg as u32, length);
        self.set_sp(saved_sp);
        loaded?;
        callee_frame.set_argument_count_including_this(&self.stack, length as i32 + 1);
        callee_frame.set_this_value(&self.stack, this_value);
        self.call_prepared_frame(call_frame, code_block, global_object, callee_frame, callee_value, kind, tail, pc, return_pc)
    }

    /// `commonCallDirectEval`: `eval(...)` quando o callee é o `eval` do realm; qualquer outro callee é uma
    /// chamada comum (`setUpCall` com `CodeForCall`). `#[inline(never)]` pelo mesmo motivo de `call_varargs`.
    #[inline(never)]
    pub(super) fn call_direct_eval(
        &mut self,
        call_frame: CallFrame,
        code_block: &CodeBlockRef,
        global_object: &JSGlobalObjectRef,
        info: DirectEvalInfo,
        pc: u32,
        return_pc: u32,
    ) -> LLIntResult<CallOutcome> {
        let DirectEvalInfo { callee, argc, argv, this_value, scope, lexically_scoped_features, .. } = info;
        let vm = global_object.vm();
        let (callee_value, this_value, scope_value) = {
            let block = code_block.borrow();
            let f = SlowPathFrame { vm, call_frame, stack: &self.stack, code_block: &block };
            (f.get(callee), f.get(this_value), f.get(scope))
        };

        // `calleeFrame = callFrame - bytecode.m_argv`, com o cabeçalho montado antes de `eval`.
        // O prólogo do chamador já reservou `m_argv` registradores abaixo do frame (`maxFrameExtent`), então a
        // subtração do C++ nunca passa do fim da pilha.
        let callee_base = call_frame
            .registers()
            .checked_sub(argv as usize)
            .expect("RELEASE_ASSERT: frame do callee do eval direto dentro da pilha");
        let callee_frame = CallFrame::create(callee_base);
        callee_frame.set_argument_count_including_this(&self.stack, argc as i32);
        callee_frame.set_caller_frame(&self.stack, Some(call_frame));
        callee_frame.set_code_block(&self.stack, None);
        callee_frame.clear_return_pc(&self.stack);
        if callee_value.is_cell() {
            callee_frame.set_callee(&self.stack, callee_value.as_cell());
        }

        // eval(): `callFrame->guaranteedJSValueCallee() != globalObject->evalFunction()` devolve o valor vazio.
        let is_eval_function = callee_value.is_cell() && global_object.eval_function() == Some(callee_value.as_cell());
        if !is_eval_function {
            return self.call_prepared_frame(
                call_frame,
                code_block,
                global_object,
                callee_frame,
                callee_value,
                CodeSpecializationKind::CodeForCall,
                false,
                pc,
                return_pc,
            );
        }

        // `jsCast<JSScope*>(callFrame->uncheckedR(scope).jsValue())`.
        let scope = if scope_value.is_cell() { JSScope::from_cell_id(scope_value.as_cell()) } else { None }
            .expect("RELEASE_ASSERT: registrador de escopo do eval direto é um JSScope");
        let bytecode_index = call_frame.bytecode_index(&self.stack);
        self.direct_eval(
            global_object,
            code_block,
            bytecode_index,
            callee_frame,
            this_value,
            &scope,
            lexically_scoped_features as LexicallyScopedFeatures,
        )
        .map(CallOutcome::Value)
    }

    /// `eval(callFrame, thisValue, callerScopeChain, callerBaselineCodeBlock, bytecodeIndex, lexicallyScopedFeatures)`
    /// depois da conferência do callee: o argumento que não é string volta como está, a string vira um
    /// `DirectEvalExecutable` no contexto do `CodeBlock` chamador (ou sai do `DirectEvalCodeCache` dele,
    /// pela chave texto e `bytecodeIndex`) e `executeEval` o roda no escopo do chamador. Fora do modo
    /// estrito, o texto que o `LiteralParser` aceita como JSON solto (`SloppyJSON`) vira valor sem compilar.
    #[allow(clippy::too_many_arguments)]
    fn direct_eval(
        &mut self,
        global_object: &JSGlobalObjectRef,
        caller_code_block: &CodeBlockRef,
        bytecode_index: BytecodeIndex,
        callee_frame: CallFrame,
        this_value: JSValue,
        caller_scope_chain: &JSScopeRef,
        lexically_scoped_features: LexicallyScopedFeatures,
    ) -> LLIntResult<JSValue> {
        if callee_frame.argument_count(&self.stack) == 0 {
            return Ok(JSValue::undefined());
        }
        let program = callee_frame.unchecked_argument(&self.stack, 0);
        let program = crate::runtime::dynamic_code_guard::guard_eval(global_object, program).map_err(|()| LLIntFailure::Thrown)?;
        if !program.is_string() {
            return Ok(program);
        }
        let program_source = program.to_wtf_string();
        // Formato de pilha do V8: de onde o `eval` veio, calculado antes de qualquer empréstimo do `CodeBlock` chamador.
        let eval_origin = crate::runtime::stack_frame::v8_stack_format()
            .then(|| crate::runtime::v8_stack::eval_origin(global_object, callee_frame));

        let cached = caller_code_block.borrow_mut().direct_eval_code_cache().get(&program_source, bytecode_index);
        if let Some(eval) = cached {
            return self.try_execute_eval(&eval, this_value, caller_scope_chain);
        }

        if lexically_scoped_features & STRICT_MODE_LEXICALLY_SCOPED_FEATURE == 0 {
            match literal_parse(&**global_object, &program_source, LiteralParseKind::Sloppy, None) {
                Ok(ParseOutcome::Value(value)) => return Ok(value),
                Ok(ParseOutcome::Failed(_)) => {}
                Err(error) => {
                    throw_json_error(global_object, error);
                    return Err(LLIntFailure::Thrown);
                }
            }
        }

        let mut variables_under_tdz = TDZEnvironment::default();
        let mut private_name_environment = PrivateNameEnvironment::default();
        JSScope::collect_closure_variables_under_tdz(
            Some(caller_scope_chain.clone()),
            &mut variables_under_tdz,
            &mut private_name_environment,
        );

        let eval = {
            let block = caller_code_block.borrow();
            let unlinked = block.unlinked_code_block().borrow();
            let is_arrow_function_context = unlinked.is_arrow_function() || unlinked.is_arrow_function_context();

            let mut derived_context_type = unlinked.derived_context_type();
            if !is_arrow_function_context && unlinked.is_class_context() {
                derived_context_type = if unlinked.is_constructor() {
                    DerivedContextType::DerivedConstructorContext
                } else {
                    DerivedContextType::DerivedMethodContext
                };
            }

            let eval_context_type = if is_function_parse_mode(unlinked.parse_mode()) || unlinked.code_type() == CodeType::EvalCode {
                unlinked.eval_context_type()
            } else {
                EvalContextType::None
            };

            let caller_source = block.source();
            let source_origin = caller_source
                .provider()
                .expect("RELEASE_ASSERT: CodeBlock chamador tem SourceProvider")
                .source_origin()
                .clone();
            let source = make_source(
                &program_source,
                &source_origin,
                SourceTaintedOrigin::Untainted,
                // Bun: o código do `eval` mostra a URL da origem do chamador nas linhas de `stack`.
                source_origin.string().clone(),
                TextPosition::default(),
                SourceProviderSourceType::Program,
            );
            if let Some(origin) = eval_origin {
                if let Some(provider) = source.provider() {
                    provider.set_eval_origin(origin);
                }
            }
            direct_eval_executable::create(
                global_object,
                &source,
                lexically_scoped_features,
                derived_context_type,
                unlinked.needs_class_field_initializer(),
                unlinked.private_brand_requirement(),
                is_arrow_function_context,
                block.owner_executable().is_inside_ordinary_function(),
                eval_context_type,
                Some(&variables_under_tdz),
                Some(&private_name_environment),
            )
        };
        // `EXCEPTION_ASSERT(!!scope.exception() == !eval)`. O `SyntaxError` da análise nasce dentro do `eval`
        // nativo: a pilha dele começa pelo frame do callee (`at eval (unknown)` entre o `<parse>` e o chamador).
        let Some(eval) = eval else {
            if let Some(exception) = global_object.vm().exception() {
                self.capture_stack_for_exception(global_object, &exception, callee_frame, true);
            }
            return Err(LLIntFailure::Thrown);
        };
        // O texto é sempre `Untainted` aqui, então o cache vale (o C++ pula o cache de texto suspeito).
        caller_code_block.borrow_mut().direct_eval_code_cache().set(&program_source, bytecode_index, &eval);
        self.try_execute_eval(&eval, this_value, caller_scope_chain)
    }
}
