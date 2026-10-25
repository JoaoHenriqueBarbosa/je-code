//! Porte de `fillInIncompleteTokens` de `markdownRenderer.ts`: completa o Markdown que ainda está chegando
//! (resposta em fluxo) para que o último trecho já saia com a forma que terá quando terminar: `**negrito` fecha o
//! negrito, `[texto](https://` fecha o link, uma tabela sem a linha separadora ganha uma, e assim por diante.
//! Cada padrão é o do original, na mesma ordem, sobre o mesmo modelo de tokens do `marked`.

use crate::JsString;
use crate::js_string;
use crate::marked::{Blockquote, Lexer, List, Options, Token, TokensList, lexer};

use super::re::{exec, match_all, push_str, test, units};

const MAX_INCOMPLETE_TOKENS_FIX_ROUNDS: usize = 3;

fn merge_raw_token_text(tokens: &[Token]) -> JsString {
    let mut merged = JsString::new();
    for token in tokens {
        merged.push_units(token.raw().units());
    }
    merged
}

/// `marked.lexer(text)[0]`.
fn first_token(text: &JsString) -> Option<Token> {
    lexer(text.units()).ok().and_then(|list| list.tokens.into_iter().next())
}

fn lex_list(text: &JsString) -> Option<TokensList> {
    lexer(text.units()).ok()
}

/// `completeWithString`: o texto do token mais o fecho, lido de novo.
fn complete_with_string(token: &Token, closing: &str, should_trim: bool) -> Option<Token> {
    let merged = token.raw().clone();
    let mut text = if should_trim { JsString::from(js_string::trim_end(merged.units())) } else { merged };
    push_str(&mut text, closing);
    first_token(&text)
}

fn last_line(raw: &[u16]) -> &[u16] {
    raw.rsplit(|&unit| unit == 10).next().unwrap_or(raw)
}

fn contains_units(haystack: &[u16], needle: &str) -> bool {
    let needle = units(needle);
    haystack.windows(needle.len()).any(|window| window == needle.as_slice())
}

fn has_link_text_and_start_of_link_target(text: &[u16]) -> bool {
    // Links depois de parêntese de abertura e de marcas de ênfase, como `**[text](htt`.
    test(r"(?:^|[\s(*_~])\[.*\]\(\w*", "", text)
}

fn has_start_of_link_target_and_no_link_text(text: &[u16]) -> bool {
    test(r"^[^\[]*\]\([^\)]*$", "", text)
}

fn complete_single_line_pattern(token: &Token) -> Option<Token> {
    let sub_tokens = token.tokens()?;
    for i in (0..sub_tokens.len()).rev() {
        let Token::Text(sub_token) = &sub_tokens[i] else {
            continue;
        };
        let line = last_line(sub_token.raw.units());

        // Um alvo de link incompleto se completa antes da ênfase e do codespan: o link é a construção aberta mais
        // interna, então qualquer marca de ênfase (o `**` de `**[text](htt`) é de um trecho que o envolve.
        let preceding_has_unbalanced_bracket =
            || sub_tokens[..i].iter().any(|t| matches!(t, Token::Text(text) if test(r"\[[^\]]*$", "", text.raw.units())));
        if has_link_text_and_start_of_link_target(line) || (has_start_of_link_target_and_no_link_text(line) && preceding_has_unbalanced_bracket()) {
            let following = &sub_tokens[i + 1..];
            // `[texto](https://microsoft.com "mais texto")`: o título do link ou o argumento de um comando.
            let title_open = matches!((following.first(), following.get(1)), (Some(Token::Link(_)), Some(Token::Text(text))) if test(r#"^ *"[^"]*$"#, "", text.raw.units()));
            if title_open || test(r#"^[^"]* +"[^"]*$"#, "", line) {
                return complete_with_string(token, "\")", false);
            }
            return complete_with_string(token, ")", false);
        } else if line.contains(&u16::from(b'`')) {
            return complete_with_string(token, "`", true);
        } else if contains_units(line, "**") {
            return complete_with_string(token, "**", true);
        } else if test(r"\*\w", "", line) {
            return complete_with_string(token, "*", true);
        } else if test(r"(^|\s)__\w", "", line) {
            return complete_with_string(token, "__", true);
        } else if test(r"(^|\s)_\w", "", line) {
            return complete_with_string(token, "_", true);
        } else if test(r"(^|\s)\[\w*[^\]]*$", "", line) {
            // Tem o começo do texto de um link e nenhum token seguinte tem o alvo.
            return complete_with_string(token, "](https://microsoft.com)", false);
        }
    }
    None
}

fn complete_blockquote_pattern(blockquote: &Blockquote, links: &std::collections::HashMap<JsString, crate::marked::LinkDefinition>) -> Option<Blockquote> {
    let last_interesting = blockquote.tokens.iter().rposition(|token| !matches!(token, Token::Space(_)))?;
    let Token::Paragraph(_) = &blockquote.tokens[last_interesting] else {
        return None;
    };
    let last_token = &blockquote.tokens[last_interesting];
    let completed = complete_single_line_pattern(last_token)?;

    let completion = JsString::from(&completed.raw().units()[js_string::trim_end(last_token.raw().units()).len()..]);
    let trailing_quote_only_lines = exec(r"(?:\n[ \t]*>[ \t]*(?=\n|$))+\n?$", "", blockquote.raw.units()).map_or(&[][..], |found| &blockquote.raw.units()[found.start()..found.end()]);
    let insertion_index = blockquote.raw.len() - trailing_quote_only_lines.len();
    let mut completed_raw = JsString::from(&blockquote.raw.units()[..insertion_index]);
    completed_raw.push_units(completion.units());
    completed_raw.push_units(trailing_quote_only_lines);

    let tokens = Lexer::new(Options::default()).with_links(links.clone()).lex(completed_raw.units()).ok()?;
    match tokens.tokens.into_iter().next()? {
        Token::Blockquote(completed_blockquote) => Some(completed_blockquote),
        _ => None,
    }
}

/// `listEndsInHeading`: um item vira título quando tem um subitem de que ainda não chegou o texto (`1. item` e
/// depois `   -`).
fn list_ends_in_heading(list: &List) -> bool {
    let Some(Token::ListItem(last_item)) = list.items.last() else {
        return false;
    };
    match last_item.tokens.last() {
        Some(Token::Heading(_)) => true,
        Some(Token::List(nested)) => list_ends_in_heading(nested),
        _ => false,
    }
}

fn complete_list_item_pattern(list: &List) -> Option<List> {
    let Some(Token::ListItem(last_list_item)) = list.items.last() else {
        return None;
    };
    let new_token = match last_list_item.tokens.last() {
        Some(sub_token @ Token::Text(_)) => complete_single_line_pattern(sub_token),
        _ if list_ends_in_heading(list) => {
            let mut text = JsString::from(js_string::trim(list.raw.units()));
            push_str(&mut text, " &nbsp;");
            return match first_token(&text)? {
                Token::List(new_list) => Some(new_list),
                _ => None,
            };
        }
        _ => None,
    };

    // O item de texto dentro do item de lista vira parágrafo.
    let Some(Token::Paragraph(new_paragraph)) = &new_token else {
        return None;
    };
    let previous_items_text = merge_raw_token_text(&list.items[..list.items.len() - 1]);

    // Tira o `- `, o `1. ` ou o `* ` do item, porque não há jeito melhor de reaproveitá-lo.
    let lead = exec(r"^(\s*(-|\d+\.|\*) +)", "", last_list_item.raw.units()).map(|found| JsString::from(&last_list_item.raw.units()[found.start()..found.end()]))?;

    let mut new_list_item_text = lead;
    new_list_item_text.push_units(merge_raw_token_text(&last_list_item.tokens[..last_list_item.tokens.len() - 1]).units());
    new_list_item_text.push_units(new_paragraph.raw.units());

    let mut text = previous_items_text;
    text.push_units(new_list_item_text.units());
    match first_token(&text)? {
        Token::List(new_list) => Some(new_list),
        _ => None,
    }
}

fn complete_heading(token: &Token, full_raw_text: JsString) -> Option<TokensList> {
    if test(r"-\s*$", "", token.raw().units()) {
        let mut text = full_raw_text;
        push_str(&mut text, " &nbsp;");
        return lex_list(&text);
    }
    None
}

fn complete_table(tokens: &[Token]) -> Option<Vec<Token>> {
    let merged = merge_raw_token_text(tokens);
    let lines: Vec<&[u16]> = merged.units().split(|&unit| unit == 10).collect();

    let mut num_cols: Option<usize> = None; // As colunas do cabeçalho da linha 1.
    let mut has_separator_row = false;
    for (i, raw_line) in lines.iter().enumerate() {
        let line = js_string::trim(raw_line);
        if num_cols.is_none() && test(r"^\s*\|", "", line) {
            let columns = match_all(r"(\|[^\|]+)(?=\||$)", "g", line);
            if !columns.is_empty() {
                num_cols = Some(columns.len());
            }
        } else if num_cols.is_some() {
            if test(r"^\s*\|", "", line) {
                if i != lines.len() - 1 {
                    // O cabeçalho e o separador vieram, há mais linhas e não virou tabela: a tabela está malformada
                    // na origem, então não se tenta consertar.
                    return None;
                }
                // Linha separadora parcial ou completa: é trocada por uma correta.
                has_separator_row = true;
            } else {
                // A linha depois do cabeçalho não é um separador válido: tabela malformada.
                return None;
            }
        }
    }

    let num_cols = num_cols.filter(|&n| n > 0)?;
    let prefix = if has_separator_row {
        let mut joined = JsString::new();
        for (i, line) in lines[..lines.len() - 1].iter().enumerate() {
            if i > 0 {
                joined.push_unit(10);
            }
            joined.push_units(line);
        }
        joined
    } else {
        merged.clone()
    };
    let line1_ends_in_pipe = test(r"\|\s*$", "", prefix.units());
    let mut new_raw_text = prefix;
    push_str(&mut new_raw_text, if line1_ends_in_pipe { "" } else { "|" });
    push_str(&mut new_raw_text, "\n|");
    push_str(&mut new_raw_text, &" --- |".repeat(num_cols));
    lex_list(&new_raw_text).map(|list| list.tokens)
}

fn fill_in_incomplete_tokens_once(tokens: &TokensList) -> Option<TokensList> {
    let list = &tokens.tokens;
    let mut replace_from = 0;
    let mut new_tokens: Option<Vec<Token>> = None;
    for (i, token) in list.iter().enumerate() {
        if matches!(token, Token::Paragraph(_)) && test(r"(\n|^)\|", "", token.raw().units()) {
            new_tokens = complete_table(&list[i..]);
            replace_from = i;
            break;
        }
    }

    // O último token "interessante" pula `space` e `html` do fim: quem embrulha o Markdown em `<body>...</body>` (para
    // o DOMPurify manter comentários do começo) deixa `</body>` como último token, e sem o pulo os conserto de
    // parágrafo e de lista nunca rodariam.
    let last_interesting_idx = list.iter().rposition(|token| !matches!(token, Token::Space(_) | Token::Html(_)));
    let trailing_tokens = last_interesting_idx.map_or(&list[..], |i| &list[i + 1..]);

    if let Some(idx) = last_interesting_idx {
        if new_tokens.is_none() {
            let replacement = match &list[idx] {
                Token::List(last_list) => complete_list_item_pattern(last_list).map(Token::List),
                Token::Blockquote(quote) => complete_blockquote_pattern(quote, &tokens.links).map(Token::Blockquote),
                Token::Paragraph(_) => complete_single_line_pattern(&list[idx]),
                _ => None,
            };
            if let Some(token) = replacement {
                let mut completed = vec![token];
                completed.extend(trailing_tokens.iter().cloned());
                new_tokens = Some(completed);
                replace_from = idx;
            }
        }
    }

    if let Some(completed) = new_tokens {
        let mut all: Vec<Token> = list[..replace_from].to_vec();
        all.extend(completed);
        return Some(TokensList { tokens: all, links: tokens.links.clone() });
    }

    if let Some(last_token @ Token::Heading(_)) = list.last() {
        return complete_heading(last_token, merge_raw_token_text(list));
    }
    None
}

/// `fillInIncompleteTokens`: até três rodadas de conserto, parando na primeira que não muda nada.
pub fn fill_in_incomplete_tokens(mut tokens: TokensList) -> TokensList {
    for _ in 0..MAX_INCOMPLETE_TOKENS_FIX_ROUNDS {
        match fill_in_incomplete_tokens_once(&tokens) {
            Some(new_tokens) => tokens = new_tokens,
            None => break,
        }
    }
    tokens
}
