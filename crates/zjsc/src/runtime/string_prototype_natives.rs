//! As cascas finas das funções nativas de `StringPrototype.cpp` (`JSC_DEFINE_HOST_FUNCTION`) e a tabela de
//! `StringPrototype::finishCreation`: cada casca lê `thisValue` e `argument(n)` do `NativeCallFrame`,
//! converte (`toString`, `toIntegerOrInfinity`, `toUInt32`, `toLength`), chama o algoritmo puro de
//! `string_prototype.rs` e transforma o erro em exceção pendente.
//!
//! LACUNAS, e por quê (ausentes em vez de falsas):
//! - `replace`, `replaceAll`, `match`, `matchAll`, `search`, `split` (com `RegExp` e `@@split`),
//!   `normalize`, os métodos HTML (`anchor`, `big`, ...) e `[Symbol.iterator]` estão em
//!   `string_prototype_natives_part2.rs` (`normalize` só responde pelos casos que o C++ resolve antes do
//!   ICU, ver lá); a tabela abaixo instala os não HTML na ordem do C++ (os HTML são `stringPrototypeTable`).
//! - `toLocaleLowerCase`/`toLocaleUpperCase` passam por `intl_case_mapping.rs` (locales `az`, `el`, `lt` e `tr`) e `localeCompare` por `intl_collator.rs`.
//!
//! DIVERGÊNCIAS:
//! - `thisValue.toString` e as conversões dos argumentos conferem a exceção pendente a cada passo
//!   (`RETURN_IF_EXCEPTION`): um `valueOf` que lança interrompe o corpo antes da próxima conversão. Um
//!   `StringObject` também passa pelo `toPrimitive`; só `to_wtf_string_or_type_error` (usado por outros
//!   arquivos) ainda lê o valor interno direto.
//! - `isRegExp(argument)` olha `@@match` do objeto e, sem ele, se é um `RegExpObject`.
//! - `substring` é a mesma função que o C++ guarda em `globalObject->stringProtoSubstringFunction()` (um
//!   `LazyProperty` que o global do porte não tem): criada aqui, na posição certa da tabela.

use crate::interpreter::call_frame::NativeCallFrame;
use crate::runtime::error::{create_range_error, create_type_error};
use crate::runtime::exception_helpers::throw_out_of_memory_error;
use crate::runtime::host_function_support::{throw_vm_type_error, ObjectRef};
use crate::runtime::host_call::{throw_thrown, Thrown};
use crate::runtime::intl_case_mapping::to_locale_case;
use crate::runtime::intl_collator::locale_compare;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{put_direct_native_function_without_transition, JSFunction};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_boolean, js_number, js_undefined, pnan, EncodedJSValue, JSValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::reg_exp_object::RegExpObject;
use crate::runtime::string_object::{StringObject, StringObjectRef};
use crate::runtime::string_prototype_natives_part2 as part2;
use crate::runtime::string_regexp_support::{check_exception, to_wtf_string_value};
use crate::runtime::string_prototype as algo;
use crate::runtime::string_prototype::StringOpError;
use crate::runtime::symbol::Symbol;
use crate::runtime::throw_scope::{throw_exception, throw_vm_exception, ThrowScope};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// O que uma casca devolve no lugar de lançar.
enum StringError {
    Op(StringOpError),
    Type(&'static str),
    /// A exceção de uma chamada ao `Intl` (`localeCompare`, `toLocaleUpperCase`).
    Thrown(Thrown),
}

impl From<StringOpError> for StringError {
    fn from(error: StringOpError) -> StringError {
        StringError::Op(error)
    }
}

/// O que o C++ lê do `CallFrame`.
struct StringCall<'a> {
    global_object: &'a JSGlobalObject,
    args: &'a [JSValue],
}

impl StringCall<'_> {
    /// `callFrame->argument(i)`.
    fn argument(&self, i: usize) -> JSValue {
        self.args.get(i).copied().unwrap_or_else(js_undefined)
    }

    /// `argument(i).toIntegerOrInfinity(globalObject)` e o `RETURN_IF_EXCEPTION`: um `valueOf` que lança
    /// interrompe o corpo antes de qualquer outra conversão.
    fn integer(&self, i: usize) -> Result<f64, StringError> {
        let integer = self.argument(i).to_integer_or_infinity();
        self.check_exception()?;
        Ok(integer)
    }

    /// `undefined` é `None`, senão `toIntegerOrInfinity`.
    fn optional_integer(&self, i: usize) -> Result<Option<f64>, StringError> {
        if self.argument(i).is_undefined() { Ok(None) } else { self.integer(i).map(Some) }
    }

    /// `argument(i).toString()` como texto.
    fn string(&self, i: usize) -> Result<WtfString, StringError> {
        value_to_wtf_string(self.global_object, &self.argument(i))
    }

    /// `RETURN_IF_EXCEPTION(scope, ...)`.
    fn check_exception(&self) -> Result<(), StringError> {
        check_exception(self.global_object).map_err(StringError::Thrown)
    }

    /// `argument(i).toLength(globalObject)`: `ToIntegerOrInfinity` limitado a `[0, 2^53 - 1]`.
    fn length(&self, i: usize) -> Result<f64, StringError> {
        let integer = self.integer(i)?;
        Ok(if integer <= 0.0 { 0.0 } else { integer.min(9_007_199_254_740_991.0) })
    }

    /// `toIntegerPreserveNaN` com o `NaN` trocado por `+Infinity` (`lastIndexOf`).
    fn position_or_infinity(&self, i: usize) -> Result<f64, StringError> {
        let number = self.argument(i).to_number();
        self.check_exception()?;
        Ok(if number.is_nan() { f64::INFINITY } else { number.trunc() })
    }
}

/// `value.toString(globalObject)` como texto, com o `RETURN_IF_EXCEPTION`: um `Symbol` ou um `toString`
/// que lança deixa a exceção pendente e o corpo não segue. Um `StringObject` também passa pelo
/// `toPrimitive` (o `toString` do programa vale), como em `JSValue::toString`.
fn value_to_wtf_string(global_object: &JSGlobalObject, value: &JSValue) -> Result<WtfString, StringError> {
    to_wtf_string_value(global_object, *value).map_err(StringError::Thrown)
}

/// `value.toString(globalObject)` como texto: `Symbol` é o `TypeError` (a mensagem é o `Err`). Para quem
/// ainda não tem como carregar a exceção pendente (`error_natives`, `js_module_loader`...).
pub fn to_wtf_string_or_type_error(value: &JSValue) -> Result<WtfString, &'static str> {
    if value.is_cell() && !value.is_string() {
        if Symbol::from_cell_id(value.as_cell()).is_some() {
            return Err("Cannot convert a symbol to a string");
        }
        if let Some(object) = StringObject::from_cell_id(value.as_cell()) {
            return Ok(object.internal_value().value());
        }
    }
    Ok(value.to_wtf_string())
}

/// `isRegExp(vm, globalObject, value)`.
pub fn is_reg_exp(global_object: &JSGlobalObject, value: &JSValue) -> bool {
    if !value.is_object() {
        return false;
    }
    let vm = global_object.vm();
    if let Some(object) = ObjectRef::from_value(value) {
        let matcher = object.get(global_object, &PropertyName::from_identifier(&vm.property_names.match_symbol));
        if vm.exception().is_some() {
            return false;
        }
        if !matcher.is_undefined() {
            return matcher.to_boolean();
        }
    }
    value.is_cell() && RegExpObject::from_cell_id(value.as_cell()).is_some()
}

type StringBody = fn(&StringCall, &WtfString) -> Result<JSValue, StringError>;

/// Lança o erro como exceção pendente.
fn throw_string_error(global_object: &JSGlobalObject, error: StringError) {
    let mut scope = ThrowScope::new(global_object.vm());
    match error {
        StringError::Op(StringOpError::RangeError(message)) => {
            let message = WtfString::from_utf8(message.as_bytes());
            throw_vm_exception(global_object, &mut scope, create_range_error(global_object, &message));
        }
        StringError::Op(StringOpError::OutOfMemory) => {
            throw_out_of_memory_error(global_object, &mut scope);
        }
        StringError::Thrown(thrown) => throw_thrown(global_object, thrown),
        StringError::Type(message) => {
            let message = WtfString::from_utf8(message.as_bytes());
            throw_exception(global_object, &mut scope, create_type_error(global_object, &message));
        }
    }
}

/// `RangeError` ou `OutOfMemoryError` de um algoritmo de string como exceção pendente.
pub fn throw_string_op_error(global_object: &JSGlobalObject, error: StringOpError) {
    throw_string_error(global_object, StringError::Op(error));
}

/// O invólucro `JSC_DEFINE_HOST_FUNCTION`: `checkObjectCoercible(thisValue)` (senão `throwVMTypeError`,
/// com `message` quando o C++ dá uma), `thisValue.toString`, o corpo e a conversão do resultado.
fn run_string_function(
    global_object: &JSGlobalObject,
    call_frame: &NativeCallFrame<'_>,
    message: Option<&'static str>,
    body: StringBody,
) -> EncodedJSValue {
    let this_value = call_frame.this_value();
    if this_value.is_undefined_or_null() {
        return throw_vm_type_error(global_object, message);
    }
    let args = call_frame.arguments_span();
    let call = StringCall { global_object, args: &args };
    let result = value_to_wtf_string(global_object, &this_value).and_then(|this_string| body(&call, &this_string));
    match result {
        Ok(value) => value.encode(),
        Err(error) => {
            throw_string_error(global_object, error);
            JSValue::empty().encode()
        }
    }
}

fn string_value(call: &StringCall, string: &WtfString) -> Result<JSValue, StringError> {
    Ok(JSValue::from_js_string(js_string(call.global_object.vm(), string)))
}

/// Define a casca `NativeFunction` de um corpo; o segundo caso leva a mensagem de `this` vazio.
macro_rules! string_host_function {
    ($wrapper:ident, $body:path) => {
        pub(crate) fn $wrapper(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
            run_string_function(global_object, call_frame, None, $body)
        }
    };
    ($wrapper:ident, $body:path, $message:expr) => {
        fn $wrapper(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
            run_string_function(global_object, call_frame, Some($message), $body)
        }
    };
}

// ------------------------------ Corpos --------------------------

fn body_char_at(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    string_value(call, &algo::char_at(this, call.integer(0)?))
}

fn body_char_code_at(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    Ok(js_number(algo::char_code_at(this, call.integer(0)?).map_or_else(pnan, f64::from)))
}

fn body_code_point_at(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    Ok(algo::code_point_at(this, call.integer(0)?).map_or_else(js_undefined, |code_point| js_number(f64::from(code_point))))
}

fn body_concat(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    let mut parts = vec![this.clone()];
    for i in 0..call.args.len() {
        parts.push(call.string(i)?);
    }
    string_value(call, &algo::concat(&parts)?)
}

fn body_index_of(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    let search = call.string(0)?;
    Ok(js_number(algo::index_of(this, &search, call.integer(1)?)))
}

fn body_last_index_of(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    let search = call.string(0)?;
    Ok(js_number(algo::last_index_of(this, &search, call.position_or_infinity(1)?)))
}

fn body_repeat(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    let count = call.integer(0)?;
    // O V8 diz `Invalid count value: -1.5` com o argumento como veio (medido no Chromium 154).
    if (count < 0.0 || count.is_infinite()) && crate::runtime::stack_frame::v8_stack_format() {
        let argument = call.argument(0);
        let shown = if argument.is_number() { argument.as_number() } else { count };
        return Err(StringError::Thrown(Thrown::range_error(&format!(
            "Invalid count value: {}",
            crate::runtime::v8_stack::number_text(shown)
        ))));
    }
    string_value(call, &algo::repeat(this, count)?)
}

fn pad_with(call: &StringCall, this: &WtfString, at_start: bool) -> Result<JSValue, StringError> {
    let max_length = call.length(0)?;
    // `if (maxLengthDouble <= stringLength) return thisString;` vem antes de qualquer conversão do
    // preenchimento: um `toString` do `fillString` nem chega a rodar.
    if max_length <= this.length() as f64 {
        return string_value(call, this);
    }
    let fill = if call.argument(1).is_undefined() { WtfString::from_latin1(b" ") } else { call.string(1)? };
    string_value(call, &algo::pad(this, max_length, &fill, at_start)?)
}

fn body_pad_start(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    pad_with(call, this, true)
}

fn body_pad_end(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    pad_with(call, this, false)
}

fn body_slice(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    let (start, end) = (call.integer(0)?, call.optional_integer(1)?);
    string_value(call, &algo::slice(this, start, end))
}

fn body_substring(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    let (start, end) = (call.integer(0)?, call.optional_integer(1)?);
    string_value(call, &algo::substring(this, start, end))
}

fn body_substr(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    let (start, length) = (call.integer(0)?, call.optional_integer(1)?);
    string_value(call, &algo::substr(this, start, length))
}

fn body_at(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    match algo::at(this, call.integer(0)?) {
        Some(character) => string_value(call, &character),
        None => Ok(js_undefined()),
    }
}

fn body_to_lower_case(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    string_value(call, &algo::to_lower_case(this))
}

fn body_to_upper_case(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    string_value(call, &algo::to_upper_case(this))
}

fn body_locale_compare(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    let that = call.string(0)?;
    let result = locale_compare(call.global_object, this, &that, call.argument(1), call.argument(2))
        .map_err(StringError::Thrown)?;
    Ok(js_number(result))
}

fn body_to_locale_lower_case(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    let converted = to_locale_case(call.global_object, this, call.argument(0), false).map_err(StringError::Thrown)?;
    string_value(call, &converted)
}

fn body_to_locale_upper_case(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    let converted = to_locale_case(call.global_object, this, call.argument(0), true).map_err(StringError::Thrown)?;
    string_value(call, &converted)
}

fn body_trim(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    string_value(call, &algo::trim(this))
}

fn body_trim_start(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    string_value(call, &algo::trim_start(this))
}

fn body_trim_end(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    string_value(call, &algo::trim_end(this))
}

/// O `isRegExp` e o `toString` do primeiro argumento de `startsWith`, `endsWith` e `includes`.
fn search_string(call: &StringCall, message: &'static str) -> Result<WtfString, StringError> {
    let is_regexp = is_reg_exp(call.global_object, &call.argument(0));
    call.check_exception()?;
    if is_regexp {
        return Err(StringError::Type(message));
    }
    call.string(0)
}

fn body_starts_with(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    let search = search_string(call, "Argument to String.prototype.startsWith cannot be a RegExp")?;
    Ok(js_boolean(algo::starts_with(this, &search, call.integer(1)?)))
}

fn body_ends_with(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    let search = search_string(call, "Argument to String.prototype.endsWith cannot be a RegExp")?;
    Ok(js_boolean(algo::ends_with(this, &search, call.optional_integer(1)?)))
}

fn body_includes(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    let search = search_string(call, "Argument to String.prototype.includes cannot be a RegExp")?;
    Ok(js_boolean(algo::includes(this, &search, call.integer(1)?)))
}

fn body_is_well_formed(_call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    Ok(js_boolean(algo::is_well_formed(this)))
}

fn body_to_well_formed(call: &StringCall, this: &WtfString) -> Result<JSValue, StringError> {
    string_value(call, &algo::to_well_formed(this))
}

string_host_function!(string_proto_func_char_at, body_char_at);
string_host_function!(string_proto_func_char_code_at, body_char_code_at);
string_host_function!(string_proto_func_code_point_at, body_code_point_at);
string_host_function!(
    string_proto_func_concat,
    body_concat,
    "String.prototype.concat requires that |this| not be null or undefined"
);
string_host_function!(string_proto_func_index_of, body_index_of);
string_host_function!(string_proto_func_last_index_of, body_last_index_of);
string_host_function!(
    string_proto_func_repeat,
    body_repeat,
    "String.prototype.repeat requires that |this| not be null or undefined"
);
string_host_function!(
    string_proto_func_pad_start,
    body_pad_start,
    "String.prototype.padStart requires that |this| not be null or undefined"
);
string_host_function!(
    string_proto_func_pad_end,
    body_pad_end,
    "String.prototype.padEnd requires that |this| not be null or undefined"
);
string_host_function!(string_proto_func_slice, body_slice);
string_host_function!(string_proto_func_substring, body_substring);
string_host_function!(string_proto_func_substr, body_substr);
string_host_function!(string_proto_func_at, body_at);
string_host_function!(string_proto_func_to_lower_case, body_to_lower_case);
string_host_function!(string_proto_func_to_upper_case, body_to_upper_case);
string_host_function!(
    string_proto_func_locale_compare,
    body_locale_compare,
    "String.prototype.localeCompare requires that |this| not be null or undefined"
);
string_host_function!(string_proto_func_to_locale_lower_case, body_to_locale_lower_case);
string_host_function!(string_proto_func_to_locale_upper_case, body_to_locale_upper_case);
string_host_function!(string_proto_func_trim, body_trim);
string_host_function!(string_proto_func_trim_start, body_trim_start);
string_host_function!(string_proto_func_trim_end, body_trim_end);
string_host_function!(string_proto_func_starts_with, body_starts_with);
string_host_function!(string_proto_func_ends_with, body_ends_with);
string_host_function!(string_proto_func_includes, body_includes);
string_host_function!(string_proto_func_is_well_formed, body_is_well_formed);
string_host_function!(string_proto_func_to_well_formed, body_to_well_formed);

/// `stringProtoFuncToString` (também `valueOf`): `this` string, ou `StringObject` com o valor interno.
fn string_proto_func_to_string(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let this_value = call_frame.this_value();
    if this_value.is_string() {
        return this_value.encode();
    }
    if this_value.is_cell() {
        if let Some(string_object) = StringObject::from_cell_id(this_value.as_cell()) {
            return JSValue::from_js_string(string_object.internal_value()).encode();
        }
    }
    throw_vm_type_error(global_object, None)
}

/// A tabela de `StringPrototype::finishCreation` (na ordem do C++, menos o que está nas LACUNAS do
/// cabeçalho): `Base::finishCreation` já foi feito por `StringObject::create`.
pub fn add_string_prototype_properties(prototype: &StringObjectRef, vm: &VM, global_object: &JSGlobalObject) {
    let names = &vm.property_names;
    let builtin_names = names.builtin_names();
    let literal = |text: &[u8]| Identifier::from_span(vm, text);
    let define = |name: &Identifier, length: u32, function: NativeFunction, intrinsic: Intrinsic| {
        put_direct_native_function_without_transition(
            vm,
            global_object,
            prototype,
            name,
            length,
            function,
            ImplementationVisibility::Public,
            intrinsic,
            DONT_ENUM,
        )
    };

    // `stringPrototypeTable` (os 13 métodos HTML) está em `string_prototype.rs` e reifica no primeiro acesso.
    define(&names.to_string, 0, string_proto_func_to_string, Intrinsic::StringPrototypeValueOfIntrinsic);
    define(&names.value_of, 0, string_proto_func_to_string, Intrinsic::StringPrototypeValueOfIntrinsic);
    define(&literal(b"charAt"), 1, string_proto_func_char_at, Intrinsic::CharAtIntrinsic);
    define(&literal(b"charCodeAt"), 1, string_proto_func_char_code_at, Intrinsic::CharCodeAtIntrinsic);
    define(&literal(b"codePointAt"), 1, string_proto_func_code_point_at, Intrinsic::StringPrototypeCodePointAtIntrinsic);
    define(&literal(b"concat"), 1, string_proto_func_concat, Intrinsic::StringPrototypeConcatIntrinsic);
    define(builtin_names.index_of_public_name(), 1, string_proto_func_index_of, Intrinsic::StringPrototypeIndexOfIntrinsic);
    define(&literal(b"lastIndexOf"), 1, string_proto_func_last_index_of, Intrinsic::StringPrototypeLastIndexOfIntrinsic);
    define(&literal(b"replace"), 2, part2::string_proto_func_replace, Intrinsic::StringPrototypeReplaceIntrinsic);
    define(&literal(b"replaceAll"), 2, part2::string_proto_func_replace_all, Intrinsic::StringPrototypeReplaceAllIntrinsic);
    define(&literal(b"repeat"), 1, string_proto_func_repeat, Intrinsic::NoIntrinsic);
    define(&literal(b"padStart"), 1, string_proto_func_pad_start, Intrinsic::NoIntrinsic);
    define(&literal(b"padEnd"), 1, string_proto_func_pad_end, Intrinsic::NoIntrinsic);
    define(&literal(b"slice"), 2, string_proto_func_slice, Intrinsic::StringPrototypeSliceIntrinsic);
    define(&literal(b"substr"), 2, string_proto_func_substr, Intrinsic::StringPrototypeSubstrIntrinsic);
    define(&literal(b"at"), 1, string_proto_func_at, Intrinsic::StringPrototypeAtIntrinsic);
    // `globalObject->stringProtoSubstringFunction()`: a `JSFunction` de `stringProtoFuncSubstring`.
    define(&literal(b"substring"), 2, string_proto_func_substring, Intrinsic::StringPrototypeSubstringIntrinsic);
    define(&literal(b"toLowerCase"), 0, string_proto_func_to_lower_case, Intrinsic::StringPrototypeToLowerCaseIntrinsic);
    define(&literal(b"toUpperCase"), 0, string_proto_func_to_upper_case, Intrinsic::StringPrototypeToUpperCaseIntrinsic);
    define(&literal(b"localeCompare"), 1, string_proto_func_locale_compare, Intrinsic::StringPrototypeLocaleCompareIntrinsic);
    define(&literal(b"toLocaleLowerCase"), 0, string_proto_func_to_locale_lower_case, Intrinsic::NoIntrinsic);
    define(&literal(b"toLocaleUpperCase"), 0, string_proto_func_to_locale_upper_case, Intrinsic::NoIntrinsic);
    define(&literal(b"trim"), 0, string_proto_func_trim, Intrinsic::StringPrototypeTrimIntrinsic);
    define(&literal(b"startsWith"), 1, string_proto_func_starts_with, Intrinsic::StringPrototypeStartsWithIntrinsic);
    define(&literal(b"endsWith"), 1, string_proto_func_ends_with, Intrinsic::StringPrototypeEndsWithIntrinsic);
    define(&literal(b"includes"), 1, string_proto_func_includes, Intrinsic::StringPrototypeIncludesIntrinsic);
    define(&literal(b"match"), 1, part2::string_proto_func_match, Intrinsic::StringPrototypeMatchIntrinsic);
    define(&literal(b"search"), 1, part2::string_proto_func_search, Intrinsic::StringPrototypeSearchIntrinsic);
    define(&literal(b"matchAll"), 1, part2::string_proto_func_match_all, Intrinsic::NoIntrinsic);
    define(&literal(b"split"), 2, part2::string_proto_func_split, Intrinsic::StringPrototypeSplitIntrinsic);
    define(&literal(b"normalize"), 0, part2::string_proto_func_normalize, Intrinsic::NoIntrinsic);
    define(&builtin_names.char_code_at_private_name(), 1, string_proto_func_char_code_at, Intrinsic::CharCodeAtIntrinsic);

    let trim_start = JSFunction::create_native(
        vm,
        global_object,
        0,
        &WtfString::from_latin1(b"trimStart"),
        string_proto_func_trim_start,
        ImplementationVisibility::Public,
        Intrinsic::StringPrototypeTrimStartIntrinsic,
        crate::runtime::js_function::call_host_function_as_constructor,
    );
    let trim_end = JSFunction::create_native(
        vm,
        global_object,
        0,
        &WtfString::from_latin1(b"trimEnd"),
        string_proto_func_trim_end,
        ImplementationVisibility::Public,
        Intrinsic::StringPrototypeTrimEndIntrinsic,
        crate::runtime::js_function::call_host_function_as_constructor,
    );
    let put = |name: &[u8], function: &crate::runtime::js_function::JSFunctionRef| {
        prototype.put_direct_without_transition(vm, &PropertyName::from_identifier(&literal(name)), function.as_value(), DONT_ENUM);
    };
    put(b"trimStart", &trim_start);
    put(b"trimLeft", &trim_start);
    put(b"trimEnd", &trim_end);
    put(b"trimRight", &trim_end);

    // `globalObject->stringProtoSymbolIteratorFunction()`: `[Symbol.iterator]` (`JSStringIteratorIntrinsic`).
    let iterator_function = JSFunction::create_native(
        vm,
        global_object,
        0,
        &WtfString::from_latin1(b"[Symbol.iterator]"),
        part2::string_proto_func_iterator,
        ImplementationVisibility::Public,
        Intrinsic::JSStringIteratorIntrinsic,
        crate::runtime::js_function::call_host_function_as_constructor,
    );
    // A propriedade `[Symbol.iterator]` só entra depois do `constructor` (ordem de `Reflect.ownKeys`
    // do bun), em `StringConstructor::create`.
    global_object.iteration_protocol.borrow_mut().string_proto_symbol_iterator_function = Some(iterator_function.as_value());

    define(&builtin_names.substr_private_name(), 2, string_proto_func_substr, Intrinsic::StringPrototypeSubstrIntrinsic);
    define(&builtin_names.ends_with_private_name(), 2, string_proto_func_ends_with, Intrinsic::StringPrototypeEndsWithIntrinsic);
    define(&names.is_well_formed, 0, string_proto_func_is_well_formed, Intrinsic::NoIntrinsic);
    define(&names.to_well_formed, 0, string_proto_func_to_well_formed, Intrinsic::NoIntrinsic);

    // "The constructor will be added later, after StringConstructor has been built" (e, depois dele,
    // `[Symbol.iterator]`, ver `StringConstructor::create`).
}
