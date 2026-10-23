//! Porte de `EditorFontLigatures` (`editorOptions.ts`, região `fontLigatures`).
//!
//! A entrada é `boolean | string` e o valor calculado é sempre a string de `font-feature-settings`.

use je_base::dynamic::{Dynamic, Truthy};

use crate::config::editor_options::{EditorOption, EditorOptionImpl, Opt};

/// Chave tipada da opção `fontLigatures`.
pub const FONT_LIGATURES: Opt<String> = Opt::new(EditorOption::FontLigatures);

/// `EditorFontLigatures`.
pub struct EditorFontLigatures {
    schema: Dynamic,
}

impl EditorFontLigatures {
    /// `EditorFontLigatures.OFF`.
    pub const OFF: &'static str = "\"liga\" off, \"calt\" off";
    /// `EditorFontLigatures.ON`.
    pub const ON: &'static str = "\"liga\" on, \"calt\" on";

    pub fn new() -> Self {
        let schema = Dynamic::object([
            (
                "anyOf",
                Dynamic::array([
                    Dynamic::object([
                        ("type", Dynamic::string("boolean")),
                        (
                            "description",
                            Dynamic::string("Enables/Disables font ligatures ('calt' and 'liga' font features). Change this to a string for fine-grained control of the 'font-feature-settings' CSS property."),
                        ),
                    ]),
                    Dynamic::object([
                        ("type", Dynamic::string("string")),
                        (
                            "description",
                            Dynamic::string("Explicit 'font-feature-settings' CSS property. A boolean can be passed instead if one only needs to turn on/off ligatures."),
                        ),
                    ]),
                ]),
            ),
            (
                "description",
                Dynamic::string("Configures font ligatures or font features. Can be either a boolean to enable/disable ligatures or a string for the value of the CSS 'font-feature-settings' property."),
            ),
            ("default", Dynamic::Bool(false)),
        ]);
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

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> String {
        match input {
            Dynamic::Undefined => self.default_value(),
            Dynamic::String(text) => {
                let text = text.to_string_lossy();
                if text == "false" || text.is_empty() {
                    Self::OFF.to_string()
                } else if text == "true" {
                    Self::ON.to_string()
                } else {
                    text
                }
            }
            other if other.is_truthy() => Self::ON.to_string(),
            _ => Self::OFF.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validate(input: &Dynamic) -> String {
        EditorOptionImpl::validate(&EditorFontLigatures::new(), input)
    }

    #[test]
    fn undefined_is_the_default() {
        assert_eq!(validate(&Dynamic::Undefined), EditorFontLigatures::OFF);
        assert_eq!(EditorOptionImpl::default_value(&EditorFontLigatures::new()), "\"liga\" off, \"calt\" off");
    }

    #[test]
    fn strings() {
        assert_eq!(validate(&Dynamic::string("false")), EditorFontLigatures::OFF);
        assert_eq!(validate(&Dynamic::string("")), EditorFontLigatures::OFF);
        assert_eq!(validate(&Dynamic::string("true")), EditorFontLigatures::ON);
        assert_eq!(validate(&Dynamic::string("\"ss01\" on")), "\"ss01\" on");
        // Só a minúscula exata conta como booleano.
        assert_eq!(validate(&Dynamic::string("True")), "True");
        assert_eq!(EditorFontLigatures::ON, "\"liga\" on, \"calt\" on");
    }

    #[test]
    fn non_strings_use_truthiness() {
        assert_eq!(validate(&Dynamic::Bool(true)), EditorFontLigatures::ON);
        assert_eq!(validate(&Dynamic::Bool(false)), EditorFontLigatures::OFF);
        assert_eq!(validate(&Dynamic::Null), EditorFontLigatures::OFF);
        assert_eq!(validate(&Dynamic::Number(0.0)), EditorFontLigatures::OFF);
        assert_eq!(validate(&Dynamic::Number(f64::NAN)), EditorFontLigatures::OFF);
        assert_eq!(validate(&Dynamic::Number(2.0)), EditorFontLigatures::ON);
        assert_eq!(validate(&Dynamic::object::<&str>([])), EditorFontLigatures::ON);
        assert_eq!(validate(&Dynamic::array([])), EditorFontLigatures::ON);
    }

    #[test]
    fn schema_follows_the_original() {
        let option = EditorFontLigatures::new();
        let schema = EditorOptionImpl::schema(&option).expect("sem esquema");
        assert_eq!(schema.own_keys(), vec!["anyOf", "description", "default"]);
        assert!(schema.get_str("anyOf").is_array());
        assert_eq!(schema.get_str("anyOf").get_str("length"), Dynamic::Number(2.0));
        assert_eq!(schema.get_str("anyOf").get_str("0").get_str("type"), Dynamic::string("boolean"));
        assert_eq!(schema.get_str("anyOf").get_str("1").get_str("type"), Dynamic::string("string"));
        assert_eq!(schema.get_str("default"), Dynamic::Bool(false));
    }
}
