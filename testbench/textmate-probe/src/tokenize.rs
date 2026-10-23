//! Tokenização de um arquivo no formato da golden: o `Snapper` do `themes.test.contribution.ts`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use ferriki_textmate::{
    EmbeddedLanguages, EncodedTokenAttributes, GrammarConfiguration, RawTheme, StandardTokenType, SyncRegistry,
    parse_raw_grammar,
};

use crate::catalog::Catalog;
use crate::jsonc;
use crate::theme::Theme;

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

/// Variações de medição, desligadas por padrão.
#[derive(Clone, Copy, Default)]
pub struct Options {
    /// Reescreve `\p` sem `{` (aceito pelo Oniguruma, recusado pelo ferroni) para `p`, para medir o
    /// que sobra quando só esse defeito da crate é contornado.
    pub workaround_bare_p: bool,
    /// Controle negativo: sem injeções, o resultado precisa divergir das goldens.
    pub no_injections: bool,
}

/// Texto de cada gramática, lido uma vez (os arquivos `.json` já normalizados de JSONC).
pub struct GrammarSources {
    texts: Vec<(String, String)>,
    options: Options,
}

/// `\p` do regex (no texto JSON, duas barras e `p`) que não é seguido de `{`, vira `p`.
fn replace_bare_p(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '\\' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let run = chars[i..].iter().take_while(|&&c| c == '\\').count();
        let bare_p = chars.get(i + run) == Some(&'p') && run % 4 == 2 && chars.get(i + run + 1) != Some(&'{');
        if bare_p {
            out.extend(std::iter::repeat_n('\\', run - 2));
            out.push('p');
            i += run + 1;
        } else {
            out.extend(std::iter::repeat_n('\\', run));
            i += run;
        }
    }
    out
}

impl GrammarSources {
    pub fn load(catalog: &Catalog, options: Options) -> Self {
        let texts = catalog
            .grammars
            .iter()
            .filter_map(|g| {
                let path = g.path.to_string_lossy().to_string();
                let text = fs::read_to_string(&g.path).ok()?;
                let text = if path.ends_with(".json") {
                    jsonc::parse(&text).ok()?.to_string()
                } else {
                    text
                };
                let text = if options.workaround_bare_p { replace_bare_p(&text) } else { text };
                Some((path, text))
            })
            .collect();
        Self { texts, options }
    }
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

fn standard_token_type(name: &str) -> Option<StandardTokenType> {
    match name {
        "string" => Some(StandardTokenType::String),
        "other" => Some(StandardTokenType::Other),
        "comment" => Some(StandardTokenType::Comment),
        "regex" => Some(StandardTokenType::RegEx),
        _ => None,
    }
}

fn build_registry(catalog: &Catalog, sources: &GrammarSources) -> Result<SyncRegistry, String> {
    let mut registry = SyncRegistry::new(None, None).map_err(|e| e.to_string())?;
    for (path, text) in &sources.texts {
        let raw = parse_raw_grammar(text, Some(path)).map_err(|e| format!("gramática {path}: {e}"))?;
        registry.add_grammar(raw, Vec::new());
    }
    // `getInjections` do TMGrammarFactory soma as injeções de cada prefixo do escopo.
    let injections = if sources.options.no_injections { BTreeMap::new() } else { catalog.injections() };
    for g in &catalog.grammars {
        let parts: Vec<&str> = g.scope_name.split('.').collect();
        let combined: Vec<String> = (1..=parts.len())
            .flat_map(|n| injections.get(parts[..n].join(".").as_str()).cloned().unwrap_or_default())
            .map(str::to_string)
            .collect();
        if !combined.is_empty() {
            registry.set_injections(g.scope_name.clone(), combined);
        }
    }
    Ok(registry)
}

pub fn run(catalog: &Catalog, themes: &[Theme], sources: &GrammarSources, fixture: &Path) -> Outcome {
    let fail = |status: String| Outcome { tokens: Vec::new(), status };
    let name = fixture.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let Some(language) = catalog.guess_language(&name) else {
        return fail("sem linguagem".into());
    };
    let Some(scope) = catalog.language_scopes().get(language).map(|s| s.to_string()) else {
        return fail(format!("sem gramática para a linguagem {language}"));
    };
    let bytes = match fs::read(fixture) {
        Ok(b) => b,
        Err(e) => return fail(format!("leitura: {e}")),
    };
    let content = String::from_utf8_lossy(&bytes);
    let lines = split_lines(&content);

    let mut registry = match build_registry(catalog, sources) {
        Ok(r) => r,
        Err(e) => return fail(format!("registro: {e}")),
    };
    let embedded: EmbeddedLanguages = catalog
        .embedded_languages_for(&scope)
        .into_iter()
        .map(|(scope, lang)| (scope, catalog.encoded_language_id(&lang)))
        .collect();
    let token_types = catalog
        .grammars
        .iter()
        .rev()
        .find(|g| g.scope_name == scope)
        .map(|g| g.token_types.iter().filter_map(|(s, t)| Some((s.clone(), standard_token_type(t)?))).collect())
        .unwrap_or_default();
    let configuration = GrammarConfiguration::default()
        .with_initial_language_id(catalog.encoded_language_id(language))
        .with_embedded_languages(embedded)
        .with_token_types(token_types)
        .with_balanced_bracket_selectors(Some(vec!["*".to_string()]));
    let grammar = match registry.grammar_for_scope_name(&scope, configuration) {
        Ok(Some(g)) => g,
        Ok(None) => return fail(format!("gramática {scope} não registrada")),
        Err(e) => return fail(format!("tema: {e}")),
    };

    let mut status = String::new();
    let mut tokens: Vec<Token> = Vec::new();
    let mut themed: Vec<Vec<ThemedToken>> = Vec::new();
    for (theme_index, theme) in themes.iter().enumerate() {
        let raw: RawTheme = match serde_json::from_value(theme.raw_theme()) {
            Ok(r) => r,
            Err(e) => return fail(format!("tema {}: {e}", theme.id)),
        };
        if let Err(e) = registry.set_theme(Some(raw), None) {
            return fail(format!("tema {}: {e}", theme.id));
        }
        let color_map = registry.get_color_map();
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
fn enrich(tokens: &mut [Token], themes: &[Theme], themed: &mut [Vec<ThemedToken>]) {
    let mut themes: Vec<Theme> = themes.to_vec();
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
