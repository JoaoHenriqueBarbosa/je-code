//! Tokenização de um arquivo no formato da golden: o `Snapper` do `themes.test.contribution.ts`.
//!
//! A linguagem, a gramática (com as injeções e as linguagens embutidas) e o tema vêm do crate
//! `je-languages`, o mesmo caminho do produto; aqui só se pede ao ferriki a tokenização com escopos
//! (`tokenize_line_with_scopes`), que o `TokenizationRegistry` não expõe.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::rc::Rc;

use ferriki_textmate::EncodedTokenAttributes;
use je_base::uri::URI;
use je_editor::language_service::{ILanguageService, LanguageService};
use je_languages::extensions::ExtensionsCatalog;
use je_languages::text_mate_tokenization_feature::TextMateTokenizationFeature;
use je_languages::token_theme::TokenTheme;

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub c: String,
    pub t: String,
    pub r: BTreeMap<String, String>,
}

pub struct Outcome {
    pub tokens: Vec<Token>,
    /// Vazio quando a tokenização terminou; senão o motivo (sem linguagem, erro de regex...).
    pub status: String,
}

fn utf16_slice(units: &[u16], start: usize, end: usize) -> Vec<u16> {
    units[start.min(units.len())..end.min(units.len()).max(start.min(units.len()))].to_vec()
}

fn text_of(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

/// `splitLines` de `strings.ts`: quebra em `\r\n`, `\r` ou `\n`.
fn split_lines(content: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' => {
                lines.push(&content[start..i]);
                i += if bytes.get(i + 1) == Some(&b'\n') { 2 } else { 1 };
                start = i;
            }
            b'\n' => {
                lines.push(&content[start..i]);
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    lines.push(&content[start..]);
    lines
}

struct ThemedToken {
    text: Vec<u16>,
    color: Option<String>,
}

pub fn run(extensions: &Path, themes: &[TokenTheme], fixture: &Path) -> Outcome {
    let catalog = match ExtensionsCatalog::load(extensions) {
        Ok(c) => c,
        Err(e) => return Outcome { tokens: Vec::new(), status: format!("catálogo: {e}") },
    };
    let fail = |status: String| Outcome { tokens: Vec::new(), status };
    let name = fixture.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let bytes = match fs::read(fixture) {
        Ok(b) => b,
        Err(e) => return fail(format!("leitura: {e}")),
    };
    let content = String::from_utf8_lossy(&bytes);
    let lines = split_lines(&content);

    let language_service = LanguageService::new(false);
    language_service.set_dynamic_languages(catalog.languages.clone());
    let service: Rc<dyn ILanguageService> = language_service.clone();
    let uri = URI::file(format!("/{name}"), je_base::path::Platform::Posix);
    let Some(language) = service.guess_language_id_by_filepath_or_first_line(Some(&uri), None) else {
        return fail("sem linguagem".into());
    };
    let feature = TextMateTokenizationFeature::new(service, &catalog.grammars, catalog.max_tokenization_line_length.clone());
    let Some(grammar) = feature.create_tokenizer(&language) else {
        return fail(format!("sem gramática para a linguagem {language}"));
    };

    let mut status = String::new();
    let mut tokens: Vec<Token> = Vec::new();
    let mut themed: Vec<Vec<ThemedToken>> = Vec::new();
    for (theme_index, theme) in themes.iter().enumerate() {
        feature.set_theme(theme);
        let color_map = feature.color_map();
        let mut state = None;
        let mut themed_tokens = Vec::new();
        for line in &lines {
            let units: Vec<u16> = line.encode_utf16().collect();
            let result = match grammar.tokenize_line_with_scopes(line, state.clone(), 0) {
                Ok(r) => r,
                Err(e) => {
                    status = format!("erro de regex na linha {:?}: {e}", line);
                    break;
                }
            };
            if theme_index == 0 {
                let mut last_scopes: Option<String> = None;
                for token in &result.tokens {
                    let text = text_of(&utf16_slice(&units, token.start_index, token.end_index));
                    let joined = token.scopes.join(" ");
                    match (&last_scopes, tokens.last_mut()) {
                        (Some(last), Some(prev)) if *last == joined => prev.c.push_str(&text),
                        _ => {
                            last_scopes = Some(joined.clone());
                            tokens.push(Token { c: text, t: joined, r: BTreeMap::new() });
                        }
                    }
                }
            }
            let binary = &result.binary_tokens;
            for j in 0..binary.len() / 2 {
                let start = binary[j * 2] as usize;
                let end = if j + 1 < binary.len() / 2 { binary[(j + 1) * 2] as usize } else { units.len() };
                let foreground = EncodedTokenAttributes::new(binary[j * 2 + 1]).foreground() as usize;
                let color = color_map.get(foreground).filter(|c| foreground != 0 && !c.is_empty()).cloned();
                themed_tokens.push(ThemedToken { text: utf16_slice(&units, start, end), color });
            }
            state = Some(result.rule_stack);
        }
        themed.push(themed_tokens);
        if !status.is_empty() {
            break;
        }
    }

    enrich(&mut tokens, themes, &mut themed);
    tokens.retain(|t| !t.c.is_empty());
    Outcome { tokens, status }
}

/// `_enrichResult`: consome o texto dos tokens temáticos conforme avança pelos tokens agrupados.
fn enrich(tokens: &mut [Token], themes: &[TokenTheme], themed: &mut [Vec<ThemedToken>]) {
    let mut themes: Vec<TokenTheme> = themes.to_vec();
    let mut index = vec![0usize; themed.len()];
    for token in tokens.iter_mut() {
        let length = token.c.encode_utf16().count();
        for (ti, theme_tokens) in themed.iter_mut().enumerate() {
            let Some(themed_token) = theme_tokens.get_mut(index[ti]) else { continue };
            themed_token.text = themed_token.text.get(length..).unwrap_or_default().to_vec();
            if let Some(color) = &themed_token.color {
                let explanation = themes[ti].explain(&token.t, color);
                token.r.insert(themes[ti].id.clone(), explanation);
            }
            if themed_token.text.is_empty() {
                index[ti] += 1;
            }
        }
    }
}
