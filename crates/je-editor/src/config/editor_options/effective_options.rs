//! Porte das quatro opções só calculadas `effective*` de `editorOptions.ts` (regiões `effectiveCursorStyle`,
//! `effectiveExperimentalEditContext`, `effectiveAllowVariableFonts` e `effectiveFullwidthCharacterWidth`).
//! Cada uma é um [`ComputedEditorOption`] (`name = "_never_"`, sem esquema).
//!
//! `effective_fullwidth_character_width` lê `env.font_info.is_monospace`. A opção `fullwidthCharacterWidth` que ela lê tem valor `String` (`'font' | 'twoCells'`).

use crate::config::editor_options::cursor_style::TextEditorCursorStyle;
use crate::config::editor_options::option_support::string_enum;
use crate::config::editor_options::{AccessibilitySupport, ComputedEditorOption, EditorOption, InputMode, Opt};

/// Chave tipada da opção `effectiveCursorStyle`.
pub const EFFECTIVE_CURSOR_STYLE: Opt<TextEditorCursorStyle> = Opt::new(EditorOption::EffectiveCursorStyle);
/// Chave tipada da opção `effectiveEditContext`.
pub const EFFECTIVE_EDIT_CONTEXT: Opt<bool> = Opt::new(EditorOption::EffectiveEditContext);
/// Chave tipada da opção `effectiveAllowVariableFonts`.
pub const EFFECTIVE_ALLOW_VARIABLE_FONTS: Opt<bool> = Opt::new(EditorOption::EffectiveAllowVariableFonts);
/// Chave tipada da opção `effectiveFullwidthCharacterWidth`.
pub const EFFECTIVE_FULLWIDTH_CHARACTER_WIDTH: Opt<FullwidthCharacterWidth> = Opt::new(EditorOption::EffectiveFullwidthCharacterWidth);

const CURSOR_STYLE: Opt<TextEditorCursorStyle> = Opt::new(EditorOption::CursorStyle);
const OVERTYPE_CURSOR_STYLE: Opt<TextEditorCursorStyle> = Opt::new(EditorOption::OvertypeCursorStyle);
const EDIT_CONTEXT: Opt<bool> = Opt::new(EditorOption::EditContext);
const ALLOW_VARIABLE_FONTS: Opt<bool> = Opt::new(EditorOption::AllowVariableFonts);
const ALLOW_VARIABLE_FONTS_IN_ACCESSIBILITY_MODE: Opt<bool> = Opt::new(EditorOption::AllowVariableFontsInAccessibilityMode);
const FULLWIDTH_CHARACTER_WIDTH: Opt<String> = Opt::new(EditorOption::FullwidthCharacterWidth);

string_enum! {
    /// `'font' | 'twoCells'`: a largura dos caracteres de largura total.
    pub enum FullwidthCharacterWidth {
        Font => "font",
        TwoCells => "twoCells",
    }
}

/// `EffectiveCursorStyle`: o estilo de sobrescrita no modo `overtype`, o de inserção no resto.
pub fn effective_cursor_style() -> ComputedEditorOption<TextEditorCursorStyle> {
    ComputedEditorOption::new(EditorOption::EffectiveCursorStyle, TextEditorCursorStyle::Line, |env, options, _| {
        if env.input_mode == InputMode::Overtype {
            *options.get(OVERTYPE_CURSOR_STYLE)
        } else {
            *options.get(CURSOR_STYLE)
        }
    })
}

/// `EffectiveEditContextEnabled`.
pub fn effective_edit_context_enabled() -> ComputedEditorOption<bool> {
    ComputedEditorOption::new(EditorOption::EffectiveEditContext, false, |env, options, _| {
        env.edit_context_supported && *options.get(EDIT_CONTEXT)
    })
}

/// `EffectiveAllowVariableFonts`: com leitor de tela ligado vale `allowVariableFontsInAccessibilityMode`.
pub fn effective_allow_variable_fonts() -> ComputedEditorOption<bool> {
    ComputedEditorOption::new(EditorOption::EffectiveAllowVariableFonts, false, |env, options, _| {
        if env.accessibility_support == AccessibilitySupport::Enabled {
            *options.get(ALLOW_VARIABLE_FONTS_IN_ACCESSIBILITY_MODE)
        } else {
            *options.get(ALLOW_VARIABLE_FONTS)
        }
    })
}

/// `EffectiveFullwidthCharacterWidth`: `twoCells` só com a opção em `twoCells` e a fonte monoespaçada.
pub fn effective_fullwidth_character_width() -> ComputedEditorOption<FullwidthCharacterWidth> {
    ComputedEditorOption::new(EditorOption::EffectiveFullwidthCharacterWidth, FullwidthCharacterWidth::Font, |env, options, _| {
        if options.get(FULLWIDTH_CHARACTER_WIDTH) == FullwidthCharacterWidth::TwoCells.as_str() && env.font_info.is_monospace {
            return FullwidthCharacterWidth::TwoCells;
        }
        FullwidthCharacterWidth::Font
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::editor_options::option_support::testing;
    use crate::config::editor_options::{EditorOptionImpl, EnvironmentalOptions};
    use je_base::dynamic::Dynamic;

    fn env(is_monospace: bool) -> EnvironmentalOptions {
        testing::environment(testing::font_info(is_monospace, 14.0, "normal"))
    }

    #[test]
    fn cursor_style_follows_the_input_mode() {
        let option = effective_cursor_style();
        let options = testing::options_with(vec![
            (EditorOption::CursorStyle, testing::value(TextEditorCursorStyle::Line)),
            (EditorOption::OvertypeCursorStyle, testing::value(TextEditorCursorStyle::Block)),
        ]);
        let mut env = env(true);
        assert_eq!(EditorOptionImpl::compute(&option, &env, &options, &TextEditorCursorStyle::Line), TextEditorCursorStyle::Line);
        env.input_mode = InputMode::Overtype;
        assert_eq!(EditorOptionImpl::compute(&option, &env, &options, &TextEditorCursorStyle::Line), TextEditorCursorStyle::Block);
        assert_eq!(EditorOptionImpl::default_value(&option), TextEditorCursorStyle::Line);
        assert_eq!(EditorOptionImpl::name(&option), "_never_");
    }

    #[test]
    fn edit_context_needs_support_and_the_option() {
        let option = effective_edit_context_enabled();
        let on = testing::options_with(vec![(EditorOption::EditContext, testing::value(true))]);
        let off = testing::options_with(vec![(EditorOption::EditContext, testing::value(false))]);
        let mut env = env(true);
        assert!(!EditorOptionImpl::compute(&option, &env, &on, &false));
        env.edit_context_supported = true;
        assert!(EditorOptionImpl::compute(&option, &env, &on, &false));
        assert!(!EditorOptionImpl::compute(&option, &env, &off, &false));
        assert!(!EditorOptionImpl::default_value(&option));
    }

    #[test]
    fn allow_variable_fonts_switches_on_accessibility() {
        let option = effective_allow_variable_fonts();
        let options = testing::options_with(vec![
            (EditorOption::AllowVariableFonts, testing::value(true)),
            (EditorOption::AllowVariableFontsInAccessibilityMode, testing::value(false)),
        ]);
        let mut env = env(true);
        for support in [AccessibilitySupport::Unknown, AccessibilitySupport::Disabled] {
            env.accessibility_support = support;
            assert!(EditorOptionImpl::compute(&option, &env, &options, &false));
        }
        env.accessibility_support = AccessibilitySupport::Enabled;
        assert!(!EditorOptionImpl::compute(&option, &env, &options, &false));
    }

    #[test]
    fn fullwidth_needs_two_cells_and_a_monospace_font() {
        let option = effective_fullwidth_character_width();
        let two_cells = testing::options_with(vec![(EditorOption::FullwidthCharacterWidth, testing::value("twoCells".to_string()))]);
        let font = testing::options_with(vec![(EditorOption::FullwidthCharacterWidth, testing::value("font".to_string()))]);
        let compute = |env: &EnvironmentalOptions, options| EditorOptionImpl::compute(&option, env, options, &FullwidthCharacterWidth::Font);
        assert_eq!(compute(&env(true), &two_cells), FullwidthCharacterWidth::TwoCells);
        assert_eq!(compute(&env(false), &two_cells), FullwidthCharacterWidth::Font);
        assert_eq!(compute(&env(true), &font), FullwidthCharacterWidth::Font);
        assert_eq!(EditorOptionImpl::default_value(&option), FullwidthCharacterWidth::Font);
    }

    #[test]
    fn computed_options_ignore_the_input_in_validate() {
        assert!(!EditorOptionImpl::validate(&effective_edit_context_enabled(), &Dynamic::Bool(true)));
        assert_eq!(
            EditorOptionImpl::validate(&effective_fullwidth_character_width(), &Dynamic::string("twoCells")),
            FullwidthCharacterWidth::Font
        );
        assert_eq!(FullwidthCharacterWidth::from_text("twoCells"), Some(FullwidthCharacterWidth::TwoCells));
        assert_eq!(FullwidthCharacterWidth::ALLOWED, ["font", "twoCells"]);
    }
}
