//! Porte de `EditorFontLigatures` (`editorOptions.ts`, região `fontLigatures`).
//!
//! A entrada é `boolean | string` e o valor calculado é sempre a string de `font-feature-settings`.
//! `// TODO(config)`: o esquema do original usa `anyOf` (booleano ou string, cada um com sua descrição),
//! que `ConfigurationPropertySchema` ainda não tem. Aqui o esquema guarda só o padrão (`false`) e a
//! descrição geral.

use crate::config::editor_options::{
    ConfigurationPropertySchema, EditorOption, EditorOptionImpl, Opt, OptionInput, OptionSchema, SchemaValue,
};

/// Chave tipada da opção `fontLigatures`.
pub const FONT_LIGATURES: Opt<String> = Opt::new(EditorOption::FontLigatures);

/// `EditorFontLigatures`.
pub struct EditorFontLigatures {
    schema: OptionSchema,
}

impl EditorFontLigatures {
    /// `EditorFontLigatures.OFF`.
    pub const OFF: &'static str = "\"liga\" off, \"calt\" off";
    /// `EditorFontLigatures.ON`.
    pub const ON: &'static str = "\"liga\" on, \"calt\" on";

    pub fn new() -> Self {
        let schema = OptionSchema::Single(ConfigurationPropertySchema {
            default: Some(SchemaValue::Bool(false)),
            description: Some(
                "Configures font ligatures or font features. Can be either a boolean to enable/disable ligatures or a string for the value of the CSS 'font-feature-settings' property."
                    .to_string(),
            ),
            ..Default::default()
        });
        Self { schema }
    }
}

impl Default for EditorFontLigatures {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorFontLigatures {
    type Value = String;

    fn id(&self) -> EditorOption {
        EditorOption::FontLigatures
    }

    fn name(&self) -> &str {
        "fontLigatures"
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
                if text == "false" || text.is_empty() {
                    Self::OFF.to_string()
                } else if text == "true" {
                    Self::ON.to_string()
                } else {
                    text.clone()
                }
            }
            other if other.truthy() => Self::ON.to_string(),
            _ => Self::OFF.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validate(input: &OptionInput) -> String {
        EditorOptionImpl::validate(&EditorFontLigatures::new(), input)
    }

    #[test]
    fn undefined_is_the_default() {
        assert_eq!(validate(&OptionInput::Undefined), EditorFontLigatures::OFF);
        assert_eq!(EditorOptionImpl::default_value(&EditorFontLigatures::new()), "\"liga\" off, \"calt\" off");
    }

    #[test]
    fn strings() {
        assert_eq!(validate(&OptionInput::string("false")), EditorFontLigatures::OFF);
        assert_eq!(validate(&OptionInput::string("")), EditorFontLigatures::OFF);
        assert_eq!(validate(&OptionInput::string("true")), EditorFontLigatures::ON);
        assert_eq!(validate(&OptionInput::string("\"ss01\" on")), "\"ss01\" on");
        // Só a minúscula exata conta como booleano.
        assert_eq!(validate(&OptionInput::string("True")), "True");
        assert_eq!(EditorFontLigatures::ON, "\"liga\" on, \"calt\" on");
    }

    #[test]
    fn non_strings_use_truthiness() {
        assert_eq!(validate(&OptionInput::Bool(true)), EditorFontLigatures::ON);
        assert_eq!(validate(&OptionInput::Bool(false)), EditorFontLigatures::OFF);
        assert_eq!(validate(&OptionInput::Null), EditorFontLigatures::OFF);
        assert_eq!(validate(&OptionInput::Number(0.0)), EditorFontLigatures::OFF);
        assert_eq!(validate(&OptionInput::Number(f64::NAN)), EditorFontLigatures::OFF);
        assert_eq!(validate(&OptionInput::Number(2.0)), EditorFontLigatures::ON);
        assert_eq!(validate(&OptionInput::Other), EditorFontLigatures::ON);
    }

    #[test]
    fn schema_default_is_false() {
        let option = EditorFontLigatures::new();
        let Some(OptionSchema::Single(schema)) = EditorOptionImpl::schema(&option) else { panic!("sem esquema") };
        assert_eq!(schema.default, Some(SchemaValue::Bool(false)));
    }
}
