//! Os `link` e `image` de um texto Markdown, o que `MarkdownString.from` de `extHostTypeConverters.ts` lê do lexer
//! (`marked.walkTokens(marked.lexer(value), ...)`).
//!
//! O `marked` ainda não existe no porte. Este módulo é um léxico de inline que cobre o que o `from` consome, na ordem em que o
//! `walkTokens` visita os tokens:
//! - links `[texto](destino "título")` e imagens `![alt](destino "título")`, com colchetes aninhados, destino `<...>` ou com
//!   parênteses balanceados, título entre aspas ou parênteses e escapes com `\`; o destino sai com os escapes de pontuação
//!   removidos (o `anyPunctuation` do `marked`), e o texto de um link é lido de novo (os links e imagens dentro dele também contam);
//! - autolinks `<esquema:resto>` e `<email>`, e as extensões GFM: `http://` e `https://`, `www.` (`href` com `http://`) e e-mail
//!   (`href` com `mailto:`), fora de um texto de link;
//! - blocos de código cercados (` ``` ` e `~~~`) e trechos de código com crases são ignorados.
//!
//! NÃO cobre: links de referência (`[texto][id]` com `[id]: destino`), HTML embutido, entidades e as regras de limite de palavra
//! do GFM além do caso de início de palavra. Quando o `marked` for portado, este módulo some e o `from` usa o lexer.

/// Um `link` ou `image` do lexer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkdownLink {
    pub href: String,
    pub is_image: bool,
}

/// Os links e as imagens de `text`, na ordem em que o `walkTokens` os visita.
pub fn collect_links(text: &str) -> Vec<MarkdownLink> {
    let mut out = Vec::new();
    let mut in_fence: Option<(char, usize)> = None;
    let mut inline = String::new();
    // O código cercado sai; o resto é lido como inline (linhas juntas, para um link poder ocupar mais de uma).
    for line in text.split('\n') {
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        let fence_char = trimmed.chars().next().filter(|c| *c == '`' || *c == '~');
        let fence_len = fence_char.map_or(0, |c| trimmed.chars().take_while(|d| *d == c).count());
        match in_fence {
            Some((c, len)) => {
                if indent < 4 && fence_char == Some(c) && fence_len >= len && trimmed[fence_len..].trim().is_empty() {
                    in_fence = None;
                }
            }
            None => {
                if indent < 4 && fence_len >= 3 && fence_char.is_some() && !(fence_char == Some('`') && trimmed[fence_len..].contains('`')) {
                    in_fence = fence_char.map(|c| (c, fence_len));
                } else {
                    inline.push_str(line);
                    inline.push('\n');
                }
            }
        }
    }
    let chars: Vec<char> = inline.chars().collect();
    lex_inline(&chars, false, &mut out);
    out
}

fn is_ascii_punctuation(c: char) -> bool {
    c.is_ascii_punctuation()
}

/// `href.replace(anyPunctuation, '$1')`: tira a barra invertida que escapa um sinal de pontuação.
fn unescape_punctuation(href: &str) -> String {
    let chars: Vec<char> = href.chars().collect();
    let mut out = String::with_capacity(href.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && chars.get(i + 1).is_some_and(|c| is_ascii_punctuation(*c)) {
            out.push(chars[i + 1]);
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn lex_inline(chars: &[char], in_link: bool, out: &mut Vec<MarkdownLink>) {
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' if chars.get(i + 1).is_some_and(|next| is_ascii_punctuation(*next)) => i += 2,
            '`' => i = skip_code_span(chars, i),
            '<' => match parse_autolink(chars, i) {
                Some((href, end)) => {
                    out.push(MarkdownLink { href, is_image: false });
                    i = end;
                }
                None => i += 1,
            },
            '!' if chars.get(i + 1) == Some(&'[') => match parse_link(chars, i + 1) {
                Some(link) => {
                    out.push(MarkdownLink { href: link.href, is_image: true });
                    i = link.end;
                }
                None => i += 1,
            },
            '[' => match parse_link(chars, i) {
                Some(link) => {
                    out.push(MarkdownLink { href: link.href, is_image: false });
                    lex_inline(&chars[link.text_start..link.text_end], true, out);
                    i = link.end;
                }
                None => i += 1,
            },
            _ => {
                let boundary = i == 0 || !(chars[i - 1].is_alphanumeric() || matches!(chars[i - 1], '.' | '!' | '#' | '$' | '%' | '&' | '\'' | '+' | '/' | '=' | '?' | '^' | '_' | '`' | '{' | '|' | '}' | '~' | '-' | '@'));
                let url = if in_link || !boundary { None } else { parse_gfm_url(chars, i) };
                match url {
                    Some((href, end)) => {
                        out.push(MarkdownLink { href, is_image: false });
                        i = end;
                    }
                    None => i += 1,
                }
            }
        }
    }
}

/// O fim de um trecho de código que começa em `start` (uma sequência de crases); sem o fechamento, só pula as crases.
fn skip_code_span(chars: &[char], start: usize) -> usize {
    let run = chars[start..].iter().take_while(|c| **c == '`').count();
    let mut i = start + run;
    while i < chars.len() {
        if chars[i] == '`' {
            let close = chars[i..].iter().take_while(|c| **c == '`').count();
            if close == run {
                return i + close;
            }
            i += close;
        } else {
            i += 1;
        }
    }
    start + run
}

struct ParsedLink {
    text_start: usize,
    text_end: usize,
    href: String,
    end: usize,
}

/// `[texto](destino "título")` com o `[` em `open`.
fn parse_link(chars: &[char], open: usize) -> Option<ParsedLink> {
    // o `]` que fecha o texto
    let mut depth = 0usize;
    let mut i = open;
    let close = loop {
        match *chars.get(i)? {
            '\\' => i += 1,
            '`' => {
                i = skip_code_span(chars, i);
                continue;
            }
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    break i;
                }
            }
            _ => {}
        }
        i += 1;
    };
    if chars.get(close + 1) != Some(&'(') {
        return None;
    }

    let mut i = close + 2;
    let skip_space = |mut i: usize| {
        while chars.get(i).is_some_and(|c| c.is_whitespace()) {
            i += 1;
        }
        i
    };
    i = skip_space(i);

    // o destino
    let href: String;
    if chars.get(i) == Some(&'<') {
        let start = i + 1;
        let mut j = start;
        loop {
            match *chars.get(j)? {
                '\\' => j += 1,
                '\n' | '<' => return None,
                '>' => break,
                _ => {}
            }
            j += 1;
        }
        href = chars[start..j].iter().collect();
        i = j + 1;
    } else {
        let start = i;
        let mut parens = 0usize;
        while let Some(&c) = chars.get(i) {
            match c {
                '\\' => i += 1,
                '(' => parens += 1,
                ')' => {
                    if parens == 0 {
                        break;
                    }
                    parens -= 1;
                }
                c if c.is_whitespace() || c.is_control() => break,
                _ => {}
            }
            i += 1;
        }
        if parens != 0 {
            return None;
        }
        href = unescape_punctuation(&chars[start..i.min(chars.len())].iter().collect::<String>());
    }

    // o título (opcional), separado do destino por espaço
    let spaced = skip_space(i);
    if spaced > i && matches!(chars.get(spaced), Some('"' | '\'' | '(')) {
        let quote = *chars.get(spaced)?;
        let closing = if quote == '(' { ')' } else { quote };
        let mut j = spaced + 1;
        loop {
            match *chars.get(j)? {
                '\\' => j += 1,
                c if c == closing => break,
                _ => {}
            }
            j += 1;
        }
        i = skip_space(j + 1);
    } else {
        i = spaced;
    }

    if chars.get(i) != Some(&')') {
        return None;
    }
    Some(ParsedLink { text_start: open + 1, text_end: close, href, end: i + 1 })
}

/// `<esquema:resto>` ou `<email>`; devolve o `href` e o índice depois do `>`.
fn parse_autolink(chars: &[char], open: usize) -> Option<(String, usize)> {
    let close = chars[open + 1..].iter().position(|c| *c == '>' || *c == '<' || c.is_whitespace())? + open + 1;
    if chars[close] != '>' {
        return None;
    }
    let content: String = chars[open + 1..close].iter().collect();
    let is_scheme = {
        let scheme_len = content.chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')).count();
        content.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && (2..=32).contains(&scheme_len) && content.chars().nth(scheme_len) == Some(':')
    };
    if is_scheme {
        return Some((content, close + 1));
    }
    if is_email(&content) {
        return Some((format!("mailto:{content}"), close + 1));
    }
    None
}

fn is_email(text: &str) -> bool {
    let Some((local, domain)) = text.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && local.chars().all(|c| c.is_ascii_alphanumeric() || ".!#$%&'*+/=?^_`{|}~-".contains(c))
        && !domain.is_empty()
        && domain.split('.').all(|label| !label.is_empty() && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        && domain.contains('.')
        && !domain.ends_with(['-', '_'])
}

/// As extensões GFM de URL e e-mail soltos, a partir de `start`.
fn parse_gfm_url(chars: &[char], start: usize) -> Option<(String, usize)> {
    let rest: String = chars[start..].iter().take_while(|c| !c.is_whitespace() && **c != '<').collect();
    let url_prefix = ["http://", "https://", "www."].into_iter().find(|prefix| rest.to_lowercase().starts_with(prefix));
    let candidate: String = if url_prefix.is_some() {
        rest.clone()
    } else {
        // e-mail: o trecho até o primeiro caractere que não é de e-mail
        rest.chars().take_while(|c| c.is_ascii_alphanumeric() || ".!#$%&'*+/=?^_`{|}~-@".contains(*c)).collect()
    };
    // pontuação final não faz parte do link
    let mut text = candidate.as_str();
    loop {
        let Some(last) = text.chars().last() else {
            break;
        };
        let unbalanced_paren = last == ')' && text.matches(')').count() > text.matches('(').count();
        if matches!(last, '?' | '!' | '.' | ',' | ':' | ';' | '*' | '_' | '~' | '\'' | '"') || unbalanced_paren {
            text = &text[..text.len() - last.len_utf8()];
        } else {
            break;
        }
    }
    if text.is_empty() {
        return None;
    }
    let end = start + text.chars().count();
    match url_prefix {
        Some("www.") if text.len() > 4 && text[4..].contains('.') => Some((format!("http://{text}"), end)),
        Some("www.") => None,
        Some(prefix) if text.len() > prefix.len() => Some((text.to_owned(), end)),
        Some(_) => None,
        None if is_email(text) => Some((format!("mailto:{text}"), end)),
        None => None,
    }
}
