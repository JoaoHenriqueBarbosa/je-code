//! Porte de `runtime/StringConstructor.cpp`: os algoritmos de `String.fromCharCode` e
//! `String.fromCodePoint` como funções puras sobre os argumentos já convertidos (o `String.raw` é só
//! conversão e concatenação intercaladas, e vive em `string_constructor_natives.rs`).
//!
//! DIVERGÊNCIAS: o objeto `StringConstructor` (um `InternalFunction` com `prototype`, `fromCharCode`,
//! `fromCodePoint` e `raw`) e as funções nativas esperam a `NativeFunction` final, que alcança
//! `thisValue` e `argument(n)` (ver `string_prototype.rs`); a chamada `String(value)` e o
//! `new String(value)` (que usa `construct_string` de `string_object.rs`, com a `JSString` já
//! convertida e o caso do `Symbol` na chamada) entram com ela. O `fromCharCode` de um só argumento
//! inteiro do C++ (`jsSingleCharacterString`) é o caso geral aqui, sem `SmallStrings`.

use crate::runtime::string_prototype::{string_from_units, StringOpError};
use crate::wtf::text::string_impl::MAX_LENGTH;
use crate::wtf::text::wtf_string::String as WtfString;

/// A mensagem do `RangeError` de `fromCodePoint`.
pub const FROM_CODE_POINT_RANGE_ERROR: &str = "Arguments contain a value that is out of range of code points";

/// Um argumento de `stringFromCodePoint` já convertido por `toNumber`: precisa ser um inteiro em
/// `[0, 0x10FFFF]` (`codePoint != codePointAsDouble || codePoint > UCHAR_MAX_VALUE`), senão `RangeError`;
/// entra como uma ou duas unidades UTF-16. O C++ confere cada argumento antes de converter o seguinte.
pub fn push_code_point(units: &mut Vec<u16>, value: f64) -> Result<(), StringOpError> {
    if !(value.is_finite() && value == value.trunc() && (0.0..=1_114_111.0).contains(&value)) {
        return Err(StringOpError::RangeError(FROM_CODE_POINT_RANGE_ERROR));
    }
    let code_point = value as u32;
    if code_point <= 0xFFFF {
        units.push(code_point as u16);
    } else {
        let offset = code_point - 0x10000;
        units.push(0xD800 + (offset >> 10) as u16);
        units.push(0xDC00 + (offset & 0x3FF) as u16);
    }
    if units.len() > MAX_LENGTH as usize {
        return Err(StringOpError::OutOfMemory);
    }
    Ok(())
}

/// `stringFromCodePoint`: cada argumento já passou por `toNumber`; precisa ser um inteiro em
/// `[0, 0x10FFFF]`, senão `RangeError`.
pub fn from_code_point(values: &[f64]) -> Result<WtfString, StringOpError> {
    let mut units: Vec<u16> = Vec::with_capacity(values.len());
    for &value in values {
        push_code_point(&mut units, value)?;
    }
    Ok(string_from_units(&units))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::string_prototype::code_units;

    #[test]
    fn constructor_algorithms() {
        assert_eq!(&*code_units(&string_from_units(&[72, 105])), &[72u16, 105][..]);
        assert_eq!(&*code_units(&from_code_point(&[128512.0]).unwrap()), &[0xD83Du16, 0xDE00][..]);
        assert!(from_code_point(&[1.5]).is_err());
        assert!(from_code_point(&[1_114_112.0]).is_err());
        assert!(from_code_point(&[f64::NAN]).is_err());
        assert!(from_code_point(&[-1.0]).is_err());
        assert!(from_code_point(&[f64::INFINITY]).is_err());
        // `-0` é o ponto de código 0, e o último ponto de código válido vira um par substituto.
        assert_eq!(&*code_units(&from_code_point(&[-0.0]).unwrap()), &[0u16][..]);
        assert_eq!(&*code_units(&from_code_point(&[1_114_111.0]).unwrap()), &[0xDBFFu16, 0xDFFF][..]);
    }

    #[test]
    fn push_code_point_checks_one_value_at_a_time() {
        let mut units = vec![65u16];
        assert!(push_code_point(&mut units, 0x1F600 as f64).is_ok());
        assert_eq!(units, vec![65u16, 0xD83D, 0xDE00]);
        // O valor recusado não deixa nada para trás.
        assert!(push_code_point(&mut units, 2.5).is_err());
        assert_eq!(units.len(), 3);
    }
}
