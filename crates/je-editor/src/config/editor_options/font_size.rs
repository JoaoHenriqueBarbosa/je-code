//! Porte de `EditorFontSize` (`editorOptions.ts`, região `fontSize`).
//!
//! O valor calculado vem de `env.fontInfo.fontSize`, que já respeita o zoom do editor.

use je_base::dynamic::Dynamic;

use crate::config::editor_options::{clamp_float, float, ComputedEditorOptions, EditorOption, EditorOptionImpl, EnvironmentalOptions, Opt};
use crate::config::font_info::editor_font_defaults;

/// Chave tipada da opção `fontSize`.
pub const FONT_SIZE: Opt<f64> = Opt::new(EditorOption::FontSize);

const MINIMUM: f64 = 6.0;
const MAXIMUM: f64 = 100.0;

/// `EditorFontSize`.
pub struct EditorFontSize {
    default_value: f64,
    schema: Dynamic,
}

impl EditorFontSize {
    pub fn new() -> Self {
        let default_value = editor_font_defaults().font_size;
        let schema = Dynamic::object([
            ("type", Dynamic::string("number")),
            ("minimum", MINIMUM.into()),
            ("maximum", MAXIMUM.into()),
            ("default", default_value.into()),
            ("description", Dynamic::string("Controls the font size in pixels.")),
        ]);
        Self { default_value, schema }
    }
}

impl Default for EditorFontSize {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorFontSize {
    type Value = f64;

    fn id(&self) -> EditorOption {
        EditorOption::FontSize
    }

    fn name(&self) -> &str {
        "fontSize"
    }

    fn default_value(&self) -> f64 {
        self.default_value
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> f64 {
        let r = float(input, self.default_value);
        if r == 0.0 {
            return editor_font_defaults().font_size;
        }
        clamp_float(r, MINIMUM, MAXIMUM)
    }

    fn compute(&self, env: &EnvironmentalOptions, _options: &ComputedEditorOptions, _value: &f64) -> f64 {
        // O `fontSize` final respeita o zoom do editor, então vale o resultado de `env.fontInfo`.
        env.font_info.font_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::editor_options::option_support::testing;

    fn validate(input: &Dynamic) -> f64 {
        EditorOptionImpl::validate(&EditorFontSize::new(), input)
    }

    #[test]
    fn default_follows_the_platform() {
        let expected = editor_font_defaults().font_size;
        assert_eq!(EditorOptionImpl::default_value(&EditorFontSize::new()), expected);
        assert_eq!(validate(&Dynamic::Undefined), expected);
        assert_eq!(validate(&Dynamic::string("abc")), expected);
        assert_eq!(validate(&Dynamic::Null), expected);
    }

    #[test]
    fn zero_means_the_platform_default() {
        let expected = editor_font_defaults().font_size;
        assert_eq!(validate(&Dynamic::Number(0.0)), expected);
        assert_eq!(validate(&Dynamic::string("0")), expected);
        assert_eq!(validate(&Dynamic::string("-0")), expected);
    }

    #[test]
    fn clamps_between_6_and_100() {
        assert_eq!(validate(&Dynamic::Number(1.0)), 6.0);
        assert_eq!(validate(&Dynamic::Number(-5.0)), 6.0);
        assert_eq!(validate(&Dynamic::Number(500.0)), 100.0);
        assert_eq!(validate(&Dynamic::Number(12.5)), 12.5);
        assert_eq!(validate(&Dynamic::string("18px")), 18.0);
        assert_eq!(validate(&Dynamic::string("Infinity")), 100.0);
    }

    #[test]
    fn compute_uses_the_font_info_size() {
        let option = EditorFontSize::new();
        let env = testing::environment(testing::font_info(true, 16.8, "normal"));
        let options = testing::options_with(vec![]);
        assert_eq!(EditorOptionImpl::compute(&option, &env, &options, &12.0), 16.8);
    }

    #[test]
    fn schema_follows_the_original() {
        let option = EditorFontSize::new();
        let schema = EditorOptionImpl::schema(&option).expect("sem esquema");
        assert_eq!(schema.own_keys(), vec!["type", "minimum", "maximum", "default", "description"]);
        assert_eq!((schema.get_str("minimum"), schema.get_str("maximum")), (Dynamic::Number(6.0), Dynamic::Number(100.0)));
        assert_eq!(schema.get_str("type"), Dynamic::string("number"));
        assert_eq!(schema.get_str("default"), Dynamic::Number(editor_font_defaults().font_size));
    }
}
