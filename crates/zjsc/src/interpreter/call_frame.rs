//! Porte da parte pura de `interpreter/CallFrame.h`: `CallFrameSlot` e os cálculos de offset.
//!
//! Valores resolvidos para Linux x86_64: `CallerFrameAndPC::sizeInRegisters` = 2 (dois registros
//! de 8 bytes: `callerFrame` e `returnPC`), então `codeBlock` = 2, `callee` = 3,
//! `argumentCountIncludingThis` = 4, `thisArgument` = 5, `firstArgument` = 6.
//! O `CallFrame` de verdade (índice de frame sobre a pilha `CLoopStack`, sem ponteiro cru) está
//! mais abaixo, junto de `CallSiteIndex`.

use crate::bytecode::bytecode_index::BytecodeIndex;
use crate::bytecode::virtual_register::VirtualRegister;
use crate::interpreter::callee_bits::CalleeBits;
use crate::interpreter::cloop_stack::CLoopStack;
use crate::interpreter::proto_call_frame::ProtoCallFrame;
use crate::interpreter::register::{CodeBlockId, Register};
use crate::runtime::js_value::JSValue;

/// `CallerFrameAndPC::sizeInRegisters`.
pub const CALLER_FRAME_AND_PC_SIZE_IN_REGISTERS: i32 = 2;

/// `enum class CallFrameSlot`.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CallFrameSlot {
    CodeBlock = CALLER_FRAME_AND_PC_SIZE_IN_REGISTERS,
    Callee = CALLER_FRAME_AND_PC_SIZE_IN_REGISTERS + 1,
    ArgumentCountIncludingThis = CALLER_FRAME_AND_PC_SIZE_IN_REGISTERS + 2,
    ThisArgument = CALLER_FRAME_AND_PC_SIZE_IN_REGISTERS + 3,
    FirstArgument = CALLER_FRAME_AND_PC_SIZE_IN_REGISTERS + 4,
}

/// Os mesmos valores como `i32`, para as contas que o C++ faz com
/// `OVERLOAD_MATH_OPERATORS_FOR_ENUM_CLASS_WITH_INTEGRALS(CallFrameSlot)`.
impl CallFrameSlot {
    pub const CODE_BLOCK: i32 = CallFrameSlot::CodeBlock as i32;
    pub const CALLEE: i32 = CallFrameSlot::Callee as i32;
    pub const ARGUMENT_COUNT_INCLUDING_THIS: i32 = CallFrameSlot::ArgumentCountIncludingThis as i32;
    pub const THIS_ARGUMENT: i32 = CallFrameSlot::ThisArgument as i32;
    pub const FIRST_ARGUMENT: i32 = CallFrameSlot::FirstArgument as i32;
}

/// `CallFrame::headerSizeInRegisters`.
pub const HEADER_SIZE_IN_REGISTERS: i32 = CallFrameSlot::ArgumentCountIncludingThis as i32 + 1;

/// `CallerFrameAndPC::sizeof`, em bytes (dois registros de 8 bytes).
pub const SIZEOF_CALLER_FRAME_AND_PC: usize = 16;

/// `class CallSiteIndex`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CallSiteIndex {
    bits: u32,
}

impl CallSiteIndex {
    const INVALID_INDEX: u32 = u32::MAX;

    /// `CallSiteIndex(uint32_t)`.
    pub fn new(bits: u32) -> CallSiteIndex {
        CallSiteIndex { bits }
    }

    /// `explicit CallSiteIndex(BytecodeIndex)`.
    pub fn from_bytecode_index(bytecode_index: BytecodeIndex) -> CallSiteIndex {
        debug_assert!(bytecode_index.checkpoint() == 0);
        // O C++ guarda `bytecodeIndex.asBits()` (deslocamento empacotado com o checkpoint), e `bytecode_index()`
        // lê de volta com `BytecodeIndex::from_bits`: gravar só o deslocamento dividia o índice por quatro.
        CallSiteIndex { bits: bytecode_index.as_bits() }
    }

    /// `deletedValue()`.
    pub fn deleted_value() -> CallSiteIndex {
        CallSiteIndex::from_bits(Self::INVALID_INDEX - 1)
    }

    /// `isHashTableDeletedValue()`.
    pub fn is_hash_table_deleted_value(&self) -> bool {
        *self == CallSiteIndex::deleted_value()
    }

    /// `explicit operator bool`.
    pub fn is_some(&self) -> bool {
        self.bits != 0
    }

    /// `bits()`.
    pub fn bits(&self) -> u32 {
        self.bits
    }

    /// `fromBits`.
    pub fn from_bits(bits: u32) -> CallSiteIndex {
        CallSiteIndex { bits }
    }

    /// `bytecodeIndex()`.
    pub fn bytecode_index(&self) -> BytecodeIndex {
        BytecodeIndex::from_bits(self.bits())
    }
}

impl Default for CallSiteIndex {
    fn default() -> CallSiteIndex {
        CallSiteIndex { bits: Self::INVALID_INDEX }
    }
}

/// `class DisposableCallSiteIndex : public CallSiteIndex`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct DisposableCallSiteIndex(pub CallSiteIndex);

impl DisposableCallSiteIndex {
    /// `DisposableCallSiteIndex(uint32_t)`.
    pub fn new(bits: u32) -> DisposableCallSiteIndex {
        DisposableCallSiteIndex(CallSiteIndex::new(bits))
    }

    /// `fromCallSiteIndex`.
    pub fn from_call_site_index(call_site_index: CallSiteIndex) -> DisposableCallSiteIndex {
        DisposableCallSiteIndex::new(call_site_index.bits())
    }
}

/// Posição do `callerFrame` dentro do `CallerFrameAndPC` (`callerFrameOffset()`, em registros).
const CALLER_FRAME_SLOT: i32 = 0;
/// Posição do `returnPC` dentro do `CallerFrameAndPC` (`returnPCOffset()`, em registros).
const RETURN_PC_SLOT: i32 = 1;

/// `class CallFrame`. No C++ é o próprio `Register*` do início do frame (`callerFrame`), com os
/// locais em índices negativos e o cabeçalho e os argumentos em índices positivos. Aqui é o índice
/// desse registro na pilha (`CLoopStack`), e todo acesso `this[i]` vira `stack.get(cfr + i)`.
/// Os métodos que leem ou escrevem a pilha recebem o `CLoopStack`; os que dependem de objetos
/// ainda ausentes (`CodeBlock`, `JSCallee`, `JSGlobalObject`) estão listados no fim do arquivo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CallFrame {
    cfr: usize,
}

impl CallFrame {
    pub const HEADER_SIZE_IN_REGISTERS: i32 = HEADER_SIZE_IN_REGISTERS;

    /// `CallFrame::create(Register*)`: o frame que começa no índice `call_frame_base`.
    pub fn create(call_frame_base: usize) -> CallFrame {
        CallFrame { cfr: call_frame_base }
    }

    /// `registers()`: o índice do primeiro registro do frame.
    pub fn registers(&self) -> usize {
        self.cfr
    }

    /// `this[offset]`: índice na pilha do registro `offset` posições do início do frame.
    fn slot(&self, offset: i32) -> usize {
        (self.cfr as isize + offset as isize) as usize
    }

    fn at(&self, stack: &CLoopStack, offset: i32) -> Register {
        stack.get(self.slot(offset))
    }

    fn set_at(&self, stack: &CLoopStack, offset: i32, register: Register) {
        stack.set(self.slot(offset), register);
    }

    /// `uncheckedR(VirtualRegister)`: o registro de um não constante (`this[reg.offset()]`).
    pub fn unchecked_r(&self, stack: &CLoopStack, reg: VirtualRegister) -> Register {
        debug_assert!(!reg.is_constant());
        self.at(stack, reg.offset())
    }

    /// Atribuição a `uncheckedR(reg)`.
    pub fn set_unchecked_r(&self, stack: &CLoopStack, reg: VirtualRegister, value: Register) {
        debug_assert!(!reg.is_constant());
        self.set_at(stack, reg.offset(), value);
    }

    // CallerFrameAndPC

    /// `callerFrameOrEntryFrame()`: o índice do frame chamador (ou do `EntryFrame`).
    pub fn caller_frame_or_entry_frame(&self, stack: &CLoopStack) -> Option<usize> {
        self.at(stack, CALLER_FRAME_SLOT).call_frame()
    }

    /// `callerFrame()`.
    pub fn caller_frame(&self, stack: &CLoopStack) -> Option<CallFrame> {
        self.caller_frame_or_entry_frame(stack).map(CallFrame::create)
    }

    /// `setCallerFrame`.
    pub fn set_caller_frame(&self, stack: &CLoopStack, frame: Option<CallFrame>) {
        self.set_at(stack, CALLER_FRAME_SLOT, Register::from_call_frame(frame.map(|f| f.cfr)));
    }

    /// `rawReturnPC()` e `returnPCForInspection()`. No interpretador em Rust o `returnPC` é o
    /// índice da instrução de retorno no fluxo do chamador (0 é nulo).
    pub fn raw_return_pc(&self, stack: &CLoopStack) -> usize {
        self.at(stack, RETURN_PC_SLOT).pointer()
    }

    /// `hasReturnPC()`.
    pub fn has_return_pc(&self, stack: &CLoopStack) -> bool {
        self.raw_return_pc(stack) != 0
    }

    /// `clearReturnPC()`.
    pub fn clear_return_pc(&self, stack: &CLoopStack) {
        self.set_at(stack, RETURN_PC_SLOT, Register::from_encoded(0));
    }

    /// `setReturnPC`.
    pub fn set_return_pc(&self, stack: &CLoopStack, value: usize) {
        self.set_at(stack, RETURN_PC_SLOT, Register::from_encoded(value as i64));
    }

    /// `callerFrameOffset()`, em registros.
    pub const fn caller_frame_offset() -> i32 {
        CALLER_FRAME_SLOT
    }

    /// `returnPCOffset()`, em registros.
    pub const fn return_pc_offset() -> i32 {
        RETURN_PC_SLOT
    }

    /// `noCaller()`.
    pub const fn no_caller() -> Option<CallFrame> {
        None
    }

    /// `isEmptyTopLevelCallFrameForDebugger`.
    pub fn is_empty_top_level_call_frame_for_debugger(&self, stack: &CLoopStack) -> bool {
        self.caller_frame_or_entry_frame(stack).is_none() && self.raw_return_pc(stack) == 0
    }

    // Call site e bytecode index

    /// `callSiteBitsAreBytecodeOffset()`: sem JIT todo `CodeBlock` é `JITType::InterpreterThunk`
    /// (ou ainda `None`, que o C++ trata como invariante violada), então vale `true`.
    pub fn call_site_bits_are_bytecode_offset(&self) -> bool {
        true
    }

    /// `callSiteBitsAreCodeOriginIndex()`: só o DFG e o FTL usam, e eles não existem.
    pub fn call_site_bits_are_code_origin_index(&self) -> bool {
        false
    }

    /// `callSiteAsRawBits()` e `unsafeCallSiteAsRawBits()`.
    pub fn call_site_as_raw_bits(&self, stack: &CLoopStack) -> u32 {
        self.at(stack, CallFrameSlot::ARGUMENT_COUNT_INCLUDING_THIS).high_word() as u32
    }

    /// `callSiteIndex()` e `unsafeCallSiteIndex()`.
    pub fn call_site_index(&self, stack: &CLoopStack) -> CallSiteIndex {
        CallSiteIndex::from_bits(self.call_site_as_raw_bits(stack))
    }

    /// `setCallSiteIndex`.
    pub fn set_call_site_index(&self, stack: &CLoopStack, call_site_index: CallSiteIndex) {
        self.set_high_word_of_argument_count(stack, call_site_index.bits() as i32);
    }

    /// `setCurrentVPC`: o C++ calcula `codeBlock()->bytecodeIndex(vpc)`; sem o `CodeBlock`, quem
    /// chama passa o `BytecodeIndex` da instrução.
    pub fn set_current_vpc(&self, stack: &CLoopStack, bytecode_index: BytecodeIndex) {
        let call_site = CallSiteIndex::from_bytecode_index(bytecode_index);
        self.set_high_word_of_argument_count(stack, call_site.bits() as i32);
    }

    /// `callSiteBitsAsBytecodeOffset()`.
    pub fn call_site_bits_as_bytecode_offset(&self, stack: &CLoopStack) -> u32 {
        debug_assert!(self.code_block(stack).is_some());
        debug_assert!(self.call_site_bits_are_bytecode_offset());
        self.call_site_index(stack).bytecode_index().offset()
    }

    /// `bytecodeIndex()`: sem Wasm, sem DFG; `BytecodeIndex(0)` quando não há `CodeBlock`.
    pub fn bytecode_index(&self, stack: &CLoopStack) -> BytecodeIndex {
        if self.callee(stack).is_native_callee() {
            return self.call_site_index(stack).bytecode_index();
        }
        if self.code_block(stack).is_none() {
            return BytecodeIndex::from_offset(0);
        }
        self.call_site_index(stack).bytecode_index()
    }

    fn set_high_word_of_argument_count(&self, stack: &CLoopStack, word: i32) {
        let mut register = self.at(stack, CallFrameSlot::ARGUMENT_COUNT_INCLUDING_THIS);
        register.set_high_word(word);
        self.set_at(stack, CallFrameSlot::ARGUMENT_COUNT_INCLUDING_THIS, register);
    }

    // Callee, CodeBlock e escopo

    /// `callee()`.
    pub fn callee(&self, stack: &CLoopStack) -> CalleeBits {
        CalleeBits::new(self.at(stack, CallFrameSlot::CALLEE).unboxed_int64())
    }

    /// `guaranteedJSValueCallee()`.
    pub fn guaranteed_js_value_callee(&self, stack: &CLoopStack) -> JSValue {
        debug_assert!(!self.callee(stack).is_native_callee());
        self.at(stack, CallFrameSlot::CALLEE).js_value()
    }

    /// `jsCallee()`: o identificador da célula do `JSObject`.
    pub fn js_callee(&self, stack: &CLoopStack) -> usize {
        debug_assert!(!self.callee(stack).is_native_callee());
        self.at(stack, CallFrameSlot::CALLEE).object()
    }

    /// `setCallee(JSObject*)`.
    pub fn set_callee(&self, stack: &CLoopStack, callee: usize) {
        self.set_at(stack, CallFrameSlot::CALLEE, Register::from_cell(callee));
    }

    /// `isNativeCalleeFrame()`.
    pub fn is_native_callee_frame(&self, stack: &CLoopStack) -> bool {
        self.callee(stack).is_native_callee()
    }

    /// `codeBlock()` e `unsafeCodeBlock()`.
    pub fn code_block(&self, stack: &CLoopStack) -> Option<CodeBlockId> {
        debug_assert!(!self.callee(stack).is_native_callee());
        self.at(stack, CallFrameSlot::CODE_BLOCK).code_block()
    }

    /// `setCodeBlock`.
    pub fn set_code_block(&self, stack: &CLoopStack, code_block: Option<CodeBlockId>) {
        self.set_at(stack, CallFrameSlot::CODE_BLOCK, Register::from_code_block(code_block));
    }

    /// `scope(int scopeRegisterOffset)`: o identificador da célula do `JSScope`.
    pub fn scope(&self, stack: &CLoopStack, scope_register_offset: i32) -> usize {
        let scope = self.at(stack, scope_register_offset).unboxed_cell();
        debug_assert!(scope != 0);
        scope
    }

    /// `setScope`.
    pub fn set_scope(&self, stack: &CLoopStack, scope_register_offset: i32, scope: usize) {
        self.set_at(stack, scope_register_offset, Register::from_cell(scope));
    }

    /// `topOfFrame()`: `registers()` sem `CodeBlock`, senão `registers() +
    /// codeBlock->stackPointerOffset()`. O `CodeBlock` não existe ainda, então o chamador passa o
    /// `stackPointerOffset()` dele (`virtualRegisterForLocal(frameRegisterCount() - 1).offset()`).
    pub fn top_of_frame(&self, stack: &CLoopStack, code_block_stack_pointer_offset: Option<i32>) -> isize {
        match self.code_block(stack) {
            None => self.registers() as isize,
            Some(_) => {
                // Invariante (ASSERT do C++): quem chama com frame que já tem CodeBlock sempre passa o
                // `stackPointerOffset()`; o valor vem do próprio CodeBlock, não do fonte JS.
                self.registers() as isize
                    + code_block_stack_pointer_offset.expect("stackPointerOffset do CodeBlock") as isize
            }
        }
    }

    // Argumentos

    /// `argumentCountIncludingThis()`.
    pub fn argument_count_including_this(&self, stack: &CLoopStack) -> usize {
        self.at(stack, CallFrameSlot::ARGUMENT_COUNT_INCLUDING_THIS).low_word() as usize
    }

    /// `argumentCount()`.
    pub fn argument_count(&self, stack: &CLoopStack) -> usize {
        self.argument_count_including_this(stack).wrapping_sub(1)
    }

    /// `setArgumentCountIncludingThis`.
    pub fn set_argument_count_including_this(&self, stack: &CLoopStack, count: i32) {
        let mut register = self.at(stack, CallFrameSlot::ARGUMENT_COUNT_INCLUDING_THIS);
        register.set_low_word(count);
        self.set_at(stack, CallFrameSlot::ARGUMENT_COUNT_INCLUDING_THIS, register);
    }

    /// `offsetFor(argumentCountIncludingThis)`.
    pub fn offset_for(argument_count_including_this: usize) -> i32 {
        CallFrameSlot::THIS_ARGUMENT + argument_count_including_this as i32 - 1
    }

    /// `argument(size_t)`: `undefined` fora do intervalo.
    pub fn argument(&self, stack: &CLoopStack, argument: usize) -> JSValue {
        if argument >= self.argument_count(stack) {
            return JSValue::undefined();
        }
        self.get_argument_unsafe(stack, argument)
    }

    /// `uncheckedArgument`.
    pub fn unchecked_argument(&self, stack: &CLoopStack, argument: usize) -> JSValue {
        debug_assert!(argument < self.argument_count(stack));
        self.get_argument_unsafe(stack, argument)
    }

    /// `getArgumentUnsafe`.
    pub fn get_argument_unsafe(&self, stack: &CLoopStack, arg_index: usize) -> JSValue {
        self.at(stack, argument_offset(arg_index as i32)).js_value()
    }

    /// `setArgument`.
    pub fn set_argument(&self, stack: &CLoopStack, argument: usize, value: JSValue) {
        self.set_at(stack, argument_offset(argument as i32), Register::from(value));
    }

    /// `argumentsSpan()` / `addressOfArgumentsStart()`: os argumentos (sem `this`) como valores.
    pub fn arguments_span(&self, stack: &CLoopStack) -> Vec<JSValue> {
        (0..self.argument_count(stack)).map(|i| self.get_argument_unsafe(stack, i)).collect()
    }

    /// `thisValue()`. O `newTarget()` do C++ tem o mesmo corpo (o `new.target` de um frame nativo
    /// de construção ocupa a posição de `this`), então quem quer o `new.target` chama este.
    pub fn this_value(&self, stack: &CLoopStack) -> JSValue {
        self.at(stack, this_argument_offset()).js_value()
    }

    /// `setThisValue`.
    pub fn set_this_value(&self, stack: &CLoopStack, value: JSValue) {
        self.set_at(stack, this_argument_offset(), Register::from(value));
    }

    /// `argIndexForRegister`: depuração e verificação, mas é função do C++.
    pub fn arg_index_for_register(&self, reg_index: usize) -> usize {
        let offset = reg_index as isize - self.registers() as isize;
        (offset - CallFrameSlot::FIRST_ARGUMENT as isize) as usize
    }

    /// Copia o conteúdo de um `ProtoCallFrame` para este frame, como o prólogo do
    /// `vmEntryToJavaScript` faz (cabeçalho de 4 registros: `codeBlock`, `callee`,
    /// `argumentCountIncludingThis` e `this`).
    pub fn copy_proto_header(&self, stack: &CLoopStack, proto: &ProtoCallFrame) {
        self.set_at(stack, CallFrameSlot::CODE_BLOCK, proto.code_block_value);
        self.set_at(stack, CallFrameSlot::CALLEE, proto.callee_value);
        self.set_at(stack, CallFrameSlot::ARGUMENT_COUNT_INCLUDING_THIS, proto.arg_count_and_code_origin_value);
        self.set_at(stack, CallFrameSlot::THIS_ARGUMENT, proto.this_arg);
    }
}

// Ainda sem porte, por dependerem de tipos que não existem: `lexicalGlobalObject`/`deprecatedVM`
// (`JSCallee::realm`, `JSCell::vm`), `r(VirtualRegister)` (constantes do `CodeBlock`),
// `callerFrame(EntryFrame*&)` e `callerSourceOrigin` (`VMEntryRecord`, `SourceOrigin` do
// `CodeBlock`), `codeOrigin`, `currentVPC` (`CodeBlock::instructions()`), `convertToZombieFrame`,
// `isZombieFrame` (`JSGlobalObject::zombieFrameCallee`), `globalObjectOfClosestCodeBlock`,
// `friendlyFunctionName`, `dump`, `describeFrame`, `argumentAfterCapture` (`Arguments`).

/// O `CallFrame*` que uma `NativeFunction` recebe: o frame do chamado (o `CallFrame` índice) junto da
/// pilha de registradores em que ele vive. No C++ o `CallFrame*` é um ponteiro para dentro da
/// `CLoopStack` e `callFrame->thisValue()` lê por ele; em Rust o índice não lê nada sozinho, então a
/// pilha viaja com o frame (sem `unsafe`, sem cópia, e as escritas do hospedeiro, como `setThisValue`,
/// chegam ao frame de verdade). A pilha é uma só por `VM` e tem mutabilidade interior, então o
/// empréstimo é compartilhado: a função nativa que reentra no interpretador (`call` por
/// `VM::interpreter()`) usa a mesma pilha enquanto este frame segue vivo.
pub struct NativeCallFrame<'a> {
    stack: &'a CLoopStack,
    frame: CallFrame,
}

impl<'a> NativeCallFrame<'a> {
    /// O frame `frame` visto pela função nativa, sobre `stack`.
    pub fn new(stack: &'a CLoopStack, frame: CallFrame) -> NativeCallFrame<'a> {
        NativeCallFrame { stack, frame }
    }

    /// O `CallFrame` índice, para as operações do interpretador.
    pub fn call_frame(&self) -> CallFrame {
        self.frame
    }

    /// A pilha do frame.
    pub fn stack(&self) -> &CLoopStack {
        self.stack
    }

    /// `thisValue()`.
    pub fn this_value(&self) -> JSValue {
        self.frame.this_value(self.stack)
    }

    /// `setThisValue`.
    pub fn set_this_value(&mut self, value: JSValue) {
        self.frame.set_this_value(self.stack, value);
    }

    /// `argumentCountIncludingThis()`.
    pub fn argument_count_including_this(&self) -> usize {
        self.frame.argument_count_including_this(self.stack)
    }

    /// `argumentCount()`.
    pub fn argument_count(&self) -> usize {
        self.frame.argument_count(self.stack)
    }

    /// `argument(size_t)`: `undefined` fora do intervalo.
    pub fn argument(&self, argument: usize) -> JSValue {
        self.frame.argument(self.stack, argument)
    }

    /// `uncheckedArgument`.
    pub fn unchecked_argument(&self, argument: usize) -> JSValue {
        self.frame.unchecked_argument(self.stack, argument)
    }

    /// `argumentsSpan()`: os argumentos (sem `this`).
    pub fn arguments_span(&self) -> Vec<JSValue> {
        self.frame.arguments_span(self.stack)
    }

    /// `jsCallee()`: o identificador da célula do `JSObject` chamado.
    pub fn js_callee(&self) -> usize {
        self.frame.js_callee(self.stack)
    }

    /// `callee()`.
    pub fn callee(&self) -> CalleeBits {
        self.frame.callee(self.stack)
    }
}

/// `CallFrame::argumentOffset`.
pub const fn argument_offset(argument: i32) -> i32 {
    CallFrameSlot::FIRST_ARGUMENT + argument
}

/// `CallFrame::argumentOffsetIncludingThis`.
pub const fn argument_offset_including_this(argument: i32) -> i32 {
    CallFrameSlot::THIS_ARGUMENT + argument
}

/// `CallFrame::thisArgumentOffset`.
pub const fn this_argument_offset() -> i32 {
    argument_offset_including_this(0)
}
