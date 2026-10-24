//! Conversões de `JSValue` (`JSCJSValue.h`, `JSCJSValueInlines.h`, `JSCJSValueCell.h`,
//! `JSCJSValue.cpp`) para todo tipo de valor: `Undefined`, `Null`, `Bool`, `Int32`, `Double` e as células
//! `JSString`, `Symbol`, `JSBigInt` e objeto. É o que os caminhos aritméticos de `1+1`, `a+1`, `"a"+1`
//! e `{}+1` precisam: `toNumber`, `toInt32`, `toUInt32`, `toIntegerOrInfinity`, `toBoolean`, `toPrimitive`,
//! `toNumeric`, `toString`, `toWTFString`.
//!
//! O `jsToNumber` de `JSGlobalObjectFunctions.cpp` (`toDouble`, `jsStrDecimalLiteral` e os literais
//! com base) mora em `ul_common::js_number`, que o zbun também usa; este módulo o reexporta.
//!
//! Também o ramo `radix == 10` de `int32ToString` e `numberToString` (`NumberPrototype.cpp`), que é o
//! único que `toStringSlowCase` usa. Os outros radix entram com `NumberPrototype`.
//!
//! Célula que não é `JSString`: o despacho virtual de `JSCell` (`toNumber`, `toBoolean`, `toPrimitive`,
//! `toStringInline`, `toObject`) é o `match` sobre [`CellKind`]. `Symbol` lança o `TypeError` de
//! `Symbol::toNumber` ("Cannot convert a symbol to a number") e o de `JSCell::toStringSlowCase`
//! ("Cannot convert a symbol to a string"); `JSBigInt` lança o de `JSBigInt::toNumber`; objeto chama
//! `JSObject::toPrimitive` (`object_to_primitive`) e depois a conversão do primitivo.
//!
//! DIVERGÊNCIAS e o que fica de fora:
//!
//! - As conversões não recebem `globalObject`: o reino é o de `current_realm`. O erro e a exceção ficam
//!   pendentes no `VM` (como o `ThrowScope` do C++), e a conversão devolve o valor que o C++ devolve nesse
//!   caso: 0 em `toNumber`, a string vazia em `toString`, `empty` em `toPrimitive`. Quem chama confere
//!   `vm.exception()`.
//! - `isBigInt32` (`USE(BIGINT32)` é 0), `toBigInt` e `toBigIntOrInt32` ficam fora; `toObject` de
//!   `JSBigInt` precisa de `BigIntObject`, que o porte ainda não tem (ver `host_function_support`).
//! - `JSBigInt::toString` passa por `tryGetString` (sem o `throwOutOfMemoryError` do `globalObject`).
//! - `NumericStrings` e `SmallStrings` (caches) não têm efeito observável e não são portados; cada
//!   chamada cria a `JSString`.

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::current_realm::{current_global_object, has_pending_exception};
use crate::runtime::host_call::Thrown;
use crate::runtime::host_function_support::{throw_vm_type_error, ObjectRef};
use crate::runtime::js_big_int::JSBigInt;
use crate::runtime::js_string::{js_empty_string, js_string, JSString, JSStringRef};
use crate::runtime::js_type::JSType;
use crate::runtime::js_value::{js_number, pnan, JSValue};
use crate::runtime::math_common;
use crate::runtime::object_to_primitive::{object_to_primitive, PreferredPrimitiveType};
use crate::runtime::vm::VM;
use crate::wtf::math_extras::truncate_double_to_int32;
use crate::wtf::text::string_view::{StringView, StringViewData};
use crate::wtf::text::wtf_string::String as WtfString;

/// O tipo concreto de uma célula, o que o despacho virtual de `JSCell` distingue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CellKind {
    String,
    Symbol,
    BigInt,
    Object,
}

/// `dynamicDowncast<JSString>`, `<Symbol>`, `<JSBigInt>` ou, no resto, `downcast<JSObject>`.
pub(crate) fn cell_kind(cell_id: usize) -> CellKind {
    match cell_registry::cell_type(cell_id) {
        Some(JSType::StringType) => CellKind::String,
        Some(JSType::SymbolType) => CellKind::Symbol,
        Some(JSType::HeapBigIntType) => CellKind::BigInt,
        _ => CellKind::Object,
    }
}

/// `throwTypeError(globalObject, scope, message)` no reino corrente.
fn throw_type_error_in_realm(message: &str) {
    throw_vm_type_error(&current_global_object(), Some(message));
}

/// `structure()->masqueradesAsUndefined(globalObject)` de uma célula: só objeto com a flag no `TypeInfo`,
/// e só no reino da própria estrutura.
pub(crate) fn cell_masquerades_as_undefined(value: &JSValue) -> bool {
    if cell_kind(value.as_cell()) != CellKind::Object {
        return false;
    }
    let Some(object) = ObjectRef::from_value(value) else {
        return false;
    };
    let structure = object.structure();
    structure.type_info().masquerades_as_undefined()
        && structure.realm().is_some_and(|realm| realm.cell_id() == current_global_object().cell_id())
}

// ---------------------------------------------------------------------------------------------
// JSGlobalObjectFunctions.cpp: jsToNumber
// ---------------------------------------------------------------------------------------------

/// `maxSafeInteger()`: 2^53 - 1.
const MAX_SAFE_INTEGER: f64 = 9007199254740991.0;

pub(crate) use ul_common::js_number::{js_str_decimal_literal, js_to_number_characters, skip_str_white_space};

/// `jsToNumber(StringView)`: o `StringToNumber` de ECMA-262. A visão nula é de 8 bits e vazia, como
/// no C++, e dá 0.
pub fn js_to_number(s: StringView) -> f64 {
    match s.data() {
        StringViewData::Null => js_to_number_characters::<u8>(&[]),
        StringViewData::Latin1(characters) => js_to_number_characters(characters),
        StringViewData::Utf16(characters) => js_to_number_characters(characters),
    }
}

/// `JSString::toNumber(JSGlobalObject*)`.
pub fn js_string_to_number(string: &JSString) -> f64 {
    let value = string.value();
    js_to_number(StringView::from(&value))
}

// ---------------------------------------------------------------------------------------------
// NumberPrototype.cpp: int32ToString / numberToString com radix 10
// ---------------------------------------------------------------------------------------------

/// `int32ToString(vm, value, 10)`: o ramo `radix == 10` de `int32ToStringInternal`.
pub fn int32_to_string_radix10(vm: &VM, value: i32) -> JSStringRef {
    js_string(vm, &WtfString::number_i32(value))
}

/// `numberToString(vm, doubleValue, 10)`: o ramo `radix == 10` de `numberToStringInternal`.
pub fn number_to_string_radix10(vm: &VM, double_value: f64) -> JSStringRef {
    let integer_value = truncate_double_to_int32(double_value);
    if integer_value as f64 == double_value {
        return int32_to_string_radix10(vm, integer_value);
    }

    // `vm.numericStrings.addJSString(vm, doubleValue)`: `String::number(double)`.
    js_string(vm, &WtfString::number_f64(double_value))
}

// ---------------------------------------------------------------------------------------------
// JSValue
// ---------------------------------------------------------------------------------------------

impl JSValue {
    /// `JSValue::toNumber(globalObject)`.
    pub fn to_number(&self) -> f64 {
        if let JSValue::Int32(i) = self {
            return *i as f64;
        }
        if let JSValue::Double(d) = self {
            return *d;
        }
        self.to_number_slow_case()
    }

    /// `JSValue::toNumberSlowCase(globalObject)`.
    fn to_number_slow_case(&self) -> f64 {
        debug_assert!(!self.is_int32() && !self.is_double());
        if self.is_cell() {
            // `asCell()->toNumber(globalObject)`.
            return match cell_kind(self.as_cell()) {
                CellKind::String => js_string_to_number(&self.as_js_string()),
                CellKind::Symbol => {
                    throw_type_error_in_realm("Cannot convert a symbol to a number");
                    0.0
                }
                CellKind::BigInt => {
                    throw_type_error_in_realm("Conversion from 'BigInt' to 'number' is not allowed.");
                    0.0
                }
                // `JSObject::toNumber`.
                CellKind::Object => {
                    let primitive = self.to_primitive_preferred(PreferredPrimitiveType::PreferNumber);
                    if primitive.is_empty() {
                        return 0.0;
                    }
                    primitive.to_number()
                }
            };
        }
        if self.is_true() {
            return 1.0;
        }
        // null and false both convert to 0.
        if self.is_undefined() { pnan() } else { 0.0 }
    }

    /// `JSValue::toNumberFromPrimitive()`.
    pub fn to_number_from_primitive(&self) -> Option<f64> {
        if self.is_empty() {
            return None;
        }
        if self.is_number() {
            return Some(self.as_number());
        }
        if self.is_boolean() {
            return Some(self.as_boolean() as i32 as f64);
        }
        if self.is_undefined() {
            return Some(pnan());
        }
        if self.is_null() {
            return Some(0.0);
        }
        None
    }

    /// `JSValue::toInt32(globalObject)`.
    pub fn to_int32(&self) -> i32 {
        if let JSValue::Int32(i) = self {
            return *i;
        }

        let d = self.to_number();
        math_common::to_int32(d)
    }

    /// `JSValue::toUInt32(globalObject)`: o `toInt32` reinterpretado como `uint32_t`
    /// (https://tc39.es/ecma262/#sec-touint32).
    pub fn to_uint32(&self) -> u32 {
        self.to_int32() as u32
    }

    /// `JSValue::toIntegerOrInfinity(globalObject)` (https://tc39.es/ecma262/#sec-tointegerorinfinity).
    pub fn to_integer_or_infinity(&self) -> f64 {
        if let JSValue::Int32(i) = self {
            return *i as f64;
        }
        let d = self.to_number();
        if d.is_nan() { 0.0 } else { d.trunc() + 0.0 }
    }

    /// `JSValue::toLength(globalObject)` (https://tc39.es/ecma262/#sec-tolength): o inteiro em
    /// `[0, 2^53 - 1]`. Com exceção pendente devolve o que `toIntegerOrInfinity` devolve (0 ou NaN em 0).
    pub fn to_length(&self) -> u64 {
        if let JSValue::Int32(i) = self {
            return (*i).max(0) as u64;
        }
        let d = self.to_integer_or_infinity();
        if d <= 0.0 {
            return 0;
        }
        d.min(MAX_SAFE_INTEGER) as u64
    }

    /// `toIntegerOrInfinity` seguido do `RETURN_IF_EXCEPTION`: `Err(Pending)` se a conversão lançou.
    pub fn to_integer_or_infinity_checked(&self) -> Result<f64, Thrown> {
        let d = self.to_integer_or_infinity();
        if has_pending_exception() { Err(Thrown::Pending) } else { Ok(d) }
    }

    /// `toLength` seguido do `RETURN_IF_EXCEPTION`: `Err(Pending)` se a conversão lançou.
    pub fn to_length_checked(&self) -> Result<u64, Thrown> {
        let length = self.to_length();
        if has_pending_exception() { Err(Thrown::Pending) } else { Ok(length) }
    }

    /// `JSValue::toIndex(globalObject, errorName)` (https://tc39.es/ecma262/#sec-toindex).
    pub fn to_index(&self, error_name: &str) -> Result<u64, Thrown> {
        if let JSValue::Int32(integer) = self {
            if *integer < 0 {
                return Err(Thrown::range_error(&format!("{error_name} cannot be negative")));
            }
            return Ok(*integer as u64);
        }

        // RETURN_IF_EXCEPTION: a conversão de objeto (`valueOf`, `@@toPrimitive`) pode ter lançado.
        let d = self.to_integer_or_infinity_checked()?;
        if d < 0.0 {
            return Err(Thrown::range_error(&format!("{error_name} cannot be negative")));
        }
        if d > MAX_SAFE_INTEGER {
            return Err(Thrown::range_error(&format!("{error_name} larger than (2 ** 53) - 1")));
        }
        Ok(d as u64)
    }

    /// `JSValue::toBoolean(globalObject)`.
    pub fn to_boolean(&self) -> bool {
        if let JSValue::Int32(i) = self {
            return *i != 0;
        }
        if let JSValue::Double(d) = self {
            // false for NaN
            return *d > 0.0 || *d < 0.0;
        }
        if self.is_cell() {
            // `asCell()->toBoolean(globalObject)`.
            return match cell_kind(self.as_cell()) {
                // `JSString::toBoolean` é `!!length()`.
                CellKind::String => self.as_js_string().length() != 0,
                CellKind::BigInt => match cell_registry::get(self.as_cell()) {
                    Some(CellEntry::BigInt(big_int)) => big_int.to_boolean(),
                    _ => unreachable!("célula HeapBigIntType fora do registro"),
                },
                // `!structure()->masqueradesAsUndefined(globalObject)`.
                CellKind::Symbol | CellKind::Object => !cell_masquerades_as_undefined(self),
            };
        }
        // false, null, and undefined all convert to false.
        self.is_true()
    }

    /// `JSValue::toPrimitive(globalObject, preferredType)`: o `JSValue` vazio com a exceção pendente no
    /// `VM` se a conversão de objeto lança. `JSString`, `Symbol` e `JSBigInt` devolvem a si mesmos.
    pub fn to_primitive_preferred(&self, preferred_type: PreferredPrimitiveType) -> JSValue {
        if !self.is_cell() || cell_kind(self.as_cell()) != CellKind::Object {
            return *self;
        }
        // Célula que `cell_kind` classifica como objeto mas que não é `JSObject` (entrada do registro
        // sem a base `JSObject`, como `Exception` ou `GetterSetter`): sem conversão possível, o valor
        // volta a si mesmo em vez de abortar. Ver wip-notes/panic-audit.md.
        let Some(object) = ObjectRef::from_value(self) else {
            return *self;
        };
        object_to_primitive(&current_global_object(), &object, preferred_type).unwrap_or_else(JSValue::empty)
    }

    /// `JSValue::toPrimitive(globalObject)` com o `NoPreference` padrão.
    pub fn to_primitive(&self) -> JSValue {
        self.to_primitive_preferred(PreferredPrimitiveType::NoPreference)
    }

    /// `JSValue::toNumeric(globalObject)`: o número ou o `JSBigInt` (vazio com a exceção pendente se a
    /// conversão lança).
    pub fn to_numeric(&self) -> JSValue {
        if self.is_int32() || self.is_double() || self.is_big_int() {
            return *self;
        }

        if self.is_string() {
            return js_number(js_string_to_number(&self.as_js_string()));
        }

        let prim_value = self.to_primitive_preferred(PreferredPrimitiveType::PreferNumber);
        if prim_value.is_empty() {
            return prim_value;
        }

        if prim_value.is_double() || prim_value.is_big_int() {
            return prim_value;
        }

        let value = prim_value.to_number();
        if has_pending_exception() {
            return JSValue::empty();
        }

        js_number(value)
    }

    /// `isBigInt()`: célula `JSBigInt` (o `BigInt32` não existe, `USE(BIGINT32)` é 0).
    pub fn is_big_int(&self) -> bool {
        self.is_cell() && cell_kind(self.as_cell()) == CellKind::BigInt
    }

    /// `isSymbol()`.
    pub fn is_symbol(&self) -> bool {
        self.is_cell() && cell_kind(self.as_cell()) == CellKind::Symbol
    }

    /// `JSValue::toString(globalObject)`.
    pub fn to_string(&self, vm: &VM) -> JSStringRef {
        if self.is_string() {
            return self.as_js_string();
        }
        self.to_string_slow_case(vm)
    }

    /// `JSValue::toStringSlowCase(globalObject, returnEmptyStringOnError)`. Com exceção pendente
    /// devolve a string vazia (o `returnEmptyStringOnError` do C++; o `nullptr` não existe aqui).
    fn to_string_slow_case(&self, vm: &VM) -> JSStringRef {
        debug_assert!(!self.is_string());
        if let JSValue::Int32(i) = self {
            return int32_to_string_radix10(vm, *i);
        }
        if let JSValue::Double(d) = self {
            return number_to_string_radix10(vm, *d);
        }
        if let Some(text) = self.keyword_text() {
            return js_string(vm, &WtfString::from_latin1(text));
        }
        // `asCell()->toStringInline(globalObject)`.
        match cell_kind(self.as_cell()) {
            CellKind::String => self.as_js_string(),
            // `JSCell::toStringSlowCase`.
            CellKind::Symbol => {
                throw_type_error_in_realm("Cannot convert a symbol to a string");
                js_empty_string(vm)
            }
            CellKind::BigInt => match cell_registry::get(self.as_cell()) {
                Some(CellEntry::BigInt(big_int)) => js_string(vm, &JSBigInt::try_get_string(vm, &big_int, 10)),
                _ => unreachable!("célula HeapBigIntType fora do registro"),
            },
            // `JSObject::toString`: `toPrimitive(PreferString)` e depois `toString` do primitivo.
            CellKind::Object => {
                let primitive = self.to_primitive_preferred(PreferredPrimitiveType::PreferString);
                if primitive.is_empty() {
                    return js_empty_string(vm);
                }
                primitive.to_string(vm)
            }
        }
    }

    /// `JSValue::toWTFString(globalObject)`.
    pub fn to_wtf_string(&self) -> WtfString {
        if self.is_string() {
            return self.as_js_string().value();
        }
        self.to_wtf_string_slow_case()
    }

    /// `JSValue::toWTFStringSlowCase(globalObject)`: a string vazia com a exceção pendente.
    fn to_wtf_string_slow_case(&self) -> WtfString {
        if let JSValue::Int32(i) = self {
            return WtfString::number_i32(*i);
        }
        if let JSValue::Double(d) = self {
            return WtfString::number_f64(*d);
        }
        if let Some(text) = self.keyword_text() {
            return WtfString::from_latin1(text);
        }
        // `toString(globalObject)`, e o `RETURN_IF_EXCEPTION` que a deixa vazia.
        let global_object = current_global_object();
        let string = self.to_string(global_object.vm());
        if global_object.vm().exception().is_some() {
            return WtfString::default();
        }
        string.value()
    }

    /// O texto de `true`, `false`, `null` e `undefined` (`propertyNames->trueKeyword` etc. e as
    /// `SmallStrings` correspondentes).
    fn keyword_text(&self) -> Option<&'static [u8]> {
        match self {
            JSValue::Bool(true) => Some(b"true"),
            JSValue::Bool(false) => Some(b"false"),
            JSValue::Null => Some(b"null"),
            JSValue::Undefined => Some(b"undefined"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::js_value::{js_boolean, js_null, js_undefined};

    fn number_of(text: &str) -> f64 {
        let string = WtfString::from_latin1(text.as_bytes());
        js_to_number(StringView::from(&string))
    }

    fn assert_same_number(actual: f64, expected: f64) {
        // Compara os bits para distinguir -0 de +0.
        assert_eq!(actual.to_bits(), expected.to_bits(), "{actual} != {expected}");
    }

    #[test]
    fn string_to_number() {
        // Casos de borda que o C++ trata antes do parser: tamanho 1 e "-d" de tamanho 2.
        assert_same_number(number_of(""), 0.0);
        assert_same_number(number_of("7"), 7.0);
        assert_same_number(number_of(" "), 0.0);
        assert_same_number(number_of("\n"), 0.0);
        assert!(number_of("x").is_nan());
        assert!(number_of("-").is_nan());
        assert_same_number(number_of("-0"), -0.0);
        assert_same_number(number_of("-5"), -5.0);
        assert!(number_of("-x").is_nan());
        // toDouble: espaço dos dois lados, literais com prefixo, Infinity.
        assert_same_number(number_of("  12  "), 12.0);
        assert_same_number(number_of("0x1F"), 31.0);
        assert_same_number(number_of("0X1f"), 31.0);
        assert_same_number(number_of("0o17"), 15.0);
        assert_same_number(number_of("0b101"), 5.0);
        assert!(number_of("0b102").is_nan());
        assert!(number_of("0o8").is_nan());
        assert!(number_of("0x").is_nan());
        assert_same_number(number_of("Infinity"), f64::INFINITY);
        assert_same_number(number_of("+Infinity"), f64::INFINITY);
        assert_same_number(number_of("-Infinity"), f64::NEG_INFINITY);
        assert!(number_of("Infinit").is_nan());
        assert!(number_of("infinity").is_nan());
        assert!(number_of("12abc").is_nan());
        assert!(number_of("1 2").is_nan());
        assert_same_number(number_of("1e3"), 1000.0);
        assert_same_number(number_of(".5"), 0.5);
        assert_same_number(number_of("+5"), 5.0);
        // Acima de 2^53 o literal hexadecimal refaz a conta por parseIntOverflow.
        assert_same_number(number_of("0x20000000000000"), 9007199254740992.0);
        // Espaço fora do Latin1 (U+2028) e NBSP.
        let wide = WtfString::from_utf16(&[0x2028, 0x37, 0x00a0]);
        assert_same_number(js_to_number(StringView::from(&wide)), 7.0);
        assert_same_number(js_to_number(StringView::default()), 0.0);
    }

    #[test]
    fn to_number_of_primitives() {
        assert_same_number(JSValue::Int32(-3).to_number(), -3.0);
        assert_same_number(JSValue::Double(-0.0).to_number(), -0.0);
        assert!(js_undefined().to_number().is_nan());
        assert_same_number(js_null().to_number(), 0.0);
        assert_same_number(js_boolean(true).to_number(), 1.0);
        assert_same_number(js_boolean(false).to_number(), 0.0);
        let vm = VM::new();
        let string = JSValue::from_js_string(js_string(&vm, &WtfString::from_latin1(b" 0x10 ")));
        assert_same_number(string.to_number(), 16.0);
        assert_eq!(JSValue::Empty.to_number_from_primitive(), None);
        assert_eq!(js_null().to_number_from_primitive(), Some(0.0));
        assert_eq!(js_boolean(true).to_number_from_primitive(), Some(1.0));
        assert!(js_undefined().to_number_from_primitive().unwrap().is_nan());
        assert_eq!(string.to_number_from_primitive(), None);
    }

    #[test]
    fn to_int32_and_uint32() {
        // ECMA-262 ToInt32: trunca para zero e reduz módulo 2^32.
        assert_eq!(JSValue::Int32(-7).to_int32(), -7);
        assert_eq!(JSValue::Double(-0.0).to_int32(), 0);
        assert_eq!(JSValue::nan().to_int32(), 0);
        assert_eq!(JSValue::Double(f64::INFINITY).to_int32(), 0);
        assert_eq!(JSValue::Double(f64::NEG_INFINITY).to_int32(), 0);
        assert_eq!(JSValue::Double(4294967296.0).to_int32(), 0); // 2^32
        assert_eq!(JSValue::Double(4294967297.0).to_int32(), 1); // 2^32 + 1
        assert_eq!(JSValue::Double(2147483648.0).to_int32(), -2147483648); // 2^31
        assert_eq!(JSValue::Double(-2147483649.0).to_int32(), 2147483647); // -2^31 - 1
        assert_eq!(JSValue::Double(4294967295.5).to_int32(), -1);
        assert_eq!(JSValue::Double(-1.9).to_int32(), -1);
        assert_eq!(js_undefined().to_int32(), 0);
        assert_eq!(js_boolean(true).to_int32(), 1);
        assert_eq!(JSValue::Int32(-1).to_uint32(), 4294967295);
        assert_eq!(JSValue::Double(4294967296.0).to_uint32(), 0);
        assert_eq!(JSValue::Double(4294967295.0).to_uint32(), 4294967295);
        assert_eq!(JSValue::Double(-4294967295.0).to_uint32(), 1);
    }

    #[test]
    fn to_integer_or_infinity() {
        assert_same_number(JSValue::Int32(5).to_integer_or_infinity(), 5.0);
        assert_same_number(JSValue::nan().to_integer_or_infinity(), 0.0);
        assert_same_number(JSValue::Double(-0.5).to_integer_or_infinity(), 0.0); // trunc(-0.5) + 0.0 é +0
        assert_same_number(JSValue::Double(-2.7).to_integer_or_infinity(), -2.0);
        assert_same_number(JSValue::Double(f64::INFINITY).to_integer_or_infinity(), f64::INFINITY);
    }

    #[test]
    fn to_boolean() {
        assert!(!JSValue::Int32(0).to_boolean());
        assert!(JSValue::Int32(-1).to_boolean());
        assert!(!JSValue::Double(-0.0).to_boolean());
        assert!(!JSValue::nan().to_boolean());
        assert!(JSValue::Double(0.5).to_boolean());
        assert!(JSValue::Double(f64::NEG_INFINITY).to_boolean());
        assert!(!js_undefined().to_boolean());
        assert!(!js_null().to_boolean());
        assert!(js_boolean(true).to_boolean());
        assert!(!js_boolean(false).to_boolean());
        let vm = VM::new();
        let empty = JSValue::from_js_string(js_string(&vm, &WtfString::from_latin1(b"")));
        let zero = JSValue::from_js_string(js_string(&vm, &WtfString::from_latin1(b"0")));
        assert!(!empty.to_boolean());
        assert!(zero.to_boolean());
    }

    #[test]
    fn to_numeric_keeps_doubles() {
        let vm = VM::new();
        assert_eq!(JSValue::Int32(3).to_numeric(), JSValue::Int32(3));
        assert!(JSValue::Double(-0.0).to_numeric().is_double());
        // toNumber(null) é 0 e jsNumber(0) é int32.
        assert_eq!(js_null().to_numeric(), JSValue::Int32(0));
        assert_eq!(js_boolean(true).to_numeric(), JSValue::Int32(1));
        assert!(js_undefined().to_numeric().as_double().is_nan());
        let text = JSValue::from_js_string(js_string(&vm, &WtfString::from_latin1(b"2.5")));
        assert_eq!(text.to_numeric(), JSValue::Double(2.5));
        let text = JSValue::from_js_string(js_string(&vm, &WtfString::from_latin1(b"4")));
        assert_eq!(text.to_numeric(), JSValue::Int32(4));
        assert_eq!(text.to_primitive(), text);
    }

    #[test]
    fn string_to_number_edge_cases() {
        // Separador numérico só vale no literal do fonte, nunca em StringToNumber.
        assert!(number_of("1_000").is_nan());
        assert!(number_of("0x1_0").is_nan());
        assert!(number_of("1,5").is_nan());
        // Prefixo sem dígito, com sinal ou com espaço no meio não é literal radix.
        assert!(number_of("-0x10").is_nan());
        assert!(number_of("+0b1").is_nan());
        assert!(number_of("0x 1").is_nan());
        assert!(number_of("0xg").is_nan());
        assert!(number_of("0b").is_nan());
        assert!(number_of("0o").is_nan());
        assert_same_number(number_of("0B11"), 3.0);
        assert_same_number(number_of("0O7"), 7.0);
        assert_same_number(number_of("\t0x0f\n"), 15.0);
        // Decimal: expoente, ponto solto, zeros à esquerda, estouro e subfluxo.
        assert!(number_of(".").is_nan());
        assert!(number_of("e5").is_nan());
        assert!(number_of("1e").is_nan());
        assert!(number_of("1e+").is_nan());
        assert_same_number(number_of("5."), 5.0);
        assert_same_number(number_of("+.5"), 0.5);
        assert_same_number(number_of("-.5e1"), -5.0);
        assert_same_number(number_of("007"), 7.0);
        assert_same_number(number_of("1e1000"), f64::INFINITY);
        assert_same_number(number_of("-1e1000"), f64::NEG_INFINITY);
        assert_same_number(number_of("1e-1000"), 0.0);
        assert_same_number(number_of("-0.0"), -0.0);
        assert_same_number(number_of("  -0  "), -0.0);
        assert_same_number(number_of("1e21"), 1e21);
        // Infinity só com a grafia exata, com sinal opcional e sem lixo depois.
        assert!(number_of("+-Infinity").is_nan());
        assert!(number_of("Infinityx").is_nan());
        assert!(number_of("INFINITY").is_nan());
        assert!(number_of("- Infinity").is_nan());
        assert_same_number(number_of(" -Infinity "), f64::NEG_INFINITY);
        // NaN não é literal numérico.
        assert!(number_of("NaN").is_nan());
        // Dígito fora do ASCII não é dígito (algarismo árabe-índico U+0663).
        let arabic = WtfString::from_utf16(&[0x663]);
        assert!(js_to_number(StringView::from(&arabic)).is_nan());
        // Literal radix acima de 2^53 refaz a conta por parseIntOverflow (octal e binário).
        assert_same_number(number_of("0b100000000000000000000000000000000000000000000000000000"), 9007199254740992.0);
        assert_same_number(number_of("0o400000000000000000"), 9007199254740992.0);
        // Latin1 vs UTF-16: o mesmo texto converte igual.
        let wide = WtfString::from_utf16(&[0x31, 0x32, 0x2e, 0x35]);
        assert_same_number(js_to_number(StringView::from(&wide)), 12.5);
    }

    #[test]
    fn to_length_clamps() {
        assert_eq!(JSValue::Int32(-5).to_length(), 0);
        assert_eq!(JSValue::Int32(7).to_length(), 7);
        assert_eq!(JSValue::nan().to_length(), 0);
        assert_eq!(JSValue::Double(-0.5).to_length(), 0);
        assert_eq!(JSValue::Double(3.9).to_length(), 3);
        assert_eq!(JSValue::Double(f64::INFINITY).to_length(), 9007199254740991);
        assert_eq!(JSValue::Double(1e300).to_length(), 9007199254740991);
        assert_eq!(JSValue::Double(f64::NEG_INFINITY).to_length(), 0);
        assert_eq!(js_undefined().to_length(), 0);
        assert_eq!(js_boolean(true).to_length(), 1);
    }

    #[test]
    fn to_int32_extremes() {
        // Múltiplos de 2^32 e potências altas vão a zero; o resto reduz módulo 2^32.
        assert_eq!(JSValue::Double(1e300).to_int32(), 0);
        assert_eq!(JSValue::Double(-1e300).to_int32(), 0);
        assert_eq!(JSValue::Double(f64::MAX).to_int32(), 0);
        assert_eq!(JSValue::Double(9007199254740992.0).to_int32(), 0); // 2^53
        assert_eq!(JSValue::Double(9007199254740991.0).to_int32(), -1); // 2^53 - 1
        assert_eq!(JSValue::Double(-9007199254740991.0).to_int32(), 1);
        assert_eq!(JSValue::Double(2147483647.9).to_int32(), 2147483647);
        assert_eq!(JSValue::Double(-2147483648.9).to_int32(), -2147483648);
        assert_eq!(JSValue::Double(-2147483649.9).to_int32(), 2147483647);
        assert_eq!(JSValue::Double(1.8446744073709552e19).to_int32(), 0); // 2^64
        assert_eq!(JSValue::Double(1.8446744073709552e19 + 4096.0).to_int32(), 4096);
        assert_eq!(JSValue::Double(5e-324).to_uint32(), 0);
        assert_eq!(JSValue::Double(-0.9).to_uint32(), 0);
        assert_eq!(JSValue::Double(4294967295.9).to_uint32(), 4294967295);
        assert_eq!(JSValue::Double(-4294967296.5).to_uint32(), 0);
        assert_eq!(JSValue::Double(f64::NEG_INFINITY).to_uint32(), 0);
        // Das conversões de string, undefined e null.
        let vm = VM::new();
        let text = |source: &str| JSValue::from_js_string(js_string(&vm, &WtfString::from_latin1(source.as_bytes())));
        assert_eq!(text("4294967297").to_int32(), 1);
        assert_eq!(text("0xFFFFFFFF").to_int32(), -1);
        assert_eq!(text("x").to_int32(), 0);
        assert_eq!(js_null().to_uint32(), 0);
    }

    #[test]
    fn to_integer_or_infinity_edges() {
        assert_same_number(JSValue::Double(-0.0).to_integer_or_infinity(), 0.0);
        assert_same_number(JSValue::Double(f64::NEG_INFINITY).to_integer_or_infinity(), f64::NEG_INFINITY);
        assert_same_number(JSValue::Double(1e300).to_integer_or_infinity(), 1e300);
        assert_same_number(JSValue::Double(2.9).to_integer_or_infinity(), 2.0);
        assert_same_number(js_undefined().to_integer_or_infinity(), 0.0);
        assert_same_number(js_boolean(true).to_integer_or_infinity(), 1.0);
    }

    #[test]
    fn to_string_of_numbers_edges() {
        let vm = VM::new();
        assert_eq!(string_of(&JSValue::Double(1e-7), &vm), b"1e-7");
        assert_eq!(string_of(&JSValue::Double(123456789012345680000.0), &vm), b"123456789012345680000");
        assert_eq!(string_of(&JSValue::Double(-1e21), &vm), b"-1e+21");
        assert_eq!(string_of(&JSValue::Double(2147483648.0), &vm), b"2147483648");
        assert_eq!(string_of(&JSValue::Double(-2147483649.0), &vm), b"-2147483649");
        assert_eq!(string_of(&JSValue::Double(0.000001), &vm), b"0.000001");
        assert_eq!(string_of(&JSValue::Double(5e-324), &vm), b"5e-324");
        assert_eq!(string_of(&JSValue::Double(f64::MAX), &vm), b"1.7976931348623157e+308");
    }

    fn string_of(value: &JSValue, vm: &VM) -> Vec<u8> {
        value.to_string(vm).value().latin1()
    }

    #[test]
    fn to_string_of_primitives() {
        let vm = VM::new();
        assert_eq!(string_of(&JSValue::Int32(-5), &vm), b"-5");
        assert_eq!(string_of(&JSValue::Int32(0), &vm), b"0");
        assert_eq!(string_of(&JSValue::Double(0.5), &vm), b"0.5");
        // numberToString: truncateDoubleToInt32(-0.0) == -0.0, então vai pelo ramo de int32.
        assert_eq!(string_of(&JSValue::Double(-0.0), &vm), b"0");
        assert_eq!(string_of(&JSValue::Double(4294967296.0), &vm), b"4294967296");
        assert_eq!(string_of(&JSValue::Double(0.1 + 0.2), &vm), b"0.30000000000000004");
        assert_eq!(string_of(&JSValue::Double(1e21), &vm), b"1e+21");
        assert_eq!(string_of(&JSValue::nan(), &vm), b"NaN");
        assert_eq!(string_of(&JSValue::Double(f64::INFINITY), &vm), b"Infinity");
        assert_eq!(string_of(&JSValue::Double(f64::NEG_INFINITY), &vm), b"-Infinity");
        assert_eq!(string_of(&js_boolean(true), &vm), b"true");
        assert_eq!(string_of(&js_boolean(false), &vm), b"false");
        assert_eq!(string_of(&js_null(), &vm), b"null");
        assert_eq!(string_of(&js_undefined(), &vm), b"undefined");
        let text = JSValue::from_js_string(js_string(&vm, &WtfString::from_latin1(b"abc")));
        assert_eq!(string_of(&text, &vm), b"abc");
        assert_eq!(JSValue::Int32(12).to_wtf_string().latin1(), b"12");
        assert_eq!(JSValue::Double(-0.0).to_wtf_string().latin1(), b"0");
        assert_eq!(js_undefined().to_wtf_string().latin1(), b"undefined");
        assert_eq!(text.to_wtf_string().latin1(), b"abc");
    }
}
