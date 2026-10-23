//! Porte de `BracketPairColorization`, `IBracketPairColorizationOptions` e
//! `InternalBracketPairColorizationOptions` (`editorOptions.ts`, linhas 4801 a 4855).
//!
//! Os padrões vêm de `EDITOR_MODEL_DEFAULTS.bracketPairColorizationOptions` ([`EDITOR_MODEL_DEFAULTS`]).

use je_base::dynamic::{Dynamic, Truthy};

use crate::config::editor_options::{boolean, EditorOption, EditorOptionImpl, Opt};
use crate::text_model_defaults::EDITOR_MODEL_DEFAULTS;


/// `InternalBracketPairColorizationOptions` (`Readonly<Required<IBracketPairColorizationOptions>>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InternalBracketPairColorizationOptions {
    /// Liga ou desliga a colorização de pares de colchetes.
    pub enabled: bool,
    /// Usa um conjunto de cores independente por tipo de colchete.
    pub independent_color_pool_per_bracket_type: bool,
}

/// Chave tipada da opção `bracketPairColorization`.
pub const BRACKET_PAIR_COLORIZATION: Opt<InternalBracketPairColorizationOptions> = Opt::new(EditorOption::BracketPairColorization);

/// `BracketPairColorization`.
pub struct BracketPairColorization {
    default_value: InternalBracketPairColorizationOptions,
    schema: Dynamic,
}

impl BracketPairColorization {
    pub fn new() -> Self {
        let defaults = InternalBracketPairColorizationOptions {
            enabled: EDITOR_MODEL_DEFAULTS.bracket_pair_colorization_options.enabled,
            independent_color_pool_per_bracket_type: EDITOR_MODEL_DEFAULTS.bracket_pair_colorization_options.independent_color_pool_per_bracket_type,
        };
        let schema = Dynamic::object([
            (
                "editor.bracketPairColorization.enabled",
                Dynamic::object([
                    ("type", Dynamic::string("boolean")),
                    ("default", Dynamic::Bool(defaults.enabled)),
                    (
                        "markdownDescription",
                        Dynamic::string(
                            "Controls whether bracket pair colorization is enabled or not. Use `#workbench.colorCustomizations#` to override the bracket highlight colors.",
                        ),
                    ),
                ]),
            ),
            (
                "editor.bracketPairColorization.independentColorPoolPerBracketType",
                Dynamic::object([
                    ("type", Dynamic::string("boolean")),
                    ("default", Dynamic::Bool(defaults.independent_color_pool_per_bracket_type)),
                    ("description", Dynamic::string("Controls whether each bracket type has its own independent color pool.")),
                ]),
            ),
        ]);
        Self { default_value: defaults, schema }
    }
}

impl Default for BracketPairColorization {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for BracketPairColorization {
    type Value = InternalBracketPairColorizationOptions;

    fn id(&self) -> EditorOption {
        EditorOption::BracketPairColorization
    }

    fn name(&self) -> &str {
        "bracketPairColorization"
    }

    fn default_value(&self) -> InternalBracketPairColorizationOptions {
        self.default_value
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> InternalBracketPairColorizationOptions {
        if !input.is_truthy() || input.type_of() != "object" {
            return self.default_value;
        }
        let defaults = &self.default_value;
        InternalBracketPairColorizationOptions {
            enabled: boolean(&input.get_str("enabled"), defaults.enabled),
            independent_color_pool_per_bracket_type: boolean(
                &input.get_str("independentColorPoolPerBracketType"),
                defaults.independent_color_pool_per_bracket_type,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Dynamic {
        Dynamic::string(value)
    }

    fn validate(entries: Vec<(&str, Dynamic)>) -> InternalBracketPairColorizationOptions {
        EditorOptionImpl::validate(&BracketPairColorization::new(), &Dynamic::object(entries))
    }

    fn defaults() -> InternalBracketPairColorizationOptions {
        EditorOptionImpl::default_value(&BracketPairColorization::new())
    }

    #[test]
    fn defaults_match_the_model_defaults() {
        let defaults = defaults();
        assert!(defaults.enabled);
        assert!(!defaults.independent_color_pool_per_bracket_type);
        assert_eq!(validate(vec![]), defaults);
        let option = BracketPairColorization::new();
        for input in [Dynamic::Undefined, Dynamic::Null, Dynamic::Bool(true), Dynamic::Number(1.0), text("x"), text("")] {
            assert_eq!(EditorOptionImpl::validate(&option, &input), defaults);
        }
    }

    #[test]
    fn fields_use_the_boolean_helper() {
        let value = validate(vec![("enabled", Dynamic::Bool(false)), ("independentColorPoolPerBracketType", Dynamic::Bool(true))]);
        assert!(!value.enabled && value.independent_color_pool_per_bracket_type);
        let value = validate(vec![("enabled", text("false")), ("independentColorPoolPerBracketType", text("yes"))]);
        assert!(!value.enabled && value.independent_color_pool_per_bracket_type);
        let value = validate(vec![("enabled", Dynamic::Null), ("independentColorPoolPerBracketType", Dynamic::Number(0.0))]);
        assert!(!value.enabled && !value.independent_color_pool_per_bracket_type);
        // `undefined` mantém o padrão de cada campo.
        let value = validate(vec![("enabled", Dynamic::Undefined)]);
        assert_eq!(value, defaults());
    }

    #[test]
    fn schema_keeps_the_original_keys_and_order() {
        let option = BracketPairColorization::new();
        let schema = EditorOptionImpl::schema(&option).expect("sem esquema");
        assert_eq!(
            schema.own_keys(),
            vec!["editor.bracketPairColorization.enabled", "editor.bracketPairColorization.independentColorPoolPerBracketType"]
        );
        let enabled = schema.get_str("editor.bracketPairColorization.enabled");
        assert_eq!(enabled.own_keys(), vec!["type", "default", "markdownDescription"]);
        assert_eq!(enabled.get_str("default"), Dynamic::Bool(true));
        let pool = schema.get_str("editor.bracketPairColorization.independentColorPoolPerBracketType");
        assert_eq!(pool.own_keys(), vec!["type", "default", "description"]);
        assert_eq!(pool.get_str("default"), Dynamic::Bool(false));
    }
}
