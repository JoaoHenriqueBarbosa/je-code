//! Porte de `EditorFontVariations` (`editorOptions.ts`, região `fontVariations`).
//!
//! A entrada é `boolean | string`; o valor calculado vem de `env.fontInfo.fontVariationSettings`.
//! `EnvironmentalOptions::font_info` (o `fontInfo: FontInfo` de `IEnvironmentalOptions`) precisa existir:
//! o `TODO(config)` de `editor_options.rs` já o anuncia.
//! `// TODO(config)`: o esquema do original usa `anyOf`, que `ConfigurationPropertySchema` ainda não tem.

use crate::config::editor_options::{
    ComputedEditorOptions, ConfigurationPropertySchema, EditorOption, EditorOptionImpl, EnvironmentalOptions, Opt, OptionInput,
    OptionSchema, SchemaValue,
};
use crate::config::font_info::{FONT_VARIATION_OFF, FONT_VARIATION_TRANSLATE};

/// Chave tipada da opção `fontVariations`.
pub const FONT_VARIATIONS: Opt<String> = Opt::new(EditorOption::FontVariations);

/// `EditorFontVariations`.
pub struct EditorFontVariations {
    schema: OptionSchema,
}

impl EditorFontVariations {
    /// `EditorFontVariations.OFF`: o texto é desenhado com as configurações padrão.
    pub const OFF: &'static str = FONT_VARIATION_OFF;
    /// `EditorFontVariations.TRANSLATE`: traduz `fontWeight` para `font-variation-settings`.
    pub const TRANSLATE: &'static str = FONT_VARIATION_TRANSLATE;

    pub fn new() -> Self {
        let schema = OptionSchema::Single(ConfigurationPropertySchema {
            default: Some(SchemaValue::Bool(false)),
            description: Some(
                "Configures font variations. Can be either a boolean to enable/disable the translation from font-weight to font-variation-settings or a string for the value of the CSS 'font-variation-settings' property."
                    .to_string(),
            ),
            ..Default::default()
        });
        Self { schema }
    }
}

impl Default for EditorFontVariations {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorFontVariations {
    type Value = String;

    fn id(&self) -> EditorOption {
        EditorOption::FontVariations
    }

    fn name(&self) -> &str {
        "fontVariations"
    }

    fn default_value(&self) -> String {
        Self::OFF.to_string()
    }

    fn schema(&self) -> Option<&OptionSchema> {
        Some(&self.schema)
    }

    fn validate(&self, input: &OptionInput) -> String {
        match input {
            OptionInput::Undefined => self.default_value(),
            OptionInput::String(text) => {
                if text == "false" {
                    Self::OFF.to_string()
                } else if text == "true" {
                    Self::TRANSLATE.to_string()
                } else {
                    // Diferente de `fontLigatures`, a string vazia passa como está.
                    text.clone()
                }
            }
            other if other.truthy() => Self::TRANSLATE.to_string(),
            _ => Self::OFF.to_string(),
        }
    }

    fn compute(&self, env: &EnvironmentalOptions, _options: &ComputedEditorOptions, _value: &String) -> String {
        // O valor vem do `fontWeight` quando é `true`, então vale o resultado de `env.fontInfo`.
        env.font_info.font_variation_settings.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::editor_options::option_support::testing;

    fn validate(input: &OptionInput) -> String {
        EditorOptionImpl::validate(&EditorFontVariations::new(), input)
    }

    #[test]
    fn default_and_strings() {
        assert_eq!(validate(&OptionInput::Undefined), "normal");
        assert_eq!(validate(&OptionInput::string("false")), EditorFontVariations::OFF);
        assert_eq!(validate(&OptionInput::string("true")), EditorFontVariations::TRANSLATE);
        assert_eq!(validate(&OptionInput::string("'wght' 450")), "'wght' 450");
        // A string vazia não é tratada como falso aqui.
        assert_eq!(validate(&OptionInput::string("")), "");
        assert_eq!(EditorFontVariations::TRANSLATE, "translate");
    }

    #[test]
    fn non_strings_use_truthiness() {
        assert_eq!(validate(&OptionInput::Bool(true)), EditorFontVariations::TRANSLATE);
        assert_eq!(validate(&OptionInput::Bool(false)), EditorFontVariations::OFF);
        assert_eq!(validate(&OptionInput::Null), EditorFontVariations::OFF);
        assert_eq!(validate(&OptionInput::Number(0.0)), EditorFontVariations::OFF);
        assert_eq!(validate(&OptionInput::Number(1.0)), EditorFontVariations::TRANSLATE);
        assert_eq!(validate(&OptionInput::Other), EditorFontVariations::TRANSLATE);
    }

    #[test]
    fn compute_takes_the_variation_settings_from_the_font_info() {
        let option = EditorFontVariations::new();
        let env = testing::environment(testing::font_info(true, 14.0, "'wght' 450"));
        let options = testing::options_with(vec![]);
        assert_eq!(EditorOptionImpl::compute(&option, &env, &options, &"normal".to_string()), "'wght' 450");
    }
}
