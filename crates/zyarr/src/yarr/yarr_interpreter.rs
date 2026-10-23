//! Porte de `yarr/YarrInterpreter.h` (568 linhas): o bytecode do interpretador de expressões
//! regulares (`ByteTerm`, `ByteDisjunction`, `BytecodePattern`) e as declarações de `byteCompile` e
//! `interpret`. Os corpos de `byteCompile`, `interpret` e das classes auxiliares do `YarrInterpreter.cpp`
//! entram nas fatias `yarr_interpreter_cpp*.rs`, incluídas no fim deste arquivo.
//!
//! # Modelo de posse (como em `yarr_pattern.rs`)
//!
//! - `ByteDisjunction*` de um termo de parênteses vira `ByteDisjunctionId(u32)`: posição em
//!   `BytecodePattern::all_parentheses_info` (o `Vector<std::unique_ptr<ByteDisjunction>>` do C++,
//!   que é o dono de todas as disjunções de parênteses). O corpo (`m_body`) é um `ByteDisjunction` por
//!   valor no próprio `BytecodePattern`, pois nenhum termo o aponta.
//! - `CharacterClass*` de um termo (e `newlineCharacterClass`, `wordcharCharacterClass`,
//!   `ignoreCaseWordcharCharacterClass`) vira `CharacterClassId` do `YarrPattern`. Como o C++ faz
//!   `m_userCharacterClasses.swap(pattern.m_userCharacterClasses)`, o `Vec` de classes do padrão passa
//!   para `BytecodePattern::user_character_classes` e os mesmos ids continuam valendo (as classes
//!   embutidas em cache já vivem nesse vetor, como no C++).
//! - As `union` do `ByteTerm` são achatadas em campos nomeados; os que ocupam o mesmo deslocamento no
//!   C++ (`patternCharacter`, `casedCharacter.lo`, `parenIds.subpatternId`,
//!   `assertionIds.firstSubpatternId`; e `casedCharacter.hi`, `parenIds.duplicateNamedGroupId`,
//!   `assertionIds.lastSubpatternId`) compartilham um único `u32`, com acessores pelos nomes do C++.
//!   `characterClass`, `parenthesesDisjunction`/`parenthesesWidth`, `alternative`, `anchors` e
//!   `checkInputCount` nunca são lidos por alias de outro membro, então são campos próprios.
//! - `BumpPointerAllocator*` e `ConcurrentJSLock*` não existem aqui: o alocador de contextos do
//!   interpretador é a memória do Rust e a trava só protege o JIT, que não é portado. Não há
//!   comportamento observável nessas duas coisas.
//! - `Checked<unsigned>` dos construtores é `u32` (o chamador do `.cpp` já converte com a mesma
//!   verificação de estouro).

use crate::yarr::yarr_flags::{FlagSet, Flags};
use crate::yarr::yarr_pattern::{
    CharacterClass, CharacterClassId, CompileMode, MatchDirection, QuantifierType, YarrPattern,
};

/// Índice de um `ByteDisjunction` de parênteses em `BytecodePattern::all_parentheses_info`
/// (o `ByteDisjunction*` do C++).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ByteDisjunctionId(pub u32);

/// `ByteTerm::Type`. A ordem importa: `is_character_type` e `is_cased_character_type` comparam
/// faixas, como o C++.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum ByteTermType {
    BodyAlternativeBegin,
    BodyAlternativeDisjunction,
    BodyAlternativeEnd,
    AlternativeBegin,
    AlternativeDisjunction,
    AlternativeEnd,
    SubpatternBegin,
    SubpatternEnd,
    AssertionBOL,
    AssertionEOL,
    AssertionWordBoundary,
    // Character Types
    PatternCharacterOnce,
    PatternCharacterFixed,
    PatternCharacterGreedy,
    PatternCharacterNonGreedy,
    // Cased Characeter Types
    PatternCasedCharacterOnce,
    PatternCasedCharacterFixed,
    PatternCasedCharacterGreedy,
    PatternCasedCharacterNonGreedy,
    CharacterClass,
    BackReference,
    ParenthesesSubpattern,
    ParenthesesSubpatternOnceBegin,
    ParenthesesSubpatternOnceEnd,
    ParenthesesSubpatternTerminalBegin,
    ParenthesesSubpatternTerminalEnd,
    ParentheticalAssertionBegin,
    ParentheticalAssertionEnd,
    CheckInput,
    UncheckInput,
    HaveCheckedInput,
    DotStarEnclosure,
}

/// O `struct atom` anônimo da primeira `union` do `ByteTerm`.
#[derive(Clone, Copy, Debug)]
pub struct ByteTermAtom {
    /// `patternCharacter`, `casedCharacter.lo`, `parenIds.subpatternId` e
    /// `assertionIds.firstSubpatternId` (mesmo deslocamento no C++).
    pub first_id: u32,
    /// `casedCharacter.hi`, `parenIds.duplicateNamedGroupId` e `assertionIds.lastSubpatternId`.
    pub second_id: u32,
    /// `characterClass` (o `CharacterClass*`).
    pub character_class: Option<CharacterClassId>,
    /// `parenthesesDisjunction` (o `ByteDisjunction*`).
    pub parentheses_disjunction: Option<ByteDisjunctionId>,
    /// `parenthesesWidth`.
    pub parentheses_width: u32,
    pub quantity_type: QuantifierType,
    pub quantity_min_count: u32,
    pub quantity_max_count: u32,
}

/// O `struct alternative` da `union` externa.
#[derive(Clone, Copy, Debug, Default)]
pub struct ByteTermAlternative {
    pub next: i32,
    pub end: i32,
    pub once_through: bool,
}

/// `struct ByteTerm`.
#[derive(Clone, Copy, Debug)]
pub struct ByteTerm {
    pub atom: ByteTermAtom,
    pub alternative: ByteTermAlternative,
    /// `anchors.m_bol`.
    pub anchors_bol: bool,
    /// `anchors.m_eol`.
    pub anchors_eol: bool,
    pub check_input_count: u32,
    pub frame_location: u32,
    pub type_: ByteTermType,
    pub flags: FlagSet,
    pub capture: bool,
    pub invert: bool,
    pub match_direction: MatchDirection,
    pub input_position: u32,
}

impl ByteTerm {
    /// O esqueleto comum da lista de inicialização de todos os construtores do C++
    /// (`frameLocation { 0 }`, `inputPosition { 0 }`, `m_capture(false)`, `m_invert(false)`,
    /// `m_matchDirection(Forward)`) e dos campos de átomo que cada construtor depois ajusta.
    fn blank(type_: ByteTermType, flags: FlagSet) -> ByteTerm {
        ByteTerm {
            atom: ByteTermAtom {
                first_id: 0,
                second_id: 0,
                character_class: None,
                parentheses_disjunction: None,
                parentheses_width: 0,
                quantity_type: QuantifierType::FixedCount,
                quantity_min_count: 1,
                quantity_max_count: 1,
            },
            alternative: ByteTermAlternative::default(),
            anchors_bol: false,
            anchors_eol: false,
            check_input_count: 0,
            frame_location: 0,
            type_,
            flags,
            capture: false,
            invert: false,
            match_direction: MatchDirection::Forward,
            input_position: 0,
        }
    }

    /// `ByteTerm(char32_t ch, unsigned inputPos, unsigned frameLocation, Checked<unsigned> quantityCount, QuantifierType quantityType, OptionSet<Flags> flags)`.
    pub fn new_character(
        ch: u32,
        input_pos: u32,
        frame_location: u32,
        quantity_count: u32,
        quantity_type: QuantifierType,
        flags: FlagSet,
    ) -> ByteTerm {
        let mut term = ByteTerm::blank(ByteTermType::PatternCharacterOnce, flags);
        term.frame_location = frame_location;
        term.input_position = input_pos;
        term.atom.first_id = ch;
        term.atom.quantity_type = quantity_type;
        term.atom.quantity_min_count = quantity_count;
        term.atom.quantity_max_count = quantity_count;

        match quantity_type {
            QuantifierType::FixedCount => {
                term.type_ = if quantity_count == 1 {
                    ByteTermType::PatternCharacterOnce
                } else {
                    ByteTermType::PatternCharacterFixed
                };
            }
            QuantifierType::Greedy => {
                term.atom.quantity_min_count = 0;
                term.type_ = ByteTermType::PatternCharacterGreedy;
            }
            QuantifierType::NonGreedy => {
                term.atom.quantity_min_count = 0;
                term.type_ = ByteTermType::PatternCharacterNonGreedy;
            }
        }
        term
    }

    /// `ByteTerm(char32_t lo, char32_t hi, unsigned inputPos, unsigned frameLocation, Checked<unsigned> quantityCount, QuantifierType quantityType, OptionSet<Flags> flags)`.
    pub fn new_cased_character(
        lo: u32,
        hi: u32,
        input_pos: u32,
        frame_location: u32,
        quantity_count: u32,
        quantity_type: QuantifierType,
        flags: FlagSet,
    ) -> ByteTerm {
        let mut term = ByteTerm::blank(ByteTermType::PatternCasedCharacterOnce, flags);
        term.frame_location = frame_location;
        term.input_position = input_pos;

        match quantity_type {
            QuantifierType::FixedCount => {
                term.type_ = if quantity_count == 1 {
                    ByteTermType::PatternCasedCharacterOnce
                } else {
                    ByteTermType::PatternCasedCharacterFixed
                };
                term.atom.quantity_min_count = quantity_count;
            }
            QuantifierType::Greedy => {
                term.type_ = ByteTermType::PatternCasedCharacterGreedy;
                term.atom.quantity_min_count = 0;
            }
            QuantifierType::NonGreedy => {
                term.type_ = ByteTermType::PatternCasedCharacterNonGreedy;
                term.atom.quantity_min_count = 0;
            }
        }

        term.atom.first_id = lo;
        term.atom.second_id = hi;
        term.atom.quantity_type = quantity_type;
        term.atom.quantity_max_count = quantity_count;
        term
    }

    /// `ByteTerm(CharacterClass* characterClass, bool invert, unsigned inputPos, OptionSet<Flags> flags)`.
    pub fn new_character_class(
        character_class: CharacterClassId,
        invert: bool,
        input_pos: u32,
        flags: FlagSet,
    ) -> ByteTerm {
        let mut term = ByteTerm::blank(ByteTermType::CharacterClass, flags);
        term.invert = invert;
        term.input_position = input_pos;
        term.atom.character_class = Some(character_class);
        term
    }

    /// `ByteTerm(Type type, unsigned subpatternId, ByteDisjunction* parenthesesInfo, bool capture, unsigned inputPos, OptionSet<Flags> flags)`.
    pub fn new_parentheses(
        type_: ByteTermType,
        subpattern_id: u32,
        parentheses_info: ByteDisjunctionId,
        capture: bool,
        input_pos: u32,
        flags: FlagSet,
    ) -> ByteTerm {
        let mut term = ByteTerm::blank(type_, flags);
        term.capture = capture;
        term.input_position = input_pos;
        term.atom.first_id = subpattern_id;
        term.atom.second_id = 0;
        term.atom.parentheses_disjunction = Some(parentheses_info);
        term
    }

    /// `ByteTerm(Type type, OptionSet<Flags> flags, bool invert = false)`.
    pub fn new_with_type(type_: ByteTermType, flags: FlagSet, invert: bool) -> ByteTerm {
        let mut term = ByteTerm::blank(type_, flags);
        term.invert = invert;
        term
    }

    /// `ByteTerm(Type type, unsigned subpatternId, bool capture, bool invert, unsigned inputPos, OptionSet<Flags> flags)`.
    pub fn new_subpattern(
        type_: ByteTermType,
        subpattern_id: u32,
        capture: bool,
        invert: bool,
        input_pos: u32,
        flags: FlagSet,
    ) -> ByteTerm {
        let mut term = ByteTerm::blank(type_, flags);
        term.capture = capture;
        term.invert = invert;
        term.input_position = input_pos;
        term.atom.first_id = subpattern_id;
        term.atom.second_id = 0;
        term
    }

    /// `ByteTerm(Type type, unsigned subpatternId, bool capture, bool invert, MatchDirection matchDirection, unsigned inputPos, OptionSet<Flags> flags)`.
    pub fn new_subpattern_directed(
        type_: ByteTermType,
        subpattern_id: u32,
        capture: bool,
        invert: bool,
        match_direction: MatchDirection,
        input_pos: u32,
        flags: FlagSet,
    ) -> ByteTerm {
        let mut term = ByteTerm::new_subpattern(type_, subpattern_id, capture, invert, input_pos, flags);
        term.match_direction = match_direction;
        term
    }

    /// `ByteTerm::BOL`.
    pub fn bol(input_pos: u32, flags: FlagSet) -> ByteTerm {
        let mut term = ByteTerm::new_with_type(ByteTermType::AssertionBOL, flags, false);
        term.input_position = input_pos;
        term
    }

    /// `ByteTerm::CheckInput`.
    pub fn check_input(count: u32, flags: FlagSet) -> ByteTerm {
        let mut term = ByteTerm::new_with_type(ByteTermType::CheckInput, flags, false);
        term.check_input_count = count;
        term
    }

    /// `ByteTerm::UncheckInput`.
    pub fn uncheck_input(count: u32, flags: FlagSet) -> ByteTerm {
        let mut term = ByteTerm::new_with_type(ByteTermType::UncheckInput, flags, false);
        term.check_input_count = count;
        term
    }

    /// `ByteTerm::HaveCheckedInput`.
    pub fn have_checked_input(count: u32, flags: FlagSet) -> ByteTerm {
        let mut term = ByteTerm::new_with_type(ByteTermType::HaveCheckedInput, flags, false);
        term.check_input_count = count;
        term
    }

    /// `ByteTerm::EOL`.
    pub fn eol(input_pos: u32, flags: FlagSet) -> ByteTerm {
        let mut term = ByteTerm::new_with_type(ByteTermType::AssertionEOL, flags, false);
        term.input_position = input_pos;
        term
    }

    /// `ByteTerm::WordBoundary`.
    pub fn word_boundary(
        invert: bool,
        match_direction: MatchDirection,
        input_pos: u32,
        flags: FlagSet,
    ) -> ByteTerm {
        let mut term = ByteTerm::new_with_type(ByteTermType::AssertionWordBoundary, flags, invert);
        term.match_direction = match_direction;
        term.input_position = input_pos;
        term
    }

    /// `ByteTerm::BackReference`.
    pub fn back_reference(
        subpattern_id: u32,
        match_direction: MatchDirection,
        input_pos: u32,
        flags: FlagSet,
    ) -> ByteTerm {
        ByteTerm::new_subpattern_directed(
            ByteTermType::BackReference,
            subpattern_id,
            false,
            false,
            match_direction,
            input_pos,
            flags,
        )
    }

    /// `ByteTerm::BodyAlternativeBegin`.
    pub fn body_alternative_begin(once_through: bool, flags: FlagSet) -> ByteTerm {
        ByteTerm::alternative_term(ByteTermType::BodyAlternativeBegin, once_through, flags)
    }

    /// `ByteTerm::BodyAlternativeDisjunction`.
    pub fn body_alternative_disjunction(once_through: bool, flags: FlagSet) -> ByteTerm {
        ByteTerm::alternative_term(ByteTermType::BodyAlternativeDisjunction, once_through, flags)
    }

    /// `ByteTerm::BodyAlternativeEnd`.
    pub fn body_alternative_end(flags: FlagSet) -> ByteTerm {
        ByteTerm::alternative_term(ByteTermType::BodyAlternativeEnd, false, flags)
    }

    /// `ByteTerm::AlternativeBegin`.
    pub fn alternative_begin(flags: FlagSet) -> ByteTerm {
        ByteTerm::alternative_term(ByteTermType::AlternativeBegin, false, flags)
    }

    /// `ByteTerm::AlternativeDisjunction`.
    pub fn alternative_disjunction(flags: FlagSet) -> ByteTerm {
        ByteTerm::alternative_term(ByteTermType::AlternativeDisjunction, false, flags)
    }

    /// `ByteTerm::AlternativeEnd`.
    pub fn alternative_end(flags: FlagSet) -> ByteTerm {
        ByteTerm::alternative_term(ByteTermType::AlternativeEnd, false, flags)
    }

    /// O corpo comum dos seis construtores estáticos de alternativa: `next = 0`, `end = 0`,
    /// `onceThrough`.
    fn alternative_term(type_: ByteTermType, once_through: bool, flags: FlagSet) -> ByteTerm {
        let mut term = ByteTerm::new_with_type(type_, flags, false);
        term.alternative.next = 0;
        term.alternative.end = 0;
        term.alternative.once_through = once_through;
        term
    }

    /// `ByteTerm::SubpatternBegin`.
    pub fn subpattern_begin(flags: FlagSet) -> ByteTerm {
        ByteTerm::new_with_type(ByteTermType::SubpatternBegin, flags, false)
    }

    /// `ByteTerm::SubpatternEnd`.
    pub fn subpattern_end(flags: FlagSet) -> ByteTerm {
        ByteTerm::new_with_type(ByteTermType::SubpatternEnd, flags, false)
    }

    /// `ByteTerm::ParentheticalAssertionBegin`.
    pub fn parenthetical_assertion_begin(
        first_subpattern_id: u32,
        invert: bool,
        match_direction: MatchDirection,
        flags: FlagSet,
    ) -> ByteTerm {
        let mut term = ByteTerm::new_with_type(ByteTermType::ParentheticalAssertionBegin, flags, false);
        term.atom.first_id = first_subpattern_id;
        term.invert = invert;
        term.match_direction = match_direction;
        term
    }

    /// `ByteTerm::ParentheticalAssertionEnd`.
    pub fn parenthetical_assertion_end(
        first_subpattern_id: u32,
        last_subpattern_id: u32,
        invert: bool,
        match_direction: MatchDirection,
        flags: FlagSet,
    ) -> ByteTerm {
        let mut term = ByteTerm::new_with_type(ByteTermType::ParentheticalAssertionEnd, flags, false);
        term.atom.first_id = first_subpattern_id;
        term.atom.second_id = last_subpattern_id;
        term.invert = invert;
        term.match_direction = match_direction;
        term
    }

    /// `ByteTerm::DotStarEnclosure`.
    pub fn dot_star_enclosure(bol_anchor: bool, eol_anchor: bool, flags: FlagSet) -> ByteTerm {
        let mut term = ByteTerm::new_with_type(ByteTermType::DotStarEnclosure, flags, false);
        term.anchors_bol = bol_anchor;
        term.anchors_eol = eol_anchor;
        term
    }

    pub fn is_character_type(&self) -> bool {
        self.type_ >= ByteTermType::PatternCharacterOnce && self.type_ <= ByteTermType::PatternCharacterNonGreedy
    }

    pub fn is_cased_character_type(&self) -> bool {
        self.type_ >= ByteTermType::PatternCasedCharacterOnce
            && self.type_ <= ByteTermType::PatternCasedCharacterNonGreedy
    }

    pub fn is_character_class(&self) -> bool {
        self.type_ == ByteTermType::CharacterClass
    }

    pub fn contains_any_captures(&self) -> bool {
        debug_assert!(
            self.type_ == ByteTermType::ParentheticalAssertionBegin
                || self.type_ == ByteTermType::ParentheticalAssertionEnd
        );
        self.last_subpattern_id() >= self.first_subpattern_id()
    }

    /// `atom.patternCharacter`.
    pub fn pattern_character(&self) -> u32 {
        self.atom.first_id
    }

    /// `atom.casedCharacter.lo`.
    pub fn cased_character_lo(&self) -> u32 {
        self.atom.first_id
    }

    /// `atom.casedCharacter.hi`.
    pub fn cased_character_hi(&self) -> u32 {
        self.atom.second_id
    }

    pub fn subpattern_id(&self) -> u32 {
        self.atom.first_id
    }

    pub fn duplicate_named_group_id(&self) -> u32 {
        self.atom.second_id
    }

    pub fn first_subpattern_id(&self) -> u32 {
        self.atom.first_id
    }

    pub fn last_subpattern_id(&self) -> u32 {
        self.atom.second_id
    }

    pub fn invert(&self) -> bool {
        self.invert
    }

    pub fn match_direction(&self) -> MatchDirection {
        self.match_direction
    }

    pub fn capture(&self) -> bool {
        self.capture
    }

    pub fn ignore_case(&self) -> bool {
        self.flags.contains(Flags::IgnoreCase)
    }

    pub fn multiline(&self) -> bool {
        self.flags.contains(Flags::Multiline)
    }

    pub fn dot_all(&self) -> bool {
        self.flags.contains(Flags::DotAll)
    }
}

/// `class ByteDisjunction`.
#[derive(Clone, Debug)]
pub struct ByteDisjunction {
    pub terms: Vec<ByteTerm>,
    pub num_subpatterns: u32,
    pub frame_size: u32,
}

impl ByteDisjunction {
    pub fn new(num_subpatterns: u32, frame_size: u32) -> ByteDisjunction {
        ByteDisjunction { terms: Vec::new(), num_subpatterns, frame_size }
    }

    /// `estimatedSizeInBytes`.
    pub fn estimated_size_in_bytes(&self) -> usize {
        self.terms.capacity() * std::mem::size_of::<ByteTerm>()
    }
}

/// `struct BytecodePattern`.
#[derive(Debug)]
pub struct BytecodePattern {
    pub body: ByteDisjunction,
    pub flags: FlagSet,

    pub num_duplicate_named_capture_groups: u32,
    pub end_anchored_fixed_size: u32,
    pub offset_vector_base_for_named_captures: u32,
    pub offsets_size: u32,
    pub duplicate_named_group_for_subpattern_id: Vec<u32>,

    pub newline_character_class: CharacterClassId,
    pub wordchar_character_class: CharacterClassId,
    pub ignore_case_wordchar_character_class: CharacterClassId,

    all_parentheses_info: Vec<ByteDisjunction>,
    user_character_classes: Vec<CharacterClass>,
}

impl BytecodePattern {
    /// O construtor do C++. As classes embutidas em cache são pedidas ao padrão antes de o vetor de
    /// classes trocar de dono, na mesma ordem do C++ (o acessor pode acrescentar a classe ao vetor).
    pub fn new(
        mut body: ByteDisjunction,
        parentheses_info_to_adopt: &mut Vec<ByteDisjunction>,
        pattern: &mut YarrPattern,
        offset_vector_base_for_named_captures: u32,
        offsets_size: u32,
    ) -> BytecodePattern {
        body.terms.shrink_to_fit();

        let newline_character_class = pattern.newline_character_class();
        let ignore_case_wordchar_character_class = if pattern.either_unicode() {
            pattern.word_unicode_ignore_case_char_character_class()
        } else {
            pattern.wordchar_character_class()
        };
        let wordchar_character_class = pattern.wordchar_character_class();

        let mut all_parentheses_info = std::mem::take(parentheses_info_to_adopt);
        all_parentheses_info.shrink_to_fit();

        let mut user_character_classes = std::mem::take(&mut pattern.character_classes);
        user_character_classes.shrink_to_fit();

        BytecodePattern {
            body,
            flags: pattern.flags,
            num_duplicate_named_capture_groups: pattern.num_duplicate_named_capture_groups,
            end_anchored_fixed_size: pattern.end_anchored_fixed_size,
            offset_vector_base_for_named_captures,
            offsets_size,
            duplicate_named_group_for_subpattern_id: pattern.duplicate_named_group_for_subpattern_id.clone(),
            newline_character_class,
            wordchar_character_class,
            ignore_case_wordchar_character_class,
            all_parentheses_info,
            user_character_classes,
        }
    }

    pub fn estimated_size_in_bytes(&self) -> usize {
        self.body.estimated_size_in_bytes()
    }

    pub fn has_duplicate_named_capture_groups(&self) -> bool {
        self.num_duplicate_named_capture_groups != 0
    }

    pub fn has_end_anchored_fixed_size(&self) -> bool {
        self.end_anchored_fixed_size != YarrPattern::END_ANCHORED_FIXED_SIZE_NOT_SET
    }

    pub fn offset_for_duplicate_named_group_id(&self, duplicate_named_group_id: u32) -> u32 {
        debug_assert!(duplicate_named_group_id != 0);
        self.offset_vector_base_for_named_captures + duplicate_named_group_id - 1
    }

    pub fn compile_mode(&self) -> CompileMode {
        if self.unicode() {
            return CompileMode::Unicode;
        }

        if self.unicode_sets() {
            return CompileMode::UnicodeSets;
        }

        CompileMode::Legacy
    }

    pub fn ignore_case(&self) -> bool {
        self.flags.contains(Flags::IgnoreCase)
    }

    pub fn multiline(&self) -> bool {
        self.flags.contains(Flags::Multiline)
    }

    pub fn has_indices(&self) -> bool {
        self.flags.contains(Flags::HasIndices)
    }

    pub fn sticky(&self) -> bool {
        self.flags.contains(Flags::Sticky)
    }

    pub fn unicode(&self) -> bool {
        self.flags.contains(Flags::Unicode)
    }

    pub fn unicode_sets(&self) -> bool {
        self.flags.contains(Flags::UnicodeSets)
    }

    pub fn either_unicode(&self) -> bool {
        self.unicode() || self.unicode_sets()
    }

    pub fn dot_all(&self) -> bool {
        self.flags.contains(Flags::DotAll)
    }

    /// Desreferência de um `CharacterClass*` de termo (`m_userCharacterClasses`).
    pub fn character_class(&self, id: CharacterClassId) -> &CharacterClass {
        &self.user_character_classes[id.0 as usize]
    }

    /// Desreferência de um `ByteDisjunction*` de termo de parênteses (`m_allParenthesesInfo`).
    pub fn parentheses_disjunction(&self, id: ByteDisjunctionId) -> &ByteDisjunction {
        &self.all_parentheses_info[id.0 as usize]
    }
}

// `std::unique_ptr<BytecodePattern> byteCompile(YarrPattern&, BumpPointerAllocator*, ErrorCode&, ConcurrentJSLock* = nullptr)`
// definida em yarr_interpreter_cpp*.rs, como
// `pub fn byte_compile(pattern: &mut YarrPattern, error_code: &mut ErrorCode) -> Option<Box<BytecodePattern>>`.
//
// `unsigned interpret(BytecodePattern*, StringView input, unsigned start, unsigned* output)`
// definida em yarr_interpreter_cpp*.rs, como
// `pub fn interpret(pattern: &BytecodePattern, input: StringView, start: u32, output: &mut [u32]) -> u32`.

include!("yarr_interpreter_cpp1.rs");

include!("yarr_interpreter_cpp2.rs");
include!("yarr_interpreter_cpp3.rs");
include!("yarr_interpreter_cpp4.rs");
include!("yarr_interpreter_cpp5.rs");
include!("yarr_interpreter_cpp6.rs");
