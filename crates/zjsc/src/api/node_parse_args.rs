//! `util.parseArgs` do bun 1.4.2: o porte de `runtime/node/util/parse_args.rs` e `parse_args_utils.rs`. O texto é
//! tratado em unidades UTF-16 (o `length` e o `substring` do JavaScript), e cada erro leva o `code` e a mensagem do
//! bun, que não são os do Node (`Option '-f' argument is ambiguous...` com quebras de linha, por exemplo).
//!
//! Tudo o que o bun faz com `MarkedArgumentBuffer` aqui é um `RootScope`: os valores que ficam em locais Rust enquanto
//! um acessor do chamador roda JavaScript.

use crate::api::eval::key_of;
use crate::host_function;
use crate::runtime::gc::RootScope;
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::js_array::JSArray;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObjectRef;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_boolean, js_number, JSValue};
use crate::runtime::node_error::throw_coded_type_error;
use crate::runtime::node_validators::{
    array_items, array_length, get_index, get_own, get_own_by_identifier, invalid_arg_type_message, is_array, validate_array, validate_boolean,
    validate_boolean_array, validate_object, validate_string, validate_string_array,
};
use crate::runtime::object_constructor::{construct_array_of, construct_empty_object, enumerable_own_entries, identifier_to_js_value, put_own_data_property_may_be_index};
use crate::runtime::process_shape::{process_argv, process_exec_argv};
use crate::runtime::property_name::PropertyName;
use crate::runtime::url::wtf_units;
use crate::wtf::text::wtf_string::String as WtfString;

type Units = Vec<u16>;

fn units(text: &str) -> Units {
    text.encode_utf16().collect()
}

fn lossy(text: &[u16]) -> String {
    String::from_utf16_lossy(text)
}

fn starts_with(text: &[u16], prefix: &str) -> bool {
    let prefix = units(prefix);
    text.len() >= prefix.len() && text[..prefix.len()] == prefix[..]
}

fn index_of(text: &[u16], unit: u8) -> Option<usize> {
    text.iter().position(|&candidate| candidate == u16::from(unit))
}

/// `substring_with_len(start, end)`: o trecho `[start, end)`, cortado ao tamanho.
fn slice_of(text: &[u16], start: usize, end: usize) -> Units {
    let end = end.min(text.len());
    text[start.min(end)..end].to_vec()
}

fn text_value(global_object: &JSGlobalObject, text: &[u16]) -> JSValue {
    JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf16(text)))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OptionType {
    Boolean,
    String,
}

/// Metadados de uma opção conhecida (o que `parseArgs(..., { options })` declarou).
struct OptionDefinition {
    long_name: Units,
    /// Vazio quando não há atalho de uma letra.
    short_name: Units,
    kind: OptionType,
    multiple: bool,
    default_value: Option<JSValue>,
}

impl OptionDefinition {
    fn dash_prefix(&self) -> &'static str {
        if self.short_name.is_empty() {
            ""
        } else {
            "-"
        }
    }

    fn comma_prefix(&self) -> &'static str {
        if self.short_name.is_empty() {
            ""
        } else {
            ", "
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenSubtype {
    OptionTerminator,
    LoneShortOption,
    ShortOptionGroup,
    ShortOptionAndValue,
    LoneLongOption,
    LongOptionAndValue,
    Positional,
}

/// A opção cujo atalho é `short_name` (de uma letra) ou, na falta, a de nome longo de uma letra igual a ele.
fn find_option_by_short_name(short_name: &[u16], options: &[OptionDefinition]) -> Option<usize> {
    let mut long_option_index = None;
    for (index, option) in options.iter().enumerate() {
        if short_name == option.short_name.as_slice() {
            return Some(index);
        }
        if option.long_name.len() == 1 && short_name == option.long_name.as_slice() {
            long_option_index = Some(index);
        }
    }
    long_option_index
}

fn find_option_by_long_name(long_name: &[u16], options: &[OptionDefinition]) -> Option<usize> {
    options.iter().position(|option| option.long_name == long_name)
}

fn classify_token(arg: &[u16], options: &[OptionDefinition]) -> TokenSubtype {
    let length = arg.len();
    if length == 2 {
        if starts_with(arg, "-") {
            return if starts_with(arg, "--") { TokenSubtype::OptionTerminator } else { TokenSubtype::LoneShortOption };
        }
    } else if length > 2 {
        if starts_with(arg, "--") {
            return if index_of(arg, b'=').unwrap_or(0) >= 3 { TokenSubtype::LongOptionAndValue } else { TokenSubtype::LoneLongOption };
        } else if starts_with(arg, "-") {
            let first_char = slice_of(arg, 1, 2);
            return match find_option_by_short_name(&first_char, options) {
                Some(index) if options[index].kind == OptionType::String => TokenSubtype::ShortOptionAndValue,
                _ => TokenSubtype::ShortOptionGroup,
            };
        }
    }
    TokenSubtype::Positional
}

/// Em modo estrito, `--port --verbose` com `port` de tipo string: o valor parece outra opção.
fn is_option_like_value(value: &[u16]) -> bool {
    value.len() > 1 && starts_with(value, "-")
}

/// O valor de um token: um `JSValue` do chamador, ou um trecho já cortado (evita criar o `JSValue` à toa).
#[derive(Clone)]
enum ValueRef {
    Js(JSValue),
    Text(Units),
}

impl ValueRef {
    fn as_units(&self, global_object: &JSGlobalObject) -> Result<Units, Thrown> {
        match self {
            ValueRef::Js(value) => Ok(wtf_units(&pending_or(global_object, value.to_wtf_string())?)),
            ValueRef::Text(text) => Ok(text.clone()),
        }
    }

    fn as_js_value(&self, global_object: &JSGlobalObject) -> JSValue {
        match self {
            ValueRef::Js(value) => *value,
            ValueRef::Text(text) => text_value(global_object, text),
        }
    }

    fn is_undefined(&self) -> bool {
        matches!(self, ValueRef::Js(value) if value.is_undefined())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OptionParseType {
    LoneShortOption,
    ShortOptionAndValue,
    LoneLongOption,
    LongOptionAndValue,
}

#[derive(Clone)]
struct OptionToken {
    index: u32,
    name: ValueRef,
    parse_type: OptionParseType,
    value: ValueRef,
    inline_value: bool,
    optgroup_idx: Option<u32>,
    option_idx: Option<usize>,
    negative: bool,
    /// O argumento cru inteiro (`--arg=1`), quando existe como veio na lista; senão o trecho que o gerou.
    raw: ValueRef,
}

enum Token {
    Positional { index: u32, value: ValueRef },
    Option(OptionToken),
    OptionTerminator { index: u32 },
}

impl Token {
    fn kind_name(&self) -> &'static str {
        match self {
            Token::Positional { .. } => "positional",
            Token::Option(_) => "option",
            Token::OptionTerminator { .. } => "option-terminator",
        }
    }

    fn kind_index(&self) -> usize {
        match self {
            Token::Positional { .. } => 0,
            Token::Option(_) => 1,
            Token::OptionTerminator { .. } => 2,
        }
    }
}

impl OptionToken {
    /// O nome cru do argumento (com os traços, sem o valor inline), como o `RawNameFormatter` do bun o escreve nas
    /// mensagens: num grupo curto (`-abc`) é só a letra, sem traço.
    fn raw_name_text(&self, global_object: &JSGlobalObject) -> Result<Units, Thrown> {
        let raw = self.raw.as_units(global_object)?;
        if let Some(group_index) = self.optgroup_idx {
            let at = group_index as usize;
            return Ok(slice_of(&raw, at, at + 1));
        }
        Ok(match self.parse_type {
            OptionParseType::LoneShortOption | OptionParseType::LoneLongOption => raw,
            OptionParseType::ShortOptionAndValue => slice_of(&raw, 0, 2),
            OptionParseType::LongOptionAndValue => {
                let equal_index = index_of(&raw, b'=').expect("a opção com valor inline tem '='");
                slice_of(&raw, 0, equal_index)
            }
        })
    }

    /// `rawName` do token entregue em `tokens`: o nome cru, com o traço quando vem de um grupo curto.
    fn raw_name_value(&self, global_object: &JSGlobalObject) -> Result<JSValue, Thrown> {
        if let Some(group_index) = self.optgroup_idx {
            let raw = self.raw.as_units(global_object)?;
            let at = group_index as usize;
            let mut name = units("-");
            name.extend(slice_of(&raw, at, at + 1));
            return Ok(text_value(global_object, &name));
        }
        match self.parse_type {
            OptionParseType::LoneShortOption | OptionParseType::LoneLongOption => Ok(self.raw.as_js_value(global_object)),
            _ => Ok(text_value(global_object, &self.raw_name_text(global_object)?)),
        }
    }
}

fn throw_parse_args_error(global_object: &JSGlobalObject, code: &str, message: &str) -> Thrown {
    throw_coded_type_error(global_object, message, code)
}

/// Em modo estrito, o valor que a opção de tipo string consumiu parece outra opção.
fn check_option_like_value(global_object: &JSGlobalObject, token: &OptionToken) -> Result<(), Thrown> {
    if token.inline_value || !is_option_like_value(&token.value.as_units(global_object)?) {
        return Ok(());
    }
    let raw = token.raw.as_units(global_object)?;
    let raw_name = lossy(&token.raw_name_text(global_object)?);
    // O exemplo curto só aparece quando o usuário usou a opção curta.
    let message = if starts_with(&raw, "--") {
        format!(
            "Option '{raw_name}' argument is ambiguous.\nDid you forget to specify the option argument for '{raw_name}'?\nTo specify an option argument starting with a dash use '{raw_name}=-XYZ'."
        )
    } else {
        let token_name = lossy(&token.name.as_units(global_object)?);
        format!(
            "Option '{raw_name}' argument is ambiguous.\nDid you forget to specify the option argument for '{raw_name}'?\nTo specify an option argument starting with a dash use '--{token_name}=-XYZ' or '{raw_name}-XYZ'."
        )
    };
    Err(throw_parse_args_error(global_object, "ERR_PARSE_ARGS_INVALID_OPTION_VALUE", &message))
}

/// Em modo estrito, os erros de uso.
fn check_option_usage(global_object: &JSGlobalObject, options: &[OptionDefinition], allow_positionals: bool, token: &OptionToken) -> Result<(), Thrown> {
    let Some(option_idx) = token.option_idx else {
        let raw_name = lossy(&token.raw_name_text(global_object)?);
        let message = if allow_positionals {
            format!("Unknown option '{raw_name}'. To specify a positional argument starting with a '-', place it at the end of the command after '--', as in '-- \"{raw_name}\"")
        } else {
            format!("Unknown option '{raw_name}'")
        };
        return Err(throw_parse_args_error(global_object, "ERR_PARSE_ARGS_UNKNOWN_OPTION", &message));
    };
    let option = &options[option_idx];
    match option.kind {
        OptionType::String => {
            if matches!(&token.value, ValueRef::Js(value) if !value.is_string()) {
                if token.negative {
                    // A opção foi achada porque o `no-` saiu do nome: sai o erro de opção desconhecida esperado.
                    let raw_name = lossy(&token.raw_name_text(global_object)?);
                    return Err(throw_parse_args_error(global_object, "ERR_PARSE_ARGS_UNKNOWN_OPTION", &format!("Unknown option '{raw_name}'")));
                }
                let message = format!(
                    "Option '{}{}{}--{} <value>' argument missing",
                    option.dash_prefix(),
                    lossy(&option.short_name),
                    option.comma_prefix(),
                    lossy(&token.name.as_units(global_object)?)
                );
                return Err(throw_parse_args_error(global_object, "ERR_PARSE_ARGS_INVALID_OPTION_VALUE", &message));
            }
        }
        OptionType::Boolean => {
            if !token.value.is_undefined() {
                let message = format!(
                    "Option '{}{}{}--{}' does not take an argument",
                    option.dash_prefix(),
                    lossy(&option.short_name),
                    option.comma_prefix(),
                    lossy(&token.name.as_units(global_object)?)
                );
                return Err(throw_parse_args_error(global_object, "ERR_PARSE_ARGS_INVALID_OPTION_VALUE", &message));
            }
        }
    }
    Ok(())
}

/// Os argumentos da linha de comando e o trecho que vale: `args[start..end]`.
struct ArgsSlice {
    array: JSValue,
    start: u32,
    end: u32,
}

impl ArgsSlice {
    fn get(&self, global_object: &JSGlobalObject, index: u32) -> Result<JSValue, Thrown> {
        get_index(global_object, self.array, self.start + index)
    }
}

/// Os argumentos padrão: `process.argv` sem o executável e o script (ou só sem o executável com `-e`, `-p`).
fn get_default_args(global_object: &JSGlobalObject) -> Result<ArgsSlice, Thrown> {
    let exec_argv = process_exec_argv(global_object);
    let argv = process_argv(global_object);
    if !(is_array(argv) && is_array(exec_argv)) {
        return Ok(ArgsSlice { array: JSValue::undefined(), start: 0, end: 0 });
    }
    for item in array_items(global_object, exec_argv)? {
        if !item.is_string() {
            continue;
        }
        let text = wtf_units(&pending_or(global_object, item.to_wtf_string())?);
        if ["-e", "--eval", "-p", "--print"].iter().any(|flag| text == units(flag)) {
            return Ok(ArgsSlice { array: argv, start: 1, end: array_length(argv) });
        }
    }
    Ok(ArgsSlice { array: argv, start: 2, end: array_length(argv) })
}

fn null_prototype_object(global_object: &JSGlobalObject) -> JSObjectRef {
    let object = construct_empty_object(global_object);
    object.set_prototype_direct(global_object.vm(), JSValue::null());
    object
}

struct ParseArgsState<'a> {
    global_object: &'a JSGlobalObject,
    roots: &'a RootScope,
    option_defs: &'a [OptionDefinition],
    allow_positionals: bool,
    strict: bool,
    allow_negative: bool,
    values: JSObjectRef,
    positionals: JSValue,
    /// `undefined` quando o chamador não pediu `tokens`.
    tokens: JSValue,
    /// O `JSValue` do nome do tipo ("positional", "option", "option-terminator"), reaproveitado entre os tokens.
    kinds: [Option<JSValue>; 3],
}

impl ParseArgsState<'_> {
    /// Guarda o valor na pilha de raízes e o devolve.
    fn rooted(&self, value: JSValue) -> JSValue {
        self.roots.append(value);
        value
    }

    fn array_push(&self, array: JSValue, value: JSValue) -> Result<(), Thrown> {
        let Some(array) = JSArray::from_value(&array) else { return Ok(()) };
        array.push(self.global_object.vm(), value).map_err(|_| Thrown::Pending)
    }

    fn put_value(&self, name: &[u16], value: JSValue) -> Result<(), Thrown> {
        let vm = self.global_object.vm();
        let property = PropertyName::from_identifier(&Identifier::from_span(vm, name));
        put_own_data_property_may_be_index(vm, &self.values, &property, value)
    }

    /// Grava a opção em `values`, no array quando é `multiple`.
    fn store_option(&self, name: &ValueRef, value: &ValueRef, option_idx: Option<usize>, negative: bool) -> Result<(), Thrown> {
        let global_object = self.global_object;
        let key = name.as_units(global_object)?;
        if key == units("__proto__") {
            return Ok(());
        }
        let value = self.rooted(value.as_js_value(global_object));
        // Guarda o que o usuário escreveu, não o tipo declarado: `undefined` vira o booleano da opção.
        let new_value = if value.is_undefined() { js_boolean(!negative) } else { value };
        if option_idx.is_some_and(|index| self.option_defs[index].multiple) {
            // `multiple` guarda sempre um array, também para booleano.
            let identifier = Identifier::from_span(global_object.vm(), &key);
            if let Some(list) = get_own_by_identifier(global_object, self.values.as_value(), &identifier)? {
                self.array_push(list, new_value)?;
            } else {
                let list = self.rooted(construct_array_of(global_object, &[new_value]).as_value());
                self.put_value(&key, list)?;
            }
        } else {
            self.put_value(&key, new_value)?;
        }
        Ok(())
    }

    fn kind_value(&mut self, token: &Token) -> JSValue {
        let index = token.kind_index();
        if let Some(value) = self.kinds[index] {
            return value;
        }
        let value = self.rooted(text_value(self.global_object, &units(token.kind_name())));
        self.kinds[index] = Some(value);
        value
    }

    fn handle_token(&mut self, token: &Token) -> Result<(), Thrown> {
        let global_object = self.global_object;
        match token {
            Token::Option(option) => {
                if self.strict {
                    check_option_usage(global_object, self.option_defs, self.allow_positionals, option)?;
                    check_option_like_value(global_object, option)?;
                }
                self.store_option(&option.name, &option.value, option.option_idx, option.negative)?;
            }
            Token::Positional { value, .. } => {
                if !self.allow_positionals {
                    let message = format!("Unexpected argument '{}'. This command does not take positional arguments", lossy(&value.as_units(global_object)?));
                    return Err(throw_parse_args_error(global_object, "ERR_PARSE_ARGS_UNEXPECTED_POSITIONAL", &message));
                }
                let value = self.rooted(value.as_js_value(global_object));
                self.array_push(self.positionals, value)?;
            }
            Token::OptionTerminator { .. } => {}
        }
        // `tokens` é opt-in: só se criam os valores JavaScript quando foi pedido.
        if self.tokens.is_undefined() {
            return Ok(());
        }
        let vm = global_object.vm();
        let kind = self.kind_value(token);
        let object = construct_empty_object(global_object);
        self.rooted(object.as_value());
        object.put_direct(vm, &key_of(vm, "kind"), kind, 0);
        match token {
            Token::Option(option) => {
                let name = self.rooted(option.name.as_js_value(global_object));
                object.put_direct(vm, &key_of(vm, "name"), name, 0);
                let raw_name = self.rooted(option.raw_name_value(global_object)?);
                object.put_direct(vm, &key_of(vm, "rawName"), raw_name, 0);
                object.put_direct(vm, &key_of(vm, "index"), js_number(f64::from(option.index)), 0);
                // `value` existe só nas opções de string; nas demais a propriedade existe com `undefined`.
                let value = self.rooted(option.value.as_js_value(global_object));
                object.put_direct(vm, &key_of(vm, "value"), value, 0);
                let inline_value = if value.is_undefined() { JSValue::undefined() } else { js_boolean(option.inline_value) };
                object.put_direct(vm, &key_of(vm, "inlineValue"), inline_value, 0);
            }
            Token::Positional { index, value } => {
                object.put_direct(vm, &key_of(vm, "index"), js_number(f64::from(*index)), 0);
                let value = self.rooted(value.as_js_value(global_object));
                object.put_direct(vm, &key_of(vm, "value"), value, 0);
            }
            Token::OptionTerminator { index } => {
                object.put_direct(vm, &key_of(vm, "index"), js_number(f64::from(*index)), 0);
            }
        }
        self.array_push(self.tokens, object.as_value())
    }
}

/// `-fXzy`: expande em `-f -X -z -y`, com a opção de string no meio levando o resto como valor (`-abfFILE`).
fn tokenize_short_option_group(state: &mut ParseArgsState, args: &ArgsSlice, arg_ref: &ValueRef, arg: &[u16], index: &mut u32, num_args: u32) -> Result<(), Thrown> {
    let global_object = state.global_object;
    let options = state.option_defs;
    let original_arg_idx = *index;
    for idx_in_optgroup in 1..arg.len() {
        let short_option = slice_of(arg, idx_in_optgroup, idx_in_optgroup + 1);
        let option_idx = find_option_by_short_name(&short_option, options);
        let option_type = option_idx.map_or(OptionType::Boolean, |found| options[found].kind);
        let name = ValueRef::Text(option_idx.map_or_else(|| short_option.clone(), |found| options[found].long_name.clone()));
        let optgroup_idx = Some(u32::try_from(idx_in_optgroup).expect("o argumento cabe em u32"));
        if option_type != OptionType::String || idx_in_optgroup == arg.len() - 1 {
            // Booleano, ou a última do grupo: bem formada, processada como `LoneShortOption`.
            let mut value = ValueRef::Js(JSValue::undefined());
            let mut has_inline_value = true;
            if option_type == OptionType::String && *index + 1 < num_args {
                value = ValueRef::Js(args.get(global_object, *index + 1)?);
                has_inline_value = false;
            }
            state.handle_token(&Token::Option(OptionToken {
                index: original_arg_idx,
                optgroup_idx,
                value,
                inline_value: has_inline_value,
                name,
                parse_type: OptionParseType::LoneShortOption,
                raw: arg_ref.clone(),
                option_idx,
                negative: false,
            }))?;
            if !has_inline_value {
                *index += 1;
            }
        } else {
            // Opção de string no meio: `-abfFILE` vira `-a -b -fFILE`, processada como `ShortOptionAndValue`.
            let value = slice_of(arg, idx_in_optgroup + 1, arg.len());
            state.handle_token(&Token::Option(OptionToken {
                index: original_arg_idx,
                optgroup_idx,
                value: ValueRef::Text(value),
                inline_value: true,
                name,
                parse_type: OptionParseType::ShortOptionAndValue,
                raw: arg_ref.clone(),
                option_idx,
                negative: false,
            }))?;
            break;
        }
    }
    Ok(())
}

/// Percorre os argumentos e entrega cada token (opção com o valor, posicional, terminador) ao estado.
fn tokenize_args(state: &mut ParseArgsState, args: &ArgsSlice) -> Result<(), Thrown> {
    let global_object = state.global_object;
    let options = state.option_defs;
    let num_args = args.end - args.start;
    let mut index: u32 = 0;
    while index < num_args {
        let raw_value = args.get(global_object, index)?;
        state.roots.append(raw_value);
        let arg_ref = ValueRef::Js(raw_value);
        let arg = arg_ref.as_units(global_object)?;
        match classify_token(&arg, options) {
            // `--` sozinho: tudo depois dele é posicional (a diretriz 10 da POSIX).
            TokenSubtype::OptionTerminator => {
                state.handle_token(&Token::OptionTerminator { index })?;
                index += 1;
                while index < num_args {
                    let value = args.get(global_object, index)?;
                    state.roots.append(value);
                    state.handle_token(&Token::Positional { index, value: ValueRef::Js(value) })?;
                    index += 1;
                }
                break;
            }
            TokenSubtype::LoneShortOption => {
                let short_option = slice_of(&arg, 1, 2);
                let option_idx = find_option_by_short_name(&short_option, options);
                let option_type = option_idx.map_or(OptionType::Boolean, |found| options[found].kind);
                let mut value = ValueRef::Js(JSValue::undefined());
                let mut has_inline_value = true;
                if option_type == OptionType::String && index + 1 < num_args {
                    // `-f bar`: consome o argumento seguinte como valor.
                    let next = args.get(global_object, index + 1)?;
                    state.roots.append(next);
                    value = ValueRef::Js(next);
                    has_inline_value = false;
                }
                state.handle_token(&Token::Option(OptionToken {
                    index,
                    value,
                    inline_value: has_inline_value,
                    name: ValueRef::Text(option_idx.map_or(short_option, |found| options[found].long_name.clone())),
                    parse_type: OptionParseType::LoneShortOption,
                    raw: arg_ref,
                    option_idx,
                    optgroup_idx: None,
                    negative: false,
                }))?;
                if !has_inline_value {
                    index += 1;
                }
            }
            TokenSubtype::ShortOptionGroup => tokenize_short_option_group(state, args, &arg_ref, &arg, &mut index, num_args)?,
            TokenSubtype::ShortOptionAndValue => {
                // `-fFILE`.
                let short_option = slice_of(&arg, 1, 2);
                let option_idx = find_option_by_short_name(&short_option, options);
                state.handle_token(&Token::Option(OptionToken {
                    index,
                    value: ValueRef::Text(slice_of(&arg, 2, arg.len())),
                    inline_value: true,
                    name: ValueRef::Text(option_idx.map_or(short_option, |found| options[found].long_name.clone())),
                    parse_type: OptionParseType::ShortOptionAndValue,
                    raw: ValueRef::Text(slice_of(&arg, 0, 2)),
                    option_idx,
                    optgroup_idx: None,
                    negative: false,
                }))?;
            }
            TokenSubtype::LoneLongOption => {
                // `--foo`, e `--no-foo` com `allowNegative`.
                let mut long_option = slice_of(&arg, 2, arg.len());
                let negative = state.allow_negative && starts_with(&long_option, "no-");
                if negative {
                    long_option = slice_of(&arg, 5, arg.len());
                }
                let option_idx = find_option_by_long_name(&long_option, options);
                let option_type = option_idx.map_or(OptionType::Boolean, |found| options[found].kind);
                let mut value = None;
                if option_type == OptionType::String && index + 1 < num_args && !negative {
                    // `--foo bar`: consome o argumento seguinte como valor.
                    let next = args.get(global_object, index + 1)?;
                    state.roots.append(next);
                    value = Some(next);
                }
                state.handle_token(&Token::Option(OptionToken {
                    index,
                    value: ValueRef::Js(value.unwrap_or_else(JSValue::undefined)),
                    inline_value: value.is_none(),
                    name: ValueRef::Text(long_option),
                    parse_type: OptionParseType::LoneLongOption,
                    raw: arg_ref,
                    option_idx,
                    optgroup_idx: None,
                    negative,
                }))?;
                if value.is_some() {
                    index += 1;
                }
            }
            TokenSubtype::LongOptionAndValue => {
                // `--foo=bar`.
                let equal_index = index_of(&arg, b'=').expect("a opção com valor inline tem '='");
                let long_option = slice_of(&arg, 2, equal_index);
                state.handle_token(&Token::Option(OptionToken {
                    index,
                    value: ValueRef::Text(slice_of(&arg, equal_index + 1, arg.len())),
                    inline_value: true,
                    option_idx: find_option_by_long_name(&long_option, options),
                    name: ValueRef::Text(long_option),
                    parse_type: OptionParseType::LongOptionAndValue,
                    raw: arg_ref,
                    optgroup_idx: None,
                    negative: false,
                }))?;
            }
            TokenSubtype::Positional => state.handle_token(&Token::Positional { index, value: arg_ref })?,
        }
        index += 1;
    }
    Ok(())
}

/// `validateStringEnum`: o valor, convertido em texto, é um dos nomes (`boolean|string`).
fn validate_option_type(global_object: &JSGlobalObject, value: JSValue, name: &str) -> Result<OptionType, Thrown> {
    let text = wtf_units(&pending_or(global_object, value.to_wtf_string())?);
    if text == units("boolean") {
        Ok(OptionType::Boolean)
    } else if text == units("string") {
        Ok(OptionType::String)
    } else {
        Err(invalid_arg_type_message(global_object, &format!("{name} must be one of: boolean|string")))
    }
}

/// Lê e valida a definição de cada opção de `config.options`.
fn parse_option_definitions(global_object: &JSGlobalObject, roots: &RootScope, options: JSValue) -> Result<Vec<OptionDefinition>, Thrown> {
    let vm = global_object.vm();
    let mut definitions = Vec::new();
    for (identifier, definition) in enumerable_own_entries(global_object, options)? {
        roots.append(definition);
        let long_name = wtf_units(&pending_or(global_object, identifier_to_js_value(vm, &identifier).to_wtf_string())?);
        let long_text = lossy(&long_name);
        validate_object(global_object, definition, &format!("options.{long_text}"))?;
        let mut option = OptionDefinition { long_name, short_name: Vec::new(), kind: OptionType::Boolean, multiple: false, default_value: None };

        // `type` é obrigatório.
        let option_type = get_own(global_object, definition, "type")?.unwrap_or_else(JSValue::undefined);
        option.kind = validate_option_type(global_object, option_type, &format!("options.{long_text}.type"))?;

        if let Some(short_option) = get_own(global_object, definition, "short")? {
            validate_string(global_object, short_option, &format!("options.{long_text}.short"))?;
            let short_name = wtf_units(&pending_or(global_object, short_option.to_wtf_string())?);
            if short_name.len() != 1 {
                let message = format!("options.{long_text}.short must be a single character");
                return Err(crate::runtime::node_validators::invalid_arg_value_message(global_object, &message));
            }
            option.short_name = short_name;
        }

        if let Some(multiple) = get_own(global_object, definition, "multiple")?.filter(|value| !value.is_undefined()) {
            option.multiple = validate_boolean(global_object, multiple, &format!("options.{long_text}.multiple"))?;
        }

        if let Some(default_value) = get_own(global_object, definition, "default")?.filter(|value| !value.is_undefined()) {
            let name = format!("options.{long_text}.default");
            match (option.kind, option.multiple) {
                (OptionType::String, true) => drop(validate_string_array(global_object, default_value, &name)?),
                (OptionType::String, false) => validate_string(global_object, default_value, &name)?,
                (OptionType::Boolean, true) => drop(validate_boolean_array(global_object, default_value, &name)?),
                (OptionType::Boolean, false) => drop(validate_boolean(global_object, default_value, &name)?),
            }
            roots.append(default_value);
            option.default_value = Some(default_value);
        }
        definitions.push(option);
    }
    Ok(definitions)
}

/// `config.name` quando a chave existe e não é `undefined` nem `null` (o `?? padrão` do Node).
fn config_flag(global_object: &JSGlobalObject, config: JSValue, name: &str) -> Result<Option<JSValue>, Thrown> {
    if config.is_undefined() {
        return Ok(None);
    }
    Ok(get_own(global_object, config, name)?.filter(|value| !value.is_undefined() && !value.is_null()))
}

/// `util.parseArgs(config)`.
fn parse_args_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let roots = RootScope::new(&[]);
    let config = call.argument(0);
    roots.append(config);

    // Fase 0.A: os argumentos de entrada.
    let config_args = if config.is_undefined() { JSValue::undefined() } else { get_own(global_object, config, "args")?.unwrap_or_else(JSValue::undefined) };
    roots.append(config_args);
    let args = if !config_args.is_undefined() && !config_args.is_null() {
        validate_array(global_object, config_args, "args", None)?;
        ArgsSlice { array: config_args, start: 0, end: array_length(config_args) }
    } else {
        let default_args = get_default_args(global_object)?;
        roots.append(default_args.array);
        default_args
    };

    // Fase 0.B: a configuração. O `null` explícito vale como chave ausente.
    let config_strict = config_flag(global_object, config, "strict")?.unwrap_or_else(|| js_boolean(true));
    let config_allow_positionals = config_flag(global_object, config, "allowPositionals")?.unwrap_or_else(|| js_boolean(!config_strict.to_boolean()));
    let config_return_tokens = config_flag(global_object, config, "tokens")?.unwrap_or_else(|| js_boolean(false));
    let config_allow_negative = config_flag(global_object, config, "allowNegative")?.unwrap_or_else(|| js_boolean(false));
    let config_options = if config.is_undefined() { JSValue::undefined() } else { get_own(global_object, config, "options")?.unwrap_or_else(JSValue::undefined) };
    roots.append(config_options);

    let strict = validate_boolean(global_object, config_strict, "strict")?;
    let allow_positionals = validate_boolean(global_object, config_allow_positionals, "allowPositionals")?;
    let return_tokens = validate_boolean(global_object, config_return_tokens, "tokens")?;
    let allow_negative = validate_boolean(global_object, config_allow_negative, "allowNegative")?;

    // Fase 0.C: as definições das opções.
    let option_defs = if !config_options.is_undefined() && !config_options.is_null() {
        validate_object(global_object, config_options, "options")?;
        parse_option_definitions(global_object, &roots, config_options)?
    } else {
        Vec::new()
    };

    // Fases 1 e 2: os tokens e, deles, os valores e os posicionais. `values` não tem protótipo, para que `values.toString`
    // não exista.
    let values = null_prototype_object(global_object);
    roots.append(values.as_value());
    let positionals = construct_array_of(global_object, &[]).as_value();
    roots.append(positionals);
    let tokens = if return_tokens { construct_array_of(global_object, &[]).as_value() } else { JSValue::undefined() };
    roots.append(tokens);
    let mut state = ParseArgsState {
        global_object,
        roots: &roots,
        option_defs: &option_defs,
        allow_positionals,
        strict,
        allow_negative,
        values,
        positionals,
        tokens,
        kinds: [None; 3],
    };
    tokenize_args(&mut state, &args)?;

    // Fase 3: o valor padrão de quem não apareceu.
    for option in &option_defs {
        let Some(default_value) = option.default_value else { continue };
        if option.long_name == units("__proto__") {
            continue;
        }
        let identifier = Identifier::from_span(vm, &option.long_name);
        if get_own_by_identifier(global_object, state.values.as_value(), &identifier)?.is_none() {
            state.put_value(&option.long_name, default_value)?;
        }
    }

    // Fase 4: `{ values, positionals, tokens? }`.
    let result = construct_empty_object(global_object);
    result.put_direct(vm, &key_of(vm, "values"), state.values.as_value(), 0);
    result.put_direct(vm, &key_of(vm, "positionals"), state.positionals, 0);
    if return_tokens {
        result.put_direct(vm, &key_of(vm, "tokens"), state.tokens, 0);
    }
    Ok(result.as_value())
}
host_function!(pub parse_args, parse_args_body);
