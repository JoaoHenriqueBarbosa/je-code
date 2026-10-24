//! Porte parcial de `runtime/JSCJSValue.h`, `JSCJSValueInlines.h` e `PureNaN.h`: o valor imediato do
//! JSVALUE64 (o único layout que este porte tem).
//!
//! PENDÊNCIA (depende do heap, camada 3): referência a célula. `JSValue::Cell(usize)` guarda o
//! endereço codificado da célula como número opaco (no C++ é o ponteiro `JSCell*` cru; aqui é o
//! índice do `CellId` do arena). `as_js_string`, `from_js_string`, `js_string`, `as_object`,
//! `is_string`, `is_object`, `is_big_int` e as conversões (`toNumber`, `toInt32`...) que olham o tipo da
//! célula ficam para quando o heap existir. Também fora: `BigInt32` (`USE(BIGINT32)` é 0 no
//! `PlatformUse.h`), `NativeCallee` do Wasm e `isAnyInt`/`Int52`.
//!
//! Contrato de codificação (`JSCJSValue.h:360`): `encode`/`decode` reproduzem os mesmos 64 bits do C++.
//!
//! ```text
//! Cell:      0000:PPPP:PPPP:PPPP   (ponteiro, bit 1 limpo)
//! Double:    bits do double + 2^49 (tags 0002..FFFC)
//! Int32:     FFFE:0000:IIII:IIII
//! Other:     False 0x06, True 0x07, Undefined 0x0a, Null 0x02
//! Empty:     0x00      Deleted: 0x04
//! ```

use crate::wtf::math_extras::try_convert_to_strict_int32;

/// `typedef int64_t EncodedJSValue`.
pub type EncodedJSValue = i64;

/// `typedef std::pair<EncodedJSValue, SourceCodeRepresentation> EncodedJSValueWithRepresentation`.
pub type EncodedJSValueWithRepresentation = (
    EncodedJSValue,
    crate::runtime::js_cjs_value_types::SourceCodeRepresentation,
);

// PureNaN.h
pub use ul_common::js_number::{pnan, PNAN_AS_BITS};
pub const IMPURE_NAN_AS_BITS: u64 = 0xffff000000000000;
pub const JS_VALUE_DOUBLE_ENCODE_OFFSET_BIT: usize = 49;
pub const JS_VALUE_DOUBLE_ENCODE_OFFSET: u64 = 1u64 << JS_VALUE_DOUBLE_ENCODE_OFFSET_BIT;
pub const JS_VALUE_NUMBER_TAG: u64 = 0xfffe000000000000;

/// `isImpureNaN`: o NaN cuja codificação não passaria no teste de "é double".
pub fn is_impure_nan(value: f64) -> bool {
    let bits = value.to_bits().wrapping_add(JS_VALUE_DOUBLE_ENCODE_OFFSET);
    let mask = bits & JS_VALUE_NUMBER_TAG;
    mask == 0 || mask == JS_VALUE_NUMBER_TAG
}

/// `purifyNaN`.
pub fn purify_nan(value: f64) -> f64 {
    // PureNaN.h:98: todo NaN vira o NaN puro, sem consultar `isImpureNaN`.
    if value.is_nan() { pnan() } else { value }
}

// Constantes de `JSValue` (JSCJSValue.h:389).
pub const DOUBLE_ENCODE_OFFSET: i64 = JS_VALUE_DOUBLE_ENCODE_OFFSET as i64;
pub const NUMBER_TAG: i64 = JS_VALUE_NUMBER_TAG as i64;
pub const LOWEST_OF_HIGH_BITS: i64 = 1i64 << 49;
pub const OTHER_TAG: i32 = 0x2;
pub const BOOL_TAG: i32 = 0x4;
pub const UNDEFINED_TAG: i32 = 0x8;
pub const VALUE_FALSE: i32 = OTHER_TAG | BOOL_TAG;
pub const VALUE_TRUE: i32 = OTHER_TAG | BOOL_TAG | 1;
pub const VALUE_UNDEFINED: i32 = OTHER_TAG | UNDEFINED_TAG;
pub const VALUE_NULL: i32 = OTHER_TAG;
pub const MISC_TAG: i64 = (OTHER_TAG | BOOL_TAG | UNDEFINED_TAG) as i64;
pub const NOT_CELL_MASK: i64 = NUMBER_TAG | OTHER_TAG as i64;
pub const VALUE_EMPTY: i32 = 0x0;
pub const VALUE_DELETED: i32 = 0x4;

/// `JSValue` no JSVALUE64, como enum decodificado. `encode`/`decode` dão os bits do C++.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JSValue {
    /// `JSValue()`: 0x0 (buraco de array, "sem valor").
    Empty,
    /// `JSValue(HashTableDeletedValue)`: 0x4.
    Deleted,
    Undefined,
    Null,
    Bool(bool),
    Int32(i32),
    /// Double que não é int32 estrito (os dois são `isNumber`).
    Double(f64),
    /// Célula do heap, opaca até o heap existir (veja o cabeçalho).
    Cell(usize),
}

impl JSValue {
    pub fn empty() -> JSValue {
        JSValue::Empty
    }

    pub fn deleted() -> JSValue {
        JSValue::Deleted
    }

    pub fn undefined() -> JSValue {
        JSValue::Undefined
    }

    pub fn null() -> JSValue {
        JSValue::Null
    }

    /// `jsNaN()`.
    pub fn nan() -> JSValue {
        JSValue::double_number(pnan())
    }

    /// `JSValue(EncodeAsDouble, d)` / `jsDoubleNumber`: sempre double, mesmo se couber em int32.
    pub fn double_number(d: f64) -> JSValue {
        debug_assert!(!is_impure_nan(d));
        JSValue::Double(d)
    }

    /// `JSValue(double)`: int32 quando estrito, double no resto.
    pub fn from_double(d: f64) -> JSValue {
        match try_convert_to_strict_int32(d) {
            Some(i) => JSValue::Int32(i),
            None => JSValue::double_number(d),
        }
    }

    /// `JSValue(unsigned)`.
    pub fn from_u32(i: u32) -> JSValue {
        if (i as i32) < 0 {
            return JSValue::double_number(i as f64);
        }
        JSValue::Int32(i as i32)
    }

    /// `JSValue(JSCell*)`.
    pub fn from_cell(cell: impl Into<usize>) -> JSValue {
        JSValue::Cell(cell.into())
    }

    /// `jsDynamicCast<JSFunction*>(value)`: `None` se o valor não é uma célula `JSFunction`.
    pub fn as_js_function(&self) -> Option<crate::runtime::js_function::JSFunctionRef> {
        let JSValue::Cell(cell_id) = self else {
            return None;
        };
        match crate::runtime::cell_registry::get(*cell_id)? {
            crate::runtime::cell_registry::CellEntry::Function(function) => Some(function),
            _ => None,
        }
    }

    /// `JSValue(JSString*)`: a variante `Cell` guarda o `cell_id` da string (veja `js_string`).
    pub fn from_js_string(string: crate::runtime::js_string::JSStringRef) -> JSValue {
        JSValue::Cell(string.cell_id())
    }

    /// `isString`: célula que é um `JSString`.
    pub fn is_string(&self) -> bool {
        match self {
            JSValue::Cell(p) => crate::runtime::js_string::JSString::from_cell_id(*p).is_some(),
            _ => false,
        }
    }

    /// `asString`: invariante de `isString`, como o `ASSERT` do C++.
    pub fn as_js_string(&self) -> crate::runtime::js_string::JSStringRef {
        match self {
            JSValue::Cell(p) => crate::runtime::js_string::JSString::from_cell_id(*p)
                .expect("as_js_string em célula que não é JSString"),
            _ => unreachable!("as_js_string em valor que não é célula"),
        }
    }

    /// `JSValue::encode`.
    pub fn encode(self) -> EncodedJSValue {
        match self {
            JSValue::Empty => VALUE_EMPTY as i64,
            JSValue::Deleted => VALUE_DELETED as i64,
            JSValue::Undefined => VALUE_UNDEFINED as i64,
            JSValue::Null => VALUE_NULL as i64,
            JSValue::Bool(true) => VALUE_TRUE as i64,
            JSValue::Bool(false) => VALUE_FALSE as i64,
            JSValue::Int32(i) => NUMBER_TAG | (i as u32 as i64),
            JSValue::Double(d) => (d.to_bits() as i64).wrapping_add(DOUBLE_ENCODE_OFFSET),
            JSValue::Cell(p) => p as i64,
        }
    }

    /// `JSValue::decode`. Padrão que o C++ nunca produz (callee do Wasm, BigInt32) é invariante
    /// violada, como o `RELEASE_ASSERT_NOT_REACHED` do C++ nas funções que o inspecionam.
    pub fn decode(encoded: EncodedJSValue) -> JSValue {
        if encoded == VALUE_EMPTY as i64 {
            return JSValue::Empty;
        }
        if encoded == VALUE_DELETED as i64 {
            return JSValue::Deleted;
        }
        if (encoded & NUMBER_TAG) == NUMBER_TAG {
            return JSValue::Int32(encoded as i32);
        }
        if encoded & NUMBER_TAG != 0 {
            return JSValue::Double(f64::from_bits(encoded.wrapping_sub(DOUBLE_ENCODE_OFFSET) as u64));
        }
        if encoded & NOT_CELL_MASK == 0 {
            return JSValue::Cell(encoded as usize);
        }
        match encoded {
            e if e == VALUE_FALSE as i64 => JSValue::Bool(false),
            e if e == VALUE_TRUE as i64 => JSValue::Bool(true),
            e if e == VALUE_UNDEFINED as i64 => JSValue::Undefined,
            e if e == VALUE_NULL as i64 => JSValue::Null,
            _ => unreachable!("JSValue com padrão de bits que não é imediato nem célula: {encoded:#x}"),
        }
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, JSValue::Empty)
    }

    pub fn is_deleted(&self) -> bool {
        matches!(self, JSValue::Deleted)
    }

    pub fn is_undefined(&self) -> bool {
        matches!(self, JSValue::Undefined)
    }

    pub fn is_null(&self) -> bool {
        matches!(self, JSValue::Null)
    }

    pub fn is_undefined_or_null(&self) -> bool {
        matches!(self, JSValue::Undefined | JSValue::Null)
    }

    pub fn is_boolean(&self) -> bool {
        matches!(self, JSValue::Bool(_))
    }

    pub fn is_true(&self) -> bool {
        matches!(self, JSValue::Bool(true))
    }

    pub fn is_false(&self) -> bool {
        matches!(self, JSValue::Bool(false))
    }

    /// `isCell`: no C++ vale também para Empty e Deleted (bits de ponteiro); a conferência é pelos
    /// bits, então os dois contam.
    pub fn is_cell(&self) -> bool {
        matches!(self, JSValue::Cell(_) | JSValue::Empty | JSValue::Deleted)
    }

    /// `JSValue::pureToBoolean`: o `ToBoolean` sem efeito colateral. Célula `JSString` decide pelo
    /// comprimento; as demais células seriam `JSCell::pureToBoolean` (objeto: verdadeiro, ou
    /// `Indeterminate` se mascara `undefined`), e como só há strings no heap por enquanto, o resto fica
    /// `Indeterminate`.
    pub fn pure_to_boolean(&self) -> crate::wtf::tri_state::TriState {
        use crate::wtf::math_extras::is_not_zero_and_ordered;
        use crate::wtf::tri_state::TriState;
        match self {
            JSValue::Bool(b) => TriState::from_bool(*b),
            JSValue::Int32(i) => TriState::from_bool(*i != 0),
            JSValue::Double(d) => TriState::from_bool(is_not_zero_and_ordered(*d)),
            JSValue::Undefined | JSValue::Null => TriState::False,
            JSValue::Cell(_) if self.is_string() => TriState::from_bool(self.as_js_string().length() != 0),
            JSValue::Cell(_) => TriState::Indeterminate,
            JSValue::Empty | JSValue::Deleted => unreachable!("pure_to_boolean em valor vazio ou deletado"),
        }
    }

    pub fn is_int32(&self) -> bool {
        matches!(self, JSValue::Int32(_))
    }

    pub fn is_uint32(&self) -> bool {
        matches!(self, JSValue::Int32(i) if *i >= 0)
    }

    pub fn is_double(&self) -> bool {
        matches!(self, JSValue::Double(_))
    }

    pub fn is_number(&self) -> bool {
        matches!(self, JSValue::Int32(_) | JSValue::Double(_))
    }

    pub fn as_boolean(&self) -> bool {
        debug_assert!(self.is_boolean());
        matches!(self, JSValue::Bool(true))
    }

    pub fn as_int32(&self) -> i32 {
        match self {
            JSValue::Int32(i) => *i,
            _ => {
                debug_assert!(false, "as_int32 em valor que não é int32");
                0
            }
        }
    }

    pub fn as_uint32(&self) -> u32 {
        match self {
            JSValue::Double(d) => *d as u32,
            _ => self.as_int32() as u32,
        }
    }

    pub fn as_double(&self) -> f64 {
        match self {
            JSValue::Double(d) => *d,
            _ => {
                debug_assert!(false, "as_double em valor que não é double");
                0.0
            }
        }
    }

    /// `asNumber`.
    pub fn as_number(&self) -> f64 {
        match self {
            JSValue::Int32(i) => *i as f64,
            _ => self.as_double(),
        }
    }

    /// `asCell` (opaco).
    pub fn as_cell(&self) -> usize {
        match self {
            JSValue::Cell(p) => *p,
            _ => {
                debug_assert!(false, "as_cell em valor que não é célula");
                0
            }
        }
    }

    /// `asInt32ForArithmetic`.
    pub fn as_int32_for_arithmetic(&self) -> i32 {
        if self.is_boolean() {
            return self.as_boolean() as i32;
        }
        self.as_int32()
    }
}

/// `jsUndefined()`.
pub fn js_undefined() -> JSValue {
    JSValue::Undefined
}

/// `jsNull()`.
pub fn js_null() -> JSValue {
    JSValue::Null
}

/// `jsTDZValue()`: o `JSValue()` vazio.
pub fn js_tdz_value() -> JSValue {
    JSValue::Empty
}

/// `jsBoolean`.
pub use JSValue::Bool as js_boolean;

/// `jsNumber(double)`; aceita também `i32` e `u32` por conversão sem perda para `f64`.
pub fn js_number(d: impl Into<f64>) -> JSValue {
    let d = d.into();
    debug_assert!(!is_impure_nan(d));
    JSValue::from_double(d)
}

/// `jsNumber(int32_t)`: `JSValue(int)`.
pub use JSValue::Int32 as js_number_i32;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immediate_bits_match_cpp() {
        assert_eq!(JSValue::Empty.encode(), 0x0);
        assert_eq!(JSValue::Deleted.encode(), 0x4);
        assert_eq!(JSValue::Null.encode(), 0x2);
        assert_eq!(JSValue::Undefined.encode(), 0xa);
        assert_eq!(JSValue::Bool(false).encode(), 0x6);
        assert_eq!(JSValue::Bool(true).encode(), 0x7);
        assert_eq!(JSValue::Int32(-1).encode() as u64, 0xfffe_0000_ffff_ffff);
        assert_eq!(JSValue::Int32(5).encode() as u64, 0xfffe_0000_0000_0005);
        // 1.5 = 0x3ff8000000000000, mais 2^49.
        assert_eq!(JSValue::Double(1.5).encode() as u64, 0x3ff8_0000_0000_0000 + (1u64 << 49));
    }

    #[test]
    fn round_trip() {
        let values = [
            JSValue::Empty,
            JSValue::Deleted,
            JSValue::Undefined,
            JSValue::Null,
            JSValue::Bool(true),
            JSValue::Bool(false),
            JSValue::Int32(0),
            JSValue::Int32(i32::MIN),
            JSValue::Int32(i32::MAX),
            JSValue::Double(1.5),
            JSValue::Double(-0.0),
            JSValue::Double(f64::INFINITY),
            JSValue::Double(f64::MAX),
            JSValue::Cell(0x7f00_1234_5670),
        ];
        for v in values {
            let back = JSValue::decode(v.encode());
            assert_eq!(back.encode(), v.encode());
            assert_eq!(back, v);
        }
        assert!(JSValue::decode(JSValue::nan().encode()).as_double().is_nan());
    }

    #[test]
    fn number_constructors() {
        assert_eq!(js_number(3.0), JSValue::Int32(3));
        assert_eq!(js_number(0), JSValue::Int32(0));
        assert!(js_number(-0.0).is_double());
        assert!(js_number(0.5).is_double());
        assert!(js_number(4294967295.0).is_double());
        assert_eq!(JSValue::from_u32(7), JSValue::Int32(7));
        assert_eq!(JSValue::from_u32(0x8000_0000), JSValue::Double(2147483648.0));
        assert!(JSValue::double_number(1.0).is_double());
        assert_eq!(JSValue::double_number(1.0).as_number(), 1.0);
    }

    #[test]
    fn predicates() {
        assert!(js_undefined().is_undefined_or_null() && js_null().is_undefined_or_null());
        assert!(js_tdz_value().is_empty());
        assert!(js_boolean(true).is_true() && js_boolean(false).is_false());
        assert!(JSValue::Int32(1).is_number() && !JSValue::Int32(1).is_double());
        assert!(JSValue::Int32(-1).is_int32() && !JSValue::Int32(-1).is_uint32());
        assert!(JSValue::from_cell(0x1000usize).is_cell());
        assert_eq!(JSValue::Int32(-1).as_uint32(), u32::MAX);
        assert_eq!(js_boolean(true).as_int32_for_arithmetic(), 1);
    }

    #[test]
    fn pure_nan() {
        assert!(!is_impure_nan(pnan()));
        assert!(is_impure_nan(f64::from_bits(IMPURE_NAN_AS_BITS)));
        assert_eq!(purify_nan(f64::from_bits(IMPURE_NAN_AS_BITS)).to_bits(), PNAN_AS_BITS);
    }
}
