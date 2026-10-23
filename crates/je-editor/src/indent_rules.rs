//! Porte de `src/vs/editor/common/languages/supports/indentRules.ts`: o que as regras de indentação
//! (`IndentationRule`) dizem de uma linha.

use crate::js_reg_exp::JsRegExp;
use crate::language_configuration::{IndentationRule, reg_exp_test};

/// `IndentConsts`.
pub mod indent_consts {
    pub const INCREASE_MASK: u32 = 0b0000_0001;
    pub const DECREASE_MASK: u32 = 0b0000_0010;
    pub const INDENT_NEXTLINE_MASK: u32 = 0b0000_0100;
    pub const UNINDENT_MASK: u32 = 0b0000_1000;
}

/// `resetGlobalRegex` seguido do `test`: zera o `lastIndex` de um regex global e o aplica a `text`.
fn matches_reset(regex: &JsRegExp, text: &[u16]) -> bool {
    if regex.flags().global {
        regex.set_last_index(0);
    }
    reg_exp_test(regex, text)
}

/// `IndentRulesSupport`.
#[derive(Debug)]
pub struct IndentRulesSupport {
    indentation_rules: IndentationRule,
}

impl IndentRulesSupport {
    pub fn new(indentation_rules: IndentationRule) -> Self {
        Self { indentation_rules }
    }

    pub fn should_increase(&self, text: &[u16]) -> bool {
        matches_reset(&self.indentation_rules.increase_indent_pattern, text)
    }

    pub fn should_decrease(&self, text: &[u16]) -> bool {
        matches_reset(&self.indentation_rules.decrease_indent_pattern, text)
    }

    pub fn should_indent_next_line(&self, text: &[u16]) -> bool {
        self.indentation_rules
            .indent_next_line_pattern
            .as_ref()
            .is_some_and(|re| matches_reset(re, text))
    }

    /// O texto casa `unIndentedLinePattern`.
    pub fn should_ignore(&self, text: &[u16]) -> bool {
        self.indentation_rules
            .un_indented_line_pattern
            .as_ref()
            .is_some_and(|re| matches_reset(re, text))
    }

    pub fn get_indent_metadata(&self, text: &[u16]) -> u32 {
        let mut ret = 0;
        if self.should_increase(text) {
            ret += indent_consts::INCREASE_MASK;
        }
        if self.should_decrease(text) {
            ret += indent_consts::DECREASE_MASK;
        }
        if self.should_indent_next_line(text) {
            ret += indent_consts::INDENT_NEXTLINE_MASK;
        }
        if self.should_ignore(text) {
            ret += indent_consts::UNINDENT_MASK;
        }
        ret
    }
}
