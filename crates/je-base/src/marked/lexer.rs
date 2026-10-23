//! O `_Lexer` do `marked`: laço de bloco (`blockTokens`), laço de linha (`inlineTokens`) e a fila de
//! trabalho de linha (`inlineQueue`).

use std::collections::HashMap;
use std::rc::Rc;

use super::helpers::{self, ch, concat, tail};
use super::re::Re;
use super::rules::Rules;
use super::{
    Blockquote, Heading, LinkDefinition, List, ListItem, MAX_NESTING_DEPTH, MarkedError, Options, Paragraph, Table,
    TableCell, Text, Token, TokensList,
};
use crate::JsString;

/// Até esta profundidade o lexer roda na pilha de quem chamou; documentos reais não passam disso.
const SHALLOW_DEPTH: usize = 32;

/// Pilha da thread que refaz a lexagem de um documento muito aninhado: em depuração cada nível de
/// citação ou lista usa cerca de 15 KiB, e o V8 aguenta perto de dois mil níveis.
#[cfg(not(target_arch = "wasm32"))]
const DEEP_STACK_BYTES: usize = 256 * 1024 * 1024;

/// Se a lexagem rasa estourou o limite de profundidade, refaz numa thread com pilha grande e o limite
/// [`MAX_NESTING_DEPTH`] (o que o V8 faria com a pilha dele). No wasm32 não há threads: vale o limite raso.
#[cfg(not(target_arch = "wasm32"))]
fn retry_deep<T: Send + 'static>(
    shallow: Result<T, MarkedError>,
    src: &[u16],
    options: Options,
    run: fn(Lexer, &[u16]) -> Result<T, MarkedError>,
) -> Result<T, MarkedError> {
    if !matches!(shallow, Err(MarkedError::RangeError)) {
        return shallow;
    }
    let src = src.to_vec();
    std::thread::Builder::new()
        .stack_size(DEEP_STACK_BYTES)
        .spawn(move || {
            let mut lexer = Lexer::new(options);
            lexer.max_depth = MAX_NESTING_DEPTH;
            run(lexer, &src)
        })
        .ok()
        .and_then(|handle| handle.join().ok())
        .unwrap_or(Err(MarkedError::RangeError))
}

#[cfg(target_arch = "wasm32")]
fn retry_deep<T>(
    shallow: Result<T, MarkedError>,
    _src: &[u16],
    _options: Options,
    _run: fn(Lexer, &[u16]) -> Result<T, MarkedError>,
) -> Result<T, MarkedError> {
    shallow
}

/// O `lexer.state` do original.
pub(super) struct State {
    pub(super) in_link: bool,
    pub(super) in_raw_block: bool,
    pub(super) top: bool,
}

/// Um item de `inlineQueue`: o texto a lexar em linha e o token que recebe o resultado.
struct QueueEntry {
    src: JsString,
    slot: usize,
}

pub struct Lexer {
    pub(super) options: Options,
    pub(super) rules: Rc<Rules>,
    pub(super) state: State,
    pub(super) links: HashMap<JsString, LinkDefinition>,
    queue: Vec<QueueEntry>,
    next_slot: usize,
    pub(super) depth: usize,
    max_depth: usize,
}

thread_local! {
    static CRLF: Re = Re::new(r"\r\n|\r", "g");
    static LEADING_TABS: Re = Re::new(r"^( *)(\t+)", "gm");
    static TABS: Re = Re::new(r"\t", "g");
    static BLANK_LINES: Re = Re::new(r"^ +$", "gm");
}

impl Lexer {
    pub fn new(options: Options) -> Lexer {
        let rules = Rules::for_options(&options);
        Lexer {
            options,
            rules,
            state: State {
                in_link: false,
                in_raw_block: false,
                top: true,
            },
            links: HashMap::new(),
            queue: Vec::new(),
            next_slot: 0,
            depth: 0,
            max_depth: SHALLOW_DEPTH,
        }
    }

    /// Nome, fonte e flags de cada regex em uso, para conferência contra o `marked.js` real.
    #[doc(hidden)]
    pub fn rule_sources(options: &Options) -> Vec<(&'static str, String, String)> {
        Rules::for_options(options).sources()
    }

    /// `Lexer.lex(src, options)`: os tokens de topo e o mapa `links`.
    pub fn lex(self, src: &[u16]) -> Result<TokensList, MarkedError> {
        let options = self.options.clone();
        retry_deep(self.lex_here(src), src, options, |lexer, src| lexer.lex_here(src))
    }

    fn lex_here(mut self, src: &[u16]) -> Result<TokensList, MarkedError> {
        let src = CRLF.with(|re| re.replace_all(src, |_| JsString::from("\n")));
        let mut tokens = Vec::new();
        self.block_tokens(src.units(), &mut tokens, false)?;
        let queue = std::mem::take(&mut self.queue);
        let mut results: HashMap<usize, Vec<Token>> = HashMap::with_capacity(queue.len());
        for entry in queue {
            let mut inline = Vec::new();
            self.inline_tokens(entry.src.units(), &mut inline)?;
            results.insert(entry.slot, inline);
        }
        fill(&mut tokens, &mut results);
        Ok(TokensList {
            tokens,
            links: self.links,
        })
    }

    /// `Lexer.lexInline(src, options)`.
    pub fn lex_inline(self, src: &[u16]) -> Result<Vec<Token>, MarkedError> {
        let options = self.options.clone();
        retry_deep(self.lex_inline_here(src), src, options, |lexer, src| {
            lexer.lex_inline_here(src)
        })
    }

    fn lex_inline_here(mut self, src: &[u16]) -> Result<Vec<Token>, MarkedError> {
        let mut tokens = Vec::new();
        self.inline_tokens(src, &mut tokens)?;
        Ok(tokens)
    }

    /// `lexer.inline(src)`: agenda o texto e devolve o número do item, que o token guarda.
    pub(super) fn inline(&mut self, src: JsString) -> usize {
        let slot = self.next_slot;
        self.next_slot += 1;
        self.queue.push(QueueEntry { src, slot });
        slot
    }

    /// `inlineQueue.pop()`.
    fn queue_pop(&mut self) {
        self.queue.pop();
    }

    /// `inlineQueue[inlineQueue.length - 1].src = src`; sem item, o JavaScript lança `TypeError`.
    fn queue_set_last(&mut self, src: JsString) -> Result<(), MarkedError> {
        match self.queue.last_mut() {
            Some(entry) => {
                entry.src = src;
                Ok(())
            }
            None => Err(MarkedError::TypeError(
                "Cannot set properties of undefined (setting 'src')",
            )),
        }
    }

    fn enter(&mut self) -> Result<(), MarkedError> {
        if self.depth >= self.max_depth {
            return Err(MarkedError::RangeError);
        }
        self.depth += 1;
        Ok(())
    }

    /// A mensagem do laço infinito (ou, com `silent`, `Ok(true)` para o chamador parar).
    fn infinite_loop(&self, src: &[u16]) -> Result<bool, MarkedError> {
        if self.options.silent {
            Ok(true)
        } else {
            Err(MarkedError::InfiniteLoop { byte: src[0] })
        }
    }

    /// `lexer.blockTokens(src, tokens, lastParagraphClipped)`.
    pub(super) fn block_tokens(
        &mut self,
        src: &[u16],
        tokens: &mut Vec<Token>,
        last_paragraph_clipped: bool,
    ) -> Result<(), MarkedError> {
        self.enter()?;
        let result = self.block_tokens_inner(src, tokens, last_paragraph_clipped);
        self.depth -= 1;
        result
    }

    fn block_tokens_inner(
        &mut self,
        src: &[u16],
        tokens: &mut Vec<Token>,
        mut last_paragraph_clipped: bool,
    ) -> Result<(), MarkedError> {
        let prepared: JsString;
        let mut src: &[u16] = if self.options.pedantic {
            let spaced = TABS.with(|re| re.replace_all(src, |_| JsString::from("    ")));
            prepared = BLANK_LINES.with(|re| re.remove_all(spaced.units()));
            prepared.units()
        } else if src.contains(&0x09) {
            prepared = LEADING_TABS.with(|re| {
                re.replace_all(src, |c| {
                    let mut out = JsString::from(c.g(1).unwrap_or_default());
                    for _ in 0..c.g(2).map_or(0, <[u16]>::len) {
                        out.push_units(&[0x20; 4]);
                    }
                    out
                })
            });
            prepared.units()
        } else {
            src
        };

        while !src.is_empty() {
            // newline
            if let Some(token) = self.space(src) {
                src = tail(src, token.raw().len());
                if token.raw().len() == 1 && !tokens.is_empty() {
                    // Um único `\n` como espaçador termina a última linha: vai para ela.
                    if let Some(last) = tokens.last_mut() {
                        last.raw_mut().push_unit(0x0A);
                    }
                } else {
                    tokens.push(token);
                }
                continue;
            }
            // code
            if let Some(token) = self.code(src) {
                src = tail(src, token.raw().len());
                if tokens
                    .last()
                    .is_some_and(|l| matches!(l, Token::Paragraph(_) | Token::Text(_)))
                {
                    // Um bloco de código por recuo não interrompe um parágrafo.
                    let last = tokens.last_mut().expect("checado");
                    let merged_raw = concat(&[last.raw().units(), &[0x0A], token.raw().units()]);
                    *last.raw_mut() = merged_raw;
                    let code_text = token.text().cloned().unwrap_or_default();
                    let text = last.text_mut().expect("parágrafo e texto têm text");
                    *text = concat(&[text.units(), &[0x0A], code_text.units()]);
                    let new_src = text.clone();
                    self.queue_set_last(new_src)?;
                } else {
                    tokens.push(token);
                }
                continue;
            }
            // fences
            if let Some(token) = self.fences(src) {
                src = tail(src, token.raw().len());
                tokens.push(token);
                continue;
            }
            // heading
            if let Some(token) = self.heading(src) {
                src = tail(src, token.raw().len());
                tokens.push(token);
                continue;
            }
            // hr
            if let Some(token) = self.hr(src) {
                src = tail(src, token.raw().len());
                tokens.push(token);
                continue;
            }
            // blockquote
            if let Some(token) = self.blockquote(src)? {
                src = tail(src, token.raw().len());
                tokens.push(token);
                continue;
            }
            // list
            if let Some(token) = self.list(src)? {
                src = tail(src, token.raw().len());
                tokens.push(token);
                continue;
            }
            // html
            if let Some(token) = self.html(src) {
                src = tail(src, token.raw().len());
                tokens.push(token);
                continue;
            }
            // def
            if let Some(Token::Def(def)) = self.def(src) {
                src = tail(src, def.raw.len());
                if tokens
                    .last()
                    .is_some_and(|l| matches!(l, Token::Paragraph(_) | Token::Text(_)))
                {
                    let last = tokens.last_mut().expect("checado");
                    let merged_raw = concat(&[last.raw().units(), &[0x0A], def.raw.units()]);
                    *last.raw_mut() = merged_raw;
                    let text = last.text_mut().expect("parágrafo e texto têm text");
                    *text = concat(&[text.units(), &[0x0A], def.raw.units()]);
                    let new_src = text.clone();
                    self.queue_set_last(new_src)?;
                } else if !self.links.contains_key(def.tag.units()) {
                    self.links.insert(
                        def.tag,
                        LinkDefinition {
                            href: def.href,
                            title: def.title,
                        },
                    );
                }
                continue;
            }
            // table (gfm)
            if let Some(token) = self.table(src) {
                src = tail(src, token.raw().len());
                tokens.push(token);
                continue;
            }
            // lheading
            if let Some(token) = self.lheading(src) {
                src = tail(src, token.raw().len());
                tokens.push(token);
                continue;
            }
            // top-level paragraph
            if self.state.top
                && let Some(token) = self.paragraph(src)
            {
                let raw_len = token.raw().len();
                if last_paragraph_clipped && matches!(tokens.last(), Some(Token::Paragraph(_))) {
                    let last = tokens.last_mut().expect("checado");
                    let merged_raw = concat(&[last.raw().units(), &[0x0A], token.raw().units()]);
                    *last.raw_mut() = merged_raw;
                    let new_text = token.text().cloned().unwrap_or_default();
                    let text = last.text_mut().expect("parágrafo tem text");
                    *text = concat(&[text.units(), &[0x0A], new_text.units()]);
                    let new_src = text.clone();
                    self.queue_pop();
                    self.queue_set_last(new_src)?;
                } else {
                    tokens.push(token);
                }
                // Sem extensões de bloco o texto nunca é cortado antes do fim.
                last_paragraph_clipped = false;
                src = tail(src, raw_len);
                continue;
            }
            // text
            if let Some(token) = self.text(src) {
                src = tail(src, token.raw().len());
                if tokens.last().is_some_and(|l| matches!(l, Token::Text(_))) {
                    let last = tokens.last_mut().expect("checado");
                    let merged_raw = concat(&[last.raw().units(), &[0x0A], token.raw().units()]);
                    *last.raw_mut() = merged_raw;
                    let new_text = token.text().cloned().unwrap_or_default();
                    let text = last.text_mut().expect("texto tem text");
                    *text = concat(&[text.units(), &[0x0A], new_text.units()]);
                    let new_src = text.clone();
                    self.queue_pop();
                    self.queue_set_last(new_src)?;
                } else {
                    tokens.push(token);
                }
                continue;
            }
            if self.infinite_loop(src)? {
                break;
            }
        }
        self.state.top = true;
        Ok(())
    }

    /// `lexer.inlineTokens(src, tokens)`.
    pub(super) fn inline_tokens(&mut self, src: &[u16], tokens: &mut Vec<Token>) -> Result<(), MarkedError> {
        self.enter()?;
        let result = self.inline_tokens_inner(src, tokens);
        self.depth -= 1;
        result
    }

    fn inline_tokens_inner(&mut self, src: &[u16], tokens: &mut Vec<Token>) -> Result<(), MarkedError> {
        let rules = Rc::clone(&self.rules);
        let inline = &rules.inline;

        // O texto com os links e os trechos de código mascarados, para a ênfase não os atravessar.
        // Cada regex de máscara exige um caractere que, ausente, dispensa a busca (que o motor faria posição
        // por posição); a dispensa não muda o resultado.
        let has = |c: char| src.contains(&ch(c));
        let mut masked: Vec<u16> = src.to_vec();
        if !self.links.is_empty() && has('[') {
            let mut last_index = 0;
            while let Some(found) = inline.reflink_search.exec_at(&masked, last_index) {
                let whole = found.whole();
                let open = whole.iter().rposition(|&c| c == ch('[')).map_or(-1, |i| i as i64);
                let key = helpers::slice(whole, open + 1, -1);
                let end = found.end();
                if self.links.contains_key(key) {
                    let mut next = masked[..found.start()].to_vec();
                    next.push(ch('['));
                    next.extend(std::iter::repeat_n(ch('a'), whole.len().saturating_sub(2)));
                    next.push(ch(']'));
                    next.extend_from_slice(tail(&masked, end));
                    masked = next;
                }
                last_index = end;
            }
        }
        let mut last_index = 0;
        let block_skip_possible = has('[') || has('`') || has('<');
        while let Some(found) = block_skip_possible
            .then(|| inline.block_skip.exec_at(&masked, last_index))
            .flatten()
        {
            let (start, end, len) = (found.start(), found.end(), found.whole().len());
            let mut next = masked[..start].to_vec();
            next.push(ch('['));
            next.extend(std::iter::repeat_n(ch('a'), len.saturating_sub(2)));
            next.push(ch(']'));
            next.extend_from_slice(tail(&masked, end));
            masked = next;
            last_index = end;
        }
        let mut last_index = 0;
        let any_punctuation_possible = has('\\');
        while let Some(found) = any_punctuation_possible
            .then(|| inline.any_punctuation.exec_at(&masked, last_index))
            .flatten()
        {
            let (start, end) = (found.start(), found.end());
            let mut next = masked[..start].to_vec();
            next.extend_from_slice(&[ch('+'), ch('+')]);
            next.extend_from_slice(tail(&masked, end));
            masked = next;
            last_index = end;
        }

        let mut src: &[u16] = src;
        let mut keep_prev_char = false;
        let mut prev_char: Option<u16> = None;
        while !src.is_empty() {
            if !keep_prev_char {
                prev_char = None;
            }
            keep_prev_char = false;

            // escape
            if let Some(token) = self.escape(src) {
                src = tail(src, token.raw().len());
                tokens.push(token);
                continue;
            }
            // tag
            if let Some(token) = self.tag(src) {
                src = tail(src, token.raw().len());
                tokens.push(token);
                continue;
            }
            // link
            if let Some(token) = self.link(src)? {
                src = tail(src, token.raw().len());
                tokens.push(token);
                continue;
            }
            // reflink, nolink
            if let Some(token) = self.reflink(src)? {
                src = tail(src, token.raw().len());
                if let (Some(Token::Text(last)), Token::Text(new)) = (tokens.last_mut(), &token) {
                    last.raw = concat(&[last.raw.units(), new.raw.units()]);
                    last.text = concat(&[last.text.units(), new.text.units()]);
                } else {
                    tokens.push(token);
                }
                continue;
            }
            // em & strong
            if let Some(token) = self.em_strong(src, &masked, prev_char)? {
                src = tail(src, token.raw().len());
                tokens.push(token);
                continue;
            }
            // code
            if let Some(token) = self.codespan(src) {
                src = tail(src, token.raw().len());
                tokens.push(token);
                continue;
            }
            // br
            if let Some(token) = self.br(src) {
                src = tail(src, token.raw().len());
                tokens.push(token);
                continue;
            }
            // del (gfm)
            if let Some(token) = self.del(src)? {
                src = tail(src, token.raw().len());
                tokens.push(token);
                continue;
            }
            // autolink
            if let Some(token) = self.autolink(src) {
                src = tail(src, token.raw().len());
                tokens.push(token);
                continue;
            }
            // url (gfm)
            if !self.state.in_link
                && let Some(token) = self.url(src)
            {
                src = tail(src, token.raw().len());
                tokens.push(token);
                continue;
            }
            // text
            if let Some(token) = self.inline_text(src) {
                src = tail(src, token.raw().len());
                if token.raw().units().last() != Some(&ch('_')) {
                    // Guarda o último caractere antes de uma sequência de `____`.
                    prev_char = token.raw().units().last().copied();
                }
                keep_prev_char = true;
                if let (Some(Token::Text(last)), Token::Text(new)) = (tokens.last_mut(), &token) {
                    last.raw = concat(&[last.raw.units(), new.raw.units()]);
                    last.text = concat(&[last.text.units(), new.text.units()]);
                } else {
                    tokens.push(token);
                }
                continue;
            }
            if self.infinite_loop(src)? {
                break;
            }
        }
        Ok(())
    }
}

/// Preenche os `tokens` de linha dos tokens de bloco com o resultado da fila.
fn fill(tokens: &mut [Token], results: &mut HashMap<usize, Vec<Token>>) {
    for token in tokens {
        match token {
            Token::Heading(Heading { tokens, slot, .. }) | Token::Paragraph(Paragraph { tokens, slot, .. }) => {
                if let Some(found) = (*slot).and_then(|s| results.remove(&s)) {
                    *tokens = found;
                }
            }
            Token::Text(Text {
                tokens,
                slot: Some(slot),
                ..
            }) => {
                *tokens = Some(results.remove(slot).unwrap_or_default());
            }
            Token::Blockquote(Blockquote { tokens, .. }) => fill(tokens, results),
            Token::List(List { items, .. }) => {
                for item in items {
                    if let Token::ListItem(ListItem { tokens, .. }) = item {
                        fill(tokens, results);
                    }
                }
            }
            Token::Table(Table { header, rows, .. }) => {
                for cell in header.iter_mut().chain(rows.iter_mut().flatten()) {
                    fill_cell(cell, results);
                }
            }
            _ => {}
        }
    }
}

fn fill_cell(cell: &mut TableCell, results: &mut HashMap<usize, Vec<Token>>) {
    if let Some(found) = cell.slot.and_then(|s| results.remove(&s)) {
        cell.tokens = found;
    }
}

/// `marked.lexer(src)` com as opções padrão.
pub fn lexer(src: &[u16]) -> Result<TokensList, MarkedError> {
    Lexer::new(Options::default()).lex(src)
}

/// `marked.lexer(src, options)`.
pub fn lexer_with(src: &[u16], options: Options) -> Result<TokensList, MarkedError> {
    Lexer::new(options).lex(src)
}

/// `marked.Lexer.lexInline(src, options)`.
pub fn lex_inline(src: &[u16], options: Options) -> Result<Vec<Token>, MarkedError> {
    Lexer::new(options).lex_inline(src)
}
