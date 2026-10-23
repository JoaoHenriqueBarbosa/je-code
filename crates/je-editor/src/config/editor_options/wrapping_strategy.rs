//! Porte de `WrappingStrategy` (`editorOptions.ts`, linhas 3073 a 3107).

use je_base::dynamic::Dynamic;

use crate::config::editor_options::{
    strings_array, AccessibilitySupport, ComputedEditorOptions, EditorOption, EditorOptionImpl, EnvironmentalOptions, Opt,
};

/// Chave tipada de `wrappingStrategy`.
pub const WRAPPING_STRATEGY: Opt<WrappingStrategyValue> = Opt::new(EditorOption::WrappingStrategy);

string_enum! {
    /// `'simple' | 'advanced'`.
    pub enum WrappingStrategyValue {
        Simple => "simple",
        Advanced => "advanced",
    }
}

/// `WrappingStrategy`.
pub struct WrappingStrategy {
    schema: Dynamic,
}

impl WrappingStrategy {
    pub fn new() -> Self {
        let property = Dynamic::object([
            (
                "enumDescriptions",
                Dynamic::array([
                    Dynamic::string("Assumes that all characters are of the same width. This is a fast algorithm that works correctly for monospace fonts and certain scripts (like Latin characters) where glyphs are of equal width."),
                    Dynamic::string("Delegates wrapping points computation to the browser. This is a slow algorithm, that might cause freezes for large files, but it works correctly in all cases."),
                ]),
            ),
            ("type", Dynamic::string("string")),
            ("enum", strings_array(WrappingStrategyValue::ALLOWED)),
            ("default", Dynamic::string("simple")),
            (
                "description",
                Dynamic::string("Controls the algorithm that computes wrapping points. Note that when in accessibility mode, advanced will be used for the best experience."),
            ),
        ]);
        Self { schema: Dynamic::object([("editor.wrappingStrategy", property)]) }
    }
}

impl Default for WrappingStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for WrappingStrategy {
    type Value = WrappingStrategyValue;

    fn id(&self) -> EditorOption {
        EditorOption::WrappingStrategy
    }

    fn name(&self) -> &str {
        "wrappingStrategy"
    }

    fn default_value(&self) -> WrappingStrategyValue {
        WrappingStrategyValue::Simple
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> WrappingStrategyValue {
        WrappingStrategyValue::validate(input, WrappingStrategyValue::Simple)
    }

    fn compute(&self, _env: &EnvironmentalOptions, options: &ComputedEditorOptions, value: &WrappingStrategyValue) -> WrappingStrategyValue {
        let accessibility_support = options.get(Opt::<AccessibilitySupport>::new(EditorOption::AccessibilitySupport));
        if *accessibility_support == AccessibilitySupport::Enabled {
            // Com leitor de tela confirmado, a estratégia vira `advanced` para que os pontos de quebra
            // do editor coincidam com os do textarea.
            return WrappingStrategyValue::Advanced;
        }
        *value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::editor_options::option_support::testing::{environment, font_info, options_with, value};

    fn text(s: &str) -> Dynamic {
        Dynamic::string(s)
    }

    #[test]
    fn validate_accepts_only_the_two_values() {
        let option = WrappingStrategy::new();
        assert_eq!(option.validate(&text("advanced")), WrappingStrategyValue::Advanced);
        assert_eq!(option.validate(&text("simple")), WrappingStrategyValue::Simple);
        assert_eq!(option.validate(&text("Advanced")), WrappingStrategyValue::Simple);
        assert_eq!(option.validate(&Dynamic::Number(1.0)), WrappingStrategyValue::Simple);
        assert_eq!(option.validate(&Dynamic::Undefined), WrappingStrategyValue::Simple);
        assert_eq!(option.validate(&Dynamic::Bool(true)), WrappingStrategyValue::Simple);
        assert_eq!(option.validate(&Dynamic::array([text("advanced")])), WrappingStrategyValue::Simple);
    }

    #[test]
    fn compute_forces_advanced_only_with_a_screen_reader() {
        let option = WrappingStrategy::new();
        let env = environment(font_info(true, 14.0, "normal"));
        for (support, expected) in [
            (AccessibilitySupport::Enabled, WrappingStrategyValue::Advanced),
            (AccessibilitySupport::Disabled, WrappingStrategyValue::Simple),
            (AccessibilitySupport::Unknown, WrappingStrategyValue::Simple),
        ] {
            let options = options_with(vec![(EditorOption::AccessibilitySupport, value(support))]);
            assert_eq!(option.compute(&env, &options, &WrappingStrategyValue::Simple), expected);
        }
        let options = options_with(vec![(EditorOption::AccessibilitySupport, value(AccessibilitySupport::Disabled))]);
        assert_eq!(option.compute(&env, &options, &WrappingStrategyValue::Advanced), WrappingStrategyValue::Advanced);
    }

    #[test]
    fn schema_keeps_the_original_keys_and_order() {
        let option = WrappingStrategy::new();
        let schema = option.schema().expect("sem esquema");
        assert_eq!(schema.own_keys(), vec!["editor.wrappingStrategy"]);
        let property = schema.get_str("editor.wrappingStrategy");
        assert_eq!(property.own_keys(), vec!["enumDescriptions", "type", "enum", "default", "description"]);
        assert_eq!(property.get_str("enum").get_str("length"), Dynamic::Number(2.0));
        assert_eq!(property.get_str("default"), text("simple"));
        assert_eq!(property.get_str("enumDescriptions").get_str("length"), Dynamic::Number(2.0));
    }
}
