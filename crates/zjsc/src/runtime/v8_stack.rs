//! O formato de pilha do V8 (`CallSiteInfo`, `ErrorUtils::FormatStackTrace`), que o programa de navegador liga com
//! [`set_v8_stack_format`](crate::runtime::stack_frame::set_v8_stack_format). Tudo o que aqui está foi medido no
//! Chromium 154 do oráculo (`tests/golden/v8_stack`); o modo do bun não passa por este módulo.
//!
//! - A posição de cada frame é a que o V8 guarda no `SourcePosition` da instrução: o `new` de uma construção, o
//!   início do identificador ou do nome de propriedade chamado, o nome lido depois do `.`; sem nome antes do `(`
//!   (`(0, eval)(...)`, `f()()`) fica o `(`. O JSC guarda o `divot` (o `(`, o `.`, o fim do identificador), e
//!   [`position_shift`] leva de um ao outro.
//! - A linha do frame é `Tipo.nome (url:linha:coluna)` na chamada de método, `nome (...)` na chamada sem receptor,
//!   `new nome (...)` na construção e só `url:linha:coluna` no código de topo e na função anônima sem receptor. A
//!   função nativa (e o builtin que o JSC escreve em JS, como `map` e `reduce`) não tem posição: `(<anonymous>)`.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::bytecode::bytecode_index::BytecodeIndex;
use crate::bytecode::code_block::CodeBlock;
use crate::bytecode::opcode::OpcodeID;
use crate::host_function;
use crate::interpreter::call_frame::CallFrame;
use crate::interpreter::stack_visitor::to_rust_string;
use crate::runtime::error_instance::ErrorInstance;
use crate::runtime::error_prototype::error_to_string;
use crate::runtime::error_type::ErrorType;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_call_site::prepare_stack_trace;
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction};
use crate::runtime::js_getter_setter::{GetterSetter, GetterSetterRef};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_string::js_string;
use crate::runtime::js_type::JSType;
use crate::runtime::js_value::JSValue;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{ACCESSOR as ACCESSOR_ATTRIBUTE, DONT_ENUM};
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::stack_frame::{is_line_terminator_unit, StackFrame};
use crate::runtime::string_regexp_support::to_wtf_string_value;
use crate::wtf::text::wtf_string::String as WtfString;

fn is_identifier_unit(unit: u16) -> bool {
    unit == u16::from(b'_') || unit == u16::from(b'$') || unit >= 0x80 || (unit < 0x80 && (unit as u8).is_ascii_alphanumeric())
}

fn is_space_unit(unit: u16) -> bool {
    unit == u16::from(b' ') || unit == u16::from(b'\t') || unit == 0xA0 || unit == 0xFEFF
}

fn is_binary_operator(opcode: OpcodeID) -> bool {
    matches!(
        opcode,
        OpcodeID::op_add
            | OpcodeID::op_sub
            | OpcodeID::op_mul
            | OpcodeID::op_div
            | OpcodeID::op_mod
            | OpcodeID::op_pow
            | OpcodeID::op_lshift
            | OpcodeID::op_rshift
            | OpcodeID::op_urshift
            | OpcodeID::op_bitand
            | OpcodeID::op_bitor
            | OpcodeID::op_bitxor
            | OpcodeID::op_less
            | OpcodeID::op_lesseq
            | OpcodeID::op_greater
            | OpcodeID::op_greatereq
    )
}

/// Quantas colunas (com sinal) separam o `divot` que o JSC guarda para a instrução do frame da posição que o V8
/// mostra, sem sair da linha do `divot`.
pub(crate) fn position_shift(block: &CodeBlock, bytecode_index: BytecodeIndex) -> i64 {
    let source = block.owner_executable().source();
    let Some(provider) = source.provider() else { return 0 };
    let text = provider.source();
    let info = block.unlinked_code_block().borrow().expression_info_for_bytecode_index(bytecode_index);
    let source_start = u32::try_from(source.start_offset()).unwrap_or(0);
    let divot = source_start.saturating_add(info.divot);
    let length = text.length();
    if divot > length {
        return 0;
    }
    let unit = |index: u32| text.code_unit_at(index);
    let mut line_start = divot;
    while line_start > source_start && !is_line_terminator_unit(unit(line_start - 1)) {
        line_start -= 1;
    }
    let opcode = block.instructions().at(bytecode_index.offset()).opcode_id_enum();
    let unit_at = |index: u32| (index < length).then(|| unit(index));
    let skip_spaces = |mut position: u32| {
        while unit_at(position).is_some_and(is_space_unit) {
            position += 1;
        }
        position
    };
    let expression_start = divot - info.start_offset.min(divot - line_start);
    if matches!(
        opcode,
        OpcodeID::op_construct | OpcodeID::op_construct_varargs | OpcodeID::op_super_construct | OpcodeID::op_super_construct_varargs
    ) {
        // `startOffset` leva do `(` ao `new`.
        return -i64::from(divot - expression_start);
    }
    // `new Array(-1)` e outras construções que o JSC compila sem `op_construct`: o `new` do começo da expressão.
    let starts_with_new = (0..3).all(|offset| unit_at(expression_start + offset) == Some(u16::from(b"new"[offset as usize])))
        && unit_at(expression_start + 3).is_some_and(|after| !is_identifier_unit(after))
        && !(expression_start..divot).any(|index| unit(index) == u16::from(b'('));
    if unit_at(divot) == Some(u16::from(b'(')) && starts_with_new {
        return -i64::from(divot - expression_start);
    }
    // Atribuição que lança (`o.a = 2` congelado, `c = 2` constante): o V8 aponta o `=`.
    if matches!(opcode, OpcodeID::op_put_by_id | OpcodeID::op_put_by_val | OpcodeID::op_put_by_val_direct | OpcodeID::op_put_to_scope | OpcodeID::op_throw_static_error) {
        let mut position = divot;
        while unit_at(position).is_some_and(|current| current == u16::from(b'.') || is_space_unit(current) || is_identifier_unit(current)) {
            position += 1;
        }
        if unit_at(position) == Some(u16::from(b'=')) && unit_at(position + 1) != Some(u16::from(b'=')) {
            return i64::from(position - divot);
        }
    }
    // Leitura de propriedade: o V8 aponta o nome depois do `.` (ou do `?.`).
    if unit_at(divot) == Some(u16::from(b'.')) {
        return i64::from(skip_spaces(divot + 1) - divot);
    }
    if unit_at(divot) == Some(u16::from(b'?')) && unit_at(divot + 1) == Some(u16::from(b'.')) {
        return i64::from(skip_spaces(divot + 2) - divot);
    }
    // Identificador que termina no `divot` (o chamado, a variável lida): o início dele. O `[` de um acesso por índice
    // fica onde está.
    if unit_at(divot) == Some(u16::from(b'[')) {
        // `const [a] = {}`: o V8 aponta o valor desestruturado, depois do `=`.
        let mut position = divot;
        let mut depth = 0i32;
        while let Some(current) = unit_at(position) {
            depth += i32::from(current == u16::from(b'[')) - i32::from(current == u16::from(b']'));
            position += 1;
            if depth == 0 {
                break;
            }
        }
        let position = skip_spaces(position);
        if unit_at(position) == Some(u16::from(b'=')) && unit_at(position + 1) != Some(u16::from(b'=')) {
            return i64::from(skip_spaces(position + 1) - divot);
        }
        return 0;
    }
    // `for (const x of 5)`: o V8 aponta o iterável, depois do `of`.
    if unit_at(divot) == Some(u16::from(b'o'))
        && unit_at(divot + 1) == Some(u16::from(b'f'))
        && unit_at(divot + 2).is_some_and(|after| !is_identifier_unit(after))
    {
        return i64::from(skip_spaces(divot + 2) - divot);
    }
    // Operador binário que lança (`1n + 1`, `Symbol() + ''`): o JSC guarda o começo do operando da esquerda, o V8 o
    // operador.
    if is_binary_operator(opcode) {
        let mut position = divot;
        let mut depth = 0i32;
        let mut seen_operand = false;
        while let Some(current) = unit_at(position) {
            if is_line_terminator_unit(current) {
                break;
            }
            let character = char::from_u32(u32::from(current)).unwrap_or('\0');
            match character {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                '+' | '-' | '*' | '/' | '%' | '&' | '|' | '^' | '<' | '>' if depth == 0 && seen_operand => {
                    return i64::from(position - divot);
                }
                _ => {}
            }
            seen_operand |= !is_space_unit(current);
            position += 1;
        }
        return 0;
    }
    let mut start = divot;
    while start > line_start && is_identifier_unit(unit(start - 1)) {
        start -= 1;
    }
    -i64::from(divot - start)
}

/// A propriedade de dados `name` ao longo da cadeia de protótipos de `object`, sem chamar getter (o
/// `JSReceiver::GetDataProperty` do V8).
fn data_property(global_object: &JSGlobalObject, object: &ObjectRef, name: &PropertyName) -> Option<JSValue> {
    let mut current = object.as_value();
    loop {
        let candidate = ObjectRef::from_value(&current)?;
        let mut slot = PropertySlot::new(current, InternalMethodType::VMInquiry);
        if candidate.get_own_property_slot(global_object, name, &mut slot) {
            return (!slot.is_accessor()).then(|| slot.get_value_for(name));
        }
        current = candidate.get_prototype_direct();
    }
}

fn non_empty_string(value: Option<JSValue>) -> Option<String> {
    let value = value.filter(JSValue::is_string)?;
    let text = crate::interpreter::stack_visitor::to_rust_string(&value.to_wtf_string());
    (!text.is_empty()).then_some(text)
}

/// O nome de uma função para o `Tipo.` da linha do frame: a propriedade de dados `name`.
fn function_name(global_object: &JSGlobalObject, function: &ObjectRef) -> Option<String> {
    let vm = global_object.vm();
    non_empty_string(data_property(global_object, function, &PropertyName::from_identifier(&vm.property_names.name)))
}

/// `CallSiteInfo::GetTypeName` / `JSReceiver::GetConstructorName` do receptor do frame: `None` no código de topo
/// (receptor `undefined`, `null` ou o objeto global), na construção e no frame assíncrono.
pub fn type_name(global_object: &JSGlobalObject, frame: &StackFrame) -> Option<String> {
    if frame.is_constructor || frame.is_async {
        return None;
    }
    // A arrow function não tem receptor próprio: o V8 a escreve como chamada de topo (`at t (...)`).
    let callee = JSValue::from_cell(frame.callee);
    if frame.callee != 0
        && callee.as_js_function().is_some_and(|function| !function.is_host_function() && function.js_executable().borrow().is_arrow_function())
    {
        return None;
    }
    let this = frame.this_value;
    if this.is_undefined() || this.is_null() || this.is_empty() {
        return None;
    }
    if this.is_number() {
        return Some("Number".to_string());
    }
    if this.is_string() {
        return Some("String".to_string());
    }
    if this.is_boolean() {
        return Some("Boolean".to_string());
    }
    if this.is_symbol() {
        return Some("Symbol".to_string());
    }
    if this.is_big_int() {
        return Some("BigInt".to_string());
    }
    let object = ObjectRef::from_value(&this)?;
    if matches!(object.type_(), JSType::GlobalObjectType | JSType::GlobalProxyType) {
        return None;
    }
    let vm = global_object.vm();
    if this.is_callable() {
        return Some(function_name(global_object, &object).unwrap_or_else(|| "Function".to_string()));
    }
    let tag = data_property(global_object, &object, &PropertyName::from_identifier(&vm.property_names.to_string_tag_symbol));
    if let Some(tag) = non_empty_string(tag) {
        return Some(tag);
    }
    let constructor = data_property(global_object, &object, &PropertyName::from_identifier(&vm.property_names.constructor));
    if let Some(constructor) = constructor.and_then(|constructor| ObjectRef::from_value(&constructor)) {
        if let Some(name) = function_name(global_object, &constructor) {
            return Some(name);
        }
    }
    Some("Object".to_string())
}

/// A posição entre parênteses da linha do frame: `url:L:C`, `<anonymous>` no nativo e, no código de `eval` e
/// `new Function`, `eval at f (origem), <anonymous>:L:C`.
fn position_text(frame: &StackFrame) -> String {
    let location = frame.location();
    if !location.has_line_and_column_info || location.source_url == "native" {
        return "<anonymous>".to_string();
    }
    if let Some(origin) = frame.eval_origin() {
        return format!("{origin}, <anonymous>:{}:{}", location.line, location.column);
    }
    if location.source_url.is_empty() {
        return format!("<anonymous>:{}:{}", location.line, location.column);
    }
    format!("{}:{}:{}", location.source_url, location.line, location.column)
}

/// A origem que o fonte de um `eval` ou `new Function` criado agora guarda: `eval at f (posição)` do frame JS
/// que o chamou, a partir de `call_frame` (o frame nativo de partida não conta).
pub fn eval_origin(global_object: &JSGlobalObject, call_frame: CallFrame) -> String {
    let vm = global_object.vm();
    let frames = vm.interpreter().get_stack_trace(vm, call_frame, 1, None, false);
    match frames.first() {
        Some(caller) => {
            let name = match caller.function_name() {
                "" | "global code" => "<anonymous>",
                "eval code" => "eval",
                name => name,
            };
            format!("eval at {name} ({})", position_text(caller))
        }
        None => "eval at <anonymous>".to_string(),
    }
}

/// Os métodos do `CallSite` do V8 que diferem do bun (`CallSiteInfo`): `None` deixa o do bun responder.
pub fn call_site_method(global_object: &JSGlobalObject, frame: &StackFrame, method: &str) -> Option<JSValue> {
    let vm = global_object.vm();
    let string = |text: &str| JSValue::from_js_string(js_string(vm, &WtfString::from_utf8(text.as_bytes())));
    let location = frame.location();
    let has_position = location.has_line_and_column_info && location.source_url != "native";
    let from_eval = frame.eval_origin().is_some();
    let type_name = type_name(global_object, frame);
    let function_name = match location.function_name.as_str() {
        "" | "global code" | "eval code" => None,
        "anonymous" if from_eval => None,
        name => Some(name.strip_prefix("async ").unwrap_or(name)).filter(|name| !name.is_empty()),
    };
    let optional = |text: Option<&str>| text.map_or(JSValue::null(), string);
    let file_name = || if has_position && !from_eval && !location.source_url.is_empty() { string(&location.source_url) } else { JSValue::undefined() };
    Some(match method {
        "getFileName" | "getScriptNameOrSourceURL" => file_name(),
        "getLineNumber" => if has_position { crate::runtime::js_value::js_number(f64::from(location.line)) } else { JSValue::null() },
        "getColumnNumber" => if has_position { crate::runtime::js_value::js_number(f64::from(location.column)) } else { JSValue::null() },
        "getFunctionName" => optional(function_name),
        "getTypeName" => optional(type_name.as_deref()),
        "getMethodName" => optional(type_name.as_ref().and(function_name).map(|name| name.rsplit('.').next().unwrap_or(name).trim_start_matches("get ").trim_start_matches("set "))),
        "getEvalOrigin" => frame.eval_origin().map_or(JSValue::undefined(), |origin| string(&origin)),
        "isToplevel" => crate::runtime::js_value::js_boolean(type_name.is_none() && !frame.is_constructor),
        "isNative" => crate::runtime::js_value::js_boolean(false),
        "isEval" => crate::runtime::js_value::js_boolean(from_eval && frame.code_type == crate::bytecode::code_type::CodeType::EvalCode),
        "toString" => string(&call_site_text(frame, type_name.as_deref())),
        _ => return None,
    })
}

/// `Number::toString` de `value`, como as mensagens do V8 o escrevem (`Invalid count value: -1`).
pub fn number_text(value: f64) -> String {
    to_rust_string(&crate::runtime::number_prototype::to_string_with_radix(value, 10))
}

/// O texto de `(evaluating '...')` / `(In '...'` de uma mensagem do JSC.
fn quoted_after<'a>(message: &'a str, marker: &str) -> Option<&'a str> {
    let start = message.find(marker)? + marker.len();
    let rest = &message[start..];
    Some(&rest[..rest.find("')").or_else(|| rest.find('\''))?])
}

/// As partes de uma expressão separadas pelos `.` de fora de parênteses, colchetes e chaves.
fn top_level_parts(expression: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (index, character) in expression.char_indices() {
        match character {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            '.' if depth == 0 => {
                parts.push(&expression[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&expression[start..]);
    parts
}

/// O chamado como o `CallPrinter` do V8 o escreve: o caminho de propriedades de um identificador, e
/// `(intermediate value)` no lugar de uma expressão primária que não é identificador.
fn callee_text(expression: &str) -> String {
    let mut expression = expression.trim();
    // Os parênteses que embrulham o chamado inteiro (`new (a.b)()`).
    while expression.starts_with('(') && expression.ends_with(')') {
        let inner = &expression[1..expression.len() - 1];
        let mut depth = 0i32;
        if inner.chars().any(|character| {
            depth += match character {
                '(' => 1,
                ')' => -1,
                _ => 0,
            };
            depth < 0
        }) {
            break;
        }
        expression = inner.trim();
    }
    let parts = top_level_parts(expression);
    let first = parts[0].trim();
    let primary = if !first.is_empty() && first.chars().all(|character| character.is_alphanumeric() || character == '_' || character == '$') {
        first.to_string()
    } else {
        "(intermediate value)".to_string()
    };
    std::iter::once(primary).chain(parts[1..].iter().map(|part| part.trim().to_string())).collect::<Vec<_>>().join(".")
}

/// A mensagem do V8 para um erro que o motor cria com a mensagem `message` do JSC, quando o texto do JSC basta
/// para escrevê-la (medido no Chromium 154, `tests/golden/v8_stack`). `None` deixa a mensagem como está.
pub fn v8_message(error_type: ErrorType, message: &str) -> Option<String> {
    let fixed = match (error_type, message) {
        (ErrorType::RangeError, "Array length must be a positive integer of safe magnitude.") => "Invalid array length",
        (ErrorType::RangeError, "Maximum call stack size exceeded.") => "Maximum call stack size exceeded",
        (ErrorType::RangeError, "toFixed() argument must be between 0 and 100") => "toFixed() digits argument must be between 0 and 100",
        (ErrorType::URIError, "URI error") => "URI malformed",
        (ErrorType::TypeError, "Cannot convert a symbol to a string") => "Cannot convert a Symbol value to a string",
        (ErrorType::TypeError, "Properties can only be defined on Objects.") => "Object.defineProperty called on non-object",
        (ErrorType::TypeError, "reduce of empty array with no initial value") => "Reduce of empty array with no initial value",
        (ErrorType::TypeError, "A Proxy's 'target' should be an Object" | "A Proxy's 'handler' should be an Object") => {
            "Cannot create proxy with a non-object as target or handler"
        }
        (ErrorType::SyntaxError, "Unexpected end of script") => "Unexpected end of input",
        (ErrorType::SyntaxError, "Return statements are only valid inside functions.") => "Illegal return statement",
        (ErrorType::SyntaxError, "Function statements must have a name.") => "Function statements require a function name",
        (ErrorType::SyntaxError, "Class statements must have a name.") => "Unexpected token '{'",
        (ErrorType::SyntaxError, "Unexpected keyword 'let'. Cannot use 'let' as an identifier name for a LexicalDeclaration.") => {
            "let is disallowed as a lexically bound name"
        }
        (ErrorType::SyntaxError, "JSON Parse error: Unexpected EOF") => "Unexpected end of JSON input",
        _ => "",
    };
    if !fixed.is_empty() {
        return Some(fixed.to_string());
    }
    if error_type == ErrorType::TypeError && message.starts_with("Invalid mix of BigInt and other type in ") {
        return Some("Cannot mix BigInt and other types, use explicit conversions".to_string());
    }
    if error_type == ErrorType::ReferenceError && message.starts_with("Cannot access '") && message.ends_with(" before initialization.") {
        return Some(message[..message.len() - 1].to_string());
    }
    if error_type != ErrorType::TypeError {
        return None;
    }
    // `o.y is not a function. (In 'o.y()', 'o.y' is undefined)`.
    if let Some(cut) = message.find(" is not a function. (In '") {
        return Some(format!("{} is not a function", &message[..cut]));
    }
    // `function is not a constructor (evaluating 'new Math.max()')`.
    if message.contains(" is not a constructor (evaluating 'new ") {
        let expression = quoted_after(message, "(evaluating 'new ")?;
        let callee = expression.strip_suffix(')').and_then(|rest| {
            let mut depth = 0i32;
            rest.char_indices().rev().find_map(|(index, character)| {
                match character {
                    ')' => depth += 1,
                    '(' if depth == 0 => return Some(&rest[..index]),
                    '(' => depth -= 1,
                    _ => {}
                }
                None
            })
        })?;
        return Some(format!("{} is not a constructor", callee_text(callee)));
    }
    // `undefined is not an object (evaluating 'o.a.b')`.
    for base in ["undefined", "null"] {
        let Some(rest) = message.strip_prefix(base).and_then(|rest| rest.strip_prefix(" is not an object (evaluating '")) else {
            continue;
        };
        let expression = rest.strip_suffix("')")?;
        if expression.ends_with("[Symbol.iterator]") {
            return Some(format!("object {base} is not iterable (cannot read property Symbol(Symbol.iterator))"));
        }
        let (target, verb) = match expression.split_once(" = ") {
            Some((target, _)) => (target, ("set", "setting")),
            None => (expression, ("read", "reading")),
        };
        let property = top_level_parts(target).last().copied().filter(|_| top_level_parts(target).len() > 1)?;
        if !property.chars().all(|character| character.is_alphanumeric() || character == '_' || character == '$') {
            return None;
        }
        return Some(format!("Cannot {} properties of {base} ({} '{property}')", verb.0, verb.1));
    }
    None
}

/// O texto depois do `at ` (`CallSiteInfo` serializado por `SerializeJSStackFrame`), com o `type_name` que
/// [`type_name`] calculou.
pub fn call_site_text(frame: &StackFrame, type_name: Option<&str>) -> String {
    let location = frame.location();
    let position = position_text(frame);
    let from_eval = frame.eval_origin().is_some();
    let name = match location.function_name.as_str() {
        "" | "global code" | "async " => None,
        "eval code" => Some("eval"),
        // A função de `new Function` se chama `anonymous`, mas o V8 a escreve como `eval`.
        "anonymous" if from_eval => Some("eval"),
        name => Some(name),
    };
    let text = if frame.is_constructor {
        format!("new {}", name.unwrap_or("<anonymous>"))
    } else if let Some(type_name) = type_name {
        match name {
            None => format!("{type_name}.<anonymous>"),
            Some(name) if name.starts_with("get ") || name.starts_with("set ") || name.starts_with(&format!("{type_name}.")) => name.to_string(),
            Some(name) => format!("{type_name}.{name}"),
        }
    } else if let Some(name) = name {
        name.to_string()
    } else if frame.is_async {
        "async <anonymous>".to_string()
    } else {
        return position;
    };
    format!("{text} ({position})")
}

/// `ErrorUtils::FormatStackTrace` sem `Error.prepareStackTrace`: o cabeçalho e um `    at ` por frame.
pub fn stack_text(global_object: &JSGlobalObject, header: &str, frames: &[StackFrame]) -> String {
    let mut text = header.to_string();
    for frame in frames {
        text.push_str("\n    at ");
        text.push_str(&call_site_text(frame, type_name(global_object, frame).as_deref()));
    }
    text
}

/// O que o acessor `stack` de um objeto guarda (o `error_stack_symbol` do V8): os frames ainda não formatados
/// (`Error.captureStackTrace` num objeto que não é `ErrorInstance`; o `ErrorInstance` os guarda nele mesmo) ou o
/// valor já formatado ou atribuído.
enum Stored {
    Frames(Vec<StackFrame>),
    Value(JSValue),
}

thread_local! {
    static STORED: RefCell<HashMap<usize, Stored>> = RefCell::new(HashMap::new());
    /// O par getter e setter compartilhado por todos os `stack` do programa (`d.get === d2.get`, medido).
    static ACCESSOR: RefCell<Option<GetterSetterRef>> = const { RefCell::new(None) };
}

/// Fim do programa: esquece os estados e o acessor.
pub(crate) fn reset_for_program() {
    let _ = STORED.try_with(|stored| stored.borrow_mut().clear());
    let _ = ACCESSOR.try_with(|accessor| accessor.borrow_mut().take());
}

/// O estado guardado é a propriedade privada do objeto no V8: vive enquanto o objeto vive. O passo de efêmeros do
/// coletor marca o que o estado de objeto marcado segura e devolve se marcou algo novo.
pub(crate) fn visit_ephemerons(marker: &mut crate::runtime::gc::Marker) -> bool {
    let Ok(Ok(stored)) = STORED.try_with(|stored| stored.try_borrow().map(|stored| {
        stored.iter().filter(|(cell_id, _)| !marker.is_doomed(**cell_id)).map(|(_, state)| match state {
            Stored::Frames(frames) => frames.iter().flat_map(|frame| [JSValue::from_cell(frame.callee), frame.this_value]).collect(),
            Stored::Value(value) => vec![*value],
        }).collect::<Vec<Vec<JSValue>>>()
    })) else {
        marker.abort();
        return false;
    };
    let mut progressed = false;
    for value in stored.into_iter().flatten() {
        if let JSValue::Cell(cell_id) = value {
            if marker.is_doomed(cell_id) {
                marker.cell(cell_id);
                progressed = true;
            }
        }
    }
    progressed
}

/// O estado do objeto que a varredura vai soltar sai com ele.
pub(crate) fn remove_dead(marker: &crate::runtime::gc::Marker) {
    let _ = STORED.try_with(|stored| stored.borrow_mut().retain(|cell_id, _| !marker.is_doomed(*cell_id)));
}

fn accessor_function(global_object: &JSGlobalObject, function: NativeFunction) -> JSValue {
    JSFunction::create_native(
        global_object.vm(),
        global_object,
        0,
        &WtfString::default(),
        function,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    )
    .as_value()
}

fn shared_accessor(global_object: &JSGlobalObject) -> GetterSetterRef {
    ACCESSOR.with(|accessor| {
        accessor
            .borrow_mut()
            .get_or_insert_with(|| {
                let getter = accessor_function(global_object, error_stack_getter as NativeFunction);
                let setter = accessor_function(global_object, error_stack_setter as NativeFunction);
                GetterSetter::create_from_values(global_object.vm(), getter, setter)
            })
            .clone()
    })
}

/// Instala em `object` a propriedade `stack` do V8: acessor, não enumerável, configurável.
pub fn install_stack_accessor(global_object: &JSGlobalObject, object: &JSObject) {
    let vm = global_object.vm();
    let _ = object.put_direct_accessor(vm, &PropertyName::from_identifier(&vm.property_names.stack), shared_accessor(global_object), ACCESSOR_ATTRIBUTE | DONT_ENUM);
}

/// `Error.captureStackTrace(object)` no V8: o acessor e os frames, formatados na primeira leitura.
pub fn capture_into(global_object: &JSGlobalObject, object: &JSObject, frames: Option<Vec<StackFrame>>) {
    let vm = global_object.vm();
    let stack = PropertyName::from_identifier(&vm.property_names.stack);
    let mut descriptor = PropertyDescriptor::default();
    descriptor.set_accessor_descriptor_from(&shared_accessor(global_object), ACCESSOR_ATTRIBUTE | DONT_ENUM);
    let _ = object.define_own_property(vm, &stack, &descriptor, false);
    let cell_id = object.cell_id();
    if let Some(error) = ErrorInstance::from_cell_id(cell_id) {
        STORED.with(|stored| stored.borrow_mut().remove(&cell_id));
        match frames {
            Some(frames) => error.replace_pending_stack(frames),
            None => store(cell_id, Stored::Value(JSValue::undefined())),
        }
        return;
    }
    store(cell_id, frames.map_or(Stored::Value(JSValue::undefined()), Stored::Frames));
}

fn store(cell_id: usize, value: Stored) {
    STORED.with(|stored| stored.borrow_mut().insert(cell_id, value));
}

/// O cabeçalho do `stack` (`ErrorUtils::ToString`): o `name` lido agora; o `message` do `ErrorInstance` é o da
/// construção (atribuir `e.message` depois não muda o cabeçalho, medido), o dos demais objetos é lido agora.
fn header(global_object: &JSGlobalObject, object: &ObjectRef) -> Result<WtfString, Thrown> {
    let vm = global_object.vm();
    let read = |name: &PropertyName| -> Result<Option<WtfString>, Thrown> {
        let value = object.get(global_object, name);
        if vm.exception().is_some() {
            return Err(Thrown::Pending);
        }
        if value.is_undefined() {
            return Ok(None);
        }
        to_wtf_string_value(global_object, value).map(Some)
    };
    let name = read(&PropertyName::from_identifier(&vm.property_names.name))?;
    let message = match ErrorInstance::from_cell_id(object.cell_id()) {
        Some(error) => Some(error.message()),
        None => read(&PropertyName::from_identifier(&vm.property_names.message))?,
    };
    error_to_string(name.as_ref(), message.as_ref()).map_err(|_| Thrown::OutOfMemory)
}

fn stack_get_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(object) = ObjectRef::from_value(&call.this_value()) else {
        return Ok(JSValue::undefined());
    };
    let cell_id = object.cell_id();
    let removed = STORED.with(|stored| stored.borrow_mut().remove(&cell_id));
    let frames = match removed {
        Some(Stored::Value(value)) => {
            store(cell_id, Stored::Value(value));
            return Ok(value);
        }
        Some(Stored::Frames(frames)) => frames,
        None => match ErrorInstance::from_cell_id(cell_id).and_then(|error| error.take_pending_stack()) {
            Some(frames) => frames,
            None => return Ok(JSValue::undefined()),
        },
    };
    let value = match prepare_stack_trace(global_object, object.as_value(), &frames) {
        Err(_) => return Err(Thrown::Pending),
        Ok(Some(prepared)) => prepared,
        Ok(None) => {
            let header = header(global_object, &object)?;
            let text = stack_text(global_object, &to_rust_string(&header), &frames);
            JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf8(text.as_bytes())))
        }
    };
    store(cell_id, Stored::Value(value));
    Ok(value)
}

fn stack_set_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if let Some(object) = ObjectRef::from_value(&call.this_value()) {
        let cell_id = object.cell_id();
        if let Some(error) = ErrorInstance::from_cell_id(cell_id) {
            error.take_pending_stack();
        }
        store(cell_id, Stored::Value(call.argument(0)));
    }
    Ok(JSValue::undefined())
}

host_function!(error_stack_getter, stack_get_body);
host_function!(error_stack_setter, stack_set_body);
