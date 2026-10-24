// Fatia 2 de `yarr/YarrInterpreter.cpp` (linhas 602 a 1199): os métodos de `Interpreter<CharType>`
// de `checkSurrogatePair` até `matchParenthesesOnceBegin`. Incluída por `include!` no fim de
// `yarr_interpreter.rs`, por isso não tem `use` no topo.
//
// Usa o contrato unificado descrito no topo de `yarr_interpreter_cpp1.rs`: `DisjunctionContextRef`,
// `self.frame`/`set_frame`/`context`/`context_mut`, contextos de parênteses como `usize` e
// `BackTrackInfoParentheses` como cópia local `&mut`. `Self::test_character_class(&CharacterClass, u32)`
// e `INTERPRETER_ERROR_CODE_POINT` são os da fatia 1; `is_legacy_compilation()` e
// `is_either_unicode_compilation()` são métodos de `&self` da fatia 4.

impl<'a, C: crate::wtf::text::string_impl::CharType> Interpreter<'a, C> {
    fn check_surrogate_pair(&mut self, term: &ByteTerm, negative_input_offset: u32) -> bool {
        debug_assert!(term.is_character_type());
        term.pattern_character() == self.input.read_surrogate_pair_checked(negative_input_offset)
    }

    fn check_cased_character(&mut self, term: &ByteTerm, negative_input_offset: u32) -> bool {
        debug_assert!(term.is_cased_character_type());
        let ch = if term.match_direction() == MatchDirection::Forward {
            self.input.read_checked(negative_input_offset)
        } else {
            self.input.try_read_backward(negative_input_offset)
        };
        term.cased_character_lo() == ch || term.cased_character_hi() == ch
    }

    fn check_character_class(&mut self, term: &ByteTerm, negative_input_offset: u32) -> bool {
        debug_assert!(term.is_character_class());

        let input_char = if term.match_direction() == MatchDirection::Forward {
            self.input.read_checked(negative_input_offset)
        } else {
            self.input.try_read_backward(negative_input_offset)
        };
        if input_char == INTERPRETER_ERROR_CODE_POINT {
            return false;
        }

        let Some(character_class) = term.atom.character_class else {
            return false;
        };
        let matched = Self::test_character_class(self.pattern.character_class(character_class), input_char);
        if term.invert() { !matched } else { matched }
    }

    fn check_character_class_dont_advance_input_for_non_bmp(
        &mut self,
        term: &ByteTerm,
        negative_input_offset: u32,
    ) -> bool {
        debug_assert!(term.is_character_class());
        let Some(character_class_id) = term.atom.character_class else {
            return false;
        };
        let character_class = self.pattern.character_class(character_class_id);

        if term.match_direction() == MatchDirection::Backward && negative_input_offset > self.input.get_pos() {
            return false;
        }

        let read_character = if character_class.has_only_non_bmp_characters() {
            self.input.read_surrogate_pair_checked(negative_input_offset)
        } else {
            self.input.read_checked(negative_input_offset)
        };

        if read_character == INTERPRETER_ERROR_CODE_POINT {
            return false;
        }

        Self::test_character_class(character_class, read_character)
    }

    fn try_consume_back_reference(&mut self, match_begin: u32, match_end: u32, term: &ByteTerm) -> bool {
        let match_size = match_end.wrapping_sub(match_begin);

        if term.match_direction() == MatchDirection::Forward && !self.input.check_input(match_size) {
            return false;
        }

        let saved_pos = self.input.get_pos();

        let mut i: u32 = 0;
        while i < match_size {
            let negative_input_offset = term.input_position.wrapping_add(match_size).wrapping_sub(i);
            if term.match_direction() == MatchDirection::Backward && negative_input_offset > self.input.get_pos() {
                self.input.set_pos(saved_pos);
                return false;
            }

            let old_ch = self.input.reread(match_begin.wrapping_add(i));
            let ch;
            // `U_IS_BMP(oldCh)`.
            if old_ch > 0xffff {
                ch = self.input.read_surrogate_pair_checked(negative_input_offset);
                i += 1;
            } else {
                ch = if term.match_direction() == MatchDirection::Forward {
                    self.input.read_checked_dont_advance(negative_input_offset)
                } else {
                    self.input.try_read_backward(negative_input_offset)
                };
            }

            if old_ch != INTERPRETER_ERROR_CODE_POINT && ch != INTERPRETER_ERROR_CODE_POINT {
                if old_ch == ch {
                    i += 1;
                    continue;
                }

                if term.ignore_case() {
                    // Veja ES 6.0, 21.2.2.8.2 para a definição de Canonicalize(). Em padrões não
                    // Unicode, valores Unicode nunca casam com ASCII. Em Unicode, é preciso conferir
                    // todos os equivalentes canônicos de um caractere.
                    if self.is_legacy_compilation() && (old_ch <= 0x7f || ch <= 0x7f) {
                        if crate::wtf::ascii_ctype::to_ascii_upper(old_ch)
                            == crate::wtf::ascii_ctype::to_ascii_upper(ch)
                        {
                            i += 1;
                            continue;
                        }
                    } else if crate::yarr::yarr_canonicalize::are_canonically_equivalent(
                        old_ch,
                        ch,
                        if self.is_either_unicode_compilation() {
                            crate::yarr::yarr_canonicalize::CanonicalMode::Unicode
                        } else {
                            crate::yarr::yarr_canonicalize::CanonicalMode::UCS2
                        },
                    ) {
                        i += 1;
                        continue;
                    }
                }
            }

            if term.match_direction() == MatchDirection::Forward {
                self.input.uncheck_input(match_size);
            } else {
                self.input.set_pos(saved_pos);
            }

            return false;
        }

        if term.match_direction() == MatchDirection::Backward {
            self.input.uncheck_input(match_size);
        }

        true
    }

    fn match_assertion_bol(&self, term: &ByteTerm) -> bool {
        self.input.at_start_with_offset(term.input_position)
            || (term.multiline()
                && Self::test_character_class(
                    self.pattern.character_class(self.pattern.newline_character_class),
                    self.input.read_checked_dont_advance(term.input_position.wrapping_add(1)),
                ))
    }

    fn match_assertion_eol(&self, term: &ByteTerm) -> bool {
        let newline_character_class = self.pattern.character_class(self.pattern.newline_character_class);
        if term.input_position != 0 {
            return self.input.at_end_with_offset(term.input_position)
                || (term.multiline()
                    && Self::test_character_class(
                        newline_character_class,
                        self.input.read_checked_dont_advance(term.input_position),
                    ));
        }

        self.input.at_end()
            || (term.multiline() && Self::test_character_class(newline_character_class, self.input.read()))
    }

    fn match_assertion_word_boundary(&self, term: &ByteTerm) -> bool {
        let input_offset = term.input_position;

        let boundary_character_class = self.pattern.character_class(if term.ignore_case() {
            self.pattern.ignore_case_wordchar_character_class
        } else {
            self.pattern.wordchar_character_class
        });

        let prev_is_wordchar = !self.input.at_start_with_offset(input_offset)
            && Self::test_character_class(
                boundary_character_class,
                self.input.read_checked_dont_advance(input_offset.wrapping_add(1)),
            );
        let read_is_wordchar = if input_offset != 0 {
            !self.input.at_end_with_offset(input_offset)
                && Self::test_character_class(
                    boundary_character_class,
                    self.input.read_checked_dont_advance(input_offset),
                )
        } else {
            !self.input.at_end() && Self::test_character_class(boundary_character_class, self.input.read())
        };

        let word_boundary = prev_is_wordchar != read_is_wordchar;
        if term.invert() { !word_boundary } else { word_boundary }
    }

    fn backtrack_pattern_character(&mut self, term: &ByteTerm, context: DisjunctionContextRef) -> bool {
        let begin_slot =
            (term.frame_location + crate::yarr::yarr_pattern::BackTrackInfoPatternCharacter::begin_index()) as usize;
        let match_amount_slot = (term.frame_location
            + crate::yarr::yarr_pattern::BackTrackInfoPatternCharacter::match_amount_index())
            as usize;

        match term.atom.quantity_type {
            QuantifierType::FixedCount => {}

            QuantifierType::Greedy => {
                let match_amount = self.frame(context, match_amount_slot);
                if match_amount != 0 {
                    self.set_frame(context, match_amount_slot, match_amount - 1);
                    // `U16_LENGTH(term.atom.patternCharacter)`.
                    let length = if term.pattern_character() <= 0xffff { 1 } else { 2 };
                    if term.match_direction() == MatchDirection::Forward {
                        self.input.uncheck_input(length);
                    } else if !self.input.check_input(length) {
                        return false;
                    }
                    return true;
                }
            }

            QuantifierType::NonGreedy => {
                if term.match_direction() == MatchDirection::Forward {
                    let match_amount = self.frame(context, match_amount_slot);
                    if match_amount < term.atom.quantity_max_count as usize && self.input.check_input(1) {
                        self.set_frame(context, match_amount_slot, match_amount + 1);
                        if self.check_character(term, term.input_position.wrapping_add(1)) {
                            return true;
                        }
                    }
                    let begin = self.frame(context, begin_slot);
                    self.input.set_pos(begin as u32);
                    return false;
                }
                // matchDirection Backward
                let position = self.input.get_pos();

                if position < term.input_position {
                    return false;
                }

                let match_amount = self.frame(context, match_amount_slot);
                if match_amount < term.atom.quantity_max_count as usize && self.input.try_uncheck_input(1) {
                    self.set_frame(context, match_amount_slot, match_amount + 1);
                    if self.check_character(term, term.input_position) {
                        return true;
                    }
                }
                let begin = self.frame(context, begin_slot);
                self.input.set_pos(begin as u32);
            }
        }

        false
    }

    fn backtrack_pattern_cased_character(&mut self, term: &ByteTerm, context: DisjunctionContextRef) -> bool {
        let begin_slot =
            (term.frame_location + crate::yarr::yarr_pattern::BackTrackInfoPatternCharacter::begin_index()) as usize;
        let match_amount_slot = (term.frame_location
            + crate::yarr::yarr_pattern::BackTrackInfoPatternCharacter::match_amount_index())
            as usize;

        match term.atom.quantity_type {
            QuantifierType::FixedCount => {}

            QuantifierType::Greedy => {
                let match_amount = self.frame(context, match_amount_slot);
                if match_amount != 0 {
                    self.set_frame(context, match_amount_slot, match_amount - 1);
                    if term.match_direction() == MatchDirection::Forward {
                        self.input.uncheck_input(1);
                    } else if !self.input.check_input(1) {
                        return false;
                    }
                    return true;
                }
            }

            QuantifierType::NonGreedy => {
                if term.match_direction() == MatchDirection::Forward {
                    let match_amount = self.frame(context, match_amount_slot);
                    if match_amount < term.atom.quantity_max_count as usize && self.input.check_input(1) {
                        self.set_frame(context, match_amount_slot, match_amount + 1);
                        if self.check_cased_character(term, term.input_position.wrapping_add(1)) {
                            return true;
                        }
                    }
                    let match_amount = self.frame(context, match_amount_slot);
                    self.input.uncheck_input(match_amount as u32);
                    return false;
                }
                // matchDirection Backward
                let position = self.input.get_pos();

                if position < term.input_position {
                    return false;
                }

                let match_amount = self.frame(context, match_amount_slot);
                if match_amount < term.atom.quantity_max_count as usize && self.input.try_uncheck_input(1) {
                    self.set_frame(context, match_amount_slot, match_amount + 1);
                    if self.check_cased_character(term, term.input_position) {
                        return true;
                    }
                }
                let begin = self.frame(context, begin_slot);
                self.input.set_pos(begin as u32);
            }
        }

        false
    }

    fn match_character_class(&mut self, term: &ByteTerm, context: DisjunctionContextRef) -> bool {
        debug_assert!(term.type_ == ByteTermType::CharacterClass);
        let begin_slot =
            (term.frame_location + crate::yarr::yarr_pattern::BackTrackInfoCharacterClass::begin_index()) as usize;
        let match_amount_slot = (term.frame_location
            + crate::yarr::yarr_pattern::BackTrackInfoCharacterClass::match_amount_index())
            as usize;
        let Some(character_class_id) = term.atom.character_class else {
            return false;
        };
        let has_only_non_bmp_characters =
            self.pattern.character_class(character_class_id).has_only_non_bmp_characters();

        match term.atom.quantity_type {
            QuantifierType::FixedCount => {
                if term.match_direction() == MatchDirection::Forward {
                    if self.is_either_unicode_compilation() {
                        let begin = self.input.get_pos();
                        self.set_frame(context, begin_slot, begin as usize);
                        let mut match_amount: u32 = 0;
                        while match_amount < term.atom.quantity_max_count {
                            if term.invert() {
                                if !self.check_character_class(term, term.input_position.wrapping_sub(match_amount)) {
                                    self.input.set_pos(begin);
                                    return false;
                                }
                            } else {
                                let match_offset = match_amount * if has_only_non_bmp_characters { 2 } else { 1 };
                                if !self.check_character_class_dont_advance_input_for_non_bmp(
                                    term,
                                    term.input_position.wrapping_sub(match_offset),
                                ) {
                                    self.input.set_pos(begin);
                                    return false;
                                }
                            }
                            match_amount += 1;
                        }

                        return true;
                    }

                    let mut match_amount: u32 = 0;
                    while match_amount < term.atom.quantity_max_count {
                        if !self.check_character_class(term, term.input_position.wrapping_sub(match_amount)) {
                            return false;
                        }
                        match_amount += 1;
                    }
                    return true;
                }

                // matchDirection is Backward
                if self.is_either_unicode_compilation() {
                    let begin = self.input.get_pos();
                    self.set_frame(context, begin_slot, begin as usize);
                    let pairs_only = !term.invert() && has_only_non_bmp_characters;
                    let mut match_amount: u32 = 0;
                    while match_amount < term.atom.quantity_max_count {
                        let match_offset = term.atom.quantity_max_count - 1 - match_amount;
                        if pairs_only {
                            if !self.check_character_class_dont_advance_input_for_non_bmp(
                                term,
                                term.input_position.wrapping_sub(2 * match_offset),
                            ) {
                                self.input.set_pos(begin);
                                return false;
                            }
                        } else if !self.check_character_class(term, term.input_position.wrapping_sub(match_offset)) {
                            self.input.set_pos(begin);
                            return false;
                        }
                        match_amount += 1;
                    }

                    return true;
                }

                if self.input.get_pos() < term.input_position {
                    return false;
                }

                let mut match_amount: u32 = 0;
                while match_amount < term.atom.quantity_max_count {
                    if !self.check_character_class(
                        term,
                        term.input_position
                            .wrapping_sub(term.atom.quantity_max_count)
                            .wrapping_add(match_amount)
                            .wrapping_add(1),
                    ) {
                        return false;
                    }
                    match_amount += 1;
                }
                true
            }

            QuantifierType::Greedy => {
                let mut position = self.input.get_pos();
                let mut match_amount: u32 = 0;
                if term.match_direction() == MatchDirection::Forward {
                    while match_amount < term.atom.quantity_max_count && self.input.check_input(1) {
                        if !self.check_character_class(term, term.input_position.wrapping_add(1)) {
                            self.input.set_pos(position);
                            break;
                        }
                        match_amount += 1;
                        position = self.input.get_pos();
                    }
                    self.set_frame(context, match_amount_slot, match_amount as usize);
                    return true;
                }

                // matchDirection = Backward
                if self.input.get_pos() < term.input_position {
                    return false;
                }

                self.set_frame(context, begin_slot, position as usize);
                while match_amount < term.atom.quantity_max_count && self.input.try_uncheck_input(1) {
                    if !self.check_character_class(term, term.input_position) {
                        self.input.set_pos(position);
                        break;
                    }
                    match_amount += 1;
                    position = self.input.get_pos();
                }
                self.set_frame(context, match_amount_slot, match_amount as usize);
                true
            }

            QuantifierType::NonGreedy => {
                let begin = self.input.get_pos();
                self.set_frame(context, begin_slot, begin as usize);
                self.set_frame(context, match_amount_slot, 0);
                true
            }
        }
    }

    fn backtrack_character_class(&mut self, term: &ByteTerm, context: DisjunctionContextRef) -> bool {
        debug_assert!(term.type_ == ByteTermType::CharacterClass);
        let begin_slot =
            (term.frame_location + crate::yarr::yarr_pattern::BackTrackInfoCharacterClass::begin_index()) as usize;
        let match_amount_slot = (term.frame_location
            + crate::yarr::yarr_pattern::BackTrackInfoCharacterClass::match_amount_index())
            as usize;

        match term.atom.quantity_type {
            QuantifierType::FixedCount => {
                if self.is_either_unicode_compilation() {
                    let begin = self.frame(context, begin_slot);
                    self.input.set_pos(begin as u32);
                }
            }

            QuantifierType::Greedy => {
                let match_amount = self.frame(context, match_amount_slot);
                if match_amount != 0 {
                    if self.is_either_unicode_compilation() {
                        // Desfaz um ponto de código.
                        if term.match_direction() == MatchDirection::Forward {
                            self.set_frame(context, match_amount_slot, match_amount - 1);
                            self.input.uncheck_input(1);
                            // Só o efeito colateral (o recuo de um par substituto) interessa.
                            let _ = self.input.try_read_backward(term.input_position);
                            return true;
                        }
                        // matchDirection Backwards
                        self.set_frame(context, match_amount_slot, match_amount - 1);
                        if match_amount - 1 == 0 {
                            let begin = self.frame(context, begin_slot);
                            self.input.set_pos(begin as u32);
                            return true;
                        }
                        // Só o efeito colateral (o avanço de um par substituto) interessa.
                        let _ = self.input.read_checked(term.input_position);
                        self.input.check_input(1);
                        return true;
                    }
                    self.set_frame(context, match_amount_slot, match_amount - 1);
                    if term.match_direction() == MatchDirection::Forward {
                        self.input.uncheck_input(1);
                    } else {
                        self.input.check_input(1);
                    }
                    return true;
                }
            }

            QuantifierType::NonGreedy => {
                if term.match_direction() == MatchDirection::Forward {
                    let match_amount = self.frame(context, match_amount_slot);
                    if match_amount < term.atom.quantity_max_count as usize && self.input.check_input(1) {
                        self.set_frame(context, match_amount_slot, match_amount + 1);
                        if self.check_character_class(term, term.input_position.wrapping_add(1)) {
                            return true;
                        }
                    }
                    let begin = self.frame(context, begin_slot);
                    self.input.set_pos(begin as u32);
                    return false;
                }
                // matchDirection Backward
                let match_amount = self.frame(context, match_amount_slot);
                if match_amount < term.atom.quantity_max_count as usize && self.input.try_uncheck_input(1) {
                    self.set_frame(context, match_amount_slot, match_amount + 1);
                    if self.check_character_class(term, term.input_position) {
                        return true;
                    }
                }
                let begin = self.frame(context, begin_slot);
                self.input.set_pos(begin as u32);
            }
        }

        false
    }

    fn match_back_reference(&mut self, term: &ByteTerm, context: DisjunctionContextRef) -> bool {
        debug_assert!(term.type_ == ByteTermType::BackReference);
        let begin_slot =
            (term.frame_location + crate::yarr::yarr_pattern::BackTrackInfoBackReference::begin_index()) as usize;
        let match_amount_slot = (term.frame_location
            + crate::yarr::yarr_pattern::BackTrackInfoBackReference::match_amount_index())
            as usize;

        // Inicializa o backtracking antes de conferir os possíveis casamentos nulos.
        match term.atom.quantity_type {
            QuantifierType::NonGreedy => {
                self.set_frame(context, match_amount_slot, 0);
                let begin = self.input.get_pos();
                self.set_frame(context, begin_slot, begin as usize);
            }

            QuantifierType::FixedCount => {
                let begin = self.input.get_pos();
                self.set_frame(context, begin_slot, begin as usize);
            }

            QuantifierType::Greedy => {
                self.set_frame(context, match_amount_slot, 0);
            }
        }

        let subpattern_id;

        let duplicate_named_group_id = term.duplicate_named_group_id();
        if duplicate_named_group_id != 0 {
            subpattern_id =
                self.output[self.pattern.offset_for_duplicate_named_group_id(duplicate_named_group_id) as usize];
            if subpattern_id < 1 {
                // Se nenhum subpadrão casou, a string a casar é vazia.
                return true;
            }
        } else {
            subpattern_id = term.subpattern_id();
        }

        let match_begin = self.output[(subpattern_id << 1) as usize];
        let match_end = self.output[((subpattern_id << 1) + 1) as usize];

        // Se o fim do casamento referenciado ainda não foi definido, a referência está dentro dos
        // próprios parênteses que ela referencia. O resultado é a string vazia, como na referência a
        // um parêntese de largura zero. Ex.: /(a\1)/
        if match_end == crate::yarr::yarr::OFFSET_NO_MATCH {
            return true;
        }

        if match_begin == crate::yarr::yarr::OFFSET_NO_MATCH {
            return true;
        }

        debug_assert!(match_begin <= match_end);

        if match_begin == match_end {
            return true;
        }

        match term.atom.quantity_type {
            QuantifierType::FixedCount => {
                let mut match_amount: u32 = 0;
                while match_amount < term.atom.quantity_max_count {
                    if !self.try_consume_back_reference(match_begin, match_end, term) {
                        let begin = self.frame(context, begin_slot);
                        self.input.set_pos(begin as u32);
                        return false;
                    }
                    match_amount += 1;
                }
                true
            }

            QuantifierType::Greedy => {
                let mut match_amount: u32 = 0;
                while match_amount < term.atom.quantity_max_count
                    && self.try_consume_back_reference(match_begin, match_end, term)
                {
                    match_amount += 1;
                }
                self.set_frame(context, match_amount_slot, match_amount as usize);
                true
            }

            QuantifierType::NonGreedy => true,
        }
    }

    fn backtrack_back_reference(&mut self, term: &ByteTerm, context: DisjunctionContextRef) -> bool {
        debug_assert!(term.type_ == ByteTermType::BackReference);
        let begin_slot =
            (term.frame_location + crate::yarr::yarr_pattern::BackTrackInfoBackReference::begin_index()) as usize;
        let match_amount_slot = (term.frame_location
            + crate::yarr::yarr_pattern::BackTrackInfoBackReference::match_amount_index())
            as usize;

        let subpattern_id;

        let duplicate_named_group_id = term.duplicate_named_group_id();
        if duplicate_named_group_id != 0 {
            subpattern_id =
                self.output[self.pattern.offset_for_duplicate_named_group_id(duplicate_named_group_id) as usize];
            if subpattern_id < 1 {
                // Se nenhum subpadrão casou, a string a casar é vazia.
                return false;
            }
        } else {
            subpattern_id = term.subpattern_id();
        }

        let match_begin = self.output[(subpattern_id << 1) as usize];
        let match_end = self.output[((subpattern_id << 1) + 1) as usize];

        if match_begin == crate::yarr::yarr::OFFSET_NO_MATCH || match_end == crate::yarr::yarr::OFFSET_NO_MATCH {
            return false;
        }

        debug_assert!(match_begin <= match_end);

        if match_begin == match_end {
            return false;
        }

        match term.atom.quantity_type {
            QuantifierType::FixedCount => {
                // Para quantityMaxCount == 1, poderia só recuar.
                let begin = self.frame(context, begin_slot);
                self.input.set_pos(begin as u32);
            }

            QuantifierType::Greedy => {
                let match_amount = self.frame(context, match_amount_slot);
                if match_amount != 0 {
                    self.set_frame(context, match_amount_slot, match_amount - 1);
                    if term.match_direction() == MatchDirection::Backward {
                        return self.input.check_input(match_end - match_begin);
                    }
                    self.input.rewind(match_end - match_begin);
                    return true;
                }
            }

            QuantifierType::NonGreedy => {
                let match_amount = self.frame(context, match_amount_slot);
                if match_amount < term.atom.quantity_max_count as usize
                    && self.try_consume_back_reference(match_begin, match_end, term)
                {
                    self.set_frame(context, match_amount_slot, match_amount + 1);
                    return true;
                }
                let begin = self.frame(context, begin_slot);
                self.input.set_pos(begin as u32);
            }
        }

        false
    }

    pub fn record_parentheses_match(&mut self, term: &ByteTerm, context: usize) {
        if term.capture() {
            let subpattern_id = term.subpattern_id();
            let disjunction_context = DisjunctionContextRef::Parentheses(context);
            let match_begin = self.context(disjunction_context).match_begin;
            let match_end = self.context(disjunction_context).match_end;
            // Em casamentos Backward, os índices capturados são gravados fim e depois início.
            self.output[((subpattern_id << 1) as usize) + term.match_direction() as usize] =
                match_begin.wrapping_sub(term.input_position);
            self.output[((subpattern_id << 1) as usize) + 1 - term.match_direction() as usize] =
                match_end.wrapping_sub(term.input_position);

            if term.duplicate_named_group_id() != 0 {
                // Registra qual dos subpadrões nomeados duplicados casou.
                let offset = self.pattern.offset_for_duplicate_named_group_id(term.duplicate_named_group_id());
                self.output[offset as usize] = subpattern_id;
            }
        }
    }

    pub fn reset_matches(&mut self, term: &ByteTerm, context: usize) {
        let first_subpattern_id = term.subpattern_id();
        let pattern = self.pattern;
        self.parentheses_contexts[context].restore_output(pattern, &mut *self.output, first_subpattern_id);
    }

    pub fn parentheses_do_backtrack(
        &mut self,
        term: &ByteTerm,
        back_track: &mut BackTrackInfoParentheses,
    ) -> crate::yarr::yarr::JSRegExpResult {
        use crate::yarr::yarr::JSRegExpResult;

        let pattern = self.pattern;
        let Some(body_id) = term.atom.parentheses_disjunction else {
            return JSRegExpResult::ErrorInternal;
        };
        let disjunction_body = pattern.parentheses_disjunction(body_id);

        while back_track.match_amount != 0 {
            let Some(context) = back_track.last_context else {
                return JSRegExpResult::ErrorInternal;
            };

            let result = self.match_disjunction(
                disjunction_body,
                DisjunctionContextRef::Parentheses(context),
                /* btrack= */ true,
            );
            if result == JSRegExpResult::Match {
                return JSRegExpResult::Match;
            }

            self.reset_matches(term, context);
            Self::pop_parentheses_disjunction_context(back_track, &self.parentheses_contexts);
            self.free_parentheses_disjunction_context(context);

            if result != JSRegExpResult::NoMatch {
                return result;
            }
        }

        JSRegExpResult::NoMatch
    }

    pub fn match_parentheses_once_begin(&mut self, term: &ByteTerm, context: DisjunctionContextRef) -> bool {
        debug_assert!(term.type_ == ByteTermType::ParenthesesSubpatternOnceBegin);
        debug_assert!(term.atom.quantity_max_count == 1);

        let begin_slot = (term.frame_location
            + crate::yarr::yarr_pattern::BackTrackInfoParenthesesOnce::begin_index())
            as usize;

        match term.atom.quantity_type {
            QuantifierType::Greedy => {
                // Grava de forma especulativa; se chegar ao fim dos parênteses, será verdade.
                let begin = self.input.get_pos();
                self.set_frame(context, begin_slot, begin as usize);
            }
            QuantifierType::NonGreedy => {
                self.set_frame(context, begin_slot, crate::wtf::text::string_common::NOT_FOUND);
                self.context_mut(context).term += term.atom.parentheses_width as i32;
                return true;
            }
            QuantifierType::FixedCount => {}
        }

        if term.capture() {
            let subpattern_id = term.subpattern_id();
            // Em casamentos Backward, os índices capturados são gravados fim e depois início.
            self.output[((subpattern_id << 1) as usize) + term.match_direction() as usize] =
                self.input.get_pos().wrapping_sub(term.input_position);
        }

        true
    }
}
