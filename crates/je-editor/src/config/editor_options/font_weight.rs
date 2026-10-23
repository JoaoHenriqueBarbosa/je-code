//! Porte de `EditorFontWeight` (`editorOptions.ts`, região `fontWeight`).

use je_base::dynamic::Dynamic;

use crate::config::editor_options::{clamped_int_or, strings_array, EditorOption, EditorOptionImpl, Opt};
use crate::config::font_info::editor_font_defaults;

/// Chave tipada da opção `fontWeight`.
pub const FONT_WEIGHT: Opt<String> = Opt::new(EditorOption::FontWeight);

/// `EditorFontWeight.SUGGESTION_VALUES`.
pub const SUGGESTION_VALUES: [&str; 11] = ["normal", "bold", "100", "200", "300", "400", "500", "600", "700", "800", "900"];

const MINIMUM_VALUE: f64 = 1.0;
const MAXIMUM_VALUE: f64 = 1000.0;

/// `EditorFontWeight`.
pub struct EditorFontWeight {
    schema: Dynamic,
}

impl EditorFontWeight {
    pub fn new() -> Self {
        let schema = Dynamic::object([
            (
                "anyOf",
                Dynamic::array([
                    Dynamic::object([
                        ("type", Dynamic::string("number")),
                        ("minimum", MINIMUM_VALUE.into()),
                        ("maximum", MAXIMUM_VALUE.into()),
                        (
                            "errorMessage",
                            Dynamic::string("Only \"normal\" and \"bold\" keywords or numbers between 1 and 1000 are allowed."),
                        ),
                    ]),
                    Dynamic::object([
                        ("type", Dynamic::string("string")),
                        ("pattern", Dynamic::string("^(normal|bold|1000|[1-9][0-9]{0,2})$")),
                    ]),
                    Dynamic::object([("enum", strings_array(&SUGGESTION_VALUES))]),
                ]),
            ),
            ("default", Dynamic::string(editor_font_defaults().font_weight)),
            (
                "description",
                Dynamic::string("Controls the font weight. Accepts \"normal\" and \"bold\" keywords or numbers between 1 and 1000."),
            ),
        ]);
        Self { schema }
    }
}

impl Default for EditorFontWeight {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorFontWeight {
    type Value = String;

    fn id(&self) -> EditorOption {
        EditorOption::FontWeight
    }

    fn name(&self) -> &str {
        "fontWeight"
    }

    fn default_value(&self) -> String {
        editor_font_defaults().font_weight.to_string()
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> String {
        if let Dynamic::String(text) = input {
            if *text == "normal" || *text == "bold" {
                return text.to_string_lossy();
            }
        }
        // `String(clampedInt(input, 'normal', 1, 1000))`: o padrão é a própria string.
        match clamped_int_or(input, editor_font_defaults().font_weight, MINIMUM_VALUE, MAXIMUM_VALUE) {
            Ok(weight) => weight.to_string(),
            Err(default_weight) => default_weight.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validate(input: &Dynamic) -> String {
        EditorOptionImpl::validate(&EditorFontWeight::new(), input)
    }

    #[test]
    fn keywords_pass_through() {
        assert_eq!(validate(&Dynamic::string("normal")), "normal");
        assert_eq!(validate(&Dynamic::string("bold")), "bold");
    }

    #[test]
    fn numbers_are_clamped_and_become_strings() {
        assert_eq!(validate(&Dynamic::Number(400.0)), "400");
        assert_eq!(validate(&Dynamic::string("700")), "700");
        assert_eq!(validate(&Dynamic::string("450px")), "450");
        assert_eq!(validate(&Dynamic::Number(0.0)), "1");
        assert_eq!(validate(&Dynamic::Number(-30.0)), "1");
        assert_eq!(validate(&Dynamic::Number(5000.0)), "1000");
        assert_eq!(validate(&Dynamic::Number(350.9)), "350");
    }

    #[test]
    fn invalid_values_fall_back_to_normal() {
        assert_eq!(validate(&Dynamic::Undefined), "normal");
        assert_eq!(validate(&Dynamic::string("semibold")), "normal");
        assert_eq!(validate(&Dynamic::string("")), "normal");
        assert_eq!(validate(&Dynamic::Bool(true)), "normal");
        assert_eq!(validate(&Dynamic::Null), "normal");
        assert_eq!(validate(&Dynamic::Number(f64::NAN)), "normal");
        assert_eq!(validate(&Dynamic::array([Dynamic::Number(400.0)])), "normal");
        // Só as palavras exatas passam direto.
        assert_eq!(validate(&Dynamic::string("Bold")), "normal");
    }

    #[test]
    fn default_suggestions_and_schema() {
        assert_eq!(EditorOptionImpl::default_value(&EditorFontWeight::new()), "normal");
        assert_eq!(SUGGESTION_VALUES.len(), 11);
        assert_eq!(SUGGESTION_VALUES[0], "normal");
        assert_eq!(SUGGESTION_VALUES[10], "900");

        let option = EditorFontWeight::new();
        let schema = EditorOptionImpl::schema(&option).expect("sem esquema");
        assert_eq!(schema.own_keys(), vec!["anyOf", "default", "description"]);
        let any_of = schema.get_str("anyOf");
        assert_eq!(any_of.get_str("length"), Dynamic::Number(3.0));
        assert_eq!(any_of.get_str("0").own_keys(), vec!["type", "minimum", "maximum", "errorMessage"]);
        assert_eq!(any_of.get_str("1").get_str("pattern"), Dynamic::string("^(normal|bold|1000|[1-9][0-9]{0,2})$"));
        assert_eq!(any_of.get_str("2").get_str("enum").get_str("length"), Dynamic::Number(11.0));
        assert_eq!(schema.get_str("default"), Dynamic::string("normal"));
    }
}
