//! Porte de `src/vs/platform/contextkey/common/scanner.ts`: o analisador léxico das expressões de
//! `when`.
//!
//! O texto é lido em unidades UTF-16, para os `offset` baterem com os do original (que são índices de
//! `String` do JavaScript). Os lexemas saem como `String`.

use std::rc::Rc;

use je_base::nls::localize;
use zyarr::RegExp;

/// `TokenType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenType {
    LParen,
    RParen,
    Neg,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    RegexOp,
    RegexStr,
    True,
    False,
    In,
    Not,
    And,
    Or,
    Str,
    QuotedStr,
    Error,
    Eof,
}

/// `Token`. `lexeme` só existe para `RegexStr`, `Str`, `QuotedStr` e `Error`; `is_triple_eq` só para `Eq` e `NotEq`.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub ty: TokenType,
    pub offset: usize,
    pub lexeme: Option<String>,
    pub is_triple_eq: bool,
}

impl Token {
    fn plain(ty: TokenType, offset: usize) -> Self {
        Self { ty, offset, lexeme: None, is_triple_eq: false }
    }

    fn with_lexeme(ty: TokenType, offset: usize, lexeme: String) -> Self {
        Self { ty, offset, lexeme: Some(lexeme), is_triple_eq: false }
    }

    /// O lexema guardado no token (só os quatro tipos que o têm).
    pub fn lexeme(&self) -> &str {
        self.lexeme.as_deref().unwrap_or("")
    }
}

/// `LexingError`: o `offset` não leva em conta escapes da codificação original da string.
#[derive(Clone, Debug, PartialEq)]
pub struct LexingError {
    pub offset: usize,
    pub lexeme: String,
    pub additional_info: Option<String>,
}

fn hint_did_you_mean(meant: &[&str]) -> Option<String> {
    match meant {
        [a] => Some(localize("contextkey.scanner.hint.didYouMean1", "Did you mean {0}?", &[a])),
        [a, b] => Some(localize("contextkey.scanner.hint.didYouMean2", "Did you mean {0} or {1}?", &[a, b])),
        [a, b, c] => Some(localize("contextkey.scanner.hint.didYouMean3", "Did you mean {0}, {1} or {2}?", &[a, b, c])),
        _ => None,
    }
}

fn hint_did_you_forget_to_open_or_close_quote() -> String {
    localize("contextkey.scanner.hint.didYouForgetToOpenOrCloseQuote", "Did you forget to open or close the quote?", &[])
}

fn hint_did_you_forget_to_escape_slash() -> String {
    localize(
        "contextkey.scanner.hint.didYouForgetToEscapeSlash",
        "Did you forget to escape the '/' (slash) character? Put two backslashes before it to escape, e.g., '\\\\/'.",
        &[],
    )
}

/// Os caracteres de uma chave ou valor sem aspas (`stringRe` do original, com `u` e `y`).
const STRING_RE: &str = r#"[a-zA-Z0-9_<>\-\./\\:\*\?\+\[\]\^,#@;"%\$\p{L}-]+"#;

/// `Scanner.getLexeme`.
pub fn get_lexeme(token: &Token) -> String {
    match token.ty {
        TokenType::LParen => "(".into(),
        TokenType::RParen => ")".into(),
        TokenType::Neg => "!".into(),
        TokenType::Eq => if token.is_triple_eq { "===" } else { "==" }.into(),
        TokenType::NotEq => if token.is_triple_eq { "!==" } else { "!=" }.into(),
        TokenType::Lt => "<".into(),
        TokenType::LtEq => "<=".into(),
        TokenType::Gt => ">".into(),
        TokenType::GtEq => ">=".into(),
        TokenType::RegexOp => "=~".into(),
        TokenType::True => "true".into(),
        TokenType::False => "false".into(),
        TokenType::In => "in".into(),
        TokenType::Not => "not".into(),
        TokenType::And => "&&".into(),
        TokenType::Or => "||".into(),
        TokenType::Eof => "EOF".into(),
        TokenType::RegexStr | TokenType::Str | TokenType::QuotedStr | TokenType::Error => token.lexeme().to_string(),
    }
}

fn keyword(lexeme: &str) -> Option<TokenType> {
    match lexeme {
        "not" => Some(TokenType::Not),
        "in" => Some(TokenType::In),
        "false" => Some(TokenType::False),
        "true" => Some(TokenType::True),
        _ => None,
    }
}

fn is_regex_flag(unit: u16) -> bool {
    matches!(unit, 0x69 | 0x67 | 0x73 | 0x6d | 0x79 | 0x75) // i g s m y u
}

thread_local! {
    /// A regex compilada uma vez por thread (o original a tem como campo de cada `Scanner`).
    static STRING_REGEXP: Rc<RegExp> = Rc::new(RegExp::from_str(STRING_RE, "uy").expect("a expressão fixa do scanner compila"));
}

const fn u(c: char) -> u16 {
    c as u16
}

/// `Scanner`.
pub struct Scanner {
    input: Vec<u16>,
    start: usize,
    current: usize,
    tokens: Vec<Token>,
    errors: Vec<LexingError>,
    string_re: Rc<RegExp>,
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

impl Scanner {
    pub fn new() -> Self {
        Self {
            input: Vec::new(),
            start: 0,
            current: 0,
            tokens: Vec::new(),
            errors: Vec::new(),
            string_re: STRING_REGEXP.with(Rc::clone),
        }
    }

    pub fn errors(&self) -> &[LexingError] {
        &self.errors
    }

    pub fn reset(&mut self, value: &str) -> &mut Self {
        self.input = value.encode_utf16().collect();
        self.start = 0;
        self.current = 0;
        self.tokens = Vec::new();
        self.errors = Vec::new();
        self
    }

    pub fn scan(&mut self) -> Vec<Token> {
        while !self.is_at_end() {
            self.start = self.current;
            let ch = self.advance();
            match ch {
                c if c == u('(') => self.add_token(TokenType::LParen),
                c if c == u(')') => self.add_token(TokenType::RParen),
                c if c == u('!') => {
                    if self.match_unit(u('=')) {
                        let is_triple_eq = self.match_unit(u('='));
                        self.tokens.push(Token { ty: TokenType::NotEq, offset: self.start, lexeme: None, is_triple_eq });
                    } else {
                        self.add_token(TokenType::Neg);
                    }
                }
                c if c == u('\'') => self.quoted_string(),
                c if c == u('/') => self.regex(),
                c if c == u('=') => {
                    if self.match_unit(u('=')) {
                        let is_triple_eq = self.match_unit(u('='));
                        self.tokens.push(Token { ty: TokenType::Eq, offset: self.start, lexeme: None, is_triple_eq });
                    } else if self.match_unit(u('~')) {
                        self.add_token(TokenType::RegexOp);
                    } else {
                        self.error(hint_did_you_mean(&["==", "=~"]));
                    }
                }
                c if c == u('<') => {
                    let ty = if self.match_unit(u('=')) { TokenType::LtEq } else { TokenType::Lt };
                    self.add_token(ty);
                }
                c if c == u('>') => {
                    let ty = if self.match_unit(u('=')) { TokenType::GtEq } else { TokenType::Gt };
                    self.add_token(ty);
                }
                c if c == u('&') => {
                    if self.match_unit(u('&')) {
                        self.add_token(TokenType::And);
                    } else {
                        self.error(hint_did_you_mean(&["&&"]));
                    }
                }
                c if c == u('|') => {
                    if self.match_unit(u('|')) {
                        self.add_token(TokenType::Or);
                    } else {
                        self.error(hint_did_you_mean(&["||"]));
                    }
                }
                // Espaço, CR, tab, LF e NO-BREAK SPACE.
                0x20 | 0x0d | 0x09 | 0x0a | 0xa0 => {}
                _ => self.string(),
            }
        }
        self.start = self.current;
        self.add_token(TokenType::Eof);
        self.tokens.clone()
    }

    fn match_unit(&mut self, expected: u16) -> bool {
        if self.is_at_end() || self.input[self.current] != expected {
            return false;
        }
        self.current += 1;
        true
    }

    fn advance(&mut self) -> u16 {
        let c = self.input[self.current];
        self.current += 1;
        c
    }

    fn peek(&self) -> u16 {
        if self.is_at_end() { 0 } else { self.input[self.current] }
    }

    fn add_token(&mut self, ty: TokenType) {
        self.tokens.push(Token::plain(ty, self.start));
    }

    fn lexeme_between(&self, start: usize, end: usize) -> String {
        String::from_utf16_lossy(&self.input[start..end])
    }

    fn error(&mut self, additional: Option<String>) {
        let offset = self.start;
        let lexeme = self.lexeme_between(self.start, self.current);
        self.errors.push(LexingError { offset, lexeme: lexeme.clone(), additional_info: additional });
        self.tokens.push(Token::with_lexeme(TokenType::Error, offset, lexeme));
    }

    fn string(&mut self) {
        if let Some(found) = self.string_re.exec(&self.input, self.start) {
            self.current = self.start + (found.end() - found.start());
            let lexeme = self.lexeme_between(self.start, self.current);
            match keyword(&lexeme) {
                Some(ty) => self.add_token(ty),
                None => self.tokens.push(Token::with_lexeme(TokenType::Str, self.start, lexeme)),
            }
        }
    }

    /// Captura o lexema sem as aspas simples da abertura e do fechamento.
    fn quoted_string(&mut self) {
        while self.peek() != u('\'') && !self.is_at_end() {
            self.advance();
        }
        if self.is_at_end() {
            self.error(Some(hint_did_you_forget_to_open_or_close_quote()));
            return;
        }
        // Consome a aspa de fechamento.
        self.advance();
        let lexeme = self.lexeme_between(self.start + 1, self.current - 1);
        self.tokens.push(Token::with_lexeme(TokenType::QuotedStr, self.start + 1, lexeme));
    }

    /// Lê uma regex `/.../[igsmyu]*`, no estilo do scanner do TypeScript. As barras dentro dela precisam
    /// vir escapadas.
    fn regex(&mut self) {
        let mut p = self.current;
        let mut in_escape = false;
        let mut in_character_class = false;
        loop {
            if p >= self.input.len() {
                self.current = p;
                self.error(Some(hint_did_you_forget_to_escape_slash()));
                return;
            }
            let ch = self.input[p];
            if in_escape {
                in_escape = false;
            } else if ch == u('/') && !in_character_class {
                p += 1;
                break;
            } else if ch == u('[') {
                in_character_class = true;
            } else if ch == u('\\') {
                in_escape = true;
            } else if ch == u(']') {
                in_character_class = false;
            }
            p += 1;
        }
        while p < self.input.len() && is_regex_flag(self.input[p]) {
            p += 1;
        }
        self.current = p;
        let lexeme = self.lexeme_between(self.start, self.current);
        self.tokens.push(Token::with_lexeme(TokenType::RegexStr, self.start, lexeme));
    }

    fn is_at_end(&self) -> bool {
        self.current >= self.input.len()
    }
}
