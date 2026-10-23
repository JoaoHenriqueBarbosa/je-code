//! Porte de `EditorLineHeight` (`editorOptions.ts`, linhas 3381 a 3404).
//!
//! É um [`EditorFloatOption`] cujo `compute` ignora o valor e devolve `env.fontInfo.lineHeight`
//! (a altura final respeita o zoom do editor). [`EnvironmentalOptions`] ainda não carrega o `FontInfo`
//! (`// TODO(config)` em `editor_options.rs`), então a conta está em
//! [`EditorLineHeight::compute_with_font_info`], e o `compute` do trait a chama assim que o ambiente
//! tiver a fonte.

use crate::config::editor_options::{clamp_float, ConfigurationPropertySchema, EditorFloatOption, EditorOption, EditorOptionImpl, Opt, OptionInput, OptionSchema};
use crate::config::font_info::{editor_font_defaults, FontInfo};

/// Chave tipada de `lineHeight`.
pub const LINE_HEIGHT: Opt<f64> = Opt::new(EditorOption::LineHeight);

/// `EditorLineHeight`.
pub struct EditorLineHeight {
    inner: EditorFloatOption,
}

impl EditorLineHeight {
    pub fn new() -> Self {
        let schema = ConfigurationPropertySchema {
            markdown_description: Some(
                "Controls the line height. \n - Use 0 to automatically compute the line height from the font size.\n - Values between 0 and 8 will be used as a multiplier with the font size.\n - Values greater than or equal to 8 will be used as effective values."
                    .to_string(),
            ),
            ..Default::default()
        };
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

    /// `compute` do original: a altura vem pronta do `FontInfo` (zero vira múltiplo do corpo da fonte,
    /// e o zoom já está aplicado).
    pub fn compute_with_font_info(&self, font_info: &FontInfo) -> f64 {
        font_info.line_height
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

    fn schema(&self) -> Option<&OptionSchema> {
        self.inner.schema()
    }

    fn validate(&self, input: &OptionInput) -> f64 {
        self.inner.validate(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::editor_options::option_support::test_font_info;
    use crate::config::editor_options::SchemaValue;

    fn validate(input: OptionInput) -> f64 {
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
        assert_eq!(validate(OptionInput::Number(20.5)), 20.5);
        assert_eq!(validate(OptionInput::Number(-1.0)), 0.0);
        assert_eq!(validate(OptionInput::Number(200.0)), 150.0);
        assert_eq!(validate(OptionInput::Number(150.0)), 150.0);
        assert_eq!(validate(OptionInput::Number(1.5)), 1.5);
    }

    #[test]
    fn validate_parses_strings_and_falls_back() {
        assert_eq!(validate(OptionInput::string("24")), 24.0);
        assert_eq!(validate(OptionInput::string("1.5em")), 1.5);
        assert_eq!(validate(OptionInput::string("abc")), 0.0);
        assert_eq!(validate(OptionInput::Undefined), 0.0);
        assert_eq!(validate(OptionInput::Bool(true)), 0.0);
        assert_eq!(validate(OptionInput::Number(f64::NAN)), 0.0);
    }

    #[test]
    fn compute_takes_the_font_info_line_height() {
        let option = EditorLineHeight::new();
        assert_eq!(option.compute_with_font_info(&test_font_info(19.0, 8.0)), 19.0);
        assert_eq!(option.compute_with_font_info(&test_font_info(30.0, 8.0)), 30.0);
    }

    #[test]
    fn schema_is_a_number_with_bounds() {
        let option = EditorLineHeight::new();
        let Some(OptionSchema::Single(schema)) = option.schema() else { panic!("sem esquema") };
        assert_eq!(schema.type_.as_deref(), Some("number"));
        assert_eq!(schema.default, Some(SchemaValue::Number(0.0)));
        assert_eq!((schema.minimum, schema.maximum), (Some(0.0), Some(150.0)));
        assert!(schema.markdown_description.is_some());
    }
}
