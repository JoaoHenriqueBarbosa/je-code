//! O laço de despacho do LLInt: `LowLevelInterpreter.asm` e `LowLevelInterpreter64.asm` traduzidos para um
//! `match` sobre o `OpcodeID` (`CONVENTIONS.md`, item 4), mais o prólogo/arity check de função
//! (`functionPrologue`, `codeBlockPrologue`, `checkStackOverflow`, `.arityFixup`) e o `op_call`
//! (`commonCallOp`, `prepareForRegularCall`, `invokeForRegularCall`).
//!
//! Handlers portados: `op_enter`, `op_mov`, `op_get_scope`, `op_add`, `op_sub`, `op_mul`, `op_jmp`,
//! `op_jtrue`, `op_jfalse`, `op_ret`, `op_loop_hint`, `op_check_traps`, `op_nop`, `op_resolve_scope`,
//! `op_get_from_scope`, `op_put_to_scope`, `op_new_func` e a família `commonCallOp` (`op_call`,
//! `op_call_ignore_result`, `op_tail_call`, `op_construct`, `op_super_construct`). Qualquer outro opcode
//! devolve `LLIntFailure::UnportedOpcode` (ver `mod.rs`): nada é inventado.
//!
//! DIVERGÊNCIAS do `.asm`:
//!
//! - Os registradores `cfr`, `PB` e `PC` do `.asm` são `call_frame` (índice na `CLoopStack`), o
//!   `CodeBlock` do frame e `pc` (o deslocamento em bytes da instrução no fluxo). Constantes são lidas
//!   do `CodeBlock` (`SlowPathFrame::get`), como o `.asm` lê `constantRegisters`.
//! - Os caminhos rápidos que dependem de `Metadata` (cache de `get_from_scope`, `op_call` com
//!   `CallLinkInfo`) vão sempre pelo caminho lento (`slow_paths.rs`), que tem a mesma semântica.
//! - `op_ret` devolve o valor para quem chamou `llint_execute` em vez de restaurar `cfr` e saltar para o
//!   `returnPC`: a chamada JS para JS é recursão de `llint_execute` (`mod.rs`).
//! - O desenrolar de exceção por `op_catch`/`HandlerInfo` e os demais handlers (`op_to_this`, `op_construct`,
//!   `op_throw`, acesso a propriedade, `switch_*`, ...) estão em `dispatch_ext.rs`; sem handler de nenhum dos
//!   dois lados, a exceção pendente no `VM` sobe todos os frames (`LLIntFailure::Thrown`).
//! - `op_call` de função nativa, de `InternalFunction` e de valor que não chama (`handleHostCall`) usa a
//!   `NativeFunction` do `CallData` sobre o frame já montado (`Interpreter::invoke_native`); o `TypeError` leva
//!   o texto da chamada (`ExpressionInfo`, `exception_helpers.rs`). `op_tail_call` de callee de script troca o frame
//!   (`replace_frame_for_tail_call`) e `dispatch_loop` o executa sem recursão nativa; com callee nativo o frame do
//!   chamador fica (o percurso de pilha o pula, `unwind.rs`), e o shadow chicken não existe.
//!   `op_call_varargs`, `op_tail_call_varargs`, `op_construct_varargs`, `op_super_construct_varargs` e
//!   `op_call_direct_eval` (`sizeFrameForVarargs`, `setupVarargsFrame`, `eval`) estão em `varargs.rs`; o frame do
//!   callee que eles montam entra em `call_prepared_frame`, o mesmo caminho do `op_call`.

use std::rc::Rc;

use crate::bytecode::bytecode_ops::{
    OpAdd, OpCall, OpCallDirectEval, OpCallIgnoreResult, OpCallVarargs, OpConstruct, OpConstructVarargs, OpGetFromScope,
    OpGetScope, OpJfalse, OpJmp, OpJtrue, OpMov, OpMul, OpNewFunc, OpPutToScope, OpResolveScope, OpRet, OpSub,
    OpSuperConstruct, OpSuperConstructVarargs, OpTailCall, OpTailCallVarargs,
};
use crate::bytecode::bytecode_index::BytecodeIndex;
use crate::bytecode::code_block::{CallLinkInfoMode, CodeBlockRef};
use crate::bytecode::op_metadata::{DataOnlyCallLinkInfo, OpTailCallMetadata, OpTailCallVarargsMetadata};
use crate::bytecode::opcode::OpcodeID;
use crate::bytecode::virtual_register::{virtual_register_for_local, VirtualRegister};
use crate::llint::dispatch_ext::{check_exception, run_ext};
use crate::runtime::call_data::{get_call_data, get_construct_data, realm_for_call, CallData};
use crate::runtime::construct_ability::ConstructAbility;
use crate::runtime::exception_helpers::{
    create_not_a_constructor_error_at, create_not_a_function_error, CallErrorSite, ErrorSite,
};
use crate::llint::slow_paths::throw_error_object;
use crate::llint::varargs::{DirectEvalInfo, VarargsInfo};
use crate::bytecompiler::label::LabelGenerator;
use crate::interpreter::call_frame::CallFrame;
use crate::interpreter::interpreter::Interpreter;
use crate::interpreter::register::Register;
use crate::llint::llint_data::{entry_checks_arity, entry_is_function, CALL_FRAME_HEADER_SLOTS};
use crate::llint::llint_entrypoint::frame_register_count_for;
use crate::llint::llint_jit_code::LLIntEntry;
use crate::llint::slow_paths::{
    slow_path_add, slow_path_get_from_scope, slow_path_mul, slow_path_new_func, slow_path_put_to_scope,
    slow_path_resolve_scope, slow_path_sub, throw_out_of_memory_error, throw_stack_overflow_error, SlowPathFrame,
};
use crate::llint::{LLIntFailure, LLIntResult};
use crate::runtime::arity_check_mode::ArityCheckMode;
use crate::runtime::code_specialization_kind::CodeSpecializationKind;
use crate::runtime::js_callee::JSCallee;
use crate::runtime::js_global_object::JSGlobalObjectRef;
use crate::runtime::js_value::{js_number_i32, JSValue};
use crate::runtime::script_executable::ScriptExecutableRef;
use crate::runtime::stack_alignment::round_argument_count_to_align_frame;
use crate::interpreter::cloop_stack::CLoopStack;

/// `callee->scope()` de `op_get_scope` e `slow_path_enter`: `uncheckedDowncast<JSCallee>(callFrame->jsCallee())`.
/// O callee de um frame de `CodeBlock` é sempre um `JSCallee` (função de script ou o `globalCallee` de
/// programa/eval/módulo), então o downcast não falha. `m_scope` nulo vira o `JSValue()` vazio, como o C++ grava.
fn callee_scope_value(call_frame: CallFrame, stack: &CLoopStack) -> JSValue {
    let callee = JSCallee::from_cell_id(call_frame.js_callee(stack))
        .expect("invariante do LLInt: o callee de um frame de CodeBlock é um JSCallee");
    callee.scope().map_or_else(JSValue::empty, |scope| scope.into_js_value())
}

/// O `CallLinkInfo` do `op_tail_call`/`op_tail_call_varargs` em `pc`, como o `linkFor` o liga: a primeira chamada
/// fixa o callee (`Monomorphic`), um callee diferente depois o torna `Polymorphic` para sempre. Devolve se o sítio
/// continua ligado a este callee só, a condição para o desembrulho da bound function em cauda. Outros opcodes de
/// cauda (sem `CallLinkInfo` no porte) seguem desembrulhando.
fn link_tail_call_site(code_block: &CodeBlockRef, pc: u32, callee: JSValue) -> bool {
    let block = code_block.borrow();
    let instruction = block.instruction_at(BytecodeIndex::from_offset(pc));
    let metadata_id = instruction.metadata_id();
    let callee_id = match callee {
        JSValue::Cell(_) => u32::try_from(callee.as_cell()).ok(),
        _ => None,
    };
    let link = |info: &mut DataOnlyCallLinkInfo| -> bool {
        match (CallLinkInfoMode::from_u8(info.mode), callee_id) {
            (CallLinkInfoMode::Init, Some(id)) => {
                info.callee = id;
                info.mode = CallLinkInfoMode::Monomorphic as u8;
                true
            }
            (CallLinkInfoMode::Monomorphic, Some(id)) if info.callee == id => true,
            (CallLinkInfoMode::Init | CallLinkInfoMode::Monomorphic, _) => {
                info.callee = 0;
                info.has_seen_closure = true;
                info.mode = CallLinkInfoMode::Polymorphic as u8;
                false
            }
            (CallLinkInfoMode::Polymorphic | CallLinkInfoMode::Virtual, _) => false,
        }
    };
    match instruction.opcode_id_enum() {
        OpcodeID::op_tail_call => block.with_metadata::<OpTailCallMetadata, _>(metadata_id, |metadata| link(&mut metadata.call_link_info)),
        OpcodeID::op_tail_call_varargs => {
            block.with_metadata::<OpTailCallVarargsMetadata, _>(metadata_id, |metadata| link(&mut metadata.call_link_info))
        }
        _ => true,
    }
}

/// `commonCallOp`: os operandos que `op_call`, `op_construct`, `op_super_construct`, `op_call_ignore_result` e
/// `op_tail_call` têm em comum. `dst` é `None` em `op_call_ignore_result`; `tail` é `op_tail_call`
/// (`prepareForTailCall` em vez de `prepareForRegularCall`).
pub(super) struct CallInfo {
    kind: CodeSpecializationKind,
    dst: Option<VirtualRegister>,
    callee: VirtualRegister,
    argc: u32,
    argv: u32,
    tail: bool,
}

impl CallInfo {
    fn new(kind: CodeSpecializationKind, dst: Option<VirtualRegister>, callee: VirtualRegister, argc: u32, argv: u32) -> CallInfo {
        CallInfo { kind, dst, callee, argc, argv, tail: false }
    }

    fn tail(dst: VirtualRegister, callee: VirtualRegister, argc: u32, argv: u32) -> CallInfo {
        CallInfo { tail: true, ..CallInfo::new(CodeSpecializationKind::CodeForCall, Some(dst), callee, argc, argv) }
    }
}

/// O que uma chamada devolve ao laço: o valor do callee, ou, em `op_tail_call`/`op_tail_call_varargs` para um
/// callee de script, o frame do callee já posto no lugar do frame do chamador (o laço o executa sem crescer a
/// pilha nativa). Em cauda para um nativo, o frame do chamador já foi trocado pelo do nativo no C++, então o valor
/// dele volta direto a quem chamou o chamador (`TailReturn`): o bytecode depois do `op_tail_call` nunca roda (o
/// `await` que o `return` de um async generator emite depois da chamada, por exemplo).
pub(super) enum CallOutcome {
    Value(JSValue),
    TailCall { frame: CallFrame, entry: LLIntEntry },
    TailReturn(JSValue),
}

/// Como `dispatch_loop_from` termina sem exceção: `op_ret`, ou a troca de frame de uma chamada em cauda.
pub(super) enum LoopExit {
    Return(JSValue),
    TailCall { frame: CallFrame, entry: LLIntEntry },
}

/// O que o `match` do laço pede depois de executar um opcode.
pub(super) enum Step {
    /// Segue para a instrução seguinte (`dispatch(size)`).
    Next,
    /// Salta `offset` bytes a partir do início da instrução (`jumpTarget`).
    Jump(i32),
    /// `op_ret`: devolve o valor para quem chamou `llint_execute`.
    Return(JSValue),
    /// `commonCallOp`: precisa do `Interpreter` inteiro, então roda fora do empréstimo da pilha.
    Call(CallInfo),
    /// `doCallVarargs`: o número de argumentos só se sabe em tempo de execução.
    CallVarargs(VarargsInfo),
    /// `op_call_direct_eval`.
    CallDirectEval(DirectEvalInfo),
}

/// `BoundLabel::target` só consulta o gerador para rótulos presos a ele (`GeneratorBackward`); um rótulo
/// lido do fluxo é sempre um deslocamento (`Offset`), então a posição nunca é consultada.
pub(super) struct DecodedLabel;

impl LabelGenerator for DecodedLabel {
    fn writer_position(&self) -> i32 {
        0
    }
}

/// As instruções que entram no caminho lento de alocação (`LocalAllocator::allocateSlowCase` chama
/// `Heap::collectIfNecessaryOrDefer`): as que criam célula e as chamadas, por onde passa a alocação dos nativos. No
/// começo delas todo valor vivo do JS está nos registradores que a varredura conservadora percorre. DIVERGÊNCIA: a
/// alocação de dentro de um nativo não coleta ali (os locais Rust dele não são varridos); a coleta que ela pediria
/// acontece no próximo destes pontos.
fn allocates(opcode: OpcodeID) -> bool {
    use OpcodeID::*;
    matches!(
        opcode,
        op_new_object
            | op_new_array
            | op_new_array_buffer
            | op_new_array_with_size
            | op_new_array_with_species
            | op_new_array_with_spread
            | op_new_reg_exp
            | op_new_func
            | op_new_func_exp
            | op_new_generator_func
            | op_new_generator_func_exp
            | op_new_async_func
            | op_new_async_func_exp
            | op_new_async_generator_func
            | op_new_async_generator_func_exp
            | op_new_generator
            | op_new_promise
            | op_create_this
            | op_create_promise
            | op_create_generator
            | op_create_async_generator
            | op_create_lexical_environment
            | op_create_rest
            | op_create_direct_arguments
            | op_create_scoped_arguments
            | op_create_cloned_arguments
            | op_strcat
            | op_call
            | op_call_ignore_result
            | op_call_varargs
            | op_call_direct_eval
            | op_construct
            | op_construct_varargs
            | op_tail_call
            | op_tail_call_varargs
    )
}

impl Interpreter {
    /// `cloopCallJSFunction` do `LLIntEntry`: executa o frame que o chamador montou na pilha (cabeçalho em
    /// `CallFrameSlot`, `CodeBlock` já no slot do frame) até o `op_ret`, e devolve o valor retornado. A
    /// exceção JS pendente vira `Err(LLIntFailure::Thrown)`.
    pub fn llint_execute(&mut self, call_frame: CallFrame, entry: LLIntEntry) -> LLIntResult<JSValue> {
        let saved_sp = self.sp();
        let (call_frame, code_block, global_object) = self.enter_frame(call_frame, entry, true)?;
        self.add_native_depth(1);
        // O código JS que um nativo (`eval`, `map`) inicia tem o próprio `topCallFrame`: um erro sem `site`
        // criado dentro dele não pode citar a chamada do nativo que o iniciou (`(0, eval)(...)`).
        let vm = global_object.vm();
        let saved_native_site = vm.replace_native_call_site(None);
        let result = self.dispatch_loop(call_frame, &code_block, &global_object);
        vm.replace_native_call_site(saved_native_site);
        self.add_native_depth(-1);
        self.set_sp(saved_sp);
        result
    }

    /// O prólogo de função do `.asm` (`functionPrologue`, `functionArityCheck`, `codeBlockPrologue`,
    /// `checkStackOverflow`): acha o `CodeBlock` do frame, faz o arity fixup e põe o `sp` no fim do frame. A
    /// profundidade nativa só se confere na entrada por chamada (`check_depth`): uma chamada em cauda reaproveita
    /// a do frame que ela substitui.
    pub(super) fn enter_frame(
        &mut self,
        call_frame: CallFrame,
        entry: LLIntEntry,
        check_depth: bool,
    ) -> LLIntResult<(CallFrame, CodeBlockRef, JSGlobalObjectRef)> {
        // Os rótulos que não são de CodeBlock (trampolim, ponto de retorno) o C++ só alcança por salto
        // interno do `.asm`, nunca como entrada de `vmEntryToJavaScript`/`prepareForExecution`.
        debug_assert!(entry_is_function(entry).is_some(), "entrada do LLInt que não é de CodeBlock");
        let code_block_id = call_frame
            .code_block(&self.stack)
            .expect("invariante do LLInt: o frame de entrada tem CodeBlock no cabeçalho");
        let code_block = self
            .code_block(code_block_id)
            .expect("invariante do LLInt: todo CodeBlock de frame foi cadastrado em register_code_block");
        let global_object = Rc::clone(code_block.borrow().global_object());

        // A chamada JS para JS é recursão nativa aqui (`llint_execute` + `dispatch_loop`), então a pilha da
        // thread também precisa ser conferida (`checkStackOverflow` do C++ confere a pilha real). O limite lógico da
        // profundidade JS é a `CLoopStack` (`ensure_capacity_for` abaixo, 5 MiB com zona suave), como no C++; o teste
        // nativo só evita a queda da thread e não decide a profundidade.
        if check_depth && !global_object.vm().is_safe_to_recurse() {
            return Err(throw_stack_overflow_error(&global_object));
        }

        // llint_function_for_call_arity_check: `functionArityCheck` antes do corpo.
        let call_frame = if entry_checks_arity(entry) {
            self.arity_fixup(call_frame, &code_block, &global_object)?
        } else {
            call_frame
        };

        // codeBlockPrologue / checkStackOverflow: `sp = cfr - frameSize`, e o frame precisa caber.
        let frame_size = frame_register_count_for(&code_block) as usize;
        let new_sp = match call_frame.registers().checked_sub(frame_size) {
            Some(new_sp) if self.stack.ensure_capacity_for(new_sp as isize) => new_sp,
            _ => return Err(throw_stack_overflow_error(&global_object)),
        };
        self.set_sp(new_sp);
        Ok((call_frame, code_block, global_object))
    }

    /// `.arityFixup` de `functionArityCheck`: com menos argumentos que `numParameters`, o frame desce
    /// `paddedArgCount - argumentCountIncludingThis` registradores, o cabeçalho e os argumentos são
    /// copiados para o novo lugar e o resto vira `undefined`. A contagem de argumentos não muda.
    fn arity_fixup(
        &mut self,
        call_frame: CallFrame,
        code_block: &CodeBlockRef,
        global_object: &JSGlobalObjectRef,
    ) -> LLIntResult<CallFrame> {
        let argument_count_including_this = call_frame.argument_count_including_this(&self.stack);
        let num_parameters = code_block.borrow().num_parameters();
        if argument_count_including_this >= num_parameters as usize {
            return Ok(call_frame);
        }
        let padded = round_argument_count_to_align_frame(num_parameters) as usize;
        let delta = padded - argument_count_including_this;
        let new_base = match call_frame.registers().checked_sub(delta) {
            Some(new_base) if self.stack.ensure_capacity_for(new_base as isize) => new_base,
            _ => return Err(throw_stack_overflow_error(global_object)),
        };

        // O novo frame fica abaixo do antigo, então copiar do menor para o maior índice é seguro.
        let old_base = call_frame.registers();
        for offset in 0..CALL_FRAME_HEADER_SLOTS + argument_count_including_this {
            let register = self.stack.get(old_base + offset);
            self.stack.set(new_base + offset, register);
        }
        let undefined = Register::from(JSValue::undefined());
        for offset in CALL_FRAME_HEADER_SLOTS + argument_count_including_this..CALL_FRAME_HEADER_SLOTS + padded {
            self.stack.set(new_base + offset, undefined);
        }
        Ok(CallFrame::create(new_base))
    }

    /// O laço `dispatch`: lê a instrução em `pc`, executa o handler e avança. `dispatch_loop` (com o
    /// desenrolar de exceção) está em `dispatch_ext.rs`.
    pub(super) fn dispatch_loop_from(
        &mut self,
        call_frame: CallFrame,
        code_block: &CodeBlockRef,
        global_object: &JSGlobalObjectRef,
        start_pc: u32,
    ) -> LLIntResult<LoopExit> {
        let vm = global_object.vm();
        let mut pc: u32 = start_pc;
        loop {
            let instruction = code_block.borrow().instructions().at(pc);
            let opcode = instruction.opcode_id_enum();
            crate::runtime::cpu_clock::count_instruction();
            let size = instruction.size() as u32;
            // `storePC`: o `currentVPC` do frame, que o desenrolar de exceção consulta.
            call_frame.set_current_vpc(&mut self.stack, BytecodeIndex::from_offset(pc));
            // `storep cfr, VM::topCallFrame` do `prepareStateForCCall`: o `.asm` o grava antes de cada
            // chamada a C++; aqui uma vez por instrução, que dá o mesmo frame a quem a instrução chamar.
            vm.set_top_call_frame(call_frame.registers());
            if allocates(opcode) {
                crate::runtime::gc::safe_point(self, &vm);
            }

            let step = {
                let block = code_block.borrow();
                let mut f = SlowPathFrame { vm, call_frame, stack: &self.stack, code_block: &block };
                match opcode {
                    // op_enter: os locais começam `undefined` (o TDZ é explícito, com `op_mov` de `empty`).
                    OpcodeID::op_enter => {
                        for local in 0..block.num_callee_locals() {
                            f.set(virtual_register_for_local(local as i32), JSValue::undefined());
                        }
                        // O `op_enter` do `.asm` (e do JIT, `emitGetScope(scopeRegister())`) também grava no
                        // `scopeRegister` do CodeBlock o escopo do callee do frame: o gerador de bytecode
                        // (`allocateScope`) só reserva o registrador e confia nisso. Sem esta gravação o
                        // registrador fica `undefined` e `resolve_scope`/`new_func` não acham o escopo.
                        let scope_register = block.scope_register();
                        if scope_register.is_valid() {
                            f.set(scope_register, callee_scope_value(call_frame, &*f.stack));
                        }
                        Step::Next
                    }
                    OpcodeID::op_mov => {
                        let op: OpMov = instruction.as_op();
                        let value = f.get(op.src);
                        f.set(op.dst, value);
                        Step::Next
                    }
                    // op_get_scope: `callee.scope`.
                    OpcodeID::op_get_scope => {
                        let op: OpGetScope = instruction.as_op();
                        f.set(op.dst, callee_scope_value(call_frame, &*f.stack));
                        Step::Next
                    }
                    // binaryOp(add): int32 sem overflow; o resto é `slow_path_add` (doubles, strings, objetos).
                    OpcodeID::op_add => {
                        let op: OpAdd = instruction.as_op();
                        let (lhs, rhs) = (f.get(op.lhs), f.get(op.rhs));
                        if lhs.is_int32() && rhs.is_int32() {
                            let (a, b) = (lhs.as_int32(), rhs.as_int32());
                            let sum = a.checked_add(b).map_or_else(|| JSValue::from_double(a as f64 + b as f64), js_number_i32);
                            f.set(op.dst, sum);
                        } else {
                            slow_path_add(&mut f, &op).map_err(|_| throw_out_of_memory_error(global_object))?;
                            check_exception(&f)?;
                        }
                        Step::Next
                    }
                    OpcodeID::op_sub => {
                        let op: OpSub = instruction.as_op();
                        slow_path_sub(&mut f, &op);
                        check_exception(&f)?;
                        Step::Next
                    }
                    OpcodeID::op_mul => {
                        let op: OpMul = instruction.as_op();
                        slow_path_mul(&mut f, &op);
                        check_exception(&f)?;
                        Step::Next
                    }
                    OpcodeID::op_jmp => {
                        let op: OpJmp = instruction.as_op();
                        Step::Jump(op.target_label.target(&DecodedLabel))
                    }
                    OpcodeID::op_jtrue => {
                        let op: OpJtrue = instruction.as_op();
                        if f.get(op.condition).to_boolean() {
                            Step::Jump(op.target_label.target(&DecodedLabel))
                        } else {
                            Step::Next
                        }
                    }
                    OpcodeID::op_jfalse => {
                        let op: OpJfalse = instruction.as_op();
                        if f.get(op.condition).to_boolean() {
                            Step::Next
                        } else {
                            Step::Jump(op.target_label.target(&DecodedLabel))
                        }
                    }
                    OpcodeID::op_ret => {
                        let op: OpRet = instruction.as_op();
                        Step::Return(f.get(op.value))
                    }
                    // Sem traps (watchdog, `VMTraps`) e sem tiering: `loop_hint` e `check_traps` não fazem nada.
                    // `op_loop_hint` é onde o laço do C++ passa pelo `checkTraps` e o heap pode coletar.
                    OpcodeID::op_loop_hint => {
                        crate::runtime::gc::safe_point(self, &vm);
                        Step::Next
                    }
                    OpcodeID::op_check_traps | OpcodeID::op_nop => Step::Next,
                    OpcodeID::op_resolve_scope => {
                        let op: OpResolveScope = instruction.as_op();
                        slow_path_resolve_scope(&mut f, &op)?;
                        check_exception(&f)?;
                        Step::Next
                    }
                    OpcodeID::op_get_from_scope => {
                        let op: OpGetFromScope = instruction.as_op();
                        // O `topCallFrame` do C++ aponta o `get_from_scope`: um getter nativo (`__proto__`
                        // solto) que lança consulta esse frame para apender `(evaluating '...')`.
                        let saved_native_site =
                            vm.replace_native_call_site(Some((code_block.clone(), BytecodeIndex::from_offset(pc))));
                        let outcome = slow_path_get_from_scope(&mut f, &op);
                        vm.replace_native_call_site(saved_native_site);
                        outcome?;
                        check_exception(&f)?;
                        Step::Next
                    }
                    OpcodeID::op_put_to_scope => {
                        let op: OpPutToScope = instruction.as_op();
                        slow_path_put_to_scope(&mut f, &op)?;
                        check_exception(&f)?;
                        Step::Next
                    }
                    OpcodeID::op_new_func => {
                        let op: OpNewFunc = instruction.as_op();
                        slow_path_new_func(&mut f, &op)?;
                        check_exception(&f)?;
                        Step::Next
                    }
                    OpcodeID::op_call => {
                        let op: OpCall = instruction.as_op();
                        Step::Call(CallInfo::new(CodeSpecializationKind::CodeForCall, Some(op.dst), op.callee, op.argc, op.argv))
                    }
                    OpcodeID::op_call_ignore_result => {
                        let op: OpCallIgnoreResult = instruction.as_op();
                        Step::Call(CallInfo::new(CodeSpecializationKind::CodeForCall, None, op.callee, op.argc, op.argv))
                    }
                    // `op_tail_call` (`prepareForTailCall`): o frame do callee substitui o do chamador.
                    OpcodeID::op_tail_call => {
                        let op: OpTailCall = instruction.as_op();
                        Step::Call(CallInfo::tail(op.dst, op.callee, op.argc, op.argv))
                    }
                    OpcodeID::op_construct => {
                        let op: OpConstruct = instruction.as_op();
                        Step::Call(CallInfo::new(CodeSpecializationKind::CodeForConstruct, Some(op.dst), op.callee, op.argc, op.argv))
                    }
                    OpcodeID::op_super_construct => {
                        let op: OpSuperConstruct = instruction.as_op();
                        Step::Call(CallInfo::new(CodeSpecializationKind::CodeForConstruct, Some(op.dst), op.callee, op.argc, op.argv))
                    }
                    OpcodeID::op_call_varargs => Step::CallVarargs(instruction.as_op::<OpCallVarargs>().into()),
                    OpcodeID::op_tail_call_varargs => Step::CallVarargs(instruction.as_op::<OpTailCallVarargs>().into()),
                    OpcodeID::op_construct_varargs => Step::CallVarargs(instruction.as_op::<OpConstructVarargs>().into()),
                    OpcodeID::op_super_construct_varargs => {
                        Step::CallVarargs(instruction.as_op::<OpSuperConstructVarargs>().into())
                    }
                    OpcodeID::op_call_direct_eval => Step::CallDirectEval(instruction.as_op::<OpCallDirectEval>().into()),
                    other => match run_ext(&mut f, &instruction)? {
                        // `LLINT_CHECK_EXCEPTION`/`bpneq ... storeVM; checkpoint` do `.asm`: um slow path que chamou JS
                        // (getter, setter, `valueOf`, trap de `Proxy`) pode devolver `Ok` com a exceção pendente no `VM`.
                        Some(step) => {
                            if matches!(step, Step::Next) {
                                check_exception(&f)?;
                            }
                            step
                        }
                        None => return Err(LLIntFailure::UnportedOpcode(other)),
                    },
                }
            };

            let (dst, value) = match step {
                Step::Next => {
                    pc += size;
                    continue;
                }
                Step::Jump(offset) => {
                    // `jumpTarget` do `.asm`: deslocamento 0 no operando é o salto fora da faixa do operando
                    // (`addOutOfLineJumpTarget` do `Label::setLocation`); o alvo verdadeiro vem da tabela.
                    // Sem isto o salto para frente além de 127 palavras re-executava o mesmo pc para sempre.
                    let offset = if offset == 0 {
                        code_block.borrow().unlinked_code_block().borrow().out_of_line_jump_offset(pc)
                    } else {
                        offset
                    };
                    pc = (pc as i64 + offset as i64) as u32;
                    continue;
                }
                Step::Return(value) => return Ok(LoopExit::Return(value)),
                Step::Call(info) => (info.dst, self.call_function(call_frame, code_block, global_object, info, pc, pc + size)?),
                Step::CallVarargs(info) => {
                    (Some(info.dst), self.call_varargs(call_frame, code_block, global_object, info, pc, pc + size)?)
                }
                Step::CallDirectEval(info) => {
                    (Some(info.dst), self.call_direct_eval(call_frame, code_block, global_object, info, pc, pc + size)?)
                }
            };
            let value = match value {
                CallOutcome::Value(value) => value,
                CallOutcome::TailCall { frame, entry } => return Ok(LoopExit::TailCall { frame, entry }),
                CallOutcome::TailReturn(value) => return Ok(LoopExit::Return(value)),
            };
            if let Some(dst) = dst {
                call_frame.set_unchecked_r(&mut self.stack, dst, Register::from(value));
            }
            pc += size;
        }
    }

    /// `commonCallOp` com `slow_path_call`/`setUpCall`: `prepareForRegularCall` monta o cabeçalho do frame do
    /// callee em `cfr - argv` (os argumentos já estão lá, o gerador os pôs). `JSFunction` de script tem o
    /// `CodeBlock` garantido (`prepareForExecution`) e `invokeForRegularCall` executa o frame; função nativa,
    /// `InternalFunction` e valor que não chama vão para `handleHostCall` (`call_native`).
    ///
    /// `#[inline(never)]`: sem isto o otimizador funde os locais da chamada no frame de `dispatch_loop_from`,
    /// que fica na pilha nativa a cada nível de JS.
    #[inline(never)]
    fn call_function(
        &mut self,
        call_frame: CallFrame,
        code_block: &CodeBlockRef,
        global_object: &JSGlobalObjectRef,
        info: CallInfo,
        pc: u32,
        return_pc: u32,
    ) -> LLIntResult<CallOutcome> {
        let CallInfo { kind, callee, argc, argv, tail, .. } = info;
        let vm = global_object.vm();
        let callee_value = {
            let block = code_block.borrow();
            SlowPathFrame { vm, call_frame, stack: &self.stack, code_block: &block }.get(callee)
        };

        // prepareForRegularCall: `calleeFrame = cfr - registerOffset`, com os slots do cabeçalho.
        let callee_base = call_frame
            .registers()
            .checked_sub(argv as usize)
            .ok_or_else(|| throw_stack_overflow_error(global_object))?;
        let callee_frame = CallFrame::create(callee_base);
        callee_frame.set_argument_count_including_this(&mut self.stack, argc as i32);
        self.call_prepared_frame(call_frame, code_block, global_object, callee_frame, callee_value, kind, tail, pc, return_pc)
    }

    /// O que `slow_path_call`, `varargsSetup` e `commonCallDirectEval` fazem depois de montar os argumentos do
    /// callee: o `callerFrame` e o `currentVPC` do chamador, e então `setUpCall`/`linkFor` (`prepareForRegularCall`
    /// e `invokeForRegularCall` para `JSFunction` de script, `handleHostCall` para o resto). Com `tail` e um
    /// callee de script, `prepareForTailCall` troca o frame do chamador pelo do callee e a execução fica com o
    /// laço de quem chamou (`CallOutcome::TailCall`); callee nativo é chamado como em `op_call`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn call_prepared_frame(
        &mut self,
        call_frame: CallFrame,
        code_block: &CodeBlockRef,
        global_object: &JSGlobalObjectRef,
        callee_frame: CallFrame,
        callee_value: JSValue,
        kind: CodeSpecializationKind,
        tail: bool,
        pc: u32,
        return_pc: u32,
    ) -> LLIntResult<CallOutcome> {
        let vm = global_object.vm();
        // O DFG (`BoundFunctionTailCall`) desembrulha o `bind` em posição de cauda; o porte faz o mesmo antes de
        // montar o frame, para a recursão por bound function não aninhar `executeCall` (medido no bun: 1e6 níveis).
        // Só no sítio monomórfico (o `CallLinkInfo` ligado a uma bound function só): medido no bun 1.4.2, o sítio que
        // vê uma bound function nova a cada chamada (`return f.bind(null)(n - 1)`) cresce a pilha e dá `RangeError`.
        let (callee_frame, callee_value) =
            if tail && kind == CodeSpecializationKind::CodeForCall && link_tail_call_site(code_block, pc, callee_value) {
                self.unwrap_bound_function_for_tail_call(callee_frame, callee_value, global_object)?
            } else {
                (callee_frame, callee_value)
            };
        call_frame.set_current_vpc(&mut self.stack, BytecodeIndex::from_offset(pc));
        callee_frame.set_caller_frame(&mut self.stack, Some(call_frame));
        let site = ErrorSite { code_block, bytecode_index: BytecodeIndex::from_offset(pc), vm, call_frame };

        let function = match callee_value.as_js_function() {
            Some(function) if !function.is_host_function() => function,
            // handleHostCall (`getJSFunction` nulo, `InternalFunction`, `NativeExecutable`): o `CallData` do
            // valor decide entre a função nativa e o `TypeError`.
            _ => {
                let value = self.handle_host_call(callee_frame, callee_value, kind, global_object, &site, tail)?;
                return Ok(if tail { CallOutcome::TailReturn(value) } else { CallOutcome::Value(value) });
            }
        };
        // setUpCall: `createNotAConstructorError` quando `kind` é construct e o executável não constrói.
        if kind == CodeSpecializationKind::CodeForConstruct
            && function.js_executable().borrow().construct_ability() == ConstructAbility::CannotConstruct
        {
            return Err(throw_error_object(global_object, create_not_a_constructor_error_at(global_object, callee_value, Some(&site))));
        }

        // slow_path_call -> linkFor -> prepareForExecution.
        let scope = function
            .scope_unchecked()
            .expect("invariante do LLInt: JSFunction de script (não host) tem escopo");
        let mut callee_code_block: Option<CodeBlockRef> = None;
        ScriptExecutableRef::Function(function.js_executable()).prepare_for_execution(
            vm,
            Some(&function),
            &scope,
            kind,
            &mut callee_code_block,
        );
        if vm.exception().is_some() {
            return Err(LLIntFailure::Thrown);
        }
        // `prepareForExecution` sem exceção sempre devolve o CodeBlock (`RELEASE_ASSERT` no `linkFor`).
        let callee_code_block =
            callee_code_block.expect("invariante do LLInt: prepareForExecution sem exceção devolve CodeBlock");
        let callee_code_block_id = self.register_code_block(&callee_code_block);
        let jit_code = callee_code_block.borrow().jit_code();
        let entry = LLIntEntry::from_code_ptr(jit_code.address_for_call(ArityCheckMode::MustCheckArity))
            .expect("invariante do LLInt: o JITCode de uma função de script é um ponto de entrada do LLInt");

        callee_frame.set_return_pc(&mut self.stack, return_pc as usize);
        callee_frame.set_code_block(&mut self.stack, Some(callee_code_block_id));
        callee_frame.set_callee(&mut self.stack, function.cell_id());

        if !tail {
            return self.llint_execute(callee_frame, entry).map(CallOutcome::Value);
        }
        let frame = self.replace_frame_for_tail_call(call_frame, code_block, global_object, callee_frame)?;
        Ok(CallOutcome::TailCall { frame, entry })
    }

    /// Troca um callee `JSBoundFunction` pelo alvo, com o `this` ligado e os argumentos ligados na frente dos da
    /// chamada. O frame novo começa `bound_args.len()` slots abaixo do original (pilha cresce para baixo), então
    /// nada acima dele é pisado; repete para bound de bound. Alvo que não é chamável fica como está e segue
    /// pelo caminho nativo, que produz o `TypeError` de sempre.
    fn unwrap_bound_function_for_tail_call(
        &mut self,
        mut callee_frame: CallFrame,
        mut callee_value: JSValue,
        global_object: &JSGlobalObjectRef,
    ) -> LLIntResult<(CallFrame, JSValue)> {
        loop {
            let Some(function) = callee_value.as_js_function() else { break };
            let Some(bound) = function.as_bound_function() else { break };
            let target = bound.target_function();
            if get_call_data(target).is_none() {
                break;
            }
            let bound_args = bound.bound_args().to_vec();
            let mut arguments = callee_frame.arguments_span(&self.stack);
            let mut new_arguments = bound_args;
            new_arguments.append(&mut arguments);
            let Some(new_base) = callee_frame.registers().checked_sub(new_arguments.len().saturating_sub(callee_frame.argument_count(&self.stack))) else {
                return Err(throw_stack_overflow_error(global_object));
            };
            let new_frame = CallFrame::create(new_base);
            new_frame.set_argument_count_including_this(&mut self.stack, (new_arguments.len() + 1) as i32);
            new_frame.set_this_value(&self.stack, bound.bound_this());
            for (index, value) in new_arguments.iter().enumerate() {
                new_frame.set_argument(&self.stack, index, *value);
            }
            callee_frame = new_frame;
            callee_value = target;
        }
        Ok((callee_frame, callee_value))
    }

    /// `prepareForTailCall`: o cabeçalho e os argumentos do callee (já montados em `callee_frame`) vão para o
    /// lugar do frame do chamador, que mantém o `callerFrame` e o `returnPC`. O fim da área de argumentos do
    /// chamador é o limite: o frame novo termina onde o antigo terminava (`cfr + header + max(argc,
    /// paddedNumParameters)`, o que `arity_fixup` deixou), então um callee com mais argumentos começa abaixo de
    /// `cfr` e um com menos sobe, sem tocar nos frames mais antigos. A cópia segue o sentido que não pisa na
    /// origem quando as duas faixas se sobrepõem.
    fn replace_frame_for_tail_call(
        &mut self,
        call_frame: CallFrame,
        code_block: &CodeBlockRef,
        global_object: &JSGlobalObjectRef,
        callee_frame: CallFrame,
    ) -> LLIntResult<CallFrame> {
        let argument_count = call_frame.argument_count_including_this(&self.stack);
        let num_parameters = code_block.borrow().num_parameters() as usize;
        let argument_area = if argument_count < num_parameters {
            round_argument_count_to_align_frame(num_parameters as u32) as usize
        } else {
            argument_count
        };
        let new_argument_count = callee_frame.argument_count_including_this(&self.stack);
        let Some(new_base) = (call_frame.registers() + argument_area).checked_sub(new_argument_count) else {
            return Err(throw_stack_overflow_error(global_object));
        };

        let caller = call_frame.caller_frame_or_entry_frame(&self.stack);
        let return_pc = call_frame.raw_return_pc(&self.stack);
        let source = callee_frame.registers();
        let slots = CALL_FRAME_HEADER_SLOTS + new_argument_count;
        if new_base > source {
            for offset in (0..slots).rev() {
                let register = self.stack.get(source + offset);
                self.stack.set(new_base + offset, register);
            }
        } else if new_base < source {
            for offset in 0..slots {
                let register = self.stack.get(source + offset);
                self.stack.set(new_base + offset, register);
            }
        }
        let frame = CallFrame::create(new_base);
        frame.set_caller_frame(&self.stack, caller.map(CallFrame::create));
        frame.set_return_pc(&self.stack, return_pc);
        Ok(frame)
    }

    /// O site da chamada que o JSC enxerga numa tail call para nativa: o `CodeBlock` e o `BytecodeIndex` do frame
    /// de baixo (o chamador do frame que faz a tail call). `None` quando esse frame é de entrada (sem `CodeBlock`
    /// de script, `vmEntryToJavaScript`) ou nativo, caso em que vale o site do próprio frame.
    fn tail_caller_site(&self, callee_frame: CallFrame) -> Option<(CodeBlockRef, BytecodeIndex)> {
        let tail_frame = callee_frame.caller_frame(&self.stack)?;
        let below = tail_frame.caller_frame(&self.stack)?;
        if below.is_native_callee_frame(&self.stack) {
            return None;
        }
        let code_block = self.code_block(below.code_block(&self.stack)?)?;
        Some((code_block, below.bytecode_index(&self.stack)))
    }

    /// `handleHostCall(calleeFrame, callee, kind)`: sem `CodeBlock` nem `returnPC` no frame, a função nativa
    /// do `CallData` do valor (`getCallDataInline`/`getConstructDataInline`, que cobrem `NativeExecutable` e
    /// `InternalFunction`) roda sobre o frame já montado; o desfecho é o de `vmEntryToNative` (exceção pendente
    /// vira `Thrown`, senão o valor devolvido vai para o `dst`). Sem `CallData`, `createNotAFunctionError`
    /// ou `createNotAConstructorError` com o texto da chamada.
    ///
    /// `#[inline(never)]`: ramo frio da recursão JS para JS; fora dele, os locais da chamada nativa não
    /// entram no frame de `call_prepared_frame`.
    #[inline(never)]
    fn handle_host_call(
        &mut self,
        callee_frame: CallFrame,
        callee_value: JSValue,
        kind: CodeSpecializationKind,
        global_object: &JSGlobalObjectRef,
        site: &ErrorSite<'_>,
        tail: bool,
    ) -> LLIntResult<JSValue> {
        callee_frame.set_code_block(&mut self.stack, None);
        callee_frame.clear_return_pc(&self.stack);

        let call_data = match kind {
            CodeSpecializationKind::CodeForCall => get_call_data(callee_value),
            CodeSpecializationKind::CodeForConstruct => get_construct_data(callee_value),
        };
        let CallData::Native { function, .. } = &call_data else {
            let error = match kind {
                CodeSpecializationKind::CodeForCall => {
                    create_not_a_function_error(global_object, callee_value, Some(&CallErrorSite(site)))
                }
                CodeSpecializationKind::CodeForConstruct => {
                    create_not_a_constructor_error_at(global_object, callee_value, Some(site))
                }
            };
            return Err(throw_error_object(global_object, error));
        };

        // `constructSymbol` lança `createNotAConstructorError`; o `Interpreter::unwind` do C++ apenda o
        // `(evaluating '...')` do `sourceAppender` do erro. Aqui o erro nasce no site, sem o desvio pelo unwind.
        // O erro nasce dentro do `constructSymbol`: a pilha dele começa pelo frame nativo (`at Symbol (unknown)`),
        // então o callee entra no frame antes de a pilha ser capturada.
        if kind == CodeSpecializationKind::CodeForConstruct
            && *function == crate::runtime::native_function::to_tagged(crate::runtime::symbol_constructor::construct_symbol)
        {
            callee_frame.set_callee(&self.stack, callee_value.as_cell());
            let failure = throw_error_object(global_object, create_not_a_constructor_error_at(global_object, callee_value, Some(site)));
            if let Some(exception) = global_object.vm().exception() {
                self.capture_stack_for_exception(global_object, &exception, callee_frame, true);
            }
            return Err(failure);
        }

        // `calleeFrame->setCallee(asObject(callee))` e `function(asObject(callee)->realm(), calleeFrame)`.
        callee_frame.set_callee(&self.stack, callee_value.as_cell());
        let realm = realm_for_call(callee_value, &call_data);
        let saved_sp = self.sp();
        let vm = global_object.vm();
        let saved_top_call_frame = vm.top_call_frame();
        self.set_sp(callee_frame.registers());
        // O `topCallFrame` do C++ aponta a instrução de chamada: o nativo que cria um erro (`Object.keys(null)`)
        // o consulta para apender `(evaluating '...')`.
        // Em `op_tail_call` o JSC já reaproveitou o frame do chamador para o callee: o `topCallFrame` do nativo é
        // o chamador do frame que fez a tail call, e o `(evaluating '...')` cita a chamada dele (`f()` em `T`).
        let native_site = if tail { self.tail_caller_site(callee_frame) } else { None }
            .unwrap_or_else(|| site.visible_location());
        let saved_native_site = vm.replace_native_call_site(Some(native_site));
        // Em tail call para nativo o JSC já trocou o frame do chamador (o builtin `apply@` de `Reflect.apply`): o
        // visitante de pilha do nativo pula esse frame (ver `js_dom_exception::add_native_error_info`).
        let saved_native_tail = vm.replace_native_call_tail(tail);
        let result = self.invoke_native(&realm, *function, callee_frame);
        vm.replace_native_call_tail(saved_native_tail);
        vm.replace_native_call_site(saved_native_site);
        self.set_sp(saved_sp);
        // O `topCallFrame` volta ao frame que chamou: o do frame nativo já saiu da pilha, e uma entrada
        // nova na mesma instrução (um `eval` direto, um getter) não pode enxergá-lo como chamador.
        vm.set_top_call_frame(saved_top_call_frame);
        result
    }
}
