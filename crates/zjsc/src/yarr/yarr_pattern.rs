//! Porte de `yarr/YarrPattern.h`: a representação em árvore de uma expressão regular compilada
//! (`YarrPattern`, `PatternDisjunction`, `PatternAlternative`, `PatternTerm`, `CharacterClass`).
//!
//! # Modelo de posse (vale para o `YarrPattern.cpp`, o `YarrParser` e o `YarrInterpreter`)
//!
//! No C++, o `YarrPattern` é dono dos `PatternDisjunction` (`Vector<std::unique_ptr<...>>`) e dos
//! `CharacterClass` de usuário, e o resto da árvore os aponta com ponteiros crus: o pai de uma
//! alternativa (`m_parent`), o pai de uma disjunção (`m_parent`), a disjunção de um termo de
//! parênteses (`term.parentheses.disjunction`), a classe de um termo (`term.characterClass`), o
//! corpo (`m_body`) e as classes em cache (`anycharCached` etc.).
//!
//! Em Rust, o `YarrPattern` guarda `disjunctions: Vec<PatternDisjunction>` e
//! `character_classes: Vec<CharacterClass>`, e cada ponteiro vira um índice estável (as listas só
//! crescem, nunca se reordenam; `reset_for_reparsing` as esvazia junto com todos os ids):
//!
//! - `DisjunctionId(u32)`: posição em `YarrPattern::disjunctions`;
//! - `AlternativeId { disjunction, index }`: posição em `PatternDisjunction::alternatives` da
//!   disjunção indicada (o `Vector<std::unique_ptr<PatternAlternative>>` vira `Vec<PatternAlternative>`
//!   por valor, pois o endereço da alternativa nunca é guardado fora do `AlternativeId`);
//! - `CharacterClassId(u32)`: posição em `YarrPattern::character_classes`.
//!
//! Para ler ou alterar o alvo de um id, use `YarrPattern::disjunction`, `disjunction_mut`,
//! `alternative`, `alternative_mut`, `character_class` e `character_class_mut`. Um ponteiro nulo do
//! C++ (`PatternDisjunction::m_parent` do corpo) vira `Option`. Como uma disjunção não sabe o próprio
//! índice, `PatternDisjunction::add_new_alternative` recebe o `DisjunctionId` de `this`.
//!
//! # Fora deste módulo, por ora
//!
//! Ficam para o porte do `YarrPattern.cpp` (no mesmo arquivo): o construtor que compila o padrão, as
//! funções `*Create` das classes embutidas (geradas em `derived/.../RegExpJitTables.h`) e os
//! acessores em cache que as chamam (`anyCharacterClass`, `newlineCharacterClass`, ...,
//! `unicodeCharacterClassFor`; o cache e o `append_character_class` já estão aqui),
//! `CharacterClass::hasSharedLeadSurrogate`, `computeFirstCharacterBitmap` e as rotinas `dump*`
//! (depuração).

use std::collections::HashMap;
use std::ops::{BitAnd, BitOr, BitOrAssign, Deref, DerefMut};

use crate::wtf::text::wtf_string::String;
use crate::yarr::reg_exp_jit_tables;
use crate::yarr::yarr::{BuiltInCharacterClassID, ExecutionMode, SpecificPattern};
use crate::yarr::yarr_unicode_properties::create_unicode_character_class_for;
use crate::yarr::yarr_flags::{FlagSet, Flags};

/// `UCHAR_MAX_VALUE` do ICU: o maior ponto de código Unicode.
pub const UCHAR_MAX_VALUE: u32 = 0x10ffff;

/// `enum class CompileMode : uint8_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CompileMode {
    Legacy,
    Unicode,
    UnicodeSets,
}

/// Índice de uma `PatternDisjunction` em `YarrPattern::disjunctions` (o `PatternDisjunction*` do C++).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DisjunctionId(pub u32);

/// Índice de uma `PatternAlternative` (o `PatternAlternative*` do C++).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AlternativeId {
    pub disjunction: DisjunctionId,
    pub index: u32,
}

/// `anycharCreate` (YarrPattern.cpp).
fn anychar_create() -> CharacterClass {
    let mut class = CharacterClass::new();
    class.ranges8.push(CharacterRange::new(0x00, 0xff));
    class.ranges32.push(CharacterRange::new(0x0100, UCHAR_MAX_VALUE));
    class.character_widths = CharacterClassWidths::HasBothBMPAndNonBMP;
    class.any_character = true;
    class
}

/// Os acessores preguiçosos de classes embutidas de `YarrPattern`: criam a classe na primeira
/// chamada (`m_userCharacterClasses.append(xxxCreate())`) e guardam o ponteiro no campo `xxxCached`.
macro_rules! lazy_character_class {
    ($($method:ident, $field:ident, $create:path;)*) => {
        impl YarrPattern {
            $(
                pub fn $method(&mut self) -> CharacterClassId {
                    if let Some(cached) = self.$field {
                        return cached;
                    }
                    let id = self.append_character_class($create());
                    self.$field = Some(id);
                    id
                }
            )*
        }
    };
}

lazy_character_class! {
    any_character_class, anychar_cached, anychar_create;
    newline_character_class, newline_cached, reg_exp_jit_tables::newline_create;
    digits_character_class, digits_cached, reg_exp_jit_tables::digits_create;
    spaces_character_class, spaces_cached, reg_exp_jit_tables::spaces_create;
    wordchar_character_class, wordchar_cached, reg_exp_jit_tables::wordchar_create;
    word_unicode_ignore_case_char_character_class, word_unicode_ignore_case_char_cached, reg_exp_jit_tables::word_unicode_ignore_case_char_create;
    nondigits_character_class, nondigits_cached, reg_exp_jit_tables::nondigits_create;
    nonspaces_character_class, nonspaces_cached, reg_exp_jit_tables::nonspaces_create;
    nonwordchar_character_class, nonwordchar_cached, reg_exp_jit_tables::nonwordchar_create;
    nonword_unicode_ignore_case_char_character_class, nonword_unicode_ignore_case_char_cached, reg_exp_jit_tables::nonword_unicode_ignore_case_char_create;
}

/// Índice de uma `CharacterClass` em `YarrPattern::character_classes` (o `CharacterClass*` do C++).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CharacterClassId(pub u32);

/// `struct CharacterRange`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CharacterRange {
    pub begin: u32,
    pub end: u32,
}

impl CharacterRange {
    pub const fn new(begin: u32, end: u32) -> Self {
        CharacterRange { begin, end }
    }
}

/// `enum struct CharacterClassWidths : unsigned char`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CharacterClassWidths {
    Unknown = 0x0,
    HasBMPChars = 0x1,
    HasNonBMPChars = 0x2,
    HasBothBMPAndNonBMP = 0x3,
}

impl CharacterClassWidths {
    const fn from_bits(bits: u8) -> Self {
        match bits & 0x3 {
            0 => CharacterClassWidths::Unknown,
            1 => CharacterClassWidths::HasBMPChars,
            2 => CharacterClassWidths::HasNonBMPChars,
            _ => CharacterClassWidths::HasBothBMPAndNonBMP,
        }
    }
}

/// `operator|(CharacterClassWidths, CharacterClassWidths)`.
impl BitOr for CharacterClassWidths {
    type Output = CharacterClassWidths;

    fn bitor(self, rhs: CharacterClassWidths) -> CharacterClassWidths {
        CharacterClassWidths::from_bits(self as u8 | rhs as u8)
    }
}

/// `operator&(CharacterClassWidths, CharacterClassWidths)`, que devolve `bool`.
impl BitAnd for CharacterClassWidths {
    type Output = bool;

    fn bitand(self, rhs: CharacterClassWidths) -> bool {
        (self as u8 & rhs as u8) != 0
    }
}

/// `operator|=(CharacterClassWidths&, CharacterClassWidths)`.
impl BitOrAssign for CharacterClassWidths {
    fn bitor_assign(&mut self, rhs: CharacterClassWidths) {
        *self = *self | rhs;
    }
}

/// `CharacterClass::Table` (`const char*`): tabela estática de 65536 entradas.
pub type CharacterClassTable = &'static [u8];

/// `CharacterClass::ByteTable`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ByteTable {
    pub data: [u8; CharacterClass::LATIN1_TABLE_SIZE as usize],
}

impl ByteTable {
    pub fn new() -> Self {
        ByteTable {
            data: [0; CharacterClass::LATIN1_TABLE_SIZE as usize],
        }
    }
}

impl Default for ByteTable {
    fn default() -> Self {
        ByteTable::new()
    }
}

/// `struct CharacterClass`.
///
/// Toda instância tem o conjunto completo de `matches` e `ranges`; pode ter ainda uma `table` para
/// consultas mais rápidas (que precisa coincidir com eles).
#[derive(Clone, Debug)]
pub struct CharacterClass {
    pub strings: Vec<Vec<u32>>,
    pub matches8: Vec<u32>,
    pub ranges8: Vec<CharacterRange>,
    pub matches32: Vec<u32>,
    pub ranges32: Vec<CharacterRange>,
    pub latin1_table: Option<Box<ByteTable>>,
    pub table: Option<CharacterClassTable>,
    pub character_widths: CharacterClassWidths,
    pub table_inverted: bool,
    pub any_character: bool,
    pub in_canonical_form: bool,
}

impl CharacterClass {
    pub const TABLE_SIZE: u32 = 65536;
    pub const LATIN1_TABLE_SIZE: u32 = 256;

    /// `CharacterClass()`.
    pub fn new() -> Self {
        CharacterClass {
            strings: Vec::new(),
            matches8: Vec::new(),
            ranges8: Vec::new(),
            matches32: Vec::new(),
            ranges32: Vec::new(),
            latin1_table: None,
            table: None,
            character_widths: CharacterClassWidths::Unknown,
            table_inverted: false,
            any_character: false,
            in_canonical_form: false,
        }
    }

    /// `CharacterClass(Table table, bool inverted)`.
    pub fn with_table(table: CharacterClassTable, inverted: bool) -> Self {
        let mut class = CharacterClass::new();
        class.table = Some(table);
        class.table_inverted = inverted;
        class
    }

    /// `CharacterClass(matches8, ranges8, matches32, ranges32, widths)`.
    pub fn with_matches(
        matches8: &[u32],
        ranges8: &[CharacterRange],
        matches32: &[u32],
        ranges32: &[CharacterRange],
        widths: CharacterClassWidths,
    ) -> Self {
        let mut class = CharacterClass::new();
        class.matches8 = matches8.to_vec();
        class.ranges8 = ranges8.to_vec();
        class.matches32 = matches32.to_vec();
        class.ranges32 = ranges32.to_vec();
        class.character_widths = widths;
        class
    }

    /// `CharacterClass(strings, matches8, ranges8, matches32, ranges32, widths, inCanonicalForm)`.
    pub fn with_strings(
        strings: Vec<Vec<u32>>,
        matches8: &[u32],
        ranges8: &[CharacterRange],
        matches32: &[u32],
        ranges32: &[CharacterRange],
        widths: CharacterClassWidths,
        in_canonical_form: bool,
    ) -> Self {
        let mut class = CharacterClass::with_matches(matches8, ranges8, matches32, ranges32, widths);
        class.strings = strings;
        class.in_canonical_form = in_canonical_form;
        class
    }

    pub fn has_non_bmp_characters(&self) -> bool {
        self.character_widths & CharacterClassWidths::HasNonBMPChars
    }

    pub fn has_one_character_size(&self) -> bool {
        self.character_widths == CharacterClassWidths::HasBMPChars
            || self.character_widths == CharacterClassWidths::HasNonBMPChars
    }

    pub fn has_only_non_bmp_characters(&self) -> bool {
        self.character_widths == CharacterClassWidths::HasNonBMPChars
    }

    pub fn has_strings(&self) -> bool {
        !self.strings.is_empty()
    }

    pub fn has_single_characters(&self) -> bool {
        !self.matches8.is_empty()
            || !self.ranges8.is_empty()
            || !self.matches32.is_empty()
            || !self.ranges32.is_empty()
    }
}

impl Default for CharacterClass {
    fn default() -> Self {
        CharacterClass::new()
    }
}

/// `struct ClassSet : public CharacterClass`.
///
/// O C++ redeclara `m_strings` e `m_inCanonicalForm` na derivada, escondendo os da base; as duas
/// cópias existem de verdade (`base.strings` e `strings`) e os métodos herdados (via `Deref`) olham
/// as da base, como no C++ (não há `virtual`).
#[derive(Clone, Debug)]
pub struct ClassSet {
    pub base: CharacterClass,
    pub strings: Vec<Vec<u32>>,
    pub in_canonical_form: bool,
}

impl ClassSet {
    /// `ClassSet()`.
    pub fn new() -> Self {
        ClassSet {
            base: CharacterClass::new(),
            strings: Vec::new(),
            in_canonical_form: true,
        }
    }

    /// `ClassSet(const char* table, bool inverted)`.
    pub fn with_table(table: CharacterClassTable, inverted: bool) -> Self {
        ClassSet {
            base: CharacterClass::with_table(table, inverted),
            strings: Vec::new(),
            in_canonical_form: true,
        }
    }

    /// `ClassSet(matches8, ranges8, matches32, ranges32, widths)`.
    pub fn with_matches(
        matches8: &[u32],
        ranges8: &[CharacterRange],
        matches32: &[u32],
        ranges32: &[CharacterRange],
        widths: CharacterClassWidths,
    ) -> Self {
        ClassSet {
            base: CharacterClass::with_matches(matches8, ranges8, matches32, ranges32, widths),
            strings: Vec::new(),
            in_canonical_form: true,
        }
    }

    /// `ClassSet(strings, matches8, ranges8, matches32, ranges32, widths)`: as `strings` ficam na
    /// derivada, a base é construída sem elas.
    pub fn with_strings_and_matches(
        strings: Vec<Vec<u32>>,
        matches8: &[u32],
        ranges8: &[CharacterRange],
        matches32: &[u32],
        ranges32: &[CharacterRange],
        widths: CharacterClassWidths,
    ) -> Self {
        ClassSet {
            base: CharacterClass::with_matches(matches8, ranges8, matches32, ranges32, widths),
            strings,
            in_canonical_form: true,
        }
    }

    /// `ClassSet(strings, inCanonicalForm)`.
    pub fn with_strings(strings: Vec<Vec<u32>>, in_canonical_form: bool) -> Self {
        ClassSet {
            base: CharacterClass::new(),
            strings,
            in_canonical_form,
        }
    }
}

impl Default for ClassSet {
    fn default() -> Self {
        ClassSet::new()
    }
}

impl Deref for ClassSet {
    type Target = CharacterClass;

    fn deref(&self) -> &CharacterClass {
        &self.base
    }
}

impl DerefMut for ClassSet {
    fn deref_mut(&mut self) -> &mut CharacterClass {
        &mut self.base
    }
}

/// `enum class QuantifierType : uint8_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum QuantifierType {
    FixedCount,
    Greedy,
    NonGreedy,
}

/// `enum MatchDirection : uint8_t`. O código assume que `Forward` é 0 e `Backward` é 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MatchDirection {
    Forward = 0,
    Backward = 1,
}

/// `PatternTerm::Type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PatternTermType {
    AssertionBOL,
    AssertionEOL,
    AssertionWordBoundary,
    PatternCharacter,
    CharacterClass,
    NumberedBackReference,
    NamedBackReference,
    NumberedForwardReference,
    NamedForwardReference,
    ParenthesesSubpattern,
    ParentheticalAssertion,
    DotStarEnclosure,
}

/// O membro `parentheses` da união anônima do `PatternTerm`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TermParentheses {
    pub disjunction: DisjunctionId,
    pub subpattern_id: u32,
    pub last_subpattern_id: u32,
    pub is_copy: bool,
    pub is_terminal: bool,
    pub is_string_list: bool,
    pub is_eol_string_list: bool,
}

/// O membro `anchors` da união anônima do `PatternTerm`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TermAnchors {
    pub bol_anchor: bool,
    pub eol_anchor: bool,
}

/// A união anônima do `PatternTerm` (`patternCharacter`, `characterClass`,
/// `backReferenceSubpatternId`, `parentheses`, `anchors`). O C++ deixa a união sem inicializar nos
/// termos sem carga (`BOL`, `EOL`, `WordBoundary`); aqui isso é `None`. Ler um membro que não é o
/// ativo (o que no C++ lê lixo) entra em pânico: é violação de invariante do porte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TermPayload {
    None,
    PatternCharacter(u32),
    CharacterClass(CharacterClassId),
    BackReferenceSubpatternId(u32),
    Parentheses(TermParentheses),
    Anchors(TermAnchors),
}

/// `struct PatternTerm`.
///
/// `quantityMinCount` e `quantityMaxCount` são `Checked<unsigned>` no C++ (abortam no estouro);
/// aqui são `u32`, e a aritmética sobre eles no porte do `.cpp` deve usar operações verificadas
/// (`checked_add(...)` seguido de aborto).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatternTerm {
    pub type_: PatternTermType,
    pub current_flags: FlagSet,
    pub capture: bool,
    pub invert: bool,
    pub match_direction: MatchDirection,
    pub quantity_type: QuantifierType,
    pub quantity_min_count: u32,
    pub quantity_max_count: u32,
    pub payload: TermPayload,
    pub input_position: u32,
    pub frame_location: u32,
    /// Marcado pela passada de auto-possessificação do `YarrPattern` num termo guloso de um só
    /// caractere (`PatternCharacter`/`CharacterClass`) cujo termo obrigatório seguinte nunca casa um
    /// caractere que este casa. Voltar atrás nele é sempre inútil, então o JIT gera um retrocesso
    /// possessivo (sem devolver) em vez do laço de nova tentativa.
    pub possessive: bool,
}

impl PatternTerm {
    fn base(type_: PatternTermType, current_flags: FlagSet, capture: bool, invert: bool, match_direction: MatchDirection, payload: TermPayload) -> Self {
        PatternTerm {
            type_,
            current_flags,
            capture,
            invert,
            match_direction,
            quantity_type: QuantifierType::FixedCount,
            quantity_min_count: 1,
            quantity_max_count: 1,
            payload,
            input_position: 0,
            frame_location: 0,
            possessive: false,
        }
    }

    /// `PatternTerm(char32_t ch, OptionSet<Flags> currFlags, MatchDirection = Forward)`.
    pub fn new_character(ch: u32, current_flags: FlagSet, match_direction: MatchDirection) -> Self {
        PatternTerm::base(PatternTermType::PatternCharacter, current_flags, false, false, match_direction, TermPayload::PatternCharacter(ch))
    }

    /// `PatternTerm(CharacterClass*, bool invert, OptionSet<Flags>, MatchDirection = Forward)`.
    pub fn new_character_class(character_class: CharacterClassId, invert: bool, current_flags: FlagSet, match_direction: MatchDirection) -> Self {
        PatternTerm::base(PatternTermType::CharacterClass, current_flags, false, invert, match_direction, TermPayload::CharacterClass(character_class))
    }

    /// `PatternTerm(Type, unsigned subpatternId, PatternDisjunction*, OptionSet<Flags>, bool capture = false,
    /// bool invert = false, MatchDirection = Forward)`.
    pub fn new_parentheses(
        type_: PatternTermType,
        subpattern_id: u32,
        disjunction: DisjunctionId,
        current_flags: FlagSet,
        capture: bool,
        invert: bool,
        match_direction: MatchDirection,
    ) -> Self {
        let parentheses = TermParentheses {
            disjunction,
            subpattern_id,
            last_subpattern_id: 0,
            is_copy: false,
            is_terminal: false,
            is_string_list: false,
            is_eol_string_list: false,
        };
        PatternTerm::base(type_, current_flags, capture, invert, match_direction, TermPayload::Parentheses(parentheses))
    }

    /// `PatternTerm(Type, OptionSet<Flags>, bool invert = false)`: termos sem carga.
    pub fn new_with_type(type_: PatternTermType, current_flags: FlagSet, invert: bool) -> Self {
        PatternTerm::base(type_, current_flags, false, invert, MatchDirection::Forward, TermPayload::None)
    }

    /// `PatternTerm(unsigned spatternId, OptionSet<Flags>)`: referência numerada.
    pub fn new_numbered_back_reference(subpattern_id: u32, current_flags: FlagSet) -> Self {
        PatternTerm::base(
            PatternTermType::NumberedBackReference,
            current_flags,
            false,
            false,
            MatchDirection::Forward,
            TermPayload::BackReferenceSubpatternId(subpattern_id),
        )
    }

    /// `PatternTerm(bool bolAnchor, bool eolAnchor, OptionSet<Flags>)`: `DotStarEnclosure`.
    pub fn new_dot_star_enclosure(bol_anchor: bool, eol_anchor: bool, current_flags: FlagSet) -> Self {
        PatternTerm::base(
            PatternTermType::DotStarEnclosure,
            current_flags,
            false,
            false,
            MatchDirection::Forward,
            TermPayload::Anchors(TermAnchors { bol_anchor, eol_anchor }),
        )
    }

    /// `PatternTerm::NamedBackReference`.
    pub fn named_back_reference(subpattern_id: u32, current_flags: FlagSet) -> Self {
        let mut term = PatternTerm::new_numbered_back_reference(subpattern_id, current_flags);
        term.type_ = PatternTermType::NamedBackReference;
        term
    }

    /// `PatternTerm::NumberedForwardReference`.
    pub fn numbered_forward_reference(current_flags: FlagSet) -> Self {
        let mut term = PatternTerm::new_with_type(PatternTermType::NumberedForwardReference, current_flags, false);
        term.payload = TermPayload::BackReferenceSubpatternId(0);
        term
    }

    /// `PatternTerm::NamedForwardReference`.
    pub fn named_forward_reference(current_flags: FlagSet) -> Self {
        let mut term = PatternTerm::new_with_type(PatternTermType::NamedForwardReference, current_flags, false);
        term.payload = TermPayload::BackReferenceSubpatternId(0);
        term
    }

    /// `PatternTerm::BOL`.
    pub fn bol(current_flags: FlagSet) -> Self {
        PatternTerm::new_with_type(PatternTermType::AssertionBOL, current_flags, false)
    }

    /// `PatternTerm::EOL`.
    pub fn eol(current_flags: FlagSet) -> Self {
        PatternTerm::new_with_type(PatternTermType::AssertionEOL, current_flags, false)
    }

    /// `PatternTerm::WordBoundary`.
    pub fn word_boundary(invert: bool, current_flags: FlagSet) -> Self {
        PatternTerm::new_with_type(PatternTermType::AssertionWordBoundary, current_flags, invert)
    }

    pub fn convert_to_numbered_backreference(&mut self) {
        self.type_ = PatternTermType::NumberedBackReference;
    }

    pub fn convert_to_named_backreference(&mut self) {
        self.type_ = PatternTermType::NamedBackReference;
    }

    /// Membro `patternCharacter` da união.
    pub fn pattern_character(&self) -> u32 {
        match self.payload {
            TermPayload::PatternCharacter(ch) => ch,
            _ => panic!("PatternTerm sem patternCharacter"),
        }
    }

    /// Membro `characterClass` da união.
    pub fn character_class(&self) -> CharacterClassId {
        match self.payload {
            TermPayload::CharacterClass(id) => id,
            _ => panic!("PatternTerm sem characterClass"),
        }
    }

    /// Membro `backReferenceSubpatternId` da união.
    pub fn back_reference_subpattern_id(&self) -> u32 {
        match self.payload {
            TermPayload::BackReferenceSubpatternId(id) => id,
            _ => panic!("PatternTerm sem backReferenceSubpatternId"),
        }
    }

    pub fn set_back_reference_subpattern_id(&mut self, subpattern_id: u32) {
        self.payload = TermPayload::BackReferenceSubpatternId(subpattern_id);
    }

    /// Membro `parentheses` da união.
    pub fn parentheses(&self) -> &TermParentheses {
        match &self.payload {
            TermPayload::Parentheses(parentheses) => parentheses,
            _ => panic!("PatternTerm sem parentheses"),
        }
    }

    pub fn parentheses_mut(&mut self) -> &mut TermParentheses {
        match &mut self.payload {
            TermPayload::Parentheses(parentheses) => parentheses,
            _ => panic!("PatternTerm sem parentheses"),
        }
    }

    /// Membro `anchors` da união.
    pub fn anchors(&self) -> &TermAnchors {
        match &self.payload {
            TermPayload::Anchors(anchors) => anchors,
            _ => panic!("PatternTerm sem anchors"),
        }
    }

    pub fn anchors_mut(&mut self) -> &mut TermAnchors {
        match &mut self.payload {
            TermPayload::Anchors(anchors) => anchors,
            _ => panic!("PatternTerm sem anchors"),
        }
    }

    pub fn invert(&self) -> bool {
        self.invert
    }

    pub fn set_match_direction(&mut self, match_direction: MatchDirection) {
        self.match_direction = match_direction;
    }

    pub fn match_direction(&self) -> MatchDirection {
        self.match_direction
    }

    pub fn capture(&self) -> bool {
        self.capture
    }

    pub fn possessive(&self) -> bool {
        self.possessive
    }

    pub fn ignore_case(&self) -> bool {
        self.current_flags.contains(Flags::IgnoreCase)
    }

    pub fn multiline(&self) -> bool {
        self.current_flags.contains(Flags::Multiline)
    }

    pub fn dot_all(&self) -> bool {
        self.current_flags.contains(Flags::DotAll)
    }

    /// `isFixedWidthCharacterClass`: o `CharacterClass*` do C++ é resolvido em `character_classes`
    /// (`YarrPattern::character_classes`).
    pub fn is_fixed_width_character_class(&self, character_classes: &[CharacterClass]) -> bool {
        self.type_ == PatternTermType::CharacterClass
            && character_classes[self.character_class().0 as usize].has_one_character_size()
            && !self.invert()
    }

    pub fn contains_any_captures(&self) -> bool {
        let parentheses = self.parentheses();
        parentheses.last_subpattern_id != 0 && parentheses.last_subpattern_id >= parentheses.subpattern_id
    }

    /// `quantify(unsigned count, QuantifierType)`.
    pub fn quantify(&mut self, count: u32, type_: QuantifierType) {
        self.quantity_min_count = 0;
        self.quantity_max_count = count;
        self.quantity_type = type_;
    }

    /// `quantify(unsigned minCount, unsigned maxCount, QuantifierType)`.
    pub fn quantify_range(&mut self, min_count: u32, max_count: u32, type_: QuantifierType) {
        self.quantity_min_count = min_count;
        self.quantity_max_count = max_count;
        self.quantity_type = type_;
    }
}

/// `struct PatternAlternative`.
#[derive(Clone, Debug)]
pub struct PatternAlternative {
    pub terms: Vec<PatternTerm>,
    pub parent: DisjunctionId,
    pub minimum_size: u32,
    pub first_subpattern_id: u32,
    pub last_subpattern_id: u32,
    pub direction: MatchDirection,
    pub once_through: bool,
    pub has_fixed_size: bool,
    pub starts_with_bol: bool,
    pub contains_bol: bool,
    pub is_last_alternative: bool,
}

impl PatternAlternative {
    pub fn new(disjunction: DisjunctionId, first_subpattern_id: u32, match_direction: MatchDirection) -> Self {
        PatternAlternative {
            terms: Vec::new(),
            parent: disjunction,
            minimum_size: 0,
            first_subpattern_id,
            last_subpattern_id: 0,
            direction: match_direction,
            once_through: false,
            has_fixed_size: false,
            starts_with_bol: false,
            contains_bol: false,
            is_last_alternative: false,
        }
    }

    pub fn last_term_index(&self) -> u32 {
        self.terms.len() as u32 - 1
    }

    pub fn last_term(&mut self) -> &mut PatternTerm {
        let index = self.last_term_index() as usize;
        &mut self.terms[index]
    }

    pub fn remove_last_term(&mut self) {
        let size = self.terms.len();
        self.terms.truncate(size - 1);
    }

    pub fn set_once_through(&mut self) {
        self.once_through = true;
    }

    pub fn once_through(&self) -> bool {
        self.once_through
    }

    pub fn need_to_cleanup_captures(&self) -> bool {
        self.last_subpattern_id != 0
    }

    pub fn first_cleanup_subpattern_id(&self) -> u32 {
        let mut first_subpattern_id_to_clear = self.first_subpattern_id;

        // Queremos limpar subpadrões, que começam em 1.
        if first_subpattern_id_to_clear == 0 {
            first_subpattern_id_to_clear += 1;
        }

        first_subpattern_id_to_clear
    }

    pub fn match_direction(&self) -> MatchDirection {
        self.direction
    }
}

/// `struct PatternDisjunction`.
#[derive(Clone, Debug)]
pub struct PatternDisjunction {
    pub alternatives: Vec<PatternAlternative>,
    pub parent: Option<AlternativeId>,
    pub minimum_size: u32,
    pub call_frame_size: u32,
    pub has_fixed_size: bool,
}

impl PatternDisjunction {
    pub fn new(parent: Option<AlternativeId>) -> Self {
        PatternDisjunction {
            alternatives: Vec::new(),
            parent,
            minimum_size: 0,
            call_frame_size: 0,
            has_fixed_size: false,
        }
    }

    /// `addNewAlternative`. `self_id` é o índice desta disjunção em `YarrPattern::disjunctions`
    /// (o `this` do C++). Os padrões do C++ são `firstSubpatternId = 1` e `Forward`.
    pub fn add_new_alternative(&mut self, self_id: DisjunctionId, first_subpattern_id: u32, match_direction: MatchDirection) -> AlternativeId {
        let index = self.alternatives.len() as u32;
        self.alternatives.push(PatternAlternative::new(self_id, first_subpattern_id, match_direction));
        AlternativeId { disjunction: self_id, index }
    }
}

/// `struct TermChain`.
#[derive(Clone, Debug)]
pub struct TermChain {
    pub term: PatternTerm,
    pub hot_terms: Vec<TermChain>,
}

impl TermChain {
    pub fn new(term: PatternTerm) -> Self {
        TermChain { term, hot_terms: Vec::new() }
    }
}

/// `struct YarrPattern`.
///
/// O construtor do C++ (`YarrPattern(StringView, OptionSet<Flags>, ErrorCode&, ExecutionMode)`)
/// inicializa os campos e chama `compile`; `with_flags` faz só a primeira parte, e a compilação é
/// do porte do `YarrPattern.cpp`.
#[derive(Debug)]
pub struct YarrPattern {
    pub contains_backreferences: bool,
    pub contains_bol: bool,
    pub contains_lookbehinds: bool,
    pub contains_unsigned_length_pattern: bool,
    pub contains_modifiers: bool,
    pub has_copied_paren_subexpressions: bool,
    pub has_named_capture_groups: bool,
    pub save_initial_start_value: bool,
    pub execution_mode: ExecutionMode,
    pub flags: FlagSet,
    pub specific_pattern: SpecificPattern,
    pub end_anchored_fixed_size: u32,
    pub num_subpatterns: u32,
    pub initial_start_value_frame_location: u32,
    pub num_duplicate_named_capture_groups: u32,
    /// Maior tamanho de frame interior (`m_callFrameSize - (base+4)`) de qualquer termo
    /// `ParenthesesSubpattern` repetitivo.
    pub max_paren_context_frame_size: u32,
    /// O `PatternDisjunction* m_body`: o corpo é uma das disjunções de `disjunctions`.
    pub body: DisjunctionId,
    pub disjunctions: Vec<PatternDisjunction>,
    pub character_classes: Vec<CharacterClass>,
    pub capture_group_names: Vec<String>,
    /// O conteúdo do vetor da direita depende de o nome ser duplicado ou não. Para um grupo nomeado
    /// usado uma só vez no padrão, o vetor tem tamanho um e a única entrada é o `subpatternId` do
    /// grupo não duplicado. Para um grupo nomeado duplicado, o tamanho é maior que 2: a primeira
    /// entrada é o `duplicateNamedGroupId` e as seguintes são os `subpatternId` desse
    /// `duplicateNamedGroupId`.
    pub named_group_to_paren_indices: HashMap<String, Vec<u32>>,
    pub duplicate_named_group_for_subpattern_id: Vec<u32>,
    pub atom: String,

    #[allow(dead_code)]
    anychar_cached: Option<CharacterClassId>,
    #[allow(dead_code)]
    newline_cached: Option<CharacterClassId>,
    #[allow(dead_code)]
    digits_cached: Option<CharacterClassId>,
    #[allow(dead_code)]
    spaces_cached: Option<CharacterClassId>,
    #[allow(dead_code)]
    wordchar_cached: Option<CharacterClassId>,
    #[allow(dead_code)]
    word_unicode_ignore_case_char_cached: Option<CharacterClassId>,
    #[allow(dead_code)]
    nondigits_cached: Option<CharacterClassId>,
    #[allow(dead_code)]
    nonspaces_cached: Option<CharacterClassId>,
    #[allow(dead_code)]
    nonwordchar_cached: Option<CharacterClassId>,
    #[allow(dead_code)]
    nonword_unicode_ignore_case_char_cached: Option<CharacterClassId>,
    #[allow(dead_code)]
    unicode_properties_cached: HashMap<u32, CharacterClassId>,
}

impl YarrPattern {
    pub const END_ANCHORED_FIXED_SIZE_NOT_SET: u32 = u32::MAX;

    /// A lista de inicialização do construtor do C++, sem o `compile`.
    pub fn with_flags(flags: FlagSet, execution_mode: ExecutionMode) -> Self {
        YarrPattern {
            contains_backreferences: false,
            contains_bol: false,
            contains_lookbehinds: false,
            contains_unsigned_length_pattern: false,
            contains_modifiers: false,
            has_copied_paren_subexpressions: false,
            has_named_capture_groups: false,
            save_initial_start_value: false,
            execution_mode,
            flags,
            specific_pattern: SpecificPattern::None,
            end_anchored_fixed_size: YarrPattern::END_ANCHORED_FIXED_SIZE_NOT_SET,
            num_subpatterns: 0,
            initial_start_value_frame_location: 0,
            num_duplicate_named_capture_groups: 0,
            max_paren_context_frame_size: 0,
            body: DisjunctionId(0),
            disjunctions: Vec::new(),
            character_classes: Vec::new(),
            capture_group_names: Vec::new(),
            named_group_to_paren_indices: HashMap::new(),
            duplicate_named_group_for_subpattern_id: Vec::new(),
            atom: String::default(),
            anychar_cached: None,
            newline_cached: None,
            digits_cached: None,
            spaces_cached: None,
            wordchar_cached: None,
            word_unicode_ignore_case_char_cached: None,
            nondigits_cached: None,
            nonspaces_cached: None,
            nonwordchar_cached: None,
            nonword_unicode_ignore_case_char_cached: None,
            unicode_properties_cached: HashMap::new(),
        }
    }

    /// `resetForReparsing`. Como no C++, `contains_modifiers` não é zerado.
    pub fn reset_for_reparsing(&mut self) {
        self.num_subpatterns = 0;
        self.initial_start_value_frame_location = 0;
        self.num_duplicate_named_capture_groups = 0;
        self.max_paren_context_frame_size = 0;

        self.contains_backreferences = false;
        self.contains_bol = false;
        self.contains_lookbehinds = false;
        self.contains_unsigned_length_pattern = false;
        self.has_copied_paren_subexpressions = false;
        self.has_named_capture_groups = false;
        self.save_initial_start_value = false;

        self.anychar_cached = None;
        self.newline_cached = None;
        self.digits_cached = None;
        self.spaces_cached = None;
        self.wordchar_cached = None;
        self.word_unicode_ignore_case_char_cached = None;
        self.nondigits_cached = None;
        self.nonspaces_cached = None;
        self.nonwordchar_cached = None;
        self.nonword_unicode_ignore_case_char_cached = None;
        self.unicode_properties_cached.clear();

        self.disjunctions.clear();
        self.character_classes.clear();
        self.capture_group_names.clear();
        self.named_group_to_paren_indices.clear();
        self.duplicate_named_group_for_subpattern_id.clear();
    }

    pub fn disjunction(&self, id: DisjunctionId) -> &PatternDisjunction {
        &self.disjunctions[id.0 as usize]
    }

    pub fn disjunction_mut(&mut self, id: DisjunctionId) -> &mut PatternDisjunction {
        &mut self.disjunctions[id.0 as usize]
    }

    pub fn alternative(&self, id: AlternativeId) -> &PatternAlternative {
        &self.disjunctions[id.disjunction.0 as usize].alternatives[id.index as usize]
    }

    pub fn alternative_mut(&mut self, id: AlternativeId) -> &mut PatternAlternative {
        &mut self.disjunctions[id.disjunction.0 as usize].alternatives[id.index as usize]
    }

    pub fn character_class(&self, id: CharacterClassId) -> &CharacterClass {
        &self.character_classes[id.0 as usize]
    }

    pub fn character_class_mut(&mut self, id: CharacterClassId) -> &mut CharacterClass {
        &mut self.character_classes[id.0 as usize]
    }

    /// `m_userCharacterClasses.append(...)` seguido de `.last().get()`.
    pub fn append_character_class(&mut self, character_class: CharacterClass) -> CharacterClassId {
        self.character_classes.push(character_class);
        CharacterClassId(self.character_classes.len() as u32 - 1)
    }

    pub fn contains_unsigned_length_pattern(&self) -> bool {
        self.contains_unsigned_length_pattern
    }

    /// `unicodeCharacterClassFor`: cria a classe na primeira chamada e guarda o ponteiro por id.
    pub fn unicode_character_class_for(&mut self, unicode_class_id: BuiltInCharacterClassID) -> CharacterClassId {
        debug_assert!(unicode_class_id.0 >= BuiltInCharacterClassID::BaseUnicodePropertyID.0);
        let class_id = unicode_class_id.0;
        if let Some(cached) = self.unicode_properties_cached.get(&class_id) {
            return *cached;
        }
        let id = self.append_character_class(*create_unicode_character_class_for(unicode_class_id));
        self.unicode_properties_cached.insert(class_id, id);
        id
    }

    /// Os dois slots de frame do `DotStarEnclosure` (`YarrStackSpaceForDotStarEnclosure`): o
    /// deslocamento de onde o casamento começou e o fim do trecho sem quebra de linha que o segue.
    pub fn initial_start_frame_location(&self) -> u32 {
        self.initial_start_value_frame_location
    }

    pub fn no_newline_before_frame_location(&self) -> u32 {
        self.initial_start_value_frame_location + 1
    }

    pub fn offset_vector_base_for_named_captures(&self) -> u32 {
        (self.num_subpatterns + 1) * 2
    }

    pub fn offsets_size(&self) -> u32 {
        self.offset_vector_base_for_named_captures() + self.num_duplicate_named_capture_groups
    }

    pub fn offset_for_duplicate_named_group_id(&self, duplicate_named_group_id: u32) -> u32 {
        self.offset_vector_base_for_named_captures() + duplicate_named_group_id - 1
    }

    pub fn global(&self) -> bool {
        self.flags.contains(Flags::Global)
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

    pub fn has_duplicate_named_capture_groups(&self) -> bool {
        self.num_duplicate_named_capture_groups != 0
    }

    pub fn has_end_anchored_fixed_size(&self) -> bool {
        self.end_anchored_fixed_size != YarrPattern::END_ANCHORED_FIXED_SIZE_NOT_SET
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
}

/// `struct BackTrackInfoPatternCharacter`. Os campos são `uintptr_t`; os `*Index` são o
/// `offsetof(campo) / sizeof(uintptr_t)`, ou seja, a posição do campo.
#[derive(Clone, Copy, Debug, Default)]
pub struct BackTrackInfoPatternCharacter {
    pub begin: usize, // Só necessário para padrões unicode.
    pub match_amount: usize,
}

impl BackTrackInfoPatternCharacter {
    pub const fn begin_index() -> u32 {
        0
    }
    pub const fn match_amount_index() -> u32 {
        1
    }
}

/// `struct BackTrackInfoCharacterClass`.
#[derive(Clone, Copy, Debug, Default)]
pub struct BackTrackInfoCharacterClass {
    pub begin: usize, // Só necessário para padrões unicode.
    pub match_amount: usize,
}

impl BackTrackInfoCharacterClass {
    pub const fn begin_index() -> u32 {
        0
    }
    pub const fn match_amount_index() -> u32 {
        1
    }
}

/// `struct BackTrackInfoBackReference`.
#[derive(Clone, Copy, Debug, Default)]
pub struct BackTrackInfoBackReference {
    pub begin: usize,        // Não é realmente necessário para quantificadores gulosos.
    pub match_amount: usize, // Não é realmente necessário para quantificadores fixos.
    pub back_reference_size: usize, // Usado pelos quantificadores gulosos para voltar atrás.
    /// O trecho de uma referência para trás (lookbehind) fica à esquerda do cursor; este campo guarda
    /// a borda esquerda do trecho durante a comparação.
    pub backward_span_edge: usize,
}

impl BackTrackInfoBackReference {
    pub const fn begin_index() -> u32 {
        0
    }
    pub const fn match_amount_index() -> u32 {
        1
    }
    pub const fn back_reference_size_index() -> u32 {
        2
    }
    pub const fn backward_span_edge_index() -> u32 {
        3
    }
}

/// `struct BackTrackInfoAlternative` (uma união com um só membro, `offset`).
#[derive(Clone, Copy, Debug, Default)]
pub struct BackTrackInfoAlternative {
    pub offset: usize,
}

/// `struct BackTrackInfoParentheticalAssertion`.
#[derive(Clone, Copy, Debug, Default)]
pub struct BackTrackInfoParentheticalAssertion {
    pub begin: usize,
}

impl BackTrackInfoParentheticalAssertion {
    pub const fn begin_index() -> u32 {
        0
    }
}

/// `struct BackTrackInfoParenthesesOnce`.
#[derive(Clone, Copy, Debug, Default)]
pub struct BackTrackInfoParenthesesOnce {
    pub begin: usize,
    pub return_address: usize,
    /// Endereço do código que continua uma cadeia de despacho pelo primeiro caractere quando a
    /// alternativa recém-entrada falha.
    pub chain_resume: usize,
}

impl BackTrackInfoParenthesesOnce {
    pub const fn begin_index() -> u32 {
        0
    }
    pub const fn return_address_index() -> u32 {
        1
    }
    pub const fn chain_resume_index() -> u32 {
        2
    }
}

/// `struct BackTrackInfoParenthesesTerminal`.
#[derive(Clone, Copy, Debug, Default)]
pub struct BackTrackInfoParenthesesTerminal {
    pub begin: usize,
    pub entry_position: usize,
}

impl BackTrackInfoParenthesesTerminal {
    pub const fn begin_index() -> u32 {
        0
    }
    pub const fn entry_position_index() -> u32 {
        1
    }
}

/// `struct BackTrackInfoParentheses`.
#[derive(Clone, Copy, Debug, Default)]
pub struct BackTrackInfoParentheses {
    pub begin: usize,
    pub return_address: usize,
    pub match_amount: usize,
    pub paren_context_head: usize,
}

impl BackTrackInfoParentheses {
    pub const fn begin_index() -> u32 {
        0
    }
    pub const fn return_address_index() -> u32 {
        1
    }
    pub const fn match_amount_index() -> u32 {
        2
    }
    pub const fn paren_context_head_index() -> u32 {
        3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_operators() {
        let mut widths = CharacterClassWidths::Unknown;
        widths |= CharacterClassWidths::HasBMPChars;
        assert_eq!(widths, CharacterClassWidths::HasBMPChars);
        assert!(!(widths & CharacterClassWidths::HasNonBMPChars));
        widths |= CharacterClassWidths::HasNonBMPChars;
        assert_eq!(widths, CharacterClassWidths::HasBothBMPAndNonBMP);
        assert!(widths & CharacterClassWidths::HasNonBMPChars);
    }

    #[test]
    fn alternatives_are_indexed() {
        let mut pattern = YarrPattern::with_flags(FlagSet::empty(),ExecutionMode::IncludeSubpatterns);
        pattern.disjunctions.push(PatternDisjunction::new(None));
        let id = DisjunctionId(0);
        let alternative = pattern.disjunctions[0].add_new_alternative(id, 1, MatchDirection::Forward);
        assert_eq!(alternative, AlternativeId { disjunction: id, index: 0 });
        assert_eq!(pattern.alternative(alternative).parent, id);
        pattern.alternative_mut(alternative).terms.push(PatternTerm::new_character('a' as u32, FlagSet::empty(),MatchDirection::Forward));
        assert_eq!(pattern.alternative(alternative).last_term_index(), 0);
        assert_eq!(pattern.alternative_mut(alternative).last_term().pattern_character(), 'a' as u32);
    }

    #[test]
    fn term_quantify_and_cleanup() {
        let mut term = PatternTerm::new_parentheses(PatternTermType::ParenthesesSubpattern, 1, DisjunctionId(1), FlagSet::empty(), true, false, MatchDirection::Forward);
        term.quantify_range(2, 5, QuantifierType::Greedy);
        assert_eq!((term.quantity_min_count, term.quantity_max_count), (2, 5));
        assert!(!term.contains_any_captures());
        term.parentheses_mut().last_subpattern_id = 1;
        assert!(term.contains_any_captures());
        let alternative = PatternAlternative::new(DisjunctionId(0), 0, MatchDirection::Forward);
        assert_eq!(alternative.first_cleanup_subpattern_id(), 1);
    }
}
