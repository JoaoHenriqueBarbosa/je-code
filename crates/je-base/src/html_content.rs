//! Porte de `src/vs/base/common/htmlContent.ts`: `IMarkdownString`, `MarkdownString` e os helpers de
//! escape e de criação de links em markdown.
//!
//! Texto é `JsString`/`&[u16]`, como no resto do porte. Diferenças de forma:
//!
//! - `baseUri` e `uris` são `UriComponents` no `IMarkdownString` do original e `URI` no `MarkdownString`
//!   (o `lift` os revive). Em Rust não há tipo estrutural, então o `IMarkdownString` já guarda `URI`:
//!   quem recebe um DTO de JSON o revive antes (`URI::revive`, ou o `revive` de `marshalling`);
//! - `isMarkdownString` confere a forma de um valor `unknown`; aqui ele confere um [`Dynamic`], e a
//!   ramificação `instanceof MarkdownString` não existe (um `MarkdownString` já é do tipo certo);
//! - `markdownStringEqual` compara `isTrusted` estruturalmente (o original, para as opções de comandos
//!   confiáveis, compara a referência do objeto);
//! - `isEqual` de `resources` é o do [`ExtUri`] que o chamador passa a `markdown_string_equal`;
//! - `createCommandUri` serializa os argumentos com `safe_stringify` (igual ao `JSON.stringify` para o que
//!   cabe em JSON; um ciclo ou `bigint`, que o `JSON.stringify` rejeitaria, vira `"[Circular]"` e
//!   `[BigInt n]`).

use std::cell::LazyCell;

use zyarr::RegExp;

use crate::dynamic::Dynamic;
use crate::icon_labels::escape_icons;
use crate::js_regexp::{match_all, replace_all};
use crate::js_string::{self, JsString};
use crate::map::OrderedMap;
use crate::network::{UriOrString, matches_some_scheme, schemas};
use crate::objects::safe_stringify;
use crate::resources::ExtUri;
use crate::uri::{UriComponents, URI};

/// `MarkdownStringTrustedOptions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownStringTrustedOptions {
    pub enabled_commands: Vec<String>,
}

/// `boolean | MarkdownStringTrustedOptions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IsTrusted {
    Bool(bool),
    Options(MarkdownStringTrustedOptions),
}

/// `IMarkdownString`.
#[derive(Debug, Clone, Default)]
pub struct IMarkdownString {
    pub value: JsString,
    pub is_trusted: Option<IsTrusted>,
    pub support_theme_icons: Option<bool>,
    pub support_html: Option<bool>,
    /// `@internal` no original.
    pub support_alert_syntax: Option<bool>,
    pub base_uri: Option<URI>,
    pub uris: Option<OrderedMap<JsString, URI>>,
}

/// `MarkdownStringTextNewlineStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MarkdownStringTextNewlineStyle {
    #[default]
    Paragraph = 0,
    Break = 1,
}

/// O objeto de opções do construtor do `MarkdownString`.
#[derive(Debug, Clone, Default)]
pub struct MarkdownStringOptions {
    pub is_trusted: Option<IsTrusted>,
    pub support_theme_icons: Option<bool>,
    pub support_html: Option<bool>,
    pub support_alert_syntax: Option<bool>,
}

impl From<&IMarkdownString> for MarkdownStringOptions {
    fn from(dto: &IMarkdownString) -> Self {
        Self {
            is_trusted: dto.is_trusted.clone(),
            support_theme_icons: dto.support_theme_icons,
            support_html: dto.support_html,
            support_alert_syntax: dto.support_alert_syntax,
        }
    }
}

/// `boolean | { isTrusted?, supportThemeIcons?, supportHtml?, supportAlertSyntax? }`.
#[derive(Debug, Clone)]
pub enum TrustedOrOptions {
    Trusted(bool),
    Options(MarkdownStringOptions),
}

impl Default for TrustedOrOptions {
    fn default() -> Self {
        Self::Trusted(false)
    }
}

#[derive(Debug, Clone)]
pub struct MarkdownString {
    pub value: JsString,
    pub is_trusted: Option<IsTrusted>,
    pub support_theme_icons: Option<bool>,
    pub support_html: Option<bool>,
    pub support_alert_syntax: Option<bool>,
    pub base_uri: Option<URI>,
    pub uris: Option<OrderedMap<JsString, URI>>,
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn js(text: &str) -> JsString {
    JsString::from(text)
}

thread_local! {
    static SPACES_AND_TABS: LazyCell<RegExp> = LazyCell::new(|| RegExp::from_str("([ \\t]+)", "").expect("regex válida"));
    static LEADING_DASH: LazyCell<RegExp> = LazyCell::new(|| RegExp::from_str("^([ \\t]*)-", "m").expect("regex válida"));
    static BACKTICK_FENCES: LazyCell<RegExp> = LazyCell::new(|| RegExp::from_str("^`+", "m").expect("regex válida"));
    static BACKTICK_RUNS: LazyCell<RegExp> = LazyCell::new(|| RegExp::from_str("`+", "").expect("regex válida"));
    static HEIGHT_PARAM: LazyCell<RegExp> = LazyCell::new(|| RegExp::from_str("height=(\\d+)", "").expect("regex válida"));
    static WIDTH_PARAM: LazyCell<RegExp> = LazyCell::new(|| RegExp::from_str("width=(\\d+)", "").expect("regex válida"));
    static PLACEHOLDER_AUTHORITY: LazyCell<RegExp> = LazyCell::new(|| RegExp::from_str("^https?:\\/\\/_[^/?#]*_(?:[/?#]|$)", "i").expect("regex válida"));
    static ABSOLUTE_PATH: LazyCell<RegExp> = LazyCell::new(|| RegExp::from_str("^(?:[/\\\\]|[a-z]:[/\\\\])", "i").expect("regex válida"));
    static HAS_SCHEME: LazyCell<RegExp> = LazyCell::new(|| RegExp::from_str("^[a-z][a-z0-9.+-]*:", "i").expect("regex válida"));
}

fn test(regex: &'static std::thread::LocalKey<LazyCell<RegExp>>, text: &[u16]) -> bool {
    regex.with(|regex| regex.exec(text, 0).is_some())
}

impl MarkdownString {
    pub fn lift(dto: &IMarkdownString) -> MarkdownString {
        let mut markdown_string = MarkdownString::new(Some(dto.value.clone()), TrustedOrOptions::Options(dto.into()));
        markdown_string.uris = dto.uris.clone();
        markdown_string.base_uri = dto.base_uri.clone();
        markdown_string
    }

    /// `new MarkdownString(value = '', isTrustedOrOptions = false)`.
    pub fn new(value: Option<JsString>, is_trusted_or_options: TrustedOrOptions) -> MarkdownString {
        let value = value.unwrap_or_default();

        match is_trusted_or_options {
            TrustedOrOptions::Trusted(is_trusted) => MarkdownString {
                value,
                is_trusted: Some(IsTrusted::Bool(is_trusted)),
                support_theme_icons: Some(false),
                support_html: Some(false),
                support_alert_syntax: Some(false),
                base_uri: None,
                uris: None,
            },
            TrustedOrOptions::Options(options) => MarkdownString {
                value,
                is_trusted: options.is_trusted,
                support_theme_icons: Some(options.support_theme_icons.unwrap_or(false)),
                support_html: Some(options.support_html.unwrap_or(false)),
                support_alert_syntax: Some(options.support_alert_syntax.unwrap_or(false)),
                base_uri: None,
                uris: None,
            },
        }
    }

    pub fn append_text(&mut self, value: &[u16], newline_style: MarkdownStringTextNewlineStyle) -> &mut Self {
        let source = if self.support_theme_icons == Some(true) { escape_icons(value) } else { JsString::from(value) };
        let escaped = escape_markdown_syntax_tokens(source.units()); // CodeQL [SM02383] The Markdown is fully sanitized after being rendered.
        let spaced = SPACES_AND_TABS.with(|regex| {
            replace_all(regex, escaped.units(), |captures| {
                // `'&nbsp;'.repeat(g1.length)`
                JsString::from("&nbsp;".repeat(captures.group(1).map_or(0, <[u16]>::len)))
            })
        });

        // `.replace(/\>/gm, '\\>')` e `.replace(/\n/g, ...)`: um caractere por vez.
        let newline = match newline_style {
            MarkdownStringTextNewlineStyle::Break => "\\\n",
            MarkdownStringTextNewlineStyle::Paragraph => "\n\n",
        };
        let mut out = JsString::new();
        for &unit in spaced.units() {
            match unit {
                0x3E => out.push_units(&units("\\>")),
                0x0A => out.push_units(&units(newline)),
                _ => out.push_unit(unit),
            }
        }
        self.value.push_units(out.units());

        self
    }

    pub fn append_markdown(&mut self, value: &[u16]) -> &mut Self {
        self.value.push_units(value);
        self
    }

    pub fn append_codeblock(&mut self, lang_id: &[u16], code: &[u16]) -> &mut Self {
        self.value.push_units(&units("\n"));
        self.value.push_units(append_escaped_markdown_code_block_fence(code, lang_id).units());
        self.value.push_units(&units("\n"));
        self
    }

    pub fn append_link(&mut self, target: UriOrOwnedString<'_>, label: &[u16], title: Option<&[u16]>) -> &mut Self {
        self.value.push_units(&units("["));
        self.value.push_units(escape_char(label, u16::from(b']')).units());
        self.value.push_units(&units("]("));
        self.value.push_units(escape_char(target.to_js_string().units(), u16::from(b')')).units());
        if let Some(title) = title.filter(|title| !title.is_empty()) {
            self.value.push_units(&units(" \""));
            let quoted = escape_char(title, u16::from(b'"'));
            self.value.push_units(escape_char(quoted.units(), u16::from(b')')).units());
            self.value.push_units(&units("\""));
        }
        self.value.push_units(&units(")"));
        self
    }
}

/// `URI | string`, o alvo de `appendLink` (`String(target)` o converte).
#[derive(Debug, Clone, Copy)]
pub enum UriOrOwnedString<'a> {
    Uri(&'a URI),
    Str(&'a [u16]),
}

impl UriOrOwnedString<'_> {
    fn to_js_string(self) -> JsString {
        match self {
            Self::Uri(uri) => uri.to_string(false),
            Self::Str(text) => JsString::from(text),
        }
    }
}

/// `_escape(value, ch)`: põe `\` antes de cada `ch` que não já vem depois de uma barra.
fn escape_char(value: &[u16], ch: u16) -> JsString {
    let mut out = JsString::new();
    for (offset, &unit) in value.iter().enumerate() {
        if unit == ch && !(offset > 0 && value[offset - 1] == u16::from(b'\\')) {
            out.push_unit(u16::from(b'\\'));
        }
        out.push_unit(unit);
    }
    out
}

/// `IMarkdownString | IMarkdownString[]`.
pub enum OneOrManyMarkdown<'a> {
    One(&'a IMarkdownString),
    Many(&'a [IMarkdownString]),
}

pub fn is_empty_markdown_string(one_or_many: Option<OneOrManyMarkdown<'_>>) -> bool {
    match one_or_many {
        Some(OneOrManyMarkdown::One(one)) => one.value.is_empty(),
        Some(OneOrManyMarkdown::Many(many)) => many.iter().all(|one| one.value.is_empty()),
        None => true,
    }
}

/// `isMarkdownString(thing)` sobre um valor dinâmico.
pub fn is_markdown_string(thing: &Dynamic) -> bool {
    if thing.type_of() != "object" || thing.is_null() {
        return false;
    }
    let field = |name: &str| thing.get_str(name);
    let is_bool_or_undefined = |value: Dynamic| matches!(value, Dynamic::Bool(_) | Dynamic::Undefined);
    let is_trusted = field("isTrusted");
    matches!(field("value"), Dynamic::String(_))
        && (matches!(is_trusted, Dynamic::Bool(_) | Dynamic::Undefined) || is_trusted.type_of() == "object")
        && is_bool_or_undefined(field("supportThemeIcons"))
        && is_bool_or_undefined(field("supportAlertSyntax"))
}

pub fn markdown_string_equal(a: &IMarkdownString, b: &IMarkdownString, ext_uri: &ExtUri) -> bool {
    a.value == b.value
        && a.is_trusted == b.is_trusted
        && a.support_theme_icons == b.support_theme_icons
        && a.support_html == b.support_html
        && a.support_alert_syntax == b.support_alert_syntax
        && match (&a.base_uri, &b.base_uri) {
            (None, None) => true,
            (Some(a), Some(b)) => ext_uri.is_equal(Some(a), Some(b), false),
            _ => false,
        }
}

const MARKDOWN_SYNTAX_TOKENS: &[u8] = b"\\`*_{}[]()#+!~";

pub fn escape_markdown_syntax_tokens(text: &[u16]) -> JsString {
    // escape markdown syntax tokens: http://daringfireball.net/projects/markdown/syntax#backslash
    let mut escaped = JsString::new();
    for &unit in text {
        if u8::try_from(unit).is_ok_and(|byte| MARKDOWN_SYNTAX_TOKENS.contains(&byte)) {
            escaped.push_unit(u16::from(b'\\'));
        }
        escaped.push_unit(unit);
    }
    LEADING_DASH.with(|regex| {
        replace_all(regex, escaped.units(), |captures| {
            let mut out = JsString::from(captures.group(1).unwrap_or_default());
            out.push_units(&units("\\-"));
            out
        })
    })
}

/// Escapes only the characters that would break out of markdown link text
/// (`[label](url)` syntax: `\` and `]`. Use this when the escaped string is
/// displayed as the visible label of a link, since renderers that extract the
/// link text without re-parsing markdown (e.g. the chat inline anchor / skill
/// pill) would otherwise show full `escapeMarkdownSyntaxTokens` backslashes
/// (`\-`, `\.`, ...) verbatim.
pub fn escape_markdown_link_label(text: &[u16]) -> JsString {
    let mut out = JsString::new();
    for &unit in text {
        if unit == u16::from(b'\\') || unit == u16::from(b']') {
            out.push_unit(u16::from(b'\\'));
        }
        out.push_unit(unit);
    }
    out
}

/// @see https://github.com/microsoft/vscode/issues/193746
pub fn append_escaped_markdown_code_block_fence(code: &[u16], lang_id: &[u16]) -> JsString {
    let longest_fence_length = BACKTICK_FENCES.with(|regex| match_all(regex, code)).iter().map(|m| m.end() - m.start()).max().unwrap_or(0);
    let desired_fence_length = if longest_fence_length >= 3 { longest_fence_length + 1 } else { 3 };

    // the markdown result
    let fence = "`".repeat(desired_fence_length);
    let mut out = JsString::from(fence.as_str());
    out.push_units(lang_id);
    out.push_units(&units("\n"));
    out.push_units(code);
    out.push_units(&units(&format!("\n{fence}")));
    out
}

/// Wraps arbitrary text in a markdown inline code span using a backtick fence
/// long enough to safely contain any backtick sequences present in the text.
///
/// Backticks inside an inline code span cannot be backslash-escaped per the
/// CommonMark spec; the only safe way is to choose a delimiter run longer
/// than any run of backticks in the content (and pad with spaces if the
/// content begins or ends with a backtick).
pub fn append_escaped_markdown_inline_code(text: &[u16]) -> JsString {
    let longest_backtick_run = BACKTICK_RUNS.with(|regex| match_all(regex, text)).iter().map(|m| m.end() - m.start()).max().unwrap_or(0);
    let fence = "`".repeat(longest_backtick_run + 1);
    let backtick = u16::from(b'`');
    let needs_space = text.first() == Some(&backtick) || text.last() == Some(&backtick);
    let mut out = JsString::from(fence.as_str());
    if needs_space {
        out.push_units(&units(" "));
    }
    out.push_units(text);
    if needs_space {
        out.push_units(&units(" "));
    }
    out.push_units(&units(&fence));
    out
}

pub fn escape_double_quotes(input: &[u16]) -> JsString {
    let mut out = JsString::new();
    for &unit in input {
        if unit == u16::from(b'"') {
            out.push_units(&units("&quot;"));
        } else {
            out.push_unit(unit);
        }
    }
    out
}

pub fn remove_markdown_escapes(text: &[u16]) -> JsString {
    // `/\\([\\`*_{}[\]()#+\-.!~])/g` com `$1`
    let escapable = |unit: u16| u8::try_from(unit).is_ok_and(|byte| b"\\`*_{}[]()#+-.!~".contains(&byte));
    let mut out = JsString::new();
    let mut i = 0;
    while i < text.len() {
        if text[i] == u16::from(b'\\') && text.get(i + 1).is_some_and(|&next| escapable(next)) {
            out.push_unit(text[i + 1]);
            i += 2;
        } else {
            out.push_unit(text[i]);
            i += 1;
        }
    }
    out
}

/// O resultado de `parseHrefAndDimensions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HrefAndDimensions {
    pub href: JsString,
    pub dimensions: Vec<String>,
}

pub fn parse_href_and_dimensions(href: &[u16]) -> HrefAndDimensions {
    let mut dimensions = Vec::new();
    let splitted: Vec<&[u16]> = href.split(|&unit| unit == u16::from(b'|')).map(js_string::trim).collect();
    let href = splitted[0];
    if let Some(parameters) = splitted.get(1).filter(|parameters| !parameters.is_empty()) {
        let digits = |regex: &'static std::thread::LocalKey<LazyCell<RegExp>>| {
            regex
                .with(|regex| regex.exec(parameters, 0))
                .and_then(|found| found.captures.get(1).cloned().flatten())
                .map(|range| String::from_utf16_lossy(&parameters[range]))
        };
        // `isFinite(parseInt(width))`: o grupo é só dígitos, então é finito quando existe.
        if let Some(width) = digits(&WIDTH_PARAM) {
            dimensions.push(format!("width=\"{width}\""));
        }
        if let Some(height) = digits(&HEIGHT_PARAM) {
            dimensions.push(format!("height=\"{height}\""));
        }
    }
    HrefAndDimensions { href: JsString::from(href), dimensions }
}

pub fn create_markdown_link(text: &[u16], href: &[u16], title: Option<&[u16]>, escape_tokens: bool) -> JsString {
    let mut out = JsString::from("[");
    out.push_units(if escape_tokens { escape_markdown_syntax_tokens(text) } else { JsString::from(text) }.units());
    out.push_units(&units("]("));
    out.push_units(href);
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        out.push_units(&units(" \""));
        out.push_units(escape_markdown_syntax_tokens(title).units());
        out.push_units(&units("\""));
    }
    out.push_units(&units(")"));
    out
}

/// O `{ text, id, arguments?, tooltip }` de `createMarkdownCommandLink`.
pub struct MarkdownCommand<'a> {
    pub text: &'a [u16],
    pub id: &'a [u16],
    pub arguments: Option<&'a [Dynamic]>,
    pub tooltip: &'a [u16],
}

pub fn create_markdown_command_link(command: &MarkdownCommand<'_>, escape_tokens: bool) -> JsString {
    let uri = create_command_uri(command.id, command.arguments.unwrap_or_default()).to_string(false);
    create_markdown_link(command.text, uri.units(), Some(command.tooltip), escape_tokens)
}

pub fn create_command_uri(command_id: &[u16], command_args: &[Dynamic]) -> URI {
    let query = (!command_args.is_empty()).then(|| {
        let json = safe_stringify(&Dynamic::array(command_args.iter().cloned())).unwrap_or_default();
        JsString::from_units(crate::uri::encode_uri_component(&units(&json)))
    });
    URI::from(
        &UriComponents {
            scheme: Some(js(schemas::COMMAND)),
            path: Some(JsString::from(command_id)),
            query,
            ..UriComponents::default()
        },
        false,
    )
}

/// VS Code addresses some content with placeholder `http` URLs whose authority is wrapped in
/// underscores, such as `http://_vscodecontentref_/0`. Underscores are illegal in hostnames, so
/// no real site collides with this shape.
///
/// Whether a target still resolves once content is copied into another application.
pub fn is_portable_link_target(target: &[u16]) -> bool {
    let trimmed = js_string::trim(target);
    let text = String::from_utf16_lossy(trimmed);
    matches_some_scheme(UriOrString::Str(&text), &[schemas::HTTP, schemas::HTTPS, schemas::MAILTO]) && !test(&PLACEHOLDER_AUTHORITY, trimmed)
}

/// Whether a markdown target is worth keeping when the markdown itself is shared. Wider than
/// [`is_portable_link_target`]: document-relative targets such as `#section` name no machine,
/// so they stay meaningful in whatever document the markdown lands in.
///
/// Os caminhos absolutos são reconhecidos por regex própria, não pelo `extpath`, cujo tratamento da letra
/// de unidade segue a plataforma do hospedeiro: um caminho do Windows precisa ser reconhecido também
/// quando a cópia acontece no macOS.
pub fn is_portable_markdown_target(target: &[u16]) -> bool {
    let trimmed = js_string::trim(target);
    is_portable_link_target(trimmed) || (!trimmed.is_empty() && !test(&HAS_SCHEME, trimmed) && !test(&ABSOLUTE_PATH, trimmed))
}
