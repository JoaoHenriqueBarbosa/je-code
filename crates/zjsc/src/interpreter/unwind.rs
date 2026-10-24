//! Porte do desenrolar de exceção do `Interpreter` (`Interpreter::unwind`, `Interpreter::handleCatch`
//! em `Interpreter.cpp`, `LLIntExceptions.cpp`) e da captura de pilha (`Interpreter::getStackTrace`,
//! `StackVisitor::visit`, `StackVisitor::Frame::computeLineAndColumn`).
//!
//! DIVERGÊNCIAS:
//!
//! - O C++ desempilha frame a frame dentro de `Interpreter::unwind`, chamando `handleCatch` quando acha
//!   um `HandlerInfo` e, sem handler, `vm.callFrameForCatch`/`topEntryFrame` para sair do `vmEntry`. Aqui
//!   cada frame JS é uma chamada recursiva de `Interpreter::llint_execute`: o `Err(Thrown)` devolvido pelo
//!   laço do callee já é o "desempilhar um frame", e quem chamou consulta o seu próprio `HandlerInfo`
//!   com o `currentVPC` que o `storePC` gravou antes da chamada. `unwind` olha um frame por vez.
//! - `handleCatch` restaura `callFrame`, `sp` e salta para `handler->target` (o `op_catch`). O salto é
//!   o `pc` que `run_instructions` devolve ao laço; o `restoreStackPointerAfterCall` é de quem chama.
//! - `vm.callFrameForCatch`, `vm.targetMachinePCForThrow` e `vm.targetInterpreterPCForThrow` não existem
//!   (não há `returnToThrow` nem `_llint_handle_uncaught_exception`): a exceção sem handler sobe como
//!   `Err(Thrown)` até `executeProgram`, que a entrega como `JSValue()` com `vm.exception()` pendente.
//! - `getStackTrace` roda no primeiro frame que vê a exceção (o mais interno), não dentro de
//!   `VM::throwException`, porque o `VM` não enxerga a `CLoopStack`. Os frames dos chamadores seguem
//!   intactos na pilha: o callee só desceu o `sp`, nenhum registrador acima foi reescrito.
//! - Os frames de função nativa (sem `CodeBlock`) entram como `nome (unknown)`, que é como o `Bun` os
//!   mostra no lugar do `[native code]` do JSC puro; o frame nativo de partida (o do construtor `Error`)
//!   fica de fora, e o frame que está numa chamada em posição de cauda também (o C++ o reaproveita).
//! - `Error.stackTraceLimit` é o espelho do global (`JSGlobalObject::stack_trace_limit`, mantido por
//!   `ErrorConstructor::put`); a
//!   pilha da `Exception` usa `Options::exceptionStackTraceLimit()`, como em `Exception::create`.

use crate::bytecode::code_block::CodeBlockRef;
use crate::bytecode::handler_info::RequiredHandler;
use crate::bytecode::opcode::OpcodeID;
use crate::interpreter::call_frame::CallFrame;
use crate::interpreter::interpreter::Interpreter;
use crate::bytecode::bytecode_index::BytecodeIndex;
use crate::interpreter::stack_visitor::{Frame, FrameCodeType, IterationStatus, StackVisitor};
use crate::parser::parser_modes::is_generator_or_async_function_body_parse_mode;
use crate::runtime::executable::ExecutableBaseRef;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use std::cell::RefCell;
use crate::runtime::error_instance::ErrorInstance;
use crate::runtime::error_natives::capture_frames;
use crate::runtime::exception::Exception;
use crate::runtime::js_global_object::{JSGlobalObject, JSGlobalObjectRef};
use crate::runtime::js_async_function_generator::{JSAsyncFunctionGenerator, JSAsyncFunctionGeneratorRef};
use crate::runtime::js_promise::JSPromise;
use crate::runtime::js_promise_combinators_context::JSPromiseCombinatorsGlobalContext;
use crate::runtime::js_value::JSValue;
use crate::runtime::options_list::Options;
use crate::runtime::script_executable::ScriptExecutableRef;
use crate::runtime::stack_frame::{Location, StackFrame};
use crate::runtime::vm::VM;
use crate::wtf::text::conversion_mode::ConversionMode;
use crate::wtf::text::wtf_string::String as WtfString;
use std::rc::Rc;

impl Interpreter {
    /// `Interpreter::unwind` para o frame `call_frame` (o que lançou, ou o que recebeu a exceção do callee): registra
    /// a pilha na primeira vez que a `Exception` é vista, e devolve o `handler->target` do `HandlerInfo` que
    /// cobre o `currentVPC` do frame, que é o `op_catch` para onde `handleCatch` salta. `None` quando não
    /// há exceção pendente, quando ela é a de terminação (não capturável, `isTerminationException`) ou
    /// quando nenhum handler do frame cobre o ponto.
    pub(crate) fn unwind(
        &self,
        call_frame: CallFrame,
        code_block: &CodeBlockRef,
        global_object: &JSGlobalObjectRef,
    ) -> Option<u32> {
        let vm = global_object.vm();
        let exception = vm.exception()?;
        self.capture_stack_for_exception(global_object, &exception, call_frame, false);
        if vm.is_termination_exception(&exception) {
            return None;
        }
        let index = call_frame.bytecode_index(&self.stack);
        let code_block = code_block.borrow();
        code_block.handler_for_bytecode_index(index, RequiredHandler::AnyHandler).map(|handler| handler.base.target)
    }

    /// `Exception::create(..., StackCaptureAction::CaptureStack)`: grava o `m_stack` uma vez e, se o valor
    /// lançado é um `ErrorInstance` sem frames guardados (um erro que não nasceu do construtor), guarda
    /// estes como `m_stackTrace`; a formatação (e o `Error.prepareStackTrace`) fica para a primeira leitura
    /// de `stack`.
    pub(crate) fn capture_stack_for_exception(
        &self,
        global_object: &JSGlobalObject,
        exception: &Rc<Exception>,
        top_call_frame: CallFrame,
        include_top_native: bool,
    ) {
        if exception.stack_captured() {
            return;
        }
        // `Exception::create` guarda o `m_stack` (até `exceptionStackTraceLimit` frames) para todo valor. Os
        // `StackFrame` resolvem nome, URL e linha só na primeira leitura, então guardar a pilha custa só o percurso;
        // o bun a lê no relato de exceção não capturada que sai de um callback (`fromErrorInstance` com
        // `&jscException->stack()`), onde os frames e a posição do `throw` substituem os do erro.
        // O `ErrorInstance` sem pilha ainda guarda a dele (limite do realm, `globalObject->stackTraceLimit()`,
        // o espelho de `Error.stackTraceLimit`, que `Error.cpp getStackTrace` lê); um que já tem pilha (o
        // `new Error` relançado) não anda a pilha de novo, porque `set_pending_stack` não faria nada.
        let thrown = exception.value();
        // `is_cell` vale também para o valor vazio (como no C++); só uma célula de verdade pode ser um `ErrorInstance`.
        let error = if matches!(thrown, JSValue::Cell(_)) { ErrorInstance::from_cell_id(thrown.as_cell()) } else { None };
        if let Some(error) = error {
            if !error.has_stack_info() {
                if let Some(frames) = capture_frames(global_object, top_call_frame, None, include_top_native) {
                    error.set_pending_stack(frames);
                }
            }
        }
        let vm = global_object.vm();
        let limit = Options::exception_stack_trace_limit() as usize;
        exception.set_stack(vm.interpreter().get_stack_trace(vm, top_call_frame, limit, None, false));
    }

    /// `Interpreter::getStackTrace(vm, results, framesToSkip, maxStackSize, caller, ...)`: percorre os
    /// frames de `top_call_frame` para fora com o `StackVisitor::visit`, do mais interno ao mais externo,
    /// até `limit` frames. Com `caller`, os frames até o dele (inclusive) ficam de fora, e se ele não está
    /// na pilha o resultado é vazio (`foundCaller`). `framesToSkip` é o frame nativo de partida: o do
    /// construtor `Error` ou de `Error.captureStackTrace`, que chama este percurso, não entra. Os frames de
    /// função nativa mais fundos (`sort`, `parse`, `construct`...) entram como `nome (unknown)`, e os de
    /// implementação privada (builtins) ficam de fora, como em `isImplementationVisibilityPrivate`.
    ///
    /// Um frame cuja instrução corrente é uma chamada em posição de cauda (`op_tail_call`) e que não é o
    /// de partida não existe no C++: o `op_tail_call` do LLInt reaproveita o frame para o callee, então o
    /// percurso o pula (é o que some com `call`, `apply` e a função de `return f()` em modo estrito).
    ///
    /// `include_top_native` é o caso da exceção lançada por uma função de host (`'a'.repeat(-1)`): o
    /// `VM::throwException` do C++ roda com o frame dela ainda de pé, e o `Bun` o mostra (`at repeat (unknown)`).
    /// Os construtores de erro (`Error`, `AggregateError`...) e `captureStackTrace` passam `false`: o frame
    /// nativo deles é o de partida.
    pub fn get_stack_trace(
        &self,
        vm: &VM,
        top_call_frame: CallFrame,
        limit: usize,
        caller: Option<usize>,
        include_top_native: bool,
    ) -> Vec<StackFrame> {
        let mut frames = Vec::new();
        let mut found_caller = caller.is_none();
        // O código de `eval` só entra pela função nativa `eval`, que o `Bun` mostra como frame logo depois
        // dele. Quando o `eval` foi chamado como função (frame nativo de verdade) é esse frame que aparece.
        let mut pending_native_eval = false;
        // O frame anterior do percurso entrou como frame nativo.
        let mut native_pushed_last = false;
        StackVisitor::visit(self, Some(top_call_frame), false, |frame| {
            let previous_native = std::mem::take(&mut native_pushed_last);
            if std::mem::take(&mut pending_native_eval) && !frame.is_native_frame() && frames.len() < limit {
                frames.push(StackFrame::native_eval());
            }
            if frames.len() >= limit {
                return IterationStatus::Done;
            }
            if !found_caller {
                // Medido no `bun` 1.4.2: o frame nativo de partida (`Error.captureStackTrace`, o construtor) nunca
                // casa com `caller` (`captureStackTrace(o, Error.captureStackTrace)` dá `Error` sem frames).
                if frame.index() == 0 && frame.is_native_frame() && !include_top_native {
                    return IterationStatus::Continue;
                }
                // O frame que fez tail call já foi trocado pelo do callee no JSC: ele não está na pilha e não casa
                // (medido no bun 1.4.2: `var f = () => Error.captureStackTrace(o, f)` deixa `Error` sem frames).
                found_caller = !(frame.index() > 0 && Self::is_in_tail_call(frame))
                    && frame.callee().is_cell()
                    && (Some(frame.callee().as_cell()) == caller || caller.is_some_and(|wrapper| is_body_of_wrapper(frame.callee().as_cell(), wrapper)));
                // A função nativa `eval` não tem frame próprio no percurso: ela vem logo depois do frame do código
                // de `eval` (ver `pending_native_eval`), então `caller == eval` casa ali e o frame some com ela.
                if !found_caller && frame.code_type() == FrameCodeType::Eval {
                    found_caller = frame.code_block().is_some_and(|block| block.borrow().global_object().eval_function() == caller);
                }
                return IterationStatus::Continue;
            }
            let wasm_frames = frame.call_frame().map_or(0, |call_frame| crate::wasm::wasm_call_stack::frames_anchored_at(call_frame.registers()));
            if wasm_frames > 0 && frame.is_native_frame() {
                // O frame nativo da função exportada não aparece (o bun mostra só as funções Wasm): no lugar dele
                // entra um `at unknown` por quadro Wasm, o mais interno primeiro.
                for _ in 0..wasm_frames {
                    if frames.len() < limit {
                        frames.push(StackFrame::native_function(String::new(), 0, JSValue::undefined()));
                    }
                }
            } else if frame.is_native_frame() {
                if (frame.index() > 0 || include_top_native) && !frame.is_implementation_visibility_private() {
                    let before = frames.len();
                    frames.extend(self.native_stack_frame_for(vm, frame));
                    native_pushed_last = frames.len() > before;
                    return IterationStatus::Continue;
                }
            } else if frame.code_block().is_some()
                && !frame.is_implementation_visibility_private()
                && !(frame.index() > 0 && Self::is_in_tail_call(frame))
            {
                // O V8 escreve `new Uint8Array (<anonymous>)`: o construtor nativo se conhece pela instrução de quem
                // o chamou.
                if previous_native && crate::runtime::stack_frame::v8_stack_format() && Self::is_constructing(frame) {
                    if let Some(native) = frames.last_mut() {
                        native.is_constructor = true;
                    }
                }
                // O `eval` direto não tem frame no V8, nem quando o fonte dele não compila.
                if previous_native && crate::runtime::stack_frame::v8_stack_format() && Self::current_opcode_is(frame, &[OpcodeID::op_call_direct_eval]) {
                    frames.pop();
                }
                frames.push(self.stack_frame_for(frame));
                // O V8 não mostra frame para o `eval` direto; o indireto é um frame nativo de verdade, que o
                // percurso já encontra.
                pending_native_eval = frame.code_type() == FrameCodeType::Eval && !crate::runtime::stack_frame::v8_stack_format();
            }
            IterationStatus::Continue
        });
        if pending_native_eval && frames.len() < limit {
            frames.push(StackFrame::native_eval());
        }
        if let Some(origin) = current_async_origin() {
            // Os frames `async` entram depois dos síncronos da entrada do microtask (`asyncStackTraceInsertPos`).
            let async_frames = Self::get_async_stack_trace(&origin, limit.saturating_sub(frames.len()));
            frames.extend(async_frames);
        }
        frames
    }

    /// `Interpreter::getAsyncStackTrace`: do gerador da async function em execução sobe pela cadeia de quem a
    /// espera (`getParentGenerator`) e devolve um frame `async nome` por gerador pai, no ponto do `await`
    /// (`computeBytecodeIndex` pela última tabela de salto de `switch`). Geradores sem `Next` de função JS pública
    /// (por exemplo o de `for await` de módulo) não geram frame.
    fn get_async_stack_trace(origin: &JSAsyncFunctionGeneratorRef, max_size: usize) -> Vec<StackFrame> {
        let mut results = Vec::new();
        let mut current = parent_generator(origin);
        while let Some(generator) = current {
            if results.len() >= max_size {
                break;
            }
            results.extend(Self::async_stack_frame_for(&generator));
            current = parent_generator(&generator);
        }
        results
    }

    /// O frame que `getAsyncStackTrace` acrescenta para `generator`, ou `None` se o `Next` não é função JS pública.
    fn async_stack_frame_for(generator: &JSAsyncFunctionGeneratorRef) -> Option<StackFrame> {
        let function = generator.next().as_js_function()?;
        if function.is_host_function() {
            return None;
        }
        let executable = function.js_executable();
        if matches!(ExecutableBaseRef::Script(ScriptExecutableRef::Function(executable.clone())).implementation_visibility(), ImplementationVisibility::Private | ImplementationVisibility::PrivateRecursive) {
            return None;
        }
        let name = format!("async {}", String::from_utf8_lossy(&executable.borrow().ecma_name().utf8()));
        let owner = ScriptExecutableRef::Function(executable.clone());
        let Some(code_block) = executable.borrow().code_block_for_call() else {
            // Sem `CodeBlock` só o nome do arquivo aparece, sem linha nem coluna.
            let mut frame = StackFrame::resolved(Location {
                function_name: name,
                source_url: source_url_stripped_of(&owner),
                line: 0,
                column: 0,
                has_line_and_column_info: false,
                construct_back_offset: 0,
            });
            frame.callee = function.cell_id();
            frame.is_strict = owner.is_in_strict_context();
            frame.script_id = i32::try_from(owner.source().provider_id()).unwrap_or(0);
            frame.is_async = true;
            return Some(frame);
        };
        let bytecode_index = {
            let block = code_block.borrow();
            let tables = block.unlinked_code_block().borrow().number_of_unlinked_switch_jump_tables();
            let state = generator.state();
            let offset = if state > 0 && tables > 0 {
                let table_index = tables - 1;
                let relative = block.unlinked_code_block().borrow().unlinked_switch_jump_table(table_index).offset_for_value(state);
                // O valor da tabela é um salto relativo ao começo do `op_switch_imm` do gerador (o despachante soma o pc), não
                // um índice absoluto: usado cru, cai no meio de uma instrução e o opcode lido é lixo.
                let switch_at = block
                    .instructions()
                    .iter()
                    .find(|instruction| {
                        instruction.opcode_id_enum() == crate::bytecode::opcode::OpcodeID::op_switch_imm
                            && instruction.as_op::<crate::bytecode::bytecode_ops::OpSwitchImm>().table_index as usize == table_index
                    })
                    .map(|instruction| instruction.offset());
                switch_at.map_or(0, |at| i64::from(at) + i64::from(relative))
            } else {
                0
            };
            BytecodeIndex::from_offset(u32::try_from(offset).unwrap_or(0))
        };
        let (line_column, _) = crate::runtime::stack_frame::displayed_position(&code_block, &code_block.borrow(), bytecode_index);
        let mut frame = StackFrame::resolved(Location {
            function_name: name,
            source_url: source_url_stripped_of(&owner),
            line: line_column.line,
            column: line_column.column,
            has_line_and_column_info: true,
            construct_back_offset: 0,
        });
        frame.callee = function.cell_id();
        frame.is_strict = owner.is_in_strict_context();
        frame.script_id = i32::try_from(owner.source().provider_id()).unwrap_or(0);
        frame.is_async = true;
        Some(frame)
    }

    /// A instrução corrente do frame é `op_tail_call` ou `op_tail_call_varargs`.
    fn is_in_tail_call(frame: &Frame) -> bool {
        Self::current_opcode_is(frame, &[OpcodeID::op_tail_call, OpcodeID::op_tail_call_varargs])
    }

    /// A instrução corrente do frame constrói (`new`, `super(...)`).
    fn is_constructing(frame: &Frame) -> bool {
        Self::current_opcode_is(
            frame,
            &[OpcodeID::op_construct, OpcodeID::op_construct_varargs, OpcodeID::op_super_construct, OpcodeID::op_super_construct_varargs],
        )
    }

    fn current_opcode_is(frame: &Frame, opcodes: &[OpcodeID]) -> bool {
        let Some(code_block) = frame.code_block() else {
            return false;
        };
        let block = code_block.borrow();
        let instruction = block.instructions().at(frame.bytecode_index().offset());
        opcodes.contains(&instruction.opcode_id_enum())
    }

    /// `StackFrame` de um frame de função nativa (`JSObject` chamado sem `CodeBlock`): o nome é o da função de
    /// host. O `VM` vem de quem captura. Sem `callee` que seja objeto o frame não tem como ser nomeado e some.
    fn native_stack_frame_for(&self, vm: &VM, frame: &Frame) -> Option<StackFrame> {
        let call_frame = frame.call_frame()?;
        let callee = frame.callee();
        if !callee.is_cell() || callee.as_cell() == 0 {
            return None;
        }
        Some(StackFrame::native_function(frame.function_name(vm), callee.as_cell(), call_frame.this_value(&self.stack)))
    }

    /// `StackFrame` de um frame JS, preguiçoso como o do C++: guarda o `CodeBlock`, o `BytecodeIndex` e o
    /// `callee`; nome, URL, linha e coluna saem em `StackFrame::location` quando alguém os lê.
    fn stack_frame_for(&self, frame: &Frame) -> StackFrame {
        let call_frame = frame.call_frame().expect("frame do percurso sem CallFrame");
        let code_block = frame.code_block().expect("frame JS sem CodeBlock");
        let (code_type, is_strict, is_constructor, script_id) = {
            let block = code_block.borrow();
            let owner = block.owner_executable();
            (block.code_type(), owner.is_in_strict_context(), block.is_constructor(), i32::try_from(owner.source().provider_id()).unwrap_or(0))
        };
        let mut stack_frame = StackFrame::lazy(Rc::clone(code_block), frame.bytecode_index(), call_frame.js_callee(&self.stack));
        // O `this` de um construtor derivado ainda não inicializado é o vazio: `CallSite.getThis` vê `undefined`.
        stack_frame.this_value = Some(call_frame.this_value(&self.stack)).filter(|value| !value.is_empty()).unwrap_or_else(JSValue::undefined);
        stack_frame.code_type = code_type;
        stack_frame.is_strict = is_strict;
        stack_frame.is_constructor = is_constructor;
        stack_frame.script_id = script_id;
        stack_frame
    }
}

/// `sourceURLStripped` do `SourceProvider` do executável (vazio sem provider).
/// A URL do frame: a do comentário `//# sourceURL=` do próprio código, quando houver (o `eval` e o `Function` que
/// se nomeiam, medido no bun 1.4.2), senão a do provider sem a query.
pub(crate) fn source_url_stripped_of(owner: &ScriptExecutableRef) -> String {
    let utf8 = |text: WtfString| String::from_utf8_lossy(&text.utf8(ConversionMode::LenientConversion)).into_owned();
    owner
        .source()
        .provider()
        .map(|provider| {
            let directive = provider.source_url_directive();
            utf8(if directive.is_null() { provider.source_url_stripped() } else { directive })
        })
        .unwrap_or_default()
}

thread_local! {
    /// Os geradores das async functions em retomada, do mais externo ao mais interno. Faz o papel do
    /// `VMEntryRecord::m_context` que `getStackTrace` lê (`updateAsyncStackTraceOriginGenerator`): o porte não tem
    /// `EntryFrame`, então quem retoma o gerador (`AsyncFunctionResume`) o registra aqui pela duração do corpo.
    static ASYNC_ORIGINS: RefCell<Vec<JSAsyncFunctionGeneratorRef>> = const { RefCell::new(Vec::new()) };
}

/// Fim do programa (`cell_registry::reset_program_state`): os geradores são células do programa.
pub(crate) fn reset_for_program() {
    let taken = ASYNC_ORIGINS.try_with(|origins| std::mem::take(&mut *origins.borrow_mut()));
    drop(taken);
}

/// O corpo (`@generatorNext`) de uma função geradora ou async é uma função à parte do wrapper que o programa vê,
/// criada pelo código do wrapper. Medido no `bun` 1.4.2: `Error.captureStackTrace(o, f)` dentro do corpo de `f`
/// descarta o frame do corpo como se fosse o do próprio `f`. `callee` é o corpo de `wrapper` quando o executável
/// dele está entre as funções filhas do código do wrapper.
fn is_body_of_wrapper(callee: usize, wrapper: usize) -> bool {
    let (Some(body), Some(wrapper)) = (JSValue::from_cell(callee).as_js_function(), JSValue::from_cell(wrapper).as_js_function()) else {
        return false;
    };
    if body.is_host_function() || wrapper.is_host_function() {
        return false;
    }
    let body_executable = body.js_executable();
    if !is_generator_or_async_function_body_parse_mode(body_executable.borrow().parse_mode()) {
        return false;
    }
    let Some(code_block) = wrapper.js_executable().borrow().code_block_for_call() else {
        return false;
    };
    let target = body_executable.borrow().unlinked_executable().clone();
    let block = code_block.borrow();
    let unlinked = block.unlinked_code_block().borrow();
    unlinked.function_exprs().iter().chain(unlinked.function_decls()).any(|child| Rc::ptr_eq(child, &target))
}

/// O gerador da retomada mais interna em curso, se há.
fn current_async_origin() -> Option<JSAsyncFunctionGeneratorRef> {
    ASYNC_ORIGINS.with(|origins| origins.borrow().last().cloned())
}

/// Registra `generator` como origem assíncrona até o fim do escopo devolvido.
pub(crate) fn enter_async_origin(generator: &JSAsyncFunctionGeneratorRef) -> AsyncOriginScope {
    ASYNC_ORIGINS.with(|origins| origins.borrow_mut().push(Rc::clone(generator)));
    AsyncOriginScope
}

/// Guarda de [`enter_async_origin`].
pub(crate) struct AsyncOriginScope;

impl Drop for AsyncOriginScope {
    fn drop(&mut self) {
        ASYNC_ORIGINS.with(|origins| {
            origins.borrow_mut().pop();
        });
    }
}

/// `getContextValueFromPromise`: o `asyncStackTraceContext` da promessa se `value` é uma promessa pendente.
fn promise_async_context(value: JSValue) -> JSValue {
    JSPromise::from_value(&value).map_or_else(JSValue::empty, |promise| promise.async_stack_trace_context())
}

/// `getParentGenerator`: o gerador que espera o resultado de `generator`, por `await` simples, por
/// `Promise.all`/`allSettled`/`any` (contexto global dos combinadores) ou por `Promise.race`.
fn parent_generator(generator: &JSAsyncFunctionGeneratorRef) -> Option<JSAsyncFunctionGeneratorRef> {
    let context = promise_async_context(generator.context());
    if context.is_empty() {
        return None;
    }
    if let Some(parent) = JSAsyncFunctionGenerator::from_value(&context) {
        return Some(parent);
    }
    if let Some(global_context) = JSPromiseCombinatorsGlobalContext::from_value(&context) {
        return JSAsyncFunctionGenerator::from_value(&promise_async_context(global_context.promise()));
    }
    JSAsyncFunctionGenerator::from_value(&promise_async_context(context))
}
