//! Porte de `EditorLineHeight` (`editorOptions.ts`, linhas 3381 a 3404).
//!
//! É um [`EditorFloatOption`] cujo `compute` ignora o valor e devolve `env.fontInfo.lineHeight`
//! (a altura final respeita o zoom do editor).

use je_base::dynamic::Dynamic;

use crate::config::editor_options::{clamp_float, ComputedEditorOptions, EditorFloatOption, EditorOption, EditorOptionImpl, EnvironmentalOptions, Opt};
use crate::config::font_info::editor_font_defaults;

/// Chave tipada de `lineHeight`.
pub const LINE_HEIGHT: Opt<f64> = Opt::new(EditorOption::LineHeight);

/// `EditorLineHeight`.
pub struct EditorLineHeight {
    inner: EditorFloatOption,
}

impl EditorLineHeight {
    pub fn new() -> Self {
        let schema = Dynamic::object([(
            "markdownDescription",
            Dynamic::string("Controls the line height. \n - Use 0 to automatically compute the line height from the font size.\n - Values between 0 and 8 will be used as a multiplier with the font size.\n - Values greater than or equal to 8 will be used as effective values."),
        )]);
        let inner = EditorFloatOption::new(
            EditorOption::LineHeight,
            "lineHeight",
            editor_font_defaults().line_height,
            |x| clamp_float(x, 0.0, 150.0),
            Some(schema),
            Some(0.0),
            Some(150.0),
        );
        Self { inner }
    }
}

impl Default for EditorLineHeight {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorLineHeight {
    type Value = f64;

    fn id(&self) -> EditorOption {
        self.inner.id()
    }

    fn name(&self) -> &str {
        self.inner.name()
    }

    fn default_value(&self) -> f64 {
        self.inner.default_value()
    }

    fn schema(&self) -> Option<&Dynamic> {
        self.inner.schema()
    }

    fn validate(&self, input: &Dynamic) -> f64 {
        self.inner.validate(input)
    }

    fn compute(&self, env: &EnvironmentalOptions, _options: &ComputedEditorOptions, _value: &f64) -> f64 {
        // A altura é calculada a partir do corpo da fonte quando vale 0, e respeita o zoom do editor:
        // o resultado vem do `FontInfo` do ambiente.
        env.font_info.line_height
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::editor_options::option_support::testing::{environment, font_info, options_with};

    fn text(s: &str) -> Dynamic {
        Dynamic::string(s)
    }

    fn validate(input: Dynamic) -> f64 {
        EditorLineHeight::new().validate(&input)
    }

    #[test]
    fn identity_and_default() {
        let option = EditorLineHeight::new();
        assert_eq!(option.id(), EditorOption::LineHeight);
        assert_eq!(option.name(), "lineHeight");
        assert_eq!(option.default_value(), editor_font_defaults().line_height);
        assert_eq!(option.default_value(), 0.0);
    }

    #[test]
    fn validate_clamps_between_0_and_150() {
        assert_eq!(validate(Dynamic::Number(20.5)), 20.5);
        assert_eq!(validate(Dynamic::Number(-1.0)), 0.0);
        assert_eq!(validate(Dynamic::Number(200.0)), 150.0);
        assert_eq!(validate(Dynamic::Number(150.0)), 150.0);
        assert_eq!(validate(Dynamic::Number(1.5)), 1.5);
    }

    #[test]
    fn validate_parses_strings_and_falls_back() {
        assert_eq!(validate(text("24")), 24.0);
        assert_eq!(validate(text("1.5em")), 1.5);
        assert_eq!(validate(text("abc")), 0.0);
        assert_eq!(validate(Dynamic::Undefined), 0.0);
        assert_eq!(validate(Dynamic::Bool(true)), 0.0);
        assert_eq!(validate(Dynamic::Number(f64::NAN)), 0.0);
    }

    #[test]
    fn compute_takes_the_font_info_line_height() {
        let option = EditorLineHeight::new();
        // O `FontInfo` de teste tem `lineHeight` 19, qualquer que seja o valor validado.
        let env = environment(font_info(true, 14.0, "normal"));
        let options = options_with(vec![]);
        assert_eq!(option.compute(&env, &options, &0.0), 19.0);
        assert_eq!(option.compute(&env, &options, &42.0), 19.0);
    }

    #[test]
    fn schema_keeps_the_original_keys_and_order() {
        let option = EditorLineHeight::new();
        let schema = option.schema().expect("sem esquema");
        assert_eq!(schema.own_keys(), vec!["markdownDescription", "type", "default", "minimum", "maximum"]);
        assert_eq!(schema.get_str("type"), text("number"));
        assert_eq!(schema.get_str("default"), Dynamic::Number(0.0));
        assert_eq!((schema.get_str("minimum"), schema.get_str("maximum")), (Dynamic::Number(0.0), Dynamic::Number(150.0)));
    }
}
