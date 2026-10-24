//! Porte parcial de `src/vs/workbench/contrib/update/browser/releaseNotesEditor.ts`: o tratamento do texto markdown das notas
//! de versão que antecede o renderer (`processConditionalBlocks` e o começo de `renderReleaseNotesMarkdown`).
//!
//! Recorte declarado:
//! - Ficam de fora o `ReleaseNotesManager` (webview, abertura do editor, cache e telemetria), o HTML do documento e o
//!   `applyReleaseNotesTokenization`, que dependem do DOM e do webview.
//! - A renderização em si (`renderMarkdownDocument` com o `marked`, o sanitizador de HTML e o `SimpleSettingRenderer`) não existe
//!   no porte ainda; o que está aqui é o texto que ela recebe.
//!
//! Decisões de tradução:
//! - As expressões regulares do original viram busca manual com a mesma semântica (guloso, preguiçoso, sem diferenciar caixa
//!   ASCII, `\s` do JavaScript e `\w` ASCII).

use std::collections::HashSet;

use crate::contrib::update::update_info_parser::is_js_whitespace;

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
