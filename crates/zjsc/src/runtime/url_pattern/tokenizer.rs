//! `URLPatternTokenizer.cpp` (WebCore do bun 1.4.2): https://urlpattern.spec.whatwg.org/#tokenizing.
//!
//! Como no C++, o erro do modo estrito fica guardado e só é lançado no topo da volta seguinte: um erro registrado na
//! última volta (o regex vazio no fim da entrada, que ainda acrescenta o token) se perde e a lista sai com o `End`.

use super::parser::{is_valid_name_codepoint, IsFirst};
use super::Units;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TokenType {
    Open,
    Close,
    Regexp,
    Name,
    Char,
    EscapedChar,
    OtherModifier,
    Asterisk,
    End,
    InvalidChar,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenizePolicy {
    Strict,
    Lenient,
}

/// `Token`: o `index` ausente é o token nulo (`isNull`).
#[derive(Clone, Debug)]
pub(crate) struct Token {
    pub(crate) kind: TokenType,
    pub(crate) index: Option<usize>,
    pub(crate) value: Units,
}

impl Token {
    pub(crate) fn null() -> Token {
        Token { kind: TokenType::End, index: None, value: Units::new() }
    }

    pub(crate) fn is_null(&self) -> bool {
        self.index.is_none()
    }
}

struct Tokenizer<'a> {
    input: &'a [u16],
    policy: TokenizePolicy,
    token_list: Vec<Token>,
    index: usize,
    next_index: usize,
    codepoint: u32,
}

fn is_lead(unit: u32) -> bool {
    (0xD800..=0xDBFF).contains(&unit)
}

fn is_trail(unit: u32) -> bool {
    (0xDC00..=0xDFFF).contains(&unit)
}

impl Tokenizer<'_> {
    /// https://urlpattern.spec.whatwg.org/#get-the-next-code-point
    fn get_next_code_point(&mut self) {
        self.codepoint = self.input[self.next_index] as u32;
        self.next_index += 1;
        if !is_lead(self.codepoint) || self.next_index >= self.input.len() {
            return;
        }
        let next = self.input[self.next_index] as u32;
        if !is_trail(next) {
            return;
        }
        self.next_index += 1;
        self.codepoint = 0x10000 + ((self.codepoint - 0xD800) << 10) + (next - 0xDC00);
    }

    /// https://urlpattern.spec.whatwg.org/#seek-and-get-the-next-code-point
    fn seek_next_code_point(&mut self, index: usize) {
        self.next_index = index;
        self.get_next_code_point();
    }

    /// https://urlpattern.spec.whatwg.org/#add-a-token
    fn add_token(&mut self, kind: TokenType, next_position: usize, value_position: usize, value_length: usize) {
        let value = self.input[value_position..value_position + value_length].to_vec();
        self.token_list.push(Token { kind, index: Some(self.index), value });
        self.index = next_position;
    }

    /// https://urlpattern.spec.whatwg.org/#add-a-token-with-default-length
    fn add_token_default_length(&mut self, kind: TokenType, next_position: usize, value_position: usize) {
        self.add_token(kind, next_position, value_position, next_position - value_position);
    }

    /// https://urlpattern.spec.whatwg.org/#add-a-token-with-default-position-and-length
    fn add_token_default(&mut self, kind: TokenType) {
        self.add_token_default_length(kind, self.next_index, self.index);
    }

    /// https://urlpattern.spec.whatwg.org/#process-a-tokenizing-error
    fn process_tokenizing_error(&mut self, next_position: usize, value_position: usize, message: String) -> Result<(), String> {
        if self.policy == TokenizePolicy::Strict {
            return Err(message);
        }
        self.add_token_default_length(TokenType::InvalidChar, next_position, value_position);
        Ok(())
    }

    /// https://urlpattern.spec.whatwg.org/#tokenize
    fn tokenize(mut self) -> Result<Vec<Token>, String> {
        let mut maybe_exception: Result<(), String> = Ok(());
        while self.index < self.input.len() {
            if self.policy == TokenizePolicy::Strict {
                maybe_exception.clone()?;
            }
            self.seek_next_code_point(self.index);
            let codepoint = self.codepoint;
            if codepoint == '*' as u32 {
                self.add_token_default(TokenType::Asterisk);
                continue;
            }
            if codepoint == '+' as u32 || codepoint == '?' as u32 {
                self.add_token_default(TokenType::OtherModifier);
                continue;
            }
            if codepoint == '\\' as u32 {
                if self.index == self.input.len() - 1 {
                    maybe_exception =
                        self.process_tokenizing_error(self.next_index, self.index, "No character is provided after escape.".into());
                    continue;
                }
                let escaped_index = self.next_index;
                self.get_next_code_point();
                self.add_token_default_length(TokenType::EscapedChar, self.next_index, escaped_index);
                continue;
            }
            if codepoint == '{' as u32 {
                self.add_token_default(TokenType::Open);
                continue;
            }
            if codepoint == '}' as u32 {
                self.add_token_default(TokenType::Close);
                continue;
            }
            if codepoint == ':' as u32 {
                let mut name_position = self.next_index;
                let name_start = name_position;
                while name_position < self.input.len() {
                    self.seek_next_code_point(name_position);
                    let first = if name_position == name_start { IsFirst::Yes } else { IsFirst::No };
                    if !is_valid_name_codepoint(self.codepoint, first) {
                        break;
                    }
                    name_position = self.next_index;
                }
                if name_position <= name_start {
                    let message = format!("Name position {name_position} is less than name start {name_start}");
                    maybe_exception = self.process_tokenizing_error(name_start, self.index, message);
                    continue;
                }
                self.add_token_default_length(TokenType::Name, name_position, name_start);
                continue;
            }
            if codepoint == '(' as u32 {
                let mut depth = 1;
                let mut regex_position = self.next_index;
                let regex_start = regex_position;
                let mut has_error = false;
                while regex_position < self.input.len() {
                    self.seek_next_code_point(regex_position);
                    if self.codepoint >= 0x80 {
                        maybe_exception = self.process_tokenizing_error(regex_start, self.index, "Current codepoint is not ascii".into());
                        has_error = true;
                        break;
                    }
                    if regex_position == regex_start && self.codepoint == '?' as u32 {
                        maybe_exception = self.process_tokenizing_error(regex_start, self.index, "Regex cannot start with modifier.".into());
                        has_error = true;
                        break;
                    }
                    if self.codepoint == '\\' as u32 {
                        if regex_position == self.input.len() - 1 {
                            maybe_exception =
                                self.process_tokenizing_error(regex_start, self.index, "No character is provided after escape.".into());
                            has_error = true;
                            break;
                        }
                        self.get_next_code_point();
                        if self.codepoint >= 0x80 {
                            maybe_exception = self.process_tokenizing_error(regex_start, self.index, "Current codepoint is not ascii".into());
                            has_error = true;
                            break;
                        }
                        regex_position = self.next_index;
                        continue;
                    }
                    if self.codepoint == ')' as u32 {
                        depth -= 1;
                        if depth == 0 {
                            regex_position = self.next_index;
                            break;
                        }
                    }
                    if self.codepoint == '(' as u32 {
                        depth += 1;
                        if regex_position == self.input.len() - 1 {
                            maybe_exception =
                                self.process_tokenizing_error(regex_start, self.index, "No closing token is provided by end of string.".into());
                            has_error = true;
                            break;
                        }
                        let temporary_position = self.next_index;
                        self.get_next_code_point();
                        if self.codepoint != '?' as u32 {
                            maybe_exception = self.process_tokenizing_error(
                                regex_start,
                                self.index,
                                "Required OtherModifier token is not provided in regex.".into(),
                            );
                            has_error = true;
                            break;
                        }
                        self.next_index = temporary_position;
                    }
                    regex_position = self.next_index;
                }
                if has_error {
                    continue;
                }
                if depth != 0 {
                    maybe_exception = self.process_tokenizing_error(
                        regex_start,
                        self.index,
                        "Current open token does not have a corresponding close token.".into(),
                    );
                    continue;
                }
                let regex_length = regex_position - regex_start - 1;
                if regex_length == 0 {
                    maybe_exception = self.process_tokenizing_error(regex_start, self.index, "Regex length is zero.".into());
                }
                self.add_token(TokenType::Regexp, regex_position, regex_start, regex_length);
                continue;
            }
            self.add_token_default(TokenType::Char);
        }
        let index = self.index;
        self.add_token(TokenType::End, index, index, 0);
        Ok(self.token_list)
    }
}

pub(crate) fn tokenize(input: &[u16], policy: TokenizePolicy) -> Result<Vec<Token>, String> {
    Tokenizer { input, policy, token_list: Vec::new(), index: 0, next_index: 0, codepoint: 0 }.tokenize()
}
