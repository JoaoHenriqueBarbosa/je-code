// Porte de `yarr/YarrPattern.cpp`, sexta fatia (linhas 3391 a 4522, o fim): o
// `extractSpecificPattern` inteiro (`tryExtractAtom`, `tryExtractSpaces`, `tryExtractNewlines`), a
// `ParenthesisContext`, os acessores privados do `YarrPatternConstructor`, a definição da struct
// `YarrPatternConstructor`, `YarrPattern::compile`, o construtor de `YarrPattern`, os `dump*` de
// depuração, `anycharCreate`, `CharacterClass::hasSharedLeadSurrogate`, o
// `FirstCharacterBitmapBuilder` e `computeFirstCharacterBitmap`.
//
// Esta fatia é incluída por `include!` no módulo da fatia 1 e, por isso, não tem `use`: usa pelo
// nome só o que a fatia 1 importa (`is_ascii_alpha`, `to_ascii_upper`, `to_ascii_lower`,
// `CharacterClass`, `CharacterRange`, `CharacterClassWidths`) e a `BitSet` da fatia 2; o resto vai
// por caminho completo.
//
// O `extractSpecificPattern` do C++ é uma função com três lambdas; no Rust os lambdas viram os
// métodos `try_extract_atom`, `try_extract_spaces` e `try_extract_newlines`.

/// `WTF::StackCheck`. O C++ compara o ponteiro de pilha com o limite de recursão da thread
/// (`StackBounds::recursionLimit`); Rust seguro não conhece os limites da thread, então o limite é
/// um orçamento de `STACK_BUDGET` bytes abaixo do ponteiro de pilha no momento da construção
/// (o endereço de uma variável local faz o papel de `currentStackPointer()`).
#[derive(Clone, Copy, Debug)]
pub struct StackCheck {
    stack_limit: usize,
}

impl StackCheck {
    /// Quanto da pilha, abaixo do ponto de construção, a recursão do parser pode consumir.
    const STACK_BUDGET: usize = 512 * 1024;

    pub fn new() -> Self {
        StackCheck { stack_limit: StackCheck::current_stack_pointer().saturating_sub(StackCheck::STACK_BUDGET) }
    }

    #[inline(never)]
    fn current_stack_pointer() -> usize {
        let marker = 0u8;
        std::ptr::addr_of!(marker) as usize
    }

    #[inline]
    pub fn is_safe_to_recurse(&self) -> bool {
        StackCheck::current_stack_pointer() >= self.stack_limit
    }
}

impl Default for StackCheck {
    fn default() -> Self {
        StackCheck::new()
    }
}

/// Um termo de classe de espaços (`m_pattern.spacesCharacterClass()`) com a posição, o tipo e as
/// contagens do quantificador dados: o teste que `tryExtractSpaces` repete em cada termo. A classe
/// de espaços só é pedida (e criada, se ainda não existir) depois de o termo ser uma classe de
/// caracteres não invertida, como na ordem de avaliação do `||` do C++.
fn is_spaces_term(
    pattern: &mut crate::yarr::yarr_pattern::YarrPattern,
    term: &crate::yarr::yarr_pattern::PatternTerm,
    input_position: u32,
    quantity_type: crate::yarr::yarr_pattern::QuantifierType,
    quantity_min_count: u32,
    quantity_max_count: u32,
) -> bool {
    if term.invert() || term.type_ != crate::yarr::yarr_pattern::PatternTermType::CharacterClass {
        return false;
    }
    if term.character_class() != pattern.spaces_character_class() {
        return false;
    }
    term.input_position == input_position
        && term.quantity_type == quantity_type
        && term.quantity_min_count == quantity_min_count
        && term.quantity_max_count == quantity_max_count
}

/// Um termo `PatternCharacter` com o caractere e o quantificador dados: o teste que
/// `isCROptionalLF` e `isLF` repetem.
fn is_pattern_character_term(
    term: &crate::yarr::yarr_pattern::PatternTerm,
    ch: u32,
    quantity_type: crate::yarr::yarr_pattern::QuantifierType,
    quantity_min_count: u32,
    quantity_max_count: u32,
) -> bool {
    term.type_ == crate::yarr::yarr_pattern::PatternTermType::PatternCharacter
        && term.pattern_character() == ch
        && term.quantity_type == quantity_type
        && term.quantity_min_count == quantity_min_count
        && term.quantity_max_count == quantity_max_count
}

/// O lambda `isCROptionalLF` de `tryExtractNewlines`: `\r\n?`.
fn is_cr_optional_lf(alternative: &crate::yarr::yarr_pattern::PatternAlternative) -> bool {
    use crate::yarr::yarr_pattern::QuantifierType;
    if alternative.terms.len() != 2 {
        return false;
    }
    is_pattern_character_term(&alternative.terms[0], '\r' as u32, QuantifierType::FixedCount, 1, 1)
        && is_pattern_character_term(&alternative.terms[1], '\n' as u32, QuantifierType::Greedy, 0, 1)
}

/// O lambda `isLF` de `tryExtractNewlines`: `\n`.
fn is_lf(alternative: &crate::yarr::yarr_pattern::PatternAlternative) -> bool {
    use crate::yarr::yarr_pattern::QuantifierType;
    if alternative.terms.len() != 1 {
        return false;
    }
    is_pattern_character_term(&alternative.terms[0], '\n' as u32, QuantifierType::FixedCount, 1, 1)
}

/// `YarrPatternConstructor::ParenthesisContext::SavedContext`.
#[derive(Clone, Copy, Debug)]
struct SavedContext {
    is_modifier: bool,
    invert: bool,
    match_direction: crate::yarr::yarr_pattern::MatchDirection,
    inside_lookbehind: bool,
    flags: crate::yarr::yarr_flags::FlagSet,
}

impl SavedContext {
    fn new(
        is_modifier: bool,
        invert: bool,
        match_direction: crate::yarr::yarr_pattern::MatchDirection,
        inside_lookbehind: bool,
        flags: crate::yarr::yarr_flags::FlagSet,
    ) -> Self {
        SavedContext { is_modifier, invert, match_direction, inside_lookbehind, flags }
    }

    fn restore(
        &self,
        is_modifier: &mut bool,
        invert: &mut bool,
        match_direction: &mut crate::yarr::yarr_pattern::MatchDirection,
        inside_lookbehind: &mut bool,
        flags: &mut crate::yarr::yarr_flags::FlagSet,
    ) {
        *is_modifier = self.is_modifier;
        *invert = self.invert;
        *match_direction = self.match_direction;
        *inside_lookbehind = self.inside_lookbehind;
        *flags = self.flags;
    }
}

/// `YarrPatternConstructor::ParenthesisContext`.
#[derive(Clone, Debug)]
struct ParenthesisContext {
    backing_stack: Vec<SavedContext>,
    stack_depth: u32,
    is_modifier: bool,
    invert: bool,
    match_direction: crate::yarr::yarr_pattern::MatchDirection,
    inside_lookbehind: bool,
    flags: crate::yarr::yarr_flags::FlagSet,
}

impl ParenthesisContext {
    fn new() -> Self {
        ParenthesisContext {
            backing_stack: Vec::new(),
            stack_depth: 0,
            is_modifier: false,
            invert: false,
            match_direction: crate::yarr::yarr_pattern::MatchDirection::Forward,
            inside_lookbehind: false,
            flags: crate::yarr::yarr_flags::FlagSet::empty(),
        }
    }

    fn push(&mut self) {
        debug_assert!(self.stack_depth < u32::MAX);

        let previous_depth = self.stack_depth;
        self.stack_depth += 1;
        if previous_depth > 0 {
            self.backing_stack.push(SavedContext::new(
                self.is_modifier,
                self.invert,
                self.match_direction,
                self.inside_lookbehind,
                self.flags,
            ));
        }

        // isModifier should only apply to one frame at a time. m_insideLookbehind is
        // inherited: a nested parenthesis stays inside any enclosing lookbehind.
        self.is_modifier = false;
    }

    fn pop(&mut self) {
        debug_assert!(self.stack_depth > 0);

        self.stack_depth -= 1;
        if self.stack_depth > 0 {
            // `takeLast`: a pilha de salvos tem uma entrada para cada nível acima do primeiro.
            if let Some(context) = self.backing_stack.pop() {
                context.restore(
                    &mut self.is_modifier,
                    &mut self.invert,
                    &mut self.match_direction,
                    &mut self.inside_lookbehind,
                    &mut self.flags,
                );
            }
        } else {
            self.is_modifier = false;
            self.invert = false;
            self.match_direction = crate::yarr::yarr_pattern::MatchDirection::Forward;
            self.inside_lookbehind = false;
            self.flags = crate::yarr::yarr_flags::FlagSet::empty();
        }
    }

    fn set_modifier(&mut self, is_mod: bool) {
        self.is_modifier = is_mod;
    }

    fn is_modifier(&self) -> bool {
        self.is_modifier
    }

    fn set_invert(&mut self, invert: bool) {
        self.invert = invert;
    }

    fn invert(&self) -> bool {
        self.invert
    }

    fn set_match_direction(&mut self, match_direction: crate::yarr::yarr_pattern::MatchDirection) {
        self.match_direction = match_direction;
        // Entering a lookbehind puts every deeper context inside one; the bit is
        // inherited through push() and cleared only when this frame pops.
        if match_direction == crate::yarr::yarr_pattern::MatchDirection::Backward {
            self.inside_lookbehind = true;
        }
    }

    fn match_direction(&self) -> crate::yarr::yarr_pattern::MatchDirection {
        self.match_direction
    }

    // True inside a lookbehind at ANY nesting depth (including within a lookahead
    // nested in a lookbehind), unlike matchDirection(), which is only the innermost
    // assertion's own direction.
    fn inside_lookbehind(&self) -> bool {
        self.inside_lookbehind
    }

    fn set_flags(&mut self, flags: crate::yarr::yarr_flags::FlagSet) {
        self.flags = flags;
    }

    fn flags(&self) -> crate::yarr::yarr_flags::FlagSet {
        self.flags
    }

    fn reset(&mut self) {
        self.backing_stack.clear();
        self.stack_depth = 0;

        self.is_modifier = false;
        self.invert = false;
        self.match_direction = crate::yarr::yarr_pattern::MatchDirection::Forward;
        self.inside_lookbehind = false;
        self.flags = crate::yarr::yarr_flags::FlagSet::empty();
    }
}

/// `class YarrPatternConstructor`.
pub struct YarrPatternConstructor<'a> {
    pattern: &'a mut crate::yarr::yarr_pattern::YarrPattern,
    /// O `PatternAlternative* m_alternative`.
    alternative: crate::yarr::yarr_pattern::AlternativeId,
    base_character_class_constructor: CharacterClassConstructor,
    /// O ponteiro `m_currentCharacterClassConstructor`: `None` é o `m_baseCharacterClassConstructor`,
    /// `Some(i)` é `m_characterClassStack[i]` (ver `current_ccc!`).
    current_character_class_constructor: Option<usize>,
    character_class_stack: Vec<CharacterClassConstructor>,
    forward_references_in_lookbehind: Vec<UnresolvedForwardReference>,
    stack_check: StackCheck,
    /// Orçamento da fatoração de alternativas (iniciado em 0).
    factoring_budget: usize,
    error: crate::yarr::yarr_error_code::ErrorCode,
    invert_character_class: bool,
    parenthesis_context: ParenthesisContext,

    initial_flags: crate::yarr::yarr_flags::FlagSet,
    flags: crate::yarr::yarr_flags::FlagSet,
}

// `static_assert(YarrSyntaxCheckable<YarrPatternConstructor>)`.
const _: fn() = || {
    fn assert_yarr_syntax_checkable<T: crate::yarr::yarr_parser::Delegate>() {}
    assert_yarr_syntax_checkable::<YarrPatternConstructor<'static>>();
};

impl<'a> YarrPatternConstructor<'a> {
    /// O lambda `tryExtractSpaces` de `extractSpecificPattern`: `^\s*`, `^\s+`, `\s*$` e `\s+$`.
    fn try_extract_spaces(&mut self) -> bool {
        use crate::yarr::yarr::{SpecificPattern, QUANTIFY_INFINITE};
        use crate::yarr::yarr_pattern::{PatternTermType, QuantifierType};

        let body = self.pattern.body;
        let alternatives = &self.pattern.disjunction(body).alternatives;
        if alternatives.len() != 1 {
            return false;
        }

        let terms = alternatives[0].terms.clone();
        if terms.is_empty() {
            return false;
        }

        if self.pattern.contains_bol {
            let term_first = terms[0];
            if term_first.invert() || term_first.type_ != PatternTermType::AssertionBOL {
                return false;
            }

            if terms.len() == 2 {
                // ^\s*
                if !is_spaces_term(self.pattern, &terms[1], 0, QuantifierType::Greedy, 0, QUANTIFY_INFINITE) {
                    return false;
                }

                self.pattern.specific_pattern = SpecificPattern::LeadingSpacesStar;
                return true;
            }

            if terms.len() == 3 {
                // ^\s+
                if !is_spaces_term(self.pattern, &terms[1], 0, QuantifierType::FixedCount, 1, 1) {
                    return false;
                }

                if !is_spaces_term(self.pattern, &terms[2], 1, QuantifierType::Greedy, 0, QUANTIFY_INFINITE) {
                    return false;
                }

                self.pattern.specific_pattern = SpecificPattern::LeadingSpacesPlus;
                return true;
            }
            return false;
        }

        let term_last = terms[terms.len() - 1];
        if term_last.invert() || term_last.type_ != PatternTermType::AssertionEOL {
            return false;
        }

        if terms.len() == 2 {
            // \s*$
            if !is_spaces_term(self.pattern, &terms[0], 0, QuantifierType::Greedy, 0, QUANTIFY_INFINITE) {
                return false;
            }

            self.pattern.specific_pattern = SpecificPattern::TrailingSpacesStar;
            return true;
        }

        if terms.len() == 3 {
            // \s+$
            if !is_spaces_term(self.pattern, &terms[0], 0, QuantifierType::FixedCount, 1, 1) {
                return false;
            }

            if !is_spaces_term(self.pattern, &terms[1], 1, QuantifierType::Greedy, 0, QUANTIFY_INFINITE) {
                return false;
            }

            self.pattern.specific_pattern = SpecificPattern::TrailingSpacesPlus;
            return true;
        }

        false
    }

    /// O lambda `tryExtractNewlines` de `extractSpecificPattern`.
    fn try_extract_newlines(&mut self) -> bool {
        // Detect patterns: \r\n?|\n or \n|\r\n?
        // These patterns match LF (\n), CR (\r), and CRLF (\r\n)

        let body = self.pattern.body;
        let alternatives = &self.pattern.disjunction(body).alternatives;

        if alternatives.len() != 2 {
            return false;
        }

        let alternative1 = &alternatives[0];
        let alternative2 = &alternatives[1];

        let matches = (is_cr_optional_lf(alternative1) && is_lf(alternative2))
            || (is_lf(alternative1) && is_cr_optional_lf(alternative2));

        if matches {
            self.pattern.specific_pattern = crate::yarr::yarr::SpecificPattern::Newlines;
            return true;
        }

        false
    }

    /// O lambda `tryExtractAtom` de `extractSpecificPattern`.
    fn try_extract_atom(&mut self) -> bool {
        use crate::yarr::yarr_pattern::{MatchDirection, PatternTermType, QuantifierType};

        if self.pattern.contains_bol {
            return false;
        }
        let body = self.pattern.body;
        let disjunction = self.pattern.disjunction(body);
        if disjunction.minimum_size == 0 {
            return false;
        }
        let alternatives = &disjunction.alternatives;
        if alternatives.len() != 1 {
            return false;
        }
        let mut builder = crate::wtf::text::string_builder::StringBuilder::new();
        let alternative = &alternatives[0];
        for (index, term) in alternative.terms.iter().enumerate() {
            if term.type_ != PatternTermType::PatternCharacter {
                return false;
            }
            if term.quantity_type != QuantifierType::FixedCount {
                return false;
            }
            if term.quantity_max_count != 1 {
                return false;
            }
            if term.input_position != index as u32 {
                return false;
            }
            let pattern_character = term.pattern_character();
            // `U16_LENGTH(c) != 1`: fora do BMP ocupa dois code units.
            if pattern_character > 0xffff {
                return false;
            }
            // `U_IS_SURROGATE`.
            if self.pattern.either_unicode() && (0xd800..=0xdfff).contains(&pattern_character) {
                return false;
            }
            if term.match_direction != MatchDirection::Forward {
                return false;
            }
            builder.append_character(pattern_character as u16);
        }
        let atom = builder.to_string().clone();
        if atom.length() > 0 {
            self.pattern.atom = atom;
            self.pattern.specific_pattern = crate::yarr::yarr::SpecificPattern::Atom;
            return true;
        }
        false
    }

    pub fn extract_specific_pattern(&mut self) {
        if self.pattern.contains_backreferences {
            return;
        }
        if self.pattern.contains_lookbehinds {
            return;
        }
        if self.pattern.contains_unsigned_length_pattern {
            return;
        }
        if self.pattern.contains_modifiers {
            return;
        }
        if self.pattern.has_copied_paren_subexpressions {
            return;
        }
        if self.pattern.has_named_capture_groups {
            return;
        }
        if self.pattern.save_initial_start_value {
            return;
        }
        if self.pattern.num_subpatterns != 0 {
            return;
        }
        if self.pattern.multiline() {
            return;
        }
        if self.pattern.sticky() {
            return;
        }
        if self.pattern.ignore_case() {
            return;
        }

        if self.try_extract_atom() {
            return;
        }

        if self.pattern.either_unicode() {
            return;
        }

        if self.try_extract_spaces() {
            return;
        }

        if self.try_extract_newlines() {
            return;
        }
    }

    pub fn error(&self) -> crate::yarr::yarr_error_code::ErrorCode {
        self.error
    }

    fn push_parenthesis_context(&mut self) {
        self.parenthesis_context.push();
    }

    fn pop_parenthesis_context(&mut self) {
        self.parenthesis_context.pop();
    }

    fn set_parenthesis_invert(&mut self, invert: bool) {
        self.parenthesis_context.set_invert(invert);
    }

    fn parenthesis_invert(&self) -> bool {
        self.parenthesis_context.invert()
    }

    fn set_parenthesis_match_direction(&mut self, match_direction: crate::yarr::yarr_pattern::MatchDirection) {
        self.parenthesis_context.set_match_direction(match_direction);
    }

    fn parenthesis_match_direction(&self) -> crate::yarr::yarr_pattern::MatchDirection {
        self.parenthesis_context.match_direction()
    }

    // Inside a lookbehind at any nesting depth (a lookahead nested within a
    // lookbehind still counts).
    fn inside_lookbehind(&self) -> bool {
        self.parenthesis_context.inside_lookbehind()
    }

    fn ignore_case(&self) -> bool {
        self.flags.contains(crate::yarr::yarr_flags::Flags::IgnoreCase)
    }

    fn multiline(&self) -> bool {
        self.flags.contains(crate::yarr::yarr_flags::Flags::Multiline)
    }

    fn dot_all(&self) -> bool {
        self.flags.contains(crate::yarr::yarr_flags::Flags::DotAll)
    }

    fn is_safe_to_recurse(&self) -> bool {
        self.stack_check.is_safe_to_recurse()
    }
}

impl crate::yarr::yarr_pattern::YarrPattern {
    /// `YarrPattern::compile(StringView)`.
    pub fn compile(&mut self, pattern_string: &crate::wtf::text::wtf_string::String) -> crate::yarr::yarr_error_code::ErrorCode {
        use crate::yarr::yarr_error_code::{has_error, ErrorCode};

        let compile_mode = self.compile_mode();
        let flags = self.flags;
        let mut constructor = YarrPatternConstructor::new(self, flags);

        {
            let error = crate::yarr::yarr_parser::parse(
                &mut constructor,
                pattern_string,
                compile_mode,
                crate::yarr::yarr::QUANTIFY_INFINITE,
                true,
            );
            if has_error(constructor.error()) {
                return constructor.error();
            }

            if has_error(error) {
                return error;
            }
        }

        constructor.recompute_starts_with_bol();
        constructor.check_for_terminal_parentheses();
        constructor.optimize_dot_star_wrapped_expressions();
        constructor.optimize_bol();
        constructor.factor_and_wrap_alternatives();
        constructor.optimize_possessive_quantifiers();

        if has_error(constructor.error()) {
            return constructor.error();
        }

        {
            let error = constructor.setup_offsets();
            if has_error(error) {
                return error;
            }
        }

        constructor.compute_end_anchored_fixed_size();
        constructor.setup_named_captures();

        constructor.extract_specific_pattern();

        if crate::yarr::yarr::DUMP_COMPILED_REG_EXP_PATTERNS {
            self.dump_pattern(pattern_string);
        }

        ErrorCode::NoError
    }

    /// `YarrPattern::YarrPattern(StringView, OptionSet<Flags>, ErrorCode&, ExecutionMode)`: a lista de
    /// inicialização é `with_flags`, e o corpo do construtor chama `compile`.
    pub fn new(
        pattern: &crate::wtf::text::wtf_string::String,
        flags: crate::yarr::yarr_flags::FlagSet,
        error: &mut crate::yarr::yarr_error_code::ErrorCode,
        execution_mode: crate::yarr::yarr::ExecutionMode,
    ) -> Self {
        debug_assert!(!flags.contains(crate::yarr::yarr_flags::Flags::DeletedValue));
        let mut yarr_pattern = crate::yarr::yarr_pattern::YarrPattern::with_flags(flags, execution_mode);
        *error = yarr_pattern.compile(pattern);
        yarr_pattern
    }
}

/// `indentForNestingLevel`.
fn indent_for_nesting_level(out: &mut std::string::String, nesting_depth: u32) {
    out.push_str("    ");
    for _ in 0..nesting_depth {
        out.push_str("  ");
    }
}

/// `dumpChar32`.
pub(crate) fn dump_char32(out: &mut std::string::String, c: u32) {
    use std::fmt::Write as _;
    if c >= ' ' as u32 && c <= 0xff {
        let _ = write!(out, "'{}'", (c as u8) as char);
    } else {
        let _ = write!(out, "0x{:04x}", c);
    }
}

/// O lambda `dumpMatches` de `dumpCharacterClass`.
fn dump_matches(out: &mut std::string::String, need_matches_ranges_separator: &mut bool, prefix: &str, matches: &[u32]) {
    use std::fmt::Write as _;
    if !matches.is_empty() {
        if *need_matches_ranges_separator {
            out.push(',');
        }
        *need_matches_ranges_separator = true;

        let _ = write!(out, "{}:(", prefix);
        for (i, &m) in matches.iter().enumerate() {
            if i != 0 {
                out.push(',');
            }
            dump_char32(out, m);
        }
        out.push(')');
    }
}

/// O lambda `dumpRanges` de `dumpCharacterClass`.
fn dump_ranges(out: &mut std::string::String, need_matches_ranges_separator: &mut bool, prefix: &str, ranges: &[CharacterRange]) {
    use std::fmt::Write as _;
    if !ranges.is_empty() {
        if *need_matches_ranges_separator {
            out.push(',');
        }
        *need_matches_ranges_separator = true;

        let _ = write!(out, "{} ranges:(", prefix);
        for (i, range) in ranges.iter().enumerate() {
            if i != 0 {
                out.push(',');
            }
            out.push('(');
            dump_char32(out, range.begin);
            out.push_str("..");
            dump_char32(out, range.end);
            out.push(')');
        }
        out.push(')');
    }
}

/// `dumpCharacterClass(out, pattern, characterClass)`. O `CharacterClass*` do C++ é o id, e o
/// `pattern` nunca é nulo nos chamadores.
pub(crate) fn dump_character_class(
    out: &mut std::string::String,
    pattern: &mut crate::yarr::yarr_pattern::YarrPattern,
    character_class: crate::yarr::yarr_pattern::CharacterClassId,
) {
    use crate::yarr::yarr_pattern::{CharacterClassId, YarrPattern};

    // Na ordem do C++: cada acessor só cria a classe embutida quando a comparação anterior falhou.
    let built_in_classes: [(fn(&mut YarrPattern) -> CharacterClassId, &str); 10] = [
        (YarrPattern::any_character_class, "<any character>"),
        (YarrPattern::newline_character_class, "<newline>"),
        (YarrPattern::digits_character_class, "<digits>"),
        (YarrPattern::spaces_character_class, "<whitespace>"),
        (YarrPattern::wordchar_character_class, "<word>"),
        (YarrPattern::word_unicode_ignore_case_char_character_class, "<unicode word ignore case>"),
        (YarrPattern::nondigits_character_class, "<non-digits>"),
        (YarrPattern::nonspaces_character_class, "<non-whitespace>"),
        (YarrPattern::nonwordchar_character_class, "<non-word>"),
        (YarrPattern::nonword_unicode_ignore_case_char_character_class, "<unicode non-word ignore case>"),
    ];
    for (accessor, name) in built_in_classes {
        if character_class == accessor(pattern) {
            out.push_str(name);
            return;
        }
    }

    let mut need_matches_ranges_separator = false;
    let character_class = pattern.character_class(character_class);

    out.push('[');
    dump_matches(out, &mut need_matches_ranges_separator, "Latin1", &character_class.matches8);
    dump_ranges(out, &mut need_matches_ranges_separator, "Latin1", &character_class.ranges8);
    dump_matches(out, &mut need_matches_ranges_separator, "NonLatin1", &character_class.matches32);
    dump_ranges(out, &mut need_matches_ranges_separator, "NonLatin1", &character_class.ranges32);
    out.push(']');
}

/// `PatternAlternative::dump`.
fn dump_alternative(
    out: &mut std::string::String,
    this_pattern: &mut crate::yarr::yarr_pattern::YarrPattern,
    alternative: crate::yarr::yarr_pattern::AlternativeId,
    nesting_depth: u32,
) {
    use std::fmt::Write as _;
    let (minimum_size, has_fixed_size, once_through, starts_with_bol, contains_bol, is_last_alternative, terms) = {
        let alternative = this_pattern.alternative(alternative);
        (
            alternative.minimum_size,
            alternative.has_fixed_size,
            alternative.once_through,
            alternative.starts_with_bol,
            alternative.contains_bol,
            alternative.is_last_alternative,
            alternative.terms.clone(),
        )
    };

    let _ = write!(out, "minimum size: {}", minimum_size);
    if has_fixed_size {
        out.push_str(",fixed size");
    }
    if once_through {
        out.push_str(",once through");
    }
    if starts_with_bol {
        out.push_str(",starts with ^");
    }
    if contains_bol {
        out.push_str(",contains ^");
    }
    if is_last_alternative {
        out.push_str(", last alternative");
    }
    out.push('\n');

    for term in &terms {
        term.dump(out, this_pattern, nesting_depth);
    }
}

/// `PatternDisjunction::dump`.
fn dump_disjunction(
    out: &mut std::string::String,
    this_pattern: &mut crate::yarr::yarr_pattern::YarrPattern,
    disjunction: crate::yarr::yarr_pattern::DisjunctionId,
    nesting_depth: u32,
) {
    use std::fmt::Write as _;
    let alternative_count = this_pattern.disjunction(disjunction).alternatives.len() as u32;
    for i in 0..alternative_count {
        indent_for_nesting_level(out, nesting_depth);
        if alternative_count > 1 {
            let _ = write!(out, "alternative #{}: ", i);
        }
        dump_alternative(
            out,
            this_pattern,
            crate::yarr::yarr_pattern::AlternativeId { disjunction, index: i },
            nesting_depth + (alternative_count > 1) as u32,
        );
    }
}

impl crate::yarr::yarr_pattern::PatternTerm {
    /// `PatternTerm::dumpQuantifier`.
    pub fn dump_quantifier(&self, out: &mut std::string::String) {
        use crate::yarr::yarr_pattern::QuantifierType;
        use std::fmt::Write as _;
        if self.quantity_type == QuantifierType::FixedCount && self.quantity_min_count == 1 && self.quantity_max_count == 1 {
            return;
        }
        let _ = write!(out, " {{{}", self.quantity_min_count);
        if self.quantity_min_count != self.quantity_max_count {
            if self.quantity_max_count == u32::MAX {
                out.push_str(",...");
            } else {
                let _ = write!(out, ",{}", self.quantity_max_count);
            }
        }
        out.push('}');
        if self.quantity_type == QuantifierType::Greedy {
            out.push_str(" greedy");
        } else if self.quantity_type == QuantifierType::NonGreedy {
            out.push_str(" non-greedy");
        }
        if self.possessive {
            out.push_str(" possessive");
        }
    }

    /// `PatternTerm::dump`.
    pub fn dump(
        &self,
        out: &mut std::string::String,
        this_pattern: &mut crate::yarr::yarr_pattern::YarrPattern,
        nesting_depth: u32,
    ) {
        use crate::yarr::yarr::{
            YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHESES, YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHESES_ONCE,
            YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHESES_TERMINAL,
        };
        use crate::yarr::yarr_flags::Flags;
        use crate::yarr::yarr_pattern::{MatchDirection, PatternTermType, QuantifierType};
        use std::fmt::Write as _;

        indent_for_nesting_level(out, nesting_depth);

        out.push('<');
        out.push(if self.current_flags.contains(Flags::IgnoreCase) { 'i' } else { ' ' });
        out.push(if self.current_flags.contains(Flags::Multiline) { 'm' } else { ' ' });
        out.push(if self.current_flags.contains(Flags::DotAll) { 's' } else { ' ' });
        out.push_str("> ");

        if self.type_ != PatternTermType::ParenthesesSubpattern && self.type_ != PatternTermType::ParentheticalAssertion {
            if self.invert() {
                out.push_str("not ");
            }
        }

        match self.type_ {
            PatternTermType::AssertionBOL => out.push_str("BOL\n"),
            PatternTermType::AssertionEOL => out.push_str("EOL\n"),
            PatternTermType::AssertionWordBoundary => out.push_str("word boundary\n"),
            PatternTermType::PatternCharacter => {
                out.push_str("character ");
                let _ = write!(out, "inputPosition {} ", self.input_position);
                if this_pattern.ignore_case() && is_ascii_alpha(self.pattern_character()) {
                    dump_char32(out, to_ascii_upper(self.pattern_character()));
                    out.push('/');
                    dump_char32(out, to_ascii_lower(self.pattern_character()));
                } else {
                    dump_char32(out, self.pattern_character());
                }
                self.dump_quantifier(out);
                if self.quantity_type != QuantifierType::FixedCount {
                    let _ = write!(out, ",frame location {}", self.frame_location);
                }
                out.push('\n');
            }
            PatternTermType::CharacterClass => {
                out.push_str("character class ");
                let _ = write!(out, "inputPosition {} ", self.input_position);
                dump_character_class(out, this_pattern, self.character_class());
                self.dump_quantifier(out);
                if self.quantity_type != QuantifierType::FixedCount || this_pattern.either_unicode() {
                    let _ = write!(out, ",frame location {}", self.frame_location);
                }
                out.push('\n');
            }
            PatternTermType::NumberedBackReference | PatternTermType::NamedBackReference => {
                out.push_str(if self.type_ == PatternTermType::NumberedBackReference { "numbered " } else { "named " });
                let _ = write!(out, "back reference of subpattern #{}", self.back_reference_subpattern_id());
                let _ = write!(out, " inputPosition {}", self.input_position);
                out.push('\n');
            }
            PatternTermType::NumberedForwardReference => out.push_str("numbered forward reference\n"),
            PatternTermType::NamedForwardReference => out.push_str("named forward reference\n"),
            PatternTermType::ParenthesesSubpattern | PatternTermType::ParentheticalAssertion => {
                if self.type_ == PatternTermType::ParenthesesSubpattern {
                    if self.capture {
                        out.push_str("captured ");
                    } else {
                        out.push_str("non-captured ");
                    }
                }

                if self.match_direction == MatchDirection::Backward {
                    if self.type_ == PatternTermType::ParenthesesSubpattern {
                        out.push_str("backwards ");
                    } else {
                        out.push_str("lookbehind ");
                    }
                }
                let _ = write!(out, "inputPosition {} ", self.input_position);
                if self.invert {
                    out.push_str("inverted ");
                }

                if self.type_ == PatternTermType::ParenthesesSubpattern {
                    out.push_str("subpattern");
                } else if self.type_ == PatternTermType::ParentheticalAssertion {
                    out.push_str("assertion");
                }

                let parentheses = *self.parentheses();
                if self.capture {
                    let _ = write!(out, " #{}", parentheses.subpattern_id);
                }

                self.dump_quantifier(out);

                if parentheses.is_copy {
                    out.push_str(",copy");
                }

                if parentheses.is_terminal {
                    out.push_str(",terminal");
                }

                if parentheses.is_string_list {
                    out.push_str(",string-list");
                }

                let _ = writeln!(out, ",frame location {}", self.frame_location);

                if this_pattern.disjunction(parentheses.disjunction).alternatives.len() > 1 {
                    indent_for_nesting_level(out, nesting_depth + 1);
                    let mut alternative_frame_location = self.frame_location;
                    if self.quantity_max_count == 1 && !parentheses.is_copy {
                        alternative_frame_location += YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHESES_ONCE;
                    } else if parentheses.is_terminal {
                        alternative_frame_location += YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHESES_TERMINAL;
                    } else {
                        alternative_frame_location += YARR_STACK_SPACE_FOR_BACK_TRACK_INFO_PARENTHESES;
                    }
                    let _ = writeln!(out, "alternative list,frame location {}", alternative_frame_location);
                }

                dump_disjunction(out, this_pattern, parentheses.disjunction, nesting_depth + 1);
            }
            PatternTermType::DotStarEnclosure => {
                let _ = writeln!(out, ".* enclosure,frame location {}", this_pattern.initial_start_value_frame_location);
            }
        }
    }
}

/// O texto de uma `StringView` para o `dumpPatternString`: Latin-1 direto, UTF-16 com substituição
/// de unidades isoladas.
fn dump_text_of(string: &crate::wtf::text::wtf_string::String) -> std::string::String {
    if string.is_8bit() {
        string.span8().iter().map(|&c| c as char).collect()
    } else {
        char::decode_utf16(string.span16().iter().copied())
            .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect()
    }
}

impl crate::yarr::yarr_pattern::YarrPattern {
    /// `YarrPattern::dumpPatternString`.
    pub fn dump_pattern_string(&self, out: &mut std::string::String, pattern_string: &crate::wtf::text::wtf_string::String) {
        out.push('/');
        out.push_str(&dump_text_of(pattern_string));
        out.push('/');

        if self.global() {
            out.push('g');
        }
        if self.ignore_case() {
            out.push('i');
        }
        if self.multiline() {
            out.push('m');
        }
        if self.unicode() {
            out.push('u');
        }
        if self.unicode_sets() {
            out.push('v');
        }
        if self.sticky() {
            out.push('y');
        }
    }

    /// `YarrPattern::dumpPattern(StringView)`: escreve no `dataFile()`, que é o stderr.
    pub fn dump_pattern(&mut self, pattern_string: &crate::wtf::text::wtf_string::String) {
        let mut out = std::string::String::new();
        self.dump_pattern_to(&mut out, pattern_string);
        crate::wtf::data_log::data_log(&out);
    }

    /// `YarrPattern::dumpPattern(PrintStream&, StringView)`.
    pub fn dump_pattern_to(&mut self, out: &mut std::string::String, pattern_string: &crate::wtf::text::wtf_string::String) {
        use std::fmt::Write as _;
        out.push_str("RegExp pattern for ");
        self.dump_pattern_string(out, pattern_string);

        if !self.flags.is_empty() {
            let mut print_separator = false;
            out.push_str(" (");
            if self.global() {
                out.push_str("global");
                print_separator = true;
            }
            if self.ignore_case() {
                if print_separator {
                    out.push('|');
                }
                out.push_str("ignore case");
                print_separator = true;
            }
            if self.multiline() {
                if print_separator {
                    out.push('|');
                }
                out.push_str("multiline");
                print_separator = true;
            }
            if self.unicode() {
                if print_separator {
                    out.push('|');
                }
                out.push_str("unicode");
                print_separator = true;
            }
            if self.unicode_sets() {
                if print_separator {
                    out.push('|');
                }
                out.push_str("unicodeSets");
                print_separator = true;
            }
            if self.sticky() {
                if print_separator {
                    out.push('|');
                }
                out.push_str("sticky");
            }
            out.push(')');
        }
        out.push_str(":\n");
        if self.specific_pattern != crate::yarr::yarr::SpecificPattern::None {
            // O `PrintStream` do C++ imprime o enum como número.
            let _ = write!(out, "    specific pattern: {}\n", self.specific_pattern as u8);
        }
        if self.has_end_anchored_fixed_size() {
            let _ = write!(out, "    end anchored fixed size: {}\n", self.end_anchored_fixed_size);
        }
        let body = self.body;
        let call_frame_size = self.disjunction(body).call_frame_size;
        if call_frame_size != 0 {
            let _ = write!(out, "    callframe size: {}\n", call_frame_size);
        }
        dump_disjunction(out, self, body, 0);
    }
}

/// `anycharCreate()`.
pub fn anychar_create() -> CharacterClass {
    let mut character_class = CharacterClass::new();
    character_class.ranges8.push(CharacterRange::new(0x00, 0xff));
    character_class.ranges32.push(CharacterRange::new(0x0100, UCHAR_MAX_VALUE));
    character_class.character_widths = CharacterClassWidths::HasBothBMPAndNonBMP;
    character_class.any_character = true;
    character_class
}

impl CharacterClass {
    /// `CharacterClass::hasSharedLeadSurrogate`.
    pub fn has_shared_lead_surrogate(&self) -> Option<u16> {
        // `U16_LEAD(supplementary)`.
        fn u16_lead(supplementary: u32) -> u16 {
            ((supplementary >> 10) + 0xd7c0) as u16
        }

        if !self.has_only_non_bmp_characters() {
            return None;
        }
        if !self.strings.is_empty() {
            return None;
        }

        debug_assert!(self.matches8.is_empty());
        debug_assert!(self.ranges8.is_empty());

        let mut common_lead_surrogate: Option<u16> = None;
        for &cp in &self.matches32 {
            debug_assert!(cp > 0xffff);
            let lead_surrogate = u16_lead(cp);
            match common_lead_surrogate {
                None => common_lead_surrogate = Some(lead_surrogate),
                Some(common) if lead_surrogate != common => return None,
                Some(_) => {}
            }
        }

        for range in &self.ranges32 {
            debug_assert!(range.begin > 0xffff);
            debug_assert!(range.end > 0xffff);
            let lead_surrogate_begin = u16_lead(range.begin);
            let lead_surrogate_end = u16_lead(range.end);
            if lead_surrogate_begin != lead_surrogate_end {
                return None;
            }

            match common_lead_surrogate {
                None => common_lead_surrogate = Some(lead_surrogate_begin),
                Some(common) if lead_surrogate_begin != common => return None,
                Some(_) => {}
            }
        }

        common_lead_surrogate
    }
}

/// `class FirstCharacterBitmapBuilder`. O `pattern` é o dono dos ids que os termos guardam.
struct FirstCharacterBitmapBuilder<'p> {
    pattern: &'p crate::yarr::yarr_pattern::YarrPattern,
    bitmap: &'p mut BitSet,
    gave_up: bool,
}

impl<'p> FirstCharacterBitmapBuilder<'p> {
    fn new(pattern: &'p crate::yarr::yarr_pattern::YarrPattern, bitmap: &'p mut BitSet) -> Self {
        FirstCharacterBitmapBuilder { pattern, bitmap, gave_up: false }
    }

    fn build(&mut self, body: crate::yarr::yarr_pattern::DisjunctionId) -> bool {
        self.add_disjunction(body, 0);
        !self.gave_up
    }

    fn set_bit(&mut self, c: u32) {
        if c <= 0xff {
            self.bitmap.set(c as usize);
        }
    }

    // Collects X's Latin-1 members into `target`. Every entry is clamped to 0..0xff: BitSet::set()
    // is unchecked, so an out-of-range member would be a wild write.
    fn collect_latin1_members(&mut self, cc: &CharacterClass, target: Option<&mut BitSet>) -> bool {
        if cc.any_character || !cc.strings.is_empty() {
            self.gave_up = true;
            return false;
        }
        let target = match target {
            Some(target) => target,
            None => &mut *self.bitmap,
        };
        for &c in &cc.matches8 {
            debug_assert!(is_latin1(c));
            if c <= 0xff {
                target.set(c as usize);
            }
        }
        for range in &cc.ranges8 {
            debug_assert!(is_latin1(range.begin));
            let end = range.end.min(0xff);
            for c in range.begin..=end {
                target.set(c as usize);
            }
        }
        true
    }

    fn add_character_class(&mut self, cc: &CharacterClass) {
        self.collect_latin1_members(cc, None);
    }

    fn add_inverted_character_class(&mut self, cc: &CharacterClass) {
        // For an 8-bit subject, [^X] matches byte c iff c is not in X. matches8/ranges8 fully
        // describe X's Latin-1 membership even when an m_table is also present, so the complement
        // over 0..0xff is a sound filter.
        let mut positive = BitSet::new(256);
        if !self.collect_latin1_members(cc, Some(&mut positive)) {
            return;
        }
        positive.invert();
        self.bitmap.merge(&positive);
    }

    // Sets `consumes` when the term definitely consumes >= 1 character, so it fully determines the
    // first character and scanning can stop.
    fn add_term(&mut self, term: &crate::yarr::yarr_pattern::PatternTerm, consumes: &mut bool, depth: u32) {
        use crate::yarr::yarr_pattern::{MatchDirection, PatternTermType};
        *consumes = false;
        if self.gave_up {
            return;
        }
        if term.match_direction == MatchDirection::Backward {
            self.gave_up = true;
            return;
        }

        match term.type_ {
            PatternTermType::AssertionBOL
            | PatternTermType::AssertionEOL
            | PatternTermType::AssertionWordBoundary
            | PatternTermType::ParentheticalAssertion => {}
            PatternTermType::PatternCharacter => {
                // A case-insensitive literal is always ASCII-alpha here (non-ASCII case-folding characters become character classes).
                let pattern_character = term.pattern_character();
                if term.ignore_case() && is_ascii_alpha(pattern_character) {
                    self.set_bit(to_ascii_upper(pattern_character));
                    self.set_bit(to_ascii_lower(pattern_character));
                } else {
                    self.set_bit(pattern_character);
                }
                *consumes = term.quantity_min_count > 0;
            }
            PatternTermType::CharacterClass => {
                // Classes are already case-folded at construction.
                let pattern = self.pattern;
                let character_class = pattern.character_class(term.character_class());
                if term.invert() {
                    self.add_inverted_character_class(character_class);
                } else {
                    self.add_character_class(character_class);
                }
                *consumes = term.quantity_min_count > 0;
            }
            PatternTermType::ParenthesesSubpattern => {
                if depth > 8 {
                    self.gave_up = true;
                    return;
                }
                let disjunction = term.parentheses().disjunction;
                self.add_disjunction(disjunction, depth + 1);
                *consumes = term.quantity_min_count > 0 && self.pattern.disjunction(disjunction).minimum_size >= 1;
            }
            _ => {
                self.gave_up = true;
            }
        }
    }

    // Returns true when the alternative definitely consumes >= 1 character. A false return with
    // m_gaveUp unset means the alternative can complete without consuming anything (matches empty).
    fn add_alternative(&mut self, alternative: &crate::yarr::yarr_pattern::PatternAlternative, depth: u32) -> bool {
        let mut consumes = false;
        for term in &alternative.terms {
            if consumes {
                // The first character is already pinned down, so no later term can add to the
                // bitmap - with one exception. A DotStarEnclosure is the residue left behind by
                // optimizeDotStarWrappedExpressions(), which DELETED a leading `^` and `.*` from
                // this alternative. The surviving first term is therefore not where the match
                // begins, so the bitmap we just built is a lie.
                if term.type_ == crate::yarr::yarr_pattern::PatternTermType::DotStarEnclosure {
                    self.gave_up = true;
                    return false;
                }
                continue;
            }
            self.add_term(term, &mut consumes, depth);
            if self.gave_up {
                return false;
            }
        }
        consumes
    }

    fn add_disjunction(&mut self, disjunction: crate::yarr::yarr_pattern::DisjunctionId, depth: u32) {
        let pattern = self.pattern;
        for alternative in &pattern.disjunction(disjunction).alternatives {
            let consumes = self.add_alternative(alternative, depth);
            if self.gave_up {
                return;
            }
            // A top-level alternative that can complete without consuming any character means the
            // pattern can match empty at position 0, so no first-character filter is sound. Nested
            // paren disjunctions are allowed to match empty; the enclosing paren term's own
            // `consumes` flag decides whether it contributes a guaranteed character.
            if depth == 0 && !consumes {
                self.gave_up = true;
                return;
            }
        }
    }
}

// Computes the Latin-1 first-character fast-fail bitmap for a pattern. The bitmap content is the
// same in every mode; only the precondition on where it may be applied differs, and that is
// selected from the flags here.
pub(crate) fn compute_first_character_bitmap(
    pattern_string: &crate::wtf::text::wtf_string::String,
    flags: crate::yarr::yarr_flags::FlagSet,
) -> Option<BitSet> {
    use crate::yarr::yarr_error_code::{has_error, ErrorCode};
    use crate::yarr::yarr_pattern::{MatchDirection, PatternTermType};

    let mut error_code = ErrorCode::NoError;
    // `!pattern.m_body` do C++: o corpo é sempre um id válido aqui, então só o erro conta.
    let pattern = crate::yarr::yarr_pattern::YarrPattern::new(
        pattern_string,
        flags,
        &mut error_code,
        crate::yarr::yarr::ExecutionMode::IncludeSubpatterns,
    );
    if has_error(error_code) {
        return None;
    }
    if !pattern.sticky() {
        if pattern.global() {
            return None;
        }
        if pattern.multiline() || pattern.contains_modifiers {
            return None;
        }
        // Check the leading term rather than PatternAlternative::m_startsWithBOL: the parser sets
        // that flag optimistically and recomputeStartsWithBOL() corrects it, but here an over-eager
        // flag is a wrong answer rather than a lost optimization.
        for alternative in &pattern.disjunction(pattern.body).alternatives {
            if alternative.terms.is_empty() {
                return None;
            }
            let first_term = &alternative.terms[0];
            if first_term.type_ != PatternTermType::AssertionBOL || first_term.match_direction != MatchDirection::Forward {
                return None;
            }
        }
    }
    let mut bitmap = BitSet::new(256);
    let built = {
        let mut builder = FirstCharacterBitmapBuilder::new(&pattern, &mut bitmap);
        builder.build(pattern.body)
    };
    if !built {
        return None;
    }
    Some(bitmap)
}

/// O `Delegate` que o `YarrParser` chama (no C++, o parâmetro de template do parser).
impl crate::yarr::yarr_parser::Delegate for YarrPatternConstructor<'_> {
    fn reset_for_reparsing(&mut self) {
        self.pattern.reset_for_reparsing();
        self.base_character_class_constructor.reset();
        self.current_character_class_constructor = None;
        self.error = crate::yarr::yarr_error_code::ErrorCode::NoError;
        self.parenthesis_context.reset();
        self.parenthesis_context.set_flags(self.flags);
        self.forward_references_in_lookbehind.clear();

        self.alternative = add_new_body_disjunction(self.pattern);

        self.flags = self.initial_flags;
    }

    fn assertion_bol(&mut self) {
        // A ^ anywhere inside a lookbehind (even within a lookahead nested in one)
        // constrains a position BEHIND the match: it never anchors an alternative to
        // the match start, so it must not feed the once-through/loop-copy split.
        if self.pattern.alternative(self.alternative).terms.is_empty()
            && !self.parenthesis_invert()
            && self.parenthesis_match_direction() == MatchDirection::Forward
            && !self.inside_lookbehind()
        {
            let alternative = self.pattern.alternative_mut(self.alternative);
            alternative.starts_with_bol = true;
            alternative.contains_bol = true;
            self.pattern.contains_bol = true;
        }

        let mut bol_term = PatternTerm::bol(self.flags);
        bol_term.set_match_direction(self.parenthesis_match_direction());
        self.pattern.alternative_mut(self.alternative).terms.push(bol_term);
    }

    fn assertion_eol(&mut self) {
        let term = PatternTerm::eol(self.flags);
        self.pattern.alternative_mut(self.alternative).terms.push(term);
    }

    fn assertion_word_boundary(&mut self, invert: bool) {
        let term = PatternTerm::word_boundary(invert, self.flags);
        self.pattern.alternative_mut(self.alternative).terms.push(term);
    }

    fn atom_pattern_character(&mut self, ch: u32, _hyphen_is_range: bool) {
        use crate::yarr::yarr_canonicalize::{canonical_range_info_for, CanonicalMode, UCS2CanonicalizationType};

        // We handle case-insensitive checking of unicode characters which do have both
        // cases by handling them as if they were defined using a CharacterClass.
        if !self.ignore_case() || (crate::wtf::ascii_ctype::is_ascii(ch) && !self.pattern.either_unicode()) {
            let term = PatternTerm::new_character(ch, self.flags, self.parenthesis_match_direction());
            self.pattern.alternative_mut(self.alternative).terms.push(term);
            return;
        }

        let info = canonical_range_info_for(
            ch,
            if self.pattern.either_unicode() { CanonicalMode::Unicode } else { CanonicalMode::UCS2 },
        );
        if info.type_ == UCS2CanonicalizationType::CanonicalizeUnique {
            let term = PatternTerm::new_character(ch, self.flags, self.parenthesis_match_direction());
            self.pattern.alternative_mut(self.alternative).terms.push(term);
            return;
        }

        current_ccc!(self).put_unicode_ignore_case(ch, info);
        let new_character_class = current_ccc!(self).char_class();
        let character_class = self.pattern.append_character_class(new_character_class);
        self.append_character_class_term(character_class, false);
    }

    fn atom_built_in_character_class(&mut self, class_id: crate::yarr::yarr::BuiltInCharacterClassID, invert: bool) {
        use crate::yarr::yarr::BuiltInCharacterClassID;

        match class_id {
            BuiltInCharacterClassID::DigitClassID => {
                let digits = self.pattern.digits_character_class();
                self.append_character_class_term(digits, invert);
            }
            BuiltInCharacterClassID::SpaceClassID => {
                let spaces = self.pattern.spaces_character_class();
                self.append_character_class_term(spaces, invert);
            }
            BuiltInCharacterClassID::WordClassID => {
                let word = if self.pattern.either_unicode() && self.ignore_case() {
                    self.pattern.word_unicode_ignore_case_char_character_class()
                } else {
                    self.pattern.wordchar_character_class()
                };
                self.append_character_class_term(word, invert);
            }
            BuiltInCharacterClassID::DotClassID => {
                if self.dot_all() {
                    let any = self.pattern.any_character_class();
                    self.append_character_class_term(any, false);
                } else {
                    let newline = self.pattern.newline_character_class();
                    self.append_character_class_term(newline, true);
                }
            }
            _ => {
                if crate::yarr::yarr_unicode_properties::character_class_may_contain_strings(class_id) {
                    let character_class = self.pattern.unicode_character_class_for(class_id);
                    if self.pattern.character_class(character_class).has_strings() {
                        self.expand_class_with_strings(character_class);
                        return;
                    }
                    // Fall through for the case where the characterClass REALLY doesn't have strings.
                }

                if self.is_ignore_case_unicode_property(class_id) {
                    if !invert || self.pattern.unicode_sets() {
                        if let Some(closure) = self.ignore_case_unicode_property_class(class_id) {
                            self.append_character_class_term(closure, invert);
                            return;
                        }
                    } else if let Some(core) = self.ignore_case_inverted_unicode_property_core(class_id) {
                        self.append_character_class_term(core, true);
                        return;
                    }
                }
                let character_class = self.pattern.unicode_character_class_for(class_id);
                self.append_character_class_term(character_class, invert);
            }
        }
    }

    fn atom_character_class_begin(&mut self, invert: bool) {
        self.invert_character_class = invert;

        // We may have modifiers, so set case sensitivity on the fly
        let ignore_case = self.ignore_case();
        current_ccc!(self).set_is_case_insensitive(ignore_case);
    }

    fn atom_character_class_atom(&mut self, ch: u32) {
        current_ccc!(self).put_char(ch);
    }

    fn atom_character_class_range(&mut self, begin: u32, end: u32) {
        current_ccc!(self).put_range(begin, end);
    }

    fn atom_character_class_built_in(&mut self, class_id: crate::yarr::yarr::BuiltInCharacterClassID, invert: bool) {
        use crate::yarr::yarr::BuiltInCharacterClassID;

        match class_id {
            BuiltInCharacterClassID::DigitClassID => {
                let digits = if invert { self.pattern.nondigits_character_class() } else { self.pattern.digits_character_class() };
                current_ccc!(self).append(self.pattern.character_class(digits));
            }

            BuiltInCharacterClassID::SpaceClassID => {
                let spaces = if invert { self.pattern.nonspaces_character_class() } else { self.pattern.spaces_character_class() };
                current_ccc!(self).append(self.pattern.character_class(spaces));
            }

            BuiltInCharacterClassID::WordClassID => {
                let word = if self.pattern.either_unicode() && self.ignore_case() {
                    if invert {
                        self.pattern.nonword_unicode_ignore_case_char_character_class()
                    } else {
                        self.pattern.word_unicode_ignore_case_char_character_class()
                    }
                } else if invert {
                    self.pattern.nonwordchar_character_class()
                } else {
                    self.pattern.wordchar_character_class()
                };
                current_ccc!(self).append(self.pattern.character_class(word));
            }

            _ => {
                let mut character_class = self.pattern.unicode_character_class_for(class_id);
                if self.is_ignore_case_unicode_property(class_id) {
                    if !invert || self.pattern.unicode_sets() {
                        if let Some(closure) = self.ignore_case_unicode_property_class(class_id) {
                            character_class = closure;
                        }
                    } else if let Some(core) = self.ignore_case_inverted_unicode_property_core(class_id) {
                        character_class = core; // and still inverted below: [^K] == closure(complement(X))
                    }
                }
                if !invert {
                    current_ccc!(self).append(self.pattern.character_class(character_class));
                } else {
                    current_ccc!(self).append_inverted(self.pattern.character_class(character_class));
                }
            }
        }
    }

    fn atom_class_string_disjunction(&mut self, utf32_strings: &mut Vec<Vec<u32>>) {
        current_ccc!(self).atom_class_string_disjunction(utf32_strings);
    }

    fn atom_character_class_set_op(&mut self, set_op: crate::yarr::yarr_parser::CharacterClassSetOp) {
        current_ccc!(self).combining_set_op(set_op);
    }

    fn atom_character_class_push_nested(&mut self, invert: bool) {
        let constructor = CharacterClassConstructor::new(self.ignore_case(), self.pattern.compile_mode());
        self.character_class_stack.push(constructor);
        self.current_character_class_constructor = Some(self.character_class_stack.len() - 1);
        self.invert_character_class = invert;
    }

    fn atom_character_class_pop_nested(&mut self, invert: bool) {
        let Some(mut current_constructor) = self.character_class_stack.pop() else {
            return;
        };

        if self.invert_character_class {
            current_constructor.invert_matches();
        }

        // O anterior é o base quando a pilha tinha um só elemento, senão o penúltimo (agora o último).
        self.current_character_class_constructor = self.character_class_stack.len().checked_sub(1);
        current_ccc!(self).perform_set_op_with_constructor(&current_constructor);
        self.invert_character_class = invert;
    }

    fn atom_character_class_end(&mut self) {
        use crate::yarr::yarr_error_code::ErrorCode;

        if current_ccc!(self).has_inverted_strings() {
            self.error = ErrorCode::NegatedClassSetMayContainStrings;
            return;
        }

        let new_character_class = current_ccc!(self).char_class();
        current_ccc!(self).reset();
        let has_strings = new_character_class.has_strings();

        if !has_strings {
            // addCharacterClassTerm: o termo aponta para a classe nova, que entra em
            // `character_classes` logo depois (o índice dela é o `len` de agora).
            let term = if !self.invert_character_class && new_character_class.any_character {
                let any = self.pattern.any_character_class();
                PatternTerm::new_character_class(any, false, self.flags, MatchDirection::Forward)
            } else {
                let id = CharacterClassId(self.pattern.character_classes.len() as u32);
                PatternTerm::new_character_class(id, self.invert_character_class, self.flags, MatchDirection::Forward)
            };
            self.pattern.alternative_mut(self.alternative).terms.push(term);
        } else {
            if self.invert_character_class {
                self.error = ErrorCode::NegatedClassSetMayContainStrings;
                return;
            }

            let id = self.pattern.append_character_class(new_character_class);
            self.expand_class_with_strings(id);
            return;
        }

        self.pattern.append_character_class(new_character_class);
    }

    fn atom_parentheses_subpattern_begin(&mut self, capture: bool, opt_group_name: Option<String>) {
        let subpattern_id = self.pattern.num_subpatterns + 1;
        if capture {
            self.pattern.num_subpatterns += 1;
            if let Some(group_name) = opt_group_name {
                self.add_capture_group_for_name(group_name, subpattern_id);
            }
        }

        let match_direction = self.parenthesis_match_direction();
        let parentheses_disjunction = DisjunctionId(self.pattern.disjunctions.len() as u32);
        self.pattern.disjunctions.push(PatternDisjunction::new(Some(self.alternative)));
        let term = PatternTerm::new_parentheses(
            PatternTermType::ParenthesesSubpattern,
            subpattern_id,
            parentheses_disjunction,
            self.flags,
            capture,
            false,
            match_direction,
        );
        self.pattern.alternative_mut(self.alternative).terms.push(term);
        let num_subpatterns = self.pattern.num_subpatterns;
        self.alternative = self
            .pattern
            .disjunction_mut(parentheses_disjunction)
            .add_new_alternative(parentheses_disjunction, num_subpatterns, match_direction);
        self.push_parenthesis_context();
    }

    fn atom_parenthetical_assertion_begin(&mut self, invert: bool, match_direction: MatchDirection) {
        let parentheses_disjunction = DisjunctionId(self.pattern.disjunctions.len() as u32);
        self.pattern.disjunctions.push(PatternDisjunction::new(Some(self.alternative)));
        let term = PatternTerm::new_parentheses(
            PatternTermType::ParentheticalAssertion,
            self.pattern.num_subpatterns + 1,
            parentheses_disjunction,
            self.flags,
            false,
            invert,
            match_direction,
        );
        self.pattern.alternative_mut(self.alternative).terms.push(term);
        let num_subpatterns = self.pattern.num_subpatterns;
        self.alternative = self
            .pattern
            .disjunction_mut(parentheses_disjunction)
            .add_new_alternative(parentheses_disjunction, num_subpatterns, match_direction);
        self.push_parenthesis_context();
        self.set_parenthesis_invert(invert);
        self.set_parenthesis_match_direction(match_direction);
        if match_direction == MatchDirection::Backward {
            self.pattern.contains_lookbehinds = true;
        }
    }

    fn atom_parenthetical_modifier_begin(&mut self, set: FlagSet, unset: FlagSet) {
        let match_direction = self.parenthesis_match_direction();
        let parentheses_disjunction = DisjunctionId(self.pattern.disjunctions.len() as u32);
        self.pattern.disjunctions.push(PatternDisjunction::new(Some(self.alternative)));
        let term = PatternTerm::new_parentheses(
            PatternTermType::ParenthesesSubpattern,
            self.pattern.num_subpatterns + 1,
            parentheses_disjunction,
            self.flags,
            false,
            false,
            match_direction,
        );
        self.pattern.alternative_mut(self.alternative).terms.push(term);
        let num_subpatterns = self.pattern.num_subpatterns;
        self.alternative = self
            .pattern
            .disjunction_mut(parentheses_disjunction)
            .add_new_alternative(parentheses_disjunction, num_subpatterns, match_direction);
        self.push_parenthesis_context();

        // Mark this context as a modifier, so we restore the flags afterwards
        self.parenthesis_context.set_modifier(true);
        // Keep the old flags here, so when we come back up we can get it
        self.parenthesis_context.set_flags(self.flags);
        // m_flags.add(set); m_flags.remove(unset);
        self.flags = FlagSet::from_raw((self.flags.to_raw() | set.to_raw()) & !unset.to_raw());
        self.pattern.contains_modifiers = true;
    }

    fn atom_parentheses_end(&mut self) {
        let parentheses_disjunction = self.alternative.disjunction;
        let Some(outer_alternative) = self.pattern.disjunction(parentheses_disjunction).parent else {
            unreachable!("atomParenthesesEnd: a disjunção de parênteses sempre tem pai");
        };
        self.alternative = outer_alternative;

        let mut num_bol_anchored_alts: usize = 0;
        let num_paren_alternatives = self.pattern.disjunction(parentheses_disjunction).alternatives.len();

        for alternative in &self.pattern.disjunction(parentheses_disjunction).alternatives {
            // Bubble up BOL flags
            if alternative.starts_with_bol {
                num_bol_anchored_alts += 1;
            }
        }

        if let Some(last_alternative) = self.pattern.disjunction_mut(parentheses_disjunction).alternatives.last_mut() {
            last_alternative.is_last_alternative = true;
        }

        if num_bol_anchored_alts != 0 {
            let alternative = self.pattern.alternative_mut(self.alternative);
            alternative.contains_bol = true;
            // If all the alternatives in parens start with BOL, then so does this one. Optimistic:
            // recomputeStartsWithBOL() redoes this once the terms are final.
            if num_bol_anchored_alts == num_paren_alternatives {
                alternative.starts_with_bol = true;
            }
        }

        let num_subpatterns = self.pattern.num_subpatterns;
        let last_term = self.pattern.alternative_mut(self.alternative).last_term();
        last_term.parentheses_mut().last_subpattern_id = num_subpatterns;
        let last_term = *last_term;

        if last_term.type_ == PatternTermType::ParenthesesSubpattern && last_term.capture() && self.inside_lookbehind() {
            self.resolve_forward_references_in_lookbehind_to(last_term.parentheses().subpattern_id);
        }

        let should_try_converting_forward_references_to_backreferences = last_term.type_
            == PatternTermType::ParentheticalAssertion
            && !self.forward_references_in_lookbehind.is_empty()
            && self.parenthesis_match_direction() == MatchDirection::Backward;

        if self.parenthesis_context.is_modifier() {
            self.flags = self.parenthesis_context.flags();
        }

        self.pop_parenthesis_context();

        if should_try_converting_forward_references_to_backreferences
            && self.parenthesis_match_direction() == MatchDirection::Forward
        {
            self.try_converting_forward_references_to_backreferences();
        }
    }

    fn atom_back_reference(&mut self, subpattern_id: u32) {
        if subpattern_id > self.pattern.num_subpatterns {
            let term = PatternTerm::numbered_forward_reference(self.flags);
            self.pattern.alternative_mut(self.alternative).terms.push(term);
            if self.parenthesis_match_direction() == MatchDirection::Backward {
                // When matching backwards, this forward reference could actually be
                // a backreference for a captured paren in the lookbehind yet to be parsed.
                let match_direction = self.parenthesis_match_direction();
                let alternative = self.pattern.alternative_mut(self.alternative);
                let term_index = alternative.last_term_index();
                let term = alternative.last_term();
                term.set_back_reference_subpattern_id(subpattern_id);
                term.match_direction = match_direction;
                self.forward_references_in_lookbehind
                    .push(UnresolvedForwardReference::new(self.alternative, term_index));
            }
            return;
        }

        let mut current_alternative = self.alternative;

        // Note to self: if we waited until the AST was baked, we could also remove forwards refs
        while let Some(parent_alternative) = self.pattern.disjunction(current_alternative.disjunction).parent {
            current_alternative = parent_alternative;
            let term = *self.pattern.alternative_mut(current_alternative).last_term();

            if term.type_ == PatternTermType::ParenthesesSubpattern
                && term.capture()
                && subpattern_id == term.parentheses().subpattern_id
            {
                let term = PatternTerm::numbered_forward_reference(self.flags);
                self.pattern.alternative_mut(self.alternative).terms.push(term);
                return;
            }
        }

        let term = PatternTerm::new_numbered_back_reference(subpattern_id, self.flags);
        self.pattern.alternative_mut(self.alternative).terms.push(term);
        self.pattern.contains_backreferences = true;
    }

    fn atom_named_back_reference(&mut self, subpattern_name: &String) {
        let paren_indices = self
            .pattern
            .named_group_to_paren_indices
            .get(subpattern_name)
            .cloned()
            .unwrap_or_default();

        if paren_indices.len() == 2 {
            // If this isn't a duplicate group, we need to go through the same analysis as a non-named backreferece to determine if
            // this backreference appears in the capture itself. A duplicate could be satisfied by a prior capture and therefore doesn't
            // need this analysis.
            let subpattern_id = paren_indices[paren_indices.len() - 1];

            let mut current_alternative = self.alternative;

            while let Some(parent_alternative) = self.pattern.disjunction(current_alternative.disjunction).parent {
                current_alternative = parent_alternative;
                let term = *self.pattern.alternative_mut(current_alternative).last_term();

                if term.type_ == PatternTermType::ParenthesesSubpattern
                    && term.capture()
                    && subpattern_id == term.parentheses().subpattern_id
                {
                    let term = PatternTerm::named_forward_reference(self.flags);
                    self.pattern.alternative_mut(self.alternative).terms.push(term);
                    return;
                }
            }
        }

        // parenIndices.last() is the highest subpattern id carrying this name (duplicates included);
        // if even that one closed before the outermost enclosing lookbehind opened, every group of
        // this name lies before it and this is an ordinary (duplicate-aware) named backreference.
        let last_paren_index = paren_indices.last().copied().unwrap_or_default();
        let mut captured_before_lookbehind = false;
        if self.parenthesis_match_direction() == MatchDirection::Backward && paren_indices.len() >= 2 {
            let mut outermost_lookbehind_first_subpattern_id = self.pattern.num_subpatterns + 1;
            let mut ancestor = self.alternative;
            while let Some(parent_alternative) = self.pattern.disjunction(ancestor.disjunction).parent {
                let enclosing = *self.pattern.alternative_mut(parent_alternative).last_term();
                if enclosing.type_ == PatternTermType::ParentheticalAssertion
                    && enclosing.match_direction() == MatchDirection::Backward
                {
                    outermost_lookbehind_first_subpattern_id = enclosing.parentheses().subpattern_id;
                }
                ancestor = parent_alternative;
            }
            captured_before_lookbehind = last_paren_index < outermost_lookbehind_first_subpattern_id;
        }

        let match_direction = self.parenthesis_match_direction();
        if match_direction == MatchDirection::Forward || captured_before_lookbehind {
            let term = PatternTerm::named_back_reference(last_paren_index, self.flags);
            self.pattern.alternative_mut(self.alternative).terms.push(term);
            self.pattern.alternative_mut(self.alternative).last_term().match_direction = match_direction;
            self.pattern.contains_backreferences = true;
            return;
        }

        // When part of a lookbehind, it could be the case that a prior alternative has a duplicate
        // named capture. Therefore we create a ForwardReference that will be converted to a
        // Backreference when the lookbehind or alternative is closed.
        let term = PatternTerm::named_forward_reference(self.flags);
        self.pattern.alternative_mut(self.alternative).terms.push(term);
        let num_subpatterns = self.pattern.num_subpatterns;
        let alternative = self.pattern.alternative_mut(self.alternative);
        let term_index = alternative.last_term_index();
        let term = alternative.last_term();
        term.match_direction = match_direction;
        // We record the current subpatternId, which we use when we try to convert to a back reference.
        // To convert this forward reference to a back reference, the patternId for the named groups must be greater than the
        // subpatternId we save here. We'll change it then.
        term.set_back_reference_subpattern_id(num_subpatterns);
        self.forward_references_in_lookbehind.push(UnresolvedForwardReference::new_with_named_group(
            self.alternative,
            term_index,
            subpattern_name.clone(),
        ));
    }

    fn atom_named_forward_reference(&mut self, subpattern_name: &String) {
        let term = PatternTerm::named_forward_reference(self.flags);
        self.pattern.alternative_mut(self.alternative).terms.push(term);

        if self.parenthesis_match_direction() == MatchDirection::Backward {
            let match_direction = self.parenthesis_match_direction();
            let alternative = self.pattern.alternative_mut(self.alternative);
            let term_index = alternative.last_term_index();
            alternative.last_term().match_direction = match_direction;
            self.forward_references_in_lookbehind.push(UnresolvedForwardReference::new_with_named_group(
                self.alternative,
                term_index,
                subpattern_name.clone(),
            ));
        }
    }

    /// `quantifyAtom`.
    fn quantify_atom(&mut self, min: u32, max: u32, greedy: bool) {
        use crate::yarr::yarr::QUANTIFY_INFINITE;
        use crate::yarr::yarr_pattern::{MatchDirection, PatternTermType, QuantifierType};

        debug_assert!(min <= max);
        let alt = self.alternative;
        debug_assert!(!self.pattern.alternative(alt).terms.is_empty());

        // Um grupo ou asserção que ancorou a alternativa no início da entrada (veja
        // atomParenthesesEnd) deixa de ancorar se puder casar zero vezes: a âncora é opcional, então
        // a alternativa pode casar em qualquer lugar. Isto precede o tratamento de {0} abaixo, que
        // remove o termo inteiro.
        if min == 0 && self.pattern.alternative(alt).terms.len() == 1 {
            self.pattern.alternative_mut(alt).starts_with_bol = false;
        }

        if max == 0 {
            // No casamento de parênteses para trás, uma referência para frente quantificada com {0}
            // pode ser elidida. Se registramos uma UnresolvedForwardReference para este termo, ela
            // sai da lista.
            if self.parenthesis_match_direction() == MatchDirection::Backward
                && !self.forward_references_in_lookbehind.is_empty()
            {
                let last_term_index = self.pattern.alternative(alt).last_term_index();
                if let Some(most_recent) = self.forward_references_in_lookbehind.last() {
                    if most_recent.alternative() == alt && most_recent.term_index() == last_term_index {
                        self.forward_references_in_lookbehind.pop();
                    }
                }
            }
            self.pattern.alternative_mut(alt).remove_last_term();
            return;
        }

        let last_index = self.pattern.alternative(alt).last_term_index() as usize;
        let mut term = self.pattern.alternative(alt).terms[last_index];
        debug_assert!(term.type_ as u8 > PatternTermType::AssertionWordBoundary as u8);
        debug_assert!(term.quantity_min_count == 1 && term.quantity_max_count == 1 && term.quantity_type == QuantifierType::FixedCount);

        if term.type_ == PatternTermType::ParentheticalAssertion {
            // Se uma asserção é quantificada com mínimo zero, ela simplesmente pode ser removida. Isso
            // vem do comportamento do RepeatMatcher na especificação. Casar uma asserção nunca consome
            // entrada, mas a continuação passada à asserção (passos 8c e 9 da definição do
            // RepeatMatcher, ES5.1 15.10.2.5) rejeita todo casamento de tamanho zero (passo 2.1). Um
            // casamento da continuação da expressão ainda é aceito (passos 8a e 11): o resultado é
            // que os casamentos da asserção não são necessários e não seriam aceitos de qualquer
            // modo, então nunca é preciso executá-la.
            if min == 0 {
                self.pattern.alternative_mut(alt).remove_last_term();
            }
            // Nunca precisamos executar uma asserção mais de uma vez. As iterações seguintes rodariam
            // com o mesmo índice inicial (asserções não capturam) e as mesmas capturas (passo 4 do
            // RepeatMatcher), então produziriam sempre o mesmo resultado. Se o primeiro casamento
            // tem sucesso, os (min - 1) seguintes também têm. Os opcionais adicionais falham (pelo
            // mesmo motivo do mínimo zero acima), mas o resultado ainda é um casamento.
            return;
        }

        let rep_type = if greedy { QuantifierType::Greedy } else { QuantifierType::NonGreedy };
        let rest = if max == QUANTIFY_INFINITE { max } else { max - min };

        if min == max {
            term.quantify_range(min, max, QuantifierType::FixedCount);
            self.pattern.alternative_mut(alt).terms[last_index] = term;
        } else if min == 0
            || (term.type_ == PatternTermType::ParenthesesSubpattern
                && (self.pattern.has_copied_paren_subexpressions || term.match_direction() == MatchDirection::Forward))
        {
            // Um único termo quantificado, compilado nativamente pelo YarrJIT
            // (opCompileParenthesesSubpattern para grupos). Parênteses para frente mantêm esta forma
            // mesmo com mínimo diferente de zero; o mesmo vale para qualquer grupo quando o padrão já
            // contém uma cópia dividida (m_hasCopiedParenSubexpressions): dividir um corpo que já
            // contém uma cópia o copia de novo, então grupos aninhados com min>0 (por exemplo
            // (?:(?:(?:a)+)+)+, em lookbehind ou para frente) fariam o padrão crescer
            // exponencialmente na profundidade do aninhamento. Só a quantificação mais interna de um
            // aninhamento assim se divide. Isso também evita os outros custos da cópia (OffsetTooLarge
            // e limites de tamanho para limites enormes, como (?:x){2147483648,...}).
            term.quantify_range(min, max, rep_type);
            self.pattern.alternative_mut(alt).terms[last_index] = term;
        } else {
            // Divide X{min,max} em uma peça FixedCount obrigatória e uma cópia opcional
            // {0,max-min} que compartilha os ids de captura de X: alcançado por átomos que não são
            // parênteses e pelo grupo quantificado mais interno de um subpadrão entre parênteses para
            // trás (lookbehind), cujo corpo espelhado passa pela maquinaria de cópia. A ordem dos
            // termos é a ordem de casamento: a ordem do fonte para frente, invertida pelo espelho nos
            // corpos para trás. isCopy marca a peça opcional; uma cópia que rodou zero iterações não
            // pode limpar os ids de captura que divide com a peça obrigatória (veja o retrocesso do
            // início de parênteses no YarrJIT).
            if term.match_direction() == MatchDirection::Forward {
                term.quantify_range(min, min, QuantifierType::FixedCount);
                self.pattern.alternative_mut(alt).terms[last_index] = term;
                // Cópias sem filtro nunca voltam mortas; a ausência de termo é o caminho de erro.
                let Some(copied) = self.copy_term(term, false) else {
                    return;
                };
                let alternative = self.pattern.alternative_mut(alt);
                alternative.terms.push(copied);
                alternative.last_term().quantify(rest, rep_type);
                if alternative.last_term().type_ == PatternTermType::ParenthesesSubpattern {
                    alternative.last_term().parentheses_mut().is_copy = true;
                }
            } else {
                let is_pending_forward_reference = (term.type_ == PatternTermType::NumberedForwardReference
                    || term.type_ == PatternTermType::NamedForwardReference)
                    && self
                        .forward_references_in_lookbehind
                        .last()
                        .is_some_and(|last| last.alternative() == alt && last.term_index() as usize == last_index);
                term.quantify(rest, rep_type);
                if term.type_ == PatternTermType::ParenthesesSubpattern {
                    term.parentheses_mut().is_copy = true;
                }
                self.pattern.alternative_mut(alt).terms[last_index] = term;
                // Cópias sem filtro nunca voltam mortas; a ausência de termo é o caminho de erro.
                let Some(copied) = self.copy_term(term, false) else {
                    return;
                };
                let alternative = self.pattern.alternative_mut(alt);
                alternative.terms.push(copied);
                alternative.last_term().quantify_range(min, min, QuantifierType::FixedCount);
                if alternative.last_term().type_ == PatternTermType::ParenthesesSubpattern {
                    alternative.last_term().parentheses_mut().is_copy = false;
                }
                if is_pending_forward_reference {
                    let new_index = self.pattern.alternative(alt).last_term_index();
                    let reference = match self.forward_references_in_lookbehind.last() {
                        Some(pending) if pending.has_named_group() => {
                            UnresolvedForwardReference::new_with_named_group(alt, new_index, pending.named_group().clone())
                        }
                        _ => UnresolvedForwardReference::new(alt, new_index),
                    };
                    self.forward_references_in_lookbehind.push(reference);
                }
            }
        }
    }

    /// `disjunction(CreateDisjunctionPurpose)`.
    fn disjunction(&mut self, purpose: crate::yarr::yarr_parser::CreateDisjunctionPurpose) {
        use crate::yarr::yarr_parser::CreateDisjunctionPurpose;

        let alt = self.alternative;
        let parent = alt.disjunction;
        if purpose == CreateDisjunctionPurpose::ForNextAlternative && self.pattern.disjunction(parent).parent.is_none() {
            // Alternativa de nível superior: registra as capturas a limpar das alternativas anteriores.
            self.pattern.alternative_mut(alt).last_subpattern_id = self.pattern.num_subpatterns;
        }

        let direction = self.parenthesis_match_direction();
        let num_subpatterns = self.pattern.num_subpatterns;
        self.alternative = self
            .pattern
            .disjunction_mut(parent)
            .add_new_alternative(parent, num_subpatterns, direction);
    }

    /// `abortedDueToError`.
    fn aborted_due_to_error(&mut self) -> bool {
        crate::yarr::yarr_error_code::has_error(self.error)
    }

    /// `abortErrorCode`.
    fn abort_error_code(&mut self) -> crate::yarr::yarr_error_code::ErrorCode {
        self.error
    }
}
