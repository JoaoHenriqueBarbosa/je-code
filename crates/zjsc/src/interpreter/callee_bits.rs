//! Porte de `interpreter/CalleeBits.h`.
//!
//! O `NativeCallee` é do Wasm, que não existe neste porte; por isso só o predicado
//! `is_native_callee` (pelos bits) existe, sem `boxNativeCallee`. O `JSCell*` é o identificador da
//! célula (o mesmo que `JSValue::Cell` carrega); 0 é o ponteiro nulo.

use crate::runtime::js_value::{EncodedJSValue, NUMBER_TAG, OTHER_TAG};

/// `JSValue::NativeCalleeTag`.
pub const NATIVE_CALLEE_TAG: i64 = OTHER_TAG as i64 | 0x1;
/// `JSValue::NativeCalleeMask`.
pub const NATIVE_CALLEE_MASK: i64 = NUMBER_TAG | 0x7;

/// `class CalleeBits`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CalleeBits {
    ptr: i64,
}

impl CalleeBits {
    /// `CalleeBits(int64_t)`.
    pub fn new(value: i64) -> CalleeBits {
        CalleeBits { ptr: value }
    }

    /// `operator=(JSCell*)`.
    pub fn from_cell(cell: usize) -> CalleeBits {
        let bits = CalleeBits { ptr: cell as i64 };
        debug_assert!(bits.is_cell());
        bits
    }

    /// `nullCallee()`.
    pub const fn null_callee() -> CalleeBits {
        CalleeBits { ptr: 0 }
    }

    /// `encodeJSCallee`: `None` é `nullptr`.
    pub fn encode_js_callee(cell: Option<usize>) -> EncodedJSValue {
        match cell {
            None => CalleeBits::null_callee().encoded_bits(),
            Some(cell) => cell as i64,
        }
    }

    /// `encodeBoxedNativeCallee`.
    pub fn encode_boxed_native_callee(boxed_callee: i64) -> EncodedJSValue {
        boxed_callee
    }

    /// `encodedBits()`.
    pub fn encoded_bits(&self) -> EncodedJSValue {
        self.ptr
    }

    /// `isNativeCallee()`.
    pub fn is_native_callee(&self) -> bool {
        (self.ptr & NATIVE_CALLEE_MASK) == NATIVE_CALLEE_TAG
    }

    /// `isCell()`.
    pub fn is_cell(&self) -> bool {
        !self.is_native_callee()
    }

    /// `asCell()`: o identificador da célula (0 é nulo).
    pub fn as_cell(&self) -> usize {
        debug_assert!(!self.is_native_callee());
        self.ptr as usize
    }

    /// `rawPtr()`.
    pub fn raw_ptr(&self) -> i64 {
        self.ptr
    }

    /// `explicit operator bool`.
    pub fn is_some(&self) -> bool {
        self.ptr != 0
    }
}
