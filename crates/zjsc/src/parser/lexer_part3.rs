// Parte 3 de `parser/Lexer.cpp` (linhas 1675 a 2140 e 3142 até o fim): `parseHex`, `parseBinary`,
// `parseOctal`, `parseDecimal`, `parseNumberAfterDecimalPoint`, `parseNumberAfterExponentIndicator`,
// `parseMultilineComment`, `parseCommentDirective` (e o valor), `consume`, `nextTokenIsColon`,
// `fillTokenInfo`, `scanSingleLineComment`, `orCharacter`, `scanRegExp`, `scanTemplateString` e
// `clear`.
//
// Incluído por `include!` no fim de `lexer.rs` (depois de `lexer_part2.rs`), então não há `use`
// nem atributo de módulo aqui. As funções de busca vetorial (`SIMD::find`, inclusive a
// `parseCommentDirectiveValueSIMD`) viram a varredura escalar com o mesmo predicado
// (`simd_find`, da parte 2): o primeiro índice que casa é o mesmo.

/// `orCharacter<T>(char16_t&, char16_t)`: a especialização de `Latin1Character` não faz nada, a de
/// `char16_t` acumula os bits.
#[inline]
fn or_character<C: crate::wtf::text::string_impl::CharType>(or_accumulator: &mut u16, character: C) {
    if C::SIZE != 1 {
        *or_accumulator |= character.to_u16();
    }
}

/// `makeString(...)` aplicado a pedaços de texto: a concatenação é 8 bits quando todos os pedaços
/// são 8 bits, 16 bits caso contrário (como o `StringTypeAdapter` da WTF).
fn make_string_from_parts(parts: &[&crate::wtf::text::wtf_string::String]) -> crate::wtf::text::wtf_string::String {
    let all_8bit = parts.iter().all(|part| part.is_8bit());
    if all_8bit {
        let mut units: Vec<u8> = Vec::new();
        for part in parts {
            units.extend_from_slice(part.span8());
        }
        return crate::wtf::text::wtf_string::String::from_latin1(&units);
    }
    let mut units: Vec<u16> = Vec::new();
    for part in parts {
        if part.is_8bit() {
            units.extend(part.span8().iter().map(|&c| c as u16));
        } else {
            units.extend_from_slice(part.span16());
        }
    }
    crate::wtf::text::wtf_string::String::from_utf16(&units)
}

impl<T: crate::wtf::text::string_impl::CharType> Lexer<T> {
    /// `peek(offset)` já como `u32`, para comparar com literais de caractere.
    #[inline(always)]
    fn peek_u32(&self, offset: i32) -> u32 {
        self.peek(offset).into()
    }

    /// `makeIdentifier(m_buffer8.span())`. O buffer sai do `self` durante a chamada porque a criação
    /// do identificador toma `&mut self`; volta intacto (o chamador decide o `shrink`).
    fn make_identifier_from_buffer8(&mut self) -> Identifier {
        let buffer = std::mem::take(&mut self.buffer8);
        let ident = self.make_identifier(&buffer[..]);
        self.buffer8 = buffer;
        ident
    }

    /// `parseHex()`.
    #[inline(always)]
    fn parse_hex(&mut self) -> Option<NumberParseResult> {
        debug_assert!(is_ascii_hex_digit(self.cur()));

        // Optimization: most hexadecimal values fit into 4 bytes.
        let mut hex_value: u32 = 0;
        let mut maximum_digits: i32 = 7;

        loop {
            if self.cur() == '_' as u32 {
                if !is_ascii_hex_digit(self.peek_u32(1)) {
                    return None;
                }

                self.shift();
            }

            hex_value = (hex_value << 4).wrapping_add(to_ascii_hex_value(self.cur()) as u32);
            self.shift();
            maximum_digits -= 1;
            if !(is_ascii_hex_digit_or_separator(self.cur()) && maximum_digits >= 0) {
                break;
            }
        }

        if maximum_digits >= 0 && self.cur() != 'n' as u32 {
            return Some(NumberParseResult::Double(hex_value as f64));
        }

        // No more place in the hexValue buffer.
        // The values are shifted out and placed into the m_buffer8 vector.
        for _ in 0..8 {
            let digit = (hex_value >> 28) as i32;
            if digit < 10 {
                self.record8(digit + '0' as i32);
            } else {
                self.record8(digit - 10 + 'a' as i32);
            }
            hex_value <<= 4;
        }

        while is_ascii_hex_digit_or_separator(self.cur()) {
            if self.cur() == '_' as u32 {
                if !is_ascii_hex_digit(self.peek_u32(1)) {
                    return None;
                }

                self.shift();
            }

            self.record8(self.cur() as i32);
            self.shift();
        }

        if self.cur() == 'n' as u32 {
            return Some(NumberParseResult::Identifier(self.make_identifier_from_buffer8()));
        }

        Some(NumberParseResult::Double(crate::runtime::parse_int::parse_int_overflow(&self.buffer8[..], 16)))
    }

    /// `parseBinary()`.
    #[inline(always)]
    fn parse_binary(&mut self) -> Option<NumberParseResult> {
        debug_assert!(is_ascii_binary_digit(self.cur()));

        // Optimization: most binary values fit into 4 bytes.
        let mut binary_value: u32 = 0;
        const MAXIMUM_DIGITS: usize = 32;
        let mut digit: i32 = MAXIMUM_DIGITS as i32 - 1;
        // Temporary buffer for the digits. Makes easier
        // to reconstruct the input characters when needed.
        let mut digits = [0u8; MAXIMUM_DIGITS];

        loop {
            if self.cur() == '_' as u32 {
                if !is_ascii_binary_digit(self.peek_u32(1)) {
                    return None;
                }

                self.shift();
            }

            binary_value = (binary_value << 1).wrapping_add(self.cur().wrapping_sub('0' as u32));
            digits[digit as usize] = self.cur() as u8;
            self.shift();
            digit -= 1;
            if !(is_ascii_binary_digit_or_separator(self.cur()) && digit >= 0) {
                break;
            }
        }

        if !is_ascii_digit_or_separator(self.cur()) && digit >= 0 && self.cur() != 'n' as u32 {
            return Some(NumberParseResult::Double(binary_value as f64));
        }

        let mut i = MAXIMUM_DIGITS as i32 - 1;
        while i > digit {
            self.record8(digits[i as usize] as i32);
            i -= 1;
        }

        while is_ascii_binary_digit_or_separator(self.cur()) {
            if self.cur() == '_' as u32 {
                if !is_ascii_binary_digit(self.peek_u32(1)) {
                    return None;
                }

                self.shift();
            }

            self.record8(self.cur() as i32);
            self.shift();
        }

        if self.cur() == 'n' as u32 {
            return Some(NumberParseResult::Identifier(self.make_identifier_from_buffer8()));
        }

        if is_ascii_digit(self.cur()) {
            return None;
        }

        Some(NumberParseResult::Double(crate::runtime::parse_int::parse_int_overflow(&self.buffer8[..], 2)))
    }

    /// `parseOctal()`.
    #[inline(always)]
    fn parse_octal(&mut self) -> Option<NumberParseResult> {
        debug_assert!(is_ascii_octal_digit(self.cur()));
        debug_assert!(self.buffer8.is_empty() || (self.buffer8.len() == 1 && self.buffer8[0] == b'0'));
        let is_legacy_literal = !self.buffer8.is_empty();

        // Optimization: most octal values fit into 4 bytes.
        let mut octal_value: u32 = 0;
        const MAXIMUM_DIGITS: usize = 10;
        let mut digit: i32 = MAXIMUM_DIGITS as i32 - 1;
        // Temporary buffer for the digits. Makes easier
        // to reconstruct the input characters when needed.
        let mut digits = [0u8; MAXIMUM_DIGITS];

        loop {
            if self.cur() == '_' as u32 {
                if !is_ascii_octal_digit(self.peek_u32(1)) || is_legacy_literal {
                    return None;
                }

                self.shift();
            }

            octal_value = octal_value.wrapping_mul(8).wrapping_add(self.cur().wrapping_sub('0' as u32));
            digits[digit as usize] = self.cur() as u8;
            self.shift();
            digit -= 1;
            if !(is_ascii_octal_digit_or_separator(self.cur()) && digit >= 0) {
                break;
            }
        }

        if !is_ascii_digit_or_separator(self.cur()) && digit >= 0 && self.cur() != 'n' as u32 {
            return Some(NumberParseResult::Double(octal_value as f64));
        }

        let mut i = MAXIMUM_DIGITS as i32 - 1;
        while i > digit {
            self.record8(digits[i as usize] as i32);
            i -= 1;
        }

        while is_ascii_octal_digit_or_separator(self.cur()) {
            if self.cur() == '_' as u32 {
                if !is_ascii_octal_digit(self.peek_u32(1)) || is_legacy_literal {
                    return None;
                }

                self.shift();
            }

            self.record8(self.cur() as i32);
            self.shift();
        }

        if self.cur() == 'n' as u32 && !is_legacy_literal {
            return Some(NumberParseResult::Identifier(self.make_identifier_from_buffer8()));
        }

        if is_ascii_digit(self.cur()) {
            return None;
        }

        Some(NumberParseResult::Double(crate::runtime::parse_int::parse_int_overflow(&self.buffer8[..], 8)))
    }

    /// `parseDecimal()`.
    #[inline(always)]
    fn parse_decimal(&mut self) -> Option<NumberParseResult> {
        debug_assert!(is_ascii_digit(self.cur()) || !self.buffer8.is_empty());
        let is_legacy_literal = !self.buffer8.is_empty() && is_ascii_digit_or_separator(self.cur());

        // Optimization: most decimal values fit into 4 bytes.
        let mut decimal_value: u32 = 0;

        // Since parseOctal may be executed before parseDecimal,
        // the m_buffer8 may hold ascii digits.
        if self.buffer8.is_empty() {
            const MAXIMUM_DIGITS: usize = 10;
            let mut digit: i32 = MAXIMUM_DIGITS as i32 - 1;
            // Temporary buffer for the digits. Makes easier
            // to reconstruct the input characters when needed.
            let mut digits = [0u8; MAXIMUM_DIGITS];

            loop {
                if self.cur() == '_' as u32 {
                    if !is_ascii_digit(self.peek_u32(1)) || is_legacy_literal {
                        return None;
                    }

                    self.shift();
                }

                decimal_value = decimal_value.wrapping_mul(10).wrapping_add(self.cur().wrapping_sub('0' as u32));
                digits[digit as usize] = self.cur() as u8;
                self.shift();
                digit -= 1;
                if !(is_ascii_digit_or_separator(self.cur()) && digit >= 0) {
                    break;
                }
            }

            if digit >= 0
                && self.cur() != '.' as u32
                && !crate::wtf::ascii_ctype::is_ascii_alpha_caseless_equal(self.cur(), b'e')
                && self.cur() != 'n' as u32
            {
                return Some(NumberParseResult::Double(decimal_value as f64));
            }

            let mut i = MAXIMUM_DIGITS as i32 - 1;
            while i > digit {
                self.record8(digits[i as usize] as i32);
                i -= 1;
            }
        }

        while is_ascii_digit_or_separator(self.cur()) {
            if self.cur() == '_' as u32 {
                if !is_ascii_digit(self.peek_u32(1)) || is_legacy_literal {
                    return None;
                }

                self.shift();
            }

            self.record8(self.cur() as i32);
            self.shift();
        }

        if self.cur() == 'n' as u32 && !is_legacy_literal {
            return Some(NumberParseResult::Identifier(self.make_identifier_from_buffer8()));
        }

        None
    }

    /// `parseNumberAfterDecimalPoint()`.
    #[inline(always)]
    fn parse_number_after_decimal_point(&mut self) -> bool {
        debug_assert!(is_ascii_digit(self.cur()));
        self.record8('.' as i32);

        loop {
            if self.cur() == '_' as u32 {
                if !is_ascii_digit(self.peek_u32(1)) {
                    return false;
                }

                self.shift();
            }

            self.record8(self.cur() as i32);
            self.shift();
            if !is_ascii_digit_or_separator(self.cur()) {
                break;
            }
        }

        true
    }

    /// `parseNumberAfterExponentIndicator()`.
    #[inline(always)]
    fn parse_number_after_exponent_indicator(&mut self) -> bool {
        self.record8('e' as i32);
        self.shift();
        if self.cur() == '+' as u32 || self.cur() == '-' as u32 {
            self.record8(self.cur() as i32);
            self.shift();
        }

        if !is_ascii_digit(self.cur()) {
            return false;
        }

        loop {
            if self.cur() == '_' as u32 {
                if !is_ascii_digit(self.peek_u32(1)) {
                    return false;
                }

                self.shift();
            }

            self.record8(self.cur() as i32);
            self.shift();
            if !is_ascii_digit_or_separator(self.cur()) {
                break;
            }
        }

        true
    }

    /// `parseMultilineComment()`.
    #[inline(always)]
    fn parse_multiline_comment(&mut self) -> bool {
        loop {
            while self.cur() == '*' as u32 {
                self.shift();
                if self.cur() == '/' as u32 {
                    self.shift();
                    return true;
                }
            }

            if self.at_end() {
                return false;
            }

            if Self::is_line_terminator(self.current) {
                self.shift_line_terminator();
                self.has_line_terminator_before_token = true;
            } else {
                self.shift();
            }
        }
    }

    /// `parseCommentDirective()`.
    #[inline(always)]
    fn parse_comment_directive(&mut self) {
        // sourceURL and sourceMappingURL directives.
        if !self.consume(b"source") {
            return;
        }

        if self.consume(b"URL=") {
            self.source_url_directive = self.parse_comment_directive_value();
            return;
        }

        if self.consume(b"MappingURL=") {
            self.source_mapping_url_directive = self.parse_comment_directive_value();
        }
    }

    /// `parseCommentDirectiveValue()`. Para `Latin1Character` o C++ procura o fim do valor com
    /// `parseCommentDirectiveValueSIMD` (espaço em branco, terminador de linha ou aspas); o predicado
    /// escalar dele é o usado aqui.
    #[inline(always)]
    fn parse_comment_directive_value(&mut self) -> WtfString {
        self.skip_whitespace();
        let mut merged_character_bits: u16 = 0;
        let string_start = self.current_source_ptr();
        let source_owner = self.source_rc();
        let source = T::span_of(&source_owner);
        if T::SIZE == 1 {
            let found = string_start
                + simd_find(&source[string_start..self.code_end], |c: T| {
                    let c = c.to_u16() as u8;
                    Lexer::<u8>::is_white_space(c)
                        || Lexer::<u8>::is_line_terminator(c)
                        || c == b'"'
                        || c == b'\''
                });
            self.code = found;
            if self.code < self.code_end {
                self.current = source[self.code];
            } else {
                self.current = T::from_u16(0);
            }
        } else {
            while !Self::is_white_space(self.current)
                && !Self::is_line_terminator(self.current)
                && self.cur() != '"' as u32
                && self.cur() != '\'' as u32
                && !self.at_end()
            {
                merged_character_bits |= self.current.to_u16();
                self.shift();
            }
        }
        let comment_directive = &source[string_start..self.current_source_ptr()];

        self.skip_whitespace();
        if !Self::is_line_terminator(self.current) && !self.at_end() {
            return WtfString::default();
        }

        if T::SIZE != 1 && is_latin1(merged_character_bits as u32) {
            let units: Vec<u16> = comment_directive.iter().map(|&c| c.to_u16()).collect();
            return WtfString::make_8bit(&units);
        }
        WtfString::from(T::create(comment_directive))
    }

    /// `consume(const char (&input)[length])`: `input` é o literal sem o byte nulo final.
    #[inline(always)]
    fn consume(&mut self, input: &[u8]) -> bool {
        let length_to_check = input.len();

        let mut i = 0;
        while i < length_to_check && self.cur() == input[i] as u32 {
            self.shift();
            i += 1;
        }

        i == length_to_check
    }

    /// `nextTokenIsColon()`.
    pub fn next_token_is_colon(&self) -> bool {
        let mut code = self.code;
        while code < self.code_end && (Self::is_white_space(self.char_at(code)) || Self::is_line_terminator(self.char_at(code))) {
            code += 1;
        }

        code < self.code_end && Into::<u32>::into(self.char_at(code)) == ':' as u32
    }

    /// `fillTokenInfo(JSToken*, JSTextPosition)`.
    fn fill_token_info(&self, token_record: &mut JSToken, end_position: JSTextPosition) {
        token_record.end_position = end_position;
    }

    /// `scanSingleLineComment(JSToken*, bool)`.
    #[inline(never)]
    fn scan_single_line_comment(&mut self, token_record: &mut JSToken, check_for_directives: bool) -> Option<JSTokenType> {
        if check_for_directives {
            // Script comment directives like "//# sourceURL=test.js".
            if (self.cur() == '#' as u32 || self.cur() == '@' as u32) && Self::is_white_space(self.peek(1)) {
                self.shift();
                self.shift();
                self.parse_comment_directive();
            }
        }

        let end_position = self.current_position();

        let source_owner = self.source_rc();
        let source = T::span_of(&source_owner);
        let start = self.current_source_ptr();
        self.code = start + simd_find(&source[start..self.code_end], |c: T| Self::is_line_terminator(c));
        if self.code == self.code_end {
            self.current = T::from_u16(0);
            self.fill_token_info(token_record, end_position);
            return Some(EOFTOK);
        }

        self.current = source[self.code];
        self.shift_line_terminator();
        self.at_line_start = true;
        self.has_line_terminator_before_token = true;
        // The caller restarts the token scan unless a restricted keyword needs the
        // automatic semicolon that the line terminator implies.
        if !is_restr_keyword(token_record.type_) {
            return None;
        }

        self.fill_token_info(token_record, end_position);
        Some(SEMICOLON)
    }

    /// `scanRegExp(JSToken*, char16_t)`.
    pub fn scan_reg_exp(&mut self, token_record: &mut JSToken, pattern_prefix: u16) -> JSTokenType {
        debug_assert!(self.buffer16.is_empty());

        let mut last_was_escape = false;
        let mut in_brackets = false;
        let mut characters_ored_together: u16 = 0;

        if pattern_prefix != 0 {
            debug_assert!(!Lexer::<u16>::is_line_terminator(pattern_prefix));
            debug_assert!(pattern_prefix != '/' as u16);
            debug_assert!(pattern_prefix != '[' as u16);
            self.record16(pattern_prefix as u32);
        }

        loop {
            if Self::is_line_terminator(self.current) || self.at_end() {
                self.buffer16.clear();
                let token = UNTERMINATED_REGEXP_LITERAL_ERRORTOK;
                self.fill_token_info(token_record, self.current_position());
                self.error = true;
                let token_text = self.get_token(token_record);
                self.lex_error_message = make_string_from_parts(&[
                    &lex_message("Unterminated regular expression literal '"),
                    &token_text,
                    &lex_message("'"),
                ]);
                return token;
            }

            let prev = self.current;
            let prev_code: u32 = prev.into();

            self.shift();

            if prev_code == '/' as u32 && !last_was_escape && !in_brackets {
                break;
            }

            self.record16(prev_code);
            or_character::<T>(&mut characters_ored_together, prev);

            if last_was_escape {
                last_was_escape = false;
                continue;
            }

            match prev_code {
                0x5B /* '[' */ => in_brackets = true,
                0x5D /* ']' */ => in_brackets = false,
                0x5C /* '\\' */ => last_was_escape = true,
                _ => {}
            }
        }

        let buffer = std::mem::take(&mut self.buffer16);
        token_record.data.pattern = Some(self.make_right_sized_identifier(&buffer[..], characters_ored_together));
        self.buffer16 = buffer;
        self.buffer16.clear();

        debug_assert!(self.buffer8.is_empty());
        while is_latin1(self.cur()) {
            if !is_ident_part(self.cur()) {
                break;
            }
            self.record8(self.cur() as i32);
            self.shift();
        }

        // Normally this would not be a lex error but dealing with surrogate pairs here is annoying and it's going to be an error anyway...
        if !is_latin1(self.cur()) && !Self::is_white_space(self.current) && !Self::is_line_terminator(self.current) {
            self.buffer8.clear();
            let token = INVALID_IDENTIFIER_UNICODE_ERRORTOK;
            self.fill_token_info(token_record, self.current_position());
            self.error = true;
            let mut code_point = WtfString::from_code_point(self.current_code_point());
            if code_point.is_null() {
                code_point = lex_message("`invalid unicode character`");
            }
            let token_text = self.get_token(token_record);
            self.lex_error_message = make_string_from_parts(&[
                &lex_message("Invalid non-latin character in RexExp literal's flags '"),
                &token_text,
                &code_point,
                &lex_message("'"),
            ]);
            return token;
        }

        token_record.data.flags = Some(self.make_identifier_from_buffer8());
        self.buffer8.clear();

        // Since RegExp always ends with / or flags (IdentifierPart), m_atLineStart always becomes false.
        self.at_line_start = false;

        let token = REGEXP;
        self.fill_token_info(token_record, self.current_position());
        token
    }

    /// `scanTemplateString(JSToken*, RawStringsBuildMode)`.
    pub fn scan_template_string(&mut self, token_record: &mut JSToken, raw_strings_build_mode: RawStringsBuildMode) -> JSTokenType {
        debug_assert!(!self.error);
        debug_assert!(self.buffer16.is_empty());

        // Leading backquote ` (for template head) or closing brace } (for template trailing) are already shifted in the previous token scan.
        // So in this re-scan phase, shift() is not needed here.
        let result = self.parse_template_literal(&mut token_record.data, raw_strings_build_mode);
        let token;
        if result != StringParseResult::StringParsedSuccessfully {
            token = if result == StringParseResult::StringUnterminated {
                UNTERMINATED_TEMPLATE_LITERAL_ERRORTOK
            } else {
                INVALID_TEMPLATE_LITERAL_ERRORTOK
            };
            self.error = true;
        } else {
            token = TEMPLATE;
        }

        // Since TemplateString always ends with ` or }, m_atLineStart always becomes false.
        self.at_line_start = false;
        self.fill_token_info(token_record, self.current_position());
        token
    }

    /// `clear()`: os `swap` com vetores novos liberam a memória dos buffers.
    pub fn clear(&mut self) {
        self.arena = None;

        self.buffer8 = Vec::new();
        self.buffer16 = Vec::new();
        self.buffer_for_raw_template_string16 = Vec::new();

        self.is_reparsing_function = false;
    }
}
