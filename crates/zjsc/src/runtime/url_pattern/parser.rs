//! `URLPatternParser.cpp` (WebCore do bun 1.4.2): https://urlpattern.spec.whatwg.org/#parsing-patterns, a geração do
//! regex e a da string de padrão.

use super::canonical::{call_encoding_callback, EncodingCallbackType};
use super::tokenizer::{tokenize, Token, TokenType, TokenizePolicy};
use super::{units, Units};
use crate::wtf::unicode::{is_id_continue, is_id_start};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PartType {
    FixedText,
    Regexp,
    SegmentWildcard,
    FullWildcard,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Modifier {
    None,
    Optional,
    ZeroOrMore,
    OneOrMore,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum IsFirst {
    No,
    Yes,
}

#[derive(Clone, Debug)]
pub(crate) struct Part {
    pub(crate) kind: PartType,
    pub(crate) value: Units,
    pub(crate) modifier: Modifier,
    pub(crate) name: Units,
    pub(crate) prefix: Units,
    pub(crate) suffix: Units,
}

#[derive(Clone, Default)]
pub(crate) struct URLPatternStringOptions {
    pub(crate) delimiter_codepoint: Units,
    pub(crate) prefix_codepoint: Units,
    pub(crate) ignore_case: bool,
}

struct URLPatternParser {
    token_list: Vec<Token>,
    part_list: Vec<Part>,
    callback_type: EncodingCallbackType,
    segment_wildcard_regexp: Units,
    pending_fixed_value: Units,
    index: usize,
    next_numeric_name: i32,
}

impl URLPatternParser {
    fn perform_parse(&mut self, options: &URLPatternStringOptions) -> Result<(), String> {
        while self.index < self.token_list.len() {
            let char_token = self.try_to_consume_token(TokenType::Char);
            let mut name_token = self.try_to_consume_token(TokenType::Name);
            let mut regex_or_wildcard_token = self.try_to_consume_regex_or_wildcard_token(&name_token);
            if !name_token.is_null() || !regex_or_wildcard_token.is_null() {
                let mut prefix = Units::new();
                if !char_token.is_null() {
                    prefix = char_token.value.clone();
                }
                if !prefix.is_empty() && prefix != options.prefix_codepoint {
                    self.pending_fixed_value.extend(std::mem::take(&mut prefix));
                }
                self.maybe_add_part_from_pending_fixed_value()?;
                let modifier_token = self.try_to_consume_modifier_token();
                self.add_part(prefix, &name_token, &regex_or_wildcard_token, Units::new(), &modifier_token)?;
                continue;
            }
            let mut fixed_token = char_token;
            if fixed_token.is_null() {
                fixed_token = self.try_to_consume_token(TokenType::EscapedChar);
            }
            if !fixed_token.is_null() {
                self.pending_fixed_value.extend(fixed_token.value);
                continue;
            }
            let open_token = self.try_to_consume_token(TokenType::Open);
            if !open_token.is_null() {
                let prefix = self.consume_text();
                name_token = self.try_to_consume_token(TokenType::Name);
                regex_or_wildcard_token = self.try_to_consume_regex_or_wildcard_token(&name_token);
                let suffix = self.consume_text();
                self.consume_required_token(TokenType::Close)?;
                let modifier_token = self.try_to_consume_modifier_token();
                self.add_part(prefix, &name_token, &regex_or_wildcard_token, suffix, &modifier_token)?;
                continue;
            }
            self.maybe_add_part_from_pending_fixed_value()?;
            self.consume_required_token(TokenType::End)?;
        }
        Ok(())
    }

    /// https://urlpattern.spec.whatwg.org/#try-to-consume-a-token
    fn try_to_consume_token(&mut self, kind: TokenType) -> Token {
        match self.token_list.get(self.index) {
            Some(next) if next.kind == kind => {
                self.index += 1;
                next.clone()
            }
            _ => Token::null(),
        }
    }

    /// https://urlpattern.spec.whatwg.org/#try-to-consume-a-regexp-or-wildcard-token
    fn try_to_consume_regex_or_wildcard_token(&mut self, token: &Token) -> Token {
        let result = self.try_to_consume_token(TokenType::Regexp);
        if result.is_null() && token.is_null() {
            return self.try_to_consume_token(TokenType::Asterisk);
        }
        result
    }

    /// https://urlpattern.spec.whatwg.org/#try-to-consume-a-modifier-token
    fn try_to_consume_modifier_token(&mut self) -> Token {
        let token = self.try_to_consume_token(TokenType::OtherModifier);
        if !token.is_null() {
            return token;
        }
        self.try_to_consume_token(TokenType::Asterisk)
    }

    /// https://urlpattern.spec.whatwg.org/#consume-text
    fn consume_text(&mut self) -> Units {
        let mut result = Units::new();
        loop {
            let mut consumed = self.try_to_consume_token(TokenType::Char);
            if consumed.is_null() {
                consumed = self.try_to_consume_token(TokenType::EscapedChar);
            }
            if consumed.is_null() {
                break;
            }
            result.extend(consumed.value);
        }
        result
    }

    /// https://urlpattern.spec.whatwg.org/#consume-a-required-token
    fn consume_required_token(&mut self, kind: TokenType) -> Result<Token, String> {
        let result = self.try_to_consume_token(kind);
        if result.is_null() {
            return Err("Null token was produced when consuming a required token.".into());
        }
        Ok(result)
    }

    /// https://urlpattern.spec.whatwg.org/#maybe-add-a-part-from-the-pending-fixed-value
    fn maybe_add_part_from_pending_fixed_value(&mut self) -> Result<(), String> {
        if self.pending_fixed_value.is_empty() {
            return Ok(());
        }
        let pending = std::mem::take(&mut self.pending_fixed_value);
        let value = call_encoding_callback(self.callback_type, &pending)?;
        self.part_list.push(Part {
            kind: PartType::FixedText,
            value,
            modifier: Modifier::None,
            name: Units::new(),
            prefix: Units::new(),
            suffix: Units::new(),
        });
        Ok(())
    }

    /// https://urlpattern.spec.whatwg.org/#add-a-part
    fn add_part(&mut self, prefix: Units, name_token: &Token, regexp_or_wildcard_token: &Token, suffix: Units, modifier_token: &Token) -> Result<(), String> {
        let mut modifier = Modifier::None;
        if !modifier_token.is_null() {
            modifier = match modifier_token.value.as_slice() {
                [unit] if *unit == '?' as u16 => Modifier::Optional,
                [unit] if *unit == '*' as u16 => Modifier::ZeroOrMore,
                [unit] if *unit == '+' as u16 => Modifier::OneOrMore,
                _ => Modifier::None,
            };
        }
        if name_token.is_null() && regexp_or_wildcard_token.is_null() && modifier == Modifier::None {
            self.pending_fixed_value.extend(prefix);
            return Ok(());
        }
        self.maybe_add_part_from_pending_fixed_value()?;
        if name_token.is_null() && regexp_or_wildcard_token.is_null() {
            debug_assert!(suffix.is_empty());
            if prefix.is_empty() {
                return Ok(());
            }
            let value = call_encoding_callback(self.callback_type, &prefix)?;
            self.part_list.push(Part {
                kind: PartType::FixedText,
                value,
                modifier,
                name: Units::new(),
                prefix: Units::new(),
                suffix: Units::new(),
            });
            return Ok(());
        }
        let mut regex_value = if regexp_or_wildcard_token.is_null() {
            self.segment_wildcard_regexp.clone()
        } else if regexp_or_wildcard_token.kind == TokenType::Asterisk {
            units(".*")
        } else {
            regexp_or_wildcard_token.value.clone()
        };
        let mut kind = PartType::Regexp;
        if regex_value == self.segment_wildcard_regexp {
            kind = PartType::SegmentWildcard;
            regex_value = Units::new();
        } else if regex_value == units(".*") {
            kind = PartType::FullWildcard;
            regex_value = Units::new();
        }
        let mut name = Units::new();
        if !name_token.is_null() {
            name = name_token.value.clone();
        } else if !regexp_or_wildcard_token.is_null() {
            name = units(&self.next_numeric_name.to_string());
            self.next_numeric_name += 1;
        }
        if self.part_list.iter().any(|part| part.name == name) {
            return Err("Duplicate name token produced when adding to parser part list.".into());
        }
        let encoded_prefix = call_encoding_callback(self.callback_type, &prefix)?;
        let encoded_suffix = call_encoding_callback(self.callback_type, &suffix)?;
        self.part_list.push(Part { kind, value: regex_value, modifier, name, prefix: encoded_prefix, suffix: encoded_suffix });
        Ok(())
    }
}

/// https://urlpattern.spec.whatwg.org/#parse-a-pattern-string
pub(crate) fn parse(input: &[u16], options: &URLPatternStringOptions, callback_type: EncodingCallbackType) -> Result<Vec<Part>, String> {
    let token_list = tokenize(input, TokenizePolicy::Strict)?;
    let mut parser = URLPatternParser {
        token_list,
        part_list: Vec::new(),
        callback_type,
        segment_wildcard_regexp: generate_segment_wildcard_regexp(options),
        pending_fixed_value: Units::new(),
        index: 0,
        next_numeric_name: 0,
    };
    parser.perform_parse(options)?;
    Ok(parser.part_list)
}

/// https://urlpattern.spec.whatwg.org/#generate-a-segment-wildcard-regexp
pub(crate) fn generate_segment_wildcard_regexp(options: &URLPatternStringOptions) -> Units {
    let mut result = units("[^");
    result.extend(escape_regex_string(&options.delimiter_codepoint));
    result.extend(units("]+?"));
    result
}

fn escape_with(input: &[u16], special: &[u8]) -> Units {
    let mut result = Units::with_capacity(input.len());
    for &unit in input {
        if unit < 0x80 && special.contains(&(unit as u8)) {
            result.push('\\' as u16);
        }
        result.push(unit);
    }
    result
}

/// https://urlpattern.spec.whatwg.org/#escape-a-regexp-string
pub(crate) fn escape_regex_string(input: &[u16]) -> Units {
    escape_with(input, b".+*?^${}()[]|/\\")
}

/// https://urlpattern.spec.whatwg.org/#escape-a-pattern-string
pub(crate) fn escape_pattern_string(input: &[u16]) -> Units {
    escape_with(input, b"+*?:()\\{}")
}

/// https://urlpattern.spec.whatwg.org/#convert-a-modifier-to-a-string
fn modifier_string(modifier: Modifier) -> &'static str {
    match modifier {
        Modifier::ZeroOrMore => "*",
        Modifier::Optional => "?",
        Modifier::OneOrMore => "+",
        Modifier::None => "",
    }
}

fn push(out: &mut Units, text: &str) {
    out.extend(text.encode_utf16());
}

/// https://urlpattern.spec.whatwg.org/#generate-a-regular-expression-and-name-list
pub(crate) fn generate_regex_and_name_list(part_list: &[Part], options: &URLPatternStringOptions) -> (Units, Vec<Units>) {
    let mut result = units("^");
    let mut name_list = Vec::new();
    for part in part_list {
        if part.kind == PartType::FixedText {
            if part.modifier == Modifier::None {
                result.extend(escape_regex_string(&part.value));
            } else {
                push(&mut result, "(?:");
                result.extend(escape_regex_string(&part.value));
                push(&mut result, ")");
                push(&mut result, modifier_string(part.modifier));
            }
            continue;
        }
        debug_assert!(!part.name.is_empty());
        name_list.push(part.name.clone());
        let regexp_value = match part.kind {
            PartType::SegmentWildcard => generate_segment_wildcard_regexp(options),
            PartType::FullWildcard => units(".*"),
            _ => part.value.clone(),
        };
        if part.prefix.is_empty() && part.suffix.is_empty() {
            if part.modifier == Modifier::None || part.modifier == Modifier::Optional {
                push(&mut result, "(");
                result.extend(&regexp_value);
                push(&mut result, ")");
                push(&mut result, modifier_string(part.modifier));
            } else {
                push(&mut result, "((?:");
                result.extend(&regexp_value);
                push(&mut result, ")");
                push(&mut result, modifier_string(part.modifier));
                push(&mut result, ")");
            }
            continue;
        }
        if part.modifier == Modifier::None || part.modifier == Modifier::Optional {
            push(&mut result, "(?:");
            result.extend(escape_regex_string(&part.prefix));
            push(&mut result, "(");
            result.extend(&regexp_value);
            push(&mut result, ")");
            result.extend(escape_regex_string(&part.suffix));
            push(&mut result, ")");
            push(&mut result, modifier_string(part.modifier));
            continue;
        }
        push(&mut result, "(?:");
        result.extend(escape_regex_string(&part.prefix));
        push(&mut result, "((?:");
        result.extend(&regexp_value);
        push(&mut result, ")(?:");
        result.extend(escape_regex_string(&part.suffix));
        result.extend(escape_regex_string(&part.prefix));
        push(&mut result, "(?:");
        result.extend(&regexp_value);
        push(&mut result, "))*)");
        result.extend(escape_regex_string(&part.suffix));
        push(&mut result, ")");
        if part.modifier == Modifier::ZeroOrMore {
            push(&mut result, "?");
        }
    }
    push(&mut result, "$");
    (result, name_list)
}

/// O primeiro ponto de código de `text` (`*codePoints().begin()`): o par substituto se junta, o solto fica como está.
fn first_code_point(text: &[u16]) -> u32 {
    let lead = text[0] as u32;
    if (0xD800..=0xDBFF).contains(&lead) {
        if let Some(&trail) = text.get(1) {
            let trail = trail as u32;
            if (0xDC00..=0xDFFF).contains(&trail) {
                return 0x10000 + ((lead - 0xD800) << 10) + (trail - 0xDC00);
            }
        }
    }
    lead
}

fn is_ascii_digit(unit: u16) -> bool {
    (b'0' as u16..=b'9' as u16).contains(&unit)
}

/// https://urlpattern.spec.whatwg.org/#generate-a-pattern-string
pub(crate) fn generate_pattern_string(part_list: &[Part], options: &URLPatternStringOptions) -> Units {
    let mut result = Units::new();
    for (index, part) in part_list.iter().enumerate() {
        let previous_part = index.checked_sub(1).map(|previous| &part_list[previous]);
        let next_part = part_list.get(index + 1);
        if part.kind == PartType::FixedText {
            if part.modifier == Modifier::None {
                result.extend(escape_pattern_string(&part.value));
                continue;
            }
            push(&mut result, "{");
            result.extend(escape_pattern_string(&part.value));
            push(&mut result, "}");
            push(&mut result, modifier_string(part.modifier));
            continue;
        }
        let has_custom_name = !part.name.is_empty() && !is_ascii_digit(part.name[0]);
        let mut needs_grouping = !part.suffix.is_empty() || (!part.prefix.is_empty() && part.prefix != options.prefix_codepoint);
        if !needs_grouping && has_custom_name && part.kind == PartType::SegmentWildcard && part.modifier == Modifier::None {
            if let Some(next) = next_part.filter(|next| next.prefix.is_empty() && next.suffix.is_empty()) {
                if next.kind == PartType::FixedText {
                    if !next.value.is_empty() {
                        needs_grouping = is_valid_name_codepoint(first_code_point(&next.value), IsFirst::No);
                    }
                } else {
                    needs_grouping = !next.name.is_empty() && is_ascii_digit(next.name[0]);
                }
            }
        }
        if !needs_grouping && part.prefix.is_empty() {
            if let Some(previous) = previous_part.filter(|previous| previous.kind == PartType::FixedText && !previous.value.is_empty()) {
                // `codePoints().codePointAt(length - 1)` com o cast para `char16_t`: a última unidade do texto.
                let last = previous.value[previous.value.len() - 1];
                if options.prefix_codepoint.len() == 1 && options.prefix_codepoint[0] == last {
                    needs_grouping = true;
                }
            }
        }
        debug_assert!(!part.name.is_empty());
        if needs_grouping {
            push(&mut result, "{");
        }
        result.extend(escape_pattern_string(&part.prefix));
        if has_custom_name {
            push(&mut result, ":");
            result.extend(&part.name);
        }
        match part.kind {
            PartType::Regexp => {
                push(&mut result, "(");
                result.extend(&part.value);
                push(&mut result, ")");
            }
            PartType::SegmentWildcard if !has_custom_name => {
                push(&mut result, "(");
                result.extend(generate_segment_wildcard_regexp(options));
                push(&mut result, ")");
            }
            PartType::FullWildcard => {
                let plain = previous_part.is_none_or(|previous| previous.kind == PartType::FixedText || previous.modifier != Modifier::None);
                if !has_custom_name && (plain || needs_grouping || !part.prefix.is_empty()) {
                    push(&mut result, "*");
                } else {
                    push(&mut result, "(.*)");
                }
            }
            _ => {}
        }
        if part.kind == PartType::SegmentWildcard
            && has_custom_name
            && !part.suffix.is_empty()
            && is_valid_name_codepoint(first_code_point(&part.suffix), IsFirst::Yes)
        {
            push(&mut result, "\\");
        }
        result.extend(escape_pattern_string(&part.suffix));
        if needs_grouping {
            push(&mut result, "}");
        }
        push(&mut result, modifier_string(part.modifier));
    }
    result
}

/// https://urlpattern.spec.whatwg.org/#is-a-valid-name-code-point
pub(crate) fn is_valid_name_codepoint(codepoint: u32, first: IsFirst) -> bool {
    let dollar_or_underscore = codepoint == '_' as u32 || codepoint == '$' as u32;
    match first {
        IsFirst::Yes => is_id_start(codepoint) || dollar_or_underscore,
        IsFirst::No => is_id_continue(codepoint) || dollar_or_underscore || codepoint == 0x200C || codepoint == 0x200D,
    }
}
