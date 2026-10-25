//! Porte parcial de `src/vs/workbench/contrib/update/browser/releaseNotesEditor.ts`: o tratamento do texto markdown das notas
//! de versão que antecede o renderer (`processConditionalBlocks`) e `renderReleaseNotesMarkdown`, o renderer das notas de
//! versão sobre o markdown (`renderMarkdownDocument`, o `SimpleSettingRenderer` e os links "Try This").
//!
//! Recorte declarado:
//! - Ficam de fora o `ReleaseNotesManager` (webview, abertura do editor, cache e telemetria), o HTML do documento e o
//!   `applyReleaseNotesTokenization`, que dependem do DOM e do webview.
//!
//! Decisões de tradução:
//! - As expressões regulares do original viram busca manual com a mesma semântica (guloso, preguiçoso, sem diferenciar caixa
//!   ASCII, `\s` do JavaScript e `\w` ASCII).
//! - A extensão do `marked` que o original passa a `renderMarkdownDocument` é a [`ReleaseNotesExtension`]: o `tokenizer.codespan`
//!   que guarda os códigos fora de link e de bloco bruto em `tryoutCodeSpans` é a leitura de `in_link` e `in_raw_block` do
//!   token; o `WeakMap` dos ids dos blocos de código (`codeBlockIds`) é uma fila na ordem do documento, que o `walkTokens`
//!   enche e o renderizador `code` esvazia (os dois percorrem os tokens na mesma ordem).
//! - `TrustedHTML` é o [`JsString`] do HTML sanitizado; `safeSetInnerHtml(container, content, config)` é o `sanitize_html` do
//!   porte com o resultado como filhos do `Element` `container`, e `container.innerHTML` é `inner_html`.

use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::rc::Rc;

use je_base::JsString;
use je_base::browser::html_parser::Element;
use je_base::cancellation::CancellationToken;
use je_base::markdown::dom::inner_html;
use je_base::markdown::sanitize::{Protocols, sanitize_html};
use je_base::marked::{Code, Codespan, Renderer, Token};
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uuid::generate_uuid;
use je_editor::language_service::ILanguageService;

use crate::contrib::markdown::markdown_document_renderer::{
    MarkdownDocumentSanitizerConfig, MarkedExtension, RenderMarkdownDocumentOptions, markdown_document_dom_config, render_markdown_document,
    serialize_nodes,
};
use crate::contrib::markdown::markdown_setting_renderer::SimpleSettingRenderer;
use crate::contrib::update::browser::release_notes_tryouts::ReleaseNotesTryouts;
use crate::contrib::update::update_info_parser::is_js_whitespace;
use crate::services::extensions::extensions::IExtensionService;

/// `text.startsWith(prefix)` sem diferenciar caixa ASCII (o `prefix` é ASCII).
fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    text.as_bytes().get(..prefix.len()).is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
}

/// Pula `\s*` a partir de `from` e devolve o índice seguinte.
fn skip_whitespace(text: &str, from: usize) -> usize {
    from + text[from..].chars().take_while(|&c| is_js_whitespace(c)).map(char::len_utf8).sum::<usize>()
}

/// `\w`.
fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Casa, em `at`, o fecho `%ENDIF\s*%\s*-->` e devolve o índice logo depois.
fn match_conditional_close(text: &str, at: usize) -> Option<usize> {
    let rest = &text[at..];
    if !starts_with_ignore_case(rest, "%ENDIF") {
        return None;
    }
    let after_blanks = skip_whitespace(text, at + "%ENDIF".len());
    if !text[after_blanks..].starts_with('%') {
        return None;
    }
    let after_tail = skip_whitespace(text, after_blanks + 1);
    text[after_tail..].starts_with("-->").then_some(after_tail + 3)
}

/// Casa, em `at`, `<!--\s*%IF\s+(\w+)\s*%([\s\S]*?)%ENDIF\s*%\s*-->` (sem diferenciar caixa) e devolve
/// `(condição, conteúdo, fim do casamento)`.
fn match_conditional_block(text: &str, at: usize) -> Option<(&str, &str, usize)> {
    if !text[at..].starts_with("<!--") {
        return None;
    }
    let after_open = skip_whitespace(text, at + 4);
    if !starts_with_ignore_case(&text[after_open..], "%IF") {
        return None;
    }
    let after_if = after_open + 3;
    let after_gap = skip_whitespace(text, after_if);
    if after_gap == after_if {
        return None;
    }
    let condition_len: usize = text[after_gap..].chars().take_while(|&c| is_word_char(c)).map(char::len_utf8).sum();
    if condition_len == 0 {
        return None;
    }
    let condition_end = after_gap + condition_len;
    let after_condition = skip_whitespace(text, condition_end);
    if !text[after_condition..].starts_with('%') {
        return None;
    }
    let content_start = after_condition + 1;

    // O `[\s\S]*?` é preguiçoso: vale o primeiro ponto em que o fecho casa.
    let mut content_end = content_start;
    loop {
        if let Some(end) = match_conditional_close(text, content_end) {
            return Some((&text[after_gap..condition_end], &text[content_start..content_end], end));
        }
        content_end += text[content_end..].chars().next()?.len_utf8();
    }
}

/// Processes conditional blocks in the release notes markdown.
///
/// Conditional blocks use a single HTML comment with the format:
/// ```text
/// <!-- %IF CONDITION %
/// Content only visible when CONDITION is active.
/// %ENDIF % -->
/// ```
///
/// Supported conditions:
/// - `IN_PRODUCT`: content shown in VS Code (both Stable and Insiders)
/// - `WEB`: content shown on the website only
/// - `STABLE`: content shown in VS Code Stable only
/// - `INSIDERS`: content shown in VS Code Insiders only
/// - `TRYOUTS`: content shown only when the local Try This renderer is provided
///
/// On the website, the entire block is a single HTML comment, so the content is hidden by default. The website
/// renderer would activate `WEB` blocks by stripping the comment markers.
pub fn process_conditional_blocks(text: &str, active_conditions: &HashSet<String>) -> String {
    let mut result = String::with_capacity(text.len());
    let mut position = 0;
    while position < text.len() {
        match match_conditional_block(text, position) {
            Some((condition, content, end)) => {
                if active_conditions.contains(&condition.to_uppercase()) {
                    // Strip comment markers, reveal content
                    result.push_str(content);
                }
                // Remove the entire block
                position = end;
            }
            None => {
                let next = text[position..].chars().next().map_or(1, char::len_utf8);
                result.push_str(&text[position..position + next]);
                position += next;
            }
        }
    }
    result
}

/// `text.replace(/<!--\s*TOC\s*/gi, '')`.
fn remove_toc_open(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut position = 0;
    while position < text.len() {
        let matched_end = text[position..].starts_with("<!--").then(|| skip_whitespace(text, position + 4)).and_then(|after_gap| {
            starts_with_ignore_case(&text[after_gap..], "TOC").then(|| skip_whitespace(text, after_gap + 3))
        });
        match matched_end {
            Some(end) => position = end,
            None => {
                let next = text[position..].chars().next().map_or(1, char::len_utf8);
                result.push_str(&text[position..position + next]);
                position += next;
            }
        }
    }
    result
}

/// `text.replace(/\s*Navigation End\s*-->/gi, '')`.
fn remove_navigation_end(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut position = 0;
    while position < text.len() {
        let after_gap = skip_whitespace(text, position);
        let matched_end = starts_with_ignore_case(&text[after_gap..], "Navigation End")
            .then(|| skip_whitespace(text, after_gap + "Navigation End".len()))
            .filter(|&after_tail| text[after_tail..].starts_with("-->"))
            .map(|after_tail| after_tail + 3);
        match matched_end {
            Some(end) => position = end,
            None => {
                let next = text[position..].chars().next().map_or(1, char::len_utf8);
                result.push_str(&text[position..position + next]);
                position += next;
            }
        }
    }
    result
}

/// As condições ativas de `renderReleaseNotesMarkdown`: `IN_PRODUCT` sempre, `STABLE` ou `INSIDERS` conforme a
/// qualidade do produto e `TRYOUTS` quando há o renderer de tryouts.
pub fn release_notes_active_conditions(quality: Option<&str>, has_tryouts: bool) -> HashSet<String> {
    let mut active_conditions = HashSet::from(["IN_PRODUCT".to_owned()]);
    match quality {
        Some("stable") => {
            active_conditions.insert("STABLE".to_owned());
        }
        Some("insider") => {
            active_conditions.insert("INSIDERS".to_owned());
        }
        _ => {}
    }
    if has_tryouts {
        active_conditions.insert("TRYOUTS".to_owned());
    }
    active_conditions
}

/// O texto que `renderReleaseNotesMarkdown` entrega ao renderer: sem os marcadores de comentário em volta da navegação do
/// índice e com os blocos condicionais resolvidos.
pub fn prepare_release_notes_markdown(text: &str, quality: Option<&str>, has_tryouts: bool) -> String {
    // Remove HTML comment markers around table of contents navigation
    let text = remove_navigation_end(&remove_toc_open(text));

    // Process conditional blocks based on active conditions
    process_conditional_blocks(&text, &release_notes_active_conditions(quality, has_tryouts))
}

/// `IReleaseNotesCodeBlock`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseNotesCodeBlock {
    pub id: String,
    pub text: String,
    pub language: Option<String>,
}

/// A extensão do `marked` que `renderReleaseNotesMarkdown` passa a `renderMarkdownDocument`.
struct ReleaseNotesExtension {
    html_renderer: Box<dyn Fn(&str) -> String>,
    code_span_renderer: Box<dyn Fn(&str) -> String>,
    tryouts: Option<Rc<ReleaseNotesTryouts>>,
    code_blocks: Option<Rc<RefCell<Vec<ReleaseNotesCodeBlock>>>>,
    code_block_ids: RefCell<VecDeque<String>>,
}

impl MarkedExtension for ReleaseNotesExtension {
    fn walk_token(&self, token: &Token) {
        if let (Some(code_blocks), Token::Code(code)) = (&self.code_blocks, token) {
            let id = format!("release-notes-code-{}", generate_uuid());
            self.code_block_ids.borrow_mut().push_back(id.clone());
            code_blocks.borrow_mut().push(ReleaseNotesCodeBlock {
                id,
                text: code.text.to_string_lossy(),
                language: code.lang.as_ref().map(JsString::to_string_lossy),
            });
        }
    }

    /// `html: simpleSettingRenderer.getHtmlRenderer()`.
    fn html(&self, raw: &str) -> Option<JsString> {
        Some(JsString::from((self.html_renderer)(raw).as_str()))
    }

    /// `codespan: token => (tryoutCodeSpans.has(token) ? tryouts?.renderCodeSpan(token.text) : undefined) ?? renderCodeSpan(token)`.
    fn codespan(&self, token: &Codespan) -> Option<JsString> {
        let text = token.text.to_string_lossy();
        let tryout = if token.in_link || token.in_raw_block { None } else { self.tryouts.as_ref().and_then(|tryouts| tryouts.render_code_span(&text)) };
        Some(JsString::from(tryout.unwrap_or_else(|| (self.code_span_renderer)(&text)).as_str()))
    }

    /// `code: token => codeBlocks ? renderer.code(token).replace('<code', `<code id="..."`) : false`.
    fn code(&self, token: &Code) -> Option<JsString> {
        self.code_blocks.as_ref()?;
        let id = self.code_block_ids.borrow_mut().pop_front()?;
        let rendered = Renderer.code(token).to_string_lossy();
        Some(JsString::from(rendered.replacen("<code", &format!("<code id=\"{id}\""), 1).as_str()))
    }
}

/// O `sanitizerConfig` de `renderReleaseNotesMarkdown`.
fn release_notes_sanitizer_config() -> MarkdownDocumentSanitizerConfig {
    MarkdownDocumentSanitizerConfig {
        allow_relative_media_paths: true,
        allowed_link_protocols_override: Some(Protocols::List(
            [schemas::HTTP, schemas::HTTPS, schemas::COMMAND, schemas::CODE_SETTING].iter().map(|scheme| (*scheme).to_owned()).collect(),
        )),
        allowed_tags_augment: ["nav", "svg", "path"].iter().map(|tag| (*tag).to_owned()).collect(),
        allowed_attributes_augment: ["aria-role", "viewBox", "fill", "xmlns", "d"].iter().map(|name| (*name).to_owned()).collect(),
        ..MarkdownDocumentSanitizerConfig::default()
    }
}

/// O `tryoutSanitizerConfig`: o `sanitizerConfig` com `button` entre as tags e os atributos dos controles de tryout.
fn tryout_sanitizer_config() -> MarkdownDocumentSanitizerConfig {
    let mut config = release_notes_sanitizer_config();
    config.allowed_tags_augment.push("button".to_owned());
    config.allowed_attributes_augment.extend(
        ["type", "hidden", "aria-label", "aria-disabled", "data-release-notes-tryout-id", "data-release-notes-tryout-index"].iter().map(|name| (*name).to_owned()),
    );
    config
}

/// O fim de `renderReleaseNotesMarkdown`: com links de tryout, o conteúdo é montado num contêiner, os links viram os controles
/// e o resultado é sanitizado de novo.
fn render_tryouts(content: JsString, tryouts: Option<&Rc<ReleaseNotesTryouts>>) -> JsString {
    let Some(tryouts) = tryouts else { return content };
    if !tryouts.needs_render(&content.to_string_lossy()) {
        return content;
    }
    let config = markdown_document_dom_config(Some(&tryout_sanitizer_config()));
    let mut container = Element::new("div", Vec::new());
    container.children = sanitize_html(content.units(), &config);
    tryouts.render(&mut container);
    serialize_nodes(sanitize_html(inner_html(&container).units(), &config))
}

/// `renderReleaseNotesMarkdown(text, extensionService, languageService, simpleSettingRenderer, quality, tryouts, codeBlocks)`.
pub fn render_release_notes_markdown(
    text: &str,
    extension_service: Rc<dyn IExtensionService>,
    language_service: Rc<dyn ILanguageService>,
    simple_setting_renderer: &Rc<SimpleSettingRenderer>,
    quality: Option<&str>,
    tryouts: Option<Rc<ReleaseNotesTryouts>>,
    code_blocks: Option<Rc<RefCell<Vec<ReleaseNotesCodeBlock>>>>,
) -> Promise<JsString> {
    let text = prepare_release_notes_markdown(text, quality, tryouts.is_some());
    let extension = ReleaseNotesExtension {
        html_renderer: Box::new(simple_setting_renderer.get_html_renderer()),
        code_span_renderer: Box::new(simple_setting_renderer.get_code_span_renderer()),
        tryouts: tryouts.clone(),
        code_blocks,
        code_block_ids: RefCell::new(VecDeque::new()),
    };
    let options = RenderMarkdownDocumentOptions { sanitizer_config: Some(release_notes_sanitizer_config()), marked_extensions: vec![Rc::new(extension)] };
    render_markdown_document(&text, extension_service, language_service, Some(options), CancellationToken::None)
        .then(move |content| render_tryouts(content, tryouts.as_ref()))
}
