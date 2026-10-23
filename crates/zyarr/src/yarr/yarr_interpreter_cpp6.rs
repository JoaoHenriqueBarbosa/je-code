// Fatia 6 de `yarr/YarrInterpreter.cpp` (linhas 3004 a 3341, o fim): `ByteTermDumper::dumpTerm` e
// `dumpDisjunction`, as funções livres `byteCompile` e `interpret`, e os `static_assert` finais.
// Incluída por `include!` no fim de `yarr_interpreter.rs`, sem `use`: caminhos completos.
//
// Modelo (ver o cabeçalho de `yarr_interpreter.rs`):
// - `PrintStream& out = WTF::dataFile()` vira um `&mut String` recebido pelo chamador, que o imprime.
// - `m_pattern` do dumper é `&YarrPattern`, mas `dumpCharacterClass` pode criar a classe embutida em
//   cache (acessores `&mut`), então `dump_term` e `dump_disjunction` recebem o padrão como `&mut`.
// - `ByteDisjunction*` de `term.atom.parenthesesDisjunction` é `ByteDisjunctionId`; quem resolve é o
//   vetor `parentheses_info` (`m_allParenthesesInfo` do `ByteCompiler` ou do `BytecodePattern`).
// - `%p` de `ByteDisjunction(%p)` imprime o endereço da referência.
// - As `static_assert` sobre `sizeof(BackTrackInfo*)` comparam o tamanho de structs C++ com o número de
//   `uintptr_t` do frame; aqui os `BackTrackInfo*` são lidos e escritos campo a campo no `Vec<usize>` do
//   frame, sem layout de memória, e as constantes `YARR_STACK_SPACE_FOR_*` são a única fonte do tamanho.
//   Sem contrapartida a portar.
// - `SuperSamplerScope` (profiler) não tem comportamento observável.
// - As sobrecargas `Interpreter<Latin1Character>` e `Interpreter<char16_t>` são `Interpreter<u8>` e
//   `Interpreter<u16>`. Suponho `Interpreter::new(pattern, output, input, start)` e
//   `Interpreter::interpret(&mut self) -> u32`, e `ByteCompiler::new(pattern)` e
//   `ByteCompiler::compile(error_code) -> Option<Box<BytecodePattern>>`.

impl ByteTermDumper<'_> {
    /// O lambda `outputTermIndexAndNest`.
    fn output_term_index_and_nest(&self, out: &mut std::string::String, index: usize, term_nesting: u32) {
        use std::fmt::Write as _;
        let term_nesting = if !self.recursive_dump { 1 } else { term_nesting };

        for _ in 0..self.line_indent {
            out.push(' ');
        }
        let _ = write!(out, "{:4}", index);
        for _ in 0..term_nesting {
            out.push_str("  ");
        }
    }

    /// O lambda `dumpQuantity`.
    fn dump_quantity(out: &mut std::string::String, term: &ByteTerm) {
        use std::fmt::Write as _;
        if term.atom.quantity_type == QuantifierType::FixedCount {
            if term.atom.quantity_max_count > 1 {
                let _ = write!(out, " {{{}}}", term.atom.quantity_max_count);
            }
            return;
        }

        let _ = write!(out, " {{{}", term.atom.quantity_min_count);
        if term.atom.quantity_max_count == u32::MAX {
            out.push_str(",inf");
        } else {
            let _ = write!(out, ",{}", term.atom.quantity_max_count);
        }
        out.push('}');

        if term.atom.quantity_type == QuantifierType::Greedy {
            out.push_str(" greedy");
        } else if term.atom.quantity_type == QuantifierType::NonGreedy {
            out.push_str(" non-greedy");
        }
    }

    /// O lambda `dumpCaptured`.
    fn dump_captured(out: &mut std::string::String, term: &ByteTerm) {
        use std::fmt::Write as _;
        if term.capture() {
            let _ = write!(out, " captured (#{})", term.subpattern_id());
        }
    }

    /// O lambda `dumpInverted`.
    fn dump_inverted(out: &mut std::string::String, term: &ByteTerm) {
        if term.invert() {
            out.push_str(" inverted");
        }
    }

    /// O lambda `dumpMatchDirection`.
    fn dump_match_direction(out: &mut std::string::String, term: &ByteTerm) {
        if term.match_direction() == MatchDirection::Backward {
            if term.type_ == ByteTermType::ParentheticalAssertionBegin
                || term.type_ == ByteTermType::ParentheticalAssertionEnd
            {
                out.push_str(" lookbehind");
            } else {
                out.push_str(" backward");
            }
        }
    }

    /// O lambda `dumpInputPosition`.
    fn dump_input_position(out: &mut std::string::String, term: &ByteTerm) {
        use std::fmt::Write as _;
        let _ = write!(out, " inputPosition {}", term.input_position);
    }

    /// O lambda `dumpFrameLocation`.
    fn dump_frame_location(out: &mut std::string::String, term: &ByteTerm) {
        use std::fmt::Write as _;
        let _ = write!(out, " frameLocation {}", term.frame_location);
    }

    /// O lambda `dumpCharacter`.
    fn dump_character(out: &mut std::string::String, term: &ByteTerm) {
        out.push(' ');
        crate::yarr::yarr_pattern_cpp1::dump_char32(out, term.atom.first_id);
    }

    /// O lambda `dumpCharClass`.
    fn dump_char_class(out: &mut std::string::String, pattern: &mut YarrPattern, term: &ByteTerm) {
        out.push(' ');
        if let Some(character_class) = term.atom.character_class {
            crate::yarr::yarr_pattern_cpp1::dump_character_class(out, pattern, character_class);
        }
    }

    /// `ByteTermDumper::dumpTerm(size_t idx, ByteTerm term)`.
    pub fn dump_term(
        &mut self,
        out: &mut std::string::String,
        pattern: &mut YarrPattern,
        parentheses_info: &[ByteDisjunction],
        idx: usize,
        term: ByteTerm,
    ) {
        use std::fmt::Write as _;

        match term.type_ {
            ByteTermType::BodyAlternativeBegin => {
                let nesting = self.nesting;
                self.nesting += 1;
                self.output_term_index_and_nest(out, idx, nesting);
                out.push_str("BodyAlternativeBegin");
                if term.alternative.once_through {
                    out.push_str(" onceThrough");
                }
            }
            ByteTermType::BodyAlternativeDisjunction => {
                self.output_term_index_and_nest(out, idx, self.nesting - 1);
                out.push_str("BodyAlternativeDisjunction");
            }
            ByteTermType::BodyAlternativeEnd => {
                self.nesting -= 1;
                self.output_term_index_and_nest(out, idx, self.nesting);
                out.push_str("BodyAlternativeEnd");
            }
            ByteTermType::AlternativeBegin => {
                let nesting = self.nesting;
                self.nesting += 1;
                self.output_term_index_and_nest(out, idx, nesting);
                out.push_str("AlternativeBegin");
                Self::dump_frame_location(out, &term);
            }
            ByteTermType::AlternativeDisjunction => {
                self.output_term_index_and_nest(out, idx, self.nesting - 1);
                out.push_str("AlternativeDisjunction");
                Self::dump_frame_location(out, &term);
            }
            ByteTermType::AlternativeEnd => {
                self.nesting -= 1;
                self.output_term_index_and_nest(out, idx, self.nesting);
                out.push_str("AlternativeEnd");
                Self::dump_frame_location(out, &term);
            }
            ByteTermType::SubpatternBegin => {
                let nesting = self.nesting;
                self.nesting += 1;
                self.output_term_index_and_nest(out, idx, nesting);
                out.push_str("SubpatternBegin");
                Self::dump_match_direction(out, &term);
            }
            ByteTermType::SubpatternEnd => {
                self.nesting -= 1;
                self.output_term_index_and_nest(out, idx, self.nesting);
                out.push_str("SubpatternEnd");
                Self::dump_match_direction(out, &term);
            }
            ByteTermType::AssertionBOL => {
                self.output_term_index_and_nest(out, idx, self.nesting);
                out.push_str("AssertionBOL");
            }
            ByteTermType::AssertionEOL => {
                self.output_term_index_and_nest(out, idx, self.nesting);
                out.push_str("AssertionEOL");
            }
            ByteTermType::AssertionWordBoundary => {
                self.output_term_index_and_nest(out, idx, self.nesting);
                out.push_str("AssertionWordBoundary");
                Self::dump_inverted(out, &term);
                Self::dump_match_direction(out, &term);
            }
            ByteTermType::PatternCharacterOnce => {
                self.output_term_index_and_nest(out, idx, self.nesting);
                out.push_str("PatternCharacterOnce");
                Self::dump_inverted(out, &term);
                Self::dump_input_position(out, &term);
                Self::dump_character(out, &term);
                Self::dump_quantity(out, &term);
                Self::dump_match_direction(out, &term);
            }
            ByteTermType::PatternCharacterFixed => {
                self.output_term_index_and_nest(out, idx, self.nesting);
                out.push_str("PatternCharacterFixed");
                Self::dump_inverted(out, &term);
                Self::dump_input_position(out, &term);
                Self::dump_frame_location(out, &term);
                Self::dump_character(out, &term);
                let _ = write!(out, " {{{}}}", term.atom.quantity_min_count);
                Self::dump_match_direction(out, &term);
            }
            ByteTermType::PatternCharacterGreedy => {
                self.output_term_index_and_nest(out, idx, self.nesting);
                out.push_str("PatternCharacterGreedy");
                Self::dump_inverted(out, &term);
                Self::dump_input_position(out, &term);
                Self::dump_frame_location(out, &term);
                Self::dump_character(out, &term);
                Self::dump_quantity(out, &term);
                Self::dump_match_direction(out, &term);
            }
            ByteTermType::PatternCharacterNonGreedy => {
                self.output_term_index_and_nest(out, idx, self.nesting);
                out.push_str("PatternCharacterNonGreedy");
                Self::dump_inverted(out, &term);
                Self::dump_input_position(out, &term);
                Self::dump_frame_location(out, &term);
                Self::dump_character(out, &term);
                Self::dump_quantity(out, &term);
                Self::dump_match_direction(out, &term);
            }
            ByteTermType::PatternCasedCharacterOnce => {
                self.output_term_index_and_nest(out, idx, self.nesting);
                out.push_str("PatternCasedCharacterOnce");
                Self::dump_match_direction(out, &term);
            }
            ByteTermType::PatternCasedCharacterFixed => {
                self.output_term_index_and_nest(out, idx, self.nesting);
                out.push_str("PatternCasedCharacterFixed");
                Self::dump_match_direction(out, &term);
            }
            ByteTermType::PatternCasedCharacterGreedy => {
                self.output_term_index_and_nest(out, idx, self.nesting);
                out.push_str("PatternCasedCharacterGreedy");
                Self::dump_match_direction(out, &term);
            }
            ByteTermType::PatternCasedCharacterNonGreedy => {
                self.output_term_index_and_nest(out, idx, self.nesting);
                out.push_str("PatternCasedCharacterNonGreedy");
                Self::dump_match_direction(out, &term);
            }
            ByteTermType::CharacterClass => {
                self.output_term_index_and_nest(out, idx, self.nesting);
                out.push_str("CharacterClass");
                Self::dump_inverted(out, &term);
                Self::dump_input_position(out, &term);
                if term.atom.quantity_type != QuantifierType::FixedCount || self.either_unicode() {
                    Self::dump_frame_location(out, &term);
                }
                Self::dump_char_class(out, pattern, &term);
                Self::dump_quantity(out, &term);
                Self::dump_match_direction(out, &term);
            }
            ByteTermType::BackReference => {
                //  Need to update this for named capture group back references
                self.output_term_index_and_nest(out, idx, self.nesting);
                let _ = write!(out, "BackReference #{}", term.subpattern_id());
                Self::dump_input_position(out, &term);
                Self::dump_quantity(out, &term);
            }
            ByteTermType::ParenthesesSubpattern => {
                self.output_term_index_and_nest(out, idx, self.nesting);
                out.push_str("ParenthesesSubpattern");
                Self::dump_captured(out, &term);
                Self::dump_inverted(out, &term);
                Self::dump_match_direction(out, &term);
                Self::dump_input_position(out, &term);
                Self::dump_frame_location(out, &term);
                Self::dump_quantity(out, &term);
                if self.recursive_dump {
                    out.push('\n');
                    if let Some(id) = term.atom.parentheses_disjunction {
                        let nesting = self.nesting;
                        self.dump_disjunction(out, pattern, parentheses_info, &parentheses_info[id.0 as usize], nesting);
                    }
                }
            }
            ByteTermType::ParenthesesSubpatternOnceBegin => {
                let nesting = self.nesting;
                self.nesting += 1;
                self.output_term_index_and_nest(out, idx, nesting);
                out.push_str("ParenthesesSubpatternOnceBegin");
                Self::dump_captured(out, &term);
                Self::dump_inverted(out, &term);
                Self::dump_match_direction(out, &term);
                Self::dump_input_position(out, &term);
                Self::dump_frame_location(out, &term);
            }
            ByteTermType::ParenthesesSubpatternOnceEnd => {
                self.nesting -= 1;
                self.output_term_index_and_nest(out, idx, self.nesting);
                out.push_str("ParenthesesSubpatternOnceEnd");
                Self::dump_captured(out, &term);
                Self::dump_inverted(out, &term);
                Self::dump_match_direction(out, &term);
                Self::dump_input_position(out, &term);
                Self::dump_frame_location(out, &term);
            }
            ByteTermType::ParenthesesSubpatternTerminalBegin => {
                let nesting = self.nesting;
                self.nesting += 1;
                self.output_term_index_and_nest(out, idx, nesting);
                out.push_str("ParenthesesSubpatternTerminalBegin");
                Self::dump_inverted(out, &term);
                Self::dump_match_direction(out, &term);
                Self::dump_input_position(out, &term);
                Self::dump_frame_location(out, &term);
            }
            ByteTermType::ParenthesesSubpatternTerminalEnd => {
                self.nesting -= 1;
                self.output_term_index_and_nest(out, idx, self.nesting);
                out.push_str("ParenthesesSubpatternTerminalEnd");
                Self::dump_inverted(out, &term);
                Self::dump_match_direction(out, &term);
                Self::dump_input_position(out, &term);
                Self::dump_frame_location(out, &term);
            }
            ByteTermType::ParentheticalAssertionBegin => {
                let nesting = self.nesting;
                self.nesting += 1;
                self.output_term_index_and_nest(out, idx, nesting);
                out.push_str("ParentheticalAssertionBegin");
                Self::dump_inverted(out, &term);
                Self::dump_match_direction(out, &term);
                Self::dump_input_position(out, &term);
                Self::dump_frame_location(out, &term);
            }
            ByteTermType::ParentheticalAssertionEnd => {
                self.nesting -= 1;
                self.output_term_index_and_nest(out, idx, self.nesting);
                out.push_str("ParentheticalAssertionEnd");
                Self::dump_inverted(out, &term);
                Self::dump_match_direction(out, &term);
                Self::dump_input_position(out, &term);
                Self::dump_frame_location(out, &term);
            }
            ByteTermType::CheckInput => {
                self.output_term_index_and_nest(out, idx, self.nesting);
                let _ = write!(out, "CheckInput {}", term.check_input_count);
            }
            ByteTermType::UncheckInput => {
                self.output_term_index_and_nest(out, idx, self.nesting);
                let _ = write!(out, "UncheckInput {}", term.check_input_count);
            }
            ByteTermType::HaveCheckedInput => {
                self.output_term_index_and_nest(out, idx, self.nesting);
                let _ = write!(out, "HaveCheckedInput {}", term.check_input_count);
            }
            ByteTermType::DotStarEnclosure => {
                self.output_term_index_and_nest(out, idx, self.nesting);
                out.push_str("DotStarEnclosure");
            }
        }
    }

    /// `ByteTermDumper::dumpDisjunction(ByteDisjunction*, unsigned nesting = 0)`.
    pub fn dump_disjunction(
        &mut self,
        out: &mut std::string::String,
        pattern: &mut YarrPattern,
        parentheses_info: &[ByteDisjunction],
        disjunction: &ByteDisjunction,
        nesting: u32,
    ) {
        use std::fmt::Write as _;

        let saved_line_indent = self.line_indent;
        if nesting == 0 {
            let _ = write!(out, "ByteDisjunction({:p}):\n", disjunction as *const ByteDisjunction);
            self.recursive_dump = true;
            self.nesting = 1;
        } else {
            self.line_indent = nesting - 1;
        }

        for idx in 0..disjunction.terms.len() {
            let term = disjunction.terms[idx];

            self.dump_term(out, pattern, parentheses_info, idx, term);

            if term.type_ != ByteTermType::ParenthesesSubpattern {
                out.push('\n');
            }
        }

        self.line_indent = saved_line_indent;
    }
}

/// `std::unique_ptr<BytecodePattern> byteCompile(YarrPattern&, BumpPointerAllocator*, ErrorCode&, ConcurrentJSLock*)`.
pub fn byte_compile(
    pattern: &mut YarrPattern,
    error_code: &mut crate::yarr::yarr_error_code::ErrorCode,
) -> Option<Box<BytecodePattern>> {
    ByteCompiler::new(pattern).compile(error_code)
}

/// `unsigned interpret(BytecodePattern*, StringView input, unsigned start, unsigned* output)`.
pub fn interpret(
    bytecode: &BytecodePattern,
    input: crate::wtf::text::string_view::StringView,
    start: u32,
    output: &mut [u32],
) -> u32 {
    if input.is_8bit() {
        return Interpreter::<u8>::new(bytecode, output, input.span8(), start).interpret();
    }
    Interpreter::<u16>::new(bytecode, output, input.span16(), start).interpret()
}
