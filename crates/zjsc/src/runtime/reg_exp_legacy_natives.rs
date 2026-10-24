//! Porte da parte legada de `runtime/RegExpConstructor.cpp`: `RegExp.escape` (`regExpConstructorEscape`) e
//! os acessores estáticos de `regExpConstructorTable` (`input`/`$_`, `multiline`/`$*`, `lastMatch`/`$&`,
//! `lastParen`/`$+`, `leftContext`/`` $` ``, `rightContext`/`$'` e `$1` a `$9`), todos `CustomAccessor` lidos do
//! `RegExpGlobalData` do realm (`reg_exp_global_data.rs`).
//!
//! DIVERGÊNCIAS:
//! - Os acessores vivem em `REG_EXP_CONSTRUCTOR_TABLE` (`HasStaticPropertyTable`) e só entram na `Structure`
//!   quando alguém define o nome ou reifica tudo (`delete`, `Object.assign`); ler e escrever não reifica.
//!   `escape` entra antes do `@@species`, como em `finishCreation` (`install_escape`).
//! - `RegExp.input` antes do primeiro casamento: o C++ devolve o `JSString*` nulo (`m_lastInput` ainda
//!   sem valor), que o `JSValue` codifica como vazio; aqui é a string vazia, o que um `RegExp.input` num
//!   motor recém-criado mostra.

use crate::runtime::collection_support::put_native_function;
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::lookup::{custom_accessor_entry, custom_getter_entry};
use crate::runtime::property_slot::{GetValueFunc, PutValueFunc};
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_string::{js_empty_string, js_string};
use crate::runtime::js_value::{js_boolean, JSValue};
use crate::runtime::parse_int::is_str_white_space;
use crate::runtime::property_attribute::{CUSTOM_ACCESSOR, DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::string_prototype::code_units;
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;
use crate::{custom_getter, custom_setter, host_function};

/// `"^$\\.*+?()[]{}|/"_s`: os `SyntaxCharacter` e `/`.
const SYNTAX_CHARACTERS: &str = "^$\\.*+?()[]{}|/";

/// `",-=<>#&!%:;@~'`\""_s`: os `otherPunctuators` de `EncodeForRegExpEscape`.
const OTHER_PUNCTUATORS: &str = ",-=<>#&!%:;@~'`\"";

/// `U16_IS_SURROGATE(c)`.
fn is_surrogate(code_point: u32) -> bool {
    (code_point & 0xFFFF_F800) == 0xD800
}

/// `StringView(characters).contains(codePoint)`.
fn contains(characters: &str, code_point: u32) -> bool {
    characters.chars().any(|character| u32::from(character) == code_point)
}

/// `isStrWhiteSpace(codePoint)`: o `char32_t` só importa até o BMP (nenhum espaço fica acima dele).
fn is_str_white_space_code_point(code_point: u32) -> bool {
    u16::try_from(code_point).is_ok_and(is_str_white_space::<u16>)
}

/// `regExpConstructorEscape` (https://tc39.es/proposal-regex-escaping/): `EncodeForRegExpEscape` sobre cada
/// ponto de código; letra ou dígito ASCII só é escapado (`\x..`) na primeira posição.
fn reg_exp_constructor_escape_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let value = call.argument(0);
    if !value.is_string() {
        return Err(Thrown::type_error("RegExp.escape requires a string"));
    }
    let string = value.as_js_string().value();
    let units = code_units(&string);

    let mut builder: Vec<u16> = Vec::with_capacity(units.len());
    let append_ascii = |builder: &mut Vec<u16>, text: &str| builder.extend(text.encode_utf16());
    let mut i = 0;
    while i < units.len() {
        // `U16_NEXT(characters, i, length, codePoint)`: um par bem formado vira um ponto de código.
        let unit = u32::from(units[i]);
        i += 1;
        let code_point = match units.get(i) {
            Some(&trail) if (0xD800..0xDC00).contains(&unit) && (0xDC00..0xE000).contains(&trail) => {
                i += 1;
                0x10000 + ((unit - 0xD800) << 10) + (u32::from(trail) - 0xDC00)
            }
            _ => unit,
        };

        if builder.is_empty() && code_point < 0x80 && (code_point as u8).is_ascii_alphanumeric() {
            append_ascii(&mut builder, &format!("\\x{code_point:x}"));
            continue;
        }

        if contains(SYNTAX_CHARACTERS, code_point) {
            builder.push(u16::from(b'\\'));
            builder.push(code_point as u16);
            continue;
        }

        let control = match code_point {
            0x09 => Some('t'),
            0x0A => Some('n'),
            0x0B => Some('v'),
            0x0C => Some('f'),
            0x0D => Some('r'),
            _ => None,
        };
        if let Some(letter) = control {
            append_ascii(&mut builder, &format!("\\{letter}"));
            continue;
        }

        if contains(OTHER_PUNCTUATORS, code_point) || is_str_white_space_code_point(code_point) || is_surrogate(code_point) {
            if code_point <= 0xFF {
                append_ascii(&mut builder, &format!("\\x{code_point:02x}"));
            } else if code_point <= 0xFFFF {
                append_ascii(&mut builder, &format!("\\u{code_point:04x}"));
            } else {
                let offset = code_point - 0x10000;
                let lead = 0xD800 + (offset >> 10);
                let trail = 0xDC00 + (offset & 0x3FF);
                append_ascii(&mut builder, &format!("\\u{lead:04x}\\u{trail:04x}"));
            }
            continue;
        }

        if code_point <= 0xFFFF {
            builder.push(code_point as u16);
        } else {
            let offset = code_point - 0x10000;
            builder.push((0xD800 + (offset >> 10)) as u16);
            builder.push((0xDC00 + (offset & 0x3FF)) as u16);
        }
    }

    Ok(JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf16(&builder))))
}
host_function!(reg_exp_constructor_escape, reg_exp_constructor_escape_body);

/// A guarda de todo acessor: `JSValue::decode(thisValue) != globalObject->regExpConstructor()` lança o
/// `TypeError` com a mensagem do acessor.
fn require_reg_exp_constructor(global_object: &JSGlobalObject, this_value: JSValue, message: &str) -> Result<(), Thrown> {
    if this_value != global_object.reg_exp_constructor() {
        return Err(Thrown::type_error(message));
    }
    Ok(())
}

/// Define um acessor de leitura que só confere o `this` e devolve `$value`.
macro_rules! legacy_getter {
    ($host:ident, $body:ident, $message:literal, |$global:ident| $value:expr) => {
        fn $body($global: &JSGlobalObject, this_value: JSValue, _property_name: &PropertyName) -> HostResult {
            require_reg_exp_constructor($global, this_value, $message)?;
            Ok($value)
        }
        custom_getter!($host, $body);
    };
}

/// `regExpConstructorDollar`: `N` é o segundo caractere do nome (`$1` a `$9`).
fn reg_exp_constructor_dollar_body(global_object: &JSGlobalObject, this_value: JSValue, property_name: &PropertyName) -> HostResult {
    require_reg_exp_constructor(global_object, this_value, "RegExp.$N getters require RegExp constructor as |this|")?;
    let uid = property_name.uid().expect("o acessor $N é instalado sob um nome");
    let n = u32::from(uid.0.char_at(1)) - u32::from(b'0');
    debug_assert!((1..=9).contains(&n));
    Ok(global_object.reg_exp_global_data().get_backref(global_object, n))
}
custom_getter!(reg_exp_constructor_dollar, reg_exp_constructor_dollar_body);

/// `regExpConstructorInput`.
fn reg_exp_constructor_input_body(global_object: &JSGlobalObject, this_value: JSValue, _property_name: &PropertyName) -> HostResult {
    require_reg_exp_constructor(global_object, this_value, "RegExp.input getter requires RegExp constructor as |this|")?;
    Ok(match global_object.reg_exp_global_data().input() {
        Some(input) => JSValue::from_js_string(input),
        None => JSValue::from_js_string(js_empty_string(global_object.vm())),
    })
}
custom_getter!(reg_exp_constructor_input, reg_exp_constructor_input_body);

legacy_getter!(
    reg_exp_constructor_multiline,
    reg_exp_constructor_multiline_body,
    "RegExp.multiline getter require RegExp constructor as |this|",
    |global_object| js_boolean(global_object.reg_exp_global_data().multiline())
);
legacy_getter!(
    reg_exp_constructor_last_match,
    reg_exp_constructor_last_match_body,
    "RegExp.lastMatch getter require RegExp constructor as |this|",
    |global_object| global_object.reg_exp_global_data().get_backref(global_object, 0)
);
legacy_getter!(
    reg_exp_constructor_last_paren,
    reg_exp_constructor_last_paren_body,
    "RegExp.lastParen getter require RegExp constructor as |this|",
    |global_object| global_object.reg_exp_global_data().get_last_paren(global_object)
);
legacy_getter!(
    reg_exp_constructor_left_context,
    reg_exp_constructor_left_context_body,
    "RegExp.leftContext getter require RegExp constructor as |this|",
    |global_object| global_object.reg_exp_global_data().get_left_context(global_object)
);
legacy_getter!(
    reg_exp_constructor_right_context,
    reg_exp_constructor_right_context_body,
    "RegExp.rightContext getter require RegExp constructor as |this|",
    |global_object| global_object.reg_exp_global_data().get_right_context(global_object)
);

/// `setRegExpConstructorInput`: `ToString(value)` e `setInput`.
fn set_reg_exp_constructor_input_body(
    global_object: &JSGlobalObject,
    this_value: JSValue,
    value: JSValue,
    _property_name: &PropertyName,
) -> Result<bool, Thrown> {
    require_reg_exp_constructor(global_object, this_value, "RegExp.input setters require RegExp constructor as |this|")?;
    let string = value.to_string(global_object.vm());
    pending_or(global_object, ())?;
    global_object.reg_exp_global_data().set_input(global_object, string);
    Ok(true)
}
custom_setter!(set_reg_exp_constructor_input, set_reg_exp_constructor_input_body);

/// `setRegExpConstructorMultiline`: `ToBoolean(value)` e `setMultiline`.
fn set_reg_exp_constructor_multiline_body(
    global_object: &JSGlobalObject,
    this_value: JSValue,
    value: JSValue,
    _property_name: &PropertyName,
) -> Result<bool, Thrown> {
    require_reg_exp_constructor(global_object, this_value, "RegExp.multiline setters require RegExp constructor as |this|")?;
    global_object.reg_exp_global_data().set_multiline(value.to_boolean());
    Ok(true)
}
custom_setter!(set_reg_exp_constructor_multiline, set_reg_exp_constructor_multiline_body);

/// `JSC_NATIVE_FUNCTION_WITHOUT_TRANSITION("escape"_s, regExpConstructorEscape, DontEnum, 1, Public)`.
pub fn install_escape(vm: &VM, global_object: &JSGlobalObject, constructor: &JSObject) {
    put_native_function(
        vm,
        global_object,
        constructor,
        &Identifier::from_span(vm, b"escape"),
        1,
        reg_exp_constructor_escape,
        Intrinsic::NoIntrinsic,
    );
}

/// `regExpConstructorTableValues` de `RegExpConstructor.lut.h`, na ordem do `@begin`.
static REG_EXP_CONSTRUCTOR_TABLE_VALUES: [HashTableValue; 21] = [
    custom_accessor_entry("input", reg_exp_constructor_input, set_reg_exp_constructor_input),
    custom_accessor_entry("$_", reg_exp_constructor_input, set_reg_exp_constructor_input),
    custom_accessor_entry("multiline", reg_exp_constructor_multiline, set_reg_exp_constructor_multiline),
    custom_accessor_entry("$*", reg_exp_constructor_multiline, set_reg_exp_constructor_multiline),
    custom_getter_entry("lastMatch", reg_exp_constructor_last_match),
    custom_getter_entry("$&", reg_exp_constructor_last_match),
    custom_getter_entry("lastParen", reg_exp_constructor_last_paren),
    custom_getter_entry("$+", reg_exp_constructor_last_paren),
    custom_getter_entry("leftContext", reg_exp_constructor_left_context),
    custom_getter_entry("$`", reg_exp_constructor_left_context),
    custom_getter_entry("rightContext", reg_exp_constructor_right_context),
    custom_getter_entry("$'", reg_exp_constructor_right_context),
    custom_getter_entry("$1", reg_exp_constructor_dollar),
    custom_getter_entry("$2", reg_exp_constructor_dollar),
    custom_getter_entry("$3", reg_exp_constructor_dollar),
    custom_getter_entry("$4", reg_exp_constructor_dollar),
    custom_getter_entry("$5", reg_exp_constructor_dollar),
    custom_getter_entry("$6", reg_exp_constructor_dollar),
    custom_getter_entry("$7", reg_exp_constructor_dollar),
    custom_getter_entry("$8", reg_exp_constructor_dollar),
    custom_getter_entry("$9", reg_exp_constructor_dollar),
];

/// `regExpConstructorTable`.
pub static REG_EXP_CONSTRUCTOR_TABLE: HashTable = HashTable { class_for_this: None, values: &REG_EXP_CONSTRUCTOR_TABLE_VALUES };
