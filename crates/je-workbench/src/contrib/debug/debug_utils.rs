//! Porte parcial de `src/vs/workbench/contrib/debug/common/debugUtils.ts`: o `isUriString` (que `debugSource` usa),
//! o `isDebuggerMainContribution` e a conversão dos caminhos das mensagens do protocolo (`convertToDAPaths` e
//! `convertToVSCPaths`), que o host de extensões e a thread principal do `debug` usam.
//!
//! Notas de tradução:
//! - As mensagens do protocolo trazem `arguments` e `body` como [`serde_json::Value`]. O `source.path` que vira URI no
//!   original (`<string><unknown>uri.parse(...)`, um objeto `URI` no lugar do texto) é o objeto JSON do `URI#toJSON`
//!   (`$mid`, `scheme`, `path`...), e o `uriToString` o revive, como o `typeof source.path === 'object'` do original.
//! - O `deepClone` da mensagem é o `clone`. Um campo que o original lê sem checar (`body.source` de um evento sem `body`,
//!   `forEach` de uma lista ausente) lança `TypeError` no JavaScript; aqui lança o mesmo erro (pânico).
//! - `source.path = fixPath(source)` com resultado `undefined` remove a chave `path` (no JSON os dois são iguais).
//! - O `IDebuggerContribution` é o `DebuggerContribution` do manifesto; o `program` que ele não declara fica em `extra`.

use std::rc::Rc;

use je_base::JsString;
use je_base::errors::{self, Error};
use je_base::network::schemas;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::uri::{URI, UriComponents, UriState};
use je_extensions::manifest::DebuggerContribution;
use je_platform::resources::host_platform;
use serde_json::{Map, Value};

use super::debug::IDebugSession;
use super::debug_protocol::ProtocolMessage;

/// `isUriString`: heuristics: a valid uri starts with a scheme and the scheme has at least 2 characters so that it
/// doesn't look like a drive letter (`/^[a-zA-Z][a-zA-Z0-9\+\-\.]+:/`). O `undefined` do original é a ausência do texto,
/// que o chamador trata antes (o texto vazio não casa).
pub fn is_uri_string(s: &str) -> bool {
    let mut chars = s.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    let mut scheme_rest = 0;
    for c in chars {
        if c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.') {
            scheme_rest += 1;
        } else {
            return c == ':' && scheme_rest >= 1;
        }
    }
    false
}

/// `isSessionAttach(session)`.
pub fn is_session_attach(session: &Rc<dyn IDebugSession>) -> bool {
    session.configuration().get("request").and_then(Value::as_str) == Some("attach")
        && get_extension_host_debug_session(session).is_none()
        && session.parent_session().is_none_or(|parent| is_session_attach(&parent))
}

/// `getExtensionHostDebugSession(session)`: returns the session or any parent which is an extension host debug session.
/// Returns `None` if there's none.
pub fn get_extension_host_debug_session(session: &Rc<dyn IDebugSession>) -> Option<Rc<dyn IDebugSession>> {
    let configuration = session.configuration();
    let mut r#type = configuration.get("type").and_then(Value::as_str).filter(|r#type| !r#type.is_empty())?.to_owned();

    if r#type == "vslsShare" {
        let proxied = configuration.get("adapterProxy").and_then(|proxy| proxy.get("configuration")).and_then(|configuration| configuration.get("type")).and_then(Value::as_str);
        if let Some(proxied) = proxied.filter(|proxied| !proxied.is_empty()) {
            r#type = proxied.to_owned();
        }
    }

    if equals_ignore_case(&r#type, "extensionhost") || equals_ignore_case(&r#type, "pwa-extensionhost") {
        return Some(session.clone());
    }

    session.parent_session().and_then(|parent| get_extension_host_debug_session(&parent))
}

/// `equalsIgnoreCase(a, b)` sobre texto.
fn equals_ignore_case(a: &str, b: &str) -> bool {
    je_base::strings::equals_ignore_case(&a.encode_utf16().collect::<Vec<u16>>(), &b.encode_utf16().collect::<Vec<u16>>())
}

/// `isDebuggerMainContribution`: only a debugger contributions with a label, program, or runtime attribute is considered
/// a "defining" or "main" debugger contribution.
pub fn is_debugger_main_contribution(dbg: &DebuggerContribution) -> bool {
    let non_empty = |value: &Option<String>| value.as_deref().is_some_and(|text| !text.is_empty());
    let has_program = matches!(dbg.extra.get("program"), Some(Value::String(program)) if !program.is_empty());
    non_empty(&dbg.r#type) && (non_empty(&dbg.label) || has_program || non_empty(&dbg.runtime))
}

// ---- conversão de caminhos

/// O `URI#toJSON` como objeto JSON (só os campos definidos, como o `JSON.stringify`).
fn uri_to_value(uri: &URI) -> Value {
    let state = uri.to_json();
    let components = &state.components;
    let mut object = Map::new();
    let mut put = |key: &str, value: Option<Value>| {
        if let Some(value) = value {
            object.insert(key.to_owned(), value);
        }
    };
    let text = |field: &Option<JsString>| field.as_ref().map(|text| Value::String(text.to_string_lossy()));
    put("$mid", state.mid.map(Value::from));
    put("fsPath", text(&state.fs_path));
    put("external", text(&state.external));
    put("path", text(&components.path));
    put("scheme", text(&components.scheme));
    put("authority", text(&components.authority));
    put("query", text(&components.query));
    put("fragment", text(&components.fragment));
    put("_sep", state.sep.map(Value::from));
    Value::Object(object)
}

/// `uri.revive(value)` de um objeto JSON no formato do `URI#toJSON`.
fn uri_from_value(object: &Map<String, Value>) -> URI {
    let text = |key: &str| object.get(key).and_then(Value::as_str).map(JsString::from);
    let state = UriState {
        mid: object.get("$mid").and_then(Value::as_i64).map(|mid| mid as i32),
        external: text("external"),
        fs_path: text("fsPath"),
        sep: object.get("_sep").and_then(Value::as_u64).map(|sep| sep as u8),
        components: UriComponents {
            scheme: text("scheme"),
            authority: text("authority"),
            path: text("path"),
            query: text("query"),
            fragment: text("fragment"),
        },
    };
    URI::revive(&state, host_platform())
}

/// `stringToUri(source)`: o novo `path` da fonte.
fn string_to_uri(source: &Value) -> Option<Value> {
    let path = source.get("path");
    if let Some(Value::String(text)) = path {
        let has_source_reference = source.get("sourceReference").and_then(Value::as_f64).is_some_and(|reference| reference > 0.0);
        if has_source_reference {
            // if there is a source reference, don't touch path
        } else if is_uri_string(text) {
            return Some(uri_to_value(&URI::parse(text.as_str(), false)));
        } else if PathModule::native(&ProcessEnvironment).is_absolute(text) {
            // assume path
            return Some(uri_to_value(&URI::file(text.as_str(), host_platform())));
        }
        // leave relative path as is
    }
    path.cloned()
}

/// `uriToString(source)`: o novo `path` da fonte.
fn uri_to_string(source: &Value) -> Option<Value> {
    let path = source.get("path");
    if let Some(Value::Object(object)) = path {
        let uri = uri_from_value(object);
        return Some(Value::String(if uri.scheme().to_string_lossy() == schemas::FILE {
            uri.fs_path(host_platform()).to_string_lossy()
        } else {
            uri.to_string(false).to_string_lossy()
        }));
    }
    path.cloned()
}

/// O `TypeError` do JavaScript ao ler uma propriedade de `undefined`.
fn read_of_undefined() -> ! {
    errors::throw(Error::type_error("Cannot read properties of undefined"))
}

/// `value.key`, com o `undefined` (e o `null`) como `None`.
fn child<'a>(value: &'a mut Value, key: &str) -> Option<&'a mut Value> {
    value.get_mut(key).filter(|child| !child.is_null())
}

/// O operando que o original lê sem checar: ausente lança `TypeError`.
fn required(value: Option<&mut Value>) -> &mut Value {
    value.unwrap_or_else(|| read_of_undefined())
}

/// `list.forEach(callback)` de uma lista que o original assume existir.
fn for_each(list: Option<&mut Value>, mut callback: impl FnMut(&mut Value)) {
    match required(list) {
        Value::Array(items) => items.iter_mut().for_each(&mut callback),
        _ => read_of_undefined(),
    }
}

/// `PathContainer` do protocolo: o `fixSourcePath(toDA, source)` só roda para uma fonte presente.
type FixSourcePath<'a> = &'a dyn Fn(bool, &mut Value);

fn fix_source(fix: FixSourcePath, to_da: bool, source: Option<&mut Value>) {
    if let Some(source) = source
        && source.is_object()
    {
        fix(to_da, source);
    }
}

/// `convertPaths(msg, fixSourcePath)`.
fn convert_paths(msg: &mut ProtocolMessage, fix: FixSourcePath) {
    match msg {
        ProtocolMessage::Event(event) => {
            let name = event.event.as_str();
            if matches!(name, "output" | "loadedSource" | "breakpoint") {
                let body = required(event.body.as_mut().filter(|body| !body.is_null()));
                match name {
                    "output" | "loadedSource" => fix_source(fix, false, child(body, "source")),
                    _ => {
                        let breakpoint = required(child(body, "breakpoint"));
                        fix_source(fix, false, child(breakpoint, "source"));
                    }
                }
            }
        }
        ProtocolMessage::Request(request) => {
            let command = request.command.as_str();
            match command {
                "setBreakpoints" | "breakpointLocations" | "source" | "gotoTargets" => {
                    let arguments = required(request.arguments.as_mut().filter(|arguments| !arguments.is_null()));
                    fix_source(fix, true, child(arguments, "source"));
                }
                "launchVSCode" => {
                    let arguments = required(request.arguments.as_mut().filter(|arguments| !arguments.is_null()));
                    for_each(child(arguments, "args"), |arg| fix_source(fix, false, Some(arg)));
                }
                _ => {}
            }
        }
        ProtocolMessage::Response(response) => {
            let truthy_body = response.body.as_mut().filter(|body| !body.is_null());
            if let Some(body) = truthy_body.filter(|_| response.success) {
                match response.command.as_str() {
                    "stackTrace" => for_each(child(body, "stackFrames"), |frame| fix_source(fix, false, child(frame, "source"))),
                    "loadedSources" => for_each(child(body, "sources"), |source| fix_source(fix, false, Some(source))),
                    "scopes" => for_each(child(body, "scopes"), |scope| fix_source(fix, false, child(scope, "source"))),
                    "setFunctionBreakpoints" | "setBreakpoints" => {
                        for_each(child(body, "breakpoints"), |breakpoint| fix_source(fix, false, child(breakpoint, "source")))
                    }
                    "disassemble" => for_each(child(body, "instructions"), |instruction| fix_source(fix, false, child(instruction, "location"))),
                    "locations" => fix_source(fix, false, child(body, "source")),
                    _ => {}
                }
            }
        }
        ProtocolMessage::Other(_) => {}
    }
}

/// O `fixPath` escolhido por `toUri`: `stringToUri` ou `uriToString`.
fn fix_path(to_uri: bool, source: &Value) -> Option<Value> {
    if to_uri { string_to_uri(source) } else { uri_to_string(source) }
}

/// `source.path = fixPath(source)`.
fn assign_path(source: &mut Value, path: Option<Value>) {
    if let Value::Object(object) = source {
        match path {
            Some(path) => {
                object.insert("path".to_owned(), path);
            }
            None => {
                object.remove("path");
            }
        }
    }
}

/// `convertToDAPaths(message, toUri)`: since we modify Source.paths in the message in place, we need to make a copy of it
/// (see #61129).
pub fn convert_to_da_paths(message: &ProtocolMessage, to_uri: bool) -> ProtocolMessage {
    let mut msg = message.clone();
    convert_paths(&mut msg, &|to_da, source| {
        if to_da {
            let path = fix_path(to_uri, source);
            assign_path(source, path);
        }
    });
    msg
}

/// `formatPII(value, excludePII, args)`: troca cada `{grupo}` (`/{([^}]+)}/g`) pelo argumento de mesmo nome; com
/// `exclude_pii`, os grupos que não começam por `_` ficam como estão.
pub fn format_pii(value: &str, exclude_pii: bool, args: Option<&std::collections::HashMap<String, String>>) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut result = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{'
            && let Some(close) = chars[i + 1..].iter().position(|c| *c == '}')
            && close > 0
        {
            let group: String = chars[i + 1..i + 1 + close].iter().collect();
            let matched: String = chars[i..i + close + 2].iter().collect();
            if exclude_pii && !group.is_empty() && !group.starts_with('_') {
                result.push_str(&matched);
            } else {
                match args.and_then(|args| args.get(&group)) {
                    Some(replacement) => result.push_str(replacement),
                    None => result.push_str(&matched),
                }
            }
            i += close + 2;
            continue;
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

/// `filterExceptionsFromTelemetry(data)`: tira as chaves marcadas com `!`.
pub fn filter_exceptions_from_telemetry(data: &Map<String, Value>) -> Map<String, Value> {
    data.iter().filter(|(key, _)| !key.starts_with('!')).map(|(key, value)| (key.clone(), value.clone())).collect()
}

/// `getExactExpressionStartAndEnd(lineContent, looseStart, looseEnd)`: note, uses 1-indexed numbers (em unidades UTF-16,
/// como o original). Devolve `(start, end)`.
pub fn get_exact_expression_start_and_end(line_content: &str, loose_start: i64, loose_end: i64) -> (i64, i64) {
    let units: Vec<u16> = line_content.encode_utf16().collect();
    let mut matching_expression: Option<Vec<u16>> = None;
    let mut start_offset: i64 = 0;

    // Some example supported expressions: myVar.prop, a.b.c.d, myVar?.prop, myVar->prop, MyClass::StaticProp, *myVar, ...foo
    // Match any character except a set of characters which often break interesting sub-expressions
    let expression = zyarr::RegExp::from_str(r"([^()\[\]{}<>\s+\-/%~#^;=|,`!]|\->)+", "g").expect("a expressão é válida");

    // First find the full expression under the cursor
    let mut last_index = 0;
    while let Some(found) = expression.exec(&units, last_index) {
        let start = found.start() as i64 + 1;
        let end = start + (found.end() - found.start()) as i64;

        if start <= loose_start && end >= loose_end {
            matching_expression = Some(units[found.start()..found.end()].to_vec());
            start_offset = start;
            break;
        }
        last_index = if found.end() == found.start() { found.end() + 1 } else { found.end() };
    }

    // Handle spread syntax: if the expression starts with '...', extract just the identifier
    if let Some(expression) = &matching_expression
        && expression.len() > 3
        && expression[..3].iter().all(|unit| *unit == u16::from(b'.'))
    {
        matching_expression = Some(expression[3..].to_vec());
        start_offset += 3; // Skip the '...' prefix
    }

    // If there are non-word characters after the cursor, we want to truncate the expression then.
    // For example in expression 'a.b.c.d', if the focus was under 'b', 'a.b' would be evaluated.
    if let Some(expression) = &matching_expression {
        let sub_expression = zyarr::RegExp::from_str(r"(\w|\p{L})+", "gu").expect("a expressão é válida");
        let mut last_index = 0;
        let mut truncate_at: Option<usize> = None;
        while let Some(found) = sub_expression.exec(expression, last_index) {
            let sub_end = found.start() as i64 + 1 + start_offset + (found.end() - found.start()) as i64;
            last_index = found.end();
            if sub_end >= loose_end {
                truncate_at = Some(found.end());
                break;
            }
        }

        if let Some(truncate_at) = truncate_at {
            matching_expression = Some(expression[..truncate_at].to_vec());
        }
    }

    match matching_expression {
        Some(expression) => (start_offset, start_offset + expression.len() as i64 - 1),
        None => (0, 0),
    }
}

/// `getPlatformSpecificConfig(config, os)`: a seção `windows`, `osx` ou `linux` da configuração.
fn get_platform_specific_config(config: &Value, os: je_base::platform::OperatingSystem) -> Option<&Value> {
    let key = match os {
        je_base::platform::OperatingSystem::Windows => "windows",
        je_base::platform::OperatingSystem::Macintosh => "osx",
        je_base::platform::OperatingSystem::Linux => "linux",
    };
    config.get(key).filter(|section| !matches!(section, Value::Null | Value::Bool(false)))
}

/// `getEffectiveConfigForPlatform(config, os = OS)`: a configuração com a seção da plataforma por cima; a `presentation` é a
/// mescla das duas.
pub fn get_effective_config_for_platform(config: &Value, os: Option<je_base::platform::OperatingSystem>) -> Value {
    let os = os.unwrap_or_else(|| je_base::platform::Platform::detect().os);
    let Some(platform_config) = get_platform_specific_config(config, os) else {
        return config.clone();
    };

    let mut result = config.as_object().cloned().unwrap_or_default();
    if let Some(platform_config) = platform_config.as_object() {
        for (key, value) in platform_config {
            result.insert(key.clone(), value.clone());
        }
    }
    let platform_presentation = platform_config.get("presentation").filter(|p| !matches!(p, Value::Null | Value::Bool(false)));
    match platform_presentation {
        Some(platform_presentation) => {
            let mut merged = config.get("presentation").and_then(Value::as_object).cloned().unwrap_or_default();
            if let Some(platform_presentation) = platform_presentation.as_object() {
                for (key, value) in platform_presentation {
                    merged.insert(key.clone(), value.clone());
                }
            }
            result.insert("presentation".to_owned(), Value::Object(merged));
        }
        None => match config.get("presentation") {
            Some(presentation) => {
                result.insert("presentation".to_owned(), presentation.clone());
            }
            None => {
                result.remove("presentation");
            }
        },
    }
    Value::Object(result)
}

/// `getEffectivePresentationForConfig(config, os = OS)`.
pub fn get_effective_presentation_for_config(config: &Value, os: Option<je_base::platform::OperatingSystem>) -> Option<Value> {
    get_effective_config_for_platform(config, os).get("presentation").cloned()
}

/// `compareOrders(first, second)`.
fn compare_orders(first: Option<f64>, second: Option<f64>) -> f64 {
    match (first, second) {
        (None, None) => 0.0,
        (None, Some(_)) => 1.0,
        (Some(_), None) => -1.0,
        (Some(first), Some(second)) => first - second,
    }
}

/// `getVisibleAndSorted(array)`: as configurações com `presentation` (`hidden`, `group`, `order`), sem as escondidas, na
/// ordem do grupo e da ordem (as sem `presentation` por último), com a ordenação estável do V8.
pub fn get_visible_and_sorted(array: &[Value]) -> Vec<Value> {
    let mut visible: Vec<Value> = array
        .iter()
        .filter(|config| !config.get("presentation").and_then(|p| p.get("hidden")).is_some_and(|hidden| !matches!(hidden, Value::Null | Value::Bool(false))))
        .cloned()
        .collect();
    let presentation = |config: &Value| config.get("presentation").filter(|p| !matches!(p, Value::Null | Value::Bool(false))).cloned();
    let group = |presentation: &Value| presentation.get("group").and_then(Value::as_str).filter(|g| !g.is_empty()).map(str::to_owned);
    let order = |presentation: &Value| presentation.get("order").and_then(Value::as_f64);
    je_base::arrays::sort_by_compare(&mut visible, |first, second| {
        let Some(first) = presentation(first) else {
            return if presentation(second).is_none() { 0.0 } else { 1.0 };
        };
        let Some(second) = presentation(second) else {
            return -1.0;
        };
        let Some(first_group) = group(&first) else {
            return if group(&second).is_none() { compare_orders(order(&first), order(&second)) } else { 1.0 };
        };
        let Some(second_group) = group(&second) else {
            return -1.0;
        };
        if first_group != second_group {
            return match je_base::collator::locale_compare(&first_group, &second_group) {
                std::cmp::Ordering::Less => -1.0,
                std::cmp::Ordering::Equal => 0.0,
                std::cmp::Ordering::Greater => 1.0,
            };
        }

        compare_orders(order(&first), order(&second))
    });
    visible
}

/// `convertToVSCPaths(message, toUri)`: since we modify Source.paths in the message in place, we need to make a copy of it
/// (see #61129).
pub fn convert_to_vsc_paths(message: &ProtocolMessage, to_uri: bool) -> ProtocolMessage {
    let mut msg = message.clone();
    convert_paths(&mut msg, &|to_da, source| {
        if !to_da {
            let path = fix_path(to_uri, source);
            assign_path(source, path);
        }
    });
    msg
}
