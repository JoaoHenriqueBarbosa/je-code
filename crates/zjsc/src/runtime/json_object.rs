//! Porte de `runtime/JSONObject.cpp`: a lógica do `JSON.stringify` (`Stringifier`), do `JSON.parse`
//! com reviver (`Walker`, `jsonParseSlow`) e a validação do `JSON.rawJSON`, como funções sobre
//! `JSValue` que pedem ao motor o que ele ainda não tem através de `JsonHost` (definido em
//! `literal_parser.rs`, junto de `JsonKey` e `JsonError`).
//!
//! LIGAÇÃO: feita em `json_object_native.rs` (`create_json_object`, chamado por
//! `install_json_reflect_and_collections` em `js_global_object_init.rs`) e `json_host.rs` (`impl
//! JsonHost for JSGlobalObject`). A ligação de cada função nativa é:
//!
//! - `jsonProtoFuncParse`: `arg0.toString` (texto) e, se `argumentCount >= 2`, `arg1` como reviver;
//!   chama `json_parse(host, &texto, reviver, Options::use_json_source_text_access())` e, no `Err`,
//!   `throw_json_error`.
//! - `jsonProtoFuncStringify`: `json_stringify(host, arg0, arg1, arg2)`; `Ok(None)` é `undefined`,
//!   `Ok(Some(s))` é `jsString(vm, s)`.
//! - `jsonProtoFuncRawJSON`: `arg0.toString`, `validate_raw_json`, e então o `JSRawJSONObject::tryCreate`
//!   (a célula ainda não existe no porte, é parte do `impl JsonHost`).
//! - `jsonProtoFuncIsRawJSON`: `host.raw_json_text(arg0).is_some()`.
//! - Os acessos da tabela `jsonTable` e o `@@toStringTag` do `JSONObject::finishCreation`.
//!
//! DIVERGÊNCIAS:
//!
//! - `FastStringifier` não foi portado: é uma otimização sem efeito colateral, que cai no
//!   `Stringifier` geral quando não resolve, e o resultado dos dois é o mesmo texto.
//! - O `Stringifier` não tem o caminho rápido por `Structure` (`canPerformFastPropertyEnumeration`,
//!   `getDirect(offset)`, `cachedSpecialProperty(ToJSON)`): as chaves vêm de
//!   `JsonHost::own_enumerable_string_keys` e os valores de `JsonHost::get`, o caminho geral do C++.
//! - `PropertyNameForFunctionCall` vira `JsonKey` mais a criação da string no uso (sem o cache de
//!   `m_value` e sem `smallStrings`).
//! - A checagem `vm.isSafeToRecurse` do `appendStringifiedValue` some: a função só recursa uma vez
//!   (o laço do holder substitui a recursão), então só vale o limite `maximumSideStackRecursion`.
//! - `MarkedArgumentBuffer`/`m_objectStack` e as demais pilhas de GC somem: o registro de células
//!   não coleta.
//! - `JSONParse`, `JSONParseWithException`, `streamingJSONParse` (Bun) e os dois `JSONStringify` de
//!   conveniência são chamadas diretas a `literal_parse` e `json_stringify`; só `json_parse_quiet`
//!   (o `JSONParse`, que devolve vazio em vez de lançar) tem função própria.

use std::collections::HashSet;
use std::rc::Rc;

use crate::runtime::error::{create_syntax_error, create_type_error};
use crate::runtime::exception_helpers::{throw_out_of_memory_error, throw_stack_overflow_error};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_promise_host::Thrown;
use crate::runtime::js_string::{js_empty_string, js_string};
use crate::runtime::js_type::JSType;
use crate::runtime::js_value::JSValue;
use crate::runtime::literal_parser::{
    literal_parse, units_to_wtf_string, wtf_string_to_units, BoxedPrimitiveKind, JsonError, JsonHost, JsonKey, JsonRangeEntry,
    JsonRangeProperties, JsonRanges, LiteralParseKind, ParseOutcome,
};
use crate::runtime::operations::same_value;
use crate::runtime::cell_registry;
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::runtime::vm::Exception;
use crate::wtf::text::string_builder::{OverflowPolicy, StringBuilder};
use crate::wtf::text::wtf_string::String as WtfString;

/// `maxGapLength`.
const MAX_GAP_LENGTH: usize = 10;

/// `maximumSideStackRecursion`: o limite da pilha de holders, bem além de qualquer uso razoável.
const MAXIMUM_SIDE_STACK_RECURSION: usize = 40000;

/// Lança no `VM` o que o `JsonError` descreve (os `throwSyntaxError`, `throwTypeError`,
/// `throwOutOfMemoryError` e `throwStackOverflowError` do C++). `Thrown::Value` é a exceção que o
/// host pediu ao chamador para lançar.
pub fn throw_json_error(global_object: &JSGlobalObject, error: JsonError) -> Rc<Exception> {
    let vm = global_object.vm();
    let mut scope = ThrowScope::new(vm);
    match error {
        JsonError::Thrown(Thrown::Value(value)) => throw_exception(global_object, &mut scope, value),
        JsonError::Thrown(Thrown::Termination) => {
            let termination = vm.ensure_termination_exception();
            throw_exception(global_object, &mut scope, termination)
        }
        JsonError::Syntax(message) => {
            let error = create_syntax_error(global_object, &message);
            throw_exception(global_object, &mut scope, error)
        }
        JsonError::Type(message) => {
            let error = create_type_error(global_object, &message);
            throw_exception(global_object, &mut scope, error)
        }
        JsonError::OutOfMemory => throw_out_of_memory_error(global_object, &mut scope),
        JsonError::StackOverflow => throw_stack_overflow_error(global_object, &mut scope),
    }
}

// ------------------------------ helper functions --------------------------------

/// `value.isSymbol()`.
pub(crate) fn is_symbol(value: JSValue) -> bool {
    matches!(value, JSValue::Cell(id) if cell_registry::cell_type(id) == Some(JSType::SymbolType))
}

/// `value.isBigInt()` (o `BigInt32` não existe: `USE(BIGINT32)` é 0).
fn is_big_int(value: JSValue) -> bool {
    matches!(value, JSValue::Cell(id) if cell_registry::cell_type(id) == Some(JSType::HeapBigIntType))
}

/// `unwrapBoxedPrimitive(globalObject, JSValue)`: não desembrulha `SymbolObject` (não é feito na
/// especificação).
fn unwrap_boxed_primitive<H: JsonHost + ?Sized>(host: &H, value: JSValue) -> Result<JSValue, JsonError> {
    if !host.is_object(value) || host.boxed_primitive_kind(value) == BoxedPrimitiveKind::None {
        return Ok(value);
    }
    Ok(host.unwrap_boxed_primitive(value)?)
}

/// `gap(globalObject, space)`: os espaços de indentação (no máximo dez unidades).
fn gap<H: JsonHost + ?Sized>(host: &H, space: JSValue) -> Result<Vec<u16>, JsonError> {
    let space = unwrap_boxed_primitive(host, space)?;

    // Número: essa quantidade de espaços (`clampTo<unsigned>(space.asNumber(), 0, maxGapLength)`).
    if space.is_number() {
        let number = space.as_number();
        let count = if number >= MAX_GAP_LENGTH as f64 {
            MAX_GAP_LENGTH
        } else if number > 0.0 {
            number as usize
        } else {
            0
        };
        return Ok(vec![0x20; count]);
    }

    // String: ela mesma, cortada em dez; qualquer outro valor não indenta.
    if space.is_string() {
        let mut units = wtf_string_to_units(&space.as_js_string().value());
        units.truncate(MAX_GAP_LENGTH);
        return Ok(units);
    }
    Ok(Vec::new())
}

/// A chave `""` (`vm.propertyNames->emptyIdentifier`).
fn empty_key() -> JsonKey {
    JsonKey::Name(WtfString::from_latin1(b""))
}

/// `PropertyNameForFunctionCall::value(vm)`: o nome como string JS.
fn property_name_value<H: JsonHost + ?Sized>(host: &H, key: &JsonKey) -> JSValue {
    JSValue::from_js_string(js_string(host.vm(), &key.to_wtf_string()))
}

// ------------------------------ Stringifier --------------------------------

/// `enum StringifyResult`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StringifyResult {
    StringifyFailed,
    StringifySucceeded,
    StringifyFailedDueToUndefinedOrSymbolValue,
}

use StringifyResult::*;

/// O que o `appendStringifiedValue` lê do holder: se é array e o objeto (o `this` do replacer).
#[derive(Clone, Copy)]
struct HolderInfo {
    is_array: bool,
    object: Option<JSValue>,
}

/// `Stringifier::Holder`, sem os campos do caminho rápido por `Structure`.
struct Holder {
    object: Option<JSValue>,
    is_array: bool,
    index: u32,
    size: u32,
    property_names: Vec<JsonKey>,
}

/// `class Stringifier`.
struct Stringifier<'a, H: JsonHost + ?Sized> {
    host: &'a H,
    replacer: JSValue,
    callable_replacer: bool,
    using_array_replacer: bool,
    array_replacer_names: Vec<JsonKey>,
    gap: Vec<u16>,
    holder_stack: Vec<Holder>,
    /// `m_indent`: o prefixo atual de `m_repeatedGap`.
    indent: Vec<u16>,
}

/// O índice `i` de um array-like como chave (`forEachInArrayLike` passa `uint64_t`).
fn array_like_key(index: u64) -> JsonKey {
    match u32::try_from(index) {
        Ok(small) if small != u32::MAX => JsonKey::Index(small),
        _ => JsonKey::Name(WtfString::number_u64(index)),
    }
}

impl<'a, H: JsonHost + ?Sized> Stringifier<'a, H> {
    /// `Stringifier::Stringifier(globalObject, replacer, space)`.
    fn new(host: &'a H, replacer: JSValue, space: JSValue) -> Result<Stringifier<'a, H>, JsonError> {
        let mut stringifier = Stringifier {
            host,
            replacer,
            callable_replacer: false,
            using_array_replacer: false,
            array_replacer_names: Vec::new(),
            gap: Vec::new(),
            holder_stack: Vec::new(),
            indent: Vec::new(),
        };

        if host.is_object(replacer) {
            if host.is_callable(replacer) {
                stringifier.callable_replacer = true;
            } else if host.is_array(replacer)? {
                stringifier.using_array_replacer = true;
                let length = host.length_of_array_like(replacer)?;
                let mut seen: HashSet<Vec<u16>> = HashSet::new();
                for i in 0..length {
                    let name = host.get(replacer, &array_like_key(i))?;
                    let is_candidate = if host.is_object(name) {
                        matches!(host.boxed_primitive_kind(name), BoxedPrimitiveKind::Number | BoxedPrimitiveKind::String)
                    } else {
                        name.is_number() || name.is_string()
                    };
                    if !is_candidate {
                        continue;
                    }

                    let text = host.to_string(name)?;
                    let key = JsonKey::from_wtf_string(&text);
                    if seen.insert(key.units()) {
                        stringifier.array_replacer_names.push(key);
                    }
                }
            }
        }

        stringifier.gap = gap(host, space)?;
        Ok(stringifier)
    }

    /// `willIndent()`.
    fn will_indent(&self) -> bool {
        !self.gap.is_empty()
    }

    /// `indent()`.
    fn indent(&mut self) {
        self.indent.extend_from_slice(&self.gap);
    }

    /// `unindent()`.
    fn unindent(&mut self) {
        debug_assert!(self.indent.len() >= self.gap.len());
        let new_length = self.indent.len() - self.gap.len();
        self.indent.truncate(new_length);
    }

    /// `startNewLine(builder)`.
    fn start_new_line(&self, builder: &mut StringBuilder) {
        if self.will_indent() {
            builder.append_character('\n' as u16);
            builder.append_utf16(&self.indent);
        }
    }

    /// `toJSON(baseValue, propertyName)`.
    fn to_json(&self, base_value: JSValue, property_name: &JsonKey) -> Result<JSValue, JsonError> {
        let to_json_key = JsonKey::Name(WtfString::from_latin1(b"toJSON"));
        let function = self.host.get(base_value, &to_json_key)?;
        if !self.host.is_callable(function) {
            return Ok(base_value);
        }
        let arguments = [property_name_value(self.host, property_name)];
        Ok(self.host.call(function, base_value, &arguments)?)
    }

    /// `appendStringifiedValue(builder, value, holder, propertyName)`.
    fn append_stringified_value(
        &mut self,
        builder: &mut StringBuilder,
        mut value: JSValue,
        holder: HolderInfo,
        property_name: &JsonKey,
    ) -> Result<StringifyResult, JsonError> {
        let host = self.host;

        // Chama o toJSON.
        if host.is_object(value) || is_big_int(value) {
            value = self.to_json(value, property_name)?;
        }

        // Chama o replacer.
        if self.callable_replacer {
            let arguments = [property_name_value(host, property_name), value];
            let this_value = holder.object.expect("holder.object() do replacer");
            value = host.call(self.replacer, this_value, &arguments)?;
        }

        if (value.is_undefined() || is_symbol(value)) && !holder.is_array {
            return Ok(StringifyFailedDueToUndefinedOrSymbolValue);
        }

        if host.is_object(value) {
            if let Some(raw_json) = host.raw_json_text(value) {
                builder.append_string(&raw_json);
                return Ok(StringifySucceeded);
            }
            value = unwrap_boxed_primitive(host, value)?;
        }

        if value.is_null() {
            builder.append_ascii_literal("null");
            return Ok(StringifySucceeded);
        }

        if value.is_boolean() {
            builder.append_ascii_literal(if value.is_true() { "true" } else { "false" });
            return Ok(StringifySucceeded);
        }

        if value.is_string() {
            builder.append_quoted_json_string(&value.as_js_string().value());
            return Ok(StringifySucceeded);
        }

        if value.is_number() {
            if let JSValue::Int32(integer) = value {
                builder.append_number_i32(integer);
            } else {
                let number = value.as_number();
                if !number.is_finite() {
                    builder.append_ascii_literal("null");
                } else {
                    builder.append_number_f64(number);
                }
            }
            return Ok(StringifySucceeded);
        }

        if is_big_int(value) {
            return Err(JsonError::Type(WtfString::from_latin1(b"JSON.stringify cannot serialize BigInt.")));
        }

        if !host.is_object(value) {
            return Ok(StringifyFailed);
        }

        if host.is_callable(value) {
            if holder.is_array {
                builder.append_ascii_literal("null");
                return Ok(StringifySucceeded);
            }
            return Ok(StringifyFailedDueToUndefinedOrSymbolValue);
        }

        if builder.has_overflowed() {
            return Ok(StringifyFailed);
        }

        // Detecção de ciclo, e o holder vai para a pilha.
        if self.holder_stack.iter().any(|stacked| stacked.object == Some(value)) {
            return Err(JsonError::Type(WtfString::from_latin1(
                b"JSON.stringify cannot serialize cyclic structures.",
            )));
        }

        if self.holder_stack.len() >= MAXIMUM_SIDE_STACK_RECURSION {
            return Err(JsonError::StackOverflow);
        }

        let holder_stack_was_empty = self.holder_stack.is_empty();
        let is_array = host.is_array(value)?;
        self.holder_stack.push(Holder { object: Some(value), is_array, index: 0, size: 0, property_names: Vec::new() });
        if !holder_stack_was_empty {
            return Ok(StringifySucceeded);
        }

        // A recursão é evitada pelo laço: só a primeira chamada (a da pilha vazia) o executa.
        loop {
            while self.append_next_property(builder)? {}
            if builder.has_overflowed() {
                return Ok(StringifyFailed);
            }
            self.holder_stack.pop();
            if self.holder_stack.is_empty() {
                break;
            }
        }
        Ok(StringifySucceeded)
    }

    /// `Holder::appendNextProperty(stringifier, builder)` sobre o holder do topo: `Ok(true)` se
    /// processou um elemento, `Ok(false)` quando o holder terminou.
    fn append_next_property(&mut self, builder: &mut StringBuilder) -> Result<bool, JsonError> {
        let host = self.host;
        let top = self.holder_stack.len() - 1;
        let object = self.holder_stack[top].object.expect("holder sem objeto");
        let is_array = self.holder_stack[top].is_array;

        // Primeira passada: inicializa.
        if self.holder_stack[top].index == 0 {
            if is_array {
                let length = host.length_of_array_like(object)?;
                if length > u32::MAX as u64 {
                    return Err(JsonError::OutOfMemory);
                }
                self.holder_stack[top].size = length as u32;
                builder.append_character('[' as u16);
            } else {
                let names = if self.using_array_replacer {
                    self.array_replacer_names.clone()
                } else {
                    host.own_enumerable_string_keys(object)?
                };
                self.holder_stack[top].size = names.len() as u32;
                self.holder_stack[top].property_names = names;
                builder.append_character('{' as u16);
            }
            self.indent();
        }
        if builder.has_overflowed() {
            return Ok(false);
        }

        // Última passada: fecha e devolve false.
        let index = self.holder_stack[top].index;
        let size = self.holder_stack[top].size;
        if index == size {
            self.unindent();
            if size != 0 && builder.char_at(builder.length() - 1) != '{' as u16 {
                self.start_new_line(builder);
            }
            let closing = if is_array { ']' } else { '}' };
            builder.append_character(closing as u16);
            return Ok(false);
        }

        // Trata um elemento do array ou do objeto.
        self.holder_stack[top].index += 1;
        let holder = HolderInfo { is_array, object: Some(object) };
        let mut roll_back_point = 0;
        let stringify_result;
        if is_array {
            let key = JsonKey::Index(index);
            let value = host.get(object, &key)?;

            // O separador.
            if index != 0 {
                builder.append_character(',' as u16);
            }
            self.start_new_line(builder);

            stringify_result = self.append_stringified_value(builder, value, holder, &key)?;
            debug_assert!(stringify_result != StringifyFailedDueToUndefinedOrSymbolValue);
        } else {
            let key = self.holder_stack[top].property_names[index as usize].clone();
            let value = host.get(object, &key)?;

            roll_back_point = builder.length();

            // O separador.
            if builder.char_at(roll_back_point - 1) != '{' as u16 {
                builder.append_character(',' as u16);
            }
            self.start_new_line(builder);

            // O nome da propriedade, os dois pontos e o espaço.
            builder.append_quoted_json_string(&key.to_wtf_string());
            builder.append_character(':' as u16);
            if self.will_indent() {
                builder.append_character(' ' as u16);
            }

            stringify_result = self.append_stringified_value(builder, value, holder, &key)?;
        }

        match stringify_result {
            StringifyFailed => builder.append_ascii_literal("null"),
            StringifySucceeded => {}
            // Só ocorre com `undefined` ou símbolo em propriedade de objeto: o separador e o nome já
            // escritos não devem ficar, então volta ao ponto anterior.
            StringifyFailedDueToUndefinedOrSymbolValue => builder.shrink(roll_back_point),
        }

        Ok(true)
    }
}

/// `Stringifier::stringify(globalObject, value, replacer, space)`: `Ok(None)` é a string nula do C++
/// (o `JSON.stringify` devolve `undefined`).
pub fn json_stringify<H: JsonHost + ?Sized>(
    host: &H,
    value: JSValue,
    replacer: JSValue,
    space: JSValue,
) -> Result<Option<WtfString>, JsonError> {
    let mut stringifier = Stringifier::new(host, replacer, space)?;

    let property_name = empty_key();

    // Com replacer que não é função, o objeto raiz não é observável e não precisa existir.
    let mut root_object = None;
    if stringifier.callable_replacer {
        let object = host.new_object()?;
        host.create_data_property(object, &empty_key(), value)?;
        root_object = Some(object);
    }

    let mut result = StringBuilder::with_overflow_policy(OverflowPolicy::RecordOverflow);
    let root = HolderInfo { is_array: false, object: root_object };
    let stringify_result = stringifier.append_stringified_value(&mut result, value, root, &property_name)?;
    if result.has_overflowed() {
        return Err(JsonError::OutOfMemory);
    }
    if stringify_result != StringifySucceeded {
        return Ok(None);
    }
    Ok(Some(result.to_string().clone()))
}

// ------------------------------ JSONObject --------------------------------

/// `enum WalkerState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WalkerState {
    StateUnknown,
    ArrayStartState,
    ArrayStartVisitMember,
    ArrayEndVisitMember,
    ObjectStartState,
    ObjectStartVisitMember,
    ObjectEndVisitMember,
}

/// `class Walker`: o `InternalizeJSONProperty` do `JSON.parse` com reviver.
struct Walker<'a, H: JsonHost + ?Sized> {
    host: &'a H,
    /// `m_source`.
    source: &'a WtfString,
    /// `m_function`.
    function: JSValue,
    /// `m_sourceRanges`.
    ranges: Option<&'a JsonRanges>,
    /// O `MarkedArgumentBuffer markStack` do C++ e os valores que lá vivem na pilha nativa (varrida pelo coletor): os
    /// holders empilhados, o valor lido e o devolvido pelo reviver, que só existem em locais enquanto ele roda JS.
    marked: crate::runtime::gc::RootScope,
}

impl<'a, H: JsonHost + ?Sized> Walker<'a, H> {
    /// `callReviver(thisObj, property, unfiltered, range)`.
    fn call_reviver(
        &self,
        this_object: JSValue,
        property: JSValue,
        unfiltered: JSValue,
        range: Option<&JsonRangeEntry>,
    ) -> Result<JSValue, JsonError> {
        let host = self.host;
        let mut context = None;
        if self.ranges.is_some() {
            let object = host.new_object()?;
            if let Some(range) = range {
                if !host.is_object(unfiltered) {
                    let substring = self.source.substring(range.begin, range.end - range.begin);
                    let value = JSValue::from_js_string(js_string(host.vm(), &substring));
                    host.create_data_property(object, &JsonKey::Name(WtfString::from_latin1(b"source")), value)?;
                }
            }
            context = Some(object);
        }

        if let Some(object) = context {
            self.marked.append(object);
        }
        let arguments = [property, unfiltered, context.unwrap_or(JSValue::Undefined)];
        let count = if context.is_some() { 3 } else { 2 };
        Ok(host.call(self.function, this_object, &arguments[..count])?)
    }

    /// O intervalo do filho `index` do array cujo intervalo está no topo de `entry_stack`.
    fn array_child_range(
        entry_stack: &[Option<&'a JsonRangeEntry>],
        index: u32,
        in_value: JSValue,
    ) -> Option<&'a JsonRangeEntry> {
        let last = (*entry_stack.last()?)?;
        let JsonRangeProperties::Array(children) = &last.properties else {
            return None;
        };
        let child = children.get(index as usize)?;
        if same_value(child.value, in_value) { Some(child) } else { None }
    }

    /// O intervalo do filho `key` do objeto cujo intervalo está no topo de `entry_stack`.
    fn object_child_range(
        entry_stack: &[Option<&'a JsonRangeEntry>],
        key: &JsonKey,
        in_value: JSValue,
    ) -> Option<&'a JsonRangeEntry> {
        let last = (*entry_stack.last()?)?;
        let JsonRangeProperties::Object(children) = &last.properties else {
            return None;
        };
        let child = children.get(&key.units())?;
        if same_value(child.value, in_value) { Some(child) } else { None }
    }

    /// `Walker::walk(unfiltered)`.
    fn walk(&self, unfiltered: JSValue) -> Result<JSValue, JsonError> {
        let host = self.host;
        let vm = host.vm();

        let mut property_stack: Vec<Vec<JsonKey>> = Vec::new();
        let mut index_stack: Vec<u32> = Vec::new();
        let mut mark_stack: Vec<JSValue> = Vec::new();
        let mut entry_stack: Vec<Option<&'a JsonRangeEntry>> = Vec::new();
        let mut array_length_stack: Vec<u32> = Vec::new();

        let mut state_stack: Vec<WalkerState> = Vec::new();
        let mut state = WalkerState::StateUnknown;
        let mut in_value = unfiltered;
        let root_range: Option<&'a JsonRangeEntry> = self.ranges.map(|ranges| ranges.root());
        let mut in_range = root_range;
        let mut out_value = JSValue::Null;
        let mut out_range = root_range;

        loop {
            match state {
                WalkerState::ArrayStartState => {
                    if mark_stack.len() >= MAXIMUM_SIDE_STACK_RECURSION {
                        return Err(JsonError::StackOverflow);
                    }

                    let array = in_value;
                    mark_stack.push(array);
                    self.marked.append(array);
                    if self.ranges.is_some() {
                        if let Some(range) = in_range {
                            if !matches!(range.properties, JsonRangeProperties::Array(_)) {
                                in_range = None;
                            }
                        }
                        entry_stack.push(in_range);
                    }
                    let length = host.length_of_array_like(array)?;
                    if length > u32::MAX as u64 {
                        return Err(JsonError::OutOfMemory);
                    }
                    array_length_stack.push(length as u32);
                    index_stack.push(0);
                    state = WalkerState::ArrayStartVisitMember;
                    continue;
                }
                WalkerState::ArrayStartVisitMember => {
                    let array = *mark_stack.last().expect("mark_stack vazia");
                    let index = *index_stack.last().expect("index_stack vazia");
                    let array_length = *array_length_stack.last().expect("array_length_stack vazia");
                    if index == array_length {
                        out_value = array;
                        if self.ranges.is_some() {
                            out_range = entry_stack.pop().expect("entry_stack vazia");
                        }
                        mark_stack.pop();
                        array_length_stack.pop();
                        index_stack.pop();
                    } else {
                        in_value = host.get(array, &JsonKey::Index(index))?;
                        self.marked.append(in_value);

                        if self.ranges.is_some() {
                            in_range = Self::array_child_range(&entry_stack, index, in_value);
                        }

                        if host.is_object(in_value) {
                            state_stack.push(WalkerState::ArrayEndVisitMember);
                            state = WalkerState::StateUnknown;
                            continue;
                        }
                        out_value = in_value;
                        out_range = in_range;
                        state = WalkerState::ArrayEndVisitMember;
                        continue;
                    }
                }
                WalkerState::ArrayEndVisitMember => {
                    let array = *mark_stack.last().expect("mark_stack vazia");
                    let index = *index_stack.last().expect("index_stack vazia");
                    let property = JSValue::from_js_string(js_string(vm, &WtfString::number_u32(index)));
                    self.marked.append(property);
                    let filtered_value = self.call_reviver(array, property, out_value, out_range)?;
                    self.marked.append(filtered_value);
                    if filtered_value.is_undefined() {
                        host.delete_property(array, &JsonKey::Index(index))?;
                    } else {
                        host.create_data_property(array, &JsonKey::Index(index), filtered_value)?;
                    }
                    *index_stack.last_mut().expect("index_stack vazia") += 1;
                    state = WalkerState::ArrayStartVisitMember;
                    continue;
                }
                WalkerState::ObjectStartState => {
                    if mark_stack.len() >= MAXIMUM_SIDE_STACK_RECURSION {
                        return Err(JsonError::StackOverflow);
                    }

                    let object = in_value;
                    mark_stack.push(object);
                    self.marked.append(object);
                    if self.ranges.is_some() {
                        if let Some(range) = in_range {
                            if !matches!(range.properties, JsonRangeProperties::Object(_)) {
                                in_range = None;
                            }
                        }
                        entry_stack.push(in_range);
                    }
                    index_stack.push(0);
                    property_stack.push(host.own_enumerable_string_keys(object)?);
                    state = WalkerState::ObjectStartVisitMember;
                    continue;
                }
                WalkerState::ObjectStartVisitMember => {
                    let object = *mark_stack.last().expect("mark_stack vazia");
                    let index = *index_stack.last().expect("index_stack vazia");
                    let property_count = property_stack.last().expect("property_stack vazia").len();
                    if index as usize == property_count {
                        out_value = object;
                        if self.ranges.is_some() {
                            out_range = entry_stack.pop().expect("entry_stack vazia");
                        }
                        mark_stack.pop();
                        index_stack.pop();
                        property_stack.pop();
                    } else {
                        let key = property_stack.last().expect("property_stack vazia")[index as usize].clone();
                        // O holder pode ser modificado pelo reviver, então qualquer leitura pode lançar.
                        in_value = host.get(object, &key)?;
                        self.marked.append(in_value);

                        if self.ranges.is_some() {
                            in_range = Self::object_child_range(&entry_stack, &key, in_value);
                        }

                        if host.is_object(in_value) {
                            state_stack.push(WalkerState::ObjectEndVisitMember);
                            state = WalkerState::StateUnknown;
                            continue;
                        }
                        out_value = in_value;
                        out_range = in_range;
                        state = WalkerState::ObjectEndVisitMember;
                        continue;
                    }
                }
                WalkerState::ObjectEndVisitMember => {
                    let object = *mark_stack.last().expect("mark_stack vazia");
                    let index = *index_stack.last().expect("index_stack vazia");
                    let key = property_stack.last().expect("property_stack vazia")[index as usize].clone();
                    let property = JSValue::from_js_string(js_string(vm, &key.to_wtf_string()));
                    self.marked.append(property);
                    let filtered_value = self.call_reviver(object, property, out_value, out_range)?;
                    self.marked.append(filtered_value);
                    if filtered_value.is_undefined() {
                        host.delete_property(object, &key)?;
                    } else {
                        // `createDataProperty(..., shouldThrow = false)`: a recusa não lança.
                        host.create_data_property(object, &key, filtered_value)?;
                    }
                    *index_stack.last_mut().expect("index_stack vazia") += 1;
                    state = WalkerState::ObjectStartVisitMember;
                    continue;
                }
                WalkerState::StateUnknown => {
                    if let Some(range) = in_range {
                        if !same_value(range.value, in_value) {
                            in_range = None;
                        }
                    }

                    if !host.is_object(in_value) {
                        out_value = in_value;
                        out_range = in_range;
                    } else {
                        let value_is_array = host.is_array(in_value)?;
                        state = if value_is_array { WalkerState::ArrayStartState } else { WalkerState::ObjectStartState };
                        continue;
                    }
                }
            }

            match state_stack.pop() {
                Some(next_state) => state = next_state,
                None => break,
            }
        }

        let final_holder = host.new_object()?;
        self.marked.append(final_holder);
        host.create_data_property(final_holder, &empty_key(), out_value)?;
        let empty_string = JSValue::from_js_string(js_empty_string(vm));
        self.call_reviver(final_holder, empty_string, out_value, out_range)
    }
}

/// `jsonProtoFuncParse` depois do `toString` do argumento: `reviver` é o segundo argumento, se veio
/// (`argumentCount >= 2`); só vale se for chamável. `use_source_text_access` é
/// `Options::useJSONSourceTextAccess()` (o terceiro argumento `context.source` do reviver).
pub fn json_parse<H: JsonHost + ?Sized>(
    host: &H,
    text: &WtfString,
    reviver: Option<JSValue>,
    use_source_text_access: bool,
) -> Result<JSValue, JsonError> {
    let function = reviver.filter(|function| host.is_callable(*function));

    // `jsonParseSlow` registra os intervalos de fonte; o caminho sem reviver não.
    let mut ranges = JsonRanges::default();
    let track_ranges = function.is_some() && use_source_text_access;
    let outcome = literal_parse(host, text, LiteralParseKind::Strict, if track_ranges { Some(&mut ranges) } else { None })?;
    let unfiltered = match outcome {
        ParseOutcome::Value(value) => value,
        ParseOutcome::Failed(message) => return Err(JsonError::Syntax(message)),
    };

    let Some(function) = function else {
        return Ok(unfiltered);
    };
    let marked = crate::runtime::gc::RootScope::new(&[unfiltered, function]);
    let walker = Walker { host, source: text, function, ranges: if track_ranges { Some(&ranges) } else { None }, marked };
    walker.walk(unfiltered)
}

/// `JSONParse(globalObject, json)`: `Ok(None)` é o `JSValue()` vazio (texto nulo ou parse que falhou,
/// sem lançar o `SyntaxError`).
pub fn json_parse_quiet<H: JsonHost + ?Sized>(host: &H, text: &WtfString) -> Result<Option<JSValue>, JsonError> {
    if text.is_null() {
        return Ok(None);
    }
    Ok(match literal_parse(host, text, LiteralParseKind::Strict, None)? {
        ParseOutcome::Value(value) => Some(value),
        ParseOutcome::Failed(_) => None,
    })
}

/// A validação do `jsonProtoFuncRawJSON` (https://tc39.es/proposal-json-parse-with-source/#sec-json.rawjson)
/// sobre o texto do `toString` do argumento; a criação do `JSRawJSONObject` é do host.
pub fn validate_raw_json<H: JsonHost + ?Sized>(host: &H, text: &WtfString) -> Result<(), JsonError> {
    let is_json_white_space = |character: u16| matches!(character, 0x0009 | 0x000A | 0x000D | 0x0020);

    let units = wtf_string_to_units(text);
    if units.is_empty() {
        return Err(JsonError::Syntax(WtfString::from_latin1(b"JSON.rawJSON cannot accept empty string")));
    }

    let character_message = |position: &str, character: u16| {
        let mut message: Vec<u16> =
            format!("JSON.rawJSON cannot accept string {position} '").bytes().map(|byte| byte as u16).collect();
        message.push(character);
        message.push('\'' as u16);
        units_to_wtf_string(&message)
    };

    let first_character = units[0];
    if is_json_white_space(first_character) {
        return Err(JsonError::Syntax(character_message("starting with", first_character)));
    }

    let last_character = units[units.len() - 1];
    if is_json_white_space(last_character) {
        return Err(JsonError::Syntax(character_message("ending with", last_character)));
    }

    match literal_parse(host, text, LiteralParseKind::StrictPrimitive, None)? {
        ParseOutcome::Value(_) => Ok(()),
        ParseOutcome::Failed(message) => Err(JsonError::Syntax(message)),
    }
}
