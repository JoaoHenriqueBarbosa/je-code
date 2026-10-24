//! `extHostTypeConverters.ts`: `MarkdownString`, a conversão entre o `vscode.MarkdownString` (e o
//! `vscode.MarkedString`) e o `IMarkdownString` do protocolo.
//!
//! Divergências:
//! - A extração de `uris` (`res.uris`), que o original faz percorrendo os tokens do `marked`
//!   (`marked.walkTokens(marked.lexer(value))`, para cada `link` e `image`), NÃO está portada: o lexer
//!   Markdown ainda não existe no porte. O `IMarkdownString` sai com `uris: None`. É a única parte do
//!   conversor que falta; quando o lexer existir, entram aqui o `collectUri` e o `_uriMassage`.
//! - `isCodeblock`/`isMarkdownString`/`typeof markup === 'string'` (duck typing) são as variantes de
//!   [`types::MarkedString`]; o ramo final `{ value: '' }` (valor que não é nenhum dos três) não existe.

use je_base::html_content::IMarkdownString;
use je_base::JsString;
use je_editor::languages::StringOrMarkdown;

use crate::api::common::ext_host_types as types;

/// `typeConverters.MarkdownString`.
pub struct MarkdownString;

impl MarkdownString {
    pub fn from_many(markup: &[types::MarkedString]) -> Vec<IMarkdownString> {
        markup.iter().map(MarkdownString::from).collect()
    }

    /// O ramo de `vscode.MarkdownString` de `from`.
    pub fn from_markdown(markup: &types::MarkdownString) -> IMarkdownString {
        IMarkdownString {
            value: markup.value.clone(),
            is_trusted: markup.is_trusted.clone(),
            support_theme_icons: markup.support_theme_icons,
            support_html: markup.support_html,
            support_alert_syntax: markup.support_alert_syntax,
            base_uri: markup.base_uri.clone(),
            uris: None,
        }
    }

    pub fn from(markup: &types::MarkedString) -> IMarkdownString {
        match markup {
            types::MarkedString::Code { language, value } => {
                IMarkdownString { value: JsString::from(format!("```{language}\n{value}\n```\n")), ..IMarkdownString::default() }
            }
            types::MarkedString::Markdown(markdown) => MarkdownString::from_markdown(markdown),
            types::MarkedString::Text(text) => IMarkdownString { value: JsString::from(text.as_str()), ..IMarkdownString::default() },
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
