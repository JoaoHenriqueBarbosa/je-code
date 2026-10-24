//! Porte de `yarr/YarrSyntaxChecker.h` e `YarrSyntaxChecker.cpp`.

use crate::wtf::text::atom_string::AtomString;
use crate::wtf::text::wtf_string::String;
use crate::yarr::yarr::{BuiltInCharacterClassID, QUANTIFY_INFINITE};
use crate::yarr::yarr_error_code::ErrorCode;
use crate::yarr::yarr_flags::{parse_flags, FlagSet};
use crate::yarr::yarr_parser::{
    CharacterClassSetOp, CreateDisjunctionPurpose, Delegate,
};
use crate::yarr::yarr_parser::{compile_mode, parse};
use crate::yarr::yarr_pattern::MatchDirection;

/// `class SyntaxChecker`: o delegate que não grava nada, só deixa o parser validar.
pub struct SyntaxChecker;

impl Delegate for SyntaxChecker {
    fn assertion_bol(&mut self) {}
    fn assertion_eol(&mut self) {}
    fn assertion_word_boundary(&mut self, _invert: bool) {}
    fn atom_pattern_character(&mut self, _ch: u32, _hyphen_is_range: bool) {}
    fn atom_built_in_character_class(&mut self, _class_id: BuiltInCharacterClassID, _invert: bool) {}
    fn atom_character_class_begin(&mut self, _invert: bool) {}
    fn atom_character_class_atom(&mut self, _ch: u32) {}
    fn atom_character_class_range(&mut self, _begin: u32, _end: u32) {}
    fn atom_character_class_built_in(&mut self, _class_id: BuiltInCharacterClassID, _invert: bool) {}
    fn atom_class_string_disjunction(&mut self, _disjunction_strings: &mut Vec<Vec<u32>>) {}
    fn atom_character_class_set_op(&mut self, _set_op: CharacterClassSetOp) {}
    fn atom_character_class_push_nested(&mut self, _invert: bool) {}
    fn atom_character_class_pop_nested(&mut self, _invert: bool) {}
    fn atom_character_class_end(&mut self) {}
    fn atom_parentheses_subpattern_begin(&mut self, _capture: bool, _group_name: Option<String>) {}
    fn atom_parenthetical_assertion_begin(&mut self, _invert: bool, _match_direction: MatchDirection) {}
    fn atom_parenthetical_modifier_begin(&mut self, _set: FlagSet, _unset: FlagSet) {}
    fn atom_parentheses_end(&mut self) {}
    fn atom_back_reference(&mut self, _subpattern_id: u32) {}
    fn atom_named_back_reference(&mut self, _subpattern_name: &String) {}
    fn atom_named_forward_reference(&mut self, _subpattern_name: &String) {}
    fn quantify_atom(&mut self, _min: u32, _max: u32, _greedy: bool) {}
    fn disjunction(&mut self, _purpose: CreateDisjunctionPurpose) {}
    fn aborted_due_to_error(&mut self) -> bool {
        false
    }
    fn abort_error_code(&mut self) -> ErrorCode {
        ErrorCode::NoError
    }
    fn reset_for_reparsing(&mut self) {}
}

/// `checkSyntax(StringView pattern, StringView flags)`.
pub fn check_syntax(pattern: &AtomString, flags: &AtomString) -> ErrorCode {
    let mut syntax_checker = SyntaxChecker;

    let parsed_flags = if flags.is_8bit() {
        parse_flags(flags.span8())
    } else {
        parse_flags(flags.span16())
    };
    if parsed_flags.is_none() {
        return ErrorCode::InvalidRegularExpressionFlags;
    }

    parse(
        &mut syntax_checker,
        pattern.string(),
        compile_mode(parsed_flags),
        QUANTIFY_INFINITE,
        true,
    )
}
