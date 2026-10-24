//! Porte de `interpreter/Register.h` e `RegisterInlines.h`.
//!
//! O `Register` do C++ é uma união de 64 bits (`EncodedJSValue`, `CallFrame*`, `CodeBlock*`,
//! `double`, `int64_t`, duas palavras de 32 bits). Aqui é um `i64` com os mesmos bits. Onde o C++
//! guarda um ponteiro, Rust seguro guarda um índice: `CallFrame*` é o índice do frame na pilha
//! (`CLoopStack`, 0 é o ponteiro nulo, porque nenhum frame começa no índice 0), `CodeBlock*` é o
//! identificador do bloco na arena (`CodeBlockId`, 0 é nulo) e `JSCell*` é o `CellId`, que é o
//! próprio valor que `JSValue::Cell` carrega.

use crate::runtime::js_value::{EncodedJSValue, JSValue};

/// `LowWordOffset`.
pub const LOW_WORD_OFFSET: isize = 0;
/// `HighWordOffset`.
pub const HIGH_WORD_OFFSET: isize = 4;

/// `JSValue::int52ShiftAmount`.
pub const INT52_SHIFT_AMOUNT: u32 = 12;

/// Identificador de `CodeBlock` na arena (o `CodeBlock*` do C++). 0 é o ponteiro nulo.
pub type CodeBlockId = usize;

/// `class Register`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Register {
    value: i64,
}

impl Default for Register {
    /// `Register()`: no build sem `NDEBUG` vale `JSValue()`, cujos bits são 0; em release o
    /// conteúdo é indefinido. Os dois casos são 0 aqui.
    fn default() -> Register {
        Register { value: JSValue::empty().encode() }
    }
}

impl From<JSValue> for Register {
    /// `Register(const JSValue&)`.
    fn from(v: JSValue) -> Register {
        Register { value: v.encode() }
    }
}

impl Register {
    /// `Register::operator=(EncodedJSValue)`.
    pub fn from_encoded(encoded: EncodedJSValue) -> Register {
        Register { value: encoded }
    }

    /// `Register::operator=(CallFrame*)`: `frame` é o índice na pilha, `None` é `nullptr`.
    pub fn from_call_frame(frame: Option<usize>) -> Register {
        Register { value: frame.unwrap_or(0) as i64 }
    }

    /// `Register::operator=(CodeBlock*)`.
    pub fn from_code_block(code_block: Option<CodeBlockId>) -> Register {
        Register { value: code_block.unwrap_or(0) as i64 }
    }

    /// `Register::operator=(JSCell*)` e `operator=(JSScope*)`: `JSValue::encode(JSValue(cell))`.
    pub fn from_cell(cell: usize) -> Register {
        Register::from(JSValue::from_cell(cell))
    }

    /// `Register::withInt`: `jsNumber(i)`.
    pub fn with_int(i: i32) -> Register {
        Register::from(JSValue::Int32(i))
    }

    /// `jsValue()` e `asanUnsafeJSValue()`.
    pub fn js_value(&self) -> JSValue {
        JSValue::decode(self.value)
    }

    /// `encodedJSValue()`.
    pub fn encoded_js_value(&self) -> EncodedJSValue {
        self.value
    }

    /// `i()`.
    pub fn i(&self) -> i32 {
        self.js_value().as_int32()
    }

    /// `callFrame()`: índice do frame na pilha, `None` para `nullptr`.
    pub fn call_frame(&self) -> Option<usize> {
        if self.value == 0 { None } else { Some(self.value as usize) }
    }

    /// `codeBlock()` e `asanUnsafeCodeBlock()`.
    pub fn code_block(&self) -> Option<CodeBlockId> {
        if self.value == 0 { None } else { Some(self.value as usize) }
    }

    /// `object()`: `asObject(jsValue())`, o identificador da célula.
    pub fn object(&self) -> usize {
        self.js_value().as_cell()
    }

    /// `unboxedUInt32()` (e `unboxedInt32()`, que é `low_word`).
    pub fn unboxed_uint32(&self) -> u32 {
        self.low_word() as u32
    }

    /// `unboxedInt52()` e `asanUnsafeUnboxedInt52()` (RegisterInlines.h).
    pub fn unboxed_int52(&self) -> i64 {
        self.value >> INT52_SHIFT_AMOUNT
    }

    /// `unboxedStrictInt52()`.
    pub fn unboxed_strict_int52(&self) -> i64 {
        self.value
    }

    /// `unboxedInt64()`.
    pub fn unboxed_int64(&self) -> i64 {
        self.value
    }

    /// `unboxedDouble()`.
    pub fn unboxed_double(&self) -> f64 {
        f64::from_bits(self.value as u64)
    }

    /// `unboxedCell()`.
    pub fn unboxed_cell(&self) -> usize {
        self.value as usize
    }

    /// `pointer()`.
    pub fn pointer(&self) -> usize {
        self.value as usize
    }

    /// `lowWord()` (leitura).
    pub fn low_word(&self) -> i32 {
        self.value as i32
    }

    /// `highWord()` (leitura) e `unsafeHighWord()`.
    pub fn high_word(&self) -> i32 {
        (self.value >> 32) as i32
    }

    /// `lowWord()` (referência): atribuição da palavra baixa, a alta fica.
    pub fn set_low_word(&mut self, word: i32) {
        self.value = (self.value & !0xffff_ffff) | (word as u32 as i64);
    }

    /// `highWord()` (referência): atribuição da palavra alta, a baixa fica.
    pub fn set_high_word(&mut self, word: i32) {
        self.value = (self.value & 0xffff_ffff) | ((word as i64) << 32);
    }
}
