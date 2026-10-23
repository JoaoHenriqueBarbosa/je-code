//! Porte de `.../bracketPairsTree/tokenizer.ts`: o `Token` e os dois tokenizadores de colchetes. O
//! `TextBufferTokenizer` lê os tokens do modelo (ciente de comentários e strings); o `FastTokenizer` varre o
//! texto com uma regex, sem informação de tokens, para a árvore inicial.
//!
//! Divergências:
//! - `ITokenizerSource` do original expõe `tokenization.getLineTokens`; aqui é o método
//!   [`ITokenizerSource::get_line_tokens`], que o `TextModel` implementa.
//! - Os tokens são `Rc<Token>` (o original compartilha o mesmo objeto entre usos), e `bracketId` é um `i64`
//!   com `-1` para os tokens de texto.
//! - `TextBufferTokenizer` guarda a fonte por empréstimo (`&dyn ITokenizerSource`).

use std::rc::Rc;

use je_base::JsString;
use je_base::errors::{self, Error};

use super::ast::{AstNodeRef, TextAstNode};
use super::brackets::{BracketTokens, LanguageAgnosticBracketTokens};
use super::length::{
    LENGTH_ZERO, Length, length_add, length_diff, length_get_column_count_if_zero_line_count,
    length_to_obj, to_length,
};
use super::small_immutable_set::SmallImmutableSet;
use crate::encoded_token_attributes::{TokenMetadata, standard_token_type};
use crate::js_reg_exp::JsRegExp;
use crate::line_tokens::{IViewLineTokens, LineTokens};

/// `Tokenizer`.
pub trait Tokenizer {
    fn offset(&self) -> Length;
    fn length(&self) -> Length;
    fn read(&mut self) -> Option<Rc<Token>>;
    fn peek(&mut self) -> Option<Rc<Token>>;
    fn skip(&mut self, length: Length);
    fn get_text(&self) -> JsString;
}

/// `TokenKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Text = 0,
    OpeningBracket = 1,
    ClosingBracket = 2,
}

/// `Token`.
pub struct Token {
    pub length: Length,
    pub kind: TokenKind,
    /// Numa abertura, o id dela. Num fechamento, o id da primeira abertura que ele fecha. Senão, `-1`.
    pub bracket_id: i64,
    /// Numa abertura, só `bracket_id`. Num fechamento, os ids de todas as aberturas que ele fecha. Senão,
    /// vazio.
    pub bracket_ids: SmallImmutableSet,
    pub ast_node: Option<AstNodeRef>,
}

impl Token {
    pub fn new(
        length: Length,
        kind: TokenKind,
        bracket_id: i64,
        bracket_ids: SmallImmutableSet,
        ast_node: Option<AstNodeRef>,
    ) -> Self {
        Self {
            length,
            kind,
            bracket_id,
            bracket_ids,
            ast_node,
        }
    }

    fn text(length: Length) -> Rc<Token> {
        Rc::new(Token::new(
            length,
            TokenKind::Text,
            -1,
            SmallImmutableSet::get_empty(),
            Some(TextAstNode::create(length)),
        ))
    }
}

/// `ITokenizerSource`.
pub trait ITokenizerSource {
    fn get_value(&self) -> JsString;
    fn get_line_count(&self) -> i32;
    fn get_line_length(&self, line_number: i32) -> usize;
    /// `tokenization.getLineTokens(lineNumber)`.
    fn get_line_tokens(&self, line_number: i32) -> LineTokens;
}

/// `TextBufferTokenizer`.
pub struct TextBufferTokenizer<'a> {
    text_model: &'a dyn ITokenizerSource,
    reader: NonPeekableTextBufferTokenizer<'a>,
    text_buffer_line_count: i32,
    text_buffer_last_line_length: usize,
    offset: Length,
    did_peek: bool,
    peeked: Option<Rc<Token>>,
}

impl<'a> TextBufferTokenizer<'a> {
    pub fn new(
        text_model: &'a dyn ITokenizerSource,
        bracket_tokens: &'a LanguageAgnosticBracketTokens,
    ) -> Self {
        let text_buffer_line_count = text_model.get_line_count();
        Self {
            text_model,
            reader: NonPeekableTextBufferTokenizer::new(text_model, bracket_tokens),
            text_buffer_line_count,
            text_buffer_last_line_length: text_model.get_line_length(text_buffer_line_count),
            offset: LENGTH_ZERO,
            did_peek: false,
            peeked: None,
        }
    }
}

impl Tokenizer for TextBufferTokenizer<'_> {
    fn offset(&self) -> Length {
        self.offset
    }

    fn length(&self) -> Length {
        to_length(
            i64::from(self.text_buffer_line_count) - 1,
            self.text_buffer_last_line_length as i64,
        )
    }

    fn get_text(&self) -> JsString {
        self.text_model.get_value()
    }

    fn skip(&mut self, length: Length) {
        self.did_peek = false;
        self.offset = length_add(self.offset, length);
        let obj = length_to_obj(self.offset);
        self.reader
            .set_position(obj.line_count as i32, obj.column_count as usize);
    }

    fn read(&mut self) -> Option<Rc<Token>> {
        let token = if let Some(peeked) = self.peeked.clone() {
            self.did_peek = false;
            Some(peeked)
        } else {
            self.reader.read()
        };
        if let Some(token) = &token {
            self.offset = length_add(self.offset, token.length);
        }
        token
    }

    fn peek(&mut self) -> Option<Rc<Token>> {
        if !self.did_peek {
            self.peeked = self.reader.read();
            self.did_peek = true;
        }
        self.peeked.clone()
    }
}

/// Não suporta `peek`.
struct NonPeekableTextBufferTokenizer<'a> {
    text_model: &'a dyn ITokenizerSource,
    bracket_tokens: &'a LanguageAgnosticBracketTokens,
    text_buffer_line_count: i32,
    text_buffer_last_line_length: usize,
    line_idx: i32,
    line: Option<JsString>,
    line_char_offset: usize,
    line_tokens: Option<LineTokens>,
    line_token_offset: usize,
    /// Tem de ser um token de zero linhas. O fim do documento não pode ser espiado.
    peeked_token: Option<Rc<Token>>,
}

impl<'a> NonPeekableTextBufferTokenizer<'a> {
    fn new(
        text_model: &'a dyn ITokenizerSource,
        bracket_tokens: &'a LanguageAgnosticBracketTokens,
    ) -> Self {
        let text_buffer_line_count = text_model.get_line_count();
        Self {
            text_model,
            bracket_tokens,
            text_buffer_line_count,
            text_buffer_last_line_length: text_model.get_line_length(text_buffer_line_count),
            line_idx: 0,
            line: None,
            line_char_offset: 0,
            line_tokens: None,
            line_token_offset: 0,
            peeked_token: None,
        }
    }

    fn set_position(&mut self, line_idx: i32, column: usize) {
        // Não podemos pular para dentro de um token!
        if line_idx == self.line_idx {
            self.line_char_offset = column;
            if self.line.is_some() {
                self.line_token_offset = if self.line_char_offset == 0 {
                    0
                } else {
                    self.line_tokens
                        .as_ref()
                        .expect("a linha tem tokens")
                        .find_token_index_at_offset(self.line_char_offset as i32)
                };
            }
        } else {
            self.line_idx = line_idx;
            self.line_char_offset = column;
            self.line = None;
        }
        self.peeked_token = None;
    }

    fn read(&mut self) -> Option<Rc<Token>> {
        if let Some(token) = self.peeked_token.take() {
            self.line_char_offset +=
                length_get_column_count_if_zero_line_count(token.length) as usize;
            return Some(token);
        }

        if self.line_idx > self.text_buffer_line_count - 1
            || (self.line_idx == self.text_buffer_line_count - 1
                && self.line_char_offset >= self.text_buffer_last_line_length)
        {
            // Estamos depois do fim
            return None;
        }

        if self.line.is_none() {
            let line_tokens = self.text_model.get_line_tokens(self.line_idx + 1);
            self.line = Some(line_tokens.get_line_content());
            self.line_token_offset = if self.line_char_offset == 0 {
                0
            } else {
                line_tokens.find_token_index_at_offset(self.line_char_offset as i32)
            };
            self.line_tokens = Some(line_tokens);
        }

        let start_line_idx = self.line_idx;
        let start_line_char_offset = self.line_char_offset;

        // Limita o comprimento dos tokens de texto. Se ficarem longos, as atualizações incrementais ficam lentas.
        let mut length_heuristic: usize = 0;

        loop {
            let token_count = self
                .line_tokens
                .as_ref()
                .expect("a linha tem tokens")
                .get_count();

            let mut peeked_bracket_token: Option<Rc<Token>> = None;

            if self.line_token_offset < token_count {
                let line_tokens = self.line_tokens.as_ref().expect("a linha tem tokens");
                let token_metadata = line_tokens.get_metadata(self.line_token_offset);

                while self.line_token_offset + 1 < token_count
                    && token_metadata == line_tokens.get_metadata(self.line_token_offset + 1)
                {
                    // Pula os tokens idênticos. Às vezes um identificador (de colchete) é dividido em vários tokens.
                    self.line_token_offset += 1;
                }

                let is_other =
                    TokenMetadata::get_token_type(token_metadata) == standard_token_type::OTHER;
                let contains_bracket_type =
                    TokenMetadata::contains_balanced_brackets(token_metadata);

                let end_offset = line_tokens.get_end_offset(self.line_token_offset) as usize;
                // Há um token de colchete adiante? Só consome texto.
                if contains_bracket_type && is_other && self.line_char_offset < end_offset {
                    let language_id = line_tokens.get_language_id(self.line_token_offset);
                    let line = self.line.as_ref().expect("a linha foi lida");
                    let text = &line.units()[self.line_char_offset..end_offset.min(line.len())];
                    let brackets = self
                        .bracket_tokens
                        .get_single_language_bracket_tokens(&language_id);
                    if let Some(regexp) = brackets.reg_exp_global() {
                        regexp.set_last_index(0);
                        if let Some(found) = regexp.exec(text) {
                            peeked_bracket_token =
                                brackets.get_token(&text[found.start()..found.end()]);
                            if peeked_bracket_token.is_some() {
                                // Consome o texto que vem antes do token
                                self.line_char_offset += found.start();
                            }
                        }
                    }
                }

                length_heuristic += end_offset.saturating_sub(self.line_char_offset);

                if let Some(bracket_token) = peeked_bracket_token {
                    // Não pula o token inteiro: um token pode conter vários colchetes.
                    if start_line_idx != self.line_idx
                        || start_line_char_offset != self.line_char_offset
                    {
                        // Há texto antes do colchete
                        self.peeked_token = Some(bracket_token);
                        break;
                    } else {
                        // Consome o token espiado
                        self.line_char_offset +=
                            length_get_column_count_if_zero_line_count(bracket_token.length)
                                as usize;
                        return Some(bracket_token);
                    }
                } else {
                    // Pula o token inteiro: ele não tem colchete nenhum.
                    self.line_token_offset += 1;
                    self.line_char_offset = end_offset;
                }
            } else {
                if self.line_idx == self.text_buffer_line_count - 1 {
                    break;
                }
                self.line_idx += 1;
                let line_tokens = self.text_model.get_line_tokens(self.line_idx + 1);
                self.line_token_offset = 0;
                self.line = Some(line_tokens.get_line_content());
                self.line_tokens = Some(line_tokens);
                self.line_char_offset = 0;

                length_heuristic += 33; // no máximo 1000/33 = 30 linhas
                // Isto limita o trabalho de recalcular a indentação mínima
                if length_heuristic > 1000 {
                    // só quebra (automaticamente) no fim da linha.
                    break;
                }
            }

            if length_heuristic > 1500 {
                // Quebra de qualquer forma, para que linhas muito longas não causem desempenho ruim. Isto limita
                // a indentação máxima a 500, já que a indentação não é calculada entre vários nós de texto.
                break;
            }
        }

        // Se um token contém alguma indentação de verdade, ele contém também \n{INDENTAÇÃO+}(?!{INDENTAÇÃO}),
        // salvo se a linha for longa demais. Assim, a indentação mínima do documento é o mínimo das indentações
        // mínimas de cada nó de texto.
        let length = length_diff(
            i64::from(start_line_idx),
            start_line_char_offset as i64,
            i64::from(self.line_idx),
            self.line_char_offset as i64,
        );
        Some(Token::text(length))
    }
}

/// `FastTokenizer`.
pub struct FastTokenizer {
    offset: Length,
    length: Length,
    tokens: Vec<Rc<Token>>,
    idx: usize,
    text: JsString,
}

impl FastTokenizer {
    pub fn new(text: JsString, brackets: &BracketTokens) -> Self {
        let reg_exp_str = brackets.get_reg_exp_str();
        let regexp = reg_exp_str.map(|source| {
            let mut pattern = source.units().to_vec();
            pattern.extend("|\n".encode_utf16());
            JsRegExp::new(&pattern, "gi")
                .unwrap_or_else(|_| errors::throw(Error::new("regex de colchetes inválida")))
        });

        let mut tokens: Vec<Rc<Token>> = Vec::new();
        let mut cur_line_count: i64 = 0;
        let mut last_line_break_offset: i64 = 0;

        let mut last_token_end_offset: i64 = 0;
        let mut last_token_end_line: i64 = 0;

        let small_text_tokens_0_line: Vec<Rc<Token>> =
            (0..60).map(|i| Token::text(to_length(0, i))).collect();
        let small_text_tokens_1_line: Vec<Rc<Token>> =
            (0..60).map(|i| Token::text(to_length(1, i))).collect();

        if let Some(regexp) = &regexp {
            regexp.set_last_index(0);

            // Se um token contém indentação, ele contém também \n{INDENTAÇÃO+}(?!{INDENTAÇÃO})
            while let Some(found) = regexp.exec(text.units()) {
                let cur_offset = found.start() as i64;
                let value = &text.units()[found.start()..found.end()];

                if value == [10] {
                    cur_line_count += 1;
                    last_line_break_offset = cur_offset + 1;
                } else {
                    if last_token_end_offset != cur_offset {
                        let token: Rc<Token>;
                        if last_token_end_line == cur_line_count {
                            let col_count = cur_offset - last_token_end_offset;
                            if (col_count as usize) < small_text_tokens_0_line.len() {
                                token = small_text_tokens_0_line[col_count as usize].clone();
                            } else {
                                token = Token::text(to_length(0, col_count));
                            }
                        } else {
                            let line_count = cur_line_count - last_token_end_line;
                            let col_count = cur_offset - last_line_break_offset;
                            if line_count == 1
                                && (col_count as usize) < small_text_tokens_1_line.len()
                            {
                                token = small_text_tokens_1_line[col_count as usize].clone();
                            } else {
                                token = Token::text(to_length(line_count, col_count));
                            }
                        }
                        tokens.push(token);
                    }

                    // `value` casa com a regex, então o token existe
                    tokens.push(
                        brackets
                            .get_token(value)
                            .expect("o colchete casado tem token"),
                    );
                    last_token_end_offset = cur_offset + value.len() as i64;
                    last_token_end_line = cur_line_count;
                }
            }
        }

        let offset = text.len() as i64;
        if last_token_end_offset != offset {
            let length = if last_token_end_line == cur_line_count {
                to_length(0, offset - last_token_end_offset)
            } else {
                to_length(
                    cur_line_count - last_token_end_line,
                    offset - last_line_break_offset,
                )
            };
            tokens.push(Token::text(length));
        }

        Self {
            offset: LENGTH_ZERO,
            length: to_length(cur_line_count, offset - last_line_break_offset),
            tokens,
            idx: 0,
            text,
        }
    }
}

impl Tokenizer for FastTokenizer {
    fn offset(&self) -> Length {
        self.offset
    }

    fn length(&self) -> Length {
        self.length
    }

    fn read(&mut self) -> Option<Rc<Token>> {
        let token = self.tokens.get(self.idx).cloned();
        self.idx += 1;
        token
    }

    fn peek(&mut self) -> Option<Rc<Token>> {
        self.tokens.get(self.idx).cloned()
    }

    fn skip(&mut self, _length: Length) {
        errors::throw(Error::new("Not supported"))
    }

    fn get_text(&self) -> JsString {
        self.text.clone()
    }
}
