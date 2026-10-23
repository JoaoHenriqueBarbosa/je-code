//! Porte de `src/vs/base/common/uriTemplate.ts`: URI Template da RFC 6570 (níveis 1 a 4).
//!
//! As variáveis de [`UriTemplate::resolve`] são um `Record<string, unknown>` do original, aqui um
//! [`Dynamic`] de objeto. Diferenças de forma, sem mudança de resultado para texto bem formado:
//!
//! - o resultado é `String`, e um valor com surrogate isolado vira U+FFFD (o original o deixaria no
//!   texto, ou lançaria `URIError` no `encodeURI`);
//! - `pctEncode` do original codifica cada unidade UTF-16 como `%XX` com o valor da unidade, não em
//!   UTF-8 (um `é` vira `%E9`, um `€` vira `%20AC`); isso é mantido igual;
//! - um elemento de lista que não é texto sai vazio na expansão sem `+`/`#` (o `pctEncode` do original
//!   lê `.length` de um número e não percorre nada), e é convertido por `String(x)` na expansão com `+`/`#`.

use crate::dynamic::Dynamic;
use crate::js_string::{JsString, is_js_whitespace};

/// `IUriTemplateVariable`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UriTemplateVariable {
    pub explodable: bool,
    pub name: String,
    pub optional: bool,
    pub prefix_length: Option<usize>,
    pub repeatable: bool,
}

/// `IUriTemplateComponent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UriTemplateExpression {
    pub expression: String,
    pub operator: String,
    pub variables: Vec<UriTemplateVariable>,
}

/// Um item de `UriTemplate.components`: texto literal ou expressão.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UriTemplateComponent {
    Literal(String),
    Expression(UriTemplateExpression),
}

/// Represents an RFC 6570 URI Template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UriTemplate {
    pub template: String,
    /// The parsed template components (expressions).
    pub components: Vec<UriTemplateComponent>,
}

const OPERATORS: [char; 7] = ['+', '#', '.', '/', ';', '?', '&'];

impl UriTemplate {
    /// Parses a URI template string into a UriTemplate instance.
    pub fn parse(template: &str) -> UriTemplate {
        let chars: Vec<char> = template.chars().collect();
        let slice = |from: usize, to: usize| chars[from..to].iter().collect::<String>();
        let mut components = Vec::new();
        let mut last_pos = 0;
        let mut i = 0;
        // `/\{([^{}]+)\}/g`
        while i < chars.len() {
            let Some(end) = match_expression(&chars, i) else {
                i += 1;
                continue;
            };
            let expression = slice(i, end + 1);
            let inner = slice(i + 1, end);
            components.push(UriTemplateComponent::Literal(slice(last_pos, i)));
            last_pos = end + 1;

            // Handle escaped braces: treat '{{' and '}}' as literals, not expressions
            if (i > 0 && chars[i - 1] == '{') || chars.get(last_pos) == Some(&'}') {
                components.push(UriTemplateComponent::Literal(inner));
                i = last_pos;
                continue;
            }

            let mut operator = String::new();
            let mut rest = inner.as_str();
            if let Some(first) = rest.chars().next()
                && OPERATORS.contains(&first)
            {
                operator = first.to_string();
                rest = &rest[first.len_utf8()..];
            }
            let variables = rest.split(',').map(parse_variable).collect();
            components.push(UriTemplateComponent::Expression(UriTemplateExpression { expression, operator, variables }));
            i = last_pos;
        }
        components.push(UriTemplateComponent::Literal(slice(last_pos, chars.len())));

        UriTemplate { template: template.to_string(), components }
    }

    /// Resolves the template with the given variables.
    pub fn resolve(&self, variables: &Dynamic) -> String {
        let mut result = String::new();
        for comp in &self.components {
            match comp {
                UriTemplateComponent::Literal(text) => result.push_str(text),
                UriTemplateComponent::Expression(expression) => result.push_str(&expand(expression, variables)),
            }
        }
        result
    }
}

/// O fim (índice do `}`) de uma expressão `{[^{}]+}` que começa em `start`.
fn match_expression(chars: &[char], start: usize) -> Option<usize> {
    if chars[start] != '{' {
        return None;
    }
    let mut j = start + 1;
    while j < chars.len() && chars[j] != '{' && chars[j] != '}' {
        j += 1;
    }
    (j > start + 1 && j < chars.len() && chars[j] == '}').then_some(j)
}

fn parse_variable(spec: &str) -> UriTemplateVariable {
    let mut name = spec;
    let mut explodable = false;
    let mut repeatable = false;
    let mut prefix_length = None;
    let mut optional = false;
    if let Some(stripped) = name.strip_suffix('*') {
        explodable = true;
        repeatable = true;
        name = stripped;
    }
    // `/^(.*?):(\d+)$/`: o `.` não casa terminador de linha.
    if let Some((head, digits)) = name.rsplit_once(':')
        && !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && !head.contains(['\n', '\r', '\u{2028}', '\u{2029}'])
    {
        name = head;
        prefix_length = Some(digits.parse::<usize>().unwrap_or(usize::MAX));
    }
    if let Some(stripped) = name.strip_suffix('?') {
        optional = true;
        name = stripped;
    }
    UriTemplateVariable { explodable, name: name.to_string(), optional, prefix_length, repeatable }
}

/// `String(value)` de um valor que não é objeto simples.
fn js_to_string(value: &Dynamic) -> String {
    match value {
        Dynamic::Undefined => "undefined".into(),
        Dynamic::Null => "null".into(),
        Dynamic::Bool(b) => b.to_string(),
        Dynamic::Number(n) => crate::dynamic::number_to_js_string(*n),
        Dynamic::BigInt(n) => n.to_string(),
        Dynamic::String(s) => s.to_string_lossy(),
        Dynamic::Array(a) => a
            .to_vec()
            .iter()
            .map(|x| if x.is_nullish() { String::new() } else { js_to_string(x) })
            .collect::<Vec<_>>()
            .join(","),
        _ => "[object Object]".into(),
    }
}

fn expand(comp: &UriTemplateExpression, variables: &Dynamic) -> String {
    let op = comp.operator.as_str();
    if comp.variables.is_empty() {
        return comp.expression.clone();
    }
    let mut vals: Vec<String> = Vec::new();
    let is_named = matches!(op, ";" | "?" | "&");
    let is_reserved = matches!(op, "+" | "#");
    let is_fragment = op == "#";
    let is_label = op == ".";
    let is_path = op == "/";
    let is_form = op == "?";
    let is_form_cont = op == "&";
    let is_param = op == ";";
    let prefix = match op {
        "#" | "." | ";" | "?" | "&" => op,
        _ => "",
    };

    for v in &comp.variables {
        let key = JsString::from(v.name.as_str());
        let value = variables.get(&key);
        let defined = variables.has_property(&key);
        let empty_list = matches!(&value, Dynamic::Array(a) if a.is_empty());
        if value.is_nullish() || empty_list {
            if is_param {
                if defined && value.is_nullish() {
                    vals.push(v.name.clone());
                }
                continue;
            }
            if (is_form || is_form_cont) && defined {
                vals.push(form_pair(&v.name, "", is_named));
            }
            continue;
        }
        if value.type_of() == "object" && !value.is_array() {
            let entries = value.own_entries();
            if v.explodable {
                let pairs: Vec<String> = entries
                    .iter()
                    .map(|(k, x)| {
                        let k = k.to_string_lossy();
                        let this_val = js_to_string(x);
                        if is_param || is_form || is_form_cont || is_label {
                            format!("{k}={this_val}")
                        } else if is_path {
                            format!("/{k}={}", encode(&this_val, is_reserved))
                        } else {
                            format!("{k}={}", encode(&this_val, is_reserved))
                        }
                    })
                    .collect();
                vals.push(pairs.join(if is_label {
                    "."
                } else if is_path {
                    ""
                } else if is_param {
                    ";"
                } else if is_form || is_form_cont {
                    "&"
                } else {
                    ","
                }));
            } else {
                // Not explodable: join as k1,v1,k2,v2,... and assign to variable name
                let pairs: Vec<String> =
                    entries.iter().flat_map(|(k, x)| [k.to_string_lossy(), js_to_string(x)]).collect();
                // For label, param, form, join as keys=semi,;,dot,.,comma,, (no encoding of , or ;)
                let joined = pairs.join(",");
                if is_label {
                    vals.push(joined);
                } else if is_param || is_form || is_form_cont {
                    vals.push(format!("{}={joined}", v.name));
                } else {
                    vals.push(joined);
                }
            }
            continue;
        }
        if let Dynamic::Array(list) = &value {
            let items = list.to_vec();
            // `Array.prototype.join`: nulo e `undefined` viram texto vazio.
            let plain = |sep: &str| {
                items
                    .iter()
                    .map(|x| if x.is_nullish() { String::new() } else { js_to_string(x) })
                    .collect::<Vec<_>>()
                    .join(sep)
            };
            let named = |sep: &str| {
                items.iter().map(|x| format!("{}={}", v.name, js_to_string(x))).collect::<Vec<_>>().join(sep)
            };
            let encoded = |sep: &str, lead: &str| {
                items.iter().map(|x| format!("{lead}{}", encode_item(x, is_reserved))).collect::<Vec<_>>().join(sep)
            };
            if v.explodable {
                vals.push(if is_label {
                    plain(".")
                } else if is_path {
                    encoded("", "/")
                } else if is_param {
                    named(";")
                } else if is_form || is_form_cont {
                    named("&")
                } else {
                    encoded(",", "")
                });
            } else if is_label {
                vals.push(plain(","));
            } else if is_param || is_form || is_form_cont {
                vals.push(format!("{}={}", v.name, plain(",")));
            } else {
                vals.push(encoded(",", ""));
            }
            continue;
        }
        let mut text = JsString::from(js_to_string(&value));
        if let Some(length) = v.prefix_length {
            text = text.slice(0, length.min(text.len()));
        }
        // For simple expansion, encode ! as well (not reserved)
        // Only + and # are reserved
        let enc = encode(&text.to_string_lossy(), op == "+" || op == "#");
        if is_param || is_form || is_form_cont {
            vals.push(format!("{}={enc}", v.name));
        } else if is_path {
            vals.push(format!("/{enc}"));
        } else {
            vals.push(enc);
        }
    }

    if is_label {
        // Remove trailing dot for missing values
        let filtered: Vec<&str> = vals.iter().map(String::as_str).filter(|v| !v.is_empty()).collect();
        if filtered.is_empty() { String::new() } else { format!("{prefix}{}", filtered.join(".")) }
    } else if is_path {
        // Remove empty segments for undefined/null
        let joined: String = vals.iter().filter(|v| !v.is_empty()).map(String::as_str).collect();
        if !joined.is_empty() && !joined.starts_with('/') { format!("/{joined}") } else { joined }
    } else if is_param {
        // For param, if value is empty string, just append ;name
        if vals.is_empty() {
            String::new()
        } else {
            format!("{prefix}{}", vals.iter().map(|v| strip_trailing_equals(v)).collect::<Vec<_>>().join(";"))
        }
    } else if is_form || is_form_cont {
        if vals.is_empty() { String::new() } else { format!("{prefix}{}", vals.join("&")) }
    } else if is_fragment {
        format!("{prefix}{}", vals.join(","))
    } else {
        vals.join(",")
    }
}

/// `v.replace(/=\s*$/, '')`.
fn strip_trailing_equals(text: &str) -> &str {
    let trimmed = text.trim_end_matches(|c: char| c.len_utf16() == 1 && is_js_whitespace(c as u16));
    trimmed.strip_suffix('=').unwrap_or(text)
}

fn encode(text: &str, reserved: bool) -> String {
    if reserved { encode_uri(text) } else { pct_encode(text) }
}

fn encode_item(item: &Dynamic, reserved: bool) -> String {
    match item {
        Dynamic::String(s) => encode(&s.to_string_lossy(), reserved),
        _ if reserved => encode_uri(&js_to_string(item)),
        _ => String::new(),
    }
}

fn form_pair(key: &str, value: &str, named: bool) -> String {
    let encoded = encode_uri_component(value);
    if named { format!("{key}={encoded}") } else { encoded }
}

fn percent_encode_bytes(text: &str, keep: impl Fn(u8) -> bool) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if keep(byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

const URI_MARKS: &[u8] = b"-_.!~*'()";

/// `encodeURIComponent`.
fn encode_uri_component(text: &str) -> String {
    percent_encode_bytes(text, |b| b.is_ascii_alphanumeric() || URI_MARKS.contains(&b))
}

/// `encodeURI`.
fn encode_uri(text: &str) -> String {
    percent_encode_bytes(text, |b| b.is_ascii_alphanumeric() || URI_MARKS.contains(&b) || b";,/?:@&=+$#".contains(&b))
}

fn pct_encode(text: &str) -> String {
    let mut out = String::new();
    for unit in text.encode_utf16() {
        let unreserved = matches!(unit, 0x30..=0x39 | 0x41..=0x5a | 0x61..=0x7a | 0x2d | 0x2e | 0x5f | 0x7e);
        if unreserved {
            out.push(char::from(unit as u8));
        } else {
            out.push_str(&format!("%{unit:02X}"));
        }
    }
    out
}
