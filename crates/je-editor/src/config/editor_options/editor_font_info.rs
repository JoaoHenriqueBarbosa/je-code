//! Porte de `EditorFontInfo` (`editorOptions.ts`, região `fontInfo`): a opção só calculada que expõe o
//! `FontInfo` medido do ambiente (`env.font_info`).

use crate::config::editor_options::{ComputedEditorOption, EditorOption, Opt};
use crate::config::font_info::{BareFontInfoOptions, FontInfo, FontInfoOptions};

/// Chave tipada da opção `fontInfo`.
pub const FONT_INFO: Opt<FontInfo> = Opt::new(EditorOption::FontInfo);

/// O valor padrão do original: um `FontInfo` com tudo zerado, não confiável.
pub fn default_font_info() -> FontInfo {
    FontInfo::new(
        FontInfoOptions {
            bare: BareFontInfoOptions {
                pixel_ratio: 0.0,
                font_family: String::new(),
                font_weight: String::new(),
                font_size: 0.0,
                font_feature_settings: String::new(),
                font_variation_settings: String::new(),
                line_height: 0.0,
                letter_spacing: 0.0,
            },
            is_monospace: false,
            typical_halfwidth_character_width: 0.0,
            typical_fullwidth_character_width: 0.0,
            can_use_halfwidth_rightwards_arrow: false,
            space_width: 0.0,
            middot_width: 0.0,
            wsmiddot_width: 0.0,
            max_digit_width: 0.0,
        },
        false,
    )
}

/// `EditorFontInfo`.
pub fn editor_font_info() -> ComputedEditorOption<FontInfo> {
    ComputedEditorOption::new(EditorOption::FontInfo, default_font_info(), |env, _options, _| env.font_info.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::editor_options::option_support::testing;
    use crate::config::editor_options::EditorOptionImpl;

    #[test]
    fn default_is_all_zero_and_untrusted() {
        let option = editor_font_info();
        let default = EditorOptionImpl::default_value(&option);
        assert_eq!(default.font_family, "");
        assert_eq!(default.font_size, 0.0);
        assert_eq!(default.line_height, 0.0);
        assert!(!default.is_monospace);
        assert!(!default.is_trusted);
        assert_eq!(EditorOptionImpl::name(&option), "_never_");
        assert_eq!(EditorOptionImpl::id(&option), EditorOption::FontInfo);
    }

    #[test]
    fn validate_ignores_the_input_and_compute_returns_the_environment_font() {
        let option = editor_font_info();
        let env_font = testing::font_info(true, 15.0, "normal");
        let env = testing::environment(env_font.clone());
        let options = testing::options_with(vec![]);
        assert_eq!(EditorOptionImpl::compute(&option, &env, &options, &default_font_info()), env_font);
        assert_eq!(
            EditorOptionImpl::validate(&option, &je_base::dynamic::Dynamic::string("x")),
            default_font_info()
        );
    }
}
