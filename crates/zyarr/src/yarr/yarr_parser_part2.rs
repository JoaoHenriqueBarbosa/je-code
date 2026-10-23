// Parte 2 de `yarr/YarrParser.h` (linhas 1223 em diante). Incluída por `include!` no fim de
// `yarr_parser.rs`: usa os `use` e os tipos de lá.

// `U_GC_*_MASK` do ICU: `1 << UCharCategory`. L = Lu, Ll, Lt, Lm, Lo; Mn = 6; Mc = 8; Nd = 9;
// Pc (CONNECTOR_PUNCTUATION) = 22.
const U_GC_L_MASK: u32 = (1 << 1) | (1 << 2) | (1 << 3) | (1 << 4) | (1 << 5);
const U_GC_MN_MASK: u32 = 1 << 6;
const U_GC_MC_MASK: u32 = 1 << 8;
const U_GC_ND_MASK: u32 = 1 << 9;
const U_GC_PC_MASK: u32 = 1 << 22;

/// `JSC_REGEXP_MOD_FLAGS`: as flags aceitas em modificadores de regexp `(?ims-ims:...)`.
const REGEXP_MOD_FLAGS: [(char, Flags); 3] = [
    ('i', Flags::IgnoreCase),
    ('m', Flags::Multiline),
    ('s', Flags::DotAll),
];

/// `U_GET_GC_MASK(c)`.
fn u_get_gc_mask(ch: u32) -> u32 {
    1u32 << crate::wtf::unicode::char_category(ch)
}

impl<'a, D: Delegate, C: CharType> Parser<'a, D, C> {
    pub fn consume_possible_surrogate_pair(&mut self, context: UnicodeParseContext) -> u32 {
        let unicode_pattern_or_group_name =
            self.is_either_unicode_compilation() || context == UnicodeParseContext::GroupName;

        let mut ch = self.consume();
        if crate::wtf::unicode::utf8_conversion::u16_is_lead(ch)
            && unicode_pattern_or_group_name
            && !self.at_end_of_pattern()
        {
            let state = self.save_state();

            let surrogate2 = self.consume();
            if crate::wtf::unicode::utf8_conversion::u16_is_trail(surrogate2) {
                ch = crate::wtf::unicode::utf8_conversion::u16_get_supplementary(ch, surrogate2);
            } else {
                self.restore_state(state);
            }
        }

        ch
    }

    fn consume_and_check_if_valid_class_set_character(&mut self) -> u32 {
        let ch = self.consume_possible_surrogate_pair(UnicodeParseContext::PatternCodePoint);

        if ch == 0 {
            self.error_code = ErrorCode::InvalidClassSetCharacter;
            return ERROR_CODE_POINT;
        }

        if is_ascii(ch) {
            // Confere se o caractere faz parte de ClassSetSyntaxCharacter.
            // Deixamos o tratamento de - e \ para o chamador.
            if "()[]{}/|)".bytes().any(|syntax| syntax as u32 == ch) {
                self.error_code = ErrorCode::InvalidClassSetCharacter;
                return ERROR_CODE_POINT;
            }

            // Confere se o caractere corrente e o próximo formam um ClassSetReservedDoublePunctuator.
            if !self.at_end_of_pattern() {
                let next_ch = self.peek();
                if ch == next_ch && "&!#$%*+,.:;<=>?@^`~".bytes().any(|punctuator| punctuator as u32 == ch) {
                    self.error_code = ErrorCode::InvalidClassSetOperation;
                    return ERROR_CODE_POINT;
                }
            }
        }

        ch
    }

    // parseAtomEscape(), parseCharacterClassEscape(), parseClassSetEscape() e
    // parseClassStringDisjunctionEscape(): são apelidos de parseEscape().
    fn parse_atom_escape(&mut self) -> TokenType {
        self.parse_escape(ParseEscapeMode::Normal, &mut AtomEscapeSink)
    }

    fn parse_character_class_escape(&mut self, delegate: &mut CharacterClassParserDelegate) {
        self.parse_escape(ParseEscapeMode::CharacterClass, delegate);
    }

    fn parse_class_set_escape(&mut self, delegate: &mut ClassSetParserDelegate) -> TokenType {
        self.parse_escape(ParseEscapeMode::ClassSet, delegate)
    }

    fn parse_class_string_disjunction_escape(&mut self, delegate: &mut ClassStringDisjunctionParserDelegate) {
        self.parse_escape(ParseEscapeMode::ClassStringDisjunction, delegate);
    }

    /// `parseCharacterClass()`:
    ///
    /// Auxiliar de `parseTokens()`; chama direta e indiretamente (via `parseCharacterClassEscape`) uma
    /// instância de `CharacterClassParserDelegate`, para descrever a classe de caracteres ao delegate.
    fn parse_character_class(&mut self) {
        debug_assert!(!has_error(self.error_code));
        debug_assert!(self.peek() == '[' as u32);
        self.consume();

        let mut character_class_constructor = CharacterClassParserDelegate::new(self.compile_mode);

        let invert = self.try_consume('^' as u16);
        character_class_constructor.begin(self.delegate, invert);

        while !self.at_end_of_pattern() {
            match char::from_u32(self.peek()) {
                Some(']') => {
                    self.consume();
                    character_class_constructor.end(self.delegate);
                    return;
                }

                Some('\\') => {
                    self.parse_character_class_escape(&mut character_class_constructor);
                }

                _ => {
                    let ch = self.consume_possible_surrogate_pair(UnicodeParseContext::PatternCodePoint);
                    character_class_constructor.atom_pattern_character(self.delegate, &mut self.error_code, ch, true);
                }
            }

            if has_error(self.error_code) {
                return;
            }
        }

        self.error_code = ErrorCode::CharacterClassUnmatched;
    }

    /// O lambda `processCharacterNormally` de `parseClassSet()`.
    fn process_class_set_character_normally(&mut self, class_set_constructor: &mut ClassSetParserDelegate) {
        let ch = self.consume_and_check_if_valid_class_set_character();
        if ch == ERROR_CODE_POINT {
            return;
        }

        class_set_constructor.atom_pattern_character(self.delegate, &mut self.error_code, ch, true);
    }

    /// `parseClassSet()`:
    ///
    /// Auxiliar de `parseTokens()`; chama direta e indiretamente (via `parseClassSetEscape`) uma
    /// instância de `ClassSetParserDelegate`, para descrever a classe de caracteres ao delegate.
    fn parse_class_set(&mut self) {
        debug_assert!(!has_error(self.error_code));
        debug_assert!(self.peek() == '[' as u32);
        self.consume();

        let mut class_set_constructor = ClassSetParserDelegate::new();

        let invert = self.try_consume('^' as u16);
        class_set_constructor.begin(self.delegate, invert);

        while !self.at_end_of_pattern() {
            let ch = self.peek();
            match char::from_u32(ch) {
                Some(']') => {
                    self.consume();
                    if class_set_constructor.nested_class_end(self.delegate, &mut self.error_code) {
                        return;
                    }
                }

                Some('[') => {
                    self.consume();
                    let invert = self.try_consume('^' as u16);
                    class_set_constructor.nested_class_begin(self.delegate, &mut self.error_code, invert);
                }

                Some('\\') => {
                    if !class_set_constructor.can_take_set_operand(self.delegate, &mut self.error_code) {
                        self.error_code = ErrorCode::InvalidClassSetOperation;
                        return;
                    }

                    let token_type = self.parse_class_set_escape(&mut class_set_constructor);

                    class_set_constructor
                        .compute_may_contain_strings(token_type == TokenType::SetDisjunctionMayContainStrings);

                    if token_type == TokenType::SetDisjunction
                        || token_type == TokenType::SetDisjunctionMayContainStrings
                    {
                        class_set_constructor.after_set_operand(self.delegate, &mut self.error_code);
                    }
                }

                Some('-') => {
                    let state = self.save_state();
                    self.consume();
                    if self.at_end_of_pattern() {
                        self.error_code = ErrorCode::CharacterClassUnmatched;
                        return;
                    }
                    if self.peek() == '-' as u32 {
                        self.consume();
                        if self.at_end_of_pattern() || self.peek() == '-' as u32 {
                            self.error_code = ErrorCode::InvalidClassSetCharacter;
                            return;
                        }
                        class_set_constructor.set_subtract_op(self.delegate, &mut self.error_code);
                    } else {
                        self.restore_state(state);
                        self.process_class_set_character_normally(&mut class_set_constructor);
                    }
                }

                Some('&') => {
                    let state = self.save_state();
                    self.consume();
                    if self.at_end_of_pattern() {
                        self.error_code = ErrorCode::CharacterClassUnmatched;
                        return;
                    }
                    if self.peek() == '&' as u32 {
                        self.consume();
                        if self.at_end_of_pattern() || self.peek() == '&' as u32 {
                            self.error_code = ErrorCode::InvalidClassSetCharacter;
                            return;
                        }
                        class_set_constructor.set_intersection_op(self.delegate, &mut self.error_code);
                    } else {
                        self.restore_state(state);
                        self.process_class_set_character_normally(&mut class_set_constructor);
                    }
                }

                _ => {
                    self.process_class_set_character_normally(&mut class_set_constructor);
                }
            }

            if has_error(self.error_code) {
                return;
            }
        }

        self.error_code = ErrorCode::CharacterClassUnmatched;
    }

    /// `parseClassStringDisjunction()`:
    ///
    /// Auxiliar de `parseTokens()`; chama direta e indiretamente (via
    /// `parseClassStringDisjunctionEscape`) uma instância de `ClassStringDisjunctionParserDelegate`,
    /// para descrever a Class String Disjunction ao delegate.
    pub fn parse_class_string_disjunction(&mut self, disjunction_may_contain_strings: &mut bool) {
        debug_assert!(!has_error(self.error_code));
        debug_assert!(self.peek() == '{' as u32);
        self.consume();

        let mut string_disjunction_delegate = ClassStringDisjunctionParserDelegate::new();

        while !self.at_end_of_pattern() {
            match char::from_u32(self.peek()) {
                Some('}') => {
                    self.consume();
                    string_disjunction_delegate.end(self.delegate);
                    *disjunction_may_contain_strings = string_disjunction_delegate.may_contain_strings();
                    return;
                }

                Some('\\') => {
                    self.parse_class_string_disjunction_escape(&mut string_disjunction_delegate);
                }

                Some('|') => {
                    self.consume();
                    string_disjunction_delegate.new_alternative();
                }

                Some('-') => {
                    self.consume();
                    self.error_code = ErrorCode::InvalidClassSetCharacter;
                    return;
                }

                _ => {
                    let ch = self.consume_and_check_if_valid_class_set_character();

                    if ch == ERROR_CODE_POINT {
                        return;
                    }

                    string_disjunction_delegate.atom_pattern_character(self.delegate, &mut self.error_code, ch, false);
                }
            }

            if has_error(self.error_code) {
                return;
            }
        }

        self.error_code = ErrorCode::ClassStringDisjunctionUnmatched;
    }

    /// `parseParenthesesBegin()`:
    ///
    /// Auxiliar de `parseTokens()`; confere os tipos de parênteses que não são subpadrões de captura
    /// comuns.
    fn parse_parentheses_begin(&mut self) {
        debug_assert!(!has_error(self.error_code));
        debug_assert!(self.peek() == '(' as u32);
        self.consume();

        let mut parentheses_type = ParenthesesType::Subpattern;
        let mut is_non_capturing_group = false;

        if self.try_consume('?' as u16) {
            if self.at_end_of_pattern() {
                self.error_code = ErrorCode::ParenthesesTypeInvalid;
                return;
            }

            match char::from_u32(self.peek()) {
                Some(':') => {
                    self.consume();
                    self.delegate.atom_parentheses_subpattern_begin(false, None);
                    is_non_capturing_group = true;
                }

                Some('=') => {
                    self.consume();
                    self.delegate.atom_parenthetical_assertion_begin(false, MatchDirection::Forward);
                    parentheses_type = ParenthesesType::Assertion;
                }

                Some('!') => {
                    self.consume();
                    self.delegate.atom_parenthetical_assertion_begin(true, MatchDirection::Forward);
                    parentheses_type = ParenthesesType::Assertion;
                }

                Some('<') => {
                    self.consume();
                    let group_name = self.try_consume_group_name();
                    if !has_error(self.error_code) {
                        if let Some(group_name) = group_name {
                            if self.k_identity_escape_seen {
                                self.error_code = ErrorCode::InvalidNamedBackReference;
                            } else {
                                let is_new_entry = self.named_capture_groups.add(group_name.clone());
                                if is_new_entry {
                                    self.delegate.atom_parentheses_subpattern_begin(true, Some(group_name));
                                    self.count_captures();
                                } else {
                                    self.error_code = ErrorCode::DuplicateGroupName;
                                }
                            }
                        } else if self.try_consume('=' as u16) {
                            self.delegate.atom_parenthetical_assertion_begin(false, MatchDirection::Backward);
                            parentheses_type = ParenthesesType::LookbehindAssertion;
                        } else if self.try_consume('!' as u16) {
                            self.delegate.atom_parenthetical_assertion_begin(true, MatchDirection::Backward);
                            parentheses_type = ParenthesesType::LookbehindAssertion;
                        } else {
                            self.error_code = ErrorCode::InvalidGroupName;
                        }
                    }
                }

                // RegularExpressionFlags válidas para modificadores de regexp.
                Some('-' | 'i' | 'm' | 's') => {
                    // Consome caracteres até o ':'.
                    let mut set = FlagSet::empty();
                    let mut unset = FlagSet::empty();
                    let mut has_hit_negation = false;
                    is_non_capturing_group = true;
                    while !self.at_end_of_pattern() {
                        let c = self.consume();
                        if c == ':' as u32 {
                            break;
                        }

                        let modifier_flag = REGEXP_MOD_FLAGS
                            .iter()
                            .find(|(key, _)| *key as u32 == c)
                            .map(|&(_, flag)| flag);

                        if c == '-' as u32 {
                            if has_hit_negation {
                                self.error_code = ErrorCode::InvalidRegularExpressionModifier;
                            }
                            has_hit_negation = true;
                        } else if let Some(flag) = modifier_flag {
                            // É erro de sintaxe se o texto de RegularExpressionModifiers tiver o mesmo
                            // ponto de código mais de uma vez.
                            if has_hit_negation {
                                if unset.contains(flag) {
                                    self.error_code = ErrorCode::InvalidRegularExpressionModifier;
                                }
                                unset.add(flag);
                            } else {
                                if set.contains(flag) {
                                    self.error_code = ErrorCode::InvalidRegularExpressionModifier;
                                }
                                set.add(flag);
                            }
                        } else {
                            self.error_code = ErrorCode::ParenthesesTypeInvalid;
                        }
                    }

                    if !has_error(self.error_code) {
                        // Consumimos (?<flags>:

                        // É erro de sintaxe se algum ponto de código do primeiro
                        // RegularExpressionModifiers também estiver no segundo.
                        // `set.containsAny(unset)`.
                        if set.contains_any(unset) {
                            self.error_code = ErrorCode::InvalidRegularExpressionModifier;
                        }
                        // É erro de sintaxe se o primeiro e o segundo RegularExpressionModifiers
                        // forem ambos vazios.
                        if set.is_empty() && unset.is_empty() {
                            self.error_code = ErrorCode::InvalidRegularExpressionModifier;
                        }
                        self.delegate.atom_parenthetical_modifier_begin(set, unset);
                    }
                }

                _ => {
                    self.error_code = ErrorCode::ParenthesesTypeInvalid;
                }
            }
        } else {
            self.delegate.atom_parentheses_subpattern_begin(true, None);
            self.count_captures();
        }

        if parentheses_type == ParenthesesType::Subpattern && !is_non_capturing_group {
            self.num_subpatterns += 1;
        }

        self.parentheses_stack.push(parentheses_type);
        self.named_capture_groups.push_parenthesis();
    }

    /// `parseParenthesesEnd()`:
    ///
    /// Auxiliar de `parseTokens()`; confere erros de parse (parênteses sem par).
    ///
    /// O valor devolvido indica se o token lido foi um Atom ou, por compatibilidade com a web, uma
    /// QuantifiableAssertion em padrão não Unicode.
    fn parse_parentheses_end(&mut self) -> TokenType {
        debug_assert!(!has_error(self.error_code));
        debug_assert!(self.peek() == ')' as u32);
        self.consume();

        let Some(parentheses_type) = self.parentheses_stack.pop() else {
            self.error_code = ErrorCode::ParenthesesUnmatched;
            return TokenType::NotAtom;
        };

        self.delegate.atom_parentheses_end();

        self.named_capture_groups.pop_parenthesis();

        if parentheses_type == ParenthesesType::LookbehindAssertion {
            return TokenType::Lookbehind;
        }

        if parentheses_type == ParenthesesType::Subpattern || self.is_legacy_compilation() {
            return TokenType::Atom;
        }

        TokenType::NotAtom
    }

    /// `parseQuantifier()`:
    ///
    /// Auxiliar de `parseTokens()`; confere erros de parse e quantificadores não gulosos.
    fn parse_quantifier(&mut self, last_token_type: TokenType, min: u32, max: u32) {
        debug_assert!(!has_error(self.error_code));
        debug_assert!(min <= max);

        if last_token_type == TokenType::Atom {
            let greedy = !self.try_consume('?' as u16);
            self.delegate.quantify_atom(min, max, greedy);
        } else if last_token_type == TokenType::Lookbehind {
            self.error_code = ErrorCode::CantQuantifyAtom;
        } else {
            self.error_code = ErrorCode::QuantifierWithoutAtom;
        }
    }

    /// `parseTokens()`:
    ///
    /// Este método percorre o padrão de entrada reportando tokens ao delegate. Retorna quando um erro
    /// de parse é detectado ou o fim do padrão é alcançado. Um estado é mantido ao redor do laço: se o
    /// último token passado ao delegate foi um átomo (necessário para detectar o erro de um
    /// quantificador sem átomo para quantificar).
    pub fn parse_tokens(&mut self) {
        let mut last_token_type = TokenType::NotAtom;

        while !self.at_end_of_pattern() {
            // Marca o `[[fallthrough]]` do case '{' para o `default`.
            let mut use_default = false;

            match char::from_u32(self.peek()) {
                Some('|') => {
                    self.consume();
                    self.delegate.disjunction(CreateDisjunctionPurpose::ForNextAlternative);
                    last_token_type = TokenType::NotAtom;
                    self.named_capture_groups.next_alternative();
                }

                Some('(') => {
                    self.parse_parentheses_begin();
                    last_token_type = TokenType::NotAtom;
                }

                Some(')') => {
                    last_token_type = self.parse_parentheses_end();
                }

                Some('^') => {
                    self.consume();
                    self.delegate.assertion_bol();
                    last_token_type = TokenType::NotAtom;
                }

                Some('$') => {
                    self.consume();
                    self.delegate.assertion_eol();
                    last_token_type = TokenType::NotAtom;
                }

                Some('.') => {
                    self.consume();
                    self.delegate.atom_built_in_character_class(BuiltInCharacterClassID::DotClassID, false);
                    last_token_type = TokenType::Atom;
                }

                Some('[') => {
                    if self.is_unicode_sets_compilation() {
                        self.parse_class_set();
                    } else {
                        self.parse_character_class();
                    }
                    last_token_type = TokenType::Atom;
                }

                Some(']' | '}') => {
                    if self.is_either_unicode_compilation() {
                        self.error_code = ErrorCode::BracketUnmatched;
                    } else {
                        let ch = self.consume();
                        self.delegate.atom_pattern_character(ch, false);
                        last_token_type = TokenType::Atom;
                    }
                }

                Some('\\') => {
                    last_token_type = self.parse_atom_escape();
                }

                Some('*') => {
                    self.consume();
                    self.parse_quantifier(last_token_type, 0, QUANTIFY_INFINITE);
                    last_token_type = TokenType::NotAtom;
                }

                Some('+') => {
                    self.consume();
                    self.parse_quantifier(last_token_type, 1, QUANTIFY_INFINITE);
                    last_token_type = TokenType::NotAtom;
                }

                Some('?') => {
                    self.consume();
                    self.parse_quantifier(last_token_type, 0, 1);
                    last_token_type = TokenType::NotAtom;
                }

                Some('{') => {
                    let state = self.save_state();

                    self.consume();
                    let mut complete_quantifier = false;
                    if self.peek_is_digit() {
                        let mut min = self.consume_number64();
                        let mut max = min;

                        if self.try_consume(',' as u16) {
                            max = if self.peek_is_digit() {
                                self.consume_number64()
                            } else {
                                QUANTIFY_INFINITE64
                            };
                        }

                        if self.try_consume('}' as u16) {
                            if min == QUANTIFY_INFINITE64 {
                                self.error_code = ErrorCode::QuantifierTooLarge;
                            } else if min <= max {
                                min = min.min(QUANTIFY_INFINITE as u64);
                                max = max.min(QUANTIFY_INFINITE as u64);
                                self.parse_quantifier(last_token_type, min as u32, max as u32);
                            } else {
                                self.error_code = ErrorCode::QuantifierOutOfOrder;
                            }
                            last_token_type = TokenType::NotAtom;
                            complete_quantifier = true;
                        }
                    }

                    if !complete_quantifier {
                        if self.is_either_unicode_compilation() {
                            self.error_code = ErrorCode::QuantifierIncomplete;
                        } else {
                            self.restore_state(state);
                            // Se não achamos um quantificador completo, cai no caso default.
                            use_default = true;
                        }
                    }
                }

                _ => {
                    use_default = true;
                }
            }

            if use_default {
                let ch = self.consume_possible_surrogate_pair(UnicodeParseContext::PatternCodePoint);
                self.delegate.atom_pattern_character(ch, false);
                last_token_type = TokenType::Atom;
            }

            if has_error(self.error_code) {
                return;
            }

            if self.delegate.aborted_due_to_error() {
                self.error_code = self.delegate.abort_error_code();
                return;
            }
        }

        if !self.parentheses_stack.is_empty() {
            self.error_code = ErrorCode::MissingParentheses;
        }
    }

    pub fn handle_illegal_references(&mut self) {
        let mut should_reparse = false;

        if self.max_seen_back_reference > self.num_subpatterns {
            // Contém backreference numérica ilegal. Veja
            // https://tc39.es/ecma262/#prod-annexB-AtomEscape
            if self.is_either_unicode_compilation() {
                self.error_code = ErrorCode::InvalidBackreference;
                return;
            }

            self.back_reference_limit = self.num_subpatterns;
            should_reparse = true;
        }

        if self.k_identity_escape_seen && !self.named_capture_groups.is_empty() {
            self.error_code = ErrorCode::InvalidNamedBackReference;
            return;
        }

        if self.contains_illegal_named_forward_reference() {
            // \k<a> é tratado como referência nomeada em padrões Unicode por causa da gramática estrita
            // de IdentityEscape. Veja
            // https://tc39.es/ecma262/#sec-patterns-static-semantics-early-errors
            if self.is_either_unicode_compilation() || !self.named_capture_groups.is_empty() {
                self.error_code = ErrorCode::InvalidNamedBackReference;
                return;
            }

            self.is_named_forward_reference_allowed = false;
            should_reparse = true;
        }

        if should_reparse {
            self.reset_for_reparsing();
            self.parse_tokens();
        }
    }

    fn contains_illegal_named_forward_reference(&self) -> bool {
        if self.forward_reference_names.is_empty() {
            return false;
        }

        if self.named_capture_groups.is_empty() {
            return true;
        }

        for entry in self.forward_reference_names.iter() {
            if !self.named_capture_groups.contains(entry) {
                return true;
            }
        }

        false
    }

    fn reset_for_reparsing(&mut self) {
        debug_assert!(!has_error(self.error_code));

        self.delegate.reset_for_reparsing();
        self.index = 0;
        self.num_subpatterns = 0;
        self.max_seen_back_reference = 0;
        self.k_identity_escape_seen = false;
        self.parentheses_stack.clear();
        self.named_capture_groups.reset();
        self.forward_reference_names.clear();
    }

    // Funções auxiliares diversas:

    pub fn save_state(&self) -> ParseState {
        self.index
    }

    pub fn restore_state(&mut self, state: ParseState) {
        self.index = state;
    }

    pub fn at_end_of_pattern(&self) -> bool {
        debug_assert!(self.index <= self.size);
        self.index == self.size
    }

    fn pattern_remaining(&self) -> u32 {
        debug_assert!(self.index <= self.size);
        self.size - self.index
    }

    pub fn peek(&self) -> u32 {
        debug_assert!(self.index < self.size);
        self.data[self.index as usize].into()
    }

    pub fn peek_is_digit(&self) -> bool {
        !self.at_end_of_pattern() && is_ascii_digit(self.peek())
    }

    #[allow(dead_code)]
    fn peek_digit(&self) -> u32 {
        debug_assert!(self.peek_is_digit());
        self.peek() - '0' as u32
    }

    pub fn try_consume_unicode_escape(&mut self, context: UnicodeParseContext) -> u32 {
        debug_assert!(!has_error(self.error_code));

        let unicode_pattern_or_group_name =
            self.is_either_unicode_compilation() || context == UnicodeParseContext::GroupName;

        if !self.try_consume('u' as u16) || self.at_end_of_pattern() {
            if unicode_pattern_or_group_name {
                self.error_code = ErrorCode::InvalidUnicodeEscape;
            }
            return ERROR_CODE_POINT;
        }

        if unicode_pattern_or_group_name && self.try_consume('{' as u16) {
            let mut code_point: u32 = 0;
            loop {
                if self.at_end_of_pattern() || !is_ascii_hex_digit(self.peek()) {
                    self.error_code = ErrorCode::InvalidUnicodeCodePointEscape;
                    return ERROR_CODE_POINT;
                }

                let digit = self.consume();
                code_point = (code_point << 4) | to_ascii_hex_value(digit) as u32;

                if code_point > UCHAR_MAX_VALUE {
                    self.error_code = ErrorCode::InvalidUnicodeCodePointEscape;
                    return ERROR_CODE_POINT;
                }

                if self.at_end_of_pattern() || self.peek() == '}' as u32 {
                    break;
                }
            }

            if !self.try_consume('}' as u16) {
                self.error_code = ErrorCode::InvalidUnicodeCodePointEscape;
                return ERROR_CODE_POINT;
            }

            return code_point;
        }

        let code_unit = self.try_consume_hex(4);
        if code_unit == ERROR_CODE_POINT {
            if unicode_pattern_or_group_name {
                self.error_code = ErrorCode::InvalidUnicodeEscape;
            }
            return ERROR_CODE_POINT;
        }

        // Se temos o primeiro de um par substituto, procura o segundo.
        if crate::wtf::unicode::utf8_conversion::u16_is_lead(code_unit)
            && unicode_pattern_or_group_name
            && self.pattern_remaining() >= 6
            && self.peek() == '\\' as u32
        {
            let state = self.save_state();
            self.consume();

            if self.try_consume('u' as u16) {
                let surrogate2 = self.try_consume_hex(4);
                if crate::wtf::unicode::utf8_conversion::u16_is_trail(surrogate2) {
                    return crate::wtf::unicode::utf8_conversion::u16_get_supplementary(code_unit, surrogate2);
                }
            }

            self.restore_state(state);
        }

        code_unit
    }

    fn try_consume_identifier_character(&mut self) -> u32 {
        if self.try_consume('\\' as u16) {
            return self.try_consume_unicode_escape(UnicodeParseContext::GroupName);
        }

        self.consume_possible_surrogate_pair(UnicodeParseContext::GroupName)
    }

    fn is_identifier_start(ch: u32) -> bool {
        (is_ascii(ch) && (is_ascii_alpha(ch) || ch == '_' as u32 || ch == '$' as u32))
            || (u_get_gc_mask(ch) & U_GC_L_MASK) != 0
    }

    fn is_identifier_part(ch: u32) -> bool {
        (is_ascii(ch) && (is_ascii_alpha(ch) || ch == '_' as u32 || ch == '$' as u32))
            || (u_get_gc_mask(ch) & (U_GC_L_MASK | U_GC_MN_MASK | U_GC_MC_MASK | U_GC_ND_MASK | U_GC_PC_MASK)) != 0
            || ch == 0x200C
            || ch == 0x200D
    }

    fn is_unicode_property_value_expression_char(ch: u32) -> bool {
        is_ascii_alphanumeric(ch) || ch == '_' as u32 || ch == '=' as u32
    }

    pub fn consume(&mut self) -> u32 {
        debug_assert!(self.index < self.size);
        let ch = self.data[self.index as usize].into();
        self.index += 1;
        ch
    }

    fn consume_digit(&mut self) -> u32 {
        debug_assert!(self.peek_is_digit());
        self.consume() - '0' as u32
    }

    pub fn consume_number(&mut self) -> u32 {
        // `CheckedUint32`: depois de estourar, continua consumindo os dígitos e devolve o infinito.
        let mut n: Option<u32> = Some(self.consume_digit());
        while self.peek_is_digit() {
            let digit = self.consume_digit();
            n = n.and_then(|value| value.checked_mul(10)).and_then(|value| value.checked_add(digit));
        }
        n.unwrap_or(QUANTIFY_INFINITE)
    }

    fn consume_number64(&mut self) -> u64 {
        // `CheckedUint64`: idem.
        let mut n: Option<u64> = Some(self.consume_digit() as u64);
        while self.peek_is_digit() {
            let digit = self.consume_digit() as u64;
            n = n.and_then(|value| value.checked_mul(10)).and_then(|value| value.checked_add(digit));
        }
        n.unwrap_or(QUANTIFY_INFINITE64)
    }

    /// https://tc39.es/ecma262/#prod-annexB-LegacyOctalEscapeSequence
    pub fn consume_octal(&mut self, count: u32) -> u32 {
        let mut octal: u32 = 0;
        let mut remaining = count;
        while remaining != 0 {
            remaining -= 1;
            if !(octal < 32 && !self.at_end_of_pattern() && is_ascii_octal_digit(self.peek())) {
                break;
            }
            octal = octal * 8 + self.consume_digit();
        }
        octal
    }

    pub fn try_consume(&mut self, ch: u16) -> bool {
        if self.at_end_of_pattern() {
            return false;
        }
        let current: u32 = self.data[self.index as usize].into();
        if current != ch as u32 {
            return false;
        }
        self.index += 1;
        true
    }

    pub fn try_consume_hex(&mut self, count: u32) -> u32 {
        let state = self.save_state();

        let mut n: u32 = 0;
        let mut remaining = count;
        while remaining != 0 {
            remaining -= 1;
            if self.at_end_of_pattern() || !is_ascii_hex_digit(self.peek()) {
                self.restore_state(state);
                return ERROR_CODE_POINT;
            }
            let digit = self.consume();
            n = (n << 4) | to_ascii_hex_value(digit) as u32;
        }
        n
    }

    pub fn try_consume_group_name(&mut self) -> Option<String> {
        if self.at_end_of_pattern() {
            return None;
        }

        let state = self.save_state();

        let mut ch = self.try_consume_identifier_character();

        if Self::is_identifier_start(ch) {
            // `StringBuilder identifierBuilder`: unidades UTF-16; o resultado é Latin1 quando cabe.
            let mut identifier_builder: Vec<u16> = Vec::new();
            Self::append_code_point(&mut identifier_builder, ch);

            while !self.at_end_of_pattern() {
                ch = self.try_consume_identifier_character();
                if ch == '>' as u32 {
                    return Some(Self::identifier_builder_to_string(&identifier_builder));
                }

                if !Self::is_identifier_part(ch) {
                    break;
                }

                Self::append_code_point(&mut identifier_builder, ch);
            }
        }

        self.restore_state(state);

        None
    }

    /// `StringBuilder::append(char32_t)`.
    fn append_code_point(builder: &mut Vec<u16>, ch: u32) {
        if ch <= 0xFFFF {
            builder.push(ch as u16);
        } else {
            builder.push(((ch >> 10) + 0xD7C0) as u16);
            builder.push(((ch & 0x3FF) | 0xDC00) as u16);
        }
    }

    /// `StringBuilder::toString()`: fica em 8 bits enquanto todas as unidades couberem em Latin1.
    fn identifier_builder_to_string(builder: &[u16]) -> String {
        if builder.iter().all(|&unit| unit <= 0xFF) {
            let latin1: Vec<u8> = builder.iter().map(|&unit| unit as u8).collect();
            String::from_latin1(&latin1)
        } else {
            String::from_utf16(builder)
        }
    }

    pub fn try_consume_unicode_property_expression(&mut self) -> Option<BuiltInCharacterClassID> {
        if self.at_end_of_pattern() || !Self::is_unicode_property_value_expression_char(self.peek()) {
            self.error_code = ErrorCode::InvalidUnicodePropertyExpression;
            return None;
        }

        // Só entram caracteres ASCII (`isUnicodePropertyValueExpressionChar`), então Latin1 basta.
        let mut expression_builder: Vec<u8> = Vec::new();
        let mut unicode_property_name = String::default();
        let mut found_equals = false;
        let mut errors: u32 = 0;

        let first = self.consume();
        expression_builder.push(first as u8);

        while !self.at_end_of_pattern() {
            let ch = self.peek();
            if ch == '}' as u32 {
                self.consume();
                if errors != 0 {
                    self.error_code = ErrorCode::InvalidUnicodePropertyExpression;
                    return None;
                }

                if found_equals {
                    let result = crate::yarr::yarr_unicode_properties::unicode_match_property_value(
                        unicode_property_name,
                        String::from_latin1(&expression_builder),
                    );
                    if result.is_none() {
                        self.error_code = ErrorCode::InvalidUnicodePropertyExpression;
                    }
                    return result;
                }

                let result = crate::yarr::yarr_unicode_properties::unicode_match_property(
                    String::from_latin1(&expression_builder),
                    self.compile_mode,
                );
                if result.is_none() {
                    self.error_code = ErrorCode::InvalidUnicodePropertyExpression;
                }
                return result;
            }

            self.consume();
            if ch == '=' as u32 {
                if !found_equals {
                    found_equals = true;
                    unicode_property_name = String::from_latin1(&expression_builder);
                    expression_builder.clear();
                } else {
                    errors += 1;
                }
            } else if !Self::is_unicode_property_value_expression_char(ch) {
                errors += 1;
            } else {
                expression_builder.push(ch as u8);
            }
        }

        self.error_code = ErrorCode::InvalidUnicodePropertyExpression;
        None
    }

    fn count_captures(&mut self) {
        self.num_captures += 1;
        if self.num_captures > MAX_CAPTURES_COUNT {
            self.error_code = ErrorCode::TooManyCaptures;
        }
    }

    pub fn is_legacy_compilation(&self) -> bool {
        self.compile_mode == CompileMode::Legacy
    }

    pub fn is_unicode_compilation(&self) -> bool {
        self.compile_mode == CompileMode::Unicode
    }

    pub fn is_unicode_sets_compilation(&self) -> bool {
        self.compile_mode == CompileMode::UnicodeSets
    }

    pub fn is_either_unicode_compilation(&self) -> bool {
        self.is_unicode_compilation() || self.is_unicode_sets_compilation()
    }
}

/// `Yarr::compileMode(std::optional<OptionSet<Flags>>)`.
pub fn compile_mode(flags: Option<FlagSet>) -> CompileMode {
    // O C++ desreferencia o optional sem conferir: as flags chegam sempre presentes.
    let flags = flags.expect("compile_mode chamado sem flags");

    if flags.contains(Flags::Unicode) {
        return CompileMode::Unicode;
    }

    if flags.contains(Flags::UnicodeSets) {
        return CompileMode::UnicodeSets;
    }

    CompileMode::Legacy
}

/// `Yarr::parse()`:
///
/// Recebe um padrão a analisar e um delegate no qual serão feitas as chamadas que gravam os tokens
/// da regex. Devolve o código de erro do parse (`NoError` em caso de sucesso).
///
/// O delegate deve implementar o conceito `YarrSyntaxCheckable` (o trait `Delegate`). O C++ usa
/// `quantifyInfinite` e `true` como valores padrão de `backReferenceLimit` e
/// `isNamedForwardReferenceAllowed`; aqui o chamador os passa.
///
/// A regex é descrita por uma sequência de chamadas `assertion*()` e `atom*()` ao delegate. Depois de
/// um átomo pode vir um `quantify_atom()` indicando que o átomo anterior é quantificado. Em átomos
/// descritos por várias chamadas (parênteses e classes de caracteres), o `quantify_atom()` vem depois
/// do `atom*_end()`, nunca depois de `atom*_begin()`.
///
/// Classes de caracteres são descritas por uma única chamada a `atom_built_in_character_class()` ou
/// por uma sequência de `atom_character_class*()`: `..._begin()`, depois `..._atom()`, `..._range()`
/// e `..._built_in()`, e por fim `..._end()`.
///
/// Sequências de átomos e asserções se dividem em alternativas por chamadas a `disjunction()`.
/// Asserções, átomos e disjunções emitidos entre `atom_parentheses_subpattern_begin()` e
/// `atom_parentheses_end()` formam o corpo de um subpadrão.
pub fn parse<D: Delegate>(
    delegate: &mut D,
    pattern: &String,
    compile_mode: CompileMode,
    back_reference_limit: u32,
    is_named_forward_reference_allowed: bool,
) -> ErrorCode {
    if pattern.is_8bit() {
        return Parser::<D, u8>::new(
            delegate,
            pattern.span8(),
            compile_mode,
            back_reference_limit,
            is_named_forward_reference_allowed,
        )
        .parse();
    }
    Parser::<D, u16>::new(
        delegate,
        pattern.span16(),
        compile_mode,
        back_reference_limit,
        is_named_forward_reference_allowed,
    )
    .parse()
}

#[cfg(test)]
mod tests {
    //! Casos conferidos linha a linha contra `YarrParser.h`: ordem das verificações, qual erro
    //! vence quando dois são definidos, e a diferença entre as flags `u` e `v`.

    use super::*;
    use crate::yarr::yarr_flags::parse_flags;
    use crate::yarr::yarr_syntax_checker::SyntaxChecker;

    fn error_for(pattern: &str, flags: &str) -> ErrorCode {
        let parsed_flags = parse_flags(flags.as_bytes());
        assert!(parsed_flags.is_some(), "flags inválidas no teste: {flags}");
        let units: Vec<u16> = pattern.encode_utf16().collect();
        let pattern_string = String::from_utf16(&units);
        let mut checker = SyntaxChecker;
        parse(&mut checker, &pattern_string, compile_mode(parsed_flags), QUANTIFY_INFINITE, true)
    }

    fn assert_errors(cases: &[(&str, &str, ErrorCode)]) {
        for &(pattern, flags, expected) in cases {
            assert_eq!(error_for(pattern, flags), expected, "/{pattern}/{flags}");
        }
    }

    #[test]
    fn legacy_octal_and_backreferences() {
        assert_errors(&[
            (r"\1(a)", "", ErrorCode::NoError),
            (r"\2(a)", "", ErrorCode::NoError),
            (r"\2(a)", "u", ErrorCode::InvalidBackreference),
            (r"\01", "", ErrorCode::NoError),
            (r"\01", "u", ErrorCode::InvalidOctalEscape),
            (r"\8", "", ErrorCode::NoError),
            (r"\8", "u", ErrorCode::InvalidBackreference),
            (r"[\1]", "", ErrorCode::NoError),
            (r"[\1]", "u", ErrorCode::InvalidOctalEscape),
            (r"\0", "u", ErrorCode::NoError),
        ]);
    }

    #[test]
    fn control_letter_escapes() {
        assert_errors(&[
            (r"\c", "", ErrorCode::NoError),
            (r"\c", "u", ErrorCode::InvalidIdentityEscape),
            (r"\cA", "u", ErrorCode::NoError),
            (r"\c1", "", ErrorCode::NoError),
            (r"\c1", "u", ErrorCode::InvalidControlLetterEscape),
            (r"[\c_]", "", ErrorCode::NoError),
            (r"[\c_]", "u", ErrorCode::InvalidControlLetterEscape),
            (r"[\c1]", "", ErrorCode::NoError),
        ]);
    }

    #[test]
    fn character_class_ranges() {
        assert_errors(&[
            (r"[\d-a]", "", ErrorCode::NoError),
            (r"[\d-a]", "u", ErrorCode::CharacterClassRangeInvalid),
            (r"[a-\d]", "", ErrorCode::NoError),
            (r"[a-\d]", "u", ErrorCode::CharacterClassRangeInvalid),
            (r"[b-a]", "", ErrorCode::CharacterClassRangeOutOfOrder),
            (r"[a", "", ErrorCode::CharacterClassUnmatched),
            (r"[\-]", "u", ErrorCode::NoError),
            (r"\-", "u", ErrorCode::InvalidIdentityEscape),
            (r"\-", "", ErrorCode::NoError),
        ]);
    }

    #[test]
    fn unicode_escapes_and_property_escapes() {
        assert_errors(&[
            (r"\u{1F600}", "u", ErrorCode::NoError),
            (r"\u{110000}", "u", ErrorCode::InvalidUnicodeCodePointEscape),
            (r"\u{", "u", ErrorCode::InvalidUnicodeCodePointEscape),
            (r"\u{61}", "", ErrorCode::NoError),
            (r"\u12", "u", ErrorCode::InvalidUnicodeEscape),
            (r"\u12", "", ErrorCode::NoError),
            (r"😀", "u", ErrorCode::NoError),
            (r"\p", "u", ErrorCode::InvalidUnicodePropertyExpression),
            (r"\p{", "u", ErrorCode::InvalidUnicodePropertyExpression),
            (r"\p{Not_A_Property}", "u", ErrorCode::InvalidUnicodePropertyExpression),
            (r"\p", "", ErrorCode::NoError),
        ]);
    }

    #[test]
    fn named_groups_and_references() {
        assert_errors(&[
            (r"(?<a>x)(?<a>y)", "", ErrorCode::DuplicateGroupName),
            (r"(?<a>x)|(?<a>y)", "", ErrorCode::NoError),
            (r"(?:(?<a>x)|(?<a>y))\k<a>", "", ErrorCode::NoError),
            (r"(?<a>x)(?:(?<a>y))", "", ErrorCode::DuplicateGroupName),
            (r"(?<1a>.)", "", ErrorCode::InvalidGroupName),
            (r"(?<a", "", ErrorCode::InvalidGroupName),
            (r"(?<a\u{62}>.)\k<ab>", "", ErrorCode::NoError),
            (r"\k<a>(?<a>.)", "", ErrorCode::NoError),
            (r"\k<b>(?<a>.)", "", ErrorCode::InvalidNamedBackReference),
            (r"\k<a>", "", ErrorCode::NoError),
            (r"\k<a>", "u", ErrorCode::InvalidNamedBackReference),
            (r"(?<a>x)\k", "", ErrorCode::InvalidNamedBackReference),
            (r"\k(?<a>x)", "", ErrorCode::InvalidNamedBackReference),
            (r"\k", "u", ErrorCode::InvalidIdentityEscape),
        ]);
    }

    #[test]
    fn regexp_modifiers_lookbehind_and_quantifiers() {
        assert_errors(&[
            (r"(?i:a)", "", ErrorCode::NoError),
            (r"(?ims-ims:a)", "", ErrorCode::InvalidRegularExpressionModifier),
            (r"(?i-m:a)", "", ErrorCode::NoError),
            (r"(?-:a)", "", ErrorCode::InvalidRegularExpressionModifier),
            (r"(?i-i:a)", "", ErrorCode::InvalidRegularExpressionModifier),
            (r"(?ii:a)", "", ErrorCode::InvalidRegularExpressionModifier),
            (r"(?i--m:a)", "", ErrorCode::InvalidRegularExpressionModifier),
            (r"(?ix:a)", "", ErrorCode::ParenthesesTypeInvalid),
            (r"(?x:a)", "", ErrorCode::ParenthesesTypeInvalid),
            (r"(?", "", ErrorCode::ParenthesesTypeInvalid),
            (r"(?<=a)", "", ErrorCode::NoError),
            (r"(?<=a)*", "", ErrorCode::CantQuantifyAtom),
            (r"(?=a)*", "", ErrorCode::NoError),
            (r"(?=a)*", "u", ErrorCode::QuantifierWithoutAtom),
            (r"a{2,1}", "", ErrorCode::QuantifierOutOfOrder),
            (r"a{99999999999999999999}", "", ErrorCode::QuantifierTooLarge),
            (r"a{1", "", ErrorCode::NoError),
            (r"a{1", "u", ErrorCode::QuantifierIncomplete),
            (r"a**", "", ErrorCode::QuantifierWithoutAtom),
            (r"]", "", ErrorCode::NoError),
            (r"]", "u", ErrorCode::BracketUnmatched),
            (r"(?:a", "", ErrorCode::MissingParentheses),
            (r"a)", "", ErrorCode::ParenthesesUnmatched),
        ]);
    }

    #[test]
    fn unicode_sets_class_set_expressions() {
        assert_errors(&[
            (r"[a&&b]", "v", ErrorCode::NoError),
            (r"[a--b]", "v", ErrorCode::NoError),
            (r"[a&&&b]", "v", ErrorCode::InvalidClassSetCharacter),
            (r"[&&a]", "v", ErrorCode::InvalidClassSetOperation),
            (r"[a&&b--c]", "v", ErrorCode::InvalidClassSetOperation),
            (r"[ab&&c]", "v", ErrorCode::InvalidClassSetOperation),
            (r"[(]", "v", ErrorCode::InvalidClassSetCharacter),
            (r"[!!]", "v", ErrorCode::InvalidClassSetOperation),
            (r"[a-z]", "v", ErrorCode::NoError),
            (r"[z-a]", "v", ErrorCode::CharacterClassRangeOutOfOrder),
            (r"[[a-z]--[aeiou]]", "v", ErrorCode::NoError),
            (r"[\q{ab|c}]", "v", ErrorCode::NoError),
            (r"[^\q{ab}]", "v", ErrorCode::NegatedClassSetMayContainStrings),
            (r"[^\q{a|b}]", "v", ErrorCode::NoError),
            (r"[\q{a", "v", ErrorCode::ClassStringDisjunctionUnmatched),
            (r"[\q{-}]", "v", ErrorCode::InvalidClassSetCharacter),
            (r"[\q]", "v", ErrorCode::InvalidIdentityEscape),
            (r"[\&]", "v", ErrorCode::NoError),
            (r"[\&]", "u", ErrorCode::InvalidIdentityEscape),
            (r"\&", "v", ErrorCode::InvalidIdentityEscape),
            (r"[\d&&\w]", "v", ErrorCode::NoError),
            (r"[a", "v", ErrorCode::CharacterClassUnmatched),
            (r"[a-", "v", ErrorCode::CharacterClassUnmatched),
            (r"[a&&b", "v", ErrorCode::CharacterClassUnmatched),
            (r"[\d-a]", "v", ErrorCode::CharacterClassRangeInvalid),
        ]);
    }

    #[test]
    fn pattern_too_large() {
        let units = vec![b'a' as u16; MAX_PATTERN_SIZE as usize + 1];
        let pattern = String::from_utf16(&units);
        let mut checker = SyntaxChecker;
        assert_eq!(
            parse(&mut checker, &pattern, CompileMode::Legacy, QUANTIFY_INFINITE, true),
            ErrorCode::PatternTooLarge
        );
    }
}
