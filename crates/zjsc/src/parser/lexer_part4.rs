// Parte 4 de `parser/Lexer.cpp` (linhas 2141 a 3141): `lexWithoutClearingLineTerminator`.
//
// Incluído por `include!` no fim de `lexer.rs`, então não há `use` nem atributo de módulo no topo;
// os `use` ficam dentro das funções.
//
// Os `goto` do C++ (`start`, `parseIdent`, `returnToken`, `invalidCharacter`, `returnError`) e as
// quedas de `case` (`[[fallthrough]]`) viram um laço `loop` sobre um `enum Label` de estado, com a
// mesma ordem de execução. Cada braço do `match` devolve o próximo rótulo; o `break` do `switch` do
// C++ vira o rótulo `AfterSwitch` (`m_atLineStart = false; goto returnToken`).
//
// O `switch` do C++ tem 256 rótulos de `case`, gerados da tabela `typesOfLatin1Characters`. Aqui os
// caracteres de pontuação e dígitos aparecem pelo código, e os grupos grandes (início de
// identificador Latin1, espaço em branco, inválidos) se decidem pela mesma tabela, que é de onde o
// C++ os gerou. O ramo de `ASSERT_ENABLED` do `case` de espaço em branco não existe.
//
// Três trechos que o C++ repete (os prefixos `0x`/`0b`/`0o`, o fim de literal numérico e a mensagem
// de erro numérica) são métodos à parte, num segundo bloco `impl` ao fim do arquivo, pela regra DRY.

impl<T: CharType> Lexer<T> {
    /// `U16_GET(m_code + ..., 0, 0, m_codeEnd - ..., codePoint)` a partir de `index`: junta o par
    /// substituto quando o caractere é o primeiro de um par válido; senão devolve a unidade crua.
    fn lexer_code_point_at(&self, index: usize) -> u32 {
        let lead = self.char_at(index).to_u16() as u32;
        if u16_is_lead(lead) && index + 1 != self.code_end {
            let trail = self.char_at(index + 1).to_u16() as u32;
            if (trail & 0xFFFF_FC00) == 0xDC00 {
                return u16_get_supplementary(lead, trail);
            }
        }
        lead
    }

    /// `Lexer<T>::lexWithoutClearingLineTerminator`.
    pub fn lex_without_clearing_line_terminator(
        &mut self,
        token_record: &mut JSToken,
        mut lexer_flags: LexerFlagSet,
        strict_mode: bool,
    ) -> JSTokenType {
        use crate::parser::parser_tokens::{
            ANDEQUAL, AND, ARROWFUNCTION, AUTOMINUSMINUS, AUTOPLUSPLUS, BACKQUOTE, BITANDEQUAL, BITAND, BITOREQUAL,
            BITOR, BITXOREQUAL, BITXOR, BIGINT, CAN_BE_ERROR_TOKEN_FLAG, CLOSEBRACE, CLOSEBRACKET, CLOSEPAREN,
            COALESCEEQUAL, COALESCE, COLON, COMMA, DIVEQUAL, DIVIDE, DOTDOTDOT, DOT, EOFTOK, EQEQ, EQUAL, ERRORTOK,
            EXCLAMATION, GE, GT, LE, LSHIFTEQUAL, LSHIFT, LT, MINUSEQUAL,
            MINUSMINUS, MINUS, MODEQUAL, MOD, MULTEQUAL, NE, OPENBRACE, OPENBRACKET, OPENPAREN, OREQUAL, OR,
            PLUSEQUAL, PLUSPLUS, PLUS, POWEQUAL, POW, QUESTIONDOT, QUESTION, RSHIFTEQUAL, RSHIFT, SEMICOLON,
            STREQ, STRING, STRNEQ, TILDE, TIMES, UNTERMINATED_MULTILINE_COMMENT_ERRORTOK,
            UNTERMINATED_OCTAL_NUMBER_ERRORTOK, UNTERMINATED_STRING_LITERAL_ERRORTOK, URSHIFTEQUAL, URSHIFT,
            INVALID_STRING_LITERAL_ERRORTOK,
        };
        use crate::wtf::ascii_ctype::is_ascii_alpha_caseless_equal;

        #[derive(Clone, Copy)]
        enum Label {
            Start,
            Zero,
            Number,
            IdentFast,
            ParseIdent,
            AfterSwitch,
            ReturnToken,
            InvalidCharacter,
            ReturnError,
        }

        debug_assert!(!self.error);

        debug_assert!(self.buffer8.is_empty());
        debug_assert!(self.buffer16.is_empty());
        let mut token: JSTokenType = ERRORTOK;

        let mut label = Label::Start;
        loop {
            label = match label {
                Label::Start => 'b: {
                    self.skip_whitespace();

                    debug_assert!(self.current_offset() >= self.current_line_start_offset());
                    token_record.start_position = self.current_position();

                    let mut ty: u8 = self.current.to_u16() as u8;
                    if !is_latin1(self.cur()) {
                        let code_point = self.lexer_code_point_at(self.code);
                        if is_non_latin1_ident_start(code_point) {
                            // We are hijacking white space characters for non-latin1 identifier start in the following dispatch since we will never see
                            // these characters in the switch because of `skipWhitespace()` call.
                            ty = b' ';
                        } else if Self::is_line_terminator(self.current) {
                            ty = b'\n';
                        } else {
                            ty = 0;
                        }
                    }

                    match ty {
                        b'>' => {
                            self.shift();
                            if self.cur() == '>' as u32 {
                                self.shift();
                                if self.cur() == '>' as u32 {
                                    self.shift();
                                    if self.cur() == '=' as u32 {
                                        self.shift();
                                        token = URSHIFTEQUAL;
                                        break 'b Label::AfterSwitch;
                                    }
                                    token = URSHIFT;
                                    break 'b Label::AfterSwitch;
                                }
                                if self.cur() == '=' as u32 {
                                    self.shift();
                                    token = RSHIFTEQUAL;
                                    break 'b Label::AfterSwitch;
                                }
                                token = RSHIFT;
                                break 'b Label::AfterSwitch;
                            }
                            if self.cur() == '=' as u32 {
                                self.shift();
                                token = GE;
                                break 'b Label::AfterSwitch;
                            }
                            token = GT;
                            break 'b Label::AfterSwitch;
                        }

                        b'=' => {
                            if self.peek_u32(1) == '>' as u32 {
                                token = ARROWFUNCTION;
                                token_record.data.line = self.line_number() as u32;
                                token_record.data.offset = self.current_offset() as u32;
                                token_record.data.line_start_offset = self.current_line_start_offset() as u32;
                                debug_assert!(token_record.data.offset >= token_record.data.line_start_offset);
                                self.shift();
                                self.shift();
                                break 'b Label::AfterSwitch;
                            }

                            self.shift();
                            if self.cur() == '=' as u32 {
                                self.shift();
                                if self.cur() == '=' as u32 {
                                    self.shift();
                                    token = STREQ;
                                    break 'b Label::AfterSwitch;
                                }
                                token = EQEQ;
                                break 'b Label::AfterSwitch;
                            }
                            token = EQUAL;
                            break 'b Label::AfterSwitch;
                        }

                        b'<' => {
                            self.shift();
                            if self.cur() == '!' as u32 && self.peek_u32(1) == '-' as u32 && self.peek_u32(2) == '-' as u32 {
                                if self.script_mode == JSParserScriptMode::Classic {
                                    // <!-- marks the beginning of a line comment (for www usage)
                                    if let Some(result) = self.scan_single_line_comment(token_record, false) {
                                        return result;
                                    }
                                    break 'b Label::Start;
                                }
                            }
                            if self.cur() == '<' as u32 {
                                self.shift();
                                if self.cur() == '=' as u32 {
                                    self.shift();
                                    token = LSHIFTEQUAL;
                                    break 'b Label::AfterSwitch;
                                }
                                token = LSHIFT;
                                break 'b Label::AfterSwitch;
                            }
                            if self.cur() == '=' as u32 {
                                self.shift();
                                token = LE;
                                break 'b Label::AfterSwitch;
                            }
                            token = LT;
                            break 'b Label::AfterSwitch;
                        }

                        b'!' => {
                            self.shift();
                            if self.cur() == '=' as u32 {
                                self.shift();
                                if self.cur() == '=' as u32 {
                                    self.shift();
                                    token = STRNEQ;
                                    break 'b Label::AfterSwitch;
                                }
                                token = NE;
                                break 'b Label::AfterSwitch;
                            }
                            token = EXCLAMATION;
                            break 'b Label::AfterSwitch;
                        }

                        b'+' => {
                            self.shift();
                            if self.cur() == '+' as u32 {
                                self.shift();
                                token = if !self.has_line_terminator_before_token { PLUSPLUS } else { AUTOPLUSPLUS };
                                break 'b Label::AfterSwitch;
                            }
                            if self.cur() == '=' as u32 {
                                self.shift();
                                token = PLUSEQUAL;
                                break 'b Label::AfterSwitch;
                            }
                            token = PLUS;
                            break 'b Label::AfterSwitch;
                        }

                        b'-' => {
                            self.shift();
                            if self.cur() == '-' as u32 {
                                self.shift();
                                if (self.at_line_start || self.has_line_terminator_before_token) && self.cur() == '>' as u32 {
                                    if self.script_mode == JSParserScriptMode::Classic {
                                        self.shift();
                                        if let Some(result) = self.scan_single_line_comment(token_record, false) {
                                            return result;
                                        }
                                        break 'b Label::Start;
                                    }
                                }
                                token = if !self.has_line_terminator_before_token { MINUSMINUS } else { AUTOMINUSMINUS };
                                break 'b Label::AfterSwitch;
                            }
                            if self.cur() == '=' as u32 {
                                self.shift();
                                token = MINUSEQUAL;
                                break 'b Label::AfterSwitch;
                            }
                            token = MINUS;
                            break 'b Label::AfterSwitch;
                        }

                        b'*' => {
                            self.shift();
                            if self.cur() == '=' as u32 {
                                self.shift();
                                token = MULTEQUAL;
                                break 'b Label::AfterSwitch;
                            }
                            if self.cur() == '*' as u32 {
                                self.shift();
                                if self.cur() == '=' as u32 {
                                    self.shift();
                                    token = POWEQUAL;
                                    break 'b Label::AfterSwitch;
                                }
                                token = POW;
                                break 'b Label::AfterSwitch;
                            }
                            token = TIMES;
                            break 'b Label::AfterSwitch;
                        }

                        b'/' => {
                            self.shift();
                            if self.cur() == '/' as u32 {
                                self.shift();
                                if let Some(result) = self.scan_single_line_comment(token_record, true) {
                                    return result;
                                }
                                break 'b Label::Start;
                            }
                            if self.cur() == '*' as u32 {
                                self.shift();
                                if self.parse_multiline_comment() {
                                    break 'b Label::Start;
                                }
                                self.lex_error_message = lex_message("Multiline comment was not closed properly");
                                token = UNTERMINATED_MULTILINE_COMMENT_ERRORTOK;
                                self.error = true;
                                let end_position = self.current_position();
                                self.fill_token_info(token_record, end_position);
                                return token;
                            }
                            if self.cur() == '=' as u32 {
                                self.shift();
                                token = DIVEQUAL;
                                break 'b Label::AfterSwitch;
                            }
                            token = DIVIDE;
                            break 'b Label::AfterSwitch;
                        }

                        b'&' => {
                            self.shift();
                            if self.cur() == '&' as u32 {
                                self.shift();
                                if self.cur() == '=' as u32 {
                                    self.shift();
                                    token = ANDEQUAL;
                                    break 'b Label::AfterSwitch;
                                }
                                token = AND;
                                break 'b Label::AfterSwitch;
                            }
                            if self.cur() == '=' as u32 {
                                self.shift();
                                token = BITANDEQUAL;
                                break 'b Label::AfterSwitch;
                            }
                            token = BITAND;
                            break 'b Label::AfterSwitch;
                        }

                        b'^' => {
                            self.shift();
                            if self.cur() == '=' as u32 {
                                self.shift();
                                token = BITXOREQUAL;
                                break 'b Label::AfterSwitch;
                            }
                            token = BITXOR;
                            break 'b Label::AfterSwitch;
                        }

                        b'%' => {
                            self.shift();
                            if self.cur() == '=' as u32 {
                                self.shift();
                                token = MODEQUAL;
                                break 'b Label::AfterSwitch;
                            }
                            token = MOD;
                            break 'b Label::AfterSwitch;
                        }

                        b'|' => {
                            self.shift();
                            if self.cur() == '=' as u32 {
                                self.shift();
                                token = BITOREQUAL;
                                break 'b Label::AfterSwitch;
                            }
                            if self.cur() == '|' as u32 {
                                self.shift();
                                if self.cur() == '=' as u32 {
                                    self.shift();
                                    token = OREQUAL;
                                    break 'b Label::AfterSwitch;
                                }
                                token = OR;
                                break 'b Label::AfterSwitch;
                            }
                            token = BITOR;
                            break 'b Label::AfterSwitch;
                        }

                        b'(' => {
                            token = OPENPAREN;
                            token_record.data.line = self.line_number() as u32;
                            token_record.data.offset = self.current_offset() as u32;
                            token_record.data.line_start_offset = self.current_line_start_offset() as u32;
                            self.shift();
                            break 'b Label::AfterSwitch;
                        }

                        b')' => {
                            token = CLOSEPAREN;
                            self.shift();
                            break 'b Label::AfterSwitch;
                        }

                        b'[' => {
                            token = OPENBRACKET;
                            self.shift();
                            break 'b Label::AfterSwitch;
                        }

                        b']' => {
                            token = CLOSEBRACKET;
                            self.shift();
                            break 'b Label::AfterSwitch;
                        }

                        b',' => {
                            token = COMMA;
                            self.shift();
                            break 'b Label::AfterSwitch;
                        }

                        b':' => {
                            token = COLON;
                            self.shift();
                            break 'b Label::AfterSwitch;
                        }

                        b'?' => {
                            self.shift();
                            if self.cur() == '?' as u32 {
                                self.shift();
                                if self.cur() == '=' as u32 {
                                    self.shift();
                                    token = COALESCEEQUAL;
                                    break 'b Label::AfterSwitch;
                                }
                                token = COALESCE;
                                break 'b Label::AfterSwitch;
                            }
                            if self.cur() == '.' as u32 && !crate::wtf::ascii_ctype::is_ascii_digit(self.peek_u32(1)) {
                                self.shift();
                                token = QUESTIONDOT;
                                break 'b Label::AfterSwitch;
                            }
                            token = QUESTION;
                            break 'b Label::AfterSwitch;
                        }

                        b'~' => {
                            token = TILDE;
                            self.shift();
                            break 'b Label::AfterSwitch;
                        }

                        b';' => {
                            self.shift();
                            token = SEMICOLON;
                            break 'b Label::AfterSwitch;
                        }

                        b'`' => {
                            self.shift();
                            token = BACKQUOTE;
                            break 'b Label::AfterSwitch;
                        }

                        b'{' => {
                            token_record.data.line = self.line_number() as u32;
                            token_record.data.offset = self.current_offset() as u32;
                            token_record.data.line_start_offset = self.current_line_start_offset() as u32;
                            debug_assert!(token_record.data.offset >= token_record.data.line_start_offset);
                            self.shift();
                            token = OPENBRACE;
                            break 'b Label::AfterSwitch;
                        }

                        b'}' => {
                            token_record.data.line = self.line_number() as u32;
                            token_record.data.offset = self.current_offset() as u32;
                            token_record.data.line_start_offset = self.current_line_start_offset() as u32;
                            debug_assert!(token_record.data.offset >= token_record.data.line_start_offset);
                            self.shift();
                            token = CLOSEBRACE;
                            break 'b Label::AfterSwitch;
                        }

                        b'.' => {
                            self.shift();
                            if !crate::wtf::ascii_ctype::is_ascii_digit(self.cur()) {
                                if self.cur() == '.' as u32 && self.peek_u32(1) == '.' as u32 {
                                    self.shift();
                                    self.shift();
                                    token = DOTDOTDOT;
                                    break 'b Label::AfterSwitch;
                                }
                                token = DOT;
                                break 'b Label::AfterSwitch;
                            }
                            if !self.parse_number_after_decimal_point() {
                                self.set_numeric_literal_error(&mut token, "Non-number found after decimal point");
                                break 'b Label::ReturnError;
                            }
                            token = DOUBLE;
                            if is_ascii_alpha_caseless_equal(self.cur(), b'e') && !self.parse_number_after_exponent_indicator() {
                                self.set_numeric_literal_error(&mut token, "Non-number found after exponent indicator");
                                break 'b Label::ReturnError;
                            }
                            let mut _parsed_length: usize = 0;
                            let value = crate::wtf::fast_float::parse_double(&self.buffer8, &mut _parsed_length);
                            token_record.data.double_value = value;

                            if self.finish_decimal_literal(&mut token) {
                                break 'b Label::ReturnError;
                            }
                            break 'b Label::AfterSwitch;
                        }

                        b'0' => break 'b Label::Zero,

                        b'1'..=b'9' => break 'b Label::Number,

                        b'"' | b'\'' => {
                            let result = if lexer_flags.contains(LexerFlags::DontBuildStrings) {
                                self.parse_string::<false>(&mut token_record.data, strict_mode)
                            } else {
                                self.parse_string::<true>(&mut token_record.data, strict_mode)
                            };

                            if result != StringParseResult::StringParsedSuccessfully {
                                token = if result == StringParseResult::StringUnterminated {
                                    UNTERMINATED_STRING_LITERAL_ERRORTOK
                                } else {
                                    INVALID_STRING_LITERAL_ERRORTOK
                                };
                                self.error = true;
                                let end_position = self.current_position();
                                self.fill_token_info(token_record, end_position);
                                return token;
                            }
                            self.shift();
                            token = STRING;
                            self.at_line_start = false;
                            let end_position = self.current_position();
                            self.fill_token_info(token_record, end_position);
                            return token;
                        }

                        b'\\' => break 'b Label::ParseIdent,

                        b'\n' | b'\r' => {
                            debug_assert!(Self::is_line_terminator(self.current));
                            self.shift_line_terminator();
                            self.at_line_start = true;
                            self.has_line_terminator_before_token = true;
                            break 'b Label::Start;
                        }

                        b'#' => {
                            // Hashbang is only permitted at the start of the source text.
                            let next = self.peek_u32(1);
                            if next == '!' as u32 && self.current_offset() == 0 {
                                self.shift();
                                self.shift();
                                if let Some(result) = self.scan_single_line_comment(token_record, false) {
                                    return result;
                                }
                                break 'b Label::Start;
                            }

                            let is_valid_private_name = if is_latin1(next) {
                                TYPES_OF_LATIN1_CHARACTERS[next as usize] == CharacterLatin1IdentifierStart
                                    || next == '\\' as u32
                            } else {
                                debug_assert!(self.code + 1 < self.code_end);
                                is_non_latin1_ident_start(self.lexer_code_point_at(self.code + 1))
                            };

                            if is_valid_private_name {
                                lexer_flags.remove(LexerFlags::DontBuildKeywords);
                                break 'b Label::ParseIdent;
                            }
                            break 'b Label::InvalidCharacter;
                        }

                        b'@' => {
                            if self.parsing_builtin_function {
                                break 'b Label::ParseIdent;
                            }
                            break 'b Label::InvalidCharacter;
                        }

                        0 => {
                            if self.at_end() {
                                token = EOFTOK;
                                break 'b Label::ReturnToken;
                            }
                            break 'b Label::InvalidCharacter;
                        }

                        // Os demais códigos se decidem pela tabela de que o `switch` do C++ foi gerado.
                        _ => match TYPES_OF_LATIN1_CHARACTERS[ty as usize] {
                            CharacterLatin1IdentifierStart => break 'b Label::IdentFast,
                            // Não são espaço em branco aqui: o `skipWhitespace()` já os pulou, e o
                            // despacho os sequestra para início de identificador não Latin1.
                            CharacterWhiteSpace => break 'b Label::ParseIdent,
                            _ => break 'b Label::InvalidCharacter,
                        },
                    }
                }

                Label::Zero => 'b: {
                    self.shift();
                    if is_ascii_alpha_caseless_equal(self.cur(), b'x') {
                        if self.lex_radix_prefixed_number(&mut token_record.data, &mut token, 16) {
                            break 'b Label::ReturnError;
                        }
                        break 'b Label::AfterSwitch;
                    }
                    if is_ascii_alpha_caseless_equal(self.cur(), b'b') {
                        if self.lex_radix_prefixed_number(&mut token_record.data, &mut token, 2) {
                            break 'b Label::ReturnError;
                        }
                        break 'b Label::AfterSwitch;
                    }

                    if is_ascii_alpha_caseless_equal(self.cur(), b'o') {
                        if self.lex_radix_prefixed_number(&mut token_record.data, &mut token, 8) {
                            break 'b Label::ReturnError;
                        }
                        break 'b Label::AfterSwitch;
                    }

                    if self.cur() == '_' as u32 {
                        self.lex_error_message = lex_message("Numeric literals may not begin with 0_");
                        token = UNTERMINATED_OCTAL_NUMBER_ERRORTOK;
                        break 'b Label::ReturnError;
                    }

                    if strict_mode && crate::wtf::ascii_ctype::is_ascii_digit(self.cur()) {
                        self.lex_error_message =
                            lex_message("Decimal integer literals with a leading zero are forbidden in strict mode");
                        token = UNTERMINATED_OCTAL_NUMBER_ERRORTOK;
                        break 'b Label::ReturnError;
                    }

                    if crate::wtf::ascii_ctype::is_ascii_octal_digit(self.cur()) {
                        self.record8('0' as i32);
                        if let Some(NumberParseResult::Double(value)) = self.parse_octal() {
                            token_record.data.double_value = value;
                            token = token_type_for_integer_like_token(value);
                        }
                    } else {
                        if !crate::wtf::ascii_ctype::is_ascii_digit(self.cur())
                            && self.cur() != '.' as u32
                            && cannot_be_ident_start(self.current)
                        {
                            token_record.data.double_value = 0.0;
                            token = INTEGER;
                            break 'b Label::AfterSwitch;
                        }
                        self.record8('0' as i32);
                    }
                    Label::Number
                }

                Label::Number => 'b: {
                    if token != INTEGER && token != DOUBLE {
                        if self.buffer8.is_empty() {
                            let start = self.code;
                            let mut ptr = self.code + 1;
                            let mut result: u64 = Into::<u32>::into(self.char_at(start)).wrapping_sub('0' as u32) as u64;
                            while ptr < self.code_end
                                && crate::wtf::ascii_ctype::is_ascii_digit(Into::<u32>::into(self.char_at(ptr)))
                            {
                                result = result
                                    .wrapping_mul(10)
                                    .wrapping_add(Into::<u32>::into(self.char_at(ptr)).wrapping_sub('0' as u32) as u64);
                                ptr += 1;
                            }

                            // The limit is 1 << (52 - 1) = 2251799813685248
                            const NUMBER_OF_DIGITS_FOR_SAFE_INT52: usize = 15;
                            if ptr < self.code_end
                                && (Into::<u32>::into(self.char_at(ptr)) != '.' as u32
                                    && cannot_be_ident_start(self.char_at(ptr)))
                                && (ptr - start) <= NUMBER_OF_DIGITS_FOR_SAFE_INT52
                            {
                                token_record.data.double_value = result as f64;
                                token = INTEGER;
                                self.code = ptr;
                                self.current = self.char_at(ptr);
                                break 'b Label::AfterSwitch;
                            }
                        }

                        match self.parse_decimal() {
                            Some(NumberParseResult::Double(value)) => {
                                token_record.data.double_value = value;
                                token = token_type_for_integer_like_token(value);
                            }
                            Some(NumberParseResult::Identifier(big_int_string)) => {
                                token = BIGINT;
                                self.shift();
                                token_record.data.big_int_string = Some(big_int_string);
                                token_record.data.radix = 10;
                            }
                            None => {
                                token = INTEGER;
                                if self.cur() == '.' as u32 {
                                    self.shift();
                                    if crate::wtf::ascii_ctype::is_ascii_digit(self.cur())
                                        && !self.parse_number_after_decimal_point()
                                    {
                                        self.set_numeric_literal_error(&mut token, "Non-number found after decimal point");
                                        break 'b Label::ReturnError;
                                    }
                                    token = DOUBLE;
                                }
                                if is_ascii_alpha_caseless_equal(self.cur(), b'e')
                                    && !self.parse_number_after_exponent_indicator()
                                {
                                    self.set_numeric_literal_error(&mut token, "Non-number found after exponent indicator");
                                    break 'b Label::ReturnError;
                                }
                                let mut _parsed_length: usize = 0;
                                let value = crate::wtf::fast_float::parse_double(&self.buffer8, &mut _parsed_length);
                                token_record.data.double_value = value;
                                if token == INTEGER {
                                    token = token_type_for_integer_like_token(value);
                                }
                            }
                        }
                    }

                    if self.finish_decimal_literal(&mut token) {
                        break 'b Label::ReturnError;
                    }
                    Label::AfterSwitch
                }

                Label::IdentFast => 'b: {
                    // We observe one character identifier very frequently because real world web pages are shipping minified JavaScript.
                    // This path handles it in a fast path.
                    let next_character = self.peek_u32(1);
                    if is_latin1(next_character) {
                        // This quickly detects the character is not a part of identifier-part *and* back-slash.
                        if TYPES_OF_LATIN1_CHARACTERS[next_character as usize] > CharacterBackSlash {
                            let character = self.current;
                            self.shift();
                            if lexer_flags.contains(LexerFlags::DontBuildKeywords) {
                                token_record.data.ident = None;
                            } else {
                                token_record.data.ident = Some(self.make_identifier(&[character]));
                            }
                            token = IDENT;
                            break 'b Label::AfterSwitch;
                        }
                    }
                    Label::ParseIdent
                }

                Label::ParseIdent => {
                    token = if lexer_flags.contains(LexerFlags::DontBuildKeywords) {
                        self.parse_identifier::<false>(&mut token_record.data, lexer_flags, strict_mode)
                    } else {
                        self.parse_identifier::<true>(&mut token_record.data, lexer_flags, strict_mode)
                    };
                    Label::AfterSwitch
                }

                Label::AfterSwitch => {
                    self.at_line_start = false;
                    Label::ReturnToken
                }

                Label::ReturnToken => {
                    let end_position = self.current_position();
                    self.fill_token_info(token_record, end_position);
                    return token;
                }

                Label::InvalidCharacter => {
                    self.lex_error_message = self.invalid_character_message();
                    token = ERRORTOK;
                    // Falls through to return error.
                    Label::ReturnError
                }

                Label::ReturnError => {
                    self.error = true;
                    let end_position = self.current_position();
                    self.fill_token_info(token_record, end_position);
                    assert!(token & CAN_BE_ERROR_TOKEN_FLAG != 0);
                    return token;
                }
            };
        }
    }
}

// Trechos que o `lexWithoutClearingLineTerminator` repete no C++, extraídos pela regra DRY.
impl<T: CharType> Lexer<T> {
    /// O par `m_lexErrorMessage = ...; token = atEnd() ? UNTERMINATED_NUMERIC_LITERAL_ERRORTOK :
    /// INVALID_NUMERIC_LITERAL_ERRORTOK;` que o C++ repete nos erros de literal numérico decimal.
    fn set_numeric_literal_error(&mut self, token: &mut JSTokenType, message: &str) {
        use crate::parser::parser_tokens::{INVALID_NUMERIC_LITERAL_ERRORTOK, UNTERMINATED_NUMERIC_LITERAL_ERRORTOK};

        self.lex_error_message = lex_message(message);
        *token = if self.at_end() { UNTERMINATED_NUMERIC_LITERAL_ERRORTOK } else { INVALID_NUMERIC_LITERAL_ERRORTOK };
    }

    /// O fim comum dos literais decimais (`cannotBeIdentStart(m_current)` ... `m_buffer8.shrink(0)`):
    /// devolve `true` quando o literal é seguido de um identificador (`goto returnError`).
    fn finish_decimal_literal(&mut self, token: &mut JSTokenType) -> bool {
        if !cannot_be_ident_start(self.current) && is_ident_start(self.current_code_point()) {
            self.set_numeric_literal_error(token, "No identifiers allowed directly after numeric literal");
            return true;
        }
        self.buffer8.clear();
        false
    }

    /// Os três ramos `0x`, `0b` e `0o` do `case '0'`, que no C++ diferem só pelo predicado de
    /// dígito, pelo `parseHex`/`parseBinary`/`parseOctal`, pela base e pelas mensagens. `self.current`
    /// está no `x`/`b`/`o`. Devolve `true` quando o C++ faria `goto returnError`.
    fn lex_radix_prefixed_number(
        &mut self,
        token_data: &mut JSTokenData,
        token: &mut JSTokenType,
        radix: u8,
    ) -> bool {
        use crate::parser::parser_tokens::{
            BIGINT, UNTERMINATED_BINARY_NUMBER_ERRORTOK, UNTERMINATED_HEX_NUMBER_ERRORTOK,
            UNTERMINATED_OCTAL_NUMBER_ERRORTOK,
        };

        let (is_digit, no_digits_message, no_space_message, error_token): (
            fn(u32) -> bool,
            &str,
            &str,
            JSTokenType,
        ) = match radix {
            16 => (
                is_ascii_hex_digit::<u32>,
                "No hexadecimal digits after '0x'",
                "No space between hexadecimal literal and identifier",
                UNTERMINATED_HEX_NUMBER_ERRORTOK,
            ),
            2 => (
                is_ascii_binary_digit::<u32>,
                "No binary digits after '0b'",
                "No space between binary literal and identifier",
                UNTERMINATED_BINARY_NUMBER_ERRORTOK,
            ),
            _ => (
                is_ascii_octal_digit::<u32>,
                "No octal digits after '0o'",
                "No space between octal literal and identifier",
                UNTERMINATED_OCTAL_NUMBER_ERRORTOK,
            ),
        };

        if !is_digit(self.peek_u32(1)) {
            self.lex_error_message = lex_message(no_digits_message);
            *token = error_token;
            return true;
        }

        // Shift out the 'x', 'b' or 'o' prefix.
        self.shift();

        let parse_number_result = match radix {
            16 => self.parse_hex(),
            2 => self.parse_binary(),
            _ => self.parse_octal(),
        };
        match parse_number_result {
            None => token_data.double_value = 0.0,
            Some(NumberParseResult::Double(value)) => token_data.double_value = value,
            Some(NumberParseResult::Identifier(big_int_string)) => {
                *token = BIGINT;
                self.shift();
                token_data.big_int_string = Some(big_int_string);
                token_data.radix = radix;
            }
        }

        if !cannot_be_ident_start(self.current) && is_ident_start(self.current_code_point()) {
            self.lex_error_message = lex_message(no_space_message);
            *token = error_token;
            return true;
        }
        if *token != BIGINT {
            *token = token_type_for_integer_like_token(token_data.double_value);
        }
        self.buffer8.clear();
        false
    }
}
