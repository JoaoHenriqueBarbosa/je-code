//! Porte de `EditorFontWeight` (`editorOptions.ts`, região `fontWeight`).
//!
//! `// TODO(config)`: o esquema do original usa `anyOf` (número de 1 a 1000 com `errorMessage`, string com
//! `pattern`, e a lista `SUGGESTION_VALUES`), que `ConfigurationPropertySchema` ainda não tem. Aqui o esquema
//! guarda só o padrão e a descrição.

use crate::config::editor_options::{
    clamped_int_or, ConfigurationPropertySchema, EditorOption, EditorOptionImpl, Opt, OptionInput, OptionSchema, SchemaValue,
};
use crate::config::font_info::editor_font_defaults;

/// Chave tipada da opção `fontWeight`.
pub const FONT_WEIGHT: Opt<String> = Opt::new(EditorOption::FontWeight);

/// `EditorFontWeight.SUGGESTION_VALUES`.
pub const SUGGESTION_VALUES: [&str; 11] = ["normal", "bold", "100", "200", "300", "400", "500", "600", "700", "800", "900"];

const MINIMUM_VALUE: f64 = 1.0;
const MAXIMUM_VALUE: f64 = 1000.0;

/// `EditorFontWeight`.
pub struct EditorFontWeight {
    schema: OptionSchema,
}

impl EditorFontWeight {
    pub fn new() -> Self {
        let schema = OptionSchema::Single(ConfigurationPropertySchema {
            default: Some(SchemaValue::String(editor_font_defaults().font_weight.to_string())),
            description: Some(
                "Controls the font weight. Accepts \"normal\" and \"bold\" keywords or numbers between 1 and 1000.".to_string(),
            ),
            ..Default::default()
        });
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

    fn schema(&self) -> Option<&OptionSchema> {
        Some(&self.schema)
    }

    fn validate(&self, input: &OptionInput) -> String {
        if let OptionInput::String(text) = input {
            if text == "normal" || text == "bold" {
                return text.clone();
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

    fn validate(input: &OptionInput) -> String {
        EditorOptionImpl::validate(&EditorFontWeight::new(), input)
    }

    #[test]
    fn keywords_pass_through() {
        assert_eq!(validate(&OptionInput::string("normal")), "normal");
        assert_eq!(validate(&OptionInput::string("bold")), "bold");
    }

    #[test]
    fn numbers_are_clamped_and_become_strings() {
        assert_eq!(validate(&OptionInput::Number(400.0)), "400");
        assert_eq!(validate(&OptionInput::string("700")), "700");
        assert_eq!(validate(&OptionInput::string("450px")), "450");
        assert_eq!(validate(&OptionInput::Number(0.0)), "1");
        assert_eq!(validate(&OptionInput::Number(-30.0)), "1");
        assert_eq!(validate(&OptionInput::Number(5000.0)), "1000");
        assert_eq!(validate(&OptionInput::Number(350.9)), "350");
    }

    #[test]
    fn invalid_values_fall_back_to_normal() {
        assert_eq!(validate(&OptionInput::Undefined), "normal");
        assert_eq!(validate(&OptionInput::string("semibold")), "normal");
        assert_eq!(validate(&OptionInput::string("")), "normal");
        assert_eq!(validate(&OptionInput::Bool(true)), "normal");
        assert_eq!(validate(&OptionInput::Null), "normal");
        assert_eq!(validate(&OptionInput::Number(f64::NAN)), "normal");
        // Só as palavras exatas passam direto.
        assert_eq!(validate(&OptionInput::string("Bold")), "normal");
    }

    #[test]
    fn default_and_suggestions() {
        assert_eq!(EditorOptionImpl::default_value(&EditorFontWeight::new()), "normal");
        assert_eq!(SUGGESTION_VALUES.len(), 11);
        assert_eq!(SUGGESTION_VALUES[0], "normal");
        assert_eq!(SUGGESTION_VALUES[10], "900");
    }
}
