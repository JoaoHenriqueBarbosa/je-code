//! `extHostTypes/markdownString.ts`.
//!
//! Divergências: `MarkdownString` do original delega tudo a um `MarkdownString` base privado (`#delegate`);
//! aqui o delegado é o [`BaseMarkdownString`] de `je-base`, exposto por [`Deref`]/[`DerefMut`] (os
//! getters e setters `value`, `isTrusted`, `supportThemeIcons`, `supportHtml`, `supportAlertSyntax`,
//! `baseUri` são os campos públicos dele). `isMarkdownString` (duck typing) não existe.

use std::ops::{Deref, DerefMut};

use je_base::JsString;
use je_base::html_content::{MarkdownString as BaseMarkdownString, MarkdownStringOptions, MarkdownStringTextNewlineStyle, TrustedOrOptions};

/// `vscode.MarkdownString`.
#[derive(Clone, Debug)]
pub struct MarkdownString {
    delegate: BaseMarkdownString,
}

impl Deref for MarkdownString {
    type Target = BaseMarkdownString;

    fn deref(&self) -> &BaseMarkdownString {
        &self.delegate
    }
}

impl DerefMut for MarkdownString {
    fn deref_mut(&mut self) -> &mut BaseMarkdownString {
        &mut self.delegate
    }
}

impl MarkdownString {
    /// `new MarkdownString(value?, supportThemeIcons = false)`.
    pub fn new(value: Option<&str>, support_theme_icons: bool) -> MarkdownString {
        let options = MarkdownStringOptions { support_theme_icons: Some(support_theme_icons), ..MarkdownStringOptions::default() };
        MarkdownString { delegate: BaseMarkdownString::new(value.map(JsString::from), TrustedOrOptions::Options(options)) }
    }

    pub fn append_text(&mut self, value: &str) -> &mut Self {
        self.delegate.append_text(JsString::from(value).units(), MarkdownStringTextNewlineStyle::default());
        self
    }

    pub fn append_markdown(&mut self, value: &str) -> &mut Self {
        self.delegate.append_markdown(JsString::from(value).units());
        self
    }

    /// `appendCodeblock(value, language?)`: a linguagem padrão é `''`.
    pub fn append_codeblock(&mut self, value: &str, language: Option<&str>) -> &mut Self {
        self.delegate.append_codeblock(JsString::from(language.unwrap_or("")).units(), JsString::from(value).units());
        self
    }
}
