//! Porte de `src/vs/workbench/contrib/terminal/browser/terminalUri.ts`: as URIs `vscode-terminal:` dos
//! editores de terminal (`/<workspaceId>/<instanceId>`), a leitura delas, e os helpers de arrastar e soltar.
//!
//! Tipos de `terminal/browser/terminal.ts` que o arquivo usa, portados só no que ele lê:
//! - `ITerminalInstance`: só o campo `resource` (o original usa `Pick<ITerminalInstance, 'resource'>`),
//!   que vira o trait [`ITerminalInstanceResource`].
//! - `TerminalDataTransfers` (`const enum` com o único membro `Terminals = 'Terminals'`): a constante
//!   [`TERMINAL_DATA_TRANSFERS_TERMINALS`].
//!
//! Notas de tradução:
//! - Exceção vira [`errors::throw`] (pânico carregando o [`Error`]), como no resto do porte.
//! - `Number.parseInt(s)` (radix ausente: 10, ou 16 com prefixo `0x`) é reproduzido por [`js_parse_int`],
//!   que devolve `NaN` quando não há dígito. `instanceId` é `number` no original, aqui `f64`.
//! - `new URLSearchParams().set('command', id).toString()` é a serialização `application/x-www-form-urlencoded`,
//!   reproduzida por [`url_search_param`].
//! - `JSON.parse` usa o parser de [`je_base::json`] com comentários e vírgula final recusados; qualquer
//!   erro de sintaxe vira `SyntaxError`. Um JSON que não é lista nem texto não é iterável (`TypeError`).
//!   Cada entrada passa por `URI.parse`, que no original converte o argumento com `ToString`
//!   ([`js_to_string`]).
//! - `IPartialDragEvent.dataTransfer: Pick<DataTransfer, 'getData'> | null` vira o trait
//!   [`DataTransferGetData`] e um `Option<&dyn ...>`.

use je_base::dynamic::number_to_js_string;
use je_base::JsString;
use je_base::errors::{self, Error};
use je_base::json::{self, ParseOptions, Value};
use je_base::network::schemas;
use je_base::uri::{URI, UriComponents};

/// `TerminalDataTransfers.Terminals` (`terminal/browser/terminal.ts`).
pub const TERMINAL_DATA_TRANSFERS_TERMINALS: &str = "Terminals";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ITerminalUriMetadata {
    pub title: Option<JsString>,
    pub command_id: Option<JsString>,
    pub command_line: Option<JsString>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ITerminalIdentifier {
    pub workspace_id: JsString,
    pub instance_id: Option<f64>,
}

/// `Pick<DataTransfer, 'getData'>`.
pub trait DataTransferGetData {
    fn get_data(&self, format: &str) -> JsString;
}

/// `IPartialDragEvent`.
pub struct IPartialDragEvent<'a> {
    pub data_transfer: Option<&'a dyn DataTransferGetData>,
}

/// `Pick<ITerminalInstance, 'resource'>`.
pub trait ITerminalInstanceResource {
    fn resource(&self) -> &URI;
}

/// `Number.parseInt(text)`: `NaN` quando a leitura não encontra nenhum dígito.
fn js_parse_int(text: &JsString) -> f64 {
    let lossy = text.to_string_lossy();
    // `StrWhiteSpaceChar`: espaços do Unicode (categoria Zs), controles de espaço e terminadores de linha,
    // mais o BOM. O `char::is_whitespace` do Rust inclui U+0085, que o JavaScript não trata como espaço.
    let trimmed = lossy.trim_start_matches(|c: char| (c.is_whitespace() && c != '\u{85}') || c == '\u{FEFF}');
    let (negative, unsigned) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let (radix, digits) = match unsigned.strip_prefix("0x").or_else(|| unsigned.strip_prefix("0X")) {
        Some(rest) => (16u32, rest),
        None => (10u32, unsigned),
    };
    let mut value = 0f64;
    let mut any = false;
    for digit in digits.chars().map_while(|c| c.to_digit(radix)) {
        value = value * f64::from(radix) + f64::from(digit);
        any = true;
    }
    if !any {
        return f64::NAN;
    }
    if negative { -value } else { value }
}

/// `ToString` de um valor vindo do `JSON.parse`, o que `URI.parse` aplica ao argumento que não é texto.
fn js_to_string(value: &Value) -> JsString {
    match value {
        Value::Null => JsString::from("null"),
        Value::Bool(b) => JsString::from(if *b { "true" } else { "false" }),
        Value::Number(n) => JsString::from(number_to_js_string(*n)),
        Value::String(s) => s.clone(),
        Value::Array(items) => {
            let mut out = JsString::new();
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_unit(u16::from(b','));
                }
                if !matches!(item, Value::Null) {
                    out.push_units(js_to_string(item).units());
                }
            }
            out
        }
        Value::Object(_) => JsString::from("[object Object]"),
    }
}

/// A serialização de `URLSearchParams` com uma única entrada `name=value`.
fn url_search_param(name: &str, value: &JsString) -> String {
    fn encode(text: &str, out: &mut String) {
        for byte in text.bytes() {
            match byte {
                b'*' | b'-' | b'.' | b'_' => out.push(char::from(byte)),
                b' ' => out.push('+'),
                b if b.is_ascii_alphanumeric() => out.push(char::from(b)),
                b => out.push_str(&format!("%{b:02X}")),
            }
        }
    }
    let mut out = String::new();
    encode(name, &mut out);
    out.push('=');
    encode(&value.to_string_lossy(), &mut out);
    out
}

pub fn parse_terminal_uri(resource: &URI) -> ITerminalIdentifier {
    let mut parts = resource.path().units().split(|&u| u == u16::from(b'/'));
    // `const [, workspaceId, instanceId] = resource.path.split('/')`.
    let workspace_id = parts.nth(1).map(JsString::from);
    let instance_id = parts.next().map(JsString::from);
    // `Number.parseInt(undefined)` é `NaN`.
    let parse = |id: &Option<JsString>| id.as_ref().map_or(f64::NAN, js_parse_int);
    let workspace_ok = workspace_id.as_ref().is_some_and(|id| !id.is_empty());
    let parsed = parse(&instance_id);
    if !workspace_ok || parsed.is_nan() || parsed == 0.0 {
        errors::throw(Error::new(format!("Could not parse terminal uri for resource {}", resource.to_string(false))));
    }
    ITerminalIdentifier { workspace_id: workspace_id.unwrap_or_default(), instance_id: Some(parsed) }
}

pub fn get_terminal_uri(
    workspace_id: impl Into<JsString>,
    instance_id: f64,
    title: Option<&str>,
    command_id: Option<&str>,
) -> URI {
    // `commandId` vazio é falso: não entra em `params` nem em `query`.
    let command_id = command_id.filter(|id| !id.is_empty());
    let query = command_id.map(|id| JsString::from(url_search_param("command", &JsString::from(id))));
    let mut path = JsString::from("/");
    path.push_units(workspace_id.into().units());
    path.push_unit(u16::from(b'/'));
    path.push_units(JsString::from(number_to_js_string(instance_id)).units());
    // `title || undefined`.
    let fragment = title.filter(|t| !t.is_empty()).map(JsString::from);
    URI::from(
        &UriComponents {
            scheme: Some(JsString::from(schemas::VSCODE_TERMINAL)),
            authority: None,
            path: Some(path),
            query,
            fragment,
        },
        false,
    )
}

/// `for (const entry of json)`: a lista, ou o texto por pontos de código.
fn iterate_json(json: Value) -> Vec<JsString> {
    match json {
        Value::Array(items) => items.iter().map(js_to_string).collect(),
        Value::String(text) => char::decode_utf16(text.units().iter().copied())
            .map(|decoded| match decoded {
                Ok(c) => JsString::from(c.to_string()),
                Err(unpaired) => JsString::from_units(vec![unpaired.unpaired_surrogate()]),
            })
            .collect(),
        _ => errors::throw(Error::named("TypeError", "json is not iterable")),
    }
}

pub fn get_terminal_resources_from_drag_event(event: &IPartialDragEvent<'_>) -> Option<Vec<URI>> {
    let resources = event.data_transfer?.get_data(TERMINAL_DATA_TRANSFERS_TERMINALS);
    if resources.is_empty() {
        return None;
    }
    let mut parse_errors = Vec::new();
    let options = ParseOptions { disallow_comments: true, allow_trailing_comma: false, allow_empty_content: false };
    let json = match json::parse(resources.units(), &mut parse_errors, options) {
        Some(json) if parse_errors.is_empty() => json,
        _ => errors::throw(Error::named("SyntaxError", format!("Unexpected token in JSON: {resources}"))),
    };
    let result: Vec<URI> = iterate_json(json).into_iter().map(|entry| URI::parse(entry, false)).collect();
    if result.is_empty() { None } else { Some(result) }
}

pub fn get_instance_from_resource<'a, T: ITerminalInstanceResource>(
    instances: &'a [T],
    resource: Option<&URI>,
) -> Option<&'a T> {
    let resource = resource?;
    // Note that the URI's workspace and instance id might not originally be from this window
    // Don't bother checking the scheme and assume instances only contains terminals
    instances.iter().find(|instance| instance.resource().path() == resource.path())
}
