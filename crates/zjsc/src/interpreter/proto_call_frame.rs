//! Porte de `interpreter/ProtoCallFrame.h` e `ProtoCallFrameInlines.h`.
//!
//! O `ProtoCallFrame` é o descritor que o `Interpreter::execute*` monta e o `vmEntryToJavaScript`
//! copia para a pilha. Ponteiros viram identificadores: `CodeBlock*` é `CodeBlockId`, `JSObject*`,
//! `JSCell*` e `JSGlobalObject*` são o `CellId` (o número que `JSValue::Cell` carrega). Os
//! argumentos (`EncodedJSValue* args`) ficam num `Vec`, porque Rust seguro não guarda ponteiro
//! para fora do dono. O `CodeBlock` ainda não existe no porte, então `init` recebe também o
//! `numParameters()` dele (`code_block_num_parameters`), único dado que o C++ lê do bloco aqui.

use crate::interpreter::register::{CodeBlockId, Register};
use crate::runtime::js_value::{EncodedJSValue, JSValue};
use crate::runtime::stack_alignment::round_argument_count_to_align_frame;

/// `struct ProtoCallFrame`.
#[derive(Clone, Debug, Default)]
pub struct ProtoCallFrame {
    pub code_block_value: Register,
    pub callee_value: Register,
    pub arg_count_and_code_origin_value: Register,
    pub this_arg: Register,
    /// `JSCell* context`.
    pub context: Option<usize>,
    pub padded_arg_count: u32,
    /// `EncodedJSValue* args`.
    pub args: Vec<EncodedJSValue>,
    /// `JSGlobalObject* globalObject`.
    pub global_object: usize,
}

impl ProtoCallFrame {
    /// `CodeBlock, Callee, ArgumentCount, and |this|.`
    pub const NUMBER_OF_REGISTERS: u32 = 4;

    /// `init`. `code_block` é `None` para `nullptr`.
    pub fn init(
        &mut self,
        code_block: Option<CodeBlockId>,
        code_block_num_parameters: u32,
        global_object: usize,
        callee: usize,
        this_value: JSValue,
        context: Option<usize>,
        arg_count_including_this: i32,
        other_args: Vec<EncodedJSValue>,
    ) {
        self.args = other_args;
        self.context = context;
        self.set_code_block(code_block);
        self.set_callee(callee);
        self.set_global_object(global_object);
        self.set_argument_count_including_this(arg_count_including_this);
        let mut padded_args_count = arg_count_including_this as usize;
        if code_block.is_some() && (arg_count_including_this as u32) < code_block_num_parameters {
            padded_args_count = code_block_num_parameters as usize;
        }
        let padded_args_count = round_argument_count_to_align_frame(padded_args_count as u32);
        self.set_padded_arg_count(padded_args_count);
        self.clear_current_vpc();
        self.set_this_value(this_value);
    }

    /// `codeBlock()`.
    pub fn code_block(&self) -> Option<CodeBlockId> {
        self.code_block_value.code_block()
    }

    /// `setCodeBlock`.
    pub fn set_code_block(&mut self, code_block: Option<CodeBlockId>) {
        self.code_block_value = Register::from_code_block(code_block);
    }

    /// `callee()`: o identificador da célula do `JSObject`.
    pub fn callee(&self) -> usize {
        self.callee_value.object()
    }

    /// `setCallee`.
    pub fn set_callee(&mut self, callee: usize) {
        self.callee_value = Register::from_cell(callee);
    }

    /// `setGlobalObject`.
    pub fn set_global_object(&mut self, object: usize) {
        self.global_object = object;
    }

    /// `argumentCountIncludingThis()`.
    pub fn argument_count_including_this(&self) -> i32 {
        self.arg_count_and_code_origin_value.low_word()
    }

    /// `argumentCount()`.
    pub fn argument_count(&self) -> i32 {
        self.argument_count_including_this() - 1
    }

    /// `setArgumentCountIncludingThis`.
    pub fn set_argument_count_including_this(&mut self, count: i32) {
        self.arg_count_and_code_origin_value.set_low_word(count);
    }

    /// `setPaddedArgCount`.
    pub fn set_padded_arg_count(&mut self, arg_count: u32) {
        self.padded_arg_count = arg_count;
    }

    /// `clearCurrentVPC`.
    pub fn clear_current_vpc(&mut self) {
        self.arg_count_and_code_origin_value.set_high_word(0);
    }

    /// `thisValue()`.
    pub fn this_value(&self) -> JSValue {
        self.this_arg.js_value()
    }

    /// `setThisValue`.
    pub fn set_this_value(&mut self, value: JSValue) {
        self.this_arg = Register::from(value);
    }

    /// `argument`.
    pub fn argument(&self, argument_index: usize) -> JSValue {
        debug_assert!((argument_index as i32) < self.argument_count());
        JSValue::decode(self.args[argument_index])
    }

    /// `setArgument`.
    pub fn set_argument(&mut self, argument_index: usize, value: JSValue) {
        debug_assert!((argument_index as i32) < self.argument_count());
        self.args[argument_index] = value.encode();
    }
}
