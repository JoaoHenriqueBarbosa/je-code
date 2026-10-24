// Fatia 5 de `yarr/YarrInterpreter.cpp` (linhas 2372 a 2985): a `ByteCompiler`, do `checkInput` ao fim do
// `emitDisjunction`. O construtor e o `compile` (linhas 2384 a 2411) pertencem à fatia anterior; a struct
// e os campos ficam declarados aqui porque o `.cpp` os declara no fim da classe (linhas 2977 a 2985).
//
// Incluída por `include!` no fim de `yarr_interpreter.rs`, sem `use`: caminhos completos.
//
// Modelo de posse (ver o cabeçalho de `yarr_interpreter.rs`):
// - `PatternDisjunction*` e `PatternAlternative*` viram `DisjunctionId` e índice de alternativa dentro dela.
// - `std::unique_ptr<ByteDisjunction> m_bodyDisjunction` é um `ByteDisjunction` por valor (o `regexBegin`
//   o substitui por um novo, como o `makeUnique`); quem monta o `BytecodePattern` o move com
//   `std::mem::replace`.
// - `Vector<std::unique_ptr<ByteDisjunction>> m_allParenthesesInfo` é `Vec<ByteDisjunction>`; o
//   `ByteDisjunction*` do termo é o `ByteDisjunctionId` da posição.
// - `CheckedUint32` é `u32` com `checked_add`/`checked_sub`: o C++ devolve `OffsetTooLarge` logo depois de
//   cada operação que estoura, então o estouro nunca sobrevive ao ponto da checagem. A soma
//   `unsigned + CheckedUint32` passada como `unsigned inputPosition` também vira `OffsetTooLarge` no estouro.
// - O trecho sob `ASSERT_ENABLED` (dump do `compile`) e os `ASSERT` somem.

// `ParenthesesStackEntry` e a struct `ByteCompiler` (com `body_disjunction: UniqueByteDisjunction`, o
// `unique_ptr<ByteDisjunction>`, que dá `Deref` para o `ByteDisjunction`) estão declaradas na fatia 4.

impl ByteCompiler<'_> {
    /// `m_bodyDisjunction->terms.last()`.
    fn last_term_mut(&mut self) -> &mut ByteTerm {
        let last = self.body_disjunction.terms.len() - 1;
        &mut self.body_disjunction.terms[last]
    }

    pub fn check_input(&mut self, count: u32) {
        self.body_disjunction.terms.push(ByteTerm::check_input(count, Default::default()));
    }

    pub fn uncheck_input(&mut self, count: u32) {
        self.body_disjunction.terms.push(ByteTerm::uncheck_input(count, Default::default()));
    }

    pub fn have_checked_input(&mut self, count: u32) {
        self.body_disjunction.terms.push(ByteTerm::have_checked_input(count, Default::default()));
    }

    pub fn assertion_bol(&mut self, input_position: u32, flags: crate::yarr::yarr_flags::FlagSet) {
        self.body_disjunction.terms.push(ByteTerm::bol(input_position, flags));
    }

    pub fn assertion_eol(&mut self, input_position: u32, flags: crate::yarr::yarr_flags::FlagSet) {
        self.body_disjunction.terms.push(ByteTerm::eol(input_position, flags));
    }

    pub fn assertion_word_boundary(
        &mut self,
        invert: bool,
        match_direction: crate::yarr::yarr_pattern::MatchDirection,
        input_position: u32,
        flags: crate::yarr::yarr_flags::FlagSet,
    ) {
        self.body_disjunction.terms.push(ByteTerm::word_boundary(invert, match_direction, input_position, flags));
    }

    pub fn atom_pattern_character(
        &mut self,
        ch: u32,
        match_direction: crate::yarr::yarr_pattern::MatchDirection,
        input_position: u32,
        frame_location: u32,
        quantity_max_count: u32,
        quantity_type: crate::yarr::yarr_pattern::QuantifierType,
        flags: crate::yarr::yarr_flags::FlagSet,
    ) {
        // For case-insesitive compares, non-ascii characters that have different
        // upper & lower case representations are converted to a character class.
        if flags.contains(crate::yarr::yarr_flags::Flags::IgnoreCase) && crate::wtf::ascii_ctype::is_ascii_alpha(ch) {
            let lo = crate::wtf::ascii_ctype::to_ascii_lower(ch as u8);
            let hi = crate::wtf::ascii_ctype::to_ascii_upper(ch as u8);
            if lo != hi {
                self.body_disjunction.terms.push(ByteTerm::new_cased_character(
                    lo as u32,
                    hi as u32,
                    input_position,
                    frame_location,
                    quantity_max_count,
                    quantity_type,
                    flags,
                ));
                self.last_term_mut().match_direction = match_direction;
                return;
            }
        }

        self.body_disjunction.terms.push(ByteTerm::new_character(
            ch,
            input_position,
            frame_location,
            quantity_max_count,
            quantity_type,
            flags,
        ));
        self.last_term_mut().match_direction = match_direction;
    }

    pub fn atom_character_class(
        &mut self,
        character_class: crate::yarr::yarr_pattern::CharacterClassId,
        invert: bool,
        match_direction: crate::yarr::yarr_pattern::MatchDirection,
        input_position: u32,
        frame_location: u32,
        quantity_max_count: u32,
        quantity_type: crate::yarr::yarr_pattern::QuantifierType,
        flags: crate::yarr::yarr_flags::FlagSet,
    ) {
        self.body_disjunction.terms.push(ByteTerm::new_character_class(character_class, invert, input_position, flags));

        let last = self.last_term_mut();
        if quantity_type != crate::yarr::yarr_pattern::QuantifierType::FixedCount {
            last.atom.quantity_min_count = 0;
        }
        last.atom.quantity_max_count = quantity_max_count;
        last.atom.quantity_type = quantity_type;
        last.frame_location = frame_location;
        last.match_direction = match_direction;
    }

    pub fn atom_back_reference(
        &mut self,
        is_named: bool,
        subpattern_id: u32,
        match_direction: crate::yarr::yarr_pattern::MatchDirection,
        input_position: u32,
        frame_location: u32,
        quantity_max_count: u32,
        quantity_type: crate::yarr::yarr_pattern::QuantifierType,
        flags: crate::yarr::yarr_flags::FlagSet,
    ) {
        debug_assert!(subpattern_id != 0);

        self.body_disjunction.terms.push(ByteTerm::back_reference(subpattern_id, match_direction, input_position, flags));

        if is_named && self.pattern.has_duplicate_named_capture_groups() {
            let duplicate_named_group_id = self.pattern.duplicate_named_group_for_subpattern_id[subpattern_id as usize];
            if duplicate_named_group_id != 0 {
                self.last_term_mut().atom.second_id = duplicate_named_group_id;
            }
        }

        let last = self.last_term_mut();
        last.atom.quantity_max_count = quantity_max_count;
        last.atom.quantity_type = quantity_type;
        last.frame_location = frame_location;
    }

    /// O corpo comum dos três `atomParentheses*Begin` (`OnceBegin`, `TerminalBegin`, `SubpatternBegin`):
    /// o C++ os repete idênticos, trocando só o tipo do primeiro termo. No `SubpatternBegin` o termo
    /// nasce `ParenthesesSubpatternOnceBegin` e é consertado no fim (bug 50136 do WebKit).
    fn atom_parentheses_begin(
        &mut self,
        type_: ByteTermType,
        subpattern_id: u32,
        match_direction: crate::yarr::yarr_pattern::MatchDirection,
        capture: bool,
        input_position: u32,
        frame_location: u32,
        alternative_frame_location: u32,
    ) {
        let begin_term = self.body_disjunction.terms.len() as u32;

        self.body_disjunction.terms.push(ByteTerm::new_subpattern_directed(
            type_,
            subpattern_id,
            capture,
            false,
            match_direction,
            input_position,
            self.current_flags,
        ));
        self.last_term_mut().frame_location = frame_location;
        self.body_disjunction.terms.push(ByteTerm::alternative_begin(self.current_flags));
        self.last_term_mut().frame_location = alternative_frame_location;

        self.parentheses_stack.push(ParenthesesStackEntry {
            begin_term,
            saved_alternative_index: self.current_alternative_index,
        });
        self.current_alternative_index = begin_term + 1;
    }

    pub fn atom_parentheses_once_begin(
        &mut self,
        subpattern_id: u32,
        match_direction: crate::yarr::yarr_pattern::MatchDirection,
        capture: bool,
        input_position: u32,
        frame_location: u32,
        alternative_frame_location: u32,
    ) {
        self.atom_parentheses_begin(
            ByteTermType::ParenthesesSubpatternOnceBegin,
            subpattern_id,
            match_direction,
            capture,
            input_position,
            frame_location,
            alternative_frame_location,
        );
    }

    pub fn atom_parentheses_terminal_begin(
        &mut self,
        subpattern_id: u32,
        match_direction: crate::yarr::yarr_pattern::MatchDirection,
        capture: bool,
        input_position: u32,
        frame_location: u32,
        alternative_frame_location: u32,
    ) {
        self.atom_parentheses_begin(
            ByteTermType::ParenthesesSubpatternTerminalBegin,
            subpattern_id,
            match_direction,
            capture,
            input_position,
            frame_location,
            alternative_frame_location,
        );
    }

    pub fn atom_parentheses_subpattern_begin(
        &mut self,
        subpattern_id: u32,
        match_direction: crate::yarr::yarr_pattern::MatchDirection,
        capture: bool,
        input_position: u32,
        frame_location: u32,
        alternative_frame_location: u32,
    ) {
        // Errrk! - this is a little crazy, we initially generate as a Type::ParenthesesSubpatternOnceBegin,
        // then fix this up at the end! - simplifying this should make it much clearer.
        // https://bugs.webkit.org/show_bug.cgi?id=50136
        self.atom_parentheses_begin(
            ByteTermType::ParenthesesSubpatternOnceBegin,
            subpattern_id,
            match_direction,
            capture,
            input_position,
            frame_location,
            alternative_frame_location,
        );
    }

    pub fn atom_parenthetical_assertion_begin(
        &mut self,
        subpattern_id: u32,
        invert: bool,
        match_direction: crate::yarr::yarr_pattern::MatchDirection,
        frame_location: u32,
        alternative_frame_location: u32,
    ) {
        let begin_term = self.body_disjunction.terms.len() as u32;

        self.body_disjunction.terms.push(ByteTerm::parenthetical_assertion_begin(
            subpattern_id,
            invert,
            match_direction,
            self.current_flags,
        ));
        self.last_term_mut().frame_location = frame_location;
        self.body_disjunction.terms.push(ByteTerm::alternative_begin(self.current_flags));
        self.last_term_mut().frame_location = alternative_frame_location;

        self.parentheses_stack.push(ParenthesesStackEntry {
            begin_term,
            saved_alternative_index: self.current_alternative_index,
        });
        self.current_alternative_index = begin_term + 1;
    }

    pub fn atom_parenthetical_assertion_end(
        &mut self,
        last_subpattern_id: u32,
        frame_location: u32,
        quantity_max_count: u32,
        quantity_type: crate::yarr::yarr_pattern::QuantifierType,
    ) {
        let begin_term = self.pop_parentheses_stack() as usize;
        self.close_alternative(begin_term as u32 + 1);
        let end_term = self.body_disjunction.terms.len();

        debug_assert!(self.body_disjunction.terms[begin_term].type_ == ByteTermType::ParentheticalAssertionBegin);

        let invert = self.body_disjunction.terms[begin_term].invert();
        let match_direction = self.body_disjunction.terms[begin_term].match_direction();
        let subpattern_id = self.body_disjunction.terms[begin_term].subpattern_id();

        self.body_disjunction.terms.push(ByteTerm::parenthetical_assertion_end(
            subpattern_id,
            last_subpattern_id,
            invert,
            match_direction,
            self.current_flags,
        ));
        let width = (end_term - begin_term) as u32;
        self.body_disjunction.terms[begin_term].atom.parentheses_width = width;
        self.body_disjunction.terms[end_term].atom.parentheses_width = width;
        self.body_disjunction.terms[end_term].frame_location = frame_location;

        self.body_disjunction.terms[begin_term].atom.quantity_max_count = quantity_max_count;
        self.body_disjunction.terms[begin_term].atom.quantity_type = quantity_type;
        self.body_disjunction.terms[end_term].atom.quantity_max_count = quantity_max_count;
        self.body_disjunction.terms[end_term].atom.quantity_type = quantity_type;
    }

    pub fn assertion_dot_star_enclosure(&mut self, bol_anchored: bool, eol_anchored: bool) {
        self.body_disjunction.terms.push(ByteTerm::dot_star_enclosure(bol_anchored, eol_anchored, self.current_flags));
    }

    pub fn pop_parentheses_stack(&mut self) -> u32 {
        debug_assert!(!self.parentheses_stack.is_empty());
        let entry = self.parentheses_stack.pop().expect("pilha de parênteses vazia (invariante do ByteCompiler)");
        self.current_alternative_index = entry.saved_alternative_index;

        debug_assert!((entry.begin_term as usize) < self.body_disjunction.terms.len());
        debug_assert!((self.current_alternative_index as usize) < self.body_disjunction.terms.len());

        entry.begin_term
    }

    pub fn close_alternative(&mut self, begin_term: u32) {
        let mut begin_term = begin_term as usize;
        let orig_begin_term = begin_term;
        debug_assert!(self.body_disjunction.terms[begin_term].type_ == ByteTermType::AlternativeBegin);
        let end_index = self.body_disjunction.terms.len();

        let frame_location = self.body_disjunction.terms[begin_term].frame_location;

        if self.body_disjunction.terms[begin_term].alternative.next == 0 {
            self.body_disjunction.terms.remove(begin_term);
        } else {
            while self.body_disjunction.terms[begin_term].alternative.next != 0 {
                begin_term =
                    begin_term.wrapping_add(self.body_disjunction.terms[begin_term].alternative.next as isize as usize);
                debug_assert!(self.body_disjunction.terms[begin_term].type_ == ByteTermType::AlternativeDisjunction);
                self.body_disjunction.terms[begin_term].alternative.end = end_index.wrapping_sub(begin_term) as i32;
                self.body_disjunction.terms[begin_term].frame_location = frame_location;
            }

            self.body_disjunction.terms[begin_term].alternative.next = orig_begin_term.wrapping_sub(begin_term) as i32;

            self.body_disjunction.terms.push(ByteTerm::alternative_end(self.current_flags));
            self.body_disjunction.terms[end_index].frame_location = frame_location;
        }
    }

    pub fn close_body_alternative(&mut self) {
        let mut begin_term = 0usize;
        let orig_begin_term = 0usize;
        debug_assert!(self.body_disjunction.terms[begin_term].type_ == ByteTermType::BodyAlternativeBegin);
        let end_index = self.body_disjunction.terms.len();

        let frame_location = self.body_disjunction.terms[begin_term].frame_location;

        while self.body_disjunction.terms[begin_term].alternative.next != 0 {
            begin_term = begin_term.wrapping_add(self.body_disjunction.terms[begin_term].alternative.next as isize as usize);
            debug_assert!(self.body_disjunction.terms[begin_term].type_ == ByteTermType::BodyAlternativeDisjunction);
            self.body_disjunction.terms[begin_term].alternative.end = end_index.wrapping_sub(begin_term) as i32;
            self.body_disjunction.terms[begin_term].frame_location = frame_location;
        }

        self.body_disjunction.terms[begin_term].alternative.next = orig_begin_term.wrapping_sub(begin_term) as i32;

        self.body_disjunction.terms.push(ByteTerm::body_alternative_end(self.current_flags));
        self.body_disjunction.terms[end_index].frame_location = frame_location;
    }

    /// `m_pattern.m_duplicateNamedGroupForSubpatternId[subpatternId]`, com o `hasDuplicateNamedCaptureGroups()`
    /// e o `capture` já testados pelo chamador: devolve 0 quando o grupo não é duplicado.
    fn duplicate_named_group_id_for(&self, subpattern_id: u32) -> u32 {
        self.pattern.duplicate_named_group_for_subpattern_id[subpattern_id as usize]
    }

    pub fn atom_parentheses_subpattern_end(
        &mut self,
        last_subpattern_id: u32,
        input_position: u32,
        frame_location: u32,
        quantity_min_count: u32,
        quantity_max_count: u32,
        quantity_type: crate::yarr::yarr_pattern::QuantifierType,
        call_frame_size: u32,
    ) {
        let begin_term = self.pop_parentheses_stack() as usize;
        self.close_alternative(begin_term as u32 + 1);
        let end_term = self.body_disjunction.terms.len();

        debug_assert!(self.body_disjunction.terms[begin_term].type_ == ByteTermType::ParenthesesSubpatternOnceBegin);

        let parentheses_begin = self.body_disjunction.terms[begin_term];

        let parentheses_match_direction = parentheses_begin.match_direction();
        let capture = parentheses_begin.capture();
        let subpattern_id = parentheses_begin.subpattern_id();

        // `unsigned` do C++: com grupo sem captura a conta dá a volta.
        let num_subpatterns = last_subpattern_id.wrapping_sub(subpattern_id).wrapping_add(1);
        let mut parentheses_disjunction = ByteDisjunction::new(num_subpatterns, call_frame_size);

        let first_term_in_parentheses = begin_term + 1;
        parentheses_disjunction.terms.reserve_exact(end_term - first_term_in_parentheses + 2);

        parentheses_disjunction.terms.push(ByteTerm::subpattern_begin(self.current_flags));
        for term_in_parentheses in first_term_in_parentheses..end_term {
            parentheses_disjunction.terms.push(self.body_disjunction.terms[term_in_parentheses]);
        }
        parentheses_disjunction.terms.push(ByteTerm::subpattern_end(self.current_flags));

        self.body_disjunction.terms.truncate(begin_term);

        let parentheses_id = ByteDisjunctionId(self.all_parentheses_info.len() as u32);
        self.body_disjunction.terms.push(ByteTerm::new_parentheses(
            ByteTermType::ParenthesesSubpattern,
            subpattern_id,
            parentheses_id,
            capture,
            input_position,
            self.current_flags,
        ));
        self.last_term_mut().match_direction = parentheses_match_direction;
        self.all_parentheses_info.push(parentheses_disjunction);

        if self.pattern.has_duplicate_named_capture_groups() && capture {
            let duplicate_named_group_id = self.duplicate_named_group_id_for(subpattern_id);
            if duplicate_named_group_id != 0 {
                self.body_disjunction.terms[begin_term].atom.second_id = duplicate_named_group_id;
            }
        }

        self.body_disjunction.terms[begin_term].atom.quantity_min_count = quantity_min_count;
        self.body_disjunction.terms[begin_term].atom.quantity_max_count = quantity_max_count;
        self.body_disjunction.terms[begin_term].atom.quantity_type = quantity_type;
        self.body_disjunction.terms[begin_term].frame_location = frame_location;
    }

    pub fn atom_parentheses_once_end(
        &mut self,
        input_position: u32,
        frame_location: u32,
        quantity_min_count: u32,
        quantity_max_count: u32,
        quantity_type: crate::yarr::yarr_pattern::QuantifierType,
    ) {
        let begin_term = self.pop_parentheses_stack() as usize;
        self.close_alternative(begin_term as u32 + 1);
        let end_term = self.body_disjunction.terms.len();

        debug_assert!(self.body_disjunction.terms[begin_term].type_ == ByteTermType::ParenthesesSubpatternOnceBegin);

        let capture = self.body_disjunction.terms[begin_term].capture();
        let subpattern_id = self.body_disjunction.terms[begin_term].subpattern_id();

        self.body_disjunction.terms.push(ByteTerm::new_subpattern(
            ByteTermType::ParenthesesSubpatternOnceEnd,
            subpattern_id,
            capture,
            false,
            input_position,
            self.current_flags,
        ));
        if self.body_disjunction.terms[begin_term].match_direction() == crate::yarr::yarr_pattern::MatchDirection::Backward {
            // Swap input positions for backward captures.
            self.body_disjunction.terms[end_term].input_position = self.body_disjunction.terms[begin_term].input_position;
            self.body_disjunction.terms[begin_term].input_position = input_position;
        }

        if self.pattern.has_duplicate_named_capture_groups() && self.body_disjunction.terms[begin_term].capture() {
            let duplicate_named_group_id = self.duplicate_named_group_id_for(subpattern_id);
            if duplicate_named_group_id != 0 {
                self.body_disjunction.terms[end_term].atom.second_id = duplicate_named_group_id;
                self.body_disjunction.terms[begin_term].atom.second_id = duplicate_named_group_id;
            }
        }

        let width = (end_term - begin_term) as u32;
        self.body_disjunction.terms[begin_term].atom.parentheses_width = width;
        self.body_disjunction.terms[end_term].atom.parentheses_width = width;
        self.body_disjunction.terms[end_term].frame_location = frame_location;
        self.body_disjunction.terms[end_term].match_direction = self.body_disjunction.terms[begin_term].match_direction();

        self.set_paren_quantity(begin_term, end_term, quantity_min_count, quantity_max_count, quantity_type);
    }

    pub fn atom_parentheses_terminal_end(
        &mut self,
        input_position: u32,
        frame_location: u32,
        quantity_min_count: u32,
        quantity_max_count: u32,
        quantity_type: crate::yarr::yarr_pattern::QuantifierType,
    ) {
        let mut input_position = input_position;
        let begin_term = self.pop_parentheses_stack() as usize;
        self.close_alternative(begin_term as u32 + 1);
        let end_term = self.body_disjunction.terms.len();

        debug_assert!(self.body_disjunction.terms[begin_term].type_ == ByteTermType::ParenthesesSubpatternTerminalBegin);

        if self.body_disjunction.terms[begin_term].match_direction() == crate::yarr::yarr_pattern::MatchDirection::Backward {
            input_position = 0;
        }
        let capture = self.body_disjunction.terms[begin_term].capture();
        let subpattern_id = self.body_disjunction.terms[begin_term].subpattern_id();

        self.body_disjunction.terms.push(ByteTerm::new_subpattern(
            ByteTermType::ParenthesesSubpatternTerminalEnd,
            subpattern_id,
            capture,
            false,
            input_position,
            self.current_flags,
        ));
        let width = (end_term - begin_term) as u32;
        self.body_disjunction.terms[begin_term].atom.parentheses_width = width;
        self.body_disjunction.terms[end_term].atom.parentheses_width = width;
        self.body_disjunction.terms[end_term].frame_location = frame_location;

        if self.pattern.has_duplicate_named_capture_groups() && self.body_disjunction.terms[begin_term].capture() {
            let duplicate_named_group_id = self.duplicate_named_group_id_for(subpattern_id);
            if duplicate_named_group_id != 0 {
                self.body_disjunction.terms[end_term].atom.second_id = duplicate_named_group_id;
                self.body_disjunction.terms[begin_term].atom.second_id = duplicate_named_group_id;
            }
        }

        self.set_paren_quantity(begin_term, end_term, quantity_min_count, quantity_max_count, quantity_type);
    }

    /// As seis atribuições de quantificador que `atomParenthesesOnceEnd` e `atomParenthesesTerminalEnd`
    /// repetem no fim, nos termos de abertura e de fechamento.
    fn set_paren_quantity(
        &mut self,
        begin_term: usize,
        end_term: usize,
        quantity_min_count: u32,
        quantity_max_count: u32,
        quantity_type: crate::yarr::yarr_pattern::QuantifierType,
    ) {
        for index in [begin_term, end_term] {
            let atom = &mut self.body_disjunction.terms[index].atom;
            atom.quantity_min_count = quantity_min_count;
            atom.quantity_max_count = quantity_max_count;
            atom.quantity_type = quantity_type;
        }
    }

    pub fn regex_begin(&mut self, num_subpatterns: u32, call_frame_size: u32, once_through: bool) {
        self.body_disjunction = UniqueByteDisjunction::new(ByteDisjunction::new(num_subpatterns, call_frame_size));
        self.body_disjunction.terms.push(ByteTerm::body_alternative_begin(once_through, self.current_flags));
        self.body_disjunction.terms[0].frame_location = 0;
        self.current_alternative_index = 0;
    }

    pub fn regex_end(&mut self) {
        self.close_body_alternative();
    }

    pub fn alternative_body_disjunction(&mut self, once_through: bool) {
        let new_alternative_index = self.body_disjunction.terms.len() as u32;
        self.body_disjunction.terms[self.current_alternative_index as usize].alternative.next =
            new_alternative_index.wrapping_sub(self.current_alternative_index) as i32;
        self.body_disjunction.terms.push(ByteTerm::body_alternative_disjunction(once_through, self.current_flags));

        self.current_alternative_index = new_alternative_index;
    }

    pub fn alternative_disjunction(&mut self) {
        let new_alternative_index = self.body_disjunction.terms.len() as u32;
        self.body_disjunction.terms[self.current_alternative_index as usize].alternative.next =
            new_alternative_index.wrapping_sub(self.current_alternative_index) as i32;
        self.body_disjunction.terms.push(ByteTerm::alternative_disjunction(self.current_flags));

        self.current_alternative_index = new_alternative_index;
    }

    /// `[[nodiscard]] std::optional<ErrorCode> emitDisjunction(PatternDisjunction*, CheckedUint32,
    /// unsigned, MatchDirection = Forward)`. O padrão `Forward` fica a cargo do chamador.
    pub fn emit_disjunction(
        &mut self,
        disjunction: crate::yarr::yarr_pattern::DisjunctionId,
        input_count_already_checked: u32,
        parentheses_input_count_already_checked: u32,
        match_direction: crate::yarr::yarr_pattern::MatchDirection,
    ) -> Option<crate::yarr::yarr_error_code::ErrorCode> {
        use crate::yarr::yarr::{
            YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHESES_ONCE, YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHESES_TERMINAL,
            YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHETICAL_ASSERTION,
        };
        use crate::yarr::yarr_error_code::ErrorCode;
        use crate::yarr::yarr_pattern::{MatchDirection, PatternTermType, QuantifierType};

        /// `currentCountAlreadyChecked - term.inputPosition` com o `hasOverflowed()` do C++.
        macro_rules! offset_from {
            ($current:expr, $term:expr) => {
                match $current.checked_sub($term.input_position) {
                    Some(value) => value,
                    None => return Some(ErrorCode::OffsetTooLarge),
                }
            };
        }
        /// Soma verificada de `unsigned` com `CheckedUint32` repassada como `unsigned`.
        macro_rules! checked_sum {
            ($a:expr, $b:expr) => {
                match $a.checked_add($b) {
                    Some(value) => value,
                    None => return Some(ErrorCode::OffsetTooLarge),
                }
            };
        }

        if !self.stack_check.is_safe_to_recurse() {
            return Some(ErrorCode::TooManyDisjunctions);
        }

        let alternative_count = self.pattern.disjunction(disjunction).alternatives.len();
        for alt in 0..alternative_count {
            let mut current_count_already_checked = input_count_already_checked;

            let (once_through, minimum_size, term_count) = {
                let alternative = &self.pattern.disjunction(disjunction).alternatives[alt];
                (alternative.once_through(), alternative.minimum_size, alternative.terms.len())
            };
            let disjunction_minimum_size = self.pattern.disjunction(disjunction).minimum_size;

            if alt != 0 {
                if disjunction == self.pattern.body {
                    self.alternative_body_disjunction(once_through);
                } else {
                    self.alternative_disjunction();
                }
            }

            debug_assert!(match_direction == MatchDirection::Backward || minimum_size >= parentheses_input_count_already_checked);
            let mut count_to_check = 0u32;
            let mut backward_uncheck_amount = 0u32;

            if match_direction == MatchDirection::Forward {
                count_to_check = minimum_size.wrapping_sub(parentheses_input_count_already_checked);
            } else {
                // Backward case
                let min_already_checked = disjunction_minimum_size.min(parentheses_input_count_already_checked);
                if minimum_size > min_already_checked {
                    count_to_check = minimum_size - min_already_checked;
                    let checked_input = checked_sum!(count_to_check, current_count_already_checked);
                    self.have_checked_input(checked_input);

                    if minimum_size > disjunction_minimum_size {
                        backward_uncheck_amount = count_to_check;
                    } else {
                        backward_uncheck_amount = minimum_size;
                    }
                }
            }

            if count_to_check != 0 {
                if match_direction == MatchDirection::Forward {
                    self.check_input(count_to_check);
                }

                current_count_already_checked = checked_sum!(current_count_already_checked, count_to_check);
            }

            for i in 0..term_count {
                let term_index = if match_direction == MatchDirection::Forward { i } else { term_count - 1 - i };
                let term = self.pattern.disjunction(disjunction).alternatives[alt].terms[term_index];

                match term.type_ {
                    PatternTermType::AssertionBOL => {
                        let current_input_position = offset_from!(current_count_already_checked, term);
                        self.assertion_bol(current_input_position, term.current_flags);
                    }

                    PatternTermType::AssertionEOL => {
                        let current_input_position = offset_from!(current_count_already_checked, term);
                        self.assertion_eol(current_input_position, term.current_flags);
                    }

                    PatternTermType::AssertionWordBoundary => {
                        let current_input_position = offset_from!(current_count_already_checked, term);
                        self.assertion_word_boundary(term.invert(), match_direction, current_input_position, term.current_flags);
                    }

                    PatternTermType::PatternCharacter => {
                        let current_input_position = offset_from!(current_count_already_checked, term);
                        self.atom_pattern_character(
                            term.pattern_character(),
                            match_direction,
                            current_input_position,
                            term.frame_location,
                            term.quantity_max_count,
                            term.quantity_type,
                            term.current_flags,
                        );
                    }

                    PatternTermType::CharacterClass => {
                        let current_input_position = offset_from!(current_count_already_checked, term);
                        self.atom_character_class(
                            term.character_class(),
                            term.invert(),
                            match_direction,
                            current_input_position,
                            term.frame_location,
                            term.quantity_max_count,
                            term.quantity_type,
                            term.current_flags,
                        );
                    }

                    PatternTermType::NumberedBackReference | PatternTermType::NamedBackReference => {
                        let current_input_position = offset_from!(current_count_already_checked, term);
                        self.atom_back_reference(
                            term.type_ == PatternTermType::NamedBackReference,
                            term.back_reference_subpattern_id(),
                            match_direction,
                            current_input_position,
                            term.frame_location,
                            term.quantity_max_count,
                            term.quantity_type,
                            term.current_flags,
                        );
                    }

                    PatternTermType::NumberedForwardReference | PatternTermType::NamedForwardReference => {}

                    PatternTermType::ParenthesesSubpattern => {
                        let parentheses = *term.parentheses();
                        let parentheses_minimum_size = self.pattern.disjunction(parentheses.disjunction).minimum_size;
                        let parentheses_call_frame_size = self.pattern.disjunction(parentheses.disjunction).call_frame_size;
                        let mut disjunction_already_checked_count = 0u32;
                        if term.quantity_max_count == 1 && !parentheses.is_copy {
                            let mut alternative_frame_location = term.frame_location;
                            // For QuantifierType::FixedCount we pre-check the minimum size; for greedy/non-greedy we reserve a slot in the frame.
                            if term.quantity_type == QuantifierType::FixedCount {
                                disjunction_already_checked_count = parentheses_minimum_size;
                            } else {
                                alternative_frame_location += YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHESES_ONCE;
                            }
                            let delegate_end_input_offset = offset_from!(current_count_already_checked, term);
                            self.atom_parentheses_once_begin(
                                parentheses.subpattern_id,
                                match_direction,
                                term.capture(),
                                checked_sum!(disjunction_already_checked_count, delegate_end_input_offset),
                                term.frame_location,
                                alternative_frame_location,
                            );
                            if let Some(error) = self.emit_disjunction(
                                parentheses.disjunction,
                                current_count_already_checked,
                                disjunction_already_checked_count,
                                match_direction,
                            ) {
                                return Some(error);
                            }
                            self.atom_parentheses_once_end(
                                delegate_end_input_offset,
                                term.frame_location,
                                term.quantity_min_count,
                                term.quantity_max_count,
                                term.quantity_type,
                            );
                        } else if parentheses.is_terminal {
                            let delegate_end_input_offset = offset_from!(current_count_already_checked, term);
                            self.atom_parentheses_terminal_begin(
                                parentheses.subpattern_id,
                                match_direction,
                                term.capture(),
                                checked_sum!(disjunction_already_checked_count, delegate_end_input_offset),
                                term.frame_location,
                                term.frame_location + YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHESES_TERMINAL,
                            );
                            if let Some(error) = self.emit_disjunction(
                                parentheses.disjunction,
                                current_count_already_checked,
                                disjunction_already_checked_count,
                                match_direction,
                            ) {
                                return Some(error);
                            }
                            self.atom_parentheses_terminal_end(
                                delegate_end_input_offset,
                                term.frame_location,
                                term.quantity_min_count,
                                term.quantity_max_count,
                                term.quantity_type,
                            );
                        } else {
                            let delegate_end_input_offset = offset_from!(current_count_already_checked, term);
                            self.atom_parentheses_subpattern_begin(
                                parentheses.subpattern_id,
                                match_direction,
                                term.capture(),
                                checked_sum!(disjunction_already_checked_count, delegate_end_input_offset),
                                term.frame_location,
                                0,
                            );
                            let input_offset = 0u32;
                            if let Some(error) = self.emit_disjunction(
                                parentheses.disjunction,
                                current_count_already_checked,
                                input_offset,
                                match_direction,
                            ) {
                                return Some(error);
                            }
                            self.atom_parentheses_subpattern_end(
                                parentheses.last_subpattern_id,
                                delegate_end_input_offset,
                                term.frame_location,
                                term.quantity_min_count,
                                term.quantity_max_count,
                                term.quantity_type,
                                parentheses_call_frame_size,
                            );
                        }
                    }

                    PatternTermType::ParentheticalAssertion => {
                        let parentheses = *term.parentheses();
                        let parentheses_minimum_size = self.pattern.disjunction(parentheses.disjunction).minimum_size;
                        let alternative_frame_location =
                            term.frame_location + YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHETICAL_ASSERTION;
                        let positive_input_offset = offset_from!(current_count_already_checked, term);
                        if term.match_direction() == MatchDirection::Forward {
                            let mut uncheck_amount = 0u32;
                            if positive_input_offset > parentheses_minimum_size {
                                uncheck_amount = positive_input_offset - parentheses_minimum_size;
                                self.uncheck_input(uncheck_amount);
                                current_count_already_checked = match current_count_already_checked.checked_sub(uncheck_amount) {
                                    Some(value) => value,
                                    None => return Some(ErrorCode::OffsetTooLarge),
                                };
                            }

                            self.atom_parenthetical_assertion_begin(
                                parentheses.subpattern_id,
                                term.invert(),
                                term.match_direction(),
                                term.frame_location,
                                alternative_frame_location,
                            );
                            if let Some(error) = self.emit_disjunction(
                                parentheses.disjunction,
                                current_count_already_checked,
                                positive_input_offset.wrapping_sub(uncheck_amount),
                                term.match_direction(),
                            ) {
                                return Some(error);
                            }
                            self.atom_parenthetical_assertion_end(
                                parentheses.last_subpattern_id,
                                term.frame_location,
                                term.quantity_max_count,
                                term.quantity_type,
                            );
                            if uncheck_amount != 0 {
                                self.check_input(uncheck_amount);
                                current_count_already_checked = checked_sum!(current_count_already_checked, uncheck_amount);
                            }
                        } else {
                            // Backward
                            let mut checked_count_for_lookbehind = offset_from!(current_count_already_checked, term);
                            let minimum_size = parentheses_minimum_size;
                            if minimum_size != 0 {
                                checked_count_for_lookbehind = checked_sum!(checked_count_for_lookbehind, minimum_size);
                                if checked_count_for_lookbehind > current_count_already_checked && !term.invert() {
                                    // Do a quick check for what is required for the lookbehind.
                                    // An inverted lookbehind can "match" without processing any input.
                                    self.have_checked_input(checked_count_for_lookbehind);
                                }
                            }
                            self.atom_parenthetical_assertion_begin(
                                parentheses.subpattern_id,
                                term.invert(),
                                term.match_direction(),
                                term.frame_location,
                                alternative_frame_location,
                            );

                            if let Some(error) = self.emit_disjunction(
                                parentheses.disjunction,
                                checked_count_for_lookbehind,
                                positive_input_offset.wrapping_add(minimum_size),
                                term.match_direction(),
                            ) {
                                return Some(error);
                            }
                            self.atom_parenthetical_assertion_end(
                                parentheses.last_subpattern_id,
                                term.frame_location,
                                term.quantity_max_count,
                                term.quantity_type,
                            );
                        }
                    }

                    PatternTermType::DotStarEnclosure => {
                        let anchors = *term.anchors();
                        self.assertion_dot_star_enclosure(anchors.bol_anchor, anchors.eol_anchor);
                    }
                }
            }

            if match_direction == MatchDirection::Backward && backward_uncheck_amount != 0 {
                self.uncheck_input(backward_uncheck_amount);
            }
        }
        None
    }
}
