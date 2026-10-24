//! Trecho de `interpreter/Interpreter.h`: `DebugHookType` (Interpreter.h:96).

/// `enum DebugHookType` (valores sequenciais a partir de 0).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DebugHookType {
    WillExecuteProgram = 0,
    DidExecuteProgram = 1,
    DidEnterCallFrame = 2,
    DidReachDebuggerStatement = 3,
    WillLeaveCallFrame = 4,
    WillExecuteStatement = 5,
    WillExecuteExpression = 6,
    WillAwait = 7,
    DidAwait = 8,
}

// ---------------------------------------------------------------------------------------------
// `class Interpreter`: a entrada na VM (`executeProgram`, `vmEntryToJavaScript`/`doVMEntry`).
//
// DIVERGÊNCIAS do C++:
//
// - O `Interpreter` do C++ vive dentro do `VM` (`vm.interpreter`) e a `CLoopStack` dentro do
//   `VM`/`StackManager`. O `VM` do porte é compartilhado por referência (`&VM`) e não é dono da
//   pilha (ver `cloop_stack.rs`), então o `Interpreter` é dono da `CLoopStack`, da tabela de
//   `CodeBlock`s por `CodeBlockId` (o `CodeBlock*` que o slot `CodeBlock` do frame guarda) e do
//   "stack pointer" do `.asm` (`sp`, aqui o menor índice em uso).
// - Sem `VMEntryRecord`/`topEntryFrame`/`topCallFrame`/`VMEntryScope`/`didEnterVM`: nenhum deles
//   é observável sem o inspetor, o `ShadowChicken` e o `Debugger`. O frame de entrada fica sem
//   chamador (`callerFrame` nulo).
// - O caminho JSONP do `executeProgram` (`LiteralParser::tryJSONPParse`) está em `execute_jsonp`; `None`
//   é o `goto failedJSONP` e `execute_program` segue dali. A entrada `Call` do `tryJSONPParse` nunca sai do
//   parser (`needsFullSourceInfo` é verdadeiro no `JSGlobalObject`), então o ramo `Call` é inalcançável na prática.
// - Rust não tem a pilha nativa separada do C++ (o CLoop do C++ também não recursa: usa a
//   pilha de `lr`). A chamada JS para JS aqui é recursão nativa; a profundidade lógica é limitada pela
//   `CLoopStack` cheia (RangeError de pilha), e a pilha nativa da thread é só a rede de segurança.
// ---------------------------------------------------------------------------------------------

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::bytecode::code_block::CodeBlockRef;
use crate::interpreter::call_frame::{CallFrame, CallFrameSlot};
use crate::interpreter::cloop_stack::CLoopStack;
use crate::interpreter::proto_call_frame::ProtoCallFrame;
use crate::interpreter::register::{CodeBlockId, Register};
use crate::llint::llint_data::CALL_FRAME_HEADER_SLOTS;
use crate::llint::llint_jit_code::LLIntEntry;
use crate::llint::slow_paths::{put_error_failure, throw_error_object, throw_stack_overflow_error, throw_type_error};
use crate::parser::source_provider::SourceProvider;
use crate::parser::source_tainted_origin::SourceTaintedOrigin;
use crate::runtime::call_data::{get_call_data, CallData};
use crate::runtime::error_messages::READONLY_PROPERTY_WRITE_ERROR;
use crate::runtime::exception_helpers::{create_not_a_function_error, create_not_an_object_error, create_tdz_error, create_undefined_variable_error};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_global_object::BindingCreationContext;
use crate::llint::slow_paths_object::put_to_primitive;
use crate::runtime::put_property_slot::PutPropertySlot;
use crate::runtime::js_scope::put_on_object;
use crate::runtime::js_symbol_table_object::SymbolTablePut;
use crate::runtime::json_object::throw_json_error;
use crate::runtime::literal_parser::{wtf_string_to_units, JsonHost, JsonKey, JsonpData, JsonpPathEntryType, LiteralParser, ParserMode};
use crate::runtime::object_to_primitive::call_function;
use crate::runtime::property_name::PropertyName;
use crate::runtime::put_property_slot::PutContext;
use crate::llint::{LLIntFailure, LLIntResult};
use crate::runtime::error::create_error;
use crate::runtime::stack_alignment::stack_alignment_registers;
use crate::wtf::math_extras::round_up_to_multiple_of;
use crate::wtf::text::wtf_string::String as WtfString;
use crate::runtime::arity_check_mode::ArityCheckMode;
use crate::runtime::code_specialization_kind::CodeSpecializationKind;
use crate::runtime::js_global_object::{JSGlobalObject, JSGlobalObjectRef};
use crate::runtime::js_scope::JSScopeRef;
use crate::runtime::js_value::{EncodedJSValue, JSValue};
use crate::runtime::options::Options;
use crate::runtime::program_executable::ProgramExecutable;
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::runtime::script_executable::ScriptExecutableRef;

/// `class Interpreter`.
///
/// É a alça do interpretador único do `VM` (`vm.interpreter`): clonar copia só as referências para o
/// estado compartilhado (pilha, `sp`, profundidade, tabela de `CodeBlock`s), de modo que o laço que
/// já está rodando e uma chamada reentrante (getter, setter ou callback nativo que chama JS, por
/// `VM::interpreter()`) enxergam a mesma pilha, como no C++. Nada aqui segura empréstimo de `RefCell`
/// por uma chamada que possa reentrar: cada acesso empresta só o tempo de uma leitura ou escrita.
#[derive(Clone)]
pub struct Interpreter {
    /// `vm.interpreter.cloopStack()`.
    pub(crate) stack: Rc<CLoopStack>,
    /// `CodeBlock*` por `CodeBlockId`.
    code_blocks: Rc<RefCell<HashMap<CodeBlockId, CodeBlockRef>>>,
    /// O `sp` do `.asm`: o menor índice da pilha em uso pelo frame mais fundo.
    sp: Rc<Cell<usize>>,
    /// Profundidade da recursão nativa de `llint_execute`.
    native_depth: Rc<Cell<usize>>,
    /// A próxima entrada vem do `callHelper` de um handler `.asm` (`call_from_llint`): o frame do chamado vai na
    /// base guardada aqui (`cfr - stackOffset` de quem chama), sem os frames nativos de `JSC::call` e do trampolim.
    llint_call: Rc<Cell<Option<usize>>>,
}

impl Default for Interpreter {
    fn default() -> Interpreter {
        Interpreter::new()
    }
}

impl Interpreter {
    /// A pilha de registradores (`CLoopStack`, `maxPerThreadStackUsage` = 5 MiB com a zona suave de 128 KiB) é o
    /// limite lógico da recursão JS, como no C++; não há teto de profundidade próprio. A pilha nativa da thread
    /// (`VM::is_safe_to_recurse`) só impede a queda nativa e não decide a profundidade em teste.

    /// `Interpreter::Interpreter()` + `CLoopStack::CLoopStack()` + `VM::updateSoftReservedZoneSize(Options::softReservedZoneSize())`
    /// (VM.cpp:322), que o construtor do VM faz logo depois de criar a pilha.
    pub fn new() -> Interpreter {
        let stack = CLoopStack::new();
        stack.set_soft_reserved_zone_size(Options::soft_reserved_zone_size() as usize);
        let sp = stack.high_address();
        Interpreter {
            stack: Rc::new(stack),
            code_blocks: Rc::new(RefCell::new(HashMap::new())),
            sp: Rc::new(Cell::new(sp)),
            native_depth: Rc::new(Cell::new(0)),
            llint_call: Rc::new(Cell::new(None)),
        }
    }

    /// `callHelper` dos handlers `.asm` de `op_iterator_open`, `op_iterator_next` e `op_async_iterator_next`: a
    /// chamada genérica sai do próprio laço do LLInt, como um `op_call`, com o frame do chamado em `frame_base`
    /// (`cfr - m_stackOffset`, dentro dos temporários que o gerador reservou), sem passar por
    /// `vmEntryToJavaScript` nem `vmEntryToNative`. O porte reentra pelo `execute_call`, mas a entrada não cobra os
    /// frames nativos (`VMEntryKind::LLIntCall`) e usa essa base.
    pub(crate) fn call_from_llint(
        &mut self,
        frame_base: usize,
        function: JSValue,
        call_data: &crate::runtime::call_data::CallData,
        this_value: JSValue,
        args: &[JSValue],
    ) -> LLIntResult<JSValue> {
        self.llint_call.set(Some(frame_base));
        let result = self.execute_call(function, call_data, this_value, None, args);
        // A chamada pode falhar antes de empilhar o frame (estouro de pilha, exceção ao compilar).
        self.llint_call.set(None);
        result
    }

    /// O `sp` do `.asm`.
    pub(crate) fn sp(&self) -> usize {
        self.sp.get()
    }

    /// Escreve o `sp` do `.asm`.
    pub(crate) fn set_sp(&self, sp: usize) {
        self.sp.set(sp);
    }

    /// As raízes do interpretador para o coletor: a pilha de registradores em uso (conservadora) e as constantes
    /// de todos os `CodeBlock`s cadastrados.
    pub(crate) fn visit_gc_roots(&self, marker: &mut crate::runtime::gc::Marker) {
        self.stack.visit_registers(self.sp(), marker);
        let Ok(code_blocks) = self.code_blocks.try_borrow() else {
            marker.abort();
            return;
        };
        for code_block in code_blocks.values() {
            match code_block.try_borrow() {
                Ok(code_block) => marker.values(code_block.constant_registers()),
                Err(_) => marker.abort(),
            }
        }
    }

    /// `CodeBlock::finalizeUnconditionally` de cada bloco, depois da marcação e antes da varredura (ver lá).
    /// Devolve `false` se algum bloco estiver emprestado.
    pub(crate) fn finalize_code_blocks_before_sweep(&self, doomed: &dyn Fn(usize) -> bool) -> bool {
        let Ok(code_blocks) = self.code_blocks.try_borrow() else {
            return false;
        };
        code_blocks
            .values()
            .all(|code_block| code_block.try_borrow().is_ok_and(|code_block| code_block.finalize_unconditionally(doomed)))
    }

    /// A profundidade da recursão nativa de `llint_execute`.
    pub(crate) fn native_depth(&self) -> usize {
        self.native_depth.get()
    }

    /// Soma `delta` à profundidade da recursão nativa.
    pub(crate) fn add_native_depth(&self, delta: isize) {
        self.native_depth.set((self.native_depth.get() as isize + delta) as usize);
    }

    /// Cadastra o `CodeBlock` sob o seu `CodeBlockId`, o que o ponteiro `CodeBlock*` do slot do
    /// frame faz no C++. Devolve o id.
    pub fn register_code_block(&self, code_block: &CodeBlockRef) -> CodeBlockId {
        let id = code_block.borrow().id();
        self.code_blocks.borrow_mut().entry(id).or_insert_with(|| Rc::clone(code_block));
        id
    }

    /// Parte do `~VM` (`Heap::lastChanceToFinalize`): solta a tabela de `CodeBlock`s e, de cada dono, o código
    /// instalado (`ScriptExecutable::clearCode`). Sem GC, `CodeBlock` e executável se seguram por `Rc`
    /// (`m_ownerExecutable` <-> `m_codeBlock...`) e o `CodeBlock` segura o `JSGlobalObject`, que segura o `VM`:
    /// sem quebrar esse ciclo o `VM` nunca chega a zero referências.
    pub(crate) fn finalize_code_blocks(&self) {
        // Tira a tabela com o empréstimo curto: o `Drop` em cascata pode chegar a `code_block`/`register_code_block`.
        let code_blocks = std::mem::take(&mut *self.code_blocks.borrow_mut());
        for code_block in code_blocks.values() {
            let owner = code_block.borrow().owner_executable().clone();
            owner.clear_code();
        }
        drop(code_blocks);
    }

    /// `CodeBlock*` de um `CodeBlockId`.
    pub fn code_block(&self, id: CodeBlockId) -> Option<CodeBlockRef> {
        self.code_blocks.borrow().get(&id).cloned()
    }

    /// `Interpreter::executeProgram`, do `failedJSONP:` em diante, sobre um `ProgramExecutable` já
    /// criado (`ProgramExecutable::create(globalObject, source)`). Devolve o valor de conclusão do
    /// programa, ou `JSValue()` (vazio) com a exceção pendente no `VM`, como o C++.
    ///
    /// Um recurso ainda não portado vira um `Error` lançado com o nome do recurso (ver
    /// `value_or_pending_exception`; `try_execute_program` devolve a lacuna como valor).
    pub fn execute_program(&mut self, program: &Rc<RefCell<ProgramExecutable>>, global_object: &JSGlobalObjectRef) -> JSValue {
        let result = self.try_execute_program(program, global_object);
        Interpreter::value_or_pending_exception(result, global_object)
    }

    /// O desfecho de `executeProgram`/`executeEval` como o C++ o devolve: o valor, ou `JSValue()` (vazio)
    /// com a exceção pendente no `VM`. Um recurso não portado (`LLIntFailure::Unported`) vira um `Error`
    /// lançado com o nome do recurso, em vez de abortar o processo; só o opcode sem handler (que no C++
    /// é `crash()`: `op_unreachable`, `op_yield` e afins) continua panicando. Ver
    /// `wip-notes/interpreter-panics.md`.
    pub(crate) fn value_or_pending_exception(result: LLIntResult<JSValue>, global_object: &JSGlobalObject) -> JSValue {
        match result {
            Ok(value) => value,
            Err(LLIntFailure::Thrown) => JSValue::empty(),
            Err(LLIntFailure::UnportedOpcode(opcode)) => {
                panic!("interpretador: opcode {opcode:?} ainda sem handler (llint-plan, fatias 8 a 10)")
            }
            Err(LLIntFailure::Unported(what)) => {
                if global_object.vm().exception().is_none() {
                    throw_error_object(global_object, create_error(global_object, &WtfString::from_utf8(what.as_bytes())));
                }
                JSValue::empty()
            }
        }
    }

    /// `executeProgram` com as lacunas do porte como `Err`.
    pub fn try_execute_program(
        &mut self,
        program: &Rc<RefCell<ProgramExecutable>>,
        global_object: &JSGlobalObjectRef,
    ) -> LLIntResult<JSValue> {
        let vm = global_object.vm();
        let scope: JSScopeRef = global_object.global_scope();
        let global_callee = global_object.global_callee();

        if !vm.is_safe_to_recurse() {
            return Err(throw_stack_overflow_error(global_object));
        }

        // First check if the "program" is actually just a JSON object (JSONP); `None` é o `failedJSONP:`.
        if let Some(result) = execute_jsonp(program, global_object) {
            return result;
        }

        // Compile source to bytecode if necessary:
        let error = ProgramExecutable::initialize_global_properties(program, vm, global_object, &scope, None);
        if vm.exception().is_some() {
            return Err(LLIntFailure::Thrown);
        }
        if let Some(error) = error {
            // `throwException(globalObject, scope, error)` com o `JSObject*` de uma célula qualquer.
            let mut throw_scope = ThrowScope::new(vm);
            throw_exception(global_object, &mut throw_scope, error);
            return Err(LLIntFailure::Thrown);
        }

        let mut code_block: Option<CodeBlockRef> = None;
        ScriptExecutableRef::Program(Rc::clone(program)).prepare_for_execution(
            vm,
            None,
            &scope,
            CodeSpecializationKind::CodeForCall,
            &mut code_block,
        );
        if vm.exception().is_some() {
            return Err(LLIntFailure::Thrown);
        }
        let code_block = code_block.expect("ASSERT(codeBlock): prepareForExecution sem exceção pendente devolve um CodeBlock");
        // ASSERT(codeBlock && codeBlock->numParameters() == 1); // 1 parameter for 'this'.
        let num_parameters = code_block.borrow().num_parameters();
        debug_assert!(num_parameters == 1);

        let code_block_id = self.register_code_block(&code_block);
        let jit_code = code_block.borrow().jit_code();
        let entry = LLIntEntry::from_code_ptr(jit_code.address_for_call(ArityCheckMode::ArityCheckNotRequired))
            .expect("RELEASE_ASSERT(JIT desligado): o JITCode do programa é o ponto de entrada do LLInt");

        let global_object_cell = JSScopeRef::GlobalObject(Rc::clone(global_object)).cell_id();
        let this_value = match global_object.global_this() {
            Some(global_this) => global_this.as_value(),
            None => JSValue::from_cell(global_object_cell),
        };
        let mut proto_call_frame = ProtoCallFrame::default();
        proto_call_frame.init(
            Some(code_block_id),
            num_parameters,
            global_object_cell,
            global_callee.cell_id(),
            this_value,
            None,
            1,
            Vec::new(),
        );
        self.vm_entry_to_javascript(global_object, proto_call_frame, entry)
    }

    /// `vmEntryToJavaScript(entry, vm, protoCallFrame)` (`doVMEntry` em `LowLevelInterpreter64.asm`):
    /// monta o frame do `protoCallFrame` na pilha, abaixo do que está em uso, e executa `entry`.
    pub fn vm_entry_to_javascript(
        &mut self,
        global_object: &JSGlobalObject,
        proto_call_frame: ProtoCallFrame,
        entry: LLIntEntry,
    ) -> LLIntResult<JSValue> {
        let (call_frame, _entry_mark) = self.push_proto_call_frame(global_object, &proto_call_frame, crate::runtime::vm::VMEntryKind::JavaScript)?;
        let _realm = crate::runtime::current_realm::CurrentRealmScope::enter(global_object);
        let saved_sp = self.sp();
        // `vmEntryRecord.m_prevTopCallFrame`: o `doVMEntry` restaura o `topCallFrame` na saída.
        let saved_top_call_frame = global_object.vm().top_call_frame();
        self.set_sp(call_frame.registers());
        let result = self.llint_execute(call_frame, entry);
        self.set_sp(saved_sp);
        global_object.vm().set_top_call_frame(saved_top_call_frame);
        result
    }

    /// O prólogo de `doVMEntry` que `vmEntryToJavaScript` e `vmEntryToNative` compartilham: aloca o
    /// frame do `protoCallFrame` abaixo do que está em uso e copia o cabeçalho e os argumentos. Não
    /// move o `sp`: quem executa o frame o faz.
    pub(crate) fn push_proto_call_frame(
        &mut self,
        global_object: &JSGlobalObject,
        proto_call_frame: &ProtoCallFrame,
        kind: crate::runtime::vm::VMEntryKind,
    ) -> LLIntResult<(CallFrame, crate::runtime::vm::VMEntryMark)> {
        let llint_base = self.llint_call.take();
        let kind = if llint_base.is_some() { crate::runtime::vm::VMEntryKind::LLIntCall } else { kind };
        let (native_bytes, entry_mark) = global_object.vm().enter_vm(kind);
        let padded_arg_count = proto_call_frame.padded_arg_count as usize;
        let argument_count_including_this = proto_call_frame.argument_count_including_this() as usize;

        let frame_base = match llint_base {
            // `prepareForRegularCall` em `cfr - m_stackOffset`; com menos argumentos que parâmetros, o
            // `slow_path_arityCheck` do chamado desce o frame pelos slots que faltam, alinhados.
            Some(base) => base.checked_sub(round_up_to_multiple_of(
                stack_alignment_registers() as usize,
                padded_arg_count.saturating_sub(argument_count_including_this),
            )),
            // loadi ProtoCallFrame::paddedArgCount; addp CallFrameHeaderSlots; subp sp
            // Mais os frames nativos que o C++ empilha antes do frame de entrada (`VM::enter_vm`).
            None => self.sp().checked_sub(
                padded_arg_count + CALL_FRAME_HEADER_SLOTS + native_bytes.div_ceil(size_of::<crate::runtime::js_value::EncodedJSValue>()),
            ),
        };
        let frame_base = match frame_base {
            Some(frame_base) if self.stack.ensure_capacity_for(frame_base as isize) => frame_base,
            // _llint_stack_check_at_vm_entry, depois _llint_throw_stack_overflow_error_from_vm_entry.
            _ => return Err(throw_stack_overflow_error(global_object)),
        };
        let call_frame = CallFrame::create(frame_base);

        // .copyHeaderLoop: CodeBlock/Callee/ArgumentCountIncludingThis/|this| do protoCallFrame.
        call_frame.copy_proto_header(&self.stack, proto_call_frame);
        // O `doVMEntry` grava no `callerFrame` do frame de entrada o `VMEntryRecord`/`EntryFrame`, e o
        // `StackVisitor` continua dali pelo `m_prevTopCallFrame` do registro, que é o `vm.topCallFrame` da
        // hora da entrada (o frame da função nativa que chamou, ou o frame JS que está num getter ou
        // `eval`). Aqui o `callerFrame` já guarda esse `topCallFrame`: zero (sem VM em execução) é o fim.
        let previous_top_call_frame = Some(global_object.vm().top_call_frame())
            .filter(|&top| top > frame_base)
            .map(CallFrame::create);
        call_frame.set_caller_frame(&self.stack, previous_top_call_frame);
        call_frame.clear_return_pc(&self.stack);

        // .fillExtraArgsLoop e .copyArgsLoop: os argumentos reais e o resto do padding com undefined.
        let undefined: EncodedJSValue = JSValue::undefined().encode();
        for index in 0..padded_arg_count.saturating_sub(1) {
            let encoded = if index < argument_count_including_this.saturating_sub(1) {
                proto_call_frame.args.get(index).copied().unwrap_or(undefined)
            } else {
                undefined
            };
            self.stack.set(frame_base + CallFrameSlot::FIRST_ARGUMENT as usize + index, Register::from_encoded(encoded));
        }

        Ok((call_frame, entry_mark))
    }
}

// ---------------------------------------------------------------------------------------------
// O ramo JSONP de `Interpreter::executeProgram` (Interpreter.cpp:1085-1243).
// ---------------------------------------------------------------------------------------------

/// O `baseObject` do laço do JSONP: o `globalObject`, o ambiente léxico global (quando o nome único do
/// caminho é uma variável `let`/`const`/`class`) ou o valor que o percurso alcançou.
#[derive(Clone, Copy)]
enum JsonpBase {
    Global,
    Lexical,
    Value(JSValue),
}

/// `RETURN_IF_EXCEPTION(throwScope, ...)`.
fn return_if_exception(global_object: &JSGlobalObject) -> LLIntResult<()> {
    if global_object.vm().exception().is_some() { Err(LLIntFailure::Thrown) } else { Ok(()) }
}

/// A exceção pendente devolvida por uma operação do `JsonHost` (que a tira do `VM`), lançada de novo.
fn rethrow(global_object: &JSGlobalObject, thrown: crate::runtime::js_promise_host::Thrown) -> LLIntFailure {
    throw_json_error(global_object, crate::runtime::literal_parser::JsonError::Thrown(thrown));
    LLIntFailure::Thrown
}

/// O escopo (objeto global ou ambiente léxico global) que o `JsonpBase` representa.
fn jsonp_scope(global_object: &JSGlobalObjectRef, base: &JsonpBase) -> Option<JSScopeRef> {
    match base {
        JsonpBase::Global => Some(JSScopeRef::GlobalObject(Rc::clone(global_object))),
        JsonpBase::Lexical => Some(JSScopeRef::GlobalLexicalEnvironment(global_object.global_lexical_environment())),
        JsonpBase::Value(_) => None,
    }
}

/// `baseObject.get(globalObject, ident)`.
fn jsonp_get(global_object: &JSGlobalObjectRef, base: &JsonpBase, ident: &Identifier) -> LLIntResult<JSValue> {
    if let Some(scope) = jsonp_scope(global_object, base) {
        let value = scope.get_property_slot(global_object, ident).unwrap_or_else(JSValue::undefined);
        return_if_exception(global_object)?;
        return Ok(value);
    }
    let JsonpBase::Value(value) = *base else { unreachable!("jsonp_scope cobre Global e Lexical") };
    if value.is_undefined_or_null() {
        return Err(throw_error_object(global_object, create_not_an_object_error(global_object, value)));
    }
    let result = match ObjectRef::from_value(&value) {
        Some(object) => object.get(global_object, &PropertyName::from_identifier(ident)),
        None => {
            let host: &JSGlobalObject = global_object;
            let key = JsonKey::from_wtf_string(ident.string().string());
            JsonHost::get(host, value, &key).map_err(|thrown| rethrow(global_object, thrown))?
        }
    };
    return_if_exception(global_object)?;
    Ok(result)
}

/// O `put` do escopo global como o `slow_path_put_to_scope` o faz em modo sloppy: a tabela de símbolos
/// primeiro (com o `ReadOnly` valendo), depois o `put` do objeto.
fn jsonp_put_to_scope(global_object: &JSGlobalObjectRef, scope: &JSScopeRef, ident: &Identifier, value: JSValue) -> LLIntResult<()> {
    if let Some(key) = ident.impl_() {
        match scope.symbol_table_put(&key, value, false, false) {
            SymbolTablePut::Stored => return Ok(()),
            SymbolTablePut::ReadOnly { should_throw } => {
                return if should_throw { Err(throw_type_error(global_object, READONLY_PROPERTY_WRITE_ERROR)) } else { Ok(()) };
            }
            SymbolTablePut::NotFound => {}
        }
    }
    match scope.put(global_object, ident, value, false, PutContext::UnknownContext, false) {
        Ok(_) => return_if_exception(global_object),
        Err(error) => Err(put_error_failure(global_object, error)),
    }
}

/// `baseObject.put(globalObject, ident, value, slot)` (`Dot`) ou `baseObject.putByIndex(...)` (`Lookup`).
fn jsonp_put(
    global_object: &JSGlobalObjectRef,
    base: &JsonpBase,
    ident: &Identifier,
    index: Option<u32>,
    value: JSValue,
) -> LLIntResult<()> {
    let vm = global_object.vm();
    let result = match (jsonp_scope(global_object, base), index) {
        (Some(scope), None) => return jsonp_put_to_scope(global_object, &scope, ident, value),
        (Some(_), Some(index)) => global_object.put_by_index(vm, index, value, false),
        (None, _) => {
            let JsonpBase::Value(base_value) = *base else { unreachable!("jsonp_scope cobre Global e Lexical") };
            if base_value.is_undefined_or_null() {
                return Err(throw_error_object(global_object, create_not_an_object_error(global_object, base_value)));
            }
            let Some(object) = ObjectRef::from_value(&base_value) else {
                // `JSValue::putToPrimitive` com o `PutPropertySlot` sloppy do JSONP.
                let slot = PutPropertySlot::new(base_value, false, PutContext::UnknownContext, false);
                return put_to_primitive(global_object, base_value, &PropertyName::from_identifier(ident), value, &slot);
            };
            match index {
                None => put_on_object(global_object, &object, base_value, ident, value, false, PutContext::UnknownContext, false),
                Some(index) => object.put_by_index(vm, index, value, false),
            }
        }
    };
    match result {
        Ok(_) => return_if_exception(global_object),
        Err(error) => Err(put_error_failure(global_object, error)),
    }
}

/// O corpo do laço `for (entry...)` do ramo JSONP para uma entrada. `Ok(None)` é o `goto failedJSONP`.
fn execute_jsonp_entry(global_object: &JSGlobalObjectRef, entry: usize, data: JsonpData) -> LLIntResult<Option<JSValue>> {
    let vm = global_object.vm();
    let JsonpData { path, value } = data;
    if path.len() == 1 && path[0].entry_type == JsonpPathEntryType::DeclareVar {
        if !global_object.is_structure_extensible() {
            return Ok(None);
        }
        let ident = &path[0].path_entry_name;
        global_object.create_global_var_binding(BindingCreationContext::Global, ident);
        return_if_exception(global_object)?;
        jsonp_put_to_scope(global_object, &JSScopeRef::GlobalObject(Rc::clone(global_object)), ident, value)?;
        return Ok(Some(JSValue::undefined()));
    }

    let mut base = JsonpBase::Global;
    let lexical = JSScopeRef::GlobalLexicalEnvironment(global_object.global_lexical_environment());
    let global_scope = JSScopeRef::GlobalObject(Rc::clone(global_object));
    for (i, step) in path[..path.len() - 1].iter().enumerate() {
        match step.entry_type {
            JsonpPathEntryType::Dot if i == 0 => {
                debug_assert!(matches!(base, JsonpBase::Global));
                // doGet(globalLexicalEnvironment) e depois doGet(globalObject): o slot vazio (TDZ) conta como ausente.
                let mut found = None;
                for scope in [&lexical, &global_scope] {
                    let result = scope.get_property_slot(global_object, &step.path_entry_name);
                    return_if_exception(global_object)?;
                    if let Some(result) = result.filter(|result| !result.is_empty()) {
                        found = Some(result);
                        break;
                    }
                }
                match found {
                    Some(result) => base = JsonpBase::Value(result),
                    None if entry != 0 => {
                        return Err(throw_error_object(global_object, create_undefined_variable_error(global_object, &step.path_entry_name)));
                    }
                    None => return Ok(None),
                }
            }
            JsonpPathEntryType::Dot => base = JsonpBase::Value(jsonp_get(global_object, &base, &step.path_entry_name)?),
            JsonpPathEntryType::Lookup => {
                let ident = Identifier::from_u32(vm, step.path_index as u32);
                base = JsonpBase::Value(jsonp_get(global_object, &base, &ident)?);
            }
            JsonpPathEntryType::DeclareVar | JsonpPathEntryType::Call => unreachable!("RELEASE_ASSERT_NOT_REACHED"),
        }
    }

    let last = path.last().expect("o caminho do JSONP não é vazio");
    let ident = &last.path_entry_name;
    if path.len() == 1 && last.entry_type != JsonpPathEntryType::Lookup {
        let has_property = lexical.has_property(global_object, ident);
        return_if_exception(global_object)?;
        if has_property {
            if lexical.get_property_slot(global_object, ident).is_some_and(|slot| slot.is_empty()) {
                return Err(throw_error_object(global_object, create_tdz_error(global_object, ident.string())));
            }
            base = JsonpBase::Lexical;
        }
    }

    match last.entry_type {
        JsonpPathEntryType::Call => {
            let function = jsonp_get(global_object, &base, ident)?;
            if matches!(get_call_data(function), CallData::None) {
                return Err(throw_error_object(global_object, create_not_a_function_error(global_object, function, None)));
            }
            let this_value = match base {
                JsonpBase::Value(this_value) if path.len() > 1 => this_value,
                _ => JSValue::undefined(),
            };
            call_function(global_object, function, this_value, &[value]).map(Some).ok_or(LLIntFailure::Thrown)
        }
        JsonpPathEntryType::Dot => jsonp_put(global_object, &base, ident, None, value).map(|()| Some(value)),
        JsonpPathEntryType::Lookup => {
            let ident = Identifier::from_u32(vm, last.path_index as u32);
            jsonp_put(global_object, &base, &ident, Some(last.path_index as u32), value).map(|()| Some(value))
        }
        JsonpPathEntryType::DeclareVar => unreachable!("RELEASE_ASSERT_NOT_REACHED"),
    }
}

/// O ramo JSONP de `executeProgram`: `None` é o `goto failedJSONP` (o programa é JS de verdade e segue
/// para a compilação); `Some` é o desfecho do programa tratado como JSON.
fn execute_jsonp(program: &Rc<RefCell<ProgramExecutable>>, global_object: &JSGlobalObjectRef) -> Option<LLIntResult<JSValue>> {
    let (tainted, program_source) = {
        let program = program.borrow();
        let source = program.source();
        let provider = source.provider().expect("ScriptExecutable sem SourceProvider");
        (provider.source_tainted_origin() != SourceTaintedOrigin::Untainted, source.view())
    };
    // Skip JSONP if the program is tainted. We want there to be a tainted frame on the stack in case the
    // program does an eval via a setter.
    if tainted {
        return None;
    }
    if program_source.is_null() {
        return Some(Ok(JSValue::undefined()));
    }

    let units = wtf_string_to_units(&program_source);
    let host: &JSGlobalObject = global_object;
    let mut parser = LiteralParser::new(host, &units, ParserMode::JSONP);
    let mut jsonp_data = Vec::new();
    // `globalObject->globalObjectMethodTable()->supportsRichSourceInfo(globalObject)`: verdadeiro no `JSGlobalObject`.
    match parser.try_jsonp_parse(&mut jsonp_data, true) {
        Err(error) => {
            throw_json_error(global_object, error);
            return Some(Err(LLIntFailure::Thrown));
        }
        Ok(false) => return None,
        Ok(true) => {}
    }

    let mut result = JSValue::empty();
    for (entry, data) in jsonp_data.into_iter().enumerate() {
        match execute_jsonp_entry(global_object, entry, data) {
            Ok(Some(value)) => result = value,
            Ok(None) => return None,
            Err(failure) => return Some(Err(failure)),
        }
    }
    Some(Ok(result))
}
