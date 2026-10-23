//! Porte de `WrappingStrategy` (`editorOptions.ts`, linhas 3073 a 3107).

use crate::config::editor_options::{
    string_set, AccessibilitySupport, ComputedEditorOptions, ConfigurationPropertySchema, EditorOption, EditorOptionImpl, EnvironmentalOptions, Opt,
    OptionInput, OptionSchema, SchemaValue,
};

/// Chave tipada de `wrappingStrategy`.
pub const WRAPPING_STRATEGY: Opt<WrappingStrategyValue> = Opt::new(EditorOption::WrappingStrategy);

/// `'simple' | 'advanced'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WrappingStrategyValue {
    Simple,
    Advanced,
}

impl WrappingStrategyValue {
    pub const fn as_str(self) -> &'static str {
        match self {
            WrappingStrategyValue::Simple => "simple",
            WrappingStrategyValue::Advanced => "advanced",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "simple" => Some(WrappingStrategyValue::Simple),
            "advanced" => Some(WrappingStrategyValue::Advanced),
            _ => None,
        }
    }
}

/// `WrappingStrategy`. `enumDescriptions` do esquema ainda não existe em
/// [`ConfigurationPropertySchema`] (`// TODO(config)`).
pub struct WrappingStrategy {
    schema: OptionSchema,
}

impl WrappingStrategy {
    pub fn new() -> Self {
        let property = ConfigurationPropertySchema {
            type_: Some("string".to_string()),
            enum_values: Some(vec!["simple".to_string(), "advanced".to_string()]),
            default: Some(SchemaValue::String("simple".to_string())),
            description: Some(
                "Controls the algorithm that computes wrapping points. Note that when in accessibility mode, advanced will be used for the best experience."
                    .to_string(),
            ),
            ..Default::default()
        };
        Self { schema: OptionSchema::Map(vec![("editor.wrappingStrategy".to_string(), property)]) }
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

    fn schema(&self) -> Option<&OptionSchema> {
        Some(&self.schema)
    }

    fn validate(&self, input: &OptionInput) -> WrappingStrategyValue {
        let value = string_set(input, "simple", &["simple", "advanced"], None);
        WrappingStrategyValue::from_str(&value).unwrap_or(WrappingStrategyValue::Simple)
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
    use crate::config::editor_options::option_support::{test_env, test_options};

    #[test]
    fn validate_accepts_only_the_two_values() {
        let option = WrappingStrategy::new();
        assert_eq!(option.validate(&OptionInput::string("advanced")), WrappingStrategyValue::Advanced);
        assert_eq!(option.validate(&OptionInput::string("simple")), WrappingStrategyValue::Simple);
        assert_eq!(option.validate(&OptionInput::string("Advanced")), WrappingStrategyValue::Simple);
        assert_eq!(option.validate(&OptionInput::Number(1.0)), WrappingStrategyValue::Simple);
        assert_eq!(option.validate(&OptionInput::Undefined), WrappingStrategyValue::Simple);
        assert_eq!(option.validate(&OptionInput::Bool(true)), WrappingStrategyValue::Simple);
    }

    #[test]
    fn compute_forces_advanced_only_with_a_screen_reader() {
        let option = WrappingStrategy::new();
        let env = test_env(1.0);
        for (support, expected) in [
            (AccessibilitySupport::Enabled, WrappingStrategyValue::Advanced),
            (AccessibilitySupport::Disabled, WrappingStrategyValue::Simple),
            (AccessibilitySupport::Unknown, WrappingStrategyValue::Simple),
        ] {
            let options = test_options(support);
            assert_eq!(option.compute(&env, &options, &WrappingStrategyValue::Simple), expected);
        }
        let options = test_options(AccessibilitySupport::Disabled);
        assert_eq!(option.compute(&env, &options, &WrappingStrategyValue::Advanced), WrappingStrategyValue::Advanced);
    }

    #[test]
    fn schema_lists_the_enum() {
        let option = WrappingStrategy::new();
        let Some(OptionSchema::Map(entries)) = option.schema() else { panic!("sem esquema") };
        assert_eq!(entries[0].0, "editor.wrappingStrategy");
        assert_eq!(entries[0].1.enum_values.as_deref(), Some(&["simple".to_string(), "advanced".to_string()][..]));
        assert_eq!(entries[0].1.default, Some(SchemaValue::String("simple".to_string())));
    }
}
