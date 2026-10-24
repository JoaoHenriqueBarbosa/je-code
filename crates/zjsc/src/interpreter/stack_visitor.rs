//! Porte de `interpreter/StackVisitor.h` e `StackVisitor.cpp`: o percurso dos frames da pilha, do mais
//! interno ao mais externo, que `Interpreter::getStackTrace`, `Function.prototype.arguments` e
//! `Function.prototype.caller` usam.
//!
//! DIVERGÊNCIAS:
//!
//! - O C++ começa em `vm.topCallFrame` e avança até `startFrame`. Aqui `visit` recebe o frame de partida
//!   direto (quem chama passa `vm.top_call_frame()` ou o frame vivo que está consultando): o `topCallFrame`
//!   do porte só é gravado nos pontos em que o `.asm` o grava, e um frame desempilhado deixaria um valor
//!   velho. Pelo mesmo motivo `skipFirstFrame` pula o frame de partida e não o `topCallFrame`.
//! - O functor recebe o [`Frame`] (o `visitor->` do C++), não o `StackVisitor`, e `visit` guarda o
//!   visitante inteiro por dentro.
//! - Sem `EntryFrame` (o `callerFrame` do frame de entrada já guarda o `m_prevTopCallFrame` do
//!   `VMEntryRecord`, o `topCallFrame` da hora da entrada, e `callerFrame` nulo termina o percurso), sem `m_returnPC`/`m_previousReturnPC`, sem frames do DFG com
//!   `InlineCallFrame` e sem frames de Wasm (`NativeCallee`): nenhum existe sem JIT nem Wasm. O frame de
//!   `callee` nativo, que o C++ aceita só como `NativeCallee`, é lido como frame sem `CodeBlock`.
//! - `functionName` de um frame de função usa `CodeBlock::inferredName` (o `ecmaName` do executável) no
//!   lugar de `getCalculatedDisplayName(callee)`, como `StackFrame` (ver `runtime/stack_frame.rs`).
//! - Sem `preRedirectURL`, `sourceID`, `calleeSaveRegistersForUnwinding`, `dump` e `CallerFunctor`: nada no
//!   porte os consulta ainda.

use crate::bytecode::bytecode_index::BytecodeIndex;
use crate::bytecode::code_block::CodeBlockRef;
use crate::bytecode::code_type::CodeType as BytecodeCodeType;
use crate::bytecode::line_column::LineColumn;
use crate::interpreter::call_frame::CallFrame;
use crate::interpreter::callee_bits::CalleeBits;
use crate::interpreter::interpreter::Interpreter;
pub use crate::parser::parser::IterationStatus;
use crate::runtime::executable::ExecutableBaseRef;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::internal_function::InternalFunction;
use crate::runtime::js_arguments_objects::create_cloned_arguments;
use crate::runtime::js_global_object::JSGlobalObjectRef;
use crate::runtime::js_object::{JSObjectRef, PutError};
use crate::runtime::js_value::JSValue;
use crate::runtime::options_list::Options;
use crate::runtime::script_executable::ScriptExecutableRef;
use crate::runtime::vm::VM;
use crate::wtf::text::conversion_mode::ConversionMode;
use crate::wtf::text::wtf_string::{String as WtfString};

/// `StackVisitor::Frame::CodeType` (sem `Wasm`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameCodeType {
    Global,
    Eval,
    Function,
    Module,
    Native,
}

/// O corpo de `Frame::computeLineAndColumn` para um `CodeBlock` e um `BytecodeIndex` quaisquer (o frame assíncrono
/// de `Interpreter::getAsyncStackTrace` não tem `Frame`): `lineColumnForBytecodeIndex` com a coluna e a linha do
/// executável e o `overrideLineNumber`.
pub fn line_and_column_for(code_block: &CodeBlockRef, bytecode_index: BytecodeIndex) -> LineColumn {
    let block = code_block.borrow();
    let owner = block.owner_executable();
    let mut line_column = block.unlinked_code_block().borrow_mut().line_column_for_bytecode_index(bytecode_index);
    line_column.column += if line_column.line != 0 { 1 } else { block.first_line_column_offset() };
    line_column.line += owner.source().first_line().one_based_int() as u32;
    if let Some(override_line) = owner.override_line_number(block.global_object().vm()) {
        line_column.line = override_line as u32;
    }
    line_column
}

/// `StackVisitor::Frame`.
pub struct Frame {
    /// `m_callFrame`: `None` é o fim do percurso (`setToEnd`).
    call_frame: Option<CallFrame>,
    /// `m_callerFrame`.
    caller_frame: Option<CallFrame>,
    callee: CalleeBits,
    code_block: Option<CodeBlockRef>,
    bytecode_index: BytecodeIndex,
    argument_count_including_this: usize,
    index: usize,
}

/// `WTF::String` como `String` do Rust (o `utf8()` do C++).
pub(crate) fn to_rust_string(string: &WtfString) -> String {
    String::from_utf8_lossy(&string.utf8(ConversionMode::LenientConversion)).into_owned()
}

impl Frame {
    /// O frame depois do último (`setToEnd`).
    fn end() -> Frame {
        Frame {
            call_frame: None,
            caller_frame: None,
            callee: CalleeBits::null_callee(),
            code_block: None,
            bytecode_index: BytecodeIndex::from_offset(0),
            argument_count_including_this: 0,
            index: 0,
        }
    }

    /// `index()`.
    pub fn index(&self) -> usize {
        self.index
    }

    /// `argumentCountIncludingThis()`.
    pub fn argument_count_including_this(&self) -> usize {
        self.argument_count_including_this
    }

    /// `callerFrame()`.
    pub fn caller_frame(&self) -> Option<CallFrame> {
        self.caller_frame
    }

    /// `callFrame()`.
    pub fn call_frame(&self) -> Option<CallFrame> {
        self.call_frame
    }

    /// `callee()`.
    pub fn callee(&self) -> CalleeBits {
        self.callee
    }

    /// `codeBlock()`.
    pub fn code_block(&self) -> Option<&CodeBlockRef> {
        self.code_block.as_ref()
    }

    /// `bytecodeIndex()`.
    pub fn bytecode_index(&self) -> BytecodeIndex {
        self.bytecode_index
    }

    /// `isNativeCalleeFrame()`.
    pub fn is_native_callee_frame(&self) -> bool {
        self.callee.is_native_callee()
    }

    /// `isNativeFrame()`.
    pub fn is_native_frame(&self) -> bool {
        self.code_block.is_none() && !self.is_native_callee_frame()
    }

    /// `codeType()`.
    pub fn code_type(&self) -> FrameCodeType {
        let Some(code_block) = &self.code_block else {
            return FrameCodeType::Native;
        };
        match code_block.borrow().code_type() {
            BytecodeCodeType::EvalCode => FrameCodeType::Eval,
            BytecodeCodeType::ModuleCode => FrameCodeType::Module,
            BytecodeCodeType::FunctionCode => FrameCodeType::Function,
            BytecodeCodeType::GlobalCode => FrameCodeType::Global,
        }
    }

    /// `functionName()`.
    pub fn function_name(&self, vm: &VM) -> String {
        match self.code_type() {
            FrameCodeType::Eval => "eval code".to_string(),
            FrameCodeType::Module => "module code".to_string(),
            FrameCodeType::Global => "global code".to_string(),
            FrameCodeType::Function => {
                self.code_block.as_ref().map(|code_block| code_block.borrow().inferred_name()).unwrap_or_default()
            }
            FrameCodeType::Native => self.native_function_name(vm),
        }
    }

    /// O ramo `CodeType::Native` de `functionName`: o nome da função de host do `callee`.
    fn native_function_name(&self, vm: &VM) -> String {
        if !self.callee.is_cell() || self.callee.as_cell() == 0 {
            return String::new();
        }
        let cell = self.callee.as_cell();
        if let Some(function) = InternalFunction::from_cell_id(cell) {
            return to_rust_string(&function.calculated_display_name(vm));
        }
        match JSValue::from_cell(cell).as_js_function().map(|function| function.executable()) {
            Some(ExecutableBaseRef::Native(native)) => to_rust_string(&native.borrow().name_js_string(vm).value()),
            _ => String::new(),
        }
    }

    /// `StackFrame::functionName` do `Bun`: no código de função o nome vem do callee (propriedade `name`,
    /// `displayName`), não do `inferredName` do `CodeBlock`; sem callee que seja função cai no
    /// [`Frame::function_name`].
    pub fn stack_function_name(&self, vm: &VM) -> String {
        if self.code_type() == FrameCodeType::Function && self.callee.is_cell() && self.callee.as_cell() != 0 {
            let function = JSValue::from_cell(self.callee.as_cell()).as_js_function();
            let global_object = self.code_block.as_ref().map(|code_block| code_block.borrow().global_object().clone());
            if let (Some(function), Some(global_object)) = (function, global_object) {
                return to_rust_string(&function.stack_frame_name(vm, &global_object));
            }
        }
        self.function_name(vm)
    }

    /// `sourceURL()`.
    pub fn source_url(&self) -> String {
        let Some(code_block) = &self.code_block else {
            return "[native code]".to_string();
        };
        let source = code_block.borrow().owner_executable().source();
        source.provider().map(|provider| to_rust_string(provider.source_url())).unwrap_or_default()
    }

    /// `toString()`: `nome@url:linha:coluna`.
    pub fn to_string(&self, vm: &VM) -> String {
        let function_name = self.function_name(vm);
        let source_url = self.source_url();
        let separator = if !source_url.is_empty() && !function_name.is_empty() { "@" } else { "" };
        if source_url.is_empty() || !self.has_line_and_column_info() {
            return format!("{function_name}{separator}{source_url}");
        }
        let line_column = self.compute_line_and_column();
        format!("{function_name}{separator}{source_url}:{}:{}", line_column.line, line_column.column)
    }

    /// `hasLineAndColumnInfo()`.
    pub fn has_line_and_column_info(&self) -> bool {
        self.code_block.is_some()
    }

    /// `computeLineAndColumn()`: `CodeBlock::lineColumnForBytecodeIndex` (com a coluna e a linha do
    /// executável) e o `overrideLineNumber`. Sem `CodeBlock`, `{ }`.
    pub fn compute_line_and_column(&self) -> LineColumn {
        match &self.code_block {
            Some(code_block) => line_and_column_for(code_block, self.bytecode_index),
            None => LineColumn::default(),
        }
    }

    /// O frame é o de uma função embutida em JS (`@builtin`, como o `Reflect.apply`): o `Bun` mostra a posição dele como
    /// `native:1:11` e não a usa como a posição do erro lançado por função nativa chamada de dentro dela.
    pub fn is_builtin_function(&self) -> bool {
        self.code_block.as_ref().is_some_and(|code_block| {
            matches!(code_block.borrow().owner_executable(), ScriptExecutableRef::Function(function) if function.borrow().is_builtin_function())
        })
    }

    /// `isImplementationVisibilityPrivate()`.
    pub fn is_implementation_visibility_private(&self) -> bool {
        let visibility = if let Some(code_block) = &self.code_block {
            ExecutableBaseRef::Script(code_block.borrow().owner_executable().clone()).implementation_visibility()
        } else if self.callee.is_cell() {
            match JSValue::from_cell(self.callee.as_cell()).as_js_function() {
                Some(function) => function.executable().implementation_visibility(),
                None => ImplementationVisibility::Public,
            }
        } else {
            ImplementationVisibility::Public
        };
        match visibility {
            ImplementationVisibility::Public => false,
            ImplementationVisibility::Private | ImplementationVisibility::PrivateRecursive => {
                !Options::show_private_scripts_in_stack_traces()
            }
        }
    }

    /// `createArguments(vm)`: `ClonedArguments::createWithMachineFrame(globalObject, physicalFrame, mode)`,
    /// com `ArgumentsMode::FakeValues` (sem argumentos) quando `Options::useFunctionDotArguments` está
    /// desligada. O `globalObject` é o do `CodeBlock` do frame (o realm do `callee`).
    pub fn create_arguments(&self, interpreter: &Interpreter) -> Result<JSObjectRef, PutError> {
        let call_frame = self.call_frame.expect("createArguments em frame do fim do percurso");
        let code_block = self.code_block.as_ref().expect("createArguments em frame sem CodeBlock");
        let global_object: JSGlobalObjectRef = code_block.borrow().global_object().clone();
        let arguments =
            if Options::use_function_dot_arguments() { call_frame.arguments_span(&interpreter.stack) } else { Vec::new() };
        let callee = self
            .callee
            .is_cell()
            .then(|| JSValue::from_cell(self.callee.as_cell()).as_js_function())
            .flatten()
            .expect("jsCast<JSFunction*>(callee): o callee de um frame com createArguments é uma JSFunction");
        create_cloned_arguments(&global_object, &arguments, &callee)
    }
}

/// `class StackVisitor`.
pub struct StackVisitor<'a> {
    interpreter: &'a Interpreter,
    frame: Frame,
}

impl<'a> StackVisitor<'a> {
    /// `StackVisitor::visit(startFrame, vm, functor, skipFirstFrame)`: chama `functor` em cada frame de
    /// `top_frame` para fora, até ele devolver `Done` ou a corrente de `callerFrame` acabar.
    pub fn visit(
        interpreter: &'a Interpreter,
        top_frame: Option<CallFrame>,
        skip_first_frame: bool,
        mut functor: impl FnMut(&Frame) -> IterationStatus,
    ) {
        let mut visitor = StackVisitor { interpreter, frame: Frame::end() };
        let start = match (top_frame, skip_first_frame) {
            (Some(top), true) => visitor.caller_of(top),
            (top, _) => top,
        };
        visitor.read_frame(start);
        while visitor.frame.call_frame.is_some() {
            if functor(&visitor.frame) != IterationStatus::Continue {
                break;
            }
            visitor.goto_next_frame();
        }
    }

    /// `callFrame->callerFrame()`. A pilha cresce para índices menores, então o chamador está sempre
    /// acima; a guarda evita laço se o cabeçalho de um frame foi corrompido.
    fn caller_of(&self, call_frame: CallFrame) -> Option<CallFrame> {
        call_frame.caller_frame(&self.interpreter.stack).filter(|caller| caller.registers() > call_frame.registers())
    }

    /// `gotoNextFrame()`.
    fn goto_next_frame(&mut self) {
        self.frame.index += 1;
        let caller = self.frame.caller_frame;
        self.read_frame(caller);
    }

    /// `readFrame(callFrame)` e `readNonInlinedFrame`.
    fn read_frame(&mut self, call_frame: Option<CallFrame>) {
        let Some(call_frame) = call_frame else {
            self.frame.call_frame = None;
            return;
        };
        let stack = &self.interpreter.stack;
        let callee = call_frame.callee(stack);
        let code_block = if callee.is_native_callee() {
            None
        } else {
            call_frame.code_block(stack).and_then(|id| self.interpreter.code_block(id))
        };
        self.frame.bytecode_index =
            if code_block.is_some() { call_frame.bytecode_index(stack) } else { BytecodeIndex::from_offset(0) };
        self.frame.call_frame = Some(call_frame);
        self.frame.argument_count_including_this = call_frame.argument_count_including_this(stack);
        self.frame.caller_frame = self.caller_of(call_frame);
        self.frame.callee = callee;
        self.frame.code_block = code_block;
    }
}
