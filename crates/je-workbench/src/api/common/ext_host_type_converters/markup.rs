//! `extHostTypeConverters.ts`: `MarkdownString`, a conversão entre o `vscode.MarkdownString` (e o
//! `vscode.MarkedString`) e o `IMarkdownString` do protocolo.
//!
//! Divergências:
//! - A extração de `uris` (`res.uris`) percorre os tokens `link` e `image` com o léxico de [`markdown_links`](super::markdown_links),
//!   um subconjunto do lexer do `marked` (que ainda não existe no porte; ver o módulo para o que ele cobre e o que não). O
//!   `collectUri` e o `_uriMassage` são portados: o `uris` sempre existe (vazio quando não há link), como o `res.uris =
//!   Object.create(null)` do original. A chave aleatória `__uri_xxxxxx` de um URI embutido na query usa seis dígitos
//!   hexadecimais de um UUID.
//! - `isCodeblock`/`isMarkdownString`/`typeof markup === 'string'` (duck typing) são as variantes de
//!   [`types::MarkedString`]; o ramo final `{ value: '' }` (valor que não é nenhum dos três) não existe.

use je_base::dynamic::Dynamic;
use je_base::errors;
use je_base::html_content::{IMarkdownString, IsTrusted, MarkdownStringTrustedOptions, parse_href_and_dimensions};
use je_base::map::OrderedMap;
use je_base::marshalling::{Marshalled, json_stringify, parse};
use je_base::uri::{Field, URI, UriChange};
use je_base::uuid::generate_uuid;
use je_platform::resources::host_platform;

use super::markdown_links::collect_links;
use je_base::JsString;
use je_editor::languages::StringOrMarkdown;

use crate::api::common::ext_host_types as types;
use crate::api::common::ext_host_values::{ApiValue, is_instance_of};

/// `typeConverters.MarkdownString`.
pub struct MarkdownString;

/// `true` quando o valor é falso em JavaScript (`!data`).
fn is_falsy(data: &Marshalled) -> bool {
    match data {
        Marshalled::Undefined | Marshalled::Null => true,
        Marshalled::Bool(value) => !*value,
        Marshalled::Number(value) => *value == 0.0 || value.is_nan(),
        Marshalled::String(value) => value.units().is_empty(),
        _ => false,
    }
}

/// O `cloneAndChange` de `_uriMassage`: cada `URI` vira uma chave `__uri_xxxxxx` guardada em `bucket`.
fn replace_uris(value: Marshalled, bucket: &mut OrderedMap<JsString, URI>, changed: &mut bool) -> Marshalled {
    match value {
        Marshalled::Uri(uri) => {
            let key = JsString::from(format!("__uri_{}", &generate_uuid().replace('-', "")[..6]));
            bucket.insert(key.clone(), uri);
            *changed = true;
            Marshalled::String(key)
        }
        Marshalled::Array(items) => Marshalled::Array(items.into_iter().map(|item| replace_uris(item, bucket, changed)).collect()),
        Marshalled::Object(entries) => Marshalled::Object(entries.into_iter().map(|(key, item)| (key, replace_uris(item, bucket, changed))).collect()),
        other => other,
    }
}

/// `_uriMassage(part, bucket)`: tira de dentro da query (um JSON) os URIs, trocando-os por chaves.
fn uri_massage(part: &JsString, bucket: &mut OrderedMap<JsString, URI>) -> JsString {
    if part.units().is_empty() {
        return part.clone();
    }
    let Ok(data) = errors::catch(|| parse(part.units(), host_platform())) else {
        return part.clone(); // ignore
    };
    if is_falsy(&data) {
        return part.clone();
    }

    let mut changed = false;
    let data = replace_uris(data, bucket, &mut changed);
    if !changed {
        return part.clone();
    }

    json_stringify(&data).unwrap_or_else(|| part.clone())
}

/// `collectUri({ href })`.
fn collect_uri(href: &JsString, uris: &mut OrderedMap<JsString, URI>) {
    let Ok(uri) = errors::catch(|| URI::parse(href.clone(), true)) else {
        return; // ignore
    };
    let query = uri_massage(uri.query(), uris);
    let uri = uri.with(&UriChange { query: Field::Set(query), ..UriChange::default() });
    uris.insert(href.clone(), uri);
}

/// O `res.uris` de `MarkdownString.from`: os `link` e `image` do valor, cada um por `href`.
fn with_uris(mut res: IMarkdownString) -> IMarkdownString {
    let mut uris: OrderedMap<JsString, URI> = OrderedMap::new();
    for link in collect_links(&res.value.to_string_lossy()) {
        let href = if link.is_image { parse_href_and_dimensions(JsString::from(link.href.as_str()).units()).href } else { JsString::from(link.href.as_str()) };
        collect_uri(&href, &mut uris);
    }
    res.uris = Some(uris);
    res
}

impl MarkdownString {
    pub fn from_many(markup: &[types::MarkedString]) -> Vec<IMarkdownString> {
        markup.iter().map(MarkdownString::from).collect()
    }

    /// O ramo de `vscode.MarkdownString` de `from`.
    pub fn from_markdown(markup: &types::MarkdownString) -> IMarkdownString {
        with_uris(IMarkdownString {
            value: markup.value.clone(),
            is_trusted: markup.is_trusted.clone(),
            support_theme_icons: markup.support_theme_icons,
            support_html: markup.support_html,
            support_alert_syntax: markup.support_alert_syntax,
            base_uri: markup.base_uri.clone(),
            uris: None,
        })
    }

    pub fn from(markup: &types::MarkedString) -> IMarkdownString {
        match markup {
            types::MarkedString::Code { language, value } => {
                with_uris(IMarkdownString { value: JsString::from(format!("```{language}\n{value}\n```\n")), ..IMarkdownString::default() })
            }
            types::MarkedString::Markdown(markdown) => MarkdownString::from_markdown(markdown),
            types::MarkedString::Text(text) => with_uris(IMarkdownString { value: JsString::from(text.as_str()), ..IMarkdownString::default() }),
        }
    }

    pub fn to(value: &IMarkdownString) -> types::MarkdownString {
        let mut result = types::MarkdownString::new(None, value.support_theme_icons.unwrap_or(false));
        result.value = value.value.clone();
        result.is_trusted = value.is_trusted.clone();
        result.support_html = value.support_html;
        result.support_alert_syntax = value.support_alert_syntax;
        result.base_uri = value.base_uri.clone();
        result
    }

    /// `fromStrict`: um valor falso (ausente ou texto vazio) lê como ausente.
    pub fn from_strict(value: Option<&types::Documentation>) -> Option<StringOrMarkdown> {
        match value? {
            types::Documentation::Text(text) if text.is_empty() => None,
            types::Documentation::Text(text) => Some(StringOrMarkdown::String(text.clone())),
            types::Documentation::Markdown(markdown) => Some(StringOrMarkdown::Markdown(MarkdownString::from_markdown(markdown))),
        }
    }
}

impl MarkdownString {
    /// O `IMarkdownString` como o DTO do RPC. O `uris` não cruza: `MarkdownString.to` não o lê.
    pub fn to_dto(markdown: &IMarkdownString) -> Dynamic {
        let is_trusted = markdown.is_trusted.as_ref().map(|trusted| match trusted {
            IsTrusted::Bool(flag) => Dynamic::Bool(*flag),
            IsTrusted::Options(options) => {
                Dynamic::object([("enabledCommands", Dynamic::array(options.enabled_commands.iter().map(|command| Dynamic::string(command))))])
            }
        });
        let entries = [
            ("value", Some(Dynamic::String(markdown.value.clone()))),
            ("isTrusted", is_trusted),
            ("supportThemeIcons", markdown.support_theme_icons.map(Dynamic::Bool)),
            ("supportHtml", markdown.support_html.map(Dynamic::Bool)),
            ("supportAlertSyntax", markdown.support_alert_syntax.map(Dynamic::Bool)),
            ("baseUri", markdown.base_uri.as_ref().map(ApiValue::to_dynamic)),
        ];
        Dynamic::object(entries.into_iter().filter_map(|(key, item)| item.map(|item| (key, item))))
    }

    /// A leitura do DTO de [`MarkdownString::to_dto`]; `None` se o valor não é um objeto com o `value` em texto.
    pub fn from_dto(value: &Dynamic) -> Option<IMarkdownString> {
        let Dynamic::String(text) = value.get_str("value") else {
            return None;
        };
        let flag = |key: &str| match value.get_str(key) {
            Dynamic::Bool(flag) => Some(flag),
            _ => None,
        };
        let is_trusted = match value.get_str("isTrusted") {
            Dynamic::Bool(flag) => Some(IsTrusted::Bool(flag)),
            options @ Dynamic::Object(_) => match options.get_str("enabledCommands") {
                Dynamic::Array(commands) => {
                    let enabled_commands = commands
                        .to_vec()
                        .into_iter()
                        .filter_map(|command| match command {
                            Dynamic::String(command) => Some(command.to_string_lossy()),
                            _ => None,
                        })
                        .collect();
                    Some(IsTrusted::Options(MarkdownStringTrustedOptions { enabled_commands }))
                }
                _ => None,
            },
            _ => None,
        };
        let base_uri = value.get_str("baseUri");
        Some(IMarkdownString {
            value: text,
            is_trusted,
            support_theme_icons: flag("supportThemeIcons"),
            support_html: flag("supportHtml"),
            support_alert_syntax: flag("supportAlertSyntax"),
            base_uri: if is_instance_of(&base_uri, "Uri") { URI::from_dynamic(&base_uri).ok() } else { None },
            uris: None,
        })
    }
}
