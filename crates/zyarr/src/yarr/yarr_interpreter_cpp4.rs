// Fatia 4 de `yarr/YarrInterpreter.cpp` (linhas 1817 a 2411): `matchDisjunction`,
// `matchNonZeroDisjunction`, `interpret`, o construtor do `Interpreter`, os predicados de modo de
// compilação, `isSafeToRecurse`, e o início da classe `ByteCompiler` (a struct, o construtor e
// `compile`). Incluída por `include!` no fim de `yarr_interpreter.rs`.
//
// # Como o `goto` virou laço
//
// `matchDisjunction` tem dois rótulos, `matchAgain` e `backtrack`, e as macros `MATCH_NEXT` e
// `BACKTRACK` saltam para eles depois de mexer em `context->term`. Aqui há um `loop` com a variável
// `backtracking` dizendo em qual dos dois `switch` estamos: `match_next!()` incrementa o termo, põe
// `backtracking = false` e volta ao topo; `backtrack!()` decrementa, põe `backtracking = true` e volta.
// O `btrack` do C++ só servia de estado para o `dataLog` (`verbose` é `false` em tempo de compilação,
// e `dataLog` não tem efeito observável), então o parâmetro `btrack` só decide o ponto de entrada.
// As macros `DUMP_*` e o `ByteTermDumper termDumper` do corpo existem só sob `verbose` e não são
// portadas.
//
// # Contexto e disjunção
//
// `DisjunctionContext*` é `DisjunctionContextRef` (ver o contrato no topo de `yarr_interpreter_cpp1.rs`).
// `ByteDisjunction*` é `&ByteDisjunction` emprestado do `BytecodePattern` (que vive mais que o
// `Interpreter`), por isso não conflita com o `&mut self`. `currentTerm()` é uma cópia do `ByteTerm`
// (o tipo é `Copy`) relida a cada uso, como o C++ relê a referência.
//
// # O que não é portado
//
// `ConcurrentJSLocker` (a trava só protege o JIT, que não existe aqui) e o
// `BumpPointerPool` (`startAllocator`/`stopAllocator`, `RELEASE_ASSERT(allocatorPool)`): a memória
// dos contextos é a do Rust, ver `alloc_disjunction_context`. `Options::dumpCompiledRegExpPatterns`
// está sob `ASSERT_ENABLED` e some.

impl<'a, C: crate::wtf::text::string_impl::CharType> Interpreter<'a, C> {
    /// `matchDisjunction(disjunction, context, btrack)`.
    #[allow(unreachable_code)]
    pub fn match_disjunction(
        &mut self,
        disjunction: &ByteDisjunction,
        context: DisjunctionContextRef,
        btrack: bool,
    ) -> crate::yarr::yarr::JSRegExpResult {
        use crate::yarr::yarr::JSRegExpResult;
        use crate::yarr::yarr_pattern::{
            BackTrackInfoPatternCharacter, MatchDirection,
        };

        if !self.is_safe_to_recurse() {
            return JSRegExpResult::ErrorNoMemory;
        }

        self.remaining_match_count = self.remaining_match_count.wrapping_sub(1);
        if self.remaining_match_count == 0 {
            return JSRegExpResult::ErrorHitLimit;
        }

        let pattern = self.pattern;
        let mut backtracking = btrack;

        macro_rules! ctx {
            () => {
                self.context_mut(context)
            };
        }
        macro_rules! current_term {
            () => {
                disjunction.terms[ctx!().term as usize]
            };
        }
        if backtracking {
            ctx!().term -= 1;
        } else {
            ctx!().match_begin = self.input.get_pos();
            ctx!().term = 0;
        }

        'main: loop {
            macro_rules! match_next {
                () => {{
                    ctx!().term += 1;
                    backtracking = false;
                    continue 'main;
                }};
            }
            macro_rules! backtrack {
                () => {{
                    ctx!().term -= 1;
                    backtracking = true;
                    continue 'main;
                }};
            }
            debug_assert!((ctx!().term as usize) < disjunction.terms.len());
            let term = current_term!();

            if !backtracking {
                // matchAgain:
                match term.type_ {
                    ByteTermType::SubpatternBegin => match_next!(),
                    ByteTermType::SubpatternEnd => {
                        ctx!().match_end = self.input.get_pos();
                        return JSRegExpResult::Match;
                    }

                    ByteTermType::BodyAlternativeBegin => match_next!(),
                    ByteTermType::BodyAlternativeDisjunction | ByteTermType::BodyAlternativeEnd => {
                        ctx!().match_end = self.input.get_pos();
                        return JSRegExpResult::Match;
                    }

                    ByteTermType::AlternativeBegin => match_next!(),
                    ByteTermType::AlternativeDisjunction | ByteTermType::AlternativeEnd => {
                        let offset = term.alternative.end;
                        // `BackTrackInfoAlternative::offset` é um `uintptr_t` (o `int` é estendido com sinal).
                        ctx!().frame[term.frame_location as usize] = offset as isize as usize;
                        ctx!().term = ctx!().term.wrapping_add(offset);
                        match_next!();
                    }

                    ByteTermType::AssertionBOL => {
                        if self.match_assertion_bol(&term) {
                            match_next!();
                        }
                        backtrack!();
                    }
                    ByteTermType::AssertionEOL => {
                        if self.match_assertion_eol(&term) {
                            match_next!();
                        }
                        backtrack!();
                    }
                    ByteTermType::AssertionWordBoundary => {
                        if self.match_assertion_word_boundary(&term) {
                            match_next!();
                        }
                        backtrack!();
                    }

                    ByteTermType::PatternCharacterOnce | ByteTermType::PatternCharacterFixed => {
                        if term.match_direction() == MatchDirection::Forward {
                            if self.is_either_unicode_compilation() {
                                if term.atom.first_id > 0xFFFF {
                                    for match_amount in 0..term.atom.quantity_max_count {
                                        if !self.check_surrogate_pair(
                                            &term,
                                            term.input_position.wrapping_sub(2u32.wrapping_mul(match_amount)),
                                        ) {
                                            backtrack!();
                                        }
                                    }
                                    match_next!();
                                }
                            }

                            let position = self.input.get_pos(); // Pode ser preciso desfazer a leitura de um par substituto.

                            for match_amount in 0..term.atom.quantity_max_count {
                                if !self.check_character(&term, term.input_position.wrapping_sub(match_amount)) {
                                    self.input.set_pos(position);
                                    backtrack!();
                                }
                            }
                        } else {
                            if self.is_either_unicode_compilation() {
                                if term.atom.first_id > 0xFFFF {
                                    for match_amount in 0..term.atom.quantity_max_count {
                                        let input_position =
                                            term.input_position.wrapping_sub(2u32.wrapping_mul(match_amount));
                                        if self.input.get_pos() < input_position {
                                            backtrack!();
                                        }
                                        if !self.check_surrogate_pair(&term, input_position) {
                                            backtrack!();
                                        }
                                    }
                                    match_next!();
                                }
                            }

                            if self.input.get_pos() < term.input_position {
                                backtrack!();
                            }

                            let position = self.input.get_pos(); // Pode ser preciso desfazer a leitura de um par substituto.

                            for match_amount in 0..term.atom.quantity_max_count {
                                if !self.check_character(
                                    &term,
                                    term.input_position
                                        .wrapping_add(match_amount)
                                        .wrapping_add(1)
                                        .wrapping_sub(term.atom.quantity_max_count),
                                ) {
                                    self.input.set_pos(position);
                                    backtrack!();
                                }
                            }
                        }
                        match_next!();
                    }
                    ByteTermType::PatternCharacterGreedy => {
                        let back_track_index = term.frame_location as usize
                            + BackTrackInfoPatternCharacter::match_amount_index() as usize;
                        let mut match_amount: u32 = 0;
                        let mut position = self.input.get_pos(); // Pode ser preciso desfazer a leitura de um par substituto.
                        if term.match_direction() == MatchDirection::Forward {
                            while match_amount < term.atom.quantity_max_count && self.input.check_input(1) {
                                if !self.check_character(&term, term.input_position.wrapping_add(1)) {
                                    self.input.set_pos(position);
                                    break;
                                }
                                match_amount += 1;
                                position = self.input.get_pos();
                            }
                        } else {
                            if self.input.get_pos() < term.input_position {
                                backtrack!();
                            }

                            while match_amount < term.atom.quantity_max_count && self.input.try_uncheck_input(1) {
                                if !self.check_character(&term, term.input_position) {
                                    self.input.set_pos(position);
                                    break;
                                }
                                match_amount += 1;
                                position = self.input.get_pos();
                            }
                        }
                        ctx!().frame[back_track_index] = match_amount as usize;

                        match_next!();
                    }

                    ByteTermType::PatternCasedCharacterNonGreedy | ByteTermType::PatternCharacterNonGreedy => {
                        if term.type_ == ByteTermType::PatternCasedCharacterNonGreedy {
                            // A comparação sem diferenciar maiúsculas de caracteres unicode é tratada como Type::CharacterClass.
                            debug_assert!(!self.is_either_unicode_compilation() || term.atom.first_id <= 0xFFFF);
                        }
                        let begin_index = term.frame_location as usize
                            + BackTrackInfoPatternCharacter::begin_index() as usize;
                        let match_amount_index = term.frame_location as usize
                            + BackTrackInfoPatternCharacter::match_amount_index() as usize;
                        ctx!().frame[begin_index] = self.input.get_pos() as usize;
                        ctx!().frame[match_amount_index] = 0;
                        match_next!();
                    }

                    ByteTermType::PatternCasedCharacterOnce | ByteTermType::PatternCasedCharacterFixed => {
                        if self.is_either_unicode_compilation() {
                            // A comparação sem diferenciar maiúsculas de caracteres unicode é tratada como Type::CharacterClass.
                            debug_assert!(term.atom.first_id <= 0xFFFF);

                            let position = self.input.get_pos(); // Pode ser preciso desfazer a leitura de um par substituto.

                            if term.match_direction() == MatchDirection::Forward {
                                for match_amount in 0..term.atom.quantity_max_count {
                                    if !self.check_cased_character(&term, term.input_position.wrapping_sub(match_amount)) {
                                        self.input.set_pos(position);
                                        backtrack!();
                                    }
                                }
                            } else {
                                if self.input.get_pos() < term.input_position {
                                    backtrack!();
                                }

                                for match_amount in 0..term.atom.quantity_max_count {
                                    if !self.check_cased_character(
                                        &term,
                                        term.input_position
                                            .wrapping_sub(term.atom.quantity_max_count)
                                            .wrapping_add(match_amount)
                                            .wrapping_add(1),
                                    ) {
                                        self.input.set_pos(position);
                                        backtrack!();
                                    }
                                }
                            }
                            match_next!();
                        }

                        for match_amount in 0..term.atom.quantity_max_count {
                            if !self.check_cased_character(&term, term.input_position.wrapping_sub(match_amount)) {
                                backtrack!();
                            }
                        }
                        match_next!();
                    }
                    ByteTermType::PatternCasedCharacterGreedy => {
                        let back_track_index = term.frame_location as usize
                            + BackTrackInfoPatternCharacter::match_amount_index() as usize;

                        // A comparação sem diferenciar maiúsculas de caracteres unicode é tratada como Type::CharacterClass.
                        debug_assert!(!self.is_either_unicode_compilation() || term.atom.first_id <= 0xFFFF);

                        if term.match_direction() == MatchDirection::Forward {
                            let mut match_amount: u32 = 0;
                            while match_amount < term.atom.quantity_max_count && self.input.check_input(1) {
                                if !self.check_cased_character(&term, term.input_position.wrapping_add(1)) {
                                    self.input.uncheck_input(1);
                                    break;
                                }
                                match_amount += 1;
                            }
                            ctx!().frame[back_track_index] = match_amount as usize;

                            match_next!();
                        } else {
                            if self.input.get_pos() < term.input_position {
                                backtrack!();
                            }

                            let mut position = self.input.get_pos();
                            let mut match_amount: u32 = 0;
                            while match_amount < term.atom.quantity_max_count && self.input.try_uncheck_input(1) {
                                if !self.check_cased_character(&term, term.input_position) {
                                    self.input.set_pos(position);
                                    break;
                                }

                                match_amount += 1;
                                position = self.input.get_pos();
                            }
                            ctx!().frame[back_track_index] = match_amount as usize;

                            match_next!();
                        }
                    }

                    ByteTermType::CharacterClass => {
                        if self.match_character_class(&term, context) {
                            match_next!();
                        }
                        backtrack!();
                    }
                    ByteTermType::BackReference => {
                        if self.match_back_reference(&term, context) {
                            match_next!();
                        }
                        backtrack!();
                    }
                    ByteTermType::ParenthesesSubpattern => {
                        let result = self.match_parentheses(&term, context);

                        if result == JSRegExpResult::Match {
                            match_next!();
                        } else if result != JSRegExpResult::NoMatch {
                            return result;
                        }

                        backtrack!();
                    }
                    ByteTermType::ParenthesesSubpatternOnceBegin => {
                        if self.match_parentheses_once_begin(&term, context) {
                            match_next!();
                        }
                        backtrack!();
                    }
                    ByteTermType::ParenthesesSubpatternOnceEnd => {
                        if self.match_parentheses_once_end(&term, context) {
                            match_next!();
                        }
                        backtrack!();
                    }
                    ByteTermType::ParenthesesSubpatternTerminalBegin => {
                        if self.match_parentheses_terminal_begin(&term, context) {
                            match_next!();
                        }
                        backtrack!();
                    }
                    ByteTermType::ParenthesesSubpatternTerminalEnd => {
                        if self.match_parentheses_terminal_end(&term, context) {
                            match_next!();
                        }
                        backtrack!();
                    }
                    ByteTermType::ParentheticalAssertionBegin => {
                        if self.match_parenthetical_assertion_begin(&term, context) {
                            match_next!();
                        }
                        backtrack!();
                    }
                    ByteTermType::ParentheticalAssertionEnd => {
                        if self.match_parenthetical_assertion_end(&term, context) {
                            match_next!();
                        }
                        backtrack!();
                    }

                    ByteTermType::CheckInput => {
                        if self.input.check_input(term.check_input_count) {
                            match_next!();
                        }
                        backtrack!();
                    }

                    ByteTermType::UncheckInput => {
                        self.input.uncheck_input(term.check_input_count);
                        match_next!();
                    }

                    ByteTermType::HaveCheckedInput => {
                        if self.input.is_valid_negative_input_offset(term.check_input_count) {
                            match_next!();
                        }
                        backtrack!();
                    }

                    ByteTermType::DotStarEnclosure => {
                        if self.match_dot_star_enclosure(&term, context) {
                            return JSRegExpResult::Match;
                        }
                        backtrack!();
                    }
                }

                // Nunca devemos cair aqui.
                unreachable!("RELEASE_ASSERT_NOT_REACHED() em YarrInterpreter.cpp:2135 (fim do switch de matchDisjunction)");
            } else {
                // backtrack:
                match term.type_ {
                    ByteTermType::SubpatternBegin => {
                        return JSRegExpResult::NoMatch;
                    }
                    ByteTermType::SubpatternEnd => unreachable!("RELEASE_ASSERT_NOT_REACHED() em YarrInterpreter.cpp:2147 (SubpatternEnd em backtrack)"),

                    ByteTermType::BodyAlternativeBegin | ByteTermType::BodyAlternativeDisjunction => {
                        let offset = term.alternative.next;
                        ctx!().term = ctx!().term.wrapping_add(offset);
                        if offset > 0 {
                            match_next!();
                        }

                        if self.input.at_end() || pattern.sticky() {
                            return JSRegExpResult::NoMatch;
                        }

                        self.input.next();

                        ctx!().match_begin = self.input.get_pos();

                        while current_term!().alternative.once_through {
                            let next = current_term!().alternative.next;
                            if next <= 0 {
                                return JSRegExpResult::NoMatch;
                            }
                            ctx!().term = ctx!().term.wrapping_add(next);
                        }

                        match_next!();
                    }
                    ByteTermType::BodyAlternativeEnd => unreachable!("RELEASE_ASSERT_NOT_REACHED() em YarrInterpreter.cpp:2177 (BodyAlternativeEnd em backtrack)"),

                    ByteTermType::AlternativeBegin | ByteTermType::AlternativeDisjunction => {
                        let offset = term.alternative.next;
                        ctx!().term = ctx!().term.wrapping_add(offset);
                        if offset > 0 {
                            match_next!();
                        }
                        backtrack!();
                    }
                    ByteTermType::AlternativeEnd => {
                        // Nunca devemos voltar para dentro de uma alternativa do corpo principal da expressão.
                        let offset = ctx!().frame[term.frame_location as usize] as u32;
                        ctx!().term = ctx!().term.wrapping_sub(offset as i32);
                        backtrack!();
                    }

                    ByteTermType::AssertionBOL | ByteTermType::AssertionEOL | ByteTermType::AssertionWordBoundary => {
                        backtrack!();
                    }

                    ByteTermType::PatternCharacterOnce
                    | ByteTermType::PatternCharacterFixed
                    | ByteTermType::PatternCharacterGreedy
                    | ByteTermType::PatternCharacterNonGreedy => {
                        if self.backtrack_pattern_character(&term, context) {
                            match_next!();
                        }
                        backtrack!();
                    }
                    ByteTermType::PatternCasedCharacterOnce
                    | ByteTermType::PatternCasedCharacterFixed
                    | ByteTermType::PatternCasedCharacterGreedy
                    | ByteTermType::PatternCasedCharacterNonGreedy => {
                        if self.backtrack_pattern_cased_character(&term, context) {
                            match_next!();
                        }
                        backtrack!();
                    }
                    ByteTermType::CharacterClass => {
                        if self.backtrack_character_class(&term, context) {
                            match_next!();
                        }
                        backtrack!();
                    }
                    ByteTermType::BackReference => {
                        if self.backtrack_back_reference(&term, context) {
                            match_next!();
                        }
                        backtrack!();
                    }
                    ByteTermType::ParenthesesSubpattern => {
                        let result = self.backtrack_parentheses(&term, context);

                        if result == JSRegExpResult::Match {
                            match_next!();
                        } else if result != JSRegExpResult::NoMatch {
                            return result;
                        }

                        backtrack!();
                    }
                    ByteTermType::ParenthesesSubpatternOnceBegin => {
                        if self.backtrack_parentheses_once_begin(&term, context) {
                            match_next!();
                        }
                        backtrack!();
                    }
                    ByteTermType::ParenthesesSubpatternOnceEnd => {
                        if self.backtrack_parentheses_once_end(&term, context) {
                            match_next!();
                        }
                        backtrack!();
                    }
                    ByteTermType::ParenthesesSubpatternTerminalBegin => {
                        if self.backtrack_parentheses_terminal_begin(&term, context) {
                            match_next!();
                        }
                        backtrack!();
                    }
                    ByteTermType::ParenthesesSubpatternTerminalEnd => {
                        if self.backtrack_parentheses_terminal_end(&term, context) {
                            match_next!();
                        }
                        backtrack!();
                    }
                    ByteTermType::ParentheticalAssertionBegin => {
                        if self.backtrack_parenthetical_assertion_begin(&term, context) {
                            match_next!();
                        }
                        backtrack!();
                    }
                    ByteTermType::ParentheticalAssertionEnd => {
                        if self.backtrack_parenthetical_assertion_end(&term, context) {
                            match_next!();
                        }
                        backtrack!();
                    }

                    ByteTermType::CheckInput => {
                        self.input.uncheck_input(term.check_input_count);
                        backtrack!();
                    }

                    ByteTermType::UncheckInput => {
                        self.input.check_input(term.check_input_count);
                        backtrack!();
                    }

                    ByteTermType::HaveCheckedInput => {
                        backtrack!();
                    }

                    ByteTermType::DotStarEnclosure => unreachable!("RELEASE_ASSERT_NOT_REACHED() em YarrInterpreter.cpp:2272 (DotStarEnclosure em backtrack)"),
                }

                unreachable!("RELEASE_ASSERT_NOT_REACHED() em YarrInterpreter.cpp:2275 (fim do switch de backtrack)");
            }
        }
    }

    /// `matchNonZeroDisjunction(disjunction, context, btrack)`.
    pub fn match_non_zero_disjunction(
        &mut self,
        disjunction: &ByteDisjunction,
        context: DisjunctionContextRef,
        btrack: bool,
    ) -> crate::yarr::yarr::JSRegExpResult {
        let mut result = self.match_disjunction(disjunction, context, btrack);

        if result == crate::yarr::yarr::JSRegExpResult::Match {
            while self.context(context).match_begin == self.context(context).match_end {
                result = self.match_disjunction(disjunction, context, /* btrack= */ true);
                if result != crate::yarr::yarr::JSRegExpResult::Match {
                    return result;
                }
            }
            return crate::yarr::yarr::JSRegExpResult::Match;
        }

        result
    }

    /// `interpret()`. O `ConcurrentJSLocker` e o `BumpPointerPool` do alocador do padrão não existem
    /// aqui (ver o cabeçalho da fatia).
    pub fn interpret(&mut self) -> u32 {
        let pattern = self.pattern;

        // FIXME do C++ (https://bugs.webkit.org/show_bug.cgi?id=195970): o interpretador não tem
        // verificação de estouro de pilha para recursão profunda.
        if !self.input.is_available_input(0) {
            return crate::yarr::yarr::OFFSET_NO_MATCH;
        }

        if pattern.has_end_anchored_fixed_size() && self.input.end() >= pattern.end_anchored_fixed_size {
            let start = std::cmp::max(
                self.input.get_pos(),
                self.input.end() - pattern.end_anchored_fixed_size,
            );
            self.input.set_pos(start);
        }

        for i in 0..pattern.body.num_subpatterns + 1 {
            self.output[(i << 1) as usize] = crate::yarr::yarr::OFFSET_NO_MATCH;
        }

        for i in pattern.offset_vector_base_for_named_captures..pattern.offsets_size {
            self.output[i as usize] = 0;
        }

        let context_index = self.alloc_disjunction_context(&pattern.body);
        let context = DisjunctionContextRef::Disjunction(context_index);

        let result = self.match_disjunction(&pattern.body, context, /* btrack= */ false);
        if result == crate::yarr::yarr::JSRegExpResult::Match {
            self.output[0] = self.context(context).match_begin;
            self.output[1] = self.context(context).match_end;
        }

        self.free_disjunction_context(context_index);

        debug_assert!(
            (result == crate::yarr::yarr::JSRegExpResult::Match) == (self.output[0] != crate::yarr::yarr::OFFSET_NO_MATCH)
        );

        self.output[0]
    }

    /// `Interpreter(BytecodePattern* pattern, unsigned* output, std::span<const CharType> input, unsigned start)`.
    /// `output` é a fatia inteira do vetor de saída do chamador.
    pub fn new(
        pattern: &'a BytecodePattern,
        output: &'a mut [u32],
        input: &'a [C],
        start: u32,
    ) -> Interpreter<'a, C> {
        Interpreter {
            pattern,
            compile_mode: pattern.compile_mode(),
            output,
            input: InputStream::new(input, start, pattern.either_unicode()),
            disjunction_contexts: Vec::new(),
            parentheses_contexts: Vec::new(),
            free_disjunction_contexts: Vec::new(),
            free_parentheses_contexts: Vec::new(),
            start_offset: start,
            no_newline_before: start,
            remaining_match_count: crate::yarr::yarr::MATCH_LIMIT,
            context_bytes: 0,
            stack_check: crate::yarr::yarr_pattern_cpp1::StackCheck::new(),
        }
    }

    /// `isLegacyCompilation()`.
    pub fn is_legacy_compilation(&self) -> bool {
        self.compile_mode == CompileMode::Legacy
    }

    /// `isUnicodeCompilation()`.
    pub fn is_unicode_compilation(&self) -> bool {
        self.compile_mode == CompileMode::Unicode
    }

    /// `isUnicodeSetsCompilation()`.
    pub fn is_unicode_sets_compilation(&self) -> bool {
        self.compile_mode == CompileMode::UnicodeSets
    }

    /// `isEitherUnicodeCompilation()`.
    pub fn is_either_unicode_compilation(&self) -> bool {
        self.is_unicode_compilation() || self.is_unicode_sets_compilation()
    }

    /// `isSafeToRecurse()`.
    pub fn is_safe_to_recurse(&self) -> bool {
        self.stack_check.is_safe_to_recurse()
    }
}

/// `ByteCompiler::ParenthesesStackEntry`.
#[derive(Clone, Copy, Debug)]
pub struct ParenthesesStackEntry {
    pub begin_term: u32,
    pub saved_alternative_index: u32,
}

impl ParenthesesStackEntry {
    pub fn new(begin_term: u32, saved_alternative_index: u32) -> ParenthesesStackEntry {
        ParenthesesStackEntry { begin_term, saved_alternative_index }
    }
}

/// `std::unique_ptr<ByteDisjunction>`: `Option<ByteDisjunction>` com `Deref`/`DerefMut` que
/// desreferenciam como o C++ (nulo é pânico). `regexBegin` o cria; `compile` o move (`take`).
#[derive(Default)]
pub struct UniqueByteDisjunction(Option<ByteDisjunction>);

impl UniqueByteDisjunction {
    pub fn new(disjunction: ByteDisjunction) -> UniqueByteDisjunction {
        UniqueByteDisjunction(Some(disjunction))
    }

    /// Move o conteúdo para fora, deixando nulo (o `std::move` do `unique_ptr`).
    pub fn take(&mut self) -> Option<ByteDisjunction> {
        self.0.take()
    }
}

impl std::ops::Deref for UniqueByteDisjunction {
    type Target = ByteDisjunction;
    fn deref(&self) -> &ByteDisjunction {
        self.0.as_ref().expect("m_bodyDisjunction nulo")
    }
}

impl std::ops::DerefMut for UniqueByteDisjunction {
    fn deref_mut(&mut self) -> &mut ByteDisjunction {
        self.0.as_mut().expect("m_bodyDisjunction nulo")
    }
}

/// `class ByteCompiler`. O `m_bodyDisjunction` é um `unique_ptr` (`UniqueByteDisjunction`);
/// `m_allParenthesesInfo` guarda as disjunções de parênteses por valor, e o `ByteDisjunctionId` de
/// um termo é a posição no vetor. É a única definição (a fatia 5 só traz o `impl`).
pub struct ByteCompiler<'p> {
    pub pattern: &'p mut YarrPattern,
    pub body_disjunction: UniqueByteDisjunction,
    pub stack_check: crate::yarr::yarr_pattern_cpp1::StackCheck,
    pub current_alternative_index: u32,
    pub parentheses_stack: Vec<ParenthesesStackEntry>,
    pub all_parentheses_info: Vec<ByteDisjunction>,
    pub current_flags: FlagSet,
}

impl<'p> ByteCompiler<'p> {
    /// `ByteCompiler(YarrPattern& pattern)`.
    pub fn new(pattern: &'p mut YarrPattern) -> ByteCompiler<'p> {
        let current_flags = pattern.flags;
        ByteCompiler {
            pattern,
            body_disjunction: UniqueByteDisjunction::default(),
            stack_check: crate::yarr::yarr_pattern_cpp1::StackCheck::new(),
            current_alternative_index: 0,
            parentheses_stack: Vec::new(),
            all_parentheses_info: Vec::new(),
            current_flags,
        }
    }

    /// `compile(allocator, lock, errorCode)`. O `BumpPointerAllocator*` e o `ConcurrentJSLock*` não
    /// existem aqui (ver `yarr_interpreter.rs`). `emit_disjunction` devolve `Option<ErrorCode>`
    /// (o `std::optional<ErrorCode>` do C++) e leva a direção de casamento no fim, aqui `Forward`
    /// (o argumento padrão).
    pub fn compile(&mut self, error_code: &mut crate::yarr::yarr_error_code::ErrorCode) -> Option<Box<BytecodePattern>> {
        if !self.stack_check.is_safe_to_recurse() {
            *error_code = crate::yarr::yarr_error_code::ErrorCode::TooManyDisjunctions;
            return None;
        }

        let body = self.pattern.body;
        let num_subpatterns = self.pattern.num_subpatterns;
        let call_frame_size = self.pattern.disjunction(body).call_frame_size;
        let once_through = self.pattern.disjunction(body).alternatives[0].once_through();
        self.regex_begin(num_subpatterns, call_frame_size, once_through);
        if let Some(error) = self.emit_disjunction(body, 0, 0, crate::yarr::yarr_pattern::MatchDirection::Forward) {
            *error_code = error;
            return None;
        }
        self.regex_end();

        let offset_vector_base_for_named_captures = self.pattern.offset_vector_base_for_named_captures();
        let offsets_size = self.pattern.offsets_size();
        let body_disjunction = self.body_disjunction.take()?;
        Some(Box::new(BytecodePattern::new(
            body_disjunction,
            &mut self.all_parentheses_info,
            &mut *self.pattern,
            offset_vector_base_for_named_captures,
            offsets_size,
        )))
    }
}
