//! O `_Tokenizer` do `marked`: um método por regra de bloco e de linha, na ordem e com a lógica do
//! original. Cada método recebe o texto que falta lexar (`src`) e devolve o token ou `None` (o
//! `undefined` do JavaScript).

use std::rc::Rc;

use super::helpers::{
    self, ch, code_point_len, code_point_unit_len, collapse_whitespace, concat, escape, find_closing_bracket,
    first_line, indent_code_compensation, join_lines, only_spaces, rtrim, search_non_space, slice, slice_from,
    split_cells, split_lines, tail,
};
use super::lexer::Lexer;
use super::re::{Re, cached};
use super::{
    Align, Blockquote, Br, Code, Codespan, Def, Del, Em, Escape, Heading, Hr, Html, Image, Link, List, ListItem,
    MarkedError, Paragraph, Space, Strong, Table, TableCell, Text, Token,
};
use crate::JsString;
use crate::js_string::{trim, trim_end, trim_start};

thread_local! {
    static INDENT_1_4: Re = Re::new(r"^ {1,4}", "gm");
    static SETEXT_CONTINUATION: Re = Re::new(r"\n {0,3}((?:=+|-+) *)(?=\n|$)", "g");
    static QUOTE_PREFIX: Re = Re::new(r"^ {0,3}>[ \t]?", "gm");
    static ENDS_WITH_BLANK_LINE: Re = Re::new(r"\n *\n *$", "");
    static MULTIPLE_LINE_BREAKS: Re = Re::new(r"\n.*\n", "");
    static PEDANTIC_REALIGN: Re = Re::new(r"^ {1,4}(?=( {4})*[^ ])", "g");
    static ANGLE_HREF: Re = Re::new(r"^<(.*)>$", "");
    static TABLE_EDGES: Re = Re::new(r"^\||\| *$", "g");
    static TABLE_TRAILING: Re = Re::new(r"\n[ \t]*$", "");
    static PIPE_OR_COLON: Re = Re::new(r"[:|]", "");
    static PEDANTIC_LINK: Re = Re::new(r#"^([^'"]*[^\s])\s+(['"])(.*)\2"#, "");
    static ESCAPED_BRACKETS: Re = Re::new(r"\\([\[\]])", "g");
    static OPEN_ANCHOR: Re = Re::new(r"^<a ", "i");
    static CLOSE_ANCHOR: Re = Re::new(r"^<\/a>", "i");
    static OPEN_RAW_BLOCK: Re = Re::new(r"^<(pre|code|kbd|script)(\s|>)", "i");
    static CLOSE_RAW_BLOCK: Re = Re::new(r"^<\/(pre|code|kbd|script)(\s|>)", "i");
    static LETTER_OR_NUMBER: Re = Re::new(r"[\p{L}\p{N}]", "u");
}

fn text_token(raw: JsString, text: JsString) -> Token {
    Token::Text(Text {
        raw,
        text,
        tokens: None,
        slot: None,
    })
}

/// `str.replace(anyPunctuation, '$1')`.
fn unescape_punctuation(any_punctuation: &Re, units: &[u16]) -> JsString {
    any_punctuation.replace_all(units, |c| JsString::from(c.g(1).unwrap_or_default()))
}

fn has_prefix(units: &[u16], prefix: char) -> bool {
    units.first() == Some(&ch(prefix))
}

fn has_suffix(units: &[u16], suffix: char) -> bool {
    units.last() == Some(&ch(suffix))
}

/// `/^ {0,3}>/.test(line)`.
fn starts_quote(line: &[u16]) -> bool {
    let spaces = line.iter().take_while(|&&c| c == 0x20).count();
    spaces <= 3 && line.get(spaces) == Some(&ch('>'))
}

impl Lexer {
    pub(super) fn space(&mut self, src: &[u16]) -> Option<Token> {
        let cap = self.rules.block.newline.exec(src)?;
        (!cap.whole().is_empty()).then(|| {
            Token::Space(Space {
                raw: JsString::from(cap.whole()),
            })
        })
    }

    pub(super) fn code(&mut self, src: &[u16]) -> Option<Token> {
        let cap = self.rules.block.code.exec(src)?;
        let text = INDENT_1_4.with(|re| re.remove_all(cap.whole()));
        let text = if self.options.pedantic {
            text
        } else {
            JsString::from(rtrim(text.units(), 0x0A, false))
        };
        Some(Token::Code(Code {
            raw: JsString::from(cap.whole()),
            indented: true,
            lang: None,
            text,
        }))
    }

    pub(super) fn fences(&mut self, src: &[u16]) -> Option<Token> {
        let cap = self.rules.block.fences.as_ref()?.exec(src)?;
        let raw = cap.whole();
        let text = indent_code_compensation(raw, cap.g(3).unwrap_or_default());
        let lang = cap.g(2).unwrap_or_default();
        let lang = if lang.is_empty() {
            JsString::new()
        } else {
            unescape_punctuation(&self.rules.inline.any_punctuation, trim(lang))
        };
        Some(Token::Code(Code {
            raw: JsString::from(raw),
            indented: false,
            lang: Some(lang),
            text,
        }))
    }

    pub(super) fn heading(&mut self, src: &[u16]) -> Option<Token> {
        let cap = self.rules.block.heading.exec(src)?;
        let mut text = trim(cap.g(2).unwrap_or_default());
        // Tira os `#` finais.
        if has_suffix(text, '#') {
            let trimmed = rtrim(text, ch('#'), false);
            if self.options.pedantic {
                text = trim(trimmed);
            } else if trimmed.is_empty() || has_suffix(trimmed, ' ') {
                // O CommonMark exige um espaço antes dos `#` finais.
                text = trim(trimmed);
            }
        }
        let text = JsString::from(text);
        let slot = self.inline(text.clone());
        Some(Token::Heading(Heading {
            raw: JsString::from(cap.whole()),
            depth: cap.g(1).unwrap_or_default().len() as u8,
            text,
            tokens: Vec::new(),
            slot: Some(slot),
        }))
    }

    pub(super) fn hr(&mut self, src: &[u16]) -> Option<Token> {
        let cap = self.rules.block.hr.exec(src)?;
        Some(Token::Hr(Hr {
            raw: JsString::from(rtrim(cap.whole(), 0x0A, false)),
        }))
    }

    pub(super) fn blockquote(&mut self, src: &[u16]) -> Result<Option<Token>, MarkedError> {
        let rules = Rc::clone(&self.rules);
        let Some(cap) = rules.block.blockquote.exec(src) else {
            return Ok(None);
        };
        let mut lines: Vec<Vec<u16>> = split_lines(rtrim(cap.whole(), 0x0A, false))
            .into_iter()
            .map(<[u16]>::to_vec)
            .collect();
        let mut raw = JsString::new();
        let mut text = JsString::new();
        let mut tokens: Vec<Token> = Vec::new();
        while !lines.is_empty() {
            let mut in_blockquote = false;
            let mut current_lines: Vec<&[u16]> = Vec::new();
            let mut i = 0;
            while i < lines.len() {
                // As linhas até uma continuação.
                if starts_quote(&lines[i]) {
                    current_lines.push(&lines[i]);
                    in_blockquote = true;
                } else if !in_blockquote {
                    current_lines.push(&lines[i]);
                } else {
                    break;
                }
                i += 1;
            }
            let current_raw = JsString::from_units(join_lines(&current_lines));
            lines.drain(..i);
            // Antes de uma continuação de setext, 4 espaços para não virar setext.
            let precede = SETEXT_CONTINUATION.with(|re| {
                re.replace_all(current_raw.units(), |c| {
                    concat(&[&helpers::u("\n    "), c.g(1).unwrap_or_default()])
                })
            });
            let current_text = QUOTE_PREFIX.with(|re| re.remove_all(precede.units()));
            raw = if raw.is_empty() {
                current_raw
            } else {
                concat(&[raw.units(), &[0x0A], current_raw.units()])
            };
            text = if text.is_empty() {
                current_text.clone()
            } else {
                concat(&[text.units(), &[0x0A], current_text.units()])
            };
            // O texto da citação é lexado como tokens de topo, mesclando o parágrafo se for continuação.
            let top = self.state.top;
            self.state.top = true;
            self.block_tokens(current_text.units(), &mut tokens, true)?;
            self.state.top = top;
            // Sem continuação, acabou.
            if lines.is_empty() {
                break;
            }
            match tokens.last() {
                Some(Token::Code(_)) => break, // uma continuação não segue um bloco de código
                Some(Token::Blockquote(old)) => {
                    // A continuação entra na citação aninhada.
                    let new_text = concat(&[
                        old.raw.units(),
                        &[0x0A],
                        &join_lines(&lines.iter().map(Vec::as_slice).collect::<Vec<_>>()),
                    ]);
                    let (old_raw, old_text) = (old.raw.clone(), old.text.clone());
                    let Some(new_token) = self.blockquote(new_text.units())? else {
                        return Err(MarkedError::TypeError(
                            "Cannot read properties of undefined (reading 'raw')",
                        ));
                    };
                    let Token::Blockquote(new_bq) = &new_token else {
                        unreachable!("blockquote devolve citação")
                    };
                    raw = concat(&[
                        helpers::head(raw.units(), raw.len().saturating_sub(old_raw.len())),
                        new_bq.raw.units(),
                    ]);
                    text = concat(&[
                        helpers::head(text.units(), text.len().saturating_sub(old_text.len())),
                        new_bq.text.units(),
                    ]);
                    *tokens.last_mut().expect("checado") = new_token;
                    break;
                }
                Some(Token::List(old)) => {
                    // A continuação entra na lista aninhada.
                    let old_raw = old.raw.clone();
                    let new_text = concat(&[
                        old_raw.units(),
                        &[0x0A],
                        &join_lines(&lines.iter().map(Vec::as_slice).collect::<Vec<_>>()),
                    ]);
                    let Some(new_token) = self.list(new_text.units())? else {
                        return Err(MarkedError::TypeError(
                            "Cannot read properties of undefined (reading 'raw')",
                        ));
                    };
                    let new_raw = new_token.raw().clone();
                    raw = concat(&[
                        helpers::head(raw.units(), raw.len().saturating_sub(old_raw.len())),
                        new_raw.units(),
                    ]);
                    text = concat(&[
                        helpers::head(text.units(), text.len().saturating_sub(old_raw.len())),
                        new_raw.units(),
                    ]);
                    *tokens.last_mut().expect("checado") = new_token;
                    let rest = tail(new_text.units(), new_raw.len());
                    lines = split_lines(rest).into_iter().map(<[u16]>::to_vec).collect();
                }
                _ => {}
            }
        }
        Ok(Some(Token::Blockquote(Blockquote { raw, tokens, text })))
    }

    pub(super) fn list(&mut self, src_in: &[u16]) -> Result<Option<Token>, MarkedError> {
        let rules = Rc::clone(&self.rules);
        let Some(cap) = rules.block.list.exec(src_in) else {
            return Ok(None);
        };
        let bull = trim(cap.g(1).unwrap_or_default());
        let ordered = bull.len() > 1;
        let start = ordered.then(|| {
            bull[..bull.len() - 1]
                .iter()
                .fold(0u32, |n, &d| n * 10 + u32::from(d - 0x30))
        });
        let bull_source = if ordered {
            format!("\\d{{1,9}}\\{}", char::from(bull[bull.len() - 1] as u8))
        } else if self.options.pedantic {
            "[*+-]".to_owned()
        } else {
            format!("\\{}", char::from(bull[0] as u8))
        };

        // O próximo item da lista.
        let item_regex = cached(format!("^( {{0,3}}{bull_source})((?:[\t ][^\\n]*)?(?:\\n|$))"), "");
        let mut list_raw = JsString::new();
        let mut loose = false;
        let mut items: Vec<ListItem> = Vec::new();
        let mut ends_with_blank_line = false;
        let mut src = src_in;
        // O marcador pode abrir um item novo?
        while !src.is_empty() {
            let mut end_early = false;
            let Some(item_cap) = item_regex.exec(src) else { break };
            if rules.block.hr.test(src) {
                // Terminou a lista: o marcador era uma linha horizontal.
                break;
            }
            let mut raw = JsString::from(item_cap.whole());
            let mut item_contents = JsString::new();
            src = tail(src, raw.len());
            let bullet = item_cap.g(1).unwrap_or_default();
            let after_bullet = item_cap.g(2).unwrap_or_default();
            let mut line = expand_leading_tabs(first_line(after_bullet));
            let mut next_line = first_line(src).to_vec();
            let mut blank_line = trim(&line).is_empty();
            let mut indent: i64;
            if self.options.pedantic {
                indent = 2;
                item_contents = JsString::from(trim_start(&line));
            } else if blank_line {
                indent = bullet.len() as i64 + 1;
            } else {
                indent = search_non_space(after_bullet);
                indent = if indent > 4 { 1 } else { indent };
                item_contents = JsString::from(slice_from(&line, indent));
                indent += bullet.len() as i64;
            }
            if blank_line && only_spaces(&next_line) {
                // Os itens começam com no máximo uma linha em branco.
                raw.push_units(&next_line);
                raw.push_unit(0x0A);
                src = tail(src, next_line.len() + 1);
                end_early = true;
            }
            if !end_early {
                let n = (indent - 1).clamp(0, 3);
                let next_bullet = cached(
                    format!("^ {{0,{n}}}(?:[*+-]|\\d{{1,9}}[.)])((?:[ \t][^\\n]*)?(?:\\n|$))"),
                    "",
                );
                let hr_regex = cached(
                    format!("^ {{0,{n}}}((?:- *){{3,}}|(?:_ *){{3,}}|(?:\\* *){{3,}})(?:\\n+|$)"),
                    "",
                );
                let fences_begin = cached(format!("^ {{0,{n}}}(?:```|~~~)"), "");
                let heading_begin = cached(format!("^ {{0,{n}}}#"), "");
                // As linhas seguintes entram no item?
                while !src.is_empty() {
                    let raw_line = first_line(src);
                    next_line = raw_line.to_vec();
                    // Realinha segundo as regras de aninhamento do CommonMark.
                    if self.options.pedantic {
                        next_line = PEDANTIC_REALIGN
                            .with(|re| re.replace_all(&next_line, |_| JsString::from("  ")))
                            .units()
                            .to_vec();
                    }
                    if fences_begin.test(&next_line) {
                        break;
                    }
                    if heading_begin.test(&next_line) {
                        break;
                    }
                    if next_bullet.test(&next_line) {
                        break;
                    }
                    if hr_regex.test(src) {
                        break;
                    }
                    if search_non_space(&next_line) >= indent || trim(&next_line).is_empty() {
                        // Tira o recuo, se der.
                        item_contents = concat(&[item_contents.units(), &[0x0A], slice_from(&next_line, indent)]);
                    } else {
                        // Recuo insuficiente.
                        if blank_line {
                            break;
                        }
                        // Continuação do parágrafo, a não ser que a última linha fosse de outro bloco.
                        if search_non_space(&line) >= 4 {
                            break;
                        }
                        if fences_begin.test(&line) {
                            break;
                        }
                        if heading_begin.test(&line) {
                            break;
                        }
                        if hr_regex.test(&line) {
                            break;
                        }
                        item_contents = concat(&[item_contents.units(), &[0x0A], &next_line]);
                    }
                    if !blank_line && trim(&next_line).is_empty() {
                        // A linha atual está em branco.
                        blank_line = true;
                    }
                    raw.push_units(raw_line);
                    raw.push_unit(0x0A);
                    src = tail(src, raw_line.len() + 1);
                    line = slice_from(&next_line, indent).to_vec();
                }
            }
            if !loose {
                if ends_with_blank_line {
                    // Se o item anterior terminou em branco, a lista é frouxa.
                    loose = true;
                } else if ENDS_WITH_BLANK_LINE.with(|re| re.test(raw.units())) {
                    ends_with_blank_line = true;
                }
            }
            let mut task = false;
            let mut checked = None;
            // Item de lista de tarefas?
            if self.options.gfm {
                let c = item_contents.units();
                if c.len() >= 4
                    && c[0] == ch('[')
                    && matches!(c[1], 0x20 | 0x78 | 0x58)
                    && c[2] == ch(']')
                    && c[3] == 0x20
                {
                    task = true;
                    checked = Some(c[1] != 0x20);
                    // `replace(/^\[[ xX]\] +/, '')`
                    let spaces = c[3..].iter().take_while(|&&s| s == 0x20).count();
                    item_contents = JsString::from(&c[3 + spaces..]);
                }
            }
            list_raw.push_units(raw.units());
            items.push(ListItem {
                raw,
                task,
                checked,
                loose: false,
                text: item_contents,
                tokens: Vec::new(),
            });
        }
        // Não consome as quebras de linha do fim do último item.
        let Some(last) = items.last_mut() else {
            return Err(MarkedError::TypeError(
                "Cannot read properties of undefined (reading 'raw')",
            ));
        };
        last.raw = JsString::from(trim_end(last.raw.units()));
        last.text = JsString::from(trim_end(last.text.units()));
        let list_raw = JsString::from(trim_end(list_raw.units()));

        // Os tokens dos itens só depois, porque o último item precisava ser aparado antes.
        for item in &mut items {
            self.state.top = false;
            let mut tokens = Vec::new();
            self.block_tokens(item.text.units(), &mut tokens, false)?;
            item.tokens = tokens;
            if !loose {
                // A lista é frouxa?
                loose = item
                    .tokens
                    .iter()
                    .any(|t| matches!(t, Token::Space(s) if MULTIPLE_LINE_BREAKS.with(|re| re.test(s.raw.units()))));
            }
        }
        // Se a lista é frouxa, todos os itens são.
        if loose {
            for item in &mut items {
                item.loose = true;
            }
        }
        Ok(Some(Token::List(List {
            raw: list_raw,
            ordered,
            start,
            loose,
            items: items.into_iter().map(Token::ListItem).collect(),
        })))
    }

    pub(super) fn html(&mut self, src: &[u16]) -> Option<Token> {
        let cap = self.rules.block.html.exec(src)?;
        let pre = cap.g(1).is_some_and(|tag| {
            ["pre", "script", "style"]
                .iter()
                .any(|name| tag.iter().copied().eq(name.encode_utf16()))
        });
        Some(Token::Html(Html {
            raw: JsString::from(cap.whole()),
            text: JsString::from(cap.whole()),
            block: true,
            pre: Some(pre),
            in_link: None,
            in_raw_block: None,
        }))
    }

    pub(super) fn def(&mut self, src: &[u16]) -> Option<Token> {
        let cap = self.rules.block.def.exec(src)?;
        let tag = collapse_whitespace(JsString::from(cap.g(1).unwrap_or_default()).to_lower_case().units());
        let any_punctuation = &self.rules.inline.any_punctuation;
        let href = match cap.g(2).filter(|g| !g.is_empty()) {
            Some(href) => {
                let unwrapped = ANGLE_HREF
                    .with(|re| re.exec(href).map(|c| JsString::from(c.g(1).unwrap_or_default())))
                    .unwrap_or_else(|| JsString::from(href));
                unescape_punctuation(any_punctuation, unwrapped.units())
            }
            None => JsString::new(),
        };
        let title = cap
            .g(3)
            .filter(|g| !g.is_empty())
            .map(|title| unescape_punctuation(any_punctuation, slice(title, 1, title.len() as i64 - 1)));
        Some(Token::Def(Def {
            raw: JsString::from(cap.whole()),
            tag,
            href,
            title,
        }))
    }

    pub(super) fn table(&mut self, src: &[u16]) -> Option<Token> {
        let cap = self.rules.block.table.as_ref()?.exec(src)?;
        let delimiter = cap.g(2).unwrap_or_default();
        if !PIPE_OR_COLON.with(|re| re.test(delimiter)) {
            // A linha delimitadora precisa de `|` ou `:`; sem isso é um título setext.
            return None;
        }
        let headers = split_cells(cap.g(1).unwrap_or_default(), None);
        let aligns_text = TABLE_EDGES.with(|re| re.remove_all(delimiter));
        let aligns: Vec<&[u16]> = aligns_text.units().split(|&c| c == ch('|')).collect();
        let rows: Vec<Vec<u16>> = match cap.g(3).filter(|g| !trim(g).is_empty()) {
            Some(body) => {
                let body = TABLE_TRAILING.with(|re| re.remove_all(body));
                split_lines(body.units()).into_iter().map(<[u16]>::to_vec).collect()
            }
            None => Vec::new(),
        };
        if headers.len() != aligns.len() {
            // Cabeçalho e alinhamento precisam ter o mesmo número de colunas; as linhas podem variar.
            return None;
        }
        let align: Vec<Option<Align>> = aligns.iter().map(|a| align_of(a)).collect();
        let header: Vec<TableCell> = headers
            .into_iter()
            .enumerate()
            .map(|(i, text)| {
                let slot = self.inline(text.clone());
                TableCell {
                    text,
                    tokens: Vec::new(),
                    header: true,
                    align: align[i],
                    slot: Some(slot),
                }
            })
            .collect();
        let width = header.len();
        let rows: Vec<Vec<TableCell>> = rows
            .iter()
            .map(|row| {
                split_cells(row, Some(width))
                    .into_iter()
                    .enumerate()
                    .map(|(i, text)| {
                        let slot = self.inline(text.clone());
                        TableCell {
                            text,
                            tokens: Vec::new(),
                            header: false,
                            align: align.get(i).copied().flatten(),
                            slot: Some(slot),
                        }
                    })
                    .collect()
            })
            .collect();
        Some(Token::Table(Table {
            raw: JsString::from(cap.whole()),
            align,
            header,
            rows,
        }))
    }

    pub(super) fn lheading(&mut self, src: &[u16]) -> Option<Token> {
        let cap = self.rules.block.lheading.exec(src)?;
        let text = JsString::from(cap.g(1).unwrap_or_default());
        let slot = self.inline(text.clone());
        let depth = if cap.g(2).unwrap_or_default().first() == Some(&ch('=')) {
            1
        } else {
            2
        };
        Some(Token::Heading(Heading {
            raw: JsString::from(cap.whole()),
            depth,
            text,
            tokens: Vec::new(),
            slot: Some(slot),
        }))
    }

    pub(super) fn paragraph(&mut self, src: &[u16]) -> Option<Token> {
        let cap = self.rules.block.paragraph.exec(src)?;
        let body = cap.g(1).unwrap_or_default();
        let text = JsString::from(if has_suffix(body, '\n') {
            &body[..body.len() - 1]
        } else {
            body
        });
        let slot = self.inline(text.clone());
        Some(Token::Paragraph(Paragraph {
            raw: JsString::from(cap.whole()),
            text,
            tokens: Vec::new(),
            slot: Some(slot),
        }))
    }

    pub(super) fn text(&mut self, src: &[u16]) -> Option<Token> {
        let cap = self.rules.block.text.exec(src)?;
        let whole = JsString::from(cap.whole());
        let slot = self.inline(whole.clone());
        Some(Token::Text(Text {
            raw: whole.clone(),
            text: whole,
            tokens: Some(Vec::new()),
            slot: Some(slot),
        }))
    }

    pub(super) fn escape(&mut self, src: &[u16]) -> Option<Token> {
        let cap = self.rules.inline.escape.exec(src)?;
        Some(Token::Escape(Escape {
            raw: JsString::from(cap.whole()),
            text: escape(cap.g(1).unwrap_or_default(), false),
        }))
    }

    pub(super) fn tag(&mut self, src: &[u16]) -> Option<Token> {
        let cap = self.rules.inline.tag.exec(src)?;
        let whole = cap.whole();
        if !self.state.in_link && OPEN_ANCHOR.with(|re| re.test(whole)) {
            self.state.in_link = true;
        } else if self.state.in_link && CLOSE_ANCHOR.with(|re| re.test(whole)) {
            self.state.in_link = false;
        }
        if !self.state.in_raw_block && OPEN_RAW_BLOCK.with(|re| re.test(whole)) {
            self.state.in_raw_block = true;
        } else if self.state.in_raw_block && CLOSE_RAW_BLOCK.with(|re| re.test(whole)) {
            self.state.in_raw_block = false;
        }
        Some(Token::Html(Html {
            raw: JsString::from(whole),
            text: JsString::from(whole),
            block: false,
            pre: None,
            in_link: Some(self.state.in_link),
            in_raw_block: Some(self.state.in_raw_block),
        }))
    }

    /// `outputLink(cap, link, raw, lexer)`.
    fn output_link(
        &mut self,
        label: &[u16],
        is_link: bool,
        href: JsString,
        title: Option<JsString>,
        raw: JsString,
    ) -> Result<Token, MarkedError> {
        let title = title.filter(|t| !t.is_empty()).map(|t| escape(t.units(), false));
        let text = ESCAPED_BRACKETS.with(|re| re.replace_all(label, |c| JsString::from(c.g(1).unwrap_or_default())));
        if is_link {
            self.state.in_link = true;
            let mut tokens = Vec::new();
            let result = self.inline_tokens(text.units(), &mut tokens);
            self.state.in_link = false;
            result?;
            return Ok(Token::Link(Link {
                raw,
                href,
                title,
                text,
                tokens,
            }));
        }
        Ok(Token::Image(Image {
            raw,
            href,
            title,
            text: escape(text.units(), false),
        }))
    }

    pub(super) fn link(&mut self, src: &[u16]) -> Result<Option<Token>, MarkedError> {
        let rules = Rc::clone(&self.rules);
        let Some(cap) = rules.inline.link.exec(src) else {
            return Ok(None);
        };
        let label = cap.g(1).unwrap_or_default();
        let mut cap0: &[u16] = cap.whole();
        let mut cap2: &[u16] = cap.g(2).unwrap_or_default();
        let mut cap3: Option<&[u16]> = cap.g(3);
        let trimmed_url = trim(cap2);
        if !self.options.pedantic && has_prefix(trimmed_url, '<') {
            // O CommonMark exige colchetes angulares casados.
            if !has_suffix(trimmed_url, '>') {
                return Ok(None);
            }
            // O colchete final não pode estar escapado.
            let rtrim_slash = rtrim(&trimmed_url[..trimmed_url.len() - 1], 0x5C, false);
            if (trimmed_url.len() - rtrim_slash.len()).is_multiple_of(2) {
                return Ok(None);
            }
        } else {
            // Acha o parêntese de fechamento.
            let last_paren = find_closing_bracket(cap2, ch('('), ch(')'));
            if last_paren > -1 {
                let start = if has_prefix(cap0, '!') { 5 } else { 4 };
                let link_len = start + label.len() as i64 + last_paren;
                cap2 = &cap2[..last_paren as usize];
                cap0 = trim(slice(cap0, 0, link_len));
                cap3 = Some(&[]);
            }
        }
        let mut href: Vec<u16> = cap2.to_vec();
        let mut title: Vec<u16> = Vec::new();
        if self.options.pedantic {
            // Separa o href e o título no modo pedantic.
            let split = PEDANTIC_LINK.with(|re| {
                re.exec(&href)
                    .map(|c| (c.g(1).unwrap_or_default().to_vec(), c.g(3).unwrap_or_default().to_vec()))
            });
            if let Some((h, t)) = split {
                href = h;
                title = t;
            }
        } else if let Some(t) = cap3.filter(|t| !t.is_empty()) {
            title = slice(t, 1, -1).to_vec();
        }
        let mut href: &[u16] = trim(&href);
        if has_prefix(href, '<') {
            href = if self.options.pedantic && !has_suffix(trimmed_url, '>') {
                // O pedantic aceita `<` sem o `>` final.
                slice_from(href, 1)
            } else {
                slice(href, 1, -1)
            };
        }
        let any_punctuation = &rules.inline.any_punctuation;
        let href = if href.is_empty() {
            JsString::new()
        } else {
            unescape_punctuation(any_punctuation, href)
        };
        let title = if title.is_empty() {
            JsString::new()
        } else {
            unescape_punctuation(any_punctuation, &title)
        };
        let is_link = !has_prefix(cap0, '!');
        self.output_link(label, is_link, href, Some(title), JsString::from(cap0))
            .map(Some)
    }

    pub(super) fn reflink(&mut self, src: &[u16]) -> Result<Option<Token>, MarkedError> {
        let rules = Rc::clone(&self.rules);
        let Some(cap) = rules.inline.reflink.exec(src).or_else(|| rules.inline.nolink.exec(src)) else {
            return Ok(None);
        };
        let label = cap.g(2).filter(|g| !g.is_empty()).or(cap.g(1)).unwrap_or_default();
        let key = collapse_whitespace(label).to_lower_case();
        let whole = cap.whole();
        let Some(definition) = self.links.get(key.units()).cloned() else {
            let first = JsString::from(&whole[..1]);
            return Ok(Some(text_token(first.clone(), first)));
        };
        let is_link = !has_prefix(whole, '!');
        self.output_link(
            cap.g(1).unwrap_or_default(),
            is_link,
            definition.href,
            definition.title,
            JsString::from(whole),
        )
        .map(Some)
    }

    pub(super) fn em_strong(
        &mut self,
        src: &[u16],
        masked: &[u16],
        prev_char: Option<u16>,
    ) -> Result<Option<Token>, MarkedError> {
        let rules = Rc::clone(&self.rules);
        let Some(ldelim) = rules.inline.em_strong_ldelim.exec(src) else {
            return Ok(None);
        };
        // O `_` não pode ficar entre dois alfanuméricos.
        if ldelim.g(3).is_some_and(|g| !g.is_empty())
            && prev_char.is_some_and(|p| LETTER_OR_NUMBER.with(|re| re.test(&[p])))
        {
            return Ok(None);
        }
        let next_char = ldelim
            .g(1)
            .filter(|g| !g.is_empty())
            .or(ldelim.g(2).filter(|g| !g.is_empty()));
        let allowed = next_char.is_none()
            || prev_char.is_none()
            || prev_char.is_some_and(|p| rules.inline.punctuation.test(&[p]));
        if !allowed {
            return Ok(None);
        }
        // O `[...match[0]].length` conta pontos de código.
        let l_length = code_point_len(ldelim.whole()) as i64 - 1;
        let end_reg = if ldelim.whole().first() == Some(&ch('*')) {
            &rules.inline.em_strong_rdelim_ast
        } else {
            &rules.inline.em_strong_rdelim_und
        };
        let mut delim_total = l_length;
        let mut mid_delim_total = 0i64;
        // `maskedSrc` recortado para a mesma seção que `src`.
        let clipped = slice_from(masked, -(src.len() as i64) + l_length);
        let mut last_index = 0;
        while let Some(found) = end_reg.exec_at(clipped, last_index) {
            last_index = found.end();
            let group = |n: usize| found.g(n).filter(|g| !g.is_empty());
            // Um `*` solto em `__abc*abc__` não é delimitador.
            let Some(r_delim) = (1..=6).find_map(group) else {
                continue;
            };
            let mut r_length = code_point_len(r_delim) as i64;
            if group(3).is_some() || group(4).is_some() {
                // Achou outro delimitador de abertura.
                delim_total += r_length;
                continue;
            } else if (group(5).is_some() || group(6).is_some()) && l_length % 3 != 0 && (l_length + r_length) % 3 == 0
            {
                // Regras 9 e 10 de ênfase do CommonMark.
                mid_delim_total += r_length;
                continue;
            }
            delim_total -= r_length;
            if delim_total > 0 {
                // Ainda faltam delimitadores de fechamento.
                continue;
            }
            // Tira os caracteres sobrando: `*a***` vira `*a*`.
            r_length = r_length.min(r_length + delim_total + mid_delim_total);
            // Um caractere pode ter mais de uma unidade.
            let last_char_length = code_point_unit_len(found.whole()) as i64;
            let raw = JsString::from(slice(
                src,
                0,
                l_length + found.start() as i64 + last_char_length + r_length,
            ));
            // `em` se o menor delimitador tem quantidade ímpar de caracteres: `*a***`.
            if l_length.min(r_length) % 2 != 0 {
                let text = JsString::from(slice(raw.units(), 1, -1));
                let mut tokens = Vec::new();
                self.inline_tokens(text.units(), &mut tokens)?;
                return Ok(Some(Token::Em(Em { raw, text, tokens })));
            }
            // `strong` se é par: `**a***`.
            let text = JsString::from(slice(raw.units(), 2, -2));
            let mut tokens = Vec::new();
            self.inline_tokens(text.units(), &mut tokens)?;
            return Ok(Some(Token::Strong(Strong { raw, text, tokens })));
        }
        Ok(None)
    }

    pub(super) fn codespan(&mut self, src: &[u16]) -> Option<Token> {
        let cap = self.rules.inline.code.exec(src)?;
        let mut text: Vec<u16> = cap
            .g(2)
            .unwrap_or_default()
            .iter()
            .map(|&c| if c == 0x0A { 0x20 } else { c })
            .collect();
        let has_non_space_chars = text.iter().any(|&c| c != 0x20);
        let has_space_chars_on_both_ends = text.first() == Some(&0x20) && text.last() == Some(&0x20);
        if has_non_space_chars && has_space_chars_on_both_ends {
            text = text[1..text.len() - 1].to_vec();
        }
        Some(Token::Codespan(Codespan {
            raw: JsString::from(cap.whole()),
            text: escape(&text, true),
        }))
    }

    pub(super) fn br(&mut self, src: &[u16]) -> Option<Token> {
        let cap = self.rules.inline.br.exec(src)?;
        Some(Token::Br(Br {
            raw: JsString::from(cap.whole()),
        }))
    }

    pub(super) fn del(&mut self, src: &[u16]) -> Result<Option<Token>, MarkedError> {
        let rules = Rc::clone(&self.rules);
        let Some(cap) = rules.inline.del.as_ref().and_then(|re| re.exec(src)) else {
            return Ok(None);
        };
        let text = JsString::from(cap.g(2).unwrap_or_default());
        let mut tokens = Vec::new();
        self.inline_tokens(text.units(), &mut tokens)?;
        Ok(Some(Token::Del(Del {
            raw: JsString::from(cap.whole()),
            text,
            tokens,
        })))
    }

    pub(super) fn autolink(&mut self, src: &[u16]) -> Option<Token> {
        let cap = self.rules.inline.autolink.exec(src)?;
        let text = escape(cap.g(1).unwrap_or_default(), false);
        let href = if cap.g(2) == Some(&[ch('@')][..]) {
            concat(&[&helpers::u("mailto:"), text.units()])
        } else {
            text.clone()
        };
        Some(link_of_text(JsString::from(cap.whole()), text, href))
    }

    pub(super) fn url(&mut self, src: &[u16]) -> Option<Token> {
        let cap = self.rules.inline.url.as_ref()?.exec(src)?;
        let whole = cap.whole();
        if cap.g(2) == Some(&[ch('@')][..]) {
            let text = escape(whole, false);
            let href = concat(&[&helpers::u("mailto:"), text.units()]);
            return Some(link_of_text(JsString::from(whole), text, href));
        }
        // Valida o caminho do autolink estendido.
        let backpedal = self.rules.inline.backpedal.as_ref()?;
        let mut current: Vec<u16> = whole.to_vec();
        loop {
            let next = backpedal.exec(&current).map(|c| c.whole().to_vec()).unwrap_or_default();
            let done = next == current;
            current = next;
            if done {
                break;
            }
        }
        let text = escape(&current, false);
        let href = if cap.g(1).is_some_and(|g| g.iter().copied().eq("www.".encode_utf16())) {
            concat(&[&helpers::u("http://"), &current])
        } else {
            JsString::from(current.as_slice())
        };
        Some(link_of_text(JsString::from(current.as_slice()), text, href))
    }

    pub(super) fn inline_text(&mut self, src: &[u16]) -> Option<Token> {
        let cap = self.rules.inline.text.exec(src)?;
        let text = if self.state.in_raw_block {
            JsString::from(cap.whole())
        } else {
            escape(cap.whole(), false)
        };
        Some(text_token(JsString::from(cap.whole()), text))
    }
}

/// O token de `autolink` e `url`: um link cujo único filho é o texto.
fn link_of_text(raw: JsString, text: JsString, href: JsString) -> Token {
    Token::Link(Link {
        raw,
        href,
        title: None,
        text: text.clone(),
        tokens: vec![text_token(text.clone(), text)],
    })
}

/// `item.align.push(...)` conforme a linha delimitadora.
fn align_of(cell: &[u16]) -> Option<Align> {
    let inner = cell.iter().skip_while(|&&c| c == 0x20).copied().collect::<Vec<_>>();
    let inner = &inner[..inner.len() - inner.iter().rev().take_while(|&&c| c == 0x20).count()];
    let dashes = |x: &[u16]| !x.is_empty() && x.iter().all(|&c| c == ch('-'));
    let colon = ch(':');
    if let Some(rest) = inner.strip_suffix(&[colon]) {
        if dashes(rest) {
            return Some(Align::Right);
        }
        if rest.first() == Some(&colon) && dashes(&rest[1..]) {
            return Some(Align::Center);
        }
    } else if inner.first() == Some(&colon) && dashes(&inner[1..]) {
        return Some(Align::Left);
    }
    None
}

/// `line.replace(/^\t+/, (t) => ' '.repeat(3 * t.length))`.
fn expand_leading_tabs(line: &[u16]) -> Vec<u16> {
    let tabs = line.iter().take_while(|&&c| c == 0x09).count();
    let mut out = vec![0x20; tabs * 3];
    out.extend_from_slice(&line[tabs..]);
    out
}
