//! Porte de `yarr/YarrParser.h`, parte 1 (linhas 1 a 1221 do C++: até o fim de `parseEscape`).
//!
//! # Desvios obrigatórios do modelo de posse (o C++ usa referências que se sobrepõem)
//!
//! No C++, `CharacterClassParserDelegate`, `ClassSetParserDelegate` e
//! `ClassStringDisjunctionParserDelegate` guardam `Delegate&` (o `m_delegate` do `Parser`) e
//! `ErrorCode&` (o `m_errorCode` do `Parser`) e são passados a `parseEscape()` enquanto o `Parser`
//! continua usando os mesmos dois objetos. Em Rust seguro isso é um empréstimo duplo. A tradução é:
//!
//! - as classes internas não guardam o delegate nem o código de erro: cada método que o C++ faz
//!   chamar `m_delegate.xxx(...)` ou escrever `m_errorCode` recebe `delegate: &mut D` e
//!   `error_code: &mut ErrorCode` como os dois primeiros argumentos (depois do `self`);
//! - o parâmetro `EscapeDelegate& delegate` de `parseEscape()` vira o trait `EscapeSink<D>`. Em modo
//!   `Normal` o C++ passa o próprio `m_delegate`; aqui passa-se `AtomEscapeSink`, que repassa tudo
//!   ao delegate do parser. Quem chama `parse_escape` passa `self.delegate` e `self.error_code`
//!   adiante através do sink, como em `sink.atom_pattern_character(self.delegate, &mut self.error_code, ...)`;
//! - os `template<ParseEscapeMode>` e `template<UnicodeParseContext>` viram parâmetro comum
//!   (`mode: ParseEscapeMode`, `context: UnicodeParseContext`): o comportamento observável é o mesmo.
//!
//! # Contrato com a parte 2 (`yarr_parser_part2.rs`, incluída no fim deste arquivo)
//!
//! A parte 2 começa na linha 1223 do C++ (`consumePossibleSurrogatePair`). `parseEscape` não pode
//! ser partido entre arquivos, por isso ele inteiro (e `isIdentityEscapeAnError`) está aqui. A parte 2
//! escreve um segundo `impl<'a, D: Delegate, C: CharType> Parser<'a, D, C>` com estes métodos, que
//! a parte 1 chama com estas assinaturas exatas (índice e tamanho em `u32`, caractere em `u32`):
//!
//! - `fn save_state(&self) -> ParseState`, `fn restore_state(&mut self, state: ParseState)`;
//! - `fn at_end_of_pattern(&self) -> bool`, `fn peek(&self) -> u32`, `fn peek_is_digit(&self) -> bool`;
//! - `fn consume(&mut self) -> u32`, `fn consume_number(&mut self) -> u32`,
//!   `fn consume_octal(&mut self, count: u32) -> u32`;
//! - `fn try_consume(&mut self, ch: u16) -> bool`, `fn try_consume_hex(&mut self, count: u32) -> u32`;
//! - `fn try_consume_group_name(&mut self) -> Option<String>`;
//! - `fn try_consume_unicode_property_expression(&mut self) -> Option<BuiltInCharacterClassID>`;
//! - `fn try_consume_unicode_escape(&mut self, context: UnicodeParseContext) -> u32`;
//! - `fn parse_class_string_disjunction(&mut self, disjunction_may_contain_strings: &mut bool)`;
//! - `fn is_legacy_compilation(&self)`, `is_unicode_compilation`, `is_unicode_sets_compilation`,
//!   `is_either_unicode_compilation` (todos `&self -> bool`);
//! - `fn parse_tokens(&mut self)` e `fn handle_illegal_references(&mut self)`.
//!
//! Esta parte já define `Parser` (com todos os campos, inclusive os que só a parte 2 usa),
//! `NamedCaptureGroups`, `ParseState`, `ParenthesesType`, `TokenType`, `UnicodeParseContext`,
//! `ParseEscapeMode`, as constantes `ERROR_CODE_POINT`, `MAX_PATTERN_SIZE` e `MAX_CAPTURES_COUNT`
//! e as três classes `*ParserDelegate`; a parte 2 não as redefine. O trait `Delegate` é o conceito
//! `YarrSyntaxCheckable`.

use std::collections::HashSet;

#[allow(unused_imports)]
use crate::wtf::ascii_ctype::{
    is_ascii, is_ascii_alpha, is_ascii_alphanumeric, is_ascii_digit, is_ascii_hex_digit,
    is_ascii_octal_digit, to_ascii_hex_value,
};
#[allow(unused_imports)]
use crate::wtf::text::string_impl::CharType;
#[allow(unused_imports)]
use crate::wtf::text::wtf_string::String;
#[allow(unused_imports)]
use crate::yarr::yarr::{BuiltInCharacterClassID, QUANTIFY_INFINITE, QUANTIFY_INFINITE64};
#[allow(unused_imports)]
use crate::yarr::yarr_error_code::{has_error, ErrorCode};
#[allow(unused_imports)]
use crate::yarr::yarr_flags::{FlagSet, Flags};
#[allow(unused_imports)]
use crate::yarr::yarr_pattern::{CompileMode, MatchDirection, UCHAR_MAX_VALUE};
use crate::yarr::yarr_unicode_properties::character_class_may_contain_strings;

/// `enum class CreateDisjunctionPurpose : uint8_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CreateDisjunctionPurpose {
    NotForNextAlternative,
    ForNextAlternative,
}

/// `enum class CharacterClassSetOp : uint8_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CharacterClassSetOp {
    Default,
    Union,
    Intersection,
    Subtraction,
}

/// O conceito `YarrSyntaxCheckable`: os métodos que o `Parser` chama no delegate.
///
/// Diferenças de forma em relação ao C++, sem efeito observável: `atomCharacterClassBegin()` (sem
/// argumento, parâmetro padrão) e `atomCharacterClassBegin(bool)` são um método só; o mesmo vale para
/// `atomParenthesesSubpatternBegin(bool)` e `atomParenthesesSubpatternBegin(bool, optional<String>)`
/// (o chamador de um argumento passa `None`); `atomCharacterClassAtom` recebe `char32_t` como os
/// delegates reais (o conceito só exige que `char16_t` seja aceito).
pub trait Delegate {
    fn assertion_bol(&mut self);
    fn assertion_eol(&mut self);
    fn assertion_word_boundary(&mut self, invert: bool);
    fn atom_pattern_character(&mut self, ch: u32, hyphen_is_range: bool);
    fn atom_built_in_character_class(&mut self, class_id: BuiltInCharacterClassID, invert: bool);
    fn atom_character_class_begin(&mut self, invert: bool);
    fn atom_character_class_atom(&mut self, ch: u32);
    fn atom_character_class_range(&mut self, begin: u32, end: u32);
    fn atom_character_class_built_in(&mut self, class_id: BuiltInCharacterClassID, invert: bool);
    fn atom_class_string_disjunction(&mut self, disjunction_strings: &mut Vec<Vec<u32>>);
    fn atom_character_class_set_op(&mut self, set_op: CharacterClassSetOp);
    fn atom_character_class_push_nested(&mut self, invert: bool);
    fn atom_character_class_pop_nested(&mut self, invert: bool);
    fn atom_character_class_end(&mut self);
    fn atom_parentheses_subpattern_begin(&mut self, capture: bool, group_name: Option<String>);
    fn atom_parenthetical_assertion_begin(&mut self, invert: bool, match_direction: MatchDirection);
    fn atom_parenthetical_modifier_begin(&mut self, set: FlagSet, unset: FlagSet);
    fn atom_parentheses_end(&mut self);
    fn atom_back_reference(&mut self, subpattern_id: u32);
    fn atom_named_back_reference(&mut self, subpattern_name: &String);
    fn atom_named_forward_reference(&mut self, subpattern_name: &String);
    fn quantify_atom(&mut self, min: u32, max: u32, greedy: bool);
    fn disjunction(&mut self, purpose: CreateDisjunctionPurpose);
    fn aborted_due_to_error(&mut self) -> bool;
    fn abort_error_code(&mut self) -> ErrorCode;
    fn reset_for_reparsing(&mut self);
}

/// `Parser::errorCodePoint`.
pub const ERROR_CODE_POINT: u32 = 0xFFFF_FFFF;

/// `Parser::maxPatternSize`: derivado de testes empíricos de tempo de compilação no PCRE e no WREC.
pub const MAX_PATTERN_SIZE: u32 = 1024 * 1024;

/// `Parser::maxCapturesCount`: derivado da configuração do V8.
pub const MAX_CAPTURES_COUNT: u32 = (1 << 16) / 2;

/// `Parser::UnicodeParseContext`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum UnicodeParseContext {
    PatternCodePoint,
    GroupName,
}

/// `Parser::ParseEscapeMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ParseEscapeMode {
    Normal,
    CharacterClass,
    ClassSet,
    ClassStringDisjunction,
}

/// `Parser::TokenType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TokenType {
    NotAtom = 0,
    Atom = 1,
    Lookbehind = 2,
    SetDisjunction = 3,
    SetDisjunctionMayContainStrings = 4,
}

/// `Parser::ParenthesesType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ParenthesesType {
    Subpattern,
    Assertion,
    LookbehindAssertion,
}

/// `Parser::ParseState` (`typedef unsigned`): o índice no padrão.
pub type ParseState = u32;

type GroupNameHashSet = HashSet<String>;

/// `Parser::NamedCaptureGroups`.
pub struct NamedCaptureGroups {
    // Nomes vistos na expressão inteira até aqui.
    capture_group_names: GroupNameHashSet,
    // Todos os nomes ativos das alternativas anteriores neste nível de aninhamento.
    nested_capture_group_names: Vec<GroupNameHashSet>,
    // Nomes vistos na disjunção/alternativa que contém e na alternativa corrente.
    active_capture_group_names: Vec<GroupNameHashSet>,
}

impl NamedCaptureGroups {
    pub fn new() -> Self {
        NamedCaptureGroups {
            capture_group_names: GroupNameHashSet::new(),
            nested_capture_group_names: vec![GroupNameHashSet::new()],
            active_capture_group_names: vec![GroupNameHashSet::new()],
        }
    }

    pub fn contains(&self, name: &String) -> bool {
        self.capture_group_names.contains(name)
    }

    pub fn is_empty(&self) -> bool {
        self.capture_group_names.is_empty()
    }

    pub fn reset(&mut self) {
        self.capture_group_names.clear();
        self.nested_capture_group_names.clear();
        self.nested_capture_group_names.push(GroupNameHashSet::new());
        self.active_capture_group_names.clear();
        self.active_capture_group_names.push(GroupNameHashSet::new());
    }

    pub fn next_alternative(&mut self) {
        let active_last = std::mem::take(
            self.active_capture_group_names
                .last_mut()
                .expect("active_capture_group_names nunca fica vazio"),
        );
        self.nested_capture_group_names
            .last_mut()
            .expect("nested_capture_group_names nunca fica vazio")
            .extend(active_last);

        // Para parênteses aninhados, a nova alternativa começa com os nomes de captura já vistos
        // na alternativa que a contém.
        let active_size = self.active_capture_group_names.len();
        if active_size > 1 {
            let containing = self.active_capture_group_names[active_size - 2].clone();
            self.active_capture_group_names[active_size - 1].extend(containing);
        }
    }

    pub fn push_parenthesis(&mut self) {
        let current_top = self
            .active_capture_group_names
            .last()
            .expect("active_capture_group_names nunca fica vazio")
            .clone();
        self.nested_capture_group_names.push(GroupNameHashSet::new());
        self.active_capture_group_names.push(current_top);
    }

    pub fn pop_parenthesis(&mut self) {
        let active_last = self
            .active_capture_group_names
            .pop()
            .expect("pop_parenthesis com a pilha ativa vazia");
        let mut nested_last = self
            .nested_capture_group_names
            .pop()
            .expect("pop_parenthesis com a pilha aninhada vazia");
        nested_last.extend(active_last);

        // Soma todos os nomes vistos nestes parênteses à alternativa que os contém.
        self.active_capture_group_names
            .last_mut()
            .expect("pop_parenthesis sem alternativa que contenha os parênteses")
            .extend(nested_last);
    }

    /// `add(String)`: devolve `true` quando o nome é novo na alternativa ativa (`isNewEntry`);
    /// se não é novo, o chamador deve sinalizar erro de sintaxe.
    pub fn add(&mut self, name: String) -> bool {
        self.capture_group_names.insert(name.clone());

        self.active_capture_group_names
            .last_mut()
            .expect("active_capture_group_names nunca fica vazio")
            .insert(name)
    }
}

impl Default for NamedCaptureGroups {
    fn default() -> Self {
        NamedCaptureGroups::new()
    }
}

/// O parâmetro `EscapeDelegate& delegate` de `parseEscape()`: o `m_delegate` do parser (modo
/// `Normal`) ou uma das três classes `*ParserDelegate`. Veja o comentário do módulo sobre os dois
/// primeiros argumentos de cada método.
pub trait EscapeSink<D: Delegate> {
    fn assertion_word_boundary(&mut self, delegate: &mut D, error_code: &mut ErrorCode, invert: bool);
    fn atom_pattern_character(&mut self, delegate: &mut D, error_code: &mut ErrorCode, ch: u32, hyphen_is_range: bool);
    fn atom_built_in_character_class(&mut self, delegate: &mut D, error_code: &mut ErrorCode, class_id: BuiltInCharacterClassID, invert: bool);
    fn atom_back_reference(&mut self, delegate: &mut D, error_code: &mut ErrorCode, subpattern_id: u32);
    fn atom_named_back_reference(&mut self, delegate: &mut D, error_code: &mut ErrorCode, subpattern_name: &String);
    fn atom_named_forward_reference(&mut self, delegate: &mut D, error_code: &mut ErrorCode, subpattern_name: &String);
}

/// O `m_delegate` do parser passado a `parseEscape<ParseEscapeMode::Normal>` (`parseAtomEscape`).
pub struct AtomEscapeSink;

impl<D: Delegate> EscapeSink<D> for AtomEscapeSink {
    fn assertion_word_boundary(&mut self, delegate: &mut D, _error_code: &mut ErrorCode, invert: bool) {
        delegate.assertion_word_boundary(invert);
    }

    fn atom_pattern_character(&mut self, delegate: &mut D, _error_code: &mut ErrorCode, ch: u32, hyphen_is_range: bool) {
        delegate.atom_pattern_character(ch, hyphen_is_range);
    }

    fn atom_built_in_character_class(&mut self, delegate: &mut D, _error_code: &mut ErrorCode, class_id: BuiltInCharacterClassID, invert: bool) {
        delegate.atom_built_in_character_class(class_id, invert);
    }

    fn atom_back_reference(&mut self, delegate: &mut D, _error_code: &mut ErrorCode, subpattern_id: u32) {
        delegate.atom_back_reference(subpattern_id);
    }

    fn atom_named_back_reference(&mut self, delegate: &mut D, _error_code: &mut ErrorCode, subpattern_name: &String) {
        delegate.atom_named_back_reference(subpattern_name);
    }

    fn atom_named_forward_reference(&mut self, delegate: &mut D, _error_code: &mut ErrorCode, subpattern_name: &String) {
        delegate.atom_named_forward_reference(subpattern_name);
    }
}

/// `enum class CharacterClassConstructionState` de `CharacterClassParserDelegate`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CharacterClassConstructionState {
    Empty,
    CachedCharacter,
    CachedCharacterHyphen,
    AfterCharacterClass,
    AfterCharacterClassHyphen,
}

/// `CharacterClassParserDelegate`:
///
/// Usada no parsing de classes de caracteres. Trata a detecção de intervalos de caracteres. Implementa
/// o bastante da interface de delegate para ser passada a `parseEscape()` como `EscapeDelegate`, o que
/// permite reusar `parseEscape()` para as sequências de escape dentro de classes.
pub struct CharacterClassParserDelegate {
    is_unicode: bool,
    state: CharacterClassConstructionState,
    character: u32,
}

impl CharacterClassParserDelegate {
    pub fn new(compile_mode: CompileMode) -> Self {
        CharacterClassParserDelegate {
            is_unicode: compile_mode == CompileMode::Unicode,
            state: CharacterClassConstructionState::Empty,
            character: 0,
        }
    }

    /// `begin()`: chamado no início da construção.
    pub fn begin<D: Delegate>(&mut self, delegate: &mut D, invert: bool) {
        delegate.atom_character_class_begin(invert);
    }

    /// `end()`: chamado no fim da construção.
    pub fn end<D: Delegate>(&mut self, delegate: &mut D) {
        if self.state == CharacterClassConstructionState::CachedCharacter {
            delegate.atom_character_class_atom(self.character);
        } else if self.state == CharacterClassConstructionState::CachedCharacterHyphen {
            delegate.atom_character_class_atom(self.character);
            delegate.atom_character_class_atom('-' as u32);
        }
        delegate.atom_character_class_end();
    }
}

impl<D: Delegate> EscapeSink<D> for CharacterClassParserDelegate {
    /// `atomPatternCharacter()`:
    ///
    /// Chamado por `parseCharacterClass()` (caractere sem escape na classe) ou por `parseEscape()`.
    /// No primeiro caso `hyphen_is_range` é `true`, e o hífen pode indicar um intervalo (`/[a-z]/` é
    /// diferente de `/[a\-z]/`).
    fn atom_pattern_character(&mut self, delegate: &mut D, error_code: &mut ErrorCode, ch: u32, hyphen_is_range: bool) {
        match self.state {
            CharacterClassConstructionState::AfterCharacterClass
            | CharacterClassConstructionState::Empty => {
                if self.state == CharacterClassConstructionState::AfterCharacterClass {
                    // Depois de uma classe embutida é preciso vigiar um hífen. Procuramos
                    // intervalos inválidos como /[\d-x]/ ou /[\d-\d]/. Se vemos um hífen após uma
                    // classe de caracteres, ao contrário do usual o reportamos ao delegate na hora e
                    // entramos num estado envenenado. Num padrão unicode, qualquer chamada seguinte
                    // para adicionar outro caractere ou classe resulta em erro de sintaxe. Um hífen
                    // após uma classe é válido em si, mas só no fim da regex.
                    if hyphen_is_range && ch == '-' as u32 {
                        delegate.atom_character_class_atom('-' as u32);
                        self.state = CharacterClassConstructionState::AfterCharacterClassHyphen;
                        return;
                    }
                    // Senão cai no caso Empty: caractere em cache.
                }
                self.character = ch;
                self.state = CharacterClassConstructionState::CachedCharacter;
            }

            CharacterClassConstructionState::CachedCharacter => {
                if hyphen_is_range && ch == '-' as u32 {
                    self.state = CharacterClassConstructionState::CachedCharacterHyphen;
                } else {
                    delegate.atom_character_class_atom(self.character);
                    self.character = ch;
                }
            }

            CharacterClassConstructionState::CachedCharacterHyphen => {
                if ch < self.character {
                    *error_code = ErrorCode::CharacterClassRangeOutOfOrder;
                    return;
                }
                delegate.atom_character_class_range(self.character, ch);
                self.state = CharacterClassConstructionState::Empty;
            }

            // Se chegamos aqui, temos um intervalo inválido como /[\d-a]/.
            // Veja o comentário em atom_built_in_character_class().
            CharacterClassConstructionState::AfterCharacterClassHyphen => {
                if self.is_unicode {
                    *error_code = ErrorCode::CharacterClassRangeInvalid;
                    return;
                }
                delegate.atom_character_class_atom(ch);
                self.state = CharacterClassConstructionState::Empty;
            }
        }
    }

    /// `atomBuiltInCharacterClass()`: adiciona uma classe embutida, chamado por `parseEscape()`.
    fn atom_built_in_character_class(&mut self, delegate: &mut D, error_code: &mut ErrorCode, class_id: BuiltInCharacterClassID, invert: bool) {
        match self.state {
            CharacterClassConstructionState::CachedCharacter
            | CharacterClassConstructionState::Empty
            | CharacterClassConstructionState::AfterCharacterClass => {
                if self.state == CharacterClassConstructionState::CachedCharacter {
                    // Descarrega o caractere em cache, depois cai no caso seguinte.
                    delegate.atom_character_class_atom(self.character);
                }
                delegate.atom_character_class_built_in(class_id, invert);
                self.state = CharacterClassConstructionState::AfterCharacterClass;
            }

            // Se chegamos a um destes dois casos, temos um intervalo inválido parecido com
            // /[a-\d]/ ou /[\d-\d]/. Desde o ES2015 isso deve ser erro de sintaxe num padrão
            // unicode, mas é tratado sem erro numa regex comum para não quebrar a web. Na prática
            // tratamos o hífen como se estivesse (implicitamente) escapado, por exemplo,
            // /[\d-a-z]/ é tratado como /[\d\-a\-z]/.
            // Veja os usos da operação abstrata CharacterRangeOrUnion em
            // https://tc39.es/ecma262/#sec-regular-expression-patterns-semantics
            CharacterClassConstructionState::CachedCharacterHyphen
            | CharacterClassConstructionState::AfterCharacterClassHyphen => {
                if self.state == CharacterClassConstructionState::CachedCharacterHyphen {
                    delegate.atom_character_class_atom(self.character);
                    delegate.atom_character_class_atom('-' as u32);
                }
                if self.is_unicode {
                    *error_code = ErrorCode::CharacterClassRangeInvalid;
                    return;
                }
                delegate.atom_character_class_built_in(class_id, invert);
                self.state = CharacterClassConstructionState::Empty;
            }
        }
    }

    // parseEscape() nunca deve chamar estes métodos de delegate com inCharacterClass ligado.
    fn assertion_word_boundary(&mut self, _delegate: &mut D, _error_code: &mut ErrorCode, _invert: bool) {
        unreachable!("RELEASE_ASSERT_NOT_REACHED() em YarrParser.h:366 (assertionWordBoundary)");
    }

    fn atom_back_reference(&mut self, _delegate: &mut D, _error_code: &mut ErrorCode, _subpattern_id: u32) {
        unreachable!("RELEASE_ASSERT_NOT_REACHED() em YarrParser.h:367 (atomBackReference)");
    }

    fn atom_named_back_reference(&mut self, _delegate: &mut D, _error_code: &mut ErrorCode, _subpattern_name: &String) {
        unreachable!("RELEASE_ASSERT_NOT_REACHED() em YarrParser.h:368 (atomNamedBackReference)");
    }

    fn atom_named_forward_reference(&mut self, _delegate: &mut D, _error_code: &mut ErrorCode, _subpattern_name: &String) {
        unreachable!("RELEASE_ASSERT_NOT_REACHED() em YarrParser.h:369 (atomNamedForwardReference)");
    }
}

/// `ClassSetParserDelegate::NestingState`.
#[derive(Clone, Copy, Debug)]
struct NestingState {
    set_op: CharacterClassSetOp,
    may_contain_strings: bool,
    inverted: bool,
}

/// `enum class ClassSetConstructionState` de `ClassSetParserDelegate`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClassSetConstructionState {
    Empty,
    CachedCharacter,
    CachedCharacterHyphen,
    AfterCharacterClass,
    AfterCharacterClassHyphen,
    AfterSetRange,
    AfterSetOperand,
    AfterSetOperator,
}

/// `ClassSetParserDelegate`:
///
/// Usada no parsing de class sets. Trata a detecção de operações de conjunto e de intervalos de
/// caracteres. Implementa o bastante da interface de delegate para ser passada a `parseEscape()`
/// como `EscapeDelegate`.
pub struct ClassSetParserDelegate {
    state: ClassSetConstructionState,
    set_op: CharacterClassSetOp,
    may_contain_strings: bool,
    inverted: bool,
    character: u32,
    nested_parse_state: Vec<NestingState>,
}

impl ClassSetParserDelegate {
    pub fn new() -> Self {
        ClassSetParserDelegate {
            state: ClassSetConstructionState::Empty,
            set_op: CharacterClassSetOp::Default,
            may_contain_strings: false,
            inverted: false,
            character: 0,
            nested_parse_state: Vec::new(),
        }
    }

    /// `begin()`: chamado no início da construção.
    pub fn begin<D: Delegate>(&mut self, delegate: &mut D, invert: bool) {
        self.inverted = invert;
        delegate.atom_character_class_begin(invert);
    }

    pub fn nested_class_begin<D: Delegate>(&mut self, delegate: &mut D, error_code: &mut ErrorCode, invert: bool) {
        self.flush_cached_character_if_needed(delegate, error_code);

        delegate.atom_character_class_push_nested(invert);
        self.nested_parse_state.push(NestingState {
            set_op: self.set_op,
            may_contain_strings: self.may_contain_strings,
            inverted: self.inverted,
        });
        self.set_op = CharacterClassSetOp::Default;
        self.may_contain_strings = false;
        self.inverted = invert;
    }

    pub fn nested_class_end<D: Delegate>(&mut self, delegate: &mut D, error_code: &mut ErrorCode) -> bool {
        self.flush_cached_character_if_needed(delegate, error_code);

        if self.inverted && self.may_contain_strings {
            *error_code = ErrorCode::NegatedClassSetMayContainStrings;
        }

        let Some(last_state) = self.nested_parse_state.pop() else {
            self.end(delegate, error_code);
            return true;
        };

        let rhs_may_contain_strings = self.may_contain_strings;

        self.set_op = last_state.set_op;
        self.inverted = last_state.inverted;
        self.may_contain_strings = last_state.may_contain_strings;

        delegate.atom_character_class_pop_nested(self.inverted);
        self.state = ClassSetConstructionState::AfterSetOperand;
        self.compute_may_contain_strings(rhs_may_contain_strings);
        false
    }

    pub fn set_union_op<D: Delegate>(&mut self, delegate: &mut D, error_code: &mut ErrorCode) {
        if self.set_op != CharacterClassSetOp::Default && self.set_op != CharacterClassSetOp::Union {
            *error_code = ErrorCode::InvalidClassSetOperation;
            return;
        }

        self.flush_cached_character_if_needed(delegate, error_code);
        self.set_op = CharacterClassSetOp::Union;
        delegate.atom_character_class_set_op(self.set_op);
    }

    pub fn switch_from_default_op_to_union_op_if_needed<D: Delegate>(&mut self, delegate: &mut D) {
        if self.set_op == CharacterClassSetOp::Default {
            self.set_op = CharacterClassSetOp::Union;
            delegate.atom_character_class_set_op(self.set_op);
        }
    }

    pub fn set_subtract_op<D: Delegate>(&mut self, delegate: &mut D, error_code: &mut ErrorCode) {
        if self.state == ClassSetConstructionState::Empty
            || (self.set_op != CharacterClassSetOp::Default && self.set_op != CharacterClassSetOp::Subtraction)
        {
            *error_code = ErrorCode::InvalidClassSetOperation;
            return;
        }

        self.flush_cached_character_if_needed(delegate, error_code);
        self.set_op = CharacterClassSetOp::Subtraction;
        delegate.atom_character_class_set_op(self.set_op);
        self.state = ClassSetConstructionState::AfterSetOperator;
    }

    pub fn set_intersection_op<D: Delegate>(&mut self, delegate: &mut D, error_code: &mut ErrorCode) {
        if self.state == ClassSetConstructionState::Empty
            || (self.set_op != CharacterClassSetOp::Default && self.set_op != CharacterClassSetOp::Intersection)
        {
            *error_code = ErrorCode::InvalidClassSetOperation;
            return;
        }

        self.flush_cached_character_if_needed(delegate, error_code);
        self.set_op = CharacterClassSetOp::Intersection;
        delegate.atom_character_class_set_op(self.set_op);
        self.state = ClassSetConstructionState::AfterSetOperator;
    }

    pub fn compute_may_contain_strings(&mut self, rhs_may_contain_strings: bool) {
        match self.set_op {
            CharacterClassSetOp::Default | CharacterClassSetOp::Union => {
                self.may_contain_strings |= rhs_may_contain_strings;
            }

            CharacterClassSetOp::Intersection => {
                self.may_contain_strings = self.may_contain_strings && rhs_may_contain_strings;
            }

            CharacterClassSetOp::Subtraction => {
                // O resultado é o valor do lado esquerdo.
            }
        }
    }

    pub fn flush_cached_character_if_needed<D: Delegate>(&mut self, delegate: &mut D, error_code: &mut ErrorCode) {
        if self.state == ClassSetConstructionState::CachedCharacter {
            delegate.atom_character_class_atom(self.character);
            self.state = ClassSetConstructionState::Empty;
        } else if self.state == ClassSetConstructionState::CachedCharacterHyphen
            || self.state == ClassSetConstructionState::AfterCharacterClassHyphen
        {
            *error_code = ErrorCode::InvalidClassSetCharacter;
        }
    }

    pub fn after_set_operand<D: Delegate>(&mut self, delegate: &mut D, error_code: &mut ErrorCode) {
        self.flush_cached_character_if_needed(delegate, error_code);
        self.state = ClassSetConstructionState::AfterSetOperand;
    }

    pub fn can_take_set_operand<D: Delegate>(&mut self, delegate: &mut D, error_code: &mut ErrorCode) -> bool {
        let union_op_active = self.set_op == CharacterClassSetOp::Default || self.set_op == CharacterClassSetOp::Union;

        match self.state {
            ClassSetConstructionState::Empty | ClassSetConstructionState::AfterSetOperator => true,

            ClassSetConstructionState::CachedCharacter => {
                if !union_op_active {
                    return false;
                }

                self.flush_cached_character_if_needed(delegate, error_code);
                true
            }

            ClassSetConstructionState::CachedCharacterHyphen
            | ClassSetConstructionState::AfterCharacterClassHyphen
            | ClassSetConstructionState::AfterCharacterClass
            | ClassSetConstructionState::AfterSetRange
            | ClassSetConstructionState::AfterSetOperand => union_op_active,
        }
    }

    /// `end()`: chamado no fim da construção.
    pub fn end<D: Delegate>(&mut self, delegate: &mut D, error_code: &mut ErrorCode) {
        if self.state == ClassSetConstructionState::CachedCharacter {
            delegate.atom_character_class_atom(self.character);
        } else if self.state == ClassSetConstructionState::CachedCharacterHyphen
            || self.state == ClassSetConstructionState::AfterCharacterClassHyphen
            || self.state == ClassSetConstructionState::AfterSetOperator
        {
            *error_code = ErrorCode::InvalidClassSetCharacter;
        }

        if self.is_inverted() && self.may_contain_strings {
            *error_code = ErrorCode::NegatedClassSetMayContainStrings;
        }

        delegate.atom_character_class_end();
    }

    pub fn is_inverted(&self) -> bool {
        self.inverted
    }

    /// Corpo do lambda `processBuiltInCharacterClass` de `atomBuiltInCharacterClass()`.
    fn process_built_in_character_class<D: Delegate>(&mut self, delegate: &mut D, class_id: BuiltInCharacterClassID, invert: bool) {
        self.compute_may_contain_strings(character_class_may_contain_strings(class_id));

        delegate.atom_character_class_built_in(class_id, invert);
        self.state = ClassSetConstructionState::AfterCharacterClass;
    }
}

impl Default for ClassSetParserDelegate {
    fn default() -> Self {
        ClassSetParserDelegate::new()
    }
}

impl<D: Delegate> EscapeSink<D> for ClassSetParserDelegate {
    /// `atomPatternCharacter()`:
    ///
    /// Chamado por `parseClassSet()` (caractere sem escape na classe) ou por `parseEscape()`. No
    /// primeiro caso `hyphen_is_range` é `true`, e o hífen pode indicar um intervalo (`/[a-z]/` é
    /// diferente de `/[a\-z]/`).
    fn atom_pattern_character(&mut self, delegate: &mut D, error_code: &mut ErrorCode, ch: u32, hyphen_is_range: bool) {
        let union_op_active = self.set_op == CharacterClassSetOp::Default || self.set_op == CharacterClassSetOp::Union;

        match self.state {
            ClassSetConstructionState::AfterCharacterClass
            | ClassSetConstructionState::AfterSetRange
            | ClassSetConstructionState::Empty
            | ClassSetConstructionState::AfterSetOperator => {
                if self.state == ClassSetConstructionState::AfterCharacterClass {
                    // Depois de uma classe embutida é preciso vigiar um hífen. Procuramos
                    // intervalos inválidos como /[\d-x]/ ou /[\d-\d]/. Se vemos um hífen após uma
                    // classe de caracteres, ao contrário do usual o reportamos ao delegate na hora e
                    // entramos num estado envenenado. Num padrão unicode, qualquer chamada seguinte
                    // para adicionar outro caractere ou classe resulta em erro de sintaxe. Um hífen
                    // após uma classe é válido em si, mas só no fim da regex.
                    if hyphen_is_range && union_op_active && ch == '-' as u32 {
                        delegate.atom_character_class_atom('-' as u32);
                        self.state = ClassSetConstructionState::AfterCharacterClassHyphen;
                        return;
                    }
                    // Senão cai no caso AfterSetRange, depois no Empty.
                }

                if self.state == ClassSetConstructionState::AfterCharacterClass
                    || self.state == ClassSetConstructionState::AfterSetRange
                {
                    self.switch_from_default_op_to_union_op_if_needed(delegate);

                    // Continua processando o caractere corrente.
                }

                // (hyphen_is_range é falso exatamente para um caractere escapado.)
                if hyphen_is_range && ch == '-' as u32 {
                    *error_code = ErrorCode::InvalidClassSetCharacter;
                    return;
                }

                // processCharacter()
                self.character = ch;
                self.state = ClassSetConstructionState::CachedCharacter;
            }

            ClassSetConstructionState::CachedCharacter => {
                if !union_op_active {
                    *error_code = ErrorCode::InvalidClassSetOperation;
                    return;
                }

                if ch == '-' as u32 {
                    self.state = ClassSetConstructionState::CachedCharacterHyphen;
                } else {
                    delegate.atom_character_class_atom(self.character);
                    self.switch_from_default_op_to_union_op_if_needed(delegate);
                    // processCharacter()
                    self.character = ch;
                    self.state = ClassSetConstructionState::CachedCharacter;
                }
            }

            ClassSetConstructionState::CachedCharacterHyphen => {
                if ch < self.character {
                    *error_code = ErrorCode::CharacterClassRangeOutOfOrder;
                    return;
                }

                delegate.atom_character_class_range(self.character, ch);
                self.switch_from_default_op_to_union_op_if_needed(delegate);
                self.state = ClassSetConstructionState::AfterSetRange;
            }

            // Se chegamos aqui, temos um intervalo inválido como /[\d-a]/.
            // Veja o comentário em atom_built_in_character_class().
            ClassSetConstructionState::AfterCharacterClassHyphen => {
                *error_code = ErrorCode::CharacterClassRangeInvalid;
            }

            ClassSetConstructionState::AfterSetOperand => {
                // Um caractere logo após uma classe aninhada ou \q{...}: uma união com ela. (Não há
                // caractere em cache a descarregar neste estado: fazer isso adicionava um caractere
                // obsoleto, U+0000 após [\q{ab}x], e um hífen escapado é membro comum aqui.)
                if !union_op_active {
                    *error_code = ErrorCode::InvalidClassSetOperation;
                    return;
                }
                if hyphen_is_range && ch == '-' as u32 {
                    // um '-' sem escape (um ClassSetSyntaxCharacter)
                    *error_code = ErrorCode::InvalidClassSetOperation;
                    return;
                }
                self.switch_from_default_op_to_union_op_if_needed(delegate);
                // processCharacter()
                self.character = ch;
                self.state = ClassSetConstructionState::CachedCharacter;
            }
        }
    }

    /// `atomBuiltInCharacterClass()`: adiciona uma classe embutida, chamado por `parseEscape()`.
    fn atom_built_in_character_class(&mut self, delegate: &mut D, error_code: &mut ErrorCode, class_id: BuiltInCharacterClassID, invert: bool) {
        let union_op_active = self.set_op == CharacterClassSetOp::Default || self.set_op == CharacterClassSetOp::Union;

        match self.state {
            ClassSetConstructionState::CachedCharacter
            | ClassSetConstructionState::AfterSetRange
            | ClassSetConstructionState::Empty
            | ClassSetConstructionState::AfterCharacterClass
            | ClassSetConstructionState::AfterSetOperator => {
                if self.state == ClassSetConstructionState::CachedCharacter {
                    if !union_op_active {
                        *error_code = ErrorCode::InvalidClassSetOperation;
                        return;
                    }

                    // Descarrega o caractere em cache, depois cai no caso seguinte.
                    delegate.atom_character_class_atom(self.character);

                    // Sim, queremos mesmo cair no caso AfterSetRange para trocar a operação Default
                    // por Union e então tratar a classe embutida caindo de novo.
                }

                if self.state == ClassSetConstructionState::CachedCharacter
                    || self.state == ClassSetConstructionState::AfterSetRange
                {
                    self.switch_from_default_op_to_union_op_if_needed(delegate);

                    // Continua processando o caractere corrente.
                }

                self.process_built_in_character_class(delegate, class_id, invert);
            }

            // Se chegamos a um destes dois casos, temos um intervalo inválido parecido com
            // /[a-\d]/ ou /[\d-\d]/. Desde o ES2015 isso deve ser erro de sintaxe num padrão
            // unicode, mas é tratado sem erro numa regex comum para não quebrar a web. Na prática
            // tratamos o hífen como se estivesse (implicitamente) escapado, por exemplo,
            // /[\d-a-z]/ é tratado como /[\d\-a\-z]/.
            // Veja os usos da operação abstrata CharacterRangeOrUnion em
            // https://tc39.es/ecma262/#sec-regular-expression-patterns-semantics
            ClassSetConstructionState::CachedCharacterHyphen
            | ClassSetConstructionState::AfterCharacterClassHyphen => {
                if self.state == ClassSetConstructionState::CachedCharacterHyphen {
                    delegate.atom_character_class_atom(self.character);
                    delegate.atom_character_class_atom('-' as u32);
                }
                *error_code = ErrorCode::CharacterClassRangeInvalid;
            }

            ClassSetConstructionState::AfterSetOperand => {
                if !union_op_active {
                    *error_code = ErrorCode::InvalidClassSetOperation;
                }

                self.process_built_in_character_class(delegate, class_id, invert);
            }
        }
    }

    // parseEscape() nunca deve chamar estes métodos de delegate com inCharacterClass ligado.
    fn assertion_word_boundary(&mut self, _delegate: &mut D, _error_code: &mut ErrorCode, _invert: bool) {
        unreachable!("RELEASE_ASSERT_NOT_REACHED() em YarrParser.h:771 (assertionWordBoundary)");
    }

    fn atom_back_reference(&mut self, _delegate: &mut D, _error_code: &mut ErrorCode, _subpattern_id: u32) {
        unreachable!("RELEASE_ASSERT_NOT_REACHED() em YarrParser.h:772 (atomBackReference)");
    }

    fn atom_named_back_reference(&mut self, _delegate: &mut D, _error_code: &mut ErrorCode, _subpattern_name: &String) {
        unreachable!("RELEASE_ASSERT_NOT_REACHED() em YarrParser.h:773 (atomNamedBackReference)");
    }

    fn atom_named_forward_reference(&mut self, _delegate: &mut D, _error_code: &mut ErrorCode, _subpattern_name: &String) {
        unreachable!("RELEASE_ASSERT_NOT_REACHED() em YarrParser.h:774 (atomNamedForwardReference)");
    }
}

/// `ClassStringDisjunctionParserDelegate`:
///
/// Usada no parsing de disjunções de strings de classe, por exemplo `\q{...}`. Monta strings a
/// partir das alternativas e as passa ao delegate de classe de caracteres.
pub struct ClassStringDisjunctionParserDelegate {
    may_contain_strings: bool,
    string_in_progress: Vec<u32>,
    strings: Vec<Vec<u32>>,
}

impl ClassStringDisjunctionParserDelegate {
    pub fn new() -> Self {
        ClassStringDisjunctionParserDelegate {
            may_contain_strings: false,
            string_in_progress: Vec::new(),
            strings: Vec::new(),
        }
    }

    pub fn new_alternative(&mut self) {
        self.strings.push(std::mem::take(&mut self.string_in_progress));
    }

    /// `end()`: chamado no fim da construção.
    pub fn end<D: Delegate>(&mut self, delegate: &mut D) {
        self.new_alternative();
        delegate.atom_class_string_disjunction(&mut self.strings);
    }

    pub fn may_contain_strings(&self) -> bool {
        self.may_contain_strings
    }
}

impl Default for ClassStringDisjunctionParserDelegate {
    fn default() -> Self {
        ClassStringDisjunctionParserDelegate::new()
    }
}

impl<D: Delegate> EscapeSink<D> for ClassStringDisjunctionParserDelegate {
    fn atom_pattern_character(&mut self, _delegate: &mut D, _error_code: &mut ErrorCode, ch: u32, _hyphen_is_range: bool) {
        self.string_in_progress.push(ch);
        if self.string_in_progress.len() > 1 {
            self.may_contain_strings = true;
        }
    }

    // parseEscape() nunca deve chamar estes métodos de delegate ao tratar uma disjunção de strings
    // de classe.
    fn assertion_word_boundary(&mut self, _delegate: &mut D, _error_code: &mut ErrorCode, _invert: bool) {
        unreachable!("RELEASE_ASSERT_NOT_REACHED() em YarrParser.h:840 (assertionWordBoundary)");
    }

    fn atom_back_reference(&mut self, _delegate: &mut D, _error_code: &mut ErrorCode, _subpattern_id: u32) {
        unreachable!("RELEASE_ASSERT_NOT_REACHED() em YarrParser.h:841 (atomBackReference)");
    }

    fn atom_named_back_reference(&mut self, _delegate: &mut D, _error_code: &mut ErrorCode, _subpattern_name: &String) {
        unreachable!("RELEASE_ASSERT_NOT_REACHED() em YarrParser.h:842 (atomNamedBackReference)");
    }

    fn atom_named_forward_reference(&mut self, _delegate: &mut D, _error_code: &mut ErrorCode, _subpattern_name: &String) {
        unreachable!("RELEASE_ASSERT_NOT_REACHED() em YarrParser.h:843 (atomNamedForwardReference)");
    }

    fn atom_built_in_character_class(&mut self, _delegate: &mut D, _error_code: &mut ErrorCode, _class_id: BuiltInCharacterClassID, _invert: bool) {
        unreachable!("RELEASE_ASSERT_NOT_REACHED() em YarrParser.h:844 (atomBuiltInCharacterClass)");
    }
}

/// `class Parser`. Não deve ser usada diretamente: só via `Yarr::parse()`.
pub struct Parser<'a, D: Delegate, C: CharType> {
    delegate: &'a mut D,
    error_code: ErrorCode,
    data: &'a [C],
    size: u32,
    index: u32,
    compile_mode: CompileMode,
    back_reference_limit: u32,
    num_subpatterns: u32,
    max_seen_back_reference: u32,
    num_captures: u32,
    is_named_forward_reference_allowed: bool,
    k_identity_escape_seen: bool,
    parentheses_stack: Vec<ParenthesesType>,
    named_capture_groups: NamedCaptureGroups,
    forward_reference_names: HashSet<String>,
}

impl<'a, D: Delegate, C: CharType> Parser<'a, D, C> {
    pub fn new(
        delegate: &'a mut D,
        pattern: &'a [C],
        compile_mode: CompileMode,
        back_reference_limit: u32,
        is_named_forward_reference_allowed: bool,
    ) -> Self {
        Parser {
            delegate,
            error_code: ErrorCode::NoError,
            data: pattern,
            size: pattern.len() as u32,
            index: 0,
            compile_mode,
            back_reference_limit,
            num_subpatterns: 0,
            max_seen_back_reference: 0,
            num_captures: 0,
            is_named_forward_reference_allowed,
            k_identity_escape_seen: false,
            parentheses_stack: Vec::new(),
            named_capture_groups: NamedCaptureGroups::new(),
            forward_reference_names: HashSet::new(),
        }
    }

    /// `parse()`: chama `parseTokens()` para percorrer a entrada e devolve o código de erro do
    /// resultado.
    pub fn parse(&mut self) -> ErrorCode {
        if self.size > MAX_PATTERN_SIZE {
            return ErrorCode::PatternTooLarge;
        }

        self.parse_tokens();

        if !has_error(self.error_code) {
            self.handle_illegal_references();
        }

        self.error_code
    }

    /// `isIdentityEscapeAnError<parseEscapeMode>()`.
    ///
    /// O tratamento de IdentityEscape depende de qual flag unicode, se alguma, está ativa. Em padrões
    /// Unicode e UnicodeSets, IdentityEscape só inclui SyntaxCharacters ou '/'. Em padrões UnicodeSets,
    /// ao tratar expressões ClassSet e ClassStringDisjunctions, inclui SyntaxCharacters, '/' e
    /// ClassSetReservedPunctuation, que é qualquer um de &-!#%,:;<=>@`~. Em padrões não unicode,
    /// quase qualquer caractere pode ser escapado.
    fn is_identity_escape_an_error(&mut self, parse_escape_mode: ParseEscapeMode, ch: u32) -> bool {
        let allowed = if parse_escape_mode == ParseEscapeMode::ClassSet
            || parse_escape_mode == ParseEscapeMode::ClassStringDisjunction
        {
            "^$\\.*+?()[]{}|/&-!#%,:;<=>@`~"
        } else {
            "^$\\.*+?()[]{}|/"
        };

        // `strchr(allowed, ch)` também acha o terminador nulo para `ch == 0`; o `|| !ch` cobre o
        // mesmo caso, então o resultado é o mesmo.
        if self.is_either_unicode_compilation()
            && ((is_ascii(ch) && !allowed.chars().any(|allowed_char| allowed_char as u32 == ch)) || ch == 0)
        {
            self.error_code = ErrorCode::InvalidIdentityEscape;
            return true;
        }

        false
    }

    /// `parseEscape()`:
    ///
    /// Auxiliar de `parseTokens()`, `parseAtomEscape()`, `parseCharacterClassEscape()`,
    /// `parseClassSetEscape()` e `parseClassStringDisjunctionEscape()`.
    ///
    /// Ao contrário dos outros métodos do parser, esta função não reporta tokens direto ao delegate
    /// membro (`self.delegate`): os tokens vão para o `sink` recebido. No caso de escapes de átomo,
    /// `parseTokens()` chama `parse_escape` com `AtomEscapeSink`, que repassa o escape ao delegate do
    /// parser. Mas também pode ser usada por `parseCharacterClass()`, `parseClassSet()` ou
    /// `parseClassStringDisjunctionEscape()`, e então o sink é um `CharacterClassParserDelegate`,
    /// `ClassSetParserDelegate` ou `ClassStringDisjunctionParserDelegate`, respectivamente.
    pub fn parse_escape<E: EscapeSink<D>>(&mut self, parse_escape_mode: ParseEscapeMode, sink: &mut E) -> TokenType {
        self.consume();

        if self.at_end_of_pattern() {
            self.error_code = ErrorCode::EscapeUnterminated;
            return TokenType::NotAtom;
        }

        let peeked = self.peek();

        'escape: {
            match char::from_u32(peeked) {
                // Assertions
                Some('b') => {
                    self.consume();
                    if parse_escape_mode != ParseEscapeMode::Normal {
                        sink.atom_pattern_character(self.delegate, &mut self.error_code, 0x08, false);
                    } else {
                        sink.assertion_word_boundary(self.delegate, &mut self.error_code, false);
                        return TokenType::NotAtom;
                    }
                }
                Some('B') => {
                    self.consume();
                    if parse_escape_mode != ParseEscapeMode::Normal {
                        if self.is_identity_escape_an_error(parse_escape_mode, 'B' as u32) {
                            break 'escape;
                        }

                        sink.atom_pattern_character(self.delegate, &mut self.error_code, 'B' as u32, false);
                    } else {
                        sink.assertion_word_boundary(self.delegate, &mut self.error_code, true);
                        return TokenType::NotAtom;
                    }
                }

                // CharacterClassEscape
                Some('d') => {
                    self.consume();
                    if parse_escape_mode == ParseEscapeMode::ClassStringDisjunction {
                        sink.atom_pattern_character(self.delegate, &mut self.error_code, 'd' as u32, false);
                        break 'escape;
                    }
                    sink.atom_built_in_character_class(self.delegate, &mut self.error_code, BuiltInCharacterClassID::DigitClassID, false);
                }
                Some('s') => {
                    self.consume();
                    if parse_escape_mode == ParseEscapeMode::ClassStringDisjunction {
                        sink.atom_pattern_character(self.delegate, &mut self.error_code, 's' as u32, false);
                        break 'escape;
                    }
                    sink.atom_built_in_character_class(self.delegate, &mut self.error_code, BuiltInCharacterClassID::SpaceClassID, false);
                }
                Some('w') => {
                    self.consume();
                    if parse_escape_mode == ParseEscapeMode::ClassStringDisjunction {
                        sink.atom_pattern_character(self.delegate, &mut self.error_code, 'w' as u32, false);
                        break 'escape;
                    }
                    sink.atom_built_in_character_class(self.delegate, &mut self.error_code, BuiltInCharacterClassID::WordClassID, false);
                }
                Some('D') => {
                    self.consume();
                    if parse_escape_mode == ParseEscapeMode::ClassStringDisjunction {
                        sink.atom_pattern_character(self.delegate, &mut self.error_code, 'D' as u32, false);
                        break 'escape;
                    }
                    sink.atom_built_in_character_class(self.delegate, &mut self.error_code, BuiltInCharacterClassID::DigitClassID, true);
                }
                Some('S') => {
                    self.consume();
                    if parse_escape_mode == ParseEscapeMode::ClassStringDisjunction {
                        sink.atom_pattern_character(self.delegate, &mut self.error_code, 'S' as u32, false);
                        break 'escape;
                    }
                    sink.atom_built_in_character_class(self.delegate, &mut self.error_code, BuiltInCharacterClassID::SpaceClassID, true);
                }
                Some('W') => {
                    self.consume();
                    if parse_escape_mode == ParseEscapeMode::ClassStringDisjunction {
                        sink.atom_pattern_character(self.delegate, &mut self.error_code, 'W' as u32, false);
                        break 'escape;
                    }
                    sink.atom_built_in_character_class(self.delegate, &mut self.error_code, BuiltInCharacterClassID::WordClassID, true);
                }

                Some('0') => {
                    self.consume();

                    if !self.peek_is_digit() {
                        sink.atom_pattern_character(self.delegate, &mut self.error_code, 0, false);
                        break 'escape;
                    }

                    if self.is_either_unicode_compilation() {
                        self.error_code = ErrorCode::InvalidOctalEscape;
                        break 'escape;
                    }

                    let octal = self.consume_octal(2);
                    sink.atom_pattern_character(self.delegate, &mut self.error_code, octal, false);
                }

                // DecimalEscape
                Some('1'..='9') => {
                    // Em padrões não Unicode, backreferences inválidas são tratadas como escapes
                    // octais ou decimais. Primeiro tenta tratar como backreference.
                    if parse_escape_mode == ParseEscapeMode::Normal {
                        let state = self.save_state();

                        let back_reference = self.consume_number();
                        if back_reference <= self.back_reference_limit {
                            self.max_seen_back_reference = self.max_seen_back_reference.max(back_reference);
                            sink.atom_back_reference(self.delegate, &mut self.error_code, back_reference);
                            break 'escape;
                        }

                        self.restore_state(state);
                        if self.is_either_unicode_compilation() {
                            self.error_code = ErrorCode::InvalidBackreference;
                            break 'escape;
                        }
                    }

                    if self.is_either_unicode_compilation() {
                        self.error_code = ErrorCode::InvalidOctalEscape;
                        break 'escape;
                    }

                    let character = if self.peek() < '8' as u32 {
                        self.consume_octal(3)
                    } else {
                        self.consume()
                    };
                    sink.atom_pattern_character(self.delegate, &mut self.error_code, character, false);
                }

                // ControlEscape
                Some('f') => {
                    self.consume();
                    sink.atom_pattern_character(self.delegate, &mut self.error_code, 0x0C, false);
                }
                Some('n') => {
                    self.consume();
                    sink.atom_pattern_character(self.delegate, &mut self.error_code, '\n' as u32, false);
                }
                Some('r') => {
                    self.consume();
                    sink.atom_pattern_character(self.delegate, &mut self.error_code, '\r' as u32, false);
                }
                Some('t') => {
                    self.consume();
                    sink.atom_pattern_character(self.delegate, &mut self.error_code, '\t' as u32, false);
                }
                Some('v') => {
                    self.consume();
                    sink.atom_pattern_character(self.delegate, &mut self.error_code, 0x0B, false);
                }

                // ControlLetter
                Some('c') => {
                    let state = self.save_state();
                    self.consume();
                    if !self.at_end_of_pattern() {
                        let control = self.consume();

                        if is_ascii_alpha(control) {
                            sink.atom_pattern_character(self.delegate, &mut self.error_code, control & 0x1f, false);
                            break 'escape;
                        }

                        if self.is_either_unicode_compilation() {
                            self.error_code = ErrorCode::InvalidControlLetterEscape;
                            break 'escape;
                        }

                        // https://tc39.es/ecma262/#prod-annexB-ClassControlLetter
                        if parse_escape_mode != ParseEscapeMode::Normal
                            && (is_ascii_digit(control) || control == '_' as u32)
                        {
                            sink.atom_pattern_character(self.delegate, &mut self.error_code, control & 0x1f, false);
                            break 'escape;
                        }
                    }

                    if self.is_either_unicode_compilation() {
                        self.error_code = ErrorCode::InvalidIdentityEscape;
                        break 'escape;
                    }

                    self.restore_state(state);
                    sink.atom_pattern_character(self.delegate, &mut self.error_code, '\\' as u32, false);
                }

                // HexEscape
                Some('x') => {
                    self.consume();
                    let x = self.try_consume_hex(2);
                    if x == ERROR_CODE_POINT {
                        if self.is_identity_escape_an_error(parse_escape_mode, 'x' as u32) {
                            break 'escape;
                        }

                        sink.atom_pattern_character(self.delegate, &mut self.error_code, 'x' as u32, false);
                    } else {
                        sink.atom_pattern_character(self.delegate, &mut self.error_code, x, false);
                    }
                }

                // Named backreference
                Some('k') => {
                    self.consume();
                    let state = self.save_state();
                    if parse_escape_mode == ParseEscapeMode::Normal && self.try_consume('<' as u16) {
                        let group_name = self.try_consume_group_name();
                        if has_error(self.error_code) {
                            break 'escape;
                        }

                        if let Some(group_name) = group_name {
                            if self.named_capture_groups.contains(&group_name) {
                                sink.atom_named_back_reference(self.delegate, &mut self.error_code, &group_name);
                                break 'escape;
                            }

                            if self.is_named_forward_reference_allowed {
                                self.forward_reference_names.insert(group_name.clone());
                                sink.atom_named_forward_reference(self.delegate, &mut self.error_code, &group_name);
                                break 'escape;
                            }
                        }
                    }

                    self.restore_state(state);
                    if !self.is_identity_escape_an_error(parse_escape_mode, 'k' as u32) {
                        sink.atom_pattern_character(self.delegate, &mut self.error_code, 'k' as u32, false);
                        self.k_identity_escape_seen = true;
                    }
                }

                // Unicode property escapes
                Some(escape_char @ ('p' | 'P')) => {
                    self.consume();

                    if self.is_legacy_compilation() || parse_escape_mode == ParseEscapeMode::ClassStringDisjunction {
                        if self.is_identity_escape_an_error(parse_escape_mode, escape_char as u32) {
                            break 'escape;
                        }
                        sink.atom_pattern_character(self.delegate, &mut self.error_code, escape_char as u32, false);
                        break 'escape;
                    }

                    if !self.at_end_of_pattern() && self.peek() == '{' as u32 {
                        self.consume();
                        let opt_class_id = self.try_consume_unicode_property_expression();
                        let Some(class_id) = opt_class_id else {
                            // try_consume_unicode_property_expression() define error_code para uma
                            // expressão de propriedade malformada.
                            break 'escape;
                        };

                        if escape_char == 'P' && character_class_may_contain_strings(class_id) {
                            self.error_code = ErrorCode::NegatedClassSetMayContainStrings;
                            break 'escape;
                        }

                        sink.atom_built_in_character_class(self.delegate, &mut self.error_code, class_id, escape_char == 'P');
                    } else {
                        self.error_code = ErrorCode::InvalidUnicodePropertyExpression;
                    }
                }

                // Class String Disjunction
                Some('q') => {
                    let escape_char = self.consume();

                    if parse_escape_mode == ParseEscapeMode::ClassSet {
                        if !self.at_end_of_pattern() && self.peek() == '{' as u32 {
                            let mut disjunction_may_contain_strings = false;
                            self.parse_class_string_disjunction(&mut disjunction_may_contain_strings);

                            return if disjunction_may_contain_strings {
                                TokenType::SetDisjunctionMayContainStrings
                            } else {
                                TokenType::SetDisjunction
                            };
                        }

                        self.error_code = ErrorCode::InvalidUnicodePropertyExpression;
                    }

                    if self.is_identity_escape_an_error(parse_escape_mode, escape_char) {
                        break 'escape;
                    }

                    sink.atom_pattern_character(self.delegate, &mut self.error_code, escape_char, false);
                }

                // UnicodeEscape
                Some('u') => {
                    let code_point = self.try_consume_unicode_escape(UnicodeParseContext::PatternCodePoint);
                    if has_error(self.error_code) {
                        break 'escape;
                    }

                    let character = if code_point == ERROR_CODE_POINT { 'u' as u32 } else { code_point };
                    sink.atom_pattern_character(self.delegate, &mut self.error_code, character, false);
                }

                // IdentityEscape
                _ => {
                    let ch = peeked;

                    if ch == '-' as u32
                        && self.is_either_unicode_compilation()
                        && parse_escape_mode != ParseEscapeMode::Normal
                    {
                        // \- é permitido em ClassEscape com a flag unicode.
                        let character = self.consume();
                        sink.atom_pattern_character(self.delegate, &mut self.error_code, character, false);
                        break 'escape;
                    }

                    if self.is_identity_escape_an_error(parse_escape_mode, ch) {
                        break 'escape;
                    }

                    let character = self.consume();
                    sink.atom_pattern_character(self.delegate, &mut self.error_code, character, false);
                }
            }
        }

        TokenType::Atom
    }
}

// Linhas 1223 em diante do YarrParser.h: yarr_parser_part2.rs.
include!("yarr_parser_part2.rs");
