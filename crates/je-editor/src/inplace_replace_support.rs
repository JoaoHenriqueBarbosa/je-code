//! Porte de `src/vs/editor/common/languages/supports/inplaceReplaceSupport.ts`: o `BasicInplaceReplace`, que sobe e
//! desce o valor sob o cursor (número, ou um item dos conjuntos de valores padrão como `true`/`false`).
//!
//! Representação:
//! - `IInplaceReplaceSupportResult` (de `languages.ts`) é o [`InplaceReplaceSupportResult`], definido aqui porque o
//!   `BasicInplaceReplace` é o único produtor e o `EditorWorker` e o controlador de `inPlaceReplace` os únicos leitores.
//! - `BasicInplaceReplace.INSTANCE` não tem estado: os métodos são associados, sem instância.
//! - `range1 && text1`: o intervalo do original é sempre presente; `text1` vazio (a falsidade do `string`) pula o ramo.
//! - `Number(value)` e `parseFloat(value)` são o `string_to_number` e o `parse_float` do `je-platform`, e `String(n)` é o
//!   `number_to_js_string`, para a aritmética e a impressão do `numberReplace` serem as do JavaScript.

use je_base::JsString;
use je_base::dynamic::number_to_js_string;
use je_platform::js::{parse_float, string_to_number};

use crate::range::Range;

/// `IInplaceReplaceSupportResult`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InplaceReplaceSupportResult {
    pub range: Range,
    pub value: JsString,
}

/// `BasicInplaceReplace._defaultValueSet`.
const DEFAULT_VALUE_SET: &[&[&str]] = &[
    &["true", "false"],
    &["True", "False"],
    &["Private", "Public", "Friend", "ReadOnly", "Partial", "Protected", "WriteOnly"],
    &["public", "protected", "private"],
];

/// `BasicInplaceReplace.navigateValueSet(range1, text1, range2, text2, up)`: tenta o primeiro texto e, se ele não tem
/// substituto, o segundo.
pub fn navigate_value_set(range1: &Range, text1: &JsString, range2: &Range, text2: Option<&JsString>, up: bool) -> Option<InplaceReplaceSupportResult> {
    if !text1.is_empty() {
        if let Some(result) = do_navigate_value_set(text1, up) {
            return Some(InplaceReplaceSupportResult { range: *range1, value: result });
        }
    }

    if let Some(text2) = text2.filter(|text| !text.is_empty()) {
        if let Some(result) = do_navigate_value_set(text2, up) {
            return Some(InplaceReplaceSupportResult { range: *range2, value: result });
        }
    }

    None
}

fn do_navigate_value_set(text: &JsString, up: bool) -> Option<JsString> {
    number_replace(text, up).or_else(|| text_replace(text, up))
}

fn number_replace(value: &JsString, up: bool) -> Option<JsString> {
    let units = value.units();
    let last_dot = units.iter().rposition(|&unit| unit == u16::from(b'.')).map_or(-1, |index| index as i32);
    let precision = 10f64.powi(units.len() as i32 - (last_dot + 1));
    let n1 = string_to_number(units);
    let n2 = parse_float(units);

    if !n1.is_nan() && !n2.is_nan() && n1 == n2 {
        if n1 == 0.0 && !up {
            return None; // don't do negative
        }
        let stepped = (n1 * precision).floor() + if up { precision } else { -precision };
        return Some(JsString::from(number_to_js_string(stepped / precision)));
    }

    None
}

fn text_replace(value: &JsString, up: bool) -> Option<JsString> {
    DEFAULT_VALUE_SET.iter().find_map(|value_set| value_set_replace(value_set, value, up))
}

fn value_set_replace(value_set: &[&str], value: &JsString, up: bool) -> Option<JsString> {
    let index = value_set.iter().position(|candidate| JsString::from(*candidate) == *value)?;
    let next = if up { (index + 1) % value_set.len() } else { index.checked_sub(1).unwrap_or(value_set.len() - 1) };
    Some(JsString::from(value_set[next]))
}
