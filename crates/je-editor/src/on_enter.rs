//! Porte de `src/vs/editor/common/languages/supports/onEnter.ts`: o que fazer ao pressionar Enter
//! (regras `onEnterRules` e a lógica dos colchetes).

use je_base::errors::on_unexpected_error;
use je_base::strings::escape_reg_exp_characters;

use crate::editor_auto_indent_strategy::EditorAutoIndentStrategy;
use je_base::js_regexp::JsRegExp;
use crate::language_configuration::{
    CharacterPair, EnterAction, IndentAction, LanguageConfiguration, OnEnterRule, reg_exp_test,
};
use crate::rich_edit_brackets::is_word_boundary_char;

/// `IOnEnterSupportOptions`.
#[derive(Clone, Debug, Default)]
pub struct OnEnterSupportOptions {
    pub brackets: Option<Vec<CharacterPair>>,
    pub on_enter_rules: Option<Vec<OnEnterRule>>,
}

impl From<&LanguageConfiguration> for OnEnterSupportOptions {
    /// O original passa a `LanguageConfiguration` inteira, que serve de `IOnEnterSupportOptions`.
    fn from(config: &LanguageConfiguration) -> Self {
        Self {
            brackets: config.brackets.clone(),
            on_enter_rules: config.on_enter_rules.clone(),
        }
    }
}

/// `IProcessedBracketPair`.
#[derive(Debug)]
struct ProcessedBracketPair {
    open_reg_exp: JsRegExp,
    close_reg_exp: JsRegExp,
}

/// `OnEnterSupport`.
#[derive(Debug)]
pub struct OnEnterSupport {
    brackets: Vec<ProcessedBracketPair>,
    reg_exp_rules: Vec<OnEnterRule>,
}

impl OnEnterSupport {
    pub fn new(opts: OnEnterSupportOptions) -> Self {
        let brackets = opts.brackets.unwrap_or_else(|| {
            [("(", ")"), ("{", "}"), ("[", "]")]
                .iter()
                .map(|(o, c)| ((*o).to_owned(), (*c).to_owned()))
                .collect()
        });

        let mut processed = Vec::new();
        for (open, close) in &brackets {
            let open_reg_exp = Self::create_open_bracket_reg_exp(open);
            let close_reg_exp = Self::create_close_bracket_reg_exp(close);
            if let (Some(open_reg_exp), Some(close_reg_exp)) = (open_reg_exp, close_reg_exp) {
                processed.push(ProcessedBracketPair {
                    open_reg_exp,
                    close_reg_exp,
                });
            }
        }
        Self {
            brackets: processed,
            reg_exp_rules: opts.on_enter_rules.unwrap_or_default(),
        }
    }

    pub fn on_enter(
        &self,
        auto_indent: EditorAutoIndentStrategy,
        previous_line_text: &[u16],
        before_enter_text: &[u16],
        after_enter_text: &[u16],
    ) -> Option<EnterAction> {
        // (1): `regExpRules`
        if auto_indent >= EditorAutoIndentStrategy::Advanced {
            for rule in &self.reg_exp_rules {
                let reg_result = [
                    (Some(&rule.before_text), before_enter_text),
                    (rule.after_text.as_ref(), after_enter_text),
                    (rule.previous_line_text.as_ref(), previous_line_text),
                ]
                .iter()
                .all(|(reg, text)| match reg {
                    None => true,
                    Some(reg) => {
                        reg.set_last_index(0); // Desfaz o efeito da flag "g".
                        reg_exp_test(reg, text)
                    }
                });

                if reg_result {
                    return Some(rule.action.clone());
                }
            }
        }

        // (2): Indent-outdent especial
        if auto_indent >= EditorAutoIndentStrategy::Brackets
            && !before_enter_text.is_empty()
            && !after_enter_text.is_empty()
        {
            for bracket in &self.brackets {
                if reg_exp_test(&bracket.open_reg_exp, before_enter_text)
                    && reg_exp_test(&bracket.close_reg_exp, after_enter_text)
                {
                    return Some(EnterAction::new(IndentAction::IndentOutdent));
                }
            }
        }

        // (4): Lógica pela abertura do colchete
        if auto_indent >= EditorAutoIndentStrategy::Brackets && !before_enter_text.is_empty() {
            for bracket in &self.brackets {
                if reg_exp_test(&bracket.open_reg_exp, before_enter_text) {
                    return Some(EnterAction::new(IndentAction::Indent));
                }
            }
        }

        None
    }

    fn create_open_bracket_reg_exp(bracket: &str) -> Option<JsRegExp> {
        let bracket: Vec<u16> = bracket.encode_utf16().collect();
        let mut source: Vec<u16> = Vec::new();
        let escaped = escape_reg_exp_characters(&bracket);
        if is_word_boundary_char(escaped.units().first().copied()) {
            source.extend("\\b".encode_utf16());
        }
        source.extend_from_slice(escaped.units());
        source.extend("\\s*$".encode_utf16());
        Self::safe_reg_exp(&source)
    }

    fn create_close_bracket_reg_exp(bracket: &str) -> Option<JsRegExp> {
        let bracket: Vec<u16> = bracket.encode_utf16().collect();
        let escaped = escape_reg_exp_characters(&bracket);
        let mut source: Vec<u16> = "^\\s*".encode_utf16().collect();
        source.extend_from_slice(escaped.units());
        if is_word_boundary_char(escaped.units().last().copied()) {
            source.extend("\\b".encode_utf16());
        }
        Self::safe_reg_exp(&source)
    }

    fn safe_reg_exp(def: &[u16]) -> Option<JsRegExp> {
        match JsRegExp::new(def, "") {
            Ok(reg_exp) => Some(reg_exp),
            Err(error) => {
                on_unexpected_error(&je_base::errors::Error::named(
                    "SyntaxError",
                    error.message(),
                ));
                None
            }
        }
    }
}
