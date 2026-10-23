//! Porte de `EditorFontVariations` (`editorOptions.ts`, região `fontVariations`).
//!
//! A entrada é `boolean | string`; o valor calculado vem de `env.fontInfo.fontVariationSettings`.

use je_base::dynamic::{Dynamic, Truthy};

use crate::config::editor_options::{ComputedEditorOptions, EditorOption, EditorOptionImpl, EnvironmentalOptions, Opt};
use crate::config::font_info::{FONT_VARIATION_OFF, FONT_VARIATION_TRANSLATE};

/// Chave tipada da opção `fontVariations`.
pub const FONT_VARIATIONS: Opt<String> = Opt::new(EditorOption::FontVariations);

/// `EditorFontVariations`.
pub struct EditorFontVariations {
    schema: Dynamic,
}

impl EditorFontVariations {
    /// `EditorFontVariations.OFF`: o texto é desenhado com as configurações padrão.
    pub const OFF: &'static str = FONT_VARIATION_OFF;
    /// `EditorFontVariations.TRANSLATE`: traduz `fontWeight` para `font-variation-settings`.
    pub const TRANSLATE: &'static str = FONT_VARIATION_TRANSLATE;

    pub fn new() -> Self {
        let schema = Dynamic::object([
            (
                "anyOf",
                Dynamic::array([
                    Dynamic::object([
                        ("type", Dynamic::string("boolean")),
                        (
                            "description",
                            Dynamic::string("Enables/Disables the translation from font-weight to font-variation-settings. Change this to a string for fine-grained control of the 'font-variation-settings' CSS property."),
                        ),
                    ]),
                    Dynamic::object([
                        ("type", Dynamic::string("string")),
                        (
                            "description",
                            Dynamic::string("Explicit 'font-variation-settings' CSS property. A boolean can be passed instead if one only needs to translate font-weight to font-variation-settings."),
                        ),
                    ]),
                ]),
            ),
            (
                "description",
                Dynamic::string("Configures font variations. Can be either a boolean to enable/disable the translation from font-weight to font-variation-settings or a string for the value of the CSS 'font-variation-settings' property."),
            ),
            ("default", Dynamic::Bool(false)),
        ]);
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

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> String {
        match input {
            Dynamic::Undefined => self.default_value(),
            Dynamic::String(text) => {
                let text = text.to_string_lossy();
                if text == "false" {
                    Self::OFF.to_string()
                } else if text == "true" {
                    Self::TRANSLATE.to_string()
                } else {
                    // Diferente de `fontLigatures`, a string vazia passa como está.
                    text
                }
            }
            other if other.is_truthy() => Self::TRANSLATE.to_string(),
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

    fn validate(input: &Dynamic) -> String {
        EditorOptionImpl::validate(&EditorFontVariations::new(), input)
    }

    #[test]
    fn default_and_strings() {
        assert_eq!(validate(&Dynamic::Undefined), "normal");
        assert_eq!(validate(&Dynamic::string("false")), EditorFontVariations::OFF);
        assert_eq!(validate(&Dynamic::string("true")), EditorFontVariations::TRANSLATE);
        assert_eq!(validate(&Dynamic::string("'wght' 450")), "'wght' 450");
        // A string vazia não é tratada como falso aqui.
        assert_eq!(validate(&Dynamic::string("")), "");
        assert_eq!(EditorFontVariations::TRANSLATE, "translate");
    }

    #[test]
    fn non_strings_use_truthiness() {
        assert_eq!(validate(&Dynamic::Bool(true)), EditorFontVariations::TRANSLATE);
        assert_eq!(validate(&Dynamic::Bool(false)), EditorFontVariations::OFF);
        assert_eq!(validate(&Dynamic::Null), EditorFontVariations::OFF);
        assert_eq!(validate(&Dynamic::Number(0.0)), EditorFontVariations::OFF);
        assert_eq!(validate(&Dynamic::Number(1.0)), EditorFontVariations::TRANSLATE);
        assert_eq!(validate(&Dynamic::object::<&str>([])), EditorFontVariations::TRANSLATE);
    }

    #[test]
    fn schema_follows_the_original() {
        let option = EditorFontVariations::new();
        let schema = EditorOptionImpl::schema(&option).expect("sem esquema");
        assert_eq!(schema.own_keys(), vec!["anyOf", "description", "default"]);
        assert_eq!(schema.get_str("anyOf").get_str("length"), Dynamic::Number(2.0));
        assert_eq!(schema.get_str("default"), Dynamic::Bool(false));
    }

    #[test]
    fn compute_takes_the_variation_settings_from_the_font_info() {
        let option = EditorFontVariations::new();
        let env = testing::environment(testing::font_info(true, 14.0, "'wght' 450"));
        let options = testing::options_with(vec![]);
        assert_eq!(EditorOptionImpl::compute(&option, &env, &options, &"normal".to_string()), "'wght' 450");
    }
}
