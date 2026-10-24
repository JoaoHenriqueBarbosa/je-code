//! `URLPatternConstructorStringParser.cpp` (WebCore do bun 1.4.2):
//! https://urlpattern.spec.whatwg.org/#constructor-string-parsing.
//!
//! Como no C++, o erro de compilar o protocolo dentro do estado `Protocol` é engolido (o `FIXME: Return exceptions`):
//! o estado não muda e a análise segue.

use super::canonical::EncodingCallbackType;
use super::component::URLPatternComponent;
use super::parser::URLPatternStringOptions;
use super::tokenizer::{tokenize, Token, TokenType, TokenizePolicy};
use super::{URLPatternInit, Units};
use crate::runtime::vm::VM;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Init,
    Protocol,
    Authority,
    Username,
    Password,
    Hostname,
    Port,
    Pathname,
    Search,
    Hash,
    Done,
}

struct ConstructorStringParser<'a> {
    vm: &'a VM,
    input: &'a [u16],
    token_list: Vec<Token>,
    result: URLPatternInit,
    component_start: usize,
    token_index: usize,
    token_increment: usize,
    group_depth: usize,
    hostname_ipv6_bracket_depth: i32,
    protocol_matches_special_scheme_flag: bool,
    state: State,
}

impl ConstructorStringParser<'_> {
    /// https://urlpattern.spec.whatwg.org/#rewind
    fn rewind(&mut self) {
        self.token_index = self.component_start;
        self.token_increment = 0;
    }

    /// https://urlpattern.spec.whatwg.org/#get-a-safe-token
    fn safe_token(&self, index: usize) -> &Token {
        self.token_list.get(index).unwrap_or_else(|| self.token_list.last().expect("a lista termina no End"))
    }

    /// https://urlpattern.spec.whatwg.org/#is-a-non-special-pattern-char
    fn is_non_special_pattern_character(&self, index: usize, value: char) -> bool {
        let token = self.safe_token(index);
        token.value.len() == 1
            && token.value[0] == value as u16
            && matches!(token.kind, TokenType::Char | TokenType::EscapedChar | TokenType::InvalidChar)
    }

    /// https://urlpattern.spec.whatwg.org/#is-a-search-prefix
    fn is_search_prefix(&self) -> bool {
        if self.is_non_special_pattern_character(self.token_index, '?') {
            return true;
        }
        if self.token_list[self.token_index].value != [b'?' as u16] {
            return false;
        }
        if self.token_index == 0 {
            return true;
        }
        let previous = self.safe_token(self.token_index - 1);
        !matches!(previous.kind, TokenType::Name | TokenType::Regexp | TokenType::Close | TokenType::Asterisk)
    }

    /// https://urlpattern.spec.whatwg.org/#next-is-authority-slashes
    fn is_authority_slashes_next(&self) -> bool {
        self.is_non_special_pattern_character(self.token_index + 1, '/') && self.is_non_special_pattern_character(self.token_index + 2, '/')
    }

    /// https://urlpattern.spec.whatwg.org/#make-a-component-string
    fn make_component_string(&self) -> Units {
        let token = &self.token_list[self.token_index];
        let start = self.safe_token(self.component_start).index.expect("token não nulo");
        let end = token.index.expect("token não nulo");
        self.input[start..end].to_vec()
    }

    fn set_component(&mut self, state: State, value: Units) {
        let slot = match state {
            State::Protocol => &mut self.result.protocol,
            State::Username => &mut self.result.username,
            State::Password => &mut self.result.password,
            State::Hostname => &mut self.result.hostname,
            State::Port => &mut self.result.port,
            State::Pathname => &mut self.result.pathname,
            State::Search => &mut self.result.search,
            State::Hash => &mut self.result.hash,
            _ => return,
        };
        *slot = Some(value);
    }

    /// https://urlpattern.spec.whatwg.org/#compute-protocol-matches-a-special-scheme-flag
    fn compute_protocol_match_special_scheme_flag(&mut self) -> Result<(), String> {
        let component = URLPatternComponent::compile(
            self.vm,
            &self.make_component_string(),
            EncodingCallbackType::Protocol,
            &URLPatternStringOptions::default(),
        )?;
        self.protocol_matches_special_scheme_flag = component.match_special_scheme_protocol();
        Ok(())
    }

    /// https://urlpattern.spec.whatwg.org/#change-state
    fn change_state(&mut self, new_state: State, skip: usize) {
        if !matches!(self.state, State::Init | State::Authority | State::Done) {
            let component = self.make_component_string();
            self.set_component(self.state, component);
        }
        if self.state != State::Init && new_state != State::Done {
            if matches!(self.state, State::Protocol | State::Authority | State::Username | State::Password)
                && matches!(new_state, State::Port | State::Pathname | State::Search | State::Hash)
                && self.result.hostname.is_none()
            {
                self.result.hostname = Some(Units::new());
            }
            if matches!(self.state, State::Protocol | State::Authority | State::Username | State::Password | State::Hostname | State::Port)
                && matches!(new_state, State::Search | State::Hash)
                && self.result.pathname.is_none()
            {
                self.result.pathname = Some(if self.protocol_matches_special_scheme_flag { vec![b'/' as u16] } else { Units::new() });
            }
            if matches!(
                self.state,
                State::Protocol | State::Authority | State::Username | State::Password | State::Hostname | State::Port | State::Pathname
            ) && new_state == State::Hash
                && self.result.search.is_none()
            {
                self.result.search = Some(Units::new());
            }
        }
        self.state = new_state;
        self.token_index += skip;
        self.component_start = self.token_index;
        self.token_increment = 0;
    }

    fn update_state(&mut self) {
        let index = self.token_index;
        match self.state {
            State::Init => {
                if self.is_non_special_pattern_character(index, ':') {
                    self.rewind();
                    self.state = State::Protocol;
                }
            }
            State::Protocol => {
                if self.is_non_special_pattern_character(index, ':') {
                    if self.compute_protocol_match_special_scheme_flag().is_err() {
                        return;
                    }
                    let mut next_state = State::Pathname;
                    let mut skip = 1;
                    if self.is_authority_slashes_next() {
                        next_state = State::Authority;
                        skip = 3;
                    } else if self.protocol_matches_special_scheme_flag {
                        next_state = State::Authority;
                    }
                    self.change_state(next_state, skip);
                }
            }
            State::Authority => {
                if self.is_non_special_pattern_character(index, '@') {
                    self.rewind();
                    self.state = State::Username;
                } else if self.is_non_special_pattern_character(index, '/')
                    || self.is_search_prefix()
                    || self.is_non_special_pattern_character(index, '#')
                {
                    self.rewind();
                    self.state = State::Hostname;
                }
            }
            State::Username => {
                if self.is_non_special_pattern_character(index, ':') {
                    self.change_state(State::Password, 1);
                } else if self.is_non_special_pattern_character(index, '@') {
                    self.change_state(State::Hostname, 1);
                }
            }
            State::Password => {
                if self.is_non_special_pattern_character(index, '@') {
                    self.change_state(State::Hostname, 1);
                }
            }
            State::Hostname => {
                if self.is_non_special_pattern_character(index, '[') {
                    self.hostname_ipv6_bracket_depth += 1;
                } else if self.is_non_special_pattern_character(index, ']') && self.hostname_ipv6_bracket_depth > 0 {
                    self.hostname_ipv6_bracket_depth -= 1;
                } else if self.is_non_special_pattern_character(index, ':') && self.hostname_ipv6_bracket_depth == 0 {
                    self.change_state(State::Port, 1);
                } else if self.is_non_special_pattern_character(index, '/') {
                    self.change_state(State::Pathname, 0);
                } else if self.is_search_prefix() {
                    self.change_state(State::Search, 1);
                } else if self.is_non_special_pattern_character(index, '#') {
                    self.change_state(State::Hash, 1);
                }
            }
            State::Port => {
                if self.is_non_special_pattern_character(index, '/') {
                    self.change_state(State::Pathname, 0);
                } else if self.is_search_prefix() {
                    self.change_state(State::Search, 1);
                } else if self.is_non_special_pattern_character(index, '#') {
                    self.change_state(State::Hash, 1);
                }
            }
            State::Pathname => {
                if self.is_search_prefix() {
                    self.change_state(State::Search, 1);
                } else if self.is_non_special_pattern_character(index, '#') {
                    self.change_state(State::Hash, 1);
                }
            }
            State::Search => {
                if self.is_non_special_pattern_character(index, '#') {
                    self.change_state(State::Hash, 1);
                }
            }
            State::Hash | State::Done => {}
        }
    }

    fn perform_parse(&mut self) {
        while self.token_index < self.token_list.len() {
            self.token_increment = 1;
            if self.token_list[self.token_index].kind == TokenType::End {
                if self.state == State::Init {
                    self.rewind();
                    if self.is_non_special_pattern_character(self.token_index, '#') {
                        self.change_state(State::Hash, 1);
                    } else if self.is_search_prefix() {
                        self.change_state(State::Search, 1);
                    } else {
                        self.change_state(State::Pathname, 0);
                    }
                    self.token_index += self.token_increment;
                    continue;
                }
                if self.state == State::Authority {
                    self.rewind();
                    self.state = State::Hostname;
                    self.token_index += self.token_increment;
                    continue;
                }
                self.change_state(State::Done, 0);
                break;
            }
            if self.token_list[self.token_index].kind == TokenType::Open {
                self.group_depth += 1;
                self.token_index += 1;
                continue;
            }
            if self.group_depth > 0 {
                if self.token_list[self.token_index].kind == TokenType::Close {
                    self.group_depth -= 1;
                } else {
                    self.token_index += self.token_increment;
                    continue;
                }
            }
            self.update_state();
            self.token_index += self.token_increment;
        }
        if self.result.hostname.is_some() && self.result.port.is_none() {
            self.result.port = Some(Units::new());
        }
    }
}

/// https://urlpattern.spec.whatwg.org/#parse-a-constructor-string
pub(crate) fn parse_constructor_string(vm: &VM, input: &[u16]) -> Result<URLPatternInit, String> {
    let token_list = tokenize(input, TokenizePolicy::Lenient)?;
    let mut parser = ConstructorStringParser {
        vm,
        input,
        token_list,
        result: URLPatternInit::default(),
        component_start: 0,
        token_index: 0,
        token_increment: 1,
        group_depth: 0,
        hostname_ipv6_bracket_depth: 0,
        protocol_matches_special_scheme_flag: false,
        state: State::Init,
    };
    parser.perform_parse();
    Ok(parser.result)
}
