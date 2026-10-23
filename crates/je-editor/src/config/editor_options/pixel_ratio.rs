//! Porte de `EditorPixelRatio` (`editorOptions.ts`, linhas 3758 a 3771): opção só calculada, padrão `1`,
//! que devolve `env.pixelRatio`.

use crate::config::editor_options::{ComputedEditorOption, EditorOption, Opt};

/// Chave tipada de `pixelRatio`.
pub const PIXEL_RATIO: Opt<f64> = Opt::new(EditorOption::PixelRatio);

/// `EditorPixelRatio`.
pub fn editor_pixel_ratio() -> ComputedEditorOption<f64> {
    ComputedEditorOption::new(EditorOption::PixelRatio, 1.0, |env, _options, _value| env.pixel_ratio)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::editor_options::option_support::testing::{environment, font_info, options_with};
    use crate::config::editor_options::EditorOptionImpl;
    use je_base::dynamic::Dynamic;

    #[test]
    fn identity_and_default() {
        let option = editor_pixel_ratio();
        assert_eq!(option.id(), EditorOption::PixelRatio);
        assert_eq!(option.name(), "_never_");
        assert_eq!(option.default_value(), 1.0);
        assert!(option.schema().is_none());
    }

    #[test]
    fn validate_ignores_the_input() {
        let option = editor_pixel_ratio();
        assert_eq!(option.validate(&Dynamic::Number(3.0)), 1.0);
        assert_eq!(option.validate(&Dynamic::string("2")), 1.0);
        assert_eq!(option.validate(&Dynamic::Undefined), 1.0);
    }

    #[test]
    fn compute_takes_the_environment_value() {
        let option = editor_pixel_ratio();
        let options = options_with(vec![]);
        let mut env = environment(font_info(true, 14.0, "normal"));
        env.pixel_ratio = 2.0;
        assert_eq!(option.compute(&env, &options, &1.0), 2.0);
        env.pixel_ratio = 1.25;
        assert_eq!(option.compute(&env, &options, &2.0), 1.25);
    }
}
