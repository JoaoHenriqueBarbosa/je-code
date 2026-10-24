//! Porte de `vs/workbench/services/themes/common/textMateScopeMatcher.ts`.
//!
//! Interpreta o seletor de escopo TextMate (`L:`/`R:`, negação, parênteses, `|` e `,`) e devolve
//! os comparadores com a prioridade de cada um.

use std::rc::Rc;

/// `Matcher<T>`: devolve a pontuação (negativa quando não casa).
pub type Matcher<T> = Box<dyn Fn(&T) -> i32>;

/// `MatcherWithPriority<T>`: o comparador e a prioridade (`-1`, `0` ou `1`).
pub struct MatcherWithPriority<T> {
    pub matcher: Matcher<T>,
    pub priority: i8,
}

/// `matchesName`: pontua os nomes de escopo contra a entrada do comparador.
pub type MatchesName<T> = Rc<dyn Fn(&[String], &T) -> i32>;

/// `createMatchers`: acrescenta a `results` um comparador por alternativa do seletor.
pub fn create_matchers<T: 'static>(
    selector: &str,
    matches_name: MatchesName<T>,
    results: &mut Vec<MatcherWithPriority<T>>,
) {
    let mut parser = Parser {
        tokenizer: Tokenizer::new(selector),
        token: None,
        matches_name,
    };
    parser.token = parser.tokenizer.next();
    while let Some(current) = parser.token.clone() {
        let mut priority: i8 = 0;
        if current.chars().count() == 2 && current.ends_with(':') {
            match current.chars().next() {
                Some('R') => priority = 1,
                Some('L') => priority = -1,
                _ => {}
            }
            parser.token = parser.tokenizer.next();
        }
        if let Some(matcher) = parser.parse_conjunction() {
            results.push(MatcherWithPriority { matcher, priority });
        }
        if parser.token.as_deref() != Some(",") {
            break;
        }
        parser.token = parser.tokenizer.next();
    }
}

struct Parser<T> {
    tokenizer: Tokenizer,
    token: Option<String>,
    matches_name: MatchesName<T>,
}

impl<T: 'static> Parser<T> {
    fn parse_operand(&mut self) -> Option<Matcher<T>> {
        if self.token.as_deref() == Some("-") {
            self.token = self.tokenizer.next();
            let expression_to_negate = self.parse_operand()?;
            return Some(Box::new(move |matcher_input| {
                let score = expression_to_negate(matcher_input);
                if score < 0 {
                    0
                } else {
                    -1
                }
            }));
        }
        if self.token.as_deref() == Some("(") {
            self.token = self.tokenizer.next();
            let expression_in_parents = self.parse_inner_expression();
            if self.token.as_deref() == Some(")") {
                self.token = self.tokenizer.next();
            }
            return expression_in_parents;
        }
        if is_identifier(self.token.as_deref()) {
            let mut identifiers: Vec<String> = Vec::new();
            while is_identifier(self.token.as_deref()) {
                identifiers.push(self.token.take().unwrap_or_default());
                self.token = self.tokenizer.next();
            }
            let matches_name = Rc::clone(&self.matches_name);
            return Some(Box::new(move |matcher_input| {
                matches_name(&identifiers, matcher_input)
            }));
        }
        None
    }

    fn parse_conjunction(&mut self) -> Option<Matcher<T>> {
        let mut matcher = self.parse_operand()?;
        let mut matchers: Vec<Matcher<T>> = Vec::new();
        loop {
            matchers.push(matcher);
            match self.parse_operand() {
                Some(next) => matcher = next,
                None => break,
            }
        }
        Some(Box::new(move |matcher_input| {
            // and
            let mut min = matchers[0](matcher_input);
            let mut i = 1;
            while min >= 0 && i < matchers.len() {
                min = min.min(matchers[i](matcher_input));
                i += 1;
            }
            min
        }))
    }

    fn parse_inner_expression(&mut self) -> Option<Matcher<T>> {
        let mut matcher = self.parse_conjunction()?;
        let mut matchers: Vec<Matcher<T>> = Vec::new();
        loop {
            matchers.push(matcher);
            if matches!(self.token.as_deref(), Some("|") | Some(",")) {
                loop {
                    self.token = self.tokenizer.next();
                    // ignora vírgulas e barras subsequentes
                    if !matches!(self.token.as_deref(), Some("|") | Some(",")) {
                        break;
                    }
                }
            } else {
                break;
            }
            match self.parse_conjunction() {
                Some(next) => matcher = next,
                None => break,
            }
        }
        Some(Box::new(move |matcher_input| {
            // or
            let mut max = matchers[0](matcher_input);
            for matcher in &matchers[1..] {
                max = max.max(matcher(matcher_input));
            }
            max
        }))
    }
}

/// `isIdentifier`: `!!token && !!token.match(/[\w\.:]+/)` (casa em qualquer posição).
fn is_identifier(token: Option<&str>) -> bool {
    token.is_some_and(|token| token.chars().any(is_identifier_start))
}

/// `[\w\.:]`: `\w` do JavaScript é só ASCII.
fn is_identifier_start(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == ':'
}

/// O tokenizador de `newTokenizer`: `/([LR]:|[\w\.:][\w\.:\-]*|[\,\|\-\(\)])/g`.
struct Tokenizer {
    chars: Vec<char>,
    pos: usize,
}

impl Tokenizer {
    fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    fn next(&mut self) -> Option<String> {
        while self.pos < self.chars.len() {
            let c = self.chars[self.pos];
            if (c == 'L' || c == 'R') && self.chars.get(self.pos + 1) == Some(&':') {
                let res: String = self.chars[self.pos..self.pos + 2].iter().collect();
                self.pos += 2;
                return Some(res);
            }
            if is_identifier_start(c) {
                let start = self.pos;
                self.pos += 1;
                while self.pos < self.chars.len()
                    && (is_identifier_start(self.chars[self.pos]) || self.chars[self.pos] == '-')
                {
                    self.pos += 1;
                }
                return Some(self.chars[start..self.pos].iter().collect());
            }
            if matches!(c, ',' | '|' | '-' | '(' | ')') {
                self.pos += 1;
                return Some(c.to_string());
            }
            self.pos += 1;
        }
        None
    }
}
