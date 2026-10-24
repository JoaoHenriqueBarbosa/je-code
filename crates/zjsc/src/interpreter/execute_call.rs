//! Tradução de `Interpreter::executeCall`, `executeCallImpl` e `executeConstruct` (`Interpreter.cpp`):
//! a entrada na VM por uma chamada vinda do hospedeiro (`call`/`construct` de `CallData.cpp` e
//! `ConstructData.cpp`), montando o `ProtoCallFrame` e entrando no laço do LLInt.
//!
//! DIVERGÊNCIAS:
//!
//! - `executeCallImpl` e `executeConstruct` diferem só no `CodeSpecializationKind`, no slot `this` do
//!   `ProtoCallFrame` (o `thisValue` de uma chamada, o `newTarget` de uma construção) e no contexto
//!   (`nullptr` na construção). O corpo é um só (`enter_callee`), com essas três diferenças como
//!   parâmetro.
//! - Sem `VMEntryScope`, `DeferTraps`, `didEnterVM`, `m_shouldAlwaysBeInlined`, `disallowVMEntryCount` e
//!   `Wasm` (nada deles é observável sem o inspetor, os traps e o JIT). `isSafeToRecurseSoft` é o
//!   `isSafeToRecurse` do `VM` do porte.
//! - `executeBoundCall` não monta um `MarkedArgumentBuffer`: os argumentos combinados são um `Vec`, sem o
//!   `hasOverflowed()` (o limite de `maxArguments` do `executeCallImpl` cobre o mesmo caso). `isTainted` e
//!   `setMightBeExecutingTaintedCode` não existem (o porte não rastreia código contaminado).
//! - `vmEntryToNative` monta o frame (o mesmo prólogo do `doVMEntry`), chama a função nativa com o
//!   [`NativeCallFrame`] (o frame sobre a pilha única) e trata o desfecho: exceção pendente no `VM` vira
//!   `Thrown`, senão o `EncodedJSValue` devolvido é o resultado. A `NativeFunction` recebe o
//!   `&JSGlobalObject` (o `JSGlobalObject*` do C++ é compartilhado e o objeto muta por mutabilidade
//!   interior), porque o realm vem de um `Rc` e não há `&mut` a emprestar.
//! - `executeConstruct` devolve o `JSObject*` como o `JSValue` do objeto.

use std::rc::Rc;

use crate::interpreter::call_frame::{CallFrame, NativeCallFrame};
use crate::interpreter::interpreter::Interpreter;
use crate::interpreter::proto_call_frame::ProtoCallFrame;
use crate::llint::llint_jit_code::LLIntEntry;
use crate::llint::slow_paths::throw_stack_overflow_error;
use crate::llint::{LLIntFailure, LLIntResult};
use crate::runtime::arity_check_mode::ArityCheckMode;
use crate::runtime::call_data::{realm_for_call, CallData};
use crate::runtime::code_specialization_kind::CodeSpecializationKind;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_scope::JSScopeRef;
use crate::runtime::js_value::JSValue;
use crate::runtime::native_function::TaggedNativeFunction;
use crate::runtime::script_executable::ScriptExecutableRef;

impl Interpreter {
    /// `Interpreter::maxArguments`.
    pub const MAX_ARGUMENTS: usize = 0x100000;

    /// `Interpreter::executeCall(function, callData, thisValue, context, args)`. `function` é o
    /// `JSObject*` (o `JSValue` da célula) e `context` o `JSCell*` opcional do `ProtoCallFrame`.
    pub fn execute_call(
        &mut self,
        function: JSValue,
        call_data: &CallData,
        this_value: JSValue,
        context: Option<usize>,
        args: &[JSValue],
    ) -> LLIntResult<JSValue> {
        let CallData::Native { is_bound_function: true, .. } = call_data else {
            return self.enter_callee(CodeSpecializationKind::CodeForCall, function, call_data, this_value, context, args);
        };
        // Only one-level unwrap is enough! O nível desembrulhado não entra no VM: chama direto o alvo, com o `this`
        // e os argumentos ligados.
        let callee = function.as_js_function().expect("callData.native.isBoundFunction com callee que não é JSFunction");
        let bound = callee.as_bound_function().expect("callData.native.isBoundFunction com callee que não é JSBoundFunction");
        let target = bound.target_function();
        let target_call_data = crate::runtime::call_data::get_call_data(target);
        debug_assert!(!target_call_data.is_none());
        if bound.bound_args_length() == 0 {
            // This is the simplest path, just replacing |this|. We do not need to go to executeBoundCall.
            return self.enter_callee(CodeSpecializationKind::CodeForCall, target, &target_call_data, bound.bound_this(), context, args);
        }
        // `executeBoundCall`.
        let mut combined_args = Vec::with_capacity(bound.bound_args_length() + args.len());
        combined_args.extend_from_slice(bound.bound_args());
        combined_args.extend_from_slice(args);
        self.enter_callee(CodeSpecializationKind::CodeForCall, target, &target_call_data, bound.bound_this(), context, &combined_args)
    }

    /// `Interpreter::executeConstruct(constructor, constructData, args, newTarget)`: o slot `this` do
    /// frame carrega o `newTarget`, e quem cria o objeto é o `op_create_this` do bytecode do construtor.
    pub fn execute_construct(
        &mut self,
        constructor: JSValue,
        construct_data: &CallData,
        args: &[JSValue],
        new_target: JSValue,
    ) -> LLIntResult<JSValue> {
        self.enter_callee(CodeSpecializationKind::CodeForConstruct, constructor, construct_data, new_target, None, args)
    }

    /// O corpo comum de `executeCallImpl` e `executeConstruct`.
    fn enter_callee(
        &mut self,
        kind: CodeSpecializationKind,
        function: JSValue,
        call_data: &CallData,
        this_slot: JSValue,
        context: Option<usize>,
        args: &[JSValue],
    ) -> LLIntResult<JSValue> {
        let global_object = realm_for_call(function, call_data);
        let vm = global_object.vm();
        // scope.assertNoException()
        debug_assert!(vm.exception().is_none());

        let args_count = 1 + args.len(); // implicit "this" parameter
        if !vm.is_safe_to_recurse() || args.len() > Interpreter::MAX_ARGUMENTS {
            return Err(throw_stack_overflow_error(&global_object));
        }

        let global_object_cell = JSScopeRef::GlobalObject(Rc::clone(&global_object)).cell_id();
        let encoded_args = args.iter().map(|arg| arg.encode()).collect();
        let mut proto_call_frame = ProtoCallFrame::default();

        match call_data {
            CallData::JS { function_executable, scope } => {
                let js_function = function
                    .as_js_function()
                    .expect("ASSERT(callData.type == CallData::Type::JS): o callee de CallData::JS é uma JSFunction");

                // Compile the callee:
                let mut code_block = None;
                ScriptExecutableRef::Function(Rc::clone(function_executable)).prepare_for_execution(
                    vm,
                    Some(&*js_function),
                    scope,
                    kind,
                    &mut code_block,
                );
                if vm.exception().is_some() {
                    return Err(LLIntFailure::Thrown);
                }
                let code_block =
                    code_block.expect("ASSERT(codeBlock): prepareForExecution sem exceção pendente devolve um CodeBlock");
                let num_parameters = code_block.borrow().num_parameters();
                let code_block_id = self.register_code_block(&code_block);
                let jit_code = code_block.borrow().jit_code();
                let entry = LLIntEntry::from_code_ptr(jit_code.address_for_call(ArityCheckMode::ArityCheckNotRequired))
                    .expect("RELEASE_ASSERT(JIT desligado): o JITCode da função é o ponto de entrada do LLInt");

                proto_call_frame.init(
                    Some(code_block_id),
                    num_parameters,
                    global_object_cell,
                    function.as_cell(),
                    this_slot,
                    context,
                    args_count as i32,
                    encoded_args,
                );
                self.vm_entry_to_javascript(&global_object, proto_call_frame, entry)
            }
            CallData::Native { function: native_function, .. } => {
                proto_call_frame.init(
                    None,
                    0,
                    global_object_cell,
                    function.as_cell(),
                    this_slot,
                    context,
                    args_count as i32,
                    encoded_args,
                );
                self.vm_entry_to_native(&global_object, proto_call_frame, *native_function)
            }
            CallData::None => unreachable!("Expected object to be callable but received CallData::Type::None"),
        }
    }

    /// `vmEntryToNative(function, vm, protoCallFrame)` (`doVMEntry` com `nativeFunc`): monta o frame
    /// do `protoCallFrame` na pilha e chama a função nativa com ele.
    pub fn vm_entry_to_native(
        &mut self,
        global_object: &JSGlobalObject,
        proto_call_frame: ProtoCallFrame,
        function: TaggedNativeFunction,
    ) -> LLIntResult<JSValue> {
        let (call_frame, _entry_mark) = self.push_proto_call_frame(global_object, &proto_call_frame, crate::runtime::vm::VMEntryKind::Native)?;
        let saved_sp = self.sp();
        // `vmEntryRecord.m_prevTopCallFrame`: o `doVMEntry` restaura o `topCallFrame` na saída.
        let saved_top_call_frame = global_object.vm().top_call_frame();
        self.set_sp(call_frame.registers());
        let result = self.invoke_native(global_object, function, call_frame);
        self.set_sp(saved_sp);
        global_object.vm().set_top_call_frame(saved_top_call_frame);
        result
    }

    /// A chamada da função nativa sobre o frame já montado (`call nativeFunc` do `doVMEntry`, seguida do
    /// teste de exceção pendente do `.asm`): o `EncodedJSValue` devolvido é o resultado, e a exceção
    /// pendente no `VM` vira `Thrown` (o `executeCall` do C++ devolve `JSValue()` nesse caso).
    pub(crate) fn invoke_native(
        &mut self,
        global_object: &JSGlobalObject,
        function: TaggedNativeFunction,
        call_frame: CallFrame,
    ) -> LLIntResult<JSValue> {
        let mut native_call_frame = NativeCallFrame::new(&self.stack, call_frame);
        // `nativeCallTrampoline`: `storep cfr, VM::topCallFrame` do frame da função de host.
        global_object.vm().set_top_call_frame(call_frame.registers());
        let encoded = function.call(global_object, &mut native_call_frame);
        if let Some(exception) = global_object.vm().exception() {
            // `VM::throwException` roda com o frame da função de host ainda de pé: a pilha da exceção que nasceu
            // nela (`'a'.repeat(-1)`) começa pelo frame nativo (`at repeat (unknown)`). Uma exceção que veio de
            // JS já foi capturada pelo `unwind` e fica como está.
            self.capture_stack_for_exception(global_object, &exception, call_frame, true);
            return Err(LLIntFailure::Thrown);
        }
        let result = JSValue::decode(encoded);
        debug_assert!(!result.is_empty(), "função nativa devolveu JSValue() sem exceção pendente");
        Ok(result)
    }
}
