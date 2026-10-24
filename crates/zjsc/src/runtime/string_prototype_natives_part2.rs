//! A segunda metade das funções nativas de `StringPrototype.cpp`: as que dependem de `RegExp`
//! (`replace`, `replaceAll`, `match`, `matchAll`, `search`, `split`), de ICU (`normalize`), de
//! `createHTML` (`anchor`, `big`, `blink`, `bold`, `fixed`, `fontcolor`, `fontsize`, `italics`, `link`,
//! `small`, `strike`, `sub`, `sup`) e do `JSStringIterator` (`[Symbol.iterator]`). A tabela de
//! `StringPrototype::finishCreation` está em `string_prototype_natives.rs`.
//!
//! Cada corpo é uma função comum sobre `HostCall` (ver `host_call.rs`); o `host_function!` gera o
//! `JSC_DEFINE_HOST_FUNCTION`.
//!
//! `normalize` usa `wtf::unicode::normalization` (crate `icu_normalizer` no lugar do `unorm2`).
//!
//! DIVERGÊNCIAS:
//! - Só o caminho da especificação para argumento objeto (`GetMethod(searchValue, @@replace)` e
//!   companhia): os atalhos de `RegExpObject` (`replaceUsingRegExpSearch`, `regExpMatchFast`,
//!   `regExpSearchFast`, `regExpSplitFast`, `isSymbol*FastAndNonObservable`) dependem de watchpoints que
//!   não existem e dão o mesmo resultado observável pela cadeia `RegExp.prototype[@@replace]` e afins
//!   (`reg_exp_prototype_natives.rs`).
//! - `replaceUsingStringSearch` e `stringReplaceStringString`/`stringReplaceAllStringString` trabalham
//!   sobre unidades UTF-16 e montam o resultado de uma vez (sem `BoyerMooreHorspoolTable`, ropes nem
//!   `CachedCall`); o `substituteBackreferencesSlow` é o do `reg == nullptr` (`$$`, `$&`, `` $` `` e `$'`).

use crate::host_function;
use crate::runtime::call_data::get_call_data;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::iterator_operations::call_checked;
use crate::runtime::js_array::construct_array;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_string::{js_string, js_substring, JSStringRef};
use crate::runtime::js_string_iterator::JSStringIterator;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::reg_exp_prototype_natives::{call_builtin_reg_exp_replace, reg_exp_create_from_value};
use crate::runtime::string_prototype as algo;
use crate::runtime::string_prototype::{code_units, find_units};
use crate::wtf::text::string_impl::MAX_LENGTH;
use crate::runtime::string_prototype_natives::is_reg_exp;
use crate::runtime::string_regexp_support::{
    check_exception, contains_unit, find_unit, get_object_property, not_a_function_error, string_value, to_string_value,
    to_uint32_value, to_wtf_string_value, units_value,
};
use crate::wtf::text::wtf_string::String as WtfString;
use crate::wtf::unicode::normalization::{normalize_utf16, NormalizationForm};
use crate::yarr::yarr_flags::{FlagSet, Flags};

/// `checkObjectCoercible(thisValue)`, senão o `TypeError` com a mensagem.
fn require_coercible(this_value: JSValue, message: &str) -> Result<(), Thrown> {
    if this_value.is_undefined_or_null() { Err(Thrown::type_error(message)) } else { Ok(()) }
}

/// `GetMethod(object, @@symbol)` e a chamada que `String.prototype.replace`, `replaceAll`, `match`,
/// `matchAll`, `search` e `split` fazem: `Some(resultado)` se o método existe (chamado com `object` como
/// `this`), `None` se é `undefined` ou `null`. `not_callable_message` é a mensagem fixa do `TypeError`
/// quando o método não é chamável; sem ela, `<descrição> is not a function`.
fn call_symbol_method(
    global_object: &JSGlobalObject,
    object: JSValue,
    symbol: &Identifier,
    args: &[JSValue],
    not_callable_message: Option<&str>,
) -> Result<Option<JSValue>, Thrown> {
    let method = get_object_property(global_object, object, symbol)?;
    if method.is_undefined_or_null() {
        return Ok(None);
    }
    if get_call_data(method).is_none() {
        return Err(match not_callable_message {
            Some(message) => Thrown::type_error(message),
            None => not_a_function_error(global_object, method),
        });
    }
    if let Some(result) = call_builtin_reg_exp_replace(global_object, method, object, args) {
        return Ok(Some(result?));
    }
    Ok(Some(call_checked(global_object, method, object, args, "Type error")?))
}

/// O `get` seguido de `getCallData` e `call` das variantes `...Slow` (`stringMatchSlow`, `stringSearchSlow`,
/// `stringMatchAllSlow`): o método do `RegExp` recém-criado tem de existir e ser chamável.
fn call_required_method(global_object: &JSGlobalObject, object: JSValue, symbol: &Identifier, args: &[JSValue]) -> HostResult {
    let method = get_object_property(global_object, object, symbol)?;
    if get_call_data(method).is_none() {
        return Err(not_a_function_error(global_object, method));
    }
    call_checked(global_object, method, object, args, "Type error")
}

/// `substituteBackreferencesSlow(result, replacement, source, ovector, nullptr, i)`: expande `$$`, `$&`,
/// `` $` `` e `$'` de `replacement` sobre o casamento `[match_start, match_end)` de `source`; qualquer
/// outro `$` fica como está (sem `RegExp` não há grupos).
fn substitute_backreferences(result: &mut Vec<u16>, replacement: &[u16], source: &[u16], match_start: usize, match_end: usize) {
    let Some(mut i) = find_unit(replacement, b'$', 0) else {
        result.extend_from_slice(replacement);
        return;
    };
    let mut offset = 0usize;
    loop {
        if i + 1 == replacement.len() {
            break;
        }
        let reference = replacement[i + 1];
        if reference == u16::from(b'$') {
            // "$$" -> "$"
            i += 1;
            result.extend_from_slice(&replacement[offset..i]);
            offset = i + 1;
        } else {
            let backreference = match u8::try_from(reference).unwrap_or(0) {
                b'&' => Some((match_start, match_end)),
                b'`' => Some((0, match_start)),
                b'\'' => Some((match_end, source.len())),
                _ => None,
            };
            if let Some((backref_start, backref_end)) = backreference {
                if i > offset {
                    result.extend_from_slice(&replacement[offset..i]);
                }
                i += 1;
                offset = i + 1;
                if backref_end >= backref_start {
                    result.extend_from_slice(&source[backref_start..backref_end]);
                }
            }
        }
        match find_unit(replacement, b'$', i + 1) {
            Some(next) => i = next,
            None => break,
        }
    }
    if replacement.len() > offset {
        result.extend_from_slice(&replacement[offset..]);
    }
}

/// `StringReplaceMode`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReplaceMode {
    Single,
    Global,
}

/// `replaceUsingStringSearch<mode>(vm, globalObject, jsString, string, searchString, replaceValue)`.
fn replace_using_string_search(
    global_object: &JSGlobalObject,
    mode: ReplaceMode,
    string: &JSStringRef,
    search: &WtfString,
    replace_value: JSValue,
) -> HostResult {
    let vm = global_object.vm();
    let source = code_units(&string.value()).into_owned();
    let search_units = code_units(search).into_owned();
    let functional = !replace_value.is_string() && !get_call_data(replace_value).is_none();
    // O frame de `replaceUsingStringSearch` que fica na pilha enquanto o callback roda.
    let _stack_frame = if functional {
        Some(vm.enter_logical_frame(crate::runtime::vm::stack_cost::REPLACE_CALLBACK).ok_or(Thrown::StackOverflow)?)
    } else {
        None
    };

    if !functional {
        // `stringReplaceStringString` / `stringReplaceAllStringString` com substituições.
        let replacement = code_units(&to_wtf_string_value(global_object, replace_value)?).into_owned();
        let mut starts = Vec::new();
        let mut from = 0usize;
        while let Some(start) = find_units(&source, &search_units, from) {
            starts.push(start);
            if mode == ReplaceMode::Single {
                break;
            }
            from = start + search_units.len();
            if search_units.is_empty() {
                from += 1;
            }
        }
        if starts.is_empty() {
            return Ok(string_value(string));
        }
        let mut result: Vec<u16> = Vec::new();
        let mut last_match_end = 0usize;
        for start in starts {
            result.extend_from_slice(&source[last_match_end..start]);
            last_match_end = start + search_units.len();
            substitute_backreferences(&mut result, &replacement, &source, start, last_match_end);
            // O `StringBuilder` do C++ marca overflow acima de `MaxLength` e o chamador lança OutOfMemoryError.
            if result.len() > MAX_LENGTH as usize {
                return Err(Thrown::OutOfMemory);
            }
        }
        result.extend_from_slice(&source[last_match_end..]);
        return Ok(units_value(vm, &result));
    }

    let Some(mut match_start) = find_units(&source, &search_units, 0) else {
        return Ok(string_value(string));
    };
    let mut end_of_last_match = 0usize;
    let mut result: Vec<u16> = Vec::new();
    loop {
        let substring = js_substring(vm, string, match_start as u32, search_units.len() as u32);
        let arguments = [JSValue::from_js_string(substring), js_number(match_start as f64), string_value(string)];
        let replacement_value = call_checked(global_object, replace_value, JSValue::undefined(), &arguments, "Type error")?;
        let replacement = to_wtf_string_value(global_object, replacement_value)?;

        result.extend_from_slice(&source[end_of_last_match..match_start]);
        result.extend_from_slice(&code_units(&replacement));
        end_of_last_match = match_start + search_units.len();
        if result.len() > MAX_LENGTH as usize {
            return Err(Thrown::OutOfMemory);
        }
        if mode == ReplaceMode::Single {
            break;
        }
        let from = if search_units.is_empty() { end_of_last_match + 1 } else { end_of_last_match };
        match find_units(&source, &search_units, from) {
            Some(next) => match_start = next,
            None => break,
        }
    }
    result.extend_from_slice(&source[end_of_last_match..]);
    Ok(units_value(vm, &result))
}

/// `stringProtoFuncReplace` (https://tc39.es/ecma262/#sec-string.prototype.replace).
fn string_replace(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let this_value = call.this_value();
    require_coercible(this_value, "String.prototype.replace requires that |this| not be null or undefined")?;

    let search_value = call.argument(0);
    let replace_value = call.argument(1);
    if search_value.is_object() {
        let arguments = [this_value, replace_value];
        let replacer = call_symbol_method(
            global_object,
            search_value,
            &vm.property_names.replace_symbol,
            &arguments,
            Some("@@replace method is not callable"),
        )?;
        if let Some(result) = replacer {
            return Ok(result);
        }
    }

    let string = to_string_value(global_object, this_value)?;
    let search_string = to_string_value(global_object, search_value)?;
    replace_using_string_search(global_object, ReplaceMode::Single, &string, &search_string.value(), replace_value)
}

/// `stringProtoFuncReplaceAll` (https://tc39.es/ecma262/#sec-string.prototype.replaceall).
fn string_replace_all(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let this_value = call.this_value();
    require_coercible(this_value, "String.prototype.replaceAll requires |this| not to be null nor undefined")?;

    let search_value = call.argument(0);
    let replace_value = call.argument(1);
    if search_value.is_object() {
        let search_value_is_reg_exp = is_reg_exp(global_object, &search_value);
        check_exception(global_object)?;
        if search_value_is_reg_exp {
            let flags_value = get_object_property(global_object, search_value, &vm.property_names.flags)?;
            let flags = to_wtf_string_value(global_object, flags_value)?;
            if !contains_unit(&flags, b'g') {
                return Err(Thrown::type_error("String.prototype.replaceAll argument must not be a non-global regular expression"));
            }
        }

        let arguments = [this_value, replace_value];
        let replacer = call_symbol_method(
            global_object,
            search_value,
            &vm.property_names.replace_symbol,
            &arguments,
            Some("@@replace method is not callable"),
        )?;
        if let Some(result) = replacer {
            return Ok(result);
        }
    }

    let string = to_string_value(global_object, this_value)?;
    let search_string = to_wtf_string_value(global_object, search_value)?;
    replace_using_string_search(global_object, ReplaceMode::Global, &string, &search_string, replace_value)
}

/// `stringMatchSlow(globalObject, thisString, regexpValue)`: `RegExpCreate(regexp, undefined)` e o `@@match` dele.
fn string_match_slow(global_object: &JSGlobalObject, this_string: &JSStringRef, regexp_value: JSValue) -> HostResult {
    let vm = global_object.vm();
    let reg_exp = reg_exp_create_from_value(global_object, regexp_value, FlagSet::empty())?;
    call_required_method(global_object, reg_exp, &vm.property_names.match_symbol, &[string_value(this_string)])
}

/// `stringProtoFuncMatch`.
fn string_match(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let this_value = call.this_value();
    require_coercible(this_value, "String.prototype.match requires that |this| not be null or undefined")?;

    let regexp_value = call.argument(0);
    if regexp_value.is_object() {
        let matcher = call_symbol_method(global_object, regexp_value, &vm.property_names.match_symbol, &[this_value], None)?;
        if let Some(result) = matcher {
            return Ok(result);
        }
    }

    let this_string = to_string_value(global_object, this_value)?;
    string_match_slow(global_object, &this_string, regexp_value)
}

/// `stringSearchSlow(globalObject, thisString, regexpValue)`.
fn string_search_slow(global_object: &JSGlobalObject, this_string: &JSStringRef, regexp_value: JSValue) -> HostResult {
    let vm = global_object.vm();
    let created_reg_exp = reg_exp_create_from_value(global_object, regexp_value, FlagSet::empty())?;
    call_required_method(global_object, created_reg_exp, &vm.property_names.search_symbol, &[string_value(this_string)])
}

/// `stringProtoFuncSearch`.
fn string_search(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let this_value = call.this_value();
    require_coercible(this_value, "String.prototype.search requires that |this| not be null or undefined")?;

    let regexp_value = call.argument(0);
    if regexp_value.is_object() {
        let searcher = call_symbol_method(global_object, regexp_value, &vm.property_names.search_symbol, &[this_value], None)?;
        if let Some(result) = searcher {
            return Ok(result);
        }
    }

    let this_string = to_string_value(global_object, this_value)?;
    string_search_slow(global_object, &this_string, regexp_value)
}

/// `stringMatchAllSlow(globalObject, thisString, regexpValue)`: o `RegExp` com a flag `g` e o `@@matchAll` dele.
fn string_match_all_slow(global_object: &JSGlobalObject, this_string: &JSStringRef, regexp_value: JSValue) -> HostResult {
    let vm = global_object.vm();
    let reg_exp = reg_exp_create_from_value(global_object, regexp_value, FlagSet::new(&[Flags::Global]))?;
    call_required_method(global_object, reg_exp, &vm.property_names.match_all_symbol, &[string_value(this_string)])
}

/// `stringProtoFuncMatchAll`.
fn string_match_all(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let this_value = call.this_value();
    require_coercible(this_value, "String.prototype.matchAll requires |this| not to be null nor undefined")?;

    let regexp_value = call.argument(0);
    if regexp_value.is_object() {
        let arg_is_reg_exp = is_reg_exp(global_object, &regexp_value);
        check_exception(global_object)?;
        if arg_is_reg_exp {
            let flags_value = get_object_property(global_object, regexp_value, &vm.property_names.flags)?;
            let flags = to_wtf_string_value(global_object, flags_value)?;
            if !contains_unit(&flags, b'g') {
                return Err(Thrown::type_error("String.prototype.matchAll argument must not be a non-global regular expression"));
            }
        }

        let matcher = call_symbol_method(global_object, regexp_value, &vm.property_names.match_all_symbol, &[this_value], None)?;
        if let Some(result) = matcher {
            return Ok(result);
        }
    }

    let this_string = to_string_value(global_object, this_value)?;
    string_match_all_slow(global_object, &this_string, regexp_value)
}

/// `stringProtoFuncSplit` (https://tc39.es/ecma262/#sec-string.prototype.split): `RegExp` e objeto com
/// `@@split` pelo método do separador, o resto pelo `algo::split`.
fn string_split(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let this_value = call.this_value();
    require_coercible(this_value, "String.prototype.split requires that |this| not be null or undefined")?;

    let separator_value = call.argument(0);
    let limit_value = call.argument(1);
    if separator_value.is_object() {
        let arguments = [this_value, limit_value];
        let splitter = call_symbol_method(
            global_object,
            separator_value,
            &vm.property_names.split_symbol,
            &arguments,
            Some("@@split method is not callable"),
        )?;
        if let Some(result) = splitter {
            return Ok(result);
        }
    }

    let this_string = to_string_value(global_object, this_value)?;
    let limit = if limit_value.is_undefined() { u32::MAX } else { to_uint32_value(global_object, limit_value)? };
    let separator = if separator_value.is_undefined() { None } else { Some(to_wtf_string_value(global_object, separator_value)?) };

    let values: Vec<JSValue> =
        algo::split(&this_string.value(), separator.as_ref(), limit).iter().map(|piece| JSValue::from_js_string(js_string(vm, piece))).collect();
    Ok(construct_array(vm, &global_object.array_structure(), &values).as_value())
}

/// `stringProtoFuncNormalize`.
fn string_normalize(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_value = call.this_value();
    require_coercible(this_value, "Type error")?;
    let string = to_string_value(global_object, this_value)?;

    let mut form = NormalizationForm::NFC;
    let form_value = call.argument(0);
    if !form_value.is_undefined() {
        let name = code_units(&to_wtf_string_value(global_object, form_value)?).into_owned();
        let is_form = |expected: &str| name.iter().copied().eq(expected.encode_utf16());
        form = if is_form("NFC") {
            NormalizationForm::NFC
        } else if is_form("NFD") {
            NormalizationForm::NFD
        } else if is_form("NFKC") {
            NormalizationForm::NFKC
        } else if is_form("NFKD") {
            NormalizationForm::NFKD
        } else {
            return Err(Thrown::range_error("argument does not match any normalization form"));
        };
    }

    // Latin-1 (U+0000..U+00FF) não muda em NFC, e ASCII (U+0000..U+007F) não muda em forma nenhuma
    // (https://unicode.org/reports/tr15/#Description_Norm).
    let value = string.value();
    if value.is_8bit() && (form == NormalizationForm::NFC || value.contains_only_ascii()) {
        return Ok(string_value(&string));
    }

    // O C++ recusa strings de 1 << 30 unidades ou mais (o ICU estoura as contas de buffer).
    if value.length() >= (1 << 30) {
        return Err(Thrown::OutOfMemory);
    }
    match normalize_utf16(form, &code_units(&value)) {
        None => Ok(string_value(&string)),
        Some(normalized) => Ok(units_value(global_object.vm(), &normalized)),
    }
}

/// `[Symbol.iterator]` (`stringProtoFuncIterator`).
fn string_iterator(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_value = call.this_value();
    require_coercible(this_value, "Type error")?;
    let string = to_string_value(global_object, this_value)?;
    Ok(JSStringIterator::create(global_object.vm(), &global_object.string_iterator_structure(), &string).as_value())
}

/// `appendEscapeAttributeValue(builder, value)`: `"` vira `&quot;`.
fn append_escaped_attribute_value(builder: &mut Vec<u16>, value: &[u16]) {
    for &unit in value {
        if unit == u16::from(b'"') {
            builder.extend("&quot;".encode_utf16());
        } else {
            builder.push(unit);
        }
    }
}

/// `createHTML(globalObject, thisValue, tagName, attributeName, attributeValue)`. `method` só entra na
/// mensagem do `TypeError` de `this` nulo.
fn create_html(global_object: &JSGlobalObject, call: &HostCall, method: &str, tag_name: &str, attribute_name: &str) -> HostResult {
    let vm = global_object.vm();
    let this_value = call.this_value();
    require_coercible(this_value, &format!("String.prototype.{method} requires that |this| not be null or undefined"))?;
    let string = to_string_value(global_object, this_value)?;

    let mut result: Vec<u16> = vec![u16::from(b'<')];
    result.extend(tag_name.encode_utf16());
    if !attribute_name.is_empty() {
        let attribute_value = to_string_value(global_object, call.argument(0))?;
        result.push(u16::from(b' '));
        result.extend(attribute_name.encode_utf16());
        result.extend("=\"".encode_utf16());
        append_escaped_attribute_value(&mut result, &code_units(&attribute_value.value()));
        result.push(u16::from(b'"'));
    }
    result.push(u16::from(b'>'));
    result.extend_from_slice(&code_units(&string.value()));
    result.extend("</".encode_utf16());
    result.extend(tag_name.encode_utf16());
    result.push(u16::from(b'>'));
    Ok(units_value(vm, &result))
}

/// Um `stringProtoFuncAnchor`/`Big`/... da tabela `stringPrototypeTable`.
macro_rules! html_function {
    ($native:ident, $body:ident, $method:literal, $tag:literal, $attribute:literal) => {
        fn $body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
            create_html(global_object, call, $method, $tag, $attribute)
        }
        host_function!(pub $native, $body);
    };
}

html_function!(string_proto_func_anchor, string_anchor, "anchor", "a", "name");
html_function!(string_proto_func_big, string_big, "big", "big", "");
html_function!(string_proto_func_bold, string_bold, "bold", "b", "");
html_function!(string_proto_func_blink, string_blink, "blink", "blink", "");
html_function!(string_proto_func_fixed, string_fixed, "fixed", "tt", "");
html_function!(string_proto_func_fontcolor, string_fontcolor, "fontcolor", "font", "color");
html_function!(string_proto_func_fontsize, string_fontsize, "fontsize", "font", "size");
html_function!(string_proto_func_italics, string_italics, "italics", "i", "");
html_function!(string_proto_func_link, string_link, "link", "a", "href");
html_function!(string_proto_func_small, string_small, "small", "small", "");
html_function!(string_proto_func_strike, string_strike, "strike", "strike", "");
html_function!(string_proto_func_sub, string_sub, "sub", "sub", "");
html_function!(string_proto_func_sup, string_sup, "sup", "sup", "");

host_function!(pub string_proto_func_replace, string_replace);
host_function!(pub string_proto_func_replace_all, string_replace_all);
host_function!(pub string_proto_func_match, string_match);
host_function!(pub string_proto_func_match_all, string_match_all);
host_function!(pub string_proto_func_search, string_search);
host_function!(pub string_proto_func_split, string_split);
host_function!(pub string_proto_func_normalize, string_normalize);
host_function!(pub string_proto_func_iterator, string_iterator);
